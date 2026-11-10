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
}

pub fn select(target: &Target) -> Result<Toolchain, String> {
    let forced = std::env::var("MIMIC_TOOLCHAIN").ok();
    if forced.as_deref() != Some("zig") && mimic_toolchain::EMBEDDED && let Some(s) = embedded_sysroot(target) {
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

fn embedded_sysroot(target: &Target) -> Option<Sysroot> {
    // Phase A step 1: native macOS builds. Linux (musl), Windows, WASI and the
    // BSDs follow as their libcs are bundled.
    if target.zig.is_none() && cfg!(target_os = "macos") {
        let sdk = macos_sdk()?;
        let sdk_version = tool_output("xcrun", &["--sdk", "macosx", "--show-sdk-version"]).unwrap_or_else(|| "11.0".into());
        let min_os = tool_output("sw_vers", &["-productVersion"]).unwrap_or_else(|| "11.0".into());
        return Some(Sysroot::Darwin { sdk, min_os, sdk_version });
    }
    None
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

/// Clang's own headers (stddef.h, stdarg.h, arm_neon.h, …).
fn resource_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("MIMIC_CLANG_RESOURCE_DIR") {
        return PathBuf::from(d);
    }
    // until the headers are bundled (Phase A3): the LLVM the compiler was built with
    PathBuf::from(mimic_toolchain::LLVM_PREFIX).join("lib/clang").join(mimic_toolchain::LLVM_MAJOR)
}

/// A C compiler command for `target`, ready for `-c src -o obj` and flags.
pub fn cc(tc: &Toolchain, target: &Target) -> Command {
    match tc {
        Toolchain::Embedded(sys) => {
            let mut c = Command::new(self_exe());
            c.arg("cc").arg("-resource-dir").arg(resource_dir());
            if let Some(t) = &target.llvm {
                c.arg(format!("--target={t}"));
            }
            match sys {
                Sysroot::Darwin { sdk, min_os, .. } => {
                    c.arg("-isysroot").arg(sdk).arg(format!("-mmacosx-version-min={min_os}"));
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
