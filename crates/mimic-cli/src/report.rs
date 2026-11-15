//! Compiler reports: a set of diagnostics plus the sources they point into,
//! rendered either for people (rustc-style text) or for tools and agents
//! (`--emit-json`, schema in docs/diagnostics.md).

use mimic_check::{FileDiag, Program};
use mimic_syntax::codes::{self as C, Code};
use mimic_syntax::{Diag, Span, SrcRef};
use serde_json::{Value, json};

/// Bump when the JSON shape changes incompatibly.
pub const SCHEMA_VERSION: u32 = 1;

/// A source file as the report shows it.
pub struct Source {
    pub display: String,
    pub src: String,
    /// The file on disk (absent for built-in sources like the prelude).
    pub path: Option<std::path::PathBuf>,
}

pub struct Entry {
    /// Index into `Report::files`; `None` for errors with no source location.
    pub file: Option<usize>,
    pub diag: Diag,
}

#[derive(Default)]
pub struct Report {
    pub files: Vec<Source>,
    pub entries: Vec<Entry>,
}

impl Report {
    /// Diagnostics found while checking `p`.
    pub fn from_program(p: &Program, diags: Vec<FileDiag>) -> Report {
        let files = p
            .files
            .iter()
            .map(|f| Source { display: f.display.clone(), src: f.src.clone(), path: f.path.is_file().then(|| f.path.clone()) })
            .collect();
        let entries = diags.into_iter().map(|d| Entry { file: Some(d.file), diag: d.diag }).collect();
        Report { files, entries }
    }

    /// Add a diagnostic for a file that is not (yet) part of a program.
    pub fn push_in(&mut self, path: Option<&std::path::Path>, display: &str, src: &str, diag: Diag) {
        let file = match self.files.iter().position(|f| f.display == display) {
            Some(i) => i,
            None => {
                self.files.push(Source { display: display.into(), src: src.into(), path: path.map(|p| p.to_path_buf()) });
                self.files.len() - 1
            }
        };
        self.entries.push(Entry { file: Some(file), diag });
    }

    /// An error without a source location.
    pub fn push_plain(&mut self, code: &'static Code, msg: impl Into<String>) {
        self.entries.push(Entry { file: None, diag: Diag::new(code, msg, Span::default()) });
    }

    /// A report holding one location-less error.
    pub fn plain(code: &'static Code, msg: impl Into<String>) -> Report {
        let mut r = Report::default();
        r.push_plain(code, msg);
        r
    }

    /// Wrap an error that only exists as text (project loading, toolchain).
    /// Text that is already a rendered diagnostic keeps its code and location.
    pub fn from_text(default: &'static Code, text: &str) -> Report {
        let mut r = Report::default();
        let mut lines = text.trim_end().lines();
        let Some(first) = lines.next() else {
            r.push_plain(default, "unknown error");
            return r;
        };
        let (code, msg) = parse_head(first).unwrap_or((default, first.trim_start_matches("error: ").to_string()));
        let rest: Vec<&str> = lines.collect();
        // `  --> path:line:col` locates rendered diagnostics
        let loc = rest.iter().find_map(|l| l.trim_start().strip_prefix("--> ")).and_then(parse_loc);
        let mut d = Diag::new(code, msg, Span::default());
        for l in &rest {
            let t = l.trim_start();
            if let Some(n) = t.strip_prefix("= note: ") {
                d = d.note(n);
            } else if let Some(h) = t.strip_prefix("= help: ") {
                d = d.help(h);
            }
        }
        match loc {
            Some((path, line, col)) => {
                let src = std::fs::read_to_string(&path).unwrap_or_default();
                let start = offset_of(&src, line, col);
                d.span = Span { start, end: start, line, col };
                let on_disk = std::path::Path::new(&path).is_file().then(|| std::path::PathBuf::from(&path));
                r.push_in(on_disk.as_deref(), &path, &src, d);
            }
            None => {
                // keep tool output (linker errors…) as notes, line by line
                if !rest.is_empty() && d.notes.is_empty() && d.help.is_empty() {
                    d = d.note(rest.join("\n"));
                }
                r.entries.push(Entry { file: None, diag: d });
            }
        }
        r
    }

