---
title: Parallel by default
description: What Mimic runs in parallel without being asked, why it is safe, and how to look under the hood.
order: 10
---

# Parallel by default

Mimic has no threads, no `async`, no locks and no `parallel` keyword. The compiler finds the work that can run at the same time and spreads it over all your cores. You write ordinary code.

## What runs in parallel

| Code | Runs in parallel when |
|---|---|
| `xs + ys`, `xs * 2`, `xs > 0`, `+/`, `⌈/`, … | the arrays are large enough to be worth splitting |
| chains like `(xs * 2 + 1) ▷ +/` | always fused into **one** pass over memory, then split across cores |
| `xs ¨ f` | `f` is pure, and the work is worth sharing: a short list runs on one core unless its elements are costly (the runtime times the first ones, and remembers what `f` cost before) |
| `gather`, `trail`, `arms` | the function or body is pure |
| `f(a) + g(b)`, `Node(make(d - 1), make(d - 1))`, `let a = f(x)` `let b = f(y)` | `f` and `g` are pure and can recurse: independent calls run at the same time |
| `let x = f(…)` where `f` only reads, sleeps or fetches | always: `f` starts at once and the program waits only where `x` is used |
| C functions | when their `extern` block says `pure` or `concurrent` ([chapter 12](12-calling-c.md)) |

**Pure** means the code never prints, never calls C and never does other input or output. The compiler works this out itself; you never annotate it.

```mimic run
module main

fn main() {
    // both recursive calls run at once, all the way down
    print(fib(30))

    // one fused, parallel loop: no temporary arrays are built
    let xs = ⍳ 10000000
    print((xs * 2 + 1) ▷ +/)

    // a pure function mapped over a list: all cores
    print([25 26 27 28] ¨ fib)
}

fn fib(n: i64) -> i64 {
    if n < 2 { n } else { fib(n - 1) + fib(n - 2) }
}
```

```output
832040
100000000000000
[75025 121393 196418 317811]
```

## Waiting on many things at once

Reading files, sleeping and, soon, network requests spend most of their time waiting. Mimic overlaps that waiting for you. A `let` whose function only does this kind of overlapping work starts right away, and the program carries on until it actually needs the value:

```mimic run
module main

fn main() {
    let dir = fs::temp_dir()
    print(fs::write(path::join(dir, "a.txt"), "alpha"))
    print(fs::write(path::join(dir, "b.txt"), "beta"))

    let start = time::clock()
    let a = slow_read(path::join(dir, "a.txt"))   // starts now
    let b = slow_read(path::join(dir, "b.txt"))   // starts now too
    print(a)                                     // waits here for a
    print(b)
    print(time::clock() - start < 600ms)               // both sleeps overlapped (one after the other takes 600ms)

    // ¨ runs them in parallel as well
    print(["a.txt" "b.txt"] ¨ (f => slow_read(path::join(dir, f))))
}

fn slow_read(path: str) -> Result[str, str] {
    time::sleep(300ms)
    fs::read(path)
}
```

```output
Ok(5)
Ok(4)
Ok("alpha")
Ok("beta")
true
[Ok("alpha") Ok("beta")]
```

Ordered effects such as `print` still happen exactly in program order. Before anything that changes files, the environment or other programs runs, everything started earlier finishes, so a write never overtakes a read. You write plain sequential code, and the waiting overlaps by itself.

## Why it is safe

Parallel code usually goes wrong when two threads change the same data. In Mimic that cannot happen:
* **Immutability.** Values are immutable unless declared `let mut`, and a `mut` variable belongs to one function. Lambdas cannot change captured variables.
* **Copy-on-write.** Updating an array or record changes it in place only if nobody else holds it. Otherwise the update goes to a fresh copy.
* **Isolated effects.** Input and output make a function impure. Impure code runs in program order, so output is never interleaved.

## How it works

Independent calls are **forked and joined**. When an expression evaluates two or more calls to pure functions that can recurse, as in `fib(n - 1) + fib(n - 2)`, a constructor like `Node(make(d - 1), make(d - 1))`, or a run of `let`s, the calls go to a work-stealing scheduler that spreads them over your cores. Near the top of the recursion the calls fork. Deeper down, once there are enough tasks to keep every core busy, they run as plain sequential code, so the small calls cost no more than in C. Small programs never wake the other cores.

Pure recursive functions also have a second form, an **interaction net**: a graph in which every independent piece of work can be reduced by any core at any time. Recursion too deep for the native stack continues there instead of overflowing.

Freeing is parallel too: dropping a large tree hands its subtrees to the same scheduler.

Array operations are **fused**. `(xs * 2 + 1) ▷ +/` becomes a single loop that reads `xs` once. Reversing, taking and dropping (`⌽ ↑ ↓`) fuse too: `⌽ (xs * 2) ↓ 1 ▷ +/` reads each element of `xs` once and builds no array at all. So does filtering before a sum, product or count: `xs ⌿ (xs > 0) ▷ +/` is one pass. The loop is split into chunks across cores and vectorized with your CPU's SIMD instructions.

## Seeing where code runs

`mimic why` lists every decision the compiler made about your program, with
the reason and, where it helps, what would change it:

```sh
mimic why            # the whole program
mimic why fib        # one function
```

```output
fib  (src/main.mim:3)
  ∥ parallel line 3    fib(n - 1)
      runs in parallel with the other 1 call(s) here (fork-join): they are pure and independent; …
main  (src/main.mim:7)
  ⇶ fused    line 8    xs ¨ square
      the operations and the reduction are one loop, with no arrays in between; …
  → ordered  line 9    shout
      `¨` applies `shout` to one element after another: it has effects that keep their order …
```

| Label | Meaning |
|---|---|
| `∥ parallel` | Runs on all cores: `¨` with a pure function, independent recursive calls (fork-join) |
| `∥ net` | Also compiled to an interaction net, reduced in parallel by the runtime |
| `⇶ fused` | A chain of array operations is one loop with no arrays in between, split over the cores when large |
| `≋ SIMD` | The function runs on several elements at once, with vector instructions |
| `▣ GPU` | Also compiled for the GPU, used when the arrays are large enough |
| `◷ task` | Starts now and runs while the code after it goes on (waiting on files or the network) |
| `→ in order` | One after another, because of effects that must keep their order (printing, writing, C calls) |
| `⏸ waits` | Waits here for running tasks, because this changes what they might read |

Editors show the same labels after the code they are about (`mimic lsp`),
with the reason on hover, and agents ask with the `why` tool of `mimic mcp`.

## Looking under the hood

```sh
mimic build --emit-net     # write the interaction nets: target/release/<name>.mnet
mimic build --emit-ir      # write the LLVM IR
mimic build --no-net       # compile everything as plain native code, to compare
```

Benchmarks live in `BENCHMARK.md` at the root of the Mimic repository.

Next: [projects and modules](11-projects-and-modules.md).
