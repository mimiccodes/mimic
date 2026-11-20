//! `mimic edit`: Mimic's own terminal editor.
//!
//! It is made for Mimic: highlighting from Mimic's own lexer (names from C
//! in their own colour), glyph input (`\i` becomes `⍳`) and a glyph palette,
//! errors as you type from the Workbench engine, and panes for problems,
//! the audit, the manual, where code runs, and the output of runs and
//! tests. Keys are the easy ones (Ctrl-S saves, Ctrl-Q quits); the mouse
//! works. The editor's state and its key handling are plain data (tested
//! without a terminal); the terminal only draws them.

use crate::driver::sources;
use crate::engine::Engine;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use mimic_syntax::glyphs::{PRIMS, SYNTAX_GLYPHS};
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph};
use ropey::Rope;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};
use unicode_width::UnicodeWidthChar;

// ---- Mimic's colours (assets/images/BRAND.md) -----------------------------------

const BG: Color = Color::Rgb(10, 10, 10);
const PANEL: Color = Color::Rgb(20, 18, 17);
const LINE: Color = Color::Rgb(38, 34, 31);
const INK: Color = Color::Rgb(246, 241, 236);
const MUTED: Color = Color::Rgb(166, 156, 148);
const ORANGE: Color = Color::Rgb(226, 75, 4);
const BRIGHT: Color = Color::Rgb(255, 106, 31);
const DEEP: Color = Color::Rgb(174, 56, 0);
const STRING: Color = Color::Rgb(159, 211, 168);
const NUMBER: Color = Color::Rgb(201, 167, 239);
const TYPE: Color = Color::Rgb(124, 196, 220);
const COMMENT: Color = Color::Rgb(125, 116, 108);
const FOREIGN: Color = Color::Rgb(230, 190, 90);
const ERROR: Color = Color::Rgb(255, 92, 92);

// ---- documents ---------------------------------------------------------------------

#[derive(Clone)]
struct Snapshot {
    text: Rope,
    cursor: (usize, usize),
}

pub struct Doc {
    path: Option<PathBuf>,
    text: Rope,
    /// (line, column in characters)
    cursor: (usize, usize),
    /// the other end of the selection
    anchor: Option<(usize, usize)>,
    /// the first line and column shown
    top: usize,
    left: usize,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    /// typing into the same word joins one undo step
    last_kind: Option<&'static str>,
    dirty: bool,
}

impl Doc {
    fn new(path: Option<PathBuf>, text: &str) -> Doc {
        Doc { path, text: Rope::from_str(text), cursor: (0, 0), anchor: None, top: 0, left: 0, undo: Vec::new(), redo: Vec::new(), last_kind: None, dirty: false }
    }

    fn name(&self) -> String {
        self.path.as_ref().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "untitled.mim".into())
    }

    fn line_len(&self, line: usize) -> usize {
        if line >= self.text.len_lines() {
            return 0;
        }
        let l = self.text.line(line);
        let n = l.len_chars();
        // without the line break
        if n > 0 && l.char(n - 1) == '\n' { n - 1 } else { n }
    }

    fn lines(&self) -> usize {
        self.text.len_lines().max(1)
    }

    fn char_at(&self, (line, col): (usize, usize)) -> usize {
        let line = line.min(self.lines() - 1);
        self.text.line_to_char(line) + col.min(self.line_len(line))
    }

    fn pos_of(&self, ch: usize) -> (usize, usize) {
        let ch = ch.min(self.text.len_chars());
        let line = self.text.char_to_line(ch);
        (line, ch - self.text.line_to_char(line))
    }

    fn line_text(&self, line: usize) -> String {
        if line >= self.text.len_lines() {
            return String::new();
        }
        let s = self.text.line(line).to_string();
        s.trim_end_matches(['\n', '\r']).to_string()
    }

    fn save_undo(&mut self, kind: &'static str) {
        if self.last_kind != Some(kind) || kind != "type" {
            self.undo.push(Snapshot { text: self.text.clone(), cursor: self.cursor });
            if self.undo.len() > 500 {
                self.undo.remove(0);
            }
        }
        self.redo.clear();
        self.last_kind = Some(kind);
        self.dirty = true;
    }

    fn selection(&self) -> Option<(usize, usize)> {
        let a = self.anchor?;
        let (x, y) = (self.char_at(a), self.char_at(self.cursor));
        (x != y).then(|| (x.min(y), x.max(y)))
    }

    fn delete_selection(&mut self) -> bool {
        let Some((a, b)) = self.selection() else { return false };
        self.text.remove(a..b);
        self.cursor = self.pos_of(a);
        self.anchor = None;
        true
    }

    fn insert(&mut self, s: &str) {
        self.save_undo(if s.chars().all(|c| c.is_alphanumeric() || c == '_') { "type" } else { "insert" });
        self.delete_selection();
        let at = self.char_at(self.cursor);
        self.text.insert(at, s);
        self.cursor = self.pos_of(at + s.chars().count());
    }

    fn backspace(&mut self) {
        self.save_undo("delete");
        if self.delete_selection() {
            return;
        }
        let at = self.char_at(self.cursor);
        if at == 0 {
            return;
        }
        // four spaces of indentation go at once
        let (line, col) = self.cursor;
        let before: String = self.line_text(line).chars().take(col).collect();
        let n = if col >= 4 && col % 4 == 0 && before.ends_with("    ") && before.trim().is_empty() { 4 } else { 1 };
        self.text.remove(at - n..at);
        self.cursor = self.pos_of(at - n);
    }

    fn delete(&mut self) {
        self.save_undo("delete");
        if self.delete_selection() {
            return;
        }
        let at = self.char_at(self.cursor);
        if at < self.text.len_chars() {
            self.text.remove(at..at + 1);
        }
    }

    /// A new line, indented like this one (one more after an opening bracket).
    fn newline(&mut self) {
        let line = self.line_text(self.cursor.0);
        let before: String = line.chars().take(self.cursor.1).collect();
        let mut indent: String = line.chars().take_while(|c| *c == ' ').collect();
        if before.trim_end().ends_with(['{', '(', '[']) {
            indent += "    ";
        }
        self.insert(&format!("\n{indent}"));
    }

    fn undo(&mut self) {
        if let Some(s) = self.undo.pop() {
            self.redo.push(Snapshot { text: self.text.clone(), cursor: self.cursor });
            self.text = s.text;
            self.cursor = s.cursor;
            self.anchor = None;
            self.last_kind = None;
            self.dirty = true;
        }
    }

    fn redo(&mut self) {
        if let Some(s) = self.redo.pop() {
            self.undo.push(Snapshot { text: self.text.clone(), cursor: self.cursor });
            self.text = s.text;
            self.cursor = s.cursor;
            self.anchor = None;
            self.last_kind = None;
            self.dirty = true;
        }
    }

    fn move_to(&mut self, pos: (usize, usize), select: bool) {
        if select {
            if self.anchor.is_none() {
                self.anchor = Some(self.cursor);
            }
        } else {
            self.anchor = None;
        }
        let line = pos.0.min(self.lines() - 1);
        self.cursor = (line, pos.1.min(self.line_len(line)));
        self.last_kind = None;
    }

    /// The word under the cursor (letters, digits, `_` and `::`).
    fn word(&self) -> String {
        let line: Vec<char> = self.line_text(self.cursor.0).chars().collect();
        let ok = |c: char| c.is_alphanumeric() || c == '_' || c == ':';
        let mut a = self.cursor.1.min(line.len());
        let mut b = a;
        while a > 0 && ok(line[a - 1]) {
            a -= 1;
        }
        while b < line.len() && ok(line[b]) {
            b += 1;
        }
        if a == b {
            // a glyph under the cursor
            return line.get(self.cursor.1).map(|c| c.to_string()).unwrap_or_default();
        }
        line[a..b].iter().collect::<String>().trim_matches(':').to_string()
    }
}

// ---- the editor -----------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Pane {
    Problems,
    Audit,
    Manual,
    Where,
    Output,
    Repl,
}

const PANES: [Pane; 6] = [Pane::Problems, Pane::Audit, Pane::Manual, Pane::Where, Pane::Output, Pane::Repl];

impl Pane {
    fn title(self) -> &'static str {
        match self {
            Pane::Problems => "Problems",
            Pane::Audit => "Audit",
            Pane::Manual => "Manual",
            Pane::Where => "Where code runs",
            Pane::Output => "Output",
            Pane::Repl => "REPL",
        }
    }
}

/// Everything the editor can do, for the toolbar, the command list (F2)
/// and the start screen: what it is called, its key, and the key it acts as.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Cmd {
    Run,
    Test,
    Check,
    Audit,
    Where,
    Manual,
    Repl,
    Find,
    Replace,
    Glyphs,
    Open,
    New,
    Save,
    Close,
    Files,
    Panel,
    GotoLine,
    Undo,
    Redo,
    Commands,
    Quit,
}

const COMMANDS: &[(Cmd, &str, &str)] = &[
    (Cmd::Run, "Run the program", "F5"),
    (Cmd::Test, "Run the tests", "F6"),
    (Cmd::Check, "Problems", "F7"),
    (Cmd::Audit, "Audit: quality and security", "F8"),
    (Cmd::Where, "Where code runs", "F9"),
    (Cmd::Manual, "The manual for the word under the cursor", "F1"),
    (Cmd::Repl, "Evaluate a line (the REPL)", "F4"),
    (Cmd::Find, "Find", "Ctrl-F"),
    (Cmd::Replace, "Replace", "Ctrl-H"),
    (Cmd::Glyphs, "Glyph palette", "Ctrl-G"),
    (Cmd::Open, "Open a file", "Ctrl-O"),
    (Cmd::New, "New program", "Ctrl-N"),
    (Cmd::Save, "Save", "Ctrl-S"),
    (Cmd::Close, "Close the file", "Ctrl-W"),
    (Cmd::Files, "Show or hide the files", "Ctrl-B"),
    (Cmd::Panel, "Show or hide the bottom pane", "Ctrl-J"),
    (Cmd::GotoLine, "Go to a line", "Ctrl-L"),
    (Cmd::Undo, "Undo", "Ctrl-Z"),
    (Cmd::Redo, "Redo", "Ctrl-Y"),
    (Cmd::Commands, "All commands", "F2"),
    (Cmd::Quit, "Quit", "Ctrl-Q"),
];

impl Cmd {
    /// The key it is: a click and the command list press it.
    fn key(self) -> KeyEvent {
        let f = |n| KeyEvent::new(KeyCode::F(n), KeyModifiers::NONE);
        let c = |ch| KeyEvent::new(KeyCode::Char(ch), KeyModifiers::CONTROL);
        match self {
            Cmd::Run => f(5),
            Cmd::Test => f(6),
            Cmd::Check => f(7),
            Cmd::Audit => f(8),
            Cmd::Where => f(9),
            Cmd::Manual => f(1),
            Cmd::Repl => f(4),
            Cmd::Commands => f(2),
            Cmd::Find => c('f'),
            Cmd::Replace => c('h'),
            Cmd::Glyphs => c('g'),
            Cmd::Open => c('o'),
            Cmd::New => c('n'),
            Cmd::Save => c('s'),
            Cmd::Close => c('w'),
            Cmd::Files => c('b'),
            Cmd::Panel => c('j'),
            Cmd::GotoLine => c('l'),
            Cmd::Undo => c('z'),
            Cmd::Redo => c('y'),
            Cmd::Quit => c('q'),
        }
    }
}

