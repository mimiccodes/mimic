# Mimic showcase

A guided tour of every Mimic feature built so far. Each file in `src/` covers one area. Running the project prints the result of every example, and section 6 lists the exact output to compare against.

**Quick start** (once the requirements in section 1 are installed):

```sh
cd examples/showcase
make compiler    # build the mimic compiler from this repository (first time only)
make run         # compile and run the tour
```

---

## 1. Requirements

These are needed once, to build the `mimic` compiler.

| Tool | Version | Used for |
|---|---|---|
| Rust | 1.88 or newer | building the compiler |
| LLVM | 23 | code generation |
| Zig | tested with 0.17 | compiling the runtime, linking, and cross-compilation |
| make | any | the targets in this folder |

### macOS (Homebrew)

```sh
curl https://sh.rustup.rs -sSf | sh     # Rust
brew install llvm zig                   # LLVM 23 and Zig
```

### Linux (Debian / Ubuntu)

```sh
curl https://sh.rustup.rs -sSf | sh                         # Rust
wget https://apt.llvm.org/llvm.sh && sudo bash llvm.sh 23   # LLVM 23 → /usr/lib/llvm-23
# Zig: download from https://ziglang.org/download/ and put `zig` on your PATH
```

On Linux, also point the build at LLVM by editing the repository's `.cargo/config.toml`:

```toml
LLVM_SYS_231_PREFIX = { value = "/usr/lib/llvm-23", force = false }
```

> Development and testing so far have been on macOS (Apple Silicon). The Linux steps are the standard ones but have not been tested yet. Building the compiler *on* Windows is untested too, although compiling *for* Windows works (see `make cross`).

---

## 2. Get the `mimic` compiler

Pick one of these:

```sh
# a) build it inside the repository (the Makefile finds it automatically)
make compiler                          # = cd ../.. && cargo build --release

# b) or install it on your PATH, so `mimic` works anywhere
cargo install --path ../../crates/mimic-cli
```

The Makefile uses `mimic` from your `PATH` if there is one, and otherwise `../../target/release/mimic`. To use a different compiler, run `make run MIMIC=/path/to/mimic`.

---

## 3. Run it

| Command | What it does |
|---|---|
| `make` | Lists every target. |
| `make run` | Builds in **debug** mode and runs. At exit it verifies that no memory leaked. |
| `make release` | Builds the **fastest** binary (`target/release/showcase`) and runs it. |
| `make check` | Type-checks every module without generating code. |
| `make build` | Builds the fastest binary without running it. Release is the default for `mimic build`. |
| `make parallel` | Times the release binary on 1 thread, then on all cores. |
| `make ir` | Writes the LLVM IR (`.ll`) and the generated interaction nets (`.mnet`) to `target/release/`. |
| `make net` | Runs the hand-written interaction net `nets/fib.mnet` and prints its speed. |
| `make cross` | Builds for Linux x86-64, Linux arm64 (static musl) and Windows. The list is in `build.mim`. |
| `make cross-one TARGET=aarch64-macos` | Builds for any one Zig target triple. |
| `make glyphs` | Prints the glyph ↔ word ↔ ASCII digraph table. |
| `make mods` | Lists every module and function, as recorded in `mod.mim`. |
| `make clean` | Removes the build output. |

The same things work without make, using `mimic` directly:

```sh
mimic run                        # debug build + run
mimic run --release              # fast build + run
mimic build                      # fastest binary → target/release/showcase
mimic build --target x86_64-linux-gnu
mimic check
```

To choose the number of threads: `MIMIC_THREADS=4 ./target/release/showcase` (the default is all cores).

---

## 4. Reading Mimic in two minutes

```mimic
module shop                                  // every file starts with its module (glyph: §)

type Item = { name: str, price: f64, qty: i64 }    // a record
type Shape = Circle(f64) | Rect(f64, f64) | Dot    // a union: one of several variants

pub fn total(i: Item) -> f64 {               // `pub` = visible outside this module
    i.price × i.qty                          // the last expression is the result
}

fn main() {
    let xs = ⍳ 10                            // immutable binding; ⍳ = iota (0..9)
    print(xs ▷ +/)                           // ▷ pipes left to right; +/ = sum
    print(iota 10 |> sum)                    // the same, spelled with words
    mut n ← 1                                // a mutable binding (← is `let`)
    n ↩ n + 1                                // reassign (↩ is `=`)
    print("n is {n}")                        // string interpolation
}
```

* **Glyphs and words.** Every glyph has a word, and you can mix them. `make glyphs` prints the table. On an ASCII keyboard, type `\i` for `⍳`, `\>` for `▷`, and so on.
* **Visibility.** Everything is private unless marked `pub`, except `main`.
* **No loops.** You work on whole arrays (`xs × 2`), use `¨`/`each` (`xs ¨ f`), and write recursion. More loop words (`gather`, `trail`, `evolve`, `settle`, `arms`) are coming.
* **No annotations for parallelism.** Pure functions and array operations run on all cores automatically.

---

## 5. What's inside

