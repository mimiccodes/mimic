//! `mimic fmt`: words or glyphs, indentation, whitespace.
//!
//! The formatter works on the source text, guided by the tokens: comments
//! and the program's own line breaks stay as they are. It
//!
//! - writes primitives and operators as words (`iota`, `each`, `|>`,
//!   `!=`, `and`) or, with `glyphs`, as glyphs (`⍳`, `¨`, `▷`, `≠`, `∧`);
//! - indents every line by its nesting depth (4 spaces a level);
//! - removes trailing spaces, keeps at most one blank line in a row, and
//!   ends the file with one newline.
//!
//! Glyphs and words lex to the same tokens, so the result is checked: it
//! must lex to exactly the tokens of the original, or nothing is changed.

use crate::glyphs::PRIMS;
use crate::lexer::{Tok, Token, lex};
use crate::{Diag, codes};

/// Syntax glyphs written as one word or operator (and back): glyph, word.
/// (`←` needs a `let` around it, `↩` is a token of its own, `¯` is part of
/// a number: they stay as written.)
const SYNTAX: &[(&str, &str)] = &[
    ("§", "module"),
    ("ƒ", "fn"),
    ("▷", "|>"),
    ("→", "->"),
    ("⇒", "=>"),
    ("×", "*"),
    ("÷", "/"),
    ("⫽", "div"),
    ("≠", "!="),
    ("≤", "<="),
    ("≥", ">="),
    ("∧", "and"),
    ("∨", "or"),
    ("¬", "not"),
    ("⋔", "arms"),
    ("⇑", "pub"),
    ("⇄", "mut"),
    ("⍰", "if"),
    ("⋄", "else"),
    ("⏎", "return"),
    ("⇐", "use"),
    ("≔", "as"),
    ("⌁", "extern"),
    ("☡", "unsafe"),
    ("✓", "trusted"),
    ("⊡", "type"),
    ("⊨", "match"),
    ("⊕", "with"),
    ("●", "true"),
    ("○", "false"),
    ("⊢", "test"),
    ("∀", "for"),
    ("⟳", "while"),
    ("↯", "break"),
    ("⎘", "include"),
];

/// Glyphs of keywords: `mimic fmt --glyphs` leaves their words as they are.
const KEYWORD_GLYPHS: &[&str] = &["§", "ƒ", "⇑", "⇄", "⍰", "⋄", "⏎", "⇐", "≔", "⌁", "☡", "✓", "⊡", "⊨", "⊕", "●", "○", "⊢", "∀", "⎘", "⟳", "↯"];

/// The text a token is written as in the chosen form, if it changes.
fn respell(t: &Token, glyphs: bool) -> Option<&'static str> {
    if let Tok::Prim(p) = &t.tok {
        let info = PRIMS.iter().find(|i| i.prim == *p)?;
        let want = if glyphs { info.glyph } else { info.word };
        return (t.text != want).then_some(want);
    }
    if glyphs {
        // (keywords keep their words: `module`, `fn`, `if`, `match`…;
        // glyphs are for primitives and operators. Back to words, every
        // glyph is written as its word.)
        let (g, _) = SYNTAX.iter().find(|(g, w)| *w == t.text && !KEYWORD_GLYPHS.contains(g))?;
        // `*` and `/` are words in other places too (`*/`): only operators
        let op = !matches!(t.tok, Tok::Ident(_));
        return op.then_some(g);
    }
    let (_, w) = SYNTAX.iter().find(|(g, _)| *g == t.text)?;
    Some(w)
}

