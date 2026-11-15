//! What `mimic --version` prints besides the version: the commit it was
//! built from and the LLVM it links.

use std::process::Command;

fn output(cmd: &str, args: &[&str]) -> Option<String> {
    let o = Command::new(cmd).args(args).output().ok()?;
    o.status.success().then(|| String::from_utf8_lossy(&o.stdout).trim().to_string()).filter(|s| !s.is_empty())
}

fn main() {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
    println!("cargo:rerun-if-changed={root}/.git/HEAD");
    println!("cargo:rerun-if-changed={root}/.git/refs/heads");
    println!("cargo:rerun-if-changed={root}/.git/packed-refs");
    let commit = output("git", &["-C", root, "rev-parse", "--short=9", "HEAD"]).unwrap_or_else(|| "unknown commit".into());
    // the LLVM inkwell links: LLVM_SYS_231_PREFIX when set (Windows CI), else `.llvm`
    println!("cargo:rerun-if-env-changed=LLVM_SYS_231_PREFIX");
    let prefix = std::env::var("LLVM_SYS_231_PREFIX").unwrap_or_else(|_| format!("{root}/.llvm"));
    let config = std::path::Path::new(&prefix).join("bin").join(format!("llvm-config{}", std::env::consts::EXE_SUFFIX));
    let llvm = output(&config.to_string_lossy(), &["--version"]).unwrap_or_else(|| "?".into());
    println!("cargo:rustc-env=MIMIC_COMMIT={commit}");
    println!("cargo:rustc-env=MIMIC_LLVM_VERSION={llvm}");
    manual(root);
}

/// The manual inside `mimic` (`mimic man`): every Markdown page under docs/
/// (the reference), docs/tutorial/ and docs/manual/, embedded in the binary.
fn manual(root: &str) {
    let docs = std::path::Path::new(root).join("docs");
    let mut pages = Vec::new();
    for sub in ["", "tutorial", "manual"] {
        let dir = docs.join(sub);
        println!("cargo:rerun-if-changed={}", dir.display());
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        let mut files: Vec<_> = rd.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.extension().is_some_and(|e| e == "md")).collect();
        files.sort();
        for f in files {
            println!("cargo:rerun-if-changed={}", f.display());
            let rel = if sub.is_empty() { f.file_name().unwrap().to_string_lossy().to_string() } else { format!("{sub}/{}", f.file_name().unwrap().to_string_lossy()) };
            let abs = f.canonicalize().unwrap_or(f);
            pages.push(format!("    ({rel:?}, include_str!({:?})),\n", abs.to_string_lossy()));
        }
    }
    let out = std::path::Path::new(&std::env::var("OUT_DIR").unwrap()).join("manual.rs");
    let code = format!("/// (path under docs/, text)\npub const PAGES: &[(&str, &str)] = &[\n{}];\n", pages.concat());
    std::fs::write(out, code).unwrap();
}
