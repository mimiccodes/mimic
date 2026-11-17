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
        Ty::Arr(e @ (El::I64 | El::F64 | El::Bool | El::F32 | El::I32), _) => Some(e),
        _ => None,
    }
}

/// A list whose elements an index reads directly: the numeric ones, and
/// bytes (each reads as an i64).
pub(crate) fn index_el(t: Ty) -> Option<El> {
    match t {
        Ty::Arr(El::U8, _) => Some(El::U8),
        _ => num_el(t),
    }
}

/// Runtime dtype code (`MIM_I64`/`MIM_F64`/`MIM_BOOL`).
fn dtype_of(e: El) -> i32 {
    match e {
        El::F64 => 1,
        El::F32 => 9,
        El::U8 => 8,
        El::I32 => 10,
        El::Bool => 2,
        _ => 0,
    }
}

/// The fused computation of one element, given the loop index.
#[derive(Clone, Debug)]
pub(crate) enum Node {
    /// `⍳ n`: the index itself
    Index,
    /// `lo ‥ hi`: `lo + index` (slot holds lo)
    Offset(usize),
    /// a materialised vector read at the index (slot holds its data pointer)
    Leaf(usize, El),
    /// a list of records read at the index: the record, lent (slot holds
    /// the list's data pointer)
    LeafObj(usize),
    /// a scalar computed once before the loop (slot holds its value)
    Scalar(usize, Ty),
    /// a number written in the program: a constant inside the kernel, so
    /// LLVM can fold it (`i % 1000` becomes a multiply and a shift)
    ConstI(i64),
    ConstF(f64),
    Bin(BinOp, Box<Node>, Box<Node>),
    Un(UnOp, Box<Node>),
    /// `xs ¨ f` with a pure `f` on unboxed numbers: call the native `f` (by
    /// name) on the element, with captured values from slots; `depth`: `f`
    /// takes the native recursion depth too
    /// `heavy`: `f` can reach recursion, so one element may be a lot of
    /// work however few elements there are (see `timed_apply`).
    /// `msl`: the function for a GPU kernel (gpufn.rs), when there is one.
    /// `simd`: its vector version (simd.rs), when there is one.
    Apply { f: String, arg: Box<Node>, caps: Vec<usize>, pt: Ty, rt: Ty, depth: bool, heavy: bool, msl: Option<std::rc::Rc<super::gpufn::MslFn>>, simd: Option<(String, Vec<usize>)> },
    /// `⌽ xs`: element `last - index` of the inner computation (slot: last)
    Rev(usize, Box<Node>),
    /// `xs ↑ n`, `xs ↓ n`: element `index + offset` (slot: offset)
    Shift(usize, Box<Node>),
    /// the second when the first (a truth value) holds, else the third: how
    /// `+/ (xs ⌿ mask)` reads, without building the filtered list
    Sel(Box<Node>, Box<Node>, Box<Node>),
    /// a value made f32 (rounded) or i32 (wrapped): kernels compute in f64
    /// and i64 and narrow where the type says so, which is exactly f32 and
    /// i32 arithmetic for + - × ÷ (and what the runtime does)
    Narrow(El, Box<Node>),
}

/// Does the computation call a function that can reach recursion?
/// Does `e` loop over arrays (a map, a range, a scan, a loop word, a
/// matrix product)?
fn array_work(e: &Expr) -> bool {
    array_work_in(|f| e.walk(f))
}

fn array_work_in(walk: impl FnOnce(&mut dyn FnMut(&Expr))) -> bool {
    // a loop over a few elements the program states (`⍳ 5`, `0 ‥ 25`) is
    // small work: nbody's five bodies must stay inlined, not be shared out
    fn small(e: &Expr) -> bool {
        match &e.kind {
            ExprKind::Prim { prim: Prim::Iota, args } => matches!(args[0].kind, ExprKind::Int(n) if n <= 4096),
            ExprKind::Prim { prim: Prim::Range, args } => matches!((&args[0].kind, &args[1].kind), (ExprKind::Int(a), ExprKind::Int(b)) if b - a <= 4096),
            ExprKind::Array(xs) => xs.len() <= 4096,
            _ => false,
        }
    }
    let mut found = false;
    walk(&mut |x| match &x.kind {
        ExprKind::Prim { prim: Prim::Iota | Prim::Range, .. } if small(x) => {}
        ExprKind::Prim { prim: Prim::Each, args } if small(&args[0]) || matches!(&args[0].kind, ExprKind::Prim { prim: Prim::Each, args: a2 } if small(&a2[0])) => {}
        ExprKind::Prim { prim: Prim::Each | Prim::Iota | Prim::Range | Prim::Gather | Prim::Trail | Prim::Evolve | Prim::Settle | Prim::MatMul, .. } => found = true,
        _ => {}
    });
    found
}

fn heavy(n: &Node) -> bool {
    match n {
        Node::Apply { heavy, arg, .. } => *heavy || self::heavy(arg),
        Node::Bin(_, a, b) => heavy(a) || heavy(b),
        Node::Un(_, a) | Node::Rev(_, a) | Node::Shift(_, a) | Node::Narrow(_, a) => heavy(a),
        Node::Sel(c, a, b) => heavy(c) || heavy(a) || heavy(b),
        _ => false,
    }
}

