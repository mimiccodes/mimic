//! Compilation pipeline: load sources → parse → check → LLVM codegen →
//! compile the runtime with the embedded Clang → link with the embedded LLD
//! (see toolchain.rs).

use crate::targets::Target;
use crate::report::Report;
use mimic_check::{Options, Program, SourceFile};
use mimic_syntax::codes as C;
use mimic_project::{MANIFEST, Project, packages};
use mimic_syntax::parser::parse_file;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

pub const RUNTIME_C: &str = include_str!("../../../runtime/mimic_rt.c");
pub const RUNTIME_H: &str = include_str!("../../../runtime/mimic_rt.h");
pub const RUNTIME_NET_C: &str = include_str!("../../../runtime/mimic_net.c");

/// What to compile: a whole project, or a single standalone `.mim` file.
pub enum Input {
    Project(Box<Project>),
    File(PathBuf),
}

impl Input {
    pub fn name(&self) -> String {
        match self {
            Input::Project(p) => p.manifest.name.clone(),
            Input::File(f) => {
                let stem = f.file_stem().map(|s| s.to_string_lossy().replace('-', "_")).unwrap_or_default();
                if mimic_project::valid_ident(&stem) { stem } else { "script".into() }
            }
        }
    }
    pub fn is_bin(&self) -> bool {
        match self {
            Input::Project(p) => p.manifest.bin,
            Input::File(_) => true,
        }
    }
}

pub struct Profile {
    pub release: bool,
    /// Compile pure functions to interaction nets (default on).
    pub net: bool,
    pub opt: u8,
    pub leak_check: bool,
    /// `--cpu`: a CPU model to generate code for (`native` = this machine).
    pub cpu: Option<String>,
}

impl Profile {
    pub fn for_input(input: &Input, release: bool) -> Profile {
        let cfg = match input {
            Input::Project(p) => p.build.clone(),
            Input::File(_) => mimic_project::BuildConfig::default(),
        };
        let p = if release { cfg.release } else { cfg.debug };
        Profile { release, net: true, opt: p.opt, leak_check: p.leak_check, cpu: None }
    }
    pub fn name(&self) -> &'static str {
        if self.release { "release" } else { "debug" }
    }
}

/// The standard prelude: compiled into every program, visible everywhere.
pub const PRELUDE: &str = "module prelude

// Optional values: `let x = find(…) else { return … }` unwraps Some.
pub type Option = Some(any) | None

// Results of operations that can fail: `else` unwraps Ok.
pub type Result = Ok(any) | Err(any)
";

fn prelude_file() -> SourceFile {
    let ast = parse_file(PRELUDE).expect("the prelude parses");
    SourceFile { path: PathBuf::from("<prelude>"), display: "<prelude>".into(), src: PRELUDE.into(), ast, package: "std".into() }
}

fn display_path(root: Option<&Path>, p: &Path) -> String {
    root.and_then(|r| p.strip_prefix(r).ok()).unwrap_or(p).display().to_string()
}

fn read_parse(path: &Path, display: String, package: &str, report: &mut Report) -> Option<SourceFile> {
    let src = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            report.push_plain(&C::IO, format!("cannot read {}: {e}", path.display()));
            return None;
        }
    };
    match parse_file(&src) {
        Ok(ast) => Some(SourceFile { path: path.to_path_buf(), display, src, ast, package: package.into() }),
        Err(d) => {
            report.push_in(Some(path), &display, &src, d);
            None
        }
    }
}

/// Load, parse, resolve and check for the host. Errors come back as a structured report.
pub fn load(input: &Input) -> Result<Program, Report> {
    load_for(input, &Target::host())
}