/// What is under the mouse: recorded while drawing.
#[derive(Clone, Debug, PartialEq)]
enum Hit {
    /// a row of the file tree
    Tree(usize),
    Tab(usize),
    TabClose(usize),
    Button(Cmd),
    Pane(Pane),
    /// a line of the bottom pane (it may name a file and line)
    PaneLine(usize),
    Errors,
    PromptItem(usize),
}

/// A row of the file tree: a folder or a file, at a depth.
#[derive(Clone, Debug)]
struct Row {
    depth: usize,
    path: PathBuf,
    dir: bool,
    name: String,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum PromptKind {
    Find,
    Replace,
    ReplaceWith(String),
    Open,
    Glyph,
    GotoLine,
    /// A line for the REPL (F4)
    Eval,
    /// The command list (F2)
    Command,
}

pub struct Prompt {
    kind: PromptKind,
    input: String,
    selected: usize,
}

/// A problem in a file: (line, column, end column, message, code, is an error).
#[derive(Clone)]
struct Problem {
    path: PathBuf,
    line: usize,
    col: usize,
    end_col: usize,
    message: String,
    code: String,
}

pub struct Editor {
    pub docs: Vec<Doc>,
    pub cur: usize,
    root: PathBuf,
    files: Vec<PathBuf>,
    pub show_files: bool,
    pub show_panel: bool,
    pub pane: Pane,
    pane_lines: Vec<(String, Style)>,
    pane_scroll: usize,
    pub prompt: Option<Prompt>,
    pub message: String,
    clipboard: String,
    engine: Option<Engine>,
    problems: Vec<Problem>,
    /// where code runs, per line of the current file
    labels: Vec<(usize, String)>,
    pub show_labels: bool,
    edited_at: Option<Instant>,
    quit_armed: bool,
    pub quit: bool,
    jobs: Option<mpsc::Receiver<(Pane, String)>>,
    /// The REPL's session (as `mimic repl`), its lines so far, and the
    /// line being evaluated on another thread
    repl: Option<crate::repl::Session>,
    repl_log: Vec<(String, Style)>,
    repl_job: Option<mpsc::Receiver<(crate::repl::Session, Result<String, String>)>>,
    last_find: String,
    /// the editor's area on screen (for the mouse)
    area: Rect,
    gutter: u16,
    /// folders of the file tree that are closed, and its first row shown
    collapsed: std::collections::HashSet<PathBuf>,
    tree_top: usize,
    tree_area: Rect,
    panel_area: Rect,
    /// what each pane showed last, to show again when its tab is clicked
    panes: std::collections::HashMap<Pane, Vec<(String, Style)>>,
    /// where things are on screen, for clicks (filled while drawing)
    hits: std::cell::RefCell<Vec<(Rect, Hit)>>,
}

fn glyph_table() -> Vec<(&'static str, &'static str, Option<&'static str>, &'static str)> {
    let mut v: Vec<_> = PRIMS.iter().map(|p| (p.glyph, p.word, p.digraph, p.doc)).collect();
    v.extend(SYNTAX_GLYPHS.iter().map(|g| (g.glyph, g.word, g.digraph, g.doc)));
    v
}

/// Does `item` contain the letters of `q` in order (a simple fuzzy match)?
/// The score is lower for tighter, earlier matches.
fn fuzzy(q: &str, item: &str) -> Option<usize> {
    if q.is_empty() {
        return Some(0);
    }
    let item_l = item.to_lowercase();
    if let Some(i) = item_l.find(&q.to_lowercase()) {
        return Some(i);
    }
    let mut pos = 0;
    let mut score = 100;
    let chars: Vec<char> = item_l.chars().collect();
    for qc in q.to_lowercase().chars() {
        let k = chars[pos..].iter().position(|&c| c == qc)?;
        score += k;
        pos += k + 1;
    }
    Some(score)
}

fn project_files(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for name in ["mim.mim", "build.mim"] {
        if root.join(name).is_file() {
            out.push(root.join(name));
        }
    }
    let mut stack = vec![root.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        let mut entries: Vec<PathBuf> = rd.filter_map(|e| e.ok().map(|e| e.path())).collect();
        entries.sort();
        for p in entries {
            let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            if name.starts_with('.') || name == "target" || name == "node_modules" {
                continue;
            }
            if p.is_dir() {
                stack.push(p);
            } else if (name.ends_with(".mim") || name.ends_with(".md") || name.ends_with(".mnet")) && name != "mim.mim" && name != "build.mim" && name != "mod.mim" {
                out.push(p);
            }
        }
        if out.len() > 2000 {
            break;
        }
    }
    out
}

impl Editor {
    /// The editor on `target`: a file, a folder (its project), or the
    /// project of the current folder.
    pub fn open(target: Option<&Path>) -> Result<Editor, String> {
        let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
        let (root, first) = match target {
            Some(p) if p.is_dir() => (mimic_project::find_root(p).unwrap_or_else(|| p.to_path_buf()), None),
            Some(p) => {
                let abs = if p.is_absolute() { p.to_path_buf() } else { cwd.join(p) };
                let dir = abs.parent().map(Path::to_path_buf).unwrap_or(cwd.clone());
                (mimic_project::find_root(&dir).unwrap_or(dir), Some(abs))
            }
            None => (mimic_project::find_root(&cwd).unwrap_or(cwd.clone()), None),
        };
        let root = root.canonicalize().unwrap_or(root);
        let engine = match &first {
            Some(f) if !f.starts_with(root.join("src")) => Some(Engine::for_file(f)),
            _ => Engine::for_project(&root).ok(),
        };
        let mut ed = Editor {
            docs: Vec::new(),
            cur: 0,
            files: project_files(&root),
            root,
            show_files: true,
            show_panel: false,
            pane: Pane::Problems,
            pane_lines: Vec::new(),
            pane_scroll: 0,
            prompt: None,
            message: String::new(),
            clipboard: String::new(),
            engine,
            problems: Vec::new(),
            labels: Vec::new(),
            show_labels: true,
            edited_at: None,
            quit_armed: false,
            quit: false,
            jobs: None,
            repl: None,
            repl_log: Vec::new(),
            repl_job: None,
            last_find: String::new(),
            area: Rect::default(),
            gutter: 6,
            collapsed: std::collections::HashSet::new(),
            tree_top: 0,
            tree_area: Rect::default(),
            panel_area: Rect::default(),
            panes: std::collections::HashMap::new(),
            hits: std::cell::RefCell::new(Vec::new()),
        };
        // folders start closed, except src/ and those holding the file opened
        for f in &ed.files {
            let mut d = f.parent();
            while let Some(dir) = d {
                if dir == ed.root || !dir.starts_with(&ed.root) {
                    break;
                }
                let keep_open = dir == ed.root.join("src") || first.as_deref().is_some_and(|x| x.starts_with(dir));
                if !keep_open {
                    ed.collapsed.insert(dir.to_path_buf());
                }
                d = dir.parent();
            }
        }
        if let Some(f) = first {
            ed.open_file(&f);
        }
        ed.check();
        Ok(ed)
    }

    fn open_file(&mut self, path: &Path) {
        let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        if let Some(i) = self.docs.iter().position(|d| d.path.as_deref() == Some(&path)) {
            self.cur = i;
            return;
        }
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        self.docs.push(Doc::new(Some(path), &text));
        self.cur = self.docs.len() - 1;
        self.refresh_labels();
    }

    fn doc(&mut self) -> Option<&mut Doc> {
        self.docs.get_mut(self.cur)
    }

    // ---- the engine -----------------------------------------------------

    /// Check again with the editor's text (open files are read from here).
    fn check(&mut self) {
        for d in &self.docs {
            if let Some(p) = &d.path
                && p.extension().is_some_and(|e| e == "mim")
            {
                sources::set_open(p, Some(d.text.to_string()));
            }
        }
        let Some(engine) = self.engine.as_mut() else { return };
        engine.check();
        self.problems.clear();
        if let Some(r) = engine.errors() {
            for e in &r.entries {
                let Some(src) = e.file.and_then(|f| r.files.get(f)) else { continue };
                let Some(path) = src.path.clone() else { continue };
                let (line, col) = (e.diag.span.line.max(1) as usize - 1, e.diag.span.col.max(1) as usize - 1);
                let (_, end_col) = mimic_syntax::line_col(&src.src, e.diag.span.end);
                let end_col = if e.diag.span.end > e.diag.span.start { end_col as usize - 1 } else { col + 1 };
                self.problems.push(Problem { path: path.canonicalize().unwrap_or(path), line, col, end_col: end_col.max(col + 1), message: e.diag.msg.clone(), code: e.diag.code.id.to_string() });
            }
        }
        if self.pane == Pane::Problems {
            self.show_problems();
        }
        self.refresh_labels();
    }

    fn refresh_labels(&mut self) {
        self.labels.clear();
        if !self.show_labels {
            return;
        }
        let Some(path) = self.docs.get(self.cur).and_then(|d| d.path.clone()) else { return };
        let Some(engine) = self.engine.as_mut() else { return };
        if engine.errors().is_some() {
            return;
        }
        let Ok(v) = engine.query("where", &json!({ "file": path.display().to_string() })) else { return };
        for d in v.as_array().into_iter().flatten() {
            let line = d["location"]["line"].as_u64().unwrap_or(1) as usize - 1;
            let mark = match d["kind"].as_str().unwrap_or("") {
                "parallel" => "∥ parallel",
                "net" => "∥ net",
                "gpu" => "▣ GPU",
                "task" => "◷ task",
                "ordered" => "→ in order",
                "waits" => "⏸ waits",
                "fused" => "⇶ fused",
                "simd" => "≋ SIMD",
                _ => continue,
            };
            match self.labels.iter_mut().find(|(l, _)| *l == line) {
                Some((_, t)) if !t.contains(mark) => *t += &format!(" · {mark}"),
                Some(_) => {}
                None => self.labels.push((line, mark.to_string())),
            }
        }
    }

    fn show_problems(&mut self) {
        self.pane_lines.clear();
        if self.problems.is_empty() {
            self.pane_lines.push(("No problems.".into(), Style::default().fg(MUTED)));
        }
        for p in &self.problems {
            let shown = p.path.strip_prefix(&self.root).unwrap_or(&p.path).display().to_string();
            self.pane_lines.push((format!("✗ {}:{}:{}  {} {}", shown, p.line + 1, p.col + 1, p.code, p.message), Style::default().fg(ERROR)));
        }
    }

    fn set_pane(&mut self, pane: Pane, lines: Vec<(String, Style)>) {
        self.panes.insert(pane, lines.clone());
        self.pane = pane;
        self.pane_lines = lines;
        self.pane_scroll = 0;
        self.show_panel = true;
    }

