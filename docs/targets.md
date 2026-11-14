# Targets and the embedded toolchain

**Mimic can target everything the LLVM inside it can generate code for.** The lists of architectures, operating systems, environments and code generators are not hand-written: `mimic targets` reads them from LLVM itself.

`mimic` carries its own tools inside its binary:
* **Clang:** the C compiler. It builds the runtime and C libraries, and is available as `mimic cc`.
* **LLD:** the linker. It is available as `mimic ld.lld`, `mimic ld64.lld`, `mimic lld-link` and `mimic wasm-ld`.
* **LLVM's archive writer:** it writes static libraries.
* **musl and compiler-rt:** these are carried as sources. A target's C library is built the first time it is used.

No other compiler, linker or SDK has to be installed for the targets that produce **complete executables** (see below).

## Compiling C: `mimic cc`

`mimic cc` (and `mimic c++`) is Clang, with everything a target needs filled in. These are its flags as usual, plus:

* `--target` is understood like `mimic build` understands it, so `aarch64-linux` means musl.
* For targets with a bundled C library, the C library, start-up files and compiler-rt are added automatically. The C library is built from the bundled sources the first time it is needed.
* Clang's own headers come from inside `mimic`, and linking uses the embedded LLD.

```sh
mimic cc -O2 hello.c -o hello                          # native
mimic cc --target=aarch64-linux hello.c -lm -o hello   # static Linux arm64 executable
mimic cc --target=x86_64-macos hello.c -o hello        # Intel macOS, from Apple silicon
mimic cc --target=wasm32-wasi hello.c -o hello.wasm    # WebAssembly + WASI
mimic cc --target=x86_64-windows -ffreestanding -c f.c # objects for any LLVM target
```

C++ compiles too, but libc++ is not bundled yet, so the C++ standard library is not available.

## Writing a target

A target is an LLVM triple. The vendor may be left out.

```text
mimic build --target x86_64-linux-musl           # static Linux executable
mimic build --target aarch64-linux               # environment defaults to musl on Linux
mimic build --target riscv64-unknown-linux-musl  # full triple
mimic build --target x86_64-apple-macosx13.0     # minimum macOS 13.0
mimic build --target aarch64-ios-simulator       # iPhone simulator (Xcode's SDK)
mimic build --target nvptx64-nvidia-cuda         # GPU: writes PTX
mimic build --target avr-unknown-unknown         # microcontroller: object file

mimic targets                       # architectures, OSes, environments, code generators
mimic targets --check aarch64-linux # how a target is understood, what it can build
mimic targets --cpus riscv64        # CPU models
mimic targets --features aarch64    # CPU features
mimic targets --json                # everything, for tools
```

### Defaults

| Situation | Default |
|---|---|
| Linux, no environment given | `musl`, producing fully static executables. 32-bit ARM, MIPS and PowerPC use `musleabihf`; MIPS64 uses `muslabi64`. |
| Windows, no environment given | `gnu` (MinGW). |
| 32-bit ARM and Thumb | ARMv7-A is the baseline CPU. |
| Apple OSes without a version | macOS 11.0, iOS and tvOS 14.0, watchOS 7.0, visionOS 1.0. Add `-simulator` for a simulator build, `-macabi` for Mac Catalyst. |
| glibc version | Written after the environment, e.g. `x86_64-linux-gnu.2.17`. |

### CPU and ABI

For each target, Mimic code is compiled with the CPU model, features and ABI that Clang would choose. For example, riscv64 uses `generic-rv64` with `+m,+a,+f,+d,+c` and the `lp64d` ABI. This keeps Mimic code, the runtime and the C library consistent; mixing floating-point ABIs is a link error.

Native builds (no `--target`) use the host CPU with all its features.

`--cpu` picks a CPU model instead, for both Mimic code and the runtime. Its features follow from the model.

```sh
mimic build --cpu apple-m3                              # Apple M3 (an alias LLVM accepts)
mimic build --target x86_64-linux --cpu x86-64-v3       # AVX2-class x86-64
mimic build --target riscv64-linux --cpu sifive-u74
mimic targets --cpus aarch64                            # the models LLVM knows
```

