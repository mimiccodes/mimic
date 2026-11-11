//! Project model: `mim.mim` (manifest), `build.mim` (build settings),
//! project scaffolding and the global package store at `~/.mimic/libraries`.

pub mod packages;

use mimic_syntax::config::{self, Table, Value};
use std::fs;
use std::path::{Path, PathBuf};

pub const MANIFEST: &str = "mim.mim";
pub const BUILD_FILE: &str = "build.mim";
pub const MOD_FILE: &str = "mod.mim";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DepSource {
    /// Installed globally under ~/.mimic/libraries/<name>.
    Installed,
    Path(PathBuf),
    Git { url: String, rev: Option<String> },
}

#[derive(Clone, Debug)]
pub struct Dependency {
    pub name: String,
    pub source: DepSource,
    pub version: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct Manifest {
    pub name: String,
    pub version: String,
    pub authors: Vec<String>,
    pub description: String,
    pub readme: Option<String>,
    pub license: Option<String>,
    pub repository: Option<String>,
    pub edition: String,
    pub bin: bool,
    pub lib: bool,
    pub dependencies: Vec<Dependency>,
    /// C libraries to link (`-l<name>`).
    pub ffi_link: Vec<String>,
    /// Extra library search paths (`-L<dir>`).
    pub ffi_search: Vec<String>,
    /// Header search paths for `extern "C" include` (`-I<dir>`), relative to the project.
    pub ffi_include: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct Profile {
    pub opt: u8,
    pub debug: bool,
    pub leak_check: bool,
}

#[derive(Clone, Debug)]
pub struct BuildConfig {
    pub debug: Profile,
    pub release: Profile,
    /// Default cross targets (LLVM triples) built by `mimic build --all-targets`.
    pub targets: Vec<String>,
}

impl Default for BuildConfig {
    fn default() -> Self {
        BuildConfig {
            debug: Profile { opt: 0, debug: true, leak_check: true },
            release: Profile { opt: 3, debug: false, leak_check: false },
            targets: Vec::new(),
        }
    }
}

pub struct Project {
    pub root: PathBuf,
    pub manifest: Manifest,
    pub build: BuildConfig,
}

fn err_at(path: &Path, src: &str, d: mimic_syntax::Diag) -> String {
    d.render(path, src)
}

fn strings(v: Option<&Value>) -> Vec<String> {
    v.and_then(Value::as_list)
        .map(|l| l.iter().filter_map(|x| x.as_str().map(String::from)).collect())
        .unwrap_or_default()
}

pub fn parse_manifest(path: &Path) -> Result<Manifest, String> {
    let src = fs::read_to_string(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let cfg = config::parse(&src).map_err(|d| err_at(path, &src, d))?;
    let bad = |msg: String, key: &str, t: &Table| {
        let d = mimic_syntax::Diag::new(&mimic_syntax::codes::PROJECT, msg, t.span_of(key).unwrap_or_default());
        err_at(path, &src, d)
    };
    if cfg.header != "mim" {
        return Err(format!("{}: the manifest must start with `module mim`", path.display()));
    }
    let root = &cfg.root;
    let proj = root.table("project").ok_or_else(|| format!("{}: missing `project {{ … }}` block", path.display()))?;
    let name = proj.str("name").ok_or_else(|| bad("project.name is required".into(), "project", root))?.to_string();
    if !valid_ident(&name) {
        return Err(bad(format!("project name `{name}` must be a valid identifier (letters, digits, _)"), "project", root));
    }
    let kinds = strings(proj.get("kind"));
    let (bin, lib) = if kinds.is_empty() {
        (true, false)
    } else {
        for k in &kinds {
            if k != "bin" && k != "lib" {
                return Err(bad(format!("unknown project kind `{k}` (expected bin or lib)"), "kind", proj));
            }
        }
        (kinds.iter().any(|k| k == "bin"), kinds.iter().any(|k| k == "lib"))
    };
    let mut deps = Vec::new();
    if let Some(t) = root.table("dependencies") {
        for (dname, v, _) in &t.entries {
            let (source, version) = match v {
                Value::Str(ver) => (DepSource::Installed, Some(ver.clone())),
                Value::Table(dt) => {
                    let version = dt.str("version").map(String::from);
                    if let Some(p) = dt.str("path") {
                        (DepSource::Path(PathBuf::from(p)), version)
                    } else if let Some(g) = dt.str("git") {
                        (DepSource::Git { url: g.into(), rev: dt.str("rev").map(String::from) }, version)
                    } else {
                        (DepSource::Installed, version)
                    }
                }
                _ => return Err(bad(format!("dependency `{dname}`: expected \"version\" or {{ path/git = … }}"), dname, t)),
            };
            deps.push(Dependency { name: dname.clone(), source, version });
        }
    }
    let ffi = root.table("ffi");
    Ok(Manifest {
        name,
        version: proj.str("version").unwrap_or("0.1.0").into(),
        authors: strings(proj.get("authors")),
        description: proj.str("description").unwrap_or("").into(),
        readme: proj.str("readme").map(String::from),
        license: proj.str("license").map(String::from),
        repository: proj.str("repository").map(String::from),
        edition: proj.str("edition").unwrap_or("2026").into(),
        bin,
        lib,
        dependencies: deps,
        ffi_link: strings(ffi.and_then(|f| f.get("link"))),
        ffi_search: strings(ffi.and_then(|f| f.get("search"))),
        ffi_include: strings(ffi.and_then(|f| f.get("include"))),
    })
}

pub fn parse_build(path: &Path) -> Result<BuildConfig, String> {
    let mut b = BuildConfig::default();
    if !path.exists() {
        return Ok(b);
    }
    let src = fs::read_to_string(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let cfg = config::parse(&src).map_err(|d| err_at(path, &src, d))?;
    if let Some(profiles) = cfg.root.table("profile") {
        for (name, v, _) in &profiles.entries {
            let Value::Table(t) = v else { continue };
            let p = match name.as_str() {
                "debug" => &mut b.debug,
                "release" => &mut b.release,
                other => return Err(format!("{}: unknown profile `{other}` (debug or release)", path.display())),
            };
            if let Some(Value::Int(o)) = t.get("opt") {
                p.opt = (*o).clamp(0, 3) as u8;
            }
            if let Some(Value::Bool(d)) = t.get("debug") {
                p.debug = *d;
            }
            if let Some(Value::Bool(l)) = t.get("leak_check") {
                p.leak_check = *l;
            }
        }
    }
    b.targets = strings(cfg.root.get("targets"));
    Ok(b)
}

pub fn valid_ident(s: &str) -> bool {
    s.chars().next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_') && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Walk up from `start` to find the directory containing `mim.mim`.
pub fn find_root(start: &Path) -> Option<PathBuf> {
    let mut cur = Some(start);
    while let Some(c) = cur {
        if c.join(MANIFEST).is_file() {
            return Some(c.to_path_buf());
        }
        cur = c.parent();
    }
    None
}

impl Project {
    pub fn load(root: &Path) -> Result<Project, String> {
        let manifest = parse_manifest(&root.join(MANIFEST))?;
        let build = parse_build(&root.join(BUILD_FILE))?;
        Ok(Project { root: root.to_path_buf(), manifest, build })
    }

    /// All `.mim` sources under `src/`, sorted for determinism.
    pub fn sources(&self) -> Result<Vec<PathBuf>, String> {
        sources_in(&self.root.join("src"))
    }

    /// Resolve a dependency to the directory of its project.
    pub fn dep_dir(&self, d: &Dependency) -> Result<PathBuf, String> {
        let dir = match &d.source {
            DepSource::Path(p) => self.root.join(p),
            DepSource::Installed | DepSource::Git { .. } => packages::libraries_dir().join(&d.name),
        };
        if !dir.join(MANIFEST).is_file() {
            let hint = match &d.source {
                DepSource::Git { url, .. } => format!("run `mimic install {url}`"),
                DepSource::Installed => format!("run `mimic install <git-url>` to install `{}`", d.name),
                DepSource::Path(_) => "check the `path` in mim.mim".into(),
            };
            return Err(format!("dependency `{}` not found at {} ({hint})", d.name, dir.display()));
        }
        Ok(dir)
    }
}

pub fn sources_in(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let mut out = Vec::new();
    fn walk(d: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
        for e in fs::read_dir(d)? {
            let p = e?.path();
            if p.is_dir() {
                walk(&p, out)?;
            } else if p.extension().is_some_and(|x| x == "mim") {
                out.push(p);
            }
        }
        Ok(())
    }
    walk(dir, &mut out).map_err(|e| format!("cannot read {}: {e}", dir.display()))?;
    out.sort();
    Ok(out)
}

// ---- scaffolding ---------------------------------------------------------------

pub fn manifest_template(name: &str, bin: bool, lib: bool) -> String {
    let mut kinds = Vec::new();
    if bin {
        kinds.push("bin");
    }
    if lib {
        kinds.push("lib");
    }
    format!(
        r#"module mim

// Project settings. See docs/manifest.md for every field.
project {{
    name = "{name}"
    version = "0.1.0"
    authors = []
    description = ""
    readme = "README.md"
    license = "MIT"
    edition = "2026"
    kind = [{kinds}]
}}

// Libraries declared here are usable from every file as `name::item`
// without an import (`use name::item` also works).
//   speech = {{ git = "github.com/harcorp/speech.git" }}
//   util   = {{ path = "../util" }}
dependencies {{
}}

// C libraries for `extern "C"` blocks.
ffi {{
    link = []
}}
"#,
        kinds = kinds.join(", ")
    )
}

pub const BUILD_TEMPLATE: &str = r#"module build

// Build profiles used by `mimic build|run --debug|--release`.
profile debug {
    opt = 0
    debug = true
    leak_check = true
}

profile release {
    opt = 3
    debug = false
    leak_check = false
}

// Extra cross-compilation targets (LLVM triples, e.g. "aarch64-linux"), built with `mimic build --all-targets`.
targets = []
"#;

pub const MAIN_TEMPLATE: &str = r#"module main

// `main` is the only item that is public by default.
fn main() {
    let xs = ⍳ 10              // iota 10 → [0 1 2 … 9]
    print(xs)
    print(xs ▷ +/)             // xs |> sum → 45
    print(xs ⌽ ↑ 3)            // reverse, take 3 → [9 8 7]
    print(xs ¨ (x => x × x) ▷ +/)  // sum of squares → 285
}
"#;

pub fn lib_template(name: &str) -> String {
    format!(
        r#"module lib

// Items of `module lib` are the library's public API: `{name}::hello()`.
pub fn hello() {{
    print("hello from {name}")
}}
"#
    )
}

/// Create the standard project layout in `dir` (which may already exist but must not contain mim.mim).
pub fn scaffold(dir: &Path, name: &str, bin: bool, lib: bool) -> Result<Vec<PathBuf>, String> {
    if !valid_ident(name) {
        return Err(format!("`{name}` is not a valid project name: use letters, digits and `_`, starting with a letter"));
    }
    if dir.join(MANIFEST).exists() {
        return Err(format!("{} already contains a mimic project", dir.display()));
    }
    let io = |e: std::io::Error| e.to_string();
    fs::create_dir_all(dir.join("src")).map_err(io)?;
    fs::create_dir_all(dir.join("target")).map_err(io)?;
    let mut written = Vec::new();
    let mut put = |rel: &str, body: String| -> Result<(), String> {
        let p = dir.join(rel);
        if p.exists() {
            return Ok(());
        }
        fs::write(&p, body).map_err(|e| format!("cannot write {}: {e}", p.display()))?;
        written.push(p);
        Ok(())
    };
    put(MANIFEST, manifest_template(name, bin, lib))?;
    put(BUILD_FILE, BUILD_TEMPLATE.into())?;
    if bin {
        let main = if lib {
            MAIN_TEMPLATE.replace("    print(xs ¨ (x => x × x) ▷ +/)  // sum of squares → 285\n", &format!("    print(xs ¨ (x => x × x) ▷ +/)  // sum of squares → 285\n    {name}::hello()\n"))
        } else {
            MAIN_TEMPLATE.into()
        };
        put("src/main.mim", main)?;
    }
    if lib {
        put("src/lib.mim", lib_template(name))?;
    }
    put(".gitignore", "/target\n".into())?;
    put("README.md", format!("# {name}\n\nA [Mimic](https://github.com/harcorp/mimic) project.\n"))?;
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn templates_parse() {
        let dir = std::env::temp_dir().join(format!("mimic-scaffold-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        scaffold(&dir, "demo", true, true).unwrap();
        let p = Project::load(&dir).unwrap();
        assert_eq!(p.manifest.name, "demo");
        assert!(p.manifest.bin && p.manifest.lib);
        assert_eq!(p.build.release.opt, 3);
        assert_eq!(p.sources().unwrap().len(), 2);
        fs::remove_dir_all(&dir).unwrap();
    }
}
