<p align="center">
  <a href="https://mimic.codes"><img src="assets/images/github_Readme_banner.png" alt="MIMIC: Everything that can run in parallel will run in parallel."></a>
</p>

<p align="center">
  <a href="https://github.com/mimiccodes/mimic/releases/latest"><img alt="Latest release" src="https://img.shields.io/github/v/release/mimiccodes/mimic?style=flat-square&labelColor=0A0A0A&color=E24B04&label=release"></a>
  <a href="https://github.com/mimiccodes/mimic/actions/workflows/ci.yml"><img alt="CI" src="https://img.shields.io/github/actions/workflow/status/mimiccodes/mimic/ci.yml?branch=main&style=flat-square&labelColor=0A0A0A&label=CI"></a>
  <a href="LICENSE"><img alt="License: Apache-2.0" src="https://img.shields.io/badge/license-Apache--2.0-E24B04?style=flat-square&labelColor=0A0A0A"></a>
  <a href="https://mimic.codes/install"><img alt="Runs on macOS, Linux and Windows" src="https://img.shields.io/badge/runs%20on-macOS%20%7C%20Linux%20%7C%20Windows-E24B04?style=flat-square&labelColor=0A0A0A"></a>
  <a href="https://github.com/mimiccodes/mimic-vscode/releases/latest"><img alt="VS Code extension" src="https://img.shields.io/github/v/release/mimiccodes/mimic-vscode?style=flat-square&labelColor=0A0A0A&color=E24B04&label=VS%20Code%20extension"></a>
</p>

<p align="center">
  <a href="https://mimic.codes"><img alt="Website: mimic.codes" src="https://img.shields.io/badge/website-mimic.codes-E24B04?style=flat-square&labelColor=0A0A0A"></a>
  <a href="https://discord.gg/gSyJjddnTm"><img alt="Discord" src="https://img.shields.io/badge/Discord-join-E24B04?style=flat-square&labelColor=0A0A0A&logo=discord&logoColor=white"></a>
  <a href="https://x.com/mimiccodes"><img alt="X: @mimiccodes" src="https://img.shields.io/badge/X-@mimiccodes-E24B04?style=flat-square&labelColor=0A0A0A&logo=x&logoColor=white"></a>
  <a href="https://www.youtube.com/@mimiccodes"><img alt="YouTube: @mimiccodes" src="https://img.shields.io/badge/YouTube-@mimiccodes-E24B04?style=flat-square&labelColor=0A0A0A&logo=youtube&logoColor=white"></a>
  <a href="https://www.linkedin.com/showcase/mimiccodes"><img alt="LinkedIn" src="https://img.shields.io/badge/LinkedIn-Mimic-E24B04?style=flat-square&labelColor=0A0A0A&logo=data%3Aimage%2Fsvg%2Bxml%3Bbase64%2CPHN2ZyB4bWxucz0iaHR0cDovL3d3dy53My5vcmcvMjAwMC9zdmciIHZpZXdCb3g9IjAgMCAyNCAyNCI%2BPHBhdGggZmlsbD0id2hpdGUiIGZpbGwtcnVsZT0iZXZlbm9kZCIgZD0iTTMgMGgxOGEzIDMgMCAwIDEgMyAzdjE4YTMgMyAwIDAgMS0zIDNIM2EzIDMgMCAwIDEtMy0zVjNhMyAzIDAgMCAxIDMtM3pNNi42IDQuOWExLjc1IDEuNzUgMCAxIDAgMCAzLjUgMS43NSAxLjc1IDAgMCAwIDAtMy41ek01LjEgOS42VjE5LjVoM1Y5LjZ6TTEwLjEgOS42VjE5LjVoM3YtNWMwLTEuNC42LTIuNCAxLjgtMi40IDEuMSAwIDEuNi44IDEuNiAyLjN2NS4xaDN2LTUuN2MwLTMtMS41LTQuMi0zLjYtNC4yLTEuNCAwLTIuNC42LTIuOSAxLjVWOS42eiIvPjwvc3ZnPg%3D%3D"></a>
</p>

<h3 align="center">Everything that can run in parallel <em>will</em> run in parallel.</h3>

Mimic is an array-based, memory-safe, compiled language. Write what you want computed, over whole arrays; Mimic turns it into one fast pass over your data and runs it on every core, with SIMD and the GPU where it helps. No annotations.

## Why Mimic

