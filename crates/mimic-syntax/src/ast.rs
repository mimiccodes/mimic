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
    /// `size: i64 = 16`: used when a call leaves the argument out.
    pub default: Option<Expr>,
    pub span: Span,
}

/// `type User = { name: str, age: i64 }` or `type Shape = Circle(f64) | Rect(f64, f64)`
#[derive(Clone, Debug)]
pub struct TypeDecl {
    pub name: String,
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
}

impl Pattern {
    pub fn span(&self) -> Span {
        match self {
            Pattern::Wild(s) | Pattern::Bind(_, s) | Pattern::Variant { span: s, .. } => *s,
            Pattern::Lit(e) => e.span,
        }
    }
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
    /// `size: 24` in a call (rewritten to a positional argument before checking)
    Named { name: String, value: Box<Expr> },
    If { cond: Box<Expr>, then: Block, els: Option<Box<Expr>> },
    Block(Block),
    Unsafe(Block),
}
