//! LLVM from the Mimic IR (crate mimic-ir), for the functions it covers;
//! everything else is compiled from the syntax tree as before. The
//! operators, calls and conversions are this code generator's own, so a
//! function gives the same results either way.
//!
//! `MIMIC_IR=1` sends the functions the IR covers through it (off by
//! default while it grows); `MIMIC_IR=print` also prints their IR.

use crate::{Cg, OrStr, R, V};
use inkwell::values::BasicValueEnum;
use mimic_check::types::Ty;
use mimic_ir::{Function, Op, Region};

/// Whether `MIMIC_IR` asks for the IR (read once).
pub(crate) fn wanted() -> (bool, bool) {
    static MODE: std::sync::OnceLock<(bool, bool)> = std::sync::OnceLock::new();
    *MODE.get_or_init(|| match std::env::var("MIMIC_IR").as_deref() {
        Ok("print") => (true, true),
        Ok("1") => (true, false),
        _ => (false, false),
    })
}

fn checker_ty(t: mimic_ir::Ty) -> Ty {
    match t {
        mimic_ir::Ty::Unit => Ty::Unit,
        mimic_ir::Ty::I64 => Ty::I64,
        mimic_ir::Ty::F64 => Ty::F64,
        mimic_ir::Ty::Bool => Ty::Bool,
        mimic_ir::Ty::Agg(t) | mimic_ir::Ty::List(t) => t,
    }
}

