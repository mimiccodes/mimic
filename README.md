![MIMIC: Everything that can run in parallel will run in parallel.](assets/images/github_Readme_banner.png)

Mimic is an array-based, massively parallel, memory-safe compiled language, named after the mimic octopus. Every construct has two interchangeable spellings: an APL/BQN/Uiua-style glyph and a modern functional word.

```mimic
module main

fn main() {
    let xs = ⍳ 10                       // iota 10
    print(xs ▷ +/)                      // 45
    print(iota 10 |> reverse |> take(3)) // [9 8 7]
    print(xs ¨ (x => x × x) ▷ +/)       // 285
}
```

```sh
mimic create hello && cd hello
mimic run                     # build (debug) and run
mimic build --release --target x86_64-linux-musl   # a static Linux binary, from any machine
```

## Install

macOS (Apple silicon), Linux (x86_64, glibc 2.28 or newer) and WSL:

```sh
curl -fsSL https://mimic.codes/install.sh | sh
```

Windows (PowerShell):

```powershell
irm https://mimic.codes/install.ps1 | iex
```

The same command updates an installed `mimic` to the latest release (or says it is already the latest); `mimic upgrade` does the same from Mimic itself. `mimic` goes into `~/.mimic/bin` and onto your `PATH`, without administrator rights; the download is checked against the release's `SHA256SUMS`. One binary holds the compiler, Clang, LLD and the C libraries for cross builds; nothing else is needed (on macOS, the Xcode Command Line Tools). Options: `sh -s -- --version 0.3.16`, `--check`, `--force`, `--uninstall` (on Windows, `$env:MIMIC_VERSION`, `MIMIC_FORCE`, `MIMIC_UNINSTALL`).

The Mimic extension for VS Code, Cursor, Windsurf and VSCodium (every one installed), installed or updated:

```sh
curl -fsSL https://mimic.codes/vscode.sh | sh        # macOS, Linux, WSL
irm https://mimic.codes/vscode.ps1 | iex             # Windows
```

