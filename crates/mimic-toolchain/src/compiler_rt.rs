//! compiler-rt's builtins (what libgcc is to GCC): 128-bit integer and
//! `long double` arithmetic, soft-float, conversions. Built per target from
//! the bundled sources on first use, like compiler-rt's CMake does: generic C
//! files, with an architecture's own files replacing those of the same name.

use crate::bundle;
use crate::jobs::{self, Job};
use std::fs;
use std::path::{Path, PathBuf};

/// Generic files that are not wanted for the targets built here.
const SKIP: &[&str] = &[
    "os_version_check",    // Apple only
    "apple_versioning",    // Apple only
    "emutls",              // emulated TLS: musl has real TLS
    "enable_execute_stack",// trampolines on executable stacks
    "eprintf",             // ancient Apple ABI
    "gcc_personality_v0",  // C exception personality (needs unwinder)
    "trampoline_setup",    // ppc32 nested functions
];

/// compiler-rt's `set_special_properties(… SUPERSEDES …)`: an architecture file
/// that also defines the functions of other files (by object name).
const SUPERSEDES: &[(&str, &[&str])] = &[
    ("arm_addsf3", &["subsf3"]),
    ("arm_adddf3", &["subdf3"]),
    ("arm_cmpdf2", &["comparedf2"]),
    ("arm_cmpsf2", &["comparesf2", "arm_comparesf2"]),
    ("arm_floatdisf", &["floatundisf"]),
];

/// The compiler-rt source subdirectory for an architecture, and which of its files to use.
fn arch_dir(triple: &str) -> Option<(&'static str, &'static [&'static str])> {
    let a = triple.split('-').next()?;
    Some(match a {
        "x86_64" => ("x86_64", &[]),
        "i386" | "i486" | "i586" | "i686" | "x86" => ("i386", &[]),
        "aarch64" | "aarch64_be" | "arm64" => ("aarch64", &["fp_mode"]),
        a if a.starts_with("arm") || a.starts_with("thumb") => ("arm", &[]),
        "riscv32" | "riscv64" => ("riscv", &[]),
        "loongarch64" => ("loongarch", &["fp_mode"]),
        "powerpc" | "powerpc64" | "powerpc64le" => ("ppc", &[]),
        "hexagon" => ("hexagon", &[]),
        _ => return None,
    })
}

/// `libcompiler_rt.a` for `triple`, building it if needed. `cflags` carries the
/// target and sysroot flags (libc headers for the few files that use them).
pub fn ensure(cache: &Path, triple: &str, cflags: &[String]) -> Result<PathBuf, String> {
    let dir = cache.join("compiler-rt").join(triple);
    let lib = dir.join("libcompiler_rt.a");
    if lib.is_file() {
        return Ok(lib);
    }
    let src = bundle::unpack(bundle::COMPILER_RT, &cache.join("src/compiler-rt"))?.join("builtins");
    let mut chosen: Vec<PathBuf> = Vec::new();
    let generic = jobs::files(&src, &["c"]);
    let mut arch_files = Vec::new();
    if let Some((sub, only)) = arch_dir(triple) {
        arch_files = jobs::files(&src.join(sub), &["c", "S"])
            .into_iter()
            .filter(|p| only.is_empty() || only.contains(&stem(p).as_str()))
            .filter(|p| !stem(p).starts_with("chkstk") && !stem(p).starts_with("sme") && stem(p) != "lse")
            .collect();
    }
    let replaced: Vec<String> = arch_files.iter().map(|p| stem(p)).collect();
    chosen.extend(generic.into_iter().filter(|p| !SKIP.contains(&stem(p).as_str()) && !replaced.contains(&stem(p))));
    chosen.extend(arch_files);

    let work = dir.join("obj");
    let mut args = cflags.to_vec();
    args.extend(
        ["-O2", "-std=c11", "-fno-builtin", "-fvisibility=hidden", "-fomit-frame-pointer", "-ffreestanding", "-w"]
            .map(String::from),
    );
    args.push(format!("-I{}", src.display()));
    let jobs: Vec<Job> = chosen
        .iter()
        .map(|p| Job { src: p.clone(), out: work.join(format!("{}.o", rel_name(&src, p))), args: args.clone() })
        .collect();
    // a few generic files do not apply to every target (e.g. x87 helpers on
    // non-x86); skip failures. An architecture file that does not assemble for
    // this CPU (ARMv7 assembly on ARMv6, …) falls back to the generic C file.
    let mut failed = jobs::run(&jobs, true)?;
    let fallback: Vec<Job> = failed
        .iter()
        .filter(|p| p.parent() != Some(src.as_path()))
        .map(|p| src.join(format!("{}.c", stem(p))))
        .filter(|g| g.is_file() && !SKIP.contains(&stem(g).as_str()))
        .map(|g| Job { out: work.join(format!("{}.o", rel_name(&src, &g))), src: g, args: args.clone() })
        .collect();
    failed.extend(jobs::run(&fallback, true)?);
    let mut objs: Vec<PathBuf> = jobs
        .iter()
        .chain(&fallback)
        .filter(|j| !failed.contains(&j.src) && j.out.is_file())
        .map(|j| j.out.clone())
        .collect();
    let built: Vec<String> = objs.iter().map(|o| stem(o)).collect();
    let superseded: Vec<&str> =
        SUPERSEDES.iter().filter(|(by, _)| built.iter().any(|b| b == by)).flat_map(|(_, gone)| gone.iter().copied()).collect();
    objs.retain(|o| !superseded.contains(&stem(o).as_str()));
    if objs.is_empty() {
        return Err(format!("could not build compiler-rt for {triple}"));
    }
    crate::write_archive(&lib, crate::ArchiveKind::Gnu, &objs)?;
    if !failed.is_empty() {
        let names: Vec<String> = failed.iter().map(|p| rel_name(&src, p)).collect();
        let _ = fs::write(dir.join("skipped.txt"), names.join("\n"));
    }
    let _ = fs::remove_dir_all(&work);
    Ok(lib)
}

fn stem(p: &Path) -> String {
    p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

/// `aarch64/fp_mode.c` → `aarch64_fp_mode` (unique object names).
fn rel_name(root: &Path, p: &Path) -> String {
    p.strip_prefix(root).unwrap_or(p).with_extension("").to_string_lossy().replace(['/', '\\'], "_")
}
