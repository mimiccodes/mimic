# Standard library

These are visible in every file without an import. The string, list and map functions are runtime intrinsics: they are implemented in Mimic's C runtime (`runtime/mimic_std.c`) and declared in the prelude with `extern "mimic"`. They are memory safe and need no `unsafe`, and they have no side effects, so code that uses them stays parallel.

## Strings

| Function | Result |
|---|---|
| `split(s, sep)` | `[str]`. `split(s, "")` splits into characters. |
| `join(parts, sep)` | `str` |
| `lines(s)`, `chars(s)` | `[str]` (UTF-8 characters stay whole) |
| `trim(s)`, `trim_start(s)`, `trim_end(s)` | `str` |
| `contains(s, part)` | `bool`. It also works on lists: `contains([1 2 3], 2)`. |
| `starts_with(s, p)`, `ends_with(s, p)` | `bool` |
| `find(s, part)` | `Some(byte index)` or `None` |
| `replace(s, from, to)` | `str` (every occurrence) |
| `upper(s)`, `lower(s)` | `str` (ASCII letters) |
| `repeat(s, n)` | `str` |
| `string(x)` | Any value as text, the way `print` shows it |
| `parse_int(s)`, `parse_float(s)` | `Some(number)` or `None` |
| `a + b` | Joins two strings. |
| `a == b` | Compares two whole strings. |
| `≢ s`, `s ↑ n`, `s ↓ n`, `⌽ s` | Length, take, drop and reverse, as for any array. |

## Lists

| Function | Result |
|---|---|
| `push(xs, x)` | A new list with `x` appended |
| `concat(xs, ys)` | `xs` followed by `ys` |
| `sort(xs)` | Ascending, stable; numbers or strings |
| `contains(xs, x)` | `bool` |

Lists are values. `push` and `concat` return new lists and leave the original unchanged.

## Maps

```mimic
let born = ["ada": 1815, "alan": 1912]     // a map literal; [:] is empty
born["alan"]                               // 1912 (missing keys stop the program)
get(born, "linus")                         // None (Option)
has(born, "ada")                           // true
≢ born                                     // 2
keys(born), values(born)                   // in insertion order
let mut m = born
m["linus"] = 1969                          // copy-on-write: born is unchanged
put(born, "grace", 1906)                   // a new map with one more key
remove(born, "ada")                        // a new map without it
```

**What a key can be:** numbers, bools, characters or strings.

**How maps behave:**
* **Order.** Entries keep their insertion order, which is the order you see when printing and in `keys` and `values`.
* **Speed.** Lookups go through a hash index.
* **Updates.** `m[k] = v` on a `let mut` map changes it in place when nothing else shares it, and copies it first otherwise.
