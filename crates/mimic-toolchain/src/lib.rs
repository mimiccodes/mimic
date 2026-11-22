//! The linker (LLD) and C compiler (Clang) embedded in the `mimic` binary.
//!
//! Both run as `mimic` itself in a child process (`mimic ld.lld …`,
//! `mimic cc …`): each tool gets a fresh process, so its global state and
//! signal handlers never touch the compiler's.

pub mod bundle;
pub mod compiler_rt;
pub mod jobs;
pub mod mbedtls;
pub mod mingw;
pub mod musl;
pub mod wasi;

use std::ffi::{CString, c_char, c_int};

unsafe extern "C" {
    fn mimic_split_codegen(module: *mut std::ffi::c_void, tm: *mut std::ffi::c_void, n: c_int, paths: *const *const c_char, err: *mut c_char, errlen: usize) -> c_int;
    fn mimic_lld_main(argc: c_int, argv: *const *const c_char) -> c_int;
    fn mimic_lld_link(argc: c_int, argv: *const *const c_char, msg: *mut *mut c_char) -> c_int;
    fn free(p: *mut std::ffi::c_void);
    fn mimic_write_archive(out: *const c_char, kind: c_int, n: c_int, members: *const *const c_char, err: *mut c_char, errlen: usize) -> c_int;
    fn mimic_list_cpus(
        triple: *const c_char,
        what: c_int,
        cb: extern "C" fn(*const c_char, *const c_char, *mut std::ffi::c_void),
        ud: *mut std::ffi::c_void,
    ) -> c_int;
    fn mimic_llvm_names(kind: c_int, cb: extern "C" fn(*const c_char, *const c_char, *mut std::ffi::c_void), ud: *mut std::ffi::c_void);
    fn mimic_llvm_triple(input: *const c_char, out: *mut c_char, outlen: usize);
    fn mimic_cpu_valid(triple: *const c_char, cpu: *const c_char) -> c_int;
    fn mimic_clang_main(argc: c_int, argv: *const *mut c_char, self_path: *const c_char) -> c_int;
}

/// The LLVM install the compiler was built against (until its files are bundled).
pub const LLVM_PREFIX: &str = env!("MIMIC_LLVM_PREFIX");
/// Clang's major version, which names its resource directory.
pub const LLVM_MAJOR: &str = env!("MIMIC_LLVM_MAJOR");

/// Clang's resource directory (its own headers), unpacked from the binary.
pub fn resource_dir(cache: &std::path::Path) -> Result<std::path::PathBuf, String> {
    bundle::unpack(bundle::CLANG_INCLUDE, &cache.join(format!("clang-{LLVM_MAJOR}")))
}

/// Set on `mimic cc` processes that mimic starts itself (runtime, libc and
/// compiler-rt builds, target queries): they run Clang exactly as given and
/// never prepare a sysroot, which would recurse into building one.
pub const RAW_CC_ENV: &str = "MIMIC_CC_RAW";

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

