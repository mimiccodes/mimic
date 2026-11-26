//! LLVM from the Mimic IR (crate mimic-ir), for the functions it covers;
//! everything else is compiled from the syntax tree as before. The
//! operators, calls and conversions are this code generator's own, so a
//! function gives the same results either way.
//!
//! Every function the IR covers goes through it (what it does not cover
//! yet is embedded: compiled by the old path where it stands);
//! `MIMIC_IR=0` turns it off (the old path for everything), and
//! `MIMIC_IR=print` also prints each function's IR.

use crate::{Cg, OrStr, R, V};
use inkwell::values::{BasicValueEnum, IntValue, PointerValue};
use mimic_check::types::Ty;
use mimic_ir::{Function, Op, Region};

/// Whether `MIMIC_IR` asks for the IR (read once).
pub(crate) fn wanted() -> (bool, bool) {
    static MODE: std::sync::OnceLock<(bool, bool)> = std::sync::OnceLock::new();
    *MODE.get_or_init(|| match std::env::var("MIMIC_IR").as_deref() {
        Ok("print") => (true, true),
        Ok("0") => (false, false),
        _ => (true, false),
    })
}

/// Whether the region (or one inside it) takes parameter `n` over, clearing
/// its slot.
fn takes(r: &Region, n: u32) -> bool {
    r.insts.iter().any(|i| matches!(i.op, Op::Take(m) if m == n) || i.op.regions().into_iter().any(|g| takes(g, n)))
}

/// A region that only yields a truth value written in the program.
fn const_bool(r: &Region) -> Option<bool> {
    match (r.insts.as_slice(), r.result) {
        ([i], Some(v)) if i.dst == v => match i.op {
            Op::Bool(b) => Some(b),
            _ => None,
        },
        _ => None,
    }
}

/// What the IR's lowering asks: the old path's own answers, in a scope
/// holding the IR's variables (read only for their types and mutability).
impl mimic_ir::Hints for Cg<'_, '_> {
    fn lazy(&mut self, name: &str, value: &mimic_syntax::ast::Expr, rest: &[mimic_syntax::ast::Stmt], tail: Option<&mimic_syntax::ast::Expr>, scope: &[(String, mimic_ir::Ty)], lazies: &[(String, *const mimic_syntax::ast::Expr)]) -> bool {
        let null = self.ptr.const_null();
        let locals = scope.iter().map(|(n, t)| crate::Local { name: n.clone(), slot: null, ty: checker_ty(*t), owned: false, mutable: false, moves: false }).collect();
        self.scopes.push(locals);
        let mark = self.lazies.len();
        self.lazies.extend(lazies.iter().cloned());
        let lazy = self.lazy_candidate(name, value, rest, tail);
        self.lazies.truncate(mark);
        self.scopes.pop();
        lazy
    }

    fn fusible(&mut self, e: &mimic_syntax::ast::Expr, scope: &[(String, mimic_ir::Ty)], lazies: &[(String, *const mimic_syntax::ast::Expr)]) -> bool {
        let null = self.ptr.const_null();
        let locals = scope.iter().map(|(n, t)| crate::Local { name: n.clone(), slot: null, ty: checker_ty(*t), owned: false, mutable: false, moves: false }).collect();
        self.scopes.push(locals);
        let mark = self.lazies.len();
        self.lazies.extend(lazies.iter().cloned());
        let ok = self.is_fae(e);
        self.lazies.truncate(mark);
        self.scopes.pop();
        ok
    }

    fn spec(&mut self, key: &crate::types::Key) -> Option<std::rc::Rc<crate::types::Spec>> {
        Cg::spec(self, key).ok()
    }

    fn takes(&mut self, key: &crate::types::Key) -> Vec<bool> {
        self.consumed(key)
    }

    fn runs_apart(&mut self, stmts: &[mimic_syntax::ast::Stmt]) -> bool {
        Cg::runs_apart(self, stmts)
    }
}

pub(crate) fn checker_ty(t: mimic_ir::Ty) -> Ty {
    match t {
        mimic_ir::Ty::Unit => Ty::Unit,
        mimic_ir::Ty::I64 => Ty::I64,
        mimic_ir::Ty::F64 => Ty::F64,
        mimic_ir::Ty::Bool => Ty::Bool,
        mimic_ir::Ty::Agg(t) | mimic_ir::Ty::List(t) | mimic_ir::Ty::Rec(t) => t,
    }
}

