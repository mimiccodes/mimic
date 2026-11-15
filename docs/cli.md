# The `mimic` command

`mimic --help` lists the commands, and `mimic <command> --help` shows the details of one.

| Command | What it does |
|---|---|
| `mimic create <name> [--bin] [--lib]` | Create a project in `./<name>`. The default kind is `--bin`; pass both flags for a combined binary and library. |
| `mimic init [--name N] [--bin] [--lib]` | Turn the current directory into a project. |
| `mimic run [file.mim] [--debug\|--release] [--emit-json] [-- args…]` | Build and run the project, or a single standalone file that declares `module main`. With `--emit-json`, compile errors go to stderr as JSON. |
| `mimic run --terminal [file.mim]`, `mimic run --window [file.mim]` | Show a `ui` program in the terminal, or in a native window, instead of the browser (sets `MIMIC_UI`). |
| `mimic run --web [file.mim] [--port 8080]` | Build a `ui` program as a web page (WebAssembly) and serve it at `http://localhost:8080/`. |
| `mimic lldb [file.mim] [-- args]` | Debug build, then lldb with Mimic's values shown as Mimic prints them (numbers, strings, lists, records, variants): breakpoints on `.mim` lines, stepping, `frame variable`, backtraces. |
| `mimic doc [file.mim] [--std] [--json] [--out DIR]` | Documentation of the public modules, functions and types: full signatures and the `//` comments right above each item, one page (`target/doc/index.html` in a project, `<name>-doc/` for a file; with a sidebar and search). `--std` documents the standard library; `--json` prints it as JSON. |
| `mimic build --web [file.mim]` | Write the web page: `<name>-web/index.html` and `app.wasm`, ready for any static web server. |
| `mimic build [file.mim] [--debug\|--release] [--target T] [--all-targets] [--emit-ir] [--emit-net] [--no-net] [--cpu CPU] [--strict-math\|--fast-math] [--emit-json]` | Compile. A binary produces `target/<profile>/<name>`; a library produces `lib<name>.a`. `--emit-net` writes the generated interaction nets; `--no-net` compiles everything natively; `--strict-math` turns off fused multiply-add (on by default, as in C compilers) for bit-identical float results on every CPU; `--fast-math` lets float sums and products be regrouped (so vectorised), divisions use reciprocals and math functions approximate, results changing in the last bits (never assuming no NaN or infinity: bounds checks stay). |
| `mimic check [file.mim] [--emit-json] [--fix]` | Parse, resolve and check everything without generating code. `--emit-json` prints the result as JSON for editors and agents; `--fix` applies the machine-applicable fixes. See [diagnostics.md](diagnostics.md). |
| `mimic test [file.mim] [words…] [--release]` | Run the tests: every `test "name" { … }` block of the project (or file), plus `src/test.mim`. Each test runs in a process of its own, many at once; a failed `expect(a == b)` shows both sides, and a crash fails only its own test. Words keep the tests whose names contain one of them. Exit code 1 when a test fails. Tests are never part of a build. |
| `mimic fmt [files…] [--glyphs] [--check]` | Format the project's `.mim` files (or the given ones): primitives and operators as words (`iota`, `each`, `\|>`, `!=`, `and`), or as glyphs with `--glyphs` (`⍳ ¨ ▷ ≠ ∧`); indentation of 4 spaces per level of nesting; no trailing spaces, at most one blank line in a row. Comments and line breaks stay as written, and the result always lexes to the same program. `--check` changes nothing, lists the files that are not formatted, and exits with 1 if there are any. |
| `mimic query [method] [key=value…] [--in file.mim] [--serve]` | Ask the analysis engine about the project, as JSON: `check`, `symbols`, `definition`, `hover`, `references`, `outline`, `docs`, `effects`, `stats`. `--serve` keeps it running, answering one JSON request per line on stdin. For scripts, CI and editors; see [workbench.md](workbench.md). |
| `mimic explain [code] [--emit-json]` | Explain an error code (`mimic explain E0401`), or list them all. |
| `mimic mod init` | Create `mod.mim`. |
| `mimic mod update` | Regenerate `mod.mim`. |
| `mimic mod list [--all]` | List the project's modules. `--all` adds functions and dependencies. |
| `mimic mod delete <module> [--yes]` | Remove a module. A nested module's block is deleted; a file module's file is deleted. This is a dry run unless you pass `--yes`. |
| `mimic install <git-url>` | Clone a library into `~/.mimic/libraries/<name>`, e.g. `mimic install github.com/harcorp/ai.git`. |
| `mimic list` | List the installed libraries. |
| `mimic update [library]` | Fast-forward every installed library, or just one. |
| `mimic uninstall <library>` | Remove an installed library. |
| `mimic glyphs` | Print the glyph ↔ word ↔ digraph table. |
| `mimic targets [--cpus ARCH] [--features ARCH] [--check TARGET] [--json]` | List what the linked LLVM can target (architectures, OSes, environments, code generators) and which targets build complete executables. See [targets.md](targets.md). |
| `mimic cc …`, `mimic c++ …` | The embedded Clang C/C++ compiler. |
| `mimic ld.lld …`, `mimic ld64.lld …`, `mimic lld-link …`, `mimic wasm-ld …` | The embedded LLD linker, in its ELF, Mach-O, COFF and WebAssembly flavours. |
| `mimic net run <file.mnet> [--release] [--stats] [--threads N]` | Reduce a raw interaction net and print the value of `@main`. This is a runtime developer tool; see [net.md](net.md). |
| `mimic net fmt <file.mnet>` | Print a net back in canonical form. |