    /// Run `mimic` with `args` in the project, on another thread; the result
    /// lands in `pane`.
    fn spawn(&mut self, pane: Pane, args: Vec<String>, what: &str) {
        self.save_all();
        let (tx, rx) = mpsc::channel();
        let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("mimic"));
        let root = self.root.clone();
        std::thread::spawn(move || {
            let out = std::process::Command::new(exe).args(&args).current_dir(&root).env("NO_COLOR", "1").stdin(std::process::Stdio::null()).output();
            let text = match out {
                Ok(o) => format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr)),
                Err(e) => format!("cannot run mimic: {e}"),
            };
            let _ = tx.send((pane, text));
        });
        self.jobs = Some(rx);
        self.set_pane(pane, vec![(format!("{what}…"), Style::default().fg(MUTED))]);
    }

    /// The target of runs and tests: the project, or the current file.
    fn target_args(&self, cmd: &str) -> Vec<String> {
        let file = self.docs.get(self.cur).and_then(|d| d.path.clone());
        match file {
            Some(f) if !self.root.join("mim.mim").is_file() || !f.starts_with(self.root.join("src")) => vec![cmd.into(), f.display().to_string()],
            _ => vec![cmd.into()],
        }
    }

    fn poll_jobs(&mut self) {
        let Some(rx) = &self.jobs else { return };
        if let Ok((pane, text)) = rx.try_recv() {
            let lines = text
                .lines()
                .filter(|l| !l.trim_start().starts_with("Compiling") && !l.trim_start().starts_with("Finished"))
                .map(|l| {
                    let st = if l.contains("error") || l.contains('✗') || l.contains("panic") {
                        Style::default().fg(ERROR)
                    } else if l.contains('✓') {
                        Style::default().fg(STRING)
                    } else {
                        Style::default().fg(INK)
                    };
                    (l.to_string(), st)
                })
                .collect();
            self.set_pane(pane, lines);
            self.jobs = None;
        }
    }

    fn manual(&mut self, topic: &str) {
        let m = crate::man::Manual::load();
        let text = if topic.is_empty() { m.index() } else { m.find(topic).map(|p| p.text.clone()).unwrap_or_else(|_| format!("No page `{topic}`. Try F1 on another word, or the contents (F1 on nothing).")) };
        let lines = crate::man::render(&text, false).lines().map(|l| (l.to_string(), if l.starts_with("    ") { Style::default().fg(TYPE) } else { Style::default().fg(INK) })).collect();
        self.set_pane(Pane::Manual, lines);
    }

    fn show_where(&mut self) {
        let Some(engine) = self.engine.as_mut() else { return };
        let mut lines = Vec::new();
        match engine.query("where", &json!({})) {
            Ok(Value::Array(v)) if !v.is_empty() => {
                for d in v {
                    lines.push((format!("{} {}  line {}  {}", d["kind"].as_str().unwrap_or(""), d["function"].as_str().unwrap_or(""), d["location"]["line"], d["code"].as_str().unwrap_or("")), Style::default().fg(BRIGHT)));
                    lines.push((format!("    {}", d["why"].as_str().unwrap_or("")), Style::default().fg(MUTED)));
                }
            }
            Ok(_) => lines.push(("Nothing to decide: the program runs in order, as written.".into(), Style::default().fg(MUTED))),
            Err(e) => lines.push((e, Style::default().fg(ERROR))),
        }
        self.set_pane(Pane::Where, lines);
    }

    fn save(&mut self) {
        let Some(d) = self.docs.get_mut(self.cur) else { return };
        let Some(p) = d.path.clone() else {
            self.message = "this file has no name yet".into();
            return;
        };
        match std::fs::write(&p, d.text.to_string()) {
            Ok(()) => {
                d.dirty = false;
                self.message = format!("saved {}", d.name());
            }
            Err(e) => self.message = format!("cannot save: {e}"),
        }
    }

    fn save_all(&mut self) {
        for d in &mut self.docs {
            if d.dirty
                && let Some(p) = &d.path
                && std::fs::write(p, d.text.to_string()).is_ok()
            {
                d.dirty = false;
            }
        }
    }

    // ---- keys ----------------------------------------------------------

    /// Handle one key; this is the whole editor, without the terminal.
    pub fn key(&mut self, k: KeyEvent) {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        let shift = k.modifiers.contains(KeyModifiers::SHIFT);
        let alt = k.modifiers.contains(KeyModifiers::ALT);
        if !(ctrl && k.code == KeyCode::Char('q')) {
            self.quit_armed = false;
        }
        if self.prompt.is_some() {
            self.prompt_key(k);
            return;
        }
        self.message.clear();
        match (k.code, ctrl, alt) {
            (KeyCode::Char('q'), true, _) => {
                if self.docs.iter().any(|d| d.dirty) && !self.quit_armed {
                    self.quit_armed = true;
                    self.message = "unsaved changes: Ctrl-Q again to quit without saving, Ctrl-S to save".into();
                } else {
                    self.quit = true;
                }
            }
            (KeyCode::Char('s'), true, _) => self.save(),
            (KeyCode::Char('o'), true, _) | (KeyCode::Char('p'), true, _) => self.prompt = Some(Prompt { kind: PromptKind::Open, input: String::new(), selected: 0 }),
            (KeyCode::Char('f'), true, _) => self.prompt = Some(Prompt { kind: PromptKind::Find, input: self.last_find.clone(), selected: 0 }),
            (KeyCode::Char('h'), true, _) | (KeyCode::Char('r'), true, _) => self.prompt = Some(Prompt { kind: PromptKind::Replace, input: self.last_find.clone(), selected: 0 }),
            (KeyCode::Char('g'), true, _) => self.prompt = Some(Prompt { kind: PromptKind::Glyph, input: String::new(), selected: 0 }),
            (KeyCode::F(2), _, _) => self.prompt = Some(Prompt { kind: PromptKind::Command, input: String::new(), selected: 0 }),
            (KeyCode::Char('l'), true, _) => self.prompt = Some(Prompt { kind: PromptKind::GotoLine, input: String::new(), selected: 0 }),
            (KeyCode::Char('b'), true, _) => self.show_files = !self.show_files,
            (KeyCode::Char('j'), true, _) => self.show_panel = !self.show_panel,
            (KeyCode::Char('w'), true, _) => {
                if !self.docs.is_empty() {
                    self.docs.remove(self.cur);
                    self.cur = self.cur.min(self.docs.len().saturating_sub(1));
                    self.refresh_labels();
                }
            }
            (KeyCode::Char('n'), true, _) => {
                self.docs.push(Doc::new(None, "module main\n\nfn main() {\n    \n}\n"));
                self.cur = self.docs.len() - 1;
                if let Some(d) = self.doc() {
                    d.cursor = (3, 4);
                }
            }
            (KeyCode::Char('z'), true, _) => {
                if let Some(d) = self.doc() {
                    d.undo();
                }
                self.edited_at = Some(Instant::now());
            }
            (KeyCode::Char('y'), true, _) => {
                if let Some(d) = self.doc() {
                    d.redo();
                }
                self.edited_at = Some(Instant::now());
            }
            (KeyCode::Char('a'), true, _) => {
                if let Some(d) = self.doc() {
                    d.anchor = Some((0, 0));
                    let last = d.lines() - 1;
                    d.cursor = (last, d.line_len(last));
                }
            }
            (KeyCode::Char('c'), true, _) | (KeyCode::Char('x'), true, _) => {
                let cut = k.code == KeyCode::Char('x');
                if let Some(d) = self.docs.get_mut(self.cur) {
                    let text = match d.selection() {
                        Some((a, b)) => d.text.slice(a..b).to_string(),
                        None => format!("{}\n", d.line_text(d.cursor.0)),
                    };
                    if cut {
                        if d.selection().is_none() {
                            let l = d.cursor.0;
                            d.anchor = Some((l, 0));
                            d.cursor = (l + 1, 0);
                            if l + 1 >= d.lines() {
                                d.cursor = (l, d.line_len(l));
                            }
                        }
                        d.save_undo("cut");
                        d.delete_selection();
                        self.edited_at = Some(Instant::now());
                    }
                    self.clipboard = text;
                }
            }
            (KeyCode::Char('v'), true, _) => {
                let t = self.clipboard.clone();
                if let Some(d) = self.doc() {
                    d.insert(&t);
                }
                self.edited_at = Some(Instant::now());
            }
            (KeyCode::Char('t'), true, _) | (KeyCode::F(6), _, _) => {
                let args = self.target_args("test");
                self.spawn(Pane::Output, args, "testing");
            }
            (KeyCode::F(4), _, _) => {
                if !self.repl_log.is_empty() {
                    let lines = self.repl_log.clone();
                    self.set_pane(Pane::Repl, lines);
                    self.scroll_to_end();
                }
                self.prompt = Some(Prompt { kind: PromptKind::Eval, input: String::new(), selected: 0 });
            }
            (KeyCode::Char('e'), true, _) => {
                // the selection, or the current line
                let code = self.docs.get(self.cur).map(|d| match d.selection() {
                    Some((a, b)) => d.text.slice(a..b).to_string(),
                    None => d.line_text(d.cursor.0),
                });
                if let Some(c) = code {
                    self.eval(c);
                }
            }
            (KeyCode::F(5), _, _) => {
                let args = self.target_args("run");
                self.spawn(Pane::Output, args, "running");
            }
            (KeyCode::F(7), _, _) => {
                self.check();
                self.show_problems();
                self.pane = Pane::Problems;
                self.show_panel = true;
            }
            (KeyCode::F(8), _, _) => {
                let mut args = self.target_args("audit");
                if args.len() == 2 {
                    args = vec!["audit".into(), "--in".into(), args[1].clone()];
                }
                args.extend(["--fail-on".into(), "never".into()]);
                self.spawn(Pane::Audit, args, "auditing");
            }
            (KeyCode::F(9), _, _) => {
                self.show_labels = !self.show_labels;
                self.refresh_labels();
                self.show_where();
            }
            (KeyCode::F(1), _, _) => {
                let w = self.docs.get(self.cur).map(Doc::word).unwrap_or_default();
                self.manual(&w);
            }
            (KeyCode::F(3), _, _) => {
                let f = self.last_find.clone();
                self.find_next(&f);
            }
            (KeyCode::Right, _, true) | (KeyCode::Tab, true, _) => {
                if !self.docs.is_empty() {
                    self.cur = (self.cur + 1) % self.docs.len();
                    self.refresh_labels();
                }
            }
            (KeyCode::Left, _, true) | (KeyCode::BackTab, true, _) => {
                if !self.docs.is_empty() {
                    self.cur = (self.cur + self.docs.len() - 1) % self.docs.len();
                    self.refresh_labels();
                }
            }
            (KeyCode::PageUp, _, _) if alt => self.pane_scroll = self.pane_scroll.saturating_sub(5),
            (KeyCode::PageDown, _, _) if alt => self.pane_scroll += 5,
            _ => self.edit_key(k, shift),
        }
    }

    fn edit_key(&mut self, k: KeyEvent, shift: bool) {
        let page = self.area.height.saturating_sub(2).max(1) as usize;
        let Some(d) = self.docs.get_mut(self.cur) else {
            if let KeyCode::Char(c) = k.code {
                // the start screen: any letter opens a new program
                let _ = c;
            }
            return;
        };
        let (line, col) = d.cursor;
        let mut edited = true;
        match k.code {
            KeyCode::Char(c) => {
                d.insert(&c.to_string());
                // glyph input: `\i` becomes `⍳`
                let (l, cc) = d.cursor;
                if cc >= 2 {
                    let before: String = d.line_text(l).chars().skip(cc - 2).take(2).collect();
                    if let Some(g) = mimic_syntax::glyphs::digraph(&before)
                        && !inside_string(&d.line_text(l).chars().take(cc - 2).collect::<String>())
                    {
                        let at = d.char_at((l, cc));
                        d.text.remove(at - 2..at);
                        d.text.insert(at - 2, g);
                        d.cursor = d.pos_of(at - 2 + g.chars().count());
                    }
                }
            }
            KeyCode::Enter => d.newline(),
            KeyCode::Tab => d.insert("    "),
            KeyCode::Backspace => d.backspace(),
            KeyCode::Delete => d.delete(),
            KeyCode::Left => {
                edited = false;
                if col > 0 {
                    d.move_to((line, col - 1), shift)
                } else if line > 0 {
                    let l = d.line_len(line - 1);
                    d.move_to((line - 1, l), shift)
                }
            }
            KeyCode::Right => {
                edited = false;
                if col < d.line_len(line) {
                    d.move_to((line, col + 1), shift)
                } else if line + 1 < d.lines() {
                    d.move_to((line + 1, 0), shift)
                }
            }
            KeyCode::Up => {
                edited = false;
                d.move_to((line.saturating_sub(1), col), shift)
            }
            KeyCode::Down => {
                edited = false;
                d.move_to((line + 1, col), shift)
            }
            KeyCode::Home => {
                edited = false;
                let indent = d.line_text(line).chars().take_while(|c| *c == ' ').count();
                d.move_to((line, if col == indent { 0 } else { indent }), shift)
            }
            KeyCode::End => {
                edited = false;
                let l = d.line_len(line);
                d.move_to((line, l), shift)
            }
            KeyCode::PageUp => {
                edited = false;
                d.move_to((line.saturating_sub(page), col), shift)
            }
            KeyCode::PageDown => {
                edited = false;
                d.move_to((line + page, col), shift)
            }
            KeyCode::Esc => {
                edited = false;
                d.anchor = None;
            }
            _ => edited = false,
        }
        if edited {
            self.edited_at = Some(Instant::now());
        }
    }

    fn find_next(&mut self, needle: &str) {
        if needle.is_empty() {
            return;
        }
        let Some(d) = self.docs.get_mut(self.cur) else { return };
        let text = d.text.to_string();
        let start = d.text.char_to_byte(d.char_at(d.cursor)) + 1;
        let hit = text.get(start.min(text.len())..).and_then(|t| t.find(needle)).map(|i| start + i).or_else(|| text.find(needle));
        match hit {
            Some(b) => {
                let ch = d.text.byte_to_char(b);
                let end = d.pos_of(ch + needle.chars().count());
                d.anchor = Some(d.pos_of(ch));
                d.cursor = end;
            }
            None => self.message = format!("`{needle}` is not in this file"),
        }
    }

    fn replace_all(&mut self, needle: &str, with: &str) {
        let Some(d) = self.docs.get_mut(self.cur) else { return };
        let text = d.text.to_string();
        let n = text.matches(needle).count();
        if n == 0 {
            self.message = format!("`{needle}` is not in this file");
            return;
        }
        d.save_undo("replace");
        d.text = Rope::from_str(&text.replace(needle, with));
        let c = d.cursor;
        d.move_to(c, false);
        self.message = format!("replaced {n}");
        self.edited_at = Some(Instant::now());
    }

    /// The choices a list prompt shows: (label, detail, value).
    fn prompt_items(&self) -> Vec<(String, String, String)> {
        let Some(p) = &self.prompt else { return Vec::new() };
        match p.kind {
            PromptKind::Open => {
                let mut v: Vec<(usize, String, String)> = self
                    .files
                    .iter()
                    .filter_map(|f| {
                        let shown = f.strip_prefix(&self.root).unwrap_or(f).display().to_string();
                        fuzzy(&p.input, &shown).map(|s| (s, shown, f.display().to_string()))
                    })
                    .collect();
                v.sort();
                v.into_iter().take(200).map(|(_, s, f)| (s, String::new(), f)).collect()
            }
            PromptKind::Glyph => {
                let mut v: Vec<(usize, (String, String, String))> = glyph_table()
                    .into_iter()
                    .filter_map(|(g, w, d, doc)| {
                        let label = format!("{g}  {w}");
                        let hay = format!("{w} {} {doc}", d.unwrap_or(""));
                        fuzzy(&p.input, &hay).map(|s| (s, (label, format!("{}  {doc}", d.map(|x| format!("type {x}")).unwrap_or_default()), g.to_string())))
                    })
                    .collect();
                v.sort_by_key(|(s, _)| *s);
                v.into_iter().map(|(_, x)| x).collect()
            }
            PromptKind::Command => {
                let mut v: Vec<(usize, (String, String, String))> = COMMANDS
                    .iter()
                    .enumerate()
                    .filter_map(|(i, (_, label, key))| fuzzy(&p.input, &format!("{label} {key}")).map(|s| (s, (label.to_string(), key.to_string(), i.to_string()))))
                    .collect();
                v.sort_by_key(|(s, _)| *s);
                v.into_iter().map(|(_, x)| x).collect()
            }
            _ => Vec::new(),
        }
    }

    fn prompt_key(&mut self, k: KeyEvent) {
        let items = self.prompt_items();
        let Some(p) = self.prompt.as_mut() else { return };
        match k.code {
            KeyCode::Esc => self.prompt = None,
            KeyCode::Backspace => {
                p.input.pop();
                p.selected = 0;
            }
            KeyCode::Up => p.selected = p.selected.saturating_sub(1),
            KeyCode::Down => p.selected = (p.selected + 1).min(items.len().saturating_sub(1)),
            KeyCode::Char(c) if !k.modifiers.contains(KeyModifiers::CONTROL) => {
                p.input.push(c);
                p.selected = 0;
                // glyph input in the REPL's line, as in the code
                if p.kind == PromptKind::Eval {
                    let n = p.input.chars().count();
                    if n >= 2 {
                        let two: String = p.input.chars().skip(n - 2).collect();
                        let before: String = p.input.chars().take(n - 2).collect();
                        if let Some(g) = mimic_syntax::glyphs::digraph(&two)
                            && !inside_string(&before)
                        {
                            p.input = before + g;
                        }
                    }
                }
            }
            KeyCode::Enter => {
                let p = self.prompt.take().unwrap();
                match p.kind {
                    PromptKind::Find => {
                        self.last_find = p.input.clone();
                        self.find_next(&p.input);
                    }
                    PromptKind::Replace => {
                        self.last_find = p.input.clone();
                        self.prompt = Some(Prompt { kind: PromptKind::ReplaceWith(p.input), input: String::new(), selected: 0 });
                    }
                    PromptKind::ReplaceWith(needle) => self.replace_all(&needle, &p.input),
                    PromptKind::Eval => self.eval(p.input),
                    PromptKind::GotoLine => {
                        if let Ok(n) = p.input.trim().parse::<usize>()
                            && let Some(d) = self.doc()
                        {
                            d.move_to((n.saturating_sub(1), 0), false);
                        }
                    }
                    PromptKind::Open => {
                        if let Some((_, _, f)) = items.get(p.selected) {
                            self.open_file(Path::new(f));
                        }
                    }
                    PromptKind::Command => {
                        if let Some((_, _, i)) = items.get(p.selected)
                            && let Some((cmd, ..)) = i.parse::<usize>().ok().and_then(|i| COMMANDS.get(i))
                        {
                            self.key(cmd.key());
                        }
                    }
                    PromptKind::Glyph => {
                        if let Some((_, _, g)) = items.get(p.selected) {
                            let g = g.clone();
                            if let Some(d) = self.doc() {
                                d.insert(&g);
                            }
                            self.edited_at = Some(Instant::now());
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn mouse(&mut self, m: MouseEvent) {
        let a = self.area;
        let inside = |r: Rect| m.column >= r.x && m.column < r.x + r.width && m.row >= r.y && m.row < r.y + r.height;
        match m.kind {
            // the wheel scrolls what is under it
            MouseEventKind::ScrollDown if self.show_files && inside(self.tree_area) => self.tree_top += 3,
            MouseEventKind::ScrollUp if self.show_files && inside(self.tree_area) => self.tree_top = self.tree_top.saturating_sub(3),
            MouseEventKind::ScrollDown if self.show_panel && inside(self.panel_area) => {
                self.pane_scroll = (self.pane_scroll + 3).min(self.pane_lines.len().saturating_sub(1));
            }
            MouseEventKind::ScrollUp if self.show_panel && inside(self.panel_area) => self.pane_scroll = self.pane_scroll.saturating_sub(3),
            MouseEventKind::ScrollDown => {
                if let Some(d) = self.doc() {
                    d.top = (d.top + 3).min(d.lines().saturating_sub(1));
                }
            }
            MouseEventKind::ScrollUp => {
                if let Some(d) = self.doc() {
                    d.top = d.top.saturating_sub(3);
                }
            }
            MouseEventKind::Down(MouseButton::Left) if self.click(m.column, m.row) => {}
            MouseEventKind::Down(MouseButton::Left) | MouseEventKind::Drag(MouseButton::Left) if self.prompt.is_none() => {
                let drag = matches!(m.kind, MouseEventKind::Drag(_));
                if m.column >= a.x + self.gutter && m.column < a.x + a.width && m.row >= a.y && m.row < a.y + a.height {
                    let gutter = self.gutter;
                    if let Some(d) = self.doc() {
                        let line = d.top + (m.row - a.y) as usize;
                        let target = (m.column - a.x - gutter) as usize + d.left;
                        // a screen column back to a character (glyphs may be wide)
                        let text = d.line_text(line.min(d.lines() - 1));
                        let (mut w, mut col) = (0, 0);
                        for ch in text.chars() {
                            let cw = ch.width().unwrap_or(1);
                            if w + cw > target {
                                break;
                            }
                            w += cw;
                            col += 1;
                        }
                        d.move_to((line, col), drag);
                    }
                }
            }
            _ => {}
        }
    }

    /// A click on something drawn: true if it was a button, a tab, a row…
    fn click(&mut self, x: u16, y: u16) -> bool {
        let hit = {
            let hits = self.hits.borrow();
            // the last drawn is on top (a pop-up list over the rest)
            hits.iter().rev().find(|(r, _)| x >= r.x && x < r.x + r.width && y >= r.y && y < r.y + r.height).map(|(_, h)| h.clone())
        };
        let Some(hit) = hit else {
            // a click outside an open list closes it
            if self.prompt.is_some() {
                self.prompt = None;
                return true;
            }
            return false;
        };
        if self.prompt.is_some() && !matches!(hit, Hit::PromptItem(_)) {
            self.prompt = None;
        }
        match hit {
            Hit::PromptItem(i) => {
                if let Some(p) = self.prompt.as_mut() {
                    p.selected = i;
                }
                self.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
            }
            Hit::Button(cmd) => self.key(cmd.key()),
            Hit::Tab(i) => {
                self.cur = i.min(self.docs.len().saturating_sub(1));
                self.refresh_labels();
            }
            Hit::TabClose(i) => {
                if self.docs.get(i).is_some_and(|d| d.dirty) {
                    self.cur = i;
                    self.message = format!("{} has unsaved changes: Ctrl-S to save, or Ctrl-W to close without saving", self.docs[i].name());
                } else if i < self.docs.len() {
                    self.docs.remove(i);
                    self.cur = self.cur.min(self.docs.len().saturating_sub(1));
                    self.refresh_labels();
                }
            }
            Hit::Tree(i) => {
                if let Some(row) = self.tree_rows().get(i).cloned() {
                    if row.dir {
                        if !self.collapsed.remove(&row.path) {
                            self.collapsed.insert(row.path);
                        }
                    } else {
                        self.open_file(&row.path);
                    }
                }
            }
            Hit::Pane(p) => {
                self.show_panel = true;
                match p {
                    Pane::Problems => self.key(Cmd::Check.key()),
                    Pane::Where => self.show_where(),
                    Pane::Repl => {
                        let lines = if self.repl_log.is_empty() { vec![("F4 or Ctrl-E evaluates a line here.".to_string(), Style::default().fg(MUTED))] } else { self.repl_log.clone() };
                        self.set_pane(Pane::Repl, lines);
                    }
                    other => {
                        let lines = self.panes.get(&other).cloned().unwrap_or_else(|| {
                            let how = match other {
                                Pane::Audit => "F8 (or ⚑ Audit) audits the project.",
                                Pane::Manual => "F1 on a word (or ? Manual) shows its page here.",
                                _ => "F5 (or ▶ Run) runs the program, F6 the tests; the output comes here.",
                            };
                            vec![(how.to_string(), Style::default().fg(MUTED))]
                        });
                        self.set_pane(other, lines);
                    }
                }
            }
            Hit::PaneLine(i) => {
                if let Some((file, line, col)) = self.pane_lines.get(i).and_then(|(t, _)| file_line(t)) {
                    let path = [self.root.join(&file), PathBuf::from(&file)].into_iter().find(|p| p.is_file());
                    match path {
                        Some(p) => {
                            self.open_file(&p);
                            if let Some(d) = self.doc() {
                                d.move_to((line.saturating_sub(1), col.saturating_sub(1)), false);
                                d.top = line.saturating_sub(6);
                            }
                        }
                        None => self.message = format!("{file} is not a file here"),
                    }
                }
            }
            Hit::Errors => self.key(Cmd::Check.key()),
        }
        true
    }

    /// Check again a moment after the last edit.
    fn tick(&mut self) {
        if let Some(t) = self.edited_at
            && t.elapsed() > Duration::from_millis(350)
        {
            self.edited_at = None;
            self.check();
        }
        self.poll_jobs();
        self.poll_repl();
    }

    /// Evaluate `code` in the REPL's session, on another thread.
    fn eval(&mut self, code: String) {
        let code = code.trim().to_string();
        if code.is_empty() {
            return;
        }
        let Some(mut session) = self.repl.take().or_else(|| self.repl_job.is_none().then(crate::repl::Session::new)) else {
            self.message = "the REPL is still busy with the last line".into();
            return;
        };
        for (i, l) in code.lines().enumerate() {
            self.repl_log.push((format!("{} {l}", if i == 0 { "›" } else { "…" }), Style::default().fg(ORANGE)));
        }
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let r = session.eval(&code);
            let _ = tx.send((session, r));
        });
        self.repl_job = Some(rx);
        let mut lines = self.repl_log.clone();
        lines.push(("…".into(), Style::default().fg(MUTED)));
        self.set_pane(Pane::Repl, lines);
        self.scroll_to_end();
    }

    fn poll_repl(&mut self) {
        let Some(rx) = &self.repl_job else { return };
        let Ok((session, r)) = rx.try_recv() else { return };
        self.repl = Some(session);
        self.repl_job = None;
        match r {
            Ok(out) => self.repl_log.extend(out.lines().map(|l| (l.to_string(), Style::default().fg(INK)))),
            Err(e) => self.repl_log.extend(e.lines().map(|l| (l.to_string(), Style::default().fg(ERROR)))),
        }
        if self.pane == Pane::Repl {
            let lines = self.repl_log.clone();
            self.set_pane(Pane::Repl, lines);
            self.scroll_to_end();
        }
    }

    /// The bottom pane scrolled to its last lines.
    fn scroll_to_end(&mut self) {
        self.pane_scroll = self.pane_lines.len().saturating_sub(8);
    }

    // ---- drawing --------------------------------------------------------------

    pub fn draw(&mut self, f: &mut Frame) {
        self.hits.borrow_mut().clear();
        let full = f.area();
        f.render_widget(Block::default().style(Style::default().bg(BG)), full);
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(1), Constraint::Min(3), Constraint::Length(if self.show_panel { 12 } else { 0 }), Constraint::Length(1)])
            .split(full);
        self.draw_tabs(f, rows[0]);
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(if self.show_files { 26 } else { 0 }), Constraint::Min(10)])
            .split(rows[1]);
        if self.show_files {
            self.draw_files(f, cols[0]);
        }
        if self.docs.is_empty() {
            self.draw_start(f, cols[1]);
        } else {
            self.draw_doc(f, cols[1]);
        }
        if self.show_panel {
            self.draw_panel(f, rows[2]);
        }
        self.draw_status(f, rows[3]);
        if self.prompt.is_some() {
            self.draw_prompt(f, full);
        }
    }

    fn draw_tabs(&self, f: &mut Frame, area: Rect) {
        let mut hits = self.hits.borrow_mut();
        // the toolbar on the right: a click runs the command
        let buttons: [(Cmd, &str); 9] = [
            (Cmd::Run, "▶ Run"),
            (Cmd::Test, "✓ Test"),
            (Cmd::Check, "● Check"),
            (Cmd::Audit, "⚑ Audit"),
            (Cmd::Find, "⌕ Find"),
            (Cmd::Glyphs, "⍳ Glyphs"),
            (Cmd::Repl, "› REPL"),
            (Cmd::Manual, "? Manual"),
            (Cmd::Commands, "≡ Commands"),
        ];
        // full labels when there is room, else the icons alone
        let full_w: u16 = buttons.iter().map(|(_, t)| t.chars().count() as u16 + 3).sum();
        let compact = area.width <= full_w + 40;
        let label = |t: &str| -> String { if compact { t.chars().next().unwrap_or(' ').to_string() } else { t.to_string() } };
        let bar_w: u16 = buttons.iter().map(|(_, t)| label(t).chars().count() as u16 + 3).sum();
        let show_bar = area.width > bar_w + 30;
        let mut spans = vec![Span::styled(" MIMIC ", Style::default().fg(BG).bg(ORANGE).add_modifier(Modifier::BOLD)), Span::raw(" ")];
        hits.push((Rect { width: 7, height: 1, ..area }, Hit::Button(Cmd::Commands)));
        let mut x = area.x + 8;
        let limit = if show_bar { area.x + area.width - bar_w - 1 } else { area.x + area.width };
        for (i, d) in self.docs.iter().enumerate() {
            let label = format!(" {}{} ", d.name(), if d.dirty { " •" } else { "" });
            let w = label.chars().count() as u16 + 2;
            if x + w > limit {
                spans.push(Span::styled(" … ", Style::default().fg(MUTED)));
                break;
            }
            let st = if i == self.cur { Style::default().fg(INK).bg(LINE).add_modifier(Modifier::BOLD) } else { Style::default().fg(MUTED) };
            spans.push(Span::styled(label.clone(), st));
            spans.push(Span::styled("× ", Style::default().fg(MUTED).bg(if i == self.cur { LINE } else { PANEL })));
            let lw = label.chars().count() as u16;
            hits.push((Rect { x, y: area.y, width: lw, height: 1 }, Hit::Tab(i)));
            hits.push((Rect { x: x + lw, y: area.y, width: 2, height: 1 }, Hit::TabClose(i)));
            x += w;
        }
        f.render_widget(Paragraph::new(Line::from(spans)).style(Style::default().bg(PANEL)), area);
        if show_bar {
            let mut bx = area.x + area.width - bar_w;
            let mut bar = Vec::new();
            for (cmd, text) in buttons {
                let text = label(text);
                let w = text.chars().count() as u16 + 2;
                let st = if cmd == Cmd::Run { Style::default().fg(BG).bg(ORANGE) } else { Style::default().fg(INK).bg(LINE) };
                bar.push(Span::styled(format!(" {text} "), st));
                bar.push(Span::raw(" "));
                hits.push((Rect { x: bx, y: area.y, width: w, height: 1 }, Hit::Button(cmd)));
                bx += w + 1;
            }
            f.render_widget(Paragraph::new(Line::from(bar)), Rect { x: area.x + area.width - bar_w, width: bar_w, ..area });
        }
    }

    /// The file tree's rows: folders (open or closed) and the files of open
    /// folders, folders first.
    fn tree_rows(&self) -> Vec<Row> {
        let mut files: Vec<(Vec<String>, &PathBuf)> = self.files.iter().map(|p| (p.strip_prefix(&self.root).unwrap_or(p).iter().map(|c| c.to_string_lossy().to_string()).collect(), p)).collect();
        // folders before files at each level
        files.sort_by(|a, b| {
            let key = |parts: &Vec<String>| parts.iter().enumerate().map(|(i, c)| (i + 1 == parts.len(), c.to_lowercase())).collect::<Vec<_>>();
            key(&a.0).cmp(&key(&b.0))
        });
        let mut rows = Vec::new();
        let mut seen = std::collections::HashSet::new();
        'file: for (parts, path) in files {
            let mut dir = self.root.clone();
            for (depth, part) in parts.iter().enumerate().take(parts.len().saturating_sub(1)) {
                dir = dir.join(part);
                if seen.insert(dir.clone()) {
                    rows.push(Row { depth, path: dir.clone(), dir: true, name: part.clone() });
                }
                if self.collapsed.contains(&dir) {
                    continue 'file;
                }
            }
            rows.push(Row { depth: parts.len().saturating_sub(1), path: path.clone(), dir: false, name: parts.last().cloned().unwrap_or_default() });
        }
        rows
    }

    fn draw_files(&mut self, f: &mut Frame, area: Rect) {
        self.tree_area = area;
        let cur = self.docs.get(self.cur).and_then(|d| d.path.clone());
        let rows = self.tree_rows();
        let title = self.root.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let block = Block::default()
            .borders(Borders::RIGHT)
            .border_style(Style::default().fg(LINE))
            .title(Span::styled(format!(" {title} "), Style::default().fg(MUTED)))
            .style(Style::default().bg(PANEL));
        let inner = block.inner(area);
        f.render_widget(block, area);
        let height = inner.height.saturating_sub(1) as usize;
        self.tree_top = self.tree_top.min(rows.len().saturating_sub(height.max(1)));
        let mut lines = Vec::new();
        let mut hits = self.hits.borrow_mut();
        for (k, (i, row)) in rows.iter().enumerate().skip(self.tree_top).take(height).enumerate() {
            let indent = "  ".repeat(row.depth);
            let line = if row.dir {
                let open = !self.collapsed.contains(&row.path);
                Line::from(vec![Span::raw(format!(" {indent}")), Span::styled(if open { "▾ " } else { "▸ " }, Style::default().fg(ORANGE)), Span::styled(format!("{}/", row.name), Style::default().fg(TYPE))])
            } else {
                let has_problem = self.problems.iter().any(|x| x.path == row.path);
                let st = if cur.as_deref() == Some(row.path.as_path()) {
                    Style::default().fg(BRIGHT).add_modifier(Modifier::BOLD)
                } else if has_problem {
                    Style::default().fg(ERROR)
                } else {
                    Style::default().fg(INK)
                };
                let icon = if row.name.ends_with(".mim") { "◆ " } else if row.name.ends_with(".mnet") { "◇ " } else { "· " };
                Line::from(vec![Span::raw(format!(" {indent}  ")), Span::styled(icon, Style::default().fg(if row.name.ends_with(".mim") { ORANGE } else { MUTED })), Span::styled(row.name.clone(), st)])
            };
            lines.push(line);
            hits.push((Rect { x: inner.x, y: inner.y + 1 + k as u16, width: inner.width, height: 1 }, Hit::Tree(i)));
        }
        f.render_widget(Paragraph::new(lines), Rect { y: inner.y + 1, height: inner.height.saturating_sub(1), ..inner });
    }

    fn draw_start(&self, f: &mut Frame, area: Rect) {
        let o = Style::default().fg(ORANGE).add_modifier(Modifier::BOLD);
        let ink = Style::default().fg(INK);
        let muted = Style::default().fg(MUTED);
        let key = |k: &str, what: &str| Line::from(vec![Span::styled(format!("  {k:<12}"), Style::default().fg(BRIGHT)), Span::styled(what.to_string(), ink)]);
        // the keys' lines are buttons (rows 10 to 18 below)
        {
            let mut hits = self.hits.borrow_mut();
            for (row, cmd) in [Cmd::Open, Cmd::New, Cmd::Glyphs, Cmd::Run, Cmd::Repl, Cmd::Check, Cmd::Where, Cmd::Manual, Cmd::Save].into_iter().enumerate() {
                hits.push((Rect { x: area.x, y: area.y + 10 + row as u16, width: area.width.min(70), height: 1 }, Hit::Button(cmd)));
            }
            hits.push((Rect { x: area.x, y: area.y + 19, width: area.width.min(70), height: 1 }, Hit::Button(Cmd::Commands)));
        }
        let lines = vec![
            Line::from(""),
            Line::from(Span::styled("  ███╗   ███╗██╗███╗   ███╗██╗ ██████╗", o)),
            Line::from(Span::styled("  ████╗ ████║██║████╗ ████║██║██╔════╝", o)),
            Line::from(Span::styled("  ██╔████╔██║██║██╔████╔██║██║██║     ", o)),
            Line::from(Span::styled("  ██║╚██╔╝██║██║██║╚██╔╝██║██║██║     ", o)),
            Line::from(Span::styled("  ██║ ╚═╝ ██║██║██║ ╚═╝ ██║██║╚██████╗", o)),
            Line::from(Span::styled("  ╚═╝     ╚═╝╚═╝╚═╝     ╚═╝╚═╝ ╚═════╝", o)),
            Line::from(""),
            Line::from(vec![Span::styled("  Everything that can run in parallel ", ink), Span::styled("will", Style::default().fg(ORANGE)), Span::styled(" run in parallel.", ink)]),
            Line::from(""),
            key("Ctrl-O", "open a file of the project"),
            key("Ctrl-N", "a new program"),
            key("Ctrl-G", "glyph palette (or type \\i for ⍳)"),
            key("F5  F6", "run, test"),
            key("F4", "evaluate a line (the REPL); Ctrl-E the line or selection"),
            key("F7  F8", "problems, audit"),
            key("F9", "where code runs"),
            key("F1", "the manual for the word under the cursor"),
            key("Ctrl-S", "save          Ctrl-Q  quit"),
            key("F2", "every command (or click ≡ Commands, or anything here)"),
            Line::from(""),
            Line::from(Span::styled(format!("  mimic {}", env!("CARGO_PKG_VERSION")), muted)),
        ];
        f.render_widget(Paragraph::new(lines).style(Style::default().bg(BG)), area);
    }

    fn draw_doc(&mut self, f: &mut Frame, area: Rect) {
        self.area = area;
        let Some(d) = self.docs.get_mut(self.cur) else { return };
        let height = area.height as usize;
        let digits = d.lines().to_string().len().max(3);
        self.gutter = digits as u16 + 3;
        let width = (area.width as usize).saturating_sub(self.gutter as usize);
        // keep the cursor in view
        if d.cursor.0 < d.top {
            d.top = d.cursor.0;
        }
        if d.cursor.0 >= d.top + height {
            d.top = d.cursor.0 + 1 - height;
        }
        let cur_text = d.line_text(d.cursor.0);
        let cur_w: usize = cur_text.chars().take(d.cursor.1).map(|c| c.width().unwrap_or(1)).sum();
        if cur_w < d.left {
            d.left = cur_w;
        }
        if cur_w >= d.left + width.max(1) {
            d.left = cur_w + 1 - width.max(1);
        }
        let text = d.text.to_string();
        let styles = highlight(&text);
        let sel = d.selection();
        let path = d.path.clone();
        let mut lines = Vec::new();
        for row in 0..height {
            let ln = d.top + row;
            if ln >= d.lines() {
                lines.push(Line::from(Span::styled(format!("{:>w$}", "~", w = digits + 1), Style::default().fg(LINE))));
                continue;
            }
            let problem = self.problems.iter().find(|p| Some(&p.path) == path.as_ref() && p.line == ln);
            let mark = if problem.is_some() { Span::styled("● ", Style::default().fg(ERROR)) } else { Span::raw("  ") };
            let num_st = if ln == d.cursor.0 { Style::default().fg(INK) } else { Style::default().fg(COMMENT) };
            let mut spans = vec![Span::styled(format!("{:>digits$} ", ln + 1), num_st), mark];
            let line_start_char = d.text.line_to_char(ln);
            let line_start_byte = d.text.char_to_byte(line_start_char);
            let chars: Vec<char> = d.line_text(ln).chars().collect();
            let (mut w, mut byte) = (0usize, line_start_byte);
            for (i, ch) in chars.iter().enumerate() {
                let cw = ch.width().unwrap_or(1);
                let mut st = styles.get(&byte).copied().unwrap_or(Style::default().fg(INK));
                byte += ch.len_utf8();
                if let Some((a, b)) = sel
                    && line_start_char + i >= a
                    && line_start_char + i < b
                {
                    st = st.bg(DEEP);
                }
                if let Some(p) = problem
                    && i >= p.col
                    && i < p.end_col
                {
                    st = st.add_modifier(Modifier::UNDERLINED).underline_color(ERROR);
                }
                if w >= d.left && w + cw <= d.left + width {
                    spans.push(Span::styled(ch.to_string(), st));
                }
                w += cw;
            }
            if self.show_labels
                && let Some((_, label)) = self.labels.iter().find(|(l, _)| *l == ln)
            {
                spans.push(Span::styled(format!("   {label}"), Style::default().fg(DEEP)));
            }
            if let Some(p) = problem
                && ln != d.cursor.0
            {
                spans.push(Span::styled(format!("   {}", p.message), Style::default().fg(ERROR)));
            }
            lines.push(Line::from(spans));
        }
        f.render_widget(Paragraph::new(lines).style(Style::default().bg(BG)), area);
        let cx = area.x + self.gutter + (cur_w - d.left) as u16;
        let cy = area.y + (d.cursor.0 - d.top) as u16;
        if self.prompt.is_none() {
            f.set_cursor_position((cx.min(area.x + area.width.saturating_sub(1)), cy));
        }
    }

    fn draw_panel(&mut self, f: &mut Frame, area: Rect) {
        self.panel_area = area;
        let mut hits = self.hits.borrow_mut();
        let mut x = area.x;
        let titles: Vec<Span> = PANES
            .iter()
            .flat_map(|p| {
                let st = if *p == self.pane { Style::default().fg(BG).bg(ORANGE).add_modifier(Modifier::BOLD) } else { Style::default().fg(MUTED) };
                let w = p.title().chars().count() as u16 + 2;
                hits.push((Rect { x, y: area.y, width: w, height: 1 }, Hit::Pane(*p)));
                x += w + 1;
                [Span::styled(format!(" {} ", p.title()), st), Span::raw(" ")]
            })
            .collect();
        let block = Block::default().borders(Borders::TOP).border_style(Style::default().fg(LINE)).title(Line::from(titles)).style(Style::default().bg(PANEL));
        let inner = block.inner(area);
        f.render_widget(block, area);
        let lines: Vec<Line> = self
            .pane_lines
            .iter()
            .enumerate()
            .skip(self.pane_scroll)
            .take(inner.height as usize)
            .map(|(i, (t, s))| {
                hits.push((Rect { x: inner.x, y: inner.y + (i - self.pane_scroll) as u16, width: inner.width, height: 1 }, Hit::PaneLine(i)));
                // a file and line to jump to: underlined
                let st = if file_line(t).is_some() { s.add_modifier(Modifier::UNDERLINED) } else { *s };
                Line::from(Span::styled(t.clone(), st))
            })
            .collect();
        f.render_widget(Paragraph::new(lines), inner);
    }

    fn draw_status(&self, f: &mut Frame, area: Rect) {
        let mut hits = self.hits.borrow_mut();
        let (errors, place) = match self.docs.get(self.cur) {
            Some(d) => (self.problems.len(), format!("Ln {}, Col {}", d.cursor.0 + 1, d.cursor.1 + 1)),
            None => (self.problems.len(), String::new()),
        };
        let (err_text, err_style) = if errors == 0 { (" ✓ no errors ".to_string(), Style::default().fg(STRING)) } else { (format!(" ✗ {errors} error(s) "), Style::default().fg(ERROR)) };
        let right_w = (err_text.chars().count() + place.chars().count() + 2) as u16;
        let room = area.width.saturating_sub(right_w + 2);
        let mut spans = Vec::new();
        let mut x = area.x;
        if self.message.is_empty() {
            // the hints are buttons too
            for (cmd, text) in [(Cmd::Commands, "F2 commands"), (Cmd::Glyphs, "Ctrl-G glyphs"), (Cmd::Manual, "F1 manual"), (Cmd::Repl, "F4 REPL"), (Cmd::Run, "F5 run"), (Cmd::Test, "F6 test"), (Cmd::Save, "Ctrl-S save"), (Cmd::Quit, "Ctrl-Q quit")] {
                let w = text.chars().count() as u16 + 2;
                if x + w > area.x + room {
                    break;
                }
                spans.push(Span::styled(format!(" {text} "), Style::default().fg(MUTED)));
                hits.push((Rect { x, y: area.y, width: w, height: 1 }, Hit::Button(cmd)));
                x += w;
            }
        } else {
            let msg: String = format!(" {}", self.message).chars().take(room as usize).collect();
            x += msg.chars().count() as u16;
            spans.push(Span::styled(msg, Style::default().fg(INK)));
        }
        spans.push(Span::raw(" ".repeat((area.x + room).saturating_sub(x) as usize + 2)));
        hits.push((Rect { x: area.x + room + 2, y: area.y, width: err_text.chars().count() as u16, height: 1 }, Hit::Errors));
        spans.push(Span::styled(err_text, err_style));
        spans.push(Span::styled(format!(" {place} "), Style::default().fg(INK)));
        f.render_widget(Paragraph::new(Line::from(spans)).style(Style::default().bg(PANEL)), area);
    }

    fn draw_prompt(&self, f: &mut Frame, full: Rect) {
        let Some(p) = &self.prompt else { return };
        let title = match &p.kind {
            PromptKind::Find => " Find ".to_string(),
            PromptKind::Replace => " Replace: find ".to_string(),
            PromptKind::ReplaceWith(n) => format!(" Replace `{n}` with "),
            PromptKind::Open => " Open a file ".to_string(),
            PromptKind::Glyph => " Glyphs: type a word or what it does ".to_string(),
            PromptKind::GotoLine => " Go to line ".to_string(),
            PromptKind::Eval => " Evaluate: an expression, a let or a fn (the REPL keeps them) ".to_string(),
            PromptKind::Command => " Commands: type part of a name, or click one ".to_string(),
        };
        let items = self.prompt_items();
        let h = if items.is_empty() { 3 } else { (items.len() as u16 + 3).min(18) };
        let w = (full.width * 2 / 3).max(40).min(full.width);
        let area = Rect { x: full.x + (full.width - w) / 2, y: full.y + 2, width: w, height: h.min(full.height) };
        f.render_widget(Clear, area);
        let block = Block::default().borders(Borders::ALL).border_style(Style::default().fg(ORANGE)).title(Span::styled(title, Style::default().fg(BRIGHT))).style(Style::default().bg(PANEL));
        let inner = block.inner(area);
        f.render_widget(block, area);
        f.render_widget(Paragraph::new(Line::from(vec![Span::styled("› ", Style::default().fg(ORANGE)), Span::styled(p.input.clone(), Style::default().fg(INK))])), Rect { height: 1, ..inner });
        f.set_cursor_position((inner.x + 2 + p.input.chars().map(|c| c.width().unwrap_or(1)).sum::<usize>() as u16, inner.y));
        if !items.is_empty() {
            let list: Vec<ListItem> = items
                .iter()
                .map(|(label, detail, _)| ListItem::new(Line::from(vec![Span::styled(format!("{label:<28}"), Style::default().fg(INK)), Span::styled(detail.clone(), Style::default().fg(MUTED))])))
                .collect();
            let mut state = ListState::default();
            state.select(Some(p.selected));
            let list = List::new(list).highlight_style(Style::default().bg(DEEP).fg(INK));
            let at = Rect { y: inner.y + 1, height: inner.height.saturating_sub(1), ..inner };
            f.render_stateful_widget(list, at, &mut state);
            // each item is a click target (the list scrolls to keep the selection in view)
            let offset = state.offset();
            let mut hits = self.hits.borrow_mut();
            for (row, i) in (offset..items.len()).enumerate().take(at.height as usize) {
                hits.push((Rect { y: at.y + row as u16, height: 1, ..at }, Hit::PromptItem(i)));
            }
        }
    }
}

