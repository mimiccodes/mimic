//! The linker (LLD) and C compiler (Clang) embedded in the `mimic` binary.
//!
//! Both run as `mimic` itself in a child process (`mimic ld.lld …`,
//! `mimic cc …`), like `zig cc`: each tool gets a fresh process, so their
//! global state and signal handlers never touch the compiler's.

pub mod bundle;

use std::ffi::{CString, c_char, c_int};

unsafe extern "C" {
    fn mimic_lld_main(argc: c_int, argv: *const *const c_char) -> c_int;
    fn mimic_write_archive(out: *const c_char, kind: c_int, n: c_int, members: *const *const c_char, err: *mut c_char, errlen: usize) -> c_int;
    fn mimic_clang_main(argc: c_int, argv: *const *mut c_char, self_path: *const c_char) -> c_int;
}

/// The LLVM install the compiler was built against (until its files are bundled).
pub const LLVM_PREFIX: &str = env!("MIMIC_LLVM_PREFIX");
/// Clang's major version, which names its resource directory.
pub const LLVM_MAJOR: &str = env!("MIMIC_LLVM_MAJOR");

/// True when this binary contains the real tools (not the stubs).
pub const EMBEDDED: bool = cfg!(mimic_toolchain_embedded);

/// LLD flavours, selected by the program name like the real tools.
pub const LLD_FLAVOURS: &[&str] = &["ld.lld", "ld64.lld", "lld-link", "wasm-ld"];

fn c_args(args: &[String]) -> Vec<CString> {
    args.iter().map(|a| CString::new(a.as_bytes()).unwrap_or_else(|_| CString::new("?").unwrap())).collect()
}

/// Run LLD in this process. `args[0]` is the flavour (`ld.lld`, `ld64.lld`, …).
pub fn lld_main(args: &[String]) -> i32 {
    let owned = c_args(args);
    let ptrs: Vec<*const c_char> = owned.iter().map(|c| c.as_ptr()).collect();
    unsafe { mimic_lld_main(ptrs.len() as c_int, ptrs.as_ptr()) }
}

/// Run the Clang driver in this process. `args[0]` is the program name.
pub fn clang_main(args: &[String]) -> i32 {
    let owned = c_args(args);
    let mut ptrs: Vec<*mut c_char> = owned.iter().map(|c| c.as_ptr() as *mut c_char).collect();
    ptrs.push(std::ptr::null_mut());
    let me = std::env::current_exe().map(|p| p.to_string_lossy().into_owned()).unwrap_or_else(|_| "mimic".into());
    let me = CString::new(me).unwrap();
    unsafe { mimic_clang_main(owned.len() as c_int, ptrs.as_ptr(), me.as_ptr()) }
}

/// Archive formats for [`write_archive`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArchiveKind {
    /// ELF and WebAssembly targets.
    Gnu,
    /// Mach-O (Apple) targets.
    Darwin,
    /// Windows targets.
    Coff,
}

/// Write a static library (`.a`/`.lib`) with a symbol index, in-process.
pub fn write_archive(out: &std::path::Path, kind: ArchiveKind, members: &[std::path::PathBuf]) -> Result<(), String> {
    let out_c = CString::new(out.to_string_lossy().as_bytes()).map_err(|e| e.to_string())?;
    let owned: Vec<CString> =
        members.iter().map(|m| CString::new(m.to_string_lossy().as_bytes())).collect::<Result<_, _>>().map_err(|e| e.to_string())?;
    let ptrs: Vec<*const c_char> = owned.iter().map(|c| c.as_ptr()).collect();
    let mut err = vec![0 as c_char; 1024];
    let k = match kind {
        ArchiveKind::Gnu => 0,
        ArchiveKind::Darwin => 1,
        ArchiveKind::Coff => 2,
    };
    let rc = unsafe { mimic_write_archive(out_c.as_ptr(), k, ptrs.len() as c_int, ptrs.as_ptr(), err.as_mut_ptr(), err.len()) };
    if rc == 0 {
        Ok(())
    } else {
        let msg = unsafe { std::ffi::CStr::from_ptr(err.as_ptr()) }.to_string_lossy().into_owned();
        Err(format!("cannot write {}: {msg}", out.display()))
    }
}
