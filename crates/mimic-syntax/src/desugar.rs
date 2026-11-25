//! Rewrites run after parsing, before checking.
//!
//! `arms (lo = 0, hi = n) { … arm(a, b) … }` becomes a hidden function of the
//! enclosing module: its parameters are the `arms` parameters plus the outer
//! variables the body uses (captured by value), and `arm(…)` calls it. Being an
//! ordinary named function, a pure `arms` body compiles to interaction nets
//! like any other pure function, so its recursive calls run in parallel.

use crate::ast::*;
use crate::codes as C;
use crate::{Diag, Span};

/// Lift every `arms` in the file (nested modules and nested `arms` included),
/// and split `|` arms that bind names into one arm per alternative.
pub fn lift_arms(file: &mut File) -> Result<(), Diag> {
    lift_items(&mut file.module.items)?;
    split_or_items(&mut file.module.items)
}

fn split_or_items(items: &mut [Item]) -> Result<(), Diag> {
    for item in items.iter_mut() {
        match item {
            Item::Module(m) => split_or_items(&mut m.items)?,
            Item::Fn(f) => {
                let mut err = None;
                f.body.walk_mut(&mut |e| {
                    if err.is_some() {
                        return;
                    }
                    if let ExprKind::Match { arms, .. } = &mut e.kind
                        && let Err(d) = split_or_arms(arms)
                    {
                        err = Some(d);
                    }
                });
                if let Some(d) = err {
                    return Err(d);
                }
            }
            _ => {}
        }
    }
    Ok(())
}

/// `A(x) | B(x) => body` → `A(x) => body`, `B(x) => body`. Alternatives that
/// bind nothing stay one arm (one test, no duplicated body).
fn split_or_arms(arms: &mut Vec<Arm>) -> Result<(), Diag> {
    let mut out = Vec::with_capacity(arms.len());
    for arm in arms.drain(..) {
        nested_or_check(&arm.pat, true)?;
        // `(A | B) as v` keeps its `as` on each alternative
        let (inner, as_name) = match &arm.pat {
            Pattern::As(p, n, s) => (&**p, Some((n.clone(), *s))),
            p => (p, None),
        };
        let Pattern::Or(alts, span) = inner else {
            out.push(arm);
            continue;
        };
        let names = |p: &Pattern| {
            let mut v = Vec::new();
            p.bound_names(&mut v);
            v.sort();
            v
        };
        let first = names(&alts[0]);
        for a in &alts[1..] {
            if names(a) != first {
                return Err(Diag::new(&C::OR_BINDINGS, "every alternative of a `|` pattern must bind the same names", *span)
                    .note(format!("the first alternative binds [{}], this one binds [{}]", first.join(", "), names(a).join(", "))));
            }
        }
        if first.is_empty() {
            out.push(arm);
            continue;
        }
        for a in alts.clone() {
            let pat = match &as_name {
                Some((n, s)) => Pattern::As(Box::new(a), n.clone(), *s),
                None => a,
            };
            out.push(Arm { pat, guard: arm.guard.clone(), body: arm.body.clone(), span: arm.span });
        }
    }
    *arms = out;
    Ok(())
}

/// Alternatives below the top of a pattern may not bind names.
fn nested_or_check(p: &Pattern, top: bool) -> Result<(), Diag> {
    let kids: Vec<&Pattern> = match p {
        Pattern::Or(alts, span) => {
            if !top {
                let mut v = Vec::new();
                p.bound_names(&mut v);
                if !v.is_empty() {
                    return Err(Diag::new(&C::OR_BINDINGS, "alternatives inside a pattern cannot bind names yet", *span)
                        .help("split the alternatives into separate arms"));
                }
            }
            return alts.iter().try_for_each(|a| nested_or_check(a, false));
        }
        Pattern::As(inner, _, _) => return nested_or_check(inner, top),
        Pattern::Variant { args, .. } | Pattern::Tuple(args, _) => args.iter().collect(),
        Pattern::Record { fields, .. } => fields.iter().map(|(_, p)| p).collect(),
        Pattern::Array { elems, .. } => elems.iter().collect(),
        _ => Vec::new(),
    };
    kids.into_iter().try_for_each(|k| nested_or_check(k, false))
}

fn lift_items(items: &mut Vec<Item>) -> Result<(), Diag> {
    let mut lifted = Vec::new();
    let mut counter = 0usize;
    for item in items.iter_mut() {
        match item {
            Item::Module(m) => lift_items(&mut m.items)?,
            Item::Fn(f) => lift_fn(f, &mut counter, &mut lifted)?,
            _ => {}
        }
    }
    // a lifted body may hold further `arms` written in terms of its own locals
    let mut i = 0;
    while i < lifted.len() {
        let mut f: FnDecl = lifted[i].clone();
        let mut more = Vec::new();
        lift_fn(&mut f, &mut counter, &mut more)?;
        lifted[i] = f;
        lifted.extend(more);
        i += 1;
    }
    items.extend(lifted.into_iter().map(Item::Fn));
    // `let f = (a, b) => …` called only as `f(x, y)`: a hidden function too
    let mut hidden = Vec::new();
    for item in items.iter_mut() {
        if let Item::Fn(f) = item {
            lift_let_lambdas(f, &mut counter, &mut hidden);
        }
    }
    items.extend(hidden.into_iter().map(Item::Fn));
    for item in items.iter_mut() {
        if let Item::Fn(f) = item {
            let mut n = 0;
            blocks_mut(&mut f.body, &mut |b| hoist_copy_reads(b, &mut n));
            sink_with(&mut f.body);
            f.body.walk_mut(&mut |e| {
                if let ExprKind::Block(b) | ExprKind::Unsafe(b) = &mut e.kind {
                    sink_with(b);
                }
                if let ExprKind::If { then, .. } = &mut e.kind {
                    sink_with(then);
                }
            });
        }
    }
    Ok(())
}

