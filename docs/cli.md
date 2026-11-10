# The `mimic` command

`mimic --help` lists the commands, and `mimic <command> --help` shows the details of one.

| Command | What it does |
|---|---|
| `mimic create <name> [--bin] [--lib]` | Create a project in `./<name>`. The default kind is `--bin`; pass both flags for a combined binary and library. |
| `mimic init [--name N] [--bin] [--lib]` | Turn the current directory into a project. |
| `mimic run [file.mim] [--debug\|--release] [-- args…]` | Build and run the project, or a single standalone file that declares `module main`. |
| `mimic build [file.mim] [--debug\|--release] [--target T] [--all-targets] [--emit-ir]` | Compile. A binary produces `target/<profile>/<name>`; a library produces `lib<name>.a`. |
| `mimic check [file.mim]` | Parse, resolve and check everything without generating code. |
| `mimic mod init` | Create `mod.mim`. |
| `mimic mod update` | Regenerate `mod.mim`. |
| `mimic mod list [--all]` | List the project's modules. `--all` adds functions and dependencies. |
| `mimic mod delete <module> [--yes]` | Remove a module. A nested module's block is deleted; a file module's file is deleted. This is a dry run unless you pass `--yes`. |
| `mimic install <git-url>` | Clone a library into `~/.mimic/libraries/<name>`, e.g. `mimic install github.com/harcorp/ai.git`. |
| `mimic list` | List the installed libraries. |
| `mimic update [library]` | Fast-forward every installed library, or just one. |
| `mimic uninstall <library>` | Remove an installed library. |
| `mimic glyphs` | Print the glyph ↔ word ↔ digraph table. |
| `mimic net run <file.mnet> [--release] [--stats] [--threads N]` | Reduce a raw interaction net and print the value of `@main`. This is a runtime developer tool; see [net.md](net.md). |
| `mimic net fmt <file.mnet>` | Print a net back in canonical form. |

## Profiles

* **`--debug`** is the default. It builds at `opt = 0`, includes debug info, and checks for leaks at exit.
* **`--release`** builds at `opt = 3`.

Native builds target the host CPU in both profiles; cross builds (`--target`) use a generic CPU for that triple.

Each profile can be tuned in `build.mim`.

## Cross-compilation

`--target` takes a Zig-style triple, such as:
* `x86_64-linux-gnu`
* `aarch64-linux-musl`
* `aarch64-macos`
* `x86_64-windows`
* `wasm32-wasi`

LLVM generates the object for that triple. The C runtime is then compiled for the same triple with `zig cc` and cached in `~/.mimic/cache/runtime`, and `zig cc` links the result. This means every target Zig can link is reachable, with no per-target toolchain to install.

## Environment

| Variable | Effect |
|---|---|
| `MIMIC_HOME` | Overrides `~/.mimic`, which holds the libraries, the runtime cache and the build directories for single-file runs. |
| `MIMIC_ZIG` | The path to the `zig` binary. Without Zig, native builds fall back to `cc`. |
| `MIMIC_THREADS` | **Runtime:** the number of worker threads (default: all CPUs; `1` runs sequentially). |
| `MIMIC_PAR_GRAIN` | **Runtime:** the number of elements per parallel chunk (default 16384). |
| `MIMIC_NET_NODES` | **Runtime:** the interaction-net capacity in nodes (default 16M). |

## Planned commands

* `mimic test`, which runs `src/test.mim`
* `mimic fmt [--glyphs|--words]`
* `mimic ffi bind`
* `mimic doc`
* a package registry for `mimic install <name>`
