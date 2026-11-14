---
title: Arrays in depth
description: Matrices and higher-rank arrays — reshape, transpose, indexing, row reductions and fast matrix products.
order: 15
---

# Arrays in depth

The arrays so far have had one dimension. Mimic arrays can have any number of dimensions: a **matrix** has two, rows and columns.

## Making a matrix

`⥊` (`reshape`) arranges the elements of an array into a new shape:

```mimic run
module main

fn main() {
    let m = ⍳ 6 ⥊ [2 3]
    print(m)
    print(⍴ m)                  // the shape
    print(≢ m)                  // the number of rows
    print([1 0] ⥊ [3 3])        // elements repeat to fill the shape
}
```

```output
[[0 1 2]
 [3 4 5]]
[2 3]
2
[[1 0 1]
 [0 1 0]
 [1 0 1]]
```

## Watch the precedence

Like every two-sided primitive (`↑`, `↓`, `⌿`), `⥊` binds **tighter** than arithmetic, so in `(⍳ 6) * 2 ⥊ [2 3]` it reshapes the `2`. Use parentheses, or the pipe, which binds loosest of all:

```mimic run
module main

fn main() {
    print(((⍳ 6) * 2) ⥊ [2 3])
    print((⍳ 6) * 2 ▷ reshape([2 3]))
}
```

```output
[[0 2 4]
 [6 8 10]]
[[0 2 4]
 [6 8 10]]
```

## Indexing

One index picks a row. One index per dimension picks an element:

```mimic run
module main

fn main() {
    let m = ⍳ 12 ⥊ [3 4]
    print(m[1])
    print(m[1, 2])
    print(m[2][3])

    let mut board = 0 ⥊ [3 3]
    board[1, 1] = 5
    board[0, 2] = 7
    print(board)
}
```

```output
[4 5 6 7]
6
11
[[0 0 7]
 [0 5 0]
 [0 0 0]]
```

## Whole-matrix operations

Every operator works elementwise, exactly as on vectors. Reductions work along each **row**:

```mimic run
module main

fn main() {
    let m = ⍳ 6 ⥊ [2 3]
    print(m * 10 + 1)
    print(m > 2)
    print(m ▷ +/)               // row sums
    print(⍉ m)                  // transpose: rows become columns
    print(⍉ m ▷ +/)             // column sums
    print(m ¨ (x => x * x))     // ¨ keeps the shape
    print(⌽ m)                  // reverse the rows
    print(m ↑ 1)                // the first row, still a matrix
}
```

```output
[[1 11 21]
 [31 41 51]]
[[false false false]
 [true true true]]
[3 12]
[[0 3]
 [1 4]
 [2 5]]
[3 5 7]
[[0 1 4]
 [9 16 25]]
[[3 4 5]
 [0 1 2]]
[[0 1 2]]
```

Shapes must match: adding a 2×3 matrix to a 3×2 one stops the program with `shape mismatch: [2 3] vs [3 2]`.

## Matrix product

`⊠` (`matmul`) is the matrix product. A vector on the left acts as a row, a vector on the right as a column, and two vectors give their dot product:

```mimic run
module main

fn main() {
    let a = [1 2 3 4] ⥊ [2 2]
    let b = [5 6 7 8] ⥊ [2 2]
    print(a ⊠ b)
    print([1 1] ⊠ a)            // a row vector times a matrix
    print([1 2 3] ⊠ [4 5 6])    // the dot product
}
```

```output
[[19 22]
 [43 50]]
[4 6]
32
```

`⊠` is built for speed:
* **Floats** go to the platform's BLAS when one is available. On Apple machines that means the Accelerate framework, which uses the chip's matrix units.
* **Otherwise**, and for integers, `⊠` uses Mimic's own cache-blocked kernel on all cores. Multiplying two 1024×1024 float matrices takes about 0.05 seconds on an M3 Pro, either way.

## Example: Fibonacci by matrix power

Raising `[[1 1] [1 0]]` to the *n*-th power gives Fibonacci numbers. Squaring repeatedly takes only about log₂ *n* products:

```mimic run
module main

fn main() {
    let q = [1 1 1 0] ⥊ [2 2]
    print(power(q, 10))
    print(power(q, 90)[0, 1])
}

fn power(m, n: i64) {
    if n == 1 { m }
    else if n % 2 == 0 { let h = power(m, n div 2); h ⊠ h }
    else { m ⊠ power(m, n - 1) }
}
```

```output
[[89 55]
 [55 34]]
2880067194370816120
```

## More dimensions

Nothing is special about two dimensions:

```mimic run
module main

fn main() {
    let cube = ⍳ 24 ⥊ [2 3 4]
    print(cube)
    print(⍴ cube)
    print(cube[1, 2])           // a row of the second block
    print(cube ▷ +/)            // sums along the last axis: a 2×3 matrix
}
```

```output
[[[0 1 2 3]
  [4 5 6 7]
  [8 9 10 11]]

 [[12 13 14 15]
  [16 17 18 19]
  [20 21 22 23]]]
[2 3 4]
[20 21 22 23]
[[6 22 38]
 [54 70 86]]
```

## Limits for now

* **Run-time checks.** Matrices are checked when the program runs, not when it compiles. A function parameter annotated `[f64]` means a vector, and passing a matrix to it is an error.
* **List functions refuse matrices.** `push`, `concat`, `sort`, `gather` and `trail` work on lists. Flatten a matrix into a vector first with `m ▷ reshape([count])`, where `count` is its number of elements (`⍴ m ▷ ×/`).

Back to the [index](index.md).
