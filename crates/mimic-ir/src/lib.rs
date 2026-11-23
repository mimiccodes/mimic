//! Mimic IR: the intermediate representation between the checked syntax tree
//! and LLVM (docs/design/ir.md).
//!
//! A function is a tree of regions: each region is a list of instructions in
//! SSA form (every value is defined once) ending in the value it yields.
//! Control flow is structured: an `if` is one instruction holding a region
//! for each branch, not jumps between blocks, which keeps loops easy to find
//! and lets nets and GPU kernels be made from the same IR later.
//!
//! The compiler moves onto it one function at a time: [`lower::function`]
//! lowers what it covers and returns `None` for anything else, which then
//! takes today's code generator. This first step covers functions of plain
//! numbers and truth values: literals, parameters, `let`, arithmetic and
//! comparisons, `and`/`or`, `if` and calls.

pub mod lower;

/// `MIMIC_IR=why`: say why each function the IR does not cover stays on the
/// old path.
pub fn why() -> bool {
    static WHY: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *WHY.get_or_init(|| std::env::var("MIMIC_IR").as_deref() == Ok("why"))
}

/// What lowering asks the code generator.
pub trait Hints {
    /// Whether `let name = value` (followed by `rest` and `tail`) is a list
    /// used only inside array expressions, which the old path fuses into
    /// those uses rather than build. `scope`: the variables in scope with
    /// their types; `lazies`: the lazy lets in scope.
    fn lazy(
        &mut self,
        name: &str,
        value: &mimic_syntax::ast::Expr,
        rest: &[mimic_syntax::ast::Stmt],
        tail: Option<&mimic_syntax::ast::Expr>,
        scope: &[(String, Ty)],
        lazies: &[(String, *const mimic_syntax::ast::Expr)],
    ) -> bool;

    /// Whether the old path computes `e` (a list) element by element, as
    /// part of a fused loop (`xs ¨ f` with a pure `f` on numbers).
    fn fusible(&mut self, e: &mimic_syntax::ast::Expr, scope: &[(String, Ty)], lazies: &[(String, *const mimic_syntax::ast::Expr)]) -> bool;

    /// The types of a user function's specialisation (for inlining it).
    fn spec(&mut self, key: &mimic_check::types::Key) -> Option<std::rc::Rc<mimic_check::types::Spec>>;

    /// Which parameters of a user function's specialisation it takes over
    /// (its callers hand over a reference).
    fn takes(&mut self, key: &mimic_check::types::Key) -> Vec<bool>;

    /// Whether the old path would run some statements of a block with
    /// these statements apart (independent calls in parallel, a call with
    /// effects as a task): the IR runs everything in order, so it leaves
    /// such a function to the old path.
    fn runs_apart(&mut self, stmts: &[mimic_syntax::ast::Stmt]) -> bool;
}
/// Whether compiling `f` from its IR gains anything over the old path. A
/// function the IR covers whole always does (it may sink work into
/// branches); one that leaves expressions to the old path (embedded) only
/// when it also does what the old path cannot: change a record or list in
/// place, build or read records, fuse array work, index, loop, take over a
/// parameter, hand a value over to a call, or pass a short list or record in
/// registers. Otherwise its embeds only lend
/// their variables (the old path would move them at their last use: `head +
/// "\r\n"` appends in place there) and copy what they read in registers.
pub fn worth(f: &Function) -> bool {
    fn scan(r: &Region, embeds: &mut bool, work: &mut bool) {
        for i in &r.insts {
            match &i.op {
                Op::Embed { .. } => *embeds = true,
                Op::Take(_) | Op::With { .. } | Op::Put { .. } | Op::Push { .. } | Op::Record { .. } | Op::Field { .. } | Op::Map(_) | Op::Reduce(..) | Op::Index(..) | Op::Loop { .. } => *work = true,
                Op::Call { moves, .. } if moves.iter().any(|m| *m) => *work = true,
                _ => {}
            }
            for g in i.op.regions() {
                scan(g, embeds, work);
            }
        }
    }
    // a short list or record passed or given back in registers: callers in
    // the IR skip building it on the heap
    let regs = f.params.iter().map(|p| p.1).chain([f.ret]).any(|t| matches!(t, Ty::Agg(_)));
    let (mut embeds, mut work) = (false, regs);
    scan(&f.body, &mut embeds, &mut work);
    !embeds || work
}

pub mod fuse;
pub mod own;
pub mod sink;

use mimic_syntax::ast::{BinOp, UnOp};
use std::fmt::{self, Write as _};

/// The type of an IR value.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Ty {
    Unit,
    I64,
    F64,
    Bool,
    /// A value kept in registers: a list of 1 … 16 numbers or truth values,
    /// or a tuple or record of numbers, truth values and such lists (at
    /// most 64 numbers). The checker's type says which.
    Agg(mimic_check::types::Ty),
    /// A list on the heap (of i64, f64 or bytes): one passed in is
    /// borrowed; one a call gives (or an `if` choosing lists) is owned, and
    /// released at the end of the region that made it unless that region
    /// yields it ([`owns`]).
    List(mimic_check::types::Ty),
    /// A record on the heap (a plain record type, not generic, not kept in
    /// registers): owned or borrowed as a list is.
    Rec(mimic_check::types::Ty),
}

