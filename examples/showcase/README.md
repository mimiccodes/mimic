# Mimic showcase

A guided tour of every Mimic feature built so far. Each module in `src/` covers one area, and running the project prints the result of every example.

## 1. Requirements

You need these once, to build the `mimic` compiler:

| Tool | Version | macOS (Homebrew) |
|---|---|---|
| Rust | 1.88 or newer | `curl https://sh.rustup.rs -sSf \| sh` |
| LLVM | 23 | `brew install llvm` |
| Zig | tested with 0.17 | `brew install zig` |
| make | any | preinstalled |

LLVM must be at `/opt/homebrew/opt/llvm`, or you can edit `LLVM_SYS_231_PREFIX` in the repository's `.cargo/config.toml`. Zig compiles the runtime and links every binary, which is also what makes cross-compilation work.

## 2. Build the compiler

From this folder:

```sh
make compiler      # = cd ../.. && cargo build --release
```

The Makefile uses `mimic` from your `PATH` if it finds one, otherwise `../../target/release/mimic`. To point it somewhere else, use `make run MIMIC=/path/to/mimic`.

## 3. Run it

```sh
make run           # debug build with the leak check, then run
make release       # fastest binary (target/release/showcase), then run
```

`make` on its own lists every target:

| Command | What it does |
|---|---|
| `make check` | Type-checks every module without generating code. |
| `make run` | Builds in debug mode and runs. At exit it verifies that no memory leaked. |
| `make build` | Builds the fastest binary. Release is the default for `mimic build`. |
| `make release` | Builds the fastest binary and runs it. |
| `make parallel` | Times the release binary on 1 thread and on all cores. |
| `make ir` | Writes the LLVM IR and the generated interaction nets to `target/release/`. |
| `make net` | Runs a hand-written interaction net (`nets/fib.mnet`) and reports its speed. |
| `make cross` | Builds for Linux x86-64, Linux arm64 (static musl) and Windows. The targets are listed in `build.mim`. |
| `make cross-one TARGET=aarch64-macos` | Builds for one Zig target triple. |
| `make glyphs` | Prints the glyph ↔ word ↔ ASCII digraph table. |
| `make mods` | Lists every module and function, as recorded in `mod.mim`. |
| `make clean` | Removes the build output. |

You can also use `mimic` directly: `mimic run`, `mimic build`, `mimic check`, `mimic build --target x86_64-linux-gnu`, `mimic run --release`, and so on.

## 4. What's inside

```
showcase/
├── mim.mim          project manifest: name, version, kind = [bin, lib]
├── build.mim        build profiles + cross-compilation targets
├── mod.mim          generated index of modules and functions (don't edit)
├── Makefile
├── nets/fib.mnet    a raw interaction net, run with `make net`
└── src/
    ├── main.mim     entry point, written with glyph keywords (§ module, ƒ fn)
    ├── lib.mim      the library part: `showcase::banner`, `showcase::version`
    ├── arrays.mim   glyphs and words, trains, primitives, copy-on-write mutation
    ├── compute.mim  parallel-by-default recursion, deep recursion, parallel ¨
    ├── ffi.mim      calling C: trusted and `unsafe` extern functions
    ├── shapes.mim   union types, exhaustive match, nested modules
    ├── shop.mim     records, `with`, lists of records, tuples, named/default arguments
    └── web.mim      route patterns, Option/Result with let … else, durations, UI-style state
```

### Feature map

| Feature | Where | Example |
|---|---|---|
| Glyph and word syntax mixed freely | `arrays.mim` | `xs ▷ +/` = `iota 10 \|> sum` |
| Left-to-right trains | `arrays.mim` | `xs ⌽ ↑ 3`: reverse, then take 3 |
| Primitives `⍳ ⍴ ⌽ +/ ×/ ⌈/ ⌊/ ≢ ↑ ↓ ⌿ ¨ ‥ ⌈ ⌊ ∣` | `arrays.mim` | `1 ‥ 6 ×/` = 120 |
| Integer and float division | `arrays.mim` | `17 div 5`, `¯17 ⫽ 5`, `7 ÷ 2` |
| ASCII digraphs | `arrays.mim` | `\i 4 \> \o` = `⍳ 4 ▷ ⌽` |
| Immutable by default, `mut`, `←`, `↩`, copy-on-write | `arrays.mim` | `ys[0] = 100` leaves `snapshot` unchanged |
| Glyph keywords | `main.mim` | `§ main`, `ƒ main()` |
| Modules, `use`, `pub`, nested modules, library items | `main.mim`, `shapes.mim` | `showcase::banner`, `units::cm_to_inch` |
| Parallel recursion (interaction nets) | `compute.mim` | `fib(n - 1) + fib(n - 2)` runs on several cores |
| Deep recursion without stack overflow | `compute.mim` | `tri(100000)` |
| Parallel `¨` for pure functions; ordered for impure ones | `compute.mim` | `[10 20 25] ¨ fib`, `⍳ 3 ¨ shout` |
| Parallel array kernels | `compute.mim` | `big × 2 ▷ +/` over 5 million elements |
| Static types with unboxed numbers | everywhere | `fn gcd(a: i64, b: i64) -> i64` |
| C FFI | `ffi.mim` | `trusted extern "C" { fn sqrt(x: f64) -> f64 }` |
| Union types and exhaustive `match` | `shapes.mim` | `Circle(r) => 3.14159 × r × r` |
| Records and `with` | `shop.mim`, `web.mim` | `{ apple with qty: apple.qty + 5 }` |
| Lists of records and strings | `shop.mim` | `cart[1].name`, `cart ¨ name_of` |
| String interpolation and `==` | `shop.mim` | `"{count} items worth {value}"` |
| Named and default arguments | `shop.mim` | `line(currency: "£", item: apple, width: 3)` |
| Tuples | `shop.mim`, `web.mim` | `let (count, value) = summary(cart)` |
| Route patterns with typed captures | `web.mim` | `("GET", "/users/{id: i64}") => …` |
| Errors without exceptions | `web.mim` | `let u: User = find(id) else { return … }` |
| Durations | `web.mim` | `250ms + 2s` |
| UI-style state, messages and view | `web.mim` | `update(s, Inc)`, `view(s2)` |
| Cross-compilation | `build.mim` | `make cross` |

## 5. Expected output (abridged)

```
== Mimic showcase v0.1.0 ==
== arrays: glyphs and words ==
[0 1 2 3 4 5 6 7 8 9]
45
…
== compute: parallel by default ==
196418
5000050000
…
== web: routes, errors, durations, UI state ==
["200 home" "200 user #42" "200 post hello-world by #7" "201 created" "404 not found"]
user 1: ada
user 9: not found
…
[clicks: 10]
[clicks: 0]
== done ==
```

## 6. Try changing things

These are good ways to see the compiler's checks for yourself:

* **Remove the `Dot =>` arm** in `shapes.mim`'s `area`. `make check` reports that the `match` does not cover `Dot`.
* **Call `line(pear, colour: "red")`.** The compiler reports that `line` has no parameter `colour`.
* **Write `let x = 1` and then `let x = 2` in one function.** Mimic does not allow shadowing.
* **Call `print(shop::name_of(1))` from `main.mim`.** The compiler reports that `showcase::shop::name_of` is private: it isn't `pub`.
* **Run `make parallel`** to see the same binary on 1 thread and on all cores. `MIMIC_THREADS=4 ./target/release/showcase` picks a specific thread count.
