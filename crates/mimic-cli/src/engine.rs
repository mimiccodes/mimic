//! The Workbench engine: one analysis of a project, kept up to date as its
//! files change, and the questions every front end asks of it. `mimic query`
//! is its command line; the language server, MCP and the editor ask the same
//! questions through the same JSON.
//!
//! Each check loads the program as `mimic check` does, but in a process that
//! stays: the standard library and every unchanged file keep their parse,
//! and files open in an editor are read from the editor, not the disk.
//! When the program has errors, the last good index stays, so lookups keep
//! working while code is being typed.

use crate::doc;
use crate::driver::{self, Input, sources};
use crate::report::Report;
use mimic_check::{FnKind, ModId, Program, Purity, Resolved};
use mimic_syntax::Span;
use mimic_syntax::ast::*;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

pub struct Engine {
    input: Input,
    /// A new number for every check.
    generation: u64,
    /// The last program that checked, and its index.
    good: Option<Analysis>,
    /// The errors of the latest check (`None`: it passed).
    errors: Option<Report>,
    took_ms: f64,
}

struct Analysis {
    program: Program,
    index: Index,
    /// Computed on the first question about effects.
    effects: Option<Vec<&'static str>>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Fn,
    /// A function of the runtime (`extern "mimic"`) or of C.
    Extern,
    Type,
    Variant,
    Module,
}

impl Kind {
    fn name(self) -> &'static str {
        match self {
            Kind::Fn => "fn",
            Kind::Extern => "extern fn",
            Kind::Type => "type",
            Kind::Variant => "variant",
            Kind::Module => "module",
        }
    }
}

pub struct Symbol {
    pub name: String,
    pub kind: Kind,
    /// As written in code: `fs::read`, `c::printf`, `geometry::area`.
    pub path: String,
    pub package: String,
    pub public: bool,
    pub file: usize,
    pub span: Span,
    pub signature: String,
    pub doc: String,
    /// For C functions: the header they came from.
    pub header: Option<String>,
    /// Index into `Program::fns` (functions only).
    fn_id: Option<usize>,
    /// Declared in a namespace of C declarations (`c::`, `cpp::`, an `as` name).
    pub foreign: bool,
}

/// A name used in code, and what it names.
struct Ref {
    file: usize,
    span: Span,
    to: Target,
}

#[derive(Clone)]
enum Target {
    Sym(usize),
    /// A parameter or local variable, defined at `def` in the same file.
    Local { name: String, def: Span },
}

#[derive(Default)]
struct Index {
    symbols: Vec<Symbol>,
    refs: Vec<Ref>,
    by_fn: HashMap<usize, usize>,
    by_type: HashMap<usize, usize>,
    by_variant: HashMap<(usize, usize), usize>,
    by_mod: HashMap<ModId, usize>,
}

impl Engine {
    /// The project in the current directory, or the standalone file `file`.
    pub fn open(file: Option<&Path>) -> Result<Engine, String> {
        let input = driver::input_from(file)?;
        sources::keep_parses();
        Ok(Engine { input, generation: 0, good: None, errors: None, took_ms: 0.0 })
    }

    /// The project whose root folder is `root`.
    pub fn for_project(root: &Path) -> Result<Engine, String> {
        let p = mimic_project::Project::load(root)?;
        sources::keep_parses();
        Ok(Engine { input: Input::Project(Box::new(p)), generation: 0, good: None, errors: None, took_ms: 0.0 })
    }

    /// A standalone file (`module main`), outside any project.
    pub fn for_file(file: &Path) -> Engine {
        sources::keep_parses();
        Engine { input: Input::File(file.to_path_buf()), generation: 0, good: None, errors: None, took_ms: 0.0 }
    }

    /// The errors of the latest check (`None`: it passed, or nothing was checked).
    pub fn errors(&self) -> Option<&Report> {
        self.errors.as_ref()
    }

    /// The files of the last program that checked: (path, text).
    pub fn files(&self) -> Vec<(PathBuf, String)> {
        self.good.as_ref().map(|a| a.program.files.iter().map(|f| (f.path.clone(), f.src.clone())).collect()).unwrap_or_default()
    }

    /// The standard functions under `prefixes` (`http::`, `tcp::`…) that the
    /// program's own files use (checks the program first if needed).
    pub fn uses_of(&mut self, prefixes: &[&str]) -> Result<Vec<String>, String> {
        let a = self.analysis()?;
        let root = &a.program.root_package;
        let mut out: Vec<String> = a
            .index
            .refs
            .iter()
            .filter(|r| a.program.files[r.file].package == *root)
            .filter_map(|r| match r.to {
                Target::Sym(i) => Some(&a.index.symbols[i]),
                Target::Local { .. } => None,
            })
            .filter(|x| x.package == "std" && prefixes.iter().any(|p| x.path.starts_with(p)))
            .map(|x| x.path.clone())
            .collect();
        out.sort();
        out.dedup();
        Ok(out)
    }

