//! Interaction-net IR for the Mimic runtime.
//!
//! A [`Book`] is a set of global definitions; each [`Def`] is a net template:
//! a root tree plus a list of redexes (active pairs). The runtime expands a
//! definition lazily when its `@name` reference meets another node.
//!
//! Text format (`.mnet`):
//!
//! ```text
//! // fib(n) = n < 2 ? n : fib(n-1) + fib(n-2)
//! @fib    = ({n0 n1} r) & n0 ~ $<(#2 c) & c ~ ?((@fib_lt @fib_ge) (n1 r))
//! @fib_lt = (n n)
//! @fib_ge = ({a b} r)
//!         & a ~ $-(#1 a1) & @fib ~ (a1 ra)
//!         & b ~ $-(#2 b1) & @fib ~ (b1 rb)
//!         & ra ~ $+(rb r)
//! @main   = r & @fib ~ (#20 r)
//! ```
//!
//! | syntax        | node                                                     |
//! |---------------|----------------------------------------------------------|
//! | `x`           | wire (every name appears exactly twice in a definition)  |
//! | `*`           | eraser                                                   |
//! | `#5 #-2 #2.5 #true` | number                                             |
//! | `@name`       | reference to a definition (expanded lazily)              |
//! | `(a b)`       | constructor (function: argument, result)                 |
//! | `{a b}` `{3 a b}` | duplicator, optional label                          |
//! | `$op(b r)`    | operator: principal ⊕ b → r; ops: `+ - * / % == != < <= > >= and or neg not` and primitive words (`iota sum take` …) |
//! | `?(sel r)`    | switch: principal ≠ 0 → `sel` gets `(r *)`, else `(* r)` |

pub mod sim;

use mimic_syntax::ast::BinOp;
use mimic_syntax::glyphs::{PRIMS, Prim};
use std::collections::HashMap;
use std::fmt;

