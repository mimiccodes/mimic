//! Lowering from the checked syntax tree to the IR, one function
//! specialisation at a time. Anything outside what the IR covers yet gives
//! `None`: the function then takes today's code generator.

use crate::{Function, Inst, Op, Region, Tree, Ty, Val};
use mimic_check::types::{Spec, id};
use mimic_check::{FnId, FnKind, Program, Resolved};
use mimic_syntax::ast::{Arm, BinOp, Block, Expr, ExprKind, Pattern, Stmt, UnOp};
use mimic_syntax::glyphs::Prim;

/// The first value number a lazy `let` stands for in `env` (lower ones are
/// values): `LAZY + k` is `Lower::lazies[k]`.
const LAZY: u32 = u32::MAX / 2;

/// A lazy `let`: its expression, the variables it reads, and the lazy lets
/// it reads (indices into `Lower::lazies`).
struct Lazy {
    name: String,
    ty: Ty,
    expr: *const Expr,
    uses: Vec<(String, Val, Ty)>,
    deps: Vec<usize>,
}

struct Lower<'a> {
    p: &'a Program,
    hints: &'a mut dyn crate::Hints,
    lazies: Vec<Lazy>,
    module: mimic_check::ModId,
    spec: std::rc::Rc<Spec>,
    /// The function lowered, and how many calls deep inlining is now.
    fid: FnId,
    inline_depth: u32,
    next: u32,
    /// The type of every value so far, by number.
    types: Vec<Ty>,
    /// Every name the function binds anywhere (parameters, lets, lambda
    /// and pattern names): one of them read by an embedded expression must
    /// have an IR value.
    local_names: Vec<String>,
    /// Variables in scope, innermost last.
    env: Vec<(String, Val, Ty)>,
    /// Set when an expression left to the old path would take over a
    /// variable (see [`Lower::takes_var`]): the whole function stays there.
    bail: bool,
    /// The first expression neither the IR nor an embed could take (for
    /// `MIMIC_IR=why`).
    miss: Option<String>,
}

/// The IR of `fid` specialised as `spec`, if the IR covers all of it.
/// `takes`: the parameters it takes over (owned here).
pub fn function(p: &Program, fid: FnId, spec: std::rc::Rc<Spec>, takes: &[bool], hints: &mut dyn crate::Hints) -> Option<Function> {
    let FnKind::User(decl) = &p.fns[fid].kind else { return None };
    let miss = |why: &str| {
        if crate::why() {
            eprintln!("mimic: IR: `{}` stays on the old path: {why}", p.fn_path(fid).join("::"));
        }
        None
    };
    let mut params = Vec::new();
    for (prm, t) in decl.params.iter().zip(&spec.params) {
        let Some(it) = Ty::of(p, *t) else { return miss(&format!("parameter `{}` is a {t:?}", prm.name)) };
        params.push((prm.name.clone(), it));
    }
    let Some(ret) = Ty::of(p, spec.ret) else { return miss(&format!("it returns a {:?}", spec.ret)) };
    if runs_apart(hints, &decl.body) {
        return miss("the old path runs some of its calls apart (in parallel, or as a task)");
    }
    let mut local_names: Vec<String> = decl.params.iter().map(|q| q.name.clone()).collect();
    decl.body.bound_names(&mut local_names);
    let mut l = Lower { p, hints, lazies: Vec::new(), module: p.fns[fid].module, spec: spec.clone(), fid, inline_depth: 0, next: 0, types: Vec::new(), local_names, env: Vec::new(), bail: false, miss: None };
    let mut body = Region::default();
    for (i, (name, t)) in params.iter().enumerate() {
        let op = if takes.get(i).copied().unwrap_or(false) {
            // (taken over: a value on the heap the function owns)
            if !matches!(t, Ty::List(_) | Ty::Rec(_)) {
                return miss(&format!("parameter `{name}` is taken over but not on the heap"));
            }
            Op::Take(i as u32)
        } else {
            Op::Param(i as u32)
        };
        let v = l.emit(&mut body, *t, op, decl.span);
        l.env.push((name.clone(), v, *t));
    }
    let Some(r) = l.block(&mut body, &decl.body) else {
        let at = l.miss.take().unwrap_or_else(|| "a statement it does not cover".into());
        return miss(&at);
    };
    if l.bail {
        return miss("an expression left to the old path would take over a variable");
    }
    // what the body gives must be what the function returns
    let rt = l.types[r.0 as usize];
    if ret != Ty::Unit && rt != ret {
        return miss("its body gives another type than it returns");
    }
    body.result = Some(r);
    let mut f = Function { name: p.fn_path(fid).join("::"), params, ret, body, values: l.next };
    crate::fuse::function(&mut f);
    crate::sink::function(&mut f);
    crate::own::function(&mut f);
    Some(f)
}

/// Whether the old path runs some statements of a block of `body` apart
/// (see [`crate::Hints::runs_apart`]).
fn runs_apart(hints: &mut dyn crate::Hints, body: &Block) -> bool {
    // every block: the body, those of expressions, and the `else` blocks
    // of `let … else` (statements, not expressions), at any depth
    let mut todo: Vec<&Block> = vec![body];
    let mut seen = 0;
    while seen < todo.len() {
        let b = todo[seen];
        seen += 1;
        if hints.runs_apart(&b.stmts) {
            return true;
        }
        for st in &b.stmts {
            if let Stmt::LetElse { els, .. } = st {
                todo.push(els);
            }
        }
        // the blocks directly inside this one's expressions (deeper ones
        // are reached from those)
        let mut inner: Vec<&Block> = Vec::new();
        b.walk(&mut |e| {
            if let ExprKind::Block(x) | ExprKind::Unsafe(x) | ExprKind::If { then: x, .. } | ExprKind::Arms { body: x, .. } = &e.kind {
                inner.push(unsafe { &*(x as *const Block) });
            }
        });
        for x in inner {
            if !todo.iter().any(|t| std::ptr::eq(*t, x)) {
                todo.push(x);
            }
        }
    }
    false
}

