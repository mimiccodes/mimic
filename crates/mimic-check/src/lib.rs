//! Semantic checking: module tree, path resolution, visibility, mutability,
//! arity, FFI safety. Powers `mimic check` and feeds codegen.

// Diagnostics are rich (labels, fixes) and only built on the error path.
#![allow(clippy::result_large_err)]

use mimic_syntax::ast::*;
use mimic_syntax::glyphs::Prim;
use mimic_syntax::codes as C;
use mimic_syntax::{Applicability, Diag, Edit, Span};
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;

pub mod types;

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
    pub types: BTreeMap<String, TypeId>,
    /// union variant constructors: name → (type, variant index)
    pub variants: BTreeMap<String, (TypeId, usize)>,
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
    Type(TypeId),
    Variant(TypeId, usize),
}

pub type TypeId = usize;

/// A user-declared record or union type.
pub struct TypeDef {
    pub name: String,
    pub module: ModId,
    pub is_pub: bool,
    pub decl: TypeDecl,
    pub file: usize,
}

impl TypeDef {
    pub fn is_record(&self) -> bool {
        matches!(self.decl.kind, TypeKind::Record(_))
    }
    /// Record field names, in declaration order.
    pub fn field_index(&self, name: &str) -> Option<usize> {
        match &self.decl.kind {
            TypeKind::Record(fs) => fs.iter().position(|(n, _, _)| n == name),
            TypeKind::Union(_) => None,
        }
    }
    pub fn variants(&self) -> &[Variant] {
        match &self.decl.kind {
            TypeKind::Union(v) => v,
            TypeKind::Record(_) => &[],
        }
    }
}

/// A diagnostic with the index of the file it belongs to.
pub struct FileDiag {
    pub file: usize,
    pub diag: Diag,
}

/// Why a path did not resolve; turned into a [`Diag`] with [`ResolveError::diag`].
#[derive(Clone, Debug)]
pub struct ResolveError {
    pub code: &'static mimic_syntax::Code,
    pub msg: String,
    /// The whole path with the misspelled segment corrected.
    pub suggestion: Option<String>,
    /// The item exists but is private.
    pub private: Option<Resolved>,
}

impl std::fmt::Display for ResolveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.msg)
    }
}

impl From<ResolveError> for String {
    fn from(e: ResolveError) -> String {
        e.msg
    }
}

impl ResolveError {
    /// A diagnostic at `span` in `file` (the text of the path being resolved),
    /// with a "did you mean" fix or a fix adding `pub` to the definition.
    pub fn diag(&self, p: &Program, file: usize, span: Span) -> Diag {
        let mut d = Diag::new(self.code, self.msg.clone(), span);
        if let Some(fixed) = &self.suggestion {
            // only offer the edit when the span is exactly the path's text
            let text = p.files.get(file).and_then(|f| f.src.get(span.start..span.end)).unwrap_or("");
            let first_seg = fixed.split("::").count();
            if text.split("::").count() == first_seg && !text.contains(['(', ' ', '{']) {
                d = d.fix(format!("did you mean `{fixed}`?"), span, fixed.clone(), Applicability::MaybeIncorrect);
            } else {
                d = d.help(format!("did you mean `{fixed}`?"));
            }
        }
        if let Some(r) = self.private
            && let Some((df, ds)) = p.decl_site(r)
        {
            d = d.label_in(df, ds, "declared here without `pub`");
            let src = &p.files[df].src;
            if ["fn ", "type ", "module "].iter().any(|k| src.get(ds.start..).is_some_and(|t| t.starts_with(k))) {
                let at = Span { start: ds.start, end: ds.start, ..ds };
                d = d.fix_edits(
                    "make it public",
                    vec![Edit { file: Some(df), span: at, text: "pub ".into() }],
                    Applicability::MachineApplicable,
                );
            }
        }
        d
    }
}

pub struct Program {
    pub files: Vec<SourceFile>,
    pub mods: Vec<Module>,
    pub fns: Vec<Function>,
    pub types: Vec<TypeDef>,
    /// package name → root module
    pub roots: BTreeMap<String, ModId>,
    pub root_package: String,
    /// C files to compile and link with the program (wrappers generated by
    /// `extern "C" include` for structs passed by value).
    pub c_sources: Vec<PathBuf>,
}

pub const C_TYPES: &[&str] =
    &["i8", "i16", "i32", "i64", "u8", "u16", "u32", "u64", "isize", "usize", "f32", "f64", "bool", "c_char", "cstr", "cptr", "fnref"];

fn mangle(segs: &[String]) -> String {
    format!("_mimic_{}", segs.join("__"))
}

