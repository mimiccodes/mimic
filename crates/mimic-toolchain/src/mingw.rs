//! mingw-w64 for the `*-windows-gnu` targets (x86_64 and aarch64): the
//! Windows side of llvm-mingw (C headers, CRT objects, UCRT import
//! libraries, static winpthreads, compiler-rt's builtins), unpacked on first
//! use. Programs link statically against everything but Windows' own DLLs.

use crate::bundle;
use std::path::{Path, PathBuf};

pub const VERSION: &str = "20261020";

pub struct Sysroot {
    pub root: PathBuf,
    /// `x86_64` or `aarch64`
    pub arch: &'static str,
}

/// The mingw-w64 architecture of an LLVM triple, if bundled.
pub fn arch(triple: &str) -> Option<&'static str> {
    match triple.split('-').next()? {
        "x86_64" | "amd64" => Some("x86_64"),
        "aarch64" | "arm64" => Some("aarch64"),
        _ => None,
    }
}

impl Sysroot {
    /// CRT objects and libraries for this architecture.
    pub fn lib(&self) -> PathBuf {
        self.root.join("lib").join(format!("{}-w64-mingw32", self.arch))
    }
    /// compiler-rt's builtins (`___chkstk_ms`, 128-bit arithmetic, …).
    pub fn builtins(&self) -> PathBuf {
        self.root.join("lib").join(format!("libclang_rt.builtins-{}.a", self.arch))
    }
    /// Flags for compiling C against this sysroot.
    pub fn cflags(&self) -> Vec<String> {
        vec![
            format!("--sysroot={}", self.root.display()),
            "-isystem".into(),
            self.root.join("include").display().to_string(),
        ]
    }
    /// LLD's MinGW emulation name for the architecture.
    pub fn emulation(&self) -> &'static str {
        if self.arch == "aarch64" { "arm64pe" } else { "i386pep" }
    }
}

pub fn ensure(cache: &Path, triple: &str) -> Result<Sysroot, String> {
    let arch = arch(triple).ok_or_else(|| format!("mingw-w64 is bundled for x86_64 and aarch64 Windows, not `{triple}`"))?;
    let root = bundle::unpack(bundle::MINGW, &cache.join(format!("mingw-{VERSION}")))?.join("mingw-sysroot");
    Ok(Sysroot { root, arch })
}
