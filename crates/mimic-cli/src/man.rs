//! `mimic man`: the manual inside `mimic`, the same version as the compiler
//! and readable offline. It holds the tutorial, the reference pages and the
//! guides (all of docs/, embedded when `mimic` is built), plus a page made
//! from the compiler itself for every standard function and module, every
//! glyph and every error code, so nothing lacks a page.

use crate::engine::{self, StdItem};
use mimic_syntax::codes;
use mimic_syntax::glyphs::{PRIMS, SYNTAX_GLYPHS};
use serde_json::{Value, json};
use std::io::{IsTerminal, Write};

mod embedded {
    include!(concat!(env!("OUT_DIR"), "/manual.rs"));
}

/// A page of the manual.
pub struct Page {
    /// What `mimic man <name>` finds it by: `arrays-and-glyphs`, `std`,
    /// `fs::read`, `keep`, `E0303`.
    pub name: String,
    pub title: String,
    /// `tutorial`, `reference`, `guide`, `function`, `module`, `type`,
    /// `glyph` or `error`.
    pub kind: &'static str,
    /// Markdown.
    pub text: String,
    /// A tutorial chapter's number.
    pub chapter: Option<usize>,
}

pub struct Manual {
    pub pages: Vec<Page>,
}

/// Front matter (`---` … `---`): the title, and the text after it.
fn front_matter(text: &str) -> (Option<String>, &str) {
    let Some(rest) = text.strip_prefix("---\n") else { return (None, text) };
    let Some(end) = rest.find("\n---\n") else { return (None, text) };
    let title = rest[..end].lines().find_map(|l| l.strip_prefix("title:")).map(|t| t.trim().trim_matches('"').to_string());
    (title, &rest[end + 5..])
}

fn first_heading(text: &str) -> Option<String> {
    text.lines().find_map(|l| l.strip_prefix("# ")).map(|t| t.trim().replace('`', ""))
}

impl Manual {
    pub fn load() -> Manual {
        let mut pages = Vec::new();
        for (path, raw) in embedded::PAGES {
            let (fm_title, body) = front_matter(raw);
            let stem = path.rsplit('/').next().unwrap_or(path).trim_end_matches(".md");
            let (kind, name) = match path.split_once('/') {
                Some(("tutorial", _)) => ("tutorial", stem.trim_start_matches(|c: char| c.is_ascii_digit() || c == '-').to_string()),
                Some(("manual", _)) => ("guide", stem.to_string()),
                _ => ("reference", stem.to_string()),
            };
            // the plan and the list of gaps are for the people making Mimic
            if kind == "reference" && matches!(stem, "roadmap" | "gaps") {
                continue;
            }
            let name = if name.is_empty() || name == "index" { if kind == "tutorial" { "tutorial".into() } else { stem.to_string() } } else { name };
            let title = fm_title.or_else(|| first_heading(body)).unwrap_or_else(|| name.clone());
            let chapter = (kind == "tutorial").then(|| stem.split('-').next().and_then(|n| n.parse().ok())).flatten();
            pages.push(Page { name, title, kind, text: body.to_string(), chapter });
        }
        // (chapter number, text)
        let tutorial: Vec<(String, String)> =
            pages.iter().filter_map(|p| p.chapter.map(|n| (n.to_string(), p.text.clone()))).collect();
        let std_md = pages.iter().find(|p| p.name == "std").map(|p| p.text.clone()).unwrap_or_default();
        let items = engine::std_items();
        for it in &items {
            pages.push(std_page(it, &items, &std_md, &tutorial));
        }
        for g in PRIMS {
            let mut t = format!("# {} {}\n\n{}\n\n", g.glyph, g.word, g.doc);
            t += &format!("| Glyph | Word | Type it as | Arguments |\n|---|---|---|---|\n| `{}` | `{}` | {} | {} |\n", g.glyph, g.word, g.digraph.map(|d| format!("`{d}`")).unwrap_or("—".into()), g.arity);
            if let Some(a) = g.ascii {
                t += &format!("\nASCII spelling: `{a}`.\n");
            }
            t += &examples(&[g.glyph, g.word], &tutorial);
            pages.push(Page { name: g.word.to_string(), title: format!("{} {}", g.glyph, g.word), kind: "glyph", text: t, chapter: None });
        }
        for g in SYNTAX_GLYPHS {
            let mut t = format!("# {} {}\n\n{}\n\n", g.glyph, g.word, g.doc);
            t += &format!("| Glyph | Word | Type it as |\n|---|---|---|\n| `{}` | `{}` | {} |\n", g.glyph, g.word, g.digraph.map(|d| format!("`{d}`")).unwrap_or("—".into()));
            t += &examples(&[g.glyph], &tutorial);
            pages.push(Page { name: g.glyph.to_string(), title: format!("{} {}", g.glyph, g.word), kind: "glyph", text: t, chapter: None });
        }
        for c in codes::ALL {
            let t = format!("# {} {}\n\n**{}**\n\n{}\n\n`mimic explain {}` prints this too; `mimic check --fix` applies the fixes the compiler is sure of.\n", c.id, c.name, c.title, c.explain, c.id);
            pages.push(Page { name: c.id.to_string(), title: format!("{} {}", c.id, c.name), kind: "error", text: t, chapter: None });
        }
        Manual { pages }
    }

