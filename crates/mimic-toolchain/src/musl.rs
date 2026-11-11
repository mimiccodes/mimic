//! musl libc for the `*-linux-musl*` targets, built on first use from the
//! bundled sources with the embedded Clang, exactly as musl's Makefile would:
//! the same source selection (an `src/*/<arch>/` file replaces the generic one
//! with the same name), flags and generated headers.
//!
//! Result: a sysroot `<cache>/musl-1.2.5/<triple>/` with `usr/include` and
//! `usr/lib/{libc.a, crt1.o, rcrt1.o, Scrt1.o, crti.o, crtn.o}`.

use crate::bundle;
use crate::jobs::{self, Job};
use std::fs;
use std::path::{Path, PathBuf};

pub const VERSION: &str = "1.2.5";

/// The musl targets that build and link complete static executables
/// (architecture × musl ABI, as musl supports them).
pub const TARGETS: &[&str] = &[
    "aarch64-linux-musl", "aarch64_be-linux-musl",
    "arm-linux-musleabi", "arm-linux-musleabihf", "armeb-linux-musleabi", "armeb-linux-musleabihf",
    "thumb-linux-musleabi", "thumb-linux-musleabihf", "thumbeb-linux-musleabi", "thumbeb-linux-musleabihf",
    "loongarch64-linux-musl", "loongarch64-linux-muslf32", "loongarch64-linux-muslsf",
    "mips-linux-musleabi", "mips-linux-musleabihf", "mipsel-linux-musleabi", "mipsel-linux-musleabihf",
    "mips64-linux-muslabi64", "mips64-linux-muslabin32", "mips64el-linux-muslabi64", "mips64el-linux-muslabin32",
    "powerpc-linux-musleabi", "powerpc-linux-musleabihf", "powerpc64-linux-musl", "powerpc64le-linux-musl",
    "riscv64-linux-musl", "s390x-linux-musl", "x86-linux-musl", "x86_64-linux-musl", "x86_64-linux-muslx32",
];

/// musl's name for the architecture of an LLVM triple, if musl supports it.
pub fn arch(triple: &str) -> Option<&'static str> {
    let (a, rest) = triple.split_once('-')?;
    let abi = rest.rsplit('-').next().unwrap_or("");
    Some(match a {
        "x86_64" if abi.contains("x32") => "x32",
        "x86_64" => "x86_64",
        "i386" | "i486" | "i586" | "i686" | "x86" => "i386",
        "aarch64" | "aarch64_be" | "arm64" => "aarch64",
        a if a.starts_with("arm") || a.starts_with("thumb") => "arm",
        "riscv64" => "riscv64",
        // riscv32 needs quad-precision soft-float helpers for musl's 128-bit
        // `long double`; compiler-rt only has them with 128-bit integers
        // (not on 32-bit targets), so riscv32 is not supported yet.
        "riscv32" => return None,
        "loongarch64" => "loongarch64",
        "powerpc64" | "powerpc64le" | "ppc64" | "ppc64le" => "powerpc64",
        "powerpc" | "ppc" => "powerpc",
        "mips" | "mipsel" => "mips",
        "mips64" | "mips64el" if abi.contains("n32") => "mipsn32",
        "mips64" | "mips64el" => "mips64",
        "s390x" => "s390x",
        "m68k" => "m68k",
        "or1k" => "or1k",
        "microblaze" | "microblazeel" => "microblaze",
        "sh" | "sheb" => "sh",
        _ => return None,
    })
}

/// Target-specific flags every C file for this triple needs.
pub fn target_flags(triple: &str) -> Vec<String> {
    let mut f = vec![format!("--target={triple}")];
    if triple.starts_with("aarch64") {
        // no outline-atomics helpers: they would need compiler-rt's lse.S variants
        f.push("-mno-outline-atomics".into());
    }
    f
}

pub struct Sysroot {
    pub root: PathBuf,
}

impl Sysroot {
    pub fn include(&self) -> PathBuf {
        self.root.join("usr/include")
    }
    pub fn lib(&self) -> PathBuf {
        self.root.join("usr/lib")
    }
}

