//! The reference bytecode interpreter.

use crate::bytecode::{Capture, Const, Lang, Module, Op, Program, Proto, RecordDesc};
use crate::natives::{self, Io, NativeError, NativeId};
use crate::value::*;
use std::cell::RefCell;
use std::collections::HashMap;
use std::io::Write;
use std::rc::Rc;

/// Maximum call depth before reporting a stack overflow.
pub const MAX_FRAMES: usize = 200_000;
/// Largest array a program may allocate in one go.
pub const MAX_ARRAY: i64 = 1 << 26;

enum RtConst {
    Val(Value),
    Record(Rc<RecordDesc>),
}

struct Frame {
    closure: Rc<Closure>,
    pc: usize,
    /// Stack index of local slot 0.
    base: usize,
    /// Stack height to restore (before pushing the result) on return.
    ret: usize,
}

#[derive(Debug)]
pub struct RuntimeError {
    pub message: String,
    pub lang: Lang,
    pub line: u32,
    /// Innermost first: (language, function name, line).
    pub trace: Vec<(Lang, String, u32)>,
}

impl RuntimeError {
    /// Render the error the way the language of the failing code would.
    pub fn render(&self) -> String {
        match self.lang {
            Lang::Lox => format!("{}\n[line {}]", self.message, self.line),
            Lang::Monkey => format!("ERROR: {}", self.message),
            Lang::Tiger => format!("runtime error: {}\n[line {}]", self.message, self.line),
        }
    }
}

pub enum Outcome {
    Finished,
    Exit(i32),
}

pub struct Vm {
    protos: Vec<Rc<Proto>>,
    consts: Vec<RtConst>,
    modules: Vec<Module>,
    globals: HashMap<Rc<str>, Value>,
    stack: Vec<Value>,
    frames: Vec<Frame>,
    pub io: Io,
}

/// Internal error signal: a message, or a request to exit.
enum Trap {
    Error(String),
    Exit(i32),
}

impl From<String> for Trap {
    fn from(s: String) -> Self {
        Trap::Error(s)
    }
}

type R<T> = Result<T, Trap>;

fn err<T>(s: impl Into<String>) -> R<T> {
    Err(Trap::Error(s.into()))
}

enum Nums {
    I(i64, i64),
    F(f64, f64),
}

fn nums(a: &Value, b: &Value) -> Option<Nums> {
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => Some(Nums::I(*x, *y)),
        (Value::Int(x), Value::Float(y)) => Some(Nums::F(*x as f64, *y)),
        (Value::Float(x), Value::Int(y)) => Some(Nums::F(*x, *y as f64)),
        (Value::Float(x), Value::Float(y)) => Some(Nums::F(*x, *y)),
        _ => None,
    }
}

fn op_symbol(op: Op) -> &'static str {
    match op {
        Op::Add => "+",
        Op::Sub => "-",
        Op::Mul => "*",
        Op::Div | Op::FDiv => "/",
        Op::Lt | Op::SLt => "<",
        Op::Le | Op::SLe => "<=",
        Op::Gt | Op::SGt => ">",
        Op::Ge | Op::SGe => ">=",
        _ => "?",
    }
}

impl Vm {
    /// Build a VM for a program.  The program must already have passed
    /// [`Program::verify`].
    pub fn new(prog: Program, io: Io) -> Vm {
        let consts = prog
            .consts
            .into_iter()
            .map(|c| match c {
                Const::Int(i) => RtConst::Val(Value::Int(i)),
                Const::Float(f) => RtConst::Val(Value::Float(f)),
                Const::Str(s) => RtConst::Val(Value::str(&s)),
                Const::Record(d) => RtConst::Record(Rc::new(d)),
            })
            .collect();
        let mut vm = Vm {
            protos: prog.protos.into_iter().map(Rc::new).collect(),
            consts,
            modules: prog.modules,
            globals: HashMap::new(),
            stack: Vec::with_capacity(1024),
            frames: Vec::new(),
            io,
        };
        // Lox's one native lives in the global namespace, as in jlox.
        vm.globals.insert(Rc::from("clock"), Value::Native(NativeId::Clock as u32));
        vm
    }

