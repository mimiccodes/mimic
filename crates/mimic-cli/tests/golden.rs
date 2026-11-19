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

/// A file's `file:` URI, as editors send it (no `\\?\` prefix on Windows).
fn file_uri(p: &Path) -> String {
    let s = p.canonicalize().unwrap().display().to_string();
    let s = s.strip_prefix(r"\\?\").unwrap_or(&s).replace('\\', "/");
    if s.starts_with('/') { format!("file://{s}") } else { format!("file:///{s}") }
}

/// Stop a server that stops answering: after `secs` seconds it is killed,
/// so a read waiting for it fails instead of hanging the test run.
fn watchdog(child: std::process::Child, secs: u64) -> (std::sync::Arc<std::sync::Mutex<std::process::Child>>, std::sync::Arc<std::sync::atomic::AtomicBool>) {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};
    let child = Arc::new(Mutex::new(child));
    let done = Arc::new(AtomicBool::new(false));
    let (c, d) = (child.clone(), done.clone());
    std::thread::spawn(move || {
        for _ in 0..secs * 4 {
            std::thread::sleep(std::time::Duration::from_millis(250));
            if d.load(Ordering::SeqCst) {
                return;
            }
        }
        eprintln!("the server did not answer for {secs} s: stopping it");
        let _ = c.lock().unwrap().kill();
    });
    (child, done)
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
        // (profile, env): the third run forces every kernel through the thread pool, one element
        // per chunk, with the leak check on. The last runs everything natively:
        // nets and native code must agree on every program.
        type Run<'a> = (&'a [&'a str], &'a [(&'a str, &'a str)]);
        let runs: [Run; 5] = [
            (&["--debug"], &[]),
            (&["--release"], &[]),
            (&["--debug"], &[("MIMIC_THREADS", "8"), ("MIMIC_PAR_GRAIN", "1")]),
            // (and every map the GPU can do on the GPU, where there is one)
            (&["--release"], &[("MIMIC_THREADS", "1"), ("MIMIC_GPU", "1")]),
            (&["--debug", "--no-net"], &[]),
        ];
        for (k, (profile, env)) in runs.into_iter().enumerate() {
            // each run gets its own temporary directory: file tests cannot collide
            let tmp = std::env::temp_dir().join(format!("mimic-golden-{}-{k}-{}", std::process::id(), f.file_stem().unwrap().to_string_lossy()));
            std::fs::create_dir_all(&tmp).unwrap();
            let out = mimic().arg("run").arg(&f).args(profile).envs(env.iter().copied()).env("TMPDIR", &tmp).output().unwrap();
            let _ = std::fs::remove_dir_all(&tmp);
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
        "module main\n\nmodule util {\n    fn double(x) { x * 2 }\n}\n\nextern \"C\" { fn puts(s: cstr) -> i32 }\n\nfn main() {\n    let n = 1\n    n = util::double(n)\n    c::puts(\"hi\")\n    print(n)\n}\n",
    )
    .unwrap();
    let out = mimic().args(["check", "--fix", "--emit-json"]).arg(&f).output().unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["success"], true, "{v}");
    assert_eq!(v["fixed"], 3, "{v}");
    let fixed = std::fs::read_to_string(&f).unwrap();
    assert!(fixed.contains("    pub fn double(x)"), "{fixed}");
    assert!(fixed.contains("    let mut n = 1"), "{fixed}");
    assert!(fixed.contains("    unsafe { c::puts(\"hi\") }"), "{fixed}");
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