impl Ty {
    pub fn name(self) -> String {
        match self {
            Ty::Unit => "unit".into(),
            Ty::I64 => "i64".into(),
            Ty::F64 => "f64".into(),
            Ty::Bool => "bool".into(),
            Ty::Agg(t) => format!("regs {t}"),
            Ty::List(t) => format!("list {t}"),
            Ty::Rec(t) => format!("rec {t}"),
        }
    }

    /// The IR type of a checker type, when the IR covers it (the program,
    /// for a record's fields).
    pub fn of(p: &mimic_check::Program, t: mimic_check::types::Ty) -> Option<Ty> {
        use mimic_check::types::Ty as C;
        Some(match t {
            C::Unit => Ty::Unit,
            C::I64 => Ty::I64,
            C::F64 => Ty::F64,
            C::Bool => Ty::Bool,
            _ if parts(p, t).is_some() => Ty::Agg(t),
            // (text and lists of text too: the IR moves and releases them as
            // any list; reading their elements is left to the old path)
            C::Arr(mimic_check::types::El::I64 | mimic_check::types::El::F64 | mimic_check::types::El::U8 | mimic_check::types::El::Char | mimic_check::types::El::Str, 0) => Ty::List(t),
            // (a plain record: not an instance of a generic or shaped one)
            C::Obj(id) if mimic_check::types::obj_tid(id) == id as usize && matches!(p.types[id as usize].decl.kind, mimic_syntax::ast::TypeKind::Record(_)) => Ty::Rec(t),
            _ => return None,
        })
    }
}

/// A short list's element type and length (1 … 16 numbers or truth values).
pub fn short_list(t: mimic_check::types::Ty) -> Option<(mimic_check::types::El, u32)> {
    use mimic_check::types::{El, Ty as C};
    match t {
        C::Arr(el @ (El::I64 | El::F64 | El::Bool), n) if n > 0 => Some((el, n as u32)),
        _ => None,
    }
}

/// The parts of a value kept in registers, in order: a short list's
/// elements, a tuple's, a record's fields as declared. The same rule as the
/// code generator's: numbers, truth values and short lists, 64 numbers at
/// most. None for anything else.
pub fn parts(p: &mimic_check::Program, t: mimic_check::types::Ty) -> Option<Vec<mimic_check::types::Ty>> {
    use mimic_check::types::{self as ct, Ty as C};
    if let Some((el, n)) = short_list(t) {
        return Some(vec![C::of_el(el); n as usize]);
    }
    let ps: Vec<C> = match t {
        C::Tup(id) => ct::tuple_elems(id),
        C::Obj(id) => {
            let tid = ct::obj_tid(id);
            let mimic_syntax::ast::TypeKind::Record(fs) = &p.types[tid].decl.kind else { return None };
            fs.iter().map(|(_, ft, _)| ct::field_type(p, tid, t, ft)).collect()
        }
        _ => return None,
    };
    let mut n = 0usize;
    for x in &ps {
        match x {
            C::I64 | C::F64 | C::Bool => n += 1,
            _ => n += short_list(*x)?.1 as usize,
        }
    }
    (!ps.is_empty() && n <= 64).then_some(ps)
}

/// A value: the result of one instruction (or a function parameter).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Val(pub u32);

impl fmt::Display for Val {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "%{}", self.0)
    }
}

