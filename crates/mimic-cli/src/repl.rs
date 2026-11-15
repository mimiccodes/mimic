//! `mimic repl`: one line of Mimic at a time (`⍳ 10 ▷ +/`).
//!
//! A session keeps the definitions (`fn`, `type`, `extern`, `use`) and the
//! `let`s typed so far. Each input becomes a small program: the definitions,
//! then `main` with the `let`s and, for an expression, `print` of it. The
//! program is built (a debug build, cached like any other) and run; its
//! output is the answer. An input that does not compile is dropped, so the
//! session always holds a program that compiles.

use crate::driver::{self, Profile};
use crate::targets::Target;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::{cursor, execute, terminal};
use std::io::{BufRead, IsTerminal, Write};
use std::path::PathBuf;

#[derive(Default)]
pub struct Session {
    defs: Vec<String>,
    lets: Vec<String>,
    /// What the session's program prints by itself (its `let`s may print).
    base: String,
    dir: PathBuf,
}

/// What a piece of input is.
fn is_definition(s: &str) -> bool {
    let t = s.trim_start();
    ["fn ", "pub fn ", "type ", "pub type ", "extern ", "trusted extern ", "use ", "module ", "test "].iter().any(|p| t.starts_with(p))
}

fn is_statement(s: &str) -> bool {
    let t = s.trim_start();
    t.starts_with("let ") || t.starts_with("← ") || t.contains(" ← ")
}

/// Brackets still open in `s` (strings and comments skipped).
fn open_brackets(s: &str) -> i64 {
    let mut depth = 0;
    let mut inside = false;
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' if inside => {
                chars.next();
            }
            '"' => inside = !inside,
            '/' if !inside && chars.peek() == Some(&'/') => {
                while chars.peek().is_some_and(|c| *c != '\n') {
                    chars.next();
                }
            }
            '{' | '(' | '[' if !inside => depth += 1,
            '}' | ')' | ']' if !inside => depth -= 1,
            _ => {}
        }
    }
    depth
}

/// A report's errors, without the session's own file and lines.
fn messages(r: crate::report::Report) -> String {
    r.entries
        .iter()
        .filter(|e| e.diag.severity == mimic_syntax::Severity::Error)
        .map(|e| format!("error[{}]: {}", e.diag.code.id, e.diag.msg))
        .collect::<Vec<_>>()
        .join("\n")
}