// ---- IR ---------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Num {
    Int(i64),
    F64(f64),
    Bool(bool),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Tree {
    Era,
    Var(String),
    Num(Num),
    Ref(String),
    Con(Box<Tree>, Box<Tree>),
    Dup(u32, Box<Tree>, Box<Tree>),
    Opr(Op, Box<Tree>, Box<Tree>),
    Swi(Box<Tree>, Box<Tree>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Def {
    pub name: String,
    pub root: Tree,
    pub redexes: Vec<(Tree, Tree)>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Book {
    pub defs: Vec<Def>,
}

/// Operator codes, matching the runtime (`OPR` aux field).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    Bin(BinOp),
    Neg,
    Not,
    Abs,
    BitNot,
    Prim1(Prim),
    Prim2(Prim),
    /// `xs[i]`
    Index,
    /// Runtime type check (type code, message index); passes the value through.
    Check(u8, u16),
}

impl Op {
    pub fn code(self) -> u32 {
        match self {
            Op::Bin(b) => b as u32,
            Op::Neg => 24,
            Op::Not => 25,
            Op::Abs => 26,
            Op::BitNot => 27,
            Op::Prim1(p) => 32 + p as u32,
            Op::Prim2(p) => 64 + p as u32,
            Op::Index => 96,
            Op::Check(ty, msg) => (128 + ty as u32) | ((msg as u32) << 12),
        }
    }

    pub fn is_unary(self) -> bool {
        matches!(self, Op::Neg | Op::Not | Op::Abs | Op::BitNot | Op::Prim1(_) | Op::Check(..))
    }

    pub fn name(self) -> String {
        match self {
            Op::Bin(b) => BIN_OPS.iter().find(|(_, o)| *o == b).map(|(n, _)| n.to_string()).unwrap(),
            Op::Neg => "neg".into(),
            Op::Not => "not".into(),
            Op::Abs => "abs".into(),
            Op::BitNot => "bnot".into(),
            Op::Prim1(p) | Op::Prim2(p) => p.info().word.into(),
            Op::Index => "index".into(),
            Op::Check(ty, msg) => format!("check{ty}_{msg}"),
        }
    }

    pub fn parse(s: &str) -> Option<Op> {
        if let Some((_, b)) = BIN_OPS.iter().find(|(n, _)| *n == s) {
            return Some(Op::Bin(*b));
        }
        match s {
            "neg" => return Some(Op::Neg),
            "not" => return Some(Op::Not),
            "abs" => return Some(Op::Abs),
            "bnot" => return Some(Op::BitNot),
            "index" => return Some(Op::Index),
            c if c.starts_with("check") => {
                let (ty, msg) = c[5..].split_once('_')?;
                return Some(Op::Check(ty.parse().ok()?, msg.parse().ok()?));
            }
            _ => {}
        }
        let p = PRIMS.iter().find(|p| p.word == s)?;
        match (p.prim, p.arity) {
            (Prim::Print | Prim::Each, _) => None, // effects / higher-order: not net operators
            (Prim::Max | Prim::Min | Prim::Abs, _) => None, // spelled as `$max` etc. (binary ops above)
            (prim, 1) => Some(Op::Prim1(prim)),
            (prim, _) => Some(Op::Prim2(prim)),
        }
    }
}

const BIN_OPS: &[(&str, BinOp)] = &[
    ("+", BinOp::Add),
    ("-", BinOp::Sub),
    ("*", BinOp::Mul),
    ("/", BinOp::Div),
    ("%", BinOp::Mod),
    ("==", BinOp::Eq),
    ("!=", BinOp::Ne),
    ("<", BinOp::Lt),
    ("<=", BinOp::Le),
    (">", BinOp::Gt),
    (">=", BinOp::Ge),
    ("and", BinOp::And),
    ("or", BinOp::Or),
    ("div", BinOp::IntDiv),
    ("max", BinOp::Max),
    ("min", BinOp::Min),
    ("band", BinOp::BitAnd),
    ("bor", BinOp::BitOr),
    ("bxor", BinOp::BitXor),
    ("shl", BinOp::Shl),
    ("shr", BinOp::Shr),
    ("ushr", BinOp::UShr),
];

// ---- compile-time pre-reduction ---------------------------------------------

impl Tree {
    /// Replace the occurrence of variable `name` with `t`; true if found.
    fn subst(&mut self, name: &str, t: &Tree) -> bool {
        match self {
            Tree::Var(v) if v == name => {
                *self = t.clone();
                true
            }
            Tree::Con(a, b) | Tree::Dup(_, a, b) | Tree::Opr(_, a, b) | Tree::Swi(a, b) => a.subst(name, t) || b.subst(name, t),
            _ => false,
        }
    }

    fn has_var(&self, name: &str) -> bool {
        match self {
            Tree::Var(v) => v == name,
            Tree::Con(a, b) | Tree::Dup(_, a, b) | Tree::Opr(_, a, b) | Tree::Swi(a, b) => a.has_var(name) || b.has_var(name),
            _ => false,
        }
    }

    fn size(&self) -> usize {
        match self {
            Tree::Con(a, b) | Tree::Dup(_, a, b) | Tree::Opr(_, a, b) | Tree::Swi(a, b) => 1 + a.size() + b.size(),
            _ => 0,
        }
    }
}

/// `x op y` on two numbers, where it is certain to be what the runtime
/// computes (wrapping integer + - ×); None to leave it to the runtime.
fn fold(op: Op, x: Num, y: Num) -> Option<Num> {
    match (op, x, y) {
        (Op::Bin(BinOp::Add), Num::Int(a), Num::Int(b)) => Some(Num::Int(a.wrapping_add(b))),
        (Op::Bin(BinOp::Sub), Num::Int(a), Num::Int(b)) => Some(Num::Int(a.wrapping_sub(b))),
        (Op::Bin(BinOp::Mul), Num::Int(a), Num::Int(b)) => Some(Num::Int(a.wrapping_mul(b))),
        _ => None,
    }
}

impl Def {
    /// Reduce at compile time what the runtime would reduce first anyway:
    /// every interaction that needs no definition expanded (annihilation,
    /// erasure, copying numbers, constant arithmetic, a switch on a known
    /// number), and a wire between a variable and a tree replaced by the tree
    /// itself (no wire to allocate at run time). References are never
    /// expanded, so laziness and recursion are as before.
    pub fn pre_reduce(&mut self) {
        let mut work: Vec<(Tree, Tree)> = std::mem::take(&mut self.redexes);
        let mut kept: Vec<(Tree, Tree)> = Vec::new();
        let mut steps = 0;
        while let Some((a, b)) = work.pop() {
            steps += 1;
            if steps > 100_000 {
                kept.push((a, b));
                kept.append(&mut work);
                break;
            }
            match (a, b) {
                // a wire: the other end of the variable gets the tree
                (Tree::Var(x), t) | (t, Tree::Var(x)) => {
                    if t.has_var(&x) {
                        kept.push((Tree::Var(x), t));
                        continue;
                    }
                    let mut done = self.root.subst(&x, &t);
                    for (p, q) in work.iter_mut().chain(kept.iter_mut()) {
                        if done {
                            break;
                        }
                        done = p.subst(&x, &t) || q.subst(&x, &t);
                    }
                    if !done {
                        kept.push((Tree::Var(x), t));
                    } else {
                        // the substituted tree may now face another tree in a
                        // kept redex: look at those again
                        let again: Vec<(Tree, Tree)> = std::mem::take(&mut kept);
                        work.extend(again);
                    }
                }
                (Tree::Con(a1, a2), Tree::Con(b1, b2)) => {
                    work.push((*a1, *b1));
                    work.push((*a2, *b2));
                }
                (Tree::Dup(l, a1, a2), Tree::Dup(m, b1, b2)) if l == m => {
                    work.push((*a1, *b1));
                    work.push((*a2, *b2));
                }
                // two nullary nodes: nothing (a reference met by an eraser
                // or a number is dropped unexpanded, as the runtime does)
                (Tree::Era | Tree::Num(_) | Tree::Ref(_), Tree::Era | Tree::Num(_) | Tree::Ref(_)) => {}
                (Tree::Era, Tree::Con(x, y) | Tree::Dup(_, x, y) | Tree::Opr(_, x, y) | Tree::Swi(x, y))
                | (Tree::Con(x, y) | Tree::Dup(_, x, y) | Tree::Opr(_, x, y) | Tree::Swi(x, y), Tree::Era) => {
                    work.push((*x, Tree::Era));
                    work.push((*y, Tree::Era));
                }
                (Tree::Num(n), Tree::Dup(_, x, y)) | (Tree::Dup(_, x, y), Tree::Num(n)) => {
                    work.push((*x, Tree::Num(n)));
                    work.push((*y, Tree::Num(n)));
                }
                (Tree::Num(n), Tree::Swi(sel, r)) | (Tree::Swi(sel, r), Tree::Num(n)) if matches!(*sel, Tree::Con(..)) => {
                    let truth = match n {
                        Num::Int(i) => i != 0,
                        Num::F64(f) => f != 0.0,
                        Num::Bool(b) => b,
                    };
                    let Tree::Con(yes, no) = *sel else { unreachable!() };
                    let (take, drop) = if truth { (yes, no) } else { (no, yes) };
                    work.push((*take, *r));
                    work.push((*drop, Tree::Era));
                }
                (Tree::Num(n), Tree::Opr(op, y, r)) | (Tree::Opr(op, y, r), Tree::Num(n)) if !op.is_unary() && matches!(*y, Tree::Num(_)) => {
                    let Tree::Num(m) = *y else { unreachable!() };
                    match fold(op, n, m) {
                        Some(v) => work.push((*r, Tree::Num(v))),
                        None => kept.push((Tree::Num(n), Tree::Opr(op, Box::new(Tree::Num(m)), r))),
                    }
                }
                (a, b) => kept.push((a, b)),
            }
        }
        kept.reverse();
        self.redexes = kept;
    }

    /// Nodes in the definition (for tests and statistics).
    pub fn node_count(&self) -> usize {
        self.root.size() + self.redexes.iter().map(|(a, b)| a.size() + b.size()).sum::<usize>()
    }
}

impl Book {
    /// Pre-reduce every definition (Def::pre_reduce).
    pub fn pre_reduce(&mut self) {
        for d in &mut self.defs {
            d.pre_reduce();
        }
    }
}

// ---- printing ---------------------------------------------------------------

impl fmt::Display for Num {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Num::Int(i) => write!(f, "#{i}"),
            Num::F64(x) => write!(f, "#{x:?}"),
            Num::Bool(b) => write!(f, "#{b}"),
        }
    }
}

