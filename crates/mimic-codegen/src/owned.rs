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

/// Whether no use of `name` runs after the call `call` (one of whose
/// arguments, `arg`, is `name`): uses inside the call's own arguments run
/// before it.
pub(crate) fn last_in_call(name: &str, body: &Block, call: usize, arg: usize) -> bool {
    let order = UseOrder::new(name, body);
    if order.in_closure {
        return false;
    }
    let mut range = None;
    let mut n = 0;
    body.walk(&mut |x| {
        if id(x) == call {
            let mut len = 0;
            x.walk(&mut |_| len += 1);
            range = Some((n, n + len));
        }
        n += 1;
    });
    let Some((a, b)) = range else { return false };
    if !order.last_but(arg, |_, w| a <= w && w < b) {
        return false;
    }
    // a use outside the call runs before it only where the order is the
    // walk's: the nearest expression holding both is a block, an `if` or a
    // `match` (in `m ⊠ power(m, n - 1)` the right operand runs first, so the
    // `m` on the left is read after the call)
    let mut nodes: Vec<(usize, usize, bool)> = Vec::new();
    let mut k = 0;
    body.walk(&mut |x| {
        let mut len = 0;
        x.walk(&mut |_| len += 1);
        nodes.push((k, k + len, matches!(x.kind, ExprKind::Block(_) | ExprKind::Unsafe(_) | ExprKind::If { .. } | ExprKind::Match { .. })));
        k += 1;
    });
    order.uses.iter().filter(|(_, w)| !(a <= *w && *w < b)).all(|(_, w)| {
        let holder = nodes.iter().filter(|(s, e, _)| *s <= a && b <= *e && *s <= *w && *w < *e).min_by_key(|(s, e, _)| e - s);
        holder.is_none_or(|&(_, _, seq)| seq)
    })
}

/// The ids of `name` where it is passed to the function `k` itself (its
/// specialisation) in the same place as the parameter `name`.
fn self_handed(k: &Key, i: Option<usize>, name: &str, body: &Block, spec: &Spec) -> Vec<usize> {
    let Some(i) = i else { return Vec::new() };
    let mut out = Vec::new();
    body.walk(&mut |x| {
        if let ExprKind::Call { args, .. } = &x.kind
            && spec.calls.get(&id(x)) == Some(k)
            && let Some(a) = args.get(i)
            && matches!(&a.kind, ExprKind::Path(p) if p.len() == 1 && p[0] == name)
        {
            out.push(id(a));
        }
    });
    out
}

/// The ids of `name` where it is the function's result, or a part of the
/// tuple or record that is (a block's tail, the branches of `if` and
/// `match`, a `return`).
fn result_uses(name: &str, body: &Block) -> Vec<usize> {
    fn results<'a>(e: &'a Expr, out: &mut Vec<&'a Expr>) {
        match &e.kind {
            ExprKind::If { then, els, .. } => {
                tails(then, out);
                if let Some(x) = els {
                    results(x, out);
                }
            }
            ExprKind::Block(b) | ExprKind::Unsafe(b) => tails(b, out),
            ExprKind::Match { arms, .. } => arms.iter().for_each(|a| results(&a.body, out)),
            ExprKind::Tuple(xs) => xs.iter().for_each(|x| out.push(x)),
            ExprKind::Record { fields, .. } => fields.iter().for_each(|(_, x, _)| out.push(x)),
            _ => out.push(e),
        }
    }
    fn tails<'a>(b: &'a Block, out: &mut Vec<&'a Expr>) {
        for st in &b.stmts {
            if let Stmt::Return(Some(e), _) = st {
                results(e, out);
            }
        }
        if let Some(t) = &b.tail {
            results(t, out);
        }
    }
    let mut res = Vec::new();
    tails(body, &mut res);
    res.into_iter().filter(|e| matches!(&e.kind, ExprKind::Path(p) if p.len() == 1 && p[0] == name)).map(id).collect()
}

