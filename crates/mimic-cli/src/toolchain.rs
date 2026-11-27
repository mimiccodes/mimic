//! Compiling C and linking with the tools embedded in `mimic` itself:
//! Clang (`mimic cc`) and LLD (`mimic ld64.lld`, `mimic ld.lld`, …), plus the
//! C libraries `mimic` builds from bundled sources.
//!
//! Every target LLVM can generate code for can be compiled to an object;
//! a complete executable also needs a C library, which `support` reports.

use crate::targets::Target;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Which toolchain builds and links for a target.
pub enum Toolchain {
    /// Clang and LLD inside this binary.
    Embedded(Sysroot),
    /// The system C compiler (host only, when nothing else is available).
    System,
}

/// What the embedded tools need to know about a target's libc.
pub enum Sysroot {
    /// Apple platforms: an SDK (headers and libSystem stubs) from Xcode.
    /// `platform` is ld64's name for it (`macos`, `ios-simulator`, …).
    Darwin { sdk: PathBuf, min_os: String, sdk_version: String, platform: &'static str },
    /// Linux with musl, built from the bundled sources: fully static executables.
    Musl { triple: String, root: PathBuf, compiler_rt: PathBuf },
    /// WebAssembly with WASI (preview 1): wasi-libc, a `.wasm` command module.
    Wasi { root: PathBuf, compiler_rt: PathBuf },
    /// Windows (x86_64, aarch64) with mingw-w64 and the UCRT: `.exe` files
    /// that need only Windows' own DLLs.
    Mingw(mimic_toolchain::mingw::Sysroot),
}

pub fn select(target: &Target) -> Result<Toolchain, String> {
    if mimic_toolchain::EMBEDDED
        && let Some(s) = embedded_sysroot(target)?
    {
        return Ok(Toolchain::Embedded(s));
    }
    if target.name.is_none() {
        return Ok(Toolchain::System);
    }
    Err(match support(target) {
        Support::NotYet(why) => format!("cannot build executables for `{}` yet: {why} (run `mimic targets`)", target.name()),
        _ => format!("cannot build executables for `{}`: this mimic was built without its embedded toolchain", target.name()),
    })
}

/// How well a target is proven. Kept in step with what the golden tests check.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Tier {
    /// Programs run on the development machine and pass every golden test.
    Stable,
    /// A program ran on the target (simulator), not the whole suite.
    Runs,
    /// Complete executables are built and checked, but not run yet.
    Links,
    /// Object files (or assembly) only: no bundled C library yet.
    Compiles,
    /// The architecture needs the GPU backend (roadmap F8).
    NotYet,
}

impl Tier {
    pub fn as_str(self) -> &'static str {
        match self {
            Tier::Stable => "stable",
            Tier::Runs => "runs",
            Tier::Links => "links",
            Tier::Compiles => "compiles",
            Tier::NotYet => "not yet",
        }
    }
    pub fn meaning(self) -> &'static str {
        match self {
            Tier::Stable => "programs run and pass every golden test",
            Tier::Runs => "a program ran on it (simulator); the full test suite not yet",
            Tier::Links => "complete executables are built and checked, not run yet",
            Tier::Compiles => "object files or assembly only; no C library bundled yet",
            Tier::NotYet => "GPU architecture: needs the GPU backend (roadmap F8)",
        }
    }
}

pub fn tier(target: &Target) -> Tier {
    if target.is_device_only() || target.backend_gap().is_some() {
        return Tier::NotYet;
    }
    let arch = target.arch.as_str();
    let apple_arm = matches!(arch, "aarch64" | "arm64");
    match (target.os.as_str(), target.env.as_str()) {
        // native builds, and the targets whose golden tests run (here and in
        // CI): macOS arm64 and x86_64 (Rosetta), Mac Catalyst, WASI (Node),
        // static Linux x86_64 and arm64, Windows x86_64 and arm64 (CI runs the
        // golden programs on a Windows arm64 machine)
        _ if target.name.is_none() => Tier::Stable,
        ("macosx", _) if apple_arm || arch == "x86_64" => Tier::Stable,
        ("ios", "macabi") if apple_arm => Tier::Stable,
        ("wasi" | "wasip1", _) if arch == "wasm32" => Tier::Stable,
        ("linux", "musl") if matches!(arch, "x86_64" | "aarch64") => Tier::Stable,
        ("windows", "gnu") if matches!(arch, "x86_64" | "aarch64") => Tier::Stable,
        ("ios", "simulator") if apple_arm => Tier::Runs,
        _ => match support(target) {
            Support::Bundled | Support::Sdk => Tier::Links,
            Support::NotYet(_) => Tier::Compiles,
        },
    }
}

