//! The common bytecode: instruction set, program container, binary
//! serialisation, linking, verification and disassembly.
//!
//! See `BYTECODE.md` for the prose specification of the format and of the
//! semantics of every instruction.

use std::fmt::{self, Write as _};

/// Magic bytes at the start of every bytecode file.
pub const MAGIC: &[u8; 4] = b"MNGR";
/// Format version; bumped on any incompatible change.
pub const VERSION: u16 = 1;

/// The source language a function was compiled from.  The VM consults this
/// only to phrase runtime error messages and to pick how values are printed
/// in diagnostics; instruction semantics never depend on it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang {
    Lox = 0,
    Monkey = 1,
    Tiger = 2,
}

impl Lang {
    pub fn from_u8(b: u8) -> Option<Lang> {
        match b {
            0 => Some(Lang::Lox),
            1 => Some(Lang::Monkey),
            2 => Some(Lang::Tiger),
            _ => None,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Lang::Lox => "lox",
            Lang::Monkey => "monkey",
            Lang::Tiger => "tiger",
        }
    }
}

/// One instruction.  Operands are indices: `k` into the constant pool, `s`
/// into the frame's local slots, `u` into the closure's captured cells, `t`
/// an absolute instruction index, `p` into the prototype table, `n` a count.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Op {
    // --- stack and constants
    Const(u32),
    Nil,
    True,
    False,
    Pop,
    Dup,
    // --- variables
    GetLocal(u32),
    SetLocal(u32),
    InitCell(u32),
    GetCell(u32),
    SetCell(u32),
    GetUpval(u32),
    SetUpval(u32),
    DefineGlobal(u32),
    GetGlobal(u32),
    GetGlobalOpt(u32),
    SetGlobal(u32),
    // --- arithmetic and comparison
    Add,
    Sub,
    Mul,
    Div,
    FDiv,
    Neg,
    Not,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    SLt,
    SLe,
    SGt,
    SGe,
    BoolToInt,
    CheckInt,
    CheckStr,
    // --- control
    Jump(u32),
    JumpIfFalse(u32),
    JumpIfTrue(u32),
    JumpIfZero(u32),
    JumpIfNotUndef(u32),
    Call(u32),
    Return,
    Closure(u32),
    Native(u32),
    Fail(u32),
    Print,
    // --- classes (Lox)
    Class(u32),
    Inherit,
    Method(u32),
    GetProp(u32),
    SetProp(u32),
    GetSuper(u32),
    // --- records and arrays (Tiger)
    NewRecord(u32),
    GetField(u32),
    SetField(u32),
    NewArray,
    ArrayGet,
    ArraySet,
    // --- arrays and hashes (Monkey)
    MakeArray(u32),
    MakeHash(u32),
    Index,
}

/// What kind of operand an instruction carries; drives (de)serialisation,
/// linking, verification and disassembly from a single table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Operand {
    None,
    /// constant usable as a value (int, float, string)
    ValueConst,
    /// string constant used as a name or message
    NameConst,
    /// record descriptor constant
    RecordConst,
    Local,
    Upval,
    Target,
    Proto,
    NativeId,
    Count,
}

macro_rules! opcodes {
    (
        nullary { $( $c0:literal => $n0:ident ),* $(,)? }
        unary { $( $c1:literal => $n1:ident ( $k1:ident ) ),* $(,)? }
    ) => {
        impl Op {
            /// Numeric opcode used in the binary encoding.
            pub fn code(&self) -> u8 {
                match self { $( Op::$n0 => $c0, )* $( Op::$n1(_) => $c1, )* }
            }
            /// The single operand, if any.
            pub fn operand(&self) -> Option<u32> {
                match *self { $( Op::$n0 => None, )* $( Op::$n1(x) => Some(x), )* }
            }
            pub fn operand_kind(&self) -> Operand {
                match self { $( Op::$n0 => Operand::None, )* $( Op::$n1(_) => Operand::$k1, )* }
            }
            pub fn mnemonic(&self) -> &'static str {
                match self { $( Op::$n0 => stringify!($n0), )* $( Op::$n1(_) => stringify!($n1), )* }
            }
            /// Rebuild an instruction from its opcode and operand.
            pub fn decode(code: u8, arg: u32) -> Option<Op> {
                match code { $( $c0 => Some(Op::$n0), )* $( $c1 => Some(Op::$n1(arg)), )* _ => None }
            }
            /// Does this opcode carry an operand?  `None` if unknown.
            pub fn code_has_operand(code: u8) -> Option<bool> {
                match code { $( $c0 => Some(false), )* $( $c1 => Some(true), )* _ => None }
            }
            /// Same instruction with its operand replaced.
            pub fn with_operand(&self, v: u32) -> Op {
                Op::decode(self.code(), v).expect("own opcode")
            }
        }
    };
}

