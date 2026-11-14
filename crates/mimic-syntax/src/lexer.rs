//! Lexer. Glyphs and words lex to the same tokens, so the parser never sees
//! the difference between `⍳ 10 ▷ +/` and `iota 10 |> sum`.

use crate::glyphs::{self, Prim};
use crate::codes as C;
use crate::{Diag, Span};

/// A piece of an interpolated string literal: text, or the source of an `{expression}`.
#[derive(Clone, Debug, PartialEq)]
pub enum StrPart {
    Text(String),
    Code(String),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Tok {
    /// a string literal containing `{…}` expressions
    Interp(Vec<StrPart>),
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
    Type,
    Match,
    /// `arms` / `⋔`: parallel divide-and-conquer recursion
    Arms,
    With,
    Dot,
    Bar,
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
    Amp,
    Caret,
    Tilde,
    Shl,
    Shr,
    UShr,
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
        Tok::Type => "type",
        Tok::Match => "match",
        Tok::Arms => "arms",
        Tok::With => "with",
        Tok::Dot => ".",
        Tok::Bar => "|",
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
        Tok::Amp => "&",
        Tok::Caret => "^",
        Tok::Tilde => "~",
        Tok::Shl => "<<",
        Tok::Shr => ">>",
        Tok::UShr => ">>>",
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

/// Does `w` read as a plain name? Keywords (`div`, `match`, `type`, …) and
/// the word forms of glyphs (`sum`, `take`, …) do not: names coming from
/// elsewhere (C headers) that are one of those need another spelling.
pub fn is_plain_name(w: &str) -> bool {
    matches!(lex(w).as_deref(), Ok([Token { tok: Tok::Ident(n), .. }, Token { tok: Tok::Eof, .. }]) if n == w)
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
        "type" => Tok::Type,
        "match" => Tok::Match,
        "arms" => Tok::Arms,
        "with" => Tok::With,
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
            '<' if next == Some('<') => {
                self.bump();
                Tok::Shl
            }
            '>' if next == Some('>') => {
                self.bump();
                if self.peek() == Some('>') {
                    self.bump();
                    Tok::UShr
                } else {
                    Tok::Shr
                }
            }
            '&' => Tok::Amp,
            '^' => Tok::Caret,
            '~' => Tok::Tilde,
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
            '|' => Tok::Bar,
            '.' => Tok::Dot,
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
            // 'raw text': no interpolation, no escapes (JSON, regexes, Windows paths)
            '\'' => {
                let mut s = String::new();
                loop {
                    match self.bump() {
                        None | Some('\n') => return Err(Diag::new(&C::UNTERMINATED_STRING, "unterminated raw string ('…')", start)),
                        Some('\'') => break,
                        Some(c) => s.push(c),
                    }
                }
                Tok::Str(s)
            }
            '\\' => {
                let d = self.bump().ok_or_else(|| Diag::new(&C::UNKNOWN_DIGRAPH, "dangling `\\`", start))?;
                let key = format!("\\{d}");
                let g = glyphs::digraph(&key)
                    .ok_or_else(|| Diag::new(&C::UNKNOWN_DIGRAPH, format!("unknown digraph `{key}` (run `mimic glyphs`)"), start))?;
                // Re-lex the glyph it stands for. ⌈/ and ⌊/ digraphs already include the slash.
                return Ok(match g {
                    "⌈/" => Tok::Prim(Prim::Maximum),
                    "⌊/" => Tok::Prim(Prim::Minimum),
                    _ => {
                        let sub = lex(g).map_err(|_| Diag::new(&C::UNKNOWN_DIGRAPH, "bad digraph", start))?;
                        sub[0].tok.clone()
                    }
                });
            }
            '¯' => {
                if self.peek().is_some_and(|c| c.is_ascii_digit()) {
                    return self.number(start, true);
                }
                return Err(Diag::new(&C::BAD_NUMBER, "`¯` (high minus) must be followed by a number", start));
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
            '⋔' => Tok::Arms,
            other => {
                if let Some(p) = glyphs::PRIMS.iter().find(|p| p.glyph.chars().count() == 1 && p.glyph.starts_with(other)) {
                    Tok::Prim(p.prim)
                } else {
                    return Err(Diag::new(&C::UNEXPECTED_CHAR, format!("unexpected character `{other}`"), start));
                }
            }
        };
        Ok(t)
    }

    fn number(&mut self, start: Span, negative: bool) -> Result<Tok, Diag> {
        // 0xFF, 0b1010, 0o17 (underscores allowed); the full 64-bit pattern is
        // accepted, so 0xFFFF_FFFF_FFFF_FFFF is -1
        if self.peek() == Some('0')
            && let Some(radix) = match self.peek2() {
                Some('x' | 'X') => Some(16),
                Some('b' | 'B') => Some(2),
                Some('o' | 'O') => Some(8),
                _ => None,
            }
        {
            self.bump();
            self.bump();
            let ds = self.pos;
            while self.peek().is_some_and(|c| c.is_ascii_alphanumeric() || c == '_') {
                self.bump();
            }
            let mut span = start;
            span.end = self.pos;
            let text: String = self.src[ds..self.pos].chars().filter(|c| *c != '_').collect();
            let v = u64::from_str_radix(&text, radix).map_err(|_| {
                let what = match radix {
                    16 => "hexadecimal",
                    2 => "binary",
                    _ => "octal",
                };
                Diag::new(&C::BAD_NUMBER, format!("invalid {what} literal (at most 64 bits)"), span)
            })? as i64;
            return Ok(Tok::Int(if negative { v.wrapping_neg() } else { v }));
        }
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
        // duration literals: 2s, 500ms, 1.5min, 10us, 3h → i64 nanoseconds
        let rest = &self.src[self.pos..];
        let unit_len = rest.chars().take_while(|c| c.is_ascii_alphabetic()).count();
        let unit = &rest[..unit_len];
        let scale: Option<f64> = match unit {
            "ns" => Some(1.0),
            "us" => Some(1e3),
            "ms" => Some(1e6),
            "s" => Some(1e9),
            "min" => Some(60e9),
            "h" => Some(3600e9),
            _ => None,
        };
        if let Some(sc) = scale {
            for _ in 0..unit_len {
                self.bump();
            }
            let mut span = start;
            span.end = self.pos;
            let v: f64 = text.parse().map_err(|_| Diag::new(&C::BAD_NUMBER, "invalid duration", span))?;
            let ns = (v * sc).round();
            return Ok(Tok::Int((if negative { -ns } else { ns }) as i64));
        }
        let mut span = start;
        span.end = self.pos;
        if is_float {
            let v: f64 = text.parse().map_err(|_| Diag::new(&C::BAD_NUMBER, "invalid float literal", span))?;
            Ok(Tok::Float(if negative { -v } else { v }))
        } else {
            let v: i64 = text.parse().map_err(|_| Diag::new(&C::BAD_NUMBER, "integer literal out of range for i64", span))?;
            Ok(Tok::Int(if negative { -v } else { v }))
        }
    }

    fn string(&mut self, start: Span) -> Result<Tok, Diag> {
        let mut s = String::new();
        let mut parts: Vec<StrPart> = Vec::new();
        loop {
            match self.bump() {
                None | Some('\n') => return Err(Diag::new(&C::UNTERMINATED_STRING, "unterminated string literal", start)),
                Some('"') => break,
                Some('{') => {
                    // `{expression}` inside a string: capture its source text
                    if !s.is_empty() {
                        parts.push(StrPart::Text(std::mem::take(&mut s)));
                    }
                    let mut depth = 1;
                    let mut code = String::new();
                    loop {
                        match self.bump() {
                            None | Some('\n') => return Err(Diag::new(&C::UNTERMINATED_STRING, "unterminated `{` in string literal", start)),
                            Some('{') => {
                                depth += 1;
                                code.push('{');
                            }
                            Some('}') => {
                                depth -= 1;
                                if depth == 0 {
                                    break;
                                }
                                code.push('}');
                            }
                            Some(c) => code.push(c),
                        }
                    }
                    parts.push(StrPart::Code(code));
                }
                Some('\\') => {
                    let e = self.bump().ok_or_else(|| Diag::new(&C::UNTERMINATED_STRING, "unterminated string literal", start))?;
                    s.push(match e {
                        '{' => '{',
                        '}' => '}',
                        'n' => '\n',
                        't' => '\t',
                        'r' => '\r',
                        // `\0` alone is NUL; `\033` (C's octal) would silently mean NUL then "33"
                        '0' if self.peek().is_some_and(|c| c.is_ascii_digit()) => {
                            return Err(Diag::new(&C::UNKNOWN_ESCAPE, "octal escapes do not exist: write `\\x1b` (two hex digits) or `\\u{1b}`", self.here()));
                        }
                        '0' => '\0',
                        '\\' => '\\',
                        '"' => '"',
                        // `\x1b`: a character by two hex digits (ASCII, 00 to 7f)
                        'x' => {
                            let mut v = 0u32;
                            for _ in 0..2 {
                                let d = self.bump().and_then(|c| c.to_digit(16)).ok_or_else(|| {
                                    Diag::new(&C::UNKNOWN_ESCAPE, "`\\x` takes two hex digits, as in `\\x1b`", self.here())
                                })?;
                                v = v * 16 + d;
                            }
                            if v > 0x7f {
                                return Err(Diag::new(&C::UNKNOWN_ESCAPE, format!("`\\x{v:02x}` is not ASCII: write the character as `\\u{{{v:x}}}`"), self.here()));
                            }
                            char::from_u32(v).unwrap()
                        }
                        // `\u{1F600}`: any Unicode character by its code point
                        'u' => {
                            if self.bump() != Some('{') {
                                return Err(Diag::new(&C::UNKNOWN_ESCAPE, "`\\u` takes a code point in braces, as in `\\u{1F600}`", self.here()));
                            }
                            let mut v = 0u32;
                            let mut n = 0;
                            loop {
                                match self.bump() {
                                    Some('}') if n > 0 => break,
                                    Some(c) if c.is_ascii_hexdigit() && n < 6 => {
                                        v = v * 16 + c.to_digit(16).unwrap();
                                        n += 1;
                                    }
                                    _ => return Err(Diag::new(&C::UNKNOWN_ESCAPE, "`\\u{…}` takes 1 to 6 hex digits, as in `\\u{1F600}`", self.here())),
                                }
                            }
                            char::from_u32(v).ok_or_else(|| Diag::new(&C::UNKNOWN_ESCAPE, format!("`\\u{{{v:x}}}` is not a Unicode character"), self.here()))?
                        }
                        other => return Err(Diag::new(&C::UNKNOWN_ESCAPE, format!("unknown escape `\\{other}` (escapes: \\n \\t \\r \\0 \\\\ \\\" \\{{ \\}} \\x1b \\u{{1F600}})"), self.here())),
                    });
                }
                Some(c) => s.push(c),
            }
        }
        if parts.is_empty() {
            return Ok(Tok::Str(s));
        }
        if !s.is_empty() {
            parts.push(StrPart::Text(s));
        }
        Ok(Tok::Interp(parts))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toks(s: &str) -> Vec<Tok> {
        lex(s).unwrap().into_iter().map(|t| t.tok).collect()
    }

    #[test]
    fn plain_names() {
        assert!(is_plain_name("printf") && is_plain_name("abs_2"));
        for w in ["div", "match", "type", "with", "sum", "take", "not"] {
            assert!(!is_plain_name(w), "{w}");
        }
    }

    #[test]
    fn string_escapes() {
        assert_eq!(toks(r#""\x1b[0m""#), vec![Tok::Str("\u{1b}[0m".into()), Tok::Eof]);
        assert_eq!(toks(r#""\u{48}\u{1F600}\x41\0""#), vec![Tok::Str("H\u{1F600}A\0".into()), Tok::Eof]);
        // never silently misread: C's octal, short or non-ASCII \x, bad \u
        for bad in [r#""\033""#, r#""\x9""#, r#""\xff""#, r#""\u{110000}""#, r#""\u41""#, r#""\u{}""#, r#""\q""#] {
            assert!(lex(bad).is_err(), "{bad} should not lex");
        }
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
        assert_eq!(toks("2s 500ms 1.5min"), vec![Tok::Int(2_000_000_000), Tok::Int(500_000_000), Tok::Int(90_000_000_000), Tok::Eof]);
        assert_eq!(toks("0xFF 0b1010 0o17 0xFFFF_FFFF_FFFF_FFFF ¯0x10"), vec![Tok::Int(255), Tok::Int(10), Tok::Int(15), Tok::Int(-1), Tok::Int(-16), Tok::Eof]);
        assert!(lex("0x1_0000_0000_0000_0000").is_err());
        assert!(lex("0b102").is_err());
    }

    #[test]
    fn raw_strings() {
        assert_eq!(toks(r#"'{"a": "\n"}'"#), vec![Tok::Str(r#"{"a": "\n"}"#.into()), Tok::Eof]);
        assert!(lex("'open").is_err());
    }

    #[test]
    fn bitwise_operators() {
        assert_eq!(
            toks("a & b | c ^ ~d << e >> f >>> g"),
            vec![
                Tok::Ident("a".into()), Tok::Amp, Tok::Ident("b".into()), Tok::Bar, Tok::Ident("c".into()), Tok::Caret,
                Tok::Tilde, Tok::Ident("d".into()), Tok::Shl, Tok::Ident("e".into()), Tok::Shr, Tok::Ident("f".into()),
                Tok::UShr, Tok::Ident("g".into()), Tok::Eof
            ]
        );
        assert_eq!(toks("a <= b >= c"), vec![Tok::Ident("a".into()), Tok::Le, Tok::Ident("b".into()), Tok::Ge, Tok::Ident("c".into()), Tok::Eof]);
    }

    #[test]
    fn comments_and_newlines() {
        assert_eq!(toks("a // c\n\n⍝ x\nb"), vec![Tok::Ident("a".into()), Tok::Newline, Tok::Ident("b".into()), Tok::Eof]);
    }
}
