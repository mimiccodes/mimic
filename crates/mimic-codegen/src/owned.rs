//! Moving values instead of sharing them (reuse in place, F3).
//!
//! Values are reference counted, and a function normally *borrows* its
//! arguments: the caller keeps them alive, and `{ p with x: … }` on a
//! borrowed `p` must copy it, because the caller still holds it. Two things
//! let a record be updated in place instead:
//!
//! - **Moving locals.** A local used once (not inside a lambda) gives its
//!   value away at that use: the slot is cleared (a null, which releasing
//!   ignores), and the use owns the value. So does the base of
//!   `{ x with … }` when every other use of `x` is in its field values
//!   (computed first). And so does a parameter the function consumes.
//! - **Consumed parameters.** A function whose record parameter moves into
//!   `{ p with … }` (as above) takes that argument owned: every caller
//!   hands over a reference (its own, when it has one to give, else a new
//!   one). Then `with` sees the only reference and writes in place.
//!
//! `xs ¨ f` with such an `f` and a list nothing else holds hands each
//! element over in the same way (mim_each_own), so a chain like
//! `ps ¨ step ¨ step` updates the same records step after step.

use super::*;
use std::collections::HashSet;

/// Where `name` (a value of our own) may move, given every expression that
/// can still use it: its only use; or the base of a `{ name with … }` whose
/// field values hold every other use (they are computed before the base
/// moves). Uses inside lambdas never move. Returns that use's id.
fn move_site(name: &str, roots: &[&Expr]) -> Option<usize> {
    let is_name = |x: &Expr| matches!(&x.kind, ExprKind::Path(p) if p.len() == 1 && p[0] == name);
    let mut in_lambda = HashSet::new();
    let mut uses = Vec::new();
    let mut withs = Vec::new();
    for r in roots {
        r.walk(&mut |x| {
            if let ExprKind::Lambda { body, .. } = &x.kind {
                body.walk(&mut |y| {
                    in_lambda.insert(id(y));
                });
            }
            if is_name(x) {
                uses.push(id(x));
            }
            if let ExprKind::With { base, fields } = &x.kind
                && is_name(base)
            {
                // the base and the uses inside its field values
                let mut inside = vec![id(base.as_ref())];
                for (_, v, _) in fields.iter() {
                    v.walk(&mut |y| {
                        if is_name(y) {
                            inside.push(id(y));
                        }
                    });
                }
                withs.push(inside);
            }
        });
    }
    if uses.iter().any(|u| in_lambda.contains(u)) {
        return None;
    }
    if uses.len() == 1 {
        return Some(uses[0]);
    }
    // one `with` on it, every other use inside its field values
    let [inside] = withs.as_slice() else { return None };
    (inside.len() == uses.len() && uses.iter().all(|u| inside.contains(u))).then_some(inside[0])
}

/// Every expression of a block's statements and tail, as roots to walk.
pub(crate) fn block_roots<'a>(stmts: &'a [Stmt], tail: Option<&'a Expr>, out: &mut Vec<&'a Expr>) {
    for s in stmts {
        match s {
            Stmt::Let { value, .. } | Stmt::Expr(value) | Stmt::LetTuple { value, .. } => out.push(value),
            Stmt::Assign { index, value, .. } => {
                if let Some(i) = index {
                    out.push(i);
                }
                out.push(value);
            }
            Stmt::Return(Some(e), _) => out.push(e),
            Stmt::Return(None, _) => {}
            Stmt::LetElse { value, els, .. } => {
                out.push(value);
                block_roots(&els.stmts, els.tail.as_deref(), out);
            }
        }
    }
    if let Some(t) = tail {
        out.push(t);
    }
}

impl<'ctx> Cg<'ctx, '_> {
    /// Which parameters of the specialisation `k` it takes owned (see the
    /// module comment): a record used once, as the base of a `with`.
    pub(crate) fn consumed(&self, k: &Key) -> Vec<bool> {
        self.consumed_sites(k).iter().map(|s| s.is_some()).collect()
    }

    /// For each parameter it consumes: where it moves (the base of a `with`).
    pub(crate) fn consumed_sites(&self, k: &Key) -> Vec<Option<usize>> {
        let FnKind::User(d) = &self.p.fns[k.0].kind else { return vec![] };
        let mut out = vec![None; d.params.len()];
        let Ok(spec) = self.spec(k) else { return out };
        if self.plan.entered.contains(&k.0) || d.export_c || !self.heap_ok_target() {
            return out;
        }
        let mut roots = Vec::new();
        block_roots(&d.body.stmts, d.body.tail.as_deref(), &mut roots);
        for (i, prm) in d.params.iter().enumerate() {
            let t = spec.params[i];
            if !matches!(t, Ty::Obj(_)) || self.flat(t).is_some() {
                continue;
            }
            // only worth it where a `with` gets the record
            let site = move_site(&prm.name, &roots);
            let mut is_base = false;
            if let Some(site) = site {
                d.body.walk(&mut |x| {
                    if let ExprKind::With { base, .. } = &x.kind
                        && id(base.as_ref()) == site
                    {
                        is_base = true;
                    }
                });
            }
            out[i] = site.filter(|_| is_base);
        }
        out
    }

    /// Optimised builds on targets with the inline allocator: where `with`
    /// updates in place (see heap_ok).
    fn heap_ok_target(&self) -> bool {
        self.opt_level >= 2 && !self.module.get_triple().as_str().to_string_lossy().starts_with("bpf")
    }

    /// Where `let name = …` (followed by `rest`) may move (see move_site).
    pub(crate) fn let_move_site(&self, name: &str, rest: &[Stmt], tail: Option<&Expr>) -> Option<usize> {
        if !self.heap_ok_target() {
            return None;
        }
        let mut roots = Vec::new();
        block_roots(rest, tail, &mut roots);
        move_site(name, &roots)
    }

    /// An argument for a parameter the callee consumes: an owned reference
    /// (the caller's, when it is a temporary; else a new one).
    pub(crate) fn hand_over(&mut self, v: V<'ctx>) -> R<V<'ctx>> {
        self.own(v)
    }
}
