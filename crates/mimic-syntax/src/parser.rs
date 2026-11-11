//! Recursive-descent / precedence-climbing parser.
//!
//! Precedence, lowest to highest:
//!
//! | level      | forms                                         |
//! |------------|-----------------------------------------------|
//! | pipeline   | `a ▷ f`  `a |> f(b)`  (may continue next line) |
//! | or         | `∨` `or`                                      |
//! | and        | `∧` `and`                                     |
//! | compare    | `== ≠ < ≤ > ≥`                                |
//! | additive   | `+ -`                                         |
//! | multiply   | `× ÷ %`                                       |
//! | unary      | `-x` `¬x`                                     |
//! | train      | postfix monadic prims `x ⌽`, dyadic `x ↑ 3`   |
//! | prefix     | `⍳ 10`, `⌽ ⍳ 5`, `take(xs, 3)`                |
//! | postfix    | `f(x)` `xs[i]` (no space before `(`/`[`)       |
//! | primary    | literals, paths, `[..]`, `( )`, `if`, blocks  |
//!
//! Trains read left to right: `⍳ 10 ⌽ ↑ 3 +/` = sum(take(reverse(iota 10), 3)).

use crate::ast::*;
use crate::glyphs::Prim;
use crate::codes as C;
use crate::lexer::{StrPart, Tok, Token, lex};
use crate::{Diag, Span};

/// Type and variant names start with an uppercase letter; that one rule keeps
/// `Inc` (a variant) apart from `x` (a variable) and `User { … }` (a record)
/// apart from `if flag { … }` (a block).
pub fn is_upper(name: &str) -> bool {
    name.chars().next().is_some_and(|c| c.is_uppercase())
}

/// Give every node of a parsed `{…}` interpolation the span of its string.
fn respan(e: &mut Expr, span: Span) {
    e.span = span;
    fn blk(b: &mut Block, span: Span) {
        b.span = span;
        for s in &mut b.stmts {
            match s {
                Stmt::Let { value, span: sp, .. } => {
                    *sp = span;
                    respan(value, span);
                }
                Stmt::Assign { index, value, span: sp, .. } => {
                    *sp = span;
                    if let Some(i) = index {
                        respan(i, span);
                    }
                    respan(value, span);
                }
                Stmt::Expr(x) => respan(x, span),
                Stmt::Return(x, sp) => {
                    *sp = span;
                    if let Some(x) = x {
                        respan(x, span);
                    }
                }
                Stmt::LetTuple { value, span: sp, .. } => {
                    *sp = span;
                    respan(value, span);
                }
                Stmt::LetElse { value, els, span: sp, .. } => {
                    *sp = span;
                    respan(value, span);
                    blk(els, span);
                }
            }
        }
        if let Some(t) = &mut b.tail {
            respan(t, span);
        }
    }
    match &mut e.kind {
        ExprKind::Array(xs) | ExprKind::Tuple(xs) => xs.iter_mut().for_each(|x| respan(x, span)),
        ExprKind::Call { callee, args } => {
            respan(callee, span);
            args.iter_mut().for_each(|x| respan(x, span));
        }
        ExprKind::Index { base, index } => {
            respan(base, span);
            respan(index, span);
        }
        ExprKind::Unary { expr, .. } | ExprKind::Lambda { body: expr, .. } | ExprKind::Field { base: expr, .. } | ExprKind::Named { value: expr, .. } => respan(expr, span),
        ExprKind::Binary { lhs, rhs, .. } => {
            respan(lhs, span);
            respan(rhs, span);
        }
        ExprKind::Prim { args, .. } => args.iter_mut().for_each(|x| respan(x, span)),
        ExprKind::If { cond, then, els } => {
            respan(cond, span);
            blk(then, span);
            if let Some(x) = els {
                respan(x, span);
            }
        }
        ExprKind::Block(b) | ExprKind::Unsafe(b) => blk(b, span),
        ExprKind::Record { fields, .. } => fields.iter_mut().for_each(|(_, x, sp)| {
            *sp = span;
            respan(x, span)
        }),
        ExprKind::With { base, fields } => {
            respan(base, span);
            fields.iter_mut().for_each(|(_, x, sp)| {
                *sp = span;
                respan(x, span)
            });
        }
        ExprKind::Match { scrut, arms } => {
            respan(scrut, span);
            arms.iter_mut().for_each(|a| {
                a.span = span;
                respan(&mut a.body, span)
            });
        }
        ExprKind::Interp(parts) => parts.iter_mut().for_each(|p| {
            if let InterpPart::Expr(x) = p {
                respan(x, span)
            }
        }),
        _ => {}
    }
}

/// Build a primitive application. `max`, `min` and `abs` are ordinary
/// operators underneath, so they stay unboxed fast paths everywhere.
fn prim_expr(prim: Prim, mut args: Vec<Expr>, span: Span) -> Expr {
    let kind = match (prim, args.len()) {
        (Prim::Max | Prim::Min, 2) => {
            let rhs = args.pop().unwrap();
            let lhs = args.pop().unwrap();
            let op = if prim == Prim::Max { BinOp::Max } else { BinOp::Min };
            ExprKind::Binary { op, lhs: Box::new(lhs), rhs: Box::new(rhs) }
        }
        (Prim::Abs, 1) => ExprKind::Unary { op: UnOp::Abs, expr: Box::new(args.pop().unwrap()) },
        _ => ExprKind::Prim { prim, args },
    };
    Expr { kind, span }
}

pub fn parse_file(src: &str) -> Result<File, Diag> {
    let toks = lex(src)?;
    let mut p = Parser { toks, pos: 0, nl_ignore: 0 };
    p.file()
}

/// Parse a single expression (used by tests and tooling).
pub fn parse_expr(src: &str) -> Result<Expr, Diag> {
    let toks = lex(src)?;
    let mut p = Parser { toks, pos: 0, nl_ignore: 0 };
    p.skip_newlines();
    let e = p.expr()?;
    p.skip_newlines();
    p.expect(&Tok::Eof)?;
    Ok(e)
}

