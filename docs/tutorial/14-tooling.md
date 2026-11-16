---
title: Tooling
description: Checking code, testing it, understanding error codes, automatic fixes and machine-readable diagnostics.
order: 14
---

# Tooling

## Check without building

```sh
mimic check                 # the whole project
mimic check file.mim        # one file
```

`check` reports every error at once, with the exact source location and, where possible, a suggestion:

```text
error[E0501]: `match` does not cover every case: missing `Dot`
  --> src/main.mim:5:5
   |
 5 |     match s {
   |     ^^^^^^^^^
  = note: add the missing arms, or a final `_ => …`
  = fix: add the missing arms
```

A `fix:` line means `mimic check --fix` can make the change for you.

## Tests

Write a test next to the code it checks, with `test "what it checks" { … }`, and check with `expect`:

```mimic
module main

fn square(x: i64) -> i64 { x * x }

fn main() { print(square(4)) }

test "square" {
    expect(square(3) == 9)
    expect(⍳ 4 ¨ square ▷ +/ == 14)
}

test "a wrong expectation" {
    expect(square(2) == 5)
}
```

```sh
mimic test                  # every test of the project (and src/test.mim)
mimic test square           # only the tests whose names contain "square"
mimic test file.mim         # the tests of one file
```

```output
     Testing 2 tests
  ✓ square
  ✗ a wrong expectation
      line 13: expect(square(2) == 5)
        left:  4
        right: 5
  1 passed, 1 failed (0.01 s)
```

* **Every failure at once.** A failed `expect` does not stop its test, and a comparison shows both sides.
* **A crash fails one test.** Each test runs in a process of its own, and the tests run in parallel.
* **Tests are never built into the program.** `mimic run` and `mimic build` leave them out, but `mimic check` still checks them.
* **Private items are in reach.** A test sees everything its module sees. Tests in `src/test.mim` (`module test`) reach the project's `pub` items, such as `main::square`.
* **What the tests miss.** `mimic test --coverage` counts how often the tests ran each line of the program, and lists the lines they never reached (also `lcov.info` for coverage services, and the gutter in VS Code).

### Property tests

Instead of picking inputs yourself, name them after `for`, and Mimic tries
100 of them: small and large numbers, empty and long lists, odd text. When
one breaks the test, Mimic shrinks it to the smallest input that still does,
and shows that case.

```mimic
module main

fn main() { print(1) }

test "sorting keeps the length" for xs: [i64] {
    expect(≢ sort(xs) == ≢ xs)
}

test "every list is short" for xs: [i64] {
    expect(≢ xs < 3)
}
```

```output
     Testing 2 tests
  ✓ sorting keeps the length
  ✗ every list is short
      line 10: expect(≢ xs < 3)
        left:  3
        right: 3
      line 9: the property fails; the smallest failing case: xs = [0 0 0]
```

Parameters can be `i64`, `f64`, `bool`, `str`, or lists of them. The inputs
are the same on every run, so a failure is easy to reproduce.

### Contracts

A function can say what it needs and what it promises, with `requires` and
`ensures` lines before its body (`result` is the value it returns):

```mimic run
module main

fn isqrt(n: i64) -> i64
    requires n >= 0
    ensures result * result <= n and (result + 1) * (result + 1) > n
{
    if n < 2 { return n }
    let r = isqrt(n div 4) * 2
    if (r + 1) * (r + 1) <= n { r + 1 } else { r }
}

fn main() {
    print(isqrt(17))
    print(isqrt(1000000))
}
```

```output
4
1000
```

`mimic run` and `mimic test` check contracts on every call; `isqrt(¯4)` would
stop with ``contract broken: `isqrt` requires n >= 0 (line 4), and the call
broke it``. Release builds leave the checks out, so they cost nothing there.
Editors and `mimic doc` show contracts as part of the signature.

### Examples in comments

An example in the comment above a function is a test too: a `// > expression` line, then the line `print` would show for its value.

```mimic
// The larger of two numbers.
// > larger(3, 7)
// 7
fn larger(a: i64, b: i64) -> i64 { a ⌈ b }
```

`mimic test` runs every such example, so the documentation can't drift from the code: a failure names the example's line and shows both values.

## Mimic's own editor

`mimic edit` opens the project in Mimic's terminal editor: errors as you
type, glyphs by typing `\i` (or Ctrl-G for the palette), F5 to run, F6 to
test, F4 to evaluate a line in the REPL pane, F1 for the manual on the word under the cursor, and labels showing
where code runs in parallel. Ctrl-S saves, Ctrl-Q quits. `mimic man editors`
lists every key. Everything is clickable as well: the toolbar, the folders of the file tree, the tabs, and any `file.mim:12` in a pane; **≡ Commands** (F2) lists every command.

## Where the time goes

`mimic profile` runs the program and shows which functions take the time,
on how many cores, on the CPU or the GPU, and how much memory it allocates,
with where the compiler runs each function (`∥ parallel`, `⇶ fused`,
`▣ GPU`). A function that takes most of the time on one core is the first
place to look; `mimic why` tells why it runs there. `mimic man profile`
explains every part of the report.

