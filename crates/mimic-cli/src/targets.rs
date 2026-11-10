//! Targets: every architecture, OS and ABI that Zig 0.17 knows, written the
//! Zig way (`arch-os[-abi][.version]`, e.g. `aarch64-linux-musl`,
//! `x86_64-linux-gnu.2.17`, `aarch64-macos.13.0`), mapped to LLVM triples.
//!
//! Whether a target can be *built* depends on two things: an LLVM backend for
//! its architecture, and a libc for it (bundled, or via the Zig fallback while
//! Phase A is in progress). `mimic targets` shows the full matrix.

/// (Zig name, LLVM triple arch, backend in the LLVM linked into mimic).
pub const ARCHES: &[(&str, &str, Backend)] = &[
    ("aarch64", "aarch64", Backend::Yes),
    ("aarch64_be", "aarch64_be", Backend::Yes),
    ("alpha", "alpha", Backend::None),
    ("amdgcn", "amdgcn", Backend::Yes),
    ("arc", "arc", Backend::Experimental),
    ("arceb", "arceb", Backend::None),
    ("arm", "arm", Backend::Yes),
    ("armeb", "armeb", Backend::Yes),
    ("avr", "avr", Backend::Yes),
    ("bpfeb", "bpfeb", Backend::Yes),
    ("bpfel", "bpfel", Backend::Yes),
    ("csky", "csky", Backend::Experimental),
    ("ez80", "ez80", Backend::None),
    ("hexagon", "hexagon", Backend::Yes),
    ("hppa", "hppa", Backend::None),
    ("hppa64", "hppa64", Backend::None),
    ("kalimba", "kalimba", Backend::None),
    ("kvx", "kvx", Backend::None),
    ("lanai", "lanai", Backend::Yes),
    ("loongarch32", "loongarch32", Backend::Yes),
    ("loongarch64", "loongarch64", Backend::Yes),
    ("m68k", "m68k", Backend::Experimental),
    ("m88k", "m88k", Backend::None),
    ("microblaze", "microblaze", Backend::None),
    ("microblazeel", "microblazeel", Backend::None),
    ("mips", "mips", Backend::Yes),
    ("mipsel", "mipsel", Backend::Yes),
    ("mips64", "mips64", Backend::Yes),
    ("mips64el", "mips64el", Backend::Yes),
    ("msp430", "msp430", Backend::Yes),
    ("nvptx", "nvptx", Backend::Yes),
    ("nvptx64", "nvptx64", Backend::Yes),
    ("or1k", "or1k", Backend::None),
    ("powerpc", "powerpc", Backend::Yes),
    ("powerpcle", "powerpcle", Backend::Yes),
    ("powerpc64", "powerpc64", Backend::Yes),
    ("powerpc64le", "powerpc64le", Backend::Yes),
    ("propeller", "propeller", Backend::None),
    ("riscv32", "riscv32", Backend::Yes),
    ("riscv32be", "riscv32be", Backend::Yes),
    ("riscv64", "riscv64", Backend::Yes),
    ("riscv64be", "riscv64be", Backend::Yes),
    ("s390x", "s390x", Backend::Yes),
    ("sh", "sh", Backend::None),
    ("sheb", "sheb", Backend::None),
    ("sparc", "sparc", Backend::Yes),
    ("sparc64", "sparcv9", Backend::Yes),
    ("spork8", "spork8", Backend::None),
    ("spirv32", "spirv32", Backend::Yes),
    ("spirv64", "spirv64", Backend::Yes),
    ("thumb", "thumb", Backend::Yes),
    ("thumbeb", "thumbeb", Backend::Yes),
    ("ve", "ve", Backend::Yes),
    ("wasm32", "wasm32", Backend::Yes),
    ("wasm64", "wasm64", Backend::Yes),
    ("x86_16", "i386", Backend::Yes),
    ("x86", "i686", Backend::Yes),
    ("x86_64", "x86_64", Backend::Yes),
    ("xcore", "xcore", Backend::Yes),
    ("xtensa", "xtensa", Backend::Experimental),
    ("xtensaeb", "xtensaeb", Backend::None),
];