fn upper_first(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

pub(crate) struct Parser {
    pub(crate) toks: Vec<Token>,
    pub(crate) pos: usize,
    /// >0 inside parens/brackets: newlines are insignificant.
    pub(crate) nl_ignore: usize,
}

type PResult<T> = Result<T, Diag>;

impl Parser {
    // ---- token helpers -------------------------------------------------

    pub(crate) fn peek_tok(&mut self) -> &Token {
        if self.nl_ignore > 0 {
            while self.toks[self.pos].tok == Tok::Newline {
                self.pos += 1;
            }
        }
        &self.toks[self.pos]
    }
    pub(crate) fn peek(&mut self) -> &Tok {
        &self.peek_tok().tok
    }
    pub(crate) fn span(&mut self) -> Span {
        self.peek_tok().span
    }
    pub(crate) fn next(&mut self) -> Token {
        let t = self.peek_tok().clone();
        if t.tok != Tok::Eof {
            self.pos += 1;
        }
        t
    }
    pub(crate) fn at(&mut self, t: &Tok) -> bool {
        self.peek() == t
    }
    pub(crate) fn eat(&mut self, t: &Tok) -> bool {
        if self.at(t) {
            self.next();
            true
        } else {
            false
        }
    }
    pub(crate) fn expect(&mut self, t: &Tok) -> PResult<Span> {
        if self.at(t) {
            Ok(self.next().span)
        } else {
            let found = self.peek_tok().clone();
            Err(Diag::new(&C::UNEXPECTED_TOKEN, format!("expected {}, found {}", t.describe(), found.tok.describe()), found.span))
        }
    }
    pub(crate) fn skip_newlines(&mut self) {
        while matches!(self.toks[self.pos].tok, Tok::Newline | Tok::Semi) {
            self.pos += 1;
        }
    }
    fn ident(&mut self) -> PResult<(String, Span)> {
        let t = self.next();
        match t.tok {
            Tok::Ident(s) => Ok((s, t.span)),
            Tok::Prim(p) => Err(Diag::new(&C::PRIMITIVE_AS_NAME, 
                format!("`{}` is a built-in primitive ({}) and cannot be used as a name", p.info().word, p.info().glyph),
                t.span,
            )),
            other => Err(Diag::new(&C::UNEXPECTED_TOKEN, format!("expected identifier, found {}", other.describe()), t.span)),
        }
    }
    /// A name where built-in words are allowed too: C function names in
    /// `extern` blocks (`fn abs(…)`) and path segments after `::`.
    fn name_or_word(&mut self) -> PResult<(String, Span)> {
        if let Tok::Prim(p) = *self.peek() {
            let t = self.next();
            return Ok((p.info().word.to_string(), t.span));
        }
        self.ident()
    }

    /// The next token is directly attached to the previous one (no whitespace).
    fn adjacent(&mut self) -> bool {
        !self.peek_tok().space_before
    }

    // ---- items ---------------------------------------------------------

    fn file(&mut self) -> PResult<File> {
        self.skip_newlines();
        let start = self.span();
        let is_pub = self.eat(&Tok::Pub);
        if !self.at(&Tok::Module) {
            let s = self.span();
            return Err(Diag::new(&C::MODULE_HEADER, "the first line of every .mim file must be `module <name>` (or `§ <name>`)", s));
        }
        self.next();
        let (name, _) = self.ident()?;
        if self.at(&Tok::LBrace) {
            let s = self.span();
            return Err(Diag::new(&C::MODULE_HEADER, "the file's header module takes no braces; write `module name` on the first line", s));
        }
        let items = self.items(true)?;
        let end = self.span();
        Ok(File { module: ModuleDecl { name, is_pub, items, span: start, full: start.to(end) } })
    }

    fn items(&mut self, top: bool) -> PResult<Vec<Item>> {
        let mut items = Vec::new();
        loop {
            self.skip_newlines();
            match self.peek() {
                Tok::Eof if top => break,
                Tok::RBrace if !top => break,
                Tok::Eof => {
                    let s = self.span();
                    return Err(Diag::new(&C::UNEXPECTED_TOKEN, "unclosed module: expected `}`", s));
                }
                _ => items.push(self.item()?),
            }
        }
        Ok(items)
    }

    fn item(&mut self) -> PResult<Item> {
        let start = self.span();
        let is_pub = self.eat(&Tok::Pub);
        match self.peek().clone() {
            Tok::Type => Ok(Item::Type(self.type_decl(is_pub, start)?)),
            Tok::Fn => Ok(Item::Fn(self.fn_decl(is_pub, start)?)),
            Tok::Module => {
                self.next();
                let (name, _) = self.ident()?;
                self.expect(&Tok::LBrace)?;
                let items = self.items(false)?;
                let end = self.expect(&Tok::RBrace)?;
                Ok(Item::Module(ModuleDecl { name, is_pub, items, span: start, full: start.to(end) }))
            }
            Tok::Use => {
                if is_pub {
                    return Err(Diag::new(&C::UNSUPPORTED_SYNTAX, "`pub use` (re-exports) is not supported yet", start));
                }
                self.next();
                let path = self.path_segments()?;
                let alias = if self.eat(&Tok::As) { Some(self.ident()?.0) } else { None };
                Ok(Item::Use(UseDecl { path, alias, span: start }))
            }
            Tok::Extern | Tok::Trusted => {
                let trusted = self.eat(&Tok::Trusted);
                self.expect(&Tok::Extern)?;
                let abi = match self.next() {
                    Token { tok: Tok::Str(s), .. } => s,
                    t => return Err(Diag::new(&C::UNEXPECTED_TOKEN, "expected ABI string, e.g. extern \"C\"", t.span)),
                };
                if abi != "C" {
                    return Err(Diag::new(&C::UNSUPPORTED_SYNTAX, format!("unsupported ABI \"{abi}\"; only \"C\" is supported"), start));
                }
                // `extern "C" fn f(x: f64) -> f64 { … }`: a Mimic function exported to C
                if self.at(&Tok::Fn) {
                    if trusted {
                        return Err(Diag::new(&C::UNEXPECTED_TOKEN, "`trusted` applies to imported C functions, not to exported ones", start));
                    }
                    let mut f = self.fn_decl(is_pub, start)?;
                    f.export_c = true;
                    return Ok(Item::Fn(f));
                }
                let mut fns = Vec::new();
                let mut includes = Vec::new();
                // `extern "C" include "math.h"`: a whole header, no braces
                if let Some(inc) = self.include_directive()? {
                    includes.push(inc);
                    return Ok(Item::Extern(ExternBlock { abi, trusted, fns, includes, is_pub, span: start }));
                }
                self.expect(&Tok::LBrace)?;
                loop {
                    self.skip_newlines();
                    if self.eat(&Tok::RBrace) {
                        break;
                    }
                    if let Some(inc) = self.include_directive()? {
                        includes.push(inc);
                        continue;
                    }
                    let fstart = self.span();
                    let fpub = self.eat(&Tok::Pub) || is_pub;
                    self.expect(&Tok::Fn)?;
                    let (name, _) = self.name_or_word()?;
                    self.expect(&Tok::LParen)?;
                    self.nl_ignore += 1;
                    let mut params = Vec::new();
                    let mut variadic = false;
                    while !self.at(&Tok::RParen) {
                        // `...`: C varargs, after the fixed parameters
                        if self.at(&Tok::Dot) {
                            for _ in 0..3 {
                                self.expect(&Tok::Dot)?;
                            }
                            variadic = true;
                            break;
                        }
                        let (pn, ps) = self.ident()?;
                        if !self.eat(&Tok::Colon) {
                            self.nl_ignore -= 1;
                            return Err(Diag::new(&C::NOT_C_TYPE, "extern parameters need a C type, e.g. `x: f64`", ps));
                        }
                        params.push((pn, self.ty()?));
                        if !self.eat(&Tok::Comma) {
                            break;
                        }
                    }
                    self.nl_ignore -= 1;
                    self.expect(&Tok::RParen)?;
                    let ret = if self.eat(&Tok::Arrow) { Some(self.ty()?) } else { None };
                    fns.push(ExternFn { name, is_pub: fpub, params, variadic, ret, span: fstart });
                }
                Ok(Item::Extern(ExternBlock { abi, trusted, fns, includes, is_pub, span: start }))
            }
            other => Err(Diag::new(&C::UNEXPECTED_TOKEN, 
                format!("expected an item (`fn`, `module`, `use`, `extern`), found {}", other.describe()),
                start,
            )),
        }
    }

    /// `include "header.h"` inside (or right after) `extern "C"`.
    fn include_directive(&mut self) -> PResult<Option<(String, Span)>> {
        if !matches!(self.peek(), Tok::Ident(w) if w == "include") {
            return Ok(None);
        }
        self.next();
        match self.next() {
            Token { tok: Tok::Str(h), span, .. } => Ok(Some((h, span))),
            t => Err(Diag::new(&C::UNEXPECTED_TOKEN, "expected a header name in quotes, e.g. include \"math.h\"", t.span)),
        }
    }

    fn type_decl(&mut self, is_pub: bool, start: Span) -> PResult<TypeDecl> {
        self.expect(&Tok::Type)?;
        let (name, nspan) = self.ident()?;
        if !is_upper(&name) {
            let fixed = upper_first(&name);
            return Err(Diag::new(&C::NAME_CASE, format!("type names start with an uppercase letter: `{fixed}`"), nspan).fix(
                format!("rename it `{fixed}` (and its uses)"),
                nspan,
                fixed.clone(),
                crate::Applicability::MaybeIncorrect,
            ));
        }
        self.expect(&Tok::Assign)?;
        // record: `{ field: Type, … }`
        if self.at(&Tok::LBrace) {
            self.next();
            self.nl_ignore += 1;
            let r = (|| {
                let mut fields = Vec::new();
                while !self.at(&Tok::RBrace) {
                    let (f, fs) = self.name_or_word()?;
                    self.expect(&Tok::Colon)?;
                    let t = self.ty()?;
                    if fields.iter().any(|(n, _, _): &(String, Type, Span)| *n == f) {
                        return Err(Diag::new(&C::DUPLICATE_ENTRY, format!("field `{f}` is declared twice"), fs));
                    }
                    fields.push((f, t, fs));
                    self.eat(&Tok::Comma);
                }
                Ok(fields)
            })();
            self.nl_ignore -= 1;
            let fields = r?;
            self.expect(&Tok::RBrace)?;
            return Ok(TypeDecl { name, is_pub, kind: TypeKind::Record(fields), span: start });
        }
        // union: `A | B(T, T) | C`, one variant per line allowed
        let mut variants = Vec::new();
        let skip_to_bar = |p: &mut Self| {
            let save = p.pos;
            while p.toks[p.pos].tok == Tok::Newline {
                p.pos += 1;
            }
            if p.toks[p.pos].tok == Tok::Bar {
                p.pos += 1;
                true
            } else {
                p.pos = save;
                false
            }
        };
        skip_to_bar(self);
        loop {
            let (v, vs) = self.ident()?;
            if !is_upper(&v) {
                let fixed = upper_first(&v);
                return Err(Diag::new(&C::NAME_CASE, format!("variant names start with an uppercase letter: `{fixed}`"), vs).fix(
                    format!("rename it `{fixed}` (and its uses)"),
                    vs,
                    fixed.clone(),
                    crate::Applicability::MaybeIncorrect,
                ));
            }
            let mut fields = Vec::new();
            if self.at(&Tok::LParen) {
                self.next();
                self.nl_ignore += 1;
                let r = (|| {
                    while !self.at(&Tok::RParen) {
                        fields.push(self.ty()?);
                        if !self.eat(&Tok::Comma) {
                            break;
                        }
                    }
                    Ok(())
                })();
                self.nl_ignore -= 1;
                r?;
                self.expect(&Tok::RParen)?;
            }
            if variants.iter().any(|x: &Variant| x.name == v) {
                return Err(Diag::new(&C::DUPLICATE_ENTRY, format!("variant `{v}` is declared twice"), vs));
            }
            variants.push(Variant { name: v, fields, span: vs });
            if !skip_to_bar(self) {
                break;
            }
        }
        Ok(TypeDecl { name, is_pub, kind: TypeKind::Union(variants), span: start })
    }

    fn fn_decl(&mut self, is_pub: bool, start: Span) -> PResult<FnDecl> {
        self.expect(&Tok::Fn)?;
        let (name, _) = self.ident()?;
        self.expect(&Tok::LParen)?;
        self.nl_ignore += 1;
        let mut params = Vec::new();
        while !self.at(&Tok::RParen) {
            if self.at(&Tok::Mut) {
                let s = self.span();
                self.nl_ignore -= 1;
                return Err(Diag::new(&C::PARAM_RULES, "parameters are immutable; bind a `let mut` copy inside the body", s));
            }
            let (pn, ps) = self.ident()?;
            let ty = if self.eat(&Tok::Colon) { Some(self.ty()?) } else { None };
            let default = if self.eat(&Tok::Assign) { Some(self.expr()?) } else { None };
            if default.is_none() && params.iter().any(|p: &Param| p.default.is_some()) {
                self.nl_ignore -= 1;
                return Err(Diag::new(&C::PARAM_RULES, "parameters without a default must come before those with one", ps));
            }
            params.push(Param { name: pn, ty, default, span: ps });
            if !self.eat(&Tok::Comma) {
                break;
            }
        }
        self.nl_ignore -= 1;
        self.expect(&Tok::RParen)?;
        let ret = if self.eat(&Tok::Arrow) { Some(self.ty()?) } else { None };
        let body = self.block()?;
        Ok(FnDecl { name, is_pub, params, ret, body, export_c: false, span: start })
    }

    fn ty(&mut self) -> PResult<Type> {
        let s = self.span();
        if self.eat(&Tok::LBracket) {
            let inner = self.ty()?;
            self.expect(&Tok::RBracket)?;
            return Ok(Type::Array(Box::new(inner), s));
        }
        let (n, _) = self.ident()?;
        if n == "ptr" {
            self.expect(&Tok::LBracket)?;
            let inner = self.ty()?;
            self.expect(&Tok::RBracket)?;
            return Ok(Type::Ptr(Box::new(inner), s));
        }
        Ok(Type::Named(n, s))
    }

    fn path_segments(&mut self) -> PResult<Vec<String>> {
        let mut segs = vec![self.ident()?.0];
        while self.eat(&Tok::PathSep) {
            segs.push(self.name_or_word()?.0);
        }
        Ok(segs)
    }

    // ---- blocks & statements ------------------------------------------

    fn block(&mut self) -> PResult<Block> {
        let start = self.expect(&Tok::LBrace)?;
        // Newlines are significant inside blocks even if the block is inside parens.
        let saved = std::mem::replace(&mut self.nl_ignore, 0);
        let r = self.block_body(start);
        self.nl_ignore = saved;
        r
    }

    fn block_body(&mut self, start: Span) -> PResult<Block> {
        let mut stmts = Vec::new();
        let mut tail = None;
        loop {
            self.skip_newlines();
            if self.at(&Tok::RBrace) {
                break;
            }
            let stmt = self.stmt()?;
            // statement terminator
            let terminated_by_semi = self.at(&Tok::Semi);
            match self.peek() {
                Tok::Newline | Tok::Semi => {
                    self.next();
                }
                Tok::RBrace => {}
                other => {
                    let d = other.describe();
                    let s = self.span();
                    return Err(Diag::new(&C::UNEXPECTED_TOKEN, format!("expected end of statement, found {d}"), s)
                        .note("put each statement on its own line or separate them with `;`"));
                }
            }
            self.skip_newlines();
            if self.at(&Tok::RBrace) && !terminated_by_semi
                && let Stmt::Expr(e) = stmt {
                    tail = Some(Box::new(e));
                    break;
                }
            stmts.push(stmt);
        }
        let end = self.expect(&Tok::RBrace)?;
        Ok(Block { stmts, tail, span: start.to(end) })
    }

    fn stmt(&mut self) -> PResult<Stmt> {
        let start = self.span();
        match self.peek().clone() {
            Tok::Let => {
                self.next();
                // `let (a, b) = value`
                if self.at(&Tok::LParen) {
                    self.next();
                    let mut names = Vec::new();
                    while !self.at(&Tok::RParen) {
                        names.push(self.ident()?.0);
                        if !self.eat(&Tok::Comma) {
                            break;
                        }
                    }
                    self.expect(&Tok::RParen)?;
                    if !self.eat(&Tok::Assign) && !self.eat(&Tok::Bind) {
                        let s = self.span();
                        return Err(Diag::new(&C::BAD_LET, "expected `=` in let binding", s));
                    }
                    let value = self.expr()?;
                    return Ok(Stmt::LetTuple { names, value, span: start });
                }
                let mutable = self.eat(&Tok::Mut);
                let (name, _) = self.ident()?;
                let ty = if self.eat(&Tok::Colon) { Some(self.ty()?) } else { None };
                if !self.eat(&Tok::Assign) && !self.eat(&Tok::Bind) {
                    let s = self.span();
                    return Err(Diag::new(&C::BAD_LET, "expected `=` in let binding", s));
                }
                let value = self.expr()?;
                if self.at(&Tok::Else) {
                    self.next();
                    if mutable {
                        return Err(Diag::new(&C::BAD_LET, "`let … else` bindings cannot be `mut`", start));
                    }
                    let els = self.block()?;
                    return Ok(Stmt::LetElse { name, ty, value, els, span: start });
                }
                Ok(Stmt::Let { name, mutable, ty, value, span: start })
            }
            Tok::Mut => {
                // `mut x ← 5`
                self.next();
                let (name, _) = self.ident()?;
                if !self.eat(&Tok::Bind) && !self.eat(&Tok::Assign) {
                    let s = self.span();
                    return Err(Diag::new(&C::BAD_LET, "expected `←` after `mut name`", s));
                }
                let value = self.expr()?;
                Ok(Stmt::Let { name, mutable: true, ty: None, value, span: start })
            }
            Tok::Return => {
                self.next();
                let value = if matches!(self.peek(), Tok::Newline | Tok::Semi | Tok::RBrace) { None } else { Some(self.expr()?) };
                Ok(Stmt::Return(value, start))
            }
            Tok::Ident(name) if self.toks.get(self.pos + 1).is_some_and(|t| t.tok == Tok::Bind) => {
                self.next();
                self.next();
                let value = self.expr()?;
                Ok(Stmt::Let { name, mutable: false, ty: None, value, span: start })
            }
            _ => {
                let e = self.expr()?;
                if self.at(&Tok::Assign) || self.at(&Tok::Reassign) {
                    self.next();
                    let value = self.expr()?;
                    return match e.kind {
                        ExprKind::Path(p) if p.len() == 1 => {
                            Ok(Stmt::Assign { name: p.into_iter().next().unwrap(), index: None, value, span: start })
                        }
                        ExprKind::Index { base, index } => match base.kind {
                            ExprKind::Path(p) if p.len() == 1 => Ok(Stmt::Assign {
                                name: p.into_iter().next().unwrap(),
                                index: Some(*index),
                                value,
                                span: start,
                            }),
                            _ => Err(Diag::new(&C::BAD_ASSIGN_TARGET, "can only assign to `name[index]`", e.span)),
                        },
                        _ => Err(Diag::new(&C::BAD_ASSIGN_TARGET, "invalid assignment target", e.span)),
                    };
                }
                Ok(Stmt::Expr(e))
            }
        }
    }

    // ---- expressions --------------------------------------------------

    pub(crate) fn expr(&mut self) -> PResult<Expr> {
        let mut lhs = self.binary(0)?;
        loop {
            // A pipeline may continue on the next line: `xs\n  ▷ ⌽`
            let save = self.pos;
            while self.toks[self.pos].tok == Tok::Newline {
                self.pos += 1;
            }
            if self.toks[self.pos].tok != Tok::Pipe {
                self.pos = save;
                break;
            }
            self.next();
            let stage = self.pipe_stage(lhs)?;
            // keep reading left to right: `xs ▷ +/ ÷ 2`, `xs ▷ ⌽ ↑ 3`
            let stage = self.train_from(stage)?;
            lhs = self.binary_from(stage, 0)?;
        }
        Ok(lhs)
    }

    fn pipe_stage(&mut self, lhs: Expr) -> PResult<Expr> {
        let start = self.span();
        match self.peek().clone() {
            Tok::Prim(p) => {
                self.next();
                let mut args = vec![lhs];
                if p.arity() == 2 {
                    args.push(self.prim_rhs(p)?);
                }
                let span = start.to(self.prev_span());
                Ok(prim_expr(p, args, span))
            }
            Tok::Ident(_) => {
                let path = self.path_segments()?;
                let callee = Expr { kind: ExprKind::Path(path), span: start };
                let mut args = vec![lhs];
                if self.at(&Tok::LParen) && self.adjacent() {
                    args.extend(self.call_args()?);
                }
                Ok(Expr { kind: ExprKind::Call { callee: Box::new(callee), args }, span: start.to(self.prev_span()) })
            }
            Tok::LParen => {
                // `xs ▷ (x => x × 2)`
                let f = self.primary()?;
                if !matches!(f.kind, ExprKind::Lambda { .. }) {
                    return Err(Diag::new(&C::BAD_PIPELINE, "a pipeline stage must be a primitive, a function or a lambda", f.span));
                }
                Ok(Expr { span: start.to(f.span), kind: ExprKind::Call { callee: Box::new(f), args: vec![lhs] } })
            }
            other => Err(Diag::new(&C::BAD_PIPELINE, 
                format!("expected a primitive or function after `▷`, found {}", other.describe()),
                start,
            )),
        }
    }

    fn prev_span(&self) -> Span {
        self.toks[self.pos.saturating_sub(1)].span
    }

    fn binary(&mut self, min_prec: u8) -> PResult<Expr> {
        let lhs = self.unary()?;
        self.binary_from(lhs, min_prec)
    }

    fn binary_from(&mut self, mut lhs: Expr, min_prec: u8) -> PResult<Expr> {
        loop {
            let (op, prec) = match self.peek() {
                Tok::Or => (BinOp::Or, 1),
                Tok::And => (BinOp::And, 2),
                Tok::EqEq => (BinOp::Eq, 3),
                Tok::Ne => (BinOp::Ne, 3),
                Tok::Lt => (BinOp::Lt, 3),
                Tok::Le => (BinOp::Le, 3),
                Tok::Gt => (BinOp::Gt, 3),
                Tok::Ge => (BinOp::Ge, 3),
                Tok::Plus => (BinOp::Add, 4),
                Tok::Minus => (BinOp::Sub, 4),
                Tok::Star => (BinOp::Mul, 5),
                Tok::Slash => (BinOp::Div, 5),
                Tok::Percent => (BinOp::Mod, 5),
                Tok::IntDiv => (BinOp::IntDiv, 5),
                _ => break,
            };
            if prec < min_prec {
                break;
            }
            let op_span = self.next().span;
            let rhs = self.binary(prec + 1)?;
            if prec == 3
                && let ExprKind::Binary { op: BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge, .. } = lhs.kind {
                    return Err(Diag::new(&C::CHAINED_COMPARISON, "comparisons cannot be chained; use `and`", op_span)
                        .help("write `a < b and b < c`"));
                }
            let span = lhs.span.to(rhs.span);
            lhs = Expr { kind: ExprKind::Binary { op, lhs: Box::new(lhs), rhs: Box::new(rhs) }, span };
        }
        Ok(lhs)
    }

    fn unary(&mut self) -> PResult<Expr> {
        let start = self.span();
        let op = match self.peek() {
            Tok::Minus => UnOp::Neg,
            Tok::Not => UnOp::Not,
            _ => return self.train(),
        };
        self.next();
        let e = self.unary()?;
        // fold negative literals
        let span = start.to(e.span);
        Ok(match (op, e.kind) {
            (UnOp::Neg, ExprKind::Int(i)) => Expr { kind: ExprKind::Int(-i), span },
            (UnOp::Neg, ExprKind::Float(f)) => Expr { kind: ExprKind::Float(-f), span },
            (op, kind) => Expr { kind: ExprKind::Unary { op, expr: Box::new(Expr { kind, span: e.span }) }, span },
        })
    }

    /// Left-to-right glyph trains: postfix monadic prims and infix dyadic prims.
    fn train(&mut self) -> PResult<Expr> {
        let e = self.prefix()?;
        self.train_from(e)
    }

    fn train_from(&mut self, mut e: Expr) -> PResult<Expr> {
        while let Tok::Prim(p) = *self.peek() {
            self.next();
            let mut args = vec![e];
            if p.arity() == 2 {
                args.push(self.prim_rhs(p)?);
            }
            let span = args[0].span.to(self.prev_span());
            e = prim_expr(p, args, span);
        }
        Ok(e)
    }

    /// Right operand of a dyadic primitive.
    fn prim_rhs(&mut self, p: Prim) -> PResult<Expr> {
        if p == Prim::Each {
            // function operand: a path, or a lambda
            let s = self.span();
            return match self.peek().clone() {
                Tok::Ident(_) if self.toks.get(self.pos + 1).is_some_and(|t| t.tok == Tok::FatArrow) => self.primary(),
                Tok::Ident(_) => {
                    let path = self.path_segments()?;
                    Ok(Expr { kind: ExprKind::Path(path), span: s })
                }
                Tok::LParen => self.primary(),
                other => Err(Diag::new(&C::BAD_PIPELINE, format!("`¨` (each) expects a function, found {}", other.describe()), s)),
            };
        }
        self.prefix()
    }

    fn prefix(&mut self) -> PResult<Expr> {
        let start = self.span();
        if let Tok::Prim(p) = *self.peek() {
            self.next();
            if self.at(&Tok::LParen) && self.adjacent() {
                let args = self.call_args()?;
                if args.len() != p.arity() {
                    return Err(Diag::new(&C::ARG_COUNT, 
                        format!("`{}` takes {} argument(s), got {}", p.info().word, p.arity(), args.len()),
                        start,
                    ));
                }
                return Ok(prim_expr(p, args, start.to(self.prev_span())));
            }
            if p.arity() != 1 {
                return Err(Diag::new(&C::PRIM_USAGE, 
                    format!(
                        "`{}` is dyadic: write `xs {} y` or `{}(xs, y)`",
                        p.info().glyph,
                        p.info().glyph,
                        p.info().word
                    ),
                    start,
                ));
            }
            let operand = self.prefix()?;
            let span = start.to(operand.span);
            return Ok(prim_expr(p, vec![operand], span));
        }
        self.postfix()
    }

    fn postfix(&mut self) -> PResult<Expr> {
        let mut e = self.primary()?;
        loop {
            if self.at(&Tok::LParen) && self.adjacent() {
                let args = self.call_args()?;
                let span = e.span.to(self.prev_span());
                e = Expr { kind: ExprKind::Call { callee: Box::new(e), args }, span };
            } else if self.at(&Tok::Dot) {
                self.next();
                // `t.0` selects a tuple element
                let name = if let Tok::Int(i) = *self.peek() {
                    self.next();
                    i.to_string()
                } else {
                    self.name_or_word()?.0
                };
                let span = e.span.to(self.prev_span());
                e = Expr { kind: ExprKind::Field { base: Box::new(e), name }, span };
            } else if self.at(&Tok::LBracket) && self.adjacent() {
                self.next();
                self.nl_ignore += 1;
                let idx = self.expr();
                self.nl_ignore -= 1;
                let idx = idx?;
                self.expect(&Tok::RBracket)?;
                let span = e.span.to(self.prev_span());
                e = Expr { kind: ExprKind::Index { base: Box::new(e), index: Box::new(idx) }, span };
            } else {
                break;
            }
        }
        Ok(e)
    }

    fn call_args(&mut self) -> PResult<Vec<Expr>> {
        self.expect(&Tok::LParen)?;
        self.nl_ignore += 1;
        let r = (|| {
            let mut args = Vec::new();
            while !self.at(&Tok::RParen) {
                // `name: value`
                let named = matches!(self.peek(), Tok::Ident(_)) && self.toks.get(self.pos + 1).is_some_and(|t| t.tok == Tok::Colon);
                if named {
                    let (name, ns) = self.ident()?;
                    self.next();
                    let value = self.expr()?;
                    let span = ns.to(value.span);
                    args.push(Expr { kind: ExprKind::Named { name, value: Box::new(value) }, span });
                } else {
                    args.push(self.expr()?);
                }
                if !self.eat(&Tok::Comma) {
                    break;
                }
            }
            Ok(args)
        })();
        self.nl_ignore -= 1;
        let args = r?;
        self.expect(&Tok::RParen)?;
        Ok(args)
    }

    fn primary(&mut self) -> PResult<Expr> {
        let t = self.next();
        let span = t.span;
        let kind = match t.tok {
            Tok::Int(i) => ExprKind::Int(i),
            Tok::Float(f) => ExprKind::Float(f),
            Tok::Str(s) => ExprKind::Str(s),
            Tok::True => ExprKind::Bool(true),
            Tok::False => ExprKind::Bool(false),
            Tok::Ident(name) => {
                if self.at(&Tok::FatArrow) {
                    self.next();
                    let body = self.expr()?;
                    let span = span.to(body.span);
                    return Ok(Expr { kind: ExprKind::Lambda { param: name, body: Box::new(body) }, span });
                }
                let mut segs = vec![name];
                while self.at(&Tok::PathSep) {
                    self.next();
                    segs.push(self.name_or_word()?.0);
                }
                if is_upper(segs.last().unwrap()) && self.at_record_body() {
                    let fields = self.field_inits()?;
                    return Ok(Expr { kind: ExprKind::Record { path: segs, fields }, span: span.to(self.prev_span()) });
                }
                ExprKind::Path(segs)
            }
            Tok::LParen => {
                self.nl_ignore += 1;
                let r = (|| {
                    let first = self.expr()?;
                    if !self.at(&Tok::Comma) {
                        return Ok((first, None));
                    }
                    // a tuple
                    let mut xs = vec![first];
                    while self.eat(&Tok::Comma) {
                        if self.at(&Tok::RParen) {
                            break;
                        }
                        xs.push(self.expr()?);
                    }
                    Ok((xs[0].clone(), Some(xs)))
                })();
                self.nl_ignore -= 1;
                let (e, tuple) = r?;
                self.expect(&Tok::RParen)?;
                let span = span.to(self.prev_span());
                return Ok(match tuple {
                    Some(xs) => Expr { kind: ExprKind::Tuple(xs), span },
                    None => Expr { kind: e.kind, span },
                });
            }
            Tok::LBracket => {
                self.nl_ignore += 1;
                let r = (|| {
                    let mut elems = Vec::new();
                    while !self.at(&Tok::RBracket) {
                        elems.push(self.expr()?);
                        self.eat(&Tok::Comma);
                    }
                    Ok(elems)
                })();
                self.nl_ignore -= 1;
                let elems = r?;
                self.expect(&Tok::RBracket)?;
                ExprKind::Array(elems)
            }
            Tok::If => return self.if_rest(span),
            Tok::LBrace => {
                // `{ base with field: value, … }` — otherwise a block
                let save = (self.pos, self.nl_ignore);
                self.nl_ignore += 1;
                let base = self.expr();
                let is_with = base.is_ok() && self.at(&Tok::With);
                self.nl_ignore = save.1;
                if is_with {
                    self.next();
                    let fields = self.field_list()?;
                    return Ok(Expr { kind: ExprKind::With { base: Box::new(base?), fields }, span: span.to(self.prev_span()) });
                }
                self.pos = save.0 - 1;
                let b = self.block()?;
                ExprKind::Block(b)
            }
            Tok::Match => return self.match_rest(span),
            Tok::Interp(parts) => {
                let mut out = Vec::new();
                for p in parts {
                    match p {
                        StrPart::Text(t) => out.push(InterpPart::Text(t)),
                        StrPart::Code(c) => {
                            let mut e = parse_expr(&c).map_err(|d| Diag::new(&C::BAD_INTERPOLATION, format!("in `{{{c}}}`: {}", d.msg), span))?;
                            respan(&mut e, span);
                            out.push(InterpPart::Expr(e));
                        }
                    }
                }
                ExprKind::Interp(out)
            }
            Tok::Unsafe => {
                let b = self.block()?;
                ExprKind::Unsafe(b)
            }
            other => {
                return Err(Diag::new(&C::UNEXPECTED_TOKEN, format!("expected an expression, found {}", other.describe()), span));
            }
        };
        Ok(Expr { kind, span: span.to(self.prev_span()) })
    }

    /// After an uppercase name: `{ }` or `{ field: …`.
    fn at_record_body(&mut self) -> bool {
        if !self.at(&Tok::LBrace) {
            return false;
        }
        let mut i = self.pos + 1;
        while self.toks[i].tok == Tok::Newline {
            i += 1;
        }
        match &self.toks[i].tok {
            Tok::RBrace => true,
            Tok::Ident(_) | Tok::Prim(_) => self.toks.get(i + 1).is_some_and(|t| t.tok == Tok::Colon),
            _ => false,
        }
    }

    /// `{ name: expr, name: expr … }` (commas or newlines between fields).
    fn field_inits(&mut self) -> PResult<Vec<(String, Expr, Span)>> {
        self.expect(&Tok::LBrace)?;
        self.field_list()
    }

    /// The fields after an opening `{` (or after `with`), through the closing `}`.
    fn field_list(&mut self) -> PResult<Vec<(String, Expr, Span)>> {
        self.nl_ignore += 1;
        let r = (|| {
            let mut fields = Vec::new();
            while !self.at(&Tok::RBrace) {
                let (f, fs) = self.name_or_word()?;
                self.expect(&Tok::Colon)?;
                let v = self.expr()?;
                if fields.iter().any(|(n, _, _): &(String, Expr, Span)| *n == f) {
                    return Err(Diag::new(&C::DUPLICATE_ENTRY, format!("field `{f}` is given twice"), fs));
                }
                fields.push((f, v, fs));
                self.eat(&Tok::Comma);
            }
            Ok(fields)
        })();
        self.nl_ignore -= 1;
        let fields = r?;
        self.expect(&Tok::RBrace)?;
        Ok(fields)
    }

    fn match_rest(&mut self, start: Span) -> PResult<Expr> {
        let saved = std::mem::replace(&mut self.nl_ignore, 0);
        let scrut = self.expr();
        self.nl_ignore = saved;
        let scrut = scrut?;
        self.expect(&Tok::LBrace)?;
        let saved = std::mem::replace(&mut self.nl_ignore, 0);
        let r = (|| {
            let mut arms = Vec::new();
            loop {
                self.skip_newlines();
                while self.eat(&Tok::Comma) {
                    self.skip_newlines();
                }
                if self.at(&Tok::RBrace) {
                    break;
                }
                let pat = self.pattern()?;
                self.expect(&Tok::FatArrow)?;
                let body = self.expr()?;
                let span = pat.span().to(body.span);
                arms.push(Arm { pat, body, span });
                if !matches!(self.peek(), Tok::Newline | Tok::Semi | Tok::Comma | Tok::RBrace) {
                    let s = self.span();
                    return Err(Diag::new(&C::UNEXPECTED_TOKEN, "expected a new line, `,` or `}` after a match arm", s));
                }
            }
            Ok(arms)
        })();
        self.nl_ignore = saved;
        let arms = r?;
        let end = self.expect(&Tok::RBrace)?;
        if arms.is_empty() {
            return Err(Diag::new(&C::EMPTY_MATCH, "`match` needs at least one arm", start));
        }
        Ok(Expr { kind: ExprKind::Match { scrut: Box::new(scrut), arms }, span: start.to(end) })
    }

    fn pattern(&mut self) -> PResult<Pattern> {
        let t = self.peek_tok().clone();
        match t.tok {
            Tok::Ident(n) if n == "_" => {
                self.next();
                Ok(Pattern::Wild(t.span))
            }
            Tok::Int(_) | Tok::Float(_) | Tok::True | Tok::False | Tok::Str(_) | Tok::Minus => {
                let e = self.unary()?;
                Ok(Pattern::Lit(e))
            }
            Tok::Interp(parts) => {
                self.next();
                let mut out = Vec::new();
                for p in parts {
                    match p {
                        StrPart::Text(x) => out.push(RoutePart::Text(x)),
                        StrPart::Code(c) => {
                            let (name, ty) = match c.split_once(':') {
                                Some((n, t)) => (n.trim().to_string(), Some(t.trim().to_string())),
                                None => (c.trim().to_string(), None),
                            };
                            if !name.chars().next().is_some_and(|c| c.is_lowercase() || c == '_') || !name.chars().all(|c| c.is_alphanumeric() || c == '_') {
                                return Err(Diag::new(&C::BAD_ROUTE, format!("a route capture is `{{name}}` or `{{name: i64}}`, found `{{{c}}}`"), t.span));
                            }
                            if let Some(ty) = &ty
                                && !matches!(ty.as_str(), "str" | "i64" | "f64")
                            {
                                return Err(Diag::new(&C::BAD_ROUTE, format!("route captures are str, i64 or f64, found `{ty}`"), t.span));
                            }
                            if matches!(out.last(), Some(RoutePart::Capture { .. })) {
                                return Err(Diag::new(&C::BAD_ROUTE, "two route captures need text between them", t.span));
                            }
                            out.push(RoutePart::Capture { name, ty });
                        }
                    }
                }
                Ok(Pattern::Route(out, t.span))
            }
            Tok::LParen => {
                self.next();
                self.nl_ignore += 1;
                let r = (|| {
                    let mut ps = Vec::new();
                    while !self.at(&Tok::RParen) {
                        ps.push(self.pattern()?);
                        if !self.eat(&Tok::Comma) {
                            break;
                        }
                    }
                    Ok(ps)
                })();
                self.nl_ignore -= 1;
                let ps = r?;
                self.expect(&Tok::RParen)?;
                Ok(Pattern::Tuple(ps, t.span.to(self.prev_span())))
            }
            Tok::Ident(n) => {
                self.next();
                let mut path = vec![n];
                while self.eat(&Tok::PathSep) {
                    path.push(self.name_or_word()?.0);
                }
                if path.len() == 1 && !is_upper(&path[0]) {
                    return Ok(Pattern::Bind(path.pop().unwrap(), t.span));
                }
                let mut args = Vec::new();
                if self.at(&Tok::LParen) {
                    self.next();
                    self.nl_ignore += 1;
                    let r = (|| {
                        while !self.at(&Tok::RParen) {
                            args.push(self.pattern()?);
                            if !self.eat(&Tok::Comma) {
                                break;
                            }
                        }
                        Ok(())
                    })();
                    self.nl_ignore -= 1;
                    r?;
                    self.expect(&Tok::RParen)?;
                }
                Ok(Pattern::Variant { path, args, span: t.span.to(self.prev_span()) })
            }
            other => Err(Diag::new(&C::UNEXPECTED_TOKEN, format!("expected a pattern, found {}", other.describe()), t.span)),
        }
    }

    fn if_rest(&mut self, start: Span) -> PResult<Expr> {
        // the condition must not swallow the `{`, so parse it with newlines significant
        let saved = std::mem::replace(&mut self.nl_ignore, 0);
        let cond = self.expr();
        self.nl_ignore = saved;
        let cond = cond?;
        let then = self.block()?;
        // `else` may be on the next line after `}`
        let save = self.pos;
        while self.toks[self.pos].tok == Tok::Newline {
            self.pos += 1;
        }
        let els = if self.toks[self.pos].tok == Tok::Else {
            self.pos += 1;
            if self.at(&Tok::If) {
                let s = self.next().span;
                Some(Box::new(self.if_rest(s)?))
            } else {
                let b = self.block()?;
                let span = b.span;
                Some(Box::new(Expr { kind: ExprKind::Block(b), span }))
            }
        } else {
            self.pos = save;
            None
        };
        let span = start.to(self.prev_span());
        Ok(Expr { kind: ExprKind::If { cond: Box::new(cond), then, els }, span })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Structural S-expression rendering for comparing parses.
    fn sx(e: &Expr) -> String {
        match &e.kind {
            ExprKind::Int(i) => i.to_string(),
            ExprKind::Float(f) => f.to_string(),
            ExprKind::Str(s) => format!("{s:?}"),
            ExprKind::Bool(b) => b.to_string(),
            ExprKind::Array(xs) => format!("[{}]", xs.iter().map(sx).collect::<Vec<_>>().join(" ")),
            ExprKind::Path(p) => p.join("::"),
            ExprKind::Call { callee, args } => {
                format!("({} {})", sx(callee), args.iter().map(sx).collect::<Vec<_>>().join(" "))
            }
            ExprKind::Index { base, index } => format!("(idx {} {})", sx(base), sx(index)),
            ExprKind::Unary { op, expr } => format!("({op:?} {})", sx(expr)),
            ExprKind::Binary { op, lhs, rhs } => format!("({op:?} {} {})", sx(lhs), sx(rhs)),
            ExprKind::Prim { prim, args } => {
                format!("({} {})", prim.info().word, args.iter().map(sx).collect::<Vec<_>>().join(" "))
            }
            ExprKind::Lambda { param, body } => format!("(λ {param} {})", sx(body)),
            ExprKind::If { cond, .. } => format!("(if {})", sx(cond)),
            ExprKind::Block(_) => "{}".into(),
            ExprKind::Unsafe(_) => "unsafe{}".into(),
            ExprKind::Field { base, name } => format!("(. {} {name})", sx(base)),
            ExprKind::Record { path, .. } => format!("(rec {})", path.join("::")),
            ExprKind::With { base, .. } => format!("(with {})", sx(base)),
            ExprKind::Match { scrut, arms } => format!("(match {} {})", sx(scrut), arms.len()),
            ExprKind::Interp(parts) => format!("(interp {})", parts.len()),
            ExprKind::Named { name, value } => format!("({name}: {})", sx(value)),
            ExprKind::Tuple(xs) => format!("(tuple {})", xs.iter().map(sx).collect::<Vec<_>>().join(" ")),
        }
    }

    fn p(s: &str) -> String {
        sx(&parse_expr(s).unwrap_or_else(|d| panic!("{s}: {}", d.msg)))
    }

    #[test]
    fn glyph_word_equivalence() {
        let pairs = [
            ("⍳ 10 ▷ ⌽ ▷ +/", "iota 10 |> reverse |> sum"),
            ("⍳ 10 ⌽ +/", "iota(10) |> reverse |> sum"),
            ("xs ↑ 3 ¨ f", "each(take(xs, 3), f)"),
            ("xs ▷ ↑ 3", "take(xs, 3)"),
            ("xs ¨ x => x × 2", "xs |> each(x => x * 2)"),
            ("a ≤ b ∧ ¬ c", "a <= b and not c"),
        ];
        for (g, w) in pairs {
            assert_eq!(p(g), p(w), "{g}  vs  {w}");
        }
    }

    #[test]
    fn trains_are_left_to_right() {
        assert_eq!(p("⍳ 10 ⌽ ↑ 3 +/"), "(sum (take (reverse (iota 10)) 3))");
        assert_eq!(p("⌽ ⍳ 5"), "(reverse (iota 5))");
        assert_eq!(p("⍳ 5 + 1"), "(Add (iota 5) 1)");
    }

    #[test]
    fn pipelines_with_args_and_paths() {
        assert_eq!(p("xs ▷ f(2) ▷ a::g"), "(a::g (f xs 2))");
        assert_eq!(p("xs\n  ▷ ⌽"), "(reverse xs)");
        assert_eq!(p("xs ▷ +/ ÷ 2"), "(Div (sum xs) 2)");
        assert_eq!(p("a div 2 + 1"), "(Add (IntDiv a 2) 1)");
        assert_eq!(p("max(a, b)"), "(Max a b)");
        assert_eq!(p("a ⌈ b"), "(Max a b)");
        assert_eq!(p("∣ x"), "(Abs x)");
        assert_eq!(p("0 ‥ 5"), "(range 0 5)");
        assert_eq!(p("xs ▷ ⌽ ↑ 3 ▷ +/"), "(sum (take (reverse xs) 3))");
        assert_eq!(p("[1 2 ¯3, 4]"), "[1 2 -3 4]");
        assert_eq!(p("xs[0] + f(1, 2)"), "(Add (idx xs 0) (f 1 2))");
    }

    #[test]
    fn file_structure() {
        let f = parse_file(
            "module main\nuse ai::speech\nfn add(a: i64, b: i64) -> i64 { a + b }\npub module inner {\n  pub fn f() { 1 }\n}\npub fn main() {\n  let x = 1\n  x ← 2\n}\n",
        );
        // `x ← 2` re-binds x: the parser accepts it, the checker rejects it.
        assert_eq!(f.unwrap().module.items.len(), 4);
        let f = parse_file("§ main\nƒ main() {\n  mut x ← 1\n  x ↩ x + 1\n  print(x)\n}\n").unwrap();
        assert_eq!(f.module.name, "main");
        assert_eq!(f.module.items.len(), 1);
    }

    #[test]
    fn header_is_required() {
        assert!(parse_file("fn main() {}").is_err());
    }
}

#[cfg(test)]
mod step3_tests {
    use super::*;

    #[test]
    fn types_records_match_interp_named() {
        let f = parse_file(
            "module main
type User = { name: str, age: i64 }
type Shape =
    | Circle(f64)
    | Rect(f64, f64)
    | Dot
fn area(s: Shape) -> f64 {
    match s {
        Circle(r) => 3.0 × r × r
        Rect(w, h) => w × h
        Dot => 0.0
    }
}
fn label(text: str, size: i64 = 16) { text }
fn main() {
    let u = User { name: \"ada\", age: 36 }
    let v = { u with age: 37 }
    print(\"hi {u.name}, next year {v.age}\")
    print(label(\"x\", size: 24))
    if u.age > 30 { print(1) }
}
",
        )
        .unwrap_or_else(|d| panic!("{}", d.msg));
        let items = &f.module.items;
        assert!(matches!(&items[0], Item::Type(TypeDecl { kind: TypeKind::Record(fs), .. }) if fs.len() == 2));
        assert!(matches!(&items[1], Item::Type(TypeDecl { kind: TypeKind::Union(vs), .. }) if vs.len() == 3 && vs[1].fields.len() == 2));
        let Item::Fn(label) = &items[3] else { panic!() };
        assert!(label.params[1].default.is_some());
    }

    #[test]
    fn naming_rules() {
        assert!(parse_file("module main\ntype user = { a: i64 }\n").unwrap_err().msg.contains("`User`"));
        assert!(parse_file("module main\ntype T = a | B\n").unwrap_err().msg.contains("uppercase"));
    }
}
