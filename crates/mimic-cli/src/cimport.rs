//! Importing C headers: `extern "C" include "math.h"`.
//!
//! The embedded Clang parses the header for the build's target and writes its
//! declarations as JSON (`-ast-dump=json`); the target's data model (the size
//! of `long`, of pointers) comes from Clang's own predefined macros. Every
//! function the header makes visible (as C's `#include` does) whose signature
//! Mimic's FFI can express becomes an extern declaration; the rest are skipped with a reason
//! (callbacks and unions are the next FFI steps).
//! Results are cached per header, target and modification time.

use crate::targets::Target;
use crate::toolchain::{self, Toolchain};
use mimic_syntax::Span;
use mimic_syntax::ast::{ExternFn, Type};
use serde_json::Value;
use std::path::{Path, PathBuf};

/// One imported function: name, C parameter types, C return type (Mimic spellings).
#[derive(Clone, Debug, PartialEq)]
pub struct CFn {
    pub name: String,
    pub params: Vec<String>,
    pub ret: Option<String>,
    pub variadic: bool,
    /// Called through a generated wrapper (callbacks): its symbol.
    pub link_name: Option<String>,
}

/// A function that takes or returns a struct by value. It is called through a
/// generated C wrapper taking plain numbers (Clang applies the target's
/// struct-passing rules) and a generated Mimic function with the C name that
/// packs and unpacks the records.
#[derive(Clone, Debug, PartialEq)]
pub struct StructFn {
    pub name: String,
    /// The Mimic function and the wrapper's extern declaration (`PUB ` marks
    /// where `pub` goes).
    pub mimic: String,
    /// The C wrapper.
    pub c: String,
    /// The C structs it needs as Mimic records.
    pub uses: Vec<String>,
}

#[derive(Default, Debug)]
pub struct Imported {
    pub fns: Vec<CFn>,
    pub struct_fns: Vec<StructFn>,
    /// C struct name → its Mimic record declaration (`PUB type Vec2 = { … }`).
    pub records: std::collections::BTreeMap<String, String>,
    /// The `#include` line wrappers start with.
    pub include_line: String,
    /// C for the callback trampolines and wrappers (empty when there are none).
    pub callback_c: String,
    /// (name, why it was skipped)
    pub skipped: Vec<(String, String)>,
}

impl Imported {
    /// The imported functions as extern declarations (spans point at the `include`).
    pub fn extern_fns(&self, is_pub: bool, span: Span) -> Vec<ExternFn> {
        let ty = |t: &str| -> Type {
            match t.strip_prefix("ptr[").and_then(|r| r.strip_suffix(']')) {
                Some(inner) => Type::Ptr(Box::new(Type::Named(inner.into(), span)), span),
                None => Type::Named(t.into(), span),
            }
        };
        self.fns
            .iter()
            .map(|f| ExternFn {
                name: f.name.clone(),
                is_pub,
                params: f.params.iter().enumerate().map(|(i, p)| (format!("a{i}"), ty(p))).collect(),
                variadic: f.variadic,
                link_name: f.link_name.clone(),
                ret: f.ret.as_deref().map(ty),
                span,
            })
            .collect()
    }
}

/// Where a header is found: a file next to the source or in `ffi.include`
/// (`#include "…"`), else a system header (`#include <…>`).
fn locate(header: &str, dirs: &[PathBuf]) -> Option<PathBuf> {
    dirs.iter().map(|d| d.join(header)).find(|p| p.is_file())
}

