//! Monkey (from Thorsten Ball's *Writing An Interpreter In Go*, *Writing A
//! Compiler In Go* and *The Lost Chapter*): lexer, Pratt parser, macro
//! expansion and code generator.
//!
//! Scoping follows the book's tree-walking evaluator, which is the
//! language's reference semantics: every function call has one
//! environment (blocks do not introduce scopes), `let` binds or rebinds a
//! name in the current environment, closures share their defining
//! environment, and a name that is not (yet) bound in an environment is
//! looked up in the enclosing one, ending at the globals and the builtins.
//! The compiler resolves all of this statically except "has this `let` run
//! yet?", which it checks at run time with `JumpIfNotUndef`.

use super::monkey_macro;
use crate::bytecode::{Lang, Module, Op, Program};
use crate::emit::{DeclId, FnBuilder, Gen};
use crate::natives::NativeId;
use std::collections::{HashMap, HashSet};
use std::fmt;

// ---------------------------------------------------------------------------
// Lexer

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum T {
    Illegal,
    Eof,
    Ident,
    Int,
    Str,
    Assign,
    Plus,
    Minus,
    Bang,
    Asterisk,
    Slash,
    Lt,
    Gt,
    Eq,
    NotEq,
    Comma,
    Semicolon,
    Colon,
    LParen,
    RParen,
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    Function,
    Let,
    True,
    False,
    If,
    Else,
    Return,
    Macro,
}