* **Arrays first.** Whole-array operations replace loops: `prices × 1.2` updates every price at once. Every glyph has a word (`⍳` is `iota`, `¨` is `each`), and you can mix them.
* **Parallel by default.** No threads to manage and nothing to mark. Your steps become one fast pass over the data, spread over every core, with SIMD and the GPU where it helps.
* **Memory safe.** Values are immutable unless marked `mut`. Reference counts and updates in place: no garbage collector, no use after free.
* **Compiled, everywhere.** LLVM to native code. One `mimic` binary holds the compiler, Clang and a linker, and builds for macOS, Linux and Windows from any of them.

## A quick look

The same program, written with words and with glyphs. Both are Mimic, and they print the same thing:

```mimic
module main

fn main() {
    let prices = [12.5 8 30 4.25]
    print(prices * 1.2)                  // every price with tax
    print(prices |> sum)                 // the total

    let xs = iota 10                     // 0 1 2 … 9
    print(xs keep (xs % 2 == 0))         // the even ones
    print(xs each (x => x * x) |> sum)   // the sum of the squares
    print(xs |> reverse |> take(3))      // the last three, reversed
}
```

```mimic
module main

fn main() {
    let prices = [12.5 8 30 4.25]
    ⎕(prices × 1.2)                      // every price with tax
    ⎕(prices ▷ +/)                       // the total

    let xs = ⍳ 10                        // 0 1 2 … 9
    ⎕(xs ⌿ (xs % 2 == 0))                // the even ones
    ⎕(xs ¨ (x ⇒ x × x) ▷ +/)             // the sum of the squares
    ⎕(xs ▷ ⌽ ▷ ↑(3))                     // the last three, reversed
}
```

```text
[15.0 9.6 36.0 5.1]
54.75
[0 2 4 6 8]
285
[9 8 7]
```

`mimic fmt --glyphs` and `mimic fmt` turn one into the other. To type glyphs, use digraphs (`\i` is `⍳`) or let your editor complete them. `mimic why` shows how each line runs: fused into one pass, on all cores, with SIMD, on the GPU, or why not.

## Install

| | |
|---|---|
| **macOS, Linux, WSL** | `curl -fsSL https://mimic.codes/install.sh \| sh` |
| **Windows** (PowerShell) | `irm https://mimic.codes/install.ps1 \| iex` |
| **Editor extension** (VS Code, Cursor, Windsurf, VSCodium) | `curl -fsSL https://mimic.codes/vscode.sh \| sh`, or on Windows `irm https://mimic.codes/vscode.ps1 \| iex` |

`mimic` goes into `~/.mimic/bin`, with no administrator rights, and is checked against the release's checksums. It is one binary with the compiler, Clang, a linker and the C libraries inside; nothing else is needed (on macOS, the Xcode Command Line Tools). **To update,** run `mimic upgrade`, or the install command again. Options, uninstalling and troubleshooting: [mimic.codes/install](https://mimic.codes/install).

## Your first program

```sh
mimic create hello && cd hello
mimic run                                          # build and run src/main.mim
mimic build --release --target x86_64-linux-musl   # a static Linux binary, from any machine
```