pub fn import(header: &str, dirs: &[PathBuf], target: &Target) -> Result<Imported, String> {
    let tc = toolchain::select(target).map_err(|e| format!("cannot read C headers for `{}`: {e}", target.name()))?;
    let local = locate(header, dirs);
    let stamp = local
        .as_ref()
        .and_then(|p| std::fs::metadata(p).ok())
        .and_then(|m| m.modified().ok())
        .map(|t| format!("{t:?}"))
        .unwrap_or_default();
    let key = mimic_syntax::fnv1a(
        format!("v8|{header}|{local:?}|{stamp}|{dirs:?}|{}|{:?}|{}", target.name(), target.llvm, toolchain::describe(&tc)).as_bytes(),
    );
    let cache = mimic_project::packages::cache_dir().join("cimport");
    let cached = cache.join(format!("{key:016x}.json"));
    if let Ok(text) = std::fs::read_to_string(&cached)
        && let Ok(v) = serde_json::from_str::<Value>(&text)
    {
        return Ok(from_cache(&v));
    }

    let _ = std::fs::create_dir_all(&cache);
    let probe = cache.join(format!("probe-{key:016x}-{}.c", std::process::id()));
    let line = match &local {
        Some(p) => format!("#include \"{}\"\n", p.display()),
        None => format!("#include <{header}>\n"),
    };
    std::fs::write(&probe, &line).map_err(|e| e.to_string())?;
    let run = |extra: &[&str]| -> Result<Vec<u8>, String> {
        let mut c = toolchain::cc(&tc, target);
        for d in dirs.iter().filter(|d| !d.as_os_str().is_empty()) {
            c.arg(format!("-I{}", d.display()));
        }
        c.args(extra).arg(&probe);
        let out = c.output().map_err(|e| e.to_string())?;
        if !out.status.success() {
            // Clang's messages, without the temporary file that holds the #include
            let msg = String::from_utf8_lossy(&out.stderr)
                .lines()
                .filter(|l| !l.trim_start().starts_with(|c: char| c.is_ascii_digit()) && !l.trim_start().starts_with('|'))
                .map(|l| l.replace(&format!("{}:1:10: ", probe.display()), ""))
                .collect::<Vec<_>>()
                .join("\n");
            return Err(format!("cannot read C header `{header}`: {}", msg.trim()));
        }
        Ok(out.stdout)
    };
    let macros = run(&["-dM", "-E"]);
    let ast = run(&["-fsyntax-only", "-Xclang", "-ast-dump=json"]);
    let _ = std::fs::remove_file(&probe);
    let (macros, ast) = (macros?, ast?);
    let model = DataModel::from_macros(&String::from_utf8_lossy(&macros), matches!(tc, Toolchain::System));
    let v: Value = serde_json::from_slice(&ast).map_err(|e| format!("cannot read Clang's AST for `{header}`: {e}"))?;
    let mut imported = collect(&v, header, local.as_deref(), &model);
    imported.include_line = line.trim().to_string();
    let _ = mimic_toolchain::jobs::write_atomic(&cached, to_cache(&imported).to_string().as_bytes());
    Ok(imported)
}

/// Sizes that decide how C's integer types map to Mimic's.
struct DataModel {
    long_bits: u32,
    ptr_bits: u32,
}

impl DataModel {
    fn from_macros(text: &str, _system: bool) -> DataModel {
        let get = |name: &str| -> Option<u32> {
            text.lines().find_map(|l| l.strip_prefix(&format!("#define {name} ")).and_then(|v| v.trim().parse().ok()))
        };
        DataModel { long_bits: get("__SIZEOF_LONG__").unwrap_or(8) * 8, ptr_bits: get("__SIZEOF_POINTER__").unwrap_or(8) * 8 }
    }
}

/// The file a declaration comes from: Clang's JSON names a file only when it
/// changes, so the current file is carried along in order.
fn update_file(node: &Value, cur: &mut Option<String>) {
    let loc = &node["loc"];
    for l in [loc, &loc["spellingLoc"], &loc["expansionLoc"]] {
        if let Some(f) = l["file"].as_str() {
            *cur = Some(f.to_string());
        }
    }
}

