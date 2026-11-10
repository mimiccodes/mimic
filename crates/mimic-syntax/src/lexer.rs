//! Lexer. Glyphs and words lex to the same tokens, so the parser never sees
//! the difference between `⍳ 10 ▷ +/` and `iota 10 |> sum`.

use crate::glyphs::{self, Prim};
use crate::{Diag, Span};

#[derive(Clone, Debug, PartialEq)]
pub enum Tok {
    Ident(String),
    Int(i64),
    Float(f64),
    Str(String),
    Prim(Prim),
    // keywords
    Module,
    Fn,
    Let,
    Mut,
    Pub,
    If,
    Else,
    Return,
    Use,
    As,
    Extern,
    Unsafe,
    Trusted,
    True,
    False,
    // punctuation
    LParen,
    RParen,
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    Comma,
    Colon,
    PathSep,
    Arrow,
    FatArrow,
    Assign,   // =
    Reassign, // ↩
    Bind,     // ←
    Semi,
    Newline,
    Pipe,
    // operators
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    IntDiv,
    EqEq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
    Not,
    Eof,
}

impl Tok {
    pub fn describe(&self) -> String {
        match self {
            Tok::Ident(s) => format!("identifier `{s}`"),
            Tok::Int(i) => format!("integer `{i}`"),
            Tok::Float(f) => format!("number `{f}`"),
            Tok::Str(_) => "string".into(),
            Tok::Prim(p) => format!("primitive `{}` ({})", p.info().glyph, p.info().word),
            Tok::Newline => "end of line".into(),
            Tok::Eof => "end of file".into(),
            other => format!("`{}`", tok_text(other)),
        }
    }
}

fn tok_text(t: &Tok) -> &'static str {
    match t {
        Tok::Module => "module",
        Tok::Fn => "fn",
        Tok::Let => "let",
        Tok::Mut => "mut",
        Tok::Pub => "pub",
        Tok::If => "if",
        Tok::Else => "else",
        Tok::Return => "return",
        Tok::Use => "use",
        Tok::As => "as",
        Tok::Extern => "extern",
        Tok::Unsafe => "unsafe",
        Tok::Trusted => "trusted",
        Tok::True => "true",
        Tok::False => "false",
        Tok::LParen => "(",
        Tok::RParen => ")",
        Tok::LBrace => "{",
        Tok::RBrace => "}",
        Tok::LBracket => "[",
        Tok::RBracket => "]",
        Tok::Comma => ",",
        Tok::Colon => ":",
        Tok::PathSep => "::",
        Tok::Arrow => "->",
        Tok::FatArrow => "=>",
        Tok::Assign => "=",
        Tok::Reassign => "↩",
        Tok::Bind => "←",
        Tok::Semi => ";",
        Tok::Pipe => "▷",
        Tok::Plus => "+",
        Tok::Minus => "-",
        Tok::Star => "×",
        Tok::Slash => "÷",
        Tok::Percent => "%",
        Tok::IntDiv => "div",
        Tok::EqEq => "==",
        Tok::Ne => "≠",
        Tok::Lt => "<",
        Tok::Le => "≤",
        Tok::Gt => ">",
        Tok::Ge => "≥",
        Tok::And => "and",
        Tok::Or => "or",
        Tok::Not => "not",
        _ => "?",
    }
}

#[derive(Clone, Debug)]
pub struct Token {
    pub tok: Tok,
    pub span: Span,
    /// Raw source text of the token (used by the config parser for keys).
    pub text: String,
    /// Whether whitespace precedes this token. `f(x)` is a call, `f (x)` is not.
    pub space_before: bool,
}

pub fn lex(src: &str) -> Result<Vec<Token>, Diag> {
    Lexer { src, pos: 0, line: 1, col: 1, out: Vec::new() }.run()
}

struct Lexer<'a> {
    src: &'a str,
    pos: usize,
    line: u32,
    col: u32,
    out: Vec<Token>,
}

fn keyword(w: &str) -> Option<Tok> {
    Some(match w {
        "module" => Tok::Module,
        "fn" => Tok::Fn,
        "let" => Tok::Let,
        "mut" => Tok::Mut,
        "pub" => Tok::Pub,
        "if" => Tok::If,
        "else" => Tok::Else,
        "return" => Tok::Return,
        "use" => Tok::Use,
        "as" => Tok::As,
        "extern" => Tok::Extern,
        "unsafe" => Tok::Unsafe,
        "trusted" => Tok::Trusted,
        "true" => Tok::True,
        "false" => Tok::False,
        "and" => Tok::And,
        "or" => Tok::Or,
        "not" => Tok::Not,
        "div" => Tok::IntDiv,
        _ => return None,
    })
}

