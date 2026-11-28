# Mimic syntax

Every primitive and every piece of syntax has a **glyph** and a **word**, and the two can be mixed freely. The lexer turns both into the same tokens, so these lines are identical programs:

```mimic
print(⍳ 10 ⌽ ↑ 3 ▷ +/)
print(iota 10 |> reverse |> take(3) |> sum)
```

Run `mimic glyphs` for the live table. To type glyphs on an ASCII keyboard, use backslash digraphs: `\i` → `⍳`, `\>` → `▷`, and so on.

## Files and modules

* **Header.** The first line of every `.mim` file is `module <name>` or `§ <name>`. Prefix it with `pub` to export the module from a library.
* **Contents.** A file can hold more modules. Nested modules use braces: `module inner { … }`.
* **Paths:**
  * `project::module::item`, e.g. `ai::speech::get_mic`.
  * Modules of the same project can be named directly (`speech::get_mic`).
  * `self::` and `super::` work too.
* **Libraries.** A library's public API is its `module lib`. The items in it are reachable as `libname::item`.
* **Imports.** Dependencies declared in `mim.mim` are usable from every file without an import. `use a::b::c` and `use a::b as x` are optional conveniences. Wildcard imports do not exist.

### Tests

`test "name" { … }` declares a test, next to the code it tests or in `src/test.mim`. Inside one, `expect(condition)` checks a condition; when `expect(a == b)` (or `!=`, `<`, `<=`, `>`, `>=`) fails, both sides are shown, and the test goes on so that one run shows every failure. `mimic test` runs the tests; builds never include them. A test can use the private items of its module. `test` is a keyword only before a test's name, so a variable can still be called `test`.

```mimic
fn square(x: i64) -> i64 { x * x }

test "square" {
    expect(square(3) == 9)
    expect(⍳ 4 ¨ square ▷ +/ == 14)
}
```

**Property tests** take parameters after `for`: the body runs for 100 generated inputs (`i64`, `f64`, `bool`, `str`, and lists of them), the same ones on every run. When one fails, its inputs are shrunk, one parameter at a time, to the smallest that still fails, and the failure shows that case.

```mimic
test "sorting keeps the length" for xs: [i64] {
    expect(≢ sort(xs) == ≢ xs)
}
```

An example in a comment is a test too: a `// > expression` line, then the value `print` would show on the next comment line.

### Contracts

`requires` and `ensures` lines between a function's signature and its body state what it needs and what it promises; in `ensures`, `result` is the value returned. Debug builds and tests check them on every call (also when the body leaves with `return`) and stop with the condition, the function and the line when one is broken; release builds leave them out, at no cost. They are part of the signature that hovers, `mimic doc` and `mimic man` show. `requires` and `ensures` are keywords only in that place.

```mimic
fn isqrt(n: i64) -> i64
    requires n >= 0
    ensures result * result <= n and (result + 1) * (result + 1) > n
{
    if n < 2 { return n }
    let r = isqrt(n div 4) * 2
    if (r + 1) * (r + 1) <= n { r + 1 } else { r }
}
```

### Visibility and mutability

* **Everything is private** except `fn main` inside `module main`. Mark items `pub` to export them.
  * Private items are visible inside their own module and its sub-modules.
  * Items in other packages must be `pub`.
* **Every binding is immutable.** `let mut x = …` (or `mut x ← …`) opts in to mutation. Parameters are always immutable.
* **No shadowing.** You cannot re-declare a name in the same function. You cannot give a variable the name of a function. A nested module cannot redefine a function of an enclosing module.

## Statements

| Word form | Glyph form | Meaning |
|---|---|---|
| `let x = e` | `x ← e` | immutable binding |
| `let mut x = e` | `mut x ← e` | mutable binding |
| `x = e` | `x ↩ e` | reassign (`mut` only) |
| `xs[i] = v` | `xs[i] ↩ v` | copy-on-write element store |
| `x += e` | `x +↩ e` | `x = x + e`; also `-=` `×=` `÷=` (`-↩` `×↩` `÷↩`, ASCII `*=` `/=`), on a `mut` name or `xs[i]` (the index computed once) |
| `return e` | | early return |
| `for x in a ‥ b { … }`, `for x in xs { … }` | `∀ x in … { … }` | a loop over a range (`b` left out) or a list's elements |
| `while cond { … }` | `⟳ cond { … }` | a loop while `cond` holds |
| `break` | `↯` | leaves the innermost loop (E0216 outside one, or inside a lambda) |