impl Lower<'_> {
    /// Whether `e` is a one-parameter lambda the checker typed `param → ret`.
    fn lambda_is(&self, e: &Expr, param: Ty, ret: Ty) -> bool {
        let ExprKind::Lambda { params, .. } = &e.kind else { return false };
        if params.len() != 1 {
            return false;
        }
        match self.spec.lambdas.get(&id(e)) {
            Some((pts, rt)) => pts.len() == 1 && Ty::of(self.p, pts[0]) == Some(param) && Ty::of(self.p, *rt) == Some(ret),
            None => false,
        }
    }

    /// The arms from the first on, matched against `v` (of type `vt`); the
    /// match gives `ty`.
    fn arms(&mut self, r: &mut Region, v: Val, vt: Ty, arms: &[Arm], ty: Ty, e: &Expr) -> Option<Val> {
        let arm = &arms[0];
        let test = self.test(r, v, vt, &arm.pat)?;
        let mark = self.env.len();
        let out = match test {
            // always matches: this arm, whatever follows
            None => {
                self.bind(r, v, vt, &arm.pat)?;
                let x = self.expr(r, &arm.body);
                self.env.truncate(mark);
                x?
            }
            Some(c) => {
                if arms.len() == 1 {
                    return None; // (nothing to fall back to)
                }
                let mut then = Region::default();
                self.bind(&mut then, v, vt, &arm.pat)?;
                let tv = self.expr(&mut then, &arm.body);
                self.env.truncate(mark);
                let tv = tv?;
                let mut els = Region::default();
                let ev = self.arms(&mut els, v, vt, &arms[1..], ty, e)?;
                if ty != Ty::Unit && (self.types[tv.0 as usize] != ty || self.types[ev.0 as usize] != ty) {
                    return None;
                }
                then.result = Some(tv);
                els.result = Some(ev);
                self.emit(r, ty, Op::If { cond: c, then, els }, e.span)
            }
        };
        Some(out)
    }

    /// Whether `v` matches `pat`: None when it always does, else a truth
    /// value computed here (no effects: parts and comparisons only).
    fn test(&mut self, r: &mut Region, v: Val, vt: Ty, pat: &Pattern) -> Option<Option<Val>> {
        let span = mimic_syntax::Span::default();
        Some(match pat {
            Pattern::Wild(_) | Pattern::Bind(..) => None,
            Pattern::As(inner, ..) => self.test(r, v, vt, inner)?,
            Pattern::Lit(x) => {
                let k = self.literal(r, x, vt)?;
                Some(self.emit(r, Ty::Bool, Op::Bin(BinOp::Eq, v, k), span))
            }
            Pattern::Range { lo, hi, .. } => {
                if !matches!(vt, Ty::I64 | Ty::F64) {
                    return None;
                }
                let a = self.literal(r, lo, vt)?;
                let b = self.literal(r, hi, vt)?;
                let x = self.emit(r, Ty::Bool, Op::Bin(BinOp::Le, a, v), span);
                let y = self.emit(r, Ty::Bool, Op::Bin(BinOp::Lt, v, b), span);
                Some(self.emit(r, Ty::Bool, Op::Bin(BinOp::And, x, y), span))
            }
            Pattern::Or(alts, _) => {
                let mut names = Vec::new();
                pat.bound_names(&mut names);
                if !names.is_empty() {
                    return None;
                }
                let mut acc: Option<Val> = None;
                for a in alts {
                    let Some(t) = self.test(r, v, vt, a)? else { return Some(None) };
                    acc = Some(match acc {
                        None => t,
                        Some(c) => self.emit(r, Ty::Bool, Op::Bin(BinOp::Or, c, t), span),
                    });
                }
                acc
            }
            Pattern::Tuple(ps, _) => {
                let Ty::Agg(t @ mimic_check::types::Ty::Tup(_)) = vt else { return None };
                let parts = crate::parts(self.p, t)?;
                if parts.len() != ps.len() {
                    return None;
                }
                self.all_parts(r, v, &parts, ps.iter().enumerate().collect())?
            }
            Pattern::Record { path, fields, .. } => {
                let Ty::Agg(t @ mimic_check::types::Ty::Obj(id)) = vt else { return None };
                let tid = mimic_check::types::obj_tid(id);
                // the pattern names this very record type
                if self.p.lookup(self.module, path) != Some(Resolved::Type(tid)) {
                    return None;
                }
                let parts = crate::parts(self.p, t)?;
                let mut which = Vec::new();
                for (name, sub) in fields {
                    which.push((self.p.types[tid].field_index(name)?, sub));
                }
                self.all_parts(r, v, &parts, which)?
            }
            _ => return None,
        })
    }

    /// The parts `which` (index, pattern) of `v` all match: None if they
    /// always do.
    fn all_parts(&mut self, r: &mut Region, v: Val, parts: &[mimic_check::types::Ty], which: Vec<(usize, &Pattern)>) -> Option<Option<Val>> {
        let span = mimic_syntax::Span::default();
        let mut acc: Option<Val> = None;
        for (k, sub) in which {
            let pt = Ty::of(self.p, *parts.get(k)?)?;
            // (a part read only when its pattern tests something)
            let mut probe = Region::default();
            let tmp = Val(self.next);
            if self.test(&mut probe, tmp, pt, sub)?.is_none() && probe.insts.is_empty() {
                continue;
            }
            let x = self.emit(r, pt, Op::Get(v, k as u32), span);
            if let Some(t) = self.test(r, x, pt, sub)? {
                acc = Some(match acc {
                    None => t,
                    Some(c) => self.emit(r, Ty::Bool, Op::Bin(BinOp::And, c, t), span),
                });
            }
        }
        Some(acc)
    }

    /// Bind the names in `pat` (matched by `v`) for the arm's body: each to
    /// its part, of the type the checker gave the name.
    fn bind(&mut self, r: &mut Region, v: Val, vt: Ty, pat: &Pattern) -> Option<()> {
        let span = mimic_syntax::Span::default();
        match pat {
            Pattern::Wild(_) | Pattern::Lit(_) | Pattern::Range { .. } | Pattern::Or(..) => {}
            Pattern::Bind(name, _) => {
                if self.spec.binds.get(&id(pat)).and_then(|t| Ty::of(self.p, *t)) != Some(vt) {
                    return None;
                }
                self.env.push((name.clone(), v, vt));
            }
            Pattern::As(inner, name, _) => {
                self.bind(r, v, vt, inner)?;
                self.env.push((name.clone(), v, vt));
            }
            Pattern::Tuple(ps, _) => {
                let Ty::Agg(t) = vt else { return None };
                let parts = crate::parts(self.p, t)?;
                for (k, sub) in ps.iter().enumerate() {
                    let mut names = Vec::new();
                    sub.bound_names(&mut names);
                    if names.is_empty() {
                        continue;
                    }
                    let pt = Ty::of(self.p, *parts.get(k)?)?;
                    let x = self.emit(r, pt, Op::Get(v, k as u32), span);
                    self.bind(r, x, pt, sub)?;
                }
            }
            Pattern::Record { fields, .. } => {
                let Ty::Agg(t @ mimic_check::types::Ty::Obj(id)) = vt else { return None };
                let tid = mimic_check::types::obj_tid(id);
                let parts = crate::parts(self.p, t)?;
                for (name, sub) in fields {
                    let mut names = Vec::new();
                    sub.bound_names(&mut names);
                    if names.is_empty() {
                        continue;
                    }
                    let k = self.p.types[tid].field_index(name)?;
                    let pt = Ty::of(self.p, *parts.get(k)?)?;
                    let x = self.emit(r, pt, Op::Get(v, k as u32), span);
                    self.bind(r, x, pt, sub)?;
                }
            }
            _ => return None,
        }
        Some(())
    }

    /// A literal of a pattern (a number, maybe negated, or a truth value)
    /// as a value of type `t`.
    fn literal(&mut self, r: &mut Region, x: &Expr, t: Ty) -> Option<Val> {
        let span = x.span;
        let (neg, inner) = match &x.kind {
            ExprKind::Unary { op: UnOp::Neg, expr } => (true, &**expr),
            _ => (false, x),
        };
        let op = match (&inner.kind, t) {
            (ExprKind::Int(n), Ty::I64) => Op::Int(if neg { n.checked_neg()? } else { *n }),
            (ExprKind::Int(n), Ty::F64) => Op::Float(if neg { -(*n as f64) } else { *n as f64 }),
            (ExprKind::Float(f), Ty::F64) => Op::Float(if neg { -f } else { *f }),
            (ExprKind::Bool(b), Ty::Bool) if !neg => Op::Bool(*b),
            _ => return None,
        };
        Some(self.emit(r, t, op, span))
    }

    /// A one-parameter lambda's body as a region, its parameter bound to `v`.
    fn lambda_region(&mut self, e: &Expr, v: Val, t: Ty) -> Option<Region> {
        let ExprKind::Lambda { params, body } = &e.kind else { return None };
        let mut reg = Region::default();
        self.env.push((params[0].clone(), v, t));
        let out = self.expr(&mut reg, body);
        self.env.pop();
        reg.result = Some(out?);
        Some(reg)
    }
}

