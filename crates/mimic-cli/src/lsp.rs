//! `mimic lsp`: the language server, over stdin and stdout. Every answer
//! comes from the Workbench engine (see engine.rs), the same one `mimic
//! query` and the other tools ask: errors as you type with the compiler's
//! fixes, hover, go to definition (into the standard library too), find
//! references, glyph input (`\i` becomes `⍳`), completion, formatting,
//! the outline, a colour of their own for names that come from C, signature
//! help, rename, the call hierarchy, smart selection, and the inferred types
//! of `let`s as inlay hints.

use crate::driver;
use crate::engine::Engine;
use crate::report::Report;
use lsp_server::{Connection, Message, Notification, Request, Response};
use lsp_types::*;
use mimic_syntax::glyphs::{PRIMS, SYNTAX_GLYPHS};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

const PROJECT_FILES: [&str; 3] = ["mim.mim", "build.mim", "mod.mim"];

pub fn serve() -> Result<(), String> {
    let (conn, io) = Connection::stdio();
    let caps = ServerCapabilities {
        position_encoding: Some(PositionEncodingKind::UTF16),
        text_document_sync: Some(TextDocumentSyncCapability::Kind(TextDocumentSyncKind::FULL)),
        hover_provider: Some(HoverProviderCapability::Simple(true)),
        definition_provider: Some(OneOf::Left(true)),
        references_provider: Some(OneOf::Left(true)),
        document_formatting_provider: Some(OneOf::Left(true)),
        document_symbol_provider: Some(OneOf::Left(true)),
        inlay_hint_provider: Some(OneOf::Left(true)),
        workspace_symbol_provider: Some(OneOf::Left(true)),
        signature_help_provider: Some(SignatureHelpOptions { trigger_characters: Some(vec!["(".into(), ",".into()]), retrigger_characters: None, work_done_progress_options: Default::default() }),
        rename_provider: Some(OneOf::Right(RenameOptions { prepare_provider: Some(true), work_done_progress_options: Default::default() })),
        call_hierarchy_provider: Some(CallHierarchyServerCapability::Simple(true)),
        selection_range_provider: Some(SelectionRangeProviderCapability::Simple(true)),
        code_action_provider: Some(CodeActionProviderCapability::Options(CodeActionOptions {
            code_action_kinds: Some(vec![CodeActionKind::QUICKFIX, CodeActionKind::REFACTOR_REWRITE, CodeActionKind::REFACTOR_EXTRACT]),
            ..Default::default()
        })),
        completion_provider: Some(CompletionOptions {
            trigger_characters: Some(vec!["\\".into(), ":".into()]),
            ..Default::default()
        }),
        semantic_tokens_provider: Some(SemanticTokensServerCapabilities::SemanticTokensOptions(SemanticTokensOptions {
            legend: legend(),
            full: Some(SemanticTokensFullOptions::Bool(true)),
            range: None,
            ..Default::default()
        })),
        ..Default::default()
    };
    let caps = serde_json::to_value(caps).map_err(|e| e.to_string())?;
    let (id, _params) = conn.initialize_start().map_err(|e| e.to_string())?;
    let result = json!({ "capabilities": caps, "serverInfo": { "name": "mimic", "version": env!("CARGO_PKG_VERSION") } });
    conn.sender.send(Message::Response(Response::new_ok(id, result))).map_err(|e| e.to_string())?;
    // the client's `initialized` arrives as an ordinary notification
    let mut s = Server { conn, engines: HashMap::new(), open: HashMap::new(), published: HashMap::new(), fixes: Vec::new() };
    s.run()?;
    drop(s);
    io.join().map_err(|e| e.to_string())
}

fn legend() -> SemanticTokensLegend {
    SemanticTokensLegend {
        token_types: vec![SemanticTokenType::NAMESPACE, SemanticTokenType::FUNCTION, SemanticTokenType::TYPE],
        token_modifiers: vec![SemanticTokenModifier::new("foreign")],
    }
}

struct Server {
    conn: Connection,
    /// one engine per project root (or standalone file)
    engines: HashMap<PathBuf, Engine>,
    /// open documents: path → text
    open: HashMap<PathBuf, String>,
    /// per engine: the files diagnostics were last published for
    published: HashMap<PathBuf, HashSet<PathBuf>>,
    /// quick fixes of the latest diagnostics: (file, range, code, title, edits)
    fixes: Vec<(PathBuf, Range, String, String, HashMap<Url, Vec<TextEdit>>)>,
}

fn path_of(uri: &Url) -> Option<PathBuf> {
    let p = uri.to_file_path().ok()?;
    Some(driver::canonical(&p))
}

fn url_of(p: &Path) -> Option<Url> {
    Url::from_file_path(p).ok()
}

/// The standard library's source, written out once, so "go to definition"
/// can open it.
fn std_file() -> PathBuf {
    let src = driver::PRELUDE;
    let dir = mimic_project::packages::cache_dir().join("std");
    let f = dir.join(format!("std-{}-{:08x}.mim", env!("CARGO_PKG_VERSION"), mimic_syntax::fnv1a(src.as_bytes()) as u32));
    if !f.is_file() {
        let _ = std::fs::create_dir_all(&dir);
        let _ = mimic_toolchain::jobs::write_atomic(&f, src.as_bytes());
    }
    f
}

// ---- positions: LSP counts UTF-16 units, the engine characters ---------------

/// (1-based line, 1-based column in characters) of an LSP position.
fn to_engine(text: &str, p: Position) -> (u32, u32) {
    let line = text.split('\n').nth(p.line as usize).unwrap_or("");
    let mut units = 0u32;
    let mut chars = 0u32;
    for c in line.chars() {
        if units >= p.character {
            break;
        }
        units += c.len_utf16() as u32;
        chars += 1;
    }
    (p.line + 1, chars + 1)
}

/// The LSP position of 1-based `line` and `col` (characters).
fn to_lsp(text: &str, line: u32, col: u32) -> Position {
    let l = text.split('\n').nth(line.saturating_sub(1) as usize).unwrap_or("");
    let units: usize = l.chars().take(col.saturating_sub(1) as usize).map(char::len_utf16).sum();
    Position { line: line.saturating_sub(1), character: units as u32 }
}

/// The LSP position of byte `at` of `text`.
fn pos_of_byte(text: &str, at: usize) -> Position {
    let at = text.floor_char_boundary(at.min(text.len()));
    let before = &text[..at];
    let line = before.matches('\n').count() as u32;
    let start = before.rfind('\n').map_or(0, |i| i + 1);
    let units: usize = before[start..].chars().map(char::len_utf16).sum();
    Position { line, character: units as u32 }
}

fn byte_of_pos(text: &str, p: Position) -> usize {
    let mut start = 0;
    for _ in 0..p.line {
        match text[start..].find('\n') {
            Some(i) => start += i + 1,
            None => return text.len(),
        }
    }
    let line_end = text[start..].find('\n').map_or(text.len(), |i| start + i);
    let mut units = 0u32;
    for (i, c) in text[start..line_end].char_indices() {
        if units >= p.character {
            return start + i;
        }
        units += c.len_utf16() as u32;
    }
    line_end
}

