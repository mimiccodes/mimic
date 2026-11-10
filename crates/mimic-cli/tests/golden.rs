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
        // The third run forces every kernel through the thread pool, one element
        // per chunk, with the leak check on. The last runs everything natively:
        // nets and native code must agree on every program.
        type Run<'a> = (&'a [&'a str], &'a [(&'a str, &'a str)]);
        let runs: [Run; 5] = [
            (&["--debug"], &[]),
            (&["--release"], &[]),
            (&["--debug"], &[("MIMIC_THREADS", "8"), ("MIMIC_PAR_GRAIN", "1")]),
            (&["--release"], &[("MIMIC_THREADS", "1")]),
            (&["--debug", "--no-net"], &[]),
        ];
        for (profile, env) in runs {
            let out = mimic().arg("run").arg(&f).args(profile).envs(env.iter().copied()).output().unwrap();
            let want = std::fs::read_to_string(f.with_extension("out")).unwrap();
            let got = String::from_utf8_lossy(&out.stdout);
            let stderr = String::from_utf8_lossy(&out.stderr);
            if !out.status.success() || got != want || stderr.contains("leak") {
                failures.push(format!("{} {profile:?} {env:?}\n--- want\n{want}--- got\n{got}--- stderr\n{stderr}", f.display()));
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
        if let Some(w) = first.strip_prefix("// expect-panic: ") {
            let out = mimic().arg("run").arg(&f).output().unwrap();
            let stderr = String::from_utf8_lossy(&out.stderr);
            if out.status.success() || !stderr.contains(w) {
                failures.push(format!("{}: expected a panic containing {w:?}, got status {:?}\n{stderr}", f.display(), out.status.code()));
            }
            continue;
        }
        let w = first.strip_prefix("// expect-error: ").unwrap_or_else(|| panic!("{}: missing `// expect-error:` or `// expect-panic:` header", f.display()));
        let (code, msg) = w.split_once(' ').unwrap();
        assert!(code.starts_with('E'), "{}: the expect-error header starts with the error code", f.display());
        // human output: `error[E0123]: message`
        let out = mimic().arg("check").arg(&f).output().unwrap();
        let stderr = String::from_utf8_lossy(&out.stderr);
        if out.status.success() || !stderr.contains(&format!("error[{code}]")) || !stderr.contains(msg) {
            failures.push(format!("{}: expected error[{code}] containing {msg:?}, got status {:?}\n{stderr}", f.display(), out.status.code()));
        }
        // JSON output: the same diagnostic, with a location inside the file
        let out = mimic().args(["check", "--emit-json"]).arg(&f).output().unwrap();
        let v: serde_json::Value = match serde_json::from_slice(&out.stdout) {
            Ok(v) => v,
            Err(e) => {
                failures.push(format!("{}: --emit-json is not JSON ({e}): {}", f.display(), String::from_utf8_lossy(&out.stdout)));
                continue;
            }
        };
        let found = v["diagnostics"].as_array().unwrap().iter().any(|d| {
            d["code"] == code && d["message"].as_str().unwrap().contains(msg) && d["location"]["start"]["line"].as_u64().unwrap_or(0) > 0
        });
        if out.status.success() || v["success"] != false || v["schema"] != 1 || !found {
            failures.push(format!("{}: JSON lacks {code} {msg:?}: {v}", f.display()));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// `mimic check --fix` applies machine-applicable fixes and leaves the rest.
#[test]
fn check_fix() {
    let dir = std::env::temp_dir().join(format!("mimic-golden-fix-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let f = dir.join("fixme.mim");
    std::fs::write(
        &f,
        "module main\n\nmodule util {\n    fn double(x) { x * 2 }\n}\n\nextern \"C\" { fn puts(s: cstr) -> i32 }\n\nfn main() {\n    let n = 1\n    n = util::double(n)\n    puts(\"hi\")\n    print(n)\n}\n",
    )
    .unwrap();
    let out = mimic().args(["check", "--fix", "--emit-json"]).arg(&f).output().unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["success"], true, "{v}");
    assert_eq!(v["fixed"], 3, "{v}");
    let fixed = std::fs::read_to_string(&f).unwrap();
    assert!(fixed.contains("    pub fn double(x)"), "{fixed}");
    assert!(fixed.contains("    let mut n = 1"), "{fixed}");
    assert!(fixed.contains("    unsafe { puts(\"hi\") }"), "{fixed}");
    let out = mimic().arg("run").arg(&f).output().unwrap();
    assert_eq!(String::from_utf8_lossy(&out.stdout), "hi\n2\n");
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Fix suggestions carry exact edits in the JSON output.
#[test]
fn json_fix_suggestions() {
    let f = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/fail/unresolved_suggest.mim");
    let out = mimic().args(["check", "--emit-json"]).arg(&f).output().unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let d = &v["diagnostics"][0];
    assert_eq!(d["name"], "unresolved-name");
    assert_eq!(d["location"]["text"], "totl");
    let fx = &d["fixes"][0];
    assert_eq!(fx["applicability"], "maybe-incorrect");
    assert_eq!(fx["edits"][0]["replacement"], "total");
    assert_eq!(fx["edits"][0]["location"]["text"], "totl");
    let f = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/fail/arg_type.mim");
    let out = mimic().args(["check", "--emit-json"]).arg(&f).output().unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let d = &v["diagnostics"][0];
    assert_eq!(d["location"]["text"], "\"ten\"", "errors point at the argument");
    assert_eq!(d["labels"][0]["location"]["text"], "x", "and label the parameter");
}

#[test]
fn explain_codes() {
    let out = mimic().args(["explain", "E0401"]).output().unwrap();
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).starts_with("E0401 (type-mismatch)"));
    let out = mimic().args(["explain", "--emit-json"]).output().unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(v.as_array().unwrap().len() >= 50);
    let out = mimic().args(["explain", "E9999"]).output().unwrap();
    assert!(!out.status.success());
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
    // `mimic build` with no flags produces the fastest (release) binary
    let out = mimic().current_dir(&proj).arg("build").output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(proj.join("target/release/demo").is_file(), "build must default to the release profile");
    let out = mimic().current_dir(&proj).args(["build", "--release", "--target", "x86_64-linux-gnu"]).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let elf = std::fs::read(proj.join("target/release/x86_64-linux-gnu/demo")).unwrap();
    assert_eq!(&elf[..4], b"\x7fELF");
    // musl targets need nothing but mimic: no Zig on PATH
    let no_zig: String = std::env::split_paths(&std::env::var_os("PATH").unwrap())
        .filter(|d| !d.join("zig").is_file())
        .map(|d| d.display().to_string())
        .collect::<Vec<_>>()
        .join(":");
    for (t, machine) in [("aarch64-linux-musl", 183u16), ("x86_64-linux-musl", 62)] {
        let out = mimic().current_dir(&proj).env("PATH", &no_zig).env_remove("MIMIC_ZIG").args(["build", "--target", t]).output().unwrap();
        assert!(out.status.success(), "{t}: {}", String::from_utf8_lossy(&out.stderr));
        let elf = std::fs::read(proj.join(format!("target/release/{t}/demo"))).unwrap();
        assert_eq!(&elf[..4], b"\x7fELF", "{t}");
        assert_eq!(u16::from_le_bytes([elf[18], elf[19]]), machine, "{t}: e_machine");
        assert_eq!(u16::from_le_bytes([elf[16], elf[17]]), 2, "{t}: ET_EXEC (static)");
    }
    std::fs::remove_dir_all(&dir).unwrap();
}

fn nets() -> Vec<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/nets");
    let mut v: Vec<_> = std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().path()).filter(|p| p.extension().is_some_and(|e| e == "mnet")).collect();
    v.sort();
    v
}

/// Every `tests/nets/*.mnet` reduces to the value in its `.out` file:
/// sequentially, in parallel (with the node/array leak check on), and optimised.
#[test]
fn net_fixtures() {
    let mut failures = Vec::new();
    for f in nets() {
        let want = std::fs::read_to_string(f.with_extension("out")).unwrap();
        for args in [&["--debug", "--threads", "1"][..], &["--debug", "--threads", "8"], &["--release"]] {
            let out = mimic().args(["net", "run"]).arg(&f).args(args).output().unwrap();
            let got = String::from_utf8_lossy(&out.stdout);
            if !out.status.success() || got != want {
                failures.push(format!("{} {args:?}: want {want:?} got {got:?}\n{}", f.display(), String::from_utf8_lossy(&out.stderr)));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Hammer the lock-free reducer: many parallel runs must all agree and leak nothing.
#[test]
fn net_parallel_stress() {
    let dir = std::env::temp_dir().join(format!("mimic-net-stress-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let src = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/nets/fib.mnet")).unwrap();
    let f = dir.join("fib24.mnet");
    std::fs::write(&f, src.replace("#20 r", "#24 r")).unwrap();
    for threads in ["2", "5", "12"] {
        for _ in 0..8 {
            let out = mimic().args(["net", "run", "--debug", "--threads", threads]).arg(&f).output().unwrap();
            assert!(out.status.success(), "threads={threads}: {}", String::from_utf8_lossy(&out.stderr));
            assert_eq!(String::from_utf8_lossy(&out.stdout), "46368\n", "threads={threads}");
        }
    }
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn targets_listing() {
    let out = mimic().args(["targets", "--json"]).output().unwrap();
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["arches"].as_array().unwrap().len(), 61);
    assert_eq!(v["oses"].as_array().unwrap().len(), 48);
    assert_eq!(v["abis"].as_array().unwrap().len(), 30);
    let libc = v["libc"].as_array().unwrap();
    assert_eq!(libc.len(), 108);
    assert!(libc.iter().any(|t| t["target"] == "x86_64-linux-musl" && t["status"] == "embedded"));
    let out = mimic().args(["targets", "--cpus", "x86_64"]).output().unwrap();
    assert!(String::from_utf8_lossy(&out.stdout).lines().any(|l| l == "znver4"));
    let f = fixtures("run").into_iter().next().unwrap();
    let out = mimic().arg("build").arg(&f).args(["--target", "m68k-linux-musl"]).output().unwrap();
    assert!(String::from_utf8_lossy(&out.stderr).contains("no code generator"));
}
