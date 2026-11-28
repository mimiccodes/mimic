---
title: Coming from Python
description: Python habits and their Mimic forms: comprehensions, built-ins, dicts, f-strings, None and exceptions.
---

# Coming from Python

Mimic reads a lot like Python with arrays built in, NumPy style: arithmetic works on whole lists at once. It is compiled, typed and runs on every core, so the same program usually runs much faster.

| In Python | In Mimic |
|---|---|
| `print(a, b)` | `print("{a} {b}")`: `print` takes one value |
| `[x * x for x in xs]` | `xs ¨ (x => x * x)`, or just `xs × xs` |
| `[x for x in xs if x > 2]` | `xs ⌿ (xs > 2)` |
| `sum(xs)`, `max(xs)`, `len(xs)` | `+/ xs`, `⌈/ xs`, `≢ xs` (or `sum`, `maximum`, `len`) |
| `range(10)`, `range(2, 5)` | `⍳ 10`, `2 ‥ 5` |
| `xs[1:4]` | `xs ↓ 1 ↑ 3` |
| `sorted(xs, key=f, reverse=True)` | `sort_by(xs, f, descending: true)` |
| `for i, x in enumerate(xs)` | `enumerate(xs) ¨ (p => …)` |
| `zip(a, b)` | `zip(a, b)` |
| `{"a": 1}`, `d.get(k)` | `["a": 1]`, `get(d, k)` (an `Option`) |
| `f"{x:.2f}"` | `"{x:.2}"` |
| `None` | `None`, inside an `Option` |
| `raise` / `try` | return `Err(…)` / `match` or `let … else` |
| `int(s) if s.isdigit() else 0` | `parse_int(s) else { 0 }` |
| `class Point:` | `type Point = { x: f64, y: f64 }` |
| `def f(x, size=16):` | `fn f(x, size: i64 = 16)` |

## Comprehensions become array operations

```mimic run
module main

fn main() {
    let xs = [5 2 8 1 9]
    print(xs ¨ (x => x * x))
    print(xs ⌿ (xs > 2))
    print("sum {+/ xs}, largest {⌈/ xs}, length {≢ xs}")
    print(⍳ 5)
    print(2 ‥ 5)
    print(xs ↓ 1 ↑ 3)
}
```

```output
[25 4 64 1 81]
[5 8 9]
sum 25, largest 9, length 5
[0 1 2 3 4]
[2 3 4]
[2 8 1]
```

Each line is one operation on the whole list, and the compiler spreads it over your cores. There are no `for` or `while` loops: `mimic man no-loops` shows what replaces them.

## Built-ins you know

```mimic run
module main

fn main() {
    let words = ["pear" "fig" "banana" "kiwi"]
    print(sort(words))
    print(sort_by(words, w => ≢ w, descending: true))
    print(max_by(words, w => ≢ w))
    print(enumerate(["a" "b"]))
    print(zip([1 2 3], ["x" "y" "z"]))
    print(any([1 5 9] > 8))
    print(all([1 5 9] > 0))
    print(unique([3 1 3 2 1]))
    print(join(["a" "b" "c"], "-"))
}
```

```output
["banana" "fig" "kiwi" "pear"]
["banana" "kiwi" "pear" "fig"]
Some("banana")
[(0, "a") (1, "b")]
[(1, "x") (2, "y") (3, "z")]
true
true
[3 1 2]
a-b-c
```

## Dicts are maps

```mimic run
module main

fn main() {
    let ages = ["ada": 36, "alan": 41]
    print(ages["ada"])
    print(get(ages, "grace"))
    print(has(ages, "alan"))
    print(keys(ages))
    let mut more = ages
    more["grace"] = 85
    print("{≢ ages} {≢ more}")      // `ages` is unchanged
}
```

```output
36
None
true
["ada" "alan"]
2 3
```

## f-strings

Interpolation is always on in Mimic strings, with Python's format specifications:

```mimic run
module main

fn main() {
    let name = "Ada"
    let score = 0.9375
    print("{name} scored {score:.1%}")
    print("[{name:>6}] [{42:05}] [{255:#x}]")
}
```

```output
Ada scored 93.8%
[   Ada] [00042] [0xff]
```

## None and exceptions

There is no `None` hiding inside every type, and no exceptions. A function that may have no answer returns an `Option`; one that may fail returns a `Result`, and the caller must deal with both cases before using the value:

```mimic run
module main

fn average(xs: [f64]) -> Option[f64] {
    if ≢ xs == 0 { None } else { Some((+/ xs) ÷ ≢ xs) }
}

fn parse_age(text: str) -> Result[i64, str] {
    let n = parse_int(text) else { return Err("not a number: {text}") }
    if n < 0 { Err("negative: {n}") } else { Ok(n) }
}

fn main() {
    print(average([1.0 2.0 4.5]))
    print(average([]))
    match parse_age("abc") {
        Ok(n) => print("age {n}"),
        Err(why) => print("error: {why}"),
    }
}
```

```output
Some(2.5)
None
error: not a number: abc
```

## Types

Most types are inferred, as in Python, but they are checked before the program runs: a misspelled field or a string passed where a number is expected is an error from `mimic check`, not a crash in production. Annotate function parameters when you want to (`x: f64`), and always for records.

## Speed

A pure function (one that does not print, read files or call C) given a whole array runs on every core, with SIMD instructions where the CPU has them. You never write threads or `multiprocessing`: `mimic man parallel-by-default`.
