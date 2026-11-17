//! Lox (from Robert Nystrom's *Crafting Interpreters*): scanner, parser,
//! resolver and code generator.  Diagnostics follow jlox's wording and
//! format so the book's test suite can be used to check conformance.

use crate::bytecode::{Lang, Module, Op, Program, FLAG_INITIALIZER, FLAG_RECEIVER};
use crate::emit::{DeclId, FnBuilder, Gen, VarLoc};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

// ---------------------------------------------------------------------------
// Scanner

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum T {
    LParen,
    RParen,
    LBrace,
    RBrace,
    Comma,
    Dot,
    Minus,
    Plus,
    Semicolon,
    Slash,
    Star,
    Bang,
    BangEqual,
    Equal,
    EqualEqual,
    Greater,
    GreaterEqual,
    Less,
    LessEqual,
    Identifier,
    String,
    Number,
    And,
    Class,
    Else,
    False,
    Fun,
    For,
    If,
    Nil,
    Or,
    Print,
    Return,
    Super,
    This,
    True,
    Var,
    While,
    Eof,
}

#[derive(Clone, Debug)]
struct Token {
    t: T,
    lexeme: Rc<str>,
    /// String contents or number value, for literals.
    lit: Lit,
    line: u32,
}

#[derive(Clone, Debug)]
enum Lit {
    None,
    Num(f64),
    Str(Rc<str>),
}

fn keyword(s: &str) -> Option<T> {
    Some(match s {
        "and" => T::And,
        "class" => T::Class,
        "else" => T::Else,
        "false" => T::False,
        "for" => T::For,
        "fun" => T::Fun,
        "if" => T::If,
        "nil" => T::Nil,
        "or" => T::Or,
        "print" => T::Print,
        "return" => T::Return,
        "super" => T::Super,
        "this" => T::This,
        "true" => T::True,
        "var" => T::Var,
        "while" => T::While,
        _ => return None,
    })
}

fn scan(src: &str, errors: &mut Vec<String>) -> Vec<Token> {
    let b = src.as_bytes();
    let mut toks = Vec::new();
    let mut i = 0;
    let mut line = 1u32;
    while i < b.len() {
        let start = i;
        let c = b[i];
        i += 1;
        let mut push = |t: T, end: usize, lit: Lit, line: u32| {
            toks.push(Token { t, lexeme: Rc::from(&src[start..end]), lit, line });
        };
        let next_is = |i: usize, ch: u8| i < b.len() && b[i] == ch;
        match c {
            b'(' => push(T::LParen, i, Lit::None, line),
            b')' => push(T::RParen, i, Lit::None, line),
            b'{' => push(T::LBrace, i, Lit::None, line),
            b'}' => push(T::RBrace, i, Lit::None, line),
            b',' => push(T::Comma, i, Lit::None, line),
            b'.' => push(T::Dot, i, Lit::None, line),
            b'-' => push(T::Minus, i, Lit::None, line),
            b'+' => push(T::Plus, i, Lit::None, line),
            b';' => push(T::Semicolon, i, Lit::None, line),
            b'*' => push(T::Star, i, Lit::None, line),
            b'!' | b'=' | b'<' | b'>' => {
                let eq = next_is(i, b'=');
                if eq {
                    i += 1;
                }
                let t = match (c, eq) {
                    (b'!', false) => T::Bang,
                    (b'!', true) => T::BangEqual,
                    (b'=', false) => T::Equal,
                    (b'=', true) => T::EqualEqual,
                    (b'<', false) => T::Less,
                    (b'<', true) => T::LessEqual,
                    (b'>', false) => T::Greater,
                    _ => T::GreaterEqual,
                };
                push(t, i, Lit::None, line)
            }
            b'/' => {
                if next_is(i, b'/') {
                    while i < b.len() && b[i] != b'\n' {
                        i += 1;
                    }
                } else {
                    push(T::Slash, i, Lit::None, line)
                }
            }
            b' ' | b'\r' | b'\t' => {}
            b'\n' => line += 1,
            b'"' => {
                while i < b.len() && b[i] != b'"' {
                    if b[i] == b'\n' {
                        line += 1;
                    }
                    i += 1;
                }
                if i >= b.len() {
                    errors.push(format!("[line {line}] Error: Unterminated string."));
                } else {
                    i += 1;
                    let s = Lit::Str(Rc::from(&src[start + 1..i - 1]));
                    push(T::String, i, s, line)
                }
            }
            b'0'..=b'9' => {
                while i < b.len() && b[i].is_ascii_digit() {
                    i += 1;
                }
                if i + 1 < b.len() && b[i] == b'.' && b[i + 1].is_ascii_digit() {
                    i += 1;
                    while i < b.len() && b[i].is_ascii_digit() {
                        i += 1;
                    }
                }
                let n: f64 = src[start..i].parse().expect("digits parse as f64");
                push(T::Number, i, Lit::Num(n), line)
            }
            c if c.is_ascii_alphabetic() || c == b'_' => {
                while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
                    i += 1;
                }
                let t = keyword(&src[start..i]).unwrap_or(T::Identifier);
                push(t, i, Lit::None, line)
            }
            _ => {
                // Skip the rest of a multi-byte character so we report once.
                while i < b.len() && (b[i] & 0xC0) == 0x80 {
                    i += 1;
                }
                errors.push(format!("[line {line}] Error: Unexpected character."))
            }
        }
    }
    toks.push(Token { t: T::Eof, lexeme: Rc::from(""), lit: Lit::None, line });
    toks
}

