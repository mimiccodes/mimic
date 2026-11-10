//! Semantic checking: module tree, path resolution, visibility, mutability,
//! arity, FFI safety. Powers `mimic check` and feeds codegen.

use mimic_syntax::ast::*;
use mimic_syntax::glyphs::Prim;
use mimic_syntax::{Diag, Span};
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;

pub type ModId = usize;
pub type FnId = usize;

/// One parsed source file belonging to a package.
pub struct SourceFile {
    pub path: PathBuf,
    /// Path shown to users / written to mod.mim (relative to the project root).
    pub display: String,
    pub src: String,
    pub ast: File,
    pub package: String,
}

pub struct Module {
    pub name: String,
    pub parent: Option<ModId>,
    pub package: String,
    pub is_pub: bool,
    pub children: BTreeMap<String, ModId>,
    pub fns: BTreeMap<String, FnId>,
    pub uses: BTreeMap<String, UseDecl>,
    pub file: Option<usize>,
    pub span: Span,
    pub full: Span,
}

pub enum FnKind {
    User(FnDecl),
    Extern { decl: ExternFn, trusted: bool },
}

pub struct Function {
    pub name: String,
    pub module: ModId,
    pub is_pub: bool,
    pub kind: FnKind,
    pub file: usize,
    pub span: Span,
    /// Linker symbol.
    pub symbol: String,
}