impl<'a> Lexer<'a> {
    fn peek(&self) -> Option<char> {
        self.src[self.pos..].chars().next()
    }
    fn peek2(&self) -> Option<char> {
        self.src[self.pos..].chars().nth(1)
    }
    fn bump(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.pos += c.len_utf8();
        if c == '\n' {
            self.line += 1;
            self.col = 1;
        } else {
            self.col += 1;
        }
        Some(c)
    }
    fn here(&self) -> Span {
        Span { start: self.pos, end: self.pos, line: self.line, col: self.col }
    }

    fn run(mut self) -> Result<Vec<Token>, Diag> {
        let mut space = true;
        loop {
            // whitespace and comments
            match self.peek() {
                None => break,
                Some(' ' | '\t' | '\r') => {
                    self.bump();
                    space = true;
                    continue;
                }
                Some('/') if self.peek2() == Some('/') => {
                    self.skip_line();
                    space = true;
                    continue;
                }
                Some('⍝') => {
                    self.skip_line();
                    space = true;
                    continue;
                }
                _ => {}
            }
            let start = self.here();
            let tok = self.token(start)?;
            let mut span = start;
            span.end = self.pos;
            let text = self.src[start.start..self.pos].to_string();
            // Collapse runs of newlines into one token.
            if tok == Tok::Newline && matches!(self.out.last(), Some(t) if t.tok == Tok::Newline) {
                space = true;
                continue;
            }
            let is_nl = tok == Tok::Newline;
            self.out.push(Token { tok, span, text, space_before: space });
            space = is_nl;
        }
        let end = self.here();
        self.out.push(Token { tok: Tok::Eof, span: end, text: String::new(), space_before: true });
        Ok(self.out)
    }

    fn skip_line(&mut self) {
        while let Some(c) = self.peek() {
            if c == '\n' {
                break;
            }
            self.bump();
        }
    }

    fn token(&mut self, start: Span) -> Result<Tok, Diag> {
        let c = self.bump().unwrap();
        let next = self.peek();
        let t = match c {
            '\n' => Tok::Newline,
            '(' => Tok::LParen,
            ')' => Tok::RParen,
            '{' => Tok::LBrace,
            '}' => Tok::RBrace,
            '[' => Tok::LBracket,
            ']' => Tok::RBracket,
            ',' => Tok::Comma,
            ';' => Tok::Semi,
            ':' if next == Some(':') => {
                self.bump();
                Tok::PathSep
            }
            ':' => Tok::Colon,
            '+' if next == Some('/') => {
                self.bump();
                Tok::Prim(Prim::Sum)
            }
            '+' => Tok::Plus,
            '-' if next == Some('>') => {
                self.bump();
                Tok::Arrow
            }
            '-' => Tok::Minus,
            '*' | '×' if next == Some('/') => {
                self.bump();
                Tok::Prim(Prim::Product)
            }
            '*' | '×' => Tok::Star,
            '/' | '÷' => Tok::Slash,
            '%' => Tok::Percent,
            '⫽' => Tok::IntDiv,
            '=' if next == Some('=') => {
                self.bump();
                Tok::EqEq
            }
            '=' if next == Some('>') => {
                self.bump();
                Tok::FatArrow
            }
            '=' => Tok::Assign,
            '!' if next == Some('=') => {
                self.bump();
                Tok::Ne
            }
            '<' if next == Some('=') => {
                self.bump();
                Tok::Le
            }
            '<' => Tok::Lt,
            '>' if next == Some('=') => {
                self.bump();
                Tok::Ge
            }
            '>' => Tok::Gt,
            '|' if next == Some('>') => {
                self.bump();
                Tok::Pipe
            }
            '≠' => Tok::Ne,
            '≤' => Tok::Le,
            '≥' => Tok::Ge,
            '∧' => Tok::And,
            '∨' => Tok::Or,
            '¬' => Tok::Not,
            '▷' => Tok::Pipe,
            '→' => Tok::Arrow,
            '⇒' => Tok::FatArrow,
            '←' => Tok::Bind,
            '↩' => Tok::Reassign,
            '§' => Tok::Module,
            'ƒ' => Tok::Fn,
            '⌈' | '⌊' => {
                if self.peek() == Some('/') {
                    self.bump();
                    Tok::Prim(if c == '⌈' { Prim::Maximum } else { Prim::Minimum })
                } else {
                    Tok::Prim(if c == '⌈' { Prim::Max } else { Prim::Min })
                }
            }
            '"' => self.string(start)?,
            '\\' => {
                let d = self.bump().ok_or_else(|| Diag::new("dangling `\\`", start))?;
                let key = format!("\\{d}");
                let g = glyphs::digraph(&key)
                    .ok_or_else(|| Diag::new(format!("unknown digraph `{key}` (run `mimic glyphs`)"), start))?;
                // Re-lex the glyph it stands for. ⌈/ and ⌊/ digraphs already include the slash.
                return Ok(match g {
                    "⌈/" => Tok::Prim(Prim::Maximum),
                    "⌊/" => Tok::Prim(Prim::Minimum),
                    _ => {
                        let sub = lex(g).map_err(|_| Diag::new("bad digraph", start))?;
                        sub[0].tok.clone()
                    }
                });
            }
            '¯' => {
                if self.peek().is_some_and(|c| c.is_ascii_digit()) {
                    return self.number(start, true);
                }
                return Err(Diag::new("`¯` (high minus) must be followed by a number", start));
            }
            c if c.is_ascii_digit() => {
                self.pos = start.start;
                self.line = start.line;
                self.col = start.col;
                return self.number(start, false);
            }
            c if c.is_alphabetic() || c == '_' => {
                while let Some(c) = self.peek() {
                    if c.is_alphanumeric() || c == '_' {
                        self.bump();
                    } else {
                        break;
                    }
                }
                let w = &self.src[start.start..self.pos];
                if let Some(k) = keyword(w) {
                    k
                } else if let Some(p) = Prim::from_word(w) {
                    Tok::Prim(p)
                } else {
                    Tok::Ident(w.to_string())
                }
            }
            other => {
                if let Some(p) = glyphs::PRIMS.iter().find(|p| p.glyph.chars().count() == 1 && p.glyph.starts_with(other)) {
                    Tok::Prim(p.prim)
                } else {
                    return Err(Diag::new(format!("unexpected character `{other}`"), start));
                }
            }
        };
        Ok(t)
    }