#[derive(Clone, Debug)]
pub enum Op {
    Int(i64),
    Float(f64),
    Bool(bool),
    Unit,
    /// The function's parameter number `n`.
    Param(u32),
    /// The function's parameter number `n`, which it takes over: the caller
    /// handed over a reference, owned here (released after its last use, or
    /// moved on).
    Take(u32),
    /// An operator on two values, both always evaluated (`and`/`or` become
    /// an [`Op::If`]).
    Bin(BinOp, Val, Val),
    Un(UnOp, Val),
    /// A call of a user function. `site` is the call's syntax node (what
    /// the code generator keys the callee's specialisation and placement by).
    /// `takes`: which parameters the callee takes over; `moves`: which
    /// arguments are handed over as they are (an owned value at its last
    /// use, set by [`own`]), the others lent (a callee that takes one gets a
    /// reference of its own).
    Call { site: usize, callee: mimic_check::FnId, args: Vec<Val>, takes: Vec<bool>, moves: Vec<bool> },
    /// `if cond { then } else { els }`: the value the branch taken yields.
    If { cond: Val, then: Region, els: Region },
    /// A value kept in registers made of its parts (a tuple's elements, a
    /// short list's, a record's fields in declaration order).
    Make(Vec<Val>),
    /// Part `k` of a value kept in registers.
    Get(Val, u32),
    /// The value with part `k` replaced (`{ r with f: x }`).
    Set(Val, u32, Val),
    /// The length of a list.
    Len(Val),
    /// Element `i` of a list whose elements are of kind `el` (an index out
    /// of range stops the program).
    Index(Val, Val, mimic_check::types::El),
    /// `+/ ×/ ⌈/ ⌊/` of the elements `tree` computes, of kind `el`, in one
    /// loop (no list in between). Of a list alone (a [`Tree::Leaf`]), `⌈/`
    /// and `⌊/` of an empty one stop the program, as the runtime's
    /// reductions do; of a computation they give the identity, as the old
    /// path's fused reductions do.
    Reduce(mimic_syntax::glyphs::Prim, Tree, mimic_check::types::El),
    /// The list of the elements `tree` computes, in one loop (owned).
    Map(Tree),
    /// Releases an owned list right after its last use in its region
    /// (placed by [`own`]; gives nothing).
    Drop(Val),
    /// A record of type `tid` on the heap, its fields in declaration order
    /// (owned). `moves`: the fields handed over as they are (owned values at
    /// their last use, set by [`own`]); the record takes a reference of its
    /// own to the others.
    Record { tid: usize, fields: Vec<Val>, moves: Vec<bool> },
    /// Field `k` of a record on the heap: borrowed from a borrowed record,
    /// its own reference from an owned one, unless `borrow` (set by
    /// [`own`]: the record is kept until the field's last use).
    Field { base: Val, tid: usize, k: u32, borrow: bool },
    /// `{ base with … }` of a record on the heap: fields `k` replaced
    /// (owned). It changes the record in place when it is held once:
    /// `moved` (set by [`own`]) hands it the base, an owned value at its
    /// last use; else the base is lent, and copied unless held once.
    /// `moves`: the new values handed over as they are, as [`Op::Record`]'s.
    /// `changes`: list fields changed from their own old value (`{ s with
    /// v: put(s.v, i, x) }`): taken out of the record once it is held once,
    /// changed in place when held once themselves, stored back.
    With { base: Val, tid: usize, fields: Vec<(u32, Val)>, moved: bool, moves: Vec<bool>, changes: Vec<(u32, Vec<Change>)> },
    /// `put(list, index, value)` on a list of i64 (owned): in place when
    /// `moved` (set by [`own`]: the list at its last use) and held once,
    /// else on a copy; an index out of range stops the program.
    Put { list: Val, index: Val, value: Val, moved: bool },
    /// `push(list, value)` on a list of i64 (owned), as [`Op::Put`].
    Push { list: Val, value: Val, moved: bool },
    /// An expression the IR does not cover yet (a map, a reduction, a call
    /// of the runtime, …), compiled by the code generator's old path where
    /// it stands: `uses` are the variables it reads, given as these IR
    /// values. A bridge while the IR grows; each kind becomes an operation
    /// of its own. (The syntax node lives as long as the compile.)
    /// `lazies`: `let`s of lists used only inside array expressions, never
    /// built: the old path fuses each into where it is read (in order:
    /// one may read an earlier one). `moves[k]`: the expression is the last
    /// use of record `uses[k]`, which the old path may move on there (into a
    /// loop that changes it in place) or release after it.
    Embed { expr: *const mimic_syntax::ast::Expr, uses: Vec<(String, Val, Ty)>, lazies: Vec<(String, *const mimic_syntax::ast::Expr)>, moves: Vec<bool> },
    /// A loop over a state (`x ▷ evolve(n, s => …)`, `x ▷ settle(s => …,
    /// s => …)`): `var` is the state, defined by the loop and seen by its
    /// regions; it starts as `init`; `step` yields the next state. With
    /// `count` (evolve) the step runs that many times (a negative count
    /// stops the program); with `done` (settle) it runs until `done`,
    /// asked before each step, yields true. The loop gives the last state.
    Loop { init: Val, var: Val, count: Option<Val>, done: Option<Region>, step: Region },
}

/// One change of a list of i64 (in a [`Op::With`]'s `changes`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Change {
    Put(Val, Val),
    Push(Val),
}

impl Change {
    fn vals(&self) -> Vec<Val> {
        match self {
            Change::Put(i, x) => vec![*i, *x],
            Change::Push(x) => vec![*x],
        }
    }
}