opcodes! {
    nullary {
        1 => Nil, 2 => True, 3 => False, 4 => Pop, 5 => Dup,
        30 => Add, 31 => Sub, 32 => Mul, 33 => Div, 34 => FDiv, 35 => Neg, 36 => Not,
        37 => Eq, 38 => Ne, 39 => Lt, 40 => Le, 41 => Gt, 42 => Ge,
        43 => SLt, 44 => SLe, 45 => SGt, 46 => SGe, 47 => BoolToInt,
        48 => CheckInt, 49 => CheckStr,
        66 => Return, 70 => Print, 81 => Inherit,
        93 => NewArray, 94 => ArrayGet, 95 => ArraySet, 102 => Index,
    }
    unary {
        0 => Const(ValueConst),
        10 => GetLocal(Local), 11 => SetLocal(Local), 12 => InitCell(Local),
        13 => GetCell(Local), 14 => SetCell(Local), 15 => GetUpval(Upval),
        16 => SetUpval(Upval), 17 => DefineGlobal(NameConst), 18 => GetGlobal(NameConst),
        19 => GetGlobalOpt(NameConst), 20 => SetGlobal(NameConst),
        60 => Jump(Target), 61 => JumpIfFalse(Target), 62 => JumpIfTrue(Target),
        63 => JumpIfZero(Target), 64 => JumpIfNotUndef(Target),
        65 => Call(Count), 67 => Closure(Proto), 68 => Native(NativeId), 69 => Fail(NameConst),
        80 => Class(NameConst), 82 => Method(NameConst), 83 => GetProp(NameConst),
        84 => SetProp(NameConst), 85 => GetSuper(NameConst),
        90 => NewRecord(RecordConst), 91 => GetField(Count), 92 => SetField(Count),
        100 => MakeArray(Count), 101 => MakeHash(Count),
    }
}

impl Op {
    /// Net change in operand-stack height, given the constant pool (needed
    /// for `NewRecord`, whose arity lives in its descriptor).
    pub fn stack_effect(&self, consts: &[Const]) -> i64 {
        use Op::*;
        match *self {
            Const(_) | Nil | True | False | Dup | GetLocal(_) | GetCell(_) | GetUpval(_) | GetGlobal(_)
            | GetGlobalOpt(_) | Closure(_) | Native(_) | Class(_) => 1,
            Pop | SetLocal(_) | InitCell(_) | SetCell(_) | SetUpval(_) | DefineGlobal(_) | SetGlobal(_) | Print => -1,
            Add | Sub | Mul | Div | FDiv | Eq | Ne | Lt | Le | Gt | Ge | SLt | SLe | SGt | SGe => -1,
            Neg | Not | BoolToInt | CheckInt | CheckStr => 0,
            Jump(_) | JumpIfNotUndef(_) => 0,
            JumpIfFalse(_) | JumpIfTrue(_) | JumpIfZero(_) => -1,
            Call(n) => -(n as i64),
            Return => -1,
            Fail(_) => 0,
            Inherit | Method(_) | SetProp(_) | GetSuper(_) => -1,
            GetProp(_) | GetField(_) => 0,
            SetField(_) => -2,
            NewRecord(k) => match consts.get(k as usize) {
                Some(self::Const::Record(d)) => 1 - d.fields.len() as i64,
                _ => 1,
            },
            NewArray | ArrayGet | Index => -1,
            ArraySet => -3,
            MakeArray(n) => 1 - n as i64,
            MakeHash(n) => 1 - 2 * n as i64,
        }
    }

