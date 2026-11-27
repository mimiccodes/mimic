---
title: Writing Mimic for agents
description: How an AI agent (or anyone new) writes correct, idiomatic Mimic the first time: the workflow, the idioms, and the mistakes code from other languages makes.
---

# Writing Mimic for agents

This page is for AI agents writing Mimic, and for anyone who learns best from rules. Mimic looks familiar, but several habits from C, Python or JavaScript are errors here. Everything below is checked against this version of the compiler.

## The workflow

1. **Look things up, don't guess.** `mimic man <name>` has a page for every standard function (`mimic man fs::read`), module (`mimic man json`), glyph (`mimic man ⌿` or `mimic man keep`) and error code (`mimic man E0303`). `mimic man --search "text"` searches everything. Add `--json` for machine-readable output.
2. **Check after every edit:** `mimic check --emit-json` gives every error with its code, location and, often, an exact fix. `mimic check --fix` applies the fixes the compiler is sure of.
3. **Run and test:** `mimic run` (or `mimic run file.mim`), and `mimic test` for `test "name" { expect(…) }` blocks.
4. **Ask about the code:** `mimic query definition|references|hover|symbols|effects …` answers as JSON (`mimic man workbench`).
5. **Format:** `mimic fmt` (words) or `mimic fmt --glyphs`.

## The shape of a program

```mimic run
module main

type Item = { name: str, price: f64, qty: i64 }

// the total of an order, with a discount above 100
fn total(items: [Item]) -> f64 {
    let subtotal = +/ (items ¨ (i => i.price × i.qty))
    if subtotal > 100.0 { subtotal × 0.9 } else { subtotal }
}

fn cheapest(items: [Item]) -> Option[str] {
    match min_by(items, i => i.price) {
        Some(i) => Some(i.name),
        None => None,
    }
}

fn main() {
    let order = [Item { name: "pen", price: 1.5, qty: 10 } Item { name: "book", price: 30.0, qty: 3 }]
    print("total: {total(order):.2}")
    print(cheapest(order))
    print(order ⌿ (order ¨ (i => i.qty > 5)) ¨ (i => i.name))
}

test "discount above 100" {
    expect(total([Item { name: "x", price: 50.0, qty: 3 }]) == 135.0)
}
```

```output
total: 94.50
Some("pen")
["pen"]
```

* The first line of every file is `module <name>`; a program's entry is `fn main()` in `module main`.
* Statements end at the end of the line. A block's last expression is its value; `return` is only for leaving early.
* Everything is private except `main`: write `pub fn` to use a function from another module.

## Idioms

| Instead of | Write |
|---|---|
| a `for` or `while` loop | `xs ¨ f` (map), `xs ⌿ mask` (filter), `+/ ⌈/ ⌊/ ×/` (reduce), `gather(f)`, `trail(f)` (scan), `evolve(n, f)`, `settle(f, done)`, or recursion |
| a counter `i` | `⍳ n` (0 to n-1), `lo ‥ hi` (lo to hi-1), `enumerate(xs)` |
| `x += 1` | `let mut x = 0` then `x = x + 1`, or better, compute the value directly |
| `null` | `Option[T]`: `Some(x)` / `None` |
| exceptions | `Result[T, E]`: `Ok(x)` / `Err(e)`, handled with `match` or `let x = r else { … }` |
| threads, async, `await` | nothing: pure work runs in parallel by itself, and waiting calls overlap by themselves |
| classes | records (`type User = { … }`), unions (`type Shape = Circle(f64) \| Rect(f64, f64)`) and functions |
| `obj.method()` | `function(obj)`, or a pipeline: `obj ▷ function` |

Glyphs and words mean the same thing and mix freely: `xs ▷ +/` is `xs |> sum`, `xs ¨ f` is `xs each f`. Prefer whichever the surrounding code uses.

## Mistakes code from other languages makes

| Mistake | Error | Write instead |
|---|---|---|
| `for x in xs { … }` | E0201 | `xs ¨ (x => …)`, see above |
| `let x = 1` then `let x = 2` | E0302: no shadowing | a new name, or `let mut x` and `x = 2` |
| assigning to a plain `let` | E0601 | `let mut` |
| `fn first<T>(…)` | E0201 | `fn first[T](xs: [T]) -> T`; generic types are `Option[i64]` |
| `type user = …` | E0204 | type and variant names start uppercase: `User` |
| `a && b`, `a \|\| b`, `!a` | E0201 | `a and b`, `a or b`, `not a` (or `∧ ∨ ¬`) |
| `print(a, b)` | E0701 | `print("{a} {b}")`: `print` takes one value |
| `[3 - 1 4]` meaning three numbers | no error, but it is `[2 4]` | `[3 -1 4]` (a `-` touching its number is a negative element) or `[3 ¯1 4]` |
| `7 / 2` expecting `3` | no error, but it is `3.5` | `7 div 2` (floored: `¯7 div 2` is `-4`); `%` is the remainder |
| `printf(…)`, `sqrt(…)` from C | E0303 | `c::printf(…)` in `unsafe { }`, or a `trusted extern "C"` block; `math::sqrt` exists already |
| `≢ "héllo"` expecting 5 | no error, but it is `6` | `≢` counts bytes; `≢ chars(s)` counts characters |
| `xs.len()`, `s.upper()` | E0303 or similar | `≢ xs`, `upper(s)` |
| `let sum = …`, `let len = …`, `let max = …` | E0214 | another name: the words of the primitives (`sum`, `len`, `take`, `keep`, `max`, `min`, `each`, …) are reserved; `mimic glyphs` lists them |

## Making code fast

* Keep functions pure (no `print`, files or C calls inside) where they do the work: pure code on arrays runs on every core with SIMD. `mimic query effects` lists which functions are pure.
* Work on whole arrays rather than element by element in recursion.
* `mimic build` makes the fastest binary (release); `mimic run` is a debug build. `mimic run --release` runs the fast one.

## Where the facts are

* `mimic man tutorial`: the tutorial, chapter by chapter (`mimic man 1` … `mimic man 20`).
* `mimic man syntax`, `mimic man std`: the language and the standard library.
* `mimic man coming-from-c`, `coming-from-python`, `coming-from-javascript`: side-by-side translations.
* `mimic glyphs`: every glyph with its word and how to type it (`\i` becomes `⍳`).