impl Program {
    /// Build the module tree for all files. `root_package` is the project being built;
    /// other packages are its dependencies.
    /// Module-tree errors (duplicates, collisions) are returned alongside the program.
    pub fn build(files: Vec<SourceFile>, root_package: &str) -> (Program, Vec<FileDiag>) {
        let mut p = Program { files, mods: Vec::new(), fns: Vec::new(), types: Vec::new(), roots: BTreeMap::new(), root_package: root_package.into(), c_sources: Vec::new() };
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
            types: BTreeMap::new(),
            variants: BTreeMap::new(),
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
            diags.push(FileDiag { file, diag: Diag::new(&C::DUPLICATE_DEFINITION, format!("{what} module `{}`", decl.name), decl.span) });
            return;
        }
        if self.mods[parent].fns.contains_key(&decl.name) {
            diags.push(FileDiag { file, diag: Diag::new(&C::DUPLICATE_DEFINITION, format!("module `{}` collides with a function of the same name", decl.name), decl.span) });
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
                Item::Type(t) => self.register_type(id, t, file, diags),
                Item::Use(u) => {
                    let name = u.local_name().to_string();
                    if self.mods[id].uses.insert(name.clone(), u.clone()).is_some() {
                        diags.push(FileDiag { file, diag: Diag::new(&C::DUPLICATE_DEFINITION, format!("`{name}` is imported twice"), u.span) });
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
                    diag: Diag::new(&C::DUPLICATE_DEFINITION, format!("`use` of `{name}` collides with a local definition"), u.span)
                        .note("rename it with `use path as other_name`"),
                });
            }
        }
    }

    fn register_type(&mut self, m: ModId, t: &TypeDecl, file: usize, diags: &mut Vec<FileDiag>) {
        let taken = |s: &Self, n: &str| s.mods[m].fns.contains_key(n) || s.mods[m].children.contains_key(n) || s.mods[m].types.contains_key(n) || s.mods[m].variants.contains_key(n);
        if taken(self, &t.name) {
            diags.push(FileDiag { file, diag: Diag::new(&C::DUPLICATE_DEFINITION, format!("`{}` is already defined in this module", t.name), t.span) });
            return;
        }
        self.types.push(TypeDef { name: t.name.clone(), module: m, is_pub: t.is_pub, decl: t.clone(), file });
        let id = self.types.len() - 1;
        self.mods[m].types.insert(t.name.clone(), id);
        if let TypeKind::Union(vs) = &t.kind {
            for (i, v) in vs.iter().enumerate() {
                if taken(self, &v.name) {
                    diags.push(FileDiag { file, diag: Diag::new(&C::DUPLICATE_DEFINITION, format!("variant `{}` collides with another name in this module", v.name), v.span) });
                    continue;
                }
                self.mods[m].variants.insert(v.name.clone(), (id, i));
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn register_fn(&mut self, m: ModId, name: &str, is_pub: bool, kind: FnKind, file: usize, span: Span, diags: &mut Vec<FileDiag>) {
        if self.mods[m].fns.contains_key(name) {
            diags.push(FileDiag { file, diag: Diag::new(&C::DUPLICATE_DEFINITION, format!("function `{name}` is defined twice in this module"), span) });
            return;
        }
        if self.mods[m].children.contains_key(name) {
            diags.push(FileDiag { file, diag: Diag::new(&C::DUPLICATE_DEFINITION, format!("function `{name}` collides with a module of the same name"), span) });
            return;
        }
        if self.mods[m].types.contains_key(name) || self.mods[m].variants.contains_key(name) {
            diags.push(FileDiag { file, diag: Diag::new(&C::DUPLICATE_DEFINITION, format!("function `{name}` collides with a type or variant of the same name"), span) });
            return;
        }
        // strict: no shadowing of functions from enclosing modules
        let mut cur = self.mods[m].parent;
        while let Some(a) = cur {
            if self.mods[a].fns.contains_key(name) {
                diags.push(FileDiag {
                    file,
                    diag: Diag::new(&C::SHADOWING, format!("function `{name}` shadows `{}::{name}` from an enclosing module", self.mod_path(a).join("::")), span),
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
            Resolved::Type(t) | Resolved::Variant(t, _) => (Some(self.types[t].module), self.types[t].is_pub),
        };
        match parent {
            None => true,
            Some(p) => is_pub || self.is_within(from, p),
        }
    }

    /// A name defined directly in module `m`.
    fn local(&self, m: ModId, name: &str) -> Option<Resolved> {
        let md = &self.mods[m];
        md.fns
            .get(name)
            .map(|f| Resolved::Fn(*f))
            .or_else(|| md.children.get(name).map(|x| Resolved::Mod(*x)))
            .or_else(|| md.types.get(name).map(|t| Resolved::Type(*t)))
            .or_else(|| md.variants.get(name).map(|(t, v)| Resolved::Variant(*t, *v)))
    }

    fn member(&self, m: ModId, name: &str) -> Option<Resolved> {
        if let Some(r) = self.local(m, name) {
            return Some(r);
        }
        // package roots expose the items of their `lib` module: `ai::get_mic` = `ai::lib::get_mic`
        if self.mods[m].parent.is_none()
            && let Some(lib) = self.mods[m].children.get("lib")
        {
            return self.local(*lib, name);
        }
        None
    }

    fn describe(&self, r: Resolved) -> String {
        match r {
            Resolved::Fn(f) => format!("function `{}`", self.fn_path(f).join("::")),
            Resolved::Mod(m) => format!("module `{}`", self.mod_path(m).join("::")),
            Resolved::Type(t) => format!("type `{}`", self.types[t].name),
            Resolved::Variant(t, v) => format!("variant `{}::{}`", self.types[t].name, self.types[t].variants()[v].name),
        }
    }

    /// Resolve `path` as seen from module `from`.
    pub fn resolve(&self, from: ModId, path: &[String]) -> Result<Resolved, ResolveError> {
        self.resolve_depth(from, path, 0)
    }

    /// Every name visible unqualified from `from` (for suggestions).
    pub fn visible_names(&self, from: ModId) -> Vec<String> {
        let mut out = Vec::new();
        let mut m = Some(from);
        while let Some(c) = m {
            out.extend(self.member_names(c, false));
            out.extend(self.mods[c].uses.keys().cloned());
            m = self.mods[c].parent;
        }
        out.extend(self.roots.keys().filter(|r| r.as_str() != "std").cloned());
        if let Some(std) = self.roots.get("std")
            && let Some(pre) = self.mods[*std].children.get("prelude")
        {
            out.extend(self.member_names(*pre, false));
        }
        out
    }

    fn member_names(&self, m: ModId, through_lib: bool) -> Vec<String> {
        let md = &self.mods[m];
        let mut v: Vec<String> =
            md.fns.keys().chain(md.children.keys()).chain(md.types.keys()).chain(md.variants.keys()).cloned().collect();
        if through_lib
            && md.parent.is_none()
            && let Some(lib) = md.children.get("lib")
        {
            v.extend(self.member_names(*lib, false));
        }
        v
    }

    fn resolve_depth(&self, from: ModId, path: &[String], depth: usize) -> Result<Resolved, ResolveError> {
        let plain = |code: &'static mimic_syntax::Code, msg: String| ResolveError { code, msg, suggestion: None, private: None };
        if depth > 16 {
            return Err(plain(&C::BAD_PATH, "`use` cycle detected".into()));
        }
        let first = &path[0];
        let mut cur = match first.as_str() {
            "self" => Some(Resolved::Mod(from)),
            "super" => Some(Resolved::Mod(
                self.mods[from].parent.ok_or_else(|| plain(&C::BAD_PATH, "`super` used at the package root".into()))?,
            )),
            _ => None,
        };
        if cur.is_none() {
            // lexical: this module, then enclosing modules
            let mut m = Some(from);
            while let Some(c) = m {
                if let Some(r) = self.local(c, first) {
                    cur = Some(r);
                    break;
                }
                if let Some(u) = self.mods[c].uses.get(first) {
                    let r = self.resolve_depth(c, &u.path, depth + 1).map_err(|mut e| {
                        e.msg = format!("in `use {}`: {}", u.path.join("::"), e.msg);
                        e.suggestion = None;
                        e
                    })?;
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
        // the standard prelude (Option, Result, …) is visible everywhere
        if cur.is_none()
            && let Some(std) = self.roots.get("std")
            && let Some(pre) = self.mods[*std].children.get("prelude")
        {
            cur = self.local(*pre, first);
        }
        let Some(mut cur) = cur else {
            let names = self.visible_names(from);
            let suggestion = mimic_syntax::closest(first, names.iter().map(String::as_str)).map(|best| {
                let mut fixed = path.to_vec();
                fixed[0] = best.to_string();
                fixed.join("::")
            });
            return Err(ResolveError {
                code: &C::UNRESOLVED_NAME,
                msg: format!("cannot find `{first}` in this scope"),
                suggestion,
                private: None,
            });
        };
        for (i, seg) in path.iter().enumerate().skip(1) {
            let Resolved::Mod(m) = cur else {
                return Err(plain(&C::WRONG_KIND, format!("{} is not a module, so `{seg}` cannot be looked up in it", self.describe(cur))));
            };
            if seg == "super" {
                cur = Resolved::Mod(self.mods[m].parent.ok_or_else(|| plain(&C::BAD_PATH, "`super` goes above the package root".into()))?);
                continue;
            }
            let Some(next) = self.member(m, seg) else {
                let names: Vec<String> =
                    self.member_names(m, true).into_iter().filter(|n| self.member(m, n).is_some_and(|r| self.accessible(from, r))).collect();
                let suggestion = mimic_syntax::closest(seg, names.iter().map(String::as_str)).map(|best| {
                    let mut fixed = path.to_vec();
                    fixed[i] = best.to_string();
                    fixed.join("::")
                });
                return Err(ResolveError {
                    code: &C::UNRESOLVED_NAME,
                    msg: format!("cannot find `{seg}` in {}", self.describe(cur)),
                    suggestion,
                    private: None,
                });
            };
            if !self.accessible(from, next) {
                return Err(ResolveError {
                    code: &C::PRIVATE_ITEM,
                    msg: format!("{} is private", self.describe(next)),
                    suggestion: None,
                    private: Some(next),
                });
            }
            cur = next;
        }
        Ok(cur)
    }

    /// Where an item is declared: (file, span of its first keyword).
    pub fn decl_site(&self, r: Resolved) -> Option<(usize, Span)> {
        match r {
            Resolved::Fn(f) => Some((self.fns[f].file, self.fns[f].span)),
            Resolved::Mod(m) => self.mods[m].file.map(|f| (f, self.mods[m].span)),
            Resolved::Type(t) | Resolved::Variant(t, _) => Some((self.types[t].file, self.types[t].decl.span)),
        }
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
                    diag: Diag::new(&C::NO_MAIN, "binary has no entry point: `module main` must define `fn main()`", span),
                });
            }
            Some(f) if p.fns[f].arity() != 0 => {
                diags.push(FileDiag { file: p.fns[f].file, diag: Diag::new(&C::NO_MAIN, "`fn main` takes no parameters", p.fns[f].span) });
            }
            _ => {}
        }
    }
    for (fid, f) in p.fns.iter().enumerate() {
        let _ = fid;
        match &f.kind {
            FnKind::User(d) => {
                // `extern "C" fn`: its signature is what C sees
                if d.export_c {
                    for prm in &d.params {
                        match &prm.ty {
                            None => diags.push(FileDiag {
                                file: f.file,
                                diag: Diag::new(&C::NOT_C_TYPE, format!("parameter `{}` of a function exported to C needs a C type, e.g. `{}: f64`", prm.name, prm.name), prm.span),
                            }),
                            Some(Type::Ptr(_, s)) => diags.push(FileDiag {
                                file: f.file,
                                diag: Diag::new(&C::FFI_UNSUPPORTED, "arrays from C (`ptr[T]`) cannot be received by exported functions yet; pass a cptr", *s),
                            }),
                            Some(t) => {
                                if let Err(e) = check_ctype(t, false) {
                                    diags.push(FileDiag { file: f.file, diag: e });
                                }
                            }
                        }
                    }
                    if let Some(t) = &d.ret
                        && let Err(e) = check_ctype(t, true)
                    {
                        diags.push(FileDiag { file: f.file, diag: e });
                    }
                }
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
                let file = m.file.unwrap_or(0);
                diags.push(FileDiag { file, diag: e.diag(p, file, u.span) });
            }
        }
    }
    diags
}

fn check_ctype(t: &Type, is_ret: bool) -> Result<(), Diag> {
    match t {
        Type::Named(n, s) => {
            if C_TYPES.contains(&n.as_str()) { Ok(()) } else { Err(Diag::new(&C::NOT_C_TYPE, format!("`{n}` is not a C type; use one of {}", C_TYPES.join(", ")), *s)) }
        }
        Type::Ptr(inner, s) => {
            if is_ret {
                return Err(Diag::new(&C::FFI_UNSUPPORTED, "returning raw pointers from C is not supported yet", *s));
            }
            check_ctype(inner, false)
        }
        Type::Array(_, s) => Err(Diag::new(&C::NOT_C_TYPE, "arrays cross the FFI boundary as `ptr[T]`", *s)),
        Type::Fn(_, _, s) => Err(Diag::new(&C::FFI_UNSUPPORTED, "C callbacks (function pointers) are not supported yet", *s)),
    }
}

/// Runtime type-check code for a Mimic annotation (see runtime TY_* constants); `None` = unchecked.
pub fn type_code(t: &Type) -> Result<i32, Diag> {
    Ok(match t {
        Type::Named(n, s) => match n.as_str() {
            "i8" | "i16" | "i32" | "i64" | "u8" | "u16" | "u32" | "u64" | "isize" | "usize" | "cptr" => 1,
            "f32" | "f64" => 2,
            "bool" => 3,
            "c_char" | "char" => 4,
            "str" | "cstr" => 5,
            other => return Err(Diag::new(&C::UNKNOWN_TYPE, format!("unknown type `{other}`"), *s)),
        },
        Type::Array(inner, s) => match &**inner {
            Type::Named(n, _) if matches!(n.as_str(), "i8" | "i16" | "i32" | "i64" | "u8" | "u16" | "u32" | "u64" | "isize" | "usize" | "cptr") => 6,
            Type::Named(n, _) if matches!(n.as_str(), "f32" | "f64") => 7,
            Type::Named(n, _) if n == "bool" => 8,
            _ => return Err(Diag::new(&C::UNSUPPORTED_TYPE, "only arrays of numbers or bools are supported in this version", *s)),
        },
        Type::Ptr(_, s) => return Err(Diag::new(&C::PTR_OUTSIDE_EXTERN, "`ptr[T]` is only allowed in `extern` declarations", *s)),
        Type::Fn(..) => 9, // TY_FN: any function value
    })
}

struct Var {
    mutable: bool,
    /// where it was declared (`let …`, a parameter, a pattern binding)
    span: Span,
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
            self.err(Diag::new(&C::SHADOWING, format!("`{name}` is already defined; Mimic does not allow shadowing"), span)
                .note("use `let mut` and reassign it, or pick a different name"));
            return;
        }
        if let Ok(r) = self.p.resolve(self.module, &[name.to_string()])
            && matches!(r, Resolved::Fn(_)) {
                self.err(Diag::new(&C::DUPLICATE_DEFINITION, format!("variable `{name}` collides with a function of the same name"), span));
                return;
            }
        self.scopes.last_mut().unwrap().insert(name.into(), Var { mutable, span });
    }

    /// Annotations: built-in types, or record/union types in scope.
    fn ann_result(&self, t: &Type) -> Result<(), Diag> {
        if type_code(t).is_ok() {
            return Ok(());
        }
        match t {
            Type::Named(n, s) => match self.p.resolve(self.module, std::slice::from_ref(n)) {
                Ok(Resolved::Type(_)) => Ok(()),
                _ if n == "str" || n == "any" => Ok(()),
                _ => {
                    let mut names: Vec<String> = BUILTIN_TYPES.iter().map(|t| t.to_string()).collect();
                    names.extend(self.p.visible_names(self.module).into_iter().filter(|x| x.starts_with(|c: char| c.is_ascii_uppercase())));
                    Err(Diag::new(&C::UNKNOWN_TYPE, format!("unknown type `{n}`"), *s).did_you_mean(n, *s, names.iter().map(String::as_str)))
                }
            },
            Type::Array(inner, _) => self.ann_result(inner),
            Type::Ptr(_, s) => Err(Diag::new(&C::PTR_OUTSIDE_EXTERN, "`ptr[T]` is only allowed in `extern` declarations", *s)),
            Type::Fn(ps, r, _) => {
                for p in ps {
                    self.ann_result(p)?;
                }
                match r {
                    Some(r) => self.ann_result(r),
                    None => Ok(()),
                }
            }
        }
    }

    fn check_ann(&mut self, t: &Type) {
        if let Err(e) = self.ann_result(t) {
            self.err(e);
        }
    }

    /// Declare the bindings of a pattern and track which variants it covers.
    fn pattern(&mut self, pat: &Pattern, top: bool, union: &mut Option<TypeId>, covered: &mut Vec<usize>, catch_all: &mut bool) {
        match pat {
            Pattern::Wild(_) => *catch_all |= top,
            Pattern::Bind(n, s) => {
                self.declare(n, false, *s);
                *catch_all |= top;
            }
            Pattern::Lit(e) => self.expr(e),
            Pattern::Tuple(ps, _) => {
                if top && ps.iter().all(|a| matches!(a, Pattern::Wild(_) | Pattern::Bind(..))) {
                    *catch_all = true;
                }
                let (mut u2, mut c2, mut a2) = (None, Vec::new(), false);
                for a in ps {
                    self.pattern(a, false, &mut u2, &mut c2, &mut a2);
                }
            }
            Pattern::Route(parts, span) => {
                for p in parts {
                    if let RoutePart::Capture { name, .. } = p {
                        self.declare(name, false, *span);
                    }
                }
            }
            Pattern::Variant { path, args, span } => match self.p.resolve(self.module, path) {
                Ok(Resolved::Variant(t, v)) => {
                    let want = self.p.types[t].variants()[v].fields.len();
                    if args.len() != want {
                        self.err(Diag::new(&C::PATTERN_ARITY, format!("`{}` has {want} field(s), but the pattern has {}", path.join("::"), args.len()), *span));
                    }
                    if top {
                        match union {
                            Some(u) if *u != t => self.err(Diag::new(&C::MIXED_ARMS, format!("this arm matches `{}`, but earlier arms match `{}`", self.p.types[t].name, self.p.types[*u].name), *span)),
                            _ => *union = Some(t),
                        }
                        if args.iter().all(|a| matches!(a, Pattern::Wild(_) | Pattern::Bind(..))) {
                            covered.push(v);
                        }
                    }
                    let (mut u2, mut c2, mut a2) = (None, Vec::new(), false);
                    for a in args {
                        self.pattern(a, false, &mut u2, &mut c2, &mut a2);
                    }
                }
                Ok(r) => self.err(Diag::new(&C::WRONG_KIND, format!("{} is not a variant", self.p.describe(r)), *span)),
                Err(m) => self.err(m.diag(self.p, self.file, *span)),
            },
        }
    }

    fn check_fn(&mut self, d: &FnDecl) {
        self.scopes.push(HashMap::new());
        for prm in &d.params {
            if let Some(t) = &prm.ty {
                self.check_ann(t);
            }
            if let Some(d) = &prm.default
                && !is_constant(self.p, self.module, d)
            {
                self.err(Diag::new(&C::DEFAULT_NOT_CONST, format!("the default value of `{}` must be a constant", prm.name), d.span)
                    .note("literals, operators on literals, arrays of them, and unit variants"));
            }
            self.declare(&prm.name, false, prm.span);
        }
        if let Some(t) = &d.ret {
            self.check_ann(t);
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
                        && let Err(e) = self.ann_result(t) {
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
                        None => self.err(Diag::new(&C::ASSIGN_UNDEFINED, format!("cannot assign to undefined variable `{name}`"), *span)),
                        Some((lvl, _)) if lvl < self.lambda_floor => {
                            self.err(Diag::new(&C::CAPTURE_MUTATION, format!("lambdas cannot modify captured variable `{name}`"), *span))
                        }
                        Some((_, v)) if !v.mutable => {
                            let decl = v.span;
                            let src = &self.p.files[self.file].src;
                            let is_let = src.get(decl.start..).is_some_and(|t| t.starts_with("let "));
                            let shown = if is_let { Span { end: decl.start + 4 + name.len(), ..decl } } else { decl };
                            let mut d = Diag::new(&C::ASSIGN_IMMUTABLE, format!("cannot assign twice to immutable variable `{name}`"), *span)
                                .label(shown, "first bound here");
                            if is_let {
                                let at = Span { start: decl.start + 4, end: decl.start + 4, line: decl.line, col: decl.col + 4 };
                                d = d.fix(format!("make `{name}` mutable"), at, "mut ", Applicability::MachineApplicable);
                            } else {
                                d = d.note(format!("declare it as `let mut {name}` (or `mut {name} ← …`)"));
                            }
                            self.err(d)
                        }
                        _ => {}
                    }
                }
                Stmt::Expr(e) => self.expr(e),
                Stmt::Return(e, _) => {
                    if let Some(e) = e {
                        self.expr(e)
                    }
                }
                Stmt::LetTuple { names, value, span } => {
                    self.expr(value);
                    for n in names {
                        if n != "_" {
                            self.declare(n, false, *span);
                        }
                    }
                }
                Stmt::LetElse { name, ty, value, els, span } => {
                    self.expr(value);
                    if let Some(t) = ty {
                        self.check_ann(t);
                    }
                    self.block(els, true);
                    if els.tail.is_some() || !matches!(els.stmts.last(), Some(Stmt::Return(..))) {
                        self.err(Diag::new(&C::ELSE_MUST_RETURN, "the `else` block of `let … else` must end with `return`", els.span)
                            .note("it runs when the value is `None` or `Err`, so the binding has no value"));
                    }
                    self.declare(name, false, *span);
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
        self.fn_operand_named(e, want_arity, "¨")
    }

    /// A function operand of a higher-order primitive (`¨`, `gather`, …).
    fn fn_operand_named(&mut self, e: &Expr, want_arity: usize, who: &str) {
        let who = if who == "¨" { "`¨`".to_string() } else { format!("`{who}`") };
        match &e.kind {
            ExprKind::Lambda { params, body } => {
                if params.len() != want_arity {
                    self.err(Diag::new(&C::ARG_COUNT, format!("this lambda takes {} argument(s), but {who} passes {want_arity}", params.len()), e.span));
                }
                self.lambda(params, body)
            }
            ExprKind::OpRef(_) if want_arity != 2 => {
                self.err(Diag::new(&C::ARG_COUNT, format!("an operator takes 2 arguments, but {who} passes {want_arity}"), e.span))
            }
            ExprKind::OpRef(_) => {}
            // a function value held in a variable: checked when called
            ExprKind::Path(path) if path.len() == 1 && self.lookup(&path[0]).is_some() => self.expr(e),
            ExprKind::Path(path) => match self.p.resolve(self.module, path) {
                Ok(Resolved::Fn(f)) => {
                    let func = &self.p.fns[f];
                    if func.is_extern() {
                        self.err(Diag::new(&C::FFI_UNSUPPORTED, "extern functions cannot be passed as values; wrap them in a Mimic function", e.span));
                    } else if func.arity() != want_arity {
                        self.err(Diag::new(&C::ARG_COUNT, format!("`{}` takes {} argument(s), but {who} passes {want_arity}", path.join("::"), func.arity()), e.span));
                    }
                }
                Ok(r) => self.err(Diag::new(&C::WRONG_KIND, format!("{} is not a function", self.p.describe(r)), e.span)),
                Err(m) => self.err(m.diag(self.p, self.file, e.span)),
            },
            // any other expression may produce a function value
            _ => self.expr(e),
        }
    }

    fn expr(&mut self, e: &Expr) {
        match &e.kind {
            ExprKind::Int(_) | ExprKind::Float(_) | ExprKind::Str(_) | ExprKind::Bool(_) => {}
            ExprKind::Array(xs) => xs.iter().for_each(|x| self.expr(x)),
            ExprKind::Path(path) => {
                // a local variable (captured by value when used inside a lambda)
                if path.len() == 1 && self.lookup(&path[0]).is_some() {
                    return;
                }
                match self.p.resolve(self.module, path) {
                    Ok(Resolved::Variant(t, v)) => {
                        let n = self.p.types[t].variants()[v].fields.len();
                        if n > 0 {
                            self.err(Diag::new(&C::WRONG_KIND, format!("variant `{}` has {n} field(s): write `{}(…)`", path.join("::"), path.join("::")), e.span));
                        }
                    }
                    // a function as a value: `let f = double`, `apply(double, 3)`
                    Ok(Resolved::Fn(f)) if self.p.fns[f].is_extern() => self.err(
                        Diag::new(&C::FFI_UNSUPPORTED, "extern functions cannot be used as values; wrap them in a Mimic function", e.span),
                    ),
                    Ok(Resolved::Fn(_)) => {}
                    Ok(r) => self.err(Diag::new(&C::WRONG_KIND, format!("{} used as a value", self.p.describe(r)), e.span)),
                    Err(_) if path.len() == 1 => {
                        let mut names: Vec<String> = self.scopes.iter().flat_map(|s| s.keys().cloned()).collect();
                        names.extend(self.p.visible_names(self.module));
                        let d = Diag::new(&C::UNRESOLVED_NAME, format!("cannot find variable `{}` in this scope", path[0]), e.span)
                            .did_you_mean(&path[0], e.span, names.iter().map(String::as_str));
                        self.err(d)
                    }
                    Err(m) => self.err(m.diag(self.p, self.file, e.span)),
                }
            }
            ExprKind::Call { callee, args } => {
                for a in args {
                    self.expr(a);
                }
                match &callee.kind {
                    ExprKind::Lambda { params, body } => {
                        if args.len() != params.len() {
                            self.err(Diag::new(&C::ARG_COUNT, format!("this lambda takes {} argument(s), but {} were given", params.len(), args.len()), e.span));
                        }
                        self.lambda(params, body);
                    }
                    ExprKind::OpRef(_) if args.len() != 2 => {
                        self.err(Diag::new(&C::ARG_COUNT, format!("an operator takes 2 arguments, but {} were given", args.len()), e.span))
                    }
                    ExprKind::OpRef(_) => {}
                    ExprKind::Path(path) => {
                        // calling a function value held in a variable
                        if path.len() == 1 && self.lookup(&path[0]).is_some() {
                            return;
                        }
                        match self.p.resolve(self.module, path) {
                            Ok(Resolved::Variant(t, v)) => {
                                let n = self.p.types[t].variants()[v].fields.len();
                                if n != args.len() {
                                    self.err(Diag::new(&C::ARG_COUNT, format!("variant `{}` has {n} field(s) but {} were given", path.join("::"), args.len()), e.span));
                                }
                            }
                            Ok(Resolved::Fn(f)) => {
                                let func = &self.p.fns[f];
                                let variadic = matches!(&func.kind, FnKind::Extern { decl, .. } if decl.variadic);
                                if func.arity() != args.len() && !(variadic && args.len() >= func.arity()) {
                                    self.err(Diag::new(&C::ARG_COUNT, 
                                        format!("`{}` takes {} argument(s) but {} were supplied", path.join("::"), func.arity(), args.len()),
                                        e.span,
                                    ));
                                }
                                if let FnKind::Extern { trusted: false, .. } = func.kind
                                    && self.unsafe_depth == 0 {
                                        let end = mimic_syntax::span_at(&self.p.files[self.file].src, e.span.end, e.span.end);
                                        let start = Span { end: e.span.start, ..e.span };
                                        self.err(Diag::new(&C::UNSAFE_REQUIRED, format!("call to extern function `{}` requires an `unsafe {{ }}` block", path.join("::")), e.span)
                                            .note("or declare the block as `trusted extern \"C\"` if the C function is memory safe")
                                            .fix_edits(
                                                "wrap the call in `unsafe { … }`",
                                                vec![
                                                    Edit { file: None, span: start, text: "unsafe { ".into() },
                                                    Edit { file: None, span: end, text: " }".into() },
                                                ],
                                                Applicability::MachineApplicable,
                                            ));
                                    }
                            }
                            Ok(r) => self.err(Diag::new(&C::WRONG_KIND, format!("{} is not a function", self.p.describe(r)), callee.span)),
                            Err(m) => self.err(m.diag(self.p, self.file, callee.span)),
                        }
                    }
                    // any expression producing a function value: `make_adder(1)(2)`
                    _ => self.expr(callee),
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
                    self.err(Diag::new(&C::ARG_COUNT, format!("`{}` takes {} argument(s), got {}", prim.info().word, prim.arity(), args.len()), e.span));
                    return;
                }
                match prim {
                    Prim::Each => {
                        self.expr(&args[0]);
                        self.fn_operand(&args[1], 1);
                    }
                    // xs ▷ gather(f), xs ▷ trail(f): f combines two values
                    Prim::Gather | Prim::Trail => {
                        self.expr(&args[0]);
                        self.fn_operand_named(&args[1], 2, prim.info().word);
                    }
                    // x ▷ evolve(n, step)
                    Prim::Evolve => {
                        self.expr(&args[0]);
                        self.expr(&args[1]);
                        self.fn_operand_named(&args[2], 1, "evolve");
                    }
                    // x ▷ settle(step, done)
                    Prim::Settle => {
                        self.expr(&args[0]);
                        self.fn_operand_named(&args[1], 1, "settle");
                        self.fn_operand_named(&args[2], 1, "settle");
                    }
                    _ => args.iter().for_each(|a| self.expr(a)),
                }
            }
            ExprKind::Field { base, .. } => self.expr(base),
            ExprKind::Tuple(xs) => xs.iter().for_each(|x| self.expr(x)),
            ExprKind::Interp(parts) => {
                for p in parts {
                    if let InterpPart::Expr(x) = p {
                        self.expr(x);
                    }
                }
            }
            ExprKind::Named { .. } => self.err(Diag::new(&C::NAMED_ARG_PLACE, "named arguments are only allowed in calls to functions", e.span)),
            ExprKind::With { base, fields } => {
                self.expr(base);
                fields.iter().for_each(|(_, v, _)| self.expr(v));
            }
            ExprKind::Record { path, fields } => {
                fields.iter().for_each(|(_, v, _)| self.expr(v));
                match self.p.resolve(self.module, path) {
                    Ok(Resolved::Type(t)) => {
                        let td = &self.p.types[t];
                        let TypeKind::Record(decl) = &td.decl.kind else {
                            self.err(Diag::new(&C::UNION_NOT_RECORD, format!("`{}` is a union: construct one of its variants", td.name), e.span));
                            return;
                        };
                        let mut errs = Vec::new();
                        for (n, _, sp) in fields {
                            if !decl.iter().any(|(d, _, _)| d == n) {
                                let at = Span { end: sp.start + n.len(), ..*sp };
                                errs.push(
                                    Diag::new(&C::NO_FIELD, format!("`{}` has no field `{n}`", td.name), *sp)
                                        .did_you_mean(n, at, decl.iter().map(|(d, _, _)| d.as_str())),
                                );
                            }
                        }
                        let missing: Vec<&str> = decl.iter().filter(|(d, _, _)| !fields.iter().any(|(n, _, _)| n == d)).map(|(d, _, _)| d.as_str()).collect();
                        if !missing.is_empty() {
                            errs.push(Diag::new(&C::MISSING_FIELDS, format!("missing field(s) of `{}`: {}", td.name, missing.join(", ")), e.span));
                        }
                        errs.into_iter().for_each(|d| self.err(d));
                    }
                    Ok(r) => self.err(Diag::new(&C::WRONG_KIND, format!("{} is not a record type", self.p.describe(r)), e.span)),
                    Err(m) => self.err(m.diag(self.p, self.file, e.span)),
                }
            }
            ExprKind::Match { scrut, arms } => {
                self.expr(scrut);
                let (mut union, mut covered, mut catch_all) = (None, Vec::new(), false);
                for arm in arms {
                    self.scopes.push(HashMap::new());
                    self.pattern(&arm.pat, true, &mut union, &mut covered, &mut catch_all);
                    self.expr(&arm.body);
                    self.scopes.pop();
                }
                if !catch_all {
                    match union {
                        Some(t) => {
                            let absent: Vec<_> =
                                self.p.types[t].variants().iter().enumerate().filter(|(i, _)| !covered.contains(i)).map(|(_, v)| v).collect();
                            let missing: Vec<String> = absent.iter().map(|v| format!("`{}`", v.name)).collect();
                            if !missing.is_empty() {
                                let mut d = Diag::new(&C::NON_EXHAUSTIVE, format!("`match` does not cover every case: missing {}", missing.join(", ")), e.span)
                                    .note("add the missing arms, or a final `_ => …`");
                                let arms_text: Vec<String> = absent
                                    .iter()
                                    .map(|v| {
                                        let pat = if v.fields.is_empty() {
                                            v.name.clone()
                                        } else {
                                            format!("{}({})", v.name, vec!["_"; v.fields.len()].join(", "))
                                        };
                                        format!("{pat} => …")
                                    })
                                    .collect();
                                if let Some(edit) = insert_arms(&self.p.files[self.file].src, e.span, &arms_text) {
                                    d = d.fix_edits("add the missing arms", vec![edit], Applicability::HasPlaceholders);
                                }
                                self.err(d);
                            }
                        }
                        None => self.err(Diag::new(&C::NON_EXHAUSTIVE, "`match` on values needs a final `_ => …` arm", e.span)),
                    }
                }
            }
            ExprKind::Lambda { params, body } => self.lambda(params, body),
            ExprKind::OpRef(_) => {}
            ExprKind::Arms { .. } => {} // lifted to a function after parsing
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

    fn lambda(&mut self, params: &[String], body: &Expr) {
        let saved_floor = self.lambda_floor;
        self.lambda_floor = self.scopes.len();
        self.scopes.push(HashMap::new());
        // the enclosing function's variables stay visible: they are captured by value
        for p in params {
            self.scopes.last_mut().unwrap().insert(p.to_string(), Var { mutable: false, span: body.span });
        }
        self.expr(body);
        self.scopes.pop();
        self.lambda_floor = saved_floor;
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
            "module main\nextern \"C\" {\n  fn abs(x: i32) -> i32\n}\nfn main() {\n  let x = 1\n  x = 2\n  let x = 3\n  mut y ← 1\n  y ↩ y + 1\n  self::abs(1)\n  unsafe { self::abs(1) }\n}\n",
        )]);
        assert_eq!(e.len(), 3, "{e:?}");
        assert!(e[0].contains("immutable"));
        assert!(e[1].contains("shadowing"));
        assert!(e[2].contains("unsafe"));
    }

    #[test]
    fn lambdas_capture_but_do_not_mutate() {
        // capturing by value is fine; assigning to a captured variable is not
        let ok = errors(&[("app", "src/main.mim", "module main\nfn main() {\n  let k = 2\n  print(⍳ 3 ¨ x => x × k)\n}\n")]);
        assert!(ok.is_empty(), "{ok:?}");
        let e = errors(&[("app", "src/main.mim", "module main\nfn main() {\n  let mut k = 2\n  let f = x => {\n    k = x\n    x\n  }\n  print(f(1))\n}\n")]);
        assert_eq!(e.len(), 1, "{e:?}");
        assert!(e[0].contains("captured"));
    }

    #[test]
    fn missing_main() {
        let e = errors(&[("app", "src/main.mim", "module main\nfn start() { 1 }\n")]);
        assert!(e[0].contains("entry point"));
    }
}

// ---- named and default arguments ------------------------------------------

/// Is `e` a constant (usable as a parameter default)?
pub fn is_constant(p: &Program, module: ModId, e: &Expr) -> bool {
    match &e.kind {
        ExprKind::Int(_) | ExprKind::Float(_) | ExprKind::Bool(_) | ExprKind::Str(_) => true,
        ExprKind::Array(xs) => xs.iter().all(|x| is_constant(p, module, x)),
        ExprKind::Unary { expr, .. } => is_constant(p, module, expr),
        ExprKind::Binary { lhs, rhs, .. } => is_constant(p, module, lhs) && is_constant(p, module, rhs),
        ExprKind::Path(path) => matches!(p.resolve(module, path), Ok(Resolved::Variant(t, v)) if p.types[t].variants()[v].fields.is_empty()),
        _ => false,
    }
}

/// Per function: its parameters' names and default values (`None` for externs).
type ParamInfo = Option<Vec<(String, Option<Expr>)>>;

/// Rewrite calls with named and default arguments into plain positional
/// calls, so every later phase sees one argument per parameter.
pub fn normalize(p: &mut Program) -> Vec<FileDiag> {
    let info: Vec<ParamInfo> = p
        .fns
        .iter()
        .map(|f| match &f.kind {
            FnKind::User(d) => Some(d.params.iter().map(|q| (q.name.clone(), q.default.clone())).collect()),
            FnKind::Extern { .. } => None,
        })
        .collect();
    let mut diags = Vec::new();
    for fid in 0..p.fns.len() {
        let (module, file) = (p.fns[fid].module, p.fns[fid].file);
        let mut body = match &mut p.fns[fid].kind {
            FnKind::User(d) => std::mem::take(&mut d.body),
            FnKind::Extern { .. } => continue,
        };
        let mut n = Norm { p, module, file, info: &info, diags: &mut diags };
        n.block(&mut body);
        if let FnKind::User(d) = &mut p.fns[fid].kind {
            d.body = body;
        }
    }
    diags
}

struct Norm<'a> {
    p: &'a Program,
    module: ModId,
    file: usize,
    info: &'a [ParamInfo],
    diags: &'a mut Vec<FileDiag>,
}

impl Norm<'_> {
    fn err(&mut self, d: Diag) {
        self.diags.push(FileDiag { file: self.file, diag: d });
    }

    fn block(&mut self, b: &mut Block) {
        for s in &mut b.stmts {
            match s {
                Stmt::Let { value, .. } | Stmt::Expr(value) => self.expr(value),
                Stmt::Assign { index, value, .. } => {
                    if let Some(i) = index {
                        self.expr(i);
                    }
                    self.expr(value);
                }
                Stmt::Return(Some(x), _) => self.expr(x),
                Stmt::Return(None, _) => {}
                Stmt::LetTuple { value, .. } => self.expr(value),
                Stmt::LetElse { value, els, .. } => {
                    self.expr(value);
                    self.block(els);
                }
            }
        }
        if let Some(t) = &mut b.tail {
            self.expr(t);
        }
    }

    fn expr(&mut self, e: &mut Expr) {
        match &mut e.kind {
            ExprKind::Array(xs) | ExprKind::Tuple(xs) => xs.iter_mut().for_each(|x| self.expr(x)),
            ExprKind::Call { callee, args } => {
                self.expr(callee);
                args.iter_mut().for_each(|x| self.expr(x));
            }
            ExprKind::Index { base, index } => {
                self.expr(base);
                self.expr(index);
            }
            ExprKind::Unary { expr, .. } | ExprKind::Lambda { body: expr, .. } | ExprKind::Field { base: expr, .. } | ExprKind::Named { value: expr, .. } => self.expr(expr),
            ExprKind::Binary { lhs, rhs, .. } => {
                self.expr(lhs);
                self.expr(rhs);
            }
            ExprKind::Prim { args, .. } => args.iter_mut().for_each(|x| self.expr(x)),
            ExprKind::If { cond, then, els } => {
                self.expr(cond);
                self.block(then);
                if let Some(x) = els {
                    self.expr(x);
                }
            }
            ExprKind::Block(b) | ExprKind::Unsafe(b) => self.block(b),
            ExprKind::Record { fields, .. } => fields.iter_mut().for_each(|(_, x, _)| self.expr(x)),
            ExprKind::With { base, fields } => {
                self.expr(base);
                fields.iter_mut().for_each(|(_, x, _)| self.expr(x));
            }
            ExprKind::Match { scrut, arms } => {
                self.expr(scrut);
                arms.iter_mut().for_each(|a| self.expr(&mut a.body));
            }
            ExprKind::Interp(parts) => parts.iter_mut().for_each(|p| {
                if let InterpPart::Expr(x) = p {
                    self.expr(x)
                }
            }),
            _ => {}
        }
        let span = e.span;
        let ExprKind::Call { callee, args } = &mut e.kind else { return };
        let ExprKind::Path(path) = &callee.kind else { return };
        let Ok(Resolved::Fn(f)) = self.p.resolve(self.module, path) else { return };
        let Some(params) = &self.info[f] else { return };
        let named = args.iter().any(|a| matches!(a.kind, ExprKind::Named { .. }));
        if !named && args.len() >= params.len() {
            return; // already positional (arity errors are reported later)
        }
        let fname = path.join("::");
        let mut slots: Vec<Option<Expr>> = vec![None; params.len()];
        let mut extra = Vec::new();
        let (mut pos, mut seen_named) = (0, false);
        for a in std::mem::take(args) {
            match a.kind {
                ExprKind::Named { name, value } => {
                    seen_named = true;
                    match params.iter().position(|(n, _)| *n == name) {
                        Some(i) if slots[i].is_some() => self.err(Diag::new(&C::DUPLICATE_ARG, format!("argument `{name}` is given twice"), a.span)),
                        Some(i) => slots[i] = Some(*value),
                        None => {
                            let at = Span { end: a.span.start + name.len(), ..a.span };
                            let d = Diag::new(&C::UNKNOWN_PARAM, format!("`{fname}` has no parameter `{name}`"), a.span)
                                .did_you_mean(&name, at, params.iter().map(|(n, _)| n.as_str()));
                            self.err(d)
                        }
                    }
                }
                _ => {
                    if seen_named {
                        self.err(Diag::new(&C::POSITIONAL_AFTER_NAMED, "positional arguments must come before named ones", a.span));
                    }
                    if pos < slots.len() {
                        slots[pos] = Some(a);
                    } else {
                        extra.push(a);
                    }
                    pos += 1;
                }
            }
        }
        let mut out = Vec::new();
        for (i, s) in slots.into_iter().enumerate() {
            match (s, &params[i].1) {
                (Some(x), _) => out.push(x),
                (None, Some(d)) => {
                    let mut d = d.clone();
                    d.span = span;
                    out.push(d);
                }
                (None, None) => self.err(Diag::new(&C::MISSING_ARG, format!("missing argument `{}` in call to `{fname}`", params[i].0), span)),
            }
        }
        out.extend(extra);
        *args = out;
    }
}

