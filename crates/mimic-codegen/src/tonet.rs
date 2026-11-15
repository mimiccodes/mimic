//! Compile pure Mimic functions to interaction nets.
//!
//! Every independent subexpression becomes an independent redex, so the
//! runtime reduces it in parallel — no annotations needed.
//!
//! * **Eligibility.** A function is compiled to a net when it is pure (see
//!   `mimic_check::Purity`) and its body uses only the supported subset: numbers,
//!   bools, variables, `let`, operators, primitives (except `¨`), indexing,
//!   `if`/`else`, tail `return`s, and calls to other eligible functions.
//!   Everything else stays native; every function keeps its LLVM version.
//! * **Entry.** An eligible function that calls functions (recursion, fan-out)
//!   is *entered* through the net: its native body becomes a wrapper around
//!   `mim_net_call`. Leaf functions stay native when called from native code
//!   (nothing to parallelise) and are REFs when called from nets.
//! * **Sharing.** A variable used k times is bound to a chain of k-1 DUP nodes;
//!   unused ones are erased.
//! * **Laziness.** `if` compiles to SWI over two lifted branch definitions that
//!   receive the free variables as arguments, so only the taken branch expands.

use mimic_check::{FnId, FnKind, ModId, Program, Purity, Resolved};
use mimic_net::{Book, Def, Num, Op, Tree};
use mimic_syntax::ast::*;
use mimic_syntax::glyphs::Prim;
use std::collections::{BTreeSet, HashMap, HashSet};

#[derive(Default)]
pub struct NetPlan {
    pub book: Book,
    /// Book index of each compiled function's definition.
    pub def_index: HashMap<FnId, u32>,
    /// Functions whose native body is a wrapper that runs the net.
    pub entered: HashSet<FnId>,
    /// Messages for type-annotation checks inside nets (`Op::Check` indices).
    pub check_msgs: Vec<String>,
}

pub fn plan(p: &Program, purity: &Purity) -> NetPlan {
    // 1. desugar tail returns and check the syntactic subset
    let mut cand: HashMap<FnId, (Block, Vec<FnId>)> = HashMap::new();
    for (fid, f) in p.fns.iter().enumerate() {
        let FnKind::User(d) = &f.kind else { continue };
        if !purity.fns[fid] {
            continue;
        }
        // nets carry 64-bit numbers: f32 and i32 code stays native
        let narrow = d.params.iter().filter_map(|p| p.ty.as_ref()).chain(d.ret.as_ref()).any(mentions_32) || {
            let mut found = false;
            d.body.walk(&mut |x| {
                if let ExprKind::Block(b) = &x.kind {
                    found |= b.stmts.iter().any(|s| matches!(s, Stmt::Let { ty: Some(t), .. } if mentions_32(t)));
                }
            });
            found || d.body.stmts.iter().any(|s| matches!(s, Stmt::Let { ty: Some(t), .. } if mentions_32(t)))
        };
        if narrow {
            continue;
        }
        let Some(body) = desugar_block(&d.body) else { continue };
        let mut callees = Vec::new();
        if (Eligible { p, module: f.module, callees: &mut callees }).block(&body) {
            cand.insert(fid, (body, callees));
        }
    }
    // 2. only keep functions whose callees are all compiled too (fixpoint)
    loop {
        let drop: Vec<FnId> = cand.iter().filter(|(_, (_, cs))| cs.iter().any(|c| !cand.contains_key(c))).map(|(f, _)| *f).collect();
        if drop.is_empty() {
            break;
        }
        for f in drop {
            cand.remove(&f);
        }
    }
    let mut ids: Vec<FnId> = cand.keys().copied().collect();
    ids.sort();
    let names: HashMap<FnId, String> = ids.iter().map(|f| (*f, def_name(p, *f))).collect();

    // 3. compile
    let mut defs = Vec::new();
    let mut check_msgs = Vec::new();
    for fid in &ids {
        let (body, _) = &cand[fid];
        let FnKind::User(d) = &p.fns[*fid].kind else { unreachable!() };
        let mut rn = Renamer::default();
        let params: Vec<String> = d.params.iter().map(|prm| rn.bind(&prm.name)).collect();
        let body = rn.block(body);
        // the same type checks native code performs (annotations are enforced in nets too)
        let path = p.fn_path(*fid).join("::");
        let mut checks: Vec<Option<Op>> = Vec::new();
        for prm in &d.params {
            checks.push(prm.ty.as_ref().and_then(|t| mimic_check::type_code(t).ok()).map(|code| {
                check_msgs.push(format!("argument `{}` of `{path}`", prm.name));
                Op::Check(code as u8, (check_msgs.len() - 1) as u16)
            }));
        }
        let ret_check = d.ret.as_ref().and_then(|t| mimic_check::type_code(t).ok()).map(|code| {
            check_msgs.push(format!("return value of `{path}`"));
            Op::Check(code as u8, (check_msgs.len() - 1) as u16)
        });
        let mut g = Gen {
            p,
            module: p.fns[*fid].module,
            names: &names,
            defs: Vec::new(),
            fresh: 0,
            branches: 0,
            base: names[fid].clone(),
            msg_base: check_msgs.len(),
            msgs: Vec::new(),
            path: path.clone(),
        };
        let main = g.function(names[fid].clone(), &params, &checks, ret_check, &body);
        defs.push(main);
        defs.extend(g.defs);
        check_msgs.extend(g.msgs);
    }
    let def_index: HashMap<FnId, u32> =
        ids.iter().map(|f| (*f, defs.iter().position(|d| d.name == names[f]).unwrap() as u32)).collect();
    let entered = ids.iter().copied().filter(|f| !cand[f].1.is_empty()).collect();
    // what needs no definition expanded is reduced now, once (F7)
    let mut book = Book { defs };
    book.pre_reduce();
    NetPlan { book, def_index, entered, check_msgs }
}

