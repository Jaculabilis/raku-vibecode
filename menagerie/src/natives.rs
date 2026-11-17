//! Built-in functions of the three languages, addressed by index from the
//! `Native` instruction.

use crate::bytecode::Lang;
use crate::value::{monkey_inspect, Value};
use std::cell::RefCell;
use std::io::{BufRead, Write};
use std::rc::Rc;

pub struct NativeDef {
    pub name: &'static str,
    pub lang: Lang,
    /// `None` for variadic natives.
    pub arity: Option<u32>,
}

macro_rules! natives {
    ($( $id:ident = $name:literal, $lang:ident, $arity:expr; )*) => {
        pub static NATIVES: &[NativeDef] = &[ $( NativeDef { name: $name, lang: Lang::$lang, arity: $arity }, )* ];
        #[allow(non_camel_case_types, clippy::upper_case_acronyms)]
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        #[repr(u32)]
        pub enum NativeId { $( $id, )* }
        impl NativeId {
            const ALL: &'static [NativeId] = &[ $( NativeId::$id, )* ];
        }
    };
}

natives! {
    Clock = "clock", Lox, Some(0);
    Len = "len", Monkey, Some(1);
    First = "first", Monkey, Some(1);
    Last = "last", Monkey, Some(1);
    Rest = "rest", Monkey, Some(1);
    Push = "push", Monkey, Some(2);
    Puts = "puts", Monkey, None;
    Print = "print", Tiger, Some(1);
    Printi = "printi", Tiger, Some(1);
    Flush = "flush", Tiger, Some(0);
    Getchar = "getchar", Tiger, Some(0);
    Ord = "ord", Tiger, Some(1);
    Chr = "chr", Tiger, Some(1);
    Size = "size", Tiger, Some(1);
    Substring = "substring", Tiger, Some(3);
    Concat = "concat", Tiger, Some(2);
    TNot = "not", Tiger, Some(1);
    Exit = "exit", Tiger, Some(1);
    // Internal helpers for Monkey's run-time `quote`; their names are not
    // valid identifiers, so programs cannot name them.
    MQuote = "%quote", Monkey, Some(1);
    MAst = "%ast", Monkey, Some(1);
}

impl NativeId {
    pub fn from_u32(i: u32) -> Option<NativeId> {
        NativeId::ALL.get(i as usize).copied()
    }
    /// Look up a language's built-in by its source-level name.
    pub fn lookup(lang: Lang, name: &str) -> Option<NativeId> {
        NATIVES.iter().position(|n| n.lang == lang && n.name == name).map(|i| NativeId::ALL[i])
    }
    pub fn def(self) -> &'static NativeDef {
        &NATIVES[self as usize]
    }
}

pub enum NativeError {
    Msg(String),
    Exit(i32),
}

impl From<String> for NativeError {
    fn from(s: String) -> Self {
        NativeError::Msg(s)
    }
}

/// Standard streams used by the program.
pub struct Io {
    pub out: Box<dyn Write>,
    pub input: Box<dyn BufRead>,
}

fn msg<T>(s: impl Into<String>) -> Result<T, NativeError> {
    Err(NativeError::Msg(s.into()))
}

fn as_int(v: &Value, what: &str) -> Result<i64, NativeError> {
    match v {
        Value::Int(i) => Ok(*i),
        Value::Float(f) if f.fract() == 0.0 => Ok(*f as i64),
        _ => msg(format!("{what}: expected an integer, got {}", v.type_name(Lang::Tiger))),
    }
}

fn as_str<'a>(v: &'a Value, what: &str) -> Result<&'a str, NativeError> {
    match v {
        Value::Str(s) => Ok(s),
        _ => msg(format!("{what}: expected a string, got {}", v.type_name(Lang::Tiger))),
    }
}

fn monkey_array<'a>(v: &'a Value, name: &str) -> Result<&'a Rc<RefCell<Vec<Value>>>, NativeError> {
    match v {
        Value::Array(a) => Ok(a),
        _ => msg(format!("argument to `{name}` must be ARRAY, got {}", v.type_name(Lang::Monkey))),
    }
}

fn io_err(e: std::io::Error) -> NativeError {
    NativeError::Msg(format!("I/O error: {e}"))
}

