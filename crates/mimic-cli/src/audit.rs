//! `mimic audit`: quality and security findings about a program.
//!
//! Every rule has an id (`Q…` quality, `S…` security), a severity (error,
//! warning, note) and a page in the manual's audit section. Findings have a
//! fingerprint (rule, file and the code itself, not its line), so a baseline
//! keeps only new findings counting while code moves around.

use mimic_check::{FnKind, Program, Resolved};
use mimic_syntax::Span;
use mimic_syntax::ast::*;
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};

pub struct Rule {
    pub id: &'static str,
    pub name: &'static str,
    pub category: &'static str,
    pub severity: &'static str,
    pub help: &'static str,
}

pub const RULES: &[Rule] = &[
    Rule { id: "Q01", name: "unused-function", category: "quality", severity: "warning", help: "A private function nothing calls. Remove it, or make it `pub` if other code is meant to use it." },
    Rule { id: "Q02", name: "unused-variable", category: "quality", severity: "warning", help: "A `let` whose value is never used. Remove it, or start its name with `_` if computing it is the point." },
    Rule { id: "Q03", name: "unchecked-result", category: "quality", severity: "warning", help: "A call returning a `Result` whose value is dropped, so a failure goes unnoticed. Handle it with `match` or `let … else`, or bind it to `_` on purpose." },
    Rule { id: "Q04", name: "long-function", category: "quality", severity: "note", help: "A function longer than 60 lines. Splitting it into named steps makes each easier to read and test." },
    Rule { id: "Q05", name: "complex-function", category: "quality", severity: "note", help: "A function with more than 15 branches (`if`, `match` arms, `and`, `or`). Smaller functions, or a `match` over a union, usually read better." },
    Rule { id: "Q06", name: "duplicated-code", category: "quality", severity: "note", help: "Two functions with the same body. Keep one and call it from both places." },
    Rule { id: "Q07", name: "wider-effects", category: "quality", severity: "note", help: "C functions that only take and return numbers, declared with C's default effect (`sync`): if they have no effects (as in math.h), `extern \"C\" pure` lets calls run in parallel." },
    Rule { id: "Q08", name: "trusted-pointer", category: "quality", severity: "warning", help: "A `trusted` C function that takes a pointer or a C string: `trusted` says calling it can never break memory safety, which a pointer argument usually cannot promise. Leave it untrusted (calls need `unsafe`) unless you have checked it." },
    Rule { id: "S01", name: "shell-injection", category: "security", severity: "error", help: "`os::shell` with a command built from data: a value containing `;` or `$(…)` runs other commands. Use `os::run(program, [arguments])`, which passes arguments without a shell." },
    Rule { id: "S02", name: "path-from-request", category: "security", severity: "error", help: "A file opened with a path taken from an HTTP request: `../` in it reaches any file. Map requests to known files, or check the path with `path::` first." },
    Rule { id: "S03", name: "secret-in-source", category: "security", severity: "error", help: "A password, token or private key written in the code: anyone with the source (or the program) has it. Read it from the environment (`os::env`) or a file kept out of version control." },
    Rule { id: "S04", name: "certificate-trust", category: "security", severity: "warning", help: "Trusting one certificate (`tls::connect_trusting`) or making one on the spot (`tls::self_signed`) outside tests skips checking certificates against the system's trusted authorities. Fine for tests and local tools; elsewhere use `tls::connect` and a real certificate." },
    Rule { id: "S05", name: "unsafe-block", category: "security", severity: "note", help: "An `unsafe` block: a call into C that Mimic cannot check. Keep these few and small, and check each C function's contract." },
    Rule { id: "S06", name: "trusted-header", category: "security", severity: "warning", help: "`trusted extern \"C\" include` trusts every function of a header as memory safe, including ones that take pointers. Trust single functions in a hand-written `trusted extern \"C\" { … }` block instead." },
    Rule { id: "S07", name: "branch-dependency", category: "security", severity: "warning", help: "A dependency that follows a branch: whoever controls the repository changes your program on the next install. Pin a tag or a commit: `rev = \"v1.2.0\"`." },
];

pub fn rule(id: &str) -> &'static Rule {
    RULES.iter().find(|r| r.id == id).expect("a known rule")
}

pub struct Finding {
    pub rule: &'static Rule,
    pub file: usize,
    pub span: Span,
    pub message: String,
}