/// Every OS Zig knows, and LLVM's (vendor, os) spelling.
pub const OSES: &[(&str, &str, &str)] = &[
    ("freestanding", "unknown", "unknown"),
    ("other", "unknown", "unknown"),
    ("contiki", "unknown", "unknown"),
    ("fuchsia", "unknown", "fuchsia"),
    ("hermit", "unknown", "hermit"),
    ("managarm", "unknown", "managarm"),
    ("haiku", "unknown", "haiku"),
    ("hurd", "unknown", "hurd"),
    ("illumos", "unknown", "illumos"),
    ("linux", "unknown", "linux"),
    ("plan9", "unknown", "unknown"),
    ("rtems", "unknown", "rtems"),
    ("serenity", "unknown", "serenity"),
    ("dragonfly", "unknown", "dragonfly"),
    ("freebsd", "unknown", "freebsd"),
    ("netbsd", "unknown", "netbsd"),
    ("openbsd", "unknown", "openbsd"),
    ("driverkit", "apple", "driverkit"),
    ("ios", "apple", "ios"),
    ("maccatalyst", "apple", "ios"),
    ("macos", "apple", "macosx"),
    ("tvos", "apple", "tvos"),
    ("visionos", "apple", "xros"),
    ("watchos", "apple", "watchos"),
    ("windows", "pc", "windows"),
    ("uefi", "unknown", "uefi"),
    ("3ds", "unknown", "unknown"),
    ("wiiu", "unknown", "unknown"),
    ("switch", "unknown", "unknown"),
    ("gba", "unknown", "unknown"),
    ("psx", "unknown", "unknown"),
    ("ps3", "scei", "lv2"),
    ("ps4", "scei", "ps4"),
    ("ps5", "scei", "ps5"),
    ("psp", "unknown", "unknown"),
    ("vita", "unknown", "unknown"),
    ("emscripten", "unknown", "emscripten"),
    ("wasi", "unknown", "wasi"),
    ("amdhsa", "amd", "amdhsa"),
    ("amdpal", "amd", "amdpal"),
    ("cuda", "nvidia", "cuda"),
    ("mesa3d", "unknown", "mesa3d"),
    ("nvcl", "nvidia", "nvcl"),
    ("opencl", "unknown", "unknown"),
    ("opengl", "unknown", "unknown"),
    ("vulkan", "unknown", "vulkan"),
    ("tios", "unknown", "unknown"),
    ("ashetos", "unknown", "unknown"),
];

/// Every ABI Zig knows (LLVM uses the same names for its environment field).
pub const ABIS: &[&str] = &[
    "none", "gnu", "gnuabin32", "gnuabi64", "gnueabi", "gnueabihf", "gnuf32", "gnusf", "gnux32", "eabi", "eabihf",
    "abin32", "x32", "ilp32", "android", "androideabi", "musl", "muslabin32", "muslabi64", "musleabi", "musleabihf",
    "muslf32", "muslsf", "muslx32", "msvc", "itanium", "simulator", "ohos", "ohoseabi", "call0",
];