fn def_name(p: &Program, f: FnId) -> String {
    p.fn_path(f).join(".")
}

// ---- desugaring: tail `return`s become if/else --------------------------------

fn always_returns(b: &Block) -> bool {
    if b.tail.is_some() {
        return false;
    }
    match b.stmts.last() {
        Some(Stmt::Return(Some(_), _)) => true,
        Some(Stmt::Expr(Expr { kind: ExprKind::If { then, els: Some(e), .. }, .. })) => {
            always_returns(then) && matches!(&e.kind, ExprKind::Block(b) if always_returns(b))
        }
        _ => false,
    }
}

fn contains_return(b: &Block) -> bool {
    fn e(x: &Expr) -> bool {
        match &x.kind {
            ExprKind::If { cond, then, els } => e(cond) || contains_return(then) || els.as_deref().is_some_and(e),
            ExprKind::Block(b) | ExprKind::Unsafe(b) => contains_return(b),
            _ => false, // returns cannot hide in other expressions in the subset
        }
    }
    b.stmts.iter().any(|s| match s {
        Stmt::Return(..) => true,
        Stmt::Let { value, .. } => e(value),
        Stmt::Expr(x) => e(x),
        _ => false,
    }) || b.tail.as_deref().is_some_and(e)
}

/// Does a type annotation name f32 or i32?
fn mentions_32(t: &Type) -> bool {
    match t {
        Type::Named(n, _) => n == "f32" || n == "i32",
        Type::App(_, ts, _) | Type::Tuple(ts, _) => ts.iter().any(mentions_32),
        Type::Array(t, _) | Type::Ptr(t, _) => mentions_32(t),
        Type::Map(a, b, _) => mentions_32(a) || mentions_32(b),
        Type::Fn(ps, r, _) => ps.iter().any(mentions_32) || r.as_deref().is_some_and(mentions_32),
    }
}