The scripts are in [tools/get](tools/get) (also at `https://raw.githubusercontent.com/mimiccodes/mimic/main/tools/get/install.sh` and so on); or download a release from [Releases](https://github.com/mimiccodes/mimic/releases), unpack it and run its `install.sh` (`install.cmd` on Windows). The extension's `.vsix` is on [its releases](https://github.com/mimiccodes/mimic-vscode/releases); it finds `mimic`, or installs and updates it for you.

## Status

**Released: 0.3.16** ([CHANGELOG.md](CHANGELOG.md)). Stages 0.1, 0.2 and 0.3 are done. Stages 0.1 and 0.2 built the language, parallel by default (fused kernels, `¨` on all cores, fork-join recursion, interaction nets, SIMD, Metal GPU kernels), the standard library (files, processes, JSON, HTTP/HTTPS, TCP, UDP, TLS, WebSockets, tasks and channels, UIs in the browser and in native windows), C interop both ways, native builds on macOS, Linux and Windows, and cross builds to 30 Linux architectures (static, musl), Windows x86_64 and arm64, WebAssembly and every Apple platform ([docs/targets.md](docs/targets.md)).

**Stage 0.3, the Workbench**, added the tools around the language, for people, editors and AI agents:

| Command | What it does |
|---|---|
| `mimic man` | The manual inside `mimic`: the tutorial, the reference, guides, a page for every standard function, glyph and error code |
| `mimic lsp` | The language server: errors as you type with fixes, hover, definitions, references, rename, call hierarchy, signature help, glyph input, inferred types |
| `mimic edit` | Mimic's own terminal editor, with errors as you type, the manual, a REPL, run and test |
| `mimic repl` | Mimic a line at a time (`⍳ 10 ▷ +/` → `45`); `--json` for notebooks |
| `mimic why` | Where code runs and why: in parallel, fused, SIMD, on the GPU, as a task, in order |
| `mimic profile` | Where the time goes: per function next to where it runs, per core, CPU and GPU, memory |
| `mimic net show`, `trace` | The interaction nets of pure functions as graphs, and their reduction round by round |
| `mimic test` | Tests, property tests (`test … for x: T`), contracts (`requires`/`ensures`), examples in comments; `--coverage` |
| `mimic audit` | Quality and security findings, SARIF, a CI gate with a baseline |
| `mimic doc` | Documentation from the code and from guides written by people or agents |
| `mimic mcp` | Mimic's tools for AI agents (Model Context Protocol) |
| `mimic dap` | Debugging from editors, with values shown as Mimic prints them |

Editors: the [VS Code extension](https://github.com/mimiccodes/mimic-vscode) (everything above, plus profiles, net views, notebooks and coverage in the editor), and TextMate and tree-sitter grammars in [mimic-syntax](https://github.com/mimiccodes/mimic-syntax) for Neovim, Helix, Zed, Sublime Text and others (`mimic man editors`).

**Speed** ([BENCHMARK.md](BENCHMARK.md)): on one core Mimic is within a few percent of C on most benchmarks and faster on several; on all cores, with no annotations, it is faster than C on seven of nine benchmarks (nbody ties; records is 1.14× C) and faster than hand-parallelised Rust (Rayon) on six of eight. Where it is still behind is listed in [docs/gaps.md](docs/gaps.md).

The plan to 1.0 is in [docs/versions.md](docs/versions.md).

| Doc | Contents |
|---|---|
| [docs/tutorial](docs/tutorial/index.md) | the tutorial, chapter by chapter (also `mimic man 1`) |
| [docs/manual](docs/manual) | guides: coming from C, Python or JavaScript; writing Mimic with agents; editors; audit; profiling; MCP |
| [docs/syntax.md](docs/syntax.md) | grammar, precedence, glyph table |
| [docs/std.md](docs/std.md) | the standard library |
| [docs/cli.md](docs/cli.md) | every `mimic` command |
| [docs/manifest.md](docs/manifest.md) | `mim.mim`, `build.mim`, `mod.mim` |
| [docs/ffi.md](docs/ffi.md) | calling C, and C calling Mimic |
| [docs/targets.md](docs/targets.md) | what `mimic` builds for |
| [docs/design.md](docs/design.md), [docs/net.md](docs/net.md) | the runtime: the array layer and interaction nets |
| [docs/workbench.md](docs/workbench.md) | the analysis engine and `mimic query` |
| [BENCHMARK.md](BENCHMARK.md) | how speed is measured, and the numbers |
| [docs/versions.md](docs/versions.md), [docs/gaps.md](docs/gaps.md) | the versions to 1.0, and what is still missing |
| [CHANGELOG.md](CHANGELOG.md) | what changed in each version |

## Example project

[`examples/showcase`](examples/showcase) is a tour of every feature built so far: glyphs, arrays, parallel recursion, FFI, unions and `match`, records, tuples, routes, `let … else` and more. Its README has step-by-step instructions:

```sh
cd examples/showcase
make compiler   # builds mimic from this repository
make run        # runs the tour
```

## Building mimic

Requirements:
* Rust 1.88 or newer
* LLVM 23 (`brew install llvm`); `.cargo/config.toml` points `LLVM_SYS_231_PREFIX` at `/opt/homebrew/opt/llvm`, so edit that if yours lives elsewhere
* `tools/deps.sh` once: it fetches the LLVM sources and musl and builds the linker that `mimic` embeds

```sh
tools/deps.sh                  # once: sources for the embedded Clang, LLD and musl
cargo build --release          # → target/release/mimic
cargo test --release           # unit tests + golden tests (tests/fixtures)
```

The editors' parts are submodules: `editors/syntax` ([mimic-syntax](https://github.com/mimiccodes/mimic-syntax)) and `editors/vscode` ([mimic-vscode](https://github.com/mimiccodes/mimic-vscode)); clone with `--recursive`, or run `git submodule update --init`.

## Repository layout

```
crates/mimic-cli       the `mimic` binary: commands, build driver, the engine, lsp, dap, mcp, edit, repl, profile
crates/mimic-syntax    lexer, parser, AST, glyph table, config format
crates/mimic-check     module tree, name resolution, visibility, mutability, FFI rules
crates/mimic-codegen   LLVM codegen (inkwell)
crates/mimic-project   mim.mim / build.mim, scaffolding, ~/.mimic/libraries
crates/mimic-net       interaction-net IR, .mnet parser, encoder to the runtime book, a reference reducer (trace)
runtime/               C runtime: array layer + interaction-net reducer (embedded, compiled per target by the embedded Clang)
crates/mimic-toolchain embedded Clang + LLD, bundled musl / compiler-rt, C library builds
tools/deps.sh          fetches and builds the native dependencies
tests/fixtures         golden programs (run/*.mim + .out) and expected failures (fail/*.mim)
tests/nets             interaction-net programs (*.mnet + .out)
bench/                 benchmarks in Mimic, C, Rust (and Rayon) and Go
docs/                  the reference, the tutorial and the guides (embedded in `mimic man`)
editors/               mimic-syntax and mimic-vscode (submodules)
```

## License

Mimic is licensed under the [Apache License, Version 2.0](LICENSE). Copyright 2026 Harcorp Industries.