    /// Check the program again (after files changed).
    pub fn check(&mut self) {
        let t = Instant::now();
        self.generation += 1;
        match driver::load(&self.input) {
            Ok(program) => {
                let index = Index::build(&program);
                self.good = Some(Analysis { program, index, effects: None });
                self.errors = None;
            }
            Err(r) => self.errors = Some(r),
        }
        self.took_ms = t.elapsed().as_secs_f64() * 1e3;
    }

    /// Answer one question. `params` is a JSON object.
    pub fn query(&mut self, method: &str, params: &Value) -> Result<Value, String> {
        let s = |k: &str| params.get(k).and_then(Value::as_str);
        let n = |k: &str| params.get(k).and_then(Value::as_u64);
        match method {
            // the editor's text for a file, used instead of the disk's
            "open" | "change" => {
                let file = self.path_param(params)?;
                let text = s("text").ok_or("`text` is missing")?;
                sources::set_open(&file, Some(text.to_string()));
                self.check();
                self.query("check", &json!({}))
            }
            "close" => {
                let file = self.path_param(params)?;
                sources::set_open(&file, None);
                self.check();
                self.query("check", &json!({}))
            }
            "check" => {
                if self.generation == 0 || params.get("again").and_then(Value::as_bool).unwrap_or(false) {
                    self.check();
                }
                let mut v = match &self.errors {
                    Some(r) => r.json("query"),
                    None => Report::default().json("query"),
                };
                v["generation"] = self.generation.into();
                v["took_ms"] = ((self.took_ms * 100.0).round() / 100.0).into();
                if let Some(a) = &self.good {
                    let p = &a.program;
                    v["files"] = p.files.iter().filter(|f| f.package == p.root_package).count().into();
                    v["functions"] = p.fns.len().into();
                    v["stale_index"] = self.errors.is_some().into();
                }
                Ok(v)
            }
            "symbols" => {
                let a = self.analysis()?;
                let q = s("query").unwrap_or("").to_lowercase();
                let kind = s("kind");
                let package = s("package");
                let limit = n("limit").unwrap_or(200) as usize;
                let mut hits: Vec<(u8, &Symbol)> = a
                    .index
                    .symbols
                    .iter()
                    .filter(|x| kind.is_none_or(|k| x.kind.name() == k || (k == "extern" && x.kind == Kind::Extern)))
                    .filter(|x| package.is_none_or(|p| x.package == p))
                    .filter_map(|x| match_rank(&q, x).map(|r| (r, x)))
                    .collect();
                hits.sort_by(|(r1, x1), (r2, x2)| r1.cmp(r2).then(x1.path.len().cmp(&x2.path.len())).then(x1.path.cmp(&x2.path)));
                let total = hits.len();
                let list: Vec<Value> = hits.into_iter().take(limit).map(|(_, x)| a.symbol_json(x)).collect();
                Ok(json!({ "total": total, "symbols": list }))
            }
            "definition" | "hover" => {
                let (file, at) = self.position(params)?;
                let a = self.analysis()?;
                let Some(to) = a.target_at(file, at) else { return Ok(Value::Null) };
                Ok(match to {
                    Target::Sym(i) => {
                        let mut v = a.symbol_json(&a.index.symbols[i]);
                        if method == "hover" {
                            v["effects"] = a.effect_of(i).map(Value::from).unwrap_or(Value::Null);
                        }
                        v
                    }
                    Target::Local { name, def } => json!({
                        "name": name,
                        "kind": "local",
                        "location": a.location(file, def),
                    }),
                })
            }
            "references" => {
                let a = self.analysis()?;
                let target = match s("name") {
                    Some(name) => Target::Sym(a.find(name).ok_or_else(|| format!("no symbol `{name}`"))?),
                    None => {
                        let (file, at) = self.position(params)?;
                        let a = self.good.as_ref().unwrap();
                        let Some(t) = a.target_at(file, at) else { return Ok(json!([])) };
                        t
                    }
                };
                let a = self.good.as_ref().unwrap();
                let same = |t: &Target| match (t, &target) {
                    (Target::Sym(x), Target::Sym(y)) => x == y,
                    (Target::Local { name: n1, def: d1 }, Target::Local { name: n2, def: d2 }) => n1 == n2 && d1.start == d2.start,
                    _ => false,
                };
                let locs: Vec<Value> = a.index.refs.iter().filter(|r| same(&r.to)).map(|r| a.location(r.file, r.span)).collect();
                Ok(Value::Array(locs))
            }
            "outline" => {
                let file = self.path_param(params)?;
                let a = self.analysis()?;
                let fi = a.file_index(&file).ok_or_else(|| format!("{} is not part of the program", file.display()))?;
                let mut syms: Vec<&Symbol> = a.index.symbols.iter().filter(|x| x.file == fi && x.kind != Kind::Module).collect();
                syms.sort_by_key(|x| x.span.start);
                Ok(Value::Array(syms.into_iter().map(|x| a.symbol_json(x)).collect()))
            }
            "docs" => {
                let name = s("name").ok_or("`name` is missing")?;
                let a = self.analysis()?;
                let i = a.find(name).ok_or_else(|| format!("no symbol `{name}`"))?;
                let mut v = a.symbol_json(&a.index.symbols[i]);
                v["effects"] = a.effect_of(i).map(Value::from).unwrap_or(Value::Null);
                Ok(v)
            }
            "effects" => {
                let a = self.analysis()?;
                let only = s("name");
                let package = s("package").map(String::from).unwrap_or_else(|| a.program.root_package.clone());
                let list: Vec<usize> = match only {
                    Some(name) => vec![a.find(name).ok_or_else(|| format!("no symbol `{name}`"))?],
                    None => (0..a.index.symbols.len())
                        .filter(|&i| a.index.symbols[i].kind == Kind::Fn && a.index.symbols[i].package == package)
                        .collect(),
                };
                let mut out = Vec::new();
                for i in list {
                    let e = a.effect_of(i);
                    let x = &a.index.symbols[i];
                    out.push(json!({ "path": x.path, "effects": e, "location": a.location(x.file, x.span) }));
                }
                Ok(Value::Array(out))
            }
            // names from C in a file (`c::sqrt`): for the editors' own colour
            "foreign" => {
                let file = self.path_param(params)?;
                let a = self.analysis()?;
                let Some(fi) = a.file_index(&file) else { return Ok(json!([])) };
                let src = &a.program.files[fi].src;
                let mut out = Vec::new();
                for r in &a.index.refs {
                    let Target::Sym(i) = r.to else { continue };
                    let x = &a.index.symbols[i];
                    if r.file != fi || !x.foreign || r.span.end > src.len() {
                        continue;
                    }
                    let text = &src[r.span.start..r.span.end];
                    let kind = if x.kind == Kind::Type { "type" } else { "function" };
                    match text.rfind("::") {
                        Some(sep) => {
                            out.push(json!({ "line": r.span.line, "column": r.span.col, "length": text[..sep].chars().count(), "kind": "namespace" }));
                            let (l, c) = mimic_syntax::line_col(src, r.span.start + sep + 2);
                            out.push(json!({ "line": l, "column": c, "length": text[sep + 2..].chars().count(), "kind": kind }));
                        }
                        None => out.push(json!({ "line": r.span.line, "column": r.span.col, "length": text.chars().count(), "kind": kind })),
                    }
                }
                Ok(Value::Array(out))
            }
            "stats" => {
                self.analysis()?;
                let generation = self.generation;
                let a = self.good.as_ref().unwrap();
                let count = |k: Kind| a.index.symbols.iter().filter(|x| x.kind == k).count();
                Ok(json!({
                    "generation": generation,
                    "files": a.program.files.len(),
                    "functions": count(Kind::Fn),
                    "extern_functions": count(Kind::Extern),
                    "types": count(Kind::Type),
                    "variants": count(Kind::Variant),
                    "modules": count(Kind::Module),
                    "references": a.index.refs.len(),
                }))
            }
            _ => Err(format!(
                "unknown method `{method}` (methods: check, open, change, close, symbols, definition, hover, references, outline, docs, effects, stats)"
            )),
        }
    }