    /// Run every module in order.
    pub fn run(&mut self) -> Result<Outcome, RuntimeError> {
        for i in 0..self.modules.len() {
            let entry = self.protos[self.modules[i].entry as usize].clone();
            let closure = Rc::new(Closure { proto: entry, upvals: Box::new([]) });
            self.stack.clear();
            self.stack.push(Value::Closure(closure.clone()));
            let outcome = self.call_closure(closure, 0, 0, false).and_then(|_| self.execute());
            match outcome {
                Ok(()) => {}
                Err(Trap::Exit(code)) => {
                    let _ = self.io.out.flush();
                    return Ok(Outcome::Exit(code));
                }
                Err(Trap::Error(message)) => {
                    let _ = self.io.out.flush();
                    let trace: Vec<(Lang, String, u32)> = self
                        .frames
                        .iter()
                        .rev()
                        .map(|f| {
                            let p = &f.closure.proto;
                            (p.lang, p.name.clone(), p.lines.get(f.pc.saturating_sub(1)).copied().unwrap_or(0))
                        })
                        .collect();
                    let (lang, line) = trace.first().map(|t| (t.0, t.2)).unwrap_or((Lang::Lox, 0));
                    self.frames.clear();
                    return Err(RuntimeError { message, lang, line, trace });
                }
            }
        }
        let _ = self.io.out.flush();
        Ok(Outcome::Finished)
    }

    fn lang(&self) -> Lang {
        self.frames.last().map_or(Lang::Lox, |f| f.closure.proto.lang)
    }

    fn pop(&mut self) -> Value {
        // The verifier guarantees the operand stack never underflows.
        self.stack.pop().expect("verified stack height")
    }

    fn peek(&self) -> &Value {
        self.stack.last().expect("verified stack height")
    }

    fn name(&self, k: u32) -> Rc<str> {
        match &self.consts[k as usize] {
            RtConst::Val(Value::Str(s)) => s.clone(),
            _ => unreachable!("verified name constant"),
        }
    }

    fn slot(&mut self, base: usize, s: u32) -> &mut Value {
        &mut self.stack[base + s as usize]
    }

    fn cell_at(&self, base: usize, s: u32) -> R<Cell> {
        match &self.stack[base + s as usize] {
            Value::Cell(c) => Ok(c.clone()),
            _ => err("internal error: local slot is not a cell"),
        }
    }

    fn call_closure(&mut self, c: Rc<Closure>, callee: usize, argc: usize, receiver: bool) -> R<()> {
        let p = &c.proto;
        if argc != p.arity as usize {
            return err(match p.lang {
                Lang::Lox => format!("Expected {} arguments but got {}.", p.arity, argc),
                Lang::Monkey => format!("wrong number of arguments: want={}, got={}", p.arity, argc),
                Lang::Tiger => format!("function '{}' expects {} arguments but got {}", p.name, p.arity, argc),
            });
        }
        if self.frames.len() >= MAX_FRAMES {
            return err(match self.lang() {
                Lang::Monkey => "stack overflow",
                _ => "Stack overflow.",
            });
        }
        let base = if receiver { callee } else { callee + 1 };
        let top = base + p.nlocals as usize;
        if self.stack.len() < top {
            self.stack.resize(top, Value::Undef);
        }
        self.frames.push(Frame { closure: c, pc: 0, base, ret: callee });
        Ok(())
    }