/// Rewrite a block so that it has no `return` statements, or give up.
fn desugar_block(b: &Block) -> Option<Block> {
    let mut out = Block { stmts: Vec::new(), tail: None, span: b.span };
    for (i, s) in b.stmts.iter().enumerate() {
        match s {
            Stmt::Return(Some(e), _) => {
                out.tail = Some(Box::new(desugar_expr(e)?));
                return Some(out); // anything after is dead code
            }
            Stmt::Return(None, _) => return None,
            Stmt::Expr(Expr { kind: ExprKind::If { cond, then, els }, span }) if contains_return(then) || els.as_deref().is_some_and(|e| matches!(&e.kind, ExprKind::Block(b) if contains_return(b))) => {
                // `if c { …; return a } rest`  ⇒  `if c { … a } else { rest }`
                let rest = Block { stmts: b.stmts[i + 1..].to_vec(), tail: b.tail.clone(), span: b.span };
                let then2 = desugar_block(then)?;
                let else_block = match els {
                    None if always_returns(then) => desugar_block(&rest)?,
                    Some(e) => match &e.kind {
                        ExprKind::Block(eb) if always_returns(then) && always_returns(eb) && rest.stmts.is_empty() && rest.tail.is_none() => desugar_block(eb)?,
                        _ => return None,
                    },
                    None => return None,
                };
                let span = *span;
                out.tail = Some(Box::new(Expr {
                    kind: ExprKind::If {
                        cond: cond.clone(),
                        then: then2,
                        els: Some(Box::new(Expr { span: else_block.span, kind: ExprKind::Block(else_block) })),
                    },
                    span,
                }));
                return Some(out);
            }
            other => {
                if let Stmt::Let { value, .. } | Stmt::Expr(value) = other {
                    let v = desugar_expr(value)?;
                    out.stmts.push(match other {
                        Stmt::Let { name, mutable, ty, span, .. } => Stmt::Let { name: name.clone(), mutable: *mutable, ty: ty.clone(), value: v, span: *span },
                        _ => Stmt::Expr(v),
                    });
                } else {
                    out.stmts.push(other.clone());
                }
            }
        }
    }
    out.tail = match &b.tail {
        Some(t) => Some(Box::new(desugar_expr(t)?)),
        None => None,
    };
    Some(out)
}

fn desugar_expr(e: &Expr) -> Option<Expr> {
    Some(match &e.kind {
        ExprKind::If { cond, then, els } => Expr {
            kind: ExprKind::If {
                cond: cond.clone(),
                then: desugar_block(then)?,
                els: match els {
                    Some(x) => Some(Box::new(desugar_expr(x)?)),
                    None => None,
                },
            },
            span: e.span,
        },
        ExprKind::Block(b) => Expr { kind: ExprKind::Block(desugar_block(b)?), span: e.span },
        _ => e.clone(),
    })
}

// ---- eligibility ----------------------------------------------------------------

struct Eligible<'a> {
    p: &'a Program,
    module: ModId,
    callees: &'a mut Vec<FnId>,
}

impl Eligible<'_> {
    fn block(&mut self, b: &Block) -> bool {
        b.stmts.iter().all(|s| match s {
            Stmt::Let { mutable: false, value, .. } => self.expr(value),
            Stmt::Expr(e) => self.expr(e),
            _ => false, // mutation and leftover returns stay native
        }) && b.tail.as_deref().is_some_and(|t| self.expr(t))
    }

    fn expr(&mut self, e: &Expr) -> bool {
        match &e.kind {
            ExprKind::Int(_) | ExprKind::Float(_) | ExprKind::Bool(_) => true,
            // variables only: uppercase names are variants (not in the net subset yet)
            // (a function named as a value is a closure, not a net variable)
            ExprKind::Path(p) => {
                p.len() == 1 && !mimic_syntax::parser::is_upper(&p[0]) && !matches!(self.p.lookup(self.module, p), Some(Resolved::Fn(_)))
            }
            ExprKind::Binary { lhs, rhs, .. } => self.expr(lhs) && self.expr(rhs),
            ExprKind::Unary { expr, .. } => self.expr(expr),
            ExprKind::Index { base, index } => self.expr(base) && self.expr(index),
            ExprKind::Prim { prim, args } => {
                !matches!(prim, Prim::Each | Prim::Print | Prim::Gather | Prim::Trail | Prim::Evolve | Prim::Settle) && args.iter().all(|a| self.expr(a))
            }
            ExprKind::Call { callee, args } => {
                let ExprKind::Path(path) = &callee.kind else { return false };
                match self.p.resolve(self.module, path) {
                    Ok(Resolved::Fn(f)) if !self.p.fns[f].is_extern() => {
                        self.callees.push(f);
                        args.iter().all(|a| self.expr(a))
                    }
                    _ => false,
                }
            }
            ExprKind::If { cond, then, els: Some(els) } => self.expr(cond) && self.block(then) && self.expr(els),
            ExprKind::Block(b) => self.block(b),
            _ => false, // strings, array literals, lambdas, unsafe, if-without-else
        }
    }
}

