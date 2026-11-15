//! `mimic doc`: documentation of a package, written by people or agents.
//!
//! The facts come from the compiler: every public module, function, type and
//! foreign (`extern`) function, with its full signature (type parameters,
//! parameter types, defaults, result) and the `//` comment lines directly
//! above it. The explanations are guides: Markdown pages in the project's
//! `docs/` folder, written by hand or by an agent through `mimic mcp`, whose
//! examples run as tests under `mimic test`. Both become one self-contained
//! HTML page (guides and modules in a sidebar, a search box, code coloured,
//! every glyph explained on hover, light and dark), or JSON for tools.

use mimic_check::Program;
use mimic_syntax::ast::{ExternBlock, FnDecl, Item, ModuleDecl, Type, TypeDecl, TypeKind};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

/// A guide: a Markdown page of the project's `docs/` folder.
pub struct Guide {
    /// `getting-started`, `api/routes`
    pub name: String,
    pub title: String,
    pub markdown: String,
}

/// The guides under `dir` (every `.md`, at any depth), in name order.
pub fn guides_in(dir: &Path) -> Vec<Guide> {
    fn walk(dir: &Path, base: &Path, out: &mut Vec<Guide>) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        let mut entries: Vec<PathBuf> = rd.filter_map(|e| e.ok().map(|e| e.path())).collect();
        entries.sort();
        for p in entries {
            if p.is_dir() {
                walk(&p, base, out);
            } else if p.extension().is_some_and(|e| e == "md")
                && let Ok(text) = std::fs::read_to_string(&p)
            {
                let rel = p.strip_prefix(base).unwrap_or(&p).with_extension("");
                let name = rel.to_string_lossy().replace('\\', "/");
                let (title, body) = front_matter(&text);
                let title = title.or_else(|| body.lines().find_map(|l| l.strip_prefix("# ")).map(|t| t.trim().replace('`', ""))).unwrap_or_else(|| name.clone());
                out.push(Guide { name, title, markdown: body.to_string() });
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out);
    out
}

/// Front matter (`---` … `---`): the title, and the text after it.
fn front_matter(text: &str) -> (Option<String>, &str) {
    let Some(rest) = text.strip_prefix("---\n") else { return (None, text) };
    let Some(end) = rest.find("\n---\n") else { return (None, text) };
    let title = rest[..end].lines().find_map(|l| l.strip_prefix("title:")).map(|t| t.trim().trim_matches('"').to_string());
    (title, &rest[end + 5..])
}

/// The runnable examples of the guides under `dir`: (test name, program,
/// expected output), for `mimic test`.
pub fn guide_examples(dir: &Path) -> Vec<(String, String, String)> {
    let mut out = Vec::new();
    for g in guides_in(dir) {
        for (i, (program, want)) in crate::man::examples_of(&g.markdown).into_iter().enumerate() {
            out.push((format!("docs/{}.md: example {}", g.name, i + 1), program, want));
        }
    }
    out
}

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

pub fn to_json(title: &str, mods: &[DocModule], guides: &[Guide]) -> Value {
    json!({
        "package": title,
        "guides": guides.iter().map(|g| json!({"name": g.name, "title": g.title, "markdown": g.markdown, "examples": crate::man::examples_of(&g.markdown).len()})).collect::<Vec<_>>(),
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

/// Mimic code as HTML: keywords, strings, numbers and comments coloured,
/// and every glyph with its word and meaning on hover.
pub fn highlight(code: &str) -> String {
    use mimic_syntax::glyphs::{PRIMS, SYNTAX_GLYPHS};
    const KEYWORDS: &[&str] = &[
        "module", "fn", "let", "mut", "pub", "if", "else", "return", "use", "as", "extern", "unsafe", "trusted", "type", "match", "arms", "with",
        "test", "and", "or", "not", "div", "true", "false", "include", "pure", "concurrent", "ordered", "sync",
    ];
    let glyph_tip = |g: &str| -> Option<String> {
        PRIMS.iter().find(|p| p.glyph == g).map(|p| format!("{}: {}", p.word, p.doc)).or_else(|| SYNTAX_GLYPHS.iter().find(|x| x.glyph == g).map(|x| format!("{}: {}", x.word, x.doc)))
    };
    let word_tip = |w: &str| PRIMS.iter().find(|p| p.word == w).map(|p| format!("{} {}: {}", p.glyph, p.word, p.doc));
    let mut out = String::new();
    let cs: Vec<char> = code.chars().collect();
    let mut i = 0;
    while i < cs.len() {
        let c = cs[i];
        let rest: String = cs[i..cs.len().min(i + 2)].iter().collect();
        if rest == "//" || c == '⍝' {
            let end = cs[i..].iter().position(|&x| x == '\n').map_or(cs.len(), |k| i + k);
            out += &format!("<span class=\"c\">{}</span>", esc(&cs[i..end].iter().collect::<String>()));
            i = end;
        } else if c == '"' || c == '\'' {
            let mut j = i + 1;
            while j < cs.len() && cs[j] != c && cs[j] != '\n' {
                if cs[j] == '\\' {
                    j += 1;
                }
                j += 1;
            }
            let end = (j + 1).min(cs.len());
            out += &format!("<span class=\"s\">{}</span>", esc(&cs[i..end].iter().collect::<String>()));
            i = end;
        } else if c.is_ascii_digit() || (c == '¯' && cs.get(i + 1).is_some_and(|d| d.is_ascii_digit())) {
            let mut j = i + 1;
            while j < cs.len() && (cs[j].is_ascii_alphanumeric() || cs[j] == '.' || cs[j] == '_') {
                j += 1;
            }
            out += &format!("<span class=\"n\">{}</span>", esc(&cs[i..j].iter().collect::<String>()));
            i = j;
        } else if c.is_alphabetic() || c == '_' {
            let mut j = i + 1;
            while j < cs.len() && (cs[j].is_alphanumeric() || cs[j] == '_') {
                j += 1;
            }
            let w: String = cs[i..j].iter().collect();
            if KEYWORDS.contains(&w.as_str()) {
                out += &format!("<span class=\"k\">{w}</span>");
            } else if let Some(t) = word_tip(&w) {
                out += &format!("<span class=\"g\" title=\"{}\">{w}</span>", esc(&t));
            } else if w.starts_with(char::is_uppercase) {
                out += &format!("<span class=\"t\">{w}</span>");
            } else {
                out += &esc(&w);
            }
            i = j;
        } else {
            // glyphs: two-character reductions first (+/ ×/ ⌈/ ⌊/)
            let two: String = cs[i..cs.len().min(i + 2)].iter().collect();
            let (g, n) = if two.chars().count() == 2 && glyph_tip(&two).is_some() { (two, 2) } else { (c.to_string(), 1) };
            match glyph_tip(&g) {
                Some(t) if !c.is_ascii() || n == 2 => out += &format!("<span class=\"g\" title=\"{}\">{}</span>", esc(&t), esc(&g)),
                _ => out += &esc(&g),
            }
            i += n;
        }
    }
    out
}

/// A guide's Markdown as HTML: fenced Mimic code is coloured, `output`
/// blocks are shown as a program's output.
fn markdown(md: &str) -> String {
    use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};
    let mut events = Vec::new();
    let mut code: Option<(String, String)> = None;
    for ev in Parser::new_ext(md, Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH) {
        match ev {
            Event::Start(Tag::CodeBlock(kind)) => {
                let info = match kind {
                    CodeBlockKind::Fenced(i) => i.to_string(),
                    CodeBlockKind::Indented => String::new(),
                };
                code = Some((info, String::new()));
            }
            Event::Text(t) if code.is_some() => code.as_mut().unwrap().1.push_str(&t),
            Event::End(TagEnd::CodeBlock) => {
                let (info, text) = code.take().unwrap_or_default();
                let lang = info.split_whitespace().next().unwrap_or("");
                let html = match lang {
                    "mimic" | "mim" => format!("<pre class=\"code\"><code>{}</code></pre>", highlight(text.trim_end())),
                    "output" => format!("<pre class=\"output\"><span class=\"label\">output</span><code>{}</code></pre>", esc(text.trim_end())),
                    _ => format!("<pre><code>{}</code></pre>", esc(text.trim_end())),
                };
                events.push(Event::Html(html.into()));
            }
            // a Markdown link to another guide (`routes.md`) points into this page
            Event::Start(Tag::Link { link_type, dest_url, title, id }) if dest_url.ends_with(".md") && !dest_url.contains("://") => {
                let name = dest_url.trim_end_matches(".md").trim_start_matches("./").to_string();
                events.push(Event::Start(Tag::Link { link_type, dest_url: format!("#guide-{}", anchor(&name)).into(), title, id }));
            }
            other => events.push(other),
        }
    }
    let mut out = String::new();
    pulldown_cmark::html::push_html(&mut out, events.into_iter());
    out
}

fn anchor(s: &str) -> String {
    s.chars().map(|c| if c.is_alphanumeric() { c.to_ascii_lowercase() } else { '-' }).collect()
}

pub fn to_html(title: &str, mods: &[DocModule], guides: &[Guide]) -> String {
    let mut nav = String::new();
    let mut body = String::new();
    if !guides.is_empty() {
        nav.push_str("<li class=\"heading\">Guides</li>");
        for g in guides {
            let id = format!("guide-{}", anchor(&g.name));
            nav.push_str(&format!("<li><a href=\"#{id}\">{}</a></li>", esc(&g.title)));
            let words = g.markdown.to_lowercase().split(|c: char| !c.is_alphanumeric() && c != '_' && c != ':').filter(|w| w.len() > 2).collect::<std::collections::BTreeSet<_>>().into_iter().collect::<Vec<_>>().join(" ");
            body.push_str(&format!("<section class=\"guide\" id=\"{id}\" data-name=\"{} {}\">{}</section>", esc(&g.title.to_lowercase()), esc(&words), markdown(&g.markdown)));
        }
    }
    if !mods.is_empty() {
        nav.push_str("<li class=\"heading\">Modules</li>");
    }
    for m in mods {
        let id = m.path.replace("::", "-");
        nav.push_str(&format!("<li><a class=\"mono\" href=\"#{id}\">{}</a></li>", esc(&m.path)));
        body.push_str(&format!("<section class=\"module\" id=\"{id}\"><h2>{}</h2>{}", esc(&m.path), prose(&m.doc)));
        for it in &m.items {
            if it.kind == "group" {
                body.push_str(&format!("<div class=\"group\">{}</div>", prose(&it.doc)));
                continue;
            }
            let aid = format!("{id}-{}", it.name);
            body.push_str(&format!(
                "<article class=\"item\" id=\"{aid}\" data-name=\"{} {}\"><div class=\"head\"><span class=\"kind\">{}</span> <a href=\"#{aid}\">{}</a></div><pre class=\"code\"><code>{}</code></pre>{}</article>",
                esc(&m.path.to_lowercase()),
                esc(&it.name.to_lowercase()),
                it.kind,
                esc(&it.name),
                highlight(&it.sig),
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
:root {{ --bg:#fbf8f5; --panel:#f3eee9; --fg:#1b1714; --muted:#6f655d; --line:#e4dcd4; --code:#f3eee9; --accent:#c24003; --kw:#ae3800; --str:#2f7d4a; --num:#8a4fbf; --com:#8f857c; --glyph:#c24003; --type:#1f6f8b; }}
@media (prefers-color-scheme: dark) {{ :root {{ --bg:#0a0a0a; --panel:#121110; --fg:#f6f1ec; --muted:#a69c94; --line:#26221f; --code:#151312; --accent:#e24b04; --kw:#ff6a1f; --str:#9fd3a8; --num:#c9a7ef; --com:#7d746c; --glyph:#ff6a1f; --type:#7cc4dc; }} }}
* {{ box-sizing:border-box; }}
body {{ margin:0; background:var(--bg); color:var(--fg); font:15px/1.6 "Space Grotesk", system-ui, -apple-system, "Segoe UI", sans-serif; }}
.wrap {{ display:flex; min-height:100vh; }}
nav {{ width:250px; flex:none; background:var(--panel); border-right:1px solid var(--line); padding:20px 16px; position:sticky; top:0; height:100vh; overflow:auto; }}
nav .brand {{ font-weight:700; letter-spacing:-0.02em; font-size:19px; margin:0 0 2px; }}
nav .rule {{ width:40px; height:3px; background:var(--accent); margin:6px 0 14px; border-radius:2px; }}
nav input {{ width:100%; padding:7px 9px; border:1px solid var(--line); border-radius:6px; background:var(--bg); color:var(--fg); font:inherit; }}
nav ul {{ list-style:none; padding:0; margin:14px 0 0; }}
nav li.heading {{ margin:16px 0 4px; font:500 11px/1 "JetBrains Mono", ui-monospace, monospace; letter-spacing:.18em; text-transform:uppercase; color:var(--muted); }}
nav li a {{ display:block; padding:3px 0; color:var(--fg); text-decoration:none; font-size:14px; }}
nav li a.mono {{ font-family:"JetBrains Mono", ui-monospace, Menlo, monospace; font-size:13px; }}
nav li a:hover {{ color:var(--accent); }}
main {{ flex:1; padding:28px 40px; max-width:920px; min-width:0; }}
h1, h2, h3 {{ letter-spacing:-0.01em; }}
.module h2 {{ font-family:"JetBrains Mono", ui-monospace, Menlo, monospace; font-size:20px; border-bottom:1px solid var(--line); padding-bottom:6px; margin-top:40px; }}
.guide {{ border-bottom:1px solid var(--line); padding-bottom:20px; margin-bottom:12px; }}
.guide h1 {{ font-size:28px; margin-top:8px; }}
.item {{ margin:18px 0; }}
.group {{ margin:26px 0 4px; padding-left:10px; border-left:3px solid var(--accent); color:var(--muted); }}
.head a {{ font-family:"JetBrains Mono", ui-monospace, Menlo, monospace; font-weight:600; color:var(--fg); text-decoration:none; }}
.kind {{ font:500 11px/1 "JetBrains Mono", ui-monospace, monospace; color:var(--muted); text-transform:uppercase; letter-spacing:.12em; }}
pre {{ background:var(--code); border:1px solid var(--line); padding:10px 12px; border-radius:8px; overflow:auto; margin:8px 0; }}
pre.output {{ background:transparent; border-style:dashed; position:relative; padding-top:22px; }}
pre.output .label {{ position:absolute; top:5px; left:12px; font:500 10px/1 "JetBrains Mono", ui-monospace, monospace; letter-spacing:.18em; text-transform:uppercase; color:var(--muted); }}
code {{ font-family:"JetBrains Mono", "DejaVu Sans Mono", ui-monospace, Menlo, monospace; font-size:13px; }}
p code, li code, td code {{ background:var(--code); padding:1px 4px; border-radius:4px; }}
table {{ border-collapse:collapse; margin:10px 0; }}
th, td {{ border:1px solid var(--line); padding:5px 9px; text-align:left; vertical-align:top; }}
a {{ color:var(--accent); }}
.k {{ color:var(--kw); font-weight:600; }} .s {{ color:var(--str); }} .n {{ color:var(--num); }} .c {{ color:var(--com); font-style:italic; }} .t {{ color:var(--type); }}
.g {{ color:var(--glyph); font-family:"DejaVu Sans Mono", ui-monospace, monospace; cursor:help; border-bottom:1px dotted var(--line); }}
footer {{ margin:40px 0 10px; color:var(--muted); font-size:13px; }}
@media (max-width:760px) {{ .wrap {{ display:block; }} nav {{ width:auto; height:auto; position:static; border-right:none; border-bottom:1px solid var(--line); }} main {{ padding:16px; }} }}
</style></head>
<body><div class="wrap"><nav><div class="brand">{t}</div><div class="rule"></div><input id="q" type="search" placeholder="Search" aria-label="Search"><ul>{nav}</ul></nav>
<main>{body}<footer>Made with <code>mimic doc</code>. Hover a glyph to see its word and what it does.</footer></main></div>
<script>
const q = document.getElementById('q');
q.addEventListener('input', () => {{
  const w = q.value.trim().toLowerCase();
  document.querySelectorAll('.group').forEach(g => {{ g.style.display = w ? 'none' : ''; }});
  document.querySelectorAll('.item, .guide').forEach(e => {{ e.style.display = !w || e.dataset.name.includes(w) ? '' : 'none'; }});
  document.querySelectorAll('.module').forEach(s => {{
    const any = [...s.querySelectorAll('.item')].some(e => e.style.display !== 'none');
    s.style.display = any || !w ? '' : 'none';
  }});
}});
</script>
</body></html>
"#,
        t = esc(title)
    )
}