    /// Apply every machine-applicable fix to the files on disk.
    /// Returns the number of fixes applied.
    pub fn apply_fixes(&self) -> Result<usize, String> {
        use std::collections::BTreeMap;
        // file → [(start, end, text, fix id)]
        let mut per_file: BTreeMap<usize, Vec<(usize, usize, &str, usize)>> = BTreeMap::new();
        let mut fix_id = 0;
        for e in &self.entries {
            for fx in &e.diag.fixes {
                fix_id += 1;
                if fx.applicability != mimic_syntax::Applicability::MachineApplicable {
                    continue;
                }
                for ed in &fx.edits {
                    if let Some(f) = ed.file.or(e.file) {
                        per_file.entry(f).or_default().push((ed.span.start, ed.span.end, &ed.text, fix_id));
                    }
                }
            }
        }
        let mut applied = std::collections::BTreeSet::new();
        for (f, mut edits) in per_file {
            let Some(src) = self.files.get(f) else { continue };
            let Some(path) = &src.path else { continue };
            // a fix whose edits overlap an earlier fix's edits is skipped entirely
            edits.sort_by_key(|e| (e.0, e.1));
            let mut taken: Vec<(usize, usize, &str, usize)> = Vec::new();
            let mut dropped = std::collections::BTreeSet::new();
            for ed in edits {
                if taken.last().is_some_and(|t| ed.0 < t.1 || (ed.0 == t.0 && ed.1 == t.1 && t.3 != ed.3)) {
                    dropped.insert(ed.3);
                    continue;
                }
                taken.push(ed);
            }
            taken.retain(|t| !dropped.contains(&t.3));
            let mut text = src.src.clone();
            for (start, end, rep, id) in taken.iter().rev() {
                if *end <= text.len() && text.is_char_boundary(*start) && text.is_char_boundary(*end) {
                    text.replace_range(*start..*end, rep);
                    applied.insert(*id);
                }
            }
            std::fs::write(path, text).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
        }
        Ok(applied.len())
    }

    pub fn errors(&self) -> usize {
        self.entries.iter().filter(|e| e.diag.severity == mimic_syntax::Severity::Error).count()
    }

