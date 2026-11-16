# Changelog

Every user-visible change, by version. The plan for the versions ahead is in [docs/versions.md](docs/versions.md).

## Unreleased

### Added
- **The Workbench engine and `mimic query`** (step 0.3.0): one analysis of the project that every tool will ask. `mimic query` answers as JSON: `check`, `symbols`, `definition`, `hover`, `references`, `outline`, `docs`, `effects` (pure, concurrent, ordered or sync) and `stats`. `mimic query --serve` keeps it running, reading one JSON request per line; files sent by an editor (`open`, `change`, `close`) are used instead of the disk, and only changed files are parsed again: an edit of the showcase project is re-checked in about 25 ms. While the program has errors, lookups use the last program that checked. See [docs/workbench.md](docs/workbench.md).

- **Libraries at a chosen version, from git** (step 0.3.1). `mimic install <git-url>@v1.2.0` installs a tag, branch or commit next to other versions (`~/.mimic/libraries/<name>@v1.2.0`), and `rev = "v1.2.0"` in a dependency of `mim.mim` picks it. `mimic install` with no URL, inside a project, installs every missing dependency that has a `git` URL (and theirs), at its `rev` or at the commit `mod.mim` locked, so a fresh clone builds with the same library code. `mimic update` moves a library to the newest commit of its branch and leaves tags and commits where they are.
- **Workspaces:** a `mim.mim` with `workspace { members = [...] }` groups projects; `check`, `build`, `test`, `fmt`, `install` and `mod update` in its folder run in every member, and all members build into its one `target/`.

- **`mimic man`, the manual inside `mimic`** (step 0.3.2): the tutorial, the reference and new guides, embedded in the binary, plus a page made from the compiler for every standard function (243) and module, every glyph (`mimic man ⌿`, `mimic man keep`, `mimic man \k`) and every error code. `--search` finds every line that mentions a text, `--run` runs a page's examples and compares their output, `--json` prints a page for tools. Every error message now ends with the page to read (`For more about this error: mimic man E0303`).
- **Guides:** "Coming from C", "Coming from Python", "Coming from JavaScript", and "Writing Mimic for agents" (the workflow, the idioms, and the mistakes ported code makes, with their error codes), in `docs/manual/`. Their examples are checked like the tutorial's.
- **Examples in comments are tests:** a `// > expression` line and the value `print` would show on the next comment line run under `mimic test`.

