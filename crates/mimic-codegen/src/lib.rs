//! LLVM code generation (via inkwell), driven by static types.
//!
//! Every function is compiled once per *specialisation*: the argument types
//! it is called with (see `mimic_check::types`). Within a specialisation,
//! `i64`, `f64` and `bool` values live unboxed in registers (`i64`, `double`,
//! `i1`); everything else — arrays, strings, values whose type is only known
//! at run time (`Dyn`) — is a `mim_val*` handle into the runtime array layer.
//!
//! Ownership of handles follows one rule: runtime calls and Mimic functions
//! **borrow** their arguments and return an **owned** (+1) value. `let`
//! bindings own their value and release it at scope exit; reads of immutable
//! bindings are borrowed; reads of `mut` bindings are retained.
//!
//! Each specialisation comes in up to three kinds:
//! * `Plain`   — ordinary typed native code;
//! * `NetWrap` — the entry of a net-compiled function: runs it as an
//!   interaction net (parallel by default);
//! * `Native`  — depth-counted native body used by the runtime's native
//!   fallback and by native-to-native recursion.
//!
//! Boxed-ABI thunks (`ptr f(ptr…)`) connect typed code to the runtime:
//! `¨` callbacks, the net's native table, and library exports.

use inkwell::builder::Builder;
use inkwell::context::Context;
use inkwell::intrinsics::Intrinsic;
use inkwell::module::{Linkage, Module};
use inkwell::passes::PassBuilderOptions;
use inkwell::targets::{CodeModel, FileType, InitializationConfig, RelocMode, Target, TargetMachine, TargetTriple};
use inkwell::types::{BasicMetadataTypeEnum, BasicType, BasicTypeEnum, PointerType};
use inkwell::values::{BasicMetadataValueEnum, BasicValueEnum, FloatValue, FunctionValue, IntValue, PointerValue};
use inkwell::{AddressSpace, FloatPredicate, IntPredicate, OptimizationLevel};
use inkwell::basic_block::BasicBlock;
use mimic_check::types::{self, El, Key, Spec, Ty, id};
use mimic_check::{FnKind, ModId, Program, Resolved, TypeId};
use mimic_syntax::ast::*;
use mimic_syntax::glyphs::Prim;
use std::collections::HashMap;
use std::fmt::Display;
use std::path::Path;
use std::rc::Rc;

mod fuse;
pub mod tonet;

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
    /// Compile pure functions to interaction nets (parallel by default).
    pub use_net: bool,
    /// CPU model; `None` = the host CPU for native builds, `generic` otherwise.
    pub cpu: Option<String>,
    /// LLVM feature string (`+d,+m,-sse4.2`); `None` = the CPU's defaults.
    pub features: Option<String>,
    /// Target ABI name (`lp64d`, `ilp32f`, `elfv2`…), as Clang chooses it.
    pub abi: Option<String>,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Kind {
    Plain,
    NetWrap,
    Native,
}

