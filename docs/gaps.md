# Known gaps

Everyday things a programmer reaches for, checked one by one against the compiler (`tools/gaps.sh` runs every check and prints what is missing). Each gap has the version that closes it in [versions.md](versions.md). When a gap closes, its check moves into the golden tests and the line here gets ✅.

Several things that look missing exist under another name, or are left out on purpose. Those are listed at the end, so nobody files them as bugs. They also need a "coming from C or Python" page in the tutorial.

## Found by first users

| Gap | Status |
|---|---|
| String escape for ESC and other control characters (`\x1b`, `\u{…}`); `\033` was silently misread | ✅ fixed |
| `include "stdlib.h"` failed: C's `div` clashes with the `div` operator | ✅ fixed (names from C live under `c::`) |
| `print` cannot write without a newline, and output cannot be flushed (terminal games, progress bars) | ✅ `io::write`, `io::flush`, the `term` module |

## Terminal and console ✅ (0.2.6)

Closed: `io::write`, `io::flush`, `io::read_char`, `io::read_bytes`; the `term` module (`raw`, `read_key`, `size`, `is_terminal`, `clear`, `move_to`, `show_cursor`, `alternate_screen`, all undone at exit); `os::on_interrupt`. Golden test `text_format`; raw mode and keys checked in a pseudo-terminal.

## Text and numbers ✅ (0.2.6)

Closed: `char_code`, `char_of`; format specs in interpolation (`{x:.2}`, `{n:>5}`, `{n:05}`, `{n:#x}`, `{n:,}`, `{x:.1%}`) and `format(x, spec)`; `parse_int(text, base)`; `pad_start`, `pad_end`; `words`; `slice`.

## Collections ✅ (0.2.7)

Closed: `unique`, `sort_by` (with `descending`), `zip`, `enumerate`, `any`, `all`, `count`, `group_by`, `flatten`, `min_by`, `max_by`, `clamp`, `to_map`, the `set` module. Golden test `collections`.

## Programs and the system ✅ (0.2.7)

Closed: `panic`, `assert`, `os::env_all`, `fs::walk`, `fs::modified`, `time::format(t, pattern)`, `random::choice`, `random::shuffle`.

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