    fn call_value(&mut self, argc: usize) -> R<()> {
        let callee = self.stack.len() - argc - 1;
        match self.stack[callee].clone() {
            Value::Closure(c) => self.call_closure(c, callee, argc, false),
            Value::Bound(b) => {
                self.stack[callee] = b.receiver.clone();
                self.call_closure(b.method.clone(), callee, argc, true)
            }
            Value::Class(class) => {
                let inst =
                    Value::Instance(Rc::new(Instance { class: class.clone(), fields: RefCell::new(HashMap::new()) }));
                let init = class.methods.borrow().get("init").cloned();
                match init {
                    Some(init) => {
                        self.stack[callee] = inst;
                        self.call_closure(init, callee, argc, true)
                    }
                    None if argc != 0 => err(format!("Expected 0 arguments but got {argc}.")),
                    None => {
                        self.stack.truncate(callee);
                        self.stack.push(inst);
                        Ok(())
                    }
                }
            }
            Value::Native(id) => {
                let id = NativeId::from_u32(id).expect("verified native id");
                let def = id.def();
                if let Some(n) = def.arity {
                    if n as usize != argc {
                        return err(match def.lang {
                            Lang::Lox => format!("Expected {n} arguments but got {argc}."),
                            Lang::Monkey => format!("wrong number of arguments. got={argc}, want={n}"),
                            Lang::Tiger => format!("{} expects {n} arguments but got {argc}", def.name),
                        });
                    }
                }
                let args = self.stack.split_off(callee + 1);
                self.stack.pop();
                match natives::call(id, &args, &mut self.io) {
                    Ok(v) => {
                        self.stack.push(v);
                        Ok(())
                    }
                    Err(NativeError::Msg(m)) => err(m),
                    Err(NativeError::Exit(c)) => Err(Trap::Exit(c)),
                }
            }
            other => err(match self.lang() {
                Lang::Lox => "Can only call functions and classes.".to_string(),
                Lang::Monkey => format!("not a function: {}", other.type_name(Lang::Monkey)),
                Lang::Tiger => format!("cannot call a value of type {}", other.type_name(Lang::Tiger)),
            }),
        }
    }

    fn binary_error(&self, op: Op, a: &Value, b: &Value) -> Trap {
        let lang = self.lang();
        Trap::Error(match lang {
            Lang::Lox => match op {
                Op::Add => "Operands must be two numbers or two strings.".into(),
                _ => "Operands must be numbers.".into(),
            },
            Lang::Monkey => {
                let (ta, tb) = (a.type_name(lang), b.type_name(lang));
                if ta != tb {
                    format!("type mismatch: {ta} {} {tb}", op_symbol(op))
                } else {
                    format!("unknown operator: {ta} {} {tb}", op_symbol(op))
                }
            }
            Lang::Tiger => format!(
                "operator '{}' cannot be applied to {} and {}",
                op_symbol(op),
                a.type_name(lang),
                b.type_name(lang)
            ),
        })
    }

    fn arith(&mut self, op: Op) -> R<()> {
        let b = self.pop();
        let a = self.pop();
        let r = match (op, nums(&a, &b)) {
            (Op::Add, Some(Nums::I(x, y))) => Value::Int(x.wrapping_add(y)),
            (Op::Sub, Some(Nums::I(x, y))) => Value::Int(x.wrapping_sub(y)),
            (Op::Mul, Some(Nums::I(x, y))) => Value::Int(x.wrapping_mul(y)),
            (Op::Div, Some(Nums::I(x, y))) => {
                if y == 0 {
                    return err("division by zero");
                }
                Value::Int(x.wrapping_div(y))
            }
            (Op::FDiv, Some(Nums::I(x, y))) => Value::Float(x as f64 / y as f64),
            (Op::Add, Some(Nums::F(x, y))) => Value::Float(x + y),
            (Op::Sub, Some(Nums::F(x, y))) => Value::Float(x - y),
            (Op::Mul, Some(Nums::F(x, y))) => Value::Float(x * y),
            (Op::Div | Op::FDiv, Some(Nums::F(x, y))) => Value::Float(x / y),
            (Op::Add, None) => match (&a, &b) {
                (Value::Str(x), Value::Str(y)) => {
                    let mut s = String::with_capacity(x.len() + y.len());
                    s.push_str(x);
                    s.push_str(y);
                    Value::str(&s)
                }
                _ => return Err(self.binary_error(op, &a, &b)),
            },
            _ => return Err(self.binary_error(op, &a, &b)),
        };
        self.stack.push(r);
        Ok(())
    }

