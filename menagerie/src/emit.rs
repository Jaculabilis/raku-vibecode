//! Code-generation helpers shared by the three frontends: a function
//! builder with labels and stack-height tracking, and a resolver for
//! variables captured across nested functions.

use crate::bytecode::{Capture, Const, Lang, Op, Program, Proto, RecordDesc};
use std::collections::HashMap;

pub type Label = usize;

struct LabelInfo {
    pos: Option<u32>,
    depth: Option<i64>,
}

/// Builds the code of one function.
pub struct FnBuilder {
    pub proto: Proto,
    /// Line attached to subsequently emitted instructions.
    pub line: u32,
    /// Current operand-stack height (statically known).
    pub depth: i64,
    reachable: bool,
    labels: Vec<LabelInfo>,
    fixups: Vec<(usize, Label)>,
}

impl FnBuilder {
    pub fn new(name: &str, lang: Lang, arity: u32, flags: u8) -> FnBuilder {
        FnBuilder {
            proto: Proto {
                name: name.to_string(),
                lang,
                arity,
                flags,
                nlocals: arity + (flags & crate::bytecode::FLAG_RECEIVER != 0) as u32,
                captures: Vec::new(),
                code: Vec::new(),
                lines: Vec::new(),
                display: String::new(),
            },
            line: 1,
            depth: 0,
            reachable: true,
            labels: Vec::new(),
            fixups: Vec::new(),
        }
    }

    /// Emit an instruction whose stack effect does not depend on constants
    /// (everything except `NewRecord`, see [`FnBuilder::emit_record`]).
    pub fn emit(&mut self, op: Op) {
        let eff = op.stack_effect(&[]);
        self.emit_effect(op, eff)
    }

    pub fn emit_record(&mut self, k: u32, nfields: usize) {
        self.emit_effect(Op::NewRecord(k), 1 - nfields as i64)
    }

    fn emit_effect(&mut self, op: Op, eff: i64) {
        self.proto.code.push(op);
        self.proto.lines.push(self.line);
        self.depth += eff;
        if op.is_terminal() {
            self.reachable = false;
        }
    }

    pub fn new_label(&mut self) -> Label {
        self.labels.push(LabelInfo { pos: None, depth: None });
        self.labels.len() - 1
    }

    /// Emit a jump (built by `mk`) to `l`.
    pub fn jump(&mut self, mk: fn(u32) -> Op, l: Label) {
        let pc = self.proto.code.len();
        self.emit(mk(0));
        self.fixups.push((pc, l));
        if self.labels[l].depth.is_none() {
            self.labels[l].depth = Some(self.depth);
        }
    }

    /// Bind `l` to the next instruction.
    pub fn place(&mut self, l: Label) {
        self.labels[l].pos = Some(self.proto.code.len() as u32);
        match self.labels[l].depth {
            Some(d) if !self.reachable => self.depth = d,
            Some(d) => debug_assert_eq!(d, self.depth, "stack height mismatch at label"),
            None => self.labels[l].depth = Some(self.depth),
        }
        self.reachable = true;
    }

    /// Mark the current position reachable again without a label (used
    /// after `break`, whose following code is dead but must still be
    /// generated consistently).
    pub fn set_reachable(&mut self, depth: i64) {
        self.depth = depth;
        self.reachable = true;
    }

    pub fn is_reachable(&self) -> bool {
        self.reachable
    }

    pub fn alloc_local(&mut self) -> u32 {
        self.proto.nlocals += 1;
        self.proto.nlocals - 1
    }

    pub fn finish(mut self) -> Proto {
        for (pc, l) in self.fixups {
            let target = self.labels[l].pos.expect("jump to unplaced label");
            self.proto.code[pc] = self.proto.code[pc].with_operand(target);
        }
        self.proto
    }
}

/// Constant pool with de-duplication.
#[derive(Default)]
pub struct ConstPool {
    pub consts: Vec<Const>,
    strs: HashMap<String, u32>,
    ints: HashMap<i64, u32>,
}

impl ConstPool {
    pub fn str(&mut self, s: &str) -> u32 {
        if let Some(&k) = self.strs.get(s) {
            return k;
        }
        let k = self.consts.len() as u32;
        self.consts.push(Const::Str(s.to_string()));
        self.strs.insert(s.to_string(), k);
        k
    }
    pub fn int(&mut self, i: i64) -> u32 {
        if let Some(&k) = self.ints.get(&i) {
            return k;
        }
        let k = self.consts.len() as u32;
        self.consts.push(Const::Int(i));
        self.ints.insert(i, k);
        k
    }
    pub fn float(&mut self, f: f64) -> u32 {
        if let Some(k) = self.consts.iter().position(|c| matches!(c, Const::Float(g) if g.to_bits() == f.to_bits())) {
            return k as u32;
        }
        self.consts.push(Const::Float(f));
        self.consts.len() as u32 - 1
    }
    pub fn record(&mut self, name: &str, fields: Vec<String>) -> u32 {
        self.consts.push(Const::Record(RecordDesc { name: name.to_string(), fields }));
        self.consts.len() as u32 - 1
    }
}