impl<'ctx> Cg<'ctx, '_> {
    /// The body of the function being defined, from its IR: the value it
    /// gives (its parameters are in their slots already).
    pub(crate) fn ir_body(&mut self, f: &Function) -> R<V<'ctx>> {
        let mut vals: Vec<Option<V<'ctx>>> = vec![None; f.values as usize];
        self.ir_region(f, &f.body, &mut vals)
    }

    fn ir_region(&mut self, f: &Function, r: &Region, vals: &mut Vec<Option<V<'ctx>>>) -> R<V<'ctx>> {
        let get = |vals: &Vec<Option<V<'ctx>>>, v: mimic_ir::Val| vals[v.0 as usize].ok_or_else(|| format!("internal compiler error: IR value {v} has no LLVM value"));
        for i in &r.insts {
            self.dbg_at(i.span);
            let ty = checker_ty(i.ty);
            let v = match &i.op {
                Op::Int(n) => V::I(self.ctx.i64_type().const_int(*n as u64, true)),
                Op::Float(x) => V::F(self.ctx.f64_type().const_float(*x)),
                Op::Bool(b) => V::B(self.ctx.bool_type().const_int(*b as u64, false)),
                Op::Unit => self.null(),
                Op::Param(n) => {
                    let name = &f.params[*n as usize].0;
                    let l = self.scopes[0].iter().find(|l| l.name == *name).ok_or("internal compiler error: IR parameter without a slot")?;
                    let (slot, lt) = (l.slot, l.ty);
                    let bv = self.b.build_load(self.repr(lt), slot, name).s()?;
                    self.val_of(bv, lt, false)
                }
                Op::Bin(op, a, b) => {
                    let r = self.scalar_binary(*op, get(vals, *a)?, get(vals, *b)?)?;
                    self.as_ty(r, ty, "operator result")?
                }
                Op::Un(op, a) => {
                    let r = self.scalar_unary(*op, get(vals, *a)?)?;
                    self.as_ty(r, ty, "operator result")?
                }
                Op::Call { site, callee, args } => {
                    // a list the IR owns is lent to the call: a callee that
                    // takes its parameter over gets its own reference (the
                    // call retains a lent value it hands over), and the IR
                    // still releases its own at the region's end
                    let a = args
                        .iter()
                        .map(|v| {
                            get(vals, *v).map(|x| match x {
                                V::P(p, true) => V::P(p, false),
                                x => x,
                            })
                        })
                        .collect::<R<Vec<_>>>()?;
                    let r = self.call_user(*site, *callee, a)?;
                    self.as_ty(r, ty, "value")?
                }
                Op::If { cond, then, els } => self.ir_if(f, ty, get(vals, *cond)?, then, els, vals)?,
                Op::Loop { init, var, count, done, step } => {
                    let (init, count) = (get(vals, *init)?, count.map(|c| get(vals, c)).transpose()?);
                    self.ir_loop(f, ty, init, *var, count, done.as_ref(), step, vals)?
                }
                // a value kept in registers: a short list, or a tuple or
                // record as a struct of its parts
                Op::Make(xs) => {
                    let parts = mimic_ir::parts(self.p, ty).ok_or("internal compiler error: IR value in registers")?;
                    let mut ps = Vec::new();
                    for (x, pt) in xs.iter().zip(&parts) {
                        ps.push(self.as_ty(get(vals, *x)?, *pt, "part")?);
                    }
                    match mimic_ir::short_list(ty) {
                        Some((el, _)) => self.fixed_from(el, &ps)?,
                        None => {
                            let mut out = self.repr(ty).into_struct_type().get_undef();
                            for (k, v) in ps.into_iter().enumerate() {
                                out = self.b.build_insert_value(out, self.basic(v), k as u32, "").s()?.into_struct_value();
                            }
                            V::R(out, ty)
                        }
                    }
                }
                // an expression the IR does not cover yet: the old path
                // compiles it here, its variables the IR's values (lent: in
                // slots of their own, never released from there)
                Op::Embed { expr, uses } => {
                    let mut locals = Vec::new();
                    for (name, v, t) in uses {
                        let ct = checker_ty(*t);
                        let val = get(vals, *v)?;
                        let slot = self.entry_alloca(ct, name)?;
                        self.b.build_store(slot, self.basic(val)).s()?;
                        locals.push(crate::Local { name: name.clone(), slot, ty: ct, owned: false, mutable: false, moves: false });
                    }
                    self.scopes.push(locals);
                    // SAFETY: the syntax tree outlives the compile
                    let e: &mimic_syntax::ast::Expr = unsafe { &**expr };
                    let out = self.expr(e);
                    self.scopes.pop();
                    let out = out?;
                    // (it may have ended the block: a return, a panic)
                    if self.terminated() {
                        self.dead_block();
                    }
                    self.as_ty(out, ty, "value")?
                }
                // a list's length, from its header (a list passed in: never
                // owned here, so nothing to release)
                Op::Len(v) => {
                    let b = self.boxed(get(vals, *v)?)?;
                    let p = self.basic(b).into_pointer_value();
                    let i64t = self.ctx.i64_type();
                    let n = self.b.build_load(i64t, self.b.build_struct_gep(self.val_ty(), p, 3, "").s()?, "len").s()?.into_int_value();
                    V::I(n)
                }
                // an element, as the code generator reads a typed list (its
                // kind checked, one bounds check)
                Op::Index(v, k, el) => {
                    let b = self.boxed(get(vals, *v)?)?;
                    let i = self.to_i64(get(vals, *k)?)?;
                    self.index_fast(b, i, *el, ty)?
                }
                Op::Get(v, k) => {
                    let base = get(vals, *v)?;
                    match base {
                        V::A(a, el) => self.elem_at(a, el, *k)?,
                        V::R(rv, rt) => {
                            let parts = mimic_ir::parts(self.p, rt).ok_or("internal compiler error: IR value in registers")?;
                            let x = self.b.build_extract_value(rv, *k, "").s()?;
                            self.val_of(x, parts[*k as usize], false)
                        }
                        _ => return Err("internal compiler error: IR part of a value not in registers".into()),
                    }
                }
                Op::Set(v, k, x) => {
                    let base = get(vals, *v)?;
                    let parts = mimic_ir::parts(self.p, ty).ok_or("internal compiler error: IR value in registers")?;
                    let nx = self.as_ty(get(vals, *x)?, parts[*k as usize], "part")?;
                    match base {
                        V::A(a, el) => V::A(self.b.build_insert_value(a, self.basic(nx), *k, "").s()?.into_array_value(), el),
                        V::R(rv, rt) => V::R(self.b.build_insert_value(rv, self.basic(nx), *k, "").s()?.into_struct_value(), rt),
                        _ => return Err("internal compiler error: IR part of a value not in registers".into()),
                    }
                }
            };
            vals[i.dst.0 as usize] = Some(v);
        }
        // the lists this region owns, released now; the one it yields
        // moves out
        for i in &r.insts {
            if mimic_ir::owns(i) && Some(i.dst) != r.result {
                let v = get(vals, i.dst)?;
                self.drop_tmp(v)?;
            }
        }
        match r.result {
            Some(v) => get(vals, v),
            None => Ok(self.null()),
        }
    }

    /// A loop over a state (evolve, settle): the state a phi of its type;
    /// `done`, when there is one, asked before each step, as the runtime
    /// asks it; a count below zero stops the program, as evolve does.
    #[allow(clippy::too_many_arguments)]
    fn ir_loop(&mut self, f: &Function, ty: Ty, init: V<'ctx>, var: mimic_ir::Val, count: Option<V<'ctx>>, done: Option<&Region>, step: &Region, vals: &mut Vec<Option<V<'ctx>>>) -> R<V<'ctx>> {
        let i64t = self.ctx.i64_type();
        let x0 = self.as_ty(init, ty, "loop state")?;
        let n = match count {
            Some(c) => {
                let n = self.to_i64(c)?;
                let neg = self.b.build_int_compare(inkwell::IntPredicate::SLT, n, i64t.const_zero(), "").s()?;
                self.panic_if(neg, "evolve: the number of steps cannot be negative")?;
                Some(n)
            }
            None => None,
        };
        let fun = self.cur_fn.ok_or("internal compiler error: a loop outside a function")?;
        let pre = self.b.get_insert_block().unwrap();
        let head = self.ctx.append_basic_block(fun, "loop");
        let body = self.ctx.append_basic_block(fun, "loop_step");
        let out = self.ctx.append_basic_block(fun, "loop_done");
        self.b.build_unconditional_branch(head).s()?;
        self.b.position_at_end(head);
        let xp = self.b.build_phi(self.repr(ty), "state").s()?;
        let ip = match n {
            Some(_) => Some(self.b.build_phi(i64t, "i").s()?),
            None => None,
        };
        let cur = self.val_of(xp.as_basic_value(), ty, false);
        vals[var.0 as usize] = Some(cur);
        let stop = match (n, done) {
            (Some(n), _) => {
                let i = ip.unwrap().as_basic_value().into_int_value();
                self.b.build_int_compare(inkwell::IntPredicate::SGE, i, n, "").s()?
            }
            (None, Some(d)) => {
                let v = self.ir_region(f, d, vals)?;
                self.truth(v)?
            }
            (None, None) => return Err("internal compiler error: a loop with no end".into()),
        };
        self.b.build_conditional_branch(stop, out, body).s()?;
        self.b.position_at_end(body);
        let next = self.ir_region(f, step, vals)?;
        let next = self.as_ty(next, ty, "loop state")?;
        let latch = self.b.get_insert_block().unwrap();
        if let Some(ip) = ip {
            let i = ip.as_basic_value().into_int_value();
            let i1 = self.b.build_int_add(i, i64t.const_int(1, false), "").s()?;
            ip.add_incoming(&[(&i64t.const_zero(), pre), (&i1, latch)]);
        }
        self.b.build_unconditional_branch(head).s()?;
        xp.add_incoming(&[(&self.basic(x0), pre), (&self.basic(next), latch)]);
        self.b.position_at_end(out);
        Ok(cur)
    }

    fn ir_if(&mut self, f: &Function, ty: Ty, cond: V<'ctx>, then: &Region, els: &Region, vals: &mut Vec<Option<V<'ctx>>>) -> R<V<'ctx>> {
        let bit = self.truth(cond)?;
        let fun = self.cur_fn.ok_or("internal compiler error: `if` outside a function")?;
        let then_bb = self.ctx.append_basic_block(fun, "then");
        let else_bb = self.ctx.append_basic_block(fun, "else");
        let merge_bb = self.ctx.append_basic_block(fun, "merge");
        self.b.build_conditional_branch(bit, then_bb, else_bb).s()?;
        let mut incoming: Vec<(BasicValueEnum<'ctx>, inkwell::basic_block::BasicBlock<'ctx>)> = Vec::new();
        for (bb, region) in [(then_bb, then), (else_bb, els)] {
            self.b.position_at_end(bb);
            let v = self.ir_region(f, region, vals)?;
            if !self.terminated() {
                if ty != Ty::Unit {
                    let v = self.as_ty(v, ty, "branch value")?;
                    // a list: owned from either branch (one passed in is
                    // retained), so the if's list is owned
                    let v = if matches!(v, V::P(..)) { self.own(v)? } else { v };
                    incoming.push((self.basic(v), self.b.get_insert_block().unwrap()));
                }
                self.b.build_unconditional_branch(merge_bb).s()?;
            }
        }
        self.b.position_at_end(merge_bb);
        if ty == Ty::Unit {
            return Ok(self.null());
        }
        if incoming.is_empty() {
            self.b.build_unreachable().s()?;
            self.dead_block();
            return Ok(self.null());
        }
        let phi = self.b.build_phi(incoming[0].0.get_type(), "if").s()?;
        for (v, b) in &incoming {
            phi.add_incoming(&[(v, *b)]);
        }
        let owned = matches!(ty, Ty::Arr(..)) && !matches!(self.val_of(phi.as_basic_value(), ty, false), V::A(..));
        Ok(self.val_of(phi.as_basic_value(), ty, owned))
    }
}
