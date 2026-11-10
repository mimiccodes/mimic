//! Compile many C/assembly files in parallel with the embedded Clang.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

pub struct Job {
    pub src: PathBuf,
    pub out: PathBuf,
    pub args: Vec<String>,
}

/// `mimic cc <args> -c <src> -o <out>` for every job, on all cores.
/// With `tolerate`, failed jobs are skipped and returned instead of failing.
pub fn run(jobs: &[Job], tolerate: bool) -> Result<Vec<PathBuf>, String> {
    let me = std::env::current_exe().map_err(|e| e.to_string())?;
    let next = AtomicUsize::new(0);
    let failed: Mutex<Vec<(PathBuf, String)>> = Mutex::new(Vec::new());
    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
    std::thread::scope(|s| {
        for _ in 0..threads {
            s.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(j) = jobs.get(i) else { break };
                    if !failed.lock().unwrap().is_empty() && !tolerate {
                        break;
                    }
                    if let Some(d) = j.out.parent() {
                        let _ = std::fs::create_dir_all(d);
                    }
                    let out = Command::new(&me).arg("cc").args(&j.args).arg("-c").arg(&j.src).arg("-o").arg(&j.out).output();
                    match out {
                        Ok(o) if o.status.success() => {}
                        Ok(o) => failed.lock().unwrap().push((j.src.clone(), String::from_utf8_lossy(&o.stderr).into_owned())),
                        Err(e) => failed.lock().unwrap().push((j.src.clone(), e.to_string())),
                    }
                }
            });
        }
    });
    let failed = failed.into_inner().unwrap();
    if !tolerate && let Some((src, err)) = failed.first() {
        return Err(format!("compiling {} failed:\n{err}", src.display()));
    }
    Ok(failed.into_iter().map(|(s, _)| s).collect())
}

/// Files in `dir` with one of `exts`, sorted.
pub fn files(dir: &Path, exts: &[&str]) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file() && p.extension().is_some_and(|e| exts.iter().any(|x| e == *x)))
        .collect();
    v.sort();
    v
}

/// Write `path` atomically (temp file + rename).
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let tmp = path.with_extension(format!("tmp{}", std::process::id()));
    std::fs::write(&tmp, bytes).map_err(|e| format!("cannot write {}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("cannot write {}: {e}", path.display()))
}
