# Versions

Mimic follows [Semantic Versioning](https://semver.org): `MAJOR.MINOR.PATCH`.

* **Before 1.0**
  * Each minor version (`0.2`, `0.3`, …) is one **stage** of the plan below.
  * `X.Y.0` is the stage's first release. Each patch `X.Y.1`, `X.Y.2`, … adds the next **step** of the stage, together with fixes.
  * A minor version may change the language or the standard library; the changes are listed in [CHANGELOG.md](../CHANGELOG.md).
  * A release made only to fix a bug takes the next patch number, and the steps after it move up by one.
* **From 1.0**
  * Nothing that compiles under `1.x` stops compiling or changes meaning in a later `1.y`.
  * Patches only fix bugs. Breaking changes wait for `2.0`, or for an edition chosen per project in `mim.mim`.
* **Mimic 1.0** has two properties:
  * **Self-hosted:** the compiler is written in Mimic and builds itself.
  * **Every target LLVM can generate code for:** every operating system, CPU architecture, SIMD extension and GPU, each at the [tier](#targets-at-10) it can be tested at.

## How a version is made

1. **One source for the number:** `version` in the workspace `Cargo.toml`. `mimic --version` prints it with the commit and the LLVM version, for example `mimic 0.1.0 (4478a0c25, LLVM 23.1.4)`.
2. **Notes as you go:** every user-visible change is written under **Unreleased** in [CHANGELOG.md](../CHANGELOG.md) in the same commit.
3. **Raising the version:** a step is released when its **Done when** checks pass and CI is green on every tier-1 platform. Then the version is raised, the Unreleased notes move under the new number, and the commit is tagged `vX.Y.Z`.
4. **What a tag does:**
   * builds `mimic` on each tier-1 platform;
   * runs the tests there;
   * attaches the binaries, checksums and changelog section to a GitHub release.
5. **Before every tag:** the owner approves it. Merging into `main` publishes nothing; only a tag does, so every release and preview reaches users through the owner's yes.
6. **Branches:**
   * `main` holds released code only: each commit on it is a release, merged from `dev` by a pull request once every required check passes (the tests on Linux, macOS and Windows and the speed gate), then tagged with the owner's approval. `git log --first-parent main` lists the releases.
   * Between releases, `main` takes one other kind of pull request: changes only to CI, meaning `.github/workflows/` and the scripts those workflows run (`bench/run.py`, `bench/record.py`, `bench/noise/`, `bench/http/load/`, `tools/ledger.py`). GitHub starts scheduled and hand-started workflows only from `main`, so new ones must reach it before the next release. They pass the same required checks; Mimic itself does not change.
   * `dev` is where work comes together. Every change reaches it as a pull request from a short-lived branch (`feat/0.4.1-ir-calls`, `fix/…`, `docs/…`), squashed into one commit once CI passes on Linux, macOS and Windows. Its version is the next one marked `-dev` (`0.4.1-dev`).
   * An urgent fix to a stable release is a `hotfix/…` branch from `main`, released as `vX.Y.Z-fix.N` and merged back into `dev`.
   * When an era of the roadmap is complete, a read-only branch keeps it as it was: `ideation` (0.1 to 0.3, at 0.3.17), then `core` (0.4), `acceleration` (0.6), `domains` (0.7), `everywhere` (0.11) and `maturity` (0.13).
7. **Channels:**

   | Channel | Tag | Made | Installed with |
   |---|---|---|---|
   | Stable | `vX.Y.Z` | when a step's **Done when** checks pass | the install command, `mimic upgrade` |
   | Preview | `vX.Y.Z-preview.N` | from `dev`, when it has something new and CI is green | `mimic upgrade --preview` |
   | Nightly | `nightly` (one, replaced) | each night from `dev`, when it changed and CI passed | `mimic upgrade --nightly` |

   Previews and nightlies are GitHub pre-releases, which are never the "latest" release, so the install command and the website only see stable releases. The channel chosen is remembered by `mimic upgrade`; `--stable` goes back.
8. **How often:**
   * A preview at least every 7 days while `dev` has new work, so a long step is never a long silence.
   * A stable release as soon as a step is done. A step that would take more than 3 weeks is split in this plan first.
   * A crash or a wrong result in a stable release is fixed in a release within 48 hours.
9. **No gap forgotten:** [gaps.md](gaps.md) lists every everyday feature still missing, each with the version that closes it, and `tools/gaps.sh` checks them all against the compiler. Every problem a user reports becomes a check there the same day, and a closed gap becomes a golden test.

**Released so far:** 0.1.0, 0.2.0 (2026-11-14), and **0.2.9** (2026-11-15), which completes stage 0.2: every step from 0.2.0 to 0.2.9 below is in it. The Linux build needs glibc 2.28 or newer; Windows has the IOCP event loop (wepoll), and Windows arm64 is tested in CI. **0.3.13** (2026-11-16) completes stage 0.3, the Workbench: every step from 0.3.0 to 0.3.13 is in it (CI green on Linux, macOS and Windows). The VS Code extension is released on GitHub (`.vsix`); publishing it to the Marketplace and Open VSX is left for later, by choice. **0.3.14** (2026-11-16) makes servers, parallel recursion and prefix sums faster and everything in `mimic edit` clickable. **0.3.15** (2026-11-16) fixes updating while an editor runs Mimic, makes `and`/`or` short-circuit, lets a profile of a server be stopped, and adds `math::to_int`. **0.3.16** (2026-11-17) makes byte, pixel and state-machine code fast (the `scale`, `filters`, `qoi` and `paths` benchmarks, a video transcoding example), and adds types on lambda parameters and `os::run_in`. **0.3.17** (2026-11-17) installs and updates Mimic from the terminal (`curl -fsSL https://mimic.codes/install.sh | sh`, `irm https://mimic.codes/install.ps1 | iex`) and from Mimic itself (`mimic upgrade`), and the editor extension with one command. **0.3.18** (2026-11-17) adds release channels: `mimic upgrade --preview` follows the previews of the next version and `mimic upgrade --nightly` the nightly builds, while work comes together on a `dev` branch and `main` holds releases only. Next: stage 0.4, fast by default and a richer vocabulary. **0.4.0-preview.1** (2026-11-18), a preview of 0.4.0: the benchmark corpus's first 16 algorithms in Mimic, C and Rust, the machine's limits, and the speed gate, which fails CI on a slowdown and was shown to on Linux, macOS and Windows (`mimic upgrade --preview`). **0.4.0-preview.2** (2026-11-18): the corpus at 54 algorithms (text, numbers and simulations added), a fix for builds running at once that could read each other's half-written runtime files, and a speed gate split over three machines per system. **0.4.0** (2026-11-20) completes the measuring stick: 130 algorithms in Mimic, C and Rust, the speed gate on all three systems, and seven standard modules taken from the corpus (`hash`, `text`, `graph`, `order`, `parse`, `compress`, and whole numbers in `math`), whose private records now hold raw numbers. **0.4.1** (2026-11-22) brings the Mimic IR: programs compiled through a typed intermediate representation, on by default, with fusion, sinking and the inlining of small helpers as passes over it (preview: 0.4.1-preview.1, 2026-11-21); the old code generator, which compiles what the IR does not cover yet, is retired in 0.4.10. **0.4.2** (2026-11-24) brings ownership by analysis: values released at their last use, handed over and changed in place wherever nothing else holds them, in the functions the IR compiles (reference-count operations over the corpus down 40%), and `mimic check --copies`. Next: 0.4.3, loops and values in registers.

---

## 0.1 — Foundations

The language, parallel by default, and the toolchain in one binary. Native on macOS and Linux.

| Version | Contents | Done when |
|---|---|---|
| **0.1.0** | **Language:** arrays with glyphs and words, records, unions, generics, closures, functions as values, loop words, patterns, implicit futures, `test` blocks.<br>**Parallel by default:** fused kernels, `¨` on all cores, fork-join recursion, interaction nets, SIMD, Metal GPU kernels.<br>**Standard library:** files, paths, processes, time, random numbers, JSON, HTTP/HTTPS, TCP, UDP, TLS, WebSockets, channels, event loop, browser UI.<br>**C interop:** both ways.<br>**One `mimic` binary** with Clang and LLD inside. Native builds on macOS and Linux; cross builds to 30 musl Linux targets, WebAssembly and every Apple platform.<br>**Tools:** `mimic test`, `fmt`, `doc`, `lldb`, `explain`.<br>**CI** on Linux x86_64 and macOS arm64. | CI green on both platforms (golden tests, tutorial, Metal kernels on the runner's GPU) |
| **0.1.1** | **Releases:**<ul><li>A tag builds and attaches `mimic` for Linux x86_64 and macOS arm64, with checksums.</li><li>The Linux build runs on older distributions (built against an old glibc, or static).</li><li>An install script (`curl … \| sh`).</li><li>`mimic --version` shows the commit and LLVM version.</li></ul> | A fresh machine runs the downloaded `mimic` and builds a project |
| **0.1.2** | **Linux performance:**<ul><li>Benchmarks complete on Linux (the C HTTP server on epoll), and BENCHMARK.md gets a Linux column.</li><li>Inside a container, the runtime takes its thread count from the container's CPU limit.</li></ul> | The weekly benchmark run is green on Linux; ratios to C recorded |
| **0.1.3** | **More tier-1 machines:**<ul><li>Linux arm64 and macOS x86_64 runners join CI.</li><li>Benchmarks run on Linux arm64.</li></ul> | CI green on all four |

## 0.2 — Windows

Programs for Windows from any machine, then `mimic` itself on Windows.

| Version | Contents | Done when |
|---|---|---|
| **0.2.0** | **Cross builds:** `--target x86_64-windows` from macOS and Linux makes an `.exe`.<ul><li>**Toolchain:** mingw-w64 (UCRT) bundled, PE linking by LLD's MinGW driver.</li><li>**Runtime on Windows:** threads, time, files, paths, environment, process spawning.</li><li>**Tests:** fixtures built on Linux run on a Windows runner.</li></ul> | Every non-network golden fixture gives the same output on Windows |
| **0.2.1** | **Networking on Windows:**<ul><li>Winsock (TCP, UDP), HTTP client and server, WebSockets.</li><li>TLS through Windows' own Schannel.</li></ul> | Network fixtures and the TLS tests pass on Windows |
| **0.2.2** | **Event loop on Windows:**<ul><li>An IOCP poller.</li><li>Coroutine switching for the Windows x64 ABI.</li><li>Timers and channels parked as on Unix.</li></ul> | 3000 concurrent WebSocket connections on the Windows runner |
| **0.2.3** | **Windows arm64, and C interop on Windows:**<ul><li>Header import from the Windows SDK headers.</li><li>The ABI suite under the Windows calling conventions.</li><li>DLL exports of `extern "C" fn`.</li></ul> | ABI suite green for x86_64 and arm64 Windows |
| **0.2.4** | **`mimic` on Windows:**<ul><li>LLVM built for Windows.</li><li>The compiler's Rust code ported (paths, processes, caches).</li><li>Native Windows builds; the full suite and benchmarks in CI; release binaries for Windows.</li></ul> | CI green on Windows x86_64 with `mimic` running natively |
| **0.2.5** | **Native windows outside macOS:**<ul><li>`ui` windows on Windows (Win32) and Linux (GTK 3, which runs on Wayland and X11, loaded at run time).</li></ul> | The window fixtures render on all three desktop systems |
| **0.2.6** | **Terminal, text and numbers** ([gaps.md](gaps.md)):<ul><li>`io::write` (no newline), `io::flush`, `io::read_char`/`read_key`, `io::read_bytes`.</li><li>A `term` module: raw mode, size, is-a-terminal; `os::on_interrupt` for Ctrl-C.</li><li>`char_code`/`char_of`; format specifiers in interpolation (`{x:.2}`, `{n:>5}`, `{n:05}`, `{n:x}`); hex, binary and octal text; `parse_int(text, base)`; `pad_start`/`pad_end`; `words`; `slice` on text.</li></ul> | A terminal game redraws a frame with colours and reads single keys with no C code; the checks of these gaps pass |
| **0.2.7** | **Collections and the system** ([gaps.md](gaps.md)):<ul><li>`unique`, `sort_by` and descending sorts, `zip`, `enumerate`, `any`/`all`, `count`, `group_by`, `flatten`, `min_by`/`max_by`, `clamp`, maps from pairs, a `set` module.</li><li>`panic` and `assert` in ordinary code; `os::env_all`; `fs::walk`, `fs::modified`; `time::format` with a pattern; `random::choice`, `random::shuffle`.</li></ul> | The checks of these gaps pass; each has a golden fixture |
| **0.2.8** | **C interop complete for C** ([gaps.md](gaps.md)):<ul><li>`#define` constants (numbers, strings), `enum` values, global variables (`c::stdout`, `c::errno`).</li><li>Structs with array fields, unions, structs C fills through a pointer (`tcgetattr`, `ioctl`).</li><li>`bytes::from_ptr`; `mimic check --fix` applies `c::` suggestions.</li></ul> | `termios.h`, `sys/ioctl.h`, `sqlite3.h` and `stdlib.h` import with nothing skipped that C programs commonly use; raw mode works through `c::` |
| **0.2.9** | **Speed proven on Windows:**<ul><li>CI counts AVX2/AVX-512 instructions in native Windows builds, as it does on Linux (NEON on macOS).</li><li>The benchmarks run on Windows in the bench workflow; `BENCHMARK.md` gets a Windows column.</li></ul> | The Windows build of the SIMD fixture holds AVX2 or AVX-512 instructions; benchmark table has Windows numbers |

## 0.3 — The Workbench: packages, manual, editors, AI agents, audit

Every tool for working with Mimic, before the GPU stages and long before bootstrap. It is **one feature**: a single analysis engine that knows the project (files, errors, types, symbols, docs, effects, where code runs in parallel), with thin front ends on top. A capability added to the engine shows up in all of them at once. The stage also finishes what Phase G left: packages with versions, values in the debugger, and benchmarks against Rust and Go.

```
                 ┌─────────────────── Mimic Workbench engine ───────────────────┐
                 │ files · packages · parse · check · types · symbols · docs ·  │
                 │ effects · parallelism · fixes · audit rules · manual index   │
                 └───┬──────────┬──────────┬──────────┬──────────┬──────────┬───┘
                 mimic man  mimic lsp  mimic mcp  mimic doc  mimic audit  mimic edit
                 (manual)   (editors,  (AI        (docs,     (quality,    (terminal
                            VS Code)   agents)    by agents  security,    editor)
                                                  too)       verification)
```

The front ends talk to the engine through one JSON protocol, the one the language server and MCP use. When the compiler is rewritten in Mimic (0.11), only the engine side of that protocol is rewritten; the front ends keep working.

**Three repositories, developed side by side.** The main repository holds the engine and every `mimic` command. Two more are submodules of it:

| Repository | Path in `mimic` | Holds |
|---|---|---|
| [`mimic-syntax`](https://github.com/mimiccodes/mimic-syntax) | `editors/syntax` | TextMate grammars for `.mim` and `.mnet`, the tree-sitter grammar, and the samples GitHub Linguist needs. Used by VS Code, by other editors and, once Mimic is used widely enough, by GitHub itself. |
| [`mimic-vscode`](https://github.com/mimiccodes/mimic-vscode) | `editors/vscode` | The VS Code extension, published to the VS Code Marketplace and Open VSX (Cursor, VSCodium). |

CI in the main repository checks that the grammar knows every glyph, word and keyword of the compiler's glyph table, so the grammar never falls behind the language.

Built with: `lsp-server` and `lsp-types` (the language server; MCP is JSON-RPC over stdio, written directly, with no async runtime), `ratatui` and `crossterm` (the terminal editor), `ropey` (the editor's text), `pulldown-cmark` (guides in `mimic doc`). All MIT or Apache 2.0, and inside the one `mimic` binary; the editor's fuzzy search is Mimic's own.

| Version | Contents | Done when |
|---|---|---|
| **0.3.0** | **Engine:**<ul><li>An incremental workspace: an edited file is re-checked alone, with what depends on it.</li><li>Indexes: symbols (definitions and references, the standard library, `c::` items with their header), docs and effects; parallel decisions joined with `mimic why` (0.3.9).</li><li>`mimic query`: the engine's JSON protocol on the command line, for scripts and CI.</li></ul> | Re-checking an edited file of the showcase project takes under 50 ms |
| **0.3.1** | **Packages from git (Phase G2, part one):**<ul><li>Installing stays git-based: `mimic install <git-url>`, as today.</li><li>A tag, branch or commit can be chosen: `mimic install <git-url>@v1.2.0`; `mim.mim` records it and `mod.mim` locks the exact commit.</li><li>`mimic install` with no URL, in a project, installs every missing git dependency at its `rev`, or at the commit `mod.mim` locked.</li><li>Workspaces: `workspace { members = [...] }`; commands at its root run in every member; one `target/`.</li><li>Later stages: version ranges, fetching on the first build, and a hosted registry (`mimic install tensor`, `mimic publish`).</li></ul> | A project depending on libraries from git at chosen tags builds on a clean machine after `mimic install`; a workspace's members check, test and build from its root |
| **0.3.2** | **`mimic man`, the manual inside `mimic`:**<ul><li>The tutorial and the whole reference, embedded: the same version as the compiler, offline.</li><li>Look up a topic, a function (`mimic man fs::read`), a glyph (`mimic man ⌿`) or its word (`mimic man keep`); full-text search (`--search`).</li><li>Runnable examples (`--run`); "Coming from C / Python / JavaScript" pages (loops, `max`, `filter`, slices in Mimic).</li><li>"Writing Mimic for agents": idioms, no loops, glyphs and words, the usual mistakes of code ported from C or Python.</li><li>Error pages: `mimic man E0303` (with `mimic explain`), and every error points to its page.</li><li>Doc tests: examples in the manual and in `//` comments run under `mimic test`.</li></ul> | Every std function, glyph and error code has a page; every example passes |
| **0.3.3** | **Grammars (`mimic-syntax`):**<ul><li>TextMate grammars for `.mim` (also `mim.mim`, `build.mim`, `mod.mim`) and `.mnet`; a tree-sitter grammar for `.mim` (Neovim, Helix, Zed).</li><li>Checked in CI against the compiler's glyph table and every golden fixture.</li><li>Linguist samples and `.gitattributes` (`mod.mim` marked generated); the request to GitHub Linguist once `.mim` files are in enough repositories (GitHub asks for about 200).</li></ul> | Every fixture highlights with no unknown token; the glyph table check passes |
| **0.3.4** | **`mimic lsp`, the language server:**<ul><li>Errors as you type; quick fixes from every compiler suggestion; formatting.</li><li>Glyph input (`\i` becomes `⍳`) and completion that shows each glyph's word and meaning.</li><li>Hover (type, docs, and for `c::` items the header they came from); go to definition and find references, into the standard library and C headers.</li><li>Semantic colours: names under `c::`, `cpp::` and `as` names in their own colour.</li></ul> | Errors as you type, glyph input, hover and go to definition work in any LSP editor |
| **0.3.5** | **The VS Code extension (`mimic-vscode`):**<ul><li>Everything of the language server; the grammars; Mimic's icons for `.mim` and `.mnet` files.</li><li>A glyph palette; Run and Test above functions and tests; the test explorer; the manual in a side panel.</li><li>Kept current: the extension has versions of its own, asks the installed `mimic` what it can do (and says which Mimic brings a missing feature), and installs or updates Mimic from its GitHub releases.</li><li>Released as a `.vsix` on GitHub; the Marketplace and Open VSX later (the release workflow publishes there once their tokens are set).</li></ul> | A fresh VS Code with the extension installed edits, runs and tests a project with no other setup |
| **0.3.6** | **Debugging (Phase G4 finished):**<ul><li>Lists, text, maps and records shown as values in `mimic lldb`, not as pointers.</li><li>`mimic dap`, the Debug Adapter Protocol: breakpoints, stepping and values in VS Code and other editors.</li></ul> | A breakpoint in VS Code shows a list of records as values |
| **0.3.7** | **`mimic mcp`, for AI agents:**<ul><li>Tools: check, build, run, test, format, explain, the manual and its search, symbol lookup, effects, targets, the project's files; audit, `why` and the doc tools join with their steps (0.3.8 to 0.3.10).</li><li>Resources: the manual, the glyph table, the standard library, "Writing Mimic for agents", so an agent learns Mimic from the source of truth.</li><li>Prompts: write idiomatic Mimic; port C or Python code to Mimic; document a module.</li><li>**Full access inside the project by default:** reading, writing files, building, running and testing. Outside it, off until the user turns it on: `--allow-outside` (files elsewhere), `--allow-install` (fetching packages), `--allow-network` (running programs that use the network or run other programs). Anything can be narrowed: `--read-only`, `--no-run`.</li><li>A setup guide in the manual (`mimic man mcp`): the standard MCP configuration every client accepts, and steps for the common editors and agent apps.</li></ul> | An agent writes, checks, runs and documents a Mimic program using only the MCP tools |
| **0.3.8** | **`mimic doc`, written by people and agents:**<ul><li>The facts come from the compiler: signatures, types, effects, where code runs.</li><li>Guides, overviews and examples are Markdown pages in `docs/`, merged into the site; agents write them through MCP (what is undocumented, the outline, write a page, check its examples).</li><li>Every example runs as a doc test, so a wrong example fails `mimic test`.</li><li>A site in Mimic's colours: glyph tooltips, runnable examples, search, light and dark.</li><li>Works with no agent at all.</li></ul> | The standard library's docs, with guides written through MCP, build and every example passes |
| **0.3.9** | **Where code runs:**<ul><li>Labels in editors (inlay hints): `∥ parallel`, `▣ GPU`, `◷ task`, `→ in order (calls sync C)`, and where SIMD and fusion apply.</li><li>`mimic why <name>`: why something did or did not run in parallel, fuse, vectorise or go to the GPU, and what would change it.</li></ul> | Every fixture's parallel decisions are explained by `mimic why` |
| **0.3.10** | **`mimic audit`: quality and security:**<ul><li>**Quality:** unused code, unchecked `Result`s, long or complex functions, duplicated code, effects declared wider than needed, `trusted` C functions that take pointers.</li><li>**Security:** `os::shell` with text built from input, files opened from request paths, secrets in source, certificate checks turned off outside tests, `unsafe`, untrusted C imports, dependencies pinned only to a branch.</li><li>Reports in the terminal, as JSON and as SARIF; a quality gate (exit status) for CI; a baseline, so only new findings count; trends across runs.</li></ul> | Each rule has a passing and a failing fixture; CI fails on a new security finding |
| **0.3.11** | **Verification:**<ul><li>Contracts: `requires` and `ensures` on functions, checked in debug builds and tests.</li><li>Property tests: `test` blocks with generated inputs, shrunk to the smallest failing case.</li><li>Proved by the language already, and reported by `mimic audit`: memory safety, no data races.</li><li>Later (0.10): static proofs of bounds, overflow and termination where an SMT solver can show them.</li></ul> | Contract and property fixtures fail and pass as they should, with the smallest counterexample shown |
| **0.3.12** | **`mimic edit`, Mimic's own terminal editor:**<ul><li>Made for Mimic: a theme in Mimic's colours, the MIMIC start screen, highlighting from Mimic's own lexer (exact, `c::` in its own colour), glyph input and a glyph palette.</li><li>Panes: files, problems, audit, manual, run and test output, where code runs.</li><li>Several files, search and replace, undo history, mouse; easy keys by default (Ctrl-S, Ctrl-Q), a vim mode later.</li><li>Uses the engine directly: everything the language server knows, with no setup.</li></ul> | A whole session (edit, check, run, test, audit, read the manual) without leaving the editor |
| **0.3.13** | **`mimic repl`, `mimic profile`, and benchmarks against Rust and Go (Phase G5):**<ul><li>The REPL: one-liners such as `⍳ 10 ▷ +/`, also a pane in the editor.</li><li>The profiler: time per function, per core, CPU against GPU, memory traffic, tied to the parallel decisions of 0.3.9.</li><li>The benchmark suite gains Rust (one thread and Rayon) and Go columns, in CI on every platform.</li></ul> | The profiler shows per-core and CPU/GPU time for every benchmark; `BENCHMARK.md` has Rust and Go columns |

## 0.4 — Fast by default, and a richer vocabulary

The compiler gets a real optimizing core, so speed comes from general analyses instead of recognised shapes (every slowdown found while building the 0.3.16 benchmarks was a program shape the compiler did not know), and the language gets the array vocabulary that graphics, networking, systems and quantum code build on. Nothing in existing programs changes: diagnostics stay warnings, and `i64` results stay exactly what they are. The goal for the stage, on every benchmark and on macOS, Linux and Windows: within 1.2× of C on one core, and at or ahead of Rust + Rayon on all cores, each benchmark reported as a share of what the machine can do. And the promise must hold for programs nobody tuned the compiler on: a **held-out set** of programs written plainly, the way a newcomer would, is measured at every 0.4 release and never used to tune anything (a slow spot it shows gets a corpus program of its own to tune on, and a gap row with the 0.4 step that closes it); at the end of the stage its geometric mean is within 1.5× of C on one core, with no program above 3×. Design notes: [docs/design/ir.md](design/ir.md) and [docs/design/vocabulary.md](design/vocabulary.md).

| Version | Contents | Done when |
|---|---|---|
| **0.4.0** | **The measuring stick:**<ul><li>A corpus of 130 everyday algorithms (parsers, compression, hashing, graph search, simulations, text, geometry, images, data structures), each with C and Rust versions; the algorithms people reach for became standard modules (`hash`, `text`, `graph`, `order`, `parse`, `compress`, whole numbers in `math`).</li><li>The machine's limits measured (peak compute and memory bandwidth), and each benchmark shown as a share of them.</li><li>Ratio gates in CI: a slowdown fails the build.</li></ul> | An injected slowdown fails CI on all three systems |
| **0.4.1** | **Mimic IR:**<ul><li>A typed intermediate representation (SSA, with structured control flow) between the syntax tree and LLVM.</li><li>Fusion, inlining and the choice of where code runs become passes over it; interaction nets are translated from it.</li><li>Introduced one function at a time; what it does not cover yet is compiled by the old code generator where it stands (embedded), until the old code generator is retired (0.4.10).</li><li>Lambdas that do not escape compiled for each place they are used (a `let` lambda, a step inside a parallel map), and small helpers inlined before fusion.</li><li>A program's values not freed one by one at exit; the GPU asked only once a map is large enough for it.</li><li>The HTTP client refuses a redirect from `https` to `http`, and drops `Authorization`, `Cookie` and `Proxy-Authorization` when a redirect changes host or scheme.</li></ul> | Every golden test passes through the IR; benchmarks within ±3% |
| **0.4.2** | **Ownership by analysis** (as in Perceus): reference counting placed by the compiler, borrowing inferred, a value changed in place wherever nothing else holds it, debug builds included, through helper calls, and a result built where its caller keeps it. `mimic check --copies` lists every copy left and why it is made. | The `qoi` and `paths` steps copy nothing; reference-count operations in the corpus at least halved. *Released short of both: `paths`' step copies nothing; reference-count operations fell 40% (937 → 563 million); `qoi`'s steps are not in the IR yet. Moved, with a result built where its caller keeps it, to 0.4.3 and 0.4.4.* |
| **0.4.3** | **Loops and values in registers:**<ul><li>`settle`, `evolve` and tail recursion become loops with the state's fields in registers. *(begun: a state on the heap, a record or a list, looped over in the IR, as `paths`' search is)*</li><li>Escape analysis: values that stay in a function live on its stack, at any size.</li><li>`Option`, tuples and small variants unboxed, tuples holding lists and records included (union-find returning `(links, root)`, `qoi`'s `chunk`) *(`qoi`'s `chunk`: inlined where its tuple is unpacked before a record update, no tuple or list made; a tuple of 2 to 4 numbers and values on the heap returned as its parts where the caller takes it apart, as union-find's `(links, root)`)*; short strings (up to 15 bytes) stored inline.</li><li>A result built where its caller keeps it: a list grown in a loop's state written into the state's own list (moved from 0.4.2). *(a list chosen by an `if` and then put back in the state, as `lz77`'s decoder does: changed in place too)*</li><li>A memory check in debug builds: a value freed is marked so and filled with junk, and a retain, a release or a read of one stops the program at once with the place and the value's kind, instead of failing later or not at all (as a list freed before its last read went unseen while its memory stayed intact). The other half of debug builds' leak check. ✅ *(the place: `mimic lldb` stops there, with the stack)*</li><li>Differential testing in CI: every run fixture in release and debug builds, through the Mimic IR and without it, on one thread and on all cores, with the same output required of each; and a generator of random valid Mimic programs run the same ways, nightly, each disagreement kept as a new fixture. ✅ *(the fixtures: `run_fixtures`, five ways, and again without the IR; random programs: `tools/fuzz.py`, nightly in `fuzz.yml` on Linux and macOS, a disagreement uploaded with each way's output)*</li><li>First, before any of the above: the held-out set (`bench/heldout/`), 40 programs written plainly across the corpus's fields (no workarounds for known slow spots), each with C and an expected output, then frozen; measured on Linux, macOS and Windows at every release from here on.</li></ul> | Steady-state loops allocate nothing; `qoi` within 1.5× of C, `paths` within 1.3×; reference-count operations of the corpus at most half of 0.4.1's 937 million (moved from 0.4.2); every run fixture passes the memory check and agrees across the five ways of running it on Linux, macOS and Windows, and the nightly generator has run a week with every disagreement it found fixed |
| **0.4.4** | **Types everywhere, checks proven away:**<ul><li>A warning, with the reason, where code falls back to dynamic types (`mimic why`, editors, the audit).</li><li>Lists of lists, lists read out of them or out of tuples, and the empty list `[]` typed from where they are used (`vm`, `tokenize`, `brainf`, `qoi`). *Begun in 0.4.3: lists of lists typed by their rows, `[[i64]]`, an element read a bounds check and a load; `[]` as one branch of an `if` typed by the other.*</li><li>Narrow integers in machine code (8, 16 and 32 bits) with the same `i64` results.</li><li>Range analysis removes the bounds checks and floored-division corrections it can prove unneeded.</li></ul> | The hot loops of `scale` and `filters` carry no checks; twice the SIMD lanes where narrow; `qoi`'s steps in the IR and copying nothing (`mimic check --copies`, moved from 0.4.2) |
| **0.4.5** | **Vocabulary I, the array core:** `⍋ ⍒` (grade), `⍸` (where), `∊` (in), `⍷` (find), `⊐` (index of), `⊇` (select), `⌸` (group), `⍤` (rank: rank polymorphism), `∘.` (outer product), an inner product, `↕` (windows), `⌺` (stencil), shifts, `⊖` (rotate), `⊤ ⊥` (encode, decode), `⌹` (solve), `√`. Each with a word and a digraph; all fused and parallel. | Every glyph has a manual page and tests; the image filters written without indexing match C |
| **0.4.6** | **Vocabulary II, numbers:**<ul><li>Complex numbers (`c64`, `c128`, literals like `3j4`).</li><li>`⊗` (tensor product) and `†` (adjoint).</li><li>`math::fft`, linear algebra and statistics on the new glyphs.</li></ul> | An FFT and a matrix decomposition within 1.2× of C |
| **0.4.7** | **Bits, bytes and small vectors:**<ul><li>Unsigned and sized integers (`u16`, `u32`, `u64`, `i16`).</li><li>Bit syntax: binary data built and matched in one expression (`<<version: 4, length: 16, rest: bytes>>`).</li><li>Packed records, zero-copy byte views.</li><li>Swizzles (`v.xyz`), color (`#ff8800`) and unit (`12px`) literals.</li></ul> | A TCP/IP header parser and a PNG chunk reader in bit syntax; swizzles compile to register moves |
| **0.4.8** | **Data layout:** lists of records stored flat (packed, or one array per field), records with text fields included; arenas for short-lived values, one per thread for each step of `¨`, `evolve` and `settle`. | `records` within 1.2× of C |
| **0.4.9** | **The runtime inside the optimizer:** the runtime as LLVM bitcode, optimized with the program; plain (non-atomic) reference counts for values that never leave their thread. **Build time:** the prelude parsed and checked once per compiler version, and the runtime linked as one prebuilt archive (moved from 0.4.1: see [gaps.md](gaps.md)). | Element access, append and retain/release are inline in hot loops; a one-line program builds in under 0.25 s |
| **0.4.10** | **One cost model, profiles, every CPU at its best:**<ul><li>The old code generator retired: each kind of expression moves into the Mimic IR along 0.4.2 to 0.4.10 (moved from 0.4.1, where it would have taken weeks with no gain in speed), calls run apart (in parallel or as tasks) included.</li><li>One model decides where code runs (fused kernel, SIMD, fork-join, interaction nets, tasks).</li><li>`mimic build --pgo` from `mimic profile` data; chunk sizes and thread counts tuned per machine.</li><li>`--cpu portable`: several CPU levels in one binary (x86-64 v2/v3/v4, Armv8/Armv9 with SVE), the best picked at start.</li></ul> | Each benchmark's choice at least as good as the best fixed choice; one binary uses AVX-512 where present |
| **0.4.11** | **Task and pipeline parallelism:** independent pure `let`s and calls run at once; pipelines overlap their stages. | The two searches of `paths` run at once; the transcoding example overlaps its stages |
| **0.4.12** | **Fast libraries:** vector kernels per CPU family behind the glyphs (sort, scan, search, hashing, UTF-8); a priority queue, hash maps and sorted sets; JSON and HTTP tuned; `mimic ffi bind <header.h>`. | `http` within 1.1× of C, `json` at C or faster |
| **0.4.13** | **Performance audit rules:** quadratic `concat`, a copy every step, a hot loop on dynamic types, a scan of a whole array inside a loop; each with a suggested rewrite. | Each rule fires on its test and stays quiet on the corpus. Closing the stage: every "done when" above met (moved ones included) and every gap row given a 0.4 step closed; the held-out set within 1.5× of C on one core (geometric mean), no program above 3× |

Alongside these, each in the release where it is ready, with no new syntax:

* **C libraries per platform:** `target linux`, `target macos` and `target windows` sections in `mim.mim` for `ffi` and `dependencies`; a file chosen by its name for the platform being built (`term.windows.mim`, `term.unix.mim`, the same module); `target::os()`, `target::arch()` and `target::family()` known while compiling, so the branch for another platform is not built. (Loading a library only if it is present, when the program runs, needs its own design first.)
* **A fast `mimic run`, and hot reload** (on the IR): optimised code cached per function, keyed by its body and what it calls, so `mimic run` is optimised by default and rebuilds only what changed (`mimic build` stays the fastest, with whole-program fusion and parallelism); then functions swapped while the program runs, at the points where that is safe (a step of `evolve` or `settle`, a UI frame, an HTTP request), first in `mimic edit` and `mimic run --watch`, with a clear rule for a record whose fields changed.
* **Steadier speed measurements:** the runtime's functions aligned, so a benchmark's time does not move with where its code lands in memory.

## 0.5 — One GPU layer, and Vulkan

GPU code written once for every backend, then the first backend for every vendor.

| Version | Contents | Done when |
|---|---|---|
| **0.5.0** | **Device layer:**<ul><li>An interface in the runtime: devices, buffers, copies, kernel launch, wait. Metal moves onto it.</li><li>Kernels generated once as LLVM IR from the fused expressions. Metal keeps its source path.</li></ul> | Metal results and speed unchanged; GPU fixtures green |
| **0.5.1** | **Vulkan:**<ul><li>SPIR-V kernels from LLVM's SPIR-V backend.</li><li>The Vulkan loader found at run time.</li><li>Maps and `¨` with pure numeric functions on NVIDIA, AMD and Intel GPUs.</li></ul> | GPU fixtures pass on Linux CI through Mesa's software Vulkan |
| **0.5.2** | **More kernels on Vulkan:**<ul><li>Reductions and scans.</li><li>f64 kernels where the device supports them (otherwise on the CPU).</li><li>Exactly the `--strict-math` results, like Metal.</li></ul> | Reduction and scan fixtures pass on Vulkan |
| **0.5.3** | **The `--gpu` choice:**<ul><li>`--gpu auto` (default) puts in every backend the target OS can use; the runtime picks the device present.</li><li>`--gpu vulkan,metal` includes only the backends named; `--gpu none` builds for the CPU only.</li><li>`mimic targets --gpu` lists the backends.</li></ul> | One binary runs its kernels on Metal on a Mac and on Vulkan on Linux |
| **0.5.4** | **Vulkan on Windows**<ul><li>A software Vulkan driver in Windows CI.</li><li>Vulkan on macOS through a Vulkan-on-Metal layer, optional.</li></ul> | GPU fixtures pass on Windows CI |

## 0.6 — CUDA and ROCm

Each vendor's native path, for top speed.

| Version | Contents | Done when |
|---|---|---|
| **0.6.0** | **CUDA:**<ul><li>PTX kernels from LLVM's NVPTX backend, loaded through the CUDA driver at run time.</li><li>Device memory, pinned host memory, asynchronous copies.</li></ul> | GPU fixtures pass on an NVIDIA machine (GPU runner or rented) |
| **0.6.1** | **ROCm:**<ul><li>AMDGPU code objects (LLVM backend, linked by LLD), loaded through HIP at run time.</li></ul> | GPU fixtures pass on an AMD machine |
| **0.6.2** | **Several GPUs:**<ul><li>Every device found is used. Large kernels are split across devices.</li><li>`MIMIC_GPU` chooses the devices.</li></ul> | A kernel split over two GPUs matches one GPU's result |

## 0.7 — Domains: graphics, quantum, protocols

Libraries for whole fields, built on the 0.4 vocabulary and the GPU stages.

| Version | Contents | Done when |
|---|---|---|
| **0.7.0** | **`gfx::`:** vectors, matrices, transforms, colors and images; shaders written in Mimic, compiled for Metal and Vulkan. | A rendering example runs its shaders on Metal and Vulkan from one source |
| **0.7.1** | **`quantum::`:** qubit registers as state vectors, gates (H, X, CNOT, rotations), circuits as values, measurement; simulated on every core and on GPUs. | A 28-qubit circuit simulates at the speed of the best CPU and GPU simulators |
| **0.7.2** | **Protocols:** DNS, TLS records, HTTP/2 frames, PNG and WAV written with bit syntax. | Each parses and builds its reference test vectors |

## 0.8 — Placement and streaming (the memory wall)

Each part of a program runs on the hardware that suits it, and data streams instead of waiting on memory.

| Version | Contents | Done when |
|---|---|---|
| **0.8.0** | **Residency:**<ul><li>Every array knows where its current copy lives (host or device).</li><li>Copies happen only when needed. A chain of GPU operations keeps its data on the device.</li></ul> | A chain of GPU maps copies its input once and its result once |
| **0.8.1** | **Cost model:** each kernel picks CPU, SIMD or GPU.<ul><li>**Inputs:** its size, the work per element, where the data is, and the speed of the link to the device (none on unified memory).</li><li>Calibrated once per machine and cached.</li></ul> | Each benchmark chooses at least as well as the best fixed choice |
| **0.8.2** | **Streaming:**<ul><li>Large arrays go through fused pipelines in cache-sized chunks, also across steps that cannot be fused, without whole-array intermediates.</li><li>Device copies are double-buffered, so they overlap with work.</li></ul> | Memory traffic of the streamed benchmarks measured below one pass per intermediate |
| **0.8.3** | **CPU and GPU together:**<ul><li>One kernel split between the CPU cores and the GPU by their measured speeds.</li><li>NUMA-aware placement on multi-socket machines.</li></ul> | Combined CPU + GPU runs faster than either alone on the mandelbrot and matmul benchmarks |
| **0.8.4** | **Beyond memory:**<ul><li>Arrays larger than RAM stream from files (memory mapping, io_uring on Linux), through the same pipelines.</li></ul> | A sum over a file larger than RAM runs at disk speed |

## 0.9 — Every operating system

A C library and the runtime for every OS LLVM targets, from any host.

| Version | Contents | Done when |
|---|---|---|
| **0.9.0** | **glibc Linux from any host:**<ul><li>Stubs per glibc version, e.g. `x86_64-linux-gnu.2.17` runs on old distributions.</li><li>Dynamic or static linking.</li></ul> | Programs for glibc 2.17 run on the newest and oldest supported distributions |
| **0.9.1** | **Android:**<ul><li>The NDK C library; executables and shared libraries for apps.</li><li>Tests in the Android emulator.</li></ul> | Golden fixtures pass in the emulator (arm64, x86_64) |
| **0.9.2** | **The BSDs:** FreeBSD, NetBSD, OpenBSD, DragonFly.<ul><li>C library stubs; the runtime (kqueue already exists).</li><li>Tests in virtual machines in CI.</li></ul> | Golden fixtures pass on FreeBSD; the rest link |
| **0.9.3** | **Apple from any host:** macOS and iOS programs built on Linux and Windows, through SDK stubs and code signing. | A Linux runner builds a macOS program that the Mac runner runs |
| **0.9.4** | **The other systems LLVM targets:** illumos/Solaris, Haiku, Fuchsia, AIX, z/OS, and the rest of LLVM's operating systems. | Each links a program; those with VMs or emulators run the fixtures |
| **0.9.5** | **No OS, and the web:**<ul><li>**UEFI and freestanding:** a runtime without threads or I/O, with static memory as an option.</li><li>**WebAssembly:** WASI preview 2 and WASI threads; the browser with JavaScript glue.</li></ul> | A UEFI program boots in an emulator; WASI and browser fixtures pass |

## 0.10 — Every CPU and SIMD extension

Programs run (not only compile) on every architecture LLVM supports, with SIMD for each vector extension.

| Version | Contents | Done when |
|---|---|---|
| **0.10.0** | **An emulated CI matrix (QEMU):** RISC-V 64, PowerPC 64 (LE and BE), s390x, LoongArch 64, MIPS and MIPS64, Arm 32-bit, x86 32-bit, SPARC 64. The full suite on each. | Golden fixtures pass on every architecture in the matrix |
| **0.10.1** | **Every vector extension:**<ul><li>**Scalable vectors:** SVE/SVE2, RISC-V V.</li><li>**Fixed-width:** AVX-512 masks, AltiVec/VSX, z/Architecture vector, LoongArch LSX/LASX, WebAssembly SIMD.</li></ul> | The SIMD fixture's machine code holds vector instructions on each, and the results match |
| **0.10.2** | **Run-time choice everywhere:** the best SIMD level chosen at start on every architecture family. | Portable binaries pick their level on x86, Arm, RISC-V and PowerPC |
| **0.10.3** | **Microcontrollers:**<ul><li>AVR, MSP430, Xtensa (ESP32), Arm Cortex-M, RISC-V 32.</li><li>A minimal runtime: no threads, static memory.</li></ul> | Programs link for each; Cortex-M and RISC-V 32 run in QEMU |
| **0.10.4** | **The remaining architectures:** Hexagon, VE, M68k, C-SKY, Lanai, XCore, ARC, SPARC 32, and BPF (eBPF programs). | Each compiles the fixtures; those QEMU emulates run them |

## 0.11 — Every GPU

Every GPU LLVM can generate code for, on every device class.

| Version | Contents | Done when |
|---|---|---|
| **0.11.0** | **OpenCL:** SPIR-V kernels through the OpenCL runtime, for Intel GPUs, older drivers and other accelerators. | GPU fixtures pass on a CPU OpenCL runtime (PoCL) in CI |
| **0.11.1** | **DirectX 12:** DXIL from LLVM's DirectX backend, on Windows. | GPU fixtures pass on Windows through a software DirectX device (WARP) |
| **0.11.2** | **Phones:** Metal on iOS, Vulkan on Android. | GPU fixtures pass in the iOS simulator and the Android emulator |
| **0.11.3** | **WebGPU in the browser:** kernels translated to WGSL by Mimic (LLVM has no WGSL backend). | GPU fixtures pass in a headless browser |
| **0.11.4** | **More work on GPUs:** matrix multiply on GPU matrix units, sorts, scans of records. | `matmul` and `sort` benchmarks run on each tier-1 GPU |

## 0.12 — Language and tools complete

Everything 1.0 promises, frozen.

| Version | Contents | Done when |
|---|---|---|
| **0.12.0** | **The rest of the language:** sized integers (`i8`, `i16`, `u16`, `u32`, `u64`), decoding unions, and the gaps listed in the roadmap. | Every feature in the language reference has fixtures |
| **0.12.1** | **Tools complete:**<ul><li>The Workbench (0.3) reviewed against the finished language: every rule, fix, manual page and MCP tool up to date.</li><li>`--json` for every command; inferred types per span (`--types`).</li></ul> | Every command, rule and MCP tool has fixtures; the manual covers the frozen language |
| **0.12.2** | **Packages:**<ul><li>The registry.</li><li>Versioned dependency resolution, lockfiles, workspaces.</li></ul> | A project depends on registry packages by version range |
| **0.12.3** | **Incremental compilation:** per module, with stable symbols. | Changing one module rebuilds only it and its dependents |
| **0.12.4** | **Standard library review:**<ul><li>Names and signatures settled; deprecated items removed.</li><li>The language and library references complete.</li></ul> | Every public std item documented with an example |
| **0.12.5** | **Freeze:** a language specification and a conformance suite. Only fixes after this. | The spec covers the whole language; the suite passes |

## 0.13 — Bootstrap

The compiler rewritten in Mimic.

| Version | Contents | Done when |
|---|---|---|
| **0.13.0** | **The front end in Mimic:** lexer, parser, checker, types. It runs beside the Rust one on every test. | Both front ends agree on every fixture |
| **0.13.1** | **Code generation in Mimic:** LLVM through Mimic's C interop, nets, fusion, SIMD and GPU kernels. | The Mimic code generator builds every fixture, with the same output |
| **0.13.2** | **The driver in Mimic:** projects, packages, the toolchain (Clang and LLD through the C interop).<ul><li>Stage 1 (built by the Rust compiler) builds stage 2. Stage 2 builds stage 3.</li></ul> | Stages 2 and 3 are identical bit for bit |
| **0.13.3** | **Parity:** the Mimic-built `mimic` passes the full suite on every tier-1 platform and compiles at least as fast as the Rust-built one. | CI green with the self-hosted compiler |
| **1.0.0-rc.N** | **Release candidates:** the self-hosted compiler is the release. The Rust compiler is kept only as a frozen seed for bootstrapping from source. | No open release blockers |

## 1.0.0

* **Self-hosted:** the self-hosted compiler is the only one shipped.
* **Every target at its tier:** every LLVM target meets its [tier](#targets-at-10).
* **The compatibility promise starts.**

---

## Targets at 1.0

What Mimic promises for each of LLVM's targets:

| Tier | Promise | Targets |
|---|---|---|
| 1 | Built and tested on real machines in CI for every commit; release binaries of `mimic` | **Systems:** Linux x86_64 and arm64, macOS arm64 and x86_64, Windows x86_64 and arm64.<br>**GPUs:** Metal, Vulkan, CUDA, AMDGPU. |
| 2 | Programs built and the full test suite run under emulation, simulators or virtual machines in CI | **Linux:** every other LLVM architecture (musl and glibc).<br>**Phones:** Android and iOS simulators.<br>**WebAssembly:** WASI and the browser.<br>**Other systems:** FreeBSD and the others with virtual machines.<br>**GPU APIs:** OpenCL, DirectX (software devices), WebGPU. |
| 3 | Programs built and linked in CI; not run | **Microcontrollers and no-OS:** AVR, MSP430, BPF, UEFI and the like.<br>**Systems without a test machine:** AIX, z/OS.<br>**GPU and DSP targets without test hardware.** |

Every LLVM triple at least compiles Mimic programs. `mimic targets` lists each with its tier.