    /// The page for `query`: a page name, a chapter number, a glyph, a
    /// glyph's word or digraph, an error code or name, a standard function
    /// (`fs::read`, or `read` when only one is called that), or a title.
    pub fn find(&self, query: &str) -> Result<&Page, Vec<&Page>> {
        let q = query.trim();
        let ql = q.to_lowercase();
        let glyph_of_digraph = mimic_syntax::glyphs::digraph(q);
        let by = |f: &dyn Fn(&Page) -> bool| self.pages.iter().find(|p| f(p));
        // a chapter's name may also be a module's (`json`): the module wins, and
        // the chapter is `tutorial/json`, or its number
        let exact = by(&|p| p.name == q && p.kind != "tutorial")
            .or_else(|| by(&|p| p.name == q))
            .or_else(|| q.strip_prefix("tutorial/").and_then(|n| by(&|p| p.kind == "tutorial" && p.name == n)))
            .or_else(|| by(&|p| p.name.eq_ignore_ascii_case(q)))
            .or_else(|| codes::find(q).and_then(|c| by(&|p| p.kind == "error" && p.name == c.id)))
            .or_else(|| {
                // a glyph, or its digraph
                let g = glyph_of_digraph.unwrap_or(q);
                PRIMS.iter().find(|p| p.glyph == g || p.ascii == Some(g)).and_then(|p| by(&|x| x.kind == "glyph" && x.name == p.word))
            })
            .or_else(|| by(&|p| p.kind == "glyph" && p.title.split(' ').nth(1).is_some_and(|w| w == q)))
            .or_else(|| q.parse::<usize>().ok().and_then(|n| by(&|p| p.chapter == Some(n))))
            .or_else(|| by(&|p| p.title.to_lowercase() == ql));
        if let Some(p) = exact {
            return Ok(p);
        }
        // a function by its own name, when only one has it
        let named: Vec<&Page> = self.pages.iter().filter(|p| p.kind == "function" && p.name.rsplit("::").next() == Some(q)).collect();
        if named.len() == 1 {
            return Ok(named[0]);
        }
        if named.len() > 1 {
            return Err(named);
        }
        // suggestions: names containing it, then close spellings
        let mut near: Vec<&Page> = self.pages.iter().filter(|p| p.name.to_lowercase().contains(&ql) || p.title.to_lowercase().contains(&ql)).collect();
        if near.is_empty() {
            let names: Vec<&str> = self.pages.iter().map(|p| p.name.as_str()).collect();
            if let Some(best) = mimic_syntax::closest(q, names.iter().copied()) {
                near.extend(self.pages.iter().filter(|p| p.name == best));
            }
        }
        near.truncate(12);
        Err(near)
    }

    /// Every line of every page containing `text` (case-insensitive):
    /// (page, heading above it, the line).
    pub fn search(&self, text: &str) -> Vec<(&Page, String, String)> {
        let t = text.to_lowercase();
        let mut out = Vec::new();
        for p in &self.pages {
            let mut heading = String::new();
            for line in p.text.lines() {
                if let Some(h) = line.strip_prefix('#') {
                    heading = h.trim_start_matches('#').trim().replace('`', "");
                }
                if line.to_lowercase().contains(&t) {
                    out.push((p, heading.clone(), line.trim().to_string()));
                }
            }
        }
        // pages written by people first, then the generated ones
        let rank = |k: &str| match k {
            "tutorial" => 0,
            "guide" => 1,
            "reference" => 2,
            _ => 3,
        };
        out.sort_by_key(|(p, ..)| rank(p.kind));
        out
    }

