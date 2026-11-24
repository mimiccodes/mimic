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

## Build time

| Gap | Today | Version |
|---|---|---|
| The prelude is parsed, resolved and checked again for every build, and the runtime linked from its parts: a one-line program takes about 0.35 s to build (warm, macOS arm64, release): front end 70 ms (prelude parse 25, resolve 15 to 20, check 17 to 21), code generation 100 ms (purity and net plans 25, emitting 14, LLVM passes 42, object 19), link 80 ms | Planned for 0.4.1, moved: saving the parsed prelude needs the whole syntax tree serializable, for 15 to 20% of a build; limiting the purity analysis to reachable functions risks code the generator calls implicitly being taken as impure (slower) for 20 ms | 0.4.9 |

## Found by a user's machine-learning project

Reported from a real project (tensors as a flat `[f64]` plus their dimensions, automatic differentiation, optimizers, training loops), with reproductions and profiles; each is a gap here with the step that closes it.

| Gap | Workaround now | Version |
|---|---|---|
| A function named like a primitive's word (`fn matmul`, `fn transpose`) compiled, and `⊠`/`⍉` or the word then meant different things | ✅ E0214 where the function is declared | 0.4.3 |
| Most numeric library code stays outside the IR: records holding lists (`Tensor`), unions (`Op`), lists of records (`[Tensor]`), function-typed parameters (`fn([Tensor]) -> Tape`), and small wrappers that only call others | none | 0.4.3 (records holding lists, lists of records, unions); 0.4.10 (function values, as the old code generator retires) |
| A parameter declared `[i64]`/`[f64]` reported as `[?]` or `dynamic` and left out of the IR | ✅ compiled for the declared type, the call converts (as in the held-out row above) | 0.4.3 |
| A tail-recursive training loop returning a tuple that holds lists (`(Adam, [Tensor])`) kept every step's dead tape alive: 4.4–8.9 GB, against 36–69 MB as an `evolve` over a record | `evolve` or `settle` over a record | 0.4.3 (tail calls become loops; tuples holding lists) |
| `xs ¨ f` with `f` a function parameter is about 14× slower than the same lambda written inline (8,192 floats: 93.8 ms against 6.8 ms) | write the lambda inline | 0.4.3 (a function applying a function parameter specialised for a known lambda at its call) |
| Chains of operations on small arrays (2k–33k floats) never use more than one core: each is below the parallel threshold | none | 0.4.10 (one cost model), 0.4.11 (independent updates at once) |
| Primitive words behave differently by where they appear: a variable gets E0214, a function took over the glyph, a parameter named `shape` gave a parse error later (`expected an expression, found '=='`) | ✅ E0214 wherever a name is declared: functions, parameters, typed `let`s, lambda parameters | 0.4.3 |
| No assignment to existing `mut` variables from a tuple: `(t, x) = f(t)` (E0208) | return a record and read its fields | 0.4.5 |
| A field cannot be read off a call's result: `f(x).field` (E0201) | `let r = f(x)` first | 0.4.5 |
| No whole-value equality: `a == b` on lists is element by element (`[bool]`) and stops the program on different lengths | `(≢ a) == (≢ b) and all(a == b)` | 0.4.5 (`≡`, same) |
| Indexing with a list of indices, `xs[[3 0]]`, passes the checker and stops the program | `ids ¨ (i => xs[i])` | 0.4.5 (`⊇`, select; a list index refused at compile time until then) |
| `math` had no `tanh`, `sinh`, `cosh`, `log1p`, `expm1`, `sigmoid` | ✅ added (libm calls that fuse into kernels; `sigmoid` without overflow) | 0.4.3 |
| No direct `bool` → number conversion (one-hot vectors, masks) | `¨ (b => if b { 1.0 } else { 0.0 })` | 0.4.5 |
| No fallback for an `Option`: `parse_int(s) ▷ or_else(300)` | ✅ `unwrap_or` for `Option`; for `Result`, a name still to choose | 0.4.3; 0.4.5 (`Result`) |
| `[u8]` from `fs::read_bytes` needs a map to be used as `[i64]` | `b ¨ (x => x * 1)` | 0.4.7 (bits, bytes and sized integers) |
| A local cannot share a name with any function of its module (`let steps` next to `fn steps`: E0301) | another name | 0.4.5 |
| Matrices are typed only at run time: a function cannot declare that it takes one | keep a flat list and its dimensions | 0.4.6 (shapes) |
| `mimic audit` does not run on a workspace, as `check`, `build`, `test` and `fmt` do | run it in each member | 0.4.13 |
| Audit rule Q05 counts a `match` over a union's variants as many branches, though its own help recommends that form | none | 0.4.13 |
| Audit rule Q06 calls two functions duplicates when their bodies match but their parameter types differ | none | 0.4.13 |
| Releases ship the toolchain for Linux x86-64, macOS arm64 and Windows x86-64 only: none for Linux arm64, macOS x86-64 or Windows arm64 | build from source | 0.4.9 (Linux arm64); 0.10 (the rest) |
| `mimic fmt --glyphs` leaves code in comments (between backticks) in word form | none | 0.4.5 |
| The generated `mod.mim` writes signatures in word form in a project formatted with glyphs | none | 0.4.5 |
| `mimic fmt --glyphs` does not convert the syntax glyphs (`§ ƒ ← ↩ ⍝`), and `mimic fmt` turns `§ ƒ` back but not `← ↩ ⍝`; `--check` passes on mixed files | none | 0.4.5 |
| `←` binds only a plain name: `b: [i64] ← …` and `(q, r) ← …` need `let` | `let` | 0.4.5 |
| `⥊` and `⍉` always copy: about 1 GB allocated per training step of a 173k-parameter model | none | 0.4.8 (a reshape or transpose a view over the same storage; one used once reuses its buffer) |
| No matrix product with a transposed operand (`Aᵀ ⊠ B`): each backward product transposes a full copy first | `⍉` then `⊠` | 0.4.6 (to the kernel's and BLAS's transpose flags) |
| `⊠` on mid-size float products keeps about 1.5 of 4 cores busy | none | 0.4.6 |
| `mimic why` sees no arrays behind record fields ("nothing to decide" for a function that is one matrix product of fields) | none | 0.4.4 |
| The profiler puts 18–34% of the time in an unnamed runtime frame `end` | none | 0.4.9 (the runtime's symbols named, or the region said) |
| A function's name where a record was meant (`if k == 0 { run } else { now }` in a function returning `R`) passed `mimic check` and stopped the program when it ran, under `mimic test` with only `index: got unit value ()` | ✅ E0401 at the name: a function where the declared result is a value, as a branch, a `match` arm or a `return` | 0.4.3 |

## Found by the held-out set

The held-out set (`bench/heldout/`) is 40 programs written plainly from the tutorial alone, never used for tuning (BENCHMARK.md has its numbers). Its first measurement (2026-11-24, 0.4.3-dev): 15 of the 40 run in Mimic, all with the right output, at 8.5–10× C on one core (geometric mean); 25 fail. Each slow spot here gets a corpus program of its own to tune on; the held-out programs are never changed for speed.

| Gap | Workaround now | Version |
|---|---|---|
| A list built by `push` in a function that calls itself, its elements not `i64` (strings, floats, records): `gen(n - 1, y, push(acc, v))`, the way every held-out program makes its input. Only a list of `i64` is taken over by such a function, so every `push` copies the list, and each copy stays alive in its call's frame: time and memory grow with the square of the length (20 of the 40 held-out programs use over 4 GB on Linux; `wordfreq`'s million words) | build the list with `¨` (`⍳ n ¨ (i => …)`) when each element can be computed from its index | 0.4.3 (a list of any element type taken over and grown in place; the recursion a loop) |
| Deep recursion that carries a list or a map crashes with a segmentation fault and no message (`bank`, `histogram`, `inventory`, `sudokucheck` at 1,000,000 to 4,000,000 calls), as in the row on deep recursion below | `evolve` or `settle` | 0.4.3 (tail recursion becomes loops); a message instead of the bare crash sooner |
| A map threaded through a function that calls itself (`tally(ws, i + 1, put(counts, w, c))`) has a dynamic type: the function stays on the generic path, outside the IR | none | 0.4.4 (types everywhere) |
| `bigfactorial` (a list of digits, schoolbook multiplication) 508–3,242× C, `life` (a grid as a list of rows) 117–360×, `nqueens` (backtracking) 116–263×, `dijkstra` (adjacency lists) 47–143×: each still to be traced to its cause | none | 0.4.3 (traced first, then each cause given its step) |
| A pure `¨` over a short list (9 cells of a sudoku row) was always handed to every core, whatever its elements cost: `sudokucheck` woke the thread pool 27,000 times in 20,000 grids and ran 6× slower on 12 cores than on one, mostly in the kernel (12–15 s of system time) | ✅ a short list (up to 16,384 elements) is shared out only once the rest is worth waking the pool for: timed at the call, or at once when the function has been costly before; the pool wakes as many threads as there are chunks | 0.4.3 |
| `g[r][c] = v`: an element of a list of lists cannot be assigned (E0208); the whole row is replaced with `put` instead | `put(g, r, put(g[r], c, v))` | 0.4.5 (the array core) |
| `{ s with … }` on a value read from a map with no declared type needs `let s: T = …` first (E0408) | annotate the `let` | 0.4.4 (types everywhere) |
| `sort_by(xs, key, descending: true)` reverses the order of equal keys; the manual promises a stable order only ascending | sort by the negated key, ascending | 0.4.3 |
| A function named like a built-in primitive's word (`fn matmul(…)`, the word of `⊠`) compiled without a word, but every `matmul(…)` is the primitive: the function could never be called (reported by a user) | ✅ refused where it is declared (E0214) | 0.4.3 |
| A parameter declared `[i64]` given a list of unknown type (from `flatten`, a list of lists, a value of unknown type) is compiled for that unknown type (`[?]`), not converted: the function misses the IR and its in-place changes, as in the row on lists of unknown type below (reported by a user, `bytes: [i64]`) | ✅ for `[i64]` and `[f64]`: the function is compiled for the declared type and the call converts the list (`bigfactorial`: 39 s → 0.9 s) | 0.4.3 |
| A function taking a list of records (`ts: [Tensor]`) stays outside the IR (a list of records is not one of its types), so it gets none of 0.4.2's release-at-last-use (reported by a user) | none | 0.4.3 |
| Stopping `mimic run` (a time limit, `kill`, a closed terminal) left the compiled program running, holding its memory | ✅ the program takes `mimic run`'s place on macOS and Linux (#139) | 0.4.3 |