impl T {
    /// The token type's name as the Go implementation spells it.
    fn name(self) -> &'static str {
        match self {
            T::Illegal => "ILLEGAL",
            T::Eof => "EOF",
            T::Ident => "IDENT",
            T::Int => "INT",
            T::Str => "STRING",
            T::Assign => "=",
            T::Plus => "+",
            T::Minus => "-",
            T::Bang => "!",
            T::Asterisk => "*",
            T::Slash => "/",
            T::Lt => "<",
            T::Gt => ">",
            T::Eq => "==",
            T::NotEq => "!=",
            T::Comma => ",",
            T::Semicolon => ";",
            T::Colon => ":",
            T::LParen => "(",
            T::RParen => ")",
            T::LBrace => "{",
            T::RBrace => "}",
            T::LBracket => "[",
            T::RBracket => "]",
            T::Function => "FUNCTION",
            T::Let => "LET",
            T::True => "TRUE",
            T::False => "FALSE",
            T::If => "IF",
            T::Else => "ELSE",
            T::Return => "RETURN",
            T::Macro => "MACRO",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Token {
    pub t: T,
    pub lit: String,
    pub line: u32,
}

fn lex(src: &str) -> Vec<Token> {
    let cs: Vec<char> = src.chars().collect();
    let mut toks = Vec::new();
    let mut i = 0;
    let mut line = 1;
    loop {
        while i < cs.len() && cs[i].is_whitespace() {
            if cs[i] == '\n' {
                line += 1;
            }
            i += 1;
        }
        if i >= cs.len() {
            toks.push(Token { t: T::Eof, lit: String::new(), line });
            return toks;
        }
        let c = cs[i];
        let next = cs.get(i + 1).copied();
        let start_line = line;
        let (t, lit, len) = match c {
            '=' if next == Some('=') => (T::Eq, "==".to_string(), 2),
            '!' if next == Some('=') => (T::NotEq, "!=".to_string(), 2),
            '=' => (T::Assign, c.to_string(), 1),
            '!' => (T::Bang, c.to_string(), 1),
            '+' => (T::Plus, c.to_string(), 1),
            '-' => (T::Minus, c.to_string(), 1),
            '*' => (T::Asterisk, c.to_string(), 1),
            '/' if next == Some('/') => {
                // Line comment (an extension; the book has none).
                while i < cs.len() && cs[i] != '\n' {
                    i += 1;
                }
                continue;
            }
            '/' => (T::Slash, c.to_string(), 1),
            '<' => (T::Lt, c.to_string(), 1),
            '>' => (T::Gt, c.to_string(), 1),
            ',' => (T::Comma, c.to_string(), 1),
            ';' => (T::Semicolon, c.to_string(), 1),
            ':' => (T::Colon, c.to_string(), 1),
            '(' => (T::LParen, c.to_string(), 1),
            ')' => (T::RParen, c.to_string(), 1),
            '{' => (T::LBrace, c.to_string(), 1),
            '}' => (T::RBrace, c.to_string(), 1),
            '[' => (T::LBracket, c.to_string(), 1),
            ']' => (T::RBracket, c.to_string(), 1),
            '"' => {
                // As in the book, a string runs to the next quote or the end
                // of input.  Escapes (\n \t \" \\) are an extension.
                let mut s = String::new();
                let mut j = i + 1;
                while j < cs.len() && cs[j] != '"' {
                    if cs[j] == '\n' {
                        line += 1;
                    }
                    if cs[j] == '\\' && j + 1 < cs.len() {
                        j += 1;
                        match cs[j] {
                            'n' => s.push('\n'),
                            't' => s.push('\t'),
                            'r' => s.push('\r'),
                            '"' => s.push('"'),
                            '\\' => s.push('\\'),
                            other => {
                                s.push('\\');
                                s.push(other)
                            }
                        }
                    } else {
                        s.push(cs[j]);
                    }
                    j += 1;
                }
                (T::Str, s, j + 1 - i)
            }
            c if c.is_ascii_alphabetic() || c == '_' => {
                let mut j = i;
                while j < cs.len() && (cs[j].is_ascii_alphanumeric() || cs[j] == '_') {
                    j += 1;
                }
                let word: String = cs[i..j].iter().collect();
                let t = match word.as_str() {
                    "fn" => T::Function,
                    "let" => T::Let,
                    "true" => T::True,
                    "false" => T::False,
                    "if" => T::If,
                    "else" => T::Else,
                    "return" => T::Return,
                    "macro" => T::Macro,
                    _ => T::Ident,
                };
                (t, word, j - i)
            }
            c if c.is_ascii_digit() => {
                let mut j = i;
                while j < cs.len() && cs[j].is_ascii_digit() {
                    j += 1;
                }
                (T::Int, cs[i..j].iter().collect(), j - i)
            }
            c => (T::Illegal, c.to_string(), 1),
        };
        i += len;
        toks.push(Token { t, lit, line: start_line });
    }
}

// ---------------------------------------------------------------------------
// Syntax tree

#[derive(Clone, Debug, PartialEq)]
pub enum Expr {
    /// Placeholder where the Go parser would leave a nil node after an error.
    Missing,
    Ident(String),
    Int(i64),
    /// Integer literal as written (kept for faithful `String()` output).
    Bool(bool),
    Str(String),
    Prefix(String, Box<Expr>, u32),
    Infix(Box<Expr>, String, Box<Expr>, u32),
    If(Box<Expr>, Vec<Stmt>, Option<Vec<Stmt>>),
    Fn(Vec<String>, Vec<Stmt>),
    Call(Box<Expr>, Vec<Expr>, u32),
    Array(Vec<Expr>),
    Index(Box<Expr>, Box<Expr>, u32),
    Hash(Vec<(Expr, Expr)>),
    Macro(Vec<String>, Vec<Stmt>),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Stmt {
    Let(String, Expr, u32),
    Return(Expr, u32),
    Expr(Expr),
}

/// `String()` of the book's AST nodes.
impl fmt::Display for Expr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Expr::Missing => Ok(()),
            Expr::Ident(s) => write!(f, "{s}"),
            Expr::Int(i) => write!(f, "{i}"),
            Expr::Bool(b) => write!(f, "{b}"),
            Expr::Str(s) => write!(f, "{s}"),
            Expr::Prefix(op, r, _) => write!(f, "({op}{r})"),
            Expr::Infix(l, op, r, _) => write!(f, "({l} {op} {r})"),
            Expr::If(c, t, e) => {
                write!(f, "if{c} {}", Block(t))?;
                if let Some(e) = e {
                    write!(f, "else {}", Block(e))?;
                }
                Ok(())
            }
            Expr::Fn(ps, body) => write!(f, "fn({}) {}", ps.join(", "), Block(body)),
            Expr::Macro(ps, body) => write!(f, "macro({}) {}", ps.join(", "), Block(body)),
            Expr::Call(c, args, _) => write!(f, "{c}({})", join(args)),
            Expr::Array(xs) => write!(f, "[{}]", join(xs)),
            Expr::Index(l, i, _) => write!(f, "({l}[{i}])"),
            Expr::Hash(ps) => {
                let parts: Vec<String> = ps.iter().map(|(k, v)| format!("{k}:{v}")).collect();
                write!(f, "{{{}}}", parts.join(", "))
            }
        }
    }
}