/// A file and line named in a line of output: `src/main.mim:12`,
/// `src/main.mim:12:5`, `--> file.mim:3:9`, `(file.mim, line 4)`.
fn file_line(text: &str) -> Option<(String, usize, usize)> {
    let bytes: Vec<char> = text.chars().collect();
    let s: String = bytes.iter().collect();
    for (i, _) in s.match_indices(".mim") {
        let end = i + 4;
        let rest = &s[end..];
        let Some(rest) = rest.strip_prefix(':') else { continue };
        let line: String = rest.chars().take_while(char::is_ascii_digit).collect();
        if line.is_empty() {
            continue;
        }
        let after = &rest[line.len()..];
        let col: String = after.strip_prefix(':').map(|c| c.chars().take_while(char::is_ascii_digit).collect()).unwrap_or_default();
        // the path: back to a space, a bracket or a quote
        let start = s[..i].rfind(|c: char| c.is_whitespace() || "([\"'`".contains(c)).map_or(0, |k| k + 1);
        let file = s[start..end].to_string();
        return Some((file, line.parse().ok()?, col.parse().unwrap_or(1)));
    }
    None
}

/// Is the end of `before` (one line) inside a string?
pub(crate) fn inside_string(before: &str) -> bool {
    let mut inside = false;
    let mut chars = before.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' if inside => {
                chars.next();
            }
            '"' => inside = !inside,
            _ => {}
        }
    }
    inside
}

