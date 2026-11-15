//! `mimic net show` and `mimic net trace`: interaction nets for people and
//! editors to look at (the VS Code net view draws their JSON).
//!
//! `show` lays out the nets of a `.mnet` file, or of the pure functions of
//! a Mimic program, as nodes, ports and wires. `trace` reduces one round at
//! a time (mimic_net::sim) and reports how many redexes each round had:
//! the parallelism the net offers.

use crate::driver;
use mimic_net::sim::{self, Graph, Kind, Limits};
use mimic_net::{Book, Def, Num};
use serde_json::{Value, json};
use std::path::Path;

/// The book of `file`: a `.mnet` file as written, or the nets of a Mimic
/// program (as the runtime gets them, or `before` compile-time reduction),
/// with the program's functions: (function, its definition, file, line,
/// entered through the net).
struct Nets {
    book: Book,
    functions: Vec<(String, String, String, u32, bool)>,
}

fn load(file: &Path, before: bool) -> Result<Nets, String> {
    let src = std::fs::read_to_string(file).map_err(|e| format!("cannot read {}: {e}", file.display()))?;
    if file.extension().is_some_and(|e| e == "mnet") {
        let mut book = mimic_net::parse(&src).map_err(|e| format!("{}:{e}", file.display()))?;
        if !before {
            book.pre_reduce();
        }
        return Ok(Nets { book, functions: Vec::new() });
    }
    let input = driver::input_from(Some(file))?;
    let p = driver::load(&input).map_err(|r| r.human())?;
    let (after, unreduced, fns) = mimic_codegen::nets(&p);
    let mut functions = Vec::new();
    for (fid, def, entered) in fns {
        let f = &p.fns[fid];
        // the program's own functions, not the standard library's
        if p.mods[f.module].package != p.root_package {
            continue;
        }
        let sf = &p.files[f.file];
        let (line, _) = mimic_syntax::line_col(&sf.src, f.span.start);
        // (the project's own package goes without saying)
        let mut path = p.fn_path(fid);
        if path.len() > 2 && path[0] == p.root_package {
            path.remove(0);
        }
        functions.push((path.join("::"), def, sf.display.clone(), line, entered));
    }
    Ok(Nets { book: if before { unreduced } else { after }, functions })
}

/// The definitions of a function: its own and those made from it (`.then1`).
fn defs_of<'a>(book: &'a Book, def: &str) -> Vec<&'a Def> {
    book.defs.iter().filter(|d| d.name == def || d.name.starts_with(&format!("{def}."))).collect()
}

fn graph_json(g: &Graph, name: &str) -> Value {
    let mut id = vec![usize::MAX; g.nodes.len()];
    let mut nodes = Vec::new();
    for (i, n) in g.nodes.iter().enumerate() {
        if let Some(n) = n {
            id[i] = nodes.len();
            // (key: the node's slot, the same from one frame to the next)
            nodes.push(json!({ "id": nodes.len(), "key": i, "kind": n.kind.name(), "label": n.kind.label(), "def": n.def }));
        }
    }
    let mut wires = Vec::new();
    for (i, n) in g.nodes.iter().enumerate() {
        let Some(n) = n else { continue };
        for (p, peer) in n.ports.iter().enumerate() {
            let Some((j, q)) = *peer else { continue };
            // each wire once
            if (i, p) < (j, q as usize) && id[j] != usize::MAX {
                let active = p == 0 && q == 0 && n.kind != Kind::Root && g.nodes[j].as_ref().is_some_and(|m| m.kind != Kind::Root);
                wires.push(json!({ "a": [id[i], p], "b": [id[j], q], "active": active }));
            }
        }
    }
    json!({ "name": name, "nodes": nodes, "wires": wires, "root": g.root.map(|r| id[r]) })
}

