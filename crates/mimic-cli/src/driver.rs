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
pub const RUNTIME_STD_C: &str = include_str!("../../../runtime/mimic_std.c");
pub const RUNTIME_IO_C: &str = include_str!("../../../runtime/mimic_io.c");

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
    /// `--emit-header`: write `<name>.h` declaring the `extern "C" fn`s.
    pub emit_header: bool,
}

impl Profile {
    pub fn for_input(input: &Input, release: bool) -> Profile {
        let cfg = match input {
            Input::Project(p) => p.build.clone(),
            Input::File(_) => mimic_project::BuildConfig::default(),
        };
        let p = if release { cfg.release } else { cfg.debug };
        Profile { release, net: true, opt: p.opt, leak_check: p.leak_check, cpu: None, emit_header: false }
    }
    pub fn name(&self) -> &'static str {
        if self.release { "release" } else { "debug" }
    }
}

/// The standard prelude: compiled into every program, visible everywhere.
pub const PRELUDE: &str = "module prelude

// Optional values: `let x = find(…) else { return … }` unwraps Some.
pub type Option[T] = Some(T) | None

// Results of operations that can fail: `else` unwraps Ok.
pub type Result[T, E] = Ok(T) | Err(E)

// Strings, lists and maps: implemented in Mimic's runtime (runtime/mimic_std.c).
pub extern \"mimic\" {
    fn split(s: str, sep: str) -> [str]
    fn join(parts: any, sep: str) -> str
    fn lines(s: str) -> [str]
    fn chars(s: str) -> [str]
    fn trim(s: str) -> str
    fn trim_start(s: str) -> str
    fn trim_end(s: str) -> str
    fn contains(within: any, part: any) -> bool
    fn starts_with(s: str, prefix: str) -> bool
    fn ends_with(s: str, suffix: str) -> bool
    fn find_index(s: str, part: str) -> i64
    fn replace(s: str, from: str, to: str) -> str
    fn upper(s: str) -> str
    fn lower(s: str) -> str
    fn repeat(s: str, n: i64) -> str
    fn string(v: any) -> str
    fn is_int(s: str) -> bool
    fn to_int(s: str) -> i64
    fn is_float(s: str) -> bool
    fn to_float(s: str) -> f64
    fn push(xs: any, x: any) -> any
    fn concat(xs: any, ys: any) -> any
    fn sort(xs: any) -> any
    fn has(m: any, key: any) -> bool
    fn put(m: any, key: any, value: any) -> any
    fn remove(m: any, key: any) -> any
    fn keys(m: any) -> any
    fn values(m: any) -> any
}

// `Some(byte index)` of the first occurrence of `part`, or `None`.
pub fn find(s: str, part: str) {
    let i = find_index(s, part)
    if i < 0 { None } else { Some(i) }
}

// `Some(n)` when `s` is an integer, else `None`.
pub fn parse_int(s: str) {
    if is_int(s) { Some(to_int(s)) } else { None }
}

// `Some(x)` when `s` is a number, else `None`.
pub fn parse_float(s: str) {
    if is_float(s) { Some(to_float(s)) } else { None }
}

// `Some(m[key])` when the map has the key, else `None`.
pub fn get(m, key) {
    if has(m, key) { Some(m[key]) } else { None }
}

// Paths: pure text operations (runtime/mimic_io.c).
pub extern \"mimic\" {
    fn path_join(a: str, b: str) -> str
    fn file_name(path: str) -> str
    fn parent(path: str) -> str
    fn extension(path: str) -> str
    fn stem(path: str) -> str
}

// The program and its surroundings. These have side effects, so they never
// run in parallel or out of order (runtime/mimic_io.c).
// \"mimic-io\": effects that must happen in program order.
pub extern \"mimic-io\" {
    fn eprint(value: any)
    fn exit(code: i64)
    fn read_input() -> str
}

// \"mimic-io-concurrent\": effects that may overlap each other (reads). A
// `let` of a function that only has these starts at once and is awaited at
// its first use (an implicit future); `¨` runs such functions in parallel.
pub extern \"mimic-io-concurrent\" {
    fn args() -> [str]
    fn exists(path: str) -> bool
    fn is_dir(path: str) -> bool
    fn is_file(path: str) -> bool
    fn current_dir() -> str
    fn temp_dir() -> str
}