## Trying things out

`mimic repl` reads Mimic a line at a time and shows each value:

```text
› ⍳ 10 ▷ +/
45
› let xs = [3 1 4 1 5]
› xs × 2
[6 2 8 2 10]
› fn sq(x: i64) -> i64 { x * x }
› xs ¨ sq
[9 1 16 1 25]
```

`let`s and functions stay for the next lines; `:show` prints them as a
program you can paste into a file, `:help` lists the other commands. Every
line is compiled to native code, so what you see is what a program does.

## Editors

`mimic lsp` is the language server: errors as you type with quick fixes, hover, go to definition, glyph input (type `\i` and pick `⍳`), completion and formatting, in VS Code, Neovim, Helix, Emacs, Sublime Text and others. `mimic man editors` shows the set-up for each.

## Audit

`mimic audit` looks for code that is likely to go wrong: functions nobody
calls, `Result`s nobody checks, commands built from user input
(`os::shell("rm {name}")`), file paths taken from requests, passwords in the
source, and more. In CI it fails on new security findings; a baseline lets an
older project adopt it one finding at a time. `mimic man audit` lists the rules.

## AI agents

`mimic mcp` gives AI agents Mimic's tools over the Model Context Protocol: they check, build, run and test code, read the manual and look up symbols, inside your project and off the network unless you allow more. `mimic man mcp` shows the set-up for each agent app.

## The manual

`mimic man` is the manual inside `mimic`: the same version as the compiler, and it works offline.

```sh
mimic man                       # the contents
mimic man 4                     # chapter 4 of this tutorial (or `mimic man no-loops`)
mimic man fs::read              # a standard function: signature, effects, examples
mimic man json                  # a module, with everything in it
mimic man ⌿                     # a glyph; its word (`mimic man keep`) and digraph (`\k`) work too
mimic man E0303                 # an error code
mimic man --search "sort by"    # every page that mentions it
mimic man --run 4               # run a chapter's examples and compare their output
```

Every standard function, module, glyph and error code has a page made from the compiler itself, and every error message ends with the page to read. Guides translate habits from other languages (`mimic man coming-from-c`, `coming-from-python`, `coming-from-javascript`), and `mimic man agents` lists the rules for writing Mimic, for AI agents and for people. `--json` prints a page for tools.

## Formatting

```sh
mimic fmt                   # every .mim file of the project, in place
mimic fmt --glyphs          # the dense form: ⍳ ¨ ▷ ≠ ∧ instead of iota each |> != and
mimic fmt --check           # change nothing; list what is not formatted (for CI)
```

`mimic fmt` writes primitives and operators as words by default, so `⍳ 4 ¨ square ▷ +/` becomes `iota 4 each square |> sum`; `--glyphs` turns them back. It indents by nesting (4 spaces) and tidies spaces and blank lines. Your comments and line breaks stay, and the formatted file is always the same program: glyphs and words are the same tokens.

## Documentation

`mimic doc` writes the documentation of your project's public functions,
types and modules to `target/doc/index.html`: each with its full signature
and the `//` comment lines right above it. Write comments for readers there:

```mimic
module main

// A point on the plane.
pub type Point = { x: f64, y: f64 }

// Distance between two points, `a` and `b` (`code` in backquotes).
pub fn dist(a: Point, b: Point) -> f64 {
    math::sqrt((a.x - b.x) * (a.x - b.x) + (a.y - b.y) * (a.y - b.y))
}

fn main() {
    print(dist(Point { x: 0.0, y: 0.0 }, Point { x: 3.0, y: 4.0 }))
}
```

`mimic doc --std` documents the standard library (`fs`, `http`, `json`,
`ui` and the rest), and `--json` prints the same as JSON for editors.

### Guides

Signatures come from the code; explanations go in **guides**: Markdown pages
in the project's `docs/` folder. `mimic doc` puts them on the same page as
the API, in the sidebar under *Guides*, with Mimic code coloured and every
glyph explained when you hover it.

A complete program in a `mimic run` code block, followed by an `output` block
with exactly what it prints, is a test: `mimic test` runs it and compares, so
a guide's examples can't go stale. The tutorial you are reading is written
the same way.

An AI agent connected with `mimic mcp` can write the documentation for you:
`docs_status` lists what has no comment yet, and `docs_write` writes a guide
and runs its examples straight away (`mimic man mcp`).

## Rebuilding