impl Server {
    fn run(&mut self) -> Result<(), String> {
        while let Ok(msg) = self.conn.receiver.recv() {
            match msg {
                Message::Request(req) => {
                    if self.conn.handle_shutdown(&req).map_err(|e| e.to_string())? {
                        return Ok(());
                    }
                    let id = req.id.clone();
                    let reply = match self.request(req) {
                        Ok(v) => Response::new_ok(id, v),
                        Err(e) => Response::new_err(id, lsp_server::ErrorCode::RequestFailed as i32, e),
                    };
                    self.send(Message::Response(reply));
                }
                Message::Notification(n) => self.notification(n),
                Message::Response(_) => {}
            }
        }
        Ok(())
    }

    fn send(&self, m: Message) {
        let _ = self.conn.sender.send(m);
    }

    fn text_of(&self, path: &Path) -> String {
        if let Some(t) = self.open.get(path) {
            return t.clone();
        }
        std::fs::read_to_string(path).unwrap_or_default()
    }

    /// The engine for `file`: its project's, or one of its own.
    fn engine_key(file: &Path) -> Option<PathBuf> {
        let name = file.file_name()?.to_string_lossy().to_string();
        if PROJECT_FILES.contains(&name.as_str()) {
            return None;
        }
        match mimic_project::find_root(file.parent()?) {
            Some(root) if file.starts_with(root.join("src")) => Some(root),
            _ => Some(file.to_path_buf()),
        }
    }

    fn engine(&mut self, file: &Path) -> Option<(&mut Engine, PathBuf)> {
        let key = Self::engine_key(file)?;
        if !self.engines.contains_key(&key) {
            let e = if key.is_dir() { Engine::for_project(&key).ok()? } else { Engine::for_file(&key) };
            self.engines.insert(key.clone(), e);
        }
        Some((self.engines.get_mut(&key).unwrap(), key))
    }

    // ---- notifications ------------------------------------------------------

    fn notification(&mut self, n: Notification) {
        match n.method.as_str() {
            "textDocument/didOpen" => {
                let Ok(p) = serde_json::from_value::<DidOpenTextDocumentParams>(n.params) else { return };
                let Some(path) = path_of(&p.text_document.uri) else { return };
                self.set_text(&path, Some(p.text_document.text));
            }
            "textDocument/didChange" => {
                let Ok(p) = serde_json::from_value::<DidChangeTextDocumentParams>(n.params) else { return };
                let Some(path) = path_of(&p.text_document.uri) else { return };
                if let Some(c) = p.content_changes.into_iter().last() {
                    self.set_text(&path, Some(c.text));
                }
            }
            "textDocument/didClose" => {
                let Ok(p) = serde_json::from_value::<DidCloseTextDocumentParams>(n.params) else { return };
                let Some(path) = path_of(&p.text_document.uri) else { return };
                self.set_text(&path, None);
            }
            "textDocument/didSave" | "workspace/didChangeWatchedFiles" => {
                // a project file or a file on disk changed: check everything again
                let keys: Vec<PathBuf> = self.engines.keys().cloned().collect();
                for k in keys {
                    self.check(&k);
                }
            }
            _ => {}
        }
    }

    fn set_text(&mut self, path: &Path, text: Option<String>) {
        match &text {
            Some(t) => {
                self.open.insert(path.to_path_buf(), t.clone());
            }
            None => {
                self.open.remove(path);
            }
        }
        driver::sources::set_open(path, text);
        if let Some((_, key)) = self.engine(path) {
            self.check(&key);
        }
    }

    /// Check one engine's program and publish its diagnostics.
    fn check(&mut self, key: &Path) {
        let Some(engine) = self.engines.get_mut(key) else { return };
        engine.check();
        let mut by_file: HashMap<PathBuf, Vec<Diagnostic>> = HashMap::new();
        let mut fixes = Vec::new();
        if let Some(r) = engine.errors() {
            for (path, d, f) in diagnostics(r, key) {
                by_file.entry(path.clone()).or_default().push(d.clone());
                for (title, edits) in f {
                    fixes.push((path.clone(), d.range, code_of(&d), title, edits));
                }
            }
        }
        // every file of the program, so fixed errors disappear
        for (p, _) in engine.files() {
            if p.is_file() {
                by_file.entry(driver::canonical(&p)).or_default();
            }
        }
        for p in self.open.keys() {
            if Self::engine_key(p).as_deref() == Some(key) {
                by_file.entry(p.clone()).or_default();
            }
        }
        let before = self.published.remove(key).unwrap_or_default();
        for p in before {
            by_file.entry(p).or_default();
        }
        self.fixes.retain(|(p, ..)| Self::engine_key(p).as_deref() != Some(key));
        self.fixes.extend(fixes);
        let mut now = HashSet::new();
        for (path, diags) in by_file {
            if !diags.is_empty() {
                now.insert(path.clone());
            }
            let Some(uri) = url_of(&path) else { continue };
            let params = PublishDiagnosticsParams { uri, diagnostics: diags, version: None };
            self.send(Message::Notification(Notification::new("textDocument/publishDiagnostics".into(), params)));
        }
        self.published.insert(key.to_path_buf(), now);
    }

    // ---- requests -----------------------------------------------------------

    fn request(&mut self, req: Request) -> Result<Value, String> {
        let params = req.params;
        let ok = |v: Option<Value>| Ok(v.unwrap_or(Value::Null));
        match req.method.as_str() {
            "textDocument/hover" => {
                let p: HoverParams = from(params)?;
                ok(self.hover(&p.text_document_position_params))
            }
            "textDocument/definition" => {
                let p: GotoDefinitionParams = from(params)?;
                ok(self.definition(&p.text_document_position_params))
            }
            "textDocument/references" => {
                let p: ReferenceParams = from(params)?;
                ok(self.references(&p.text_document_position, p.context.include_declaration))
            }
            "textDocument/completion" => {
                let p: CompletionParams = from(params)?;
                ok(self.completion(&p.text_document_position))
            }
            "textDocument/formatting" => {
                let p: DocumentFormattingParams = from(params)?;
                ok(self.formatting(&p.text_document.uri))
            }
            "textDocument/codeAction" => {
                let p: CodeActionParams = from(params)?;
                ok(self.code_actions(&p))
            }
            "textDocument/documentSymbol" => {
                let p: DocumentSymbolParams = from(params)?;
                ok(self.outline(&p.text_document.uri))
            }
            "workspace/symbol" => {
                let p: WorkspaceSymbolParams = from(params)?;
                ok(self.workspace_symbols(&p.query))
            }
            "textDocument/inlayHint" => {
                let p: InlayHintParams = from(params)?;
                ok(self.where_hints(&p.text_document.uri, p.range))
            }
            "textDocument/signatureHelp" => {
                let p: SignatureHelpParams = from(params)?;
                ok(self.signature_help(&p.text_document_position_params))
            }
            "textDocument/prepareRename" => {
                let p: TextDocumentPositionParams = from(params)?;
                ok(self.prepare_rename(&p))
            }
            "textDocument/rename" => {
                let p: RenameParams = from(params)?;
                self.rename(&p.text_document_position, &p.new_name).map(|v| v.unwrap_or(Value::Null))
            }
            "textDocument/prepareCallHierarchy" => {
                let p: CallHierarchyPrepareParams = from(params)?;
                ok(self.call_item_at(&p.text_document_position_params).and_then(|i| serde_json::to_value(vec![i]).ok()))
            }
            "callHierarchy/incomingCalls" => {
                let p: CallHierarchyIncomingCallsParams = from(params)?;
                ok(self.incoming_calls(&p.item))
            }
            "callHierarchy/outgoingCalls" => {
                let p: CallHierarchyOutgoingCallsParams = from(params)?;
                ok(self.outgoing_calls(&p.item))
            }
            "textDocument/selectionRange" => {
                let p: SelectionRangeParams = from(params)?;
                ok(self.selection_ranges(&p.text_document.uri, &p.positions))
            }
            "textDocument/semanticTokens/full" => {
                let p: SemanticTokensParams = from(params)?;
                ok(self.semantic_tokens(&p.text_document.uri))
            }
            m => Err(format!("unsupported request {m}")),
        }
    }

