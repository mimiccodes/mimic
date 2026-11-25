//! Static type inference with per-call-site specialisation.
//!
//! Every function is analysed once per combination of argument types it is
//! called with (a *specialisation*), the way C++ templates and Rust generics
//! are compiled. Within a specialisation every expression gets a [`Ty`].
//! Scalars (`i64`, `f64`, `bool`) can then live in CPU registers; anything
//! that cannot be known statically is [`Ty::Dyn`] and stays a boxed runtime
//! value, so behaviour is identical to the dynamic semantics.
//!
//! Recursion is solved by fixpoint iteration: return types start at
//! [`Ty::Never`] (no information yet) and are joined upwards until stable.

use crate::{type_path, FileDiag, FnId, FnKind, ModId, Program, Resolved};
use mimic_syntax::ast::*;
use mimic_syntax::codes as C;
use mimic_syntax::glyphs::Prim;
use mimic_syntax::{Diag, Span};
use std::collections::HashMap;
use std::sync::Mutex;

/// Tuple shapes, interned so a tuple type is a small copyable id.
static TUPLES: Mutex<Vec<Vec<Ty>>> = Mutex::new(Vec::new());

pub fn tuple_id(elems: &[Ty]) -> u32 {
    let mut t = TUPLES.lock().unwrap();
    if let Some(i) = t.iter().position(|e| e == elems) {
        return i as u32;
    }
    t.push(elems.to_vec());
    (t.len() - 1) as u32
}

pub fn tuple_elems(id: u32) -> Vec<Ty> {
    TUPLES.lock().unwrap()[id as usize].clone()
}

/// The row types of lists of lists (`[[i64]]`: rows `[i64]`), interned as
/// tuples are, so `El::Rows` stays a small copyable id.
static ROWS: Mutex<Vec<Ty>> = Mutex::new(Vec::new());

pub fn rows_id(row: Ty) -> u32 {
    let mut t = ROWS.lock().unwrap();
    if let Some(i) = t.iter().position(|e| *e == row) {
        return i as u32;
    }
    t.push(row);
    (t.len() - 1) as u32
}

pub fn rows_of(id: u32) -> Ty {
    ROWS.lock().unwrap()[id as usize]
}

/// Generic instances (`Option[i64]`), interned so `Ty::Obj` stays a small
/// copyable id. A type without arguments is `Ty::Obj(type id)`; an instance is
/// `Ty::Obj(INST | index)`. Values are represented the same whatever the
/// arguments (fields are boxed handles), so instances only sharpen types.
///
/// A record instance can also carry a *shape*: the known length of each of
/// its list fields (`lens`, one per field, 0 = not known; see Ty::Arr), when
/// it was built from lists whose lengths the compiler knows. Reading such a
/// field gives a list of that length, so functions are specialised on it.
static INSTANCES: Mutex<Vec<(usize, Vec<Ty>, Vec<u8>)>> = Mutex::new(Vec::new());
const INST: u32 = 1 << 31;

/// The record/union type `tid` with type arguments `args` (`Never` = not known yet).
pub fn obj(tid: usize, args: &[Ty]) -> Ty {
    obj_shaped(tid, args, &[])
}

/// [`obj`] with the known lengths of the record's list fields.
pub fn obj_shaped(tid: usize, args: &[Ty], lens: &[u8]) -> Ty {
    let lens: &[u8] = if lens.iter().all(|n| *n == 0) { &[] } else { lens };
    if args.is_empty() && lens.is_empty() {
        return Ty::Obj(tid as u32);
    }
    let mut t = INSTANCES.lock().unwrap();
    let key = (tid, args.to_vec(), lens.to_vec());
    let i = match t.iter().position(|e| *e == key) {
        Some(i) => i,
        None => {
            t.push(key);
            t.len() - 1
        }
    };
    Ty::Obj(INST | i as u32)
}

/// The declared type behind a `Ty::Obj` id.
pub fn obj_tid(id: u32) -> usize {
    if id & INST == 0 { id as usize } else { INSTANCES.lock().unwrap()[(id & !INST) as usize].0 }
}

/// The type arguments of a `Ty::Obj` id (empty for non-generic types).
pub fn obj_args(id: u32) -> Vec<Ty> {
    if id & INST == 0 { Vec::new() } else { INSTANCES.lock().unwrap()[(id & !INST) as usize].1.clone() }
}

/// The known lengths of a record instance's list fields (empty: none known).
pub fn obj_lens(id: u32) -> Vec<u8> {
    if id & INST == 0 { Vec::new() } else { INSTANCES.lock().unwrap()[(id & !INST) as usize].2.clone() }
}

/// Lengths both shapes agree on (0 where they differ).
fn join_lens(a: &[u8], b: &[u8]) -> Vec<u8> {
    if a.is_empty() || b.is_empty() {
        return Vec::new();
    }
    a.iter().zip(b).map(|(x, y)| if x == y { *x } else { 0 }).collect()
}

/// Type-parameter bindings of an instance: `T → i64`. Unknown arguments are dynamic.
pub fn inst_env(p: &Program, id: u32) -> Vec<(String, Ty)> {
    let td = &p.types[obj_tid(id)];
    let args = obj_args(id);
    td.decl
        .params
        .iter()
        .enumerate()
        .map(|(i, n)| (n.clone(), args.get(i).copied().filter(|t| *t != Ty::Never).unwrap_or(Ty::Dyn)))
        .collect()
}

/// Type of a field declared as `ft` in a value of type `of` (an instance or `Dyn`).
pub fn field_type(p: &Program, tid: usize, of: Ty, ft: &Type) -> Ty {
    let env = match of {
        Ty::Obj(id) if obj_tid(id) == tid => inst_env(p, id),
        _ => p.types[tid].decl.params.iter().map(|n| (n.clone(), Ty::Dyn)).collect(),
    };
    let t = ann_env(p, p.types[tid].module, ft, &env).unwrap_or(Ty::Dyn);
    // a list field of a shaped record: its known length
    if let (Ty::Obj(id), Ty::Arr(el, 0)) = (of, t)
        && let TypeKind::Record(fs) = &p.types[tid].decl.kind
        && let Some(i) = fs.iter().position(|(_, f, _)| std::ptr::eq(f, ft))
    {
        let lens = obj_lens(id);
        if let Some(n) = lens.get(i).copied().filter(|n| *n > 0) {
            return Ty::Arr(el, fixed_len(el, n as usize));
        }
    }
    t
}

/// Can a value of type `b` stand where `a` is expected (and vice versa)?
/// Unknown parts (`Dyn`, `Never`) are compatible with anything.
pub fn compatible(a: Ty, b: Ty) -> bool {
    match (a, b) {
        _ if a == b => true,
        (Ty::Dyn | Ty::Never, _) | (_, Ty::Dyn | Ty::Never) => true,
        (Ty::Obj(x), Ty::Obj(y)) => {
            obj_tid(x) == obj_tid(y) && {
                let (p, q) = (obj_args(x), obj_args(y));
                p.is_empty() || q.is_empty() || p.iter().zip(&q).all(|(s, t)| compatible(*s, *t))
            }
        }
        (Ty::Arr(x, _), Ty::Arr(y, _)) => x == El::Dyn || y == El::Dyn || compatible(Ty::of_el(x), Ty::of_el(y)),
        (Ty::Tup(x), Ty::Tup(y)) => {
            let (p, q) = (tuple_elems(x), tuple_elems(y));
            p.len() == q.len() && p.iter().zip(&q).all(|(s, t)| compatible(*s, *t))
        }
        _ => false,
    }
}

/// The more precise of two compatible types (`Option[i64]` over `Option[?]`).
pub fn refine(a: Ty, got: Ty) -> Ty {
    match (a, got) {
        (Ty::Dyn | Ty::Never, g) => g,
        (Ty::Obj(x), Ty::Obj(y)) if obj_tid(x) == obj_tid(y) => {
            let (p, q) = (obj_args(x), obj_args(y));
            // a value's shape is more precise than a declaration's
            let lens = if obj_lens(y).is_empty() { obj_lens(x) } else { obj_lens(y) };
            if p.is_empty() || q.is_empty() || p.len() != q.len() {
                return obj_shaped(obj_tid(x), &p, &lens);
            }
            let r: Vec<Ty> = p.iter().zip(&q).map(|(s, t)| refine(*s, *t)).collect();
            obj_shaped(obj_tid(x), &r, &lens)
        }
        (Ty::Arr(El::Dyn, _), g @ Ty::Arr(_, _)) => g,
        _ => a,
    }
}

/// Bind type parameters by matching annotation `t` against an actual type.
/// A parameter seen with two different concrete types reports a conflict.
pub fn unify(p: &Program, module: ModId, t: &Type, got: Ty, params: &[String], env: &mut Vec<(String, Ty)>, conflict: &mut Option<String>) {
    if got == Ty::Never {
        return;
    }
    match t {
        Type::Named(n, _) if params.contains(n) => match env.iter_mut().find(|(m, _)| m == n) {
            Some((_, b)) => {
                if compatible(*b, got) {
                    *b = refine(*b, got);
                } else if conflict.is_none() {
                    *conflict = Some(format!("type parameter `{n}` is {} in one place and {} in another", show(p, *b), show(p, got)));
                }
            }
            None => env.push((n.clone(), got)),
        },
        Type::Tuple(ts, _) => {
            if let Ty::Tup(id) = got {
                for (t, g) in ts.iter().zip(tuple_elems(id)) {
                    unify(p, module, t, g, params, env, conflict);
                }
            }
        }
        Type::Array(inner, _) => {
            if let Ty::Arr(el, _) = got
                && el != El::Dyn
            {
                unify(p, module, inner, Ty::of_el(el), params, env, conflict);
            }
        }
        Type::App(n, targs, _) => {
            if let (Some(Resolved::Type(tid)), Ty::Obj(id)) = (p.lookup(module, &type_path(n)), got)
                && obj_tid(id) == tid
            {
                for (ta, ga) in targs.iter().zip(obj_args(id)) {
                    unify(p, module, ta, ga, params, env, conflict);
                }
            }
        }
        _ => {}
    }
}

/// Element type of an array.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum El {
    I64,
    F64,
    Bool,
    Char,
    /// strings (a list of strings)
    Str,
    /// records / unions (a list of records)
    Obj(u32),
    /// bytes (`[u8]`): stored one per byte; each element reads as an i64
    U8,
    /// tuples (a list of tuples)
    Tup(u32),
    /// lists of a known type (a list of lists, `[[i64]]`: each element one
    /// row, the row's type interned, see [`rows_of`])
    Rows(u32),
    Dyn,
    /// 32-bit floats and integers (`[f32]`, `[i32]`): 4 bytes each; f32
    /// arithmetic rounds to f32, i32 arithmetic wraps
    F32,
    I32,
}

/// The longest list whose length types carry (see [`Ty::Arr`]): 16, so a
/// 3×3 kernel (9) or a 4×4 matrix stays in registers.
pub const MAX_FIXED: usize = 16;

/// The known length of a list of `el` with `n` elements: lists of numbers
/// and truth values of 1..=MAX_FIXED elements; else 0 (not known).
pub fn fixed_len(el: El, n: usize) -> u8 {
    if matches!(el, El::I64 | El::F64 | El::Bool) && (1..=MAX_FIXED).contains(&n) { n as u8 } else { 0 }
}

/// A record field's known length: declared as a list of numbers, given a
/// list of known length (its elements are converted to the declared type).
fn shape_len(declared: Ty, given: Option<Ty>) -> u8 {
    match (declared, given) {
        (Ty::Arr(el @ (El::I64 | El::F64 | El::Bool), 0), Some(Ty::Arr(g, n))) if n > 0 && matches!(g, El::I64 | El::F64 | El::Bool) => fixed_len(el, n as usize),
        _ => 0,
    }
}