// ---- renaming: every binding gets a unique name ----------------------------------

#[derive(Default)]
struct Renamer {
    scopes: Vec<HashMap<String, String>>,
    n: usize,
}

impl Renamer {
    fn bind(&mut self, name: &str) -> String {
        if self.scopes.is_empty() {
            self.scopes.push(HashMap::new());
        }
        self.n += 1;
        let u = format!("{name}_{}", self.n);
        self.scopes.last_mut().unwrap().insert(name.into(), u.clone());
        u
    }
    fn lookup(&self, name: &str) -> Option<String> {
        self.scopes.iter().rev().find_map(|s| s.get(name).cloned())
    }
    fn block(&mut self, b: &Block) -> Block {
        self.scopes.push(HashMap::new());
        let stmts = b
            .stmts
            .iter()
            .map(|s| match s {
                Stmt::Let { name, mutable, ty, value, span } => {
                    let value = self.expr(value);
                    Stmt::Let { name: self.bind(name), mutable: *mutable, ty: ty.clone(), value, span: *span }
                }
                Stmt::Expr(e) => Stmt::Expr(self.expr(e)),
                other => other.clone(),
            })
            .collect();
        let tail = b.tail.as_ref().map(|t| Box::new(self.expr(t)));
        self.scopes.pop();
        Block { stmts, tail, span: b.span }
    }
    fn expr(&mut self, e: &Expr) -> Expr {
        let kind = match &e.kind {
            ExprKind::Path(p) if p.len() == 1 => ExprKind::Path(vec![self.lookup(&p[0]).unwrap_or_else(|| p[0].clone())]),
            ExprKind::Binary { op, lhs, rhs } => ExprKind::Binary { op: *op, lhs: Box::new(self.expr(lhs)), rhs: Box::new(self.expr(rhs)) },
            ExprKind::Unary { op, expr } => ExprKind::Unary { op: *op, expr: Box::new(self.expr(expr)) },
            ExprKind::Index { base, index } => ExprKind::Index { base: Box::new(self.expr(base)), index: Box::new(self.expr(index)) },
            ExprKind::Prim { prim, args } => ExprKind::Prim { prim: *prim, args: args.iter().map(|a| self.expr(a)).collect() },
            ExprKind::Call { callee, args } => ExprKind::Call { callee: callee.clone(), args: args.iter().map(|a| self.expr(a)).collect() },
            ExprKind::If { cond, then, els } => ExprKind::If {
                cond: Box::new(self.expr(cond)),
                then: self.block(then),
                els: els.as_ref().map(|x| Box::new(self.expr(x))),
            },
            ExprKind::Block(b) => ExprKind::Block(self.block(b)),
            other => other.clone(),
        };
        Expr { kind, span: e.span }
    }
}

// ---- free variables and use counts ---------------------------------------------

/// Variables referenced in `e` (after renaming every name is unique).
fn refs_expr(e: &Expr, out: &mut BTreeSet<String>, bound: &mut BTreeSet<String>) {
    match &e.kind {
        ExprKind::Path(p) if p.len() == 1 => {
            out.insert(p[0].clone());
        }
        ExprKind::Binary { lhs, rhs, .. } => {
            refs_expr(lhs, out, bound);
            refs_expr(rhs, out, bound);
        }
        ExprKind::Unary { expr, .. } => refs_expr(expr, out, bound),
        ExprKind::Index { base, index } => {
            refs_expr(base, out, bound);
            refs_expr(index, out, bound);
        }
        ExprKind::Prim { args, .. } | ExprKind::Call { args, .. } => args.iter().for_each(|a| refs_expr(a, out, bound)),
        ExprKind::If { cond, then, els } => {
            refs_expr(cond, out, bound);
            refs_block(then, out, bound);
            if let Some(x) = els {
                refs_expr(x, out, bound);
            }
        }
        ExprKind::Block(b) => refs_block(b, out, bound),
        _ => {}
    }
}

fn refs_block(b: &Block, out: &mut BTreeSet<String>, bound: &mut BTreeSet<String>) {
    for s in &b.stmts {
        match s {
            Stmt::Let { name, value, .. } => {
                refs_expr(value, out, bound);
                bound.insert(name.clone());
            }
            Stmt::Expr(e) => refs_expr(e, out, bound),
            _ => {}
        }
    }
    if let Some(t) = &b.tail {
        refs_expr(t, out, bound);
    }
}