## Profiles

* **Builds that change nothing are skipped.** A build records what its output depends on (every source file, the C declarations imported from headers, C sources, flags, target, the compiler itself) in `<name>.stamp` next to it; when all of that is unchanged, `mimic build` and `mimic run` reuse the program as it is (about 0.2 s). Otherwise large programs get their machine code generated on several threads at once (the optimised module split into parts, one object each).
* **`mimic build` produces the fastest binary by default** (the release profile). It uses `opt = 3`, the host CPU's full instruction set for both the program and the runtime, and interaction nets for parallelism. Pass `--debug` for a debug build, which also carries debug information (DWARF) for debuggers such as lldb and gdb: breakpoints on `.mim` lines, stepping, numeric parameters and `let`s, backtraces with file and line.
* **`mimic run` defaults to `--debug`**, for fast compiles and the leak check at exit. Pass `--release` to run the fast build.
* **`--debug`** builds at `opt = 0`, includes debug info, and checks for leaks at exit.
* **`--release`** builds at `opt = 3`.

Native builds target the host CPU in both profiles; cross builds (`--target`) use a generic CPU for that triple.

Each profile can be tuned in `build.mim`.

## Cross-compilation

`--target` takes an LLVM target triple; the vendor may be left out:
* `x86_64-linux-musl` (the ABI defaults to musl on Linux, so `x86_64-linux` is the same)
* `aarch64-linux`
* `x86_64-apple-macosx13.0`
* `riscv64-unknown-linux-musl`
* `wasm32-wasi`, `nvptx64-nvidia-cuda`, `avr-unknown-unknown`, …

LLVM generates the code for that triple. If `mimic` has a C library for the target, the runtime is compiled for the same triple by the embedded Clang (cached in `~/.mimic/cache/runtime`) and the embedded LLD links a complete executable. Otherwise the object file (or assembly, for NVPTX) is produced. `mimic targets` lists everything; see [targets.md](targets.md).

## Environment

| Variable | Effect |
|---|---|
| `MIMIC_HOME` | Overrides `~/.mimic`, which holds the libraries, the runtime cache and the build directories for single-file runs. |
| `MIMIC_THREADS` | **Runtime:** the number of worker threads (default: all CPUs; `1` runs sequentially). |
| `MIMIC_PAR_GRAIN` | **Runtime:** the number of elements per parallel chunk (default 16384). |
| `MIMIC_GPU` | **Runtime:** `0` never uses the GPU, `1` runs every map the GPU can compute there (default: only maps with a lot of work; macOS). `MIMIC_GPU_DEBUG=1` reports each GPU kernel, `2` also prints its Metal source. |
| `MIMIC_NET_NODES` | **Runtime:** the interaction-net capacity in nodes (default 16M). |

## Planned commands

* `mimic ffi bind`
* `mimic doc`
* a package registry for `mimic install <name>`