fn collect(ast: &Value, header: &str, local: Option<&Path>, m: &DataModel) -> Imported {
    let mut out = Imported::default();
    let mut cur: Option<String> = None;
    let mine = |f: &str| -> bool {
        match local {
            Some(p) => Path::new(f) == p || std::fs::canonicalize(f).ok() == std::fs::canonicalize(p).ok(),
            None => f == header || f.ends_with(&format!("/{header}")),
        }
    };
    // typedef name → what it stands for (Clang does not desugar return types)
    let mut typedefs: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    for node in ast["inner"].as_array().into_iter().flatten() {
        if node["kind"] == "TypedefDecl"
            && let Some(name) = node["name"].as_str()
        {
            let t = &node["type"];
            if let Some(u) = t["desugaredQualType"].as_str().or(t["qualType"].as_str()) {
                typedefs.insert(name.to_string(), u.to_string());
            }
        }
    }
    let resolve = |t: &str| -> String {
        let mut t = t.trim().to_string();
        for _ in 0..16 {
            let (base, stars) = (t.trim_end_matches(['*', ' ']).trim_start_matches("const ").trim(), t.matches('*').count());
            match typedefs.get(base) {
                Some(u) => t = format!("{u}{}", " *".repeat(stars)),
                None => break,
            }
        }
        t
    };
    // complete struct definitions: name → (how C spells the type, fields)
    let mut records: HashMap<String, CRecord> = HashMap::new();
    let mut anonymous: HashMap<String, Vec<(String, String, bool)>> = HashMap::new();
    let fields_of = |n: &Value| -> Vec<(String, String, bool)> {
        n["inner"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|f| f["kind"] == "FieldDecl")
            .map(|f| {
                let t = &f["type"];
                let ty = t["desugaredQualType"].as_str().or(t["qualType"].as_str()).unwrap_or("").to_string();
                (f["name"].as_str().unwrap_or("").to_string(), ty, f["isBitfield"].as_bool() == Some(true))
            })
            .collect()
    };
    for node in ast["inner"].as_array().into_iter().flatten() {
        if node["kind"] == "RecordDecl" && node["tagUsed"] == "struct" && node["completeDefinition"].as_bool() == Some(true) {
            match node["name"].as_str() {
                Some(n) => {
                    records.insert(n.to_string(), CRecord { spelling: format!("struct {n}"), fields: fields_of(node) });
                }
                None => {
                    if let Some(id) = node["id"].as_str() {
                        anonymous.insert(id.to_string(), fields_of(node));
                    }
                }
            }
        }
        // `typedef struct { … } name;`: the struct is known by the typedef's name
        if node["kind"] == "TypedefDecl"
            && let Some(name) = node["name"].as_str()
            && let Some(id) = node["inner"][0]["ownedTagDecl"]["id"].as_str().or(node["inner"][0]["decl"]["id"].as_str())
            && let Some(fields) = anonymous.get(id)
        {
            records.insert(name.to_string(), CRecord { spelling: name.to_string(), fields: fields.clone() });
        }
    }
    let ctx = StructCtx { records: &records, model: m, resolve: &resolve };
    let mut seen = std::collections::HashSet::new();
    for node in ast["inner"].as_array().into_iter().flatten() {
        update_file(node, &mut cur);
        if node["kind"] != "FunctionDecl" || node["isImplicit"].as_bool() == Some(true) {
            continue;
        }
        // everything the header makes visible (like C's #include), except
        // the compiler's own builtin headers
        if cur.as_deref().is_some_and(|f| f.contains("/lib/clang/") || f.contains("/clang-23/include/")) && !cur.as_deref().is_some_and(mine) {
            continue;
        }
        let Some(name) = node["name"].as_str() else { continue };
        if !seen.insert(name.to_string()) {
            continue; // redeclaration
        }
        let skip = |why: &str| (name.to_string(), why.to_string());
        if name.starts_with('_') {
            continue; // reserved: implementation details
        }
        if name.starts_with(|c: char| c.is_ascii_uppercase()) {
            out.skipped.push(skip("names starting with an uppercase letter are types or variants in Mimic"));
            continue;
        }
        if node["storageClass"] == "static" || node["inline"].as_bool() == Some(true) {
            out.skipped.push(skip("static/inline: no symbol to link (wrappers come with the next FFI step)"));
            continue;
        }
        let variadic = node["variadic"].as_bool() == Some(true);
        let ftype = node["type"]["qualType"].as_str().unwrap_or("");
        let ret_c = resolve(ftype.split('(').next().unwrap_or("").trim());
        // structs by value: through generated wrappers
        let parms: Vec<(String, String)> = node["inner"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|p| p["kind"] == "ParmVarDecl")
            .map(|p| {
                let q = p["type"]["qualType"].as_str().unwrap_or("").to_string();
                let d = p["type"]["desugaredQualType"].as_str().map(String::from).unwrap_or_else(|| resolve(&q));
                (q, d)
            })
            .collect();
        // function-pointer parameters: trampolines that call Mimic function values
        if parms.iter().any(|(_, d)| is_fn_ptr(d)) {
            if variadic || ctx.struct_name(&ret_c).is_some() || parms.iter().any(|(_, d)| ctx.struct_name(d).is_some()) {
                out.skipped.push(skip("callbacks together with varargs or structs by value"));
                continue;
            }
            let ret_q = ftype.split('(').next().unwrap_or("").trim().to_string();
            match callback_wrapper(name, &ret_q, &ret_c, &parms, m) {
                Ok((f, c)) => {
                    out.fns.push(f);
                    out.callback_c.push_str(&c);
                }
                Err(why) => out.skipped.push(skip(&why)),
            }
            continue;
        }
        if ctx.struct_name(&ret_c).is_some() || parms.iter().any(|(_, d)| ctx.struct_name(d).is_some()) {
            if variadic {
                out.skipped.push(skip("varargs together with structs by value"));
                continue;
            }
            let ret_q = ftype.split('(').next().unwrap_or("").trim();
            match ctx.wrap(name, (ret_q, &ret_c), &parms) {
                Ok((f, used)) => {
                    for r in &used {
                        out.records.entry(r.clone()).or_insert_with(|| ctx.record_decl(r));
                    }
                    out.struct_fns.push(f);
                }
                Err(why) => out.skipped.push(skip(&why)),
            }
            continue;
        }
        let ret = match ret_c.as_str() {
            "void" => None,
            r => match map_type(r, None, m, true) {
                Ok(t) => Some(t),
                Err(why) => {
                    out.skipped.push(skip(&why));
                    continue;
                }
            },
        };
        let mut params = Vec::new();
        let mut bad = None;
        for p in node["inner"].as_array().into_iter().flatten().filter(|p| p["kind"] == "ParmVarDecl") {
            let q = p["type"]["qualType"].as_str().unwrap_or("");
            let d = p["type"]["desugaredQualType"].as_str().map(String::from).unwrap_or_else(|| resolve(q));
            match map_type(q, Some(&d), m, false) {
                Ok(t) => params.push(t),
                Err(why) => {
                    bad = Some(why);
                    break;
                }
            }
        }
        if let Some(why) = bad {
            out.skipped.push(skip(&why));
            continue;
        }
        out.fns.push(CFn { name: name.to_string(), params, ret, variadic, link_name: None });
    }
    out
}