// ---- purity ---------------------------------------------------------------

/// Which functions are free of side effects (no `print`, no FFI — transitively).
/// Pure functions may be evaluated in parallel, in any order, or not at all.
pub struct Purity {
    pub fns: Vec<bool>,
}

impl Purity {
    pub fn analyze(p: &Program) -> Purity {
        // Optimistic fixpoint: assume pure, then knock out until stable.
        let mut pure: Vec<bool> = p.fns.iter().map(|f| !f.is_extern()).collect();
        loop {
            let mut changed = false;
            for (fid, f) in p.fns.iter().enumerate() {
                if !pure[fid] {
                    continue;
                }
                if let FnKind::User(d) = &f.kind {
                    let ok = PurityWalk { p, module: f.module, pure: &pure }.block(&d.body);
                    if !ok {
                        pure[fid] = false;
                        changed = true;
                    }
                }
            }
            if !changed {
                break;
            }
        }
        Purity { fns: pure }
    }

    /// Is a function operand (path or lambda, as passed to `¨`) pure?
    pub fn operand_is_pure(&self, p: &Program, module: ModId, e: &Expr) -> bool {
        match &e.kind {
            ExprKind::Path(path) => matches!(p.resolve(module, path), Ok(Resolved::Fn(f)) if self.fns[f]),
            ExprKind::Lambda { body, .. } => PurityWalk { p, module, pure: &self.fns }.expr(body),
            _ => false,
        }
    }
}

