//! `mimic upgrade`: the latest Mimic (and, with `--editors`, the editor
//! extension), installed by the same scripts as
//! `curl -fsSL https://mimic.codes/install.sh | sh` and
//! `irm https://mimic.codes/install.ps1 | iex`. They are carried inside
//! mimic, so upgrading needs no script from the network, only the release.

use std::path::PathBuf;
use std::process::{Command, ExitCode};

const INSTALL_SH: &str = include_str!("../../../tools/get/install.sh");
const INSTALL_PS1: &str = include_str!("../../../tools/get/install.ps1");
const VSCODE_SH: &str = include_str!("../../../tools/get/vscode.sh");
const VSCODE_PS1: &str = include_str!("../../../tools/get/vscode.ps1");

pub struct Options {
    pub version: Option<String>,
    pub force: bool,
    pub check: bool,
    pub editors: bool,
}

/// Write a script to a temporary file and run it; its exit status.
fn run_script(name: &str, text: &str, args: &[String], env: &[(&str, String)]) -> Result<i32, String> {
    let dir = std::env::temp_dir().join(format!("mimic-upgrade-{}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(name);
    std::fs::write(&path, text).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
    let mut cmd = if cfg!(windows) {
        let mut c = Command::new("powershell");
        c.args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"]).arg(&path);
        c
    } else {
        let mut c = Command::new("sh");
        c.arg(&path).args(args);
        c
    };
    for (k, v) in env {
        cmd.env(k, v);
    }
    let status = cmd.status().map_err(|e| format!("cannot run {name}: {e}"));
    let _ = std::fs::remove_dir_all(&dir);
    Ok(status?.code().unwrap_or(1))
}

pub fn run(o: Options) -> Result<ExitCode, String> {
    // where the installers put mimic; a mimic elsewhere (built from source,
    // a package manager) is left alone
    let home = std::env::var_os("MIMIC_HOME").map(PathBuf::from).or_else(|| {
        let h = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })?;
        Some(PathBuf::from(h).join(".mimic"))
    });
    if let (Some(home), Ok(me)) = (&home, std::env::current_exe()) {
        let bin = home.join("bin");
        if !me.starts_with(&bin) && !o.check {
            eprintln!("note: this mimic is {}; the upgrade goes to {} (the one on PATH after it)", me.display(), bin.display());
        }
    }
    let code = if cfg!(windows) {
        let mut env = vec![];
        if let Some(v) = &o.version {
            env.push(("MIMIC_VERSION", v.clone()));
        }
        if o.force {
            env.push(("MIMIC_FORCE", "1".to_string()));
        }
        if o.check {
            env.push(("MIMIC_CHECK", "1".to_string()));
        }
        run_script("install.ps1", INSTALL_PS1, &[], &env)?
    } else {
        let mut args = vec![];
        if let Some(v) = &o.version {
            args.push("--version".to_string());
            args.push(v.clone());
        }
        if o.force {
            args.push("--force".to_string());
        }
        if o.check {
            args.push("--check".to_string());
        }
        run_script("install.sh", INSTALL_SH, &args, &[])?
    };
    if code != 0 {
        return Ok(ExitCode::from(code.clamp(1, 255) as u8));
    }
    if o.editors && !o.check {
        let code = if cfg!(windows) {
            let env = if o.force { vec![("MIMIC_FORCE", "1".to_string())] } else { vec![] };
            run_script("vscode.ps1", VSCODE_PS1, &[], &env)?
        } else {
            run_script("vscode.sh", VSCODE_SH, &if o.force { vec!["--force".to_string()] } else { vec![] }, &[])?
        };
        if code != 0 {
            return Ok(ExitCode::from(code.clamp(1, 255) as u8));
        }
    }
    Ok(ExitCode::SUCCESS)
}
