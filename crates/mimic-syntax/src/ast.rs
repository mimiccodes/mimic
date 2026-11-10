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
    Fn(FnDecl),
    Module(ModuleDecl),
    Use(UseDecl),
    Extern(ExternBlock),
}

#[derive(Clone, Debug)]
pub struct FnDecl {
    pub name: String,
    pub is_pub: bool,
    pub params: Vec<Param>,
    pub ret: Option<Type>,
    pub body: Block,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct Param {
    pub name: String,
    pub ty: Option<Type>,
    pub span: Span,
}

/// Type annotation: `i64`, `f64`, `str`, `[f64]`, `ptr[u8]`, `cstr`.
#[derive(Clone, Debug, PartialEq)]
pub enum Type {
    Named(String, Span),
    Array(Box<Type>, Span),
    Ptr(Box<Type>, Span),
}

impl Type {
    pub fn span(&self) -> Span {
        match self {
            Type::Named(_, s) | Type::Array(_, s) | Type::Ptr(_, s) => *s,
        }
    }
}

impl std::fmt::Display for Type {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Type::Named(n, _) => write!(f, "{n}"),
            Type::Array(t, _) => write!(f, "[{t}]"),
            Type::Ptr(t, _) => write!(f, "ptr[{t}]"),
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
    pub fns: Vec<ExternFn>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct ExternFn {
    pub name: String,
    pub is_pub: bool,
    pub params: Vec<(String, Type)>,
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
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnOp {
    Neg = 0,
    Not = 1,
    Abs = 2,
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
    Index { base: Box<Expr>, index: Box<Expr> },
    Unary { op: UnOp, expr: Box<Expr> },
    Binary { op: BinOp, lhs: Box<Expr>, rhs: Box<Expr> },
    /// A fully applied primitive (glyph or word form).
    Prim { prim: Prim, args: Vec<Expr> },
    Lambda { param: String, body: Box<Expr> },
    If { cond: Box<Expr>, then: Block, els: Option<Box<Expr>> },
    Block(Block),
    Unsafe(Block),
}
