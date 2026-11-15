//! `mimic` — compiler, project manager, code checker and package manager.

// Diagnostics are rich (labels, fixes) and only built on the error path.
#![allow(clippy::result_large_err)]

mod audit;
mod cc;
mod cimport;
mod doc;
mod driver;
mod edit;
mod dap;
mod engine;
mod lsp;
mod man;
mod mcp;
mod modfile;
mod profile;
mod repl;
mod report;
mod targets;
mod toolchain;
mod web;

use clap::{Args, Parser, Subcommand};
use driver::{Input, Profile};
use mimic_check::Program;
use mimic_project::{MOD_FILE, Project, packages};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use report::Report;
use targets::Target;

#[derive(Parser)]
#[command(
    name = "mimic",
    version = concat!(env!("CARGO_PKG_VERSION"), " (", env!("MIMIC_COMMIT"), ", LLVM ", env!("MIMIC_LLVM_VERSION"), ")"),
    about = "MIMIC (https://mimic.codes/) :: A Compiled, Array-based, Massively Parallel, Memory-safe Programming Language by Harcorp Industries (https://harcorp.ai)",
    long_about = "MIMIC (https://mimic.codes/) :: A Compiled, Array-based, Massively Parallel, Memory-safe\n\
                  Programming Language by Harcorp Industries (https://harcorp.ai)\n\n\
                  Every primitive has a glyph and a word: `⍳ 10 ▷ +/` is `iota 10 |> sum`.\n\
                  Run `mimic glyphs` for the full table.",
    after_help = "Examples:\n  mimic create hello && cd hello && mimic run\n  mimic run script.mim --release\n  mimic build                     # fastest binary (release is the default)\n  mimic build --target x86_64-linux-gnu\n  mimic install github.com/harcorp/ai.git\n\n\
                  Made With Love By Harsh Kumar"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Args, Clone)]
struct ProfileArgs {
    /// Debug profile: no optimisation, debug info, leak check on exit
    /// (default for `run`)
    #[arg(long, conflicts_with = "release")]
    debug: bool,
    /// Release profile: fastest code — -O3, host CPU features, nets on
    /// (default for `build`)
    #[arg(long)]
    release: bool,
    /// Do not compile pure functions to interaction nets (everything native)
    #[arg(long)]
    no_net: bool,
    /// Generate code for this CPU model, e.g. apple-m3, x86-64-v3, cortex-a72;
    /// `native` = this machine (the default for native builds); `portable` =
    /// the oldest model of the family (apple-m1, x86-64-v2), runs on all of them
    #[arg(long, value_name = "CPU")]
    cpu: Option<String>,
    /// No fused multiply-add: `a * b + c` rounds twice, giving bit-identical
    /// results on every CPU. By default Mimic fuses them, like C compilers
    #[arg(long)]
    strict_math: bool,
    /// Faster float arithmetic: sums and products may be regrouped (and so
    /// vectorised), divisions done by reciprocals, math functions
    /// approximated. Results can change in the last bits
    #[arg(long, conflicts_with = "strict_math")]
    fast_math: bool,
}

/// `default_release`: `build` produces the fastest binary unless told otherwise;
/// `run` defaults to debug for quick iteration with the leak check.
fn profile_of(input: &Input, args: ProfileArgs, default_release: bool) -> Profile {
    let release = args.release || (default_release && !args.debug);
    let mut p = Profile::for_input(input, release);
    p.net = !args.no_net;
    p.cpu = args.cpu;
    p.strict_math = args.strict_math;
    p.fast_math = args.fast_math;
    p
}

#[derive(Args)]
struct KindArgs {
    /// Create a binary project (src/main.mim) — the default
    #[arg(long)]
    bin: bool,
    /// Create a library project (src/lib.mim); combine with --bin for both
    #[arg(long)]
    lib: bool,
}

impl KindArgs {
    fn kinds(&self) -> (bool, bool) {
        if !self.bin && !self.lib { (true, false) } else { (self.bin, self.lib) }
    }
}