/// One change of a list of i64, its numbers computed.
#[derive(Clone, Copy)]
enum Ch<'ctx> {
    Put(IntValue<'ctx>, IntValue<'ctx>),
    Push(IntValue<'ctx>),
    /// appends a list of i64 (lent)
    Concat(PointerValue<'ctx>),
}

impl<'ctx> Cg<'ctx, '_> {
    /// The body of the function being defined, from its IR: the value it
    /// gives (its parameters are in their slots already).
    pub(crate) fn ir_body(&mut self, f: &Function) -> R<V<'ctx>> {
        let mut vals: Vec<Option<V<'ctx>>> = vec![None; f.values as usize];
        if self.body_exit.is_some() {
            // `main` runs once and the process exits after it, so its own
            // values are not freed one by one (see Cg::exit_fid); the old
            // path keeps a `let` until the end of its scope too, so this
            // holds no more memory than it would. Drops inside loops and
            // branches stay.
            let mut body = f.body.clone();
            body.insts.retain(|i| !matches!(i.op, Op::Drop(_)));
            self.ir_exit_top = &body as *const mimic_ir::Region as usize;
            let r = self.ir_region(f, &body, &mut vals);
            self.ir_exit_top = 0;
            return r;
        }
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
                // a parameter taken over: owned by the IR from here (its slot
                // cleared, so the function's end does not release it again)
                Op::Take(n) => {
                    let name = &f.params[*n as usize].0;
                    let l = self.scopes[0].iter().find(|l| l.name == *name).ok_or("internal compiler error: IR parameter without a slot")?;
                    let (slot, lt) = (l.slot, l.ty);
                    let bv = self.b.build_load(self.repr(lt), slot, name).s()?;
                    self.b.build_store(slot, self.ptr.const_null()).s()?;
                    self.val_of(bv, lt, true)
                }
                Op::Bin(op, a, b) => {
                    let r = self.scalar_binary(*op, get(vals, *a)?, get(vals, *b)?)?;
                    self.as_ty(r, ty, "operator result")?
                }
                Op::Un(op, a) => {
                    let r = self.scalar_unary(*op, get(vals, *a)?)?;
                    self.as_ty(r, ty, "operator result")?
                }
                Op::Call { site, callee, args, moves, .. } => {
                    // an owned value at its last use, to a callee that takes
                    // it over: handed over as it is; any other lent (a callee
                    // that takes its parameter over gets its own reference:
                    // the call retains a lent value it hands over)
                    let a = args
                        .iter()
                        .zip(moves)
                        .map(|(v, m)| {
                            get(vals, *v).map(|x| match x {
                                V::P(p, true) if !m => V::P(p, false),
                                x => x,
                            })
                        })
                        .collect::<R<Vec<_>>>()?;
                    let r = self.call_user(*site, *callee, a)?;
                    self.as_ty(r, ty, "value")?
                }
                Op::If { cond, then, els } => self.ir_if(f, ty, get(vals, *cond)?, then, els, vals)?,
                Op::Loop { init, var, count, done, step, moved } => {
                    let (init, count) = (get(vals, *init)?, count.map(|c| get(vals, c)).transpose()?);
                    // a state on the heap: the loop holds a reference of its
                    // own (the first state handed over, or retained)
                    let init = match init {
                        V::P(p, true) if !moved => self.own(V::P(p, false))?,
                        V::P(p, false) => self.own(V::P(p, false))?,
                        x => x,
                    };
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
                Op::Embed { expr, uses, lazies, moves } => {
                    // SAFETY: the syntax tree outlives the compile
                    let e: &mimic_syntax::ast::Expr = unsafe { &**expr };
                    let mut locals = Vec::new();
                    let mut moved = Vec::new();
                    for ((name, v, t), m) in uses.iter().zip(moves) {
                        let ct = checker_ty(*t);
                        // its last use: owned by the expression's slot, moved
                        // on where the old path can, released after it if not
                        if *m {
                            // (a value the IR counts as owned may be lent here: a
                            // field read off a borrowed record, `a = f.opt` of a
                            // parameter, has no reference of its own; the
                            // expression is given one, or it would release the
                            // record's field and the record release it again)
                            let x = self.own(get(vals, *v)?)?;
                            let slot = self.entry_alloca(ct, name)?;
                            self.b.build_store(slot, self.basic(x)).s()?;
                            if let Some(site) = self.let_move_site(name, &[], Some(e)) {
                                self.move_at.insert(site);
                            }
                            moved.push(crate::Local { name: name.clone(), slot, ty: ct, owned: true, mutable: false, moves: true });
                            locals.push(crate::Local { name: name.clone(), slot, ty: ct, owned: true, mutable: false, moves: true });
                            continue;
                        }
                        // a parameter read as it is: its own slot (never
                        // written while the function runs, and a record in
                        // registers is not copied once per embed)
                        let own = f.body.insts.iter().find(|i| i.dst == *v).and_then(|i| match i.op {
                            Op::Param(n) if !takes(&f.body, n) => self.scopes[0].iter().find(|l| l.name == f.params[n as usize].0 && l.ty == ct).map(|l| l.slot),
                            _ => None,
                        });
                        if let Some(slot) = own {
                            locals.push(crate::Local { name: name.clone(), slot, ty: ct, owned: false, mutable: false, moves: false });
                            continue;
                        }
                        let val = get(vals, *v)?;
                        let slot = self.entry_alloca(ct, name)?;
                        self.b.build_store(slot, self.basic(val)).s()?;
                        locals.push(crate::Local { name: name.clone(), slot, ty: ct, owned: false, mutable: false, moves: false });
                    }
                    self.scopes.push(locals);
                    // the lazy lets it reads: fused where it reads them
                    let lazy_mark = self.lazies.len();
                    self.lazies.extend(lazies.iter().cloned());
                    let out = self.expr(e);
                    self.lazies.truncate(lazy_mark);
                    self.scopes.pop();
                    let out = out?;
                    // (it may have ended the block: a return, a panic)
                    if self.terminated() {
                        self.dead_block();
                    }
                    // (in main's body: the process exits next, as for main's
                    // own values; `ls`, 1.6 million strings in http_parse,
                    // was freed one by one just before the exit)
                    if r as *const mimic_ir::Region as usize != self.ir_exit_top {
                        self.release_scope(&moved)?;
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
                // a reduction: the fused engine's kernel over the one list
                // (lent to it)
                Op::Reduce(p, t, el) => {
                    let out = {
                        let vals: &Vec<Option<V<'ctx>>> = vals;
                        self.reduce_tree(*p, t, &|v| get(vals, v), *el, i.span)?
                    };
                    self.as_ty(out, ty, "reduction")?
                }
                // a record on the heap, built as the code generator builds one:
                // a value the IR moves in becomes the record's, any other is
                // lent (the record takes a reference of its own)
                Op::Record { tid, fields, moves } => {
                    let vs = fields
                        .iter()
                        .zip(moves)
                        .map(|(v, m)| {
                            get(vals, *v).map(|x| match x {
                                V::P(p, true) if !m => V::P(p, false),
                                x => x,
                            })
                        })
                        .collect::<R<Vec<_>>>()?;
                    self.construct(*tid, None, vs)?
                }
                // a field read in place: a number stored raw loaded, else
                // borrowed from a borrowed record, its own reference from an
                // owned one (released after its last use, as any)
                Op::Field { base: b, tid, k, borrow } => {
                    let base = get(vals, *b)?;
                    let tid = *tid;
                    let p = self.basic(base).into_pointer_value();
                    let fp = self.slot_ptr(p, *k as usize)?;
                    if let Some(el) = self.raw_field(tid, None, *k as usize) {
                        let x = self.load_raw(fp, el)?;
                        self.as_ty(x, ty, "field")?
                    } else {
                        let x = self.b.build_load(self.ptr, fp, "field").s()?.into_pointer_value();
                        let v = if let (V::P(_, true), false) = (base, *borrow) {
                            self.call_rt("mim_retain", &[x.into()])?;
                            V::P(x, true)
                        } else {
                            V::P(x, false)
                        };
                        self.as_ty(v, ty, "field")?
                    }
                }
                // `{ r with … }` (in place when the record is held once, else
                // on a copy): the base handed over when the IR moves it there,
                // else a reference of its own; each new value moved in or lent,
                // as a record's
                Op::With { base, tid, fields, moved, moves, changes } => {
                    let b = get(vals, *base)?;
                    let b = match b {
                        V::P(p, true) if !moved => V::P(p, false),
                        x => x,
                    };
                    let cur = self.own(self.boxed(b)?)?;
                    let news = fields
                        .iter()
                        .zip(moves)
                        .map(|((k, x), m)| {
                            get(vals, *x).map(|x| match x {
                                V::P(p, owned) => (*k as usize, V::P(p, owned && *m)),
                                x => (*k as usize, x),
                            })
                        })
                        .collect::<R<Vec<_>>>()?;
                    let mut chs = Vec::new();
                    // (lists boxed here from registers for a concat: released after)
                    let mut temps = Vec::new();
                    for (k, cs) in changes {
                        let mut steps = Vec::new();
                        for c in cs {
                            steps.push(match c {
                                mimic_ir::Change::Put(i, x) => Ch::Put(self.to_i64(get(vals, *i)?)?, self.to_i64(get(vals, *x)?)?),
                                mimic_ir::Change::Push(x) => Ch::Push(self.to_i64(get(vals, *x)?)?),
                                mimic_ir::Change::Concat(x) => {
                                    let v = get(vals, *x)?;
                                    let b = match v {
                                        V::P(..) => v,
                                        _ => {
                                            let b = self.boxed(v)?;
                                            temps.push(b);
                                            b
                                        }
                                    };
                                    Ch::Concat(self.basic(b).into_pointer_value())
                                }
                            });
                        }
                        chs.push((*k as usize, steps));
                    }
                    let r = self.ir_with(*tid, cur, news, chs)?;
                    for t in temps {
                        self.drop_tmp(t)?;
                    }
                    self.as_ty(r, ty, "record")?
                }
                // put/push of a list of i64: the list handed over when the IR
                // moves it there (in place when held once), else a reference
                // of its own (so a copy)
                Op::Put { list, index, value, moved } => {
                    let (i, x) = (self.to_i64(get(vals, *index)?)?, self.to_i64(get(vals, *value)?)?);
                    if *moved {
                        let l = self.list_arg(get(vals, *list)?, true)?;
                        V::P(self.ir_change(l, Ch::Put(i, x))?, true)
                    } else {
                        // borrowed: a copy with the change (no reference taken)
                        let l = self.borrowed_list(get(vals, *list)?)?;
                        V::P(self.rt_list_fn("mim_put_i64", &[l.into(), i.into(), x.into()])?, true)
                    }
                }
                Op::Push { list, value, moved } => {
                    let x = self.to_i64(get(vals, *value)?)?;
                    if *moved {
                        let l = self.list_arg(get(vals, *list)?, true)?;
                        V::P(self.ir_change(l, Ch::Push(x))?, true)
                    } else {
                        let l = self.borrowed_list(get(vals, *list)?)?;
                        V::P(self.rt_list_fn("mim_push_i64", &[l.into(), x.into()])?, true)
                    }
                }
                // an owned list's last use is past: released now
                Op::Drop(v) => {
                    let x = get(vals, *v)?;
                    self.drop_tmp(x)?;
                    self.null()
                }
                // a list computed element by element: one loop (owned)
                Op::Map(t) => {
                    let Ty::Arr(el, _) = ty else { return Err("internal compiler error: an IR map that is not a list".into()) };
                    let vals: &Vec<Option<V<'ctx>>> = vals;
                    self.map_tree(t, &|v| get(vals, v), el, i.span)?
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
        // (every value the region owns ends where the ownership pass says:
        // dropped, moved on, or yielded)
        match r.result {
            Some(v) => get(vals, v),
            None => Ok(self.null()),
        }
    }

    /// `{ r with … }` on the owned record `cur`: used as it is when held
    /// once (one load, one compare: the common case once the IR moves it),
    /// else copied; then each field written directly (a number raw, any
    /// other value with a reference of its own, the old one released).
    /// `changes`: list fields changed from their old value, after the
    /// others: taken out of the record (held once by then), changed, stored
    /// back; a list still shared is seen as shared (and copied).
    fn ir_with(&mut self, tid: usize, cur: V<'ctx>, news: Vec<(usize, V<'ctx>)>, changes: Vec<(usize, Vec<Ch<'ctx>>)>) -> R<V<'ctx>> {
        let i64t = self.ctx.i64_type();
        let V::P(p0, _) = cur else { return Err("internal compiler error: `with` on a value not on the heap".into()) };
        if !self.in_place_ok() {
            let mut cur = cur;
            for (k, x) in news {
                let v = self.boxed(x)?;
                cur = self.call_rt_ptr("mim_obj_with", &[self.basic(cur).into(), i64t.const_int(k as u64, false).into(), self.basic(v).into()])?;
                self.drop_tmp(v)?;
            }
            for (k, steps) in changes {
                // (the old list, a reference of its own: so changed on a copy)
                let V::P(c, _) = cur else { unreachable!() };
                let fp = self.slot_ptr(c, k)?;
                let mut list = self.b.build_load(self.ptr, fp, "old").s()?.into_pointer_value();
                self.call_rt("mim_retain", &[list.into()])?;
                for st in steps {
                    list = self.ir_change(list, st)?;
                }
                cur = self.call_rt_ptr("mim_obj_with", &[self.basic(cur).into(), i64t.const_int(k as u64, false).into(), list.into()])?;
                self.release(list)?;
            }
            return Ok(cur);
        }
        let rc = self.b.build_load(i64t, p0, "rc").s()?;
        inkwell::values::BasicValue::as_instruction_value(&rc).unwrap().set_atomic_ordering(inkwell::AtomicOrdering::Acquire).map_err(|e| e.to_string())?;
        let one = self.b.build_int_compare(inkwell::IntPredicate::EQ, rc.into_int_value(), i64t.const_int(1, false), "").s()?;
        let fun = self.cur_fn.ok_or("internal compiler error: `with` outside a function")?;
        let pre = self.b.get_insert_block().unwrap();
        let copy_bb = self.ctx.append_basic_block(fun, "with_copy");
        let go_bb = self.ctx.append_basic_block(fun, "with_go");
        self.b.build_conditional_branch(one, go_bb, copy_bb).s()?;
        self.b.position_at_end(copy_bb);
        let V::P(copied, _) = self.call_rt_ptr("mim_obj_unique", &[p0.into()])? else { unreachable!() };
        let copy_end = self.b.get_insert_block().unwrap();
        self.b.build_unconditional_branch(go_bb).s()?;
        self.b.position_at_end(go_bb);
        let phi = self.b.build_phi(self.ptr, "rec").s()?;
        phi.add_incoming(&[(&p0, pre), (&copied, copy_end)]);
        let rec = phi.as_basic_value().into_pointer_value();
        for (k, v) in news {
            let fp = self.slot_ptr(rec, k)?;
            if let Some(el) = self.raw_field(tid, None, k) {
                self.store_raw(fp, v, el)?;
                continue;
            }
            let v = self.own(self.boxed(v)?)?;
            let old = self.b.build_load(self.ptr, fp, "old").s()?.into_pointer_value();
            self.b.build_store(fp, self.basic(v)).s()?;
            self.release(old)?;
        }
        for (k, steps) in changes {
            let fp = self.slot_ptr(rec, k)?;
            let mut list = self.b.build_load(self.ptr, fp, "taken").s()?.into_pointer_value();
            self.b.build_store(fp, self.ptr.const_null()).s()?;
            for st in steps {
                list = self.ir_change(list, st)?;
            }
            self.b.build_store(fp, list).s()?;
        }
        Ok(V::P(rec, true))
    }

    /// A list handed to put/push (which take it): as it is when moved
    /// there, else a reference of its own.
    fn list_arg(&mut self, l: V<'ctx>, moved: bool) -> R<PointerValue<'ctx>> {
        let l = match l {
            V::P(p, true) if !moved => V::P(p, false),
            x => x,
        };
        let V::P(p, _) = self.own(self.boxed(l)?)? else { return Err("internal compiler error: put/push of a value not on the heap".into()) };
        Ok(p)
    }

    /// A list the IR reads without taking a reference.
    fn borrowed_list(&mut self, l: V<'ctx>) -> R<PointerValue<'ctx>> {
        let V::P(p, _) = self.boxed(l)? else { return Err("internal compiler error: put/push of a value not on the heap".into()) };
        Ok(p)
    }

    /// A runtime function on lists of i64 (a list, then i64s) giving a new
    /// list: declared on first use.
    fn rt_list_fn(&mut self, name: &str, args: &[inkwell::values::BasicMetadataValueEnum<'ctx>]) -> R<PointerValue<'ctx>> {
        let i64t = self.ctx.i64_type();
        let f = self.module.get_function(name).unwrap_or_else(|| {
            let mut ps: Vec<inkwell::types::BasicMetadataTypeEnum> = vec![self.ptr.into()];
            ps.extend(std::iter::repeat_n(inkwell::types::BasicMetadataTypeEnum::from(i64t), args.len() - 1));
            self.module.add_function(name, self.ptr.fn_type(&ps, false), Some(inkwell::module::Linkage::External))
        });
        Ok(self.b.build_call(f, args, "").s()?.try_as_basic_value().basic().unwrap().into_pointer_value())
    }

    /// One change of the owned list of i64 `list` (taken): the list it gives.
    fn ir_change(&mut self, list: PointerValue<'ctx>, c: Ch<'ctx>) -> R<PointerValue<'ctx>> {
        let i64t = self.ctx.i64_type();
        match c {
            Ch::Put(i, x) => self.put_i64_inline(list, i, x),
            Ch::Push(x) => self.append_i64_inline(list, &[x], &mut |cg| {
                let f = cg.module.get_function("mim_push_i64_own").unwrap_or_else(|| cg.module.add_function("mim_push_i64_own", cg.ptr.fn_type(&[cg.ptr.into(), i64t.into()], false), Some(inkwell::module::Linkage::External)));
                Ok(cg.b.build_call(f, &[list.into(), x.into()], "").s()?.try_as_basic_value().basic().unwrap().into_pointer_value())
            }),
            Ch::Concat(ys) => {
                let f = self.module.get_function("mim_std_concat_own").unwrap_or_else(|| self.module.add_function("mim_std_concat_own", self.ptr.fn_type(&[self.ptr.into(), self.ptr.into()], false), Some(inkwell::module::Linkage::External)));
                Ok(self.b.build_call(f, &[list.into(), ys.into()], "").s()?.try_as_basic_value().basic().unwrap().into_pointer_value())
            }
        }
    }

    /// `put(xs, i, x)` on the owned list `xs`: held once, of i64, one
    /// dimension and `i` in range, the number is stored in place here;
    /// anything else is mim_put_i64_own's (which copies, converts or stops
    /// with the message).
    fn put_i64_inline(&mut self, list: PointerValue<'ctx>, i: IntValue<'ctx>, x: IntValue<'ctx>) -> R<PointerValue<'ctx>> {
        let i64t = self.ctx.i64_type();
        self.put_i64_inline_or(list, i, x, &mut |cg| {
            let rf = cg.module.get_function("mim_put_i64_own").unwrap_or_else(|| cg.module.add_function("mim_put_i64_own", cg.ptr.fn_type(&[cg.ptr.into(), i64t.into(), i64t.into()], false), Some(inkwell::module::Linkage::External)));
            Ok(cg.b.build_call(rf, &[list.into(), i.into(), x.into()], "").s()?.try_as_basic_value().basic().unwrap().into_pointer_value())
        })
    }

    /// [`Self::put_i64_inline`] with the runtime's call given (`slow`).
    pub(crate) fn put_i64_inline_or(&mut self, list: PointerValue<'ctx>, i: IntValue<'ctx>, x: IntValue<'ctx>, slow: &mut dyn FnMut(&mut Self) -> R<PointerValue<'ctx>>) -> R<PointerValue<'ctx>> {
        let i64t = self.ctx.i64_type();
        let i32t = self.ctx.i32_type();
        let hdr = self.ctx.struct_type(&[i64t.into(), i32t.into(), i32t.into(), i64t.into(), self.ptr.into()], false);
        let rc = self.b.build_load(i64t, self.b.build_struct_gep(hdr, list, 0, "").s()?, "rc").s()?;
        inkwell::values::BasicValue::as_instruction_value(&rc).unwrap().set_atomic_ordering(inkwell::AtomicOrdering::Acquire).map_err(|e| e.to_string())?;
        let dtype = self.b.build_load(i32t, self.b.build_struct_gep(hdr, list, 1, "").s()?, "dtype").s()?.into_int_value();
        let rank = self.b.build_load(i32t, self.b.build_struct_gep(hdr, list, 2, "").s()?, "rank").s()?.into_int_value();
        let len = self.b.build_load(i64t, self.b.build_struct_gep(hdr, list, 3, "").s()?, "len").s()?.into_int_value();
        let once = self.b.build_int_compare(inkwell::IntPredicate::EQ, rc.into_int_value(), i64t.const_int(1, false), "").s()?;
        let ints = self.b.build_int_compare(inkwell::IntPredicate::EQ, dtype, i32t.const_zero(), "").s()?;
        let flat = self.b.build_int_compare(inkwell::IntPredicate::EQ, rank, i32t.const_int(1, false), "").s()?;
        let inside = self.b.build_int_compare(inkwell::IntPredicate::ULT, i, len, "").s()?;
        let ok = self.b.build_and(self.b.build_and(once, ints, "").s()?, self.b.build_and(flat, inside, "").s()?, "").s()?;
        let f = self.cur_fn.ok_or("internal compiler error: put outside a function")?;
        let here = self.ctx.append_basic_block(f, "put_here");
        let slow_bb = self.ctx.append_basic_block(f, "put_slow");
        let done = self.ctx.append_basic_block(f, "put_done");
        self.b.build_conditional_branch(ok, here, slow_bb).s()?;
        self.b.position_at_end(here);
        let data = unsafe { self.b.build_in_bounds_gep(self.val_ty(), list, &[i32t.const_int(1, false)], "data").s()? };
        let at = unsafe { self.b.build_in_bounds_gep(i64t, data, &[i], "").s()? };
        self.b.build_store(at, x).s()?;
        let here_end = self.b.get_insert_block().unwrap();
        self.b.build_unconditional_branch(done).s()?;
        self.b.position_at_end(slow_bb);
        let r = slow(self)?;
        let slow_end = self.b.get_insert_block().unwrap();
        self.b.build_unconditional_branch(done).s()?;
        self.b.position_at_end(done);
        let phi = self.b.build_phi(self.ptr, "list").s()?;
        phi.add_incoming(&[(&list, here_end), (&r, slow_end)]);
        Ok(phi.as_basic_value().into_pointer_value())
    }

    /// `push(xs, x)` / `concat(xs, [a b …])` on the owned list `xs`: held
    /// once, of i64, one dimension, with room already for the new numbers
    /// (a list grown in place keeps its room in `desc`), they are stored
    /// here and the length raised; anything else (a short list in a pool,
    /// one that must grow, shared or of another type) is `slow`'s, the
    /// runtime's mim_push_i64_own or mim_concat_i64_own.
    pub(crate) fn append_i64_inline(&mut self, list: PointerValue<'ctx>, xs: &[IntValue<'ctx>], slow: &mut dyn FnMut(&mut Self) -> R<PointerValue<'ctx>>) -> R<PointerValue<'ctx>> {
        let i64t = self.ctx.i64_type();
        let i32t = self.ctx.i32_type();
        let hdr = self.ctx.struct_type(&[i64t.into(), i32t.into(), i32t.into(), i64t.into(), self.ptr.into()], false);
        let rc = self.b.build_load(i64t, self.b.build_struct_gep(hdr, list, 0, "").s()?, "rc").s()?;
        inkwell::values::BasicValue::as_instruction_value(&rc).unwrap().set_atomic_ordering(inkwell::AtomicOrdering::Acquire).map_err(|e| e.to_string())?;
        let dtype = self.b.build_load(i32t, self.b.build_struct_gep(hdr, list, 1, "").s()?, "dtype").s()?.into_int_value();
        let rank = self.b.build_load(i32t, self.b.build_struct_gep(hdr, list, 2, "").s()?, "rank").s()?.into_int_value();
        let lenp = self.b.build_struct_gep(hdr, list, 3, "").s()?;
        let len = self.b.build_load(i64t, lenp, "len").s()?.into_int_value();
        let room = self.b.build_load(self.ptr, self.b.build_struct_gep(hdr, list, 4, "").s()?, "room").s()?.into_pointer_value();
        let room = self.b.build_ptr_to_int(room, i64t, "").s()?;
        let n = self.b.build_int_add(len, i64t.const_int(xs.len() as u64, false), "").s()?;
        let need = self.b.build_int_mul(n, i64t.const_int(8, false), "").s()?;
        let once = self.b.build_int_compare(inkwell::IntPredicate::EQ, rc.into_int_value(), i64t.const_int(1, false), "").s()?;
        let ints = self.b.build_int_compare(inkwell::IntPredicate::EQ, dtype, i32t.const_zero(), "").s()?;
        let flat = self.b.build_int_compare(inkwell::IntPredicate::EQ, rank, i32t.const_int(1, false), "").s()?;
        // (room 0: not known, the runtime asks the allocator)
        let fits = self.b.build_int_compare(inkwell::IntPredicate::ULE, need, room, "").s()?;
        let ok = self.b.build_and(self.b.build_and(once, ints, "").s()?, self.b.build_and(flat, fits, "").s()?, "").s()?;
        let f = self.cur_fn.ok_or("internal compiler error: push outside a function")?;
        let here = self.ctx.append_basic_block(f, "append_here");
        let slow_bb = self.ctx.append_basic_block(f, "append_slow");
        let done = self.ctx.append_basic_block(f, "append_done");
        self.b.build_conditional_branch(ok, here, slow_bb).s()?;
        self.b.position_at_end(here);
        let data = unsafe { self.b.build_in_bounds_gep(self.val_ty(), list, &[i32t.const_int(1, false)], "data").s()? };
        for (k, x) in xs.iter().enumerate() {
            let at = self.b.build_int_add(len, i64t.const_int(k as u64, false), "").s()?;
            let at = unsafe { self.b.build_in_bounds_gep(i64t, data, &[at], "").s()? };
            self.b.build_store(at, *x).s()?;
        }
        self.b.build_store(lenp, n).s()?;
        let here_end = self.b.get_insert_block().unwrap();
        self.b.build_unconditional_branch(done).s()?;
        self.b.position_at_end(slow_bb);
        let r = slow(self)?;
        let slow_end = self.b.get_insert_block().unwrap();
        self.b.build_unconditional_branch(done).s()?;
        self.b.position_at_end(done);
        let phi = self.b.build_phi(self.ptr, "list").s()?;
        phi.add_incoming(&[(&list, here_end), (&r, slow_end)]);
        Ok(phi.as_basic_value().into_pointer_value())
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
        // (on the heap: owned, given to the step; lent to `done`)
        let cur = self.val_of(xp.as_basic_value(), ty, true);
        let lent = match cur {
            V::P(p, _) => V::P(p, false),
            x => x,
        };
        vals[var.0 as usize] = Some(lent);
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
        vals[var.0 as usize] = Some(cur);
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
        // the exit after the step's blocks, as the old path laid it out
        // (made before them, it sat between the test and the step, and
        // sieve's crossing-out loop ran 8% slower on Linux)
        if let Some(last) = fun.get_last_basic_block()
            && last != out
        {
            out.move_after(last).map_err(|_| "internal compiler error: loop exit")?;
        }
        self.b.position_at_end(out);
        Ok(cur)
    }

    fn ir_if(&mut self, f: &Function, ty: Ty, cond: V<'ctx>, then: &Region, els: &Region, vals: &mut Vec<Option<V<'ctx>>>) -> R<V<'ctx>> {
        // `a and b` / `a or b` (one branch the truth value the left side
        // decides): as the old path makes it, a branch past the right side
        // and a phi, not two arms; LLVM then keeps the branch and sinks
        // what only the right side needs into it (pip computed its edge
        // test on every step once the two arms became selects)
        if ty == Ty::Bool
            && let Some((decided, more)) = match (const_bool(then), const_bool(els)) {
                (None, Some(false)) => Some((false, then)),
                (Some(true), None) => Some((true, els)),
                _ => None,
            }
        {
            let a = self.truth(cond)?;
            let fun = self.cur_fn.ok_or("internal compiler error: `and` outside a function")?;
            let pre = self.b.get_insert_block().unwrap();
            let more_bb = self.ctx.append_basic_block(fun, if decided { "or_rhs" } else { "and_rhs" });
            let done = self.ctx.append_basic_block(fun, "logic_done");
            if decided {
                self.b.build_conditional_branch(a, done, more_bb).s()?;
            } else {
                self.b.build_conditional_branch(a, more_bb, done).s()?;
            }
            self.b.position_at_end(more_bb);
            let b = self.ir_region(f, more, vals)?;
            let b = self.truth(b)?;
            let more_end = self.b.get_insert_block().unwrap();
            self.b.build_unconditional_branch(done).s()?;
            self.b.position_at_end(done);
            let phi = self.b.build_phi(self.ctx.bool_type(), "logic").s()?;
            phi.add_incoming(&[(&a, pre), (&b, more_end)]);
            return Ok(V::B(phi.as_basic_value().into_int_value()));
        }
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
        // (a list or a record on the heap: owned from either branch)
        let owned = matches!(ty, Ty::Arr(..) | Ty::Obj(_)) && matches!(self.val_of(phi.as_basic_value(), ty, false), V::P(..));
        Ok(self.val_of(phi.as_basic_value(), ty, owned))
    }
}
