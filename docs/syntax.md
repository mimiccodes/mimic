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

**Literals.** Inside `[ ]`, elements are separated by spaces or commas. Use `¯3` or commas for negative elements, because `[1 -2]` parses as `[1 - 2]`.

**Integer literals** can be written in hexadecimal `0xFF`, binary `0b1010` or octal `0o17`, with `_` separators anywhere: `1_000_000`, `0xFFFF_0000`. A hex/binary/octal literal may use all 64 bits, so `0xFFFF_FFFF_FFFF_FFFF` is `-1`.

**Bitwise operators** work on integers, elementwise on arrays, and in fused kernels, nets and `¨` like every other operator:
* **Where they bind.** They bind tighter than comparisons, unlike C, so `x & mask == 0` means `(x & mask) == 0`. They bind looser than `+` and `*`, so `1 + 2 << 3` is `24`.
* **Shifts.** `>>` is arithmetic and keeps the sign: `¯16 >> 2` is `-4`. `>>>` is logical and fills with zeros. A shift amount uses its low 6 bits, exactly as the hardware does, so every shift is a single instruction and `x << 64` is `x`.
* **Bools.** `& | ^ ~` also work on bools, without short-circuiting, which is handy for masks: `(xs > 2) & (xs < 6)`.
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
| `⌈` | `max` | 2 | larger of two values (elementwise); `(max)` / `gather(max)` as a function value |
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

The prelude, available everywhere, defines `type Option[T] = Some(T) | None` and `type Result[T, E] = Ok(T) | Err(E)` (see [Generics](#generics)).

```mimic
fn name_of(id: i64) -> str {
    let u: User = find(id) else { return "nobody" }   // unwraps Some / Ok
    u.name
}
```

* The `else` block runs on `None` / `Err`, and must end with `return`.
* The unwrapped value keeps its static type. For example, `let n = parse_int(s) else { … }` gives an `i64`.

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
* arrays: `[i64]`, `[f64]`, `[bool]`, `[str]`, `[Point]`;
* records and unions by name, and generic ones with type arguments: `Option[i64]`, `Pair[str, f64]`, `[Tree[str]]`;
* function types: `fn(i64) -> i64`.

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

* **Type arguments are inferred.** You never write `first[i64](…)`. `Some(42)` is an `Option[i64]`, and `Pair { first: 1, second: "one" }` is a `Pair[i64, str]`. `None` and `Leaf` take their type arguments from where they are used.
* **Mismatches are compile errors.** For example, `let o: Option[str] = Some(3)` is an error (E0401). So is calling `or_else(Some(1), "s")` against `fn or_else[T](o: Option[T], d: T)`, because `T` would have to be both `i64` and `str`. The wrong number of type arguments is E0411.
* **Generic code runs as fast as hand-written code.** Every function is compiled once per combination of argument types it is called with, so `first` on `[i64]` and `first` on `[str]` are separate, fully typed functions. A value taken out of an `Option[i64]`, by `match`, `let … else` or `or_else`, is a plain machine integer.
* **Values look the same whatever their type arguments.** An `Option[i64]` and an `Option[str]` share one runtime layout, so generic types add no code size.
* **A generic type named without arguments means any instance.** `fn f(o: Option)` accepts every `Option`.
* **`Option[T]` and `Result[T, E]`** in the prelude are ordinary generic unions: `type Option[T] = Some(T) | None`.
* **`<`, `<=`, `>`, `>=` on two strings** compare them as whole values, in byte-wise lexicographic order, the same way `==` does. A sorted tree of strings therefore works with the same code as a tree of numbers.

In M1, annotations on parameters, return values and `let` are checked at runtime. A violation is a `type error` panic.