impl Function {
    pub fn arity(&self) -> usize {
        match &self.kind {
            FnKind::User(d) => d.params.len(),
            FnKind::Extern { decl, .. } => decl.params.len(),
        }
    }
    pub fn is_extern(&self) -> bool {
        matches!(self.kind, FnKind::Extern { .. })
    }
    /// Signature text, hashed into mod.mim for incremental builds.
    pub fn signature(&self) -> String {
        let (params, ret): (Vec<String>, Option<&Type>) = match &self.kind {
            FnKind::User(d) => (
                d.params.iter().map(|p| match &p.ty {
                    Some(t) => format!("{}: {t}", p.name),
                    None => p.name.clone(),
                }).collect(),
                d.ret.as_ref(),
            ),
            FnKind::Extern { decl, .. } => {
                (decl.params.iter().map(|(n, t)| format!("{n}: {t}")).collect(), decl.ret.as_ref())
            }
        };
        let mut s = format!("{}fn {}({})", if self.is_pub { "pub " } else { "" }, self.name, params.join(", "));
        if let Some(r) = ret {
            s += &format!(" -> {r}");
        }
        s
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Resolved {
    Fn(FnId),
    Mod(ModId),
}

/// A diagnostic with the index of the file it belongs to.
pub struct FileDiag {
    pub file: usize,
    pub diag: Diag,
}

pub struct Program {
    pub files: Vec<SourceFile>,
    pub mods: Vec<Module>,
    pub fns: Vec<Function>,
    /// package name → root module
    pub roots: BTreeMap<String, ModId>,
    pub root_package: String,
}

pub const C_TYPES: &[&str] =
    &["i8", "i16", "i32", "i64", "u8", "u16", "u32", "u64", "isize", "usize", "f32", "f64", "bool", "c_char", "cstr"];

fn mangle(segs: &[String]) -> String {
    format!("_mimic_{}", segs.join("__"))
}

impl Program {
    /// Build the module tree for all files. `root_package` is the project being built;
    /// other packages are its dependencies.
    /// Module-tree errors (duplicates, collisions) are returned alongside the program.
    pub fn build(files: Vec<SourceFile>, root_package: &str) -> (Program, Vec<FileDiag>) {
        let mut p = Program { files, mods: Vec::new(), fns: Vec::new(), roots: BTreeMap::new(), root_package: root_package.into() };
        let mut diags = Vec::new();
        for fi in 0..p.files.len() {
            let pkg = p.files[fi].package.clone();
            let root = match p.roots.get(&pkg) {
                Some(r) => *r,
                None => {
                    let id = p.new_mod(pkg.clone(), None, &pkg, true, None, Span::default(), Span::default());
                    p.roots.insert(pkg.clone(), id);
                    id
                }
            };
            let decl = p.files[fi].ast.module.clone();
            p.register_module(&decl, root, fi, true, &mut diags);
        }
        (p, diags)
    }

    #[allow(clippy::too_many_arguments)]
    fn new_mod(&mut self, name: String, parent: Option<ModId>, pkg: &str, is_pub: bool, file: Option<usize>, span: Span, full: Span) -> ModId {
        self.mods.push(Module {
            name,
            parent,
            package: pkg.into(),
            is_pub,
            children: BTreeMap::new(),
            fns: BTreeMap::new(),
            uses: BTreeMap::new(),
            file,
            span,
            full,
        });
        self.mods.len() - 1
    }

    pub fn mod_path(&self, m: ModId) -> Vec<String> {
        let mut segs = vec![self.mods[m].name.clone()];
        let mut cur = self.mods[m].parent;
        while let Some(c) = cur {
            segs.push(self.mods[c].name.clone());
            cur = self.mods[c].parent;
        }
        segs.reverse();
        segs
    }

    pub fn fn_path(&self, f: FnId) -> Vec<String> {
        let mut p = self.mod_path(self.fns[f].module);
        p.push(self.fns[f].name.clone());
        p
    }

    fn register_module(&mut self, decl: &ModuleDecl, parent: ModId, file: usize, is_file_root: bool, diags: &mut Vec<FileDiag>) {
        let pkg = self.mods[parent].package.clone();
        if self.mods[parent].children.contains_key(&decl.name) {
            let what = if is_file_root { "another file already declares" } else { "duplicate" };
            diags.push(FileDiag { file, diag: Diag::new(format!("{what} module `{}`", decl.name), decl.span) });
            return;
        }
        if self.mods[parent].fns.contains_key(&decl.name) {
            diags.push(FileDiag { file, diag: Diag::new(format!("module `{}` collides with a function of the same name", decl.name), decl.span) });
            return;
        }
        // `lib` is the library entry point and is always public.
        let is_pub = decl.is_pub || (is_file_root && decl.name == "lib");
        let id = self.new_mod(decl.name.clone(), Some(parent), &pkg, is_pub, Some(file), decl.span, decl.full);
        self.mods[parent].children.insert(decl.name.clone(), id);
        for item in &decl.items {
            match item {
                Item::Module(m) => self.register_module(m, id, file, false, diags),
                Item::Fn(f) => {
                    let is_main = f.name == "main" && decl.name == "main" && is_file_root;
                    self.register_fn(id, &f.name, f.is_pub || is_main, FnKind::User(f.clone()), file, f.span, diags);
                }
                Item::Extern(b) => {
                    for ef in &b.fns {
                        self.register_fn(id, &ef.name, ef.is_pub, FnKind::Extern { decl: ef.clone(), trusted: b.trusted }, file, ef.span, diags);
                    }
                }
                Item::Use(u) => {
                    let name = u.local_name().to_string();
                    if self.mods[id].uses.insert(name.clone(), u.clone()).is_some() {
                        diags.push(FileDiag { file, diag: Diag::new(format!("`{name}` is imported twice"), u.span) });
                    }
                }
            }
        }
        // a `use` must not collide with a local definition
        let m = &self.mods[id];
        for (name, u) in &m.uses {
            if m.fns.contains_key(name) || m.children.contains_key(name) {
                diags.push(FileDiag {
                    file,
                    diag: Diag::new(format!("`use` of `{name}` collides with a local definition"), u.span)
                        .note("rename it with `use path as other_name`"),
                });
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn register_fn(&mut self, m: ModId, name: &str, is_pub: bool, kind: FnKind, file: usize, span: Span, diags: &mut Vec<FileDiag>) {
        if self.mods[m].fns.contains_key(name) {
            diags.push(FileDiag { file, diag: Diag::new(format!("function `{name}` is defined twice in this module"), span) });
            return;
        }
        if self.mods[m].children.contains_key(name) {
            diags.push(FileDiag { file, diag: Diag::new(format!("function `{name}` collides with a module of the same name"), span) });
            return;
        }
        // strict: no shadowing of functions from enclosing modules
        let mut cur = self.mods[m].parent;
        while let Some(a) = cur {
            if self.mods[a].fns.contains_key(name) {
                diags.push(FileDiag {
                    file,
                    diag: Diag::new(format!("function `{name}` shadows `{}::{name}` from an enclosing module", self.mod_path(a).join("::")), span),
                });
            }
            cur = self.mods[a].parent;
        }
        let mut segs = self.mod_path(m);
        segs.push(name.to_string());
        let symbol = match &kind {
            FnKind::Extern { decl, .. } => decl.name.clone(),
            FnKind::User(_) => mangle(&segs),
        };
        self.fns.push(Function { name: name.into(), module: m, is_pub, kind, file, span, symbol });
        let id = self.fns.len() - 1;
        self.mods[m].fns.insert(name.into(), id);
    }

    fn is_within(&self, m: ModId, ancestor: ModId) -> bool {
        let mut cur = Some(m);
        while let Some(c) = cur {
            if c == ancestor {
                return true;
            }
            cur = self.mods[c].parent;
        }
        false
    }

    /// Rust-style privacy: private items are visible inside their parent module and its descendants.
    fn accessible(&self, from: ModId, r: Resolved) -> bool {
        let (parent, is_pub) = match r {
            Resolved::Fn(f) => (Some(self.fns[f].module), self.fns[f].is_pub),
            Resolved::Mod(m) => (self.mods[m].parent, self.mods[m].is_pub),
        };
        match parent {
            None => true,
            Some(p) => is_pub || self.is_within(from, p),
        }
    }

    fn member(&self, m: ModId, name: &str) -> Option<Resolved> {
        if let Some(f) = self.mods[m].fns.get(name) {
            return Some(Resolved::Fn(*f));
        }
        if let Some(c) = self.mods[m].children.get(name) {
            return Some(Resolved::Mod(*c));
        }
        // package roots expose the items of their `lib` module: `ai::get_mic` = `ai::lib::get_mic`
        if self.mods[m].parent.is_none()
            && let Some(lib) = self.mods[m].children.get("lib") {
                if let Some(f) = self.mods[*lib].fns.get(name) {
                    return Some(Resolved::Fn(*f));
                }
                if let Some(c) = self.mods[*lib].children.get(name) {
                    return Some(Resolved::Mod(*c));
                }
            }
        None
    }

    fn describe(&self, r: Resolved) -> String {
        match r {
            Resolved::Fn(f) => format!("function `{}`", self.fn_path(f).join("::")),
            Resolved::Mod(m) => format!("module `{}`", self.mod_path(m).join("::")),
        }
    }

    /// Resolve `path` as seen from module `from`.
    pub fn resolve(&self, from: ModId, path: &[String]) -> Result<Resolved, String> {
        self.resolve_depth(from, path, 0)
    }

    fn resolve_depth(&self, from: ModId, path: &[String], depth: usize) -> Result<Resolved, String> {
        if depth > 16 {
            return Err("`use` cycle detected".into());
        }
        let first = &path[0];
        let mut cur = match first.as_str() {
            "self" => Some(Resolved::Mod(from)),
            "super" => Some(Resolved::Mod(self.mods[from].parent.ok_or("`super` used at the package root")?)),
            _ => None,
        };
        if cur.is_none() {
            // lexical: this module, then enclosing modules
            let mut m = Some(from);
            while let Some(c) = m {
                if let Some(r) = self.mods[c].fns.get(first).map(|f| Resolved::Fn(*f)).or_else(|| self.mods[c].children.get(first).map(|x| Resolved::Mod(*x))) {
                    cur = Some(r);
                    break;
                }
                if let Some(u) = self.mods[c].uses.get(first) {
                    let r = self.resolve_depth(c, &u.path, depth + 1).map_err(|e| format!("in `use {}`: {e}", u.path.join("::")))?;
                    cur = Some(r);
                    break;
                }
                m = self.mods[c].parent;
            }
        }
        if cur.is_none()
            && let Some(r) = self.roots.get(first) {
                cur = Some(Resolved::Mod(*r));
            }
        let mut cur = cur.ok_or_else(|| format!("cannot find `{first}` in this scope"))?;
        for seg in &path[1..] {
            let Resolved::Mod(m) = cur else {
                return Err(format!("{} is not a module, so `{seg}` cannot be looked up in it", self.describe(cur)));
            };
            if seg == "super" {
                cur = Resolved::Mod(self.mods[m].parent.ok_or("`super` goes above the package root")?);
                continue;
            }
            let next = self.member(m, seg).ok_or_else(|| format!("cannot find `{seg}` in {}", self.describe(cur)))?;
            if !self.accessible(from, next) {
                return Err(format!("{} is private", self.describe(next)));
            }
            cur = next;
        }
        Ok(cur)
    }

    /// The module that encloses a file's top-level items.
    pub fn file_module(&self, file: usize) -> ModId {
        let pkg = &self.files[file].package;
        let root = self.roots[pkg];
        self.mods[root].children[&self.files[file].ast.module.name]
    }

    /// The program entry point `<root package>::main::main`, if any.
    pub fn entry(&self) -> Option<FnId> {
        let root = *self.roots.get(&self.root_package)?;
        let main = *self.mods[root].children.get("main")?;
        self.mods[main].fns.get("main").copied()
    }
}

// ---- body checking --------------------------------------------------------

pub struct Options {
    /// Require `<root>::main::main` (binaries).
    pub require_main: bool,
}

pub fn check(p: &Program, opts: &Options) -> Vec<FileDiag> {
    let mut diags = Vec::new();
    if opts.require_main {
        match p.entry() {
            None => {
                let file = p.files.iter().position(|f| f.package == p.root_package && f.ast.module.name == "main");
                let span = file.map(|f| p.files[f].ast.module.span).unwrap_or_default();
                diags.push(FileDiag {
                    file: file.unwrap_or(0),
                    diag: Diag::new("binary has no entry point: `module main` must define `fn main()`", span),
                });
            }
            Some(f) if p.fns[f].arity() != 0 => {
                diags.push(FileDiag { file: p.fns[f].file, diag: Diag::new("`fn main` takes no parameters", p.fns[f].span) });
            }
            _ => {}
        }
    }
    for (fid, f) in p.fns.iter().enumerate() {
        let _ = fid;
        match &f.kind {
            FnKind::User(d) => {
                let mut c = Checker { p, module: f.module, file: f.file, scopes: Vec::new(), lambda_floor: 0, unsafe_depth: 0, diags: &mut diags };
                c.check_fn(d);
            }
            FnKind::Extern { decl, .. } => {
                for (_, t) in &decl.params {
                    if let Err(e) = check_ctype(t, false) {
                        diags.push(FileDiag { file: f.file, diag: e });
                    }
                }
                if let Some(t) = &decl.ret
                    && let Err(e) = check_ctype(t, true) {
                        diags.push(FileDiag { file: f.file, diag: e });
                    }
            }
        }
    }
    // validate `use` targets
    for (mid, m) in p.mods.iter().enumerate() {
        for u in m.uses.values() {
            if let Err(e) = p.resolve(mid, &u.path) {
                diags.push(FileDiag { file: m.file.unwrap_or(0), diag: Diag::new(e, u.span) });
            }
        }
    }
    diags
}

fn check_ctype(t: &Type, is_ret: bool) -> Result<(), Diag> {
    match t {
        Type::Named(n, s) => {
            if C_TYPES.contains(&n.as_str()) { Ok(()) } else { Err(Diag::new(format!("`{n}` is not a C type; use one of {}", C_TYPES.join(", ")), *s)) }
        }
        Type::Ptr(inner, s) => {
            if is_ret {
                return Err(Diag::new("returning raw pointers from C is not supported yet", *s));
            }
            check_ctype(inner, false)
        }
        Type::Array(_, s) => Err(Diag::new("arrays cross the FFI boundary as `ptr[T]`", *s)),
    }
}

/// Runtime type-check code for a Mimic annotation (see runtime TY_* constants); `None` = unchecked.
pub fn type_code(t: &Type) -> Result<i32, Diag> {
    Ok(match t {
        Type::Named(n, s) => match n.as_str() {
            "i8" | "i16" | "i32" | "i64" | "u8" | "u16" | "u32" | "u64" | "isize" | "usize" => 1,
            "f32" | "f64" => 2,
            "bool" => 3,
            "c_char" | "char" => 4,
            "str" => 5,
            other => return Err(Diag::new(format!("unknown type `{other}`"), *s)),
        },
        Type::Array(inner, s) => match &**inner {
            Type::Named(n, _) if matches!(n.as_str(), "i8" | "i16" | "i32" | "i64" | "u8" | "u16" | "u32" | "u64" | "isize" | "usize") => 6,
            Type::Named(n, _) if matches!(n.as_str(), "f32" | "f64") => 7,
            Type::Named(n, _) if n == "bool" => 8,
            _ => return Err(Diag::new("only arrays of numbers or bools are supported in this version", *s)),
        },
        Type::Ptr(_, s) => return Err(Diag::new("`ptr[T]` is only allowed in `extern` declarations", *s)),
    })
}

struct Var {
    mutable: bool,
}

struct Checker<'a> {
    p: &'a Program,
    module: ModId,
    file: usize,
    scopes: Vec<HashMap<String, Var>>,
    /// scopes below this index belong to an enclosing function (lambda captures)
    lambda_floor: usize,
    unsafe_depth: usize,
    diags: &'a mut Vec<FileDiag>,
}

impl Checker<'_> {
    fn err(&mut self, d: Diag) {
        self.diags.push(FileDiag { file: self.file, diag: d });
    }

    fn lookup(&self, name: &str) -> Option<(usize, &Var)> {
        for (i, s) in self.scopes.iter().enumerate().rev() {
            if let Some(v) = s.get(name) {
                return Some((i, v));
            }
        }
        None
    }

    fn declare(&mut self, name: &str, mutable: bool, span: Span) {
        if self.lookup(name).is_some() {
            self.err(Diag::new(format!("`{name}` is already defined; Mimic does not allow shadowing"), span)
                .note("use `let mut` and reassign it, or pick a different name"));
            return;
        }
        if let Ok(r) = self.p.resolve(self.module, &[name.to_string()])
            && matches!(r, Resolved::Fn(_)) {
                self.err(Diag::new(format!("variable `{name}` collides with a function of the same name"), span));
                return;
            }
        self.scopes.last_mut().unwrap().insert(name.into(), Var { mutable });
    }

    fn check_fn(&mut self, d: &FnDecl) {
        self.scopes.push(HashMap::new());
        for prm in &d.params {
            if let Some(t) = &prm.ty
                && let Err(e) = type_code(t) {
                    self.err(e);
                }
            self.declare(&prm.name, false, prm.span);
        }
        if let Some(t) = &d.ret
            && let Err(e) = type_code(t) {
                self.err(e);
            }
        self.block(&d.body, false);
        self.scopes.pop();
    }

    fn block(&mut self, b: &Block, new_scope: bool) {
        if new_scope {
            self.scopes.push(HashMap::new());
        }
        for s in &b.stmts {
            match s {
                Stmt::Let { name, mutable, ty, value, span } => {
                    self.expr(value);
                    if let Some(t) = ty
                        && let Err(e) = type_code(t) {
                            self.err(e);
                        }
                    self.declare(name, *mutable, *span);
                }
                Stmt::Assign { name, index, value, span } => {
                    if let Some(i) = index {
                        self.expr(i);
                    }
                    self.expr(value);
                    match self.lookup(name) {
                        None => self.err(Diag::new(format!("cannot assign to undefined variable `{name}`"), *span)),
                        Some((lvl, _)) if lvl < self.lambda_floor => {
                            self.err(Diag::new(format!("lambdas cannot modify captured variable `{name}`"), *span))
                        }
                        Some((_, v)) if !v.mutable => self.err(
                            Diag::new(format!("cannot assign twice to immutable variable `{name}`"), *span)
                                .note(format!("declare it as `let mut {name}` (or `mut {name} ← …`)")),
                        ),
                        _ => {}
                    }
                }
                Stmt::Expr(e) => self.expr(e),
                Stmt::Return(e, _) => {
                    if let Some(e) = e {
                        self.expr(e)
                    }
                }
            }
        }
        if let Some(t) = &b.tail {
            self.expr(t);
        }
        if new_scope {
            self.scopes.pop();
        }
    }

    fn fn_operand(&mut self, e: &Expr, want_arity: usize) {
        match &e.kind {
            ExprKind::Lambda { param, body } => self.lambda(param, body),
            ExprKind::Path(path) => {
                if path.len() == 1 && self.lookup(&path[0]).is_some() {
                    self.err(Diag::new(format!("`{}` is a variable, not a function", path[0]), e.span));
                    return;
                }
                match self.p.resolve(self.module, path) {
                    Ok(Resolved::Fn(f)) => {
                        let func = &self.p.fns[f];
                        if func.is_extern() {
                            self.err(Diag::new("extern functions cannot be passed as values; wrap them in a Mimic function", e.span));
                        } else if func.arity() != want_arity {
                            self.err(Diag::new(format!("`{}` takes {} argument(s), but `¨` passes {want_arity}", path.join("::"), func.arity()), e.span));
                        }
                    }
                    Ok(r) => self.err(Diag::new(format!("{} is not a function", self.p.describe(r)), e.span)),
                    Err(m) => self.err(Diag::new(m, e.span)),
                }
            }
            _ => self.err(Diag::new("expected a function or a lambda", e.span)),
        }
    }

    fn expr(&mut self, e: &Expr) {
        match &e.kind {
            ExprKind::Int(_) | ExprKind::Float(_) | ExprKind::Str(_) | ExprKind::Bool(_) => {}
            ExprKind::Array(xs) => xs.iter().for_each(|x| self.expr(x)),
            ExprKind::Path(path) => {
                if path.len() == 1
                    && let Some((lvl, _)) = self.lookup(&path[0]) {
                        if lvl < self.lambda_floor {
                            self.err(Diag::new(format!("lambdas cannot capture variables yet (`{}`)", path[0]), e.span)
                                .note("pass it as an argument or make it a top-level function"));
                        }
                        return;
                    }
                match self.p.resolve(self.module, path) {
                    Ok(r) => self.err(Diag::new(format!("{} used as a value", self.p.describe(r)), e.span)
                        .note("call functions with `f(x)` or a pipeline `x ▷ f`")),
                    Err(_) if path.len() == 1 => self.err(Diag::new(format!("cannot find variable `{}` in this scope", path[0]), e.span)),
                    Err(m) => self.err(Diag::new(m, e.span)),
                }
            }
            ExprKind::Call { callee, args } => {
                for a in args {
                    self.expr(a);
                }
                match &callee.kind {
                    ExprKind::Lambda { param, body } => {
                        if args.len() != 1 {
                            self.err(Diag::new("lambdas take exactly one argument", e.span));
                        }
                        self.lambda(param, body);
                    }
                    ExprKind::Path(path) => {
                        if path.len() == 1 && self.lookup(&path[0]).is_some() {
                            self.err(Diag::new(format!("`{}` is a variable, not a function", path[0]), callee.span));
                            return;
                        }
                        match self.p.resolve(self.module, path) {
                            Ok(Resolved::Fn(f)) => {
                                let func = &self.p.fns[f];
                                if func.arity() != args.len() {
                                    self.err(Diag::new(
                                        format!("`{}` takes {} argument(s) but {} were supplied", path.join("::"), func.arity(), args.len()),
                                        e.span,
                                    ));
                                }
                                if let FnKind::Extern { trusted: false, .. } = func.kind
                                    && self.unsafe_depth == 0 {
                                        self.err(Diag::new(format!("call to extern function `{}` requires an `unsafe {{ }}` block", path.join("::")), e.span)
                                            .note("or declare the block as `trusted extern \"C\"` if the C function is memory safe"));
                                    }
                            }
                            Ok(r) => self.err(Diag::new(format!("{} is not a function", self.p.describe(r)), callee.span)),
                            Err(m) => self.err(Diag::new(m, callee.span)),
                        }
                    }
                    _ => self.err(Diag::new("only named functions and lambdas can be called", callee.span)),
                }
            }
            ExprKind::Index { base, index } => {
                self.expr(base);
                self.expr(index);
            }
            ExprKind::Unary { expr, .. } => self.expr(expr),
            ExprKind::Binary { lhs, rhs, .. } => {
                self.expr(lhs);
                self.expr(rhs);
            }
            ExprKind::Prim { prim, args } => {
                if args.len() != prim.arity() {
                    self.err(Diag::new(format!("`{}` takes {} argument(s), got {}", prim.info().word, prim.arity(), args.len()), e.span));
                    return;
                }
                if *prim == Prim::Each {
                    self.expr(&args[0]);
                    self.fn_operand(&args[1], 1);
                } else {
                    args.iter().for_each(|a| self.expr(a));
                }
            }
            ExprKind::Lambda { .. } => self.err(
                Diag::new("lambdas are not first-class values yet", e.span)
                    .note("call them in a pipeline `x ▷ (y => …)` or pass them to `¨` (each)"),
            ),
            ExprKind::If { cond, then, els } => {
                self.expr(cond);
                self.block(then, true);
                if let Some(e) = els {
                    self.expr(e);
                }
            }
            ExprKind::Block(b) => self.block(b, true),
            ExprKind::Unsafe(b) => {
                self.unsafe_depth += 1;
                self.block(b, true);
                self.unsafe_depth -= 1;
            }
        }
    }

    fn lambda(&mut self, param: &str, body: &Expr) {
        {
            {
                let saved_floor = self.lambda_floor;
                self.lambda_floor = self.scopes.len();
                self.scopes.push(HashMap::new());
                // lambda params may reuse outer names: the outer scope is invisible inside
                self.scopes.last_mut().unwrap().insert(param.to_string(), Var { mutable: false });
                self.expr(body);
                self.scopes.pop();
                self.lambda_floor = saved_floor;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mimic_syntax::parser::parse_file;

    fn prog(files: &[(&str, &str, &str)]) -> (Program, Vec<FileDiag>) {
        let files = files
            .iter()
            .map(|(pkg, path, src)| SourceFile {
                path: path.into(),
                display: path.to_string(),
                src: src.to_string(),
                ast: parse_file(src).unwrap_or_else(|d| panic!("{path}: {}", d.msg)),
                package: pkg.to_string(),
            })
            .collect();
        Program::build(files, "app")
    }

    fn errors(files: &[(&str, &str, &str)]) -> Vec<String> {
        let (p, d) = prog(files);
        if !d.is_empty() {
            return d.into_iter().map(|d| d.diag.msg).collect();
        }
        check(&p, &Options { require_main: true }).into_iter().map(|d| d.diag.msg).collect()
    }

    #[test]
    fn ok_program() {
        let e = errors(&[
            ("app", "src/main.mim", "module main\nfn main() {\n  let xs = ⍳ 10\n  print(app::speech::get_mic(xs))\n  print(speech::get_mic(xs))\n  print(dep::hello())\n}\n"),
            ("app", "src/speech.mim", "module speech\npub fn get_mic(x) { x ▷ +/ }\n"),
            ("dep", "lib.mim", "module lib\npub fn hello() { 1 }\n"),
        ]);
        assert!(e.is_empty(), "{e:?}");
    }

    #[test]
    fn privacy_across_packages_and_modules() {
        let e = errors(&[
            ("app", "src/main.mim", "module main\nfn main() {\n  dep::secret()\n  inner::hidden()\n}\nmodule inner {\n  fn hidden() { 1 }\n}\n"),
            ("dep", "lib.mim", "module lib\nfn secret() { 1 }\n"),
        ]);
        assert_eq!(e.len(), 2, "{e:?}");
        assert!(e.iter().all(|m| m.contains("private")), "{e:?}");
    }

    #[test]
    fn mutability_shadowing_and_unsafe() {
        let e = errors(&[(
            "app",
            "src/main.mim",
            "module main\nextern \"C\" {\n  fn abs(x: i32) -> i32\n}\nfn main() {\n  let x = 1\n  x = 2\n  let x = 3\n  mut y ← 1\n  y ↩ y + 1\n  abs(1)\n  unsafe { abs(1) }\n}\n",
        )]);
        assert_eq!(e.len(), 3, "{e:?}");
        assert!(e[0].contains("immutable"));
        assert!(e[1].contains("shadowing"));
        assert!(e[2].contains("unsafe"));
    }

    #[test]
    fn lambdas_cannot_capture() {
        let e = errors(&[("app", "src/main.mim", "module main\nfn main() {\n  let k = 2\n  print(⍳ 3 ¨ x => x × k)\n  print(⍳ 3 ¨ x => x × 2)\n}\n")]);
        assert_eq!(e.len(), 1, "{e:?}");
        assert!(e[0].contains("capture"));
    }

    #[test]
    fn missing_main() {
        let e = errors(&[("app", "src/main.mim", "module main\nfn start() { 1 }\n")]);
        assert!(e[0].contains("entry point"));
    }
}
