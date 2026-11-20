//! Fork-join: independent calls run in parallel, with nothing to declare.
//!
//! A *fork site* is an expression with two or more calls among the operands
//! it always evaluates (`fib(n - 1) + fib(n - 2)`, `Node(make(d - 1),
//! make(d - 1))`, `1 + check(l) + check(r)`), or a run of `let`s bound to
//! such calls, where each call is to a pure function that can recurse (any
//! amount of work) and its arguments have no effects. The calls may then run
//! in any order, at the same time.
//!
//! Each function is compiled in up to three versions:
//!
//! - the ordinary one (`Plain`/`Native`): at a fork site it may open a
//!   fork-join region (`mim_fj_open`, a hint; `mim_fj_enter` decides), and
//!   then runs the calls in parallel as fork versions; else it runs them as
//!   sequential versions;
//! - the fork version (`Fork`, with a hidden fork depth): while the depth is
//!   below `mim_fj_cut` it hands all but the last call of a site to the
//!   runtime's work-stealing deque, runs the last itself and joins the
//!   others; deeper, it runs them as sequential versions;
//! - the sequential version (`Seq`/`NativeSeq`): the ordinary code, without
//!   even the check at the top. Recursion below the cut-off runs there at
//!   full single-thread speed.

use super::*;
use std::collections::HashSet;

#[derive(Clone, Copy, Default)]
pub(crate) enum Mode<'ctx> {
    /// may open a region
    #[default]
    Root,
    /// never forks
    Seq,
    /// inside a region, at this fork depth
    Fork(IntValue<'ctx>),
}

#[derive(Default)]
pub(crate) struct State<'ctx> {
    pub mode: Mode<'ctx>,
    /// the function whose fork sites are known (the one being defined)
    site_fn: Option<FunctionValue<'ctx>>,
    /// fork sites: expression → the calls that run in parallel
    sites: HashMap<usize, Vec<*const Expr>>,
    /// expressions inside lambdas (compiled elsewhere, or inlined in loops)
    in_lambda: HashSet<usize>,
    /// the function is a loop to LLVM: its own calls stay where they are
    self_loops: bool,
    /// call results computed ahead (taken by `expr`)
    pub pre: HashMap<usize, V<'ctx>>,
    /// the version (and fork depth) a call must use
    pub kinds: HashMap<usize, (Kind, Option<IntValue<'ctx>>)>,
    /// `void task(ctx)` per callee: runs its fork version
    tasks: HashMap<Key, FunctionValue<'ctx>>,
    candidates: HashMap<usize, bool>,
}

impl State<'_> {
    /// Nothing in the function being defined runs in parallel by fork and
    /// join (no fork sites, no calls placed or computed ahead).
    pub(crate) fn quiet(&self) -> bool {
        self.sites.is_empty() && self.kinds.is_empty() && self.pre.is_empty() && !matches!(self.mode, Mode::Fork(_))
    }
}

/// The version of a callee to run sequentially.
pub(crate) fn seq_of(k: Kind) -> Kind {
    match k {
        Kind::Plain => Kind::Seq,
        Kind::Native => Kind::NativeSeq,
        k => k,
    }
}