struct Audit<'p> {
    p: &'p Program,
    out: Vec<Finding>,
}

impl Audit<'_> {
    fn add(&mut self, id: &str, file: usize, span: Span, message: String) {
        self.out.push(Finding { rule: rule(id), file, span, message });
    }
}

/// Every finding about the program's own files.
pub fn run(p: &Program) -> Vec<Finding> {
    let mut a = Audit { p, out: Vec::new() };
    let mine = |f: &mimic_check::Function| p.mods[f.module].package == p.root_package;

    // which functions are called (or used as values) anywhere
    let mut used: HashSet<usize> = HashSet::new();
    for f in &p.fns {
        let FnKind::User(d) = &f.kind else { continue };
        let mut visit = |e: &Expr| {
            if let ExprKind::Path(path) = &e.kind
                && let Some(Resolved::Fn(g)) = p.lookup(f.module, path)
            {
                used.insert(g);
            }
        };
        d.body.walk(&mut visit);
        for prm in &d.params {
            if let Some(dv) = &prm.default {
                dv.walk(&mut visit);
            }
        }
    }
    let mut bodies: HashMap<String, (usize, Span, String)> = HashMap::new();
    for (fid, f) in p.fns.iter().enumerate() {
        let FnKind::User(d) = &f.kind else { continue };
        if !mine(f) || f.name.contains('·') || f.name.starts_with('$') {
            continue;
        }
        let src = &p.files[f.file].src;
        let is_test = f.name.starts_with("test ") || p.files[f.file].path.file_name().is_some_and(|n| n == "test.mim");
        // Q01: a private function nothing uses
        if !f.is_pub && !used.contains(&fid) && f.name != "main" && !is_test && !d.export_c {
            a.add("Q01", f.file, f.span, format!("`{}` is never used", f.name));
        }
        // Q02: lets never read
        unused_lets(&mut a, f.file, &d.body, src);
        // Q03: dropped Results
        unchecked(&mut a, f.module, f.file, &d.body);
        // Q04, Q05: size and branches
        let body_text = src.get(d.span.start..d.body.span_end(src)).unwrap_or("");
        let lines = body_text.lines().count();
        if lines > 60 {
            a.add("Q04", f.file, f.span, format!("`{}` is {lines} lines long", f.name));
        }
        let mut branches = 0;
        d.body.walk(&mut |e: &Expr| match &e.kind {
            ExprKind::If { .. } => branches += 1,
            ExprKind::Match { arms, .. } => branches += arms.len().saturating_sub(1),
            ExprKind::Binary { op: BinOp::And | BinOp::Or, .. } => branches += 1,
            _ => {}
        });
        if branches > 15 {
            a.add("Q05", f.file, f.span, format!("`{}` has {branches} branches", f.name));
        }
        // Q06: the same body as another function
        if lines >= 3 && !is_test {
            let body = src.get(body_start(src, d.span)..d.body.span_end(src)).unwrap_or("");
            let norm: String = body.split_whitespace().collect::<Vec<_>>().join(" ");
            if norm.len() >= 40 {
                match bodies.get(&norm) {
                    Some((_, _, other)) => {
                        let other = other.clone();
                        a.add("Q06", f.file, f.span, format!("`{}` has the same body as `{other}`", f.name));
                    }
                    None => {
                        bodies.insert(norm, (f.file, f.span, f.name.clone()));
                    }
                }
            }
        }
        // security in the body
        let params: Vec<&Param> = d.params.iter().collect();
        security(&mut a, f, d, &params, is_test, src);
    }
    // extern blocks: Q07, Q08, S06
    for (fi, file) in p.files.iter().enumerate() {
        if file.package != p.root_package {
            continue;
        }
        externs(&mut a, fi, &file.ast.module.items);
    }
    a.out.sort_by_key(|f| (f.file, f.span.start, f.rule.id));
    a.out
}

/// Where a function's body starts (its `{`), for comparing bodies.
fn body_start(src: &str, decl: Span) -> usize {
    src[decl.start.min(src.len())..].find('{').map_or(decl.start, |i| decl.start + i)
}

trait BlockEnd {
    fn span_end(&self, src: &str) -> usize;
}