use std::collections::HashMap;

fn is_fn_ptr(t: &str) -> bool {
    t.contains("(*)")
}

/// `int (*)(const void *, const void *)` → ("int", ["const void *", "const void *"]).
fn split_fn_ptr(t: &str) -> Option<(String, Vec<String>)> {
    let star = t.find("(*)")?;
    let ret = t[..star].trim().to_string();
    let rest = t[star + 3..].trim();
    let inner = rest.strip_prefix('(')?.strip_suffix(')')?;
    if inner.contains('(') {
        return None; // callbacks taking callbacks
    }
    let params: Vec<String> = match inner.trim() {
        "" | "void" => Vec::new(),
        x => x.split(',').map(|p| p.trim().to_string()).collect(),
    };
    Some((ret, params))
}

/// How a C value becomes a Mimic value inside a trampoline.
fn to_mimic(c: &str, v: &str, m: &DataModel) -> Option<String> {
    let base = c.trim().trim_start_matches("const ").trim();
    if base.ends_with('*') {
        let pointee = base.trim_end_matches('*').trim().trim_start_matches("const ").trim();
        return Some(if pointee == "char" && base.matches('*').count() == 1 {
            format!("mim_from_cstr({v})")
        } else {
            format!("mim_int((long long)(__INTPTR_TYPE__){v})")
        });
    }
    if base.starts_with("enum ") {
        return Some(format!("mim_int((long long){v})"));
    }
    Some(match scalar(base, m)?.as_str() {
        "f32" | "f64" => format!("mim_float((double){v})"),
        "bool" => format!("mim_bool((long long){v})"),
        _ => format!("mim_int((long long){v})"),
    })
}

/// How a Mimic value becomes the callback's C result.
fn from_mimic(c: &str, v: &str, m: &DataModel) -> Option<String> {
    let base = c.trim().trim_start_matches("const ").trim();
    if base.ends_with('*') {
        if base.trim_end_matches('*').trim() == "char" {
            return None; // a Mimic string cannot outlive the callback
        }
        return Some(format!("({c})(__INTPTR_TYPE__)mim_as_i64({v})"));
    }
    if base.starts_with("enum ") {
        return Some(format!("({c})mim_as_i64({v})"));
    }
    Some(match scalar(base, m)?.as_str() {
        "f32" | "f64" => format!("({c})mim_as_f64({v})"),
        _ => format!("({c})mim_as_i64({v})"),
    })
}

/// A function with function-pointer parameters: each becomes `fnref` (a Mimic
/// function value), passed to a C wrapper that installs it in a thread-local
/// slot (nesting-safe) and hands C a trampoline with the callback's own
/// signature. Callbacks must be called during the C call, on its thread.
fn callback_wrapper(name: &str, ret_q: &str, ret_c: &str, parms: &[(String, String)], m: &DataModel) -> Result<(CFn, String), String> {
    let w = format!("mimicw_{name}");
    let mut c = String::new();
    let mut wparams = Vec::new();
    let mut call_args = Vec::new();
    let mut ffi = Vec::new();
    let mut installs = String::new();
    let mut restores = String::new();
    for (i, (q, d)) in parms.iter().enumerate() {
        let a = format!("a{i}");
        if is_fn_ptr(d) {
            let (cret, cps) = split_fn_ptr(d).ok_or_else(|| format!("callback type `{q}` is not supported yet"))?;
            let slot = format!("{w}_cb{i}");
            let tramp = format!("{w}_t{i}");
            let tparams: Vec<String> = cps.iter().enumerate().map(|(j, t)| format!("{t} p{j}")).collect();
            let mut body = format!("    mim_val *args[{}];\n", cps.len().max(1));
            for (j, t) in cps.iter().enumerate() {
                let conv = to_mimic(t, &format!("p{j}"), m).ok_or_else(|| format!("callback argument type `{t}` cannot become a Mimic value yet"))?;
                body.push_str(&format!("    args[{j}] = {conv};\n"));
            }
            body.push_str(&format!("    mim_val *r = mim_call({slot}, {}, args);\n", cps.len()));
            body.push_str(&format!("    for (int i = 0; i < {}; i++) mim_release(args[i]);\n", cps.len()));
            if cret == "void" {
                body.push_str("    mim_release(r);\n");
            } else {
                let conv = from_mimic(&cret, "r", m).ok_or_else(|| format!("callback result type `{cret}` cannot come from Mimic yet"))?;
                body.push_str(&format!("    {cret} out = {conv};\n    mim_release(r);\n    return out;\n"));
            }
            c.push_str(&format!("static _Thread_local mim_val *{slot};\n"));
            c.push_str(&format!("static {cret} {tramp}({}) {{\n{body}}}\n", if tparams.is_empty() { "void".into() } else { tparams.join(", ") }));
            wparams.push(format!("mim_val *{a}"));
            installs.push_str(&format!("    mim_val *saved{i} = {slot}; {slot} = {a};\n"));
            restores.push_str(&format!("    {slot} = saved{i};\n"));
            call_args.push(format!("{a} ? {tramp} : 0"));
            ffi.push("fnref".to_string());
        } else {
            let t = map_type(q, Some(d), m, false)?;
            wparams.push(format!("{q} {a}"));
            call_args.push(a);
            ffi.push(t);
        }
    }
    let (ret, call) = if ret_c.trim() == "void" {
        (None, format!("    {name}({});\n", call_args.join(", ")))
    } else {
        (Some(map_type(ret_c, None, m, true)?), format!("    {ret_q} r = {name}({});\n", call_args.join(", ")))
    };
    let tail = if ret.is_some() { "    return r;\n" } else { "" };
    c.push_str(&format!(
        "{} {w}({}) {{\n{installs}{call}{restores}{tail}}}\n",
        if ret.is_some() { ret_q } else { "void" },
        if wparams.is_empty() { "void".into() } else { wparams.join(", ") }
    ));
    Ok((CFn { name: name.to_string(), params: ffi, ret, variadic: false, link_name: Some(w) }, c))
}

