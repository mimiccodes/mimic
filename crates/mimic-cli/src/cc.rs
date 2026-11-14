//! `mimic cc` / `mimic c++`: Clang, with everything a target needs filled in.
//!
//! `mimic cc --target=aarch64-linux hello.c -o hello` produces a static Linux
//! executable with no other flags: the target is normalised like `mimic build`
//! does it, and the C library (built from the bundled sources on first use),
//! start-up files, compiler-rt, Clang's headers and the embedded LLD are added.

use crate::targets::Target;
use crate::toolchain::{self, Sysroot, Toolchain};
use mimic_project::packages;
use std::path::{Path, PathBuf};

/// Run the embedded Clang for `mimic cc` (`cxx`: `mimic c++`). `rest` is
/// everything after the subcommand.
pub fn main(cxx: bool, rest: &[String]) -> i32 {
    let mut args = vec![if cxx { "clang++".to_string() } else { "clang".to_string() }];
    // `mimic cc -cc1 …` is Clang re-running itself, and mimic's own compile
    // jobs say exactly what they want: pass both straight through
    let raw = std::env::var_os(mimic_toolchain::RAW_CC_ENV).is_some();
    if raw || rest.first().is_some_and(|a| a.starts_with("-cc1")) {
        args.extend(rest.iter().cloned());
        return mimic_toolchain::clang_main(&args);
    }
    // guard (driver mode only): preparing a sysroot runs `mimic cc` again (raw); if anything ever
    // re-enters this path, stop instead of multiplying processes
    let depth: u32 = std::env::var("MIMIC_CC_DEPTH").ok().and_then(|d| d.parse().ok()).unwrap_or(0);
    if depth >= 2 {
        eprintln!("error: mimic cc re-entered itself while preparing a target (internal error)");
        return 1;
    }
    // SAFETY: single-threaded at this point (before any tool runs)
    unsafe { std::env::set_var("MIMIC_CC_DEPTH", (depth + 1).to_string()) };

    let mut user: Vec<String> = rest.to_vec();
    let mut pre: Vec<String> = Vec::new();
    let mut post: Vec<String> = Vec::new();
    let mut flavour_target: Option<String> = None;

    if let Some((at, value)) = target_arg(&user) {
        let t = match Target::parse(&value) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("error: {e}");
                return 1;
            }
        };
        let triple = t.llvm.clone().unwrap_or(value);
        // normalised: `aarch64-linux` means musl, like `mimic build`
        match at {
            TargetAt::Joined(i) => user[i] = format!("--target={triple}"),
            TargetAt::Split(i) => {
                user[i] = format!("--target={triple}");
                user.remove(i + 1);
            }
        }
        flavour_target = Some(triple.clone());
        match toolchain::select(&t) {
            Ok(Toolchain::Embedded(Sysroot::Musl { root, compiler_rt, triple })) => {
                pre.extend(mimic_toolchain::musl::target_flags(&triple).into_iter().filter(|f| !f.starts_with("--target")));
                pre.push(format!("--sysroot={}", root.display()));
                if links(&user) && !user.iter().any(|a| a == "-shared") {
                    let lib = root.join("usr/lib");
                    pre.extend(["-nostdlib".into(), "-static".into()]);
                    pre.push(path(&lib.join("crt1.o")));
                    pre.push(path(&lib.join("crti.o")));
                    post.push(format!("-L{}", lib.display()));
                    post.push("-Wl,--start-group".into());
                    post.push(path(&lib.join("libc.a")));
                    post.push(path(&compiler_rt));
                    post.push("-Wl,--end-group".into());
                    post.push(path(&lib.join("crtn.o")));
                }
            }
            Ok(Toolchain::Embedded(Sysroot::Wasi { root, compiler_rt })) => {
                // clang's WASI driver finds crt1, -lc and the headers under the sysroot
                if let Some(i) = at_index(&user) {
                    user[i] = format!("--target={}", mimic_toolchain::wasi::CLANG_TRIPLE);
                }
                pre.push(format!("--sysroot={}", root.display()));
                if links(&user) {
                    post.push(path(&compiler_rt));
                    pre.push("-nodefaultlibs".into());
                    post.push("-lc".into());
                }
            }
            Ok(Toolchain::Embedded(Sysroot::Mingw(m))) => {
                pre.extend(m.cflags());
                if links(&user) && !user.iter().any(|a| a == "-shared") {
                    // everything explicit, as `mimic build` links it
                    let lib = m.lib();
                    pre.extend(["-nostdlib".into(), "-static".into()]);
                    pre.push(path(&lib.join("crt2.o")));
                    pre.push(path(&lib.join("crtbegin.o")));
                    post.push(format!("-L{}", lib.display()));
                    for l in ["mingw32", "mingwex", "ucrt", "pthread", "kernel32", "user32", "ws2_32", "advapi32", "bcrypt", "secur32", "crypt32", "ncrypt", "shell32", "ntdll", "mingw32", "mingwex", "ucrt"] {
                        post.push(format!("-l{l}"));
                    }
                    post.push(path(&m.builtins()));
                    post.push("-lkernel32".into());
                    post.push(path(&lib.join("crtend.o")));
                }
            }
            Ok(Toolchain::Embedded(Sysroot::Darwin { sdk, .. })) => {
                if !user.iter().any(|a| a == "-isysroot" || a.starts_with("--sysroot")) {
                    pre.extend(["-isysroot".into(), path(&sdk)]);
                }
            }
            Ok(Toolchain::System) => {}
            // no bundled C library: freestanding C still compiles
            Err(why) => eprintln!("note: {}", why.replace("cannot build executables for", "no C library (headers, libc) for")),
        }
    } else if cfg!(target_os = "macos")
        && !user.iter().any(|a| a == "-isysroot" || a.starts_with("--sysroot"))
        && let Some(sdk) = toolchain::xcode_sdk("macosx")
    {
        // native macOS: the SDK's headers and libraries (Clang finds no default)
        pre.extend(["-isysroot".into(), path(&sdk)]);
    }

    let cache = packages::cache_dir().join("libc");
    if !user.iter().any(|a| a.starts_with("-resource-dir"))
        && let Ok(dir) = mimic_toolchain::resource_dir(&cache)
    {
        args.push("-resource-dir".into());
        args.push(path(&dir));
    }
    // link with the embedded LLD, not whatever linker the system has
    if links(&user)
        && !user.iter().any(|a| a.starts_with("-fuse-ld") || a.starts_with("--ld-path"))
        && let Some(ld) = tool_link(lld_flavour(flavour_target.as_deref()))
    {
        if ld.ends_with("wasm-ld") {
            // Clang's WebAssembly driver looks `wasm-ld` up by name
            args.push(format!("-B{}", ld.parent().unwrap().display()));
        } else {
            args.push(format!("--ld-path={}", ld.display()));
        }
    }
    args.extend(pre);
    args.extend(user);
    args.extend(post);
    mimic_toolchain::clang_main(&args)
}