    /// The front page: how to use the manual, and what is in it.
    pub fn index(&self) -> String {
        let mut t = String::from(
            "# The Mimic manual\n\nThe tutorial, the reference and the guides, the same version as this `mimic`.\n\n\
             ```sh\nmimic man arrays-and-glyphs     # a chapter (or `mimic man 3`)\nmimic man fs::read              # a standard function, or a module: `mimic man fs`\nmimic man ⌿                     # a glyph, or its word: `mimic man keep`\nmimic man E0303                 # an error code\nmimic man --search \"sort by\"    # every page that mentions it\nmimic man --run 4               # run a page's examples\n```\n",
        );
        let mut section = |title: &str, kind: &str| {
            let rows: Vec<String> = self
                .pages
                .iter()
                .filter(|p| p.kind == kind)
                .map(|p| match p.chapter {
                    Some(n) => format!("| `{n}` | {} (`{}`) |", p.title, p.name),
                    None => format!("| `{}` | {} |", p.name, p.title),
                })
                .collect();
            if !rows.is_empty() {
                t += &format!("\n## {title}\n\n| mimic man | Page |\n|---|---|\n{}\n", rows.join("\n"));
            }
        };
        section("Tutorial", "tutorial");
        section("Guides", "guide");
        section("Reference", "reference");
        let count = |k: &str| self.pages.iter().filter(|p| p.kind == k).count();
        t += &format!(
            "\n## Generated from the compiler\n\n{} standard functions and {} modules (`mimic man --list function`), {} glyphs (`mimic glyphs`), {} error codes (`mimic explain`).\n",
            count("function"),
            count("module"),
            count("glyph"),
            count("error")
        );
        t
    }
}

/// The page of a standard function, type or module.
fn std_page(it: &StdItem, all: &[StdItem], std_md: &str, tutorial: &[(String, String)]) -> Page {
    let mut t = format!("# {}\n\n```mimic\n{}\n```\n\n", it.path, it.signature);
    if !it.doc.is_empty() {
        t += &format!("{}\n\n", it.doc);
    }
    if let Some(e) = it.effects {
        let meaning = match e {
            "pure" => "no effects: calls may run in parallel",
            "concurrent" => "effects that may overlap (reading, waiting): calls may run at the same time",
            "ordered" => "effects that keep their order (such as printing)",
            _ => "waits for pending work first (writing, running programs)",
        };
        t += &format!("**Effects:** {e}, {meaning}.\n\n");
    }
    let kind = match it.kind {
        "module" => "module",
        "type" | "variant" => "type",
        _ => "function",
    };
    if kind == "module" {
        let prefix = format!("{}::", it.path);
        let members: Vec<&StdItem> = all.iter().filter(|x| x.path.strip_prefix(&prefix).is_some_and(|r| !r.contains("::"))).collect();
        if !members.is_empty() {
            t += "| Item | Signature |\n|---|---|\n";
            for m in members {
                let one_line = m.signature.split_whitespace().collect::<Vec<_>>().join(" ").replace("{ ", "{").replace(", }", "}");
                t += &format!("| `{}` | `{}` |\n", m.path, one_line.replace('|', "\\|"));
            }
            t += "\n";
        }
    }
    // what the reference says about it: its rows in docs/std.md
    let call = format!("`{}(", it.path);
    let rows: Vec<&str> = std_md.lines().filter(|l| l.starts_with('|') && (l.contains(&call) || l.contains(&format!("`{}`", it.path)))).collect();
    if !rows.is_empty() {
        // the table's header: the row above the `|---|` line before the first match
        let lines: Vec<&str> = std_md.lines().collect();
        let first = lines.iter().position(|l| rows.contains(l)).unwrap_or(0);
        let header = (1..=first).rev().find(|&i| lines[i].starts_with("|---")).map_or("| | |", |i| lines[i - 1]);
        let cols = header.matches('|').count().saturating_sub(1);
        t += &format!("From the standard library reference (`mimic man std`):\n\n{header}\n|{}\n{}\n\n", "---|".repeat(cols.max(1)), rows.join("\n"));
    }
    if kind == "function" {
        t += &examples(&[&format!("{}(", it.path)], tutorial);
    }
    Page { name: it.path.clone(), title: it.path.clone(), kind, text: t, chapter: None }
}