/// How `mimic` provides the C library for a target.
pub enum Support {
    /// Built from sources bundled in `mimic` (musl, …).
    Bundled,
    /// The platform SDK found on this machine (macOS).
    Sdk,
    /// Not available yet; the reason.
    NotYet(String),
}

pub fn support(target: &Target) -> Support {
    let triple = target.llvm.clone().unwrap_or_default();
    if target.os == "linux" && target.env.starts_with("musl") {
        return if mimic_toolchain::musl::arch(&triple).is_some() || target.name.is_none() {
            Support::Bundled
        } else if target.arch == "riscv32" {
            Support::NotYet("musl's 128-bit long double needs quad-precision soft-float, not available for riscv32 yet".into())
        } else {
            Support::NotYet(format!("musl has no port for {}", target.arch))
        };
    }
    if let Some((sdk, _)) = apple_platform(target)
        && cfg!(target_os = "macos")
        && xcode_sdk(sdk).is_some()
    {
        return Support::Sdk;
    }
    if is_wasi(target) || is_mingw(target) {
        return Support::Bundled;
    }
    let os = if target.os.is_empty() { "unknown" } else { target.os.as_str() };
    let what = match (os, target.env.as_str()) {
        ("linux", e) if e.starts_with("gnu") => "glibc stubs are not bundled yet",
        ("linux", e) if e.starts_with("android") => "the Android NDK libc is not bundled yet",
        ("windows", _) => "mingw-w64 is not bundled yet",
        ("wasi", _) => "wasi-libc is not bundled yet",
        ("freebsd" | "netbsd" | "openbsd" | "dragonfly", _) => "BSD libc stubs are not bundled yet",
        ("macosx" | "ios" | "tvos" | "watchos" | "xros" | "driverkit", _) => "the Apple SDK for it is not installed (Xcode), and SDK stubs are not bundled yet",
        ("cuda" | "amdhsa" | "amdpal" | "mesa3d" | "nvcl" | "vulkan" | "opencl" | "shadermodel", _) => {
            "a GPU target: mimic produces device code, to be loaded by a host program"
        }
        ("unknown", _) => "no operating system: there is no C library for the runtime",
        _ => "no C library for this OS is bundled",
    };
    Support::NotYet(what.into())
}

