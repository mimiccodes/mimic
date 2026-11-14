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
| **0.2.5** | **Native windows outside macOS:**<ul><li>`ui` windows on Windows (Win32) and Linux (Wayland, with X11 as fallback).</li></ul> | The window fixtures render on all three desktop systems |

## 0.3 — One GPU layer, and Vulkan

GPU code written once for every backend, then the first backend for every vendor.

| Version | Contents | Done when |
|---|---|---|
| **0.3.0** | **Device layer:**<ul><li>An interface in the runtime: devices, buffers, copies, kernel launch, wait. Metal moves onto it.</li><li>Kernels generated once as LLVM IR from the fused expressions. Metal keeps its source path.</li></ul> | Metal results and speed unchanged; GPU fixtures green |
| **0.3.1** | **Vulkan:**<ul><li>SPIR-V kernels from LLVM's SPIR-V backend.</li><li>The Vulkan loader found at run time.</li><li>Maps and `¨` with pure numeric functions on NVIDIA, AMD and Intel GPUs.</li></ul> | GPU fixtures pass on Linux CI through Mesa's software Vulkan |
| **0.3.2** | **More kernels on Vulkan:**<ul><li>Reductions and scans.</li><li>f64 kernels where the device supports them (otherwise on the CPU).</li><li>Exactly the `--strict-math` results, like Metal.</li></ul> | Reduction and scan fixtures pass on Vulkan |
| **0.3.3** | **The `--gpu` choice:**<ul><li>`--gpu auto` (default) puts in every backend the target OS can use; the runtime picks the device present.</li><li>`--gpu vulkan,metal` includes only the backends named; `--gpu none` builds for the CPU only.</li><li>`mimic targets --gpu` lists the backends.</li></ul> | One binary runs its kernels on Metal on a Mac and on Vulkan on Linux |
| **0.3.4** | **Vulkan on Windows**<ul><li>A software Vulkan driver in Windows CI.</li><li>Vulkan on macOS through a Vulkan-on-Metal layer, optional.</li></ul> | GPU fixtures pass on Windows CI |

## 0.4 — CUDA and ROCm

Each vendor's native path, for top speed.

| Version | Contents | Done when |
|---|---|---|
| **0.4.0** | **CUDA:**<ul><li>PTX kernels from LLVM's NVPTX backend, loaded through the CUDA driver at run time.</li><li>Device memory, pinned host memory, asynchronous copies.</li></ul> | GPU fixtures pass on an NVIDIA machine (GPU runner or rented) |
| **0.4.1** | **ROCm:**<ul><li>AMDGPU code objects (LLVM backend, linked by LLD), loaded through HIP at run time.</li></ul> | GPU fixtures pass on an AMD machine |
| **0.4.2** | **Several GPUs:**<ul><li>Every device found is used. Large kernels are split across devices.</li><li>`MIMIC_GPU` chooses the devices.</li></ul> | A kernel split over two GPUs matches one GPU's result |

## 0.5 — Placement and streaming (the memory wall)

Each part of a program runs on the hardware that suits it, and data streams instead of waiting on memory.

| Version | Contents | Done when |
|---|---|---|
| **0.5.0** | **Residency:**<ul><li>Every array knows where its current copy lives (host or device).</li><li>Copies happen only when needed. A chain of GPU operations keeps its data on the device.</li></ul> | A chain of GPU maps copies its input once and its result once |
| **0.5.1** | **Cost model:** each kernel picks CPU, SIMD or GPU.<ul><li>**Inputs:** its size, the work per element, where the data is, and the speed of the link to the device (none on unified memory).</li><li>Calibrated once per machine and cached.</li></ul> | Each benchmark chooses at least as well as the best fixed choice |
| **0.5.2** | **Streaming:**<ul><li>Large arrays go through fused pipelines in cache-sized chunks, also across steps that cannot be fused, without whole-array intermediates.</li><li>Device copies are double-buffered, so they overlap with work.</li></ul> | Memory traffic of the streamed benchmarks measured below one pass per intermediate |
| **0.5.3** | **CPU and GPU together:**<ul><li>One kernel split between the CPU cores and the GPU by their measured speeds.</li><li>NUMA-aware placement on multi-socket machines.</li></ul> | Combined CPU + GPU runs faster than either alone on the mandelbrot and matmul benchmarks |
| **0.5.4** | **Beyond memory:**<ul><li>Arrays larger than RAM stream from files (memory mapping, io_uring on Linux), through the same pipelines.</li></ul> | A sum over a file larger than RAM runs at disk speed |
| **0.5.5** | **Portable fast binaries:**<ul><li>`--cpu portable` carries code for several CPU levels (x86-64 v2/v3/v4, Armv8/Armv9 with SVE) and picks the best at start.</li></ul> | One binary uses AVX-512 where present and still runs on an older CPU |

## 0.6 — Every operating system

A C library and the runtime for every OS LLVM targets, from any host.