fn wordy(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Format a whole file. `Err` when it does not lex (or, which would be a
/// bug, when the result would not mean the same).
/// [`format`] a file, after checking that it parses: a file that does not
/// is left as it is, and the parse error comes back, as `gofmt` and
/// `rustfmt` do (respelling `/` beside a glyph the lexer misread would
/// otherwise change code its author never wrote). `name` is the file's
/// name: `mim.mim` and `build.mim` are a project's configuration.
pub fn format_file(name: &str, src: &str, glyphs: bool) -> Result<String, Diag> {
    if matches!(name, "mim.mim" | "build.mim") {
        crate::config::parse(src)?;
    } else {
        crate::parser::parse_file(src)?;
    }
    format(src, glyphs)
}

pub fn format(src: &str, glyphs: bool) -> Result<String, Diag> {
    let toks = lex(src)?;
    // 1. spellings
    let mut out = String::with_capacity(src.len());
    let mut at = 0;
    for t in &toks {
        if t.tok == Tok::Eof {
            break;
        }
        let Some(new) = respell(t, glyphs) else { continue };
        out.push_str(&src[at..t.span.start]);
        // a word must not run into a neighbouring word: `xs¨f` → `xs each f`
        if new.starts_with(wordy) && out.chars().last().is_some_and(wordy) {
            out.push(' ');
        }
        out.push_str(new);
        if new.ends_with(wordy) && src[t.span.end..].chars().next().is_some_and(wordy) {
            out.push(' ');
        }
        at = t.span.end;
    }
    out.push_str(&src[at..]);
    // 2. indentation, from the tokens of the respelled text
    let toks2 = lex(&out)?;
    let mut depth_at_line: Vec<Option<(usize, bool)>> = Vec::new(); // per line: depth before it, starts with a closer
    let mut depth: usize = 0;
    let mut line_of_first: Option<u32> = None;
    let mut inside_string_lines: Vec<u32> = Vec::new();
    let mut end_depth: Vec<Option<usize>> = Vec::new();
    for t in &toks2 {
        let line = t.span.line as usize;
        if matches!(t.tok, Tok::Newline | Tok::Eof) {
            continue;
        }
        if depth_at_line.len() <= line {
            depth_at_line.resize(line + 1, None);
        }
        if line_of_first != Some(t.span.line) {
            line_of_first = Some(t.span.line);
            if depth_at_line[line].is_none() {
                let closer = matches!(t.tok, Tok::RBrace | Tok::RParen | Tok::RBracket);
                depth_at_line[line] = Some((depth, closer));
            }
        }
        // a token over several lines (a string): its other lines stay as they are
        let text = &out[t.span.start..t.span.end];
        for k in 1..=text.matches('\n').count() as u32 {
            inside_string_lines.push(t.span.line + k);
        }
        match t.tok {
            Tok::LBrace | Tok::LParen | Tok::LBracket => depth += 1,
            Tok::RBrace | Tok::RParen | Tok::RBracket => depth = depth.saturating_sub(1),
            _ => {}
        }
        // the depth after this line's tokens (a comment below sits there)
        let last = t.span.line as usize + text.matches('\n').count();
        if end_depth.len() <= last {
            end_depth.resize(last + 1, None);
        }
        end_depth[last] = Some(depth);
    }
    let mut lines = Vec::new();
    let mut blank = 0;
    for (i, line) in out.split('\n').enumerate() {
        let n = i as u32 + 1;
        if inside_string_lines.contains(&n) {
            lines.push(line.to_string());
            blank = 0;
            continue;
        }
        let body = line.trim();
        if body.is_empty() {
            blank += 1;
            // no blank line right after an opening bracket
            let after_open = lines.last().is_some_and(|l: &String| l.ends_with(['{', '(', '[']));
            if blank <= 1 && !lines.is_empty() && !after_open {
                lines.push(String::new());
            }
            continue;
        }
        // nor right before a closing one
        if body.starts_with(['}', ')', ']']) && lines.last().is_some_and(|l| l.is_empty()) {
            lines.pop();
        }
        blank = 0;
        let d = match depth_at_line.get(n as usize).copied().flatten() {
            Some((d, closer)) => d.saturating_sub(closer as usize),
            // a line of only a comment: as deep as where it sits
            None => end_depth.iter().take(n as usize).rev().flatten().next().copied().unwrap_or(0),
        };
        // a contract line (`requires …`, `ensures …`) under a function's
        // signature: one level in, like the body it describes
        let contract = |l: &str| l.starts_with("requires ") || l.starts_with("ensures ");
        let d = if contract(body)
            && lines.last().is_some_and(|l: &String| {
                let l = l.trim_start();
                l.starts_with("fn ") || l.starts_with("pub fn ") || l.starts_with("ƒ") || contract(l)
            }) {
            d + 1
        } else {
            d
        };
        lines.push(format!("{}{}", "    ".repeat(d), body.trim_end()));
    }
    while lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }
    let mut text = lines.join("\n");
    text.push('\n');
    // 3. the same program: the same tokens, whatever their spelling
    let same = |a: &[Token], b: &[Token]| a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.tok == y.tok);
    let check = lex(&text)?;
    if !same(&toks, &check) {
        return Err(Diag::new(&codes::UNSUPPORTED_SYNTAX, "mimic fmt: formatting would change this file's meaning; it was left as it is (please report this)", crate::Span::default()));
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::{format, format_file};

    #[test]
    fn words_and_glyphs() {
        let src = "module main\n\nfn main() {\nprint(⍳ 4 ¨ (x ⇒ x × x) ▷ +/)   \n\n\n  let ok = 1 ≠ 2 ∧ ¬ false\n}\n";
        let w = format(src, false).unwrap();
        assert_eq!(w, "module main\n\nfn main() {\n    print(iota 4 each (x => x * x) |> sum)\n\n    let ok = 1 != 2 and not false\n}\n");
        let g = format(&w, true).unwrap();
        assert_eq!(g, "module main\n\nfn main() {\n    ⎕(⍳ 4 ¨ (x ⇒ x × x) ▷ +/)\n\n    let ok = 1 ≠ 2 ∧ ¬ false\n}\n");
        assert_eq!(format(&g, false).unwrap(), w);
    }

    #[test]
    fn and_or_reductions_keep_their_slash() {
        // `∧/` is one token (all): `--glyphs` must not respell its `/` as `÷`
        let src = "module main\nfn main() {\n    let ys = [true false] |> ∧/\n    let zs = ∨/ [true]\n}\n";
        let g = format(src, true).unwrap();
        assert!(g.contains("▷ ∧/") && g.contains("∨/ [true]"), "{g}");
    }

    #[test]
    fn a_file_that_does_not_parse_is_left_alone() {
        let src = "module main\nfn main() {\n    let x = 4 / 2 (\n}\n";
        assert!(format_file("main.mim", src, true).is_err());
        let ok = "module main\nfn main() {\n    let x = 4 / 2\n}\n";
        assert!(format_file("main.mim", ok, true).unwrap().contains("4 ÷ 2"));
    }

    #[test]
    fn words_do_not_run_together() {
        assert_eq!(format("module main\nfn f(xs) { xs¨g }\n", false).unwrap(), "module main\nfn f(xs) { xs each g }\n");
    }

    #[test]
    fn comments_and_strings_stay() {
        let src = "§ main\n// a comment ¨\nfn main() {\n        // inside\n  print('raw ¨ {x}')\n\n  // last\n}\n";
        assert_eq!(format(src, false).unwrap(), "module main\n// a comment ¨\nfn main() {\n    // inside\n    print('raw ¨ {x}')\n\n    // last\n}\n");
    }
}