/// A single number or truth value (not a value with parts).
fn scalar(t: Ty) -> bool {
    matches!(t, Ty::I64 | Ty::F64 | Ty::Bool)
}

impl Lower<'_> {
    fn emit(&mut self, r: &mut Region, ty: Ty, op: Op, span: mimic_syntax::Span) -> Val {
        let dst = Val(self.next);
        self.next += 1;
        self.types.push(ty);
        r.insts.push(Inst { dst, ty, op, span });
        dst
    }

    /// The IR type of an expression, from the checker.
    fn ty(&self, e: &Expr) -> Option<Ty> {
        Ty::of(self.p, *self.spec.exprs.get(&id(e))?)
    }

    fn block(&mut self, r: &mut Region, b: &Block) -> Option<Val> {
        let mark = self.env.len();
        for (i, s) in b.stmts.iter().enumerate() {
            match s {
                // a list used only inside array expressions: never built,
                // fused where it is read (by the embedded expressions)
                Stmt::Let { name, mutable: false, ty: None, value, .. } if self.lazy(name, value, &b.stmts[i + 1..], b.tail.as_deref()) => {}
                Stmt::Let { name, mutable: false, ty, value, .. } => {
                    let v = self.expr(r, value)?;
                    let vt = self.ty(value)?;
                    // a declared type that converts the value: not covered yet
                    if ty.is_some() && self.spec.lets.get(&id(s)).and_then(|t| Ty::of(self.p, *t)) != Some(vt) {
                        return None;
                    }
                    self.env.push((name.clone(), v, vt));
                }
                Stmt::Expr(e) => {
                    self.expr(r, e)?;
                }
                // `let (a, b) = t` of a tuple kept in registers: its parts
                Stmt::LetTuple { names, value, .. } => {
                    let v = self.expr(r, value)?;
                    let Ty::Agg(t) = self.types[v.0 as usize] else { return None };
                    let ps = crate::parts(self.p, t)?;
                    if names.len() > ps.len() {
                        return None;
                    }
                    for (k, n) in names.iter().enumerate() {
                        if n == "_" {
                            continue;
                        }
                        let pt = Ty::of(self.p, ps[k])?;
                        // the name's own type is the part's (no conversion yet)
                        if self.spec.tuple_lets.get(&(id(s), k)).and_then(|t| Ty::of(self.p, *t)) != Some(pt) {
                            return None;
                        }
                        let x = self.emit(r, pt, Op::Get(v, k as u32), value.span);
                        self.env.push((n.clone(), x, pt));
                    }
                }
                _ => return None,
            }
        }
        let v = match &b.tail {
            Some(t) => self.expr(r, t)?,
            None => self.emit(r, Ty::Unit, Op::Unit, b.span),
        };
        self.env.truncate(mark);
        Some(v)
    }

    /// `e` in the IR: its own operations where the IR covers it, else (a
    /// map, a reduction, a call of the runtime …) embedded whole, compiled
    /// by the old path where it stands. What a failed lowering emitted is
    /// taken back first, so nothing runs twice.
    fn expr(&mut self, r: &mut Region, e: &Expr) -> Option<Val> {
        let (mark, env_mark) = (r.insts.len(), self.env.len());
        if let Some(v) = self.lower(r, e) {
            return Some(v);
        }
        r.insts.truncate(mark);
        self.env.truncate(env_mark);
        self.embed(r, e)
    }

    /// Whether `let name = value` is lazy (see [`crate::Hints::lazy`]); if
    /// so it is recorded, in scope from here.
    fn lazy(&mut self, name: &str, value: &Expr, rest: &[Stmt], tail: Option<&Expr>) -> bool {
        let scope: Vec<(String, Ty)> = self.env.iter().filter(|(_, v, _)| v.0 < LAZY).map(|(n, _, t)| (n.clone(), *t)).collect();
        let lazies: Vec<(String, *const Expr)> =
            self.env.iter().filter(|(_, v, _)| v.0 >= LAZY).map(|(n, v, _)| (n.clone(), self.lazies[(v.0 - LAZY) as usize].expr)).collect();
        if !self.hints.lazy(name, value, rest, tail, &scope, &lazies) {
            return false;
        }
        // what it reads, as the variables are now
        let mut uses: Vec<(String, Val, Ty)> = Vec::new();
        let mut deps = Vec::new();
        let mut ok = true;
        value.walk(&mut |x| {
            if let ExprKind::Path(p) = &x.kind
                && p.len() == 1
            {
                match self.env.iter().rev().find(|(n, ..)| *n == p[0]) {
                    Some((_, v, _)) if v.0 >= LAZY => deps.push((v.0 - LAZY) as usize),
                    Some((n, v, t)) => {
                        if !uses.iter().any(|(u, ..)| u == n) {
                            uses.push((n.clone(), *v, *t));
                        }
                    }
                    None if self.local_names.contains(&p[0]) => ok = false,
                    None => {}
                }
            }
            // (a lambda's own parameters are not read from outside: the
            // old path declines a lazy let holding one anyway)
            if matches!(x.kind, ExprKind::Lambda { .. }) {
                ok = false;
            }
        });
        if !ok {
            return false;
        }
        let k = self.lazies.len() as u32;
        let t = self.ty(value).unwrap_or(Ty::Unit);
        self.lazies.push(Lazy { name: name.to_string(), ty: t, expr: value as *const Expr, uses, deps });
        self.env.push((name.to_string(), Val(LAZY + k), t));
        true
    }

    /// A call of a user function.
    fn call(&mut self, r: &mut Region, e: &Expr, ty: Ty) -> Option<Val> {
        let ExprKind::Call { callee, args } = &e.kind else { return None };
        let ExprKind::Path(path) = &callee.kind else { return None };
        // a variable holding a function: not covered yet
        if path.len() == 1 && self.env.iter().any(|(n, ..)| *n == path[0]) {
            return None;
        }
        let Some(Resolved::Fn(f)) = self.p.lookup(self.module, path) else { return None };
        // (a call's specialisation is looked up in the caller's types by
        // the code generator: none inside an inlined body)
        if !matches!(self.p.fns[f].kind, FnKind::User(_)) || !self.spec.calls.contains_key(&id(e)) || self.inline_depth > 0 {
            return None;
        }
        let mut vals = Vec::new();
        for a in args {
            self.ty(a)?;
            vals.push(self.expr(r, a)?);
        }
        let key = self.spec.calls.get(&id(e))?.clone();
        let mut takes = self.hints.takes(&key);
        takes.resize(vals.len(), false);
        let moves = vec![false; vals.len()];
        Some(self.emit(r, ty, Op::Call { site: id(e), callee: f, args: vals, takes, moves }, e.span))
    }

    /// The callee of `e`, when it is a call worth lowering in place: a user
    /// function (not this one, at most 4 calls deep) whose body is one
    /// expression, and whose specialisation the code generator knows.
    fn inlinable(&mut self, e: &Expr) -> Option<(FnId, std::rc::Rc<Spec>)> {
        let ExprKind::Call { callee, args } = &e.kind else { return None };
        let ExprKind::Path(path) = &callee.kind else { return None };
        if self.inline_depth >= 4 || (path.len() == 1 && self.env.iter().any(|(n, ..)| *n == path[0])) {
            return None;
        }
        let Some(Resolved::Fn(f)) = self.p.lookup(self.module, path) else { return None };
        let FnKind::User(d) = &self.p.fns[f].kind else { return None };
        if f == self.fid || !d.body.stmts.is_empty() || d.body.tail.is_none() || d.params.len() != args.len() {
            return None;
        }
        let key = self.spec.calls.get(&id(e))?.clone();
        let cs = self.hints.spec(&key)?;
        Some((f, cs))
    }

    /// The body of the helper `e` calls as a tree, its parameters the
    /// arguments (each evaluated once, here, as the call would).
    fn inline_tree(&mut self, r: &mut Region, e: &Expr) -> Option<Tree> {
        let (f, cs) = self.inlinable(e)?;
        let ExprKind::Call { args, .. } = &e.kind else { return None };
        let FnKind::User(d) = &self.p.fns[f].kind else { return None };
        let mut binds = Vec::new();
        for (k, (prm, a)) in d.params.iter().zip(args).enumerate() {
            let t = self.ty(a)?;
            // the callee specialised on exactly these types
            if Ty::of(self.p, *cs.params.get(k)?) != Some(t) {
                return None;
            }
            binds.push((prm.name.clone(), self.expr(r, a)?, t));
        }
        let mut names: Vec<String> = d.params.iter().map(|q| q.name.clone()).collect();
        d.body.bound_names(&mut names);
        let saved = (
            std::mem::replace(&mut self.spec, cs),
            std::mem::replace(&mut self.module, self.p.fns[f].module),
            std::mem::replace(&mut self.env, binds),
            std::mem::replace(&mut self.local_names, names),
        );
        self.inline_depth += 1;
        let body_mark = r.insts.len();
        let t = d.body.tail.as_deref().and_then(|tail| self.tree(r, tail));
        self.inline_depth -= 1;
        // what the body computes once is at the call, for debug information
        // (the callee's places are in its own file)
        fn at(r: &mut Region, span: mimic_syntax::Span) {
            for i in &mut r.insts {
                i.span = span;
                match &mut i.op {
                    Op::If { then, els, .. } => {
                        at(then, span);
                        at(els, span);
                    }
                    Op::Loop { done, step, .. } => {
                        if let Some(d) = done {
                            at(d, span);
                        }
                        at(step, span);
                    }
                    _ => {}
                }
            }
        }
        let mut body = Region { insts: r.insts.split_off(body_mark), result: None };
        at(&mut body, e.span);
        r.insts.append(&mut body.insts);
        (self.spec, self.module, self.env, self.local_names) = saved;
        t
    }

    /// `e`, an operand of an element-by-element computation: a number (the
    /// same for every element), or a list, computed element by element
    /// where it is `⍳`, an operator or a list the IR holds.
    fn tree(&mut self, r: &mut Region, e: &Expr) -> Option<Tree> {
        let ty = self.ty(e)?;
        if matches!(ty, Ty::I64 | Ty::F64) {
            return Some(match &e.kind {
                // (written in the program: a constant inside the loop)
                ExprKind::Int(n) if ty == Ty::I64 => Tree::Int(*n),
                ExprKind::Float(x) if ty == Ty::F64 => Tree::Float(*x),
                _ => Tree::Scalar(self.expr(r, e)?),
            });
        }
        if !elementwise_list(ty) {
            return None;
        }
        let Ty::List(mimic_check::types::Ty::Arr(el, _)) = ty else { return None };
        match &e.kind {
            ExprKind::Prim { prim: Prim::Iota, args } if args.len() == 1 && el == mimic_check::types::El::I64 && self.ty(&args[0])? == Ty::I64 => {
                Some(Tree::Iota(self.expr(r, &args[0])?))
            }
            ExprKind::Binary { op, lhs, rhs } if !matches!(op, BinOp::And | BinOp::Or) => {
                let a = self.tree(r, lhs)?;
                let b = self.tree(r, rhs)?;
                Some(Tree::Bin(*op, Box::new(a), Box::new(b)))
            }
            ExprKind::Unary { op, expr } => Some(Tree::Un(*op, Box::new(self.tree(r, expr)?))),
            // a small helper's body, inlined (else the call's list, below)
            ExprKind::Call { .. } if self.inlinable(e).is_some() => {
                let (mark, env_mark) = (r.insts.len(), self.env.len());
                match self.inline_tree(r, e) {
                    Some(t) => Some(t),
                    None => {
                        r.insts.truncate(mark);
                        self.env.truncate(env_mark);
                        Some(Tree::Leaf(self.direct(r, e)?, el))
                    }
                }
            }
            // `xs ¨ f` the old path computes element by element (a pure
            // `f` on numbers): built by its engine inside this loop
            ExprKind::Prim { prim: Prim::Each, .. } if self.inline_depth == 0 => {
                let scope: Vec<(String, Ty)> = self.env.iter().filter(|(_, v, _)| v.0 < LAZY).map(|(n, _, t)| (n.clone(), *t)).collect();
                let lazies: Vec<(String, *const Expr)> =
                    self.env.iter().filter(|(_, v, _)| v.0 >= LAZY).map(|(n, v, _)| (n.clone(), self.lazies[(v.0 - LAZY) as usize].expr)).collect();
                if !self.hints.fusible(e, &scope, &lazies) {
                    return None;
                }
                let (uses, lazies) = self.reads(e)?;
                Some(Tree::Ast { expr: e as *const Expr, uses, lazies })
            }
            // a lazy let: its computation here (read with the variables
            // as they were where it was bound), as the old path does where
            // it fuses one
            ExprKind::Path(p) if p.len() == 1 && self.env.iter().rev().find(|(n, ..)| *n == p[0]).is_some_and(|(_, v, _)| v.0 >= LAZY) => {
                let k = self.env.iter().rev().find(|(n, ..)| *n == p[0]).map(|(_, v, _)| (v.0 - LAZY) as usize)?;
                let z = &self.lazies[k];
                let (expr, uses, deps) = (z.expr, z.uses.clone(), z.deps.clone());
                let mark = self.env.len();
                self.env.extend(uses);
                for d in deps {
                    let t = self.lazies[d].ty;
                    self.env.push((self.lazies[d].name.clone(), Val(LAZY + d as u32), t));
                }
                // SAFETY: the syntax tree outlives the lowering
                let t = self.tree(r, unsafe { &*expr });
                self.env.truncate(mark);
                t
            }
            // a computation this tree cannot hold: not a list to read
            // either (lowering it as a value would come back here)
            ExprKind::Prim { prim: Prim::Iota | Prim::Each, .. } | ExprKind::Binary { .. } => None,
            _ => Some(Tree::Leaf(self.direct(r, e)?, el)),
        }
    }

    /// `e` in the IR's own operations, never embedded (what a failed
    /// lowering emitted taken back).
    fn direct(&mut self, r: &mut Region, e: &Expr) -> Option<Val> {
        let (mark, env_mark) = (r.insts.len(), self.env.len());
        let v = self.lower(r, e);
        if v.is_none() {
            r.insts.truncate(mark);
            self.env.truncate(env_mark);
        }
        v
    }

    /// `e` kept whole for the old path, reading these variables.
    fn embed(&mut self, r: &mut Region, e: &Expr) -> Option<Val> {
        let v = self.embed_here(r, e);
        if v.is_none() && self.miss.is_none() && self.inline_depth == 0 {
            let what = match self.ty(e) {
                None => format!("{:?}", self.spec.exprs.get(&id(e))),
                Some(t) => format!("{t:?}"),
            };
            self.miss = Some(format!("line {}:{}: an expression of type {what} it cannot take or embed", e.span.line, e.span.col));
        }
        v
    }

    fn embed_here(&mut self, r: &mut Region, e: &Expr) -> Option<Val> {
        // (inside an inlined body the code generator's types are the
        // caller's: the old path could not compile the callee's syntax)
        if self.inline_depth > 0 {
            return None;
        }
        let ty = self.ty(e)?;
        // (a lambda on its own is a function value: not an IR type anyway)
        if matches!(e.kind, ExprKind::Lambda { .. }) {
            return None;
        }
        if self.takes_var(e) {
            self.bail = true;
            return None;
        }
        let (uses, lazies) = self.reads(e)?;
        Some(self.emit(r, ty, Op::Embed { expr: e as *const Expr, uses, lazies }, e.span))
    }

    /// `put(xs, i, x)` or `push(xs, x)` (the prelude's) of a list of i64
    /// and numbers, giving a list of i64: its name and arguments.
    fn list_change<'e>(&self, e: &'e Expr) -> Option<(&'static str, &'e [Expr])> {
        let ExprKind::Call { callee, args } = &e.kind else { return None };
        let ExprKind::Path(path) = &callee.kind else { return None };
        if path.len() == 1 && self.env.iter().any(|(n, ..)| *n == path[0]) {
            return None;
        }
        let Some(Resolved::Fn(f)) = self.p.lookup(self.module, path) else { return None };
        let FnKind::Extern { decl, .. } = &self.p.fns[f].kind else { return None };
        if !decl.intrinsic || self.p.mods[self.p.fns[f].module].name != "prelude" {
            return None;
        }
        let op = match (decl.name.as_str(), args.len()) {
            ("put", 3) => "put",
            ("push", 2) => "push",
            _ => return None,
        };
        let list = Ty::List(mimic_check::types::Ty::Arr(mimic_check::types::El::I64, 0));
        if self.ty(&args[0])? != list || self.ty(e)? != list || args[1..].iter().any(|a| self.ty(a) != Some(Ty::I64)) {
            return None;
        }
        Some((op, args))
    }

    /// `x`, the new value of field `n` of the record `base` names, as
    /// changes of that field's old value (`push(put(s.v, i, x), y)`): each
    /// (name, the other arguments), innermost first.
    fn change_chain<'e>(&self, base: &[String], n: &str, x: &'e Expr) -> Option<Vec<(&'static str, &'e [Expr])>> {
        let mut chain = Vec::new();
        let mut cur = x;
        loop {
            let (op, args) = self.list_change(cur)?;
            chain.push((op, &args[1..]));
            match &args[0].kind {
                ExprKind::Field { base: fb, name } if name == n && matches!(&fb.kind, ExprKind::Path(q) if q == base) => break,
                _ => cur = &args[0],
            }
        }
        chain.reverse();
        Some(chain)
    }

    /// Whether the old path, compiling `e`, would take over a variable on
    /// the heap (and so change it in place where it is held once): the base
    /// of `{ x with … }`, an argument a user function takes over. (Not the
    /// list of `push`/`put`: the old path copies it anyway.) Embedded, it would only borrow it (the IR
    /// owns its values) and copy instead.
    fn takes_var(&mut self, e: &Expr) -> bool {
        let heap_var = |l: &Self, x: &Expr| matches!(&x.kind, ExprKind::Path(q) if q.len() == 1 && l.env.iter().rev().find(|(n, ..)| *n == q[0]).is_some_and(|(_, _, t)| matches!(t, Ty::List(_) | Ty::Rec(_))));
        let mut found = false;
        // (each call passing one: its node, its callee, which arguments are such variables)
        let mut calls: Vec<(usize, Vec<String>, Vec<bool>)> = Vec::new();
        e.walk(&mut |x| match &x.kind {
            ExprKind::With { base, .. } if heap_var(self, base) => found = true,
            ExprKind::Call { callee, args } if args.iter().any(|a| heap_var(self, a)) => {
                if let ExprKind::Path(path) = &callee.kind {
                    calls.push((id(x), path.clone(), args.iter().map(|a| heap_var(self, a)).collect()));
                }
            }
            _ => {}
        });
        if found {
            return true;
        }
        for (site, path, heap) in calls {
            let Some(Resolved::Fn(f)) = self.p.lookup(self.module, &path) else { continue };
            if let FnKind::User(_) = &self.p.fns[f].kind
                && let Some(key) = self.spec.calls.get(&site).cloned()
                && heap.iter().zip(self.hints.takes(&key)).any(|(h, t)| *h && t)
            {
                return true;
            }
        }
        false
    }

    /// The variables `e` reads, as the IR's values, and the lazy lets it
    /// reads (with those they read, in the order they were bound, and the
    /// variables they read): what the old path needs to compile `e` where
    /// it stands. None when it cannot be compiled from here.
    #[allow(clippy::type_complexity)]
    fn reads(&self, e: &Expr) -> Option<(Vec<(String, Val, Ty)>, Vec<(String, *const Expr)>)> {
        // the variables it reads: the IR's values for them (names it binds
        // itself are its own)
        let mut inside = Vec::new();
        Block { stmts: vec![], tail: Some(Box::new(e.clone())), span: e.span }.bound_names(&mut inside);
        let mut uses: Vec<(String, Val, Ty)> = Vec::new();
        let mut lazy_ks: Vec<usize> = Vec::new();
        let mut unknown = false;
        e.walk(&mut |x| {
            if let ExprKind::Path(p) = &x.kind
                && p.len() == 1
                && !inside.contains(&p[0])
                && !uses.iter().any(|(n, ..)| *n == p[0])
            {
                match self.env.iter().rev().find(|(n, ..)| *n == p[0]) {
                    Some((_, v, _)) if v.0 >= LAZY => lazy_ks.push((v.0 - LAZY) as usize),
                    Some((n, v, t)) => uses.push((n.clone(), *v, *t)),
                    // a name the function binds but the IR has no value
                    // for: cannot be read from here
                    None if self.local_names.contains(&p[0]) => unknown = true,
                    None => {}
                }
            }
            // an assignment inside changes a variable the IR holds as a value
            if let ExprKind::Block(b) | ExprKind::Unsafe(b) | ExprKind::If { then: b, .. } = &x.kind
                && b.stmts.iter().any(|s| matches!(s, Stmt::Assign { .. }))
            {
                unknown = true;
            }
        });
        if unknown {
            return None;
        }
        // the lazy lets it reads, and those they read (in the order they
        // were bound), with the variables they read
        let mut all: Vec<usize> = Vec::new();
        while let Some(k) = lazy_ks.pop() {
            if !all.contains(&k) {
                all.push(k);
                lazy_ks.extend(self.lazies[k].deps.iter().copied());
            }
        }
        all.sort_unstable();
        let mut lazies = Vec::new();
        for k in all {
            let z = &self.lazies[k];
            for (n, v, t) in &z.uses {
                match uses.iter().find(|(u, ..)| u == n) {
                    // the same name for another value (rebound since): not here
                    Some((_, w, _)) if w != v => return None,
                    Some(_) => {}
                    None => uses.push((n.clone(), *v, *t)),
                }
            }
            lazies.push((z.name.clone(), z.expr));
        }
        Some((uses, lazies))
    }

    fn lower(&mut self, r: &mut Region, e: &Expr) -> Option<Val> {
        let ty = self.ty(e)?;
        match &e.kind {
            ExprKind::Int(n) if ty == Ty::I64 => Some(self.emit(r, ty, Op::Int(*n), e.span)),
            ExprKind::Float(x) if ty == Ty::F64 => Some(self.emit(r, ty, Op::Float(*x), e.span)),
            ExprKind::Bool(b) if ty == Ty::Bool => Some(self.emit(r, ty, Op::Bool(*b), e.span)),
            ExprKind::Path(path) if path.len() == 1 => {
                let (_, v, t) = self.env.iter().rev().find(|(n, ..)| *n == path[0])?.clone();
                // (a lazy let has no value: what reads it is embedded)
                (t == ty && v.0 < LAZY).then_some(v)
            }
            ExprKind::Block(b) => self.block(r, b),
            ExprKind::Binary { op: op @ (BinOp::And | BinOp::Or), lhs, rhs } => {
                if ty != Ty::Bool || self.ty(lhs)? != Ty::Bool || self.ty(rhs)? != Ty::Bool {
                    return None;
                }
                // the right side only when the left does not decide
                let a = self.expr(r, lhs)?;
                let mut more = Region::default();
                let b = self.expr(&mut more, rhs)?;
                more.result = Some(b);
                let mut decided = Region::default();
                let c = self.emit(&mut decided, Ty::Bool, Op::Bool(*op == BinOp::Or), e.span);
                decided.result = Some(c);
                let (then, els) = if *op == BinOp::And { (more, decided) } else { (decided, more) };
                Some(self.emit(r, Ty::Bool, Op::If { cond: a, then, els }, e.span))
            }
            // a list computed element by element (`⍳ n % k × 2`, `xs + ys`,
            // `-xs`, `xs ¨ f`): one map, the whole expression one loop
            ExprKind::Prim { prim: Prim::Iota | Prim::Each, .. } | ExprKind::Binary { .. } | ExprKind::Unary { .. } if elementwise_list(ty) => {
                let t = self.tree(r, e)?;
                if matches!(t, Tree::Leaf(..)) {
                    return None;
                }
                Some(self.emit(r, ty, Op::Map(t), e.span))
            }
            // a small helper giving a list element by element: its body
            // here, a map the fusion pass can move into what reads it
            ExprKind::Call { .. } if elementwise_list(ty) && self.inlinable(e).is_some() => {
                let (mark, env_mark) = (r.insts.len(), self.env.len());
                match self.inline_tree(r, e) {
                    Some(t) if !matches!(t, Tree::Leaf(..)) => Some(self.emit(r, ty, Op::Map(t), e.span)),
                    // (not a computation after all: the call as it is)
                    _ => {
                        r.insts.truncate(mark);
                        self.env.truncate(env_mark);
                        self.call(r, e, ty)
                    }
                }
            }
            // (operators on single values: on lists, above)
            ExprKind::Binary { op, lhs, rhs } => {
                if !scalar(ty) || !scalar(self.ty(lhs)?) || !scalar(self.ty(rhs)?) {
                    return None;
                }
                let a = self.expr(r, lhs)?;
                let b = self.expr(r, rhs)?;
                Some(self.emit(r, ty, Op::Bin(*op, a, b), e.span))
            }
            ExprKind::Unary { op, expr } => {
                if !scalar(ty) || !scalar(self.ty(expr)?) {
                    return None;
                }
                let a = self.expr(r, expr)?;
                Some(self.emit(r, ty, Op::Un(*op, a), e.span))
            }
            // a record on the heap: its fields' values, each of the type its
            // field declares, in declaration order
            ExprKind::Record { fields, .. } if matches!(ty, Ty::Rec(_)) => {
                let Ty::Rec(mimic_check::types::Ty::Obj(id)) = ty else { return None };
                let tid = id as usize;
                let td = &self.p.types[tid];
                let mimic_syntax::ast::TypeKind::Record(fs) = &td.decl.kind else { return None };
                let mut vals: Vec<Option<Val>> = vec![None; fs.len()];
                for (n, x, _) in fields {
                    let i = fs.iter().position(|(m, _, _)| m == n)?;
                    let want = Ty::of(self.p, mimic_check::types::ann_in(self.p, td.module, &fs[i].1)?)?;
                    let v = self.expr(r, x)?;
                    if self.types[v.0 as usize] != want {
                        return None;
                    }
                    vals[i] = Some(v);
                }
                let fields: Vec<Val> = vals.into_iter().collect::<Option<_>>()?;
                Some(self.emit(r, ty, Op::Record { tid, moves: vec![false; fields.len()], fields }, e.span))
            }
            // a field of a record on the heap, of the type it declares
            ExprKind::Field { base, name } if matches!(self.ty(base)?, Ty::Rec(_)) => {
                let Ty::Rec(mimic_check::types::Ty::Obj(id)) = self.ty(base)? else { return None };
                let td = &self.p.types[id as usize];
                let mimic_syntax::ast::TypeKind::Record(fs) = &td.decl.kind else { return None };
                let k = fs.iter().position(|(m, _, _)| m == name)?;
                if Ty::of(self.p, mimic_check::types::ann_in(self.p, td.module, &fs[k].1)?)? != ty {
                    return None;
                }
                let b = self.expr(r, base)?;
                Some(self.emit(r, ty, Op::Field { base: b, tid: id as usize, k: k as u32, borrow: false }, e.span))
            }
            // `{ r with f: x, … }` of a record on the heap, each new value of
            // the type its field declares. A list of i64 changed from its own
            // old value by `put`/`push` (`{ s with v: put(s.v, i, x) }`) is a
            // change: taken out of the record and changed in place. Any other
            // field value changing a field of the base: the old path's.
            ExprKind::With { base, fields } if matches!(ty, Ty::Rec(_)) && self.ty(base)? == ty => {
                let Ty::Rec(mimic_check::types::Ty::Obj(id)) = ty else { return None };
                let tid = id as usize;
                let td = &self.p.types[tid];
                let mimic_syntax::ast::TypeKind::Record(fs) = &td.decl.kind else { return None };
                let bp = match &base.kind {
                    ExprKind::Path(bp) => Some(bp.clone()),
                    _ => None,
                };
                // the new values first (they may read the base), then the base
                let mut news = Vec::new();
                let mut changes = Vec::new();
                for (n, x, _) in fields {
                    let k = fs.iter().position(|(m, _, _)| m == n)?;
                    if let Some(bp) = &bp
                        && let Some(chain) = self.change_chain(bp, n, x)
                    {
                        let mut cs = Vec::new();
                        for (op, rest) in chain {
                            let vs = rest.iter().map(|a| self.expr(r, a)).collect::<Option<Vec<_>>>()?;
                            cs.push(if op == "put" { crate::Change::Put(vs[0], vs[1]) } else { crate::Change::Push(vs[0]) });
                        }
                        changes.push((k as u32, cs));
                        continue;
                    }
                    if let Some(bp) = &bp {
                        let mut changes_field = false;
                        x.walk(&mut |y| {
                            if let ExprKind::Call { args, .. } = &y.kind
                                && let Some(ExprKind::Field { base: fb, .. }) = args.first().map(|a| &a.kind)
                                && matches!(&fb.kind, ExprKind::Path(q) if q == bp)
                            {
                                changes_field = true;
                            }
                        });
                        if changes_field {
                            return None;
                        }
                    }
                    let want = Ty::of(self.p, mimic_check::types::ann_in(self.p, td.module, &fs[k].1)?)?;
                    let v = self.expr(r, x)?;
                    if self.types[v.0 as usize] != want {
                        return None;
                    }
                    news.push((k as u32, v));
                }
                let b = self.expr(r, base)?;
                Some(self.emit(r, ty, Op::With { base: b, tid, moves: vec![false; news.len()], fields: news, moved: false, changes }, e.span))
            }
            // ≢ xs of a list (an array expression the old path computes
            // fused, without building it: embedded whole)
            ExprKind::Prim { prim: Prim::Len, args } if args.len() == 1 && ty == Ty::I64 && matches!(self.ty(&args[0])?, Ty::List(_)) => {
                let v = self.direct(r, &args[0])?;
                Some(self.emit(r, ty, Op::Len(v), e.span))
            }
            // +/ ×/ ⌈/ ⌊/ of a list of numbers the IR holds or computes
            // element by element: one loop (of anything else, such as a
            // map with a function: embedded whole, fused by the old path)
            ExprKind::Prim { prim: p @ (Prim::Sum | Prim::Product | Prim::Maximum | Prim::Minimum), args } if args.len() == 1 => {
                let Ty::List(mimic_check::types::Ty::Arr(el @ (mimic_check::types::El::I64 | mimic_check::types::El::F64), _)) = self.ty(&args[0])? else { return None };
                let want = if el == mimic_check::types::El::F64 { Ty::F64 } else { Ty::I64 };
                if ty != want {
                    return None;
                }
                let t = self.tree(r, &args[0])?;
                Some(self.emit(r, ty, Op::Reduce(*p, t, el), e.span))
            }
            // xs[i] of a list: a number of its kind
            ExprKind::Index { base, index } if matches!(self.ty(base)?, Ty::List(_)) => {
                let Ty::List(mimic_check::types::Ty::Arr(el, _)) = self.ty(base)? else { return None };
                let want = match el {
                    mimic_check::types::El::F64 => Ty::F64,
                    _ => Ty::I64,
                };
                if ty != want || self.ty(index)? != Ty::I64 {
                    return None;
                }
                let b = self.expr(r, base)?;
                let i = self.expr(r, index)?;
                Some(self.emit(r, ty, Op::Index(b, i, el), e.span))
            }
            // put/push of a list of i64: in place where the IR owns the list
            // at its last use (own), else on a copy
            ExprKind::Call { .. } if self.list_change(e).is_some() => {
                let (op, args) = self.list_change(e)?;
                let list = self.expr(r, &args[0])?;
                let a = self.expr(r, &args[1])?;
                let o = if op == "put" {
                    let x = self.expr(r, &args[2])?;
                    Op::Put { list, index: a, value: x, moved: false }
                } else {
                    Op::Push { list, value: a, moved: false }
                };
                Some(self.emit(r, ty, o, e.span))
            }
            ExprKind::Call { .. } => self.call(r, e, ty),
            ExprKind::If { cond, then, els } => {
                if self.ty(cond)? != Ty::Bool {
                    return None;
                }
                let c = self.expr(r, cond)?;
                let mut t = Region::default();
                let tv = self.block(&mut t, then)?;
                let mut f = Region::default();
                let fv = match els {
                    Some(x) => self.expr(&mut f, x)?,
                    None if ty == Ty::Unit => self.emit(&mut f, Ty::Unit, Op::Unit, e.span),
                    None => return None,
                };
                // both branches yield the `if`'s own type (no conversions yet)
                if ty != Ty::Unit && (self.types[tv.0 as usize] != ty || self.types[fv.0 as usize] != ty) {
                    return None;
                }
                t.result = Some(tv);
                f.result = Some(fv);
                Some(self.emit(r, ty, Op::If { cond: c, then: t, els: f }, e.span))
            }
            // x ▷ evolve(n, s => …), x ▷ settle(s => …, s => …): a loop whose
            // state keeps x's type (the step's lambda typed state → state)
            ExprKind::Prim { prim: prim @ (Prim::Evolve | Prim::Settle), args } if args.len() == 3 => {
                let evolve = *prim == Prim::Evolve;
                let (step, done) = if evolve { (&args[2], None) } else { (&args[1], Some(&args[2])) };
                // (a list as the state: each step's list would have to free
                // the last one; not covered yet)
                if matches!(ty, Ty::List(_) | Ty::Rec(_)) || self.ty(&args[0])? != ty || !self.lambda_is(step, ty, ty) || !done.is_none_or(|d| self.lambda_is(d, ty, Ty::Bool)) {
                    return None;
                }
                let init = self.expr(r, &args[0])?;
                let count = if evolve {
                    if self.ty(&args[1])? != Ty::I64 {
                        return None;
                    }
                    Some(self.expr(r, &args[1])?)
                } else {
                    None
                };
                // the state: a value the loop defines, seen by its regions
                let var = Val(self.next);
                self.next += 1;
                self.types.push(ty);
                let done = match done {
                    Some(d) => Some(self.lambda_region(d, var, ty)?),
                    None => None,
                };
                let step = self.lambda_region(step, var, ty)?;
                Some(self.emit(r, ty, Op::Loop { init, var, count, done, step }, e.span))
            }
            // match on a number, a truth value or a value kept in registers:
            // each arm's test, then the arm (its names bound) or the next;
            // the last arm must always match (no guards yet)
            ExprKind::Match { scrut, arms } if !arms.is_empty() => {
                let st = self.ty(scrut)?;
                if !matches!(st, Ty::I64 | Ty::F64 | Ty::Bool | Ty::Agg(_)) || arms.iter().any(|a| a.guard.is_some()) {
                    return None;
                }
                let v = self.expr(r, scrut)?;
                self.arms(r, v, st, arms, ty, e)
            }
            // a value kept in registers, from its parts
            ExprKind::Tuple(xs) | ExprKind::Array(xs) if matches!(ty, Ty::Agg(_)) => {
                let Ty::Agg(t) = ty else { unreachable!() };
                let ps = crate::parts(self.p, t)?;
                if ps.len() != xs.len() {
                    return None;
                }
                let mut vals = Vec::new();
                for (x, pt) in xs.iter().zip(&ps) {
                    // each part already of its place's type (no conversion yet)
                    if self.ty(x)? != Ty::of(self.p, *pt)? {
                        return None;
                    }
                    vals.push(self.expr(r, x)?);
                }
                Some(self.emit(r, ty, Op::Make(vals), e.span))
            }
            ExprKind::Record { fields, .. } if matches!(ty, Ty::Agg(_)) => {
                let Ty::Agg(t @ mimic_check::types::Ty::Obj(id)) = ty else { return None };
                let tid = mimic_check::types::obj_tid(id);
                let ps = crate::parts(self.p, t)?;
                if fields.len() != ps.len() {
                    return None;
                }
                // written in any order, kept in declaration order (each
                // evaluated where it is written)
                let mut slots: Vec<Option<Val>> = vec![None; ps.len()];
                for (name, x, _) in fields {
                    let k = self.p.types[tid].field_index(name)?;
                    if slots[k].is_some() || self.ty(x)? != Ty::of(self.p, ps[k])? {
                        return None;
                    }
                    slots[k] = Some(self.expr(r, x)?);
                }
                let vals = slots.into_iter().collect::<Option<Vec<_>>>()?;
                Some(self.emit(r, ty, Op::Make(vals), e.span))
            }
            ExprKind::Field { base, name } => {
                let Ty::Agg(bt) = self.ty(base)? else { return None };
                let k = match bt {
                    mimic_check::types::Ty::Tup(_) => name.parse().ok()?,
                    mimic_check::types::Ty::Obj(id) => self.p.types[mimic_check::types::obj_tid(id)].field_index(name)?,
                    _ => return None,
                };
                let ps = crate::parts(self.p, bt)?;
                if Ty::of(self.p, *ps.get(k)?)? != ty {
                    return None;
                }
                let b = self.expr(r, base)?;
                Some(self.emit(r, ty, Op::Get(b, k as u32), e.span))
            }
            // an element of a short list at a constant index
            ExprKind::Index { base, index } => {
                let Ty::Agg(bt) = self.ty(base)? else { return None };
                let (_, n) = crate::short_list(bt)?;
                let ExprKind::Int(k) = index.kind else { return None };
                if k < 0 || k >= n as i64 {
                    return None;
                }
                let b = self.expr(r, base)?;
                Some(self.emit(r, ty, Op::Get(b, k as u32), e.span))
            }
            // (the base a variable: reading it has no effects, so the new
            // values can be computed first, in the order written)
            ExprKind::With { base, fields } if matches!(ty, Ty::Agg(_)) && matches!(base.kind, ExprKind::Path(_)) && self.ty(base)? == ty => {
                let Ty::Agg(t @ mimic_check::types::Ty::Obj(id)) = ty else { return None };
                let tid = mimic_check::types::obj_tid(id);
                let ps = crate::parts(self.p, t)?;
                // the new values first (they may read the base), then the base
                let mut news = Vec::new();
                for (name, x, _) in fields {
                    let k = self.p.types[tid].field_index(name)?;
                    if self.ty(x)? != Ty::of(self.p, ps[k])? {
                        return None;
                    }
                    news.push((k, self.expr(r, x)?));
                }
                let mut cur = self.expr(r, base)?;
                for (k, x) in news {
                    cur = self.emit(r, ty, Op::Set(cur, k as u32, x), e.span);
                }
                Some(cur)
            }
            _ => None,
        }
    }
}

/// A list of i64 or f64 on the heap: what a computation element by element
/// makes (`⍳ n`, `xs × 2`).
fn elementwise_list(t: Ty) -> bool {
    matches!(t, Ty::List(mimic_check::types::Ty::Arr(mimic_check::types::El::I64 | mimic_check::types::El::F64, 0)))
}
