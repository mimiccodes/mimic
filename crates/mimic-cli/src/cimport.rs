//! Importing C headers: `extern "C" include "math.h"`.
//!
//! The embedded Clang parses the header for the build's target and writes its
//! declarations as JSON (`-ast-dump=json`); the target's data model (the size
//! of `long`, of pointers) comes from Clang's own predefined macros. Every
//! function the header makes visible (as C's `#include` does) whose signature
//! Mimic's FFI can express becomes an extern declaration; the rest are skipped with a reason
//! (structs by value, varargs and callbacks are the next FFI steps).
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
}

#[derive(Default, Debug)]
pub struct Imported {
    pub fns: Vec<CFn>,
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
        format!("v2|{header}|{local:?}|{stamp}|{dirs:?}|{}|{:?}|{}", target.name(), target.llvm, toolchain::describe(&tc)).as_bytes(),
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
    std::fs::write(&probe, line).map_err(|e| e.to_string())?;
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
    let imported = collect(&v, header, local.as_deref(), &model);
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
        if node["variadic"].as_bool() == Some(true) {
            out.skipped.push(skip("variadic (printf-style): FFI step B3"));
            continue;
        }
        let ftype = node["type"]["qualType"].as_str().unwrap_or("");
        let ret_c = ftype.split('(').next().unwrap_or("").trim();
        let ret = match ret_c {
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
            let d = p["type"]["desugaredQualType"].as_str();
            match map_type(q, d, m, false) {
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
        out.fns.push(CFn { name: name.to_string(), params, ret });
    }
    out
}

/// A C type (as Clang spells it) → a Mimic FFI type, or why it cannot be one yet.
fn map_type(qual: &str, desugared: Option<&str>, m: &DataModel, is_ret: bool) -> Result<String, String> {
    let t = desugared.unwrap_or(qual).trim();
    let t = t.trim_start_matches("const ").trim_start_matches("volatile ").trim();
    let t = t.trim_end_matches(" restrict").trim_end_matches("restrict").trim();
    if let Some(inner) = t.strip_suffix('*') {
        let inner = inner.trim().trim_start_matches("const ").trim();
        if inner == "char" {
            return Ok("cstr".into());
        }
        if is_ret {
            return Err(format!("returns a pointer (`{qual}`): raw pointers come with FFI step B5"));
        }
        return match scalar(inner, m) {
            Some(s) => Ok(format!("ptr[{s}]")),
            None => Err(format!("pointer to `{inner}`: FFI step B2/B5")),
        };
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
        "fns": i.fns.iter().map(|f| serde_json::json!({"name": f.name, "params": f.params, "ret": f.ret})).collect::<Vec<_>>(),
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
            })
            .collect(),
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
        assert!(map_type("double *", None, &lp64, true).is_err());
        assert!(map_type("struct timeval", None, &lp64, false).is_err());
        assert_eq!(map_type("enum color", None, &lp64, false).unwrap(), "i32");
    }
}