// ---------------------------------------------------------------------------
// Syntax tree

type ExprId = usize;

#[derive(Debug)]
enum Expr {
    Nil,
    Bool(bool),
    Num(f64),
    Str(Rc<str>),
    Grouping(Box<Expr>),
    Unary(Token, Box<Expr>),
    Binary(Box<Expr>, Token, Box<Expr>),
    Logical(Box<Expr>, Token, Box<Expr>),
    Variable(Token, ExprId),
    Assign(Token, Box<Expr>, ExprId),
    Call(Box<Expr>, Token, Vec<Expr>),
    Get(Box<Expr>, Token),
    Set(Box<Expr>, Token, Box<Expr>),
    This(Token, ExprId),
    /// `super.method`: keyword, method name, id for `super`, id for `this`.
    Super(Token, Token, ExprId, ExprId),
}

#[derive(Debug)]
struct Function {
    name: Token,
    params: Vec<Token>,
    body: Vec<Stmt>,
    /// Declaration ids: the function's own name, its parameters, and `this`.
    decl: DeclId,
    param_decls: Vec<DeclId>,
    this_decl: DeclId,
    id: usize,
}

#[derive(Debug)]
enum Stmt {
    Expr(Expr),
    Print(Expr),
    Var(Token, Option<Expr>, DeclId),
    Block(Vec<Stmt>),
    If(Expr, Box<Stmt>, Option<Box<Stmt>>),
    While(Expr, Box<Stmt>),
    Function(Box<Function>),
    Return(Token, Option<Expr>),
    Class { name: Token, decl: DeclId, superclass: Option<Expr>, super_decl: DeclId, methods: Vec<Function> },
}

// ---------------------------------------------------------------------------
// Parser

struct Parser {
    toks: Vec<Token>,
    pos: usize,
    errors: Vec<String>,
    next_id: usize,
}

struct ParseError;
type PResult<T> = Result<T, ParseError>;

fn error_at(tok: &Token, msg: &str) -> String {
    if tok.t == T::Eof {
        format!("[line {}] Error at end: {msg}", tok.line)
    } else {
        format!("[line {}] Error at '{}': {msg}", tok.line, tok.lexeme)
    }
}

impl Parser {
    fn id(&mut self) -> usize {
        self.next_id += 1;
        self.next_id
    }
    fn peek(&self) -> &Token {
        &self.toks[self.pos]
    }
    fn previous(&self) -> &Token {
        &self.toks[self.pos - 1]
    }
    fn check(&self, t: T) -> bool {
        self.peek().t == t
    }
    fn at_end(&self) -> bool {
        self.check(T::Eof)
    }
    fn advance(&mut self) -> Token {
        if !self.at_end() {
            self.pos += 1;
        }
        self.previous().clone()
    }
    fn matches(&mut self, ts: &[T]) -> bool {
        if ts.contains(&self.peek().t) {
            self.advance();
            true
        } else {
            false
        }
    }
    fn consume(&mut self, t: T, msg: &str) -> PResult<Token> {
        if self.check(t) {
            Ok(self.advance())
        } else {
            Err(self.error(&self.peek().clone(), msg))
        }
    }
    fn error(&mut self, tok: &Token, msg: &str) -> ParseError {
        self.errors.push(error_at(tok, msg));
        ParseError
    }
    fn synchronize(&mut self) {
        self.advance();
        while !self.at_end() {
            if self.previous().t == T::Semicolon {
                return;
            }
            if matches!(self.peek().t, T::Class | T::Fun | T::Var | T::For | T::If | T::While | T::Print | T::Return) {
                return;
            }
            self.advance();
        }
    }

    fn program(&mut self) -> Vec<Stmt> {
        let mut stmts = Vec::new();
        while !self.at_end() {
            if let Some(s) = self.declaration() {
                stmts.push(s)
            }
        }
        stmts
    }

    fn declaration(&mut self) -> Option<Stmt> {
        let r = if self.matches(&[T::Class]) {
            self.class_declaration()
        } else if self.matches(&[T::Fun]) {
            self.function("function").map(|f| Stmt::Function(Box::new(f)))
        } else if self.matches(&[T::Var]) {
            self.var_declaration()
        } else {
            self.statement()
        };
        match r {
            Ok(s) => Some(s),
            Err(ParseError) => {
                self.synchronize();
                None
            }
        }
    }

