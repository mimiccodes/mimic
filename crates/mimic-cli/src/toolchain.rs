//! Compiling C and linking with the tools embedded in `mimic` itself:
//! Clang (`mimic cc`) and LLD (`mimic ld64.lld`, `mimic ld.lld`, …).
//!
//! Targets move here from Zig one family at a time; a target the embedded
//! toolchain cannot handle yet falls back to `zig cc` when Zig is installed.
//! Set `MIMIC_TOOLCHAIN=zig` to force the old path (for comparison).

use crate::targets::Target;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Which toolchain builds and links for a target.
pub enum Toolchain {
    /// Clang and LLD inside this binary.
    Embedded(Sysroot),
    /// `zig cc` (legacy fallback for targets not yet embedded).
    Zig(PathBuf),
    /// The system C compiler (host only, when nothing else is available).
    System,
}

/// What the embedded tools need to know about a target's libc.
pub enum Sysroot {
    /// macOS: an SDK (headers and libSystem stubs).
    Darwin { sdk: PathBuf, min_os: String, sdk_version: String },
    /// Linux with musl, built from the bundled sources: fully static executables.
    Musl { triple: String, root: PathBuf, compiler_rt: PathBuf },
}

pub fn select(target: &Target) -> Result<Toolchain, String> {
    let forced = std::env::var("MIMIC_TOOLCHAIN").ok();
    if forced.as_deref() != Some("zig") && mimic_toolchain::EMBEDDED && let Some(s) = embedded_sysroot(target)? {
        return Ok(Toolchain::Embedded(s));
    }
    if let Some(z) = zig() {
        return Ok(Toolchain::Zig(z));
    }
    if target.zig.is_none() {
        return Ok(Toolchain::System);
    }
    Err(format!(
        "target `{}` is not yet supported by the embedded toolchain; install Zig (https://ziglang.org) or set MIMIC_ZIG for now",
        target.name()
    ))
}

fn embedded_sysroot(target: &Target) -> Result<Option<Sysroot>, String> {
    if target.zig.is_none() && cfg!(target_os = "macos") {
        let Some(sdk) = macos_sdk() else { return Ok(None) };
        let sdk_version = tool_output("xcrun", &["--sdk", "macosx", "--show-sdk-version"]).unwrap_or_else(|| "11.0".into());
        let min_os = tool_output("sw_vers", &["-productVersion"]).unwrap_or_else(|| "11.0".into());
        return Ok(Some(Sysroot::Darwin { sdk, min_os, sdk_version }));
    }
    if let Some(triple) = &target.llvm
        && target.os == "linux"
        && triple.rsplit('-').next().is_some_and(|abi| abi.starts_with("musl"))
        && mimic_toolchain::musl::arch(triple).is_some()
    {
        let cache = libc_cache();
        let musl = mimic_toolchain::musl::ensure(&cache, triple)?;
        let mut flags = mimic_toolchain::musl::target_flags(triple);
        flags.push(format!("--sysroot={}", musl.root.display()));
        flags.push("-resource-dir".into());
        flags.push(mimic_toolchain::resource_dir(&cache)?.display().to_string());
        let compiler_rt = mimic_toolchain::compiler_rt::ensure(&cache, triple, &flags)?;
        return Ok(Some(Sysroot::Musl { triple: triple.clone(), root: musl.root, compiler_rt }));
    }
    Ok(None)
}

/// Where target libraries (libc, compiler-rt, headers) are built and kept.
fn libc_cache() -> PathBuf {
    mimic_project::packages::cache_dir().join("libc")
}

fn tool_output(cmd: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(cmd).args(args).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string()).filter(|s| !s.is_empty())
}

/// The macOS SDK: `$SDKROOT`, else what `xcrun` reports.
fn macos_sdk() -> Option<PathBuf> {
    if let Some(s) = std::env::var_os("SDKROOT").map(PathBuf::from).filter(|p| p.is_dir()) {
        return Some(s);
    }
    tool_output("xcrun", &["--sdk", "macosx", "--show-sdk-path"]).map(PathBuf::from).filter(|p| p.is_dir())
}