A loop's body may reassign only its function's `let mut` variables; loops are sequential (parallel work is `¨` and the array primitives). `for` is a `while` over a hidden counter: the range's bounds, or the list and its length, are taken once, before the loop. Each bound is a whole expression: `for j in 1 ‥ n + 1` counts up to `n + 1` (as an expression, `1 ‥ n + 1` is `(1 ‥ n) + 1`, the range binding tighter; in a `for`, parentheses keep that meaning).

Statements end at a newline or `;`. A block's last expression is its value. A pipeline may continue on the next line if that line starts with `▷`/`|>`.

## Expressions, loosest to tightest

| Level | Forms |
|---|---|
| pipeline | `a ▷ f`, `a ▷ f(b)` = `f(a, b)`, `a ▷ ↑ 3`, `a ▷ (x ⇒ …)` |
| or / and | `∨` `or`, `∧` `and` (elementwise) |
| compare | `==` `≠`/`!=` `<` `≤`/`<=` `>` `≥`/`>=` (not chainable) |
| bitwise or | `\|` |
| bitwise xor | `^` |
| bitwise and | `&` |
| shift | `<<` `>>` (arithmetic) `>>>` (logical) |
| additive | `+` `-` |
| multiplicative | `×`/`*` `÷`/`/` (always f64) `div`/`⫽` (floored integer division) `%` (floored mod) |
| unary | `-x`, `¬x`/`not x`, `~x` (bitwise complement) |
| **train** | postfix monadic prims `xs ⌽`, infix dyadic prims `xs ↑ 3`, `xs ¨ f` |
| prefix | `⍳ 10`, `⌽ ⍳ 5`, call form `take(xs, 3)` |
| postfix | `f(x)`, `xs[i]` (no space before `(` / `[`) |
| primary | `1` `¯2` `2.5` `"s"` `true` `[1 2 3]` `(e)` `if` `{ }` `unsafe { }` `x ⇒ e` |

**Trains read left to right.** `⍳ 10 ⌽ ↑ 3 +/` means "iota 10, reversed, take 3, sum", which is 24. A prefix primitive binds to the next operand only, so `⍳ 5 + 1` is `(⍳ 5) + 1`. After a pipeline stage, reading continues left to right: `xs ▷ +/ ÷ 2` is `(sum xs) ÷ 2`.

