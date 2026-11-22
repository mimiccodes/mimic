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
pub(crate) fn move_site(name: &str, roots: &[&Expr]) -> Option<usize> {
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

/// `name` may move at several `{ name with … }`, when each is in tail
/// position (the value the function or lambda gives back on that path):
/// whatever else reads `name` on that path has run by then (the field values
/// of the `with` too: they are computed before its base). Not when `name` is
/// used inside a lambda, or a `with` on it is not in tail position. Returns
/// the ids of those bases.
pub(crate) fn tail_with_sites(name: &str, body: &Block) -> Option<Vec<usize>> {
    tail_with_sites_in(name, body.tail.as_deref(), |f| body.walk(f))
}

/// The same for a lambda's body (an expression).
pub(crate) fn tail_with_sites_expr(name: &str, body: &Expr) -> Option<Vec<usize>> {
    tail_with_sites_in(name, Some(body), |f| body.walk(f))
}

fn tail_with_sites_in(name: &str, tail: Option<&Expr>, walk: impl Fn(&mut dyn FnMut(&Expr))) -> Option<Vec<usize>> {
    fn tails<'a>(e: &'a Expr, out: &mut Vec<&'a Expr>) {
        match &e.kind {
            ExprKind::Block(b) => {
                if let Some(t) = b.tail.as_deref() {
                    tails(t, out);
                }
            }
            ExprKind::If { then, els, .. } => {
                if let Some(t) = then.tail.as_deref() {
                    tails(t, out);
                }
                if let Some(e) = els {
                    tails(e, out);
                }
            }
            ExprKind::Match { arms, .. } => arms.iter().for_each(|a| tails(&a.body, out)),
            _ => out.push(e),
        }
    }
    let is_name = |x: &Expr| matches!(&x.kind, ExprKind::Path(p) if p.len() == 1 && p[0] == name);
    let mut ends = Vec::new();
    if let Some(t) = tail {
        tails(t, &mut ends);
    }
    let sites: Vec<usize> = ends
        .iter()
        .filter_map(|e| match &e.kind {
            ExprKind::With { base, .. } if is_name(base) => Some(id(base.as_ref())),
            // `step(s, x)` as the result: s handed on (no other argument
            // reads it: they are computed in order)
            // or `step({ s with … }, x)`, `f(g({ s with … }), x)`: the last
            // use of s is in one argument (the others do not read it), as the
            // argument itself or a `with` on it, at any depth of calls
            ExprKind::Call { .. } => last_use(e, &is_name),
            _ => None,
        })
        .collect();
    if sites.is_empty() {
        return None;
    }
    // every `with` on it is one of those; no use inside a lambda
    let mut ok = true;
    let mut check = |x: &Expr| {
        if let ExprKind::With { base, .. } = &x.kind
            && is_name(base)
            && !sites.contains(&id(base.as_ref()))
        {
            ok = false;
        }
        if let ExprKind::Lambda { body, .. } = &x.kind {
            body.walk(&mut |y| {
                if is_name(y) {
                    ok = false;
                }
            });
        }
    };
    walk(&mut check);
    ok.then_some(sites)
}

/// Where `e` uses the variable `is_name` names for the last time, when that
/// use can take it over: `e` is the variable, a `with` on it, or a call with
/// every use in one argument whose last use is such.
fn last_use(e: &Expr, is_name: &dyn Fn(&Expr) -> bool) -> Option<usize> {
    let count = |x: &Expr| {
        let mut n = 0;
        x.walk(&mut |y| {
            if is_name(y) {
                n += 1;
            }
        });
        n
    };
    match &e.kind {
        ExprKind::Path(_) if is_name(e) => Some(id(e)),
        ExprKind::With { base, .. } if is_name(base) => Some(id(base.as_ref())),
        ExprKind::Call { args, .. } => {
            let total: usize = args.iter().map(count).sum();
            let a = args.iter().find(|a| count(a) > 0)?;
            (count(a) == total).then(|| last_use(a, is_name)).flatten()
        }
        _ => None,
    }
}

/// Whether every one of `sites` is parameter `i` (`name`) handed unchanged
/// to the same place of a call to the function itself: a record only passed
/// along, as `nearest(c, x, j + 1)` passes `c`. Nothing then updates it, so
/// the function borrows it; consuming it would retain and release it on every
/// call from outside (from every element of a `¨`, all threads on one count).
fn passed_through(fname: &str, i: usize, name: &str, body: &Block, sites: &[usize]) -> bool {
    let mut through = Vec::new();
    body.walk(&mut |x| {
        if let ExprKind::Call { callee, args } = &x.kind
            && matches!(&callee.kind, ExprKind::Path(p) if p.len() == 1 && p[0] == fname)
            && let Some(a) = args.get(i)
            && matches!(&a.kind, ExprKind::Path(p) if p.len() == 1 && p[0] == name)
        {
            through.push(id(a));
        }
    });
    sites.iter().all(|s| through.contains(s))
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
        self.consumed_sites(k).iter().map(|s| !s.is_empty()).collect()
    }

    /// For each parameter it consumes: where it moves (the bases of the
    /// `with`s it ends in); empty when it is borrowed.
    pub(crate) fn consumed_sites(&self, k: &Key) -> Vec<Vec<usize>> {
        let FnKind::User(d) = &self.p.fns[k.0].kind else { return vec![] };
        let mut out = vec![Vec::new(); d.params.len()];
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
            out[i] = match site.filter(|_| is_base) {
                Some(s) => vec![s],
                // a state machine's step: `{ s with … }` in every branch
                // that changes it, each the function's result there
                None => tail_with_sites(&prm.name, &d.body).filter(|s| !passed_through(&d.name, i, &prm.name, &d.body, s)).unwrap_or_default(),
            };
        }
        out
    }

    /// Where values move and parameters are taken over (so `with` updates in
    /// place): every build, debug included, on targets with the atomic loads
    /// this needs (not eBPF).
    fn heap_ok_target(&self) -> bool {
        !self.module.get_triple().as_str().to_string_lossy().starts_with("bpf")
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
