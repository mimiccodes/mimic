//! Mimic front end: lexer, parser, AST, glyph table and the `.mim` config format.

// Diagnostics are rich (labels, fixes) and only built on the error path.
#![allow(clippy::result_large_err)]

pub mod ast;
pub mod codes;
pub mod config;
pub mod desugar;
pub mod format;
pub mod glyphs;
pub mod lexer;
pub mod parser;

use std::fmt::Write as _;
use std::path::Path;

/// Byte range plus 1-based line/column of the start.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub line: u32,
    pub col: u32,
}

impl Span {
    pub fn to(self, other: Span) -> Span {
        Span { start: self.start, end: other.end.max(self.end), line: self.line, col: self.col }
    }
}

pub use codes::Code;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

impl Severity {
    pub fn as_str(self) -> &'static str {
        match self {
            Severity::Error => "error",
            Severity::Warning => "warning",
        }
    }
}

/// A secondary location with a short message ("declared here").
/// `file` is an index into the caller's file table; `None` means the
/// diagnostic's own file.
#[derive(Clone, Debug)]
pub struct Label {
    pub file: Option<usize>,
    pub span: Span,
    pub msg: String,
}

/// One text edit: replace the bytes of `span` with `text` (an empty span inserts).
#[derive(Clone, Debug)]
pub struct Edit {
    pub file: Option<usize>,
    pub span: Span,
    pub text: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Applicability {
    /// Applying the edits produces the intended code.
    MachineApplicable,
    /// Probably right, but a human (or agent) should confirm.
    MaybeIncorrect,
    /// The edits contain `…` placeholders that must be filled in.
    HasPlaceholders,
}

impl Applicability {
    pub fn as_str(self) -> &'static str {
        match self {
            Applicability::MachineApplicable => "machine-applicable",
            Applicability::MaybeIncorrect => "maybe-incorrect",
            Applicability::HasPlaceholders => "has-placeholders",
        }
    }
}

/// A suggested fix: a set of edits applied together.
#[derive(Clone, Debug)]
pub struct Fix {
    pub msg: String,
    pub edits: Vec<Edit>,
    pub applicability: Applicability,
}

/// A compiler diagnostic attached to a span of one source file.
#[derive(Clone, Debug)]
pub struct Diag {
    pub code: &'static Code,
    pub severity: Severity,
    pub msg: String,
    pub span: Span,
    pub labels: Vec<Label>,
    pub notes: Vec<String>,
    pub help: Vec<String>,
    pub fixes: Vec<Fix>,
}

/// A source file as seen by the renderer.
#[derive(Clone, Copy)]
pub struct SrcRef<'a> {
    pub display: &'a str,
    pub src: &'a str,
}

impl Diag {
    pub fn new(code: &'static Code, msg: impl Into<String>, span: Span) -> Self {
        Diag {
            code,
            severity: Severity::Error,
            msg: msg.into(),
            span,
            labels: Vec::new(),
            notes: Vec::new(),
            help: Vec::new(),
            fixes: Vec::new(),
        }
    }
    pub fn note(mut self, n: impl Into<String>) -> Self {
        self.notes.push(n.into());
        self
    }
    pub fn help(mut self, h: impl Into<String>) -> Self {
        self.help.push(h.into());
        self
    }
    /// A secondary label in the same file.
    pub fn label(mut self, span: Span, msg: impl Into<String>) -> Self {
        self.labels.push(Label { file: None, span, msg: msg.into() });
        self
    }
    /// A secondary label in another file of the program.
    pub fn label_in(mut self, file: usize, span: Span, msg: impl Into<String>) -> Self {
        self.labels.push(Label { file: Some(file), span, msg: msg.into() });
        self
    }
    /// A single-edit fix in the same file.
    pub fn fix(self, msg: impl Into<String>, span: Span, text: impl Into<String>, a: Applicability) -> Self {
        self.fix_edits(msg, vec![Edit { file: None, span, text: text.into() }], a)
    }
    pub fn fix_edits(mut self, msg: impl Into<String>, edits: Vec<Edit>, applicability: Applicability) -> Self {
        self.fixes.push(Fix { msg: msg.into(), edits, applicability });
        self
    }
    /// Append "did you mean `x`?" with a fix replacing `span`, if a close
    /// candidate exists.
    pub fn did_you_mean<'a>(self, wrong: &str, span: Span, candidates: impl IntoIterator<Item = &'a str>) -> Self {
        match closest(wrong, candidates) {
            Some(best) => self.fix(format!("did you mean `{best}`?"), span, best, Applicability::MaybeIncorrect),
            None => self,
        }
    }

    /// Render a diagnostic whose file is the only one involved.
    pub fn render(&self, path: &Path, src: &str) -> String {
        let shown = path.display().to_string();
        self.render_in(0, &[SrcRef { display: &shown, src }])
    }

    /// Render rustc-style with source excerpts, labels, notes and fixes.
    /// `file` indexes `files`; labels and edits may point at other entries.
    pub fn render_in(&self, file: usize, files: &[SrcRef]) -> String {
        let mut out = String::new();
        let _ = writeln!(out, "{}[{}]: {}", self.severity.as_str(), self.code.id, self.msg);
        let cur = files.get(file).copied();
        if let Some(f) = cur {
            let _ = writeln!(out, "  --> {}:{}:{}", f.display, self.span.line, self.span.col);
            excerpt(&mut out, f.src, self.span, '^', "");
        }
        for l in &self.labels {
            let Some(f) = l.file.map_or(cur, |i| files.get(i).copied()) else { continue };
            if l.file.is_some_and(|i| i != file) {
                let _ = writeln!(out, "  ::: {}:{}:{}", f.display, l.span.line, l.span.col);
            }
            excerpt(&mut out, f.src, l.span, '-', &l.msg);
        }
        for n in &self.notes {
            let _ = writeln!(out, "  = note: {n}");
        }
        for h in &self.help {
            let _ = writeln!(out, "  = help: {h}");
        }
        for fx in &self.fixes {
            match fx.edits.as_slice() {
                [e] if !e.text.contains('\n') => {
                    let same = e.file.is_none_or(|i| i == file);
                    let place = if same { cur } else { e.file.and_then(|i| files.get(i).copied()) };
                    let at = match place {
                        Some(f) if !same => format!(" at {}:{}:{}", f.display, e.span.line, e.span.col),
                        Some(_) => format!(" at {}:{}", e.span.line, e.span.col),
                        None => String::new(),
                    };
                    let old = place.and_then(|f| f.src.get(e.span.start..e.span.end)).unwrap_or("");
                    let sep = if fx.msg.ends_with('?') { " " } else { ": " };
                    if old.is_empty() {
                        let _ = writeln!(out, "  = fix: {}{sep}insert `{}`{at}", fx.msg, e.text);
                    } else {
                        let _ = writeln!(out, "  = fix: {}{sep}replace `{old}` with `{}`{at}", fx.msg, e.text);
                    }
                }
                _ => {
                    let _ = writeln!(out, "  = fix: {}", fx.msg);
                }
            }
        }
        out
    }
}

