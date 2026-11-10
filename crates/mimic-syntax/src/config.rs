//! Declarative `.mim` config format used by `mim.mim`, `build.mim` and `mod.mim`.
//!
//! It shares the Mimic lexer, so config files look like Mimic code:
//!
//! ```text
//! module mim
//!
//! project {
//!     name = "demo"
//!     kind = [bin, lib]
//! }
//! profile release { opt = 3 }      // nested key: profile.release.opt
//! ```

use crate::lexer::{Tok, Token, lex};
use crate::codes as C;
use crate::{Diag, Span};
use std::fmt::Write as _;

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Str(String),
    Int(i64),
    Float(f64),
    Bool(bool),
    /// Bare word such as `bin` in `kind = [bin]`.
    Ident(String),
    List(Vec<Value>),
    Table(Table),
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Table {
    pub entries: Vec<(String, Value, Span)>,
}

impl Table {
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.entries.iter().find(|(k, _, _)| k == key).map(|(_, v, _)| v)
    }
    pub fn span_of(&self, key: &str) -> Option<Span> {
        self.entries.iter().find(|(k, _, _)| k == key).map(|(_, _, s)| *s)
    }
    pub fn table(&self, key: &str) -> Option<&Table> {
        match self.get(key) {
            Some(Value::Table(t)) => Some(t),
            _ => None,
        }
    }
    pub fn str(&self, key: &str) -> Option<&str> {
        match self.get(key) {
            Some(Value::Str(s)) | Some(Value::Ident(s)) => Some(s),
            _ => None,
        }
    }
    fn insert_path(&mut self, keys: &[String], v: Value, span: Span) -> Result<(), Diag> {
        if keys.len() == 1 {
            if self.get(&keys[0]).is_some() {
                return Err(Diag::new(&C::DUPLICATE_ENTRY, format!("duplicate key `{}`", keys[0]), span));
            }
            self.entries.push((keys[0].clone(), v, span));
            return Ok(());
        }
        if self.get(&keys[0]).is_none() {
            self.entries.push((keys[0].clone(), Value::Table(Table::default()), span));
        }
        match self.entries.iter_mut().find(|(k, _, _)| *k == keys[0]) {
            Some((_, Value::Table(t), _)) => t.insert_path(&keys[1..], v, span),
            _ => Err(Diag::new(&C::PROJECT, format!("`{}` is not a table", keys[0]), span)),
        }
    }
}

impl Value {
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) | Value::Ident(s) => Some(s),
            _ => None,
        }
    }
    pub fn as_list(&self) -> Option<&[Value]> {
        match self {
            Value::List(v) => Some(v),
            _ => None,
        }
    }
}

/// A parsed config file: its header name (`module mim`) plus the root table.
#[derive(Clone, Debug)]
pub struct ConfigFile {
    pub header: String,
    pub root: Table,
}

pub fn parse(src: &str) -> Result<ConfigFile, Diag> {
    let toks = lex(src)?;
    let mut p = CP { toks, pos: 0 };
    p.skip_nl();
    if !matches!(p.peek().tok, Tok::Module) {
        return Err(Diag::new(&C::MODULE_HEADER, "config files start with `module <name>`", p.peek().span));
    }
    p.pos += 1;
    let header = p.key()?.0;
    let root = p.entries(false)?;
    Ok(ConfigFile { header, root })
}

struct CP {
    toks: Vec<Token>,
    pos: usize,
}