/// Like [`load`], for a target: C headers (`extern "C" include`) are read as
/// that target sees them.
pub fn load_for(input: &Input, target: &Target) -> Result<Program, Report> {
    let mut report = Report::default();
    let mut files = Vec::new();
    let pkg = input.name();
    match input {
        Input::File(f) => {
            if let Some(sf) = read_parse(f, f.display().to_string(), &pkg, &mut report) {
                if sf.ast.module.name != "main" {
                    let d = mimic_syntax::Diag::new(
                        &C::ENTRY_MODULE,
                        format!("`mimic run <file>` expects the file to declare `module main` (found `module {}`)", sf.ast.module.name),
                        sf.ast.module.span,
                    );
                    report.push_in(Some(&sf.path), &sf.display, &sf.src, d);
                }
                files.push(sf);
            }
        }
        Input::Project(p) => {
            for path in p.sources()? {
                // test.mim is only compiled by `mimic test`
                if path.file_name().is_some_and(|n| n == "test.mim") && path.parent() == Some(&p.root.join("src")) {
                    continue;
                }
                if let Some(sf) = read_parse(&path, display_path(Some(&p.root), &path), &pkg, &mut report) {
                    files.push(sf);
                }
            }
            if !p.manifest.bin && !files.iter().any(|f| f.ast.module.name == "lib") {
                report.push_plain(&C::ENTRY_MODULE, "library projects need src/lib.mim with `module lib`");
            }
            // dependencies (transitively), compiled from their library sources
            let mut seen = BTreeSet::from([pkg.clone()]);
            let mut queue: Vec<(PathBuf, mimic_project::Dependency)> =
                p.manifest.dependencies.iter().map(|d| (p.root.clone(), d.clone())).collect();
            let dep_err = |s: String| Report::from_text(&C::DEPENDENCY, &s);
            while let Some((from_root, dep)) = queue.pop() {
                if !seen.insert(dep.name.clone()) {
                    continue;
                }
                let holder = Project::load(&from_root)?;
                let dir = holder.dep_dir(&dep).map_err(dep_err)?;
                let dp = Project::load(&dir)?;
                if dp.manifest.name != dep.name {
                    return Err(Report::plain(
                        &C::DEPENDENCY,
                        format!("dependency `{}` resolves to {}, whose mim.mim names it `{}`", dep.name, dir.display(), dp.manifest.name),
                    ));
                }
                if !dp.manifest.lib {
                    return Err(Report::plain(&C::DEPENDENCY, format!("dependency `{}` is not a library", dep.name)));
                }
                for path in dp.sources()? {
                    let fname = path.file_name().unwrap().to_string_lossy().to_string();
                    if path.parent() == Some(&dir.join("src")) && (fname == "main.mim" || fname == "test.mim") {
                        continue;
                    }
                    let shown = format!("{}/{}", dep.name, display_path(Some(&dir), &path));
                    if let Some(sf) = read_parse(&path, shown, &dep.name, &mut report) {
                        files.push(sf);
                    }
                }
                for d in &dp.manifest.dependencies {
                    queue.push((dir.clone(), d.clone()));
                }
            }
        }
    }
    if !report.entries.is_empty() {
        return Err(report);
    }
    // C headers named by `extern "C" include "…"`
    let mut include_dirs: Vec<PathBuf> = Vec::new();
    if let Input::Project(p) = input {
        include_dirs.push(p.root.clone());
        include_dirs.extend(p.manifest.ffi_include.iter().map(|d| p.root.join(d)));
    }
    for sf in &mut files {
        let here = sf.path.parent().filter(|d| !d.as_os_str().is_empty()).map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("."));
        let mut dirs = vec![here];
        dirs.extend(include_dirs.iter().cloned());
        let mut errs = Vec::new();
        import_headers(&mut sf.ast.module.items, &dirs, target, &mut errs);
        for (msg, span) in errs {
            report.push_in(Some(&sf.path), &sf.display, &sf.src, mimic_syntax::Diag::new(&C::CIMPORT, msg, span));
        }
    }
    if !report.entries.is_empty() {
        return Err(report);
    }
    files.push(prelude_file());
    let (mut program, diags) = Program::build(files, &pkg);
    if !diags.is_empty() {
        return Err(Report::from_program(&program, diags));
    }
    // named and default arguments become plain positional calls
    let ndiags = mimic_check::normalize(&mut program);
    if !ndiags.is_empty() {
        return Err(Report::from_program(&program, ndiags));
    }
    let diags = mimic_check::check(&program, &Options { require_main: input.is_bin() });
    if !diags.is_empty() {
        return Err(Report::from_program(&program, diags));
    }
    // static type errors (inference over every reachable specialisation)
    let tdiags = mimic_check::types::check_types(&program, !input.is_bin());
    if !tdiags.is_empty() {
        return Err(Report::from_program(&program, tdiags));
    }
    Ok(program)
}

// ---- toolchain --------------------------------------------------------------

pub use crate::toolchain::run_tool;
use crate::toolchain::{self, LinkJob, Toolchain};