impl fmt::Display for Tree {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Tree::Era => write!(f, "*"),
            Tree::Var(v) => write!(f, "{v}"),
            Tree::Num(n) => write!(f, "{n}"),
            Tree::Ref(r) => write!(f, "@{r}"),
            Tree::Con(a, b) => write!(f, "({a} {b})"),
            Tree::Dup(0, a, b) => write!(f, "{{{a} {b}}}"),
            Tree::Dup(l, a, b) => write!(f, "{{{l} {a} {b}}}"),
            Tree::Opr(op, a, b) => write!(f, "${}({a} {b})", op.name()),
            Tree::Swi(a, b) => write!(f, "?({a} {b})"),
        }
    }
}

impl fmt::Display for Def {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "@{} = {}", self.name, self.root)?;
        for (a, b) in &self.redexes {
            write!(f, "\n  & {a} ~ {b}")?;
        }
        Ok(())
    }
}

impl fmt::Display for Book {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for d in &self.defs {
            writeln!(f, "{d}")?;
        }
        Ok(())
    }
}

// ---- parsing ----------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct ParseError {
    pub msg: String,
    pub line: usize,
    pub col: usize,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}: {}", self.line, self.col, self.msg)
    }
}

pub fn parse(src: &str) -> Result<Book, ParseError> {
    let mut p = P { s: src.as_bytes(), src, i: 0 };
    let mut book = Book::default();
    loop {
        p.ws();
        if p.i >= p.s.len() {
            break;
        }
        p.expect(b'@')?;
        let name = p.name()?;
        p.ws();
        p.expect(b'=')?;
        let root = p.tree()?;
        let mut redexes = Vec::new();
        loop {
            p.ws();
            if p.peek() != Some(b'&') {
                break;
            }
            p.i += 1;
            let a = p.tree()?;
            p.ws();
            p.expect(b'~')?;
            let b = p.tree()?;
            redexes.push((a, b));
        }
        if book.defs.iter().any(|d| d.name == name) {
            return Err(p.err(format!("@{name} is defined twice")));
        }
        book.defs.push(Def { name, root, redexes });
    }
    Ok(book)
}

