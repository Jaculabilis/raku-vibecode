//! Monkey macros, from *The Lost Chapter*: `macro` literals bound by
//! top-level `let`s are removed from the program, and calls to them are
//! replaced, at compile time, by the syntax tree their body returns.
//!
//! Macro bodies are ordinary Monkey code, so expansion needs an evaluator;
//! this module contains a small tree-walking one that mirrors the book's
//! (`quote`/`unquote` included).  It runs only inside the compiler.

use super::monkey::{fn_display, Expr, Stmt};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

/// A piece of a quoted expression's text.
pub enum Piece {
    Text(String),
    /// An `unquote(expr)` hole.
    Unquote(Expr),
}

fn is_unquote(e: &Expr) -> Option<&Expr> {
    match e {
        Expr::Call(c, args, _) if args.len() == 1 && matches!(&**c, Expr::Ident(n) if n == "unquote") => Some(&args[0]),
        _ => None,
    }
}

/// Rebuild an expression bottom-up, letting `f` replace any node.
fn modify_expr(e: Expr, f: &mut dyn FnMut(Expr) -> Result<Expr, String>) -> Result<Expr, String> {
    let e = match e {
        Expr::Prefix(op, r, l) => Expr::Prefix(op, Box::new(modify_expr(*r, f)?), l),
        Expr::Infix(a, op, b, l) => Expr::Infix(Box::new(modify_expr(*a, f)?), op, Box::new(modify_expr(*b, f)?), l),
        Expr::Index(a, b, l) => Expr::Index(Box::new(modify_expr(*a, f)?), Box::new(modify_expr(*b, f)?), l),
        Expr::If(c, t, el) => Expr::If(
            Box::new(modify_expr(*c, f)?),
            modify_stmts(t, f)?,
            match el {
                Some(el) => Some(modify_stmts(el, f)?),
                None => None,
            },
        ),
        Expr::Fn(ps, body) => Expr::Fn(ps, modify_stmts(body, f)?),
        Expr::Array(xs) => Expr::Array(xs.into_iter().map(|x| modify_expr(x, f)).collect::<Result<_, _>>()?),
        Expr::Hash(ps) => Expr::Hash(
            ps.into_iter().map(|(k, v)| Ok((modify_expr(k, f)?, modify_expr(v, f)?))).collect::<Result<_, String>>()?,
        ),
        Expr::Call(c, args, l) if is_unquote(&Expr::Call(c.clone(), args.clone(), l)).is_none() => Expr::Call(
            Box::new(modify_expr(*c, f)?),
            args.into_iter().map(|x| modify_expr(x, f)).collect::<Result<_, _>>()?,
            l,
        ),
        other => other,
    };
    f(e)
}

fn modify_stmts(ss: Vec<Stmt>, f: &mut dyn FnMut(Expr) -> Result<Expr, String>) -> Result<Vec<Stmt>, String> {
    ss.into_iter()
        .map(|s| {
            Ok(match s {
                Stmt::Let(n, v, l) => Stmt::Let(n, modify_expr(v, f)?, l),
                Stmt::Return(v, l) => Stmt::Return(modify_expr(v, f)?, l),
                Stmt::Expr(e) => Stmt::Expr(modify_expr(e, f)?),
            })
        })
        .collect()
}

/// Split the text of `quote(e)`'s argument around its `unquote` holes.
pub fn quote_pieces(e: &Expr) -> Vec<Piece> {
    const MARK: char = '\u{F8FF}';
    let mut holes = Vec::new();
    let marked = modify_expr(e.clone(), &mut |n| {
        Ok(match is_unquote(&n) {
            Some(inner) => {
                holes.push(inner.clone());
                Expr::Ident(format!("{MARK}{}{MARK}", holes.len() - 1))
            }
            None => n,
        })
    })
    .expect("marking cannot fail");
    let text = marked.to_string();
    let mut pieces = Vec::new();
    for (i, part) in text.split(MARK).enumerate() {
        if i % 2 == 0 {
            if !part.is_empty() {
                pieces.push(Piece::Text(part.to_string()));
            }
        } else {
            let idx: usize = part.parse().expect("hole index");
            pieces.push(Piece::Unquote(holes[idx].clone()));
        }
    }
    pieces
}

// ---------------------------------------------------------------------------
// Compile-time evaluator

