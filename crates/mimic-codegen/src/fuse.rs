//! Kernel fusion: a chain of elementwise array operations compiles to ONE
//! loop that computes each element in registers, instead of one pass (and one
//! temporary array) per operation. A reduction at the end (`+/ ×/ ⌈/ ⌊/ ≢`)
//! fuses too, so `xs × xs + 1 ▷ +/` allocates nothing at all.
//!
//! The loop body is a generated `kernel(ctx, lo, hi)` run by the runtime's
//! thread pool with its fixed-size chunks, so results — floating-point sums
//! included — are identical for any number of threads.
//!
//! `let` bindings of array expressions that are only ever used inside other
//! fusible expressions are not materialised either: they are substituted at
//! their uses ("lazy lets"), which is what lets a Monte-Carlo pipeline written
//! as several `let`s run as one pass over no memory.

use super::*;

/// Element type of a numeric vector the kernels can read and write directly.
pub(crate) fn num_el(t: Ty) -> Option<El> {
    match t {
        Ty::Arr(e @ (El::I64 | El::F64 | El::Bool)) => Some(e),
        _ => None,
    }
}

/// Runtime dtype code (`MIM_I64`/`MIM_F64`/`MIM_BOOL`).
fn dtype_of(e: El) -> i32 {
    match e {
        El::F64 => 1,
        El::Bool => 2,
        _ => 0,
    }
}

/// The fused computation of one element, given the loop index.
#[derive(Clone)]
pub(crate) enum Node {
    /// `⍳ n`: the index itself
    Index,
    /// `lo ‥ hi`: `lo + index` (slot holds lo)
    Offset(usize),
    /// a materialised vector read at the index (slot holds its data pointer)
    Leaf(usize, El),
    /// a scalar computed once before the loop (slot holds its value)
    Scalar(usize, Ty),
    Bin(BinOp, Box<Node>, Box<Node>),
    Un(UnOp, Box<Node>),
}

/// Values computed before the loop and handed to the kernel.
#[derive(Default)]
pub(crate) struct Plan<'ctx> {
    slots: Vec<BasicValueEnum<'ctx>>,
    lens: Vec<IntValue<'ctx>>,
    temps: Vec<V<'ctx>>,
}

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Map(El),
    Reduce(Prim, bool), // primitive, float accumulator
}

