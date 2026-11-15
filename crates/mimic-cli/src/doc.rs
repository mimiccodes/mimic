//! `mimic doc`: documentation of a package's public items (G7).
//!
//! Every public module, function, type and foreign (`extern`) function, with
//! its full signature (type parameters, parameter types, defaults, result)
//! and the `//` comment lines directly above it, as one self-contained HTML
//! page (a sidebar of modules, a search box) or as JSON for editors.

use mimic_check::Program;
use mimic_syntax::ast::{ExternBlock, FnDecl, Item, ModuleDecl, Type, TypeDecl, TypeKind};
use serde_json::{Value, json};

pub struct DocItem {
    pub kind: &'static str,
    pub name: String,
    pub sig: String,
    pub doc: String,
}

pub struct DocModule {
    pub path: String,
    pub doc: String,
    pub items: Vec<DocItem>,
}

/// A type as it is written.
pub fn show_type(t: &Type) -> String {
    match t {
        Type::Named(n, _) => n.clone(),
        Type::App(n, args, _) => format!("{n}[{}]", args.iter().map(show_type).collect::<Vec<_>>().join(", ")),
        Type::Array(t, _) => format!("[{}]", show_type(t)),
        Type::Map(k, v, _) => format!("[{}: {}]", show_type(k), show_type(v)),
        Type::Tuple(ts, _) => format!("({})", ts.iter().map(show_type).collect::<Vec<_>>().join(", ")),
        Type::Ptr(t, _) => format!("ptr[{}]", show_type(t)),
        Type::Fn(ps, r, _) => {
            let ps = ps.iter().map(show_type).collect::<Vec<_>>().join(", ");
            match r {
                Some(r) => format!("fn({ps}) -> {}", show_type(r)),
                None => format!("fn({ps})"),
            }
        }
    }
}

/// The `//` comment lines right above line `line` (1-based) of `src`.
pub fn comment_above(src: &str, line: u32) -> String {
    let lines: Vec<&str> = src.lines().collect();
    let mut k = line as usize;
    let mut out = Vec::new();
    while k >= 2 {
        let l = lines.get(k - 2).map(|l| l.trim()).unwrap_or("");
        let Some(text) = l.strip_prefix("//") else { break };
        out.push(text.strip_prefix(' ').unwrap_or(text).to_string());
        k -= 1;
    }
    out.reverse();
    out.join("\n")
}

pub fn fn_sig(f: &FnDecl, src: &str) -> String {
    let tp = if f.type_params.is_empty() { String::new() } else { format!("[{}]", f.type_params.join(", ")) };
    let ps: Vec<String> = f
        .params
        .iter()
        .map(|p| {
            let mut s = p.name.clone();
            if let Some(t) = &p.ty {
                s.push_str(&format!(": {}", show_type(t)));
            }
            if let Some(d) = &p.default {
                let text = src.get(d.span.start..d.span.end).unwrap_or("…");
                s.push_str(&format!(" = {text}"));
            }
            s
        })
        .collect();
    let ret = f.ret.as_ref().map(|r| format!(" -> {}", show_type(r))).unwrap_or_default();
    format!("fn {}{tp}({}){ret}", f.name, ps.join(", "))
}

pub fn type_sig(t: &TypeDecl) -> String {
    let tp = if t.params.is_empty() { String::new() } else { format!("[{}]", t.params.join(", ")) };
    match &t.kind {
        TypeKind::Record(fs) => {
            let fields: Vec<String> = fs.iter().map(|(n, ft, _)| format!("    {n}: {}", show_type(ft))).collect();
            format!("type {}{tp} = {{\n{}\n}}", t.name, fields.join(",\n"))
        }
        TypeKind::Union(vs) => {
            let vs: Vec<String> = vs
                .iter()
                .map(|v| if v.fields.is_empty() { v.name.clone() } else { format!("{}({})", v.name, v.fields.iter().map(show_type).collect::<Vec<_>>().join(", ")) })
                .collect();
            format!("type {}{tp} = {}", t.name, vs.join(" | "))
        }
    }
}