    /// Ask the engine about a position: (engine answer, the file's text).
    fn ask(&mut self, method: &str, tdp: &TextDocumentPositionParams) -> Option<Value> {
        let path = path_of(&tdp.text_document.uri)?;
        let text = self.text_of(&path);
        let (line, column) = to_engine(&text, tdp.position);
        let (engine, _) = self.engine(&path)?;
        let v = engine.query(method, &json!({ "file": path.display().to_string(), "line": line, "column": column })).ok()?;
        (!v.is_null()).then_some(v)
    }

    /// An engine location as an LSP one.
    fn location(&self, loc: &Value) -> Option<Location> {
        let file = loc.get("file")?.as_str()?;
        let path = if file == "<prelude>" { std_file() } else { PathBuf::from(file) };
        let text = if file == "<prelude>" { driver::PRELUDE.to_string() } else { self.text_of(&path) };
        let n = |k: &str| loc.get(k).and_then(Value::as_u64).unwrap_or(1) as u32;
        let range = Range { start: to_lsp(&text, n("line"), n("column")), end: to_lsp(&text, n("end_line"), n("end_column")) };
        Some(Location { uri: url_of(&path)?, range })
    }

    fn hover(&mut self, tdp: &TextDocumentPositionParams) -> Option<Value> {
        if let Some(v) = self.ask("hover", tdp) {
            let mut md = String::new();
            if v.get("kind").and_then(Value::as_str) == Some("local") {
                md += &format!("```mimic\n{}\n```\nlocal variable", v["name"].as_str().unwrap_or(""));
            } else {
                md += &format!("```mimic\n{}\n```\n", v["signature"].as_str().unwrap_or(""));
                if let Some(path) = v.get("path").and_then(Value::as_str) {
                    md += &format!("`{path}`");
                }
                if let Some(e) = v.get("effects").and_then(Value::as_str) {
                    md += &format!(" · {e}");
                }
                if let Some(h) = v.get("header").and_then(Value::as_str) {
                    md += &format!(" · from C header `{h}`");
                }
                md += "\n";
                if let Some(d) = v.get("doc").and_then(Value::as_str).filter(|d| !d.is_empty()) {
                    md += &format!("\n{d}\n");
                }
                if let Some(path) = v.get("path").and_then(Value::as_str)
                    && v.get("package").and_then(Value::as_str) == Some("std")
                {
                    md += &format!("\n`mimic man {path}`\n");
                }
            }
            return serde_json::to_value(Hover { contents: HoverContents::Markup(MarkupContent { kind: MarkupKind::Markdown, value: md }), range: None }).ok();
        }
        // a glyph or a primitive's word
        let path = path_of(&tdp.text_document.uri)?;
        let text = self.text_of(&path);
        let at = byte_of_pos(&text, tdp.position);
        let md = glyph_hover(&text, at)?;
        serde_json::to_value(Hover { contents: HoverContents::Markup(MarkupContent { kind: MarkupKind::Markdown, value: md }), range: None }).ok()
    }

    fn definition(&mut self, tdp: &TextDocumentPositionParams) -> Option<Value> {
        let v = self.ask("definition", tdp)?;
        let loc = self.location(v.get("location")?)?;
        serde_json::to_value(GotoDefinitionResponse::Scalar(loc)).ok()
    }

    fn references(&mut self, tdp: &TextDocumentPositionParams, declaration: bool) -> Option<Value> {
        let v = self.ask("references", tdp)?;
        let mut locs: Vec<Location> = v.as_array()?.iter().filter_map(|l| self.location(l)).collect();
        if declaration
            && let Some(d) = self.ask("definition", tdp)
            && let Some(loc) = d.get("location").and_then(|l| self.location(l))
            && !locs.contains(&loc)
        {
            locs.insert(0, loc);
        }
        serde_json::to_value(locs).ok()
    }

    fn completion(&mut self, tdp: &TextDocumentPositionParams) -> Option<Value> {
        let path = path_of(&tdp.text_document.uri)?;
        let text = self.text_of(&path);
        let at = byte_of_pos(&text, tdp.position);
        let line_start = text[..at].rfind('\n').map_or(0, |i| i + 1);
        let before = &text[line_start..at];
        let mut items = Vec::new();
        // a digraph being typed: `\` or `\i` becomes the glyph
        if let Some(bs) = before.rfind('\\')
            && before[bs + 1..].chars().count() <= 1
            && !inside_string(before)
        {
            let typed = &before[bs..];
            let start = pos_of_byte(&text, line_start + bs);
            let range = Range { start, end: tdp.position };
            for (glyph, word, digraph, doc) in glyph_table() {
                let Some(d) = digraph else { continue };
                if !d.starts_with(typed) {
                    continue;
                }
                items.push(CompletionItem {
                    label: format!("{d}  {glyph}  {word}"),
                    filter_text: Some(d.to_string()),
                    sort_text: Some(d.to_string()),
                    kind: Some(CompletionItemKind::OPERATOR),
                    detail: Some(format!("{glyph} {word}")),
                    documentation: Some(Documentation::String(doc.to_string())),
                    text_edit: Some(CompletionTextEdit::Edit(TextEdit { range, new_text: glyph.to_string() })),
                    ..Default::default()
                });
            }
            return serde_json::to_value(CompletionResponse::Array(items)).ok();
        }
        // a name: the primitives' words, keywords, and the program's symbols
        let word_len = before.chars().rev().take_while(|c| c.is_alphanumeric() || *c == '_' || *c == ':').count();
        let word: String = before.chars().rev().take(word_len).collect::<Vec<_>>().into_iter().rev().collect();
        if !word.contains("::") {
            for (glyph, word_, _, doc) in glyph_table() {
                if word_.chars().all(|c| c.is_ascii_lowercase() || c == '_') {
                    items.push(CompletionItem {
                        label: word_.to_string(),
                        kind: Some(CompletionItemKind::FUNCTION),
                        detail: Some(format!("{glyph} {word_}: {doc}")),
                        ..Default::default()
                    });
                }
            }
            for k in ["module", "fn", "let", "mut", "pub", "if", "else", "match", "return", "use", "type", "extern", "unsafe", "trusted", "arms", "with", "test", "and", "or", "not", "div"] {
                items.push(CompletionItem { label: k.into(), kind: Some(CompletionItemKind::KEYWORD), ..Default::default() });
            }
        }
        let (engine, _) = self.engine(&path)?;
        let found = engine.query("symbols", &json!({ "query": word, "limit": 300 })).ok()?;
        let prefix = word.rsplit_once("::").map(|(ns, _)| format!("{ns}::"));
        for x in found["symbols"].as_array().into_iter().flatten() {
            let full = x["path"].as_str().unwrap_or("");
            let label = match &prefix {
                Some(p) => match full.strip_prefix(p.as_str()) {
                    Some(rest) if !rest.contains("::") => rest.to_string(),
                    _ => continue,
                },
                None => full.to_string(),
            };
            let kind = match x["kind"].as_str() {
                Some("type") => CompletionItemKind::STRUCT,
                Some("variant") => CompletionItemKind::ENUM_MEMBER,
                Some("module") => CompletionItemKind::MODULE,
                _ => CompletionItemKind::FUNCTION,
            };
            let doc = x["doc"].as_str().unwrap_or("");
            items.push(CompletionItem {
                label,
                kind: Some(kind),
                detail: x["signature"].as_str().map(String::from),
                documentation: (!doc.is_empty()).then(|| Documentation::String(doc.to_string())),
                ..Default::default()
            });
        }
        serde_json::to_value(CompletionResponse::List(CompletionList { is_incomplete: true, items })).ok()
    }