struct P<'a> {
    s: &'a [u8],
    src: &'a str,
    i: usize,
}

impl P<'_> {
    fn err(&self, msg: String) -> ParseError {
        let before = &self.src[..self.i.min(self.src.len())];
        let line = before.matches('\n').count() + 1;
        let col = before.rsplit('\n').next().map(|l| l.chars().count()).unwrap_or(0) + 1;
        ParseError { msg, line, col }
    }
    fn peek(&self) -> Option<u8> {
        self.s.get(self.i).copied()
    }
    fn ws(&mut self) {
        while let Some(c) = self.peek() {
            if c.is_ascii_whitespace() {
                self.i += 1;
            } else if c == b'/' && self.s.get(self.i + 1) == Some(&b'/') {
                while self.peek().is_some_and(|c| c != b'\n') {
                    self.i += 1;
                }
            } else {
                break;
            }
        }
    }
    fn expect(&mut self, c: u8) -> Result<(), ParseError> {
        self.ws();
        if self.peek() == Some(c) {
            self.i += 1;
            Ok(())
        } else {
            Err(self.err(format!("expected `{}`", c as char)))
        }
    }
    fn name(&mut self) -> Result<String, ParseError> {
        let start = self.i;
        while self.peek().is_some_and(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'.') {
            self.i += 1;
        }
        if start == self.i {
            return Err(self.err("expected a name".into()));
        }
        Ok(self.src[start..self.i].to_string())
    }
    fn tree(&mut self) -> Result<Tree, ParseError> {
        self.ws();
        let c = self.peek().ok_or_else(|| self.err("unexpected end of input".into()))?;
        Ok(match c {
            b'*' => {
                self.i += 1;
                Tree::Era
            }
            b'@' => {
                self.i += 1;
                Tree::Ref(self.name()?)
            }
            b'#' => {
                self.i += 1;
                let start = self.i;
                while self.peek().is_some_and(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'+' | b'.')) {
                    self.i += 1;
                }
                let t = &self.src[start..self.i];
                Tree::Num(match t {
                    "true" => Num::Bool(true),
                    "false" => Num::Bool(false),
                    _ if t.contains(['.', 'e']) => Num::F64(t.parse().map_err(|_| self.err(format!("bad number `#{t}`")))?),
                    _ => Num::Int(t.parse().map_err(|_| self.err(format!("bad number `#{t}`")))?),
                })
            }
            b'(' => {
                self.i += 1;
                let a = self.tree()?;
                let b = self.tree()?;
                self.expect(b')')?;
                Tree::Con(Box::new(a), Box::new(b))
            }
            b'{' => {
                self.i += 1;
                self.ws();
                let mut label = 0;
                if self.peek().is_some_and(|c| c.is_ascii_digit()) {
                    let start = self.i;
                    while self.peek().is_some_and(|c| c.is_ascii_digit()) {
                        self.i += 1;
                    }
                    label = self.src[start..self.i].parse().map_err(|_| self.err("bad label".into()))?;
                    if label >= 1 << 27 {
                        return Err(self.err("label too large".into()));
                    }
                }
                let a = self.tree()?;
                let b = self.tree()?;
                self.expect(b'}')?;
                Tree::Dup(label, Box::new(a), Box::new(b))
            }
            b'$' => {
                self.i += 1;
                let start = self.i;
                while self.peek().is_some_and(|c| c != b'(' && !c.is_ascii_whitespace()) {
                    self.i += 1;
                }
                let name = &self.src[start..self.i];
                let op = Op::parse(name).ok_or_else(|| self.err(format!("unknown operator `${name}`")))?;
                self.expect(b'(')?;
                let a = self.tree()?;
                let b = self.tree()?;
                self.expect(b')')?;
                Tree::Opr(op, Box::new(a), Box::new(b))
            }
            b'?' => {
                self.i += 1;
                self.expect(b'(')?;
                let a = self.tree()?;
                let b = self.tree()?;
                self.expect(b')')?;
                Tree::Swi(Box::new(a), Box::new(b))
            }
            c if c.is_ascii_alphabetic() || c == b'_' => Tree::Var(self.name()?),
            other => return Err(self.err(format!("unexpected `{}`", other as char))),
        })
    }
}

