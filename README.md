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
* cross-compilation to any target Zig supports.

The interaction-net parallel runtime is Milestone 2. The roadmap is in [docs/design.md](docs/design.md).

| Doc | Contents |
|---|---|
| [docs/design.md](docs/design.md) | runtime model: interaction nets plus the array layer; roadmap |
| [docs/syntax.md](docs/syntax.md) | grammar, precedence, glyph table |
| [docs/manifest.md](docs/manifest.md) | `mim.mim`, `build.mim`, `mod.mim` |
| [docs/ffi.md](docs/ffi.md) | C FFI |
| [docs/cli.md](docs/cli.md) | every `mimic` command |

## Building mimic

Requirements:
* Rust 1.88 or newer
* LLVM 23 (`brew install llvm`); `.cargo/config.toml` points `LLVM_SYS_231_PREFIX` at `/opt/homebrew/opt/llvm`, so edit that if yours lives elsewhere
* Zig, tested with 0.17 (`brew install zig`)

```sh
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
runtime/               C array runtime (embedded into mimic, compiled per target by zig cc)
tests/fixtures         golden programs (run/*.mim + .out) and expected failures (fail/*.mim)
```
