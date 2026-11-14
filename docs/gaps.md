# Known gaps

Everyday things a programmer reaches for, checked one by one against the compiler (`tools/gaps.sh` runs every check and prints what is missing). Each gap has the version that closes it in [versions.md](versions.md). When a gap closes, its check moves into the golden tests and the line here gets ✅.

Several things that look missing exist under another name, or are left out on purpose. Those are listed at the end, so nobody files them as bugs. They also need a "coming from C or Python" page in the tutorial.

## Found by first users

| Gap | Status |
|---|---|
| String escape for ESC and other control characters (`\x1b`, `\u{…}`); `\033` was silently misread | ✅ fixed |
| `include "stdlib.h"` failed: C's `div` clashes with the `div` operator | ✅ fixed (names from C live under `c::`) |
| `print` cannot write without a newline, and output cannot be flushed (terminal games, progress bars) | 0.2.6 |

## Terminal and console (0.2.6)

| Gap | Today |
|---|---|
| `io::write(text)`: write without a newline | Only `print`, which adds one |
| `io::flush()` | Output is flushed at exit, before reading stdin, and by `eprint` |
| `io::read_char()` / `io::read_key()`: one key, without waiting for Enter | Only whole lines (`io::read_line`) |
| `io::read_bytes(n)` from stdin | Only text |
| `term::raw(on)`: raw mode (no echo, no line editing) | Needs termios from C, which cannot be imported yet (see C interop) |
| `term::size()`: columns and rows | Needs `ioctl(TIOCGWINSZ)` from C, which cannot be imported yet |
| `term::is_terminal(stream)` | None |
| Ctrl-C: `os::on_interrupt(f)` | The program ends at once |

## Text and numbers (0.2.6)

| Gap | Today |
|---|---|
| Character codes: `char_code("A")` → 65, `char_of(65)` → `"A"` | None |
| Format specifiers in interpolation: `"{x:.2}"`, `"{n:>5}"`, `"{n:05}"`, `"{n:x}"` | Only the default formatting |
| Numbers as hex, binary, octal text | None |
| `parse_int(text, base)` | Base 10 only |
| `pad_start`, `pad_end` | None |
| `words(text)`: split on any whitespace | `split` on one separator |
| Substring by position: `slice(text, from, to)` | `↑` / `↓` work (take, drop), but are two steps |

## Collections (0.2.7)

| Gap | Today |
|---|---|
| `unique(xs)` | None |
| `sort_by(xs, key)`, sorting descending | `sort` ascending only |
| `zip(a, b)`, indices with values (`enumerate`) | `⍳ ≢ xs` gives the indices |
| `any(mask)`, `all(mask)` | `gather` with `or` / `and` |
| `count(xs, f)`, `group_by(xs, f)`, `flatten` | None |
| `min_by` / `max_by` | `⌈/` and `⌊/` on numbers |
| `clamp(x, lo, hi)` | `lo ⌈ x ⌊ hi` |
| Sets: `set::of(xs)`, membership, union, intersection | Maps with dummy values |
| A map from a list of pairs | `map_of(keys, values)` takes two lists |

## Programs and the system (0.2.7)

| Gap | Today |
|---|---|
| `panic(message)` and `assert(cond)` in ordinary code | `expect` exists in tests only |
| `os::env_all()` | One variable at a time |
| `fs::walk(dir)`: a directory tree | `fs::list_dir`, one level |
| File times and metadata: `fs::modified(path)` | `fs::size`, `fs::is_file`, `fs::is_dir` |
| `time::format(t, pattern)` | One fixed format |
| `random::choice`, `random::shuffle` | `random::int`, then index |

## C interop (0.2.8)

| Gap | Today |
|---|---|
| `#define` constants: `c::EXIT_SUCCESS`, `c::SEEK_SET`, `c::TIOCGWINSZ`, string constants | Not imported |
| `enum` values: `c::GREEN` | The type maps to `i32`, but the names are not imported |
| Global variables: `c::stdout`, `c::stderr`, `c::errno` | Not imported |
| Structs with array fields (`struct termios`, `struct sockaddr_un`) | Functions using them are skipped |
| Unions | Skipped |
| A struct filled in through a pointer (`ioctl(fd, TIOCGWINSZ, &ws)`, `tcgetattr(fd, &t)`) | Only arrays of numbers can be passed for C to fill |
| Reading memory C returns (`bytes::from_ptr(p, n)`) | Only `cstr` and arrays passed in |
| `mimic check --fix` applies the `c::name` suggestion | Shown, not applied |

## Tools (the Workbench, 0.3; Windows speed, 0.2.9)

| Gap | Today | Version |
|---|---|---|
| A manual inside `mimic`: topics, functions, glyphs (`mimic man ⌿`), search, "coming from C or Python" pages | `mimic explain` for errors, `mimic doc --std` | 0.3.1 |
| Syntax highlighting for editors (TextMate and tree-sitter grammars), names under `c::` and `cpp::` in their own colour | None | 0.3.2 |
| A language server: errors as you type, glyph input, hover, go to definition | `mimic check --emit-json` | 0.3.2 |
| `mimic check --fix` for every suggestion it shows | Some fixes only | 0.3.2 |
| An MCP server for AI agents | None | 0.3.3 |
| Seeing where code runs in parallel, and why not (`mimic why`) | None | 0.3.4 |
| Quality, security and verification checks (`mimic audit`, contracts, property tests) | None | 0.3.5, 0.3.6 |
| A terminal editor (`mimic edit`) | None | 0.3.7 |
| A REPL and a profiler | None | 0.3.8 |
| CI checks that native Windows builds use AVX2/AVX-512, as the Linux job does | Measured on Linux and macOS only | 0.2.9 |

## Not gaps: other names, or left out on purpose

| You might look for | In Mimic |
|---|---|
| `for`, `while` loops | `¨` (each), recursion, `evolve(n, f)`, `settle(f, done)`: no loops on purpose; they hide parallelism |
| `max(xs)`, `min(xs)` of a list | `⌈/ xs` (`maximum`), `⌊/ xs` (`minimum`) |
| `filter(xs, f)` | `xs ⌿ (xs ¨ f)` (keep by mask) |
| `len(xs)` | `≢ xs` (`len`) |
| `range(a, b)` | `a ‥ b` (`range`); `⍳ n` from 0 |
| `xs[1:3]` | `xs ↓ 1 ↑ 2` (drop, take) |
| Integer division | `div` (`⫽`); `/` always gives a float |
| Exceptions | `Result` and `else { … }` |