// ---- encoding to the runtime book format ------------------------------------

// tags (runtime/mimic_net.c)
const T_VAR: u64 = 0;
const T_REF: u64 = 1;
const T_ERA: u64 = 2;
const T_NUM: u64 = 3;
const T_CON: u64 = 4;
const T_DUP: u64 = 5;
const T_OPR: u64 = 6;
const T_SWI: u64 = 7;
const T_LIT: u64 = 9;

// dtypes (runtime/mimic_rt.h)
const MIM_I64: u64 = 0;
const MIM_F64: u64 = 1;

const INLINE_MIN: i64 = -(1 << 57);
const INLINE_MAX: i64 = (1 << 57) - 1;

fn mk(tag: u64, pay: u64) -> u64 {
    (pay << 4) | tag
}
fn mkn(tag: u64, loc: u32, aux: u32) -> u64 {
    mk(tag, ((aux as u64) << 32) | loc as u64)
}

/// The book as flat words, ready to hand to `mim_net_book`.
#[derive(Debug, Clone)]
pub struct Encoded {
    pub words: Vec<u64>,
    pub offsets: Vec<u64>,
    pub names: Vec<String>,
}

impl Encoded {
    pub fn id(&self, name: &str) -> Option<u32> {
        self.names.iter().position(|n| n == name).map(|i| i as u32)
    }
}

