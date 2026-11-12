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
use mimic_syntax::Diag;
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

/// Generic instances (`Option[i64]`), interned so `Ty::Obj` stays a small
/// copyable id. A type without arguments is `Ty::Obj(type id)`; an instance is
/// `Ty::Obj(INST | index)`. Values are represented the same whatever the
/// arguments (fields are boxed handles), so instances only sharpen types.
static INSTANCES: Mutex<Vec<(usize, Vec<Ty>)>> = Mutex::new(Vec::new());
const INST: u32 = 1 << 31;

/// The record/union type `tid` with type arguments `args` (`Never` = not known yet).
pub fn obj(tid: usize, args: &[Ty]) -> Ty {
    if args.is_empty() {
        return Ty::Obj(tid as u32);
    }
    let mut t = INSTANCES.lock().unwrap();
    let key = (tid, args.to_vec());
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
    ann_env(p, p.types[tid].module, ft, &env).unwrap_or(Ty::Dyn)
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
        (Ty::Arr(x), Ty::Arr(y)) => x == El::Dyn || y == El::Dyn || compatible(Ty::of_el(x), Ty::of_el(y)),
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
            if p.is_empty() || q.is_empty() || p.len() != q.len() {
                return a;
            }
            let r: Vec<Ty> = p.iter().zip(&q).map(|(s, t)| refine(*s, *t)).collect();
            obj(obj_tid(x), &r)
        }
        (Ty::Arr(El::Dyn), g @ Ty::Arr(_)) => g,
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
        Type::Array(inner, _) => {
            if let Ty::Arr(el) = got
                && el != El::Dyn
            {
                unify(p, module, inner, Ty::of_el(el), params, env, conflict);
            }
        }
        Type::App(n, targs, _) => {
            if let (Ok(Resolved::Type(tid)), Ty::Obj(id)) = (p.resolve(module, &type_path(n)), got)
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
    Dyn,
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
    /// Array (vectors); `Arr(El::Char)` is a string.
    Arr(El),
    /// A record or union value of a user-declared type.
    Obj(u32),
    /// A tuple (interned element types, see [`tuple_elems`]).
    Tup(u32),
    /// Unknown statically: a boxed runtime value.
    Dyn,
}