impl fmt::Display for Stmt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Stmt::Let(n, v, _) => write!(f, "let {n} = {v};"),
            Stmt::Return(v, _) => write!(f, "return {v};"),
            Stmt::Expr(e) => write!(f, "{e}"),
        }
    }
}

pub struct Block<'a>(pub &'a [Stmt]);

impl fmt::Display for Block<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for s in self.0 {
            write!(f, "{s}")?;
        }
        Ok(())
    }
}

fn join(xs: &[Expr]) -> String {
    xs.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(", ")
}

/// How the evaluator's `Inspect()` shows a function.
pub fn fn_display(params: &[String], body: &[Stmt]) -> String {
    format!("fn({}) {{\n{}\n}}", params.join(", "), Block(body))
}

// ---------------------------------------------------------------------------
// Parser: a direct transcription of the book's Pratt parser, including its
// error messages and recovery behaviour.

#[derive(Clone, Copy, PartialEq, PartialOrd)]
enum Prec {
    Lowest,
    Equals,
    LessGreater,
    Sum,
    Product,
    Prefix,
    Call,
    Index,
}

fn precedence(t: T) -> Prec {
    match t {
        T::Eq | T::NotEq => Prec::Equals,
        T::Lt | T::Gt => Prec::LessGreater,
        T::Plus | T::Minus => Prec::Sum,
        T::Slash | T::Asterisk => Prec::Product,
        T::LParen => Prec::Call,
        T::LBracket => Prec::Index,
        _ => Prec::Lowest,
    }
}

struct Parser {
    toks: Vec<Token>,
    pos: usize,
    errors: Vec<String>,
}

impl Parser {
    fn cur(&self) -> &Token {
        &self.toks[self.pos.min(self.toks.len() - 1)]
    }
    fn peek(&self) -> &Token {
        &self.toks[(self.pos + 1).min(self.toks.len() - 1)]
    }
    fn next(&mut self) {
        if self.pos < self.toks.len() - 1 {
            self.pos += 1;
        }
    }
    fn cur_is(&self, t: T) -> bool {
        self.cur().t == t
    }
    fn peek_is(&self, t: T) -> bool {
        self.peek().t == t
    }
    fn expect_peek(&mut self, t: T) -> bool {
        if self.peek_is(t) {
            self.next();
            true
        } else {
            self.errors.push(format!("expected next token to be {}, got {} instead", t.name(), self.peek().t.name()));
            false
        }
    }

    fn program(&mut self) -> Vec<Stmt> {
        let mut stmts = Vec::new();
        while !self.cur_is(T::Eof) {
            if let Some(s) = self.statement() {
                stmts.push(s);
            }
            self.next();
        }
        stmts
    }

    fn statement(&mut self) -> Option<Stmt> {
        match self.cur().t {
            T::Let => {
                let line = self.cur().line;
                if !self.expect_peek(T::Ident) {
                    return None;
                }
                let name = self.cur().lit.clone();
                if !self.expect_peek(T::Assign) {
                    return None;
                }
                self.next();
                let v = self.expression(Prec::Lowest);
                if self.peek_is(T::Semicolon) {
                    self.next();
                }
                Some(Stmt::Let(name, v, line))
            }
            T::Return => {
                let line = self.cur().line;
                self.next();
                let v = self.expression(Prec::Lowest);
                if self.peek_is(T::Semicolon) {
                    self.next();
                }
                Some(Stmt::Return(v, line))
            }
            _ => {
                let e = self.expression(Prec::Lowest);
                if self.peek_is(T::Semicolon) {
                    self.next();
                }
                Some(Stmt::Expr(e))
            }
        }
    }