/// Identifies one variable declaration within a compilation unit.
pub type DeclId = usize;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VarLoc {
    /// Plain value in a local slot.
    Local(u32),
    /// Captured variable: the local slot holds a cell.
    Cell(u32),
    /// Captured cell of the current closure.
    Upval(u32),
}

pub struct FnCtx {
    pub b: FnBuilder,
    locals: HashMap<DeclId, VarLoc>,
    upvals: HashMap<DeclId, u32>,
}

/// The stack of functions being compiled (innermost last), plus the
/// program being assembled.
pub struct Gen {
    pub fns: Vec<FnCtx>,
    pub pool: ConstPool,
    pub protos: Vec<Proto>,
}

impl Gen {
    pub fn new() -> Gen {
        Gen { fns: Vec::new(), pool: ConstPool::default(), protos: Vec::new() }
    }

    pub fn b(&mut self) -> &mut FnBuilder {
        &mut self.fns.last_mut().expect("function context").b
    }

    pub fn emit(&mut self, op: Op) {
        self.b().emit(op)
    }

    pub fn push_fn(&mut self, b: FnBuilder) {
        self.fns.push(FnCtx { b, locals: HashMap::new(), upvals: HashMap::new() });
    }

    /// Finish the innermost function, returning its prototype index.
    pub fn pop_fn(&mut self) -> u32 {
        let ctx = self.fns.pop().expect("function context");
        self.protos.push(ctx.b.finish());
        self.protos.len() as u32 - 1
    }

    /// Declare a variable in the innermost function, allocating a slot.
    pub fn declare(&mut self, decl: DeclId, captured: bool) -> VarLoc {
        let slot = self.b().alloc_local();
        self.bind(decl, slot, captured)
    }

    /// Bind a declaration to an existing slot (parameters, receiver).
    pub fn bind(&mut self, decl: DeclId, slot: u32, captured: bool) -> VarLoc {
        let loc = if captured { VarLoc::Cell(slot) } else { VarLoc::Local(slot) };
        self.fns.last_mut().unwrap().locals.insert(decl, loc);
        loc
    }

    pub fn is_bound_here(&self, decl: DeclId) -> bool {
        self.fns.last().is_some_and(|f| f.locals.contains_key(&decl))
    }

    /// Where is `decl` from the innermost function's point of view?
    /// `None` if it is not a local of any enclosing function.
    pub fn resolve(&mut self, decl: DeclId) -> Option<VarLoc> {
        let top = self.fns.len().checked_sub(1)?;
        self.resolve_in(top, decl)
    }

    fn resolve_in(&mut self, f: usize, decl: DeclId) -> Option<VarLoc> {
        if let Some(&loc) = self.fns[f].locals.get(&decl) {
            return Some(loc);
        }
        if let Some(&u) = self.fns[f].upvals.get(&decl) {
            return Some(VarLoc::Upval(u));
        }
        if f == 0 {
            return None;
        }
        let capture = match self.resolve_in(f - 1, decl)? {
            VarLoc::Cell(s) => Capture::Local(s),
            VarLoc::Upval(u) => Capture::Upval(u),
            VarLoc::Local(_) => panic!("compiler bug: variable captured but not allocated in a cell"),
        };
        let caps = &mut self.fns[f].b.proto.captures;
        caps.push(capture);
        let u = caps.len() as u32 - 1;
        self.fns[f].upvals.insert(decl, u);
        Some(VarLoc::Upval(u))
    }

    pub fn emit_get(&mut self, loc: VarLoc) {
        self.emit(match loc {
            VarLoc::Local(s) => Op::GetLocal(s),
            VarLoc::Cell(s) => Op::GetCell(s),
            VarLoc::Upval(u) => Op::GetUpval(u),
        })
    }

    /// Pop the top of stack into an existing variable.
    pub fn emit_set(&mut self, loc: VarLoc) {
        self.emit(match loc {
            VarLoc::Local(s) => Op::SetLocal(s),
            VarLoc::Cell(s) => Op::SetCell(s),
            VarLoc::Upval(u) => Op::SetUpval(u),
        })
    }

    /// Pop the top of stack into a freshly declared variable (a captured
    /// variable gets a new cell, so each declaration is a new variable).
    pub fn emit_init(&mut self, loc: VarLoc) {
        self.emit(match loc {
            VarLoc::Local(s) => Op::SetLocal(s),
            VarLoc::Cell(s) => Op::InitCell(s),
            VarLoc::Upval(u) => Op::SetUpval(u),
        })
    }

    /// Box a parameter (already in its slot) if it is captured.
    pub fn box_param(&mut self, loc: VarLoc) {
        if let VarLoc::Cell(s) = loc {
            self.emit(Op::GetLocal(s));
            self.emit(Op::InitCell(s));
        }
    }

    pub fn into_program(self, modules: Vec<crate::bytecode::Module>) -> Program {
        Program { consts: self.pool.consts, protos: self.protos, modules }
    }
}

impl Default for Gen {
    fn default() -> Self {
        Self::new()
    }
}