/// `let out = if c { push(s.out, x) } else { concat(s.out, ys) }` followed
/// by `{ s with k: s.k + 2, out: out }` as the block's result: the update
/// moves into each branch, `if c { { s with k: s.k + 2, out: push(s.out, x) } }
/// else { … }`, where a record held once has its list taken out and changed
/// in place (codegen's steals) instead of copied at every step (a list
/// grown in a loop's state took time growing with the square of its
/// length). The same values in the same order: the branch's value first,
/// then the other fields (it is listed first). Only when `out` is used nowhere else, no branch
/// binds a name the other fields read, and some branch reads `s.out`.
fn sink_with(b: &mut Block) {
    let Some(tail) = &b.tail else { return };
    let ExprKind::With { base, fields } = &tail.kind else { return };
    let ExprKind::Path(bp) = &base.kind else { return };
    if bp.len() != 1 {
        return;
    }
    let Some(Stmt::Let { name: v, mutable: false, ty: None, value, .. }) = b.stmts.last() else { return };
    if !matches!(value.kind, ExprKind::If { .. }) || bp[0] == *v {
        return;
    }
    let is_v = |e: &Expr| matches!(&e.kind, ExprKind::Path(p) if p.len() == 1 && p[0] == *v);
    let at: Vec<usize> = fields.iter().enumerate().filter(|(_, (_, x, _))| is_v(x)).map(|(i, _)| i).collect();
    let [k] = at[..] else { return };
    let field = fields[k].0.clone();
    // `v` read nowhere else (the other fields, or inside the branches)
    let mut names = vec![bp[0].clone()];
    for (i, (_, x, _)) in fields.iter().enumerate() {
        if i == k {
            continue;
        }
        x.walk(&mut |e| {
            if let ExprKind::Path(p) = &e.kind
                && p.len() == 1
            {
                names.push(p[0].clone());
            }
        });
    }
    if names.contains(v) {
        return;
    }
    // the branches: each a block with a result, the last an `else`
    let mut branches: Vec<&Block> = Vec::new();
    let mut cur = value;
    loop {
        let ExprKind::If { then, els, .. } = &cur.kind else { return };
        branches.push(then);
        match els.as_deref() {
            Some(e) if matches!(e.kind, ExprKind::If { .. }) => cur = e,
            Some(Expr { kind: ExprKind::Block(eb), .. }) => {
                branches.push(eb);
                break;
            }
            _ => return,
        }
    }
    if branches.len() > 4 {
        return;
    }
    let mut reads_field = false;
    for br in &branches {
        let Some(t) = &br.tail else { return };
        let mut bound = Vec::new();
        br.bound_names(&mut bound);
        t.walk(&mut |e| match &e.kind {
            ExprKind::Lambda { params, .. } => bound.extend(params.iter().cloned()),
            ExprKind::Field { base, name } if *name == field && matches!(&base.kind, ExprKind::Path(p) if p.len() == 1 && p[0] == bp[0]) => reads_field = true,
            _ => {}
        });
        if bound.iter().any(|n| names.contains(n) || n == v) {
            return;
        }
        let mut uses_v = false;
        t.walk(&mut |e| uses_v |= is_v(e));
        for st in &br.stmts {
            if let Stmt::Let { value, .. } | Stmt::LetTuple { value, .. } | Stmt::Expr(value) | Stmt::Assign { value, .. } = st {
                value.walk(&mut |e| uses_v |= is_v(e));
            }
        }
        if uses_v {
            return;
        }
    }
    if !reads_field {
        return;
    }
    // rewrite: each branch's result becomes the record update
    let Some(Stmt::Let { value, .. }) = b.stmts.pop() else { unreachable!() };
    let tail = b.tail.take().unwrap();
    let ExprKind::With { base, fields } = tail.kind else { unreachable!() };
    let span = tail.span;
    fn put(e: &mut Expr, base: &Expr, fields: &[(String, Expr, crate::Span)], k: usize, span: crate::Span) {
        let ExprKind::If { then, els, .. } = &mut e.kind else { return };
        let wrap = |bl: &mut Block| {
            let r = bl.tail.take().unwrap();
            // (the branch's value listed first: the fields are computed in
            // their order, and it came first as written)
            let mut fs = fields.to_vec();
            let mut f = fs.remove(k);
            f.1 = *r;
            fs.insert(0, f);
            bl.tail = Some(Box::new(Expr { kind: ExprKind::With { base: Box::new(base.clone()), fields: fs }, span }));
        };
        wrap(then);
        match els.as_deref_mut() {
            Some(x) if matches!(x.kind, ExprKind::If { .. }) => put(x, base, fields, k, span),
            Some(Expr { kind: ExprKind::Block(eb), .. }) => wrap(eb),
            _ => {}
        }
    }
    let mut value = value;
    put(&mut value, &base, &fields, k, span);
    b.tail = Some(Box::new(value));
}

