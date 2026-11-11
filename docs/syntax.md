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
| `return e` | | early return |

Statements end at a newline or `;`. A block's last expression is its value. A pipeline may continue on the next line if that line starts with `▷`/`|>`.

## Expressions, loosest to tightest

| Level | Forms |
|---|---|
| pipeline | `a ▷ f`, `a ▷ f(b)` = `f(a, b)`, `a ▷ ↑ 3`, `a ▷ (x ⇒ …)` |
| or / and | `∨` `or`, `∧` `and` (elementwise) |
| compare | `==` `≠`/`!=` `<` `≤`/`<=` `>` `≥`/`>=` (not chainable) |
| additive | `+` `-` |
| multiplicative | `×`/`*` `÷`/`/` (always f64) `div`/`⫽` (floored integer division) `%` (floored mod) |
| unary | `-x`, `¬x`/`not x` |
| **train** | postfix monadic prims `xs ⌽`, infix dyadic prims `xs ↑ 3`, `xs ¨ f` |
| prefix | `⍳ 10`, `⌽ ⍳ 5`, call form `take(xs, 3)` |
| postfix | `f(x)`, `xs[i]` (no space before `(` / `[`) |
| primary | `1` `¯2` `2.5` `"s"` `true` `[1 2 3]` `(e)` `if` `{ }` `unsafe { }` `x ⇒ e` |

**Trains read left to right.** `⍳ 10 ⌽ ↑ 3 +/` means "iota 10, reversed, take 3, sum", which is 24. A prefix primitive binds to the next operand only, so `⍳ 5 + 1` is `(⍳ 5) + 1`. After a pipeline stage, reading continues left to right: `xs ▷ +/ ÷ 2` is `(sum xs) ÷ 2`.

**Literals.** Inside `[ ]`, elements are separated by spaces or commas. Use `¯3` or commas for negative elements, because `[1 -2]` parses as `[1 - 2]`.

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
apply(double, 21)                      // a named function as a value
fold(xs, (+), 0)                       // an operator as a value: (+) (*) (==) …
fn make_adder(n: i64) { x => x + n }   // returning a closure
make_adder(100)(1)                     // calling a call's result
xs ¨ add_k                             // ¨ with any function value
fn apply(f: fn(i64) -> i64, x: i64) -> i64 { f(x) }   // a function type
```

**How they behave:**
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
| `≢` | `len` | 1 | length |
| `⎕` | `print` | 1 | print with newline |
| `↑` | `take` | 2 | first n (last n if negative) |
| `↓` | `drop` | 2 | drop first n (last n if negative) |
| `⌿` | `keep` | 2 | replicate / filter by mask |
| `¨` | `each` | 2 | map a function over elements |
| `‥` | `range` | 2 | `lo ‥ hi` → `[lo … hi-1]` |
| `⌈` | `max` | 2 | larger of two values (elementwise) |
| `⌊` | `min` | 2 | smaller of two values (elementwise) |
| `∣` | `abs` | 1 | absolute value (elementwise) |

`⌈` and `⌊` on their own are max and min. Followed by `/` (`⌈/`, `⌊/`), they are the reductions.

Arithmetic and comparison broadcast a scalar against a vector, and work elementwise on two vectors of equal length.

**Parallelism is automatic.** Nothing needs to be declared:
* Primitives over large arrays run on all cores.
* `xs ¨ f` runs in parallel whenever `f` is pure, meaning it never reaches `print` or FFI.
* Pure recursive functions compile to interaction nets, so independent calls run on different cores. In `fib(n - 1) + fib(n - 2)`, both calls proceed at once.

**Division:** `÷` always produces `f64`. `div` (glyph `⫽`) is integer division, floored to match `%`, so `(a div b) × b + a % b == a` always holds:
* `7 div 2` is 3;
* `¯7 div 2` is ¯4.

**Loops** are replaced by words; the plan is in [roadmap.md](roadmap.md#loops-are-replaced-by-words).

## Keyword glyphs

| Glyph | Word | Glyph | Word |
|---|---|---|---|
| `§` | `module` | `▷` | `\|>` |
| `ƒ` | `fn` | `→` | `->` |
| `←` | `let … =` | `⇒` | `=>` |
| `↩` | `=` (reassign) | `⍝` | `//` comment |

`pub`, `mut`, `if`, `else`, `use`, `extern`, `unsafe` and `return` only have word forms for now. Their glyphs are still to be designed.

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
* **Matching on numbers or strings needs a final `_ =>` arm.**
* **Patterns:**
  * `_` matches anything;
  * a lowercase name binds the value;
  * literals (`1`, `"ada"`, `true`);
  * variants with sub-patterns (`Rect(w, _)`).
* **Strings compare as whole values** with `==` and `!=`.
* **Under the hood:** records and variants are immutable, reference-counted values. `with` updates in place when nothing else shares the record, and copies otherwise.

## Strings

**Interpolation:** `"hi {u.name}, next year {u.age + 1}"`. Any expression can go inside `{ }`, and it is formatted the way `print` shows it. Write `\{` for a literal brace.

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

Tuple types are tracked statically: `(i64, str)` keeps its `i64` unboxed.

## Errors without exceptions: `Option`, `Result` and `let … else`

The prelude, available everywhere, defines `Option = Some(any) | None` and `Result = Ok(any) | Err(any)`.

```mimic
fn name_of(id: i64) -> str {
    let u: User = find(id) else { return "nobody" }   // unwraps Some / Ok
    u.name
}
```

* The `else` block runs on `None` / `Err`, and must end with `return`.
* Until generics arrive (M4), `Some` and `Ok` carry dynamically typed payloads. Annotate the binding (`let u: User = …`) to get a checked, statically typed value back.

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

* Positional arguments come first.
* Defaults must be constants: literals, operators on literals, arrays of literals, or unit variants.

## Lists of anything

Array literals can hold strings, records or other arrays: `["ada" "bob"]`, `[u, older]`. Indexing and `¨` work on them as usual, and their element types are tracked statically, so `users[0].age` is checked at compile time.

## Types (annotations)

The annotation types are:
* scalar C dtypes: `i8 … i64`, `u8 … u64`, `isize`, `usize`, `f32`, `f64`, `bool`;
* `str`;
* arrays: `[i64]`, `[f64]`, `[bool]`.

In M1, annotations on parameters, return values and `let` are checked at runtime. A violation is a `type error` panic.