#[derive(Subcommand)]
enum Cmd {
    /// Create a new project in a new directory
    Create {
        name: String,
        #[command(flatten)]
        kind: KindArgs,
    },
    /// Create a project in the current directory
    Init {
        /// Project name (defaults to the directory name)
        #[arg(long)]
        name: Option<String>,
        #[command(flatten)]
        kind: KindArgs,
    },
    /// Build and run the current project, or a single .mim file
    Run {
        /// A standalone .mim file to run instead of the current project
        file: Option<PathBuf>,
        #[command(flatten)]
        profile: ProfileArgs,
        /// On a compile error, print the diagnostics as JSON to stderr
        /// (program output stays on stdout)
        #[arg(long)]
        emit_json: bool,
        /// Run it as a web page (WebAssembly): build, then serve it at
        /// http://localhost:<port>/ (see `mimic build --web`)
        #[arg(long)]
        web: bool,
        /// The port for --web
        #[arg(long, default_value_t = 8080)]
        port: u16,
        /// Show a `ui` program in this terminal instead of the browser
        #[arg(long, conflicts_with_all = ["web", "window"])]
        terminal: bool,
        /// Show a `ui` program in a native window instead of the browser
        #[arg(long, conflicts_with = "web")]
        window: bool,
        /// Arguments passed to the program (after `--`)
        #[arg(last = true)]
        args: Vec<String>,
    },
    /// Run the tests (`test "…" { }` blocks, and src/test.mim) of the
    /// current project or a file, each in a process of its own, in parallel
    Test {
        /// A .mim file to test instead of the current project; other words
        /// keep only the tests whose names contain one of them
        args: Vec<String>,
        #[command(flatten)]
        profile: ProfileArgs,
    },
    /// Format .mim files: words for primitives and operators (or glyphs),
    /// indentation by nesting, tidy whitespace; comments and line breaks stay
    Fmt {
        /// Files to format (default: every .mim file of the current project)
        files: Vec<PathBuf>,
        /// Write primitives and operators as glyphs (⍳ ¨ ▷ ≠ ∧) instead of words
        #[arg(long)]
        glyphs: bool,
        /// Change nothing: list the files that are not formatted (exit code 1 if any)
        #[arg(long)]
        check: bool,
    },
    /// Write documentation of the public functions, types and modules of
    /// the current project (or a file) from their signatures and the `//`
    /// comments above them: one HTML page (target/doc/index.html)
    Doc {
        file: Option<PathBuf>,
        /// Document the standard library instead
        #[arg(long)]
        std: bool,
        /// Print the documentation as JSON instead (for editors and tools)
        #[arg(long)]
        json: bool,
        /// Where to write index.html
        #[arg(long, value_name = "DIR")]
        out: Option<PathBuf>,
    },
    /// Debug the current project (or a file) in lldb: a debug build, with
    /// Mimic's values (numbers, strings, lists, records) shown as Mimic
    /// writes them
    Lldb {
        file: Option<PathBuf>,
        /// Arguments passed to the program (after `--`)
        #[arg(last = true)]
        args: Vec<String>,
    },
    /// Compile the current project (or a single file)
    Build {
        file: Option<PathBuf>,
        #[command(flatten)]
        profile: ProfileArgs,
        /// A web page: build for wasm32-wasi and write <name>-web/ with
        /// index.html and app.wasm, ready for any static web server
        #[arg(long)]
        web: bool,
        /// Cross-compile for an LLVM target triple (vendor optional), e.g. x86_64-linux-musl, aarch64-linux, x86_64-apple-macosx13.0; `mimic targets` lists them
        #[arg(long)]
        target: Option<String>,
        /// Also build every target listed in build.mim `targets`
        #[arg(long)]
        all_targets: bool,
        /// Write the LLVM IR next to the object file (target/<profile>/<name>.ll)
        #[arg(long)]
        emit_ir: bool,
        /// Write the generated interaction nets (target/<profile>/<name>.mnet)
        #[arg(long)]
        emit_net: bool,
        /// Write a C header for the `extern "C" fn`s (target/<profile>/<name>.h)
        #[arg(long)]
        emit_header: bool,
        /// A library project as a shared library (.dll, .so, .dylib) exporting its `extern "C" fn`s
        #[arg(long)]
        shared: bool,
        /// Print the result as JSON on stdout (see docs/diagnostics.md)
        #[arg(long)]
        emit_json: bool,
    },
    /// Check the project (or a file) for errors without compiling
    Check {
        file: Option<PathBuf>,
        /// Print the result as JSON on stdout, for editors and agents
        /// (schema: docs/diagnostics.md)
        #[arg(long)]
        emit_json: bool,
        /// Apply every machine-applicable fix to the sources, then re-check
        #[arg(long)]
        fix: bool,
    },
    /// List what LLVM can target (architectures, OSes, environments) and what builds executables
    Targets {
        /// List the CPU models of an architecture (e.g. `--cpus aarch64`), for `--cpu`
        #[arg(long, value_name = "ARCH")]
        cpus: Option<String>,
        /// List the CPU features of an architecture
        #[arg(long, value_name = "ARCH")]
        features: Option<String>,
        /// Show how a target is understood (LLVM triple) and what it can build
        #[arg(long, value_name = "TARGET")]
        check: Option<String>,
        /// Print as JSON
        #[arg(long)]
        json: bool,
    },
    /// Read the manual: a chapter, a standard function (`mimic man fs::read`), a glyph (`mimic man ⌿`), an error code
    Man {
        /// A page, chapter number, function, module, glyph or its word, or error code; nothing for the contents
        topic: Vec<String>,
        /// Every line of the manual that mentions this
        #[arg(long, value_name = "TEXT")]
        search: Option<String>,
        /// Run the page's examples and compare their output
        #[arg(long)]
        run: bool,
        /// List the pages (of one kind: tutorial, guide, reference, function, module, type, glyph, error)
        #[arg(long, value_name = "KIND", num_args = 0..=1, default_missing_value = "")]
        list: Option<String>,
        /// Print as JSON
        #[arg(long)]
        json: bool,
    },
    /// Debugging from editors (Debug Adapter Protocol, over stdin and stdout): builds the program and runs it under lldb-dap
    Dap,
    /// Tools for AI agents over the Model Context Protocol (stdin and stdout): check, build, run, test, the manual, symbols
    Mcp {
        /// The project (default: the one in this folder or above)
        #[arg(long, value_name = "DIR")]
        project: Option<PathBuf>,
        /// Allow files outside the project
        #[arg(long)]
        allow_outside: bool,
        /// Allow installing libraries (into ~/.mimic)
        #[arg(long)]
        allow_install: bool,
        /// Allow running programs that use the network (http, tcp, udp, ws, tls) or run other programs
        #[arg(long)]
        allow_network: bool,
        /// Only reading, checking and looking up: no writing, building or running
        #[arg(long)]
        read_only: bool,
        /// No running programs or tests
        #[arg(long)]
        no_run: bool,
    },
    /// Mimic's own terminal editor: errors as you type, glyphs, the manual, run, test, audit
    Edit {
        /// A file to open, or a project folder (default: the project here)
        path: Option<PathBuf>,
    },
    /// Type Mimic a line at a time and see the values (`:help` lists the commands)
    Repl,
    /// Run the program and show where its time goes: per function, per core, CPU and GPU, memory
    Profile {
        /// A .mim file (default: the project here)
        file: Option<PathBuf>,
        /// Print the report as JSON
        #[arg(long)]
        json: bool,
        /// Samples per second on each thread (default 1000)
        #[arg(long)]
        hz: Option<u32>,
        /// How many functions to list
        #[arg(long, default_value_t = 15)]
        top: usize,
        /// Arguments for the program (after `--`)
        #[arg(last = true)]
        args: Vec<String>,
    },
    /// The language server for editors (VS Code, Neovim, Helix, Zed, …), over stdin and stdout
    Lsp {
        /// Accepted for editors that pass it; stdin and stdout are always used
        #[arg(long, hide = true)]
        stdio: bool,
    },
    /// Where code runs and why: in parallel, on the GPU, as a task, or in order (`mimic why mandelbrot`)
    Why {
        /// A function (`mandelbrot`, `geometry::area`); nothing for the whole program
        name: Option<String>,
        /// A standalone file instead of the project in this folder
        #[arg(long = "in", value_name = "FILE")]
        input: Option<PathBuf>,
        /// Print as JSON
        #[arg(long)]
        json: bool,
    },
    /// Quality and security findings (`mimic man audit` lists the rules)
    Audit {
        /// A standalone file instead of the project in this folder
        #[arg(long = "in", value_name = "FILE")]
        input: Option<PathBuf>,
        /// text (default), json, or sarif (for code scanning)
        #[arg(long, default_value = "text")]
        format: String,
        /// Exit with 1 when a (new) finding is at least this severe: error, warning, note, or never
        #[arg(long, default_value = "error")]
        fail_on: String,
        /// Count only findings not in this baseline file
        #[arg(long, value_name = "FILE")]
        baseline: Option<PathBuf>,
        /// Write the current findings to the baseline file (default .mimic-audit.json)
        #[arg(long)]
        update_baseline: bool,
    },
    /// Ask the analysis engine about the project, as JSON (`mimic query definition file=src/main.mim line=3 column=9`)
    Query {
        /// What to ask: check, symbols, definition, hover, references, outline, docs, effects, stats
        method: Option<String>,
        /// Parameters as key=value (file=… line=… column=… name=… query=… kind=… limit=…)
        params: Vec<String>,
        /// A standalone file to analyse instead of the project in this folder
        #[arg(long = "in", value_name = "FILE")]
        input: Option<PathBuf>,
        /// Keep running: answer one JSON request per line on stdin ({"id", "method", "params"})
        #[arg(long)]
        serve: bool,
    },
    /// Explain an error code (`mimic explain E0401`), or list them all
    Explain {
        /// A code (E0401) or its name (type-mismatch); omit to list every code
        code: Option<String>,
        /// Print as JSON
        #[arg(long)]
        emit_json: bool,
    },
    /// Manage the module index (mod.mim)
    Mod {
        #[command(subcommand)]
        cmd: ModCmd,
    },
    /// Install a library from git into ~/.mimic/libraries; with no URL, inside a project, install its dependencies
    Install {
        /// Git URL, e.g. github.com/harcorp/ai.git, optionally @tag, @branch or @commit
        source: Option<String>,
    },
    /// List globally installed libraries
    List,
    /// Update all installed libraries, or one
    Update { library: Option<String> },
    /// Remove a globally installed library
    Uninstall { library: String },
    /// Show the glyph ↔ word ↔ digraph table
    Glyphs,
    /// Interaction-net tools (runtime development)
    Net {
        #[command(subcommand)]
        cmd: NetCmd,
    },
}

#[derive(Subcommand)]
enum NetCmd {
    /// Reduce a raw interaction-net file (.mnet) and print the value of @main
    Run {
        file: PathBuf,
        #[command(flatten)]
        profile: ProfileArgs,
        /// Print interaction count and speed
        #[arg(long)]
        stats: bool,
        /// Worker threads (default: all CPUs)
        #[arg(long)]
        threads: Option<usize>,
    },
    /// Parse a .mnet file and print it back in canonical form
    Fmt { file: PathBuf },
}

#[derive(Subcommand)]
enum ModCmd {
    /// Create mod.mim for the current project
    Init,
    /// Regenerate mod.mim from the sources
    Update,
    /// List the project's modules (--all: also functions and dependencies)
    List {
        #[arg(long)]
        all: bool,
    },
    /// Delete a module: removes its block (or its file, for file modules)
    Delete {
        /// Module path, e.g. `speech` or `speech::inner`
        module: String,
        /// Actually delete; without this flag only shows what would be removed
        #[arg(long)]
        yes: bool,
    },
}