A name LLVM does not accept is an error, with a suggestion for the closest name. `--cpu native` means "this machine" and applies only to native builds.

`--cpu portable` builds for the oldest model of the target's CPU family, so the binary runs on every machine of that family: `apple-m1` for Apple silicon (M1 to M5), `x86-64-v2` for x86-64. Native builds default to `native`, the fastest binary for the machine that builds it; use `portable` for binaries you distribute.

## Tiers

`mimic targets` groups targets by how well they are proven, and `mimic targets --check <target>` shows a target's tier.

| Tier | Meaning | Targets today |
|---|---|---|
| **stable** | Programs run and pass every golden test (on the development machine or in CI) | Native, `aarch64-macos`, `x86_64-macos` (under Rosetta), `aarch64-ios-macabi` (Mac Catalyst), `wasm32-wasi` (Node), `x86_64-linux-musl` and `aarch64-linux-musl` (static Linux), `x86_64-windows` |
| **runs** | A program ran on the target, but not the full test suite yet | `aarch64-ios-simulator` (iPhone simulator) |
| **links** | Complete executables are built and checked (format, architecture, platform), but not run | The other 28 Linux musl targets; `aarch64-windows`; iOS, tvOS, watchOS and visionOS devices and the other simulators |
| **compiles** | Object files or assembly only: no C library is bundled yet | The other CPU architectures |
| **not yet** | GPU architecture: needs the GPU backend (roadmap F8) | amdgpu, r600, spirv, spirv32, spirv64 |

A target moves up a tier when tests run on it. CI runs every golden program on Linux x86_64 (native and static), on Arm Linux (static, cross-built), and on Windows x86_64 (built there by `mimic` itself, and cross-built on Linux).

## What each target produces

LLVM's triple parser knows 62 architectures, 49 operating systems and 53 environments. This build has code generators for 44 of those architectures.

**All 39 CPU architectures compile Mimic programs, and a test checks this for each one:** arm, armeb, aarch64, aarch64_be, aarch64_32, avr, bpfel, bpfeb, hexagon, loongarch32, loongarch64, mips, mipsel, mips64, mips64el, msp430, powerpc, powerpcle, powerpc64, powerpc64le, riscv32, riscv64, riscv32be, riscv64be, sparc, sparcv9, sparcel, s390x, thumb, thumbeb, i386, x86_64, xcore, nvptx, nvptx64, lanai, wasm32, wasm64, ve.

The 5 GPU architectures (amdgpu, r600, spirv, spirv32, spirv64) take compute kernels, not whole programs: a program's runtime, threads and function pointers cannot run on them. Compiling Mimic's array kernels for GPUs is the GPU backend on the roadmap (F8). Until then, building for them says so. NVPTX already gets a whole program as PTX assembly.

| Output | Targets |
|---|---|
| **Complete executable** | Every Apple platform whose SDK is installed with Xcode: macOS (arm64, x86_64), Mac Catalyst, iOS (device and simulator), tvOS, watchOS (including `arm64_32`), visionOS, each with and without the simulator. Binaries are ad-hoc signed. Checks so far: Mac Catalyst builds run here with every golden test matching, and an iOS-simulator build ran in the iPhone simulator; the other platforms are checked for the right platform tag in the binary.<br>`wasm32-wasi` (WASI preview 1): a `.wasm` command module, using wasi-libc from wasi-sdk 34 and compiler-rt. It runs in any WASI runtime (Node, wasmtime, browsers via a shim). All golden tests produce the same output as native builds. It is single-threaded for now; `wasm32-wasip1-threads` comes later. Deep recursion hands over to the heap-based net reducer after 256 native frames instead of 2048, because engines give WebAssembly one small stack. Known issue: Node 22 itself occasionally (about 1–3% of runs, more under load) crashes with SIGSEGV on the 100,000-deep recursion test. A WebAssembly program cannot cause that by itself; when Node does not crash, the output is always correct.<br>30 Linux musl targets, using musl 1.2.5 and compiler-rt built from the bundled sources, the first time each target is used (about 40 s), then cached in `~/.mimic/cache/libc/`. Executables are fully static.<br>Windows on x86_64 and arm64 (`x86_64-windows`, `aarch64-windows`): `.exe` files using mingw-w64 with the UCRT (from llvm-mingw), linked by LLD's MinGW driver. Everything but Windows' own DLLs is linked in, threads included. The golden programs are built on Linux and run on a Windows machine in CI. Networking uses Winsock, and TLS uses Schannel with Windows' trust store; `os::shell` runs `cmd.exe /c` there. |
| **Object file** | Any other target on a CPU architecture: glibc Linux, Android, the BSDs, UEFI, bare metal (`riscv32-unknown-elf`), AVR, MSP430, BPF, Hexagon, … |
| **Assembly** | Backends that do not write object files: PTX for `nvptx`/`nvptx64`, `.s` for `xcore`. |

