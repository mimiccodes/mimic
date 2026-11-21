//! Lowering from the checked syntax tree to the IR, one function
//! specialisation at a time. Anything outside what the IR covers yet gives
//! `None`: the function then takes today's code generator.

use crate::{Function, Inst, Op, Region, Ty, Val};
use mimic_check::types::{Spec, id};
use mimic_check::{FnId, FnKind, Program, Resolved};
use mimic_syntax::ast::{BinOp, Block, Expr, ExprKind, Stmt};

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
            ExprKind::Call { callee, args } => {
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