/// The length an elementwise operation on `a` and `b` gives, when known.
fn same_len(a: Ty, b: Ty) -> u8 {
    match (a, b) {
        (Ty::Arr(_, n), Ty::Arr(_, m)) => if n == m { n } else { 0 },
        (Ty::Arr(_, n), _) | (_, Ty::Arr(_, n)) => n,
        _ => 0,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum Ty {
    /// No information yet (fixpoint bottom) / does not return.
    #[default]
    Never,
    Unit,
    I64,
    F64,
    Bool,
    Char,
    /// Array (vectors); `Arr(El::Char, 0)` is a string. The second field is
    /// the length when it is known and small (1..=MAX_FIXED): a list of
    /// numbers whose length the compiler knows (a literal, `⍳ 5`, and what
    /// elementwise operations and `¨` make of them); 0 when it is not known.
    /// Functions are specialised on it like on any other type, so code for a
    /// list of 5 numbers can keep them in registers.
    Arr(El, u8),
    /// A record or union value of a user-declared type.
    Obj(u32),
    /// A tuple (interned element types, see [`tuple_elems`]).
    Tup(u32),
    /// Unknown statically: a boxed runtime value.
    Dyn,
    /// 32-bit float and integer scalars (for now boxed: their arithmetic is
    /// the runtime's)
    F32,
    I32,
}

impl Ty {
    pub fn is_scalar(self) -> bool {
        matches!(self, Ty::I64 | Ty::F64 | Ty::Bool | Ty::Char | Ty::F32 | Ty::I32)
    }
    /// f64 or f32
    pub fn is_float(self) -> bool {
        matches!(self, Ty::F64 | Ty::F32)
    }
    /// Scalars that live unboxed in registers.
    pub fn is_unboxed(self) -> bool {
        matches!(self, Ty::I64 | Ty::F64 | Ty::Bool)
    }
    fn el(self) -> El {
        match self {
            Ty::I64 => El::I64,
            Ty::F64 => El::F64,
            Ty::Bool => El::Bool,
            Ty::Char => El::Char,
            Ty::F32 => El::F32,
            Ty::I32 => El::I32,
            Ty::Arr(El::Char, _) => El::Str,
            Ty::Obj(t) => El::Obj(t),
            Ty::Tup(t) => El::Tup(t),
            // (a list of known element type: rows of unknown type, and text,
            // stay as they were)
            Ty::Arr(e, _) if !matches!(e, El::Char | El::Dyn) => El::Rows(rows_id(Ty::Arr(e, 0))),
            _ => El::Dyn,
        }
    }
    pub fn of_el(e: El) -> Ty {
        match e {
            El::I64 => Ty::I64,
            El::F64 => Ty::F64,
            El::Bool => Ty::Bool,
            El::Char => Ty::Char,
            El::Str => Ty::Arr(El::Char, 0),
            El::Obj(t) => Ty::Obj(t),
            El::U8 => Ty::I64,
            El::Tup(t) => Ty::Tup(t),
            El::Rows(r) => rows_of(r),
            El::Dyn => Ty::Dyn,
            El::F32 => Ty::F32,
            El::I32 => Ty::I32,
        }
    }
    pub fn join(self, o: Ty) -> Ty {
        match (self, o) {
            (Ty::Never, x) | (x, Ty::Never) => x,
            (a, b) if a == b => a,
            (Ty::Arr(x, _), Ty::Arr(y, _)) if x == y => Ty::Arr(x, 0), // lengths differ
            (Ty::Arr(_, _), Ty::Arr(_, _)) => Ty::Arr(El::Dyn, 0),
            (Ty::Obj(a), Ty::Obj(b)) if obj_tid(a) == obj_tid(b) => {
                let (x, y) = (obj_args(a), obj_args(b));
                let lens = join_lens(&obj_lens(a), &obj_lens(b));
                if x.len() != y.len() {
                    return obj_shaped(obj_tid(a), &[], &lens);
                }
                let j: Vec<Ty> = x.iter().zip(&y).map(|(p, q)| p.join(*q)).collect();
                obj_shaped(obj_tid(a), &j, &lens)
            }
            (Ty::Tup(a), Ty::Tup(b)) => {
                let (x, y) = (tuple_elems(a), tuple_elems(b));
                if x.len() != y.len() {
                    return Ty::Dyn;
                }
                let j: Vec<Ty> = x.iter().zip(&y).map(|(p, q)| p.join(*q)).collect();
                Ty::Tup(tuple_id(&j))
            }
            _ => Ty::Dyn,
        }
    }
}

impl std::fmt::Display for Ty {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let el = |e: &El| match e {
            El::I64 => "i64",
            El::F64 => "f64",
            El::Bool => "bool",
            El::Char => "char",
            El::Str => "str",
            El::Obj(_) => "record",
            El::U8 => "u8",
            El::Tup(_) => "tuple",
            El::Rows(_) => "list",
            El::Dyn => "?",
            El::F32 => "f32",
            El::I32 => "i32",
        };
        match self {
            Ty::Never => write!(f, "never"),
            Ty::Unit => write!(f, "()"),
            Ty::I64 => write!(f, "i64"),
            Ty::F64 => write!(f, "f64"),
            Ty::F32 => write!(f, "f32"),
            Ty::I32 => write!(f, "i32"),
            Ty::Bool => write!(f, "bool"),
            Ty::Char => write!(f, "char"),
            Ty::Arr(El::Char, _) => write!(f, "str"),
            Ty::Arr(El::Rows(r), _) => write!(f, "[{}]", rows_of(*r)),
            Ty::Arr(e, _) => write!(f, "[{}]", el(e)),
            Ty::Obj(_) => write!(f, "record"),
            Ty::Tup(id) => {
                let e: Vec<String> = tuple_elems(*id).iter().map(|t| t.to_string()).collect();
                write!(f, "({})", e.join(", "))
            }
            Ty::Dyn => write!(f, "dynamic"),
        }
    }
}

/// A specialisation: a function together with its argument types.
pub type Key = (FnId, Vec<Ty>);

/// Pointer identity of an AST node (the AST is immutable while compiling).
/// The expressions whose value is a function's result (their ids and
/// places): block tails, the branches of `if` and `match`, and `return`
/// statements along the way
fn results_block(b: &Block, out: &mut Vec<(usize, Span)>) {
    for st in &b.stmts {
        if let Stmt::Return(Some(e), _) = st {
            results(e, out);
        }
    }
    if let Some(t) = &b.tail {
        results(t, out);
    }
}

fn results(e: &Expr, out: &mut Vec<(usize, Span)>) {
    match &e.kind {
        ExprKind::If { then, els, .. } => {
            results_block(then, out);
            if let Some(x) = els {
                results(x, out);
            }
        }
        ExprKind::Block(b) | ExprKind::Unsafe(b) => results_block(b, out),
        ExprKind::Match { arms, .. } => arms.iter().for_each(|a| results(&a.body, out)),
        _ => out.push((id(e), e.span)),
    }
}

/// The values of `return` statements anywhere in a function's body (in a
/// loop, a branch taken as a statement), not in its lambdas: a lambda's
/// `return` is the lambda's
fn returns_anywhere(body: &Block, out: &mut Vec<(usize, Span)>) {
    let mut inside = std::collections::HashSet::new();
    body.walk(&mut |e| match &e.kind {
        ExprKind::Lambda { body, .. } => body.walk(&mut |x| {
            inside.insert(id(x));
        }),
        ExprKind::Arms { body, .. } => body.walk(&mut |x| {
            inside.insert(id(x));
        }),
        _ => {}
    });
    let scan = |b: &Block, out: &mut Vec<(usize, Span)>| {
        for st in &b.stmts {
            if let Stmt::Return(Some(e), _) = st {
                results(e, out);
            }
        }
    };
    scan(body, out);
    body.walk(&mut |e| {
        if inside.contains(&id(e)) {
            return;
        }
        match &e.kind {
            ExprKind::If { then, .. } => scan(then, out),
            ExprKind::Block(b) | ExprKind::Unsafe(b) => scan(b, out),
            _ => {}
        }
    });
}

pub fn id<T>(x: &T) -> usize {
    x as *const T as usize
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Spec {
    /// Parameter types after annotation coercion.
    pub params: Vec<Ty>,
    pub ret: Ty,
    /// Type of every expression (by [`id`]).
    pub exprs: HashMap<usize, Ty>,
    /// Type of every `let` binding (by statement [`id`]); joins over assignments.
    pub lets: HashMap<usize, Ty>,
    /// Callee specialisation of every user-function call (by call/each expression id).
    pub calls: HashMap<usize, Key>,
    /// Lambdas: (parameter type, result type) by lambda expression id.
    pub lambdas: HashMap<usize, (Vec<Ty>, Ty)>,
    /// Types of `match` pattern bindings (by pattern id).
    pub binds: HashMap<usize, Ty>,
    /// `let (a, b) = …` element types, by (statement id, position).
    pub tuple_lets: HashMap<(usize, usize), Ty>,
    /// Route-pattern capture types, by (pattern id, capture index).
    pub route_binds: HashMap<(usize, usize), Ty>,
    /// Arguments converted to their parameter's declared list type, by (call
    /// id, argument index): a list of unknown type passed for `xs: [i64]`
    /// (the callee is compiled for `[i64]`, the call converts it).
    pub converts: std::collections::HashSet<(usize, usize)>,
}

pub struct TypeInfo {
    pub specs: HashMap<Key, Spec>,
    pub errors: Vec<FileDiag>,
}

impl TypeInfo {
    pub fn spec(&self, k: &Key) -> &Spec {
        &self.specs[k]
    }
}

/// Type of an annotation, for checking and coercion.
fn ann_ty(t: &Type) -> Option<Ty> {
    Some(match t {
        Type::Named(n, _) => match n.as_str() {
            "i32" => Ty::I32,
            "f32" => Ty::F32,
            "i8" | "i16" | "i64" | "u8" | "u16" | "u32" | "u64" | "isize" | "usize" | "cptr" => Ty::I64,
            "f64" => Ty::F64,
            "bool" => Ty::Bool,
            "char" | "c_char" => Ty::Char,
            "str" | "cstr" => Ty::Arr(El::Char, 0),
            "any" => Ty::Dyn,
            _ => return None,
        },
        Type::Array(inner, _) if matches!(&**inner, Type::Named(n, _) if n == "u8") => Ty::Arr(El::U8, 0),
        Type::Array(inner, _) => match ann_ty(inner)? {
            Ty::I64 => Ty::Arr(El::I64, 0),
            Ty::F64 => Ty::Arr(El::F64, 0),
            Ty::F32 => Ty::Arr(El::F32, 0),
            Ty::I32 => Ty::Arr(El::I32, 0),
            Ty::Bool => Ty::Arr(El::Bool, 0),
            _ => return None,
        },
        Type::Ptr(..) | Type::App(..) | Type::Tuple(..) => return None,
        Type::Fn(..) | Type::Map(..) => Ty::Dyn, // function values and maps are handles
    })
}

/// An annotation's type, resolving record/union names from `module`.
pub fn ann_in(p: &Program, module: ModId, t: &Type) -> Option<Ty> {
    ann_env(p, module, t, &[])
}

/// An annotation's type with type parameters bound by `env` (`T → i64`).
pub fn ann_env(p: &Program, module: ModId, t: &Type, env: &[(String, Ty)]) -> Option<Ty> {
    if let Type::Named(n, _) = t
        && let Some((_, b)) = env.iter().find(|(m, _)| m == n)
    {
        return Some(*b);
    }
    if let Some(x) = ann_ty(t) {
        return Some(x);
    }
    match t {
        // (quiet lookups: no "did you mean" search for names that may be
        // type parameters or simply unknown here)
        Type::Named(n, _) => match p.lookup(module, &type_path(n)) {
            // a generic type named without arguments: any instance
            Some(Resolved::Type(id)) => Some(obj(id, &vec![Ty::Dyn; p.types[id].decl.params.len()])),
            _ => None,
        },
        Type::App(n, args, _) => match p.lookup(module, &type_path(n)) {
            Some(Resolved::Type(id)) => {
                let want = p.types[id].decl.params.len();
                let mut a: Vec<Ty> = args.iter().map(|x| ann_env(p, module, x, env).unwrap_or(Ty::Dyn)).collect();
                a.resize(want, Ty::Dyn);
                Some(obj(id, &a))
            }
            _ => None,
        },
        Type::Array(inner, _) => Some(Ty::Arr(ann_env(p, module, inner, env)?.el(), 0)),
        Type::Map(..) => Some(Ty::Dyn),
        Type::Tuple(ts, _) => {
            let elems: Vec<Ty> = ts.iter().map(|t| ann_env(p, module, t, env).unwrap_or(Ty::Dyn)).collect();
            Some(Ty::Tup(tuple_id(&elems)))
        }
        Type::Ptr(..) => None,
        Type::Fn(..) => Some(Ty::Dyn),
    }
}

/// Coerce a value of type `got` into a slot annotated `ann` (user types allowed).
pub fn coerce_in(p: &Program, module: ModId, ann: Option<&Type>, got: Ty) -> Result<Ty, String> {
    coerce_env(p, module, ann, got, &[])
}

/// [`coerce_in`] with type parameters bound by `env`.
pub fn coerce_env(p: &Program, module: ModId, ann: Option<&Type>, got: Ty, env: &[(String, Ty)]) -> Result<Ty, String> {
    let Some(t) = ann else { return Ok(got) };
    match ann_env(p, module, t, env) {
        Some(a) => coerce_to(p, a, got),
        None => coerce(ann, got),
    }
}

/// Coerce a value of type `got` into a slot of type `a`.
pub fn coerce_to(p: &Program, a: Ty, got: Ty) -> Result<Ty, String> {
    match a {
        Ty::Dyn => Ok(Ty::Dyn),
        Ty::Obj(_) | Ty::Tup(_) | Ty::Arr(El::Obj(_) | El::Str | El::Tup(_) | El::Rows(_), _) => match got {
            Ty::Never | Ty::Dyn => Ok(a),
            Ty::Arr(El::Dyn, _) if matches!(a, Ty::Arr(_, _)) => Ok(a),
            g if compatible(a, g) => Ok(refine(a, g)),
            g => Err(format!("expected {}, found {}", show(p, a), show(p, g))),
        },
        _ => coerce_ty(a, got),
    }
}

/// A type for messages, naming user types.
pub fn show(p: &Program, t: Ty) -> String {
    match t {
        Ty::Obj(id) => {
            let name = &p.types[obj_tid(id)].name;
            let args = obj_args(id);
            if args.is_empty() || args.iter().all(|t| *t == Ty::Dyn) {
                return name.clone();
            }
            let a: Vec<String> = args.iter().map(|t| if matches!(t, Ty::Never | Ty::Dyn) { "_".into() } else { show(p, *t) }).collect();
            format!("{name}[{}]", a.join(", "))
        }
        Ty::Arr(El::Obj(id), _) => format!("[{}]", show(p, Ty::Obj(id))),
        Ty::Arr(El::Tup(id), _) => format!("[{}]", show(p, Ty::Tup(id))),
        Ty::Arr(El::Rows(r), _) => format!("[{}]", show(p, rows_of(r))),
        Ty::Arr(El::Str, _) => "[str]".into(),
        Ty::Tup(id) => format!("({})", tuple_elems(id).iter().map(|t| show(p, *t)).collect::<Vec<_>>().join(", ")),
        other => other.to_string(),
    }
}

/// Coerce a value of type `got` into a slot annotated `ann`.
/// Scalars convert (i64→f64, bool→i64); arrays must already be compatible.
/// `Dyn` is accepted (checked at run time). Returns the slot's type.
pub fn coerce(ann: Option<&Type>, got: Ty) -> Result<Ty, String> {
    let Some(a) = ann.and_then(ann_ty) else { return Ok(got) };
    coerce_ty(a, got)
}

/// [`coerce`] for an already-resolved scalar or array type.
fn coerce_ty(a: Ty, got: Ty) -> Result<Ty, String> {
    // a list of anything (e.g. an unbound `[T]`) takes any list
    if a == Ty::Arr(El::Dyn, 0) && matches!(got, Ty::Arr(_, _) | Ty::Never | Ty::Dyn) {
        return Ok(got);
    }
    if a == Ty::Dyn {
        return Ok(Ty::Dyn); // `any` accepts every value (boxed)
    }
    let ok = matches!(
        (a, got),
        (_, Ty::Never | Ty::Dyn)
            | (Ty::I64, Ty::I64 | Ty::Bool | Ty::I32)
            | (Ty::F64, Ty::I64 | Ty::F64 | Ty::Bool | Ty::F32 | Ty::I32)
            | (Ty::F32, Ty::I64 | Ty::F64 | Ty::Bool | Ty::F32 | Ty::I32)
            | (Ty::I32, Ty::I64 | Ty::Bool | Ty::I32)
            | (Ty::Arr(El::F32 | El::I32, _), Ty::Arr(El::I64 | El::F64 | El::Bool | El::F32 | El::I32 | El::Dyn, _))
            | (Ty::Arr(El::F64, _), Ty::Arr(El::F32 | El::I32, _))
            | (Ty::Arr(El::I64, _), Ty::Arr(El::I32, _))
            | (Ty::Bool, Ty::Bool)
            | (Ty::Char, Ty::Char)
            | (Ty::Arr(El::Char, _), Ty::Arr(El::Char | El::Dyn, _))
            | (Ty::Arr(El::I64, _), Ty::Arr(El::I64 | El::Bool | El::Dyn, _))
            | (Ty::Arr(El::F64, _), Ty::Arr(El::I64 | El::F64 | El::Bool | El::Dyn, _))
            | (Ty::Arr(El::Bool, _), Ty::Arr(El::Bool | El::Dyn, _))
            | (Ty::Arr(El::U8, _), Ty::Arr(El::U8 | El::Dyn, _))
    );
    if !ok {
        return Err(format!("expected {a}, found {got}"));
    }
    Ok(match (a, got) {
        // lists of f32 or i32 (and from them): converted to the annotation
        (Ty::Arr(e @ (El::F32 | El::I32), _), g) if g != Ty::Never && g != Ty::Arr(e, 0) => Ty::Arr(e, 0),
        (Ty::Arr(El::F64 | El::I64, _), Ty::Arr(El::F32 | El::I32, _)) => a,
        (Ty::Arr(_, _), g) if g != Ty::Dyn && g != Ty::Never => g, // arrays: a check, not a conversion
        (Ty::Arr(_, _), _) => got,
        (scalar, _) => scalar,
    })
}

/// C return type of an extern function.
fn ctype_ty(t: Option<&Type>) -> Ty {
    match t {
        None => Ty::Unit,
        Some(Type::Named(n, _)) => match n.as_str() {
            "f32" | "f64" => Ty::F64,
            "bool" => Ty::Bool,
            "cstr" => Ty::Arr(El::Char, 0),
            _ => Ty::I64,
        },
        Some(_) => Ty::Dyn,
    }
}

/// Infer types for every specialisation reachable from `roots`.
pub fn infer(p: &Program, roots: &[Key]) -> TypeInfo {
    let mut inf = Infer { p, specs: HashMap::new(), order: Vec::new(), errors: Vec::new(), report: false };
    for r in roots {
        inf.ensure(r.clone());
    }
    for _ in 0..64 {
        if !inf.pass() {
            break;
        }
    }
    // final pass: only now are types stable enough to report errors
    inf.report = true;
    inf.pass();
    let mut seen = std::collections::HashSet::new();
    let errors = inf.errors.into_iter().filter(|e| seen.insert((e.file, e.diag.span.start, e.diag.msg.clone()))).collect();
    TypeInfo { specs: inf.specs, errors }
}

/// Default roots: the entry point, every fully annotated function (so type
/// errors in uncalled code are still found), and library exports.
pub fn default_roots(p: &Program, library: bool) -> Vec<Key> {
    let mut roots = Vec::new();
    if let Some(e) = p.entry() {
        roots.push((e, vec![]));
    }
    for (fid, f) in p.fns.iter().enumerate() {
        let FnKind::User(d) = &f.kind else { continue };
        // the standard library is checked where programs reach it, not as a whole
        if p.mods[f.module].package == "std" && p.root_package != "std" {
            continue;
        }
        let tys: Option<Vec<Ty>> = d.params.iter().map(|prm| prm.ty.as_ref().and_then(|t| ann_in(p, f.module, t))).collect();
        if let Some(tys) = tys
            && !tys.is_empty()
        {
            roots.push((fid, tys));
        }
        if library && f.is_pub && p.mods[f.module].package == p.root_package {
            roots.push((fid, vec![Ty::Dyn; d.params.len()]));
        }
        // tests are checked like the rest of the program
        if d.name.starts_with("test ") && p.mods[f.module].package == p.root_package {
            roots.push((fid, vec![]));
        }
    }
    roots
}

/// The program's tests (`test "…" { }`), in source order: function and
/// shown name (`name`, or `module::path › name` outside `main`).
pub fn tests(p: &Program) -> Vec<(FnId, String)> {
    let mut out = Vec::new();
    for (fid, f) in p.fns.iter().enumerate() {
        let FnKind::User(d) = &f.kind else { continue };
        if !d.name.starts_with("test ") || p.mods[f.module].package != p.root_package {
            continue;
        }
        let name = &d.name[5..];
        let path = p.mod_path(f.module);
        let shown = if path.len() <= 1 || path.last().is_some_and(|m| m == "main") { name.to_string() } else { format!("{} › {name}", path[1..].join("::")) };
        out.push((fid, shown));
    }
    out
}

struct Infer<'p> {
    p: &'p Program,
    specs: HashMap<Key, Spec>,
    order: Vec<Key>,
    errors: Vec<FileDiag>,
    report: bool,
}

impl Infer<'_> {
    fn ensure(&mut self, k: Key) {
        if !self.specs.contains_key(&k) {
            self.specs.insert(k.clone(), Spec { params: k.1.clone(), ret: Ty::Never, ..Default::default() });
            self.order.push(k);
        }
    }

    /// One pass over every known specialisation. Returns whether anything changed.
    fn pass(&mut self) -> bool {
        let mut changed = false;
        let mut i = 0;
        while i < self.order.len() {
            let k = self.order[i].clone();
            i += 1;
            // walk the Program's own AST: expression ids are node addresses
            let p = self.p;
            let FnKind::User(d) = &p.fns[k.0].kind else { continue };
            let prev = self.specs[&k].clone();
            let (module, file) = (self.p.fns[k.0].module, self.p.fns[k.0].file);
            let path = self.p.fn_path(k.0).join("::");
            let mut w = Walk {
                inf: self,
                module,
                file,
                out: Spec { params: Vec::new(), ret: Ty::Never, lets: prev.lets.clone(), ..Default::default() },
                scopes: vec![HashMap::new()],
                ret: Ty::Never,
                fn_vals: HashMap::new(),
            };
            // type parameters: bound from the argument types of this specialisation
            let mut env: Vec<(String, Ty)> = Vec::new();
            if !d.type_params.is_empty() {
                let mut conflict = None;
                for (prm, arg) in d.params.iter().zip(&k.1) {
                    if let Some(t) = &prm.ty {
                        unify(p, module, t, *arg, &d.type_params, &mut env, &mut conflict);
                    }
                }
                if let Some(m) = conflict {
                    w.err(Diag::new(&C::TYPE_MISMATCH, format!("in a call of `{path}`: {m}"), d.span));
                }
                for tp in &d.type_params {
                    if !env.iter().any(|(n, _)| n == tp) {
                        env.push((tp.clone(), Ty::Dyn));
                    }
                }
            }
            // parameters (annotation coercion)
            for (prm, arg) in d.params.iter().zip(&k.1) {
                let t = match coerce_env(p, module, prm.ty.as_ref(), *arg, &env) {
                    Ok(t) => t,
                    Err(m) => {
                        w.err(Diag::new(&C::TYPE_MISMATCH, format!("argument `{}` of `{path}`: {m}", prm.name), prm.span));
                        Ty::Dyn
                    }
                };
                w.out.params.push(t);
                w.scopes[0].insert(prm.name.clone(), (id(prm), t));
            }
            let body = w.block(&d.body);
            let mut ret = body.join(w.ret);
            // a function named where the declared result is a value: `run`
            // where `now` was meant, in `-> R { if k == 0 { run } else { now } }`.
            // A function value is not typed yet (it is dynamic), so the check
            // below would let it through and the program fail when it runs
            if let Some(rt) = &d.ret
                && !matches!(rt, Type::Fn(..))
                && !matches!(rt, Type::Named(n, _) if d.type_params.contains(n))
                && matches!(ann_env(p, module, rt, &env), Some(t) if t != Ty::Dyn)
            {
                let mut res = Vec::new();
                results_block(&d.body, &mut res);
                returns_anywhere(&d.body, &mut res);
                res.sort_by_key(|r| r.0);
                res.dedup_by_key(|r| r.0);
                for (e, span) in res {
                    if let Some(&f) = w.fn_vals.get(&e) {
                        let name = p.fns[f].name.clone();
                        w.err(Diag::new(&C::TYPE_MISMATCH, format!("`{name}` is a function, but `{path}` returns `{rt}`: call it (`{name}(…)`) or name the value meant"), span));
                    }
                }
            }
            if let Some(rt) = &d.ret {
                match coerce_env(p, module, Some(rt), ret, &env) {
                    Ok(t) => ret = t,
                    Err(m) => w.err(Diag::new(&C::TYPE_MISMATCH, format!("return value of `{path}`: {m}"), d.span)),
                }
                // `-> [i64]` (or `[f64]`) whose body gives a list of unknown
                // type (`if … { [] } else { [x] }`): the declared list, the
                // value made typed (checked) once as it is returned, so a
                // caller's `flatten`, indexing and arithmetic on it run typed
                if let Ty::Arr(El::Dyn, _) = ret
                    && let Some(Ty::Arr(el @ (El::I64 | El::F64), _)) = ann_env(p, module, rt, &env)
                {
                    ret = Ty::Arr(el, 0);
                }
            }
            w.out.ret = ret;
            let out = std::mem::take(&mut w.out);
            if out != prev {
                changed = true;
            }
            self.specs.insert(k, out);
        }
        changed
    }
}

