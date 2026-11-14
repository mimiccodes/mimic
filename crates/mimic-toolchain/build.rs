//! Compiles the C++ shim that exposes LLD and the Clang driver, and links
//! their static libraries. Inputs (see tools/deps.sh):
//!   LLVM_SYS_231_PREFIX  the LLVM install inkwell links (headers, clang libs)
//!   MIMIC_DEPS           where tools/deps.sh put LLD and the LLVM sources
//! Without them the stubs are built and `mimic` falls back to external tools.

use std::path::{Path, PathBuf};

const LLVM_VERSION: &str = "23.1.4";
const WASI_SDK: &str = "34";
const LLVM_MINGW: &str = "20261020";

fn main() {
    println!("cargo:rerun-if-changed=src/shim.cpp");
    println!("cargo:rerun-if-changed=src/stub.cpp");
    println!("cargo:rerun-if-env-changed=MIMIC_DEPS");
    println!("cargo:rerun-if-env-changed=LLVM_SYS_231_PREFIX");
    println!("cargo::rustc-check-cfg=cfg(mimic_toolchain_embedded)");

    let llvm = PathBuf::from(std::env::var("LLVM_SYS_231_PREFIX").unwrap_or_else(|_| "/opt/homebrew/opt/llvm".into()));
    let deps = std::env::var("MIMIC_DEPS")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".mimic/deps"));
    // LLD: part of the LLVM install when it was built by tools/build-llvm.sh,
    // otherwise the standalone build from tools/deps.sh
    let lld = if llvm.join("lib/liblldELF.a").is_file() { llvm.clone() } else { deps.join(format!("lld-{LLVM_VERSION}")) };
    println!("cargo:rustc-env=MIMIC_LLVM_PREFIX={}", llvm.display());
    println!("cargo:rustc-env=MIMIC_LLVM_MAJOR={}", LLVM_VERSION.split('.').next().unwrap());
    let driver = deps.join(format!("llvm-project-{LLVM_VERSION}.src/clang/tools/driver"));
    println!("cargo:rerun-if-changed={}", lld.join("lib").display());
    println!("cargo:rerun-if-changed={}", driver.display());

    bundle(&deps, &llvm);

    let ready = lld.join("lib/liblldELF.a").is_file()
        && driver.join("driver.cpp").is_file()
        && llvm.join("lib/libclangDriver.a").is_file();
    if !ready {
        println!(
            "cargo:warning=embedded LLD/Clang not available (missing {} or {}); run tools/deps.sh. Building stubs.",
            lld.display(),
            driver.display()
        );
        cc::Build::new().cpp(true).file("src/stub.cpp").compile("mimic_toolchain");
        return;
    }

    let mut b = cc::Build::new();
    b.cpp(true)
        .std("c++17")
        .warnings(false)
        .define("NDEBUG", None)
        .flag_if_supported("-fno-rtti")
        .flag_if_supported("-fno-exceptions")
        .flag_if_supported("-funwind-tables")
        .include(llvm.join("include"))
        .include(lld.join("include"))
        .file("src/shim.cpp");
    for f in ["driver.cpp", "cc1_main.cpp", "cc1as_main.cpp", "cc1gen_reproducer_main.cpp"] {
        let p = driver.join(f);
        if p.is_file() {
            b.file(p);
        }
    }
    b.compile("mimic_toolchain");

    // LLD
    println!("cargo:rustc-link-search=native={}", lld.join("lib").display());
    for l in ["lldMachO", "lldELF", "lldCOFF", "lldMinGW", "lldWasm", "lldCommon"] {
        println!("cargo:rustc-link-lib=static={l}");
    }
    // Clang: every library except those of the separate tools (tidy, clangd, …)
    println!("cargo:rustc-link-search=native={}", llvm.join("lib").display());
    for l in clang_libs(&llvm.join("lib")) {
        println!("cargo:rustc-link-lib=static={l}");
    }
    if cfg!(target_os = "macos") {
        println!("cargo:rustc-link-lib=dylib=c++");
        // the clang driver asks xcselect for the active SDK
        println!("cargo:rustc-link-lib=dylib=xcselect");
    } else {
        println!("cargo:rustc-link-lib=dylib=stdc++");
    }
    println!("cargo:rustc-cfg=mimic_toolchain_embedded");
}

fn clang_libs(dir: &Path) -> Vec<String> {
    const SKIP: &[&str] = &[
        "Tidy", "Daemon", "clangd", "Doc", "Move", "Query", "ReorderFields", "IncludeFixer", "IncludeCleaner",
        "ChangeNamespace", "ApplyReplacements", "Handle", "Interpreter", "Transformer",
        "DynamicASTMatchers", "Clangd", "ScalableStaticAnalysisTool",
    ];
    let mut v: Vec<String> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| n.starts_with("libclang") && n.ends_with(".a") && n.as_bytes().get(8).is_some_and(u8::is_ascii_uppercase))
        .filter(|n| !SKIP.iter().any(|s| n.contains(s)))
        .map(|n| n.trim_start_matches("lib").trim_end_matches(".a").to_string())
        .collect();
    v.sort();
    v
}

/// Pack the sources `mimic` builds target libraries from into compressed blobs
/// that `src/bundle.rs` embeds: musl, compiler-rt's builtins and Clang's
/// resource headers. Missing inputs give empty blobs (that target family then
/// reports itself unavailable).
fn bundle(deps: &Path, llvm: &Path) {
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let src = deps.join(format!("llvm-project-{LLVM_VERSION}.src"));
    let musl = deps.join("musl-1.2.5.tar.gz");
    println!("cargo:rerun-if-changed={}", musl.display());
    // musl ships as a .tar.gz already
    let bytes = std::fs::read(&musl).unwrap_or_default();
    std::fs::write(out.join("musl.tar.gz"), bytes).unwrap();
    pack(&src.join("compiler-rt/lib/builtins"), "builtins", &out.join("compiler-rt.tar.gz"));
    // wasi-libc (wasm32-wasip1), trimmed by tools/deps.sh
    pack(&deps.join(format!("wasi-sysroot-{WASI_SDK}")), "wasi-sysroot", &out.join("wasi.tar.gz"));
    // mingw-w64 for Windows (x86_64, aarch64), trimmed by tools/deps.sh
    pack(&deps.join(format!("mingw-sysroot-{LLVM_MINGW}")), "mingw-sysroot", &out.join("mingw.tar.gz"));
    let major = LLVM_VERSION.split('.').next().unwrap();
    pack(&llvm.join("lib/clang").join(major).join("include"), "include", &out.join("clang-include.tar.gz"));
}

fn pack(dir: &Path, name: &str, to: &Path) {
    if !dir.is_dir() {
        std::fs::write(to, b"").unwrap();
        return;
    }
    let f = std::fs::File::create(to).unwrap();
    let gz = flate2::write::GzEncoder::new(f, flate2::Compression::best());
    let mut t = tar::Builder::new(gz);
    t.mode(tar::HeaderMode::Deterministic);
    t.follow_symlinks(true);
    t.append_dir_all(name, dir).unwrap();
    t.into_inner().unwrap().finish().unwrap();
}