    /// Minimum stack height this instruction needs before it runs.
    pub fn stack_needs(&self, consts: &[Const]) -> i64 {
        use Op::*;
        match *self {
            Pop | SetLocal(_) | InitCell(_) | SetCell(_) | SetUpval(_) | DefineGlobal(_) | SetGlobal(_) | Print
            | Dup | Neg | Not | BoolToInt | CheckInt | CheckStr | JumpIfFalse(_) | JumpIfTrue(_) | JumpIfZero(_)
            | JumpIfNotUndef(_) | Return | GetProp(_) | GetField(_) => 1,
            Add | Sub | Mul | Div | FDiv | Eq | Ne | Lt | Le | Gt | Ge | SLt | SLe | SGt | SGe | Inherit
            | Method(_) | SetProp(_) | GetSuper(_) | SetField(_) | NewArray | ArrayGet | Index => 2,
            ArraySet => 3,
            Call(n) => n as i64 + 1,
            NewRecord(_) => 1 - self.stack_effect(consts),
            MakeArray(n) => n as i64,
            MakeHash(n) => 2 * n as i64,
            _ => 0,
        }
    }

    /// Does control never fall through to the next instruction?
    pub fn is_terminal(&self) -> bool {
        matches!(self, Op::Jump(_) | Op::Return | Op::Fail(_))
    }

    pub fn jump_target(&self) -> Option<u32> {
        match *self {
            Op::Jump(t) | Op::JumpIfFalse(t) | Op::JumpIfTrue(t) | Op::JumpIfZero(t) | Op::JumpIfNotUndef(t) => Some(t),
            _ => None,
        }
    }
}

/// Field names of a Tiger record type (or any record-shaped value).
#[derive(Clone, Debug, PartialEq)]
pub struct RecordDesc {
    pub name: String,
    pub fields: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Const {
    Int(i64),
    Float(f64),
    Str(String),
    Record(RecordDesc),
}

/// How a closure obtains one of its captured cells when it is created.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Capture {
    /// The cell stored in local slot `s` of the creating frame.
    Local(u32),
    /// Captured cell `u` of the creating closure.
    Upval(u32),
}

pub const FLAG_RECEIVER: u8 = 1; // slot 0 holds `this`; arity excludes it
pub const FLAG_INITIALIZER: u8 = 2; // Lox `init` method

/// A compiled function.
#[derive(Clone, Debug, PartialEq)]
pub struct Proto {
    pub name: String,
    pub lang: Lang,
    pub arity: u32,
    pub flags: u8,
    pub nlocals: u32,
    pub captures: Vec<Capture>,
    pub code: Vec<Op>,
    /// Source line of each instruction (same length as `code`).
    pub lines: Vec<u32>,
    /// Source-like rendering of the function, used when printing it
    /// (Monkey prints functions as their source).  May be empty.
    pub display: String,
}

impl Proto {
    pub fn has_receiver(&self) -> bool {
        self.flags & FLAG_RECEIVER != 0
    }
    pub fn is_initializer(&self) -> bool {
        self.flags & FLAG_INITIALIZER != 0
    }
    /// Number of slots filled by the caller (receiver + parameters).
    pub fn param_slots(&self) -> u32 {
        self.arity + self.has_receiver() as u32
    }
}