struct Walk<'a, 'p> {
    inf: &'a mut Infer<'p>,
    module: ModId,
    file: usize,
    out: Spec,
    /// name → (declaration id, type)
    scopes: Vec<HashMap<String, (usize, Ty)>>,
    /// join of `return` statements
    ret: Ty,
    /// expressions that name a function as a value (`run`, not `run(x)`):
    /// expression id → the function
    fn_vals: HashMap<usize, FnId>,
}

impl Walk<'_, '_> {
    fn err(&mut self, d: Diag) {
        if self.inf.report {
            self.inf.errors.push(FileDiag { file: self.file, diag: d });
        }
    }

    fn lookup(&self, name: &str) -> Option<(usize, Ty)> {
        self.scopes.iter().rev().find_map(|s| s.get(name).copied())
    }

    fn block(&mut self, b: &Block) -> Ty {
        self.scopes.push(HashMap::new());
        let mut diverged = false;
        for s in &b.stmts {
            match s {
                Stmt::Let { name, ty, value, span, .. } => {
                    let v = self.expr(value);
                    let mut t = match coerce_in(self.inf.p, self.module, ty.as_ref(), v) {
                        Ok(t) => t,
                        Err(m) => {
                            // (a lambda's typed parameter is such a `let`)
                            let what = if matches!(&value.kind, ExprKind::Path(p) if p.len() == 1 && p[0].contains('·')) { format!("parameter `{name}`") } else { format!("`let {name}`") };
                            self.err(Diag::new(&C::TYPE_MISMATCH, format!("{what}: {m}"), *span));
                            Ty::Dyn
                        }
                    };
                    // mutable bindings: join with every value assigned so far
                    let key = id(s);
                    if let Some(prev) = self.out.lets.get(&key) {
                        t = t.join(*prev);
                    }
                    self.out.lets.insert(key, t);
                    self.scopes.last_mut().unwrap().insert(name.clone(), (key, t));
                }
                Stmt::Assign { name, index, value, .. } => {
                    let v = self.expr(value);
                    if let Some(i) = index {
                        self.expr(i);
                    }
                    if let Some((key, cur)) = self.lookup(name) {
                        let nt = match index {
                            None => cur.join(v),
                            // element store: an f64 into an int array promotes it
                            Some(_) => match (cur, v) {
                                (Ty::Arr(e, _), s) if Ty::of_el(e) == s || s == Ty::Never => cur,
                                (Ty::Arr(El::I64 | El::Bool, _), Ty::F64) => Ty::Arr(El::Dyn, 0),
                                (Ty::Arr(_, _), _) => Ty::Arr(El::Dyn, 0),
                                _ => Ty::Dyn,
                            },
                        };
                        if nt != cur {
                            self.out.lets.insert(key, nt);
                            for s in self.scopes.iter_mut().rev() {
                                if let Some(e) = s.get_mut(name.as_str()) {
                                    e.1 = nt;
                                    break;
                                }
                            }
                        }
                    }
                }
                Stmt::Expr(e) => {
                    self.expr(e);
                }
                Stmt::Return(e, _) => {
                    let t = match e {
                        Some(e) => self.expr(e),
                        None => Ty::Unit,
                    };
                    self.ret = self.ret.join(t);
                    diverged = true;
                }
                Stmt::LetTuple { names, value, span } => {
                    let vt = self.expr(value);
                    let elems = match vt {
                        Ty::Tup(id) => {
                            let e = tuple_elems(id);
                            if e.len() != names.len() {
                                self.err(Diag::new(&C::TUPLE_ARITY, format!("this tuple has {} elements, but {} names are bound", e.len(), names.len()), *span));
                            }
                            e
                        }
                        Ty::Dyn | Ty::Never => vec![vt; names.len()],
                        other => {
                            self.err(Diag::new(&C::TUPLE_ARITY, format!("`let (…) =` needs a tuple, found {}", show(self.inf.p, other)), *span));
                            vec![Ty::Dyn; names.len()]
                        }
                    };
                    for (i, n) in names.iter().enumerate() {
                        let t = elems.get(i).copied().unwrap_or(Ty::Dyn);
                        self.out.tuple_lets.insert((id(s), i), t);
                        if n != "_" {
                            self.scopes.last_mut().unwrap().insert(n.clone(), (id(s) + 1 + i, t));
                        }
                    }
                }
                Stmt::LetElse { name, ty, value, els, span } => {
                    let vt = self.expr(value);
                    // the unwrapped value: the first type argument of Option[T] / Result[T, E]
                    let mut inner = Ty::Dyn;
                    if let Ty::Obj(t) = vt {
                        let tn = &self.inf.p.types[obj_tid(t)].name;
                        if tn != "Option" && tn != "Result" {
                            self.err(Diag::new(&C::LET_ELSE_TYPE, format!("`let … else` needs an Option or Result, found `{tn}`"), *span));
                        } else if let Some(a) = obj_args(t).first().copied().filter(|a| *a != Ty::Never) {
                            inner = a;
                        }
                    }
                    self.block(els);
                    let t = match coerce_in(self.inf.p, self.module, ty.as_ref(), inner) {
                        Ok(t) => t,
                        Err(m) => {
                            self.err(Diag::new(&C::TYPE_MISMATCH, format!("`let {name}`: {m}"), *span));
                            Ty::Dyn
                        }
                    };
                    self.out.lets.insert(id(s), t);
                    self.scopes.last_mut().unwrap().insert(name.clone(), (id(s), t));
                }
            }
        }
        let t = match &b.tail {
            Some(e) => self.expr(e),
            None if diverged => Ty::Never,
            None => Ty::Unit,
        };
        self.scopes.pop();
        t
    }