/// Compile (and cache) the C runtime for a target.
pub fn runtime_object(tc: &Toolchain, target: &Target, release: bool, cpu: Option<&str>) -> Result<PathBuf, String> {
    let portable = cpu == Some("portable");
    let resolved = if portable { portable_cpu(target) } else { cpu.map(String::from) };
    let cpu = resolved.as_deref().or(if portable { Some("") } else { None });
    let key = mimic_syntax::fnv1a(
        format!(
            "{RUNTIME_C}{RUNTIME_H}{RUNTIME_NET_C}{}{release}{}{:?}{cpu:?}",
            target.name(),
            toolchain::describe(tc),
            target.llvm,
        )
        .as_bytes(),
    );
    let dir = packages::cache_dir().join("runtime");
    fs::create_dir_all(&dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    let obj = dir.join(format!("mimic_rt-{}-{key:016x}.o", target.name()));
    if obj.is_file() {
        return Ok(obj);
    }
    // each runtime version gets its own directory: mimic_rt.c #includes mimic_net.c
    let src_dir = dir.join(format!("src-{key:016x}"));
    fs::create_dir_all(&src_dir).map_err(|e| e.to_string())?;
    // written atomically: other mimic processes may compile from here at the
    // same time (the header lives beside the sources, one copy per version)
    let src = src_dir.join("mimic_rt.c");
    mimic_toolchain::jobs::write_atomic(&src, RUNTIME_C.as_bytes())?;
    mimic_toolchain::jobs::write_atomic(&src_dir.join("mimic_net.c"), RUNTIME_NET_C.as_bytes())?;
    mimic_toolchain::jobs::write_atomic(&src_dir.join("mimic_rt.h"), RUNTIME_H.as_bytes())?;
    let tmp = obj.with_extension(format!("o.tmp{}", std::process::id()));
    let mut c = toolchain::cc(tc, target);
    c.args(["-c", if release { "-O3" } else { "-O1" }, "-fPIC"]);
    match cpu {
        Some("native") | None if release && target.name.is_none() => {
            c.arg("-mcpu=native"); /* native builds use every feature of this CPU */
        }
        Some("") => {} // portable without a family baseline: the target default
        Some(cpu) if cpu != "native" => {
            // x86 names a CPU with -march; every other architecture with -mcpu
            let x86 = matches!(target.arch.as_str(), "x86_64" | "x86" | "i386" | "i686");
            c.arg(if x86 { format!("-march={cpu}") } else { format!("-mcpu={cpu}") });
        }
        _ => {}
    }
    c.arg(&src).arg("-o").arg(&tmp);
    run_tool(c, "compiling the mimic runtime")?;
    fs::rename(&tmp, &obj).map_err(|e| e.to_string())?;
    Ok(obj)
}

pub struct Artifact {
    pub path: PathBuf,
    pub kind: ArtifactKind,
    /// Why only an object was produced, when that happens.
    pub note: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ArtifactKind {
    Executable,
    StaticLibrary,
    /// LLVM can generate code for the target, but `mimic` has no C library
    /// for it yet: the program's object file, without the runtime.
    Object,
    /// Like `Object`, for backends that only emit assembly (NVPTX: `.ptx`).
    Assembly,
}

impl ArtifactKind {
    pub fn describe(self) -> &'static str {
        match self {
            ArtifactKind::Executable => "executable",
            ArtifactKind::StaticLibrary => "static library",
            ArtifactKind::Object => "object file",
            ArtifactKind::Assembly => "assembly",
        }
    }
    pub fn json(self) -> &'static str {
        match self {
            ArtifactKind::Executable => "executable",
            ArtifactKind::StaticLibrary => "static-library",
            ArtifactKind::Object => "object",
            ArtifactKind::Assembly => "assembly",
        }
    }
}

pub fn out_dir(input: &Input, profile: &Profile, target: &Target) -> PathBuf {
    let base = match input {
        Input::Project(p) => p.root.join("target"),
        Input::File(f) => {
            let abs = fs::canonicalize(f).unwrap_or_else(|_| f.clone());
            let h = mimic_syntax::fnv1a(abs.to_string_lossy().as_bytes());
            packages::cache_dir().join("run").join(format!("{}-{h:08x}", input.name()))
        }
    };
    let mut d = base.join(profile.name());
    if target.name.is_some() {
        d = d.join(target.name());
    }
    d
}