/// The musl sysroot for `triple`, building it if needed (about a minute, once).
pub fn ensure(cache: &Path, triple: &str) -> Result<Sysroot, String> {
    let arch = arch(triple).ok_or_else(|| format!("musl does not support the architecture of `{triple}`"))?;
    let root = cache.join(format!("musl-{VERSION}")).join(triple);
    let done = root.join(".built");
    if done.is_file() {
        return Ok(Sysroot { root });
    }
    eprintln!("    Building musl {VERSION} for {triple} (first use; cached afterwards)");
    let src = bundle::unpack(bundle::MUSL, &cache.join(format!("src/musl-{VERSION}")))?.join(format!("musl-{VERSION}"));
    let work = root.join("obj");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(work.join("include/bits")).map_err(|e| e.to_string())?;
    fs::create_dir_all(work.join("src/internal")).map_err(|e| e.to_string())?;

    // generated headers
    let read = |p: PathBuf| fs::read_to_string(&p).map_err(|e| format!("cannot read {}: {e}", p.display()));
    let alltypes = mkalltypes(&read(src.join(format!("arch/{arch}/bits/alltypes.h.in")))?)
        + &mkalltypes(&read(src.join("include/alltypes.h.in"))?);
    fs::write(work.join("include/bits/alltypes.h"), alltypes).map_err(|e| e.to_string())?;
    let sys_in = read(src.join(format!("arch/{arch}/bits/syscall.h.in")))?;
    let mut syscall = sys_in.clone();
    for line in sys_in.lines().filter(|l| l.contains("__NR_")) {
        syscall.push_str(&line.replacen("__NR_", "SYS_", 1));
        syscall.push('\n');
    }
    fs::write(work.join("include/bits/syscall.h"), syscall).map_err(|e| e.to_string())?;
    fs::write(work.join("src/internal/version.h"), format!("#define VERSION \"{VERSION}\"\n")).map_err(|e| e.to_string())?;

    // sources: src/*/, src/malloc/mallocng, compat?, crt — arch files replace generic ones
    let mut dirs: Vec<PathBuf> = fs::read_dir(src.join("src"))
        .map_err(|e| e.to_string())?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    dirs.push(src.join("src/malloc/mallocng"));
    dirs.push(src.join("crt"));
    dirs.sort();
    let mut chosen: Vec<PathBuf> = Vec::new();
    for d in &dirs {
        let base = jobs::files(d, &["c"]);
        let arch_files = jobs::files(&d.join(arch), &["c", "s", "S"]);
        let replaced: Vec<String> = arch_files.iter().map(|p| stem(p)).collect();
        chosen.extend(base.into_iter().filter(|p| !replaced.contains(&stem(p))));
        chosen.extend(arch_files);
    }

    let common: Vec<String> = {
        let mut f = target_flags(triple);
        f.extend(
            [
                "-std=c99", "-nostdinc", "-ffreestanding", "-fexcess-precision=standard", "-frounding-math",
                "-fno-strict-aliasing", "-Wa,--noexecstack", "-D_XOPEN_SOURCE=700", "-Os", "-pipe",
                "-fomit-frame-pointer", "-fno-unwind-tables", "-fno-asynchronous-unwind-tables",
                "-ffunction-sections", "-fdata-sections", "-fno-stack-protector", "-w",
            ]
            .map(String::from),
        );
        for inc in [
            src.join(format!("arch/{arch}")),
            src.join("arch/generic"),
            work.join("src/internal"),
            src.join("src/include"),
            src.join("src/internal"),
            work.join("include"),
            src.join("include"),
        ] {
            f.push(format!("-I{}", inc.display()));
        }
        f
    };
    let mut libc_jobs = Vec::new();
    let mut crt_jobs = Vec::new();
    for p in &chosen {
        let rel = p.strip_prefix(&src).unwrap();
        let out = work.join(rel).with_extension("o");
        let mut args = common.clone();
        let is_crt = rel.starts_with("crt");
        if is_crt {
            args.push("-DCRT".into());
            let n = stem(p);
            if n == "Scrt1" || n == "rcrt1" {
                args.push("-fPIC".into());
            }
        }
        if ["memcpy", "memmove", "memcmp", "memset"].contains(&stem(p).as_str()) {
            args.push("-fno-builtin".into());
        }
        let job = Job { src: p.clone(), out, args };
        if is_crt { crt_jobs.push(job) } else { libc_jobs.push(job) }
    }
    jobs::run(&crt_jobs, false)?;
    jobs::run(&libc_jobs, false)?;

    // install: headers (include/, generic bits, arch bits, generated bits)
    let inc = root.join("usr/include");
    copy_dir(&src.join("include"), &inc)?;
    copy_dir(&src.join("arch/generic/bits"), &inc.join("bits"))?;
    copy_dir(&src.join(format!("arch/{arch}/bits")), &inc.join("bits"))?;
    copy_dir(&work.join("include/bits"), &inc.join("bits"))?;
    for f in fs::read_dir(inc.join("bits")).map_err(|e| e.to_string())?.flatten() {
        if f.path().extension().is_some_and(|e| e == "in") {
            let _ = fs::remove_file(f.path());
        }
    }
    let _ = fs::remove_file(inc.join("alltypes.h.in"));
    // libraries
    let lib = root.join("usr/lib");
    fs::create_dir_all(&lib).map_err(|e| e.to_string())?;
    for j in &crt_jobs {
        let name = format!("{}.o", stem(&j.src));
        fs::copy(&j.out, lib.join(name)).map_err(|e| e.to_string())?;
    }
    let objs: Vec<PathBuf> = libc_jobs.iter().map(|j| j.out.clone()).collect();
    crate::write_archive(&lib.join("libc.a"), crate::ArchiveKind::Gnu, &objs)?;
    // empty companions so `-lm -lpthread -lrt -ldl` work as on any Linux
    for empty in ["libm.a", "libpthread.a", "librt.a", "libdl.a", "libutil.a", "libxnet.a", "libcrypt.a", "libresolv.a"] {
        crate::write_archive(&lib.join(empty), crate::ArchiveKind::Gnu, &[])?;
    }
    let _ = fs::remove_dir_all(&work);
    fs::write(&done, b"").map_err(|e| e.to_string())?;
    Ok(Sysroot { root })
}