Then follow the [tutorial](https://mimic.codes/tutorial), or read [Mimic for Python](https://mimic.codes/docs/coming-from-python), [JavaScript](https://mimic.codes/docs/coming-from-javascript) or [C](https://mimic.codes/docs/coming-from-c) programmers.

## The toolbox

Everything is in the one `mimic` binary:

| Command | What it does |
|---|---|
| `mimic run`, `mimic build` | Build and run; cross-build for any target (`--target`) |
| `mimic test` | Tests, property tests, contracts, examples in comments; `--coverage` |
| `mimic why` | Where and how each line runs: fused, in parallel, SIMD, GPU, or why not |
| `mimic profile` | Where the time goes: per function, per core, CPU and GPU, memory |
| `mimic repl` | Mimic a line at a time: `⍳ 10 ▷ +/` → `45` |
| `mimic edit` | Mimic's own terminal editor, with errors as you type, the manual and a REPL |
| `mimic man` | The manual: tutorial, reference, every function, glyph and error code |
| `mimic audit` | Quality and security findings, SARIF, a CI gate |
| `mimic upgrade` | Update Mimic (`--editors` updates the extension too) |

Also `fmt`, `doc`, `lsp`, `dap` and `mcp` (tools for AI agents). Every command: [docs/cli.md](docs/cli.md). Editors: the [VS Code extension](https://github.com/mimiccodes/mimic-vscode), and grammars for Neovim, Helix, Zed, Sublime Text and others in [mimic-syntax](https://github.com/mimiccodes/mimic-syntax).

## Speed, honestly

Every benchmark exists in Mimic, C, Rust and Go with the same algorithm, and must print the same result. Time relative to C on Linux x86-64 (GitHub's CI machines, 2026-11-17). **Lower is faster; below 1.00× beats C.**

| Benchmark | Mimic, one core | Mimic, all cores | Rust + Rayon, all cores |
|---|---|---|---|
| matmul | 0.38× | **0.22×** | 2.04× |
| sort | 0.24× | **0.15×** | 0.16× |
| mandelbrot | 0.77× | 0.32× | **0.31×** |
| tree | 0.65× | **0.32×** | 0.84× |
| fib | 0.89× | **0.41×** | 0.43× |
| filters | 1.30× | **0.61×** | 0.67× |
| records | 3.15× | **1.85×** | 2.02× |
| paths | 2.70× | 2.70× | **0.66×** |
| qoi | 14.65× | 8.70× | **0.91×** |

Mimic's parallel numbers need no annotations; Rayon's are parallelised by hand. Where Mimic is behind (`qoi`, `paths`, `records`) is the work of the next stage. All results, macOS and Windows too: [mimic.codes/benchmarks](https://mimic.codes/benchmarks) and [BENCHMARK.md](BENCHMARK.md).

## Status and roadmap

**Latest release: 0.3.17** ([changelog](CHANGELOG.md)).

| Era | Versions | |
|---|---|---|
| **Ideation** | 0.1 – 0.3 | ✓ Done: the language, the standard library, Windows, and the Workbench (editors, profiler, manual, tools for AI agents) |
| **Core** | 0.4 | ← Now: fast by default on every kind of program, and a richer vocabulary of glyphs |
| **Acceleration** | 0.5 – 0.6 | One GPU layer for every vendor: Vulkan, CUDA, ROCm |
| **Domains** | 0.7 | Graphics, quantum and protocols |
| **Everywhere** | 0.8 – 0.11 | Every operating system, CPU and GPU |
| **Maturity** | 0.12 – 0.13 | The language complete, and a compiler written in Mimic |
| **Stable** | 1.0 | The compatibility promise starts |

Every step and what it must pass: [mimic.codes/roadmap](https://mimic.codes/roadmap) ([docs/versions.md](docs/versions.md)). What is still missing: [docs/gaps.md](docs/gaps.md).

## Documentation

| | Website | In this repository |
|---|---|---|
| Tutorial, 20 chapters | [mimic.codes/tutorial](https://mimic.codes/tutorial) | [docs/tutorial](docs/tutorial/index.md), or `mimic man 1` |
| Guides: coming from C, Python, JavaScript; editors; profiling; agents | [mimic.codes/docs](https://mimic.codes/docs) | [docs/manual](docs/manual) |
| Syntax and every glyph | [mimic.codes/docs/syntax](https://mimic.codes/docs/syntax) | [docs/syntax.md](docs/syntax.md) |
| Standard library | [mimic.codes/docs/std](https://mimic.codes/docs/std) | [docs/std.md](docs/std.md) |
| Calling C, and C calling Mimic | [mimic.codes/docs/ffi](https://mimic.codes/docs/ffi) | [docs/ffi.md](docs/ffi.md) |
| How it works inside | [mimic.codes/docs/design](https://mimic.codes/docs/design) | [docs/design.md](docs/design.md), [docs/net.md](docs/net.md) |

A larger program to read: [examples/showcase](examples/showcase), a tour of the language.

## Community

Questions, ideas and bugs are welcome: [Discord](https://discord.gg/gSyJjddnTm) · [X](https://x.com/mimiccodes) · [LinkedIn](https://www.linkedin.com/showcase/mimiccodes) · [YouTube](https://www.youtube.com/@mimiccodes) · [issues](https://github.com/mimiccodes/mimic/issues) · [mimic@mimic.codes](mailto:mimic@mimic.codes)

To build Mimic from source or contribute, see [CONTRIBUTING.md](CONTRIBUTING.md). To report a security problem privately, see [SECURITY.md](SECURITY.md).

## License

Mimic is licensed under the [Apache License, Version 2.0](LICENSE). Copyright 2026 Harcorp Industries.