    fn expression(&mut self, prec: Prec) -> Expr {
        let Some(mut left) = self.prefix() else {
            return Expr::Missing;
        };
        while !self.peek_is(T::Semicolon) && prec < precedence(self.peek().t) {
            let t = self.peek().t;
            let line = self.peek().line;
            match t {
                T::Plus | T::Minus | T::Slash | T::Asterisk | T::Eq | T::NotEq | T::Lt | T::Gt => {
                    self.next();
                    let op = self.cur().lit.clone();
                    let p = precedence(t);
                    self.next();
                    let right = self.expression(p);
                    left = Expr::Infix(Box::new(left), op, Box::new(right), line);
                }
                T::LParen => {
                    self.next();
                    let args = self.expression_list(T::RParen);
                    left = Expr::Call(Box::new(left), args.unwrap_or_default(), line);
                }
                T::LBracket => {
                    self.next();
                    self.next();
                    let idx = self.expression(Prec::Lowest);
                    if !self.expect_peek(T::RBracket) {
                        return Expr::Missing;
                    }
                    left = Expr::Index(Box::new(left), Box::new(idx), line);
                }
                _ => return left,
            }
        }
        left
    }

    /// Returns `None` where the Go parser's prefix function is missing (the
    /// caller then yields a nil expression); `Some(Expr::Missing)` where a
    /// prefix function itself returned nil.
    fn prefix(&mut self) -> Option<Expr> {
        let tok = self.cur().clone();
        Some(match tok.t {
            T::Ident => Expr::Ident(tok.lit),
            T::Int => match tok.lit.parse::<i64>() {
                Ok(i) => Expr::Int(i),
                Err(_) => {
                    self.errors.push(format!("could not parse {:?} as integer", tok.lit));
                    Expr::Missing
                }
            },
            T::Str => Expr::Str(tok.lit),
            T::True => Expr::Bool(true),
            T::False => Expr::Bool(false),
            T::Bang | T::Minus => {
                self.next();
                let r = self.expression(Prec::Prefix);
                Expr::Prefix(tok.lit, Box::new(r), tok.line)
            }
            T::LParen => {
                self.next();
                let e = self.expression(Prec::Lowest);
                if !self.expect_peek(T::RParen) {
                    return Some(Expr::Missing);
                }
                e
            }
            T::If => {
                if !self.expect_peek(T::LParen) {
                    return Some(Expr::Missing);
                }
                self.next();
                let cond = self.expression(Prec::Lowest);
                if !self.expect_peek(T::RParen) || !self.expect_peek(T::LBrace) {
                    return Some(Expr::Missing);
                }
                let then = self.block();
                let mut els = None;
                if self.peek_is(T::Else) {
                    self.next();
                    if !self.expect_peek(T::LBrace) {
                        return Some(Expr::Missing);
                    }
                    els = Some(self.block());
                }
                Expr::If(Box::new(cond), then, els)
            }
            T::Function | T::Macro => {
                if !self.expect_peek(T::LParen) {
                    return Some(Expr::Missing);
                }
                let Some(params) = self.parameters() else {
                    return Some(Expr::Missing);
                };
                if !self.expect_peek(T::LBrace) {
                    return Some(Expr::Missing);
                }
                let body = self.block();
                if tok.t == T::Function {
                    Expr::Fn(params, body)
                } else {
                    Expr::Macro(params, body)
                }
            }
            T::LBracket => match self.expression_list(T::RBracket) {
                Some(xs) => Expr::Array(xs),
                None => Expr::Missing,
            },
            T::LBrace => {
                let mut pairs = Vec::new();
                while !self.peek_is(T::RBrace) {
                    self.next();
                    let k = self.expression(Prec::Lowest);
                    if !self.expect_peek(T::Colon) {
                        return Some(Expr::Missing);
                    }
                    self.next();
                    let v = self.expression(Prec::Lowest);
                    pairs.push((k, v));
                    if !self.peek_is(T::RBrace) && !self.expect_peek(T::Comma) {
                        return Some(Expr::Missing);
                    }
                }
                if !self.expect_peek(T::RBrace) {
                    return Some(Expr::Missing);
                }
                Expr::Hash(pairs)
            }
            t => {
                self.errors.push(format!("no prefix parse function for {} found", t.name()));
                return None;
            }
        })
    }

