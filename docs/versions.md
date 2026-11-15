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
5. **Before every tag:** the owner approves it.
6. **No gap forgotten:** [gaps.md](gaps.md) lists every everyday feature still missing, each with the version that closes it, and `tools/gaps.sh` checks them all against the compiler. Every problem a user reports becomes a check there the same day, and a closed gap becomes a golden test.

**Released so far:** 0.1.0, 0.2.0 (2026-11-14), and **0.2.9** (2026-11-15), which completes stage 0.2: every step from 0.2.0 to 0.2.9 below is in it. The Linux build needs glibc 2.28 or newer; Windows has the IOCP event loop (wepoll), and Windows arm64 is tested in CI. Next: stage 0.3, the Workbench.

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

## 0.3 — The Workbench: manual, editors, AI agents, audit

Every tool for working with Mimic, before the GPU stages and long before bootstrap. It is **one feature**: a single analysis engine that knows the project (files, errors, types, symbols, docs, effects, where code runs in parallel), with thin front ends on top. A capability added to the engine shows up in all of them at once.

```
                 ┌─────────────────── Mimic Workbench engine ───────────────────┐
                 │ files · parse · check · types · symbols · docs · effects ·   │
                 │ parallelism · fixes · audit rules · manual index             │
                 └──────┬───────────┬───────────┬───────────┬───────────┬───────┘
                   mimic man    mimic lsp    mimic mcp   mimic audit  mimic edit
                   (manual)     (editors)    (AI agents) (quality,    (terminal
                                                          security,    editor)
                                                          verification)
```

The front ends talk to the engine through one JSON protocol, the one the language server and MCP use. When the compiler is rewritten in Mimic (0.11), only the engine side of that protocol is rewritten; the front ends keep working.

