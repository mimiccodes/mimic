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

## C interop ✅ (0.2.8)

| Gap | Today |
|---|---|
| `#define` constants: `c::EXIT_SUCCESS`, `c::SEEK_SET`, `c::TIOCGWINSZ`, string constants | ✅ |
| `enum` values: `c::GREEN` | ✅ |
| Global variables: `c::stdout`, `c::stderr`, `c::errno` | ✅ (read through getters) |
| Structs with array fields (`struct termios`, `struct sockaddr_un`) | ✅ (lists in the record) |
| Unions | ✅ (records; the member set is the one written) |
| A struct filled in through a pointer (`tcgetattr(fd, &t)`) | ✅ (returned in a tuple; not through `...` varargs) |
| Reading memory C returns | ✅ (`memcpy` into a `[u8]` array, see ffi.md) |
| `mimic check --fix` applies the `c::name` suggestion | ✅ |

## Tools (the Workbench, 0.3) and Windows speed (0.2.9)

| Gap | Today | Version |
|---|---|---|
| Libraries installed from git at a chosen tag or commit; workspaces | ✅ (`mimic install <git-url>@v1.2.0`, `rev = …`, `mimic install` in a project, `workspace { members }`) | 0.3.1 |
| A manual inside `mimic`: topics, functions, glyphs (`mimic man ⌿`), search, "coming from C or Python" pages, a guide for agents | ✅ (`mimic man`) | 0.3.2 |
| Syntax highlighting for editors (TextMate and tree-sitter grammars) and, later, on GitHub | ✅ editors ([mimic-syntax](https://github.com/mimiccodes/mimic-syntax)); GitHub once Linguist accepts Mimic | 0.3.3 |
| A language server: errors as you type, glyph input, hover, go to definition; names under `c::` and `cpp::` in their own colour | ✅ (`mimic lsp`) | 0.3.4 |
| `mimic check --fix` for every suggestion it shows | Some fixes only | 0.3.4 |
| A VS Code extension | ✅ ([mimic-vscode](https://github.com/mimiccodes/mimic-vscode); Marketplace and Open VSX once the publisher accounts exist) | 0.3.5 |
| Lists, text and records shown as values in the debugger; debugging from editors | ✅ (`mimic lldb`, `mimic dap`) | 0.3.6 |
| An MCP server for AI agents | ✅ (`mimic mcp`) | 0.3.7 |
| Docs with guides and examples beyond signatures, written by people or agents | ✅ (guides in `docs/`, `docs_write` in `mimic mcp`) | 0.3.8 |
| Seeing where code runs in parallel, and why not (`mimic why`) | ✅ (`mimic why`, labels in editors) | 0.3.9 |
| Quality, security and verification checks (`mimic audit`, contracts, property tests) | None | 0.3.10, 0.3.11 |
| A terminal editor (`mimic edit`) | None | 0.3.12 |
| A REPL, a profiler, and benchmarks against Rust and Go | Benchmarks against C | 0.3.13 |
| CI checks that native Windows builds use AVX2/AVX-512, as the Linux job does | ✅ (CI counts them; benchmarks run on Windows) | 0.2.9 |
| Windows as fast as Linux: parallel `fib` and `prefix_sum` slower than one core, `mandelbrot` without SIMD across elements, `sum_of_squares` 2.5× C (BENCHMARK.md) | ✅ (native CPU, real naps, bigger scan chunks) | 0.2.9 |

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
