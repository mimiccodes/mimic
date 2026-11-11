//! Targets. Every architecture, OS and environment comes from the LLVM linked
//! into `mimic`: whatever LLVM can generate code for, `mimic` can target.
//!
//! A target is written as an LLVM triple, with the vendor optional:
//! `x86_64-linux-musl`, `aarch64-apple-macosx13.0`, `riscv64-unknown-linux-musl`.
//! A glibc version may follow the environment: `x86_64-linux-gnu.2.17`.
//!
//! Building a *complete executable* also needs a C library for the target;
//! which ones `mimic` provides is decided in `toolchain.rs`.

use mimic_toolchain::{Names, llvm_names, llvm_triple};

pub struct Target {
    /// The target as the user wrote it, with defaults filled in
    /// (`aarch64-linux` → `aarch64-linux-musl`); `None` = the host.
    pub name: Option<String>,
    /// Normalised LLVM triple for codegen; `None` = host.
    pub llvm: Option<String>,
    /// LLVM's names for the parts (`x86_64`, `linux`, `musl`).
    pub arch: String,
    pub os: String,
    pub env: String,
    /// `2.17` in `x86_64-linux-gnu.2.17` (read by the glibc sysroot once bundled).
    #[allow(dead_code)]
    pub version: Option<String>,
}

impl Target {
    pub fn host() -> Target {
        let os = match std::env::consts::OS {
            "macos" => "macosx",
            o => o,
        };
        Target {
            name: None,
            llvm: None,
            arch: std::env::consts::ARCH.to_string(),
            os: os.to_string(),
            env: if cfg!(target_env = "musl") { "musl".into() } else { String::new() },
            version: None,
        }
    }

    /// `arch[-vendor]-os[-env][.version]`, or `native`.
    pub fn parse(t: &str) -> Result<Target, String> {
        if t == "native" || t == "host" {
            return Ok(Target::host());
        }
        let mut parts: Vec<String> = t.split('-').map(String::from).collect();
        if parts.len() < 2 || parts.iter().any(|p| p.is_empty()) {
            return Err(format!("invalid target `{t}`: expected <arch>-<os>[-<env>], e.g. x86_64-linux-musl (run `mimic targets`)"));
        }
        // a glibc version after the environment: `gnu.2.17`
        let mut version = None;
        if let Some(last) = parts.last_mut()
            && let Some((env, v)) = last.split_once('.')
            && env.starts_with("gnu")
        {
            version = Some(v.to_string());
            *last = env.to_string();
        }
        // spellings LLVM does not take on its own
        if parts[0] == "x86" {
            parts[0] = "i686".into();
        }
        for p in parts.iter_mut().skip(1) {
            if p == "freestanding" || p == "none" {
                *p = "unknown".into();
            }
        }
        // 32-bit ARM: ARMv7-A is the baseline (atomics, VFP); bare `arm` would mean ARMv4T
        match parts[0].as_str() {
            "arm" => parts[0] = "armv7a".into(),
            "armeb" => parts[0] = "armebv7a".into(),
            "thumb" => parts[0] = "thumbv7a".into(),
            "thumbeb" => parts[0] = "thumbebv7a".into(),
            _ => {}
        }
        // Windows without an environment: MinGW (`gnu`), the C runtime mimic
        // can bundle (LLVM alone would pick MSVC)
        let mut implied_env = None;
        if parts.len() == 2 && parts[1] == "windows" {
            parts.push("gnu".into());
            implied_env = Some("gnu");
        }
        let mut tt = llvm_triple(&parts.join("-"));
        if tt.arch.is_empty() || tt.arch == "unknown" {
            return Err(format!("unknown architecture `{}` in target `{t}` (run `mimic targets`)", parts[0]));
        }
        // every component the user wrote must mean something to LLVM
        let known = [tt.vendor.clone(), tt.os.clone(), tt.env.clone()];
        for p in &parts[1..] {
            let base = p.trim_end_matches(|c: char| c.is_ascii_digit() || c == '.');
            // object formats (`riscv32-unknown-elf`) are a valid last component
            let recognised = p == "unknown"
                || ["elf", "coff", "macho", "wasm", "xcoff", "goff", "spirv", "dxcontainer"].contains(&base)
                || known.iter().any(|k| !k.is_empty() && (k == base || base.starts_with(k.as_str()) || k.starts_with(base)))
                || (base == "macos" && tt.os == "macosx");
            if !recognised {
                return Err(format!("`{p}` in target `{t}` is not an OS, vendor or environment LLVM knows (run `mimic targets`)"));
            }
        }
        if tt.backend.is_none() {
            return Err(format!("this build of mimic has no code generator for `{}` (run `mimic targets`)", tt.arch));
        }
        // defaults: Linux gets musl (static executables, no system libc);
        // Apple OSes get a minimum version
        let mut name = if version.is_some() { t.split('.').next().unwrap_or(t).to_string() } else { t.to_string() };
        if tt.env.is_empty() {
            let a = tt.arch.as_str();
            let env = match tt.os.as_str() {
                "linux" if a.starts_with("arm") || a.starts_with("thumb") || matches!(a, "mips" | "mipsel" | "ppc") => {
                    Some("musleabihf")
                }
                "linux" if matches!(a, "mips64" | "mips64el") => Some("muslabi64"),
                "linux" => Some("musl"),
                "windows" => Some("gnu"),
                _ => None,
            };
            if let Some(e) = env {
                name = format!("{name}-{e}");
                tt = llvm_triple(&format!("{}-{e}", tt.triple));
            }
        }
        // Apple OSes: vendor `apple`, canonical `macosx` spelling
        if matches!(tt.os.as_str(), "macosx" | "ios" | "tvos" | "watchos" | "xros" | "driverkit") && tt.vendor != "apple" {
            let mut f: Vec<String> = tt.triple.split('-').map(String::from).collect();
            f[1] = "apple".into();
            if f[2].starts_with("macos") && !f[2].starts_with("macosx") {
                f[2] = f[2].replacen("macos", "macosx", 1);
            }
            tt = llvm_triple(&f.join("-"));
        }
        let apple_min = match tt.os.as_str() {
            "macosx" => Some("11.0"),
            "ios" | "tvos" => Some("14.0"),
            "watchos" => Some("7.0"),
            "xros" => Some("1.0"),
            _ => None,
        };
        if let Some(v) = apple_min
            && !tt.triple.split('-').nth(2).is_some_and(|os| os.ends_with(|c: char| c.is_ascii_digit()))
        {
            let mut f: Vec<String> = tt.triple.split('-').map(String::from).collect();
            f[2].push_str(v);
            tt = llvm_triple(&f.join("-"));
        }
        if let Some(e) = implied_env {
            name = format!("{name}-{e}");
        }
        if let Some(v) = &version {
            name = format!("{name}.{v}");
        }
        Ok(Target { name: Some(name), llvm: Some(tt.triple), arch: tt.arch, os: tt.os, env: tt.env, version })
    }