/// Values computed before the loop and handed to the kernel.
#[derive(Default)]
pub(crate) struct Plan<'ctx> {
    slots: Vec<BasicValueEnum<'ctx>>,
    lens: Vec<IntValue<'ctx>>,
    temps: Vec<V<'ctx>>,
    /// each leaf's slot and length (a GPU kernel copies the leaves)
    leaf_lens: Vec<(usize, IntValue<'ctx>)>,
    /// variables already read as leaves: name, slot
    named: Vec<(String, usize)>,
}

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Map(El),
    Reduce(Prim, bool), // primitive, float accumulator
    /// running results: each chunk starts from its offset (slot after the grain)
    Scan(Prim, El),
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
            ExprKind::Prim { prim: Prim::Each, args } => self.each_fusible(e, args),
            // reversed, taken or dropped: the same elements at other indices
            ExprKind::Prim { prim: Prim::Reverse, args } => args.len() == 1 && self.is_vec_operand(&args[0]),
            ExprKind::Prim { prim: Prim::Take | Prim::Drop, args } => args.len() == 2 && self.is_vec_operand(&args[0]) && self.ty_of(&args[1]) == Ty::I64,
            _ => false,
        }
    }

    /// A numeric vector, fusible or materialised.
    fn is_vec_operand(&self, x: &Expr) -> bool {
        self.is_fae(x) || index_el(self.ty_of(x)).is_some()
    }

    /// `xs ¨ f` computed element by element: `xs` a numeric array, `f` a pure
    /// lambda of one parameter (capturing only numbers and numeric arrays) or
    /// a pure named function, both on unboxed numbers.
    fn each_fusible(&self, e: &Expr, args: &[Expr]) -> bool {
        self.each_fusible_to(e, args, false)
    }

    /// `xs ¨ f` over numbers (or a fusible expression) where `f` makes a
    /// record: one pass that writes the records straight into the new list
    /// (no `⍳` list, no boxed call per element), in parallel.
    pub(crate) fn each_obj_fusible(&self, e: &Expr) -> bool {
        let ExprKind::Prim { prim: Prim::Each, args } = &e.kind else { return false };
        matches!(self.ty_of(e), Ty::Arr(El::Obj(_), _))
            // (rows of a screen keep their own path: reused when unchanged)
            && !self.screen_rows(self.ty_of(e))
            && args.len() == 2
            && (self.is_fae(&args[0]) || num_el(self.ty_of(&args[0])).is_some() || self.flat_list(self.ty_of(&args[0])) || self.obj_chain(&args[0]))
            && self.each_fusible_to(e, args, true)
    }

    /// A list of flat records (each read into registers as it is used).
    fn flat_list(&self, t: Ty) -> bool {
        matches!(t, Ty::Arr(el @ El::Obj(_), _) if self.flat(Ty::of_el(el)).is_some())
    }

    /// `xs ¨ f` making flat records from numbers or flat records: inside a
    /// bigger fused loop each record stays in registers (`ps ¨ step ¨ step`
    /// is one pass, with no list of records in between).
    fn obj_chain(&self, x: &Expr) -> bool {
        let ExprKind::Prim { prim: Prim::Each, args } = &x.kind else { return false };
        self.flat_list(self.ty_of(x))
            && !self.screen_rows(self.ty_of(x))
            && args.len() == 2
            && (self.is_fae(&args[0]) || num_el(self.ty_of(&args[0])).is_some() || self.flat_list(self.ty_of(&args[0])) || self.obj_chain(&args[0]))
            && self.each_fusible_to(x, args, true)
    }

    /// (`records`: `f` may return a record)
    fn each_fusible_to(&self, e: &Expr, args: &[Expr], records: bool) -> bool {
        if args.len() != 2 || !(self.is_fae(&args[0]) || index_el(self.ty_of(&args[0])).is_some() || matches!(self.ty_of(&args[0]), Ty::Arr(El::Obj(_), _))) {
            return false;
        }
        if !self.purity.operand_is_pure(self.p, self.cur_mod, self.cur_fid, &args[1]) {
            return false;
        }
        let simple_num = |t: Ty| (t.is_unboxed() || matches!(t, Ty::F32 | Ty::I32)) && t != Ty::Unit;
        let simple = |t: Ty| simple_num(t) || (records && matches!(t, Ty::Obj(_)));
        // a record on the heap comes in lent, as a pointer; a flat one is
        // read into registers first
        let simple_in = |t: Ty| simple_num(t) || matches!(t, Ty::Obj(_));
        match &args[1].kind {
            ExprKind::Lambda { params, body } => {
                let Some((pts, rt)) = self.cur_spec.as_ref().and_then(|s| s.lambdas.get(&id(&args[1])).cloned()) else { return false };
                params.len() == 1
                    && pts.len() == 1
                    && simple_in(pts[0])
                    && simple(rt)
                    // captured values reach the kernel as they are (numbers
                    // unboxed, anything else as a borrowed reference)
                    && self.captures(params, body).iter().all(|(_, t)| *t != Ty::Unit && *t != Ty::Never)
            }
            ExprKind::Path(path) => {
                let Ok(fid) = self.resolve_fn(path) else { return false };
                if !matches!(self.p.fns[fid].kind, FnKind::User(_)) {
                    return false;
                }
                let Some(k) = self.cur_spec.as_ref().and_then(|s| s.calls.get(&id(e)).cloned()) else { return false };
                let Ok(spec) = self.spec(&k) else { return false };
                spec.params.len() == 1 && simple_in(spec.params[0]) && simple(spec.ret)
            }
            _ => false,
        }
    }

    /// An operand of a fused operation: a fusible array, any numeric vector
    /// (read directly), or an unboxed scalar.
    fn is_operand(&self, x: &Expr) -> bool {
        self.is_fae(x) || index_el(self.ty_of(x)).is_some() || self.ty_of(x).is_unboxed()
    }

    /// Worth fusing: an operation over arrays (a lone `⍳ n` or variable is not).
    pub(crate) fn fusible_root(&self, e: &Expr) -> bool {
        match &e.kind {
            ExprKind::Binary { .. } | ExprKind::Unary { .. } | ExprKind::Prim { prim: Prim::Each, .. } => self.is_fae(e),
            // `⌽ (xs × 2)`: one pass; `⌽ xs` alone is a plain copy (the runtime's)
            ExprKind::Prim { prim: Prim::Reverse | Prim::Take | Prim::Drop, args } => self.is_fae(e) && self.is_fae(&args[0]) && !matches!(args[0].kind, ExprKind::Path(_)),
            _ => false,
        }
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
                // f32 (i32) arithmetic: its operands made f32 (i32) first
                // (`xs * 0.1` multiplies by the f32 nearest 0.1; `xs > 0.1`
                // compares with it), then the result, unless a truth value
                let (tl, tr) = (self.ty_of(lhs), self.ty_of(rhs));
                let el_of = |t: Ty| match t {
                    Ty::Arr(el, _) => (Ty::of_el(el), true),
                    t => (t, false),
                };
                let ((x, xl), (y, yl)) = (el_of(tl), el_of(tr));
                let joint = types::num_join(x, xl, y, yl);
                if matches!(joint, Ty::F32 | Ty::I32) {
                    let el = if joint == Ty::F32 { El::F32 } else { El::I32 };
                    let (a, b) = (Node::Narrow(el, Box::new(a)), Node::Narrow(el, Box::new(b)));
                    let bin = Node::Bin(*op, Box::new(a), Box::new(b));
                    return Ok(if num_el(self.ty_of(e)) == Some(el) { Node::Narrow(el, Box::new(bin)) } else { bin });
                }
                Ok(Node::Bin(*op, Box::new(a), Box::new(b)))
            }
            ExprKind::Unary { op, expr } => {
                let a = Node::Un(*op, Box::new(self.operand(expr, plan)?));
                if let Some(el @ (El::F32 | El::I32)) = num_el(self.ty_of(e)) {
                    return Ok(Node::Narrow(el, Box::new(a)));
                }
                Ok(a)
            }
            ExprKind::Prim { prim: Prim::Each, args } => {
                let arg = Box::new(self.operand(&args[0], plan)?);
                match &args[1].kind {
                    ExprKind::Lambda { params, body } => {
                        let caps = self.captures(params, body);
                        let mut slots = Vec::new();
                        for (name, t) in &caps {
                            let l = self.lookup(name).ok_or("internal compiler error: captured variable")?;
                            let (slot, lt) = (l.slot, l.ty);
                            let v = self.b.build_load(self.repr(lt), slot, name).s()?;
                            plan.slots.push(v);
                            slots.push(plan.slots.len() - 1);
                            let _ = t;
                        }
                        let mut called = Vec::new();
                        body.walk(&mut |x| {
                            if let Some(k) = self.cur_spec.as_ref().and_then(|s| s.calls.get(&id(x)).cloned()) {
                                called.push(k);
                            }
                        });
                        let heavy = array_work(body) || self.reaches_heavy(called);
                        let (f, pts, rt) = self.lambda_fn_with(&args[1], &caps)?;
                        let msl = self.gpu_wanted().then(|| self.msl_lambda(&args[1], &caps)).flatten().map(std::rc::Rc::new);
                        // (the captures it divides by: as slots)
                        let simd = if self.opt_level >= 2 { self.simd_lambda(&args[1], &caps).map(|(f, g)| (f.get_name().to_str().unwrap().to_string(), g.iter().map(|k| slots[*k]).collect())) } else { None };
                        if simd.is_some() {
                            self.note(args[1].span, "simd", format!("SIMD across elements: the function runs on {} elements at once (vector instructions)", crate::simd::lanes()));
                        }
                        Ok(Node::Apply { f: f.get_name().to_str().unwrap().to_string(), arg, caps: slots, pt: pts[0], rt, depth: false, heavy, msl, simd })
                    }
                    ExprKind::Path(path) => {
                        let fid = self.resolve_fn(path)?;
                        let k = self.cur_spec.as_ref().and_then(|s| s.calls.get(&id(e)).cloned()).ok_or("internal compiler error: untyped ¨ callback")?;
                        let kind = if self.plan.entered.contains(&fid) { Kind::Native } else { Kind::Plain };
                        let f = self.request(&k, kind)?;
                        let spec = self.spec(&k)?;
                        let heavy = self.reaches_heavy(vec![k.clone()]);
                        let msl = self.gpu_wanted().then(|| self.msl_named(&k)).flatten().map(std::rc::Rc::new);
                        let simd = if self.opt_level >= 2 { self.simd_named(&k, 0).map(|f| (f.get_name().to_str().unwrap().to_string(), Vec::new())) } else { None };
                        if simd.is_some() {
                            self.note(args[1].span, "simd", format!("SIMD across elements: the function runs on {} elements at once (vector instructions)", crate::simd::lanes()));
                        }
                        Ok(Node::Apply { f: f.get_name().to_str().unwrap().to_string(), arg, caps: vec![], pt: spec.params[0], rt: spec.ret, depth: kind == Kind::Native, heavy, msl, simd })
                    }
                    _ => Err("internal compiler error: not a fusible ¨".into()),
                }
            }
            ExprKind::Prim { prim: p @ (Prim::Reverse | Prim::Take | Prim::Drop), args } => {
                // the inner computation has a length of its own
                let mark = plan.lens.len();
                let inner = self.operand(&args[0], plan)?;
                let inner_lens: Vec<IntValue<'ctx>> = plan.lens.drain(mark..).collect();
                let n = self.common_len(&inner_lens)?;
                let i64t = self.ctx.i64_type();
                let zero = i64t.const_zero();
                if *p == Prim::Reverse {
                    let last = self.b.build_int_sub(n, i64t.const_int(1, false), "last").s()?;
                    plan.lens.push(n);
                    plan.slots.push(last.into());
                    return Ok(Node::Rev(plan.slots.len() - 1, Box::new(inner)));
                }
                // as the runtime: |count| clamped to the length; a negative
                // count takes (drops) from the end
                let c = self.expr(&args[1])?;
                let c = self.to_i64(c)?;
                let neg = self.b.build_int_compare(IntPredicate::SLT, c, zero, "").s()?;
                let mc = self.b.build_int_neg(c, "").s()?;
                let k = self.b.build_select(neg, mc, c, "").s()?.into_int_value();
                let big = self.b.build_int_compare(IntPredicate::SGT, k, n, "").s()?;
                let k = self.b.build_select(big, n, k, "k").s()?.into_int_value();
                let rest = self.b.build_int_sub(n, k, "").s()?;
                let (len, off) = if *p == Prim::Take {
                    (k, self.b.build_select(neg, rest, zero, "").s()?.into_int_value())
                } else {
                    (rest, self.b.build_select(neg, zero, k, "").s()?.into_int_value())
                };
                plan.lens.push(len);
                plan.slots.push(off.into());
                Ok(Node::Shift(plan.slots.len() - 1, Box::new(inner)))
            }
            _ => Err("internal compiler error: not a fusible expression".into()),
        }
    }

    fn operand(&mut self, x: &Expr, plan: &mut Plan<'ctx>) -> R<Node> {
        if self.is_fae(x) {
            return self.build(x, plan);
        }
        match &x.kind {
            ExprKind::Int(v) if self.ty_of(x) == Ty::I64 => return Ok(Node::ConstI(*v)),
            ExprKind::Float(v) if self.ty_of(x) == Ty::F64 => return Ok(Node::ConstF(*v)),
            // f32 and i32 numbers written in the program: already rounded
            // (wrapped), so constants too
            ExprKind::Float(v) if self.ty_of(x) == Ty::F32 => return Ok(Node::ConstF(*v as f32 as f64)),
            ExprKind::Int(v) if self.ty_of(x) == Ty::F32 => return Ok(Node::ConstF(*v as f64 as f32 as f64)),
            ExprKind::Int(v) if self.ty_of(x) == Ty::I32 => return Ok(Node::ConstI(*v as i32 as i64)),
            _ => {}
        }
        if self.obj_chain(x) {
            return self.build(x, plan);
        }
        let t = self.ty_of(x);
        if let Ty::Arr(el @ El::Obj(_), _) = t {
            let v = self.expr(x)?;
            let v = self.boxed(v)?;
            let (len, data) = self.vec_header(v, el)?;
            plan.lens.push(len);
            plan.slots.push(data);
            plan.temps.push(v);
            return Ok(Node::LeafObj(plan.slots.len() - 1));
        }
        if let Some(el) = index_el(t) {
            // a variable read again: the same leaf
            let name = match &x.kind {
                ExprKind::Path(p) if p.len() == 1 && self.lookup(&p[0]).is_some() => Some(p[0].clone()),
                _ => None,
            };
            if let Some(n) = &name
                && let Some((_, s)) = plan.named.iter().find(|(m, _)| m == n)
            {
                // (its length again: ⌽ ↑ ↓ collect the lengths inside them)
                let s = *s;
                let len = plan.leaf_lens.iter().find(|(t, _)| *t == s).map(|(_, l)| *l).ok_or("internal compiler error: a leaf without a length")?;
                plan.lens.push(len);
                return Ok(Node::Leaf(s, el));
            }
            let v = self.expr(x)?;
            let node = self.leaf(v, el, plan)?;
            if let (Some(n), Node::Leaf(s, _)) = (name, &node) {
                plan.named.push((n, *s));
            }
            return Ok(node);
        }
        let v = self.expr(x)?;
        let v = self.as_ty(v, t, "operand")?;
        plan.slots.push(self.basic(v));
        Ok(Node::Scalar(plan.slots.len() - 1, t))
    }

    /// A numeric list read at the index.
    fn leaf(&mut self, v: V<'ctx>, el: El, plan: &mut Plan<'ctx>) -> R<Node> {
        // a list in registers: the loop reads a stack copy of it
        if let V::A(a, _) = v {
            let at = a.get_type();
            let tmp = self.entry_alloca_ty(at.into(), "leaf")?;
            self.b.build_store(tmp, a).s()?;
            let len = self.ctx.i64_type().const_int(at.len() as u64, false);
            plan.lens.push(len);
            plan.slots.push(tmp.into());
            plan.leaf_lens.push((plan.slots.len() - 1, len));
            return Ok(Node::Leaf(plan.slots.len() - 1, el));
        }
        let v = self.boxed(v)?;
        let (len, data) = self.vec_header(v, el)?;
        plan.lens.push(len);
        plan.slots.push(data);
        plan.temps.push(v);
        plan.leaf_lens.push((plan.slots.len() - 1, len));
        Ok(Node::Leaf(plan.slots.len() - 1, el))
    }

    /// The common length of every array in the expression (panics on mismatch).
    fn plan_len(&self, plan: &Plan<'ctx>) -> R<IntValue<'ctx>> {
        self.common_len(&plan.lens)
    }

    fn common_len(&self, lens: &[IntValue<'ctx>]) -> R<IntValue<'ctx>> {
        let n = *lens.first().ok_or("internal compiler error: a fused expression without arrays")?;
        for l in &lens[1..] {
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
            } else if v.get_type().is_struct_type() || v.get_type().is_array_type() {
                // a list or record kept in registers is wider than a slot:
                // it goes to the stack, the slot holds its address (it was
                // stored over the next slots, and the kernel read garbage)
                let at = self.entry_alloca_ty(v.get_type(), "kctx_value")?;
                self.b.build_store(at, *v).s()?;
                at.into()
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
                    // 4-byte numbers, widened for the computation
                    El::F32 => {
                        let p = unsafe { self.b.build_in_bounds_gep(self.ctx.f32_type(), base, &[i], "").s()? };
                        let x = self.b.build_load(self.ctx.f32_type(), p, "").s()?.into_float_value();
                        V::F(self.b.build_float_ext(x, self.ctx.f64_type(), "").s()?)
                    }
                    El::I32 => {
                        let p = unsafe { self.b.build_in_bounds_gep(self.ctx.i32_type(), base, &[i], "").s()? };
                        let x = self.b.build_load(self.ctx.i32_type(), p, "").s()?.into_int_value();
                        V::I(self.b.build_int_s_extend(x, self.ctx.i64_type(), "").s()?)
                    }
                    El::F64 => {
                        let p = unsafe { self.b.build_in_bounds_gep(self.ctx.f64_type(), base, &[i], "").s()? };
                        V::F(self.b.build_load(self.ctx.f64_type(), p, "").s()?.into_float_value())
                    }
                    El::U8 => {
                        let p = unsafe { self.b.build_in_bounds_gep(self.ctx.i8_type(), base, &[i], "").s()? };
                        let b = self.b.build_load(self.ctx.i8_type(), p, "").s()?.into_int_value();
                        V::I(self.b.build_int_z_extend(b, self.ctx.i64_type(), "").s()?)
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
            Node::LeafObj(s) => {
                let p = unsafe { self.b.build_in_bounds_gep(self.ptr, slots[*s].into_pointer_value(), &[i], "").s()? };
                V::P(self.b.build_load(self.ptr, p, "elem").s()?.into_pointer_value(), false)
            }
            Node::Scalar(s, t) => self.val_of(slots[*s], *t, false),
            Node::ConstI(v) => V::I(self.ctx.i64_type().const_int(*v as u64, true)),
            Node::ConstF(v) => V::F(self.ctx.f64_type().const_float(*v)),
            Node::Bin(op, a, b) => {
                let x = self.eval_node(a, i, slots)?;
                let y = self.eval_node(b, i, slots)?;
                self.scalar_binary(*op, x, y)?
            }
            Node::Un(op, a) => {
                let x = self.eval_node(a, i, slots)?;
                self.scalar_unary(*op, x)?
            }
            Node::Rev(s, a) => {
                let j = self.b.build_int_sub(slots[*s].into_int_value(), i, "").s()?;
                self.eval_node(a, j, slots)?
            }
            Node::Narrow(el, a) => {
                let x = self.eval_node(a, i, slots)?;
                match (el, x) {
                    (_, V::B(_)) => x, // a comparison's truth value stays one
                    (El::F32, _) => {
                        let d = self.to_f64(x)?;
                        let f = self.b.build_float_trunc(d, self.ctx.f32_type(), "").s()?;
                        V::F(self.b.build_float_ext(f, self.ctx.f64_type(), "").s()?)
                    }
                    (_, V::F(_)) => x, // (an i32 operand of a float: not narrowed)
                    _ => {
                        let n = self.to_i64(x)?;
                        let t = self.b.build_int_truncate(n, self.ctx.i32_type(), "").s()?;
                        V::I(self.b.build_int_s_extend(t, self.ctx.i64_type(), "").s()?)
                    }
                }
            }
            Node::Sel(c, a, b) => {
                let c = self.eval_node(c, i, slots)?;
                let c = self.truth(c)?;
                let (x, y) = (self.eval_node(a, i, slots)?, self.eval_node(b, i, slots)?);
                match (x, y) {
                    (V::I(x), V::I(y)) => V::I(self.b.build_select(c, x, y, "").s()?.into_int_value()),
                    _ => {
                        let (x, y) = (self.to_f64(x)?, self.to_f64(y)?);
                        V::F(self.b.build_select(c, x, y, "").s()?.into_float_value())
                    }
                }
            }
            Node::Shift(s, a) => {
                let j = self.b.build_int_add(i, slots[*s].into_int_value(), "").s()?;
                self.eval_node(a, j, slots)?
            }
            Node::Apply { f, arg, caps, pt, rt, depth, heavy, .. } => {
                let x = self.eval_node(arg, i, slots)?;
                let x: BasicMetadataValueEnum = match pt {
                    Ty::F64 => self.to_f64(x)?.into(),
                    Ty::F32 | Ty::I32 => {
                        let n = self.narrow_to(x, *pt, "argument")?;
                        self.basic(n).into()
                    }
                    Ty::Bool => self.truth(x)?.into(),
                    // a flat record in registers (read from the heap when it is there)
                    Ty::Obj(_) if let Some(fs) = self.flat(*pt) => {
                        // (an element of a list of these records: no check)
                        let r = self.as_flat_in(x, *pt, &fs, "argument", false)?;
                        self.basic(r).into()
                    }
                    Ty::Obj(_) => self.basic(x).into(), // a record, lent
                    _ => self.to_i64(x)?.into(),
                };
                let mut args = vec![x];
                for c in caps {
                    args.push(slots[*c].into());
                }
                if *depth {
                    args.push(self.ctx.i32_type().const_zero().into()); // a fresh native stack
                }
                let fv = self.module.get_function(f).ok_or("internal compiler error: missing ¨ function")?;
                let call = self.b.build_call(fv, &args, "").s()?;
                // the element's function goes inside the loop (as C's loop
                // body is): its checks that do not depend on the element
                // (divisors, the lists' headers) leave the loop, and the
                // index is known to be ≥ 0. Not for work that may recurse.
                if !*heavy && !*depth {
                    let always = self.ctx.create_enum_attribute(inkwell::attributes::Attribute::get_named_enum_kind_id("alwaysinline"), 0);
                    call.add_attribute(inkwell::attributes::AttributeLoc::Function, always);
                }
                let r = call.try_as_basic_value().basic().ok_or("internal compiler error: ¨ function returns nothing")?;
                // (a record made by the function is ours)
                self.val_of(r, *rt, matches!(rt, Ty::Obj(_)))
            }
        })
    }

    /// The kernel of `xs ¨ f` with a vector version of `f` (simd.rs): W
    /// elements at a time through it, the rest one by one.
    fn emit_simd_kernel(&mut self, root: &Node, plan: &Plan<'ctx>, el: El) -> R<Option<FunctionValue<'ctx>>> {
        let Node::Apply { arg, caps, pt, rt, simd: Some((vname, guards)), depth: false, .. } = root else { return Ok(None) };
        let guards = guards.clone();
        let Some(vf) = self.module.get_function(vname) else { return Ok(None) };
        let (arg, caps, pt, rt) = (arg.as_ref().clone(), caps.clone(), *pt, *rt);
        let w = super::simd::lanes() as u64;
        self.lambdas += 1;
        let i64t = self.ctx.i64_type();
        let ft = self.ctx.void_type().fn_type(&[self.ptr.into(), i64t.into(), i64t.into()], false);
        let f = self.module.add_function(&self.kernel_name(), ft, Some(Linkage::Internal));
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
                // a list or record kept in registers: the slot holds a
                // pointer to it (pack_ctx)
                if t.is_struct_type() || t.is_array_type() {
                    let at = cg.b.build_load(cg.ptr, p, "").s()?.into_pointer_value();
                    return cg.b.build_load(t, at, "").s();
                }
                cg.b.build_load(t, p, "").s()
            };
            let mut slots = Vec::new();
            for (k, t) in kinds.iter().enumerate() {
                slots.push(load(cg, k, *t)?);
            }
            let out: BasicValueEnum = load(cg, nslots, cg.ptr.into())?;
            let mask = {
                let b1 = cg.ctx.bool_type().const_int(1, false);
                inkwell::types::VectorType::const_vector(&vec![b1; w as usize])
            };
            // W at a time
            let entry = cg.b.get_insert_block().unwrap();
            let vhead = cg.ctx.append_basic_block(f, "vhead");
            let vbody = cg.ctx.append_basic_block(f, "vbody");
            let shead = cg.ctx.append_basic_block(f, "shead");
            let sbody = cg.ctx.append_basic_block(f, "sbody");
            let done = cg.ctx.append_basic_block(f, "done");
            // a divisor the scalar code would check (0) or overflow on (-1):
            // everything one by one
            let mut ok = cg.ctx.bool_type().const_int(1, false);
            for g in &guards {
                let v = slots[*g].into_int_value();
                let v = if v.get_type().get_bit_width() < 64 { cg.b.build_int_s_extend(v, i64t, "").s()? } else { v };
                let nz = cg.b.build_int_compare(IntPredicate::NE, v, i64t.const_zero(), "").s()?;
                let nm = cg.b.build_int_compare(IntPredicate::NE, v, i64t.const_all_ones(), "").s()?;
                ok = cg.b.build_and(ok, nz, "").s()?;
                ok = cg.b.build_and(ok, nm, "").s()?;
            }
            let pre = cg.ctx.append_basic_block(f, "vpre");
            cg.b.build_conditional_branch(ok, pre, shead).s()?;
            cg.b.position_at_end(pre);
            cg.b.build_unconditional_branch(vhead).s()?;
            cg.b.position_at_end(vhead);
            let iv = cg.b.build_phi(i64t, "i").s()?;
            iv.add_incoming(&[(&lo, pre)]);
            let i = iv.as_basic_value().into_int_value();
            let end = cg.b.build_int_add(i, i64t.const_int(w, false), "").s()?;
            let fits = cg.b.build_int_compare(IntPredicate::SLE, end, hi, "").s()?;
            cg.b.build_conditional_branch(fits, vbody, shead).s()?;
            cg.b.position_at_end(vbody);
            let vt = cg.vty_of(pt).ok_or("internal compiler error: simd parameter")?;
            let mut xv = vt.get_undef();
            for j in 0..w {
                let ij = cg.b.build_int_add(i, i64t.const_int(j, false), "").s()?;
                let x = cg.eval_node(&arg, ij, &slots)?;
                let x = cg.lane_value(x, pt)?;
                xv = cg.b.build_insert_element(xv, x, cg.ctx.i32_type().const_int(j, false), "").s()?;
            }
            let mut args: Vec<BasicMetadataValueEnum> = vec![xv.into()];
            for c in &caps {
                args.push(slots[*c].into());
            }
            args.push(mask.into());
            let r = cg.b.build_call(vf, &args, "").s()?.try_as_basic_value().basic().ok_or("internal compiler error: simd result")?.into_vector_value();
            for j in 0..w {
                let ij = cg.b.build_int_add(i, i64t.const_int(j, false), "").s()?;
                let x = cg.b.build_extract_element(r, cg.ctx.i32_type().const_int(j, false), "").s()?;
                let v = cg.val_of(x, rt, false);
                cg.store_elem(out, el, ij, v)?;
            }
            let vlatch = cg.b.get_insert_block().unwrap();
            cg.b.build_unconditional_branch(vhead).s()?;
            iv.add_incoming(&[(&end, vlatch)]);
            // the rest, one by one
            cg.b.position_at_end(shead);
            let jv = cg.b.build_phi(i64t, "j").s()?;
            jv.add_incoming(&[(&i, vhead), (&lo, entry)]);
            let j = jv.as_basic_value().into_int_value();
            let more = cg.b.build_int_compare(IntPredicate::SLT, j, hi, "").s()?;
            cg.b.build_conditional_branch(more, sbody, done).s()?;
            cg.b.position_at_end(sbody);
            let v = cg.eval_node(&root, j, &slots)?;
            cg.store_elem(out, el, j, v)?;
            let j1 = cg.b.build_int_add(j, i64t.const_int(1, false), "").s()?;
            let slatch = cg.b.get_insert_block().unwrap();
            cg.b.build_unconditional_branch(shead).s()?;
            jv.add_incoming(&[(&j1, slatch)]);
            cg.b.position_at_end(done);
            cg.b.build_return(None).s()?;
            Ok(())
        })?;
        Ok(Some(f))
    }

    /// An element as a lane of a vector of type `t` (as Apply converts it).
    fn lane_value(&self, x: V<'ctx>, t: Ty) -> R<BasicValueEnum<'ctx>> {
        Ok(match t {
            Ty::F64 => self.to_f64(x)?.into(),
            Ty::Bool => self.truth(x)?.into(),
            Ty::F32 | Ty::I32 => {
                let n = self.narrow_to(x, t, "argument")?;
                self.basic(n)
            }
            _ => self.to_i64(x)?.into(),
        })
    }

    fn vty_of(&self, t: Ty) -> Option<inkwell::types::VectorType<'ctx>> {
        let w = super::simd::lanes();
        Some(match t {
            Ty::F64 => self.ctx.f64_type().vec_type(w),
            Ty::F32 => self.ctx.f32_type().vec_type(w),
            Ty::I64 => self.ctx.i64_type().vec_type(w),
            Ty::I32 => self.ctx.i32_type().vec_type(w),
            Ty::Bool => self.ctx.bool_type().vec_type(w),
            _ => return None,
        })
    }

    /// Generate `void kernel(ptr ctx, i64 lo, i64 hi)`.
    fn emit_kernel(&mut self, root: &Node, plan: &Plan<'ctx>, mode: Mode) -> R<FunctionValue<'ctx>> {
        if let Mode::Map(el) = mode
            && let Some(k) = self.emit_simd_kernel(root, plan, el)?
        {
            return Ok(k);
        }
        self.lambdas += 1;
        let i64t = self.ctx.i64_type();
        let ft = self.ctx.void_type().fn_type(&[self.ptr.into(), i64t.into(), i64t.into()], false);
        let f = self.module.add_function(&self.kernel_name(), ft, Some(Linkage::Internal));
        // slot types in the kernel mirror the caller's values
        let kinds: Vec<BasicTypeEnum<'ctx>> = plan.slots.iter().map(|v| v.get_type()).collect();
        let nslots = kinds.len();
        let root = root.clone();
        // the length, when the program says it (⍳ 100000000): a known bound
        let known_len = plan.lens.iter().find_map(|l| l.get_zero_extended_constant());
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
                // a list or record kept in registers: the slot holds a
                // pointer to it (pack_ctx)
                if t.is_struct_type() || t.is_array_type() {
                    let at = cg.b.build_load(cg.ptr, p, "").s()?.into_pointer_value();
                    return cg.b.build_load(t, at, "").s();
                }
                cg.b.build_load(t, p, "").s()
            };
            let mut slots = Vec::new();
            for (k, t) in kinds.iter().enumerate() {
                slots.push(load(cg, k, *t)?);
            }
            let out = load(cg, nslots, cg.ptr.into())?.into_pointer_value();
            // chunks start at 0 or later: indices are never negative, which
            // lets LLVM drop the sign fix-ups of floored `%` and `div`
            let nonneg = cg.b.build_int_compare(IntPredicate::SGE, lo, i64t.const_zero(), "").s()?;
            let assume = Intrinsic::find("llvm.assume")
                .and_then(|i| i.get_declaration(&cg.module, &[]))
                .ok_or("internal compiler error: llvm.assume")?;
            cg.b.build_call(assume, &[nonneg.into()], "").s()?;
            // a scan's chunk starts from what the chunks before it combine to
            let scan_start: Option<BasicValueEnum<'ctx>> = match mode {
                Mode::Scan(_, el) => {
                    let grain = load(cg, nslots + 1, i64t.into())?.into_int_value();
                    let offs = load(cg, nslots + 2, cg.ptr.into())?.into_pointer_value();
                    let k = cg.b.build_int_signed_div(lo, grain, "").s()?;
                    let t: BasicTypeEnum = if el == El::F64 { cg.ctx.f64_type().into() } else { i64t.into() };
                    let p = unsafe { cg.b.build_in_bounds_gep(t, offs, &[k], "").s()? };
                    Some(cg.b.build_load(t, p, "").s()?)
                }
                _ => None,
            };
            let entry = cg.b.get_insert_block().unwrap();
            let body = cg.ctx.append_basic_block(f, "body");
            let done = cg.ctx.append_basic_block(f, "done");
            let empty = cg.b.build_int_compare(IntPredicate::SGE, lo, hi, "").s()?;
            cg.b.build_conditional_branch(empty, done, body).s()?;
            cg.b.position_at_end(body);
            let iphi = cg.b.build_phi(i64t, "i").s()?;
            let (acc_ty, float) = match mode {
                Mode::Reduce(_, fl) => (if fl { cg.ctx.f64_type().as_basic_type_enum() } else { i64t.as_basic_type_enum() }, fl),
                Mode::Scan(_, el) => (if el == El::F64 { cg.ctx.f64_type().as_basic_type_enum() } else { i64t.as_basic_type_enum() }, el == El::F64),
                Mode::Map(_) => (i64t.as_basic_type_enum(), false),
            };
            let accphi = cg.b.build_phi(acc_ty, "acc").s()?;
            let i = iphi.as_basic_value().into_int_value();
            // a length the program states that fits in 32 bits: the index is
            // computed with as a visibly small number, so LLVM can work in
            // 32-bit lanes (twice as many per SIMD register), as C does
            let iv = match known_len {
                Some(n) if n <= u32::MAX as u64 => {
                    let t = cg.b.build_int_truncate(i, cg.ctx.i32_type(), "").s()?;
                    cg.b.build_int_z_extend(t, i64t, "i32").s()?
                }
                _ => i,
            };
            let v = cg.eval_node(&root, iv, &slots)?;
            let acc = accphi.as_basic_value();
            let next_acc: BasicValueEnum = match mode {
                Mode::Map(el) => {
                    cg.store_elem(out.into(), el, i, v)?;
                    acc
                }
                Mode::Reduce(prim, _) => cg.combine(prim, float, acc, v)?,
                Mode::Scan(prim, el) => {
                    let next = cg.combine(prim, float, acc, v)?;
                    let (t, p): (BasicTypeEnum, _) = if el == El::F64 {
                        (cg.ctx.f64_type().into(), unsafe { cg.b.build_in_bounds_gep(cg.ctx.f64_type(), out, &[i], "").s()? })
                    } else {
                        (i64t.into(), unsafe { cg.b.build_in_bounds_gep(i64t, out, &[i], "").s()? })
                    };
                    let _ = t;
                    cg.b.build_store(p, next).s()?;
                    next
                }
            };
            let i1 = cg.b.build_int_add(i, i64t.const_int(1, false), "").s()?;
            let more = cg.b.build_int_compare(IntPredicate::SLT, i1, hi, "").s()?;
            let latch = cg.b.get_insert_block().unwrap();
            cg.b.build_conditional_branch(more, body, done).s()?;
            iphi.add_incoming(&[(&lo, entry), (&i1, latch)]);
            let ident = scan_start.unwrap_or_else(|| cg.identity(mode, float));
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
            Mode::Scan(p, _) | Mode::Reduce(p, _) => match (p, float) {
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
                Prim::Product => self.contracted(b.build_float_mul(a, x, "").s()?).into(),
                Prim::Maximum => {
                    let c = b.build_float_compare(FloatPredicate::OGT, x, a, "").s()?;
                    b.build_select(c, x, a, "").s()?
                }
                Prim::Minimum => {
                    let c = b.build_float_compare(FloatPredicate::OLT, x, a, "").s()?;
                    b.build_select(c, x, a, "").s()?
                }
                _ => self.contracted(b.build_float_add(a, x, "").s()?).into(),
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

    /// Can calling any of `keys` (specialised functions) lead to recursion?
    /// Then a single call may be any amount of work.
    pub(crate) fn reaches_recursion(&mut self, keys: Vec<types::Key>) -> bool {
        // depth-first over the call graph: a function met again on the
        // current path is a cycle
        fn visit(cg: &mut Cg, k: &types::Key, path: &mut Vec<types::Key>, done: &mut Vec<types::Key>) -> bool {
            if path.contains(k) {
                return true;
            }
            if done.contains(k) {
                return false;
            }
            let Ok(spec) = cg.spec(k) else { return false };
            path.push(k.clone());
            let callees: Vec<types::Key> = spec.calls.values().cloned().collect();
            let found = callees.iter().any(|c| visit(cg, c, path, done));
            path.pop();
            done.push(k.clone());
            found
        }
        let (mut path, mut done) = (Vec::new(), Vec::new());
        keys.iter().any(|k| visit(self, k, &mut path, &mut done))
    }

    /// May one call of these be a lot of work: they reach recursion, or
    /// loop over arrays themselves (`¨`, `⍳`, scans …)? Then a fused loop
    /// over them is shared out by time (mim_par_for_timed), not by count:
    /// `⍳ 30 ¨ frame` with a million-pixel `frame` uses every core.
    pub(crate) fn reaches_heavy(&mut self, keys: Vec<types::Key>) -> bool {
        let mut seen: Vec<types::Key> = Vec::new();
        let mut todo = keys.clone();
        while let Some(k) = todo.pop() {
            if seen.contains(&k) {
                continue;
            }
            if let FnKind::User(d) = &self.p.fns[k.0].kind
                && array_work_in(|f| d.body.walk(f))
            {
                return true;
            }
            if let Ok(spec) = self.spec(&k) {
                todo.extend(spec.calls.values().cloned());
            }
            seen.push(k);
        }
        self.reaches_recursion(keys)
    }

    /// Run a map kernel `k` over `0..n` timed: the first element alone, then
    /// the rest on the thread pool (one element per task) when they would
    /// take longer than waking it costs, else here.
    fn timed_apply(&mut self, n: IntValue<'ctx>, k: FunctionValue<'ctx>, ctx: PointerValue<'ctx>) -> R<()> {
        self.call_rt("mim_par_for_timed", &[n.into(), self.fn_addr(k).into(), ctx.into()])?;
        Ok(())
    }

    /// Materialise a fusible array expression with one fused pass.
    pub(crate) fn fused_map(&mut self, e: &Expr) -> R<V<'ctx>> {
        self.fused_at.set(Some(e.span));
        self.note(e.span, "fused", "the whole chain of array operations is one loop, with no arrays in between; it is split over the cores when large, and uses SIMD where the CPU has it".into());
        let el = num_el(self.ty_of(e)).ok_or("internal compiler error: fused_map on a non-numeric array")?;
        let mut plan = Plan::default();
        let root = self.build(e, &mut plan)?;
        let n = self.plan_len(&plan)?;
        // elements that may each be much work: timed, then shared out
        if heavy(&root) {
            let mode = Mode::Map(el);
            let k = self.emit_kernel(&root, &plan, mode)?;
            let out = match super::fixed(self.ty_of(e)) {
                Some((_, len)) => {
                    let at = self.el_ty(el).array_type(len);
                    let buf = self.entry_alloca_ty(at.into(), "fixed")?;
                    let ctx = self.pack_ctx(&plan, &[buf.into(), self.ctx.i64_type().const_zero().into()])?;
                    self.timed_apply(n, k, ctx)?;
                    let a = self.b.build_load(at, buf, "").s()?.into_array_value();
                    V::A(a, el)
                }
                None => {
                    let out = self.alloc_vec(dtype_of(el), n)?;
                    let data = self.data_of(out)?;
                    self.gpu_or(&root, &plan, el, n, data, |cg| {
                        let ctx = cg.pack_ctx(&plan, &[data, cg.ctx.i64_type().const_zero().into()])?;
                        cg.timed_apply(n, k, ctx)
                    })?;
                    out
                }
            };
            self.release_plan(&plan)?;
            return Ok(out);
        }
        // a result of known length: written to the stack, kept in registers
        if let Some((_, len)) = super::fixed(self.ty_of(e)) {
            let at = self.el_ty(el).array_type(len);
            let buf = self.entry_alloca_ty(at.into(), "fixed")?;
            let mode = Mode::Map(el);
            let n = self.ctx.i64_type().const_int(len as u64, false);
            self.inline_loop(&root, &plan, n, mode, buf.into())?;
            self.release_plan(&plan)?;
            let a = self.b.build_load(at, buf, "").s()?.into_array_value();
            return Ok(V::A(a, el));
        }
        let out = self.alloc_vec(dtype_of(el), n)?;
        let data = self.data_of(out)?;
        let mode = Mode::Map(el);
        self.small_or_kernel(&root, &plan, n, mode, data, |cg| {
            cg.gpu_or(&root, &plan, el, n, data, |cg| {
                let k = cg.emit_kernel(&root, &plan, mode)?;
                let ctx = cg.pack_ctx(&plan, &[data, cg.ctx.i64_type().const_zero().into()])?;
                cg.par_for(n, k, ctx)
            })?;
            Ok(None)
        })?;
        self.release_plan(&plan)?;
        Ok(out)
    }

    /// A fusible list of a length the program states (`⍳ 9 ¨ f`): its
    /// `len` elements written to a stack buffer, no heap list.
    pub(crate) fn fused_to_stack(&mut self, e: &Expr, el: El, len: u32) -> R<PointerValue<'ctx>> {
        let mut plan = Plan::default();
        let root = self.build(e, &mut plan)?;
        let at = self.el_ty(el).array_type(len);
        let buf = self.entry_alloca_ty(at.into(), "stack_list")?;
        let n = self.ctx.i64_type().const_int(len as u64, false);
        self.inline_loop(&root, &plan, n, Mode::Map(el), buf.into())?;
        self.release_plan(&plan)?;
        Ok(buf)
    }

    /// `bytes::from(e)` over a fusible integer expression: one pass that
    /// writes the bytes, checking each (no list of i64 in between).
    pub(crate) fn fused_bytes(&mut self, e: &Expr) -> R<V<'ctx>> {
        self.fused_at.set(Some(e.span));
        self.note(e.span, "fused", "the whole chain of array operations is one loop writing the bytes, with no arrays in between; it is split over the cores when large".into());
        let mut plan = Plan::default();
        let root = self.build(e, &mut plan)?;
        let n = self.plan_len(&plan)?;
        let el = El::U8;
        let out = self.alloc_vec(dtype_of(el), n)?;
        let data = self.data_of(out)?;
        let mode = Mode::Map(el);
        if heavy(&root) {
            let k = self.emit_kernel(&root, &plan, mode)?;
            let ctx = self.pack_ctx(&plan, &[data, self.ctx.i64_type().const_zero().into()])?;
            self.timed_apply(n, k, ctx)?;
        } else {
            self.small_or_kernel(&root, &plan, n, mode, data, |cg| {
                let k = cg.emit_kernel(&root, &plan, mode)?;
                let ctx = cg.pack_ctx(&plan, &[data, cg.ctx.i64_type().const_zero().into()])?;
                cg.par_for(n, k, ctx)?;
                Ok(None)
            })?;
        }
        self.release_plan(&plan)?;
        Ok(out)
    }

    /// `xs ¨ f` making records (see each_obj_fusible).
    pub(crate) fn fused_each_obj(&mut self, e: &Expr) -> R<V<'ctx>> {
        self.fused_at.set(Some(e.span));
        self.note(e.span, "fused", "`¨` building records is one loop, with no arrays in between; it is split over the cores when large, and uses SIMD where the CPU has it".into());
        let Ty::Arr(el @ El::Obj(_), _) = self.ty_of(e) else { return Err("internal compiler error: not a list of records".into()) };
        let mut plan = Plan::default();
        let root = self.build(e, &mut plan)?;
        let n = self.plan_len(&plan)?;
        let out = self.alloc_vec(4, n)?; // MIM_BOXED: a list of handles
        let data = self.data_of(out)?;
        let mode = Mode::Map(el);
        let k = self.emit_kernel(&root, &plan, mode)?;
        let ctx = self.pack_ctx(&plan, &[data, self.ctx.i64_type().const_zero().into()])?;
        if heavy(&root) {
            self.timed_apply(n, k, ctx)?;
        } else {
            self.par_for(n, k, ctx)?;
        }
        self.release_plan(&plan)?;
        Ok(out)
    }

    /// Bytes per element of a kernel's vector.
    fn el_bytes(el: El) -> u64 {
        match el {
            El::F32 | El::I32 => 4,
            El::Bool | El::U8 => 1,
            _ => 8,
        }
    }

    /// A map into `data` (n elements of `el`): on the GPU when the runtime
    /// wants it there (gpu.rs; macOS), else `cpu`.
    /// Are GPU kernels made in this build? (macOS, optimised.) The GPU
    /// never fuses a multiply and an add: its results are those of
    /// --strict-math exactly.
    pub(crate) fn gpu_wanted(&self) -> bool {
        let triple = self.module.get_triple().as_str().to_string_lossy().into_owned();
        let mac = triple.contains("apple") && (triple.contains("macos") || triple.contains("darwin")) && !triple.contains("macabi");
        mac && self.opt_level >= 2
    }

    fn gpu_or(&mut self, root: &Node, plan: &Plan<'ctx>, el: El, n: IntValue<'ctx>, data: BasicValueEnum<'ctx>, cpu: impl FnOnce(&mut Self) -> R<()>) -> R<()> {
        let gk = if self.gpu_wanted() { gpu::map_kernel(root, el, plan.slots.len()) } else { None };
        let Some(gk) = gk else { return cpu(self) };
        if let Some(sp) = self.fused_at.get() {
            self.note(sp, "gpu", "also compiled for the GPU (Metal): it runs there when the arrays are large enough to pay for the copy, on the CPU otherwise".into());
        }
        let i64t = self.ctx.i64_type();
        let i32t = self.ctx.i32_type();
        let f = self.cur_fn.unwrap();
        let (gb, cb, done) = (self.ctx.append_basic_block(f, "gpu"), self.ctx.append_basic_block(f, "cpu"), self.ctx.append_basic_block(f, "gpu_done"));
        let wants = self.call_rt("mim_gpu_wants", &[n.into(), i64t.const_int(gk.cost as u64, false).into()])?.unwrap().into_int_value();
        let mut w = self.b.build_int_compare(IntPredicate::NE, wants, i32t.const_zero(), "").s()?;
        // divisors the CPU would check: anything doubtful stays there
        for s in &gk.guards {
            let v = plan.slots[*s].into_int_value();
            let v = if v.get_type().get_bit_width() < 64 { self.b.build_int_s_extend(v, i64t, "").s()? } else { v };
            let nz = self.b.build_int_compare(IntPredicate::NE, v, i64t.const_zero(), "").s()?;
            let nm = self.b.build_int_compare(IntPredicate::NE, v, i64t.const_all_ones(), "").s()?;
            w = self.b.build_and(w, nz, "").s()?;
            w = self.b.build_and(w, nm, "").s()?;
        }
        self.b.build_conditional_branch(w, gb, cb).s()?;
        self.b.position_at_end(gb);
        let nb = gk.bufs.len() as u32;
        let ins_ty = self.ptr.array_type(nb.max(1));
        let ins = self.entry_array_of(ins_ty)?;
        let bytes_ty = i64t.array_type(nb.max(1));
        let bytes = self.entry_array_of(bytes_ty)?;
        for (k, (slot, lel)) in gk.bufs.iter().enumerate() {
            let at = |cg: &Self, ty: inkwell::types::ArrayType<'ctx>, base: PointerValue<'ctx>| unsafe { cg.b.build_in_bounds_gep(ty, base, &[i64t.const_zero(), i64t.const_int(k as u64, false)], "") };
            let p = at(self, ins_ty, ins).s()?;
            self.b.build_store(p, plan.slots[*slot]).s()?;
            let len = plan.leaf_lens.iter().find(|(s, _)| s == slot).map(|(_, l)| *l).ok_or("internal compiler error: GPU leaf without a length")?;
            let sz = self.b.build_int_mul(len, i64t.const_int(Self::el_bytes(*lel), false), "").s()?;
            let p = at(self, bytes_ty, bytes).s()?;
            self.b.build_store(p, sz).s()?;
        }
        let na = plan.slots.len() as u32;
        let args_ty = i64t.array_type(na.max(1));
        let args = self.entry_array_of(args_ty)?;
        for (s, a) in gk.args.iter().enumerate() {
            let v: IntValue = match a {
                gpu::Arg::None => continue,
                gpu::Arg::Raw => {
                    let v = plan.slots[s].into_int_value();
                    match v.get_type().get_bit_width() {
                        1 => self.b.build_int_z_extend(v, i64t, "").s()?,
                        64 => v,
                        _ => self.b.build_int_s_extend(v, i64t, "").s()?,
                    }
                }
                gpu::Arg::F32 => {
                    let d = plan.slots[s].into_float_value();
                    let f = if d.get_type() == self.ctx.f32_type() { d } else { self.b.build_float_trunc(d, self.ctx.f32_type(), "").s()? };
                    let bits = self.b.build_bit_cast(f, i32t, "").s()?.into_int_value();
                    self.b.build_int_z_extend(bits, i64t, "").s()?
                }
            };
            let p = unsafe { self.b.build_in_bounds_gep(args_ty, args, &[i64t.const_zero(), i64t.const_int(s as u64, false)], "").s()? };
            self.b.build_store(p, v).s()?;
        }
        let src = self.cstring(&gk.src)?;
        let out_bytes = self.b.build_int_mul(n, i64t.const_int(Self::el_bytes(el), false), "").s()?;
        let ok = self
            .call_rt("mim_gpu_run", &[src.into(), n.into(), i32t.const_int(nb as u64, false).into(), ins.into(), bytes.into(), args.into(), i32t.const_int(na as u64, false).into(), data.into(), out_bytes.into()])?
            .unwrap()
            .into_int_value();
        let ok = self.b.build_int_compare(IntPredicate::NE, ok, i32t.const_zero(), "").s()?;
        self.b.build_conditional_branch(ok, done, cb).s()?;
        self.b.position_at_end(cb);
        cpu(self)?;
        self.b.build_unconditional_branch(done).s()?;
        self.b.position_at_end(done);
        Ok(())
    }

    /// Small loops (up to SMALL_LOOP elements) run inline in the calling
    /// function: no kernel, no context, nothing between the iterations, so
    /// LLVM can unroll them and keep values in registers. Larger ones run
    /// `kernel`. A known length picks one; an unknown one branches.
    /// Returns the reduction's result (None for a map).
    fn small_or_kernel(
        &mut self,
        root: &Node,
        plan: &Plan<'ctx>,
        n: IntValue<'ctx>,
        mode: Mode,
        out: BasicValueEnum<'ctx>,
        kernel: impl FnOnce(&mut Self) -> R<Option<V<'ctx>>>,
    ) -> R<Option<V<'ctx>>> {
        const SMALL_LOOP: u64 = 64;
        match n.get_sign_extended_constant() {
            Some(c) if c >= 0 && (c as u64) <= SMALL_LOOP => return self.inline_loop(root, plan, n, mode, out),
            Some(_) => return kernel(self),
            None => {}
        }
        let i64t = self.ctx.i64_type();
        let f = self.cur_fn.unwrap();
        let small = self.b.build_int_compare(IntPredicate::ULE, n, i64t.const_int(SMALL_LOOP, false), "").s()?;
        let a = self.ctx.append_basic_block(f, "small");
        let k = self.ctx.append_basic_block(f, "large");
        let j = self.ctx.append_basic_block(f, "fused_done");
        self.b.build_conditional_branch(small, a, k).s()?;
        self.b.position_at_end(a);
        let va = self.inline_loop(root, plan, n, mode, out)?;
        let a_end = self.b.get_insert_block().unwrap();
        self.b.build_unconditional_branch(j).s()?;
        self.b.position_at_end(k);
        let vk = kernel(self)?;
        let k_end = self.b.get_insert_block().unwrap();
        self.b.build_unconditional_branch(j).s()?;
        self.b.position_at_end(j);
        Ok(match (va, vk) {
            (Some(x), Some(y)) => {
                let bx = self.basic(x);
                let phi = self.b.build_phi(bx.get_type(), "red").s()?;
                phi.add_incoming(&[(&bx, a_end), (&self.basic(y), k_end)]);
                let r = phi.as_basic_value();
                Some(if r.is_float_value() { V::F(r.into_float_value()) } else { V::I(r.into_int_value()) })
            }
            _ => None,
        })
    }

    /// Ask LLVM to unroll the loop whose back edge is `br` completely
    /// (`!llvm.loop !{self, !{"llvm.loop.unroll.full"}}`: a self-referential
    /// node, built through LLVM's C API).
    fn unroll_fully(&self, br: inkwell::values::InstructionValue<'ctx>) {
        use inkwell::values::AsValueRef;
        use std::ffi::c_void;
        unsafe extern "C" {
            fn LLVMMDStringInContext2(c: *mut c_void, s: *const std::ffi::c_char, n: usize) -> *mut c_void;
            fn LLVMMDNodeInContext2(c: *mut c_void, mds: *mut *mut c_void, n: usize) -> *mut c_void;
            fn LLVMTemporaryMDNode(c: *mut c_void, mds: *mut *mut c_void, n: usize) -> *mut c_void;
            fn LLVMMetadataReplaceAllUsesWith(tmp: *mut c_void, md: *mut c_void);
            fn LLVMMetadataAsValue(c: *mut c_void, md: *mut c_void) -> *mut c_void;
            fn LLVMSetMetadata(v: *mut c_void, kind: u32, md: *mut c_void);
        }
        let c = self.ctx.raw() as *mut c_void;
        let kind = self.ctx.get_kind_id("llvm.loop");
        let name = "llvm.loop.unroll.full";
        unsafe {
            let s = LLVMMDStringInContext2(c, name.as_ptr() as *const _, name.len());
            let mut ops = [s];
            let full = LLVMMDNodeInContext2(c, ops.as_mut_ptr(), 1);
            let tmp = LLVMTemporaryMDNode(c, std::ptr::null_mut(), 0);
            let mut ops = [tmp, full];
            let node = LLVMMDNodeInContext2(c, ops.as_mut_ptr(), 2);
            LLVMMetadataReplaceAllUsesWith(tmp, node);
            LLVMSetMetadata(br.as_value_ref() as *mut c_void, kind, LLVMMetadataAsValue(c, node));
        }
    }

    /// out[i] = v, for a map's output of element type `el`.
    fn store_elem(&mut self, out: BasicValueEnum<'ctx>, el: El, i: IntValue<'ctx>, v: V<'ctx>) -> R<()> {
        let i64t = self.ctx.i64_type();
        let base = out.into_pointer_value();
        match el {
            El::F64 => {
                let p = unsafe { self.b.build_in_bounds_gep(self.ctx.f64_type(), base, &[i], "").s()? };
                self.b.build_store(p, self.to_f64(v)?).s()?;
            }
            El::F32 => {
                let p = unsafe { self.b.build_in_bounds_gep(self.ctx.f32_type(), base, &[i], "").s()? };
                let d = self.to_f64(v)?;
                self.b.build_store(p, self.b.build_float_trunc(d, self.ctx.f32_type(), "").s()?).s()?;
            }
            El::I32 => {
                let p = unsafe { self.b.build_in_bounds_gep(self.ctx.i32_type(), base, &[i], "").s()? };
                let n = self.to_i64(v)?;
                self.b.build_store(p, self.b.build_int_truncate(n, self.ctx.i32_type(), "").s()?).s()?;
            }
            El::Bool => {
                let p = unsafe { self.b.build_in_bounds_gep(self.ctx.i8_type(), base, &[i], "").s()? };
                let t = self.truth(v)?;
                self.b.build_store(p, self.b.build_int_z_extend(t, self.ctx.i8_type(), "").s()?).s()?;
            }
            // a byte (bytes::from): 0 … 255, else the program stops
            El::U8 => {
                let n = self.to_i64(v)?;
                let bad = self.b.build_int_compare(IntPredicate::UGT, n, i64t.const_int(255, false), "").s()?;
                let f = self.cur_fn.unwrap();
                let fail = self.ctx.append_basic_block(f, "not_byte");
                let ok = self.ctx.append_basic_block(f, "byte");
                self.b.build_conditional_branch(bad, fail, ok).s()?;
                self.b.position_at_end(fail);
                self.call_rt("mim_byte_fail", &[n.into(), i.into()])?;
                self.b.build_unreachable().s()?;
                self.b.position_at_end(ok);
                let p = unsafe { self.b.build_in_bounds_gep(self.ctx.i8_type(), base, &[i], "").s()? };
                self.b.build_store(p, self.b.build_int_truncate(n, self.ctx.i8_type(), "").s()?).s()?;
            }
            // a record: the list takes a reference of its own
            El::Obj(_) => {
                let p = unsafe { self.b.build_in_bounds_gep(self.ptr, base, &[i], "").s()? };
                let b = self.boxed(v)?;
                let b = self.own(b)?;
                self.b.build_store(p, self.basic(b)).s()?;
            }
            _ => {
                let p = unsafe { self.b.build_in_bounds_gep(i64t, base, &[i], "").s()? };
                self.b.build_store(p, self.to_i64(v)?).s()?;
            }
        }
        Ok(())
    }

    /// The loop itself, in the current function, over `0..n`.
    fn inline_loop(&mut self, root: &Node, plan: &Plan<'ctx>, n: IntValue<'ctx>, mode: Mode, out: BasicValueEnum<'ctx>) -> R<Option<V<'ctx>>> {
        let i64t = self.ctx.i64_type();
        let f = self.cur_fn.unwrap();
        let float = match mode {
            Mode::Reduce(_, fl) => fl,
            _ => false,
        };
        let acc_ty: BasicTypeEnum = if float { self.ctx.f64_type().into() } else { i64t.into() };
        let pre = self.b.get_insert_block().unwrap();
        let body = self.ctx.append_basic_block(f, "sm_body");
        let done = self.ctx.append_basic_block(f, "sm_done");
        let none = self.b.build_int_compare(IntPredicate::SLE, n, i64t.const_zero(), "").s()?;
        self.b.build_conditional_branch(none, done, body).s()?;
        self.b.position_at_end(body);
        let iphi = self.b.build_phi(i64t, "si").s()?;
        let aphi = self.b.build_phi(acc_ty, "sacc").s()?;
        let i = iphi.as_basic_value().into_int_value();
        let slots = plan.slots.clone();
        let v = self.eval_node(root, i, &slots)?;
        let acc = aphi.as_basic_value();
        let next: BasicValueEnum = match mode {
            Mode::Map(el) => {
                self.store_elem(out, el, i, v)?;
                acc
            }
            Mode::Reduce(prim, _) => self.combine(prim, float, acc, v)?,
            Mode::Scan(..) => return Err("internal compiler error: inline scan".into()),
        };
        let i1 = self.b.build_int_add(i, i64t.const_int(1, false), "").s()?;
        let more = self.b.build_int_compare(IntPredicate::SLT, i1, n, "").s()?;
        let latch = self.b.get_insert_block().unwrap();
        let back = self.b.build_conditional_branch(more, body, done).s()?;
        // a short loop of known length: LLVM writes it out once everything is
        // inlined (indices become constants, element choices fold away)
        if n.get_sign_extended_constant().is_some_and(|c| (0..=32).contains(&c)) {
            self.unroll_fully(back);
        }
        let ident = self.identity(mode, float);
        iphi.add_incoming(&[(&i64t.const_zero(), pre), (&i1, latch)]);
        aphi.add_incoming(&[(&ident, pre), (&next, latch)]);
        self.b.position_at_end(done);
        let r = self.b.build_phi(acc_ty, "sred").s()?;
        r.add_incoming(&[(&ident, pre), (&next, latch)]);
        Ok(match mode {
            Mode::Reduce(..) => {
                let r = r.as_basic_value();
                Some(if float { V::F(r.into_float_value()) } else { V::I(r.into_int_value()) })
            }
            _ => None,
        })
    }

    /// `xs ⌿ mask` with numbers on the left, truth values on the right, and
    /// at least one side an expression to fuse: counted, then filled, from
    /// one evaluation of each side's parts (no mask or values list built).
    pub(crate) fn keep_list_fusible(&self, e: &Expr) -> bool {
        let ExprKind::Prim { prim: Prim::Keep, args } = &e.kind else { return false };
        args.len() == 2
            && matches!(num_el(self.ty_of(&args[0])), Some(El::I64 | El::F64))
            && num_el(self.ty_of(&args[1])) == Some(El::Bool)
            && (self.is_fae(&args[0]) || self.is_fae(&args[1]))
            && num_el(self.ty_of(e)).is_some()
    }

    pub(crate) fn fused_keep_list(&mut self, e: &Expr) -> R<V<'ctx>> {
        self.fused_at.set(Some(e.span));
        self.note(e.span, "fused", "filtering is fused into one loop, with no arrays in between; it is split over the cores when large, and uses SIMD where the CPU has it".into());
        let ExprKind::Prim { args, .. } = &e.kind else { return Err("internal compiler error: not a keep".into()) };
        let el = num_el(self.ty_of(e)).ok_or("internal compiler error: keep of non-numbers")?;
        let i64t = self.ctx.i64_type();
        let mut plan = Plan::default();
        let xs = self.operand(&args[0], &mut plan)?;
        let lx = self.common_len(&plan.lens)?;
        let k = plan.lens.len();
        let m = self.operand(&args[1], &mut plan)?;
        let lm = self.common_len(&plan.lens[k..])?;
        let ne = self.b.build_int_compare(IntPredicate::NE, lx, lm, "").s()?;
        self.panic_if(ne, "⌿ (keep): length mismatch")?;
        // 1. how many are kept
        let count_root = Node::Sel(Box::new(m.clone()), Box::new(Node::ConstI(1)), Box::new(Node::ConstI(0)));
        let unit = i64t.const_zero().into();
        let count = self.inline_loop(&count_root, &plan, lx, Mode::Reduce(Prim::Sum, false), unit)?.ok_or("internal compiler error: keep count")?;
        let V::I(count) = count else { return Err("internal compiler error: keep count".into()) };
        // 2. those, in order
        let out = self.alloc_vec(dtype_of(el), count)?;
        let data = self.data_of(out)?;
        let f = self.cur_fn.unwrap();
        let pre = self.b.get_insert_block().unwrap();
        let body = self.ctx.append_basic_block(f, "keep_body");
        let put = self.ctx.append_basic_block(f, "keep_put");
        let next = self.ctx.append_basic_block(f, "keep_next");
        let done = self.ctx.append_basic_block(f, "keep_done");
        let none = self.b.build_int_compare(IntPredicate::SLE, lx, i64t.const_zero(), "").s()?;
        self.b.build_conditional_branch(none, done, body).s()?;
        self.b.position_at_end(body);
        let iphi = self.b.build_phi(i64t, "ki").s()?;
        let kphi = self.b.build_phi(i64t, "kk").s()?;
        let (i, kk) = (iphi.as_basic_value().into_int_value(), kphi.as_basic_value().into_int_value());
        let slots = plan.slots.clone();
        let c = self.eval_node(&m, i, &slots)?;
        let c = self.truth(c)?;
        let test_end = self.b.get_insert_block().unwrap(); // (the mask may add blocks)
        self.b.build_conditional_branch(c, put, next).s()?;
        self.b.position_at_end(put);
        let v = self.eval_node(&xs, i, &slots)?;
        self.store_elem(data, el, kk, v)?;
        let k1 = self.b.build_int_add(kk, i64t.const_int(1, false), "").s()?;
        let put_end = self.b.get_insert_block().unwrap();
        self.b.build_unconditional_branch(next).s()?;
        self.b.position_at_end(next);
        let kn = self.b.build_phi(i64t, "kn").s()?;
        let body_end = self.b.get_insert_block().unwrap();
        kn.add_incoming(&[(&kk, test_end), (&k1, put_end)]);
        let _ = body_end;
        let i1 = self.b.build_int_add(i, i64t.const_int(1, false), "").s()?;
        let more = self.b.build_int_compare(IntPredicate::SLT, i1, lx, "").s()?;
        let latch = self.b.get_insert_block().unwrap();
        self.b.build_conditional_branch(more, body, done).s()?;
        iphi.add_incoming(&[(&i64t.const_zero(), pre), (&i1, latch)]);
        kphi.add_incoming(&[(&i64t.const_zero(), pre), (&kn.as_basic_value(), latch)]);
        self.b.position_at_end(done);
        self.release_plan(&plan)?;
        Ok(out)
    }

    /// `+/ ×/ ≢` over `xs ⌿ mask` (a list of truth values): one pass, the
    /// filtered list never built.
    pub(crate) fn keep_fusible(&self, prim: Prim, arg: &Expr) -> bool {
        let ExprKind::Prim { prim: Prim::Keep, args } = &arg.kind else { return false };
        matches!(prim, Prim::Sum | Prim::Product | Prim::Len)
            && args.len() == 2
            && matches!(num_el(self.ty_of(&args[0])), Some(El::I64 | El::F64))
            && num_el(self.ty_of(&args[1])) == Some(El::Bool)
    }

    pub(crate) fn fused_reduce_keep(&mut self, prim: Prim, arg: &Expr) -> R<V<'ctx>> {
        self.fused_at.set(Some(arg.span));
        self.note(arg.span, "fused", "the filter and the reduction are one loop, with no arrays in between; it is split over the cores when large, and uses SIMD where the CPU has it".into());
        let ExprKind::Prim { args, .. } = &arg.kind else { return Err("internal compiler error: not a keep".into()) };
        let mut plan = Plan::default();
        let xs = self.operand(&args[0], &mut plan)?;
        let lx = self.common_len(&plan.lens)?;
        let k = plan.lens.len();
        let m = self.operand(&args[1], &mut plan)?;
        let lm = self.common_len(&plan.lens[k..])?;
        let ne = self.b.build_int_compare(IntPredicate::NE, lx, lm, "").s()?;
        self.panic_if(ne, "⌿ (keep): length mismatch")?;
        plan.lens = vec![lx];
        let float = num_el(self.ty_of(&args[0])) == Some(El::F64);
        let (root, prim, float) = match prim {
            // the count: how many truth values hold
            Prim::Len => (Node::Sel(Box::new(m), Box::new(Node::ConstI(1)), Box::new(Node::ConstI(0))), Prim::Sum, false),
            _ => {
                let unit = match (prim, float) {
                    (Prim::Product, true) => Node::ConstF(1.0),
                    (Prim::Product, false) => Node::ConstI(1),
                    (_, true) => Node::ConstF(0.0),
                    _ => Node::ConstI(0),
                };
                (Node::Sel(Box::new(m), Box::new(xs), Box::new(unit)), prim, float)
            }
        };
        self.fused_reduce_root(prim, root, plan, float)
    }

    /// A reduction (`+/ ×/ ⌈/ ⌊/ ≢`) over a fusible expression: no array is built.
    pub(crate) fn fused_reduce(&mut self, prim: Prim, arg: &Expr) -> R<V<'ctx>> {
        self.fused_at.set(Some(arg.span));
        self.note(arg.span, "fused", "the operations and the reduction are one loop, with no arrays in between; it is split over the cores when large, and uses SIMD where the CPU has it".into());
        let mut plan = Plan::default();
        let root = self.build(arg, &mut plan)?;
        let float = num_el(self.ty_of(arg)) == Some(El::F64);
        self.fused_reduce_root(prim, root, plan, float)
    }

    fn fused_reduce_root(&mut self, prim: Prim, root: Node, plan: Plan<'ctx>, float: bool) -> R<V<'ctx>> {
        let n = self.plan_len(&plan)?;
        if prim == Prim::Len {
            self.release_plan(&plan)?;
            return Ok(V::I(n));
        }
        let i64t = self.ctx.i64_type();
        let acc_ty: BasicTypeEnum = if float { self.ctx.f64_type().into() } else { i64t.into() };
        if matches!(prim, Prim::Maximum | Prim::Minimum) {
            let z = self.b.build_int_compare(IntPredicate::EQ, n, i64t.const_zero(), "").s()?;
            self.panic_if(z, "maximum/minimum of an empty array")?;
        }
        let mode = Mode::Reduce(prim, float);
        // elements that may each be much work: computed timed (and shared
        // out) into a buffer, then combined here in order
        if heavy(&root) {
            let el = if float { El::F64 } else { El::I64 };
            let k = self.emit_kernel(&root, &plan, Mode::Map(el))?;
            let tmp = self.alloc_vec(dtype_of(el), n)?;
            let data = self.data_of(tmp)?;
            // (the GPU computes the elements when it can; they are combined
            // here, in order, either way)
            self.gpu_or(&root, &plan, el, n, data, |cg| {
                let ctx = cg.pack_ctx(&plan, &[data, cg.ctx.i64_type().const_zero().into()])?;
                cg.timed_apply(n, k, ctx)
            })?;
            self.release_plan(&plan)?;
            let mut p2 = Plan::default();
            p2.lens.push(n);
            p2.slots.push(data);
            let leaf = Node::Leaf(0, el);
            let r = self.inline_loop(&leaf, &p2, n, mode, i64t.const_zero().into())?;
            self.drop_tmp(tmp)?;
            return r.ok_or_else(|| "internal compiler error: reduction".into());
        }
        let unit = self.ctx.i64_type().const_zero().into();
        let r = self.small_or_kernel(&root, &plan, n, mode, unit, |cg| {
            // one partial result per fixed-size chunk, combined in chunk order
            let grain = cg.grain()?;
            let g1 = cg.b.build_int_sub(grain, i64t.const_int(1, false), "").s()?;
            let nc = cg.b.build_int_add(n, g1, "").s()?;
            let nc = cg.b.build_int_signed_div(nc, grain, "").s()?;
            let bytes = cg.b.build_int_mul(nc, i64t.const_int(8, false), "").s()?;
            let bytes = cg.b.build_int_add(bytes, i64t.const_int(8, false), "").s()?;
            let (partials, heap) = cg.partials_buf(nc, bytes)?;
            let k = cg.emit_kernel(&root, &plan, Mode::Reduce(prim, float))?;
            let ctx = cg.pack_ctx(&plan, &[partials.into(), grain.into()])?;
            cg.par_for(n, k, ctx)?;
            // combine the partials in order
            let f = cg.cur_fn.unwrap();
            let pre = cg.b.get_insert_block().unwrap();
            let body = cg.ctx.append_basic_block(f, "comb");
            let done = cg.ctx.append_basic_block(f, "comb_done");
            let ident = cg.identity(Mode::Reduce(prim, float), float);
            let none = cg.b.build_int_compare(IntPredicate::EQ, nc, i64t.const_zero(), "").s()?;
            cg.b.build_conditional_branch(none, done, body).s()?;
            cg.b.position_at_end(body);
            let kphi = cg.b.build_phi(i64t, "k").s()?;
            let aphi = cg.b.build_phi(acc_ty, "a").s()?;
            let kk = kphi.as_basic_value().into_int_value();
            let p = unsafe { cg.b.build_in_bounds_gep(acc_ty, partials, &[kk], "").s()? };
            let part = cg.b.build_load(acc_ty, p, "").s()?;
            let pv = if float { V::F(part.into_float_value()) } else { V::I(part.into_int_value()) };
            let next = cg.combine(prim, float, aphi.as_basic_value(), pv)?;
            let k1 = cg.b.build_int_add(kk, i64t.const_int(1, false), "").s()?;
            let more = cg.b.build_int_compare(IntPredicate::SLT, k1, nc, "").s()?;
            let latch = cg.b.get_insert_block().unwrap();
            cg.b.build_conditional_branch(more, body, done).s()?;
            kphi.add_incoming(&[(&i64t.const_zero(), pre), (&k1, latch)]);
            aphi.add_incoming(&[(&ident, pre), (&next, latch)]);
            cg.b.position_at_end(done);
            let res = cg.b.build_phi(acc_ty, "red").s()?;
            res.add_incoming(&[(&ident, pre), (&next, latch)]);
            cg.free_partials(partials, heap)?;
            let r = res.as_basic_value();
            Ok(Some(if float { V::F(r.into_float_value()) } else { V::I(r.into_int_value()) }))
        })?;
        self.release_plan(&plan)?;
        Ok(r.ok_or("internal compiler error: reduction without a result")?)
    }

    /// `xs ▷ trail(+)` (or × max min) over a fusible expression: one kernel
    /// sums each chunk, the sums become each chunk's starting offset, and a
    /// second kernel writes the running results. No input array is built.
    pub(crate) fn fused_trail(&mut self, prim: Prim, arg: &Expr) -> R<V<'ctx>> {
        self.fused_at.set(Some(arg.span));
        self.note(arg.span, "fused", "the running results are one parallel scan, with no arrays in between; it is split over the cores when large, and uses SIMD where the CPU has it".into());
        let el = num_el(self.ty_of(arg)).ok_or("internal compiler error: fused_trail on a non-numeric array")?;
        let float = el == El::F64;
        let i64t = self.ctx.i64_type();
        let acc_ty: BasicTypeEnum = if float { self.ctx.f64_type().into() } else { i64t.into() };
        let mut plan = Plan::default();
        let root = self.build(arg, &mut plan)?;
        let n = self.plan_len(&plan)?;
        let out = self.alloc_vec(dtype_of(el), n)?;
        let data = self.data_of(out)?;
        // chunks 16 times the usual: a scan writes all it reads, and many
        // small chunks first-touched by turns on different threads made the
        // parallel scan slower than one thread on Windows (and no faster
        // elsewhere); fixed, so float results do not depend on the threads
        let grain = self.grain()?;
        let grain = self.b.build_int_mul(grain, i64t.const_int(16, false), "scan_grain").s()?;
        let g1 = self.b.build_int_sub(grain, i64t.const_int(1, false), "").s()?;
        let nc = self.b.build_int_add(n, g1, "").s()?;
        let nc = self.b.build_int_signed_div(nc, grain, "").s()?;
        let bytes = self.b.build_int_mul(nc, i64t.const_int(8, false), "").s()?;
        let bytes = self.b.build_int_add(bytes, i64t.const_int(8, false), "").s()?;
        let (parts, heap) = self.partials_buf(nc, bytes)?;
        let f = self.cur_fn.unwrap();
        // one chunk (one core, or a short list): it starts from nothing, so
        // the first pass (each chunk's total) is skipped
        let ident = self.identity(Mode::Reduce(prim, float), float);
        let one = self.b.build_int_compare(IntPredicate::SLE, nc, i64t.const_int(1, false), "").s()?;
        let th = self.threads()?;
        let one_thread = self.b.build_int_compare(IntPredicate::SLE, th, i64t.const_int(1, false), "").s()?;
        let one = self.b.build_or(one, one_thread, "").s()?;
        let single = self.ctx.append_basic_block(f, "scan_one");
        let multi = self.ctx.append_basic_block(f, "scan_many");
        let scan = self.ctx.append_basic_block(f, "scan");
        self.b.build_conditional_branch(one, single, multi).s()?;
        self.b.position_at_end(single);
        self.b.build_store(parts, ident).s()?;
        self.b.build_unconditional_branch(scan).s()?;
        self.b.position_at_end(multi);
        // 1: each chunk's own total
        let k1 = self.emit_kernel(&root, &plan, Mode::Reduce(prim, float))?;
        let ctx1 = self.pack_ctx(&plan, &[parts.into(), grain.into()])?;
        self.par_for_grain(n, grain, k1, ctx1)?;
        // 2: totals → starting offsets (in place, in chunk order)
        let pre = self.b.get_insert_block().unwrap();
        let body = self.ctx.append_basic_block(f, "offs");
        let done = self.ctx.append_basic_block(f, "offs_done");
        let none = self.b.build_int_compare(IntPredicate::EQ, nc, i64t.const_zero(), "").s()?;
        self.b.build_conditional_branch(none, done, body).s()?;
        self.b.position_at_end(body);
        let kphi = self.b.build_phi(i64t, "k").s()?;
        let rphi = self.b.build_phi(acc_ty, "run").s()?;
        let kk = kphi.as_basic_value().into_int_value();
        let p = unsafe { self.b.build_in_bounds_gep(acc_ty, parts, &[kk], "").s()? };
        let total = self.b.build_load(acc_ty, p, "").s()?;
        self.b.build_store(p, rphi.as_basic_value()).s()?;
        let tv = if float { V::F(total.into_float_value()) } else { V::I(total.into_int_value()) };
        let next = self.combine(prim, float, rphi.as_basic_value(), tv)?;
        let k1n = self.b.build_int_add(kk, i64t.const_int(1, false), "").s()?;
        let more = self.b.build_int_compare(IntPredicate::SLT, k1n, nc, "").s()?;
        let latch = self.b.get_insert_block().unwrap();
        self.b.build_conditional_branch(more, body, done).s()?;
        kphi.add_incoming(&[(&i64t.const_zero(), pre), (&k1n, latch)]);
        rphi.add_incoming(&[(&ident, pre), (&next, latch)]);
        self.b.position_at_end(done);
        self.b.build_unconditional_branch(scan).s()?;
        self.b.position_at_end(scan);
        // 3: the running results (one thread: the whole list is one chunk)
        let k2 = self.emit_kernel(&root, &plan, Mode::Scan(prim, el))?;
        let sgrain = self.b.build_select(one, n, grain, "").s()?.into_int_value();
        let sgrain = self.b.build_select(self.b.build_int_compare(IntPredicate::SGT, sgrain, i64t.const_zero(), "").s()?, sgrain, grain, "").s()?.into_int_value();
        let ctx2 = self.pack_ctx(&plan, &[data, sgrain.into(), parts.into()])?;
        self.scan_for(n, grain, k2, ctx2, one)?;
        self.release_plan(&plan)?;
        self.free_partials(parts, heap)?;
        Ok(out)
    }

    /// The elements of a vector just allocated: a new vector keeps them
    /// right after its header (alloc in mimic_rt.c), so this is an address,
    /// not a load, and LLVM sees they belong to the new vector alone.
    fn data_of(&mut self, v: V<'ctx>) -> R<BasicValueEnum<'ctx>> {
        let vt = self.val_ty();
        let p = self.basic(v).into_pointer_value();
        Ok(unsafe { self.b.build_in_bounds_gep(vt, p, &[self.ctx.i32_type().const_int(1, false)], "data").s()? }.into())
    }

    /// The runtime's chunk size (a global it sets once).
    /// The runtime's thread count (a global it sets once).
    fn threads(&mut self) -> R<IntValue<'ctx>> {
        let i64t = self.ctx.i64_type();
        let g = self.module.get_global("mim_threads").unwrap_or_else(|| {
            let g = self.module.add_global(i64t, None, "mim_threads");
            g.set_linkage(Linkage::External);
            g
        });
        let p = self.gaddr(g);
        Ok(self.b.build_load(i64t, p, "threads").s()?.into_int_value())
    }

    fn grain(&mut self) -> R<IntValue<'ctx>> {
        let i64t = self.ctx.i64_type();
        let g = self.module.get_global("mim_grain").unwrap_or_else(|| {
            let g = self.module.add_global(i64t, None, "mim_grain");
            g.set_linkage(Linkage::External);
            g
        });
        let p = self.gaddr(g);
        Ok(self.b.build_load(i64t, p, "grain").s()?.into_int_value())
    }

    /// Length and data of a vector a kernel reads: straight from its header
    /// when it is a vector of `el`; anything else is reported by the runtime.
    fn vec_header(&mut self, v: V<'ctx>, el: El) -> R<(IntValue<'ctx>, BasicValueEnum<'ctx>)> {
        let i64t = self.ctx.i64_type();
        let i32t = self.ctx.i32_type();
        let hdr = self.ctx.struct_type(&[i64t.into(), i32t.into(), i32t.into(), i64t.into(), self.ptr.into()], false);
        let p = self.basic(v).into_pointer_value();
        // a vector's header never changes while it is alive (as in index_fast)
        let invariant = self.ctx.get_kind_id("invariant.load");
        let empty = self.ctx.metadata_node(&[]);
        let load = |cg: &Self, k: u32, t: BasicTypeEnum<'ctx>| -> R<BasicValueEnum<'ctx>> {
            let fp = cg.b.build_struct_gep(hdr, p, k, "").s()?;
            let v = cg.b.build_load(t, fp, "").s()?;
            inkwell::values::BasicValue::as_instruction_value(&v).unwrap().set_metadata(empty, invariant).map_err(|e| e.to_string())?;
            Ok(v)
        };
        let dtype = load(self, 1, i32t.into())?.into_int_value();
        let rank = load(self, 2, i32t.into())?.into_int_value();
        // a list of records holds handles (an empty list has no elements to read)
        let want = if matches!(el, El::Obj(_)) { 4 } else { dtype_of(el) };
        let mut ok_t = self.b.build_int_compare(IntPredicate::EQ, dtype, i32t.const_int(want as u64, false), "").s()?;
        if want == 4 {
            let n = load(self, 3, i64t.into())?.into_int_value();
            let empty = self.b.build_int_compare(IntPredicate::EQ, n, i64t.const_zero(), "").s()?;
            ok_t = self.b.build_or(ok_t, empty, "").s()?;
        }
        let ok_r = self.b.build_int_compare(IntPredicate::EQ, rank, i32t.const_int(1, false), "").s()?;
        let ok = self.b.build_and(ok_t, ok_r, "").s()?;
        let f = self.cur_fn.unwrap();
        let bad = self.ctx.append_basic_block(f, "vec_bad");
        let good = self.ctx.append_basic_block(f, "vec_ok");
        self.b.build_conditional_branch(ok, good, bad).s()?;
        self.b.position_at_end(bad);
        self.call_rt("mim_vec_len", &[p.into(), self.i32c(want)])?; // reports it
        self.b.build_unreachable().s()?;
        self.b.position_at_end(good);
        let len = load(self, 3, i64t.into())?.into_int_value();
        // the elements always follow the header (alloc in mimic_rt.c)
        let data = unsafe { self.b.build_in_bounds_gep(self.val_ty(), p, &[i32t.const_int(1, false)], "data").s()? };
        Ok((len, data.into()))
    }

    /// Run kernel `k` over `0..n`: directly when it is one chunk (no thread
    /// pool, no locks), else through the runtime's parallel loop.
    /// A scan's second pass: one direct call over everything when `one`
    /// (one chunk), else the usual parallel loop.
    fn scan_for(&mut self, n: IntValue<'ctx>, grain: IntValue<'ctx>, k: FunctionValue<'ctx>, ctx: PointerValue<'ctx>, one: IntValue<'ctx>) -> R<()> {
        let f = self.cur_fn.unwrap();
        let direct = self.ctx.append_basic_block(f, "scan_direct");
        let split = self.ctx.append_basic_block(f, "scan_split");
        let done = self.ctx.append_basic_block(f, "scan_done");
        self.b.build_conditional_branch(one, direct, split).s()?;
        self.b.position_at_end(direct);
        self.b.build_call(k, &[ctx.into(), self.ctx.i64_type().const_zero().into(), n.into()], "").s()?;
        self.b.build_unconditional_branch(done).s()?;
        self.b.position_at_end(split);
        self.par_for_grain(n, grain, k, ctx)?;
        self.b.build_unconditional_branch(done).s()?;
        self.b.position_at_end(done);
        Ok(())
    }

    fn par_for(&mut self, n: IntValue<'ctx>, k: FunctionValue<'ctx>, ctx: PointerValue<'ctx>) -> R<()> {
        let grain = self.grain()?;
        self.par_for_with(n, grain, k, ctx, false)
    }

    /// par_for in chunks of `grain` (the kernel must count chunks the same way)
    fn par_for_grain(&mut self, n: IntValue<'ctx>, grain: IntValue<'ctx>, k: FunctionValue<'ctx>, ctx: PointerValue<'ctx>) -> R<()> {
        self.par_for_with(n, grain, k, ctx, true)
    }

    fn par_for_with(&mut self, n: IntValue<'ctx>, grain: IntValue<'ctx>, k: FunctionValue<'ctx>, ctx: PointerValue<'ctx>, explicit: bool) -> R<()> {
        let one = self.b.build_int_compare(IntPredicate::SLE, n, grain, "").s()?;
        let f = self.cur_fn.unwrap();
        let direct = self.ctx.append_basic_block(f, "k_direct");
        let pooled = self.ctx.append_basic_block(f, "k_pool");
        let done = self.ctx.append_basic_block(f, "k_done");
        self.b.build_conditional_branch(one, direct, pooled).s()?;
        self.b.position_at_end(direct);
        self.b.build_call(k, &[ctx.into(), self.ctx.i64_type().const_zero().into(), n.into()], "").s()?;
        self.b.build_unconditional_branch(done).s()?;
        self.b.position_at_end(pooled);
        if explicit {
            // (chunks of `grain`, each thread a contiguous run of them)
            self.call_rt("mim_par_for_contig", &[n.into(), grain.into(), self.fn_addr(k).into(), ctx.into()])?;
        } else {
            self.call_rt("mim_par_for", &[n.into(), self.fn_addr(k).into(), ctx.into()])?;
        }
        self.b.build_unconditional_branch(done).s()?;
        self.b.position_at_end(done);
        Ok(())
    }

    /// A buffer for one partial result per chunk: on the stack for up to 64
    /// chunks, else on the heap. Returns it and whether it came from the heap.
    fn partials_buf(&mut self, nc: IntValue<'ctx>, bytes: IntValue<'ctx>) -> R<(PointerValue<'ctx>, IntValue<'ctx>)> {
        let i64t = self.ctx.i64_type();
        let stack = self.entry_array_of(i64t.array_type(65))?;
        let heap = self.b.build_int_compare(IntPredicate::SGT, nc, i64t.const_int(64, false), "").s()?;
        let f = self.cur_fn.unwrap();
        let pre = self.b.get_insert_block().unwrap();
        let big = self.ctx.append_basic_block(f, "parts_heap");
        let done = self.ctx.append_basic_block(f, "parts_done");
        self.b.build_conditional_branch(heap, big, done).s()?;
        self.b.position_at_end(big);
        let m = self.call_malloc(bytes)?;
        let big_end = self.b.get_insert_block().unwrap();
        self.b.build_unconditional_branch(done).s()?;
        self.b.position_at_end(done);
        let phi = self.b.build_phi(self.ptr, "parts").s()?;
        phi.add_incoming(&[(&stack, pre), (&m, big_end)]);
        Ok((phi.as_basic_value().into_pointer_value(), heap))
    }

    fn free_partials(&mut self, p: PointerValue<'ctx>, heap: IntValue<'ctx>) -> R<()> {
        let f = self.cur_fn.unwrap();
        let yes = self.ctx.append_basic_block(f, "parts_free");
        let done = self.ctx.append_basic_block(f, "parts_freed");
        self.b.build_conditional_branch(heap, yes, done).s()?;
        self.b.position_at_end(yes);
        self.call_free(p)?;
        self.b.build_unconditional_branch(done).s()?;
        self.b.position_at_end(done);
        Ok(())
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
        // substituted at every use: once, or cheap enough to compute again
        // (else a chain `b = a × a`, `c = b × b`, … doubles at each step)
        let mut roots = Vec::new();
        super::owned::block_roots(rest, tail, &mut roots);
        let mut uses = 0;
        for r in &roots {
            r.walk(&mut |x| {
                if matches!(&x.kind, ExprKind::Path(p) if p.len() == 1 && p[0] == name) {
                    uses += 1;
                }
            });
        }
        if uses > 1 && self.lazy_size(value) > 8 {
            return false;
        }
        self.lazies.push((name.to_string(), value as *const Expr));
        let ok = rest.iter().all(|s| self.stmt_uses_ok(name, s)) && tail.is_none_or(|t| self.uses_ok(name, t, false));
        self.lazies.pop();
        ok
    }

    /// How many operations `e` is, lazy lets in it substituted.
    fn lazy_size(&self, e: &Expr) -> usize {
        let mut n = 0;
        e.walk(&mut |x| {
            n += match &x.kind {
                ExprKind::Path(p) if p.len() == 1 => self.lazy_expr(&p[0]).map_or(1, |inner| self.lazy_size(inner)),
                _ => 1,
            };
        });
        n
    }

    fn trivially_pure(&self, e: &Expr) -> bool {
        match &e.kind {
            ExprKind::Int(_) | ExprKind::Float(_) | ExprKind::Bool(_) => true,
            ExprKind::Path(p) => {
                p.len() == 1 && (self.lazy_expr(&p[0]).is_some() || self.lookup(&p[0]).is_some_and(|l| !l.mutable))
            }
            ExprKind::Binary { lhs, rhs, .. } => self.trivially_pure(lhs) && self.trivially_pure(rhs),
            ExprKind::Unary { expr, .. } => self.trivially_pure(expr),
            ExprKind::Prim { prim: Prim::Iota | Prim::Range | Prim::Reverse | Prim::Take | Prim::Drop, args } => args.iter().all(|a| self.trivially_pure(a)),
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
            ExprKind::Prim { prim: Prim::Reverse | Prim::Take | Prim::Drop, args } => {
                let f = self.is_fae(e);
                self.uses_ok(name, &args[0], f) && args[1..].iter().all(|a| self.uses_ok(name, a, false))
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