impl CP {
    fn peek(&self) -> &Token {
        &self.toks[self.pos]
    }
    fn skip_nl(&mut self) {
        while matches!(self.peek().tok, Tok::Newline | Tok::Semi | Tok::Comma) {
            self.pos += 1;
        }
    }
    fn key(&mut self) -> Result<(String, Span), Diag> {
        let t = self.toks[self.pos].clone();
        let is_word = t.text.chars().next().is_some_and(|c| c.is_alphabetic() || c == '_')
            && t.text.chars().all(|c| c.is_alphanumeric() || c == '_');
        match t.tok {
            Tok::Str(s) => {
                self.pos += 1;
                Ok((s, t.span))
            }
            _ if is_word => {
                self.pos += 1;
                Ok((t.text, t.span))
            }
            other => Err(Diag::new(&C::UNEXPECTED_TOKEN, format!("expected a key, found {}", other.describe()), t.span)),
        }
    }
    fn entries(&mut self, nested: bool) -> Result<Table, Diag> {
        let mut table = Table::default();
        loop {
            self.skip_nl();
            match self.peek().tok {
                Tok::Eof if !nested => return Ok(table),
                Tok::RBrace if nested => return Ok(table),
                Tok::Eof => return Err(Diag::new(&C::UNEXPECTED_TOKEN, "expected `}`", self.peek().span)),
                _ => {}
            }
            let (k, span) = self.key()?;
            let mut keys = vec![k];
            loop {
                match self.peek().tok {
                    Tok::Assign => {
                        self.pos += 1;
                        let v = self.value()?;
                        table.insert_path(&keys, v, span)?;
                        break;
                    }
                    Tok::LBrace => {
                        let v = self.value()?;
                        table.insert_path(&keys, v, span)?;
                        break;
                    }
                    _ => keys.push(self.key()?.0),
                }
            }
        }
    }
    fn value(&mut self) -> Result<Value, Diag> {
        let t = self.toks[self.pos].clone();
        self.pos += 1;
        Ok(match t.tok {
            Tok::Str(s) => Value::Str(s),
            Tok::Int(i) => Value::Int(i),
            Tok::Float(f) => Value::Float(f),
            Tok::True => Value::Bool(true),
            Tok::False => Value::Bool(false),
            Tok::Minus => match self.value()? {
                Value::Int(i) => Value::Int(-i),
                Value::Float(f) => Value::Float(-f),
                _ => return Err(Diag::new(&C::UNEXPECTED_TOKEN, "`-` must precede a number", t.span)),
            },
            Tok::LBracket => {
                let mut items = Vec::new();
                loop {
                    self.skip_nl();
                    if matches!(self.peek().tok, Tok::RBracket) {
                        self.pos += 1;
                        break;
                    }
                    items.push(self.value()?);
                }
                Value::List(items)
            }
            Tok::LBrace => {
                let tbl = self.entries(true)?;
                self.pos += 1; // `}`
                Value::Table(tbl)
            }
            _ if t.text.chars().all(|c| c.is_alphanumeric() || c == '_') && !t.text.is_empty() => Value::Ident(t.text),
            other => return Err(Diag::new(&C::UNEXPECTED_TOKEN, format!("expected a value, found {}", other.describe()), t.span)),
        })
    }
}

// ---- writer --------------------------------------------------------------

pub fn quote(s: &str) -> String {
    let mut o = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => o += "\\\"",
            '\\' => o += "\\\\",
            '\n' => o += "\\n",
            '\t' => o += "\\t",
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

fn is_bare_key(k: &str) -> bool {
    k.chars().next().is_some_and(|c| c.is_alphabetic() || c == '_') && k.chars().all(|c| c.is_alphanumeric() || c == '_')
}

pub fn write_value(v: &Value, indent: usize, out: &mut String) {
    match v {
        Value::Str(s) => out.push_str(&quote(s)),
        Value::Int(i) => {
            let _ = write!(out, "{i}");
        }
        Value::Float(f) => {
            let _ = write!(out, "{f:?}");
        }
        Value::Bool(b) => {
            let _ = write!(out, "{b}");
        }
        Value::Ident(s) => out.push_str(s),
        Value::List(items) => {
            out.push('[');
            for (i, it) in items.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                write_value(it, indent, out);
            }
            out.push(']');
        }
        Value::Table(t) => {
            // short tables inline, others block-formatted
            let inline = t.entries.iter().all(|(_, v, _)| !matches!(v, Value::Table(_))) && t.entries.len() <= 6;
            if inline {
                out.push_str("{ ");
                for (i, (k, v, _)) in t.entries.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    out.push_str(&if is_bare_key(k) { k.clone() } else { quote(k) });
                    out.push_str(" = ");
                    write_value(v, indent, out);
                }
                out.push_str(" }");
            } else {
                out.push_str("{\n");
                write_entries(t, indent + 4, out);
                out.push_str(&" ".repeat(indent));
                out.push('}');
            }
        }
    }
}

pub fn write_entries(t: &Table, indent: usize, out: &mut String) {
    for (k, v, _) in &t.entries {
        out.push_str(&" ".repeat(indent));
        out.push_str(&if is_bare_key(k) { k.clone() } else { quote(k) });
        out.push_str(" = ");
        write_value(v, indent, out);
        out.push('\n');
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_manifest_shapes() {
        let c = parse(
            "module mim\nproject {\n  name = \"demo\"\n  kind = [bin, lib]\n  version = \"0.1.0\"\n}\n\
             dependencies {\n  speech = { git = \"github.com/x/speech.git\" }\n}\n\
             profile release { opt = 3, lto = true }\n\"a::b\" = { line = 3 }\n",
        )
        .unwrap();
        assert_eq!(c.header, "mim");
        let p = c.root.table("project").unwrap();
        assert_eq!(p.str("name"), Some("demo"));
        assert_eq!(p.get("kind").unwrap().as_list().unwrap().len(), 2);
        assert_eq!(c.root.table("profile").unwrap().table("release").unwrap().get("opt"), Some(&Value::Int(3)));
        assert!(c.root.table("a::b").is_some());
    }

    #[test]
    fn round_trip() {
        let mut t = Table::default();
        t.entries.push(("x".into(), Value::List(vec![Value::Int(1), Value::Str("s".into())]), Span::default()));
        let mut out = String::from("module t\n");
        write_entries(&t, 0, &mut out);
        let back = parse(&out).unwrap();
        assert_eq!(back.root.get("x"), t.get("x"));
    }
}
