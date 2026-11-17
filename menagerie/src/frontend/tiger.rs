//! Tiger (from Andrew Appel's *Modern Compiler Implementation*, as
//! specified in Stephen Edwards' *Tiger Language Reference Manual*): lexer,
//! parser, type checker and code generator.
//!
//! Extension for linking with other languages: a declaration
//! `primitive name(params) : type` (syntax from the EPITA Tiger dialect)
//! declares a function implemented elsewhere, looked up by name in the
//! shared global namespace when called.  Functions declared in a program's
//! outermost `let` are exported to that namespace.

use crate::bytecode::{Lang, Module, Op, Program};
use crate::emit::{DeclId, FnBuilder, Gen, Label, VarLoc};
use crate::natives::NativeId;
use std::collections::{HashMap, HashSet};

type Pos = (u32, u32);

// ---------------------------------------------------------------------------
// Lexer

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Id(String),
    Int(i64),
    Str(String),
    Kw(&'static str),
    P(&'static str),
    Eof,
}

impl Tok {
    fn describe(&self) -> String {
        match self {
            Tok::Id(s) => format!("identifier '{s}'"),
            Tok::Int(i) => format!("integer {i}"),
            Tok::Str(_) => "string literal".into(),
            Tok::Kw(k) => format!("'{k}'"),
            Tok::P(p) => format!("'{p}'"),
            Tok::Eof => "end of file".into(),
        }
    }
}

const KEYWORDS: &[&str] = &[
    "array", "break", "do", "else", "end", "for", "function", "if", "in", "let", "nil", "of", "then", "to", "type",
    "var", "while",
];
const PUNCT: &[&str] = &[
    ":=", "<>", "<=", ">=", ",", ":", ";", "(", ")", "[", "]", "{", "}", ".", "+", "-", "*", "/", "=", "<", ">", "&",
    "|",
];

fn lex(src: &str) -> Result<Vec<(Tok, Pos)>, (Pos, String)> {
    let cs: Vec<char> = src.chars().collect();
    let (mut i, mut line, mut col) = (0usize, 1u32, 1u32);
    let mut out = Vec::new();
    macro_rules! bump {
        () => {{
            if cs[i] == '\n' {
                line += 1;
                col = 1;
            } else {
                col += 1;
            }
            i += 1;
        }};
    }
    loop {
        // whitespace and (nested) comments
        loop {
            if i < cs.len() && matches!(cs[i], ' ' | '\t' | '\n' | '\r' | '\x0c') {
                bump!();
            } else if i + 1 < cs.len() && cs[i] == '/' && cs[i + 1] == '*' {
                let start = (line, col);
                let mut depth = 0;
                loop {
                    if i + 1 >= cs.len() {
                        return Err((start, "unterminated comment".into()));
                    }
                    if cs[i] == '/' && cs[i + 1] == '*' {
                        depth += 1;
                        bump!();
                        bump!();
                    } else if cs[i] == '*' && cs[i + 1] == '/' {
                        depth -= 1;
                        bump!();
                        bump!();
                        if depth == 0 {
                            break;
                        }
                    } else {
                        bump!();
                    }
                }
            } else {
                break;
            }
        }
        let pos = (line, col);
        if i >= cs.len() {
            out.push((Tok::Eof, pos));
            return Ok(out);
        }
        let c = cs[i];
        if c.is_ascii_alphabetic() {
            let mut s = String::new();
            while i < cs.len() && (cs[i].is_ascii_alphanumeric() || cs[i] == '_') {
                s.push(cs[i]);
                bump!();
            }
            out.push((KEYWORDS.iter().find(|k| **k == s).map_or(Tok::Id(s), |k| Tok::Kw(k)), pos));
        } else if c.is_ascii_digit() {
            let mut s = String::new();
            while i < cs.len() && cs[i].is_ascii_digit() {
                s.push(cs[i]);
                bump!();
            }
            let n = s.parse::<i64>().map_err(|_| (pos, format!("integer literal {s} is too large")))?;
            out.push((Tok::Int(n), pos));
        } else if c == '"' {
            bump!();
            let mut s = String::new();
            loop {
                if i >= cs.len() {
                    return Err((pos, "unterminated string literal".into()));
                }
                let ch = cs[i];
                if ch == '"' {
                    bump!();
                    break;
                }
                if ch != '\\' {
                    s.push(ch);
                    bump!();
                    continue;
                }
                let esc_pos = (line, col);
                bump!();
                let bad = || (esc_pos, "illegal escape sequence in string literal".to_string());
                let e = *cs.get(i).ok_or_else(bad)?;
                match e {
                    'n' => s.push('\n'),
                    't' => s.push('\t'),
                    '"' => s.push('"'),
                    '\\' => s.push('\\'),
                    '^' => {
                        bump!();
                        let c = *cs.get(i).ok_or_else(bad)?;
                        if !('@'..='_').contains(&c) {
                            return Err(bad());
                        }
                        s.push(char::from(c as u8 - b'@'));
                    }
                    d if d.is_ascii_digit() => {
                        if i + 2 >= cs.len() || !cs[i + 1].is_ascii_digit() || !cs[i + 2].is_ascii_digit() {
                            return Err(bad());
                        }
                        let n: u32 = cs[i..i + 3].iter().collect::<String>().parse().unwrap();
                        if n > 255 {
                            return Err((esc_pos, format!("character code \\{n} out of range")));
                        }
                        s.push(char::from_u32(n).unwrap());
                        bump!();
                        bump!();
                    }
                    ' ' | '\t' | '\n' | '\r' | '\x0c' => {
                        while i < cs.len() && matches!(cs[i], ' ' | '\t' | '\n' | '\r' | '\x0c') {
                            bump!();
                        }
                        if i >= cs.len() || cs[i] != '\\' {
                            return Err((esc_pos, "unterminated \\...\\ whitespace sequence".into()));
                        }
                    }
                    _ => return Err(bad()),
                }
                bump!();
            }
            out.push((Tok::Str(s), pos));
        } else {
            let rest: String = cs[i..cs.len().min(i + 2)].iter().collect();
            let Some(p) = PUNCT.iter().find(|p| rest.starts_with(**p)) else {
                return Err((pos, format!("illegal character '{c}'")));
            };
            for _ in 0..p.len() {
                bump!();
            }
            out.push((Tok::P(p), pos));
        }
    }
}

// ---------------------------------------------------------------------------
// Syntax tree

#[derive(Debug)]
enum Exp {
    Nil,
    Int(i64),
    Str(String),
    Var(Var),
    Call(String, Vec<Exp>, Pos),
    Op(Box<Exp>, &'static str, Box<Exp>, Pos),
    Neg(Box<Exp>, Pos),
    Record(String, Vec<(String, Exp, Pos)>, Pos),
    Seq(Vec<Exp>),
    Assign(Var, Box<Exp>, Pos),
    If(Box<Exp>, Box<Exp>, Option<Box<Exp>>, Pos),
    While(Box<Exp>, Box<Exp>, Pos),
    For(String, Box<Exp>, Box<Exp>, Box<Exp>, Pos),
    Break(Pos),
    Let(Vec<Dec>, Vec<Exp>, Pos),
    Array(String, Box<Exp>, Box<Exp>, Pos),
}

#[derive(Debug)]
enum Var {
    Simple(String, Pos),
    Field(Box<Var>, String, Pos),
    Subscript(Box<Var>, Box<Exp>, Pos),
}

#[derive(Debug)]
struct Field {
    name: String,
    typ: String,
    pos: Pos,
}

#[derive(Debug)]
struct FunDec {
    name: String,
    params: Vec<Field>,
    result: Option<(String, Pos)>,
    /// `None` for a `primitive` declaration.
    body: Option<Exp>,
    pos: Pos,
}

#[derive(Debug)]
enum Ty {
    Name(String, Pos),
    Record(Vec<Field>),
    Array(String, Pos),
}

#[derive(Debug)]
enum Dec {
    Funs(Vec<FunDec>),
    Var(String, Option<(String, Pos)>, Exp, Pos),
    Types(Vec<(String, Ty, Pos)>),
}

// ---------------------------------------------------------------------------
// Parser

struct Parser {
    toks: Vec<(Tok, Pos)>,
    pos: usize,
}

type PR<T> = Result<T, (Pos, String)>;

impl Parser {
    fn peek(&self) -> &Tok {
        &self.toks[self.pos].0
    }
    fn peek2(&self) -> &Tok {
        &self.toks[(self.pos + 1).min(self.toks.len() - 1)].0
    }
    fn here(&self) -> Pos {
        self.toks[self.pos].1
    }
    fn advance(&mut self) -> Tok {
        let t = self.toks[self.pos].0.clone();
        if self.pos < self.toks.len() - 1 {
            self.pos += 1;
        }
        t
    }
    fn is(&self, t: &Tok) -> bool {
        self.peek() == t
    }
    fn is_p(&self, p: &str) -> bool {
        matches!(self.peek(), Tok::P(q) if *q == p)
    }
    fn is_kw(&self, k: &str) -> bool {
        matches!(self.peek(), Tok::Kw(q) if *q == k)
    }
    fn err<T>(&self, expected: &str) -> PR<T> {
        Err((self.here(), format!("syntax error: expected {expected}, found {}", self.peek().describe())))
    }
    fn expect_p(&mut self, p: &'static str) -> PR<()> {
        if self.is_p(p) {
            self.advance();
            Ok(())
        } else {
            self.err(&format!("'{p}'"))
        }
    }
    fn expect_kw(&mut self, k: &'static str) -> PR<()> {
        if self.is_kw(k) {
            self.advance();
            Ok(())
        } else {
            self.err(&format!("'{k}'"))
        }
    }
    fn ident(&mut self) -> PR<(String, Pos)> {
        let pos = self.here();
        match self.peek().clone() {
            Tok::Id(s) => {
                self.advance();
                Ok((s, pos))
            }
            _ => self.err("an identifier"),
        }
    }

    fn program(&mut self) -> PR<Exp> {
        let e = self.exp()?;
        if !self.is(&Tok::Eof) {
            return self.err("end of file");
        }
        Ok(e)
    }

    fn exp(&mut self) -> PR<Exp> {
        let pos = self.here();
        let e = self.or_exp()?;
        if self.is_p(":=") {
            let apos = self.here();
            let Exp::Var(v) = e else {
                return Err((pos, "syntax error: the left side of ':=' must be a variable, field or subscript".into()));
            };
            self.advance();
            let rhs = self.exp()?;
            return Ok(Exp::Assign(v, Box::new(rhs), apos));
        }
        Ok(e)
    }

    fn or_exp(&mut self) -> PR<Exp> {
        let mut e = self.and_exp()?;
        while self.is_p("|") {
            let pos = self.here();
            self.advance();
            let r = self.and_exp()?;
            e = Exp::Op(Box::new(e), "|", Box::new(r), pos);
        }
        Ok(e)
    }

    fn and_exp(&mut self) -> PR<Exp> {
        let mut e = self.cmp_exp()?;
        while self.is_p("&") {
            let pos = self.here();
            self.advance();
            let r = self.cmp_exp()?;
            e = Exp::Op(Box::new(e), "&", Box::new(r), pos);
        }
        Ok(e)
    }

    fn cmp_op(&self) -> Option<&'static str> {
        match self.peek() {
            Tok::P(p) if ["=", "<>", "<", "<=", ">", ">="].contains(p) => Some(p),
            _ => None,
        }
    }

    fn cmp_exp(&mut self) -> PR<Exp> {
        let e = self.add_exp()?;
        if let Some(op) = self.cmp_op() {
            let pos = self.here();
            self.advance();
            let r = self.add_exp()?;
            if self.cmp_op().is_some() {
                return Err((self.here(), "syntax error: comparison operators do not associate".into()));
            }
            return Ok(Exp::Op(Box::new(e), op, Box::new(r), pos));
        }
        Ok(e)
    }

    fn add_exp(&mut self) -> PR<Exp> {
        let mut e = self.mul_exp()?;
        while self.is_p("+") || self.is_p("-") {
            let pos = self.here();
            let Tok::P(op) = self.advance() else { unreachable!() };
            let r = self.mul_exp()?;
            e = Exp::Op(Box::new(e), op, Box::new(r), pos);
        }
        Ok(e)
    }

    fn mul_exp(&mut self) -> PR<Exp> {
        let mut e = self.unary()?;
        while self.is_p("*") || self.is_p("/") {
            let pos = self.here();
            let Tok::P(op) = self.advance() else { unreachable!() };
            let r = self.unary()?;
            e = Exp::Op(Box::new(e), op, Box::new(r), pos);
        }
        Ok(e)
    }

    fn unary(&mut self) -> PR<Exp> {
        if self.is_p("-") {
            let pos = self.here();
            self.advance();
            let e = self.unary()?;
            return Ok(Exp::Neg(Box::new(e), pos));
        }
        self.primary()
    }

    fn seq(&mut self, end: &Tok) -> PR<Vec<Exp>> {
        let mut es = Vec::new();
        if self.is(end) {
            return Ok(es);
        }
        es.push(self.exp()?);
        while self.is_p(";") {
            self.advance();
            es.push(self.exp()?);
        }
        Ok(es)
    }

    fn primary(&mut self) -> PR<Exp> {
        let pos = self.here();
        match self.peek().clone() {
            Tok::Int(i) => {
                self.advance();
                Ok(Exp::Int(i))
            }
            Tok::Str(s) => {
                self.advance();
                Ok(Exp::Str(s))
            }
            Tok::Kw("nil") => {
                self.advance();
                Ok(Exp::Nil)
            }
            Tok::Kw("break") => {
                self.advance();
                Ok(Exp::Break(pos))
            }
            Tok::P("(") => {
                self.advance();
                let es = self.seq(&Tok::P(")"))?;
                self.expect_p(")")?;
                Ok(Exp::Seq(es))
            }
            Tok::Kw("if") => {
                self.advance();
                let c = self.exp()?;
                self.expect_kw("then")?;
                let t = self.exp()?;
                let e = if self.is_kw("else") {
                    self.advance();
                    Some(Box::new(self.exp()?))
                } else {
                    None
                };
                Ok(Exp::If(Box::new(c), Box::new(t), e, pos))
            }
            Tok::Kw("while") => {
                self.advance();
                let c = self.exp()?;
                self.expect_kw("do")?;
                let b = self.exp()?;
                Ok(Exp::While(Box::new(c), Box::new(b), pos))
            }
            Tok::Kw("for") => {
                self.advance();
                let (v, _) = self.ident()?;
                self.expect_p(":=")?;
                let lo = self.exp()?;
                self.expect_kw("to")?;
                let hi = self.exp()?;
                self.expect_kw("do")?;
                let b = self.exp()?;
                Ok(Exp::For(v, Box::new(lo), Box::new(hi), Box::new(b), pos))
            }
            Tok::Kw("let") => {
                self.advance();
                let decs = self.decs()?;
                self.expect_kw("in")?;
                let body = self.seq(&Tok::Kw("end"))?;
                self.expect_kw("end")?;
                Ok(Exp::Let(decs, body, pos))
            }
            Tok::Id(name) => {
                self.advance();
                if self.is_p("(") {
                    self.advance();
                    let mut args = Vec::new();
                    if !self.is_p(")") {
                        args.push(self.exp()?);
                        while self.is_p(",") {
                            self.advance();
                            args.push(self.exp()?);
                        }
                    }
                    self.expect_p(")")?;
                    return Ok(Exp::Call(name, args, pos));
                }
                if self.is_p("{") {
                    self.advance();
                    let mut fields = Vec::new();
                    if !self.is_p("}") {
                        loop {
                            let (f, fpos) = self.ident()?;
                            self.expect_p("=")?;
                            let e = self.exp()?;
                            fields.push((f, e, fpos));
                            if !self.is_p(",") {
                                break;
                            }
                            self.advance();
                        }
                    }
                    self.expect_p("}")?;
                    return Ok(Exp::Record(name, fields, pos));
                }
                let mut v = Var::Simple(name.clone(), pos);
                if self.is_p("[") {
                    let bpos = self.here();
                    self.advance();
                    let idx = self.exp()?;
                    self.expect_p("]")?;
                    if self.is_kw("of") {
                        self.advance();
                        let init = self.exp()?;
                        return Ok(Exp::Array(name, Box::new(idx), Box::new(init), pos));
                    }
                    v = Var::Subscript(Box::new(v), Box::new(idx), bpos);
                }
                loop {
                    let p = self.here();
                    if self.is_p(".") {
                        self.advance();
                        let (f, _) = self.ident()?;
                        v = Var::Field(Box::new(v), f, p);
                    } else if self.is_p("[") {
                        self.advance();
                        let idx = self.exp()?;
                        self.expect_p("]")?;
                        v = Var::Subscript(Box::new(v), Box::new(idx), p);
                    } else {
                        return Ok(Exp::Var(v));
                    }
                }
            }
            _ => self.err("an expression"),
        }
    }

    fn type_fields(&mut self, end: &str) -> PR<Vec<Field>> {
        let mut fs = Vec::new();
        if self.is_p(end) {
            return Ok(fs);
        }
        loop {
            let (name, pos) = self.ident()?;
            self.expect_p(":")?;
            let (typ, _) = self.ident()?;
            fs.push(Field { name, typ, pos });
            if !self.is_p(",") {
                return Ok(fs);
            }
            self.advance();
        }
    }

    fn is_primitive(&self) -> bool {
        matches!(self.peek(), Tok::Id(s) if s == "primitive") && matches!(self.peek2(), Tok::Id(_))
    }

    fn decs(&mut self) -> PR<Vec<Dec>> {
        let mut decs: Vec<Dec> = Vec::new();
        loop {
            let pos = self.here();
            if self.is_kw("type") {
                self.advance();
                let (name, _) = self.ident()?;
                self.expect_p("=")?;
                let tpos = self.here();
                let ty = if self.is_p("{") {
                    self.advance();
                    let fs = self.type_fields("}")?;
                    self.expect_p("}")?;
                    Ty::Record(fs)
                } else if self.is_kw("array") {
                    self.advance();
                    self.expect_kw("of")?;
                    let (t, p) = self.ident()?;
                    Ty::Array(t, p)
                } else {
                    let (t, _) = self.ident()?;
                    Ty::Name(t, tpos)
                };
                match decs.last_mut() {
                    Some(Dec::Types(g)) => g.push((name, ty, pos)),
                    _ => decs.push(Dec::Types(vec![(name, ty, pos)])),
                }
            } else if self.is_kw("var") {
                self.advance();
                let (name, _) = self.ident()?;
                let typ = if self.is_p(":") {
                    self.advance();
                    Some(self.ident()?)
                } else {
                    None
                };
                self.expect_p(":=")?;
                let init = self.exp()?;
                decs.push(Dec::Var(name, typ, init, pos));
            } else if self.is_kw("function") || self.is_primitive() {
                let primitive = !self.is_kw("function");
                self.advance();
                let (name, _) = self.ident()?;
                self.expect_p("(")?;
                let params = self.type_fields(")")?;
                self.expect_p(")")?;
                let result = if self.is_p(":") {
                    self.advance();
                    Some(self.ident()?)
                } else {
                    None
                };
                let body = if primitive {
                    None
                } else {
                    self.expect_p("=")?;
                    Some(self.exp()?)
                };
                let f = FunDec { name, params, result, body, pos };
                match decs.last_mut() {
                    Some(Dec::Funs(g)) => g.push(f),
                    _ => decs.push(Dec::Funs(vec![f])),
                }
            } else {
                return Ok(decs);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Types and the typed tree produced by the checker

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Type {
    Int,
    Str,
    Nil,
    Unit,
    Record(usize),
    Array(usize),
    /// A name from a type declaration, resolved through `Semant::names`.
    Name(usize),
    /// Result of an erroneous expression; compatible with everything so
    /// that one mistake produces one message.
    Error,
}

struct RecordInfo {
    name: String,
    fields: Vec<(String, Type)>,
}

struct ArrayInfo {
    name: String,
    elem: Type,
}

#[derive(Clone, Copy, Debug)]
enum CheckKind {
    Int,
    Str,
    None,
}

#[derive(Debug)]
enum Callee {
    Local(DeclId),
    Native(NativeId),
    Extern(String, CheckKind),
}

/// A typed expression.  `value` says whether its code leaves a value on the
/// stack (expressions of type unit leave nothing).
#[derive(Debug)]
struct TE {
    k: TK,
    value: bool,
    line: u32,
}

#[derive(Debug)]
enum TK {
    Int(i64),
    Str(String),
    Nil,
    Get(DeclId),
    Set(DeclId, Box<TE>),
    Field(Box<TE>, u32),
    SetField(Box<TE>, u32, Box<TE>),
    Sub(Box<TE>, Box<TE>),
    SetSub(Box<TE>, Box<TE>, Box<TE>),
    Call(Callee, Vec<TE>),
    Bin(Op, Box<TE>, Box<TE>),
    /// Comparison producing 0/1 (the op yields a boolean).
    Cmp(Op, Box<TE>, Box<TE>),
    And(Box<TE>, Box<TE>),
    Or(Box<TE>, Box<TE>),
    Neg(Box<TE>),
    Record(usize, Vec<TE>),
    Array(Box<TE>, Box<TE>),
    Seq(Vec<TE>),
    If(Box<TE>, Box<TE>, Option<Box<TE>>),
    While(Box<TE>, Box<TE>),
    For(DeclId, Box<TE>, Box<TE>, Box<TE>),
    Break,
    Let(Vec<TD>, Box<TE>),
}

#[derive(Debug)]
enum TD {
    Var(DeclId, TE),
    Funs(Vec<TFun>),
}

#[derive(Debug)]
struct TFun {
    decl: DeclId,
    name: String,
    params: Vec<(DeclId, CheckKind)>,
    body: TE,
    returns_value: bool,
    exported: bool,
}

#[derive(Clone)]
enum VEntry {
    Var { decl: DeclId, ty: Type, readonly: bool },
    Fun { callee: FunRef, formals: Vec<Type>, result: Type },
}

#[derive(Clone)]
enum FunRef {
    Local(DeclId),
    Native(NativeId),
    Extern(String),
}

/// A symbol table with nested scopes.
struct Scoped<V> {
    scopes: Vec<HashMap<String, V>>,
}

impl<V: Clone> Scoped<V> {
    fn new() -> Self {
        Scoped { scopes: vec![HashMap::new()] }
    }
    fn push(&mut self) {
        self.scopes.push(HashMap::new())
    }
    fn pop(&mut self) {
        self.scopes.pop();
    }
    fn insert(&mut self, k: &str, v: V) {
        self.scopes.last_mut().unwrap().insert(k.to_string(), v);
    }
    fn get(&self, k: &str) -> Option<V> {
        self.scopes.iter().rev().find_map(|s| s.get(k).cloned())
    }
}

// ---------------------------------------------------------------------------
// Type checker

struct Semant {
    tenv: Scoped<Type>,
    venv: Scoped<VEntry>,
    records: Vec<RecordInfo>,
    arrays: Vec<ArrayInfo>,
    names: Vec<(String, Option<Type>)>,
    errors: Vec<String>,
    file: String,
    /// Function currently being checked (0 = the main program).
    cur_fn: usize,
    next_fn: usize,
    decl_owner: Vec<usize>,
    captured: HashSet<DeclId>,
    loop_depth: usize,
}

fn check_kind(t: Type) -> CheckKind {
    match t {
        Type::Int => CheckKind::Int,
        Type::Str => CheckKind::Str,
        _ => CheckKind::None,
    }
}

impl Semant {
    fn new(file: &str) -> Semant {
        let mut s = Semant {
            tenv: Scoped::new(),
            venv: Scoped::new(),
            records: Vec::new(),
            arrays: Vec::new(),
            names: Vec::new(),
            errors: Vec::new(),
            file: file.to_string(),
            cur_fn: 0,
            next_fn: 1,
            decl_owner: Vec::new(),
            captured: HashSet::new(),
            loop_depth: 0,
        };
        s.tenv.insert("int", Type::Int);
        s.tenv.insert("string", Type::Str);
        use Type::{Int, Str, Unit};
        let lib: &[(&str, &[Type], Type)] = &[
            ("print", &[Str], Unit),
            ("printi", &[Int], Unit),
            ("flush", &[], Unit),
            ("getchar", &[], Str),
            ("ord", &[Str], Int),
            ("chr", &[Int], Str),
            ("size", &[Str], Int),
            ("substring", &[Str, Int, Int], Str),
            ("concat", &[Str, Str], Str),
            ("not", &[Int], Int),
            ("exit", &[Int], Unit),
        ];
        for (name, formals, result) in lib {
            let id = NativeId::lookup(Lang::Tiger, name).expect("Tiger native");
            s.venv.insert(name, VEntry::Fun { callee: FunRef::Native(id), formals: formals.to_vec(), result: *result });
        }
        s
    }

    fn error(&mut self, pos: Pos, msg: impl Into<String>) {
        self.errors.push(format!("{}:{}:{}: error: {}", self.file, pos.0, pos.1, msg.into()));
    }

    fn new_decl(&mut self) -> DeclId {
        self.decl_owner.push(self.cur_fn);
        self.decl_owner.len() - 1
    }

    fn use_decl(&mut self, d: DeclId) {
        if self.decl_owner[d] != self.cur_fn {
            self.captured.insert(d);
        }
    }

    /// Follow type-name links to the underlying type.
    fn actual(&self, mut t: Type) -> Type {
        for _ in 0..=self.names.len() {
            match t {
                Type::Name(n) => match self.names[n].1 {
                    Some(u) => t = u,
                    None => return Type::Error,
                },
                other => return other,
            }
        }
        Type::Error // cycle; reported at declaration
    }

    fn type_name(&self, t: Type) -> String {
        match self.actual(t) {
            Type::Int => "int".into(),
            Type::Str => "string".into(),
            Type::Nil => "nil".into(),
            Type::Unit => "no value".into(),
            Type::Record(r) => format!("record type '{}'", self.records[r].name),
            Type::Array(a) => format!("array type '{}'", self.arrays[a].name),
            Type::Name(_) | Type::Error => "<error>".into(),
        }
    }

    /// Can a value of type `actual` be used where `expected` is required?
    fn fits(&self, actual: Type, expected: Type) -> bool {
        let (a, e) = (self.actual(actual), self.actual(expected));
        a == e || a == Type::Error || e == Type::Error || (a == Type::Nil && matches!(e, Type::Record(_)))
    }

    fn lookup_type(&mut self, name: &str, pos: Pos) -> Type {
        match self.tenv.get(name) {
            Some(t) => t,
            None => {
                self.error(pos, format!("undefined type '{name}'"));
                Type::Error
            }
        }
    }

    fn te(k: TK, ty: Type, line: u32) -> (TE, Type) {
        (TE { k, value: ty != Type::Unit, line }, ty)
    }

    fn check_int(&mut self, ty: Type, pos: Pos, what: &str) {
        if !self.fits(ty, Type::Int) {
            let found = self.type_name(ty);
            self.error(pos, format!("{what} must be an integer, found {found}"));
        }
    }

    fn exp(&mut self, e: &Exp, line: u32) -> (TE, Type) {
        match e {
            Exp::Nil => Self::te(TK::Nil, Type::Nil, line),
            Exp::Int(i) => Self::te(TK::Int(*i), Type::Int, line),
            Exp::Str(s) => Self::te(TK::Str(s.clone()), Type::Str, line),
            Exp::Var(v) => self.var(v),
            Exp::Call(name, args, pos) => {
                let targs: Vec<(TE, Type, Pos)> = args
                    .iter()
                    .map(|a| {
                        let (t, ty) = self.exp(a, pos.0);
                        (t, ty, *pos)
                    })
                    .collect();
                match self.venv.get(name) {
                    Some(VEntry::Fun { callee, formals, result }) => {
                        if formals.len() != targs.len() {
                            self.error(
                                *pos,
                                format!(
                                    "function '{name}' expects {} arguments but got {}",
                                    formals.len(),
                                    targs.len()
                                ),
                            );
                        } else {
                            for (i, ((_, ty, _), f)) in targs.iter().zip(&formals).enumerate() {
                                if !self.fits(*ty, *f) {
                                    let (want, got) = (self.type_name(*f), self.type_name(*ty));
                                    self.error(
                                        *pos,
                                        format!("argument {} of '{name}' must be {want}, found {got}", i + 1),
                                    );
                                }
                            }
                        }
                        let callee = match callee {
                            FunRef::Local(d) => {
                                self.use_decl(d);
                                Callee::Local(d)
                            }
                            FunRef::Native(id) => Callee::Native(id),
                            FunRef::Extern(n) => Callee::Extern(n, check_kind(self.actual(result))),
                        };
                        let args = targs.into_iter().map(|a| a.0).collect();
                        Self::te(TK::Call(callee, args), result, pos.0)
                    }
                    Some(VEntry::Var { .. }) => {
                        self.error(*pos, format!("'{name}' is a variable, not a function"));
                        Self::te(TK::Nil, Type::Error, pos.0)
                    }
                    None => {
                        self.error(*pos, format!("undefined function '{name}'"));
                        Self::te(TK::Nil, Type::Error, pos.0)
                    }
                }
            }
            Exp::Op(a, op, b, pos) => {
                let (ta, tya) = self.exp(a, pos.0);
                let (tb, tyb) = self.exp(b, pos.0);
                let (a, b) = (Box::new(ta), Box::new(tb));
                match *op {
                    "+" | "-" | "*" | "/" | "&" | "|" => {
                        self.check_int(tya, *pos, &format!("left operand of '{op}'"));
                        self.check_int(tyb, *pos, &format!("right operand of '{op}'"));
                        let k = match *op {
                            "+" => TK::Bin(Op::Add, a, b),
                            "-" => TK::Bin(Op::Sub, a, b),
                            "*" => TK::Bin(Op::Mul, a, b),
                            "/" => TK::Bin(Op::Div, a, b),
                            "&" => TK::And(a, b),
                            _ => TK::Or(a, b),
                        };
                        Self::te(k, Type::Int, pos.0)
                    }
                    "<" | "<=" | ">" | ">=" => {
                        let (x, y) = (self.actual(tya), self.actual(tyb));
                        let strings = match (x, y) {
                            (Type::Int | Type::Error, Type::Int | Type::Error) => false,
                            (Type::Str | Type::Error, Type::Str | Type::Error) => true,
                            _ => {
                                let (n1, n2) = (self.type_name(x), self.type_name(y));
                                self.error(*pos, format!("cannot compare {n1} with {n2} using '{op}' (both operands must be integers or both strings)"));
                                false
                            }
                        };
                        let o = match (*op, strings) {
                            ("<", false) => Op::Lt,
                            ("<=", false) => Op::Le,
                            (">", false) => Op::Gt,
                            (">=", false) => Op::Ge,
                            ("<", true) => Op::SLt,
                            ("<=", true) => Op::SLe,
                            (">", true) => Op::SGt,
                            _ => Op::SGe,
                        };
                        Self::te(TK::Cmp(o, a, b), Type::Int, pos.0)
                    }
                    _ => {
                        let (x, y) = (self.actual(tya), self.actual(tyb));
                        let ok = match (x, y) {
                            (Type::Error, _) | (_, Type::Error) => true,
                            (Type::Unit, _) | (_, Type::Unit) => false,
                            (Type::Nil, Type::Nil) => false,
                            (Type::Nil, Type::Record(_)) | (Type::Record(_), Type::Nil) => true,
                            _ => x == y,
                        };
                        if !ok {
                            let msg = if x == Type::Nil && y == Type::Nil {
                                "cannot compare nil with nil: the record type is unknown".to_string()
                            } else {
                                format!("cannot compare {} with {} using '{op}'", self.type_name(x), self.type_name(y))
                            };
                            self.error(*pos, msg);
                        }
                        Self::te(TK::Cmp(if *op == "=" { Op::Eq } else { Op::Ne }, a, b), Type::Int, pos.0)
                    }
                }
            }
            Exp::Neg(a, pos) => {
                let (t, ty) = self.exp(a, pos.0);
                self.check_int(ty, *pos, "operand of unary '-'");
                Self::te(TK::Neg(Box::new(t)), Type::Int, pos.0)
            }
            Exp::Record(tname, fields, pos) => {
                let t = self.lookup_type(tname, *pos);
                let rt = self.actual(t);
                let tfields: Vec<(TE, Type)> = fields.iter().map(|(_, e, p)| self.exp(e, p.0)).collect();
                match rt {
                    Type::Record(r) => {
                        let decl: Vec<(String, Type)> = self.records[r].fields.clone();
                        let names_match =
                            decl.len() == fields.len() && decl.iter().zip(fields).all(|((d, _), (f, _, _))| d == f);
                        if !names_match {
                            let want: Vec<&str> = decl.iter().map(|f| f.0.as_str()).collect();
                            self.error(
                                *pos,
                                format!(
                                    "record '{tname}' needs exactly the fields {{{}}} in that order",
                                    want.join(", ")
                                ),
                            );
                        } else {
                            for ((fname, fty), (_, aty)) in decl.iter().zip(&tfields) {
                                if !self.fits(*aty, *fty) {
                                    let (w, g) = (self.type_name(*fty), self.type_name(*aty));
                                    self.error(*pos, format!("field '{fname}' of '{tname}' must be {w}, found {g}"));
                                }
                            }
                        }
                        Self::te(TK::Record(r, tfields.into_iter().map(|f| f.0).collect()), t, pos.0)
                    }
                    Type::Error => Self::te(TK::Nil, Type::Error, pos.0),
                    _ => {
                        self.error(*pos, format!("'{tname}' is not a record type"));
                        Self::te(TK::Nil, Type::Error, pos.0)
                    }
                }
            }
            Exp::Seq(es) => {
                let mut items = Vec::new();
                let mut ty = Type::Unit;
                for e in es {
                    let (t, ety) = self.exp(e, line);
                    items.push(t);
                    ty = ety;
                }
                let l = items.first().map_or(line, |t| t.line);
                Self::te(TK::Seq(items), ty, l)
            }
            Exp::Assign(v, rhs, pos) => {
                if let Var::Simple(name, vpos) = v {
                    if let Some(VEntry::Var { readonly: true, .. }) = self.venv.get(name) {
                        self.error(*vpos, format!("cannot assign to loop variable '{name}'"));
                    }
                }
                let (tv, vty) = self.var(v);
                let (tr, rty) = self.exp(rhs, pos.0);
                if !self.fits(rty, vty) {
                    let (w, g) = (self.type_name(vty), self.type_name(rty));
                    self.error(*pos, format!("cannot assign {g} to a variable of {w}"));
                }
                let rhs = Box::new(tr);
                let k = match tv.k {
                    TK::Get(d) => TK::Set(d, rhs),
                    TK::Field(r, i) => TK::SetField(r, i, rhs),
                    TK::Sub(a, i) => TK::SetSub(a, i, rhs),
                    // Assigning to a variable of unit type: just evaluate.
                    _ => TK::Seq(vec![*rhs]),
                };
                Self::te(k, Type::Unit, pos.0)
            }
            Exp::If(c, t, e, pos) => {
                let (tc, cty) = self.exp(c, pos.0);
                self.check_int(cty, *pos, "condition of 'if'");
                let (tt, tty) = self.exp(t, pos.0);
                match e {
                    None => {
                        if self.actual(tty) != Type::Unit && tty != Type::Error {
                            let found = self.type_name(tty);
                            self.error(
                                *pos,
                                format!("'if-then' without 'else' must not produce a value (found {found})"),
                            );
                        }
                        Self::te(TK::If(Box::new(tc), Box::new(tt), None), Type::Unit, pos.0)
                    }
                    Some(e) => {
                        let (te, ety) = self.exp(e, pos.0);
                        let ty = if self.fits(tty, ety) && self.actual(tty) != Type::Nil {
                            tty
                        } else if self.fits(ety, tty) {
                            ety
                        } else if self.actual(tty) == Type::Nil && self.actual(ety) == Type::Nil {
                            Type::Nil
                        } else {
                            let (a, b) = (self.type_name(tty), self.type_name(ety));
                            self.error(*pos, format!("branches of 'if' have different types: {a} and {b}"));
                            Type::Error
                        };
                        let (k, ty) = (TK::If(Box::new(tc), Box::new(tt), Some(Box::new(te))), ty);
                        // A value-producing branch whose type is "error" still
                        // pushes; keep `value` consistent with the branches.
                        let value = self.actual(ty) != Type::Unit;
                        (TE { k, value, line: pos.0 }, ty)
                    }
                }
            }
            Exp::While(c, b, pos) => {
                let (tc, cty) = self.exp(c, pos.0);
                self.check_int(cty, *pos, "condition of 'while'");
                self.loop_depth += 1;
                let (tb, bty) = self.exp(b, pos.0);
                self.loop_depth -= 1;
                if self.actual(bty) != Type::Unit && bty != Type::Error {
                    let found = self.type_name(bty);
                    self.error(*pos, format!("body of 'while' must not produce a value (found {found})"));
                }
                Self::te(TK::While(Box::new(tc), Box::new(tb)), Type::Unit, pos.0)
            }
            Exp::For(v, lo, hi, b, pos) => {
                let (tlo, loty) = self.exp(lo, pos.0);
                self.check_int(loty, *pos, "lower bound of 'for'");
                let (thi, hity) = self.exp(hi, pos.0);
                self.check_int(hity, *pos, "upper bound of 'for'");
                self.venv.push();
                let d = self.new_decl();
                self.venv.insert(v, VEntry::Var { decl: d, ty: Type::Int, readonly: true });
                self.loop_depth += 1;
                let (tb, bty) = self.exp(b, pos.0);
                self.loop_depth -= 1;
                self.venv.pop();
                if self.actual(bty) != Type::Unit && bty != Type::Error {
                    let found = self.type_name(bty);
                    self.error(*pos, format!("body of 'for' must not produce a value (found {found})"));
                }
                Self::te(TK::For(d, Box::new(tlo), Box::new(thi), Box::new(tb)), Type::Unit, pos.0)
            }
            Exp::Break(pos) => {
                if self.loop_depth == 0 {
                    self.error(*pos, "'break' outside of a loop");
                }
                Self::te(TK::Break, Type::Unit, pos.0)
            }
            Exp::Let(decs, body, pos) => self.let_exp(decs, body, *pos, false),
            Exp::Array(tname, size, init, pos) => {
                let t = self.lookup_type(tname, *pos);
                let (ts, sty) = self.exp(size, pos.0);
                self.check_int(sty, *pos, "array size");
                let (ti, ity) = self.exp(init, pos.0);
                match self.actual(t) {
                    Type::Array(a) => {
                        let elem = self.arrays[a].elem;
                        if !self.fits(ity, elem) {
                            let (w, g) = (self.type_name(elem), self.type_name(ity));
                            self.error(*pos, format!("initial value of '{tname}' elements must be {w}, found {g}"));
                        }
                        Self::te(TK::Array(Box::new(ts), Box::new(ti)), t, pos.0)
                    }
                    Type::Error => Self::te(TK::Nil, Type::Error, pos.0),
                    _ => {
                        self.error(*pos, format!("'{tname}' is not an array type"));
                        Self::te(TK::Nil, Type::Error, pos.0)
                    }
                }
            }
        }
    }

    fn var(&mut self, v: &Var) -> (TE, Type) {
        match v {
            Var::Simple(name, pos) => match self.venv.get(name) {
                Some(VEntry::Var { decl, ty, .. }) => {
                    self.use_decl(decl);
                    if self.actual(ty) == Type::Unit {
                        // A variable of type unit holds nothing worth loading.
                        return (TE { k: TK::Seq(vec![]), value: false, line: pos.0 }, ty);
                    }
                    Self::te(TK::Get(decl), ty, pos.0)
                }
                Some(VEntry::Fun { .. }) => {
                    self.error(*pos, format!("'{name}' is a function, not a variable"));
                    Self::te(TK::Nil, Type::Error, pos.0)
                }
                None => {
                    self.error(*pos, format!("undefined variable '{name}'"));
                    Self::te(TK::Nil, Type::Error, pos.0)
                }
            },
            Var::Field(r, f, pos) => {
                let (tr, rty) = self.var(r);
                match self.actual(rty) {
                    Type::Record(ri) => match self.records[ri].fields.iter().position(|(n, _)| n == f) {
                        Some(i) => {
                            let fty = self.records[ri].fields[i].1;
                            Self::te(TK::Field(Box::new(tr), i as u32), fty, pos.0)
                        }
                        None => {
                            let rname = self.records[ri].name.clone();
                            self.error(*pos, format!("record type '{rname}' has no field '{f}'"));
                            Self::te(TK::Nil, Type::Error, pos.0)
                        }
                    },
                    Type::Error => Self::te(TK::Nil, Type::Error, pos.0),
                    other => {
                        let n = self.type_name(other);
                        self.error(*pos, format!("cannot access field '{f}' of {n}: not a record"));
                        Self::te(TK::Nil, Type::Error, pos.0)
                    }
                }
            }
            Var::Subscript(a, i, pos) => {
                let (ta, aty) = self.var(a);
                let (ti, ity) = self.exp(i, pos.0);
                self.check_int(ity, *pos, "array index");
                match self.actual(aty) {
                    Type::Array(ai) => {
                        let elem = self.arrays[ai].elem;
                        Self::te(TK::Sub(Box::new(ta), Box::new(ti)), elem, pos.0)
                    }
                    Type::Error => Self::te(TK::Nil, Type::Error, pos.0),
                    other => {
                        let n = self.type_name(other);
                        self.error(*pos, format!("cannot subscript {n}: not an array"));
                        Self::te(TK::Nil, Type::Error, pos.0)
                    }
                }
            }
        }
    }

    fn let_exp(&mut self, decs: &[Dec], body: &[Exp], pos: Pos, outermost: bool) -> (TE, Type) {
        self.tenv.push();
        self.venv.push();
        let mut tdecs = Vec::new();
        for d in decs {
            match d {
                Dec::Types(group) => self.type_group(group),
                Dec::Var(name, typ, init, vpos) => {
                    let (ti, ity) = self.exp(init, vpos.0);
                    let ty = match typ {
                        Some((tn, tpos)) => {
                            let t = self.lookup_type(tn, *tpos);
                            if !self.fits(ity, t) {
                                let (w, g) = (self.type_name(t), self.type_name(ity));
                                self.error(
                                    *vpos,
                                    format!("variable '{name}' is declared as {w} but initialised with {g}"),
                                );
                            }
                            t
                        }
                        None => {
                            if self.actual(ity) == Type::Nil {
                                self.error(
                                    *vpos,
                                    format!(
                                        "variable '{name}' is initialised with nil, so it needs a declared record type"
                                    ),
                                );
                            }
                            ity
                        }
                    };
                    let decl = self.new_decl();
                    self.venv.insert(name, VEntry::Var { decl, ty, readonly: false });
                    tdecs.push(TD::Var(decl, ti));
                }
                Dec::Funs(group) => {
                    let fs = self.fun_group(group, outermost);
                    if !fs.is_empty() {
                        tdecs.push(TD::Funs(fs));
                    }
                }
            }
        }
        let mut items = Vec::new();
        let mut bty = Type::Unit;
        for e in body {
            let (t, ety) = self.exp(e, pos.0);
            items.push(t);
            bty = ety;
        }
        self.venv.pop();
        self.tenv.pop();
        let value = self.actual(bty) != Type::Unit;
        let seq = TE { k: TK::Seq(items), value, line: pos.0 };
        (TE { k: TK::Let(tdecs, Box::new(seq)), value, line: pos.0 }, bty)
    }

    fn type_group(&mut self, group: &[(String, Ty, Pos)]) {
        let mut seen = HashSet::new();
        let mut ids = Vec::new();
        for (name, _, pos) in group {
            if !seen.insert(name.as_str()) {
                self.error(
                    *pos,
                    format!("type '{name}' is declared twice in the same group of mutually recursive types"),
                );
            }
            self.names.push((name.clone(), None));
            let id = self.names.len() - 1;
            ids.push(id);
            self.tenv.insert(name, Type::Name(id));
        }
        for ((name, ty, _), &id) in group.iter().zip(&ids) {
            let t = match ty {
                Ty::Name(n, p) => self.lookup_type(n, *p),
                Ty::Array(n, p) => {
                    let elem = self.lookup_type(n, *p);
                    self.arrays.push(ArrayInfo { name: name.clone(), elem });
                    Type::Array(self.arrays.len() - 1)
                }
                Ty::Record(fields) => {
                    let mut fs = Vec::new();
                    let mut fseen = HashSet::new();
                    for f in fields {
                        if !fseen.insert(f.name.as_str()) {
                            self.error(f.pos, format!("field '{}' appears twice in record type '{name}'", f.name));
                        }
                        let t = self.lookup_type(&f.typ, f.pos);
                        fs.push((f.name.clone(), t));
                    }
                    self.records.push(RecordInfo { name: name.clone(), fields: fs });
                    Type::Record(self.records.len() - 1)
                }
            };
            self.names[id].1 = Some(t);
        }
        // Every cycle must pass through a record or array type.
        for ((name, _, pos), &id) in group.iter().zip(&ids) {
            let mut t = Type::Name(id);
            let mut steps = 0;
            while let Type::Name(n) = t {
                steps += 1;
                if steps > ids.len() {
                    self.error(*pos, format!("type '{name}' is defined in terms of itself (cycles must pass through a record or array type)"));
                    self.names[id].1 = Some(Type::Error);
                    break;
                }
                t = self.names[n].1.unwrap_or(Type::Error);
            }
        }
    }

    fn fun_group(&mut self, group: &[FunDec], outermost: bool) -> Vec<TFun> {
        let mut seen = HashSet::new();
        let mut headers = Vec::new();
        for f in group {
            if !seen.insert(f.name.as_str()) {
                self.error(
                    f.pos,
                    format!(
                        "function '{}' is declared twice in the same group of mutually recursive functions",
                        f.name
                    ),
                );
            }
            let formals: Vec<Type> = f.params.iter().map(|p| self.lookup_type(&p.typ, p.pos)).collect();
            let result = match &f.result {
                Some((t, p)) => self.lookup_type(t, *p),
                None => Type::Unit,
            };
            let callee = if f.body.is_some() { FunRef::Local(self.new_decl()) } else { FunRef::Extern(f.name.clone()) };
            self.venv.insert(&f.name, VEntry::Fun { callee: callee.clone(), formals: formals.clone(), result });
            headers.push((callee, formals, result));
        }
        let mut out = Vec::new();
        for (f, (callee, formals, result)) in group.iter().zip(headers) {
            let (Some(body), FunRef::Local(decl)) = (&f.body, callee) else { continue };
            let saved = (self.cur_fn, self.loop_depth);
            self.cur_fn = self.next_fn;
            self.next_fn += 1;
            self.loop_depth = 0;
            self.venv.push();
            let mut pseen = HashSet::new();
            let mut params = Vec::new();
            for (p, &t) in f.params.iter().zip(&formals) {
                if !pseen.insert(p.name.as_str()) {
                    self.error(p.pos, format!("parameter '{}' appears twice", p.name));
                }
                let d = self.new_decl();
                self.venv.insert(&p.name, VEntry::Var { decl: d, ty: t, readonly: false });
                params.push((d, check_kind(self.actual(t))));
            }
            let (tb, bty) = self.exp(body, f.pos.0);
            self.venv.pop();
            (self.cur_fn, self.loop_depth) = saved;
            if self.actual(result) == Type::Unit {
                if self.actual(bty) != Type::Unit && bty != Type::Error {
                    let found = self.type_name(bty);
                    self.error(
                        f.pos,
                        format!(
                            "procedure '{}' must not produce a value (its body has {found}); declare a result type",
                            f.name
                        ),
                    );
                }
            } else if !self.fits(bty, result) {
                let (w, g) = (self.type_name(result), self.type_name(bty));
                self.error(f.pos, format!("function '{}' must return {w}, but its body has {g}", f.name));
            }
            out.push(TFun {
                decl,
                name: f.name.clone(),
                params,
                body: tb,
                returns_value: self.actual(result) != Type::Unit,
                exported: outermost,
            });
        }
        out
    }
}

// ---------------------------------------------------------------------------
// Code generation

struct Compiler {
    g: Gen,
    captured: HashSet<DeclId>,
    /// Innermost loop last: (exit label, stack height at the loop head).
    loops: Vec<(Label, i64)>,
    record_descs: HashMap<usize, u32>,
    records: Vec<RecordInfo>,
}

impl Compiler {
    fn declare(&mut self, d: DeclId) -> VarLoc {
        let captured = self.captured.contains(&d);
        self.g.declare(d, captured)
    }

    fn loc(&mut self, d: DeclId) -> VarLoc {
        self.g.resolve(d).expect("checked variable is in scope")
    }

    /// Compile for a value even if the expression is of type unit.
    fn value(&mut self, e: &TE) {
        self.exp(e);
        if !e.value {
            self.g.emit(Op::Nil);
        }
    }

    /// Compile a condition, jumping to `on_false` when it is zero.
    fn cond(&mut self, e: &TE, on_false: Label) {
        if let TK::Cmp(op, a, b) = &e.k {
            self.exp(a);
            self.exp(b);
            self.g.b().line = e.line;
            self.g.emit(*op);
            self.g.b().jump(Op::JumpIfFalse, on_false);
        } else {
            self.exp(e);
            self.g.b().jump(Op::JumpIfZero, on_false);
        }
    }

    fn exp(&mut self, e: &TE) {
        self.g.b().line = e.line;
        match &e.k {
            TK::Int(i) => {
                let k = self.g.pool.int(*i);
                self.g.emit(Op::Const(k));
            }
            TK::Str(s) => {
                let k = self.g.pool.str(s);
                self.g.emit(Op::Const(k));
            }
            TK::Nil => self.g.emit(Op::Nil),
            TK::Get(d) => {
                let loc = self.loc(*d);
                self.g.emit_get(loc);
            }
            TK::Set(d, v) => {
                self.value(v);
                let loc = self.loc(*d);
                self.g.emit_set(loc);
            }
            TK::Field(r, i) => {
                self.exp(r);
                self.g.b().line = e.line;
                self.g.emit(Op::GetField(*i));
            }
            TK::SetField(r, i, v) => {
                self.exp(r);
                self.value(v);
                self.g.b().line = e.line;
                self.g.emit(Op::SetField(*i));
            }
            TK::Sub(a, i) => {
                self.exp(a);
                self.exp(i);
                self.g.b().line = e.line;
                self.g.emit(Op::ArrayGet);
            }
            TK::SetSub(a, i, v) => {
                self.exp(a);
                self.exp(i);
                self.value(v);
                self.g.b().line = e.line;
                self.g.emit(Op::ArraySet);
            }
            TK::Call(callee, args) => {
                match callee {
                    Callee::Local(d) => {
                        let loc = self.loc(*d);
                        self.g.emit_get(loc);
                    }
                    Callee::Native(id) => self.g.emit(Op::Native(*id as u32)),
                    Callee::Extern(name, _) => {
                        let k = self.g.pool.str(name);
                        self.g.emit(Op::GetGlobal(k));
                    }
                }
                for a in args {
                    self.value(a);
                }
                self.g.b().line = e.line;
                self.g.emit(Op::Call(args.len() as u32));
                if let Callee::Extern(_, kind) = callee {
                    match kind {
                        CheckKind::Int => self.g.emit(Op::CheckInt),
                        CheckKind::Str => self.g.emit(Op::CheckStr),
                        CheckKind::None => {}
                    }
                }
                if !e.value {
                    self.g.emit(Op::Pop);
                }
            }
            TK::Bin(op, a, b) => {
                self.exp(a);
                self.exp(b);
                self.g.b().line = e.line;
                self.g.emit(*op);
            }
            TK::Cmp(op, a, b) => {
                self.exp(a);
                self.exp(b);
                self.g.b().line = e.line;
                self.g.emit(*op);
                self.g.emit(Op::BoolToInt);
            }
            TK::And(a, b) => {
                let (f, end) = (self.g.b().new_label(), self.g.b().new_label());
                self.cond(a, f);
                self.exp(b);
                self.g.b().jump(Op::Jump, end);
                self.g.b().place(f);
                let k = self.g.pool.int(0);
                self.g.emit(Op::Const(k));
                self.g.b().place(end);
            }
            TK::Or(a, b) => {
                let (f, end) = (self.g.b().new_label(), self.g.b().new_label());
                self.cond(a, f);
                let k = self.g.pool.int(1);
                self.g.emit(Op::Const(k));
                self.g.b().jump(Op::Jump, end);
                self.g.b().place(f);
                self.exp(b);
                self.g.b().place(end);
            }
            TK::Neg(a) => {
                self.exp(a);
                self.g.emit(Op::Neg);
            }
            TK::Record(r, fields) => {
                for f in fields {
                    self.value(f);
                }
                let k = match self.record_descs.get(r) {
                    Some(&k) => k,
                    None => {
                        let info = &self.records[*r];
                        let k = self.g.pool.record(&info.name, info.fields.iter().map(|f| f.0.clone()).collect());
                        self.record_descs.insert(*r, k);
                        k
                    }
                };
                self.g.b().line = e.line;
                self.g.b().emit_record(k, fields.len());
            }
            TK::Array(size, init) => {
                self.exp(size);
                self.value(init);
                self.g.b().line = e.line;
                self.g.emit(Op::NewArray);
            }
            TK::Seq(items) => {
                if let Some((last, init)) = items.split_last() {
                    for i in init {
                        self.exp(i);
                        if i.value {
                            self.g.emit(Op::Pop);
                        }
                    }
                    self.exp(last);
                    if last.value && !e.value {
                        self.g.emit(Op::Pop);
                    }
                }
            }
            TK::If(c, t, el) => {
                let (f, end) = (self.g.b().new_label(), self.g.b().new_label());
                self.cond(c, f);
                self.branch(t, e.value);
                self.g.b().jump(Op::Jump, end);
                self.g.b().place(f);
                if let Some(el) = el {
                    self.branch(el, e.value);
                }
                self.g.b().place(end);
            }
            TK::While(c, body) => {
                let (top, end) = (self.g.b().new_label(), self.g.b().new_label());
                self.g.b().place(top);
                let depth = self.g.b().depth;
                self.cond(c, end);
                self.loops.push((end, depth));
                self.exp(body);
                self.loops.pop();
                self.g.b().jump(Op::Jump, top);
                self.g.b().place(end);
            }
            TK::For(d, lo, hi, body) => {
                let iv = self.declare(*d);
                self.exp(lo);
                self.g.emit_init(iv);
                let hslot = self.g.b().alloc_local();
                self.exp(hi);
                self.g.emit(Op::SetLocal(hslot));
                let (top, end) = (self.g.b().new_label(), self.g.b().new_label());
                let depth = self.g.b().depth;
                self.g.emit_get(iv);
                self.g.emit(Op::GetLocal(hslot));
                self.g.emit(Op::Gt);
                self.g.b().jump(Op::JumpIfTrue, end);
                self.g.b().place(top);
                self.loops.push((end, depth));
                self.exp(body);
                self.loops.pop();
                // Test before incrementing, so hi = max int cannot overflow.
                self.g.emit_get(iv);
                self.g.emit(Op::GetLocal(hslot));
                self.g.emit(Op::Ge);
                self.g.b().jump(Op::JumpIfTrue, end);
                self.g.emit_get(iv);
                let one = self.g.pool.int(1);
                self.g.emit(Op::Const(one));
                self.g.emit(Op::Add);
                self.g.emit_set(iv);
                self.g.b().jump(Op::Jump, top);
                self.g.b().place(end);
            }
            TK::Break => {
                let (end, depth) = *self.loops.last().expect("checked: break inside loop");
                let saved = self.g.b().depth;
                for _ in depth..saved {
                    self.g.emit(Op::Pop);
                }
                self.g.b().jump(Op::Jump, end);
                // Code after `break` is dead; keep generating it as if
                // `break` were an ordinary unit expression.
                self.g.b().depth = saved;
            }
            TK::Let(decs, body) => {
                for d in decs {
                    self.dec(d);
                }
                self.exp(body);
            }
        }
    }

    fn branch(&mut self, e: &TE, want_value: bool) {
        if want_value {
            self.value(e)
        } else {
            self.exp(e);
            if e.value {
                self.g.emit(Op::Pop);
            }
        }
    }

    fn dec(&mut self, d: &TD) {
        match d {
            TD::Var(decl, init) => {
                self.value(init);
                let loc = self.declare(*decl);
                self.g.emit_init(loc);
            }
            TD::Funs(fs) => {
                // Bind every name first (as cells where captured, which
                // includes any recursive reference), then build closures.
                let locs: Vec<VarLoc> = fs.iter().map(|f| self.declare(f.decl)).collect();
                for &loc in &locs {
                    if let VarLoc::Cell(_) = loc {
                        self.g.emit(Op::Nil);
                        self.g.emit_init(loc);
                    }
                }
                for (f, &loc) in fs.iter().zip(&locs) {
                    self.function(f);
                    match loc {
                        VarLoc::Cell(_) => self.g.emit_set(loc),
                        _ => self.g.emit_init(loc),
                    }
                }
                for (f, &loc) in fs.iter().zip(&locs) {
                    if f.exported {
                        self.g.emit_get(loc);
                        let k = self.g.pool.str(&f.name);
                        self.g.emit(Op::DefineGlobal(k));
                    }
                }
            }
        }
    }

    fn function(&mut self, f: &TFun) {
        let mut b = FnBuilder::new(&f.name, Lang::Tiger, f.params.len() as u32, 0);
        b.line = f.body.line;
        self.g.push_fn(b);
        let saved_loops = std::mem::take(&mut self.loops);
        for (i, &(d, kind)) in f.params.iter().enumerate() {
            let slot = i as u32;
            // Exported functions may be called from untyped languages:
            // check (and normalise) their arguments.
            if f.exported {
                match kind {
                    CheckKind::Int => {
                        self.g.emit(Op::GetLocal(slot));
                        self.g.emit(Op::CheckInt);
                        self.g.emit(Op::SetLocal(slot));
                    }
                    CheckKind::Str => {
                        self.g.emit(Op::GetLocal(slot));
                        self.g.emit(Op::CheckStr);
                        self.g.emit(Op::Pop);
                    }
                    CheckKind::None => {}
                }
            }
            let loc = self.g.bind(d, slot, self.captured.contains(&d));
            self.g.box_param(loc);
        }
        if f.returns_value {
            self.value(&f.body);
        } else {
            self.exp(&f.body);
            if f.body.value {
                self.g.emit(Op::Pop);
            }
            self.g.emit(Op::Nil);
        }
        self.g.emit(Op::Return);
        self.loops = saved_loops;
        let p = self.g.pop_fn();
        self.g.emit(Op::Closure(p));
    }
}

/// Compile one Tiger source file into a single-module program.
pub fn compile(src: &str, module_name: &str) -> Result<Program, Vec<String>> {
    let syntax = |(p, m): (Pos, String)| vec![format!("{module_name}:{}:{}: error: {m}", p.0, p.1)];
    let toks = lex(src).map_err(syntax)?;
    let ast = Parser { toks, pos: 0 }.program().map_err(syntax)?;
    let mut s = Semant::new(module_name);
    let (te, _) = match &ast {
        Exp::Let(decs, body, pos) => s.let_exp(decs, body, *pos, true),
        other => s.exp(other, 1),
    };
    if !s.errors.is_empty() {
        return Err(s.errors);
    }
    let mut c = Compiler {
        g: Gen::new(),
        captured: s.captured,
        loops: Vec::new(),
        record_descs: HashMap::new(),
        records: s.records,
    };
    c.g.push_fn(FnBuilder::new(module_name, Lang::Tiger, 0, 0));
    c.exp(&te);
    if te.value {
        c.g.emit(Op::Pop);
    }
    c.g.emit(Op::Nil);
    c.g.emit(Op::Return);
    let entry = c.g.pop_fn();
    Ok(c.g.into_program(vec![Module { name: module_name.to_string(), lang: Lang::Tiger, entry }]))
}