/// One source file: running a program runs its modules' entry functions in
/// order, all sharing a single global namespace.
#[derive(Clone, Debug, PartialEq)]
pub struct Module {
    pub name: String,
    pub lang: Lang,
    pub entry: u32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Program {
    pub consts: Vec<Const>,
    pub protos: Vec<Proto>,
    pub modules: Vec<Module>,
}

// ---------------------------------------------------------------------------
// Serialisation

struct Writer(Vec<u8>);

impl Writer {
    fn u8(&mut self, v: u8) {
        self.0.push(v)
    }
    fn u16(&mut self, v: u16) {
        self.0.extend_from_slice(&v.to_le_bytes())
    }
    fn u32(&mut self, v: u32) {
        self.0.extend_from_slice(&v.to_le_bytes())
    }
    fn u64(&mut self, v: u64) {
        self.0.extend_from_slice(&v.to_le_bytes())
    }
    fn len(&mut self, n: usize) {
        self.u32(u32::try_from(n).expect("table too large for the bytecode format"))
    }
    fn str(&mut self, s: &str) {
        self.len(s.len());
        self.0.extend_from_slice(s.as_bytes())
    }
}

impl Program {
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut w = Writer(Vec::new());
        w.0.extend_from_slice(MAGIC);
        w.u16(VERSION);
        w.len(self.consts.len());
        for c in &self.consts {
            match c {
                Const::Int(i) => {
                    w.u8(0);
                    w.u64(*i as u64)
                }
                Const::Float(f) => {
                    w.u8(1);
                    w.u64(f.to_bits())
                }
                Const::Str(s) => {
                    w.u8(2);
                    w.str(s)
                }
                Const::Record(d) => {
                    w.u8(3);
                    w.str(&d.name);
                    w.len(d.fields.len());
                    for f in &d.fields {
                        w.str(f)
                    }
                }
            }
        }
        w.len(self.protos.len());
        for p in &self.protos {
            w.str(&p.name);
            w.u8(p.lang as u8);
            w.u32(p.arity);
            w.u8(p.flags);
            w.u32(p.nlocals);
            w.len(p.captures.len());
            for c in &p.captures {
                match c {
                    Capture::Local(s) => {
                        w.u8(0);
                        w.u32(*s)
                    }
                    Capture::Upval(u) => {
                        w.u8(1);
                        w.u32(*u)
                    }
                }
            }
            w.len(p.code.len());
            for (op, line) in p.code.iter().zip(&p.lines) {
                w.u8(op.code());
                if let Some(a) = op.operand() {
                    w.u32(a)
                }
                w.u32(*line);
            }
            w.str(&p.display);
        }
        w.len(self.modules.len());
        for m in &self.modules {
            w.str(&m.name);
            w.u8(m.lang as u8);
            w.u32(m.entry);
        }
        w.0
    }

    /// Decode a bytecode file.  Structural problems are reported as errors;
    /// call [`Program::verify`] afterwards before executing.
    pub fn from_bytes(bytes: &[u8]) -> Result<Program, String> {
        let mut r = Reader { b: bytes, pos: 0 };
        if r.take(4)? != MAGIC {
            return Err("not a menagerie bytecode file (bad magic)".into());
        }
        let v = r.u16()?;
        if v != VERSION {
            return Err(format!("unsupported bytecode version {v} (expected {VERSION})"));
        }
        let mut prog = Program::default();
        for _ in 0..r.count()? {
            let c = match r.u8()? {
                0 => Const::Int(r.u64()? as i64),
                1 => Const::Float(f64::from_bits(r.u64()?)),
                2 => Const::Str(r.str()?),
                3 => {
                    let name = r.str()?;
                    let n = r.count()?;
                    let mut fields = Vec::new();
                    for _ in 0..n {
                        fields.push(r.str()?)
                    }
                    Const::Record(RecordDesc { name, fields })
                }
                t => return Err(format!("bad constant tag {t}")),
            };
            prog.consts.push(c);
        }
        for _ in 0..r.count()? {
            let name = r.str()?;
            let lang = Lang::from_u8(r.u8()?).ok_or("bad language tag")?;
            let arity = r.u32()?;
            let flags = r.u8()?;
            let nlocals = r.u32()?;
            let mut captures = Vec::new();
            for _ in 0..r.count()? {
                captures.push(match r.u8()? {
                    0 => Capture::Local(r.u32()?),
                    1 => Capture::Upval(r.u32()?),
                    t => return Err(format!("bad capture tag {t}")),
                })
            }
            let n = r.count()?;
            let mut code = Vec::new();
            let mut lines = Vec::new();
            for _ in 0..n {
                let c = r.u8()?;
                let has = Op::code_has_operand(c).ok_or_else(|| format!("bad opcode {c}"))?;
                let arg = if has { r.u32()? } else { 0 };
                code.push(Op::decode(c, arg).expect("checked opcode"));
                lines.push(r.u32()?);
            }
            let display = r.str()?;
            prog.protos.push(Proto { name, lang, arity, flags, nlocals, captures, code, lines, display });
        }
        for _ in 0..r.count()? {
            let name = r.str()?;
            let lang = Lang::from_u8(r.u8()?).ok_or("bad language tag")?;
            let entry = r.u32()?;
            prog.modules.push(Module { name, lang, entry });
        }
        if r.pos != bytes.len() {
            return Err("trailing bytes after program".into());
        }
        Ok(prog)
    }

