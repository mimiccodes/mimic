# Changelog

Every user-visible change, by version. The plan for the versions ahead is in [docs/versions.md](docs/versions.md).

## Unreleased

Next: 0.1.1 (release builds); work on 0.2.0 (Windows) has started.

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
