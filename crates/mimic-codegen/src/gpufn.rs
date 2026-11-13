//! Functions as Metal functions, for `xs ¨ f` on the GPU (see gpu.rs).
//!
//! A pure function (or lambda) on f32, i32, i64 and truth values becomes a
//! Metal function computing exactly what the CPU computes:
//!
//! - f32 arithmetic is f32 arithmetic on both (the CPU rounds each result
//!   to f32, see Cg::narrow_binary); i64 wraps; i32 is computed as i64 and
//!   wrapped, as on the CPU;
//! - `let`, `if`/`else`, `and`/`or`, `not`, comparisons, `⌈ ⌊`, `abs`,
//!   calls to other such functions;
//! - a function calling itself in tail position becomes a loop (Metal has
//!   no recursion); any other recursion gives no kernel.
//!
//! Anything that could stop the program on the CPU (an integer division by
//! a variable, a conversion out of range) or that the GPU cannot compute
//! exactly (f64) gives no kernel either: the map then runs on the CPU.

use super::*;
use std::fmt::Write as _;

/// A Metal function: its name and the definitions it needs (its own last).
#[derive(Debug)]
pub(crate) struct MslFn {
    pub name: String,
    pub defs: String,
    /// the types of the captured values (further parameters)
    pub caps: Vec<Ty>,
    /// captured values divided by: the GPU runs only when none is 0 or -1
    /// (the CPU stops the program on 0)
    pub guards: Vec<usize>,
    /// it does 64-bit integer arithmetic (slow on Apple GPUs: emulated)
    pub long_math: bool,
}

/// Names unique in the program (a kernel may call several functions).
static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

fn prefix() -> String {
    format!("m{}_", NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed))
}

/// The Metal type of a Mimic type, if a kernel can hold it.
pub(crate) fn msl_ty(t: Ty) -> Option<&'static str> {
    Some(match t {
        Ty::F32 => "float",
        Ty::I32 => "int",
        Ty::I64 => "long",
        Ty::Bool => "bool",
        _ => return None,
    })
}

/// A float written exactly (its bits).
pub(crate) fn f32_lit(x: f32) -> String {
    format!("as_type<float>({:#010x}u)", x.to_bits())
}

/// Definitions made so far, shared by every function of one kernel.
#[derive(Default)]
struct Gen {
    long_math: bool,
    pre: String,
    guards: Vec<usize>,
    defs: String,
    done: Vec<(Key, String)>,
    visiting: Vec<Key>,
}

/// One function body being written.
struct Body<'a> {
    spec: &'a Spec,
    /// in scope: Mimic name, Metal name, type
    vars: Vec<(String, String, Ty)>,
    out: String,
    fresh: usize,
    /// the function itself (its tail calls loop): key, parameter names and types
    me: Option<(Key, Vec<(String, Ty)>)>,
    ret: Ty,
    /// the parameters from this one on are captured values (a lambda's)
    caps_from: Option<usize>,
}

impl Body<'_> {
    fn tmp(&mut self) -> String {
        self.fresh += 1;
        format!("t{}", self.fresh)
    }

    fn ty(&self, e: &Expr) -> Ty {
        self.spec.exprs.get(&id(e)).copied().unwrap_or(Ty::Dyn)
    }
}

/// A value of type `from` as type `to`, where that is exact and cannot fail
/// (as the CPU converts it).
fn conv(x: String, from: Ty, to: Ty) -> Option<String> {
    Some(match (from, to) {
        _ if from == to => x,
        (Ty::I32 | Ty::Bool, Ty::I64) => format!("((long){x})"),
        (Ty::Bool, Ty::I32) => format!("((int){x})"),
        (Ty::I32, Ty::F32) => format!("((float){x})"),
        (Ty::I64, Ty::F32) => format!("l2f({x})"),
        _ => return None,
    })
}

