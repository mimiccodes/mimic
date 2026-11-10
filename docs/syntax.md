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
| multiplicative | `×`/`*` `÷`/`/` (always f64) `%` (floored mod) |
| unary | `-x`, `¬x`/`not x` |
| **train** | postfix monadic prims `xs ⌽`, infix dyadic prims `xs ↑ 3`, `xs ¨ f` |
| prefix | `⍳ 10`, `⌽ ⍳ 5`, call form `take(xs, 3)` |
| postfix | `f(x)`, `xs[i]` (no space before `(` / `[`) |
| primary | `1` `¯2` `2.5` `"s"` `true` `[1 2 3]` `(e)` `if` `{ }` `unsafe { }` `x ⇒ e` |

**Trains read left to right.** `⍳ 10 ⌽ ↑ 3 +/` means "iota 10, reversed, take 3, sum", which is 24. A prefix primitive binds to the next operand only, so `⍳ 5 + 1` is `(⍳ 5) + 1`. After a pipeline stage, reading continues left to right: `xs ▷ +/ ÷ 2` is `(sum xs) ÷ 2`.

**Literals.** Inside `[ ]`, elements are separated by spaces or commas. Use `¯3` or commas for negative elements, because `[1 -2]` parses as `[1 - 2]`.

**Lambdas.** A lambda is `x => body` (or `x ⇒ body`). It can be called through a pipeline or passed to `¨`. In M1 lambdas cannot capture variables, so pass values as arguments.

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

Arithmetic and comparison broadcast a scalar against a vector, and work elementwise on two vectors of equal length.

**Parallelism is automatic.** Primitives over large arrays run on all cores. `xs ¨ f` runs in parallel whenever `f` is pure, meaning it never reaches `print` or FFI. Nothing needs to be declared.

**Known gap:** `÷` always produces `f64`, and there is no integer division operator yet.

## Keyword glyphs

| Glyph | Word | Glyph | Word |
|---|---|---|---|
| `§` | `module` | `▷` | `\|>` |
| `ƒ` | `fn` | `→` | `->` |
| `←` | `let … =` | `⇒` | `=>` |
| `↩` | `=` (reassign) | `⍝` | `//` comment |

`pub`, `mut`, `if`, `else`, `use`, `extern`, `unsafe` and `return` only have word forms for now. Their glyphs are still to be designed.

## Types (annotations)

The annotation types are:
* scalar C dtypes: `i8 … i64`, `u8 … u64`, `isize`, `usize`, `f32`, `f64`, `bool`;
* `str`;
* arrays: `[i64]`, `[f64]`, `[bool]`.

In M1, annotations on parameters, return values and `let` are checked at runtime. A violation is a `type error` panic.