    fn class_declaration(&mut self) -> PResult<Stmt> {
        let name = self.consume(T::Identifier, "Expect class name.")?;
        let superclass = if self.matches(&[T::Less]) {
            let n = self.consume(T::Identifier, "Expect superclass name.")?;
            let id = self.id();
            Some(Expr::Variable(n, id))
        } else {
            None
        };
        self.consume(T::LBrace, "Expect '{' before class body.")?;
        let mut methods = Vec::new();
        while !self.check(T::RBrace) && !self.at_end() {
            methods.push(self.function("method")?);
        }
        self.consume(T::RBrace, "Expect '}' after class body.")?;
        let (decl, super_decl) = (self.id(), self.id());
        Ok(Stmt::Class { name, decl, superclass, super_decl, methods })
    }

    fn function(&mut self, kind: &str) -> PResult<Function> {
        let name = self.consume(T::Identifier, &format!("Expect {kind} name."))?;
        self.consume(T::LParen, &format!("Expect '(' after {kind} name."))?;
        let mut params = Vec::new();
        if !self.check(T::RParen) {
            loop {
                if params.len() >= 255 {
                    let t = self.peek().clone();
                    self.error(&t, "Can't have more than 255 parameters.");
                }
                params.push(self.consume(T::Identifier, "Expect parameter name.")?);
                if !self.matches(&[T::Comma]) {
                    break;
                }
            }
        }
        self.consume(T::RParen, "Expect ')' after parameters.")?;
        self.consume(T::LBrace, &format!("Expect '{{' before {kind} body."))?;
        let body = self.block()?;
        let param_decls = params.iter().map(|_| self.id()).collect();
        Ok(Function { name, params, body, decl: self.id(), param_decls, this_decl: self.id(), id: self.id() })
    }

    fn var_declaration(&mut self) -> PResult<Stmt> {
        let name = self.consume(T::Identifier, "Expect variable name.")?;
        let init = if self.matches(&[T::Equal]) { Some(self.expression()?) } else { None };
        self.consume(T::Semicolon, "Expect ';' after variable declaration.")?;
        let id = self.id();
        Ok(Stmt::Var(name, init, id))
    }

    fn statement(&mut self) -> PResult<Stmt> {
        if self.matches(&[T::For]) {
            return self.for_statement();
        }
        if self.matches(&[T::If]) {
            self.consume(T::LParen, "Expect '(' after 'if'.")?;
            let cond = self.expression()?;
            self.consume(T::RParen, "Expect ')' after if condition.")?;
            let then = Box::new(self.statement()?);
            let els = if self.matches(&[T::Else]) { Some(Box::new(self.statement()?)) } else { None };
            return Ok(Stmt::If(cond, then, els));
        }
        if self.matches(&[T::Print]) {
            let v = self.expression()?;
            self.consume(T::Semicolon, "Expect ';' after value.")?;
            return Ok(Stmt::Print(v));
        }
        if self.matches(&[T::Return]) {
            let kw = self.previous().clone();
            let v = if !self.check(T::Semicolon) { Some(self.expression()?) } else { None };
            self.consume(T::Semicolon, "Expect ';' after return value.")?;
            return Ok(Stmt::Return(kw, v));
        }
        if self.matches(&[T::While]) {
            self.consume(T::LParen, "Expect '(' after 'while'.")?;
            let cond = self.expression()?;
            self.consume(T::RParen, "Expect ')' after condition.")?;
            let body = self.statement()?;
            return Ok(Stmt::While(cond, Box::new(body)));
        }
        if self.matches(&[T::LBrace]) {
            return Ok(Stmt::Block(self.block()?));
        }
        let e = self.expression()?;
        self.consume(T::Semicolon, "Expect ';' after expression.")?;
        Ok(Stmt::Expr(e))
    }

    /// `for` desugars to `while`, exactly as in jlox.
    fn for_statement(&mut self) -> PResult<Stmt> {
        self.consume(T::LParen, "Expect '(' after 'for'.")?;
        let init = if self.matches(&[T::Semicolon]) {
            None
        } else if self.matches(&[T::Var]) {
            Some(self.var_declaration()?)
        } else {
            let e = self.expression()?;
            self.consume(T::Semicolon, "Expect ';' after expression.")?;
            Some(Stmt::Expr(e))
        };
        let cond = if !self.check(T::Semicolon) { Some(self.expression()?) } else { None };
        self.consume(T::Semicolon, "Expect ';' after loop condition.")?;
        let incr = if !self.check(T::RParen) { Some(self.expression()?) } else { None };
        self.consume(T::RParen, "Expect ')' after for clauses.")?;
        let mut body = self.statement()?;
        if let Some(incr) = incr {
            body = Stmt::Block(vec![body, Stmt::Expr(incr)]);
        }
        body = Stmt::While(cond.unwrap_or(Expr::Bool(true)), Box::new(body));
        if let Some(init) = init {
            body = Stmt::Block(vec![init, body]);
        }
        Ok(body)
    }