impl<'ctx> Cg<'ctx, '_> {
    /// The Metal function for a lambda of one parameter of type `pt` (and
    /// its captures, as further parameters, in `caps` order).
    pub(crate) fn msl_lambda(&self, lambda: &Expr, caps: &[(String, Ty)]) -> Option<MslFn> {
        let ExprKind::Lambda { params, body } = &lambda.kind else { return None };
        let spec = self.cur_spec.as_ref()?;
        let (pts, rt) = spec.lambdas.get(&id(lambda)).cloned()?;
        if params.len() != 1 || pts.len() != 1 {
            return None;
        }
        let mut g = Gen { pre: prefix(), ..Gen::default() };
        let mut sig = vec![(params[0].clone(), pts[0])];
        sig.extend(caps.iter().cloned());
        let name = format!("{}lam", g.pre);
        self.msl_body(&mut g, spec, &name, &sig, rt, None, |b, cg, g| {
            b.caps_from = Some(1);
            cg.msl_tail(g, b, body)
        })?;
        Some(MslFn { name, defs: g.defs, caps: caps.iter().map(|(_, t)| *t).collect(), guards: g.guards, long_math: g.long_math })
    }

    /// The Metal function for the specialisation `k` of a user function.
    pub(crate) fn msl_named(&self, k: &Key) -> Option<MslFn> {
        let mut g = Gen { pre: prefix(), ..Gen::default() };
        let name = self.msl_fn(&mut g, k)?;
        Some(MslFn { name, defs: g.defs, caps: Vec::new(), guards: Vec::new(), long_math: g.long_math })
    }

    fn msl_fn(&self, g: &mut Gen, k: &Key) -> Option<String> {
        if let Some((_, n)) = g.done.iter().find(|(d, _)| d == k) {
            return Some(n.clone());
        }
        if g.visiting.contains(k) || g.visiting.len() > 16 {
            return None; // recursion other than a tail call to itself
        }
        let FnKind::User(d) = &self.p.fns[k.0].kind else { return None };
        if !self.purity.fns[k.0] || d.export_c {
            return None;
        }
        let spec = self.spec(k).ok()?;
        let sig: Vec<(String, Ty)> = d.params.iter().zip(&spec.params).map(|(p, t)| (p.name.clone(), *t)).collect();
        let name = format!("{}f{}", g.pre, g.done.len() + g.visiting.len());
        g.visiting.push(k.clone());
        let r = self.msl_body(g, &spec, &name, &sig, spec.ret, Some(k.clone()), |b, cg, g| cg.msl_block_tail(g, b, &d.body));
        g.visiting.pop();
        r?;
        g.done.push((k.clone(), name.clone()));
        Some(name)
    }

    /// Write `static T name(params) { while (true) { … } }` into g.defs.
    #[allow(clippy::too_many_arguments)]
    fn msl_body(&self, g: &mut Gen, spec: &Spec, name: &str, sig: &[(String, Ty)], ret: Ty, me: Option<Key>, f: impl FnOnce(&mut Body, &Self, &mut Gen) -> Option<()>) -> Option<()> {
        let rt = msl_ty(ret)?;
        let mut b = Body { spec, vars: Vec::new(), out: String::new(), fresh: 0, me: None, ret, caps_from: None };
        let mut params = Vec::new();
        for (i, (pname, t)) in sig.iter().enumerate() {
            let m = format!("p{i}");
            params.push(format!("{} {m}", msl_ty(*t)?));
            b.vars.push((pname.clone(), m, *t));
        }
        if let Some(k) = me {
            b.me = Some((k, sig.to_vec()));
        }
        f(&mut b, self, g)?;
        let _ = write!(g.defs, "static {rt} {name}({}) {{\n  while (true) {{\n{}  }}\n}}\n", params.join(", "), b.out);
        Some(())
    }

    /// A block whose value the function returns.
    fn msl_block_tail(&self, g: &mut Gen, b: &mut Body, blk: &Block) -> Option<()> {
        let depth = b.vars.len();
        for s in &blk.stmts {
            self.msl_stmt(g, b, s)?;
        }
        let r = self.msl_tail(g, b, blk.tail.as_deref()?);
        b.vars.truncate(depth);
        r
    }

    /// An expression whose value the function returns: `return`, or for a
    /// call of the function itself, new parameters and round again.
    fn msl_tail(&self, g: &mut Gen, b: &mut Body, e: &Expr) -> Option<()> {
        match &e.kind {
            ExprKind::If { cond, then, els } => {
                let c = self.msl_expr(g, b, cond)?;
                if b.ty(cond) != Ty::Bool {
                    return None;
                }
                let _ = writeln!(b.out, "  if ({c}) {{");
                self.msl_block_tail(g, b, then)?;
                b.out.push_str("  } else {\n");
                self.msl_tail(g, b, els.as_deref()?)?;
                b.out.push_str("  }\n");
                Some(())
            }
            ExprKind::Block(blk) => self.msl_block_tail(g, b, blk),
            ExprKind::Call { args, .. } if b.me.as_ref().is_some_and(|(k, _)| b.spec.calls.get(&id(e)) == Some(k)) => {
                let (_, sig) = b.me.clone()?;
                let mut vals = Vec::new();
                for (a, (_, t)) in args.iter().zip(&sig) {
                    vals.push(self.msl_as(g, b, a, *t)?);
                }
                if vals.len() != sig.len() {
                    return None;
                }
                // all arguments first, then the parameters (they may use each other)
                let mut names = Vec::new();
                for (v, (_, t)) in vals.iter().zip(&sig) {
                    let n = b.tmp();
                    let _ = writeln!(b.out, "  {} {n} = {v};", msl_ty(*t)?);
                    names.push(n);
                }
                for (i, n) in names.iter().enumerate() {
                    let _ = writeln!(b.out, "  p{i} = {n};");
                }
                b.out.push_str("  continue;\n");
                Some(())
            }
            _ => {
                let x = self.msl_as(g, b, e, b.ret)?;
                let _ = writeln!(b.out, "  return {x};");
                Some(())
            }
        }
    }

    fn msl_stmt(&self, g: &mut Gen, b: &mut Body, s: &Stmt) -> Option<()> {
        match s {
            Stmt::Let { name, mutable: false, value, .. } => {
                let t = b.spec.lets.get(&id(s)).copied()?;
                let x = self.msl_as(g, b, value, t)?;
                let m = b.tmp();
                let _ = writeln!(b.out, "  {} {m} = {x};", msl_ty(t)?);
                b.vars.push((name.clone(), m, t));
                Some(())
            }
            _ => None,
        }
    }

    /// The value of `e` (its type: b.ty(e)), statements it needs written first.
    /// `e` as a value of type `to` (an argument, a binding, a result): a
    /// number written in the program converted here, as the CPU's compiler
    /// does; anything else as conv allows.
    fn msl_as(&self, g: &mut Gen, b: &mut Body, e: &Expr, to: Ty) -> Option<String> {
        let (lit, neg) = match &e.kind {
            ExprKind::Unary { op: UnOp::Neg, expr } => (expr.as_ref(), true),
            _ => (e, false),
        };
        let sign = if neg { -1.0 } else { 1.0 };
        match (&lit.kind, to) {
            (ExprKind::Float(v), Ty::F32) => return Some(f32_lit((sign * v) as f32)),
            (ExprKind::Int(v), Ty::F32) => return Some(f32_lit((sign * *v as f64) as f32)),
            (ExprKind::Int(v), Ty::I32) => {
                let v = if neg { v.checked_neg()? } else { *v };
                return (v == v as i32 as i64).then(|| format!("({v})"));
            }
            _ => {}
        }
        let x = self.msl_expr(g, b, e)?;
        conv(x, b.ty(e), to)
    }

    fn msl_expr(&self, g: &mut Gen, b: &mut Body, e: &Expr) -> Option<String> {
        let ty = b.ty(e);
        msl_ty(ty)?;
        Some(match &e.kind {
            ExprKind::Int(v) => match ty {
                Ty::I64 if *v != i64::MIN => format!("({v}L)"),
                Ty::I32 => format!("({v})"),
                Ty::F32 => f32_lit(*v as f64 as f32),
                _ => return None,
            },
            ExprKind::Float(v) if ty == Ty::F32 => f32_lit(*v as f32),
            ExprKind::Bool(v) => v.to_string(),
            ExprKind::Path(p) if p.len() == 1 => b.vars.iter().rev().find(|(n, ..)| *n == p[0]).map(|(_, m, _)| m.clone())?,
            ExprKind::Unary { op, expr } => {
                let tx = b.ty(expr);
                let x = self.msl_expr(g, b, expr)?;
                match (op, tx, ty) {
                    (UnOp::Not, Ty::Bool, Ty::Bool) => format!("(!{x})"),
                    (UnOp::Neg, Ty::F32, Ty::F32) => format!("(-{x})"),
                    (UnOp::Abs, Ty::F32, Ty::F32) => format!("fabs({x})"),
                    (UnOp::Neg, Ty::I64 | Ty::I32, _) => wrap(format!("((long)(0ul - (ulong){x}))"), ty)?,
                    (UnOp::Abs, Ty::I64 | Ty::I32, _) => wrap(format!("((long)((long){x} < 0 ? 0ul - (ulong){x} : (ulong){x}))"), ty)?,
                    (UnOp::BitNot, Ty::I64 | Ty::I32, _) => wrap(format!("(~(long){x})"), ty)?,
                    _ => return None,
                }
            }
            ExprKind::Binary { op, lhs, rhs } => {
                let (ta, tb) = (b.ty(lhs), b.ty(rhs));
                let x = self.msl_expr(g, b, lhs)?;
                let y = self.msl_expr(g, b, rhs)?;
                // dividing by a captured value: checked once, before the GPU
                let cap = match (&rhs.kind, b.caps_from) {
                    (ExprKind::Path(p), Some(from)) if p.len() == 1 && matches!(op, BinOp::IntDiv | BinOp::Mod) => {
                        b.vars.iter().rev().find(|(n, ..)| *n == p[0]).and_then(|(_, m, _)| m.strip_prefix('p')?.parse::<usize>().ok()).filter(|i| *i >= from).map(|i| i - from)
                    }
                    _ => None,
                };
                if let Some(c) = cap
                    && !g.guards.contains(&c)
                {
                    g.guards.push(c);
                }
                // (in a function that may loop: once per element is cheap)
                if b.me.is_some() && types::num_join(ta, false, tb, false) == Ty::I64 && !matches!(op, BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge | BinOp::And | BinOp::Or) {
                    g.long_math = true;
                }
                msl_binary(*op, x, ta, y, tb, ty, rhs, cap.is_some())?
            }
            ExprKind::If { cond, then, els } => {
                if b.ty(cond) != Ty::Bool {
                    return None;
                }
                let c = self.msl_expr(g, b, cond)?;
                let t = b.tmp();
                let _ = writeln!(b.out, "  {} {t};", msl_ty(ty)?);
                let _ = writeln!(b.out, "  if ({c}) {{");
                let x = self.msl_block_value(g, b, then)?;
                let x = conv(x, then.tail.as_deref().map_or(Ty::Unit, |v| b.ty(v)), ty)?;
                let _ = writeln!(b.out, "  {t} = {x};\n  }} else {{");
                let els = els.as_deref()?;
                let y = self.msl_expr(g, b, els)?;
                let y = conv(y, b.ty(els), ty)?;
                let _ = writeln!(b.out, "  {t} = {y};\n  }}");
                t
            }
            ExprKind::Block(blk) => self.msl_block_value(g, b, blk)?,
            ExprKind::Call { callee, args } => {
                let ExprKind::Path(_) = &callee.kind else { return None };
                let k = b.spec.calls.get(&id(e)).cloned()?;
                if b.me.as_ref().is_some_and(|(m, _)| *m == k) {
                    return None; // not a tail call: real recursion
                }
                let name = self.msl_fn(g, &k)?;
                let spec = self.spec(&k).ok()?;
                let mut vals = Vec::new();
                for (a, t) in args.iter().zip(&spec.params) {
                    vals.push(self.msl_as(g, b, a, *t)?);
                }
                if vals.len() != spec.params.len() {
                    return None;
                }
                conv(format!("{name}({})", vals.join(", ")), spec.ret, ty)?
            }
            _ => return None,
        })
    }

    /// A block's value (its statements written first, in their own scope).
    fn msl_block_value(&self, g: &mut Gen, b: &mut Body, blk: &Block) -> Option<String> {
        let depth = b.vars.len();
        for s in &blk.stmts {
            self.msl_stmt(g, b, s)?;
        }
        let r = self.msl_expr(g, b, blk.tail.as_deref()?);
        b.vars.truncate(depth);
        r
    }
}