/// Up to three lines of the tutorial's code that use one of `needles`, with
/// the chapters they are in.
fn examples(needles: &[&str], tutorial: &[(String, String)]) -> String {
    let mut lines = Vec::new();
    let mut chapters = Vec::new();
    for (name, text) in tutorial {
        let mut in_code = false;
        for l in text.lines() {
            if l.starts_with("```") {
                in_code = l.starts_with("```mimic");
                continue;
            }
            let code = l.split("//").next().unwrap_or(l);
            if in_code && needles.iter().any(|n| contains_token(code, n)) {
                if lines.len() < 3 && !lines.iter().any(|x: &String| x == l.trim()) {
                    lines.push(l.trim().to_string());
                }
                if !chapters.contains(name) {
                    chapters.push(name.clone());
                }
            }
        }
    }
    let mut t = String::new();
    if !lines.is_empty() {
        t += &format!("\n## Examples\n\n```mimic\n{}\n```\n", lines.join("\n"));
    }
    if !chapters.is_empty() {
        let list: Vec<&str> = chapters.iter().map(String::as_str).collect();
        t += &format!("\nIn the tutorial: chapter{} {} (`mimic man {}`).\n", if list.len() > 1 { "s" } else { "" }, list.join(", "), list[0]);
    }
    t
}

/// `needle` in `line` as a whole word (glyphs always count).
fn contains_token(line: &str, needle: &str) -> bool {
    if !needle.chars().next().is_some_and(|c| c.is_ascii_alphabetic()) {
        return line.contains(needle);
    }
    let word = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric() || c == '_' || c == ':');
    let mut from = 0;
    while let Some(i) = line[from..].find(needle) {
        let at = from + i;
        let end = at + needle.len();
        if !word(line[..at].chars().next_back()) && (needle.ends_with('(') || !word(line[end..].chars().next())) {
            return true;
        }
        from = end;
    }
    false
}

// ---- showing a page ---------------------------------------------------------

struct Style {
    on: bool,
}

impl Style {
    fn paint(&self, code: &str, s: &str) -> String {
        if self.on { format!("\x1b[{code}m{s}\x1b[0m") } else { s.to_string() }
    }
}

/// Markdown for a terminal: headings stand out, code is indented and
/// coloured, tables are aligned, links show their text.
pub fn render(md: &str, color: bool) -> String {
    let st = Style { on: color };
    let mut out = String::new();
    let mut fence: Option<String> = None;
    let mut table: Vec<Vec<String>> = Vec::new();
    let flush_table = |table: &mut Vec<Vec<String>>, out: &mut String| {
        if table.is_empty() {
            return;
        }
        let cols = table.iter().map(Vec::len).max().unwrap_or(0);
        let mut w = vec![0; cols];
        for row in table.iter() {
            for (i, c) in row.iter().enumerate() {
                w[i] = w[i].max(c.chars().count());
            }
        }
        for (r, row) in table.iter().enumerate() {
            let cells: Vec<String> = (0..cols)
                .map(|i| {
                    let c = row.get(i).map(String::as_str).unwrap_or("");
                    let pad = " ".repeat(w[i] - c.chars().count());
                    if r == 0 { st.paint("1", &format!("{c}{pad}")) } else { format!("{c}{pad}") }
                })
                .collect();
            out.push_str(&format!("  {}\n", cells.join("  ").trim_end()));
        }
        table.clear();
    };
    for line in md.lines() {
        if let Some(lang) = &fence {
            if line.starts_with("```") {
                fence = None;
                out.push('\n');
                continue;
            }
            let colour = if lang == "output" { "2" } else { "36" };
            out.push_str(&format!("    {}\n", st.paint(colour, line)));
            continue;
        }
        if let Some(info) = line.strip_prefix("```") {
            flush_table(&mut table, &mut out);
            let lang = info.split_whitespace().next().unwrap_or("").to_string();
            if lang == "output" {
                out.push_str(&format!("    {}\n", st.paint("2", "output:")));
            }
            fence = Some(lang);
            continue;
        }
        if line.starts_with('|') {
            if line.chars().all(|ch| matches!(ch, '|' | '-' | ':' | ' ')) {
                continue; // the line under the header
            }
            let cells: Vec<String> = split_row(line).iter().map(|c| inline(c, &Style { on: false })).collect();
            table.push(cells);
            continue;
        }
        flush_table(&mut table, &mut out);
        if let Some(h) = line.strip_prefix("# ") {
            out.push_str(&format!("{}\n\n", st.paint("1;4", &inline(h, &Style { on: false }))));
        } else if let Some(h) = line.strip_prefix("## ") {
            out.push_str(&format!("{}\n", st.paint("1", &inline(h, &Style { on: false }).to_uppercase())));
        } else if let Some(h) = line.strip_prefix("### ").or_else(|| line.strip_prefix("#### ")) {
            out.push_str(&format!("{}\n", st.paint("1", &inline(h, &Style { on: false }))));
        } else if line.trim_start().starts_with("import ") && line.contains(" from ") {
            continue; // page components of the documentation site
        } else {
            out.push_str(&format!("{}\n", inline(line, &st)));
        }
    }
    flush_table(&mut table, &mut out);
    let mut s = out.replace("\n\n\n", "\n\n");
    while s.contains("\n\n\n") {
        s = s.replace("\n\n\n", "\n\n");
    }
    s.trim_start().to_string()
}