/// The targets for which Zig ships a libc, so complete executables can be
/// built from any machine (`zig targets` → `libc`).
pub const LIBC_TARGETS: &[&str] = &[
    "arc-linux-gnu", "arm-freebsd-eabihf", "arm-linux-gnueabi", "arm-linux-gnueabihf", "arm-linux-musleabi",
    "arm-linux-musleabihf", "arm-netbsd-eabi", "arm-netbsd-eabihf", "arm-openbsd-eabi", "armeb-linux-gnueabi",
    "armeb-linux-gnueabihf", "armeb-linux-musleabi", "armeb-linux-musleabihf", "armeb-netbsd-eabi",
    "armeb-netbsd-eabihf", "thumb-linux-musleabi", "thumb-linux-musleabihf", "thumb-windows-gnu",
    "thumbeb-linux-musleabi", "thumbeb-linux-musleabihf", "aarch64-freebsd-none", "aarch64-linux-gnu",
    "aarch64-linux-musl", "aarch64-maccatalyst-none", "aarch64-macos-none", "aarch64-netbsd-none",
    "aarch64-openbsd-none", "aarch64-windows-gnu", "aarch64_be-linux-gnu", "aarch64_be-linux-musl",
    "aarch64_be-netbsd-none", "csky-linux-gnueabi", "csky-linux-gnueabihf", "hexagon-linux-musl",
    "loongarch32-linux-gnu", "loongarch32-linux-gnusf", "loongarch64-linux-gnu", "loongarch64-linux-gnusf",
    "loongarch64-linux-musl", "loongarch64-linux-muslf32", "loongarch64-linux-muslsf", "m68k-linux-gnu",
    "m68k-linux-musl", "m68k-netbsd-none", "mips-linux-gnueabi", "mips-linux-gnueabihf", "mips-linux-musleabi",
    "mips-linux-musleabihf", "mips-netbsd-eabi", "mips-netbsd-eabihf", "mipsel-linux-gnueabi",
    "mipsel-linux-gnueabihf", "mipsel-linux-musleabi", "mipsel-linux-musleabihf", "mipsel-netbsd-eabi",
    "mipsel-netbsd-eabihf", "mips64-linux-gnuabi64", "mips64-linux-gnuabin32", "mips64-linux-muslabi64",
    "mips64-linux-muslabin32", "mips64-openbsd-none", "mips64el-linux-gnuabi64", "mips64el-linux-gnuabin32",
    "mips64el-linux-muslabi64", "mips64el-linux-muslabin32", "mips64el-openbsd-none", "powerpc-linux-musleabi",
    "powerpc-linux-musleabihf", "powerpc-netbsd-eabihf", "powerpc-openbsd-eabihf", "powerpc64-freebsd-none",
    "powerpc64-linux-musl", "powerpc64-openbsd-none", "powerpc64le-freebsd-none", "powerpc64le-linux-gnu",
    "powerpc64le-linux-musl", "riscv32-linux-gnu", "riscv32-linux-musl", "riscv32-netbsd-none",
    "riscv64-freebsd-none", "riscv64-linux-gnu", "riscv64-linux-musl", "riscv64-netbsd-none",
    "riscv64-openbsd-none", "s390x-linux-gnu", "s390x-linux-musl", "sparc-linux-gnu", "sparc-netbsd-none",
    "sparc64-linux-gnu", "sparc64-netbsd-none", "sparc64-openbsd-none", "wasm32-wasi-musl", "x86-freebsd-none",
    "x86-linux-gnu", "x86-linux-musl", "x86-netbsd-none", "x86-openbsd-none", "x86-windows-gnu",
    "x86_64-freebsd-none", "x86_64-linux-gnu", "x86_64-linux-gnux32", "x86_64-linux-musl", "x86_64-linux-muslx32",
    "x86_64-maccatalyst-none", "x86_64-macos-none", "x86_64-netbsd-none", "x86_64-openbsd-none",
    "x86_64-windows-gnu",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Backend {
    /// The LLVM linked into mimic can generate code for it.
    Yes,
    /// LLVM has an experimental backend, not enabled in this build.
    Experimental,
    /// No LLVM backend exists.
    None,
}

impl Backend {
    pub fn as_str(self) -> &'static str {
        match self {
            Backend::Yes => "yes",
            Backend::Experimental => "experimental (not in this build)",
            Backend::None => "none",
        }
    }
}

pub struct Target {
    /// The target as the user wrote it, normalised (`x86_64-linux-musl`);
    /// `None` = the host.
    pub zig: Option<String>,
    /// LLVM triple for codegen; `None` = host.
    pub llvm: Option<String>,
    pub arch: String,
    pub os: String,
    pub abi: String,
    /// `.13.0` in `aarch64-macos.13.0`, `.2.17` in `x86_64-linux-gnu.2.17`
    /// (read by the glibc and Darwin sysroots as they are bundled).
    #[allow(dead_code)]
    pub version: Option<String>,
}