pub fn zig() -> Option<PathBuf> {
    if let Some(z) = std::env::var_os("MIMIC_ZIG") {
        return Some(PathBuf::from(z));
    }
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).map(|d| d.join(if cfg!(windows) { "zig.exe" } else { "zig" })).find(|p| p.is_file())
}

fn self_exe() -> PathBuf {
    std::env::current_exe().unwrap_or_else(|_| PathBuf::from("mimic"))
}

/// Clang's own headers (stddef.h, stdarg.h, arm_neon.h, …), bundled in the binary.
fn resource_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("MIMIC_CLANG_RESOURCE_DIR") {
        return PathBuf::from(d);
    }
    mimic_toolchain::resource_dir(&libc_cache())
        .unwrap_or_else(|_| PathBuf::from(mimic_toolchain::LLVM_PREFIX).join("lib/clang").join(mimic_toolchain::LLVM_MAJOR))
}

/// A C compiler command for `target`, ready for `-c src -o obj` and flags.
pub fn cc(tc: &Toolchain, target: &Target) -> Command {
    match tc {
        Toolchain::Embedded(sys) => {
            let mut c = Command::new(self_exe());
            c.arg("cc").arg("-resource-dir").arg(resource_dir());
            if let Some(t) = &target.llvm
                && !matches!(sys, Sysroot::Musl { .. })
            {
                c.arg(format!("--target={t}"));
            }
            match sys {
                Sysroot::Darwin { sdk, min_os, .. } => {
                    c.arg("-isysroot").arg(sdk).arg(format!("-mmacosx-version-min={min_os}"));
                }
                Sysroot::Musl { triple, root, .. } => {
                    c.args(mimic_toolchain::musl::target_flags(triple)).arg(format!("--sysroot={}", root.display()));
                }
            }
            c
        }
        Toolchain::Zig(z) => {
            let mut c = Command::new(z);
            c.arg("cc");
            if let Some(t) = &target.zig {
                c.args(["-target", t]);
            }
            c
        }
        Toolchain::System => Command::new("cc"),
    }
}

/// What to link into an executable.
pub struct LinkJob<'a> {
    pub objects: &'a [PathBuf],
    pub output: &'a Path,
    pub search: Vec<PathBuf>,
    pub libs: Vec<String>,
    pub debug: bool,
}

/// The command that links `job` for `target`.
pub fn link(tc: &Toolchain, target: &Target, job: &LinkJob) -> Command {
    match tc {
        Toolchain::Embedded(Sysroot::Darwin { sdk, min_os, sdk_version }) => {
            let arch = match target.llvm.as_deref().map(|t| t.split('-').next().unwrap_or("")) {
                Some("x86_64") => "x86_64",
                Some(_) => "arm64",
                None if cfg!(target_arch = "x86_64") => "x86_64",
                None => "arm64",
            };
            let mut c = Command::new(self_exe());
            c.arg("ld64.lld")
                .args(["-arch", arch])
                .args(["-platform_version", "macos", min_os, sdk_version])
                .arg("-syslibroot")
                .arg(sdk)
                .arg("-dead_strip")
                .arg("-o")
                .arg(job.output)
                .args(job.objects);
            for s in &job.search {
                c.arg(format!("-L{}", s.display()));
            }
            for l in &job.libs {
                c.arg(format!("-l{l}"));
            }
            c.arg("-lSystem");
            c
        }
        Toolchain::Embedded(Sysroot::Musl { root, compiler_rt, .. }) => {
            let lib = root.join("usr/lib");
            let mut c = Command::new(self_exe());
            c.arg("ld.lld").arg("-static").arg("--gc-sections").arg("-o").arg(job.output);
            c.arg(lib.join("crt1.o")).arg(lib.join("crti.o")).args(job.objects);
            c.arg(format!("-L{}", lib.display()));
            for s in &job.search {
                c.arg(format!("-L{}", s.display()));
            }
            c.arg("--start-group");
            for l in &job.libs {
                c.arg(format!("-l{l}"));
            }
            c.arg(lib.join("libc.a")).arg(compiler_rt).arg("--end-group");
            c.arg(lib.join("crtn.o"));
            c
        }
        Toolchain::Zig(_) | Toolchain::System => {
            let mut c = cc(tc, target);
            c.args(job.objects).arg("-o").arg(job.output);
            for s in &job.search {
                c.arg(format!("-L{}", s.display()));
            }
            for l in &job.libs {
                c.arg(format!("-l{l}"));
            }
            if target.needs_libm() {
                c.arg("-lm");
            }
            if job.debug {
                c.arg("-g");
            }
            c
        }
    }
}