fn main() -> ExitCode {
    // The embedded tools: `mimic cc …` is clang, `mimic ld.lld …` (and
    // ld64.lld, lld-link, wasm-ld) is LLD. They take the tools' own flags, so
    // they are dispatched before argument parsing.
    let argv: Vec<String> = std::env::args().collect();
    // invoked through a tool link (`ld64.lld` → mimic): run that tool
    let me = argv.first().and_then(|a| Path::new(a).file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    if mimic_toolchain::LLD_FLAVOURS.contains(&me.as_str()) {
        return ExitCode::from(mimic_toolchain::lld_main(&argv).clamp(0, 255) as u8);
    }
    if let Some(tool) = argv.get(1).map(String::as_str) {
        if tool == "cc" || tool == "c++" {
            return ExitCode::from(cc::main(tool == "c++", &argv[2..]).clamp(0, 255) as u8);
        }
        if mimic_toolchain::LLD_FLAVOURS.contains(&tool) {
            return ExitCode::from(mimic_toolchain::lld_main(&argv[1..]).clamp(0, 255) as u8);
        }
    }
    let cli = Cli::parse();
    match run(cli) {
        Ok(code) => code,
        Err(e) => {
            let e = e.trim_end();
            if e.starts_with("error") {
                eprintln!("{e}");
            } else {
                eprintln!("error: {e}");
            }
            ExitCode::FAILURE
        }
    }
}

fn status(verb: &str, msg: impl AsRef<str>) {
    eprintln!("{verb:>12} {}", msg.as_ref());
}

fn project_here() -> Result<Project, String> {
    match driver::input_from(None)? {
        Input::Project(p) => Ok(*p),
        Input::File(_) => unreachable!(),
    }
}

fn run(cli: Cli) -> Result<ExitCode, String> {
    if let Some(code) = for_each_member(&cli.cmd)? {
        return Ok(code);
    }
    match cli.cmd {
        Cmd::Create { name, kind } => {
            let (bin, lib) = kind.kinds();
            let dir = PathBuf::from(&name);
            if dir.exists() {
                return Err(format!("destination `{}` already exists", dir.display()));
            }
            let pname = Path::new(&name).file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or(name.clone());
            mimic_project::scaffold(&dir, &pname, bin, lib)?;
            write_initial_modfile(&dir);
            status("Created", format!("{} `{pname}` project", kind_label(bin, lib)));
            Ok(ExitCode::SUCCESS)
        }
        Cmd::Init { name, kind } => {
            let (bin, lib) = kind.kinds();
            let dir = std::env::current_dir().map_err(|e| e.to_string())?;
            let name = name.unwrap_or_else(|| dir.file_name().map(|s| s.to_string_lossy().replace('-', "_")).unwrap_or_default());
            mimic_project::scaffold(&dir, &name, bin, lib)?;
            write_initial_modfile(&dir);
            status("Created", format!("{} `{name}` project", kind_label(bin, lib)));
            Ok(ExitCode::SUCCESS)
        }
        Cmd::Check { file, emit_json, fix } => {
            let check_once = || -> Result<(String, Program), Report> {
                let input = driver::input_from(file.as_deref())?;
                let p = driver::load(&input)?;
                if let Input::Project(proj) = &input {
                    modfile::sync(proj, &p)?;
                }
                Ok((input.name(), p))
            };
            let mut checked = check_once();
            let mut fixed = 0;
            // fixes can uncover further errors (the parser stops at the first one)
            for _ in 0..8 {
                let Err(r) = &checked else { break };
                if !fix {
                    break;
                }
                let n = r.apply_fixes()?;
                if n == 0 {
                    break;
                }
                fixed += n;
                checked = check_once();
            }
            if fixed > 0 && !emit_json {
                status("Fixed", format!("{fixed} problem(s)"));
            }
            match checked {
                Ok((name, p)) => {
                    let nfiles = p.files.iter().filter(|f| f.package == p.root_package).count();
                    if emit_json {
                        let mut v = Report::default().json("check");
                        v["fixed"] = fixed.into();
                        v["package"] = name.clone().into();
                        v["files"] = nfiles.into();
                        v["functions"] = p.fns.len().into();
                        println!("{v}");
                    } else {
                        status("Checked", format!("`{name}`: no errors ({nfiles} file(s), {} function(s))", p.fns.len()));
                    }
                    Ok(ExitCode::SUCCESS)
                }
                Err(r) => {
                    if emit_json {
                        let mut v = r.json("check");
                        v["fixed"] = fixed.into();
                        println!("{v}");
                        return Ok(ExitCode::FAILURE);
                    }
                    fail(r, false, "check", false)
                }
            }
        }
        Cmd::Build { file, profile, web, target, all_targets, emit_ir, emit_net, emit_header, shared, emit_json } => {
            let built = (|| -> Result<Vec<serde_json::Value>, Report> {
                let input = driver::input_from(file.as_deref())?;
                let mut prof = profile_of(&input, profile.clone(), true);
                prof.emit_header = emit_header;
                prof.shared = shared;
                let mut targets = vec![match &target {
                    _ if web => Target::parse("wasm32-wasi")?,
                    Some(t) => Target::parse(t)?,
                    None => Target::host(),
                }];
                if all_targets
                    && let Input::Project(p) = &input {
                        for t in &p.build.targets {
                            targets.push(Target::parse(t)?);
                        }
                    }
                let mut out = Vec::new();
                for t in &targets {
                    if !emit_json {
                        status("Compiling", format!("{} ({}, {})", input.name(), prof.name(), t.name()));
                    }
                    let a = driver::build(&input, &prof, t, emit_ir, emit_net)?;
                    if web {
                        let dir = web::package(&a.path).map_err(|e| Report::plain(&mimic_syntax::codes::BUILD, format!("cannot write the web page: {e}")))?;
                        if !emit_json {
                            status("Finished", format!("web page → {}", shown(&dir)));
                        }
                        out.push(serde_json::json!({ "target": t.name(), "profile": prof.name(), "kind": "web", "path": dir.display().to_string() }));
                        continue;
                    }
                    if !emit_json {
                        status("Finished", format!("{} → {}", a.kind.describe(), shown(&a.path)));
                        if let Some(n) = &a.note {
                            eprintln!("     warning: no executable: {n}");
                        }
                    }
                    out.push(serde_json::json!({
                        "target": t.name(),
                        "profile": prof.name(),
                        "kind": a.kind.json(),
                        "note": a.note,
                        "path": a.path.display().to_string(),
                    }));
                }
                Ok(out)
            })();
            match built {
                Ok(arts) => {
                    if emit_json {
                        let mut v = Report::default().json("build");
                        v["artifacts"] = arts.into();
                        println!("{v}");
                    }
                    Ok(ExitCode::SUCCESS)
                }
                Err(r) => fail(r, emit_json, "build", false),
            }
        }
        Cmd::Explain { code, emit_json } => explain(code.as_deref(), emit_json),
        Cmd::Profile { file, json, hz, top, args } => profile::run(profile::Options { file, json, hz, top, args }),
        Cmd::Repl => {
            repl::run()?;
            Ok(ExitCode::SUCCESS)
        }
        Cmd::Edit { path } => {
            edit::run(path.as_deref())?;
            Ok(ExitCode::SUCCESS)
        }
        Cmd::Audit { input, format, fail_on, baseline, update_baseline } => audit_cmd(input, &format, &fail_on, baseline, update_baseline),
        Cmd::Why { name, input, json } => why_cmd(name, input, json),
        Cmd::Mcp { project, allow_outside, allow_install, allow_network, read_only, no_run } => {
            mcp::serve(project, mcp::Access { allow_outside, allow_install, allow_network, read_only, no_run })?;
            Ok(ExitCode::SUCCESS)
        }
        Cmd::Dap => {
            dap::serve()?;
            Ok(ExitCode::SUCCESS)
        }
        Cmd::Lsp { .. } => {
            lsp::serve()?;
            Ok(ExitCode::SUCCESS)
        }
        Cmd::Man { topic, search, run, list, json } => {
            let ok = man::command(&topic, search.as_deref(), run, list.as_deref(), json)?;
            Ok(if ok { ExitCode::SUCCESS } else { ExitCode::FAILURE })
        }
        Cmd::Query { method, params, input, serve } => {
            let mut eng = engine::Engine::open(input.as_deref()).map_err(|e| Report::from_text(&mimic_syntax::codes::IO, &e))?;
            if serve {
                engine::serve(&mut eng);
                return Ok(ExitCode::SUCCESS);
            }
            let method = method.unwrap_or_else(|| "check".into());
            let params = engine::params_from_words(&params).map_err(|e| Report::from_text(&mimic_syntax::codes::IO, &e))?;
            match eng.query(&method, &params) {
                Ok(v) => {
                    println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
                    let failed = method == "check" && v.get("success") == Some(&serde_json::Value::Bool(false));
                    Ok(if failed { ExitCode::FAILURE } else { ExitCode::SUCCESS })
                }
                Err(e) => {
                    println!("{}", serde_json::json!({ "error": e }));
                    Ok(ExitCode::FAILURE)
                }
            }
        }
        Cmd::Targets { cpus, features, check, json } => targets_cmd(cpus.as_deref(), features.as_deref(), check.as_deref(), json),
        Cmd::Run { file, profile, emit_json, web, port, terminal, window, args } => {
            let built = (|| -> Result<driver::Artifact, Report> {
                let input = driver::input_from(file.as_deref())?;
                if !input.is_bin() {
                    return Err(Report::plain(
                        &mimic_syntax::codes::ENTRY_MODULE,
                        "this is a library project; there is nothing to run (add `bin` to `kind` in mim.mim)",
                    ));
                }
                let prof = profile_of(&input, profile, false);
                let target = if web { Target::parse("wasm32-wasi")? } else { Target::host() };
                driver::build(&input, &prof, &target, false, false)
            })();
            let a = match built {
                Ok(a) => a,
                Err(r) => return fail(r, emit_json, "run", true),
            };
            if web {
                let dir = web::package(&a.path).map_err(|e| format!("cannot write the web page: {e}"))?;
                web::serve(&dir, port).map_err(|e| format!("cannot serve the page on port {port}: {e}"))?;
                return Ok(ExitCode::SUCCESS);
            }
            status("Running", shown(&a.path));
            let mut child = std::process::Command::new(&a.path);
            child.args(&args);
            if terminal {
                child.env("MIMIC_UI", "terminal");
            } else if window {
                child.env("MIMIC_UI", "window");
            }
            let st = child.status().map_err(|e| format!("cannot run {}: {e}", a.path.display()))?;
            Ok(ExitCode::from(st.code().unwrap_or(1).clamp(0, 255) as u8))
        }
        Cmd::Test { args, profile } => test_cmd(args, profile),
        Cmd::Fmt { files, glyphs, check } => fmt_cmd(files, glyphs, check),
        Cmd::Doc { file, std, json, out } => doc_cmd(file, std, json, out),
        Cmd::Lldb { file, args } => lldb_cmd(file, args),
        Cmd::Mod { cmd } => mod_cmd(cmd),
        Cmd::Install { source: Some(source) } => {
            status("Installing", &source);
            let i = packages::install(&source)?;
            status("Installed", format!("{} v{} → {}", i.name, i.version, i.dir.display()));
            match &i.rev {
                Some(r) => eprintln!("             add it to a project's mim.mim:  dependencies {{ {} = {{ git = {}, rev = {} }} }}", i.name, mimic_syntax::config::quote(&i.source), mimic_syntax::config::quote(r)),
                None => eprintln!("             add it to a project's mim.mim:  dependencies {{ {} = {{ git = {} }} }}", i.name, mimic_syntax::config::quote(&i.source)),
            }
            Ok(ExitCode::SUCCESS)
        }
        Cmd::Install { source: None } => install_dependencies(),
        Cmd::List => {
            let libs = packages::list()?;
            if libs.is_empty() {
                println!("no libraries installed in {}", packages::libraries_dir().display());
            }
            for l in libs {
                println!("{:<20} {:<10} {}", l.name, l.version, l.source);
            }
            Ok(ExitCode::SUCCESS)
        }
        Cmd::Update { library } => {
            let libs = match library {
                Some(n) => vec![packages::find(&n)?],
                None => packages::list()?,
            };
            if libs.is_empty() {
                status("Updated", "nothing to update");
            }
            for l in libs {
                let (a, b) = packages::update(&l)?;
                if a == b { status("Unchanged", format!("{} ({a})", l.name)) } else { status("Updated", format!("{} {a} → {b}", l.name)) }
            }
            Ok(ExitCode::SUCCESS)
        }
        Cmd::Uninstall { library } => {
            let dir = packages::uninstall(&library)?;
            status("Removed", format!("{library} ({})", dir.display()));
            Ok(ExitCode::SUCCESS)
        }
        Cmd::Net { cmd } => net_cmd(cmd),
        Cmd::Glyphs => {
            print!("{}", mimic_syntax::glyphs::table());
            Ok(ExitCode::SUCCESS)
        }
    }
}

/// At the root of a workspace (a `mim.mim` with `workspace { members = [...] }`
/// and no project): `check`, `build`, `test`, `fmt`, `install` and `mod update`
/// run in every member, in order; other project commands say to pick one.
fn for_each_member(cmd: &Cmd) -> Result<Option<ExitCode>, String> {
    let each = match cmd {
        Cmd::Check { file: None, .. } | Cmd::Build { file: None, .. } | Cmd::Install { source: None } => true,
        // a file to test is named first; words only filter the tests
        Cmd::Test { args, .. } => !args.first().is_some_and(|a| a.ends_with(".mim")),
        Cmd::Fmt { files, .. } => files.is_empty(),
        Cmd::Mod { cmd: ModCmd::Update } => true,
        Cmd::Run { file: None, .. } | Cmd::Doc { file: None, .. } | Cmd::Mod { .. } | Cmd::Lldb { file: None, .. } => false,
        _ => return Ok(None),
    };
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    let Some((root, members)) = mimic_project::workspace_root(&cwd) else { return Ok(None) };
    let names: Vec<String> = members.iter().map(|m| m.strip_prefix(&root).unwrap_or(m).display().to_string()).collect();
    if !each {
        return Err(format!("{} is a workspace: run this inside one of its members ({})", root.display(), names.join(", ")));
    }
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut failed = Vec::new();
    for (m, name) in members.iter().zip(&names) {
        status("Member", name);
        let ok = std::process::Command::new(&exe).args(&args).current_dir(m).status().map_err(|e| e.to_string())?.success();
        if !ok {
            failed.push(name.clone());
        }
    }
    if failed.is_empty() {
        status("Workspace", format!("{} member(s): all passed", members.len()));
        Ok(Some(ExitCode::SUCCESS))
    } else {
        status("Workspace", format!("failed in {}", failed.join(", ")));
        Ok(Some(ExitCode::FAILURE))
    }
}

/// `mimic install` in a project: every git dependency that is not installed
/// yet (and theirs), at the `rev` mim.mim asks for, or at the commit mod.mim
/// locked when it asks for none.
fn install_dependencies() -> Result<ExitCode, String> {
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    let root = mimic_project::find_root(&cwd)
        .ok_or("not inside a project: give a git URL (`mimic install github.com/<owner>/<library>.git`)".to_string())?;
    let proj = Project::load(&root)?;
    let locked = modfile::locked(&proj);
    let mut queue: Vec<(PathBuf, mimic_project::Dependency, bool)> =
        proj.manifest.dependencies.iter().rev().map(|d| (root.clone(), d.clone(), true)).collect();
    let mut seen = std::collections::BTreeSet::new();
    let (mut installed, mut present) = (0, 0);
    while let Some((from, dep, direct)) = queue.pop() {
        if !seen.insert(dep.name.clone()) {
            continue;
        }
        let holder = Project::load(&from)?;
        let dir = match holder.dep_dir(&dep) {
            Ok(dir) => {
                present += 1;
                dir
            }
            Err(e) => match &dep.source {
                mimic_project::DepSource::Git { url, rev } => {
                    let commit = if direct && rev.is_none() { locked.get(&dep.name).map(String::as_str) } else { None };
                    let at = rev.as_deref().or(commit.map(|c| &c[..c.len().min(12)])).unwrap_or("the default branch");
                    status("Installing", format!("{} ({url} at {at})", dep.name));
                    let i = packages::install_at(&packages::normalize_url(url), rev.as_deref(), commit)?;
                    if i.name != dep.name {
                        return Err(format!("{url} provides library `{}`, but mim.mim calls it `{}`", i.name, dep.name));
                    }
                    installed += 1;
                    i.dir
                }
                _ => return Err(format!("{e}\n  (to install it automatically, give its git URL in mim.mim: `{} = {{ git = \"…\" }}`)", dep.name)),
            },
        };
        for d in Project::load(&dir)?.manifest.dependencies.iter().rev() {
            queue.push((dir.clone(), d.clone(), false));
        }
    }
    status("Installed", format!("{installed} new, {present} already present"));
    Ok(ExitCode::SUCCESS)
}

/// `mimic why`: the decisions about where code runs, by function and line.
fn why_cmd(name: Option<String>, input: Option<PathBuf>, json: bool) -> Result<ExitCode, String> {
    let mut eng = engine::Engine::open(input.as_deref())?;
    let check = eng.query("check", &serde_json::json!({}))?;
    if check["success"] != true {
        let first = check["diagnostics"][0]["rendered"].as_str().unwrap_or("").to_string();
        return Err(format!("the program has errors; `mimic check` first\n{first}"));
    }
    let mut params = serde_json::json!({});
    if let Some(n) = &name {
        params["name"] = n.clone().into();
    }
    let v = eng.query("where", &params)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
        return Ok(ExitCode::SUCCESS);
    }
    let list = v.as_array().cloned().unwrap_or_default();
    if list.is_empty() {
        match &name {
            Some(n) => println!("`{n}`: nothing to decide: it runs in order, as written (no arrays to split, no calls to run at the same time, no effects to wait for)"),
            None => println!("nothing to decide: the program runs in order, as written"),
        }
        return Ok(ExitCode::SUCCESS);
    }
    let mark = |k: &str| match k {
        "parallel" | "net" => "∥",
        "gpu" => "▣",
        "task" => "◷",
        "ordered" => "→",
        "waits" => "⏸",
        "fused" => "⇶",
        "simd" => "≋",
        _ => "·",
    };
    let mut last = String::new();
    for d in &list {
        let f = d["function"].as_str().unwrap_or("");
        if f != last {
            let loc = &d["location"];
            println!("{}{f}  ({}:{})", if last.is_empty() { "" } else { "\n" }, loc["shown"].as_str().unwrap_or(""), loc["line"]);
            last = f.to_string();
        }
        let k = d["kind"].as_str().unwrap_or("");
        println!("  {} {:<8} line {:<4} {}", mark(k), k, d["location"]["line"], d["code"].as_str().unwrap_or(""));
        println!("      {}", d["why"].as_str().unwrap_or(""));
    }
    Ok(ExitCode::SUCCESS)
}