impl Target {
    pub fn host() -> Target {
        let arch = std::env::consts::ARCH;
        Target {
            zig: None,
            llvm: None,
            arch: arch.into(),
            os: std::env::consts::OS.to_string(),
            abi: if cfg!(target_env = "musl") { "musl".into() } else { "none".into() },
            version: None,
        }
    }

    /// `arch-os[-abi][.version]` (or `native`).
    pub fn parse(t: &str) -> Result<Target, String> {
        if t == "native" || t == "host" {
            return Ok(Target::host());
        }
        let parts: Vec<&str> = t.split('-').collect();
        if parts.len() < 2 || parts.len() > 3 {
            return Err(format!("invalid target `{t}`: expected <arch>-<os>[-<abi>], e.g. x86_64-linux-musl (run `mimic targets`)"));
        }
        let arch = match parts[0] {
            "arm64" => "aarch64",
            "amd64" => "x86_64",
            "i386" | "i486" | "i586" | "i686" => "x86",
            a => a,
        };
        let Some(&(_, llvm_arch, backend)) = ARCHES.iter().find(|(n, _, _)| *n == arch) else {
            return Err(format!("unknown architecture `{arch}` in target `{t}` (run `mimic targets`)"));
        };
        if backend != Backend::Yes {
            return Err(format!(
                "architecture `{arch}` has no code generator in this build (LLVM backend: {}); run `mimic targets`",
                backend.as_str()
            ));
        }
        // the version suffix sits on the last component: `macos.13.0`, `gnu.2.17`
        let last = parts.len() - 1;
        let (tail, version) = match parts[last].split_once('.') {
            Some((a, v)) => (a, Some(v.to_string())),
            None => (parts[last], None),
        };
        let os = if last == 1 { tail } else { parts[1] };
        let Some(&(_, vendor, llvm_os)) = OSES.iter().find(|(n, _, _)| *n == os) else {
            return Err(format!("unknown OS `{os}` in target `{t}` (run `mimic targets`)"));
        };
        let abi = if last == 2 { tail.to_string() } else { default_abi(arch, os).to_string() };
        if !ABIS.contains(&abi.as_str()) {
            return Err(format!("unknown ABI `{abi}` in target `{t}` (run `mimic targets`)"));
        }
        let mut llvm_os = llvm_os.to_string();
        let mut env = if abi == "none" { String::new() } else { abi.clone() };
        match os {
            "macos" | "ios" | "tvos" | "watchos" | "visionos" | "driverkit" => {
                llvm_os.push_str(version.as_deref().unwrap_or(default_os_version(os, arch)));
                if abi != "simulator" {
                    env.clear();
                }
            }
            "maccatalyst" => {
                llvm_os.push_str(version.as_deref().unwrap_or("14.0"));
                env = "macabi".into();
            }
            "vulkan" => llvm_os.push_str(version.as_deref().unwrap_or("1.3")),
            _ => {}
        }
        // 32-bit ARM: ARMv7-A is the baseline (atomics, VFP); LLVM's bare
        // `arm` would mean ARMv4T
        let llvm_arch = match llvm_arch {
            "arm" => "armv7a",
            "armeb" => "armebv7a",
            "thumb" => "thumbv7a",
            "thumbeb" => "thumbebv7a",
            a => a,
        };
        let mut llvm = format!("{llvm_arch}-{vendor}-{llvm_os}");
        if !env.is_empty() {
            llvm.push('-');
            llvm.push_str(&env);
        }
        if arch == "x86_16" {
            llvm = format!("i386-{vendor}-{llvm_os}-code16");
        }
        let mut name = format!("{arch}-{os}-{abi}");
        if let Some(v) = &version {
            name.push('.');
            name.push_str(v);
        }
        Ok(Target { zig: Some(name), llvm: Some(llvm), arch: arch.into(), os: os.into(), abi, version })
    }

