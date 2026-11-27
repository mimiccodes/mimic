---
title: Errors without exceptions
description: Option, Result and let … else, the way Mimic handles missing values and failures.
order: 7
---

# Errors without exceptions

Mimic has no exceptions and no `null`. A value that may be missing is an `Option`. An operation that may fail returns a `Result`. Both are ordinary generic unions from the prelude:

```mimic
type Option[T] = Some(T) | None
type Result[T, E] = Ok(T) | Err(E)
```

## Option

```mimic run
module main

fn main() {
    print(first_even([1 3 6 8]))
    print(first_even([1 3]))
    match first_even([5 10]) {
        Some(n) => print("found {n}"),
        None => print("no even number"),
    }
}

fn first_even(xs: [i64]) -> Option[i64] {
    match xs ⌿ (xs % 2 == 0) {
        [e, ..] => Some(e),
        _ => None,
    }
}
```

```output
Some(6)
None
found 10
```

## Result

`Err` carries a reason:

```mimic run
module main

fn main() {
    print(withdraw(100, 30))
    print(withdraw(100, 300))
}

fn withdraw(balance: i64, amount: i64) -> Result[i64, str] {
    if amount > balance {
        Err("insufficient funds: {balance} < {amount}")
    } else {
        Ok(balance - amount)
    }
}
```

```output
Ok(70)
Err("insufficient funds: 100 < 300")
```

## let … else

`let x = value else { … }` unwraps `Some` or `Ok`. On `None` or `Err` it runs the `else` block, which must leave the function. This keeps the happy path flat:

```mimic run
module main

fn main() {
    print(sum_fields("3,4,5"))
    print(sum_fields("3,x,5"))
}

fn sum_fields(line: str) -> Result[i64, str] {
    let parts = split(line, ",")
    let a = parse_int(parts[0]) else { return Err("bad first field") }
    let b = parse_int(parts[1]) else { return Err("bad second field: {parts[1]}") }
    let c = parse_int(parts[2]) else { return Err("bad third field") }
    Ok(a + b + c)
}
```

```output
Ok(12)
Err("bad second field: x")
```

The unwrapped value keeps its type. In `sum_fields`, `a`, `b` and `c` are real `i64`s, so `a + b + c` is plain integer addition.

## A fallback value: `e else { … }`

When there is a sensible value to use instead, the `else` block can give it rather than leave. This works anywhere an expression does, not only in a `let`:

```mimic run
module main

fn main() {
    let width = parse_int("640") else { 800 }
    let height = parse_int("tall") else { 600 }
    print("{width}x{height}")
    let fields = ["1" "x" "3"]
    print(⍳ 3 ¨ (i => parse_int(fields[i]) else { 0 }) ▷ +/)
}
```

```output
640x600
4
```

The block's value must have the type of the value inside (an `i64` here); the compiler says so if it does not (E0401), and a block that gives nothing is E0504. It works the same on `Result`: `read_config() else { defaults() }`.

## Comparing

Options and Results compare like any other value:

```mimic run
module main

fn main() {
    print(parse_int("42") == Some(42))
    print(find("abc", "z") == None)
}
```

```output
true
true
```

## When something truly cannot happen

Some errors are bugs rather than expected outcomes: dividing by zero, indexing past the end, a missing map key. These stop the program with a clear message:

```text
mimic panic: index 5 out of bounds for length 3
```

Use `Option` and `Result` for things that can legitimately go wrong, like user input, files and the network. Leave panics for broken assumptions.

Next: [functions as values](08-functions-as-values.md).