fn stem(p: &Path) -> String {
    p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

/// musl's tools/mkalltypes.sed.
fn mkalltypes(src: &str) -> String {
    let mut out = String::new();
    for line in src.lines() {
        let guarded = |kind: &str, name: &str, body: String| {
            let tag = if kind.is_empty() { name.to_string() } else { format!("{kind}_{name}") };
            format!("#if defined(__NEED_{tag}) && !defined(__DEFINED_{tag})\n{body}\n#define __DEFINED_{tag}\n#endif\n\n")
        };
        if let Some(rest) = line.strip_prefix("TYPEDEF ") {
            // `TYPEDEF <type> <name>;`
            let rest = rest.trim().trim_end_matches(';');
            let (ty, name) = rest.rsplit_once(' ').unwrap_or((rest, ""));
            out.push_str(&guarded("", name, format!("typedef {} {name};", ty.trim())));
        } else if let Some(rest) = line.strip_prefix("STRUCT ") {
            let rest = rest.trim().trim_end_matches(';');
            let (name, body) = rest.split_once(' ').unwrap_or((rest, ""));
            out.push_str(&guarded("struct", name, format!("struct {name} {};", body.trim())));
        } else if let Some(rest) = line.strip_prefix("UNION ") {
            let rest = rest.trim().trim_end_matches(';');
            let (name, body) = rest.split_once(' ').unwrap_or((rest, ""));
            out.push_str(&guarded("union", name, format!("union {name} {};", body.trim())));
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

fn copy_dir(from: &Path, to: &Path) -> Result<(), String> {
    fs::create_dir_all(to).map_err(|e| e.to_string())?;
    for e in fs::read_dir(from).map_err(|e| format!("cannot read {}: {e}", from.display()))?.flatten() {
        let p = e.path();
        let dest = to.join(e.file_name());
        if p.is_dir() {
            copy_dir(&p, &dest)?;
        } else {
            fs::copy(&p, &dest).map_err(|e| format!("cannot copy {}: {e}", p.display()))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alltypes_matches_the_sed_script() {
        let got = mkalltypes("TYPEDEF unsigned _Addr size_t;\nSTRUCT timeval { time_t tv_sec; suseconds_t tv_usec; };\n#define X 1\n");
        assert!(got.contains("#if defined(__NEED_size_t) && !defined(__DEFINED_size_t)\ntypedef unsigned _Addr size_t;\n#define __DEFINED_size_t\n#endif\n"));
        assert!(got.contains("struct timeval { time_t tv_sec; suseconds_t tv_usec; };"));
        assert!(got.contains("__NEED_struct_timeval"));
        assert!(got.contains("#define X 1\n"));
    }

    #[test]
    fn arches() {
        assert_eq!(arch("x86_64-unknown-linux-musl"), Some("x86_64"));
        assert_eq!(arch("x86_64-unknown-linux-muslx32"), Some("x32"));
        assert_eq!(arch("mips64el-unknown-linux-muslabin32"), Some("mipsn32"));
        assert_eq!(arch("aarch64_be-unknown-linux-musl"), Some("aarch64"));
        assert_eq!(arch("armebv7a-unknown-linux-musleabi"), Some("arm"));
        assert_eq!(arch("wasm32-unknown-wasi"), None);
        assert_eq!(TARGETS.len(), 30);
    }
}