    fn block(&mut self) -> Vec<Stmt> {
        let mut stmts = Vec::new();
        self.next();
        while !self.cur_is(T::RBrace) && !self.cur_is(T::Eof) {
            if let Some(s) = self.statement() {
                stmts.push(s);
            }
            self.next();
        }
        stmts
    }

    fn parameters(&mut self) -> Option<Vec<String>> {
        let mut ps = Vec::new();
        if self.peek_is(T::RParen) {
            self.next();
            return Some(ps);
        }
        self.next();
        ps.push(self.cur().lit.clone());
        while self.peek_is(T::Comma) {
            self.next();
            self.next();
            ps.push(self.cur().lit.clone());
        }
        if !self.expect_peek(T::RParen) {
            return None;
        }
        Some(ps)
    }

    fn expression_list(&mut self, end: T) -> Option<Vec<Expr>> {
        let mut xs = Vec::new();
        if self.peek_is(end) {
            self.next();
            return Some(xs);
        }
        self.next();
        xs.push(self.expression(Prec::Lowest));
        while self.peek_is(T::Comma) {
            self.next();
            self.next();
            xs.push(self.expression(Prec::Lowest));
        }
        if !self.expect_peek(end) {
            return None;
        }
        Some(xs)
    }
}

pub fn parse(src: &str) -> Result<Vec<Stmt>, Vec<String>> {
    let mut p = Parser { toks: lex(src), pos: 0, errors: Vec::new() };
    let prog = p.program();
    if p.errors.is_empty() {
        Ok(prog)
    } else {
        Err(p.errors)
    }
}

// ---------------------------------------------------------------------------
// Scope analysis

/// Names bound in a function's own environment: its parameters and every
/// `let` in its body outside nested function literals.
fn collect_lets(stmts: &[Stmt], out: &mut HashSet<String>) {
    fn expr(e: &Expr, out: &mut HashSet<String>) {
        match e {
            Expr::If(c, t, el) => {
                expr(c, out);
                collect_lets(t, out);
                if let Some(el) = el {
                    collect_lets(el, out);
                }
            }
            Expr::Prefix(_, r, _) => expr(r, out),
            Expr::Infix(l, _, r, _) | Expr::Index(l, r, _) => {
                expr(l, out);
                expr(r, out);
            }
            Expr::Call(c, args, _) => {
                expr(c, out);
                args.iter().for_each(|a| expr(a, out));
            }
            Expr::Array(xs) => xs.iter().for_each(|a| expr(a, out)),
            Expr::Hash(ps) => ps.iter().for_each(|(k, v)| {
                expr(k, out);
                expr(v, out)
            }),
            _ => {}
        }
    }
    for s in stmts {
        match s {
            Stmt::Let(n, v, _) => {
                out.insert(n.clone());
                expr(v, out);
            }
            Stmt::Return(v, _) | Stmt::Expr(v) => expr(v, out),
        }
    }
}

/// Names that a function literal may look up in its enclosing
/// environments: identifiers used in it or in functions nested inside it,
/// except its own parameters (which are always bound).
fn escaping(params: &[String], body: &[Stmt]) -> HashSet<String> {
    fn expr(e: &Expr, out: &mut HashSet<String>) {
        match e {
            Expr::Ident(n) => {
                out.insert(n.clone());
            }
            Expr::Fn(ps, b) | Expr::Macro(ps, b) => out.extend(escaping(ps, b)),
            Expr::If(c, t, el) => {
                expr(c, out);
                stmts(t, out);
                if let Some(el) = el {
                    stmts(el, out);
                }
            }
            Expr::Prefix(_, r, _) => expr(r, out),
            Expr::Infix(l, _, r, _) | Expr::Index(l, r, _) => {
                expr(l, out);
                expr(r, out);
            }
            Expr::Call(c, args, _) => {
                expr(c, out);
                args.iter().for_each(|a| expr(a, out));
            }
            Expr::Array(xs) => xs.iter().for_each(|a| expr(a, out)),
            Expr::Hash(ps) => ps.iter().for_each(|(k, v)| {
                expr(k, out);
                expr(v, out)
            }),
            Expr::Missing | Expr::Int(_) | Expr::Bool(_) | Expr::Str(_) => {}
        }
    }
    fn stmts(ss: &[Stmt], out: &mut HashSet<String>) {
        for s in ss {
            match s {
                Stmt::Let(_, v, _) | Stmt::Return(v, _) | Stmt::Expr(v) => expr(v, out),
            }
        }
    }
    let mut out = HashSet::new();
    stmts(body, &mut out);
    for p in params {
        out.remove(p);
    }
    out
}

