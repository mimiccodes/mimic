# Known gaps

Everyday things a programmer reaches for, checked one by one against the compiler (`tools/gaps.sh` runs every check and prints what is missing). Each gap has the version that closes it in [versions.md](versions.md). When a gap closes, its check moves into the golden tests and the line here gets ✅.

Several things that look missing exist under another name, or are left out on purpose. Those are listed at the end, so nobody files them as bugs. They also need a "coming from C or Python" page in the tutorial.

## Found by first users

| Gap | Status |
|---|---|
| String escape for ESC and other control characters (`\x1b`, `\u{…}`); `\033` was silently misread | ✅ fixed |
| `include "stdlib.h"` failed: C's `div` clashes with the `div` operator | ✅ fixed (names from C live under `c::`) |
| `print` cannot write without a newline, and output cannot be flushed (terminal games, progress bars) | ✅ `io::write`, `io::flush`, the `term` module |
| `and` / `or` did not short-circuit: `n > 0 and xs[0] == "x"` read `xs[0]` of an empty list and crashed | ✅ fixed |
| `{` in a string starts an interpolation; the error spoke of digraphs | ✅ the message says so, and to write `\{` |
| `module` as a record field name | ✅ fixed |
| Types on a lambda's parameters: `(a: i64, b: i64) => a + b` | ✅ fixed |
| `match` on a list of pairs needed `_ => …` after `[]` and `[(k, v), ..rest]` | ✅ fixed |
| An empty map literal | ✅ `[:]` |
| `split("", "\n")` gave `[]` | ✅ `[""]` |
| Running a program in another folder (`os::run` has no working directory) | ✅ `os::run_in(dir, program, arguments)` |
| Is input waiting? (debouncing while reading) | a task reading into a channel, and `chan::try_receive` |
| Reading an exact number of bytes from standard input | ✅ `io::read_bytes(n)` |
| `mimic check src/a_module.mim` said `mimic run <file>` expects `module main` | ✅ a file inside a project is checked with its project |
| Checking stops at the first syntax error without saying so | ✅ it says that names and types are checked once the file parses |
| `mimic doc --json` printed errors on standard output | ✅ standard error, exit status 1 |
| `mimic create mimic-ls` refused the name | ✅ the project is `mimic_ls` in the folder `mimic-ls` |
| A float could not be turned into an integer | ✅ `math::to_int(x)` |
| Standard functions did not check their arguments' types (`to_float(5)` crashed when run) | ✅ error E0401 |

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
| Quality, security and verification checks (`mimic audit`, contracts, property tests) | ✅ (`mimic audit`, `requires`/`ensures`, `test … for`) | 0.3.10, 0.3.11 |
| A terminal editor (`mimic edit`) | ✅ | 0.3.12 |
| A REPL, a profiler, and benchmarks against Rust and Go | ✅ (`mimic repl`, `mimic profile`, Rust + Rayon in `bench/*/rayon`) | 0.3.13 |
| The profiler on every platform as on macOS | ✅ Sampling runs on macOS, Linux and Windows (CI profiles a program on each); on x86_64 Linux, a C library function without frame pointers can cut a stack short (the program and Mimic's runtime keep them in profiling builds) | 0.3.13 |
| The profiler in editors: time beside each function, a flame graph | The terminal report and its JSON | later |
| The HTTP server as fast as C's | ✅ the event loop's lock: socket waits register, fire and close outside it (the lock only guards the timer heap); the server went from 1.7–1.9× C to 1.10× C on the M3 Pro. What is left is mostly the system calls of each wait (duplicating the descriptor, kevent/epoll, reading the socket's timeout) | 0.3.14 |
| Windows as fast as macOS and Linux against Rayon: on the CI machines (4 cores, run 43823317121) fib (0.41× C against Rayon 0.39×), prefix sum (0.81 / 0.76) and records (3.36 / 2.83) are behind, and records on one core is 4.9× C (2.0–3.2× elsewhere); sort is now ahead (0.07 / 0.09) | fib was 0.61×: its parallel leaves counted their depth on every call, which x86 pays for; they now run the plain version (BENCHMARK.md) | a later performance step |
| Faster than hand-parallelised Rust (Rayon) on every benchmark: on the CI machines (run 43823317121) ahead or even on all of them on macOS; on Linux sort is behind (0.25× C against 0.22×); on Windows fib, prefix sum and records (row above) | Prefix sum: each thread scans one contiguous run of chunks, so fresh pages fault in one region per thread (0.28 s → 0.17 s, Rayon 0.16 s, M3 Pro). Fib: a recursion that splits itself runs as fork-join when called from ordinary code, not on the net (median 103 → 80 ms, Rayon 76 ms). On x86 its leaves ran the depth-counting version, ~13% more CPU than one thread; they now run the plain one. Both within ~5% of Rayon locally; the CI machines to confirm Linux fib now even with Rayon (0.42× / 0.40×, was 0.55×) | a later performance step |
| The REPL answers in milliseconds: each line is a new program, which macOS checks on its first run (about 1 s, unless the terminal is a Developer Tool); compiling into the running `mimic` would skip it | Each line builds and runs a program (about 0.5 s, plus the check on macOS) | later |
| CI checks that native Windows builds use AVX2/AVX-512, as the Linux job does | ✅ (CI counts them; benchmarks run on Windows) | 0.2.9 |
| Windows as fast as Linux: parallel `fib` and `prefix_sum` slower than one core, `mandelbrot` without SIMD across elements, `sum_of_squares` 2.5× C (BENCHMARK.md) | ✅ (native CPU, real naps, bigger scan chunks) | 0.2.9 |

## Found by the 0.4.0 corpus

Each everyday algorithm in the benchmark corpus (`bench/`, in Mimic, C and Rust) that is awkward to write in Mimic, or slow, is a gap here, with the step of stage 0.4 that closes it. Times are on one core, as a ratio to C.

| Gap | Today | Version |
|---|---|---|
| A fold over a list with a function (`fold(xs, f, start)`), as Rust, Python (`reduce`) and others have; `/` takes only an operator | `settle` over a tuple of (position, state) | 0.4.5 |
| A sequential pass over an array as fast as C's loop: `crc32` 21×, `adler32` 28×, `fnv1a` 57×, `murmur3` 21×, `xxhash32` 13× (`settle` over a tuple) | the tuple is boxed each step and the array read with checks | 0.4.3 (loops and values in registers), 0.4.4 (checks proven away) |
| Small fixed arrays as working state: `sha256` 170× (a new 8-word array every round, the message schedule grown one `push` at a time) | each step allocates | 0.4.2 (changed in place), 0.4.3 (unboxed) |
| A pass over the characters of a string as fast as C's: `tokenize` 25× (a `settle` over positions, one byte at a time) | as above | 0.4.3, 0.4.4 |
| `find(s, part)` from a position, so a parser can walk a long text without slicing it; `http_parse` splits the stream into lines first | `slice` then `find` (a copy) | 0.4.12 |
| `"x" * 3` is `[360]` (the character's code times 3), not `"xxx"`; nothing warns | `repeat("x", 3)`; it should be a compile error that names `repeat` | 0.4.5 |
| A parameter named like a built-in word (`keep`, `len`) loses to the word with a confusing message (`keep takes 2 arguments`); `let len = …` says clearly that it is built in | rename it | 0.4.5 |
| A `settle` step written as a lambda changes its state in place, as a named step function does: in `huffman` the same encoder took 37 s as `settle(s => { … put(s.words, …) … })` and 2.1 s as `settle(s => encode(s, …))`; the lambda's state is copied on every `put`/`push` | write the step as a named function taking the state (as `qoi` and `huffman` do) | 0.4.2 (ownership by analysis) |
| A named step function that reads an array from its state and then changes it (`let x = … s.v[i] …` then `{ s with v: put(s.v, j, x) }`) copies the array on every step: Myers `diff` (1,000 pairs) ran ~800 s, and 6 s once the change moved into a helper taking the state (`reach(s, …)`). Only the time grows with the array (5× faster with a 6× smaller one) The same when the state is used again after a helper changed it: in `trie`, `{ grow(t, at) with k: t.k + 1 }` took over 10 minutes, 2.5 s with `k` passed into `grow` | change the array in a separate helper that receives the state, and use nothing of the old state after it | 0.4.2 (ownership by analysis) |
| Building a long text from many small pieces (`join(⍳ 2097152 ¨ (i => piece(i)), "")`) takes ~1.8 s for 4 MiB, most of `kmp` and `aho`; C appends into one buffer in ~10 ms | none; a string builder or `join` that sizes its result first | 0.4.12 (fast libraries) |
| A list made by `flatten` of many small lists (6 million bytes from 3 million pieces) is slow to build (10.7 s) and then slow to index: three passes over it took 4.5 s in `wordfreq`, against well under a second for the same bytes from `bytes::of_text` | build text as strings, then `bytes::of_text` | 0.4.3 (unboxed), 0.4.8 (data layout) |
| `put` on a `[u8]` copied the whole list every time: only lists of `i64`, `f64` and `bool` were changed in place. 200,000 `put`s into 200,000 bytes took 3.5 s (0.03 s for `[i64]`) | ✅ changed in place when the list is held once, without boxing in a record's `with`: `sieve` with byte flags 2.9 s on one core (3.6 s with `[i64]` flags), 0.35 s on all cores | 0.4.0 |
| A record used inside `¨` was retained and released for every element: on all cores every core wrote the same reference count. `kmeans` passing its centres as a record took 3.9 s on one core and 76 CPU-seconds (9.9 s) on all cores for the nearest-centre passes; the same lists passed on their own took 1.0 s and 1.2 s | ✅ a function that only hands a record on to its own recursive call (`nearest(c, x, j + 1)`) borrows it instead of taking it over: `kmeans` with its centres in a record 1.5 s on all cores (11 s before), as fast as with the lists taken out | 0.4.0 |
| `math::pi` without `()` is the function itself, and arithmetic on it passes the checker and stops the program when it runs ("arithmetic is not defined on records or lists of values") | write `math::pi()` | 0.4.4 (types everywhere) |
| An FFT written with `¨` is 6× C (`fft`): each stage builds new real and imaginary lists and reads every input twice, once per list, where C changes one pair of arrays in place | none yet | 0.4.6 (complex numbers, `math::fft`) |
| Updates in place through helper functions and branches: `topo` took 118 s with `next: if d == 0 { push(k.next, w) } else { k.next }` inside `{ k with … }` and 7 s with the two cases written as two updates; union-find with path halving took 540 s (a `find` that changes the links copies them: the caller's record still holds them) and 1.1 s without | write each case as its own `{ … with … }`; `components` uses union-find without path compression | 0.4.2 (ownership by analysis) |
| A growing list held by a `settle` state as fast as a vector: `lz77`'s decoder is 476× C. `push(o, o[i])` in `evolve` and `concat(out, …)` copy the whole list (it is still held by the state), and pushing bytes goes through the generic array paths | copy a match as a slice (`concat` of `↓`/`↑`) where it can; the copies remain | 0.4.2 (changed in place: the state is not used again), 0.4.3 |
| Many small lists as fast as a flat array: `rle` 66× C builds a two-element list per run | `flatten` is linear now (it was quadratic, found by `rle`) | 0.4.3 (unboxed), 0.4.8 (data layout) |
| An interpreter's loop as fast as C's: `brainf` 27×, `vm` 54×, `regex` 31× (a `settle` over a state record, one instruction or character a step) | named step functions keep the record in place; each step still goes through the record | 0.4.3 (loops and values in registers) |
| `xs ↓ -1`: after a glyph, `-` is read as subtraction, so a negative number needs `¯1` | write `¯1` | 0.4.5 |
| `sort_index` (the order that sorts a list, as `⍋` will) is in the prelude but not in docs/std.md | it works; `bwt` uses it | 0.4.5 (`⍋`) |
| Bytes as `[u8]` in fused code: the corpus builds its data as `[i64]`, 8 bytes per byte | `[u8]` exists; `⍳ n ¨ …` makes `[i64]` | 0.4.7 (bits, bytes and small vectors) |

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
