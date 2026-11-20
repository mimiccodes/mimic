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