```
showcase/
├── mim.mim          project manifest: name, version, kind = [bin, lib]
├── build.mim        build profiles and cross-compilation targets
├── mod.mim          generated index of modules and functions (do not edit)
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

| Feature | File | Example |
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

---

## 6. Expected output

`make run` and `make release` print exactly this (with the `Running …` line from `make run` printed first):

```
== Mimic showcase v0.1.0 ==
== arrays: glyphs and words ==
[0 1 2 3 4 5 6 7 8 9]                       ⍳ 10
45                                          xs ▷ +/
45                                          iota 10 |> sum
[9 8 7]                                     xs ⌽ ↑ 3
[7 8 9]                                     xs ↓ 7
10                                          xs ≢
[10]                                        xs ⍴
120                                         1 ‥ 6 ×/
9                                           [4 9 1 7] ⌈/
1                                           [4 9 1 7] ⌊/
[0 2 4 6 8]                                 xs ⌿ (xs % 2 == 0)
[0 1 4 9 16 25 36 49 64 81]                 xs ¨ (x => x × x)
[6 8 4]                                     [1 8 3] ⌈ [6 2 4]
[3 2 1]                                     ∣ [¯3 2 ¯1]
3                                           17 div 5
-4                                          ¯17 ⫽ 5
3.5                                         7 ÷ 2
[3 2 1 0]                                   \i 4 \> \o
[200 2 4 6 8]                               ys after ys[0] = 100 and ¨ double
[0 1 2 3 4]                                 snapshot (unchanged)
== compute: parallel by default ==
196418                                      fib(27)
5000050000                                  tri(100000)
103                                         ackermann(2, 50)
21                                          gcd(1071, 462)
111                                         collatz(27)
[55 6765 75025]                             [10 20 25] ¨ fib
24999995000000                              ⍳ 5000000 × 2 ▷ +/
  shout 0                                   impure ¨ runs in order
  shout 1
  shout 2
3
== ffi: calling C ==
1.4142135623731                             sqrt(2.0)
5.0                                         hypot(3.0, 4.0)
5                                           strlen("mimic")
== shapes: unions and match ==
[Circle(1.0) Rect(2.0, 3.0) Square(2.0) Dot]
[3.14159 6.0 4.0 0.0]                       all ¨ area
["round" "rect" "square" "point"]           all ¨ describe
13.14159                                    all ¨ area ▷ +/
100.0                                       units::cm_to_inch(254.0)
== shop: records, tuples, named arguments ==
Item { name: "apple", price: 0.5, qty: 10 }
15                                          more.qty
15.5                                        cart ¨ total ▷ +/
pear                                        cart[1].name
pear: 4 × $0.75 (w1)                        line(pear)
pear: 4 × €0.75 (w1)                        line(pear, currency: "€")
apple: 10 × £0.5 (w3)                       line(currency: "£", item: apple, width: 3)
3 items worth 15.5                          let (count, value) = summary(cart)
["apple" "pear" "apple"]                    cart ¨ name_of
true                                        names[0] == "apple"
== web: routes, errors, durations, UI state ==
["200 home" "200 user #42" "200 post hello-world by #7" "201 created" "404 not found"]
user 1: ada                                 lookup(1)
user 9: not found                           lookup(9): the `else` branch
age 42, next year 43                        age_of("42")
invalid age `forty`                         age_of("forty"): "forty" isn't an i64
timeout = 2250 ms                           250ms + 2s
[clicks: 10]                                Inc, Add(10), Dec
[clicks: 0]                                 Reset
== done ==
```

(The right-hand column is commentary, not program output.)

---

## 7. Try changing things

Each of these shows one of the compiler's checks. All five have been tested.

| Change | What the compiler says |
|---|---|
| Remove the `Dot =>` arm from `area` in `shapes.mim` | `` `match` does not cover every case: missing `Dot` `` |
| In `shop.mim`, change `line(pear)` to `line(pear, colour: "red")` | `` `line` has no parameter `colour` `` |
| Write `let x = 1` and then `let x = 2` in one function | `` `x` is already defined; Mimic does not allow shadowing `` |
| Add `print(shop::name_of(1))` to `main.mim` | `` function `showcase::shop::name_of` is private `` |
| Pass a float where an `i64` is expected: `gcd(10.5, 3)` | `` argument `a` of `showcase::compute::gcd`: expected i64, found f64 `` |

Run `make check` after each change, then undo it.

---

## 8. Troubleshooting

| Symptom | Fix |
|---|---|
| `error: mimic compiler not found` | Run `make compiler`, or install with `cargo install --path ../../crates/mimic-cli`. |
| Building the compiler fails in `llvm-sys` because it can't find LLVM | Install LLVM 23 and set `LLVM_SYS_231_PREFIX` in `.cargo/config.toml` to its path. |
| `cross-compilation needs Zig` | Install Zig and make sure `zig` is on your `PATH` (or set `MIMIC_ZIG=/path/to/zig`). |
| The first run of a new binary takes about a second on macOS | macOS scans newly built executables once. Later runs start instantly. |
| Timings from `make parallel` vary between runs | Other programs share the cores. Run it a few times and compare the best. |