    fn compare(&mut self, op: Op) -> R<()> {
        let b = self.pop();
        let a = self.pop();
        let ord = match op {
            Op::Lt | Op::Le | Op::Gt | Op::Ge => match nums(&a, &b) {
                Some(Nums::I(x, y)) => x.partial_cmp(&y),
                Some(Nums::F(x, y)) => x.partial_cmp(&y),
                None => return Err(self.binary_error(op, &a, &b)),
            },
            _ => match (&a, &b) {
                (Value::Str(x), Value::Str(y)) => x.as_bytes().partial_cmp(y.as_bytes()),
                _ => return Err(self.binary_error(op, &a, &b)),
            },
        };
        use std::cmp::Ordering::*;
        let r = match (op, ord) {
            (_, None) => false, // NaN
            (Op::Lt | Op::SLt, Some(o)) => o == Less,
            (Op::Le | Op::SLe, Some(o)) => o != Greater,
            (Op::Gt | Op::SGt, Some(o)) => o == Greater,
            (_, Some(o)) => o != Less,
        };
        self.stack.push(Value::Bool(r));
        Ok(())
    }

    fn undefined_global(&self, name: &str) -> Trap {
        Trap::Error(match self.lang() {
            Lang::Lox => format!("Undefined variable '{name}'."),
            Lang::Monkey => format!("identifier not found: {name}"),
            Lang::Tiger => format!("undefined external '{name}'"),
        })
    }

