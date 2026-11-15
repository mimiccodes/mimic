# Changelog

Every user-visible change, by version. The plan for the versions ahead is in [docs/versions.md](docs/versions.md).

## Unreleased

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
