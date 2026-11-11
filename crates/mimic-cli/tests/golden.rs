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
    // complete static Linux executables, built only with the tools inside mimic
    for (t, machine) in [("aarch64-linux-musl", 183u16), ("x86_64-linux-musl", 62)] {
        let out = mimic().current_dir(&proj).args(["build", "--target", t]).output().unwrap();
        assert!(out.status.success(), "{t}: {}", String::from_utf8_lossy(&out.stderr));
        let elf = std::fs::read(proj.join(format!("target/release/{t}/demo"))).unwrap();
        assert_eq!(&elf[..4], b"\x7fELF", "{t}");
        assert_eq!(u16::from_le_bytes([elf[18], elf[19]]), machine, "{t}: e_machine");
        assert_eq!(u16::from_le_bytes([elf[16], elf[17]]), 2, "{t}: ET_EXEC (static)");
    }
    // a target without a bundled C library still gets its object file
    let out = mimic().current_dir(&proj).args(["build", "--target", "x86_64-windows"]).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let coff = std::fs::read(proj.join("target/release/x86_64-windows-gnu/demo.o")).unwrap();
    assert_eq!(&coff[..2], &[0x64, 0x86], "x86-64 COFF object");
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
    // everything comes from the linked LLVM
    let arches = v["arches"].as_array().unwrap();
    assert!(arches.len() > 50, "LLVM knows more than 50 architectures");
    let with_backend: Vec<&str> = arches.iter().filter(|a| !a["backend"].is_null()).map(|a| a["name"].as_str().unwrap()).collect();
    assert!(with_backend.len() >= 40, "{with_backend:?}");
    assert!(v["backends"].as_array().unwrap().iter().any(|b| b["name"] == "riscv64"));
    assert!(v["oses"].as_array().unwrap().iter().any(|o| o == "linux"));
    assert!(v["environments"].as_array().unwrap().iter().any(|e| e == "musleabihf"));
    assert!(v["executables"].as_array().unwrap().iter().any(|t| t == "x86_64-linux-musl"));
    let out = mimic().args(["targets", "--cpus", "x86_64"]).output().unwrap();
    assert!(String::from_utf8_lossy(&out.stdout).lines().any(|l| l == "znver4"));
    let out = mimic().args(["targets", "--check", "aarch64-linux", "--json"]).output().unwrap();
    let c: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(c["llvm"], "aarch64-unknown-linux-musl");
    let f = fixtures("run").into_iter().next().unwrap();
    let out = mimic().arg("build").arg(&f).args(["--target", "bogus-linux"]).output().unwrap();
    assert!(String::from_utf8_lossy(&out.stderr).contains("unknown architecture"));
}

