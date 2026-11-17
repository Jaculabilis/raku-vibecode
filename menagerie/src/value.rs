//! Runtime values shared by all three languages.

use crate::bytecode::{Lang, Proto, RecordDesc};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

pub type Cell = Rc<RefCell<Value>>;

#[derive(Clone, Debug)]
pub enum Value {
    /// Internal sentinel: a Monkey variable that has not been bound yet.
    /// Never visible to programs.
    Undef,
    /// Lox `nil`, Monkey `null`, Tiger `nil` and the unit value.
    Nil,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(Rc<str>),
    Array(Rc<RefCell<Vec<Value>>>),
    Hash(Rc<HashObj>),
    Record(Rc<RecordObj>),
    Closure(Rc<Closure>),
    Native(u32),
    Class(Rc<Class>),
    Instance(Rc<Instance>),
    Bound(Rc<Bound>),
    /// Monkey's quoted syntax tree, held as its `String()` text.
    Quote(Rc<str>),
    /// Internal: a heap cell holding a captured variable.  Only ever stored
    /// in local slots, never on the operand stack.
    Cell(Cell),
}

#[derive(Debug)]
pub struct Closure {
    pub proto: Rc<Proto>,
    pub upvals: Box<[Cell]>,
}

#[derive(Debug)]
pub struct Class {
    pub name: Rc<str>,
    pub methods: RefCell<HashMap<Rc<str>, Rc<Closure>>>,
}

#[derive(Debug)]
pub struct Instance {
    pub class: Rc<Class>,
    pub fields: RefCell<HashMap<Rc<str>, Value>>,
}

#[derive(Debug)]
pub struct Bound {
    pub receiver: Value,
    pub method: Rc<Closure>,
}

#[derive(Debug)]
pub struct RecordObj {
    pub desc: Rc<RecordDesc>,
    pub fields: RefCell<Vec<Value>>,
}

/// Keys usable in a Monkey hash.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum HashKey {
    Int(i64),
    Bool(bool),
    Str(Rc<str>),
}

/// An immutable, insertion-ordered hash (Monkey hashes are values).
#[derive(Debug, Default)]
pub struct HashObj {
    pub pairs: Vec<(Value, Value)>,
    pub index: HashMap<HashKey, usize>,
}

impl HashObj {
    pub fn insert(&mut self, key: HashKey, k: Value, v: Value) {
        match self.index.get(&key) {
            Some(&i) => self.pairs[i] = (k, v),
            None => {
                self.index.insert(key, self.pairs.len());
                self.pairs.push((k, v));
            }
        }
    }
    pub fn get(&self, key: &HashKey) -> Option<&Value> {
        self.index.get(key).map(|&i| &self.pairs[i].1)
    }
}

impl Value {
    pub fn str(s: &str) -> Value {
        Value::Str(Rc::from(s))
    }

    /// Lox and Monkey truthiness: only `nil`/`null` and `false` are false.
    pub fn truthy(&self) -> bool {
        !matches!(self, Value::Nil | Value::Bool(false) | Value::Undef)
    }

    pub fn hash_key(&self) -> Option<HashKey> {
        match self {
            Value::Int(i) => Some(HashKey::Int(*i)),
            Value::Float(f) if f.fract() == 0.0 && f.abs() < 9.0e15 => Some(HashKey::Int(*f as i64)),
            Value::Bool(b) => Some(HashKey::Bool(*b)),
            Value::Str(s) => Some(HashKey::Str(s.clone())),
            _ => None,
        }
    }

    /// Equality shared by all three languages: numbers and strings by value,
    /// everything else by identity.  Values of different types are unequal
    /// (except ints and floats, which compare numerically).
    pub fn equals(&self, other: &Value) -> bool {
        use Value::*;
        match (self, other) {
            (Nil, Nil) => true,
            (Bool(a), Bool(b)) => a == b,
            (Int(a), Int(b)) => a == b,
            (Float(a), Float(b)) => a == b,
            (Int(a), Float(b)) | (Float(b), Int(a)) => (*a as f64) == *b,
            (Str(a), Str(b)) => a == b,
            (Array(a), Array(b)) => Rc::ptr_eq(a, b),
            (Hash(a), Hash(b)) => Rc::ptr_eq(a, b),
            (Record(a), Record(b)) => Rc::ptr_eq(a, b),
            (Closure(a), Closure(b)) => Rc::ptr_eq(a, b),
            (Native(a), Native(b)) => a == b,
            (Class(a), Class(b)) => Rc::ptr_eq(a, b),
            (Instance(a), Instance(b)) => Rc::ptr_eq(a, b),
            (Bound(a), Bound(b)) => Rc::ptr_eq(a, b),
            (Quote(a), Quote(b)) => Rc::ptr_eq(a, b),
            _ => false,
        }
    }