impl Ty {
    pub fn is_scalar(self) -> bool {
        matches!(self, Ty::I64 | Ty::F64 | Ty::Bool | Ty::Char)
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
            Ty::Arr(El::Char) => El::Str,
            Ty::Obj(t) => El::Obj(t),
            _ => El::Dyn,
        }
    }
    pub fn of_el(e: El) -> Ty {
        match e {
            El::I64 => Ty::I64,
            El::F64 => Ty::F64,
            El::Bool => Ty::Bool,
            El::Char => Ty::Char,
            El::Str => Ty::Arr(El::Char),
            El::Obj(t) => Ty::Obj(t),
            El::U8 => Ty::I64,
            El::Dyn => Ty::Dyn,
        }
    }
    pub fn join(self, o: Ty) -> Ty {
        match (self, o) {
            (Ty::Never, x) | (x, Ty::Never) => x,
            (a, b) if a == b => a,
            (Ty::Arr(_), Ty::Arr(_)) => Ty::Arr(El::Dyn),
            (Ty::Obj(a), Ty::Obj(b)) if obj_tid(a) == obj_tid(b) => {
                let (x, y) = (obj_args(a), obj_args(b));
                if x.len() != y.len() {
                    return Ty::Obj(obj_tid(a) as u32);
                }
                let j: Vec<Ty> = x.iter().zip(&y).map(|(p, q)| p.join(*q)).collect();
                obj(obj_tid(a), &j)
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
            El::Dyn => "?",
        };
        match self {
            Ty::Never => write!(f, "never"),
            Ty::Unit => write!(f, "()"),
            Ty::I64 => write!(f, "i64"),
            Ty::F64 => write!(f, "f64"),
            Ty::Bool => write!(f, "bool"),
            Ty::Char => write!(f, "char"),
            Ty::Arr(El::Char) => write!(f, "str"),
            Ty::Arr(e) => write!(f, "[{}]", el(e)),
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
            "i8" | "i16" | "i32" | "i64" | "u8" | "u16" | "u32" | "u64" | "isize" | "usize" | "cptr" => Ty::I64,
            "f32" | "f64" => Ty::F64,
            "bool" => Ty::Bool,
            "char" | "c_char" => Ty::Char,
            "str" | "cstr" => Ty::Arr(El::Char),
            "any" => Ty::Dyn,
            _ => return None,
        },
        Type::Array(inner, _) if matches!(&**inner, Type::Named(n, _) if n == "u8") => Ty::Arr(El::U8),
        Type::Array(inner, _) => match ann_ty(inner)? {
            Ty::I64 => Ty::Arr(El::I64),
            Ty::F64 => Ty::Arr(El::F64),
            Ty::Bool => Ty::Arr(El::Bool),
            _ => return None,
        },
        Type::Ptr(..) | Type::App(..) => return None,
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
        Type::Named(n, _) => match p.resolve(module, &type_path(n)) {
            // a generic type named without arguments: any instance
            Ok(Resolved::Type(id)) => Some(obj(id, &vec![Ty::Dyn; p.types[id].decl.params.len()])),
            _ => None,
        },
        Type::App(n, args, _) => match p.resolve(module, &type_path(n)) {
            Ok(Resolved::Type(id)) => {
                let want = p.types[id].decl.params.len();
                let mut a: Vec<Ty> = args.iter().map(|x| ann_env(p, module, x, env).unwrap_or(Ty::Dyn)).collect();
                a.resize(want, Ty::Dyn);
                Some(obj(id, &a))
            }
            _ => None,
        },
        Type::Array(inner, _) => Some(Ty::Arr(ann_env(p, module, inner, env)?.el())),
        Type::Map(..) => Some(Ty::Dyn),
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
        Ty::Obj(_) | Ty::Tup(_) | Ty::Arr(El::Obj(_) | El::Str) => match got {
            Ty::Never | Ty::Dyn => Ok(a),
            Ty::Arr(El::Dyn) if matches!(a, Ty::Arr(_)) => Ok(a),
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
        Ty::Arr(El::Obj(id)) => format!("[{}]", show(p, Ty::Obj(id))),
        Ty::Arr(El::Str) => "[str]".into(),
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
    if a == Ty::Arr(El::Dyn) && matches!(got, Ty::Arr(_) | Ty::Never | Ty::Dyn) {
        return Ok(got);
    }
    if a == Ty::Dyn {
        return Ok(Ty::Dyn); // `any` accepts every value (boxed)
    }
    let ok = matches!(
        (a, got),
        (_, Ty::Never | Ty::Dyn)
            | (Ty::I64, Ty::I64 | Ty::Bool)
            | (Ty::F64, Ty::I64 | Ty::F64 | Ty::Bool)
            | (Ty::Bool, Ty::Bool)
            | (Ty::Char, Ty::Char)
            | (Ty::Arr(El::Char), Ty::Arr(El::Char | El::Dyn))
            | (Ty::Arr(El::I64), Ty::Arr(El::I64 | El::Bool | El::Dyn))
            | (Ty::Arr(El::F64), Ty::Arr(El::I64 | El::F64 | El::Bool | El::Dyn))
            | (Ty::Arr(El::Bool), Ty::Arr(El::Bool | El::Dyn))
            | (Ty::Arr(El::U8), Ty::Arr(El::U8 | El::Dyn))
    );
    if !ok {
        return Err(format!("expected {a}, found {got}"));
    }
    Ok(match (a, got) {
        (Ty::Arr(_), g) if g != Ty::Dyn && g != Ty::Never => g, // arrays: a check, not a conversion
        (Ty::Arr(_), _) => got,
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
            "cstr" => Ty::Arr(El::Char),
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
        let tys: Option<Vec<Ty>> = d.params.iter().map(|prm| prm.ty.as_ref().and_then(|t| ann_in(p, f.module, t))).collect();
        if let Some(tys) = tys
            && !tys.is_empty()
        {
            roots.push((fid, tys));
        }
        if library && f.is_pub && p.mods[f.module].package == p.root_package {
            roots.push((fid, vec![Ty::Dyn; d.params.len()]));
        }
    }
    roots
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
            if let Some(rt) = &d.ret {
                match coerce_env(p, module, Some(rt), ret, &env) {
                    Ok(t) => ret = t,
                    Err(m) => w.err(Diag::new(&C::TYPE_MISMATCH, format!("return value of `{path}`: {m}"), d.span)),
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
                            self.err(Diag::new(&C::TYPE_MISMATCH, format!("`let {name}`: {m}"), *span));
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
                                (Ty::Arr(e), s) if Ty::of_el(e) == s || s == Ty::Never => cur,
                                (Ty::Arr(El::I64 | El::Bool), Ty::F64) => Ty::Arr(El::Dyn),
                                (Ty::Arr(_), _) => Ty::Arr(El::Dyn),
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

    /// `f` used as a value: its dynamic specialisation must exist.
    fn fn_value(&mut self, f: FnId) {
        if !self.inf.p.fns[f].is_extern() {
            let n = self.inf.p.fns[f].arity();
            self.inf.ensure((f, vec![Ty::Dyn; n]));
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
            ExprKind::Str(_) => Ty::Arr(El::Char),
            ExprKind::Array(xs) => {
                let mut el = Ty::Never;
                for x in xs {
                    let t = self.expr(x);
                    el = match (el, t) {
                        (Ty::Never, t) => t,
                        (a, b) if a == b => a,
                        (a, b) if a.is_scalar() && b.is_scalar() => {
                            if a == Ty::F64 || b == Ty::F64 { Ty::F64 } else { Ty::I64 }
                        }
                        _ => Ty::Dyn,
                    };
                }
                match el {
                    // `[]` has no element type of its own: it fits any list
                    Ty::Never if xs.is_empty() => Ty::Arr(El::Dyn),
                    Ty::Never => Ty::Never,
                    t => Ty::Arr(t.el()), // numbers, strings and records; anything else is dynamic
                }
            }
            ExprKind::Path(path) => match self.lookup(&path[0]) {
                Some((_, t)) if path.len() == 1 => t,
                _ => match self.inf.p.resolve(self.module, path) {
                    // `None`: the type arguments are not known yet
                    Ok(Resolved::Variant(t, _)) => obj(t, &vec![Ty::Never; self.inf.p.types[t].decl.params.len()]),
                    Ok(Resolved::Fn(f)) => {
                        self.fn_value(f);
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
                                let env: Vec<(String, Ty)> = decl
                                    .type_params
                                    .iter()
                                    .zip(args.iter().filter_map(|a| match &a.kind {
                                        ExprKind::TypeArg(t) => Some(ann_in(p, self.module, t).unwrap_or(Ty::Dyn)),
                                        _ => None,
                                    }))
                                    .map(|(n, t)| (n.clone(), t))
                                    .collect();
                                return match &decl.ret {
                                    Some(rt) => ann_env(p, m, rt, &env).unwrap_or(Ty::Dyn),
                                    None => Ty::Unit,
                                };
                            }
                            self.check_args_at_call(e, path, f, args, &mut argt);
                            self.call_user(id(e), f, argt)
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
                    Ty::Arr(el) => Ty::of_el(el),
                    Ty::Never => Ty::Never,
                    _ => Ty::Dyn,
                }
            }
            ExprKind::Unary { op, expr } => {
                let t = self.expr(expr);
                if *op == UnOp::BitNot
                    && matches!(t, Ty::F64 | Ty::Char | Ty::Arr(El::F64 | El::Char | El::Str))
                {
                    self.err(Diag::new(&C::NO_OPERATOR, "`~` needs an integer (or a bool)", expr.span));
                    return Ty::Dyn;
                }
                let scalar = |s: Ty| match (op, s) {
                    (UnOp::Not, _) => Ty::Bool,
                    (UnOp::BitNot, Ty::Bool) => Ty::Bool,
                    (UnOp::BitNot, Ty::Dyn) => Ty::Dyn,
                    (UnOp::BitNot, _) => Ty::I64,
                    (_, Ty::F64) => Ty::F64,
                    (_, Ty::Dyn) => Ty::Dyn,
                    _ => Ty::I64,
                };
                match t {
                    Ty::Never => Ty::Never,
                    Ty::Arr(el) => Ty::Arr(scalar(Ty::of_el(el)).el()),
                    s if s.is_scalar() => scalar(s),
                    _ => Ty::Dyn,
                }
            }
            ExprKind::Binary { op, lhs, rhs } => {
                let a = self.expr(lhs);
                let b = self.expr(rhs);
                let elem = |t: Ty| match t {
                    Ty::Arr(x) => Ty::of_el(x),
                    t => t,
                };
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
                    } else if matches!(a, Ty::Arr(_)) || matches!(b, Ty::Arr(_)) {
                        Ty::Arr(El::Bool)
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
                        Ty::Arr(x) => Ty::of_el(x),
                        t => t,
                    };
                    for (t, side) in [(a, lhs), (b, rhs)] {
                        let t = el(t);
                        if matches!(t, Ty::F64 | Ty::Char | Ty::Arr(_)) || (t == Ty::Bool && op.is_shift()) {
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
                Ty::Arr(El::Char)
            }
            ExprKind::Named { value, .. } => {
                self.expr(value);
                Ty::Dyn
            }
            ExprKind::Record { path, fields } => {
                let p = self.inf.p;
                let Ok(Resolved::Type(t)) = p.resolve(self.module, path) else { return Ty::Dyn };
                let td = &p.types[t];
                let TypeKind::Record(decl) = &td.decl.kind else { return Ty::Dyn };
                let vts: Vec<Ty> = fields.iter().map(|(_, v, _)| self.expr(v)).collect();
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
                obj(t, &targs)
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
                        Ty::Arr(el) => Ty::of_el(el),
                        Ty::Never => return Ty::Never,
                        s => s,
                    };
                    let r = match &args[1].kind {
                        ExprKind::Lambda { .. } => self.lambda(&args[1], vec![el]),
                        ExprKind::Path(path) if path.len() == 1 && self.lookup(&path[0]).is_some() => Ty::Dyn,
                        ExprKind::Path(path) => match self.inf.p.resolve(self.module, path) {
                            Ok(Resolved::Fn(f)) => self.call_user(id(e), f, vec![el]),
                            _ => Ty::Dyn,
                        },
                        _ => {
                            self.expr(&args[1]);
                            Ty::Dyn
                        }
                    };
                    return match (a, r) {
                        (_, Ty::Never) => Ty::Never,
                        (Ty::Arr(_), r) => Ty::Arr(r.el()),
                        (_, r) => r, // `¨` on a scalar applies f once
                    };
                }
                let b = if args.len() > 1 { self.expr(&args[1]) } else { Ty::Unit };
                for extra in args.iter().skip(2) {
                    self.expr(extra);
                }
                if a == Ty::Never || b == Ty::Never {
                    return Ty::Never;
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
                let t = self.block(then);
                match els {
                    Some(x) => t.join(self.expr(x)),
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
                if !matches!(t, Ty::Arr(El::Char) | Ty::Dyn | Ty::Never) {
                    self.err(Diag::new(&C::TYPE_MISMATCH, format!("a route pattern matches a str, but the value is {}", show(self.inf.p, t)), *span));
                }
                let mut k = 0;
                for part in parts {
                    if let RoutePart::Capture { name, ty } = part {
                        let ct = match ty.as_deref() {
                            Some("i64") => Ty::I64,
                            Some("f64") => Ty::F64,
                            _ => Ty::Arr(El::Char),
                        };
                        self.out.route_binds.insert((id(pat), k), ct);
                        self.scopes.last_mut().unwrap().insert(name.clone(), (id(pat) + 1 + k, ct));
                        k += 1;
                    }
                }
            }
            Pattern::Variant { path, args, .. } => {
                let p = self.inf.p;
                let Ok(Resolved::Variant(tid, v)) = p.resolve(self.module, path) else { return };
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
                let Ok(Resolved::Type(tid)) = p.resolve(self.module, path) else { return };
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
                    Ty::Arr(e) => Ty::of_el(e),
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
                    let rt = if matches!(t, Ty::Arr(_)) { t } else { Ty::Dyn };
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
    if matches!(op, Eq | Ne | Lt | Le | Gt | Ge) && a == Ty::Arr(El::Char) && b == Ty::Arr(El::Char) {
        return Ty::Bool;
    }
    // "a" + "b" joins strings
    if op == Add && a == Ty::Arr(El::Char) && b == Ty::Arr(El::Char) {
        return Ty::Arr(El::Char);
    }
    let scalar = |x: Ty, y: Ty| -> Ty {
        match op {
            Eq | Ne | Lt | Le | Gt | Ge | And | Or => Ty::Bool,
            BitAnd | BitOr | BitXor if x == Ty::Bool && y == Ty::Bool => Ty::Bool,
            BitAnd | BitOr | BitXor | Shl | Shr | UShr if x == Ty::Dyn || y == Ty::Dyn => Ty::Dyn,
            BitAnd | BitOr | BitXor | Shl | Shr | UShr => Ty::I64,
            Div => Ty::F64,
            _ if x == Ty::Dyn || y == Ty::Dyn => Ty::Dyn,
            _ if x == Ty::F64 || y == Ty::F64 => Ty::F64,
            _ => Ty::I64,
        }
    };
    match (a, b) {
        (x, y) if x.is_scalar() && y.is_scalar() => scalar(x, y),
        (Ty::Arr(x), y) if y.is_scalar() => Ty::Arr(scalar(Ty::of_el(x), y).el()),
        (x, Ty::Arr(y)) if x.is_scalar() => Ty::Arr(scalar(x, Ty::of_el(y)).el()),
        (Ty::Arr(x), Ty::Arr(y)) => Ty::Arr(scalar(Ty::of_el(x), Ty::of_el(y)).el()),
        (Ty::Arr(_), _) | (_, Ty::Arr(_)) => Ty::Dyn,
        _ => Ty::Dyn,
    }
}

/// Result type of a primitive (other than `¨`).
pub fn prim_ty(p: Prim, a: Ty, b: Ty) -> Ty {
    let reduce = |a: Ty| match a {
        Ty::Arr(El::F64) => Ty::F64,
        Ty::Arr(El::I64 | El::Bool | El::Char) => Ty::I64,
        Ty::Arr(El::Dyn) | Ty::Dyn => Ty::Dyn,
        s => s, // reducing a scalar returns it
    };
    match p {
        Prim::Iota | Prim::Shape | Prim::Range => Ty::Arr(El::I64),
        Prim::Reverse => a,
        Prim::Sum | Prim::Product | Prim::Maximum | Prim::Minimum => reduce(a),
        Prim::Len => Ty::I64,
        Prim::Print => Ty::Unit,
        Prim::Take | Prim::Drop | Prim::Keep => match a {
            Ty::Arr(_) => a,
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
        let p = prog("module main\nfn main() { print(f(⍳ 10)) }\nfn f(xs) { (xs × 2.0 ▷ +/) + (xs ▷ ≢) }\n");
        let t = infer(&p, &default_roots(&p, false));
        assert_eq!(t.spec(&(fid(&p, "f"), vec![Ty::Arr(El::I64)])).ret, Ty::F64);
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