impl Session {
    pub fn new() -> Session {
        let dir = std::env::temp_dir().join(format!("mimic-repl-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        Session { dir, ..Session::default() }
    }

    fn program(&self, extra_def: Option<&str>, extra_let: Option<&str>, expr: Option<&str>) -> String {
        let mut p = String::from("module main\n\n");
        for d in self.defs.iter().map(String::as_str).chain(extra_def) {
            p += d;
            p += "\n\n";
        }
        p += "fn main() {\n";
        for l in self.lets.iter().map(String::as_str).chain(extra_let) {
            p += "    ";
            p += l;
            p += "\n";
        }
        if let Some(e) = expr {
            p += &format!("    print({e})\n");
        }
        p += "}\n";
        p
    }

    /// Build and run a program: its output, or the compiler's messages.
    fn run(&self, program: &str) -> Result<String, String> {
        let f = self.dir.join("repl.mim");
        std::fs::write(&f, program).map_err(|e| e.to_string())?;
        let input = driver::input_from(Some(&f))?;
        let mut prof = Profile::for_input(&input, false);
        prof.leak_check = false;
        let a = driver::build(&input, &prof, &Target::host(), false, false).map_err(messages)?;
        let out = std::process::Command::new(&a.path).stdin(std::process::Stdio::null()).output().map_err(|e| e.to_string())?;
        let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
        let err = String::from_utf8_lossy(&out.stderr);
        if !out.status.success() {
            text += &err;
        }
        Ok(text)
    }

    /// Evaluate one input: the value of an expression, the output of a
    /// statement, or nothing for a definition. The session keeps what
    /// compiled.
    pub fn eval(&mut self, input: &str) -> Result<String, String> {
        let input = input.trim();
        if input.is_empty() {
            return Ok(String::new());
        }
        if is_definition(input) {
            let p = self.program(Some(input), None, None);
            // run it (with the earlier `let`s) only to check that it compiles
            self.base = self.run(&p)?;
            self.defs.push(input.to_string());
            return Ok(String::new());
        }
        if is_statement(input) {
            let out = self.run(&self.program(None, Some(input), None))?;
            self.lets.push(input.to_string());
            // only what this statement printed
            let new = out.strip_prefix(&self.base).unwrap_or(&out).to_string();
            self.base = out;
            return Ok(new);
        }
        // an expression: its value (and anything it printed); a call that
        // gives nothing (`print(x)`) shows only what it printed
        let out = self.run(&self.program(None, None, Some(input)))?;
        let new = out.strip_prefix(&self.base).unwrap_or(&out);
        Ok(new.strip_suffix("()\n").unwrap_or(new).to_string())
    }

    pub fn reset(&mut self) {
        self.defs.clear();
        self.lets.clear();
        self.base.clear();
    }

    /// The session as a program (`:show`).
    pub fn show(&self) -> String {
        self.program(None, None, None)
    }
}

/// What reading a line gave.
enum Line {
    Text(String),
    /// Ctrl-C: drop the line.
    Cancel,
    /// Ctrl-D on an empty line, or the end of the input.
    End,
}

/// Read one line in a terminal: arrows, Home/End, history (Up/Down),
/// Ctrl-U, and glyph input (`\i` → `⍳`, not inside strings).
fn read_line(prompt: &str, prompt_width: u16, history: &[String]) -> Line {
    let mut buf: Vec<char> = Vec::new();
    let mut at = 0usize;
    let mut back = history.len();
    let mut out = std::io::stdout();
    let _ = terminal::enable_raw_mode();
    let draw = |buf: &[char], at: usize, out: &mut std::io::Stdout| {
        let text: String = buf.iter().collect();
        let col: usize = buf[..at].iter().map(|c| unicode_width::UnicodeWidthChar::width(*c).unwrap_or(0)).sum();
        let _ = execute!(out, cursor::MoveToColumn(0), terminal::Clear(terminal::ClearType::UntilNewLine));
        let _ = write!(out, "{prompt}{text}");
        let _ = execute!(out, cursor::MoveToColumn(prompt_width + col as u16));
        let _ = out.flush();
    };
    draw(&buf, at, &mut out);
    let result = loop {
        let Ok(ev) = event::read() else { break Line::End };
        let Event::Key(k) = ev else { continue };
        if k.kind == KeyEventKind::Release {
            continue;
        }
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        match k.code {
            KeyCode::Char('c') if ctrl => break Line::Cancel,
            KeyCode::Char('d') if ctrl => {
                if buf.is_empty() {
                    break Line::End;
                }
                if at < buf.len() {
                    buf.remove(at);
                }
            }
            KeyCode::Char('a') if ctrl => at = 0,
            KeyCode::Char('e') if ctrl => at = buf.len(),
            KeyCode::Char('u') if ctrl => {
                buf.drain(..at);
                at = 0;
            }
            KeyCode::Char(c) if !ctrl => {
                buf.insert(at, c);
                at += 1;
                if at >= 2 {
                    let two: String = buf[at - 2..at].iter().collect();
                    let before: String = buf[..at - 2].iter().collect();
                    if let Some(g) = mimic_syntax::glyphs::digraph(&two)
                        && !crate::edit::inside_string(&before)
                    {
                        buf.splice(at - 2..at, g.chars());
                        at = at - 2 + g.chars().count();
                    }
                }
            }
            KeyCode::Tab => {
                buf.splice(at..at, "    ".chars());
                at += 4;
            }
            KeyCode::Backspace if at > 0 => {
                at -= 1;
                buf.remove(at);
            }
            KeyCode::Delete if at < buf.len() => {
                buf.remove(at);
            }
            KeyCode::Left if at > 0 => at -= 1,
            KeyCode::Right if at < buf.len() => at += 1,
            KeyCode::Home => at = 0,
            KeyCode::End => at = buf.len(),
            KeyCode::Up if back > 0 => {
                back -= 1;
                buf = history[back].chars().collect();
                at = buf.len();
            }
            KeyCode::Down if back < history.len() => {
                back += 1;
                buf = history.get(back).map(|h| h.chars().collect()).unwrap_or_default();
                at = buf.len();
            }
            KeyCode::Enter => break Line::Text(buf.iter().collect()),
            _ => {}
        }
        draw(&buf, at, &mut out);
    };
    let _ = terminal::disable_raw_mode();
    println!();
    result
}

pub fn run() -> Result<(), String> {
    let tty = std::io::stdin().is_terminal();
    let color = tty && std::env::var_os("NO_COLOR").is_none();
    let paint = |code: &str, s: &str| if color { format!("\x1b[{code}m{s}\x1b[0m") } else { s.to_string() };
    if tty {
        println!("{} {}", paint("1;38;2;226;75;4", "MIMIC"), paint("2", &format!("repl {} · :help for commands · \\i types ⍳", env!("CARGO_PKG_VERSION"))));
    }
    let mut session = Session::new();
    let stdin = std::io::stdin();
    let mut lines = stdin.lock().lines();
    let mut history: Vec<String> = Vec::new();
    let (first, more) = (format!("{} ", paint("38;2;226;75;4", "›")), format!("{} ", paint("2", "…")));
    // one line: from the terminal (edited) or from piped input
    let mut next = |prompt: &str, history: &[String]| -> Line {
        if tty {
            read_line(prompt, 2, history)
        } else {
            match lines.next() {
                Some(Ok(l)) => Line::Text(l),
                _ => Line::End,
            }
        }
    };
    'session: loop {
        let mut input = match next(&first, &history) {
            Line::Text(t) => t,
            Line::Cancel => continue,
            Line::End => break,
        };
        // a definition or block over several lines: read until it closes
        while open_brackets(&input) > 0 {
            match next(&more, &history) {
                Line::Text(l) => {
                    input.push('\n');
                    input.push_str(&l);
                }
                Line::Cancel => continue 'session,
                Line::End => break,
            }
        }
        if tty && !input.trim().is_empty() && history.last() != Some(&input) {
            history.push(input.clone());
        }
        let cmd = input.trim();
        match cmd.split_whitespace().next().unwrap_or("") {
            ":q" | ":quit" | ":exit" => break 'session,
            ":help" => {
                println!("Type an expression to see its value, a `let` to keep a value, or a `fn` or `type` to define it.");
                println!("  :show    the session as a program     :reset   forget everything");
                println!("  :man X   the manual's page for X     :glyphs  every glyph and how to type it");
                println!("  :quit    leave (or Ctrl-D)");
                println!("Glyphs: type `\\i` for ⍳, `\\e` for ¨, `\\k` for ⌿ (`:glyphs` lists them). Up and Down go through what you typed.");
                continue;
            }
            ":show" => {
                print!("{}", session.show());
                continue;
            }
            ":reset" => {
                session.reset();
                println!("{}", paint("2", "(the session is empty)"));
                continue;
            }
            ":glyphs" => {
                print!("{}", mimic_syntax::glyphs::table());
                continue;
            }
            ":man" => {
                let topic = cmd.trim_start_matches(":man").trim();
                let m = crate::man::Manual::load();
                match m.find(topic) {
                    Ok(p) => print!("{}", crate::man::render(&p.text, color)),
                    Err(_) => println!("no page `{topic}`"),
                }
                continue;
            }
            _ => {}
        }
        match session.eval(&input) {
            Ok(out) => print!("{out}"),
            Err(e) => println!("{}", paint("38;2;255;92;92", &e)),
        }
        let _ = std::io::stdout().flush();
    }
    let _ = std::fs::remove_dir_all(&session.dir);
    Ok(())
}