#[derive(Clone, Debug)]
enum V {
    Null,
    Int(i64),
    Bool(bool),
    Str(String),
    Array(Vec<V>),
    Hash(Vec<(V, V)>),
    Fn(Vec<String>, Vec<Stmt>, Env),
    Macro(Vec<String>, Vec<Stmt>, Env),
    Builtin(&'static str),
    Quote(Expr),
}

type Env = Rc<RefCell<EnvData>>;

#[derive(Debug, Default)]
struct EnvData {
    store: HashMap<String, V>,
    outer: Option<Env>,
}

fn env_get(env: &Env, name: &str) -> Option<V> {
    let e = env.borrow();
    match e.store.get(name) {
        Some(v) => Some(v.clone()),
        None => e.outer.as_ref().and_then(|o| env_get(o, name)),
    }
}

fn enclosed(outer: &Env) -> Env {
    Rc::new(RefCell::new(EnvData { store: HashMap::new(), outer: Some(outer.clone()) }))
}

enum Flow {
    Value(V),
    Return(V),
}

fn type_name(v: &V) -> &'static str {
    match v {
        V::Null => "NULL",
        V::Int(_) => "INTEGER",
        V::Bool(_) => "BOOLEAN",
        V::Str(_) => "STRING",
        V::Array(_) => "ARRAY",
        V::Hash(_) => "HASH",
        V::Fn(..) => "FUNCTION",
        V::Macro(..) => "MACRO",
        V::Builtin(_) => "BUILTIN",
        V::Quote(_) => "QUOTE",
    }
}

fn inspect(v: &V) -> String {
    match v {
        V::Null => "null".into(),
        V::Int(i) => i.to_string(),
        V::Bool(b) => b.to_string(),
        V::Str(s) => s.clone(),
        V::Array(xs) => format!("[{}]", xs.iter().map(inspect).collect::<Vec<_>>().join(", ")),
        V::Hash(ps) => format!(
            "{{{}}}",
            ps.iter().map(|(k, v)| format!("{}: {}", inspect(k), inspect(v))).collect::<Vec<_>>().join(", ")
        ),
        V::Fn(ps, b, _) => fn_display(ps, b),
        V::Macro(ps, b, _) => format!("macro({}) {{\n{}\n}}", ps.join(", "), super::monkey::Block(b)),
        V::Builtin(_) => "builtin function".into(),
        V::Quote(e) => format!("QUOTE({e})"),
    }
}

fn equal(a: &V, b: &V) -> bool {
    match (a, b) {
        (V::Null, V::Null) => true,
        (V::Int(x), V::Int(y)) => x == y,
        (V::Bool(x), V::Bool(y)) => x == y,
        (V::Str(x), V::Str(y)) => x == y,
        _ => false,
    }
}

fn truthy(v: &V) -> bool {
    !matches!(v, V::Null | V::Bool(false))
}

/// Limit on evaluation depth, so a runaway macro cannot crash the compiler.
const MAX_DEPTH: usize = 2000;

struct Eval {
    depth: usize,
}

impl Eval {
    fn block(&mut self, ss: &[Stmt], env: &Env) -> Result<Flow, String> {
        let mut last = V::Null;
        for s in ss {
            match s {
                Stmt::Let(n, v, _) => {
                    let v = self.expr(v, env)?;
                    env.borrow_mut().store.insert(n.clone(), v);
                    last = V::Null;
                }
                Stmt::Return(v, _) => return Ok(Flow::Return(self.expr(v, env)?)),
                Stmt::Expr(e) => match self.expr_flow(e, env)? {
                    Flow::Value(v) => last = v,
                    r => return Ok(r),
                },
            }
        }
        Ok(Flow::Value(last))
    }

    fn expr(&mut self, e: &Expr, env: &Env) -> Result<V, String> {
        Ok(match self.expr_flow(e, env)? {
            Flow::Value(v) | Flow::Return(v) => v,
        })
    }