/// A 64-bit integer result as type `ty` (wrapped to an i32).
fn wrap(x: String, ty: Ty) -> Option<String> {
    Some(match ty {
        Ty::I64 => x,
        Ty::I32 => format!("((int){x})"),
        _ => return None,
    })
}

/// `x op y` as the CPU computes it (Cg::narrow_binary, scalar_binary).
#[allow(clippy::too_many_arguments)]
fn msl_binary(op: BinOp, x: String, ta: Ty, y: String, tb: Ty, ty: Ty, rhs: &Expr, guarded: bool) -> Option<String> {
    use BinOp::*;
    if matches!(op, And | Or) {
        if ta != Ty::Bool || tb != Ty::Bool {
            return None;
        }
        return Some(format!("({x} {} {y})", if op == And { "&&" } else { "||" }));
    }
    let cmp = match op {
        Eq => Some("=="),
        Ne => Some("!="),
        Lt => Some("<"),
        Le => Some("<="),
        Gt => Some(">"),
        Ge => Some(">="),
        _ => None,
    };
    if ta == Ty::Bool && tb == Ty::Bool {
        return match cmp {
            Some(c @ ("==" | "!=")) => Some(format!("({x} {c} {y})")),
            _ if matches!(op, BitAnd | BitOr | BitXor) => Some(format!("({x} {} {y})", match op {
                BitAnd => "&",
                BitOr => "|",
                _ => "^",
            })),
            _ => None,
        };
    }
    let joint = types::num_join(ta, false, tb, false);
    match joint {
        Ty::F32 => {
            let (x, y) = (conv(x, ta, Ty::F32)?, conv(y, tb, Ty::F32)?);
            if let Some(c) = cmp {
                return Some(format!("({x} {c} {y})"));
            }
            if ty != Ty::F32 {
                return None;
            }
            Some(match op {
                Add => format!("({x} + {y})"),
                Sub => format!("({x} - {y})"),
                Mul => format!("({x} * {y})"),
                Div => format!("({x} / {y})"),
                Max => format!("({x} > {y} ? {x} : {y})"),
                Min => format!("({x} < {y} ? {x} : {y})"),
                _ => return None,
            })
        }
        Ty::I64 | Ty::I32 => {
            let (x, y) = (conv(x, ta, Ty::I64)?, conv(y, tb, Ty::I64)?);
            if let Some(c) = cmp {
                return Some(format!("({x} {c} {y})"));
            }
            let w = |o: &str| format!("((long)((ulong){x} {o} (ulong){y}))");
            let r = match op {
                Add => w("+"),
                Sub => w("-"),
                Mul => w("*"),
                Max => format!("max({x}, {y})"),
                Min => format!("min({x}, {y})"),
                BitAnd => format!("({x} & {y})"),
                BitOr => format!("({x} | {y})"),
                BitXor => format!("({x} ^ {y})"),
                Shl => format!("((long)((ulong){x} << ({y} & 63)))"),
                Shr => format!("({x} >> ({y} & 63))"),
                UShr => format!("((long)((ulong){x} >> ({y} & 63)))"),
                // floored, by a number written in the program that is
                // neither 0 nor -1 (nothing to stop the program for)
                IntDiv | Mod if guarded => format!("{}({x}, {y})", if op == IntDiv { "fdiv" } else { "fmod_" }),
                IntDiv | Mod => {
                    let d = match &rhs.kind {
                        ExprKind::Int(d) => *d,
                        ExprKind::Unary { op: UnOp::Neg, expr } => match expr.kind {
                            ExprKind::Int(d) => -d,
                            _ => return None,
                        },
                        _ => return None,
                    };
                    if d == 0 || d == -1 {
                        return None;
                    }
                    format!("{}({x}, {y})", if op == IntDiv { "fdiv" } else { "fmod_" })
                }
                _ => return None,
            };
            wrap(r, ty)
        }
        _ => None,
    }
}