/// `mimic audit`: run every rule, report, compare with the baseline and the
/// last run, and fail on new findings at `--fail-on` or worse.
fn audit_cmd(file: Option<PathBuf>, format: &str, fail_on: &str, baseline: Option<PathBuf>, update_baseline: bool) -> Result<ExitCode, String> {
    let input = driver::input_from(file.as_deref())?;
    let p = match driver::load(&input) {
        Ok(p) => p,
        Err(r) => {
            // the manifest's rules need no code: say them anyway
            let mut msg = format!("the program does not load; `mimic check` first\n{}", r.human());
            if let Input::Project(pr) = &input {
                for (_, what) in audit::dependencies(pr) {
                    msg += &format!("\nwarning[S07] branch-dependency: {what}");
                }
            }
            return Err(msg);
        }
    };
    let rank = |s: &str| match s {
        "error" => 3,
        "warning" => 2,
        "note" => 1,
        _ => 0,
    };
    if !["error", "warning", "note", "never"].contains(&fail_on) {
        return Err(format!("--fail-on takes error, warning, note or never, not `{fail_on}`"));
    }
    let mut list: Vec<serde_json::Value> = Vec::new();
    let row = |rule: &audit::Rule, display: &str, path: &str, src: &str, span: mimic_syntax::Span, message: String| {
        let (end_line, end_column) = mimic_syntax::line_col(src, span.end);
        let line_text = src.lines().nth(span.line.saturating_sub(1) as usize).unwrap_or("").trim().to_string();
        let code = src.get(span.start..span.end).filter(|c| !c.is_empty()).unwrap_or(&line_text).lines().next().unwrap_or("").to_string();
        serde_json::json!({
            "rule": rule.id, "name": rule.name, "category": rule.category, "severity": rule.severity,
            "message": message, "help": rule.help,
            "file": display, "path": path, "line": span.line, "column": span.col, "end_line": end_line, "end_column": end_column,
            "source": line_text,
            "fingerprint": audit::fingerprint(rule.id, display, &code),
        })
    };
    for f in audit::run(&p) {
        let sf = &p.files[f.file];
        list.push(row(f.rule, &sf.display, &sf.path.display().to_string(), &sf.src, f.span, f.message));
    }
    let root = match &input {
        Input::Project(pr) => {
            let manifest = pr.root.join(mimic_project::MANIFEST);
            let text = std::fs::read_to_string(&manifest).unwrap_or_default();
            for (name, msg) in audit::dependencies(pr) {
                let at = text.find(&format!("{name} =")).or_else(|| text.find(&name)).unwrap_or(0);
                let span = mimic_syntax::span_at(&text, at, at + name.len());
                list.push(row(audit::rule("S07"), "mim.mim", &manifest.display().to_string(), &text, span, msg));
            }
            pr.root.clone()
        }
        Input::File(f) => f.parent().map(Path::to_path_buf).unwrap_or_default(),
    };
    // the baseline: findings already known
    let base_path = baseline.clone().unwrap_or_else(|| root.join(".mimic-audit.json"));
    let known: std::collections::HashSet<String> = if baseline.is_some() || update_baseline {
        std::fs::read_to_string(&base_path)
            .ok()
            .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
            .and_then(|v| v["fingerprints"].as_array().cloned())
            .unwrap_or_default()
            .iter()
            .filter_map(|x| x.as_str().map(String::from))
            .collect()
    } else {
        Default::default()
    };
    for f in &mut list {
        let new = !known.contains(f["fingerprint"].as_str().unwrap_or(""));
        f["new"] = new.into();
    }
    if update_baseline {
        let fps: Vec<&serde_json::Value> = list.iter().map(|f| &f["fingerprint"]).collect();
        let text = serde_json::to_string_pretty(&serde_json::json!({ "tool": "mimic audit", "fingerprints": fps })).unwrap_or_default();
        std::fs::write(&base_path, text + "\n").map_err(|e| format!("cannot write {}: {e}", base_path.display()))?;
        // everything found now is known from now on
        for f in &mut list {
            f["new"] = false.into();
        }
    }
    // trends: counts per run, kept beside the build
    let count = |sev: &str| list.iter().filter(|f| f["severity"] == sev).count();
    let (errors, warnings, notes) = (count("error"), count("warning"), count("note"));
    let security = list.iter().filter(|f| f["category"] == "security").count();
    let hist_dir = match &input {
        Input::Project(pr) => pr.target_dir().join("audit"),
        Input::File(_) => packages::cache_dir().join("audit"),
    };
    let hist = hist_dir.join(format!("history-{}.jsonl", input.name()));
    let previous = std::fs::read_to_string(&hist).ok().and_then(|t| t.lines().last().and_then(|l| serde_json::from_str::<serde_json::Value>(l).ok()));
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let _ = std::fs::create_dir_all(&hist_dir);
    if let Ok(mut h) = std::fs::OpenOptions::new().create(true).append(true).open(&hist) {
        use std::io::Write;
        let _ = writeln!(h, "{}", serde_json::json!({ "time": now, "error": errors, "warning": warnings, "note": notes, "security": security, "total": list.len() }));
    }
    let gate = rank(fail_on);
    let failing = list.iter().filter(|f| f["new"] == true && gate > 0 && rank(f["severity"].as_str().unwrap_or("")) >= gate).count();
    match format {
        "json" => println!("{}", serde_json::to_string_pretty(&serde_json::json!({ "findings": list, "summary": { "error": errors, "warning": warnings, "note": notes, "security": security, "failing": failing } })).unwrap_or_default()),
        "sarif" => println!("{}", serde_json::to_string_pretty(&audit::sarif(&list)).unwrap_or_default()),
        "text" => {
            for f in &list {
                let sev = f["severity"].as_str().unwrap_or("");
                println!(
                    "{sev}[{}] {}: {}{}\n  --> {}:{}:{}\n   | {}\n   = help: {} (`mimic man {}`)\n",
                    f["rule"].as_str().unwrap_or(""),
                    f["name"].as_str().unwrap_or(""),
                    f["message"].as_str().unwrap_or(""),
                    if f["new"] == false { " (in the baseline)" } else { "" },
                    f["file"].as_str().unwrap_or(""),
                    f["line"],
                    f["column"],
                    f["source"].as_str().unwrap_or(""),
                    f["help"].as_str().unwrap_or(""),
                    f["rule"].as_str().unwrap_or("")
                );
            }
            let mut summary = format!("{errors} error(s), {warnings} warning(s), {notes} note(s); {security} about security");
            if let Some(prev) = previous {
                let d = list.len() as i64 - prev["total"].as_i64().unwrap_or(0);
                summary += &format!("; {} since the last run", match d {
                    0 => "no change".to_string(),
                    d if d > 0 => format!("{d} more"),
                    d => format!("{} fewer", -d),
                });
            }
            status("Audited", summary);
            if failing > 0 {
                status("Failing", format!("{failing} new finding(s) at `{fail_on}` or worse (see --fail-on, --baseline)"));
            }
        }
        f => return Err(format!("--format takes text, json or sarif, not `{f}`")),
    }
    Ok(if failing > 0 { ExitCode::FAILURE } else { ExitCode::SUCCESS })
}

