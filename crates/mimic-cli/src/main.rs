//! `mimic` — compiler, project manager, code checker and package manager.

mod driver;
mod modfile;
mod report;
mod targets;
mod toolchain;

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
    version,
    about = "Mimic: an array-based, massively parallel, memory-safe compiled language",
    long_about = "Mimic: an array-based, massively parallel, memory-safe compiled language.\n\n\
                  Every primitive has a glyph and a word: `⍳ 10 ▷ +/` is `iota 10 |> sum`.\n\
                  Run `mimic glyphs` for the full table.",
    after_help = "Examples:\n  mimic create hello && cd hello && mimic run\n  mimic run script.mim --release\n  mimic build                     # fastest binary (release is the default)\n  mimic build --target x86_64-linux-gnu\n  mimic install github.com/harcorp/ai.git"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Args, Clone, Copy)]
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
}

/// `default_release`: `build` produces the fastest binary unless told otherwise;
/// `run` defaults to debug for quick iteration with the leak check.
fn profile_of(input: &Input, args: ProfileArgs, default_release: bool) -> Profile {
    let release = args.release || (default_release && !args.debug);
    let mut p = Profile::for_input(input, release);
    p.net = !args.no_net;
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
        /// Arguments passed to the program (after `--`)
        #[arg(last = true)]
        args: Vec<String>,
    },
    /// Compile the current project (or a single file)
    Build {
        file: Option<PathBuf>,
        #[command(flatten)]
        profile: ProfileArgs,
        /// Cross-compile for a Zig-style target triple, e.g. x86_64-linux-gnu, aarch64-macos, x86_64-windows
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
    /// Install a library globally into ~/.mimic/libraries (from a git URL)
    Install {
        /// Git URL, e.g. github.com/harcorp/ai.git
        source: String,
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
    if let Some(tool) = argv.get(1).map(String::as_str) {
        if tool == "cc" || tool == "c++" {
            let mut args = vec![if tool == "cc" { "clang".to_string() } else { "clang++".to_string() }];
            args.extend(argv[2..].iter().cloned());
            return ExitCode::from(mimic_toolchain::clang_main(&args).clamp(0, 255) as u8);
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
        Cmd::Build { file, profile, target, all_targets, emit_ir, emit_net, emit_json } => {
            let built = (|| -> Result<Vec<serde_json::Value>, Report> {
                let input = driver::input_from(file.as_deref())?;
                let prof = profile_of(&input, profile, true);
                let mut targets = vec![match &target {
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
                    if !emit_json {
                        status("Finished", format!("{} → {}", if a.is_exe { "executable" } else { "static library" }, shown(&a.path)));
                    }
                    out.push(serde_json::json!({
                        "target": t.name(),
                        "profile": prof.name(),
                        "kind": if a.is_exe { "executable" } else { "static-library" },
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
        Cmd::Run { file, profile, emit_json, args } => {
            let built = (|| -> Result<driver::Artifact, Report> {
                let input = driver::input_from(file.as_deref())?;
                if !input.is_bin() {
                    return Err(Report::plain(
                        &mimic_syntax::codes::ENTRY_MODULE,
                        "this is a library project; there is nothing to run (add `bin` to `kind` in mim.mim)",
                    ));
                }
                let prof = profile_of(&input, profile, false);
                driver::build(&input, &prof, &Target::host(), false, false)
            })();
            let a = match built {
                Ok(a) => a,
                Err(r) => return fail(r, emit_json, "run", true),
            };
            status("Running", shown(&a.path));
            let st = std::process::Command::new(&a.path).args(&args).status().map_err(|e| format!("cannot run {}: {e}", a.path.display()))?;
            Ok(ExitCode::from(st.code().unwrap_or(1).clamp(0, 255) as u8))
        }
        Cmd::Mod { cmd } => mod_cmd(cmd),
        Cmd::Install { source } => {
            status("Installing", &source);
            let i = packages::install(&source)?;
            status("Installed", format!("{} v{} → {}", i.name, i.version, i.dir.display()));
            eprintln!("             add it to a project's mim.mim:  dependencies {{ {} = {{}} }}", i.name);
            Ok(ExitCode::SUCCESS)
        }
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
                println!("{} ({}): {}\n\n{}", c.id, c.name, c.title, c.explain);
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