impl BlockEnd for Block {
    /// The end of a block's last statement or tail (blocks keep no span).
    fn span_end(&self, src: &str) -> usize {
        let end = std::cell::Cell::new(0usize);
        let visit = |e: &Expr| e.walk(&mut |x: &Expr| end.set(end.get().max(x.span.end)));
        for s in &self.stmts {
            match s {
                Stmt::Let { value, span, .. } | Stmt::Assign { value, span, .. } | Stmt::LetTuple { value, span, .. } | Stmt::LetElse { value, span, .. } => {
                    end.set(end.get().max(span.end));
                    visit(value);
                }
                Stmt::Expr(e) => visit(e),
                Stmt::Return(e, span) => {
                    end.set(end.get().max(span.end));
                    if let Some(e) = e {
                        visit(e);
                    }
                }
            }
        }
        if let Some(t) = &self.tail {
            visit(t);
        }
        let end = end.get();
        // up to the closing `}` of the line
        let rest = &src[end.min(src.len())..];
        end + rest.find('}').map_or(0, |i| i + 1)
    }
}

fn unused_lets(a: &mut Audit, file: usize, body: &Block, src: &str) {
    let mut lets: Vec<(String, Span)> = Vec::new();
    let mut uses: HashMap<String, usize> = HashMap::new();
    fn count(e: &Expr, uses: &mut HashMap<String, usize>) {
        e.walk(&mut |x: &Expr| {
            if let ExprKind::Path(p) = &x.kind
                && p.len() == 1
            {
                *uses.entry(p[0].clone()).or_default() += 1;
            }
        })
    }
    fn collect(b: &Block, lets: &mut Vec<(String, Span)>, uses: &mut HashMap<String, usize>) {
        for s in &b.stmts {
            match s {
                Stmt::Let { name, value, span, .. } => {
                    lets.push((name.clone(), *span));
                    count(value, uses);
                }
                Stmt::LetElse { name, value, els, span, .. } => {
                    lets.push((name.clone(), *span));
                    count(value, uses);
                    collect(els, lets, uses);
                }
                // reassigning is not reading, unless the index reads it
                Stmt::Assign { index, value, .. } => {
                    if let Some(i) = index {
                        count(i, uses);
                    }
                    count(value, uses);
                }
                Stmt::LetTuple { value, .. } | Stmt::Expr(value) => count(value, uses),
                Stmt::Return(e, _) => {
                    if let Some(e) = e {
                        count(e, uses);
                    }
                }
            }
        }
        if let Some(t) = &b.tail {
            count(t, uses);
        }
    }
    collect(body, &mut lets, &mut uses);
    // (nested blocks inside expressions are walked by `walk`; their lets are
    // counted where they are read)
    for (name, span) in lets {
        if !name.starts_with('_') && uses.get(&name).copied().unwrap_or(0) == 0 && !src[span.start.min(src.len())..].starts_with("let _") {
            a.add("Q02", file, span, format!("`{name}` is never used"));
        }
    }
}

fn returns_result(p: &Program, module: usize, callee: &Expr) -> Option<String> {
    let ExprKind::Path(path) = &callee.kind else { return None };
    let Some(Resolved::Fn(f)) = p.lookup(module, path) else { return None };
    let ret = match &p.fns[f].kind {
        FnKind::User(d) => d.ret.as_ref(),
        FnKind::Extern { decl, .. } => decl.ret.as_ref(),
    }?;
    match ret {
        Type::App(n, ..) if n == "Result" => Some(path.join("::")),
        _ => None,
    }
}

fn unchecked(a: &mut Audit, module: usize, file: usize, body: &Block) {
    let p = a.p;
    let mut found = Vec::new();
    let visit_block = |b: &Block, found: &mut Vec<(Span, String)>| {
        for s in &b.stmts {
            if let Stmt::Expr(e) = s
                && let ExprKind::Call { callee, .. } = &e.kind
                && let Some(name) = returns_result(p, module, callee)
            {
                found.push((e.span, name));
            }
        }
    };
    visit_block(body, &mut found);
    for s in &body.stmts {
        let e = match s {
            Stmt::Let { value, .. } | Stmt::LetElse { value, .. } | Stmt::LetTuple { value, .. } | Stmt::Expr(value) | Stmt::Assign { value, .. } => Some(value),
            Stmt::Return(e, _) => e.as_ref(),
        };
        if let Some(e) = e {
            e.walk(&mut |x: &Expr| {
                if let ExprKind::Block(b) | ExprKind::Unsafe(b) = &x.kind {
                    visit_block(b, &mut found);
                }
                if let ExprKind::If { then, .. } = &x.kind {
                    visit_block(then, &mut found);
                }
            });
        }
    }
    for (span, name) in found {
        a.add("Q03", file, span, format!("the `Result` of `{name}` is dropped: a failure would go unnoticed"));
    }
}