    fn formatting(&mut self, uri: &Url) -> Option<Value> {
        let path = path_of(uri)?;
        let text = self.text_of(&path);
        // keep the style the file mostly uses: glyphs or words
        // (the primitives written either way, and the pipeline; `print` reads
        // the same in both styles)
        let (mut glyphs, mut words) = (text.matches('▷').count(), text.matches("|>").count());
        for p in PRIMS.iter().filter(|p| p.word != "print") {
            glyphs += text.matches(p.glyph).count();
            words += text.split(|c: char| !c.is_alphanumeric() && c != '_').filter(|t| *t == p.word).count();
        }
        let formatted = mimic_syntax::format::format(&text, glyphs > words).ok()?;
        if formatted == text {
            return serde_json::to_value(Vec::<TextEdit>::new()).ok();
        }
        let end = pos_of_byte(&text, text.len());
        serde_json::to_value(vec![TextEdit { range: Range { start: Position::new(0, 0), end }, new_text: formatted }]).ok()
    }

    fn code_actions(&mut self, p: &CodeActionParams) -> Option<Value> {
        let path = path_of(&p.text_document.uri)?;
        let overlaps = |a: &Range, b: &Range| !(a.end < b.start || b.end < a.start);
        let mut actions = Vec::new();
        for (file, range, code, title, edits) in &self.fixes {
            if *file != path || !overlaps(range, &p.range) {
                continue;
            }
            let diag = p.context.diagnostics.iter().find(|d| d.range == *range && code_of(d) == *code).cloned();
            actions.push(CodeActionOrCommand::CodeAction(CodeAction {
                title: title.clone(),
                kind: Some(CodeActionKind::QUICKFIX),
                diagnostics: diag.map(|d| vec![d]),
                edit: Some(WorkspaceEdit { changes: Some(edits.clone()), ..Default::default() }),
                is_preferred: Some(true),
                ..Default::default()
            }));
        }
        // a selected expression into a function of its own
        if p.range.start != p.range.end
            && let Some(a) = self.extract_function(&p.text_document.uri, p.range)
        {
            actions.push(a);
        }
        // write an inferred type in: `let x = …` → `let x: T = …`
        if let Some((engine, _)) = self.engine(&path)
            && engine.errors().is_none()
            && let Ok(types) = engine.query("types", &json!({ "file": path.display().to_string() }))
        {
            let text = self.text_of(&path);
            for t in types.as_array().into_iter().flatten() {
                let pos = to_lsp(&text, t["line"].as_u64().unwrap_or(1) as u32, t["column"].as_u64().unwrap_or(1) as u32);
                if pos.line < p.range.start.line || pos.line > p.range.end.line {
                    continue;
                }
                let ty = t["type"].as_str().unwrap_or("");
                let mut changes = HashMap::new();
                changes.insert(p.text_document.uri.clone(), vec![TextEdit { range: Range { start: pos, end: pos }, new_text: format!(": {ty}") }]);
                actions.push(CodeActionOrCommand::CodeAction(CodeAction {
                    title: format!("Write the type of `{}`: {ty}", t["name"].as_str().unwrap_or("")),
                    kind: Some(CodeActionKind::REFACTOR_REWRITE),
                    edit: Some(WorkspaceEdit { changes: Some(changes), ..Default::default() }),
                    ..Default::default()
                }));
            }
        }
        serde_json::to_value(actions).ok()
    }

    /// "Extract into a function": the selection becomes `extracted(a, b)`,
    /// and `fn extracted(a, b) { … }` follows the function it was in; its
    /// parameters are the variables the selection uses from around it.
    fn extract_function(&mut self, uri: &Url, range: Range) -> Option<CodeActionOrCommand> {
        let path = path_of(uri)?;
        let text = self.text_of(&path);
        let (a, b) = (byte_of_pos(&text, range.start), byte_of_pos(&text, range.end));
        let sel = text.get(a..b)?.trim();
        if sel.is_empty() || sel.contains('\n') || open_brackets_of(sel) != 0 || sel.starts_with("let ") || sel.ends_with('{') {
            return None;
        }
        let lead = text[a..b].len() - text[a..b].trim_start().len();
        // the variables it uses that are defined outside it
        let mut params: Vec<String> = Vec::new();
        let bytes = sel.as_bytes();
        let mut i = 0;
        let mut inside = false;
        while i < bytes.len() {
            let c = sel[i..].chars().next()?;
            if c == '"' {
                inside = !inside;
            }
            if !inside && (c.is_alphabetic() || c == '_') && !sel[..i].ends_with("::") && !sel[..i].ends_with('.') {
                let len: usize = sel[i..].chars().take_while(|c| c.is_alphanumeric() || *c == '_').map(char::len_utf8).sum();
                let word = &sel[i..i + len];
                let at = a + lead + i;
                let tdp = TextDocumentPositionParams { text_document: TextDocumentIdentifier { uri: uri.clone() }, position: pos_of_byte(&text, at) };
                if let Some(v) = self.ask("definition", &tdp)
                    && v.get("kind").and_then(Value::as_str) == Some("local")
                {
                    let def_line = v["location"]["line"].as_u64().unwrap_or(0) as u32;
                    let def_col = v["location"]["column"].as_u64().unwrap_or(0) as u32;
                    let def = to_lsp(&text, def_line, def_col);
                    let outside = def < range.start || def >= range.end;
                    if outside && !params.iter().any(|p| p == word) {
                        params.push(word.to_string());
                    }
                }
                i += len.max(1);
            } else {
                i += c.len_utf8();
            }
        }
        // after the function the selection is in: its closing brace
        let fns = self.functions_of(&path);
        let line = range.start.line + 1;
        let (fn_line, _) = fns.iter().rev().find(|(l, _)| *l <= line)?;
        let start = byte_of_pos(&text, Position { line: fn_line - 1, character: 0 });
        let mut depth = 0i32;
        let mut end = None;
        let mut in_str = false;
        for (k, c) in text[start..].char_indices() {
            match c {
                '"' => in_str = !in_str,
                '{' if !in_str => depth += 1,
                '}' if !in_str => {
                    depth -= 1;
                    if depth == 0 {
                        end = Some(start + k + 1);
                        break;
                    }
                }
                _ => {}
            }
        }
        let end = end?;
        if end < b {
            return None;
        }
        let mut name = "extracted".to_string();
        let mut n = 2;
        while text.contains(&format!("fn {name}(")) {
            name = format!("extracted_{n}");
            n += 1;
        }
        let call = format!("{name}({})", params.join(", "));
        let def = format!("\n\nfn {name}({}) {{\n    {sel}\n}}", params.join(", "));
        let at_end = pos_of_byte(&text, end);
        let mut changes = HashMap::new();
        changes.insert(
            uri.clone(),
            vec![
                TextEdit { range: Range { start: pos_of_byte(&text, a + lead), end: pos_of_byte(&text, a + lead + sel.len()) }, new_text: call },
                TextEdit { range: Range { start: at_end, end: at_end }, new_text: def },
            ],
        );
        Some(CodeActionOrCommand::CodeAction(CodeAction {
            title: format!("Extract into a function ({name})"),
            kind: Some(CodeActionKind::REFACTOR_EXTRACT),
            edit: Some(WorkspaceEdit { changes: Some(changes), ..Default::default() }),
            ..Default::default()
        }))
    }