/// The cells of a table row; `\|` and `|` inside `code` stay in the cell.
fn split_row(line: &str) -> Vec<String> {
    let inner = line.trim().strip_prefix('|').unwrap_or(line.trim());
    let inner = inner.strip_suffix('|').unwrap_or(inner);
    let mut cells = Vec::new();
    let mut cur = String::new();
    let mut code = false;
    let mut chars = inner.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' if chars.peek() == Some(&'|') => cur.push(chars.next().unwrap()),
            '`' => {
                code = !code;
                cur.push(c);
            }
            '|' if !code => cells.push(std::mem::take(&mut cur).trim().to_string()),
            _ => cur.push(c),
        }
    }
    cells.push(cur.trim().to_string());
    cells
}

/// Inline Markdown: `code`, **bold**, [links](…).
fn inline(s: &str, st: &Style) -> String {
    let mut out = String::new();
    let mut rest = s;
    while !rest.is_empty() {
        if let Some(r) = rest.strip_prefix('`')
            && let Some(end) = r.find('`')
        {
            out.push_str(&if st.on { st.paint("36", &r[..end]) } else { format!("`{}`", &r[..end]) });
            rest = &r[end + 1..];
        } else if let Some(r) = rest.strip_prefix("**")
            && let Some(end) = r.find("**")
        {
            out.push_str(&st.paint("1", &r[..end]));
            rest = &r[end + 2..];
        } else if let Some(r) = rest.strip_prefix('[')
            && let Some(close) = r.find("](")
            && let Some(end) = r[close..].find(')')
        {
            let text = &r[..close];
            let target = &r[close + 2..close + end];
            out.push_str(&inline(text, st));
            if target.starts_with("http") {
                out.push_str(&format!(" ({target})"));
            }
            rest = &r[close + end + 1..];
        } else {
            let c = rest.chars().next().unwrap();
            out.push(c);
            rest = &rest[c.len_utf8()..];
        }
    }
    out
}

/// Print, through a pager when the text is longer than the terminal.
fn show(text: &str) {
    let tty = std::io::stdout().is_terminal();
    let lines = text.lines().count();
    let rows = std::env::var("LINES").ok().and_then(|v| v.parse::<usize>().ok()).unwrap_or(40);
    let pager = std::env::var("MIMIC_PAGER").or_else(|_| std::env::var("PAGER")).unwrap_or_else(|_| "less -R -F -X".into());
    if tty && lines > rows && !pager.trim().is_empty() && !cfg!(windows) {
        let mut parts = pager.split_whitespace();
        if let Some(cmd) = parts.next()
            && let Ok(mut child) = std::process::Command::new(cmd).args(parts).stdin(std::process::Stdio::piped()).spawn()
        {
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(text.as_bytes());
            }
            let _ = child.wait();
            return;
        }
    }
    print!("{text}");
}

fn color() -> bool {
    std::io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none()
}

// ---- running a page's examples -------------------------------------------------

/// The complete programs of a page whose output is shown: (program, output).
pub fn examples_of(md: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut rest = md;
    while let Some(i) = rest.find("```mimic run\n") {
        let after = &rest[i + 13..];
        let Some(end) = after.find("```") else { break };
        let program = after[..end].to_string();
        let tail = &after[end + 3..];
        let output = tail
            .trim_start_matches('\n')
            .strip_prefix("```output\n")
            .and_then(|o| o.find("```").map(|e| o[..e].to_string()))
            .unwrap_or_default();
        out.push((program, output));
        rest = tail;
    }
    out
}

