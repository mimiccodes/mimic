//! Global package store: `~/.mimic/libraries/<name>/` (override with `MIMIC_HOME`).
//!
//! Milestone 1 installs from git URLs; a registry comes later.

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
    pub dir: PathBuf,
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
    let looks_like_url = src.contains('/') || src.contains("://") || src.ends_with(".git");
    if !looks_like_url {
        return Err(format!(
            "`{src}` looks like a package name, but the package registry is not available yet.\n\
             install from git instead, e.g. `mimic install github.com/<owner>/{src}.git`"
        ));
    }
    let url = normalize_url(src);
    let tmp = libraries_dir().join(format!(".tmp-{}", std::process::id()));
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(libraries_dir()).map_err(|e| e.to_string())?;
    git(&["clone", "--depth", "1", "--quiet", &url, &tmp.to_string_lossy()], None)?;
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
    if manifest.name != repo_name(&url) {
        eprintln!("note: repository `{}` provides library `{}`", repo_name(&url), manifest.name);
    }
    let dest = libraries_dir().join(&manifest.name);
    if dest.exists() {
        let _ = fs::remove_dir_all(&tmp);
        return Err(format!("`{}` is already installed; use `mimic update {}`", manifest.name, manifest.name));
    }
    fs::write(tmp.join(SOURCE_FILE), &url).map_err(|e| e.to_string())?;
    fs::rename(&tmp, &dest).map_err(|e| e.to_string())?;
    Ok(Installed { name: manifest.name, version: manifest.version, source: url, dir: dest })
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
        let source = fs::read_to_string(p.join(SOURCE_FILE)).unwrap_or_default().trim().to_string();
        out.push(Installed { name, version, source, dir: p });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

pub fn find(name: &str) -> Result<Installed, String> {
    list()?.into_iter().find(|i| i.name == name).ok_or_else(|| format!("library `{name}` is not installed (see `mimic list`)"))
}

/// Fast-forward one installed library. Returns (old rev, new rev).
pub fn update(lib: &Installed) -> Result<(String, String), String> {
    if !lib.dir.join(".git").exists() {
        return Err(format!("`{}` was not installed from git; reinstall it", lib.name));
    }
    let before = git(&["rev-parse", "--short", "HEAD"], Some(&lib.dir))?;
    git(&["pull", "--ff-only", "--quiet"], Some(&lib.dir))?;
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
    }
}
