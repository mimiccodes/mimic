//! `xs ¨ f` several elements at a time (SIMD across elements, as ISPC does).
//!
//! A pure function on numbers and truth values (the same kind gpufn.rs puts
//! on the GPU, here with f64 too) gets a *vector version*: it takes lanes()
//! elements at once, in the lanes of vectors, and computes them together.
//!
//! - Every value is a vector of lanes() lanes; arithmetic is the same as the
//!   scalar code's (scalar_binary), lane by lane.
//! - `if` computes both sides for every lane and picks per lane (the code is
//!   pure and nothing in it can fail, so computing a side a lane does not
//!   take is harmless); only calls look at the condition: a call gets the
//!   lanes that take it as a mask, and the other lanes don't run its loop.
//! - A function calling itself in tail position is a loop: each lane either
//!   finishes (its result kept) or goes round again with new parameters,
//!   and the loop runs while any lane goes round.
//!
//! The kernel then runs lanes() elements at a time through the vector version and
//! the rest one by one through the scalar function, so results are those of
//! the scalar code.

use super::*;
use inkwell::types::VectorType;
use inkwell::values::VectorValue;

/// Lanes per vector: 8 (several vector registers each: their chains of
/// operations overlap; mandelbrot on one core 2.65 s scalar, 1.75 s with 8
/// lanes, 2.38 s with 4, 1.59 s with 16 at twice the code).
pub(crate) fn lanes() -> u32 {
    8
}

type VV<'ctx> = VectorValue<'ctx>;

thread_local! {
    /// Vector versions being written (recursion other than a tail call to
    /// itself has none) and ones that could not be made, by module and name.
    static BUSY: std::cell::RefCell<std::collections::HashSet<(String, String)>> = Default::default();
    static NONE: std::cell::RefCell<std::collections::HashSet<(String, String)>> = Default::default();
    /// The lambda being written: its captures' names, and the ones it
    /// divides by (checked once by the kernel: 0 or -1 runs it one by one)
    static CAPS: std::cell::RefCell<(Vec<String>, Vec<usize>)> = Default::default();
}

/// A function's body: a lambda's expression or a function's block.
#[derive(Clone, Copy)]
enum Body<'a> {
    Expr(&'a Expr),
    Block(&'a Block),
}

/// The loop state of the function being translated.
struct Loop<'ctx> {
    me: Option<Key>,
    /// the parameters for the next round (per lane: new values where the lane
    /// called itself, else as they were)
    next: Vec<VV<'ctx>>,
    /// lanes going round again
    cont: VV<'ctx>,
    /// lanes' results so far
    res: VV<'ctx>,
    ret: Ty,
}

