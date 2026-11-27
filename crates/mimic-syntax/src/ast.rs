//! Abstract syntax tree.

use crate::Span;
use crate::glyphs::Prim;

/// A parsed `.mim` file: its header module (first line) and everything inside it.
#[derive(Clone, Debug)]
pub struct File {
    pub module: ModuleDecl,
}

#[derive(Clone, Debug)]
pub struct ModuleDecl {
    pub name: String,
    pub is_pub: bool,
    pub items: Vec<Item>,
    pub span: Span,
    /// Byte range of the whole declaration (used by `mimic mod delete`).
    pub full: Span,
}

#[derive(Clone, Debug)]
pub enum Item {
    Type(TypeDecl),
    Fn(FnDecl),
    Module(ModuleDecl),
    Use(UseDecl),
    Extern(ExternBlock),
}

#[derive(Clone, Debug)]
pub struct FnDecl {
    pub name: String,
    /// `fn first[T](xs: [T]) -> Option[T]`
    pub type_params: Vec<String>,
    pub is_pub: bool,
    pub params: Vec<Param>,
    pub ret: Option<Type>,
    pub body: Block,
    /// `extern "C" fn name(…) { … }`: also exported to C under its plain
    /// name, with C types and the C calling convention.
    pub export_c: bool,
    /// `requires n >= 0`: checked when the function is called (debug builds
    /// and tests; see desugar::contracts)
    pub requires: Vec<Expr>,
    /// `ensures result >= 0`: checked when it returns, `result` its value
    pub ensures: Vec<Expr>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct Param {
    pub name: String,
    pub ty: Option<Type>,
    /// `size: i64 = 16`: used when a call leaves the argument out.
    pub default: Option<Expr>,
    pub span: Span,
}

/// `type User = { name: str, age: i64 }` or `type Shape = Circle(f64) | Rect(f64, f64)`
#[derive(Clone, Debug)]
pub struct TypeDecl {
    pub name: String,
    /// `type Pair[A, B] = { first: A, second: B }`
    pub params: Vec<String>,
    pub is_pub: bool,
    pub kind: TypeKind,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub enum TypeKind {
    Record(Vec<(String, Type, Span)>),
    Union(Vec<Variant>),
}

#[derive(Clone, Debug)]
pub struct Variant {
    pub name: String,
    pub fields: Vec<Type>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub enum InterpPart {
    Text(String),
    Expr(Expr),
}

#[derive(Clone, Debug)]
pub struct Arm {
    pub pat: Pattern,
    /// `pat if cond => …`
    pub guard: Option<Expr>,
    pub body: Expr,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub enum Pattern {
    /// `_`
    Wild(Span),
    /// a lowercase name binds the value
    Bind(String, Span),
    /// a literal: number, bool or string
    Lit(Expr),
    /// `Inc`, `Circle(r)`, `shapes::Rect(w, _)`
    Variant { path: Vec<String>, args: Vec<Pattern>, span: Span },
    /// `(a, 0, _)`
    Tuple(Vec<Pattern>, Span),
    /// `"/users/{id: i64}/posts/{slug}"`: text must match, captures bind
    Route(Vec<RoutePart>, Span),
    /// `A | B | C`: any alternative
    Or(Vec<Pattern>, Span),
    /// `lo ‥ hi`: lo ≤ value < hi (like the `‥` primitive)
    Range { lo: Expr, hi: Expr, span: Span },
    /// `pat as name`: match `pat` and also bind the whole value
    As(Box<Pattern>, String, Span),
    /// `Point { x, y: 0, .. }`
    Record { path: Vec<String>, fields: Vec<(String, Pattern)>, rest: bool, span: Span },
    /// `[a b ..rest]`, `[x, ..]`, `[]`: `rest` is `Some(name)` for `..name`, `Some(None)` for `..`
    Array { elems: Vec<Pattern>, rest: Option<Option<String>>, span: Span },
}

#[derive(Clone, Debug)]
pub enum RoutePart {
    Text(String),
    /// `{name}` (a string) or `{name: i64}` / `{name: f64}`
    Capture { name: String, ty: Option<String> },
}

impl Pattern {
    pub fn span(&self) -> Span {
        match self {
            Pattern::Wild(s)
            | Pattern::Bind(_, s)
            | Pattern::Variant { span: s, .. }
            | Pattern::Tuple(_, s)
            | Pattern::Route(_, s)
            | Pattern::Or(_, s)
            | Pattern::Range { span: s, .. }
            | Pattern::As(_, _, s)
            | Pattern::Record { span: s, .. }
            | Pattern::Array { span: s, .. } => *s,
            Pattern::Lit(e) => e.span,
        }
    }
}

/// Type annotation: `i64`, `f64`, `str`, `[f64]`, `ptr[u8]`, `cstr`.
#[derive(Clone, Debug, PartialEq)]
pub enum Type {
    Named(String, Span),
    /// `Option[i64]`, `Pair[str, f64]`: a generic type with arguments
    App(String, Vec<Type>, Span),
    Array(Box<Type>, Span),
    /// `[str: i64]`: a map from keys to values
    Map(Box<Type>, Box<Type>, Span),
    /// `(i64, str)`: a tuple
    Tuple(Vec<Type>, Span),
    Ptr(Box<Type>, Span),
    /// `fn(i64, f64) -> bool`: a function value.
    Fn(Vec<Type>, Option<Box<Type>>, Span),
}

impl Type {
    pub fn span(&self) -> Span {
        match self {
            Type::Named(_, s) | Type::App(_, _, s) | Type::Array(_, s) | Type::Map(_, _, s) | Type::Tuple(_, s) | Type::Ptr(_, s) | Type::Fn(_, _, s) => *s,
        }
    }
}

impl std::fmt::Display for Type {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Type::Named(n, _) => write!(f, "{n}"),
            Type::App(n, args, _) => {
                let a: Vec<String> = args.iter().map(|t| t.to_string()).collect();
                write!(f, "{n}[{}]", a.join(", "))
            }
            Type::Array(t, _) => write!(f, "[{t}]"),
            Type::Map(k, v, _) => write!(f, "[{k}: {v}]"),
            Type::Tuple(ts, _) => {
                let a: Vec<String> = ts.iter().map(|t| t.to_string()).collect();
                write!(f, "({})", a.join(", "))
            }
            Type::Ptr(t, _) => write!(f, "ptr[{t}]"),
            Type::Fn(ps, r, _) => {
                let ps: Vec<String> = ps.iter().map(|p| p.to_string()).collect();
                write!(f, "fn({})", ps.join(", "))?;
                match r {
                    Some(r) => write!(f, " -> {r}"),
                    None => Ok(()),
                }
            }
        }
    }
}

#[derive(Clone, Debug)]
pub struct UseDecl {
    pub path: Vec<String>,
    pub alias: Option<String>,
    pub span: Span,
}

impl UseDecl {
    pub fn local_name(&self) -> &str {
        self.alias.as_deref().unwrap_or_else(|| self.path.last().map(String::as_str).unwrap_or(""))
    }
}

#[derive(Clone, Debug)]
pub struct ExternBlock {
    pub abi: String,
    pub trusted: bool,
    /// What the functions do besides computing a result (`extern "C" pure { … }`).
    pub effect: Effect,
    pub fns: Vec<ExternFn>,
    /// `include "math.h"`: C headers whose declarations are imported
    /// (filled into `fns` by the C importer before checking).
    pub includes: Vec<(String, Span)>,
    /// `pub extern …`: imported declarations are public.
    pub is_pub: bool,
    /// `extern "C" include "sqlite3.h" as sql`: the namespace the names live
    /// in (`sql::…`); `c::` when not given.
    pub alias: Option<String>,
    pub span: Span,
}

/// The effects of foreign functions, declared on their `extern` block. The
/// compiler trusts the declaration: it decides what may run in parallel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Effect {
    /// No effects at all: calls may run in parallel, be reordered or skipped.
    Pure,
    /// Effects that may overlap each other (reads, queries, requests):
    /// calls become implicit futures and run in parallel under `¨`.
    Concurrent,
    /// Effects in program order that change nothing other calls read
    /// (printing, logging).
    Ordered,
    /// Effects that change shared state (writes): pending futures finish
    /// first. The default for C functions.
    Sync,
}

impl Effect {
    pub fn from_word(w: &str) -> Option<Effect> {
        Some(match w {
            "pure" => Effect::Pure,
            "concurrent" => Effect::Concurrent,
            "ordered" => Effect::Ordered,
            "sync" => Effect::Sync,
            _ => return None,
        })
    }