    /// Check argument types against the callee's annotations here, so errors
    /// point at the argument (with the parameter as a secondary label).
    /// Mismatched arguments become `Dyn` so the callee is not blamed again.
    fn check_args_at_call(&mut self, call: &Expr, path: &[String], f: FnId, args: &[Expr], argt: &mut [Ty]) {
        let p = self.inf.p;
        // a function argument must take as many arguments as the parameter's
        // function type says (a lambda, a named function or a constructor)
        let fn_params: Vec<(usize, &str, &Type)> = match &p.fns[f].kind {
            FnKind::User(d) => d.params.iter().enumerate().filter_map(|(i, prm)| prm.ty.as_ref().map(|t| (i, prm.name.as_str(), t))).collect(),
            FnKind::Extern { decl, .. } => decl.params.iter().enumerate().map(|(i, (n, t))| (i, n.as_str(), t)).collect(),
        };
        for (i, name, t) in fn_params {
            let (Type::Fn(want, ..), Some(a)) = (t, args.get(i)) else { continue };
            if !(a.span.start >= call.span.start && a.span.end <= call.span.end) {
                continue;
            }
            let module = self.module;
            let got = match &a.kind {
                ExprKind::Lambda { params, .. } => Some(params.len()),
                ExprKind::Path(path) if path.len() > 1 || self.lookup(&path[0]).is_none() => match p.lookup(module, path) {
                    Some(Resolved::Fn(g)) => Some(p.fns[g].arity()),
                    _ => None,
                },
                _ => None,
            };
            if let Some(got) = got
                && got != want.len()
            {
                let what = if matches!(&a.kind, ExprKind::Lambda { params, .. } if params.first().is_some_and(|p| p.starts_with("ctor·"))) {
                    "this constructor has"
                } else {
                    "this function takes"
                };
                self.err(Diag::new(
                    &C::TYPE_MISMATCH,
                    format!("argument `{name}` of `{}`: a function of {} argument(s) is expected, but {what} {got}", path.join("::"), want.len()),
                    a.span,
                ));
                if let Some(t) = argt.get_mut(i) {
                    *t = Ty::Dyn;
                }
            }
        }
        // the standard library's own functions: their parameters' types too
        // (`to_float(5)` is an error, not a crash when it runs)
        if let FnKind::Extern { decl, .. } = &p.fns[f].kind
            && decl.intrinsic
        {
            let module = p.fns[f].module;
            for (i, (pname, pt)) in decl.params.iter().enumerate() {
                let (Some(a), Some(t)) = (args.get(i), argt.get(i).copied()) else { break };
                let here = a.span.start >= call.span.start && a.span.end <= call.span.end;
                if !here || matches!(t, Ty::Never | Ty::Dyn) {
                    continue;
                }
                if let Err(m) = coerce_in(p, module, Some(pt), t) {
                    self.err(Diag::new(&C::TYPE_MISMATCH, format!("argument `{pname}` of `{}`: {m}", path.join("::")), a.span));
                    argt[i] = Ty::Dyn;
                }
            }
            return;
        }
        let FnKind::User(d) = &p.fns[f].kind else { return };
        let (module, file) = (p.fns[f].module, p.fns[f].file);
        for (i, prm) in d.params.iter().enumerate() {
            let (Some(a), Some(t)) = (args.get(i), argt.get(i).copied()) else { break };
            // defaults filled in by `normalize` live in the callee's file
            let here = a.span.start >= call.span.start && a.span.end <= call.span.end;
            if prm.ty.is_none() || !here || matches!(t, Ty::Never) {
                continue;
            }
            if let Err(m) = coerce_in(p, module, prm.ty.as_ref(), t) {
                let d = Diag::new(&C::TYPE_MISMATCH, format!("argument `{}` of `{}`: {m}", prm.name, path.join("::")), a.span)
                    .label_in(file, prm.span, "parameter declared here");
                self.err(d);
                argt[i] = Ty::Dyn;
            }
        }
    }