/// `mimic fmt`: format files in place (or, with --check, only report).
fn fmt_cmd(files: Vec<PathBuf>, glyphs: bool, check: bool) -> Result<ExitCode, String> {
    let files = if files.is_empty() {
        let input = driver::input_from(None).map_err(|e| e.to_string())?;
        match input {
            Input::Project(p) => {
                let mut fs = p.sources()?;
                for extra in ["mim.mim", "build.mim"] {
                    if p.root.join(extra).is_file() {
                        fs.push(p.root.join(extra));
                    }
                }
                fs
            }
            Input::File(f) => vec![f],
        }
    } else {
        files
    };
    let (mut changed, mut failed) = (Vec::new(), 0);
    for f in &files {
        let src = std::fs::read_to_string(f).map_err(|e| format!("cannot read {}: {e}", f.display()))?;
        match mimic_syntax::format::format(&src, glyphs) {
            Ok(text) if text != src => {
                if !check {
                    std::fs::write(f, &text).map_err(|e| format!("cannot write {}: {e}", f.display()))?;
                }
                changed.push(f.display().to_string());
            }
            Ok(_) => {}
            Err(d) => {
                failed += 1;
                eprintln!("{}:{}:{}: {}", f.display(), d.span.line, d.span.col, d.msg);
            }
        }
    }
    if check {
        for c in &changed {
            println!("{c}");
        }
        status("Checked", format!("{} file(s): {} not formatted", files.len(), changed.len()));
        return Ok(if changed.is_empty() && failed == 0 { ExitCode::SUCCESS } else { ExitCode::from(1) });
    }
    status("Formatted", format!("{} of {} file(s) changed", changed.len(), files.len()));
    Ok(if failed == 0 { ExitCode::SUCCESS } else { ExitCode::from(1) })
}