    pub fn name(&self) -> &str {
        self.zig.as_deref().unwrap_or("native")
    }

    pub fn exe_suffix(&self) -> &'static str {
        match self.os.as_str() {
            "windows" | "uefi" => ".exe",
            "wasi" | "emscripten" => ".wasm",
            _ => "",
        }
    }

    pub fn needs_libm(&self) -> bool {
        !matches!(self.os.as_str(), "windows" | "wasi" | "freestanding" | "macos")
    }
}

/// Zig's defaults: Linux gets musl (fully static, no system libc needed);
/// Windows gets the GNU (MinGW) ABI; ARM gets hard-float EABI.
fn default_abi(arch: &str, os: &str) -> &'static str {
    let arm = matches!(arch, "arm" | "armeb" | "thumb" | "thumbeb");
    let mips32 = matches!(arch, "mips" | "mipsel" | "powerpc");
    match os {
        "linux" if arm || mips32 => "musleabihf",
        "linux" if matches!(arch, "mips64" | "mips64el") => "muslabi64",
        "linux" => "musl",
        "windows" => "gnu",
        "wasi" => "musl",
        "freebsd" | "netbsd" | "openbsd" if arm || mips32 => "eabihf",
        _ => "none",
    }
}

fn default_os_version(os: &str, arch: &str) -> &'static str {
    match (os, arch) {
        ("macos", "x86_64") => "11.0",
        ("macos", _) => "11.0",
        ("ios", _) => "14.0",
        ("tvos", _) => "14.0",
        ("watchos", _) => "7.0",
        ("visionos", _) => "1.0",
        ("driverkit", _) => "20.0",
        _ => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_triples() {
        let l = |t: &str| Target::parse(t).unwrap().llvm.unwrap();
        assert_eq!(l("x86_64-linux-gnu"), "x86_64-unknown-linux-gnu");
        assert_eq!(l("x86_64-linux"), "x86_64-unknown-linux-musl");
        assert_eq!(l("aarch64-macos"), "aarch64-apple-macosx11.0");
        assert_eq!(l("aarch64-macos.13.0"), "aarch64-apple-macosx13.0");
        assert_eq!(l("x86_64-windows"), "x86_64-pc-windows-gnu");
        assert_eq!(l("x86-linux-musl"), "i686-unknown-linux-musl");
        assert_eq!(l("sparc64-linux-gnu"), "sparcv9-unknown-linux-gnu");
        assert_eq!(l("arm-linux"), "armv7a-unknown-linux-musleabihf");
        assert_eq!(l("aarch64-maccatalyst"), "aarch64-apple-ios14.0-macabi");
        assert_eq!(l("wasm32-freestanding"), "wasm32-unknown-unknown");
        let g = Target::parse("x86_64-linux-gnu.2.17").unwrap();
        assert_eq!(g.version.as_deref(), Some("2.17"));
        assert_eq!(g.name(), "x86_64-linux-gnu.2.17");
        assert_eq!(Target::parse("wasm32-wasi").unwrap().exe_suffix(), ".wasm");
        assert!(Target::parse("nonsense").is_err());
        let err = |t: &str| Target::parse(t).err().unwrap();
        assert!(err("m68k-linux-musl").contains("no code generator"));
        assert!(err("x86_64-linux-bogus").contains("unknown ABI"));
    }

    #[test]
    fn every_zig_target_parses_or_explains() {
        assert_eq!(ARCHES.len(), 61);
        assert_eq!(OSES.len(), 48);
        assert_eq!(ABIS.len(), 30);
        assert_eq!(LIBC_TARGETS.len(), 108);
        for t in LIBC_TARGETS {
            match Target::parse(t) {
                Ok(tg) => assert!(tg.llvm.is_some(), "{t}"),
                Err(e) => assert!(e.contains("no code generator"), "{t}: {e}"),
            }
        }
    }
}