## Found by the 0.4.0 corpus

Each everyday algorithm in the benchmark corpus (`bench/`, in Mimic, C and Rust) that is awkward to write in Mimic, or slow, is a gap here, with the step of stage 0.4 that closes it. Times are on one core, as a ratio to C; where a line says 0.4.0, the ratio is the geometric mean over Linux x86-64, macOS arm64 and Windows x86-64 as released.

| Gap | Today | Version |
|---|---|---|
| A fold over a list with a function (`fold(xs, f, start)`), as Rust, Python (`reduce`) and others have; `/` takes only an operator | `settle` over a tuple of (position, state) | 0.4.5 |
| A sequential pass over an array as fast as C's loop: `crc32` 32×, `adler32` 31×, `fnv1a` 45×, `murmur3` 21×, `xxhash32` 15×, `siphash` 27× in 0.4.0 (`settle` over a tuple; `adler32` written as a function that calls itself runs 4.6 s → 0.6 s, `crc64` written so is 1.2×) | ✅ a tuple of numbers and truth values stays in registers: on macOS, one core, `crc32` 6.8 s → 0.79 s, `fnv1a` 4.5 s → 0.62 s, `adler32` 0.15 s of CPU; the array is still read with checks | 0.4.1; 0.4.4 (checks proven away) |
| Small fixed arrays as working state: `sha256` 82× in 0.4.0 (170× before) (a new 8-word array every round, the message schedule grown one `push` at a time) | ✅ the schedule grown by `push` stays a typed list (a list state whose length changes, `push` of numbers typed), and the round's `(i, [8 words])` stays in registers: `sha256` 4.7 s → 0.35 s of CPU on macOS | 0.4.1 |
| A pass over the characters of a string as fast as C's: `tokenize` 26× in 0.4.0 (a `settle` over positions, one byte at a time) | as above | 0.4.3, 0.4.4 |
| `find(s, part)` from a position, so a parser can walk a long text without slicing it; `http_parse` splits the stream into lines first | `slice` then `find` (a copy) | 0.4.12 |
| `"x" * 3` is `[360]` (the character's code times 3), not `"xxx"`; nothing warns | `repeat("x", 3)`; it should be a compile error that names `repeat` | 0.4.5 |
| A parameter named like a built-in word (`keep`, `len`) loses to the word with a confusing message (`keep takes 2 arguments`); `let len = …` says clearly that it is built in Also a `let count = (v) => …` in `main`: calls to `count(0)` go to the prelude's `count(xs, f)` ("missing argument f"); a function named `take` is called as the built-in and stops with "take/drop count must be an integer scalar" at run time; a parameter `len` is read as `≢` | rename it | 0.4.5 |
| A `settle` step written as a lambda changes its state in place, as a named step function does: in `huffman` the same encoder took 37 s as `settle(s => { … put(s.words, …) … })` and 2.1 s as `settle(s => encode(s, …))`; the lambda's state is copied on every `put`/`push` | write the step as a named function taking the state (as `qoi` and `huffman` do) | 0.4.3 (moved from 0.4.2: the lambda is compiled as a closure, outside the IR's ownership analysis) |
| A named step function that reads an array from its state and then changes it (`let x = … s.v[i] …` then `{ s with v: put(s.v, j, x) }`) copies the array on every step: Myers `diff` (1,000 pairs) ran ~800 s, and 6 s once the change moved into a helper taking the state (`reach(s, …)`). Only the time grows with the array (5× faster with a 6× smaller one) The same when the state is used again after a helper changed it: in `trie`, `{ grow(t, at) with k: t.k + 1 }` took over 10 minutes, 2.5 s with `k` passed into `grow`. `lzw`'s decoder (`let e = learn(d, c)` after reading `codes[d.i]`) copied its 4 MB output for every code: 3.3 s for 35,000 codes, 0.05 s once each step hands the state on whole, `emit(learn(d, codes), codes)`. The same in `hull`: `let p = ps[c.i]` then `pop(c, p)` copied the chain at every point (over 10 minutes; 1.6 s as `push_next(pop(c, ps), ps)`). And a state handed to helpers whose result goes into a `let` is only borrowed, so every helper copies: `maze`'s search as `let done = look(…look(s, …)…)` then `{ done with head: … }` took 7 minutes 40 s, under a second as `advance(look(…look(s, …)…))`; `bipartite` the same (over 10 minutes; 0.7 s) | ✅ in the functions the IR compiles: reading the state and then changing it (`diff`), a helper's result changed again (`{ grow(t, at) with k: t.k + 1 }`), a field read and then the state handed on (`hull`, `lzw`'s decoder), helpers in a `let` and then a `with` (`maze`, `bipartite`): each changes in place, the state taken by the step (`state_updates` in the tests) | 0.4.2 |
| Building a long text from many small pieces (`join(⍳ 2097152 ¨ (i => piece(i)), "")`) takes ~1.8 s for 4 MiB, most of `kmp` and `aho`; C appends into one buffer in ~10 ms | none; a string builder or `join` that sizes its result first | 0.4.12 (fast libraries) |
| A list made by `flatten` of many small lists (6 million bytes from 3 million pieces) is slow to build (10.7 s) and then slow to index: three passes over it took 4.5 s in `wordfreq`, against well under a second for the same bytes from `bytes::of_text` | build text as strings, then `bytes::of_text` | 0.4.3 (unboxed), 0.4.8 (data layout) |
| A list of unknown type (from `flatten`, or a list of lists) taken by a parameter declared `[i64]` becomes a list of `i64`: today the declaration is checked, not converted, so the function runs on the generic path. A bottom-up merge sort of 2,000,000 numbers whose passes `flatten` their merged runs takes 0.4 s for the first pass and ~1.8 s for every later one (35 s in all; C sorts them in ~0.2 s), so it is not in the corpus yet | ✅ a parameter declared `[i64]` or `[f64]` given such a list is compiled for its type, and the call converts the list once (0.4.3); a list of lists stays of unknown type (see the held-out rows above) | 0.4.3; 0.4.4 (lists of lists) |
| A list of lists, or a list taken out of a tuple of lists, has elements of unknown type, and so does what is read from it: `vm`'s program `[[op, arg] …]` gives `code = program ¨ (p => p[0])` of unknown type, `tokenize`'s per-line counts are a list of lists, and `brainf`'s jump table comes out of a `settle` over a tuple of lists (one of them starting as `[]`). Every function that takes such a value runs on the generic path and stays out of the IR (`MIMIC_IR=why` says which), so it gets none of 0.4.2's release-at-last-use and in-place changes | write each column out as a list of numbers of its own (`let code = [0 2 0 2 …]`, or `⍳ n ¨ (i => …)` computing a number), and keep lists in a record (start a list field with `⍳ 0`) rather than in a tuple | 0.4.4 (types everywhere) |
| `put` on a `[u8]` copied the whole list every time: only lists of `i64`, `f64` and `bool` were changed in place. 200,000 `put`s into 200,000 bytes took 3.5 s (0.03 s for `[i64]`) | ✅ changed in place when the list is held once, without boxing in a record's `with`: `sieve` with byte flags 2.9 s on one core (3.6 s with `[i64]` flags), 0.35 s on all cores | 0.4.0 |
| A record used inside `¨` was retained and released for every element: on all cores every core wrote the same reference count. `kmeans` passing its centres as a record took 3.9 s on one core and 76 CPU-seconds (9.9 s) on all cores for the nearest-centre passes; the same lists passed on their own took 1.0 s and 1.2 s | ✅ a function that only hands a record on to its own recursive call (`nearest(c, x, j + 1)`) borrows it instead of taking it over: `kmeans` with its centres in a record 1.5 s on all cores (11 s before), as fast as with the lists taken out | 0.4.0 |
| `math::pi` without `()` is the function itself, and arithmetic on it passes the checker and stops the program when it runs ("arithmetic is not defined on records or lists of values") | write `math::pi()` | 0.4.4 (types everywhere) |
| An FFT written with `¨` is 6× C (`fft`): each stage builds new real and imaginary lists and reads every input twice, once per list, where C changes one pair of arrays in place | none yet | 0.4.6 (complex numbers, `math::fft`) |
| A loop as a `settle` over a tuple as fast as the same loop as a recursion: `collatz` written `(n, 0) ▷ settle(t => …, t => t.0 <= 1)` took 119 s on one core, 4.7 s as `steps(x, s)` calling itself (C: 1.9 s) | write the loop as a function that calls itself, with the state as parameters | 0.4.3 (loops and values in registers) |
| An element's row and column from its index as fast as C's nested loops: `floyd` computes `p div n` and `p % n` for every one of 640,000 pairs, 800 times (2.9× C on one core, where C runs `i` and `j` in two loops) | none | 0.4.4 (checks proven away), 0.4.6 (shapes) |
| A list passed bare to a function that `put`s into it and calls itself updated in place: `up(put(put(a, p, x), i, y), p)` copies `a` at every `put` (a heap's sift-up over 1,000,000 pushes ran for over 10 minutes); a function takes over a record it changes with `{ h with … }`, not a bare list. `histeq` counting each row into 256 bins this way took 20 s for 8 million `put`s, 0.28 s with the bins in a record. And a list changed under a `let` first is copied too: `let buf = put(d.buf, …)` then `{ d with buf: buf }` (`dither` ran for minutes; under a second with the `put`s inside the `with`) | keep the list in a record and change it with `{ h with a: put(h.a, …) }` (`heap` does: 1.0 s). ✅ the `let buf = put(d.buf, …)` then `{ d with buf: buf }` case: the change is made by the `with`, in place (`lz77`'s table, 15% faster) | 0.4.2 (ownership by analysis) |
| `xs ▷ trail(+)` over numbers gave a list of unknown type, so every read of the result went through the generic path: a 256-entry table built by `trail(+)` (histogram equalization) took 3 s to look up 8 million pixels, against 0.03 s for the same table built with `¨` | ✅ `trail` and `gather` with `+ × ⌈ ⌊` over a list of `i64` or `f64` give that type (the runtime already scanned such lists unboxed) | 0.4.0 |
| `push` of a number onto a `[u8]` list copied the whole list each time (the number is not of the list's kind): writing bytes one at a time was quadratic, 0.63 s for 100,000 pushes | ✅ a byte pushed onto bytes held once grows them in place: 2 ms for 100,000, as for `[i64]` | 0.4.0 |
| `xs ⌿ (xs ¨ f)` with a function: the filter was fused into one loop that computed the mask twice on one core, 12× slower than building the mask with `¨` (300,000 Miller-Rabin tests: 1.2 s against 0.1 s) | ✅ a filter whose sides call a function is not fused: the mask is one parallel map | 0.4.0 |
| `concat` of two lists of numbers gave a list of unknown type, so every later read went the generic way (35× slower to read than a list made by `¨`); and `push` onto a list still in use boxed every element to copy it | ✅ `concat` gives the list's kind back; the copy is one `memcpy`. (`push` still gives a list of unknown type: typing it made a tiny map after it ask the GPU on macOS, whose first question sets up Metal, 14 ms, in `components`; the GPU is asked before the size is checked) | 0.4.0 |
| `gather` and `trail` with a lambda as fast as with an operator: `ways ▷ gather((x, y) => (x + y) % p)` over 3,000,000 numbers took 1.2 s on one core (~400 ns a value: each pair goes through the runtime as boxed values), where `(ways ▷ +/) % p` takes milliseconds | ✅ a lambda over numbers that gives back the same kind is typed and folded in a loop: 3,000,000 numbers through `gather` and `trail`, 2.35 s → 0.08 s of CPU | 0.4.1 |
| Small tuples as cheap as separate values: a tuple is allocated each time it is made. `siphash` returns its four state words as a tuple from each group of rounds: 22× C (14.6 s on one core when the state was also passed as a tuple, 3.2 s as four parameters; C 0.14 s); `md5` took 14× C with an `(f, g)` pair per step and 1.4× with two `let`s | ✅ a tuple of numbers and truth values is kept in registers (built, passed, returned, taken apart); tuples holding lists or text are still allocated | 0.4.1 (numbers); 0.4.3 (the rest) |
| Deep recursion through a function that takes a list or returns a tuple: native recursion hands over to the net every 2,048 levels, but such a function comes back to native code, so the native stack still grows. A tail-recursive walk over 100,000 elements returning a pair stops with a segmentation fault and no message (20,000 is fine; debug builds fail sooner); a function of plain numbers recurses 100,000 deep without trouble | loop with `evolve` or `settle` (`md5` does over its 131,073 blocks) | 0.4.3 (tail recursion becomes loops); a message instead of the bare crash sooner |
| `type V = (i64, i64, i64, i64)`: a type name for a tuple is not accepted ("expected identifier, found `(`") | write the tuple type out, or use a record | 0.4.5 |
| `(ay > y) != (by > y)`: two parenthesized comparisons compared with `!=` are refused as a chained comparison ("comparisons cannot be chained") | name them first: `let (a, b) = (ay > y, by > y)`, then `a != b` | 0.4.5 |
| A long condition split over lines: neither `x or` at the end of a line nor `or y` at the start of the next continues the expression ("expected an expression") | wrap the whole condition in parentheses: inside them, lines continue | 0.4.5 |
| A lambda kept in a `let` and called in a hot loop as fast as a function: `integral`'s `let at = (x, y) => sat[y * (w() + 1) + x]`, called 4 times for each of 8,000,000 boxes, took 24 s on one core (11 s with typed parameters); `fn at(sat: [i64], x: i64, y: i64)` took 0.6 s | ✅ a `let` lambda that is only called becomes a hidden function (its captured variables extra arguments): 8 million calls in a map, 5.8 s → 0.03 s of CPU; one used as a value, or seeing a variable that changes, stays a lambda | 0.4.1 |
| An `evolve` or `settle` inside a parallel map (`⍳ k ¨ (c => … ▷ evolve(…))`) as fast as one outside it: inside the map it is not compiled as a loop and boxes its numbers; a radix sort's per-chunk counts that way took 6 s for 4,000,000 keys | ✅ (updates in place in every build, tuples of numbers in registers): 4,000,000 keys counted into bins per chunk inside a map, 0.05 s of CPU | 0.4.1 |
| `{ s with f: put(s.f, …) }` (and `push`) changing the list in place in a debug build, as in an optimised one: without the inline heap the step copies the whole list, so a sieve of 1,000,000 takes about a minute in debug and 0.03 s optimised | ✅ debug builds move values, take over parameters and update records in place as optimised ones do: a debug sieve of 200,000 in 1.8 s (over ten minutes before) | 0.4.1 |
| `let x = f() else { panic(…) }`: the `else` block must end with `return`, though `panic` never returns | `match f() { Some(v) => v, None => panic(…) }` | 0.4.5 |
| Updates in place through helper functions and branches: `topo` took 118 s with `next: if d == 0 { push(k.next, w) } else { k.next }` inside `{ k with … }` and 7 s with the two cases written as two updates; union-find with path halving took 540 s (a `find` that changes the links copies them: the caller's record still holds them) and 1.1 s without | ✅ a branch inside the `with` (`next: if d == 0 { push(k.next, w) } else { k.next }`): two updates, the one taken changes the list in place (0.4.2). Union-find with path halving, `find` returning `(links, root)`, still copies: a tuple holding a record is not in the IR yet | 0.4.2; 0.4.3 (tuples holding records) |
| A growing list held by a `settle` state as fast as a vector: `lz77`'s decoder is 1,200× C in 0.4.0 (626× on macOS, 2,560× on Linux). `push(o, o[i])` in `evolve` and `concat(out, …)` copy the whole list (it is still held by the state), and pushing bytes goes through the generic array paths | copy a match as a slice (`concat` of `↓`/`↑`) where it can; the copies remain. In 0.4.2 the encoder's table is changed in place (15% faster), but the decoder, a `settle` step written as a lambda over a state starting with `[]`, still copies: 373–1,960× C on the CI machines | 0.4.3 (moved from 0.4.2: the lambda step and a result built where its caller keeps it) |
| Many small lists as fast as a flat array: `rle` 18× C in 0.4.0 (66× before) builds a two-element list per run | `flatten` is linear now (it was quadratic, found by `rle`) | 0.4.3 (unboxed), 0.4.8 (data layout) |
| An interpreter's loop as fast as C's: `brainf` 23×, `vm` 66×, `regex` 38× in 0.4.0 (a `settle` over a state record, one instruction or character a step) | named step functions keep the record in place; each step still goes through the record | 0.4.3 (loops and values in registers) |
| `xs ↓ -1`: after a glyph, `-` is read as subtraction, so a negative number needs `¯1` | write `¯1` | 0.4.5 |
| `sort_index` (the order that sorts a list, as `⍋` will) is in the prelude but not in docs/std.md | it works; `bwt` uses it | 0.4.5 (`⍋`) |
| A short list grown by `push` reallocated on every later `push`, though its block had room: the room was only looked up for lists past the pooled sizes. `levenshtein` (rows of ~20 numbers built a `push` at a time) spent most of its time in `realloc`: 13× C, 25× on Windows, where `realloc` is slower | ✅ the room is used: `levenshtein` 2.5× faster on macOS | 0.4.1 |
| A binary search written as a `settle` over a tuple (`lower` in `topo`, `bfs`, `pagerank`) boxes its state every step: 92% of `topo` (47× C), most of `bfs` (28×), 32% of `pagerank` (18×) | ✅ the state stays in registers: `topo` 5.05 s → 0.79 s on macOS, one core | 0.4.1 |
| A list from `flatten` of many small lists has no known type, so everything after it runs on the generic path (`mim_binop`, `check_index` in each element): `utf8` 15× C. (`diff`, 19×, too: its `edited` is declared `-> [i64]` but has an `[]` branch, and a declared list type was only checked, so its result had no known type; a function declared to return a list of numbers now returns one: `diff` 3.8 s → 0.54 s of CPU) | ✅ `flatten(xs ¨ (x => …))` whose lambda gives a list of numbers is a list of those numbers: `utf8` 4.0 s → 0.71 s of CPU; `flatten` of anything else is still untyped | 0.4.1; 0.4.3, 0.4.4 (the rest) |
| A small fixed list as working state (`chacha20`'s 16 words, changed by `put` in every quarter round): 17× C, as `sha256` | none. Since 0.4.2 the 16 words stay in registers and are not copied, but `chacha20` is still 9–16× C on the CI machines | 0.4.3 (loops with the state in registers) |
| A function returning a constant list (`values()`, `symbols()` in `roman`) builds it again on every call; with the string grown a piece at a time, `roman` is 13× C | ✅ a list of literals is built once and shared (a change to it copies): `roman` 3.2 s → 1.5 s of CPU on macOS; what is left is the string grown a piece at a time | 0.4.1; 0.4.12 (a string builder) |
| The prelude's `count(xs, f)` calls its lambda through the generic path, one core: 32% of `markdown` (11× C) | ✅ `count(xs, x => …)` is written out as its body, `+/ (xs ¨ (x => …))`: typed and fused (`markdown` 1.2×, `maze` 5× faster); a named function or a local `count` as before | 0.4.1 |
| Small values made for every pixel in `qoi` (13× C) and element writes on the generic path in `huffman` (14×; 27% of its time is in a system call not yet traced) | none | 0.4.3; `huffman` to profile further |
| A benchmark's time moving with where its code lands in memory: `crc32` built by 0.3.18 and by 0.4.0 compiles to the same program and calls the same unchanged runtime functions, yet measured 6–9% slower on some Windows machines (a larger runtime moves the code), against a 5% tolerance | measure again on other machines. Tried in 0.4.1 (#116, closed): every function, the runtime's and the program's, started on a 64-byte line; it did not make times independent of placement: on one Windows machine of three the gate found `vm` 61% slower, then on a re-run `sieve` 8–9% slower, the others even (where loops fall inside functions matters, not where functions start) | 0.4.10 (hot code placed by the cost model and profiles) |
| `mimic` itself takes ~0.12 s to start, before reading its arguments (even `mimic --version`), and every build pays it: on macOS about half is the static initializers of the LLVM, Clang and LLD libraries inside it, a quarter the loader binding their weak C++ symbols | none; it is paid once per command | LLVM built with hidden symbols, or a `mimic` that stays running (the fast `mimic run`) |
| `put` into a list of `i64` held once, stored in place without a call (`{ s with v: put(s.v, i, x) }` calls the runtime for every element): faster on Linux and macOS (`heap` 1.35×, `dither` 1.44×), but `huffman` 5–6% slower on Windows, confirmed on two machines; the change (#99) was closed | ✅ in the IR (#125): `{ s with v: put(s.v, i, x) }` stores the number in place, with no call, when the list is held once | 0.4.2 |
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