    fn analysis(&mut self) -> Result<&mut Analysis, String> {
        if self.generation == 0 {
            self.check();
        }
        match (&mut self.good, &self.errors) {
            (Some(a), _) => Ok(a),
            (None, Some(r)) => Err(format!("the program does not check yet: {}", first_error(r))),
            (None, None) => Err("nothing checked yet".into()),
        }
    }

    fn path_param(&self, params: &Value) -> Result<PathBuf, String> {
        let f = params.get("file").and_then(Value::as_str).ok_or("`file` is missing")?;
        let p = Path::new(f);
        let p = if p.is_absolute() { p.to_path_buf() } else { std::env::current_dir().map_err(|e| e.to_string())?.join(p) };
        Ok(p.canonicalize().unwrap_or(p))
    }

    /// `file`, `line` and `column` (1-based, columns in characters): the file's
    /// index in the program and the byte offset.
    fn position(&mut self, params: &Value) -> Result<(usize, usize), String> {
        let file = self.path_param(params)?;
        let line = params.get("line").and_then(Value::as_u64).ok_or("`line` is missing")? as usize;
        let col = params.get("column").and_then(Value::as_u64).unwrap_or(1) as usize;
        let a = self.analysis()?;
        let fi = a.file_index(&file).ok_or_else(|| format!("{} is not part of the program", file.display()))?;
        Ok((fi, offset_of(&a.program.files[fi].src, line, col)))
    }
}

fn first_error(r: &Report) -> String {
    r.entries.first().map(|e| e.diag.msg.clone()).unwrap_or_default()
}

/// How well `q` matches a symbol: 0 exact path, 1 exact name, 2 prefix,
/// 3 contains; `None` when it does not.
fn match_rank(q: &str, x: &Symbol) -> Option<u8> {
    if q.is_empty() {
        return Some(3);
    }
    let (path, name) = (x.path.to_lowercase(), x.name.to_lowercase());
    if path == q {
        Some(0)
    } else if name == q {
        Some(1)
    } else if name.starts_with(q) || path.starts_with(q) {
        Some(2)
    } else if path.contains(q) {
        Some(3)
    } else {
        None
    }
}