**Literals.** `⍬` (digraph `\z`) is an empty list of `i64`, and `⍬[f64]`, `⍬[str]`, `⍬[Point]` an empty list of that element type. A bare `[]` has no element type of its own and takes one from where it is used; where nothing gives it one (a loop's first state, a list grown from it), it is a list of unknown type, read the slow, generic way, so write `⍬` or `⍬[T]` there. `` `a` `` is a character's code, a number (97), as `char_code("a")` gives, with the escapes `` `\n` ``, `` `\t` ``, `` `\`` ``, `` `\x1b` `` and `` `\u{e9}` ``. Work on bytes compares and computes with it, `` c >= `a` and c <= `z` ``, with no one-letter string made. Inside `[ ]`, elements are separated by spaces or commas. A `-` with a space before it and a digit right after starts a negative element, so `[256 -1 255]` is three numbers; `[a - 1]` and `[a-1]` subtract, as does `xs[n -1]` (an index, not a list) and anything in parentheses inside the list. `¯3` is a negative number everywhere.

**Integer literals** can be written in hexadecimal `0xFF`, binary `0b1010` or octal `0o17`, with `_` separators anywhere: `1_000_000`, `0xFFFF_0000`. A hex/binary/octal literal may use all 64 bits, so `0xFFFF_FFFF_FFFF_FFFF` is `-1`.

**Bitwise operators** work on integers, elementwise on arrays, and in fused kernels, nets and `¨` like every other operator:
* **Where they bind.** They bind tighter than comparisons, unlike C, so `x & mask == 0` means `(x & mask) == 0`. They bind looser than `+` and `*`, so `1 + 2 << 3` is `24`.
* **Shifts.** `>>` is arithmetic and keeps the sign: `¯16 >> 2` is `-4`. `>>>` is logical and fills with zeros. A shift amount uses its low 6 bits, exactly as the hardware does, so every shift is a single instruction and `x << 64` is `x`.
* **Bools.** `& | ^ ~` also work on bools, without short-circuiting, which is handy for masks: `(xs > 2) & (xs < 6)`. `and` and `or` short-circuit on single truth values (the right side runs only when it decides) and work element by element on lists.
* **Floats and text** are a compile error (E0407).

**Loops are words.** Mimic has no loop statements. These words, and recursion, replace them:

```mimic
prices ▷ gather(+)                         // combine: 31   (∕ glyph, `\g`)
prices ▷ trail(+)                          // running totals (⍀ glyph, `\t`)
1 ▷ evolve(10, x => x × 2)                 // apply 10 times: 1024 (⍣)
1.0 ▷ settle(newton, x => ∣(x × x - 2) < 1e-12)   // until done (⍥)
arms (lo = 0, hi = 1000000) {              // divide and conquer (⋔), in parallel
    if hi - lo < 1000 { lo ‥ hi ▷ gather(+) }
    else {
        let mid = (lo + hi) div 2
        arm(lo, mid) + arm(mid, hi)
    }
}
```

**How each word works:**
* **`gather` and `trail`** combine with any two-argument function. When the function is pure they run in parallel, so it must be associative; `gather(+)` and `gather(×)` become the fused `+/` and `×/`.
* **`evolve` and `settle`** step a state. `settle` checks `done` before each step.
* **`arms`** binds its parameters, and `arm(…)` recurses with new values. The body captures outer variables by value.

**Functions are values.** Lambdas, named functions and operators can be stored, passed, returned and called:

```mimic
let k = 10
let add_k = x => x + k                 // a lambda capturing k (by value)
let mul = (a, b) => a * b              // several parameters; `() => …` for none
let area = (w: f64, h: f64) => w * h   // parameters may carry types, checked as a function's
apply(double, 21)                      // a named function as a value
fold(xs, (+), 0)                       // an operator as a value: (+) (*) (==) …
[1 2] ¨ Some                           // a variant constructor as a value: x => Some(x)
fn make_adder(n: i64) { x => x + n }   // returning a closure
make_adder(100)(1)                     // calling a call's result
xs ¨ add_k                             // ¨ with any function value
fn apply(f: fn(i64) -> i64, x: i64) -> i64 { f(x) }   // a function type
```

**How they behave:**
* **Constructors.** A variant with fields, written without parentheses, is the function that builds it: `Typed` means `x => Typed(x)`, and `Moved` (two fields) means `(x, y) => Moved(x, y)`.
* **Purity follows the functions passed in.** A function that calls a function parameter is as pure as the function given to it. `fn twice(f, x) { f(f(x)) }` called with a pure lambda runs in parallel like any pure call. Called with a function that prints, the call counts as printing.
* **Functions kept in data are pure.** A function stored in a record, variant, list or map must have no side effects (error E0605), so code that takes it out may call it freely.
* **Standard functions are values too.** `xs ¨ math::sqrt` passes the function itself, the same as `xs ¨ (x => math::sqrt(x))`. C functions must be wrapped in a Mimic function first.
* **Arity is checked.** Passing a function, lambda or constructor that takes the wrong number of arguments for a parameter's function type is a compile-time error.
* **Capture.** Lambdas capture the variables they use, by value, when they are created. Values are immutable, so a closure always sees the value it was created with.
* **No mutation.** A lambda may not modify a captured variable (error E0603).
* **Types.** A function type `fn(i64) -> i64` is checked as "a function" when the program runs.
* **Parallelism.** `¨` runs a function value on all cores when the function is pure, which the compiler records when it creates the value.
* **Speed.** Lambdas that capture nothing and are called on the spot, or passed to `¨`, compile to direct typed calls. Calls through function values work on boxed values; typed fast paths for them are part of the performance phase.

## Primitives (M1)

| Glyph | Word | Arity | Meaning |
|---|---|---|---|
| `⍳` | `iota` | 1 | `⍳ n` → `[0 … n-1]` |
| `⍴` | `shape` | 1 | shape (`[]` for scalars) |
| `⌽` | `reverse` | 1 | reverse |
| `+/` | `sum` | 1 | sum |
| `×/` | `product` | 1 | product |
| `⌈/` | `maximum` | 1 | maximum |
| `⌊/` | `minimum` | 1 | minimum |
| `∧/` | `all` | 1 | all true (and reduction) |
| `∨/` | `any` | 1 | any true (or reduction) |
| `≢` | `len` | 1 | length |
| `⎕` | `print` | 1 | print with newline |
| `↑` | `take` | 2 | first n (last n if negative) |
| `↓` | `drop` | 2 | drop first n (last n if negative) |
| `⌿` | `keep` | 2 | replicate / filter by mask |
| `¨` | `each` | 2 | map a function over elements |
| `‥` | `range` | 2 | `lo ‥ hi` → `[lo … hi-1]` |
| `⌈` | `max` | 2 | larger of two values (elementwise); `(max)` / `gather(max)` as a function value |
| `⌊` | `min` | 2 | smaller of two values (elementwise) |
| `∣` | `abs` | 1 | absolute value (elementwise) |
| `⥊` | `reshape` | 2 | `xs ⥊ [2 3]` → a 2×3 matrix of the elements of `xs`, repeated to fill |
| `⍉` | `transpose` | 1 | reverse the axes: rows ↔ columns |
| `⊠` | `matmul` | 2 | matrix product; a vector acts as a row (left) or a column (right) |

`⌈` and `⌊` on their own are max and min. Followed by `/` (`⌈/`, `⌊/`), they are the reductions. Likewise `∧` and `∨` are and and or, and `∧/` and `∨/` reduce a list of booleans (the glyph spellings of `all` and `any`).

Arithmetic and comparison broadcast a scalar against a vector, and work elementwise on two vectors of equal length.

### Arrays of rank 2 and more

```mimic
let m = ⍳ 6 ⥊ [2 3]        // [[0 1 2]
                           //  [3 4 5]]
⍴ m                        // [2 3]
≢ m                        // 2: the leading axis (rows)
m[1]                       // [3 4 5]: a row
m[1, 2]                    // 5
m + 10, m * m, m > 2       // elementwise, same shapes (or a scalar)
m ▷ +/                     // [3 12]: reductions work along each row
⍉ m ▷ +/                   // [3 5 7]: column sums
m ⊠ ⍉ m                    // matrix product
m ↑ 1, ⌽ m                 // take / drop / reverse act on rows
m ¨ f                      // keeps the shape
let mut g = 0 ⥊ [3 3]
g[1, 1] = 5                // one index per dimension: an element
g[0] = [7 8 9]             // fewer: a whole row (a number fills it)
```

How matrices behave:
* **Storage.** Elements are stored flat in row-major order, so elementwise operations are the same fused, parallel loops as for vectors.
* **Shapes must match.** Operands of an elementwise operation must have the same shape, or one of them must be a scalar. A mismatch stops the program with `shape mismatch: [2 3] vs [3 2]`.
* **Primitives bind tightly.** Like every dyadic primitive, `⥊` binds tighter than arithmetic: `(⍳ 6) * 2 ⥊ [2 3]` reshapes `2`. Use parentheses, or the pipe, which binds loosest: `(⍳ 6) * 2 ▷ reshape([2 3])`.
* **Matrix product speed.** `⊠` on floats uses the platform BLAS where one exists (Apple's Accelerate, which uses the matrix units, or OpenBLAS on Linux). Otherwise it uses Mimic's own packed, register-blocked kernel on all cores.
* **List functions refuse matrices.** `push`, `concat`, `sort`, `gather` and `trail` work on lists and stop on a matrix. Reshape it first.
* **Typing.** Matrices are checked at run time for now. A matrix passed to a `[f64]` parameter is a type error, because `[f64]` means a vector.

**Parallelism is automatic.** Nothing needs to be declared:
* Primitives over large arrays run on all cores.
* `xs ¨ f` runs in parallel whenever `f` is pure, meaning it never reaches `print` or FFI.
* Pure recursive functions compile to interaction nets, so independent calls run on different cores. In `fib(n - 1) + fib(n - 2)`, both calls proceed at once.

**Division:** `÷` always produces `f64`. `div` (glyph `⫽`) is integer division, floored to match `%`, so `(a div b) × b + a % b == a` always holds:
* `7 div 2` is 3;
* `¯7 div 2` is ¯4.

**Loops** are replaced by words; the plan is in [roadmap.md](roadmap.md#loops-are-replaced-by-words).

## Keyword glyphs

Every keyword has a glyph, and every glyph a digraph (`\U` types `⇑`). Words and glyphs mix freely; `mimic fmt` writes them all as words, and `mimic glyphs` lists them.

| Glyph | Word | Digraph | Glyph | Word | Digraph |
|---|---|---|---|---|---|
| `§` | `module` | `\S` | `⊡` | `type` | `\Y` |
| `ƒ` | `fn` | `\f` | `⊨` | `match` | `\K` |
| `⇑` | `pub` | `\U` | `⊕` | `with` | `\w` |
| `⇄` | `mut` | `\D` | `⇐` | `use` | `\I` |
| `←` | `let … =` | `\<` | `≔` | `as` | `\A` |
| `↩` | `=` (reassign) | `\=` | `⌁` | `extern` | `\E` |
| `⍰` | `if` | `\?` | `☡` | `unsafe` | `\W` |
| `⋄` | `else` | `\:` | `✓` | `trusted` | `\V` |
| `⏎` | `return` | `\L` | `⎘` | `include` | `\C` |
| `●` | `true` | `\1` | `⊢` | `test` | `\H` |
| `○` | `false` | `\0` | `∀` | `for` (loops, property tests) | `\F` |
| `⟳` | `while` | `\O` | `↯` | `break` | `\B` |
| `▷` | `\|>` | `\>` | `→` | `->` | |
| `⇒` | `=>` | | `⍝` | `//` comment | |

```mimic
⊡ Shape = Circle(f64) | Rect(f64, f64)

⇑ ƒ area(s: Shape) → f64 {
    ⊨ s {
        Circle(r) ⇒ 3.14159 × r × r,
        Rect(w, h) ⇒ w × h,
    }
}

ƒ clamp(x: i64) → i64 {
    ⍰ x < 0 { ⏎ 0 }
    ⍰ x > 255 { 255 } ⋄ { x }
}
```

## Records, unions and `match`

**Naming rule:** type and variant names start with an **uppercase** letter, and variables and functions start with lowercase. That one rule is what keeps these apart:
* `Inc` (a variant) from `inc` (a variable);
* `User { … }` (a record) from `if ready { … }` (a block).

```mimic
type User = { name: str, age: i64 }

type Shape =
    | Circle(f64)
    | Rect(f64, f64)
    | Dot

let u = User { name: "ada", age: 36 }      // every field, in any order
let older = { u with age: u.age + 1 }      // a new record; `u` is unchanged
let moved = { u with home.city: "Paris" }  // reaches into nested records…
let done = { s with todos[i].done: true }  // …and into list elements
print(u.name)

fn area(s: Shape) -> f64 {
    match s {
        Circle(r)  => 3.14159 × r × r
        Rect(w, h) => w × h
        Dot        => 0.0
    }
}
```

* **Matching on a union must be exhaustive.** Every variant needs an arm, or there must be a final `_ =>`. The compiler names any missing variants.
* **Matching on numbers or strings needs a final `_ =>` arm.** Arrays are covered when the arms handle every length, as `[]`, `[x]`, `[x, ..rest]` do.
* **An arm with a guard covers nothing**, because the guard may be false.

**Patterns:**

| Pattern | Matches |
|---|---|
| `_` | anything |
| `n` (lowercase name) | anything, and binds it |
| `1`, `-2.5`, `"ada"`, `true` | that literal |
| `0 ‥ 13` | a number with `0 ≤ x < 13`, like the `‥` primitive |
| `Rect(w, _)` | a variant, with sub-patterns for its fields |
| `Point { x: 0, y }`, `Point { x, .. }` | a record, by field name; `..` ignores the fields not listed |
| `(a, 0)` | a tuple |
| `[]`, `[a b]`, `[first, ..rest]`, `[x, ..]` | an array or string of that length; `..rest` binds the remaining elements |
| `A \| B` | either alternative |
| `p as name` | `p`, and also binds the whole value to `name` |
| `"/users/{id: i64}"` | a route (see below) |

```mimic
match event {
    Click(Point { x: 0, y: 0 })            => "origin"
    Click(Point { x, .. } as p) if x > 2   => "right: {p}"     // guard
    Key("q" | "Q")                         => "quit"           // alternatives
    Scroll(n) as s if n < 0                => "{s} up"
    Circle(r) | Square(r)                  => "r = {r}"
    _                                      => "other"
}

if let Some(year) = get(born, "ada") { print(year) } else { print("unknown") }
```

**Rules for patterns:**
* **Guards.** `pat if cond =>` tries the arm only when `cond` is true, and `cond` can use the pattern's variables. When the guard is false, matching continues with the next arm.
* **Alternatives** must all bind the same names (E0505). Inside a larger pattern, alternatives cannot bind names yet.
* **`if let pat = value { … } else { … }`** is a two-arm `match`, and `else if` / `else if let` chain as usual.
* **Strings compare as whole values** with `==` and `!=`.
* **Records, variants and tuples compare structurally** with `==` and `!=`. Two values are equal when they have the same type (and variant) and equal fields, compared deeply through nested records, strings, lists and maps. `parse_int(s) == Some(42)` and `find(s, "x") == None` just work.
  * **Same type only.** Comparing values of two different types is a compile error (E0401), because the result would always be false.
  * **Lists compare elementwise.** `points == origin` gives a list of bools, the same as for numbers.
  * **What counts as equal.** Numbers compare by value, so `1 == 1.0`. Maps are equal when they have the same keys and values, in any order. Functions are equal only to themselves.
* **Under the hood:** records and variants are immutable, reference-counted values. `with` updates in place when nothing else shares the record, and copies otherwise.

## Strings

**Interpolation:** `"hi {u.name}, next year {u.age + 1}"`. Any expression can go inside `{ }`, and it is formatted the way `print` shows it. Write `\{` for a literal brace.

**Escapes:** `\n` `\t` `\r` `\0` `\\` `\"` `\{` `\}`; `\x1b` for an ASCII character by two hex digits (00 to 7f); `\u{1F600}` for any Unicode character by its code point (1 to 6 hex digits). There are no octal escapes: `\033` is an error that suggests `\x1b`. Anything else after `\` is an error (E0105).

**Raw strings** use single quotes: `'{"name": "Ada", "path": "C:\\dir"}'`. They have no interpolation and no escapes, which suits JSON, regular expressions and Windows paths. They end at the next `'` and cannot span lines.

## Tuples

```mimic
fn divmod(a: i64, b: i64) { (a div b, a % b) }

let (q, r) = divmod(17, 5)     // `_` skips an element
let pair = (3, "three")
print(pair.0)

match point {
    (0, _) => "on the y axis"
    (_, 0) => "on the x axis"
    _      => "elsewhere"
}
```

Tuple types are tracked statically: `(i64, str)` keeps its `i64` unboxed. They can be written in annotations like any other type: `fn divmod(a: i64, b: i64) -> (i64, i64)`, `[(str, i64)]`, `fn swap[A, B](p: (A, B)) -> (B, A)`.

## Errors without exceptions: `Option`, `Result` and `let … else`

The prelude, available everywhere, defines `type Option[T] = Some(T) | None` and `type Result[T, E] = Ok(T) | Err(E)` (see [Generics](#generics)).

```mimic
fn name_of(id: i64) -> str {
    let u: User = find(id) else { return "nobody" }   // unwraps Some / Ok
    u.name
}
```

* The `else` block runs on `None` / `Err`. It either leaves (`return`), or gives a value of the type inside, which is then used instead: `let port = parse_int(s) else { 8080 }`.
* `e else { … }` is an expression, so it also works inside others: `if 1 < ≢ args { parse_int(args[1]) else { 1086 } } else { 1086 }`, `xs ¨ (s => parse_int(s) else { 0 })`. It binds loosest, so write `(parse_int(s) else { 0 }) + 1` to add to the result.
* The unwrapped value keeps its static type. For example, `let n = parse_int(s) else { … }` gives an `i64`.
* Errors: `else` after a value that is not an `Option` or `Result` (E0406), a block giving another type (E0401), a block giving nothing (E0504).

## Concurrency: implicit futures

Mimic has no `async`, no `await` and no threads to manage. Effects come in three kinds:
* **Overlapping effects** may happen at the same time as each other: reading files, listing directories, reading the environment, the clocks, `sleep`, HTTP requests, and serving HTTP with a handler that is itself overlapping.
* **Ordered effects** happen in program order on the main thread: printing, standard input, random numbers, TCP sockets, `exit`.
* **Synchronizing effects** change what overlapping effects could read: writing, renaming or removing files, changing the environment, running programs.

The compiler works out which kind every function has, transitively, like purity. For C functions, which it cannot see inside, the `extern` block says so (`extern "C" concurrent { … }`, see [ffi.md](ffi.md#what-c-functions-do-effects)). The rules:

* **A `let` starts work early.** When `let x = f(…)` calls a function whose effects all overlap, it starts at once on its own thread. The program goes on, and waits only when the value is needed.
* **When the program waits.** It waits right before the first statement that uses `x`, before any synchronizing effect, at every `return`, and at the end of the block. The arguments are evaluated first, in order. Printing does not wait, so a program can print while its own server, running as a future, keeps answering.
* **`¨` runs overlapping functions in parallel**, just as it does pure ones. Results stay in order. A function that returns nothing (it is run only for its effects) makes `xs ¨ f` return nothing too.
* **Tasks talk through channels** (`chan::new[T]()`, `chan::send`, `chan::receive`). A long-running `let t = worker(rx)` is a task, and Mimic has no other kind of shared state between tasks.

```mimic
let a = fetch_page(url1)        // both start now
let b = fetch_page(url2)
let words = count(text)         // pure work runs meanwhile
print(a + b)                    // waits here for both

let pages = urls ¨ fetch_page   // all at once
```

**The program behaves as if it ran in order.** Because synchronizing effects wait for everything started before them, a write can never overtake an earlier read of the same file, and output never interleaves, because futures never print. The only things that may reorder are overlapping effects among themselves, and their results are the same in any order. On platforms without threads (WebAssembly), futures simply run where they are written.

## Durations

`2s`, `500ms`, `1.5min`, `10us`, `3h` are `i64` nanoseconds, so `250ms + 1s` and `timeout div 1s` just work.

## Route patterns

A string pattern can contain typed captures, which is how a server routes requests:

```mimic
fn route(path: str) -> str {
    match path {
        "/"                             => "home"
        "/users/{id: i64}"              => "user #{id}"
        "/users/{id: i64}/posts/{slug}" => "post {slug} by #{id}"
        _                               => "404"
    }
}
```

* Captures are `str` (the default), `i64` or `f64`.
* A capture is non-empty, never spans a `/`, and must parse as its type. `"/users/abc"` does not match `{id: i64}`.
* Two captures need text between them.

## Named and default arguments

```mimic
fn label(text: str, size: i64 = 16) -> str { "{text}@{size}" }

label("x")                  // size = 16
label("x", size: 24)
label(size: 9, text: "y")   // named arguments in any order
```

* Named arguments take their parameters; positional arguments fill the remaining parameters in order, wherever they are written: `column(gap: 12, [a, b])` gives the list to the first free parameter.
* Defaults must be constants: literals, operators on literals, arrays of literals, or unit variants.

## Lists of anything

Array literals can hold strings, records or other arrays: `["ada" "bob"]`, `[u, older]`. Indexing and `¨` work on them as usual, and their element types are tracked statically, so `users[0].age` is checked at compile time.

## Types (annotations)

The annotation types are:
* numbers: `i64`, `f64`, `i32`, `f32`, `bool` (and C's `i8`, `i16`, `u8 … u64`, `isize`, `usize`, which are `i64` in Mimic code);
* `str`;
* arrays: `[i64]`, `[f64]`, `[bool]`, `[str]`, `[Point]`, and the compact `[f32]`, `[i32]`, `[u8]`;
* records and unions by name, and generic ones with type arguments: `Option[i64]`, `Pair[str, f64]`, `[Tree[str]]`;
* tuples: `(i64, str)`;
* maps: `[str: i64]`;
* types from other modules by path: `http::Response`, `time::DateTime`;
* function types: `fn(i64) -> i64`.

### 32-bit numbers: `f32` and `i32`

`f32` and `i32` (and lists of them, `[f32]`, `[i32]`) take 4 bytes a number instead of 8. They are real types: `f32` arithmetic rounds every result to an f32, and `i32` arithmetic wraps around, as in C. An annotation converts: `let xs: [f32] = ⍳ 5 * 0.1` rounds each number to an f32, and a value an `i32` cannot hold stops the program.

* **Mixing.** Floats beat integers. Between two widths of the same kind, a list keeps its own type against a single value: `xs * 0.5` on an `[f32]` is an `[f32]`, and `ys + 1` on an `[i32]` is an `[i32]`. Otherwise the wider type wins: an `f32` times an `f64` is an `f64`.
* **Division.** `/` of integers gives an `f64`, as always; `div` keeps an `i32`.
* **Sums.** `+/` of an `[f32]` adds in f32, and of an `[i32]` wraps.
* **Numbers written in the program** take the 32-bit type of what they meet: `z * 0.5` on an `f32` is `f32`, `n + 1` on an `i32` is `i32` (wrapping), and `if c { 0 } else { n }` is an `i32`. (A float literal next to an `i32` is still an `f64`.)
* **Multiply-add.** As with `f64`, `a * b + c` on `f32` may be one fused multiply-add (one rounding instead of two) unless the program is built with `--strict-math`.
* **The GPU.** Long maps over `[f32]`, `[i32]`, `[i64]` and `[bool]` with a lot of work per element, including `xs ¨ f` with a pure numeric `f`, can run on the GPU (macOS, Metal), with the results of `--strict-math`. See [design.md](design.md#gpu-kernels).
* **The other C widths** (`i8`, `i16`, `u16`, `u32`, `u64`, `isize`, `usize`) are still `i64` in Mimic code; they keep their meaning for C functions.

### Generics

Types and functions can take type parameters, written in `[ ]` after the name, the same brackets as array types. Type parameters start with an uppercase letter.

```mimic
type Pair[A, B] = { first: A, second: B }
type Tree[T] = Leaf | Node(Tree[T], T, Tree[T])

fn first[T](xs: [T]) -> Option[T] {
    match xs { [x, ..] => Some(x), _ => None }
}
fn swap[A, B](p: Pair[A, B]) -> Pair[B, A] { Pair { first: p.second, second: p.first } }

first([3 1 2])            // Some(3): an Option[i64]
first(["x" "y"])          // Some("x"): an Option[str]
```

* **Explicit type arguments.** A function whose result type cannot be inferred from its arguments takes its type arguments in `[…]` right after its name: `json::decode[User](text)`. Only the standard library declares such functions so far.
* **Type arguments are inferred.** You never write `first[i64](…)`. `Some(42)` is an `Option[i64]`, and `Pair { first: 1, second: "one" }` is a `Pair[i64, str]`. `None` and `Leaf` take their type arguments from where they are used.
* **Mismatches are compile errors.** For example, `let o: Option[str] = Some(3)` is an error (E0401). So is calling `or_else(Some(1), "s")` against `fn or_else[T](o: Option[T], d: T)`, because `T` would have to be both `i64` and `str`. The wrong number of type arguments is E0411.
* **Generic code runs as fast as hand-written code.** Every function is compiled once per combination of argument types it is called with, so `first` on `[i64]` and `first` on `[str]` are separate, fully typed functions. A value taken out of an `Option[i64]`, by `match`, `let … else` or `or_else`, is a plain machine integer.
* **Values look the same whatever their type arguments.** An `Option[i64]` and an `Option[str]` share one runtime layout, so generic types add no code size.
* **A generic type named without arguments means any instance.** `fn f(o: Option)` accepts every `Option`.
* **`Option[T]` and `Result[T, E]`** in the prelude are ordinary generic unions: `type Option[T] = Some(T) | None`.
* **`<`, `<=`, `>`, `>=` on two strings** compare them as whole values, in byte-wise lexicographic order, the same way `==` does. A sorted tree of strings therefore works with the same code as a tree of numbers.

In M1, annotations on parameters, return values and `let` are checked at runtime. A violation is a `type error` panic.