/// Write a static library (`.a`) containing `objects`.
pub fn archive(tc: &Toolchain, target: &Target, output: &Path, objects: &[PathBuf]) -> Result<(), String> {
    if mimic_toolchain::EMBEDDED {
        let kind = match target.os.as_str() {
            "macos" | "ios" | "tvos" | "watchos" | "visionos" | "maccatalyst" | "driverkit" => mimic_toolchain::ArchiveKind::Darwin,
            "windows" | "uefi" => mimic_toolchain::ArchiveKind::Coff,
            _ => mimic_toolchain::ArchiveKind::Gnu,
        };
        return mimic_toolchain::write_archive(output, kind, objects);
    }
    let mut c = match tc {
        Toolchain::Zig(z) => {
            let mut c = Command::new(z);
            c.arg("ar");
            c
        }
        _ => Command::new("ar"),
    };
    c.arg("rcs").arg(output).args(objects);
    run_tool(c, "archiving")
}

pub fn run_tool(mut c: Command, what: &str) -> Result<(), String> {
    let out = c.output().map_err(|e| format!("cannot run {what}: {e}"))?;
    if !out.status.success() {
        return Err(format!("{what} failed:\n{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr)));
    }
    Ok(())
}

/// A short name for messages and cache keys.
pub fn describe(tc: &Toolchain) -> &'static str {
    match tc {
        Toolchain::Embedded(_) => "embedded",
        Toolchain::Zig(_) => "zig",
        Toolchain::System => "system",
    }
}

/// The CPU, features and ABI Clang uses for `triple` by default, so Mimic code
/// matches the C code (runtime, libc) it is linked with. Cached per triple.
pub fn clang_defaults(triple: &str) -> Option<(String, String, Option<String>)> {
    if !mimic_toolchain::EMBEDDED {
        return None;
    }
    let cache = libc_cache().join("targets");
    let file = cache.join(format!("{triple}.txt"));
    let text = match std::fs::read_to_string(&file) {
        Ok(t) => t,
        Err(_) => {
            let out = Command::new(self_exe())
                .args(["cc", &format!("--target={triple}"), "-###", "-c", "-x", "c", "/dev/null", "-o", "/dev/null"])
                .output()
                .ok()?;
            let all = String::from_utf8_lossy(&out.stderr).into_owned();
            let line = all.lines().rev().find(|l| l.contains("\"-cc1\""))?.to_string();
            let words: Vec<String> = line.split("\" \"").map(|w| w.trim_matches([' ', '"']).to_string()).collect();
            let mut cpu = String::from("generic");
            let mut feats = Vec::new();
            let mut abi = String::new();
            for w in words.windows(2) {
                match w[0].as_str() {
                    "-target-cpu" => cpu = w[1].clone(),
                    // clang-only pseudo-feature; the float ABI comes from the triple
                    "-target-feature" if w[1] == "+soft-float-abi" => {}
                    "-target-feature" => feats.push(w[1].clone()),
                    "-target-abi" => abi = w[1].clone(),
                    _ => {}
                }
            }
            let t = format!("{cpu}\n{}\n{abi}\n", feats.join(","));
            let _ = std::fs::create_dir_all(&cache);
            let _ = mimic_toolchain::jobs::write_atomic(&file, t.as_bytes());
            t
        }
    };
    let mut l = text.lines();
    let cpu = l.next()?.to_string();
    let features = l.next().unwrap_or("").to_string();
    let abi = l.next().filter(|a| !a.is_empty()).map(String::from);
    Some((cpu, features, abi))
}