/// Link with LLD in this process, one link at a time (LLD keeps global
/// state): its exit code and what it printed.
pub fn lld_link(args: &[String]) -> (i32, String) {
    static ONE: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let owned = c_args(args);
    let ptrs: Vec<*const c_char> = owned.iter().map(|c| c.as_ptr()).collect();
    let mut msg: *mut c_char = std::ptr::null_mut();
    let code = unsafe { mimic_lld_link(ptrs.len() as c_int, ptrs.as_ptr(), &mut msg) };
    let text = if msg.is_null() {
        String::new()
    } else {
        let t = unsafe { std::ffi::CStr::from_ptr(msg) }.to_string_lossy().into_owned();
        unsafe { free(msg.cast()) };
        t
    };
    (code, text)
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

/// CPU models (`features = false`) or (feature, description) pairs that LLVM
/// knows for `triple`; `None` if there is no backend for it.
pub fn cpus(triple: &str, features: bool) -> Option<Vec<(String, String)>> {
    extern "C" fn push(k: *const c_char, d: *const c_char, ud: *mut std::ffi::c_void) {
        let v = unsafe { &mut *(ud as *mut Vec<(String, String)>) };
        let s = |p: *const c_char| unsafe { std::ffi::CStr::from_ptr(p) }.to_string_lossy().into_owned();
        v.push((s(k), s(d)));
    }
    let t = CString::new(triple).ok()?;
    let mut v: Vec<(String, String)> = Vec::new();
    let rc = unsafe { mimic_list_cpus(t.as_ptr(), c_int::from(features), push, &mut v as *mut _ as *mut std::ffi::c_void) };
    (rc == 0).then_some(v)
}

extern "C" fn push_pair(k: *const c_char, d: *const c_char, ud: *mut std::ffi::c_void) {
    let v = unsafe { &mut *(ud as *mut Vec<(String, String)>) };
    let s = |p: *const c_char| unsafe { std::ffi::CStr::from_ptr(p) }.to_string_lossy().into_owned();
    v.push((s(k), s(d)));
}

/// What LLVM knows, straight from the linked library.
#[derive(Clone, Copy)]
pub enum Names {
    Arches = 0,
    Vendors = 1,
    Oses = 2,
    Environments = 3,
    /// Code generators in this build: (name, description).
    Backends = 4,
}

pub fn llvm_names(kind: Names) -> Vec<(String, String)> {
    let mut v: Vec<(String, String)> = Vec::new();
    unsafe { mimic_llvm_names(kind as c_int, push_pair, &mut v as *mut _ as *mut std::ffi::c_void) };
    v
}

/// A triple as LLVM's parser understands it.
#[derive(Clone, Debug, Default)]
pub struct Triple {
    /// Normalised: `x86_64-unknown-linux-musl`.
    pub triple: String,
    /// Empty fields are unknown to LLVM.
    pub arch: String,
    pub vendor: String,
    pub os: String,
    pub env: String,
    /// The code generator for it, if this build has one.
    pub backend: Option<String>,
}

pub fn llvm_triple(input: &str) -> Triple {
    let Ok(c) = CString::new(input) else { return Triple::default() };
    let mut buf = vec![0 as c_char; 512];
    unsafe { mimic_llvm_triple(c.as_ptr(), buf.as_mut_ptr(), buf.len()) };
    let s = unsafe { std::ffi::CStr::from_ptr(buf.as_ptr()) }.to_string_lossy().into_owned();
    let f: Vec<&str> = s.split('\t').collect();
    let get = |i: usize| f.get(i).map(|x| x.to_string()).unwrap_or_default();
    Triple {
        triple: get(0),
        arch: get(1),
        vendor: get(2),
        os: get(3),
        env: get(4),
        backend: Some(get(5)).filter(|b| !b.is_empty()),
    }
}

/// Does LLVM accept `cpu` (a model or an alias) for `triple`?
/// `None` when this build has no code generator for it.
pub fn cpu_valid(triple: &str, cpu: &str) -> Option<bool> {
    let (t, c) = (CString::new(triple).ok()?, CString::new(cpu).ok()?);
    match unsafe { mimic_cpu_valid(t.as_ptr(), c.as_ptr()) } {
        -1 => None,
        v => Some(v == 1),
    }
}

/// Machine code for an optimised LLVM module in parts at once, one object
/// file per path (see mimic_split_codegen in shim.cpp). Returns how many of
/// the paths were written (a module may split into fewer parts).
///
/// # Safety
/// `module` must be a live LLVMModuleRef and `tm` a live
/// LLVMTargetMachineRef, neither used elsewhere during the call.
pub unsafe fn split_codegen(module: *mut std::ffi::c_void, tm: *mut std::ffi::c_void, paths: &[std::path::PathBuf]) -> Result<usize, String> {
    let cs: Vec<std::ffi::CString> = paths.iter().map(|p| std::ffi::CString::new(p.to_string_lossy().as_bytes()).unwrap()).collect();
    let ptrs: Vec<*const c_char> = cs.iter().map(|c| c.as_ptr()).collect();
    let mut err = vec![0 as c_char; 1024];
    let r = unsafe { mimic_split_codegen(module, tm, paths.len() as c_int, ptrs.as_ptr(), err.as_mut_ptr(), err.len()) };
    if r > 0 {
        let msg = unsafe { std::ffi::CStr::from_ptr(err.as_ptr()) }.to_string_lossy().into_owned();
        return Err(format!("parallel code generation: {msg}"));
    }
    Ok((-r) as usize)
}