fn free_vars(b: &Body) -> BTreeSet<String> {
    let (mut out, mut bound) = (BTreeSet::new(), BTreeSet::new());
    match b {
        Body::Block(x) => refs_block(x, &mut out, &mut bound),
        Body::Expr(x) => refs_expr(x, &mut out, &mut bound),
    }
    out.difference(&bound).cloned().collect()
}

fn if_free_vars(then: &Block, els: &Expr) -> Vec<String> {
    let mut fv = free_vars(&Body::Block(then));
    fv.extend(free_vars(&Body::Expr(els)));
    fv.into_iter().collect()
}

/// How many times each variable is consumed in this region. A nested `if`
/// consumes each of its free variables once (they are passed to the branch).
fn count_expr(e: &Expr, c: &mut HashMap<String, usize>) {
    match &e.kind {
        ExprKind::Path(p) if p.len() == 1 => *c.entry(p[0].clone()).or_default() += 1,
        ExprKind::Binary { lhs, rhs, .. } => {
            count_expr(lhs, c);
            count_expr(rhs, c);
        }
        ExprKind::Unary { expr, .. } => count_expr(expr, c),
        ExprKind::Index { base, index } => {
            count_expr(base, c);
            count_expr(index, c);
        }
        ExprKind::Prim { args, .. } | ExprKind::Call { args, .. } => args.iter().for_each(|a| count_expr(a, c)),
        ExprKind::If { cond, then, els: Some(els) } => {
            count_expr(cond, c);
            for v in if_free_vars(then, els) {
                *c.entry(v).or_default() += 1;
            }
        }
        ExprKind::Block(b) => count_block(b, c),
        _ => {}
    }
}

fn count_block(b: &Block, c: &mut HashMap<String, usize>) {
    for s in &b.stmts {
        if let Stmt::Let { value, .. } | Stmt::Expr(value) = s {
            count_expr(value, c);
        }
    }
    if let Some(t) = &b.tail {
        count_expr(t, c);
    }
}

// ---- generation ---------------------------------------------------------------

enum Body<'a> {
    Block(&'a Block),
    Expr(&'a Expr),
}

struct Region {
    redexes: Vec<(Tree, Tree)>,
    counts: HashMap<String, usize>,
    taken: HashMap<String, usize>,
}

struct Gen<'a> {
    p: &'a Program,
    module: ModId,
    names: &'a HashMap<FnId, String>,
    /// lifted branch definitions
    defs: Vec<Def>,
    fresh: usize,
    branches: usize,
    base: String,
    /// `let x: T` check messages, appended to the plan's table at `msg_base`
    msg_base: usize,
    msgs: Vec<String>,
    path: String,
}

fn var(s: impl Into<String>) -> Tree {
    Tree::Var(s.into())
}
fn con(a: Tree, b: Tree) -> Tree {
    Tree::Con(Box::new(a), Box::new(b))
}
fn opr(op: Op, a: Tree, b: Tree) -> Tree {
    Tree::Opr(op, Box::new(a), Box::new(b))
}