Built with: `lsp-server` and `lsp-types` (the language server), `rmcp` (MCP), `ratatui` and `crossterm` (the terminal editor), `ropey` (the editor's text), `nucleo` (fuzzy search). All MIT or Apache 2.0, and inside the one `mimic` binary.

| Version | Contents | Done when |
|---|---|---|
| **0.3.0** | **Engine:**<ul><li>An incremental workspace: an edited file is re-checked alone, with what depends on it.</li><li>Indexes: symbols (definitions and references, the standard library, `c::` items with their header), docs, effects and parallel decisions.</li><li>`mimic query`: the engine's JSON protocol on the command line, for scripts and CI.</li></ul> | Re-checking an edited file of the showcase project takes under 50 ms |
| **0.3.1** | **`mimic man`, the manual inside `mimic`:**<ul><li>The tutorial and the whole reference, embedded: the same version as the compiler, offline.</li><li>Look up a topic, a function (`mimic man fs::read`), a glyph (`mimic man ⌿`) or its word (`mimic man keep`); full-text search (`--search`).</li><li>Runnable examples (`--run`); "Coming from C / Python / JavaScript" pages (loops, `max`, `filter`, slices in Mimic).</li><li>Error pages: `mimic man E0303` (with `mimic explain`), and every error points to its page.</li><li>Doc tests: examples in the manual and in `//` comments run under `mimic test`.</li></ul> | Every std function, glyph and error code has a page; every example passes |
| **0.3.2** | **`mimic lsp`, the language server:**<ul><li>Errors as you type; quick fixes from every compiler suggestion; formatting.</li><li>Glyph input (`\i` becomes `⍳`) and completion that shows each glyph's word and meaning.</li><li>Hover (type, docs, and for `c::` items the header they came from); go to definition and find references, into the standard library and C headers.</li><li>Semantic colours: names under `c::`, `cpp::` and `as` names in their own colour.</li><li>A VS Code extension; a TextMate grammar and a tree-sitter grammar (Neovim, Helix, Zed and others).</li></ul> | In VS Code: errors as you type, glyph input, hover, go to definition, `c::` coloured |
| **0.3.3** | **`mimic mcp`, for AI agents:**<ul><li>Tools: check, build, run, test, format, explain, manual search, symbol lookup, audit, `why`, targets, the gap checks.</li><li>Resources: the manual, the glyph table, the standard library, the "coming from" pages, so an agent learns Mimic from the source of truth.</li><li>Prompts: write idiomatic Mimic; port C or Python code to Mimic.</li><li>Safe by default: read-only, inside the project folder. Building, running and writing files are allowed only with `--allow-build`, `--allow-run`, `--allow-write`.</li></ul> | An agent writes, checks and runs a Mimic program using only the MCP tools |
| **0.3.4** | **Where code runs:**<ul><li>Labels in editors (inlay hints): `∥ parallel`, `▣ GPU`, `◷ task`, `→ in order (calls sync C)`, and where SIMD and fusion apply.</li><li>`mimic why <name>`: why something did or did not run in parallel, fuse, vectorise or go to the GPU, and what would change it.</li></ul> | Every fixture's parallel decisions are explained by `mimic why` |
| **0.3.5** | **`mimic audit`: quality and security:**<ul><li>**Quality:** unused code, unchecked `Result`s, long or complex functions, duplicated code, effects declared wider than needed, `trusted` C functions that take pointers.</li><li>**Security:** `os::shell` with text built from input, files opened from request paths, secrets in source, certificate checks turned off outside tests, `unsafe`, untrusted C imports, dependencies pinned only to a branch.</li><li>Reports in the terminal, as JSON and as SARIF; a quality gate (exit status) for CI; a baseline, so only new findings count; trends across runs.</li></ul> | Each rule has a passing and a failing fixture; CI fails on a new security finding |
| **0.3.6** | **Verification:**<ul><li>Contracts: `requires` and `ensures` on functions, checked in debug builds and tests.</li><li>Property tests: `test` blocks with generated inputs, shrunk to the smallest failing case.</li><li>Proved by the language already, and reported by `mimic audit`: memory safety, no data races.</li><li>Later (0.10): static proofs of bounds, overflow and termination where an SMT solver can show them.</li></ul> | Contract and property fixtures fail and pass as they should, with the smallest counterexample shown |
| **0.3.7** | **`mimic edit`, the terminal editor:**<ul><li>Highlighting from Mimic's own lexer (exact, `c::` in its own colour); glyph input and a glyph palette.</li><li>Panes: files, problems, audit, manual, run and test output, where code runs.</li><li>Several files, search and replace, undo history, mouse; easy keys by default (Ctrl-S, Ctrl-Q), a vim mode later.</li><li>Uses the engine directly: everything the language server knows, with no setup.</li></ul> | A whole session (edit, check, run, test, audit, read the manual) without leaving the editor |
| **0.3.8** | **`mimic repl` and `mimic profile`:**<ul><li>The REPL: one-liners such as `⍳ 10 ▷ +/`, also a pane in the editor.</li><li>The profiler: time per function, per core, CPU against GPU, memory traffic, tied to the parallel decisions of 0.3.4.</li></ul> | The profiler shows per-core and CPU/GPU time for every benchmark |

## 0.4 — One GPU layer, and Vulkan

GPU code written once for every backend, then the first backend for every vendor.

| Version | Contents | Done when |
|---|---|---|
| **0.4.0** | **Device layer:**<ul><li>An interface in the runtime: devices, buffers, copies, kernel launch, wait. Metal moves onto it.</li><li>Kernels generated once as LLVM IR from the fused expressions. Metal keeps its source path.</li></ul> | Metal results and speed unchanged; GPU fixtures green |
| **0.4.1** | **Vulkan:**<ul><li>SPIR-V kernels from LLVM's SPIR-V backend.</li><li>The Vulkan loader found at run time.</li><li>Maps and `¨` with pure numeric functions on NVIDIA, AMD and Intel GPUs.</li></ul> | GPU fixtures pass on Linux CI through Mesa's software Vulkan |
| **0.4.2** | **More kernels on Vulkan:**<ul><li>Reductions and scans.</li><li>f64 kernels where the device supports them (otherwise on the CPU).</li><li>Exactly the `--strict-math` results, like Metal.</li></ul> | Reduction and scan fixtures pass on Vulkan |
| **0.4.3** | **The `--gpu` choice:**<ul><li>`--gpu auto` (default) puts in every backend the target OS can use; the runtime picks the device present.</li><li>`--gpu vulkan,metal` includes only the backends named; `--gpu none` builds for the CPU only.</li><li>`mimic targets --gpu` lists the backends.</li></ul> | One binary runs its kernels on Metal on a Mac and on Vulkan on Linux |
| **0.4.4** | **Vulkan on Windows**<ul><li>A software Vulkan driver in Windows CI.</li><li>Vulkan on macOS through a Vulkan-on-Metal layer, optional.</li></ul> | GPU fixtures pass on Windows CI |

## 0.5 — CUDA and ROCm

Each vendor's native path, for top speed.

| Version | Contents | Done when |
|---|---|---|
| **0.5.0** | **CUDA:**<ul><li>PTX kernels from LLVM's NVPTX backend, loaded through the CUDA driver at run time.</li><li>Device memory, pinned host memory, asynchronous copies.</li></ul> | GPU fixtures pass on an NVIDIA machine (GPU runner or rented) |
| **0.5.1** | **ROCm:**<ul><li>AMDGPU code objects (LLVM backend, linked by LLD), loaded through HIP at run time.</li></ul> | GPU fixtures pass on an AMD machine |
| **0.5.2** | **Several GPUs:**<ul><li>Every device found is used. Large kernels are split across devices.</li><li>`MIMIC_GPU` chooses the devices.</li></ul> | A kernel split over two GPUs matches one GPU's result |

## 0.6 — Placement and streaming (the memory wall)

Each part of a program runs on the hardware that suits it, and data streams instead of waiting on memory.

| Version | Contents | Done when |
|---|---|---|
| **0.6.0** | **Residency:**<ul><li>Every array knows where its current copy lives (host or device).</li><li>Copies happen only when needed. A chain of GPU operations keeps its data on the device.</li></ul> | A chain of GPU maps copies its input once and its result once |
| **0.6.1** | **Cost model:** each kernel picks CPU, SIMD or GPU.<ul><li>**Inputs:** its size, the work per element, where the data is, and the speed of the link to the device (none on unified memory).</li><li>Calibrated once per machine and cached.</li></ul> | Each benchmark chooses at least as well as the best fixed choice |
| **0.6.2** | **Streaming:**<ul><li>Large arrays go through fused pipelines in cache-sized chunks, also across steps that cannot be fused, without whole-array intermediates.</li><li>Device copies are double-buffered, so they overlap with work.</li></ul> | Memory traffic of the streamed benchmarks measured below one pass per intermediate |
| **0.6.3** | **CPU and GPU together:**<ul><li>One kernel split between the CPU cores and the GPU by their measured speeds.</li><li>NUMA-aware placement on multi-socket machines.</li></ul> | Combined CPU + GPU runs faster than either alone on the mandelbrot and matmul benchmarks |
| **0.6.4** | **Beyond memory:**<ul><li>Arrays larger than RAM stream from files (memory mapping, io_uring on Linux), through the same pipelines.</li></ul> | A sum over a file larger than RAM runs at disk speed |
| **0.6.5** | **Portable fast binaries:**<ul><li>`--cpu portable` carries code for several CPU levels (x86-64 v2/v3/v4, Armv8/Armv9 with SVE) and picks the best at start.</li></ul> | One binary uses AVX-512 where present and still runs on an older CPU |

## 0.7 — Every operating system

A C library and the runtime for every OS LLVM targets, from any host.

| Version | Contents | Done when |
|---|---|---|
| **0.7.0** | **glibc Linux from any host:**<ul><li>Stubs per glibc version, e.g. `x86_64-linux-gnu.2.17` runs on old distributions.</li><li>Dynamic or static linking.</li></ul> | Programs for glibc 2.17 run on the newest and oldest supported distributions |
| **0.7.1** | **Android:**<ul><li>The NDK C library; executables and shared libraries for apps.</li><li>Tests in the Android emulator.</li></ul> | Golden fixtures pass in the emulator (arm64, x86_64) |
| **0.7.2** | **The BSDs:** FreeBSD, NetBSD, OpenBSD, DragonFly.<ul><li>C library stubs; the runtime (kqueue already exists).</li><li>Tests in virtual machines in CI.</li></ul> | Golden fixtures pass on FreeBSD; the rest link |
| **0.7.3** | **Apple from any host:** macOS and iOS programs built on Linux and Windows, through SDK stubs and code signing. | A Linux runner builds a macOS program that the Mac runner runs |
| **0.7.4** | **The other systems LLVM targets:** illumos/Solaris, Haiku, Fuchsia, AIX, z/OS, and the rest of LLVM's operating systems. | Each links a program; those with VMs or emulators run the fixtures |
| **0.7.5** | **No OS, and the web:**<ul><li>**UEFI and freestanding:** a runtime without threads or I/O, with static memory as an option.</li><li>**WebAssembly:** WASI preview 2 and WASI threads; the browser with JavaScript glue.</li></ul> | A UEFI program boots in an emulator; WASI and browser fixtures pass |

## 0.8 — Every CPU and SIMD extension

Programs run (not only compile) on every architecture LLVM supports, with SIMD for each vector extension.

| Version | Contents | Done when |
|---|---|---|
| **0.8.0** | **An emulated CI matrix (QEMU):** RISC-V 64, PowerPC 64 (LE and BE), s390x, LoongArch 64, MIPS and MIPS64, Arm 32-bit, x86 32-bit, SPARC 64. The full suite on each. | Golden fixtures pass on every architecture in the matrix |
| **0.8.1** | **Every vector extension:**<ul><li>**Scalable vectors:** SVE/SVE2, RISC-V V.</li><li>**Fixed-width:** AVX-512 masks, AltiVec/VSX, z/Architecture vector, LoongArch LSX/LASX, WebAssembly SIMD.</li></ul> | The SIMD fixture's machine code holds vector instructions on each, and the results match |
| **0.8.2** | **Run-time choice everywhere:** the best SIMD level chosen at start on every architecture family. | Portable binaries pick their level on x86, Arm, RISC-V and PowerPC |
| **0.8.3** | **Microcontrollers:**<ul><li>AVR, MSP430, Xtensa (ESP32), Arm Cortex-M, RISC-V 32.</li><li>A minimal runtime: no threads, static memory.</li></ul> | Programs link for each; Cortex-M and RISC-V 32 run in QEMU |
| **0.8.4** | **The remaining architectures:** Hexagon, VE, M68k, C-SKY, Lanai, XCore, ARC, SPARC 32, and BPF (eBPF programs). | Each compiles the fixtures; those QEMU emulates run them |

## 0.9 — Every GPU

Every GPU LLVM can generate code for, on every device class.

| Version | Contents | Done when |
|---|---|---|
| **0.9.0** | **OpenCL:** SPIR-V kernels through the OpenCL runtime, for Intel GPUs, older drivers and other accelerators. | GPU fixtures pass on a CPU OpenCL runtime (PoCL) in CI |
| **0.9.1** | **DirectX 12:** DXIL from LLVM's DirectX backend, on Windows. | GPU fixtures pass on Windows through a software DirectX device (WARP) |
| **0.9.2** | **Phones:** Metal on iOS, Vulkan on Android. | GPU fixtures pass in the iOS simulator and the Android emulator |
| **0.9.3** | **WebGPU in the browser:** kernels translated to WGSL by Mimic (LLVM has no WGSL backend). | GPU fixtures pass in a headless browser |
| **0.9.4** | **More work on GPUs:** matrix multiply on GPU matrix units, sorts, scans of records. | `matmul` and `sort` benchmarks run on each tier-1 GPU |

## 0.10 — Language and tools complete

Everything 1.0 promises, frozen.

| Version | Contents | Done when |
|---|---|---|
| **0.10.0** | **The rest of the language:** sized integers (`i8`, `i16`, `u16`, `u32`, `u64`), decoding unions, and the gaps listed in the roadmap. | Every feature in the language reference has fixtures |
| **0.10.1** | **Tools complete:**<ul><li>The Workbench (0.3) reviewed against the finished language: every rule, fix, manual page and MCP tool up to date.</li><li>`--json` for every command; inferred types per span (`--types`).</li></ul> | Every command, rule and MCP tool has fixtures; the manual covers the frozen language |
| **0.10.2** | **Packages:**<ul><li>The registry.</li><li>Versioned dependency resolution, lockfiles, workspaces.</li></ul> | A project depends on registry packages by version range |
| **0.10.3** | **Incremental compilation:** per module, with stable symbols. | Changing one module rebuilds only it and its dependents |
| **0.10.4** | **Standard library review:**<ul><li>Names and signatures settled; deprecated items removed.</li><li>The language and library references complete.</li></ul> | Every public std item documented with an example |
| **0.10.5** | **Freeze:** a language specification and a conformance suite. Only fixes after this. | The spec covers the whole language; the suite passes |

## 0.11 — Bootstrap

The compiler rewritten in Mimic.

| Version | Contents | Done when |
|---|---|---|
| **0.11.0** | **The front end in Mimic:** lexer, parser, checker, types. It runs beside the Rust one on every test. | Both front ends agree on every fixture |
| **0.11.1** | **Code generation in Mimic:** LLVM through Mimic's C interop, nets, fusion, SIMD and GPU kernels. | The Mimic code generator builds every fixture, with the same output |
| **0.11.2** | **The driver in Mimic:** projects, packages, the toolchain (Clang and LLD through the C interop).<ul><li>Stage 1 (built by the Rust compiler) builds stage 2. Stage 2 builds stage 3.</li></ul> | Stages 2 and 3 are identical bit for bit |
| **0.11.3** | **Parity:** the Mimic-built `mimic` passes the full suite on every tier-1 platform and compiles at least as fast as the Rust-built one. | CI green with the self-hosted compiler |
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