/// The Workbench engine (`mimic query`): names, references, effects, C
/// headers, and edits sent through `--serve` that override the disk.
#[test]
fn query_engine() {
    use std::io::{BufRead, BufReader, Write};
    let dir = std::env::temp_dir().join(format!("mimic-golden-query-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let f = dir.join("q.mim");
    let src = "module main\n\ntrusted extern \"C\" pure include \"math.h\"\n\n// twice the value\nfn double(x: f64) -> f64 { x × 2.0 }\n\nfn main() {\n    let y = double(3.0)\n    print(y + c::sqrt(y))\n    print(double(y))\n}\n";
    std::fs::write(&f, src).unwrap();
    let q = |args: &[&str]| -> serde_json::Value {
        let out = mimic().arg("query").arg("--in").arg(&f).args(args).output().unwrap();
        serde_json::from_slice(&out.stdout).unwrap_or_else(|e| panic!("{e}: {}", String::from_utf8_lossy(&out.stdout)))
    };
    let file = format!("file={}", f.display());
    let v = q(&["check"]);
    assert_eq!(v["success"], true, "{v}");
    // `double` on line 9, column 13: its definition, with its comment
    let v = q(&["definition", &file, "line=9", "column=13"]);
    assert_eq!(v["path"], "double", "{v}");
    assert_eq!(v["doc"], "twice the value");
    assert_eq!(v["signature"], "fn double(x: f64) -> f64");
    assert_eq!(v["location"]["line"], 6);
    assert_eq!(v["location"]["column"], 4);
    let v = q(&["references", "name=double"]);
    assert_eq!(v.as_array().unwrap().len(), 2, "{v}");
    let v = q(&["hover", &file, "line=9", "column=13"]);
    assert_eq!(v["effects"], "pure", "{v}");
    // a local: `y` used on line 10 is the `let y` of line 9
    let v = q(&["hover", &file, "line=10", "column=11"]);
    assert_eq!(v["kind"], "local", "{v}");
    assert_eq!(v["location"]["line"], 9);
    assert_eq!(v["location"]["column"], 9);
    // a C function knows its header
    let v = q(&["hover", &file, "line=10", "column=20"]);
    assert_eq!(v["header"], "math.h", "{v}");
    let v = q(&["symbols", "query=dou", "package=q"]);
    assert_eq!(v["total"], 1, "{v}");
    let v = q(&["frobnicate"]);
    assert!(v["error"].as_str().unwrap().contains("unknown method"), "{v}");

    // the editor's text wins over the disk; the last good index stays
    let mut child = mimic()
        .args(["query", "--serve", "--in"])
        .arg(&f)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let mut output = BufReader::new(child.stdout.take().unwrap());
    let mut ask = |req: serde_json::Value| -> serde_json::Value {
        writeln!(input, "{req}").unwrap();
        let mut line = String::new();
        output.read_line(&mut line).unwrap();
        serde_json::from_str(&line).unwrap()
    };
    let path = f.display().to_string();
    let r = ask(serde_json::json!({ "id": 1, "method": "check" }));
    assert_eq!(r["id"], 1);
    assert_eq!(r["result"]["success"], true, "{r}");
    let broken = src.replace("print(double(y))", "print(dubble(y))");
    let r = ask(serde_json::json!({ "id": 2, "method": "change", "params": { "file": path, "text": broken } }));
    assert_eq!(r["result"]["success"], false, "{r}");
    assert_eq!(r["result"]["stale_index"], true);
    assert_eq!(r["result"]["diagnostics"][0]["code"], "E0303", "{r}");
    let r = ask(serde_json::json!({ "id": 3, "method": "definition", "params": { "file": path, "line": 9, "column": 13 } }));
    assert_eq!(r["result"]["path"], "double", "lookups still work: {r}");
    assert_eq!(std::fs::read_to_string(&f).unwrap(), src, "the disk is never written");
    let r = ask(serde_json::json!({ "id": 4, "method": "close", "params": { "file": path } }));
    assert_eq!(r["result"]["success"], true, "{r}");
    writeln!(input, "{{\"method\": \"exit\"}}").unwrap();
    assert!(child.wait().unwrap().success());
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Libraries from git: pinned to a tag (`rev`), several versions installed
/// side by side, and `mimic install` in a project restoring the commit
/// mod.mim locked when mim.mim pins none.
#[test]
fn git_dependencies() {
    let dir = std::env::temp_dir().join(format!("mimic-golden-git-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let home = dir.join("home");
    let m = || {
        let mut c = mimic();
        c.env("MIMIC_HOME", &home);
        c
    };
    let git = |args: &[&str], cwd: &Path| {
        let ok = Command::new("git").args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"]).args(args).current_dir(cwd).output().unwrap();
        assert!(ok.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&ok.stderr));
    };
    // a library with three versions: v1.0.0 and v2.0.0 tagged, then one more commit
    let lib = dir.join("greet");
    assert!(m().args(["create", "greet", "--lib"]).current_dir(&dir).status().unwrap().success());
    git(&["init", "-q", "-b", "main"], &lib);
    for (word, tag) in [("hello", Some("v1.0.0")), ("howdy", Some("v2.0.0")), ("hey", None)] {
        std::fs::write(lib.join("src/lib.mim"), format!("module lib\n\npub fn hello() {{\n    print(\"{word} from greet\")\n}}\n")).unwrap();
        git(&["add", "-A"], &lib);
        git(&["commit", "-qm", word], &lib);
        if let Some(t) = tag {
            git(&["tag", t], &lib);
        }
    }
    let app = dir.join("app");
    assert!(m().args(["create", "app"]).current_dir(&dir).status().unwrap().success());
    std::fs::write(app.join("src/main.mim"), "module main\n\nfn main() { greet::hello() }\n").unwrap();
    let url = lib.display().to_string().replace('\\', "/");
    let manifest = std::fs::read_to_string(app.join("mim.mim")).unwrap();
    let with_dep = |rev: Option<&str>| {
        let dep = match rev {
            Some(r) => format!("dependencies {{\n    greet = {{ git = \"{url}\", rev = \"{r}\" }}\n}}"),
            None => format!("dependencies {{\n    greet = {{ git = \"{url}\" }}\n}}"),
        };
        let start = manifest.find("dependencies {").unwrap();
        let end = start + manifest[start..].find('}').unwrap() + 1;
        std::fs::write(app.join("mim.mim"), format!("{}{dep}{}", &manifest[..start], &manifest[end..])).unwrap();
    };
    let run = || {
        let out = m().arg("run").current_dir(&app).output().unwrap();
        format!("{}{}", String::from_utf8_lossy(&out.stdout), if out.status.success() { String::new() } else { String::from_utf8_lossy(&out.stderr).to_string() })
    };
    // pinned to v1.0.0: missing until `mimic install`
    with_dep(Some("v1.0.0"));
    assert!(run().contains("mimic install"), "the error says how to get it");
    assert!(m().arg("install").current_dir(&app).status().unwrap().success());
    assert_eq!(run(), "hello from greet\n");
    // v2.0.0 next to it
    with_dep(Some("v2.0.0"));
    assert!(m().arg("install").current_dir(&app).status().unwrap().success());
    assert_eq!(run(), "howdy from greet\n");
    assert!(home.join("libraries/greet@v1.0.0").is_dir() && home.join("libraries/greet@v2.0.0").is_dir());
    // no pin: `mimic install` takes the commit mod.mim locked (v2), not the branch's newest
    with_dep(None);
    assert!(m().arg("install").current_dir(&app).status().unwrap().success());
    assert_eq!(run(), "howdy from greet\n");
    // `mimic update` moves it to the newest commit of the branch
    assert!(m().args(["update", "greet"]).status().unwrap().success());
    assert_eq!(run(), "hey from greet\n");
    // a commit, by URL
    let out = m().args(["install", &format!("{url}@v1.0.0")]).output().unwrap();
    assert!(!out.status.success(), "already installed");
    std::fs::remove_dir_all(&dir).unwrap();
}

/// A workspace: commands at its root run in every member, members build
/// into the workspace's one `target/`, and a member uses another by path.
#[test]
fn workspace() {
    let dir = std::env::temp_dir().join(format!("mimic-golden-ws-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("mim.mim"), "module mim\n\nworkspace {\n    members = [\"app\", \"greet\"]\n}\n").unwrap();
    assert!(mimic().args(["create", "greet", "--lib"]).current_dir(&dir).status().unwrap().success());
    assert!(mimic().args(["create", "app"]).current_dir(&dir).status().unwrap().success());
    let manifest = std::fs::read_to_string(dir.join("app/mim.mim")).unwrap();
    let start = manifest.find("dependencies {").unwrap();
    let end = start + manifest[start..].find('}').unwrap() + 1;
    let manifest = format!("{}dependencies {{\n    greet = {{ path = \"../greet\" }}\n}}{}", &manifest[..start], &manifest[end..]);
    std::fs::write(dir.join("app/mim.mim"), manifest).unwrap();
    std::fs::write(dir.join("app/src/main.mim"), "module main\n\nfn main() { greet::hello() }\n\ntest \"adds\" { expect(1 + 1 == 2) }\n").unwrap();
    let out = mimic().arg("check").current_dir(&dir).output().unwrap();
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{err}");
    assert!(err.contains("Checked `app`") && err.contains("Checked `greet`") && err.contains("2 member(s)"), "{err}");
    let out = mimic().arg("test").current_dir(&dir).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    // `run` needs one member
    let out = mimic().arg("run").current_dir(&dir).output().unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("is a workspace: run this inside one of its members (app, greet)"));
    let out = mimic().arg("run").current_dir(dir.join("app")).output().unwrap();
    assert_eq!(String::from_utf8_lossy(&out.stdout), "hello from greet\n");
    assert!(dir.join("target/debug/app").exists() || dir.join("target/debug/app.exe").exists(), "the workspace's target/");
    assert!(!dir.join("app/target/debug").exists(), "nothing built in the member's own target/");
    // a broken member fails the whole command, and says which
    std::fs::write(dir.join("greet/src/lib.mim"), "module lib\n\npub fn hello() { print(nope) }\n").unwrap();
    let out = mimic().arg("check").current_dir(&dir).output().unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("failed in app, greet"), "{}", String::from_utf8_lossy(&out.stderr));
    std::fs::remove_dir_all(&dir).unwrap();
}

/// `mimic man`: a page for every standard function, glyph and error code;
/// lookups by name, number, glyph, digraph and word; search; examples run.
#[test]
fn manual() {
    let man = |args: &[&str]| {
        let out = mimic().arg("man").args(args).env("NO_COLOR", "1").output().unwrap();
        (out.status.success(), String::from_utf8_lossy(&out.stdout).to_string(), String::from_utf8_lossy(&out.stderr).to_string())
    };
    let (ok, text, _) = man(&["fs::read"]);
    assert!(ok && text.contains("fn read(path: str) -> Result[str, str]") && text.contains("concurrent"), "{text}");
    let (ok, keep, _) = man(&["⌿"]);
    assert!(ok && keep.contains("keep") && keep.contains("\\k"), "{keep}");
    assert_eq!(man(&["keep"]).1, keep, "the word finds the glyph's page");
    assert_eq!(man(&["\\k"]).1, keep, "and so does the digraph");
    let (ok, text, _) = man(&["E0303"]);
    assert!(ok && text.contains("unresolved-name"), "{text}");
    let (ok, text, _) = man(&["4"]);
    assert!(ok && text.starts_with("No loops"), "chapter 4: {text}");
    let (ok, text, _) = man(&["json"]);
    assert!(ok && text.contains("json::decode"), "the module, not the chapter: {text}");
    let (ok, text, _) = man(&["coming-from-python"]);
    assert!(ok && text.contains("Coming from Python"), "{text}");
    let (ok, _, err) = man(&["frobnicate"]);
    assert!(!ok && err.contains("--search"), "{err}");
    let (ok, text, _) = man(&["--search", "high minus"]);
    assert!(ok && text.contains("mimic man"), "{text}");
    // every function, glyph and error code has a page
    let out = mimic().args(["man", "--list", "--json"]).output().unwrap();
    let pages: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let names: Vec<&str> = pages.as_array().unwrap().iter().map(|p| p["name"].as_str().unwrap()).collect();
    let out = mimic().args(["explain", "--emit-json"]).output().unwrap();
    let codes: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    for c in codes.as_array().unwrap() {
        assert!(names.contains(&c["code"].as_str().unwrap()), "no page for {}", c["code"]);
    }
    for f in ["sort_by", "fs::write", "json::decode", "http::serve", "time::format", "math::sqrt"] {
        assert!(names.contains(&f), "no page for {f}");
    }
    // a page's examples run
    let (ok, text, _) = man(&["--run", "coming-from-c"]);
    assert!(ok && text.contains("0 failed"), "{text}");
    // an error points to its page
    let f = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/fail/arity.mim");
    let out = mimic().arg("check").arg(&f).output().unwrap();
    assert!(String::from_utf8_lossy(&out.stderr).contains("`mimic man E0705`"));
}

/// Examples in `//` comments (`// > expression`, then the value) run as tests.
#[test]
fn doc_tests() {
    let dir = std::env::temp_dir().join(format!("mimic-golden-doctest-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let f = dir.join("dt.mim");
    std::fs::write(&f, "module main\n\n// > larger(3, 7)\n// 7\n// > [larger(¯2, ¯5)]\n// [-2]\nfn larger(a: i64, b: i64) -> i64 { a ⌈ b }\n\n// > \"{1 + 1}\"\n// 2\nfn main() { print(larger(1, 2)) }\n").unwrap();
    let out = mimic().arg("test").arg(&f).output().unwrap();
    let err = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(out.status.success() && err.contains("3 passed"), "{err}");
    std::fs::write(&f, "module main\n\n// > larger(3, 7)\n// 3\nfn larger(a: i64, b: i64) -> i64 { a ⌈ b }\n\nfn main() { print(larger(1, 2)) }\n").unwrap();
    let out = mimic().arg("test").arg(&f).output().unwrap();
    let err = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(!out.status.success() && err.contains("example on line 3") && err.contains("\"7\""), "{err}");
    // and they are not part of the program
    let out = mimic().arg("run").arg(&f).output().unwrap();
    assert_eq!(String::from_utf8_lossy(&out.stdout), "2\n");
    std::fs::remove_dir_all(&dir).unwrap();
}

/// The grammars (editors/syntax) know the whole language: every glyph, its
/// word and digraph, and every keyword are highlighted as such, every test
/// program and example tokenizes with no unrecognised text, and the
/// tree-sitter grammar parses them with no error. Needs the
/// submodule and `npm ci` in it (CI does both); skipped otherwise, except in CI.
#[test]
fn grammar_knows_the_language() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let syntax = root.join("editors/syntax");
    if !syntax.join("node_modules/vscode-textmate").is_dir() {
        assert!(std::env::var_os("CI").is_none(), "CI must check the grammars: run `npm ci` in editors/syntax");
        eprintln!("skipped: no editors/syntax/node_modules (git submodule update --init; npm ci in editors/syntax)");
        return;
    }
    let mut words: Vec<String> = Vec::new();
    for p in mimic_syntax::glyphs::PRIMS {
        words.push(p.glyph.to_string());
        words.push(p.word.to_string());
        words.extend(p.digraph.map(String::from));
        words.extend(p.ascii.map(String::from));
    }
    for g in mimic_syntax::glyphs::SYNTAX_GLYPHS {
        words.push(g.glyph.to_string());
        words.extend(g.digraph.map(String::from));
    }
    for k in ["module", "fn", "let", "mut", "pub", "if", "else", "return", "use", "as", "extern", "unsafe", "trusted", "type", "match", "arms", "with", "true", "false", "and", "or", "not", "div", "|>", "->", "=>"] {
        words.push(k.to_string());
    }
    let dir = std::env::temp_dir().join(format!("mimic-golden-grammar-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let list = dir.join("words.mim");
    std::fs::write(&list, words.join("\n") + "\n").unwrap();
    let node = |args: &[&Path]| {
        let out = Command::new("node").arg("test/highlight.mjs").args(args).current_dir(&syntax).output().expect("node");
        (out.status.success(), String::from_utf8_lossy(&out.stdout).to_string())
    };
    let (ok, text) = node(&[Path::new("--words"), &list]);
    assert!(ok, "{text}");
    let (ok, text) = node(&[&root.join("tests/fixtures"), &root.join("tests/nets"), &root.join("examples"), &syntax.join("samples")]);
    assert!(ok, "{text}");
    std::fs::remove_dir_all(&dir).unwrap();
    // the tree-sitter grammar parses every test program and example (it
    // compiles its parser with the system's C compiler; not on Windows)
    if cfg!(windows) {
        return;
    }
    let ts = syntax.join("tree-sitter-mimic");
    if !ts.join("node_modules/tree-sitter-cli").is_dir() {
        assert!(std::env::var_os("CI").is_none(), "CI must check the tree-sitter grammar: run `npm ci` in editors/syntax/tree-sitter-mimic");
        return;
    }
    let out = Command::new("node")
        .arg("../test/tree-sitter-parse.mjs")
        .args([root.join("tests/fixtures/run"), root.join("tests/fixtures/window"), root.join("examples"), root.join("bench")])
        .current_dir(&ts)
        .output()
        .expect("node");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stdout));
}

/// `mimic lsp` speaks the Language Server Protocol: errors as you type with
/// the compiler's fixes, hover, go to definition (into the standard library),
/// glyph input, formatting that keeps glyphs, and names from C coloured.
#[test]
fn language_server() {
    use std::io::{BufRead, BufReader, Read, Write};
    let dir = std::env::temp_dir().join(format!("mimic-golden-lsp-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let f = dir.join("t.mim");
    let text = "module main\n\ntrusted extern \"C\" pure include \"math.h\"\n\n// twice\nfn double(x: f64) -> f64 { x × 2.0 }\n\nfn main() {\n    let y = double(3.0)\n    print(c::sqrt(y) ⌿ 1)\n    print(sort_by([3 1], v => v))\n}\n";
    std::fs::write(&f, text).unwrap();
    let uri = file_uri(&f);
    let mut child = mimic().arg("lsp").stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).spawn().unwrap();
    let mut input = child.stdin.take().unwrap();
    let mut output = BufReader::new(child.stdout.take().unwrap());
    let (child, done) = watchdog(child, 180);
    let mut send = |v: serde_json::Value| {
        let b = v.to_string();
        write!(input, "Content-Length: {}\r\n\r\n{b}", b.len()).unwrap();
        input.flush().unwrap();
    };
    let mut notes: Vec<serde_json::Value> = Vec::new();
    let mut read = |notes: &mut Vec<serde_json::Value>, id: Option<i64>| -> serde_json::Value {
        loop {
            let mut len = 0;
            loop {
                let mut line = String::new();
                output.read_line(&mut line).unwrap();
                if line.trim().is_empty() {
                    break;
                }
                if let Some(n) = line.to_lowercase().strip_prefix("content-length:") {
                    len = n.trim().parse().unwrap();
                }
            }
            let mut body = vec![0; len];
            output.read_exact(&mut body).unwrap();
            let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
            match id {
                Some(i) if v["id"] == i => return v["result"].clone(),
                None if v["method"] == "textDocument/publishDiagnostics" => return v,
                _ => notes.push(v),
            }
        }
    };
    let pos = |line: u32, ch: u32| serde_json::json!({ "textDocument": { "uri": uri }, "position": { "line": line, "character": ch } });
    send(serde_json::json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": { "processId": null, "capabilities": {} } }));
    let caps = read(&mut notes, Some(1));
    assert!(caps["capabilities"]["hoverProvider"] == true, "{caps}");
    send(serde_json::json!({ "jsonrpc": "2.0", "method": "initialized", "params": {} }));
    send(serde_json::json!({ "jsonrpc": "2.0", "method": "textDocument/didOpen", "params": { "textDocument": { "uri": uri, "languageId": "mimic", "version": 1, "text": text } } }));
    let d = read(&mut notes, None);
    assert_eq!(d["params"]["diagnostics"], serde_json::json!([]), "{d}");
    // hover: signature, effects, comment; a C function's header; a glyph
    send(serde_json::json!({ "jsonrpc": "2.0", "id": 2, "method": "textDocument/hover", "params": pos(8, 14) }));
    let h = read(&mut notes, Some(2))["contents"]["value"].as_str().unwrap().to_string();
    assert!(h.contains("fn double(x: f64) -> f64") && h.contains("pure") && h.contains("twice"), "{h}");
    send(serde_json::json!({ "jsonrpc": "2.0", "id": 3, "method": "textDocument/hover", "params": pos(9, 14) }));
    let h = read(&mut notes, Some(3))["contents"]["value"].as_str().unwrap().to_string();
    assert!(h.contains("math.h"), "{h}");
    send(serde_json::json!({ "jsonrpc": "2.0", "id": 4, "method": "textDocument/hover", "params": pos(9, 21) }));
    let h = read(&mut notes, Some(4))["contents"]["value"].as_str().unwrap().to_string();
    assert!(h.contains("keep") && h.contains("\\k"), "{h}");
    // definition: in the file, and in the standard library
    send(serde_json::json!({ "jsonrpc": "2.0", "id": 5, "method": "textDocument/definition", "params": pos(8, 14) }));
    let r = read(&mut notes, Some(5));
    assert_eq!(r["range"]["start"], serde_json::json!({ "line": 5, "character": 3 }), "{r}");
    send(serde_json::json!({ "jsonrpc": "2.0", "id": 6, "method": "textDocument/definition", "params": pos(10, 10) }));
    let r = read(&mut notes, Some(6));
    let std_file = r["uri"].as_str().unwrap().to_string();
    assert!(std_file.contains("/std/std-"), "{r}");
    // `c::` and `sqrt` get the foreign colour
    send(serde_json::json!({ "jsonrpc": "2.0", "id": 7, "method": "textDocument/semanticTokens/full", "params": { "textDocument": { "uri": uri } } }));
    let r = read(&mut notes, Some(7));
    assert_eq!(r["data"], serde_json::json!([9, 10, 1, 0, 1, 0, 3, 4, 1, 1]), "{r}");
    // an error as you type, and its fix
    let broken = text.replace("    let y = double(3.0)\n", "    let y = double(3.0)\n    y = 2.0\n");
    send(serde_json::json!({ "jsonrpc": "2.0", "method": "textDocument/didChange", "params": { "textDocument": { "uri": uri, "version": 2 }, "contentChanges": [{ "text": broken }] } }));
    let d = read(&mut notes, None);
    assert_eq!(d["params"]["diagnostics"][0]["code"], "E0601", "{d}");
    let range = d["params"]["diagnostics"][0]["range"].clone();
    send(serde_json::json!({ "jsonrpc": "2.0", "id": 8, "method": "textDocument/codeAction", "params": { "textDocument": { "uri": uri }, "range": range, "context": { "diagnostics": [] } } }));
    let r = read(&mut notes, Some(8));
    let edit = r[0]["edit"]["changes"][&uri][0].clone();
    assert_eq!(edit["newText"], "mut ", "{r}");
    assert_eq!(std::fs::read_to_string(&f).unwrap(), text, "the server never writes the file");
    // glyph input: `\i` becomes `⍳`
    let typing = format!("{broken}\\i");
    send(serde_json::json!({ "jsonrpc": "2.0", "method": "textDocument/didChange", "params": { "textDocument": { "uri": uri, "version": 3 }, "contentChanges": [{ "text": typing }] } }));
    let last = typing.lines().count() as u32 - 1;
    send(serde_json::json!({ "jsonrpc": "2.0", "id": 9, "method": "textDocument/completion", "params": pos(last, 2) }));
    let r = read(&mut notes, Some(9));
    let items = if r.is_array() { r.clone() } else { r["items"].clone() };
    assert_eq!(items[0]["textEdit"]["newText"], "⍳", "{r}");
    // formatting keeps glyphs in a file written with glyphs
    let messy = "module main\n\nfn main() {\n        let xs = ⍳ 5\n  print(xs ▷ +/)\n}\n";
    send(serde_json::json!({ "jsonrpc": "2.0", "method": "textDocument/didChange", "params": { "textDocument": { "uri": uri, "version": 4 }, "contentChanges": [{ "text": messy }] } }));
    send(serde_json::json!({ "jsonrpc": "2.0", "id": 10, "method": "textDocument/formatting", "params": { "textDocument": { "uri": uri }, "options": { "tabSize": 4, "insertSpaces": true } } }));
    let r = read(&mut notes, Some(10));
    assert_eq!(r[0]["newText"], "module main\n\nfn main() {\n    let xs = ⍳ 5\n    ⎕(xs ▷ +/)\n}\n", "{r}");
    send(serde_json::json!({ "jsonrpc": "2.0", "id": 11, "method": "shutdown", "params": null }));
    read(&mut notes, Some(11));
    send(serde_json::json!({ "jsonrpc": "2.0", "method": "exit", "params": null }));
    done.store(true, std::sync::atomic::Ordering::SeqCst);
    assert!(child.lock().unwrap().wait().unwrap().success());
    std::fs::remove_dir_all(&dir).unwrap();
}

/// `mimic dap`: an editor's debug session (through LLVM's lldb-dap) stops
/// at a breakpoint on a `.mim` line and shows every variable as Mimic prints
/// it: lists, text, records, maps and options. Skipped where lldb-dap is not
/// installed (macOS has it with Xcode).
#[test]
fn debugger() {
    use std::io::{BufRead, BufReader, Read, Write};
    let have = Command::new("xcrun").args(["-f", "lldb-dap"]).output().is_ok_and(|o| o.status.success())
        || std::env::var_os("MIMIC_LLDB_DAP").is_some();
    if !have {
        eprintln!("skipped: no lldb-dap");
        return;
    }
    let dir = std::env::temp_dir().join(format!("mimic-golden-dap-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let f = dir.join("dbg.mim");
    std::fs::write(&f, "module main\n\ntype Point = { x: f64, y: f64 }\n\nfn show(xs: [i64], name: str, p: Point, m: [str: i64], o: Option[i64]) -> i64 {\n    let total = +/ xs\n    print(\"{name} {total} {p.x} {≢ m} {o}\")\n    total\n}\n\nfn main() {\n    let xs = [3 1 4 1 5]\n    let p = Point { x: 1.5, y: 2.0 }\n    let m = [\"a\": 1, \"b\": 2]\n    print(show(xs, \"hi\", p, m, Some(7)))\n}\n").unwrap();
    let mut child = mimic().arg("dap").stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).spawn().unwrap();
    let mut input = child.stdin.take().unwrap();
    let mut output = BufReader::new(child.stdout.take().unwrap());
    let mut seq = 0;
    let mut send = |cmd: &str, args: serde_json::Value| -> i64 {
        seq += 1;
        let b = serde_json::json!({ "seq": seq, "type": "request", "command": cmd, "arguments": args }).to_string();
        write!(input, "Content-Length: {}\r\n\r\n{b}", b.len()).unwrap();
        input.flush().unwrap();
        seq
    };
    let mut wait = |pred: &dyn Fn(&serde_json::Value) -> bool| -> serde_json::Value {
        loop {
            let mut len = 0;
            loop {
                let mut line = String::new();
                assert!(output.read_line(&mut line).unwrap() > 0, "the adapter stopped");
                if line.trim().is_empty() {
                    break;
                }
                if let Some(n) = line.to_lowercase().strip_prefix("content-length:") {
                    len = n.trim().parse().unwrap();
                }
            }
            let mut body = vec![0; len];
            output.read_exact(&mut body).unwrap();
            let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
            if pred(&v) {
                return v;
            }
        }
    };
    let reply = |s: i64| move |v: &serde_json::Value| v["type"] == "response" && v["request_seq"] == s;
    let s = send("initialize", serde_json::json!({ "adapterID": "mimic", "linesStartAt1": true, "columnsStartAt1": true, "pathFormat": "path" }));
    assert_eq!(wait(&reply(s))["success"], true);
    send("launch", serde_json::json!({ "program": f.display().to_string() }));
    wait(&|v| v["event"] == "initialized");
    let s = send("setBreakpoints", serde_json::json!({ "source": { "path": f.display().to_string() }, "breakpoints": [{ "line": 7 }] }));
    assert_eq!(wait(&reply(s))["body"]["breakpoints"][0]["verified"], true);
    let s = send("configurationDone", serde_json::json!({}));
    wait(&reply(s));
    let stop = wait(&|v| v["event"] == "stopped");
    let s = send("stackTrace", serde_json::json!({ "threadId": stop["body"]["threadId"] }));
    let frames = wait(&reply(s))["body"]["stackFrames"].clone();
    assert_eq!(frames[0]["name"], "dbg::main::show", "{frames}");
    assert_eq!(frames[0]["line"], 7);
    let s = send("scopes", serde_json::json!({ "frameId": frames[0]["id"] }));
    let scopes = wait(&reply(s))["body"]["scopes"].clone();
    let s = send("variables", serde_json::json!({ "variablesReference": scopes[0]["variablesReference"] }));
    let vars = wait(&reply(s))["body"]["variables"].clone();
    let get = |n: &str| vars.as_array().unwrap().iter().find(|v| v["name"] == n).map(|v| v["value"].as_str().unwrap().to_string()).unwrap_or_default();
    assert_eq!(get("xs"), "[3 1 4 1 5]", "{vars}");
    assert_eq!(get("name"), "\"hi\"", "{vars}");
    assert_eq!(get("p"), "Point { x: 1.5, y: 2.0 }", "{vars}");
    assert_eq!(get("m"), "[\"a\": 1, \"b\": 2]", "{vars}");
    assert_eq!(get("o"), "Some(7)", "{vars}");
    assert_eq!(get("total"), "14", "{vars}");
    let s = send("disconnect", serde_json::json!({ "terminateDebuggee": true }));
    wait(&reply(s));
    assert!(child.wait().unwrap().success());
    std::fs::remove_dir_all(&dir).unwrap();
}

/// `mimic mcp`: an agent checks, fixes, runs and tests a program through the
/// MCP tools alone, and stays inside the project and off the network.
#[test]
fn mcp_server() {
    use std::io::{BufRead, BufReader, Write};
    let dir = std::env::temp_dir().join(format!("mimic-golden-mcp-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    assert!(mimic().args(["create", "app"]).current_dir(&dir).status().unwrap().success());
    let app = dir.join("app");
    let session = |extra: &[&str]| {
        let mut child = mimic().arg("mcp").args(extra).current_dir(&app).stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).spawn().unwrap();
        let input = child.stdin.take().unwrap();
        let output = BufReader::new(child.stdout.take().unwrap());
        (child, input, output)
    };
    let (mut child, mut input, mut output) = session(&[]);
    let mut id = 0;
    let mut call = |method: &str, params: serde_json::Value| -> serde_json::Value {
        id += 1;
        writeln!(input, "{}", serde_json::json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params })).unwrap();
        let mut line = String::new();
        output.read_line(&mut line).unwrap();
        let v: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(v["id"], id);
        v["result"].clone()
    };
    let r = call("initialize", serde_json::json!({ "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "test", "version": "1" } }));
    assert_eq!(r["serverInfo"]["name"], "mimic");
    assert!(r["instructions"].as_str().unwrap().contains("mimic://manual/agents"));
    let tools: Vec<String> = call("tools/list", serde_json::json!({}))["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap().to_string()).collect();
    for t in ["check", "run", "test", "manual", "symbol", "write_file"] {
        assert!(tools.contains(&t.to_string()), "{tools:?}");
    }
    assert!(!tools.contains(&"install".to_string()), "install is off by default");
    let mut tool = |name: &str, args: serde_json::Value| -> (bool, String) {
        let r = call("tools/call", serde_json::json!({ "name": name, "arguments": args }));
        (r["isError"] == true, r["content"][0]["text"].as_str().unwrap().to_string())
    };
    // write a program with an error; check finds it with its fix
    tool("write_file", serde_json::json!({ "path": "src/main.mim", "text": "module main\n\nfn main() {\n    let x = 1\n    x = 2\n    print(x)\n}\n" }));
    let (_, t) = tool("check", serde_json::json!({}));
    let v: serde_json::Value = serde_json::from_str(&t).unwrap();
    assert_eq!(v["success"], false);
    assert_eq!(v["diagnostics"][0]["code"], "E0601");
    assert_eq!(v["diagnostics"][0]["fixes"][0]["message"], "make `x` mutable");
    // fixed, run, tested
    tool("write_file", serde_json::json!({ "path": "src/main.mim", "text": "module main\n\n// > double(2)\n// 4\nfn double(x: i64) -> i64 { x × 2 }\n\nfn main() {\n    print(⍳ 5 ¨ double)\n}\n" }));
    let (_, t) = tool("check", serde_json::json!({}));
    assert!(t.contains("\"success\": true"), "{t}");
    let (err, t) = tool("run", serde_json::json!({}));
    assert!(!err && t.contains("[0 2 4 6 8]"), "{t}");
    let (err, t) = tool("test", serde_json::json!({}));
    assert!(!err && t.contains("1 passed"), "{t}");
    let (_, t) = tool("symbol", serde_json::json!({ "name": "double" }));
    assert!(t.contains("fn double(x: i64) -> i64") && t.contains("pure"), "{t}");
    let (_, t) = tool("manual", serde_json::json!({ "topic": "keep" }));
    assert!(t.contains("⌿"), "{t}");
    // outside the project, and the network: off
    let (err, t) = tool("write_file", serde_json::json!({ "path": "../outside.txt", "text": "x" }));
    assert!(err && t.contains("outside the project"), "{t}");
    assert!(!dir.join("outside.txt").exists());
    tool("write_file", serde_json::json!({ "path": "src/main.mim", "text": "module main\n\nfn main() {\n    print(http::fetch(\"https://example.com\"))\n}\n" }));
    let (err, t) = tool("run", serde_json::json!({}));
    assert!(err && t.contains("--allow-network") && t.contains("http::fetch"), "{t}");
    let r = call("resources/read", serde_json::json!({ "uri": "mimic://manual/agents" }));
    assert!(r["contents"][0]["text"].as_str().unwrap().contains("Writing Mimic for agents"));
    let r = call("prompts/get", serde_json::json!({ "name": "port_to_mimic", "arguments": { "code": "x = 1", "language": "Python" } }));
    assert!(r["messages"][0]["content"]["text"].as_str().unwrap().contains("coming-from-python"));
    drop(input);
    assert!(child.wait().unwrap().success());
    // read-only: no writing tools at all
    let (mut child, mut input, mut output) = session(&["--read-only"]);
    writeln!(input, "{}", serde_json::json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" })).unwrap();
    let mut line = String::new();
    output.read_line(&mut line).unwrap();
    assert!(!line.contains("write_file") && !line.contains("\"run\"") && line.contains("\"check\""), "{line}");
    drop(input);
    assert!(child.wait().unwrap().success());
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Guides: Markdown pages in docs/ join `mimic doc` (with Mimic code
/// coloured and glyphs explained), their examples run under `mimic test`,
/// and agents write them through `mimic mcp` with the examples checked.
#[test]
fn doc_guides() {
    use std::io::{BufRead, BufReader, Write};
    let dir = std::env::temp_dir().join(format!("mimic-golden-guides-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    assert!(mimic().args(["create", "app"]).current_dir(&dir).status().unwrap().success());
    let app = dir.join("app");
    std::fs::write(app.join("src/main.mim"), "module main\n\n// The larger of two numbers.\npub fn larger(a: i64, b: i64) -> i64 { a ⌈ b }\n\npub fn smaller(a: i64, b: i64) -> i64 { a ⌊ b }\n\nfn main() { print(larger(1, 2)) }\n").unwrap();
    std::fs::create_dir_all(app.join("docs")).unwrap();
    let guide = "# Comparing\n\n```mimic run\nmodule main\n\nfn main() {\n    print(⍳ 3 ▷ +/)\n}\n```\n\n```output\n3\n```\n";
    std::fs::write(app.join("docs/comparing.md"), guide).unwrap();
    let out = mimic().arg("test").current_dir(&app).output().unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success() && text.contains("✓ docs/comparing.md: example 1"), "{text}");
    std::fs::write(app.join("docs/comparing.md"), guide.replace("```output\n3\n", "```output\n4\n")).unwrap();
    let out = mimic().arg("test").current_dir(&app).output().unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success() && text.contains("✗ docs/comparing.md: example 1") && text.contains("expected:"), "{text}");
    std::fs::write(app.join("docs/comparing.md"), guide).unwrap();
    // the site: the guide, coloured code, glyph tooltips
    let out = mimic().args(["doc", "--json"]).current_dir(&app).output().unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["guides"][0]["name"], "comparing", "{v}");
    assert_eq!(v["guides"][0]["examples"], 1);
    assert!(mimic().arg("doc").current_dir(&app).status().unwrap().success());
    let html = std::fs::read_to_string(app.join("target/doc/index.html")).unwrap();
    assert!(html.contains("id=\"guide-comparing\""));
    assert!(html.contains("title=\"iota: ⍳ n → [0 1 … n-1]\""), "glyph tooltips");
    assert!(html.contains("<span class=\"k\">fn</span>"), "coloured code");
    // an agent through MCP: what is undocumented, and a guide checked as it is written
    let mut child = mimic().arg("mcp").current_dir(&app).stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).spawn().unwrap();
    let mut input = child.stdin.take().unwrap();
    let mut output = BufReader::new(child.stdout.take().unwrap());
    let mut id = 0;
    let mut tool = |name: &str, args: serde_json::Value| -> (bool, String) {
        id += 1;
        writeln!(input, "{}", serde_json::json!({ "jsonrpc": "2.0", "id": id, "method": "tools/call", "params": { "name": name, "arguments": args } })).unwrap();
        let mut line = String::new();
        output.read_line(&mut line).unwrap();
        let v: serde_json::Value = serde_json::from_str(&line).unwrap();
        (v["result"]["isError"] == true, v["result"]["content"][0]["text"].as_str().unwrap().to_string())
    };
    let (_, t) = tool("docs_status", serde_json::json!({}));
    assert!(t.contains("main::smaller") && !t.contains("main::larger"), "{t}");
    let (err, t) = tool("docs_write", serde_json::json!({ "name": "more", "markdown": guide.replace("```output\n3\n", "```output\n5\n") }));
    assert!(err && t.contains("\"expected\": \"5\\n\"") && t.contains("\"got\": \"3\\n\""), "{t}");
    let (err, _) = tool("docs_write", serde_json::json!({ "name": "../escape", "markdown": "x" }));
    assert!(err);
    drop(tool);
    drop(input);
    assert!(child.wait().unwrap().success());
    std::fs::remove_dir_all(&dir).unwrap();
}

/// `mimic why`: where code runs, as the compiler decides it: in parallel
/// (`¨` on all cores, fork-join recursion), fused, as a task, in order; and
/// every run fixture's decisions can be explained.
#[test]
fn where_code_runs() {
    let dir = std::env::temp_dir().join(format!("mimic-golden-why-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let f = dir.join("w.mim");
    std::fs::write(&f, "module main\n\nfn fib(n: i64) -> i64 { if n < 2 { n } else { fib(n - 1) + fib(n - 2) } }\n\nfn square(x: i64) -> i64 { x * x }\n\nfn shout(x: i64) -> i64 {\n    print(x)\n    x\n}\n\nfn load(path: str) -> Result[str, str] { fs::read(path) }\n\nfn main() {\n    let xs = ⍳ 1000\n    print(xs × 2 ▷ +/)\n    print(xs ¨ square ▷ +/)\n    print([\"ab\" \"cd\"] ¨ (s => upper(s)))\n    print([1 2] ¨ shout)\n    let text = load(\"w.mim\")\n    print(fib(20))\n    print(text)\n}\n").unwrap();
    let out = mimic().args(["why", "--in"]).arg(&f).arg("--json").output().unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap_or_else(|_| panic!("{}", String::from_utf8_lossy(&out.stderr)));
    let has = |func: &str, kind: &str, code: &str| v.as_array().unwrap().iter().any(|d| d["function"] == func && d["kind"] == kind && d["code"].as_str().unwrap().contains(code));
    assert!(has("fib", "parallel", "fib(n - 1)"), "fork-join: {v}");
    assert!(has("main", "fused", "xs ¨ square") && has("main", "simd", "square"), "fused, with SIMD across elements: {v}");
    assert!(has("main", "parallel", "upper"), "¨ on all cores: {v}");
    assert!(has("main", "ordered", "shout"), "¨ with printing stays in order: {v}");
    assert!(has("main", "task", "load("), "an implicit task: {v}");
    assert!(has("main", "fused", "xs × 2"), "fused: {v}");
    let out = mimic().args(["why", "shout", "--in"]).arg(&f).output().unwrap();
    assert!(String::from_utf8_lossy(&out.stdout).contains("nothing to decide"), "{}", String::from_utf8_lossy(&out.stdout));
    let out = mimic().args(["why", "main", "--in"]).arg(&f).output().unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("∥ parallel") && text.contains("→ ordered") && text.contains("would run on all cores"), "{text}");
    // every run fixture's decisions are explained
    let mut failed = Vec::new();
    for fx in fixtures("run") {
        let out = mimic().args(["why", "--json", "--in"]).arg(&fx).output().unwrap();
        if !out.status.success() || serde_json::from_slice::<serde_json::Value>(&out.stdout).is_err() {
            failed.push(format!("{}: {}", fx.display(), String::from_utf8_lossy(&out.stderr)));
        }
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
    std::fs::remove_dir_all(&dir).unwrap();
}

/// `mimic audit`: every rule finds its failing fixture and not its passing
/// one (tests/audit), the gate fails on new findings only (baseline), SARIF
/// for code scanning, and dependencies that follow a branch.
#[test]
fn audit_rules() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/audit");
    let audit = |f: &Path| -> std::collections::BTreeSet<String> {
        let out = mimic().args(["audit", "--format", "json", "--fail-on", "never", "--in"]).arg(f).output().unwrap();
        let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap_or_else(|_| panic!("{}: {}", f.display(), String::from_utf8_lossy(&out.stderr)));
        v["findings"].as_array().unwrap().iter().map(|x| x["rule"].as_str().unwrap().to_string()).collect()
    };
    let mut problems = Vec::new();
    let mut rules = std::collections::BTreeSet::new();
    for e in std::fs::read_dir(&dir).unwrap() {
        let f = e.unwrap().path();
        let name = f.file_name().unwrap().to_string_lossy().to_string();
        let Some((rule, kind)) = name.trim_end_matches(".mim").split_once('_') else { continue };
        let rule = rule.to_uppercase();
        rules.insert(rule.clone());
        let found = audit(&f);
        match kind {
            "fail" if !found.contains(&rule) => problems.push(format!("{name}: {rule} not found ({found:?})")),
            "pass" if found.contains(&rule) => problems.push(format!("{name}: {rule} found")),
            _ => {}
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
    assert!(rules.len() >= 14, "a fixture pair for every rule: {rules:?}");
    // the gate: a security finding fails; once in the baseline, it does not
    let tmp = std::env::temp_dir().join(format!("mimic-golden-audit-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let f = tmp.join("a.mim");
    std::fs::copy(dir.join("s01_fail.mim"), &f).unwrap();
    let gate = |extra: &[&str]| mimic().arg("audit").args(extra).arg("--in").arg(&f).output().unwrap().status.success();
    assert!(!gate(&[]), "a new security finding fails");
    let base = tmp.join("base.json");
    assert!(gate(&["--update-baseline", "--baseline", base.to_str().unwrap()]), "{}", base.display());
    assert!(gate(&["--baseline", base.to_str().unwrap()]), "known findings pass");
    std::fs::write(&f, std::fs::read_to_string(&f).unwrap().replace("print(os::shell(\"ls {dir}\"))", "print(os::shell(\"ls {dir}\"))\n    print(os::shell(\"rm {dir}\"))")).unwrap();
    assert!(!gate(&["--baseline", base.to_str().unwrap()]), "a new one fails again");
    assert!(gate(&["--fail-on", "never"]));
    // SARIF
    let out = mimic().args(["audit", "--format", "sarif", "--fail-on", "never", "--in"]).arg(&f).output().unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["version"], "2.1.0");
    assert_eq!(v["runs"][0]["results"][0]["ruleId"], "S01");
    // S07: a dependency following a branch
    assert!(mimic().args(["create", "app"]).current_dir(&tmp).status().unwrap().success());
    let app = tmp.join("app");
    let m = std::fs::read_to_string(app.join("mim.mim")).unwrap();
    let start = m.find("dependencies {").unwrap();
    let end = start + m[start..].find('}').unwrap() + 1;
    let deps = "dependencies {\n    lib_a = { git = \"github.com/x/a.git\" }\n    lib_b = { git = \"github.com/x/b.git\", rev = \"v1.2.0\" }\n}";
    std::fs::write(app.join("mim.mim"), format!("{}{deps}{}", &m[..start], &m[end..])).unwrap();
    // (the dependencies need not exist for the manifest rule: audit only the manifest's view)
    let out = mimic().args(["audit", "--format", "json", "--fail-on", "never"]).current_dir(&app).output().unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    let err = String::from_utf8_lossy(&out.stderr);
    // (the libraries are not installed: the code does not load, the manifest's rule still speaks)
    assert!(!out.status.success() && err.contains("S07") && err.contains("`lib_a` follows the default branch") && !err.contains("branch-dependency: `lib_b`"), "{text}{err}");
    std::fs::remove_dir_all(&tmp).unwrap();
}

/// Contracts: `requires` and `ensures` are checked in debug builds and tests
/// (an early `return` too), cost nothing in release builds, appear in
/// signatures and keep their place under `mimic fmt`.
#[test]
fn contracts() {
    let dir = std::env::temp_dir().join(format!("mimic-golden-contracts-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let f = dir.join("c.mim");
    let src = "module main\n\n// The whole square root of n.\nfn isqrt(n: i64) -> i64\n    requires n >= 0\n    ensures result * result <= n and (result + 1) * (result + 1) > n\n{\n    if n < 2 { return n }\n    let r = isqrt(n div 4) * 2\n    if (r + 1) * (r + 1) <= n { r + 1 } else { r }\n}\n\nfn wrong(n: i64) -> i64\n    ensures result > n\n{\n    if n > 5 { return n }\n    n + 1\n}\n\nfn main() {\n    print(isqrt(1000000))\n    print(wrong(3))\n    print(wrong(9))\n}\n";
    std::fs::write(&f, src).unwrap();
    let out = mimic().arg("run").arg(&f).output().unwrap();
    assert_eq!(out.status.code(), Some(101));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "1000\n4\n");
    assert!(String::from_utf8_lossy(&out.stderr).contains("contract broken: `wrong` ensures result > n (line 14), but it returned 9"), "{}", String::from_utf8_lossy(&out.stderr));
    let out = mimic().args(["run", "--release"]).arg(&f).output().unwrap();
    assert!(out.status.success(), "release builds leave contracts out");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "1000\n4\n9\n");
    std::fs::write(&f, src.replace("print(isqrt(1000000))", "print(isqrt(¯4))")).unwrap();
    let out = mimic().arg("run").arg(&f).output().unwrap();
    assert!(String::from_utf8_lossy(&out.stderr).contains("`isqrt` requires n >= 0 (line 5), and the call broke it"));
    std::fs::write(&f, src).unwrap();
    let out = mimic().args(["query", "--in"]).arg(&f).args(["docs", "name=isqrt"]).output().unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["signature"], "fn isqrt(n: i64) -> i64\n    requires n >= 0\n    ensures result * result <= n and (result + 1) * (result + 1) > n");
    assert!(mimic().args(["fmt", "--check"]).arg(&f).status().unwrap().success(), "contract lines stay indented");
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Property tests: `test "…" for xs: [i64]` runs generated inputs, and a
/// failure is shrunk to the smallest case, the same on every run.
#[test]
fn property_tests() {
    let dir = std::env::temp_dir().join(format!("mimic-golden-props-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let f = dir.join("p.mim");
    std::fs::write(&f, "module main\n\nfn main() { print(1) }\n\ntest \"sorting keeps the length\" for xs: [i64] {\n    expect(≢ sort(xs) == ≢ xs)\n}\n\ntest \"reverse twice\" for s: str, xs: [str] {\n    expect(⌽ ⌽ xs == xs)\n    expect(≢ s >= 0)\n}\n\ntest \"every list is short\" for xs: [i64] {\n    expect(≢ xs < 3)\n}\n\ntest \"small numbers\" for x: i64 {\n    expect(x < 10)\n}\n").unwrap();
    let run = || {
        let out = mimic().arg("test").arg(&f).output().unwrap();
        (out.status.success(), format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr)))
    };
    let (ok, text) = run();
    assert!(!ok, "{text}");
    assert!(text.contains("✓ sorting keeps the length") && text.contains("✓ reverse twice"), "{text}");
    assert!(text.contains("the smallest failing case: xs = [0 0 0]"), "{text}");
    assert!(text.contains("the smallest failing case: x = 10"), "{text}");
    assert!(text.contains("left:  10") && text.contains("right: 10"), "the failed expect shows its sides: {text}");
    assert_eq!(run().1.lines().filter(|l| l.contains("smallest")).collect::<Vec<_>>(), text.lines().filter(|l| l.contains("smallest")).collect::<Vec<_>>(), "the same cases every run");
    std::fs::write(&f, "module main\n\nfn main() { print(1) }\n\ntest \"no\" for m: [str: i64] {\n    expect(true)\n}\n").unwrap();
    let out = mimic().arg("check").arg(&f).output().unwrap();
    assert!(String::from_utf8_lossy(&out.stderr).contains("property tests generate i64, f64, bool, str and lists of them"));
    std::fs::remove_dir_all(&dir).unwrap();
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
    assert!(proj.join(format!("target/release/demo{}", std::env::consts::EXE_SUFFIX)).is_file(), "build must default to the release profile");
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
        // backends LLVM ships that cannot compile Mimic code yet say so
        if name == "arc" {
            let out = mimic().arg("build").arg(&f).args(["--target", &t]).output().unwrap();
            assert!(String::from_utf8_lossy(&out.stderr).contains("experimental ARC backend"), "{t}");
            continue;
        }
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
    // (C's stdout in text mode writes \r\n on Windows)
    assert_eq!(String::from_utf8_lossy(&std::process::Command::new(&exe).output().unwrap().stdout).replace("\r\n", "\n"), "1.414\n");
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
    // Node's WASI on Windows lacks parts of preview 1 (listing directories)
    // and its engine faults there; WASI programs are checked on Linux and macOS
    if cfg!(windows) {
        eprintln!("skipping: Node's WASI is incomplete on Windows");
        return;
    }
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
         const wasi = new WASI({ version: 'preview1', args: [f], env: {}, returnOnExit: true, preopens: { '/tmp': process.argv[3] } });\n\
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
        // the program's /tmp: a fresh host directory per fixture
        let tmp = dir.join(format!("tmp-{stem}"));
        std::fs::create_dir_all(&tmp).unwrap();
        let mut run = std::process::Command::new("node").arg("--no-warnings").arg(&runner).arg(wasm).arg(&tmp).output().unwrap();
        for _ in 0..2 {
            let Some(sig) = signal_of(run.status) else { break };
            eprintln!("{stem}: node died from signal {sig} (engine fault); retrying");
            run = std::process::Command::new("node").arg("--no-warnings").arg(&runner).arg(wasm).arg(&tmp).output().unwrap();
        }
        let status = run.status;
        let mut probe = String::new();
        {
            if signal_of(status).is_some() {
                // still crashing: which engine settings avoid it (diagnostics)
                let sh = |c: &str| std::process::Command::new("sh").arg("-c").arg(c).output().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default();
                probe.push_str(&format!("ulimit -s: {}; node: {}\n", sh("ulimit -s"), sh("node --version")));
                for flags in [&["--liftoff-only"][..], &["--no-liftoff"], &["--stack-size=400"], &["--stack-size=4000"], &["--no-wasm-tier-up"], &["--no-wasm-lazy-compilation"], &["--no-wasm-trap-handler"], &["--single-threaded"]] {
                    let r = std::process::Command::new("node").arg("--no-warnings").args(flags).arg(&runner).arg(wasm).arg(&tmp).output().unwrap();
                    let ok = String::from_utf8_lossy(&r.stdout) == std::fs::read_to_string(f.with_extension("out")).unwrap_or_default();
                    probe.push_str(&format!("  node {flags:?}: {} ({})\n", if ok { "ok" } else { "differs" }, r.status));
                    if !r.status.success() {
                        let e = String::from_utf8_lossy(&r.stderr);
                        probe.push_str(&format!("    {}\n", e.lines().rev().take(3).collect::<Vec<_>>().join(" | ")));
                    }
                }
            }
        }
        // a fixture may expect different output under WASI (no processes, …)
        let wasi_out = f.with_extension("wasi.out");
        let want = std::fs::read_to_string(if wasi_out.is_file() { wasi_out } else { f.with_extension("out") }).unwrap();
        let got = String::from_utf8_lossy(&run.stdout);
        if got != want {
            failures.push(format!(
                "{stem} ({status}, {wasm})\n--- want\n{want}--- got\n{got}--- stderr\n{}{probe}",
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

/// `mimic doc`: public items with their signatures and the comments above
/// them; private ones left out; HTML page and JSON.
#[test]
fn doc_command() {
    let dir = std::env::temp_dir().join(format!("mimic-doc-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let f = dir.join("shapes.mim");
    std::fs::write(
        &f,
        "module main\n\n// A point on the plane.\npub type Point = { x: f64, y: f64 }\n\n// Distance between two points, `a` and `b`.\npub fn dist(a: Point, b: Point, scale: f64 = 1.0) -> f64 {\n    scale * math::sqrt((a.x - b.x) * (a.x - b.x) + (a.y - b.y) * (a.y - b.y))\n}\n\nfn hidden() -> i64 { 1 }\n\nfn main() {\n    print(dist(Point { x: 0.0, y: 0.0 }, Point { x: 3.0, y: 4.0 }))\n}\n",
    )
    .unwrap();
    let out = mimic().arg("doc").arg(&f).arg("--json").output().unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap_or_else(|_| panic!("{}", String::from_utf8_lossy(&out.stderr)));
    let items = &v["modules"][0]["items"];
    assert_eq!(items[0]["signature"], "type Point = {\n    x: f64,\n    y: f64\n}");
    assert_eq!(items[0]["doc"], "A point on the plane.");
    assert_eq!(items[1]["signature"], "fn dist(a: Point, b: Point, scale: f64 = 1.0) -> f64");
    assert_eq!(items[1]["doc"], "Distance between two points, `a` and `b`.");
    assert_eq!(items.as_array().unwrap().len(), 2, "{v}");
    let html = mimic().arg("doc").arg(&f).arg("--out").arg(dir.join("out")).output().unwrap();
    assert!(html.status.success(), "{}", String::from_utf8_lossy(&html.stderr));
    let page = std::fs::read_to_string(dir.join("out/index.html")).unwrap();
    // (the signature is coloured: keywords and types in spans)
    assert!(page.contains("<code>a</code>") && page.contains("<span class=\"k\">fn</span> dist(a: <span class=\"t\">Point</span>") && !page.contains("hidden"), "{page}");
    let std = mimic().arg("doc").arg("--std").arg("--json").output().unwrap();
    let s: serde_json::Value = serde_json::from_slice(&std.stdout).unwrap();
    let paths: Vec<&str> = s["modules"].as_array().unwrap().iter().map(|m| m["path"].as_str().unwrap()).collect();
    assert!(paths.contains(&"fs") && paths.contains(&"http") && paths.contains(&"json"), "{paths:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Debug builds carry DWARF: the functions under their Mimic names, the
/// source file, line tables; release builds carry none.
#[test]
fn debug_info() {
    let dir = std::env::temp_dir().join(format!("mimic-dwarf-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let f = dir.join("dwarfy.mim");
    std::fs::write(&f, "module main\n\nfn area(w: f64, h: f64) -> f64 {\n    let a = w * h\n    a\n}\n\nfn main() {\n    print(area(3.0, 4.0))\n}\n").unwrap();
    let has = |release: bool| -> (bool, bool, bool) {
        let out = mimic().arg("build").arg(&f).arg(if release { "--release" } else { "--debug" }).arg("--emit-json").output().unwrap();
        let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap_or_else(|_| panic!("{}", String::from_utf8_lossy(&out.stderr)));
        let exe = std::path::PathBuf::from(v["artifacts"][0]["path"].as_str().unwrap());
        // (on macOS the DWARF stays in the object files next to the program:
        // `dwarfy.o`, and `dwarfy.part1.o` … when the code is generated in
        // parts, as a larger prelude makes it)
        let stem = exe.file_stem().unwrap().to_string_lossy().to_string();
        let mut bytes = Vec::new();
        for e in std::fs::read_dir(exe.parent().unwrap()).unwrap() {
            let p = e.unwrap().path();
            let name = p.file_name().unwrap().to_string_lossy().to_string();
            if name.starts_with(&stem) && name.ends_with(".o") {
                bytes.extend(std::fs::read(&p).unwrap());
            }
        }
        let find = |s: &str| bytes.windows(s.len()).any(|w| w == s.as_bytes());
        (find("debug_line"), find("main::area"), find("dwarfy.mim"))
    };
    assert_eq!(has(false), (true, true, true));
    assert_eq!(has(true), (false, false, false));
    let _ = std::fs::remove_dir_all(&dir);
}

/// `--fast-math` and `--strict-math`: every fixture of numbers gives its
/// expected output either way (their float results are exact), and the two
/// flags exclude each other.
#[test]
fn math_flags() {
    for name in ["fusion.mim", "sized_numbers.mim", "gpu_map.mim", "each_kernels.mim"] {
        let f = fixtures("run").into_iter().find(|p| p.ends_with(name)).unwrap();
        let want = std::fs::read_to_string(f.with_extension("out")).unwrap();
        for flag in ["--fast-math", "--strict-math"] {
            let out = mimic().arg("run").arg(&f).args(["--release", flag]).output().unwrap();
            assert_eq!(String::from_utf8_lossy(&out.stdout), want, "{name} {flag}: {}", String::from_utf8_lossy(&out.stderr));
        }
    }
    let f = fixtures("run").into_iter().find(|p| p.ends_with("basics.mim")).unwrap();
    let out = mimic().arg("build").arg(&f).args(["--fast-math", "--strict-math"]).output().unwrap();
    assert!(!out.status.success() && String::from_utf8_lossy(&out.stderr).contains("cannot be used with"));
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
    print(unsafe { c::sqlite3_open(":memory:", db_out) })
    let db = db_out[0]
    let mut stmt_out = [0]
    let mut tail = [0]
    print(unsafe { c::sqlite3_prepare_v2(db, "select 'mimic', 40 + 2", ¯1, stmt_out, tail) })
    let stmt = stmt_out[0]
    print(unsafe { c::sqlite3_step(stmt) })
    print(unsafe { c::sqlite3_column_text(stmt, 0) })
    print(unsafe { c::sqlite3_column_int64(stmt, 1) })
    print(unsafe { c::sqlite3_finalize(stmt) } + unsafe { c::sqlite3_close(db) })
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
        "module main\n\ntrusted extern \"C\" include \"vec.h\"\n\nfn main() {\n    print(c::dot([1.0 2.0 3.0], [4.0 5.0 6.0], 3))\n    print(c::greeting())\n}\n",
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
        "module main\n\ntrusted extern \"C\" include \"geo.h\"\n\nfn main() {\n    print(c::vlen(c::Vec2 { x: 3.0, y: 4.0 }))\n    print(c::vadd(c::Vec2 { x: 3.0, y: 4.0 }, c::Vec2 { x: 1.0, y: 1.0 }))\n\
         \n    let r = c::mkrect(1.5, 2.5, 640, 480)\n    print(r)\n    print(c::area(r))\n}\n",
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

/// C names keep their spelling under `c::`, keywords and glyph words too
/// (`c::div` from stdlib.h, a field `t.type`, `c::sum`), next to Mimic's own.
#[test]
fn c_reserved_names() {
    let dir = std::env::temp_dir().join(format!("mimic-golden-creserved-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    assert!(mimic().current_dir(&dir).args(["create", "kw"]).status().unwrap().success());
    let proj = dir.join("kw");
    std::fs::create_dir_all(proj.join("c")).unwrap();
    std::fs::write(
        proj.join("c/kw.h"),
        "struct token { int type; int match; };
struct token next(int type);
int sum(int a, int b);
int take(int n);
",
    )
    .unwrap();
    std::fs::write(
        proj.join("c/kw.c"),
        "#include \"kw.h\"\nstruct token next(int type) { struct token t = { type + 1, 7 }; return t; }\n\
         int sum(int a, int b) { return a + b; }\nint take(int n) { return n * 2; }\n",
    )
    .unwrap();
    let manifest = std::fs::read_to_string(proj.join("mim.mim")).unwrap();
    std::fs::write(proj.join("mim.mim"), manifest.replace("link = []", "sources = [\"c/kw.c\"]\n    include = [\"c\"]")).unwrap();
    std::fs::write(
        proj.join("src/main.mim"),
        "module main\n\ntrusted extern \"C\" include \"kw.h\"\ntrusted extern \"C\" include \"stdlib.h\"\n\n\
         fn main() {\n    let t = c::next(4)\n    print((t.type, t.match))\n    print(c::sum(2, 3))\n    print(c::take(21))\n\
             let d = c::div(17, 5)\n    print((d.quot, d.rem))\n    print(c::abs(¯3))\n    print([1 2 3] ▷ sum)\n}\n",
    )
    .unwrap();
    let out = mimic().current_dir(&proj).arg("run").output().unwrap();
    assert_eq!(String::from_utf8_lossy(&out.stdout), "(5, 7)\n5\n42\n(3, 2)\n3\n6\n", "{}", String::from_utf8_lossy(&out.stderr));
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Constants and globals from C: `#define` numbers, text and floats, enum
/// values (counted), `extern` globals read live, expression macros
/// (`c::stdout`, `c::errno`), from a project header and system headers.
#[test]
fn c_constants() {
    let dir = std::env::temp_dir().join(format!("mimic-golden-cconst-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    assert!(mimic().current_dir(&dir).args(["create", "kc"]).status().unwrap().success());
    let proj = dir.join("kc");
    std::fs::create_dir_all(proj.join("c")).unwrap();
    std::fs::write(
        proj.join("c/kc.h"),
        "#define ANSWER 42\n#define TWICE (ANSWER * 2)\n#define HALF (0.5f)\n#define GREETING \"hi\\tthere\"\n\
         #define ALL_BITS 0xFFFFFFFFFFFFFFFFULL\nenum color { RED, GREEN = 5, BLUE };\nextern int counter;\nint bump(void);\n",
    )
    .unwrap();
    std::fs::write(proj.join("c/kc.c"), "#include \"kc.h\"\nint counter = 7;\nint bump(void) { return ++counter; }\n").unwrap();
    let manifest = std::fs::read_to_string(proj.join("mim.mim")).unwrap();
    std::fs::write(proj.join("mim.mim"), manifest.replace("link = []", "sources = [\"c/kc.c\"]\n    include = [\"c\"]")).unwrap();
    std::fs::write(
        proj.join("src/main.mim"),
        "module main\n\ntrusted extern \"C\" include \"kc.h\"\ntrusted extern \"C\" include \"stdio.h\"\n\
         trusted extern \"C\" include \"errno.h\"\ntrusted extern \"C\" include \"math.h\"\n\n\
         fn main() {\n    print((c::ANSWER, c::TWICE, c::HALF, c::GREETING, c::ALL_BITS))\n    print((c::RED, c::GREEN, c::BLUE))\n\
             print(c::counter)\n    c::bump()\n    print(c::counter)\n    print((c::SEEK_END, c::EOF, c::M_PI > 3.14))\n\
             print(c::fflush(c::stdout))\n    print(c::errno >= 0)\n}\n",
    )
    .unwrap();
    let out = mimic().current_dir(&proj).arg("run").output().unwrap();
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "(42, 84, 0.5, \"hi\tthere\", -1)\n(0, 5, 6)\n7\n8\n(2, -1, true)\n0\ntrue\n",
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    // the same program for Windows: its getters link against mingw-w64
    let out = mimic().current_dir(&proj).args(["build", "--target", "x86_64-windows"]).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Structs with array fields, `c::T { … }` with fields left out (zero, as
/// `= {0}`), plain structs through pointers (`const`: copied in; otherwise
/// copied in and back out, in a tuple with the result), termios, and
/// `mimic check --fix` adding `c::` to old code.
#[test]
fn c_struct_pointers() {
    let dir = std::env::temp_dir().join(format!("mimic-golden-cptr-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    assert!(mimic().current_dir(&dir).args(["create", "ka"]).status().unwrap().success());
    let proj = dir.join("ka");
    std::fs::create_dir_all(proj.join("c")).unwrap();
    std::fs::write(
        proj.join("c/ka.h"),
        "struct buf { char data[4]; int len; };\nstruct pt { double xy[2]; };\nstruct box { struct pt corner; int tags[3]; };\n\
         int sum_buf(struct buf b);\nstruct buf make_buf(int n);\nvoid fill_box(struct box *b);\ndouble box_area(const struct box *b);\n\
         int scale(struct pt *p, double k);\n",
    )
    .unwrap();
    std::fs::write(
        proj.join("c/ka.c"),
        "#include \"ka.h\"\nint sum_buf(struct buf b) { int s = 0; for (int i = 0; i < 4; i++) s += b.data[i]; return s + b.len; }\n\
         struct buf make_buf(int n) { struct buf b = {{1, 2, 3, 4}, n}; return b; }\n\
         void fill_box(struct box *b) { b->corner.xy[0] = 3; b->corner.xy[1] = 4; b->tags[2] = 9; }\n\
         double box_area(const struct box *b) { return b->corner.xy[0] * b->corner.xy[1]; }\n\
         int scale(struct pt *p, double k) { p->xy[0] *= k; p->xy[1] *= k; return 7; }\n",
    )
    .unwrap();
    let manifest = std::fs::read_to_string(proj.join("mim.mim")).unwrap();
    std::fs::write(proj.join("mim.mim"), manifest.replace("link = []", "sources = [\"c/ka.c\"]\n    include = [\"c\"]")).unwrap();
    // termios is a Unix header: the Windows run checks the rest
    let (tio_inc, tio_use) = if cfg!(unix) {
        (
            "trusted extern \"C\" include \"termios.h\"\n",
            "    let (got, t) = c::tcgetattr(0, c::Termios {})\n    print((got == 0 or got == ¯1, ≢ t.c_cc == c::NCCS))\n",
        )
    } else {
        ("", "    print((true, true))\n")
    };
    std::fs::write(
        proj.join("src/main.mim"),
        format!(
            "module main\n\ntrusted extern \"C\" include \"ka.h\"\n{tio_inc}\n\
             fn main() {{\n    print(c::sum_buf(c::Buf {{ data: [1 2 3 4], len: 10 }}))\n    print(c::make_buf(5))\n    print(c::Buf {{ len: 2 }})\n\
                 let b = c::fill_box(c::Box {{}})\n    print(b)\n    print(c::box_area(b))\n\
                 let (rc, p) = c::scale(c::Pt {{ xy: [1.0 2.0] }}, 3.0)\n    print((rc, p))\n{tio_use}}}\n"
        ),
    )
    .unwrap();
    let out = mimic().current_dir(&proj).arg("run").stdin(std::process::Stdio::null()).output().unwrap();
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "20\nBuf { data: [1 2 3 4], len: 5 }\nBuf { data: [0 0 0 0], len: 2 }\nBox { corner: Pt { xy: [3.0 4.0] }, tags: [0 0 9] }\n12.0\n(7, Pt { xy: [3.0 6.0] })\n(true, true)\n",
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    // old code without `c::`: `mimic check --fix` adds it, functions and constants alike
    let old = dir.join("old.mim");
    std::fs::write(&old, "module main\n\ntrusted extern \"C\" include \"math.h\"\ntrusted extern \"C\" include \"stdio.h\"\n\nfn main() {\n    print(floor(2.7) + sqrt(4.0))\n    print(SEEK_END)\n}\n").unwrap();
    assert!(mimic().args(["check", "--fix"]).arg(&old).status().unwrap().success());
    let fixed = std::fs::read_to_string(&old).unwrap();
    assert!(fixed.contains("c::floor(2.7) + c::sqrt(4.0)") && fixed.contains("print(c::SEEK_END)"), "{fixed}");
    std::fs::remove_dir_all(&dir).unwrap();
}

/// C unions: a record with one field per member; from C every member is
/// filled in, to C the members that are not zero are written (so the one
/// set wins); by value, in structs, through pointers, with array members.
#[test]
fn c_unions() {
    let dir = std::env::temp_dir().join(format!("mimic-golden-cunion-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    assert!(mimic().current_dir(&dir).args(["create", "ku"]).status().unwrap().success());
    let proj = dir.join("ku");
    std::fs::create_dir_all(proj.join("c")).unwrap();
    std::fs::write(
        proj.join("c/ku.h"),
        "union num { int i; float f; };\ntypedef union { long long whole; unsigned char bytes[8]; } word;\nstruct tagged { int kind; union num value; };\n\
         int kind_of(union num n);\nunion num make_float(float x);\ndouble read(struct tagged t);\nvoid bump(struct tagged *t);\nlong long low_byte(word w);\n",
    )
    .unwrap();
    std::fs::write(
        proj.join("c/ku.c"),
        "#include \"ku.h\"\nint kind_of(union num n) { return n.i; }\nunion num make_float(float x) { union num n; n.f = x; return n; }\n\
         double read(struct tagged t) { return t.kind == 0 ? t.value.i : t.value.f; }\n\
         void bump(struct tagged *t) { if (t->kind == 0) t->value.i += 1; else t->value.f += 1.0f; }\n\
         long long low_byte(word w) { return w.bytes[0]; }\n",
    )
    .unwrap();
    let manifest = std::fs::read_to_string(proj.join("mim.mim")).unwrap();
    std::fs::write(proj.join("mim.mim"), manifest.replace("link = []", "sources = [\"c/ku.c\"]\n    include = [\"c\"]")).unwrap();
    std::fs::write(
        proj.join("src/main.mim"),
        "module main\n\ntrusted extern \"C\" include \"ku.h\"\n\nfn main() {\n    print(c::kind_of(c::Num { i: 42 }))\n\
             let n = c::make_float(2.5)\n    print((n.f, c::kind_of(n) == 1075838976))\n\
             print(c::read(c::Tagged { kind: 0, value: c::Num { i: 7 } }))\n    print(c::read(c::Tagged { kind: 1, value: c::Num { f: 1.5 } }))\n\
             let t = c::bump(c::Tagged { kind: 1, value: c::Num { f: 1.5 } })\n    print(t.value.f)\n\
             print((c::low_byte(c::Word { whole: 258 }), c::low_byte(c::Word { bytes: [9 0 0 0 0 0 0 0] })))\n    print(c::Num {})\n}\n",
    )
    .unwrap();
    let out = mimic().current_dir(&proj).arg("run").output().unwrap();
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "42\n(2.5, true)\n7.0\n1.5\n2.5\n(2, 9)\nNum { i: 0, f: 0.0 }\n",
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

/// 3000 WebSocket connections open at the same time against one server
/// (0.2.2): each client holds its connection for half a second, so this
/// passes in seconds only when they all wait at once on the event loop
/// (`¨` over a function that waits: a task per element), on every OS.
#[test]
fn websocket_scale() {
    let start = 12000 + (std::process::id() as u16 % 6000) + 500;
    let port = (start..start + 1000).find(|p| std::net::TcpListener::bind(("127.0.0.1", *p)).is_ok()).expect("a free port");
    let dir = std::env::temp_dir().join(format!("mimic-golden-wsscale-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let src = dir.join("scale.mim");
    std::fs::write(
        &src,
        format!(
            "module main\n\nfn echo(sock: ws::Socket) -> i64 {{\n    match ws::receive(sock) {{\n        Ok(ws::Text(t)) => {{\n            ws::send(sock, t)\n            1\n        }}\n        _ => 0,\n    }}\n}}\n\n\
             fn client(port: i64, i: i64) -> i64 {{\n    match ws::connect(\"ws://127.0.0.1:{{port}}/\") {{\n        Ok(s) => {{\n            time::sleep(500ms)\n            ws::send(s, \"hi {{i}}\")\n\
                         let back = match ws::receive(s) {{\n                Ok(ws::Text(t)) => if t == \"hi {{i}}\" {{ 1 }} else {{ 0 }},\n                _ => 0,\n            }}\n            ws::close(s)\n            back\n        }}\n        Err(e) => 0,\n    }}\n}}\n\n\
             fn server(port: i64, n: i64) -> Result[i64, str] {{ ws::serve_connections(port, echo, n) }}\n\n\
             fn main() {{\n    let n = 3000\n    let port = {port}\n    let served = server(port, n)\n    time::sleep(300ms)\n    let start = time::clock()\n\
                 let ok = +/ (⍳ n ¨ (i => client(port, i)))\n    print(\"{{ok}} of {{n}} echoed\")\n    print(served)\n    print((time::clock() - start) < 30s)\n}}\n"
        ),
    )
    .unwrap();
    let out = mimic().args(["run", "--release"]).arg(&src).output().unwrap();
    assert_eq!(String::from_utf8_lossy(&out.stdout), "3000 of 3000 echoed\nOk(3000)\ntrue\n", "{}", String::from_utf8_lossy(&out.stderr));
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
         pub extern \"C\" fn mm_hypot(a: f64, b: f64) -> f64 { c::sqrt(a * a + b * b) }\n\
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
    let mut cc = mimic();
    cc.current_dir(&proj).args(["cc", "main.c", "-Itarget/release", "target/release/libmm.a", "-o", "main_c"]);
    if cfg!(target_os = "linux") {
        // glibc keeps maths and threads in their own libraries
        cc.args(["-lm", "-lpthread"]);
    }
    let out = cc.output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let run = std::process::Command::new(proj.join("main_c")).output().unwrap();
    assert_eq!(String::from_utf8_lossy(&run.stdout).replace("\r\n", "\n"), "5.0 499500 10\nhello, C\n");

    // the same library shared (.dll, .so, .dylib): linked against, and loaded at run time
    let out = mimic().current_dir(&proj).args(["build", "--shared", "--emit-json"]).output().unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap_or_else(|_| panic!("{}", String::from_utf8_lossy(&out.stderr)));
    assert_eq!(v["artifacts"][0]["kind"], "shared-library", "{v}");
    let (shared, link_with) = if cfg!(windows) {
        ("mm.dll", "target/release/libmm.dll.a")
    } else if cfg!(target_os = "macos") {
        ("libmm.dylib", "target/release/libmm.dylib")
    } else {
        ("libmm.so", "target/release/libmm.so")
    };
    assert!(v["artifacts"][0]["path"].as_str().unwrap().ends_with(shared), "{v}");
    let mut cc = mimic();
    cc.current_dir(&proj).args(["cc", "main.c", "-Itarget/release", link_with, "-o", "main_shared"]);
    if !cfg!(windows) {
        cc.arg("-Wl,-rpath,target/release");
    }
    let out = cc.output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let mut run = std::process::Command::new(proj.join("main_shared"));
    run.current_dir(&proj);
    if cfg!(windows) {
        // Windows finds DLLs on PATH
        run.env("PATH", format!("{};{}", proj.join("target/release").display(), std::env::var("PATH").unwrap_or_default()));
    }
    let run = run.output().unwrap();
    assert_eq!(String::from_utf8_lossy(&run.stdout).replace("\r\n", "\n"), "5.0 499500 10\nhello, C\n", "{}", String::from_utf8_lossy(&run.stderr));
    // only the `extern "C" fn`s are exported
    std::fs::write(
        proj.join("load.c"),
        "#include <stdio.h>\n#ifdef _WIN32\n#include <windows.h>\n#define OPEN(p) LoadLibraryA(p)\n#define SYM(h, n) (void *)GetProcAddress(h, n)\n\
         #else\n#include <dlfcn.h>\n#define OPEN(p) dlopen(p, RTLD_NOW)\n#define SYM(h, n) dlsym(h, n)\n#endif\n\
         int main(int argc, char **argv) {\n  void *h = OPEN(argv[1]);\n  if (!h) { puts(\"no library\"); return 1; }\n\
           long long (*sum_to)(long long) = (long long (*)(long long))SYM(h, \"mm_sum_to\");\n\
           printf(\"%lld %d\\n\", sum_to ? sum_to(10) : -1LL, SYM(h, \"mim_rt_init\") == 0);\n  return 0;\n}\n",
    )
    .unwrap();
    let mut cc = mimic();
    cc.current_dir(&proj).args(["cc", "load.c", "-o", "load_c"]);
    if cfg!(target_os = "linux") {
        cc.arg("-ldl");
    }
    let out = cc.output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let run = std::process::Command::new(proj.join("load_c")).arg(proj.join("target/release").join(shared)).output().unwrap();
    assert_eq!(String::from_utf8_lossy(&run.stdout).replace("\r\n", "\n"), "45 1\n", "{}", String::from_utf8_lossy(&run.stderr));
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

/// C callbacks: Mimic functions, lambdas and closures passed to C as
/// function pointers through generated trampolines (nested calls included).
#[test]
fn c_callbacks() {
    let dir = std::env::temp_dir().join(format!("mimic-golden-cb-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    assert!(mimic().current_dir(&dir).args(["create", "cbp"]).status().unwrap().success());
    let proj = dir.join("cbp");
    std::fs::create_dir_all(proj.join("c")).unwrap();
    std::fs::write(
        proj.join("c/cb.h"),
        "typedef double (*real_fn)(double);\ndouble integrate(real_fn f, double a, double b, int n);\n\
         long long apply_twice(long long (*f)(long long), long long x);\nvoid count_up(int n, void (*visit)(int i, void *ud), void *ud);\n\
         int sum_mapped(int n, int (*f)(int));\n",
    )
    .unwrap();
    std::fs::write(
        proj.join("c/cb.c"),
        "#include \"cb.h\"\ndouble integrate(real_fn f, double a, double b, int n) { double h = (b - a) / n, s = 0; for (int i = 0; i < n; i++) s += f(a + (i + 0.5) * h) * h; return s; }\n\
         long long apply_twice(long long (*f)(long long), long long x) { return f(f(x)); }\n\
         void count_up(int n, void (*visit)(int, void *), void *ud) { for (int i = 0; i < n; i++) visit(i, ud); }\n\
         int sum_mapped(int n, int (*f)(int)) { int s = 0; for (int i = 1; i <= n; i++) s += f(i); return s; }\n",
    )
    .unwrap();
    let manifest = std::fs::read_to_string(proj.join("mim.mim")).unwrap();
    std::fs::write(proj.join("mim.mim"), manifest.replace("link = []", "sources = [\"c/cb.c\"]\n    include = [\"c\"]")).unwrap();
    std::fs::write(
        proj.join("src/main.mim"),
        "module main\n\nextern \"C\" include \"cb.h\"\n\nfn square(x: f64) -> f64 { x × x }\n\nfn main() {\n\
         \x20   print(unsafe { c::integrate(x => 2.0 × x, 0.0, 1.0, 10) })\n    let k = 10\n    print(unsafe { c::apply_twice(x => x + k, 1) })\n\
         \x20   unsafe { c::count_up(3, (i, ud) => print(\"visit {i}\"), 0) }\n    print(unsafe { c::sum_mapped(4, i => unsafe { c::apply_twice(j => j × 2, i) }) })\n\
         \x20   print(unsafe { c::integrate(square, 0.0, 1.0, 4) })\n}\n",
    )
    .unwrap();
    let out = mimic().current_dir(&proj).arg("run").output().unwrap();
    assert_eq!(String::from_utf8_lossy(&out.stdout), "1.0\n21\nvisit 0\nvisit 1\nvisit 2\n40\n0.328125\n", "{}", String::from_utf8_lossy(&out.stderr));
    std::fs::remove_dir_all(&dir).unwrap();
}

/// `mimic build --web`: the page's own loader (taken from its index.html)
/// starts the program in node, then each line of `<name>.events` is sent as
/// the page would send it; what comes back must match `<name>.out`.
#[test]
fn web_pages() {
    if Command::new("node").arg("--version").output().is_err() {
        eprintln!("skipping: node is not installed");
        return;
    }
    let dir = std::env::temp_dir().join(format!("mimic-golden-web-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let driver = dir.join("drive.cjs");
    std::fs::write(
        &driver,
        r#"const fs = require('fs'), path = require('path')
const [page, events] = process.argv.slice(2)
let html = fs.readFileSync(path.join(page, 'index.html'), 'utf8')
let js = html.slice(html.indexOf('<script type="module">') + 22, html.indexOf('</script>'))
js = js.slice(0, js.indexOf('if (!ex.mim_ui_script)'))
js = js.replace("await WebAssembly.instantiateStreaming(fetch('app.wasm'), imports)", "await WebAssembly.instantiate(fs.readFileSync(path.join(page, 'app.wasm')), imports)")
js += `
const start = JSON.parse(call(''))
console.log('title: ' + start.title + ', timers (ms): ' + JSON.stringify(start.every))
console.log(start.html)
for (const e of fs.readFileSync(events, 'utf8').split(String.fromCharCode(10)).filter(l => l)) console.log(call(e))
`
new Function('fs', 'path', 'page', 'events', 'return (async () => {' + js.replace('const { instance }', 'var { instance }') + '})()')(fs, path, page, events)
"#,
    )
    .unwrap();
    let mut failures = Vec::new();
    for f in fixtures("web") {
        let out = mimic().arg("build").arg("--web").arg(&f).arg("--emit-json").output().unwrap();
        let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap_or_else(|_| panic!("{}", String::from_utf8_lossy(&out.stderr)));
        let page = v["artifacts"][0]["path"].as_str().unwrap_or_else(|| panic!("{v}")).to_string();
        let run = Command::new("node").arg(&driver).arg(&page).arg(f.with_extension("events")).output().unwrap();
        let got = String::from_utf8_lossy(&run.stdout).replace("\r\n", "\n");
        let want = std::fs::read_to_string(f.with_extension("out")).unwrap_or_default();
        if got != want {
            failures.push(format!("{}\n--- want\n{want}--- got\n{got}--- stderr\n{}", f.display(), String::from_utf8_lossy(&run.stderr)));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Native windows (AppKit, Win32, GTK 3): each tests/fixtures/window/*.mim is
/// driven by its .steps through the real controls (MIMIC_UI_TEST) and prints
/// its views; that must match its .out on every system. It opens windows on
/// the screen, so it only runs with MIMIC_TEST_WINDOWS=1 (CI: Linux under a
/// virtual display).
#[test]
fn windows() {
    if std::env::var_os("MIMIC_TEST_WINDOWS").is_none() {
        eprintln!("skipping: set MIMIC_TEST_WINDOWS=1 to open test windows");
        return;
    }
    let mut failures = Vec::new();
    for f in fixtures("window") {
        let out = mimic().arg("build").arg(&f).arg("--emit-json").output().unwrap();
        let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap_or_else(|_| panic!("{}", String::from_utf8_lossy(&out.stderr)));
        let exe = v["artifacts"][0]["path"].as_str().unwrap_or_else(|| panic!("{v}")).to_string();
        let run = Command::new(&exe).env("MIMIC_UI", "window").env("MIMIC_UI_TEST", f.with_extension("steps")).output().unwrap();
        let got = String::from_utf8_lossy(&run.stdout).to_string();
        let want = std::fs::read_to_string(f.with_extension("out")).unwrap_or_default();
        if got != want {
            failures.push(format!("{}\n--- want\n{want}--- got\n{got}--- stderr\n{}", f.display(), String::from_utf8_lossy(&run.stderr)));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}


/// `http::serve` keeps connections open (HTTP/1.1): pipelined requests,
/// bodies between them, answered in order; `Connection: close` ends it.
#[test]
fn http_keep_alive() {
    use std::io::{Read, Write};
    // a free port below the ones the OS hands out (32768+), so no outgoing
    // connection of a parallel test takes it between this check and the
    // server's bind (12000-18999: apart from the fixtures' 20000-31999)
    let start = 12000 + std::process::id() as u16 % 6000;
    let port = (start..start + 1000).find(|p| std::net::TcpListener::bind(("127.0.0.1", *p)).is_ok()).expect("a free port");
    let dir = std::env::temp_dir().join(format!("mimic-golden-keepalive-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let src = dir.join("ka.mim");
    std::fs::write(&src, format!("module main\n\nfn handle(req: http::Request) -> http::Response {{ http::ok(\"{{req.path}}:{{req.body}}\") }}\n\nfn main() {{ http::serve({port}, handle) }}\n")).unwrap();
    let out = mimic().args(["build", "--emit-json"]).arg(&src).output().unwrap();
    let json = String::from_utf8_lossy(&out.stdout);
    let at = json.find("\"path\":\"").expect("an artifact path") + 8;
    let exe = &json[at..at + json[at..].find('"').unwrap()];
    let mut server = Command::new(exe).stderr(std::process::Stdio::piped()).spawn().unwrap();
    let mut conn = None;
    // up to 30 s: under the whole suite's load, a new binary can take
    // seconds to start (the OS checks every freshly built executable)
    for _ in 0..1200 {
        if let Ok(c) = std::net::TcpStream::connect(("127.0.0.1", port)) {
            conn = Some(c);
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    // three requests in one write, the second with a body, the third closing;
    // then the same split into pieces (each read separately by the server)
    let msg: &[u8] = b"GET /a HTTP/1.1\r\nHost: x\r\n\r\nPOST /b HTTP/1.1\r\nHost: x\r\nContent-Length: 3\r\n\r\nxyzGET /c HTTP/1.1\r\nConnection: close\r\n\r\n";
    let exchange = |c: &mut std::net::TcpStream, cuts: &[usize]| -> std::io::Result<String> {
        c.set_read_timeout(Some(std::time::Duration::from_secs(10)))?;
        let mut at = 0;
        for &cut in cuts.iter().chain([msg.len()].iter()) {
            c.write_all(&msg[at..cut])?;
            c.flush()?;
            std::thread::sleep(std::time::Duration::from_millis(5));
            at = cut;
        }
        let mut got = String::new();
        c.read_to_string(&mut got)?;
        Ok(got)
    };
    if conn.is_none() {
        // say why: the server stopped (and what it printed), or is still starting
        let state = match server.try_wait() {
            Ok(Some(status)) => {
                let mut err = String::new();
                let _ = server.stderr.take().map(|mut e| e.read_to_string(&mut err));
                format!("it exited with {status}: {err}")
            }
            _ => "it is still running".to_string(),
        };
        let _ = server.kill();
        let _ = server.wait();
        panic!("the server does not listen on port {port} after 30 s: {state}");
    }
    let first = conn.ok_or("the server does not listen").and_then(|mut c| exchange(&mut c, &[]).map_err(|_| "no answer"));
    let split = std::net::TcpStream::connect(("127.0.0.1", port)).map_err(|_| "no second connection").and_then(|mut c| exchange(&mut c, &[7, 32, 51, 71, 78, 93]).map_err(|_| "no answer (split)"));
    let _ = server.kill(); // before any assertion: never leave it running
    let _ = server.wait();
    let got = first.unwrap();
    assert_eq!(split.unwrap(), got, "the same answers when the requests arrive in pieces");
    let bodies: Vec<&str> = got.split("\r\n\r\n").skip(1).map(|s| s.split("HTTP/1.1").next().unwrap()).collect();
    assert_eq!(bodies, ["/a:", "/b:xyz", "/c:"], "{got}");
    assert_eq!(got.matches("Connection: close").count(), 1, "{got}");
    std::fs::remove_dir_all(&dir).unwrap();
}

/// `mimic test`: `test "…" { }` blocks (in any file, and src/test.mim) run
/// each in a process of its own; failures show both sides of a comparison,
/// a panic fails only its test, words filter by name, builds leave tests out.
#[test]
fn test_command() {
    let dir = std::env::temp_dir().join(format!("mimic-golden-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    assert!(mimic().current_dir(&dir).args(["create", "tt"]).status().unwrap().success());
    let proj = dir.join("tt");
    std::fs::write(
        proj.join("src/main.mim"),
        "module main\n\npub fn square(x: i64) -> i64 { x * x }\n\nfn main() { print(square(3)) }\n\n\
         test \"square\" {\n    expect(square(3) == 9)\n    expect(⍳ 4 ¨ square ▷ +/ == 14)\n}\n\n\
         test \"wrong\" {\n    let test = 2\n    expect(square(test) == 5)\n    expect([1 2] == [1 3])\n}\n\n\
         test \"crash\" { print([1 2][5]) }\n",
    )
    .unwrap();
    std::fs::write(proj.join("src/test.mim"), "module test\n\ntest \"from test.mim\" { expect(main::square(4) == 16) }\n").unwrap();
    let out = mimic().current_dir(&proj).arg("test").output().unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert_eq!(out.status.code(), Some(1), "{text}");
    for want in [
        "  ✓ square",
        "  ✗ wrong",
        "line 14: expect(square(test) == 5)",
        "left:  4",
        "right: 5",
        "right: [1 3]",
        "  ✗ crash",
        "index 5 out of bounds",
        "  ✓ test › from test.mim",
        "2 passed, 2 failed",
    ] {
        assert!(text.contains(want), "missing {want:?} in:\n{text}");
    }
    let only = mimic().current_dir(&proj).args(["test", "square"]).output().unwrap();
    assert!(only.status.success() && String::from_utf8_lossy(&only.stdout).contains("1 passed, 0 failed"), "{}", String::from_utf8_lossy(&only.stdout));
    let run = mimic().current_dir(&proj).arg("run").output().unwrap();
    assert_eq!(String::from_utf8_lossy(&run.stdout), "9\n");
    std::fs::remove_dir_all(&dir).unwrap();
}

/// `mimic fmt`: words by default, glyphs on request, indentation and
/// whitespace; `--check` changes nothing and reports.
#[test]
fn fmt_command() {
    let dir = std::env::temp_dir().join(format!("mimic-golden-fmt-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let f = dir.join("f.mim");
    std::fs::write(&f, "module main\n\nfn main() {\n  // sum of squares\nprint(⍳ 4 ¨ (x ⇒ x × x) ▷ +/)   \n\n\n}\n").unwrap();
    let check = mimic().args(["fmt", "--check"]).arg(&f).output().unwrap();
    assert_eq!(check.status.code(), Some(1));
    assert!(std::fs::read_to_string(&f).unwrap().contains("⍳"), "--check changes nothing");
    assert!(mimic().arg("fmt").arg(&f).status().unwrap().success());
    assert_eq!(std::fs::read_to_string(&f).unwrap(), "module main\n\nfn main() {\n    // sum of squares\n    print(iota 4 each (x => x * x) |> sum)\n}\n");
    assert!(mimic().args(["fmt", "--glyphs"]).arg(&f).status().unwrap().success());
    assert_eq!(std::fs::read_to_string(&f).unwrap(), "module main\n\nfn main() {\n    // sum of squares\n    ⎕(⍳ 4 ¨ (x ⇒ x × x) ▷ +/)\n}\n");
    let run = mimic().arg("run").arg(&f).output().unwrap();
    assert_eq!(String::from_utf8_lossy(&run.stdout), "14\n");
    std::fs::remove_dir_all(&dir).unwrap();
}

/// The signal that ended a process (Unix; never on Windows).
fn signal_of(s: std::process::ExitStatus) -> Option<i32> {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        s.signal()
    }
    #[cfg(not(unix))]
    {
        let _ = s;
        None
    }
}

/// `mimic repl` from a pipe: values, `let`s, definitions over several lines,
/// errors that are not kept, and the session as a program.
#[test]
fn repl_session() {
    use std::io::Write;
    let mut child = mimic().arg("repl").stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).spawn().unwrap();
    let input = "⍳ 10 ▷ +/\nlet xs = [3 1 4]\nxs × 2\nfn sq(x: i64) -> i64 {\n    x * x\n}\nxs ¨ sq\nnope(1)\nprint(\"hi\")\n\\i 3\n:show\n:reset\n:show\n";
    child.stdin.take().unwrap().write_all(input.as_bytes()).unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    let want = "45\n[6 2 8]\n[9 1 16]\nerror[E0303]: cannot find `nope` in this scope\nhi\n[0 1 2]\nmodule main\n\nfn sq(x: i64) -> i64 {\n    x * x\n}\n\nfn main() {\n    let xs = [3 1 4]\n}\n(the session is empty)\nmodule main\n\nfn main() {\n}\n";
    assert_eq!(text, want);
}

/// `mimic profile`: the samples name the program's functions, the busy one
/// first, with where the compiler runs it; threads, GPU and memory are there.
#[test]
fn profile_names_functions() {
    let dir = std::env::temp_dir().join(format!("mimic-golden-profile-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let f = dir.join("busy.mim");
    // a long sequential chain: one function holds nearly all the time
    std::fs::write(
        &f,
        "module main\n\nfn spin(n: i64, acc: i64) -> i64 {\n    if n == 0 { acc } else { spin(n - 1, (acc * 31 + n) % 1000003) }\n}\n\nfn main() {\n    print(spin(50000000, 1) > 0)\n}\n",
    )
    .unwrap();
    let out = mimic().args(["profile", "--json"]).arg(&f).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(v["samples"].as_u64().unwrap() >= 20, "{v}");
    let top = &v["functions"][0];
    assert!(top["function"].as_str().unwrap().ends_with("spin") || top["function"] == "main::main", "{v}");
    assert!(top["self"].as_f64().unwrap() > 0.5, "{v}");
    assert!(!v["threads"].as_array().unwrap().is_empty());
    assert!(v["memory"]["allocations"].is_number() && v["gpu"]["kernels"].is_number());
    // the program's own output goes to stderr with --json
    assert!(String::from_utf8_lossy(&out.stderr).contains("true"));
    // a program that never ends (a server): `--for` stops it and still profiles it
    let g = dir.join("forever.mim");
    std::fs::write(&g, "module main\n\nfn tick(n: i64) -> i64 {\n    time::sleep(20ms)\n    tick(n + 1)\n}\n\nfn main() {\n    print(\"started\")\n    print(tick(0))\n}\n").unwrap();
    let t0 = std::time::Instant::now();
    let out = mimic().args(["profile", "--json", "--for", "1"]).arg(&g).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(t0.elapsed().as_secs() < 60, "stopped after about a second");
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(v["wall_seconds"].as_f64().unwrap() >= 0.9, "{v}");
    assert!(String::from_utf8_lossy(&out.stderr).contains("started"), "the program's output is kept");
    std::fs::remove_dir_all(&dir).unwrap();
}

/// `mimic net show` and `mimic net trace`: a program's nets as graphs, and
/// a call reduced round by round with the runtime's result.
#[test]
fn net_show_and_trace() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let out = mimic().args(["net", "show", "--json"]).arg(dir.join("bench/fib/main.mim")).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let f = &v["functions"][0];
    assert_eq!(f["function"], "main::fib");
    assert_eq!(f["entered"], true);
    let g = &f["defs"][0]["graph"];
    assert!(g["nodes"].as_array().unwrap().len() > 5 && g["root"].is_number(), "{g}");
    // a call, traced
    let out = mimic().args(["net", "trace", "--json", "--frames", "20"]).arg(dir.join("bench/fib/main.mim")).args(["--fn", "fib", "15"]).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let t: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(t["result"], "610");
    assert!(t["widest"].as_u64().unwrap() > 50, "fib's calls run side by side: {}", t["widest"]);
    assert_eq!(t["frames"].as_array().unwrap().len(), 20);
    // a .mnet file's @main, as the runtime computes it
    let out = mimic().args(["net", "trace"]).arg(dir.join("tests/nets/church.mnet")).output().unwrap();
    assert!(String::from_utf8_lossy(&out.stdout).starts_with("@main = 9"), "{}", String::from_utf8_lossy(&out.stdout));
}

/// `mimic lsp`, finding one's way: signature help, rename (the program's
/// own names, not the standard library's), the call hierarchy, smart
/// selection, inferred types as inlay hints and as a code action.
#[test]
fn language_server_navigation() {
    use std::io::{BufRead, BufReader, Read, Write};
    let dir = std::env::temp_dir().join(format!("mimic-golden-lsp-nav-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let f = dir.join("nav.mim");
    let text = "module main\n\n// adds one\nfn inc(x: i64) -> i64 { x + 1 }\n\nfn twice(x: i64) -> i64 { inc(inc(x)) }\n\nfn main() {\n    let a = twice(3)\n    let xs = [1 2 3]\n    print(a + inc(1))\n}\n";
    std::fs::write(&f, text).unwrap();
    let uri = file_uri(&f);
    let mut child = mimic().arg("lsp").stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).spawn().unwrap();
    let mut input = child.stdin.take().unwrap();
    let mut output = BufReader::new(child.stdout.take().unwrap());
    let (child, done) = watchdog(child, 180);
    let mut send = |v: serde_json::Value| {
        let b = v.to_string();
        write!(input, "Content-Length: {}\r\n\r\n{b}", b.len()).unwrap();
        input.flush().unwrap();
    };
    let mut read = |id: i64| -> serde_json::Value {
        loop {
            let mut len = 0;
            loop {
                let mut line = String::new();
                output.read_line(&mut line).unwrap();
                if line.trim().is_empty() {
                    break;
                }
                if let Some(n) = line.to_lowercase().strip_prefix("content-length:") {
                    len = n.trim().parse().unwrap();
                }
            }
            let mut body = vec![0; len];
            output.read_exact(&mut body).unwrap();
            let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
            if v["id"] == id {
                return if v.get("error").is_some() { serde_json::json!({ "error": v["error"] }) } else { v["result"].clone() };
            }
        }
    };
    let pos = |line: u32, ch: u32| serde_json::json!({ "textDocument": { "uri": uri }, "position": { "line": line, "character": ch } });
    let mut id = 0;
    // a request (its answer), or a notification (`tell`: no answer)
    let mut rpc = |method: &str, params: serde_json::Value, tell: bool| {
        if tell {
            send(serde_json::json!({ "jsonrpc": "2.0", "method": method, "params": params }));
            return serde_json::Value::Null;
        }
        id += 1;
        send(serde_json::json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }));
        read(id)
    };

    let caps = rpc("initialize", serde_json::json!({ "processId": null, "capabilities": {} }), false);
    assert!(caps["capabilities"]["renameProvider"]["prepareProvider"] == true && caps["capabilities"]["callHierarchyProvider"] == true, "{caps}");
    rpc("initialized", serde_json::json!({}), true);
    rpc("textDocument/didOpen", serde_json::json!({ "textDocument": { "uri": uri, "languageId": "mimic", "version": 1, "text": text } }), true);
    // signature help inside `inc(` on line 10
    let h = rpc("textDocument/signatureHelp", pos(10, 18), false);
    assert_eq!(h["signatures"][0]["label"], "fn inc(x: i64) -> i64", "{h}");
    assert_eq!(h["activeParameter"], 0);
    assert_eq!(h["signatures"][0]["parameters"][0]["label"], serde_json::json!([7, 13]), "{h}");
    // rename: inc everywhere; print is the standard library's
    let p = rpc("textDocument/prepareRename", pos(10, 15), false);
    assert_eq!(p["placeholder"], "inc", "{p}");
    assert!(rpc("textDocument/prepareRename", pos(10, 5), false).is_null(), "print is not renamed");
    let mut rp = pos(10, 15);
    rp["newName"] = "bump".into();
    let e = rpc("textDocument/rename", rp, false);
    let edits = e["changes"][&uri].as_array().unwrap_or_else(|| panic!("{e}"));
    assert_eq!(edits.len(), 4, "the definition and three calls: {e}");
    assert!(edits.iter().all(|x| x["newText"] == "bump"));
    // the call hierarchy of inc: called from twice (twice) and main; twice calls inc
    let items = rpc("textDocument/prepareCallHierarchy", pos(3, 4), false);
    assert_eq!(items[0]["name"], "inc", "{items}");
    let inc = items[0].clone();
    let incoming = rpc("callHierarchy/incomingCalls", serde_json::json!({ "item": inc }), false);
    let mut callers: Vec<(String, usize)> = incoming.as_array().unwrap().iter().map(|c| (c["from"]["name"].as_str().unwrap().to_string(), c["fromRanges"].as_array().unwrap().len())).collect();
    callers.sort();
    assert_eq!(callers, vec![("main".to_string(), 1), ("twice".to_string(), 2)], "{incoming}");
    let twice = rpc("textDocument/prepareCallHierarchy", pos(5, 4), false)[0].clone();
    let outgoing = rpc("callHierarchy/outgoingCalls", serde_json::json!({ "item": twice }), false);
    assert_eq!(outgoing[0]["to"]["name"], "inc", "{outgoing}");
    assert_eq!(outgoing[0]["fromRanges"].as_array().unwrap().len(), 2, "{outgoing}");
    // smart selection: x, inc(x)'s brackets, … the whole file
    let s = rpc("textDocument/selectionRange", serde_json::json!({ "textDocument": { "uri": uri }, "positions": [{ "line": 5, "character": 34 }] }), false);
    let mut depth = 0;
    let mut r = &s[0];
    while r.is_object() {
        depth += 1;
        r = &r["parent"];
    }
    assert!(depth >= 4, "{s}");
    // inferred types: hints after the names, and a code action that writes one in
    let hints = rpc("textDocument/inlayHint", serde_json::json!({ "textDocument": { "uri": uri }, "range": { "start": { "line": 0, "character": 0 }, "end": { "line": 12, "character": 0 } } }), false);
    let labels: Vec<String> = hints.as_array().unwrap().iter().filter(|h| h["kind"] == 1).map(|h| format!("{}:{}", h["position"]["line"], h["label"].as_str().unwrap())).collect();
    assert!(labels.contains(&"8:: i64".to_string()) && labels.contains(&"9:: [i64]".to_string()), "{labels:?}");
    let acts = rpc("textDocument/codeAction", serde_json::json!({ "textDocument": { "uri": uri }, "range": { "start": { "line": 8, "character": 0 }, "end": { "line": 8, "character": 0 } }, "context": { "diagnostics": [] } }), false);
    assert!(acts.as_array().unwrap().iter().any(|a| a["title"] == "Write the type of `a`: i64"), "{acts}");
    // a selected expression into a function: `a + inc(1)` uses `a` from main
    let ex = rpc("textDocument/codeAction", serde_json::json!({ "textDocument": { "uri": uri }, "range": { "start": { "line": 10, "character": 10 }, "end": { "line": 10, "character": 20 } }, "context": { "diagnostics": [] } }), false);
    let ex = ex.as_array().unwrap().iter().find(|a| a["title"].as_str().is_some_and(|t| t.starts_with("Extract into a function"))).unwrap_or_else(|| panic!("{ex}")).clone();
    let edits = ex["edit"]["changes"][&uri].as_array().unwrap().clone();
    assert_eq!(edits[0]["newText"], "extracted(a)", "{ex}");
    assert_eq!(edits[1]["newText"], "\n\nfn extracted(a) {\n    a + inc(1)\n}", "{ex}");
    rpc("shutdown", serde_json::Value::Null, false);
    rpc("exit", serde_json::Value::Null, true);
    done.store(true, std::sync::atomic::Ordering::SeqCst);
    assert!(child.lock().unwrap().wait().unwrap().success());
}

/// `mimic test --coverage`: each line of the program's code the tests ran,
/// and how often; lines of functions nobody called count as not run.
#[test]
fn test_coverage() {
    let dir = std::env::temp_dir().join(format!("mimic-golden-coverage-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let f = dir.join("cov.mim");
    std::fs::write(
        &f,
        "module main\n\nfn classify(x: i64) -> str {\n    if x < 0 {\n        \"negative\"\n    } else if x == 0 {\n        \"zero\"\n    } else {\n        \"positive\"\n    }\n}\n\nfn never(x: i64) -> i64 {\n    let y = x * 2\n    y + 1\n}\n\nfn main() { print(classify(1)) }\n\ntest \"positive\" {\n    expect(classify(5) == \"positive\")\n}\n\ntest \"zero\" {\n    expect(classify(0) == \"zero\")\n}\n",
    )
    .unwrap();
    let out = mimic().args(["test", "--coverage"]).arg(&f).output().unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{text}{}", String::from_utf8_lossy(&out.stderr));
    assert!(text.contains("(3 of 7 lines); not run: 5, 14, 15, 18"), "{text}");
    let err = String::from_utf8_lossy(&out.stderr);
    let saved = err.lines().find_map(|l| l.trim().strip_prefix("Saved ")).unwrap_or_else(|| panic!("{err}"));
    let json = saved.trim_end_matches(" and lcov.info");
    let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(json).unwrap()).unwrap();
    assert_eq!(v["files"][0]["lines"], serde_json::json!([[4, 2], [5, 0], [7, 1], [9, 1], [14, 0], [15, 0], [18, 0]]), "{v}");
    let lcov = std::fs::read_to_string(std::path::Path::new(json).with_file_name("lcov.info")).unwrap();
    assert!(lcov.contains("DA:4,2") && lcov.contains("LF:7"), "{lcov}");
    std::fs::remove_dir_all(&dir).unwrap();
}
