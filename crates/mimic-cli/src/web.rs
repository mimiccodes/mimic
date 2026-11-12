//! `mimic build --web` / `mimic run --web`: a program as a web page.
//!
//! The program is built for wasm32-wasi. The page's loader gives it the few
//! system calls it needs (console output, clocks, random numbers) and tells
//! it, through the environment, that it runs in a page. `ui::run` then hands
//! its app to the page instead of serving it (runtime/ui.mim, mimic_ui.c):
//! the page asks for the first screen, sends events and applies the patches
//! it gets back, with the same browser code the served version uses.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};

/// The page: a WASI shim, then start the program and connect it.
const PAGE: &str = r#"<!doctype html>
<html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>Mimic</title><style id="mimic-theme"></style></head>
<body><div id="mimic-app">loading…</div>
<script type="module">
class Exit { constructor(code) { this.code = code } }
let mem, ex
const text = new TextDecoder(), bytes = new TextEncoder()
const view = () => new DataView(mem.buffer)
const env = ['MIMIC_UI_BROWSER=1'], args = ['app']
let line = ''
function strings(list, countPtr, sizePtr) {
  view().setUint32(countPtr, list.length, true)
  view().setUint32(sizePtr, list.reduce((n, s) => n + bytes.encode(s).length + 1, 0), true)
  return 0
}
function fill(list, ptrs, buf) {
  for (const s of list) {
    view().setUint32(ptrs, buf, true); ptrs += 4
    const b = bytes.encode(s + '\0'); new Uint8Array(mem.buffer, buf, b.length).set(b); buf += b.length
  }
  return 0
}
const wasi = {
  fd_write(fd, iovs, n, written) {
    let total = 0
    for (let i = 0; i < n; i++) {
      const p = view().getUint32(iovs + 8 * i, true), len = view().getUint32(iovs + 8 * i + 4, true)
      line += text.decode(new Uint8Array(mem.buffer, p, len)); total += len
    }
    const parts = line.split('\n'); line = parts.pop()
    for (const l of parts) (fd === 2 ? console.error : console.log)(l)
    view().setUint32(written, total, true)
    return 0
  },
  proc_exit(code) { throw new Exit(code) },
  environ_sizes_get: (c, s) => strings(env, c, s), environ_get: (p, b) => fill(env, p, b),
  args_sizes_get: (c, s) => strings(args, c, s), args_get: (p, b) => fill(args, p, b),
  clock_time_get(id, precision, out) {
    const ns = id === 0 ? BigInt(Date.now()) * 1000000n : BigInt(Math.round(performance.now() * 1e6))
    view().setBigUint64(out, ns, true); return 0
  },
  random_get(p, n) { crypto.getRandomValues(new Uint8Array(mem.buffer, p, n)); return 0 },
  // sleeping returns at once: a page must not block
  poll_oneoff(subs, events, n, count) {
    for (let i = 0; i < n; i++) {
      view().setBigUint64(events + 32 * i, view().getBigUint64(subs + 48 * i, true), true)
      view().setUint16(events + 32 * i + 8, 0, true); view().setUint8(events + 32 * i + 10, 0)
    }
    view().setUint32(count, n, true); return 0
  },
  fd_fdstat_get(fd, out) { new Uint8Array(mem.buffer, out, 24).fill(0); view().setUint8(out, 2); return 0 },
  fd_close: () => 0, fd_seek: () => 70, fd_read(fd, iovs, n, read) { view().setUint32(read, 0, true); return 0 },
  sched_yield: () => 0,
}
const imports = { wasi_snapshot_preview1: new Proxy(wasi, { get: (t, k) => t[k] || (() => 52) }) }
const { instance } = await WebAssembly.instantiateStreaming(fetch('app.wasm'), imports)
ex = instance.exports; mem = ex.memory
try { ex._start() } catch (e) { if (!(e instanceof Exit)) throw e }
function call(s) {
  const b = bytes.encode(s), p = ex.mim_ui_alloc(b.length)
  new Uint8Array(mem.buffer, p, b.length).set(b)
  const r = ex.mim_ui_call(p, b.length), m = new Uint8Array(mem.buffer)
  let end = r; while (m[end]) end++
  return text.decode(m.subarray(r, end))
}
if (!ex.mim_ui_script) throw new Error('this program does not call ui::run')
const script = (() => { const m = new Uint8Array(mem.buffer), r = ex.mim_ui_script(); let e = r; while (m[e]) e++; return text.decode(m.subarray(r, e)) })()
const start = JSON.parse(call(''))
document.title = start.title
document.getElementById('mimic-theme').textContent = start.css
document.getElementById('mimic-app').outerHTML = start.html
const mimicUi = new Function(script + ';return mimicUi')()
let apply
apply = mimicUi(m => apply(JSON.parse(call(m))))
start.every.forEach((ms, i) => { if (ms > 0) setInterval(() => apply(JSON.parse(call('@every ' + i))), ms) })
</script></body></html>
"#;

/// Put the page next to the program: `<name>-web/index.html` and `app.wasm`.
pub fn package(wasm: &Path) -> std::io::Result<PathBuf> {
    let stem = wasm.file_stem().and_then(|s| s.to_str()).unwrap_or("app");
    let dir = wasm.with_file_name(format!("{stem}-web"));
    std::fs::create_dir_all(&dir)?;
    std::fs::copy(wasm, dir.join("app.wasm"))?;
    std::fs::write(dir.join("index.html"), PAGE)?;
    Ok(dir)
}

/// Serve `dir` at http://localhost:{port}/ until the process is stopped.
pub fn serve(dir: &Path, port: u16) -> std::io::Result<()> {
    let listener = TcpListener::bind(("127.0.0.1", port))?;
    eprintln!("     Serving {} at http://localhost:{port}/ (Ctrl-C stops)", dir.display());
    for stream in listener.incoming() {
        let Ok(mut stream) = stream else { continue };
        let mut first = String::new();
        let mut reader = BufReader::new(&stream);
        if reader.read_line(&mut first).is_err() {
            continue;
        }
        // skip the rest of the request head
        let mut h = String::new();
        while reader.read_line(&mut h).map(|n| n > 2).unwrap_or(false) {
            h.clear();
        }
        let path = first.split_whitespace().nth(1).unwrap_or("/");
        let name = match path.split('?').next().unwrap_or("/") {
            "/" => "index.html",
            p => p.trim_start_matches('/'),
        };
        let file = (!name.contains("..") && !name.contains('/')).then(|| dir.join(name));
        let (status, kind, body) = match file.and_then(|f| std::fs::read(f).ok()) {
            Some(b) => {
                let kind = if name.ends_with(".wasm") {
                    "application/wasm"
                } else if name.ends_with(".html") {
                    "text/html; charset=utf-8"
                } else {
                    "application/octet-stream"
                };
                ("200 OK", kind, b)
            }
            None => ("404 Not Found", "text/plain", b"not found".to_vec()),
        };
        let head = format!("HTTP/1.1 {status}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n", body.len());
        let _ = stream.write_all(head.as_bytes()).and_then(|_| stream.write_all(&body));
    }
    Ok(())
}