/// Styles by byte offset: from Mimic's own lexer when the text lexes, and a
/// simple scan otherwise (while a string is half typed).
fn highlight(text: &str) -> std::collections::HashMap<usize, Style> {
    use mimic_syntax::lexer::{Tok, lex};
    let mut out = std::collections::HashMap::new();
    fn paint(out: &mut std::collections::HashMap<usize, Style>, text: &str, a: usize, b: usize, st: Style) {
        let mut i = a;
        while i < b.min(text.len()) {
            out.insert(i, st);
            i += text[i..].chars().next().map_or(1, char::len_utf8);
        }
    }
    // comments: `//` outside strings, and `⍝`
    for (ls, line) in text.split_inclusive('\n').scan(0, |at, l| {
        let s = *at;
        *at += l.len();
        Some((s, l))
    }) {
        let mut inside = false;
        let mut prev = ' ';
        for (i, c) in line.char_indices() {
            if c == '"' && prev != '\\' {
                inside = !inside;
            }
            if !inside && (c == '⍝' || (c == '/' && line[i + 1..].starts_with('/'))) {
                paint(&mut out, text, ls + i, ls + line.len(), Style::default().fg(COMMENT).add_modifier(Modifier::ITALIC));
                break;
            }
            prev = c;
        }
    }
    match lex(text) {
        Ok(toks) => {
            for (k, t) in toks.iter().enumerate() {
                let (a, b) = (t.span.start, t.span.end);
                if out.contains_key(&a) {
                    continue; // inside a comment
                }
                let st = match &t.tok {
                    Tok::Str(_) | Tok::Interp(_) => Style::default().fg(STRING),
                    Tok::Int(_) | Tok::Float(_) => Style::default().fg(NUMBER),
                    Tok::Prim(_) => Style::default().fg(BRIGHT),
                    Tok::True | Tok::False => Style::default().fg(NUMBER),
                    Tok::Ident(w) if w.starts_with(char::is_uppercase) => Style::default().fg(TYPE),
                    Tok::Ident(w) if (w == "c" || w == "cpp") && matches!(toks.get(k + 1).map(|x| &x.tok), Some(Tok::PathSep)) => Style::default().fg(FOREIGN),
                    // the name after `c::`
                    Tok::Ident(_) if k >= 2 && matches!(toks[k - 1].tok, Tok::PathSep) && matches!(&toks[k - 2].tok, Tok::Ident(w) if w == "c" || w == "cpp") => Style::default().fg(FOREIGN),
                    Tok::Ident(w) if w == "test" || w == "requires" || w == "ensures" => Style::default().fg(ORANGE).add_modifier(Modifier::BOLD),
                    Tok::Ident(_) => Style::default().fg(INK),
                    Tok::Newline | Tok::Eof => continue,
                    _ => {
                        let s = &text[a.min(text.len())..b.min(text.len())];
                        if s.chars().next().is_some_and(|c| c.is_ascii_alphabetic()) {
                            Style::default().fg(ORANGE).add_modifier(Modifier::BOLD) // keywords
                        } else if !s.is_ascii() {
                            Style::default().fg(BRIGHT) // glyphs
                        } else {
                            Style::default().fg(MUTED)
                        }
                    }
                };
                paint(&mut out, text, a, b, st);
            }
        }
        Err(_) => {
            // half-typed code: strings, numbers and glyphs by a simple scan
            let mut inside = None;
            for (i, c) in text.char_indices() {
                if out.contains_key(&i) && inside.is_none() {
                    continue;
                }
                match (inside, c) {
                    (None, '"') => {
                        inside = Some(i);
                        paint(&mut out, text, i, i + 1, Style::default().fg(STRING));
                    }
                    (Some(_), '"') | (Some(_), '\n') => {
                        inside = None;
                        paint(&mut out, text, i, i + 1, Style::default().fg(STRING));
                    }
                    (Some(_), _) => paint(&mut out, text, i, i + c.len_utf8(), Style::default().fg(STRING)),
                    (None, c) if c.is_ascii_digit() => paint(&mut out, text, i, i + 1, Style::default().fg(NUMBER)),
                    (None, c) if !c.is_ascii() => paint(&mut out, text, i, i + c.len_utf8(), Style::default().fg(BRIGHT)),
                    _ => {}
                }
            }
        }
    }
    out
}

