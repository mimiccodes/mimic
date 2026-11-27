---
title: Strings, lists and maps
description: The standard library functions every program can use without imports.
order: 6
---

# Strings, lists and maps

Everything in this chapter is available in every file, with no import. The functions have no side effects, so code that uses them still runs in parallel.

## Strings

```mimic run
module main

fn main() {
    let line = "  ada,grace , alan  "
    print(line ▷ trim ▷ split(","))
    print(split("a-b-c", "-") ▷ join("+"))
    print(upper("mimic") + "!")
    print(replace("2024-01-05", "-", "/"))
    print(starts_with("mimic.mim", "mimic"))
    print(contains("hello", "ell"))
    print(repeat("ab", 3))
    print(≢ "héllo")            // length in bytes
    print(chars("héllo"))       // characters stay whole
}
```

```output
["ada" "grace " " alan"]
a+b+c
MIMIC!
2024/01/05
true
true
ababab
6
["h" "é" "l" "l" "o"]
```

Two strings compare as whole values with `==` `!=` `<` `>` `<=` `>=`. The ordering is alphabetical, by byte. Insert values into text with `{…}`:

```mimic run
module main

fn main() {
    let name = "Ada"
    print("{name} has {≢ name} letters")
    print("apple" < "banana")
    print(sort(["pear" "apple" "fig"]))
    print(string(42) + "!")
}
```

```output
Ada has 3 letters
true
["apple" "fig" "pear"]
42!
```

Escapes write characters you cannot type directly: `\n` (new line), `\t` (tab), `\r`, `\0`, `\\`, `\"`, `\{` and `\}`. Use `\x1b` for any ASCII character by two hex digits, and `\u{1F600}` for any Unicode character. ESC (`\x1b`) starts the terminal's colour and cursor codes:

```mimic run
module main

fn main() {
    let bold = "\x1b[1m"
    let reset = "\x1b[0m"
    print(≢ "{bold}hi{reset}")      // two codes of 4 bytes, and "hi"
    print("caf\u{E9} \u{2192} \u{1F600}")
}
```

```output
10
café → 😀
```

C's octal escapes such as `\033` do not exist; the compiler says to write `\x1b`.

### Formatting numbers and text

Put a format after a `:` inside `{…}`. It uses the same rules as Python and Rust:

```mimic run
module main

fn main() {
    let price = 1234.5
    let n = 255
    print("{price:.2}  {price:,.1}  {0.256:.1%}")
    print("{n:x}  {n:#b}  {n:06}  {n:+}")
    print("[{"name":<8}] [{n:>5}] [{"ok":*^6}]")
    print(pad_start("7", 3, "0") + " " + string(char_code("A")) + " " + char_of(65))
}
```

```output
1234.50  1,234.5  25.6%
ff  0b11111111  000255  +255
[name    ] [  255] [**ok**]
007 65 A
```

| Spec | Meaning |
|---|---|
| `.2` | Two decimals |
| `>8` `<8` `^8` | Width 8, aligned right, left or centred (a fill character may come first: `*^8`) |
| `06` | Pad with zeros |
| `+` | Always show the sign |
| `,` | Thousands separators |
| `x` `o` `b` | Hex, octal, binary (`#` adds `0x`, `0o`, `0b`) |
| `e` `%` | Exponent; percent |

## Characters as numbers

`` `a` `` is the code of the character `a`, the number 97 (`char_code("a")`). Working on the bytes of a text, compare and compute with characters this way: nothing is made for each one, where `char_of(c) == "a"` makes a one-letter string every time.

```mimic run
module main

fn rot13(c: i64) -> i64 {
    if c >= `a` and c <= `z` { (c - `a` + 13) % 26 + `a` } else { c }
}

fn main() {
    let b = bytes::of_text("hello")
    print(bytes::to_text(bytes::from(b ¨ rot13)) else { "?" })
    print(b ¨ (c => c == `l`) ▷ +/)
}
```

```output
uryyb
2
```

`format(x, spec)` does the same as a function. `parse_int("ff", 16)` reads numbers in other bases, `words(s)` splits on any whitespace, and `slice(s, from, to)` takes a part of a string.

## Parsing

`parse_int` and `parse_float` return an `Option`, which is `Some(value)` or `None` ([chapter 7](07-errors.md)):

```mimic run
module main

fn main() {
    print(parse_int("42"))
    print(parse_int("4x2"))
    print(parse_float("2.5"))
    print(find("hello", "ll"))
}
```

```output
Some(42)
None
Some(2.5)
Some(2)
```

The `parse` module reads common formats: CSV (quoted fields included), URLs, and version numbers by Semantic Versioning.

```mimic run
module main

fn main() {
    print(parse::csv("name,age\n\"Lovelace, Ada\",36\n"))
    let u = parse::url("https://example.com:8443/docs?page=2#top")
    print("{u.host} {u.port} {u.path} {u.query} {u.fragment}")
    let a = parse::semver("1.0.0-rc.1") else { return }
    let b = parse::semver("1.0.0") else { return }
    print(parse::semver_compare(a, b))        // ¯1: the release candidate comes first
}
```