    /// Evaluate, propagating a `return` out of an `if` block.
    fn expr_flow(&mut self, e: &Expr, env: &Env) -> Result<Flow, String> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err("macro evaluation nested too deeply".into());
        }
        let r = self.expr_inner(e, env);
        self.depth -= 1;
        r
    }

    fn expr_inner(&mut self, e: &Expr, env: &Env) -> Result<Flow, String> {
        Ok(Flow::Value(match e {
            Expr::Missing => V::Null,
            Expr::Int(i) => V::Int(*i),
            Expr::Bool(b) => V::Bool(*b),
            Expr::Str(s) => V::Str(s.clone()),
            Expr::Ident(n) => match env_get(env, n) {
                Some(v) => v,
                None => match n.as_str() {
                    "len" | "first" | "last" | "rest" | "push" | "puts" => V::Builtin(
                        ["len", "first", "last", "rest", "push", "puts"].into_iter().find(|b| b == n).unwrap(),
                    ),
                    _ => return Err(format!("identifier not found: {n}")),
                },
            },
            Expr::Prefix(op, r, _) => {
                let r = self.expr(r, env)?;
                match (op.as_str(), r) {
                    ("!", r) => V::Bool(!truthy(&r)),
                    ("-", V::Int(i)) => V::Int(i.wrapping_neg()),
                    (op, r) => return Err(format!("unknown operator: {op}{}", type_name(&r))),
                }
            }
            Expr::Infix(a, op, b, _) => {
                let (a, b) = (self.expr(a, env)?, self.expr(b, env)?);
                match (&a, &b, op.as_str()) {
                    (V::Int(x), V::Int(y), _) => match op.as_str() {
                        "+" => V::Int(x.wrapping_add(*y)),
                        "-" => V::Int(x.wrapping_sub(*y)),
                        "*" => V::Int(x.wrapping_mul(*y)),
                        "/" if *y == 0 => return Err("division by zero".into()),
                        "/" => V::Int(x.wrapping_div(*y)),
                        "<" => V::Bool(x < y),
                        ">" => V::Bool(x > y),
                        "==" => V::Bool(x == y),
                        _ => V::Bool(x != y),
                    },
                    (V::Str(x), V::Str(y), "+") => V::Str(format!("{x}{y}")),
                    (_, _, "==") => V::Bool(equal(&a, &b)),
                    (_, _, "!=") => V::Bool(!equal(&a, &b)),
                    _ if type_name(&a) != type_name(&b) => {
                        return Err(format!("type mismatch: {} {op} {}", type_name(&a), type_name(&b)))
                    }
                    _ => return Err(format!("unknown operator: {} {op} {}", type_name(&a), type_name(&b))),
                }
            }
            Expr::If(c, t, el) => {
                let c = self.expr(c, env)?;
                return if truthy(&c) {
                    self.block(t, env)
                } else if let Some(el) = el {
                    self.block(el, env)
                } else {
                    Ok(Flow::Value(V::Null))
                };
            }
            Expr::Fn(ps, b) => V::Fn(ps.clone(), b.clone(), env.clone()),
            Expr::Macro(ps, b) => V::Macro(ps.clone(), b.clone(), env.clone()),
            Expr::Array(xs) => V::Array(xs.iter().map(|x| self.expr(x, env)).collect::<Result<_, _>>()?),
            Expr::Hash(ps) => {
                let mut out: Vec<(V, V)> = Vec::new();
                for (k, v) in ps {
                    let k = self.expr(k, env)?;
                    if !matches!(k, V::Int(_) | V::Bool(_) | V::Str(_)) {
                        return Err(format!("unusable as hash key: {}", type_name(&k)));
                    }
                    let v = self.expr(v, env)?;
                    match out.iter_mut().find(|(k2, _)| equal(k2, &k)) {
                        Some(p) => p.1 = v,
                        None => out.push((k, v)),
                    }
                }
                V::Hash(out)
            }
            Expr::Index(a, i, _) => {
                let (a, i) = (self.expr(a, env)?, self.expr(i, env)?);
                match (&a, &i) {
                    (V::Array(xs), V::Int(i)) => {
                        usize::try_from(*i).ok().and_then(|i| xs.get(i).cloned()).unwrap_or(V::Null)
                    }
                    (V::Hash(ps), k) if matches!(k, V::Int(_) | V::Bool(_) | V::Str(_)) => {
                        ps.iter().find(|(k2, _)| equal(k2, k)).map(|p| p.1.clone()).unwrap_or(V::Null)
                    }
                    (V::Hash(_), k) => return Err(format!("unusable as hash key: {}", type_name(k))),
                    _ => return Err(format!("index operator not supported: {}", type_name(&a))),
                }
            }
            Expr::Call(c, args, _) => {
                if let Expr::Ident(n) = &**c {
                    if n == "quote" && args.len() == 1 && env_get(env, "quote").is_none() {
                        return Ok(Flow::Value(V::Quote(self.unquote_calls(&args[0], env)?)));
                    }
                }
                let f = self.expr(c, env)?;
                let args: Vec<V> = args.iter().map(|a| self.expr(a, env)).collect::<Result<_, _>>()?;
                self.apply(f, args)?
            }
        }))
    }

    fn apply(&mut self, f: V, args: Vec<V>) -> Result<V, String> {
        match f {
            V::Fn(ps, body, fenv) => {
                if ps.len() != args.len() {
                    return Err(format!("wrong number of arguments: want={}, got={}", ps.len(), args.len()));
                }
                let env = enclosed(&fenv);
                for (p, a) in ps.iter().zip(args) {
                    env.borrow_mut().store.insert(p.clone(), a);
                }
                Ok(match self.block(&body, &env)? {
                    Flow::Value(v) | Flow::Return(v) => v,
                })
            }
            V::Builtin(name) => builtin(name, args),
            other => Err(format!("not a function: {}", type_name(&other))),
        }
    }

    /// Evaluate the `unquote` calls inside a quoted expression.
    fn unquote_calls(&mut self, e: &Expr, env: &Env) -> Result<Expr, String> {
        modify_expr(e.clone(), &mut |n| match is_unquote(&n) {
            Some(inner) => Ok(to_ast(self.expr(inner, env)?)),
            None => Ok(n),
        })
    }
}

