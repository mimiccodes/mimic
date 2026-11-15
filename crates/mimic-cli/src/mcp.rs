//! `mimic mcp`: Mimic for AI agents, over the Model Context Protocol
//! (newline-delimited JSON-RPC on stdin and stdout).
//!
//! Tools check, build, run, test and format code, explain errors, look up
//! the manual and the program's symbols; resources serve the manual, the
//! glyph table and the guide for agents; prompts start common jobs.
//!
//! **Access.** Inside the project the agent may do everything: read and
//! write files, check, build, run, test, format. Outside it, nothing,
//! until the user turns it on: `--allow-outside` (files elsewhere),
//! `--allow-install` (libraries into ~/.mimic), `--allow-network`
//! (running programs that use `http`, `tcp`, `udp`, `ws` or `tls`, or run
//! other programs with `os::run` or `os::shell`; C code is not checked).
//! `--read-only` and `--no-run` narrow it further.

use crate::engine::Engine;
use crate::man;
use serde_json::{Value, json};
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const PROTOCOL: &str = "2025-06-18";
/// What can reach the network: Mimic's network modules, and other programs
/// (which may do anything). C code is not seen by this check.
const NETWORK: [&str; 7] = ["http::", "tcp::", "udp::", "ws::", "tls::", "os::run", "os::shell"];

#[derive(Clone, Copy, Default)]
pub struct Access {
    pub allow_outside: bool,
    pub allow_install: bool,
    pub allow_network: bool,
    pub read_only: bool,
    pub no_run: bool,
}

impl Access {
    fn describe(&self) -> String {
        let on = |b: bool| if b { "on" } else { "off" };
        format!(
            "Inside the project: reading{}{}{}. Outside it: files {}, installing libraries {}, programs using the network {}.",
            if self.read_only { "" } else { ", writing files, formatting" },
            if self.read_only { "" } else { ", building" },
            if self.read_only || self.no_run { "" } else { ", running and testing" },
            on(self.allow_outside),
            on(self.allow_install),
            on(self.allow_network)
        )
    }
}

struct Server {
    root: PathBuf,
    access: Access,
    engine: Option<Engine>,
    exe: PathBuf,
}

pub fn serve(project: Option<PathBuf>, access: Access) -> Result<(), String> {
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    let start = project.unwrap_or(cwd);
    let root = mimic_project::find_root(&start).unwrap_or(start);
    let root = root.canonicalize().unwrap_or(root);
    let engine = Engine::for_project(&root).ok();
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut s = Server { root, access, engine, exe };
    let stdin = std::io::stdin();
    let mut out = std::io::stdout();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let Ok(msg) = serde_json::from_str::<Value>(&line) else {
            let e = json!({ "jsonrpc": "2.0", "id": null, "error": { "code": -32700, "message": "parse error" } });
            let _ = writeln!(out, "{e}");
            continue;
        };
        // notifications get no answer
        let Some(id) = msg.get("id").cloned() else { continue };
        let method = msg["method"].as_str().unwrap_or("");
        let params = msg.get("params").cloned().unwrap_or(json!({}));
        let reply = match s.handle(method, &params) {
            Ok(r) => json!({ "jsonrpc": "2.0", "id": id, "result": r }),
            Err((code, m)) => json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": m } }),
        };
        let _ = writeln!(out, "{reply}");
        let _ = out.flush();
    }
    Ok(())
}

fn text(t: impl Into<String>) -> Value {
    json!({ "content": [{ "type": "text", "text": t.into() }], "isError": false })
}

fn failure(t: impl Into<String>) -> Value {
    json!({ "content": [{ "type": "text", "text": t.into() }], "isError": true })
}

fn tool(name: &str, description: &str, props: Value, required: &[&str], read_only: bool) -> Value {
    json!({
        "name": name,
        "description": description,
        "inputSchema": { "type": "object", "properties": props, "required": required },
        "annotations": { "readOnlyHint": read_only, "openWorldHint": false },
    })
}

