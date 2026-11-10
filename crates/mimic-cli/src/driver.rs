//! Compilation pipeline: load sources → parse → check → LLVM codegen →
//! compile runtime with `zig cc` → link with `zig cc`.

use crate::targets::Target;
use mimic_check::{FileDiag, Options, Program, SourceFile};
use mimic_project::{MANIFEST, Project, packages};
use mimic_syntax::parser::parse_file;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

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
}

impl Profile {
    pub fn for_input(input: &Input, release: bool) -> Profile {
        let cfg = match input {
            Input::Project(p) => p.build.clone(),
            Input::File(_) => mimic_project::BuildConfig::default(),
        };
        let p = if release { cfg.release } else { cfg.debug };
        Profile { release, net: true, opt: p.opt, leak_check: p.leak_check }
    }
    pub fn name(&self) -> &'static str {
        if self.release { "release" } else { "debug" }
    }
}

fn display_path(root: Option<&Path>, p: &Path) -> String {
    root.and_then(|r| p.strip_prefix(r).ok()).unwrap_or(p).display().to_string()
}

fn read_parse(path: &Path, display: String, package: &str, errors: &mut String) -> Option<SourceFile> {
    let src = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            errors.push_str(&format!("error: cannot read {}: {e}\n", path.display()));
            return None;
        }
    };
    match parse_file(&src) {
        Ok(ast) => Some(SourceFile { path: path.to_path_buf(), display, src, ast, package: package.into() }),
        Err(d) => {
            errors.push_str(&d.render(Path::new(&display), &src));
            None
        }
    }
}

pub fn render(p: &Program, diags: &[FileDiag]) -> String {
    let mut out = String::new();
    for d in diags {
        let f = &p.files[d.file];
        out.push_str(&d.diag.render(Path::new(&f.display), &f.src));
        out.push('\n');
    }
    out
}

fn count_errors(s: &str) -> usize {
    s.matches("error: ").count()
}

/// Load, parse, resolve and check. Errors come back fully rendered.
pub fn load(input: &Input) -> Result<Program, String> {
    let mut errors = String::new();
    let mut files = Vec::new();
    let pkg = input.name();
    match input {
        Input::File(f) => {
            if let Some(sf) = read_parse(f, f.display().to_string(), &pkg, &mut errors) {
                if sf.ast.module.name != "main" {
                    errors.push_str(&format!(
                        "error: `mimic run <file>` expects the file to declare `module main` (found `module {}`)\n",
                        sf.ast.module.name
                    ));
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
                if let Some(sf) = read_parse(&path, display_path(Some(&p.root), &path), &pkg, &mut errors) {
                    files.push(sf);
                }
            }
            if !p.manifest.bin && !files.iter().any(|f| f.ast.module.name == "lib") {
                errors.push_str("error: library projects need src/lib.mim with `module lib`\n");
            }
            // dependencies (transitively), compiled from their library sources
            let mut seen = BTreeSet::from([pkg.clone()]);
            let mut queue: Vec<(PathBuf, mimic_project::Dependency)> =
                p.manifest.dependencies.iter().map(|d| (p.root.clone(), d.clone())).collect();
            while let Some((from_root, dep)) = queue.pop() {
                if !seen.insert(dep.name.clone()) {
                    continue;
                }
                let holder = Project::load(&from_root)?;
                let dir = holder.dep_dir(&dep)?;
                let dp = Project::load(&dir)?;
                if dp.manifest.name != dep.name {
                    return Err(format!(
                        "error: dependency `{}` resolves to {}, whose mim.mim names it `{}`",
                        dep.name,
                        dir.display(),
                        dp.manifest.name
                    ));
                }
                if !dp.manifest.lib {
                    return Err(format!("error: dependency `{}` is not a library", dep.name));
                }
                for path in dp.sources()? {
                    let fname = path.file_name().unwrap().to_string_lossy().to_string();
                    if path.parent() == Some(&dir.join("src")) && (fname == "main.mim" || fname == "test.mim") {
                        continue;
                    }
                    let shown = format!("{}/{}", dep.name, display_path(Some(&dir), &path));
                    if let Some(sf) = read_parse(&path, shown, &dep.name, &mut errors) {
                        files.push(sf);
                    }
                }
                for d in &dp.manifest.dependencies {
                    queue.push((dir.clone(), d.clone()));
                }
            }
        }
    }
    if !errors.is_empty() {
        return Err(summarize(errors));
    }
    let (program, diags) = Program::build(files, &pkg);
    if !diags.is_empty() {
        return Err(summarize(render(&program, &diags)));
    }
    let diags = mimic_check::check(&program, &Options { require_main: input.is_bin() });
    if !diags.is_empty() {
        return Err(summarize(render(&program, &diags)));
    }
    Ok(program)
}

fn summarize(mut errors: String) -> String {
    let n = count_errors(&errors).max(1);
    errors.push_str(&format!("error: could not compile due to {n} previous error{}", if n == 1 { "" } else { "s" }));
    errors
}

// ---- toolchain --------------------------------------------------------------

fn zig() -> Option<PathBuf> {
    if let Some(z) = std::env::var_os("MIMIC_ZIG") {
        return Some(PathBuf::from(z));
    }
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).map(|d| d.join(if cfg!(windows) { "zig.exe" } else { "zig" })).find(|p| p.is_file())
}