/// A computation over lists done one element at a time: every list in it
/// is read at the same index (all of one length, or the program stops),
/// and no list is built in between. The fused engine compiles it (SIMD,
/// split over the cores when large).
#[derive(Clone, Debug)]
pub enum Tree {
    /// Element `i` of a list the IR holds, of kind `el`.
    Leaf(Val, mimic_check::types::El),
    /// A number the IR holds, the same for every element.
    Scalar(Val),
    /// A number written in the program.
    Int(i64),
    Float(f64),
    /// `⍳ n`: the index itself (`n` the length).
    Iota(Val),
    Bin(BinOp, Box<Tree>, Box<Tree>),
    Un(UnOp, Box<Tree>),
    /// A list the old path's engine computes element by element (`xs ¨ f`),
    /// built inside the loop from its syntax: `uses` the variables it reads,
    /// `lazies` the lazy lets (as for [`Op::Embed`]).
    Ast { expr: *const mimic_syntax::ast::Expr, uses: Vec<(String, Val, Ty)>, lazies: Vec<(String, *const mimic_syntax::ast::Expr)> },
}

impl Tree {
    /// The IR values it reads.
    pub fn vals(&self, out: &mut Vec<Val>) {
        match self {
            Tree::Leaf(v, _) | Tree::Scalar(v) | Tree::Iota(v) => out.push(*v),
            Tree::Int(_) | Tree::Float(_) => {}
            Tree::Bin(_, a, b) => {
                a.vals(out);
                b.vals(out);
            }
            Tree::Un(_, a) => a.vals(out),
            Tree::Ast { uses, .. } => out.extend(uses.iter().map(|(_, v, _)| *v)),
        }
    }

    pub fn reads(&self, v: Val) -> bool {
        let mut out = Vec::new();
        self.vals(&mut out);
        out.contains(&v)
    }

    fn show(&self) -> String {
        match self {
            Tree::Leaf(v, _) => format!("{v}[i]"),
            Tree::Scalar(v) => v.to_string(),
            Tree::Int(n) => n.to_string(),
            Tree::Float(x) => format!("{x:?}"),
            Tree::Iota(n) => format!("⍳{n}"),
            Tree::Bin(op, a, b) => format!("({} {} {})", a.show(), op.symbol(), b.show()),
            Tree::Un(op, a) => format!("{op:?}({})", a.show()),
            Tree::Ast { uses, lazies, .. } => {
                let mut a: Vec<String> = uses.iter().map(|(n, v, _)| format!("{n}={v}")).collect();
                a.extend(lazies.iter().map(|(n, _)| format!("{n}=lazy")));
                format!("ast({})", a.join(", "))
            }
        }
    }
}

#[derive(Clone, Debug)]
pub struct Inst {
    pub dst: Val,
    pub ty: Ty,
    pub op: Op,
    /// Where it comes from in the source (for debug information).
    pub span: mimic_syntax::Span,
}

/// A list of instructions and the value it yields.
#[derive(Clone, Debug, Default)]
pub struct Region {
    pub insts: Vec<Inst>,
    pub result: Option<Val>,
}

#[derive(Clone, Debug)]
pub struct Function {
    pub name: String,
    pub params: Vec<(String, Ty)>,
    pub ret: Ty,
    pub body: Region,
    /// How many values the function defines (parameters included).
    pub values: u32,
}

impl Op {
    /// The values the operation reads itself (not inside its regions).
    pub fn operands(&self) -> Vec<Val> {
        match self {
            Op::Int(_) | Op::Float(_) | Op::Bool(_) | Op::Unit | Op::Param(_) | Op::Take(_) => vec![],
            Op::Bin(_, a, b) | Op::Index(a, b, _) | Op::Set(a, _, b) => vec![*a, *b],
            Op::Un(_, a) | Op::Get(a, _) | Op::Len(a) | Op::Drop(a) | Op::Field { base: a, .. } => vec![*a],
            Op::Record { fields, .. } => fields.clone(),
            Op::With { base, fields, changes, .. } => std::iter::once(*base).chain(fields.iter().map(|f| f.1)).chain(changes.iter().flat_map(|(_, cs)| cs.iter().flat_map(|c| c.vals()))).collect(),
            Op::Put { list, index, value, .. } => vec![*list, *index, *value],
            Op::Push { list, value, .. } => vec![*list, *value],
            Op::Reduce(_, t, _) | Op::Map(t) => {
                let mut v = Vec::new();
                t.vals(&mut v);
                v
            }
            Op::Call { args, .. } => args.clone(),
            Op::Make(xs) => xs.clone(),
            Op::Embed { uses, .. } => uses.iter().map(|(_, v, _)| *v).collect(),
            Op::If { cond, .. } => vec![*cond],
            Op::Loop { init, count, .. } => std::iter::once(*init).chain(*count).collect(),
        }
    }

    /// The regions inside the operation.
    pub fn regions(&self) -> Vec<&Region> {
        match self {
            Op::If { then, els, .. } => vec![then, els],
            Op::Loop { done, step, .. } => done.iter().chain(std::iter::once(step)).collect(),
            _ => vec![],
        }
    }
}

impl Region {
    /// Whether anything in the region (its regions included) reads `v`.
    pub fn mentions(&self, v: Val) -> bool {
        self.result == Some(v) || self.insts.iter().any(|i| i.op.operands().contains(&v) || i.op.regions().iter().any(|r| r.mentions(v)))
    }
}