    fn call_user(&mut self, at: usize, fid: FnId, args: Vec<Ty>) -> Ty {
        if self.inf.p.fns[fid].is_extern() {
            let FnKind::Extern { decl, .. } = &self.inf.p.fns[fid].kind else { unreachable!() };
            if decl.intrinsic {
                let m = self.inf.p.fns[fid].module;
                // put(xs, i, x) and sort(xs) on a list of numbers give that
                // kind of list back, of the same length
                if self.inf.p.mods[m].name == "prelude" {
                    match (decl.name.as_str(), args.as_slice()) {
                        // not known yet (inference has not reached it): wait,
                        // so an early guess does not stick to the result
                        ("put" | "sort" | "concat", a) if a.contains(&Ty::Never) => return Ty::Never,
                        // concat of two lists of one kind of number: that
                        // kind of list (the runtime copies them as they are)
                        ("concat", [Ty::Arr(e1 @ (El::I64 | El::F64 | El::Bool | El::U8), _), Ty::Arr(e2, _)]) if e1 == e2 => return Ty::Arr(*e1, 0),
                        ("put", [a @ Ty::Arr(el @ (El::I64 | El::F64 | El::Bool), _), Ty::I64 | Ty::Bool, x])
                            if *x == Ty::of_el(*el) || (*el == El::F64 && *x == Ty::I64) =>
                        {
                            return *a;
                        }
                        ("sort", [a @ Ty::Arr(El::I64 | El::F64, _)]) => return *a,
                        _ => {}
                    }
                }
                // panic and os::exit never return: they take no part in the
                // type of a match or an if (`Ok(b) => b, Err(e) => panic(…)`
                // is the type of b, not dynamic)
                let module = &self.inf.p.mods[m].name;
                if (module == "prelude" && decl.name == "panic") || (module == "os" && decl.name == "exit") {
                    return Ty::Never;
                }
                return match &decl.ret {
                    None => Ty::Unit,
                    Some(t) => ann_in(self.inf.p, m, t).unwrap_or(Ty::Dyn),
                };
            }
            return ctype_ty(decl.ret.as_ref());
        }
        if args.contains(&Ty::Never) {
            return Ty::Never; // argument types not known yet
        }
        // a list of unknown type (`[]`, a `flatten`, a value of unknown type)
        // for a parameter declared a typed list (`xs: [i64]`): the function
        // is compiled for the declared type, and this call converts the list
        // (before, the declaration was only checked: the function ran on
        // the generic path for that call)
        let mut args = args;
        if let FnKind::User(d) = &self.inf.p.fns[fid].kind {
            let m = self.inf.p.fns[fid].module;
            for (i, prm) in d.params.iter().enumerate() {
                if i < args.len()
                    && matches!(args[i], Ty::Dyn | Ty::Arr(El::Dyn, _))
                    && let Some(t) = prm.ty.as_ref().and_then(|t| ann_in(self.inf.p, m, t))
                    && matches!(t, Ty::Arr(El::I64 | El::F64, 0))
                {
                    args[i] = t;
                    self.out.converts.insert((at, i));
                }
            }
        }
        let k: Key = (fid, args);
        self.inf.ensure(k.clone());
        self.out.calls.insert(at, k.clone());
        self.inf.specs[&k].ret
    }

    /// A lambda applied to arguments of types `args`. The enclosing scopes stay
    /// visible: captured variables keep their types (codegen unboxes them to
    /// the same types inside the closure).
    fn lambda(&mut self, e: &Expr, args: Vec<Ty>) -> Ty {
        let ExprKind::Lambda { params, body } = &e.kind else { return Ty::Dyn };
        let mut scope = HashMap::new();
        for (i, p) in params.iter().enumerate() {
            scope.insert(p.clone(), (id(e) + i, args.get(i).copied().unwrap_or(Ty::Dyn)));
        }
        self.scopes.push(scope);
        let r = self.expr(body);
        self.scopes.pop();
        let pts: Vec<Ty> = (0..params.len()).map(|i| args.get(i).copied().unwrap_or(Ty::Dyn)).collect();
        self.out.lambdas.insert(id(e), (pts, r));
        self.out.exprs.insert(id(e), r);
        r
    }

    /// `push(xs, x)` with the prelude's `push`, xs a list of i64 or f64 and
    /// x a number of the same kind: that list, of any length.
    fn push_of_numbers(&self, f: FnId, argt: &[Ty]) -> Option<Ty> {
        let p = self.inf.p;
        let m = &p.mods[p.fns[f].module];
        if p.fns[f].name != "push" || m.name != "prelude" || m.package != "std" || argt.len() != 2 {
            return None;
        }
        match (argt[0], argt[1]) {
            (Ty::Arr(El::I64, _), Ty::I64) => Some(Ty::Arr(El::I64, 0)),
            (Ty::Arr(El::F64, _), Ty::F64) => Some(Ty::Arr(El::F64, 0)),
            // the number not known yet (a call whose result a later pass
            // finds): not known yet either, rather than a list of anything,
            // which would settle a loop's state as dynamic for good
            (Ty::Arr(El::I64 | El::F64, _), Ty::Never) | (Ty::Never, _) => Some(Ty::Never),
            _ => None,
        }
    }

    /// `flatten(xs ¨ (x => …))` with the prelude's `flatten`, the lambda's
    /// result (already typed) a list of i64 or f64: that element type.
    fn flatten_of_numbers(&self, f: FnId, args: &[Expr]) -> Option<El> {
        let p = self.inf.p;
        let m = &p.mods[p.fns[f].module];
        if p.fns[f].name != "flatten" || m.name != "prelude" || m.package != "std" || args.len() != 1 {
            return None;
        }
        let ExprKind::Prim { prim: Prim::Each, args: each } = &args[0].kind else { return None };
        let lam = each.get(1).filter(|x| matches!(x.kind, ExprKind::Lambda { .. }))?;
        match self.out.lambdas.get(&id(lam))?.1 {
            Ty::Arr(el @ (El::I64 | El::F64), _) => Some(el),
            _ => None,
        }
    }

    /// `f` used as a value: its dynamic specialisation must exist.
    fn fn_value(&mut self, f: FnId) {
        if !self.inf.p.fns[f].is_extern() {
            let n = self.inf.p.fns[f].arity();
            self.inf.ensure((f, vec![Ty::Dyn; n]));
        }
    }

    /// A literal (or a negated one) given the type `t` (see literal_as).
    fn retype_literal(&mut self, e: &Expr, t: Ty) {
        self.out.exprs.insert(id(e), t);
        if let ExprKind::Unary { expr, .. } = &e.kind {
            self.out.exprs.insert(id(expr.as_ref()), t);
        }
    }

    fn expr(&mut self, e: &Expr) -> Ty {
        let t = self.expr_inner(e);
        self.out.exprs.insert(id(e), t);
        t
    }