/// The source line under `span` with an underline of `mark`s and a message.
fn excerpt(out: &mut String, src: &str, span: Span, mark: char, msg: &str) {
    let Some(line) = src.lines().nth(span.line.saturating_sub(1) as usize) else { return };
    let ln = span.line.to_string();
    let pad = " ".repeat(ln.len().max(2));
    let lnp = format!("{ln:>w$}", w = pad.len());
    let _ = writeln!(out, "{pad} |");
    let _ = writeln!(out, "{lnp} | {line}");
    let col = span.col.saturating_sub(1) as usize;
    let prefix: String = line.chars().take(col).map(|c| if c == '\t' { '\t' } else { ' ' }).collect();
    let width = src
        .get(span.start..span.end)
        .map(|s| s.lines().next().unwrap_or("").chars().count())
        .unwrap_or(1)
        .max(1);
    let marks: String = std::iter::repeat_n(mark, width).collect();
    let sep = if msg.is_empty() { "" } else { " " };
    let _ = writeln!(out, "{pad} | {prefix}{marks}{sep}{msg}");
}

/// 1-based line and column (in characters) of byte offset `at`.
pub fn line_col(src: &str, at: usize) -> (u32, u32) {
    let at = at.min(src.len());
    let before = &src[..src.floor_char_boundary(at)];
    let line = before.matches('\n').count() as u32 + 1;
    let col = before.rsplit('\n').next().unwrap_or("").chars().count() as u32 + 1;
    (line, col)
}

/// A span covering bytes `start..end` of `src`.
pub fn span_at(src: &str, start: usize, end: usize) -> Span {
    let (line, col) = line_col(src, start);
    Span { start, end, line, col }
}

/// Levenshtein distance (for "did you mean" suggestions).
pub fn edit_distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut cur = vec![i + 1];
        for (j, cb) in b.iter().enumerate() {
            let sub = prev[j] + usize::from(ca != *cb);
            cur.push(sub.min(prev[j + 1] + 1).min(cur[j] + 1));
        }
        prev = cur;
    }
    prev[b.len()]
}

/// The closest candidate to `wrong` within a third of its length (at least 1 edit).
pub fn closest<'a>(wrong: &str, candidates: impl IntoIterator<Item = &'a str>) -> Option<&'a str> {
    let limit = (wrong.chars().count() / 3).max(1);
    let lower = wrong.to_lowercase();
    candidates
        .into_iter()
        .filter(|c| *c != wrong)
        .map(|c| {
            // a case-only difference is the closest possible miss
            let d = if c.to_lowercase() == lower { 0 } else { edit_distance(wrong, c) };
            (d, c)
        })
        .filter(|(d, _)| *d <= limit)
        .min_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(b.1)))
        .map(|(_, c)| c)
}

/// Stable 64-bit FNV-1a hash (used for mod.mim signature hashes and cache keys).
pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}