| Version | Contents | Done when |
|---|---|---|
| **0.6.0** | **glibc Linux from any host:**<ul><li>Stubs per glibc version, e.g. `x86_64-linux-gnu.2.17` runs on old distributions.</li><li>Dynamic or static linking.</li></ul> | Programs for glibc 2.17 run on the newest and oldest supported distributions |
| **0.6.1** | **Android:**<ul><li>The NDK C library; executables and shared libraries for apps.</li><li>Tests in the Android emulator.</li></ul> | Golden fixtures pass in the emulator (arm64, x86_64) |
| **0.6.2** | **The BSDs:** FreeBSD, NetBSD, OpenBSD, DragonFly.<ul><li>C library stubs; the runtime (kqueue already exists).</li><li>Tests in virtual machines in CI.</li></ul> | Golden fixtures pass on FreeBSD; the rest link |
| **0.6.3** | **Apple from any host:** macOS and iOS programs built on Linux and Windows, through SDK stubs and code signing. | A Linux runner builds a macOS program that the Mac runner runs |
| **0.6.4** | **The other systems LLVM targets:** illumos/Solaris, Haiku, Fuchsia, AIX, z/OS, and the rest of LLVM's operating systems. | Each links a program; those with VMs or emulators run the fixtures |
| **0.6.5** | **No OS, and the web:**<ul><li>**UEFI and freestanding:** a runtime without threads or I/O, with static memory as an option.</li><li>**WebAssembly:** WASI preview 2 and WASI threads; the browser with JavaScript glue.</li></ul> | A UEFI program boots in an emulator; WASI and browser fixtures pass |

## 0.7 — Every CPU and SIMD extension

Programs run (not only compile) on every architecture LLVM supports, with SIMD for each vector extension.

| Version | Contents | Done when |
|---|---|---|
| **0.7.0** | **An emulated CI matrix (QEMU):** RISC-V 64, PowerPC 64 (LE and BE), s390x, LoongArch 64, MIPS and MIPS64, Arm 32-bit, x86 32-bit, SPARC 64. The full suite on each. | Golden fixtures pass on every architecture in the matrix |
| **0.7.1** | **Every vector extension:**<ul><li>**Scalable vectors:** SVE/SVE2, RISC-V V.</li><li>**Fixed-width:** AVX-512 masks, AltiVec/VSX, z/Architecture vector, LoongArch LSX/LASX, WebAssembly SIMD.</li></ul> | The SIMD fixture's machine code holds vector instructions on each, and the results match |
| **0.7.2** | **Run-time choice everywhere:** the best SIMD level chosen at start on every architecture family. | Portable binaries pick their level on x86, Arm, RISC-V and PowerPC |
| **0.7.3** | **Microcontrollers:**<ul><li>AVR, MSP430, Xtensa (ESP32), Arm Cortex-M, RISC-V 32.</li><li>A minimal runtime: no threads, static memory.</li></ul> | Programs link for each; Cortex-M and RISC-V 32 run in QEMU |
| **0.7.4** | **The remaining architectures:** Hexagon, VE, M68k, C-SKY, Lanai, XCore, ARC, SPARC 32, and BPF (eBPF programs). | Each compiles the fixtures; those QEMU emulates run them |

## 0.8 — Every GPU

Every GPU LLVM can generate code for, on every device class.

| Version | Contents | Done when |
|---|---|---|
| **0.8.0** | **OpenCL:** SPIR-V kernels through the OpenCL runtime, for Intel GPUs, older drivers and other accelerators. | GPU fixtures pass on a CPU OpenCL runtime (PoCL) in CI |
| **0.8.1** | **DirectX 12:** DXIL from LLVM's DirectX backend, on Windows. | GPU fixtures pass on Windows through a software DirectX device (WARP) |
| **0.8.2** | **Phones:** Metal on iOS, Vulkan on Android. | GPU fixtures pass in the iOS simulator and the Android emulator |
| **0.8.3** | **WebGPU in the browser:** kernels translated to WGSL by Mimic (LLVM has no WGSL backend). | GPU fixtures pass in a headless browser |
| **0.8.4** | **More work on GPUs:** matrix multiply on GPU matrix units, sorts, scans of records. | `matmul` and `sort` benchmarks run on each tier-1 GPU |

## 0.9 — Language and tools complete

Everything 1.0 promises, frozen.

| Version | Contents | Done when |
|---|---|---|
| **0.9.0** | **The rest of the language:** sized integers (`i8`, `i16`, `u16`, `u32`, `u64`), decoding unions, and the gaps listed in the roadmap. | Every feature in the language reference has fixtures |
| **0.9.1** | **Editors:**<ul><li>Inferred types per span (`--types`), and `--json` for every command.</li><li>A language server (LSP) and a VS Code extension.</li></ul> | Go-to-definition, hover types and diagnostics work in VS Code |
| **0.9.2** | **Packages:**<ul><li>The registry.</li><li>Versioned dependency resolution, lockfiles, workspaces.</li></ul> | A project depends on registry packages by version range |
| **0.9.3** | **Incremental compilation:** per module, with stable symbols. | Changing one module rebuilds only it and its dependents |
| **0.9.4** | **Standard library review:**<ul><li>Names and signatures settled; deprecated items removed.</li><li>The language and library references complete.</li></ul> | Every public std item documented with an example |
| **0.9.5** | **Freeze:** a language specification and a conformance suite. Only fixes after this. | The spec covers the whole language; the suite passes |

## 0.10 — Bootstrap

The compiler rewritten in Mimic.

| Version | Contents | Done when |
|---|---|---|
| **0.10.0** | **The front end in Mimic:** lexer, parser, checker, types. It runs beside the Rust one on every test. | Both front ends agree on every fixture |
| **0.10.1** | **Code generation in Mimic:** LLVM through Mimic's C interop, nets, fusion, SIMD and GPU kernels. | The Mimic code generator builds every fixture, with the same output |
| **0.10.2** | **The driver in Mimic:** projects, packages, the toolchain (Clang and LLD through the C interop).<ul><li>Stage 1 (built by the Rust compiler) builds stage 2. Stage 2 builds stage 3.</li></ul> | Stages 2 and 3 are identical bit for bit |
| **0.10.3** | **Parity:** the Mimic-built `mimic` passes the full suite on every tier-1 platform and compiles at least as fast as the Rust-built one. | CI green with the self-hosted compiler |
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