    fn block(&mut self) -> PResult<Vec<Stmt>> {
        let mut stmts = Vec::new();
        while !self.check(T::RBrace) && !self.at_end() {
            if let Some(s) = self.declaration() {
                stmts.push(s)
            }
        }
        self.consume(T::RBrace, "Expect '}' after block.")?;
        Ok(stmts)
    }

    fn expression(&mut self) -> PResult<Expr> {
        self.assignment()
    }

    fn assignment(&mut self) -> PResult<Expr> {
        let e = self.or()?;
        if self.matches(&[T::Equal]) {
            let equals = self.previous().clone();
            let value = self.assignment()?;
            return Ok(match e {
                Expr::Variable(name, _) => {
                    let id = self.id();
                    Expr::Assign(name, Box::new(value), id)
                }
                Expr::Get(obj, name) => Expr::Set(obj, name, Box::new(value)),
                other => {
                    self.error(&equals, "Invalid assignment target.");
                    other
                }
            });
        }
        Ok(e)
    }

    fn or(&mut self) -> PResult<Expr> {
        let mut e = self.and()?;
        while self.matches(&[T::Or]) {
            let op = self.previous().clone();
            let r = self.and()?;
            e = Expr::Logical(Box::new(e), op, Box::new(r));
        }
        Ok(e)
    }

    fn and(&mut self) -> PResult<Expr> {
        let mut e = self.equality()?;
        while self.matches(&[T::And]) {
            let op = self.previous().clone();
            let r = self.equality()?;
            e = Expr::Logical(Box::new(e), op, Box::new(r));
        }
        Ok(e)
    }

    fn binary_level(&mut self, ops: &[T], next: fn(&mut Parser) -> PResult<Expr>) -> PResult<Expr> {
        let mut e = next(self)?;
        while self.matches(ops) {
            let op = self.previous().clone();
            let r = next(self)?;
            e = Expr::Binary(Box::new(e), op, Box::new(r));
        }
        Ok(e)
    }

    fn equality(&mut self) -> PResult<Expr> {
        self.binary_level(&[T::BangEqual, T::EqualEqual], Parser::comparison)
    }
    fn comparison(&mut self) -> PResult<Expr> {
        self.binary_level(&[T::Greater, T::GreaterEqual, T::Less, T::LessEqual], Parser::term)
    }
    fn term(&mut self) -> PResult<Expr> {
        self.binary_level(&[T::Minus, T::Plus], Parser::factor)
    }
    fn factor(&mut self) -> PResult<Expr> {
        self.binary_level(&[T::Slash, T::Star], Parser::unary)
    }

    fn unary(&mut self) -> PResult<Expr> {
        if self.matches(&[T::Bang, T::Minus]) {
            let op = self.previous().clone();
            let r = self.unary()?;
            return Ok(Expr::Unary(op, Box::new(r)));
        }
        self.call()
    }

    fn call(&mut self) -> PResult<Expr> {
        let mut e = self.primary()?;
        loop {
            if self.matches(&[T::LParen]) {
                let mut args = Vec::new();
                if !self.check(T::RParen) {
                    loop {
                        if args.len() >= 255 {
                            let t = self.peek().clone();
                            self.error(&t, "Can't have more than 255 arguments.");
                        }
                        args.push(self.expression()?);
                        if !self.matches(&[T::Comma]) {
                            break;
                        }
                    }
                }
                let paren = self.consume(T::RParen, "Expect ')' after arguments.")?;
                e = Expr::Call(Box::new(e), paren, args);
            } else if self.matches(&[T::Dot]) {
                let name = self.consume(T::Identifier, "Expect property name after '.'.")?;
                e = Expr::Get(Box::new(e), name);
            } else {
                return Ok(e);
            }
        }
    }

    fn primary(&mut self) -> PResult<Expr> {
        let tok = self.peek().clone();
        let e = match tok.t {
            T::False => Expr::Bool(false),
            T::True => Expr::Bool(true),
            T::Nil => Expr::Nil,
            T::Number => match tok.lit {
                Lit::Num(n) => Expr::Num(n),
                _ => unreachable!(),
            },
            T::String => match &tok.lit {
                Lit::Str(s) => Expr::Str(s.clone()),
                _ => unreachable!(),
            },
            T::Super => {
                self.advance();
                self.consume(T::Dot, "Expect '.' after 'super'.")?;
                let method = self.consume(T::Identifier, "Expect superclass method name.")?;
                let (a, b) = (self.id(), self.id());
                return Ok(Expr::Super(tok, method, a, b));
            }
            T::This => {
                let id = self.id();
                Expr::This(tok.clone(), id)
            }
            T::Identifier => {
                let id = self.id();
                Expr::Variable(tok.clone(), id)
            }
            T::LParen => {
                self.advance();
                let e = self.expression()?;
                self.consume(T::RParen, "Expect ')' after expression.")?;
                return Ok(Expr::Grouping(Box::new(e)));
            }
            _ => return Err(self.error(&tok, "Expect expression.")),
        };
        self.advance();
        Ok(e)
    }
}