    // ---- signature help ---------------------------------------------------------

    fn signature_help(&mut self, tdp: &TextDocumentPositionParams) -> Option<Value> {
        let path = path_of(&tdp.text_document.uri)?;
        let text = self.text_of(&path);
        let at = byte_of_pos(&text, tdp.position);
        let (open, active) = open_call(&text, at)?;
        // the name before `(`
        let before = text[..open].trim_end();
        let name_len = before.chars().rev().take_while(|c| c.is_alphanumeric() || *c == '_' || *c == ':').map(char::len_utf8).sum::<usize>();
        if name_len == 0 {
            return None;
        }
        let name_at = before.len() - name_len;
        let pos = pos_of_byte(&text, name_at + name_len.saturating_sub(1).min(name_len));
        let v = self.ask("hover", &TextDocumentPositionParams { text_document: tdp.text_document.clone(), position: pos })?;
        let label = v.get("signature")?.as_str()?.to_string();
        let params = param_spans(&label);
        let utf16 = |s: &str| s.encode_utf16().count() as u32;
        let parameters = params.iter().map(|(a, b)| ParameterInformation { label: ParameterLabel::LabelOffsets([utf16(&label[..*a]), utf16(&label[..*b])]), documentation: None }).collect::<Vec<_>>();
        let doc = v.get("doc").and_then(Value::as_str).filter(|d| !d.is_empty()).map(|d| Documentation::MarkupContent(MarkupContent { kind: MarkupKind::Markdown, value: d.to_string() }));
        let help = SignatureHelp {
            signatures: vec![SignatureInformation { label, documentation: doc, parameters: Some(parameters), active_parameter: Some(active) }],
            active_signature: Some(0),
            active_parameter: Some(active),
        };
        serde_json::to_value(help).ok()
    }

    // ---- rename -------------------------------------------------------------------

    /// What a rename at `tdp` would rename: the word, if it is the
    /// program's own (not the standard library's or C's).
    fn rename_target(&mut self, tdp: &TextDocumentPositionParams) -> Option<(Range, String, Value)> {
        let path = path_of(&tdp.text_document.uri)?;
        let text = self.text_of(&path);
        let at = byte_of_pos(&text, tdp.position);
        let (a, b) = word_at(&text, at)?;
        let v = self.ask("definition", tdp)?;
        let local = v.get("kind").and_then(Value::as_str) == Some("local");
        let own = local || (v.get("package").and_then(Value::as_str) != Some("std") && !matches!(v.get("kind").and_then(Value::as_str), Some("extern") | Some("module")));
        if !own || v["location"]["file"].as_str() == Some("<prelude>") {
            return None;
        }
        Some((Range { start: pos_of_byte(&text, a), end: pos_of_byte(&text, b) }, text[a..b].to_string(), v))
    }

    fn prepare_rename(&mut self, tdp: &TextDocumentPositionParams) -> Option<Value> {
        let (range, placeholder, _) = self.rename_target(tdp)?;
        serde_json::to_value(PrepareRenameResponse::RangeWithPlaceholder { range, placeholder }).ok()
    }

    fn rename(&mut self, tdp: &TextDocumentPositionParams, new: &str) -> Result<Option<Value>, String> {
        let Some((_, old, def)) = self.rename_target(tdp) else { return Err("only the program's own names can be renamed here".into()) };
        let ok_name = new.chars().next().is_some_and(|c| c.is_alphabetic() || c == '_') && new.chars().all(|c| c.is_alphanumeric() || c == '_');
        if !ok_name {
            return Err(format!("`{new}` is not a name"));
        }
        let mut locs: Vec<Location> = Vec::new();
        if let Some(l) = def.get("location").and_then(|l| self.location(l)) {
            locs.push(l);
        }
        if let Some(refs) = self.ask("references", tdp) {
            locs.extend(refs.as_array().into_iter().flatten().filter_map(|l| self.location(l)));
        }
        let mut changes: HashMap<Url, Vec<TextEdit>> = HashMap::new();
        for loc in locs {
            let Some(p) = path_of(&loc.uri) else { continue };
            let text = self.text_of(&p);
            let (a, b) = (byte_of_pos(&text, loc.range.start), byte_of_pos(&text, loc.range.end));
            // a path (`shapes::area`) renames its last part
            let Some(seen) = text.get(a..b) else { continue };
            let start = if seen.ends_with(old.as_str()) { b - old.len() } else if let Some(i) = seen.find(old.as_str()) { a + i } else { continue };
            let range = Range { start: pos_of_byte(&text, start), end: pos_of_byte(&text, start + old.len()) };
            let edits = changes.entry(loc.uri.clone()).or_default();
            if !edits.iter().any(|e| e.range == range) {
                edits.push(TextEdit { range, new_text: new.to_string() });
            }
        }
        Ok(serde_json::to_value(WorkspaceEdit { changes: Some(changes), ..Default::default() }).ok())
    }

    // ---- the call hierarchy ---------------------------------------------------------

    fn call_item(&self, v: &Value) -> Option<CallHierarchyItem> {
        if v.get("kind").and_then(Value::as_str) != Some("fn") {
            return None;
        }
        let loc = self.location(v.get("location")?)?;
        Some(CallHierarchyItem {
            name: v.get("name")?.as_str()?.to_string(),
            kind: SymbolKind::FUNCTION,
            tags: None,
            detail: v.get("signature").and_then(Value::as_str).map(String::from),
            uri: loc.uri,
            range: loc.range,
            selection_range: loc.range,
            data: Some(json!({ "path": v.get("path").cloned().unwrap_or(Value::Null) })),
        })
    }

    fn call_item_at(&mut self, tdp: &TextDocumentPositionParams) -> Option<CallHierarchyItem> {
        let v = self.ask("definition", tdp)?;
        self.call_item(&v)
    }