/// `mimic test`: build the test program, list its tests, run each in a
/// process of its own (a crash fails only that test), in parallel; report
/// in source order.
/// lldb formatters for Mimic values (see lldb_mimic.py).
const LLDB_FORMATTERS: &str = include_str!("lldb_mimic.py");

/// A debug build for a debugger: the executable (`mimic lldb`, `mimic dap`).
pub(crate) fn debug_build(input: &Input) -> Result<PathBuf, String> {
    let mut prof = Profile::for_input(input, false);
    prof.net = true;
    let a = driver::build(input, &prof, &Target::host(), false, false).map_err(|r| r.human())?;
    Ok(a.path)
}

/// The lldb formatters that show Mimic's values, written to the cache.
pub(crate) fn lldb_formatters() -> Result<PathBuf, String> {
    let dir = packages::cache_dir().join("lldb");
    std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    let script = dir.join("mimic_values.py");
    if std::fs::read_to_string(&script).ok().as_deref() != Some(LLDB_FORMATTERS) {
        mimic_toolchain::jobs::write_atomic(&script, LLDB_FORMATTERS.as_bytes()).map_err(|e| format!("cannot write {}: {e}", script.display()))?;
    }
    Ok(script)
}

fn lldb_cmd(file: Option<PathBuf>, args: Vec<String>) -> Result<ExitCode, String> {
    let input = driver::input_from(file.as_deref()).map_err(|e| e.to_string())?;
    let exe = debug_build(&input)?;
    let script = lldb_formatters()?;
    status("Debugging", exe.display().to_string());
    let st = std::process::Command::new("lldb")
        .arg("-o")
        .arg(format!("command script import {}", script.display()))
        .arg("--")
        .arg(&exe)
        .args(&args)
        .status()
        .map_err(|e| format!("cannot start lldb ({e}); it comes with the Xcode command line tools on macOS and the lldb package on Linux"))?;
    Ok(if st.success() { ExitCode::SUCCESS } else { ExitCode::FAILURE })
}

fn doc_cmd(file: Option<PathBuf>, std: bool, json: bool, out: Option<PathBuf>) -> Result<ExitCode, String> {
    let (program, package, title, strip, default_out) = if std {
        // any program loads the standard library: an empty one
        let dir = std::env::temp_dir().join(format!("mimic-doc-std-{}", std::process::id()));
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let f = dir.join("main.mim");
        std::fs::write(&f, "module main\n\nfn main() {\n}\n").map_err(|e| e.to_string())?;
        let input = driver::input_from(Some(f.as_path())).map_err(|e| e.to_string())?;
        let p = driver::load(&input).map_err(|r| r.human());
        let _ = std::fs::remove_dir_all(&dir);
        (p?, "std".to_string(), "Mimic standard library".to_string(), Some("prelude"), PathBuf::from("std-doc"))
    } else {
        let input = driver::input_from(file.as_deref()).map_err(|e| e.to_string())?;
        let p = driver::load(&input).map_err(|r| r.human())?;
        let name = input.name();
        let out = match &input {
            Input::Project(pr) => pr.target_dir().join("doc"),
            Input::File(_) => PathBuf::from(format!("{name}-doc")),
        };
        (p, name.clone(), name, None, out)
    };
    let mods = doc::collect(&program, &package, strip);
    // the guides: the project's docs/ folder; for the standard library, the
    // manual's guides
    let guides: Vec<doc::Guide> = if std {
        man::Manual::load()
            .pages
            .into_iter()
            .filter(|p| p.kind == "guide")
            .map(|p| doc::Guide { name: p.name, title: p.title, markdown: p.text })
            .collect()
    } else {
        match driver::input_from(file.as_deref()) {
            Ok(Input::Project(pr)) => doc::guides_in(&pr.root.join("docs")),
            _ => Vec::new(),
        }
    };
    if json {
        println!("{}", serde_json::to_string_pretty(&doc::to_json(&title, &mods, &guides)).map_err(|e| e.to_string())?);
        return Ok(ExitCode::SUCCESS);
    }
    let dir = out.unwrap_or(default_out);
    std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    let page = dir.join("index.html");
    std::fs::write(&page, doc::to_html(&title, &mods, &guides)).map_err(|e| format!("cannot write {}: {e}", page.display()))?;
    let n: usize = mods.iter().map(|m| m.items.iter().filter(|i| i.kind != "group").count()).sum();
    let g = if guides.is_empty() { String::new() } else { format!("{} guide{}, ", guides.len(), if guides.len() == 1 { "" } else { "s" }) };
    status("Documented", format!("{g}{n} items in {} modules → {}", mods.len(), page.display()));
    Ok(ExitCode::SUCCESS)
}