/// The book's `convertObjectToASTNode`, extended to every value that has a
/// literal syntax.
fn to_ast(v: V) -> Expr {
    match v {
        V::Int(i) => {
            if i < 0 {
                Expr::Prefix("-".into(), Box::new(Expr::Int(i.wrapping_neg())), 0)
            } else {
                Expr::Int(i)
            }
        }
        V::Bool(b) => Expr::Bool(b),
        V::Str(s) => Expr::Str(s),
        V::Quote(e) => e,
        V::Array(xs) => Expr::Array(xs.into_iter().map(to_ast).collect()),
        V::Hash(ps) => Expr::Hash(ps.into_iter().map(|(k, v)| (to_ast(k), to_ast(v))).collect()),
        V::Fn(ps, b, _) => Expr::Fn(ps, b),
        V::Builtin(n) => Expr::Ident(n.to_string()),
        V::Null | V::Macro(..) => Expr::Missing,
    }
}

fn builtin(name: &str, args: Vec<V>) -> Result<V, String> {
    let want = if name == "push" { 2 } else { 1 };
    if name != "puts" && args.len() != want {
        return Err(format!("wrong number of arguments. got={}, want={want}", args.len()));
    }
    let arr = |v: &V| match v {
        V::Array(xs) => Ok(xs.clone()),
        other => Err(format!("argument to `{name}` must be ARRAY, got {}", type_name(other))),
    };
    Ok(match name {
        "len" => match &args[0] {
            V::Str(s) => V::Int(s.chars().count() as i64),
            V::Array(xs) => V::Int(xs.len() as i64),
            other => return Err(format!("argument to `len` not supported, got {}", type_name(other))),
        },
        "first" => arr(&args[0])?.first().cloned().unwrap_or(V::Null),
        "last" => arr(&args[0])?.last().cloned().unwrap_or(V::Null),
        "rest" => {
            let xs = arr(&args[0])?;
            if xs.is_empty() {
                V::Null
            } else {
                V::Array(xs[1..].to_vec())
            }
        }
        "push" => {
            let mut xs = arr(&args[0])?;
            xs.push(args[1].clone());
            V::Array(xs)
        }
        _ => {
            // `puts` during macro expansion runs at compile time; send it to
            // stderr so it cannot be mistaken for program output.
            for a in &args {
                eprintln!("{}", inspect(a));
            }
            V::Null
        }
    })
}

/// Define and expand macros (the book's `DefineMacros` and `ExpandMacros`).
pub fn expand(prog: Vec<Stmt>) -> Result<Vec<Stmt>, String> {
    let macro_env: Env = Rc::new(RefCell::new(EnvData::default()));
    let mut rest = Vec::new();
    for s in prog {
        match s {
            Stmt::Let(name, Expr::Macro(ps, body), _) => {
                let m = V::Macro(ps, body, macro_env.clone());
                macro_env.borrow_mut().store.insert(name, m);
            }
            other => rest.push(other),
        }
    }
    if macro_env.borrow().store.is_empty() {
        return Ok(rest);
    }
    let mut ev = Eval { depth: 0 };
    modify_stmts(rest, &mut |n| {
        let Expr::Call(c, args, _) = &n else { return Ok(n) };
        let Expr::Ident(name) = &**c else { return Ok(n) };
        let Some(V::Macro(ps, body, menv)) = env_get(&macro_env, name) else { return Ok(n) };
        if ps.len() != args.len() {
            return Err(format!("macro {name}: wrong number of arguments: want={}, got={}", ps.len(), args.len()));
        }
        let env = enclosed(&menv);
        for (p, a) in ps.iter().zip(args) {
            env.borrow_mut().store.insert(p.clone(), V::Quote(a.clone()));
        }
        match ev.block(&body, &env)? {
            Flow::Value(V::Quote(e)) | Flow::Return(V::Quote(e)) => Ok(e),
            _ => Err(format!("macro {name}: we only support returning AST-nodes from macros")),
        }
    })
}