/// Every block in `b` (itself included, inner ones first).
fn blocks_mut(b: &mut Block, f: &mut dyn FnMut(&mut Block)) {
    fn expr(e: &mut Expr, f: &mut dyn FnMut(&mut Block)) {
        match &mut e.kind {
            ExprKind::If { cond, then, els } => {
                expr(cond, f);
                blocks_mut(then, f);
                if let Some(x) = els {
                    expr(x, f);
                }
            }
            ExprKind::Block(b) | ExprKind::Unsafe(b) => blocks_mut(b, f),
            ExprKind::Match { scrut, arms } => {
                expr(scrut, f);
                for a in arms {
                    expr(&mut a.body, f);
                }
            }
            ExprKind::Lambda { body, .. } => expr(body, f),
            ExprKind::Call { callee, args } => {
                expr(callee, f);
                args.iter_mut().for_each(|a| expr(a, f));
            }
            ExprKind::Binary { lhs, rhs, .. } => {
                expr(lhs, f);
                expr(rhs, f);
            }
            ExprKind::Unary { expr: x, .. } => expr(x, f),
            _ => {}
        }
    }
    for st in &mut b.stmts {
        match st {
            Stmt::Let { value, .. } | Stmt::LetTuple { value, .. } | Stmt::Expr(value) | Stmt::Assign { value, .. } => expr(value, f),
            Stmt::Return(Some(e), _) => expr(e, f),
            Stmt::LetElse { value, els, .. } => {
                expr(value, f);
                blocks_mut(els, f);
            }
            Stmt::Return(None, _) => {}
        }
    }
    if let Some(t) = &mut b.tail {
        expr(t, f);
    }
    f(b);
}

/// `let mut s = h` followed straight away by `s[i] = h[j]` `s[j] = h[i]`:
/// the indices and values are computed first, into hidden `let`s, then the
/// copy is made and changed. The values are the same (they read `h`, never
/// `s`), but `h` is now used last where it is copied, so a list `h` held
/// once is changed in place instead of copied at every swap (a hand-written
/// heap swapped a million-element list at every step). Only when every
/// index and value is plain (names, numbers, indexing, fields, arithmetic:
/// nothing a reordering could make observable) and reads `h`.
fn hoist_copy_reads(b: &mut Block, n: &mut usize) {
    fn plain(e: &Expr) -> bool {
        match &e.kind {
            ExprKind::Path(_) | ExprKind::Int(_) | ExprKind::Float(_) | ExprKind::Bool(_) => true,
            ExprKind::Index { base, index } => plain(base) && plain(index),
            ExprKind::Field { base, .. } => plain(base),
            ExprKind::Binary { lhs, rhs, .. } => plain(lhs) && plain(rhs),
            ExprKind::Unary { expr, .. } => plain(expr),
            _ => false,
        }
    }
    fn mentions(e: &Expr, name: &str) -> bool {
        let mut found = false;
        e.walk(&mut |x| {
            if matches!(&x.kind, ExprKind::Path(p) if p.len() == 1 && p[0] == name) {
                found = true;
            }
        });
        found
    }
    let mut i = 0;
    while i < b.stmts.len() {
        let (copy, from) = match &b.stmts[i] {
            Stmt::Let { name, mutable: true, ty: None, value, .. } => match &value.kind {
                ExprKind::Path(p) if p.len() == 1 && p[0] != *name => (name.clone(), p[0].clone()),
                // (or a part of it: `let mut rest = h ↑ (n - 1)`)
                ExprKind::Prim { prim: crate::glyphs::Prim::Take | crate::glyphs::Prim::Drop, args }
                    if args.len() == 2 && plain(&args[1]) && matches!(&args[0].kind, ExprKind::Path(p) if p.len() == 1 && p[0] != *name) =>
                {
                    let ExprKind::Path(p) = &args[0].kind else { unreachable!() };
                    (name.clone(), p[0].clone())
                }
                _ => {
                    i += 1;
                    continue;
                }
            },
            _ => {
                i += 1;
                continue;
            }
        };
        let assign_ok = |st: &Stmt| {
            matches!(st, Stmt::Assign { name, index: Some(idx), value, .. }
                if *name == copy && plain(idx) && plain(value) && !mentions(idx, &copy) && !mentions(value, &copy))
        };
        // `if c { s[i] = h[j] }`, c plain and not reading `s`, no else
        let guarded_ok = |st: &Stmt| {
            matches!(st, Stmt::Expr(Expr { kind: ExprKind::If { cond, then, els: None }, .. })
                if plain(cond) && !mentions(cond, &copy) && then.tail.is_none() && !then.stmts.is_empty() && then.stmts.iter().all(assign_ok))
        };
        let mut end = i + 1;
        while let Some(st) = b.stmts.get(end)
            && (assign_ok(st) || guarded_ok(st))
        {
            end += 1;
        }
        let reads_of = |st: &Stmt| matches!(st, Stmt::Assign { index: Some(idx), value, .. } if mentions(idx, &from) || mentions(value, &from));
        let reads = b.stmts[i + 1..end].iter().any(|st| match st {
            Stmt::Expr(Expr { kind: ExprKind::If { then, .. }, .. }) => then.stmts.iter().any(reads_of),
            st => reads_of(st),
        });
        if end == i + 1 || !reads {
            i = end.max(i + 1);
            continue;
        }
        let mut hoisted = Vec::new();
        for st in &mut b.stmts[i + 1..end] {
            match st {
                Stmt::Assign { index: Some(idx), value, span, .. } => {
                    for e in [idx, value] {
                        let t = format!("\0read{n}");
                        *n += 1;
                        let sp = e.span;
                        let old = std::mem::replace(e, Expr { kind: ExprKind::Path(vec![t.clone()]), span: sp });
                        hoisted.push(Stmt::Let { name: t, mutable: false, ty: None, value: old, span: *span });
                    }
                }
                // guarded: read only when the guard holds, as a list of
                // nothing or the value (`if c { [h[j]] } else { [] }`), and
                // taken from it inside the same guard: never a read the
                // program would not have made
                Stmt::Expr(Expr { kind: ExprKind::If { cond, then, .. }, .. }) => {
                    for inner in &mut then.stmts {
                        let Stmt::Assign { index: Some(idx), value, span, .. } = inner else { unreachable!() };
                        for e in [idx, value] {
                            // (only what reads the list: a constant index stays)
                            if !mentions(e, &from) {
                                continue;
                            }
                            let t = format!("\0read{n}");
                            *n += 1;
                            let sp = e.span;
                            let at = |kind| Expr { kind, span: sp };
                            let old = std::mem::replace(e, at(ExprKind::Index { base: Box::new(at(ExprKind::Path(vec![t.clone()]))), index: Box::new(at(ExprKind::Int(0))) }));
                            let some = Block { stmts: Vec::new(), tail: Some(Box::new(at(ExprKind::Array(vec![old])))), span: sp };
                            let none = at(ExprKind::Block(Block { stmts: Vec::new(), tail: Some(Box::new(at(ExprKind::Array(Vec::new())))), span: sp }));
                            let value = at(ExprKind::If { cond: cond.clone(), then: some, els: Some(Box::new(none)) });
                            hoisted.push(Stmt::Let { name: t, mutable: false, ty: None, value, span: *span });
                        }
                    }
                }
                _ => unreachable!(),
            }
        }
        let k = hoisted.len();
        b.stmts.splice(i..i, hoisted);
        i = end + k;
    }
    hoist_call_reads(b, n);
}