/// The ids of `name` where it is the whole value of a `let mut v = name`,
/// anywhere in `body`.
fn mut_copies(name: &str, body: &Block) -> Vec<usize> {
    let mut out = Vec::new();
    let mut of = |b: &Block| {
        for st in &b.stmts {
            if let Stmt::Let { mutable: true, value, .. } = st
                && matches!(&value.kind, ExprKind::Path(p) if p.len() == 1 && p[0] == name)
            {
                out.push(id(value));
            }
        }
    };
    of(body);
    body.walk(&mut |e| match &e.kind {
        ExprKind::If { then, .. } => of(then),
        ExprKind::Block(b) | ExprKind::Unsafe(b) => of(b),
        _ => {}
    });
    out
}

/// One argument of a tail call to the function itself.
#[derive(Clone)]
pub(crate) struct TailArg {
    /// the variable, when the argument is one passed as it is
    pub direct: Option<String>,
    /// the names the argument's value may point into
    pub roots: HashSet<String>,
}

/// The calls of the function `fid` to itself that are its result (a block's
/// tail, a branch of an `if` or `match` there, a `return` outside lambdas),
/// each with, per argument, the names it may still be using when the call
/// is made (a variable passed as it is: just that name): every other local
/// can be released before the call, which is then the function's last act,
/// and the recursion runs as a loop.
pub(crate) fn tail_self_calls(p: &Program, module: ModId, fid: usize, body: &Block) -> HashMap<usize, Vec<TailArg>> {
    fn results<'a>(e: &'a Expr, out: &mut Vec<&'a Expr>) {
        match &e.kind {
            ExprKind::If { then, els, .. } => {
                tails(then, out);
                if let Some(x) = els {
                    results(x, out);
                }
            }
            ExprKind::Block(b) | ExprKind::Unsafe(b) => tails(b, out),
            ExprKind::Match { arms, .. } => arms.iter().for_each(|a| results(&a.body, out)),
            _ => out.push(e),
        }
    }
    fn tails<'a>(b: &'a Block, out: &mut Vec<&'a Expr>) {
        for st in &b.stmts {
            if let Stmt::Return(Some(e), _) = st {
                results(e, out);
            }
        }
        if let Some(t) = &b.tail {
            results(t, out);
        }
    }
    // the names an argument's value may point into (borrowed): the
    // variable itself, the list or record it is read from; anything else
    // but a call, arithmetic or a literal (which make new values): every
    // name in it
    fn roots(e: &Expr, out: &mut HashSet<String>) {
        match &e.kind {
            ExprKind::Path(p) if p.len() == 1 => {
                out.insert(p[0].clone());
            }
            ExprKind::Path(_) | ExprKind::Int(_) | ExprKind::Float(_) | ExprKind::Str(_) | ExprKind::Bool(_) => {}
            ExprKind::Field { base, .. } | ExprKind::Index { base, .. } => roots(base, out),
            ExprKind::Call { .. } | ExprKind::Binary { .. } | ExprKind::Unary { .. } => {}
            _ => e.walk(&mut |x| {
                if let ExprKind::Path(p) = &x.kind
                    && p.len() == 1
                {
                    out.insert(p[0].clone());
                }
            }),
        }
    }
    let mut res = Vec::new();
    tails(body, &mut res);
    let mut out = HashMap::new();
    for e in res {
        if let ExprKind::Call { callee, args } = &e.kind
            && let ExprKind::Path(path) = &callee.kind
            && matches!(p.lookup(module, path), Some(Resolved::Fn(f)) if f == fid)
        {
            let keep = args
                .iter()
                .map(|a| {
                    let mut roots_of = HashSet::new();
                    roots(a, &mut roots_of);
                    let direct = match &a.kind {
                        ExprKind::Path(p) if p.len() == 1 => Some(p[0].clone()),
                        _ => None,
                    };
                    TailArg { direct, roots: roots_of }
                })
                .collect();
            out.insert(id(e), keep);
        }
    }
    out
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
    ///
    /// Also a list of i64 that the function changes with `put` or `push`,
    /// or hands to a function that takes it: it arrives owned, so the
    /// change is in place when the caller gives up its list (released at
    /// the end when no site moves it).
    pub(crate) fn consumed(&self, k: &Key) -> Vec<bool> {
        self.consumed_in(k, &mut Vec::new())
    }

    /// `consumed`, with `stack` the functions being decided (a call back
    /// into one of them does not take: that answer is still open; nor does
    /// a call 8 deep). The answer depends only on `k`, so every caller sees
    /// the one the callee is compiled with.
    fn consumed_in(&self, k: &Key, stack: &mut Vec<Key>) -> Vec<bool> {
        let mut out: Vec<bool> = self.consumed_sites(k).iter().map(|s| !s.is_empty()).collect();
        let FnKind::User(d) = &self.p.fns[k.0].kind else { return out };
        let Ok(spec) = self.spec(k) else { return out };
        if self.plan.entered.contains(&k.0) || d.export_c || !self.heap_ok_target() {
            return out;
        }
        stack.push(k.clone());
        for (i, prm) in d.params.iter().enumerate() {
            // (a list of any kind, or a value of unknown type: a map, a list)
            if !out[i] && matches!(spec.params[i], Ty::Arr(_, 0) | Ty::Dyn) {
                out[i] = self.list_taken(k, &prm.name, &d.body, &spec, stack);
            }
            if !out[i] && matches!(spec.params[i], Ty::Obj(_)) && self.flat(spec.params[i]).is_none() {
                out[i] = self.record_taken(k, &prm.name, &d.body, &spec, stack);
            }
        }
        stack.pop();
        out
    }

    /// Whether the list parameter `name` of `k` reaches, at a last use, the
    /// list of a `put`/`push`/`concat` (of any element type; a map's `put`
    /// too) or a parameter another function takes:
    /// only then can taking it save a copy (a list still read after its
    /// change is copied anyway, and taking it would cost a retain per call).
    fn list_taken(&self, k: &Key, name: &str, body: &Block, spec: &Spec, stack: &mut Vec<Key>) -> bool {
        let is_name = |x: &Expr| matches!(&x.kind, ExprKind::Path(p) if p.len() == 1 && p[0] == name);
        let order = UseOrder::new(name, body);
        if order.in_closure {
            return false;
        }
        let module = self.p.fns[k.0].module;
        let pos = match &self.p.fns[k.0].kind {
            FnKind::User(d) => d.params.iter().position(|p| p.name == name),
            _ => None,
        };
        let mut found = false;
        body.walk(&mut |x| {
            let ExprKind::Call { callee, args } = &x.kind else { return };
            if found || !args.iter().any(|a| is_name(a) && order.last(a)) {
                return;
            }
            if let Some(k2) = spec.calls.get(&id(x)) {
                if k2 == k || stack.contains(k2) || stack.len() >= 8 {
                    return;
                }
                let takes = self.consumed_in(k2, stack);
                found = args.iter().enumerate().any(|(j, a)| is_name(a) && order.last(a) && takes.get(j).copied().unwrap_or(false));
                return;
            }
            let ExprKind::Path(path) = &callee.kind else { return };
            let Some(Resolved::Fn(f)) = self.p.lookup(module, path) else { return };
            let FnKind::Extern { decl, .. } = &self.p.fns[f].kind else { return };
            // (of any type: `acc` passed as `[]` arrives untyped, and the
            // `push` gives back a typed list)
            found = decl.intrinsic
                && self.p.mods[self.p.fns[f].module].name == "prelude"
                && matches!((decl.name.as_str(), args.len()), ("put", 3) | ("push", 2) | ("concat", 2))
                && is_name(&args[0])
                && order.last(&args[0]);
        });
        // `let mut v = xs` at its last use: a copy to change (`v[i] = …`,
        // `push`), which taking the list makes the list itself
        found
            || mut_copies(name, body).into_iter().any(|v| order.last_id(v))
            // handed to the function itself in the same place, or given back
            // (alone, or in the tuple or record that is the result): taking
            // it saves the reference the caller would keep, so a change
            // further down (`compressed[i] = root` in a `find` returning
            // `(parents, root)`) is made in place rather than to a copy
            || self_handed(k, pos, name, body, spec).into_iter().any(|v| order.last_id(v))
            || result_uses(name, body).into_iter().any(|v| order.last_id(v))
    }

    /// Whether the record parameter `name` of `k` ends, at a last use, as the
    /// base of a `{ … with … }` or as an argument another function takes
    /// (`look(look(s, 0), 1)`, `{ grow(t, t.k) with k: t.k + 1 }`, a field
    /// read and then `{ s with i: s.i + 1 }` handed on). A later use that
    /// only reads a field and is computed before the record moves does not
    /// count: one in another argument of the same call, or in the field
    /// values of the `with` whose base holds the move. Taking a record that
    /// is then not moved costs one retain per call; never a wrong result
    /// (callers and the callee ask the same question).
    fn record_taken(&self, k: &Key, name: &str, body: &Block, spec: &Spec, stack: &mut Vec<Key>) -> bool {
        let is_name = |x: &Expr| matches!(&x.kind, ExprKind::Path(p) if p.len() == 1 && p[0] == name);
        let order = UseOrder::new(name, body);
        if order.in_closure {
            return false;
        }
        // pre-order positions: each node's [start, end)
        let mut span: std::collections::HashMap<usize, (usize, usize)> = std::collections::HashMap::new();
        let mut n = 0;
        body.walk(&mut |x| {
            let mut len = 0;
            x.walk(&mut |_| len += 1);
            span.insert(id(x), (n, n + len));
            n += 1;
        });
        // the name's field reads, the `with` holding each node as its base,
        // and the places it may move: (its use, the node that takes it, the
        // call and argument when that is a call)
        let mut reads = HashSet::new();
        let mut base_of: std::collections::HashMap<usize, usize> = std::collections::HashMap::new();
        let mut moves: Vec<(usize, usize, Option<usize>)> = Vec::new();
        body.walk(&mut |x| match &x.kind {
            ExprKind::Field { base, .. } if is_name(base) => {
                reads.insert(id(base.as_ref()));
            }
            ExprKind::With { base, .. } => {
                base_of.insert(id(base.as_ref()), id(x));
                if is_name(base) {
                    moves.push((id(base.as_ref()), id(x), None));
                }
            }
            ExprKind::Call { args, .. } => {
                for (j, a) in args.iter().enumerate() {
                    if is_name(a) {
                        moves.push((id(a), id(x), Some(j)));
                    }
                }
            }
            _ => {}
        });
        for (u, at, arg) in moves {
            // what is computed before the move: the node, and the `with`
            // whose base it is
            let mut region = span[&at];
            if let Some(w) = base_of.get(&at) {
                region = span[w];
            }
            let ok = order.last_but(u, |w_id, w_pos| reads.contains(&w_id) && w_pos >= region.0 && w_pos < region.1);
            if !ok {
                continue;
            }
            let Some(j) = arg else { return true };
            let Some(k2) = spec.calls.get(&at) else { continue };
            if k2 == k || stack.contains(k2) || stack.len() >= 8 {
                continue;
            }
            if self.consumed_in(k2, stack).get(j).copied().unwrap_or(false) {
                return true;
            }
        }
        false
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
    pub(crate) fn heap_ok_target(&self) -> bool {
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

/// Every use of `name` in `body` that is a last use (one in each branch
/// that ends with it), or none when a use is inside a lambda: where a
/// parameter the function takes over can move on.
pub(crate) fn last_use_sites(name: &str, body: &Block) -> Vec<usize> {
    let order = UseOrder::new(name, body);
    if order.in_closure {
        return Vec::new();
    }
    // (only where the order is the walk's: the nearest expression holding
    // this use and another is a block, an `if` or a `match`. In
    // `xs ⌿ (xs ¨ f)` the mask runs first, so moving the last `xs` written
    // into `¨` released the list before `⌿` read it; in `f(xs, g(xs))` the
    // first argument is read before `g` takes it)
    let mut span: HashMap<usize, (usize, usize, bool)> = HashMap::new();
    let mut n = 0;
    body.walk(&mut |x| {
        let mut len = 0;
        x.walk(&mut |_| len += 1);
        let seq = matches!(x.kind, ExprKind::Block(_) | ExprKind::Unsafe(_) | ExprKind::If { .. } | ExprKind::Match { .. });
        span.insert(id(x), (n, n + len, seq));
        n += 1;
    });
    let ordered = |site: usize| {
        let Some(&(at, _, _)) = span.get(&site) else { return false };
        // the smallest expression holding the site and another use
        let holder = span
            .values()
            .filter(|(a, b, _)| *a <= at && at < *b && order.uses.iter().any(|(u, _)| *u != site && span.get(u).is_some_and(|&(p, _, _)| *a <= p && p < *b)))
            .min_by_key(|(a, b, _)| b - a);
        holder.is_none_or(|&(_, _, seq)| seq)
    };
    order.uses.iter().map(|(i, _)| *i).filter(|i| order.last_but(*i, |_, _| false) && ordered(*i)).collect()
}

/// The uses of a name in a body, in the order they run (the walk's order:
/// a function has no loops), and the branches of each `if` and `match`.
struct UseOrder {
    /// (expression id, position) of each use
    uses: Vec<(usize, usize)>,
    /// each `if`/`match`: the position range of every branch
    branches: Vec<Vec<(usize, usize)>>,
    /// a use inside a lambda (it may run any number of times, any time)
    in_closure: bool,
}

impl UseOrder {
    fn new(name: &str, body: &Block) -> UseOrder {
        let is_name = |x: &Expr| matches!(&x.kind, ExprKind::Path(p) if p.len() == 1 && p[0] == name);
        // a pre-order walk: every subtree is a run of positions
        let size = |e: &Expr| {
            let mut n = 0;
            e.walk(&mut |_| n += 1);
            n
        };
        let bsize = |b: &Block| {
            let mut n = 0;
            b.walk(&mut |_| n += 1);
            n
        };
        let mut uses = Vec::new();
        let mut closures = Vec::new();
        let mut branches = Vec::new();
        let mut n = 0;
        body.walk(&mut |x| {
            if is_name(x) {
                uses.push((id(x), n));
            }
            match &x.kind {
                ExprKind::Lambda { .. } | ExprKind::Arms { .. } => closures.push((n, n + size(x))),
                ExprKind::If { cond, then, els } => {
                    let t = n + 1 + size(cond);
                    let e = t + bsize(then);
                    branches.push(vec![(t, e), (e, e + els.as_deref().map_or(0, size))]);
                }
                ExprKind::Match { scrut, arms } => {
                    let mut at = n + 1 + size(scrut);
                    let mut r = Vec::new();
                    for arm in arms {
                        // (only the body is the arm's own: its guard runs on
                        // the way to every later arm too, so a use there is
                        // not a last use when a later arm uses the value:
                        // `Prop(k, v) if k == name => …, _ => prop(…, name)`
                        // moved `name` into the comparison)
                        let g = arm.guard.as_ref().map_or(0, size);
                        let len = g + size(&arm.body);
                        r.push((at + g, at + len));
                        at += len;
                    }
                    branches.push(r);
                }
                _ => {}
            }
            n += 1;
        });
        let in_closure = uses.iter().any(|(_, p)| closures.iter().any(|(s, e)| p > s && p < e));
        UseOrder { uses, branches, in_closure }
    }

    /// `last` for the use with id `u`, where a later use `allow(id,
    /// position)` accepts does not count.
    fn last_but(&self, u: usize, allow: impl Fn(usize, usize) -> bool) -> bool {
        let Some(&(_, at)) = self.uses.iter().find(|(i, _)| *i == u) else { return false };
        self.uses.iter().filter(|(_, w)| *w > at).all(|&(wi, w)| {
            allow(wi, w)
                || self.branches.iter().any(|bs| {
                    let pos = |p: usize| bs.iter().position(|(s, t)| p >= *s && p < *t);
                    matches!((pos(at), pos(w)), (Some(a), Some(b)) if a != b)
                })
        })
    }

    /// Whether the use `e` is a last use: every use after it is in another
    /// branch of an `if`/`match` that holds both.
    fn last(&self, e: &Expr) -> bool {
        self.last_id(id(e))
    }

    fn last_id(&self, e: usize) -> bool {
        let Some(&(_, u)) = self.uses.iter().find(|(i, _)| *i == e) else { return false };
        self.uses.iter().filter(|(_, w)| *w > u).all(|(_, w)| {
            self.branches.iter().any(|bs| {
                let at = |p: usize| bs.iter().position(|(s, t)| p >= *s && p < *t);
                matches!((at(u), at(*w)), (Some(a), Some(b)) if a != b)
            })
        })
    }
}