pub fn encode(book: &Book) -> Result<Encoded, String> {
    let ids: HashMap<&str, u32> = book.defs.iter().enumerate().map(|(i, d)| (d.name.as_str(), i as u32)).collect();
    let mut words = Vec::new();
    let mut offsets = Vec::new();
    for d in &book.defs {
        offsets.push(words.len() as u64);
        let mut e = DefEnc { ids: &ids, def: &d.name, nodes: Vec::new(), vars: HashMap::new(), uses: HashMap::new(), lits: Vec::new() };
        let root = e.tree(&d.root)?;
        let mut redexes = Vec::new();
        for (a, b) in &d.redexes {
            let a = e.tree(a)?;
            let b = e.tree(b)?;
            redexes.push((a, b));
        }
        for (v, n) in &e.uses {
            if *n != 2 {
                return Err(format!("@{}: wire `{v}` is used {n} time(s); every wire must connect exactly two ports", d.name));
            }
        }
        words.extend([e.nodes.len() as u64, e.vars.len() as u64, redexes.len() as u64, e.lits.len() as u64 / 2, root]);
        for (a, b) in &e.nodes {
            words.extend([*a, *b]);
        }
        for (a, b) in &redexes {
            words.extend([*a, *b]);
        }
        words.extend(&e.lits);
    }
    Ok(Encoded { words, offsets, names: book.defs.iter().map(|d| d.name.clone()).collect() })
}

struct DefEnc<'a> {
    ids: &'a HashMap<&'a str, u32>,
    def: &'a str,
    nodes: Vec<(u64, u64)>,
    vars: HashMap<String, u32>,
    uses: HashMap<String, u32>,
    lits: Vec<u64>,
}

impl DefEnc<'_> {
    fn node(&mut self, tag: u64, aux: u32, a: &Tree, b: &Tree) -> Result<u64, String> {
        let loc = self.nodes.len() as u32;
        self.nodes.push((0, 0));
        let pa = self.tree(a)?;
        let pb = self.tree(b)?;
        self.nodes[loc as usize] = (pa, pb);
        Ok(mkn(tag, loc, aux))
    }

    fn tree(&mut self, t: &Tree) -> Result<u64, String> {
        Ok(match t {
            Tree::Era => mk(T_ERA, 0),
            Tree::Var(v) => {
                let n = self.vars.len() as u32;
                let id = *self.vars.entry(v.clone()).or_insert(n);
                *self.uses.entry(v.clone()).or_insert(0) += 1;
                mk(T_VAR, id as u64)
            }
            Tree::Ref(r) => {
                let id = self.ids.get(r.as_str()).ok_or_else(|| format!("@{}: unknown definition @{r}", self.def))?;
                mk(T_REF, *id as u64)
            }
            Tree::Num(Num::Int(i)) if (INLINE_MIN..=INLINE_MAX).contains(i) => ((*i as u64) << 6) | T_NUM,
            Tree::Num(Num::Bool(b)) => ((*b as u64) << 6) | (2 << 4) | T_NUM,
            Tree::Num(n) => {
                let idx = self.lits.len() as u64 / 2;
                match n {
                    Num::Int(i) => self.lits.extend([MIM_I64, *i as u64]),
                    Num::F64(f) => self.lits.extend([MIM_F64, f.to_bits()]),
                    Num::Bool(_) => unreachable!(),
                }
                mk(T_LIT, idx)
            }
            Tree::Con(a, b) => self.node(T_CON, 0, a, b)?,
            Tree::Dup(l, a, b) => self.node(T_DUP, *l, a, b)?,
            Tree::Opr(op, a, b) => self.node(T_OPR, op.code(), a, b)?,
            Tree::Swi(a, b) => self.node(T_SWI, 0, a, b)?,
        })
    }
}

