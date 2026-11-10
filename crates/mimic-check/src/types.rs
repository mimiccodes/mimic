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

use crate::{FileDiag, FnId, FnKind, ModId, Program, Resolved};
use mimic_syntax::ast::*;
use mimic_syntax::glyphs::Prim;
use mimic_syntax::Diag;
use std::collections::HashMap;

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
            El::Dyn => Ty::Dyn,
        }
    }
    pub fn join(self, o: Ty) -> Ty {
        match (self, o) {
            (Ty::Never, x) | (x, Ty::Never) => x,
            (a, b) if a == b => a,
            (Ty::Arr(_), Ty::Arr(_)) => Ty::Arr(El::Dyn),
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
    pub lambdas: HashMap<usize, (Ty, Ty)>,
    /// Types of `match` pattern bindings (by pattern id).
    pub binds: HashMap<usize, Ty>,
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
            "i8" | "i16" | "i32" | "i64" | "u8" | "u16" | "u32" | "u64" | "isize" | "usize" => Ty::I64,
            "f32" | "f64" => Ty::F64,
            "bool" => Ty::Bool,
            "char" | "c_char" => Ty::Char,
            "str" => Ty::Arr(El::Char),
            _ => return None,
        },
        Type::Array(inner, _) => match ann_ty(inner)? {
            Ty::I64 => Ty::Arr(El::I64),
            Ty::F64 => Ty::Arr(El::F64),
            Ty::Bool => Ty::Arr(El::Bool),
            _ => return None,
        },
        Type::Ptr(..) => return None,
    })
}

/// An annotation's type, resolving record/union names from `module`.
pub fn ann_in(p: &Program, module: ModId, t: &Type) -> Option<Ty> {
    if let Some(x) = ann_ty(t) {
        return Some(x);
    }
    match t {
        Type::Named(n, _) => match p.resolve(module, std::slice::from_ref(n)) {
            Ok(Resolved::Type(id)) => Some(Ty::Obj(id as u32)),
            _ => None,
        },
        Type::Array(inner, _) => Some(Ty::Arr(ann_in(p, module, inner)?.el())),
        Type::Ptr(..) => None,
    }
}

/// Coerce a value of type `got` into a slot annotated `ann` (user types allowed).
pub fn coerce_in(p: &Program, module: ModId, ann: Option<&Type>, got: Ty) -> Result<Ty, String> {
    let Some(t) = ann else { return Ok(got) };
    match ann_in(p, module, t) {
        Some(a @ (Ty::Obj(_) | Ty::Arr(El::Obj(_) | El::Str))) => match got {
            Ty::Never | Ty::Dyn => Ok(a),
            g if g == a => Ok(a),
            Ty::Arr(El::Dyn) if matches!(a, Ty::Arr(_)) => Ok(a),
            g => Err(format!("expected {}, found {}", show(p, a), show(p, g))),
        },
        _ => coerce(ann, got),
    }
}

/// A type for messages, naming user types.
pub fn show(p: &Program, t: Ty) -> String {
    match t {
        Ty::Obj(id) => p.types[id as usize].name.clone(),
        Ty::Arr(El::Obj(id)) => format!("[{}]", p.types[id as usize].name),
        Ty::Arr(El::Str) => "[str]".into(),
        other => other.to_string(),
    }
}