// Low-level forms: on failure they set an error (per thread) that the
// wrappers below read on the same thread.
extern \"mimic-io-concurrent\" {
    fn io_ok() -> bool
    fn io_error() -> str
    fn io_env(name: str) -> str
    fn io_read_file(path: str) -> str
    fn io_file_size(path: str) -> i64
    fn io_list_dir(path: str) -> [str]
}

extern \"mimic-io\" {
    fn io_set_env(name: str, value: str) -> bool
    fn io_read_line() -> str
    fn io_write_file(path: str, text: str, append: bool) -> i64
    fn io_remove_file(path: str) -> bool
    fn io_rename(from: str, to: str) -> bool
    fn io_copy_file(from: str, to: str) -> i64
    fn io_make_dir(path: str) -> bool
}

// `Some(value)` of an environment variable, or `None` when it is not set.
pub fn env(name: str) -> Option[str] {
    let v = io_env(name)
    if io_ok() { Some(v) } else { None }
}

pub fn set_env(name: str, value: str) -> Result[bool, str] {
    let done = io_set_env(name, value)
    if done { Ok(true) } else { Err(io_error()) }
}

// The next line of standard input (without its newline); `None` at the end.
pub fn read_line() -> Option[str] {
    let line = io_read_line()
    if io_ok() { Some(line) } else { None }
}

pub fn read_file(path: str) -> Result[str, str] {
    let text = io_read_file(path)
    if io_ok() { Ok(text) } else { Err(io_error()) }
}

// Replace the file's contents; `Ok(bytes written)`.
pub fn write_file(path: str, text: str) -> Result[i64, str] {
    let n = io_write_file(path, text, false)
    if io_ok() { Ok(n) } else { Err(io_error()) }
}

pub fn append_file(path: str, text: str) -> Result[i64, str] {
    let n = io_write_file(path, text, true)
    if io_ok() { Ok(n) } else { Err(io_error()) }
}

pub fn file_size(path: str) -> Result[i64, str] {
    let n = io_file_size(path)
    if io_ok() { Ok(n) } else { Err(io_error()) }
}

// Remove a file, or an empty directory.
pub fn remove_file(path: str) -> Result[bool, str] {
    let done = io_remove_file(path)
    if done { Ok(true) } else { Err(io_error()) }
}

pub fn rename(from: str, to: str) -> Result[bool, str] {
    let done = io_rename(from, to)
    if done { Ok(true) } else { Err(io_error()) }
}

pub fn copy_file(from: str, to: str) -> Result[i64, str] {
    let n = io_copy_file(from, to)
    if io_ok() { Ok(n) } else { Err(io_error()) }
}

// The names in a directory, sorted (without `.` and `..`).
pub fn list_dir(path: str) -> Result[[str], str] {
    let names = io_list_dir(path)
    if io_ok() { Ok(names) } else { Err(io_error()) }
}

// Create a directory and any missing parents (fine if it exists).
pub fn make_dir(path: str) -> Result[bool, str] {
    let done = io_make_dir(path)
    if done { Ok(true) } else { Err(io_error()) }
}

// ---- time, random numbers, processes -------------------------------------

// A calendar date and time. weekday: 1 = Monday … 7 = Sunday; utc_offset in seconds.
pub type DateTime = {
    year: i64, month: i64, day: i64,
    hour: i64, minute: i64, second: i64, nanosecond: i64,
    weekday: i64, utc_offset: i64,
}

// What a finished program left behind.
pub type Output = { status: i64, stdout: str, stderr: str }

pub extern \"mimic-io-concurrent\" {
    fn now() -> i64
    fn clock() -> i64
    fn sleep(duration: i64)
    fn pid() -> i64
}

pub extern \"mimic-io\" {
    fn random() -> f64
    fn random_int(lo: i64, hi: i64) -> i64
    fn seed_random(seed: i64)
}

pub extern \"mimic\" {
    fn format_time(t: i64) -> str
    fn timestamp(year: i64, month: i64, day: i64, hour: i64, minute: i64, second: i64) -> i64
}

extern \"mimic\" {
    fn time_utc_parts(t: i64) -> [i64]
}

extern \"mimic-io-concurrent\" {
    fn time_local_parts(t: i64) -> [i64]
}