struct PurityWalk<'a> {
    p: &'a Program,
    module: ModId,
    pure: &'a [bool],
}

impl PurityWalk<'_> {
    fn block(&self, b: &Block) -> bool {
        b.stmts.iter().all(|s| match s {
            Stmt::Let { value, .. } => self.expr(value),
            Stmt::Assign { index, value, .. } => index.as_ref().is_none_or(|i| self.expr(i)) && self.expr(value),
            Stmt::Expr(e) => self.expr(e),
            Stmt::Return(e, _) => e.as_ref().is_none_or(|e| self.expr(e)),
            Stmt::LetTuple { value, .. } => self.expr(value),
            Stmt::LetElse { value, els, .. } => self.expr(value) && self.block(els),
        }) && b.tail.as_ref().is_none_or(|t| self.expr(t))
    }

    fn expr(&self, e: &Expr) -> bool {
        match &e.kind {
            ExprKind::Int(_) | ExprKind::Float(_) | ExprKind::Str(_) | ExprKind::Bool(_) | ExprKind::Path(_) => true,
            ExprKind::OpRef(_) => true,
            ExprKind::Arms { .. } => false, // lifted after parsing
            ExprKind::Array(xs) => xs.iter().all(|x| self.expr(x)),
            ExprKind::Call { callee, args } => {
                args.iter().all(|a| self.expr(a))
                    && match &callee.kind {
                        ExprKind::Lambda { body, .. } => self.expr(body),
                        ExprKind::OpRef(_) => true,
                        ExprKind::Path(path) => match self.p.resolve(self.module, path) {
                            Ok(Resolved::Fn(f)) => self.pure[f],
                            Ok(Resolved::Variant(..)) => true,
                            _ => false,
                        },
                        _ => false,
                    }
            }
            ExprKind::Index { base, index } => self.expr(base) && self.expr(index),
            ExprKind::Unary { expr, .. } => self.expr(expr),
            ExprKind::Binary { lhs, rhs, .. } => self.expr(lhs) && self.expr(rhs),
            ExprKind::Prim { prim: prim @ (Prim::Gather | Prim::Trail | Prim::Evolve | Prim::Settle), args } => {
                // which arguments are functions: those must be known-pure functions
                let fn_args: &[usize] = match prim {
                    Prim::Gather | Prim::Trail => &[1],
                    Prim::Evolve => &[2],
                    _ => &[1, 2],
                };
                args.iter().enumerate().all(|(i, a)| {
                    if !fn_args.contains(&i) {
                        return self.expr(a);
                    }
                    match &a.kind {
                        ExprKind::Lambda { body, .. } => self.expr(body),
                        ExprKind::OpRef(_) => true,
                        ExprKind::Path(path) => matches!(self.p.resolve(self.module, path), Ok(Resolved::Fn(f)) if self.pure[f]),
                        _ => false, // a function value from elsewhere: unknown
                    }
                })
            }
            ExprKind::Prim { prim, args } => {
                *prim != Prim::Print
                    && if *prim == Prim::Each {
                        self.expr(&args[0])
                            && match &args[1].kind {
                                ExprKind::Lambda { body, .. } => self.expr(body),
                                ExprKind::OpRef(_) => true,
                                ExprKind::Path(path) => matches!(self.p.resolve(self.module, path), Ok(Resolved::Fn(f)) if self.pure[f]),
                                _ => false,
                            }
                    } else {
                        args.iter().all(|a| self.expr(a))
                    }
            }
            ExprKind::Lambda { body, .. } => self.expr(body),
            ExprKind::If { cond, then, els } => self.expr(cond) && self.block(then) && els.as_ref().is_none_or(|e| self.expr(e)),
            ExprKind::Block(b) | ExprKind::Unsafe(b) => self.block(b),
            ExprKind::Field { base, .. } => self.expr(base),
            ExprKind::Tuple(xs) => xs.iter().all(|x| self.expr(x)),
            ExprKind::Named { value, .. } => self.expr(value),
            ExprKind::Record { fields, .. } => fields.iter().all(|(_, v, _)| self.expr(v)),
            ExprKind::With { base, fields } => self.expr(base) && fields.iter().all(|(_, v, _)| self.expr(v)),
            ExprKind::Match { scrut, arms } => self.expr(scrut) && arms.iter().all(|a| self.expr(&a.body)),
            ExprKind::Interp(parts) => parts.iter().all(|p| match p {
                InterpPart::Expr(x) => self.expr(x),
                InterpPart::Text(_) => true,
            }),
        }
    }
}