/// Where `--target` is, after normalisation (always the joined form).
fn at_index(args: &[String]) -> Option<usize> {
    args.iter().position(|a| a.starts_with("--target="))
}

enum TargetAt {
    /// `--target=x`
    Joined(usize),
    /// `-target x` / `--target x`
    Split(usize),
}

fn target_arg(args: &[String]) -> Option<(TargetAt, String)> {
    for (i, a) in args.iter().enumerate() {
        if let Some(v) = a.strip_prefix("--target=") {
            return Some((TargetAt::Joined(i), v.to_string()));
        }
        if (a == "-target" || a == "--target") && i + 1 < args.len() {
            return Some((TargetAt::Split(i), args[i + 1].clone()));
        }
    }
    None
}

/// Does this invocation link (rather than compile, preprocess or query)?
fn links(args: &[String]) -> bool {
    let stops = ["-c", "-S", "-E", "-fsyntax-only", "-M", "-MM", "-###", "--version", "-v", "-print-resource-dir"];
    !args.iter().any(|a| stops.contains(&a.as_str()) || a.starts_with("-print-"))
        && args.iter().any(|a| !a.starts_with('-'))
}

fn path(p: &Path) -> String {
    p.display().to_string()
}

/// The LLD flavour Clang should link with.
fn lld_flavour(target: Option<&str>) -> &'static str {
    let target = target.map(String::from).unwrap_or_else(|| if cfg!(target_os = "macos") { "apple-darwin".into() } else { "linux".into() });
    if target.contains("apple") || target.contains("darwin") || target.contains("macos") {
        "ld64.lld"
    } else if target.contains("windows-msvc") {
        "lld-link"
    } else if target.starts_with("wasm") {
        "wasm-ld"
    } else {
        "ld.lld"
    }
}

/// `<cache>/bin/<name>`: a link to this binary, so tools that run a linker by
/// path (Clang's driver) get the embedded one.
fn tool_link(name: &str) -> Option<PathBuf> {
    if !mimic_toolchain::EMBEDDED {
        return None;
    }
    let me = std::env::current_exe().ok()?;
    let me = std::fs::canonicalize(&me).unwrap_or(me);
    let dir = packages::cache_dir().join("bin");
    let link = dir.join(name);
    if std::fs::read_link(&link).ok().as_deref() == Some(me.as_path()) {
        return Some(link);
    }
    std::fs::create_dir_all(&dir).ok()?;
    let tmp = dir.join(format!(".{name}.{}", std::process::id()));
    #[cfg(unix)]
    std::os::unix::fs::symlink(&me, &tmp).ok()?;
    #[cfg(not(unix))]
    std::fs::copy(&me, &tmp).ok()?;
    std::fs::rename(&tmp, &link).ok()?;
    Some(link)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_linking() {
        let a = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(links(&a(&["hello.c", "-o", "hello"])));
        assert!(!links(&a(&["-c", "hello.c"])));
        assert!(!links(&a(&["--version"])));
        assert!(!links(&a(&["-print-search-dirs"])));
    }
}