fn embedded_sysroot(target: &Target) -> Result<Option<Sysroot>, String> {
    if let Some((sdk_name, platform)) = apple_platform(target)
        && cfg!(target_os = "macos")
    {
        let Some(sdk) = xcode_sdk(sdk_name) else { return Ok(None) };
        let sdk_version = sdk_version(&sdk).or_else(|| tool_output("xcrun", &["--sdk", sdk_name, "--show-sdk-version"])).unwrap_or_else(|| "11.0".into());
        // native builds target this machine's OS; cross builds the triple's minimum
        let min_os = match &target.llvm {
            Some(t) => t
                .split('-')
                .nth(2)
                .map(|os| os.trim_start_matches(|c: char| c.is_ascii_alphabetic()).to_string())
                .filter(|v| !v.is_empty()),
            // this machine's OS, major version (what Clang targets natively)
            None => macos_version().or_else(|| tool_output("sw_vers", &["-productVersion"])).map(|v| format!("{}.0", v.split('.').next().unwrap_or("11"))),
        }
        .unwrap_or_else(|| "11.0".into());
        return Ok(Some(Sysroot::Darwin { sdk, min_os, sdk_version, platform }));
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
    if is_mingw(target) {
        let triple = target.llvm.clone().unwrap_or_default();
        return Ok(Some(Sysroot::Mingw(mimic_toolchain::mingw::ensure(&libc_cache(), &triple)?)));
    }
    if is_wasi(target) {
        let cache = libc_cache();
        let w = mimic_toolchain::wasi::ensure(&cache, &mimic_toolchain::resource_dir(&cache)?)?;
        return Ok(Some(Sysroot::Wasi { root: w.root, compiler_rt: w.compiler_rt }));
    }
    Ok(None)
}

/// Mbed TLS for a static Linux target (built on first use), which the runtime's
/// TLS uses there instead of loading OpenSSL. `None` for other targets, or
/// when it cannot be built (TLS then reports that it is not available).
pub fn static_tls(tc: &Toolchain) -> Option<mimic_toolchain::mbedtls::Built> {
    let Toolchain::Embedded(Sysroot::Musl { triple, root, .. }) = tc else { return None };
    let cache = libc_cache();
    let mut flags = mimic_toolchain::musl::target_flags(triple);
    flags.push(format!("--sysroot={}", root.display()));
    flags.push("-resource-dir".into());
    flags.push(mimic_toolchain::resource_dir(&cache).ok()?.display().to_string());
    match mimic_toolchain::mbedtls::ensure(&cache, triple, &flags) {
        Ok(b) => Some(b),
        Err(e) => {
            eprintln!("note: {e}; TLS will not be available in this program");
            None
        }
    }
}

/// Windows with the GNU environment on x86_64 or aarch64: what the bundled
/// mingw-w64 covers.
fn is_mingw(target: &Target) -> bool {
    target.os == "windows"
        && target.env.starts_with("gnu")
        && target.llvm.as_deref().and_then(mimic_toolchain::mingw::arch).is_some()
}

/// WASI preview 1 on wasm32: what wasi-libc covers.
fn is_wasi(target: &Target) -> bool {
    target.arch == "wasm32" && matches!(target.os.as_str(), "wasi" | "wasip1")
}

/// Where target libraries (libc, compiler-rt, headers) are built and kept.
fn libc_cache() -> PathBuf {
    mimic_project::packages::cache_dir().join("libc")
}

/// The file that reads as empty and swallows writes.
const NULL_DEVICE: &str = if cfg!(windows) { "NUL" } else { "/dev/null" };

fn tool_output(cmd: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(cmd).args(args).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string()).filter(|s| !s.is_empty())
}

/// (Xcode SDK name, ld64 platform name) of an Apple target.
fn apple_platform(target: &Target) -> Option<(&'static str, &'static str)> {
    let sim = target.env == "simulator";
    Some(match target.os.as_str() {
        "macosx" | "darwin" => ("macosx", "macos"),
        "ios" if target.env == "macabi" => ("macosx", "mac-catalyst"),
        "ios" if sim => ("iphonesimulator", "ios-simulator"),
        "ios" => ("iphoneos", "ios"),
        "tvos" if sim => ("appletvsimulator", "tvos-simulator"),
        "tvos" => ("appletvos", "tvos"),
        "watchos" if sim => ("watchsimulator", "watchos-simulator"),
        "watchos" => ("watchos", "watchos"),
        "xros" if sim => ("xrsimulator", "xros-simulator"),
        "xros" => ("xros", "xros"),
        "driverkit" => ("driverkit", "driverkit"),
        _ => return None,
    })
}