#[cfg(test)]
mod purity_tests {
    use super::*;
    use mimic_syntax::parser::parse_file;

    #[test]
    fn transitive_purity() {
        let src = "module main\ntrusted extern \"C\" {\n  fn sqrt(x: f64) -> f64\n}\nfn main() { 1 }\nfn a(x) { b(x) + 1 }\nfn b(x) { x × 2 }\nfn c(x) { print(x) }\nfn d(x) { c(x) }\nfn e(x) { sqrt(x) }\nfn r(x) { if x == 0 { 0 } else { r(x - 1) } }\n";
        let f = SourceFile { path: "m.mim".into(), display: "m.mim".into(), src: src.into(), ast: parse_file(src).unwrap(), package: "t".into() };
        let (p, d) = Program::build(vec![f], "t");
        assert!(d.is_empty());
        let pu = Purity::analyze(&p);
        let get = |n: &str| pu.fns[p.fns.iter().position(|f| f.name == n).unwrap()];
        assert!(get("a") && get("b") && get("r"));
        assert!(!get("c") && !get("d") && !get("e") && !get("sqrt"));
    }
}

/// Built-in type names (for suggestions).
const BUILTIN_TYPES: &[&str] = &["i64", "f64", "bool", "char", "str", "any"];

/// An edit inserting `arms` (one per line) before the closing `}` of the
/// `match` expression at `span`, indented like the existing arms.
fn insert_arms(src: &str, span: Span, arms: &[String]) -> Option<Edit> {
    let text = src.get(span.start..span.end)?;
    let close = span.start + text.rfind('}')?;
    let before = &src[..close];
    let line_start = before.rfind('\n').map_or(0, |i| i + 1);
    let closing_indent: String = before[line_start..].chars().take_while(|c| c.is_whitespace()).collect();
    if before[line_start..].trim().is_empty() {
        // `}` on its own line: insert full lines above it, indented one level deeper
        let indent = format!("{closing_indent}    ");
        let body: String = arms.iter().map(|a| format!("{indent}{a}\n")).collect();
        Some(Edit { file: None, span: mimic_syntax::span_at(src, line_start, line_start), text: body })
    } else {
        // single-line match: `match x { A => 1 }` → `match x { A => 1, B => … }`
        let trimmed = before.trim_end();
        let at = trimmed.len();
        let sep = if trimmed.ends_with('{') || trimmed.ends_with(',') { " " } else { ", " };
        Some(Edit { file: None, span: mimic_syntax::span_at(src, at, at), text: format!("{sep}{}", arms.join(", ")) })
    }
}
