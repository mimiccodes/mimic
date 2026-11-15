---
title: Coming from C
description: C habits and their Mimic forms: loops, arrays, structs, NULL, errors, memory and calling C.
---

# Coming from C

Mimic compiles to native code like C, and runs as fast. Most of what changes is how you say things: whole arrays instead of loops, values instead of pointers, `Option` and `Result` instead of `NULL` and error codes.

| In C | In Mimic |
|---|---|
| `for (i = 0; i < n; i++) b[i] = a[i] * 2;` | `let b = a × 2` |
| `for (…) sum += a[i];` | `a ▷ +/` (or `sum(a)`) |
| a loop keeping the largest | `⌈/ a` (`maximum`) |
| a loop copying the elements that pass a test | `a ⌿ (a > 0)` (`keep`) |
| `memcpy(b, a + 2, 3 * sizeof *a)` | `a ↓ 2 ↑ 3` (drop 2, take 3) |
| `struct point { double x, y; };` | `type Point = { x: f64, y: f64 }` |
| `NULL` for "no result" | `Option[T]`: `Some(x)` or `None` |
| `return -1;` with `errno` | `Result[T, str]`: `Ok(x)` or `Err("why")` |
| `malloc` / `free` | nothing: memory is freed when the last user is done |
| `printf("%.2f\n", x)` | `print("{x:.2}")` |
| `a / b` on `int` | `a div b` (floored); `/` always gives `f64` |
| `pthread_create` | nothing: independent work runs in parallel by itself |

## Loops become array operations

```mimic run
module main

fn main() {
    let a = [3 ¯1 4 ¯1 5 9]
    print(a × 2)                 // every element doubled
    print(a ▷ +/)                // the sum
    print(⌈/ a)                  // the largest
    print(a ⌿ (a > 0))           // only the positive ones
    print(a ↓ 2 ↑ 3)             // three elements, from index 2
    print(≢ a)                   // the length
}
```

```output
[6 -2 8 -2 10 18]
19
9
[3 4 5 9]
[4 -1 5]
6
```

Elements of an array literal are separated by spaces, so a negative one is written with the high minus: `[3 ¯1 4]`. `[3 -1 4]` would mean `[(3 - 1) 4]`.

A loop that carries state from one step to the next becomes `evolve` (n times) or `settle` (until a condition holds), and anything else a recursive function. Tail calls are jumps, as `-O2` would make them in C. `mimic man no-loops` has them all.

## Structs are records

```mimic run
module main

type Point = { x: f64, y: f64 }

fn length(p: Point) -> f64 { math::sqrt(p.x × p.x + p.y × p.y) }

fn main() {
    let p = Point { x: 3.0, y: 4.0 }
    let q = { p with y: 0.0 }        // a changed copy; p stays as it is
    print(length(p))
    print(q)
}
```

```output
5.0
Point { x: 3.0, y: 0.0 }
```

Values never change unless declared `let mut`, and changing one never changes another: there is no aliasing. A copy is only made when something else still uses the old value.

## No NULL, no error codes

```mimic run
module main

fn find_index(xs: [i64], x: i64) -> Option[i64] {
    let at = ⍳ (≢ xs) ⌿ (xs == x)
    if ≢ at == 0 { None } else { Some(at[0]) }
}

fn parse_port(text: str) -> Result[i64, str] {
    let n = parse_int(text) else { return Err("not a number: {text}") }
    if n < 1 or n > 65535 { Err("out of range: {n}") } else { Ok(n) }
}

fn main() {
    print(find_index([5 7 9], 7))
    print(find_index([5 7 9], 4))
    print(parse_port("8080"))
    print(parse_port("80x"))
}
```

```output
Some(1)
None
Ok(8080)
Err("not a number: 80x")
```

The compiler makes you handle `None` and `Err` before you use the value inside (`match`, `let … else`), so the null-pointer crash and the unchecked error cannot happen.

## Memory

There is no `malloc`, `free` or garbage collector. Every value counts its users and is freed when the last one is done, as soon as that happens. Indexing out of bounds stops the program with a message instead of reading other memory. Debug builds (`mimic run`) check for leaks at exit.

## Calling your C code

C functions are written with `c::`, and calling one needs `unsafe { }` unless its block is declared `trusted`:

```mimic run
module main

trusted extern "C" pure include "math.h"

fn main() {
    print(c::cbrt(27.0))
    print(c::hypot(3.0, 4.0))
}
```

```output
3.0
5.0
```

Headers are imported as they are (`include "sqlite3.h"`), C structs become records, and Mimic functions can be exported to C (`extern "C" fn`). See `mimic man calling-c` and `mimic man ffi`.

## Threads

Write no threads. Pure functions (no printing, no files, no C calls) run on all cores when given whole arrays, `¨` maps run in parallel, and independent recursive calls such as `fib(n - 1) + fib(n - 2)` run at the same time. `mimic man parallel-by-default` explains what runs in parallel and why it is safe.