    /// Concatenate programs, renumbering constant and prototype references.
    /// Module order is preserved, so the result runs each input in turn.
    pub fn link(parts: Vec<Program>) -> Program {
        let mut out = Program::default();
        for part in parts {
            let kbase = out.consts.len() as u32;
            let pbase = out.protos.len() as u32;
            out.consts.extend(part.consts);
            for mut p in part.protos {
                for op in &mut p.code {
                    match op.operand_kind() {
                        Operand::ValueConst | Operand::NameConst | Operand::RecordConst => {
                            *op = op.with_operand(op.operand().unwrap() + kbase)
                        }
                        Operand::Proto => *op = op.with_operand(op.operand().unwrap() + pbase),
                        _ => {}
                    }
                }
                out.protos.push(p);
            }
            for mut m in part.modules {
                m.entry += pbase;
                out.modules.push(m);
            }
        }
        out
    }
}

struct Reader<'a> {
    b: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        let end = self.pos.checked_add(n).filter(|&e| e <= self.b.len()).ok_or("unexpected end of bytecode")?;
        let s = &self.b[self.pos..end];
        self.pos = end;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16, String> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }
    fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn u64(&mut self) -> Result<u64, String> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }
    /// A table length; bounded by the remaining input so a corrupt length
    /// cannot make us allocate absurd amounts of memory.
    fn count(&mut self) -> Result<usize, String> {
        let n = self.u32()? as usize;
        if n > self.b.len() - self.pos {
            return Err("table length exceeds file size".into());
        }
        Ok(n)
    }
    fn str(&mut self) -> Result<String, String> {
        let n = self.count()?;
        String::from_utf8(self.take(n)?.to_vec()).map_err(|_| "invalid UTF-8 in string".into())
    }
}

// ---------------------------------------------------------------------------
// Verification

/// Upper bound on call argument counts, so a hostile file cannot request
/// enormous frames.
pub const MAX_ARGS: u32 = 1 << 16;
pub const MAX_LOCALS: u32 = 1 << 20;

