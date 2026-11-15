//! Global package store: `~/.mimic/libraries/<name>/` (override with `MIMIC_HOME`).
//!
//! Libraries come from git: `mimic install <git-url>` takes the default
//! branch into `<name>/`; `mimic install <git-url>@<ref>` takes a tag,
//! branch or commit into `<name>@<ref>/`, so projects pinned to different
//! versions of one library each find their own. A registry comes later.

use crate::{MANIFEST, parse_manifest};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

pub fn mimic_home() -> PathBuf {
    if let Some(h) = std::env::var_os("MIMIC_HOME") {
        return PathBuf::from(h);
    }
    // Windows: the user's profile folder (HOME, when set, is a Unix shell's)
    let order = if cfg!(windows) { ["USERPROFILE", "HOME"] } else { ["HOME", "USERPROFILE"] };
    let home = order.iter().find_map(std::env::var_os).unwrap_or_else(|| ".".into());
    PathBuf::from(home).join(".mimic")
}

pub fn libraries_dir() -> PathBuf {
    mimic_home().join("libraries")
}

pub fn cache_dir() -> PathBuf {
    mimic_home().join("cache")
}

pub struct Installed {
    pub name: String,
    pub version: String,
    pub source: String,
    /// The tag, branch or commit it was installed at (`None`: the default branch).
    pub rev: Option<String>,
    pub dir: PathBuf,
}

/// `github.com/x/y.git@v1.2.0` → (`github.com/x/y.git`, `Some("v1.2.0")`).
/// Only an `@` after the last `/` counts (`git@github.com:…` is a URL).
pub fn split_ref(src: &str) -> (String, Option<String>) {
    let tail_at = src.rfind(['/', ':']).map_or(0, |i| i + 1);
    match src[tail_at..].rfind('@') {
        Some(i) if i > 0 && tail_at + i + 1 < src.len() => (src[..tail_at + i].to_string(), Some(src[tail_at + i + 1..].to_string())),
        _ => (src.to_string(), None),
    }
}

/// Where library `name` is installed: `<name>` for the default branch,
/// `<name>@<rev>` for a tag, branch or commit (`/` in a branch name becomes `-`).
pub fn dir_for(name: &str, rev: Option<&str>) -> PathBuf {
    match rev {
        None => libraries_dir().join(name),
        Some(r) => libraries_dir().join(format!("{name}@{}", r.replace(['/', '\\'], "-"))),
    }
}

const SOURCE_FILE: &str = ".mimic-source";

/// Normalise `github.com/x/y.git` / `https://…` / `git@…` into a clonable URL.
pub fn normalize_url(src: &str) -> String {
    if src.contains("://") || src.starts_with("git@") || Path::new(src).exists() {
        src.to_string()
    } else {
        format!("https://{src}")
    }
}

fn repo_name(url: &str) -> String {
    let last = url.trim_end_matches('/').rsplit(['/', ':']).next().unwrap_or(url);
    last.trim_end_matches(".git").to_string()
}