impl<'ctx> Cg<'ctx, '_> {
    fn lazy_expr(&self, name: &str) -> Option<&Expr> {
        // SAFETY: lazy expressions point into the Program's AST, which outlives codegen
        self.lazies.iter().rev().find(|(n, _)| n == name).map(|(_, e)| unsafe { &**e })
    }

    /// Is `e` an array expression this module can compute element by element?
    pub(crate) fn is_fae(&self, e: &Expr) -> bool {
        if num_el(self.ty_of(e)).is_none() {
            return false;
        }
        match &e.kind {
            ExprKind::Prim { prim: Prim::Iota, args } => self.ty_of(&args[0]) == Ty::I64,
            ExprKind::Prim { prim: Prim::Range, args } => self.ty_of(&args[0]) == Ty::I64 && self.ty_of(&args[1]) == Ty::I64,
            ExprKind::Binary { lhs, rhs, .. } => self.is_operand(lhs) && self.is_operand(rhs),
            ExprKind::Unary { expr, .. } => self.is_operand(expr),
            ExprKind::Path(p) => p.len() == 1 && self.lazy_expr(&p[0]).is_some(),
            _ => false,
        }
    }

    /// An operand of a fused operation: a fusible array, any numeric vector
    /// (read directly), or an unboxed scalar.
    fn is_operand(&self, x: &Expr) -> bool {
        self.is_fae(x) || num_el(self.ty_of(x)).is_some() || self.ty_of(x).is_unboxed()
    }

    /// Worth fusing: an operation over arrays (a lone `⍳ n` or variable is not).
    pub(crate) fn fusible_root(&self, e: &Expr) -> bool {
        matches!(e.kind, ExprKind::Binary { .. } | ExprKind::Unary { .. }) && self.is_fae(e)
    }

    /// Evaluate the parts of a fusible expression that are computed once
    /// (leaves and scalars), producing the per-element computation.
    fn build(&mut self, e: &Expr, plan: &mut Plan<'ctx>) -> R<Node> {
        if let ExprKind::Path(p) = &e.kind
            && let Some(inner) = self.lazy_expr(&p[0])
        {
            let inner: *const Expr = inner;
            // SAFETY: as in lazy_expr
            return self.build(unsafe { &*inner }, plan);
        }
        match &e.kind {
            ExprKind::Prim { prim: Prim::Iota, args } => {
                let n = self.expr(&args[0])?;
                let n = self.to_i64(n)?;
                plan.lens.push(n);
                Ok(Node::Index)
            }
            ExprKind::Prim { prim: Prim::Range, args } => {
                let lo = self.expr(&args[0])?;
                let lo = self.to_i64(lo)?;
                let hi = self.expr(&args[1])?;
                let hi = self.to_i64(hi)?;
                let d = self.b.build_int_sub(hi, lo, "").s()?;
                let neg = self.b.build_int_compare(IntPredicate::SLT, d, self.ctx.i64_type().const_zero(), "").s()?;
                let len = self.b.build_select(neg, self.ctx.i64_type().const_zero(), d, "").s()?.into_int_value();
                plan.lens.push(len);
                plan.slots.push(lo.into());
                Ok(Node::Offset(plan.slots.len() - 1))
            }
            ExprKind::Binary { op, lhs, rhs } => {
                let a = self.operand(lhs, plan)?;
                let b = self.operand(rhs, plan)?;
                Ok(Node::Bin(*op, Box::new(a), Box::new(b)))
            }
            ExprKind::Unary { op, expr } => Ok(Node::Un(*op, Box::new(self.operand(expr, plan)?))),
            _ => Err("internal compiler error: not a fusible expression".into()),
        }
    }

    fn operand(&mut self, x: &Expr, plan: &mut Plan<'ctx>) -> R<Node> {
        if self.is_fae(x) {
            return self.build(x, plan);
        }
        let t = self.ty_of(x);
        if let Some(el) = num_el(t) {
            let v = self.expr(x)?;
            let v = self.boxed(v)?;
            let len = self.call_rt("mim_vec_len", &[self.basic(v).into(), self.i32c(dtype_of(el))])?.unwrap().into_int_value();
            let data = self.call_rt("mim_dataptr", &[self.basic(v).into()])?.unwrap();
            plan.lens.push(len);
            plan.slots.push(data);
            plan.temps.push(v);
            return Ok(Node::Leaf(plan.slots.len() - 1, el));
        }
        let v = self.expr(x)?;
        let v = self.as_ty(v, t, "operand")?;
        plan.slots.push(self.basic(v));
        Ok(Node::Scalar(plan.slots.len() - 1, t))
    }

    /// The common length of every array in the expression (panics on mismatch).
    fn plan_len(&self, plan: &Plan<'ctx>) -> R<IntValue<'ctx>> {
        let n = plan.lens[0];
        for l in &plan.lens[1..] {
            let ne = self.b.build_int_compare(IntPredicate::NE, n, *l, "").s()?;
            self.panic_if(ne, "length mismatch between arrays in an elementwise operation")?;
        }
        Ok(n)
    }

    /// Store the plan's slots into a stack array the kernel reads (8 bytes each).
    fn pack_ctx(&self, plan: &Plan<'ctx>, extra: &[BasicValueEnum<'ctx>]) -> R<PointerValue<'ctx>> {
        let n = (plan.slots.len() + extra.len()) as u32;
        let ty = self.ctx.i64_type().array_type(n.max(1));
        let ctx = self.entry_array_of(ty)?;
        let i64t = self.ctx.i64_type();
        for (i, v) in plan.slots.iter().chain(extra.iter()).enumerate() {
            let slot = unsafe { self.b.build_in_bounds_gep(ty, ctx, &[i64t.const_zero(), i64t.const_int(i as u64, false)], "").s()? };
            let v = if v.is_int_value() && v.into_int_value().get_type().get_bit_width() == 1 {
                self.b.build_int_z_extend(v.into_int_value(), i64t, "").s()?.into()
            } else {
                *v
            };
            self.b.build_store(slot, v).s()?;
        }
        Ok(ctx)
    }

    fn entry_array_of(&self, ty: inkwell::types::ArrayType<'ctx>) -> R<PointerValue<'ctx>> {
        let f = self.cur_fn.unwrap();
        let entry = f.get_first_basic_block().unwrap();
        let tmp = self.ctx.create_builder();
        match entry.get_first_instruction() {
            Some(i) => tmp.position_before(&i),
            None => tmp.position_at_end(entry),
        }
        let a = tmp.build_alloca(ty, "kctx").s()?;
        self.to_generic(&tmp, a)
    }

    /// The per-element value inside a kernel.
    fn eval_node(&self, n: &Node, i: IntValue<'ctx>, slots: &[BasicValueEnum<'ctx>]) -> R<V<'ctx>> {
        Ok(match n {
            Node::Index => V::I(i),
            Node::Offset(s) => V::I(self.b.build_int_add(slots[*s].into_int_value(), i, "").s()?),
            Node::Leaf(s, el) => {
                let base = slots[*s].into_pointer_value();
                match el {
                    El::F64 => {
                        let p = unsafe { self.b.build_in_bounds_gep(self.ctx.f64_type(), base, &[i], "").s()? };
                        V::F(self.b.build_load(self.ctx.f64_type(), p, "").s()?.into_float_value())
                    }
                    El::Bool => {
                        let p = unsafe { self.b.build_in_bounds_gep(self.ctx.i8_type(), base, &[i], "").s()? };
                        let b = self.b.build_load(self.ctx.i8_type(), p, "").s()?.into_int_value();
                        V::B(self.b.build_int_compare(IntPredicate::NE, b, self.ctx.i8_type().const_zero(), "").s()?)
                    }
                    _ => {
                        let p = unsafe { self.b.build_in_bounds_gep(self.ctx.i64_type(), base, &[i], "").s()? };
                        V::I(self.b.build_load(self.ctx.i64_type(), p, "").s()?.into_int_value())
                    }
                }
            }
            Node::Scalar(s, t) => self.val_of(slots[*s], *t, false),
            Node::Bin(op, a, b) => {
                let x = self.eval_node(a, i, slots)?;
                let y = self.eval_node(b, i, slots)?;
                self.scalar_binary(*op, x, y)?
            }
            Node::Un(op, a) => {
                let x = self.eval_node(a, i, slots)?;
                self.scalar_unary(*op, x)?
            }
        })
    }

    /// Generate `void kernel(ptr ctx, i64 lo, i64 hi)`.
    fn emit_kernel(&mut self, root: &Node, plan: &Plan<'ctx>, mode: Mode) -> R<FunctionValue<'ctx>> {
        self.lambdas += 1;
        let i64t = self.ctx.i64_type();
        let ft = self.ctx.void_type().fn_type(&[self.ptr.into(), i64t.into(), i64t.into()], false);
        let f = self.module.add_function(&format!("_mimic_kernel_{}", self.lambdas), ft, Some(Linkage::Internal));
        // slot types in the kernel mirror the caller's values
        let kinds: Vec<BasicTypeEnum<'ctx>> = plan.slots.iter().map(|v| v.get_type()).collect();
        let nslots = kinds.len();
        let root = root.clone();
        self.in_new_fn(f, |cg| {
            let ctxp = f.get_nth_param(0).unwrap().into_pointer_value();
            let lo = f.get_nth_param(1).unwrap().into_int_value();
            let hi = f.get_nth_param(2).unwrap().into_int_value();
            let arr = i64t.array_type((nslots + 3) as u32);
            let load = |cg: &Self, k: usize, t: BasicTypeEnum<'ctx>| -> R<BasicValueEnum<'ctx>> {
                let p = unsafe { cg.b.build_in_bounds_gep(arr, ctxp, &[i64t.const_zero(), i64t.const_int(k as u64, false)], "").s()? };
                if t.is_int_type() && t.into_int_type().get_bit_width() == 1 {
                    let w = cg.b.build_load(i64t, p, "").s()?.into_int_value();
                    return Ok(cg.b.build_int_truncate(w, cg.ctx.bool_type(), "").s()?.into());
                }
                cg.b.build_load(t, p, "").s()
            };
            let mut slots = Vec::new();
            for (k, t) in kinds.iter().enumerate() {
                slots.push(load(cg, k, *t)?);
            }
            let out = load(cg, nslots, cg.ptr.into())?.into_pointer_value();
            let entry = cg.b.get_insert_block().unwrap();
            let body = cg.ctx.append_basic_block(f, "body");
            let done = cg.ctx.append_basic_block(f, "done");
            let empty = cg.b.build_int_compare(IntPredicate::SGE, lo, hi, "").s()?;
            cg.b.build_conditional_branch(empty, done, body).s()?;
            cg.b.position_at_end(body);
            let iphi = cg.b.build_phi(i64t, "i").s()?;
            let (acc_ty, float) = match mode {
                Mode::Reduce(_, fl) => (if fl { cg.ctx.f64_type().as_basic_type_enum() } else { i64t.as_basic_type_enum() }, fl),
                Mode::Map(_) => (i64t.as_basic_type_enum(), false),
            };
            let accphi = cg.b.build_phi(acc_ty, "acc").s()?;
            let i = iphi.as_basic_value().into_int_value();
            let v = cg.eval_node(&root, i, &slots)?;
            let acc = accphi.as_basic_value();
            let next_acc: BasicValueEnum = match mode {
                Mode::Map(el) => {
                    match el {
                        El::F64 => {
                            let p = unsafe { cg.b.build_in_bounds_gep(cg.ctx.f64_type(), out, &[i], "").s()? };
                            cg.b.build_store(p, cg.to_f64(v)?).s()?;
                        }
                        El::Bool => {
                            let p = unsafe { cg.b.build_in_bounds_gep(cg.ctx.i8_type(), out, &[i], "").s()? };
                            let t = cg.truth(v)?;
                            cg.b.build_store(p, cg.b.build_int_z_extend(t, cg.ctx.i8_type(), "").s()?).s()?;
                        }
                        _ => {
                            let p = unsafe { cg.b.build_in_bounds_gep(i64t, out, &[i], "").s()? };
                            cg.b.build_store(p, cg.to_i64(v)?).s()?;
                        }
                    }
                    acc
                }
                Mode::Reduce(prim, _) => cg.combine(prim, float, acc, v)?,
            };
            let i1 = cg.b.build_int_add(i, i64t.const_int(1, false), "").s()?;
            let more = cg.b.build_int_compare(IntPredicate::SLT, i1, hi, "").s()?;
            let latch = cg.b.get_insert_block().unwrap();
            cg.b.build_conditional_branch(more, body, done).s()?;
            iphi.add_incoming(&[(&lo, entry), (&i1, latch)]);
            let ident = cg.identity(mode, float);
            accphi.add_incoming(&[(&ident, entry), (&next_acc, latch)]);
            cg.b.position_at_end(done);
            if let Mode::Reduce(..) = mode {
                // partials[lo / grain] = acc   (an empty chunk keeps the identity)
                let accd = cg.b.build_phi(acc_ty, "accd").s()?;
                accd.add_incoming(&[(&ident, entry), (&next_acc, latch)]);
                let grain = load(cg, nslots + 1, i64t.into())?.into_int_value();
                let k = cg.b.build_int_signed_div(lo, grain, "").s()?;
                let p = unsafe { cg.b.build_in_bounds_gep(acc_ty, out, &[k], "").s()? };
                cg.b.build_store(p, accd.as_basic_value()).s()?;
            }
            cg.b.build_return(None).s()?;
            Ok(())
        })?;
        Ok(f)
    }

    fn identity(&self, mode: Mode, float: bool) -> BasicValueEnum<'ctx> {
        let i64t = self.ctx.i64_type();
        let f64t = self.ctx.f64_type();
        match mode {
            Mode::Map(_) => i64t.const_zero().into(),
            Mode::Reduce(p, _) => match (p, float) {
                (Prim::Product, true) => f64t.const_float(1.0).into(),
                (Prim::Product, false) => i64t.const_int(1, false).into(),
                (Prim::Maximum, true) => f64t.const_float(f64::NEG_INFINITY).into(),
                (Prim::Maximum, false) => i64t.const_int(i64::MIN as u64, true).into(),
                (Prim::Minimum, true) => f64t.const_float(f64::INFINITY).into(),
                (Prim::Minimum, false) => i64t.const_int(i64::MAX as u64, true).into(),
                (_, true) => f64t.const_float(0.0).into(),
                (_, false) => i64t.const_zero().into(),
            },
        }
    }

    /// acc ⊕ v, with the same semantics as the runtime's reductions.
    fn combine(&self, prim: Prim, float: bool, acc: BasicValueEnum<'ctx>, v: V<'ctx>) -> R<BasicValueEnum<'ctx>> {
        let b = &self.b;
        if float {
            let a = acc.into_float_value();
            let x = self.to_f64(v)?;
            return Ok(match prim {
                Prim::Product => b.build_float_mul(a, x, "").s()?.into(),
                Prim::Maximum => {
                    let c = b.build_float_compare(FloatPredicate::OGT, x, a, "").s()?;
                    b.build_select(c, x, a, "").s()?
                }
                Prim::Minimum => {
                    let c = b.build_float_compare(FloatPredicate::OLT, x, a, "").s()?;
                    b.build_select(c, x, a, "").s()?
                }
                _ => b.build_float_add(a, x, "").s()?.into(),
            });
        }
        let a = acc.into_int_value();
        let x = self.to_i64(v)?;
        Ok(match prim {
            Prim::Product => b.build_int_mul(a, x, "").s()?.into(),
            Prim::Maximum => {
                let c = b.build_int_compare(IntPredicate::SGT, x, a, "").s()?;
                b.build_select(c, x, a, "").s()?
            }
            Prim::Minimum => {
                let c = b.build_int_compare(IntPredicate::SLT, x, a, "").s()?;
                b.build_select(c, x, a, "").s()?
            }
            _ => b.build_int_add(a, x, "").s()?.into(),
        })
    }

    fn release_plan(&self, plan: &Plan<'ctx>) -> R<()> {
        for t in &plan.temps {
            self.drop_tmp(*t)?;
        }
        Ok(())
    }

    /// Materialise a fusible array expression with one fused pass.
    pub(crate) fn fused_map(&mut self, e: &Expr) -> R<V<'ctx>> {
        let el = num_el(self.ty_of(e)).ok_or("internal compiler error: fused_map on a non-numeric array")?;
        let mut plan = Plan::default();
        let root = self.build(e, &mut plan)?;
        let n = self.plan_len(&plan)?;
        let out = self.call_rt_ptr("mim_alloc_vec", &[self.i32c(dtype_of(el)), n.into()])?;
        let data = self.call_rt("mim_dataptr", &[self.basic(out).into()])?.unwrap();
        let k = self.emit_kernel(&root, &plan, Mode::Map(el))?;
        let ctx = self.pack_ctx(&plan, &[data, self.ctx.i64_type().const_zero().into()])?;
        self.call_rt("mim_par_for", &[n.into(), self.fn_addr(k).into(), ctx.into()])?;
        self.release_plan(&plan)?;
        Ok(out)
    }

    /// A reduction (`+/ ×/ ⌈/ ⌊/ ≢`) over a fusible expression: no array is built.
    pub(crate) fn fused_reduce(&mut self, prim: Prim, arg: &Expr) -> R<V<'ctx>> {
        let mut plan = Plan::default();
        let root = self.build(arg, &mut plan)?;
        let n = self.plan_len(&plan)?;
        if prim == Prim::Len {
            self.release_plan(&plan)?;
            return Ok(V::I(n));
        }
        let float = num_el(self.ty_of(arg)) == Some(El::F64);
        let i64t = self.ctx.i64_type();
        let acc_ty: BasicTypeEnum = if float { self.ctx.f64_type().into() } else { i64t.into() };
        if matches!(prim, Prim::Maximum | Prim::Minimum) {
            let z = self.b.build_int_compare(IntPredicate::EQ, n, i64t.const_zero(), "").s()?;
            self.panic_if(z, "maximum/minimum of an empty array")?;
        }
        // one partial result per fixed-size chunk, combined in chunk order
        let grain = self.call_rt("mim_par_grain", &[])?.unwrap().into_int_value();
        let g1 = self.b.build_int_sub(grain, i64t.const_int(1, false), "").s()?;
        let nc = self.b.build_int_add(n, g1, "").s()?;
        let nc = self.b.build_int_signed_div(nc, grain, "").s()?;
        let bytes = self.b.build_int_mul(nc, i64t.const_int(8, false), "").s()?;
        let bytes = self.b.build_int_add(bytes, i64t.const_int(8, false), "").s()?;
        let partials = self.call_malloc(bytes)?;
        let k = self.emit_kernel(&root, &plan, Mode::Reduce(prim, float))?;
        let ctx = self.pack_ctx(&plan, &[partials.into(), grain.into()])?;
        self.call_rt("mim_par_for", &[n.into(), self.fn_addr(k).into(), ctx.into()])?;
        self.release_plan(&plan)?;
        // combine the partials in order
        let f = self.cur_fn.unwrap();
        let pre = self.b.get_insert_block().unwrap();
        let body = self.ctx.append_basic_block(f, "comb");
        let done = self.ctx.append_basic_block(f, "comb_done");
        let ident = self.identity(Mode::Reduce(prim, float), float);
        let none = self.b.build_int_compare(IntPredicate::EQ, nc, i64t.const_zero(), "").s()?;
        self.b.build_conditional_branch(none, done, body).s()?;
        self.b.position_at_end(body);
        let kphi = self.b.build_phi(i64t, "k").s()?;
        let aphi = self.b.build_phi(acc_ty, "a").s()?;
        let kk = kphi.as_basic_value().into_int_value();
        let p = unsafe { self.b.build_in_bounds_gep(acc_ty, partials, &[kk], "").s()? };
        let part = self.b.build_load(acc_ty, p, "").s()?;
        let pv = if float { V::F(part.into_float_value()) } else { V::I(part.into_int_value()) };
        let next = self.combine(prim, float, aphi.as_basic_value(), pv)?;
        let k1 = self.b.build_int_add(kk, i64t.const_int(1, false), "").s()?;
        let more = self.b.build_int_compare(IntPredicate::SLT, k1, nc, "").s()?;
        let latch = self.b.get_insert_block().unwrap();
        self.b.build_conditional_branch(more, body, done).s()?;
        kphi.add_incoming(&[(&i64t.const_zero(), pre), (&k1, latch)]);
        aphi.add_incoming(&[(&ident, pre), (&next, latch)]);
        self.b.position_at_end(done);
        let res = self.b.build_phi(acc_ty, "red").s()?;
        res.add_incoming(&[(&ident, pre), (&next, latch)]);
        self.call_free(partials)?;
        let r = res.as_basic_value();
        Ok(if float { V::F(r.into_float_value()) } else { V::I(r.into_int_value()) })
    }

    fn call_malloc(&self, bytes: IntValue<'ctx>) -> R<PointerValue<'ctx>> {
        let f = self.module.get_function("malloc").unwrap_or_else(|| {
            self.module.add_function("malloc", self.ptr.fn_type(&[self.size.into()], false), Some(Linkage::External))
        });
        // the byte count is computed in 64 bits; size_t may be narrower (wasm32)
        let bytes =
            if self.size.get_bit_width() < 64 { self.b.build_int_truncate(bytes, self.size, "").s()? } else { bytes };
        Ok(self.b.build_call(f, &[bytes.into()], "").s()?.try_as_basic_value().basic().unwrap().into_pointer_value())
    }

    fn call_free(&self, p: PointerValue<'ctx>) -> R<()> {
        let f = self.module.get_function("free").unwrap_or_else(|| {
            self.module.add_function("free", self.ctx.void_type().fn_type(&[self.ptr.into()], false), Some(Linkage::External))
        });
        self.b.build_call(f, &[p.into()], "").s()?;
        Ok(())
    }

    // ---- lazy lets ------------------------------------------------------

    /// May `let name = value` stay unmaterialised? Only when `value` is a
    /// fusible expression over immutable inputs (cheap and pure to recompute)
    /// and every later use of `name` sits inside a fused expression.
    pub(crate) fn lazy_candidate(&mut self, name: &str, value: &Expr, rest: &[Stmt], tail: Option<&Expr>) -> bool {
        if !self.is_fae(value) || !self.trivially_pure(value) {
            return false;
        }
        self.lazies.push((name.to_string(), value as *const Expr));
        let ok = rest.iter().all(|s| self.stmt_uses_ok(name, s)) && tail.is_none_or(|t| self.uses_ok(name, t, false));
        self.lazies.pop();
        ok
    }

    fn trivially_pure(&self, e: &Expr) -> bool {
        match &e.kind {
            ExprKind::Int(_) | ExprKind::Float(_) | ExprKind::Bool(_) => true,
            ExprKind::Path(p) => {
                p.len() == 1 && (self.lazy_expr(&p[0]).is_some() || self.lookup(&p[0]).is_some_and(|l| !l.mutable))
            }
            ExprKind::Binary { lhs, rhs, .. } => self.trivially_pure(lhs) && self.trivially_pure(rhs),
            ExprKind::Unary { expr, .. } => self.trivially_pure(expr),
            ExprKind::Prim { prim: Prim::Iota | Prim::Range, args } => args.iter().all(|a| self.trivially_pure(a)),
            _ => false,
        }
    }

    fn stmt_uses_ok(&self, name: &str, s: &Stmt) -> bool {
        match s {
            Stmt::Let { value, .. } | Stmt::Expr(value) | Stmt::LetTuple { value, .. } => self.uses_ok(name, value, false),
            Stmt::Assign { name: n, index, value, .. } => n != name && index.as_ref().is_none_or(|i| self.uses_ok(name, i, false)) && self.uses_ok(name, value, false),
            Stmt::Return(e, _) => e.as_ref().is_none_or(|e| self.uses_ok(name, e, false)),
            Stmt::LetElse { value, els, .. } => self.uses_ok(name, value, false) && self.block_uses_ok(name, els),
        }
    }

    fn block_uses_ok(&self, name: &str, b: &Block) -> bool {
        b.stmts.iter().all(|s| self.stmt_uses_ok(name, s)) && b.tail.as_ref().is_none_or(|t| self.uses_ok(name, t, false))
    }

    /// Does every occurrence of `name` inside `e` sit where fusion substitutes it?
    fn uses_ok(&self, name: &str, e: &Expr, fused_pos: bool) -> bool {
        match &e.kind {
            ExprKind::Path(p) => !(p.len() == 1 && p[0] == name) || fused_pos,
            ExprKind::Binary { lhs, rhs, .. } => {
                let f = self.is_fae(e);
                self.uses_ok(name, lhs, f) && self.uses_ok(name, rhs, f)
            }
            ExprKind::Unary { expr, .. } => self.uses_ok(name, expr, self.is_fae(e)),
            ExprKind::Prim { prim: Prim::Sum | Prim::Product | Prim::Maximum | Prim::Minimum | Prim::Len, args } => {
                self.uses_ok(name, &args[0], self.is_fae(&args[0]))
            }
            ExprKind::Prim { args, .. } | ExprKind::Array(args) | ExprKind::Tuple(args) => args.iter().all(|a| self.uses_ok(name, a, false)),
            ExprKind::Call { callee, args } => self.uses_ok(name, callee, false) && args.iter().all(|a| self.uses_ok(name, a, false)),
            ExprKind::Index { base, index } => self.uses_ok(name, base, false) && self.uses_ok(name, index, false),
            ExprKind::If { cond, then, els } => {
                self.uses_ok(name, cond, false) && self.block_uses_ok(name, then) && els.as_ref().is_none_or(|x| self.uses_ok(name, x, false))
            }
            ExprKind::Block(b) | ExprKind::Unsafe(b) => self.block_uses_ok(name, b),
            ExprKind::Field { base, .. } => self.uses_ok(name, base, false),
            ExprKind::Record { fields, .. } => fields.iter().all(|(_, v, _)| self.uses_ok(name, v, false)),
            ExprKind::With { base, fields } => self.uses_ok(name, base, false) && fields.iter().all(|(_, v, _)| self.uses_ok(name, v, false)),
            ExprKind::Match { scrut, arms } => {
                self.uses_ok(name, scrut, false)
                    && arms.iter().all(|a| a.guard.as_ref().is_none_or(|g| self.uses_ok(name, g, false)) && self.uses_ok(name, &a.body, false))
            }
            ExprKind::Interp(parts) => parts.iter().all(|p| match p {
                InterpPart::Expr(x) => self.uses_ok(name, x, false),
                InterpPart::Text(_) => true,
            }),
            // a lambda capturing `name` needs it as a real variable (unless a
            // parameter of the same name hides it)
            ExprKind::Lambda { params, body } => params.iter().any(|p| p == name) || self.uses_ok(name, body, false),
            ExprKind::Map(kvs) => kvs.iter().all(|(k, v)| self.uses_ok(name, k, false) && self.uses_ok(name, v, false)),
            _ => true,
        }
    }
}