```output
[["name" "age"] ["Lovelace, Ada" "36"]]
example.com 8443 /docs page=2 top
-1
```

### Text algorithms

The `text` module has the algorithms that come up again and again: how far apart two strings are, every place a part occurs, a phonetic code for names, and wrapping words into lines.

```mimic run
module main

fn main() {
    print(text::distance("kitten", "sitting"))       // 3 edits
    print(text::find_all("abababa", "aba"))           // overlapping ones too
    print(text::soundex("Robert") == text::soundex("Rupert"))
    print(text::wrap("the quick brown fox jumps over the lazy dog", 16))
}
```

```output
3
[0 2 4]
true
["the quick brown" "fox jumps over" "the lazy dog"]
```

### Graphs

The `graph` module works on nodes numbered from 0 and edges given as lists: edge `e` goes from `from[e]` to `to[e]`, with weight `w[e]` where one is needed.

```mimic run
module main

fn main() {
    // six nodes; edge e goes from[e] → to[e], with weight w[e]
    let (from, to, w) = ([0 0 1 2 3], [1 2 3 3 4], [4 1 2 5 1])
    print(graph::bfs(6, from, to, 0))             // hops from node 0 (¯1: none)
    print(graph::shortest(6, from, to, w, 0))     // cheapest paths
    print(graph::components(6, from, to))         // node 5 stands alone
    print(graph::topo(6, from, to))
    print(graph::mst(6, from, to, w))             // the edges a spanning forest takes
}
```

```output
[0 1 1 2 3 -1]
[0 4 1 6 7 -1]
[0 0 0 0 0 5]
Some([0 5 1 2 3 4])
[0 1 2 4]
```

### Searching sorted lists

`sort` and `sort_index` order a list; the `order` module searches one that is sorted, finds the k-th smallest without sorting everything, and merges two sorted lists.

```mimic run
module main

fn main() {
    let xs = [3 9 9 9 20 41]                 // sorted
    print(order::lower_bound(xs, 9))          // where the 9s start
    print(order::upper_bound(xs, 9) - order::lower_bound(xs, 9))   // how many 9s
    print(order::select([40 10 30 20], 1))    // the second smallest
    print(order::merge([1 4 9], [2 3 10]))
}
```

```output
1
3
20
[1 2 3 4 9 10]
```

## Lists

```mimic run
module main

fn main() {
    let xs = [3 1 2]
    print(push(xs, 4))
    print(concat(xs, [9 9]))
    print(sort(xs))
    print(contains(xs, 2))
    print(xs)                  // unchanged: lists are values
}
```

```output
[3 1 2 4]
[3 1 2 9 9]
[1 2 3]
true
[3 1 2]
```

More list functions, for the everyday jobs:

```mimic run
module main

type Fruit = { name: str, price: f64 }

fn main() {
    let fruit = [Fruit { name: "fig", price: 2.5 } Fruit { name: "apple", price: 0.5 } Fruit { name: "kiwi", price: 2.5 }]
    print(sort_by(fruit, f => f.price) ¨ (f => f.name))      // cheapest first, ties keep their order
    print(group_by(fruit, f => f.price))
    print(max_by(fruit, f => ≢ f.name))
    print(unique([3 1 3 2 1]))
    print(zip(["a" "b"], [1 2]))
    print(enumerate(["x" "y"]))
    print((any([1 5 2] > 4), all([1 5 2] > 4), count([1 5 2], x => x > 1)))
    print(flatten([[1 2] [3]]))
    let seen = set::of(["red" "green"])
    print((set::has(seen, "red"), set::has(seen, "blue")))
}
```

```output
["apple" "fig" "kiwi"]
[2.5: [Fruit { name: "fig", price: 2.5 } Fruit { name: "kiwi", price: 2.5 }], 0.5: [Fruit { name: "apple", price: 0.5 }]]
Some(Fruit { name: "apple", price: 0.5 })
[3 1 2]
[("a", 1) ("b", 2)]
[(0, "x") (1, "y")]
(true, false, 2)
[1 2 3]
(true, false)
```

## Maps

A map links keys to values. Keys can be numbers, bools or strings:

```mimic run
module main

fn main() {
    let born = ["ada": 1815, "alan": 1912]
    print(born["alan"])
    print(get(born, "linus"))         // None: get is the safe lookup
    print(has(born, "ada"))
    print(keys(born))
    let mut m = born
    m["grace"] = 1906                  // copy-on-write; born is unchanged
    print(≢ m)
    print(≢ born)
    print(put(born, "linus", 1969) ▷ keys)
    print(remove(born, "ada"))
}
```

```output
1912
None
true
["ada" "alan"]
3
2
["ada" "alan" "linus"]
["alan": 1912]
```

How maps behave:
* **Order.** Maps keep their keys in insertion order.
* **Lookups.** Indexing `m[k]` with a missing key stops the program. Use `get` when a key may be absent.

See the full list in the [standard library reference](../std.md).

Next: [errors without exceptions](07-errors.md).