fn test_cmd(args: Vec<String>, profile: ProfileArgs) -> Result<ExitCode, String> {
    let (files, filters): (Vec<String>, Vec<String>) = args.into_iter().partition(|a| a.ends_with(".mim"));
    if files.len() > 1 {
        return Err("give at most one .mim file".into());
    }
    let input = driver::input_from(files.first().map(std::path::Path::new)).map_err(|e| e.to_string())?;
    let mut prof = profile_of(&input, profile, false);
    prof.tests = true;
    let a = driver::build(&input, &prof, &Target::host(), false, false).map_err(|r| r.human())?;
    let out = std::process::Command::new(&a.path).arg("--list").output().map_err(|e| format!("cannot run {}: {e}", a.path.display()))?;
    let mut names: Vec<String> = String::from_utf8_lossy(&out.stdout).lines().map(String::from).collect();
    // the examples of the project's guides (docs/*.md) are tests too
    let examples = match &input {
        Input::Project(p) => doc::guide_examples(&p.root.join("docs")),
        Input::File(_) => Vec::new(),
    };
    let compiled = names.len();
    names.extend(examples.iter().map(|(n, ..)| n.clone()));
    let chosen: Vec<(usize, String)> = names.into_iter().enumerate().filter(|(_, n)| filters.is_empty() || filters.iter().any(|f| n.contains(f.as_str()))).collect();
    let me = std::env::current_exe().map_err(|e| e.to_string())?;
    let scratch = std::env::temp_dir().join(format!("mimic-doc-examples-{}", std::process::id()));
    if chosen.is_empty() {
        status("Testing", if filters.is_empty() { "no tests (write `test \"what it checks\" { expect(…) }`)" } else { "no test matches" });
        return Ok(ExitCode::SUCCESS);
    }
    status("Testing", format!("{} test{}", chosen.len(), if chosen.len() == 1 { "" } else { "s" }));
    let t0 = std::time::Instant::now();
    let jobs = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).min(chosen.len());
    let next = std::sync::atomic::AtomicUsize::new(0);
    let results: std::sync::Mutex<Vec<Option<(bool, String, f64)>>> = std::sync::Mutex::new(vec![None; chosen.len()]);
    std::thread::scope(|sc| {
        for _ in 0..jobs {
            sc.spawn(|| loop {
                let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                if i >= chosen.len() {
                    break;
                }
                let k = chosen[i].0;
                let t = std::time::Instant::now();
                if k >= compiled {
                    // a guide's example: run it, compare its output
                    let (_, program, want) = &examples[k - compiled];
                    let dir = scratch.join(k.to_string());
                    let _ = std::fs::create_dir_all(&dir);
                    let f = dir.join("example.mim");
                    let r = match std::fs::write(&f, program).and_then(|_| std::process::Command::new(&me).arg("run").arg(&f).env("NO_COLOR", "1").output()) {
                        Ok(o) => {
                            let got = String::from_utf8_lossy(&o.stdout).into_owned();
                            if got == *want {
                                (true, String::new(), t.elapsed().as_secs_f64())
                            } else {
                                let err = String::from_utf8_lossy(&o.stderr);
                                (false, format!("expected:\n{want}got:\n{got}{}", if o.status.success() { String::new() } else { err.into_owned() }), t.elapsed().as_secs_f64())
                            }
                        }
                        Err(e) => (false, format!("cannot run the example: {e}"), 0.0),
                    };
                    results.lock().unwrap()[i] = Some(r);
                    continue;
                }
                let mut c = std::process::Command::new(&a.path);
                c.args(["--run", &k.to_string()]);
                if std::env::var_os("MIMIC_THREADS").is_none() {
                    c.env("MIMIC_THREADS", "2"); // tests share the cores
                }
                let r = match c.output() {
                    Ok(o) => {
                        let mut text = String::from_utf8_lossy(&o.stdout).into_owned();
                        text.push_str(&String::from_utf8_lossy(&o.stderr));
                        (o.status.success(), text, t.elapsed().as_secs_f64())
                    }
                    Err(e) => (false, format!("cannot run the test: {e}"), 0.0),
                };
                results.lock().unwrap()[i] = Some(r);
            });
        }
    });
    let results = results.into_inner().unwrap();
    let mut failed = 0;
    for ((_, name), r) in chosen.iter().zip(results) {
        let (ok, text, secs) = r.unwrap_or((false, "not run".into(), 0.0));
        if ok {
            println!("  ✓ {name}{}", if secs >= 0.5 { format!(" ({secs:.1} s)") } else { String::new() });
        } else {
            failed += 1;
            println!("  ✗ {name}");
            for l in text.lines() {
                println!("    {l}");
            }
        }
    }
    let _ = std::fs::remove_dir_all(&scratch);
    let passed = chosen.len() - failed;
    let summary = format!("{passed} passed, {failed} failed ({:.2} s)", t0.elapsed().as_secs_f64());
    println!("  {summary}");
    Ok(if failed == 0 { ExitCode::SUCCESS } else { ExitCode::from(1) })
}

/// Report a failed check/build/run: JSON (stdout, or stderr for `run`) or text.
fn fail(r: Report, json: bool, command: &str, json_to_stderr: bool) -> Result<ExitCode, String> {
    if !json {
        return Err(r.human());
    }
    let v = r.json(command);
    if json_to_stderr {
        eprintln!("{v}");
    } else {
        println!("{v}");
    }
    Ok(ExitCode::FAILURE)
}

fn targets_cmd(cpus: Option<&str>, features: Option<&str>, check: Option<&str>, json: bool) -> Result<ExitCode, String> {
    use mimic_toolchain::{Names, llvm_names};
    use serde_json::json;
    use toolchain::Support;
    if let Some(arch) = cpus.or(features) {
        let t = if arch.contains('-') { Target::parse(arch)? } else { Target::parse(&format!("{arch}-unknown-unknown"))? };
        let triple = t.llvm.unwrap();
        let list = mimic_toolchain::cpus(&triple, features.is_some()).ok_or_else(|| format!("no CPU list for `{arch}`"))?;
        if json {
            let v: Vec<_> = list.iter().map(|(k, d)| if d.is_empty() { json!(k) } else { json!({"name": k, "description": d}) }).collect();
            println!("{}", serde_json::Value::Array(v));
        } else {
            for (k, d) in list {
                if d.is_empty() { println!("{k}") } else { println!("{k:<32} {d}") }
            }
        }
        return Ok(ExitCode::SUCCESS);
    }
    let support_str = |t: &Target| match toolchain::support(t) {
        Support::Bundled => "executables (bundled libc)".to_string(),
        Support::Sdk => "executables (platform SDK)".to_string(),
        Support::NotYet(why) => format!("objects only: {why}"),
    };
    if let Some(t) = check {
        let tg = Target::parse(t)?;
        let s = support_str(&tg);
        let tier = toolchain::tier(&tg);
        if json {
            println!(
                "{}",
                json!({"target": tg.name(), "llvm": tg.llvm, "arch": tg.arch, "os": tg.os, "env": tg.env,
                       "support": s, "tier": tier.as_str()})
            );
        } else {
            println!("{} → {}", tg.name(), tg.llvm.as_deref().unwrap_or("native"));
            let why = tg.backend_gap().map(str::to_string).unwrap_or_else(|| tier.meaning().to_string());
            println!("  tier:    {} ({why})", tier.as_str());
            println!("  output:  {s}");
        }
        return Ok(ExitCode::SUCCESS);
    }
    let arches = targets::arches();
    let backends = llvm_names(Names::Backends);
    let oses: Vec<String> = llvm_names(Names::Oses).into_iter().map(|x| x.0).collect();
    let envs: Vec<String> = llvm_names(Names::Environments).into_iter().map(|x| x.0).collect();
    let vendors: Vec<String> = llvm_names(Names::Vendors).into_iter().map(|x| x.0).collect();
    // complete executables: the bundled-musl targets, plus macOS with its SDK
    let mut exes: Vec<String> = mimic_toolchain::musl::TARGETS.iter().map(|t| t.to_string()).collect();
    exes.push("wasm32-wasi".into());
    // Windows with the bundled MinGW-w64
    exes.extend(["x86_64-windows".into(), "aarch64-windows".into()]);
    // Apple platforms whose SDK is installed (Xcode)
    for t in [
        "aarch64-macos", "x86_64-macos", "aarch64-ios-macabi", "aarch64-ios", "aarch64-ios-simulator",
        "x86_64-ios-simulator", "aarch64-tvos", "aarch64-tvos-simulator", "arm64_32-watchos",
        "aarch64-watchos-simulator", "aarch64-xros", "aarch64-xros-simulator",
    ] {
        if let Ok(tg) = Target::parse(t)
            && matches!(toolchain::support(&tg), Support::Sdk)
        {
            exes.push(t.to_string());
        }
    }
    if json {
        let v = json!({
            "backends": backends.iter().map(|(n, d)| json!({"name": n, "description": d})).collect::<Vec<_>>(),
            "arches": arches.iter().map(|(n, b)| json!({"name": n, "backend": b})).collect::<Vec<_>>(),
            "vendors": vendors,
            "oses": oses,
            "environments": envs,
            "executables": exes,
            "tiers": exes.iter().filter_map(|e| Target::parse(e).ok().map(|t| json!({"target": e, "tier": toolchain::tier(&t).as_str()}))).collect::<Vec<_>>(),
        });
        println!("{v}");
        return Ok(ExitCode::SUCCESS);
    }
    println!("Code generators in this build ({}):", backends.len());
    for (n, d) in &backends {
        println!("  {n:<14} {d}");
    }
    let with: Vec<&str> = arches.iter().filter(|a| a.1.is_some()).map(|a| a.0.as_str()).collect();
    let without: Vec<&str> = arches.iter().filter(|a| a.1.is_none()).map(|a| a.0.as_str()).collect();
    println!("\nArchitectures ({} known to LLVM, {} with a code generator here):\n  {}", arches.len(), with.len(), with.join(" "));
    if !without.is_empty() {
        println!("  without a code generator in this build: {}", without.join(" "));
    }
    println!("\nOperating systems ({}):\n  {}", oses.len(), oses.join(" "));
    println!("\nEnvironments / ABIs ({}):\n  {}", envs.len(), envs.join(" "));
    println!("\nVendors ({}):\n  {}", vendors.len(), vendors.join(" "));
    println!("\nTargets by tier (any other CPU target: compiles):");
    for tier in [toolchain::Tier::Stable, toolchain::Tier::Runs, toolchain::Tier::Links] {
        let list: Vec<&str> = exes
            .iter()
            .filter(|e| Target::parse(e).is_ok_and(|t| toolchain::tier(&t) == tier))
            .map(String::as_str)
            .collect();
        println!("  {:<8} {} ({})", tier.as_str(), list.len(), tier.meaning());
        for chunk in list.chunks(4) {
            println!("           {}", chunk.join("  "));
        }
    }
    println!("  {:<8} the other CPU architectures ({})", "compiles", toolchain::Tier::Compiles.meaning());
    println!("  {:<8} amdgpu r600 spirv spirv32 spirv64 ({})", "not yet", toolchain::Tier::NotYet.meaning());
    println!("\nWrite a target as an LLVM triple, vendor optional: x86_64-linux-musl, aarch64-linux, x86_64-apple-macosx13.0.");
    println!("`mimic targets --check <target>` shows how a target is understood and what it can build.");
    Ok(ExitCode::SUCCESS)
}