/// Fill `extern "C" include` blocks (in every module, nested ones too) with the
/// functions the header makes visible. Names the module already defines (its
/// own functions, types, modules, hand-written externs) win over imported ones.
fn import_headers(items: &mut [mimic_syntax::ast::Item], dirs: &[PathBuf], target: &Target, errs: &mut Vec<(String, mimic_syntax::Span)>) {
    use mimic_syntax::ast::Item;
    let mut taken: std::collections::HashSet<String> = std::collections::HashSet::new();
    for item in items.iter() {
        match item {
            Item::Fn(f) => {
                taken.insert(f.name.clone());
            }
            Item::Type(t) => {
                taken.insert(t.name.clone());
            }
            Item::Module(m) => {
                taken.insert(m.name.clone());
            }
            Item::Use(u) => {
                taken.insert(u.alias.clone().unwrap_or_else(|| u.path.last().cloned().unwrap_or_default()));
            }
            Item::Extern(b) => taken.extend(b.fns.iter().map(|f| f.name.clone())),
        }
    }
    for item in items.iter_mut() {
        match item {
            Item::Module(m) => import_headers(&mut m.items, dirs, target, errs),
            Item::Extern(b) if !b.includes.is_empty() => {
                for (header, span) in b.includes.clone() {
                    match crate::cimport::import(&header, dirs, target) {
                        Ok(imp) => {
                            for f in imp.extern_fns(b.is_pub, span) {
                                if taken.insert(f.name.clone()) {
                                    b.fns.push(f);
                                }
                            }
                        }
                        Err(e) => errs.push((e, span)),
                    }
                }
            }
            _ => {}
        }
    }
}

pub fn build(input: &Input, profile: &Profile, target: &Target, emit_ir: bool, emit_net: bool) -> Result<Artifact, Report> {
    let program = load_for(input, target)?;
    if let Input::Project(p) = input {
        crate::modfile::sync(p, &program)?;
    }
    codegen_and_link(input, &program, profile, target, emit_ir, emit_net).map_err(|e| Report::from_text(&C::BUILD, &e))
}