extern \"mimic-io\" {
    fn io_run(program: str, arguments: [str]) -> any
}

fn date_of(p: [i64]) -> DateTime {
    DateTime {
        year: p[0], month: p[1], day: p[2],
        hour: p[3], minute: p[4], second: p[5], nanosecond: p[6],
        weekday: p[7], utc_offset: p[8],
    }
}

// The calendar date and time of instant `t` (from now(), timestamp(…)) in UTC.
pub fn utc(t: i64) -> DateTime { date_of(time_utc_parts(t)) }

// … and in this machine's time zone.
pub fn local(t: i64) -> DateTime { date_of(time_local_parts(t)) }

// Run a program (found through PATH) and wait for it; its output is captured.
pub fn run(program: str, arguments: [str]) -> Result[Output, str] {
    let r = io_run(program, arguments)
    if io_ok() { Ok(Output { status: r[0], stdout: r[1], stderr: r[2] }) } else { Err(io_error()) }
}

// Run a command line through the system shell (/bin/sh -c).
pub fn shell(command: str) -> Result[Output, str] { run(\"/bin/sh\", [\"-c\" command]) }
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
    match parse_file(&src).and_then(|mut ast| mimic_syntax::desugar::lift_arms(&mut ast).map(|_| ast)) {
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
    let mut wrappers: Vec<String> = Vec::new();
    for sf in &mut files {
        let dirs = header_dirs(input, Some(&sf.path));
        let mut errs = Vec::new();
        import_headers(&mut sf.ast.module.items, &dirs, target, &mut errs, &mut wrappers);
        for (msg, span) in errs {
            report.push_in(Some(&sf.path), &sf.display, &sf.src, mimic_syntax::Diag::new(&C::CIMPORT, msg, span));
        }
    }
    if !report.entries.is_empty() {
        return Err(report);
    }
    files.push(prelude_file());
    let (mut program, diags) = Program::build(files, &pkg);
    if !wrappers.is_empty() {
        // one C file with every struct wrapper, named by its content
        let text = wrappers.join("\n");
        let dir = packages::cache_dir().join("cimport");
        let path = dir.join(format!("wrap-{:016x}.c", mimic_syntax::fnv1a(text.as_bytes())));
        if !path.is_file() {
            let _ = fs::create_dir_all(&dir);
            mimic_toolchain::jobs::write_atomic(&path, text.as_bytes()).map_err(|e| Report::plain(&C::CIMPORT, e))?;
        }
        program.c_sources.push(path);
    }
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
    // the compiler flags (target, sysroot, minimum OS, CPU) are part of the key
    let mut flags: Vec<String> = vec!["-c".into(), if release { "-O3" } else { "-O1" }.into(), "-fPIC".into()];
    match cpu {
        Some("native") | None if release && target.name.is_none() => {
            flags.push("-mcpu=native".into()); /* native builds use every feature of this CPU */
        }
        Some("") => {} // portable without a family baseline: the target default
        Some(cpu) if cpu != "native" => {
            // x86 names a CPU with -march; every other architecture with -mcpu
            let x86 = matches!(target.arch.as_str(), "x86_64" | "x86" | "i386" | "i686");
            flags.push(if x86 { format!("-march={cpu}") } else { format!("-mcpu={cpu}") });
        }
        _ => {}
    }
    let base = toolchain::cc(tc, target);
    let base_args: Vec<String> = base.get_args().map(|a| a.to_string_lossy().into_owned()).collect();
    let key = mimic_syntax::fnv1a(format!("{RUNTIME_C}{RUNTIME_H}{RUNTIME_NET_C}{RUNTIME_STD_C}{RUNTIME_IO_C}{}{base_args:?}{flags:?}", target.name()).as_bytes());
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
    mimic_toolchain::jobs::write_atomic(&src_dir.join("mimic_std.c"), RUNTIME_STD_C.as_bytes())?;
    mimic_toolchain::jobs::write_atomic(&src_dir.join("mimic_io.c"), RUNTIME_IO_C.as_bytes())?;
    mimic_toolchain::jobs::write_atomic(&src_dir.join("mimic_rt.h"), RUNTIME_H.as_bytes())?;
    let tmp = obj.with_extension(format!("o.tmp{}", std::process::id()));
    let mut c = toolchain::cc(tc, target);
    c.args(&flags).arg(&src).arg("-o").arg(&tmp);
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

/// What generated callback trampolines need from Mimic's runtime.
const CALLBACK_PRELUDE: &str = "typedef struct mim_val mim_val;\n\
mim_val *mim_call(mim_val *f, int f_argc, mim_val **args);\n\
mim_val *mim_int(long long x);\nmim_val *mim_float(double x);\nmim_val *mim_bool(long long x);\n\
mim_val *mim_from_cstr(const char *s);\nlong long mim_as_i64(mim_val *v);\ndouble mim_as_f64(mim_val *v);\n\
void mim_release(mim_val *v);\n";

/// Where C headers are looked for: next to `file`, the project root, `ffi.include`.
fn header_dirs(input: &Input, file: Option<&Path>) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(f) = file {
        dirs.push(f.parent().filter(|d| !d.as_os_str().is_empty()).map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from(".")));
    }
    if let Input::Project(p) = input {
        dirs.push(p.root.clone());
        dirs.extend(p.manifest.ffi_include.iter().map(|d| p.root.join(d)));
    }
    dirs
}

/// Fill `extern "C" include` blocks (in every module, nested ones too) with the
/// functions the header makes visible. Names the module already defines (its
/// own functions, types, modules, hand-written externs) win over imported ones.
/// Functions taking or returning structs by value get generated Mimic wrappers
/// (added to the module) and C wrappers (collected in `wrappers`); they are
/// only generated for `trusted` imports, since the wrapper hides the C call.
fn import_headers(
    items: &mut Vec<mimic_syntax::ast::Item>,
    dirs: &[PathBuf],
    target: &Target,
    errs: &mut Vec<(String, mimic_syntax::Span)>,
    wrappers: &mut Vec<String>,
) {
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
    let mut generated = String::new();
    for item in items.iter_mut() {
        match item {
            Item::Module(m) => import_headers(&mut m.items, dirs, target, errs, wrappers),
            Item::Extern(b) if !b.includes.is_empty() => {
                for (header, span) in b.includes.clone() {
                    match crate::cimport::import(&header, dirs, target) {
                        Ok(imp) => {
                            for f in imp.extern_fns(b.is_pub, span) {
                                if taken.insert(f.name.clone()) {
                                    b.fns.push(f);
                                }
                            }
                            // callback trampolines: the externs keep their unsafe rule
                            if !imp.callback_c.is_empty() {
                                wrappers.push(format!("{}\n{CALLBACK_PRELUDE}{}", imp.include_line, imp.callback_c));
                            }
                            if !b.trusted {
                                continue; // struct wrappers hide the unsafe call: trusted imports only
                            }
                            let pubkw = if b.is_pub { "pub " } else { "" };
                            let mut c = String::new();
                            for f in &imp.struct_fns {
                                if !taken.insert(f.name.clone()) {
                                    continue;
                                }
                                for r in &f.uses {
                                    let decl = &imp.records[r];
                                    let tname = decl.split_whitespace().nth(2).unwrap_or("").to_string();
                                    if taken.insert(tname) {
                                        generated.push_str(&decl.replacen("PUB ", pubkw, 1));
                                        generated.push('\n');
                                    }
                                }
                                generated.push_str(&f.mimic.replace("PUB ", pubkw));
                                c.push_str(&f.c);
                            }
                            if !c.is_empty() {
                                wrappers.push(format!("{}\n{c}", imp.include_line));
                            }
                        }
                        Err(e) => errs.push((e, span)),
                    }
                }
            }
            _ => {}
        }
    }
    if !generated.is_empty() {
        match mimic_syntax::parser::parse_file(&format!("module generated\n{generated}")) {
            Ok(f) => items.extend(f.module.items),
            Err(d) => errs.push((format!("internal error in generated C bindings: {}\n{generated}", d.msg), d.span)),
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
    if let Some(why) = target.backend_gap() {
        return Err(format!("`{}`: {why}", target.name()));
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
    // eBPF: v4 (Linux 6.6+) is the first generation with signed division,
    // which floored `div` and `%` need
    if target.arch.starts_with("bpf") && cpu.as_deref().is_none_or(|c| c == "generic") {
        cpu = Some("v4".into());
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
    if profile.emit_header {
        let h = dir.join(format!("{name}.h"));
        fs::write(&h, c_header(program, &name)).map_err(|e| format!("cannot write {}: {e}", h.display()))?;
    }
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
    let c_objects = compile_c_sources(input, &program.c_sources, &header_dirs(input, None), &tc, target, profile, &dir)?;
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

/// A C header declaring every `extern "C" fn` of the program.
pub fn c_header(p: &Program, name: &str) -> String {
    use mimic_syntax::ast::Type;
    let cty = |t: &Type, ret: bool| -> String {
        match t {
            Type::Named(n, _) => match n.as_str() {
                "i8" | "i16" | "i32" | "i64" => format!("int{}_t", &n[1..]),
                "u8" | "u16" | "u32" | "u64" => format!("uint{}_t", &n[1..]),
                "isize" => "intptr_t".into(),
                "usize" => "size_t".into(),
                "f32" => "float".into(),
                "f64" => "double".into(),
                "bool" => "bool".into(),
                "c_char" => "char".into(),
                "cstr" if ret => "char *".into(),
                "cstr" => "const char *".into(),
                _ => "void *".into(),
            },
            _ => "void *".into(),
        }
    };
    let guard = format!("{}_H", name.to_uppercase().replace(|c: char| !c.is_ascii_alphanumeric(), "_"));
    let mut out = format!(
        "/* {name}.h: generated by mimic. Functions exported with `extern \"C\" fn`.\n * Strings returned to C are malloc'd copies: free() them. */\n\
         #ifndef {guard}\n#define {guard}\n\n#include <stdbool.h>\n#include <stddef.h>\n#include <stdint.h>\n\n\
         #ifdef __cplusplus\nextern \"C\" {{\n#endif\n\n"
    );
    for f in &p.fns {
        let mimic_check::FnKind::User(d) = &f.kind else { continue };
        if !d.export_c || p.mods[f.module].package != p.root_package {
            continue;
        }
        let ret = d.ret.as_ref().map(|t| cty(t, true)).unwrap_or_else(|| "void".into());
        let params: Vec<String> = d.params.iter().map(|prm| format!("{} {}", prm.ty.as_ref().map(|t| cty(t, false)).unwrap_or_default(), prm.name)).collect();
        out.push_str(&format!("{ret} {}({});\n", d.name, if params.is_empty() { "void".into() } else { params.join(", ") }));
    }
    out.push_str("\n#ifdef __cplusplus\n}\n#endif\n\n#endif\n");
    out
}

/// Compile the project's C sources (`ffi { sources = [...] }`) for the target,
/// in parallel; an object is reused while it is newer than its source and was
/// built with the same flags.
fn compile_c_sources(
    input: &Input,
    extra: &[PathBuf],
    include_dirs: &[PathBuf],
    tc: &Toolchain,
    target: &Target,
    profile: &Profile,
    dir: &Path,
) -> Result<Vec<PathBuf>, String> {
    let (mut sources, mut cflags): (Vec<(String, PathBuf)>, Vec<String>) = (Vec::new(), Vec::new());
    if let Input::Project(p) = input {
        sources.extend(p.manifest.ffi_sources.iter().map(|s| (s.clone(), p.root.join(s))));
        cflags.extend(p.manifest.ffi_cflags.iter().cloned());
    }
    sources.extend(extra.iter().map(|e| (e.file_name().unwrap_or_default().to_string_lossy().into_owned(), e.clone())));
    if sources.is_empty() {
        return Ok(Vec::new());
    }
    let out_dir = dir.join("c");
    fs::create_dir_all(&out_dir).map_err(|e| format!("cannot create {}: {e}", out_dir.display()))?;
    let mut flags: Vec<String> = vec![if profile.release { "-O2".into() } else { "-O0".into() }, "-g".into()];
    flags.extend(include_dirs.iter().filter(|d| !d.as_os_str().is_empty()).map(|d| format!("-I{}", d.display())));
    flags.extend(cflags);
    let stamp = format!("{:016x}", mimic_syntax::fnv1a(format!("{flags:?}{:?}", target.llvm).as_bytes()));
    let jobs: Vec<(PathBuf, PathBuf)> = sources
        .into_iter()
        .map(|(s, src)| {
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