/// Call a native.  Arity has already been checked by the VM.
pub fn call(id: NativeId, args: &[Value], io: &mut Io) -> Result<Value, NativeError> {
    use NativeId::*;
    Ok(match id {
        Clock => {
            let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
            Value::Float(t.as_secs_f64())
        }
        Len => match &args[0] {
            Value::Str(s) => Value::Int(s.chars().count() as i64),
            Value::Array(a) => Value::Int(a.borrow().len() as i64),
            other => return msg(format!("argument to `len` not supported, got {}", other.type_name(Lang::Monkey))),
        },
        First => monkey_array(&args[0], "first")?.borrow().first().cloned().unwrap_or(Value::Nil),
        Last => monkey_array(&args[0], "last")?.borrow().last().cloned().unwrap_or(Value::Nil),
        Rest => {
            let a = monkey_array(&args[0], "rest")?.borrow();
            if a.is_empty() {
                Value::Nil
            } else {
                Value::Array(Rc::new(RefCell::new(a[1..].to_vec())))
            }
        }
        Push => {
            let mut a = monkey_array(&args[0], "push")?.borrow().clone();
            a.push(args[1].clone());
            Value::Array(Rc::new(RefCell::new(a)))
        }
        Puts => {
            for a in args {
                writeln!(io.out, "{}", monkey_inspect(a)).map_err(io_err)?;
            }
            Value::Nil
        }
        Print => {
            write!(io.out, "{}", as_str(&args[0], "print")?).map_err(io_err)?;
            Value::Nil
        }
        Printi => {
            write!(io.out, "{}", as_int(&args[0], "printi")?).map_err(io_err)?;
            Value::Nil
        }
        Flush => {
            io.out.flush().map_err(io_err)?;
            Value::Nil
        }
        Getchar => {
            io.out.flush().map_err(io_err)?;
            Value::str(&read_char(&mut *io.input).map_err(io_err)?)
        }
        Ord => Value::Int(as_str(&args[0], "ord")?.chars().next().map_or(-1, |c| c as i64)),
        Chr => {
            let i = as_int(&args[0], "chr")?;
            match u32::try_from(i).ok().filter(|&c| c <= 255).and_then(char::from_u32) {
                Some(c) => Value::str(c.encode_utf8(&mut [0; 4])),
                None => return msg(format!("chr: character code {i} out of range")),
            }
        }
        Size => Value::Int(as_str(&args[0], "size")?.chars().count() as i64),
        Substring => {
            let s = as_str(&args[0], "substring")?;
            let (f, n) = (as_int(&args[1], "substring")?, as_int(&args[2], "substring")?);
            let len = s.chars().count() as i64;
            if f < 0 || n < 0 || f.checked_add(n).is_none_or(|e| e > len) {
                return msg(format!("substring: range [{f}, {f}+{n}) out of bounds for string of size {len}"));
            }
            Value::str(&s.chars().skip(f as usize).take(n as usize).collect::<String>())
        }
        Concat => {
            let mut s = as_str(&args[0], "concat")?.to_string();
            s.push_str(as_str(&args[1], "concat")?);
            Value::str(&s)
        }
        TNot => Value::Int((as_int(&args[0], "not")? == 0) as i64),
        Exit => {
            let code = as_int(&args[0], "exit")?;
            return Err(NativeError::Exit(code as i32));
        }
        MQuote => Value::Quote(Rc::from(as_str(&args[0], "quote")?)),
        MAst => Value::str(&ast_text(&args[0])),
    })
}

/// The syntax an `unquote`d value splices into a quoted tree (the book's
/// `convertObjectToASTNode`, extended to every value).
pub fn ast_text(v: &Value) -> String {
    match v {
        Value::Quote(q) => q.to_string(),
        other => monkey_inspect(other),
    }
}

/// Read one character (one UTF-8 scalar, or one byte if the input is not
/// valid UTF-8) from the input; empty at end of file.
fn read_char(input: &mut dyn BufRead) -> std::io::Result<String> {
    let first = {
        let buf = input.fill_buf()?;
        if buf.is_empty() {
            return Ok(String::new());
        }
        buf[0]
    };
    input.consume(1);
    let extra = match first {
        0xC0..=0xDF => 1,
        0xE0..=0xEF => 2,
        0xF0..=0xF7 => 3,
        _ => 0,
    };
    let mut bytes = vec![first];
    for _ in 0..extra {
        let buf = input.fill_buf()?;
        if buf.is_empty() {
            break;
        }
        bytes.push(buf[0]);
        input.consume(1);
    }
    Ok(match String::from_utf8(bytes) {
        Ok(s) => s,
        Err(e) => char::from(e.as_bytes()[0]).to_string(),
    })
}
