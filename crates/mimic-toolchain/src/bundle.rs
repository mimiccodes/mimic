//! Sources embedded in the `mimic` binary, unpacked on first use into the cache.

use std::fs;
use std::path::{Path, PathBuf};

/// musl 1.2.5 (`musl-1.2.5/…`), the libc of the `*-linux-musl*` targets.
pub static MUSL: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/musl.tar.gz"));
/// compiler-rt's builtins (`builtins/…`): integer, float and atomic helpers.
pub static COMPILER_RT: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/compiler-rt.tar.gz"));
/// wasi-libc from wasi-sdk 34 (`wasi-sysroot/{include,lib}/wasm32-wasip1…`).
pub static WASI: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/wasi.tar.gz"));
/// Clang's resource headers (`include/…`): stddef.h, stdarg.h, intrinsics.
pub static CLANG_INCLUDE: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/clang-include.tar.gz"));

/// Unpack `blob` under `dest` once (keyed by the blob's hash) and return `dest`.
/// Concurrent `mimic` processes are safe: each unpacks into a private
/// directory and renames it into place.
pub fn unpack(blob: &[u8], dest: &Path) -> Result<PathBuf, String> {
    if blob.is_empty() {
        return Err("this mimic was built without the bundled sources (run tools/deps.sh and rebuild)".into());
    }
    let done = dest.join(".unpacked");
    if done.is_file() {
        return Ok(dest.to_path_buf());
    }
    let parent = dest.parent().ok_or("bad cache path")?;
    fs::create_dir_all(parent).map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
    let tmp = parent.join(format!(".tmp-{}-{}", std::process::id(), dest.file_name().unwrap().to_string_lossy()));
    let _ = fs::remove_dir_all(&tmp);
    let gz = flate2::read::GzDecoder::new(blob);
    tar::Archive::new(gz).unpack(&tmp).map_err(|e| format!("cannot unpack into {}: {e}", tmp.display()))?;
    fs::write(tmp.join(".unpacked"), b"").map_err(|e| e.to_string())?;
    if fs::rename(&tmp, dest).is_err() {
        // another process won the race
        let _ = fs::remove_dir_all(&tmp);
        if !done.is_file() {
            return Err(format!("cannot unpack into {}", dest.display()));
        }
    }
    Ok(dest.to_path_buf())
}