/// An Xcode SDK: `$SDKROOT` for macOS, else what `xcrun` reports.
pub(crate) fn xcode_sdk(name: &str) -> Option<PathBuf> {
    if name == "macosx"
        && let Some(s) = std::env::var_os("SDKROOT").map(PathBuf::from).filter(|p| p.is_dir())
    {
        return Some(s);
    }
    // what xcrun said last time, while the same developer tools are selected
    // and their SDKs folder is unchanged (each run of xcrun costs ~25 ms)
    let dev = std::env::var_os("DEVELOPER_DIR").map(PathBuf::from).or_else(|| std::fs::read_link("/var/db/xcode_select_link").ok());
    let memo = libc_cache().join(format!("sdk-{name}.txt"));
    if let Some(dev) = &dev
        && let Ok(text) = std::fs::read_to_string(&memo)
        && let [d, path, when] = text.lines().collect::<Vec<_>>()[..]
        && Path::new(d) == dev.as_path()
        && sdks_changed(Path::new(path)).as_deref() == Some(when)
    {
        return Some(PathBuf::from(path));
    }
    let sdk = tool_output("xcrun", &["--sdk", name, "--show-sdk-path"]).map(PathBuf::from).filter(|p| p.is_dir())?;
    if let Some(dev) = &dev
        && let Some(when) = sdks_changed(&sdk)
        && std::fs::create_dir_all(libc_cache()).is_ok()
    {
        let _ = mimic_toolchain::jobs::write_atomic(&memo, format!("{}\n{}\n{when}\n", dev.display(), sdk.display()).as_bytes());
    }
    Some(sdk)
}

/// When the folder holding an SDK last changed (an SDK added or removed),
/// or None if the SDK is gone.
fn sdks_changed(sdk: &Path) -> Option<String> {
    if !sdk.is_dir() {
        return None;
    }
    let t = std::fs::metadata(sdk.parent()?).ok()?.modified().ok()?;
    Some(t.duration_since(std::time::UNIX_EPOCH).ok()?.as_nanos().to_string())
}

/// An SDK's version, from its own SDKSettings.json (as `xcrun --show-sdk-version` reads it).
fn sdk_version(sdk: &Path) -> Option<String> {
    let v: serde_json::Value = serde_json::from_slice(&std::fs::read(sdk.join("SDKSettings.json")).ok()?).ok()?;
    v["Version"].as_str().map(String::from).filter(|s| !s.is_empty())
}

/// This Mac's OS version, from the file `sw_vers` reads.
fn macos_version() -> Option<String> {
    let text = std::fs::read_to_string("/System/Library/CoreServices/SystemVersion.plist").ok()?;
    let after = &text[text.find("<key>ProductVersion</key>")? + 25..];
    let start = after.find("<string>")? + 8;
    let end = after[start..].find("</string>")? + start;
    Some(after[start..end].trim().to_string()).filter(|s| !s.is_empty())
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
            c.env(mimic_toolchain::RAW_CC_ENV, "1").arg("cc").arg("-resource-dir").arg(resource_dir());
            if let Some(t) = &target.llvm
                && !matches!(sys, Sysroot::Musl { .. } | Sysroot::Wasi { .. })
            {
                c.arg(format!("--target={t}"));
            }
            match sys {
                Sysroot::Darwin { sdk, min_os, .. } => {
                    c.arg("-isysroot").arg(sdk);
                    // cross targets carry the minimum OS in the triple
                    if target.llvm.is_none() {
                        c.arg(format!("-mmacosx-version-min={min_os}"));
                    }
                }
                Sysroot::Musl { triple, root, .. } => {
                    c.args(mimic_toolchain::musl::target_flags(triple)).arg(format!("--sysroot={}", root.display()));
                }
                Sysroot::Wasi { root, .. } => {
                    c.args(mimic_toolchain::wasi::cflags(root));
                }
                Sysroot::Mingw(m) => {
                    c.args(m.cflags());
                }
            }
            c
        }
        // the host's own headers and libraries (Linux with glibc, …), compiled
        // by the embedded Clang when there is one: C code (the runtime, imported
        // headers) always sees Clang, whatever the system compiler is
        Toolchain::System if mimic_toolchain::EMBEDDED => {
            let mut c = Command::new(self_exe());
            c.env(mimic_toolchain::RAW_CC_ENV, "1").arg("cc").arg("-resource-dir").arg(resource_dir());
            c
        }
        Toolchain::System => Command::new("cc"),
    }
}

/// The main thread's stack a macOS executable asks for: 512 MB, reserved
/// and used as it grows, so deep recursion the net cannot take over (a
/// function giving back a list) runs as far as on Windows and Linux.
pub const MAC_STACK: u64 = 0x2000_0000;

