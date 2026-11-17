---
title: Getting started
description: Install mimic, create a project and run your first program.
order: 1
---

# Getting started

## Install

On macOS (Apple silicon), Linux and WSL:

```sh
curl -fsSL https://mimic.codes/install.sh | sh
```

On Windows, in PowerShell:

```powershell
irm https://mimic.codes/install.ps1 | iex
```

`mimic` goes into `~/.mimic/bin` and onto your `PATH`. Open a new terminal and check that it works:

```sh
mimic --version
```

`mimic` is a single executable, and there is nothing else to install. The compiler carries LLVM, Clang, a linker and the C libraries it needs for every target it supports. To update later, run `mimic upgrade` (or the same install command again); `mimic upgrade --preview` follows the previews of the next version instead, and `mimic upgrade --nightly` the nightly builds (`--stable` goes back). For your editor, `curl -fsSL https://mimic.codes/vscode.sh | sh` (on Windows, `irm https://mimic.codes/vscode.ps1 | iex`) installs the Mimic extension in VS Code, Cursor, Windsurf and VSCodium.

## Your first program

Create a project and run it:

```sh
mimic create hello
cd hello
mimic run
```

`mimic create` makes this layout:

```text
hello/
├── mim.mim        project manifest: name, version, dependencies, C libraries
├── build.mim      build profiles and default targets
├── mod.mim        generated module index (commit it)
├── README.md
└── src/
    └── main.mim   your program
```

`src/main.mim` starts out like this:

```mimic run
module main

fn main() {
    print("hello, mimic")
}
```

```output
hello, mimic
```

How it works:

* **The module line.** Every file starts with `module <name>`. The program starts at `fn main` in `module main`.
* **`print`** prints one value of any kind, followed by a newline.

## Running single files

You don't need a project to try things. Any `.mim` file runs on its own:

```sh
mimic run scratch.mim
```

## Debug and release

`mimic run` compiles in **debug** mode: it compiles quickly and adds extra checks. `mimic build` makes the **fastest possible binary** by default, using full optimization and tuning for your exact CPU:

```sh
mimic build                 # → target/release/hello
mimic run --release         # run the optimized build
mimic build --debug         # a debug binary
```

## A first taste of arrays

```mimic run
module main

fn main() {
    let prices = [12.5 8.0 3.25]
    print(prices × 2)          // every element at once
    print(prices ▷ +/)         // ▷ pipes a value into the next step; +/ is sum
    print(⍳ 5)                 // ⍳ n counts 0 … n-1
}
```

```output
[25.0 16.0 6.5]
23.75
[0 1 2 3 4]
```

`×`, `▷`, `+/` and `⍳` are **glyphs**. Each one also has a plain word, so the program above can be written as:

```mimic run
module main

fn main() {
    let prices = [12.5 8.0 3.25]
    print(prices * 2)
    print(prices |> sum)
    print(iota 5)
}
```

```output
[25.0 16.0 6.5]
23.75
[0 1 2 3 4]
```

Use whichever you like, and mix them freely. The [next chapter](02-values-and-functions.md) covers values and functions, and [chapter 3](03-arrays-and-glyphs.md) explains glyphs properly.
