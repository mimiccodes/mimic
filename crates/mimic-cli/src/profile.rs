//! `mimic profile`: where a program spends its time.
//!
//! The program is built for release with frame pointers (target/profiling/)
//! and run with `MIMIC_PROFILE` set: the runtime then samples every thread
//! about a thousand times a second (see the profiler in mimic_rt.c) and
//! writes the samples and its counters at exit. This file names the sampled
//! addresses with the program's symbol table, charges each sample to the
//! innermost Mimic function on the stack (time in the runtime goes to the
//! Mimic code that called it), and joins every function with the compiler's
//! decisions about where it runs (`mimic why`).

use crate::driver::{self, Input, Profile};
use crate::targets::Target;
use mimic_check::Program;
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

pub struct Options {
    pub file: Option<PathBuf>,
    pub json: bool,
    pub hz: Option<u32>,
    pub top: usize,
    /// Stop the program after this many seconds (a server), writing the profile
    pub seconds: Option<f64>,
    pub args: Vec<String>,
}

/// What the runtime wrote.
struct Data {
    hz: f64,
    wall_ns: f64,
    anchor: u64,
    counters: HashMap<String, i64>,
    /// (name, samples spent waiting for work)
    threads: Vec<(String, i64)>,
    /// (thread, stack: innermost first)
    samples: Vec<(usize, Vec<u64>)>,
    /// addresses in system libraries, named by the runtime: (library, name)
    system: HashMap<u64, (String, String)>,
}

fn parse(bytes: &[u8]) -> Result<Data, String> {
    let mut d = Data { hz: 1000.0, wall_ns: 0.0, anchor: 0, counters: HashMap::new(), threads: Vec::new(), samples: Vec::new(), system: HashMap::new() };
    let mut at = 0;
    let bad = || "the profile is damaged (was the program stopped while it was written?)".to_string();
    loop {
        let end = bytes[at..].iter().position(|b| *b == b'\n').ok_or_else(bad)? + at;
        let line = std::str::from_utf8(&bytes[at..end]).map_err(|_| bad())?;
        at = end + 1;
        let mut w = line.split(' ');
        match w.next().unwrap_or("") {
            "mimic-profile" => {}
            "hz" => d.hz = w.next().and_then(|v| v.parse().ok()).unwrap_or(1000.0),
            "wall_ns" => d.wall_ns = w.next().and_then(|v| v.parse().ok()).unwrap_or(0.0),
            "anchor" => d.anchor = w.next().and_then(|v| u64::from_str_radix(v, 16).ok()).unwrap_or(0),
            "counter" => {
                if let (Some(k), Some(v)) = (w.next(), w.next().and_then(|v| v.parse().ok())) {
                    d.counters.insert(k.to_string(), v);
                }
            }
            "sym" => {
                if let (Some(pc), Some(lib), Some(name)) = (w.next().and_then(|v| u64::from_str_radix(v, 16).ok()), w.next(), w.next()) {
                    d.system.insert(pc, (lib.to_string(), name.to_string()));
                }
            }
            "thread" => {
                let _id = w.next();
                let waiting = w.next().and_then(|v| v.parse().ok()).unwrap_or(0);
                d.threads.push((w.collect::<Vec<_>>().join(" "), waiting));
            }
            "samples" => {
                let n: usize = w.next().and_then(|v| v.parse().ok()).ok_or_else(bad)?;
                let depth: usize = w.next().and_then(|v| v.parse().ok()).ok_or_else(bad)?;
                let size = 8 + 8 * depth;
                for i in 0..n {
                    let s = bytes.get(at + i * size..at + (i + 1) * size).ok_or_else(bad)?;
                    let thread = u16::from_le_bytes([s[0], s[1]]) as usize;
                    let k = (s[3] as usize).min(depth);
                    let pcs = (0..k).map(|j| u64::from_le_bytes(s[8 + 8 * j..16 + 8 * j].try_into().unwrap())).collect();
                    d.samples.push((thread, pcs));
                }
                return Ok(d);
            }
            _ => {}
        }
    }
}