/// Text built from data: an interpolation, a `+` with anything but literals,
/// or a variable.
fn built_from_data(e: &Expr) -> bool {
    match &e.kind {
        ExprKind::Str(_) => false,
        ExprKind::Interp(parts) => parts.iter().any(|p| matches!(p, InterpPart::Expr(_))),
        ExprKind::Binary { op: BinOp::Add, lhs, rhs } => built_from_data(lhs) || built_from_data(rhs),
        _ => true,
    }
}

fn mentions(e: &Expr, name: &str) -> bool {
    let mut hit = false;
    e.walk(&mut |x: &Expr| {
        if let ExprKind::Path(p) = &x.kind
            && p.len() == 1
            && p[0] == name
        {
            hit = true;
        }
    });
    hit
}

const SECRET_NAMES: &[&str] = &["password", "passwd", "secret", "token", "api_key", "apikey", "private_key", "access_key"];

fn looks_secret(s: &str) -> bool {
    let aws = s.len() >= 20 && s.starts_with("AKIA") && s[4..20].chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit());
    let pem = s.contains("-----BEGIN") && s.contains("PRIVATE KEY");
    let gh = ["ghp_", "gho_", "ghs_", "github_pat_"].iter().any(|p| s.starts_with(p) && s.len() >= 30);
    let slack = s.starts_with("xox") && s.len() > 20;
    let stripe = s.starts_with("sk_live_");
    aws || pem || gh || slack || stripe
}

fn security(a: &mut Audit, f: &mimic_check::Function, d: &FnDecl, params: &[&Param], is_test: bool, _src: &str) {
    let p = a.p;
    let request_params: Vec<String> = params
        .iter()
        .filter(|q| matches!(&q.ty, Some(Type::Named(n, _)) if n == "http::Request" || n == "Request"))
        .map(|q| q.name.clone())
        .collect();
    let mut found: Vec<(&'static str, Span, String)> = Vec::new();
    let check_lets = |s: &Stmt, found: &mut Vec<(&'static str, Span, String)>| {
        if let Stmt::Let { name, value, span, .. } = s
            && let ExprKind::Str(text) = &value.kind
            && !text.is_empty()
            && SECRET_NAMES.iter().any(|w| name.to_lowercase().contains(w))
        {
            found.push(("S03", *span, format!("`{name}` is written in the code")));
        }
    };
    for s in &d.body.stmts {
        check_lets(s, &mut found);
    }
    d.body.walk(&mut |e: &Expr| {
        match &e.kind {
            ExprKind::Call { callee, args } => {
                let ExprKind::Path(path) = &callee.kind else { return };
                let Some(Resolved::Fn(g)) = p.lookup(f.module, path) else { return };
                let gname = &p.fns[g].name;
                let gmod = &p.mods[p.fns[g].module].name;
                let std = p.mods[p.fns[g].module].package == "std";
                if std && gmod == "os" && gname == "shell" && args.first().is_some_and(built_from_data) {
                    found.push(("S01", e.span, "`os::shell` runs a command built from data".into()));
                }
                if std && gmod == "fs" && args.iter().any(|x| request_params.iter().any(|r| mentions(x, r))) {
                    found.push(("S02", e.span, format!("`fs::{gname}` with a path from the request")));
                }
                if std && gmod == "tls" && (gname == "connect_trusting" || gname == "self_signed") && !is_test {
                    found.push(("S04", e.span, format!("`tls::{gname}` outside tests")));
                }
            }
            ExprKind::Str(s) if looks_secret(s) => found.push(("S03", e.span, "a key or token is written in the code".into())),
            ExprKind::Unsafe(_) => found.push(("S05", e.span, "`unsafe` block".into())),
            ExprKind::Block(b) => {
                for s in &b.stmts {
                    if let Stmt::Let { name, value, span, .. } = s
                        && let ExprKind::Str(text) = &value.kind
                        && !text.is_empty()
                        && SECRET_NAMES.iter().any(|w| name.to_lowercase().contains(w))
                    {
                        found.push(("S03", *span, format!("`{name}` is written in the code")));
                    }
                }
            }
            _ => {}
        }
    });
    for (id, span, msg) in found {
        a.add(id, f.file, span, msg);
    }
}

fn externs(a: &mut Audit, file: usize, items: &[Item]) {
    for it in items {
        match it {
            Item::Module(m) => externs(a, file, &m.items),
            Item::Extern(b) if b.abi == "C" => {
                if b.trusted && !b.includes.is_empty() {
                    let headers: Vec<&str> = b.includes.iter().map(|(h, _)| h.as_str()).collect();
                    a.add("S06", file, b.span, format!("trusts every function of {}", headers.join(", ")));
                }
                let declared: Vec<&ExternFn> = b.fns.iter().filter(|f| !b.includes.iter().any(|(_, s)| s.start == f.span.start)).collect();
                if b.trusted {
                    for f in &declared {
                        if f.params.iter().any(|(_, t)| matches!(t, Type::Ptr(..)) || matches!(t, Type::Named(n, _) if n == "cptr" || n == "cstr")) {
                            a.add("Q08", file, f.span, format!("`{}` is trusted but takes a pointer", f.name));
                        }
                    }
                }
                let numbers = |t: &Type| matches!(t, Type::Named(n, _) if ["i8", "i16", "i32", "i64", "u8", "u16", "u32", "u64", "isize", "usize", "f32", "f64", "bool"].contains(&n.as_str()));
                if b.effect == Effect::Sync
                    && b.includes.is_empty()
                    && !declared.is_empty()
                    && declared.iter().all(|f| f.params.iter().all(|(_, t)| numbers(t)) && f.ret.as_ref().is_some_and(numbers))
                {
                    a.add("Q07", file, b.span, "these C functions only take and return numbers, but are declared with effects".into());
                }
            }
            _ => {}
        }
    }
}

/// S07: dependencies of `mim.mim` that follow a branch.
pub fn dependencies(project: &mimic_project::Project) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for d in &project.manifest.dependencies {
        if let mimic_project::DepSource::Git { url, rev } = &d.source {
            let pinned = rev.as_deref().is_some_and(|r| {
                let tag = r.starts_with('v') && r[1..].starts_with(|c: char| c.is_ascii_digit());
                let commit = r.len() >= 7 && r.chars().all(|c| c.is_ascii_hexdigit());
                tag || commit || r.chars().next().is_some_and(|c| c.is_ascii_digit())
            });
            if !pinned {
                let what = match rev {
                    Some(r) => format!("`{}` follows the branch `{r}` of {url}", d.name),
                    None => format!("`{}` follows the default branch of {url}", d.name),
                };
                out.push((d.name.clone(), what));
            }
        }
    }
    out
}