/// After linking with the embedded LLD (which does not implement
/// `-stack_size`): `LC_MAIN`'s stack size set in the executable, which is
/// then signed again (Apple's arm64 code must be; `codesign` is part of
/// macOS). Not on other hosts, nor when the executable is not a plain
/// 64-bit Mach-O one: it keeps the default 8 MB.
pub fn mac_main_stack(tc: &Toolchain, exe: &Path) -> Result<(), String> {
    let Toolchain::Embedded(Sysroot::Darwin { platform: "macos", .. }) = tc else { return Ok(()) };
    if !cfg!(target_os = "macos") {
        return Ok(());
    }
    let mut b = std::fs::read(exe).map_err(|e| format!("cannot read {}: {e}", exe.display()))?;
    let u32_at = |b: &[u8], o: usize| u32::from_le_bytes(b[o..o + 4].try_into().unwrap());
    if b.len() < 32 || u32_at(&b, 0) != 0xfeed_facf {
        return Ok(());
    }
    let (ncmds, mut off) = (u32_at(&b, 16) as usize, 32usize);
    for _ in 0..ncmds {
        if off + 8 > b.len() {
            return Ok(());
        }
        let (cmd, size) = (u32_at(&b, off), u32_at(&b, off + 4) as usize);
        // LC_MAIN: cmd, cmdsize, entryoff (u64), stacksize (u64)
        if cmd == 0x8000_0028 && size >= 24 && off + 24 <= b.len() {
            b[off + 16..off + 24].copy_from_slice(&MAC_STACK.to_le_bytes());
            std::fs::write(exe, &b).map_err(|e| format!("cannot write {}: {e}", exe.display()))?;
            let out = Command::new("/usr/bin/codesign").args(["--force", "--sign", "-"]).arg(exe).output().map_err(|e| format!("cannot run codesign: {e}"))?;
            if !out.status.success() {
                return Err(format!("signing {} failed: {}", exe.display(), String::from_utf8_lossy(&out.stderr)));
            }
            return Ok(());
        }
        off += size.max(8);
    }
    Ok(())
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
        Toolchain::Embedded(Sysroot::Darwin { sdk, min_os, sdk_version, platform }) => {
            let arch = match target.llvm.as_deref().map(|t| t.split('-').next().unwrap_or("")) {
                Some("x86_64") => "x86_64",
                Some("arm64_32" | "aarch64_32") => "arm64_32",
                Some(_) => "arm64",
                None if cfg!(target_arch = "x86_64") => "x86_64",
                None => "arm64",
            };
            let mut c = Command::new(self_exe());
            c.arg("ld64.lld")
                .args(["-arch", arch])
                .args(["-platform_version", platform, min_os, sdk_version])
                .arg("-syslibroot")
                .arg(sdk)
                .arg("-dead_strip")
                .arg("-o")
                .arg(job.output)
                .args(job.objects);
            // arm64 Apple code must be signed to run; LLD signs only macOS by default
            if arch != "x86_64" {
                c.arg("-adhoc_codesign");
            }
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
        Toolchain::Embedded(Sysroot::Wasi { root, compiler_rt }) => {
            let lib = root.join("lib").join(mimic_toolchain::wasi::CLANG_TRIPLE);
            let mut c = Command::new(self_exe());
            // 8 MB C stack in linear memory, like a native thread (the default is 64 KB)
            c.arg("wasm-ld").args(["-m", "wasm32", "-z", "stack-size=8388608"]).arg("--gc-sections").arg("-o").arg(job.output);
            c.arg(lib.join("crt1-command.o")).args(job.objects);
            c.arg(format!("-L{}", lib.display()));
            for s in &job.search {
                c.arg(format!("-L{}", s.display()));
            }
            for l in &job.libs {
                c.arg(format!("-l{l}"));
            }
            c.arg("-lc").arg(compiler_rt);
            c
        }
        Toolchain::Embedded(Sysroot::Mingw(m)) => {
            let lib = m.lib();
            let mut c = Command::new(self_exe());
            // a 512 MB main-thread stack, reserved and committed as it grows
            // (Windows' default is 1 MB): deep recursion the net cannot take
            // over (a function giving back a list) runs on it
            c.arg("ld.lld").args(["-m", m.emulation()]).args(["--subsystem", "console", "--stack", "536870912"]).arg("--gc-sections");
            c.arg("-o").arg(job.output);
            c.arg(lib.join("crt2.o")).arg(lib.join("crtbegin.o")).args(job.objects);
            c.arg(format!("-L{}", lib.display()));
            for s in &job.search {
                c.arg(format!("-L{}", s.display()));
            }
            for l in &job.libs {
                c.arg(format!("-l{l}"));
            }
            // the C runtime, threads, and the Windows DLLs the runtime uses;
            // mingw's libraries refer to each other, so they go in a group
            c.arg("--start-group");
            c.args(["-lmingw32", "-lmingwex", "-lucrt", "-lpthread", "-lkernel32", "-luser32", "-lws2_32", "-ladvapi32", "-lbcrypt", "-lsecur32", "-lcrypt32", "-lncrypt", "-lshell32", "-lntdll", "-lsynchronization"]);
            c.arg(m.builtins());
            c.arg("--end-group");
            c.arg(lib.join("crtend.o"));
            c
        }
        Toolchain::System => {
            // linked by the system compiler driver, which knows its C library
            let mut c = Command::new("cc");
            c.args(job.objects).arg("-o").arg(job.output);
            for s in &job.search {
                c.arg(format!("-L{}", s.display()));
            }
            for l in &job.libs {
                c.arg(format!("-l{l}"));
            }
            if !target.is_apple() {
                // drop runtime code the program never reaches (and with it
                // references to what only Mimic programs define)
                c.arg("-Wl,--gc-sections").arg("-lm");
            }
            // a 512 MB main-thread stack on macOS (MAC_STACK)
            if matches!(target.os.as_str(), "macosx" | "darwin") && target.env != "macabi" {
                c.arg(format!("-Wl,-stack_size,{MAC_STACK:#x}"));
            }
            if job.debug {
                c.arg("-g");
            }
            c
        }
    }
}