/// Whether the instruction gives a value on the heap it owns (a list or a
/// record): a call's, an `if`'s (both branches then yield owned values), a
/// record made or changed, a parameter taken over, …
pub fn owns(i: &Inst) -> bool {
    matches!(i.ty, Ty::List(_) | Ty::Rec(_)) && matches!(i.op, Op::Call { .. } | Op::If { .. } | Op::Embed { .. } | Op::Map(_) | Op::Record { .. } | Op::Field { borrow: false, .. } | Op::With { .. } | Op::Take(_) | Op::Put { .. } | Op::Push { .. })
}

// ---- printing ---------------------------------------------------------------

impl fmt::Display for Function {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let ps: Vec<String> = self.params.iter().enumerate().map(|(i, (n, t))| format!("%{i} {n}: {}", t.name())).collect();
        writeln!(f, "fn {}({}) -> {} {{", self.name, ps.join(", "), self.ret.name())?;
        let mut s = String::new();
        region(&mut s, &self.body, 1);
        f.write_str(&s)?;
        writeln!(f, "}}")
    }
}

fn region(out: &mut String, r: &Region, depth: usize) {
    let pad = "    ".repeat(depth);
    for i in &r.insts {
        let _ = write!(out, "{pad}{} = ", i.dst);
        match &i.op {
            Op::Int(n) => {
                let _ = writeln!(out, "{n}: i64");
            }
            Op::Float(x) => {
                let _ = writeln!(out, "{x:?}: f64");
            }
            Op::Bool(b) => {
                let _ = writeln!(out, "{b}");
            }
            Op::Unit => {
                let _ = writeln!(out, "()");
            }
            Op::Param(n) => {
                let _ = writeln!(out, "param {n}");
            }
            Op::Take(n) => {
                let _ = writeln!(out, "take param {n}: {}", i.ty.name());
            }
            Op::Bin(op, a, b) => {
                let _ = writeln!(out, "{a} {} {b}: {}", op.symbol(), i.ty.name());
            }
            Op::Un(op, a) => {
                let _ = writeln!(out, "{op:?} {a}: {}", i.ty.name());
            }
            Op::Call { callee, args, takes, moves, .. } => {
                let a: Vec<String> = args.iter().enumerate().map(|(k, v)| if moves[k] { format!("move {v}") } else if takes[k] { format!("lend {v}") } else { v.to_string() }).collect();
                let _ = writeln!(out, "call fn#{callee}({}): {}", a.join(", "), i.ty.name());
            }
            Op::Make(xs) => {
                let a: Vec<String> = xs.iter().map(|v| v.to_string()).collect();
                let _ = writeln!(out, "make({}): {}", a.join(", "), i.ty.name());
            }
            Op::Get(v, k) => {
                let _ = writeln!(out, "{v}.{k}: {}", i.ty.name());
            }
            Op::Len(v) => {
                let _ = writeln!(out, "len {v}: {}", i.ty.name());
            }
            Op::Reduce(p, t, _) => {
                let _ = writeln!(out, "reduce {p:?} {}: {}", t.show(), i.ty.name());
            }
            Op::Map(t) => {
                let _ = writeln!(out, "map {}: {}", t.show(), i.ty.name());
            }
            Op::Drop(v) => {
                let _ = writeln!(out, "drop {v}");
            }
            Op::Record { tid, fields, .. } => {
                let a: Vec<String> = fields.iter().map(|v| v.to_string()).collect();
                let _ = writeln!(out, "record #{tid}({}): {}", a.join(", "), i.ty.name());
            }
            Op::Field { base, k, borrow, .. } => {
                let _ = writeln!(out, "{}{base}.field {k}: {}", if *borrow { "borrow " } else { "" }, i.ty.name());
            }
            Op::Put { list, index, value, moved } => {
                let _ = writeln!(out, "put {}{list}[{index}] = {value}: {}", if *moved { "move " } else { "" }, i.ty.name());
            }
            Op::Push { list, value, moved } => {
                let _ = writeln!(out, "push {}{list}, {value}: {}", if *moved { "move " } else { "" }, i.ty.name());
            }
            Op::With { base, fields, moved, changes, .. } => {
                let mut a: Vec<String> = fields.iter().map(|(k, v)| format!(".{k} = {v}")).collect();
                for (k, cs) in changes {
                    let c: Vec<String> = cs
                        .iter()
                        .map(|c| match c {
                            Change::Put(i, x) => format!("put [{i}] = {x}"),
                            Change::Push(x) => format!("push {x}"),
                        })
                        .collect();
                    a.push(format!(".{k} changed by {}", c.join(", ")));
                }
                let _ = writeln!(out, "{}{base} with {}: {}", if *moved { "move " } else { "" }, a.join(", "), i.ty.name());
            }
            Op::Embed { uses, lazies, moves, .. } => {
                let mut a: Vec<String> = uses.iter().zip(moves).map(|((n, v, _), m)| format!("{}{n}={v}", if *m { "move " } else { "" })).collect();
                a.extend(lazies.iter().map(|(n, _)| format!("{n}=lazy")));
                let _ = writeln!(out, "embed({}): {}", a.join(", "), i.ty.name());
            }
            Op::Index(v, k, _) => {
                let _ = writeln!(out, "{v}[{k}]: {}", i.ty.name());
            }
            Op::Set(v, k, x) => {
                let _ = writeln!(out, "{v} with .{k} = {x}: {}", i.ty.name());
            }
            Op::Loop { init, var, count, done, step } => {
                match count {
                    Some(n) => {
                        let _ = writeln!(out, "loop {var} = {init}, {n} times: {} {{", i.ty.name());
                    }
                    None => {
                        let _ = writeln!(out, "loop {var} = {init}: {} {{", i.ty.name());
                    }
                }
                if let Some(d) = done {
                    let _ = writeln!(out, "{pad}  until:");
                    region(out, d, depth + 1);
                }
                let _ = writeln!(out, "{pad}  step:");
                region(out, step, depth + 1);
                let _ = writeln!(out, "{pad}}}");
            }
            Op::If { cond, then, els } => {
                let _ = writeln!(out, "if {cond}: {} {{", i.ty.name());
                region(out, then, depth + 1);
                let _ = writeln!(out, "{pad}}} else {{");
                region(out, els, depth + 1);
                let _ = writeln!(out, "{pad}}}");
            }
        }
    }
    if let Some(v) = r.result {
        let _ = writeln!(out, "{pad}yield {v}");
    }
}

