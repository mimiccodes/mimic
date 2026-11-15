//! `mimic dap`: debugging from editors, over the Debug Adapter Protocol.
//!
//! It stands in front of LLVM's `lldb-dap` and passes every message
//! through, except `launch`: there it builds the program (a `.mim` file or
//! a project folder) with debug information, as `mimic lldb` does, points
//! `lldb-dap` at the executable, and loads the formatters that show Mimic's
//! values as Mimic prints them (`[3 1 4]`, `Point { x: 1.5, y: 2.0 }`,
//! `["a": 1]`, `Some(7)`). Breakpoints on `.mim` lines, stepping, the
//! call stack and the variables then work in any DAP editor.

use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};

/// Read one DAP message (`Content-Length` header, blank line, JSON body).
fn read_message(r: &mut impl BufRead) -> Option<Vec<u8>> {
    let mut len = None;
    loop {
        let mut line = String::new();
        if r.read_line(&mut line).ok()? == 0 {
            return None;
        }
        let line = line.trim_end();
        if line.is_empty() {
            break;
        }
        if let Some(n) = line.to_ascii_lowercase().strip_prefix("content-length:") {
            len = n.trim().parse::<usize>().ok();
        }
    }
    let mut body = vec![0; len?];
    r.read_exact(&mut body).ok()?;
    Some(body)
}

fn write_message(w: &mut impl Write, body: &[u8]) -> std::io::Result<()> {
    write!(w, "Content-Length: {}\r\n\r\n", body.len())?;
    w.write_all(body)?;
    w.flush()
}

/// LLVM's debug adapter: `MIMIC_LLDB_DAP`, Xcode's, or on `PATH`.
fn find_lldb_dap() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("MIMIC_LLDB_DAP") {
        return Some(PathBuf::from(p));
    }
    if cfg!(target_os = "macos")
        && let Ok(o) = Command::new("xcrun").args(["-f", "lldb-dap"]).output()
        && o.status.success()
    {
        return Some(PathBuf::from(String::from_utf8_lossy(&o.stdout).trim()));
    }
    let names = ["lldb-dap", "lldb-dap-23", "lldb-dap-22", "lldb-dap-21", "lldb-dap-20", "lldb-dap-19", "lldb-dap-18", "lldb-vscode"];
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        for n in names {
            let f = dir.join(format!("{n}{}", std::env::consts::EXE_SUFFIX));
            if f.is_file() {
                return Some(f);
            }
        }
    }
    None
}

/// Build `program` (a `.mim` file, a project folder, or the project of the
/// current folder) for debugging: (executable, project or file folder).
fn build(program: Option<&str>) -> Result<(PathBuf, PathBuf), String> {
    let input = match program {
        Some(p) if Path::new(p).is_dir() => {
            let root = mimic_project::find_root(Path::new(p)).ok_or_else(|| format!("no mim.mim in {p} or above"))?;
            crate::driver::Input::Project(Box::new(mimic_project::Project::load(&root)?))
        }
        Some(p) if p.ends_with(".mim") => {
            let f = Path::new(p);
            // a file of a project builds the project
            match mimic_project::find_root(f.parent().unwrap_or(Path::new("."))) {
                Some(root) if f.starts_with(root.join("src")) => crate::driver::Input::Project(Box::new(mimic_project::Project::load(&root)?)),
                _ => crate::driver::input_from(Some(f))?,
            }
        }
        Some(p) => return Err(format!("`program` must be a .mim file or a project folder, not {p}")),
        None => crate::driver::input_from(None)?,
    };
    let dir = match &input {
        crate::driver::Input::Project(p) => p.root.clone(),
        crate::driver::Input::File(f) => f.parent().map(Path::to_path_buf).unwrap_or_default(),
    };
    let exe = crate::debug_build(&input).map_err(|e| e.trim_end().to_string())?;
    Ok((exe, dir))
}

