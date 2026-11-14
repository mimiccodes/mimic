---
title: Generics
description: Type parameters on types and functions, inferred everywhere and compiled to fully typed code.
order: 9
---

# Generics

A generic type or function works for any element type. Type parameters go in square brackets after the name, the same brackets array types use, and they start with an uppercase letter.

## Generic functions

```mimic run
module main

fn main() {
    print(first([3 1 2]))
    print(first(["x" "y"]))
    print(first([1 2] ↑ 0))           // an empty list
    print(or_else(first([5 6]), 0) + 1)
}

fn first[T](xs: [T]) -> Option[T] {
    match xs {
        [x, ..] => Some(x),
        _ => None,
    }
}

fn or_else[T](o: Option[T], fallback: T) -> T {
    match o {
        Some(x) => x,
        None => fallback,
    }
}
```

```output
Some(3)
Some("x")
None
6
```

You never write the type arguments. Mimic works them out from the values you pass.

## Generic types

```mimic run
module main

type Pair[A, B] = { first: A, second: B }

fn swap[A, B](p: Pair[A, B]) -> Pair[B, A] { Pair { first: p.second, second: p.first } }

fn main() {
    let p = Pair { first: 1, second: "one" }   // a Pair[i64, str]
    print(swap(p))
    print(p.first + 1)
}
```

```output
Pair { first: "one", second: 1 }
2
```

## A generic tree

A sorted binary tree that works for numbers and for strings alike:

```mimic run
module main

type Tree[T] = Leaf | Node(Tree[T], T, Tree[T])

fn insert[T](t: Tree[T], x: T) -> Tree[T] {
    match t {
        Leaf => Node(Leaf, x, Leaf),
        Node(l, v, r) if x < v => Node(insert(l, x), v, r),
        Node(l, v, r) if x > v => Node(l, v, insert(r, x)),
        _ => t,
    }
}

fn from_list[T](xs: [T], t: Tree[T]) -> Tree[T] {
    match xs {
        [] => t,
        [x, ..rest] => from_list(rest, insert(t, x)),
    }
}

fn to_list[T](t: Tree[T]) -> [T] {
    match t {
        Leaf => [],
        Node(l, v, r) => concat(push(to_list(l), v), to_list(r)),
    }
}

fn main() {
    print(to_list(from_list([5 3 8 1 4], Leaf)))
    print(to_list(from_list(["mimic" "ada" "zig"], Leaf)))
}
```

```output
[1 3 4 5 8]
["ada" "mimic" "zig"]
```

## Tuples in types

Tuple types are written like tuple values: `(i64, str)`. They work everywhere a type does, including generics and lists:

```mimic run
module main

fn divmod(a: i64, b: i64) -> (i64, i64) { (a div b, a % b) }

fn swap[A, B](p: (A, B)) -> (B, A) { (p.1, p.0) }

fn main() {
    print(divmod(17, 5))
    print(swap((1, "one")))
    let scores: [(str, i64)] = [("ada", 3), ("alan", 5)]
    print(scores ¨ (s => s.1) ▷ +/)
}
```

```output
(3, 2)
("one", 1)
8
```

## Type errors are caught at compile time

```mimic
let o: Option[str] = Some(3)
// error[E0401]: `let o`: expected Option[str], found Option[i64]

or_else(Some(1), "none")
// error[E0401]: type parameter `T` is i64 in one place and str in another

fn f(p: Pair[i64]) { … }
// error[E0411]: `Pair` takes 2 type argument(s), got 1
```

## No cost

* **Speed.** Mimic compiles a separate, fully typed copy of a function for each set of argument types it is used with. `or_else` on `Option[i64]` compiles to a function that returns a plain machine integer, exactly as if you had written it for `i64` by hand.
* **Size.** All instances of a generic type share one memory layout, so `Option[i64]` and `Option[str]` don't multiply your binary's size.

Next: [parallel by default](10-parallel-by-default.md).