    fn number(&mut self, start: Span, negative: bool) -> Result<Tok, Diag> {
        let digits_start = self.pos;
        let mut is_float = false;
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() || c == '_' {
                self.bump();
            } else if c == '.' && !is_float && self.peek2().is_some_and(|d| d.is_ascii_digit()) {
                is_float = true;
                self.bump();
            } else if (c == 'e' || c == 'E')
                && (self.peek2().is_some_and(|d| d.is_ascii_digit() || d == '-' || d == '+'))
            {
                is_float = true;
                self.bump();
                if matches!(self.peek(), Some('-' | '+')) {
                    self.bump();
                }
            } else {
                break;
            }
        }
        let text: String = self.src[digits_start..self.pos].chars().filter(|c| *c != '_').collect();
        let mut span = start;
        span.end = self.pos;
        if is_float {
            let v: f64 = text.parse().map_err(|_| Diag::new("invalid float literal", span))?;
            Ok(Tok::Float(if negative { -v } else { v }))
        } else {
            let v: i64 = text.parse().map_err(|_| Diag::new("integer literal out of range for i64", span))?;
            Ok(Tok::Int(if negative { -v } else { v }))
        }
    }

    fn string(&mut self, start: Span) -> Result<Tok, Diag> {
        let mut s = String::new();
        loop {
            match self.bump() {
                None | Some('\n') => return Err(Diag::new("unterminated string literal", start)),
                Some('"') => break,
                Some('\\') => {
                    let e = self.bump().ok_or_else(|| Diag::new("unterminated string literal", start))?;
                    s.push(match e {
                        'n' => '\n',
                        't' => '\t',
                        '0' => '\0',
                        '\\' => '\\',
                        '"' => '"',
                        other => return Err(Diag::new(format!("unknown escape `\\{other}`"), self.here())),
                    });
                }
                Some(c) => s.push(c),
            }
        }
        Ok(Tok::Str(s))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toks(s: &str) -> Vec<Tok> {
        lex(s).unwrap().into_iter().map(|t| t.tok).collect()
    }

    #[test]
    fn glyphs_and_words_lex_identically() {
        assert_eq!(toks("⍳ 10 ▷ ⌽ ▷ +/"), toks("iota 10 |> reverse |> sum"));
        assert_eq!(toks("xs ↑ 3 ¨ f"), toks("xs take 3 each f"));
        assert_eq!(toks("a × b ÷ c ≠ d"), toks("a * b / c != d"));
        assert_eq!(toks("§ main"), toks("module main"));
        assert_eq!(toks("\\i 3 \\> \\o"), toks("⍳ 3 ▷ ⌽"));
        assert_eq!(toks("xs ×/"), toks("xs product"));
        assert_eq!(toks("a ⫽ b ⌈ c ⌊ d ‥ e ∣"), toks("a div b max c min d range e abs"));
    }

    #[test]
    fn numbers() {
        assert_eq!(toks("¯3 2.5 1_000 1e3"), vec![Tok::Int(-3), Tok::Float(2.5), Tok::Int(1000), Tok::Float(1000.0), Tok::Eof]);
    }

    #[test]
    fn comments_and_newlines() {
        assert_eq!(toks("a // c\n\n⍝ x\nb"), vec![Tok::Ident("a".into()), Tok::Newline, Tok::Ident("b".into()), Tok::Eof]);
    }
}