    pub fn name(&self) -> &str {
        self.name.as_deref().unwrap_or("native")
    }

    pub fn is_apple(&self) -> bool {
        matches!(self.os.as_str(), "macosx" | "ios" | "tvos" | "watchos" | "xros" | "driverkit" | "darwin")
    }

    /// GPU architectures whose backends cannot take a whole program (runtime
    /// calls, threads, function pointers); they need the GPU backend (F8),
    /// which compiles Mimic's array kernels for them.
    pub fn is_device_only(&self) -> bool {
        matches!(self.arch.as_str(), "amdgcn" | "amdgpu" | "r600" | "spirv" | "spirv32" | "spirv64" | "dxil")
    }

    pub fn exe_suffix(&self) -> &'static str {
        match self.os.as_str() {
            "windows" | "uefi" => ".exe",
            "wasi" | "emscripten" => ".wasm",
            _ => "",
        }
    }
}

/// The architectures LLVM knows, each with the code generator this build has
/// for it (if any).
pub fn arches() -> Vec<(String, Option<String>)> {
    llvm_names(Names::Arches)
        .into_iter()
        .map(|(a, _)| {
            let backend = llvm_triple(&format!("{a}-unknown-unknown")).backend;
            (a, backend)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_with_llvm() {
        let l = |t: &str| Target::parse(t).unwrap().llvm.unwrap();
        assert_eq!(l("x86_64-linux-gnu"), "x86_64-unknown-linux-gnu");
        assert_eq!(l("x86_64-linux"), "x86_64-unknown-linux-musl");
        assert_eq!(l("x86_64-unknown-linux-musl"), "x86_64-unknown-linux-musl");
        assert_eq!(l("aarch64-macos"), "aarch64-apple-macosx11.0");
        assert_eq!(l("aarch64-apple-macosx13.0"), "aarch64-apple-macosx13.0");
        assert_eq!(l("x86_64-windows"), "x86_64-unknown-windows-gnu");
        assert_eq!(l("x86-linux-musl"), "i686-unknown-linux-musl");
        assert_eq!(l("arm-linux"), "armv7a-unknown-linux-musleabihf");
        assert_eq!(l("wasm32-wasi"), "wasm32-unknown-wasi");
        let g = Target::parse("x86_64-linux-gnu.2.17").unwrap();
        assert_eq!(g.version.as_deref(), Some("2.17"));
        assert_eq!(g.name(), "x86_64-linux-gnu.2.17");
        assert_eq!(Target::parse("wasm32-wasi").unwrap().exe_suffix(), ".wasm");
        let err = |t: &str| Target::parse(t).err().unwrap();
        assert!(err("nonsense").contains("invalid target"));
        assert!(err("bogus-linux").contains("unknown architecture"));
        assert!(err("x86_64-linux-bogus").contains("not an OS"));
    }

    #[test]
    fn arch_list_comes_from_llvm() {
        let a = arches();
        assert!(a.len() > 40);
        assert!(a.iter().any(|(n, b)| n == "riscv64" && b.is_some()));
        assert!(a.iter().any(|(n, _)| n == "x86_64"));
    }
}
