//! Mbed TLS for the static Linux targets: their programs cannot load the
//! system's OpenSSL, so the runtime's TLS (`https://`, `tls::`) is linked in
//! from Mbed TLS instead. Built per target from the bundled sources on first
//! use, like compiler-rt.
use crate::bundle;
use crate::jobs::{self, Job};
use std::fs;
use std::path::{Path, PathBuf};

pub const VERSION: &str = "3.6.7";

pub struct Built {
    /// What code using it is compiled with: its headers, and the
    /// configuration the library was built with
    pub cflags: Vec<String>,
    /// `libmbedtls.a`: TLS, X.509 and crypto in one archive
    pub lib: PathBuf,
}

/// Additions to Mbed TLS's default configuration: locking, since a Mimic
/// server handles connections on many threads at once.
const USER_CONFIG: &str = "/* written by mimic */\n#define MBEDTLS_THREADING_C\n#define MBEDTLS_THREADING_PTHREAD\n";

/// Mbed TLS for `triple`, building it if needed. `target_flags` carries the
/// target and sysroot flags (the C library's headers).
pub fn ensure(cache: &Path, triple: &str, target_flags: &[String]) -> Result<Built, String> {
    let src = bundle::unpack(bundle::MBEDTLS, &cache.join(format!("src/mbedtls-{VERSION}")))?.join("mbedtls");
    let base = cache.join("mbedtls").join(VERSION);
    let config = base.join("config");
    let header = config.join("mimic_user_config.h");
    if fs::read_to_string(&header).ok().as_deref() != Some(USER_CONFIG) {
        fs::create_dir_all(&config).map_err(|e| format!("cannot create {}: {e}", config.display()))?;
        jobs::write_atomic(&header, USER_CONFIG.as_bytes())?;
    }
    let cflags = vec![
        format!("-I{}", src.join("include").display()),
        format!("-I{}", config.display()),
        "-DMBEDTLS_USER_CONFIG_FILE=\"mimic_user_config.h\"".to_string(),
    ];
    let dir = base.join(triple);
    let lib = dir.join("libmbedtls.a");
    if lib.is_file() {
        return Ok(Built { cflags, lib });
    }
    // a private work directory: other mimic processes may be building the
    // same library right now (the finished archive is moved into place)
    let work = dir.join(format!(".obj-{}", std::process::id()));
    let _ = fs::remove_dir_all(&work);
    let mut args = target_flags.to_vec();
    args.extend(["-O2", "-std=c99", "-fvisibility=hidden", "-ffunction-sections", "-fdata-sections", "-w"].map(String::from));
    args.extend(cflags.iter().cloned());
    args.push(format!("-I{}", src.join("library").display()));
    let jobs: Vec<Job> = jobs::files(&src.join("library"), &["c"])
        .into_iter()
        .map(|p| {
            let stem = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            Job { out: work.join(format!("{stem}.o")), src: p, args: args.clone() }
        })
        .collect();
    let failures = jobs::run(&jobs, true)?;
    if let Some((p, why)) = failures.first() {
        let _ = fs::remove_dir_all(&work);
        let first: Vec<&str> = why.lines().filter(|l| l.contains("error")).take(2).collect();
        return Err(format!("could not build Mbed TLS for {triple}: {}: {}", p.display(), first.join(" | ")));
    }
    let objs: Vec<PathBuf> = jobs.iter().map(|j| j.out.clone()).collect();
    let tmp = dir.join(format!(".libmbedtls-{}.a", std::process::id()));
    crate::write_archive(&tmp, crate::ArchiveKind::Gnu, &objs)?;
    fs::rename(&tmp, &lib).map_err(|e| format!("cannot write {}: {e}", lib.display()))?;
    let _ = fs::remove_dir_all(&work);
    Ok(Built { cflags, lib })
}