/// Byte offset of 1-based `line` and `col` (in characters) in `src`.
fn offset_of(src: &str, line: usize, col: usize) -> usize {
    let mut start = 0;
    for _ in 1..line {
        match src[start..].find('\n') {
            Some(i) => start += i + 1,
            None => return src.len(),
        }
    }
    let rest = &src[start..];
    let end = rest.find('\n').unwrap_or(rest.len());
    start + rest[..end].char_indices().nth(col.saturating_sub(1)).map_or(end, |(i, _)| i)
}

impl Analysis {
    fn file_index(&self, file: &Path) -> Option<usize> {
        self.program.files.iter().position(|f| f.path == file || f.path.canonicalize().is_ok_and(|c| c == file))
    }

    fn find(&self, name: &str) -> Option<usize> {
        let syms = &self.index.symbols;
        syms.iter()
            .position(|x| x.path == name)
            .or_else(|| syms.iter().position(|x| x.package == self.program.root_package && x.name == name))
            .or_else(|| syms.iter().position(|x| x.name == name && x.kind != Kind::Module))
    }

    /// What the name at byte `at` of file `file` refers to.
    fn target_at(&self, file: usize, at: usize) -> Option<Target> {
        let best = self
            .index
            .refs
            .iter()
            .filter(|r| r.file == file && r.span.start <= at && at < r.span.end.max(r.span.start + 1))
            .min_by_key(|r| r.span.end - r.span.start);
        if let Some(r) = best {
            return Some(r.to.clone());
        }
        // on a definition's own name
        self.index
            .symbols
            .iter()
            .position(|x| x.file == file && x.kind != Kind::Module && x.span.start <= at && at < x.span.end)
            .map(Target::Sym)
    }

    fn location(&self, file: usize, span: Span) -> Value {
        let f = &self.program.files[file];
        let (end_line, end_col) = mimic_syntax::line_col(&f.src, span.end);
        json!({
            "file": if f.path.is_file() { f.path.display().to_string() } else { f.display.clone() },
            "shown": f.display,
            "line": span.line,
            "column": span.col,
            "end_line": end_line,
            "end_column": end_col,
        })
    }

    fn symbol_json(&self, x: &Symbol) -> Value {
        let mut v = json!({
            "name": x.name,
            "kind": x.kind.name(),
            "path": x.path,
            "package": x.package,
            "public": x.public,
            "signature": x.signature,
            "doc": x.doc,
            "location": self.location(x.file, x.span),
        });
        if let Some(h) = &x.header {
            v["header"] = h.clone().into();
        }
        v
    }

    /// What a function does besides computing its result: `pure` (may run in
    /// parallel), `concurrent` (effects that may overlap: reading, waiting),
    /// `ordered` (effects that keep their order, such as printing) or `sync`
    /// (waits for pending work first: writing files, running programs).
    fn effect_of(&mut self, sym: usize) -> Option<&'static str> {
        let f = self.index.symbols[sym].fn_id?;
        if self.effects.is_none() {
            let pu = Purity::analyze(&self.program);
            let e = (0..self.program.fns.len())
                .map(|i| {
                    if pu.fns[i] {
                        "pure"
                    } else if pu.overlap[i] {
                        "concurrent"
                    } else if pu.calm[i] {
                        "ordered"
                    } else {
                        "sync"
                    }
                })
                .collect();
            self.effects = Some(e);
        }
        self.effects.as_ref().map(|e| e[f])
    }
}

/// The span of `name` itself inside a declaration starting at `decl`.
fn name_span(src: &str, decl: Span, name: &str) -> Option<Span> {
    let from = decl.start.min(src.len());
    let from = src.floor_char_boundary(from);
    let window = &src[from..src.floor_char_boundary(src.len().min(from + 400))];
    let mut search = 0;
    while let Some(i) = window[search..].find(name) {
        let at = search + i;
        let before = window[..at].chars().next_back();
        let after = window[at + name.len()..].chars().next();
        let word = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric() || c == '_');
        if !word(before) && !word(after) {
            // line and column counted on from the declaration's own
            let (mut line, mut col) = (decl.line, decl.col);
            if from != decl.start {
                (line, col) = mimic_syntax::line_col(src, from);
            }
            for c in window[..at].chars() {
                if c == '\n' {
                    line += 1;
                    col = 1;
                } else {
                    col += 1;
                }
            }
            return Some(Span { start: from + at, end: from + at + name.len(), line, col });
        }
        search = at + name.len();
    }
    None
}

// ---- building the index -----------------------------------------------------