    /// Type name as the given language would spell it in an error message.
    pub fn type_name(&self, lang: Lang) -> &'static str {
        use Value::*;
        match lang {
            Lang::Monkey => match self {
                Undef | Nil => "NULL",
                Bool(_) => "BOOLEAN",
                Int(_) => "INTEGER",
                Float(_) => "FLOAT",
                Str(_) => "STRING",
                Array(_) => "ARRAY",
                Hash(_) => "HASH",
                Closure(_) | Bound(_) => "FUNCTION",
                Native(_) => "BUILTIN",
                Record(_) => "RECORD",
                Class(_) => "CLASS",
                Instance(_) => "INSTANCE",
                Quote(_) => "QUOTE",
                Cell(_) => "CELL",
            },
            _ => match self {
                Undef | Nil => "nil",
                Bool(_) => "boolean",
                Int(_) => "int",
                Float(_) => "number",
                Str(_) => "string",
                Array(_) => "array",
                Hash(_) => "hash",
                Closure(_) | Bound(_) => "function",
                Native(_) => "native function",
                Record(_) => "record",
                Class(_) => "class",
                Instance(_) => "instance",
                Quote(_) => "quote",
                Cell(_) => "cell",
            },
        }
    }
}

/// Lox's number formatting: integral values print without a fractional
/// part, as jlox does.
pub fn format_float(f: f64) -> String {
    if f.is_nan() {
        "NaN".into()
    } else if f.is_infinite() {
        if f > 0.0 {
            "Infinity".into()
        } else {
            "-Infinity".into()
        }
    } else if f.abs() >= 1e21 {
        format!("{f:e}")
    } else {
        // Rust's shortest round-trip formatting; integral values print
        // without a fractional part ("3", "-0").
        format!("{f}")
    }
}

/// How Lox's `print` renders a value.
pub fn lox_str(v: &Value) -> String {
    match v {
        Value::Undef | Value::Nil => "nil".into(),
        Value::Bool(b) => b.to_string(),
        Value::Int(i) => i.to_string(),
        Value::Float(f) => format_float(*f),
        Value::Str(s) => s.to_string(),
        Value::Closure(c) => fn_name(&c.proto),
        Value::Bound(b) => fn_name(&b.method.proto),
        Value::Native(_) => "<native fn>".into(),
        Value::Class(c) => c.name.to_string(),
        Value::Instance(i) => format!("{} instance", i.class.name),
        Value::Array(_) | Value::Hash(_) => monkey_inspect(v),
        Value::Record(_) => tiger_str(v),
        Value::Quote(_) => monkey_inspect(v),
        Value::Cell(c) => lox_str(&c.borrow()),
    }
}

fn fn_name(p: &Proto) -> String {
    if p.lang == Lang::Monkey && !p.display.is_empty() {
        p.display.clone()
    } else {
        format!("<fn {}>", p.name)
    }
}

/// Containers nested deeper than this print as `...`, which also stops
/// infinite output for cyclic structures (possible with Tiger records).
const MAX_PRINT_DEPTH: usize = 100;

/// How Monkey's `Inspect()` renders a value.
pub fn monkey_inspect(v: &Value) -> String {
    inspect_at(v, 0)
}

fn inspect_at(v: &Value, depth: usize) -> String {
    if depth > MAX_PRINT_DEPTH {
        return "...".into();
    }
    match v {
        Value::Undef | Value::Nil => "null".into(),
        Value::Bool(b) => b.to_string(),
        Value::Int(i) => i.to_string(),
        Value::Float(f) => format_float(*f),
        Value::Str(s) => s.to_string(),
        Value::Array(a) => {
            let items: Vec<String> = a.borrow().iter().map(|x| inspect_at(x, depth + 1)).collect();
            format!("[{}]", items.join(", "))
        }
        Value::Hash(h) => {
            let items: Vec<String> = h
                .pairs
                .iter()
                .map(|(k, v)| format!("{}: {}", inspect_at(k, depth + 1), inspect_at(v, depth + 1)))
                .collect();
            format!("{{{}}}", items.join(", "))
        }
        Value::Native(_) => "builtin function".into(),
        Value::Closure(c) => fn_name(&c.proto),
        Value::Bound(b) => fn_name(&b.method.proto),
        Value::Record(_) => record_at(v, depth),
        Value::Class(_) | Value::Instance(_) => lox_str(v),
        Value::Quote(q) => format!("QUOTE({q})"),
        Value::Cell(c) => inspect_at(&c.borrow(), depth),
    }
}

/// A readable rendering of a Tiger record (used when other languages print
/// one): `name {field=value, ...}`.
pub fn tiger_str(v: &Value) -> String {
    record_at(v, 0)
}

fn record_at(v: &Value, depth: usize) -> String {
    match v {
        Value::Record(_) if depth > MAX_PRINT_DEPTH => "...".into(),
        Value::Record(r) => {
            let fields = r.fields.borrow();
            let parts: Vec<String> = r
                .desc
                .fields
                .iter()
                .zip(fields.iter())
                .map(|(n, v)| format!("{n}={}", record_at(v, depth + 1)))
                .collect();
            format!("{} {{{}}}", r.desc.name, parts.join(", "))
        }
        Value::Str(s) => format!("{s:?}"),
        other => inspect_at(other, depth),
    }
}