    /// The functions of a file, by the line their name is on.
    fn functions_of(&mut self, path: &Path) -> Vec<(u32, Value)> {
        let Some((engine, _)) = self.engine(path) else { return Vec::new() };
        let Ok(v) = engine.query("outline", &json!({ "file": path.display().to_string() })) else { return Vec::new() };
        let mut fns: Vec<(u32, Value)> = v.as_array().into_iter().flatten().filter(|s| s["kind"] == "fn").map(|s| (s["location"]["line"].as_u64().unwrap_or(0) as u32, s.clone())).collect();
        fns.sort_by_key(|(l, _)| *l);
        fns
    }

    fn incoming_calls(&mut self, item: &CallHierarchyItem) -> Option<Value> {
        let path = path_of(&item.uri)?;
        let name = item.data.as_ref().and_then(|d| d["path"].as_str()).map(String::from).unwrap_or(item.name.clone());
        let refs = {
            let (engine, _) = self.engine(&path)?;
            engine.query("references", &json!({ "name": name })).ok()?
        };
        let mut by_caller: Vec<(CallHierarchyItem, Vec<Range>)> = Vec::new();
        for r in refs.as_array().into_iter().flatten() {
            let Some(loc) = self.location(r) else { continue };
            let Some(file) = path_of(&loc.uri) else { continue };
            let line = r["line"].as_u64().unwrap_or(0) as u32;
            let fns = self.functions_of(&file);
            let Some((_, caller)) = fns.iter().rev().find(|(l, _)| *l <= line) else { continue };
            let Some(it) = self.call_item(caller) else { continue };
            match by_caller.iter_mut().find(|(c, _)| c.uri == it.uri && c.range == it.range) {
                Some((_, rs)) => rs.push(loc.range),
                None => by_caller.push((it, vec![loc.range])),
            }
        }
        let calls: Vec<CallHierarchyIncomingCall> = by_caller.into_iter().map(|(from, from_ranges)| CallHierarchyIncomingCall { from, from_ranges }).collect();
        serde_json::to_value(calls).ok()
    }

    fn outgoing_calls(&mut self, item: &CallHierarchyItem) -> Option<Value> {
        let path = path_of(&item.uri)?;
        let text = self.text_of(&path);
        let fns = self.functions_of(&path);
        let start_line = item.range.start.line + 1;
        let end_line = fns.iter().find(|(l, _)| *l > start_line).map(|(l, _)| *l).unwrap_or(u32::MAX);
        let from = byte_of_pos(&text, Position { line: item.range.start.line, character: 0 });
        let to = if end_line == u32::MAX { text.len() } else { byte_of_pos(&text, Position { line: end_line - 1, character: 0 }) };
        let body = &text[from..to.max(from)];
        // every `name(` in the function: what it names
        let mut out: Vec<(CallHierarchyItem, Vec<Range>)> = Vec::new();
        let bytes = body.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            let c = body[i..].chars().next().unwrap_or(' ');
            if c.is_alphabetic() || c == '_' {
                let len: usize = body[i..].chars().take_while(|c| c.is_alphanumeric() || *c == '_' || *c == ':').map(char::len_utf8).sum();
                let after = body[i + len..].trim_start_matches(' ');
                // (not the function's own `fn name(`)
                if after.starts_with('(') && !body[..i].trim_end().ends_with("fn") {
                    let at = from + i + len - 1;
                    let tdp = TextDocumentPositionParams { text_document: TextDocumentIdentifier { uri: item.uri.clone() }, position: pos_of_byte(&text, at) };
                    if let Some(v) = self.ask("definition", &tdp)
                        && let Some(to) = self.call_item(&v)
                    {
                        let range = Range { start: pos_of_byte(&text, from + i), end: pos_of_byte(&text, from + i + len) };
                        match out.iter_mut().find(|(c, _)| c.uri == to.uri && c.range == to.range) {
                            Some((_, rs)) => rs.push(range),
                            None => out.push((to, vec![range])),
                        }
                    }
                }
                i += len.max(1);
            } else {
                i += c.len_utf8();
            }
        }
        let calls: Vec<CallHierarchyOutgoingCall> = out.into_iter().map(|(to, from_ranges)| CallHierarchyOutgoingCall { to, from_ranges }).collect();
        serde_json::to_value(calls).ok()
    }

    // ---- smart selection ----------------------------------------------------------------

    fn selection_ranges(&mut self, uri: &Url, positions: &[Position]) -> Option<Value> {
        let path = path_of(uri)?;
        let text = self.text_of(&path);
        let out: Vec<SelectionRange> = positions.iter().map(|p| selection_chain(&text, byte_of_pos(&text, *p))).collect();
        serde_json::to_value(out).ok()
    }

    fn outline(&mut self, uri: &Url) -> Option<Value> {
        let path = path_of(uri)?;
        let text = self.text_of(&path);
        let (engine, _) = self.engine(&path)?;
        let v = engine.query("outline", &json!({ "file": path.display().to_string() })).ok()?;
        #[allow(deprecated)]
        let syms: Vec<DocumentSymbol> = v
            .as_array()?
            .iter()
            // what the file declares, not what it imports from C headers
            .filter(|x| x.get("header").is_none())
            .map(|x| {
                let l = &x["location"];
                let n = |k: &str| l.get(k).and_then(Value::as_u64).unwrap_or(1) as u32;
                let range = Range { start: to_lsp(&text, n("line"), n("column")), end: to_lsp(&text, n("end_line"), n("end_column")) };
                DocumentSymbol {
                    name: x["name"].as_str().unwrap_or("").to_string(),
                    detail: x["signature"].as_str().map(|s| s.lines().next().unwrap_or("").to_string()),
                    kind: symbol_kind(x["kind"].as_str().unwrap_or("")),
                    tags: None,
                    deprecated: None,
                    range,
                    selection_range: range,
                    children: None,
                }
            })
            .collect();
        serde_json::to_value(DocumentSymbolResponse::Nested(syms)).ok()
    }

    fn workspace_symbols(&mut self, query: &str) -> Option<Value> {
        let keys: Vec<PathBuf> = self.engines.keys().cloned().collect();
        let mut out = Vec::new();
        for k in keys {
            let v = self.engines.get_mut(&k)?.query("symbols", &json!({ "query": query, "limit": 200 })).ok()?;
            for x in v["symbols"].as_array().into_iter().flatten() {
                if x["package"].as_str() == Some("std") && query.is_empty() {
                    continue;
                }
                let Some(loc) = self.location(&x["location"]) else { continue };
                #[allow(deprecated)]
                out.push(SymbolInformation {
                    name: x["path"].as_str().unwrap_or("").to_string(),
                    kind: symbol_kind(x["kind"].as_str().unwrap_or("")),
                    tags: None,
                    deprecated: None,
                    location: loc,
                    container_name: x["package"].as_str().map(String::from),
                });
            }
        }
        serde_json::to_value(out).ok()
    }

    /// Where code runs, as labels after the code they are about: `∥ parallel`,
    /// `▣ GPU`, `◷ task`, `→ in order`, with the reason on hover. Only while
    /// the program checks, so the labels never point at code that moved.
    fn where_hints(&mut self, uri: &Url, range: Range) -> Option<Value> {
        let path = path_of(uri)?;
        let text = self.text_of(&path);
        let (engine, _) = self.engine(&path)?;
        if engine.errors().is_some() {
            return serde_json::to_value(Vec::<InlayHint>::new()).ok();
        }
        let v = engine.query("where", &json!({ "file": path.display().to_string() })).ok()?;
        let types = engine.query("types", &json!({ "file": path.display().to_string() })).unwrap_or(Value::Null);
        // one label per piece of code: its kinds together
        let mut at: Vec<(Position, Vec<String>, Vec<String>)> = Vec::new();
        for d in v.as_array()? {
            let l = &d["location"];
            let n = |k: &str| l.get(k).and_then(Value::as_u64).unwrap_or(1) as u32;
            let pos = to_lsp(&text, n("end_line"), n("end_column"));
            if pos.line < range.start.line || pos.line > range.end.line {
                continue;
            }
            let label = match d["kind"].as_str().unwrap_or("") {
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
            let why = d["why"].as_str().unwrap_or("").to_string();
            match at.iter_mut().find(|(p, ..)| *p == pos) {
                Some((_, labels, whys)) => {
                    if !labels.iter().any(|l| l == label) {
                        labels.push(label.to_string());
                        whys.push(why);
                    }
                }
                None => at.push((pos, vec![label.to_string()], vec![why])),
            }
        }
        // the inferred type of each `let` without an annotation
        let mut type_hints: Vec<InlayHint> = Vec::new();
        for t in types.as_array().into_iter().flatten() {
            let pos = to_lsp(&text, t["line"].as_u64().unwrap_or(1) as u32, t["column"].as_u64().unwrap_or(1) as u32);
            if pos.line < range.start.line || pos.line > range.end.line {
                continue;
            }
            type_hints.push(InlayHint {
                position: pos,
                label: InlayHintLabel::String(format!(": {}", t["type"].as_str().unwrap_or(""))),
                kind: Some(InlayHintKind::TYPE),
                text_edits: Some(vec![TextEdit { range: Range { start: pos, end: pos }, new_text: format!(": {}", t["type"].as_str().unwrap_or("")) }]),
                tooltip: None,
                padding_left: None,
                padding_right: None,
                data: None,
            });
        }
        let hints: Vec<InlayHint> = at
            .into_iter()
            .map(|(position, labels, whys)| InlayHint {
                position,
                label: InlayHintLabel::String(labels.join(" · ")),
                kind: None,
                text_edits: None,
                tooltip: Some(InlayHintTooltip::MarkupContent(MarkupContent { kind: MarkupKind::Markdown, value: whys.join("\n\n") + "\n\n`mimic why` lists every decision" })),
                padding_left: Some(true),
                padding_right: None,
                data: None,
            })
            .chain(type_hints)
            .collect();
        serde_json::to_value(hints).ok()
    }

    fn semantic_tokens(&mut self, uri: &Url) -> Option<Value> {
        let path = path_of(uri)?;
        let text = self.text_of(&path);
        let (engine, _) = self.engine(&path)?;
        let v = engine.query("foreign", &json!({ "file": path.display().to_string() })).ok()?;
        let mut toks: Vec<(u32, u32, u32, u32)> = v
            .as_array()?
            .iter()
            .filter_map(|t| {
                let line = t["line"].as_u64()? as u32;
                let col = t["column"].as_u64()? as u32;
                let len_chars = t["length"].as_u64()? as u32;
                let start = to_lsp(&text, line, col);
                let end = to_lsp(&text, line, col + len_chars);
                let kind = match t["kind"].as_str()? {
                    "namespace" => 0,
                    "function" => 1,
                    _ => 2,
                };
                Some((start.line, start.character, end.character.saturating_sub(start.character), kind))
            })
            .collect();
        toks.sort();
        toks.dedup();
        let mut data = Vec::new();
        let (mut pl, mut pc) = (0, 0);
        for (l, c, len, kind) in toks {
            let dl = l - pl;
            let dc = if dl == 0 { c - pc } else { c };
            data.push(SemanticToken { delta_line: dl, delta_start: dc, length: len, token_type: kind, token_modifiers_bitset: 1 });
            pl = l;
            pc = c;
        }
        serde_json::to_value(SemanticTokensResult::Tokens(SemanticTokens { result_id: None, data })).ok()
    }
}