    fn expr_inner(&mut self, e: &Expr) -> Ty {
        match &e.kind {
            ExprKind::TypeArg(_) => Ty::Dyn, // a schema handle
            ExprKind::Int(_) => Ty::I64,
            ExprKind::Float(_) => Ty::F64,
            ExprKind::Bool(_) => Ty::Bool,
            ExprKind::Str(_) => Ty::Arr(El::Char, 0),
            ExprKind::Array(xs) => {
                let mut el = Ty::Never;
                for x in xs {
                    let t = self.expr(x);
                    el = match (el, t) {
                        (Ty::Never, t) => t,
                        (a, b) if a == b => a,
                        (a, b) if a.is_scalar() && b.is_scalar() => num_join(a, false, b, false),
                        _ => Ty::Dyn,
                    };
                }
                match el {
                    // `[]` has no element type of its own: it fits any list
                    Ty::Never if xs.is_empty() => Ty::Arr(El::Dyn, 0),
                    Ty::Never => Ty::Never,
                    t => Ty::Arr(t.el(), fixed_len(t.el(), xs.len())), // numbers, strings and records; anything else is dynamic
                }
            }
            ExprKind::Path(path) => match self.lookup(&path[0]) {
                Some((_, t)) if path.len() == 1 => t,
                _ => match self.inf.p.lookup(self.module, path) {
                    // `None`: the type arguments are not known yet
                    Some(Resolved::Variant(t, _)) => obj(t, &vec![Ty::Never; self.inf.p.types[t].decl.params.len()]),
                    Some(Resolved::Fn(f)) => {
                        self.fn_value(f);
                        self.fn_vals.insert(id(e), f);
                        Ty::Dyn
                    }
                    _ => Ty::Dyn,
                },
            },
            ExprKind::OpRef(_) => Ty::Dyn,
            ExprKind::Arms { .. } => Ty::Dyn, // lifted after parsing
            ExprKind::Map(kvs) => {
                for (k, v) in kvs {
                    self.expr(k);
                    self.expr(v);
                }
                Ty::Dyn
            }
            ExprKind::Call { callee, args } => {
                let mut argt: Vec<Ty> = args.iter().map(|a| self.expr(a)).collect();
                match &callee.kind {
                    ExprKind::Lambda { .. } => self.lambda(callee, argt.clone()),
                    // a function value in a variable: its result is dynamic
                    ExprKind::Path(path) if path.len() == 1 && self.lookup(&path[0]).is_some() => Ty::Dyn,
                    ExprKind::Path(path) => match self.inf.p.resolve(self.module, path) {
                        Ok(Resolved::Fn(f)) => {
                            // an intrinsic with type parameters: `json::decode[User](…)`
                            if let FnKind::Extern { decl, .. } = &self.inf.p.fns[f].kind
                                && !decl.type_params.is_empty()
                            {
                                let p = self.inf.p;
                                let m = p.fns[f].module;
                                let explicit: Vec<Ty> = args
                                    .iter()
                                    .filter_map(|a| match &a.kind {
                                        ExprKind::TypeArg(t) => Some(ann_in(p, self.module, t).unwrap_or(Ty::Dyn)),
                                        _ => None,
                                    })
                                    .collect();
                                let mut env: Vec<(String, Ty)> = Vec::new();
                                if explicit.is_empty() {
                                    // `chan::send(tx, v)`: bound from the arguments, like any generic call
                                    let mut conflict = None;
                                    let vals = args.iter().zip(&argt).filter(|(a, _)| !matches!(a.kind, ExprKind::TypeArg(_)));
                                    for ((_, pt), (_, at)) in decl.params.iter().zip(vals) {
                                        unify(p, m, pt, *at, &decl.type_params, &mut env, &mut conflict);
                                    }
                                    if let Some(msg) = conflict {
                                        self.err(Diag::new(&C::TYPE_MISMATCH, format!("in a call of `{}`: {msg}", path.join("::")), e.span));
                                    }
                                } else {
                                    env = decl.type_params.iter().cloned().zip(explicit).collect();
                                }
                                // the arguments must fit the parameters (with T bound)
                                let vals: Vec<(&Expr, Ty)> =
                                    args.iter().zip(argt.iter().copied()).filter(|(a, _)| !matches!(a.kind, ExprKind::TypeArg(_))).collect();
                                for ((pname, pt), (a, at)) in decl.params.iter().zip(vals) {
                                    if let Err(msg) = coerce_env(p, m, Some(pt), at, &env) {
                                        self.err(Diag::new(&C::TYPE_MISMATCH, format!("argument `{pname}` of `{}`: {msg}", path.join("::")), a.span));
                                    }
                                }
                                return match &decl.ret {
                                    Some(rt) => ann_env(p, m, rt, &env).unwrap_or(Ty::Dyn),
                                    None => Ty::Unit,
                                };
                            }
                            self.check_args_at_call(e, path, f, args, &mut argt);
                            // the prelude's `push(xs, x)`: a number onto a list of
                            // that kind of number gives such a list (the runtime
                            // keeps it typed), so a list grown in a loop stays typed
                            let pushed = self.push_of_numbers(f, &argt);
                            // the prelude's `flatten` of a list of lists: the one
                            // copy for lists of unknown type, as before lists of
                            // lists had a type. Rows of numbers go to the runtime's
                            // concat either way and the result is typed below, so a
                            // copy for `[[i64]]` only adds code: inlined into a
                            // parallel `¨` (the benchmark `diff`) it measured 6%
                            // slower on Windows on all cores, running the same
                            // instructions
                            let p = self.inf.p;
                            let fm = &p.mods[p.fns[f].module];
                            if p.fns[f].name == "flatten" && fm.name == "prelude" && fm.package == "std" {
                                for t in argt.iter_mut() {
                                    if matches!(t, Ty::Arr(El::Rows(_), _)) {
                                        *t = Ty::Arr(El::Dyn, 0);
                                    }
                                }
                            }
                            let r = self.call_user(id(e), f, argt);
                            if let Some(t) = pushed {
                                return t;
                            }
                            // the prelude's `flatten(xs ¨ (x => …))` whose lambda gives a
                            // list of numbers: a list of those numbers (the result is
                            // checked and made typed once, here; everything after it
                            // then runs typed instead of on values of unknown type)
                            if let Some(el) = self.flatten_of_numbers(f, args)
                                && matches!(r, Ty::Dyn | Ty::Arr(El::Dyn, _))
                            {
                                return Ty::Arr(el, 0);
                            }
                            r
                        }
                        Ok(Resolved::Variant(t, v)) => {
                            let p = self.inf.p;
                            let td = &p.types[t];
                            let (fields, tm, vname) = (&td.variants()[v].fields, td.module, &td.variants()[v].name);
                            // `Some(42)`: bind the type parameters from the field values
                            let mut env = Vec::new();
                            let mut conflict = None;
                            for (ft, at) in fields.iter().zip(&argt) {
                                unify(p, tm, ft, *at, &td.decl.params, &mut env, &mut conflict);
                            }
                            if let Some(m) = conflict {
                                self.err(Diag::new(&C::TYPE_MISMATCH, format!("`{vname}`: {m}"), e.span));
                            }
                            for (i, (ft, at)) in fields.iter().zip(&argt).enumerate() {
                                if let Err(m) = coerce_env(p, tm, Some(ft), *at, &env) {
                                    self.err(Diag::new(&C::TYPE_MISMATCH, format!("field {} of `{vname}`: {m}", i + 1), args[i].span));
                                }
                            }
                            let targs: Vec<Ty> =
                                td.decl.params.iter().map(|n| env.iter().find(|(m, _)| m == n).map(|(_, t)| *t).unwrap_or(Ty::Never)).collect();
                            obj(t, &targs)
                        }
                        _ => Ty::Dyn,
                    },
                    // any other callee produces a function value: `make_adder(1)(2)`
                    _ => {
                        self.expr(callee);
                        Ty::Dyn
                    }
                }
            }
            ExprKind::Index { base, index } => {
                let b = self.expr(base);
                self.expr(index);
                if matches!(index.kind, ExprKind::Tuple(_)) {
                    return if b == Ty::Never { Ty::Never } else { Ty::Dyn }; // m[i, j]
                }
                match b {
                    Ty::Arr(el, _) => Ty::of_el(el),
                    Ty::Never => Ty::Never,
                    _ => Ty::Dyn,
                }
            }
            ExprKind::Unary { op, expr } => {
                let t = self.expr(expr);
                if *op == UnOp::BitNot
                    && matches!(t, Ty::F64 | Ty::F32 | Ty::Char | Ty::Arr(El::F64 | El::F32 | El::Char | El::Str, _))
                {
                    self.err(Diag::new(&C::NO_OPERATOR, "`~` needs an integer (or a bool)", expr.span));
                    return Ty::Dyn;
                }
                let scalar = |s: Ty| match (op, s) {
                    (UnOp::Not, _) => Ty::Bool,
                    (UnOp::BitNot, Ty::Bool) => Ty::Bool,
                    (UnOp::BitNot, Ty::Dyn) => Ty::Dyn,
                    (UnOp::BitNot, Ty::I32) => Ty::I32,
                    (UnOp::BitNot, _) => Ty::I64,
                    (_, Ty::F64) => Ty::F64,
                    (_, s @ (Ty::F32 | Ty::I32)) => s,
                    (_, Ty::Dyn) => Ty::Dyn,
                    _ => Ty::I64,
                };
                match t {
                    Ty::Never => Ty::Never,
                    Ty::Arr(el, n) => Ty::Arr(scalar(Ty::of_el(el)).el(), n),
                    s if s.is_scalar() => scalar(s),
                    _ => Ty::Dyn,
                }
            }
            ExprKind::Binary { op, lhs, rhs } => {
                let mut a = self.expr(lhs);
                let mut b = self.expr(rhs);
                let elem = |t: Ty| match t {
                    Ty::Arr(x, _) => Ty::of_el(x),
                    t => t,
                };
                // a number written in the program takes the 32-bit type of
                // the other side: `z * 0.5` on an f32 is f32, `n + 1` on an
                // i32 is i32 (as Go's constants)
                if let Some(t) = literal_as(lhs, elem(b)) {
                    a = t;
                    self.retype_literal(lhs, t);
                } else if let Some(t) = literal_as(rhs, elem(a)) {
                    b = t;
                    self.retype_literal(rhs, t);
                }
                let whole = |t: Ty| matches!(elem(t), Ty::Obj(_) | Ty::Tup(_));
                if matches!(op, BinOp::Eq | BinOp::Ne) && (whole(a) || whole(b)) {
                    // structural equality: same type, compared field by field
                    // (elementwise when either side is a list)
                    let (x, y) = (elem(a), elem(b));
                    let comparable = compatible(x, y);
                    if !comparable {
                        let p = self.inf.p;
                        self.err(
                            Diag::new(&C::TYPE_MISMATCH, format!("comparing a {} with a {} is always false", show(p, a), show(p, b)), e.span)
                                .help("`==` compares values of the same type, field by field"),
                        );
                    }
                    return if a == Ty::Never || b == Ty::Never {
                        Ty::Never
                    } else if matches!(a, Ty::Arr(_, _)) || matches!(b, Ty::Arr(_, _)) {
                        Ty::Arr(El::Bool, same_len(a, b))
                    } else {
                        Ty::Bool
                    };
                }
                if matches!(a, Ty::Obj(_)) || matches!(b, Ty::Obj(_)) {
                    self.err(Diag::new(&C::NO_OPERATOR, format!("operator `{}` is not defined for records and variants (use `match`)", op.symbol()), e.span));
                    return Ty::Dyn;
                }
                if op.is_bitwise() {
                    let el = |t: Ty| match t {
                        Ty::Arr(x, _) => Ty::of_el(x),
                        t => t,
                    };
                    for (t, side) in [(a, lhs), (b, rhs)] {
                        let t = el(t);
                        if matches!(t, Ty::F64 | Ty::Char | Ty::Arr(_, _)) || (t == Ty::Bool && op.is_shift()) {
                            let what = match t {
                                Ty::F64 => "a float",
                                Ty::Bool => "a bool",
                                _ => "text",
                            };
                            self.err(
                                Diag::new(&C::NO_OPERATOR, format!("`{}` needs integers, but this is {what}", op.symbol()), side.span)
                                    .help("bitwise operators work on integers (and `& | ^` also on bools)"),
                            );
                            return Ty::Dyn;
                        }
                    }
                }
                binary_ty(*op, a, b)
            }
            ExprKind::Tuple(xs) => {
                let ts: Vec<Ty> = xs.iter().map(|x| self.expr(x)).collect();
                if ts.contains(&Ty::Never) {
                    return Ty::Never;
                }
                Ty::Tup(tuple_id(&ts))
            }
            ExprKind::Interp(parts) => {
                for part in parts {
                    if let InterpPart::Expr(x) = part {
                        self.expr(x);
                    }
                }
                Ty::Arr(El::Char, 0)
            }
            ExprKind::Named { value, .. } => {
                self.expr(value);
                Ty::Dyn
            }
            ExprKind::Record { path, fields } => {
                let p = self.inf.p;
                let Some(Resolved::Type(t)) = p.lookup(self.module, path) else { return Ty::Dyn };
                let td = &p.types[t];
                let TypeKind::Record(decl) = &td.decl.kind else { return Ty::Dyn };
                let vts: Vec<Ty> = fields.iter().map(|(_, v, _)| self.expr(v)).collect();
                // a field not known yet: the record is not either (an early
                // guess at its shape would stick, types only grow)
                if vts.contains(&Ty::Never) {
                    return Ty::Never;
                }
                let mut env = Vec::new();
                let mut conflict = None;
                for ((n, _, _), vt) in fields.iter().zip(&vts) {
                    if let Some((_, ft, _)) = decl.iter().find(|(d, _, _)| d == n) {
                        unify(p, td.module, ft, *vt, &td.decl.params, &mut env, &mut conflict);
                    }
                }
                if let Some(m) = conflict {
                    self.err(Diag::new(&C::TYPE_MISMATCH, format!("`{}`: {m}", td.name), e.span));
                }
                for ((n, _, sp), vt) in fields.iter().zip(&vts) {
                    if let Some((_, ft, _)) = decl.iter().find(|(d, _, _)| d == n)
                        && let Err(m) = coerce_env(p, td.module, Some(ft), *vt, &env)
                    {
                        self.err(Diag::new(&C::TYPE_MISMATCH, format!("field `{n}` of `{}`: {m}", td.name), *sp));
                    }
                }
                let targs: Vec<Ty> =
                    td.decl.params.iter().map(|n| env.iter().find(|(m, _)| m == n).map(|(_, t)| *t).unwrap_or(Ty::Never)).collect();
                // the shape: list fields given lists of known length keep it
                let lens: Vec<u8> = decl
                    .iter()
                    .map(|(n, ft, _)| {
                        let vt = fields.iter().zip(&vts).find(|((m, _, _), _)| m == n).map(|(_, vt)| *vt);
                        shape_len(ann_env(p, td.module, ft, &env).unwrap_or(Ty::Dyn), vt)
                    })
                    .collect();
                obj_shaped(t, &targs, &lens)
            }
            ExprKind::Field { base, name } => {
                let b = self.expr(base);
                self.field_ty(b, name, e.span)
            }
            ExprKind::With { base, fields } => {
                let b = self.expr(base);
                for (n, v, sp) in fields {
                    let vt = self.expr(v);
                    if let Ty::Obj(t) = b {
                        let p = self.inf.p;
                        let tid = obj_tid(t);
                        let td = &p.types[tid];
                        match &td.decl.kind {
                            TypeKind::Record(decl) => match decl.iter().find(|(d, _, _)| d == n) {
                                Some((_, ft, _)) => {
                                    if let Err(m) = coerce_to(p, field_type(p, tid, b, ft), vt) {
                                        self.err(Diag::new(&C::TYPE_MISMATCH, format!("field `{n}` of `{}`: {m}", td.name), *sp));
                                    }
                                }
                                None => self.err(Diag::new(&C::NO_FIELD, format!("`{}` has no field `{n}`", td.name), *sp)),
                            },
                            TypeKind::Union(_) => self.err(Diag::new(&C::UNION_NOT_RECORD, format!("`{}` is a union; `with` works on records", td.name), e.span)),
                        }
                    }
                }
                // the updated fields' new lengths
                if let Ty::Obj(oid) = b
                    && let TypeKind::Record(decl) = &self.inf.p.types[obj_tid(oid)].decl.kind
                {
                    let p = self.inf.p;
                    let tid = obj_tid(oid);
                    let mut lens = obj_lens(oid);
                    lens.resize(decl.len(), 0);
                    for (n, v, _) in fields {
                        if let Some(i) = decl.iter().position(|(d, _, _)| d == n) {
                            let vt = self.out.exprs.get(&id(v)).copied();
                            let declared = ann_env(p, p.types[tid].module, &decl[i].1, &inst_env(p, oid)).unwrap_or(Ty::Dyn);
                            lens[i] = shape_len(declared, vt);
                        }
                    }
                    return obj_shaped(tid, &obj_args(oid), &lens);
                }
                match b {
                    Ty::Obj(_) | Ty::Never => b,
                    _ => {
                        self.err(Diag::new(&C::TYPE_NOT_KNOWN, "`with` needs a value whose record type is known here", base.span)
                            .note("annotate the parameter or binding, e.g. `u: User`"));
                        Ty::Dyn
                    }
                }
            }
            ExprKind::Match { scrut, arms } => {
                let st = self.expr(scrut);
                let mut out = Ty::Never;
                for arm in arms {
                    self.scopes.push(HashMap::new());
                    self.bind_pattern(&arm.pat, st);
                    if let Some(g) = &arm.guard {
                        self.expr(g);
                    }
                    out = out.join(self.expr(&arm.body));
                    self.scopes.pop();
                }
                out
            }
            ExprKind::Prim { prim, args } => {
                let a = self.expr(&args[0]);
                if *prim == Prim::Each {
                    let el = match a {
                        Ty::Arr(el, _) => Ty::of_el(el),
                        Ty::Never => return Ty::Never,
                        s => s,
                    };
                    let r = match &args[1].kind {
                        ExprKind::Lambda { .. } => self.lambda(&args[1], vec![el]),
                        ExprKind::Path(path) if path.len() == 1 && self.lookup(&path[0]).is_some() => Ty::Dyn,
                        ExprKind::Path(path) => match self.inf.p.lookup(self.module, path) {
                            Some(Resolved::Fn(f)) => self.call_user(id(e), f, vec![el]),
                            _ => Ty::Dyn,
                        },
                        _ => {
                            self.expr(&args[1]);
                            Ty::Dyn
                        }
                    };
                    return match (a, r) {
                        (_, Ty::Never) => Ty::Never,
                        (Ty::Arr(_, n), r) => Ty::Arr(r.el(), fixed_len(r.el(), n as usize)),
                        (Ty::Dyn, _) => Ty::Dyn, // a list or a scalar: not known here
                        (_, r) => r,             // `¨` on a scalar applies f once
                    };
                }
                // xs ▷ trail(+) / gather(+) over numbers (+ × max min): the
                // running results are numbers of the same kind, and the
                // whole gathered is one (the runtime scans such lists typed)
                if matches!(prim, Prim::Trail | Prim::Gather)
                    && let Ty::Arr(el @ (El::I64 | El::F64), _) = a
                    && matches!(&args[1].kind, ExprKind::OpRef(BinOp::Add | BinOp::Mul | BinOp::Max | BinOp::Min))
                {
                    return if *prim == Prim::Trail { Ty::Arr(el, 0) } else { Ty::of_el(el) };
                }
                // xs ▷ trail((a, b) => …) / gather(…) over numbers, with a
                // lambda that gives back the same kind of number: typed as
                // (number, number) → number, so its body compiles typed
                // (code generation folds it in a loop, nothing boxed)
                if matches!(prim, Prim::Trail | Prim::Gather)
                    && let Ty::Arr(el @ (El::I64 | El::F64), _) = a
                    && matches!(&args[1].kind, ExprKind::Lambda { params, .. } if params.len() == 2)
                {
                    let t = Ty::of_el(el);
                    if self.lambda(&args[1], vec![t, t]) == t {
                        // (the lambda itself: a function value)
                        self.out.exprs.insert(id(&args[1]), Ty::Dyn);
                        return if *prim == Prim::Trail { Ty::Arr(el, 0) } else { t };
                    }
                }
                // x ▷ evolve(n, step) / x ▷ settle(step, done): the step
                // function takes the state; when it gives back the same type,
                // the state keeps that type throughout (else it is dynamic)
                if matches!(prim, Prim::Evolve | Prim::Settle) && a != Ty::Never && a != Ty::Dyn {
                    let (step, done) = if *prim == Prim::Evolve {
                        self.expr(&args[1]);
                        (&args[2], None)
                    } else {
                        (&args[1], Some(&args[2]))
                    };
                    let ExprKind::Lambda { params, .. } = &step.kind else {
                        args.iter().skip(1).for_each(|x| {
                            self.expr(x);
                        });
                        return Ty::Dyn;
                    };
                    if params.len() != 1 {
                        args.iter().skip(1).for_each(|x| {
                            self.expr(x);
                        });
                        return Ty::Dyn;
                    }
                    let r = self.lambda(step, vec![a]);
                    // the same record, its list fields' known lengths aside
                    // (a start built from `[x]` knows one): the record type
                    // without them is the state throughout
                    let same_record = matches!((a, r), (Ty::Obj(x), Ty::Obj(y)) if x != y && obj_tid(x) == obj_tid(y) && obj_args(x) == obj_args(y));
                    // the same kind of list, its length changing (a message
                    // schedule grown by `push` from 16 words): a list of that
                    // kind of any length is the state throughout
                    let same_list = matches!((a, r), (Ty::Arr(x, n), Ty::Arr(y, m)) if x == y && n != m && !matches!(x, El::Dyn));
                    let (a, r) = if same_record {
                        let Ty::Obj(x) = a else { unreachable!() };
                        let plain = obj(obj_tid(x), &obj_args(x));
                        (plain, self.lambda(step, vec![plain]))
                    } else if same_list {
                        let Ty::Arr(x, _) = a else { unreachable!() };
                        let plain = Ty::Arr(x, 0);
                        (plain, self.lambda(step, vec![plain]))
                    } else {
                        (a, r)
                    };
                    let st = if r == a {
                        a
                    } else {
                        if r != Ty::Never {
                            self.lambda(step, vec![Ty::Dyn]);
                        }
                        Ty::Dyn
                    };
                    if let Some(d) = done {
                        match &d.kind {
                            ExprKind::Lambda { params, .. } if params.len() == 1 => {
                                self.lambda(d, vec![st]);
                                self.out.exprs.insert(id(d), Ty::Dyn);
                            }
                            _ => {
                                self.expr(d);
                            }
                        }
                    }
                    // the lambdas themselves are function values
                    self.out.exprs.insert(id(step), Ty::Dyn);
                    return if r == Ty::Never { Ty::Never } else { st };
                }
                let b = if args.len() > 1 { self.expr(&args[1]) } else { Ty::Unit };
                for extra in args.iter().skip(2) {
                    self.expr(extra);
                }
                if a == Ty::Never || b == Ty::Never {
                    return Ty::Never;
                }
                // ⍳ 5: five numbers, a length the compiler knows
                if *prim == Prim::Iota && args.len() == 1
                    && let ExprKind::Int(k) = args[0].kind {
                        return Ty::Arr(El::I64, fixed_len(El::I64, k.max(0) as usize));
                    }
                prim_ty(*prim, a, b)
            }
            // a lambda as a value: its parameters can be anything
            ExprKind::Lambda { params, .. } => {
                let n = params.len();
                self.lambda(e, vec![Ty::Dyn; n]);
                Ty::Dyn
            }
            ExprKind::If { cond, then, els } => {
                self.expr(cond);
                let mut t = self.block(then);
                match els {
                    Some(x) => {
                        let mut u = self.expr(x);
                        // a branch that is a number written in the program
                        // takes the other's f32 or i32: `if c { 0 } else { n }`
                        if let Some(lit) = then.tail.as_deref().filter(|l| is_literal(l))
                            && let Some(nt) = literal_branch_as(lit, u)
                        {
                            self.retype_literal(lit, nt);
                            t = nt;
                        } else if let Some(lit) = tail_literal(x)
                            && let Some(nt) = literal_branch_as(lit, t)
                        {
                            self.retype_literal(lit, nt);
                            self.out.exprs.insert(id(x.as_ref()), nt);
                            u = nt;
                        }
                        t.join(u)
                    }
                    None => t.join(Ty::Unit),
                }
            }
            ExprKind::Block(b) | ExprKind::Unsafe(b) => self.block(b),
        }
    }
}

