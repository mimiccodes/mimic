//! Sources embedded in the `mimic` binary, unpacked on first use into the cache.

use std::fs;
use std::path::{Path, PathBuf};

/// musl 1.2.5 (`musl-1.2.5/…`), the libc of the `*-linux-musl*` targets.
pub static MUSL: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/musl.tar.gz"));
/// compiler-rt's builtins (`builtins/…`): integer, float and atomic helpers.
pub static COMPILER_RT: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/compiler-rt.tar.gz"));
/// wasi-libc from wasi-sdk 34 (`wasi-sysroot/{include,lib}/wasm32-wasip1…`).
pub static WASI: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/wasi.tar.gz"));
/// mingw-w64 from llvm-mingw (`mingw-sysroot/{include,lib/<arch>-w64-mingw32}`):
/// the C library of the Windows targets (UCRT), with static winpthreads.
pub static MINGW: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/mingw.tar.gz"));
/// Mbed TLS 3.6 (`mbedtls/{library,include,LICENSE}`): TLS for the static
/// Linux targets, which cannot load the system's OpenSSL.
pub static MBEDTLS: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/mbedtls.tar.gz"));
/// Clang's resource headers (`include/…`): stddef.h, stdarg.h, intrinsics.
pub static CLANG_INCLUDE: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/clang-include.tar.gz"));

/// Unpack `blob` under `dest` once (keyed by the blob's hash) and return `dest`.
/// Concurrent `mimic` processes are safe: each unpacks into a private
/// directory and renames it into place.
pub fn unpack(blob: &[u8], dest: &Path) -> Result<PathBuf, String> {
    if blob.is_empty() {
        return Err("this mimic was built without the bundled sources (run tools/deps.sh and rebuild)".into());
    }
    if crate::jobs::intact(dest, ".unpacked") {
        return Ok(dest.to_path_buf());
    }
    let parent = dest.parent().ok_or("bad cache path")?;
    fs::create_dir_all(parent).map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
    let name = dest.file_name().unwrap().to_string_lossy().to_string();
    let tmp = parent.join(format!(".tmp-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&tmp);
    let gz = flate2::read::GzDecoder::new(blob);
    tar::Archive::new(gz).unpack(&tmp).map_err(|e| format!("cannot unpack into {}: {e}", tmp.display()))?;
    crate::jobs::seal(&tmp, ".unpacked")?;
    for attempt in 0..3 {
        // another process may have finished first
        if crate::jobs::intact(dest, ".unpacked") {
            let _ = fs::remove_dir_all(&tmp);
            return Ok(dest.to_path_buf());
        }
        // a damaged copy (files purged from a temporary directory, an
        // interrupted run): move it aside atomically, never delete in place
        if dest.exists() {
            let stale = parent.join(format!(".stale-{}-{attempt}-{name}", std::process::id()));
            if fs::rename(dest, &stale).is_ok() {
                let _ = fs::remove_dir_all(&stale);
            }
        }
        if fs::rename(&tmp, dest).is_ok() {
            return Ok(dest.to_path_buf());
        }
    }
    let _ = fs::remove_dir_all(&tmp);
    if crate::jobs::intact(dest, ".unpacked") { Ok(dest.to_path_buf()) } else { Err(format!("cannot unpack into {}", dest.display())) }
}