/// `sift_up(push(h, v), ≢ h)`: a later argument that only measures a
/// variable an earlier one uses is computed first, into a hidden `let`, so
/// the earlier one is the variable's last use (`push` then grows `h` in
/// place instead of copying it at every push). Only arguments that cannot
/// fail or do anything (names, numbers, fields, `≢`, `+ - ×` of those), in a
/// call that is a statement's value or a block's result.
fn hoist_call_reads(b: &mut Block, n: &mut usize) {
    fn quiet(e: &Expr) -> bool {
        match &e.kind {
            ExprKind::Path(_) | ExprKind::Int(_) | ExprKind::Float(_) | ExprKind::Bool(_) => true,
            ExprKind::Field { base, .. } => quiet(base),
            ExprKind::Prim { prim: crate::glyphs::Prim::Len, args } => args.iter().all(quiet),
            ExprKind::Binary { op: BinOp::Add | BinOp::Sub | BinOp::Mul, lhs, rhs } => quiet(lhs) && quiet(rhs),
            _ => false,
        }
    }
    fn names(e: &Expr) -> Vec<String> {
        let mut out = Vec::new();
        e.walk(&mut |x| {
            if let ExprKind::Path(p) = &x.kind
                && p.len() == 1
            {
                out.push(p[0].clone());
            }
        });
        out
    }
    // (the call and where its hidden `let`s go)
    let mut i = 0;
    while i <= b.stmts.len() {
        let mut call = if i < b.stmts.len() {
            match &mut b.stmts[i] {
                Stmt::Let { value, .. } | Stmt::Expr(value) | Stmt::Return(Some(value), _) => Some(value),
                _ => None,
            }
        } else {
            b.tail.as_deref_mut()
        };
        let mut hoisted = Vec::new();
        if let Some(c) = call.as_deref_mut() {
            first_arg_reads(c, &mut hoisted, n);
        }
        if let Some(Expr { kind: ExprKind::Call { args, .. }, .. }) = call {
            for j in 1..args.len() {
                let a = &args[j];
                if matches!(a.kind, ExprKind::Path(_) | ExprKind::Int(_) | ExprKind::Float(_) | ExprKind::Bool(_)) || !quiet(a) {
                    continue;
                }
                // (only beside an earlier argument that may take the
                // variable over, a call such as `push(h, v)`: plain
                // arithmetic on numbers, `f(zr * zr - zi * zi, 2.0 * zr * zi)`,
                // gains nothing and its order is better left alone)
                let earlier: Vec<String> = args[..j].iter().filter(|x| !quiet(x)).flat_map(names).collect();
                if !names(a).iter().any(|x| earlier.contains(x)) {
                    continue;
                }
                let t = format!("\0read{n}");
                *n += 1;
                let sp = args[j].span;
                let old = std::mem::replace(&mut args[j], Expr { kind: ExprKind::Path(vec![t.clone()]), span: sp });
                hoisted.push(Stmt::Let { name: t, mutable: false, ty: None, value: old, span: sp });
            }
        }
        let k = hoisted.len();
        let at = i.min(b.stmts.len());
        b.stmts.splice(at..at, hoisted);
        i += k + 1;
    }
}