fn extern_items(b: &ExternBlock, src: &str, out: &mut Vec<DocItem>) {
    // the block's comment describes the group of functions in it
    let block_doc = comment_above(src, b.span.line);
    if !block_doc.is_empty() && b.fns.iter().any(|f| b.is_pub || f.is_pub) {
        out.push(DocItem { kind: "group", name: String::new(), sig: String::new(), doc: block_doc });
    }
    for f in b.fns.iter() {
        if !(b.is_pub || f.is_pub) {
            continue;
        }
        let tp = if f.type_params.is_empty() { String::new() } else { format!("[{}]", f.type_params.join(", ")) };
        let mut ps: Vec<String> = f.params.iter().map(|(n, t)| format!("{n}: {}", show_type(t))).collect();
        if f.variadic {
            ps.push("...".into());
        }
        let ret = f.ret.as_ref().map(|r| format!(" -> {}", show_type(r))).unwrap_or_default();
        let doc = comment_above(src, f.span.line);
        out.push(DocItem { kind: "fn", name: f.name.clone(), sig: format!("fn {}{tp}({}){ret}", f.name, ps.join(", ")), doc });
    }
}

fn walk(m: &ModuleDecl, path: &str, src: &str, root: bool, out: &mut Vec<DocModule>) {
    let mut items = Vec::new();
    let mut subs = Vec::new();
    for it in &m.items {
        match it {
            Item::Fn(f) if f.is_pub && !f.name.starts_with("test ") => {
                items.push(DocItem { kind: "fn", name: f.name.clone(), sig: fn_sig(f, src), doc: comment_above(src, f.span.line) })
            }
            Item::Type(t) if t.is_pub => items.push(DocItem { kind: "type", name: t.name.clone(), sig: type_sig(t), doc: comment_above(src, t.span.line) }),
            Item::Extern(b) => extern_items(b, src, &mut items),
            Item::Module(sub) if sub.is_pub => subs.push(sub),
            _ => {}
        }
    }
    let doc = if root { String::new() } else { comment_above(src, m.span.line) };
    if !items.is_empty() || !doc.is_empty() {
        out.push(DocModule { path: path.to_string(), doc, items });
    }
    for s in subs {
        walk(s, &format!("{path}::{}", s.name), src, false, out);
    }
}