/// Every CPU architecture with an LLVM backend in this build compiles Mimic
/// programs; GPU architectures explain that they need the GPU backend.
#[test]
fn every_backend_compiles() {
    let out = mimic().args(["targets", "--json"]).output().unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let f = fixtures("run").into_iter().find(|p| p.ends_with("basics.mim")).unwrap();
    let mut failures = Vec::new();
    // GPU backends take kernels, not whole programs (roadmap F8: GPU backend)
    const DEVICE: &[&str] = &["amdgcn", "amdgpu", "r600", "spirv", "spirv32", "spirv64", "dxil"];
    for a in v["arches"].as_array().unwrap().iter().filter(|a| !a["backend"].is_null()) {
        let name = a["name"].as_str().unwrap();
        let t = format!("{name}-unknown-unknown");
        if DEVICE.contains(&name) {
            let out = mimic().arg("build").arg(&f).args(["--target", &t]).output().unwrap();
            assert!(String::from_utf8_lossy(&out.stderr).contains("GPU target"), "{t}");
            continue;
        }
        let out = mimic().arg("build").arg(&f).args(["--target", &t]).output().unwrap();
        if !out.status.success() {
            failures.push(format!("{t}: {}", String::from_utf8_lossy(&out.stderr)));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// `mimic cc` is Clang with the target's C library filled in.
#[test]
fn mimic_cc() {
    let dir = std::env::temp_dir().join(format!("mimic-golden-cc-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let c = dir.join("hello.c");
    std::fs::write(&c, "#include <stdio.h>\n#include <math.h>\nint main(void){printf(\"%.3f\\n\", sqrt(2.0));return 0;}\n").unwrap();
    // native: compile, link with the embedded LLD, run
    let exe = dir.join("hello");
    let out = mimic().current_dir(&dir).args(["cc", "-O2", "hello.c", "-o"]).arg(&exe).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&std::process::Command::new(&exe).output().unwrap().stdout), "1.414\n");
    // cross: a static Linux executable with no flags beyond --target
    let out = mimic().current_dir(&dir).args(["cc", "--target=aarch64-linux", "hello.c", "-lm", "-o", "hello-linux"]).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let elf = std::fs::read(dir.join("hello-linux")).unwrap();
    assert_eq!(&elf[..4], b"\x7fELF");
    assert_eq!(u16::from_le_bytes([elf[18], elf[19]]), 183, "aarch64");
    assert_eq!(u16::from_le_bytes([elf[16], elf[17]]), 2, "static executable");
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Every run fixture, compiled to WebAssembly (WASI) and run in Node's WASI
/// implementation, prints exactly what the native build prints.
#[test]
fn wasi_fixtures() {
    let Ok(node) = std::process::Command::new("node").arg("--version").output() else {
        eprintln!("skipping: node is not installed");
        return;
    };
    assert!(node.status.success());
    let dir = std::env::temp_dir().join(format!("mimic-golden-wasi-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let runner = dir.join("run.mjs");
    std::fs::write(
        &runner,
        "import { WASI } from 'node:wasi';\nimport { readFileSync } from 'node:fs';\nconst f = process.argv[2];\n\
         const wasi = new WASI({ version: 'preview1', args: [f], env: {}, returnOnExit: true });\n\
         const m = await WebAssembly.compile(readFileSync(f));\n\
         process.exitCode = wasi.start(await WebAssembly.instantiate(m, wasi.getImportObject()));\n",
    )
    .unwrap();
    let mut failures = Vec::new();
    for f in fixtures("run") {
        let stem = f.file_stem().unwrap().to_string_lossy().to_string();
        let out = mimic().arg("build").arg(&f).args(["--target", "wasm32-wasi", "--emit-json"]).output().unwrap();
        let v: serde_json::Value = match serde_json::from_slice(&out.stdout) {
            Ok(v) => v,
            Err(_) => {
                failures.push(format!("{stem}: build failed\n{}", String::from_utf8_lossy(&out.stderr)));
                continue;
            }
        };
        let Some(wasm) = v["artifacts"][0]["path"].as_str() else {
            failures.push(format!("{stem}: {v}"));
            continue;
        };
        // A WebAssembly program cannot crash its host: a signal kills Node only
        // through an engine fault (seen ~1-3% of runs on deep recursion under
        // load). Those runs are retried; wrong output or a trap fails at once.
        let mut run = std::process::Command::new("node").arg("--no-warnings").arg(&runner).arg(wasm).output().unwrap();
        for _ in 0..2 {
            use std::os::unix::process::ExitStatusExt;
            let Some(sig) = run.status.signal() else { break };
            eprintln!("{stem}: node died from signal {sig} (engine fault); retrying");
            run = std::process::Command::new("node").arg("--no-warnings").arg(&runner).arg(wasm).output().unwrap();
        }
        let status = run.status;
        let want = std::fs::read_to_string(f.with_extension("out")).unwrap();
        let got = String::from_utf8_lossy(&run.stdout);
        if got != want {
            failures.push(format!(
                "{stem} ({status}, {wasm})\n--- want\n{want}--- got\n{got}--- stderr\n{}",
                String::from_utf8_lossy(&run.stderr)
            ));
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
    if !failures.is_empty() {
        let _ = std::fs::write(std::env::temp_dir().join("mimic-wasi-failures.log"), failures.join("\n"));
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Apple platforms with an installed SDK: every variant links, with the right
/// platform in the binary; Mac Catalyst builds run here and must match.
#[test]
fn apple_platforms() {
    if !cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        return;
    }
    let f = fixtures("run").into_iter().find(|p| p.ends_with("basics.mim")).unwrap();
    for t in ["aarch64-ios-simulator", "aarch64-ios", "aarch64-tvos", "arm64_32-watchos", "aarch64-xros-simulator", "aarch64-ios-macabi", "x86_64-macos"] {
        let probe = std::process::Command::new("xcrun").args(["--sdk", sdk_of(t), "--show-sdk-path"]).output();
        if !probe.is_ok_and(|o| o.status.success()) {
            eprintln!("skipping {t}: SDK not installed");
            continue;
        }
        let out = mimic().arg("build").arg(&f).args(["--target", t, "--emit-json"]).output().unwrap();
        let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap_or_else(|_| panic!("{t}: {}", String::from_utf8_lossy(&out.stderr)));
        assert_eq!(v["artifacts"][0]["kind"], "executable", "{t}: {v}");
        let exe = v["artifacts"][0]["path"].as_str().unwrap().to_string();
        let bytes = std::fs::read(&exe).unwrap();
        assert!(bytes[..4] == [0xcf, 0xfa, 0xed, 0xfe] || bytes[..4] == [0xce, 0xfa, 0xed, 0xfe], "{t}: Mach-O");
        // x86_64 macOS runs under Rosetta, when it is installed
        if t == "x86_64-macos" {
            if std::process::Command::new("arch").args(["-x86_64", "/usr/bin/true"]).status().is_ok_and(|s| s.success()) {
                let run = std::process::Command::new("arch").arg("-x86_64").arg(&exe).output().unwrap();
                assert_eq!(String::from_utf8_lossy(&run.stdout), std::fs::read_to_string(f.with_extension("out")).unwrap(), "{t}");
            }
            continue;
        }
        if t.ends_with("macabi") {
            let run = std::process::Command::new(&exe).output().unwrap();
            assert_eq!(String::from_utf8_lossy(&run.stdout), std::fs::read_to_string(f.with_extension("out")).unwrap(), "{t}");
        }
    }
}

fn sdk_of(t: &str) -> &'static str {
    match t {
        t if t.ends_with("macos") => "macosx",
        t if t.ends_with("macabi") => "macosx",
        t if t.contains("ios-simulator") => "iphonesimulator",
        t if t.contains("ios") => "iphoneos",
        t if t.contains("tvos") => "appletvos",
        t if t.contains("watchos") => "watchos",
        t if t.contains("xros-simulator") => "xrsimulator",
        _ => "xros",
    }
}

/// `--cpu` tunes for a model LLVM knows; unknown names get a suggestion.
#[test]
fn cpu_flag() {
    let f = fixtures("run").into_iter().find(|p| p.ends_with("basics.mim")).unwrap();
    let want = std::fs::read_to_string(f.with_extension("out")).unwrap();
    if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        let out = mimic().arg("run").arg(&f).args(["--release", "--cpu", "apple-m1"]).output().unwrap();
        assert_eq!(String::from_utf8_lossy(&out.stdout), want, "{}", String::from_utf8_lossy(&out.stderr));
    }
    let out = mimic().arg("build").arg(&f).args(["--target", "riscv64-linux", "--cpu", "sifive-u74"]).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let out = mimic().arg("build").arg(&f).args(["--target", "x86_64-linux", "--cpu", "znver44"]).output().unwrap();
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("unknown CPU `znver44`") && err.contains("did you mean `znver4`?"), "{err}");
}

/// A real C library with no binding code: SQLite's header is imported, its
/// handles are opaque `cptr` values, out-parameters are one-element arrays.
#[test]
fn c_import_sqlite() {
    if !cfg!(target_os = "macos") {
        return; // macOS ships SQLite (header in the SDK, library in the OS)
    }
    let dir = std::env::temp_dir().join(format!("mimic-golden-sqlite-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    assert!(mimic().current_dir(&dir).args(["create", "sq"]).status().unwrap().success());
    let proj = dir.join("sq");
    let manifest = std::fs::read_to_string(proj.join("mim.mim")).unwrap();
    std::fs::write(proj.join("mim.mim"), manifest.replace("link = []", "link = [\"sqlite3\"]")).unwrap();
    std::fs::write(
        proj.join("src/main.mim"),
        r#"module main

extern "C" include "sqlite3.h"

fn main() {
    let mut db_out = [0]
    print(unsafe { sqlite3_open(":memory:", db_out) })
    let db = db_out[0]
    let mut stmt_out = [0]
    let mut tail = [0]
    print(unsafe { sqlite3_prepare_v2(db, "select 'mimic', 40 + 2", ¯1, stmt_out, tail) })
    let stmt = stmt_out[0]
    print(unsafe { sqlite3_step(stmt) })
    print(unsafe { sqlite3_column_text(stmt, 0) })
    print(unsafe { sqlite3_column_int64(stmt, 1) })
    print(unsafe { sqlite3_finalize(stmt) } + unsafe { sqlite3_close(db) })
}
"#,
    )
    .unwrap();
    let out = mimic().current_dir(&proj).arg("run").output().unwrap();
    assert_eq!(String::from_utf8_lossy(&out.stdout), "0\n0\n100\nmimic\n42\n0\n", "{}", String::from_utf8_lossy(&out.stderr));
    std::fs::remove_dir_all(&dir).unwrap();
}

/// C sources listed in mim.mim are compiled for the target and linked in;
/// their header is imported directly.
#[test]
fn c_sources_in_project() {
    let dir = std::env::temp_dir().join(format!("mimic-golden-csrc-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    assert!(mimic().current_dir(&dir).args(["create", "cv"]).status().unwrap().success());
    let proj = dir.join("cv");
    std::fs::create_dir_all(proj.join("c")).unwrap();
    std::fs::write(proj.join("c/vec.h"), "double dot(const double *a, const double *b, int n);\nconst char *greeting(void);\n").unwrap();
    std::fs::write(
        proj.join("c/vec.c"),
        "#include \"vec.h\"\ndouble dot(const double *a, const double *b, int n) { double s = 0; for (int i = 0; i < n; i++) s += a[i] * b[i]; return s; }\nconst char *greeting(void) { return \"hello from C\"; }\n",
    )
    .unwrap();
    let manifest = std::fs::read_to_string(proj.join("mim.mim")).unwrap();
    std::fs::write(proj.join("mim.mim"), manifest.replace("link = []", "sources = [\"c/vec.c\"]\n    include = [\"c\"]")).unwrap();
    std::fs::write(
        proj.join("src/main.mim"),
        "module main\n\ntrusted extern \"C\" include \"vec.h\"\n\nfn main() {\n    print(dot([1.0 2.0 3.0], [4.0 5.0 6.0], 3))\n    print(greeting())\n}\n",
    )
    .unwrap();
    let out = mimic().current_dir(&proj).arg("run").output().unwrap();
    assert_eq!(String::from_utf8_lossy(&out.stdout), "32.0\nhello from C\n", "{}", String::from_utf8_lossy(&out.stderr));
    // the same project, cross-compiled: the C file is built for the target too
    let out = mimic().current_dir(&proj).args(["build", "--target", "aarch64-linux"]).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    std::fs::remove_dir_all(&dir).unwrap();
}

/// C structs by value, both ways, nested and anonymous-typedef'd: the
/// importer turns them into records and generates wrappers Clang compiles.
#[test]
fn c_structs_by_value() {
    let dir = std::env::temp_dir().join(format!("mimic-golden-cstruct-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    assert!(mimic().current_dir(&dir).args(["create", "geo"]).status().unwrap().success());
    let proj = dir.join("geo");
    std::fs::create_dir_all(proj.join("c")).unwrap();
    std::fs::write(
        proj.join("c/geo.h"),
        "struct vec2 { double x, y; };\ntypedef struct { int w; int h; } size2;\ntypedef struct rect { struct vec2 origin; size2 size; unsigned char flags; } rect_t;\n\
         double vlen(struct vec2 v);\nstruct vec2 vadd(struct vec2 a, struct vec2 b);\nrect_t mkrect(double x, double y, int w, int h);\nint area(rect_t r);\n",
    )
    .unwrap();
    std::fs::write(
        proj.join("c/geo.c"),
        "#include <math.h>\n#include \"geo.h\"\ndouble vlen(struct vec2 v) { return sqrt(v.x * v.x + v.y * v.y); }\n\
         struct vec2 vadd(struct vec2 a, struct vec2 b) { struct vec2 r = { a.x + b.x, a.y + b.y }; return r; }\n\
         rect_t mkrect(double x, double y, int w, int h) { rect_t r; r.origin.x = x; r.origin.y = y; r.size.w = w; r.size.h = h; r.flags = 7; return r; }\n\
         int area(rect_t r) { return r.size.w * r.size.h; }\n",
    )
    .unwrap();
    let manifest = std::fs::read_to_string(proj.join("mim.mim")).unwrap();
    std::fs::write(proj.join("mim.mim"), manifest.replace("link = []", "sources = [\"c/geo.c\"]\n    include = [\"c\"]")).unwrap();
    std::fs::write(
        proj.join("src/main.mim"),
        "module main\n\ntrusted extern \"C\" include \"geo.h\"\n\nfn main() {\n    print(vlen(Vec2 { x: 3.0, y: 4.0 }))\n    print(vadd(Vec2 { x: 3.0, y: 4.0 }, Vec2 { x: 1.0, y: 1.0 }))\n\
         \n    let r = mkrect(1.5, 2.5, 640, 480)\n    print(r)\n    print(area(r))\n}\n",
    )
    .unwrap();
    let want = "5.0\nVec2 { x: 4.0, y: 5.0 }\nRect { origin: Vec2 { x: 1.5, y: 2.5 }, size: Size2 { w: 640, h: 480 }, flags: 7 }\n307200\n";
    let out = mimic().current_dir(&proj).arg("run").output().unwrap();
    assert_eq!(String::from_utf8_lossy(&out.stdout), want, "{}", String::from_utf8_lossy(&out.stderr));
    for t in ["aarch64-linux", "x86-linux", "wasm32-wasi"] {
        let out = mimic().current_dir(&proj).args(["build", "--target", t]).output().unwrap();
        assert!(out.status.success(), "{t}: {}", String::from_utf8_lossy(&out.stderr));
    }
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Mimic called from C: `extern "C" fn` exports, `--emit-header`, a C program
/// linked against the Mimic library (the runtime starts itself).
#[test]
fn export_to_c() {
    let dir = std::env::temp_dir().join(format!("mimic-golden-export-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    assert!(mimic().current_dir(&dir).args(["create", "mm", "--lib"]).status().unwrap().success());
    let proj = dir.join("mm");
    std::fs::write(
        proj.join("src/lib.mim"),
        "module lib\n\ntrusted extern \"C\" include \"math.h\"\n\n\
         pub extern \"C\" fn mm_hypot(a: f64, b: f64) -> f64 { sqrt(a * a + b * b) }\n\
         pub extern \"C\" fn mm_sum_to(n: i64) -> i64 { ⍳ n ▷ +/ }\n\
         pub extern \"C\" fn mm_is_even(n: i64) -> bool { n % 2 == 0 }\n\
         pub extern \"C\" fn mm_greet(name: cstr) -> cstr { \"hello, {name}\" }\n",
    )
    .unwrap();
    let out = mimic().current_dir(&proj).args(["build", "--emit-header"]).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let h = std::fs::read_to_string(proj.join("target/release/mm.h")).unwrap();
    assert!(h.contains("double mm_hypot(double a, double b);") && h.contains("char * mm_greet(const char * name);"), "{h}");
    std::fs::write(
        proj.join("main.c"),
        "#include <stdio.h>\n#include <stdlib.h>\n#include \"mm.h\"\nint main(void) {\n  printf(\"%.1f %lld %d%d\\n\", mm_hypot(3, 4), (long long)mm_sum_to(1000), mm_is_even(10), mm_is_even(7));\n  char *g = mm_greet(\"C\"); printf(\"%s\\n\", g); free(g); return 0;\n}\n",
    )
    .unwrap();
    let out = mimic().current_dir(&proj).args(["cc", "main.c", "-Itarget/release", "target/release/libmm.a", "-o", "main_c"]).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let run = std::process::Command::new(proj.join("main_c")).output().unwrap();
    assert_eq!(String::from_utf8_lossy(&run.stdout), "5.0 499500 10\nhello, C\n");
    std::fs::remove_dir_all(&dir).unwrap();
}

/// C ABI conformance: struct shapes whose passing rules differ by platform
/// (1-byte, float/double HFAs, int+float, 64-byte, padded, nested, stack
/// spill) round-trip exactly on every stable target.
#[test]
fn abi_conformance() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/abi");
    let want = std::fs::read_to_string(src.join("expected.out")).unwrap();
    let dir = std::env::temp_dir().join(format!("mimic-golden-abi-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    assert!(mimic().current_dir(&dir).args(["create", "abi"]).status().unwrap().success());
    let proj = dir.join("abi");
    std::fs::create_dir_all(proj.join("c")).unwrap();
    for f in ["abi.h", "abi.c"] {
        std::fs::copy(src.join(f), proj.join("c").join(f)).unwrap();
    }
    std::fs::copy(src.join("main.mim"), proj.join("src/main.mim")).unwrap();
    let manifest = std::fs::read_to_string(proj.join("mim.mim")).unwrap();
    std::fs::write(proj.join("mim.mim"), manifest.replace("link = []", "sources = [\"c/abi.c\"]\n    include = [\"c\"]")).unwrap();

    let build = |t: Option<&str>| -> String {
        let mut c = mimic();
        c.current_dir(&proj).args(["build", "--emit-json"]);
        if let Some(t) = t {
            c.args(["--target", t]);
        }
        let out = c.output().unwrap();
        let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap_or_else(|_| panic!("{t:?}: {}", String::from_utf8_lossy(&out.stderr)));
        v["artifacts"][0]["path"].as_str().unwrap_or_else(|| panic!("{t:?}: {v}")).to_string()
    };
    let check = |name: &str, out: std::process::Output| {
        assert_eq!(String::from_utf8_lossy(&out.stdout), want, "{name}: {}", String::from_utf8_lossy(&out.stderr));
    };
    // native
    check("native", std::process::Command::new(build(None)).output().unwrap());
    if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        check("catalyst", std::process::Command::new(build(Some("aarch64-ios-macabi"))).output().unwrap());
        if std::process::Command::new("arch").args(["-x86_64", "/usr/bin/true"]).status().is_ok_and(|s| s.success()) {
            check("x86_64 (Rosetta)", std::process::Command::new("arch").arg("-x86_64").arg(build(Some("x86_64-macos"))).output().unwrap());
        }
    }
    // WebAssembly, when node is available
    if std::process::Command::new("node").arg("--version").output().is_ok() {
        let wasm = build(Some("wasm32-wasi"));
        let runner = dir.join("run.mjs");
        std::fs::write(
            &runner,
            "import { WASI } from 'node:wasi';\nimport { readFileSync } from 'node:fs';\nconst wasi = new WASI({ version: 'preview1', args: [process.argv[2]], env: {}, returnOnExit: true });\n\
             const m = await WebAssembly.compile(readFileSync(process.argv[2]));\nprocess.exitCode = wasi.start(await WebAssembly.instantiate(m, wasi.getImportObject()));\n",
        )
        .unwrap();
        check("wasm32-wasi", std::process::Command::new("node").arg("--no-warnings").arg(&runner).arg(wasm).output().unwrap());
    }
    // links for the other ABIs
    for t in ["aarch64-linux", "x86_64-linux", "riscv64-linux", "arm-linux", "x86-linux", "powerpc64le-linux"] {
        build(Some(t));
    }
    std::fs::remove_dir_all(&dir).unwrap();
}