/// `put(xs, j + 1, xs[j])`, also as the first argument of a call
/// (`insert(put(xs, j + 1, xs[j]), j - 1, v)`): an element of the list given
/// first, read by a later argument, is read first into a hidden `let`, so
/// the list is handed over and changed in place instead of copied at every
/// call (a hand-written insertion sort copied its list at every shift).
/// Only `name[i]` with `i` and the arguments between quiet, so what runs
/// before it is unchanged: nothing that can fail or do anything.
fn first_arg_reads(e: &mut Expr, hoisted: &mut Vec<Stmt>, n: &mut usize) {
    fn quiet(e: &Expr) -> bool {
        match &e.kind {
            ExprKind::Path(_) | ExprKind::Int(_) | ExprKind::Float(_) | ExprKind::Bool(_) => true,
            ExprKind::Field { base, .. } => quiet(base),
            ExprKind::Prim { prim: crate::glyphs::Prim::Len, args } => args.iter().all(quiet),
            ExprKind::Binary { op: BinOp::Add | BinOp::Sub | BinOp::Mul, lhs, rhs } => quiet(lhs) && quiet(rhs),
            _ => false,
        }
    }
    let ExprKind::Call { args, .. } = &mut e.kind else { return };
    let Some(first) = args.first_mut() else { return };
    if matches!(first.kind, ExprKind::Call { .. }) {
        first_arg_reads(first, hoisted, n);
        return;
    }
    let ExprKind::Path(p) = &first.kind else { return };
    if p.len() != 1 {
        return;
    }
    let name = p[0].clone();
    for j in 1..args.len() {
        let read = match &args[j].kind {
            ExprKind::Index { base, index } => matches!(&base.kind, ExprKind::Path(q) if q.len() == 1 && q[0] == name) && quiet(index),
            _ => false,
        };
        if read {
            let t = format!("\0read{n}");
            *n += 1;
            let sp = args[j].span;
            let old = std::mem::replace(&mut args[j], Expr { kind: ExprKind::Path(vec![t.clone()]), span: sp });
            hoisted.push(Stmt::Let { name: t, mutable: false, ty: None, value: old, span: sp });
        } else if !quiet(&args[j]) {
            return;
        }
    }
}

/// `let f = (a, b) => body` that the rest of its block only calls, `f(x, y)`,
/// becomes a hidden function of the module, `body`'s outer variables passed
/// as extra arguments (as `arms` does): each call is then a direct call of a
/// function specialised on its argument types, not a call of a function
/// value with every argument boxed (`integral` called such a helper 32
/// million times: 24 s, 0.6 s as a named function). Only when that changes
/// nothing: `f` is never used as a value, neither `f` nor what the lambda
/// uses is bound again after it in the block, and nothing it uses is ever
/// reassigned (a lambda sees a variable's value when it is made).
fn lift_let_lambdas(f: &mut FnDecl, counter: &mut usize, out: &mut Vec<FnDecl>) {
    let mut locals: Vec<String> = f.params.iter().map(|p| p.name.clone()).collect();
    f.body.bound_names(&mut locals);
    let mut changing = Vec::new();
    mutated_names(&f.body, &mut changing);
    let owner = f.name.clone();
    let type_params = f.type_params.clone();
    let mut each = |b: &mut Block| lift_in_block(b, &owner, &type_params, &locals, &changing, counter, out);
    f.body.walk_mut(&mut |e| match &mut e.kind {
        ExprKind::Block(b) | ExprKind::Unsafe(b) | ExprKind::If { then: b, .. } => each(b),
        _ => {}
    });
    each(&mut f.body);
}

/// Names declared `let mut` or assigned anywhere in the block.
fn mutated_names(b: &Block, out: &mut Vec<String>) {
    let mut blocks = vec![b];
    while let Some(b) = blocks.pop() {
        for s in &b.stmts {
            match s {
                Stmt::Let { name, mutable: true, .. } | Stmt::Assign { name, .. } => out.push(name.clone()),
                Stmt::LetElse { els, .. } => blocks.push(els),
                _ => {}
            }
        }
        b.walk(&mut |e| {
            if let ExprKind::Block(inner) | ExprKind::Unsafe(inner) | ExprKind::If { then: inner, .. } = &e.kind {
                for s in &inner.stmts {
                    if let Stmt::Let { name, mutable: true, .. } | Stmt::Assign { name, .. } = s {
                        out.push(name.clone());
                    }
                }
            }
        });
    }
}