pub fn show(file: &Path, function: Option<&str>, before: bool, json_out: bool) -> Result<(), String> {
    let nets = load(file, before)?;
    let other = if before { None } else { load(file, true).ok() };
    let size = |b: &Book, defs: &[&Def]| -> usize {
        let names: Vec<&str> = defs.iter().map(|d| d.name.as_str()).collect();
        b.defs.iter().filter(|d| names.contains(&d.name.as_str())).map(Def::node_count).sum()
    };
    let mut out = Vec::new();
    if nets.functions.is_empty() {
        // a .mnet file: every definition
        for d in &nets.book.defs {
            if function.is_some_and(|f| f != d.name) {
                continue;
            }
            out.push(json!({ "def": d.name, "text": d.to_string(), "nodes": d.node_count(), "graph": graph_json(&Graph::of_def(d), &d.name) }));
        }
        if json_out {
            println!("{}", serde_json::to_string_pretty(&json!({ "file": file.display().to_string(), "before": before, "defs": out })).unwrap_or_default());
        } else {
            for d in &nets.book.defs {
                if function.is_none_or(|f| f == d.name) {
                    println!("{d}");
                }
            }
        }
        return Ok(());
    }
    for (name, def, shown, line, entered) in &nets.functions {
        if let Some(f) = function
            && f != name
            && !name.ends_with(&format!("::{f}"))
        {
            continue;
        }
        let defs = defs_of(&nets.book, def);
        let unreduced = other.as_ref().map(|o| size(&o.book, &defs_of(&o.book, def)));
        out.push(json!({
            "function": name,
            "def": def,
            "location": { "file": shown, "line": line },
            "entered": entered,
            "nodes": size(&nets.book, &defs),
            "nodes_before": unreduced,
            "defs": defs.iter().map(|d| json!({ "def": d.name, "text": d.to_string(), "nodes": d.node_count(), "graph": graph_json(&Graph::of_def(d), &d.name) })).collect::<Vec<_>>(),
        }));
    }
    if json_out {
        println!("{}", serde_json::to_string_pretty(&json!({ "file": file.display().to_string(), "before": before, "functions": out })).unwrap_or_default());
        return Ok(());
    }
    if out.is_empty() {
        println!("{}", match function {
            Some(f) => format!("`{f}` is not compiled to an interaction net (only pure functions on numbers are; `mimic why {f}` says why)"),
            None => "no function of this program is compiled to an interaction net (only pure functions on numbers are)".into(),
        });
        return Ok(());
    }
    for f in &out {
        let entered = if f["entered"] == true { "runs on the net when called from native code" } else { "native from native code; a net when called from nets" };
        let before = f["nodes_before"].as_u64().map(|n| format!(", {n} before compile-time reduction")).unwrap_or_default();
        println!("{}  ({}:{})  {} nodes{before}; {entered}", f["function"].as_str().unwrap_or(""), f["location"]["file"].as_str().unwrap_or(""), f["location"]["line"], f["nodes"]);
        for d in f["defs"].as_array().into_iter().flatten() {
            println!("  {}", d["text"].as_str().unwrap_or("").replace('\n', "\n  "));
        }
        println!();
    }
    Ok(())
}

fn parse_num(s: &str) -> Result<Num, String> {
    let s = s.trim();
    match s {
        "true" => return Ok(Num::Bool(true)),
        "false" => return Ok(Num::Bool(false)),
        _ => {}
    }
    if let Ok(i) = s.parse::<i64>() {
        return Ok(Num::Int(i));
    }
    s.parse::<f64>().map(Num::F64).map_err(|_| format!("`{s}` is not a number (arguments of a traced call are numbers)"))
}

pub struct TraceOpts<'a> {
    pub function: Option<&'a str>,
    pub args: Vec<String>,
    pub before: bool,
    pub steps: usize,
    pub frames: usize,
    pub frame_nodes: usize,
    pub json: bool,
}