impl Walk<'_, '_> {
    /// The type of `base.name`.
    fn field_ty(&mut self, base: Ty, name: &str, span: mimic_syntax::Span) -> Ty {
        let p = self.inf.p;
        match base {
            Ty::Never => Ty::Never,
            Ty::Tup(id) => {
                let e = tuple_elems(id);
                match name.parse::<usize>() {
                    Ok(i) if i < e.len() => e[i],
                    _ => {
                        self.err(Diag::new(&C::NO_FIELD, format!("this tuple has {} elements; `.{name}` is not one of them", e.len()), span));
                        Ty::Dyn
                    }
                }
            }
            Ty::Dyn => Ty::Dyn, // looked up by name at run time
            Ty::Obj(t) => {
                let tid = obj_tid(t);
                let td = &p.types[tid];
                match &td.decl.kind {
                    TypeKind::Record(decl) => match decl.iter().find(|(d, _, _)| d == name) {
                        Some((_, ft, _)) => field_type(p, tid, base, ft),
                        None => {
                            self.err(Diag::new(&C::NO_FIELD, format!("`{}` has no field `{name}`", td.name), span));
                            Ty::Dyn
                        }
                    },
                    TypeKind::Union(_) => {
                        self.err(Diag::new(&C::UNION_NOT_RECORD, format!("`{}` is a union: use `match` to get at its fields", td.name), span));
                        Ty::Dyn
                    }
                }
            }
            other => {
                self.err(Diag::new(&C::NO_FIELD, format!("`.{name}`: a {} has no fields", show(p, other)), span));
                Ty::Dyn
            }
        }
    }

    /// Bind the variables of a pattern matched against a value of type `t`.
    fn bind_pattern(&mut self, pat: &Pattern, t: Ty) {
        match pat {
            Pattern::Wild(_) => {}
            Pattern::Bind(n, _) => {
                self.out.binds.insert(id(pat), t);
                self.scopes.last_mut().unwrap().insert(n.clone(), (id(pat), t));
            }
            Pattern::Lit(e) => {
                let lt = self.expr(e);
                if t.is_scalar() && lt.is_scalar() && (t == Ty::Bool) != (lt == Ty::Bool) {
                    self.err(Diag::new(&C::TYPE_MISMATCH, format!("this pattern is a {lt}, but the value is a {t}"), e.span));
                }
            }
            Pattern::Tuple(ps, _) => {
                let elems = match t {
                    Ty::Tup(id) => tuple_elems(id),
                    _ => vec![Ty::Dyn; ps.len()],
                };
                for (i, a) in ps.iter().enumerate() {
                    self.bind_pattern(a, elems.get(i).copied().unwrap_or(Ty::Dyn));
                }
            }
            Pattern::Route(parts, span) => {
                if !matches!(t, Ty::Arr(El::Char, _) | Ty::Dyn | Ty::Never) {
                    self.err(Diag::new(&C::TYPE_MISMATCH, format!("a route pattern matches a str, but the value is {}", show(self.inf.p, t)), *span));
                }
                let mut k = 0;
                for part in parts {
                    if let RoutePart::Capture { name, ty } = part {
                        let ct = match ty.as_deref() {
                            Some("i64") => Ty::I64,
                            Some("f64") => Ty::F64,
                            _ => Ty::Arr(El::Char, 0),
                        };
                        self.out.route_binds.insert((id(pat), k), ct);
                        self.scopes.last_mut().unwrap().insert(name.clone(), (id(pat) + 1 + k, ct));
                        k += 1;
                    }
                }
            }
            Pattern::Variant { path, args, .. } => {
                let p = self.inf.p;
                let Some(Resolved::Variant(tid, v)) = p.lookup(self.module, path) else { return };
                let td = &p.types[tid];
                if let Ty::Obj(st) = t
                    && obj_tid(st) != tid
                {
                    self.err(Diag::new(&C::TYPE_MISMATCH, format!("this pattern is a `{}`, but the value is a `{}`", td.name, show(p, t)), pat.span()));
                }
                let fts: Vec<Ty> = td.variants()[v].fields.iter().map(|f| field_type(p, tid, t, f)).collect();
                for (a, ft) in args.iter().zip(fts) {
                    self.bind_pattern(a, ft);
                }
            }
            Pattern::Or(alts, _) => {
                for a in alts {
                    self.bind_pattern(a, t);
                }
            }
            Pattern::Range { lo, hi, span } => {
                let (a, b) = (self.expr(lo), self.expr(hi));
                for x in [a, b] {
                    if !matches!(x, Ty::I64 | Ty::F64) {
                        self.err(Diag::new(&C::TYPE_MISMATCH, format!("a range pattern needs numbers, found {}", show(self.inf.p, x)), *span));
                    }
                }
                if !matches!(t, Ty::I64 | Ty::F64 | Ty::Dyn | Ty::Never) {
                    self.err(Diag::new(&C::TYPE_MISMATCH, format!("a range pattern matches a number, but the value is {}", show(self.inf.p, t)), *span));
                }
            }
            Pattern::As(inner, n, _) => {
                self.bind_pattern(inner, t);
                self.out.binds.insert(id(pat), t);
                self.scopes.last_mut().unwrap().insert(n.clone(), (id(pat), t));
            }
            Pattern::Record { path, fields, .. } => {
                let p = self.inf.p;
                let Some(Resolved::Type(tid)) = p.lookup(self.module, path) else { return };
                let td = &p.types[tid];
                if let Ty::Obj(st) = t
                    && obj_tid(st) != tid
                {
                    self.err(Diag::new(&C::TYPE_MISMATCH, format!("this pattern is a `{}`, but the value is a `{}`", td.name, show(p, t)), pat.span()));
                }
                let TypeKind::Record(fs) = &td.decl.kind else { return };
                for (f, a) in fields {
                    let ft = fs.iter().find(|(n, _, _)| n == f).map(|(_, ft, _)| field_type(p, tid, t, ft)).unwrap_or(Ty::Dyn);
                    self.bind_pattern(a, ft);
                }
            }
            Pattern::Array { elems, rest, span } => {
                let el = match t {
                    Ty::Arr(e, _) => Ty::of_el(e),
                    Ty::Dyn | Ty::Never => Ty::Dyn,
                    other => {
                        self.err(Diag::new(&C::TYPE_MISMATCH, format!("an array pattern matches an array or string, but the value is {}", show(self.inf.p, other)), *span));
                        Ty::Dyn
                    }
                };
                for a in elems {
                    self.bind_pattern(a, el);
                }
                if let Some(Some(n)) = rest {
                    // the rest has the array's own type; keyed one past the pattern
                    let rt = if let Ty::Arr(e, _) = t { Ty::Arr(e, 0) } else { Ty::Dyn };
                    self.out.binds.insert(id(pat) + 1, rt);
                    self.scopes.last_mut().unwrap().insert(n.clone(), (id(pat) + 1, rt));
                }
            }
        }
    }
}