/// The C driver: `zig cc` (cross-capable) or the system `cc` for native builds.
pub fn cc(target: &Target) -> Result<Command, String> {
    match zig() {
        Some(z) => {
            let mut c = Command::new(z);
            c.arg("cc");
            if let Some(t) = &target.zig {
                c.args(["-target", t]);
            }
            Ok(c)
        }
        None if target.zig.is_none() => Ok(Command::new("cc")),
        None => Err("cross-compilation needs Zig: install it (https://ziglang.org) or set MIMIC_ZIG".into()),
    }
}

pub fn run_tool(mut c: Command, what: &str) -> Result<(), String> {
    let out = c.output().map_err(|e| format!("cannot run {what}: {e}"))?;
    if !out.status.success() {
        return Err(format!("{what} failed:\n{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr)));
    }
    Ok(())
}

/// Compile (and cache) the C runtime for a target.
pub fn runtime_object(target: &Target, release: bool) -> Result<PathBuf, String> {
    let key = mimic_syntax::fnv1a(format!("{RUNTIME_C}{RUNTIME_H}{RUNTIME_NET_C}{}{release}", target.name()).as_bytes());
    let dir = packages::cache_dir().join("runtime");
    fs::create_dir_all(&dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    let obj = dir.join(format!("mimic_rt-{}-{key:016x}.o", target.name()));
    if obj.is_file() {
        return Ok(obj);
    }
    // each runtime version gets its own directory: mimic_rt.c #includes mimic_net.c
    let src_dir = dir.join(format!("src-{key:016x}"));
    fs::create_dir_all(&src_dir).map_err(|e| e.to_string())?;
    let src = src_dir.join("mimic_rt.c");
    fs::write(&src, RUNTIME_C).map_err(|e| e.to_string())?;
    fs::write(src_dir.join("mimic_net.c"), RUNTIME_NET_C).map_err(|e| e.to_string())?;
    fs::write(dir.join("mimic_rt.h"), RUNTIME_H).map_err(|e| e.to_string())?;
    let tmp = obj.with_extension(format!("o.tmp{}", std::process::id()));
    let mut c = cc(target)?;
    c.args(["-c", if release { "-O3" } else { "-O1" }, "-fPIC"]);
    if release && target.zig.is_none() {
        c.arg("-mcpu=native"); /* native builds use every feature of this CPU */
    }
    c.arg(&src).arg("-o").arg(&tmp);
    run_tool(c, "compiling the mimic runtime")?;
    fs::rename(&tmp, &obj).map_err(|e| e.to_string())?;
    Ok(obj)
}

pub struct Artifact {
    pub path: PathBuf,
    pub is_exe: bool,
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
    if target.zig.is_some() {
        d = d.join(target.name());
    }
    d
}

pub fn build(input: &Input, profile: &Profile, target: &Target, emit_ir: bool, emit_net: bool) -> Result<Artifact, String> {
    let program = load(input)?;
    if let Input::Project(p) = input {
        crate::modfile::sync(p, &program)?;
    }
    let dir = out_dir(input, profile, target);
    fs::create_dir_all(&dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    let name = input.name();
    let obj = dir.join(format!("{name}.o"));
    let ir = dir.join(format!("{name}.ll"));
    let net = dir.join(format!("{name}.mnet"));
    let is_exe = input.is_bin();
    mimic_codegen::compile(
        &program,
        &mimic_codegen::Settings {
            triple: target.llvm.clone(),
            opt_level: profile.opt,
            leak_check: profile.leak_check,
            emit_main: is_exe,
            export_pub: !is_exe,
            use_net: profile.net,
        },
        &obj,
        emit_ir.then_some(ir.as_path()),
        emit_net.then_some(net.as_path()),
    )?;
    let rt = runtime_object(target, profile.release)?;
    if is_exe {
        let exe = dir.join(format!("{name}{}", target.exe_suffix()));
        let mut c = cc(target)?;
        c.arg(&obj).arg(&rt).arg("-o").arg(&exe);
        if let Input::Project(p) = input {
            for s in &p.manifest.ffi_search {
                c.arg(format!("-L{}", p.root.join(s).display()));
            }
            for l in &p.manifest.ffi_link {
                c.arg(format!("-l{l}"));
            }
        }
        if target.needs_libm() {
            c.arg("-lm");
        }
        if !profile.release {
            c.arg("-g");
        }
        run_tool(c, "linking")?;
        Ok(Artifact { path: exe, is_exe: true })
    } else {
        // library: static archive of the program object + runtime
        let lib = dir.join(format!("lib{name}.a"));
        let _ = fs::remove_file(&lib);
        let mut c = match zig() {
            Some(z) => {
                let mut c = Command::new(z);
                c.arg("ar");
                c
            }
            None => Command::new("ar"),
        };
        c.arg("rcs").arg(&lib).arg(&obj).arg(&rt);
        run_tool(c, "archiving")?;
        Ok(Artifact { path: lib, is_exe: false })
    }
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
    let rt = runtime_object(&target, release)?;
    let exe = dir.join(&stem);
    let mut c = cc(&target)?;
    c.arg(if release { "-O2" } else { "-O0" }).arg(&c_src).arg(&rt).arg("-o").arg(&exe);
    if target.needs_libm() {
        c.arg("-lm");
    }
    run_tool(c, "compiling the net")?;
    Ok(exe)
}