fn lift_in_block(b: &mut Block, owner: &str, type_params: &[String], locals: &[String], changing: &[String], counter: &mut usize, out: &mut Vec<FnDecl>) {
    let mut i = 0;
    while i < b.stmts.len() {
        let Stmt::Let { name, mutable: false, ty: None, value, span } = &b.stmts[i] else {
            i += 1;
            continue;
        };
        let ExprKind::Lambda { params, body } = &value.kind else {
            i += 1;
            continue;
        };
        let (name, params, body, span) = (name.clone(), params.clone(), (**body).clone(), *span);
        let body_block = Block { stmts: vec![], tail: Some(Box::new(body.clone())), span };
        // what the lambda uses from outside: its captured variables
        let mut inside = params.clone();
        body_block.bound_names(&mut inside);
        let mut caps: Vec<String> = Vec::new();
        body.walk(&mut |x| {
            if let ExprKind::Path(p) = &x.kind
                && p.len() == 1
                && locals.contains(&p[0])
                && !inside.contains(&p[0])
                && !caps.contains(&p[0])
            {
                caps.push(p[0].clone());
            }
        });
        // the rest of the block: only calls of `name`, nothing bound again
        let rest = Block { stmts: b.stmts[i + 1..].to_vec(), tail: b.tail.clone(), span: b.span };
        let mut again = Vec::new();
        rest.bound_names(&mut again);
        let (mut uses, mut calls) = (0usize, 0usize);
        rest.walk(&mut |x| match &x.kind {
            ExprKind::Path(p) if p.len() == 1 && p[0] == name => uses += 1,
            ExprKind::Call { callee, args } if matches!(&callee.kind, ExprKind::Path(p) if p.len() == 1 && p[0] == name) && args.len() == params.len() => calls += 1,
            _ => {}
        });
        // an assignment or a `return` inside the lambda: left alone, so its
        // own diagnostics (a captured variable assigned, …) stay as they are
        let mut plain = true;
        body_block.walk(&mut |x| {
            if let ExprKind::Block(inner) | ExprKind::Unsafe(inner) | ExprKind::If { then: inner, .. } = &x.kind
                && inner.stmts.iter().any(|s| matches!(s, Stmt::Assign { .. } | Stmt::Return(..)))
            {
                plain = false;
            }
        });
        let liftable = plain
            && calls > 0
            && uses == calls
            && !again.contains(&name)
            && !changing.contains(&name)
            && caps.iter().all(|c| !again.contains(c) && !changing.contains(c));
        if !liftable {
            i += 1;
            continue;
        }
        *counter += 1;
        let hidden = format!("lambda_{owner}_{counter}");
        let path = |n: &str, span: Span| Expr { kind: ExprKind::Path(vec![n.to_string()]), span };
        let mut rewrite = |x: &mut Expr| {
            if let ExprKind::Call { callee, args } = &mut x.kind
                && matches!(&callee.kind, ExprKind::Path(p) if p.len() == 1 && p[0] == name)
            {
                callee.kind = ExprKind::Path(vec![hidden.clone()]);
                for c in &caps {
                    args.push(path(c, x.span));
                }
            }
        };
        let mut tail = b.tail.take();
        let mut later: Vec<Stmt> = b.stmts.drain(i + 1..).collect();
        {
            let mut tmp = Block { stmts: std::mem::take(&mut later), tail: tail.take(), span: b.span };
            tmp.walk_mut(&mut rewrite);
            later = tmp.stmts;
            tail = tmp.tail;
        }
        b.stmts.remove(i);
        b.stmts.extend(later);
        b.tail = tail;
        let mut fparams: Vec<Param> = params.iter().map(|n| Param { name: n.clone(), ty: None, default: None, span }).collect();
        fparams.extend(caps.iter().map(|c| Param { name: c.clone(), ty: None, default: None, span }));
        out.push(FnDecl { name: hidden, type_params: type_params.to_vec(), is_pub: false, params: fparams, ret: None, body: body_block, export_c: false, requires: vec![], ensures: vec![], span });
    }
}

fn lift_fn(f: &mut FnDecl, counter: &mut usize, out: &mut Vec<FnDecl>) -> Result<(), Diag> {
    // every name the function binds: what an `arms` body may capture
    let mut locals: Vec<String> = f.params.iter().map(|p| p.name.clone()).collect();
    f.body.bound_names(&mut locals);
    let owner = f.name.clone();
    let mut err: Option<Diag> = None;
    f.body.walk_mut(&mut |e| {
        if err.is_some() {
            return;
        }
        let ExprKind::Arms { params, body } = &mut e.kind else { return };
        *counter += 1;
        let name = format!("arms_{owner}_{counter}");
        let pnames: Vec<String> = params.iter().map(|(n, _)| n.clone()).collect();
        // captured: outer locals the body uses that it does not bind itself
        let mut inside = Vec::new();
        body.bound_names(&mut inside);
        let mut caps: Vec<String> = Vec::new();
        body.walk(&mut |x| {
            if let ExprKind::Path(p) = &x.kind
                && p.len() == 1
                && locals.contains(&p[0])
                && !pnames.contains(&p[0])
                && !inside.contains(&p[0])
                && !caps.contains(&p[0])
            {
                caps.push(p[0].clone());
            }
        });
        let path = |n: &str, span: Span| Expr { kind: ExprKind::Path(vec![n.to_string()]), span };
        // `arm(a, b)` → `name(a, b, captures…)`
        let mut body = body.clone();
        body.walk_mut(&mut |x| {
            if err.is_some() {
                return;
            }
            if let ExprKind::Call { callee, args } = &mut x.kind
                && matches!(&callee.kind, ExprKind::Path(p) if p.len() == 1 && p[0] == "arm")
            {
                if args.len() != pnames.len() {
                    err = Some(Diag::new(
                        &C::ARG_COUNT,
                        format!("`arm` takes {} argument(s) (one per `arms` parameter), got {}", pnames.len(), args.len()),
                        x.span,
                    ));
                    return;
                }
                callee.kind = ExprKind::Path(vec![name.clone()]);
                for c in &caps {
                    args.push(path(c, x.span));
                }
            }
        });
        let span = e.span;
        let mut fparams: Vec<Param> = pnames.iter().map(|n| Param { name: n.clone(), ty: None, default: None, span }).collect();
        fparams.extend(caps.iter().map(|c| Param { name: c.clone(), ty: None, default: None, span }));
        out.push(FnDecl { name: name.clone(), type_params: f.type_params.clone(), is_pub: false, params: fparams, ret: None, body, export_c: false, requires: vec![], ensures: vec![], span });
        // the `arms` expression itself: the first call
        let mut args: Vec<Expr> = params.iter().map(|(_, x)| x.clone()).collect();
        args.extend(caps.iter().map(|c| path(c, span)));
        e.kind = ExprKind::Call { callee: Box::new(path(&name, span)), args };
    });
    match err {
        Some(d) => Err(d),
        None => Ok(()),
    }
}

// ---- contracts ------------------------------------------------------------------

