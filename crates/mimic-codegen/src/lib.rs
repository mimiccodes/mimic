//! LLVM code generation (via inkwell).
//!
//! Milestone 1 lowering: every Mimic value is a `mim_val*` handle into the
//! runtime array layer. Codegen inserts retain/release so that every value
//! has exactly one owner:
//!
//! * runtime calls and Mimic functions **borrow** their arguments and return
//!   an **owned** (+1) value;
//! * `let` bindings own their value and release it at scope exit;
//! * reads of immutable bindings are borrowed (no refcount traffic);
//!   reads of `mut` bindings are retained, so reassignment can't dangle.

use inkwell::basic_block::BasicBlock;
use inkwell::builder::Builder;
use inkwell::context::Context;
use inkwell::module::{Linkage, Module};
use inkwell::passes::PassBuilderOptions;
use inkwell::targets::{CodeModel, FileType, InitializationConfig, RelocMode, Target, TargetMachine, TargetTriple};
use inkwell::types::{BasicMetadataTypeEnum, BasicType, BasicTypeEnum, PointerType};
use inkwell::values::{BasicMetadataValueEnum, BasicValueEnum, FunctionValue, PointerValue};
use inkwell::{AddressSpace, IntPredicate, OptimizationLevel};
use mimic_check::{FnKind, ModId, Program, Resolved};
use mimic_syntax::ast::*;
use mimic_syntax::glyphs::Prim;
use std::fmt::Display;
use std::path::Path;

pub struct Settings {
    /// LLVM target triple; `None` = host.
    pub triple: Option<String>,
    pub opt_level: u8,
    /// Emit the runtime leak check at exit (debug builds).
    pub leak_check: bool,
    /// Emit a C `main` that calls `<root>::main::main`.
    pub emit_main: bool,
    /// Keep `pub` functions externally visible (library builds).
    pub export_pub: bool,
}

type R<T> = Result<T, String>;

trait OrStr<T> {
    fn s(self) -> R<T>;
}
impl<T, E: Display> OrStr<T> for Result<T, E> {
    fn s(self) -> R<T> {
        self.map_err(|e| e.to_string())
    }
}

/// Compile the checked program to a native object file (and optionally textual IR).
pub fn compile(p: &Program, settings: &Settings, obj: &Path, ir: Option<&Path>) -> R<()> {
    Target::initialize_all(&InitializationConfig::default());
    let triple = match &settings.triple {
        Some(t) => TargetTriple::create(t),
        None => TargetMachine::get_default_triple(),
    };
    let target = Target::from_triple(&triple).s()?;
    let (cpu, features) = if settings.triple.is_none() {
        (TargetMachine::get_host_cpu_name().to_string(), TargetMachine::get_host_cpu_features().to_string())
    } else {
        ("generic".to_string(), String::new())
    };
    let level = match settings.opt_level {
        0 => OptimizationLevel::None,
        1 => OptimizationLevel::Less,
        2 => OptimizationLevel::Default,
        _ => OptimizationLevel::Aggressive,
    };
    let tm = target
        .create_target_machine(&triple, &cpu, &features, level, RelocMode::PIC, CodeModel::Default)
        .ok_or_else(|| format!("LLVM cannot create a target machine for `{}`", triple.as_str().to_string_lossy()))?;

    let ctx = Context::create();
    let module = ctx.create_module(&p.root_package);
    module.set_triple(&triple);
    module.set_data_layout(&tm.get_target_data().get_data_layout());

    let mut cg = Cg {
        ctx: &ctx,
        module,
        b: ctx.create_builder(),
        p,
        ptr: ctx.ptr_type(AddressSpace::default()),
        fns: Vec::new(),
        cur_fn: None,
        cur_mod: 0,
        cur_ret: None,
        scopes: Vec::new(),
        lambdas: 0,
        purity: mimic_check::Purity::analyze(p),
    };
    cg.declare_all(settings)?;
    for fid in 0..p.fns.len() {
        cg.define(fid)?;
    }
    if settings.emit_main {
        cg.emit_main(settings.leak_check)?;
    }
    cg.module.verify().map_err(|e| format!("internal compiler error: invalid LLVM IR:\n{}", e.to_string()))?;
    if settings.opt_level > 0 {
        cg.module
            .run_passes(&format!("default<O{}>", settings.opt_level.min(3)), &tm, PassBuilderOptions::create())
            .s()?;
    }
    if let Some(ir) = ir {
        cg.module.print_to_file(ir).s()?;
    }
    tm.write_to_file(&cg.module, FileType::Object, obj).s()
}

