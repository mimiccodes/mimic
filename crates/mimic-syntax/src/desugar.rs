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
    Ok(())
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
        out.push(FnDecl { name: name.clone(), type_params: f.type_params.clone(), is_pub: false, params: fparams, ret: None, body, export_c: false, span });
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