/// `requires` and `ensures`. With `on` (debug builds and tests), a function
/// with contracts becomes a wrapper: it checks each `requires`, calls the
/// original body (moved to a hidden function `name·body`, so a `return`
/// inside it still passes the checks below), checks each `ensures` with
/// `result` bound to the value, and returns it. A failed check stops the
/// program with the condition, the function and the line. Without `on`
/// (release builds) the contracts are dropped and cost nothing.
pub fn contracts(file: &mut File, src: &str, on: bool) {
    contract_items(&mut file.module.items, src, on);
}

fn contract_items(items: &mut Vec<Item>, src: &str, on: bool) {
    let mut added = Vec::new();
    for item in items.iter_mut() {
        match item {
            Item::Module(m) => contract_items(&mut m.items, src, on),
            Item::Fn(f) if !f.requires.is_empty() || !f.ensures.is_empty() => {
                // (they stay on the declaration, for signatures in hovers and docs)
                let (requires, ensures) = (f.requires.clone(), f.ensures.clone());
                if on {
                    added.push(checked(f, requires, ensures, src));
                }
            }
            _ => {}
        }
    }
    items.extend(added.into_iter().map(Item::Fn));
}

fn checked(f: &mut FnDecl, requires: Vec<Expr>, ensures: Vec<Expr>, src: &str) -> FnDecl {
    let span = f.span;
    let p = |n: &str| Expr { kind: ExprKind::Path(vec![n.to_string()]), span };
    let text = |e: &Expr| src.get(e.span.start..e.span.end).unwrap_or("…").split_whitespace().collect::<Vec<_>>().join(" ");
    let fail = |msg: Vec<InterpPart>, at: Span| -> Stmt {
        let call = Expr {
            kind: ExprKind::Call { callee: Box::new(Expr { kind: ExprKind::Path(vec!["contract_failed".into()]), span: at }), args: vec![Expr { kind: ExprKind::Interp(msg), span: at }] },
            span: at,
        };
        Stmt::Expr(call)
    };
    let check = |cond: &Expr, msg: Vec<InterpPart>| -> Stmt {
        let not = Expr { kind: ExprKind::Unary { op: UnOp::Not, expr: Box::new(cond.clone()) }, span: cond.span };
        Stmt::Expr(Expr {
            kind: ExprKind::If { cond: Box::new(not), then: Block { stmts: vec![fail(msg, cond.span)], tail: None, span: cond.span }, els: None },
            span: cond.span,
        })
    };
    let name = f.name.clone();
    let inner = format!("{name}·body");
    let mut stmts = Vec::new();
    for r in &requires {
        stmts.push(check(r, vec![InterpPart::Text(format!("`{name}` requires {} (line {}), and the call broke it", text(r), r.span.line))]));
    }
    let call = Expr { kind: ExprKind::Call { callee: Box::new(p(&inner)), args: f.params.iter().map(|q| p(&q.name)).collect() }, span };
    stmts.push(Stmt::Let { name: "result".into(), mutable: false, ty: f.ret.clone(), value: call, span });
    for e in &ensures {
        stmts.push(check(e, vec![InterpPart::Text(format!("`{name}` ensures {} (line {}), but it returned ", text(e), e.span.line)), InterpPart::Expr(p("result"))]));
    }
    let body = Block { stmts, tail: Some(Box::new(p("result"))), span: f.body.span };
    let original = std::mem::replace(&mut f.body, body);
    FnDecl {
        name: inner,
        type_params: f.type_params.clone(),
        is_pub: false,
        params: f.params.iter().map(|q| Param { name: q.name.clone(), ty: q.ty.clone(), default: None, span: q.span }).collect(),
        ret: f.ret.clone(),
        body: original,
        export_c: false,
        requires: vec![],
        ensures: vec![],
        span,
    }
}

// ---- property tests ---------------------------------------------------------

/// The generator and shrinker of a parameter type, or `None` when property
/// tests cannot generate it.
fn prop_kind(t: &Type) -> Option<(&'static str, &'static str)> {
    match t {
        Type::Named(n, _) => match n.as_str() {
            "i64" => Some(("prop_int(k)", "prop_shrink_int")),
            "f64" => Some(("prop_float(k)", "prop_shrink_float")),
            "bool" => Some(("prop_bool(k)", "prop_shrink_bool")),
            "str" => Some(("prop_str(k)", "prop_shrink_str")),
            _ => None,
        },
        Type::Array(el, _) => match &**el {
            Type::Named(n, _) => match n.as_str() {
                "i64" => Some(("prop_ints(k)", "prop_shrink_list_int")),
                "f64" => Some(("prop_floats(k)", "prop_shrink_list_float")),
                "bool" => Some(("prop_bools(k)", "prop_shrink_list")),
                "str" => Some(("prop_strs(k)", "prop_shrink_list")),
                _ => None,
            },
            _ => None,
        },
        _ => None,
    }
}

/// `test "name" for x: i64, xs: [i64] { … }`: the body runs for 100
/// generated inputs (seeded from the name, so every run is the same). When
/// one fails, its inputs are shrunk, one parameter at a time, to the
/// smallest that still fails; the body then runs once more out loud, so the
/// failed `expect` shows its sides, and the case is reported.
pub fn properties(file: &mut File) -> Result<(), Diag> {
    prop_items(&mut file.module.items)
}

fn prop_items(items: &mut Vec<Item>) -> Result<(), Diag> {
    let mut added = Vec::new();
    for item in items.iter_mut() {
        match item {
            Item::Module(m) => prop_items(&mut m.items)?,
            Item::Fn(f) if f.name.starts_with("test ") && !f.params.is_empty() => added.extend(property(f)?),
            _ => {}
        }
    }
    items.extend(added);
    Ok(())
}