impl Index {
    fn build(p: &Program) -> Index {
        let mut ix = Index::default();
        let headers = c_headers(p);
        let lines: Vec<Vec<&str>> = p.files.iter().map(|f| f.src.lines().collect()).collect();
        for (m, md) in p.mods.iter().enumerate() {
            if md.parent.is_none() {
                continue; // a package root: named by its package
            }
            let file = md.file.unwrap_or(0);
            let doc = lines.get(file).map(|l| comment_above(l, md.span.line)).unwrap_or_default();
            let id = ix.push(Symbol {
                name: md.name.clone(),
                kind: Kind::Module,
                path: mod_path(p, m),
                package: md.package.clone(),
                public: md.is_pub,
                file,
                span: md.span,
                signature: format!("module {}", md.name),
                doc,
                header: None,
                fn_id: None,
                foreign: false,
            });
            ix.by_mod.insert(m, id);
        }
        for (fid, f) in p.fns.iter().enumerate() {
            let src = &p.files[f.file].src;
            let path = join(&mod_path(p, f.module), &f.name);
            let (kind, signature, header) = match &f.kind {
                FnKind::User(d) => (Kind::Fn, doc::fn_sig(d, src), None),
                FnKind::Extern { decl, .. } => {
                    let ps: Vec<String> = decl.params.iter().map(|(n, t)| format!("{n}: {}", doc::show_type(t))).collect();
                    let va = if decl.variadic { ", ..." } else { "" };
                    let ret = decl.ret.as_ref().map(|r| format!(" -> {}", doc::show_type(r))).unwrap_or_default();
                    let header = headers.get(&(f.file, f.span.start)).cloned();
                    (Kind::Extern, format!("fn {}({}{va}){ret}", f.name, ps.join(", ")), header)
                }
            };
            // functions made by the compiler (lifted `arms`, wrappers) are not shown
            if f.name.contains('·') || f.name.starts_with('$') {
                continue;
            }
            let doc = if header.is_some() { String::new() } else { comment_above(&lines[f.file], f.span.line) };
            let id = ix.push(Symbol {
                name: f.name.clone(),
                kind,
                path,
                package: p.mods[f.module].package.clone(),
                public: f.is_pub,
                file: f.file,
                span: f.span,
                signature,
                doc,
                header,
                fn_id: Some(fid),
                foreign: p.mods[f.module].foreign,
            });
            ix.by_fn.insert(fid, id);
        }
        for (tid, t) in p.types.iter().enumerate() {
            let base = mod_path(p, t.module);
            let id = ix.push(Symbol {
                name: t.name.clone(),
                kind: Kind::Type,
                path: join(&base, &t.name),
                package: p.mods[t.module].package.clone(),
                public: t.is_pub,
                file: t.file,
                span: t.decl.span,
                signature: doc::type_sig(&t.decl),
                doc: comment_above(&lines[t.file], t.decl.span.line),
                header: None,
                fn_id: None,
                foreign: p.mods[t.module].foreign,
            });
            ix.by_type.insert(tid, id);
            for (vi, v) in t.variants().iter().enumerate() {
                let sig = if v.fields.is_empty() {
                    v.name.clone()
                } else {
                    format!("{}({})", v.name, v.fields.iter().map(doc::show_type).collect::<Vec<_>>().join(", "))
                };
                let id = ix.push(Symbol {
                    name: v.name.clone(),
                    kind: Kind::Variant,
                    path: join(&base, &v.name),
                    package: p.mods[t.module].package.clone(),
                    public: t.is_pub,
                    file: t.file,
                    span: v.span,
                    signature: format!("{sig}: {}", t.name),
                    doc: String::new(),
                    header: None,
                    fn_id: None,
                    foreign: p.mods[t.module].foreign,
                });
                ix.by_variant.insert((tid, vi), id);
            }
        }
        // a symbol is located at its name, not the start of its declaration
        for x in &mut ix.symbols {
            if let Some(f) = p.files.get(x.file)
                && let Some(n) = name_span(&f.src, x.span, &x.name)
            {
                x.span = n;
            }
        }
        // references, from every function body and signature
        for (fid, f) in p.fns.iter().enumerate() {
            let FnKind::User(d) = &f.kind else { continue };
            let mut w = RefWalk { p, ix: &mut ix, module: f.module, file: f.file, scopes: vec![Vec::new()] };
            for prm in &d.params {
                if let Some(t) = &prm.ty {
                    w.ty(t);
                }
                let def = name_span(&p.files[f.file].src, prm.span, &prm.name).unwrap_or(prm.span);
                w.bind(&prm.name, def);
            }
            if let Some(t) = &d.ret {
                w.ty(t);
            }
            w.block(&d.body);
            let _ = fid;
        }
        for t in &p.types {
            let mut w = RefWalk { p, ix: &mut ix, module: t.module, file: t.file, scopes: vec![Vec::new()] };
            match &t.decl.kind {
                TypeKind::Record(fs) => fs.iter().for_each(|(_, ft, _)| w.ty(ft)),
                TypeKind::Union(vs) => vs.iter().flat_map(|v| &v.fields).for_each(|ft| w.ty(ft)),
            }
        }
        ix
    }

    fn push(&mut self, s: Symbol) -> usize {
        self.symbols.push(s);
        self.symbols.len() - 1
    }