fn codegen_and_link(
    input: &Input,
    program: &Program,
    profile: &Profile,
    target: &Target,
    emit_ir: bool,
    emit_net: bool,
) -> Result<Artifact, String> {
    if target.is_device_only() {
        return Err(format!(
            "`{}` is a GPU target: a whole program cannot run there; compiling Mimic's array kernels for GPUs is the GPU backend on the roadmap (F8)",
            target.name()
        ));
    }
    let dir = out_dir(input, profile, target);
    fs::create_dir_all(&dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    let name = input.name();
    let obj = dir.join(format!("{name}.o"));
    let ir = dir.join(format!("{name}.ll"));
    let net = dir.join(format!("{name}.mnet"));
    let is_exe = input.is_bin();
    // cross targets: the same CPU/features/ABI as Clang uses for the C side
    let (mut cpu, mut features, abi) = match &target.llvm {
        Some(t) => match toolchain::clang_defaults(t) {
            Some((c, f, a)) => (Some(c), Some(f), a),
            None => (None, None, None),
        },
        None => (None, None, None),
    };
    // --cpu: a named model replaces the defaults (its features follow from it)
    let portable = profile.cpu.as_deref() == Some("portable");
    let cpu_choice = if portable { portable_cpu(target) } else { profile.cpu.clone() };
    match cpu_choice.as_deref() {
        None if portable => {
            // no family baseline known: the target's default CPU is already portable
            if target.name.is_none() {
                cpu = Some("generic".into());
                features = Some(String::new());
            }
        }
        None => {}
        Some("native") if target.name.is_none() => {}
        Some("native") => return Err(format!("--cpu native only applies to native builds, not `{}`", target.name())),
        Some(c) => {
            check_cpu(target, c)?;
            cpu = Some(c.to_string());
            features = None;
        }
    }
    let obj = mimic_codegen::compile(
        program,
        &mimic_codegen::Settings {
            triple: target.llvm.clone(),
            opt_level: profile.opt,
            leak_check: profile.leak_check,
            emit_main: is_exe,
            export_pub: !is_exe,
            use_net: profile.net,
            cpu: cpu.clone(),
            features: features.clone(),
            abi: abi.clone(),
        },
        &obj,
        emit_ir.then_some(ir.as_path()),
        emit_net.then_some(net.as_path()),
    )?;
    let tc = match toolchain::select(target) {
        Ok(tc) => tc,
        // every target LLVM supports gets at least its object file
        Err(why) if target.name.is_some() => {
            let kind = if obj.extension().is_some_and(|e| e == "o") { ArtifactKind::Object } else { ArtifactKind::Assembly };
            return Ok(Artifact { path: obj, kind, note: Some(why) });
        }
        Err(why) => return Err(why),
    };
    let rt = runtime_object(&tc, target, profile.release, profile.cpu.as_deref())?;
    let c_objects = match input {
        Input::Project(p) => compile_c_sources(p, &tc, target, profile, &dir)?,
        Input::File(_) => Vec::new(),
    };
    if is_exe {
        let exe = dir.join(format!("{name}{}", target.exe_suffix()));
        let mut objects = vec![obj, rt];
        objects.extend(c_objects);
        let mut job = LinkJob { objects: &objects, output: &exe, search: Vec::new(), libs: Vec::new(), debug: !profile.release };
        if let Input::Project(p) = input {
            job.search = p.manifest.ffi_search.iter().map(|s| p.root.join(s)).collect();
            job.libs = p.manifest.ffi_link.clone();
        }
        run_tool(toolchain::link(&tc, target, &job), "linking")?;
        Ok(Artifact { path: exe, kind: ArtifactKind::Executable, note: None })
    } else {
        // library: static archive of the program object + runtime
        let lib = dir.join(format!("lib{name}.a"));
        let _ = fs::remove_file(&lib);
        let mut members = vec![obj, rt];
        members.extend(c_objects);
        toolchain::archive(&tc, target, &lib, &members)?;
        Ok(Artifact { path: lib, kind: ArtifactKind::StaticLibrary, note: None })
    }
}

/// Compile the project's C sources (`ffi { sources = [...] }`) for the target,
/// in parallel; an object is reused while it is newer than its source and was
/// built with the same flags.
fn compile_c_sources(p: &Project, tc: &Toolchain, target: &Target, profile: &Profile, dir: &Path) -> Result<Vec<PathBuf>, String> {
    if p.manifest.ffi_sources.is_empty() {
        return Ok(Vec::new());
    }
    let out_dir = dir.join("c");
    fs::create_dir_all(&out_dir).map_err(|e| format!("cannot create {}: {e}", out_dir.display()))?;
    let mut flags: Vec<String> = vec![if profile.release { "-O2".into() } else { "-O0".into() }, "-g".into()];
    flags.push(format!("-I{}", p.root.display()));
    flags.extend(p.manifest.ffi_include.iter().map(|d| format!("-I{}", p.root.join(d).display())));
    flags.extend(p.manifest.ffi_cflags.iter().cloned());
    let stamp = format!("{:016x}", mimic_syntax::fnv1a(format!("{flags:?}{:?}", target.llvm).as_bytes()));
    let jobs: Vec<(PathBuf, PathBuf)> = p
        .manifest
        .ffi_sources
        .iter()
        .map(|s| {
            let src = p.root.join(s);
            let obj = out_dir.join(format!("{}-{stamp}.o", s.replace(['/', '\\', '.'], "_")));
            (src, obj)
        })
        .collect();
    let errors = std::sync::Mutex::new(Vec::new());
    std::thread::scope(|scope| {
        for (src, obj) in &jobs {
            let (flags, errors) = (&flags, &errors);
            scope.spawn(move || {
                let fresh = match (fs::metadata(src).and_then(|m| m.modified()), fs::metadata(obj).and_then(|m| m.modified())) {
                    (Ok(s), Ok(o)) => o >= s,
                    (Err(e), _) => {
                        errors.lock().unwrap().push(format!("cannot read C source {}: {e}", src.display()));
                        return;
                    }
                    _ => false,
                };
                if fresh {
                    return;
                }
                let mut c = toolchain::cc(tc, target);
                c.args(flags).arg("-c").arg(src).arg("-o").arg(obj);
                if let Err(e) = run_tool(c, &format!("compiling {}", src.display())) {
                    errors.lock().unwrap().push(e);
                }
            });
        }
    });
    let errors = errors.into_inner().unwrap();
    if !errors.is_empty() {
        return Err(errors.join("\n"));
    }
    Ok(jobs.into_iter().map(|(_, o)| o).collect())
}

/// `--cpu portable`: the oldest model of the target's CPU family, so one binary
/// runs on every machine of that family.
fn portable_cpu(target: &Target) -> Option<String> {
    let arch = if target.name.is_none() { std::env::consts::ARCH } else { target.arch.as_str() };
    let apple = target.name.is_none() && cfg!(target_os = "macos") || target.is_apple();
    match arch {
        "aarch64" | "arm64" if apple => Some("apple-m1".into()),
        "x86_64" => Some("x86-64-v2".into()),
        _ => None,
    }
}

/// `--cpu` must name a model LLVM knows for the target.
fn check_cpu(target: &Target, cpu: &str) -> Result<(), String> {
    let triple = target.llvm.clone().unwrap_or_else(|| format!("{}-{}", std::env::consts::ARCH, std::env::consts::OS));
    let triple = mimic_toolchain::llvm_triple(&triple).triple;
    // aliases count (apple-m3 is an alias LLVM accepts)
    if mimic_toolchain::cpu_valid(&triple, cpu) != Some(false) {
        return Ok(());
    }
    let Some(list) = mimic_toolchain::cpus(&triple, false) else { return Ok(()) };
    let names: Vec<&str> = list.iter().map(|(n, _)| n.as_str()).collect();
    let hint = mimic_syntax::closest(cpu, names.iter().copied()).map(|b| format!("; did you mean `{b}`?")).unwrap_or_default();
    Err(format!("unknown CPU `{cpu}` for `{}`{hint} (run `mimic targets --cpus {}`)", target.name(), target.arch))
}

/// Resolve the CLI's notion of "current project" or a given file.
pub fn input_from(file: Option<&Path>) -> Result<Input, String> {
    if let Some(f) = file {
        if !f.is_file() {
            return Err(format!("error: no such file: {}", f.display()));
        }
        if f.extension().is_none_or(|e| e != "mim") {
            return Err(format!("error: {} is not a .mim file", f.display()));
        }
        return Ok(Input::File(f.to_path_buf()));
    }
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    let root = mimic_project::find_root(&cwd).ok_or_else(|| {
        format!("error: could not find `{MANIFEST}` in {} or any parent directory\n  = help: create a project with `mimic create <name>` or `mimic init`, or pass a file: `mimic run file.mim`", cwd.display())
    })?;
    Ok(Input::Project(Box::new(Project::load(&root)?)))
}

/// Compile a raw `.mnet` interaction-net file into an executable that reduces `@main`.
pub fn build_net(file: &Path, release: bool) -> Result<PathBuf, String> {
    let src = fs::read_to_string(file).map_err(|e| format!("cannot read {}: {e}", file.display()))?;
    let book = mimic_net::parse(&src).map_err(|e| format!("error: {}:{e}", file.display()))?;
    let enc = mimic_net::encode(&book).map_err(|e| format!("error: {}: {e}", file.display()))?;
    let entry = enc.id("main").ok_or_else(|| format!("error: {}: no @main definition", file.display()))?;
    let target = Target::host();
    let abs = fs::canonicalize(file).unwrap_or_else(|_| file.to_path_buf());
    let h = mimic_syntax::fnv1a(abs.to_string_lossy().as_bytes());
    let stem = file.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "net".into());
    let dir = packages::cache_dir().join("net").join(format!("{stem}-{h:08x}")).join(if release { "release" } else { "debug" });
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let c_src = dir.join("main.c");
    fs::write(&c_src, mimic_net::emit_c_main(&enc, entry, !release)).map_err(|e| e.to_string())?;
    let tc = toolchain::select(&target)?;
    let rt = runtime_object(&tc, &target, release, None)?;
    let main_obj = dir.join("main.o");
    let mut c = toolchain::cc(&tc, &target);
    c.args(["-c", if release { "-O2" } else { "-O0" }]).arg(&c_src).arg("-o").arg(&main_obj);
    run_tool(c, "compiling the net")?;
    let exe = dir.join(&stem);
    let objects = [main_obj, rt];
    let job = LinkJob { objects: &objects, output: &exe, search: Vec::new(), libs: Vec::new(), debug: !release };
    run_tool(toolchain::link(&tc, &target, &job), "linking the net")?;
    Ok(exe)
}
