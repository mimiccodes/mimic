//! Lowering from the checked syntax tree to the IR, one function
//! specialisation at a time. Anything outside what the IR covers yet gives
//! `None`: the function then takes today's code generator.

use crate::{Function, Inst, Op, Region, Ty, Val};
use mimic_check::types::{Spec, id};
use mimic_check::{FnId, FnKind, Program, Resolved};
use mimic_syntax::ast::{Arm, BinOp, Block, Expr, ExprKind, Pattern, Stmt, UnOp};
use mimic_syntax::glyphs::Prim;

struct Lower<'a> {
    p: &'a Program,
    module: mimic_check::ModId,
    spec: &'a Spec,
    next: u32,
    /// The type of every value so far, by number.
    types: Vec<Ty>,
    /// Variables in scope, innermost last.
    env: Vec<(String, Val, Ty)>,
}

/// The IR of `fid` specialised as `spec`, if the IR covers all of it.
pub fn function(p: &Program, fid: FnId, spec: &Spec) -> Option<Function> {
    let FnKind::User(decl) = &p.fns[fid].kind else { return None };
    let params: Vec<(String, Ty)> = decl.params.iter().zip(&spec.params).map(|(prm, t)| Some((prm.name.clone(), Ty::of(p, *t)?))).collect::<Option<_>>()?;
    let ret = Ty::of(p, spec.ret)?;
    if !calls_apart(&decl.body) {
        return None;
    }
    let mut l = Lower { p, module: p.fns[fid].module, spec, next: 0, types: Vec::new(), env: Vec::new() };
    let mut body = Region::default();
    for (i, (name, t)) in params.iter().enumerate() {
        let v = l.emit(&mut body, *t, Op::Param(i as u32), decl.span);
        l.env.push((name.clone(), v, *t));
    }
    let r = l.block(&mut body, &decl.body)?;
    // what the body gives must be what the function returns
    let rt = l.types[r.0 as usize];
    if ret != Ty::Unit && rt != ret {
        return None;
    }
    body.result = Some(r);
    let f = Function { name: p.fn_path(fid).join("::"), params, ret, body, values: l.next };
    Some(f)
}

/// No block runs two statements that call functions: such calls may run
/// at once (the code generator starts independent calls in parallel),
/// which the IR does not do yet.
fn calls_apart(b: &Block) -> bool {
    let mut ok = true;
    b.walk(&mut |e| {
        let blocks: Vec<&Block> = match &e.kind {
            ExprKind::Block(b) | ExprKind::Unsafe(b) => vec![b],
            ExprKind::If { then, .. } => vec![then],
            _ => vec![],
        };
        for b in blocks {
            ok &= calls_in_stmts(b) <= 1;
        }
    });
    ok && calls_in_stmts(b) <= 1
}

fn calls_in_stmts(b: &Block) -> usize {
    let has_call = |e: &Expr| {
        let mut c = false;
        e.walk(&mut |x| c |= matches!(x.kind, ExprKind::Call { .. }));
        c
    };
    let mut n = b
        .stmts
        .iter()
        .filter(|s| match s {
            Stmt::Let { value, .. } | Stmt::Expr(value) => has_call(value),
            _ => false,
        })
        .count();
    if let Some(t) = &b.tail
        && has_call(t)
    {
        n += 1;
    }
    n
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
        for s in &b.stmts {
            match s {
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

    fn expr(&mut self, r: &mut Region, e: &Expr) -> Option<Val> {
        let ty = self.ty(e)?;
        match &e.kind {
            ExprKind::Int(n) if ty == Ty::I64 => Some(self.emit(r, ty, Op::Int(*n), e.span)),
            ExprKind::Float(x) if ty == Ty::F64 => Some(self.emit(r, ty, Op::Float(*x), e.span)),
            ExprKind::Bool(b) if ty == Ty::Bool => Some(self.emit(r, ty, Op::Bool(*b), e.span)),
            ExprKind::Path(path) if path.len() == 1 => {
                let (_, v, t) = self.env.iter().rev().find(|(n, ..)| *n == path[0])?.clone();
                (t == ty).then_some(v)
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
            // (operators on single values only: on lists they go element by
            // element, which the IR does not cover yet)
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
            // ≢ xs of a list
            ExprKind::Prim { prim: Prim::Len, args } if args.len() == 1 && ty == Ty::I64 && matches!(self.ty(&args[0])?, Ty::List(_)) => {
                let v = self.expr(r, &args[0])?;
                Some(self.emit(r, ty, Op::Len(v), e.span))
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
            ExprKind::Call { callee, args } => {
                // (a call giving a list would hand one over to be freed:
                // not covered yet)
                if matches!(ty, Ty::List(_)) {
                    return None;
                }
                let ExprKind::Path(path) = &callee.kind else { return None };
                // a variable holding a function: not covered yet
                if path.len() == 1 && self.env.iter().any(|(n, ..)| *n == path[0]) {
                    return None;
                }
                let Some(Resolved::Fn(f)) = self.p.lookup(self.module, path) else { return None };
                if !matches!(self.p.fns[f].kind, FnKind::User(_)) || !self.spec.calls.contains_key(&id(e)) {
                    return None;
                }
                let mut vals = Vec::new();
                for a in args {
                    self.ty(a)?;
                    vals.push(self.expr(r, a)?);
                }
                Some(self.emit(r, ty, Op::Call { site: id(e), callee: f, args: vals }, e.span))
            }
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
                if self.ty(&args[0])? != ty || !self.lambda_is(step, ty, ty) || !done.is_none_or(|d| self.lambda_is(d, ty, Ty::Bool)) {
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