    fn of(&self, r: Resolved) -> Option<usize> {
        match r {
            Resolved::Fn(f) => self.by_fn.get(&f).copied(),
            Resolved::Type(t) => self.by_type.get(&t).copied(),
            Resolved::Variant(t, v) => self.by_variant.get(&(t, v)).copied(),
            Resolved::Mod(m) => self.by_mod.get(&m).copied(),
        }
    }
}

fn join(base: &str, name: &str) -> String {
    if base.is_empty() { name.to_string() } else { format!("{base}::{name}") }
}

/// A module's path as code writes it: `fs`, `c`, `geometry::shapes`. The
/// standard library's modules sit in its prelude, which code never names;
/// the root module of a package is named by the package only from outside.
fn mod_path(p: &Program, m: ModId) -> String {
    let mut segs = Vec::new();
    let mut cur = Some(m);
    while let Some(c) = cur {
        let md = &p.mods[c];
        if md.parent.is_none() {
            break;
        }
        segs.push(md.name.clone());
        cur = md.parent;
    }
    segs.reverse();
    let pkg = &p.mods[m].package;
    if pkg == "std" {
        if segs.first().is_some_and(|s| s == "prelude") {
            segs.remove(0);
        }
    } else if pkg != &p.root_package {
        segs.insert(0, pkg.clone());
    } else if segs.first().is_some_and(|s| s == "lib") {
        // a library's `lib` module is named by the package: `showcase::banner`
        segs[0] = pkg.clone();
    } else if segs.first().is_some_and(|s| s == "main") {
        // the program's `main` module: its names are written bare
        segs.remove(0);
    }
    segs.join("::")
}

/// `(file, start of the include)` → header, for functions imported from C
/// (they carry the span of their `include`).
fn c_headers(p: &Program) -> HashMap<(usize, usize), String> {
    fn items(items: &[Item], file: usize, out: &mut HashMap<(usize, usize), String>) {
        for it in items {
            match it {
                Item::Extern(b) => {
                    for (h, s) in &b.includes {
                        out.insert((file, s.start), h.clone());
                    }
                }
                Item::Module(m) => items_of(m, file, out),
                _ => {}
            }
        }
    }
    fn items_of(m: &ModuleDecl, file: usize, out: &mut HashMap<(usize, usize), String>) {
        items(&m.items, file, out);
    }
    let mut out = HashMap::new();
    for (fi, f) in p.files.iter().enumerate() {
        items_of(&f.ast.module, fi, &mut out);
    }
    out
}

/// The `//` lines right above line `line` (1-based), as `mimic doc` reads them.
fn comment_above(lines: &[&str], line: u32) -> String {
    let mut k = line as usize;
    let mut out = Vec::new();
    while k >= 2 {
        let l = lines.get(k - 2).map(|l| l.trim()).unwrap_or("");
        let Some(text) = l.strip_prefix("//") else { break };
        out.push(text.strip_prefix(' ').unwrap_or(text));
        k -= 1;
    }
    out.reverse();
    out.join("\n")
}

struct RefWalk<'a> {
    p: &'a Program,
    ix: &'a mut Index,
    module: ModId,
    file: usize,
    /// names in scope, innermost last, each with where it was bound
    scopes: Vec<Vec<(String, Span)>>,
}

