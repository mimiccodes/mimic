//! Mimic front end: lexer, parser, AST, glyph table and the `.mim` config format.

pub mod ast;
pub mod config;
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

/// A compiler diagnostic attached to a span of one source file.
#[derive(Clone, Debug)]
pub struct Diag {
    pub msg: String,
    pub span: Span,
    pub notes: Vec<String>,
}

impl Diag {
    pub fn new(msg: impl Into<String>, span: Span) -> Self {
        Diag { msg: msg.into(), span, notes: Vec::new() }
    }
    pub fn note(mut self, n: impl Into<String>) -> Self {
        self.notes.push(n.into());
        self
    }

    /// Render rustc-style with a source excerpt and caret underline.
    pub fn render(&self, path: &Path, src: &str) -> String {
        let mut out = String::new();
        let _ = writeln!(out, "error: {}", self.msg);
        let _ = writeln!(out, "  --> {}:{}:{}", path.display(), self.span.line, self.span.col);
        if let Some(line) = src.lines().nth(self.span.line.saturating_sub(1) as usize) {
            let ln = self.span.line.to_string();
            let pad = " ".repeat(ln.len());
            let _ = writeln!(out, "{pad} |");
            let _ = writeln!(out, "{ln} | {line}");
            let col = self.span.col.saturating_sub(1) as usize;
            let prefix: String = line.chars().take(col).map(|c| if c == '\t' { '\t' } else { ' ' }).collect();
            let width = src
                .get(self.span.start..self.span.end)
                .map(|s| s.lines().next().unwrap_or("").chars().count())
                .unwrap_or(1)
                .max(1);
            let _ = writeln!(out, "{pad} | {prefix}{}", "^".repeat(width));
        }
        for n in &self.notes {
            let _ = writeln!(out, "  = note: {n}");
        }
        out
    }
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