impl Server {
    fn handle(&mut self, method: &str, p: &Value) -> Result<Value, (i64, String)> {
        match method {
            "initialize" => {
                let asked = p["protocolVersion"].as_str().unwrap_or(PROTOCOL);
                let version = if ["2025-06-18", "2025-03-26", "2024-11-05"].contains(&asked) { asked } else { PROTOCOL };
                Ok(json!({
                    "protocolVersion": version,
                    "capabilities": { "tools": {}, "resources": {}, "prompts": {} },
                    "serverInfo": { "name": "mimic", "version": env!("CARGO_PKG_VERSION") },
                    "instructions": format!(
                        "Mimic tools for the project at {}. Before writing Mimic, read the resource mimic://manual/agents (the rules and the usual mistakes), and look things up with the manual tool rather than guessing. After each edit, call check; it gives every error with its code and, often, an exact fix. {}",
                        self.root.display(),
                        self.access.describe()
                    ),
                }))
            }
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({ "tools": self.tools() })),
            "tools/call" => {
                let name = p["name"].as_str().unwrap_or("");
                let args = p.get("arguments").cloned().unwrap_or(json!({}));
                Ok(match self.call(name, &args) {
                    Ok(v) => v,
                    Err(e) => failure(e),
                })
            }
            "resources/list" => Ok(json!({ "resources": self.resources() })),
            "resources/read" => {
                let uri = p["uri"].as_str().unwrap_or("");
                let t = self.read_resource(uri).map_err(|e| (-32002, e))?;
                Ok(json!({ "contents": [{ "uri": uri, "mimeType": "text/markdown", "text": t }] }))
            }
            "resources/templates/list" => Ok(json!({ "resourceTemplates": [
                { "uriTemplate": "mimic://manual/{page}", "name": "A page of the manual", "description": "A chapter, guide, function (fs::read), module, glyph (keep) or error code (E0303)", "mimeType": "text/markdown" }
            ] })),
            "prompts/list" => Ok(json!({ "prompts": prompts() })),
            "prompts/get" => {
                let name = p["name"].as_str().unwrap_or("");
                let args = p.get("arguments").cloned().unwrap_or(json!({}));
                prompt(name, &args).map_err(|e| (-32602, e))
            }
            _ => Err((-32601, format!("unknown method {method}"))),
        }
    }

    // ---- tools --------------------------------------------------------------

    fn tools(&self) -> Vec<Value> {
        let file = json!({ "type": "string", "description": "A .mim file (relative to the project, or absolute); omit for the whole project" });
        let mut t = vec![
            tool("check", "Check the project (or one standalone file) for errors without building. Returns every diagnostic with its code, message, location and suggested fixes (exact edits), as JSON.", json!({ "file": file }), &[], true),
            tool("explain", "Explain an error code: what it means, an example, the usual fix.", json!({ "code": { "type": "string", "description": "E0303, or its name (unresolved-name)" } }), &["code"], true),
            tool("manual", "A page of the Mimic manual, as Markdown: a chapter (\"no-loops\", \"4\"), a guide (\"agents\", \"coming-from-python\"), a standard function (\"fs::read\", \"sort_by\"), a module (\"json\"), a glyph or its word (\"⌿\", \"keep\"), an error code (\"E0303\"). Empty for the contents.", json!({ "topic": { "type": "string" } }), &[], true),
            tool("manual_search", "Every line of the manual that mentions a text, with its page and section.", json!({ "text": { "type": "string" } }), &["text"], true),
            tool("symbols", "Find functions, types, variants and modules of the project and the standard library by name: signature, comment, location.", json!({ "query": { "type": "string" }, "kind": { "type": "string", "enum": ["fn", "extern fn", "type", "variant", "module"] }, "package": { "type": "string", "description": "\"std\" for the standard library, or the project's name" } }), &["query"], true),
            tool("symbol", "Everything about one symbol (fs::read, geometry::area): signature, comment, effects (pure, concurrent, ordered, sync), where it is defined and every place it is used.", json!({ "name": { "type": "string" } }), &["name"], true),
            tool("effects", "What each function of the project does besides computing a result: pure (runs in parallel), concurrent, ordered or sync.", json!({}), &[], true),
            tool("targets", "The platforms Mimic builds for, by how well they are tested (stable, runs, links, compiles).", json!({}), &[], true),
            tool("docs_status", "The documentation of the project: public items with no `//` comment above them, and the guides in docs/ (Markdown pages) with how many runnable examples each has.", json!({}), &[], true),
            tool("read_file", "Read a file of the project.", json!({ "path": { "type": "string" } }), &["path"], true),
            tool("list_files", "The .mim files and project files of the project.", json!({}), &[], true),
        ];
        if !self.access.read_only {
            t.push(tool("write_file", "Write a file of the project (creating folders as needed). Check after writing Mimic.", json!({ "path": { "type": "string" }, "text": { "type": "string" } }), &["path", "text"], false));
            t.push(tool("format", "Format a .mim file in place (mimic fmt): words for primitives by default, glyphs with glyphs: true.", json!({ "file": { "type": "string" }, "glyphs": { "type": "boolean" } }), &["file"], false));
            t.push(tool("docs_write", "Write a guide: a Markdown page docs/<name>.md, shown by mimic doc beside the API. Put complete programs in ```mimic run blocks, each followed by an ```output block with exactly what it prints: they run as tests, and this tool runs them right away and reports any that differ. Signatures and types come from the code; guides explain how and why.", json!({ "name": { "type": "string", "description": "e.g. getting-started, or api/routes" }, "markdown": { "type": "string" } }), &["name", "markdown"], false));
            t.push(tool("docs_build", "Build the documentation site (mimic doc): guides and API on one page, at target/doc/index.html.", json!({}), &[], false));
            t.push(tool("build", "Build the project (or a standalone file): a release build by default. Returns the compiler's output and the artifact.", json!({ "file": file, "debug": { "type": "boolean" }, "target": { "type": "string", "description": "An LLVM triple, e.g. x86_64-linux-musl" } }), &[], false));
        }
        if !self.access.read_only && !self.access.no_run {
            t.push(tool("run", "Build and run the project (or a standalone file with module main); returns its output and exit status. Stops it after timeout_s seconds (default 30).", json!({ "file": file, "args": { "type": "array", "items": { "type": "string" } }, "stdin": { "type": "string" }, "release": { "type": "boolean" }, "timeout_s": { "type": "integer" } }), &[], false));
            t.push(tool("test", "Run the tests (test blocks and examples in comments) of the project or a file; words filter by name.", json!({ "file": file, "filter": { "type": "array", "items": { "type": "string" } } }), &[], false));
        }
        if self.access.allow_install && !self.access.read_only {
            t.push(tool("install", "Install the project's git dependencies (no url), or one library from git (url, optionally @tag).", json!({ "url": { "type": "string" } }), &[], false));
        }
        t
    }

    /// A path the agent named: inside the project unless `--allow-outside`.
    fn path(&self, p: &str) -> Result<PathBuf, String> {
        let raw = Path::new(p);
        let abs = if raw.is_absolute() { raw.to_path_buf() } else { self.root.join(raw) };
        // resolve `..` and links through the deepest folder that exists
        let mut existing = abs.clone();
        let mut rest = Vec::new();
        while !existing.exists() {
            match (existing.file_name(), existing.parent()) {
                (Some(n), Some(parent)) => {
                    rest.push(n.to_os_string());
                    existing = parent.to_path_buf();
                }
                _ => break,
            }
        }
        let mut real = existing.canonicalize().map_err(|e| format!("{p}: {e}"))?;
        for n in rest.iter().rev() {
            if n == ".." || n == "." {
                return Err(format!("{p}: write the path without `..`"));
            }
            real.push(n);
        }
        if !self.access.allow_outside && !real.starts_with(&self.root) {
            return Err(format!("{p} is outside the project ({}); the user can allow that with `mimic mcp --allow-outside`", self.root.display()));
        }
        Ok(real)
    }

    /// The project's engine, checked again: the agent may have changed any
    /// file since the last tool call (only changed files are parsed again).
    fn engine(&mut self) -> Result<&mut Engine, String> {
        if self.engine.is_none() {
            self.engine = Some(Engine::for_project(&self.root).map_err(|e| format!("no Mimic project at {}: {e}", self.root.display()))?);
        }
        let e = self.engine.as_mut().unwrap();
        e.check();
        Ok(e)
    }

    /// Run this `mimic` with `args` in the project: (status, stdout, stderr).
    fn mimic(&self, args: &[String], stdin: Option<&str>, timeout: Duration) -> Result<(Option<i32>, String, String, bool), String> {
        let mut child = Command::new(&self.exe)
            .args(args)
            .current_dir(&self.root)
            .env("NO_COLOR", "1")
            .stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| e.to_string())?;
        if let (Some(text), Some(mut w)) = (stdin, child.stdin.take()) {
            let text = text.to_string();
            std::thread::spawn(move || {
                let _ = w.write_all(text.as_bytes());
            });
        }
        let read = |r: Option<Box<dyn std::io::Read + Send>>| {
            std::thread::spawn(move || {
                let mut s = String::new();
                if let Some(mut r) = r {
                    let mut buf = Vec::new();
                    let _ = r.read_to_end(&mut buf);
                    s = String::from_utf8_lossy(&buf).to_string();
                }
                s
            })
        };
        let out = read(child.stdout.take().map(|r| Box::new(r) as Box<dyn std::io::Read + Send>));
        let err = read(child.stderr.take().map(|r| Box::new(r) as Box<dyn std::io::Read + Send>));
        let start = Instant::now();
        let mut timed_out = false;
        let status = loop {
            if let Some(st) = child.try_wait().map_err(|e| e.to_string())? {
                break st.code();
            }
            if start.elapsed() > timeout {
                let _ = child.kill();
                let _ = child.wait();
                timed_out = true;
                break None;
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        let cap = |s: String| if s.len() > 200_000 { format!("{}\n… ({} bytes in all)", &s[..s.floor_char_boundary(200_000)], s.len()) } else { s };
        Ok((status, cap(out.join().unwrap_or_default()), cap(err.join().unwrap_or_default()), timed_out))
    }

    fn file_arg(&self, a: &Value) -> Result<Option<String>, String> {
        match a.get("file").and_then(Value::as_str) {
            Some(f) if !f.is_empty() => Ok(Some(self.path(f)?.display().to_string())),
            _ => Ok(None),
        }
    }

    fn call(&mut self, name: &str, a: &Value) -> Result<Value, String> {
        let s = |k: &str| a.get(k).and_then(Value::as_str).unwrap_or("").to_string();
        let writes = ["write_file", "format", "build", "run", "test", "install", "docs_write", "docs_build"];
        if self.access.read_only && writes.contains(&name) {
            return Err(format!("`{name}` is off: this server is read-only (mimic mcp --read-only)"));
        }
        if self.access.no_run && ["run", "test"].contains(&name) {
            return Err(format!("`{name}` is off: the user started this server with --no-run"));
        }
        match name {
            "check" => {
                let v = match self.file_arg(a)? {
                    Some(f) => {
                        let mut e = Engine::for_file(Path::new(&f));
                        e.query("check", &json!({}))?
                    }
                    None => self.engine()?.query("check", &json!({}))?,
                };
                let ok = v["success"] == true;
                let mut v = v;
                if let Some(o) = v.as_object_mut() {
                    o.retain(|k, _| ["success", "diagnostics", "summary", "files", "functions"].contains(&k.as_str()));
                }
                // the human rendering is long: keep the facts
                if let Some(ds) = v["diagnostics"].as_array_mut() {
                    for d in ds {
                        if let Some(o) = d.as_object_mut() {
                            o.remove("rendered");
                        }
                    }
                }
                let t = serde_json::to_string_pretty(&v).unwrap_or_default();
                Ok(if ok { text(t) } else { json!({ "content": [{ "type": "text", "text": t }], "isError": false }) })
            }
            "explain" => {
                let c = mimic_syntax::codes::find(&s("code")).ok_or_else(|| format!("no error code `{}`", s("code")))?;
                Ok(text(format!("{} ({}): {}\n\n{}", c.id, c.name, c.title, c.explain)))
            }
            "manual" => {
                let m = man::Manual::load();
                let topic = s("topic");
                if topic.is_empty() {
                    return Ok(text(m.index()));
                }
                match m.find(&topic) {
                    Ok(p) => Ok(text(p.text.clone())),
                    Err(near) => Err(format!(
                        "no page `{topic}`{}",
                        if near.is_empty() { String::new() } else { format!("; nearby: {}", near.iter().map(|p| p.name.as_str()).collect::<Vec<_>>().join(", ")) }
                    )),
                }
            }
            "manual_search" => {
                let m = man::Manual::load();
                let hits: Vec<Value> = m.search(&s("text")).into_iter().take(80).map(|(p, h, l)| json!({ "page": p.name, "section": h, "line": l })).collect();
                Ok(text(serde_json::to_string_pretty(&hits).unwrap_or_default()))
            }
            "symbols" => {
                let mut q = json!({ "query": s("query"), "limit": 50 });
                for k in ["kind", "package"] {
                    if let Some(v) = a.get(k) {
                        q[k] = v.clone();
                    }
                }
                let v = self.engine()?.query("symbols", &q)?;
                Ok(text(serde_json::to_string_pretty(&v).unwrap_or_default()))
            }
            "symbol" => {
                let e = self.engine()?;
                let mut v = e.query("docs", &json!({ "name": s("name") }))?;
                v["used_at"] = e.query("references", &json!({ "name": s("name") }))?;
                Ok(text(serde_json::to_string_pretty(&v).unwrap_or_default()))
            }
            "effects" => {
                let v = self.engine()?.query("effects", &json!({}))?;
                Ok(text(serde_json::to_string_pretty(&v).unwrap_or_default()))
            }
            "targets" => {
                let (_, out, err, _) = self.mimic(&["targets".into()], None, Duration::from_secs(60))?;
                let tiers = out.split("Targets by tier").nth(1).map(|t| format!("Targets by tier{t}")).unwrap_or(out + &err);
                Ok(text(tiers))
            }
            "docs_status" => {
                let (st, out, err, _) = self.mimic(&["doc".into(), "--json".into()], None, Duration::from_secs(120))?;
                if st != Some(0) {
                    return Err(format!("mimic doc failed:\n{err}"));
                }
                let v: Value = serde_json::from_str(&out).map_err(|e| e.to_string())?;
                let mut missing = Vec::new();
                for m in v["modules"].as_array().into_iter().flatten() {
                    if m["doc"].as_str().unwrap_or("").is_empty() && m["path"].as_str().is_some_and(|p| p.contains("::")) {
                        missing.push(json!({ "module": m["path"] }));
                    }
                    for it in m["items"].as_array().into_iter().flatten() {
                        if it["kind"] != "group" && it["doc"].as_str().unwrap_or("").is_empty() {
                            missing.push(json!({ "item": format!("{}::{}", m["path"].as_str().unwrap_or(""), it["name"].as_str().unwrap_or("")), "signature": it["signature"] }));
                        }
                    }
                }
                let guides: Vec<Value> = v["guides"].as_array().into_iter().flatten().map(|g| json!({ "name": g["name"], "title": g["title"], "examples": g["examples"] })).collect();
                let body = json!({ "undocumented": missing, "guides": guides, "how": "Document an item with `//` lines right above it (edit the source with write_file); write guides with docs_write; then docs_build." });
                Ok(text(serde_json::to_string_pretty(&body).unwrap_or_default()))
            }
            "docs_write" => {
                let name = s("name").trim_end_matches(".md").trim_start_matches("docs/").to_string();
                if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '/') || name.starts_with('/') || name.contains("//") {
                    return Err("name: letters, digits, `-`, `_` and `/` only, such as getting-started or api/routes".into());
                }
                let p = self.path(&format!("docs/{name}.md"))?;
                if let Some(d) = p.parent() {
                    std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
                }
                let md = s("markdown");
                std::fs::write(&p, &md).map_err(|e| format!("{}: {e}", p.display()))?;
                // run its examples now, as `mimic test` will
                let examples = crate::man::examples_of(&md);
                let mut report = Vec::new();
                let dir = std::env::temp_dir().join(format!("mimic-mcp-doc-{}", std::process::id()));
                let _ = std::fs::create_dir_all(&dir);
                for (i, (program, want)) in examples.iter().enumerate() {
                    let f = dir.join(format!("example{i}.mim"));
                    std::fs::write(&f, program).map_err(|e| e.to_string())?;
                    let (_, out, err, _) = self.mimic(&["run".into(), f.display().to_string()], None, Duration::from_secs(60))?;
                    if out != *want {
                        report.push(json!({ "example": i + 1, "expected": want, "got": out, "errors": err.lines().filter(|l| !l.trim_start().starts_with("Running")).collect::<Vec<_>>().join("\n") }));
                    }
                }
                let _ = std::fs::remove_dir_all(&dir);
                let msg = json!({ "wrote": p.display().to_string(), "examples": examples.len(), "failing": report });
                let t = serde_json::to_string_pretty(&msg).unwrap_or_default();
                Ok(if msg["failing"].as_array().is_some_and(|a| !a.is_empty()) { failure(t) } else { text(t) })
            }
            "docs_build" => {
                let (st, out, err, _) = self.mimic(&["doc".into()], None, Duration::from_secs(300))?;
                Ok(if st == Some(0) { text(format!("{out}{err}")) } else { failure(format!("{out}{err}")) })
            }
            "read_file" => {
                let p = self.path(&s("path"))?;
                let t = std::fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))?;
                Ok(text(t))
            }
            "list_files" => {
                let mut files = Vec::new();
                for name in ["mim.mim", "build.mim", "mod.mim"] {
                    if self.root.join(name).is_file() {
                        files.push(name.to_string());
                    }
                }
                if let Ok(src) = mimic_project::sources_in(&self.root.join("src")) {
                    files.extend(src.iter().map(|f| f.strip_prefix(&self.root).unwrap_or(f).display().to_string()));
                }
                Ok(text(files.join("\n")))
            }
            "write_file" => {
                let p = self.path(&s("path"))?;
                if let Some(d) = p.parent() {
                    std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
                }
                std::fs::write(&p, s("text")).map_err(|e| format!("{}: {e}", p.display()))?;
                Ok(text(format!("wrote {} ({} bytes)", p.display(), s("text").len())))
            }
            "format" => {
                let p = self.path(&s("file"))?;
                let src = std::fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))?;
                let glyphs = a.get("glyphs").and_then(Value::as_bool).unwrap_or(false);
                let out = mimic_syntax::format::format(&src, glyphs).map_err(|d| d.render(&p, &src))?;
                if out != src {
                    std::fs::write(&p, &out).map_err(|e| e.to_string())?;
                }
                Ok(text(if out == src { "already formatted".to_string() } else { format!("formatted {}", p.display()) }))
            }
            "build" => {
                let mut args = vec!["build".to_string(), "--emit-json".into()];
                if let Some(f) = self.file_arg(a)? {
                    args.push(f);
                }
                if a.get("debug").and_then(Value::as_bool) == Some(true) {
                    args.push("--debug".into());
                }
                let t = s("target");
                if !t.is_empty() {
                    args.extend(["--target".into(), t]);
                }
                let (st, out, err, _) = self.mimic(&args, None, Duration::from_secs(900))?;
                let body = format!("{out}{}", if err.trim().is_empty() { String::new() } else { format!("\n{err}") });
                Ok(if st == Some(0) { text(body) } else { failure(body) })
            }
            "run" => {
                if !self.access.allow_network {
                    let uses = match self.file_arg(a)? {
                        Some(f) => Engine::for_file(Path::new(&f)).uses_of(&NETWORK).unwrap_or_default(),
                        None => self.engine()?.uses_of(&NETWORK).unwrap_or_default(),
                    };
                    if !uses.is_empty() {
                        return Err(format!(
                            "this program uses the network or runs other programs ({}); running it is off until the user starts the server with --allow-network. It can still be checked and built.",
                            uses.join(", ")
                        ));
                    }
                }
                let mut args = vec!["run".to_string()];
                if a.get("release").and_then(Value::as_bool) == Some(true) {
                    args.push("--release".into());
                }
                if let Some(f) = self.file_arg(a)? {
                    args.push(f);
                }
                if let Some(xs) = a.get("args").and_then(Value::as_array) {
                    args.push("--".into());
                    args.extend(xs.iter().filter_map(|x| x.as_str().map(String::from)));
                }
                let secs = a.get("timeout_s").and_then(Value::as_u64).unwrap_or(30).clamp(1, 600);
                let stdin = a.get("stdin").and_then(Value::as_str);
                let (st, out, err, timed_out) = self.mimic(&args, stdin, Duration::from_secs(secs))?;
                let mut body = json!({ "exit_status": st, "stdout": out, "stderr": err.lines().filter(|l| !l.trim_start().starts_with("Running") && !l.trim_start().starts_with("Compiling") && !l.trim_start().starts_with("Finished")).collect::<Vec<_>>().join("\n") });
                if timed_out {
                    body["timed_out"] = format!("stopped after {secs} s").into();
                }
                let t = serde_json::to_string_pretty(&body).unwrap_or_default();
                Ok(if st == Some(0) { text(t) } else { failure(t) })
            }
            "test" => {
                let mut args = vec!["test".to_string()];
                if let Some(f) = self.file_arg(a)? {
                    args.push(f);
                }
                if let Some(xs) = a.get("filter").and_then(Value::as_array) {
                    args.extend(xs.iter().filter_map(|x| x.as_str().map(String::from)));
                }
                let (st, out, err, _) = self.mimic(&args, None, Duration::from_secs(600))?;
                let body = format!("{out}{err}");
                Ok(if st == Some(0) { text(body) } else { failure(body) })
            }
            "install" => {
                if !self.access.allow_install {
                    return Err("installing is off: the user can allow it with mimic mcp --allow-install".into());
                }
                let mut args = vec!["install".to_string()];
                let u = s("url");
                if !u.is_empty() {
                    args.push(u);
                }
                let (st, out, err, _) = self.mimic(&args, None, Duration::from_secs(600))?;
                Ok(if st == Some(0) { text(format!("{out}{err}")) } else { failure(format!("{out}{err}")) })
            }
            _ => Err(format!("unknown tool `{name}`")),
        }
    }

    // ---- resources ------------------------------------------------------------

    fn resources(&self) -> Vec<Value> {
        let mut r = vec![
            json!({ "uri": "mimic://manual/agents", "name": "Writing Mimic for agents", "description": "Read first: the workflow, idioms, and the mistakes code from other languages makes, with their error codes.", "mimeType": "text/markdown" }),
            json!({ "uri": "mimic://glyphs", "name": "The glyph table", "description": "Every glyph with its word, how to type it and what it does.", "mimeType": "text/markdown" }),
            json!({ "uri": "mimic://manual/std", "name": "The standard library", "description": "Strings, lists, maps and the modules fs, path, io, os, time, random, json, http, ws, tcp, udp, tls, chan, ui.", "mimeType": "text/markdown" }),
            json!({ "uri": "mimic://manual/syntax", "name": "The syntax", "description": "The language reference.", "mimeType": "text/markdown" }),
        ];
        for p in man::Manual::load().pages.iter().filter(|p| matches!(p.kind, "tutorial" | "guide")) {
            let uri = format!("mimic://manual/{}", p.name);
            if !r.iter().any(|x| x["uri"] == uri.as_str()) {
                r.push(json!({ "uri": uri, "name": p.title, "mimeType": "text/markdown" }));
            }
        }
        r
    }

    fn read_resource(&self, uri: &str) -> Result<String, String> {
        if uri == "mimic://glyphs" {
            let mut t = String::from("# Glyphs\n\n| Glyph | Word | Type it as | Meaning |\n|---|---|---|---|\n");
            for p in mimic_syntax::glyphs::PRIMS {
                t += &format!("| `{}` | `{}` | {} | {} |\n", p.glyph, p.word, p.digraph.map(|d| format!("`{d}`")).unwrap_or_default(), p.doc.replace('|', "\\|"));
            }
            for g in mimic_syntax::glyphs::SYNTAX_GLYPHS {
                t += &format!("| `{}` | `{}` | {} | {} |\n", g.glyph, g.word.replace('|', "\\|"), g.digraph.map(|d| format!("`{d}`")).unwrap_or_default(), g.doc.replace('|', "\\|"));
            }
            return Ok(t);
        }
        let Some(page) = uri.strip_prefix("mimic://manual/") else { return Err(format!("unknown resource {uri}")) };
        let m = man::Manual::load();
        m.find(page).map(|p| p.text.clone()).map_err(|_| format!("no page `{page}` in the manual"))
    }
}

