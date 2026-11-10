//! Target triples. Users write Zig-style triples (`x86_64-linux-gnu`,
//! `aarch64-macos`, `x86_64-windows-gnu`, `wasm32-wasi`); LLVM needs its own
//! spelling. Zig does the linking, so every target Zig supports is reachable.

pub struct Target {
    /// As passed to `zig cc -target`; `None` = host.
    pub zig: Option<String>,
    /// LLVM triple for codegen; `None` = host.
    pub llvm: Option<String>,
    pub os: String,
}

impl Target {
    pub fn host() -> Target {
        Target { zig: None, llvm: None, os: std::env::consts::OS.to_string() }
    }

    pub fn parse(t: &str) -> Result<Target, String> {
        let parts: Vec<&str> = t.split('-').collect();
        if parts.len() < 2 {
            return Err(format!("invalid target `{t}`: expected <arch>-<os>[-<abi>], e.g. x86_64-linux-gnu"));
        }
        let arch = match parts[0] {
            "x86_64" | "aarch64" | "riscv64" | "wasm32" | "wasm64" | "powerpc64le" | "loongarch64" | "arm" | "thumb" | "mips" | "s390x" => parts[0],
            "x86" | "i386" | "i686" => "i686",
            "arm64" => "aarch64",
            other => return Err(format!("unsupported architecture `{other}` in target `{t}`")),
        };
        let os = parts[1];
        let abi = parts.get(2).copied();
        let llvm = match os {
            "linux" => format!("{arch}-unknown-linux-{}", abi.unwrap_or("gnu")),
            "macos" => format!("{arch}-apple-macosx11.0.0"),
            "windows" => format!("{arch}-pc-windows-{}", match abi.unwrap_or("gnu") {
                "msvc" => "msvc",
                _ => "gnu",
            }),
            "freebsd" | "netbsd" | "openbsd" => format!("{arch}-unknown-{os}"),
            "wasi" => format!("{arch}-unknown-wasi"),
            "freestanding" => format!("{arch}-unknown-unknown"),
            other => return Err(format!("unsupported OS `{other}` in target `{t}`")),
        };
        let zig_arch = if arch == "i686" { "x86" } else { arch };
        let zig = match abi {
            Some(a) => format!("{zig_arch}-{os}-{a}"),
            None => format!("{zig_arch}-{os}"),
        };
        Ok(Target { zig: Some(zig), llvm: Some(llvm), os: os.to_string() })
    }

    pub fn name(&self) -> &str {
        self.zig.as_deref().unwrap_or("native")
    }

    pub fn exe_suffix(&self) -> &'static str {
        match self.os.as_str() {
            "windows" => ".exe",
            "wasi" => ".wasm",
            _ => "",
        }
    }

    pub fn needs_libm(&self) -> bool {
        !matches!(self.os.as_str(), "windows" | "wasi" | "freestanding" | "macos")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn maps_triples() {
        assert_eq!(Target::parse("x86_64-linux-gnu").unwrap().llvm.unwrap(), "x86_64-unknown-linux-gnu");
        assert_eq!(Target::parse("aarch64-macos").unwrap().llvm.unwrap(), "aarch64-apple-macosx11.0.0");
        assert_eq!(Target::parse("x86_64-windows").unwrap().llvm.unwrap(), "x86_64-pc-windows-gnu");
        assert_eq!(Target::parse("wasm32-wasi").unwrap().exe_suffix(), ".wasm");
        assert!(Target::parse("nonsense").is_err());
    }
}