impl Program {
    /// Check everything the VM relies on: operand ranges, constant kinds,
    /// jump targets, closure captures, and a consistent operand-stack height
    /// at every reachable instruction (so the VM never underflows).
    pub fn verify(&self) -> Result<(), String> {
        let nk = self.consts.len();
        for (pi, p) in self.protos.iter().enumerate() {
            let fail = |pc: usize, msg: String| Err(format!("function #{pi} '{}' at {pc}: {msg}", p.name));
            if p.lines.len() != p.code.len() {
                return fail(0, "line table length mismatch".into());
            }
            if p.nlocals > MAX_LOCALS || p.param_slots() > p.nlocals {
                return fail(0, "bad local slot count".into());
            }
            if p.code.is_empty() {
                return fail(0, "empty function".into());
            }
            for (pc, op) in p.code.iter().enumerate() {
                let a = op.operand().unwrap_or(0);
                let ok = match op.operand_kind() {
                    Operand::None => true,
                    Operand::ValueConst => {
                        matches!(self.consts.get(a as usize), Some(Const::Int(_) | Const::Float(_) | Const::Str(_)))
                    }
                    Operand::NameConst => matches!(self.consts.get(a as usize), Some(Const::Str(_))),
                    Operand::RecordConst => matches!(self.consts.get(a as usize), Some(Const::Record(_))),
                    Operand::Local => a < p.nlocals,
                    Operand::Upval => (a as usize) < p.captures.len(),
                    Operand::Target => (a as usize) < p.code.len(),
                    Operand::Proto => match self.protos.get(a as usize) {
                        None => false,
                        Some(q) => q.captures.iter().all(|c| match *c {
                            Capture::Local(s) => s < p.nlocals,
                            Capture::Upval(u) => (u as usize) < p.captures.len(),
                        }),
                    },
                    Operand::NativeId => (a as usize) < crate::natives::NATIVES.len(),
                    Operand::Count => a <= MAX_ARGS,
                };
                if !ok {
                    return fail(pc, format!("bad operand in {}", disasm_op(self, op)));
                }
                let _ = nk;
            }
            // Stack-height dataflow over reachable code.
            let mut height: Vec<Option<i64>> = vec![None; p.code.len()];
            let mut work = vec![(0usize, 0i64)];
            while let Some((pc, h)) = work.pop() {
                if pc >= p.code.len() {
                    return fail(pc, "control falls off the end of the function".into());
                }
                match height[pc] {
                    Some(old) if old == h => continue,
                    Some(old) => return fail(pc, format!("inconsistent stack height ({old} vs {h})")),
                    None => height[pc] = Some(h),
                }
                let op = &p.code[pc];
                if h < op.stack_needs(&self.consts) {
                    return fail(pc, format!("stack underflow in {}", op.mnemonic()));
                }
                let nh = h + op.stack_effect(&self.consts);
                if let Some(t) = op.jump_target() {
                    work.push((t as usize, nh));
                }
                if !op.is_terminal() {
                    work.push((pc + 1, nh));
                }
            }
        }
        for m in &self.modules {
            match self.protos.get(m.entry as usize) {
                Some(p) if p.arity == 0 && p.captures.is_empty() && !p.has_receiver() => {}
                _ => return Err(format!("module '{}' has an invalid entry function", m.name)),
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Disassembly

fn disasm_op(prog: &Program, op: &Op) -> String {
    let mut s = format!("{:<13}", op.mnemonic());
    if let Some(a) = op.operand() {
        let _ = write!(s, " {a}");
        let extra = match op.operand_kind() {
            Operand::ValueConst | Operand::NameConst | Operand::RecordConst => {
                prog.consts.get(a as usize).map(|c| match c {
                    Const::Int(i) => i.to_string(),
                    Const::Float(f) => format!("{f:?}"),
                    Const::Str(s) => format!("{s:?}"),
                    Const::Record(d) => format!("record {} {{{}}}", d.name, d.fields.join(", ")),
                })
            }
            Operand::Proto => prog.protos.get(a as usize).map(|p| format!("<fn {}>", p.name)),
            Operand::NativeId => crate::natives::NATIVES.get(a as usize).map(|n| format!("<native {}>", n.name)),
            _ => None,
        };
        if let Some(e) = extra {
            let _ = write!(s, "    ; {e}");
        }
    }
    s
}

impl fmt::Display for Program {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, m) in self.modules.iter().enumerate() {
            writeln!(f, "module #{i} {:?} [{}] entry=#{}", m.name, m.lang.name(), m.entry)?;
        }
        for (i, p) in self.protos.iter().enumerate() {
            writeln!(f)?;
            writeln!(
                f,
                "function #{i} {:?} [{}] arity={} locals={} flags={:#x}",
                p.name,
                p.lang.name(),
                p.arity,
                p.nlocals,
                p.flags
            )?;
            if !p.captures.is_empty() {
                let caps: Vec<String> = p
                    .captures
                    .iter()
                    .map(|c| match c {
                        Capture::Local(s) => format!("local {s}"),
                        Capture::Upval(u) => format!("upval {u}"),
                    })
                    .collect();
                writeln!(f, "  captures: {}", caps.join(", "))?;
            }
            let mut last_line = u32::MAX;
            for (pc, op) in p.code.iter().enumerate() {
                let line = p.lines[pc];
                let ls = if line == last_line { "   |".to_string() } else { format!("{line:4}") };
                last_line = line;
                writeln!(f, "  {pc:5} {ls}  {}", disasm_op(self, op))?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn proto(code: Vec<Op>, nlocals: u32) -> Proto {
        let lines = vec![1; code.len()];
        Proto {
            name: "t".into(),
            lang: Lang::Lox,
            arity: 0,
            flags: 0,
            nlocals,
            captures: vec![],
            code,
            lines,
            display: String::new(),
        }
    }

    fn program(code: Vec<Op>) -> Program {
        Program {
            consts: vec![
                Const::Int(1),
                Const::Str("x".into()),
                Const::Record(RecordDesc { name: "r".into(), fields: vec!["a".into()] }),
            ],
            protos: vec![proto(code, 1)],
            modules: vec![Module { name: "m".into(), lang: Lang::Lox, entry: 0 }],
        }
    }

    #[test]
    fn every_opcode_round_trips() {
        for code in 0..=255u8 {
            if let Some(has) = Op::code_has_operand(code) {
                let op = Op::decode(code, if has { 7 } else { 0 }).unwrap();
                assert_eq!(op.code(), code);
                assert_eq!(op.operand(), if has { Some(7) } else { None });
            }
        }
    }

    #[test]
    fn serialisation_round_trips() {
        let p = program(vec![Op::Const(0), Op::Const(0), Op::Add, Op::NewRecord(2), Op::Return]);
        let bytes = p.to_bytes();
        assert_eq!(Program::from_bytes(&bytes).unwrap(), p);
        assert!(Program::from_bytes(&bytes[..bytes.len() - 1]).is_err());
        let mut extra = bytes.clone();
        extra.push(0);
        assert!(Program::from_bytes(&extra).is_err());
    }

    #[test]
    fn verifier_accepts_valid_code() {
        program(vec![Op::Const(0), Op::Const(0), Op::Add, Op::NewRecord(2), Op::Return]).verify().unwrap();
    }

    #[test]
    fn verifier_rejects_bad_code() {
        let bad = [
            vec![Op::Add, Op::Return],                                       // underflow
            vec![Op::Nil],                                                   // falls off the end
            vec![Op::Const(1), Op::Return],                                  // name const used as value is fine...
            vec![Op::GetGlobal(0), Op::Return],                              // ...but an int is not a name
            vec![Op::NewRecord(0), Op::Return],                              // not a record descriptor
            vec![Op::GetLocal(1), Op::Return],                               // slot out of range
            vec![Op::Jump(9)],                                               // jump out of range
            vec![Op::Closure(5), Op::Return],                                // no such function
            vec![Op::Native(9999), Op::Return],                              // no such native
            vec![Op::Nil, Op::JumpIfFalse(3), Op::Nil, Op::Nil, Op::Return], // height mismatch
        ];
        for (i, code) in bad.into_iter().enumerate() {
            let r = program(code).verify();
            if i == 2 {
                assert!(r.is_ok(), "case {i}");
            } else {
                assert!(r.is_err(), "case {i} should be rejected");
            }
        }
    }

    #[test]
    fn link_renumbers_references() {
        let a = program(vec![Op::Const(0), Op::Return]);
        let b = program(vec![Op::Closure(0), Op::GetGlobal(1), Op::Pop, Op::Return]);
        let l = Program::link(vec![a, b]);
        assert_eq!(l.protos[1].code[0], Op::Closure(1));
        assert_eq!(l.protos[1].code[1], Op::GetGlobal(4));
        assert_eq!(l.modules[1].entry, 1);
        l.verify().unwrap();
    }
}