These are the 30 Linux musl targets:

| Architecture | Targets |
|---|---|
| x86 | x86_64, x86_64 x32, x86 |
| AArch64 | aarch64, aarch64_be |
| 32-bit ARM | arm, armeb, thumb and thumbeb, each in `eabi` and `eabihf` |
| LoongArch | loongarch64 (`musl`, `muslf32`, `muslsf`) |
| MIPS, 32-bit | mips, mipsel (`eabi`, `eabihf`) |
| MIPS, 64-bit | mips64, mips64el (`abi64`, `abin32`) |
| Others | powerpc (`eabi`, `eabihf`), powerpc64, powerpc64le, riscv64, s390x |

`mimic targets --check <target>` says which of these three outputs a target gets, and why.

### Next C libraries

These will turn object-only targets into complete executables:
* glibc stubs for linking against any glibc version;
* the BSDs' and Android's libcs;
* Apple SDK stubs, so that macOS no longer needs the SDK installed.

Two musl targets are not available yet:
* `riscv32` needs quad-precision soft-float for musl's 128-bit `long double`;
* `hexagon` has no musl port.

## Architectures LLVM knows but this build cannot generate

LLVM's triple parser knows more architectures than this build has backends for, for example the experimental `m68k`, `csky`, `arc`, `xtensa` and `directx` backends. `mimic targets` lists these separately. They become available when `mimic` is linked against an LLVM built with them; that is roadmap step A8.

## Building `mimic` itself

`tools/deps.sh` downloads the LLVM 23.1.4 sources, musl 1.2.5 and the wasi-sdk 34 sysroot (trimmed to its wasm32-wasip1 C library, 8.9 MB), checks their sha256 against the published values, and builds static LLD into `~/.mimic/deps`. The Rust build then compiles the Clang driver from those sources and links everything statically.

If those inputs are missing, `mimic` still builds without the embedded tools. In that case native builds use the system C compiler, and cross targets produce object files only.

**Which LLVM.** Cargo links against the LLVM that the git-ignored `.llvm` link in the repository root points at. `tools/deps.sh` points it at Homebrew's LLVM 23.1.4 if nothing is linked yet. `tools/build-llvm.sh` builds a static LLVM with Clang and LLD into `~/.mimic/deps/llvm-23.1.4`, using all backends including the experimental ones, and then points `.llvm` at it. This takes about 3 hours on 4 cores; `JOBS` and `NICE` control the load. With that build, the release `mimic` binary depends on system libraries only:

```sh
otool -L target/release/mimic   # libSystem, libc++, libffi, libiconv, libxcselect: all in /usr/lib
```

**Backends that cannot compile Mimic code yet:**
* **ARC** (experimental): LLVM's instruction selection fails on Mimic's code. `mimic targets --check arc-unknown-unknown` reports this.
* **eBPF** defaults to CPU `v4` (Linux 6.6 and later), the first generation with the signed division that `div` and `%` need.
* **Xtensa** is compiled without position-independent code, because it has no PIC model.