// ---------------------------------------------------------------------------
// Resolver: static scoping, as in jlox, plus closure-capture analysis.

#[derive(Clone, Copy, PartialEq)]
enum FnKind {
    None,
    Function,
    Method,
    Initializer,
}

#[derive(Clone, Copy, PartialEq)]
enum ClassKind {
    None,
    Class,
    Subclass,
}

struct Resolver {
    /// Innermost last; each maps name -> (decl, defined?).
    scopes: Vec<HashMap<Rc<str>, (DeclId, bool)>>,
    /// Function id owning each declaration.
    owner: HashMap<DeclId, usize>,
    /// Current function id (0 = module top level).
    fn_stack: Vec<usize>,
    /// Resolution of variable uses to local declarations (absent = global).
    uses: HashMap<ExprId, DeclId>,
    captured: HashSet<DeclId>,
    fn_kind: FnKind,
    class_kind: ClassKind,
    errors: Vec<String>,
}

impl Resolver {
    fn begin(&mut self) {
        self.scopes.push(HashMap::new())
    }
    fn end(&mut self) {
        self.scopes.pop();
    }
    fn current_fn(&self) -> usize {
        *self.fn_stack.last().unwrap()
    }
    fn declare(&mut self, name: &Token, decl: DeclId) {
        let f = self.current_fn();
        let Some(scope) = self.scopes.last_mut() else { return };
        if scope.contains_key(&name.lexeme) {
            self.errors.push(error_at(name, "Already a variable with this name in this scope."));
        }
        scope.insert(name.lexeme.clone(), (decl, false));
        self.owner.insert(decl, f);
    }
    fn define(&mut self, name: &Token) {
        if let Some(scope) = self.scopes.last_mut() {
            if let Some(e) = scope.get_mut(&name.lexeme) {
                e.1 = true;
            }
        }
    }
    /// Declare and define a synthetic name (`this`, `super`).
    fn define_synthetic(&mut self, name: &str, decl: DeclId) {
        let f = self.current_fn();
        self.scopes.last_mut().unwrap().insert(Rc::from(name), (decl, true));
        self.owner.insert(decl, f);
    }
    fn resolve_local(&mut self, name: &str, id: ExprId) {
        for scope in self.scopes.iter().rev() {
            if let Some(&(decl, _)) = scope.get(name) {
                self.uses.insert(id, decl);
                if self.owner[&decl] != self.current_fn() {
                    self.captured.insert(decl);
                }
                return;
            }
        }
    }

    fn stmts(&mut self, ss: &[Stmt]) {
        for s in ss {
            self.stmt(s)
        }
    }

    fn stmt(&mut self, s: &Stmt) {
        match s {
            Stmt::Block(ss) => {
                self.begin();
                self.stmts(ss);
                self.end();
            }
            Stmt::Var(name, init, decl) => {
                self.declare(name, *decl);
                if let Some(e) = init {
                    self.expr(e);
                }
                self.define(name);
            }
            Stmt::Function(f) => {
                self.declare(&f.name, f.decl);
                self.define(&f.name);
                self.function(f, FnKind::Function);
            }
            Stmt::Expr(e) | Stmt::Print(e) => self.expr(e),
            Stmt::If(c, t, e) => {
                self.expr(c);
                self.stmt(t);
                if let Some(e) = e {
                    self.stmt(e)
                }
            }
            Stmt::While(c, b) => {
                self.expr(c);
                self.stmt(b);
            }
            Stmt::Return(kw, v) => {
                if self.fn_kind == FnKind::None {
                    self.errors.push(error_at(kw, "Can't return from top-level code."));
                }
                if let Some(v) = v {
                    if self.fn_kind == FnKind::Initializer {
                        self.errors.push(error_at(kw, "Can't return a value from an initializer."));
                    }
                    self.expr(v);
                }
            }
            Stmt::Class { name, decl, superclass, super_decl, methods } => {
                let enclosing = self.class_kind;
                self.class_kind = ClassKind::Class;
                self.declare(name, *decl);
                self.define(name);
                if let Some(sc) = superclass {
                    if let Expr::Variable(sname, _) = sc {
                        if sname.lexeme == name.lexeme {
                            self.errors.push(error_at(sname, "A class can't inherit from itself."));
                        }
                    }
                    self.class_kind = ClassKind::Subclass;
                    self.expr(sc);
                    self.begin();
                    self.define_synthetic("super", *super_decl);
                }
                for m in methods {
                    let kind = if &*m.name.lexeme == "init" { FnKind::Initializer } else { FnKind::Method };
                    self.function(m, kind);
                }
                if superclass.is_some() {
                    self.end();
                }
                self.class_kind = enclosing;
            }
        }
    }