impl RefWalk<'_> {
    fn src(&self) -> &str {
        &self.p.files[self.file].src
    }

    fn bind(&mut self, name: &str, def: Span) {
        if name != "_" {
            self.scopes.last_mut().unwrap().push((name.to_string(), def));
        }
    }

    fn local(&self, name: &str) -> Option<Span> {
        self.scopes.iter().rev().flat_map(|s| s.iter().rev()).find(|(n, _)| n == name).map(|(_, d)| *d)
    }

    fn add(&mut self, span: Span, to: Target) {
        self.ix.refs.push(Ref { file: self.file, span, to });
    }

    fn path(&mut self, path: &[String], span: Span) {
        if path.len() == 1
            && let Some(def) = self.local(&path[0])
        {
            self.add(span, Target::Local { name: path[0].clone(), def });
            return;
        }
        if let Some(r) = self.p.lookup(self.module, path)
            && let Some(s) = self.ix.of(r)
        {
            self.add(span, Target::Sym(s));
        }
    }

    fn ty(&mut self, t: &Type) {
        match t {
            Type::Named(n, s) => {
                if let Some(r @ Resolved::Type(_)) = self.p.lookup(self.module, &mimic_check::type_path(n))
                    && let Some(x) = self.ix.of(r)
                {
                    self.add(*s, Target::Sym(x));
                }
            }
            Type::App(n, args, s) => {
                if let Some(r @ Resolved::Type(_)) = self.p.lookup(self.module, &mimic_check::type_path(n))
                    && let Some(x) = self.ix.of(r)
                {
                    let name_end = (s.start + n.len()).min(s.end);
                    self.add(Span { end: name_end, ..*s }, Target::Sym(x));
                }
                args.iter().for_each(|a| self.ty(a));
            }
            Type::Array(t, _) | Type::Ptr(t, _) => self.ty(t),
            Type::Map(k, v, _) => {
                self.ty(k);
                self.ty(v);
            }
            Type::Tuple(ts, _) => ts.iter().for_each(|t| self.ty(t)),
            Type::Fn(ps, r, _) => {
                ps.iter().for_each(|t| self.ty(t));
                if let Some(r) = r {
                    self.ty(r);
                }
            }
        }
    }

    fn block(&mut self, b: &Block) {
        self.scopes.push(Vec::new());
        for s in &b.stmts {
            match s {
                Stmt::Let { name, ty, value, span, .. } => {
                    if let Some(t) = ty {
                        self.ty(t);
                    }
                    self.expr(value);
                    let def = name_span(self.src(), *span, name).unwrap_or(*span);
                    self.bind(name, def);
                }
                Stmt::LetElse { name, ty, value, els, span } => {
                    if let Some(t) = ty {
                        self.ty(t);
                    }
                    self.expr(value);
                    self.block(els);
                    let def = name_span(self.src(), *span, name).unwrap_or(*span);
                    self.bind(name, def);
                }
                Stmt::LetTuple { names, value, span } => {
                    self.expr(value);
                    for n in names {
                        let def = name_span(self.src(), *span, n).unwrap_or(*span);
                        self.bind(n, def);
                    }
                }
                Stmt::Assign { name, index, value, span } => {
                    if let Some(def) = self.local(name) {
                        let at = name_span(self.src(), *span, name).unwrap_or(*span);
                        self.add(at, Target::Local { name: name.clone(), def });
                    }
                    if let Some(i) = index {
                        self.expr(i);
                    }
                    self.expr(value);
                }
                Stmt::Expr(e) => self.expr(e),
                Stmt::Return(e, _) => {
                    if let Some(e) = e {
                        self.expr(e);
                    }
                }
            }
        }
        if let Some(t) = &b.tail {
            self.expr(t);
        }
        self.scopes.pop();
    }

    fn pattern(&mut self, pat: &Pattern) {
        match pat {
            Pattern::Wild(_) => {}
            Pattern::Bind(n, s) => self.bind(n, *s),
            Pattern::Lit(e) => self.expr(e),
            Pattern::Variant { path, args, span } => {
                let name_end = (span.start + path.join("::").len()).min(span.end);
                self.path(path, Span { end: name_end, ..*span });
                args.iter().for_each(|a| self.pattern(a));
            }
            Pattern::Tuple(ps, _) | Pattern::Or(ps, _) => ps.iter().for_each(|x| self.pattern(x)),
            Pattern::Route(parts, s) => {
                for part in parts {
                    if let RoutePart::Capture { name, .. } = part {
                        let def = name_span(self.src(), *s, name).unwrap_or(*s);
                        self.bind(name, def);
                    }
                }
            }
            Pattern::Range { lo, hi, .. } => {
                self.expr(lo);
                self.expr(hi);
            }
            Pattern::As(inner, name, s) => {
                self.pattern(inner);
                let def = name_span(self.src(), *s, name).unwrap_or(*s);
                self.bind(name, def);
            }
            Pattern::Record { path, fields, span, .. } => {
                let name_end = (span.start + path.join("::").len()).min(span.end);
                self.path(path, Span { end: name_end, ..*span });
                fields.iter().for_each(|(_, x)| self.pattern(x));
            }
            Pattern::Array { elems, rest, span } => {
                elems.iter().for_each(|x| self.pattern(x));
                if let Some(Some(n)) = rest {
                    let def = name_span(self.src(), *span, n).unwrap_or(*span);
                    self.bind(n, def);
                }
            }
        }
    }

    fn expr(&mut self, e: &Expr) {
        match &e.kind {
            ExprKind::Int(_) | ExprKind::Float(_) | ExprKind::Str(_) | ExprKind::Bool(_) | ExprKind::OpRef(_) => {}
            ExprKind::Path(path) => self.path(path, e.span),
            ExprKind::TypeArg(t) => self.ty(t),
            ExprKind::Array(xs) | ExprKind::Tuple(xs) => xs.iter().for_each(|x| self.expr(x)),
            ExprKind::Call { callee, args } => {
                self.expr(callee);
                args.iter().for_each(|a| self.expr(a));
            }
            ExprKind::Index { base, index } => {
                self.expr(base);
                self.expr(index);
            }
            ExprKind::Unary { expr, .. } => self.expr(expr),
            ExprKind::Binary { lhs, rhs, .. } => {
                self.expr(lhs);
                self.expr(rhs);
            }
            ExprKind::Prim { args, .. } => args.iter().for_each(|a| self.expr(a)),
            ExprKind::Lambda { params, body } => {
                self.scopes.push(Vec::new());
                for prm in params {
                    // made by the compiler for a constructor used as a value
                    if prm.contains('·') {
                        continue;
                    }
                    let def = name_span(self.src(), e.span, prm).unwrap_or(e.span);
                    self.bind(prm, def);
                }
                self.expr(body);
                self.scopes.pop();
            }
            ExprKind::Map(kvs) => kvs.iter().for_each(|(k, v)| {
                self.expr(k);
                self.expr(v);
            }),
            ExprKind::Arms { params, body } => {
                self.scopes.push(Vec::new());
                for (n, v) in params {
                    self.expr(v);
                    self.bind(n, e.span);
                }
                self.block(body);
                self.scopes.pop();
            }
            ExprKind::Field { base, .. } => self.expr(base),
            ExprKind::Record { path, fields } => {
                let name_end = (e.span.start + path.join("::").len()).min(e.span.end);
                self.path(path, Span { end: name_end, ..e.span });
                fields.iter().for_each(|(_, v, _)| self.expr(v));
            }
            ExprKind::With { base, fields } => {
                self.expr(base);
                fields.iter().for_each(|(_, v, _)| self.expr(v));
            }
            ExprKind::Match { scrut, arms } => {
                self.expr(scrut);
                for a in arms {
                    self.scopes.push(Vec::new());
                    self.pattern(&a.pat);
                    if let Some(g) = &a.guard {
                        self.expr(g);
                    }
                    self.expr(&a.body);
                    self.scopes.pop();
                }
            }
            ExprKind::Interp(parts) => parts.iter().for_each(|p| {
                if let InterpPart::Expr(x) = p {
                    self.expr(x);
                }
            }),
            ExprKind::Named { value, .. } => self.expr(value),
            ExprKind::If { cond, then, els } => {
                self.expr(cond);
                self.block(then);
                if let Some(e) = els {
                    self.expr(e);
                }
            }
            ExprKind::Block(b) | ExprKind::Unsafe(b) => self.block(b),
        }
    }
}