/// A standalone C program that loads the book, calls `@entry` with no
/// arguments, prints the result and runs the leak check.
pub fn emit_c_main(enc: &Encoded, entry: u32, leak_check: bool) -> String {
    let mut s = String::from(
        "#include <stdint.h>\n#include <stdio.h>\n#include <stdlib.h>\n#include <time.h>\n\
         typedef struct mim_val mim_val;\n\
         void mim_rt_init(int32_t, char **);\nint32_t mim_rt_shutdown(int32_t);\n\
         void mim_net_book(const uint64_t *, const uint64_t *, uint32_t);\n\
         mim_val *mim_net_call(uint32_t, int32_t, mim_val **);\n\
         mim_val *mim_prim1(int32_t, mim_val *);\nvoid mim_release(mim_val *);\n\
         uint64_t mim_net_interactions(void);\n",
    );
    let join = |v: &[u64]| v.iter().map(|w| format!("{w}ull")).collect::<Vec<_>>().join(",");
    s += &format!("static const uint64_t W[] = {{{}}};\n", if enc.words.is_empty() { "0".into() } else { join(&enc.words) });
    s += &format!("static const uint64_t O[] = {{{}}};\n", if enc.offsets.is_empty() { "0".into() } else { join(&enc.offsets) });
    s += &format!(
        r#"int main(int argc, char **argv) {{
    mim_rt_init(argc, argv);
    mim_net_book(W, O, {n});
    struct timespec t0, t1;
    timespec_get(&t0, TIME_UTC);
    mim_val *r = mim_net_call({entry}, 0, 0);
    timespec_get(&t1, TIME_UTC);
    mim_prim1({print}, r);
    mim_release(r);
    if (getenv("MIMIC_NET_STATS")) {{
        double s = (double)(t1.tv_sec - t0.tv_sec) + (double)(t1.tv_nsec - t0.tv_nsec) / 1e9;
        unsigned long long n = (unsigned long long)mim_net_interactions();
        fprintf(stderr, "interactions: %llu\ntime: %.3fs\nspeed: %.1f M interactions/s\n", n, s, (double)n / s / 1e6);
    }}
    return mim_rt_shutdown({leak});
}}
"#,
        n = enc.names.len(),
        print = Prim::Print as u32,
        leak = leak_check as i32,
    );
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIB: &str = "
        @fib    = ({n0 n1} r) & n0 ~ $<(#2 c) & c ~ ?((@fib_lt @fib_ge) (n1 r))
        @fib_lt = (n n)
        @fib_ge = ({a b} r)
                & a ~ $-(#1 a1) & @fib ~ (a1 ra)
                & b ~ $-(#2 b1) & @fib ~ (b1 rb)
                & ra ~ $+(rb r)
        @main   = r & @fib ~ (#20 r)
    ";

    #[test]
    fn parse_print_round_trip() {
        let b = parse(FIB).unwrap();
        assert_eq!(b.defs.len(), 4);
        let again = parse(&b.to_string()).unwrap();
        assert_eq!(b, again);
    }

    #[test]
    fn encodes() {
        let e = encode(&parse(FIB).unwrap()).unwrap();
        assert_eq!(e.id("main"), Some(3));
        // @fib: 6 nodes: CON, DUP, OPR, SWI, CON(branches), CON(args)
        let fib = &e.words[e.offsets[0] as usize..];
        assert_eq!(fib[0], 6, "nodes");
        assert_eq!(fib[1], 4, "vars: n0 n1 r c");
        assert_eq!(fib[2], 2, "redexes");
    }

    #[test]
    fn rejects_bad_wiring_and_refs() {
        assert!(encode(&parse("@main = (a a) & a ~ *").unwrap()).unwrap_err().contains("used 3"));
        assert!(encode(&parse("@main = @nope").unwrap()).unwrap_err().contains("unknown definition"));
        assert!(parse("@main = $bogus(a a)").is_err());
    }

    #[test]
    fn pre_reduction() {
        // annihilation, erasing, copying a number, constant arithmetic, a
        // switch on a known number, wires: all done; the reference stays
        let mut b = parse("@yes = #1\n@no = #0\n@main = r & (x y) ~ (#3 c) & c ~ * & y ~ * & x ~ {p q} & p ~ $+(#4 s) & q ~ * & s ~ ?((@yes @no) r)").unwrap();
        b.pre_reduce();
        assert_eq!(b.defs[2].to_string(), "@main = @yes");
        // a reference is never expanded
        let mut f = parse(FIB).unwrap();
        let before = f.defs[3].clone();
        f.pre_reduce();
        assert_eq!(f.defs[3].to_string(), "@main = r\n  & @fib ~ (#20 r)");
        let _ = before;
    }

    #[test]
    fn literals() {
        let e = encode(&parse("@main = r & #2.5 ~ $+(#999999999999999999 r)").unwrap()).unwrap();
        // two non-inline literals: the float and the large int
        assert_eq!(e.words[3], 2);
    }
}