impl<'ctx> Cg<'ctx, '_> {
    /// Targets with threads.
    pub(crate) fn fj_target(&self) -> bool {
        let t = self.module.get_triple();
        let t = t.as_str().to_string_lossy();
        !["wasm", "bpf", "nvptx", "amdgcn", "spirv", "xcore", "avr", "msp430"].iter().any(|p| t.starts_with(p))
    }

    /// Find the fork sites of the function about to be defined.
    pub(crate) fn fj_begin(&mut self, fv: FunctionValue<'ctx>, kind: Kind, decl: &FnDecl) -> R<()> {
        self.fj.mode = if matches!(kind, Kind::Seq | Kind::NativeSeq) { Mode::Seq } else { Mode::Root };
        self.fj.site_fn = Some(fv);
        self.fj.sites.clear();
        self.fj.in_lambda.clear();
        self.fj.pre.clear();
        self.fj.kinds.clear();
        self.fj.candidates.clear();
        if kind == Kind::NetWrap || !self.fj_target() {
            return Ok(());
        }
        let Some(fid) = self.cur_fid else { return Ok(()) };
        self.fj.self_loops = loops_only(self.p, fid, decl);
        let mut all: Vec<*const Expr> = Vec::new();
        decl.body.walk(&mut |x| all.push(x as *const Expr));
        let mut in_lambda = HashSet::new();
        for &x in &all {
            if let ExprKind::Lambda { body, .. } = unsafe { &(*x).kind } {
                body.walk(&mut |y| {
                    in_lambda.insert(id(y));
                });
            }
        }
        self.fj.in_lambda = in_lambda;
        // outermost first: a site claims its calls
        let mut claimed: HashSet<usize> = HashSet::new();
        for &x in &all {
            let e = unsafe { &*x };
            if self.fj.in_lambda.contains(&id(e)) {
                continue;
            }
            let mut calls = Vec::new();
            self.fj_collect(e, true, &mut calls);
            calls.retain(|c| !claimed.contains(&(*c as usize)));
            if calls.len() >= 2 {
                claimed.extend(calls.iter().map(|c| *c as usize));
                self.fj.sites.insert(id(e), calls);
            }
        }
        Ok(())
    }

    pub(crate) fn fj_end(&mut self) {
        self.fj.site_fn = None;
        self.fj.sites.clear();
        self.fj.mode = Mode::Root;
    }

    /// The calls of a fork site at `e`, when `e` is one here.
    pub(crate) fn fj_site_at(&self, e: &Expr) -> Option<Vec<*const Expr>> {
        if self.cur_fn.is_none() || self.cur_fn != self.fj.site_fn {
            return None;
        }
        self.fj.sites.get(&id(e)).cloned()
    }

    /// A call that may run in parallel with others: to a pure function that
    /// can recurse, with arguments that have no effects.
    fn fork_candidate(&mut self, e: &Expr) -> bool {
        if let Some(b) = self.fj.candidates.get(&id(e)) {
            return *b;
        }
        let b = self.fork_candidate_uncached(e);
        self.fj.candidates.insert(id(e), b);
        b
    }

    fn fork_candidate_uncached(&mut self, e: &Expr) -> bool {
        let ExprKind::Call { callee, .. } = &e.kind else { return false };
        if !matches!(callee.kind, ExprKind::Path(_)) {
            return false;
        }
        let Some(k) = self.cur_spec.as_ref().and_then(|s| s.calls.get(&id(e)).cloned()) else { return false };
        if self.purity.fork_call(self.p, self.cur_mod, self.cur_fid, e) != Some(k.0) {
            return false;
        }
        if self.fj.self_loops && Some(k.0) == self.cur_fid {
            return false;
        }
        let ok_ty = |t: Ty| matches!(t, Ty::I64 | Ty::F64 | Ty::Bool | Ty::Char | Ty::Obj(_) | Ty::Tup(_));
        if !ok_ty(self.ty_of(e)) {
            return false;
        }
        match self.spec(&k) {
            Ok(s) if ok_ty(s.ret) => {}
            _ => return false,
        }
        self.reaches_recursion(vec![k])
    }

    /// The candidate calls among the operands `e` always evaluates.
    fn fj_collect(&mut self, e: &Expr, top: bool, out: &mut Vec<*const Expr>) {
        if !top && self.fork_candidate(e) {
            out.push(e as *const Expr);
            return;
        }
        match &e.kind {
            ExprKind::Binary { op, lhs, rhs } if !matches!(op, BinOp::And | BinOp::Or) => {
                self.fj_collect(lhs, false, out);
                self.fj_collect(rhs, false, out);
            }
            ExprKind::Unary { expr, .. } | ExprKind::Field { base: expr, .. } | ExprKind::Named { value: expr, .. } => self.fj_collect(expr, false, out),
            ExprKind::Call { callee, args }
                if matches!(&callee.kind, ExprKind::Path(path)
                    if !(path.len() == 1 && self.cur_spec.as_ref().is_some_and(|_| self.is_param(&path[0])))
                    && matches!(self.p.lookup(self.cur_mod, path), Some(Resolved::Fn(_) | Resolved::Variant(..)))) =>
            {
                for a in args {
                    self.fj_collect(a, false, out);
                }
            }
            ExprKind::Tuple(xs) => {
                for x in xs {
                    self.fj_collect(x, false, out);
                }
            }
            ExprKind::Record { fields, .. } => {
                for (_, x, _) in fields {
                    self.fj_collect(x, false, out);
                }
            }
            _ => {}
        }
    }

    /// Is `name` a parameter of the function being defined (a function value)?
    fn is_param(&self, name: &str) -> bool {
        match self.cur_fid.map(|f| &self.p.fns[f].kind) {
            Some(FnKind::User(d)) => d.params.iter().any(|q| q.name == name),
            _ => false,
        }
    }

    /// `let a = f(…)` `let b = f(…)` …: a run of independent calls.
    pub(crate) fn fork_lets(&mut self, stmts: &[Stmt]) -> R<()> {
        if self.cur_fn.is_none() || self.cur_fn != self.fj.site_fn || matches!(self.fj.mode, Mode::Seq) || !self.fj_target() {
            return Ok(());
        }
        let mut run: Vec<*const Expr> = Vec::new();
        let mut names: Vec<String> = Vec::new();
        for s in stmts {
            let Stmt::Let { name, mutable: false, value, .. } = s else { break };
            if self.fj.pre.contains_key(&id(value)) || self.fj.in_lambda.contains(&id(value)) || !self.fork_candidate(value) {
                break;
            }
            let mut used = Vec::new();
            expr_names(value, &mut used);
            if used.iter().any(|u| names.contains(u)) {
                break;
            }
            names.push(name.clone());
            run.push(value as *const Expr);
        }
        if run.len() >= 2 {
            self.fork_site(&run)?;
        }
        Ok(())
    }

    fn global_i32(&self, name: &str) -> R<IntValue<'ctx>> {
        let i32t = self.ctx.i32_type();
        let g = self.module.get_global(name).unwrap_or_else(|| {
            let g = self.module.add_global(i32t, None, name);
            g.set_linkage(Linkage::External);
            g
        });
        let p = self.gaddr(g);
        Ok(self.b.build_load(i32t, p, name).s()?.into_int_value())
    }

    /// A value as its static type, owned, for a stack slot.
    fn fj_settle(&mut self, v: V<'ctx>, ty: Ty) -> R<BasicValueEnum<'ctx>> {
        let v = self.as_ty(v, ty, "value")?;
        let v = self.own(v)?;
        Ok(self.basic(v))
    }

    /// Compute the calls of a fork site ahead (in parallel when that is
    /// on); `expr` then takes their results.
    pub(crate) fn fork_site(&mut self, calls: &[*const Expr]) -> R<()> {
        let calls: Vec<&Expr> = calls.iter().map(|c| unsafe { &**c }).collect();
        if calls.iter().any(|c| self.fj.pre.contains_key(&id(*c))) {
            return Ok(());
        }
        for c in &calls {
            self.note(c.span, "parallel", format!("runs in parallel with the other {} call(s) here (fork-join): they are pure and independent; below a depth, they run in order to keep the overhead small", calls.len() - 1));
        }
        let fun = self.cur_fn.ok_or("internal compiler error: fork site outside a function")?;
        let serial = self.ctx.append_basic_block(fun, "fj.seq");
        let merge = self.ctx.append_basic_block(fun, "fj.done");
        let forking = match self.fj.mode {
            Mode::Seq => {
                self.b.build_unconditional_branch(serial).s()?;
                None
            }
            Mode::Root => {
                let fork = self.ctx.append_basic_block(fun, "fj.fork");
                let try_bb = self.ctx.append_basic_block(fun, "fj.try");
                let open = self.global_i32("mim_fj_open")?;
                let c = self.b.build_int_compare(IntPredicate::NE, open, self.ctx.i32_type().const_zero(), "").s()?;
                self.b.build_conditional_branch(c, try_bb, serial).s()?;
                self.b.position_at_end(try_bb);
                let r = self.call_rt("mim_fj_enter", &[])?.unwrap().into_int_value();
                let c = self.b.build_int_compare(IntPredicate::NE, r, self.ctx.i32_type().const_zero(), "").s()?;
                self.b.build_conditional_branch(c, fork, serial).s()?;
                Some((fork, None, true))
            }
            Mode::Fork(d) => {
                let fork = self.ctx.append_basic_block(fun, "fj.fork");
                let cut = self.global_i32("mim_fj_cut")?;
                let c = self.b.build_int_compare(IntPredicate::SLT, d, cut, "").s()?;
                self.b.build_conditional_branch(c, fork, serial).s()?;
                Some((fork, Some(d), false))
            }
        };
        // one after the other, as sequential versions
        self.b.position_at_end(serial);
        let mut slots = Vec::new();
        for c in &calls {
            let fid = self.cur_spec.as_ref().and_then(|s| s.calls.get(&id(*c))).map(|k| k.0).ok_or("internal compiler error: fork call without a callee")?;
            // below the cut, a recursion that splits itself runs its plain
            // sequential version, as on one thread (the native one counts
            // its depth on every call: ~13% slower on x86)
            let kind = if matches!(self.fj.mode, Mode::Fork(_)) && self.plan.wide.contains(&fid) { Kind::Seq } else { seq_of(self.kind_for(fid)) };
            self.fj.kinds.insert(id(*c), (kind, None));
            let v = self.expr(c)?;
            let bv = self.fj_settle(v, self.ty_of(c))?;
            let slot = self.entry_alloca_ty(bv.get_type(), "fj")?;
            self.b.build_store(slot, bv).s()?;
            slots.push((slot, bv.get_type()));
        }
        self.b.build_unconditional_branch(merge).s()?;
        if let Some((fork, d, root)) = forking {
            self.b.position_at_end(fork);
            let one = self.ctx.i32_type().const_int(1, false);
            let dv = match d {
                Some(d) => self.b.build_int_add(d, one, "").s()?,
                None => one,
            };
            self.fork_calls(&calls, &slots, dv)?;
            if root {
                self.call_rt("mim_fj_leave", &[])?;
            }
            self.b.build_unconditional_branch(merge).s()?;
        }
        self.b.position_at_end(merge);
        for (c, (slot, t)) in calls.iter().zip(slots) {
            let ld = self.b.build_load(t, slot, "").s()?;
            let v = self.val_of(ld, self.ty_of(c), true);
            self.fj.pre.insert(id(*c), v);
        }
        Ok(())
    }

    /// All but the last call to the deque, the last here, then join.
    fn fork_calls(&mut self, calls: &[&Expr], slots: &[(PointerValue<'ctx>, BasicTypeEnum<'ctx>)], depth: IntValue<'ctx>) -> R<()> {
        let n = calls.len();
        let i32t = self.ctx.i32_type();
        let mut pending = Vec::new();
        let mut temps = Vec::new();
        for c in &calls[..n - 1] {
            // a fork site among its own arguments goes first
            if let Some(inner) = self.fj_site_at(c) {
                self.fork_site(&inner)?;
            }
            let ExprKind::Call { args, .. } = &c.kind else { return Err("internal compiler error: fork of a non-call".into()) };
            let k = self.cur_spec.as_ref().and_then(|s| s.calls.get(&id(*c)).cloned()).ok_or("internal compiler error: fork call without a callee")?;
            let spec = self.spec(&k)?;
            let consumed = self.consumed(&k);
            let mut argv = Vec::new();
            for (j, a) in args.iter().enumerate() {
                let v = self.expr(a)?;
                let cv = self.as_ty(v, spec.params[j], "argument")?;
                if consumed.get(j).copied().unwrap_or(false) {
                    let cv = self.hand_over(cv)?; // the task's call takes it
                    argv.push(self.basic(cv));
                    continue;
                }
                argv.push(self.basic(cv));
                temps.push(cv);
            }
            let st = self.fj_ctx_type(&spec);
            let ctx = self.entry_alloca_ty(st.into(), "fj.ctx")?;
            let dp = self.b.build_struct_gep(st, ctx, 1, "").s()?;
            self.b.build_store(dp, depth).s()?;
            for (j, a) in argv.iter().enumerate() {
                let ap = self.b.build_struct_gep(st, ctx, j as u32 + 2, "").s()?;
                self.b.build_store(ap, *a).s()?;
            }
            let task = self.fork_task(&k)?;
            let ts = self.entry_alloca_ty(self.ctx.i64_type().array_type(4).into(), "fj.task")?;
            self.call_rt("mim_fj_fork", &[ts.into(), self.fn_addr(task).into(), ctx.into()])?;
            pending.push((*c, k, st, ctx, ts));
        }
        let last = calls[n - 1];
        self.fj.kinds.insert(id(last), (Kind::Fork, Some(depth)));
        let v = self.expr(last)?;
        let bv = self.fj_settle(v, self.ty_of(last))?;
        if bv.get_type() != slots[n - 1].1 {
            return Err("internal compiler error: fork-join value types differ".into());
        }
        self.b.build_store(slots[n - 1].0, bv).s()?;
        for (i, (c, k, st, ctx, ts)) in pending.into_iter().enumerate().rev() {
            self.call_rt("mim_fj_join", &[ts.into()])?;
            let spec = self.spec(&k)?;
            let rp = self.b.build_struct_gep(st, ctx, 0, "").s()?;
            let r = self.b.build_load(self.repr(spec.ret), rp, "").s()?;
            let v = self.val_of(r, spec.ret, true);
            // what `expr` makes of a call's result
            let ty = self.ty_of(c);
            let v = match (self.in_regs(ty), v) {
                (true, V::P(..)) => self.as_ty(v, ty, "value")?,
                (false, V::A(..) | V::R(..)) => self.boxed(v)?,
                _ => v,
            };
            let bv = self.fj_settle(v, ty)?;
            if bv.get_type() != slots[i].1 {
                return Err("internal compiler error: fork-join value types differ".into());
            }
            self.b.build_store(slots[i].0, bv).s()?;
        }
        let _ = i32t;
        for t in temps {
            self.drop_tmp(t)?;
        }
        Ok(())
    }

    /// `{ result, i32 depth, arguments… }`
    fn fj_ctx_type(&self, spec: &Spec) -> inkwell::types::StructType<'ctx> {
        let mut fields: Vec<BasicTypeEnum> = vec![self.repr(spec.ret), self.ctx.i32_type().into()];
        fields.extend(spec.params.iter().map(|t| self.repr(*t)));
        self.ctx.struct_type(&fields, false)
    }

    /// `void task(ctx)`: run the fork version of `k` on the arguments in ctx.
    fn fork_task(&mut self, k: &Key) -> R<FunctionValue<'ctx>> {
        if let Some(f) = self.fj.tasks.get(k) {
            return Ok(*f);
        }
        let target = self.request(k, Kind::Fork)?;
        let spec = self.spec(k)?;
        let name = format!("{}$task", target.get_name().to_str().unwrap());
        let f = self.module.add_function(&name, self.ctx.void_type().fn_type(&[self.ptr.into()], false), Some(Linkage::Internal));
        self.fj.tasks.insert(k.clone(), f);
        let st = self.fj_ctx_type(&spec);
        self.in_new_fn(f, |cg| {
            let ctx = f.get_nth_param(0).unwrap().into_pointer_value();
            let mut args: Vec<BasicMetadataValueEnum> = Vec::new();
            for (j, t) in spec.params.iter().enumerate() {
                let p = cg.b.build_struct_gep(st, ctx, j as u32 + 2, "").s()?;
                args.push(cg.b.build_load(cg.repr(*t), p, "").s()?.into());
            }
            let dp = cg.b.build_struct_gep(st, ctx, 1, "").s()?;
            args.push(cg.b.build_load(cg.ctx.i32_type(), dp, "depth").s()?.into());
            let r = cg.b.build_call(target, &args, "").s()?.try_as_basic_value().basic().unwrap();
            let rp = cg.b.build_struct_gep(st, ctx, 0, "").s()?;
            cg.b.build_store(rp, r).s()?;
            cg.b.build_return(None).s()?;
            Ok(())
        })?;
        Ok(f)
    }

    /// Results computed ahead that the site's code did not take.
    pub(crate) fn fj_drop_unused(&mut self, calls: &[*const Expr]) -> R<()> {
        for c in calls {
            if let Some(v) = self.fj.pre.remove(&(*c as usize))
                && !self.terminated()
            {
                self.drop_tmp(v)?;
            }
        }
        Ok(())
    }
}
