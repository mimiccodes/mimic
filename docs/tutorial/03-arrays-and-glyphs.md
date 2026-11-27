---
title: Arrays and glyphs
description: Whole-array operations, primitives, glyph and word spellings, pipelines and trains.
order: 3
---

# Arrays and glyphs

In Mimic you rarely handle one element at a time. Operators apply to **whole arrays**.

## Elementwise arithmetic

```mimic run
module main

fn main() {
    let xs = [1 2 3 4]
    print(xs + 10)          // a scalar is applied to every element
    print(xs * xs)          // two equal-length arrays: pairwise
    print(xs > 2)           // comparisons give arrays of bools
    print(xs % 2 == 0)
}
```

```output
[11 12 13 14]
[1 4 9 16]
[false false true true]
[false true false true]
```

Elements of an array literal are separated by spaces or commas. Write negative elements as `¯3`, or use commas, because `[1 -2]` means `[1 - 2]`.

## Primitives

Primitives are built-in array operations. Each has a **glyph** and a **word**:

| Glyph | Word | Meaning |
|---|---|---|
| `⍳` | `iota` | `⍳ 5` → `[0 1 2 3 4]` |
| `‥` | `range` | `2 ‥ 6` → `[2 3 4 5]` |
| `≢` | `len` | length |
| `⌽` | `reverse` | reverse |
| `↑` / `↓` | `take` / `drop` | first or last *n* elements |
| `⌿` | `keep` | keep elements where a mask is true |
| `+/` `×/` | `sum` `product` | add or multiply all elements |
| `⌈/` `⌊/` | `maximum` `minimum` | largest or smallest element |
| `∧/` `∨/` | `all` `any` | are all of these true? is any? |
| `⌈` `⌊` | `max` `min` | larger or smaller of two values |
| `∣` | `abs` | absolute value |
| `¨` | `each` | apply a function to every element |
| `⥊` `⍉` `⊠` | `reshape` `transpose` `matmul` | matrices ([chapter 15](15-multidimensional-arrays.md)) |

```mimic run
module main

fn main() {
    let xs = [5 3 8 1 9 2]
    print(≢ xs)
    print(⌽ xs)
    print(xs ↑ 3)           // first 3
    print(xs ↓ 4)           // drop 4
    print(xs ↑ ¯2)          // last 2
    print(xs ⌿ (xs > 4))    // filter
    print(xs ▷ ⌈/)          // maximum
    print(2 ‥ 6)
}
```

```output
6
[2 9 1 8 3 5]
[5 3 8]
[9 2]
[9 2]
[5 8 9]
9
[2 3 4 5]
```

## Reading left to right

Mimic reads strictly **left to right**. A chain of primitives is called a *train*:

```mimic run
module main

fn main() {
    // "count to 10, reverse, take 3, sum": 9 + 8 + 7
    print(⍳ 10 ⌽ ↑ 3 +/)
    // the same with words and the pipe
    print(iota 10 |> reverse |> take(3) |> sum)
}
```

```output
24
24
```

How the pieces work:
* **The pipe.** `▷` (or `|>`) passes the value on its left as the first argument of what follows: `xs ▷ take(3)` is `take(xs, 3)`.
* **Prefix primitives** bind to the next value only: `⍳ 5 + 1` is `(⍳ 5) + 1`.

## Typing glyphs

You don't need a special keyboard. Every glyph has an ASCII **digraph** starting with a backslash, which the compiler reads as the glyph itself:

```mimic run
module main

fn main() {
    print(\i 4 \o)          // ⍳ 4 ⌽
}
```

```output
[3 2 1 0]
```

Run `mimic glyphs` to see the full table of glyphs, words and digraphs.

## Keywords have glyphs too

Every keyword has a glyph as well: `ƒ` for `fn`, `⍰` for `if`, `⋄` for `else`, `⏎` for `return`, `⊡` for `type`, `⊨` for `match`, `⇑` for `pub` and so on ([the full table](../syntax.md#keyword-glyphs)). Write whichever reads better; `mimic fmt` turns them all back into words.

```mimic run
§ main

ƒ sign(n: i64) → i64 {
    ⍰ n < 0 { ⏎ ¯1 }
    ⍰ n == 0 { 0 } ⋄ { 1 }
}

ƒ main() {
    print(⍳ 5 ¨ (i => sign(i - 2)))
}
```

```output
[-1 -1 0 1 1]
```

## Indexing and updating

```mimic run
module main

fn main() {
    let xs = [10 20 30]
    print(xs[1])
    let mut ys = xs
    ys[1] = 99              // copy-on-write: xs is untouched
    print(xs)
    print(ys)
}
```

```output
20
[10 20 30]
[10 99 30]
```

Arrays are values. Updating `ys` never changes `xs`. Under the hood, Mimic copies only when the array is actually shared, so an update to an array you own is done in place.

## Lists of anything

Arrays can hold strings, records or other arrays:

```mimic run
module main

fn main() {
    let names = ["ada" "grace" "alan"]
    print(names[2])
    print(≢ names)
    print(names ¨ (n => ≢ n))   // the length of each name
}
```

```output
alan
3
[3 5 4]
```

## Small lists are free

When the compiler can see how long a list of numbers is, it keeps short ones (up to 16 elements) in registers rather than on the heap. A literal like `[x y z]`, `⍳ 5`, or anything elementwise built from them counts. So does a record whose fields are such lists or plain numbers. Nothing in your program changes, and there is no special type to write. A point, a color, a 3×3 filter kernel or a 4×4 matrix as a list costs no more than separate variables would:

```mimic run
module main

type Body = { pos: [f64], vel: [f64] }

fn step(b: Body) -> Body {
    { b with pos: b.pos + 0.5 * b.vel }
}

fn main() {
    let b = Body { pos: [0.0 0.0 0.0], vel: [1.0 2.0 3.0] }
    let done = b ▷ evolve(4, s => step(s))
    print(done.pos)
}
```

```output
[2.0 4.0 6.0]
```

Here `step` works on six numbers in registers and the four steps allocate nothing. As soon as such a value meets code that takes a list of any length (printing it, storing it in a longer list, passing it to C), it becomes an ordinary list once, at that point.

Arrays with more than one dimension are covered in the [arrays in depth](15-multidimensional-arrays.md) chapter.

Next: [no loops](04-no-loops.md).