// ---- prompts --------------------------------------------------------------------

fn prompts() -> Vec<Value> {
    vec![
        json!({ "name": "write_mimic", "description": "Write idiomatic Mimic for a task, checked with the tools.", "arguments": [{ "name": "task", "description": "What the program should do", "required": true }] }),
        json!({ "name": "port_to_mimic", "description": "Port code from another language to idiomatic Mimic.", "arguments": [{ "name": "code", "description": "The code to port", "required": true }, { "name": "language", "description": "C, Python, JavaScript, …", "required": false }] }),
        json!({ "name": "fix_errors", "description": "Fix every error mimic check reports in the project.", "arguments": [] }),
        json!({ "name": "document_project", "description": "Document the project: comments on public items, and guides with examples that run as tests.", "arguments": [{ "name": "focus", "description": "A module or topic to start with", "required": false }] }),
    ]
}

fn prompt(name: &str, args: &Value) -> Result<Value, String> {
    let a = |k: &str| args.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    let rules = "First read the resource mimic://manual/agents. Use the manual tool for any function, module or glyph you are not sure of, rather than guessing. Mimic has no loops: use array operations (¨ ⌿ +/ gather trail), evolve, settle or recursion. After every change, call check and fix what it reports; then run or test.";
    let (description, body) = match name {
        "write_mimic" => ("Write idiomatic Mimic", format!("Write a Mimic program for this task:\n\n{}\n\n{rules}", a("task"))),
        "port_to_mimic" => {
            let lang = if a("language").is_empty() { "another language".to_string() } else { a("language") };
            let guide = match a("language").to_lowercase().as_str() {
                "c" | "c++" => "coming-from-c",
                "python" => "coming-from-python",
                "javascript" | "typescript" | "js" | "ts" => "coming-from-javascript",
                _ => "agents",
            };
            ("Port code to Mimic", format!("Port this {lang} code to idiomatic Mimic (not a line-by-line translation: whole-array operations instead of loops, Option and Result instead of null and exceptions). The manual page `{guide}` lists the usual translations.\n\n```\n{}\n```\n\n{rules}", a("code")))
        }
        "document_project" => (
            "Document the project",
            format!(
                "Document this Mimic project{}. Call docs_status to see what has no comment and which guides exist. Add a short `//` comment right above each undocumented public item (what it does, its inputs and result; an example as `// > expression` with the value `print` would show on the next comment line runs as a test). Then write guides with docs_write: an overview of what the project is for, and a guide per main task, each with complete programs in ```mimic run blocks followed by their exact ```output; docs_write runs them and tells you which differ. Do not restate signatures: mimic doc shows them. Finish with docs_build and test. {rules}",
                if a("focus").is_empty() { String::new() } else { format!(", starting with {}", a("focus")) }
            ),
        ),
        "fix_errors" => ("Fix the project's errors", format!("Call check on the project and fix every error it reports, applying its suggested fixes where they fit; explain any you change differently. Repeat until check passes, then run the tests. {rules}")),
        _ => return Err(format!("unknown prompt `{name}`")),
    };
    Ok(json!({ "description": description, "messages": [{ "role": "user", "content": { "type": "text", "text": body } }] }))
}