pub fn trace(file: &Path, o: TraceOpts) -> Result<(), String> {
    let nets = load(file, o.before)?;
    let mut book = nets.book.clone();
    let args = o.args.iter().flat_map(|a| a.split(',')).filter(|a| !a.trim().is_empty()).map(parse_num).collect::<Result<Vec<_>, _>>()?;
    // what to reduce: @main of a .mnet file, or a call of a function
    let (entry, what) = match o.function {
        None if nets.functions.is_empty() => ("main".to_string(), "@main".to_string()),
        None => return Err("which function? `mimic net trace FILE --fn NAME ARGS…` (the functions with nets: `mimic net show FILE`)".into()),
        Some(f) => {
            let def = nets
                .functions
                .iter()
                .find(|(n, ..)| n == f || n.ends_with(&format!("::{f}")))
                .map(|(_, d, ..)| d.clone())
                .or_else(|| book.defs.iter().find(|d| d.name == f).map(|d| d.name.clone()))
                .ok_or_else(|| format!("`{f}` is not compiled to an interaction net (`mimic net show {}` lists those that are)", file.display()))?;
            let mut call = sim::call(&def, &args);
            call.name = "trace·call".into();
            book.defs.push(call);
            let shown: Vec<String> = args.iter().map(|a| a.to_string().trim_start_matches('#').to_string()).collect();
            ("trace·call".to_string(), format!("{f}({})", shown.join(", ")))
        }
    };
    let limits = Limits { steps: o.steps, nodes: 4_000_000, frame_nodes: if o.frames > 0 { o.frame_nodes } else { 0 }, frames: o.frames };
    let t = sim::trace(&book, &entry, &limits)?;
    let rounds = t.steps.len();
    let widest = t.steps.iter().map(|s| s.redexes).max().unwrap_or(0);
    let average = if rounds > 0 { t.interactions as f64 / rounds as f64 } else { 0.0 };
    let result = t.result.map(|n| n.to_string().trim_start_matches('#').to_string());
    let mut rules: std::collections::BTreeMap<&str, usize> = Default::default();
    for s in &t.steps {
        for (k, v) in &s.rules {
            *rules.entry(k).or_default() += v;
        }
    }
    if o.json {
        // long traces: at most 2000 points, each the widest of its stretch
        let bucket = rounds.div_ceil(2000).max(1);
        let steps: Vec<Value> = t
            .steps
            .chunks(bucket)
            .map(|c| json!({ "redexes": c.iter().map(|s| s.redexes).max().unwrap_or(0), "nodes": c.last().map_or(0, |s| s.nodes), "rules": c.iter().fold(serde_json::Map::new(), |mut m, s| { for (k, v) in &s.rules { let e = m.entry(k.to_string()).or_insert(json!(0)); *e = json!(e.as_u64().unwrap_or(0) + *v as u64); } m }) }))
            .collect();
        let frames: Vec<Value> = t.frames.iter().enumerate().map(|(i, g)| graph_json(g, &format!("round {i}"))).collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "file": file.display().to_string(),
                "call": what,
                "before": o.before,
                "result": result,
                "stopped": t.stopped,
                "interactions": t.interactions,
                "rounds": rounds,
                "widest": widest,
                "average": average,
                "rules": rules,
                "steps": steps,
                "steps_per_point": bucket,
                "frames": frames,
            }))
            .unwrap_or_default()
        );
        return Ok(());
    }
    println!("{what} = {}", result.as_deref().unwrap_or("(not a number)"));
    if let Some(s) = &t.stopped {
        println!("({s})");
    }
    println!("{} interactions in {rounds} rounds: {average:.1} at once on average, {widest} at the widest", t.interactions);
    println!("rules: {}", rules.iter().map(|(k, v)| format!("{k} {v}")).collect::<Vec<_>>().join(", "));
    // redexes per round, as a strip of bars
    let bars = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    let width = 72.min(rounds.max(1));
    let per = rounds.div_ceil(width).max(1);
    let strip: String = t
        .steps
        .chunks(per)
        .map(|c| {
            let m = c.iter().map(|s| s.redexes).max().unwrap_or(0);
            bars[((m as f64 / widest.max(1) as f64) * 7.0).round() as usize]
        })
        .collect();
    println!("redexes per round: {strip}");
    println!("(rounds as if every redex had a core: the parallelism the net offers; the runtime has as many cores as the machine and runs some calls natively)");
    Ok(())
}