fn from<T: serde::de::DeserializeOwned>(v: Value) -> Result<T, String> {
    serde_json::from_value(v).map_err(|e| e.to_string())
}

fn code_of(d: &Diagnostic) -> String {
    match &d.code {
        Some(NumberOrString::String(s)) => s.clone(),
        Some(NumberOrString::Number(n)) => n.to_string(),
        None => String::new(),
    }
}

fn symbol_kind(k: &str) -> SymbolKind {
    match k {
        "type" => SymbolKind::STRUCT,
        "variant" => SymbolKind::ENUM_MEMBER,
        "module" => SymbolKind::MODULE,
        _ => SymbolKind::FUNCTION,
    }
}

/// (glyph, word, digraph, meaning) of every primitive and syntax glyph.
fn glyph_table() -> Vec<(&'static str, &'static str, Option<&'static str>, &'static str)> {
    let mut v: Vec<_> = PRIMS.iter().map(|p| (p.glyph, p.word, p.digraph, p.doc)).collect();
    v.extend(SYNTAX_GLYPHS.iter().map(|g| (g.glyph, g.word, g.digraph, g.doc)));
    v
}

/// Is the end of `before` (one line) inside a string literal?
fn inside_string(before: &str) -> bool {
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

/// Hover text for the glyph, or primitive word, at byte `at`.
fn glyph_hover(text: &str, at: usize) -> Option<String> {
    let at = text.floor_char_boundary(at.min(text.len()));
    let rest = &text[at..];
    let back = &text[text.floor_char_boundary(at.saturating_sub(4))..at];
    let word_start = text[..at].rfind(|c: char| !(c.is_alphanumeric() || c == '_')).map_or(0, |i| i + text[i..].chars().next().map_or(1, char::len_utf8));
    let word_end = at + rest.find(|c: char| !(c.is_alphanumeric() || c == '_')).unwrap_or(rest.len());
    let word = &text[word_start..word_end];
    for (glyph, w, digraph, doc) in glyph_table() {
        let hit = (!word.is_empty() && w == word)
            || rest.starts_with(glyph)
            || (glyph.chars().count() == 2 && back.ends_with(glyph.chars().next().unwrap()) && rest.starts_with(glyph.chars().nth(1).unwrap()));
        if hit {
            let typed = digraph.map(|d| format!(" · type `{d}`")).unwrap_or_default();
            return Some(format!("**{glyph}** `{w}`{typed}\n\n{doc}\n\n`mimic man {w}`"));
        }
    }
    None
}

/// The LSP diagnostics of a report, by file, with their fixes as edits.
type FixEdits = Vec<(String, HashMap<Url, Vec<TextEdit>>)>;

/// Brackets opened and not closed in `s` (strings skipped).
fn open_brackets_of(s: &str) -> i32 {
    let mut depth = 0;
    let mut inside = false;
    for c in s.chars() {
        match c {
            '"' => inside = !inside,
            '(' | '[' | '{' if !inside => depth += 1,
            ')' | ']' | '}' if !inside => depth -= 1,
            _ => {}
        }
        if depth < 0 {
            return depth;
        }
    }
    depth
}

/// The call whose argument list `at` is in: the byte of its `(` and which
/// argument (counting commas at its level).
fn open_call(text: &str, at: usize) -> Option<(usize, u32)> {
    let mut depth = 0i32;
    let mut commas = 0u32;
    let from = at.saturating_sub(4000);
    let mut inside = false;
    for (i, c) in text[from..at.min(text.len())].char_indices().rev() {
        let i = from + i;
        match c {
            '"' => inside = !inside,
            _ if inside => {}
            ')' | ']' | '}' => depth += 1,
            '(' if depth == 0 => return Some((i, commas)),
            '(' | '[' | '{' => {
                depth -= 1;
                if depth < 0 {
                    return None;
                }
            }
            ',' if depth == 0 => commas += 1,
            _ => {}
        }
    }
    None
}

/// The byte ranges of the parameters in a signature `fn f(a: T, b: U) -> R`.
fn param_spans(sig: &str) -> Vec<(usize, usize)> {
    let Some(open) = sig.find('(') else { return Vec::new() };
    let mut out = Vec::new();
    let mut depth = 0;
    let mut start = open + 1;
    for (i, c) in sig[open + 1..].char_indices() {
        let i = open + 1 + i;
        match c {
            '(' | '[' | '{' => depth += 1,
            ')' if depth == 0 => {
                if !sig[start..i].trim().is_empty() {
                    out.push((start + (sig[start..i].len() - sig[start..i].trim_start().len()), i));
                }
                break;
            }
            ')' | ']' | '}' => depth -= 1,
            ',' if depth == 0 => {
                out.push((start + (sig[start..i].len() - sig[start..i].trim_start().len()), i));
                start = i + 1;
            }
            _ => {}
        }
    }
    out
}

/// The name at `at` (letters, digits, `_`).
fn word_at(text: &str, at: usize) -> Option<(usize, usize)> {
    let ok = |c: char| c.is_alphanumeric() || c == '_';
    let a = at - text[..at.min(text.len())].chars().rev().take_while(|c| ok(*c)).map(char::len_utf8).sum::<usize>();
    let b = at + text[at.min(text.len())..].chars().take_while(|c| ok(*c)).map(char::len_utf8).sum::<usize>();
    (a < b).then_some((a, b))
}

/// From the word at `at` outwards: inside brackets, the brackets, the line,
/// and so on to the whole file.
fn selection_chain(text: &str, at: usize) -> SelectionRange {
    let mut spans: Vec<(usize, usize)> = Vec::new();
    if let Some(w) = word_at(text, at) {
        spans.push(w);
    }
    let (mut lo, mut hi) = spans.last().copied().unwrap_or((at, at));
    loop {
        // the brackets around [lo, hi)
        let mut depth = 0i32;
        let mut open = None;
        for (i, c) in text[..lo].char_indices().rev() {
            match c {
                ')' | ']' | '}' => depth += 1,
                '(' | '[' | '{' if depth == 0 => {
                    open = Some(i);
                    break;
                }
                '(' | '[' | '{' => depth -= 1,
                _ => {}
            }
        }
        let Some(o) = open else { break };
        let want = match &text[o..o + 1] {
            "(" => ')',
            "[" => ']',
            _ => '}',
        };
        let mut depth = 0i32;
        let mut close = None;
        for (i, c) in text[hi..].char_indices() {
            match c {
                '(' | '[' | '{' => depth += 1,
                c2 if c2 == want && depth == 0 => {
                    close = Some(hi + i);
                    break;
                }
                ')' | ']' | '}' => depth -= 1,
                _ => {}
            }
        }
        let Some(c) = close else { break };
        let inner = (o + 1, c);
        if inner != (lo, hi) && inner.0 <= lo && inner.1 >= hi {
            spans.push(inner);
        }
        // the line(s) the brackets are on, then the brackets
        let line_lo = text[..o].rfind('\n').map_or(0, |i| i + 1);
        let line_hi = text[c..].find('\n').map_or(text.len(), |i| c + i);
        spans.push((o, c + 1));
        let trimmed = (line_lo + (text[line_lo..o].len() - text[line_lo..o].trim_start().len()), line_hi);
        if trimmed != (o, c + 1) {
            spans.push(trimmed);
        }
        lo = trimmed.0;
        hi = trimmed.1;
    }
    spans.push((0, text.len()));
    spans.dedup();
    let mut range: Option<SelectionRange> = None;
    for (a, b) in spans.into_iter().rev() {
        range = Some(SelectionRange { range: Range { start: pos_of_byte(text, a), end: pos_of_byte(text, b) }, parent: range.map(Box::new) });
    }
    range.unwrap()
}

fn diagnostics(r: &Report, engine_key: &Path) -> Vec<(PathBuf, Diagnostic, FixEdits)> {
    let mut out = Vec::new();
    let fallback = if engine_key.is_dir() { engine_key.join("mim.mim") } else { engine_key.to_path_buf() };
    for e in &r.entries {
        let d = &e.diag;
        let (path, src) = match e.file.and_then(|f| r.files.get(f)) {
            Some(s) => (s.path.clone().unwrap_or_else(|| fallback.clone()), s.src.as_str()),
            None => (fallback.clone(), ""),
        };
        let path = driver::canonical(&path);
        let range = if src.is_empty() { Range::default() } else { Range { start: pos_of_byte(src, d.span.start), end: pos_of_byte(src, d.span.end.max(d.span.start)) } };
        let mut message = d.msg.clone();
        for n in &d.notes {
            message += &format!("\nnote: {n}");
        }
        for h in &d.help {
            message += &format!("\nhelp: {h}");
        }
        let related: Vec<DiagnosticRelatedInformation> = d
            .labels
            .iter()
            .filter(|l| !l.msg.is_empty())
            .filter_map(|l| {
                let (lp, lsrc) = match l.file.and_then(|f| r.files.get(f)) {
                    Some(s) => (s.path.clone()?, s.src.as_str()),
                    None => (path.clone(), src),
                };
                Some(DiagnosticRelatedInformation {
                    location: Location { uri: url_of(&lp)?, range: Range { start: pos_of_byte(lsrc, l.span.start), end: pos_of_byte(lsrc, l.span.end) } },
                    message: l.msg.clone(),
                })
            })
            .collect();
        let diag = Diagnostic {
            range,
            severity: Some(if d.severity == mimic_syntax::Severity::Error { DiagnosticSeverity::ERROR } else { DiagnosticSeverity::WARNING }),
            code: Some(NumberOrString::String(d.code.id.to_string())),
            source: Some("mimic".into()),
            message,
            related_information: (!related.is_empty()).then_some(related),
            ..Default::default()
        };
        let mut fixes = Vec::new();
        for fx in &d.fixes {
            let mut changes: HashMap<Url, Vec<TextEdit>> = HashMap::new();
            for ed in &fx.edits {
                let (ep, esrc) = match ed.file.and_then(|f| r.files.get(f)) {
                    Some(s) => (s.path.clone().unwrap_or_else(|| path.clone()), s.src.as_str()),
                    None => (path.clone(), src),
                };
                let Some(uri) = url_of(&ep) else { continue };
                changes.entry(uri).or_default().push(TextEdit { range: Range { start: pos_of_byte(esrc, ed.span.start), end: pos_of_byte(esrc, ed.span.end) }, new_text: ed.text.clone() });
            }
            if !changes.is_empty() {
                fixes.push((fx.msg.clone(), changes));
            }
        }
        out.push((path, diag, fixes));
    }
    out
}