/// Run every example of `page` with this `mimic`; print what differs.
fn run_examples(page: &Page) -> Result<bool, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let examples = examples_of(&page.text);
    if examples.is_empty() {
        println!("`{}` has no runnable examples", page.name);
        return Ok(true);
    }
    let dir = std::env::temp_dir().join(format!("mimic-man-{}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let mut failed = 0;
    for (i, (program, want)) in examples.iter().enumerate() {
        let f = dir.join("example.mim");
        std::fs::write(&f, program).map_err(|e| e.to_string())?;
        let out = std::process::Command::new(&exe).arg("run").arg(&f).output().map_err(|e| e.to_string())?;
        let got = String::from_utf8_lossy(&out.stdout);
        if got == *want {
            println!("ok      example {} of `{}`", i + 1, page.name);
        } else {
            failed += 1;
            println!("FAILED  example {} of `{}`", i + 1, page.name);
            println!("{}", program.trim_end());
            println!("--- expected\n{}--- got\n{}{}", want, got, String::from_utf8_lossy(&out.stderr));
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
    println!("{} example(s), {failed} failed", examples.len());
    Ok(failed == 0)
}

// ---- the command --------------------------------------------------------------

pub fn page_json(p: &Page) -> Value {
    json!({ "name": p.name, "title": p.title, "kind": p.kind, "text": p.text })
}

/// `mimic man [topic…] [--search text] [--run] [--list [kind]] [--json]`
pub fn command(topic: &[String], search: Option<&str>, run: bool, list: Option<&str>, as_json: bool) -> Result<bool, String> {
    let man = Manual::load();
    if let Some(kind) = list {
        let pages: Vec<&Page> = man.pages.iter().filter(|p| kind.is_empty() || p.kind == kind).collect();
        if as_json {
            println!("{}", Value::Array(pages.iter().map(|p| json!({ "name": p.name, "title": p.title, "kind": p.kind })).collect()));
        } else {
            let mut t = String::new();
            for p in pages {
                t += &format!("{:<10} {:<34} {}\n", p.kind, p.name, p.title);
            }
            show(&t);
        }
        return Ok(true);
    }
    if let Some(text) = search {
        let hits = man.search(text);
        if as_json {
            let v: Vec<Value> = hits.iter().map(|(p, h, l)| json!({ "page": p.name, "kind": p.kind, "section": h, "line": l })).collect();
            println!("{}", Value::Array(v));
            return Ok(true);
        }
        if hits.is_empty() {
            println!("nothing in the manual mentions `{text}`");
            return Ok(false);
        }
        let st = Style { on: color() };
        let mut t = String::new();
        let mut last = String::new();
        for (p, heading, line) in hits.iter().take(200) {
            let here = format!("{} › {}", p.name, heading);
            if here != last {
                t += &format!("\n{}\n", st.paint("1", &format!("mimic man {}{}", p.name, if heading.is_empty() { String::new() } else { format!("   ({heading})") })));
                last = here;
            }
            let shown: String = line.chars().take(150).collect();
            t += &format!("    {}\n", inline(&shown, &st));
        }
        if hits.len() > 200 {
            t += &format!("\n… and {} more lines\n", hits.len() - 200);
        }
        show(t.trim_start());
        return Ok(true);
    }
    let query = topic.join(" ");
    if query.is_empty() {
        if as_json {
            println!("{}", json!({ "name": "index", "title": "The Mimic manual", "kind": "index", "text": man.index() }));
        } else {
            show(&render(&man.index(), color()));
        }
        return Ok(true);
    }
    match man.find(&query) {
        Ok(page) => {
            if run {
                return run_examples(page);
            }
            if as_json {
                println!("{}", page_json(page));
            } else {
                show(&render(&page.text, color()));
            }
            Ok(true)
        }
        Err(near) => {
            let mut msg = format!("no page `{query}` in the manual");
            if !near.is_empty() {
                msg += "; did you mean:\n";
                for p in &near {
                    msg += &format!("    mimic man {:<28} {}\n", p.name, p.title);
                }
            } else {
                msg += "\n";
            }
            msg += &format!("  (search every page: mimic man --search \"{query}\")");
            Err(msg)
        }
    }
}