`mimic build` and `mimic run` remember what a program was built from: its
source files, the C headers it imports, the flags and the compiler. When
none of that changed, they reuse the program they built last time instead
of compiling it again, which takes a fraction of a second. A debug build
(`mimic run`'s default) of a small program takes about a second; a
release build optimises more and takes a few.

## Debugging

Debug builds carry debug information (DWARF), so a debugger works with your
`.mim` files: breakpoints on lines, stepping, variables, and backtraces with
file and line. `mimic lldb` makes a debug build and starts lldb on it (on a
Mac lldb comes with the Xcode tools), with Mimic's values shown the way
Mimic prints them:

```sh
mimic lldb                     # or: mimic lldb app.mim -- program arguments
(lldb) breakpoint set --file main.mim --line 8
(lldb) run
(lldb) frame variable
(value) p = Point { x: 1.5, y: -2.0, name: "origin" }
(long[4]) xs = [1 2 3 4]
(value) words = ["a" "bc"]
(value) ages = ["ada": 36, "alan": 41]
(value) s = Circle(2.5)
(lldb) next
(lldb) bt
```

Functions appear under their Mimic names (`app::main::area`). Release
builds have no debug information.

Editors debug through `mimic dap`, which speaks the Debug Adapter Protocol
(through LLVM's `lldb-dap`): in VS Code, press F5 or click **Debug** above
`fn main`; other editors are set up in `mimic man editors`. Breakpoints,
stepping, the call stack and the variables work the same way, with the
same values.

## Error codes

Every error has a code. `mimic explain` describes what the code means and how to fix it, with an example:

```sh
mimic explain E0501
mimic explain               # list every code
```

## Automatic fixes

Many errors come with a fix the compiler can apply for you. It can rename `myType` to `MyType`, insert missing `match` arms, and more:

```sh
mimic check --fix
```

## For editors and scripts

`--emit-json` prints the result as JSON on stdout, including diagnostics, locations and suggested fixes. It works with `check`, `build` and `run`:

```sh
mimic check --emit-json
```

The format is described in [docs/diagnostics.md](../diagnostics.md).

`mimic query` asks the analysis engine about your code: where a function is defined, where it is used, its signature, its comment, and whether it is pure.

```sh
mimic query definition file=src/main.mim line=12 column=9
mimic query references name=area
mimic query symbols query=read
```

`mimic query --serve` keeps the engine running for an editor or a script, which gets an answer within milliseconds after each edit. The questions are listed in [docs/workbench.md](../workbench.md).

## Glyphs at a glance

```sh
mimic glyphs                # glyph ↔ word ↔ ASCII digraph for everything
```

## Useful build flags

| Flag | Effect |
|---|---|
| `--debug` / `--release` | Debug: fast to compile, extra checks. Release: fastest code. `build` defaults to release, `run` to debug. |
| `--target <triple>` | Cross-compile ([chapter 13](13-targets.md)) |
| `--cpu <name>` | Tune for a CPU, or `portable` |
| `--emit-ir`, `--emit-net` | Write the LLVM IR or the interaction nets next to the binary |
| `--emit-header` | Write a C header for `extern "C" fn` exports |
| `--shared` | A library as a shared library (`.dylib`, `.so`, `.dll`) exporting its `extern "C" fn`s |
| `--no-net` | Compile everything as plain native code |
| `--strict-math` | No fused multiply-add (see below) |
| `--fast-math` | Faster floats: sums may be regrouped and vectorised (see below) |

### Floating point: fused multiply-add

Like C compilers, Mimic lets the CPU compute `a * b + c` as one *fused
multiply-add* when both operations sit in the same expression. That is
faster, and slightly more accurate since there's one rounding instead of two. The cost
is that a result can differ in its last bits between a CPU that has the
instruction and one that does not. Nothing is ever reordered: `(a + b) + c`
stays exactly that. When you need the same bits on every machine (golden
files, simulations compared across platforms), build with `--strict-math`.

The other way, `--fast-math` lets Mimic regroup float sums and products
(`(a + b) + c` may become `a + (b + c)`), which lets the CPU add several
numbers at once: `+/` over a long list of floats gets several times faster.
Results can change in their last bits. It never assumes a number isn't NaN
or infinity, so every bounds check stays.

### The GPU

On a Mac, long maps over `[f32]`, `[i32]`, `[i64]` or `[bool]` with a lot
of work per element can run on the GPU, including `xs ¨ f` where `f` is a
pure function on such numbers (one that calls itself last becomes a loop
there). You don't ask for it, just as you don't ask for threads. The GPU
never fuses a multiply and an add, so its results are exactly those of
`--strict-math`. An f32 Mandelbrot set, for example:

```mimic
module main

fn inside(cr: f32, ci: f32, zr: f32, zi: f32, k: i32) -> i32 {
    if k == 200 {
        1
    } else if zr * zr + zi * zi > 4.0 {
        0
    } else {
        inside(cr, ci, zr * zr - zi * zi + cr, 2.0 * zr * zi + ci, k + 1)
    }
}

fn main() {
    let n = 400
    let s: f32 = 2.5 / n
    let pixels = ⍳ (n * n) ¨ (p => inside(-2.0 + s * (p % n), -1.25 + s * (p div n), 0.0, 0.0, 0))
    print(+/ pixels)
}
```

Set
`MIMIC_GPU=0` to keep everything on the CPU, `MIMIC_GPU=1` to send every
map the GPU can do there, and `MIMIC_GPU_DEBUG=1` to see which kernels ran:

```sh
MIMIC_GPU=1 MIMIC_GPU_DEBUG=1 ./target/release/app
```

That's the tour. For every detail, see the reference in [`docs/`](../).