impl Gen<'_> {
    fn wire(&mut self) -> String {
        self.fresh += 1;
        format!("t{}", self.fresh)
    }

    /// The binding site of a variable: erased, a single wire, or a DUP chain.
    fn bind(&self, rg: &Region, name: &str) -> Tree {
        let k = rg.counts.get(name).copied().unwrap_or(0);
        match k {
            0 => Tree::Era,
            _ => {
                let mut t = var(format!("{name}_u{}", k - 1));
                for i in (0..k - 1).rev() {
                    t = Tree::Dup(0, Box::new(var(format!("{name}_u{i}"))), Box::new(t));
                }
                t
            }
        }
    }

    fn take(&self, rg: &mut Region, name: &str) -> Tree {
        let i = rg.taken.entry(name.into()).or_default();
        let t = var(format!("{name}_u{i}"));
        *i += 1;
        t
    }

    /// A function definition: like a region, but arguments and the result pass
    /// through their type-annotation checks.
    fn function(&mut self, name: String, params: &[String], checks: &[Option<Op>], ret: Option<Op>, body: &Block) -> Def {
        let mut counts = HashMap::new();
        count_block(body, &mut counts);
        let mut rg = Region { redexes: Vec::new(), counts, taken: HashMap::new() };
        let mut binds = Vec::new();
        for (prm, chk) in params.iter().zip(checks) {
            let site = self.bind(&rg, prm);
            binds.push(match chk {
                Some(op) => {
                    let w = self.wire();
                    rg.redexes.push((var(&w), opr(*op, Tree::Era, site)));
                    var(w)
                }
                None => site,
            });
        }
        let mut result = self.block(&mut rg, body);
        if let Some(op) = ret {
            result = self.op_node(&mut rg, op, result, Tree::Era);
        }
        let root = binds.into_iter().rev().fold(result, |acc, b| con(b, acc));
        Def { name, root, redexes: rg.redexes }
    }

    /// A definition `@name = (p1 (p2 … body))` for one region.
    fn region(&mut self, name: String, params: &[String], body: Body) -> Def {
        let mut counts = HashMap::new();
        match &body {
            Body::Block(b) => count_block(b, &mut counts),
            Body::Expr(e) => count_expr(e, &mut counts),
        }
        let mut rg = Region { redexes: Vec::new(), counts, taken: HashMap::new() };
        let binds: Vec<Tree> = params.iter().map(|p| self.bind(&rg, p)).collect();
        let result = match body {
            Body::Block(b) => self.block(&mut rg, b),
            Body::Expr(e) => self.expr(&mut rg, e),
        };
        let root = binds.into_iter().rev().fold(result, |acc, b| con(b, acc));
        Def { name, root, redexes: rg.redexes }
    }

    fn block(&mut self, rg: &mut Region, b: &Block) -> Tree {
        for s in &b.stmts {
            match s {
                Stmt::Let { name, value, ty, .. } => {
                    let mut t = self.expr(rg, value);
                    if let Some(code) = ty.as_ref().and_then(|t| mimic_check::type_code(t).ok()) {
                        // renamed `x_7` → report the source name `x`
                        let shown = name.rsplit_once('_').map(|(a, _)| a).unwrap_or(name);
                        self.msgs.push(format!("`let {shown}` in `{}`", self.path));
                        let op = Op::Check(code as u8, (self.msg_base + self.msgs.len() - 1) as u16);
                        t = self.op_node(rg, op, t, Tree::Era);
                    }
                    let site = self.bind(rg, name);
                    rg.redexes.push((t, site));
                }
                Stmt::Expr(e) => {
                    let t = self.expr(rg, e);
                    rg.redexes.push((t, Tree::Era));
                }
                _ => unreachable!("not eligible"),
            }
        }
        self.expr(rg, b.tail.as_deref().expect("eligible blocks have a tail"))
    }

    fn op_node(&mut self, rg: &mut Region, op: Op, principal: Tree, other: Tree) -> Tree {
        let r = self.wire();
        rg.redexes.push((principal, opr(op, other, var(&r))));
        var(r)
    }

    fn expr(&mut self, rg: &mut Region, e: &Expr) -> Tree {
        match &e.kind {
            ExprKind::Int(i) => Tree::Num(Num::Int(*i)),
            ExprKind::Float(f) => Tree::Num(Num::F64(*f)),
            ExprKind::Bool(b) => Tree::Num(Num::Bool(*b)),
            ExprKind::Path(p) => self.take(rg, &p[0]),
            ExprKind::Binary { op, lhs, rhs } => {
                let a = self.expr(rg, lhs);
                let b = self.expr(rg, rhs);
                self.op_node(rg, Op::Bin(*op), a, b)
            }
            ExprKind::Unary { op, expr } => {
                let a = self.expr(rg, expr);
                let op = match op {
                    UnOp::Neg => Op::Neg,
                    UnOp::Not => Op::Not,
                    UnOp::Abs => Op::Abs,
                    UnOp::BitNot => Op::BitNot,
                };
                self.op_node(rg, op, a, Tree::Era)
            }
            ExprKind::Index { base, index } => {
                let a = self.expr(rg, base);
                let i = self.expr(rg, index);
                self.op_node(rg, Op::Index, a, i)
            }
            ExprKind::Prim { prim, args } => {
                let a = self.expr(rg, &args[0]);
                if prim.arity() == 2 {
                    let b = self.expr(rg, &args[1]);
                    self.op_node(rg, Op::Prim2(*prim), a, b)
                } else {
                    self.op_node(rg, Op::Prim1(*prim), a, Tree::Era)
                }
            }
            ExprKind::Call { callee, args } => {
                let ExprKind::Path(path) = &callee.kind else { unreachable!() };
                let Ok(Resolved::Fn(f)) = self.p.resolve(self.module, path) else { unreachable!() };
                let target = Tree::Ref(self.names[&f].clone());
                if args.is_empty() {
                    return target; // a lazy constant
                }
                let argv: Vec<Tree> = args.iter().map(|a| self.expr(rg, a)).collect();
                let r = self.wire();
                let app = argv.into_iter().rev().fold(var(&r), |acc, a| con(a, acc));
                rg.redexes.push((target, app));
                var(r)
            }
            ExprKind::If { cond, then, els: Some(els) } => {
                let fv = if_free_vars(then, els);
                self.branches += 1;
                let n = self.branches;
                let then_name = format!("{}.then{n}", self.base);
                let else_name = format!("{}.else{n}", self.base);
                let then_def = self.region(then_name.clone(), &fv, Body::Block(then));
                let else_def = self.region(else_name.clone(), &fv, Body::Expr(els));
                self.defs.push(then_def);
                self.defs.push(else_def);
                let c = self.expr(rg, cond);
                let r = self.wire();
                let args = fv.iter().rev().fold(var(&r), |acc, v| {
                    let u = self.take(rg, v);
                    con(u, acc)
                });
                rg.redexes.push((c, Tree::Swi(Box::new(con(Tree::Ref(then_name), Tree::Ref(else_name))), Box::new(args))));
                var(r)
            }
            ExprKind::Block(b) => self.block(rg, b),
            _ => unreachable!("not eligible: {:?}", e.kind),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mimic_check::SourceFile;
    use mimic_syntax::parser::parse_file;

    fn plan_of(src: &str) -> (Program, NetPlan) {
        let f = SourceFile { path: "m.mim".into(), display: "m.mim".into(), src: src.into(), ast: parse_file(src).unwrap(), package: "t".into() };
        let (p, d) = Program::build(vec![f], "t");
        assert!(d.is_empty());
        let purity = Purity::analyze(&p);
        let plan = plan(&p, &purity);
        (p, plan)
    }

    fn fid(p: &Program, name: &str) -> FnId {
        p.fns.iter().position(|f| f.name == name).unwrap()
    }

    #[test]
    fn eligibility_and_entry() {
        let (p, plan) = plan_of(
            "module main
fn main() { print(fib(3)) }
fn fib(n) {
    if n < 2 {
        return n
    }
    fib(n - 1) + fib(n - 2)
}
fn leaf(x) { x × 2 + 1 }
fn calls_leaf(x) { leaf(x) + leaf(x + 1) }
fn noisy(x) { print(x) }
fn mutates(x) {
    let mut y = x
    y = y + 1
    y
}
fn strings() { \"no\" }
",
        );
        let has = |n: &str| plan.def_index.contains_key(&fid(&p, n));
        let entered = |n: &str| plan.entered.contains(&fid(&p, n));
        assert!(has("fib") && entered("fib"), "recursive pure fn runs on the net");
        assert!(has("leaf") && !entered("leaf"), "leaf: compiled for net callers, native from native code");
        assert!(has("calls_leaf") && entered("calls_leaf"));
        assert!(!has("noisy") && !has("main"), "impure functions stay native");
        assert!(!has("mutates") && !has("strings"), "outside the net subset");
        // the book encodes, and the if became a SWI over two lazy branch defs
        mimic_net::encode(&plan.book).unwrap();
        assert!(plan.book.defs.iter().any(|d| d.name.ends_with("fib.then1")));
    }

    #[test]
    fn shared_variables_are_duplicated_and_unused_erased() {
        let (_, plan) = plan_of("module main\nfn main() { print(f(1, 2)) }\nfn f(a, b) { g(a) + g(a) }\nfn g(x) { x }\n");
        let f = plan.book.defs.iter().find(|d| d.name == "t.main.f").unwrap();
        let text = f.to_string();
        assert!(text.contains('{'), "`a` used twice → DUP: {text}");
        assert!(text.contains("(* "), "`b` unused → erased: {text}");
    }
}