    fn function(&mut self, f: &Function, kind: FnKind) {
        let enclosing = self.fn_kind;
        self.fn_kind = kind;
        self.fn_stack.push(f.id);
        if matches!(kind, FnKind::Method | FnKind::Initializer) {
            // jlox binds `this` in a scope around each method.
            self.begin();
            self.define_synthetic("this", f.this_decl);
        }
        self.begin();
        for (p, &d) in f.params.iter().zip(&f.param_decls) {
            self.declare(p, d);
            self.define(p);
        }
        self.stmts(&f.body);
        self.end();
        if matches!(kind, FnKind::Method | FnKind::Initializer) {
            self.end();
        }
        self.fn_stack.pop();
        self.fn_kind = enclosing;
    }

    fn expr(&mut self, e: &Expr) {
        match e {
            Expr::Nil | Expr::Bool(_) | Expr::Num(_) | Expr::Str(_) => {}
            Expr::Grouping(e) | Expr::Unary(_, e) | Expr::Get(e, _) => self.expr(e),
            Expr::Binary(a, _, b) | Expr::Logical(a, _, b) | Expr::Set(a, _, b) => {
                self.expr(a);
                self.expr(b);
            }
            Expr::Variable(name, id) => {
                if let Some(scope) = self.scopes.last() {
                    if let Some(&(_, false)) = scope.get(&name.lexeme) {
                        self.errors.push(error_at(name, "Can't read local variable in its own initializer."));
                    }
                }
                self.resolve_local(&name.lexeme, *id);
            }
            Expr::Assign(name, v, id) => {
                self.expr(v);
                self.resolve_local(&name.lexeme, *id);
            }
            Expr::Call(c, _, args) => {
                self.expr(c);
                for a in args {
                    self.expr(a)
                }
            }
            Expr::This(kw, id) => {
                if self.class_kind == ClassKind::None {
                    self.errors.push(error_at(kw, "Can't use 'this' outside of a class."));
                } else {
                    self.resolve_local("this", *id);
                }
            }
            Expr::Super(kw, _, id, this_id) => {
                match self.class_kind {
                    ClassKind::None => self.errors.push(error_at(kw, "Can't use 'super' outside of a class.")),
                    ClassKind::Class => {
                        self.errors.push(error_at(kw, "Can't use 'super' in a class with no superclass."))
                    }
                    ClassKind::Subclass => {}
                }
                self.resolve_local("super", *id);
                self.resolve_local("this", *this_id);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Code generation

struct Compiler {
    g: Gen,
    uses: HashMap<ExprId, DeclId>,
    captured: HashSet<DeclId>,
    /// Block nesting depth within the current function; top-level
    /// declarations at depth 0 of the module are globals.
    scope_depth: usize,
    /// Whether the function being compiled is an initializer.
    in_initializer: bool,
    /// `this` declarations of the enclosing methods, innermost last.
    this_decl: Vec<DeclId>,
}

impl Compiler {
    fn line(&mut self, t: &Token) {
        self.g.b().line = t.line;
    }

    fn is_global_scope(&self) -> bool {
        self.g.fns.len() == 1 && self.scope_depth == 0
    }

    fn declare(&mut self, decl: DeclId) -> VarLoc {
        let captured = self.captured.contains(&decl);
        self.g.declare(decl, captured)
    }

    /// Store the top of stack into a newly declared variable named `name`.
    fn define(&mut self, name: &Token, decl: DeclId) {
        if self.is_global_scope() {
            let k = self.g.pool.str(&name.lexeme);
            self.g.emit(Op::DefineGlobal(k));
        } else {
            let loc = self.declare(decl);
            self.g.emit_init(loc);
        }
    }

    fn get_var(&mut self, name: &str, id: ExprId) {
        match self.uses.get(&id).and_then(|&d| self.g.resolve(d)) {
            Some(loc) => self.g.emit_get(loc),
            None => {
                let k = self.g.pool.str(name);
                self.g.emit(Op::GetGlobal(k))
            }
        }
    }

    fn set_var(&mut self, name: &str, id: ExprId) {
        match self.uses.get(&id).and_then(|&d| self.g.resolve(d)) {
            Some(loc) => self.g.emit_set(loc),
            None => {
                let k = self.g.pool.str(name);
                self.g.emit(Op::SetGlobal(k))
            }
        }
    }

    fn stmts(&mut self, ss: &[Stmt]) {
        for s in ss {
            self.stmt(s)
        }
    }

    fn stmt(&mut self, s: &Stmt) {
        match s {
            Stmt::Expr(e) => {
                self.expr(e);
                self.g.emit(Op::Pop);
            }
            Stmt::Print(e) => {
                self.expr(e);
                self.g.emit(Op::Print);
            }
            Stmt::Var(name, init, decl) => {
                self.line(name);
                match init {
                    Some(e) => self.expr(e),
                    None => self.g.emit(Op::Nil),
                }
                self.define(name, *decl);
            }
            Stmt::Block(ss) => {
                self.scope_depth += 1;
                self.stmts(ss);
                self.scope_depth -= 1;
            }
            Stmt::If(c, t, e) => {
                self.expr(c);
                let (else_l, end_l) = (self.g.b().new_label(), self.g.b().new_label());
                self.g.b().jump(Op::JumpIfFalse, else_l);
                self.stmt(t);
                self.g.b().jump(Op::Jump, end_l);
                self.g.b().place(else_l);
                if let Some(e) = e {
                    self.stmt(e);
                }
                self.g.b().place(end_l);
            }
            Stmt::While(c, body) => {
                let (top, end) = (self.g.b().new_label(), self.g.b().new_label());
                self.g.b().place(top);
                self.expr(c);
                self.g.b().jump(Op::JumpIfFalse, end);
                self.stmt(body);
                self.g.b().jump(Op::Jump, top);
                self.g.b().place(end);
            }
            Stmt::Function(f) => {
                self.line(&f.name);
                if self.is_global_scope() {
                    self.function(f, 0);
                    let k = self.g.pool.str(&f.name.lexeme);
                    self.g.emit(Op::DefineGlobal(k));
                } else {
                    // Declare first so the function can refer to itself.
                    let loc = self.declare(f.decl);
                    if let VarLoc::Cell(_) = loc {
                        self.g.emit(Op::Nil);
                        self.g.emit_init(loc);
                        self.function(f, 0);
                        self.g.emit_set(loc);
                    } else {
                        self.function(f, 0);
                        self.g.emit_init(loc);
                    }
                }
            }
            Stmt::Return(kw, v) => {
                self.line(kw);
                if self.in_initializer {
                    self.emit_this();
                } else {
                    match v {
                        Some(v) => self.expr(v),
                        None => self.g.emit(Op::Nil),
                    }
                }
                self.g.emit(Op::Return);
            }
            Stmt::Class { name, decl, superclass, super_decl, methods } => {
                self.line(name);
                let kname = self.g.pool.str(&name.lexeme);
                self.g.emit(Op::Class(kname));
                let class_loc = if self.is_global_scope() {
                    self.g.emit(Op::DefineGlobal(kname));
                    None
                } else {
                    let loc = self.declare(*decl);
                    self.g.emit_init(loc);
                    Some(loc)
                };
                let load_class = |c: &mut Compiler| match class_loc {
                    Some(loc) => c.g.emit_get(loc),
                    None => c.g.emit(Op::GetGlobal(kname)),
                };
                if let Some(sc) = superclass {
                    self.scope_depth += 1;
                    self.expr(sc);
                    self.g.emit(Op::Dup);
                    let loc = self.declare(*super_decl);
                    self.g.emit_init(loc);
                    load_class(self);
                    self.line(name);
                    self.g.emit(Op::Inherit);
                } else {
                    load_class(self);
                }
                for m in methods {
                    let init = &*m.name.lexeme == "init";
                    let flags = FLAG_RECEIVER | if init { FLAG_INITIALIZER } else { 0 };
                    self.function(m, flags);
                    let k = self.g.pool.str(&m.name.lexeme);
                    self.g.emit(Op::Method(k));
                }
                self.g.emit(Op::Pop);
                if superclass.is_some() {
                    self.scope_depth -= 1;
                }
            }
        }
    }

    fn emit_this(&mut self) {
        // `this` always lives in slot 0 of the current method, or is
        // captured from the enclosing method.
        let decl = self.this_decl.last().copied().expect("this inside method");
        let loc = self.g.resolve(decl).expect("this is resolvable");
        self.g.emit_get(loc);
    }

    /// Compile a function body and emit a `Closure` for it.
    fn function(&mut self, f: &Function, flags: u8) {
        let mut b = FnBuilder::new(&f.name.lexeme, Lang::Lox, f.params.len() as u32, flags);
        b.line = f.name.line;
        self.g.push_fn(b);
        let saved_depth = std::mem::replace(&mut self.scope_depth, 0);
        let saved_init = std::mem::replace(&mut self.in_initializer, flags & FLAG_INITIALIZER != 0);
        let receiver = flags & FLAG_RECEIVER != 0;
        if receiver {
            let loc = self.g.bind(f.this_decl, 0, self.captured.contains(&f.this_decl));
            self.g.box_param(loc);
            self.this_decl.push(f.this_decl);
        }
        for (i, &d) in f.param_decls.iter().enumerate() {
            let loc = self.g.bind(d, i as u32 + receiver as u32, self.captured.contains(&d));
            self.g.box_param(loc);
        }
        self.stmts(&f.body);
        if self.g.b().is_reachable() {
            if self.in_initializer {
                self.emit_this();
            } else {
                self.g.emit(Op::Nil);
            }
            self.g.emit(Op::Return);
        }
        if receiver {
            self.this_decl.pop();
        }
        self.in_initializer = saved_init;
        self.scope_depth = saved_depth;
        let p = self.g.pop_fn();
        self.g.emit(Op::Closure(p));
    }

    fn expr(&mut self, e: &Expr) {
        match e {
            Expr::Nil => self.g.emit(Op::Nil),
            Expr::Bool(true) => self.g.emit(Op::True),
            Expr::Bool(false) => self.g.emit(Op::False),
            Expr::Num(n) => {
                let k = self.g.pool.float(*n);
                self.g.emit(Op::Const(k))
            }
            Expr::Str(s) => {
                let k = self.g.pool.str(s);
                self.g.emit(Op::Const(k))
            }
            Expr::Grouping(e) => self.expr(e),
            Expr::Unary(op, e) => {
                self.expr(e);
                self.line(op);
                self.g.emit(if op.t == T::Minus { Op::Neg } else { Op::Not });
            }
            Expr::Binary(a, op, b) => {
                self.expr(a);
                self.expr(b);
                self.line(op);
                let ins = match op.t {
                    T::Plus => Op::Add,
                    T::Minus => Op::Sub,
                    T::Star => Op::Mul,
                    T::Slash => Op::FDiv,
                    T::Greater => Op::Gt,
                    T::GreaterEqual => Op::Ge,
                    T::Less => Op::Lt,
                    T::LessEqual => Op::Le,
                    T::EqualEqual => Op::Eq,
                    T::BangEqual => Op::Ne,
                    _ => unreachable!("binary operator"),
                };
                self.g.emit(ins);
            }
            Expr::Logical(a, op, b) => {
                self.expr(a);
                let end = self.g.b().new_label();
                self.g.emit(Op::Dup);
                self.g.b().jump(if op.t == T::And { Op::JumpIfFalse } else { Op::JumpIfTrue }, end);
                self.g.emit(Op::Pop);
                self.expr(b);
                self.g.b().place(end);
            }
            Expr::Variable(name, id) => {
                self.line(name);
                self.get_var(&name.lexeme, *id)
            }
            Expr::Assign(name, v, id) => {
                self.expr(v);
                self.line(name);
                self.g.emit(Op::Dup);
                self.set_var(&name.lexeme, *id);
            }
            Expr::Call(callee, paren, args) => {
                self.expr(callee);
                for a in args {
                    self.expr(a);
                }
                self.line(paren);
                self.g.emit(Op::Call(args.len() as u32));
            }
            Expr::Get(obj, name) => {
                self.expr(obj);
                self.line(name);
                let k = self.g.pool.str(&name.lexeme);
                self.g.emit(Op::GetProp(k));
            }
            Expr::Set(obj, name, v) => {
                self.expr(obj);
                self.expr(v);
                self.line(name);
                let k = self.g.pool.str(&name.lexeme);
                self.g.emit(Op::SetProp(k));
            }
            Expr::This(kw, id) => {
                self.line(kw);
                self.get_var("this", *id)
            }
            Expr::Super(kw, method, id, this_id) => {
                self.line(kw);
                self.get_var("this", *this_id);
                self.get_var("super", *id);
                self.line(method);
                let k = self.g.pool.str(&method.lexeme);
                self.g.emit(Op::GetSuper(k));
            }
        }
    }
}

/// Compile one Lox source file into a single-module program.
pub fn compile(src: &str, module_name: &str) -> Result<Program, Vec<String>> {
    let mut errors = Vec::new();
    let toks = scan(src, &mut errors);
    let mut p = Parser { toks, pos: 0, errors: Vec::new(), next_id: 0 };
    let stmts = p.program();
    // Like jlox: all scanner errors, then all parser errors.
    errors.extend(p.errors);
    if !errors.is_empty() {
        return Err(errors);
    }
    let mut r = Resolver {
        scopes: Vec::new(),
        owner: HashMap::new(),
        fn_stack: vec![0],
        uses: HashMap::new(),
        captured: HashSet::new(),
        fn_kind: FnKind::None,
        class_kind: ClassKind::None,
        errors: Vec::new(),
    };
    r.stmts(&stmts);
    if !r.errors.is_empty() {
        return Err(r.errors);
    }
    let mut c = Compiler {
        g: Gen::new(),
        uses: r.uses,
        captured: r.captured,
        scope_depth: 0,
        in_initializer: false,
        this_decl: Vec::new(),
    };
    c.g.push_fn(FnBuilder::new(module_name, Lang::Lox, 0, 0));
    c.stmts(&stmts);
    c.g.emit(Op::Nil);
    c.g.emit(Op::Return);
    let entry = c.g.pop_fn();
    Ok(c.g.into_program(vec![Module { name: module_name.to_string(), lang: Lang::Lox, entry }]))
}