/// Coerce a value of type `got` into a slot annotated `ann`.
/// Scalars convert (i64→f64, bool→i64); arrays must already be compatible.
/// `Dyn` is accepted (checked at run time). Returns the slot's type.
pub fn coerce(ann: Option<&Type>, got: Ty) -> Result<Ty, String> {
    let Some(a) = ann.and_then(ann_ty) else { return Ok(got) };
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
            // parameters (annotation coercion)
            for (prm, arg) in d.params.iter().zip(&k.1) {
                let t = match coerce_in(p, module, prm.ty.as_ref(), *arg) {
                    Ok(t) => t,
                    Err(m) => {
                        w.err(Diag::new(format!("argument `{}` of `{path}`: {m}", prm.name), prm.span));
                        Ty::Dyn
                    }
                };
                w.out.params.push(t);
                w.scopes[0].insert(prm.name.clone(), (id(prm), t));
            }
            let body = w.block(&d.body);
            let mut ret = body.join(w.ret);
            if let Some(rt) = &d.ret {
                match coerce_in(p, module, Some(rt), ret) {
                    Ok(t) => ret = t,
                    Err(m) => w.err(Diag::new(format!("return value of `{path}`: {m}"), d.span)),
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
                            self.err(Diag::new(format!("`let {name}`: {m}"), *span));
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

    fn call_user(&mut self, at: usize, fid: FnId, args: Vec<Ty>) -> Ty {
        if self.inf.p.fns[fid].is_extern() {
            let FnKind::Extern { decl, .. } = &self.inf.p.fns[fid].kind else { unreachable!() };
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

    /// A lambda applied to one argument of type `arg`.
    fn lambda(&mut self, e: &Expr, arg: Ty) -> Ty {
        let ExprKind::Lambda { param, body } = &e.kind else { return Ty::Dyn };
        let saved = std::mem::replace(&mut self.scopes, vec![HashMap::new()]);
        self.scopes[0].insert(param.clone(), (id(e), arg));
        let r = self.expr(body);
        self.scopes = saved;
        self.out.lambdas.insert(id(e), (arg, r));
        self.out.exprs.insert(id(e), r);
        r
    }

    fn expr(&mut self, e: &Expr) -> Ty {
        let t = self.expr_inner(e);
        self.out.exprs.insert(id(e), t);
        t
    }

    fn expr_inner(&mut self, e: &Expr) -> Ty {
        match &e.kind {
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
                    Ty::Never if xs.is_empty() => Ty::Arr(El::I64),
                    Ty::Never => Ty::Never,
                    t => Ty::Arr(t.el()), // numbers, strings and records; anything else is dynamic
                }
            }
            ExprKind::Path(path) => match self.lookup(&path[0]) {
                Some((_, t)) if path.len() == 1 => t,
                _ => match self.inf.p.resolve(self.module, path) {
                    Ok(Resolved::Variant(t, _)) => Ty::Obj(t as u32),
                    _ => Ty::Dyn,
                },
            },
            ExprKind::Call { callee, args } => {
                let argt: Vec<Ty> = args.iter().map(|a| self.expr(a)).collect();
                match &callee.kind {
                    ExprKind::Lambda { .. } => self.lambda(callee, argt.first().copied().unwrap_or(Ty::Dyn)),
                    ExprKind::Path(path) => match self.inf.p.resolve(self.module, path) {
                        Ok(Resolved::Fn(f)) => self.call_user(id(e), f, argt),
                        Ok(Resolved::Variant(t, v)) => {
                            let td = &self.inf.p.types[t];
                            let (fields, tm, vname) = (td.variants()[v].fields.clone(), td.module, td.variants()[v].name.clone());
                            for (i, (ft, at)) in fields.iter().zip(&argt).enumerate() {
                                if let Err(m) = coerce_in(self.inf.p, tm, Some(ft), *at) {
                                    self.err(Diag::new(format!("field {} of `{vname}`: {m}", i + 1), args[i].span));
                                }
                            }
                            Ty::Obj(t as u32)
                        }
                        _ => Ty::Dyn,
                    },
                    _ => Ty::Dyn,
                }
            }
            ExprKind::Index { base, index } => {
                let b = self.expr(base);
                self.expr(index);
                match b {
                    Ty::Arr(el) => Ty::of_el(el),
                    Ty::Never => Ty::Never,
                    _ => Ty::Dyn,
                }
            }
            ExprKind::Unary { op, expr } => {
                let t = self.expr(expr);
                let scalar = |s: Ty| match (op, s) {
                    (UnOp::Not, _) => Ty::Bool,
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
                if matches!(a, Ty::Obj(_)) || matches!(b, Ty::Obj(_)) {
                    self.err(Diag::new(format!("operator `{op:?}` is not defined for records and variants (use `match`)"), e.span));
                    return Ty::Dyn;
                }
                binary_ty(*op, a, b)
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
                for (n, v, sp) in fields {
                    let vt = self.expr(v);
                    if let Some((_, ft, _)) = decl.iter().find(|(d, _, _)| d == n)
                        && let Err(m) = coerce_in(p, td.module, Some(ft), vt)
                    {
                        self.err(Diag::new(format!("field `{n}` of `{}`: {m}", td.name), *sp));
                    }
                }
                Ty::Obj(t as u32)
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
                        let td = &p.types[t as usize];
                        match &td.decl.kind {
                            TypeKind::Record(decl) => match decl.iter().find(|(d, _, _)| d == n) {
                                Some((_, ft, _)) => {
                                    if let Err(m) = coerce_in(p, td.module, Some(ft), vt) {
                                        self.err(Diag::new(format!("field `{n}` of `{}`: {m}", td.name), *sp));
                                    }
                                }
                                None => self.err(Diag::new(format!("`{}` has no field `{n}`", td.name), *sp)),
                            },
                            TypeKind::Union(_) => self.err(Diag::new(format!("`{}` is a union; `with` works on records", td.name), e.span)),
                        }
                    }
                }
                match b {
                    Ty::Obj(_) | Ty::Never => b,
                    _ => {
                        self.err(Diag::new("`with` needs a value whose record type is known here", base.span)
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
                        ExprKind::Lambda { .. } => self.lambda(&args[1], el),
                        ExprKind::Path(path) => match self.inf.p.resolve(self.module, path) {
                            Ok(Resolved::Fn(f)) => self.call_user(id(e), f, vec![el]),
                            _ => Ty::Dyn,
                        },
                        _ => Ty::Dyn,
                    };
                    return match (a, r) {
                        (_, Ty::Never) => Ty::Never,
                        (Ty::Arr(_), r) => Ty::Arr(r.el()),
                        (_, r) => r, // `¨` on a scalar applies f once
                    };
                }
                let b = if args.len() > 1 { self.expr(&args[1]) } else { Ty::Unit };
                if a == Ty::Never || b == Ty::Never {
                    return Ty::Never;
                }
                prim_ty(*prim, a, b)
            }
            ExprKind::Lambda { .. } => Ty::Dyn,
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
            Ty::Dyn => Ty::Dyn, // looked up by name at run time
            Ty::Obj(t) => {
                let td = &p.types[t as usize];
                match &td.decl.kind {
                    TypeKind::Record(decl) => match decl.iter().find(|(d, _, _)| d == name) {
                        Some((_, ft, _)) => ann_in(p, td.module, ft).unwrap_or(Ty::Dyn),
                        None => {
                            self.err(Diag::new(format!("`{}` has no field `{name}`", td.name), span));
                            Ty::Dyn
                        }
                    },
                    TypeKind::Union(_) => {
                        self.err(Diag::new(format!("`{}` is a union: use `match` to get at its fields", td.name), span));
                        Ty::Dyn
                    }
                }
            }
            other => {
                self.err(Diag::new(format!("`.{name}`: a {} has no fields", show(p, other)), span));
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
                    self.err(Diag::new(format!("this pattern is a {lt}, but the value is a {t}"), e.span));
                }
            }
            Pattern::Variant { path, args, .. } => {
                let p = self.inf.p;
                let Ok(Resolved::Variant(tid, v)) = p.resolve(self.module, path) else { return };
                let td = &p.types[tid];
                if let Ty::Obj(st) = t
                    && st as usize != tid
                {
                    self.err(Diag::new(format!("this pattern is a `{}`, but the value is a `{}`", td.name, p.types[st as usize].name), pat.span()));
                }
                let fts: Vec<Ty> = td.variants()[v].fields.iter().map(|f| ann_in(p, td.module, f).unwrap_or(Ty::Dyn)).collect();
                for (a, ft) in args.iter().zip(fts) {
                    self.bind_pattern(a, ft);
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
    if matches!(op, Eq | Ne) && a == Ty::Arr(El::Char) && b == Ty::Arr(El::Char) {
        return Ty::Bool;
    }
    let scalar = |x: Ty, y: Ty| -> Ty {
        match op {
            Eq | Ne | Lt | Le | Gt | Ge | And | Or => Ty::Bool,
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
        assert!(t.errors.iter().any(|e| e.diag.msg.contains("argument `n` of `t::main::k`: expected i64, found f64")), "{:?}", t.errors.iter().map(|e| &e.diag.msg).collect::<Vec<_>>());
    }

    #[test]
    fn mutation_joins() {
        let p = prog("module main\nfn main() {\n  let mut x = 1\n  x = 2.5\n  print(x)\n}\n");
        let t = infer(&p, &default_roots(&p, false));
        let s = t.spec(&(fid(&p, "main"), vec![]));
        assert!(s.lets.values().any(|t| *t == Ty::Dyn));
    }
}
