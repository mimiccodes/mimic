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
                }                Stmt::While { cond, body, span: sp } => {
                    *sp = span;
                    respan(cond, span);
                    blk(body, span);
                }
                Stmt::Break(sp) => *sp = span,
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
    // `evolve(n, step)` and `settle(step, done)` with functions by name: the
    // same as `s => step(s)` (and `s => done(s)`), which the compiler steps
    // in place (the state handed on, a list in it updated without a copy)
    // where a function value is called with a state it cannot take over
    let named: &[usize] = match prim {
        Prim::Evolve => &[2],
        Prim::Settle => &[1, 2],
        _ => &[],
    };
    for &i in named {
        if let Some(f) = args.get_mut(i)
            && matches!(f.kind, ExprKind::Path(_))
        {
            let at = f.span;
            let callee = std::mem::replace(f, Expr { kind: ExprKind::Path(vec![]), span: at });
            let state = Expr { kind: ExprKind::Path(vec!["step·state".to_string()]), span: at };
            let body = Expr { kind: ExprKind::Call { callee: Box::new(callee), args: vec![state] }, span: at };
            *f = Expr { kind: ExprKind::Lambda { params: vec!["step·state".to_string()], body: Box::new(body) }, span: at };
        }
    }
    let kind = match (prim, args.len()) {
        (Prim::Max | Prim::Min, 2) => {
            let rhs = args.pop().unwrap();
            let lhs = args.pop().unwrap();
            let op = if prim == Prim::Max { BinOp::Max } else { BinOp::Min };
            ExprKind::Binary { op, lhs: Box::new(lhs), rhs: Box::new(rhs) }
        }
        (Prim::Abs, 1) => ExprKind::Unary { op: UnOp::Abs, expr: Box::new(args.pop().unwrap()) },
        // gather(+) and gather(×) are the fused, typed sum and product
        // gather(+), gather(×), gather(max), gather(min): the fused, typed reductions
        (Prim::Gather, 2) if matches!(args[1].kind, ExprKind::OpRef(BinOp::Add | BinOp::Mul | BinOp::Max | BinOp::Min)) => {
            let ExprKind::OpRef(op) = args.pop().unwrap().kind else { unreachable!() };
            let prim = match op {
                BinOp::Add => Prim::Sum,
                BinOp::Mul => Prim::Product,
                BinOp::Max => Prim::Maximum,
                _ => Prim::Minimum,
            };
            ExprKind::Prim { prim, args }
        }
        // gather(^), gather(&), gather(|) (and trail): the lambda
        // `(a, b) => a ^ b`, which the checker types over integers and code
        // generation folds in a loop, nothing boxed (the runtime's generic
        // fold called a function value with two boxed numbers per element)
        (Prim::Gather | Prim::Trail, 2) if matches!(args[1].kind, ExprKind::OpRef(BinOp::BitXor | BinOp::BitAnd | BinOp::BitOr)) => {
            let ExprKind::OpRef(op) = args[1].kind else { unreachable!() };
            let at = args[1].span;
            let var = |n: &str| Expr { kind: ExprKind::Path(vec![n.to_string()]), span: at };
            let body = Expr { kind: ExprKind::Binary { op, lhs: Box::new(var("__fa")), rhs: Box::new(var("__fb")) }, span: at };
            args[1] = Expr { kind: ExprKind::Lambda { params: vec!["__fa".into(), "__fb".into()], body: Box::new(body) }, span: at };
            ExprKind::Prim { prim, args }
        }
        _ => ExprKind::Prim { prim, args },
    };
    Expr { kind, span }
}

pub fn parse_file(src: &str) -> Result<File, Diag> {
    let toks = lex(src)?;
    let mut p = Parser { toks, pos: 0, nl_ignore: 0, no_lambda: false };
    p.file()
}

/// Parse a single expression (used by tests and tooling).
/// `value else { els }` as the nodes every pass already knows: the value
/// bound once (named after where it is written), then the value inside it
/// when it holds one (`Some`, `Ok`; the prelude's `__has_value` and
/// `__value_of`, typed by the checker), else the block, which may give a
/// value or leave with `return`.
fn or_else(value: Expr, els: Block) -> Expr {
    let span = Span { end: els.span.end, ..value.span };
    let tmp = format!("__or{}", value.span.start);
    let var = || Expr { kind: ExprKind::Path(vec![tmp.clone()]), span };
    let call = |f: &str| Expr { kind: ExprKind::Call { callee: Box::new(Expr { kind: ExprKind::Path(vec![f.to_string()]), span }), args: vec![var()] }, span };
    let then = Block { stmts: vec![], tail: Some(Box::new(call("__value_of"))), span };
    let els_span = els.span;
    let test = Expr { kind: ExprKind::If { cond: Box::new(call("__has_value")), then, els: Some(Box::new(Expr { kind: ExprKind::Block(els), span: els_span })) }, span };
    let bind = Stmt::Let { name: tmp.clone(), mutable: false, ty: None, value, span };
    Expr { kind: ExprKind::Block(Block { stmts: vec![bind], tail: Some(Box::new(test)), span }), span }
}

/// `for x in a ‥ b { … }` / `for x in xs { … }` as a `while` over a hidden
/// counter (the bounds, or the list and its length, computed once, before
/// the loop): `{ let __hi = b; let mut __i = a; while __i < __hi { let x =
/// __i; __i = __i + 1; … } }`. The counter moves on before the body, so a
/// `break` in it leaves nothing to undo.
fn for_loop(var: String, iter: Expr, body: Block, span: Span) -> Stmt {
    let at = span.start;
    let (i, hi, xs) = (format!("__i{at}"), format!("__hi{at}"), format!("__xs{at}"));
    let path = |n: &str| Expr { kind: ExprKind::Path(vec![n.to_string()]), span };
    let int = |v: i64| Expr { kind: ExprKind::Int(v), span };
    let bin = |op: BinOp, a: Expr, b: Expr| Expr { kind: ExprKind::Binary { op, lhs: Box::new(a), rhs: Box::new(b) }, span };
    let let_ = |n: &str, mutable: bool, value: Expr| Stmt::Let { name: n.to_string(), mutable, ty: None, value, span };
    let (mut setup, start, element) = match iter.kind {
        ExprKind::Prim { prim: Prim::Range, args } if args.len() == 2 => {
            let mut args = args.into_iter();
            let (a, b) = (args.next().unwrap(), args.next().unwrap());
            (vec![let_(&hi, false, b)], a, path(&i))
        }
        _ => {
            let len = Expr { kind: ExprKind::Prim { prim: Prim::Len, args: vec![path(&xs)] }, span };
            let elem = Expr { kind: ExprKind::Index { base: Box::new(path(&xs)), index: Box::new(path(&i)) }, span };
            (vec![let_(&xs, false, iter), let_(&hi, false, len)], int(0), elem)
        }
    };
    setup.push(let_(&i, true, start));
    let mut stmts = vec![let_(&var, false, element), Stmt::Assign { name: i.clone(), index: None, value: bin(BinOp::Add, path(&i), int(1)), span }];
    stmts.extend(body.stmts);
    if let Some(t) = body.tail {
        stmts.push(Stmt::Expr(*t));
    }
    let lp = Stmt::While { cond: bin(BinOp::Lt, path(&i), path(&hi)), body: Block { stmts, tail: None, span: body.span }, span };
    setup.push(lp);
    Stmt::Expr(Expr { kind: ExprKind::Block(Block { stmts: setup, tail: None, span }), span })
}