/// A stable name for a finding: its rule, file and code (not its line).
pub fn fingerprint(rule: &str, file: &str, code: &str) -> String {
    let norm: String = code.split_whitespace().collect::<Vec<_>>().join(" ");
    format!("{rule}:{file}:{:016x}", mimic_syntax::fnv1a(norm.as_bytes()))
}

pub fn sarif(findings: &[Value]) -> Value {
    let rules: Vec<Value> = RULES
        .iter()
        .map(|r| {
            json!({
                "id": r.id,
                "name": r.name,
                "shortDescription": { "text": r.name.replace('-', " ") },
                "fullDescription": { "text": r.help },
                "help": { "text": format!("{} See `mimic man audit`.", r.help) },
                "properties": { "tags": [r.category] },
                "defaultConfiguration": { "level": r.severity },
            })
        })
        .collect();
    let results: Vec<Value> = findings
        .iter()
        .map(|f| {
            json!({
                "ruleId": f["rule"],
                "level": f["severity"],
                "message": { "text": f["message"] },
                "partialFingerprints": { "mimicFingerprint/v1": f["fingerprint"] },
                "locations": [{ "physicalLocation": {
                    "artifactLocation": { "uri": f["file"] },
                    "region": { "startLine": f["line"], "startColumn": f["column"], "endLine": f["end_line"], "endColumn": f["end_column"] },
                } }],
            })
        })
        .collect();
    json!({
        "$schema": "https://json.schemastore.org/sarif-2.1.0.json",
        "version": "2.1.0",
        "runs": [{ "tool": { "driver": { "name": "mimic audit", "version": env!("CARGO_PKG_VERSION"), "informationUri": "https://github.com/mimiccodes/mimic", "rules": rules } }, "results": results }],
    })
}
