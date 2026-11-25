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
use mimic_check::{FnId, FnKind, ModId, Program, Resolved, TypeId};
use mimic_syntax::ast::*;
use mimic_syntax::glyphs::Prim;
use std::collections::HashMap;
use std::fmt::Display;
use std::path::Path;
use std::rc::Rc;

mod forkjoin;
mod ir;
mod owned;
mod fuse;
mod gpu;
mod gpufn;
mod dbg;
mod simd;
pub mod tonet;

pub struct Settings {
    /// LLVM target triple; `None` = host.
    pub triple: Option<String>,
    /// A build for this machine: its CPU and features are the defaults (also
    /// with a triple, as on Windows, where the host triple is MinGW's).
    pub native: bool,
    pub opt_level: u8,
    /// Emit the runtime leak check at exit (debug builds).
    pub leak_check: bool,
    /// The memory check (debug builds, with a runtime built for it): every
    /// release goes to the runtime, which marks what it frees, so no value
    /// is freed by compiled code out of its sight.
    pub memcheck: bool,
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
    /// No fused multiply-add: `a * b + c` rounds twice, the same on every
    /// CPU (`--strict-math`). Off by default: like C compilers, Mimic fuses
    /// them within an expression (faster; can differ in the last bits).
    pub strict_math: bool,
    /// `--fast-math`: float `+ - × ÷` and reductions may be regrouped (so
    /// vectorised), divisions use reciprocals, math functions approximate,
    /// zero's sign is ignored. Never "no NaN / no infinity": LLVM could then
    /// drop bounds checks on indices computed from floats.
    pub fast_math: bool,
    /// Debug information (DWARF): debug builds (dbg.rs).
    pub debug_info: bool,
    /// Frame pointers in every function, so a sampling profiler can walk
    /// the stack (`mimic profile`).
    pub frame_pointers: bool,
    /// Count how often each line of the program's own code runs
    /// (`mimic test --coverage`): a counter per line, a map of them next to
    /// the object file (`.covmap`), and the runtime writes the counts.
    pub coverage: bool,
    /// Machine code for a large module in parts at once, one object per
    /// part (the toolchain's split_codegen); `None`: one object.
    pub split: Option<SplitFn>,
    /// A test build: a `main` that lists and runs the program's tests
    /// (`test "…" { }`; see mim_test_main) instead of `main::main`.
    pub tests: bool,
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
    /// Plain, never opening a fork-join region (see forkjoin.rs).
    Seq,
    /// Native, never opening a fork-join region.
    NativeSeq,
    /// Inside a fork-join region: independent calls run in parallel while
    /// the hidden fork depth is below `mim_fj_cut`.
    Fork,
}

impl Kind {
    /// Has the hidden trailing `i32` depth argument.
    fn has_depth(self) -> bool {
        matches!(self, Kind::Native | Kind::NativeSeq | Kind::Fork)
    }
}

/// Write machine code for an optimised module (as `LLVMModuleRef`, with an
/// `LLVMTargetMachineRef`) in parts, one object per path; how many it wrote.
pub type SplitFn = unsafe fn(*mut std::ffi::c_void, *mut std::ffi::c_void, &[std::path::PathBuf]) -> Result<usize, String>;

/// Compile the checked program to a native object file (and optionally
/// textual LLVM IR and the generated interaction nets). Returns the file
/// written: the object, or — for backends that only emit assembly, such as
/// NVPTX — the assembly next to it (`.ptx` / `.s`).
/// Returns the files written: the object (or assembly), then any further
/// parts of it (see Settings::split).
/// A decision about where a piece of code runs, as code generation made it
/// (for `mimic why` and the labels editors show).
#[derive(Clone, Debug)]
pub struct Decision {
    /// The function it is in.
    pub fid: Option<mimic_check::FnId>,
    /// The file (index into `Program::files`) and the code it is about.
    pub file: usize,
    pub span: mimic_syntax::Span,
    /// `parallel`, `gpu`, `task`, `ordered`, `fused`, `simd`, `net`, `waits`
    pub kind: &'static str,
    /// Why, and what would change it.
    pub why: String,
}

thread_local! {
    /// Decisions being recorded (see `plan`); `None` in ordinary builds.
    static DECISIONS: std::cell::RefCell<Option<Vec<Decision>>> = const { std::cell::RefCell::new(None) };
}

/// The interaction nets of a program's pure functions (`mimic net show`):
/// the book as the runtime gets it, the book before compile-time
/// reduction, and each compiled function with its definition's name and
/// whether native code enters it through the net.
pub fn nets(p: &Program) -> (mimic_net::Book, mimic_net::Book, Vec<(mimic_check::FnId, String, bool)>) {
    let purity = mimic_check::Purity::analyze(p);
    let mut np = tonet::plan_unreduced(p, &purity);
    let before = np.book.clone();
    np.book.pre_reduce();
    let mut fns: Vec<_> = np.def_index.keys().map(|&f| (f, tonet::def_name(p, f), np.entered.contains(&f))).collect();
    fns.sort();
    (np.book, before, fns)
}