fn explain(code: Option<&str>, json: bool) -> Result<ExitCode, String> {
    use mimic_syntax::codes;
    let one = |c: &codes::Code| serde_json::json!({ "code": c.id, "name": c.name, "title": c.title, "explanation": c.explain });
    match code {
        None if json => println!("{}", serde_json::Value::Array(codes::ALL.iter().map(|c| one(c)).collect())),
        None => {
            for c in codes::ALL {
                println!("{}  {:<26} {}", c.id, c.name, c.title);
            }
            println!("\nRun `mimic explain <code>` for details.");
        }
        Some(q) => {
            let c = codes::find(q).ok_or_else(|| format!("no error code `{q}`; run `mimic explain` for the list"))?;
            if json {
                println!("{}", one(c));
            } else {
                println!("{} ({}): {}\n\n{}\n\nThe manual has this page too: `mimic man {}`", c.id, c.name, c.title, c.explain, c.id);
            }
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn kind_label(bin: bool, lib: bool) -> &'static str {
    match (bin, lib) {
        (true, true) => "binary + library",
        (false, true) => "library",
        _ => "binary",
    }
}

fn shown(p: &Path) -> String {
    let cwd = std::env::current_dir().unwrap_or_default();
    p.strip_prefix(&cwd).unwrap_or(p).display().to_string()
}

fn write_initial_modfile(dir: &Path) {
    if let Ok(proj) = Project::load(dir)
        && let Ok(p) = driver::load(&Input::Project(Box::new(proj)))
            && let Ok(proj) = Project::load(dir) {
                let _ = modfile::sync(&proj, &p);
            }
}

fn mod_cmd(cmd: ModCmd) -> Result<ExitCode, String> {
    let proj = project_here()?;
    let path = proj.root.join(MOD_FILE);
    match cmd {
        ModCmd::Init => {
            if path.exists() {
                return Err(format!("{MOD_FILE} already exists; use `mimic mod update`"));
            }
            let root = proj.root.clone();
            let p = driver::load(&Input::Project(Box::new(proj)))?;
            modfile::sync(&Project::load(&root)?, &p)?;
            status("Created", MOD_FILE);
        }
        ModCmd::Update => {
            let root = proj.root.clone();
            let p = driver::load(&Input::Project(Box::new(proj)))?;
            let changed = modfile::sync(&Project::load(&root)?, &p)?;
            status(if changed { "Updated" } else { "Fresh" }, format!("{MOD_FILE} ({} modules, {} functions)", p.mods.iter().filter(|m| m.file.is_some()).count(), p.fns.len()));
        }
        ModCmd::List { all } => {
            let p = driver::load(&Input::Project(Box::new(proj)))?;
            for (path, kind, is_pub, loc) in modfile::listing(&p, all) {
                println!("{:<7} {:<4} {:<40} {loc}", kind, if is_pub { "pub" } else { "" }, path);
            }
        }
        ModCmd::Delete { module, yes } => {
            let root = proj.root.clone();
            let name = proj.manifest.name.clone();
            let p = driver::load(&Input::Project(Box::new(proj)))?;
            let mut segs: Vec<String> = module.split("::").map(String::from).collect();
            if segs.first() != Some(&name) {
                segs.insert(0, name.clone());
            }
            let mid = p
                .mods
                .iter()
                .enumerate()
                .find(|(i, m)| m.file.is_some() && m.package == name && p.mod_path(*i) == segs)
                .map(|(i, _)| i)
                .ok_or_else(|| format!("no module `{}` in this project (see `mimic mod list`)", segs.join("::")))?;
            let m = &p.mods[mid];
            let file = &p.files[m.file.unwrap()];
            let is_file_module = m.parent == p.roots.get(&name).copied();
            if is_file_module && (m.name == "main" || m.name == "lib") {
                return Err(format!("refusing to delete the `{}` entry-point module", m.name));
            }
            if !yes {
                if is_file_module {
                    println!("would delete file {}", file.display);
                } else {
                    println!("would remove module `{}` ({}:{}-{})", segs.join("::"), file.display, m.full.line, line_at(&file.src, m.full.end));
                }
                println!("re-run with --yes to delete");
                return Ok(ExitCode::SUCCESS);
            }
            if is_file_module {
                std::fs::remove_file(&file.path).map_err(|e| e.to_string())?;
                status("Deleted", &file.display);
            } else {
                let mut src = file.src.clone();
                let mut end = m.full.end;
                if src[end..].starts_with('\n') {
                    end += 1;
                }
                let start = src[..m.full.start].rfind('\n').map(|i| i + 1).unwrap_or(0);
                src.replace_range(start..end, "");
                std::fs::write(&file.path, src).map_err(|e| e.to_string())?;
                status("Removed", format!("module `{}` from {}", segs.join("::"), file.display));
            }
            match driver::load(&Input::Project(Box::new(Project::load(&root)?))) {
                Ok(p2) => {
                    modfile::sync(&Project::load(&root)?, &p2)?;
                    status("Updated", MOD_FILE);
                }
                Err(e) => {
                    eprintln!("{e}");
                    eprintln!("warning: the project no longer checks cleanly; fix the references above, then run `mimic mod update`");
                }
            }
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn line_at(src: &str, byte: usize) -> usize {
    src[..byte.min(src.len())].matches('\n').count() + 1
}

fn net_cmd(cmd: NetCmd) -> Result<ExitCode, String> {
    match cmd {
        NetCmd::Fmt { file } => {
            let src = std::fs::read_to_string(&file).map_err(|e| format!("cannot read {}: {e}", file.display()))?;
            let book = mimic_net::parse(&src).map_err(|e| format!("{}:{e}", file.display()))?;
            print!("{book}");
            Ok(ExitCode::SUCCESS)
        }
        NetCmd::Run { file, profile, stats, threads } => {
            let exe = driver::build_net(&file, profile.release)?;
            let mut c = std::process::Command::new(&exe);
            if stats {
                c.env("MIMIC_NET_STATS", "1");
            }
            if let Some(t) = threads {
                c.env("MIMIC_THREADS", t.to_string());
            }
            let st = c.status().map_err(|e| format!("cannot run {}: {e}", exe.display()))?;
            Ok(ExitCode::from(st.code().unwrap_or(1).clamp(0, 255) as u8))
        }
    }
}