/// Names used by function literals nested (at any depth) in `body` that
/// may resolve to the enclosing function's environment.
fn used_by_inner(body: &[Stmt]) -> HashSet<String> {
    // `escaping` of a parameterless pseudo-function over the body includes
    // direct uses too; subtract nothing but collect only from nested fns.
    fn expr(e: &Expr, out: &mut HashSet<String>) {
        match e {
            Expr::Fn(ps, b) | Expr::Macro(ps, b) => out.extend(escaping(ps, b)),
            Expr::If(c, t, el) => {
                expr(c, out);
                stmts(t, out);
                if let Some(el) = el {
                    stmts(el, out);
                }
            }
            Expr::Prefix(_, r, _) => expr(r, out),
            Expr::Infix(l, _, r, _) | Expr::Index(l, r, _) => {
                expr(l, out);
                expr(r, out);
            }
            Expr::Call(c, args, _) => {
                expr(c, out);
                args.iter().for_each(|a| expr(a, out));
            }
            Expr::Array(xs) => xs.iter().for_each(|a| expr(a, out)),
            Expr::Hash(ps) => ps.iter().for_each(|(k, v)| {
                expr(k, out);
                expr(v, out)
            }),
            _ => {}
        }
    }
    fn stmts(ss: &[Stmt], out: &mut HashSet<String>) {
        for s in ss {
            match s {
                Stmt::Let(_, v, _) | Stmt::Return(v, _) | Stmt::Expr(v) => expr(v, out),
            }
        }
    }
    let mut out = HashSet::new();
    stmts(body, &mut out);
    out
}

// ---------------------------------------------------------------------------
// Code generation

/// One function environment being compiled.
struct Env {
    params: HashSet<String>,
    lets: HashSet<String>,
    decls: HashMap<String, DeclId>,
}

struct Compiler {
    g: Gen,
    /// Function environments, outermost first (the module top level is not
    /// included: its bindings are globals).
    envs: Vec<Env>,
    next_decl: DeclId,
}

impl Compiler {
    fn line(&mut self, line: u32) {
        self.g.b().line = line;
    }

    /// Emit code that pushes the value of identifier `name`.
    fn ident(&mut self, name: &str) {
        // Candidate environments, innermost first.
        let mut cands: Vec<(DeclId, bool)> = Vec::new();
        for env in self.envs.iter().rev() {
            if env.params.contains(name) {
                cands.push((env.decls[name], true));
                break;
            }
            if env.lets.contains(name) {
                cands.push((env.decls[name], false));
            }
        }
        let definite = cands.last().is_some_and(|c| c.1);
        if let [(d, true)] = cands[..] {
            let loc = self.g.resolve(d).expect("bound variable");
            self.g.emit_get(loc);
            return;
        }
        let end = self.g.b().new_label();
        for (i, &(d, def)) in cands.iter().enumerate() {
            let loc = self.g.resolve(d).expect("bound variable");
            self.g.emit_get(loc);
            if def {
                debug_assert_eq!(i, cands.len() - 1);
            } else {
                self.g.b().jump(Op::JumpIfNotUndef, end);
                self.g.emit(Op::Pop);
            }
        }
        if !definite {
            let k = self.g.pool.str(name);
            self.g.emit(Op::GetGlobalOpt(k));
            self.g.b().jump(Op::JumpIfNotUndef, end);
            self.g.emit(Op::Pop);
            match NativeId::lookup(Lang::Monkey, name) {
                Some(id) => self.g.emit(Op::Native(id as u32)),
                None => {
                    let k = self.g.pool.str(&format!("identifier not found: {name}"));
                    self.g.emit(Op::Fail(k));
                }
            }
        }
        self.g.b().place(end);
    }