struct CRecord {
    /// How C names the type in a declaration: `struct vec2`, or a typedef name.
    spelling: String,
    /// (field, its C type, is a bitfield)
    fields: Vec<(String, String, bool)>,
}

/// A scalar leaf of a flattened struct.
#[derive(Clone, Copy, PartialEq)]
enum Leaf {
    Int,
    Float,
    Bool,
    Ptr,
}

impl Leaf {
    fn c(self) -> &'static str {
        match self {
            Leaf::Int | Leaf::Bool => "long long",
            Leaf::Float => "double",
            Leaf::Ptr => "void *",
        }
    }
    fn ffi(self) -> &'static str {
        match self {
            Leaf::Int | Leaf::Bool => "i64",
            Leaf::Float => "f64",
            Leaf::Ptr => "cptr",
        }
    }
    fn mimic(self) -> &'static str {
        match self {
            Leaf::Int | Leaf::Ptr => "i64",
            Leaf::Float => "f64",
            Leaf::Bool => "bool",
        }
    }
}

struct StructCtx<'a> {
    records: &'a HashMap<String, CRecord>,
    model: &'a DataModel,
    resolve: &'a dyn Fn(&str) -> String,
}

/// `vec2` → `Vec2`, `size_t2` → `SizeT2`: Mimic type names start uppercase.
fn camel(c: &str) -> String {
    c.split('_').filter(|w| !w.is_empty()).map(|w| {
        let mut ch = w.chars();
        ch.next().map(|f| f.to_ascii_uppercase().to_string() + ch.as_str()).unwrap_or_default()
    }).collect()
}

