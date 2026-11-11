//! wasi-libc for the `wasm32-wasi` targets: the sysroot of wasi-sdk 34
//! (WebAssembly code, the same on every host), unpacked on first use, plus
//! compiler-rt built for wasm32 from the bundled sources.

use crate::bundle;
use std::path::{Path, PathBuf};

pub const SDK: &str = "34";
/// Clang's name for the target: its sysroot directories are named after it.
pub const CLANG_TRIPLE: &str = "wasm32-wasip1";

pub struct Sysroot {
    pub root: PathBuf,
    pub compiler_rt: PathBuf,
}

impl Sysroot {
    pub fn lib(&self) -> PathBuf {
        self.root.join("lib").join(CLANG_TRIPLE)
    }
}

/// Flags for compiling C for WASI against `root`.
pub fn cflags(root: &Path) -> Vec<String> {
    vec![format!("--target={CLANG_TRIPLE}"), format!("--sysroot={}", root.display())]
}

pub fn ensure(cache: &Path, resource_dir: &Path) -> Result<Sysroot, String> {
    let root = bundle::unpack(bundle::WASI, &cache.join(format!("wasi-sdk-{SDK}")))?.join("wasi-sysroot");
    let mut flags = cflags(&root);
    flags.push("-resource-dir".into());
    flags.push(resource_dir.display().to_string());
    let compiler_rt = crate::compiler_rt::ensure(cache, CLANG_TRIPLE, &flags)?;
    Ok(Sysroot { root, compiler_rt })
}