    fn stmt(&mut self, s: &Stmt) {
        match s {
            Stmt::Let(name, v, line) => {
                self.expr_named(v, Some(name));
                self.line(*line);
                match self.envs.last() {
                    None => {
                        let k = self.g.pool.str(name);
                        self.g.emit(Op::DefineGlobal(k));
                    }
                    Some(env) => {
                        let d = env.decls[name];
                        let loc = self.g.resolve(d).expect("own variable");
                        self.g.emit_set(loc);
                    }
                }
            }
            Stmt::Return(v, line) => {
                self.expr(v);
                self.line(*line);
                self.g.emit(Op::Return);
            }
            Stmt::Expr(e) => {
                self.expr(e);
                self.g.emit(Op::Pop);
            }
        }
    }

    /// Compile a block for its value: the value of its last statement if
    /// that is an expression statement, otherwise null.
    fn block_value(&mut self, stmts: &[Stmt]) {
        match stmts.split_last() {
            None => self.g.emit(Op::Nil),
            Some((last, init)) => {
                for s in init {
                    self.stmt(s);
                }
                match last {
                    Stmt::Expr(e) => self.expr(e),
                    other => {
                        self.stmt(other);
                        self.g.emit(Op::Nil);
                    }
                }
            }
        }
    }

    fn expr(&mut self, e: &Expr) {
        self.expr_named(e, None)
    }

    fn expr_named(&mut self, e: &Expr, name: Option<&str>) {
        match e {
            Expr::Missing => self.g.emit(Op::Nil),
            Expr::Ident(n) => self.ident(n),
            Expr::Int(i) => {
                let k = self.g.pool.int(*i);
                self.g.emit(Op::Const(k))
            }
            Expr::Bool(true) => self.g.emit(Op::True),
            Expr::Bool(false) => self.g.emit(Op::False),
            Expr::Str(s) => {
                let k = self.g.pool.str(s);
                self.g.emit(Op::Const(k))
            }
            Expr::Prefix(op, r, line) => {
                self.expr(r);
                self.line(*line);
                self.g.emit(if op == "-" { Op::Neg } else { Op::Not });
            }
            Expr::Infix(l, op, r, line) => {
                self.expr(l);
                self.expr(r);
                self.line(*line);
                self.g.emit(match op.as_str() {
                    "+" => Op::Add,
                    "-" => Op::Sub,
                    "*" => Op::Mul,
                    "/" => Op::Div,
                    "<" => Op::Lt,
                    ">" => Op::Gt,
                    "==" => Op::Eq,
                    _ => Op::Ne,
                });
            }
            Expr::If(c, t, el) => {
                self.expr(c);
                let (else_l, end) = (self.g.b().new_label(), self.g.b().new_label());
                self.g.b().jump(Op::JumpIfFalse, else_l);
                self.block_value(t);
                self.g.b().jump(Op::Jump, end);
                self.g.b().place(else_l);
                match el {
                    Some(el) => self.block_value(el),
                    None => self.g.emit(Op::Nil),
                }
                self.g.b().place(end);
            }
            Expr::Fn(params, body) => self.function(params, body, name.unwrap_or("fn")),
            Expr::Macro(..) => {
                // Macro literals outside top-level `let`s evaluate to
                // nothing useful at run time; the book's evaluator rejects
                // calling them.  Represent them as null.
                self.g.emit(Op::Nil)
            }
            Expr::Call(callee, args, line) => {
                if let Expr::Ident(n) = &**callee {
                    if n == "quote" && args.len() == 1 && !self.shadowed("quote") {
                        self.line(*line);
                        return self.quote(&args[0]);
                    }
                }
                self.expr(callee);
                for a in args {
                    self.expr(a);
                }
                self.line(*line);
                self.g.emit(Op::Call(args.len() as u32));
            }
            Expr::Array(xs) => {
                for x in xs {
                    self.expr(x);
                }
                self.g.emit(Op::MakeArray(xs.len() as u32));
            }
            Expr::Index(l, i, line) => {
                self.expr(l);
                self.expr(i);
                self.line(*line);
                self.g.emit(Op::Index);
            }
            Expr::Hash(ps) => {
                for (k, v) in ps {
                    self.expr(k);
                    self.expr(v);
                }
                self.g.emit(Op::MakeHash(ps.len() as u32));
            }
        }
    }