impl StructCtx<'_> {
    /// `struct vec2` / `const struct vec2` → `vec2`, when it is a known struct (not a pointer).
    fn struct_name(&self, t: &str) -> Option<String> {
        let t = t.trim().trim_start_matches("const ").trim();
        if t.contains('*') {
            return None;
        }
        let n = t.strip_prefix("struct ").unwrap_or(t).trim();
        self.records.contains_key(n).then(|| n.to_string())
    }

    /// The scalar leaves of struct `n`: (path of field names, kind).
    fn flatten(&self, n: &str, depth: usize) -> Result<Vec<(Vec<String>, Leaf)>, String> {
        let r = self.records.get(n).ok_or_else(|| format!("struct `{n}` is not defined in the header"))?;
        if depth > 8 {
            return Err(format!("struct `{n}` nests too deeply"));
        }
        let mut out = Vec::new();
        for (f, ty, _bitfield) in &r.fields {
            if f.is_empty() {
                return Err(format!("struct `{n}` has an anonymous member"));
            }
            let t = (self.resolve)(ty);
            if let Some(inner) = self.struct_name(&t) {
                for (mut path, leaf) in self.flatten(&inner, depth + 1)? {
                    path.insert(0, f.clone());
                    out.push((path, leaf));
                }
                continue;
            }
            let base = t.trim_start_matches("const ").trim_start_matches("volatile ").trim();
            let leaf = if base.ends_with('*') {
                Leaf::Ptr
            } else if base.starts_with("enum ") {
                Leaf::Int
            } else {
                match scalar(base, self.model).as_deref() {
                    Some("f32" | "f64") => Leaf::Float,
                    Some("bool") => Leaf::Bool,
                    Some(_) => Leaf::Int,
                    None => return Err(format!("field `{f}` of struct `{n}` (`{ty}`) cannot cross to Mimic yet")),
                }
            };
            out.push((vec![f.clone()], leaf));
        }
        Ok(out)
    }

    /// The structs `n` needs (itself and the ones nested in it).
    fn closure(&self, n: &str, acc: &mut Vec<String>) {
        if acc.iter().any(|a| a == n) {
            return;
        }
        acc.push(n.to_string());
        if let Some(r) = self.records.get(n) {
            for (_, ty, _) in &r.fields {
                if let Some(inner) = self.struct_name(&(self.resolve)(ty)) {
                    self.closure(&inner, acc);
                }
            }
        }
    }

    /// `PUB type Vec2 = { x: f64, y: f64 }`
    fn record_decl(&self, n: &str) -> String {
        let r = &self.records[n];
        let fields: Vec<String> = r
            .fields
            .iter()
            .map(|(f, ty, _)| {
                let t = (self.resolve)(ty);
                let mt = match self.struct_name(&t) {
                    Some(inner) => camel(&inner),
                    None => match self.flatten_leaf(&t) {
                        Some(l) => l.mimic().to_string(),
                        None => "i64".into(),
                    },
                };
                format!("{f}: {mt}")
            })
            .collect();
        format!("PUB type {} = {{ {} }}", camel(n), fields.join(", "))
    }

    fn flatten_leaf(&self, t: &str) -> Option<Leaf> {
        let base = t.trim_start_matches("const ").trim();
        if base.ends_with('*') {
            return Some(Leaf::Ptr);
        }
        if base.starts_with("enum ") {
            return Some(Leaf::Int);
        }
        Some(match scalar(base, self.model)?.as_str() {
            "f32" | "f64" => Leaf::Float,
            "bool" => Leaf::Bool,
            _ => Leaf::Int,
        })
    }

    /// The C wrapper and the Mimic function for one struct-taking/returning function.
    fn wrap(&self, name: &str, (ret_q, ret_c): (&str, &str), parms: &[(String, String)]) -> Result<(StructFn, Vec<String>), String> {
        let mut used = Vec::new();
        let mut c_params: Vec<String> = Vec::new(); // wrapper parameter list
        let mut c_setup = String::new(); // building struct arguments
        let mut c_args: Vec<String> = Vec::new(); // arguments of the real call
        let mut ext_params: Vec<String> = Vec::new(); // extern decl parameters
        let mut call_args: Vec<String> = Vec::new(); // Mimic call arguments
        let mut m_params: Vec<String> = Vec::new(); // Mimic function parameters
        let ret_struct = self.struct_name(ret_c);
        if ret_struct.is_some() {
            c_params.push("long long *ri".into());
            c_params.push("double *rf".into());
            ext_params.push("ri: ptr[i64]".into());
            ext_params.push("rf: ptr[f64]".into());
            call_args.push("ri".into());
            call_args.push("rf".into());
        }
        for (i, (q, d)) in parms.iter().enumerate() {
            let a = format!("a{i}");
            if let Some(sn) = self.struct_name(d) {
                self.closure(&sn, &mut used);
                c_setup.push_str(&format!("{} {a}; ", self.records[&sn].spelling));
                for (k, (path, leaf)) in self.flatten(&sn, 0)?.into_iter().enumerate() {
                    let p = format!("{a}_{k}");
                    c_params.push(format!("{} {p}", leaf.c()));
                    let cast = if leaf == Leaf::Ptr { "(void *)" } else { "" };
                    c_setup.push_str(&format!("{a}.{} = {cast}{p}; ", path.join(".")));
                    ext_params.push(format!("{p}: {}", leaf.ffi()));
                    call_args.push(format!("{a}.{}", path.join(".")));
                }
                c_args.push(a.clone());
                m_params.push(format!("{a}: {}", camel(&sn)));
            } else {
                let ffi = map_type(q, Some(d), self.model, false)?;
                c_params.push(format!("{q} {a}"));
                c_args.push(a.clone());
                ext_params.push(format!("{a}: {ffi}"));
                call_args.push(a.clone());
                let mt = match ffi.as_str() {
                    "cstr" => "str".to_string(),
                    "cptr" => "i64".to_string(),
                    "f32" | "f64" => "f64".to_string(),
                    "bool" => "bool".to_string(),
                    t if t.starts_with("ptr[") => {
                        let inner = &t[4..t.len() - 1];
                        format!("[{}]", if inner == "f32" || inner == "f64" { "f64" } else { "i64" })
                    }
                    _ => "i64".to_string(),
                };
                m_params.push(format!("{a}: {mt}"));
            }
        }
        let call = format!("{name}({})", c_args.join(", "));
        let w = format!("mimicw_{name}");
        let (c_ret, c_body, ext_ret, m_ret, m_body) = match &ret_struct {
            Some(sn) => {
                self.closure(sn, &mut used);
                let leaves = self.flatten(sn, 0)?;
                let mut stores = String::new();
                let (mut ni, mut nf) = (0, 0);
                let mut idx = Vec::new();
                for (path, leaf) in &leaves {
                    let f = path.join(".");
                    match leaf {
                        Leaf::Float => {
                            stores.push_str(&format!("rf[{nf}] = r.{f}; "));
                            idx.push(format!("rf[{nf}]"));
                            nf += 1;
                        }
                        Leaf::Ptr => {
                            stores.push_str(&format!("ri[{ni}] = (long long)(__INTPTR_TYPE__)r.{f}; "));
                            idx.push(format!("ri[{ni}]"));
                            ni += 1;
                        }
                        Leaf::Int | Leaf::Bool => {
                            stores.push_str(&format!("ri[{ni}] = (long long)r.{f}; "));
                            idx.push(if *leaf == Leaf::Bool { format!("ri[{ni}] != 0") } else { format!("ri[{ni}]") });
                            ni += 1;
                        }
                    }
                }
                let lit = self.literal(sn, &mut idx.into_iter())?;
                let zeros = |n: usize, z: &str| vec![z; n.max(1)].join(" ");
                let body = format!(
                    "    let mut ri = [{}]\n    let mut rf = [{}]\n    unsafe {{ {w}({}) }}\n    {lit}",
                    zeros(ni, "0"),
                    zeros(nf, "0.0"),
                    call_args.join(", ")
                );
                ("void".to_string(), format!("{} r = {call}; {stores}", self.records[sn].spelling), String::new(), format!(" -> {}", camel(sn)), body)
            }
            None => {
                let (er, mr) = match ret_c.trim() {
                    "void" => (String::new(), String::new()),
                    r => {
                        let ffi = map_type(r, None, self.model, true)?;
                        let mt = match ffi.as_str() {
                            "cstr" => "str",
                            "f32" | "f64" => "f64",
                            "bool" => "bool",
                            _ => "i64",
                        };
                        (format!(" -> {ffi}"), format!(" -> {mt}"))
                    }
                };
                let c_body = if ret_c.trim() == "void" { format!("{call};") } else { format!("return {call};") };
                (ret_q.to_string(), c_body, er, mr, format!("    unsafe {{ {w}({}) }}", call_args.join(", ")))
            }
        };
        let c = format!("{c_ret} {w}({}) {{ {c_setup}{c_body} }}\n", if c_params.is_empty() { "void".into() } else { c_params.join(", ") });
        let mimic = format!(
            "extern \"C\" {{ fn {w}({}){ext_ret} }}\nPUB fn {name}({}){m_ret} {{\n{m_body}\n}}\n",
            ext_params.join(", "),
            m_params.join(", ")
        );
        Ok((StructFn { name: name.to_string(), mimic, c, uses: used.clone() }, used))
    }

    /// A record literal for struct `n`, taking leaf values in order.
    fn literal(&self, n: &str, vals: &mut dyn Iterator<Item = String>) -> Result<String, String> {
        let r = &self.records[n];
        let mut parts = Vec::new();
        for (f, ty, _) in &r.fields {
            let t = (self.resolve)(ty);
            let v = match self.struct_name(&t) {
                Some(inner) => self.literal(&inner, vals)?,
                None => vals.next().ok_or("internal: struct leaves out of step")?,
            };
            parts.push(format!("{f}: {v}"));
        }
        Ok(format!("{} {{ {} }}", camel(n), parts.join(", ")))
    }
}