    /// (effects, overlap, sync) flags of an extern function.
    pub fn flags(self) -> (bool, bool, bool) {
        match self {
            Effect::Pure => (false, false, false),
            Effect::Concurrent => (true, true, false),
            Effect::Ordered => (true, false, false),
            Effect::Sync => (true, false, true),
        }
    }
}

#[derive(Clone, Debug)]
pub struct ExternFn {
    pub name: String,
    pub is_pub: bool,
    pub params: Vec<(String, Type)>,
    /// `fn printf(fmt: cstr, ...)`: more arguments may follow the fixed ones.
    pub variadic: bool,
    /// The symbol to call, when it differs from `name` (generated C wrappers).
    pub link_name: Option<String>,
    /// `extern "mimic"`: implemented in Mimic's runtime, taking and returning
    /// Mimic values (memory safe: no `unsafe` needed).
    pub intrinsic: bool,
    /// `fn decode[T](text: str) -> Result[T, str]`: given explicitly at the call
    pub type_params: Vec<String>,
    /// `extern "mimic-io"`: an intrinsic with side effects (files, environment,
    /// stdin…): never run in parallel or out of order.
    pub effects: bool,
    /// `extern "mimic-io-concurrent"`: effects that may overlap each other
    /// (reads, sleeping): calls can run concurrently as implicit futures.
    pub overlap: bool,
    /// `extern "mimic-io-sync"`: changes state that overlapping effects read
    /// (files, environment, processes): waits for pending implicit futures.
    pub sync: bool,
    pub ret: Option<Type>,
    pub span: Span,
}

#[derive(Clone, Debug, Default)]
pub struct Block {
    pub stmts: Vec<Stmt>,
    pub tail: Option<Box<Expr>>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub enum Stmt {
    Let { name: String, mutable: bool, ty: Option<Type>, value: Expr, span: Span },
    Assign { name: String, index: Option<Expr>, value: Expr, span: Span },
    Expr(Expr),
    Return(Option<Expr>, Span),
    /// `let (a, b) = value` (`_` skips an element)
    LetTuple { names: Vec<String>, value: Expr, span: Span },
    /// `let x = value else { … }`: unwraps `Some`/`Ok`; otherwise the block runs and must leave
    LetElse { name: String, ty: Option<Type>, value: Expr, els: Block, span: Span },
    /// `while cond { … }` (`⟳`): runs the body while `cond` holds; the body
    /// may reassign only its function's `let mut` variables. (`for` is
    /// written as one by the parser.)
    While { cond: Expr, body: Block, span: Span },
    /// `break` (`↯`): leaves the innermost loop.
    Break(Span),
}

#[derive(Clone, Debug)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinOp {
    Add = 0,
    Sub = 1,
    Mul = 2,
    Div = 3,
    Mod = 4,
    Eq = 5,
    Ne = 6,
    Lt = 7,
    Le = 8,
    Gt = 9,
    Ge = 10,
    And = 11,
    Or = 12,
    IntDiv = 13,
    Max = 14,
    Min = 15,
    BitAnd = 16,
    BitOr = 17,
    BitXor = 18,
    Shl = 19,
    /// arithmetic (sign-propagating) shift right
    Shr = 20,
    /// logical (zero-filling) shift right
    UShr = 21,
}

impl BinOp {
    /// How the operator is written in source.
    pub fn symbol(self) -> &'static str {
        use BinOp::*;
        match self {
            Add => "+",
            Sub => "-",
            Mul => "*",
            Div => "/",
            Mod => "%",
            Eq => "==",
            Ne => "!=",
            Lt => "<",
            Le => "<=",
            Gt => ">",
            Ge => ">=",
            And => "and",
            Or => "or",
            IntDiv => "div",
            Max => "max",
            Min => "min",
            BitAnd => "&",
            BitOr => "|",
            BitXor => "^",
            Shl => "<<",
            Shr => ">>",
            UShr => ">>>",
        }
    }