    fn src_refs(&self) -> Vec<SrcRef<'_>> {
        self.files.iter().map(|f| SrcRef { display: &f.display, src: &f.src }).collect()
    }

    /// rustc-style text for a terminal.
    pub fn human(&self) -> String {
        let refs = self.src_refs();
        let mut out = String::new();
        for e in &self.entries {
            match e.file {
                Some(f) => out.push_str(&e.diag.render_in(f, &refs)),
                None => {
                    // no location: header, then notes/help
                    out.push_str(&e.diag.render_in(usize::MAX, &refs));
                }
            }
            out.push('\n');
        }
        // every error has a page in the manual
        let mut codes: Vec<&str> = Vec::new();
        for e in &self.entries {
            if !codes.contains(&e.diag.code.id) {
                codes.push(e.diag.code.id);
            }
        }
        match codes.as_slice() {
            [] => {}
            [one] => out.push_str(&format!("For more about this error: `mimic man {one}`\n")),
            many => out.push_str(&format!("For more about these errors: `mimic man {}` (and {})\n", many[0], many[1..].join(", "))),
        }
        let n = self.errors().max(1);
        out.push_str(&format!("error: could not compile due to {n} previous error{}", if n == 1 { "" } else { "s" }));
        out
    }

    /// The machine-readable report (see docs/diagnostics.md).
    pub fn json(&self, command: &str) -> Value {
        let diags: Vec<Value> = self.entries.iter().map(|e| self.diag_json(e)).collect();
        json!({
            "schema": SCHEMA_VERSION,
            "tool": "mimic",
            "version": env!("CARGO_PKG_VERSION"),
            "command": command,
            "success": self.errors() == 0,
            "diagnostics": diags,
            "summary": { "errors": self.errors(), "warnings": self.entries.len() - self.errors() },
        })
    }

    fn loc_json(&self, file: Option<usize>, span: Span) -> Value {
        let Some(f) = file.and_then(|i| self.files.get(i)) else { return Value::Null };
        let (end_line, end_col) = mimic_syntax::line_col(&f.src, span.end);
        json!({
            "file": f.display,
            "start": { "offset": span.start, "line": span.line, "column": span.col },
            "end": { "offset": span.end, "line": end_line, "column": end_col },
            "text": f.src.get(span.start..span.end).unwrap_or(""),
        })
    }

    fn diag_json(&self, e: &Entry) -> Value {
        let d = &e.diag;
        let labels: Vec<Value> = d
            .labels
            .iter()
            .map(|l| json!({ "location": self.loc_json(l.file.or(e.file), l.span), "message": l.msg }))
            .collect();
        let fixes: Vec<Value> = d
            .fixes
            .iter()
            .map(|fx| {
                let edits: Vec<Value> = fx
                    .edits
                    .iter()
                    .map(|ed| json!({ "location": self.loc_json(ed.file.or(e.file), ed.span), "replacement": ed.text }))
                    .collect();
                json!({ "message": fx.msg, "applicability": fx.applicability.as_str(), "edits": edits })
            })
            .collect();
        let refs = self.src_refs();
        json!({
            "severity": d.severity.as_str(),
            "code": d.code.id,
            "name": d.code.name,
            "message": d.msg,
            "location": self.loc_json(e.file, d.span),
            "labels": labels,
            "notes": d.notes,
            "help": d.help,
            "fixes": fixes,
            "rendered": e.file.map(|f| d.render_in(f, &refs)).unwrap_or_else(|| d.render_in(usize::MAX, &refs)),
        })
    }
}

impl From<String> for Report {
    fn from(s: String) -> Report {
        Report::from_text(&C::PROJECT, &s)
    }
}

/// `error[E0123]: message` → (code, message).
fn parse_head(line: &str) -> Option<(&'static Code, String)> {
    let rest = line.strip_prefix("error[")?;
    let (id, msg) = rest.split_once("]: ")?;
    Some((C::find(id)?, msg.to_string()))
}

/// `path:line:col` → (path, line, col).
fn parse_loc(s: &str) -> Option<(String, u32, u32)> {
    let mut it = s.trim().rsplitn(3, ':');
    let col = it.next()?.parse().ok()?;
    let line = it.next()?.parse().ok()?;
    Some((it.next()?.to_string(), line, col))
}

/// Byte offset of 1-based (line, col in chars).
fn offset_of(src: &str, line: u32, col: u32) -> usize {
    let mut off = 0;
    for (i, l) in src.split_inclusive('\n').enumerate() {
        if i + 1 == line as usize {
            return off + l.char_indices().nth(col.saturating_sub(1) as usize).map_or(l.len(), |(b, _)| b);
        }
        off += l.len();
    }
    src.len()
}

impl std::fmt::Display for Report {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.human())
    }
}

impl From<Report> for String {
    fn from(r: Report) -> String {
        r.human()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_errors_keep_code_and_location() {
        let r = Report::from_text(&C::BUILD, "error[E0901]: missing `project` block\n  --> /nonexistent/mim.mim:3:5\n");
        assert_eq!(r.entries[0].diag.code.id, "E0901");
        assert_eq!(r.entries[0].diag.span.line, 3);
        let r = Report::from_text(&C::BUILD, "linking failed:\nld: library not found for -lfoo");
        assert_eq!(r.entries[0].diag.code.id, "E0905");
        assert!(r.entries[0].diag.notes[0].contains("-lfoo"));
    }

    #[test]
    fn offsets() {
        assert_eq!(offset_of("ab\ncd", 2, 2), 4);
        assert_eq!(offset_of("⍳x\n", 1, 2), "⍳".len());
    }
}