/// The file name of the shared library `name` for `target`.
pub fn shared_name(target: &Target, name: &str) -> String {
    if target.os == "windows" {
        format!("{name}.dll")
    } else if target.is_apple() {
        format!("lib{name}.dylib")
    } else {
        format!("lib{name}.so")
    }
}

/// The command that links `job` into a shared library exporting only
/// `exports` (C symbol names). Export lists are written into `dir`. On
/// Windows an import library `lib<name>.dll.a` is written next to the DLL.
pub fn link_shared(tc: &Toolchain, target: &Target, job: &LinkJob, exports: &[String], dir: &Path) -> Result<Command, String> {
    let write = |file: &str, text: String| -> Result<PathBuf, String> {
        let path = dir.join(file);
        std::fs::write(&path, text).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
        Ok(path)
    };
    let stem = job.output.file_stem().and_then(|s| s.to_str()).unwrap_or("lib").to_string();
    let file = job.output.file_name().and_then(|s| s.to_str()).unwrap_or("lib").to_string();
    match tc {
        Toolchain::Embedded(Sysroot::Darwin { .. }) => {
            let list = write("exports.list", exports.iter().map(|e| format!("_{e}\n")).collect())?;
            let mut c = link(tc, target, job);
            c.arg("-dylib").arg("-install_name").arg(format!("@rpath/{file}")).arg("-exported_symbols_list").arg(list);
            Ok(c)
        }
        Toolchain::Embedded(Sysroot::Mingw(m)) => {
            let def = write(&format!("{stem}.def"), format!("LIBRARY {file}\nEXPORTS\n{}", exports.iter().map(|e| format!("    {e}\n")).collect::<String>()))?;
            let implib = job.output.with_file_name(format!("lib{stem}.dll.a"));
            let lib = m.lib();
            let mut c = Command::new(self_exe());
            c.arg("ld.lld").args(["-m", m.emulation()]).arg("--shared").args(["--subsystem", "console"]).arg("--gc-sections");
            c.arg("-o").arg(job.output).arg(format!("--out-implib={}", implib.display()));
            c.arg(lib.join("dllcrt2.o")).arg(lib.join("crtbegin.o")).args(job.objects).arg(def);
            c.arg(format!("-L{}", lib.display()));
            for s in &job.search {
                c.arg(format!("-L{}", s.display()));
            }
            for l in &job.libs {
                c.arg(format!("-l{l}"));
            }
            c.arg("--start-group");
            c.args(["-lmingw32", "-lmingwex", "-lucrt", "-lpthread", "-lkernel32", "-luser32", "-lws2_32", "-ladvapi32", "-lbcrypt", "-lsecur32", "-lcrypt32", "-lncrypt", "-lshell32", "-lntdll", "-lsynchronization"]);
            c.arg(m.builtins());
            c.arg("--end-group");
            c.arg(lib.join("crtend.o"));
            Ok(c)
        }
        Toolchain::System if target.is_apple() => {
            let list = write("exports.list", exports.iter().map(|e| format!("_{e}\n")).collect())?;
            let mut c = link(tc, target, job);
            c.arg("-dynamiclib").arg(format!("-Wl,-install_name,@rpath/{file}")).arg(format!("-Wl,-exported_symbols_list,{}", list.display()));
            Ok(c)
        }
        Toolchain::System => {
            let script = write("exports.map", format!("{{\n  global:\n{}  local: *;\n}};\n", exports.iter().map(|e| format!("    {e};\n")).collect::<String>()))?;
            let mut c = link(tc, target, job);
            c.arg("-shared").arg(format!("-Wl,--version-script={}", script.display())).arg("-lpthread");
            Ok(c)
        }
        Toolchain::Embedded(Sysroot::Musl { .. }) => Err("shared libraries need a dynamic C library; static Linux targets (musl) build static libraries only: drop --shared, or build for the host".into()),
        Toolchain::Embedded(Sysroot::Wasi { .. }) => Err("WebAssembly has no shared libraries: drop --shared".into()),
    }
}