/// A value produced by an expression: a handle plus whether we own a reference.
#[derive(Clone, Copy)]
struct V<'ctx> {
    v: PointerValue<'ctx>,
    owned: bool,
}

struct Local<'ctx> {
    name: String,
    slot: PointerValue<'ctx>,
    owned: bool,
    mutable: bool,
}

struct Cg<'ctx, 'p> {
    ctx: &'ctx Context,
    module: Module<'ctx>,
    b: Builder<'ctx>,
    p: &'p Program,
    ptr: PointerType<'ctx>,
    fns: Vec<Option<FunctionValue<'ctx>>>,
    cur_fn: Option<FunctionValue<'ctx>>,
    cur_mod: ModId,
    /// (runtime type code, description) of the current function's return annotation
    cur_ret: Option<(i32, String)>,
    scopes: Vec<Vec<Local<'ctx>>>,
    lambdas: usize,
    purity: mimic_check::Purity,
}

impl<'ctx> Cg<'ctx, '_> {
    // ---- runtime declarations ---------------------------------------

    fn rt(&self, name: &str) -> FunctionValue<'ctx> {
        if let Some(f) = self.module.get_function(name) {
            return f;
        }
        let p: BasicMetadataTypeEnum = self.ptr.into();
        let i64t: BasicMetadataTypeEnum = self.ctx.i64_type().into();
        let i32t: BasicMetadataTypeEnum = self.ctx.i32_type().into();
        let f64t: BasicMetadataTypeEnum = self.ctx.f64_type().into();
        let ptr = self.ptr;
        let void = self.ctx.void_type();
        let ty = match name {
            "mim_int" | "mim_bool" => ptr.fn_type(&[i64t], false),
            "mim_float" => ptr.fn_type(&[f64t], false),
            "mim_str" => ptr.fn_type(&[p, i64t], false),
            "mim_lit_int" => ptr.fn_type(&[p, i64t], false),
            "mim_lit_float" => ptr.fn_type(&[p, f64t], false),
            "mim_lit_str" => ptr.fn_type(&[p, p, i64t], false),
            "mim_array_lit" => ptr.fn_type(&[i64t, p], false),
            "mim_retain" | "mim_release" => void.fn_type(&[p], false),
            "mim_binop" | "mim_prim2" => ptr.fn_type(&[i32t, p, p], false),
            "mim_unop" | "mim_prim1" => ptr.fn_type(&[i32t, p], false),
            "mim_index" => ptr.fn_type(&[p, p], false),
            "mim_each" => ptr.fn_type(&[p, p, i32t], false),
            "mim_set_index" => void.fn_type(&[p, p, p], false),
            "mim_truthy" | "mim_as_i64" => self.ctx.i64_type().fn_type(&[p], false),
            "mim_as_f64" => self.ctx.f64_type().fn_type(&[p], false),
            "mim_cstr" | "mim_dataptr" | "mim_from_cstr" => ptr.fn_type(&[p], false),
            "mim_check_type" => void.fn_type(&[p, i32t, p], false),
            "mim_rt_init" => void.fn_type(&[i32t, p], false),
            "mim_rt_shutdown" => self.ctx.i32_type().fn_type(&[i32t], false),
            other => panic!("unknown runtime function {other}"),
        };
        self.module.add_function(name, ty, Some(Linkage::External))
    }

    fn call_rt(&self, name: &str, args: &[BasicMetadataValueEnum<'ctx>]) -> R<Option<BasicValueEnum<'ctx>>> {
        let f = self.rt(name);
        Ok(self.b.build_call(f, args, "").s()?.try_as_basic_value().basic())
    }

    fn call_rt_ptr(&self, name: &str, args: &[BasicMetadataValueEnum<'ctx>]) -> R<V<'ctx>> {
        let v = self.call_rt(name, args)?.ok_or("runtime call returned void")?.into_pointer_value();
        Ok(V { v, owned: true })
    }

    /// A fresh `ptr` global (initially null) that caches one literal.
    fn literal_slot(&self) -> PointerValue<'ctx> {
        let g = self.module.add_global(self.ptr, None, "lit");
        g.set_linkage(Linkage::Internal);
        g.set_initializer(&self.ptr.const_null());
        g.as_pointer_value()
    }

    /// Immortal values need no release.
    fn borrowed(&self, v: V<'ctx>) -> R<V<'ctx>> {
        Ok(V { v: v.v, owned: false })
    }

    fn i32c(&self, x: i32) -> BasicMetadataValueEnum<'ctx> {
        self.ctx.i32_type().const_int(x as u64, true).into()
    }

    fn null(&self) -> V<'ctx> {
        V { v: self.ptr.const_null(), owned: true }
    }

    fn own(&self, v: V<'ctx>) -> R<V<'ctx>> {
        if !v.owned {
            self.call_rt("mim_retain", &[v.v.into()])?;
        }
        Ok(V { v: v.v, owned: true })
    }

    fn drop_tmp(&self, v: V<'ctx>) -> R<()> {
        if v.owned {
            self.call_rt("mim_release", &[v.v.into()])?;
        }
        Ok(())
    }

    fn cstring(&self, s: &str) -> R<PointerValue<'ctx>> {
        Ok(self.b.build_global_string_ptr(s, "s").s()?.as_pointer_value())
    }

    // ---- declarations -----------------------------------------------

    fn declare_all(&mut self, settings: &Settings) -> R<()> {
        for (fid, f) in self.p.fns.iter().enumerate() {
            let fv = match &f.kind {
                FnKind::User(d) => {
                    let params: Vec<BasicMetadataTypeEnum> = d.params.iter().map(|_| self.ptr.into()).collect();
                    let linkage = if settings.export_pub && f.is_pub && self.p.mods[f.module].package == self.p.root_package {
                        Linkage::External
                    } else {
                        Linkage::Internal
                    };
                    Some(self.module.add_function(&f.symbol, self.ptr.fn_type(&params, false), Some(linkage)))
                }
                FnKind::Extern { .. } => None,
            };
            let _ = fid;
            self.fns.push(fv);
        }
        Ok(())
    }

    fn extern_fn(&self, decl: &ExternFn) -> FunctionValue<'ctx> {
        if let Some(f) = self.module.get_function(&decl.name) {
            return f;
        }
        let params: Vec<BasicMetadataTypeEnum> = decl.params.iter().map(|(_, t)| self.ctype(t).into()).collect();
        let ty = match &decl.ret {
            Some(t) => self.ctype(t).fn_type(&params, false),
            None => self.ctx.void_type().fn_type(&params, false),
        };
        self.module.add_function(&decl.name, ty, Some(Linkage::External))
    }

    fn ctype(&self, t: &Type) -> BasicTypeEnum<'ctx> {
        match t {
            Type::Named(n, _) => match n.as_str() {
                "i8" | "u8" | "c_char" => self.ctx.i8_type().into(),
                "i16" | "u16" => self.ctx.i16_type().into(),
                "i32" | "u32" => self.ctx.i32_type().into(),
                "f32" => self.ctx.f32_type().into(),
                "f64" => self.ctx.f64_type().into(),
                "bool" => self.ctx.bool_type().into(),
                "cstr" => self.ptr.into(),
                _ => self.ctx.i64_type().into(), // i64 u64 isize usize
            },
            Type::Ptr(..) | Type::Array(..) => self.ptr.into(),
        }
    }

    /// Mimic value → C argument.
    fn mimic_to_c(&self, v: PointerValue<'ctx>, t: &Type) -> R<BasicMetadataValueEnum<'ctx>> {
        let ty = self.ctype(t);
        Ok(match t {
            Type::Named(n, _) if n == "f64" || n == "f32" => {
                let f = self.call_rt("mim_as_f64", &[v.into()])?.unwrap().into_float_value();
                if n == "f32" { self.b.build_float_trunc(f, self.ctx.f32_type(), "").s()?.into() } else { f.into() }
            }
            Type::Named(n, _) if n == "bool" => {
                let i = self.call_rt("mim_as_i64", &[v.into()])?.unwrap().into_int_value();
                self.b.build_int_compare(IntPredicate::NE, i, self.ctx.i64_type().const_zero(), "").s()?.into()
            }
            Type::Named(n, _) if n == "cstr" => self.call_rt("mim_cstr", &[v.into()])?.unwrap().into(),
            Type::Ptr(..) => self.call_rt("mim_dataptr", &[v.into()])?.unwrap().into(),
            _ => {
                let i = self.call_rt("mim_as_i64", &[v.into()])?.unwrap().into_int_value();
                let it = ty.into_int_type();
                if it.get_bit_width() < 64 { self.b.build_int_truncate(i, it, "").s()?.into() } else { i.into() }
            }
        })
    }

    /// C return value → owned Mimic value.
    fn c_to_mimic(&self, r: BasicValueEnum<'ctx>, t: &Type) -> R<V<'ctx>> {
        match t {
            Type::Named(n, _) if n == "f64" || n == "f32" => {
                let mut f = r.into_float_value();
                if n == "f32" {
                    f = self.b.build_float_ext(f, self.ctx.f64_type(), "").s()?;
                }
                self.call_rt_ptr("mim_float", &[f.into()])
            }
            Type::Named(n, _) if n == "bool" => {
                let i = self.b.build_int_z_extend(r.into_int_value(), self.ctx.i64_type(), "").s()?;
                self.call_rt_ptr("mim_bool", &[i.into()])
            }
            Type::Named(n, _) if n == "cstr" => self.call_rt_ptr("mim_from_cstr", &[r.into()]),
            Type::Named(n, _) => {
                let i = r.into_int_value();
                let i = if i.get_type().get_bit_width() < 64 {
                    if n.starts_with('u') {
                        self.b.build_int_z_extend(i, self.ctx.i64_type(), "").s()?
                    } else {
                        self.b.build_int_s_extend(i, self.ctx.i64_type(), "").s()?
                    }
                } else {
                    i
                };
                self.call_rt_ptr("mim_int", &[i.into()])
            }
            _ => Err("unsupported FFI return type".into()),
        }
    }

    // ---- function bodies -----------------------------------------------

    fn entry_alloca(&self, name: &str) -> R<PointerValue<'ctx>> {
        let f = self.cur_fn.unwrap();
        let entry = f.get_first_basic_block().unwrap();
        let tmp = self.ctx.create_builder();
        match entry.get_first_instruction() {
            Some(i) => tmp.position_before(&i),
            None => tmp.position_at_end(entry),
        }
        tmp.build_alloca(self.ptr, name).s()
    }

    fn define(&mut self, fid: usize) -> R<()> {
        let f = &self.p.fns[fid];
        let FnKind::User(decl) = &f.kind else { return Ok(()) };
        let fv = self.fns[fid].unwrap();
        let path = self.p.fn_path(fid).join("::");
        self.cur_fn = Some(fv);
        self.cur_mod = f.module;
        self.cur_ret = match &decl.ret {
            Some(t) => Some((mimic_check::type_code(t).map_err(|d| d.msg)?, format!("return value of `{path}`"))),
            None => None,
        };
        let entry = self.ctx.append_basic_block(fv, "entry");
        self.b.position_at_end(entry);
        self.scopes = vec![Vec::new()];
        for (i, prm) in decl.params.iter().enumerate() {
            let arg = fv.get_nth_param(i as u32).unwrap().into_pointer_value();
            if let Some(t) = &prm.ty {
                let code = mimic_check::type_code(t).map_err(|d| d.msg)?;
                let what = self.cstring(&format!("argument `{}` of `{path}`", prm.name))?;
                self.call_rt("mim_check_type", &[arg.into(), self.i32c(code), what.into()])?;
            }
            let slot = self.entry_alloca(&prm.name)?;
            self.b.build_store(slot, arg).s()?;
            self.scopes[0].push(Local { name: prm.name.clone(), slot, owned: false, mutable: false });
        }
        let v = self.block(&decl.body)?;
        if !self.terminated() {
            self.ret(v)?;
        }
        self.scopes.clear();
        Ok(())
    }

    fn terminated(&self) -> bool {
        self.b.get_insert_block().and_then(|b| b.get_terminator()).is_some()
    }

    /// Release every owned local in every scope, check the return type, and return.
    fn ret(&mut self, v: V<'ctx>) -> R<()> {
        let v = self.own(v)?;
        for scope in self.scopes.iter().rev() {
            for l in scope.iter().rev() {
                if l.owned {
                    let cur = self.b.build_load(self.ptr, l.slot, "").s()?;
                    self.call_rt("mim_release", &[cur.into()])?;
                }
            }
        }
        if let Some((code, what)) = self.cur_ret.clone() {
            let w = self.cstring(&what)?;
            self.call_rt("mim_check_type", &[v.v.into(), self.i32c(code), w.into()])?;
        }
        self.b.build_return(Some(&v.v)).s()?;
        Ok(())
    }

    fn emit_main(&mut self, leak_check: bool) -> R<()> {
        let entry = self.p.entry().ok_or("no entry point")?;
        let i32t = self.ctx.i32_type();
        let ty = i32t.fn_type(&[i32t.into(), self.ptr.into()], false);
        let main = self.module.add_function("main", ty, None);
        let bb = self.ctx.append_basic_block(main, "entry");
        self.b.position_at_end(bb);
        let argc = main.get_nth_param(0).unwrap();
        let argv = main.get_nth_param(1).unwrap();
        self.call_rt("mim_rt_init", &[argc.into(), argv.into()])?;
        let r = self.b.build_call(self.fns[entry].unwrap(), &[], "").s()?.try_as_basic_value().basic().unwrap();
        self.call_rt("mim_release", &[r.into()])?;
        let code = self.call_rt("mim_rt_shutdown", &[self.i32c(leak_check as i32)])?.unwrap();
        self.b.build_return(Some(&code)).s()?;
        Ok(())
    }

    fn lookup(&self, name: &str) -> Option<&Local<'ctx>> {
        self.scopes.iter().rev().flat_map(|s| s.iter().rev()).find(|l| l.name == name)
    }

    fn block(&mut self, b: &Block) -> R<V<'ctx>> {
        self.scopes.push(Vec::new());
        for s in &b.stmts {
            self.stmt(s)?;
        }
        if self.terminated() {
            self.dead_block();
        }
        let v = match &b.tail {
            Some(e) => self.expr(e)?,
            None => self.null(),
        };
        let v = self.own(v)?;
        let scope = self.scopes.pop().unwrap();
        if !self.terminated() {
            for l in scope.iter().rev() {
                if l.owned {
                    let cur = self.b.build_load(self.ptr, l.slot, "").s()?;
                    self.call_rt("mim_release", &[cur.into()])?;
                }
            }
        }
        Ok(v)
    }

    /// After a `return`, keep emitting into an unreachable block.
    fn dead_block(&self) {
        let bb = self.ctx.append_basic_block(self.cur_fn.unwrap(), "dead");
        self.b.position_at_end(bb);
    }

    fn stmt(&mut self, s: &Stmt) -> R<()> {
        if self.terminated() {
            self.dead_block();
        }
        match s {
            Stmt::Let { name, mutable, ty, value, .. } => {
                let v = self.expr(value)?;
                let v = self.own(v)?;
                if let Some(t) = ty {
                    let code = mimic_check::type_code(t).map_err(|d| d.msg)?;
                    let w = self.cstring(&format!("`let {name}`"))?;
                    self.call_rt("mim_check_type", &[v.v.into(), self.i32c(code), w.into()])?;
                }
                let slot = self.entry_alloca(name)?;
                self.b.build_store(slot, v.v).s()?;
                self.scopes.last_mut().unwrap().push(Local { name: name.clone(), slot, owned: true, mutable: *mutable });
            }
            Stmt::Assign { name, index, value, .. } => {
                let slot = self.lookup(name).ok_or_else(|| format!("unknown variable {name}"))?.slot;
                match index {
                    None => {
                        let v = self.expr(value)?;
                        let v = self.own(v)?;
                        let old = self.b.build_load(self.ptr, slot, "").s()?;
                        self.b.build_store(slot, v.v).s()?;
                        self.call_rt("mim_release", &[old.into()])?;
                    }
                    Some(i) => {
                        let iv = self.expr(i)?;
                        let v = self.expr(value)?;
                        self.call_rt("mim_set_index", &[slot.into(), iv.v.into(), v.v.into()])?;
                        self.drop_tmp(iv)?;
                        self.drop_tmp(v)?;
                    }
                }
            }
            Stmt::Expr(e) => {
                let v = self.expr(e)?;
                if !self.terminated() {
                    self.drop_tmp(v)?;
                }
            }
            Stmt::Return(e, _) => {
                let v = match e {
                    Some(e) => self.expr(e)?,
                    None => self.null(),
                };
                if !self.terminated() {
                    self.ret(v)?;
                }
            }
        }
        Ok(())
    }

    fn lambda(&mut self, param: &str, body: &Expr) -> R<FunctionValue<'ctx>> {
        self.lambdas += 1;
        let name = format!("_mimic_lambda_{}", self.lambdas);
        let f = self.module.add_function(&name, self.ptr.fn_type(&[self.ptr.into()], false), Some(Linkage::Internal));
        let saved_bb: BasicBlock = self.b.get_insert_block().unwrap();
        let saved_fn = self.cur_fn.replace(f);
        let saved_scopes = std::mem::take(&mut self.scopes);
        let saved_ret = self.cur_ret.take();
        let entry = self.ctx.append_basic_block(f, "entry");
        self.b.position_at_end(entry);
        let slot = self.entry_alloca(param)?;
        self.b.build_store(slot, f.get_nth_param(0).unwrap()).s()?;
        self.scopes = vec![vec![Local { name: param.into(), slot, owned: false, mutable: false }]];
        let v = self.expr(body)?;
        if !self.terminated() {
            self.ret(v)?;
        }
        self.scopes = saved_scopes;
        self.cur_fn = saved_fn;
        self.cur_ret = saved_ret;
        self.b.position_at_end(saved_bb);
        Ok(f)
    }

    fn resolve_fn(&self, path: &[String]) -> R<usize> {
        match self.p.resolve(self.cur_mod, path)? {
            Resolved::Fn(f) => Ok(f),
            Resolved::Mod(_) => Err(format!("`{}` is a module", path.join("::"))),
        }
    }

    fn fn_pointer(&mut self, e: &Expr) -> R<PointerValue<'ctx>> {
        match &e.kind {
            ExprKind::Lambda { param, body } => Ok(self.lambda(param, body)?.as_global_value().as_pointer_value()),
            ExprKind::Path(path) => {
                let f = self.resolve_fn(path)?;
                Ok(self.fns[f].ok_or("extern function used as a value")?.as_global_value().as_pointer_value())
            }
            _ => Err("expected a function".into()),
        }
    }

    fn expr(&mut self, e: &Expr) -> R<V<'ctx>> {
        let i64t = self.ctx.i64_type();
        match &e.kind {
            // Literals are immortal values created once and cached in a global slot:
            // no allocation and no refcount traffic on the hot path.
            ExprKind::Int(i) => {
                let slot = self.literal_slot();
                self.borrowed(self.call_rt_ptr("mim_lit_int", &[slot.into(), i64t.const_int(*i as u64, true).into()])?)
            }
            ExprKind::Float(f) => {
                let slot = self.literal_slot();
                self.borrowed(self.call_rt_ptr("mim_lit_float", &[slot.into(), self.ctx.f64_type().const_float(*f).into()])?)
            }
            ExprKind::Bool(b) => self.borrowed(self.call_rt_ptr("mim_bool", &[i64t.const_int(*b as u64, false).into()])?),
            ExprKind::Str(s) => {
                let slot = self.literal_slot();
                let p = self.cstring(s)?;
                self.borrowed(self.call_rt_ptr("mim_lit_str", &[slot.into(), p.into(), i64t.const_int(s.len() as u64, false).into()])?)
            }
            ExprKind::Array(xs) => {
                let n = xs.len() as u32;
                let arr_ty = self.ptr.array_type(n.max(1));
                let f = self.cur_fn.unwrap();
                let entry = f.get_first_basic_block().unwrap();
                let tmp = self.ctx.create_builder();
                match entry.get_first_instruction() {
                    Some(i) => tmp.position_before(&i),
                    None => tmp.position_at_end(entry),
                }
                let arr = tmp.build_alloca(arr_ty, "elems").s()?;
                let mut vals = Vec::new();
                for (i, x) in xs.iter().enumerate() {
                    let v = self.expr(x)?;
                    let slot = unsafe {
                        self.b.build_in_bounds_gep(arr_ty, arr, &[i64t.const_zero(), i64t.const_int(i as u64, false)], "").s()?
                    };
                    self.b.build_store(slot, v.v).s()?;
                    vals.push(v);
                }
                let r = self.call_rt_ptr("mim_array_lit", &[i64t.const_int(n as u64, false).into(), arr.into()])?;
                for v in vals {
                    self.drop_tmp(v)?;
                }
                Ok(r)
            }
            ExprKind::Path(path) => {
                let l = self.lookup(&path[0]).ok_or_else(|| format!("unknown variable `{}`", path.join("::")))?;
                let (slot, mutable) = (l.slot, l.mutable);
                let v = self.b.build_load(self.ptr, slot, &path[0]).s()?.into_pointer_value();
                if mutable { self.own(V { v, owned: false }) } else { Ok(V { v, owned: false }) }
            }
            ExprKind::Call { callee, args } => {
                let mut vals = Vec::new();
                for a in args {
                    vals.push(self.expr(a)?);
                }
                let r = match &callee.kind {
                    ExprKind::Lambda { param, body } => {
                        let f = self.lambda(param, body)?;
                        let r = self.b.build_call(f, &[vals[0].v.into()], "").s()?;
                        V { v: r.try_as_basic_value().basic().unwrap().into_pointer_value(), owned: true }
                    }
                    ExprKind::Path(path) => {
                        let fid = self.resolve_fn(path)?;
                        match &self.p.fns[fid].kind {
                            FnKind::User(_) => {
                                let argv: Vec<BasicMetadataValueEnum> = vals.iter().map(|v| v.v.into()).collect();
                                let r = self.b.build_call(self.fns[fid].unwrap(), &argv, "").s()?;
                                V { v: r.try_as_basic_value().basic().unwrap().into_pointer_value(), owned: true }
                            }
                            FnKind::Extern { decl, .. } => {
                                let decl = decl.clone();
                                let f = self.extern_fn(&decl);
                                let mut argv = Vec::new();
                                for (v, (_, t)) in vals.iter().zip(&decl.params) {
                                    argv.push(self.mimic_to_c(v.v, t)?);
                                }
                                let r = self.b.build_call(f, &argv, "").s()?;
                                match (&decl.ret, r.try_as_basic_value().basic()) {
                                    (Some(t), Some(rv)) => self.c_to_mimic(rv, t)?,
                                    _ => self.null(),
                                }
                            }
                        }
                    }
                    _ => return Err("unsupported callee".into()),
                };
                for v in vals {
                    self.drop_tmp(v)?;
                }
                Ok(r)
            }
            ExprKind::Index { base, index } => {
                let b = self.expr(base)?;
                let i = self.expr(index)?;
                let r = self.call_rt_ptr("mim_index", &[b.v.into(), i.v.into()])?;
                self.drop_tmp(b)?;
                self.drop_tmp(i)?;
                Ok(r)
            }
            ExprKind::Unary { op, expr } => {
                let a = self.expr(expr)?;
                let r = self.call_rt_ptr("mim_unop", &[self.i32c(*op as i32), a.v.into()])?;
                self.drop_tmp(a)?;
                Ok(r)
            }
            ExprKind::Binary { op, lhs, rhs } => {
                let a = self.expr(lhs)?;
                let b = self.expr(rhs)?;
                let r = self.call_rt_ptr("mim_binop", &[self.i32c(*op as i32), a.v.into(), b.v.into()])?;
                self.drop_tmp(a)?;
                self.drop_tmp(b)?;
                Ok(r)
            }
            ExprKind::Prim { prim, args } => {
                let a = self.expr(&args[0])?;
                let r = if *prim == Prim::Each {
                    let f = self.fn_pointer(&args[1])?;
                    let pure = self.purity.operand_is_pure(self.p, self.cur_mod, &args[1]);
                    self.call_rt_ptr("mim_each", &[a.v.into(), f.into(), self.i32c(pure as i32)])?
                } else if prim.arity() == 2 {
                    let b = self.expr(&args[1])?;
                    let r = self.call_rt_ptr("mim_prim2", &[self.i32c(*prim as i32), a.v.into(), b.v.into()])?;
                    self.drop_tmp(b)?;
                    r
                } else {
                    self.call_rt_ptr("mim_prim1", &[self.i32c(*prim as i32), a.v.into()])?
                };
                self.drop_tmp(a)?;
                Ok(r)
            }
            ExprKind::Lambda { .. } => Err("lambdas can only be called or passed to ¨ (each)".into()),
            ExprKind::Block(b) | ExprKind::Unsafe(b) => self.block(b),
            ExprKind::If { cond, then, els } => {
                let c = self.expr(cond)?;
                let t = self.call_rt("mim_truthy", &[c.v.into()])?.unwrap().into_int_value();
                self.drop_tmp(c)?;
                let bit = self.b.build_int_compare(IntPredicate::NE, t, i64t.const_zero(), "cond").s()?;
                let f = self.cur_fn.unwrap();
                let then_bb = self.ctx.append_basic_block(f, "then");
                let else_bb = self.ctx.append_basic_block(f, "else");
                let merge_bb = self.ctx.append_basic_block(f, "merge");
                self.b.build_conditional_branch(bit, then_bb, else_bb).s()?;

                let mut incoming = Vec::new();
                self.b.position_at_end(then_bb);
                let tv = self.block(then)?;
                if !self.terminated() {
                    incoming.push((tv.v, self.b.get_insert_block().unwrap()));
                    self.b.build_unconditional_branch(merge_bb).s()?;
                }
                self.b.position_at_end(else_bb);
                let ev = match els {
                    Some(e) => {
                        let v = self.expr(e)?;
                        self.own(v)?
                    }
                    None => self.null(),
                };
                if !self.terminated() {
                    incoming.push((ev.v, self.b.get_insert_block().unwrap()));
                    self.b.build_unconditional_branch(merge_bb).s()?;
                }
                self.b.position_at_end(merge_bb);
                if incoming.is_empty() {
                    self.b.build_unreachable().s()?;
                    self.dead_block();
                    return Ok(self.null());
                }
                let phi = self.b.build_phi(self.ptr, "ifv").s()?;
                for (v, bb) in &incoming {
                    phi.add_incoming(&[(v, *bb)]);
                }
                Ok(V { v: phi.as_basic_value().into_pointer_value(), owned: true })
            }
        }
    }
}