- **Syntax highlighting** (step 0.3.3): TextMate grammars for `.mim` and `.mnet` in the new [mimic-syntax](https://github.com/mimiccodes/mimic-syntax) repository (a submodule at `editors/syntax`), for VS Code, Sublime Text and other editors, and later GitHub. Names from C (`c::`, `cpp::`, `as` names) get a scope of their own. A tree-sitter grammar with highlight queries for Neovim, Helix and Zed. A golden test checks that the grammar highlights every glyph, word, digraph and keyword of the compiler, that every test program and example tokenizes with nothing unrecognised, and that tree-sitter parses them all with no error. `mod.mim` is marked as generated for GitHub.

- **`mimic lsp`, the language server** (step 0.3.4): errors as you type with the compiler's fixes as quick fixes; hover with signature, comment and effects (for C functions, their header; for glyphs and their words, what they do and how to type them); go to definition and find references, into the standard library; glyph input (`\i` → `⍳`) and completion of names, modules, words and keywords; formatting in the style the file mostly uses; the outline and workspace symbols; names from C as semantic tokens with the modifier `foreign`. Set-up for VS Code, Neovim, Helix, Emacs, Sublime Text and Zed: `mimic man editors`.

- **The VS Code extension** (step 0.3.5), in the new [mimic-vscode](https://github.com/mimiccodes/mimic-vscode) repository (a submodule at `editors/vscode`): everything of `mimic lsp`, the highlighting and file icon, a glyph palette, **Run** and **Run test** above functions and tests, the Test Explorer (tests and examples in comments, with both sides of a failed `expect`), the manual beside the code, conversion between glyphs and words, and a check that `mimic` is new enough. CI runs it in a real VS Code against the `mimic` it builds.

- **Debugging shows Mimic's values** (step 0.3.6). `mimic lldb` shows text, lists, maps (`["a": 1, "b": 2]`), records (also those kept in registers, which had no debug information before: `Point { x: 1.5, y: 2.0 }`), lists of numbers as `[3 1 4]` and variants as `Some(7)`, without the handles' addresses. `mimic dap` debugs from editors over the Debug Adapter Protocol (through LLVM's `lldb-dap`): breakpoints on `.mim` lines, stepping, the call stack, and the same values; in VS Code, F5 or **Debug** above `fn main`.

- **`mimic mcp`, Mimic for AI agents** (step 0.3.7), over the Model Context Protocol: tools to check (every error with its exact fixes, as JSON), build, run (with input and a timeout), test, format, explain errors, read the manual and search it, look up symbols and effects, and read and write the project's files; resources (the guide for agents, the glyph table, every page of the manual) and prompts (write Mimic, port code to Mimic, fix the errors). Full access inside the project; outside it, files (`--allow-outside`), installing (`--allow-install`) and running programs that use the network or run other programs (`--allow-network`) stay off until allowed; `--read-only` and `--no-run` narrow it. Set-up for VS Code, Cursor, Windsurf, Zed, Codex CLI and any MCP app: `mimic man mcp`.

- **Documentation written by people or agents** (step 0.3.8). Guides, Markdown pages in the project's `docs/` folder, join `mimic doc` beside the API; their `mimic run` examples run under `mimic test` against their `output` blocks. The site is new: guides and modules in the sidebar, Mimic code coloured, every glyph explained on hover, Mimic's colours in light and dark; `mimic doc --std` includes the manual's guides. Through `mimic mcp`, agents see what is undocumented (`docs_status`), write guides whose examples are checked as they are written (`docs_write`), and build the site (`docs_build`); the `document_project` prompt starts the job.

- **Where code runs** (step 0.3.9). `mimic why` lists every decision the compiler makes about a program or one function: `¨` on all cores, fork-join recursion, interaction nets, fused loops, SIMD, the GPU, implicit tasks, effects kept in order and the places that wait for tasks, each with its reason and what would change it. Code generation records these decisions as it makes them, and `mimic why` reads them from a compile that stops before LLVM optimises. Editors show them as labels after the code (`∥ parallel`, `⇶ fused`, `▣ GPU`, `◷ task`, `→ in order`) with the reason on hover, and agents ask with the `why` tool of `mimic mcp`.

- **`mimic audit`** (step 0.3.10): quality rules (unused functions and variables, dropped `Result`s, long, complex and duplicated functions, C functions that could be `pure`, `trusted` C functions taking pointers) and security rules (`os::shell` with commands built from data, files opened with paths from HTTP requests, secrets in source, certificate trust outside tests, `unsafe` blocks, trusted headers, dependencies following a branch), each with a manual page (`mimic man S01`). Reports as text, JSON or SARIF; `--fail-on` makes it a gate for CI (errors by default); a baseline counts only new findings; each run's counts are kept and compared. Agents get it as the `audit` tool, and CI audits the examples.

- **Contracts and property tests** (step 0.3.11). `requires` and `ensures` lines before a function's body (`result` is the value returned) are checked on every call in debug builds and tests, also when the body leaves with `return`, and left out of release builds; a broken one stops with the condition, the function and the line, and they show in signatures (hover, `mimic doc`). `test "…" for xs: [i64] { … }` runs the body for 100 generated inputs (`i64`, `f64`, `bool`, `str` and lists of them), the same on every run, and shrinks a failure to its smallest case. Both grammars know `requires`, `ensures` and `for`.

- **`mimic repl`** (step 0.3.13): Mimic a line at a time. An expression shows its value, a `let` keeps it, a `fn` or `type` stays defined, and an open bracket continues on the next line. Glyph input as you type (`\i` → `⍳`) and history with Up and Down; `:show` prints the session as a program, `:reset`, `:man`, `:glyphs`, `:quit`. Every line is compiled to native code and run; a line that does not compile shows its errors and is not kept. In `mimic edit`, the same REPL is a pane: F4 evaluates a line you type, Ctrl-E the current line or the selection.

- **The language server finds its way around**: signature help, rename (the program's own names), the call hierarchy (who calls a function, what it calls), smart selection, the inferred type of each `let` as an inlay hint with a code action that writes it in, and *Extract into a function*. `mimic query types` gives those types to other tools.
- **`mimic profile --for SECONDS`**: profiles a server (or any program that does not end by itself) for that long, timed by the program. Servers are profiled properly: the event loop's threads and the tasks on them are sampled, and waiting for the network, for work or in `time::sleep` counts as waiting.
- **Targets:** `aarch64-windows` is stable (CI runs every golden program on a Windows arm64 machine).
- **`mimic test --coverage`**: how often the tests ran each line of the program's own code, lines of functions nobody called included: a summary per file with the lines not run, `coverage.json` for editors (the VS Code extension's Coverage run) and `lcov.info` for coverage services.
- **`mimic repl --json`**: requests and answers as JSON lines, for notebooks and tools (the VS Code extension's `.mimnb` notebooks).
- **Windows**: the language server compared Windows paths in their `\\?\` form with the paths editors send, so errors and lookups could miss open files; paths are now compared and reported as editors write them. A test of the language server that hung for two hours on Windows CI (it waited for an answer to a malformed path) now finishes, and any test talking to a server stops it after three minutes without an answer.
- **`mimic net show` and `mimic net trace`**: the interaction nets of a program's pure functions (or of a `.mnet` file) as text or as graphs (`--json`: nodes, ports, wires, active pairs), also as compiled before compile-time reduction (`--before`); and a call reduced round by round with the runtime's rules, giving the result, the interactions, the redexes of each round (the parallelism the net offers) and the rules that fired, with the net before each round for drawing (`--frames`). The VS Code extension draws and plays them.
- **The VS Code extension 0.4–0.10**: finds Mimic by itself (VS Code started from the Dock does not see the PATH of `~/.zshrc`, so it asked for `mimic.path`), installs and updates it from GitHub, shows errors on save with a Mimic without the language server, profiles (a panel, shares in the code, hot functions), lists where code runs, runs the REPL, and shows and traces interaction nets.
- **`mimic profile`** (step 0.3.13): runs the program and shows where its time goes: the share per function (self, and with what it calls) beside where the compiler runs it, the runtime and system functions that took it, each thread's share of time working, GPU kernels and their time, and memory allocated and copied. The runtime samples every thread about 1000 times a second (signals on Linux and macOS, suspending the thread on Windows) and follows the frame pointers a profiling build keeps; time on the pool's workers counts for the function that started the loop. The report is saved as JSON; `--json` prints it, and `mimic mcp` has a `profile` tool. `mimic man profile` explains it.
- **Tail-recursive loops stay native.** A pure function whose calls are all in different branches (a loop written as tail recursion) no longer runs on the interaction nets, where it gained nothing: it compiles to a loop, as fast as the same loop in C (a 400-million-step loop: 20 s on the nets on 8 cores' worth of CPU, now 10 s on one core, like C). Found with `mimic profile`.
- Fused kernels are named after the function they come from (`…main__f$i$kernel3`), in profiles and debuggers.

- **Rust + Rayon in the benchmarks** (step 0.3.13): `bench/<name>/rayon/` is the hand-parallelised Rust version of seven benchmarks, timed on all cores next to Mimic on all cores in `bench/run.py`, `BENCHMARK.md` and the weekly CI run. Cargo projects in the suite now build with `-C target-cpu=native`, as the single-file Rust versions do. Mimic is ahead on six of eight with no annotations; Rayon is ahead on prefix sum and fib.

- **`mimic edit`, Mimic's own terminal editor** (step 0.3.12): in Mimic's colours, with a MIMIC start screen; highlighting from Mimic's own lexer (names from C in their own colour); glyph input (`\i` → `⍳`, never inside strings) and a glyph palette; errors as you type from the engine (a mark, the code underlined, the message); panes for problems, the audit, where code runs (also as labels beside the code), the manual for the word under the cursor, and run and test output (in the background); tabs, a file tree, a fuzzy file opener, find and replace, undo and redo, the clipboard, the mouse, indentation that follows the code. Easy keys: Ctrl-S, Ctrl-Q (asking about unsaved files), Ctrl-O, Ctrl-G, F1, F5, F6.

### Changed
- **`mimic check` is about 2.5× faster** (the showcase: 90 ms → 35 ms of analysis). Name lookups are remembered once the module tree is built, and names that turn out to be local variables no longer run the "did you mean" search, which only an error needs.

## 0.2.9 — 2026-11-15

Stage 0.2 complete: native windows on Windows and Linux, shared libraries, thousands of waiting tasks on the event loop, terminal programs, text formats, collections, C constants, globals, structs and unions, and Windows at full speed.

**Upgrading from 0.2.0:** names from C are now written with `c::` (`c::printf`, `c::sqrt`). Run `mimic check --fix` in each project to add it where it is missing. The install script replaces the old `mimic` and clears its build cache.

### Changed (breaking)
- **Everything from C is written with the namespace `c::`:** `c::printf(…)`, `c::sqrt(2.0)`, `c::Vec2 { … }`, for functions from `extern "C"` blocks, imported headers and C struct records alike. It is required, so C code is always visible, and C names never collide with Mimic's (`c::abs` next to `abs`, `c::div` next to the `div` operator). `extern "C" include "sqlite3.h" as sql` picks another namespace. A C name used without it is error E0303 with the fix (`did you mean c::floor?`). C++ will use `cpp::`. To update code: put `c::` before each C function and struct name.

### Fixed
- **Windows programs run at full speed.** Native Windows builds compiled for the plain x86-64 baseline (no AVX2, no FMA), because the MinGW host triple picked up Clang's cross-compiling defaults; they now use the machine's own CPU, as on macOS and Linux. On a Zen 3 runner: `mandelbrot` 1.14× → 0.76× C on one core, `sum_of_squares` 4.1× → 1.1×. CI now checks which CPU Windows code generation used.
- Windows: idle worker threads napped with `Sleep(0)`, which returns at once, so they spun on the cores the working threads needed; they now sleep 20 µs on a high-resolution timer (`fib` on all cores 0.89× → 0.68× C).
- Parallel prefix sums (`trail`) were slower on several threads than on one on Windows: scans now work in larger chunks (Windows 1.46× → 0.84× C on 4 threads; macOS and Linux a little faster too).
- A rare hang on Windows: two tasks using one socket (a WebSocket's reader and writer) could switch its non-blocking mode under each other, and a read then blocked a whole event-loop thread. Sockets used by tasks now stay non-blocking.
- A coroutine's lock could be freed while the thread finishing it was still releasing it (harmless with glibc, not with Windows' threads library).
- `time::format`'s message for an unknown pattern code was cut off.
- `bench/run.py` writes its results as UTF-8 on Windows.
- Tests: `http_keep_alive` takes its port below the ones the OS hands out (it could race with a parallel test); `c_struct_pointers` uses `termios.h` only on Unix.
- `"\033"` and other C octal escapes were read as NUL followed by digits, silently; they are now an error that suggests `\x1b`.
- Importing a C header failed when it declared a name Mimic uses itself (`div` in `stdlib.h`, a field `type`, a function `sum`). Under `c::` (below) they keep their C spelling; a wrapper that cannot be written in Mimic is left out instead of failing the import.

### Added
- `MIMIC_DEBUG_WATCHDOG=<seconds>`: a program still running after that long prints every task and what it waits for (a channel, a join, a socket, a timer), to find hangs. `MIMIC_DEBUG_CPU=1` prints the CPU code generation chose. `tools/speed-probe.py` and the manual "speed probe" workflow time benchmarks against C by thread count on CI machines.
- Native windows on Windows and Linux: `ui::run(…, show: "window")` (`mimic run --window`) uses Win32 controls on Windows and GTK 3 on Linux (loaded when the window opens, so programs do not depend on it), as AppKit on macOS. The same window test runs on all three in CI, Linux under a virtual display.
- Shared libraries: `mimic build --shared` builds a library project as `lib<name>.dylib`, `lib<name>.so` or `<name>.dll` (with the import library `lib<name>.dll.a`), exporting only its `extern "C" fn`s, for any language that calls C. Cross builds too (`--target x86_64-windows`).
- `¨` over a function that waits on the network or sleeps runs every element as a task on the event loop instead of a core's worth at a time: 3000 WebSocket clients each holding a connection for half a second finish in about a second (was two minutes). Programs raise their open-file limit at start, so thousands of sockets fit.
- Speed checked on Windows: CI counts AVX2/AVX-512 instructions in a native Windows build of the SIMD fixture, and the benchmark workflow runs on Windows too (`bench/run.py` works there).
- C unions: a record with one field per member (`c::Num { f: 2.5 }`), by value, in structs and through pointers; the member set is the one C sees.
- C structs: array fields are lists; `c::T { … }` leaves fields out as zero (`c::Termios {}`); plain-data structs through pointers are records (`const`: copied in; otherwise copied in and returned, in a tuple with the result), so `c::tcgetattr(0, c::Termios {})` works. Records exist for every C struct the code names.
- `mimic check --fix` adds a missing `c::` (functions and constants).
- C constants and globals: `#define` constants and enum values (`c::SEEK_END`, `c::M_PI`, `c::TIOCGWINSZ`), computed by Clang and inlined; globals and expression macros (`c::stdout`, `c::errno`) read through generated getters; `#define` aliases of functions (`c::CreateFile`); C functions with uppercase names (the Windows API). Struct, callback and getter wrappers are generated only for the C names a program uses.
- Collections: `unique`, `sort_by` (stable, `descending: true`), `zip`, `enumerate`, `any`, `all`, `count`, `group_by`, `flatten`, `min_by`, `max_by`, `clamp`, `to_map`, and a `set` module (`set::of`, `has`, `add`, `remove`, `union`, `intersection`, `difference`).
- `panic(message)` and `assert(ok, message)` in ordinary code; `os::env_all()`; `fs::walk(dir)` and `fs::modified(path)`; `time::format(t, pattern)` (`%Y-%m-%d %H:%M`); `random::choice` and `random::shuffle`.
- Terminal programs without C: `io::write` (no newline), `io::flush`, `io::read_char`, `io::read_bytes`; the `term` module (`raw`, `read_key`, `size`, `is_terminal`, `clear`, `move_to`, `show_cursor`, `alternate_screen`, undone when the program ends), on Windows too (ANSI codes are switched on); `os::on_interrupt` for Ctrl-C.
- Formats in interpolation, as in Python and Rust: `"{x:.2}"`, `"{n:>8}"`, `"{n:06}"`, `"{n:#x}"`, `"{n:,}"`, `"{x:.1%}"`; `format(x, spec)`. A bad format is a compile error.
- `char_code`, `char_of`, `pad_start`, `pad_end`, `words`, `slice`, `parse_int(text, base)`.
- `docs/gaps.md` and `tools/gaps.sh`: everyday features checked against the compiler, each with the version that closes it.
- String escapes `\x1b` (an ASCII character by two hex digits) and `\u{1F600}` (any Unicode character), so terminal colour and cursor codes can be written.
- TLS in static Linux programs: `https://` and `tls::` work in `--target x86_64-linux` (and every other `*-linux-musl`) builds, through Mbed TLS 3.6 bundled in `mimic` and built for the target on first use. They trust the system's CA bundle, or `SSL_CERT_FILE`.
- The Linux `mimic` is built against glibc 2.28, so it runs on Linux systems from 2018 on (Ubuntu 20.04, Debian 10, RHEL 8 and later).
- `NOTICE` lists the software bundled in `mimic`; release downloads include it.
- The event loop on Windows: connections and the tasks they start run as coroutines (fibers), waiting on sockets through IOCP (wepoll) instead of a thread each, as on macOS and Linux.

## 0.2.0 — 2026-11-14

Downloadable builds for Linux, macOS and Windows; Windows as a target and as a host; static Linux and Windows programs proven in CI.

### Added
- `mimic --version` shows the commit and the LLVM version.
- `MIMIC_GPU_DEBUG=1` reports whether a Metal GPU was found.
- Windows programs from any machine: `--target x86_64-windows` (and `aarch64-windows`) builds `.exe` files with the bundled mingw-w64 (UCRT); `mimic cc` compiles and links C for Windows too. Threads, implicit futures, files, time and processes work.
- Networking on Windows: TCP, UDP, HTTP, WebSockets and the browser UI through Winsock; TLS (`https://`, `tls::`, `serve_tls`) through Schannel, checked against Windows' trust store; `tls::self_signed` keys through BCrypt.
- `os::shell` uses the system shell: `/bin/sh -c`, or `cmd.exe /c` on Windows.
- CI builds the golden programs for Windows on Linux and runs them on a Windows machine (`tools/build-fixtures.sh`, `tools/run-fixtures.sh`).
- Downloads: a version tag builds `mimic` for Linux x86_64, macOS arm64 and Windows x86_64 and publishes them as a GitHub release with checksums. Each download has an install script (`install.sh`, or `install.cmd` on Windows) that puts `mimic` on `PATH` and checks what the system needs.
- Stable targets: static Linux x86_64 and arm64 (`x86_64-linux-musl`, `aarch64-linux-musl`) and Windows x86_64, whose golden programs CI now runs. `mimic targets` lists the Windows targets.
- `mimic` builds on Windows (MSVC Rust, a Windows LLVM from the llvm workflow); native builds there target MinGW (`x86_64-windows-gnu`) with this machine's CPU features. CI builds it and runs the golden programs it makes.

### Fixed
- Windows: each socket call ran once, not twice (binding, sending and receiving failed or repeated); `os::pid()` is the process id; listening ports are claimed exclusively; Winsock errors read as words.
- `time::sleep` never returns early, also where the clock wakes a little early (WASI hosts).
- The C version of the HTTP benchmark runs on Linux (epoll).
- Linux: C code (the runtime, imported headers) is compiled by the embedded Clang; the system compiler only links. Header import failed with GCC.
- Linux: a runtime function clashed with a glibc maths function (`jnl`).
- Linux: unused runtime code is dropped at link time, so raw `.mnet` programs link.
- x86-64 release builds compile the runtime for this CPU with `-march=native`.
- LLVM's target list is registered once per process (concurrent registration could hang).
- compiler-rt builds for the same target in several processes at once no longer remove each other's files.

## 0.1.0 — 2026-11-14

The first version: the foundations listed in [docs/versions.md](docs/versions.md#the-plan).

- **Language:** arrays with glyphs and words, records, unions, generics, closures, functions as values, loop words, patterns, implicit futures, `test` blocks.
- **Parallel by default:** fused array kernels on all cores, `¨` in parallel, fork-join recursion, interaction nets, SIMD across elements, Metal GPU kernels.
- **Standard library:** files, paths, processes, time, random numbers, JSON, HTTP and HTTPS, TCP, UDP, TLS, WebSockets, channels, an event loop, a browser UI.
- **C interop:** headers imported directly, structs by value, varargs, callbacks, C sources in projects, Mimic functions exported to C.
- **Toolchain:** one `mimic` binary with Clang and LLD inside; native macOS and Linux; cross builds to musl Linux targets, WebAssembly (WASI) and Apple platforms; `mimic test`, `fmt`, `doc`, `lldb`, `explain`; JSON diagnostics.
- **CI:** Linux x86_64 and macOS arm64 on every push; weekly benchmarks.