/// lldb-dap shows a handle as its address and then what it holds
/// (`0x0000000c3940d8c0 "hi"`): keep only what it holds.
fn tidy_values(body: Vec<u8>) -> Vec<u8> {
    if !body.windows(10).any(|w| w == b"\"variables") && !body.windows(9).any(|w| w == b"\"evaluate") {
        return body;
    }
    let Ok(mut v) = serde_json::from_slice::<Value>(&body) else { return body };
    let strip = |x: &mut Value| {
        if x.get("type").and_then(Value::as_str) != Some("value") {
            return;
        }
        for key in ["value", "result"] {
            if let Some(s) = x.get(key).and_then(Value::as_str)
                && let Some(rest) = s.strip_prefix("0x")
                && let Some((hex, shown)) = rest.split_once(' ')
                && hex.chars().all(|c| c.is_ascii_hexdigit())
            {
                x[key] = shown.to_string().into();
            }
        }
    };
    if let Some(vars) = v.pointer_mut("/body/variables").and_then(Value::as_array_mut) {
        vars.iter_mut().for_each(strip);
    }
    if let Some(b) = v.get_mut("body") {
        strip(b);
    }
    v.to_string().into_bytes()
}

pub fn serve() -> Result<(), String> {
    let out = Arc::new(Mutex::new(std::io::stdout()));
    let mut stdin = BufReader::new(std::io::stdin());
    let Some(dap) = find_lldb_dap() else {
        // say why on the first request, then stop
        if let Some(body) = read_message(&mut stdin)
            && let Ok(req) = serde_json::from_slice::<Value>(&body)
        {
            let msg = "mimic dap needs LLVM's lldb-dap: on macOS it comes with Xcode (xcode-select --install), on Linux with the lldb package; or set MIMIC_LLDB_DAP";
            let reply = json!({ "seq": 1, "type": "response", "request_seq": req["seq"], "success": false, "command": req["command"], "message": msg });
            let _ = write_message(&mut *out.lock().unwrap(), reply.to_string().as_bytes());
        }
        return Err("lldb-dap not found".into());
    };
    let mut child = Command::new(&dap)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| format!("cannot start {}: {e}", dap.display()))?;
    let mut to_dap = child.stdin.take().unwrap();
    let mut from_dap = BufReader::new(child.stdout.take().unwrap());
    // everything lldb-dap says goes to the editor as it is
    let out2 = Arc::clone(&out);
    let copier = std::thread::spawn(move || {
        while let Some(body) = read_message(&mut from_dap) {
            let body = tidy_values(body);
            if write_message(&mut *out2.lock().unwrap(), &body).is_err() {
                break;
            }
        }
        // the session is over when lldb-dap is: the editor may keep our
        // stdin open, so leave now
        std::process::exit(0);
    });
    while let Some(body) = read_message(&mut stdin) {
        let mut msg: Value = match serde_json::from_slice(&body) {
            Ok(v) => v,
            Err(_) => {
                let _ = write_message(&mut to_dap, &body);
                continue;
            }
        };
        if msg["type"] == "request" && msg["command"] == "launch" {
            let args = msg["arguments"].clone();
            match build(args.get("program").and_then(Value::as_str)) {
                Ok((exe, dir)) => {
                    let script = crate::lldb_formatters().map_err(|e| e.to_string())?;
                    let mut init = vec![Value::from(format!("command script import {}", script.display()))];
                    if let Some(more) = args.get("initCommands").and_then(Value::as_array) {
                        init.extend(more.iter().cloned());
                    }
                    let a = &mut msg["arguments"];
                    a["program"] = exe.display().to_string().into();
                    a["initCommands"] = Value::Array(init);
                    if a.get("cwd").and_then(Value::as_str).is_none() {
                        a["cwd"] = dir.display().to_string().into();
                    }
                }
                Err(e) => {
                    let reply = json!({ "seq": 0, "type": "response", "request_seq": msg["seq"], "success": false, "command": "launch", "message": format!("the program does not build:\n{e}") });
                    let _ = write_message(&mut *out.lock().unwrap(), reply.to_string().as_bytes());
                    continue;
                }
            }
        }
        if write_message(&mut to_dap, msg.to_string().as_bytes()).is_err() {
            break;
        }
    }
    drop(to_dap);
    let _ = child.wait();
    let _ = copier.join();
    Ok(())
}