/// Compile the checked program to a native object file (and optionally
/// textual LLVM IR and the generated interaction nets). Returns the file
/// written: the object, or — for backends that only emit assembly, such as
/// NVPTX — the assembly next to it (`.ptx` / `.s`).
pub fn compile(p: &Program, settings: &Settings, obj: &Path, ir: Option<&Path>, net_out: Option<&Path>) -> R<std::path::PathBuf> {
    Target::initialize_all(&InitializationConfig::default());
    let triple = match &settings.triple {
        Some(t) => TargetTriple::create(t),
        None => TargetMachine::get_default_triple(),
    };
    let target = Target::from_triple(&triple).s()?;
    let (mut cpu, mut features) = if settings.triple.is_none() {
        (TargetMachine::get_host_cpu_name().to_string(), TargetMachine::get_host_cpu_features().to_string())
    } else {
        (String::new(), String::new()) // LLVM's default CPU for the triple
    };
    if let Some(c) = &settings.cpu {
        cpu = c.clone();
        if settings.features.is_none() && settings.triple.is_none() {
            features.clear(); // an explicit CPU replaces the host's feature list
        }
    }
    if let Some(f) = &settings.features {
        features = f.clone();
    }
    let level = match settings.opt_level {
        0 => OptimizationLevel::None,
        1 => OptimizationLevel::Less,
        2 => OptimizationLevel::Default,
        _ => OptimizationLevel::Aggressive,
    };
    let mut opts = inkwell::targets::TargetMachineOptions::new()
        .set_cpu(&cpu)
        .set_features(&features)
        .set_level(level)
        .set_reloc_mode(RelocMode::PIC)
        .set_code_model(CodeModel::Default);
    if let Some(abi) = &settings.abi {
        opts = opts.set_abi(abi);
    }
    let tm = target
        .create_target_machine_from_options(&triple, opts)
        .ok_or_else(|| format!("LLVM cannot create a target machine for `{}`", triple.as_str().to_string_lossy()))?;

    let ctx = Context::create();
    let module = ctx.create_module(&p.root_package);
    module.set_triple(&triple);
    module.set_data_layout(&tm.get_target_data().get_data_layout());

    let purity = mimic_check::Purity::analyze(p);
    let plan = if settings.use_net { tonet::plan(p, &purity) } else { tonet::NetPlan::default() };
    if let Some(path) = net_out {
        std::fs::write(path, plan.book.to_string()).s()?;
    }

    // Specialisations to infer: the entry point, library exports, and a fully
    // dynamic version of every net-compiled function (the native table's fallback).
    let mut roots: Vec<Key> = Vec::new();
    if settings.emit_main
        && let Some(e) = p.entry()
    {
        roots.push((e, vec![]));
    }
    let exports: Vec<usize> = if settings.export_pub {
        p.fns
            .iter()
            .enumerate()
            .filter(|(_, f)| f.is_pub && !f.is_extern() && p.mods[f.module].package == p.root_package)
            .map(|(i, _)| i)
            .collect()
    } else {
        Vec::new()
    };
    for f in &exports {
        roots.push((*f, vec![Ty::Dyn; p.fns[*f].arity()]));
    }
    // `extern "C" fn`: exported to C in every kind of build
    let c_exports: Vec<usize> = p
        .fns
        .iter()
        .enumerate()
        .filter(|(_, f)| matches!(&f.kind, FnKind::User(d) if d.export_c) && p.mods[f.module].package == p.root_package)
        .map(|(i, _)| i)
        .collect();
    for f in &c_exports {
        roots.push((*f, vec![Ty::Dyn; p.fns[*f].arity()]));
    }
    for f in plan.def_index.keys() {
        roots.push((*f, vec![Ty::Dyn; p.fns[*f].arity()]));
    }
    let tinfo = types::infer(p, &roots);
    if let Some(e) = tinfo.errors.first() {
        return Err(format!("type error: {}", e.diag.msg));
    }
    let specs: HashMap<Key, Rc<Spec>> = tinfo.specs.into_iter().map(|(k, s)| (k, Rc::new(s))).collect();

    let mut cg = Cg {
        ctx: &ctx,
        module,
        b: ctx.create_builder(),
        p,
        ptr: ctx.ptr_type(AddressSpace::default()),
        size: match tm.get_target_data().get_pointer_byte_size(None) {
            2 => ctx.i16_type(),
            4 => ctx.i32_type(),
            _ => ctx.i64_type(),
        },
        specs,
        fns: HashMap::new(),
        thunks: HashMap::new(),
        queue: Vec::new(),
        purity,
        fn_descs: HashMap::new(),
        plan,
        net_ensure: None,
        cur_fn: None,
        cur_mod: 0,
        cur_spec: None,
        cur_ret: Ty::Unit,
        ret_what: String::new(),
        scopes: Vec::new(),
        native_frame: false,
        cur_depth: None,
        lambdas: 0,
        descs: HashMap::new(),
        lazies: Vec::new(),
        opt_level: settings.opt_level,
    };
    if !cg.plan.book.defs.is_empty() {
        let f = cg.module.add_function("_mimic_net_ensure", cg.ctx.void_type().fn_type(&[], false), Some(Linkage::Internal));
        cg.net_ensure = Some(f);
    }
    let entry_fn = match (settings.emit_main, p.entry()) {
        (true, Some(e)) => Some(cg.request(&(e, vec![]), Kind::Plain)?),
        _ => None,
    };
    for f in &exports {
        cg.export(*f)?;
    }
    for f in &c_exports {
        cg.export_c(*f)?;
    }
    cg.drain()?;
    cg.emit_book()?;
    cg.drain()?;
    if let Some(f) = entry_fn {
        cg.emit_main(f, settings.leak_check)?;
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
    // backends that only write assembly
    let tstr = triple.as_str().to_string_lossy().to_string();
    if tstr.starts_with("nvptx") || tstr.starts_with("xcore") {
        let asm = obj.with_extension(if tstr.starts_with("nvptx") { "ptx" } else { "s" });
        tm.write_to_file(&cg.module, FileType::Assembly, &asm).s()?;
        return Ok(asm);
    }
    match tm.write_to_file(&cg.module, FileType::Object, obj) {
        Ok(()) => Ok(obj.to_path_buf()),
        Err(_) => {
            let ext = if triple.as_str().to_string_lossy().starts_with("nvptx") { "ptx" } else { "s" };
            let asm = obj.with_extension(ext);
            tm.write_to_file(&cg.module, FileType::Assembly, &asm).s()?;
            Ok(asm)
        }
    }
}

/// A compiled value: an unboxed scalar, or a handle with an ownership flag.
#[derive(Clone, Copy)]
enum V<'ctx> {
    P(PointerValue<'ctx>, bool),
    I(IntValue<'ctx>),
    F(FloatValue<'ctx>),
    B(IntValue<'ctx>),
}

struct Local<'ctx> {
    name: String,
    slot: PointerValue<'ctx>,
    ty: Ty,
    owned: bool,
    mutable: bool,
}

struct Cg<'ctx, 'p> {
    ctx: &'ctx Context,
    module: Module<'ctx>,
    b: Builder<'ctx>,
    p: &'p Program,
    ptr: PointerType<'ctx>,
    /// C's `size_t` / `usize`: as wide as a pointer (32 bits on wasm32, …).
    size: inkwell::types::IntType<'ctx>,
    specs: HashMap<Key, Rc<Spec>>,
    fns: HashMap<(Key, Kind), FunctionValue<'ctx>>,
    thunks: HashMap<(Key, Kind), FunctionValue<'ctx>>,
    queue: Vec<(Key, Kind)>,
    purity: mimic_check::Purity,
    /// Descriptors of function values already emitted (`fn:<id>`, `op:<n>`).
    fn_descs: HashMap<String, PointerValue<'ctx>>,
    plan: tonet::NetPlan,
    net_ensure: Option<FunctionValue<'ctx>>,
    // current function
    cur_fn: Option<FunctionValue<'ctx>>,
    cur_mod: ModId,
    cur_spec: Option<Rc<Spec>>,
    cur_ret: Ty,
    ret_what: String,
    scopes: Vec<Vec<Local<'ctx>>>,
    native_frame: bool,
    /// Recursion depth (hidden `i32` argument of `Native` bodies).
    cur_depth: Option<IntValue<'ctx>>,
    lambdas: usize,
    /// Runtime descriptors of record types / union variants.
    descs: HashMap<(TypeId, Option<usize>), PointerValue<'ctx>>,
    /// `let` bindings of array expressions that are fused into their uses
    /// instead of being materialised (pointers into the Program's AST).
    lazies: Vec<(String, *const Expr)>,
    /// LLVM optimisation level (tail recursion becomes loops at ≥ 2).
    opt_level: u8,
}

/// Does `fid` call itself only in tail position, or as one operand of `+` / `×`
/// whose other operand has no self-call (LLVM's accumulator tail-recursion
/// elimination)? Then optimised builds compile the recursion to a loop.
fn loops_only(p: &Program, fid: usize, d: &FnDecl) -> bool {
    struct W<'a> {
        p: &'a Program,
        fid: usize,
        module: ModId,
        ok: bool,
    }
    impl W<'_> {
        fn is_self(&self, e: &Expr) -> bool {
            matches!(&e.kind, ExprKind::Call { callee, .. }
                if matches!(&callee.kind, ExprKind::Path(path) if matches!(self.p.resolve(self.module, path), Ok(Resolved::Fn(f)) if f == self.fid)))
        }
        /// any self-call anywhere in `e` is a non-tail call
        fn none(&mut self, e: &Expr) {
            if self.is_self(e) {
                self.ok = false;
            }
            self.children(e, false);
        }
        /// `e` is in tail position
        fn tail(&mut self, e: &Expr) {
            match &e.kind {
                ExprKind::Call { args, .. } if self.is_self(e) => args.iter().for_each(|a| self.none(a)),
                ExprKind::Binary { op: BinOp::Add | BinOp::Mul, lhs, rhs } if self.is_self(lhs) || self.is_self(rhs) => {
                    let (call, other) = if self.is_self(lhs) { (lhs, rhs) } else { (rhs, lhs) };
                    self.tail(call);
                    self.none(other);
                }
                ExprKind::If { cond, then, els } => {
                    self.none(cond);
                    self.block(then, true);
                    if let Some(x) = els {
                        self.tail(x);
                    }
                }
                ExprKind::Block(b) | ExprKind::Unsafe(b) => self.block(b, true),
                ExprKind::Match { scrut, arms } => {
                    self.none(scrut);
                    for a in arms {
                        if let Some(g) = &a.guard {
                            self.none(g);
                        }
                        self.tail(&a.body);
                    }
                }
                _ => self.none(e),
            }
        }
        fn block(&mut self, b: &Block, tail: bool) {
            for s in &b.stmts {
                match s {
                    Stmt::Return(Some(x), _) => self.tail(x),
                    Stmt::LetElse { value, els, .. } => {
                        self.none(value);
                        self.block(els, false);
                    }
                    Stmt::Let { value, .. } | Stmt::Expr(value) | Stmt::LetTuple { value, .. } => self.none(value),
                    Stmt::Assign { index, value, .. } => {
                        if let Some(i) = index {
                            self.none(i);
                        }
                        self.none(value);
                    }
                    Stmt::Return(None, _) => {}
                }
            }
            if let Some(t) = &b.tail {
                if tail { self.tail(t) } else { self.none(t) }
            }
        }
        fn children(&mut self, e: &Expr, _: bool) {
            match &e.kind {
                ExprKind::Array(xs) | ExprKind::Tuple(xs) | ExprKind::Prim { args: xs, .. } => xs.iter().for_each(|x| self.none(x)),
                ExprKind::Call { callee, args } => {
                    self.none(callee);
                    args.iter().for_each(|x| self.none(x));
                }
                ExprKind::Index { base, index } => {
                    self.none(base);
                    self.none(index);
                }
                ExprKind::Unary { expr, .. } | ExprKind::Field { base: expr, .. } | ExprKind::Named { value: expr, .. } => self.none(expr),
                ExprKind::Binary { lhs, rhs, .. } => {
                    self.none(lhs);
                    self.none(rhs);
                }
                ExprKind::If { cond, then, els } => {
                    self.none(cond);
                    self.block(then, false);
                    if let Some(x) = els {
                        self.none(x);
                    }
                }
                ExprKind::Block(b) | ExprKind::Unsafe(b) => self.block(b, false),
                ExprKind::Record { fields, .. } => fields.iter().for_each(|(_, x, _)| self.none(x)),
                ExprKind::With { base, fields } => {
                    self.none(base);
                    fields.iter().for_each(|(_, x, _)| self.none(x));
                }
                ExprKind::Match { scrut, arms } => {
                    self.none(scrut);
                    for a in arms {
                        if let Some(g) = &a.guard {
                            self.none(g);
                        }
                        self.none(&a.body);
                    }
                }
                ExprKind::Interp(parts) => parts.iter().for_each(|p| {
                    if let InterpPart::Expr(x) = p {
                        self.none(x)
                    }
                }),
                ExprKind::Lambda { body, .. } => self.none(body),
                _ => {}
            }
        }
    }
    let mut w = W { p, fid, module: p.fns[fid].module, ok: true };
    w.block(&d.body, true);
    w.ok
}

/// Native recursion is bounded: deeper calls go to the heap-allocated net.
const NATIVE_LIMIT: u64 = 2048;
/// On WebAssembly (one ~1 MB engine stack); must match runtime/mimic_net.c.
const NATIVE_LIMIT_WASM: u64 = 256;

/// Runtime type codes (`TY_*` in mimic_rt.c) for checked unboxing.
fn type_code(t: Ty) -> i32 {
    match t {
        Ty::I64 => 1,
        Ty::F64 => 2,
        Ty::Bool => 3,
        _ => 0,
    }
}

fn ty_suffix(tys: &[Ty]) -> String {
    tys.iter()
        .map(|t| match t {
            Ty::I64 => "i".to_string(),
            Ty::F64 => "f".to_string(),
            Ty::Bool => "b".to_string(),
            Ty::Dyn => "d".to_string(),
            other => format!("{other}").replace(['[', ']', '(', ')', '?'], "_"),
        })
        .collect::<Vec<_>>()
        .join("")
}

impl<'ctx> Cg<'ctx, '_> {
    // ---- types & values -----------------------------------------------

    fn repr(&self, t: Ty) -> BasicTypeEnum<'ctx> {
        match t {
            Ty::I64 => self.ctx.i64_type().into(),
            Ty::F64 => self.ctx.f64_type().into(),
            Ty::Bool => self.ctx.bool_type().into(),
            _ => self.ptr.into(),
        }
    }

    /// A function's address as a plain data pointer. On targets that keep code
    /// in its own address space (AVR's program memory) this is a cast.
    /// A stack slot as a plain pointer. On targets whose stack lives in its own
    /// address space (AMDGPU: 5) this is a cast to the generic one.
    pub(crate) fn to_generic(&self, b: &inkwell::builder::Builder<'ctx>, p: PointerValue<'ctx>) -> R<PointerValue<'ctx>> {
        if p.get_type().get_address_space() == self.ptr.get_address_space() {
            Ok(p)
        } else {
            b.build_address_space_cast(p, self.ptr, "").s()
        }
    }

    /// A global's address as a plain pointer (GPU targets keep globals in
    /// their own address space).
    pub(crate) fn gaddr(&self, g: inkwell::values::GlobalValue<'ctx>) -> PointerValue<'ctx> {
        let p = g.as_pointer_value();
        if p.get_type().get_address_space() == self.ptr.get_address_space() { p } else { p.const_address_space_cast(self.ptr) }
    }

    pub(crate) fn fn_addr(&self, f: FunctionValue<'ctx>) -> PointerValue<'ctx> {
        let p = f.as_global_value().as_pointer_value();
        if p.get_type().get_address_space() == self.ptr.get_address_space() { p } else { p.const_address_space_cast(self.ptr) }
    }

    fn fn_type(&self, params: &[Ty], ret: Ty) -> inkwell::types::FunctionType<'ctx> {
        let ps: Vec<BasicMetadataTypeEnum> = params.iter().map(|t| self.repr(*t).into()).collect();
        self.repr(ret).fn_type(&ps, false)
    }

    fn basic(&self, v: V<'ctx>) -> BasicValueEnum<'ctx> {
        match v {
            V::P(p, _) => p.into(),
            V::I(i) | V::B(i) => i.into(),
            V::F(f) => f.into(),
        }
    }

    fn val_of(&self, bv: BasicValueEnum<'ctx>, t: Ty, owned: bool) -> V<'ctx> {
        match t {
            Ty::I64 => V::I(bv.into_int_value()),
            Ty::F64 => V::F(bv.into_float_value()),
            Ty::Bool => V::B(bv.into_int_value()),
            _ => V::P(bv.into_pointer_value(), owned),
        }
    }

    fn null(&self) -> V<'ctx> {
        V::P(self.ptr.const_null(), true)
    }

    fn own(&self, v: V<'ctx>) -> R<V<'ctx>> {
        if let V::P(p, false) = v {
            self.call_rt("mim_retain", &[p.into()])?;
            return Ok(V::P(p, true));
        }
        Ok(v)
    }

    fn drop_tmp(&self, v: V<'ctx>) -> R<()> {
        if let V::P(p, true) = v {
            self.call_rt("mim_release", &[p.into()])?;
        }
        Ok(())
    }

    /// Box a value into a runtime handle (owned for new boxes).
    fn boxed(&self, v: V<'ctx>) -> R<V<'ctx>> {
        Ok(match v {
            V::P(..) => v,
            V::I(i) => self.call_rt_ptr("mim_int", &[i.into()])?,
            V::F(f) => self.call_rt_ptr("mim_float", &[f.into()])?,
            V::B(b) => {
                let i = self.b.build_int_z_extend(b, self.ctx.i64_type(), "").s()?;
                self.call_rt_ptr("mim_bool", &[i.into()])?
            }
        })
    }

    fn to_i64(&self, v: V<'ctx>) -> R<IntValue<'ctx>> {
        Ok(match v {
            V::I(i) => i,
            V::B(b) => self.b.build_int_z_extend(b, self.ctx.i64_type(), "").s()?,
            V::F(f) => self.b.build_float_to_signed_int(f, self.ctx.i64_type(), "").s()?,
            V::P(p, _) => self.call_rt("mim_as_i64", &[p.into()])?.unwrap().into_int_value(),
        })
    }

    fn to_f64(&self, v: V<'ctx>) -> R<FloatValue<'ctx>> {
        Ok(match v {
            V::F(f) => f,
            V::I(i) => self.b.build_signed_int_to_float(i, self.ctx.f64_type(), "").s()?,
            V::B(b) => self.b.build_unsigned_int_to_float(b, self.ctx.f64_type(), "").s()?,
            V::P(p, _) => self.call_rt("mim_as_f64", &[p.into()])?.unwrap().into_float_value(),
        })
    }

    /// Truth of a condition (`i1`).
    fn truth(&self, v: V<'ctx>) -> R<IntValue<'ctx>> {
        Ok(match v {
            V::B(b) => b,
            V::I(i) => self.b.build_int_compare(IntPredicate::NE, i, self.ctx.i64_type().const_zero(), "").s()?,
            V::F(f) => self.b.build_float_compare(FloatPredicate::UNE, f, self.ctx.f64_type().const_zero(), "").s()?,
            V::P(p, _) => {
                let t = self.call_rt("mim_truthy", &[p.into()])?.unwrap().into_int_value();
                self.drop_tmp(v)?;
                self.b.build_int_compare(IntPredicate::NE, t, self.ctx.i64_type().const_zero(), "").s()?
            }
        })
    }

    /// Convert a value to the representation of static type `t`. Unboxing a
    /// handle checks its run-time type (`what` names the slot in the message).
    fn as_ty(&self, v: V<'ctx>, t: Ty, what: &str) -> R<V<'ctx>> {
        match (t, v) {
            (Ty::I64, V::I(_)) | (Ty::F64, V::F(_)) | (Ty::Bool, V::B(_)) => Ok(v),
            (Ty::I64, V::B(_)) => Ok(V::I(self.to_i64(v)?)),
            (Ty::F64, V::I(_) | V::B(_)) => Ok(V::F(self.to_f64(v)?)),
            (Ty::I64 | Ty::F64 | Ty::Bool, V::P(p, _)) => {
                let w = self.cstring(what)?;
                self.call_rt("mim_check_type", &[p.into(), self.i32c(type_code(t)), w.into()])?;
                let r = match t {
                    Ty::I64 => V::I(self.to_i64(v)?),
                    Ty::F64 => V::F(self.to_f64(v)?),
                    _ => {
                        let i = self.call_rt("mim_as_i64", &[p.into()])?.unwrap().into_int_value();
                        V::B(self.b.build_int_compare(IntPredicate::NE, i, self.ctx.i64_type().const_zero(), "").s()?)
                    }
                };
                self.drop_tmp(v)?;
                Ok(r)
            }
            (Ty::I64, V::F(_)) => Ok(V::I(self.to_i64(v)?)),
            (Ty::Bool, V::I(_) | V::F(_)) => Ok(V::B(self.truth(v)?)),
            _ => self.boxed(v),
        }
    }

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
            "mim_lit_str" => ptr.fn_type(&[p, p, i64t], false),
            "mim_array_lit" => ptr.fn_type(&[i64t, p], false),
            "mim_retain" | "mim_release" | "mim_panic" => void.fn_type(&[p], false),
            "mim_binop" | "mim_prim2" => ptr.fn_type(&[i32t, p, p], false),
            "mim_unop" | "mim_prim1" => ptr.fn_type(&[i32t, p], false),
            "mim_index" => ptr.fn_type(&[p, p], false),
            "mim_pat_is" => self.ctx.i64_type().fn_type(&[p, p], false),
            "mim_pat_len" => self.ctx.i64_type().fn_type(&[p], false),
            "mim_pat_at" | "mim_pat_rest" => ptr.fn_type(&[p, i64t], false),
            "mim_each" => ptr.fn_type(&[p, p, i32t], false),
            "mim_set_index" => void.fn_type(&[p, p, p], false),
            "mim_truthy" | "mim_as_i64" => self.ctx.i64_type().fn_type(&[p], false),
            "mim_as_f64" => self.ctx.f64_type().fn_type(&[p], false),
            "mim_cstr" | "mim_dataptr" | "mim_from_cstr" => ptr.fn_type(&[p], false),
            "mim_check_type" => void.fn_type(&[p, i32t, p], false),
            "mim_is_type" => self.ctx.i32_type().fn_type(&[p, i32t], false),
            "mim_rt_init" => void.fn_type(&[i32t, p], false),
            "mim_rt_ensure" => void.fn_type(&[], false),
            "mim_closure" => ptr.fn_type(&[p, i64t, p], false),
            "mim_call" => ptr.fn_type(&[p, i32t, p], false),
            "mim_each_fn" => ptr.fn_type(&[p, p], false),
            "mim_map_lit" => ptr.fn_type(&[i64t, p, p], false),
            "mim_gather" | "mim_trail" => ptr.fn_type(&[p, p], false),
            "mim_evolve" => ptr.fn_type(&[p, i64t, p], false),
            "mim_settle" => ptr.fn_type(&[p, p, p], false),
            "mim_cstr_dup" => ptr.fn_type(&[p], false),
            "mim_rt_shutdown" => self.ctx.i32_type().fn_type(&[i32t], false),
            "mim_net_book_once" => void.fn_type(&[p, p, i32t, p, p, p], false),
            "mim_deep_enter" | "mim_deep_leave" => void.fn_type(&[], false),
            "mim_net_call" => ptr.fn_type(&[i32t, i32t, p], false),
            "mim_obj" => ptr.fn_type(&[p, i64t, p], false),
            "mim_obj_tag" => self.ctx.i64_type().fn_type(&[p], false),
            "mim_obj_get" => ptr.fn_type(&[p, i64t], false),
            "mim_obj_get_named" => ptr.fn_type(&[p, p], false),
            "mim_obj_with" => ptr.fn_type(&[p, i64t, p], false),
            "mim_format" => ptr.fn_type(&[p], false),
            "mim_concat" => ptr.fn_type(&[i64t, p], false),
            "mim_str_eq" | "mim_str_cmp" | "mim_equal" => self.ctx.i64_type().fn_type(&[p, p], false),
            "mim_unwrap_ok" => self.ctx.i64_type().fn_type(&[p], false),
            "mim_par_for" => void.fn_type(&[i64t, p, p], false),
            "mim_par_grain" => self.ctx.i64_type().fn_type(&[], false),
            "mim_alloc_vec" => ptr.fn_type(&[i32t, i64t], false),
            "mim_vec_len" => self.ctx.i64_type().fn_type(&[p, i32t], false),
            "mim_route" => self.ctx.i64_type().fn_type(&[p, i64t, p, p, p], false),
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
        Ok(V::P(v, true))
    }

    fn i32c(&self, x: i32) -> BasicMetadataValueEnum<'ctx> {
        self.ctx.i32_type().const_int(x as u64, true).into()
    }

    fn cstring(&self, s: &str) -> R<PointerValue<'ctx>> {
        let g = self.b.build_global_string_ptr(s, "s").s()?;
        Ok(self.gaddr(g))
    }

    fn intrinsic(&self, name: &str) -> R<FunctionValue<'ctx>> {
        let i = Intrinsic::find(name).ok_or_else(|| format!("missing LLVM intrinsic {name}"))?;
        i.get_declaration(&self.module, &[self.ctx.f64_type().into()]).ok_or_else(|| format!("cannot declare {name}"))
    }

    /// Branch to a panic when `cond` holds; continue in a fresh block otherwise.
    fn panic_if(&self, cond: IntValue<'ctx>, msg: &str) -> R<()> {
        let f = self.cur_fn.unwrap();
        let bad = self.ctx.append_basic_block(f, "panic");
        let ok = self.ctx.append_basic_block(f, "ok");
        self.b.build_conditional_branch(cond, bad, ok).s()?;
        self.b.position_at_end(bad);
        let m = self.cstring(msg)?;
        self.call_rt("mim_panic", &[m.into()])?;
        self.b.build_unreachable().s()?;
        self.b.position_at_end(ok);
        Ok(())
    }

    // ---- specialisations ----------------------------------------------

    fn spec(&self, k: &Key) -> R<Rc<Spec>> {
        self.specs.get(k).cloned().ok_or_else(|| format!("internal compiler error: no type information for {}", self.p.fn_path(k.0).join("::")))
    }

    /// The LLVM function for a specialisation, declared on first use.
    fn request(&mut self, k: &Key, kind: Kind) -> R<FunctionValue<'ctx>> {
        if let Some(f) = self.fns.get(&(k.clone(), kind)) {
            return Ok(*f);
        }
        let s = self.spec(k)?;
        let suffix = match kind {
            Kind::Plain => "",
            Kind::NetWrap => ".net",
            Kind::Native => ".native",
        };
        let name = format!("{}${}{}", self.p.fns[k.0].symbol, ty_suffix(&k.1), suffix);
        let mut ft = self.fn_type(&s.params, s.ret);
        if kind == Kind::Native {
            // hidden trailing argument: the native recursion depth
            let mut ps: Vec<BasicMetadataTypeEnum> = s.params.iter().map(|t| self.repr(*t).into()).collect();
            ps.push(self.ctx.i32_type().into());
            ft = self.repr(s.ret).fn_type(&ps, false);
        }
        let f = self.module.add_function(&name, ft, Some(Linkage::Internal));
        self.fns.insert((k.clone(), kind), f);
        self.queue.push((k.clone(), kind));
        Ok(f)
    }

    fn drain(&mut self) -> R<()> {
        while let Some((k, kind)) = self.queue.pop() {
            self.define(&k, kind)?;
        }
        Ok(())
    }

    /// Which kind of a callee to call from the current context.
    fn kind_for(&self, fid: usize) -> Kind {
        if self.plan.entered.contains(&fid) {
            if self.native_frame { Kind::Native } else { Kind::NetWrap }
        } else {
            Kind::Plain
        }
    }

    fn entry_alloca(&self, t: Ty, name: &str) -> R<PointerValue<'ctx>> {
        let f = self.cur_fn.unwrap();
        let entry = f.get_first_basic_block().unwrap();
        let tmp = self.ctx.create_builder();
        match entry.get_first_instruction() {
            Some(i) => tmp.position_before(&i),
            None => tmp.position_at_end(entry),
        }
        let a = tmp.build_alloca(self.repr(t), name).s()?;
        self.to_generic(&tmp, a)
    }

    /// A stack slot of an arbitrary LLVM type, in the entry block.
    fn entry_alloca_ty(&self, t: BasicTypeEnum<'ctx>, name: &str) -> R<PointerValue<'ctx>> {
        let f = self.cur_fn.unwrap();
        let entry = f.get_first_basic_block().unwrap();
        let tmp = self.ctx.create_builder();
        match entry.get_first_instruction() {
            Some(i) => tmp.position_before(&i),
            None => tmp.position_at_end(entry),
        }
        let a = tmp.build_alloca(t, name).s()?;
        self.to_generic(&tmp, a)
    }

    fn define(&mut self, k: &Key, kind: Kind) -> R<()> {
        let fv = self.fns[&(k.clone(), kind)];
        let spec = self.spec(k)?;
        // the Program's own AST: expression types are keyed by node address
        let p = self.p;
        let FnKind::User(decl) = &p.fns[k.0].kind else { return Ok(()) };
        let path = self.p.fn_path(k.0).join("::");
        self.cur_fn = Some(fv);
        self.cur_mod = self.p.fns[k.0].module;
        self.cur_spec = Some(spec.clone());
        self.cur_ret = spec.ret;
        self.ret_what = format!("return value of `{path}`");
        self.native_frame = false;
        let entry = self.ctx.append_basic_block(fv, "entry");
        self.b.position_at_end(entry);
        self.scopes = vec![Vec::new()];
        for (i, prm) in decl.params.iter().enumerate() {
            let t = spec.params[i];
            let slot = self.entry_alloca(t, &prm.name)?;
            self.b.build_store(slot, fv.get_nth_param(i as u32).unwrap()).s()?;
            self.scopes[0].push(Local { name: prm.name.clone(), slot, ty: t, owned: false, mutable: false });
        }
        match kind {
            Kind::NetWrap => {
                // Parallel by default: run this function as an interaction net.
                let def = self.plan.def_index[&k.0];
                self.b.build_call(self.net_ensure.unwrap(), &[], "").s()?;
                let n = decl.params.len() as u32;
                let arr_ty = self.ptr.array_type(n.max(1));
                let arr = self.b.build_alloca(arr_ty, "args").s()?;
                let arr = self.to_generic(&self.b, arr)?;
                let i64t = self.ctx.i64_type();
                let mut temps = Vec::new();
                for i in 0..n {
                    let v = self.val_of(fv.get_nth_param(i).unwrap(), spec.params[i as usize], false);
                    let b = self.boxed(v)?;
                    let V::P(bp, _) = b else { unreachable!() };
                    let slot = unsafe { self.b.build_in_bounds_gep(arr_ty, arr, &[i64t.const_zero(), i64t.const_int(i as u64, false)], "").s()? };
                    self.b.build_store(slot, bp).s()?;
                    temps.push(b);
                }
                let r = self.call_rt_ptr("mim_net_call", &[self.i32c(def as i32), self.i32c(n as i32), arr.into()])?;
                for t in temps {
                    self.drop_tmp(t)?;
                }
                self.ret(r)?;
                self.scopes.clear();
                return Ok(());
            }
            Kind::Native if self.opt_level >= 2 && loops_only(self.p, k.0, decl) => {
                // every self-call is a tail call (or `x + f(…)` / `x × f(…)`):
                // LLVM turns the recursion into a loop, so it cannot grow the stack
                let depth = fv.get_nth_param(decl.params.len() as u32).unwrap().into_int_value();
                self.native_frame = true;
                self.cur_depth = Some(depth);
            }
            Kind::Native => {
                // Bounded native recursion: past the limit, hand the call to the net.
                let depth = fv.get_nth_param(decl.params.len() as u32).unwrap().into_int_value();
                let wasm = self.module.get_triple().as_str().to_string_lossy().starts_with("wasm");
                let limit = self.ctx.i32_type().const_int(if wasm { NATIVE_LIMIT_WASM } else { NATIVE_LIMIT }, false);
                let is_deep = self.b.build_int_compare(IntPredicate::UGT, depth, limit, "deep").s()?;
                let deep_bb = self.ctx.append_basic_block(fv, "deep");
                let body_bb = self.ctx.append_basic_block(fv, "body");
                self.b.build_conditional_branch(is_deep, deep_bb, body_bb).s()?;
                self.b.position_at_end(deep_bb);
                let wrap = self.request(k, Kind::NetWrap)?;
                let args: Vec<BasicMetadataValueEnum> = (0..decl.params.len()).map(|i| fv.get_nth_param(i as u32).unwrap().into()).collect();
                self.call_rt("mim_deep_enter", &[])?;
                let r = self.b.build_call(wrap, &args, "").s()?.try_as_basic_value().basic().unwrap();
                self.call_rt("mim_deep_leave", &[])?;
                self.b.build_return(Some(&r)).s()?;
                self.b.position_at_end(body_bb);
                self.native_frame = true;
                self.cur_depth = Some(depth);
            }
            Kind::Plain => {}
        }
        let v = self.block(&decl.body)?;
        if !self.terminated() {
            self.ret(v)?;
        }
        self.scopes.clear();
        self.native_frame = false;
        self.cur_depth = None;
        Ok(())
    }

    fn terminated(&self) -> bool {
        self.b.get_insert_block().and_then(|b| b.get_terminator()).is_some()
    }

    /// Convert to the return type, release every owned local, and return.
    fn ret(&mut self, v: V<'ctx>) -> R<()> {
        let v = match self.cur_ret {
            Ty::Unit | Ty::Never => {
                self.drop_tmp(v)?;
                self.null()
            }
            t => {
                let w = self.ret_what.clone();
                let v = self.as_ty(v, t, &w)?;
                self.own(v)?
            }
        };
        for scope in self.scopes.iter().rev() {
            self.release_scope(scope)?;
        }
        let bv = self.basic(v);
        self.b.build_return(Some(&bv)).s()?;
        Ok(())
    }

    // ---- boxed-ABI thunks ---------------------------------------------

    /// Run `f` with the builder positioned in a fresh function, restoring the
    /// current function state afterwards.
    fn in_new_fn<T>(&mut self, fv: FunctionValue<'ctx>, f: impl FnOnce(&mut Self) -> R<T>) -> R<T> {
        let saved_bb = self.b.get_insert_block();
        let saved_fn = self.cur_fn.replace(fv);
        let saved_scopes = std::mem::take(&mut self.scopes);
        let saved_native = std::mem::replace(&mut self.native_frame, false);
        let saved_depth = self.cur_depth.take();
        let saved_ret = self.cur_ret;
        let saved_what = std::mem::take(&mut self.ret_what);
        let entry = self.ctx.append_basic_block(fv, "entry");
        self.b.position_at_end(entry);
        let r = f(self);
        self.cur_fn = saved_fn;
        self.scopes = saved_scopes;
        self.native_frame = saved_native;
        self.cur_depth = saved_depth;
        self.cur_ret = saved_ret;
        self.ret_what = saved_what;
        if let Some(bb) = saved_bb {
            self.b.position_at_end(bb);
        }
        r
    }

    /// `ptr thunk(ptr…)`: unbox arguments (checked), call the typed function, box the result.
    fn thunk(&mut self, k: &Key, kind: Kind) -> R<FunctionValue<'ctx>> {
        if let Some(f) = self.thunks.get(&(k.clone(), kind)) {
            return Ok(*f);
        }
        let target = self.request(k, kind)?;
        let spec = self.spec(k)?;
        let n = spec.params.len();
        let name = format!("{}$box", target.get_name().to_str().unwrap());
        let ps: Vec<BasicMetadataTypeEnum> = (0..n).map(|_| self.ptr.into()).collect();
        let f = self.module.add_function(&name, self.ptr.fn_type(&ps, false), Some(Linkage::Internal));
        self.thunks.insert((k.clone(), kind), f);
        let path = self.p.fn_path(k.0).join("::");
        let pnames: Vec<String> = match &self.p.fns[k.0].kind {
            FnKind::User(d) => d.params.iter().map(|p| p.name.clone()).collect(),
            _ => vec![],
        };
        self.in_new_fn(f, |cg| {
            let mut args = Vec::new();
            for (i, pname) in pnames.iter().enumerate().take(n) {
                let p = f.get_nth_param(i as u32).unwrap().into_pointer_value();
                let v = cg.as_ty(V::P(p, false), spec.params[i], &format!("argument `{pname}` of `{path}`"))?;
                args.push(cg.basic(v).into());
            }
            if kind == Kind::Native {
                args.push(cg.ctx.i32_type().const_zero().into()); // a fresh native stack
            }
            let r = cg.b.build_call(target, &args, "").s()?.try_as_basic_value().basic().unwrap();
            let v = cg.val_of(r, spec.ret, true);
            let b = cg.boxed(v)?;
            let bv = cg.basic(b);
            cg.b.build_return(Some(&bv)).s()?;
            Ok(())
        })?;
        Ok(f)
    }

    /// The native-table entry for a net definition: dispatch to the typed
    /// specialisation when the run-time arguments fit it, else the dynamic one.
    fn dispatch_thunk(&mut self, fid: usize) -> R<FunctionValue<'ctx>> {
        let n = self.p.fns[fid].arity();
        let dyn_key: Key = (fid, vec![Ty::Dyn; n]);
        let kind = if self.plan.entered.contains(&fid) { Kind::Native } else { Kind::Plain };
        // the preferred typed specialisation: all-unboxed parameters, deterministic choice
        let mut typed: Vec<&Key> = self.specs.keys().filter(|k| k.0 == fid && !k.1.is_empty() && k.1.iter().all(|t| t.is_unboxed())).collect();
        typed.sort_by_key(|k| ty_suffix(&k.1));
        let Some(tk) = typed.first().map(|k| (*k).clone()) else { return self.thunk(&dyn_key, kind) };
        let typed_thunk = self.thunk(&tk, kind)?;
        let dyn_thunk = self.thunk(&dyn_key, kind)?;
        let name = format!("{}$dispatch", self.p.fns[fid].symbol);
        let ps: Vec<BasicMetadataTypeEnum> = (0..n).map(|_| self.ptr.into()).collect();
        let f = self.module.add_function(&name, self.ptr.fn_type(&ps, false), Some(Linkage::Internal));
        let tys = tk.1.clone();
        self.in_new_fn(f, |cg| {
            let mut ok = cg.ctx.bool_type().const_int(1, false);
            for (i, t) in tys.iter().enumerate() {
                let p = f.get_nth_param(i as u32).unwrap();
                let r = cg.call_rt("mim_is_type", &[p.into(), cg.i32c(type_code(*t))])?.unwrap().into_int_value();
                let b = cg.b.build_int_compare(IntPredicate::NE, r, cg.ctx.i32_type().const_zero(), "").s()?;
                ok = cg.b.build_and(ok, b, "").s()?;
            }
            let yes = cg.ctx.append_basic_block(f, "typed");
            let no = cg.ctx.append_basic_block(f, "dyn");
            cg.b.build_conditional_branch(ok, yes, no).s()?;
            let args: Vec<BasicMetadataValueEnum> = (0..n).map(|i| f.get_nth_param(i as u32).unwrap().into()).collect();
            for (bb, t) in [(yes, typed_thunk), (no, dyn_thunk)] {
                cg.b.position_at_end(bb);
                let r = cg.b.build_call(t, &args, "").s()?.try_as_basic_value().basic().unwrap();
                cg.b.build_return(Some(&r)).s()?;
            }
            Ok(())
        })?;
        Ok(f)
    }

    /// `extern "C" fn name(…)`: a C-ABI function named `name` that starts the
    /// runtime if needed, converts its C arguments to Mimic values, runs the
    /// Mimic function and converts the result back.
    fn export_c(&mut self, fid: usize) -> R<()> {
        let FnKind::User(d) = &self.p.fns[fid].kind else { return Ok(()) };
        let d = d.clone();
        let n = d.params.len();
        let k: Key = (fid, vec![Ty::Dyn; n]);
        let kind = self.kind_for(fid);
        let inner = self.thunk(&k, kind)?;
        let ctys: Vec<Type> = d.params.iter().map(|p| p.ty.clone().unwrap()).collect();
        let ps: Vec<BasicMetadataTypeEnum> = ctys.iter().map(|t| self.ctype(t).into()).collect();
        let fty = match &d.ret {
            Some(t) => self.ctype(t).fn_type(&ps, false),
            None => self.ctx.void_type().fn_type(&ps, false),
        };
        if self.module.get_function(&d.name).is_some() {
            return Err(format!("`{}` is exported to C, but a C function of that name is also declared", d.name));
        }
        let f = self.module.add_function(&d.name, fty, Some(Linkage::External));
        self.in_new_fn(f, |cg| {
            cg.call_rt("mim_rt_ensure", &[])?;
            let mut boxes = Vec::new();
            for (i, t) in ctys.iter().enumerate() {
                let v = cg.c_to_mimic(f.get_nth_param(i as u32).unwrap(), t)?;
                boxes.push(cg.boxed(v)?);
            }
            let args: Vec<BasicMetadataValueEnum> = boxes.iter().map(|b| cg.basic(*b).into()).collect();
            let r = cg.b.build_call(inner, &args, "").s()?.try_as_basic_value().basic().unwrap();
            let rv = V::P(r.into_pointer_value(), true);
            let out = match &d.ret {
                // a string for C: a copy the caller frees, not memory Mimic owns
                Some(Type::Named(n, _)) if n == "cstr" => Some(cg.call_rt("mim_cstr_dup", &[r.into()])?.unwrap()),
                Some(t) => Some(match cg.mimic_to_c(rv, t)? {
                    BasicMetadataValueEnum::IntValue(i) => i.into(),
                    BasicMetadataValueEnum::FloatValue(x) => x.into(),
                    BasicMetadataValueEnum::PointerValue(x) => x.into(),
                    _ => return Err("unsupported C return type".into()),
                }),
                None => None,
            };
            for b in boxes {
                cg.drop_tmp(b)?;
            }
            cg.drop_tmp(rv)?;
            match out {
                Some(v) => cg.b.build_return(Some(&v)).s()?,
                None => cg.b.build_return(None).s()?,
            };
            Ok(())
        })
    }

    /// A library export: the boxed ABI under the function's public symbol.
    fn export(&mut self, fid: usize) -> R<()> {
        let n = self.p.fns[fid].arity();
        let k: Key = (fid, vec![Ty::Dyn; n]);
        let kind = self.kind_for(fid);
        let inner = self.thunk(&k, kind)?;
        let ps: Vec<BasicMetadataTypeEnum> = (0..n).map(|_| self.ptr.into()).collect();
        let f = self.module.add_function(&self.p.fns[fid].symbol, self.ptr.fn_type(&ps, false), Some(Linkage::External));
        self.in_new_fn(f, |cg| {
            let args: Vec<BasicMetadataValueEnum> = (0..n).map(|i| f.get_nth_param(i as u32).unwrap().into()).collect();
            let r = cg.b.build_call(inner, &args, "").s()?.try_as_basic_value().basic().unwrap();
            cg.b.build_return(Some(&r)).s()?;
            Ok(())
        })
    }

    // ---- interaction-net book --------------------------------------------

    fn emit_book(&mut self) -> R<()> {
        let Some(ensure) = self.net_ensure else { return Ok(()) };
        let enc = mimic_net::encode(&self.plan.book).map_err(|e| format!("internal compiler error: {e}"))?;
        let i64t = self.ctx.i64_type();
        let global = |m: &Module<'ctx>, name: &str, data: &[u64]| {
            let vals: Vec<_> = data.iter().map(|w| i64t.const_int(*w, false)).collect();
            let arr = i64t.const_array(&vals);
            let g = m.add_global(arr.get_type(), None, name);
            g.set_linkage(Linkage::Internal);
            g.set_constant(true);
            g.set_initializer(&arr);
            self.gaddr(g)
        };
        let words = global(&self.module, "mimic_net_words", &enc.words);
        let offsets = global(&self.module, "mimic_net_offsets", &enc.offsets);
        // def index → native function (boxed ABI) and arity, for native fallback
        let ndefs = enc.offsets.len();
        let mut fptrs = vec![self.ptr.const_null(); ndefs];
        let mut arity = vec![0u64; ndefs];
        let mut defs: Vec<(usize, u32)> = self.plan.def_index.iter().map(|(f, d)| (*f, *d)).collect();
        defs.sort();
        for (fid, def) in defs {
            let n = self.p.fns[fid].arity();
            if n <= 6 {
                let f = self.dispatch_thunk(fid)?;
                fptrs[def as usize] = self.fn_addr(f);
                arity[def as usize] = n as u64;
            }
        }
        let ftab = self.ptr.const_array(&fptrs);
        let gf = self.module.add_global(ftab.get_type(), None, "mimic_net_natives");
        gf.set_linkage(Linkage::Internal);
        gf.set_constant(true);
        gf.set_initializer(&ftab);
        let i8t = self.ctx.i8_type();
        let atab = i8t.const_array(&arity.iter().map(|a| i8t.const_int(*a, false)).collect::<Vec<_>>());
        let ga = self.module.add_global(atab.get_type(), None, "mimic_net_arity");
        ga.set_linkage(Linkage::Internal);
        ga.set_constant(true);
        ga.set_initializer(&atab);
        let msgs_src = self.plan.check_msgs.clone();
        self.in_new_fn(ensure, |cg| {
            let msgs: Vec<PointerValue> = msgs_src.iter().map(|m| cg.cstring(m)).collect::<R<_>>()?;
            let mtab = cg.ptr.const_array(&if msgs.is_empty() { vec![cg.ptr.const_null()] } else { msgs });
            let gm = cg.module.add_global(mtab.get_type(), None, "mimic_net_check_msgs");
            gm.set_linkage(Linkage::Internal);
            gm.set_constant(true);
            gm.set_initializer(&mtab);
            let n = cg.i32c(ndefs as i32);
            cg.call_rt(
                "mim_net_book_once",
                &[words.into(), offsets.into(), n, cg.gaddr(gf).into(), cg.gaddr(ga).into(), cg.gaddr(gm).into()],
            )?;
            cg.b.build_return(None).s()?;
            Ok(())
        })
    }

    fn emit_main(&mut self, entry: FunctionValue<'ctx>, leak_check: bool) -> R<()> {
        let i32t = self.ctx.i32_type();
        let ty = i32t.fn_type(&[i32t.into(), self.ptr.into()], false);
        // WebAssembly's C ABI names a `main(argc, argv)` `__main_argc_argv`
        // (Clang renames it; the WASI start code calls that name)
        let wasm = self.module.get_triple().as_str().to_string_lossy().starts_with("wasm");
        let main = self.module.add_function(if wasm { "__main_argc_argv" } else { "main" }, ty, None);
        let bb = self.ctx.append_basic_block(main, "entry");
        self.b.position_at_end(bb);
        let argc = main.get_nth_param(0).unwrap();
        let argv = main.get_nth_param(1).unwrap();
        self.call_rt("mim_rt_init", &[argc.into(), argv.into()])?;
        let r = self.b.build_call(entry, &[], "").s()?.try_as_basic_value().basic().unwrap();
        if r.is_pointer_value() {
            self.call_rt("mim_release", &[r.into()])?;
        }
        let code = self.call_rt("mim_rt_shutdown", &[self.i32c(leak_check as i32)])?.unwrap();
        self.b.build_return(Some(&code)).s()?;
        Ok(())
    }

    // ---- FFI ------------------------------------------------------------

    fn extern_fn(&self, decl: &ExternFn) -> FunctionValue<'ctx> {
        if decl.link_name.is_none()
            && let Some(f) = self.module.get_function(&decl.name)
        {
            return f;
        }
        let params: Vec<BasicMetadataTypeEnum> = decl.params.iter().map(|(_, t)| self.ctype(t).into()).collect();
        let ty = match &decl.ret {
            Some(t) => self.ctype(t).fn_type(&params, decl.variadic),
            None => self.ctx.void_type().fn_type(&params, decl.variadic),
        };
        let symbol = decl.link_name.as_deref().unwrap_or(&decl.name);
        if let Some(f) = self.module.get_function(symbol) {
            return f;
        }
        self.module.add_function(symbol, ty, Some(Linkage::External))
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
                "cstr" | "cptr" | "fnref" => self.ptr.into(),
                "isize" | "usize" => self.size.into(),
                _ => self.ctx.i64_type().into(),
            },
            Type::Ptr(..) | Type::Array(..) | Type::Fn(..) | Type::App(..) => self.ptr.into(),
        }
    }

    /// Mimic value → C argument (unboxed values go straight through).
    fn mimic_to_c(&self, v: V<'ctx>, t: &Type) -> R<BasicMetadataValueEnum<'ctx>> {
        Ok(match t {
            Type::Named(n, _) if n == "f64" || n == "f32" => {
                let f = self.to_f64(v)?;
                if n == "f32" { self.b.build_float_trunc(f, self.ctx.f32_type(), "").s()?.into() } else { f.into() }
            }
            Type::Named(n, _) if n == "bool" => self.truth(v)?.into(),
            Type::Named(n, _) if n == "cstr" => {
                let V::P(p, _) = v else { return Err("FFI: cstr expects a string".into()) };
                self.call_rt("mim_cstr", &[p.into()])?.unwrap().into()
            }
            // a Mimic function for a C callback: the generated wrapper gets the value itself
            Type::Named(n, _) if n == "fnref" => {
                let b = self.boxed(v)?;
                self.basic(b).into()
            }
            // an opaque C pointer travels through Mimic as its address (an integer)
            Type::Named(n, _) if n == "cptr" => {
                let i = self.to_i64(v)?;
                self.b.build_int_to_ptr(i, self.ptr, "").s()?.into()
            }
            Type::Ptr(..) => {
                let V::P(p, _) = v else { return Err("FFI: ptr[T] expects an array".into()) };
                self.call_rt("mim_dataptr", &[p.into()])?.unwrap().into()
            }
            _ => {
                let i = self.to_i64(v)?;
                let it = self.ctype(t).into_int_type();
                if it.get_bit_width() < 64 { self.b.build_int_truncate(i, it, "").s()?.into() } else { i.into() }
            }
        })
    }

    /// C return value → Mimic value.
    fn c_to_mimic(&self, r: BasicValueEnum<'ctx>, t: &Type) -> R<V<'ctx>> {
        Ok(match t {
            Type::Named(n, _) if n == "f64" => V::F(r.into_float_value()),
            Type::Named(n, _) if n == "f32" => V::F(self.b.build_float_ext(r.into_float_value(), self.ctx.f64_type(), "").s()?),
            Type::Named(n, _) if n == "bool" => V::B(r.into_int_value()),
            Type::Named(n, _) if n == "cstr" => self.call_rt_ptr("mim_from_cstr", &[r.into()])?,
            Type::Named(n, _) if n == "cptr" => V::I(self.b.build_ptr_to_int(r.into_pointer_value(), self.ctx.i64_type(), "").s()?),
            Type::Named(n, _) => {
                let i = r.into_int_value();
                V::I(if i.get_type().get_bit_width() < 64 {
                    if n.starts_with('u') {
                        self.b.build_int_z_extend(i, self.ctx.i64_type(), "").s()?
                    } else {
                        self.b.build_int_s_extend(i, self.ctx.i64_type(), "").s()?
                    }
                } else {
                    i
                })
            }
            _ => return Err("unsupported FFI return type".into()),
        })
    }

    // ---- statements & blocks --------------------------------------------

    fn ty_of(&self, e: &Expr) -> Ty {
        self.cur_spec.as_ref().and_then(|s| s.exprs.get(&id(e)).copied()).unwrap_or(Ty::Dyn)
    }

    fn lookup(&self, name: &str) -> Option<&Local<'ctx>> {
        self.scopes.iter().rev().flat_map(|s| s.iter().rev()).find(|l| l.name == name)
    }

    fn dead_block(&self) {
        let bb = self.ctx.append_basic_block(self.cur_fn.unwrap(), "dead");
        self.b.position_at_end(bb);
    }

    fn release_scope(&self, scope: &[Local<'ctx>]) -> R<()> {
        for l in scope.iter().rev() {
            if l.owned {
                let cur = self.b.build_load(self.ptr, l.slot, "").s()?;
                self.call_rt("mim_release", &[cur.into()])?;
            }
        }
        Ok(())
    }

    fn block(&mut self, b: &Block) -> R<V<'ctx>> {
        self.scopes.push(Vec::new());
        let lazy_mark = self.lazies.len();
        for (i, s) in b.stmts.iter().enumerate() {
            // an array `let` used only inside other array expressions is fused
            // into those uses rather than materialised
            if let Stmt::Let { name, mutable: false, ty: None, value, .. } = s
                && self.lazy_candidate(name, value, &b.stmts[i + 1..], b.tail.as_deref())
            {
                self.lazies.push((name.clone(), value as *const Expr));
                continue;
            }
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
        self.lazies.truncate(lazy_mark);
        if !self.terminated() {
            self.release_scope(&scope)?;
        }
        Ok(v)
    }

    fn stmt(&mut self, s: &Stmt) -> R<()> {
        if self.terminated() {
            self.dead_block();
        }
        match s {
            Stmt::Let { name, mutable, value, .. } => {
                let v = self.expr(value)?;
                let t = self.cur_spec.as_ref().and_then(|sp| sp.lets.get(&id(s)).copied()).unwrap_or(Ty::Dyn);
                let v = self.as_ty(v, t, &format!("`let {name}`"))?;
                let v = self.own(v)?;
                let slot = self.entry_alloca(t, name)?;
                self.b.build_store(slot, self.basic(v)).s()?;
                let owned = matches!(v, V::P(..));
                self.scopes.last_mut().unwrap().push(Local { name: name.clone(), slot, ty: t, owned, mutable: *mutable });
            }
            Stmt::Assign { name, index, value, .. } => {
                let (slot, t) = {
                    let l = self.lookup(name).ok_or_else(|| format!("unknown variable {name}"))?;
                    (l.slot, l.ty)
                };
                match index {
                    None => {
                        let v = self.expr(value)?;
                        let v = self.as_ty(v, t, &format!("`{name}`"))?;
                        let v = self.own(v)?;
                        if matches!(v, V::P(..)) {
                            let old = self.b.build_load(self.ptr, slot, "").s()?;
                            self.b.build_store(slot, self.basic(v)).s()?;
                            self.call_rt("mim_release", &[old.into()])?;
                        } else {
                            self.b.build_store(slot, self.basic(v)).s()?;
                        }
                    }
                    Some(i) => {
                        let iv = self.expr(i)?;
                        let iv = self.boxed(iv)?;
                        let v = self.expr(value)?;
                        let v = self.boxed(v)?;
                        self.call_rt("mim_set_index", &[slot.into(), self.basic(iv).into(), self.basic(v).into()])?;
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
            Stmt::LetTuple { names, value, .. } => {
                let v = self.expr(value)?;
                let b = self.boxed(v)?;
                for (i, n) in names.iter().enumerate() {
                    if n == "_" {
                        continue;
                    }
                    let t = self.cur_spec.as_ref().and_then(|sp| sp.tuple_lets.get(&(id(s), i)).copied()).unwrap_or(Ty::Dyn);
                    let fv = self.call_rt_ptr("mim_obj_get", &[self.basic(b).into(), self.ctx.i64_type().const_int(i as u64, false).into()])?;
                    let fv = self.as_ty(fv, t, &format!("`{n}`"))?;
                    let slot = self.entry_alloca(t, n)?;
                    self.b.build_store(slot, self.basic(fv)).s()?;
                    let owned = matches!(fv, V::P(..));
                    self.scopes.last_mut().unwrap().push(Local { name: n.clone(), slot, ty: t, owned, mutable: false });
                }
                self.drop_tmp(b)?;
            }
            Stmt::LetElse { name, value, els, .. } => {
                let v = self.expr(value)?;
                let b = self.boxed(v)?;
                let ok = self.call_rt("mim_unwrap_ok", &[self.basic(b).into()])?.unwrap().into_int_value();
                let c = self.b.build_int_compare(IntPredicate::NE, ok, self.ctx.i64_type().const_zero(), "").s()?;
                let f = self.cur_fn.unwrap();
                let some_bb = self.ctx.append_basic_block(f, "some");
                let none_bb = self.ctx.append_basic_block(f, "none");
                self.b.build_conditional_branch(c, some_bb, none_bb).s()?;
                // the `else` block runs (and returns) when there is no value
                self.b.position_at_end(none_bb);
                self.drop_tmp(b)?;
                let ev = self.block(els)?;
                if !self.terminated() {
                    self.drop_tmp(ev)?;
                    self.b.build_unreachable().s()?;
                }
                self.b.position_at_end(some_bb);
                let t = self.cur_spec.as_ref().and_then(|sp| sp.lets.get(&id(s)).copied()).unwrap_or(Ty::Dyn);
                let fv = self.call_rt_ptr("mim_obj_get", &[self.basic(b).into(), self.ctx.i64_type().const_zero().into()])?;
                let fv = self.as_ty(fv, t, &format!("`{name}`"))?;
                self.drop_tmp(b)?;
                let slot = self.entry_alloca(t, name)?;
                self.b.build_store(slot, self.basic(fv)).s()?;
                let owned = matches!(fv, V::P(..));
                self.scopes.last_mut().unwrap().push(Local { name: name.clone(), slot, ty: t, owned, mutable: false });
            }
        }
        Ok(())
    }

    // ---- expressions ----------------------------------------------------

    /// Compile a lambda without captures as a typed function `(params…) -> ret`.
    fn lambda_fn(&mut self, e: &Expr) -> R<(FunctionValue<'ctx>, Vec<Ty>, Ty)> {
        let ExprKind::Lambda { params, body } = &e.kind else { return Err("expected a lambda".into()) };
        let (pts, rt) = self
            .cur_spec
            .as_ref()
            .and_then(|s| s.lambdas.get(&id(e)).cloned())
            .unwrap_or_else(|| (vec![Ty::Dyn; params.len()], Ty::Dyn));
        let rt = if rt == Ty::Never { Ty::Dyn } else { rt };
        self.lambdas += 1;
        let f = self.module.add_function(&format!("_mimic_lambda_{}", self.lambdas), self.fn_type(&pts, rt), Some(Linkage::Internal));
        self.in_new_fn(f, |cg| {
            cg.cur_ret = rt;
            cg.ret_what = "lambda result".into();
            let mut locals = Vec::new();
            for (i, (name, pt)) in params.iter().zip(&pts).enumerate() {
                let slot = cg.entry_alloca(*pt, name)?;
                cg.b.build_store(slot, f.get_nth_param(i as u32).unwrap()).s()?;
                locals.push(Local { name: name.clone(), slot, ty: *pt, owned: false, mutable: false });
            }
            cg.scopes = vec![locals];
            let v = cg.expr(body)?;
            if !cg.terminated() {
                cg.ret(v)?;
            }
            Ok(())
        })?;
        Ok((f, pts, rt))
    }

    /// The outer variables a lambda's body uses: captured by value.
    fn captures(&self, params: &[String], body: &Expr) -> Vec<(String, Ty)> {
        let mut seen: Vec<(String, Ty)> = Vec::new();
        body.walk(&mut |x| {
            if let ExprKind::Path(path) = &x.kind
                && path.len() == 1
                && !params.contains(&path[0])
                && !seen.iter().any(|(n, _)| n == &path[0])
                && let Some(l) = self.lookup(&path[0])
            {
                seen.push((l.name.clone(), l.ty));
            }
        });
        seen
    }

    /// A descriptor `{ name, code, arity, pure }` for a function value.
    fn fn_desc(&mut self, name: &str, code: FunctionValue<'ctx>, arity: usize, pure: bool) -> PointerValue<'ctx> {
        let i32t = self.ctx.i32_type();
        let st = self.ctx.struct_type(&[self.ptr.into(), self.ptr.into(), i32t.into(), i32t.into()], false);
        let nm = self.global_cstr(name);
        let init = st.const_named_struct(&[
            nm.into(),
            self.fn_addr(code).into(),
            i32t.const_int(arity as u64, false).into(),
            i32t.const_int(pure as u64, false).into(),
        ]);
        let g = self.module.add_global(st, None, "fndesc");
        g.set_linkage(Linkage::Internal);
        g.set_constant(true);
        g.set_initializer(&init);
        self.gaddr(g)
    }

    /// `mim_closure(desc, n, env)`: a function value capturing `env` (by value).
    fn make_closure(&mut self, desc: PointerValue<'ctx>, env: &[V<'ctx>]) -> R<V<'ctx>> {
        let i64t = self.ctx.i64_type();
        let n = env.len();
        let arr = if n == 0 {
            self.ptr.const_null()
        } else {
            let arr_ty = self.ptr.array_type(n as u32);
            let arr = self.entry_alloca_ty(arr_ty.into(), "env")?;
            for (i, v) in env.iter().enumerate() {
                let slot = unsafe { self.b.build_in_bounds_gep(arr_ty, arr, &[i64t.const_zero(), i64t.const_int(i as u64, false)], "").s()? };
                self.b.build_store(slot, self.basic(*v)).s()?;
            }
            arr
        };
        self.call_rt_ptr("mim_closure", &[desc.into(), i64t.const_int(n as u64, false).into(), arr.into()])
    }

    /// A lambda as a value: a closure over the variables it uses.
    fn closure_value(&mut self, e: &Expr) -> R<V<'ctx>> {
        let ExprKind::Lambda { params, body } = &e.kind else { return Err("expected a lambda".into()) };
        let caps = self.captures(params, body);
        let (pts, _) = self
            .cur_spec
            .as_ref()
            .and_then(|s| s.lambdas.get(&id(e)).cloned())
            .unwrap_or_else(|| (vec![Ty::Dyn; params.len()], Ty::Dyn));
        self.lambdas += 1;
        let code = self.module.add_function(
            &format!("_mimic_closure_{}", self.lambdas),
            self.ptr.fn_type(&[self.ptr.into(), self.ptr.into()], false),
            Some(Linkage::Internal),
        );
        let caps2 = caps.clone();
        let params2 = params.clone();
        self.in_new_fn(code, |cg| {
            cg.cur_ret = Ty::Dyn;
            cg.ret_what = "lambda result".into();
            let env = code.get_nth_param(0).unwrap().into_pointer_value();
            let args = code.get_nth_param(1).unwrap().into_pointer_value();
            let mut locals = Vec::new();
            let mut bind = |cg: &mut Self, base: PointerValue<'ctx>, i: usize, name: &str, t: Ty| -> R<()> {
                let at = unsafe { cg.b.build_in_bounds_gep(cg.ptr, base, &[cg.ctx.i64_type().const_int(i as u64, false)], "").s()? };
                let p = cg.b.build_load(cg.ptr, at, name).s()?.into_pointer_value();
                let v = cg.as_ty(V::P(p, false), t, "lambda argument")?;
                let owned = matches!(v, V::P(_, true));
                let slot = cg.entry_alloca(t, name)?;
                cg.b.build_store(slot, cg.basic(v)).s()?;
                locals.push(Local { name: name.to_string(), slot, ty: t, owned, mutable: false });
                Ok(())
            };
            for (i, name) in params2.iter().enumerate() {
                bind(cg, args, i, name, pts.get(i).copied().unwrap_or(Ty::Dyn))?;
            }
            for (j, (name, t)) in caps2.iter().enumerate() {
                bind(cg, env, j, name, *t)?;
            }
            cg.scopes = vec![locals];
            let v = cg.expr(body)?;
            if !cg.terminated() {
                cg.ret(v)?;
            }
            Ok(())
        })?;
        let pure = self.purity.operand_is_pure(self.p, self.cur_mod, e);
        let desc = self.fn_desc("lambda", code, params.len(), pure);
        let mut env = Vec::new();
        for (name, _) in &caps {
            let l = self.lookup(name).ok_or("internal error: captured variable")?;
            let (slot, lt) = (l.slot, l.ty);
            let bv = self.b.build_load(self.repr(lt), slot, name).s()?;
            env.push(self.boxed(self.val_of(bv, lt, false))?);
        }
        let r = self.make_closure(desc, &env)?;
        for v in env {
            self.drop_tmp(v)?;
        }
        Ok(r)
    }

    /// A named function as a value.
    fn fn_value(&mut self, fid: usize) -> R<V<'ctx>> {
        let key = format!("fn:{fid}");
        let desc = match self.fn_descs.get(&key) {
            Some(d) => *d,
            None => {
                let n = self.p.fns[fid].arity();
                let k: Key = (fid, vec![Ty::Dyn; n]);
                let kind = self.kind_for(fid);
                let inner = self.thunk(&k, kind)?;
                let code = self.module.add_function(
                    &format!("{}$value", self.p.fns[fid].symbol),
                    self.ptr.fn_type(&[self.ptr.into(), self.ptr.into()], false),
                    Some(Linkage::Internal),
                );
                self.in_new_fn(code, |cg| {
                    let args = code.get_nth_param(1).unwrap().into_pointer_value();
                    let mut av: Vec<BasicMetadataValueEnum> = Vec::new();
                    for i in 0..n {
                        let at = unsafe { cg.b.build_in_bounds_gep(cg.ptr, args, &[cg.ctx.i64_type().const_int(i as u64, false)], "").s()? };
                        av.push(cg.b.build_load(cg.ptr, at, "").s()?.into());
                    }
                    let r = cg.b.build_call(inner, &av, "").s()?.try_as_basic_value().basic().unwrap();
                    cg.b.build_return(Some(&r)).s()?;
                    Ok(())
                })?;
                let name = self.p.fn_path(fid).join("::");
                let d = self.fn_desc(&name, code, n, self.purity.fns[fid]);
                self.fn_descs.insert(key, d);
                d
            }
        };
        self.make_closure(desc, &[])
    }

    /// An operator as a value: `(+)`.
    fn op_value(&mut self, op: mimic_syntax::ast::BinOp) -> R<V<'ctx>> {
        let key = format!("op:{}", op as i32);
        let desc = match self.fn_descs.get(&key) {
            Some(d) => *d,
            None => {
                let code = self.module.add_function(
                    &format!("_mimic_op_{}", op as i32),
                    self.ptr.fn_type(&[self.ptr.into(), self.ptr.into()], false),
                    Some(Linkage::Internal),
                );
                self.in_new_fn(code, |cg| {
                    let args = code.get_nth_param(1).unwrap().into_pointer_value();
                    let mut av: Vec<BasicMetadataValueEnum> = vec![cg.i32c(op as i32)];
                    for i in 0..2u64 {
                        let at = unsafe { cg.b.build_in_bounds_gep(cg.ptr, args, &[cg.ctx.i64_type().const_int(i, false)], "").s()? };
                        av.push(cg.b.build_load(cg.ptr, at, "").s()?.into());
                    }
                    let r = cg.call_rt("mim_binop", &av)?.unwrap();
                    cg.b.build_return(Some(&r)).s()?;
                    Ok(())
                })?;
                let d = self.fn_desc(&format!("({op:?})"), code, 2, true);
                self.fn_descs.insert(key, d);
                d
            }
        };
        self.make_closure(desc, &[])
    }

    /// Call a function value: `mim_call(f, n, args)`.
    fn call_value(&mut self, f: V<'ctx>, args: &[Expr], ty: Ty) -> R<V<'ctx>> {
        let i64t = self.ctx.i64_type();
        let f = self.boxed(f)?;
        let mut vals = Vec::new();
        for a in args {
            let v = self.expr(a)?;
            vals.push(self.boxed(v)?);
        }
        let n = vals.len();
        let arr = if n == 0 {
            self.ptr.const_null()
        } else {
            let arr_ty = self.ptr.array_type(n as u32);
            let arr = self.entry_alloca_ty(arr_ty.into(), "args")?;
            for (i, v) in vals.iter().enumerate() {
                let slot = unsafe { self.b.build_in_bounds_gep(arr_ty, arr, &[i64t.const_zero(), i64t.const_int(i as u64, false)], "").s()? };
                self.b.build_store(slot, self.basic(*v)).s()?;
            }
            arr
        };
        let r = self.call_rt_ptr("mim_call", &[self.basic(f).into(), self.i32c(n as i32), arr.into()])?;
        for v in vals {
            self.drop_tmp(v)?;
        }
        self.drop_tmp(f)?;
        self.as_ty(r, ty, "call result")
    }

    /// A boxed-ABI wrapper around a typed lambda (for `¨`).
    fn lambda_thunk(&mut self, e: &Expr) -> R<FunctionValue<'ctx>> {
        let (lf, pts, rt) = self.lambda_fn(e)?;
        let pt = pts.first().copied().unwrap_or(Ty::Dyn);
        self.lambdas += 1;
        let f = self.module.add_function(&format!("_mimic_lambda_{}$box", self.lambdas), self.ptr.fn_type(&[self.ptr.into()], false), Some(Linkage::Internal));
        self.in_new_fn(f, |cg| {
            let p = f.get_nth_param(0).unwrap().into_pointer_value();
            let a = cg.as_ty(V::P(p, false), pt, "lambda argument")?;
            let r = cg.b.build_call(lf, &[cg.basic(a).into()], "").s()?.try_as_basic_value().basic().unwrap();
            let b = cg.boxed(cg.val_of(r, rt, true))?;
            let bv = cg.basic(b);
            cg.b.build_return(Some(&bv)).s()?;
            Ok(())
        })?;
        Ok(f)
    }

    fn resolve_fn(&self, path: &[String]) -> R<usize> {
        match self.p.resolve(self.cur_mod, path)? {
            Resolved::Fn(f) => Ok(f),
            r => Err(format!("`{}` is not a function ({r:?})", path.join("::"))),
        }
    }

    // ---- records, variants, strings ---------------------------------------

    /// A NUL-terminated string constant that needs no builder position.
    fn global_cstr(&self, s: &str) -> PointerValue<'ctx> {
        let bytes = self.ctx.const_string(s.as_bytes(), true);
        let g = self.module.add_global(bytes.get_type(), None, "cstr");
        g.set_initializer(&bytes);
        g.set_constant(true);
        g.set_linkage(Linkage::Private);
        self.gaddr(g)
    }

    /// The runtime descriptor (`mim_desc`) of a record type or union variant.
    /// `t == usize::MAX` is the shared tuple descriptor (prints as `(a, b)`).
    fn desc(&mut self, t: TypeId, variant: Option<usize>) -> PointerValue<'ctx> {
        if let Some(d) = self.descs.get(&(t, variant)) {
            return *d;
        }
        if t == usize::MAX {
            let i32t = self.ctx.i32_type();
            let st = self.ctx.struct_type(&[self.ptr.into(), self.ptr.into(), i32t.into(), i32t.into(), self.ptr.into()], false);
            let (tn, vn) = (self.global_cstr("tuple"), self.global_cstr(""));
            let val = st.const_named_struct(&[tn.into(), vn.into(), i32t.const_zero().into(), i32t.const_zero().into(), self.ptr.const_null().into()]);
            let g = self.module.add_global(st, None, "tuple_desc");
            g.set_initializer(&val);
            g.set_constant(true);
            g.set_linkage(Linkage::Private);
            let p = self.gaddr(g);
            self.descs.insert((t, variant), p);
            return p;
        }
        let td = &self.p.types[t];
        let i32t = self.ctx.i32_type();
        let tname = self.global_cstr(&td.name);
        let (vname, tag, n, fields) = match variant {
            Some(v) => {
                let var = &td.variants()[v];
                (self.global_cstr(&var.name), v as u64, var.fields.len() as u64, self.ptr.const_null())
            }
            None => {
                let names: Vec<String> = match &td.decl.kind {
                    mimic_syntax::ast::TypeKind::Record(fs) => fs.iter().map(|(n, _, _)| n.clone()).collect(),
                    _ => vec![],
                };
                let ptrs: Vec<PointerValue> = names.iter().map(|n| self.global_cstr(n)).collect();
                let arr = self.ptr.const_array(&ptrs);
                let g = self.module.add_global(arr.get_type(), None, "fields");
                g.set_initializer(&arr);
                g.set_constant(true);
                g.set_linkage(Linkage::Private);
                (self.ptr.const_null(), 0, names.len() as u64, self.gaddr(g))
            }
        };
        let st = self.ctx.struct_type(&[self.ptr.into(), self.ptr.into(), i32t.into(), i32t.into(), self.ptr.into()], false);
        let val = st.const_named_struct(&[tname.into(), vname.into(), i32t.const_int(tag, false).into(), i32t.const_int(n, false).into(), fields.into()]);
        let g = self.module.add_global(st, None, "desc");
        g.set_initializer(&val);
        g.set_constant(true);
        g.set_linkage(Linkage::Private);
        let p = self.gaddr(g);
        self.descs.insert((t, variant), p);
        p
    }

    /// Build a record or variant from field values (in field order).
    fn construct(&mut self, t: TypeId, variant: Option<usize>, vals: Vec<V<'ctx>>) -> R<V<'ctx>> {
        // (tuples use t = usize::MAX)
        let i64t = self.ctx.i64_type();
        let n = vals.len() as u32;
        let arr_ty = self.ptr.array_type(n.max(1));
        let arr = self.entry_array(arr_ty)?;
        let mut boxed = Vec::new();
        for (i, v) in vals.into_iter().enumerate() {
            let b = self.boxed(v)?;
            let slot = unsafe { self.b.build_in_bounds_gep(arr_ty, arr, &[i64t.const_zero(), i64t.const_int(i as u64, false)], "").s()? };
            self.b.build_store(slot, self.basic(b)).s()?;
            boxed.push(b);
        }
        let d = self.desc(t, variant);
        let r = self.call_rt_ptr("mim_obj", &[d.into(), i64t.const_int(n as u64, false).into(), arr.into()])?;
        for b in boxed {
            self.drop_tmp(b)?;
        }
        Ok(r)
    }

    fn entry_array(&self, ty: inkwell::types::ArrayType<'ctx>) -> R<PointerValue<'ctx>> {
        let f = self.cur_fn.unwrap();
        let entry = f.get_first_basic_block().unwrap();
        let tmp = self.ctx.create_builder();
        match entry.get_first_instruction() {
            Some(i) => tmp.position_before(&i),
            None => tmp.position_at_end(entry),
        }
        let a = tmp.build_alloca(ty, "arr").s()?;
        self.to_generic(&tmp, a)
    }

    fn field_tys(&self, t: TypeId, v: usize) -> Vec<Ty> {
        let td = &self.p.types[t];
        td.variants()[v].fields.iter().map(|f| types::ann_in(self.p, td.module, f).unwrap_or(Ty::Dyn)).collect()
    }

    /// `kinds` and `texts` constant tables of a route pattern.
    fn route_tables(&mut self, parts: &[RoutePart]) -> (u64, PointerValue<'ctx>, PointerValue<'ctx>, usize) {
        let i64t = self.ctx.i64_type();
        let mut kinds = Vec::new();
        let mut texts = Vec::new();
        let mut ncap = 0;
        for p in parts {
            match p {
                RoutePart::Text(t) => {
                    kinds.push(i64t.const_int(0, false));
                    texts.push(self.global_cstr(t));
                }
                RoutePart::Capture { ty, .. } => {
                    let k = match ty.as_deref() {
                        Some("i64") => 2,
                        Some("f64") => 3,
                        _ => 1,
                    };
                    kinds.push(i64t.const_int(k, false));
                    texts.push(self.ptr.const_null());
                    ncap += 1;
                }
            }
        }
        let ka = i64t.const_array(&kinds);
        let kg = self.module.add_global(ka.get_type(), None, "route_kinds");
        kg.set_initializer(&ka);
        kg.set_constant(true);
        kg.set_linkage(Linkage::Private);
        let ta = self.ptr.const_array(&texts);
        let tg = self.module.add_global(ta.get_type(), None, "route_texts");
        tg.set_initializer(&ta);
        tg.set_constant(true);
        tg.set_linkage(Linkage::Private);
        (parts.len() as u64, self.gaddr(kg), self.gaddr(tg), ncap)
    }

    /// Emit the tests of a pattern against `val`; on failure jump to `fail`.
    /// The builder is left in the success path.
    fn pattern_test(&mut self, pat: &Pattern, val: V<'ctx>, fail: BasicBlock<'ctx>) -> R<()> {
        let f = self.cur_fn.unwrap();
        match pat {
            Pattern::Wild(_) | Pattern::Bind(..) => Ok(()),
            Pattern::Lit(e) => {
                let cond = if matches!(e.kind, ExprKind::Str(_)) {
                    let lv = self.expr(e)?;
                    let b = self.boxed(val)?;
                    let r = self.call_rt("mim_str_eq", &[self.basic(b).into(), self.basic(lv).into()])?.unwrap().into_int_value();
                    self.b.build_int_compare(IntPredicate::NE, r, self.ctx.i64_type().const_zero(), "").s()?
                } else {
                    self.pat_compare(BinOp::Eq, val, e)?
                };
                let ok = self.ctx.append_basic_block(f, "pat_ok");
                self.b.build_conditional_branch(cond, ok, fail).s()?;
                self.b.position_at_end(ok);
                Ok(())
            }
            Pattern::Range { lo, hi, .. } => {
                let a = self.pat_compare(BinOp::Ge, val, lo)?;
                let mid = self.ctx.append_basic_block(f, "range_lo_ok");
                self.b.build_conditional_branch(a, mid, fail).s()?;
                self.b.position_at_end(mid);
                let b = self.pat_compare(BinOp::Lt, val, hi)?;
                let ok = self.ctx.append_basic_block(f, "range_ok");
                self.b.build_conditional_branch(b, ok, fail).s()?;
                self.b.position_at_end(ok);
                Ok(())
            }
            Pattern::Or(alts, _) => {
                let ok = self.ctx.append_basic_block(f, "or_ok");
                for (i, a) in alts.iter().enumerate() {
                    let next = if i + 1 == alts.len() { fail } else { self.ctx.append_basic_block(f, "or_next") };
                    self.pattern_test(a, val, next)?;
                    self.b.build_unconditional_branch(ok).s()?;
                    if i + 1 < alts.len() {
                        self.b.position_at_end(next);
                    }
                }
                self.b.position_at_end(ok);
                Ok(())
            }
            Pattern::As(inner, _, _) => self.pattern_test(inner, val, fail),
            Pattern::Record { path, fields, .. } => {
                let Ok(Resolved::Type(t)) = self.p.resolve(self.cur_mod, path) else { return Err("not a record type".into()) };
                let b = self.boxed(val)?;
                let d = self.desc(t, None);
                let is = self.call_rt("mim_pat_is", &[self.basic(b).into(), d.into()])?.unwrap().into_int_value();
                let c = self.b.build_int_compare(IntPredicate::NE, is, self.ctx.i64_type().const_zero(), "").s()?;
                let ok = self.ctx.append_basic_block(f, "record_ok");
                self.b.build_conditional_branch(c, ok, fail).s()?;
                self.b.position_at_end(ok);
                for (name, a) in fields {
                    if matches!(a, Pattern::Wild(_) | Pattern::Bind(..)) {
                        continue;
                    }
                    let (i, ft) = self.record_field(t, name);
                    let fv = self.call_rt_ptr("mim_obj_get", &[self.basic(b).into(), self.ctx.i64_type().const_int(i as u64, false).into()])?;
                    let fv = self.as_ty(fv, ft, "field")?;
                    self.sub_test(a, fv, fail)?;
                }
                Ok(())
            }
            Pattern::Array { elems, rest, .. } => {
                let i64t = self.ctx.i64_type();
                let b = self.boxed(val)?;
                let len = self.call_rt("mim_pat_len", &[self.basic(b).into()])?.unwrap().into_int_value();
                let n = i64t.const_int(elems.len() as u64, false);
                let c = if rest.is_some() {
                    self.b.build_int_compare(IntPredicate::SGE, len, n, "").s()?
                } else {
                    self.b.build_int_compare(IntPredicate::EQ, len, n, "").s()?
                };
                let ok = self.ctx.append_basic_block(f, "array_len_ok");
                self.b.build_conditional_branch(c, ok, fail).s()?;
                self.b.position_at_end(ok);
                for (i, a) in elems.iter().enumerate() {
                    if matches!(a, Pattern::Wild(_) | Pattern::Bind(..)) {
                        continue;
                    }
                    let ev = self.call_rt_ptr("mim_pat_at", &[self.basic(b).into(), i64t.const_int(i as u64, false).into()])?;
                    self.sub_test(a, ev, fail)?;
                }
                Ok(())
            }
            Pattern::Route(parts, _) => {
                let (n, kinds, texts, _) = self.route_tables(parts);
                let b = self.boxed(val)?;
                let i64t = self.ctx.i64_type();
                let r = self
                    .call_rt("mim_route", &[self.basic(b).into(), i64t.const_int(n, false).into(), kinds.into(), texts.into(), self.ptr.const_null().into()])?
                    .unwrap()
                    .into_int_value();
                let c = self.b.build_int_compare(IntPredicate::NE, r, i64t.const_zero(), "").s()?;
                let ok = self.ctx.append_basic_block(f, "route_ok");
                self.b.build_conditional_branch(c, ok, fail).s()?;
                self.b.position_at_end(ok);
                Ok(())
            }
            Pattern::Tuple(ps, _) => {
                let b = self.boxed(val)?;
                for (i, a) in ps.iter().enumerate() {
                    if matches!(a, Pattern::Wild(_) | Pattern::Bind(..)) {
                        continue;
                    }
                    let fv = self.call_rt_ptr("mim_obj_get", &[self.basic(b).into(), self.ctx.i64_type().const_int(i as u64, false).into()])?;
                    let fail_k = self.ctx.append_basic_block(f, "pat_fail");
                    self.pattern_test(a, fv, fail_k)?;
                    self.drop_tmp(fv)?;
                    let cont = self.b.get_insert_block().unwrap();
                    self.b.position_at_end(fail_k);
                    self.drop_tmp(fv)?;
                    self.b.build_unconditional_branch(fail).s()?;
                    self.b.position_at_end(cont);
                }
                Ok(())
            }
            Pattern::Variant { path, args, .. } => {
                let Ok(Resolved::Variant(t, v)) = self.p.resolve(self.cur_mod, path) else { return Err("not a variant".into()) };
                let b = self.boxed(val)?;
                let tag = self.call_rt("mim_obj_tag", &[self.basic(b).into()])?.unwrap().into_int_value();
                let eq = self.b.build_int_compare(IntPredicate::EQ, tag, self.ctx.i64_type().const_int(v as u64, false), "").s()?;
                let ok = self.ctx.append_basic_block(f, "variant_ok");
                self.b.build_conditional_branch(eq, ok, fail).s()?;
                self.b.position_at_end(ok);
                let ftys = self.field_tys(t, v);
                for (i, a) in args.iter().enumerate() {
                    if matches!(a, Pattern::Wild(_) | Pattern::Bind(..)) {
                        continue;
                    }
                    let fv = self.call_rt_ptr("mim_obj_get", &[self.basic(b).into(), self.ctx.i64_type().const_int(i as u64, false).into()])?;
                    let fv = self.as_ty(fv, ftys[i], "field")?;
                    // release the field on both outcomes
                    let fail_k = self.ctx.append_basic_block(f, "pat_fail");
                    self.pattern_test(a, fv, fail_k)?;
                    self.drop_tmp(fv)?;
                    let cont = self.b.get_insert_block().unwrap();
                    self.b.position_at_end(fail_k);
                    self.drop_tmp(fv)?;
                    self.b.build_unconditional_branch(fail).s()?;
                    self.b.position_at_end(cont);
                }
                Ok(())
            }
        }
    }

    /// Test a sub-value (owned temporary `v`) against `a`, releasing it on both outcomes.
    fn sub_test(&mut self, a: &Pattern, v: V<'ctx>, fail: BasicBlock<'ctx>) -> R<()> {
        let f = self.cur_fn.unwrap();
        let fail_k = self.ctx.append_basic_block(f, "pat_fail");
        self.pattern_test(a, v, fail_k)?;
        self.drop_tmp(v)?;
        let cont = self.b.get_insert_block().unwrap();
        self.b.position_at_end(fail_k);
        self.drop_tmp(v)?;
        self.b.build_unconditional_branch(fail).s()?;
        self.b.position_at_end(cont);
        Ok(())
    }

    /// `val <op> e` for a literal pattern expression, as an i1.
    fn pat_compare(&mut self, op: BinOp, val: V<'ctx>, e: &Expr) -> R<IntValue<'ctx>> {
        let lv = self.expr(e)?;
        if Self::is_unboxed(&val) {
            let V::B(c) = self.scalar_binary(op, val, lv)? else { unreachable!() };
            return Ok(c);
        }
        let lb = self.boxed(lv)?;
        let r = self.call_rt_ptr("mim_binop", &[self.i32c(op as i32), self.basic(val).into(), self.basic(lb).into()])?;
        self.drop_tmp(lb)?;
        self.truth(r)
    }

    /// Index and static type of record field `name`.
    fn record_field(&self, t: TypeId, name: &str) -> (usize, Ty) {
        let td = &self.p.types[t];
        let i = td.field_index(name).unwrap_or(0);
        let ty = match &td.decl.kind {
            mimic_syntax::ast::TypeKind::Record(fs) => types::ann_in(self.p, td.module, &fs[i].1).unwrap_or(Ty::Dyn),
            _ => Ty::Dyn,
        };
        (i, ty)
    }

    /// A new local `n` holding (a retained copy of) `val`; its type was recorded under `key`.
    fn bind_local(&mut self, n: &str, key: usize, val: V<'ctx>) -> R<()> {
        let t = self.cur_spec.as_ref().and_then(|s| s.binds.get(&key).copied()).unwrap_or(Ty::Dyn);
        let v = match val {
            V::P(p, _) => {
                self.call_rt("mim_retain", &[p.into()])?;
                V::P(p, true)
            }
            other => other,
        };
        let v = self.as_ty(v, t, &format!("`{n}`"))?;
        let slot = self.entry_alloca(t, n)?;
        self.b.build_store(slot, self.basic(v)).s()?;
        let owned = matches!(v, V::P(..));
        self.scopes.last_mut().unwrap().push(Local { name: n.to_string(), slot, ty: t, owned, mutable: false });
        Ok(())
    }

    /// Bind the variables of a pattern that matched `val`.
    fn pattern_bind(&mut self, pat: &Pattern, val: V<'ctx>) -> R<()> {
        match pat {
            Pattern::Wild(_) | Pattern::Lit(_) | Pattern::Range { .. } => Ok(()),
            // alternatives that bind names were split into separate arms
            Pattern::Or(..) => Ok(()),
            Pattern::Bind(n, _) => self.bind_local(n, id(pat), val),
            Pattern::As(inner, n, _) => {
                self.pattern_bind(inner, val)?;
                self.bind_local(n, id(pat), val)
            }
            Pattern::Record { path, fields, .. } => {
                let Ok(Resolved::Type(t)) = self.p.resolve(self.cur_mod, path) else { return Ok(()) };
                let b = self.boxed(val)?;
                for (name, a) in fields {
                    if matches!(a, Pattern::Wild(_) | Pattern::Lit(_) | Pattern::Range { .. }) {
                        continue;
                    }
                    let (i, ft) = self.record_field(t, name);
                    let fv = self.call_rt_ptr("mim_obj_get", &[self.basic(b).into(), self.ctx.i64_type().const_int(i as u64, false).into()])?;
                    let fv = self.as_ty(fv, ft, "field")?;
                    self.pattern_bind(a, fv)?;
                    self.drop_tmp(fv)?;
                }
                Ok(())
            }
            Pattern::Array { elems, rest, .. } => {
                let i64t = self.ctx.i64_type();
                let b = self.boxed(val)?;
                for (i, a) in elems.iter().enumerate() {
                    if matches!(a, Pattern::Wild(_) | Pattern::Lit(_) | Pattern::Range { .. }) {
                        continue;
                    }
                    let ev = self.call_rt_ptr("mim_pat_at", &[self.basic(b).into(), i64t.const_int(i as u64, false).into()])?;
                    self.pattern_bind(a, ev)?;
                    self.drop_tmp(ev)?;
                }
                if let Some(Some(n)) = rest {
                    let rv = self.call_rt_ptr("mim_pat_rest", &[self.basic(b).into(), i64t.const_int(elems.len() as u64, false).into()])?;
                    self.bind_local(n, id(pat) + 1, rv)?;
                    self.drop_tmp(rv)?;
                }
                Ok(())
            }
            Pattern::Tuple(ps, _) => {
                let b = self.boxed(val)?;
                for (i, a) in ps.iter().enumerate() {
                    if matches!(a, Pattern::Wild(_) | Pattern::Lit(_)) {
                        continue;
                    }
                    let fv = self.call_rt_ptr("mim_obj_get", &[self.basic(b).into(), self.ctx.i64_type().const_int(i as u64, false).into()])?;
                    self.pattern_bind(a, fv)?;
                    self.drop_tmp(fv)?;
                }
                Ok(())
            }
            Pattern::Route(parts, _) => {
                let (n, kinds, texts, ncap) = self.route_tables(parts);
                let i64t = self.ctx.i64_type();
                let arr_ty = self.ptr.array_type(ncap.max(1) as u32);
                let out = self.entry_array(arr_ty)?;
                let b = self.boxed(val)?;
                self.call_rt("mim_route", &[self.basic(b).into(), i64t.const_int(n, false).into(), kinds.into(), texts.into(), out.into()])?;
                let mut k = 0;
                for part in parts {
                    if let RoutePart::Capture { name, .. } = part {
                        let t = self.cur_spec.as_ref().and_then(|s| s.route_binds.get(&(id(pat), k)).copied()).unwrap_or(Ty::Dyn);
                        let slot = unsafe { self.b.build_in_bounds_gep(arr_ty, out, &[i64t.const_zero(), i64t.const_int(k as u64, false)], "").s()? };
                        let cv = self.b.build_load(self.ptr, slot, "").s()?.into_pointer_value();
                        let v = self.as_ty(V::P(cv, true), t, &format!("`{name}`"))?;
                        let lslot = self.entry_alloca(t, name)?;
                        self.b.build_store(lslot, self.basic(v)).s()?;
                        let owned = matches!(v, V::P(..));
                        self.scopes.last_mut().unwrap().push(Local { name: name.clone(), slot: lslot, ty: t, owned, mutable: false });
                        k += 1;
                    }
                }
                Ok(())
            }
            Pattern::Variant { path, args, .. } => {
                let Ok(Resolved::Variant(t, v)) = self.p.resolve(self.cur_mod, path) else { return Ok(()) };
                let ftys = self.field_tys(t, v);
                let b = self.boxed(val)?;
                for (i, a) in args.iter().enumerate() {
                    if matches!(a, Pattern::Wild(_) | Pattern::Lit(_)) {
                        continue;
                    }
                    let fv = self.call_rt_ptr("mim_obj_get", &[self.basic(b).into(), self.ctx.i64_type().const_int(i as u64, false).into()])?;
                    let fv = self.as_ty(fv, ftys[i], "field")?;
                    self.pattern_bind(a, fv)?;
                    self.drop_tmp(fv)?;
                }
                Ok(())
            }
        }
    }

    fn scalar_binary(&self, op: BinOp, a: V<'ctx>, b: V<'ctx>) -> R<V<'ctx>> {
        use BinOp::*;
        let bld = &self.b;
        if matches!(op, And | Or) {
            let x = self.truth(a)?;
            let y = self.truth(b)?;
            return Ok(V::B(if op == And { bld.build_and(x, y, "").s()? } else { bld.build_or(x, y, "").s()? }));
        }
        if op.is_bitwise() {
            return self.bitwise(op, a, b);
        }
        let float = matches!(a, V::F(_)) || matches!(b, V::F(_)) || op == Div;
        if float {
            let x = self.to_f64(a)?;
            let y = self.to_f64(b)?;
            let cmp = |p: FloatPredicate| -> R<V<'ctx>> { Ok(V::B(bld.build_float_compare(p, x, y, "").s()?)) };
            return match op {
                Add => Ok(V::F(bld.build_float_add(x, y, "").s()?)),
                Sub => Ok(V::F(bld.build_float_sub(x, y, "").s()?)),
                Mul => Ok(V::F(bld.build_float_mul(x, y, "").s()?)),
                Div => Ok(V::F(bld.build_float_div(x, y, "").s()?)),
                Mod => Ok(V::F(bld.build_float_rem(x, y, "").s()?)),
                IntDiv => {
                    let q = bld.build_float_div(x, y, "").s()?;
                    let floor = self.intrinsic("llvm.floor")?;
                    Ok(V::F(bld.build_call(floor, &[q.into()], "").s()?.try_as_basic_value().basic().unwrap().into_float_value()))
                }
                Max | Min => {
                    let c = bld.build_float_compare(if op == Max { FloatPredicate::OGT } else { FloatPredicate::OLT }, x, y, "").s()?;
                    Ok(V::F(bld.build_select(c, x, y, "").s()?.into_float_value()))
                }
                Eq => cmp(FloatPredicate::OEQ),
                Ne => cmp(FloatPredicate::UNE),
                Lt => cmp(FloatPredicate::OLT),
                Le => cmp(FloatPredicate::OLE),
                Gt => cmp(FloatPredicate::OGT),
                Ge => cmp(FloatPredicate::OGE),
                And | Or | BitAnd | BitOr | BitXor | Shl | Shr | UShr => unreachable!(),
            };
        }
        let x = self.to_i64(a)?;
        let y = self.to_i64(b)?;
        let zero = self.ctx.i64_type().const_zero();
        let cmp = |p: IntPredicate| -> R<V<'ctx>> { Ok(V::B(bld.build_int_compare(p, x, y, "").s()?)) };
        // Floored division / modulo by a constant power of two are exactly an
        // arithmetic shift and a mask (two's complement), for negative x too.
        if matches!(op, IntDiv | Mod)
            && let Some(c) = y.get_sign_extended_constant()
            && c > 0
            && (c as u64).is_power_of_two()
        {
            let k = (c as u64).trailing_zeros() as u64;
            let i64t = self.ctx.i64_type();
            return Ok(V::I(if op == IntDiv {
                bld.build_right_shift(x, i64t.const_int(k, false), true, "").s()?
            } else {
                bld.build_and(x, i64t.const_int((c - 1) as u64, false), "").s()?
            }));
        }
        match op {
            Add => Ok(V::I(bld.build_int_add(x, y, "").s()?)),
            Sub => Ok(V::I(bld.build_int_sub(x, y, "").s()?)),
            Mul => Ok(V::I(bld.build_int_mul(x, y, "").s()?)),
            IntDiv => {
                let z = bld.build_int_compare(IntPredicate::EQ, y, zero, "").s()?;
                self.panic_if(z, "integer division by zero")?;
                let q = bld.build_int_signed_div(x, y, "").s()?;
                let r = bld.build_int_signed_rem(x, y, "").s()?;
                // floor: step down when the remainder is non-zero and the signs differ
                let nz = bld.build_int_compare(IntPredicate::NE, r, zero, "").s()?;
                let sx = bld.build_xor(r, y, "").s()?;
                let neg = bld.build_int_compare(IntPredicate::SLT, sx, zero, "").s()?;
                let adj = bld.build_and(nz, neg, "").s()?;
                let adj = bld.build_int_z_extend(adj, self.ctx.i64_type(), "").s()?;
                Ok(V::I(bld.build_int_sub(q, adj, "").s()?))
            }
            Mod => {
                let z = bld.build_int_compare(IntPredicate::EQ, y, zero, "").s()?;
                self.panic_if(z, "integer modulo by zero")?;
                // floored modulo with ONE division: r = x rem y, then add y
                // when r is non-zero and its sign differs from y's
                let r = bld.build_int_signed_rem(x, y, "").s()?;
                let nz = bld.build_int_compare(IntPredicate::NE, r, zero, "").s()?;
                let sx = bld.build_xor(r, y, "").s()?;
                let neg = bld.build_int_compare(IntPredicate::SLT, sx, zero, "").s()?;
                let adj = bld.build_and(nz, neg, "").s()?;
                let ry = bld.build_int_add(r, y, "").s()?;
                Ok(V::I(bld.build_select(adj, ry, r, "").s()?.into_int_value()))
            }
            Max | Min => {
                let c = bld.build_int_compare(if op == Max { IntPredicate::SGT } else { IntPredicate::SLT }, x, y, "").s()?;
                Ok(V::I(bld.build_select(c, x, y, "").s()?.into_int_value()))
            }
            Eq => cmp(IntPredicate::EQ),
            Ne => cmp(IntPredicate::NE),
            Lt => cmp(IntPredicate::SLT),
            Le => cmp(IntPredicate::SLE),
            Gt => cmp(IntPredicate::SGT),
            Ge => cmp(IntPredicate::SGE),
            Div | And | Or | BitAnd | BitOr | BitXor | Shl | Shr | UShr => unreachable!(),
        }
    }

    /// `& | ^ << >> >>>`. Shift amounts use their low 6 bits (`x << 64` is
    /// `x << 0`), which is what the hardware does, so a shift is one instruction.
    fn bitwise(&self, op: BinOp, a: V<'ctx>, b: V<'ctx>) -> R<V<'ctx>> {
        use BinOp::*;
        let bld = &self.b;
        if matches!(a, V::F(_)) || matches!(b, V::F(_)) {
            return Err(format!("`{}` needs integers, not floats", op.symbol()));
        }
        if let (V::B(x), V::B(y)) = (a, b)
            && matches!(op, BitAnd | BitOr | BitXor)
        {
            return Ok(V::B(match op {
                BitAnd => bld.build_and(x, y, "").s()?,
                BitOr => bld.build_or(x, y, "").s()?,
                _ => bld.build_xor(x, y, "").s()?,
            }));
        }
        let x = self.to_i64(a)?;
        let y = self.to_i64(b)?;
        let i64t = self.ctx.i64_type();
        Ok(V::I(match op {
            BitAnd => bld.build_and(x, y, "").s()?,
            BitOr => bld.build_or(x, y, "").s()?,
            BitXor => bld.build_xor(x, y, "").s()?,
            _ => {
                let k = bld.build_and(y, i64t.const_int(63, false), "").s()?;
                match op {
                    Shl => bld.build_left_shift(x, k, "").s()?,
                    Shr => bld.build_right_shift(x, k, true, "").s()?,
                    _ => bld.build_right_shift(x, k, false, "").s()?,
                }
            }
        }))
    }

    pub(crate) fn scalar_unary(&self, op: UnOp, a: V<'ctx>) -> R<V<'ctx>> {
        let i64t = self.ctx.i64_type();
        Ok(match (op, a) {
            (UnOp::Not, _) => V::B(self.b.build_not(self.truth(a)?, "").s()?),
            (UnOp::BitNot, V::B(x)) => V::B(self.b.build_not(x, "").s()?),
            (UnOp::BitNot, V::F(_)) => return Err("`~` needs an integer, not a float".into()),
            (UnOp::BitNot, _) => V::I(self.b.build_not(self.to_i64(a)?, "").s()?),
            (UnOp::Neg, V::F(f)) => V::F(self.b.build_float_neg(f, "").s()?),
            (UnOp::Neg, _) => V::I(self.b.build_int_neg(self.to_i64(a)?, "").s()?),
            (UnOp::Abs, V::F(f)) => {
                let fabs = self.intrinsic("llvm.fabs")?;
                V::F(self.b.build_call(fabs, &[f.into()], "").s()?.try_as_basic_value().basic().unwrap().into_float_value())
            }
            (UnOp::Abs, _) => {
                let x = self.to_i64(a)?;
                let neg = self.b.build_int_compare(IntPredicate::SLT, x, i64t.const_zero(), "").s()?;
                let nx = self.b.build_int_neg(x, "").s()?;
                V::I(self.b.build_select(neg, nx, x, "").s()?.into_int_value())
            }
        })
    }

    fn is_unboxed(v: &V) -> bool {
        !matches!(v, V::P(..))
    }

    fn expr(&mut self, e: &Expr) -> R<V<'ctx>> {
        let i64t = self.ctx.i64_type();
        let ty = self.ty_of(e);
        // kernel fusion: whole array expressions become one loop
        match &e.kind {
            ExprKind::Prim { prim: p @ (Prim::Sum | Prim::Product | Prim::Maximum | Prim::Minimum | Prim::Len), args }
                if self.is_fae(&args[0]) =>
            {
                let r = self.fused_reduce(*p, &args[0])?;
                return self.as_ty(r, ty, "reduction");
            }
            ExprKind::Path(p) if p.len() == 1 && self.lazies.iter().any(|(n, _)| *n == p[0]) => return self.fused_map(e),
            _ if self.fusible_root(e) => return self.fused_map(e),
            _ => {}
        }
        match &e.kind {
            ExprKind::Int(i) => Ok(V::I(i64t.const_int(*i as u64, true))),
            ExprKind::Float(f) => Ok(V::F(self.ctx.f64_type().const_float(*f))),
            ExprKind::Bool(b) => Ok(V::B(self.ctx.bool_type().const_int(*b as u64, false))),
            ExprKind::Str(s) => {
                // an immortal literal created once and cached in a global slot
                let g = self.module.add_global(self.ptr, None, "lit");
                g.set_linkage(Linkage::Internal);
                g.set_initializer(&self.ptr.const_null());
                let p = self.cstring(s)?;
                let v = self.call_rt_ptr("mim_lit_str", &[self.gaddr(g).into(), p.into(), i64t.const_int(s.len() as u64, false).into()])?;
                let V::P(x, _) = v else { unreachable!() };
                Ok(V::P(x, false))
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
                let arr = self.to_generic(&tmp, arr)?;
                let mut vals = Vec::new();
                for (i, x) in xs.iter().enumerate() {
                    let v = self.expr(x)?;
                    let v = self.boxed(v)?;
                    let slot = unsafe { self.b.build_in_bounds_gep(arr_ty, arr, &[i64t.const_zero(), i64t.const_int(i as u64, false)], "").s()? };
                    self.b.build_store(slot, self.basic(v)).s()?;
                    vals.push(v);
                }
                let r = self.call_rt_ptr("mim_array_lit", &[i64t.const_int(n as u64, false).into(), arr.into()])?;
                for v in vals {
                    self.drop_tmp(v)?;
                }
                Ok(r)
            }
            ExprKind::Path(path) if !(path.len() == 1 && self.lookup(&path[0]).is_some()) => match self.p.resolve(self.cur_mod, path)? {
                Resolved::Variant(t, v) => self.construct(t, Some(v), vec![]),
                Resolved::Fn(f) => {
                    let v = self.fn_value(f)?;
                    self.as_ty(v, ty, "function value")
                }
                _ => Err(format!("unknown variable `{}`", path.join("::"))),
            },
            ExprKind::OpRef(op) => {
                let v = self.op_value(*op)?;
                self.as_ty(v, ty, "function value")
            }
            ExprKind::Path(path) => {
                let l = self.lookup(&path[0]).ok_or_else(|| format!("unknown variable `{}`", path.join("::")))?;
                let (slot, lt, mutable) = (l.slot, l.ty, l.mutable);
                let bv = self.b.build_load(self.repr(lt), slot, &path[0]).s()?;
                let v = self.val_of(bv, lt, false);
                if mutable { self.own(v) } else { Ok(v) }
            }
            ExprKind::Call { callee, args } => {
                if let ExprKind::Lambda { params, body } = &callee.kind {
                    // a lambda called on the spot: a direct typed call unless it captures
                    if self.captures(params, body).is_empty() {
                        let mut vals = Vec::new();
                        for a in args {
                            vals.push(self.expr(a)?);
                        }
                        let (lf, pts, rt) = self.lambda_fn(callee)?;
                        let mut av = Vec::new();
                        for (v, pt) in vals.iter().zip(&pts) {
                            av.push(self.as_ty(*v, *pt, "lambda argument")?);
                        }
                        let argv: Vec<BasicMetadataValueEnum> = av.iter().map(|a| self.basic(*a).into()).collect();
                        let r = self.b.build_call(lf, &argv, "").s()?.try_as_basic_value().basic().unwrap();
                        for a in av {
                            self.drop_tmp(a)?;
                        }
                        let v = self.val_of(r, rt, true);
                        return self.as_ty(v, ty, "lambda result");
                    }
                    let f = self.closure_value(callee)?;
                    return self.call_value(f, args, ty);
                }
                // anything that is not a named function or variant is a function value
                let named = match &callee.kind {
                    ExprKind::Path(path) if !(path.len() == 1 && self.lookup(&path[0]).is_some()) => {
                        matches!(self.p.resolve(self.cur_mod, path), Ok(Resolved::Fn(_) | Resolved::Variant(..)))
                    }
                    _ => false,
                };
                if !named {
                    let f = self.expr(callee)?;
                    return self.call_value(f, args, ty);
                }
                let ExprKind::Path(path) = &callee.kind else { return Err("unsupported callee".into()) };
                if let Ok(Resolved::Variant(t, v)) = self.p.resolve(self.cur_mod, path) {
                    let ftys = self.field_tys(t, v);
                    let mut vals = Vec::new();
                    for (i, a) in args.iter().enumerate() {
                        let x = self.expr(a)?;
                        vals.push(self.as_ty(x, ftys[i], "field")?);
                    }
                    return self.construct(t, Some(v), vals);
                }
                let fid = self.resolve_fn(path)?;
                let mut vals = Vec::new();
                for a in args {
                    vals.push(self.expr(a)?);
                }
                // a runtime intrinsic: Mimic values in, a Mimic value out
                if let FnKind::Extern { decl, .. } = &self.p.fns[fid].kind
                    && decl.intrinsic
                {
                    let name = format!("mim_std_{}", decl.name);
                    let f = match self.module.get_function(&name) {
                        Some(f) => f,
                        None => {
                            let ps: Vec<BasicMetadataTypeEnum> = vals.iter().map(|_| self.ptr.into()).collect();
                            self.module.add_function(&name, self.ptr.fn_type(&ps, false), Some(Linkage::External))
                        }
                    };
                    let mut boxed = Vec::new();
                    for v in &vals {
                        boxed.push(self.boxed(*v)?);
                    }
                    let argv: Vec<BasicMetadataValueEnum> = boxed.iter().map(|b| self.basic(*b).into()).collect();
                    let r = self.b.build_call(f, &argv, "").s()?.try_as_basic_value().basic().unwrap();
                    for b in boxed {
                        self.drop_tmp(b)?;
                    }
                    return self.as_ty(V::P(r.into_pointer_value(), true), ty, &format!("result of `{}`", decl.name));
                }
                if let FnKind::Extern { decl, .. } = &self.p.fns[fid].kind {
                    let decl = decl.clone();
                    let f = self.extern_fn(&decl);
                    let mut argv = Vec::new();
                    for (v, (_, t)) in vals.iter().zip(&decl.params) {
                        argv.push(self.mimic_to_c(*v, t)?);
                    }
                    // varargs: C's default argument promotions
                    for (i, v) in vals.iter().enumerate().skip(decl.params.len()) {
                        let ty = self.ty_of(&args[i]);
                        argv.push(match (ty, v) {
                            (Ty::F64, _) | (_, V::F(_)) => self.to_f64(*v)?.into(),
                            (Ty::Bool, _) | (_, V::B(_)) => {
                                let b = self.truth(*v)?;
                                self.b.build_int_z_extend(b, self.ctx.i32_type(), "").s()?.into()
                            }
                            (Ty::Arr(mimic_check::types::El::Char), V::P(p, _)) => self.call_rt("mim_cstr", &[(*p).into()])?.unwrap().into(),
                            (Ty::Arr(_), V::P(p, _)) => self.call_rt("mim_dataptr", &[(*p).into()])?.unwrap().into(),
                            _ => self.to_i64(*v)?.into(),
                        });
                    }
                    let r = self.b.build_call(f, &argv, "").s()?;
                    let out = match (&decl.ret, r.try_as_basic_value().basic()) {
                        (Some(t), Some(rv)) => self.c_to_mimic(rv, t)?,
                        _ => self.null(),
                    };
                    for v in vals {
                        self.drop_tmp(v)?;
                    }
                    return Ok(out);
                }
                let Some(k) = self.cur_spec.as_ref().and_then(|s| s.calls.get(&id(e)).cloned()) else {
                    // the call's argument types never became known (it cannot be reached)
                    let m = self.cstring("internal error: unreachable call")?;
                    self.call_rt("mim_panic", &[m.into()])?;
                    return Ok(self.null());
                };
                let kind = self.kind_for(fid);
                let f = self.request(&k, kind)?;
                let spec = self.spec(&k)?;
                let path_s = self.p.fn_path(fid).join("::");
                let pnames: Vec<String> = match &self.p.fns[fid].kind {
                    FnKind::User(d) => d.params.iter().map(|p| p.name.clone()).collect(),
                    _ => vec![],
                };
                let mut argv = Vec::new();
                let mut conv = Vec::new();
                for (i, v) in vals.into_iter().enumerate() {
                    let c = self.as_ty(v, spec.params[i], &format!("argument `{}` of `{path_s}`", pnames[i]))?;
                    argv.push(self.basic(c).into());
                    conv.push(c);
                }
                if kind == Kind::Native {
                    let d = self.cur_depth.ok_or("internal compiler error: native call outside a native frame")?;
                    let d1 = self.b.build_int_add(d, self.ctx.i32_type().const_int(1, false), "").s()?;
                    argv.push(d1.into());
                }
                let r = self.b.build_call(f, &argv, "").s()?.try_as_basic_value().basic().unwrap();
                for c in conv {
                    self.drop_tmp(c)?;
                }
                Ok(self.val_of(r, spec.ret, true))
            }
            ExprKind::Index { base, index } => {
                let b = self.expr(base)?;
                let b = self.boxed(b)?;
                let i = self.expr(index)?;
                let i = self.boxed(i)?;
                let r = self.call_rt_ptr("mim_index", &[self.basic(b).into(), self.basic(i).into()])?;
                self.drop_tmp(b)?;
                self.drop_tmp(i)?;
                self.as_ty(r, ty, "element")
            }
            ExprKind::Unary { op, expr } => {
                let a = self.expr(expr)?;
                if Self::is_unboxed(&a) && ty.is_unboxed() {
                    return self.scalar_unary(*op, a);
                }
                let a = self.boxed(a)?;
                let r = self.call_rt_ptr("mim_unop", &[self.i32c(*op as i32), self.basic(a).into()])?;
                self.drop_tmp(a)?;
                self.as_ty(r, ty, "operator result")
            }
            ExprKind::Binary { op, lhs, rhs }
                if matches!(op, BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge)
                    && self.ty_of(lhs) == Ty::Arr(El::Char)
                    && self.ty_of(rhs) == Ty::Arr(El::Char) =>
            {
                let a = self.expr(lhs)?;
                let b = self.expr(rhs)?;
                let (f, pred) = match op {
                    BinOp::Eq => ("mim_str_eq", IntPredicate::NE),
                    BinOp::Ne => ("mim_str_eq", IntPredicate::EQ),
                    BinOp::Lt => ("mim_str_cmp", IntPredicate::SLT),
                    BinOp::Le => ("mim_str_cmp", IntPredicate::SLE),
                    BinOp::Gt => ("mim_str_cmp", IntPredicate::SGT),
                    _ => ("mim_str_cmp", IntPredicate::SGE),
                };
                let r = self.call_rt(f, &[self.basic(a).into(), self.basic(b).into()])?.unwrap().into_int_value();
                self.drop_tmp(a)?;
                self.drop_tmp(b)?;
                Ok(V::B(self.b.build_int_compare(pred, r, i64t.const_zero(), "").s()?))
            }
            // records, variants and tuples: structural equality
            ExprKind::Binary { op, lhs, rhs }
                if matches!(op, BinOp::Eq | BinOp::Ne)
                    && ty == Ty::Bool
                    && (matches!(self.ty_of(lhs), Ty::Obj(_) | Ty::Tup(_)) || matches!(self.ty_of(rhs), Ty::Obj(_) | Ty::Tup(_))) =>
            {
                let a = self.expr(lhs)?;
                let a = self.boxed(a)?;
                let b = self.expr(rhs)?;
                let b = self.boxed(b)?;
                let r = self.call_rt("mim_equal", &[self.basic(a).into(), self.basic(b).into()])?.unwrap().into_int_value();
                self.drop_tmp(a)?;
                self.drop_tmp(b)?;
                let pred = if *op == BinOp::Eq { IntPredicate::NE } else { IntPredicate::EQ };
                Ok(V::B(self.b.build_int_compare(pred, r, i64t.const_zero(), "").s()?))
            }
            ExprKind::Binary { op, lhs, rhs } => {
                let a = self.expr(lhs)?;
                let b = self.expr(rhs)?;
                if Self::is_unboxed(&a) && Self::is_unboxed(&b) && ty.is_unboxed() {
                    return self.scalar_binary(*op, a, b);
                }
                let a = self.boxed(a)?;
                let b = self.boxed(b)?;
                let r = self.call_rt_ptr("mim_binop", &[self.i32c(*op as i32), self.basic(a).into(), self.basic(b).into()])?;
                self.drop_tmp(a)?;
                self.drop_tmp(b)?;
                self.as_ty(r, ty, "operator result")
            }
            // loop words: runtime loops over function values
            ExprKind::Prim { prim: prim @ (Prim::Gather | Prim::Trail | Prim::Evolve | Prim::Settle), args } => {
                let mut vals = Vec::new();
                for (i, a) in args.iter().enumerate() {
                    let v = self.expr(a)?;
                    // evolve's count is a plain integer
                    vals.push(if *prim == Prim::Evolve && i == 1 { v } else { self.boxed(v)? });
                }
                let r = match prim {
                    Prim::Gather => self.call_rt_ptr("mim_gather", &[self.basic(vals[0]).into(), self.basic(vals[1]).into()])?,
                    Prim::Trail => self.call_rt_ptr("mim_trail", &[self.basic(vals[0]).into(), self.basic(vals[1]).into()])?,
                    Prim::Evolve => {
                        let n = self.to_i64(vals[1])?;
                        self.call_rt_ptr("mim_evolve", &[self.basic(vals[0]).into(), n.into(), self.basic(vals[2]).into()])?
                    }
                    _ => self.call_rt_ptr("mim_settle", &[self.basic(vals[0]).into(), self.basic(vals[1]).into(), self.basic(vals[2]).into()])?,
                };
                for v in vals {
                    self.drop_tmp(v)?;
                }
                self.as_ty(r, ty, "loop result")
            }
            ExprKind::Arms { .. } => Err("internal error: `arms` reached codegen".into()),
            ExprKind::Map(kvs) => {
                let i64t = self.ctx.i64_type();
                let n = kvs.len();
                let (karr, varr) = if n == 0 {
                    (self.ptr.const_null(), self.ptr.const_null())
                } else {
                    let arr_ty = self.ptr.array_type(n as u32);
                    (self.entry_alloca_ty(arr_ty.into(), "keys")?, self.entry_alloca_ty(arr_ty.into(), "vals")?)
                };
                let mut held = Vec::new();
                for (i, (k, v)) in kvs.iter().enumerate() {
                    let arr_ty = self.ptr.array_type(n as u32);
                    for (x, arr) in [(k, karr), (v, varr)] {
                        let xv = self.expr(x)?;
                        let xv = self.boxed(xv)?;
                        let slot = unsafe { self.b.build_in_bounds_gep(arr_ty, arr, &[i64t.const_zero(), i64t.const_int(i as u64, false)], "").s()? };
                        self.b.build_store(slot, self.basic(xv)).s()?;
                        held.push(xv);
                    }
                }
                let r = self.call_rt_ptr("mim_map_lit", &[i64t.const_int(n as u64, false).into(), karr.into(), varr.into()])?;
                for x in held {
                    self.drop_tmp(x)?;
                }
                self.as_ty(r, ty, "map")
            }
            ExprKind::Prim { prim, args } => {
                let a = self.expr(&args[0])?;
                let a = self.boxed(a)?;
                let r = if *prim == Prim::Each {
                    // a function value (variable, capturing lambda, any expression): mim_each_fn
                    let value_fn = match &args[1].kind {
                        ExprKind::Lambda { params, body } => !self.captures(params, body).is_empty(),
                        ExprKind::Path(path) => path.len() == 1 && self.lookup(&path[0]).is_some(),
                        _ => true,
                    };
                    if value_fn {
                        let f = match &args[1].kind {
                            ExprKind::Lambda { .. } => self.closure_value(&args[1])?,
                            _ => self.expr(&args[1])?,
                        };
                        let f = self.boxed(f)?;
                        let r = self.call_rt_ptr("mim_each_fn", &[self.basic(a).into(), self.basic(f).into()])?;
                        self.drop_tmp(f)?;
                        self.drop_tmp(a)?;
                        return self.as_ty(r, ty, "primitive result");
                    }
                    let f = match &args[1].kind {
                        ExprKind::Lambda { .. } => self.lambda_thunk(&args[1])?,
                        _ => {
                            let ExprKind::Path(path) = &args[1].kind else { return Err("expected a function".into()) };
                            let fid = self.resolve_fn(path)?;
                            let k = self.cur_spec.as_ref().and_then(|s| s.calls.get(&id(e)).cloned()).ok_or("internal error: untyped ¨ callback")?;
                            // ¨ is already parallel across elements: use the native body
                            let kind = if self.plan.entered.contains(&fid) { Kind::Native } else { Kind::Plain };
                            self.thunk(&k, kind)?
                        }
                    };
                    let pure = self.purity.operand_is_pure(self.p, self.cur_mod, &args[1]);
                    self.call_rt_ptr("mim_each", &[self.basic(a).into(), self.fn_addr(f).into(), self.i32c(pure as i32)])?
                } else if prim.arity() == 2 {
                    let b = self.expr(&args[1])?;
                    let b = self.boxed(b)?;
                    let r = self.call_rt_ptr("mim_prim2", &[self.i32c(*prim as i32), self.basic(a).into(), self.basic(b).into()])?;
                    self.drop_tmp(b)?;
                    r
                } else {
                    self.call_rt_ptr("mim_prim1", &[self.i32c(*prim as i32), self.basic(a).into()])?
                };
                self.drop_tmp(a)?;
                self.as_ty(r, ty, "primitive result")
            }
            ExprKind::Lambda { .. } => {
                let v = self.closure_value(e)?;
                self.as_ty(v, ty, "function value")
            }
            ExprKind::Named { .. } => Err("named arguments are only allowed in calls to functions".into()),
            ExprKind::Tuple(xs) => {
                let mut vals = Vec::new();
                for x in xs {
                    vals.push(self.expr(x)?);
                }
                self.construct(usize::MAX, None, vals)
            }
            ExprKind::Record { path, fields } => {
                let Ok(Resolved::Type(t)) = self.p.resolve(self.cur_mod, path) else { return Err("not a record type".into()) };
                let order: Vec<String> = match &self.p.types[t].decl.kind {
                    mimic_syntax::ast::TypeKind::Record(fs) => fs.iter().map(|(n, _, _)| n.clone()).collect(),
                    _ => vec![],
                };
                let ftys: Vec<Ty> = match &self.p.types[t].decl.kind {
                    mimic_syntax::ast::TypeKind::Record(fs) => fs.iter().map(|(_, ft, _)| types::ann_in(self.p, self.p.types[t].module, ft).unwrap_or(Ty::Dyn)).collect(),
                    _ => vec![],
                };
                // evaluate in source order, store in declaration order
                let mut slots: Vec<Option<V>> = vec![None; order.len()];
                for (n, x, _) in fields {
                    let i = order.iter().position(|o| o == n).ok_or("unknown field")?;
                    let v = self.expr(x)?;
                    slots[i] = Some(self.as_ty(v, ftys[i], &format!("field `{n}`"))?);
                }
                let vals: Vec<V> = slots.into_iter().map(|s| s.ok_or("missing field")).collect::<Result<_, _>>()?;
                self.construct(t, None, vals)
            }
            ExprKind::Field { base, name } => {
                let b = self.expr(base)?;
                let b = self.boxed(b)?;
                let r = match self.ty_of(base) {
                    Ty::Tup(_) => {
                        let i: u64 = name.parse().map_err(|_| "bad tuple index")?;
                        self.call_rt_ptr("mim_obj_get", &[self.basic(b).into(), i64t.const_int(i, false).into()])?
                    }
                    Ty::Obj(t) => {
                        let i = self.p.types[types::obj_tid(t)].field_index(name).ok_or_else(|| format!("no field {name}"))?;
                        self.call_rt_ptr("mim_obj_get", &[self.basic(b).into(), i64t.const_int(i as u64, false).into()])?
                    }
                    _ => {
                        let n = self.global_cstr(name);
                        self.call_rt_ptr("mim_obj_get_named", &[self.basic(b).into(), n.into()])?
                    }
                };
                self.drop_tmp(b)?;
                self.as_ty(r, ty, &format!("field `{name}`"))
            }
            ExprKind::With { base, fields } => {
                let Ty::Obj(t) = self.ty_of(base) else { return Err("`with` needs a record".into()) };
                let b = self.expr(base)?;
                // mim_obj_with consumes its record: hand it an owned reference
                let mut cur = self.own(self.boxed(b)?)?;
                for (n, x, _) in fields {
                    let i = self.p.types[types::obj_tid(t)].field_index(n).ok_or("unknown field")?;
                    let v = self.expr(x)?;
                    let v = self.boxed(v)?;
                    cur = self.call_rt_ptr("mim_obj_with", &[self.basic(cur).into(), i64t.const_int(i as u64, false).into(), self.basic(v).into()])?;
                    self.drop_tmp(v)?;
                }
                Ok(cur)
            }
            ExprKind::Interp(parts) => {
                let n = parts.len() as u32;
                let arr_ty = self.ptr.array_type(n.max(1));
                let arr = self.entry_array(arr_ty)?;
                let mut temps = Vec::new();
                for (i, part) in parts.iter().enumerate() {
                    let s = match part {
                        InterpPart::Text(t) => {
                            let g = self.module.add_global(self.ptr, None, "lit");
                            g.set_linkage(Linkage::Internal);
                            g.set_initializer(&self.ptr.const_null());
                            let p = self.cstring(t)?;
                            let V::P(x, _) = self.call_rt_ptr("mim_lit_str", &[self.gaddr(g).into(), p.into(), i64t.const_int(t.len() as u64, false).into()])? else { unreachable!() };
                            V::P(x, false)
                        }
                        InterpPart::Expr(x) => {
                            let v = self.expr(x)?;
                            let v = self.boxed(v)?;
                            let s = self.call_rt_ptr("mim_format", &[self.basic(v).into()])?;
                            self.drop_tmp(v)?;
                            s
                        }
                    };
                    let slot = unsafe { self.b.build_in_bounds_gep(arr_ty, arr, &[i64t.const_zero(), i64t.const_int(i as u64, false)], "").s()? };
                    self.b.build_store(slot, self.basic(s)).s()?;
                    temps.push(s);
                }
                let r = self.call_rt_ptr("mim_concat", &[i64t.const_int(n as u64, false).into(), arr.into()])?;
                for t in temps {
                    self.drop_tmp(t)?;
                }
                Ok(r)
            }
            ExprKind::Match { scrut, arms } => {
                let sv = self.expr(scrut)?;
                let rt = if ty == Ty::Never { Ty::Dyn } else { ty };
                let f = self.cur_fn.unwrap();
                // keep an owned scrutinee in a scope so early `return`s release it
                self.scopes.push(Vec::new());
                if let V::P(p, true) = sv {
                    let slot = self.entry_alloca(Ty::Dyn, "scrutinee")?;
                    self.b.build_store(slot, p).s()?;
                    self.scopes.last_mut().unwrap().push(Local { name: "\u{0}scrutinee".into(), slot, ty: Ty::Dyn, owned: true, mutable: false });
                }
                let merge = self.ctx.append_basic_block(f, "match_end");
                let mut incoming = Vec::new();
                for arm in arms {
                    let next = self.ctx.append_basic_block(f, "next_arm");
                    self.pattern_test(&arm.pat, sv, next)?;
                    self.scopes.push(Vec::new());
                    self.pattern_bind(&arm.pat, sv)?;
                    if let Some(g) = &arm.guard {
                        let gv = self.expr(g)?;
                        let bit = self.truth(gv)?;
                        let pass = self.ctx.append_basic_block(f, "guard_ok");
                        let reject = self.ctx.append_basic_block(f, "guard_fail");
                        self.b.build_conditional_branch(bit, pass, reject).s()?;
                        self.b.position_at_end(reject);
                        // the arm's bindings die before trying the next arm
                        let scope = self.scopes.pop().unwrap();
                        self.release_scope(&scope)?;
                        self.scopes.push(scope);
                        self.b.build_unconditional_branch(next).s()?;
                        self.b.position_at_end(pass);
                    }
                    let v = self.expr(&arm.body)?;
                    if !self.terminated() {
                        let v = self.as_ty(v, rt, "match value")?;
                        let v = self.own(v)?;
                        let scope = self.scopes.pop().unwrap();
                        self.release_scope(&scope)?;
                        incoming.push((self.basic(v), self.b.get_insert_block().unwrap()));
                        self.b.build_unconditional_branch(merge).s()?;
                    } else {
                        self.scopes.pop();
                    }
                    self.b.position_at_end(next);
                }
                let m = self.cstring("no `match` arm matched")?;
                self.call_rt("mim_panic", &[m.into()])?;
                self.b.build_unreachable().s()?;
                self.b.position_at_end(merge);
                let scope = self.scopes.pop().unwrap();
                if incoming.is_empty() {
                    self.b.build_unreachable().s()?;
                    self.dead_block();
                    return Ok(self.null());
                }
                let phi = self.b.build_phi(self.repr(rt), "matchv").s()?;
                for (v, bb) in &incoming {
                    phi.add_incoming(&[(v, *bb)]);
                }
                self.release_scope(&scope)?;
                Ok(self.val_of(phi.as_basic_value(), rt, true))
            }
            ExprKind::Block(b) | ExprKind::Unsafe(b) => self.block(b),
            ExprKind::If { cond, then, els } => {
                let c = self.expr(cond)?;
                let bit = self.truth(c)?;
                let f = self.cur_fn.unwrap();
                let then_bb = self.ctx.append_basic_block(f, "then");
                let else_bb = self.ctx.append_basic_block(f, "else");
                let merge_bb = self.ctx.append_basic_block(f, "merge");
                self.b.build_conditional_branch(bit, then_bb, else_bb).s()?;
                // values of a type that is not statically a single scalar are boxed
                let rt = if ty == Ty::Never { Ty::Dyn } else { ty };
                let mut incoming = Vec::new();
                self.b.position_at_end(then_bb);
                let tv = self.block(then)?;
                if !self.terminated() {
                    let tv = self.as_ty(tv, rt, "branch value")?;
                    let tv = self.own(tv)?;
                    incoming.push((self.basic(tv), self.b.get_insert_block().unwrap()));
                    self.b.build_unconditional_branch(merge_bb).s()?;
                }
                self.b.position_at_end(else_bb);
                let ev = match els {
                    Some(x) => self.expr(x)?,
                    None => self.null(),
                };
                if !self.terminated() {
                    let ev = self.as_ty(ev, rt, "branch value")?;
                    let ev = self.own(ev)?;
                    incoming.push((self.basic(ev), self.b.get_insert_block().unwrap()));
                    self.b.build_unconditional_branch(merge_bb).s()?;
                }
                self.b.position_at_end(merge_bb);
                if incoming.is_empty() {
                    self.b.build_unreachable().s()?;
                    self.dead_block();
                    return Ok(self.null());
                }
                let phi = self.b.build_phi(self.repr(rt), "ifv").s()?;
                for (v, bb) in &incoming {
                    phi.add_incoming(&[(v, *bb)]);
                }
                Ok(self.val_of(phi.as_basic_value(), rt, true))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn suffixes_are_distinct() {
        use mimic_check::types::{El, Ty};
        assert_ne!(super::ty_suffix(&[Ty::I64]), super::ty_suffix(&[Ty::F64]));
        assert_ne!(super::ty_suffix(&[Ty::Arr(El::I64)]), super::ty_suffix(&[Ty::Arr(El::F64)]));
    }
}