impl<'ctx> Cg<'ctx, '_> {
    fn vty(&self, t: Ty) -> Option<VectorType<'ctx>> {
        Some(match t {
            Ty::F64 => self.ctx.f64_type().vec_type(lanes()),
            Ty::F32 => self.ctx.f32_type().vec_type(lanes()),
            Ty::I64 => self.ctx.i64_type().vec_type(lanes()),
            Ty::I32 => self.ctx.i32_type().vec_type(lanes()),
            Ty::Bool => self.ctx.bool_type().vec_type(lanes()),
            _ => return None,
        })
    }

    fn splat(&self, v: BasicValueEnum<'ctx>) -> R<VV<'ctx>> {
        let vt = match v {
            BasicValueEnum::FloatValue(f) => f.get_type().vec_type(lanes()),
            BasicValueEnum::IntValue(i) => i.get_type().vec_type(lanes()),
            _ => return Err("internal compiler error: splat".into()),
        };
        let mut x = vt.get_undef();
        for k in 0..lanes() {
            x = self.b.build_insert_element(x, v, self.ctx.i32_type().const_int(k as u64, false), "").s()?;
        }
        Ok(x)
    }

    fn mask_const(&self, on: bool) -> VV<'ctx> {
        let b = self.ctx.bool_type().const_int(on as u64, false);
        VectorType::const_vector(&vec![b; lanes() as usize])
    }

    fn any(&self, m: VV<'ctx>) -> R<IntValue<'ctx>> {
        let it = self.ctx.custom_width_int_type(std::num::NonZeroU32::new(lanes()).unwrap()).map_err(|e| e.to_string())?;
        let bits = self.b.build_bit_cast(m, it, "").s()?.into_int_value();
        self.b.build_int_compare(IntPredicate::NE, bits, bits.get_type().const_zero(), "").s()
    }

    fn sel(&self, m: VV<'ctx>, a: VV<'ctx>, b: VV<'ctx>) -> R<VV<'ctx>> {
        Ok(self.b.build_select(m, a, b, "").s()?.into_vector_value())
    }

    fn flags(&self, v: VV<'ctx>) -> VV<'ctx> {
        use inkwell::values::FastMathFlags as F;
        if self.contract
            && let Some(i) = inkwell::values::BasicValue::as_instruction_value(&v)
        {
            let flags = if self.fast { F::AllowContract | F::AllowReassoc | F::NoSignedZeros | F::AllowReciprocal | F::ApproxFunc } else { F::AllowContract };
            let _ = i.set_fast_math_flags(flags);
        }
        v
    }

    /// A vector of type `from` as type `to`, where the scalar code converts
    /// the same way and nothing can fail.
    fn vconv(&self, x: VV<'ctx>, from: Ty, to: Ty) -> Option<VV<'ctx>> {
        let b = &self.b;
        let f64v = self.ctx.f64_type().vec_type(lanes());
        let i64v = self.ctx.i64_type().vec_type(lanes());
        Some(match (from, to) {
            _ if from == to => x,
            (Ty::I64 | Ty::I32, Ty::F64) => b.build_signed_int_to_float(x, f64v, "").ok()?,
            (Ty::Bool, Ty::F64) => b.build_unsigned_int_to_float(x, f64v, "").ok()?,
            (Ty::Bool, Ty::I64) => b.build_int_z_extend(x, i64v, "").ok()?,
            (Ty::I32, Ty::I64) => b.build_int_s_extend(x, i64v, "").ok()?,
            (Ty::F32, Ty::F64) => b.build_float_ext(x, f64v, "").ok()?,
            (Ty::F64, Ty::F32) => b.build_float_trunc(x, self.ctx.f32_type().vec_type(lanes()), "").ok()?,
            (Ty::I32 | Ty::I64 | Ty::Bool, Ty::F32) => {
                let d = self.vconv(x, from, Ty::F64)?;
                b.build_float_trunc(d, self.ctx.f32_type().vec_type(lanes()), "").ok()?
            }
            (Ty::Bool, Ty::I32) => b.build_int_z_extend(x, self.ctx.i32_type().vec_type(lanes()), "").ok()?,
            _ => return None,
        })
    }

    /// The vector version of the specialisation `k` (cached):
    /// `<lanes() x R> f(<lanes() x P>…, <lanes() x i1> lanes)`.
    pub(crate) fn simd_named(&mut self, k: &Key, depth: usize) -> Option<FunctionValue<'ctx>> {
        let name = format!("_mimic_simd_{:016x}", mimic_syntax::fnv1a(format!("{k:?}").as_bytes()));
        let key = (self.module.get_name().to_string_lossy().into_owned(), name.clone());
        if BUSY.with(|b| b.borrow().contains(&key)) || NONE.with(|n| n.borrow().contains(&key)) || depth > 8 {
            return None;
        }
        if let Some(f) = self.module.get_function(&name) {
            return Some(f);
        }
        let FnKind::User(d) = &self.p.fns[k.0].kind else { return None };
        if !self.purity.fns[k.0] || d.export_c {
            return None;
        }
        let spec = self.spec(k).ok()?;
        let mut pts = Vec::new();
        for t in &spec.params {
            pts.push(self.vty(*t)?.into());
        }
        pts.push(self.ctx.bool_type().vec_type(lanes()).into());
        let rt = self.vty(spec.ret)?;
        let f = self.module.add_function(&name, rt.fn_type(&pts, false), Some(Linkage::Internal));
        let names: Vec<(String, Ty)> = d.params.iter().zip(&spec.params).map(|(p, t)| (p.name.clone(), *t)).collect();
        BUSY.with(|b| b.borrow_mut().insert(key.clone()));
        let ok = self.simd_body(f, &spec, &names, &[], spec.ret, Some(k.clone()), Body::Block(&d.body), depth);
        BUSY.with(|b| b.borrow_mut().remove(&key));
        if !ok {
            unsafe { f.delete() };
            NONE.with(|n| n.borrow_mut().insert(key));
            return None;
        }
        Some(f)
    }

    /// The vector version of a lambda of one parameter:
    /// `<lanes() x R> f(<lanes() x P>, captures…, <lanes() x i1> lanes)` (captures as they are).
    /// (With the captures it divides by: see CAPS.)
    pub(crate) fn simd_lambda(&mut self, lambda: &Expr, caps: &[(String, Ty)]) -> Option<(FunctionValue<'ctx>, Vec<usize>)> {
        let ExprKind::Lambda { params, body } = &lambda.kind else { return None };
        let spec = self.cur_spec.clone()?;
        let (lpts, rt) = spec.lambdas.get(&id(lambda)).cloned()?;
        if params.len() != 1 || lpts.len() != 1 {
            return None;
        }
        let mut pts: Vec<BasicMetadataTypeEnum> = vec![self.vty(lpts[0])?.into()];
        for (_, t) in caps {
            if !matches!(t, Ty::F64 | Ty::F32 | Ty::I64 | Ty::I32 | Ty::Bool) {
                return None;
            }
            pts.push(self.repr(*t).into());
        }
        pts.push(self.ctx.bool_type().vec_type(lanes()).into());
        self.lambdas += 1;
        let f = self.module.add_function(&format!("_mimic_simd_lambda_{}", self.lambdas), self.vty(rt)?.fn_type(&pts, false), Some(Linkage::Internal));
        let names = vec![(params[0].clone(), lpts[0])];
        CAPS.with(|c| *c.borrow_mut() = (caps.iter().map(|(n, _)| n.clone()).collect(), Vec::new()));
        let ok = self.simd_body(f, &spec, &names, caps, rt, None, Body::Expr(body), 0);
        let guards = CAPS.with(|c| std::mem::take(&mut *c.borrow_mut())).1;
        if ok {
            Some((f, guards))
        } else {
            unsafe { f.delete() };
            None
        }
    }

    /// Write the body of a vector function: a loop over rounds (see the
    /// module comment). False when some part has no vector form.
    #[allow(clippy::too_many_arguments)]
    fn simd_body(&mut self, f: FunctionValue<'ctx>, spec: &Spec, params: &[(String, Ty)], caps: &[(String, Ty)], ret: Ty, me: Option<Key>, body: Body, depth: usize) -> bool {
        let r = self.in_new_fn(f, |cg| {
            let rvt = cg.vty(ret).ok_or("")?;
            let np = params.len();
            let mask_in = f.get_nth_param((np + caps.len()) as u32).unwrap().into_vector_value();
            // captures: the same in every lane
            let mut vars: Vec<(String, VV<'ctx>, Ty)> = Vec::new();
            for (i, (n, t)) in caps.iter().enumerate() {
                let v = cg.splat(f.get_nth_param((np + i) as u32).unwrap())?;
                vars.push((n.clone(), v, *t));
            }
            let entry = cg.b.get_insert_block().unwrap();
            let lp = cg.ctx.append_basic_block(f, "round");
            let exit = cg.ctx.append_basic_block(f, "done");
            cg.b.build_unconditional_branch(lp).s()?;
            cg.b.position_at_end(lp);
            let mut phis = Vec::new();
            for (i, (n, t)) in params.iter().enumerate() {
                let phi = cg.b.build_phi(cg.vty(*t).ok_or("")?, n).s()?;
                phi.add_incoming(&[(&f.get_nth_param(i as u32).unwrap(), entry)]);
                vars.push((n.clone(), phi.as_basic_value().into_vector_value(), *t));
                phis.push(phi);
            }
            let act = cg.b.build_phi(cg.ctx.bool_type().vec_type(lanes()), "lanes").s()?;
            act.add_incoming(&[(&mask_in, entry)]);
            let res = cg.b.build_phi(rvt, "res").s()?;
            res.add_incoming(&[(&rvt.const_zero(), entry)]);
            let mut lp_state = Loop {
                me,
                next: phis.iter().map(|p| p.as_basic_value().into_vector_value()).collect(),
                cont: cg.mask_const(false),
                res: res.as_basic_value().into_vector_value(),
                ret,
            };
            let a = act.as_basic_value().into_vector_value();
            let ok = match body {
                Body::Expr(e) => cg.simd_tail(spec, &mut vars, &mut lp_state, e, a, depth),
                Body::Block(b) => cg.simd_tail_block(spec, &mut vars, &mut lp_state, b, a, depth),
            };
            if !ok {
                return Err("no vector form".into());
            }
            let latch = cg.b.get_insert_block().unwrap();
            for (phi, v) in phis.iter().zip(&lp_state.next) {
                phi.add_incoming(&[(v, latch)]);
            }
            act.add_incoming(&[(&lp_state.cont, latch)]);
            res.add_incoming(&[(&lp_state.res, latch)]);
            let more = cg.any(lp_state.cont)?;
            cg.b.build_conditional_branch(more, lp, exit).s()?;
            cg.b.position_at_end(exit);
            cg.b.build_return(Some(&lp_state.res)).s()?;
            Ok(())
        });
        r.is_ok()
    }

    /// `e` is what the function returns, for the lanes in `m`.
    fn simd_tail(&mut self, spec: &Spec, vars: &mut Vec<(String, VV<'ctx>, Ty)>, lp: &mut Loop<'ctx>, e: &Expr, m: VV<'ctx>, depth: usize) -> bool {
        let ty = |x: &Expr| spec.exprs.get(&id(x)).copied().unwrap_or(Ty::Dyn);
        match &e.kind {
            ExprKind::If { cond, then, els } => {
                let Some(els) = els.as_deref() else { return false };
                let Some(c) = self.simd_expr(spec, vars, cond, m, depth) else { return false };
                if ty(cond) != Ty::Bool {
                    return false;
                }
                let (Ok(yes), Ok(nc)) = (self.b.build_and(m, c, ""), self.b.build_not(c, "")) else { return false };
                let Ok(no) = self.b.build_and(m, nc, "") else { return false };
                self.simd_tail_block(spec, vars, lp, then, yes, depth) && self.simd_tail(spec, vars, lp, els, no, depth)
            }
            ExprKind::Block(b) => self.simd_tail_block(spec, vars, lp, b, m, depth),
            ExprKind::Call { args, .. } if lp.me.is_some() && spec.calls.get(&id(e)) == lp.me.as_ref() => {
                let Some(me) = lp.me.clone() else { return false };
                let Ok(sp) = self.spec(&me) else { return false };
                if args.len() != sp.params.len() {
                    return false;
                }
                for (i, (a, t)) in args.iter().zip(&sp.params).enumerate() {
                    let Some(v) = self.simd_expr(spec, vars, a, m, depth) else { return false };
                    let Some(v) = self.vconv(v, ty(a), *t) else { return false };
                    let Ok(n) = self.sel(m, v, lp.next[i]) else { return false };
                    lp.next[i] = n;
                }
                let Ok(c) = self.b.build_or(lp.cont, m, "") else { return false };
                lp.cont = c;
                true
            }
            _ => {
                let Some(v) = self.simd_expr(spec, vars, e, m, depth) else { return false };
                let Some(v) = self.vconv(v, ty(e), lp.ret) else { return false };
                let Ok(r) = self.sel(m, v, lp.res) else { return false };
                lp.res = r;
                true
            }
        }
    }

    fn simd_tail_block(&mut self, spec: &Spec, vars: &mut Vec<(String, VV<'ctx>, Ty)>, lp: &mut Loop<'ctx>, b: &Block, m: VV<'ctx>, depth: usize) -> bool {
        let ty = |x: &Expr| spec.exprs.get(&id(x)).copied().unwrap_or(Ty::Dyn);
        let mark = vars.len();
        let mut ok = true;
        for s in &b.stmts {
            let Stmt::Let { name, mutable: false, value, .. } = s else {
                ok = false;
                break;
            };
            let Some(t) = spec.lets.get(&id(s)).copied() else {
                ok = false;
                break;
            };
            let Some(v) = self.simd_expr(spec, vars, value, m, depth).and_then(|v| self.vconv(v, ty(value), t)) else {
                ok = false;
                break;
            };
            vars.push((name.clone(), v, t));
        }
        ok = ok && b.tail.as_deref().is_some_and(|t| self.simd_tail(spec, vars, lp, t, m, depth));
        vars.truncate(mark);
        ok
    }

    /// The value of `e` in every lane (lanes outside `m` may hold anything).
    fn simd_expr(&mut self, spec: &Spec, vars: &mut Vec<(String, VV<'ctx>, Ty)>, e: &Expr, m: VV<'ctx>, depth: usize) -> Option<VV<'ctx>> {
        let ty = |x: &Expr| spec.exprs.get(&id(x)).copied().unwrap_or(Ty::Dyn);
        let t = ty(e);
        let vt = self.vty(t)?;
        Some(match &e.kind {
            ExprKind::Int(v) => match t {
                Ty::F64 | Ty::F32 => self.splat(vt.get_element_type().into_float_type().const_float(*v as f64).into()).ok()?,
                Ty::I32 if *v == *v as i32 as i64 => self.splat(self.ctx.i32_type().const_int(*v as u64, true).into()).ok()?,
                Ty::I64 => self.splat(self.ctx.i64_type().const_int(*v as u64, true).into()).ok()?,
                _ => return None,
            },
            ExprKind::Float(v) => match t {
                Ty::F64 => self.splat(self.ctx.f64_type().const_float(*v).into()).ok()?,
                Ty::F32 => self.splat(self.ctx.f32_type().const_float(*v as f32 as f64).into()).ok()?,
                _ => return None,
            },
            ExprKind::Bool(v) => self.mask_const(*v),
            ExprKind::Path(p) if p.len() == 1 => vars.iter().rev().find(|(n, ..)| *n == p[0]).map(|(_, v, _)| *v)?,
            ExprKind::Unary { op, expr } => {
                let tx = ty(expr);
                let x = self.simd_expr(spec, vars, expr, m, depth)?;
                let b = &self.b;
                match (op, tx) {
                    (UnOp::Not, Ty::Bool) => b.build_not(x, "").ok()?,
                    (UnOp::Neg, Ty::F64 | Ty::F32) => b.build_float_neg(x, "").ok()?,
                    (UnOp::Abs, Ty::F64 | Ty::F32) => {
                        let fabs = Intrinsic::find("llvm.fabs")?.get_declaration(&self.module, &[x.get_type().into()])?;
                        b.build_call(fabs, &[x.into()], "").ok()?.try_as_basic_value().basic()?.into_vector_value()
                    }
                    (UnOp::Neg, Ty::I64 | Ty::I32) => b.build_int_neg(x, "").ok()?,
                    (UnOp::Abs, Ty::I64 | Ty::I32) => {
                        let neg = b.build_int_neg(x, "").ok()?;
                        let lt = b.build_int_compare(IntPredicate::SLT, x, x.get_type().const_zero(), "").ok()?;
                        self.sel(lt, neg, x).ok()?
                    }
                    (UnOp::BitNot, Ty::I64 | Ty::I32) => b.build_not(x, "").ok()?,
                    _ => return None,
                }
            }
            ExprKind::Binary { op, lhs, rhs } => {
                let (ta, tb) = (ty(lhs), ty(rhs));
                let x = self.simd_expr(spec, vars, lhs, m, depth)?;
                let y = self.simd_expr(spec, vars, rhs, m, depth)?;
                // dividing by a captured value (the same in every lane)
                let mut guarded = false;
                if matches!(op, BinOp::IntDiv | BinOp::Mod)
                    && depth == 0
                    && let ExprKind::Path(p) = &rhs.kind
                    && p.len() == 1
                    && let Some(pos) = vars.iter().rposition(|(n, ..)| *n == p[0])
                {
                    CAPS.with(|c| {
                        let mut c = c.borrow_mut();
                        if pos < c.0.len() && c.0[pos] == p[0] {
                            if !c.1.contains(&pos) {
                                c.1.push(pos);
                            }
                            guarded = true;
                        }
                    });
                }
                self.simd_binary(*op, x, ta, y, tb, t, rhs, guarded)?
            }
            ExprKind::If { cond, then, els } => {
                if ty(cond) != Ty::Bool {
                    return None;
                }
                let c = self.simd_expr(spec, vars, cond, m, depth)?;
                let yes = self.b.build_and(m, c, "").ok()?;
                let nc = self.b.build_not(c, "").ok()?;
                let no = self.b.build_and(m, nc, "").ok()?;
                let (x, tx) = self.simd_block(spec, vars, then, yes, depth)?;
                let x = self.vconv(x, tx, t)?;
                let els = els.as_deref()?;
                let y = self.simd_expr(spec, vars, els, no, depth)?;
                let y = self.vconv(y, ty(els), t)?;
                self.sel(c, x, y).ok()?
            }
            ExprKind::Block(blk) => {
                let (r, tr) = self.simd_block(spec, vars, blk, m, depth)?;
                self.vconv(r, tr, t)?
            }
            ExprKind::Call { callee, args } => {
                let ExprKind::Path(_) = &callee.kind else { return None };
                let k = spec.calls.get(&id(e)).cloned()?;
                let sp = self.spec(&k).ok()?;
                if args.len() != sp.params.len() {
                    return None;
                }
                let mut vals: Vec<BasicMetadataValueEnum> = Vec::new();
                for (a, pt) in args.iter().zip(&sp.params) {
                    let v = self.simd_expr(spec, vars, a, m, depth)?;
                    vals.push(self.vconv(v, ty(a), *pt)?.into());
                }
                let g = self.simd_named(&k, depth + 1)?;
                vals.push(m.into());
                let r = self.b.build_call(g, &vals, "").ok()?.try_as_basic_value().basic()?.into_vector_value();
                self.vconv(r, sp.ret, t)?
            }
            _ => return None,
        })
    }

    /// A block's value and its type (its tail's).
    fn simd_block(&mut self, spec: &Spec, vars: &mut Vec<(String, VV<'ctx>, Ty)>, blk: &Block, m: VV<'ctx>, depth: usize) -> Option<(VV<'ctx>, Ty)> {
        let ty = |x: &Expr| spec.exprs.get(&id(x)).copied().unwrap_or(Ty::Dyn);
        let mark = vars.len();
        let mut r = None;
        let mut ok = true;
        for s in &blk.stmts {
            let Stmt::Let { name, mutable: false, value, .. } = s else {
                ok = false;
                break;
            };
            let Some(lt) = spec.lets.get(&id(s)).copied() else {
                ok = false;
                break;
            };
            let Some(v) = self.simd_expr(spec, vars, value, m, depth).and_then(|v| self.vconv(v, ty(value), lt)) else {
                ok = false;
                break;
            };
            vars.push((name.clone(), v, lt));
        }
        if ok && let Some(tail) = blk.tail.as_deref() {
            r = self.simd_expr(spec, vars, tail, m, depth).map(|v| (v, ty(tail)));
        }
        vars.truncate(mark);
        r
    }

    /// `x op y` lane by lane, as Cg::narrow_binary / scalar_binary compute it.
    #[allow(clippy::too_many_arguments)]
    fn simd_binary(&self, op: BinOp, x: VV<'ctx>, ta: Ty, y: VV<'ctx>, tb: Ty, ty: Ty, rhs: &Expr, guarded: bool) -> Option<VV<'ctx>> {
        use BinOp::*;
        let b = &self.b;
        if matches!(op, And | Or) {
            if ta != Ty::Bool || tb != Ty::Bool {
                return None;
            }
            return (if op == And { b.build_and(x, y, "") } else { b.build_or(x, y, "") }).ok();
        }
        if ta == Ty::Bool && tb == Ty::Bool {
            return match op {
                Eq => b.build_int_compare(IntPredicate::EQ, x, y, "").ok(),
                Ne => b.build_int_compare(IntPredicate::NE, x, y, "").ok(),
                BitAnd => b.build_and(x, y, "").ok(),
                BitOr => b.build_or(x, y, "").ok(),
                BitXor => b.build_xor(x, y, "").ok(),
                _ => None,
            };
        }
        // f32 and i32: their operands made the joint type first, computed
        // wide, narrowed (as narrow_binary)
        let joint = types::num_join(ta, false, tb, false);
        let (x, ta, y, tb) = if matches!(joint, Ty::F32 | Ty::I32) {
            (self.vconv(x, ta, joint)?, joint, self.vconv(y, tb, joint)?, joint)
        } else {
            (x, ta, y, tb)
        };
        let is_f = |t: Ty| matches!(t, Ty::F64 | Ty::F32);
        let float = is_f(ta) || is_f(tb) || op == Div;
        let r = if float {
            let x = self.vconv(x, ta, Ty::F64)?;
            let y = self.vconv(y, tb, Ty::F64)?;
            let cmp = |p: FloatPredicate| b.build_float_compare(p, x, y, "").ok();
            match op {
                Add => self.flags(b.build_float_add(x, y, "").ok()?),
                Sub => self.flags(b.build_float_sub(x, y, "").ok()?),
                Mul => self.flags(b.build_float_mul(x, y, "").ok()?),
                Div => self.flags(b.build_float_div(x, y, "").ok()?),
                Max => self.sel(cmp(FloatPredicate::OGT)?, x, y).ok()?,
                Min => self.sel(cmp(FloatPredicate::OLT)?, x, y).ok()?,
                Eq => return cmp(FloatPredicate::OEQ),
                Ne => return cmp(FloatPredicate::UNE),
                Lt => return cmp(FloatPredicate::OLT),
                Le => return cmp(FloatPredicate::OLE),
                Gt => return cmp(FloatPredicate::OGT),
                Ge => return cmp(FloatPredicate::OGE),
                _ => return None,
            }
        } else {
            let x = self.vconv(x, ta, Ty::I64)?;
            let y = self.vconv(y, tb, Ty::I64)?;
            let cmp = |p: IntPredicate| b.build_int_compare(p, x, y, "").ok();
            let i64v = self.ctx.i64_type().vec_type(lanes());
            match op {
                Add => b.build_int_add(x, y, "").ok()?,
                Sub => b.build_int_sub(x, y, "").ok()?,
                Mul => b.build_int_mul(x, y, "").ok()?,
                Max => self.sel(cmp(IntPredicate::SGT)?, x, y).ok()?,
                Min => self.sel(cmp(IntPredicate::SLT)?, x, y).ok()?,
                Eq => return cmp(IntPredicate::EQ),
                Ne => return cmp(IntPredicate::NE),
                Lt => return cmp(IntPredicate::SLT),
                Le => return cmp(IntPredicate::SLE),
                Gt => return cmp(IntPredicate::SGT),
                Ge => return cmp(IntPredicate::SGE),
                BitAnd => b.build_and(x, y, "").ok()?,
                BitOr => b.build_or(x, y, "").ok()?,
                BitXor => b.build_xor(x, y, "").ok()?,
                Shl | Shr | UShr => {
                    let k = b.build_and(y, VectorType::const_vector(&vec![self.ctx.i64_type().const_int(63, false); lanes() as usize]), "").ok()?;
                    match op {
                        Shl => b.build_left_shift(x, k, "").ok()?,
                        Shr => b.build_right_shift(x, k, true, "").ok()?,
                        _ => b.build_right_shift(x, k, false, "").ok()?,
                    }
                }
                // floored, by a number written in the program (neither 0
                // nor -1: nothing to stop the program for)
                IntDiv | Mod => {
                    let d = match &rhs.kind {
                        _ if guarded => 2,
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
                    let zero = i64v.const_zero();
                    let q = b.build_int_signed_div(x, y, "").ok()?;
                    let r = b.build_int_signed_rem(x, y, "").ok()?;
                    let nz = b.build_int_compare(IntPredicate::NE, r, zero, "").ok()?;
                    let sx = b.build_xor(r, y, "").ok()?;
                    let neg = b.build_int_compare(IntPredicate::SLT, sx, zero, "").ok()?;
                    let adj = b.build_and(nz, neg, "").ok()?;
                    if op == IntDiv {
                        let a = b.build_int_z_extend(adj, i64v, "").ok()?;
                        b.build_int_sub(q, a, "").ok()?
                    } else {
                        let ry = b.build_int_add(r, y, "").ok()?;
                        self.sel(adj, ry, r).ok()?
                    }
                }
                _ => return None,
            }
        };
        // the result as the expression's type (f32 rounded, i32 wrapped)
        let rt = if float { Ty::F64 } else { Ty::I64 };
        match (rt, ty) {
            _ if rt == ty => Some(r),
            (Ty::I64, Ty::I32) => b.build_int_truncate(r, self.ctx.i32_type().vec_type(lanes()), "").ok(),
            (Ty::F64, Ty::F32) => b.build_float_trunc(r, self.ctx.f32_type().vec_type(lanes()), "").ok(),
            _ => None,
        }
    }
}