/// A C type (as Clang spells it) → a Mimic FFI type, or why it cannot be one yet.
fn map_type(qual: &str, desugared: Option<&str>, m: &DataModel, is_ret: bool) -> Result<String, String> {
    let t = desugared.unwrap_or(qual).trim();
    let t = t.trim_start_matches("const ").trim_start_matches("volatile ").trim();
    let t = t.trim_end_matches(" restrict").trim_end_matches("restrict").trim();
    if t.contains("(*)") || t.contains("(^)") {
        return Err(format!("`{qual}`: function pointers (callbacks) come with FFI step B4"));
    }
    if let Some(inner) = t.strip_suffix('*') {
        let inner = inner.trim().trim_start_matches("const ").trim_end_matches(" const").trim();
        // strings: char* both ways; unsigned char* when C returns it (sqlite3_column_text)
        if inner == "char" || (is_ret && inner == "unsigned char") {
            return Ok("cstr".into());
        }
        // T** out-parameters: a one-element array C writes a pointer into
        if inner.ends_with('*') {
            return Ok(if is_ret { "cptr".into() } else { format!("ptr[{}]", if m.ptr_bits == 64 { "cptr" } else { "i32" }) });
        }
        if !is_ret && let Some(s) = scalar(inner, m) {
            return Ok(format!("ptr[{s}]"));
        }
        // everything else is an opaque handle: sqlite3 *, FILE *, void *, returned buffers
        return Ok("cptr".into());
    }
    if t.starts_with("enum ") {
        return Ok("i32".into());
    }
    if t.starts_with("struct ") || t.starts_with("union ") {
        return Err(format!("`{qual}` by value: FFI step B2"));
    }
    if t.contains('(') || t.contains('[') {
        return Err(format!("`{qual}`: function pointers and arrays come with FFI steps B4/B2"));
    }
    scalar(t, m).ok_or_else(|| format!("`{qual}` has no Mimic equivalent yet"))
}

