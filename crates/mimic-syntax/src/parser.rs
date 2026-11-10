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
use crate::lexer::{Tok, Token, lex};
use crate::{Diag, Span};

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
            Err(Diag::new(format!("expected {}, found {}", t.describe(), found.tok.describe()), found.span))
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
            Tok::Prim(p) => Err(Diag::new(
                format!("`{}` is a built-in primitive ({}) and cannot be used as a name", p.info().word, p.info().glyph),
                t.span,
            )),
            other => Err(Diag::new(format!("expected identifier, found {}", other.describe()), t.span)),
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
            return Err(Diag::new("the first line of every .mim file must be `module <name>` (or `§ <name>`)", s));
        }
        self.next();
        let (name, _) = self.ident()?;
        if self.at(&Tok::LBrace) {
            let s = self.span();
            return Err(Diag::new("the file's header module takes no braces; write `module name` on the first line", s));
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
                    return Err(Diag::new("unclosed module: expected `}`", s));
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
                    return Err(Diag::new("`pub use` (re-exports) is not supported yet", start));
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
                    t => return Err(Diag::new("expected ABI string, e.g. extern \"C\"", t.span)),
                };
                if abi != "C" {
                    return Err(Diag::new(format!("unsupported ABI \"{abi}\"; only \"C\" is supported"), start));
                }
                self.expect(&Tok::LBrace)?;
                let mut fns = Vec::new();
                loop {
                    self.skip_newlines();
                    if self.eat(&Tok::RBrace) {
                        break;
                    }
                    let fstart = self.span();
                    let fpub = self.eat(&Tok::Pub) || is_pub;
                    self.expect(&Tok::Fn)?;
                    let (name, _) = self.name_or_word()?;
                    self.expect(&Tok::LParen)?;
                    self.nl_ignore += 1;
                    let mut params = Vec::new();
                    while !self.at(&Tok::RParen) {
                        let (pn, ps) = self.ident()?;
                        if !self.eat(&Tok::Colon) {
                            self.nl_ignore -= 1;
                            return Err(Diag::new("extern parameters need a C type, e.g. `x: f64`", ps));
                        }
                        params.push((pn, self.ty()?));
                        if !self.eat(&Tok::Comma) {
                            break;
                        }
                    }
                    self.nl_ignore -= 1;
                    self.expect(&Tok::RParen)?;
                    let ret = if self.eat(&Tok::Arrow) { Some(self.ty()?) } else { None };
                    fns.push(ExternFn { name, is_pub: fpub, params, ret, span: fstart });
                }
                Ok(Item::Extern(ExternBlock { abi, trusted, fns, span: start }))
            }
            other => Err(Diag::new(
                format!("expected an item (`fn`, `module`, `use`, `extern`), found {}", other.describe()),
                start,
            )),
        }
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
                return Err(Diag::new("parameters are immutable; bind a `let mut` copy inside the body", s));
            }
            let (pn, ps) = self.ident()?;
            let ty = if self.eat(&Tok::Colon) { Some(self.ty()?) } else { None };
            params.push(Param { name: pn, ty, span: ps });
            if !self.eat(&Tok::Comma) {
                break;
            }
        }
        self.nl_ignore -= 1;
        self.expect(&Tok::RParen)?;
        let ret = if self.eat(&Tok::Arrow) { Some(self.ty()?) } else { None };
        let body = self.block()?;
        Ok(FnDecl { name, is_pub, params, ret, body, span: start })
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
                    return Err(Diag::new(format!("expected end of statement, found {d}"), s)
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
                let mutable = self.eat(&Tok::Mut);
                let (name, _) = self.ident()?;
                let ty = if self.eat(&Tok::Colon) { Some(self.ty()?) } else { None };
                if !self.eat(&Tok::Assign) && !self.eat(&Tok::Bind) {
                    let s = self.span();
                    return Err(Diag::new("expected `=` in let binding", s));
                }
                let value = self.expr()?;
                Ok(Stmt::Let { name, mutable, ty, value, span: start })
            }
            Tok::Mut => {
                // `mut x ← 5`
                self.next();
                let (name, _) = self.ident()?;
                if !self.eat(&Tok::Bind) && !self.eat(&Tok::Assign) {
                    let s = self.span();
                    return Err(Diag::new("expected `←` after `mut name`", s));
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
                            _ => Err(Diag::new("can only assign to `name[index]`", e.span)),
                        },
                        _ => Err(Diag::new("invalid assignment target", e.span)),
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
                    return Err(Diag::new("a pipeline stage must be a primitive, a function or a lambda", f.span));
                }
                Ok(Expr { span: start.to(f.span), kind: ExprKind::Call { callee: Box::new(f), args: vec![lhs] } })
            }
            other => Err(Diag::new(
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
                    return Err(Diag::new("comparisons cannot be chained; use `and`", op_span));
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
                other => Err(Diag::new(format!("`¨` (each) expects a function, found {}", other.describe()), s)),
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
                    return Err(Diag::new(
                        format!("`{}` takes {} argument(s), got {}", p.info().word, p.arity(), args.len()),
                        start,
                    ));
                }
                return Ok(prim_expr(p, args, start.to(self.prev_span())));
            }
            if p.arity() != 1 {
                return Err(Diag::new(
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
                args.push(self.expr()?);
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
                ExprKind::Path(segs)
            }
            Tok::LParen => {
                self.nl_ignore += 1;
                let e = self.expr();
                self.nl_ignore -= 1;
                let e = e?;
                self.expect(&Tok::RParen)?;
                return Ok(Expr { kind: e.kind, span: span.to(self.prev_span()) });
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
                self.pos -= 1;
                let b = self.block()?;
                ExprKind::Block(b)
            }
            Tok::Unsafe => {
                let b = self.block()?;
                ExprKind::Unsafe(b)
            }
            other => {
                return Err(Diag::new(format!("expected an expression, found {}", other.describe()), span));
            }
        };
        Ok(Expr { kind, span: span.to(self.prev_span()) })
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