fn property(f: &mut FnDecl) -> Result<Vec<Item>, Diag> {
    let name = f.name["test ".len()..].to_string();
    let id: String = format!("{:x}", crate::fnv1a(name.as_bytes()));
    let mut kinds = Vec::new();
    for p in &f.params {
        let ty = p.ty.as_ref().expect("property parameters have types");
        let Some(k) = prop_kind(ty) else {
            return Err(Diag::new(&C::UNSUPPORTED_SYNTAX, format!("property tests generate i64, f64, bool, str and lists of them, not `{ty}`"), p.span));
        };
        kinds.push((p.name.clone(), ty.to_string(), k));
    }
    let names: Vec<&str> = kinds.iter().map(|(n, ..)| n.as_str()).collect();
    let typed: Vec<String> = kinds.iter().map(|(n, t, _)| format!("{n}: {t}")).collect();
    let tuple_ty = if kinds.len() == 1 { kinds[0].1.clone() } else { format!("({})", kinds.iter().map(|(_, t, _)| t.clone()).collect::<Vec<_>>().join(", ")) };
    let tuple = |xs: &[String]| if xs.len() == 1 { xs[0].clone() } else { format!("({})", xs.join(", ")) };
    let args = names.join(", ");
    let body_fn = format!("prop_{id}_body");
    let fails_fn = format!("prop_{id}_fails");
    let search_fn = format!("prop_{id}_search");
    let shrink_fn = format!("prop_{id}_shrink");
    let gens: String = kinds.iter().map(|(n, _, (g, _))| format!("    let {n} = {g}\n")).collect();
    // shrinking: the first parameter that has a smaller failing value
    let mut steps = String::new();
    for (i, (n, _, (_, shrink))) in kinds.iter().enumerate() {
        let mut call: Vec<String> = names.iter().map(|x| x.to_string()).collect();
        call[i] = "prop_candidate".into();
        let mut next: Vec<String> = names.iter().map(|x| x.to_string()).collect();
        next[i] = format!("prop_smaller{i}[0]");
        steps += &format!(
            "    let prop_smaller{i} = {shrink}({n}) ⌿ ({shrink}({n}) ¨ (prop_candidate => {fails_fn}({})))\n    if ≢ prop_smaller{i} > 0 {{\n        return {shrink_fn}({}, rounds - 1)\n    }}\n",
            call.join(", "),
            next.join(", ")
        );
    }
    let shown: Vec<String> = kinds.iter().map(|(n, ..)| format!("{n} = {{{n}}}")).collect();
    let unpack = if kinds.len() == 1 { format!("let {} = case", names[0]) } else { format!("let ({}) = case", args) };
    let run_fn = format!("prop_{id}_run");
    let text = format!(
        "module generated\n\n\
fn {fails_fn}({typed}) -> bool {{\n    let before = expect_failures()\n    {body_fn}({args})\n    expect_failures() != before\n}}\n\n\
fn {search_fn}(k: i64) -> Option[{tuple_ty}] {{\n    if k == 100 {{ return None }}\n{gens}    if {fails_fn}({args}) {{ Some({tuple_args}) }} else {{ {search_fn}(k + 1) }}\n}}\n\n\
fn {shrink_fn}({typed}, rounds: i64) -> {tuple_ty} {{\n    if rounds == 0 {{ return {tuple_args} }}\n{steps}    {tuple_args}\n}}\n\n\
fn {run_fn}() {{\n    random::seed({seed})\n    expect_quiet(true)\n    let start = expect_failures()\n    let found = {search_fn}(0)\n    match found {{\n        None => {{\n            expect_forgive(start)\n            expect_quiet(false)\n        }},\n        Some(first) => {{\n            let case = {shrink_fn}({first_args}, 200)\n            {unpack}\n            expect_forgive(start)\n            expect_quiet(false)\n            {body_fn}({args})\n            prop_failed(\"{shown}\", {line})\n        }},\n    }}\n}}\n",
        typed = typed.join(", "),
        tuple_args = tuple(&names.iter().map(|x| x.to_string()).collect::<Vec<_>>()),
        run_fn = run_fn,
        seed = crate::fnv1a(name.as_bytes()) as i64 & 0x7fff_ffff,
        first_args = if kinds.len() == 1 { "first".to_string() } else { (0..kinds.len()).map(|i| format!("first.{i}")).collect::<Vec<_>>().join(", ") },
        shown = shown.join(", "),
        line = f.span.line,
    );
    let generated = crate::parser::parse_file(&text).map_err(|d| Diag::new(&C::UNSUPPORTED_SYNTAX, format!("internal: the property test could not be generated: {}", d.msg), f.span))?;
    // the body becomes a function of the parameters; the test runs the search
    let body = FnDecl {
        name: body_fn,
        type_params: vec![],
        is_pub: false,
        params: std::mem::take(&mut f.params),
        ret: None,
        body: std::mem::take(&mut f.body),
        export_c: false,
        requires: vec![],
        ensures: vec![],
        span: f.span,
    };
    let span = f.span;
    let call = Expr { kind: ExprKind::Call { callee: Box::new(Expr { kind: ExprKind::Path(vec![format!("prop_{id}_run")]), span }), args: vec![] }, span };
    f.body = Block { stmts: vec![Stmt::Expr(call)], tail: None, span };
    let mut out = vec![Item::Fn(body)];
    out.extend(generated.module.items);
    Ok(out)
}