/// A named code address.
#[derive(Clone)]
enum Place {
    /// A Mimic function (its index in `Program::fns`), or a fused kernel or
    /// a fork-join task made from it.
    Mimic { fid: usize, part: Option<&'static str> },
    /// Mimic's runtime (`mim_…`) or the system (libc, Metal, …).
    Other(String),
}

struct Names<'a> {
    syms: Vec<(u64, String)>,
    system: &'a HashMap<u64, (String, String)>,
    slide: u64,
    by_symbol: HashMap<&'a str, usize>,
    entry: Option<usize>,
    cache: HashMap<u64, Place>,
}

impl<'a> Names<'a> {
    fn new(exe: &Path, program: &'a Program, anchor: u64, system: &'a HashMap<u64, (String, String)>) -> Result<Names<'a>, String> {
        let syms = mimic_codegen::symbols(exe)?;
        let at = syms.iter().find(|(_, n)| n == "_mim_rt_init" || n == "mim_rt_init").map(|(a, _)| *a).ok_or("the program has no symbol table (was it stripped?)")?;
        let by_symbol = program.fns.iter().enumerate().map(|(i, f)| (f.symbol.as_str(), i)).collect();
        Ok(Names { syms, system, slide: anchor.wrapping_sub(at), by_symbol, entry: program.entry(), cache: HashMap::new() })
    }

    fn place(&mut self, pc: u64) -> Place {
        if let Some(p) = self.cache.get(&pc) {
            return p.clone();
        }
        if let Some((lib, name)) = self.system.get(&pc) {
            let p = Place::Other(format!("{} ({lib})", name.trim_start_matches('_')));
            self.cache.insert(pc, p.clone());
            return p;
        }
        let addr = pc.wrapping_sub(self.slide);
        let i = self.syms.partition_point(|(a, _)| *a <= addr);
        let name = if i == 0 { "?" } else { self.syms[i - 1].1.as_str() };
        // Mach-O and COFF put `_` before C names
        let name = if name.starts_with("__") { &name[1..] } else { name };
        let base = name.split('$').next().unwrap_or(name);
        // the C `main`: `main::main` is compiled into it
        let entry = if base.trim_start_matches('_') == "main" { self.entry } else { None };
        let p = match entry.or_else(|| self.by_symbol.get(base).copied()) {
            Some(fid) => {
                let part = if name.contains("$kernel") {
                    Some("kernel")
                } else if name.ends_with("$task") {
                    Some("task")
                } else {
                    None
                };
                Place::Mimic { fid, part }
            }
            None => Place::Other(name.trim_start_matches('_').to_string()),
        };
        self.cache.insert(pc, p.clone());
        p
    }
}

fn fn_name(p: &Program, fid: usize) -> String {
    let f = &p.fns[fid];
    let mut segs = p.mod_path(f.module);
    segs.push(f.name.clone());
    // the project's own package goes without saying
    if segs.len() > 2 && segs[0] == p.root_package {
        segs.remove(0);
    }
    segs.join("::")
}