    /// `& | ^ << >> >>>`: integer-only operators.
    pub fn is_bitwise(self) -> bool {
        matches!(self, BinOp::BitAnd | BinOp::BitOr | BinOp::BitXor | BinOp::Shl | BinOp::Shr | BinOp::UShr)
    }

    pub fn is_shift(self) -> bool {
        matches!(self, BinOp::Shl | BinOp::Shr | BinOp::UShr)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnOp {
    Neg = 0,
    Not = 1,
    Abs = 2,
    /// `~x`: bitwise complement
    BitNot = 3,
}

#[derive(Clone, Debug)]
pub enum ExprKind {
    Int(i64),
    Float(f64),
    Str(String),
    Bool(bool),
    Array(Vec<Expr>),
    /// A name or `a::b::c` path.
    Path(Vec<String>),
    Call { callee: Box<Expr>, args: Vec<Expr> },
    /// An explicit type argument, `User` in `json::decode[User](text)`: it
    /// becomes a leading argument of the call (a schema the runtime follows).
    TypeArg(Type),
    Index { base: Box<Expr>, index: Box<Expr> },
    Unary { op: UnOp, expr: Box<Expr> },
    Binary { op: BinOp, lhs: Box<Expr>, rhs: Box<Expr> },
    /// A fully applied primitive (glyph or word form).
    Prim { prim: Prim, args: Vec<Expr> },
    /// `x => e`, `(a, b) => e`: may capture variables of the enclosing function.
    Lambda { params: Vec<String>, body: Box<Expr> },
    /// `(+)`: an operator as a function value.
    OpRef(BinOp),
    /// `["a": 1, "b": 2]`, `[:]`: a map.
    Map(Vec<(Expr, Expr)>),
    /// `arms (lo = 0, hi = n) { … arm(a, b) … }`: divide-and-conquer
    /// recursion; lifted to a hidden function before checking.
    Arms { params: Vec<(String, Expr)>, body: Block },
    /// `u.name`
    Field { base: Box<Expr>, name: String },
    /// `User { name: "ada", age: 36 }`
    Record { path: Vec<String>, fields: Vec<(String, Expr, Span)> },
    /// `{ u with age: 37 }`
    With { base: Box<Expr>, fields: Vec<(String, Expr, Span)> },
    /// `match x { pattern => value … }`
    Match { scrut: Box<Expr>, arms: Vec<Arm> },
    /// `"hi {name}"`
    Interp(Vec<InterpPart>),
    /// `(a, b, c)`
    Tuple(Vec<Expr>),
    /// `size: 24` in a call (rewritten to a positional argument before checking)
    Named { name: String, value: Box<Expr> },
    If { cond: Box<Expr>, then: Block, els: Option<Box<Expr>> },
    Block(Block),
    Unsafe(Block),
}

impl Expr {
    /// Visit this expression and every expression inside it (blocks, arms,
    /// interpolations, nested lambdas), parents before children.
    pub fn walk(&self, f: &mut dyn FnMut(&Expr)) {
        f(self);
        let block = |b: &Block, f: &mut dyn FnMut(&Expr)| b.walk(f);
        match &self.kind {
            ExprKind::Int(_) | ExprKind::Float(_) | ExprKind::Str(_) | ExprKind::Bool(_) | ExprKind::Path(_) | ExprKind::OpRef(_) | ExprKind::TypeArg(_) => {}
            ExprKind::Array(xs) | ExprKind::Tuple(xs) | ExprKind::Prim { args: xs, .. } => xs.iter().for_each(|x| x.walk(f)),
            ExprKind::Call { callee, args } => {
                callee.walk(f);
                args.iter().for_each(|x| x.walk(f));
            }
            ExprKind::Index { base, index } => {
                base.walk(f);
                index.walk(f);
            }
            ExprKind::Unary { expr, .. } | ExprKind::Field { base: expr, .. } | ExprKind::Named { value: expr, .. } => expr.walk(f),
            ExprKind::Lambda { body, .. } => body.walk(f),
            ExprKind::Binary { lhs, rhs, .. } => {
                lhs.walk(f);
                rhs.walk(f);
            }
            ExprKind::Record { fields, .. } => fields.iter().for_each(|(_, x, _)| x.walk(f)),
            ExprKind::With { base, fields } => {
                base.walk(f);
                fields.iter().for_each(|(_, x, _)| x.walk(f));
            }
            ExprKind::Match { scrut, arms } => {
                scrut.walk(f);
                for a in arms {
                    if let Some(g) = &a.guard {
                        g.walk(f);
                    }
                    a.body.walk(f);
                }
            }
            ExprKind::Interp(parts) => parts.iter().for_each(|p| {
                if let InterpPart::Expr(x) = p {
                    x.walk(f)
                }
            }),
            ExprKind::If { cond, then, els } => {
                cond.walk(f);
                block(then, f);
                if let Some(e) = els {
                    e.walk(f);
                }
            }
            ExprKind::Block(b) | ExprKind::Unsafe(b) => block(b, f),
            ExprKind::Arms { params, body } => {
                params.iter().for_each(|(_, x)| x.walk(f));
                block(body, f);
            }
            ExprKind::Map(kvs) => kvs.iter().for_each(|(k, v)| {
                k.walk(f);
                v.walk(f);
            }),
        }
    }
}

impl Block {
    /// Visit every expression in the block (see [`Expr::walk`]).
    pub fn walk(&self, f: &mut dyn FnMut(&Expr)) {
        for s in &self.stmts {
            match s {
                Stmt::Let { value, .. } | Stmt::LetTuple { value, .. } | Stmt::Expr(value) => value.walk(f),
                Stmt::Assign { index, value, .. } => {
                    if let Some(i) = index {
                        i.walk(f);
                    }
                    value.walk(f);
                }
                Stmt::Return(e, _) => {
                    if let Some(e) = e {
                        e.walk(f);
                    }
                }
                Stmt::LetElse { value, els, .. } => {
                    value.walk(f);
                    els.walk(f);
                }
                Stmt::While { cond, body, .. } => {
                    cond.walk(f);
                    body.walk(f);
                }
                Stmt::Break(_) => {}
            }
        }
        if let Some(t) = &self.tail {
            t.walk(f);
        }
    }
}

impl Expr {
    /// Visit every expression bottom-up (children before parents), mutably.
    pub fn walk_mut(&mut self, f: &mut dyn FnMut(&mut Expr)) {
        match &mut self.kind {
            ExprKind::Int(_) | ExprKind::Float(_) | ExprKind::Str(_) | ExprKind::Bool(_) | ExprKind::Path(_) | ExprKind::OpRef(_) | ExprKind::TypeArg(_) => {}
            ExprKind::Array(xs) | ExprKind::Tuple(xs) | ExprKind::Prim { args: xs, .. } => xs.iter_mut().for_each(|x| x.walk_mut(f)),
            ExprKind::Call { callee, args } => {
                callee.walk_mut(f);
                args.iter_mut().for_each(|x| x.walk_mut(f));
            }
            ExprKind::Index { base, index } => {
                base.walk_mut(f);
                index.walk_mut(f);
            }
            ExprKind::Unary { expr, .. } | ExprKind::Field { base: expr, .. } | ExprKind::Named { value: expr, .. } => expr.walk_mut(f),
            ExprKind::Lambda { body, .. } => body.walk_mut(f),
            ExprKind::Binary { lhs, rhs, .. } => {
                lhs.walk_mut(f);
                rhs.walk_mut(f);
            }
            ExprKind::Record { fields, .. } => fields.iter_mut().for_each(|(_, x, _)| x.walk_mut(f)),
            ExprKind::With { base, fields } => {
                base.walk_mut(f);
                fields.iter_mut().for_each(|(_, x, _)| x.walk_mut(f));
            }
            ExprKind::Match { scrut, arms } => {
                scrut.walk_mut(f);
                for a in arms {
                    if let Some(g) = &mut a.guard {
                        g.walk_mut(f);
                    }
                    a.body.walk_mut(f);
                }
            }
            ExprKind::Interp(parts) => parts.iter_mut().for_each(|p| {
                if let InterpPart::Expr(x) = p {
                    x.walk_mut(f)
                }
            }),
            ExprKind::If { cond, then, els } => {
                cond.walk_mut(f);
                then.walk_mut(f);
                if let Some(e) = els {
                    e.walk_mut(f);
                }
            }
            ExprKind::Block(b) | ExprKind::Unsafe(b) => b.walk_mut(f),
            ExprKind::Arms { params, body } => {
                params.iter_mut().for_each(|(_, x)| x.walk_mut(f));
                body.walk_mut(f);
            }
            ExprKind::Map(kvs) => kvs.iter_mut().for_each(|(k, v)| {
                k.walk_mut(f);
                v.walk_mut(f);
            }),
        }
        f(self);
    }
}

impl Block {
    /// See [`Expr::walk_mut`].
    pub fn walk_mut(&mut self, f: &mut dyn FnMut(&mut Expr)) {
        for s in &mut self.stmts {
            match s {
                Stmt::Let { value, .. } | Stmt::LetTuple { value, .. } | Stmt::Expr(value) => value.walk_mut(f),
                Stmt::Assign { index, value, .. } => {
                    if let Some(i) = index {
                        i.walk_mut(f);
                    }
                    value.walk_mut(f);
                }
                Stmt::Return(e, _) => {
                    if let Some(e) = e {
                        e.walk_mut(f);
                    }
                }
                Stmt::LetElse { value, els, .. } => {
                    value.walk_mut(f);
                    els.walk_mut(f);
                }
                Stmt::While { cond, body, .. } => {
                    cond.walk_mut(f);
                    body.walk_mut(f);
                }
                Stmt::Break(_) => {}
            }
        }
        if let Some(t) = &mut self.tail {
            t.walk_mut(f);
        }
    }

    /// Every name this block binds anywhere inside it (lets, lambda and arms
    /// parameters, pattern bindings).
    pub fn bound_names(&self, out: &mut Vec<String>) {
        for s in &self.stmts {
            match s {
                Stmt::Let { name, .. } | Stmt::LetElse { name, .. } => out.push(name.clone()),
                Stmt::LetTuple { names, .. } => out.extend(names.iter().cloned()),
                _ => {}
            }
            if let Stmt::LetElse { els, .. } = s {
                els.bound_names(out);
            }
            if let Stmt::While { body, .. } = s {
                body.bound_names(out);
            }
        }
        self.walk(&mut |e| match &e.kind {
            ExprKind::Lambda { params, .. } => out.extend(params.iter().cloned()),
            ExprKind::Arms { params, body } => {
                out.extend(params.iter().map(|(n, _)| n.clone()));
                body.bound_names(out);
            }
            ExprKind::Block(b) | ExprKind::Unsafe(b) => b.bound_names(out),
            ExprKind::If { then, .. } => then.bound_names(out),
            ExprKind::Match { arms, .. } => {
                for a in arms {
                    a.pat.bound_names(out);
                }
            }
            _ => {}
        });
    }
}

impl Pattern {
    pub fn bound_names(&self, out: &mut Vec<String>) {
        match self {
            Pattern::Bind(n, _) => out.push(n.clone()),
            Pattern::Variant { args, .. } | Pattern::Tuple(args, _) => args.iter().for_each(|a| a.bound_names(out)),
            // alternatives bind the same names
            Pattern::Or(alts, _) => alts[0].bound_names(out),
            Pattern::As(p, n, _) => {
                p.bound_names(out);
                out.push(n.clone());
            }
            Pattern::Record { fields, .. } => fields.iter().for_each(|(_, p)| p.bound_names(out)),
            Pattern::Array { elems, rest, .. } => {
                elems.iter().for_each(|p| p.bound_names(out));
                if let Some(Some(n)) = rest {
                    out.push(n.clone());
                }
            }
            Pattern::Route(parts, _) => {
                for p in parts {
                    if let RoutePart::Capture { name: n, .. } = p {
                        out.push(n.clone());
                    }
                }
            }
            _ => {}
        }
    }
}
