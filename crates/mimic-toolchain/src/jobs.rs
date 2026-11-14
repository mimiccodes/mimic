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
/// With `tolerate`, failed jobs are skipped and returned (source, compiler
/// message) instead of failing.
pub fn run(jobs: &[Job], tolerate: bool) -> Result<Vec<(PathBuf, String)>, String> {
    let me = std::env::current_exe().map_err(|e| e.to_string())?;
    let next = AtomicUsize::new(0);
    let failed: Mutex<Vec<(PathBuf, String)>> = Mutex::new(Vec::new());
    // could not run at all: always an error, even when tolerating failures
    let hard: Mutex<Vec<(PathBuf, String)>> = Mutex::new(Vec::new());
    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
    std::thread::scope(|s| {
        for _ in 0..threads {
            s.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(j) = jobs.get(i) else { break };
                    if !hard.lock().unwrap().is_empty() || (!failed.lock().unwrap().is_empty() && !tolerate) {
                        break;
                    }
                    if let Some(d) = j.out.parent() {
                        let _ = std::fs::create_dir_all(d);
                    }
                    // a job that could not start or was killed (system load,
                    // process limits) is retried; it is never "not applicable"
                    let mut attempt = 0;
                    loop {
                        attempt += 1;
                        let out = Command::new(&me)
                            .env(crate::RAW_CC_ENV, "1")
                            .arg("cc")
                            .args(&j.args)
                            .arg("-c")
                            .arg(&j.src)
                            .arg("-o")
                            .arg(&j.out)
                            .output();
                        let infra = match &out {
                            Ok(o) if o.status.success() => break,
                            Ok(o) => o.status.code().is_none(), // killed by a signal
                            Err(_) => true,                     // could not spawn
                        };
                        if infra && attempt < 4 {
                            std::thread::sleep(std::time::Duration::from_millis(200 * attempt));
                            continue;
                        }
                        let msg = match out {
                            Ok(o) => String::from_utf8_lossy(&o.stderr).into_owned(),
                            Err(e) => e.to_string(),
                        };
                        if infra {
                            hard.lock().unwrap().push((j.src.clone(), msg));
                        } else {
                            failed.lock().unwrap().push((j.src.clone(), msg));
                        }
                        break;
                    }
                }
            });
        }
    });
    if let Some((src, err)) = hard.into_inner().unwrap().first() {
        return Err(format!("could not run the compiler for {} (system busy?): {err}", src.display()));
    }
    let failed = failed.into_inner().unwrap();
    if !tolerate && let Some((src, err)) = failed.first() {
        return Err(format!("compiling {} failed:\n{err}", src.display()));
    }
    Ok(failed)
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

/// Mark a finished cache directory: `marker` lists every file under `dir`
/// with its size. Caches can live where the system deletes old files (macOS
/// purges unused files from the temporary directory after a few days), so a
/// marker alone proves nothing; [`intact`] checks the files are still there.
pub fn seal(dir: &std::path::Path, marker: &str) -> Result<(), String> {
    fn walk(base: &std::path::Path, d: &std::path::Path, out: &mut String) -> std::io::Result<()> {
        for e in std::fs::read_dir(d)? {
            let e = e?;
            let p = e.path();
            let ft = e.file_type()?;
            if ft.is_dir() {
                walk(base, &p, out)?;
            } else if ft.is_file() {
                let rel = p.strip_prefix(base).unwrap_or(&p);
                out.push_str(&format!("{}\t{}\n", e.metadata()?.len(), rel.display()));
            }
        }
        Ok(())
    }
    let mut list = String::new();
    walk(dir, dir, &mut list).map_err(|e| format!("cannot list {}: {e}", dir.display()))?;
    write_atomic(&dir.join(marker), list.as_bytes())
}

/// Is the cache directory complete: its marker present and every file it
/// lists still there with the same size?
pub fn intact(dir: &std::path::Path, marker: &str) -> bool {
    let Ok(list) = std::fs::read_to_string(dir.join(marker)) else { return false };
    if list.is_empty() {
        return false; // an old-style empty marker: rebuild once and seal properly
    }
    list.lines().all(|l| {
        let Some((size, rel)) = l.split_once('\t') else { return false };
        std::fs::metadata(dir.join(rel)).is_ok_and(|m| m.len().to_string() == size)
    })
}
