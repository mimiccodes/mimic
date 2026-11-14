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
    let llvm = output(&format!("{root}/.llvm/bin/llvm-config"), &["--version"]).unwrap_or_else(|| "?".into());
    println!("cargo:rustc-env=MIMIC_COMMIT={commit}");
    println!("cargo:rustc-env=MIMIC_LLVM_VERSION={llvm}");
}
