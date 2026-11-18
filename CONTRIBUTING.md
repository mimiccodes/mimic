# Contributing to Mimic

Thank you for helping. Questions are welcome on [Discord](https://discord.gg/gSyJjddnTm) or in an [issue](https://github.com/mimiccodes/mimic/issues); bugs and slow programs especially. A program that Mimic runs slower than C is a bug too.

## Building mimic

You need:

* **Rust** 1.88 or newer
* **LLVM 23** (`brew install llvm` on macOS). `.cargo/config.toml` points `LLVM_SYS_231_PREFIX` at `/opt/homebrew/opt/llvm`; change it if yours lives elsewhere.
* the editors' parts, which are submodules: clone with `--recursive`, or run `git submodule update --init`

```sh
tools/deps.sh                  # once: the sources of the embedded Clang, LLD and musl
cargo build --release          # → target/release/mimic
cargo test --release           # unit tests, golden programs, the tutorial's examples
cargo clippy --workspace --all-targets -- -D warnings   # lints; CI fails on any warning
tools/gaps.sh                  # the known gaps (docs/gaps.md), each checked against the compiler
```

## Making a change

Work happens on `dev`; `main` holds releases only (see [docs/versions.md](docs/versions.md#how-a-version-is-made)).

1. Branch from `dev`: `feat/0.4.5-grade` for a step of the plan, `fix/…` for a bug, `docs/…` for writing.
2. Open a pull request into `dev`. CI builds and tests it on Linux, macOS and Windows; it is merged, squashed into one commit, once CI passes.
3. Commit messages name the part and the change (`upgrade: remember the channel`), with the reason in the body.

Every change:

* **Tests.** A new feature or fix comes with a golden program in `tests/fixtures/run` (`name.mim` and the expected output in `name.out`), or an expected failure in `tests/fixtures/fail`.
* **Docs in the same change.** Anything a user can see goes under **Unreleased** in [CHANGELOG.md](CHANGELOG.md), and into the tutorial (`docs/tutorial`) and the reference (`docs/*.md`) where it belongs. The examples in the tutorial are run by the tests, so they stay true.
* **Speed.** Changes to code generation or the runtime run the benchmarks (`python3 bench/run.py`, see [BENCHMARK.md](BENCHMARK.md)). Numbers in the docs are measured, never estimated.
* **New library features** go in a module (`fs::`, `http::`, …), not in the global namespace.
* **Parallel by default.** Mimic has no annotations for parallelism; the compiler decides. A change that needs one is a design question: open an issue first.
* **Reported problems stay fixed.** A bug a user reports becomes a check in `tools/gaps.sh` ([docs/gaps.md](docs/gaps.md)) the day it is reported, and a golden program once fixed.

Versions, branches, channels and what each release must pass are in [docs/versions.md](docs/versions.md).

**Machines on AWS.** Precise and heavy work (benchmarks on dedicated cores, arm64, later GPUs) can run on an AWS machine started for one job and gone after it: `.github/workflows/aws-runner.yml` (and `aws-smoke.yml` to try it). It reaches AWS without keys (GitHub's OIDC), only from `main` and `dev`, never for pull requests, and may only start and end machines tagged `Project=mimic-runner`; each machine runs one job, powers off after at most two hours, and lets nothing in; `aws-sweeper.yml` ends any such machine older than three hours, every 30 minutes. Every run's results go to the `bench` branch (`bench/record.py`: one file per run, a history per machine, a summary per release), and every machine's cost to a private ledger (`tools/ledger.py`; `ledger-billing.yml` adds what AWS billed, daily).

## Repository layout

```
crates/mimic-cli       the `mimic` binary: commands, build driver, the engine, lsp, dap, mcp, edit, repl, profile, upgrade
crates/mimic-syntax    lexer, parser, AST, glyph table, config format
crates/mimic-check     module tree, name resolution, visibility, mutability, FFI rules
crates/mimic-codegen   LLVM code generation (inkwell): fused kernels, SIMD, GPU, fork-join, ownership
crates/mimic-project   mim.mim / build.mim, scaffolding, ~/.mimic/libraries
crates/mimic-net       interaction-net IR, .mnet parser, encoder to the runtime book, a reference reducer (trace)
crates/mimic-toolchain embedded Clang + LLD, bundled musl / compiler-rt, C library builds
runtime/               C runtime: the array layer and the interaction-net reducer (compiled per target by the embedded Clang)
tools/deps.sh          fetches and builds the native dependencies
tools/get/             the install scripts behind mimic.codes/install.sh, install.ps1, vscode.sh, vscode.ps1
tests/fixtures         golden programs (run/*.mim + .out) and expected failures (fail/*.mim)
tests/nets             interaction-net programs (*.mnet + .out)
bench/                 benchmarks in Mimic, C and Rust (and Rayon)
examples/              larger programs: a tour of the language, video transcoding
docs/                  the reference, the tutorial and the guides (also inside `mimic man`)
editors/               mimic-syntax and mimic-vscode (submodules)
```

## License

By contributing, you agree that your contribution is licensed under the [Apache License, Version 2.0](LICENSE), like the rest of Mimic.
