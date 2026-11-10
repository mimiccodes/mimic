//! Golden tests: every `tests/fixtures/run/*.mim` is compiled and run through the
//! real `mimic` binary and its stdout compared with the `.out` file next to it.
//! `tests/fixtures/fail/*.mim` must fail with the message in their first line.

use std::path::{Path, PathBuf};
use std::process::Command;

fn fixtures(kind: &str) -> Vec<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures").join(kind);
    let mut v: Vec<_> = std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().path()).filter(|p| p.extension().is_some_and(|e| e == "mim")).collect();
    v.sort();
    v
}

fn mimic() -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_mimic"));
    // keep the runtime cache and run directories out of the user's ~/.mimic
    c.env("MIMIC_HOME", std::env::temp_dir().join("mimic-golden-home"));
    c
}

#[test]
fn run_fixtures() {
    let mut failures = Vec::new();
    for f in fixtures("run") {
        // (profile, env): the third run forces every kernel through the thread
        // pool, one element per chunk, with the leak check on.
        let runs: [(&str, &[(&str, &str)]); 4] = [
            ("--debug", &[]),
            ("--release", &[]),
            ("--debug", &[("MIMIC_THREADS", "8"), ("MIMIC_PAR_GRAIN", "1")]),
            ("--release", &[("MIMIC_THREADS", "1")]),
        ];
        for (profile, env) in runs {
            let out = mimic().arg("run").arg(&f).arg(profile).envs(env.iter().copied()).output().unwrap();
            let want = std::fs::read_to_string(f.with_extension("out")).unwrap();
            let got = String::from_utf8_lossy(&out.stdout);
            let stderr = String::from_utf8_lossy(&out.stderr);
            if !out.status.success() || got != want || stderr.contains("leak") {
                failures.push(format!("{} {profile} {env:?}\n--- want\n{want}--- got\n{got}--- stderr\n{stderr}", f.display()));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn fail_fixtures() {
    let mut failures = Vec::new();
    for f in fixtures("fail") {
        let src = std::fs::read_to_string(&f).unwrap();
        let first = src.lines().next().unwrap();
        let (cmd, want) = if let Some(w) = first.strip_prefix("// expect-error: ") {
            ("check", w)
        } else if let Some(w) = first.strip_prefix("// expect-panic: ") {
            ("run", w)
        } else {
            panic!("{}: missing `// expect-error:` or `// expect-panic:` header", f.display());
        };
        let out = mimic().arg(cmd).arg(&f).output().unwrap();
        let stderr = String::from_utf8_lossy(&out.stderr);
        if out.status.success() || !stderr.contains(want) {
            failures.push(format!("{}: expected failure containing {want:?}, got status {:?}\n{stderr}", f.display(), out.status.code()));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn create_build_cross() {
    let dir = std::env::temp_dir().join(format!("mimic-golden-create-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let ok = mimic().current_dir(&dir).args(["create", "demo", "--bin", "--lib"]).status().unwrap();
    assert!(ok.success());
    let proj = dir.join("demo");
    for f in ["mim.mim", "build.mim", "mod.mim", "src/main.mim", "src/lib.mim"] {
        assert!(proj.join(f).is_file(), "missing {f}");
    }
    let out = mimic().current_dir(&proj).arg("run").output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "[0 1 2 3 4 5 6 7 8 9]\n45\n[9 8 7]\n285\nhello from demo\n");
    let out = mimic().current_dir(&proj).args(["build", "--release", "--target", "x86_64-linux-gnu"]).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let elf = std::fs::read(proj.join("target/release/x86_64-linux-gnu/demo")).unwrap();
    assert_eq!(&elf[..4], b"\x7fELF");
    std::fs::remove_dir_all(&dir).unwrap();
}