// ---- the standard library, for the manual ------------------------------------

/// A public item of the standard library, as `mimic man` shows it.
pub struct StdItem {
    /// As code writes it: `fs::read`, `sort_by`, `Option`.
    pub path: String,
    /// `fn`, `extern fn`, `type`, `variant`, `module`
    pub kind: &'static str,
    pub signature: String,
    pub doc: String,
    /// For functions: `pure`, `concurrent`, `ordered` or `sync`.
    pub effects: Option<&'static str>,
}

/// Every public item of the standard library.
pub fn std_items() -> Vec<StdItem> {
    let (p, _) = Program::build(vec![driver::prelude_file()], "manual");
    let index = Index::build(&p);
    let pu = Purity::analyze(&p);
    let effect = |f: usize| {
        if pu.fns[f] {
            "pure"
        } else if pu.overlap[f] {
            "concurrent"
        } else if pu.calm[f] {
            "ordered"
        } else {
            "sync"
        }
    };
    index
        .symbols
        .into_iter()
        .filter(|x| x.package == "std" && x.public && !x.path.is_empty() && x.path != "prelude")
        .map(|x| StdItem {
            effects: x.fn_id.map(effect),
            path: x.path,
            kind: x.kind.name(),
            signature: x.signature,
            doc: x.doc,
        })
        .collect()
}

// ---- `mimic query` ----------------------------------------------------------

/// `key=value` words as a JSON object (numbers and `true`/`false` as such;
/// a value starting with `{`, `[` or `"` is read as JSON).
pub fn params_from_words(words: &[String]) -> Result<Value, String> {
    let mut obj = serde_json::Map::new();
    for w in words {
        let (k, v) = w.split_once('=').ok_or_else(|| format!("`{w}`: write parameters as key=value"))?;
        let val = if v.starts_with(['{', '[', '"']) {
            serde_json::from_str(v).map_err(|e| format!("`{k}`: {e}"))?
        } else if let Ok(n) = v.parse::<u64>() {
            n.into()
        } else if v == "true" || v == "false" {
            (v == "true").into()
        } else {
            v.into()
        };
        obj.insert(k.to_string(), val);
    }
    Ok(Value::Object(obj))
}

/// Answer requests read from stdin, one JSON object per line:
/// `{"id": 1, "method": "definition", "params": {…}}`. Each answer is one
/// line: `{"id": 1, "result": …}` or `{"id": 1, "error": "…"}`.
pub fn serve(engine: &mut Engine) {
    use std::io::{BufRead, Write};
    let stdin = std::io::stdin();
    let mut out = std::io::stdout();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<Value>(&line) {
            Ok(req) => {
                let id = req.get("id").cloned().unwrap_or(Value::Null);
                let method = req.get("method").and_then(Value::as_str).unwrap_or("");
                if method == "exit" {
                    break;
                }
                let params = req.get("params").cloned().unwrap_or(json!({}));
                match engine.query(method, &params) {
                    Ok(r) => json!({ "id": id, "result": r }),
                    Err(e) => json!({ "id": id, "error": e }),
                }
            }
            Err(e) => json!({ "id": null, "error": format!("not JSON: {e}") }),
        };
        let _ = writeln!(out, "{reply}");
        let _ = out.flush();
    }
}