/// The function symbols of a built program (Mach-O, ELF or COFF), with
/// their addresses, sorted: `mimic profile` names sampled addresses by them.
pub fn symbols(path: &Path) -> R<Vec<(u64, String)>> {
    let buf = inkwell::memory_buffer::MemoryBuffer::create_from_file(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let bin = buf.create_binary_file(None).map_err(|e| format!("cannot read the symbols of {}: {e}", path.display()))?;
    let mut out = Vec::new();
    if let Some(mut syms) = bin.get_symbols() {
        while let Some(sym) = syms.next_symbol() {
            let Some(name) = sym.get_name() else { continue };
            let addr = sym.get_address();
            if addr != 0 {
                out.push((addr, name.to_string_lossy().into_owned()));
            }
        }
    }
    out.sort();
    out.dedup_by_key(|(a, _)| *a);
    Ok(out)
}

/// Every decision about where code runs, made by compiling `p` as a build
/// with `settings` would, but stopping before LLVM optimises or writes
/// anything.
pub fn plan(p: &Program, settings: &Settings) -> R<Vec<Decision>> {
    DECISIONS.with(|d| *d.borrow_mut() = Some(Vec::new()));
    let r = compile_inner(p, settings, Path::new(""), None, None, true);
    let log = DECISIONS.with(|d| d.borrow_mut().take()).unwrap_or_default();
    r.map(|_| log)
}

pub fn compile(p: &Program, settings: &Settings, obj: &Path, ir: Option<&Path>, net_out: Option<&Path>) -> R<Vec<std::path::PathBuf>> {
    compile_inner(p, settings, obj, ir, net_out, false)
}

fn compile_inner(p: &Program, settings: &Settings, obj: &Path, ir: Option<&Path>, net_out: Option<&Path>, plan_only: bool) -> R<Vec<std::path::PathBuf>> {
    // once per process: LLVM's target registry is not safe to fill from
    // several threads at once
    static INIT: std::sync::Once = std::sync::Once::new();
    INIT.call_once(|| Target::initialize_all(&InitializationConfig::default()));
    let triple = match &settings.triple {
        Some(t) => TargetTriple::create(t),
        None => TargetMachine::get_default_triple(),
    };
    let target = Target::from_triple(&triple).s()?;
    let (mut cpu, mut features) = if settings.native {
        (TargetMachine::get_host_cpu_name().to_string(), TargetMachine::get_host_cpu_features().to_string())
    } else {
        (String::new(), String::new()) // LLVM's default CPU for the triple
    };
    if let Some(c) = &settings.cpu {
        cpu = c.clone();
        if settings.features.is_none() && settings.native {
            features.clear(); // an explicit CPU replaces the host's feature list
        }
    }
    if let Some(f) = &settings.features {
        features = f.clone();
    }
    if std::env::var_os("MIMIC_DEBUG_CPU").is_some() {
        eprintln!("mimic: code for {} cpu={} features={}", triple.as_str().to_string_lossy(), if cpu.is_empty() { "(default)" } else { &cpu }, features);
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
        // embedded Xtensa has no position-independent code model
        .set_reloc_mode(if triple.as_str().to_string_lossy().starts_with("xtensa") { RelocMode::Static } else { RelocMode::PIC })
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
        && !settings.tests
        && let Some(e) = p.entry()
    {
        roots.push((e, vec![]));
    }
    let tests = if settings.tests { types::tests(p) } else { Vec::new() };
    for (f, _) in &tests {
        roots.push((*f, vec![]));
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

    let main_file = p.files.iter().find(|f| f.package == p.root_package).map(|f| f.path.clone()).unwrap_or_else(|| "main.mim".into());
    let dbg = settings.debug_info.then(|| dbg::Dbg::new(&module, &ctx, &main_file, settings.opt_level >= 2));
    let mut cg = Cg {
        dbg,
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
        own_thunks: HashMap::new(),
        queue: Vec::new(),
        purity,
        fn_descs: HashMap::new(),
        plan,
        net_ensure: None,
        cur_fn: None,
        cur_mod: 0,
        cur_fid: None,
        cur_spec: None,
        cur_ret: Ty::Unit,
        ret_what: String::new(),
        scopes: Vec::new(),
        native_frame: false,
        cur_depth: None,
        lambdas: 0,
        heaps: Default::default(),
        descs: Default::default(),
        lazies: Vec::new(),
        futures: Vec::new(),
        schemas: HashMap::new(),
        opt_level: settings.opt_level,
        memcheck: settings.memcheck,
        profiling: settings.frame_pointers,
        cov: settings.coverage.then(HashMap::new),
        cov_order: Vec::new(),
        contract: !settings.strict_math,
        fast: settings.fast_math,
        exit_fid: None,
        building_literal: false,
        body_exit: None,
        tail_calls: HashMap::new(),
        tail_fns: std::collections::HashSet::new(),
        ir_exit_top: 0,
        lazy_deep: None,
        cur_kind: Kind::Plain,
        borrow_binds: false,
        let_moves: false,
        tuple_moves: Vec::new(),
        after_args: Vec::new(),
        move_at: Default::default(),
        fj: Default::default(),
        fused_at: std::cell::Cell::new(None),
    };
    // every standard-library type descriptor, for the runtime (see desc())
    cg.desc(usize::MAX, None);
    for (t, td) in p.types.iter().enumerate() {
        if p.mods[td.module].package == "std" {
            if td.is_record() {
                cg.desc(t, None);
            } else {
                for v in 0..td.variants().len() {
                    cg.desc(t, Some(v));
                }
            }
        }
    }
    if !cg.plan.book.defs.is_empty() {
        let f = cg.module.add_function("_mimic_net_ensure", cg.ctx.void_type().fn_type(&[], false), Some(Linkage::Internal));
        cg.net_ensure = Some(f);
    }
    if settings.emit_main && !settings.tests && !settings.leak_check {
        cg.exit_fid = p.entry();
    }
    let entry_fn = match (settings.emit_main && !settings.tests, p.entry()) {
        (true, Some(e)) => Some(cg.request(&(e, vec![]), Kind::Plain)?),
        _ => None,
    };
    // every test as `ptr f()`, for the test main
    let mut test_fns = Vec::new();
    for (f, name) in &tests {
        test_fns.push((name.clone(), cg.thunk(&(*f, vec![]), Kind::Plain)?));
    }
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
    if settings.tests {
        cg.emit_test_main(&test_fns, settings.leak_check)?;
    }
    if plan_only {
        // the net-compiled functions: reduced as interaction nets, in parallel
        for &f in cg.plan.def_index.keys() {
            let func = &p.fns[f];
            if p.mods[func.module].package == p.root_package {
                note_at(Some(f), func.file, func.span, "net", "also compiled to an interaction net: it is pure, so when it is called from other such code, independent calls inside it are reduced in parallel by the runtime".into());
            }
        }
        return Ok(Vec::new());
    }
    if let Some(d) = &cg.dbg {
        d.finalize();
    }
    if !cg.cov_order.is_empty() && !plan_only {
        cg.emit_coverage(obj)?;
    }
    if settings.frame_pointers {
        let fp = cg.ctx.create_string_attribute("frame-pointer", "all");
        for f in cg.module.get_functions().filter(|f| f.count_basic_blocks() > 0) {
            f.add_attribute(inkwell::attributes::AttributeLoc::Function, fp);
        }
    }
    cg.mark_arg_arrays();
    cg.module.verify().map_err(|e| format!("internal compiler error: invalid LLVM IR:\n{}", e.to_string()))?;
    if settings.opt_level > 0 {
        cg.module
            .run_passes(&format!("default<O{}>", settings.opt_level.min(3)), &tm, PassBuilderOptions::create())
            .s()?;
    } else {
        // (every other function is left as written: `optnone`, which these
        // passes skip, so the debugger finds its variables where they are)
        let optnone = inkwell::attributes::Attribute::get_named_enum_kind_id("optnone");
        let noinline = inkwell::attributes::Attribute::get_named_enum_kind_id("noinline");
        for f in cg.module.get_functions().filter(|f| f.count_basic_blocks() > 0) {
            let always = f.get_enum_attribute(inkwell::attributes::AttributeLoc::Function, inkwell::attributes::Attribute::get_named_enum_kind_id("alwaysinline")).is_some();
            if !always && !cg.tail_fns.contains(f.get_name().to_string_lossy().as_ref()) {
                f.add_attribute(inkwell::attributes::AttributeLoc::Function, cg.ctx.create_enum_attribute(optnone, 0));
                f.add_attribute(inkwell::attributes::AttributeLoc::Function, cg.ctx.create_enum_attribute(noinline, 0));
            }
        }
        // debug builds: no optimisation, but a function that calls itself
        // last still runs as a loop, as in a release build, so a program
        // that works there does not overflow the stack here (variables
        // become registers first, which the debugger still follows, so a
        // release of a value moved before the call folds away)
        cg.module.run_passes("function(mem2reg,instsimplify,simplifycfg,jump-threading,simplifycfg,tailcallelim)", &tm, PassBuilderOptions::create()).s()?;
    }
    // every function of the program at a 64-byte boundary too, as the
    // runtime's (driver.rs): where the runtime's code falls no longer moves
    // a program's own hot functions within a cache line (fib's recursion,
    // which calls nothing of the runtime on one core, moved 12% with it)
    let tstr = triple.as_str().to_string_lossy().to_string();
    if !(tstr.starts_with("wasm") || tstr.starts_with("nvptx") || tstr.starts_with("amdgcn") || tstr.starts_with("spirv") || tstr.starts_with("bpf")) {
        for f in cg.module.get_functions().filter(|f| f.count_basic_blocks() > 0) {
            f.as_global_value().set_alignment(64);
        }
    }
    if let Some(ir) = ir {
        cg.module.print_to_file(ir).s()?;
    }
    // backends that only write assembly
    if tstr.starts_with("nvptx") || tstr.starts_with("xcore") {
        let asm = obj.with_extension(if tstr.starts_with("nvptx") { "ptx" } else { "s" });
        tm.write_to_file(&cg.module, FileType::Assembly, &asm).s()?;
        return Ok(vec![asm]);
    }
    // a large module: machine code for its parts at once, on several threads
    let defined = cg.module.get_functions().filter(|f| f.count_basic_blocks() > 0).count();
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get()).min(8);
    if let Some(split) = settings.split
        && defined >= 64
        && threads > 1
    {
        let stem = obj.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let paths: Vec<std::path::PathBuf> = (0..threads).map(|k| if k == 0 { obj.to_path_buf() } else { obj.with_file_name(format!("{stem}.part{k}.o")) }).collect();
        // SAFETY: the module and target machine are live LLVM objects owned
        // by this function for the whole call
        let n = unsafe { split(cg.module.as_mut_ptr() as *mut std::ffi::c_void, tm.as_mut_ptr() as *mut std::ffi::c_void, &paths)? };
        return Ok(paths[..n.max(1)].to_vec());
    }
    match tm.write_to_file(&cg.module, FileType::Object, obj) {
        Ok(()) => Ok(vec![obj.to_path_buf()]),
        Err(_) => {
            let ext = if triple.as_str().to_string_lossy().starts_with("nvptx") { "ptx" } else { "s" };
            let asm = obj.with_extension(ext);
            tm.write_to_file(&cg.module, FileType::Assembly, &asm).s()?;
            Ok(vec![asm])
        }
    }
}

/// Record a decision, when `plan` is listening.
fn note_at(fid: Option<mimic_check::FnId>, file: usize, span: mimic_syntax::Span, kind: &'static str, why: String) {
    DECISIONS.with(|d| {
        if let Some(v) = d.borrow_mut().as_mut() {
            v.push(Decision { fid, file, span, kind, why });
        }
    });
}

/// A compiled value: an unboxed scalar, or a handle with an ownership flag.
#[derive(Clone, Copy)]
enum V<'ctx> {
    P(PointerValue<'ctx>, bool),
    I(IntValue<'ctx>),
    F(FloatValue<'ctx>),
    B(IntValue<'ctx>),
    /// an f32 (LLVM `float`) and an i32 (LLVM `i32`): 32-bit numbers in
    /// registers; arithmetic widens them, computes and narrows again (which
    /// is exactly f32 / i32 arithmetic, see `narrow_to`)
    F32(FloatValue<'ctx>),
    I32(IntValue<'ctx>),
    /// A list of numbers (or truth values) of a length known statically: the
    /// elements themselves, in registers (`[n x double]`, …); see Ty::Arr.
    A(inkwell::values::ArrayValue<'ctx>, El),
    /// A flat record (see Cg::flat): its fields themselves, in registers.
    R(inkwell::values::StructValue<'ctx>, Ty),
}

struct Local<'ctx> {
    name: String,
    slot: PointerValue<'ctx>,
    ty: Ty,
    owned: bool,
    mutable: bool,
    /// used once: that use takes the value (see owned.rs)
    moves: bool,
}

/// An implicit future: `let name = f(…)` running on its own thread.
#[derive(Clone)]
struct Pending<'ctx> {
    name: String,
    /// where the runtime's future handle is kept until it is awaited
    hslot: PointerValue<'ctx>,
    ty: Ty,
}

fn stmt_span(s: &Stmt) -> mimic_syntax::Span {
    match s {
        Stmt::Let { span, .. } | Stmt::Assign { span, .. } | Stmt::LetTuple { span, .. } | Stmt::LetElse { span, .. } | Stmt::Return(_, span) => *span,
        Stmt::Expr(e) => e.span,
    }
}

/// Every single-segment path (variable use) in an expression.
fn expr_names(e: &Expr, out: &mut Vec<String>) {
    e.walk(&mut |x| {
        if let ExprKind::Path(p) = &x.kind
            && p.len() == 1
        {
            out.push(p[0].clone());
        }
    });
}

fn stmt_names(s: &Stmt) -> Vec<String> {
    let mut out = Vec::new();
    match s {
        Stmt::Let { value, .. } | Stmt::Expr(value) | Stmt::LetTuple { value, .. } => expr_names(value, &mut out),
        Stmt::Assign { name, index, value, .. } => {
            out.push(name.clone());
            if let Some(i) = index {
                expr_names(i, &mut out);
            }
            expr_names(value, &mut out);
        }
        Stmt::Return(e, _) => {
            if let Some(e) = e {
                expr_names(e, &mut out);
            }
        }
        Stmt::LetElse { value, .. } => expr_names(value, &mut out),
    }
    out
}

struct Cg<'ctx, 'p> {
    /// runtime type schemas (`json::decode[User]`), by static type
    schemas: HashMap<Ty, PointerValue<'ctx>>,
    /// implicit futures still pending, per enclosing block (awaited at `return`)
    futures: Vec<Vec<Pending<'ctx>>>,
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
    /// thunks that take over consumed arguments (owned.rs)
    own_thunks: HashMap<(Key, Kind), FunctionValue<'ctx>>,
    queue: Vec<(Key, Kind)>,
    purity: mimic_check::Purity,
    /// Descriptors of function values already emitted (`fn:<id>`, `op:<n>`).
    fn_descs: HashMap<String, PointerValue<'ctx>>,
    plan: tonet::NetPlan,
    net_ensure: Option<FunctionValue<'ctx>>,
    // current function
    cur_fn: Option<FunctionValue<'ctx>>,
    cur_mod: ModId,
    /// the function being compiled (its parameters count in purity questions)
    cur_fid: Option<usize>,
    cur_spec: Option<Rc<Spec>>,
    cur_ret: Ty,
    ret_what: String,
    scopes: Vec<Vec<Local<'ctx>>>,
    native_frame: bool,
    /// Recursion depth (hidden `i32` argument of `Native` bodies).
    cur_depth: Option<IntValue<'ctx>>,
    lambdas: usize,
    /// Runtime descriptors of record types / union variants.
    descs: std::cell::RefCell<HashMap<(TypeId, Option<usize>), PointerValue<'ctx>>>,
    /// `let` bindings of array expressions that are fused into their uses
    /// instead of being materialised (pointers into the Program's AST).
    lazies: Vec<(String, *const Expr)>,
    /// LLVM optimisation level (tail recursion becomes loops at ≥ 2).
    opt_level: u8,
    memcheck: bool,
    /// A profiling build (Settings::frame_pointers): every allocation goes
    /// through the runtime's allocator, which counts them.
    profiling: bool,
    /// Coverage counters by (file, line), and their order (Settings::coverage).
    cov: Option<HashMap<(usize, u32), inkwell::values::GlobalValue<'ctx>>>,
    cov_order: Vec<(usize, u32)>,
    /// Float `+ - ×` may fuse into multiply-adds (Settings::strict_math off).
    contract: bool,
    /// --fast-math (see Settings::fast_math)
    fast: bool,
    /// Debug information being written (debug builds; dbg.rs).
    dbg: Option<dbg::Dbg<'ctx>>,
    /// The program's `main`, when its last values need not be freed: the
    /// process ends right after it (release builds, no leak check). Freeing
    /// ten million records one by one only to exit costs more than making
    /// them; the system takes the memory back at once.
    exit_fid: Option<mimic_check::FnId>,
    /// Building a list of literals for its cache (the cached path is off).
    building_literal: bool,
    /// The scope depth of that function's body while it is compiled.
    body_exit: Option<usize>,
    /// The function's calls to itself that are its result, and the locals
    /// each one's arguments may still use (see owned::tail_self_calls)
    tail_calls: HashMap<usize, Vec<owned::TailArg>>,
    /// The functions that call themselves last (their LLVM names): in debug
    /// builds only these are optimised (into loops), the rest stay as written
    /// for the debugger
    tail_fns: std::collections::HashSet<String>,
    /// The IR region of `main`'s body while it is compiled (its address):
    /// what an expression there is handed is not released after it either,
    /// as main's own values are not (the process exits next)
    ir_exit_top: usize,
    /// In a native function of plain numbers: its key (the depth is checked
    /// before each call it makes; see `scalars_only`).
    lazy_deep: Option<Key>,
    /// This thread's heap (`mim_heap()`), fetched once at the top of each
    /// function that allocates or frees inline.
    heaps: std::cell::RefCell<Vec<(FunctionValue<'ctx>, PointerValue<'ctx>)>>,
    /// The kind of the function being defined.
    cur_kind: Kind,
    /// Pattern variables borrow what they bind (see the `match` codegen).
    borrow_binds: bool,
    /// The `let` about to be compiled may move (owned.rs).
    let_moves: bool,
    /// The names of the `let (a, b) = …` being compiled that move at their
    /// one use (as a `let`'s, see let_move_site)
    tuple_moves: Vec<String>,
    /// For the call being compiled: its arguments that are a variable at its
    /// last use (argument index, name), released once every argument is
    /// evaluated when the callee takes them (see call_user)
    after_args: Vec<(usize, String)>,
    /// The variable uses (expression ids) where a value moves (owned.rs).
    move_at: std::collections::HashSet<usize>,
    /// Fork-join state of the function being defined (see forkjoin.rs).
    fj: forkjoin::State<'ctx>,
    /// the fused expression being compiled (for the GPU decision's note)
    fused_at: std::cell::Cell<Option<mimic_syntax::Span>>,
}

/// Does `fid` call itself only in tail position, or as one operand of `+` / `×`
/// whose other operand has no self-call (LLVM's accumulator tail-recursion
/// elimination)? Then optimised builds compile the recursion to a loop.
/// Does every value in `d` (parameters, result, every expression and
/// binding) live in registers as a plain number or truth value? Then it
/// holds nothing on the heap at any point, and a call can be abandoned and
/// started over (on the net) without leaking.
fn scalars_only(spec: &Spec, d: &FnDecl) -> bool {
    let plain = |t: Ty| matches!(t, Ty::I64 | Ty::F64 | Ty::Bool | Ty::Never | Ty::Unit);
    if !spec.params.iter().all(|t| matches!(t, Ty::I64 | Ty::F64 | Ty::Bool)) || !plain(spec.ret) {
        return false;
    }
    let mut ok = true;
    d.body.walk(&mut |x| {
        match spec.exprs.get(&id(x)).copied() {
            Some(t) => ok &= plain(t),
            // a function's name in a call (values all have a type)
            None => ok &= matches!(x.kind, ExprKind::Path(_)),
        }
    });
    ok && spec.binds.values().all(|t| plain(*t)) && spec.lets.values().all(|t| plain(*t))
}

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
                if matches!(&callee.kind, ExprKind::Path(path) if matches!(self.p.lookup(self.module, path), Some(Resolved::Fn(f)) if f == self.fid)))
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
        Ty::F32 => 11,
        Ty::I32 => 12,
        _ => 0,
    }
}

/// A list type whose values are kept in registers: (element, length).
fn fixed(t: Ty) -> Option<(El, u32)> {
    match t {
        Ty::Arr(el @ (El::I64 | El::F64 | El::Bool), n) if n > 0 => Some((el, n as u32)),
        _ => None,
    }
}

fn ty_suffix(tys: &[Ty]) -> String {
    tys.iter()
        .map(|t| match t {
            Ty::I64 => "i".to_string(),
            Ty::F64 => "f".to_string(),
            Ty::Bool => "b".to_string(),
            Ty::Dyn => "d".to_string(),
            // a list of known length: its own specialisation
            Ty::Arr(e, n) if *n > 0 => format!("{}x{n}", Ty::Arr(*e, 0)).replace(['[', ']'], "_"),
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
            Ty::F32 => self.ctx.f32_type().into(),
            Ty::I32 => self.ctx.i32_type().into(),
            _ => match fixed(t) {
                Some((el, n)) => self.el_ty(el).array_type(n).into(),
                None => match self.flat(t) {
                    Some(fs) => {
                        let ts: Vec<BasicTypeEnum> = fs.iter().map(|f| self.repr(*f)).collect();
                        self.ctx.struct_type(&ts, false).into()
                    }
                    None => self.ptr.into(),
                },
            },
        }
    }

    /// A record kept in registers: one whose fields are all numbers, truth
    /// values or lists of known length (a shaped record; at most 64 numbers in
    /// all). Its field types, in declaration order.
    fn flat(&self, t: Ty) -> Option<Vec<Ty>> {
        // a tuple of numbers and truth values: in registers too (a `settle`
        // over (position, crc) keeps its state there, not on the heap)
        if let Ty::Tup(id) = t {
            // (numbers, truth values and short lists of known length, as a
            // flat record holds: a round counter and its 8 working words)
            let es = types::tuple_elems(id);
            let mut n = 0;
            for e in &es {
                match e {
                    Ty::I64 | Ty::F64 | Ty::Bool => n += 1,
                    _ => match fixed(*e) {
                        Some((_, k)) => n += k as usize,
                        None => return None,
                    },
                }
            }
            return (!es.is_empty() && n <= 64).then_some(es);
        }
        let Ty::Obj(id) = t else { return None };
        let tid = types::obj_tid(id);
        let mimic_syntax::ast::TypeKind::Record(fs) = &self.p.types[tid].decl.kind else { return None };
        if fs.is_empty() {
            return None;
        }
        let mut n = 0;
        let mut out = Vec::new();
        for (_, ft, _) in fs {
            let f = types::field_type(self.p, tid, t, ft);
            match f {
                Ty::I64 | Ty::F64 | Ty::Bool => n += 1,
                _ => match fixed(f) {
                    Some((_, k)) => n += k as usize,
                    None => return None,
                },
            }
            out.push(f);
        }
        (n <= 64).then_some(out)
    }

    /// Kept in registers: a list of known length or a flat record.
    fn in_regs(&self, t: Ty) -> bool {
        matches!(t, Ty::F32 | Ty::I32) || fixed(t).is_some() || self.flat(t).is_some()
    }

    /// The LLVM type of one element of a list kept in registers.
    fn el_ty(&self, el: El) -> BasicTypeEnum<'ctx> {
        match el {
            El::F64 => self.ctx.f64_type().into(),
            El::Bool => self.ctx.bool_type().into(),
            _ => self.ctx.i64_type().into(),
        }
    }

    /// Element `k` (a constant) of a list in registers, as a scalar value.
    fn elem_at(&self, a: inkwell::values::ArrayValue<'ctx>, el: El, k: u32) -> R<V<'ctx>> {
        let x = self.b.build_extract_value(a, k, "").s()?;
        Ok(match el {
            El::F64 => V::F(x.into_float_value()),
            El::Bool => V::B(x.into_int_value()),
            _ => V::I(x.into_int_value()),
        })
    }

    /// A list in registers from its elements (converted to `el`).
    fn fixed_from(&self, el: El, elems: &[V<'ctx>]) -> R<V<'ctx>> {
        let at = self.el_ty(el).array_type(elems.len() as u32);
        let mut a = at.get_undef();
        for (k, v) in elems.iter().enumerate() {
            let x: BasicValueEnum = match el {
                El::F64 => self.to_f64(*v)?.into(),
                El::Bool => self.truth(*v)?.into(),
                _ => self.to_i64(*v)?.into(),
            };
            a = self.b.build_insert_value(a, x, k as u32, "").s()?.into_array_value();
        }
        Ok(V::A(a, el))
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
            V::I(i) | V::B(i) | V::I32(i) => i.into(),
            V::F(f) | V::F32(f) => f.into(),
            V::A(a, _) => a.into(),
            V::R(r, _) => r.into(),
        }
    }

    fn val_of(&self, bv: BasicValueEnum<'ctx>, t: Ty, owned: bool) -> V<'ctx> {
        match t {
            Ty::I64 => V::I(bv.into_int_value()),
            Ty::F64 => V::F(bv.into_float_value()),
            Ty::Bool => V::B(bv.into_int_value()),
            Ty::F32 => V::F32(bv.into_float_value()),
            Ty::I32 => V::I32(bv.into_int_value()),
            _ => match fixed(t) {
                Some((el, _)) => V::A(bv.into_array_value(), el),
                None if self.flat(t).is_some() => V::R(bv.into_struct_value(), t),
                None => V::P(bv.into_pointer_value(), owned),
            },
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
            self.release(p)?;
        }
        Ok(())
    }

    /// In the functions that call themselves last, and those they call (two
    /// levels: what inlining may bring into them): a runtime call given an
    /// array of values on the stack, which it reads during the call and does
    /// not keep (runtime/mimic_rt.c), says so to LLVM (`captures(none)`). The
    /// array then does not escape, and the call to itself can still be a
    /// jump (a recursion that builds a string or a record on the way runs as
    /// a loop). Only there: said everywhere, LLVM marked other calls as tail
    /// calls too, and `vm` and `http_parse` ran 6-10% slower.
    fn mark_arg_arrays(&self) {
        use inkwell::values::{CallSiteValue, InstructionOpcode};
        let kind = inkwell::attributes::Attribute::get_named_enum_kind_id("captures");
        if kind == 0 || self.tail_fns.is_empty() {
            return;
        }
        let calls = |f: FunctionValue<'ctx>| -> Vec<CallSiteValue<'ctx>> {
            f.get_basic_blocks()
                .into_iter()
                .flat_map(|b| b.get_instructions())
                .filter(|i| i.get_opcode() == InstructionOpcode::Call)
                .filter_map(|i| CallSiteValue::try_from(i).ok())
                .collect()
        };
        let mut set: Vec<FunctionValue<'ctx>> = self.module.get_functions().filter(|f| self.tail_fns.contains(f.get_name().to_string_lossy().as_ref())).collect();
        let mut frontier = set.clone();
        for _ in 0..2 {
            let mut next = Vec::new();
            for f in &frontier {
                for c in calls(*f) {
                    if let Some(g) = c.get_called_fn_value()
                        && g.count_basic_blocks() > 0
                        && !set.contains(&g)
                    {
                        set.push(g);
                        next.push(g);
                    }
                }
            }
            frontier = next;
        }
        for f in set {
            for c in calls(f) {
                let Some(g) = c.get_called_fn_value() else { continue };
                let i = match g.get_name().to_bytes() {
                    b"mim_concat" | b"mim_array_lit" => 1,
                    b"mim_obj" | b"mim_closure" => 2,
                    b"mim_closure_checked" => 3,
                    _ => continue,
                };
                c.add_attribute(inkwell::attributes::AttributeLoc::Param(i), self.ctx.create_enum_attribute(kind, 0));
            }
        }
    }

    /// Release a value: short numeric vectors held once go straight back to
    /// this thread's free list, inline; everything else through mim_release.
    fn release(&self, p: PointerValue<'ctx>) -> R<()> {
        // a function that allocates has its heap at hand already
        let known = self.cur_fn.and_then(|f| self.heaps.borrow().iter().find(|(g, _)| *g == f).map(|(_, h)| *h));
        match self.heap_ok() {
            true => {
                match known {
                    Some(h) => self.b.build_call(self.release_fn(true), &[h.into(), p.into()], "").s()?,
                    None => self.b.build_call(self.release_fn(false), &[p.into()], "").s()?,
                };
            }
            false if !self.tail_calls.is_empty() => self.release_out(p)?,
            false => {
                self.call_rt("mim_release", &[p.into()])?;
            }
        }
        Ok(())
    }

    /// `mim_release`, out of line, after a check for null (a value moved
    /// away): once the variable is a register, a release after a moved
    /// value folds away, and a call before it can still be the function's
    /// last act (a recursion that runs as a loop)
    fn release_out(&self, p: PointerValue<'ctx>) -> R<()> {
        let f = self.b.get_insert_block().and_then(|b| b.get_parent()).ok_or("internal compiler error: release outside a function")?;
        let go = self.ctx.append_basic_block(f, "rel");
        let done = self.ctx.append_basic_block(f, "rel_done");
        let null = self.b.build_is_null(p, "").s()?;
        self.b.build_conditional_branch(null, done, go).s()?;
        self.b.position_at_end(go);
        self.call_rt("mim_release", &[p.into()])?;
        self.b.build_unconditional_branch(done).s()?;
        self.b.position_at_end(done);
        Ok(())
    }

    /// A fresh vector of `n` elements of `dtype` (uninitialised elements).
    fn alloc_vec(&self, dtype: i32, n: IntValue<'ctx>) -> R<V<'ctx>> {
        match self.heap()? {
            Some(h) => {
                let f = self.alloc_vec_fn();
                let r = self.b.build_call(f, &[h.into(), self.i32c(dtype), n.into()], "").s()?;
                Ok(V::P(r.try_as_basic_value().basic().unwrap().into_pointer_value(), true))
            }
            None => self.call_rt_ptr("mim_alloc_vec", &[self.i32c(dtype), n.into()]),
        }
    }

    /// This thread's heap for the current function: one `mim_heap()` call at
    /// the top of the function (optimised builds only).
    /// Whether this build allocates and frees inline: optimised builds, on
    /// targets with the atomic loads the inline paths use (not eBPF).
    fn heap_ok(&self) -> bool {
        self.cur_fn.is_some() && self.opt_level >= 2 && !self.module.get_triple().as_str().to_string_lossy().starts_with("bpf")
    }

    /// Whether a record held once is updated in place by `with` (its lists
    /// taken out and changed, not copied): every build, debug included (it
    /// needs only atomic loads, which eBPF lacks), unlike the inline heap.
    fn in_place_ok(&self) -> bool {
        self.cur_fn.is_some() && !self.module.get_triple().as_str().to_string_lossy().starts_with("bpf")
    }

    fn heap(&self) -> R<Option<PointerValue<'ctx>>> {
        let Some(f) = self.cur_fn else { return Ok(None) };
        if !self.heap_ok() {
            return Ok(None);
        }
        if let Some((_, h)) = self.heaps.borrow().iter().find(|(g, _)| *g == f) {
            return Ok(Some(*h));
        }
        let entry = f.get_first_basic_block().ok_or("function without a body")?;
        let b = self.ctx.create_builder();
        match entry.get_first_instruction() {
            Some(i) => b.position_before(&i),
            None => b.position_at_end(entry),
        }
        let h = b.build_call(self.rt("mim_heap"), &[], "heap").s()?.try_as_basic_value().basic().unwrap().into_pointer_value();
        self.heaps.borrow_mut().push((f, h));
        Ok(Some(h))
    }

    /// LLVM types shared with the runtime: `mim_val` and `heap_state`.
    fn val_ty(&self) -> inkwell::types::StructType<'ctx> {
        let (i64t, i32t) = (self.ctx.i64_type(), self.ctx.i32_type());
        // rc, dtype, rank, len, desc; the elements follow (mim_val in mimic_rt.c)
        self.ctx.struct_type(&[i64t.into(), i32t.into(), i32t.into(), i64t.into(), self.ptr.into()], false)
    }

    fn heap_ty(&self) -> inkwell::types::StructType<'ctx> {
        let i32t = self.ctx.i32_type();
        self.ctx.struct_type(&[self.ptr.array_type(3).into(), i32t.array_type(3).into(), self.ptr.into()], false)
    }

    /// The size class of a payload of `bytes` (0, 1, 2; 3: too big), as in
    /// the runtime's size_class.
    fn size_class_of(&self, b: &Builder<'ctx>, bytes: IntValue<'ctx>) -> R<IntValue<'ctx>> {
        let i64t = self.ctx.i64_type();
        let le = |n: u64| b.build_int_compare(IntPredicate::ULE, bytes, i64t.const_int(n, false), "");
        let c2 = b.build_select(le(256).s()?, i64t.const_int(2, false), i64t.const_int(3, false), "").s()?.into_int_value();
        let c1 = b.build_select(le(64).s()?, i64t.const_int(1, false), c2, "").s()?.into_int_value();
        Ok(b.build_select(le(16).s()?, i64t.const_zero(), c1, "").s()?.into_int_value())
    }

    /// `_mimic_alloc_vec(heap, dtype, n)`: alloc (mimic_rt.c) for a vector,
    /// inline when its size class has a free block, else mim_alloc_vec.
    fn alloc_vec_fn(&self) -> FunctionValue<'ctx> {
        if let Some(f) = self.module.get_function("_mimic_alloc_vec") {
            return f;
        }
        let (i64t, i32t, i8t) = (self.ctx.i64_type(), self.ctx.i32_type(), self.ctx.i8_type());
        let f = self.module.add_function("_mimic_alloc_vec", self.ptr.fn_type(&[self.ptr.into(), i32t.into(), i64t.into()], false), Some(Linkage::Internal));
        // a new vector: nothing else points into it
        f.add_attribute(inkwell::attributes::AttributeLoc::Return, self.ctx.create_enum_attribute(inkwell::attributes::Attribute::get_named_enum_kind_id("noalias"), 0));
        let build = || -> R<()> {
            let (vt, ht) = (self.val_ty(), self.heap_ty());
            let b = self.ctx.create_builder();
            let entry = self.ctx.append_basic_block(f, "entry");
            let pop = self.ctx.append_basic_block(f, "pop");
            let take = self.ctx.append_basic_block(f, "take");
            let slow = self.ctx.append_basic_block(f, "slow");
            let h = f.get_nth_param(0).unwrap().into_pointer_value();
            let dt = f.get_nth_param(1).unwrap().into_int_value();
            let n = f.get_nth_param(2).unwrap().into_int_value();
            b.position_at_end(entry);
            // bool, char and byte vectors take 1 byte per element, numbers 8
            let small = |k: u64| b.build_int_compare(IntPredicate::EQ, dt, i32t.const_int(k, false), "");
            let one = b.build_or(b.build_or(small(2).s()?, small(3).s()?, "").s()?, small(8).s()?, "").s()?;
            // f32 and i32: 4 bytes
            let four = b.build_or(small(9).s()?, small(10).s()?, "").s()?;
            let es = b.build_select(four, i64t.const_int(4, false), i64t.const_int(8, false), "").s()?.into_int_value();
            let es = b.build_select(one, i64t.const_int(1, false), es, "").s()?.into_int_value();
            // (1-byte elements: one more byte, kept 0; see payload_bytes)
            let extra = b.build_int_z_extend(one, i64t, "").s()?;
            let bytes = b.build_int_add(b.build_int_mul(n, es, "").s()?, extra, "bytes").s()?;
            let c = self.size_class_of(&b, bytes)?;
            let fits = b.build_int_compare(IntPredicate::ULT, c, i64t.const_int(3, false), "").s()?;
            // (a profiling build counts every allocation: the runtime's path)
            let fits = if self.profiling { self.ctx.bool_type().const_zero() } else { fits };
            b.build_conditional_branch(fits, pop, slow).s()?;
            b.position_at_end(pop);
            let zero = i32t.const_zero();
            let slot = unsafe { b.build_in_bounds_gep(ht, h, &[zero, zero, c], "").s()? };
            let v = b.build_load(self.ptr, slot, "v").s()?.into_pointer_value();
            let some = b.build_is_not_null(v, "").s()?;
            b.build_conditional_branch(some, take, slow).s()?;
            b.position_at_end(take);
            let next = b.build_load(self.ptr, v, "").s()?;
            b.build_store(slot, next).s()?;
            let lenp = unsafe { b.build_in_bounds_gep(ht, h, &[zero, i32t.const_int(1, false), c], "").s()? };
            let fl = b.build_load(i32t, lenp, "").s()?.into_int_value();
            b.build_store(lenp, b.build_int_sub(fl, i32t.const_int(1, false), "").s()?).s()?;
            // the header, as alloc writes it
            let fld = |k: u32| b.build_struct_gep(vt, v, k, "");
            b.build_store(fld(0).s()?, i64t.const_int(1, false)).s()?;
            b.build_store(fld(1).s()?, dt).s()?;
            b.build_store(fld(2).s()?, i32t.const_int(1, false)).s()?;
            b.build_store(fld(3).s()?, n).s()?;
            let data = unsafe { b.build_in_bounds_gep(vt, v, &[i32t.const_int(1, false)], "data").s()? };
            b.build_store(fld(4).s()?, self.ptr.const_null()).s()?;
            // the NUL after 1-byte elements (an 8-byte element vector has
            // none: index n × es of a 1-byte vector is its last byte)
            let nul = self.ctx.append_basic_block(f, "nul");
            let counted = self.ctx.append_basic_block(f, "counted");
            b.build_conditional_branch(one, nul, counted).s()?;
            b.position_at_end(nul);
            let last = unsafe { b.build_in_bounds_gep(i8t, data, &[n], "").s()? };
            b.build_store(last, i8t.const_zero()).s()?;
            b.build_unconditional_branch(counted).s()?;
            b.position_at_end(counted);
            // one more live value on this thread
            let livep = b.build_struct_gep(ht, h, 2, "").s()?;
            let live = b.build_load(self.ptr, livep, "").s()?.into_pointer_value();
            let cnt = b.build_load(i64t, live, "").s()?;
            inkwell::values::BasicValue::as_instruction_value(&cnt).unwrap().set_atomic_ordering(inkwell::AtomicOrdering::Monotonic).map_err(|e| e.to_string())?;
            let st = b.build_store(live, b.build_int_add(cnt.into_int_value(), i64t.const_int(1, false), "").s()?).s()?;
            st.set_atomic_ordering(inkwell::AtomicOrdering::Monotonic).map_err(|e| e.to_string())?;
            b.build_return(Some(&v)).s()?;
            b.position_at_end(slow);
            let r = b.build_call(self.rt("mim_alloc_vec"), &[dt.into(), n.into()], "").s()?.try_as_basic_value().basic().unwrap();
            b.build_return(Some(&r)).s()?;
            Ok(())
        };
        build().expect("building _mimic_alloc_vec");
        f
    }

    /// `_mimic_release(v)`: mim_release, with dealloc inline for a vector of
    /// numbers held once whose block fits a free list (this thread's heap is
    /// fetched only then: releases that free nothing cost nothing more).
    /// (`with_heap`: `_mimic_release_h(heap, v)`, for functions that have it.)
    fn release_fn(&self, with_heap: bool) -> FunctionValue<'ctx> {
        let name = if with_heap { "_mimic_release_h" } else { "_mimic_release" };
        if let Some(f) = self.module.get_function(name) {
            return f;
        }
        let (i64t, i32t) = (self.ctx.i64_type(), self.ctx.i32_type());
        let params: Vec<BasicMetadataTypeEnum> = if with_heap { vec![self.ptr.into(), self.ptr.into()] } else { vec![self.ptr.into()] };
        let f = self.module.add_function(name, self.ctx.void_type().fn_type(&params, false), Some(Linkage::Internal));
        // (the memory check: the runtime frees everything, and marks it)
        if self.memcheck {
            let b = self.ctx.create_builder();
            b.position_at_end(self.ctx.append_basic_block(f, "entry"));
            let v = f.get_nth_param(if with_heap { 1 } else { 0 }).unwrap();
            b.build_call(self.rt("mim_release"), &[v.into()], "").expect("building _mimic_release");
            b.build_return(None).expect("building _mimic_release");
            return f;
        }
        let build = || -> R<()> {
            let (vt, ht) = (self.val_ty(), self.heap_ty());
            let b = self.ctx.create_builder();
            let entry = self.ctx.append_basic_block(f, "entry");
            let check = self.ctx.append_basic_block(f, "check");
            let kind_bb = self.ctx.append_basic_block(f, "kind");
            let push_bb = self.ctx.append_basic_block(f, "push");
            let done = self.ctx.append_basic_block(f, "done");
            let slow = self.ctx.append_basic_block(f, "slow");
            let v = f.get_nth_param(if with_heap { 1 } else { 0 }).unwrap().into_pointer_value();
            b.position_at_end(entry);
            let null = b.build_is_null(v, "").s()?;
            b.build_conditional_branch(null, done, check).s()?;
            b.position_at_end(check);
            let rcp = b.build_struct_gep(vt, v, 0, "").s()?;
            let rc = b.build_load(i64t, rcp, "rc").s()?;
            inkwell::values::BasicValue::as_instruction_value(&rc).unwrap().set_atomic_ordering(inkwell::AtomicOrdering::Acquire).map_err(|e| e.to_string())?;
            let once = b.build_int_compare(IntPredicate::EQ, rc.into_int_value(), i64t.const_int(1, false), "").s()?;
            b.build_conditional_branch(once, kind_bb, slow).s()?;
            // a vector (rank 1) of i64 or f64: nothing inside to release
            b.position_at_end(kind_bb);
            let kp = b.build_struct_gep(vt, v, 1, "").s()?;
            let kind = b.build_load(i64t, kp, "").s()?.into_int_value();
            let little = self.module.get_data_layout().as_str().to_string_lossy().starts_with('e');
            let k = |dt: u64| if little { (1u64 << 32) | dt } else { (dt << 32) | 1 };
            let is_i = b.build_int_compare(IntPredicate::EQ, kind, i64t.const_int(k(0), false), "").s()?;
            let is_f = b.build_int_compare(IntPredicate::EQ, kind, i64t.const_int(k(1), false), "").s()?;
            let num = b.build_or(is_i, is_f, "").s()?;
            let len = b.build_load(i64t, b.build_struct_gep(vt, v, 3, "").s()?, "").s()?.into_int_value();
            let bytes = b.build_int_mul(len, i64t.const_int(8, false), "").s()?;
            let c = self.size_class_of(&b, bytes)?;
            let fits = b.build_int_compare(IntPredicate::ULT, c, i64t.const_int(3, false), "").s()?;
            let zero = i32t.const_zero();
            let numfit = b.build_and(num, fits, "").s()?;
            let heap_bb = self.ctx.append_basic_block(f, "heap");
            b.build_conditional_branch(numfit, heap_bb, slow).s()?;
            b.position_at_end(heap_bb);
            let h = if with_heap {
                f.get_nth_param(0).unwrap().into_pointer_value()
            } else {
                b.build_call(self.rt("mim_heap"), &[], "heap").s()?.try_as_basic_value().basic().unwrap().into_pointer_value()
            };
            // c < 3 is checked before the free-list length is read
            let c_safe = b.build_select(fits, c, i64t.const_zero(), "").s()?.into_int_value();
            let lenp = unsafe { b.build_in_bounds_gep(ht, h, &[zero, i32t.const_int(1, false), c_safe], "").s()? };
            let fl = b.build_load(i32t, lenp, "").s()?.into_int_value();
            let room = b.build_int_compare(IntPredicate::ULT, fl, i32t.const_int(1 << 18, false), "").s()?; // FREE_LIST_CAP
            b.build_conditional_branch(room, push_bb, slow).s()?;
            b.position_at_end(push_bb);
            // (MIMIC_COUNT_RC=1: the one release that does not reach the
            // runtime, counted there; the code around stays as it is)
            if counting_rc() {
                b.build_call(self.rt("mim_rc_inline_free"), &[], "").s()?;
            }
            let slot = unsafe { b.build_in_bounds_gep(ht, h, &[zero, zero, c_safe], "").s()? };
            let head = b.build_load(self.ptr, slot, "").s()?;
            b.build_store(v, head).s()?;
            b.build_store(slot, v).s()?;
            b.build_store(lenp, b.build_int_add(fl, i32t.const_int(1, false), "").s()?).s()?;
            let livep = b.build_struct_gep(ht, h, 2, "").s()?;
            let live = b.build_load(self.ptr, livep, "").s()?.into_pointer_value();
            let cnt = b.build_load(i64t, live, "").s()?;
            inkwell::values::BasicValue::as_instruction_value(&cnt).unwrap().set_atomic_ordering(inkwell::AtomicOrdering::Monotonic).map_err(|e| e.to_string())?;
            let st = b.build_store(live, b.build_int_sub(cnt.into_int_value(), i64t.const_int(1, false), "").s()?).s()?;
            st.set_atomic_ordering(inkwell::AtomicOrdering::Monotonic).map_err(|e| e.to_string())?;
            b.build_unconditional_branch(done).s()?;
            // an immortal value (a small number, true, false, a literal):
            // never counted or freed, so nothing to call the runtime for
            b.position_at_end(slow);
            let call_bb = self.ctx.append_basic_block(f, "call");
            let immortal = b.build_int_compare(IntPredicate::SGE, rc.into_int_value(), i64t.const_int(1 << 62, false), "").s()?; // MIM_IMMORTAL
            b.build_conditional_branch(immortal, done, call_bb).s()?;
            b.position_at_end(call_bb);
            b.build_call(self.rt("mim_release"), &[v.into()], "").s()?;
            b.build_unconditional_branch(done).s()?;
            b.position_at_end(done);
            b.build_return(None).s()?;
            Ok(())
        };
        build().expect("building _mimic_release");
        f
    }

    /// Box a value into a runtime handle (owned for new boxes).
    fn boxed(&self, v: V<'ctx>) -> R<V<'ctx>> {
        Ok(match v {
            V::P(..) => v,
            V::F32(_) => self.call_rt_ptr("mim_float32", &[self.to_f64(v)?.into()])?,
            V::I32(_) => self.call_rt_ptr("mim_int32", &[self.to_i64(v)?.into()])?,
            // a flat record becomes a record on the heap
            V::R(r, t) => {
                let fs = self.flat(t).ok_or("internal compiler error: not a flat record")?;
                let mut vals = Vec::new();
                for (k, ft) in fs.iter().enumerate() {
                    let x = self.b.build_extract_value(r, k as u32, "").s()?;
                    vals.push(self.val_of(x, *ft, false));
                }
                match t {
                    Ty::Obj(id) => self.construct_in(types::obj_tid(id), None, vals)?,
                    // (a tuple: what `(a, b)` makes on the heap)
                    _ => self.construct_in(usize::MAX, None, vals)?,
                }
            }
            // a list in registers becomes a vector on the heap
            V::A(a, el) => {
                let n = a.get_type().len();
                let dt = match el {
                    El::F64 => 1,
                    El::Bool => 2,
                    _ => 0,
                };
                let r = self.alloc_vec(dt, self.ctx.i64_type().const_int(n as u64, false))?;
                let V::P(p, _) = r else { unreachable!() };
                let data = unsafe { self.b.build_in_bounds_gep(self.val_ty(), p, &[self.ctx.i32_type().const_int(1, false)], "data").s()? };
                let et: BasicTypeEnum = if el == El::Bool { self.ctx.i8_type().into() } else { self.el_ty(el) };
                for k in 0..n {
                    let x = self.b.build_extract_value(a, k, "").s()?;
                    let x: BasicValueEnum = if el == El::Bool { self.b.build_int_z_extend(x.into_int_value(), self.ctx.i8_type(), "").s()?.into() } else { x };
                    let ep = unsafe { self.b.build_in_bounds_gep(et, data, &[self.ctx.i64_type().const_int(k as u64, false)], "").s()? };
                    self.b.build_store(ep, x).s()?;
                }
                r
            }
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
            V::I32(i) => self.b.build_int_s_extend(i, self.ctx.i64_type(), "").s()?,
            V::F(f) | V::F32(f) => self.b.build_float_to_signed_int(f, self.ctx.i64_type(), "").s()?,
            V::P(p, _) => self.call_rt("mim_as_i64", &[p.into()])?.unwrap().into_int_value(),
            // a list where a number is expected: the runtime reports it
            V::A(..) | V::R(..) => return self.to_i64(self.boxed(v)?),
        })
    }

    fn to_f64(&self, v: V<'ctx>) -> R<FloatValue<'ctx>> {
        Ok(match v {
            V::F(f) => f,
            V::F32(f) => self.b.build_float_ext(f, self.ctx.f64_type(), "").s()?,
            V::I(i) | V::I32(i) => self.b.build_signed_int_to_float(i, self.ctx.f64_type(), "").s()?,
            V::B(b) => self.b.build_unsigned_int_to_float(b, self.ctx.f64_type(), "").s()?,
            V::P(p, _) => self.call_rt("mim_as_f64", &[p.into()])?.unwrap().into_float_value(),
            V::A(..) | V::R(..) => return self.to_f64(self.boxed(v)?),
        })
    }

    /// Truth of a condition (`i1`).
    fn truth(&self, v: V<'ctx>) -> R<IntValue<'ctx>> {
        Ok(match v {
            V::B(b) => b,
            V::I(i) | V::I32(i) => self.b.build_int_compare(IntPredicate::NE, i, i.get_type().const_zero(), "").s()?,
            V::F(f) | V::F32(f) => self.b.build_float_compare(FloatPredicate::UNE, f, f.get_type().const_zero(), "").s()?,
            V::P(p, _) => {
                let t = self.call_rt("mim_truthy", &[p.into()])?.unwrap().into_int_value();
                self.drop_tmp(v)?;
                self.b.build_int_compare(IntPredicate::NE, t, self.ctx.i64_type().const_zero(), "").s()?
            }
            V::A(..) | V::R(..) => return self.truth(self.boxed(v)?),
        })
    }

    /// Convert a value to the representation of static type `t`. Unboxing a
    /// handle checks its run-time type (`what` names the slot in the message).
    fn as_ty(&self, v: V<'ctx>, t: Ty, what: &str) -> R<V<'ctx>> {
        if let Some((el, n)) = fixed(t) {
            return self.as_fixed(v, el, n, what);
        }
        if matches!(t, Ty::F32 | Ty::I32) {
            return self.narrow_to(v, t, what);
        }
        // lists of f32 and i32: converted by the runtime, which rounds to
        // f32 and range-checks i32
        let narrow = match t {
            Ty::Arr(El::F32, _) => Some(9),
            Ty::Arr(El::I32, _) => Some(10),
            _ => None,
        };
        if let Some(dt) = narrow {
            let b = self.boxed(v)?;
            let f = if matches!(t, Ty::Arr(..)) { "mim_vec_as" } else { "mim_scalar_as" };
            let r = self.call_rt_ptr(f, &[self.basic(b).into(), self.i32c(dt)])?;
            self.drop_tmp(b)?;
            return Ok(r);
        }
        if let Some(fs) = self.flat(t) {
            return self.as_flat(v, t, &fs, what);
        }
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

    /// A value as an f32 (rounded, as the runtime's `(float)` of a double)
    /// or an i32 (one that does not fit stops the program, as the runtime
    /// does).
    pub(crate) fn narrow_to(&self, v: V<'ctx>, t: Ty, what: &str) -> R<V<'ctx>> {
        let (f32t, i32t) = (self.ctx.f32_type(), self.ctx.i32_type());
        match (t, v) {
            (Ty::F32, V::F32(_)) | (Ty::I32, V::I32(_)) => Ok(v),
            (Ty::F32, V::F(_) | V::I(_) | V::I32(_) | V::B(_)) => {
                let d = self.to_f64(v)?;
                Ok(V::F32(self.b.build_float_trunc(d, f32t, "").s()?))
            }
            (Ty::I32, V::I(x)) => {
                let n = self.b.build_int_truncate(x, i32t, "").s()?;
                if x.get_sign_extended_constant().is_some_and(|c| c == c as i32 as i64) {
                    return Ok(V::I32(n));
                }
                // out of range: the runtime says so (and stops)
                let back = self.b.build_int_s_extend(n, self.ctx.i64_type(), "").s()?;
                let bad = self.b.build_int_compare(IntPredicate::NE, back, x, "").s()?;
                let f = self.cur_fn.unwrap();
                let (bb, ok) = (self.ctx.append_basic_block(f, "i32_range"), self.ctx.append_basic_block(f, "i32_ok"));
                self.b.build_conditional_branch(bad, bb, ok).s()?;
                self.b.position_at_end(bb);
                let boxed = self.boxed(v)?;
                self.call_rt("mim_scalar_as", &[self.basic(boxed).into(), self.i32c(10)])?;
                self.b.build_unconditional_branch(ok).s()?;
                self.b.position_at_end(ok);
                Ok(V::I32(n))
            }
            (Ty::I32, V::B(_)) => Ok(V::I32(self.b.build_int_truncate(self.to_i64(v)?, i32t, "").s()?)),
            _ => {
                // anything else (a handle, a float as an i32): the runtime
                let b = self.boxed(v)?;
                let w = self.cstring(what)?;
                let _ = w;
                let dt = if t == Ty::F32 { 9 } else { 10 };
                let r = self.call_rt_ptr("mim_scalar_as", &[self.basic(b).into(), self.i32c(dt)])?;
                self.drop_tmp(b)?;
                let out = if t == Ty::F32 {
                    let d = self.to_f64(r)?;
                    V::F32(self.b.build_float_trunc(d, f32t, "").s()?)
                } else {
                    V::I32(self.b.build_int_truncate(self.to_i64(r)?, i32t, "").s()?)
                };
                self.drop_tmp(r)?;
                Ok(out)
            }
        }
    }

    /// The `n` elements of a vector `q` known to hold exactly that many of
    /// `el`, into registers (no checks: the caller knows).
    fn load_fixed(&self, q: PointerValue<'ctx>, el: El, n: u32) -> R<V<'ctx>> {
        let (i32t, i64t) = (self.ctx.i32_type(), self.ctx.i64_type());
        let data = unsafe { self.b.build_in_bounds_gep(self.val_ty(), q, &[i32t.const_int(1, false)], "data").s()? };
        let et: BasicTypeEnum = if el == El::Bool { self.ctx.i8_type().into() } else { self.el_ty(el) };
        let mut elems = Vec::new();
        for k in 0..n {
            let ep = unsafe { self.b.build_in_bounds_gep(et, data, &[i64t.const_int(k as u64, false)], "").s()? };
            let x = self.b.build_load(et, ep, "").s()?;
            elems.push(match el {
                El::F64 => V::F(x.into_float_value()),
                El::Bool => V::B(self.b.build_int_compare(IntPredicate::NE, x.into_int_value(), self.ctx.i8_type().const_zero(), "").s()?),
                _ => V::I(x.into_int_value()),
            });
        }
        self.fixed_from(el, &elems)
    }

    /// The field `name` of record `b` (of type `t`, boxed), read straight
    /// from the record: its fields follow its header (alloc in mimic_rt.c).
    /// Borrowed while the record is.
    fn field_ptr(&self, b: V<'ctx>, t: u32, name: &str, ty: Ty) -> R<PointerValue<'ctx>> {
        let (i32t, i64t) = (self.ctx.i32_type(), self.ctx.i64_type());
        let i = self.p.types[types::obj_tid(t)].field_index(name).ok_or_else(|| format!("no field {name}"))?;
        let bp = self.basic(b).into_pointer_value();
        let data = unsafe { self.b.build_in_bounds_gep(self.val_ty(), bp, &[i32t.const_int(1, false)], "fields").s()? };
        let invariant = self.ctx.get_kind_id("invariant.load");
        let empty = self.ctx.metadata_node(&[]);
        let fp = unsafe { self.b.build_in_bounds_gep(self.ptr, data, &[i64t.const_int(i as u64, false)], "").s()? };
        let fv = self.b.build_load(self.ptr, fp, "").s()?;
        inkwell::values::BasicValue::as_instruction_value(&fv).unwrap().set_metadata(empty, invariant).map_err(|e| e.to_string())?;
        // a shaped list field is a heap vector like any other: same facts
        self.assume_live(fv, if fixed(ty).is_some() { Ty::Arr(El::F64, 0) } else { ty })?;
        Ok(fv.into_pointer_value())
    }

    /// `v` as the flat record type `t` (fields `fs`): another flat record of
    /// the same declared type converted field by field, or a record on the
    /// heap read field by field (each checked as usual).
    fn as_flat(&self, v: V<'ctx>, t: Ty, fs: &[Ty], what: &str) -> R<V<'ctx>> {
        self.as_flat_in(v, t, fs, what, true)
    }

    /// (`check`: false when the value is known to be a record of type `t`,
    /// an element of a list of them)
    fn as_flat_in(&self, v: V<'ctx>, t: Ty, fs: &[Ty], what: &str, check: bool) -> R<V<'ctx>> {
        let st = self.repr(t).into_struct_type();
        let mut out = st.get_undef();
        match v {
            V::R(_, u) if u == t => return Ok(v),
            V::R(r, u) => {
                let us = self.flat(u).ok_or("internal compiler error: not a flat record")?;
                for (k, ft) in fs.iter().enumerate() {
                    let x = self.b.build_extract_value(r, k as u32, "").s()?;
                    let x = self.as_ty(self.val_of(x, us[k], false), *ft, what)?;
                    out = self.b.build_insert_value(out, self.basic(x), k as u32, "").s()?.into_struct_value();
                }
            }
            V::P(p, _) if matches!(t, Ty::Tup(_)) => {
                // a tuple on the heap: element by element (the runtime checks
                // that it is a tuple long enough)
                for (k, ft) in fs.iter().enumerate() {
                    let x = self.call_rt_ptr("mim_obj_get", &[p.into(), self.ctx.i64_type().const_int(k as u64, false).into()])?;
                    let x = self.as_ty(x, *ft, what)?;
                    out = self.b.build_insert_value(out, self.basic(x), k as u32, "").s()?.into_struct_value();
                }
                self.drop_tmp(v)?;
            }
            V::P(p, _) => {
                let Ty::Obj(id) = t else { unreachable!() };
                // a record of this type (else the runtime reports it)
                if check {
                    let w = self.cstring(what)?;
                    let d = self.desc(types::obj_tid(id), None);
                    self.call_rt("mim_check_record", &[p.into(), d.into(), w.into()])?;
                }
                let i32t = self.ctx.i32_type();
                let fields = unsafe { self.b.build_in_bounds_gep(self.val_ty(), p, &[i32t.const_int(1, false)], "fields").s()? };
                for (k, ft) in fs.iter().enumerate() {
                    let fp = unsafe { self.b.build_in_bounds_gep(self.ptr, fields, &[self.ctx.i64_type().const_int(k as u64, false)], "").s()? };
                    if let Some(el) = self.raw_field(types::obj_tid(id), None, k) {
                        let x = self.load_raw(fp, el)?;
                        let x = self.as_ty(x, *ft, what)?;
                        out = self.b.build_insert_value(out, self.basic(x), k as u32, "").s()?.into_struct_value();
                        continue;
                    }
                    let fv = self.b.build_load(self.ptr, fp, "").s()?.into_pointer_value();
                    let x = self.as_ty(V::P(fv, false), *ft, what)?;
                    out = self.b.build_insert_value(out, self.basic(x), k as u32, "").s()?.into_struct_value();
                }
                self.drop_tmp(v)?;
            }
            other => {
                let b = self.boxed(other)?;
                return self.as_flat(b, t, fs, what);
            }
        }
        Ok(V::R(out, t))
    }

    /// `v` as a list of `n` elements of `el` in registers: converted from
    /// another list in registers, or read from a vector (checked once: a
    /// vector of that length; integers where floats are wanted are converted
    /// by the runtime, anything else is an error).
    fn as_fixed(&self, v: V<'ctx>, el: El, n: u32, what: &str) -> R<V<'ctx>> {
        match v {
            V::A(a, e) if e == el && a.get_type().len() == n => Ok(v),
            V::A(a, e) => {
                let elems: Vec<V> = (0..a.get_type().len().min(n)).map(|k| self.elem_at(a, e, k)).collect::<R<_>>()?;
                self.fixed_from(el, &elems)
            }
            V::P(p, _) => {
                let (i32t, i64t) = (self.ctx.i32_type(), self.ctx.i64_type());
                let dt: u64 = match el {
                    El::F64 => 1,
                    El::Bool => 2,
                    _ => 0,
                };
                let w = self.cstring(what)?;
                // the runtime returns a vector of that type and length (a new
                // reference), or stops the program
                let q = self.call_rt("mim_fixed_in", &[p.into(), i32t.const_int(dt, false).into(), i64t.const_int(n as u64, false).into(), w.into()])?.unwrap().into_pointer_value();
                let r = self.load_fixed(q, el, n)?;
                self.release(q)?;
                self.drop_tmp(v)?;
                Ok(r)
            }
            // a scalar where a list is expected: let the runtime report it
            other => {
                let b = self.boxed(other)?;
                self.as_fixed(b, el, n, what)
            }
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
            "mim_float" | "mim_float32" => ptr.fn_type(&[f64t], false),
            "mim_int32" => ptr.fn_type(&[i64t], false),
            "mim_lit_str" => ptr.fn_type(&[p, p, i64t], false),
            "mim_lit_variant" => ptr.fn_type(&[p, p], false),
            "mim_array_lit" => ptr.fn_type(&[i64t, p], false),
            "mim_retain" | "mim_release" | "mim_panic" => void.fn_type(&[p], false),
            "mim_rc_inline_free" => void.fn_type(&[], false),
            "mim_binop" | "mim_prim2" => ptr.fn_type(&[i32t, p, p], false),
            "mim_unop" | "mim_prim1" => ptr.fn_type(&[i32t, p], false),
            "mim_index" => ptr.fn_type(&[p, p], false),
            "mim_pat_is" => self.ctx.i64_type().fn_type(&[p, p], false),
            "mim_pat_len" => self.ctx.i64_type().fn_type(&[p], false),
            "mim_pat_at" | "mim_pat_rest" => ptr.fn_type(&[p, i64t], false),
            "mim_each" | "mim_each_own" => ptr.fn_type(&[p, p, i32t], false),
            "mim_each_raw" => ptr.fn_type(&[p, p, i32t, i32t], false),
            "mim_each_memo" => ptr.fn_type(&[p, p, i32t, p], false),
            "mim_each_fn_memo" => ptr.fn_type(&[p, p, p], false),
            "mim_set_index" => void.fn_type(&[p, p, p], false),
            "mim_set_index_v" => ptr.fn_type(&[p, p, p], false),
            "mim_shrink_own" => ptr.fn_type(&[p, i64t, i32t], false),
            "mim_truthy" | "mim_as_i64" => self.ctx.i64_type().fn_type(&[p], false),
            "mim_decides" => self.ctx.i64_type().fn_type(&[p, self.ctx.i64_type().into()], false),
            "mim_as_f64" => self.ctx.f64_type().fn_type(&[p], false),
            "mim_cstr" | "mim_dataptr" | "mim_from_cstr" => ptr.fn_type(&[p], false),
            "mim_check_type" => void.fn_type(&[p, i32t, p], false),
            "mim_is_type" => self.ctx.i32_type().fn_type(&[p, i32t], false),
            "mim_rt_init" => void.fn_type(&[i32t, p], false),
            "mim_rt_ensure" => void.fn_type(&[], false),
            "mim_closure" => ptr.fn_type(&[p, i64t, p], false),
            "mim_closure_checked" => ptr.fn_type(&[p, p, i64t, p], false),
            "mim_index_fail" => void.fn_type(&[p, i64t], false),
            "mim_byte_fail" => void.fn_type(&[i64t, i64t], false),
            "mim_sort_i64_small" | "mim_sort_f64_small" => void.fn_type(&[p, i64t], false),
            "mim_vec_as" => ptr.fn_type(&[p, i32t], false),
            "mim_call" => ptr.fn_type(&[p, i32t, p], false),
            "mim_spawn" => ptr.fn_type(&[p, i32t, p], false),
            "mim_await" => ptr.fn_type(&[p], false),
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
            "mim_lit_list" => ptr.fn_type(&[p, p], false),
            "mim_obj_unique" => ptr.fn_type(&[p], false),
            "mim_format" | "mim_std_char_code" => ptr.fn_type(&[p], false),
            "mim_concat" => ptr.fn_type(&[i64t, p], false),
            "mim_str_eq" | "mim_str_cmp" | "mim_equal" => self.ctx.i64_type().fn_type(&[p, p], false),
            "mim_unwrap_ok" => self.ctx.i64_type().fn_type(&[p], false),
            "mim_par_for" => void.fn_type(&[i64t, p, p], false),
            "mim_par_for_grain" | "mim_par_for_contig" => void.fn_type(&[i64t, i64t, p, p], false),
            "mim_gpu_wants" => self.ctx.i32_type().fn_type(&[i64t, i64t], false),
            "mim_gpu_run" => self.ctx.i32_type().fn_type(&[p, i64t, i32t, p, p, p, i32t, p, i64t], false),
            "mim_par_for_timed" => void.fn_type(&[i64t, p, p], false),
            "mim_scalar_as" => ptr.fn_type(&[p, i32t], false),
            "mim_format_i64" => ptr.fn_type(&[i64t], false),
            "mim_fj_enter" => self.ctx.i32_type().fn_type(&[], false),
            "mim_fj_leave" => void.fn_type(&[], false),
            "mim_fj_fork" => void.fn_type(&[p, p, p], false),
            "mim_fj_join" => void.fn_type(&[p], false),
            "mim_par_grain" => self.ctx.i64_type().fn_type(&[], false),
            "mim_alloc_vec" => ptr.fn_type(&[i32t, i64t], false),
            "mim_heap" => ptr.fn_type(&[], false),
            "mim_fixed_in" => ptr.fn_type(&[p, i32t, i64t, p], false),
            "mim_check_record" => void.fn_type(&[p, p, p], false),
            "mim_vec_len" => self.ctx.i64_type().fn_type(&[p, i32t], false),
            "mim_route" => self.ctx.i64_type().fn_type(&[p, i64t, p, p, p], false),
            other => panic!("unknown runtime function {other}"),
        };
        let f = self.module.add_function(name, ty, Some(Linkage::External));
        let attr = |n: &str| self.ctx.create_enum_attribute(inkwell::attributes::Attribute::get_named_enum_kind_id(n), 0);
        if name == "mim_alloc_vec" {
            // a new vector: nothing else points into it
            f.add_attribute(inkwell::attributes::AttributeLoc::Return, attr("noalias"));
        }
        // these never free a value nor synchronise with other threads: values
        // read before them stay readable after them (so reads may move
        // around them, out of loops)
        // this thread's heap is the same for the whole call (a coroutine never
        // moves between threads): one fetch serves inlined callees too
        if name == "mim_heap" {
            let none = self.ctx.create_enum_attribute(inkwell::attributes::Attribute::get_named_enum_kind_id("memory"), 0);
            f.add_attribute(inkwell::attributes::AttributeLoc::Function, none);
            f.add_attribute(inkwell::attributes::AttributeLoc::Function, attr("nounwind"));
            f.add_attribute(inkwell::attributes::AttributeLoc::Function, attr("willreturn"));
        }
        if matches!(name, "mim_alloc_vec" | "mim_heap" | "mim_index_fail" | "mim_vec_len") {
            f.add_attribute(inkwell::attributes::AttributeLoc::Function, attr("nofree"));
            f.add_attribute(inkwell::attributes::AttributeLoc::Function, attr("nosync"));
        }
        f
    }

    fn call_rt(&self, name: &str, args: &[BasicMetadataValueEnum<'ctx>]) -> R<Option<BasicValueEnum<'ctx>>> {
        let f = self.rt(name);
        Ok(self.b.build_call(f, args, "").s()?.try_as_basic_value().basic())
    }

    /// How `xs ¨ f` runs: 0 in order; 1 pure (spread over the cores); 2
    /// effects that may overlap, such as waiting on the network (a task per
    /// element on the event loop, so thousands can wait at once).
    fn each_mode(&self, f: &Expr) -> i32 {
        let src = |e: &Expr| self.cur_fid.and_then(|k| self.p.files[self.p.fns[k].file].src.get(e.span.start..e.span.end)).unwrap_or("f").to_string();
        if self.purity.operand_is_pure(self.p, self.cur_mod, self.cur_fid, f) {
            self.note(f.span, "parallel", format!("`¨` applies `{}` to every element on all cores: it is pure", src(f)));
            1
        } else if self.purity.operand_overlaps(self.p, self.cur_mod, self.cur_fid, f) {
            self.note(f.span, "task", format!("`¨` runs `{}` as a task per element on the event loop: its effects may overlap (reading, waiting), so thousands can wait at once", src(f)));
            2
        } else {
            self.note(f.span, "ordered", format!("`¨` applies `{}` to one element after another: it has effects that keep their order (printing, writing files, C calls). Without them (return the values and print after the `¨`) it would run on all cores", src(f)));
            0
        }
    }

    /// Record a decision about the code at `span` of the function being
    /// compiled (the program's own functions only).
    fn note(&self, span: mimic_syntax::Span, kind: &'static str, why: String) {
        if let Some(f) = self.cur_fid
            && self.p.mods[self.p.fns[f].module].package == self.p.root_package
        {
            note_at(Some(f), self.p.fns[f].file, span, kind, why);
        }
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
            Kind::Seq => ".seq",
            Kind::NativeSeq => ".nseq",
            Kind::Fork => ".fork",
        };
        let name = format!("{}${}{}", self.p.fns[k.0].symbol, ty_suffix(&k.1), suffix);
        let mut ft = self.fn_type(&s.params, s.ret);
        if kind.has_depth() {
            // hidden trailing argument: the native recursion depth
            let mut ps: Vec<BasicMetadataTypeEnum> = s.params.iter().map(|t| self.repr(*t).into()).collect();
            ps.push(self.ctx.i32_type().into());
            ft = self.repr(s.ret).fn_type(&ps, false);
        }
        let f = self.module.add_function(&name, ft, Some(Linkage::Internal));
        // a vector parameter always points at a live header (rc, dtype, rank,
        // len, data): LLVM may read it early, so header checks leave loops
        // (not one the function consumes: it may be gone before the end)
        let consumed = self.consumed(k);
        for (i, t) in s.params.iter().enumerate() {
            if matches!(t, Ty::Arr(El::F64 | El::I64 | El::Bool, 0) | Ty::Obj(_)) && self.flat(*t).is_none() && !consumed.get(i).copied().unwrap_or(false) {
                let attr = |name: &str, v: u64| self.ctx.create_enum_attribute(inkwell::attributes::Attribute::get_named_enum_kind_id(name), v);
                f.add_attribute(inkwell::attributes::AttributeLoc::Param(i as u32), attr("nonnull", 0));
                f.add_attribute(inkwell::attributes::AttributeLoc::Param(i as u32), attr("dereferenceable", self.live_bytes(*t)));
                f.add_attribute(inkwell::attributes::AttributeLoc::Param(i as u32), attr("align", 8));
            }
        }
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
        let base = if self.plan.entered.contains(&fid) {
            if self.native_frame {
                Kind::Native
            } else if self.plan.wide.contains(&fid) && self.fj_target() {
                // a recursion that splits itself: fork-join from ordinary code
                Kind::Plain
            } else {
                Kind::NetWrap
            }
        } else {
            Kind::Plain
        };
        // a sequential or fork version calls itself sequentially (a loop
        // stays a loop: it calls the same function)
        if Some(fid) == self.cur_fid && matches!(self.cur_kind, Kind::Seq | Kind::NativeSeq | Kind::Fork) {
            return forkjoin::seq_of(base);
        }
        base
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
        self.cur_fid = Some(k.0);
        self.cur_spec = Some(spec.clone());
        self.cur_ret = spec.ret;
        self.ret_what = format!("return value of `{path}`");
        self.native_frame = false;
        self.lazy_deep = None;
        self.cur_kind = kind;
        let entry = self.ctx.append_basic_block(fv, "entry");
        self.b.position_at_end(entry);
        self.dbg_enter(fv, &path, Some(self.cur_mod), decl.span.line, false);
        self.scopes = vec![Vec::new()];
        self.fj_begin(fv, kind, decl)?;
        // parameters it consumes arrive owned (released at the end unless moved on)
        let sites = self.consumed_sites(k);
        let consumed = self.consumed(k);
        self.move_at.clear();
        self.move_at.extend(sites.iter().flatten());
        for (i, prm) in decl.params.iter().enumerate() {
            let t = spec.params[i];
            let slot = self.entry_alloca(t, &prm.name)?;
            self.b.build_store(slot, fv.get_nth_param(i as u32).unwrap()).s()?;
            self.dbg_var(&prm.name, slot, t, decl.span.line, Some(i as u32 + 1));
            let c = consumed.get(i).copied().unwrap_or(false);
            let mut m = sites.get(i).is_some_and(|s| !s.is_empty());
            // a list (or a value of unknown type) taken over: it moves on at
            // its last use in each branch (into the `push` that grows it in
            // place, into the call that takes it)
            if c && !m && matches!(t, Ty::Arr(_, 0) | Ty::Dyn) {
                let at = owned::last_use_sites(&prm.name, &decl.body);
                if !at.is_empty() {
                    self.move_at.extend(at);
                    m = true;
                }
            }
            self.scopes[0].push(Local { name: prm.name.clone(), slot, ty: t, owned: c, mutable: false, moves: m });
        }
        if self.opt_level >= 2 {
            for (i, t) in spec.params.iter().enumerate() {
                if !consumed.get(i).copied().unwrap_or(false) {
                    self.assume_params_live(fv.get_nth_param(i as u32).unwrap(), *t)?;
                }
            }
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
            // (an accumulated float sum is not a loop to LLVM: it may not
            // reorder float additions, so only integer results qualify)
            Kind::Native | Kind::NativeSeq if self.opt_level >= 2 && matches!(spec.ret, Ty::I64 | Ty::Bool | Ty::Unit) && loops_only(self.p, k.0, decl) => {
                // every self-call is a tail call (or `x + f(…)` / `x × f(…)`):
                // LLVM turns the recursion into a loop, so it cannot grow the stack
                let depth = fv.get_nth_param(decl.params.len() as u32).unwrap().into_int_value();
                self.native_frame = true;
                self.cur_depth = Some(depth);
            }
            Kind::Native | Kind::NativeSeq if self.opt_level >= 2 && scalars_only(spec.as_ref(), decl) => {
                // a function of plain numbers only: the depth is checked just
                // before it calls itself (calls that end the recursion pay
                // nothing); past the limit it starts over on the net, which is
                // safe: it is pure, and holds nothing that needs releasing
                let depth = fv.get_nth_param(decl.params.len() as u32).unwrap().into_int_value();
                self.native_frame = true;
                self.cur_depth = Some(depth);
                self.lazy_deep = Some(k.clone());
            }
            Kind::Native | Kind::NativeSeq => {
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
            Kind::Fork => {
                // at most `mim_fj_cut` deep: no depth checks of its own; the
                // fork depth counts as the native depth of what it calls
                let depth = fv.get_nth_param(decl.params.len() as u32).unwrap().into_int_value();
                self.native_frame = true;
                self.cur_depth = Some(depth);
                self.fj.mode = forkjoin::Mode::Fork(depth);
            }
            Kind::Plain | Kind::Seq => {}
        }
        self.body_exit = (self.exit_fid == Some(k.0) && kind == Kind::Plain && k.1.is_empty()).then_some(self.scopes.len() + 1);
        // (the IR's embedded expressions too: one that is the function's
        // result ends with the call as well)
        self.tail_calls = if self.heap_ok_target() { owned::tail_self_calls(p, p.fns[k.0].module, k.0, &decl.body) } else { HashMap::new() };
        if !self.tail_calls.is_empty() {
            self.tail_fns.insert(fv.get_name().to_string_lossy().into_owned());
        }
        // through the Mimic IR, when it covers the function (MIMIC_IR, ir.rs)
        let (use_ir, print_ir) = ir::wanted();
        // (MIMIC_IR_ONLY=text: only functions whose path holds it, to find
        // which function an IR difference comes from)
        let only = std::env::var("MIMIC_IR_ONLY").ok();
        // (not in coverage builds: the IR counts no lines)
        let lowered = if use_ir && self.cov.is_none() && matches!(kind, Kind::Plain | Kind::Seq | Kind::Native | Kind::NativeSeq) && self.fj.quiet() && only.as_ref().is_none_or(|o| path.contains(o.as_str())) {
            let takes = self.consumed(k);
            mimic_ir::lower::function(self.p, k.0, spec.clone(), &takes, self).filter(|f| match mimic_ir::verify(f) {
                Ok(()) => true,
                Err(e) => {
                    eprintln!("mimic: IR check failed, compiling `{path}` without it: {e}");
                    false
                }
            })
        } else {
            None
        };
        let v = match &lowered {
            Some(f) => {
                if print_ir {
                    eprint!("{f}");
                }
                self.ir_body(f)?
            }
            None => self.block(&decl.body)?,
        };
        self.body_exit = None;
        if !self.terminated() {
            self.ret(v)?;
        }
        // (after the return: its releases check for null too)
        self.tail_calls.clear();
        self.scopes.clear();
        self.native_frame = false;
        self.cur_depth = None;
        self.fj_end();
        self.dbg_leave();
        Ok(())
    }

    fn terminated(&self) -> bool {
        self.b.get_insert_block().and_then(|b| b.get_terminator()).is_some()
    }

    /// Convert to the return type, release every owned local, and return.
    fn ret(&mut self, v: V<'ctx>) -> R<()> {
        // leaving the function: every implicit future still running finishes first
        let pending: Vec<Pending<'ctx>> = self.futures.iter().flatten().cloned().collect();
        for p in pending {
            let h = self.b.build_load(self.ptr, p.hslot, "").s()?;
            let r = self.call_rt("mim_await", &[h.into()])?.unwrap().into_pointer_value();
            self.call_rt("mim_release", &[r.into()])?;
        }
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
    /// Count one run of the line of `span` (coverage builds, the program's
    /// own functions, not tests).
    fn cover(&mut self, span: mimic_syntax::Span) -> R<()> {
        let (Some(fid), true) = (self.cur_fid, self.cov.is_some()) else { return Ok(()) };
        if span.line == 0 {
            return Ok(());
        }
        let f = &self.p.fns[fid];
        if self.p.mods[f.module].package != self.p.root_package || f.name.starts_with("test ") || f.name.starts_with("prop_") {
            return Ok(());
        }
        let key = (f.file, span.line);
        let i64t = self.ctx.i64_type();
        let g = match self.cov.as_ref().unwrap().get(&key) {
            Some(g) => *g,
            None => {
                let g = self.module.add_global(i64t, None, &format!("__mimic_cov_{}", self.cov_order.len()));
                g.set_linkage(Linkage::Internal);
                g.set_initializer(&i64t.const_zero());
                self.cov.as_mut().unwrap().insert(key, g);
                self.cov_order.push(key);
                g
            }
        };
        self.b
            .build_atomicrmw(inkwell::AtomicRMWBinOp::Add, g.as_pointer_value(), i64t.const_int(1, false), inkwell::AtomicOrdering::Monotonic)
            .s()?;
        Ok(())
    }

    /// The table of coverage counters, registered with the runtime before
    /// `main` (a constructor), and the map of which line each counts.
    fn emit_coverage(&mut self, obj: &Path) -> R<()> {
        let i64t = self.ctx.i64_type();
        let ptrs: Vec<PointerValue<'ctx>> = self.cov_order.iter().map(|k| self.cov.as_ref().unwrap()[k].as_pointer_value()).collect();
        let arr = self.ptr.const_array(&ptrs);
        let table = self.module.add_global(arr.get_type(), None, "__mimic_cov_table");
        table.set_linkage(Linkage::Internal);
        table.set_constant(true);
        table.set_initializer(&arr);
        let reg = self.module.add_function("mim_cov_register", self.ctx.void_type().fn_type(&[self.ptr.into(), i64t.into()], false), Some(Linkage::External));
        let init = self.module.add_function("__mimic_cov_init", self.ctx.void_type().fn_type(&[], false), Some(Linkage::Internal));
        let b = self.ctx.create_builder();
        b.position_at_end(self.ctx.append_basic_block(init, "entry"));
        b.build_call(reg, &[table.as_pointer_value().into(), i64t.const_int(ptrs.len() as u64, false).into()], "").s()?;
        b.build_return(None).s()?;
        // llvm.global_ctors: { priority, function, data }
        let i32t = self.ctx.i32_type();
        let entry_ty = self.ctx.struct_type(&[i32t.into(), self.ptr.into(), self.ptr.into()], false);
        let entry = entry_ty.const_named_struct(&[i32t.const_int(65535, false).into(), init.as_global_value().as_pointer_value().into(), self.ptr.const_null().into()]);
        let ctors = self.module.add_global(entry_ty.array_type(1), None, "llvm.global_ctors");
        ctors.set_linkage(Linkage::Appending);
        ctors.set_initializer(&entry_ty.const_array(&[entry]));
        // the lines of functions nothing compiled (never called) count too,
        // as never run: after the counters, in the map only
        let mut order = self.cov_order.clone();
        let mut seen: std::collections::HashSet<(usize, u32)> = order.iter().copied().collect();
        for f in &self.p.fns {
            let FnKind::User(d) = &f.kind else { continue };
            if self.p.mods[f.module].package != self.p.root_package || f.name.starts_with("test ") || f.name.starts_with("prop_") {
                continue;
            }
            let mut blocks: Vec<*const Block> = vec![&d.body];
            d.body.walk(&mut |e| match &e.kind {
                ExprKind::Block(b) | ExprKind::Unsafe(b) => blocks.push(b),
                ExprKind::If { then, .. } => blocks.push(then),
                ExprKind::Arms { body, .. } => blocks.push(body),
                _ => {}
            });
            for b in blocks {
                // SAFETY: pointers into `self.p`, alive for this call
                let b = unsafe { &*b };
                let spans = b.stmts.iter().map(stmt_span).chain(b.tail.as_deref().map(|t| t.span));
                for sp in spans {
                    if sp.line > 0 && seen.insert((f.file, sp.line)) {
                        order.push((f.file, sp.line));
                    }
                }
            }
        }
        // the map: [["src/main.mim", "/abs/src/main.mim", 12], …], in counter order
        let esc = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"");
        let rows: Vec<String> = order
            .iter()
            .map(|(f, line)| {
                let file = &self.p.files[*f];
                format!("[\"{}\", \"{}\", {line}]", esc(&file.display), esc(&file.path.display().to_string()))
            })
            .collect();
        std::fs::write(obj.with_extension("covmap"), format!("[\n{}\n]\n", rows.join(",\n"))).map_err(|e| format!("cannot write the coverage map: {e}"))?;
        Ok(())
    }

    /// A fused kernel's name: the function it comes from, so profiles and
    /// debuggers show where its time belongs (`…main__f$i$kernel3`).
    fn kernel_name(&self) -> String {
        match self.cur_fn {
            Some(f) => format!("{}$kernel{}", f.get_name().to_string_lossy(), self.lambdas),
            None => format!("_mimic_kernel_{}", self.lambdas),
        }
    }

    fn in_new_fn<T>(&mut self, fv: FunctionValue<'ctx>, f: impl FnOnce(&mut Self) -> R<T>) -> R<T> {
        let saved_bb = self.b.get_insert_block();
        let saved_fn = self.cur_fn.replace(fv);
        let saved_scopes = std::mem::take(&mut self.scopes);
        let saved_native = std::mem::replace(&mut self.native_frame, false);
        let saved_depth = self.cur_depth.take();
        let saved_lazy = self.lazy_deep.take();
        let saved_ret = self.cur_ret;
        let saved_what = std::mem::take(&mut self.ret_what);
        let saved_futures = std::mem::take(&mut self.futures);
        let saved_kind = std::mem::replace(&mut self.cur_kind, Kind::Plain);
        let saved_dbg = self.dbg_save();
        let entry = self.ctx.append_basic_block(fv, "entry");
        self.b.position_at_end(entry);
        // (made by the compiler: described in the file and at the line of
        // the code it was made for)
        let line = saved_dbg.as_ref().and_then(|(_, l)| l.map(|l| l.get_line())).unwrap_or(0);
        let name = fv.get_name().to_string_lossy().into_owned();
        self.dbg_enter(fv, &name, None, line, true);
        let r = f(self);
        self.dbg_restore(saved_dbg);
        self.cur_fn = saved_fn;
        self.scopes = saved_scopes;
        self.native_frame = saved_native;
        self.cur_depth = saved_depth;
        self.lazy_deep = saved_lazy;
        self.cur_ret = saved_ret;
        self.ret_what = saved_what;
        self.futures = saved_futures;
        self.cur_kind = saved_kind;
        if let Some(bb) = saved_bb {
            self.b.position_at_end(bb);
        }
        r
    }

    /// `ptr thunk(ptr…)`: unbox arguments (checked), call the typed function, box the result.
    fn thunk(&mut self, k: &Key, kind: Kind) -> R<FunctionValue<'ctx>> {
        self.thunk_with(k, kind, false)
    }

    /// `owning`: the arguments for parameters the function consumes are
    /// handed over by the caller (mim_each_own), else they are lent.
    fn thunk_with(&mut self, k: &Key, kind: Kind, owning: bool) -> R<FunctionValue<'ctx>> {
        if let Some(f) = (if owning { &self.own_thunks } else { &self.thunks }).get(&(k.clone(), kind)) {
            return Ok(*f);
        }
        let target = self.request(k, kind)?;
        let spec = self.spec(k)?;
        let n = spec.params.len();
        let name = format!("{}$box{}", target.get_name().to_str().unwrap(), if owning { "_own" } else { "" });
        let ps: Vec<BasicMetadataTypeEnum> = (0..n).map(|_| self.ptr.into()).collect();
        let f = self.module.add_function(&name, self.ptr.fn_type(&ps, false), Some(Linkage::Internal));
        if owning {
            self.own_thunks.insert((k.clone(), kind), f);
        } else {
            self.thunks.insert((k.clone(), kind), f);
        }
        let consumed = self.consumed(k);
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
                // a consumed parameter: handed over (owning), else a new reference
                let v = if consumed.get(i).copied().unwrap_or(false) && !owning { cg.hand_over(v)? } else { v };
                args.push(cg.basic(v).into());
            }
            if kind.has_depth() {
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
        self.dbg_leave();
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

    /// `main` of a test build: the runtime's mim_test_main with every test's
    /// name and function (it lists them, runs one, or runs all).
    fn emit_test_main(&mut self, tests: &[(String, FunctionValue<'ctx>)], leak_check: bool) -> R<()> {
        let i32t = self.ctx.i32_type();
        let ty = i32t.fn_type(&[i32t.into(), self.ptr.into()], false);
        let wasm = self.module.get_triple().as_str().to_string_lossy().starts_with("wasm");
        let main = self.module.add_function(if wasm { "__main_argc_argv" } else { "main" }, ty, None);
        let bb = self.ctx.append_basic_block(main, "entry");
        self.b.position_at_end(bb);
        self.dbg_leave();
        self.call_rt("mim_rt_init", &[main.get_nth_param(0).unwrap().into(), main.get_nth_param(1).unwrap().into()])?;
        let n = tests.len().max(1) as u32;
        let names: Vec<PointerValue> = tests.iter().map(|(t, _)| self.global_cstr(t)).collect();
        let fns: Vec<PointerValue> = tests.iter().map(|(_, f)| self.fn_addr(*f)).collect();
        let arr = |cg: &Self, vals: Vec<PointerValue<'ctx>>, name: &str| {
            let vals = if vals.is_empty() { vec![cg.ptr.const_null()] } else { vals };
            let a = cg.ptr.const_array(&vals);
            let g = cg.module.add_global(cg.ptr.array_type(n), None, name);
            g.set_initializer(&a);
            g.set_constant(true);
            g.set_linkage(Linkage::Private);
            cg.gaddr(g)
        };
        let (np, fp) = (arr(self, names, "test_names"), arr(self, fns, "test_fns"));
        let f = self.module.get_function("mim_test_main").unwrap_or_else(|| {
            let t = i32t.fn_type(&[i32t.into(), self.ptr.into(), self.ptr.into(), i32t.into()], false);
            self.module.add_function("mim_test_main", t, Some(Linkage::External))
        });
        let code = self.b.build_call(f, &[i32t.const_int(tests.len() as u64, false).into(), np.into(), fp.into(), self.i32c(leak_check as i32)], "").s()?;
        let code = code.try_as_basic_value().basic().unwrap();
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
            Type::Ptr(..) | Type::Array(..) | Type::Fn(..) | Type::App(..) | Type::Map(..) | Type::Tuple(..) => self.ptr.into(),
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
                if l.moves {
                    // (inline: a value that moved away left a null, which
                    // LLVM sees and drops the release entirely)
                    self.release(cur.into_pointer_value())?;
                } else if !self.tail_calls.is_empty() {
                    // (a function that calls itself last: checked for the
                    // null an early release left, see release_out)
                    self.release_out(cur.into_pointer_value())?;
                } else {
                    self.call_rt("mim_release", &[cur.into()])?;
                }
            }
        }
        Ok(())
    }

    fn block(&mut self, b: &Block) -> R<V<'ctx>> {
        self.scopes.push(Vec::new());
        // the end of `main`: the process exits next (see Cg::exit_fid)
        let at_exit = self.body_exit == Some(self.scopes.len());
        let lazy_mark = self.lazies.len();
        // implicit futures started in this block and not awaited yet
        let mut pending: Vec<Pending<'ctx>> = Vec::new();
        self.futures.push(Vec::new());
        for (i, s) in b.stmts.iter().enumerate() {
            if !pending.is_empty() {
                if self.purity.stmt_may_overlap(self.p, self.cur_mod, self.cur_fid, s) {
                    self.await_used(&mut pending, &stmt_names(s))?;
                } else {
                    let names: Vec<&str> = pending.iter().map(|p| p.name.as_str()).collect();
                    self.note(stmt_span(s), "waits", format!("waits here for the tasks still running ({}): this changes what they might read (files, the environment, programs) or leaves early", names.join(", ")));
                    self.await_all(&mut pending)?; // an ordered effect, or leaving the block
                }
            }
            // `let a = f(…)` `let b = f(…)`: independent calls in parallel
            if pending.is_empty() {
                self.fork_lets(&b.stmts[i..])?;
            }
            // `let x = f(…)` with only overlapping effects: start it, await at first use
            if let Stmt::Let { name, mutable: false, value, .. } = s
                && let Some(p) = self.start_future(s, name, value)?
            {
                pending.push(p);
                continue;
            }
            // an array `let` used only inside other array expressions is fused
            // into those uses rather than materialised
            if let Stmt::Let { name, mutable: false, ty: None, value, .. } = s
                && self.lazy_candidate(name, value, &b.stmts[i + 1..], b.tail.as_deref())
            {
                self.lazies.push((name.clone(), value as *const Expr));
                continue;
            }
            *self.futures.last_mut().unwrap() = pending.clone(); // for `return`s inside
            self.let_moves = false;
            if let Stmt::Let { name, mutable: false, .. } = s
                && let Some(site) = self.let_move_site(name, &b.stmts[i + 1..], b.tail.as_deref())
            {
                self.let_moves = true;
                self.move_at.insert(site);
            }
            // (each name of a tuple `let` likewise: `let (p, root) = find(…)`
            // then `let mut c = p` changes the list in place, not a copy)
            // (only a function's result, which nothing else holds: a tuple
            // a variable still holds, `let (i, open, jump) = s`, keeps its
            // parts, and moving those of a `settle`'s state gained nothing
            // but code)
            if let Stmt::LetTuple { names, value, .. } = s
                && matches!(value.kind, ExprKind::Call { .. })
            {
                for n in names.iter().filter(|n| *n != "_") {
                    if let Some(site) = self.let_move_site(n, &b.stmts[i + 1..], b.tail.as_deref()) {
                        self.move_at.insert(site);
                        self.tuple_moves.push(n.clone());
                    }
                }
            }
            self.stmt(s)?;
            self.let_moves = false;
            self.tuple_moves.clear();
        }
        if !pending.is_empty() && !self.terminated() {
            match &b.tail {
                Some(e) if self.purity.expr_may_overlap(self.p, self.cur_mod, self.cur_fid, e) => {
                    let mut names = Vec::new();
                    expr_names(e, &mut names);
                    self.await_used(&mut pending, &names)?
                }
                _ => self.await_all(&mut pending)?,
            }
        }
        if self.terminated() {
            self.dead_block();
        }
        *self.futures.last_mut().unwrap() = pending.clone();
        let v = match &b.tail {
            Some(e) => {
                if !self.terminated() {
                    self.cover(e.span)?;
                }
                self.expr(e)?
            }
            None => self.null(),
        };
        // every future finishes before the block does
        if !pending.is_empty() && !self.terminated() {
            self.await_all(&mut pending)?;
        }
        self.futures.pop();
        let v = self.own(v)?;
        let scope = self.scopes.pop().unwrap();
        self.lazies.truncate(lazy_mark);
        if !self.terminated() && !at_exit {
            self.release_scope(&scope)?;
        }
        Ok(v)
    }

    /// Start `let name = f(args)` as an implicit future when `f` qualifies.
    fn start_future(&mut self, s: &Stmt, name: &str, value: &Expr) -> R<Option<Pending<'ctx>>> {
        if self.purity.future_call(self.p, self.cur_mod, self.cur_fid, value).is_none() {
            return Ok(None);
        }
        self.note(value.span, "task", format!("starts now as a task (its effects may overlap: reading, waiting) and the code after it goes on; it is waited for where `{name}` is first used"));
        let Some(k) = self.cur_spec.as_ref().and_then(|sp| sp.calls.get(&id(value)).cloned()) else { return Ok(None) };
        let ExprKind::Call { args, .. } = &value.kind else { return Ok(None) };
        let t = self.cur_spec.as_ref().and_then(|sp| sp.lets.get(&id(s)).copied()).unwrap_or(Ty::Dyn);
        // arguments now, in order; the call itself on its own thread
        let mut vals = Vec::new();
        for a in args {
            let v = self.expr(a)?;
            vals.push(self.boxed(v)?);
        }
        let n = vals.len();
        let arr_ty = self.ptr.array_type(n.max(1) as u32);
        let arr = self.entry_array(arr_ty)?;
        let i64t = self.ctx.i64_type();
        for (i, v) in vals.iter().enumerate() {
            let at = unsafe { self.b.build_in_bounds_gep(arr_ty, arr, &[i64t.const_zero(), i64t.const_int(i as u64, false)], "").s()? };
            self.b.build_store(at, self.basic(*v)).s()?;
        }
        let f = self.spec_value(&k)?;
        let h = self.call_rt("mim_spawn", &[self.basic(f).into(), self.i32c(n as i32), arr.into()])?.unwrap();
        self.drop_tmp(f)?;
        for v in vals {
            self.drop_tmp(v)?;
        }
        let hslot = self.entry_alloca_ty(self.ptr.into(), &format!("{name}.future"))?;
        self.b.build_store(hslot, h).s()?;
        Ok(Some(Pending { name: name.to_string(), hslot, ty: t }))
    }

    /// Await the futures whose variables `names` mentions.
    fn await_used(&mut self, pending: &mut Vec<Pending<'ctx>>, names: &[String]) -> R<()> {
        let (now, later): (Vec<_>, Vec<_>) = std::mem::take(pending).into_iter().partition(|p| names.contains(&p.name));
        *pending = later;
        for p in now {
            self.await_one(p)?;
        }
        Ok(())
    }

    fn await_all(&mut self, pending: &mut Vec<Pending<'ctx>>) -> R<()> {
        for p in std::mem::take(pending) {
            self.await_one(p)?;
        }
        Ok(())
    }

    /// Wait for a future and bind its variable (from here on an ordinary local).
    fn await_one(&mut self, p: Pending<'ctx>) -> R<()> {
        let h = self.b.build_load(self.ptr, p.hslot, "").s()?;
        let r = self.call_rt("mim_await", &[h.into()])?.unwrap().into_pointer_value();
        let v = self.as_ty(V::P(r, true), p.ty, &format!("`let {}`", p.name))?;
        let v = self.own(v)?;
        let slot = self.entry_alloca(p.ty, &p.name)?;
        self.b.build_store(slot, self.basic(v)).s()?;
        let owned = matches!(v, V::P(..));
        self.scopes.last_mut().unwrap().push(Local { name: p.name, slot, ty: p.ty, owned, mutable: false, moves: false });
        Ok(())
    }

    fn stmt(&mut self, s: &Stmt) -> R<()> {
        if self.terminated() {
            self.dead_block();
        }
        self.cover(stmt_span(s))?;
        match s {
            Stmt::Let { name, mutable, value, span, .. } => {
                let v = self.expr(value)?;
                let t = self.cur_spec.as_ref().and_then(|sp| sp.lets.get(&id(s)).copied()).unwrap_or(Ty::Dyn);
                let what = if matches!(&value.kind, ExprKind::Path(p) if p.len() == 1 && p[0].contains('·')) { format!("parameter `{name}`") } else { format!("`let {name}`") };
                let v = self.as_ty(v, t, &what)?;
                let v = self.own(v)?;
                let slot = self.entry_alloca(t, name)?;
                self.b.build_store(slot, self.basic(v)).s()?;
                self.dbg_var(name, slot, t, span.line, None);
                let owned = matches!(v, V::P(..));
                let moves = std::mem::take(&mut self.let_moves);
                self.scopes.last_mut().unwrap().push(Local { name: name.clone(), slot, ty: t, owned, mutable: *mutable, moves });
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
                    // an element of a list in registers: checked against its
                    // known length, stored in place
                    Some(i) if fixed(t).is_some() && self.ty_of(i) == Ty::I64 => {
                        let (el, n) = fixed(t).unwrap();
                        let i64t = self.ctx.i64_type();
                        let iv = self.expr(i)?;
                        let iv = self.to_i64(iv)?;
                        let v = self.expr(value)?;
                        let x: BasicValueEnum = match el {
                            El::F64 => self.to_f64(v)?.into(),
                            El::Bool => self.truth(v)?.into(),
                            _ => self.to_i64(v)?.into(),
                        };
                        let ok = self.b.build_int_compare(IntPredicate::ULT, iv, i64t.const_int(n as u64, false), "").s()?;
                        let f = self.cur_fn.unwrap();
                        let good = self.ctx.append_basic_block(f, "set_ok");
                        let bad = self.ctx.append_basic_block(f, "set_bad");
                        self.b.build_conditional_branch(ok, good, bad).s()?;
                        self.b.position_at_end(bad);
                        let cur = self.b.build_load(self.repr(t), slot, "").s()?;
                        let b = self.boxed(self.val_of(cur, t, false))?;
                        self.call_rt("mim_index_fail", &[self.basic(b).into(), iv.into()])?;
                        self.b.build_unreachable().s()?;
                        self.b.position_at_end(good);
                        let ep = unsafe { self.b.build_in_bounds_gep(self.repr(t), slot, &[i64t.const_zero(), iv], "").s()? };
                        self.b.build_store(ep, x).s()?;
                    }
                    Some(i) => {
                        let iv = self.expr(i)?;
                        let iv = self.boxed(iv)?;
                        let v = self.expr(value)?;
                        let v = self.boxed(v)?;
                        // (the list in, the list out: the variable's slot is
                        // not handed to the runtime, so it stays a register
                        // and a call after it can still be a tail call, as
                        // `run(k - 1, bump(cells, i))` with `bump` inlined)
                        // (everywhere: a function that does this may be
                        // inlined into one that calls itself last)
                        let cur = self.b.build_load(self.ptr, slot, "").s()?;
                        let out = self.call_rt_ptr("mim_set_index_v", &[cur.into(), self.basic(iv).into(), self.basic(v).into()])?;
                        self.b.build_store(slot, self.basic(out)).s()?;
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
            Stmt::LetTuple { names, value, .. } if self.let_tuple_in_regs(s, names, value)? => {}
            Stmt::LetTuple { names, value, .. } => {
                let v = self.expr(value)?;
                if let V::R(r, rt) = v
                    && let Some(fs) = self.flat(rt)
                    && fs.len() >= names.len()
                {
                    // in registers: each part straight from them
                    for (i, n) in names.iter().enumerate() {
                        if n == "_" {
                            continue;
                        }
                        let t = self.cur_spec.as_ref().and_then(|sp| sp.tuple_lets.get(&(id(s), i)).copied()).unwrap_or(Ty::Dyn);
                        let x = self.b.build_extract_value(r, i as u32, n).s()?;
                        let fv = self.as_ty(self.val_of(x, fs[i], false), t, &format!("`{n}`"))?;
                        let slot = self.entry_alloca(t, n)?;
                        self.b.build_store(slot, self.basic(fv)).s()?;
                        let owned = matches!(fv, V::P(..));
                        self.scopes.last_mut().unwrap().push(Local { name: n.clone(), slot, ty: t, owned, mutable: false, moves: false });
                    }
                    return Ok(());
                }
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
                    let moves = owned && self.tuple_moves.contains(n);
                    self.scopes.last_mut().unwrap().push(Local { name: n.clone(), slot, ty: t, owned, mutable: false, moves });
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
                self.scopes.last_mut().unwrap().push(Local { name: name.clone(), slot, ty: t, owned, mutable: false, moves: false });
            }
        }
        Ok(())
    }

    // ---- expressions ----------------------------------------------------

    /// Compile a lambda without captures as a typed function `(params…) -> ret`.
    /// A lambda as a native function whose captured variables are extra
    /// parameters after its own (used by fused `¨` kernels).
    fn lambda_fn_with(&mut self, e: &Expr, caps: &[(String, Ty)]) -> R<(FunctionValue<'ctx>, Vec<Ty>, Ty)> {
        self.lambda_fn_owning(e, caps, false)
    }

    /// `own_first`: the lambda takes its first argument owned (a loop's
    /// state, handed over each step), moving it on where it is used once or
    /// ends in `{ s with … }` (owned.rs), else releasing it at the end.
    fn lambda_fn_owning(&mut self, e: &Expr, caps: &[(String, Ty)], own_first: bool) -> R<(FunctionValue<'ctx>, Vec<Ty>, Ty)> {
        let ExprKind::Lambda { params, body } = &e.kind else { return Err("expected a lambda".into()) };
        let (pts, rt) = self
            .cur_spec
            .as_ref()
            .and_then(|s| s.lambdas.get(&id(e)).cloned())
            .unwrap_or_else(|| (vec![Ty::Dyn; params.len()], Ty::Dyn));
        let rt = if rt == Ty::Never { Ty::Dyn } else { rt };
        let mut all: Vec<Ty> = pts.clone();
        all.extend(caps.iter().map(|(_, t)| *t));
        self.lambdas += 1;
        let f = self.module.add_function(&format!("_mimic_each_{}", self.lambdas), self.fn_type(&all, rt), Some(Linkage::Internal));
        let names: Vec<String> = params.iter().cloned().chain(caps.iter().map(|(n, _)| n.clone())).collect();
        self.in_new_fn(f, |cg| {
            cg.cur_ret = rt;
            cg.ret_what = "lambda result".into();
            // the kernel already runs in parallel: calls stay native (not the
            // net), each element starting on a fresh native stack
            cg.native_frame = true;
            cg.cur_depth = Some(cg.ctx.i32_type().const_zero());
            let mut locals = Vec::new();
            for (i, (name, pt)) in names.iter().zip(&all).enumerate() {
                let slot = cg.entry_alloca(*pt, name)?;
                cg.b.build_store(slot, f.get_nth_param(i as u32).unwrap()).s()?;
                let owned = own_first && i == 0;
                if owned {
                    let sites = owned::move_site(name, &[body.as_ref()]).map(|s| vec![s]).or_else(|| owned::tail_with_sites_expr(name, body)).unwrap_or_default();
                    cg.move_at.extend(sites);
                }
                locals.push(Local { name: name.clone(), slot, ty: *pt, owned, mutable: false, moves: owned });
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
                locals.push(Local { name: name.clone(), slot, ty: *pt, owned: false, mutable: false, moves: false });
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

    /// `x ▷ evolve(n, step)` as a loop: `step` (a lambda) compiled as a
    /// typed function taking the state and its captured variables.
    fn evolve_loop(&mut self, x: &Expr, n: &Expr, step: &Expr, st: Ty) -> R<V<'ctx>> {
        let ExprKind::Lambda { params, body } = &step.kind else { return Err("internal compiler error: evolve step".into()) };
        let i64t = self.ctx.i64_type();
        let x0 = self.expr(x)?;
        let x0 = self.as_ty(x0, st, "evolve state")?;
        // the loop owns its state (each step's result replaces it)
        let x0 = self.own(x0)?;
        let nv = self.expr(n)?;
        let nv = self.to_i64(nv)?;
        let neg = self.b.build_int_compare(IntPredicate::SLT, nv, i64t.const_zero(), "").s()?;
        self.panic_if(neg, "evolve: the number of steps cannot be negative")?;
        let caps = self.captures(params, body);
        let mut capv = Vec::new();
        for (name, t) in &caps {
            let l = self.lookup(name).ok_or("internal error: captured variable")?;
            let (slot, lt) = (l.slot, l.ty);
            let bv = self.b.build_load(self.repr(lt), slot, name).s()?;
            let v = self.as_ty(self.val_of(bv, lt, false), *t, name)?;
            capv.push(v);
        }
        // a record on the heap is handed to the step, which may change it
        // in place (it holds the only reference); anything else is lent
        let own_state = matches!(st, Ty::Obj(_)) && self.flat(st).is_none() && self.in_place_ok();
        let (f, _, _) = self.lambda_fn_owning(step, &caps, own_state)?;
        let fun = self.cur_fn.unwrap();
        let pre = self.b.get_insert_block().unwrap();
        let head = self.ctx.append_basic_block(fun, "evolve");
        let body_bb = self.ctx.append_basic_block(fun, "evolve_step");
        let done = self.ctx.append_basic_block(fun, "evolve_done");
        self.b.build_unconditional_branch(head).s()?;
        self.b.position_at_end(head);
        let ip = self.b.build_phi(i64t, "i").s()?;
        let xp = self.b.build_phi(self.repr(st), "x").s()?;
        let i = ip.as_basic_value().into_int_value();
        let more = self.b.build_int_compare(IntPredicate::SLT, i, nv, "").s()?;
        self.b.build_conditional_branch(more, body_bb, done).s()?;
        self.b.position_at_end(body_bb);
        let cur = self.val_of(xp.as_basic_value(), st, true);
        let mut argv: Vec<BasicMetadataValueEnum> = vec![self.basic(cur).into()];
        argv.extend(capv.iter().map(|v| BasicMetadataValueEnum::from(self.basic(*v))));
        let r = self.b.build_call(f, &argv, "").s()?.try_as_basic_value().basic().ok_or("evolve: step returns nothing")?;
        let next = self.val_of(r, st, true);
        let next = self.as_ty(next, st, "evolve state")?;
        if !own_state {
            self.drop_tmp(cur)?;
        }
        let i1 = self.b.build_int_add(i, i64t.const_int(1, false), "").s()?;
        let latch = self.b.get_insert_block().unwrap();
        self.b.build_unconditional_branch(head).s()?;
        ip.add_incoming(&[(&i64t.const_zero(), pre), (&i1, latch)]);
        xp.add_incoming(&[(&self.basic(x0), pre), (&self.basic(next), latch)]);
        self.b.position_at_end(done);
        for v in capv {
            self.drop_tmp(v)?;
        }
        Ok(self.val_of(xp.as_basic_value(), st, true))
    }

    /// The state type of `x ▷ evolve/settle(step …)` when the step takes and
    /// gives back x's type (a known one).
    fn state_type(&self, x: &Expr, step: &Expr) -> Option<Ty> {
        let t = self.ty_of(x);
        if matches!(t, Ty::Dyn | Ty::Never | Ty::Unit) {
            return None;
        }
        let (pts, rt) = self.cur_spec.as_ref()?.lambdas.get(&id(step)).cloned()?;
        let p0 = *pts.first()?;
        // x may be the same record with list lengths known (`[x]`): the
        // step's type is the state's, and x converts to it
        // (and a list of known length becomes the step's list of any length)
        let same = p0 == t
            || matches!((p0, t), (Ty::Obj(a), Ty::Obj(b)) if types::obj_tid(a) == types::obj_tid(b))
            || matches!((p0, t), (Ty::Arr(a, 0), Ty::Arr(b, _)) if a == b);
        (same && rt == p0).then_some(p0)
    }

    /// `x ▷ settle(step, done)` whose step keeps the state's type: a loop
    /// here, `done` asked before each step, as the runtime's mim_settle; a
    /// record state is handed to the step owned (as in evolve_loop).
    fn settle_loop(&mut self, x: &Expr, step: &Expr, done: &Expr, st: Ty) -> R<V<'ctx>> {
        let (ExprKind::Lambda { params: sp, body: sb }, ExprKind::Lambda { params: dp, body: db }) = (&step.kind, &done.kind) else {
            return Err("internal compiler error: settle".into());
        };
        let x0 = self.expr(x)?;
        let x0 = self.as_ty(x0, st, "settle state")?;
        let x0 = self.own(x0)?;
        let load_caps = |cg: &mut Self, caps: &[(String, Ty)]| -> R<Vec<V<'ctx>>> {
            let mut out = Vec::new();
            for (name, t) in caps {
                let l = cg.lookup(name).ok_or("internal error: captured variable")?;
                let (slot, lt) = (l.slot, l.ty);
                let bv = cg.b.build_load(cg.repr(lt), slot, name).s()?;
                out.push(cg.as_ty(cg.val_of(bv, lt, false), *t, name)?);
            }
            Ok(out)
        };
        let scaps = self.captures(sp, sb);
        let dcaps = self.captures(dp, db);
        let sv = load_caps(self, &scaps)?;
        let dv = load_caps(self, &dcaps)?;
        let own_state = matches!(st, Ty::Obj(_)) && self.flat(st).is_none() && self.in_place_ok();
        let (sf, _, _) = self.lambda_fn_owning(step, &scaps, own_state)?;
        let (df, _, _) = self.lambda_fn_with(done, &dcaps)?;
        let fun = self.cur_fn.unwrap();
        let pre = self.b.get_insert_block().unwrap();
        let head = self.ctx.append_basic_block(fun, "settle");
        let body_bb = self.ctx.append_basic_block(fun, "settle_step");
        let out_bb = self.ctx.append_basic_block(fun, "settle_done");
        self.b.build_unconditional_branch(head).s()?;
        self.b.position_at_end(head);
        let xp = self.b.build_phi(self.repr(st), "x").s()?;
        let cur = self.val_of(xp.as_basic_value(), st, true);
        let mut dargs: Vec<BasicMetadataValueEnum> = vec![self.basic(cur).into()];
        dargs.extend(dv.iter().map(|v| BasicMetadataValueEnum::from(self.basic(*v))));
        let d = self.b.build_call(df, &dargs, "").s()?.try_as_basic_value().basic().ok_or("settle: done gives nothing")?;
        let stop = self.val_of(d, self.ty_of(db), true);
        let stop = self.truth(stop)?;
        self.b.build_conditional_branch(stop, out_bb, body_bb).s()?;
        self.b.position_at_end(body_bb);
        let mut sargs: Vec<BasicMetadataValueEnum> = vec![self.basic(cur).into()];
        sargs.extend(sv.iter().map(|v| BasicMetadataValueEnum::from(self.basic(*v))));
        let r = self.b.build_call(sf, &sargs, "").s()?.try_as_basic_value().basic().ok_or("settle: step gives nothing")?;
        let next = self.val_of(r, st, true);
        let next = self.as_ty(next, st, "settle state")?;
        if !own_state {
            self.drop_tmp(cur)?;
        }
        let latch = self.b.get_insert_block().unwrap();
        self.b.build_unconditional_branch(head).s()?;
        xp.add_incoming(&[(&self.basic(x0), pre), (&self.basic(next), latch)]);
        self.b.position_at_end(out_bb);
        for v in sv.into_iter().chain(dv) {
            self.drop_tmp(v)?;
        }
        Ok(self.val_of(xp.as_basic_value(), st, true))
    }

    /// `xs ▷ gather(f)` (`trail`: every running value, a new list) with `f`
    /// a typed lambda over the list's numbers, left to right as the
    /// runtime's fold_range: an empty list stops the program (`gather`, as
    /// the runtime does) or gives an empty list (`trail`).
    fn typed_fold(&mut self, trail: bool, xs: &Expr, f: &Expr, el: El) -> R<V<'ctx>> {
        let ExprKind::Lambda { params, body } = &f.kind else { return Err("internal compiler error: fold".into()) };
        let i64t = self.ctx.i64_type();
        let t = Ty::of_el(el);
        let et = self.repr(t);
        let list = self.expr(xs)?;
        let list = self.as_ty(list, Ty::Arr(el, 0), "fold list")?;
        let list = self.own(list)?;
        let (n, data) = self.vec_header(list, el)?;
        let data = data.into_pointer_value();
        let caps = self.captures(params, body);
        let mut capv = Vec::new();
        for (name, ct) in &caps {
            let l = self.lookup(name).ok_or("internal error: captured variable")?;
            let (slot, lt) = (l.slot, l.ty);
            let bv = self.b.build_load(self.repr(lt), slot, name).s()?;
            capv.push(self.as_ty(self.val_of(bv, lt, false), *ct, name)?);
        }
        let (lf, _, _) = self.lambda_fn_with(f, &caps)?;
        let fun = self.cur_fn.unwrap();
        let empty_bb = self.ctx.append_basic_block(fun, "fold_empty");
        let start_bb = self.ctx.append_basic_block(fun, "fold_start");
        let is_empty = self.b.build_int_compare(IntPredicate::EQ, n, i64t.const_zero(), "").s()?;
        self.b.build_conditional_branch(is_empty, empty_bb, start_bb).s()?;
        // an empty list
        self.b.position_at_end(empty_bb);
        let empty_end;
        let empty_v = if trail {
            let v = self.alloc_vec(if el == El::F64 { 1 } else { 0 }, i64t.const_zero())?;
            // (what the loop would have released at its end)
            for c in &capv {
                self.drop_tmp(*c)?;
            }
            self.drop_tmp(list)?;
            empty_end = self.b.get_insert_block().unwrap();
            Some(v)
        } else {
            let m = self.cstring("gather of an empty array: there is no first value to start from")?;
            self.call_rt("mim_panic", &[m.into()])?;
            self.b.build_unreachable().s()?;
            empty_end = empty_bb;
            None
        };
        let done_bb = self.ctx.append_basic_block(fun, "fold_done");
        if empty_v.is_some() {
            self.b.build_unconditional_branch(done_bb).s()?;
        }
        // the first value, then each next one folded in
        self.b.position_at_end(start_bb);
        let x0 = self.b.build_load(et, data, "x0").s()?;
        let out = if trail {
            let o = self.alloc_vec(if el == El::F64 { 1 } else { 0 }, n)?;
            let od = self.data_of(o)?.into_pointer_value();
            self.b.build_store(od, x0).s()?;
            Some((o, od))
        } else {
            None
        };
        let pre = self.b.get_insert_block().unwrap();
        let head = self.ctx.append_basic_block(fun, "fold");
        let step_bb = self.ctx.append_basic_block(fun, "fold_step");
        let end_bb = self.ctx.append_basic_block(fun, "fold_end");
        self.b.build_unconditional_branch(head).s()?;
        self.b.position_at_end(head);
        let ip = self.b.build_phi(i64t, "i").s()?;
        let accp = self.b.build_phi(et, "acc").s()?;
        let i = ip.as_basic_value().into_int_value();
        let more = self.b.build_int_compare(IntPredicate::SLT, i, n, "").s()?;
        self.b.build_conditional_branch(more, step_bb, end_bb).s()?;
        self.b.position_at_end(step_bb);
        let xp = unsafe { self.b.build_in_bounds_gep(et, data, &[i], "").s()? };
        let x = self.b.build_load(et, xp, "x").s()?;
        let mut argv: Vec<BasicMetadataValueEnum> = vec![accp.as_basic_value().into(), x.into()];
        argv.extend(capv.iter().map(|v| BasicMetadataValueEnum::from(self.basic(*v))));
        let next = self.b.build_call(lf, &argv, "").s()?.try_as_basic_value().basic().ok_or("fold: the lambda gives nothing")?;
        if let Some((_, od)) = out {
            let op = unsafe { self.b.build_in_bounds_gep(et, od, &[i], "").s()? };
            self.b.build_store(op, next).s()?;
        }
        let i1 = self.b.build_int_add(i, i64t.const_int(1, false), "").s()?;
        let latch = self.b.get_insert_block().unwrap();
        self.b.build_unconditional_branch(head).s()?;
        ip.add_incoming(&[(&i64t.const_int(1, false), pre), (&i1, latch)]);
        accp.add_incoming(&[(&x0, pre), (&next, latch)]);
        self.b.position_at_end(end_bb);
        for v in capv {
            self.drop_tmp(v)?;
        }
        self.drop_tmp(list)?;
        let full = match out {
            Some((o, _)) => o,
            None => self.val_of(accp.as_basic_value(), t, false),
        };
        let full_end = self.b.get_insert_block().unwrap();
        self.b.build_unconditional_branch(done_bb).s()?;
        self.b.position_at_end(done_bb);
        match empty_v {
            Some(ev) => {
                let phi = self.b.build_phi(self.ptr, "trail").s()?;
                phi.add_incoming(&[(&self.basic(ev), empty_end), (&self.basic(full), full_end)]);
                Ok(V::P(phi.as_basic_value().into_pointer_value(), true))
            }
            None => Ok(full),
        }
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
    /// A function value's descriptor; `mode`: 0 effects in order, 1 pure, 2
    /// effects that may overlap (see `each_mode`).
    fn fn_desc(&mut self, name: &str, code: FunctionValue<'ctx>, arity: usize, mode: u32) -> PointerValue<'ctx> {
        let i32t = self.ctx.i32_type();
        let st = self.ctx.struct_type(&[self.ptr.into(), self.ptr.into(), i32t.into(), i32t.into()], false);
        let nm = self.global_cstr(name);
        let init = st.const_named_struct(&[
            nm.into(),
            self.fn_addr(code).into(),
            i32t.const_int(arity as u64, false).into(),
            i32t.const_int(mode as u64, false).into(),
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

    /// `mim_closure_checked(pure, effects, n, env)`: the same, pure when the
    /// function values in `env` are.
    fn make_closure_checked(&mut self, pure: PointerValue<'ctx>, effects: PointerValue<'ctx>, env: &[V<'ctx>]) -> R<V<'ctx>> {
        let i64t = self.ctx.i64_type();
        let n = env.len();
        let arr_ty = self.ptr.array_type(n as u32);
        let arr = self.entry_alloca_ty(arr_ty.into(), "env")?;
        for (i, v) in env.iter().enumerate() {
            let slot = unsafe { self.b.build_in_bounds_gep(arr_ty, arr, &[i64t.const_zero(), i64t.const_int(i as u64, false)], "").s()? };
            self.b.build_store(slot, self.basic(*v)).s()?;
        }
        self.call_rt_ptr("mim_closure_checked", &[pure.into(), effects.into(), i64t.const_int(n as u64, false).into(), arr.into()])
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
                let p = cg.b.build_load(cg.ptr, at, name).s()?;
                cg.assume_live(p, t)?;
                let v = cg.as_ty(V::P(p.into_pointer_value(), false), t, "lambda argument")?;
                let owned = matches!(v, V::P(_, true));
                let slot = cg.entry_alloca(t, name)?;
                cg.b.build_store(slot, cg.basic(v)).s()?;
                locals.push(Local { name: name.to_string(), slot, ty: t, owned, mutable: false, moves: false });
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
        // `¨` runs in parallel when the function is pure or its effects may overlap
        let mode = self.each_mode(e) as u32;
        let desc = self.fn_desc("lambda", code, params.len(), mode);
        // pure only if the functions it captures are: checked when it is made
        let effects_desc = if mode > 0 && !caps.is_empty() { Some(self.fn_desc("lambda", code, params.len(), 0)) } else { None };
        let mut env = Vec::new();
        for (name, _) in &caps {
            let l = self.lookup(name).ok_or("internal error: captured variable")?;
            let (slot, lt) = (l.slot, l.ty);
            let bv = self.b.build_load(self.repr(lt), slot, name).s()?;
            env.push(self.boxed(self.val_of(bv, lt, false))?);
        }
        let r = match effects_desc {
            Some(other) => self.make_closure_checked(desc, other, &env)?,
            None => self.make_closure(desc, &env)?,
        };
        for v in env {
            self.drop_tmp(v)?;
        }
        Ok(r)
    }

    /// A named function as a value.
    fn fn_value(&mut self, fid: usize) -> R<V<'ctx>> {
        let n = self.p.fns[fid].arity();
        self.spec_value(&(fid, vec![Ty::Dyn; n]))
    }

    /// One specialisation of a function as a value (boxed arguments and result).
    fn spec_value(&mut self, k: &Key) -> R<V<'ctx>> {
        let fid = k.0;
        let key = format!("fn:{fid}:{:?}", k.1);
        let desc = match self.fn_descs.get(&key) {
            Some(d) => *d,
            None => {
                let n = self.p.fns[fid].arity();
                let k: Key = k.clone();
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
                let mode = if self.purity.fns[fid] { 1 } else if self.purity.overlap[fid] { 2 } else { 0 };
                let d = self.fn_desc(&name, code, n, mode);
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
                let d = self.fn_desc(&format!("({op:?})"), code, 2, 1);
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
    /// `xs ¨ f ¨ g ¨ …` with pure functions (named, or lambdas capturing
    /// nothing): `xs ¨ (x => … g(f(x)))`, one pass and no list between the
    /// stages; each element goes through all of them while it is at hand.
    /// A stage that consumes its record (owned.rs) takes the last stage's
    /// result over, so `with` updates it in place.
    fn each_chain(&mut self, e: &Expr, ty: Ty) -> R<Option<V<'ctx>>> {
        if self.screen_rows(ty) || !self.heap_ok() {
            return Ok(None);
        }
        // the stages, outermost first: (the ¨ expression, its function)
        let mut stages: Vec<(&Expr, &Expr)> = Vec::new();
        let mut cur = e;
        while let ExprKind::Prim { prim: Prim::Each, args } = &cur.kind {
            if args.len() != 2 || !self.purity.operand_is_pure(self.p, self.cur_mod, self.cur_fid, &args[1]) {
                break;
            }
            let ok = match &args[1].kind {
                ExprKind::Lambda { params, body } => params.len() == 1 && self.captures(params, body).is_empty(),
                ExprKind::Path(path) => {
                    !(path.len() == 1 && self.lookup(&path[0]).is_some())
                        && matches!(self.p.lookup(self.cur_mod, path), Some(Resolved::Fn(f)) if matches!(self.p.fns[f].kind, FnKind::User(_)))
                        && self.cur_spec.as_ref().is_some_and(|sp| sp.calls.contains_key(&id(cur)))
                }
                _ => false,
            };
            if !ok {
                break;
            }
            stages.push((cur, &args[1]));
            let ExprKind::Prim { args, .. } = &cur.kind else { unreachable!() };
            cur = &args[0];
        }
        if stages.len() < 2 {
            return Ok(None);
        }
        stages.reverse(); // innermost first
        let base = cur;
        // a stage as a boxed-ABI function: (thunk, takes its element over)
        let mut thunks = Vec::new();
        for (i, (se, f)) in stages.iter().enumerate() {
            let t = match &f.kind {
                ExprKind::Lambda { .. } => (self.lambda_thunk(f)?, false),
                _ => {
                    let ExprKind::Path(path) = &f.kind else { unreachable!() };
                    let fid = self.resolve_fn(path)?;
                    let k = self.cur_spec.as_ref().and_then(|sp| sp.calls.get(&id(*se)).cloned()).ok_or("internal error: untyped ¨ stage")?;
                    let kind = if self.plan.entered.contains(&fid) { Kind::Native } else { Kind::Plain };
                    // (after the first stage, every element is one of our own)
                    if i > 0 && self.consumed(&k) == [true] { (self.thunk_with(&k, kind, true)?, true) } else { (self.thunk(&k, kind)?, false) }
                }
            };
            thunks.push(t);
        }
        // numbers in: the first stage on the number itself
        let raw_first = match (fuse::num_el(self.ty_of(base)), &stages[0].1.kind) {
            (Some(el @ (El::I64 | El::F64)), ExprKind::Lambda { .. }) => {
                let et = if el == El::F64 { Ty::F64 } else { Ty::I64 };
                let (lf, pts, rt) = self.lambda_fn(stages[0].1)?;
                (pts.len() == 1 && pts[0] == et).then_some((lf, et, rt, false, el))
            }
            (Some(el @ (El::I64 | El::F64)), ExprKind::Path(path)) => {
                let et = if el == El::F64 { Ty::F64 } else { Ty::I64 };
                let fid = self.resolve_fn(path)?;
                let k = self.cur_spec.as_ref().and_then(|sp| sp.calls.get(&id(stages[0].0)).cloned()).ok_or("internal error: untyped ¨ stage")?;
                if self.spec(&k)?.params == [et] {
                    let kind = if self.plan.entered.contains(&fid) { Kind::Native } else { Kind::Plain };
                    let f = self.request(&k, kind)?;
                    Some((f, et, self.spec(&k)?.ret, kind == Kind::Native, el))
                } else {
                    None
                }
            }
            _ => None,
        };
        let a = self.expr(base)?;
        let a = self.boxed(a)?;
        // a list of our own into a first stage that consumes: its elements
        // are handed over (mim_each_own), so the first `with` is in place too
        let mut own_first = false;
        if raw_first.is_none()
            && matches!(a, V::P(_, true))
            && let ExprKind::Path(path) = &stages[0].1.kind
        {
            let fid = self.resolve_fn(path)?;
            let k = self.cur_spec.as_ref().and_then(|sp| sp.calls.get(&id(stages[0].0)).cloned()).ok_or("internal error: untyped ¨ stage")?;
            if self.consumed(&k) == [true] {
                let kind = if self.plan.entered.contains(&fid) { Kind::Native } else { Kind::Plain };
                thunks[0] = (self.thunk_with(&k, kind, true)?, true);
                own_first = true;
            }
        }
        let first_raw = match raw_first {
            Some((target, et, rt, depth, _)) => Some(self.raw_thunk(target, et, rt, depth)?),
            None => None,
        };
        self.lambdas += 1;
        let pt: BasicMetadataTypeEnum = if first_raw.is_some() { self.ctx.i64_type().into() } else { self.ptr.into() };
        let comp = self.module.add_function(&format!("_mimic_chain_{}", self.lambdas), self.ptr.fn_type(&[pt], false), Some(Linkage::Internal));
        let first = first_raw.unwrap_or(thunks[0].0);
        let rest: Vec<(FunctionValue<'ctx>, bool)> = thunks[1..].to_vec();
        self.in_new_fn(comp, |cg| {
            let x = comp.get_nth_param(0).unwrap();
            let mut v = cg.b.build_call(first, &[x.into()], "").s()?.try_as_basic_value().basic().unwrap().into_pointer_value();
            for (t, takes) in &rest {
                let r = cg.b.build_call(*t, &[v.into()], "").s()?.try_as_basic_value().basic().unwrap().into_pointer_value();
                if !takes {
                    cg.release(v)?; // lent to the stage: the last stage's result ends here
                }
                v = r;
            }
            cg.b.build_return(Some(&v)).s()?;
            Ok(())
        })?;
        let r = match raw_first {
            Some((_, _, _, _, el)) => {
                let dt = if el == El::F64 { 1 } else { 0 };
                self.call_rt_ptr("mim_each_raw", &[self.basic(a).into(), self.fn_addr(comp).into(), self.i32c(dt), self.i32c(1)])?
            }
            None if own_first => {
                let r = self.call_rt_ptr("mim_each_own", &[self.basic(a).into(), self.fn_addr(comp).into(), self.i32c(1)])?;
                return Ok(Some(self.as_ty(r, ty, "primitive result")?)); // (the list was handed over)
            }
            None => self.call_rt_ptr("mim_each", &[self.basic(a).into(), self.fn_addr(comp).into(), self.i32c(1)])?,
        };
        self.drop_tmp(a)?;
        Ok(Some(self.as_ty(r, ty, "primitive result")?))
    }

    /// A list of screen parts (`[ui::Node[M]]`): what `xs ¨ row` makes in
    /// a `view`.
    fn screen_rows(&self, t: Ty) -> bool {
        let Ty::Arr(El::Obj(id), _) = t else { return false };
        let td = &self.p.types[types::obj_tid(id)];
        let m = &self.p.mods[td.module];
        td.name == "Node" && m.name == "ui" && m.package == "std"
    }

    /// `ptr thunk(i64 bits)`: call `target` (parameter `pt`, i64 or f64,
    /// from the bits as they are; `depth`: a native body) and box its result.
    fn raw_thunk(&mut self, target: FunctionValue<'ctx>, pt: Ty, rt: Ty, depth: bool) -> R<FunctionValue<'ctx>> {
        let name = format!("{}$raw", target.get_name().to_str().unwrap());
        if let Some(f) = self.module.get_function(&name) {
            return Ok(f);
        }
        let i64t = self.ctx.i64_type();
        let f = self.module.add_function(&name, self.ptr.fn_type(&[i64t.into()], false), Some(Linkage::Internal));
        self.in_new_fn(f, |cg| {
            let bits = f.get_nth_param(0).unwrap().into_int_value();
            let a: BasicMetadataValueEnum = if pt == Ty::F64 { cg.b.build_bit_cast(bits, cg.ctx.f64_type(), "").s()?.into() } else { bits.into() };
            let mut args = vec![a];
            if depth {
                args.push(cg.ctx.i32_type().const_zero().into()); // a fresh native stack
            }
            let r = cg.b.build_call(target, &args, "").s()?.try_as_basic_value().basic().unwrap();
            let b = cg.boxed(cg.val_of(r, rt, true))?;
            let bv = cg.basic(b);
            cg.b.build_return(Some(&bv)).s()?;
            Ok(())
        })?;
        Ok(f)
    }

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
    fn desc(&self, t: TypeId, variant: Option<usize>) -> PointerValue<'ctx> {
        if let Some(d) = self.descs.borrow().get(&(t, variant)) {
            return *d;
        }
        if t == usize::MAX {
            let i32t = self.ctx.i32_type();
            let i64t = self.ctx.i64_type();
            let st = self.ctx.struct_type(&[self.ptr.into(), self.ptr.into(), i32t.into(), i32t.into(), self.ptr.into(), i64t.into(), i64t.into(), i64t.into()], false);
            let (tn, vn) = (self.global_cstr("tuple"), self.global_cstr(""));
            let z = i64t.const_zero();
            let val = st.const_named_struct(&[tn.into(), vn.into(), i32t.const_zero().into(), i32t.const_zero().into(), self.ptr.const_null().into(), z.into(), z.into(), z.into()]);
            // exported (weak) so the runtime can build tuples: mim_desc_tuple
            let g = self.module.add_global(st, None, "mim_desc_tuple");
            g.set_initializer(&val);
            g.set_constant(true);
            g.set_linkage(Linkage::WeakODR);
            let p = self.gaddr(g);
            self.descs.borrow_mut().insert((t, variant), p);
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
        // numbers stored raw in the object (see raw_field)
        let (mut raw, mut fb, mut bb) = (0u64, 0u64, 0u64);
        for i in 0..n as usize {
            match self.raw_field(t, variant, i) {
                Some(El::F64) => (raw, fb) = (raw | 1 << i, fb | 1 << i),
                Some(El::Bool) => (raw, bb) = (raw | 1 << i, bb | 1 << i),
                Some(_) => raw |= 1 << i,
                None => {}
            }
        }
        let i64t = self.ctx.i64_type();
        let st = self.ctx.struct_type(&[self.ptr.into(), self.ptr.into(), i32t.into(), i32t.into(), self.ptr.into(), i64t.into(), i64t.into(), i64t.into()], false);
        let val = st.const_named_struct(&[
            tname.into(),
            vname.into(),
            i32t.const_int(tag, false).into(),
            i32t.const_int(n, false).into(),
            fields.into(),
            i64t.const_int(raw, false).into(),
            i64t.const_int(fb, false).into(),
            i64t.const_int(bb, false).into(),
        ]);
        // the standard library's types are known to the runtime by name
        // (`mim_desc_Ok`, `mim_desc_Response`), so intrinsics can build them
        // (`mim_desc_Ok` in the prelude, `mim_desc_json_Text` in a std module)
        let std_name = (self.p.mods[td.module].package == "std").then(|| {
            let m = &self.p.mods[td.module].name;
            let prefix = if m == "prelude" { String::new() } else { format!("{m}_") };
            match variant {
                Some(v) => format!("mim_desc_{prefix}{}", td.variants()[v].name),
                None => format!("mim_desc_{prefix}{}", td.name),
            }
        });
        let g = self.module.add_global(st, None, std_name.as_deref().unwrap_or("desc"));
        g.set_initializer(&val);
        g.set_constant(true);
        g.set_linkage(if std_name.is_some() { Linkage::WeakODR } else { Linkage::Private });
        let p = self.gaddr(g);
        self.descs.borrow_mut().insert((t, variant), p);
        p
    }

    /// A static description of a type for the runtime (`mim_schema` in
    /// runtime/mimic_json.c): kind, item count, record descriptor, items.
    /// Kinds: 0 any, 1 int, 2 float, 3 bool, 4 str, 5 list, 7 record,
    /// 8 Option, 9 tuple, 10 other union. Memoised first, so recursive
    /// records refer to themselves.
    fn schema(&mut self, t: Ty) -> PointerValue<'ctx> {
        if let Some(p) = self.schemas.get(&t) {
            return *p;
        }
        let i32t = self.ctx.i32_type();
        let st = self.ctx.struct_type(&[i32t.into(), i32t.into(), self.ptr.into(), self.ptr.into()], false);
        let g = self.module.add_global(st, None, "schema");
        g.set_constant(true);
        g.set_linkage(Linkage::Private);
        let gp = self.gaddr(g);
        self.schemas.insert(t, gp);
        let (kind, desc, items): (u64, PointerValue<'ctx>, Vec<Ty>) = match t {
            Ty::I64 => (1, self.ptr.const_null(), vec![]),
            Ty::F64 => (2, self.ptr.const_null(), vec![]),
            Ty::Bool => (3, self.ptr.const_null(), vec![]),
            Ty::Char | Ty::Arr(El::Char, _) => (4, self.ptr.const_null(), vec![]),
            Ty::Arr(el, _) => (5, self.ptr.const_null(), vec![Ty::of_el(el)]),
            Ty::Tup(id) => (9, self.desc(usize::MAX, None), types::tuple_elems(id)),
            Ty::Obj(id) => {
                let tid = types::obj_tid(id);
                let td = &self.p.types[tid];
                if td.is_record() {
                    let fts: Vec<Ty> = match &td.decl.kind {
                        mimic_syntax::ast::TypeKind::Record(fs) => fs.iter().map(|(_, ft, _)| types::field_type(self.p, tid, t, ft)).collect(),
                        _ => vec![],
                    };
                    (7, self.desc(tid, None), fts)
                } else if td.name == "Option" && self.p.mods[td.module].package == "std" {
                    let arg = types::obj_args(id).first().copied().filter(|a| *a != Ty::Never).unwrap_or(Ty::Dyn);
                    (8, self.ptr.const_null(), vec![arg])
                } else {
                    (10, self.ptr.const_null(), vec![])
                }
            }
            _ => (0, self.ptr.const_null(), vec![]),
        };
        let ptrs: Vec<PointerValue<'ctx>> = items.iter().map(|it| self.schema(*it)).collect();
        let arr = self.ptr.const_array(&ptrs);
        let ag = self.module.add_global(arr.get_type(), None, "schema_items");
        ag.set_initializer(&arr);
        ag.set_constant(true);
        ag.set_linkage(Linkage::Private);
        let val = st.const_named_struct(&[
            i32t.const_int(kind, false).into(),
            i32t.const_int(items.len() as u64, false).into(),
            desc.into(),
            self.gaddr(ag).into(),
        ]);
        g.set_initializer(&val);
        gp
    }

    /// A variant without fields: one immortal value per variant, made once
    /// and cached in a global (no allocation, no counting afterwards).
    fn unit_variant(&mut self, t: TypeId, v: usize) -> R<V<'ctx>> {
        if !self.field_tys(t, v).is_empty() {
            return self.construct(t, Some(v), vec![]);
        }
        let name = format!("unit_variant.{t}.{v}");
        let g = self.module.get_global(&name).unwrap_or_else(|| {
            let g = self.module.add_global(self.ptr, None, &name);
            g.set_linkage(Linkage::Internal);
            g.set_initializer(&self.ptr.const_null());
            g
        });
        let slot = self.gaddr(g);
        let cur = self.b.build_load(self.ptr, slot, "").s()?;
        inkwell::values::BasicValue::as_instruction_value(&cur).unwrap().set_atomic_ordering(inkwell::AtomicOrdering::Acquire).map_err(|e| e.to_string())?;
        let cur = cur.into_pointer_value();
        let f = self.cur_fn.unwrap();
        let have = self.b.build_is_not_null(cur, "").s()?;
        let pre = self.b.get_insert_block().unwrap();
        let make = self.ctx.append_basic_block(f, "variant_make");
        let done = self.ctx.append_basic_block(f, "variant_done");
        self.b.build_conditional_branch(have, done, make).s()?;
        self.b.position_at_end(make);
        let d = self.desc(t, Some(v));
        let made = self.call_rt("mim_lit_variant", &[slot.into(), d.into()])?.unwrap().into_pointer_value();
        let make_end = self.b.get_insert_block().unwrap();
        self.b.build_unconditional_branch(done).s()?;
        self.b.position_at_end(done);
        let phi = self.b.build_phi(self.ptr, "variant").s()?;
        phi.add_incoming(&[(&cur, pre), (&made, make_end)]);
        Ok(V::P(phi.as_basic_value().into_pointer_value(), false))
    }

    fn construct(&mut self, t: TypeId, variant: Option<usize>, vals: Vec<V<'ctx>>) -> R<V<'ctx>> {
        self.construct_in(t, variant, vals)
    }

    /// A record, variant or tuple on the heap. (tuples use t = usize::MAX)
    fn construct_in(&self, t: TypeId, variant: Option<usize>, vals: Vec<V<'ctx>>) -> R<V<'ctx>> {
        let i64t = self.ctx.i64_type();
        // optimised builds: allocate inline and move the fields in (a value
        // owned here becomes the record's; a borrowed one is retained;
        // numbers are stored raw, never boxed first)
        if self.heap_ok() && t != usize::MAX {
            let i32t = self.ctx.i32_type();
            let n = vals.len() as u64;
            let V::P(r, _) = self.alloc_vec(5, i64t.const_int(n, false))? else { unreachable!() }; // MIM_OBJ
            let vt = self.val_ty();
            self.b.build_store(self.b.build_struct_gep(vt, r, 2, "").s()?, i32t.const_zero()).s()?; // rank 0
            let d = self.desc(t, variant);
            self.b.build_store(self.b.build_struct_gep(vt, r, 4, "").s()?, d).s()?;
            let fields = unsafe { self.b.build_in_bounds_gep(vt, r, &[i32t.const_int(1, false)], "fields").s()? };
            for (i, v) in vals.into_iter().enumerate() {
                if let Some(el) = self.raw_field(t, variant, i) {
                    let fp = unsafe { self.b.build_in_bounds_gep(self.ptr, fields, &[i64t.const_int(i as u64, false)], "").s()? };
                    self.store_raw(fp, v, el)?;
                    continue;
                }
                let b = self.boxed(v)?;
                let b = self.own(b)?;
                let fp = unsafe { self.b.build_in_bounds_gep(self.ptr, fields, &[i64t.const_int(i as u64, false)], "").s()? };
                self.b.build_store(fp, self.basic(b)).s()?;
            }
            return Ok(V::P(r, true));
        }
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

    /// Is field `i` of a record (`variant` None) or variant stored as a raw
    /// number (8 bytes, no box)? Fields declared `i64`, `f64` or `bool` are,
    /// in the program's types and the standard library's private ones (its
    /// public types, which the runtime builds too, keep handles; so do
    /// generic fields).
    fn raw_field(&self, t: TypeId, variant: Option<usize>, i: usize) -> Option<El> {
        // (a slot is pointer-sized: 64-bit targets only)
        if t == usize::MAX || i >= 64 || self.size.get_bit_width() != 64 {
            return None;
        }
        let td = &self.p.types[t];
        if self.p.mods[td.module].package == "std" && td.is_pub {
            return None;
        }
        let ty = match variant {
            Some(v) => *self.field_tys(t, v).get(i)?,
            None => match &td.decl.kind {
                mimic_syntax::ast::TypeKind::Record(fs) => types::ann_in(self.p, td.module, &fs.get(i)?.1)?,
                _ => return None,
            },
        };
        match ty {
            Ty::I64 => Some(El::I64),
            Ty::F64 => Some(El::F64),
            Ty::Bool => Some(El::Bool),
            _ => None,
        }
    }

    /// The address of slot `i` of an object.
    fn slot_ptr(&self, p: PointerValue<'ctx>, i: usize) -> R<PointerValue<'ctx>> {
        let (i32t, i64t) = (self.ctx.i32_type(), self.ctx.i64_type());
        let fields = unsafe { self.b.build_in_bounds_gep(self.val_ty(), p, &[i32t.const_int(1, false)], "fields").s()? };
        Ok(unsafe { self.b.build_in_bounds_gep(self.ptr, fields, &[i64t.const_int(i as u64, false)], "").s()? })
    }

    /// A raw number field, read.
    fn load_raw(&self, fp: PointerValue<'ctx>, el: El) -> R<V<'ctx>> {
        let i64t = self.ctx.i64_type();
        Ok(match el {
            El::F64 => V::F(self.b.build_load(self.ctx.f64_type(), fp, "").s()?.into_float_value()),
            El::Bool => {
                let x = self.b.build_load(i64t, fp, "").s()?.into_int_value();
                V::B(self.b.build_int_compare(IntPredicate::NE, x, i64t.const_zero(), "").s()?)
            }
            _ => V::I(self.b.build_load(i64t, fp, "").s()?.into_int_value()),
        })
    }

    /// A value stored as a raw number field.
    fn store_raw(&self, fp: PointerValue<'ctx>, v: V<'ctx>, el: El) -> R<()> {
        let i64t = self.ctx.i64_type();
        match el {
            El::F64 => {
                let x = self.to_f64(v)?;
                self.b.build_store(fp, x).s()?;
            }
            El::Bool => {
                let x = self.truth(v)?;
                self.b.build_store(fp, self.b.build_int_z_extend(x, i64t, "").s()?).s()?;
            }
            _ => {
                let x = self.to_i64(v)?;
                self.b.build_store(fp, x).s()?;
            }
        }
        self.drop_tmp(v)
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
    /// The type a pattern reads a part as: patterns test and take apart
    /// values on the heap, so parts kept in registers elsewhere stay there
    /// (a binding converts its value itself).
    fn pat_ty(&self, t: Ty) -> Ty {
        if self.in_regs(t) { Ty::Dyn } else { t }
    }

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
                    let fv = self.as_ty(fv, self.pat_ty(ft), "field")?;
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
                let tag = self.obj_tag(b)?;
                let eq = self.b.build_int_compare(IntPredicate::EQ, tag, self.ctx.i64_type().const_int(v as u64, false), "").s()?;
                let ok = self.ctx.append_basic_block(f, "variant_ok");
                self.b.build_conditional_branch(eq, ok, fail).s()?;
                self.b.position_at_end(ok);
                let ftys = self.field_tys(t, v);
                for (i, a) in args.iter().enumerate() {
                    if matches!(a, Pattern::Wild(_) | Pattern::Bind(..)) {
                        continue;
                    }
                    let fv = self.obj_field(b, t, v, i)?;
                    let fv = self.as_ty(fv, self.pat_ty(ftys[i]), "field")?;
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

    /// The variant tag of `b` (a record or union value), read inline after
    /// one check that it is one (anything else: the runtime reports it).
    fn obj_tag(&mut self, b: V<'ctx>) -> R<IntValue<'ctx>> {
        let (i32t, i64t) = (self.ctx.i32_type(), self.ctx.i64_type());
        let p = self.basic(b).into_pointer_value();
        let vt = self.val_ty();
        // dtype OBJ (5) and rank 0, as one 64-bit word
        let little = self.module.get_data_layout().as_str().to_string_lossy().starts_with('e');
        let want = if little { 5u64 } else { 5u64 << 32 };
        let kind = self.b.build_load(i64t, self.b.build_struct_gep(vt, p, 1, "").s()?, "").s()?.into_int_value();
        let is_obj = self.b.build_int_compare(IntPredicate::EQ, kind, i64t.const_int(want, false), "").s()?;
        let f = self.cur_fn.unwrap();
        let ok = self.ctx.append_basic_block(f, "obj_ok");
        let bad = self.ctx.append_basic_block(f, "obj_bad");
        let join = self.ctx.append_basic_block(f, "obj_tag");
        self.b.build_conditional_branch(is_obj, ok, bad).s()?;
        self.b.position_at_end(bad);
        let slow = self.call_rt("mim_obj_tag", &[p.into()])?.unwrap().into_int_value();
        let bad_end = self.b.get_insert_block().unwrap();
        self.b.build_unconditional_branch(join).s()?;
        self.b.position_at_end(ok);
        // the descriptor: { type_name, variant, tag (i32), … } (a tuple has
        // none: the runtime path reports it)
        let d = self.b.build_load(self.ptr, self.b.build_struct_gep(vt, p, 4, "").s()?, "desc").s()?.into_pointer_value();
        let has = self.b.build_is_not_null(d, "").s()?;
        let ok2 = self.ctx.append_basic_block(f, "obj_desc");
        self.b.build_conditional_branch(has, ok2, bad).s()?;
        self.b.position_at_end(ok2);
        let dt = self.ctx.struct_type(&[self.ptr.into(), self.ptr.into(), i32t.into()], false);
        let tag = self.b.build_load(i32t, self.b.build_struct_gep(dt, d, 2, "").s()?, "").s()?.into_int_value();
        let tag = self.b.build_int_s_extend(tag, i64t, "").s()?;
        let ok_end = self.b.get_insert_block().unwrap();
        self.b.build_unconditional_branch(join).s()?;
        self.b.position_at_end(join);
        let phi = self.b.build_phi(i64t, "tag").s()?;
        phi.add_incoming(&[(&tag, ok_end), (&slow, bad_end)]);
        Ok(phi.as_basic_value().into_int_value())
    }

    /// Field `i` of `b`, a record or variant whose type has been checked:
    /// read in place (fields follow the header), borrowed while `b` lives.
    fn obj_field(&self, b: V<'ctx>, t: TypeId, v: usize, i: usize) -> R<V<'ctx>> {
        let p = self.basic(b).into_pointer_value();
        let fp = self.slot_ptr(p, i)?;
        if let Some(el) = self.raw_field(t, Some(v), i) {
            return self.load_raw(fp, el);
        }
        Ok(V::P(self.b.build_load(self.ptr, fp, "field").s()?.into_pointer_value(), false))
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
        // part of a scrutinee that outlives the arm: borrowed, like a parameter
        if self.borrow_binds
            && let V::P(_, false) = val
        {
            let v = self.as_ty(val, t, &format!("`{n}`"))?;
            let slot = self.entry_alloca(t, n)?;
            self.b.build_store(slot, self.basic(v)).s()?;
            let owned = matches!(v, V::P(_, true));
            self.scopes.last_mut().unwrap().push(Local { name: n.to_string(), slot, ty: t, owned, mutable: false, moves: false });
            return Ok(());
        }
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
        self.scopes.last_mut().unwrap().push(Local { name: n.to_string(), slot, ty: t, owned, mutable: false, moves: false });
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
                    let fv = self.as_ty(fv, self.pat_ty(ft), "field")?;
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
                        self.scopes.last_mut().unwrap().push(Local { name: name.clone(), slot: lslot, ty: t, owned, mutable: false, moves: false });
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
                    let fv = self.obj_field(b, t, v, i)?;
                    let fv = self.as_ty(fv, self.pat_ty(ftys[i]), "field")?;
                    self.pattern_bind(a, fv)?;
                    self.drop_tmp(fv)?;
                }
                Ok(())
            }
        }
    }

    /// A float `+ - ×` that may fuse with its neighbours into a multiply-add
    /// (unless --strict-math): what C compilers do within an expression.
    pub(crate) fn contracted(&self, v: FloatValue<'ctx>) -> FloatValue<'ctx> {
        use inkwell::values::FastMathFlags as F;
        if self.contract
            && let Some(i) = inkwell::values::BasicValue::as_instruction_value(&v)
        {
            let flags = if self.fast { F::AllowContract | F::AllowReassoc | F::NoSignedZeros | F::AllowReciprocal | F::ApproxFunc } else { F::AllowContract };
            let _ = i.set_fast_math_flags(flags);
        }
        v
    }

    fn scalar_binary(&self, op: BinOp, a: V<'ctx>, b: V<'ctx>) -> R<V<'ctx>> {
        use BinOp::*;
        let (a, b) = (self.widen(a)?, self.widen(b)?);
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
                Add => Ok(V::F(self.contracted(bld.build_float_add(x, y, "").s()?))),
                Sub => Ok(V::F(self.contracted(bld.build_float_sub(x, y, "").s()?))),
                Mul => Ok(V::F(self.contracted(bld.build_float_mul(x, y, "").s()?))),
                Div => Ok(V::F(self.contracted(bld.build_float_div(x, y, "").s()?))),
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
            IntDiv | Mod => {
                let z = bld.build_int_compare(IntPredicate::EQ, y, zero, "").s()?;
                self.panic_if(z, if op == IntDiv { "integer division by zero" } else { "integer modulo by zero" })?;
                // x >= 0 and y > 0 (indices, sizes: nearly always): floored
                // and truncated division agree, and an unsigned divide needs
                // no correction. LLVM drops the test where it can prove it
                // (an index of a fused loop) or hoists it (a divisor the same
                // for every element); else it is one well-predicted branch.
                let bld = &self.b;
                let f = self.cur_fn.unwrap();
                let xp = bld.build_int_compare(IntPredicate::SGE, x, zero, "").s()?;
                let yp = bld.build_int_compare(IntPredicate::SGT, y, zero, "").s()?;
                let pos = bld.build_and(xp, yp, "").s()?;
                let (fast, slow, done) = (self.ctx.append_basic_block(f, "div_pos"), self.ctx.append_basic_block(f, "div_floor"), self.ctx.append_basic_block(f, "div_done"));
                bld.build_conditional_branch(pos, fast, slow).s()?;
                bld.position_at_end(fast);
                let a = if op == IntDiv { bld.build_int_unsigned_div(x, y, "").s()? } else { bld.build_int_unsigned_rem(x, y, "").s()? };
                bld.build_unconditional_branch(done).s()?;
                bld.position_at_end(slow);
                let r = bld.build_int_signed_rem(x, y, "").s()?;
                // floor: when the remainder is non-zero and its sign differs
                // from y's, the quotient steps down and the remainder adds y
                let nz = bld.build_int_compare(IntPredicate::NE, r, zero, "").s()?;
                let sx = bld.build_xor(r, y, "").s()?;
                let neg = bld.build_int_compare(IntPredicate::SLT, sx, zero, "").s()?;
                let adj = bld.build_and(nz, neg, "").s()?;
                let b = if op == IntDiv {
                    let q = bld.build_int_signed_div(x, y, "").s()?;
                    let adj = bld.build_int_z_extend(adj, self.ctx.i64_type(), "").s()?;
                    bld.build_int_sub(q, adj, "").s()?
                } else {
                    let ry = bld.build_int_add(r, y, "").s()?;
                    bld.build_select(adj, ry, r, "").s()?.into_int_value()
                };
                bld.build_unconditional_branch(done).s()?;
                bld.position_at_end(done);
                let phi = bld.build_phi(self.ctx.i64_type(), "").s()?;
                phi.add_incoming(&[(&a, fast), (&b, slow)]);
                Ok(V::I(phi.as_basic_value().into_int_value()))
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

    /// `let (a, b) = (x, y)`, or `= if c { (x, y) } else { (z, w) }`, whose
    /// parts are numbers or truth values: each name gets its value in a
    /// register, and no tuple is made (in a loop over pixels that was an
    /// allocation, a check and a release per pixel). False: not this shape;
    /// nothing was compiled.
    fn let_tuple_in_regs(&mut self, s: &Stmt, names: &[String], value: &Expr) -> R<bool> {
        let tys: Vec<Ty> = (0..names.len()).map(|i| self.cur_spec.as_ref().and_then(|sp| sp.tuple_lets.get(&(id(s), i)).copied()).unwrap_or(Ty::Dyn)).collect();
        if !tys.iter().all(|t| matches!(t, Ty::I64 | Ty::F64 | Ty::Bool)) {
            return Ok(false);
        }
        let parts = |e: &Expr| -> Option<Vec<Expr>> {
            let tail = match &e.kind {
                ExprKind::Tuple(xs) => return Some(xs.clone()),
                ExprKind::Block(b) if b.stmts.is_empty() => b.tail.as_deref()?,
                _ => return None,
            };
            match &tail.kind {
                ExprKind::Tuple(xs) => Some(xs.clone()),
                _ => None,
            }
        };
        let eval = |cg: &mut Self, xs: &[Expr]| -> R<Vec<BasicValueEnum<'ctx>>> {
            let mut out = Vec::new();
            for (x, t) in xs.iter().zip(&tys) {
                let v = cg.expr(x)?;
                let v = cg.as_ty(v, *t, "tuple element")?;
                out.push(cg.basic(v));
            }
            Ok(out)
        };
        let vals: Vec<BasicValueEnum<'ctx>> = match &value.kind {
            ExprKind::Tuple(xs) if xs.len() == names.len() => eval(self, xs)?,
            ExprKind::If { cond, then, els: Some(els) } => {
                let then_e = Expr { kind: ExprKind::Block(then.clone()), span: value.span };
                let (Some(xs), Some(ys)) = (parts(&then_e), parts(els)) else { return Ok(false) };
                if xs.len() != names.len() || ys.len() != names.len() {
                    return Ok(false);
                }
                let c = self.expr(cond)?;
                let c = self.truth(c)?;
                let f = self.cur_fn.unwrap();
                let (tb, eb, done) = (self.ctx.append_basic_block(f, "tup_then"), self.ctx.append_basic_block(f, "tup_else"), self.ctx.append_basic_block(f, "tup_done"));
                self.b.build_conditional_branch(c, tb, eb).s()?;
                self.b.position_at_end(tb);
                let a = eval(self, &xs)?;
                let a_end = self.b.get_insert_block().unwrap();
                self.b.build_unconditional_branch(done).s()?;
                self.b.position_at_end(eb);
                let b = eval(self, &ys)?;
                let b_end = self.b.get_insert_block().unwrap();
                self.b.build_unconditional_branch(done).s()?;
                self.b.position_at_end(done);
                let mut out = Vec::new();
                for (x, y) in a.iter().zip(&b) {
                    let phi = self.b.build_phi(x.get_type(), "").s()?;
                    phi.add_incoming(&[(x, a_end), (y, b_end)]);
                    out.push(phi.as_basic_value());
                }
                out
            }
            _ => return Ok(false),
        };
        for ((n, t), v) in names.iter().zip(&tys).zip(vals) {
            if n == "_" {
                continue;
            }
            let slot = self.entry_alloca(*t, n)?;
            self.b.build_store(slot, v).s()?;
            self.scopes.last_mut().unwrap().push(Local { name: n.clone(), slot, ty: *t, owned: false, mutable: false, moves: false });
        }
        Ok(true)
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

    /// A call of user function `fid` with its arguments already computed,
    /// from the call at `site` (its syntax node: the callee's
    /// specialisation and the version to run are keyed by it).
    pub(crate) fn call_user(&mut self, site: usize, fid: FnId, vals: Vec<V<'ctx>>) -> R<V<'ctx>> {
        let Some(k) = self.cur_spec.as_ref().and_then(|s| s.calls.get(&site).cloned()) else {
            // the call's argument types never became known (it cannot be reached)
            let m = self.cstring("internal error: unreachable call")?;
            self.call_rt("mim_panic", &[m.into()])?;
            return Ok(self.null());
        };
        let (kind, fork_depth) = match self.fj.kinds.remove(&site) {
            Some((kd, d)) => (kd, d),
            None => (self.kind_for(fid), None),
        };
        let f = self.request(&k, kind)?;
        let spec = self.spec(&k)?;
        let path_s = self.p.fn_path(fid).join("::");
        let pnames: Vec<String> = match &self.p.fns[fid].kind {
            FnKind::User(d) => d.params.iter().map(|p| p.name.clone()).collect(),
            _ => vec![],
        };
        let mut argv = Vec::new();
        let mut conv = Vec::new();
        let consumed = self.consumed(&k);
        let converts: Vec<bool> = (0..vals.len()).map(|i| self.cur_spec.as_ref().is_some_and(|s| s.converts.contains(&(site, i)))).collect();
        // (converted copies of lent lists, released after the call)
        let mut copies = Vec::new();
        for (i, v) in vals.into_iter().enumerate() {
            // a list of unknown type for a parameter declared a typed list:
            // converted here (the callee is compiled for the declared type)
            let what = format!("argument `{}` of `{path_s}`", pnames[i]);
            let takes = consumed.get(i).copied().unwrap_or(false);
            let c = if converts[i] && !takes {
                let (c, copy) = self.convert_arg(v, spec.params[i], &what)?;
                copies.extend(copy);
                c
            } else if converts[i] {
                self.convert_slot(v, Ty::Dyn, spec.params[i], &what)?
            } else {
                self.as_ty(v, spec.params[i], &what)?
            };
            if takes {
                let c = self.hand_over(c)?; // the callee takes it
                argv.push(self.basic(c).into());
                continue;
            }
            argv.push(self.basic(c).into());
            conv.push(c);
        }
        if matches!(kind, Kind::Native | Kind::NativeSeq)
            && let Some(me) = self.lazy_deep.clone()
        {
            // too deep: this whole call starts over on the net
            let d = self.cur_depth.ok_or("internal compiler error: native call outside a native frame")?;
            let wasm = self.module.get_triple().as_str().to_string_lossy().starts_with("wasm");
            let limit = self.ctx.i32_type().const_int(if wasm { NATIVE_LIMIT_WASM } else { NATIVE_LIMIT }, false);
            let is_deep = self.b.build_int_compare(IntPredicate::UGT, d, limit, "deep").s()?;
            let fun = self.cur_fn.unwrap();
            let deep_bb = self.ctx.append_basic_block(fun, "restart_deep");
            let go = self.ctx.append_basic_block(fun, "call");
            self.b.build_conditional_branch(is_deep, deep_bb, go).s()?;
            self.b.position_at_end(deep_bb);
            let wrap = self.request(&me, Kind::NetWrap)?;
            let n = fun.count_params() - 1;
            let own: Vec<BasicMetadataValueEnum> = (0..n).map(|i| fun.get_nth_param(i).unwrap().into()).collect();
            self.call_rt("mim_deep_enter", &[])?;
            let r = self.b.build_call(wrap, &own, "").s()?.try_as_basic_value().basic().unwrap();
            self.call_rt("mim_deep_leave", &[])?;
            self.b.build_return(Some(&r)).s()?;
            self.b.position_at_end(go);
        }
        if kind.has_depth() {
            let d1 = match fork_depth {
                Some(d) => d,
                None => {
                    let d = self.cur_depth.ok_or("internal compiler error: native call outside a native frame")?;
                    self.b.build_int_add(d, self.ctx.i32_type().const_int(1, false), "").s()?
                }
            };
            argv.push(d1.into());
        }
        // a variable handed to a parameter the callee takes, at its last
        // use: the callee holds its own reference now (hand_over), so the
        // variable's goes before the call, not after it, and the callee holds
        // the only one (`compressed[i] = root` further down changes the list
        // in place instead of copying it at every level of a `find`)
        for (j, name) in std::mem::take(&mut self.after_args) {
            if consumed.get(j).copied().unwrap_or(false)
                && let Some(slot) = self.lookup(&name).filter(|l| l.owned).map(|l| l.slot)
            {
                // (checked for null: a variable that moved into the call
                // left one, and the release then folds away)
                let cur = self.b.build_load(self.ptr, slot, "").s()?;
                self.release_out(cur.into_pointer_value())?;
                self.b.build_store(slot, self.ptr.const_null()).s()?;
            }
        }
        // a call to itself that is the function's result: every local its
        // arguments do not use is released first (and its slot emptied, so
        // the release at the end of its scope folds away), leaving the call
        // the last thing done: the recursion runs as a loop, deep or not
        if Some(fid) == self.cur_fid
            && self.body_exit.is_none()
            && self.futures.iter().all(|f| f.is_empty())
            && let Some(per_arg) = self.tail_calls.get(&site).cloned()
        {
            // (a variable passed as it is to a parameter the callee takes
            // has been handed its own reference: the variable's goes too,
            // and the callee holds the only one, changing it in place)
            let keep: std::collections::HashSet<String> = per_arg
                .iter()
                .enumerate()
                .filter(|(j, a)| !(a.direct.is_some() && consumed.get(*j).copied().unwrap_or(false)))
                .flat_map(|(_, a)| a.roots.iter().cloned())
                .collect();
            // a kept name that borrows from another local (a `match`
            // binding points into the value matched, which a local of its
            // own holds): what it borrows from is not known here, so
            // nothing is released early (a parameter is the caller's)
            let params: Vec<String> = match &self.p.fns[fid].kind {
                FnKind::User(d) => d.params.iter().map(|p| p.name.clone()).collect(),
                _ => Vec::new(),
            };
            // (a number held in a register borrows nothing)
            let borrows = keep.iter().any(|n| !params.contains(n) && self.lookup(n).is_some_and(|l| !l.owned && self.repr(l.ty).is_pointer_type()));
            let early: Vec<(PointerValue<'ctx>, bool)> =
                if borrows { Vec::new() } else { self.scopes.iter().flatten().filter(|l| l.owned && !keep.contains(&l.name)).map(|l| (l.slot, l.moves)).collect() };
            for (slot, moves) in early {
                let cur = self.b.build_load(self.ptr, slot, "").s()?;
                if moves {
                    self.release(cur.into_pointer_value())?;
                } else {
                    self.release_out(cur.into_pointer_value())?;
                }
                self.b.build_store(slot, self.ptr.const_null()).s()?;
            }
        }
        let r = self.b.build_call(f, &argv, "").s()?.try_as_basic_value().basic().unwrap();
        for c in conv {
            self.drop_tmp(c)?;
        }
        for (p, copied) in copies {
            let fun = self.cur_fn.unwrap();
            let free = self.ctx.append_basic_block(fun, "conv_free");
            let after = self.ctx.append_basic_block(fun, "conv_after");
            self.b.build_conditional_branch(copied, free, after).s()?;
            self.b.position_at_end(free);
            self.release(p)?;
            self.b.build_unconditional_branch(after).s()?;
            self.b.position_at_end(after);
        }
        Ok(self.val_of(r, spec.ret, true))
    }

    pub(crate) fn scalar_unary(&self, op: UnOp, a: V<'ctx>) -> R<V<'ctx>> {
        let i64t = self.ctx.i64_type();
        let a = self.widen(a)?;
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

    /// An f32 or i32 as an f64 or i64 (exactly); anything else as it is.
    pub(crate) fn widen(&self, v: V<'ctx>) -> R<V<'ctx>> {
        Ok(match v {
            V::F32(_) => V::F(self.to_f64(v)?),
            V::I32(_) => V::I(self.to_i64(v)?),
            _ => v,
        })
    }

    /// `op` on scalars where f32 or i32 is involved: the operands made the
    /// type the two make together (types::num_join), the operation done
    /// wide, the result narrowed to `ty` (a comparison stays a truth
    /// value). For + - × ÷ that is exactly f32 (i32) arithmetic.
    fn narrow_binary(&self, op: BinOp, a: V<'ctx>, ta: Ty, b: V<'ctx>, tb: Ty, ty: Ty) -> R<V<'ctx>> {
        let joint = types::num_join(ta, false, tb, false);
        let (a, b) = if matches!(joint, Ty::F32 | Ty::I32) { (self.narrow_to(a, joint, "operand")?, self.narrow_to(b, joint, "operand")?) } else { (a, b) };
        let r = self.scalar_binary(op, a, b)?;
        if matches!(ty, Ty::F32 | Ty::I32) { self.wrap_to(r, ty) } else { self.as_ty(r, ty, "operator result") }
    }

    /// An arithmetic result as an f32 (rounded) or an i32 (wrapped).
    fn wrap_to(&self, v: V<'ctx>, ty: Ty) -> R<V<'ctx>> {
        match (ty, v) {
            (Ty::I32, V::I(x)) => Ok(V::I32(self.b.build_int_truncate(x, self.ctx.i32_type(), "").s()?)),
            _ => self.narrow_to(v, ty, "operator result"),
        }
    }

    /// `v` (of type `from`) moving into a slot of type `to`. A value's type
    /// says how it is stored, so a list of integers (or booleans) going where
    /// a list of floats (or integers) is declared is converted here, once;
    /// a list whose type is not known statically is checked.
    /// A list of unknown type passed for a parameter declared `[i64]` or
    /// `[f64]` (a lent one): passed as it is when it is already that kind
    /// of list, which one load and compare shows (the common case: a call
    /// in a loop pays nothing more); else converted by the runtime. The
    /// second part, when given, is the value passed and whether it is a
    /// converted copy, to release after the call.
    #[allow(clippy::type_complexity)]
    fn convert_arg(&mut self, v: V<'ctx>, to: Ty, what: &str) -> R<(V<'ctx>, Option<(PointerValue<'ctx>, IntValue<'ctx>)>)> {
        let dt: u64 = match to {
            Ty::Arr(El::I64, _) => 0,
            Ty::Arr(El::F64, _) => 1,
            _ => return Ok((self.as_ty(v, to, what)?, None)),
        };
        // (a list in registers, or one this call owns: the simple way)
        let V::P(p, false) = v else { return Ok((self.convert_slot(v, Ty::Dyn, to, what)?, None)) };
        let i64t = self.ctx.i64_type();
        let i32t = self.ctx.i32_type();
        let f = self.cur_fn.ok_or("internal compiler error: an argument outside a function")?;
        let check = self.ctx.append_basic_block(f, "conv_check");
        let slow = self.ctx.append_basic_block(f, "conv_slow");
        let done = self.ctx.append_basic_block(f, "conv_done");
        let null = self.b.build_is_null(p, "").s()?;
        self.b.build_conditional_branch(null, slow, check).s()?;
        // dtype and rank side by side: one 64-bit load and compare
        self.b.position_at_end(check);
        let hdr = self.ctx.struct_type(&[i64t.into(), i32t.into(), i32t.into(), i64t.into(), self.ptr.into()], false);
        let kind_p = self.b.build_struct_gep(hdr, p, 1, "").s()?;
        let kind = self.b.build_load(i64t, kind_p, "kind").s()?.into_int_value();
        let little = self.module.get_data_layout().as_str().to_string_lossy().starts_with('e');
        let want = if little { (1u64 << 32) | dt } else { (dt << 32) | 1 };
        let ok = self.b.build_int_compare(IntPredicate::EQ, kind, i64t.const_int(want, false), "").s()?;
        self.b.build_conditional_branch(ok, done, slow).s()?;
        self.b.position_at_end(slow);
        let r = self.call_rt_ptr("mim_vec_as", &[p.into(), self.i32c(dt as i32)])?;
        let r = self.basic(r).into_pointer_value();
        let slow_end = self.b.get_insert_block().unwrap();
        self.b.build_unconditional_branch(done).s()?;
        self.b.position_at_end(done);
        let ptr = self.b.build_phi(self.ptr, "conv").s()?;
        ptr.add_incoming(&[(&p, check), (&r, slow_end)]);
        let flag = self.b.build_phi(self.ctx.bool_type(), "conv_owned").s()?;
        let (no, yes) = (self.ctx.bool_type().const_zero(), self.ctx.bool_type().const_int(1, false));
        flag.add_incoming(&[(&no, check), (&yes, slow_end)]);
        let ptr = ptr.as_basic_value().into_pointer_value();
        let c = self.as_ty(V::P(ptr, false), to, what)?;
        Ok((c, Some((ptr, flag.as_basic_value().into_int_value()))))
    }

    fn convert_slot(&mut self, v: V<'ctx>, from: Ty, to: Ty, what: &str) -> R<V<'ctx>> {
        let want = match to {
            Ty::Arr(El::F64, _) => Some(1),
            Ty::Arr(El::I64, _) => Some(0),
            _ => None,
        };
        let differs = matches!(
            (from, to),
            (Ty::Arr(El::I64 | El::Bool, _), Ty::Arr(El::F64, _)) | (Ty::Arr(El::Bool, _), Ty::Arr(El::I64, _)) | (Ty::Dyn | Ty::Arr(El::Dyn, _), Ty::Arr(El::F64 | El::I64, _))
        );
        let v = match (want, differs, v) {
            // a list in registers: convert its elements, then store it
            (Some(w), true, V::A(a, _)) => {
                let el = if w == 1 { El::F64 } else { El::I64 };
                let c = self.as_fixed(v, el, a.get_type().len(), what)?;
                self.boxed(c)?
            }
            (Some(w), true, V::P(..)) => {
                let b = self.boxed(v)?;
                let r = self.call_rt_ptr("mim_vec_as", &[self.basic(b).into(), self.i32c(w)])?;
                self.drop_tmp(b)?;
                r
            }
            (_, _, v) => v,
        };
        self.as_ty(v, to, what)
    }

    /// At the top of a function: tell LLVM, with `llvm.assume` bundles, that
    /// a parameter's vector (or a record parameter and its vector fields) can
    /// be read. A borrowed value lives through the whole call, and unlike
    /// parameter attributes these facts survive inlining: header checks in
    /// loops, also in functions inlined into them, can then be hoisted.
    fn assume_params_live(&self, v: BasicValueEnum<'ctx>, t: Ty) -> R<()> {
        let BasicValueEnum::PointerValue(p) = v else { return Ok(()) };
        let (i32t, i64t) = (self.ctx.i32_type(), self.ctx.i64_type());
        let assume = |p: PointerValue<'ctx>, bytes: u64| -> R<()> {
            let f = Intrinsic::find("llvm.assume").ok_or("no llvm.assume")?.get_declaration(&self.module, &[]).ok_or("no llvm.assume")?;
            let bundles = [
                inkwell::values::OperandBundle::create("dereferenceable", &[p.into(), i64t.const_int(bytes, false).into()]),
                inkwell::values::OperandBundle::create("align", &[p.into(), i64t.const_int(8, false).into()]),
            ];
            self.b.build_direct_call_with_operand_bundles(f, &[self.ctx.bool_type().const_int(1, false).into()], &bundles, "").s()?;
            Ok(())
        };
        match t {
            Ty::Arr(El::F64 | El::I64 | El::Bool, 0) => assume(p, self.live_bytes(t))?,
            Ty::Obj(_) if self.flat(t).is_some() => {}
            Ty::Obj(id) => {
                assume(p, self.live_bytes(t))?;
                let td = &self.p.types[types::obj_tid(id)];
                let mimic_syntax::ast::TypeKind::Record(fs) = &td.decl.kind else { return Ok(()) };
                let invariant = self.ctx.get_kind_id("invariant.load");
                let empty = self.ctx.metadata_node(&[]);
                let fields = unsafe { self.b.build_in_bounds_gep(self.val_ty(), p, &[i32t.const_int(1, false)], "fields").s()? };
                for (i, (_, ft, _)) in fs.iter().enumerate() {
                    let fty = types::ann_in(self.p, td.module, ft).unwrap_or(Ty::Dyn);
                    if !matches!(fty, Ty::Arr(El::F64 | El::I64 | El::Bool, 0)) {
                        continue;
                    }
                    let fp = unsafe { self.b.build_in_bounds_gep(self.ptr, fields, &[i64t.const_int(i as u64, false)], "").s()? };
                    let fv = self.b.build_load(self.ptr, fp, "field").s()?;
                    inkwell::values::BasicValue::as_instruction_value(&fv).unwrap().set_metadata(empty, invariant).map_err(|e| e.to_string())?;
                    self.assume_live(fv, fty)?;
                    assume(fv.into_pointer_value(), self.live_bytes(fty))?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// How many bytes at a live value of type `t` can always be read: the
    /// header, and for a record also its fields, which follow the header.
    fn live_bytes(&self, t: Ty) -> u64 {
        let pb = self.size.get_bit_width() as u64 / 8;
        let header = (24 + pb).div_ceil(8) * 8;
        match t {
            Ty::Obj(id) => match &self.p.types[types::obj_tid(id)].decl.kind {
                mimic_syntax::ast::TypeKind::Record(fs) => header + pb * fs.len() as u64,
                _ => header,
            },
            _ => header,
        }
    }

    /// A loaded value of type `t` that is always a live vector or record:
    /// tell LLVM its header can be read anywhere, so checks on it may leave
    /// loops.
    fn assume_live(&self, load: BasicValueEnum<'ctx>, t: Ty) -> R<()> {
        if !matches!(t, Ty::Arr(El::F64 | El::I64 | El::Bool, 0) | Ty::Obj(_)) {
            return Ok(());
        }
        let i = inkwell::values::BasicValue::as_instruction_value(&load).ok_or("not an instruction")?;
        let empty = self.ctx.metadata_node(&[]);
        i.set_metadata(empty, self.ctx.get_kind_id("nonnull")).map_err(|e| e.to_string())?;
        let n = self.ctx.metadata_node(&[self.ctx.i64_type().const_int(self.live_bytes(t), false).into()]);
        i.set_metadata(n, self.ctx.get_kind_id("dereferenceable")).map_err(|e| e.to_string())?;
        // values are 8-byte aligned: with that, header loads may be hoisted
        let a = self.ctx.metadata_node(&[self.ctx.i64_type().const_int(8, false).into()]);
        i.set_metadata(a, self.ctx.get_kind_id("align")).map_err(|e| e.to_string())?;
        Ok(())
    }

    /// `xs[i]` on a vector `q` known to hold exactly `n` elements of `el`:
    /// one check against the constant length, one load.
    fn index_known(&mut self, q: PointerValue<'ctx>, el: El, n: u32, i: IntValue<'ctx>) -> R<V<'ctx>> {
        let (i32t, i64t) = (self.ctx.i32_type(), self.ctx.i64_type());
        if i.get_zero_extended_constant().is_none_or(|k| k >= n as u64) {
            let ok = self.b.build_int_compare(IntPredicate::ULT, i, i64t.const_int(n as u64, false), "").s()?;
            let f = self.cur_fn.unwrap();
            let good = self.ctx.append_basic_block(f, "fk_ok");
            let bad = self.ctx.append_basic_block(f, "fk_bad");
            self.b.build_conditional_branch(ok, good, bad).s()?;
            self.b.position_at_end(bad);
            self.call_rt("mim_index_fail", &[q.into(), i.into()])?;
            self.b.build_unreachable().s()?;
            self.b.position_at_end(good);
        }
        let data = unsafe { self.b.build_in_bounds_gep(self.val_ty(), q, &[i32t.const_int(1, false)], "data").s()? };
        let et: BasicTypeEnum = if el == El::Bool { self.ctx.i8_type().into() } else { self.el_ty(el) };
        let ep = unsafe { self.b.build_in_bounds_gep(et, data, &[i], "").s()? };
        let x = self.b.build_load(et, ep, "").s()?;
        Ok(match el {
            El::F64 => V::F(x.into_float_value()),
            El::Bool => V::B(self.b.build_int_compare(IntPredicate::NE, x.into_int_value(), self.ctx.i8_type().const_zero(), "").s()?),
            _ => V::I(x.into_int_value()),
        })
    }

    /// `xs[i]` on a local list kept in registers (its stack slot `slot`):
    /// checked against the known length, read in place.
    fn index_slot(&mut self, slot: PointerValue<'ctx>, t: Ty, i: IntValue<'ctx>) -> R<V<'ctx>> {
        let (el, n) = fixed(t).ok_or("internal compiler error: not a list in registers")?;
        let i64t = self.ctx.i64_type();
        let at = self.repr(t);
        if i.get_zero_extended_constant().is_none_or(|k| k >= n as u64) {
            let ok = self.b.build_int_compare(IntPredicate::ULT, i, i64t.const_int(n as u64, false), "").s()?;
            let f = self.cur_fn.unwrap();
            let good = self.ctx.append_basic_block(f, "fx_ok");
            let bad = self.ctx.append_basic_block(f, "fx_bad");
            self.b.build_conditional_branch(ok, good, bad).s()?;
            self.b.position_at_end(bad);
            let cur = self.b.build_load(at, slot, "").s()?;
            let bx = self.boxed(self.val_of(cur, t, false))?;
            self.call_rt("mim_index_fail", &[self.basic(bx).into(), i.into()])?;
            self.b.build_unreachable().s()?;
            self.b.position_at_end(good);
        }
        // the whole value, then a select: no indexed access to the slot, so
        // LLVM keeps the local in registers
        let whole = self.b.build_load(at, slot, "").s()?.into_array_value();
        self.select_elem(whole, el, i)
    }

    /// `xs[i]` on a list in registers: a constant index is checked when
    /// compiling (and reads the register); any other is checked against the
    /// known length and reads a stack copy (LLVM keeps it in registers when
    /// it can).
    fn index_fixed(&mut self, a: V<'ctx>, i: IntValue<'ctx>) -> R<V<'ctx>> {
        let V::A(arr, el) = a else { return Err("internal compiler error: not a list in registers".into()) };
        let n = arr.get_type().len();
        let i64t = self.ctx.i64_type();
        if let Some(k) = i.get_zero_extended_constant().filter(|k| *k < n as u64) {
            return self.elem_at(arr, el, k as u32);
        }
        let ok = self.b.build_int_compare(IntPredicate::ULT, i, i64t.const_int(n as u64, false), "").s()?;
        let f = self.cur_fn.unwrap();
        let good = self.ctx.append_basic_block(f, "fx_ok");
        let bad = self.ctx.append_basic_block(f, "fx_bad");
        self.b.build_conditional_branch(ok, good, bad).s()?;
        self.b.position_at_end(bad);
        let bx = self.boxed(a)?;
        self.call_rt("mim_index_fail", &[self.basic(bx).into(), i.into()])?;
        self.b.build_unreachable().s()?;
        self.b.position_at_end(good);
        self.select_elem(arr, el, i)
    }

    /// Element `i` (in bounds) of a list in registers, chosen with compares
    /// and selects: no memory, and it folds to one register once `i` is
    /// known (after inlining and unrolling).
    fn select_elem(&self, arr: inkwell::values::ArrayValue<'ctx>, el: El, i: IntValue<'ctx>) -> R<V<'ctx>> {
        let i64t = self.ctx.i64_type();
        let n = arr.get_type().len();
        let mut x = self.b.build_extract_value(arr, 0, "").s()?;
        for k in 1..n {
            let is_k = self.b.build_int_compare(IntPredicate::EQ, i, i64t.const_int(k as u64, false), "").s()?;
            let y = self.b.build_extract_value(arr, k, "").s()?;
            x = self.b.build_select(is_k, y, x, "").s()?;
        }
        Ok(match el {
            El::F64 => V::F(x.into_float_value()),
            El::Bool => V::B(x.into_int_value()),
            _ => V::I(x.into_int_value()),
        })
    }

    /// `xs[i]` on a numeric vector known by type: read the element straight
    /// from the array. A value's type says how it is stored (see
    /// `convert_slot`), so one check covers it all: a vector (not a matrix) of
    /// that element type, with `i` in bounds. Anything else stops the
    /// program with `mim_index_fail`, which never returns, so code after the
    /// check may assume it held.
    /// In `{ s with f: op(s.f, rest…) }` (op: push, concat or put; s the
    /// base variable; f a list field): (op, rest), when the list can be taken
    /// out of the record. Not when `rest` reads another field taken this way
    /// (it would read a list already changed), nor a field kept in registers.
    /// A chain `op2(op1(s.f, a…), b…)` of such updates: each (op, rest),
    /// innermost first.
    fn steal_chain<'e>(&self, base: &Expr, tid: TypeId, n: &str, x: &'e Expr, fields: &[(String, Expr, mimic_syntax::Span)]) -> Option<Vec<(&'static str, &'e [Expr])>> {
        let mut chain = Vec::new();
        let mut cur = x;
        loop {
            let (op, rest, inner) = self.steal_step(cur)?;
            chain.push((op, rest));
            match &inner.kind {
                ExprKind::Field { .. } => break,
                _ => cur = inner,
            }
        }
        chain.reverse();
        // the innermost reads s.f itself: the usual checks
        let (_, _) = self.steal_of(base, tid, n, self.innermost(x)?, fields)?;
        // and no level reads another field taken out this way (fields that
        // are only assigned are fine: every value is computed first)
        let ExprKind::Path(bp) = &base.kind else { return None };
        let stolen: Vec<&str> = fields
            .iter()
            .filter(|(m, y, _)| m != n && self.innermost(y).is_some_and(|i| matches!(&i.kind, ExprKind::Call { args, .. } if matches!(&args[0].kind, ExprKind::Field { base: b2, name: f2 } if f2 == m && matches!(&b2.kind, ExprKind::Path(p) if p == bp)))))
            .map(|(m, _, _)| m.as_str())
            .collect();
        for (_, rest) in &chain {
            for a in rest.iter() {
                let mut bad = false;
                a.walk(&mut |y| {
                    if let ExprKind::Field { base: b2, name: f2 } = &y.kind
                        && matches!(&b2.kind, ExprKind::Path(p) if p == bp)
                        && stolen.contains(&f2.as_str())
                    {
                        bad = true;
                    }
                });
                if bad {
                    return None;
                }
            }
        }
        Some(chain)
    }

    /// One level of a chain: (op, rest, the list argument).
    fn steal_step<'e>(&self, x: &'e Expr) -> Option<(&'static str, &'e [Expr], &'e Expr)> {
        let ExprKind::Call { callee, args } = &x.kind else { return None };
        let ExprKind::Path(path) = &callee.kind else { return None };
        let Some(Resolved::Fn(fid)) = self.p.lookup(self.cur_mod, path) else { return None };
        let FnKind::Extern { decl, .. } = &self.p.fns[fid].kind else { return None };
        if !decl.intrinsic || self.p.mods[self.p.fns[fid].module].name != "prelude" {
            return None;
        }
        let op: &'static str = match (decl.name.as_str(), args.len()) {
            ("push", 2) => "push",
            ("concat", 2) => "concat",
            ("put", 3) => "put",
            _ => return None,
        };
        Some((op, &args[1..], &args[0]))
    }

    /// The innermost update of a chain (whose list argument is a field).
    fn innermost<'e>(&self, x: &'e Expr) -> Option<&'e Expr> {
        let mut cur = x;
        loop {
            let (_, _, inner) = self.steal_step(cur)?;
            if matches!(inner.kind, ExprKind::Field { .. }) {
                return Some(cur);
            }
            cur = inner;
        }
    }

    fn steal_of<'e>(&self, base: &Expr, tid: TypeId, n: &str, x: &'e Expr, fields: &[(String, Expr, mimic_syntax::Span)]) -> Option<(&'static str, &'e [Expr])> {
        let ExprKind::Path(bp) = &base.kind else { return None };
        let ExprKind::Call { callee, args } = &x.kind else { return None };
        let ExprKind::Path(path) = &callee.kind else { return None };
        let Some(Resolved::Fn(fid)) = self.p.lookup(self.cur_mod, path) else { return None };
        let FnKind::Extern { decl, .. } = &self.p.fns[fid].kind else { return None };
        if !decl.intrinsic || self.p.mods[self.p.fns[fid].module].name != "prelude" {
            return None;
        }
        let op: &'static str = match (decl.name.as_str(), args.len()) {
            ("push", 2) => "push",
            ("concat", 2) => "concat",
            ("put", 3) => "put",
            _ => return None,
        };
        let ExprKind::Field { base: fb, name: fname } = &args[0].kind else { return None };
        if fname != n || !matches!(&fb.kind, ExprKind::Path(p) if p == bp) {
            return None;
        }
        let i = self.p.types[tid].field_index(n)?;
        if self.raw_field(tid, None, i).is_some() {
            return None;
        }
        let (_, ft) = self.record_field(tid, n);
        if fixed(ft).is_some() || !matches!(ft, Ty::Arr(..)) {
            return None;
        }
        // the fields other steals take
        let others: Vec<&str> = fields
            .iter()
            .filter(|(m, y, _)| m != n && matches!(&y.kind, ExprKind::Call { args, .. } if matches!(args.first().map(|a| &a.kind), Some(ExprKind::Field { base: b2, name: f2 }) if f2 == m && matches!(&b2.kind, ExprKind::Path(p) if p == bp))))
            .map(|(m, _, _)| m.as_str())
            .collect();
        let rest = &args[1..];
        let mut clash = false;
        for a in rest {
            a.walk(&mut |y| {
                if let ExprKind::Field { base: b2, name: f2 } = &y.kind
                    && matches!(&b2.kind, ExprKind::Path(p) if p == bp)
                    && others.contains(&f2.as_str())
                {
                    clash = true;
                }
            });
        }
        (!clash).then_some((op, rest))
    }

    /// `sort(e)` where e is a fusible list of numbers of a small length the
    /// program states (`⍳ 9 ¨ f`, up to 64): (e, element type, length).
    fn small_sort_arg<'e>(&self, base: &'e Expr) -> Option<(&'e Expr, El, u32)> {
        let ExprKind::Call { callee, args } = &base.kind else { return None };
        let ExprKind::Path(path) = &callee.kind else { return None };
        let Some(Resolved::Fn(fid)) = self.p.lookup(self.cur_mod, path) else { return None };
        let FnKind::Extern { decl, .. } = &self.p.fns[fid].kind else { return None };
        if !decl.intrinsic || decl.name != "sort" || self.p.mods[self.p.fns[fid].module].name != "prelude" || args.len() != 1 {
            return None;
        }
        let a = &args[0];
        let el = match fuse::num_el(self.ty_of(a)) {
            Some(e @ (El::I64 | El::F64)) => e,
            _ => return None,
        };
        // ⍳ N, or ⍳ N ¨ f, with N written in the program
        let iota = match &a.kind {
            ExprKind::Prim { prim: Prim::Each, args } => &args[0],
            _ => a,
        };
        let ExprKind::Prim { prim: Prim::Iota, args: ia } = &iota.kind else { return None };
        let ExprKind::Int(n) = ia[0].kind else { return None };
        ((1..=64).contains(&n) && self.is_fae(a)).then_some((a, el, n as u32))
    }

    /// `char_code(s)`: the first byte when `s` is text whose first character
    /// is ASCII; else (empty, other characters) `mim_std_char_code`.
    fn char_code_inline(&mut self, s: V<'ctx>) -> R<IntValue<'ctx>> {
        let i64t = self.ctx.i64_type();
        let i32t = self.ctx.i32_type();
        let i8t = self.ctx.i8_type();
        let hdr = self.ctx.struct_type(&[i64t.into(), i32t.into(), i32t.into(), i64t.into(), self.ptr.into()], false);
        let p = self.basic(s).into_pointer_value();
        let kind_p = self.b.build_struct_gep(hdr, p, 1, "").s()?;
        let kind = self.b.build_load(i64t, kind_p, "").s()?.into_int_value();
        let len_p = self.b.build_struct_gep(hdr, p, 3, "").s()?;
        let len = self.b.build_load(i64t, len_p, "").s()?.into_int_value();
        let little = self.module.get_data_layout().as_str().to_string_lossy().starts_with('e');
        let want: u64 = if little { (1u64 << 32) | 3 } else { (3u64 << 32) | 1 };
        let ok_k = self.b.build_int_compare(IntPredicate::EQ, kind, i64t.const_int(want, false), "").s()?;
        let ok_n = self.b.build_int_compare(IntPredicate::SGT, len, i64t.const_zero(), "").s()?;
        let go = self.b.build_and(ok_k, ok_n, "").s()?;
        let f = self.cur_fn.unwrap();
        let read = self.ctx.append_basic_block(f, "cc_read");
        let fast = self.ctx.append_basic_block(f, "cc_fast");
        let slow = self.ctx.append_basic_block(f, "cc_slow");
        let done = self.ctx.append_basic_block(f, "cc_done");
        self.b.build_conditional_branch(go, read, slow).s()?;
        self.b.position_at_end(read);
        let data = unsafe { self.b.build_in_bounds_gep(self.val_ty(), p, &[i32t.const_int(1, false)], "data").s()? };
        let byte = self.b.build_load(i8t, data, "").s()?.into_int_value();
        let ascii = self.b.build_int_compare(IntPredicate::SGE, byte, i8t.const_zero(), "").s()?;
        self.b.build_conditional_branch(ascii, fast, slow).s()?;
        self.b.position_at_end(fast);
        let quick = self.b.build_int_z_extend(byte, i64t, "").s()?;
        self.b.build_unconditional_branch(done).s()?;
        self.b.position_at_end(slow);
        let r = self.call_rt_ptr("mim_std_char_code", &[p.into()])?;
        let full = self.to_i64(r)?;
        self.drop_tmp(r)?;
        let slow_end = self.b.get_insert_block().unwrap();
        self.b.build_unconditional_branch(done).s()?;
        self.b.position_at_end(done);
        let phi = self.b.build_phi(i64t, "").s()?;
        phi.add_incoming(&[(&quick, fast), (&full, slow_end)]);
        Ok(phi.as_basic_value().into_int_value())
    }

    fn index_fast(&mut self, arr: V<'ctx>, i: IntValue<'ctx>, el: El, ty: Ty) -> R<V<'ctx>> {
        let i64t = self.ctx.i64_type();
        let i32t = self.ctx.i32_type();
        let hdr = self.ctx.struct_type(&[i64t.into(), i32t.into(), i32t.into(), i64t.into(), self.ptr.into()], false);
        let p = self.basic(arr).into_pointer_value();
        // an array's header never changes while it is alive (a changed array
        // is a new one): its loads may be hoisted and shared
        let invariant = self.ctx.get_kind_id("invariant.load");
        let empty = self.ctx.metadata_node(&[]);
        let field = |cg: &Self, k: u32, t: BasicTypeEnum<'ctx>| -> R<BasicValueEnum<'ctx>> {
            let fp = cg.b.build_struct_gep(hdr, p, k, "").s()?;
            let v = cg.b.build_load(t, fp, "").s()?;
            inkwell::values::BasicValue::as_instruction_value(&v).unwrap().set_metadata(empty, invariant).map_err(|e| e.to_string())?;
            Ok(v)
        };
        // dtype and rank sit side by side: one 64-bit load and compare checks
        // both (little-endian: dtype in the low half)
        let kind_p = self.b.build_struct_gep(hdr, p, 1, "").s()?;
        let kind = self.b.build_load(i64t, kind_p, "").s()?;
        inkwell::values::BasicValue::as_instruction_value(&kind).unwrap().set_metadata(empty, invariant).map_err(|e| e.to_string())?;
        let kind = kind.into_int_value();
        let len = field(self, 3, i64t.into())?.into_int_value();
        // a value's elements always follow its header (alloc in mimic_rt.c)
        let data = unsafe { self.b.build_in_bounds_gep(self.val_ty(), p, &[i32t.const_int(1, false)], "data").s()? };
        let want: u64 = match el {
            El::F64 => 1,
            El::Bool => 2,
            El::U8 => 8,
            El::F32 => 9,
            El::I32 => 10,
            _ => 0,
        };
        let little = self.module.get_data_layout().as_str().to_string_lossy().starts_with('e');
        let want = if little { (1u64 << 32) | want } else { (want << 32) | 1 };
        let b = &self.b;
        let ok_k = b.build_int_compare(IntPredicate::EQ, kind, i64t.const_int(want, false), "").s()?;
        // one unsigned compare checks 0 <= i < len
        let ok_i = b.build_int_compare(IntPredicate::ULT, i, len, "").s()?;
        let go = b.build_and(ok_k, ok_i, "").s()?;
        let f = self.cur_fn.unwrap();
        let fast = self.ctx.append_basic_block(f, "idx_fast");
        let bad = self.ctx.append_basic_block(f, "idx_bad");
        // the fast path is almost always taken
        let expect = self.module.get_function("llvm.expect.i1").unwrap_or_else(|| {
            let bt = self.ctx.bool_type();
            self.module.add_function("llvm.expect.i1", bt.fn_type(&[bt.into(), bt.into()], false), None)
        });
        let go = self.b.build_call(expect, &[go.into(), self.ctx.bool_type().const_int(1, false).into()], "").s()?.try_as_basic_value().basic().unwrap().into_int_value();
        self.b.build_conditional_branch(go, fast, bad).s()?;
        // anything else stops the program (out of bounds, or a matrix): a
        // value's type says how it is stored, so a [f64] always holds f64s
        self.b.position_at_end(bad);
        self.call_rt("mim_index_fail", &[p.into(), i.into()])?;
        self.b.build_unreachable().s()?;
        // fast: the element itself
        self.b.position_at_end(fast);
        let fv: V<'ctx> = match el {
            El::F64 => {
                let ep = unsafe { self.b.build_in_bounds_gep(self.ctx.f64_type(), data, &[i], "").s()? };
                V::F(self.b.build_load(self.ctx.f64_type(), ep, "").s()?.into_float_value())
            }
            El::Bool => {
                let ep = unsafe { self.b.build_in_bounds_gep(self.ctx.i8_type(), data, &[i], "").s()? };
                let x = self.b.build_load(self.ctx.i8_type(), ep, "").s()?.into_int_value();
                V::B(self.b.build_int_compare(IntPredicate::NE, x, self.ctx.i8_type().const_zero(), "").s()?)
            }
            // a byte: 0 … 255 as an i64
            El::U8 => {
                let ep = unsafe { self.b.build_in_bounds_gep(self.ctx.i8_type(), data, &[i], "").s()? };
                let x = self.b.build_load(self.ctx.i8_type(), ep, "").s()?.into_int_value();
                V::I(self.b.build_int_z_extend(x, i64t, "").s()?)
            }
            El::F32 => {
                let ep = unsafe { self.b.build_in_bounds_gep(self.ctx.f32_type(), data, &[i], "").s()? };
                let x = self.b.build_load(self.ctx.f32_type(), ep, "").s()?.into_float_value();
                V::F(self.b.build_float_ext(x, self.ctx.f64_type(), "").s()?)
            }
            El::I32 => {
                let ep = unsafe { self.b.build_in_bounds_gep(self.ctx.i32_type(), data, &[i], "").s()? };
                let x = self.b.build_load(self.ctx.i32_type(), ep, "").s()?.into_int_value();
                V::I(self.b.build_int_s_extend(x, i64t, "").s()?)
            }
            _ => {
                let ep = unsafe { self.b.build_in_bounds_gep(i64t, data, &[i], "").s()? };
                V::I(self.b.build_load(i64t, ep, "").s()?.into_int_value())
            }
        };
        self.as_ty(fv, ty, "element")
    }

    /// Element `i` of a list of handles (lent): checked to be such a list
    /// and in bounds, else the runtime stops the program with the reason.
    fn index_handle(&mut self, arr: V<'ctx>, i: IntValue<'ctx>) -> R<V<'ctx>> {
        let (i64t, i32t) = (self.ctx.i64_type(), self.ctx.i32_type());
        let vt = self.val_ty();
        let p = self.basic(arr).into_pointer_value();
        let kind = self.b.build_load(i64t, self.b.build_struct_gep(vt, p, 1, "").s()?, "kind").s()?.into_int_value();
        let len = self.b.build_load(i64t, self.b.build_struct_gep(vt, p, 3, "").s()?, "len").s()?.into_int_value();
        let little = self.module.get_data_layout().as_str().to_string_lossy().starts_with('e');
        let want = if little { (1u64 << 32) | 4 } else { (4u64 << 32) | 1 }; // MIM_BOXED, rank 1
        let ok_k = self.b.build_int_compare(IntPredicate::EQ, kind, i64t.const_int(want, false), "").s()?;
        let ok_i = self.b.build_int_compare(IntPredicate::ULT, i, len, "").s()?;
        let go = self.b.build_and(ok_k, ok_i, "").s()?;
        let f = self.cur_fn.unwrap();
        let fast = self.ctx.append_basic_block(f, "idx_fast");
        let bad = self.ctx.append_basic_block(f, "idx_bad");
        self.b.build_conditional_branch(go, fast, bad).s()?;
        self.b.position_at_end(bad);
        self.call_rt("mim_index_fail", &[p.into(), i.into()])?;
        self.b.build_unreachable().s()?;
        self.b.position_at_end(fast);
        let data = unsafe { self.b.build_in_bounds_gep(vt, p, &[i32t.const_int(1, false)], "data").s()? };
        let ep = unsafe { self.b.build_in_bounds_gep(self.ptr, data, &[i], "").s()? };
        Ok(V::P(self.b.build_load(self.ptr, ep, "elem").s()?.into_pointer_value(), false))
    }

    /// An expression's value, in the form its type asks for: a list of known
    /// length in registers (V::A), any other list as a pointer.
    fn expr(&mut self, e: &Expr) -> R<V<'ctx>> {
        self.dbg_at(e.span);
        // computed ahead, in parallel with its siblings (forkjoin.rs)
        if let Some(v) = self.fj.pre.remove(&id(e)) {
            return Ok(v);
        }
        if let Some(calls) = self.fj_site_at(e) {
            self.fork_site(&calls)?;
            let v = self.expr_plain(e);
            self.fj_drop_unused(&calls)?;
            return v;
        }
        self.expr_plain(e)
    }

    fn expr_plain(&mut self, e: &Expr) -> R<V<'ctx>> {
        let v = self.expr_raw(e)?;
        let ty = self.ty_of(e);
        match (self.in_regs(ty), v) {
            (true, V::P(..)) => self.as_ty(v, ty, "value"),
            // (a number written in the program, typed f32 or i32)
            (true, V::F(_) | V::I(_)) if matches!(ty, Ty::F32 | Ty::I32) => self.narrow_to(v, ty, "value"),
            (false, V::A(..) | V::R(..)) => self.boxed(v),
            _ => Ok(v),
        }
    }

    fn expr_raw(&mut self, e: &Expr) -> R<V<'ctx>> {
        let i64t = self.ctx.i64_type();
        let ty = self.ty_of(e);
        // kernel fusion: whole array expressions become one loop
        match &e.kind {
            // `xs ↑ k` / `xs ↓ k` at xs's last use (it moves here): a list
            // held once shrinks in place instead of being copied (a heap's
            // pop, `h ↑ (n - 1)`, copied the whole heap every time)
            ExprKind::Prim { prim: p @ (Prim::Take | Prim::Drop), args }
                if args.len() == 2
                    && self.move_at.contains(&id(&args[0]))
                    && matches!(&args[0].kind, ExprKind::Path(q) if q.len() == 1 && self.lookup(&q[0]).is_some_and(|l| l.owned))
                    && matches!(self.ty_of(&args[0]), Ty::Arr(..))
                    && self.ty_of(&args[1]) == Ty::I64 =>
            {
                let l = self.expr(&args[0])?;
                let l = self.own(self.boxed(l)?)?;
                let k = self.expr(&args[1])?;
                let k = self.to_i64(k)?;
                let drop = self.i32c((*p == Prim::Drop) as i32);
                let r = self.call_rt_ptr("mim_shrink_own", &[self.basic(l).into(), k.into(), drop])?;
                return self.as_ty(r, ty, "take/drop");
            }
            // `≢ xs` of a list: its length, from the header (or known)
            ExprKind::Prim { prim: Prim::Len, args } if matches!(self.ty_of(&args[0]), Ty::Arr(..)) && !self.is_fae(&args[0]) && !self.keep_fusible(Prim::Len, &args[0]) => {
                let v = self.expr(&args[0])?;
                if let V::A(a, _) = v {
                    return self.as_ty(V::I(i64t.const_int(a.get_type().len() as u64, false)), ty, "length");
                }
                let v = self.boxed(v)?;
                let p = self.basic(v).into_pointer_value();
                let n = self.b.build_load(i64t, self.b.build_struct_gep(self.val_ty(), p, 3, "").s()?, "len").s()?.into_int_value();
                self.drop_tmp(v)?;
                return self.as_ty(V::I(n), ty, "length");
            }
            // `(xs × 2) ⌿ (xs > 0)`: counted, then filled; nothing else built
            ExprKind::Prim { prim: Prim::Keep, .. } if self.keep_list_fusible(e) => {
                let r = self.fused_keep_list(e)?;
                return self.as_ty(r, ty, "keep");
            }
            // `+/ (xs ⌿ mask)`: filtered and summed in one pass
            ExprKind::Prim { prim: p @ (Prim::Sum | Prim::Product | Prim::Len), args } if self.keep_fusible(*p, &args[0]) => {
                let r = self.fused_reduce_keep(*p, &args[0])?;
                return self.as_ty(r, ty, "reduction");
            }
            ExprKind::Prim { prim: p @ (Prim::Sum | Prim::Product | Prim::Maximum | Prim::Minimum | Prim::Len), args }
                if self.is_fae(&args[0]) && (*p == Prim::Len || !matches!(fuse::num_el(self.ty_of(&args[0])), Some(El::F32 | El::I32))) =>
            {
                let r = self.fused_reduce(*p, &args[0])?;
                return self.as_ty(r, ty, "reduction");
            }
            // `xs ▷ trail(+ × max min)` over a fusible expression of numbers
            ExprKind::Prim { prim: Prim::Trail, args }
                if args.len() == 2
                    && self.is_fae(&args[0])
                    && matches!(fuse::num_el(self.ty_of(&args[0])), Some(El::I64 | El::F64))
                    && matches!(&args[1].kind, ExprKind::OpRef(BinOp::Add | BinOp::Mul | BinOp::Max | BinOp::Min)) =>
            {
                let ExprKind::OpRef(op) = &args[1].kind else { unreachable!() };
                let prim = match op {
                    BinOp::Add => Prim::Sum,
                    BinOp::Mul => Prim::Product,
                    BinOp::Max => Prim::Maximum,
                    _ => Prim::Minimum,
                };
                return self.fused_trail(prim, &args[0]);
            }
            ExprKind::Path(p) if p.len() == 1 && self.lazies.iter().any(|(n, _)| *n == p[0]) => return self.fused_map(e),
            _ if self.fusible_root(e) => return self.fused_map(e),
            // numbers into records: one fused pass
            ExprKind::Prim { prim: Prim::Each, .. } if self.each_obj_fusible(e) => return self.fused_each_obj(e),
            _ => {}
        }
        match &e.kind {
            // `User` in `json::decode[User](…)`: the type's schema (not a value to release)
            ExprKind::TypeArg(t) => {
                let ty = types::ann_in(self.p, self.cur_mod, t).unwrap_or(Ty::Dyn);
                Ok(V::P(self.schema(ty), false))
            }
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
            // a list of literals (a table of names, of constants): built the
            // first time, then the same immortal list every time after (a
            // change to it copies, as to any list held more than once)
            ExprKind::Array(xs)
                if xs.len() >= 2
                    && !self.building_literal
                    && fixed(ty).is_none()
                    && self.cur_fn.is_some()
                    && xs.iter().all(|x| matches!(x.kind, ExprKind::Int(_) | ExprKind::Float(_) | ExprKind::Bool(_) | ExprKind::Str(_))) =>
            {
                let g = self.module.add_global(self.ptr, None, "litlist");
                g.set_linkage(Linkage::Internal);
                g.set_initializer(&self.ptr.const_null());
                let slot = self.gaddr(g);
                let cached = self.b.build_load(self.ptr, slot, "lit").s()?;
                inkwell::values::BasicValue::as_instruction_value(&cached)
                    .unwrap()
                    .set_atomic_ordering(inkwell::AtomicOrdering::Acquire)
                    .map_err(|e| e.to_string())?;
                let cached = cached.into_pointer_value();
                let f = self.cur_fn.unwrap();
                let pre = self.b.get_insert_block().unwrap();
                let build_bb = self.ctx.append_basic_block(f, "lit_build");
                let done_bb = self.ctx.append_basic_block(f, "lit_done");
                let have = self.b.build_is_not_null(cached, "").s()?;
                self.b.build_conditional_branch(have, done_bb, build_bb).s()?;
                self.b.position_at_end(build_bb);
                self.building_literal = true;
                let built = self.expr_raw(e);
                self.building_literal = false;
                let built = self.boxed(built?)?;
                let built = self.own(built)?;
                let published = self.call_rt_ptr("mim_lit_list", &[slot.into(), self.basic(built).into()])?;
                let V::P(published, _) = published else { unreachable!() };
                let build_end = self.b.get_insert_block().unwrap();
                self.b.build_unconditional_branch(done_bb).s()?;
                self.b.position_at_end(done_bb);
                let phi = self.b.build_phi(self.ptr, "list").s()?;
                phi.add_incoming(&[(&cached, pre), (&published, build_end)]);
                let v = V::P(phi.as_basic_value().into_pointer_value(), false);
                self.as_ty(v, ty, "list")
            }
            ExprKind::Array(xs) if fixed(ty).is_some() => {
                let (el, _) = fixed(ty).unwrap();
                let mut elems = Vec::new();
                for x in xs {
                    elems.push(self.expr(x)?);
                }
                self.fixed_from(el, &elems)
            }
            // a list of records, strings or tuples: built here, each element
            // moved in (a temporary) or retained (one held elsewhere)
            ExprKind::Array(xs) if !xs.is_empty() && matches!(ty, Ty::Arr(El::Obj(_) | El::Str | El::Tup(_), _)) && self.heap_ok() => {
                let i32t = self.ctx.i32_type();
                let V::P(r, _) = self.alloc_vec(4, i64t.const_int(xs.len() as u64, false))? else { unreachable!() }; // MIM_BOXED
                let data = unsafe { self.b.build_in_bounds_gep(self.val_ty(), r, &[i32t.const_int(1, false)], "data").s()? };
                for (i, x) in xs.iter().enumerate() {
                    let v = self.expr(x)?;
                    let v = self.own(self.boxed(v)?)?;
                    let slot = unsafe { self.b.build_in_bounds_gep(self.ptr, data, &[i64t.const_int(i as u64, false)], "").s()? };
                    self.b.build_store(slot, self.basic(v)).s()?;
                }
                Ok(V::P(r, true))
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
                Resolved::Variant(t, v) => self.unit_variant(t, v),
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
                let (slot, lt, mutable, moves) = (l.slot, l.ty, l.mutable, l.owned && l.moves && !l.mutable && self.move_at.contains(&id(e)));
                let bv = self.b.build_load(self.repr(lt), slot, &path[0]).s()?;
                let v = self.val_of(bv, lt, false);
                // its one use: the value moves out (the slot keeps a null)
                if moves && let V::P(p, _) = v {
                    self.b.build_store(slot, self.ptr.const_null()).s()?;
                    return Ok(V::P(p, true));
                }
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
                        matches!(self.p.lookup(self.cur_mod, path), Some(Resolved::Fn(_) | Resolved::Variant(..)))
                    }
                    _ => false,
                };
                if !named {
                    let f = self.expr(callee)?;
                    return self.call_value(f, args, ty);
                }
                let ExprKind::Path(path) = &callee.kind else { return Err("unsupported callee".into()) };
                if let Some(Resolved::Variant(t, v)) = self.p.lookup(self.cur_mod, path) {
                    let ftys = self.field_tys(t, v);
                    let mut vals = Vec::new();
                    for (i, a) in args.iter().enumerate() {
                        let x = self.expr(a)?;
                        vals.push(self.convert_slot(x, self.ty_of(a), ftys[i], "field")?);
                    }
                    return self.construct(t, Some(v), vals);
                }
                let fid = self.resolve_fn(path)?;
                // char_code of text starting with an ASCII character: its
                // first byte, read in place (a call, a type check and a boxed
                // result per character otherwise); other text: the runtime's
                if let FnKind::Extern { decl, .. } = &self.p.fns[fid].kind
                    && decl.intrinsic
                    && decl.name == "char_code"
                    && self.p.mods[self.p.fns[fid].module].name == "prelude"
                    && args.len() == 1
                    && self.ty_of(&args[0]) == Ty::Arr(El::Char, 0)
                {
                    // (of a literal, `char_code("a")`: known now)
                    if let ExprKind::Str(t) = &args[0].kind
                        && let Some(c) = t.chars().next()
                    {
                        return self.as_ty(V::I(self.ctx.i64_type().const_int(c as u64, false)), ty, "result of `char_code`");
                    }
                    let v = self.expr(&args[0])?;
                    let b = self.boxed(v)?;
                    let r = self.char_code_inline(b)?;
                    self.drop_tmp(b)?;
                    return self.as_ty(V::I(r), ty, "result of `char_code`");
                }
                // sort of a list kept in registers (a 3×3 neighbourhood):
                // sorted on the stack, back in registers, no heap list
                if let FnKind::Extern { decl, .. } = &self.p.fns[fid].kind
                    && decl.intrinsic
                    && decl.name == "sort"
                    && self.p.mods[self.p.fns[fid].module].name == "prelude"
                    && args.len() == 1
                    && let Some((el @ (El::I64 | El::F64), n)) = fixed(self.ty_of(&args[0]))
                {
                    let v = self.expr(&args[0])?;
                    let v = self.as_ty(v, self.ty_of(&args[0]), "sort")?;
                    if let V::A(a, _) = v {
                        let at = a.get_type();
                        let buf = self.entry_alloca_ty(at.into(), "sort_buf")?;
                        self.b.build_store(buf, a).s()?;
                        let f = self.rt(if el == El::F64 { "mim_sort_f64_small" } else { "mim_sort_i64_small" });
                        self.b.build_call(f, &[buf.into(), self.ctx.i64_type().const_int(n as u64, false).into()], "").s()?;
                        let sorted = self.b.build_load(at, buf, "").s()?.into_array_value();
                        return self.as_ty(V::A(sorted, el), ty, "sort");
                    }
                    let b = self.boxed(v)?;
                    let r = self.call_rt_ptr("mim_std_sort", &[self.basic(b).into()])?;
                    self.drop_tmp(b)?;
                    return self.as_ty(r, ty, "sort");
                }
                // bytes::from over a fused integer expression: written
                // straight into bytes, in one pass
                if let FnKind::Extern { decl, .. } = &self.p.fns[fid].kind
                    && decl.intrinsic
                    && decl.name == "from"
                    && self.p.mods[self.p.fns[fid].module].name == "bytes"
                    && args.len() == 1
                    && fuse::num_el(self.ty_of(&args[0])) == Some(El::I64)
                    && self.is_fae(&args[0])
                {
                    return self.fused_bytes(&args[0]);
                }
                let mut vals = Vec::new();
                for a in args {
                    vals.push(self.expr(a)?);
                }
                // math:: on numbers: an LLVM intrinsic or a libm call, no boxing
                if let FnKind::Extern { decl, .. } = &self.p.fns[fid].kind
                    && decl.intrinsic
                    && self.p.mods[self.p.fns[fid].module].name == "math"
                    && self.p.mods[self.p.fns[fid].module].package == "std"
                    && decl.name != "mulmod" && decl.name != "powmod" // whole numbers: the runtime's 128-bit products
                    && decl.name != "to_bits32" && decl.name != "from_bits32" // f32 values: the runtime's
                    && vals.iter().all(|v| matches!(v, V::I(_) | V::F(_)))
                {
                    let name = decl.name.clone();
                    // a float's 64 bits as an integer, and back: the same bits
                    if name == "to_bits" {
                        let x = self.to_f64(vals[0])?;
                        let n = self.b.build_bit_cast(x, self.ctx.i64_type(), "").s()?.into_int_value();
                        return self.as_ty(V::I(n), ty, "result of `math::to_bits`");
                    }
                    if name == "from_bits" {
                        let n = self.to_i64(vals[0])?;
                        let x = self.b.build_bit_cast(n, self.ctx.f64_type(), "").s()?.into_float_value();
                        return self.as_ty(V::F(x), ty, "result of `math::from_bits`");
                    }
                    if name == "to_int" {
                        // towards zero, as C's cast; NaN and numbers out of
                        // range stop the program instead of wrapping
                        let x = self.to_f64(vals[0])?;
                        let f64t = self.ctx.f64_type();
                        let lo = self.b.build_float_compare(inkwell::FloatPredicate::OGE, x, f64t.const_float(-9223372036854775808.0), "").s()?;
                        let hi = self.b.build_float_compare(inkwell::FloatPredicate::OLT, x, f64t.const_float(9223372036854775808.0), "").s()?;
                        let fits = self.b.build_and(lo, hi, "").s()?;
                        let bad = self.b.build_not(fits, "").s()?;
                        self.panic_if(bad, "math::to_int: the number is NaN or too large for an i64")?;
                        let n = self.b.build_float_to_signed_int(x, self.ctx.i64_type(), "").s()?;
                        return self.as_ty(V::I(n), ty, "result of `math::to_int`");
                    }
                    let mut fargs: Vec<BasicMetadataValueEnum> = Vec::new();
                    for v in &vals {
                        fargs.push(self.to_f64(*v)?.into());
                    }
                    let llvm = match name.as_str() {
                        "sqrt" => Some("llvm.sqrt"),
                        "sin" => Some("llvm.sin"),
                        "cos" => Some("llvm.cos"),
                        "exp" => Some("llvm.exp"),
                        "ln" => Some("llvm.log"),
                        "log2" => Some("llvm.log2"),
                        "log10" => Some("llvm.log10"),
                        "pow" => Some("llvm.pow"),
                        "floor" => Some("llvm.floor"),
                        "ceil" => Some("llvm.ceil"),
                        "round" => Some("llvm.round"),
                        _ => None,
                    };
                    let f = match llvm {
                        Some(i) => self.intrinsic(i)?,
                        None => {
                            let f64t = self.ctx.f64_type();
                            let ps: Vec<BasicMetadataTypeEnum> = fargs.iter().map(|_| f64t.into()).collect();
                            self.module.get_function(&name).unwrap_or_else(|| self.module.add_function(&name, f64t.fn_type(&ps, false), Some(Linkage::External)))
                        }
                    };
                    let r = self.b.build_call(f, &fargs, "").s()?.try_as_basic_value().basic().unwrap();
                    return self.as_ty(V::F(r.into_float_value()), ty, &format!("result of `math::{name}`"));
                }
                // a runtime intrinsic: Mimic values in, a Mimic value out
                if let FnKind::Extern { decl, .. } = &self.p.fns[fid].kind
                    && decl.intrinsic
                {
                    // `tcp::send` → mim_std_tcp_send; prelude functions → mim_std_<name>
                    let m = &self.p.mods[self.p.fns[fid].module];
                    // `push`, `put` or `concat` of a list this code owns here
                    // (a variable at its last use): handed over to
                    // the runtime's owning version, which changes it in
                    // place when nothing else holds it (else copies it and
                    // releases it, as lending it would)
                    // (a variable at its last use: `push(acc, x)` handed down a
                    // recursion. Not a temporary, `put(put(t, i, x), j, y)`:
                    // changing one in place left it where an earlier step made
                    // it, which measured slower on some machines than the copy)
                    let own_first = m.name == "prelude"
                        && matches!(decl.name.as_str(), "push" | "put" | "concat")
                        && matches!(vals.first(), Some(V::P(_, true)))
                        && (matches!(args.first().map(|a| &a.kind), Some(ExprKind::Path(_)))
                            // (or a list another `push` just gave back:
                            // `push(push(pieces, word), sep)` held nowhere else;
                            // copied, a text built by such a recursion took
                            // time growing with the square of its length)
                            || (decl.name == "push"
                                && matches!(args.first().map(|a| &a.kind), Some(ExprKind::Call { callee, .. })
                                    if matches!(&callee.kind, ExprKind::Path(p) if p.last().is_some_and(|l| l == "push")))));
                    let name = if own_first {
                        format!("mim_take_{}", decl.name)
                    } else if m.name == "prelude" {
                        format!("mim_std_{}", decl.name)
                    } else {
                        format!("mim_std_{}_{}", m.name, decl.name)
                    };
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
                    // (the list handed over is the runtime's now)
                    for b in boxed.into_iter().skip(usize::from(own_first)) {
                        self.drop_tmp(b)?;
                    }
                    return self.as_ty(V::P(r.into_pointer_value(), true), ty, &format!("result of `{}`", decl.name));
                }
                if let FnKind::Extern { decl, .. } = &self.p.fns[fid].kind {
                    let decl = decl.clone();
                    let f = self.extern_fn(&decl);
                    // C reads lists through a pointer: lists in registers go
                    // to the heap first (released below with the others).
                    // C may fill such a buffer: a local list passed this way
                    // is read back after the call
                    let mut write_back = Vec::new();
                    let mut boxed_vals = Vec::new();
                    for (i, v) in vals.into_iter().enumerate() {
                        if let V::A(a, el) = v {
                            let b = self.boxed(v)?;
                            if let Some(ExprKind::Path(path)) = args.get(i).map(|a| &a.kind)
                                && path.len() == 1
                                && let Some(l) = self.lookup(&path[0])
                            {
                                write_back.push((l.slot, b, el, a.get_type().len()));
                            }
                            boxed_vals.push(b);
                        } else {
                            boxed_vals.push(v);
                        }
                    }
                    let vals = boxed_vals;
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
                            (Ty::Arr(mimic_check::types::El::Char, _), V::P(p, _)) => self.call_rt("mim_cstr", &[(*p).into()])?.unwrap().into(),
                            (Ty::Arr(_, _), V::P(p, _)) => self.call_rt("mim_dataptr", &[(*p).into()])?.unwrap().into(),
                            _ => self.to_i64(*v)?.into(),
                        });
                    }
                    let r = self.b.build_call(f, &argv, "").s()?;
                    for (slot, b, el, n) in write_back {
                        let V::P(q, _) = b else { continue };
                        let back = self.load_fixed(q, el, n)?;
                        self.b.build_store(slot, self.basic(back)).s()?;
                    }
                    let out = match (&decl.ret, r.try_as_basic_value().basic()) {
                        (Some(t), Some(rv)) => self.c_to_mimic(rv, t)?,
                        _ => self.null(),
                    };
                    for v in vals {
                        self.drop_tmp(v)?;
                    }
                    return Ok(out);
                }
                // (a variable whose last use is this call, also read by
                // another argument: `find(parent, parent[i])`)
                let mut after = Vec::new();
                if let Some(body) = self.cur_fid.and_then(|f| match &self.p.fns[f].kind {
                    FnKind::User(d) => Some(&d.body),
                    _ => None,
                }) {
                    for (j, a) in args.iter().enumerate() {
                        if let ExprKind::Path(p) = &a.kind
                            && p.len() == 1
                            // (not one that moves into this argument itself:
                            // its slot is empty already)
                            && !self.move_at.contains(&id(a))
                            && self.lookup(&p[0]).is_some_and(|l| l.owned)
                            && owned::last_in_call(&p[0], body, id(e), id(a))
                        {
                            after.push((j, p[0].clone()));
                        }
                    }
                }
                self.after_args = after;
                self.call_user(id(e), fid, vals)
            }
            // an element of a list in registers
            ExprKind::Index { base, index } if fixed(self.ty_of(base)).is_some() && self.ty_of(index) == Ty::I64 && ty.is_unboxed() => {
                // a local (or parameter): read its stack slot, no copy
                if let ExprKind::Path(path) = &base.kind
                    && let Some((slot, lt)) = (path.len() == 1).then(|| self.lookup(&path[0]).map(|l| (l.slot, l.ty))).flatten()
                        && lt == self.ty_of(base) {
                            let i = self.expr(index)?;
                            let i = self.to_i64(i)?;
                            let r = self.index_slot(slot, lt, i)?;
                            return self.as_ty(r, ty, "element");
                        }
                // a list field of a shaped record: index the record's vector
                // in place (its length is known: one check, one load)
                if let ExprKind::Field { base: rec, name } = &base.kind
                    && let Ty::Obj(t) = self.ty_of(rec)
                {
                    let (el, n) = fixed(self.ty_of(base)).unwrap();
                    let rb = self.expr(rec)?;
                    if let V::R(..) = rb {
                        // a flat record: the field is in registers already
                        let fv = self.expr(base)?;
                        let i = self.expr(index)?;
                        let i = self.to_i64(i)?;
                        let r = self.index_fixed(fv, i)?;
                        return self.as_ty(r, ty, "element");
                    }
                    let rb = self.boxed(rb)?;
                    let fv = self.field_ptr(rb, t, name, self.ty_of(base))?;
                    let i = self.expr(index)?;
                    let i = self.to_i64(i)?;
                    let r = self.index_known(fv, el, n, i)?;
                    self.drop_tmp(rb)?;
                    return self.as_ty(r, ty, "element");
                }
                let b = self.expr(base)?;
                let i = self.expr(index)?;
                let i = self.to_i64(i)?;
                let r = self.index_fixed(b, i)?;
                self.as_ty(r, ty, "element")
            }
            // `sort(⍳ 9 ¨ f)[4]` (a median): the few elements on the stack,
            // sorted there, one read: no list is allocated
            ExprKind::Index { base, index }
                if self.ty_of(index) == Ty::I64
                    && ty.is_unboxed()
                    && let Some((arg, el, len)) = self.small_sort_arg(base) =>
            {
                let buf = self.fused_to_stack(arg, el, len)?;
                let f = self.rt(if el == El::F64 { "mim_sort_f64_small" } else { "mim_sort_i64_small" });
                let i64t = self.ctx.i64_type();
                self.b.build_call(f, &[buf.into(), i64t.const_int(len as u64, false).into()], "").s()?;
                let i = self.expr(index)?;
                let i = self.to_i64(i)?;
                let bad = self.b.build_int_compare(IntPredicate::UGE, i, i64t.const_int(len as u64, false), "").s()?;
                self.panic_if(bad, "index out of bounds of a sorted list")?;
                let et = self.el_ty(el);
                let ep = unsafe { self.b.build_in_bounds_gep(et, buf, &[i], "").s()? };
                let x = self.b.build_load(et, ep, "").s()?;
                let v = if el == El::F64 { V::F(x.into_float_value()) } else { V::I(x.into_int_value()) };
                self.as_ty(v, ty, "element")
            }
            // (bytes too: an element of a [u8] reads as an i64)
            ExprKind::Index { base, index }
                if fuse::index_el(self.ty_of(base)).is_some() && self.ty_of(index) == Ty::I64 && ty.is_unboxed() =>
            {
                let b = self.expr(base)?;
                let b = self.boxed(b)?;
                let i = self.expr(index)?;
                let i = self.to_i64(i)?;
                let r = self.index_fast(b, i, fuse::index_el(self.ty_of(base)).unwrap(), ty)?;
                self.drop_tmp(b)?;
                Ok(r)
            }
            // an element of a list of records, strings or tuples: one bounds
            // check and a load (lent while the list is; else a reference)
            ExprKind::Index { base, index }
                if matches!(self.ty_of(base), Ty::Arr(El::Obj(_) | El::Str | El::Tup(_) | El::Rows(_), _)) && self.ty_of(index) == Ty::I64 && self.heap_ok() =>
            {
                let b = self.expr(base)?;
                let b = self.boxed(b)?;
                let i = self.expr(index)?;
                let i = self.to_i64(i)?;
                let e = self.index_handle(b, i)?;
                let e = if matches!(b, V::P(_, true)) { self.own(e)? } else { e };
                self.drop_tmp(b)?;
                self.as_ty(e, ty, "element")
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
                if Self::is_unboxed(&a) && matches!(ty, Ty::F32 | Ty::I32) {
                    let r = self.scalar_unary(*op, a)?;
                    return self.wrap_to(r, ty);
                }
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
                    && self.ty_of(lhs) == Ty::Arr(El::Char, 0)
                    && self.ty_of(rhs) == Ty::Arr(El::Char, 0) =>
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
            // `a and b`, `a or b` on truth values: b only when it decides
            // (`n > 0 and xs[0] == "x"` never reads xs[0] when n is 0)
            ExprKind::Binary { op: op @ (BinOp::And | BinOp::Or), lhs, rhs } if self.ty_of(lhs) == Ty::Bool && self.ty_of(rhs) == Ty::Bool => {
                let a = self.expr(lhs)?;
                let a = self.truth(a)?;
                let f = self.cur_fn.ok_or("internal compiler error: `and` outside a function")?;
                let pre = self.b.get_insert_block().unwrap();
                let more = self.ctx.append_basic_block(f, if *op == BinOp::And { "and_rhs" } else { "or_rhs" });
                let done = self.ctx.append_basic_block(f, "logic_done");
                if *op == BinOp::And {
                    self.b.build_conditional_branch(a, more, done).s()?;
                } else {
                    self.b.build_conditional_branch(a, done, more).s()?;
                }
                self.b.position_at_end(more);
                let b = self.expr(rhs)?;
                let b = self.truth(b)?;
                let more_end = self.b.get_insert_block().unwrap();
                self.b.build_unconditional_branch(done).s()?;
                self.b.position_at_end(done);
                // (from the left side alone its value decides: false for
                // `and`, true for `or`)
                let phi = self.b.build_phi(self.ctx.bool_type(), "logic").s()?;
                phi.add_incoming(&[(&a, pre), (&b, more_end)]);
                self.as_ty(V::B(phi.as_basic_value().into_int_value()), ty, "operator result")
            }
            // the same when a side's type is known only when it runs: the
            // right side runs unless the left is a single truth value that
            // decides; otherwise both go to the generic operator (lists:
            // element by element)
            ExprKind::Binary { op: op @ (BinOp::And | BinOp::Or), lhs, rhs }
                if matches!(self.ty_of(lhs), Ty::Bool | Ty::Dyn) && matches!(self.ty_of(rhs), Ty::Bool | Ty::Dyn) =>
            {
                let is_or = *op == BinOp::Or;
                let a = self.expr(lhs)?;
                let decides = match a {
                    V::B(b) if is_or => b,
                    V::B(b) => self.b.build_not(b, "").s()?,
                    _ => {
                        let a = self.boxed(a)?;
                        let d = self.call_rt("mim_decides", &[self.basic(a).into(), self.ctx.i64_type().const_int(is_or as u64, false).into()])?.unwrap().into_int_value();
                        self.b.build_int_compare(IntPredicate::NE, d, self.ctx.i64_type().const_zero(), "").s()?
                    }
                };
                let f = self.cur_fn.ok_or("internal compiler error: `and` outside a function")?;
                let short = self.ctx.append_basic_block(f, "logic_short");
                let more = self.ctx.append_basic_block(f, if is_or { "or_rhs" } else { "and_rhs" });
                let done = self.ctx.append_basic_block(f, "logic_done");
                self.b.build_conditional_branch(decides, short, more).s()?;
                self.b.position_at_end(short);
                let sv = self.own(self.boxed(V::B(self.ctx.bool_type().const_int(is_or as u64, false)))?)?;
                self.drop_tmp(a)?;
                let short_end = self.b.get_insert_block().unwrap();
                self.b.build_unconditional_branch(done).s()?;
                self.b.position_at_end(more);
                let b = self.expr(rhs)?;
                let ab = self.boxed(a)?;
                let bb = self.boxed(b)?;
                let r = self.call_rt_ptr("mim_binop", &[self.i32c(*op as i32), self.basic(ab).into(), self.basic(bb).into()])?;
                self.drop_tmp(ab)?;
                self.drop_tmp(bb)?;
                let r = self.own(r)?;
                let more_end = self.b.get_insert_block().unwrap();
                self.b.build_unconditional_branch(done).s()?;
                self.b.position_at_end(done);
                let phi = self.b.build_phi(self.ptr, "logic").s()?;
                phi.add_incoming(&[(&self.basic(sv), short_end), (&self.basic(r), more_end)]);
                self.as_ty(V::P(phi.as_basic_value().into_pointer_value(), true), ty, "operator result")
            }
            ExprKind::Binary { op, lhs, rhs } => {
                let a = self.expr(lhs)?;
                let b = self.expr(rhs)?;
                let (ta, tb) = (self.ty_of(lhs), self.ty_of(rhs));
                let small = |t: Ty| matches!(t, Ty::F32 | Ty::I32);
                if (small(ta) || small(tb) || small(ty)) && Self::is_unboxed(&a) && Self::is_unboxed(&b) && (ty.is_unboxed() || small(ty)) {
                    return self.narrow_binary(*op, a, ta, b, tb, ty);
                }
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
            // x ▷ evolve(n, s => …) whose step keeps the state's type: a loop
            // here, calling the step as a typed function (the state stays in
            // registers when its type allows; no boxing per step)
            ExprKind::Prim { prim: Prim::Evolve, args }
                if matches!(&args[2].kind, ExprKind::Lambda { params, .. } if params.len() == 1)
                    && let Some(st) = self.state_type(&args[0], &args[2]) =>
            {
                let r = self.evolve_loop(&args[0], &args[1], &args[2], st)?;
                self.as_ty(r, ty, "evolve result")
            }
            // (the step's own types decide: the state's type in and out; the
            // expression may still be typed loosely while inference settled)
            ExprKind::Prim { prim: Prim::Settle, args }
                if matches!(&args[1].kind, ExprKind::Lambda { params, .. } if params.len() == 1)
                    && matches!(&args[2].kind, ExprKind::Lambda { params, .. } if params.len() == 1)
                    && let Some(st) = self.state_type(&args[0], &args[1]) =>
            {
                let r = self.settle_loop(&args[0], &args[1], &args[2], st)?;
                self.as_ty(r, ty, "settle result")
            }
            // xs ▷ gather((a, b) => …) / trail(…) over a list of numbers, the
            // lambda typed (number, number) → number: a loop here, left to
            // right, calling the lambda as a typed function (nothing boxed)
            ExprKind::Prim { prim: prim @ (Prim::Gather | Prim::Trail), args }
                if let ExprKind::Lambda { params, .. } = &args[1].kind
                    && params.len() == 2
                    && let Ty::Arr(el @ (El::I64 | El::F64), 0) = self.ty_of(&args[0])
                    && self.cur_spec.as_ref().and_then(|s| s.lambdas.get(&id(&args[1]))).is_some_and(|(pts, rt)| *pts == [Ty::of_el(el), Ty::of_el(el)] && *rt == Ty::of_el(el)) =>
            {
                let r = self.typed_fold(*prim == Prim::Trail, &args[0], &args[1], el)?;
                self.as_ty(r, ty, "fold result")
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
                // `xs ¨ f ¨ g ¨ …`: one pass, each element through every stage
                if *prim == Prim::Each
                    && let Some(r) = self.each_chain(e, ty)?
                {
                    return Ok(r);
                }
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
                        // rows of a screen from a lambda that captures: kept
                        // while the captures stay the same (mim_each_fn_memo)
                        if self.screen_rows(ty) && self.purity.operand_is_pure(self.p, self.cur_mod, self.cur_fid, &args[1]) {
                            let g = self.module.add_global(self.ptr, None, "screen_rows");
                            g.set_linkage(Linkage::Internal);
                            g.set_initializer(&self.ptr.const_null());
                            let r = self.call_rt_ptr("mim_each_fn_memo", &[self.basic(a).into(), self.basic(f).into(), self.gaddr(g).into()])?;
                            self.drop_tmp(f)?;
                            self.drop_tmp(a)?;
                            return self.as_ty(r, ty, "primitive result");
                        }
                        let r = self.call_rt_ptr("mim_each_fn", &[self.basic(a).into(), self.basic(f).into()])?;
                        self.drop_tmp(f)?;
                        self.drop_tmp(a)?;
                        return self.as_ty(r, ty, "primitive result");
                    }
                    // numbers in, the function on a number: no box per element
                    if let Some(el @ (El::I64 | El::F64)) = fuse::num_el(self.ty_of(&args[0])) {
                        let et = if el == El::F64 { Ty::F64 } else { Ty::I64 };
                        let typed = match &args[1].kind {
                            ExprKind::Lambda { .. } => {
                                let (lf, pts, rt) = self.lambda_fn(&args[1])?;
                                (pts.len() == 1 && pts[0] == et).then_some((lf, rt, false))
                            }
                            ExprKind::Path(path) => {
                                let fid = self.resolve_fn(path)?;
                                match self.cur_spec.as_ref().and_then(|s| s.calls.get(&id(e)).cloned()) {
                                    Some(k) if matches!(self.p.fns[fid].kind, FnKind::User(_)) && self.spec(&k)?.params == [et] => {
                                        let kind = if self.plan.entered.contains(&fid) { Kind::Native } else { Kind::Plain };
                                        let f = self.request(&k, kind)?;
                                        Some((f, self.spec(&k)?.ret, kind == Kind::Native))
                                    }
                                    _ => None,
                                }
                            }
                            _ => None,
                        };
                        if let Some((target, rt, depth)) = typed {
                            let t = self.raw_thunk(target, et, rt, depth)?;
                            let pure = self.each_mode(&args[1]);
                            let dt = if el == El::F64 { 1 } else { 0 };
                            let r = self.call_rt_ptr("mim_each_raw", &[self.basic(a).into(), self.fn_addr(t).into(), self.i32c(dt), self.i32c(pure)])?;
                            self.drop_tmp(a)?;
                            return self.as_ty(r, ty, "primitive result");
                        }
                    }
                    // rows of a screen: kept from the last run (mim_each_memo)
                    let memo = self.screen_rows(ty) && self.purity.operand_is_pure(self.p, self.cur_mod, self.cur_fid, &args[1]);
                    let f = match &args[1].kind {
                        ExprKind::Lambda { .. } => self.lambda_thunk(&args[1])?,
                        _ => {
                            let ExprKind::Path(path) = &args[1].kind else { return Err("expected a function".into()) };
                            let fid = self.resolve_fn(path)?;
                            let k = self.cur_spec.as_ref().and_then(|s| s.calls.get(&id(e)).cloned()).ok_or("internal error: untyped ¨ callback")?;
                            // ¨ is already parallel across elements: use the native body
                            let kind = if self.plan.entered.contains(&fid) { Kind::Native } else { Kind::Plain };
                            // a list of our own to a function that consumes its
                            // element: the elements are handed over (in place)
                            if matches!(a, V::P(_, true)) && self.consumed(&k) == [true] {
                                let f = self.thunk_with(&k, kind, true)?;
                                let pure = self.each_mode(&args[1]);
                                let r = self.call_rt_ptr("mim_each_own", &[self.basic(a).into(), self.fn_addr(f).into(), self.i32c(pure)])?;
                                return self.as_ty(r, ty, "primitive result");
                            }
                            self.thunk(&k, kind)?
                        }
                    };
                    let pure = self.each_mode(&args[1]);
                    if memo {
                        // this call site's kept rows (released at exit)
                        let g = self.module.add_global(self.ptr, None, "screen_rows");
                        g.set_linkage(Linkage::Internal);
                        g.set_initializer(&self.ptr.const_null());
                        let r = self.call_rt_ptr("mim_each_memo", &[self.basic(a).into(), self.fn_addr(f).into(), self.i32c(pure), self.gaddr(g).into()])?;
                        self.drop_tmp(a)?;
                        return self.as_ty(r, ty, "primitive result");
                    }
                    self.call_rt_ptr("mim_each", &[self.basic(a).into(), self.fn_addr(f).into(), self.i32c(pure)])?
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
                // numbers and truth values: in registers
                if let Some(fs) = self.flat(ty)
                    && fs.len() == vals.len()
                {
                    let mut out = self.repr(ty).into_struct_type().get_undef();
                    for (k, (v, ft)) in vals.into_iter().zip(&fs).enumerate() {
                        let v = self.as_ty(v, *ft, "tuple element")?;
                        out = self.b.build_insert_value(out, self.basic(v), k as u32, "").s()?.into_struct_value();
                    }
                    return Ok(V::R(out, ty));
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
                // a flat record's fields have their own (known-length) types
                let ftys = self.flat(ty).unwrap_or(ftys);
                // evaluate in source order, store in declaration order
                let mut slots: Vec<Option<V>> = vec![None; order.len()];
                for (n, x, _) in fields {
                    let i = order.iter().position(|o| o == n).ok_or("unknown field")?;
                    let v = self.expr(x)?;
                    slots[i] = Some(self.convert_slot(v, self.ty_of(x), ftys[i], &format!("field `{n}`"))?);
                }
                let vals: Vec<V> = slots.into_iter().map(|s| s.ok_or("missing field")).collect::<Result<_, _>>()?;
                // a flat record: built in registers
                if let Some(fs) = self.flat(ty) {
                    let st = self.repr(ty).into_struct_type();
                    let mut out = st.get_undef();
                    for (k, (v, ft)) in vals.into_iter().zip(&fs).enumerate() {
                        let x = self.as_ty(v, *ft, "field")?;
                        out = self.b.build_insert_value(out, self.basic(x), k as u32, "").s()?.into_struct_value();
                    }
                    return Ok(V::R(out, ty));
                }
                self.construct(t, None, vals)
            }
            ExprKind::Field { base, name } => {
                let b = self.expr(base)?;
                if let V::R(r, rt) = b {
                    let k = match rt {
                        Ty::Obj(id) => self.p.types[types::obj_tid(id)].field_index(name).ok_or_else(|| format!("no field {name}"))?,
                        _ => name.parse().map_err(|_| "bad tuple index")?,
                    };
                    let fs = self.flat(rt).unwrap();
                    let x = self.b.build_extract_value(r, k as u32, name).s()?;
                    return self.as_ty(self.val_of(x, fs[k], false), ty, &format!("field `{name}`"));
                }
                let b = self.boxed(b)?;
                let r = match self.ty_of(base) {
                    Ty::Tup(_) => {
                        let i: u64 = name.parse().map_err(|_| "bad tuple index")?;
                        self.call_rt_ptr("mim_obj_get", &[self.basic(b).into(), i64t.const_int(i, false).into()])?
                    }
                    Ty::Obj(t) => {
                        let tid = types::obj_tid(t);
                        let k = self.p.types[tid].field_index(name).ok_or_else(|| format!("no field {name}"))?;
                        if let Some(el) = self.raw_field(tid, None, k) {
                            // a number stored in the record itself
                            let fp = self.slot_ptr(self.basic(b).into_pointer_value(), k)?;
                            let x = self.load_raw(fp, el)?;
                            self.drop_tmp(b)?;
                            return self.as_ty(x, ty, &format!("field `{name}`"));
                        }
                        // (borrowed while the record is; an owned record passes it on)
                        let fv = self.field_ptr(b, t, name, ty)?;
                        if let Some((el, n)) = fixed(ty) {
                            // a list field of known length (a shaped record):
                            // it holds exactly n elements of `el`, read them
                            let r = self.load_fixed(fv, el, n)?;
                            self.drop_tmp(b)?;
                            return Ok(r);
                        }
                        if let V::P(_, true) = b {
                            self.call_rt("mim_retain", &[fv.into()])?;
                            V::P(fv, true)
                        } else {
                            V::P(fv, false)
                        }
                    }
                    _ => {
                        let n = self.global_cstr(name);
                        self.call_rt_ptr("mim_obj_get_named", &[self.basic(b).into(), n.into()])?
                    }
                };
                self.drop_tmp(b)?;
                self.as_ty(r, ty, &format!("field `{name}`"))
            }
            ExprKind::With { base, fields } if self.flat(ty).is_some() && fields.iter().all(|(n, _, _)| !n.contains(['.', '['])) => {
                let b = self.expr(base)?;
                let fs = self.flat(ty).unwrap();
                let Ty::Obj(id) = ty else { unreachable!() };
                let tid = types::obj_tid(id);
                let cur = self.as_ty(b, ty, "record")?;
                let V::R(mut r, _) = cur else { return Err("internal compiler error: flat record".into()) };
                for (n, x, _) in fields {
                    let k = self.p.types[tid].field_index(n).ok_or("unknown field")?;
                    let v = self.expr(x)?;
                    let v = self.convert_slot(v, self.ty_of(x), fs[k], &format!("field `{n}`"))?;
                    r = self.b.build_insert_value(r, self.basic(v), k as u32, "").s()?.into_struct_value();
                }
                Ok(V::R(r, ty))
            }
            // a record on the heap: updated in place when nothing else holds
            // it (else copied once), its fields written directly
            ExprKind::With { base, fields }
                if matches!(self.ty_of(base), Ty::Obj(_)) && self.in_place_ok() && fields.iter().all(|(n, _, _)| !n.contains(['.', '['])) =>
            {
                let Ty::Obj(t) = self.ty_of(base) else { unreachable!() };
                let tid = types::obj_tid(t);
                // a variable as the base: the new values first (they may read
                // it), then the record (which may move: owned.rs); reading a
                // variable has no effects, so the order is not observable
                let path_base = matches!(base.kind, ExprKind::Path(_));
                // `f: push|concat|put(s.f, …)`: the list is taken out of the
                // record (held once by then) and changed in place, not copied
                let steals: Vec<Option<Vec<(&str, &[Expr])>>> = fields.iter().map(|(n, x, _)| if path_base { self.steal_chain(base, tid, n, x, fields) } else { None }).collect();
                let mut early = Vec::new();
                let mut early_rest: Vec<Vec<Vec<V<'ctx>>>> = Vec::new();
                if path_base {
                    for ((_, x, _), st) in fields.iter().zip(&steals) {
                        match st {
                            Some(chain) => {
                                // numbers stay raw, short lists in registers
                                // (typed calls below); anything else boxed
                                let mut levels = Vec::new();
                                for (_, rest) in chain {
                                    let mut vs = Vec::new();
                                    for a in rest.iter() {
                                        let v = self.expr(a)?;
                                        vs.push(match v {
                                            V::I(_) | V::A(_, El::I64) => v,
                                            _ => self.boxed(v)?,
                                        });
                                    }
                                    levels.push(vs);
                                }
                                early_rest.push(levels);
                                early.push(V::I(self.ctx.i64_type().const_zero()));
                            }
                            None => {
                                early_rest.push(Vec::new());
                                early.push(self.expr(x)?);
                            }
                        }
                    }
                }
                let b = self.expr(base)?;
                let cur = self.own(self.boxed(b)?)?;
                // the only reference (the common case after a move): used as
                // it is, no call; else the runtime copies it
                let V::P(p0, _) = cur else { unreachable!() };
                let i64t = self.ctx.i64_type();
                let rc = self.b.build_load(i64t, p0, "rc").s()?;
                inkwell::values::BasicValue::as_instruction_value(&rc).unwrap().set_atomic_ordering(inkwell::AtomicOrdering::Acquire).map_err(|e| e.to_string())?;
                let one = self.b.build_int_compare(IntPredicate::EQ, rc.into_int_value(), i64t.const_int(1, false), "").s()?;
                let fun = self.cur_fn.unwrap();
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
                let cur = phi.as_basic_value().into_pointer_value();
                for (k, (n, x, _)) in fields.iter().enumerate() {
                    if steals[k].is_some() {
                        continue; // after every other field (below)
                    }
                    let i = self.p.types[tid].field_index(n).ok_or("unknown field")?;
                    let v = if path_base { early[k] } else { self.expr(x)? };
                    let fp = self.slot_ptr(cur, i)?;
                    if let Some(el) = self.raw_field(tid, None, i) {
                        self.store_raw(fp, v, el)?;
                        continue;
                    }
                    let (_, ft) = self.record_field(tid, n);
                    let v = if matches!(ft, Ty::Arr(El::F64 | El::I64, _)) { self.convert_slot(v, self.ty_of(x), ft, &format!("field `{n}`"))? } else { v };
                    let v = self.own(self.boxed(v)?)?;
                    let old = self.b.build_load(self.ptr, fp, "old").s()?.into_pointer_value();
                    self.b.build_store(fp, self.basic(v)).s()?;
                    self.release(old)?;
                }
                // the steals, last: every other field already holds what it
                // keeps, so a list still shared is seen as shared (and copied)
                for (k, (n, _, _)) in fields.iter().enumerate() {
                    let Some(chain) = steals[k].clone() else { continue };
                    let i = self.p.types[tid].field_index(n).ok_or("unknown field")?;
                    let fp = self.slot_ptr(cur, i)?;
                    let first = self.b.build_load(self.ptr, fp, "steal").s()?.into_pointer_value();
                    self.b.build_store(fp, self.ptr.const_null()).s()?;
                    let mut list = first;
                    for (lvl, (op, _)) in chain.iter().enumerate() {
                    let op = *op;
                    let rest = early_rest[k][lvl].clone();
                    let i64_list = matches!(self.record_field(tid, n).1, Ty::Arr(El::I64, 0));
                    let u8_list = matches!(self.record_field(tid, n).1, Ty::Arr(El::U8, 0));
                    let i64t = self.ctx.i64_type();
                    // a list of i64 and raw numbers: the typed versions
                    let typed: Option<(&str, Vec<BasicMetadataValueEnum>)> = match (op, rest.as_slice()) {
                        ("push", [V::I(x)]) if i64_list => Some(("mim_push_i64_own", vec![list.into(), (*x).into()])),
                        ("push", [V::I(x)]) if u8_list => Some(("mim_push_u8_own", vec![list.into(), (*x).into()])),
                        ("put", [V::I(i), V::I(x)]) if i64_list => Some(("mim_put_i64_own", vec![list.into(), (*i).into(), (*x).into()])),
                        ("put", [V::I(i), V::I(x)]) if u8_list => Some(("mim_put_u8_own", vec![list.into(), (*i).into(), (*x).into()])),
                        ("concat", [V::A(a, El::I64)]) if i64_list => {
                            let at = a.get_type();
                            let buf = self.entry_alloca_ty(at.into(), "concat_buf")?;
                            self.b.build_store(buf, *a).s()?;
                            Some(("mim_concat_i64_own", vec![list.into(), buf.into(), i64t.const_int(at.len() as u64, false).into()]))
                        }
                        _ => None,
                    };
                    if let Some((name, argv)) = typed {
                        let call = move |cg: &mut Self| -> R<PointerValue<'ctx>> {
                            let f = cg.module.get_function(name).unwrap_or_else(|| {
                                let ps: Vec<BasicMetadataTypeEnum> = argv.iter().enumerate().map(|(j, _)| if j == 0 || (name == "mim_concat_i64_own" && j == 1) { cg.ptr.into() } else { i64t.into() }).collect();
                                cg.module.add_function(name, cg.ptr.fn_type(&ps, false), Some(Linkage::External))
                            });
                            Ok(cg.b.build_call(f, &argv, "").s()?.try_as_basic_value().basic().unwrap().into_pointer_value())
                        };
                        // the common case, a list held once with room (or an
                        // index in range), done here; the runtime otherwise
                        list = match (op, rest.as_slice()) {
                            ("push", [V::I(x)]) if i64_list && self.opt_level >= 2 => self.append_i64_inline(list, &[*x], &mut { call })?,
                            ("put", [V::I(i), V::I(x)]) if i64_list && self.opt_level >= 2 => self.put_i64_inline_or(list, *i, *x, &mut { call })?,
                            ("concat", [V::A(a, El::I64)]) if i64_list && self.opt_level >= 2 && a.get_type().len() <= 8 => {
                                let mut xs = Vec::new();
                                for j in 0..a.get_type().len() {
                                    xs.push(self.b.build_extract_value(*a, j, "").s()?.into_int_value());
                                }
                                self.append_i64_inline(list, &xs, &mut { call })?
                            }
                            _ => call(self)?,
                        };
                        continue;
                    }
                    let mut boxed_rest = Vec::new();
                    for v in rest {
                        boxed_rest.push(self.boxed(v)?);
                    }
                    let mut argv: Vec<BasicMetadataValueEnum> = vec![list.into()];
                    for v in &boxed_rest {
                        argv.push(self.basic(*v).into());
                    }
                    let name = format!("mim_std_{op}_own");
                    let f = self.module.get_function(&name).unwrap_or_else(|| {
                        let ps: Vec<BasicMetadataTypeEnum> = argv.iter().map(|_| self.ptr.into()).collect();
                        self.module.add_function(&name, self.ptr.fn_type(&ps, false), Some(Linkage::External))
                    });
                    list = self.b.build_call(f, &argv, "").s()?.try_as_basic_value().basic().unwrap().into_pointer_value();
                    for v in boxed_rest {
                        self.drop_tmp(v)?;
                    }
                    }
                    self.b.build_store(fp, list).s()?;
                }
                Ok(V::P(cur, true))
            }
            ExprKind::With { base, fields } => {
                let Ty::Obj(t) = self.ty_of(base) else { return Err("`with` needs a record".into()) };
                let b = self.expr(base)?;
                // mim_obj_with consumes its record: hand it an owned reference
                let mut cur = self.own(self.boxed(b)?)?;
                for (n, x, _) in fields {
                    let tid = types::obj_tid(t);
                    let i = self.p.types[tid].field_index(n).ok_or("unknown field")?;
                    let ft = match &self.p.types[tid].decl.kind {
                        mimic_syntax::ast::TypeKind::Record(fs) => fs.get(i).and_then(|(_, ft, _)| types::ann_in(self.p, self.p.types[tid].module, ft)).unwrap_or(Ty::Dyn),
                        _ => Ty::Dyn,
                    };
                    let v = self.expr(x)?;
                    let v = if matches!(ft, Ty::Arr(El::F64 | El::I64, _)) { self.convert_slot(v, self.ty_of(x), ft, &format!("field `{n}`"))? } else { v };
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
                        InterpPart::Expr(x) => match self.expr(x)? {
                            // an integer: straight to text
                            V::I(i) => self.call_rt_ptr("mim_format_i64", &[i.into()])?,
                            v => {
                                let v = self.boxed(v)?;
                                let s = self.call_rt_ptr("mim_format", &[self.basic(v).into()])?;
                                self.drop_tmp(v)?;
                                s
                            }
                        },
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
                // patterns read lists through the runtime: a list in registers
                // is boxed once (and released with the match)
                let sv = if let V::A(..) | V::R(..) = sv { self.boxed(sv)? } else { sv };
                let scrut_lives = match sv {
                    V::P(_, true) => true,
                    V::P(_, false) => matches!(&scrut.kind, ExprKind::Path(path) if path.len() == 1 && self.lookup(&path[0]).is_some_and(|l| !l.mutable)),
                    _ => false,
                };
                let rt = if ty == Ty::Never { Ty::Dyn } else { ty };
                let f = self.cur_fn.unwrap();
                // keep an owned scrutinee in a scope so early `return`s release it
                self.scopes.push(Vec::new());
                if let V::P(p, true) = sv {
                    let slot = self.entry_alloca(Ty::Dyn, "scrutinee")?;
                    self.b.build_store(slot, p).s()?;
                    self.scopes.last_mut().unwrap().push(Local { name: "\u{0}scrutinee".into(), slot, ty: Ty::Dyn, owned: true, mutable: false, moves: false });
                }
                let merge = self.ctx.append_basic_block(f, "match_end");
                let mut incoming = Vec::new();
                for arm in arms {
                    let next = self.ctx.append_basic_block(f, "next_arm");
                    self.pattern_test(&arm.pat, sv, next)?;
                    self.scopes.push(Vec::new());
                    // the scrutinee lives until the match ends (owned here, or
                    // an immutable variable): what it holds can be borrowed
                    self.borrow_binds = scrut_lives;
                    let r = self.pattern_bind(&arm.pat, sv);
                    self.borrow_binds = false;
                    r?;
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

/// `MIMIC_COUNT_RC=1` (read once): the inline release's fast path tells the
/// runtime, which counts every reference-count operation
/// (docs/design/ownership.md).
fn counting_rc() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("MIMIC_COUNT_RC").is_ok_and(|v| v == "1"))
}

#[cfg(test)]
mod tests {
    #[test]
    fn suffixes_are_distinct() {
        use mimic_check::types::{El, Ty};
        assert_ne!(super::ty_suffix(&[Ty::I64]), super::ty_suffix(&[Ty::F64]));
        assert_ne!(super::ty_suffix(&[Ty::Arr(El::I64, 0)]), super::ty_suffix(&[Ty::Arr(El::F64, 0)]));
    }
}
