# Mimic

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
mimic build --release --target x86_64-linux-gnu
```

## Status

Milestone 1 is a working vertical slice:
* the CLI and project system, including `mod.mim` and installing packages from git;
* the parser, with glyphs and words as equivalent spellings;
* a checker for modules, privacy, immutability and FFI `unsafe`;
* **LLVM** code generation and a refcounted array runtime that the tests show to be leak-free;
* calls into C;
* cross-compilation to every architecture LLVM supports, with Clang and LLD built into `mimic` (see [docs/targets.md](docs/targets.md)).

Milestone 2 is in progress:
* **Phase 1, done:** the array layer runs on all cores automatically.
* **Phase 2, done:** the interaction-net parallel reducer.
* **Phase 3, done:** pure Mimic functions compile to nets.

M3 is in progress. Static types and unboxed scalars are done: `fib(38)` runs at 1.42× of C on one core and **5.4× faster than C on 12 cores**, with no annotations beyond `n: i64`.

The roadmap is in [docs/design.md](docs/design.md).

| Doc | Contents |
|---|---|
| [docs/design.md](docs/design.md) | runtime model: interaction nets plus the array layer; roadmap |
| [docs/syntax.md](docs/syntax.md) | grammar, precedence, glyph table |
| [docs/manifest.md](docs/manifest.md) | `mim.mim`, `build.mim`, `mod.mim` |
| [docs/ffi.md](docs/ffi.md) | C FFI |
| [docs/cli.md](docs/cli.md) | every `mimic` command |
| [docs/net.md](docs/net.md) | the interaction-net runtime and `.mnet` format |
| [BENCHMARK.md](BENCHMARK.md) | how speed is measured, current numbers, targets |
| [docs/roadmap.md](docs/roadmap.md) | what's done, what's next, and the syntax plan for loops, UI and networking |

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
cargo test                     # unit tests + golden tests (tests/fixtures)
```

## Repository layout

```
crates/mimic-cli       the `mimic` binary: commands, build driver, mod.mim
crates/mimic-syntax    lexer, parser, AST, glyph table, config format
crates/mimic-check     module tree, name resolution, visibility, mutability, FFI rules
crates/mimic-codegen   LLVM codegen (inkwell)
crates/mimic-project   mim.mim / build.mim, scaffolding, ~/.mimic/libraries
crates/mimic-net       interaction-net IR, .mnet parser, encoder to the runtime book
runtime/               C runtime: array layer + interaction-net reducer (embedded, compiled per target by the embedded Clang)
crates/mimic-toolchain embedded Clang + LLD, bundled musl / compiler-rt, C library builds
tools/deps.sh          fetches and builds the native dependencies
tests/fixtures         golden programs (run/*.mim + .out) and expected failures (fail/*.mim)
tests/nets             interaction-net programs (*.mnet + .out)
```

## License

Mimic is licensed under the [Apache License, Version 2.0](LICENSE). Copyright 2026 Harcorp Industries.