    fn shadowed(&self, name: &str) -> bool {
        self.envs.iter().any(|e| e.decls.contains_key(name))
    }

    /// `quote(expr)` at run time evaluates to a quoted syntax tree, which
    /// we represent by its `String()` text with `unquote(...)` results
    /// spliced in (the book's `String()` is compositional, so splicing text
    /// is the same as splicing nodes).
    fn quote(&mut self, e: &Expr) {
        self.g.emit(Op::Native(NativeId::MQuote as u32));
        let k = self.g.pool.str("");
        self.g.emit(Op::Const(k));
        for p in monkey_macro::quote_pieces(e) {
            match p {
                monkey_macro::Piece::Text(t) => {
                    let k = self.g.pool.str(&t);
                    self.g.emit(Op::Const(k));
                }
                monkey_macro::Piece::Unquote(inner) => {
                    self.g.emit(Op::Native(NativeId::MAst as u32));
                    self.expr(&inner);
                    self.g.emit(Op::Call(1));
                }
            }
            self.g.emit(Op::Add);
        }
        self.g.emit(Op::Call(1));
    }

    fn function(&mut self, params: &[String], body: &[Stmt], name: &str) {
        let mut b = FnBuilder::new(name, Lang::Monkey, params.len() as u32, 0);
        b.proto.display = fn_display(params, body);
        b.line = self.g.b().line;
        self.g.push_fn(b);
        let inner = used_by_inner(body);
        let mut lets = HashSet::new();
        collect_lets(body, &mut lets);
        let mut env = Env { params: params.iter().cloned().collect(), lets, decls: HashMap::new() };
        // Parameters: if a name repeats, the last occurrence wins, as in the
        // evaluator's environment.
        let mut param_slot: HashMap<&str, u32> = HashMap::new();
        for (i, p) in params.iter().enumerate() {
            param_slot.insert(p, i as u32);
        }
        let mut params_sorted: Vec<(&str, u32)> = param_slot.into_iter().collect();
        params_sorted.sort_by_key(|p| p.1);
        for (p, slot) in params_sorted {
            let d = self.fresh();
            env.decls.insert(p.to_string(), d);
            let loc = self.g.bind(d, slot, inner.contains(p));
            self.g.box_param(loc);
        }
        let mut lets: Vec<&String> = env.lets.iter().filter(|n| !env.params.contains(*n)).collect();
        lets.sort();
        for n in lets {
            let d = self.fresh();
            env.decls.insert(n.clone(), d);
            let loc = self.g.declare(d, inner.contains(n));
            // A captured `let` gets its cell up front, holding "unbound".
            self.g.box_param(loc);
        }
        self.envs.push(env);
        self.block_value(body);
        self.g.emit(Op::Return);
        self.envs.pop();
        let p = self.g.pop_fn();
        self.g.emit(Op::Closure(p));
    }

    fn fresh(&mut self) -> DeclId {
        self.next_decl += 1;
        self.next_decl
    }
}

/// Compile one Monkey source file into a single-module program.
pub fn compile(src: &str, module_name: &str) -> Result<Program, Vec<String>> {
    let prog = parse(src).map_err(|errs| {
        let mut out = vec!["parser errors:".to_string()];
        out.extend(errs.into_iter().map(|e| format!("\t{e}")));
        out
    })?;
    let prog = monkey_macro::expand(prog).map_err(|e| vec![format!("macro expansion error: {e}")])?;
    let mut c = Compiler { g: Gen::new(), envs: Vec::new(), next_decl: 0 };
    c.g.push_fn(FnBuilder::new(module_name, Lang::Monkey, 0, 0));
    for s in &prog {
        c.stmt(s);
    }
    c.g.emit(Op::Nil);
    c.g.emit(Op::Return);
    let entry = c.g.pop_fn();
    Ok(c.g.into_program(vec![Module { name: module_name.to_string(), lang: Lang::Monkey, entry }]))
}