/// Result type of a binary operator.
pub fn binary_ty(op: BinOp, a: Ty, b: Ty) -> Ty {
    use BinOp::*;
    if a == Ty::Never || b == Ty::Never {
        return Ty::Never;
    }
    // strings compare as whole values
    if matches!(op, Eq | Ne | Lt | Le | Gt | Ge) && a == Ty::Arr(El::Char, 0) && b == Ty::Arr(El::Char, 0) {
        return Ty::Bool;
    }
    // "a" + "b" joins strings
    if op == Add && a == Ty::Arr(El::Char, 0) && b == Ty::Arr(El::Char, 0) {
        return Ty::Arr(El::Char, 0);
    }
    // (xl, yl: the operand is a list; see num_join)
    let scalar = |x: Ty, xl: bool, y: Ty, yl: bool| -> Ty {
        match op {
            Eq | Ne | Lt | Le | Gt | Ge | And | Or => Ty::Bool,
            BitAnd | BitOr | BitXor if x == Ty::Bool && y == Ty::Bool => Ty::Bool,
            BitAnd | BitOr | BitXor | Shl | Shr | UShr if x == Ty::Dyn || y == Ty::Dyn => Ty::Dyn,
            BitAnd | BitOr | BitXor | Shl | Shr | UShr if num_join(x, xl, y, yl) == Ty::I32 => Ty::I32,
            BitAnd | BitOr | BitXor | Shl | Shr | UShr => Ty::I64,
            _ if x == Ty::Dyn || y == Ty::Dyn => Ty::Dyn,
            Div => match num_join(x, xl, y, yl) {
                t if t.is_float() => t,
                _ => Ty::F64, // integers divided: a float
            },
            _ => num_join(x, xl, y, yl),
        }
    };
    match (a, b) {
        (x, y) if x.is_scalar() && y.is_scalar() => scalar(x, false, y, false),
        (Ty::Arr(x, n), y) if y.is_scalar() => Ty::Arr(scalar(Ty::of_el(x), true, y, false).el(), n),
        (x, Ty::Arr(y, n)) if x.is_scalar() => Ty::Arr(scalar(x, false, Ty::of_el(y), true).el(), n),
        (Ty::Arr(x, n), Ty::Arr(y, m)) => Ty::Arr(scalar(Ty::of_el(x), true, Ty::of_el(y), true).el(), if n == m { n } else { 0 }),
        (Ty::Arr(_, _), _) | (_, Ty::Arr(_, _)) => Ty::Dyn,
        _ => Ty::Dyn,
    }
}

/// The type a number written in the program takes next to a value of
/// type `other`: f32 for a float (or an integer) next to an f32, i32 for an
/// integer that fits next to an i32; None to keep its own.
fn literal_as(e: &Expr, other: Ty) -> Option<Ty> {
    let (lit, neg) = match &e.kind {
        ExprKind::Unary { op: UnOp::Neg, expr } => (expr.as_ref(), true),
        _ => (e, false),
    };
    match (&lit.kind, other) {
        (ExprKind::Float(_), Ty::F32) => Some(Ty::F32),
        (ExprKind::Int(v), Ty::I32) => {
            let v = if neg { v.checked_neg()? } else { *v };
            (v == v as i32 as i64).then_some(Ty::I32)
        }
        _ => None,
    }
}

fn is_literal(e: &Expr) -> bool {
    match &e.kind {
        ExprKind::Int(_) | ExprKind::Float(_) => true,
        ExprKind::Unary { op: UnOp::Neg, expr } => matches!(expr.kind, ExprKind::Int(_) | ExprKind::Float(_)),
        _ => false,
    }
}

/// The literal an `else` branch is (`else { 0 }`), if it is one.
fn tail_literal(e: &Expr) -> Option<&Expr> {
    match &e.kind {
        ExprKind::Block(b) if b.stmts.is_empty() => b.tail.as_deref().filter(|l| is_literal(l)),
        _ => is_literal(e).then_some(e),
    }
}

/// The type a literal branch takes next to a branch of type `other`.
fn literal_branch_as(e: &Expr, other: Ty) -> Option<Ty> {
    let (lit, neg) = match &e.kind {
        ExprKind::Unary { op: UnOp::Neg, expr } => (expr.as_ref(), true),
        _ => (e, false),
    };
    match (&lit.kind, other) {
        (ExprKind::Float(_) | ExprKind::Int(_), Ty::F32) => Some(Ty::F32),
        (ExprKind::Int(v), Ty::I32) => {
            let v = if neg { v.checked_neg()? } else { *v };
            (v == v as i32 as i64).then_some(Ty::I32)
        }
        _ => None,
    }
}

/// The number type of arithmetic on `x` and `y` (`xl`, `yl`: a list's
/// elements). Floats beat integers; between two widths of the same kind, a
/// list keeps its own type against a single value (`[f32] * 0.5` stays
/// `[f32]`), else the wider wins. The runtime (arith_dtype) does the same.
pub fn num_join(x: Ty, xl: bool, y: Ty, yl: bool) -> Ty {
    if x == Ty::Dyn || y == Ty::Dyn {
        return Ty::Dyn;
    }
    let (fx, fy) = (x.is_float(), y.is_float());
    if fx != fy {
        return if fx { x } else { y };
    }
    if fx {
        return if x == y {
            x
        } else if xl != yl {
            if xl { x } else { y }
        } else {
            Ty::F64
        };
    }
    let small = |t: Ty| matches!(t, Ty::Bool | Ty::Char);
    match (x, y) {
        (Ty::I32, Ty::I32) => Ty::I32,
        (Ty::I32, t) | (t, Ty::I32) if small(t) => Ty::I32,
        (Ty::I32, Ty::I64) | (Ty::I64, Ty::I32) if xl != yl => {
            if xl { x } else { y }
        }
        _ => Ty::I64,
    }
}

/// Result type of a primitive (other than `¨`).
pub fn prim_ty(p: Prim, a: Ty, b: Ty) -> Ty {
    let reduce = |a: Ty| match a {
        Ty::Arr(El::F64, _) => Ty::F64,
        Ty::Arr(El::F32, _) => Ty::F32,
        Ty::Arr(El::I32, _) => Ty::I32,
        Ty::Arr(El::I64 | El::Bool | El::Char | El::U8, _) => Ty::I64,
        Ty::Arr(El::Dyn, _) | Ty::Dyn => Ty::Dyn,
        s => s, // reducing a scalar returns it
    };
    match p {
        Prim::Iota | Prim::Shape | Prim::Range => Ty::Arr(El::I64, 0),
        Prim::Reverse => a,
        Prim::Sum | Prim::Product | Prim::Maximum | Prim::Minimum => reduce(a),
        Prim::Len => Ty::I64,
        Prim::Print => Ty::Unit,
        Prim::Take | Prim::Drop | Prim::Keep => match a {
            Ty::Arr(e, _) => Ty::Arr(e, 0), // the length changes
            _ => Ty::Dyn,
        },
        Prim::Gather | Prim::Trail | Prim::Evolve | Prim::Settle => Ty::Dyn,
        // rank ≥ 2 arrays are dynamic values for now (the runtime knows their shape)
        Prim::Reshape | Prim::Transpose | Prim::MatMul => Ty::Dyn,
        Prim::Each | Prim::Max | Prim::Min | Prim::Abs => {
            let _ = b;
            Ty::Dyn
        }
    }
}

/// Run inference and convert errors for `mimic check`.
pub fn check_types(p: &Program, library: bool) -> Vec<FileDiag> {
    infer(p, &default_roots(p, library)).errors
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SourceFile;
    use mimic_syntax::parser::parse_file;

    fn prog(src: &str) -> Program {
        let f = SourceFile { path: "m.mim".into(), display: "m.mim".into(), src: src.into(), ast: parse_file(src).unwrap(), package: "t".into() };
        let (p, d) = Program::build(vec![f], "t");
        assert!(d.is_empty());
        p
    }
    fn fid(p: &Program, n: &str) -> FnId {
        p.fns.iter().position(|f| f.name == n).unwrap()
    }

    #[test]
    fn recursion_and_scalars() {
        let p = prog("module main\nfn main() { print(fib(30)) }\nfn fib(n) {\n  if n < 2 {\n    return n\n  }\n  fib(n - 1) + fib(n - 2)\n}\n");
        let t = infer(&p, &default_roots(&p, false));
        assert!(t.errors.is_empty());
        assert_eq!(t.spec(&(fid(&p, "fib"), vec![Ty::I64])).ret, Ty::I64);
        assert_eq!(t.spec(&(fid(&p, "main"), vec![])).ret, Ty::Unit);
    }

    #[test]
    fn arrays_and_reductions() {
        let p = prog("module main\nfn main() { print(f(⍳ 20)) }\nfn f(xs) { (xs × 2.0 ▷ +/) + (xs ▷ ≢) }\n");
        let t = infer(&p, &default_roots(&p, false));
        // ⍳ 20: longer than MAX_FIXED, so its length is not carried
        assert_eq!(t.spec(&(fid(&p, "f"), vec![Ty::Arr(El::I64, 0)])).ret, Ty::F64);
        let p = prog("module main\nfn main() { print(f(⍳ 5)) }\nfn f(xs) { xs × 2 ▷ +/ }\n");
        let t = infer(&p, &default_roots(&p, false));
        // ⍳ 5: five numbers, a length the compiler knows
        assert_eq!(t.spec(&(fid(&p, "f"), vec![Ty::Arr(El::I64, 5)])).ret, Ty::I64);
    }

    #[test]
    fn mixed_branches_are_dynamic() {
        let p = prog("module main\nfn main() { print(g(true)) }\nfn g(c) { if c { 1 } else { 2.5 } }\n");
        let t = infer(&p, &default_roots(&p, false));
        assert_eq!(t.spec(&(fid(&p, "g"), vec![Ty::Bool])).ret, Ty::Dyn);
    }

    #[test]
    fn annotations_coerce_and_report() {
        let p = prog("module main\nfn main() {\n  print(h(3))\n  print(k(2.5))\n}\nfn h(x: f64) -> f64 { x }\nfn k(n: i64) -> i64 { n }\n");
        let t = infer(&p, &default_roots(&p, false));
        assert_eq!(t.spec(&(fid(&p, "h"), vec![Ty::I64])).params, vec![Ty::F64], "i64 coerces into an f64 parameter");
        assert!(t.errors.iter().any(|e| e.diag.msg.contains("argument `n` of `k`: expected i64, found f64") && e.diag.span.line == 4), "{:?}", t.errors.iter().map(|e| &e.diag.msg).collect::<Vec<_>>());
    }

    #[test]
    fn mutation_joins() {
        let p = prog("module main\nfn main() {\n  let mut x = 1\n  x = 2.5\n  print(x)\n}\n");
        let t = infer(&p, &default_roots(&p, false));
        let s = t.spec(&(fid(&p, "main"), vec![]));
        assert!(s.lets.values().any(|t| *t == Ty::Dyn));
    }
}
