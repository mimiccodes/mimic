# Targets and the embedded toolchain

`mimic` compiles C, links, and builds C libraries with tools **inside its own binary**:
* **Clang:** the C compiler. It builds the runtime and C sources, and is available as `mimic cc`.
* **LLD:** the linker. It is available as `mimic ld.lld`, `mimic ld64.lld`, `mimic lld-link` and `mimic wasm-ld`.
* **LLVM's archive writer:** it writes static libraries.

You do not need Zig, Xcode's command-line tools or a system C compiler for the targets marked **embedded**. This is roadmap Phase A. Targets that are not embedded yet fall back to `zig cc` if Zig is installed.

## Writing a target

```text
mimic build --target <arch>-<os>[-<abi>][.<version>]

mimic build --target x86_64-linux-musl        # static Linux executable
mimic build --target aarch64-linux            # ABI defaults to musl on Linux
mimic build --target x86_64-linux-gnu.2.17    # glibc 2.17 (Zig fallback for now)
mimic build --target aarch64-macos.13.0       # minimum macOS 13.0
mimic targets                                 # the full list, with status
mimic targets --json                          # the same, for tools
mimic targets --cpus riscv64                  # CPU models
mimic targets --features aarch64              # CPU features
```

The names are Zig's. Every architecture (61), OS (48) and ABI (30) that Zig 0.17 knows is accepted. An architecture without a code generator in this build is rejected with a clear message.

### Defaults when the ABI is omitted

| OS | Default ABI |
|---|---|
| Linux | `musl`, producing fully static executables with no system libc needed. 32-bit ARM, MIPS and PowerPC use `musleabihf`; MIPS64 uses `muslabi64`. |
| Windows | `gnu` (MinGW). |
| Everything else | `none`. |

### CPU and ABI

For each target, Mimic code is compiled with the same CPU model, features and ABI that Clang would choose, for example `generic-rv64` with `+m,+a,+f,+d,+c` and `lp64d` on riscv64. This keeps Mimic code, the runtime and the C library consistent; mixing floating-point ABIs is a link error.

Native builds (no `--target`) use the host CPU and all its features.

## Status

| Family | Status |
|---|---|
| macOS, native | **Embedded:** runtime compiled with the embedded Clang, linked with `ld64.lld`, with ad-hoc code signing on arm64. Still uses the macOS SDK from `xcrun`; bundled SDK stubs come next. |
| Linux, musl (30 of 33 targets) | **Embedded.** musl 1.2.5 and compiler-rt are built from sources bundled in `mimic`, the first time each target is used (about 40 s per target), then cached under `~/.mimic/cache/libc/`. Executables are fully static. Not yet: `riscv32` (needs quad-precision soft-float for `long double`) and `hexagon` (musl has no port); `m68k` has no code generator in this build. |
| Linux, glibc | Zig fallback. Next step: glibc stubs for 2.0–2.44. |
| Windows (MinGW) | Zig fallback. Next step: bundled mingw-w64. |
| WASI | Zig fallback. Next step: bundled wasi-libc. |
| FreeBSD, NetBSD, OpenBSD | Zig fallback. Next step: generated libc stubs. |
| No libc (`freestanding`, GPU targets) | Object files only (tier 3). |

`mimic targets` prints the status of every target with a libc:
* `embedded` means it is built only with the tools inside `mimic`;
* `zig fallback` means it needs Zig until that libc is bundled;
* `no code generator` means this LLVM build has no backend for the architecture.

## Architectures without a code generator

Zig *lists* 61 architectures, but generating code needs a backend:
* **Experimental in LLVM**, not in this build: `arc`, `csky`, `m68k`, `xtensa`. Building our own LLVM, in step A8, can enable them.
* **No LLVM backend at all:** `alpha`, `arceb`, `ez80`, `hppa`, `hppa64`, `kalimba`, `kvx`, `m88k`, `microblaze`, `microblazeel`, `or1k`, `propeller`, `sh`, `sheb`, `spork8`, `xtensaeb`.

## Building `mimic` itself

`tools/deps.sh` downloads and checks (sha256) the LLVM 23.1.4 sources and musl 1.2.5, then builds static LLD into `~/.mimic/deps`. The Rust build then compiles the Clang driver from those sources and links everything statically.

If those inputs are missing, `mimic` still builds, but without the embedded tools: `mimic targets` and the build fall back to Zig or the system compiler.