fn scalar(t: &str, m: &DataModel) -> Option<String> {
    let int = |bits: u32, signed: bool| format!("{}{bits}", if signed { "i" } else { "u" });
    Some(match t {
        "char" => "c_char".into(),
        "signed char" => "i8".into(),
        "unsigned char" => "u8".into(),
        "short" | "short int" | "signed short" => "i16".into(),
        "unsigned short" | "unsigned short int" => "u16".into(),
        "int" | "signed int" | "signed" => "i32".into(),
        "unsigned int" | "unsigned" => "u32".into(),
        "long" | "long int" | "signed long" => int(m.long_bits, true),
        "unsigned long" | "unsigned long int" => int(m.long_bits, false),
        "long long" | "long long int" | "signed long long" => "i64".into(),
        "unsigned long long" | "unsigned long long int" => "u64".into(),
        "float" => "f32".into(),
        "double" => "f64".into(),
        "_Bool" | "bool" => "bool".into(),
        _ if m.ptr_bits == 0 => return None,
        _ => return None,
    })
}

fn to_cache(i: &Imported) -> Value {
    serde_json::json!({
        "fns": i.fns.iter().map(|f| serde_json::json!({"name": f.name, "params": f.params, "ret": f.ret, "variadic": f.variadic, "link": f.link_name})).collect::<Vec<_>>(),
        "callback_c": i.callback_c,
        "struct_fns": i.struct_fns.iter().map(|f| serde_json::json!({"name": f.name, "mimic": f.mimic, "c": f.c, "uses": f.uses})).collect::<Vec<_>>(),
        "records": i.records,
        "include": i.include_line,
        "skipped": i.skipped.iter().map(|(n, w)| serde_json::json!([n, w])).collect::<Vec<_>>(),
    })
}

fn from_cache(v: &Value) -> Imported {
    let s = |x: &Value| x.as_str().unwrap_or("").to_string();
    Imported {
        fns: v["fns"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|f| CFn {
                name: s(&f["name"]),
                params: f["params"].as_array().into_iter().flatten().map(s).collect(),
                ret: f["ret"].as_str().map(String::from),
                variadic: f["variadic"].as_bool() == Some(true),
                link_name: f["link"].as_str().map(String::from),
            })
            .collect(),
        struct_fns: v["struct_fns"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|f| StructFn {
                name: s(&f["name"]),
                mimic: s(&f["mimic"]),
                c: s(&f["c"]),
                uses: f["uses"].as_array().into_iter().flatten().map(s).collect(),
            })
            .collect(),
        records: v["records"].as_object().map(|o| o.iter().map(|(k, v)| (k.clone(), s(v))).collect()).unwrap_or_default(),
        include_line: s(&v["include"]),
        callback_c: s(&v["callback_c"]),
        skipped: v["skipped"].as_array().into_iter().flatten().map(|p| (s(&p[0]), s(&p[1]))).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_c_types_by_data_model() {
        let lp64 = DataModel { long_bits: 64, ptr_bits: 64 };
        let ilp32 = DataModel { long_bits: 32, ptr_bits: 32 };
        assert_eq!(map_type("long", None, &lp64, false).unwrap(), "i64");
        assert_eq!(map_type("long", None, &ilp32, false).unwrap(), "i32");
        assert_eq!(map_type("size_t", Some("unsigned long"), &lp64, false).unwrap(), "u64");
        assert_eq!(map_type("const char *", None, &lp64, false).unwrap(), "cstr");
        assert_eq!(map_type("double *", None, &lp64, false).unwrap(), "ptr[f64]");
        assert_eq!(map_type("double *", None, &lp64, true).unwrap(), "cptr");
        assert_eq!(map_type("sqlite3 *", None, &lp64, false).unwrap(), "cptr");
        assert_eq!(map_type("sqlite3 **", None, &lp64, false).unwrap(), "ptr[cptr]");
        assert_eq!(map_type("const unsigned char *", None, &lp64, true).unwrap(), "cstr");
        assert!(map_type("int (*)(void *, int)", None, &lp64, false).is_err());
        assert_eq!(map_type("enum color", None, &lp64, false).unwrap(), "i32");
    }
}