fn git(args: &[&str], cwd: Option<&Path>) -> Result<String, String> {
    let mut c = Command::new("git");
    c.args(args);
    if let Some(d) = cwd {
        c.current_dir(d);
    }
    let out = c.output().map_err(|e| format!("cannot run git: {e} (is git installed?)"))?;
    if !out.status.success() {
        return Err(format!("git {} failed:\n{}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

pub fn install(src: &str) -> Result<Installed, String> {
    let (src, rev) = split_ref(src);
    let looks_like_url = src.contains('/') || src.contains("://") || src.ends_with(".git");
    if !looks_like_url {
        return Err(format!(
            "`{src}` looks like a package name, but the package registry is not available yet.\n\
             install from git instead, e.g. `mimic install github.com/<owner>/{src}.git`"
        ));
    }
    install_at(&normalize_url(&src), rev.as_deref(), None)
}

/// Clone `url` at `rev` (a tag, branch or commit; `None`: the default
/// branch), then, when `commit` is given, check out that exact commit (a
/// revision locked in mod.mim).
pub fn install_at(url: &str, rev: Option<&str>, commit: Option<&str>) -> Result<Installed, String> {
    let tmp = libraries_dir().join(format!(".tmp-{}", std::process::id()));
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(libraries_dir()).map_err(|e| e.to_string())?;
    let tmp_s = tmp.to_string_lossy().to_string();
    let cloned = match (rev, commit) {
        // a tag or branch, newest commit only
        (Some(r), None) => git(&["clone", "--depth", "1", "--quiet", "--branch", r, url, &tmp_s], None).is_ok(),
        (None, None) => {
            git(&["clone", "--depth", "1", "--quiet", url, &tmp_s], None)?;
            true
        }
        _ => false,
    };
    if !cloned {
        // a commit (or a locked revision): the whole history, then check it out
        let _ = fs::remove_dir_all(&tmp);
        git(&["clone", "--quiet", url, &tmp_s], None)?;
        let want = commit.or(rev).unwrap_or("HEAD");
        if let Err(e) = git(&["checkout", "--quiet", want], Some(&tmp)) {
            let _ = fs::remove_dir_all(&tmp);
            return Err(format!("{url} has no tag, branch or commit `{want}`:\n{e}"));
        }
    }
    let manifest = match parse_manifest(&tmp.join(MANIFEST)) {
        Ok(m) => m,
        Err(e) => {
            let _ = fs::remove_dir_all(&tmp);
            return Err(format!("{url} is not a mimic project (no valid mim.mim):\n{e}"));
        }
    };
    if !manifest.lib {
        let _ = fs::remove_dir_all(&tmp);
        return Err(format!("`{}` is not a library (add `lib` to `kind` in its mim.mim)", manifest.name));
    }
    if manifest.name != repo_name(url) {
        eprintln!("note: repository `{}` provides library `{}`", repo_name(url), manifest.name);
    }
    let dest = dir_for(&manifest.name, rev);
    if dest.exists() {
        let _ = fs::remove_dir_all(&tmp);
        let shown = dest.file_name().unwrap_or_default().to_string_lossy().to_string();
        return Err(format!("`{shown}` is already installed; use `mimic update {shown}`"));
    }
    let record = match rev {
        Some(r) => format!("{url}\n{r}\n"),
        None => format!("{url}\n"),
    };
    fs::write(tmp.join(SOURCE_FILE), record).map_err(|e| e.to_string())?;
    fs::rename(&tmp, &dest).map_err(|e| e.to_string())?;
    Ok(Installed { name: manifest.name, version: manifest.version, source: url.to_string(), rev: rev.map(String::from), dir: dest })
}

pub fn list() -> Result<Vec<Installed>, String> {
    let dir = libraries_dir();
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for e in fs::read_dir(&dir).map_err(|e| e.to_string())? {
        let p = e.map_err(|e| e.to_string())?.path();
        let name = p.file_name().unwrap().to_string_lossy().to_string();
        if !p.is_dir() || name.starts_with('.') {
            continue;
        }
        let version = parse_manifest(&p.join(MANIFEST)).map(|m| m.version).unwrap_or_else(|_| "?".into());
        let record = fs::read_to_string(p.join(SOURCE_FILE)).unwrap_or_default();
        let mut lines = record.lines().map(str::trim);
        let source = lines.next().unwrap_or("").to_string();
        let rev = lines.next().filter(|r| !r.is_empty()).map(String::from);
        out.push(Installed { name, version, source, rev, dir: p });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

/// An installed library by its folder name: `ai`, or `ai@v1.2.0`.
pub fn find(name: &str) -> Result<Installed, String> {
    list()?.into_iter().find(|i| i.name == name).ok_or_else(|| format!("library `{name}` is not installed (see `mimic list`)"))
}

/// The library's name without its version: `ai@v1.2.0` → `ai`.
pub fn base_name(installed: &str) -> &str {
    installed.split('@').next().unwrap_or(installed)
}

/// Move one installed library to the newest commit of its branch (the
/// default branch, or the branch it was installed at). A tag or a commit
/// stays where it is. Returns (old rev, new rev).
pub fn update(lib: &Installed) -> Result<(String, String), String> {
    if !lib.dir.join(".git").exists() {
        return Err(format!("`{}` was not installed from git; reinstall it", lib.name));
    }
    let before = git(&["rev-parse", "--short", "HEAD"], Some(&lib.dir))?;
    let branch = match &lib.rev {
        None => Some("HEAD".to_string()),
        Some(r) => {
            let heads = git(&["ls-remote", "--heads", "origin", r], Some(&lib.dir))?;
            (!heads.is_empty()).then(|| r.clone())
        }
    };
    if let Some(b) = branch {
        git(&["fetch", "--quiet", "--depth", "1", "origin", &b], Some(&lib.dir))?;
        git(&["checkout", "--quiet", "FETCH_HEAD"], Some(&lib.dir))?;
    }
    let after = git(&["rev-parse", "--short", "HEAD"], Some(&lib.dir))?;
    Ok((before, after))
}

pub fn uninstall(name: &str) -> Result<PathBuf, String> {
    let lib = find(name)?;
    fs::remove_dir_all(&lib.dir).map_err(|e| format!("cannot remove {}: {e}", lib.dir.display()))?;
    Ok(lib.dir)
}

/// Current commit of an installed library (for the lock section of mod.mim).
pub fn revision(dir: &Path) -> Option<String> {
    git(&["rev-parse", "HEAD"], Some(dir)).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn urls() {
        assert_eq!(normalize_url("github.com/harcorp/ai.git"), "https://github.com/harcorp/ai.git");
        assert_eq!(repo_name("https://github.com/harcorp/ai.git"), "ai");
        assert_eq!(repo_name("git@github.com:harcorp/speech.git"), "speech");
        assert_eq!(split_ref("github.com/x/y.git@v1.2.0"), ("github.com/x/y.git".into(), Some("v1.2.0".into())));
        assert_eq!(split_ref("git@github.com:x/y.git"), ("git@github.com:x/y.git".into(), None));
        assert_eq!(split_ref("git@github.com:x/y.git@main"), ("git@github.com:x/y.git".into(), Some("main".into())));
        assert_eq!(split_ref("https://github.com/x/y"), ("https://github.com/x/y".into(), None));
        assert_eq!(base_name("ai@v1"), "ai");
    }
}