pub fn parse_expr(src: &str) -> Result<Expr, Diag> {
    let toks = lex(src)?;
    let mut p = Parser { toks, pos: 0, nl_ignore: 0, no_lambda: false };
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
    /// In a match guard: `x => …` ends the guard instead of starting a lambda.
    pub(crate) no_lambda: bool,
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
    /// A name being declared (a parameter, a `let`): a primitive's word is
    /// refused here too, though before a `:` the lexer reads any word as a
    /// name (`shape: [i64]`): every later use of it would be the primitive.
    fn decl_ident(&mut self) -> PResult<(String, Span)> {
        let (name, span) = self.ident()?;
        if let Some(p) = Prim::from_word(&name) {
            return Err(Diag::new(&C::PRIMITIVE_AS_NAME, format!("`{name}` is a built-in primitive ({}) and cannot be used as a name", p.info().glyph), span));
        }
        Ok((name, span))
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
            // `test "name" { … }`: a test (run by `mimic test`, never part of
            // a build). `test` is a word only here: a variable may be called
            // `test`. It becomes a function named `test name`, which no code
            // can call (a name with a space).
            Tok::Ident(w) if w == "test" && matches!(self.toks.get(self.pos + 1).map(|t| &t.tok), Some(Tok::Str(_))) => {
                if is_pub {
                    return Err(Diag::new(&C::UNEXPECTED_TOKEN, "a test is not `pub`: tests run only under `mimic test`", start));
                }
                self.next();
                let Token { tok: Tok::Str(name), span, .. } = self.next() else { unreachable!() };
                if name.trim().is_empty() {
                    return Err(Diag::new(&C::UNEXPECTED_TOKEN, "a test needs a name: test \"what it checks\" { … }", span));
                }
                // `test "…" for x: i64, xs: [i64] { … }`: a property, run with
                // generated inputs (see desugar::properties)
                let mut params = Vec::new();
                if matches!(self.peek(), Tok::Ident(w) if w == "for") {
                    self.next();
                    loop {
                        let (pn, ps) = self.ident()?;
                        self.expect(&Tok::Colon)?;
                        let ty = self.ty()?;
                        params.push(Param { name: pn, ty: Some(ty), default: None, span: ps });
                        if !self.eat(&Tok::Comma) {
                            break;
                        }
                    }
                }
                let body = self.block()?;
                Ok(Item::Fn(FnDecl { name: format!("test {name}"), type_params: vec![], is_pub: false, params, ret: None, body, export_c: false, requires: vec![], ensures: vec![], span: start }))
            }
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
                let intrinsic = abi == "mimic";
                if abi != "C" && !intrinsic {
                    return Err(Diag::new(&C::UNSUPPORTED_SYNTAX, format!("unsupported ABI \"{abi}\"; only \"C\" is supported"), start));
                }
                // `extern "C" concurrent { … }`: what the functions do besides
                // returning a value. C functions without it are assumed to
                // change shared state (`sync`); runtime intrinsics to be `pure`.
                let effect = match self.peek().clone() {
                    Tok::Ident(w) if Effect::from_word(&w).is_some() => {
                        self.next();
                        Effect::from_word(&w).unwrap()
                    }
                    _ if intrinsic => Effect::Pure,
                    _ => Effect::Sync,
                };
                let (effects, overlap, sync) = effect.flags();
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
                // `as sql`: the namespace its names live in (default `c::`)
                let alias_of = |p: &mut Self| -> Result<Option<String>, Diag> { if p.eat(&Tok::As) { Ok(Some(p.ident()?.0)) } else { Ok(None) } };
                // `extern "C" include "math.h"`: a whole header, no braces
                if let Some(inc) = self.include_directive()? {
                    includes.push(inc);
                    let alias = alias_of(self)?;
                    return Ok(Item::Extern(ExternBlock { abi, trusted, effect, fns, includes, is_pub, alias, span: start }));
                }
                let alias = alias_of(self)?;
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
                    let type_params = self.type_params()?;
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
                    fns.push(ExternFn { name, is_pub: fpub, params, variadic, link_name: None, intrinsic, type_params, effects, overlap, sync, ret, span: fstart });
                }
                let trusted = trusted || intrinsic;
                Ok(Item::Extern(ExternBlock { abi, trusted, effect, fns, includes, is_pub, alias, span: start }))
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
        let params = self.type_params()?;
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
            return Ok(TypeDecl { name, params, is_pub, kind: TypeKind::Record(fields), span: start });
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
        Ok(TypeDecl { name, params, is_pub, kind: TypeKind::Union(variants), span: start })
    }

    fn fn_decl(&mut self, is_pub: bool, start: Span) -> PResult<FnDecl> {
        self.expect(&Tok::Fn)?;
        let (name, name_span) = self.ident()?;
        // (after `fn` any word is a name, so this one has to be refused here:
        // everywhere else the word is the primitive, so the function could
        // never be called. C functions keep such names under `c::`.)
        if let Some(p) = Prim::from_word(&name) {
            return Err(Diag::new(
                &C::PRIMITIVE_AS_NAME,
                format!("`{name}` is a built-in primitive ({}): a function with this name could never be called, since `{name}(…)` is always the primitive; rename the function", p.info().glyph),
                name_span,
            ));
        }
        let type_params = self.type_params()?;
        self.expect(&Tok::LParen)?;
        self.nl_ignore += 1;
        let mut params = Vec::new();
        while !self.at(&Tok::RParen) {
            if self.at(&Tok::Mut) {
                let s = self.span();
                self.nl_ignore -= 1;
                return Err(Diag::new(&C::PARAM_RULES, "parameters are immutable; bind a `let mut` copy inside the body", s));
            }
            let (pn, ps) = self.decl_ident()?;
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
        // contracts: `requires cond` and `ensures cond` lines before the body
        let (mut requires, mut ensures) = (Vec::new(), Vec::new());
        loop {
            let save = self.pos;
            self.skip_newlines();
            match &self.toks[self.pos].tok {
                Tok::Ident(w) if w == "requires" || w == "ensures" => {
                    let which = w.clone();
                    self.pos += 1;
                    let e = self.expr()?;
                    if which == "requires" { requires.push(e) } else { ensures.push(e) }
                }
                _ => {
                    if requires.is_empty() && ensures.is_empty() {
                        self.pos = save;
                    }
                    break;
                }
            }
        }
        let body = self.block()?;
        Ok(FnDecl { name, type_params, is_pub, params, ret, body, export_c: false, requires, ensures, span: start })
    }

    /// `[T, U]` after a type or function name: type parameters (uppercase names).
    fn type_params(&mut self) -> PResult<Vec<String>> {
        let mut out: Vec<String> = Vec::new();
        if !self.eat(&Tok::LBracket) {
            return Ok(out);
        }
        loop {
            let (n, s) = self.ident()?;
            if !is_upper(&n) {
                let fixed = upper_first(&n);
                return Err(Diag::new(&C::NAME_CASE, format!("type parameters start with an uppercase letter: `{fixed}`"), s));
            }
            if out.contains(&n) {
                return Err(Diag::new(&C::DUPLICATE_ENTRY, format!("type parameter `{n}` is declared twice"), s));
            }
            out.push(n);
            if !self.eat(&Tok::Comma) {
                break;
            }
        }
        self.expect(&Tok::RBracket)?;
        Ok(out)
    }

    fn ty(&mut self) -> PResult<Type> {
        let s = self.span();
        if self.eat(&Tok::LBracket) {
            let inner = self.ty()?;
            if self.eat(&Tok::Colon) {
                let v = self.ty()?;
                self.expect(&Tok::RBracket)?;
                return Ok(Type::Map(Box::new(inner), Box::new(v), s));
            }
            self.expect(&Tok::RBracket)?;
            return Ok(Type::Array(Box::new(inner), s));
        }
        // `(i64, str)`: a tuple; `(T)` is just T
        if self.eat(&Tok::LParen) {
            let mut ts = vec![self.ty()?];
            while self.eat(&Tok::Comma) {
                ts.push(self.ty()?);
            }
            self.expect(&Tok::RParen)?;
            if ts.len() == 1 {
                return Ok(ts.pop().unwrap());
            }
            return Ok(Type::Tuple(ts, s.to(self.prev_span())));
        }
        // `fn(i64, f64) -> bool`
        if self.eat(&Tok::Fn) {
            self.expect(&Tok::LParen)?;
            let mut ps = Vec::new();
            while !self.at(&Tok::RParen) {
                ps.push(self.ty()?);
                if !self.eat(&Tok::Comma) {
                    break;
                }
            }
            self.expect(&Tok::RParen)?;
            let ret = if self.eat(&Tok::Arrow) { Some(Box::new(self.ty()?)) } else { None };
            return Ok(Type::Fn(ps, ret, s));
        }
        let (mut n, _) = self.ident()?;
        // `http::Response`: a type from another module
        while self.eat(&Tok::PathSep) {
            n.push_str("::");
            n.push_str(&self.ident()?.0);
        }
        if n == "ptr" {
            self.expect(&Tok::LBracket)?;
            let inner = self.ty()?;
            self.expect(&Tok::RBracket)?;
            return Ok(Type::Ptr(Box::new(inner), s));
        }
        // `Option[i64]`: type arguments
        if self.eat(&Tok::LBracket) {
            let mut args = vec![self.ty()?];
            while self.eat(&Tok::Comma) {
                args.push(self.ty()?);
            }
            self.expect(&Tok::RBracket)?;
            return Ok(Type::App(n, args, s.to(self.prev_span())));
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

    /// What a `for` goes over: `a ‥ b` with each bound a whole expression
    /// (`for j in 1 ‥ n + 1` counts to `n + 1`, as it reads, where an
    /// expression `1 ‥ n + 1` is `(1 ‥ n) + 1`, the range binding tighter),
    /// or any expression giving a list.
    fn for_iterable(&mut self) -> PResult<Expr> {
        // a `‥` outside brackets before the body's `{`
        let mut depth = 0i32;
        let mut at = None;
        for (k, t) in self.toks.iter().enumerate().skip(self.pos) {
            match &t.tok {
                Tok::LParen | Tok::LBracket => depth += 1,
                Tok::RParen | Tok::RBracket => depth -= 1,
                Tok::LBrace if depth == 0 => break,
                Tok::Newline | Tok::Eof => break,
                Tok::Prim(Prim::Range) if depth == 0 => {
                    at = Some(k);
                    break;
                }
                _ => {}
            }
        }
        let Some(k) = at else { return self.expr() };
        let start = self.span();
        // the left bound: up to the `‥` (read as the end of a statement)
        let range_tok = std::mem::replace(&mut self.toks[k].tok, Tok::Semi);
        let lo = self.expr();
        self.toks[k].tok = range_tok;
        let lo = lo?;
        if self.pos != k {
            let s = self.span();
            return Err(Diag::new(&C::UNEXPECTED_TOKEN, "expected `‥` after the start of the range", s));
        }
        self.next();
        let hi = self.expr()?;
        let span = start.to(self.prev_span());
        Ok(Expr { kind: ExprKind::Prim { prim: Prim::Range, args: vec![lo, hi] }, span })
    }

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
                let (name, _) = self.decl_ident()?;
                let ty = if self.eat(&Tok::Colon) { Some(self.ty()?) } else { None };
                if !self.eat(&Tok::Assign) && !self.eat(&Tok::Bind) {
                    let s = self.span();
                    return Err(Diag::new(&C::BAD_LET, "expected `=` in let binding", s));
                }
                let value = self.expr_no_else()?;
                if self.at(&Tok::Else) {
                    self.next();
                    let els = self.block()?;
                    // a block that gives a value: the binding is the value
                    // inside, or that (`let w = parse_int(s) else { 80 }`);
                    // one that leaves: `let … else` as before
                    if els.tail.is_some() {
                        let value = or_else(value, els);
                        return Ok(Stmt::Let { name, mutable, ty, value, span: start });
                    }
                    if mutable {
                        return Err(Diag::new(&C::BAD_LET, "`let … else` bindings cannot be `mut`", start));
                    }
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
            // `while cond { … }` (`⟳`), `for x in … { … }` (`∀`), `break`
            // (`↯`): words a statement starts with, not reserved elsewhere
            Tok::Ident(w) if w == "while" && !self.toks.get(self.pos + 1).is_some_and(|t| matches!(t.tok, Tok::Bind | Tok::Assign | Tok::Reassign | Tok::LParen | Tok::Dot)) => {
                self.next();
                // (the condition must not swallow the `{`)
                let saved = std::mem::replace(&mut self.nl_ignore, 0);
                let cond = self.expr();
                self.nl_ignore = saved;
                let cond = cond?;
                let body = self.block()?;
                Ok(Stmt::While { cond, body, span: start.to(self.prev_span()) })
            }
            Tok::Ident(w) if w == "for" && matches!(self.toks.get(self.pos + 1).map(|t| &t.tok), Some(Tok::Ident(_))) && matches!(self.toks.get(self.pos + 2).map(|t| &t.tok), Some(Tok::Ident(k)) if k == "in") => {
                self.next();
                let (var, _) = self.decl_ident()?;
                self.next(); // in
                let saved = std::mem::replace(&mut self.nl_ignore, 0);
                let iter = self.for_iterable();
                self.nl_ignore = saved;
                let iter = iter?;
                let body = self.block()?;
                Ok(for_loop(var, iter, body, start.to(self.prev_span())))
            }
            Tok::Ident(w) if w == "break" && matches!(self.toks.get(self.pos + 1).map(|t| &t.tok), Some(Tok::Newline | Tok::Semi | Tok::RBrace | Tok::Eof)) => {
                self.next();
                Ok(Stmt::Break(start))
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

    /// An expression, and `e else { … }` after it: the value inside an
    /// `Option` or `Result`, else what the block gives.
    pub(crate) fn expr(&mut self) -> PResult<Expr> {
        let value = self.expr_no_else()?;
        if !self.at(&Tok::Else) {
            return Ok(value);
        }
        self.next();
        let els = self.block()?;
        Ok(or_else(value, els))
    }

    /// An expression, an `else` after it left to the caller.
    fn expr_no_else(&mut self) -> PResult<Expr> {
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
                // loop words: `x ▷ evolve(60, step)`, `xs ▷ gather(+)`
                if matches!(p, Prim::Gather | Prim::Trail | Prim::Evolve | Prim::Settle) && self.at(&Tok::LParen) {
                    args.extend(self.call_args()?);
                    if args.len() != p.arity() {
                        return Err(Diag::new(
                            &C::ARG_COUNT,
                            format!("`{}` takes {} argument(s) after the piped value, got {}", p.info().word, p.arity() - 1, args.len() - 1),
                            start,
                        ));
                    }
                    let span = start.to(self.prev_span());
                    return Ok(prim_expr(p, args, span));
                }
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

    /// After a `(`: is this `ident, … ) =>` or `) =>` (a lambda's parameters)?
    fn lambda_params_ahead(&self) -> bool {
        let mut i = self.pos;
        let tok = |i: usize| self.toks.get(i).map(|t| &t.tok);
        let skip_nl = |mut i: usize| {
            while matches!(tok(i), Some(Tok::Newline)) {
                i += 1;
            }
            i
        };
        i = skip_nl(i);
        if matches!(tok(i), Some(Tok::RParen)) {
            return matches!(tok(skip_nl(i + 1)), Some(Tok::FatArrow));
        }
        loop {
            if !matches!(tok(i), Some(Tok::Ident(_))) {
                return false;
            }
            i = skip_nl(i + 1);
            // `name: Type`: skip the type, to the `,` or `)` at this depth
            if matches!(tok(i), Some(Tok::Colon)) {
                let mut depth = 0usize;
                i += 1;
                loop {
                    match tok(i) {
                        None | Some(Tok::Newline) if depth == 0 => return false,
                        Some(Tok::LParen | Tok::LBracket) => depth += 1,
                        Some(Tok::RParen | Tok::RBracket) if depth > 0 => depth -= 1,
                        Some(Tok::Comma | Tok::RParen) if depth == 0 => break,
                        None => return false,
                        _ => {}
                    }
                    i += 1;
                }
            }
            match tok(i) {
                Some(Tok::Comma) => i = skip_nl(i + 1),
                Some(Tok::RParen) => return matches!(tok(skip_nl(i + 1)), Some(Tok::FatArrow)),
                _ => return false,
            }
        }
    }

    /// In an argument list: an operator followed by `,` or `)`.
    fn bare_op_arg(&self) -> Option<BinOp> {
        let op = self.op_at(self.pos)?;
        matches!(self.toks.get(self.pos + 1).map(|t| &t.tok), Some(Tok::RParen | Tok::Comma)).then_some(op)
    }

    fn op_at(&self, i: usize) -> Option<BinOp> {
        Some(match self.toks.get(i).map(|t| &t.tok)? {
            Tok::Plus => BinOp::Add,
            Tok::Minus => BinOp::Sub,
            Tok::Star => BinOp::Mul,
            Tok::Slash => BinOp::Div,
            Tok::Percent => BinOp::Mod,
            Tok::IntDiv => BinOp::IntDiv,
            Tok::EqEq => BinOp::Eq,
            Tok::Ne => BinOp::Ne,
            Tok::Lt => BinOp::Lt,
            Tok::Le => BinOp::Le,
            Tok::Gt => BinOp::Gt,
            Tok::Ge => BinOp::Ge,
            Tok::And => BinOp::And,
            Tok::Or => BinOp::Or,
            Tok::Amp => BinOp::BitAnd,
            Tok::Bar => BinOp::BitOr,
            Tok::Caret => BinOp::BitXor,
            Tok::Shl => BinOp::Shl,
            Tok::Shr => BinOp::Shr,
            Tok::UShr => BinOp::UShr,
            // `max` / `min` (⌈ ⌊) as values: `gather(max)`, `(max)`
            Tok::Prim(Prim::Max) => BinOp::Max,
            Tok::Prim(Prim::Min) => BinOp::Min,
            _ => return None,
        })
    }

    /// After a `(`: is this `op )`, an operator as a value?
    fn op_ref_ahead(&self) -> Option<BinOp> {
        let op = self.op_at(self.pos)?;
        matches!(self.toks.get(self.pos + 1).map(|t| &t.tok), Some(Tok::RParen)).then_some(op)
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
                // bitwise operators bind tighter than comparisons:
                // `x & mask == 0` is `(x & mask) == 0`
                Tok::Bar => (BinOp::BitOr, 4),
                Tok::Caret => (BinOp::BitXor, 5),
                Tok::Amp => (BinOp::BitAnd, 6),
                Tok::Shl => (BinOp::Shl, 7),
                Tok::Shr => (BinOp::Shr, 7),
                Tok::UShr => (BinOp::UShr, 7),
                Tok::Plus => (BinOp::Add, 8),
                Tok::Minus => (BinOp::Sub, 8),
                Tok::Star => (BinOp::Mul, 9),
                Tok::Slash => (BinOp::Div, 9),
                Tok::Percent => (BinOp::Mod, 9),
                Tok::IntDiv => (BinOp::IntDiv, 9),
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
            Tok::Tilde => UnOp::BitNot,
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
        // `∧/ xs`, `∨/ xs`: the and and or reductions written as glyphs (the
        // lexer reads them as `all` and `any`), applied like `+/ xs`
        if let Tok::Ident(n) = self.peek().clone()
            && matches!(n.as_str(), "all" | "any")
            && start.end - start.start != n.len()
        {
            self.next();
            if !(self.at(&Tok::LParen) && self.adjacent()) {
                let operand = self.prefix()?;
                let span = start.to(operand.span);
                let callee = Expr { kind: ExprKind::Path(vec![n]), span: start };
                return Ok(Expr { kind: ExprKind::Call { callee: Box::new(callee), args: vec![operand] }, span });
            }
            self.pos -= 1;
        }
        self.postfix()
    }

    /// `f[User, i64](…)`: explicit type arguments. Only types that read as
    /// types (uppercase names, built-in type names, `[…]`) followed directly by
    /// `(` count, so `xs[i]` and `handlers[k](x)` stay indexing.
    fn type_args_ahead(&mut self) -> Option<Vec<Type>> {
        const BUILTIN: &[&str] = &["i8", "i16", "i32", "i64", "u8", "u16", "u32", "u64", "isize", "usize", "f32", "f64", "bool", "char", "str", "any"];
        let save = (self.pos, self.nl_ignore);
        let typey = |t: &Type| match t {
            Type::Named(n, _) | Type::App(n, _, _) => {
                n.split("::").last().is_some_and(|l| l.starts_with(|c: char| c.is_ascii_uppercase())) || BUILTIN.contains(&n.as_str())
            }
            Type::Array(..) | Type::Map(..) | Type::Tuple(..) => true,
            _ => false,
        };
        let attempt = (|| {
            self.next(); // [
            let mut ts = vec![self.ty().ok()?];
            while self.eat(&Tok::Comma) {
                ts.push(self.ty().ok()?);
            }
            if !self.eat(&Tok::RBracket) || !self.at(&Tok::LParen) || !self.adjacent() {
                return None;
            }
            ts.iter().all(typey).then_some(ts)
        })();
        if attempt.is_none() {
            (self.pos, self.nl_ignore) = save;
        }
        attempt
    }

    fn postfix(&mut self) -> PResult<Expr> {
        let mut e = self.primary()?;
        loop {
            if self.at(&Tok::LBracket)
                && self.adjacent()
                && matches!(e.kind, ExprKind::Path(_))
                && let Some(ts) = self.type_args_ahead()
            {
                let mut args: Vec<Expr> = ts.into_iter().map(|t| {
                    let span = t.span();
                    Expr { kind: ExprKind::TypeArg(t), span }
                }).collect();
                args.extend(self.call_args()?);
                let span = e.span.to(self.prev_span());
                e = Expr { kind: ExprKind::Call { callee: Box::new(e), args }, span };
            } else if self.at(&Tok::LParen) && self.adjacent() {
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
                let open = self.next().span;
                self.nl_ignore += 1;
                // `m[i, j]`: several indices are one tuple index
                let r = (|| {
                    let mut xs = vec![self.expr()?];
                    while self.eat(&Tok::Comma) {
                        xs.push(self.expr()?);
                    }
                    Ok(xs)
                })();
                self.nl_ignore -= 1;
                let mut xs = r?;
                self.expect(&Tok::RBracket)?;
                let idx = if xs.len() == 1 {
                    xs.pop().unwrap()
                } else {
                    let span = open.to(self.prev_span());
                    Expr { kind: ExprKind::Tuple(xs), span }
                };
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
                } else if let Some(op) = self.bare_op_arg() {
                    // `gather(+)`: an operator standing alone is a function value
                    let sp = self.span();
                    self.next();
                    args.push(Expr { kind: ExprKind::OpRef(op), span: sp });
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
            // `⍬`: an empty list of i64; `⍬[f64]`, `⍬[str]`, `⍬[Point]`: of
            // that element type. Written as `{ let __z: [T] = []; __z }`,
            // which every pass already types and compiles
            Tok::Ident(name) if name == "⍬" => {
                let el = if self.at(&Tok::LBracket) && self.adjacent() {
                    self.next();
                    let t = self.ty()?;
                    self.expect(&Tok::RBracket)?;
                    t
                } else {
                    Type::Named("i64".into(), span)
                };
                let span = span.to(self.prev_span());
                let tmp = format!("__z{}", span.start);
                let ty = Type::Array(Box::new(el), span);
                let bind = Stmt::Let { name: tmp.clone(), mutable: false, ty: Some(ty), value: Expr { kind: ExprKind::Array(vec![]), span }, span };
                let tail = Expr { kind: ExprKind::Path(vec![tmp]), span };
                return Ok(Expr { kind: ExprKind::Block(Block { stmts: vec![bind], tail: Some(Box::new(tail)), span }), span });
            }
            Tok::Ident(name) => {
                if self.at(&Tok::FatArrow) && !self.no_lambda {
                    self.next();
                    let body = self.expr()?;
                    let span = span.to(body.span);
                    return Ok(Expr { kind: ExprKind::Lambda { params: vec![name], body: Box::new(body) }, span });
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
            Tok::Arms => {
                // `arms (lo = 0, hi = n) { … arm(a, b) … }`
                self.expect(&Tok::LParen)?;
                let mut params = Vec::new();
                while !self.at(&Tok::RParen) {
                    let (name, _) = self.ident()?;
                    if !self.eat(&Tok::Assign) {
                        return Err(Diag::new(&C::UNEXPECTED_TOKEN, "`arms` parameters start with a value: `arms (lo = 0, hi = n) { … }`", self.span()));
                    }
                    params.push((name, self.expr()?));
                    if !self.eat(&Tok::Comma) {
                        break;
                    }
                }
                self.expect(&Tok::RParen)?;
                let body = self.block()?;
                return Ok(Expr { span: span.to(body.span), kind: ExprKind::Arms { params, body } });
            }
            Tok::LParen if !self.no_lambda && self.lambda_params_ahead() => {
                // `(a, b) => e`, `() => e`, `(a: i64, b) => e`
                let mut params = Vec::new();
                let mut typed = Vec::new();
                while !self.at(&Tok::RParen) {
                    let (name, nspan) = self.decl_ident()?;
                    if self.eat(&Tok::Colon) {
                        // a typed parameter: the lambda takes `name·k` and
                        // its body starts `let name: Type = name·k`, which
                        // checks (and converts) as any annotated `let`
                        let t = self.ty()?;
                        let inner = format!("{name}·{}", params.len());
                        typed.push(Stmt::Let { name: name.clone(), mutable: false, ty: Some(t), value: Expr { kind: ExprKind::Path(vec![inner.clone()]), span: nspan }, span: nspan });
                        params.push(inner);
                    } else {
                        params.push(name);
                    }
                    if !self.eat(&Tok::Comma) {
                        break;
                    }
                }
                self.expect(&Tok::RParen)?;
                self.expect(&Tok::FatArrow)?;
                let mut body = self.expr()?;
                if !typed.is_empty() {
                    let bspan = body.span;
                    body = Expr { kind: ExprKind::Block(Block { stmts: typed, tail: Some(Box::new(body)), span: bspan }), span: bspan };
                }
                let span = span.to(body.span);
                return Ok(Expr { kind: ExprKind::Lambda { params, body: Box::new(body) }, span });
            }
            Tok::LParen if self.op_ref_ahead().is_some() => {
                // `(+)`: an operator as a function value
                let op = self.op_ref_ahead().unwrap();
                self.next();
                self.expect(&Tok::RParen)?;
                return Ok(Expr { kind: ExprKind::OpRef(op), span: span.to(self.prev_span()) });
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
                // `[:]` is the empty map; `[k: v, …]` a map; otherwise an array
                let r = (|| {
                    if self.at(&Tok::Colon) && matches!(self.toks.get(self.pos + 1).map(|t| &t.tok), Some(Tok::RBracket)) {
                        self.next();
                        return Ok(Err(Vec::new()));
                    }
                    let mut elems = Vec::new();
                    let mut pairs = Vec::new();
                    while !self.at(&Tok::RBracket) {
                        let x = self.expr()?;
                        if self.eat(&Tok::Colon) {
                            if !elems.is_empty() {
                                return Err(Diag::new(&C::UNEXPECTED_TOKEN, "a map literal has `key: value` for every entry", x.span));
                            }
                            pairs.push((x, self.expr()?));
                        } else {
                            if !pairs.is_empty() {
                                return Err(Diag::new(&C::UNEXPECTED_TOKEN, "a map literal has `key: value` for every entry", x.span));
                            }
                            elems.push(x);
                        }
                        self.eat(&Tok::Comma);
                    }
                    Ok(if pairs.is_empty() { Ok(elems) } else { Err(pairs) })
                })();
                self.nl_ignore -= 1;
                let r = r?;
                self.expect(&Tok::RBracket)?;
                match r {
                    Ok(elems) => ExprKind::Array(elems),
                    Err(pairs) => ExprKind::Map(pairs),
                }
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
                    let entries = self.with_entries()?;
                    let all = span.to(self.prev_span());
                    return Ok(with_desugar(base?, entries, all));
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
                            // `{x:.2}`: a format after a single `:` at the top level
                            let (code, spec) = split_format_spec(&c);
                            let mut e = match spec {
                                Some(spec) => {
                                    check_format_spec(spec).map_err(|why| Diag::new(&C::BAD_INTERPOLATION, format!("in `{{{c}}}`: {why}"), span))?;
                                    let quoted: String = spec.chars().map(|ch| if matches!(ch, '"' | '\\' | '{' | '}') { format!("\\{ch}") } else { ch.to_string() }).collect();
                                    parse_expr(&format!("std::prelude::format({code}, \"{quoted}\")"))
                                }
                                None => parse_expr(&c),
                            }
                            .map_err(|d| {
                                Diag::new(&C::BAD_INTERPOLATION, format!("in `{{{c}}}`: {}", d.msg), span)
                                    .note("`{` in a string starts an interpolation: what follows, up to `}`, is read as code")
                                    .help("for a brace in the text, write `\\{` and `\\}` (`\"\\{\\\"a\\\": 1\\}\"` is the text `{\"a\": 1}`)")
                            })?;
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

    /// The entries of `{ base with … }`: `field: value`, or a nested target
    /// such as `todos[i].done: value`.
    fn with_entries(&mut self) -> PResult<Vec<WithEntry>> {
        self.nl_ignore += 1;
        let r = (|| {
            let mut out: Vec<WithEntry> = Vec::new();
            while !self.at(&Tok::RBrace) {
                let (f, fs) = self.name_or_word()?;
                let mut segs = Vec::new();
                loop {
                    if self.eat(&Tok::Dot) {
                        segs.push(WithSeg::Field(self.name_or_word()?.0));
                    } else if self.at(&Tok::LBracket) {
                        self.next();
                        let i = self.expr()?;
                        self.expect(&Tok::RBracket)?;
                        segs.push(WithSeg::Index(i));
                    } else {
                        break;
                    }
                }
                self.expect(&Tok::Colon)?;
                let v = self.expr()?;
                if segs.is_empty() && out.iter().any(|e| e.segs.is_empty() && e.field == f) {
                    return Err(Diag::new(&C::DUPLICATE_ENTRY, format!("field `{f}` is given twice"), fs));
                }
                out.push(WithEntry { field: f, segs, value: v, span: fs });
                self.eat(&Tok::Comma);
            }
            Ok(out)
        })();
        self.nl_ignore -= 1;
        let out = r?;
        self.expect(&Tok::RBrace)?;
        Ok(out)
    }

    /// `Point { x, y: 0, .. }` (the path is already read)
    fn record_pattern(&mut self, path: Vec<String>, start: Span) -> PResult<Pattern> {
        self.expect(&Tok::LBrace)?;
        self.nl_ignore += 1;
        let r = (|| {
            let mut fields: Vec<(String, Pattern)> = Vec::new();
            let mut rest = false;
            loop {
                while self.eat(&Tok::Comma) {}
                if self.at(&Tok::RBrace) {
                    break;
                }
                if rest {
                    let s = self.span();
                    return Err(Diag::new(&C::UNEXPECTED_TOKEN, "`..` must come last in a record pattern", s));
                }
                if self.at(&Tok::Dot) {
                    self.next();
                    self.expect(&Tok::Dot)?;
                    rest = true;
                    continue;
                }
                let (f, fs) = self.ident()?;
                if fields.iter().any(|(g, _)| *g == f) {
                    return Err(Diag::new(&C::DUPLICATE_ENTRY, format!("field `{f}` appears twice in this pattern"), fs));
                }
                let p = if self.eat(&Tok::Colon) { self.pattern()? } else { Pattern::Bind(f.clone(), fs) };
                fields.push((f, p));
            }
            Ok((fields, rest))
        })();
        self.nl_ignore -= 1;
        let (fields, rest) = r?;
        let end = self.expect(&Tok::RBrace)?;
        Ok(Pattern::Record { path, fields, rest, span: start.to(end) })
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
                // `pat if cond => …`: in the guard, `x => …` is not a lambda
                let guard = if self.eat(&Tok::If) {
                    let saved = std::mem::replace(&mut self.no_lambda, true);
                    let g = self.expr();
                    self.no_lambda = saved;
                    Some(g?)
                } else {
                    None
                };
                self.expect(&Tok::FatArrow)?;
                let body = self.expr()?;
                let span = pat.span().to(body.span);
                arms.push(Arm { pat, guard, body, span });
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

    /// A pattern: alternatives `A | B`, optionally `as name`.
    fn pattern(&mut self) -> PResult<Pattern> {
        let first = self.pattern_atom()?;
        let start = first.span();
        let mut alts = vec![first];
        loop {
            // `|` may start the next line
            let save = self.pos;
            while self.nl_ignore > 0 && self.toks[self.pos].tok == Tok::Newline {
                self.pos += 1;
            }
            if !self.eat(&Tok::Bar) {
                self.pos = save;
                break;
            }
            self.skip_newlines();
            alts.push(self.pattern_atom()?);
        }
        let mut pat = if alts.len() == 1 {
            alts.pop().unwrap()
        } else {
            let span = start.to(self.prev_span());
            Pattern::Or(alts, span)
        };
        if self.eat(&Tok::As) {
            let (n, ns) = self.ident()?;
            if is_upper(&n) {
                return Err(Diag::new(&C::NAME_CASE, format!("`as` binds a variable, which starts with a lowercase letter: `{n}`"), ns));
            }
            let span = start.to(ns);
            pat = Pattern::As(Box::new(pat), n, span);
        }
        Ok(pat)
    }

    /// A literal in a pattern: `3`, `-3`, `¯3`, `2.5`, `"s"`, `true`.
    fn pattern_lit(&mut self) -> PResult<Expr> {
        let t = self.next();
        let (neg, t) = if t.tok == Tok::Minus { (true, self.next()) } else { (false, t) };
        let kind = match t.tok {
            Tok::Int(i) => ExprKind::Int(if neg { -i } else { i }),
            Tok::Float(f) => ExprKind::Float(if neg { -f } else { f }),
            Tok::Str(s) if !neg => ExprKind::Str(s),
            Tok::True if !neg => ExprKind::Bool(true),
            Tok::False if !neg => ExprKind::Bool(false),
            other => return Err(Diag::new(&C::UNEXPECTED_TOKEN, format!("expected a literal in this pattern, found {}", other.describe()), t.span)),
        };
        Ok(Expr { kind, span: t.span })
    }

    fn pattern_atom(&mut self) -> PResult<Pattern> {
        let t = self.peek_tok().clone();
        match t.tok {
            Tok::Ident(n) if n == "_" => {
                self.next();
                Ok(Pattern::Wild(t.span))
            }
            Tok::Int(_) | Tok::Float(_) | Tok::True | Tok::False | Tok::Str(_) | Tok::Minus => {
                let lo = self.pattern_lit()?;
                if self.at(&Tok::Prim(Prim::Range)) {
                    self.next();
                    let hi = self.pattern_lit()?;
                    let span = lo.span.to(hi.span);
                    return Ok(Pattern::Range { lo, hi, span });
                }
                Ok(Pattern::Lit(lo))
            }
            Tok::LBracket => {
                self.next();
                self.nl_ignore += 1;
                let r = (|| {
                    let mut elems = Vec::new();
                    let mut rest = None;
                    loop {
                        while self.eat(&Tok::Comma) {}
                        if self.at(&Tok::RBracket) {
                            break;
                        }
                        if rest.is_some() {
                            let s = self.span();
                            return Err(Diag::new(&C::UNEXPECTED_TOKEN, "`..rest` must be the last element of an array pattern", s));
                        }
                        if self.at(&Tok::Dot) {
                            self.next();
                            self.expect(&Tok::Dot)?;
                            rest = Some(match self.peek().clone() {
                                Tok::Ident(n) if n != "_" => {
                                    self.next();
                                    Some(n)
                                }
                                Tok::Ident(_) => {
                                    self.next();
                                    None
                                }
                                _ => None,
                            });
                            continue;
                        }
                        elems.push(self.pattern()?);
                    }
                    Ok((elems, rest))
                })();
                self.nl_ignore -= 1;
                let (elems, rest) = r?;
                let end = self.expect(&Tok::RBracket)?;
                Ok(Pattern::Array { elems, rest, span: t.span.to(end) })
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
                if self.at(&Tok::LBrace) {
                    return self.record_pattern(path, t.span);
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
        // `if let pat = value { … } else { … }` is a two-arm `match`
        if self.eat(&Tok::Let) {
            let pat = self.pattern()?;
            self.expect(&Tok::Assign)?;
            let saved = std::mem::replace(&mut self.nl_ignore, 0);
            let scrut = self.expr();
            self.nl_ignore = saved;
            let scrut = scrut?;
            let then = self.block()?;
            let then_span = then.span;
            let save = self.pos;
            while self.toks[self.pos].tok == Tok::Newline {
                self.pos += 1;
            }
            let els = if self.toks[self.pos].tok == Tok::Else {
                self.pos += 1;
                if self.at(&Tok::If) {
                    let s = self.next().span;
                    self.if_rest(s)?
                } else {
                    let b = self.block()?;
                    let span = b.span;
                    Expr { kind: ExprKind::Block(b), span }
                }
            } else {
                self.pos = save;
                let span = self.prev_span();
                Expr { kind: ExprKind::Block(Block { stmts: Vec::new(), tail: None, span }), span }
            };
            let span = start.to(self.prev_span());
            let pat_span = pat.span();
            let arms = vec![
                Arm { pat, guard: None, body: Expr { kind: ExprKind::Block(then), span: then_span }, span: pat_span.to(then_span) },
                Arm { pat: Pattern::Wild(els.span), guard: None, span: els.span, body: els },
            ];
            return Ok(Expr { kind: ExprKind::Match { scrut: Box::new(scrut), arms }, span });
        }
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


/// One entry of `{ base with … }`.
pub(crate) struct WithEntry {
    field: String,
    segs: Vec<WithSeg>,
    value: Expr,
    span: Span,
}

pub(crate) enum WithSeg {
    Field(String),
    Index(Expr),
}

fn ex(kind: ExprKind, span: Span) -> Expr {
    Expr { kind, span }
}

/// `{ base with a: 1, b.c: 2, d[i].e: 3 }` in plain terms: the plain fields
/// form one `with`; each nested entry is applied on top, in order, through a
/// temporary, so nothing is evaluated twice:
/// `{ let t = …; { t with d: put(t.d, i, { t.d[i] with e: 3 }) } }`.
fn with_desugar(base: Expr, entries: Vec<WithEntry>, span: Span) -> Expr {
    let (plain, nested): (Vec<WithEntry>, Vec<WithEntry>) = entries.into_iter().partition(|e| e.segs.is_empty());
    let mut cur = if !plain.is_empty() || nested.is_empty() {
        let fields = plain.into_iter().map(|e| (e.field, e.value, e.span)).collect();
        ex(ExprKind::With { base: Box::new(base), fields }, span)
    } else {
        base
    };
    for (k, e) in nested.into_iter().enumerate() {
        let t = format!("with·{}·{k}", span.start);
        let tv = ex(ExprKind::Path(vec![t.clone()]), e.span);
        let target = ex(ExprKind::Field { base: Box::new(tv.clone()), name: e.field.clone() }, e.span);
        let inner = with_path(target, e.segs, e.value, &t, 0);
        let upd = ex(ExprKind::With { base: Box::new(tv), fields: vec![(e.field, inner, e.span)] }, span);
        cur = ex(
            ExprKind::Block(Block {
                stmts: vec![Stmt::Let { name: t, mutable: false, ty: None, value: cur, span }],
                tail: Some(Box::new(upd)),
                span,
            }),
            span,
        );
    }
    cur
}

/// The new value of `target` (the current value there) after setting
/// `target` + `segs` to `value`.
fn with_path(target: Expr, mut segs: Vec<WithSeg>, value: Expr, tmp: &str, depth: usize) -> Expr {
    if segs.is_empty() {
        return value;
    }
    let span = target.span;
    match segs.remove(0) {
        WithSeg::Field(f) => {
            let next = ex(ExprKind::Field { base: Box::new(target.clone()), name: f.clone() }, span);
            let v = with_path(next, segs, value, tmp, depth + 1);
            ex(ExprKind::With { base: Box::new(target), fields: vec![(f, v, span)] }, span)
        }
        WithSeg::Index(i) => {
            let iname = format!("{tmp}·at{depth}");
            let iv = ex(ExprKind::Path(vec![iname.clone()]), i.span);
            let next = ex(ExprKind::Index { base: Box::new(target.clone()), index: Box::new(iv.clone()) }, span);
            let v = with_path(next, segs, value, tmp, depth + 1);
            let put = ex(
                ExprKind::Call { callee: Box::new(ex(ExprKind::Path(vec!["std".into(), "put".into()]), span)), args: vec![target, iv, v] },
                span,
            );
            ex(
                ExprKind::Block(Block { stmts: vec![Stmt::Let { name: iname, mutable: false, ty: None, value: i, span }], tail: Some(Box::new(put)), span }),
                span,
            )
        }
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
            ExprKind::TypeArg(t) => format!("(type {t})"),
            ExprKind::Call { callee, args } => {
                format!("({} {})", sx(callee), args.iter().map(sx).collect::<Vec<_>>().join(" "))
            }
            ExprKind::Index { base, index } => format!("(idx {} {})", sx(base), sx(index)),
            ExprKind::Unary { op, expr } => format!("({op:?} {})", sx(expr)),
            ExprKind::Binary { op, lhs, rhs } => format!("({op:?} {} {})", sx(lhs), sx(rhs)),
            ExprKind::Prim { prim, args } => {
                format!("({} {})", prim.info().word, args.iter().map(sx).collect::<Vec<_>>().join(" "))
            }
            ExprKind::Lambda { params, body } => format!("(λ {} {})", params.join(" "), sx(body)),
            ExprKind::OpRef(op) => format!("({op:?})"),
            ExprKind::Map(kvs) => format!("(map {})", kvs.iter().map(|(k, v)| format!("{}:{}", sx(k), sx(v))).collect::<Vec<_>>().join(" ")),
            ExprKind::Arms { params, .. } => format!("(arms {})", params.iter().map(|(n, x)| format!("{n}={}", sx(x))).collect::<Vec<_>>().join(" ")),
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

    #[test]
    fn type_arguments_versus_indexing() {
        // how many explicit type arguments the outermost call has (None: not a call)
        let targs = |src: &str| match parse_expr(src).unwrap().kind {
            ExprKind::Call { args, .. } => Some(args.iter().filter(|a| matches!(a.kind, ExprKind::TypeArg(_))).count()),
            _ => None,
        };
        assert_eq!(targs("json::decode[User](t)"), Some(1));
        assert_eq!(targs("json::decode[[i64]](t)"), Some(1));
        assert_eq!(targs("f[Page[User], str](t)"), Some(2));
        assert_eq!(targs("handlers[k](x)"), Some(0)); // indexing, then a call
        assert_eq!(targs("xs[i]"), None);
    }
}

/// `x:.2` → (`x`, Some(`.2`)): the format of an interpolation, after a `:`
/// that is alone (not `::`) and outside brackets, braces, parentheses and
/// strings (so `{c::x}`, `{f(size: 3)}` and `{P { x: 1 }}` have none).
fn split_format_spec(code: &str) -> (&str, Option<&str>) {
    let b = code.as_bytes();
    let (mut depth, mut quote) = (0i32, None::<u8>);
    for i in 0..b.len() {
        let ch = b[i];
        if let Some(q) = quote {
            if ch == b'\\' {
                continue;
            }
            if ch == q && (i == 0 || b[i - 1] != b'\\') {
                quote = None;
            }
            continue;
        }
        match ch {
            b'"' | b'\'' => quote = Some(ch),
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth -= 1,
            b':' if depth == 0 && b.get(i + 1) != Some(&b':') && (i == 0 || b[i - 1] != b':') => {
                return (&code[..i], Some(&code[i + 1..]));
            }
            _ => {}
        }
    }
    (code, None)
}

/// The same grammar as the runtime's `format` (runtime/mimic_std.c):
/// `[[fill]align][sign][#][0][width][,|_][.precision][type]`.
fn check_format_spec(spec: &str) -> Result<(), String> {
    let s: Vec<char> = spec.chars().collect();
    let mut i = 0;
    if s.len() > 1 && "<>^".contains(s[1]) {
        i = 2;
    } else if !s.is_empty() && "<>^".contains(s[0]) {
        i = 1;
    }
    if i < s.len() && "+- ".contains(s[i]) {
        i += 1;
    }
    if i < s.len() && s[i] == '#' {
        i += 1;
    }
    while i < s.len() && s[i].is_ascii_digit() {
        i += 1;
    }
    if i < s.len() && (s[i] == ',' || s[i] == '_') {
        i += 1;
    }
    if i < s.len() && s[i] == '.' {
        i += 1;
        if i >= s.len() || !s[i].is_ascii_digit() {
            return Err("a precision after `.`, as in `.2`".into());
        }
        while i < s.len() && s[i].is_ascii_digit() {
            i += 1;
        }
    }
    if i < s.len() && "xXobeEf%sd".contains(s[i]) {
        i += 1;
    }
    if i != s.len() {
        let rest: String = s[i..].iter().collect();
        return Err(format!("`{rest}` is not part of a format (as in `>8`, `05`, `.2`, `x`, `+,.3f`)"));
    }
    Ok(())
}