/// The public items of `package`'s files. `strip`: a root module name left
/// out of paths (the standard library's `prelude`).
pub fn collect(p: &Program, package: &str, strip: Option<&str>) -> Vec<DocModule> {
    let mut out = Vec::new();
    for f in p.files.iter().filter(|f| f.package == package) {
        walk(&f.ast.module, &f.ast.module.name, &f.src, true, &mut out);
    }
    if let Some(s) = strip {
        for m in &mut out {
            if let Some(rest) = m.path.strip_prefix(&format!("{s}::")) {
                m.path = rest.to_string();
            }
        }
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    out
}

pub fn to_json(title: &str, mods: &[DocModule]) -> Value {
    json!({
        "package": title,
        "modules": mods.iter().map(|m| json!({
            "path": m.path,
            "doc": m.doc,
            "items": m.items.iter().map(|i| json!({"kind": i.kind, "name": i.name, "signature": i.sig, "doc": i.doc})).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
    })
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// Comment text as HTML: paragraphs at blank lines, `code` in backquotes.
fn prose(s: &str) -> String {
    let mut out = String::new();
    for para in s.split("\n\n").filter(|p| !p.trim().is_empty()) {
        let mut h = String::new();
        for (k, part) in esc(para).split('`').enumerate() {
            if k % 2 == 1 {
                h.push_str(&format!("<code>{part}</code>"));
            } else {
                h.push_str(part);
            }
        }
        out.push_str(&format!("<p>{}</p>", h.replace('\n', " ")));
    }
    out
}

pub fn to_html(title: &str, mods: &[DocModule]) -> String {
    let mut nav = String::new();
    let mut body = String::new();
    for m in mods {
        let id = m.path.replace("::", "-");
        nav.push_str(&format!("<li><a href=\"#{id}\">{}</a></li>", esc(&m.path)));
        body.push_str(&format!("<section class=\"module\" id=\"{id}\"><h2>{}</h2>{}", esc(&m.path), prose(&m.doc)));
        for it in &m.items {
            if it.kind == "group" {
                body.push_str(&format!("<div class=\"group\">{}</div>", prose(&it.doc)));
                continue;
            }
            let aid = format!("{id}-{}", it.name);
            body.push_str(&format!(
                "<article class=\"item\" id=\"{aid}\" data-name=\"{} {}\"><div class=\"head\"><span class=\"kind\">{}</span> <a href=\"#{aid}\">{}</a></div><pre><code>{}</code></pre>{}</article>",
                esc(&m.path.to_lowercase()),
                esc(&it.name.to_lowercase()),
                it.kind,
                esc(&it.name),
                esc(&it.sig),
                prose(&it.doc)
            ));
        }
        body.push_str("</section>");
    }
    format!(
        r#"<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>{t} · docs</title>
<style>
:root {{ --bg:#fbfaf8; --fg:#1d1d1f; --muted:#6b6b70; --line:#e4e1dc; --code:#f1eee9; --accent:#2f5bd3; }}
@media (prefers-color-scheme: dark) {{ :root {{ --bg:#141416; --fg:#e8e6e3; --muted:#9a9aa0; --line:#2c2c30; --code:#1e1e22; --accent:#7ea2ff; }} }}
* {{ box-sizing:border-box; }}
body {{ margin:0; background:var(--bg); color:var(--fg); font:15px/1.55 -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif; }}
.wrap {{ display:flex; min-height:100vh; }}
nav {{ width:240px; flex:none; border-right:1px solid var(--line); padding:20px 16px; position:sticky; top:0; height:100vh; overflow:auto; }}
nav h1 {{ font-size:18px; margin:0 0 12px; }}
nav input {{ width:100%; padding:7px 9px; border:1px solid var(--line); border-radius:6px; background:var(--bg); color:var(--fg); font:inherit; }}
nav ul {{ list-style:none; padding:0; margin:14px 0 0; }}
nav li a {{ display:block; padding:3px 0; color:var(--fg); text-decoration:none; font-family:ui-monospace, Menlo, monospace; font-size:13px; }}
nav li a:hover {{ color:var(--accent); }}
main {{ flex:1; padding:24px 32px; max-width:900px; min-width:0; }}
h2 {{ font-family:ui-monospace, Menlo, monospace; font-size:20px; border-bottom:1px solid var(--line); padding-bottom:6px; margin-top:36px; }}
.item {{ margin:18px 0; }}
.group {{ margin:26px 0 4px; padding-left:10px; border-left:3px solid var(--line); color:var(--muted); }}
.head a {{ font-family:ui-monospace, Menlo, monospace; font-weight:600; color:var(--fg); text-decoration:none; }}
.kind {{ font-size:12px; color:var(--muted); text-transform:uppercase; letter-spacing:.04em; }}
pre {{ background:var(--code); padding:10px 12px; border-radius:6px; overflow:auto; margin:6px 0; }}
code {{ font-family:ui-monospace, Menlo, monospace; font-size:13px; }}
p code {{ background:var(--code); padding:1px 4px; border-radius:4px; }}
p {{ margin:6px 0; color:var(--fg); }}
@media (max-width:720px) {{ .wrap {{ display:block; }} nav {{ width:auto; height:auto; position:static; border-right:none; border-bottom:1px solid var(--line); }} main {{ padding:16px; }} }}
</style></head>
<body><div class="wrap"><nav><h1>{t}</h1><input id="q" type="search" placeholder="Search" aria-label="Search"><ul>{nav}</ul></nav>
<main>{body}</main></div>
<script>
const q = document.getElementById('q');
q.addEventListener('input', () => {{
  const w = q.value.trim().toLowerCase();
  document.querySelectorAll('.group').forEach(g => {{ g.style.display = w ? 'none' : ''; }});
  document.querySelectorAll('.item').forEach(e => {{ e.style.display = !w || e.dataset.name.includes(w) ? '' : 'none'; }});
  document.querySelectorAll('.module').forEach(s => {{
    const any = [...s.querySelectorAll('.item')].some(e => e.style.display !== 'none');
    s.style.display = any || !w ? '' : 'none';
  }});
}});
</script></body></html>
"#,
        t = esc(title)
    )
}