    fn execute(&mut self) -> R<()> {
        loop {
            let frame = self.frames.last_mut().expect("active frame");
            let base = frame.base;
            let op = frame.closure.proto.code[frame.pc];
            frame.pc += 1;
            match op {
                Op::Const(k) => match &self.consts[k as usize] {
                    RtConst::Val(v) => {
                        let v = v.clone();
                        self.stack.push(v)
                    }
                    RtConst::Record(_) => unreachable!("verified value constant"),
                },
                Op::Nil => self.stack.push(Value::Nil),
                Op::True => self.stack.push(Value::Bool(true)),
                Op::False => self.stack.push(Value::Bool(false)),
                Op::Pop => {
                    self.pop();
                }
                Op::Dup => {
                    let v = self.peek().clone();
                    self.stack.push(v)
                }
                Op::GetLocal(s) => {
                    let v = self.slot(base, s).clone();
                    self.stack.push(v)
                }
                Op::SetLocal(s) => {
                    let v = self.pop();
                    *self.slot(base, s) = v
                }
                Op::InitCell(s) => {
                    let v = self.pop();
                    *self.slot(base, s) = Value::Cell(Rc::new(RefCell::new(v)))
                }
                Op::GetCell(s) => {
                    let v = self.cell_at(base, s)?.borrow().clone();
                    self.stack.push(v)
                }
                Op::SetCell(s) => {
                    let v = self.pop();
                    *self.cell_at(base, s)?.borrow_mut() = v
                }
                Op::GetUpval(u) => {
                    let frame = self.frames.last().unwrap();
                    let v = frame.closure.upvals[u as usize].borrow().clone();
                    self.stack.push(v)
                }
                Op::SetUpval(u) => {
                    let v = self.pop();
                    let frame = self.frames.last().unwrap();
                    *frame.closure.upvals[u as usize].borrow_mut() = v
                }
                Op::DefineGlobal(k) => {
                    let v = self.pop();
                    self.globals.insert(self.name(k), v);
                }
                Op::GetGlobal(k) => {
                    let name = self.name(k);
                    match self.globals.get(&name) {
                        Some(v) => {
                            let v = v.clone();
                            self.stack.push(v)
                        }
                        None => return Err(self.undefined_global(&name)),
                    }
                }
                Op::GetGlobalOpt(k) => {
                    let v = self.globals.get(&self.name(k)).cloned().unwrap_or(Value::Undef);
                    self.stack.push(v)
                }
                Op::SetGlobal(k) => {
                    let name = self.name(k);
                    let v = self.pop();
                    match self.globals.get_mut(&name) {
                        Some(slot) => *slot = v,
                        None => return Err(self.undefined_global(&name)),
                    }
                }
                Op::Add | Op::Sub | Op::Mul | Op::Div | Op::FDiv => self.arith(op)?,
                Op::Lt | Op::Le | Op::Gt | Op::Ge | Op::SLt | Op::SLe | Op::SGt | Op::SGe => self.compare(op)?,
                Op::Eq | Op::Ne => {
                    let b = self.pop();
                    let a = self.pop();
                    self.stack.push(Value::Bool(a.equals(&b) == (op == Op::Eq)))
                }
                Op::Neg => {
                    let v = self.pop();
                    let r = match v {
                        Value::Int(i) => Value::Int(i.wrapping_neg()),
                        Value::Float(f) => Value::Float(-f),
                        other => {
                            return err(match self.lang() {
                                Lang::Lox => "Operand must be a number.".to_string(),
                                Lang::Monkey => format!("unknown operator: -{}", other.type_name(Lang::Monkey)),
                                Lang::Tiger => {
                                    format!("cannot negate a value of type {}", other.type_name(Lang::Tiger))
                                }
                            })
                        }
                    };
                    self.stack.push(r)
                }
                Op::Not => {
                    let v = self.pop();
                    self.stack.push(Value::Bool(!v.truthy()))
                }
                Op::BoolToInt => {
                    let v = self.pop();
                    self.stack.push(match v {
                        Value::Bool(b) => Value::Int(b as i64),
                        other => other,
                    })
                }
                Op::CheckInt => {
                    let v = self.pop();
                    self.stack.push(match v {
                        Value::Int(_) => v,
                        Value::Float(f) if f.fract() == 0.0 && f.abs() < 9.3e18 => Value::Int(f as i64),
                        Value::Bool(b) => Value::Int(b as i64),
                        other => return err(format!("expected an int but got {}", other.type_name(Lang::Tiger))),
                    })
                }
                Op::CheckStr => {
                    if !matches!(self.peek(), Value::Str(_)) {
                        return err(format!("expected a string but got {}", self.peek().type_name(Lang::Tiger)));
                    }
                }
                Op::Jump(t) => self.frames.last_mut().unwrap().pc = t as usize,
                Op::JumpIfFalse(t) => {
                    if !self.pop().truthy() {
                        self.frames.last_mut().unwrap().pc = t as usize
                    }
                }
                Op::JumpIfTrue(t) => {
                    if self.pop().truthy() {
                        self.frames.last_mut().unwrap().pc = t as usize
                    }
                }
                Op::JumpIfZero(t) => {
                    let zero = match self.pop() {
                        Value::Int(i) => i == 0,
                        Value::Float(f) => f == 0.0,
                        Value::Bool(b) => !b,
                        other => return err(format!("condition must be an int, got {}", other.type_name(Lang::Tiger))),
                    };
                    if zero {
                        self.frames.last_mut().unwrap().pc = t as usize
                    }
                }
                Op::JumpIfNotUndef(t) => {
                    if !matches!(self.peek(), Value::Undef) {
                        self.frames.last_mut().unwrap().pc = t as usize
                    }
                }
                Op::Call(n) => self.call_value(n as usize)?,
                Op::Return => {
                    let result = self.pop();
                    let frame = self.frames.pop().unwrap();
                    self.stack.truncate(frame.ret);
                    if self.frames.is_empty() {
                        return Ok(());
                    }
                    self.stack.push(result)
                }
                Op::Closure(p) => {
                    let proto = self.protos[p as usize].clone();
                    let frame = self.frames.last().unwrap();
                    let mut upvals = Vec::with_capacity(proto.captures.len());
                    for c in &proto.captures {
                        upvals.push(match *c {
                            Capture::Local(s) => self.cell_at(base, s)?,
                            Capture::Upval(u) => frame.closure.upvals[u as usize].clone(),
                        });
                    }
                    self.stack.push(Value::Closure(Rc::new(Closure { proto, upvals: upvals.into_boxed_slice() })))
                }
                Op::Native(id) => self.stack.push(Value::Native(id)),
                Op::Fail(k) => return err(self.name(k).to_string()),
                Op::Print => {
                    let v = self.pop();
                    writeln!(self.io.out, "{}", lox_str(&v)).map_err(|e| format!("I/O error: {e}"))?
                }
                Op::Class(k) => {
                    let class = Class { name: self.name(k), methods: RefCell::new(HashMap::new()) };
                    self.stack.push(Value::Class(Rc::new(class)))
                }
                Op::Inherit => {
                    let class = self.pop();
                    let sup = self.pop();
                    match (&sup, &class) {
                        (Value::Class(s), Value::Class(c)) => {
                            let inherited = s.methods.borrow().clone();
                            c.methods.borrow_mut().extend(inherited);
                        }
                        _ => return err("Superclass must be a class."),
                    }
                    self.stack.push(class)
                }
                Op::Method(k) => {
                    let m = self.pop();
                    match (self.peek(), m) {
                        (Value::Class(c), Value::Closure(m)) => {
                            c.methods.borrow_mut().insert(self.name(k), m);
                        }
                        _ => return err("internal error: Method needs a class and a closure"),
                    }
                }
                Op::GetProp(k) => {
                    let obj = self.pop();
                    let name = self.name(k);
                    let v = match &obj {
                        Value::Instance(i) => {
                            let field = i.fields.borrow().get(&name).cloned();
                            match field {
                                Some(v) => v,
                                None => match i.class.methods.borrow().get(&name) {
                                    Some(m) => {
                                        Value::Bound(Rc::new(Bound { receiver: obj.clone(), method: m.clone() }))
                                    }
                                    None => return err(format!("Undefined property '{name}'.")),
                                },
                            }
                        }
                        Value::Record(r) => match r.desc.fields.iter().position(|f| **f == *name) {
                            Some(i) => r.fields.borrow()[i].clone(),
                            None => return err(format!("Undefined property '{name}'.")),
                        },
                        _ => return err("Only instances have properties."),
                    };
                    self.stack.push(v)
                }
                Op::SetProp(k) => {
                    let v = self.pop();
                    let obj = self.pop();
                    let name = self.name(k);
                    match &obj {
                        Value::Instance(i) => {
                            i.fields.borrow_mut().insert(name, v.clone());
                        }
                        Value::Record(r) => match r.desc.fields.iter().position(|f| **f == *name) {
                            Some(i) => r.fields.borrow_mut()[i] = v.clone(),
                            None => return err(format!("Undefined property '{name}'.")),
                        },
                        _ => return err("Only instances have fields."),
                    }
                    self.stack.push(v)
                }
                Op::GetSuper(k) => {
                    let sup = self.pop();
                    let this = self.pop();
                    let name = self.name(k);
                    let m = match &sup {
                        Value::Class(c) => c.methods.borrow().get(&name).cloned(),
                        _ => None,
                    };
                    match m {
                        Some(m) => self.stack.push(Value::Bound(Rc::new(Bound { receiver: this, method: m }))),
                        None => return err(format!("Undefined property '{name}'.")),
                    }
                }
                Op::NewRecord(k) => {
                    let desc = match &self.consts[k as usize] {
                        RtConst::Record(d) => d.clone(),
                        _ => unreachable!("verified record constant"),
                    };
                    let fields = self.stack.split_off(self.stack.len() - desc.fields.len());
                    self.stack.push(Value::Record(Rc::new(RecordObj { desc, fields: RefCell::new(fields) })))
                }
                Op::GetField(i) => {
                    let r = self.pop();
                    let v = match &r {
                        Value::Record(r) => r.fields.borrow().get(i as usize).cloned(),
                        Value::Nil => return err("field access on a nil record"),
                        _ => None,
                    };
                    match v {
                        Some(v) => self.stack.push(v),
                        None => return err("internal error: bad record field access"),
                    }
                }
                Op::SetField(i) => {
                    let v = self.pop();
                    let r = self.pop();
                    match &r {
                        Value::Record(r) if (i as usize) < r.desc.fields.len() => r.fields.borrow_mut()[i as usize] = v,
                        Value::Nil => return err("field assignment on a nil record"),
                        _ => return err("internal error: bad record field assignment"),
                    }
                }
                Op::NewArray => {
                    let init = self.pop();
                    let n = match self.pop() {
                        Value::Int(n) => n,
                        _ => return err("array size must be an int"),
                    };
                    if !(0..=MAX_ARRAY).contains(&n) {
                        return err(format!("invalid array size {n}"));
                    }
                    self.stack.push(Value::Array(Rc::new(RefCell::new(vec![init; n as usize]))))
                }
                Op::ArrayGet => {
                    let i = self.pop();
                    let a = self.pop();
                    let v = match (&a, &i) {
                        (Value::Array(a), Value::Int(i)) => {
                            let a = a.borrow();
                            match usize::try_from(*i).ok().and_then(|i| a.get(i)) {
                                Some(v) => v.clone(),
                                None => return err(format!("array index {i} out of bounds for size {}", a.len())),
                            }
                        }
                        _ => return err("subscript needs an array and an int"),
                    };
                    self.stack.push(v)
                }
                Op::ArraySet => {
                    let v = self.pop();
                    let i = self.pop();
                    let a = self.pop();
                    match (&a, &i) {
                        (Value::Array(a), Value::Int(i)) => {
                            let mut a = a.borrow_mut();
                            let len = a.len();
                            match usize::try_from(*i).ok().and_then(|i| a.get_mut(i)) {
                                Some(slot) => *slot = v,
                                None => return err(format!("array index {i} out of bounds for size {len}")),
                            }
                        }
                        _ => return err("subscript needs an array and an int"),
                    }
                }
                Op::MakeArray(n) => {
                    let items = self.stack.split_off(self.stack.len() - n as usize);
                    self.stack.push(Value::Array(Rc::new(RefCell::new(items))))
                }
                Op::MakeHash(n) => {
                    let items = self.stack.split_off(self.stack.len() - 2 * n as usize);
                    let mut h = HashObj::default();
                    let mut it = items.into_iter();
                    while let (Some(k), Some(v)) = (it.next(), it.next()) {
                        match k.hash_key() {
                            Some(key) => h.insert(key, k, v),
                            None => return err(format!("unusable as hash key: {}", k.type_name(Lang::Monkey))),
                        }
                    }
                    self.stack.push(Value::Hash(Rc::new(h)))
                }
                Op::Index => {
                    let i = self.pop();
                    let obj = self.pop();
                    let v = match (&obj, &i) {
                        (Value::Array(a), Value::Int(i)) => {
                            usize::try_from(*i).ok().and_then(|i| a.borrow().get(i).cloned()).unwrap_or(Value::Nil)
                        }
                        (Value::Hash(h), _) => match i.hash_key() {
                            Some(key) => h.get(&key).cloned().unwrap_or(Value::Nil),
                            None => return err(format!("unusable as hash key: {}", i.type_name(Lang::Monkey))),
                        },
                        _ => return err(format!("index operator not supported: {}", obj.type_name(Lang::Monkey))),
                    };
                    self.stack.push(v)
                }
            }
        }
    }
}