// ---- checking ----------------------------------------------------------------

/// Checks that every value is defined once, before it is used, in a region
/// that encloses the use, and that operands have the types the operations
/// need. Run after lowering (and later after every pass).
/// A tree's leaves are lists, its scalars numbers, and `⍳`'s length an i64.
fn tree_ok(f: &Function, t: &Tree, types: &[Option<Ty>], scope: &[Val]) -> Result<(), String> {
    let ty = |v: Val| {
        if !scope.contains(&v) {
            return Err(format!("{}: {v} is used where it is not defined", f.name));
        }
        types.get(v.0 as usize).copied().flatten().ok_or_else(|| format!("{}: {v} has no type", f.name))
    };
    match t {
        Tree::Leaf(v, _) if !matches!(ty(*v)?, Ty::List(_)) => Err(format!("{}: {v} read element-wise is not a list", f.name)),
        Tree::Scalar(v) if !matches!(ty(*v)?, Ty::I64 | Ty::F64) => Err(format!("{}: {v} is not a number", f.name)),
        Tree::Iota(v) if ty(*v)? != Ty::I64 => Err(format!("{}: ⍳ of {v}, not an i64", f.name)),
        Tree::Bin(_, a, b) => tree_ok(f, a, types, scope).and(tree_ok(f, b, types, scope)),
        Tree::Un(_, a) => tree_ok(f, a, types, scope),
        Tree::Ast { uses, .. } => {
            for (_, v, t) in uses {
                if ty(*v)? != *t {
                    return Err(format!("{}: a tree's syntax reads {v} as another type", f.name));
                }
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

pub fn verify(f: &Function) -> Result<(), String> {
    let mut types: Vec<Option<Ty>> = vec![None; f.values as usize];
    let mut scope: Vec<Val> = Vec::new();
    verify_region(f, &f.body, &mut types, &mut scope)?;
    if let Some(v) = f.body.result {
        let t = types[v.0 as usize].ok_or_else(|| format!("{}: the result {v} is not defined", f.name))?;
        if t != f.ret {
            return Err(format!("{}: yields {} for a {} result", f.name, t.name(), f.ret.name()));
        }
    }
    Ok(())
}

fn verify_region(f: &Function, r: &Region, types: &mut Vec<Option<Ty>>, scope: &mut Vec<Val>) -> Result<(), String> {
    let mark = scope.len();
    let seen = |scope: &Vec<Val>, v: Val| scope.contains(&v);
    for i in &r.insts {
        let ty = |v: Val, types: &Vec<Option<Ty>>| -> Result<Ty, String> {
            if !seen(scope, v) {
                return Err(format!("{}: {v} is used where it is not defined", f.name));
            }
            types[v.0 as usize].ok_or_else(|| format!("{}: {v} has no type", f.name))
        };
        match &i.op {
            Op::Int(_) | Op::Float(_) | Op::Bool(_) | Op::Unit => {}
            Op::Param(n) | Op::Take(n) => {
                let want = f.params.get(*n as usize).map(|p| p.1).ok_or_else(|| format!("{}: no parameter {n}", f.name))?;
                if want != i.ty {
                    return Err(format!("{}: parameter {n} is {}, not {}", f.name, want.name(), i.ty.name()));
                }
            }
            Op::Bin(_, a, b) => {
                ty(*a, types)?;
                ty(*b, types)?;
            }
            Op::Un(_, a) => {
                ty(*a, types)?;
            }
            Op::Make(args) => {
                for a in args {
                    ty(*a, types)?;
                }
            }
            Op::Call { args, takes, moves, .. } => {
                for a in args {
                    ty(*a, types)?;
                }
                if takes.len() != args.len() || moves.len() != args.len() || moves.iter().zip(takes).any(|(m, t)| *m && !t) {
                    return Err(format!("{}: a call {} moves an argument its callee does not take", f.name, i.dst));
                }
            }
            Op::Embed { uses, .. } => {
                for (_, a, t) in uses {
                    if ty(*a, types)? != *t {
                        return Err(format!("{}: an embedded expression reads {a} as another type", f.name));
                    }
                }
            }
            Op::Len(v) => {
                if !matches!(ty(*v, types)?, Ty::List(_)) {
                    return Err(format!("{}: {v} is not a list", f.name));
                }
            }
            Op::Reduce(_, t, el) => {
                let want = if *el == mimic_check::types::El::F64 { Ty::F64 } else { Ty::I64 };
                if i.ty != want {
                    return Err(format!("{}: a reduction of {el:?} giving {}", f.name, i.ty.name()));
                }
                tree_ok(f, t, types, scope)?;
            }
            Op::Map(t) => {
                if !matches!(i.ty, Ty::List(_)) {
                    return Err(format!("{}: a map giving {}", f.name, i.ty.name()));
                }
                tree_ok(f, t, types, scope)?;
            }
            Op::Drop(v) => {
                if !matches!(ty(*v, types)?, Ty::List(_) | Ty::Rec(_)) {
                    return Err(format!("{}: drop of {v}, not on the heap", f.name));
                }
            }
            Op::Record { fields, moves, .. } => {
                if moves.len() != fields.len() {
                    return Err(format!("{}: a record {} with moves not one per field", f.name, i.dst));
                }
                for v in fields {
                    ty(*v, types)?;
                }
                if !matches!(i.ty, Ty::Rec(_)) {
                    return Err(format!("{}: a record giving {}", f.name, i.ty.name()));
                }
            }
            Op::Field { base, .. } => {
                if !matches!(ty(*base, types)?, Ty::Rec(_)) {
                    return Err(format!("{}: a field of {base}, not a record", f.name));
                }
            }
            Op::Put { list, index, value, .. } => {
                if ty(*list, types)? != i.ty || ty(*index, types)? != Ty::I64 || ty(*value, types)? != Ty::I64 || i.ty != Ty::List(mimic_check::types::Ty::Arr(mimic_check::types::El::I64, 0)) {
                    return Err(format!("{}: put {} is not of a list of i64", f.name, i.dst));
                }
            }
            Op::Push { list, value, .. } => {
                if ty(*list, types)? != i.ty || ty(*value, types)? != Ty::I64 || i.ty != Ty::List(mimic_check::types::Ty::Arr(mimic_check::types::El::I64, 0)) {
                    return Err(format!("{}: push {} is not of a list of i64", f.name, i.dst));
                }
            }
            Op::With { base, fields, moves, changes, .. } => {
                for (_, cs) in changes {
                    for c in cs {
                        for v in c.vals() {
                            if ty(v, types)? != Ty::I64 {
                                return Err(format!("{}: a change in `with` {} by {v}, not an i64", f.name, i.dst));
                            }
                        }
                    }
                }
                if moves.len() != fields.len() {
                    return Err(format!("{}: a `with` {} with moves not one per field", f.name, i.dst));
                }
                if ty(*base, types)? != i.ty || !matches!(i.ty, Ty::Rec(_)) {
                    return Err(format!("{}: `with` on {base}, not a record of its type", f.name));
                }
                for (_, v) in fields {
                    ty(*v, types)?;
                }
            }
            Op::Index(v, k, _) => {
                if !matches!(ty(*v, types)?, Ty::List(_)) || ty(*k, types)? != Ty::I64 {
                    return Err(format!("{}: {v}[{k}] is not a list indexed by an i64", f.name));
                }
            }
            Op::Get(v, _) => {
                if !matches!(ty(*v, types)?, Ty::Agg(_)) {
                    return Err(format!("{}: {v} has no parts", f.name));
                }
            }
            Op::Set(v, _, x) => {
                if !matches!(ty(*v, types)?, Ty::Agg(_)) {
                    return Err(format!("{}: {v} has no parts", f.name));
                }
                ty(*x, types)?;
            }
            Op::Loop { init, var, count, done, step } => {
                let st = ty(*init, types)?;
                if st != i.ty {
                    return Err(format!("{}: the loop {} starts as another type", f.name, i.dst));
                }
                if let Some(n) = count
                    && ty(*n, types)? != Ty::I64
                {
                    return Err(format!("{}: the loop {} counts with a value that is not an i64", f.name, i.dst));
                }
                // the state: defined by the loop, seen inside it
                let vd = var.0 as usize;
                if vd >= types.len() || types[vd].is_some() {
                    return Err(format!("{}: the loop's state {var} is defined twice", f.name));
                }
                types[vd] = Some(st);
                scope.push(*var);
                if let Some(d) = done {
                    verify_region(f, d, types, scope)?;
                    let v = d.result.ok_or_else(|| format!("{}: the loop's until yields nothing", f.name))?;
                    if types[v.0 as usize] != Some(Ty::Bool) {
                        return Err(format!("{}: the loop's until is not a truth value", f.name));
                    }
                }
                verify_region(f, step, types, scope)?;
                let v = step.result.ok_or_else(|| format!("{}: the loop's step yields nothing", f.name))?;
                if types[v.0 as usize] != Some(st) {
                    return Err(format!("{}: the loop's step yields another type", f.name));
                }
                scope.retain(|x| x != var);
            }
            Op::If { cond, then, els } => {
                if ty(*cond, types)? != Ty::Bool {
                    return Err(format!("{}: the condition {cond} is not a truth value", f.name));
                }
                for b in [then, els] {
                    verify_region(f, b, types, scope)?;
                    if i.ty != Ty::Unit {
                        let v = b.result.ok_or_else(|| format!("{}: a branch of {} yields nothing", f.name, i.dst))?;
                        if types[v.0 as usize] != Some(i.ty) {
                            return Err(format!("{}: a branch of {} yields another type", f.name, i.dst));
                        }
                    }
                }
            }
        }
        let d = i.dst.0 as usize;
        if d >= types.len() {
            return Err(format!("{}: {} is past the function's values", f.name, i.dst));
        }
        if types[d].is_some() {
            return Err(format!("{}: {} is defined twice", f.name, i.dst));
        }
        types[d] = Some(i.ty);
        scope.push(i.dst);
    }
    if let Some(v) = r.result
        && !seen(scope, v)
    {
        return Err(format!("{}: yields {v}, which is not defined here", f.name));
    }
    scope.truncate(mark);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use mimic_syntax::Span;

    fn inst(dst: u32, ty: Ty, op: Op) -> Inst {
        Inst { dst: Val(dst), ty, op, span: Span::default() }
    }

    fn func(insts: Vec<Inst>, result: u32, values: u32) -> Function {
        Function { name: "f".into(), params: vec![("a".into(), Ty::I64)], ret: Ty::I64, body: Region { insts, result: Some(Val(result)) }, values }
    }

    #[test]
    fn verify_accepts_a_good_function() {
        let f = func(vec![inst(0, Ty::I64, Op::Param(0)), inst(1, Ty::I64, Op::Int(1)), inst(2, Ty::I64, Op::Bin(BinOp::Add, Val(0), Val(1)))], 2, 3);
        assert_eq!(verify(&f), Ok(()));
        assert!(f.to_string().contains("%2 = %0 + %1: i64"), "{f}");
    }

    #[test]
    fn verify_rejects_a_value_defined_twice() {
        let f = func(vec![inst(0, Ty::I64, Op::Param(0)), inst(0, Ty::I64, Op::Int(1))], 0, 1);
        assert!(verify(&f).unwrap_err().contains("defined twice"));
    }

    #[test]
    fn verify_rejects_a_use_before_its_definition() {
        let f = func(vec![inst(0, Ty::I64, Op::Bin(BinOp::Add, Val(1), Val(1))), inst(1, Ty::I64, Op::Int(1))], 0, 2);
        assert!(verify(&f).unwrap_err().contains("not defined"));
    }

    #[test]
    fn verify_rejects_a_value_from_inside_a_branch() {
        // %2 is defined in the `then` region: it does not exist after the `if`
        let then = Region { insts: vec![inst(2, Ty::I64, Op::Int(5))], result: Some(Val(2)) };
        let els = Region { insts: vec![inst(3, Ty::I64, Op::Int(6))], result: Some(Val(3)) };
        let f = func(
            vec![inst(0, Ty::I64, Op::Param(0)), inst(1, Ty::Bool, Op::Bool(true)), inst(4, Ty::I64, Op::If { cond: Val(1), then, els }), inst(5, Ty::I64, Op::Bin(BinOp::Add, Val(2), Val(4)))],
            5,
            6,
        );
        assert!(verify(&f).unwrap_err().contains("%2"));
    }

    #[test]
    fn verify_rejects_a_condition_that_is_not_a_truth_value() {
        let then = Region { insts: vec![], result: Some(Val(0)) };
        let els = Region { insts: vec![], result: Some(Val(0)) };
        let f = func(vec![inst(0, Ty::I64, Op::Param(0)), inst(1, Ty::I64, Op::If { cond: Val(0), then, els })], 1, 2);
        assert!(verify(&f).unwrap_err().contains("truth value"));
    }
}
