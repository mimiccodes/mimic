---
title: Learn Mimic
description: A guided tour of Mimic, from your first program to parallel code, C libraries and cross-compiling.
order: 0
---

# Learn Mimic

Mimic is a compiled language for programs that should be **fast without effort**. Its main traits:

* **Array-based.** Whole arrays are values. `prices × 1.2` updates every price at once.
* **Parallel by default.** You never mark code as parallel. The compiler runs everything that can safely run at the same time across all your cores.
* **Memory safe.** It has no garbage collector and no manual freeing. Values are immutable unless you say `mut`.
* **No loops.** Iteration is written with array operations and a handful of loop *words*.
* **One binary.** `mimic` contains its own LLVM, Clang and linker. It builds native programs for macOS, Linux, Windows, iOS, WebAssembly and more, with nothing else installed.

## How to use this guide

The chapters build on each other, so read them in order the first time. Every example that shows output is a complete program you can paste into a file and run with `mimic run file.mim`. The outputs shown are checked against the real compiler.

| # | Chapter | You will learn |
|---|---|---|
| 1 | [Getting started](01-getting-started.md) | Install, your first program, `mimic run` |
| 2 | [Values and functions](02-values-and-functions.md) | Numbers, `let`, `fn`, `if`, string interpolation |
| 3 | [Arrays and glyphs](03-arrays-and-glyphs.md) | Array operations, glyphs ↔ words, pipelines |
| 4 | [No loops](04-no-loops.md) | `¨`, `gather`, `trail`, `evolve`, `settle`, `arms`, recursion |
| 5 | [Records, unions and match](05-records-unions-match.md) | Your own types and pattern matching |
| 6 | [Strings, lists and maps](06-strings-lists-maps.md) | The standard library |
| 7 | [Errors without exceptions](07-errors.md) | `Option`, `Result`, `let … else` |
| 8 | [Functions as values](08-functions-as-values.md) | Lambdas, closures, operators as values |
| 9 | [Generics](09-generics.md) | `Option[T]`, `fn first[T]`, generic types |
| 10 | [Parallel by default](10-parallel-by-default.md) | What runs in parallel, and why it is safe |
| 11 | [Projects and modules](11-projects-and-modules.md) | `mimic create`, `mim.mim`, modules, `pub`, libraries |
| 12 | [Calling C](12-calling-c.md) | `extern "C"`, importing headers, callbacks, declaring effects |
| 13 | [Building for other platforms](13-targets.md) | `--target`, `--cpu`, `mimic targets` |
| 14 | [Tooling](14-tooling.md) | `check`, `explain`, `--fix`, JSON diagnostics |
| 15 | [Arrays in depth](15-multidimensional-arrays.md) | Matrices, reshape, transpose, `m[i, j]`, fast matrix products |
| 16 | [Files and the outside world](16-files-and-programs.md) | Arguments, environment, stdin, files, directories, paths, time, random numbers, running programs |
| 17 | [Networking](17-networking.md) | The `http`, `ws` and `tcp` modules: HTTPS, servers, routes, WebSockets, sockets |
| 18 | [JSON](18-json.md) | `json::encode`, `json::decode[T]` into your records, raw strings, a JSON API |
| 19 | [Tasks and channels](19-tasks-and-channels.md) | Background tasks, `chan::new[T]()`, a worker, a WebSocket chat room |
| 20 | [Interfaces](20-interfaces.md) | `view(state)`, `update(state, message)`, `ui::run` in the browser, a to-do list |

For the complete rules, see the reference in [`docs/`](../docs/): [syntax](../docs/syntax.md), [standard library](../docs/std.md), [FFI](../docs/ffi.md), [targets](../docs/targets.md) and [CLI](../docs/cli.md).