/// Write a static library (`.a`) containing `objects`.
pub fn archive(tc: &Toolchain, target: &Target, output: &Path, objects: &[PathBuf]) -> Result<(), String> {
    if mimic_toolchain::EMBEDDED {
        let kind = match target.os.as_str() {
            _ if target.is_apple() => mimic_toolchain::ArchiveKind::Darwin,
            "windows" | "uefi" => mimic_toolchain::ArchiveKind::Coff,
            _ => mimic_toolchain::ArchiveKind::Gnu,
        };
        return mimic_toolchain::write_archive(output, kind, objects);
    }
    let _ = tc;
    let mut c = Command::new("ar");
    c.arg("rcs").arg(output).args(objects);
    run_tool(c, "archiving")
}

pub fn run_tool(mut c: Command, what: &str) -> Result<(), String> {
    // LLD (this binary run as `ld.lld`, `ld64.lld`, …): in this process, which
    // saves starting a second one (~0.1 s)
    if c.get_program() == self_exe().as_os_str()
        && c.get_envs().next().is_none()
        && c.get_current_dir().is_none()
        && !crate::driver::debug_disabled("in-process-link")
    {
        let args: Vec<String> = c.get_args().map(|a| a.to_string_lossy().into_owned()).collect();
        if args.first().is_some_and(|f| mimic_toolchain::LLD_FLAVOURS.contains(&f.as_str())) {
            let (code, text) = mimic_toolchain::lld_link(&args);
            return if code == 0 { Ok(()) } else { Err(format!("{what} failed:\n{text}")) };
        }
    }
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
                .env(mimic_toolchain::RAW_CC_ENV, "1")
                .args(["cc", &format!("--target={triple}"), "-###", "-c", "-x", "c", NULL_DEVICE, "-o", NULL_DEVICE])
                .output()
                .ok()?;
            let all = String::from_utf8_lossy(&out.stderr).into_owned();
            let line = all.lines().rev().find(|l| l.contains("\"-cc1\""))?.to_string();
            let words: Vec<String> = line.split("\" \"").map(|w| w.trim_matches([' ', '"']).to_string()).collect();
            let mut cpu = String::new(); // none named: LLVM's default
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