pub fn run(o: Options) -> Result<ExitCode, String> {
    let input = driver::input_from(o.file.as_deref())?;
    if !input.is_bin() {
        return Err("this is a library project; there is nothing to run (add `bin` to `kind` in mim.mim)".into());
    }
    let program = driver::load(&input).map_err(|r| r.human())?;
    let mut prof = Profile::for_input(&input, true);
    prof.frame_pointers = true;
    let art = driver::build(&input, &prof, &Target::host(), false, false).map_err(|r| r.human())?;
    let data_path = art.path.with_extension("prof");
    let _ = std::fs::remove_file(&data_path);
    eprintln!("{:>12} {}", "Profiling", art.path.display());
    let mut child = std::process::Command::new(&art.path);
    child.args(&o.args).env("MIMIC_PROFILE", &data_path);
    if o.json {
        // stdout is for the report
        child.stdout(std::process::Stdio::from(std::io::stderr()));
    }
    if let Some(hz) = o.hz {
        child.env("MIMIC_PROFILE_HZ", hz.to_string());
    }
    if let Some(secs) = o.seconds {
        // timed by the program itself, from when it starts running
        child.env("MIMIC_PROFILE_FOR", secs.to_string());
    }
    let mut proc = child.spawn().map_err(|e| format!("cannot run {}: {e}", art.path.display()))?;
    let status = match o.seconds {
        None => proc.wait().map_err(|e| e.to_string())?,
        // (a program that never starts its runtime is stopped well after)
        Some(secs) => {
            let t0 = std::time::Instant::now();
            loop {
                if let Some(st) = proc.try_wait().map_err(|e| e.to_string())? {
                    break st;
                }
                if t0.elapsed().as_secs_f64() >= secs + 60.0 {
                    let _ = proc.kill();
                    break proc.wait().map_err(|e| e.to_string())?;
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
        }
    };
    let bytes = std::fs::read(&data_path).map_err(|_| "the program wrote no profile (did it end in a crash, or call os::exit before the runtime could write it?)".to_string())?;
    let data = parse(&bytes)?;
    let report = analyse(&input, &program, &art.path, &data, o.top)?;
    let json_path = art.path.with_extension("profile.json");
    let _ = std::fs::write(&json_path, serde_json::to_string_pretty(&report).unwrap_or_default());
    if o.json {
        println!("{}", serde_json::to_string_pretty(&report).unwrap_or_default());
    } else {
        print!("{}", render(&report, o.top));
        eprintln!("{:>12} {}", "Saved", json_path.display());
    }
    Ok(if status.success() { ExitCode::SUCCESS } else { ExitCode::from(status.code().unwrap_or(1).clamp(1, 255) as u8) })
}

fn analyse(input: &Input, program: &Program, exe: &Path, d: &Data, top: usize) -> Result<Value, String> {
    let mut names = Names::new(exe, program, d.anchor, &d.system)?;
    let busy = d.samples.len().max(1) as f64;
    // per Mimic function: self (innermost Mimic frame) and total (anywhere)
    let mut own: HashMap<(usize, Option<&'static str>), usize> = HashMap::new();
    let mut total: HashMap<usize, usize> = HashMap::new();
    let mut other: HashMap<String, usize> = HashMap::new();
    let mut outside = 0usize; // samples with no Mimic frame at all
    let mut per_thread: Vec<usize> = vec![0; d.threads.len()];
    for (t, stack) in &d.samples {
        if let Some(c) = per_thread.get_mut(*t) {
            *c += 1;
        }
        let places: Vec<Place> = stack.iter().map(|pc| names.place(*pc)).collect();
        if let Some(Place::Other(n)) = places.first() {
            *other.entry(n.clone()).or_default() += 1;
        }
        match places.iter().find_map(|p| if let Place::Mimic { fid, part } = p { Some((*fid, *part)) } else { None }) {
            Some(k) => *own.entry(k).or_default() += 1,
            None => outside += 1,
        }
        let mut seen = HashSet::new();
        for p in &places {
            if let Place::Mimic { fid, .. } = p
                && seen.insert(*fid)
            {
                *total.entry(*fid).or_default() += 1;
            }
        }
    }
    // where the compiler said each function runs
    let mut decided: HashMap<usize, Vec<&'static str>> = HashMap::new();
    if let Ok(plan) = driver::plan(input, program) {
        for dec in plan {
            if let Some(f) = dec.fid {
                let v = decided.entry(f).or_default();
                if !v.contains(&dec.kind) {
                    v.push(dec.kind);
                }
            }
        }
    }
    let mut rows: Vec<_> = own.iter().collect();
    rows.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    let functions: Vec<Value> = rows
        .iter()
        .take(top)
        .map(|((fid, part), n)| {
            let f = &program.fns[*fid];
            let file = &program.files[f.file];
            let (line, _) = mimic_syntax::line_col(&file.src, f.span.start);
            json!({
                "function": fn_name(program, *fid),
                "part": part,
                "self": **n as f64 / busy,
                "total": *total.get(fid).unwrap_or(&0) as f64 / busy,
                "samples": n,
                "location": { "file": file.display, "line": line },
                "where": decided.get(fid).cloned().unwrap_or_default(),
            })
        })
        .collect();
    let mut others: Vec<_> = other.into_iter().collect();
    others.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    let runtime: Vec<Value> = others.iter().take(top).map(|(n, c)| json!({ "function": n, "self": *c as f64 / busy, "samples": c })).collect();
    // each thread: the share of the run it was working
    let wall_s = d.wall_ns / 1e9;
    // the sampler's real rate: it may fall behind when every core is busy
    let rounds = *d.counters.get("rounds").unwrap_or(&0) as f64;
    let rate = if rounds > 0.0 && wall_s > 0.0 { rounds / wall_s } else { d.hz };
    let expected = (wall_s * rate).max(1.0);
    let threads: Vec<Value> = d
        .threads
        .iter()
        .zip(&per_thread)
        .map(|((name, waiting), working)| {
            let seen = (*working as f64 + *waiting as f64).max(1.0);
            json!({ "name": name, "working": *working as f64 / seen, "seen": (seen / expected).min(1.0), "samples": working, "waiting": waiting })
        })
        .collect();
    let c = |k: &str| *d.counters.get(k).unwrap_or(&0);
    let cpu_s = d.samples.len() as f64 / rate;
    let gpu_s = c("gpu_ns") as f64 / 1e9;
    let mut by_file: BTreeMap<String, f64> = BTreeMap::new();
    for f in &functions {
        *by_file.entry(f["location"]["file"].as_str().unwrap_or("").to_string()).or_default() += f["self"].as_f64().unwrap_or(0.0);
    }
    Ok(json!({
        "program": exe.display().to_string(),
        "wall_seconds": wall_s,
        "cpu_seconds": cpu_s,
        "rate_hz": rate.round(),
        "samples": d.samples.len(),
        "lost_samples": c("lost"),
        "outside_mimic": outside as f64 / busy,
        "functions": functions,
        "runtime": runtime,
        "threads": threads,
        "gpu": { "seconds": gpu_s, "kernels": c("gpu_kernels"), "bytes": c("gpu_bytes") },
        "memory": { "allocated_bytes": c("alloc_bytes"), "allocations": c("allocs"), "copied_bytes": c("copy_bytes"), "copies": c("copies") },
    }))
}

fn bytes(n: f64) -> String {
    if n >= 1e9 {
        format!("{:.1} GB", n / 1e9)
    } else if n >= 1e6 {
        format!("{:.1} MB", n / 1e6)
    } else if n >= 1e3 {
        format!("{:.1} kB", n / 1e3)
    } else {
        format!("{n} B")
    }
}

fn mark(kind: &str) -> &'static str {
    match kind {
        "parallel" => "∥ parallel",
        "net" => "∥ net",
        "gpu" => "▣ GPU",
        "task" => "◷ task",
        "ordered" => "→ in order",
        "waits" => "⏸ waits",
        "fused" => "⇶ fused",
        "simd" => "≋ SIMD",
        _ => "·",
    }
}

fn render(r: &Value, top: usize) -> String {
    let f = |v: &Value| v.as_f64().unwrap_or(0.0);
    let pct = |x: f64| format!("{:5.1}%", x * 100.0);
    let wall = f(&r["wall_seconds"]);
    let cpu = f(&r["cpu_seconds"]);
    let mut out = String::new();
    out += &format!(
        "\n{:.3} s on the clock, {:.3} s of CPU time on {} thread(s) ({} samples at {} Hz)\n",
        wall,
        cpu,
        r["threads"].as_array().map_or(0, Vec::len),
        r["samples"],
        r["rate_hz"]
    );
    if r["lost_samples"].as_i64().unwrap_or(0) > 0 {
        out += &format!("({} samples did not fit and were dropped: lower the rate with --hz)\n", r["lost_samples"]);
    }
    out += "\nFunctions (self: time in the function and the runtime calls it makes; total: with what it calls)\n";
    out += "   self   total  function                               where it runs\n";
    for row in r["functions"].as_array().into_iter().flatten() {
        let mut name = row["function"].as_str().unwrap_or("").to_string();
        if let Some(p) = row["part"].as_str() {
            name += &format!(" ({p})");
        }
        let at = format!("{}:{}", row["location"]["file"].as_str().unwrap_or(""), row["location"]["line"]);
        let wher: Vec<&str> = row["where"].as_array().into_iter().flatten().filter_map(Value::as_str).map(mark).collect();
        let wher = if wher.is_empty() { "one core, in order".to_string() } else { wher.join(", ") };
        out += &format!("{} {}  {:<38} {}\n", pct(f(&row["self"])), pct(f(&row["total"])), name, wher);
        out += &format!("                 {at}\n");
    }
    if f(&r["outside_mimic"]) > 0.0 {
        out += &format!("{}         (no Mimic function on the stack: start-up, the runtime's own threads)\n", pct(f(&r["outside_mimic"])));
    }
    let rt: Vec<&Value> = r["runtime"].as_array().into_iter().flatten().filter(|v| f(&v["self"]) >= 0.01).take(top.min(8)).collect();
    if !rt.is_empty() {
        out += "\nInside the runtime and the system (part of the self time above)\n";
        for row in rt {
            out += &format!("{}  {}\n", pct(f(&row["self"])), row["function"].as_str().unwrap_or(""));
        }
    }
    out += "\nCores (each thread: the share of its time spent working, not waiting for work)\n";
    let threads = r["threads"].as_array().cloned().unwrap_or_default();
    let mut avg = 0.0;
    for (i, t) in threads.iter().enumerate() {
        let w = f(&t["working"]);
        avg += w;
        let bar = (w * 30.0).round() as usize;
        out += &format!("  {:>2} {:<7} {} {}\n", i, t["name"].as_str().unwrap_or(""), format!("{}{}", "█".repeat(bar), "·".repeat(30 - bar.min(30))), pct(w));
    }
    if !threads.is_empty() {
        out += &format!("  average {}: {:.1} of {} cores kept busy\n", pct(avg / threads.len() as f64), cpu / wall.max(1e-9), threads.len());
    }
    let gpu = f(&r["gpu"]["seconds"]);
    out += "\nCPU and GPU\n";
    if r["gpu"]["kernels"].as_i64().unwrap_or(0) > 0 {
        out += &format!(
            "  GPU: {} kernel(s), {:.3} s ({} of the run), {} moved; CPU: {:.3} s\n",
            r["gpu"]["kernels"],
            gpu,
            pct(gpu / wall.max(1e-9)).trim(),
            bytes(f(&r["gpu"]["bytes"])),
            cpu
        );
    } else {
        out += &format!("  everything ran on the CPU ({:.3} s); no kernel went to the GPU\n", cpu);
    }
    let m = &r["memory"];
    out += "\nMemory\n";
    out += &format!(
        "  {} allocated in {} value(s) ({}/s); {} copied in {} copy-on-write copies\n",
        bytes(f(&m["allocated_bytes"])),
        m["allocations"],
        bytes(f(&m["allocated_bytes"]) / wall.max(1e-9)),
        bytes(f(&m["copied_bytes"])),
        m["copies"]
    );
    out
}
