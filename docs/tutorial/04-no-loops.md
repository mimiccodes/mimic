---
title: Loops and loop words
description: Iteration with each, gather, trail, evolve, settle, arms and recursion, and the plain loops for, while and break.
order: 4
---

# Loops and loop words

Most iteration in Mimic is written with array operations and a handful of loop *words*, and it pays to reach for them first:
* **Parallelism.** A loop forces one step after another. Array operations and the loop words below say *what* to compute, which leaves the compiler free to spread the work over every core.
* **Safety.** Many bug types simply can't happen: off-by-one indices, forgotten increments, loops that never end by accident.

For work that really is one step after another (a parser walking its input, a simulation's state, a search), there are also plain loops: `for`, `while` and `break` ([below](#for-while-and-break)). They run on one core, in order.

| You want to… | Use |
|---|---|
| transform every element | `xs ¨ f` (each) |
| combine all elements into one | `xs ▷ gather(f)`, or `+/` `×/` `⌈/` `⌊/` `∧/` `∨/` |
| keep the running result | `xs ▷ trail(f)` |
| repeat a step *n* times | `x ▷ evolve(n, step)` |
| repeat until a condition holds | `x ▷ settle(step, done)` |
| split work in two, recursively | `arms (…) { … arm(…) … }` |
| anything else | a recursive function |
| step by step, changing variables | `for`, `while`, `break` |

## each: `¨`

```mimic run
module main

fn main() {
    let xs = [1 2 3 4]
    print(xs ¨ (x => x * x))
    print(xs ¨ double)
    print(["a" "bb" "ccc"] ¨ (s => s + "!"))
}

fn double(x: i64) -> i64 { x * 2 }
```

```output
[1 4 9 16]
[2 4 6 8]
["a!" "bb!" "ccc!"]
```

`x => x * x` is a **lambda**, an unnamed function (see [chapter 8](08-functions-as-values.md)). When the function is pure, meaning it doesn't print or call C, `¨` runs it on all cores.

## gather: combine everything

```mimic run
module main

fn main() {
    let prices = [4 8 15 16 23 42]
    print(prices ▷ gather(+))          // the same as +/
    print(prices ▷ gather(max))        // the same as ⌈/
    print([12 18 30] ▷ gather(gcd))    // any associative function
}

fn gcd(a: i64, b: i64) -> i64 { if b == 0 { a } else { gcd(b, a % b) } }
```

```output
108
42
6
```

`gather` may combine elements in any grouping to work in parallel, so the function must be **associative**: `f(f(a, b), c)` must equal `f(a, f(b, c))`. `+`, `×`, `max` and `min` all are. The glyph form is `∕`.

## trail: running results

```mimic run
module main

fn main() {
    print([1 2 3 4 5] ▷ trail(+))      // running totals
    print([3 1 4 1 5] ▷ trail(max))    // running maximum
}
```

```output
[1 3 6 10 15]
[3 3 4 4 5]
```

The glyph form is `⍀`.

## evolve: repeat n times

```mimic run
module main

fn main() {
    print(1 ▷ evolve(10, x => x * 2))          // 2^10
    print([0 1] ▷ evolve(10, p => [p[1], p[0] + p[1]]))   // Fibonacci pair
}
```

```output
1024
[55 89]
```

The glyph form is `⍣`.

## settle: repeat until done

`settle(step, done)` checks `done` first, then applies `step`, until `done` is true:

```mimic run
module main

fn main() {
    // Newton's method for √2
    let root = 1.0 ▷ settle(x => (x + 2.0 / x) / 2.0, x => ∣(x * x - 2.0) < 1e-12)
    print(root)
    // the first power of two above 1000
    print(1 ▷ settle(x => x * 2, x => x > 1000))
}
```

```output
1.414213562373095
1024
```

The glyph form is `⍥`.

## arms: divide and conquer, in parallel

`arms` names some parameters with starting values. Inside its body, `arm(…)` recurses with new values. When the body is pure, the two halves of each split run in parallel:

```mimic run
module main

fn main() {
    let n = 1000000
    let total = arms (lo = 0, hi = n) {
        if hi - lo < 10000 {
            lo ‥ hi ▷ +/
        } else {
            let mid = (lo + hi) div 2
            arm(lo, mid) + arm(mid, hi)
        }
    }
    print(total)
}
```

```output
499999500000
```

The glyph form is `⋔`.

## for, while and break

A loop changes `let mut` variables of its function, one step after another:

```mimic run
module main

fn main() {
    let mut total = 0
    for i in 1 ‥ 11 {
        total = total + i
    }
    print(total)

    let mut n = 27
    let mut steps = 0
    while n > 1 {
        if n % 2 == 0 { n = n div 2 } else { n = 3 * n + 1 }
        steps = steps + 1
    }
    print(steps)

    let mut first = ¯1
    for x in [4 9 16 25] {
        if x > 10 {
            first = x
            break
        }
    }
    print(first)
}
```

```output
55
111
16
```

* `for i in a ‥ b` counts from `a` up to, not including, `b`; `for x in xs` takes each element of a list.
* `while cond { … }` runs while `cond` holds; `break` leaves the innermost loop.
* The glyphs are `∀` (`for`), `⟳` (`while`) and `↯` (`break`).
* A loop may change only its own function's `let mut` variables, and a lambda is a function of its own: a `break` inside one is an error (E0216).

A loop is sequential. When the steps don't depend on each other, `¨` and the reductions above do the same work on every core.

## Recursion

For anything the words don't cover, write a recursive function. When a function's last act is calling itself (a tail call), Mimic turns the call into a jump, in debug and release builds alike, so the recursion doesn't use up the stack however deep it goes. That holds when the function carries lists, strings or records too: whatever the call no longer needs is released or handed over before it. A call whose result is still worked on (`n * fact(n - 1)`) is not a tail call, and very deep recursion of that kind is better written with `evolve` or `settle`:

```mimic run
module main

fn main() {
    print(gcd(1071, 462))
    print(collatz_steps(27))
}

fn gcd(a: i64, b: i64) -> i64 { if b == 0 { a } else { gcd(b, a % b) } }

fn collatz_steps(n: i64) -> i64 { steps(n, 0) }

fn steps(n: i64, count: i64) -> i64 {
    if n == 1 { count }
    else if n % 2 == 0 { steps(n div 2, count + 1) }
    else { steps(3 * n + 1, count + 1) }
}
```

```output
21
111
```

Pure recursive functions are also parallel. In `fib(n - 1) + fib(n - 2)`, both calls run at the same time ([chapter 10](10-parallel-by-default.md)).

Next: [records, unions and match](05-records-unions-match.md).