// ---- the terminal ------------------------------------------------------------------------

pub fn run(target: Option<&Path>) -> Result<(), String> {
    use crossterm::execute;
    use crossterm::terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode};
    use std::io::IsTerminal;
    if !std::io::stdout().is_terminal() {
        return Err("mimic edit needs a terminal".into());
    }
    let mut ed = Editor::open(target)?;
    enable_raw_mode().map_err(|e| e.to_string())?;
    let mut out = std::io::stdout();
    execute!(out, EnterAlternateScreen, crossterm::event::EnableMouseCapture).map_err(|e| e.to_string())?;
    // leave the terminal as it was, even on a crash
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(std::io::stdout(), crossterm::event::DisableMouseCapture, LeaveAlternateScreen);
        default_hook(info);
    }));
    let mut term = ratatui::Terminal::new(ratatui::backend::CrosstermBackend::new(std::io::stdout())).map_err(|e| e.to_string())?;
    let result = (|| -> Result<(), String> {
        while !ed.quit {
            term.draw(|f| ed.draw(f)).map_err(|e| e.to_string())?;
            if event::poll(Duration::from_millis(100)).map_err(|e| e.to_string())? {
                match event::read().map_err(|e| e.to_string())? {
                    Event::Key(k) if k.kind == event::KeyEventKind::Press => ed.key(k),
                    Event::Mouse(m) => ed.mouse(m),
                    Event::Paste(s) => {
                        if let Some(d) = ed.doc() {
                            d.insert(&s);
                        }
                    }
                    _ => {}
                }
            }
            ed.tick();
        }
        Ok(())
    })();
    let _ = disable_raw_mode();
    let _ = execute!(std::io::stdout(), crossterm::event::DisableMouseCapture, LeaveAlternateScreen);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;

    fn key(c: KeyCode) -> KeyEvent {
        KeyEvent::new(c, KeyModifiers::NONE)
    }
    fn ctrl(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
    }
    fn typing(ed: &mut Editor, s: &str) {
        for c in s.chars() {
            ed.key(if c == '\n' { key(KeyCode::Enter) } else { key(KeyCode::Char(c)) });
        }
    }
    fn screen(ed: &mut Editor) -> String {
        let mut t = ratatui::Terminal::new(TestBackend::new(100, 30)).unwrap();
        t.draw(|f| ed.draw(f)).unwrap();
        let buf = t.backend().buffer().clone();
        (0..30).map(|y| (0..100).map(|x| buf[(x, y)].symbol().to_string()).collect::<String>() + "\n").collect()
    }
    fn temp(name: &str, text: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("mimic-edit-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("main.mim");
        std::fs::write(&f, text).unwrap();
        f
    }

    #[test]
    fn typing_glyphs_undo_save() {
        let f = temp("typing", "module main\n\nfn main() {\n}\n");
        let mut ed = Editor::open(Some(&f)).unwrap();
        ed.docs[0].cursor = (2, 11);
        ed.key(key(KeyCode::Enter));
        typing(&mut ed, "print(\\i 5 \\e (x => x))");
        assert_eq!(ed.docs[0].line_text(3), "    print(⍳ 5 ¨ (x => x))", "digraphs become glyphs, indentation follows");
        // not inside a string
        typing(&mut ed, "\nprint(\"\\i\")");
        assert_eq!(ed.docs[0].line_text(4), "    print(\"\\i\")");
        ed.key(ctrl('z'));
        ed.key(ctrl('z'));
        assert!(!ed.docs[0].text.to_string().contains("\\i\""), "{}", ed.docs[0].text);
        ed.key(ctrl('y'));
        ed.key(ctrl('s'));
        assert_eq!(std::fs::read_to_string(&f).unwrap(), ed.docs[0].text.to_string());
        assert!(!ed.docs[0].dirty);
    }

    /// Wait for the REPL's line (it builds and runs a program).
    fn settle(ed: &mut Editor) {
        for _ in 0..600 {
            ed.tick();
            if ed.repl_job.is_none() {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        panic!("the REPL did not answer");
    }

    #[test]
    fn repl_pane() {
        // (what lines evaluate to is the golden test repl_session: the same
        // Session; whether a unit test can link programs depends on the
        // system, so an expression is typed, which answers either way: its
        // value, or the error. The pane is what is tested)
        let f = temp("repl", "module main\n\nfn main() {\n    print(⍳ 4 ▷ +/)\n}\n");
        let mut ed = Editor::open(Some(&f)).unwrap();
        ed.key(key(KeyCode::F(4)));
        typing(&mut ed, "\\i 5");
        assert_eq!(ed.prompt.as_ref().unwrap().input, "⍳ 5", "glyph input in the prompt");
        ed.key(key(KeyCode::Enter));
        assert_eq!(ed.pane, Pane::Repl);
        assert!(ed.repl_job.is_some(), "evaluated on another thread");
        settle(&mut ed);
        let log = |ed: &Editor| ed.pane_lines.iter().map(|l| l.0.clone()).collect::<Vec<_>>();
        assert_eq!(log(&ed)[0], "› ⍳ 5");
        assert!(log(&ed).len() >= 2, "an answer: {:?}", log(&ed));
        // Ctrl-E: the line under the cursor
        ed.docs[0].cursor = (3, 0);
        ed.key(ctrl('e'));
        settle(&mut ed);
        assert!(log(&ed).iter().any(|l| l == "› print(⍳ 4 ▷ +/)"), "{:?}", log(&ed));
        assert!(screen(&mut ed).contains("REPL"));
    }

    fn click_at(ed: &mut Editor, x: u16, y: u16) {
        ed.mouse(MouseEvent { kind: MouseEventKind::Down(MouseButton::Left), column: x, row: y, modifiers: KeyModifiers::NONE });
    }

    /// Where a text is on the screen (column, row).
    fn find_on(screen: &str, text: &str) -> (u16, u16) {
        for (y, line) in screen.lines().enumerate() {
            if let Some(i) = line.find(text) {
                return (line[..i].chars().count() as u16, y as u16);
            }
        }
        panic!("`{text}` is not on the screen:\n{screen}");
    }

    #[test]
    fn everything_is_clickable() {
        // a project: src/main.mim, src/util/text.mim, docs/guide.md
        let dir = std::env::temp_dir().join(format!("mimic-edit-test-{}-clicks", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src/util")).unwrap();
        std::fs::create_dir_all(dir.join("docs")).unwrap();
        std::fs::write(dir.join("mim.mim"), "module mim\n\nproject {\n    name = \"clicks\"\n    version = \"0.1.0\"\n    kind = [\"bin\"]\n}\n").unwrap();
        std::fs::write(dir.join("src/main.mim"), "module main\n\nfn main() {\n    print(1)\n}\n").unwrap();
        std::fs::write(dir.join("src/util/text.mim"), "module text\n").unwrap();
        std::fs::write(dir.join("docs/guide.md"), "# Guide\n").unwrap();
        let mut ed = Editor::open(Some(&dir)).unwrap();
        // folders: src open, docs closed; a click opens and closes them
        let s = screen(&mut ed);
        assert!(s.contains("▾ src/") && s.contains("▸ docs/") && !s.contains("guide.md"), "{s}");
        let (x, y) = find_on(&s, "▸ docs/");
        click_at(&mut ed, x, y);
        let s = screen(&mut ed);
        assert!(s.contains("▾ docs/") && s.contains("guide.md"), "{s}");
        let (x, y) = find_on(&s, "▾ docs/");
        click_at(&mut ed, x, y);
        assert!(!screen(&mut ed).contains("guide.md"));
        // a nested folder, then a file in it
        let s = screen(&mut ed);
        let (x, y) = find_on(&s, "▸ util/");
        click_at(&mut ed, x, y);
        let s = screen(&mut ed);
        let (x, y) = find_on(&s, "text.mim");
        click_at(&mut ed, x, y);
        assert_eq!(ed.docs[ed.cur].name(), "text.mim");
        // open main.mim from the tree: two tabs; a tab switches, its × closes it
        let s = screen(&mut ed);
        let (x, y) = find_on(&s, "◆ main.mim");
        click_at(&mut ed, x + 2, y);
        assert_eq!(ed.docs.len(), 2);
        let s = screen(&mut ed);
        let (x, y) = find_on(&s, " text.mim ");
        click_at(&mut ed, x + 1, y);
        assert_eq!(ed.docs[ed.cur].name(), "text.mim");
        let s = screen(&mut ed);
        let (x, y) = find_on(&s, " text.mim ");
        click_at(&mut ed, x + " text.mim ".len() as u16, y);
        assert_eq!(ed.docs.len(), 1, "× closed it");
        // the toolbar's ≡ Commands opens the command list; a click runs one
        let s = screen(&mut ed);
        let (x, y) = find_on(&s, " ≡ ");
        click_at(&mut ed, x, y);
        assert_eq!(ed.prompt.as_ref().map(|p| p.kind.clone()), Some(PromptKind::Command));
        typing(&mut ed, "go to");
        let s = screen(&mut ed);
        let (x, y) = find_on(&s, "Go to a line");
        click_at(&mut ed, x, y);
        assert_eq!(ed.prompt.as_ref().map(|p| p.kind.clone()), Some(PromptKind::GotoLine), "the command ran");
        ed.key(key(KeyCode::Esc));
        // a pane's file:line jumps there
        ed.set_pane(Pane::Output, vec![("error at src/main.mim:4:5 here".into(), Style::default())]);
        let s = screen(&mut ed);
        let (x, y) = find_on(&s, "error at src/main.mim:4:5");
        click_at(&mut ed, x, y);
        assert_eq!((ed.docs[ed.cur].name(), ed.docs[ed.cur].cursor), ("main.mim".to_string(), (3, 4)));
        // the pane tabs switch panes
        let s = screen(&mut ed);
        let (x, y) = find_on(&s, " REPL ");
        click_at(&mut ed, x + 1, y);
        assert_eq!(ed.pane, Pane::Repl);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn toolbar_labels_on_a_wide_screen() {
        let f = temp("wide", "module main\n\nfn main() {\n}\n");
        let mut ed = Editor::open(Some(&f)).unwrap();
        let mut t = ratatui::Terminal::new(TestBackend::new(160, 30)).unwrap();
        t.draw(|fr| ed.draw(fr)).unwrap();
        let buf = t.backend().buffer().clone();
        let top: String = (0..160).map(|x| buf[(x, 0)].symbol().to_string()).collect();
        for label in ["▶ Run", "✓ Test", "⚑ Audit", "⍳ Glyphs", "≡ Commands"] {
            assert!(top.contains(label), "{top}");
        }
        let bottom: String = (0..160).map(|x| buf[(x, 29)].symbol().to_string()).collect();
        assert!(bottom.contains("F2 commands") && bottom.contains("no errors"), "{bottom}");
    }

    #[test]
    fn file_and_line_in_output() {
        assert_eq!(file_line("  --> src/main.mim:12:5"), Some(("src/main.mim".into(), 12, 5)));
        assert_eq!(file_line("✗ main.mim:3:1  E0303 nope"), Some(("main.mim".into(), 3, 1)));
        assert_eq!(file_line("(/tmp/x.mim:7)"), Some(("/tmp/x.mim".into(), 7, 1)));
        assert_eq!(file_line("no file here"), None);
    }

    #[test]
    fn errors_as_you_type_and_panes() {
        let f = temp("errors", "module main\n\nfn main() {\n    print(1)\n}\n");
        let mut ed = Editor::open(Some(&f)).unwrap();
        assert!(ed.problems.is_empty());
        ed.docs[0].cursor = (3, 11);
        typing(&mut ed, " + nope");
        ed.check();
        assert_eq!(ed.problems.len(), 1);
        assert_eq!(ed.problems[0].code, "E0303");
        let s = screen(&mut ed);
        assert!(s.contains("✗ 1 error(s)"), "{s}");
        assert!(s.contains("cannot find `nope`") || s.contains("●"), "{s}");
        // the manual for the word under the cursor
        ed.docs[0].cursor = (3, 6);
        ed.key(key(KeyCode::F(1)));
        assert_eq!(ed.pane, Pane::Manual);
        assert!(ed.pane_lines.iter().any(|(l, _)| l.contains("print")), "manual for print");
        // quitting with unsaved changes asks first
        ed.key(ctrl('q'));
        assert!(!ed.quit && ed.message.contains("unsaved"));
        ed.key(ctrl('q'));
        assert!(ed.quit);
    }

    #[test]
    fn palette_find_replace_open() {
        let f = temp("palette", "module main\n\nfn main() {\n    print(1)\n    print(1)\n}\n");
        let mut ed = Editor::open(Some(&f)).unwrap();
        ed.docs[0].cursor = (3, 10);
        ed.key(ctrl('g'));
        typing(&mut ed, "keep");
        ed.key(key(KeyCode::Enter));
        assert_eq!(ed.docs[0].line_text(3), "    print(⌿1)");
        ed.key(ctrl('h'));
        typing(&mut ed, "print");
        ed.key(key(KeyCode::Enter));
        typing(&mut ed, "write");
        ed.key(key(KeyCode::Enter));
        assert!(ed.message.contains("replaced 2"), "{}", ed.message);
        ed.key(ctrl('o'));
        typing(&mut ed, "main");
        let items = ed.prompt_items();
        assert!(items.iter().any(|(l, ..)| l.ends_with("main.mim")), "{:?}", items.iter().map(|x| &x.0).collect::<Vec<_>>());
        ed.key(key(KeyCode::Esc));
        assert!(ed.prompt.is_none());
    }

    #[test]
    fn start_screen() {
        let dir = std::env::temp_dir().join(format!("mimic-edit-test-{}-start", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let mut ed = Editor::open(Some(&dir)).unwrap();
        let s = screen(&mut ed);
        assert!(s.contains("will run in parallel") && s.contains("Ctrl-G") && s.contains("MIMIC"), "{s}");
    }
}
