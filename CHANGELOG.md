# Changelog

Every user-visible change, by version. The plan for the versions ahead is in [docs/versions.md](docs/versions.md).

## Unreleased

Stage 0.4.0, the measuring stick, so far (0.4.0-preview.2): a benchmark corpus of 54 everyday algorithms (hashing, parsing, graphs, text, interpreters, compression, numbers and simulations), each in Mimic, C and Rust; the machine's limits; and a speed gate that fails CI on a slowdown. The corpus already found bugs and the gaps the rest of stage 0.4 closes.

### Changed
- **The speed gate times short benchmarks longer:** each side runs until it has run for a second (at most 30 runs) before the best time is taken. Benchmarks of 50–130 ms raised false alarms of 6–22% with the best of 3.
- **The gate's noise is measured as the gate compares:** `bench/run.py --noise` now runs the gate's comparison with the same compiler on both sides; a benchmark too noisy for the 25% limit on a platform is reported there, not gated. (The first noise data, ratios to C across whole rounds, overstated it, and a preview's identical compiler failed the gate on `tree` and `ui`.)
- **Benchmarks against C and Rust only:** the Go versions are gone, from the suite and from the tables in BENCHMARK.md and on the website. The http benchmark's load generator is now a Rust program (`bench/http/load/main.rs`).

### Added
- **The 0.4.0 corpus, text:** `levenshtein`, `kmp`, `utf8`, `aho` (Aho-Corasick), `diff` (Myers), `trie` and `wordfreq`, in Mimic, C and Rust with the same algorithms, checked against Python. Work that splits into independent pieces runs close to C on all cores (`kmp` 1.07×), while one byte or cell at a time stays 22–36× on one core.
- **The 0.4.0 corpus, numbers and simulations:** `sieve`, `montecarlo`, `blackscholes`, `heat`, `life`, `fft`, `kmeans` and `bigmul`, in Mimic, C and Rust with the same algorithms, checked against Python (numpy, or its own big integers). Arithmetic over independent elements is at or near C on one core (`montecarlo` 1.0×, `blackscholes` 1.18×, `kmeans` 0.84×) and several times faster on all cores. Writing them found four gaps (docs/gaps.md): a record used inside `¨` is retained and released for every element (20× the CPU time on all cores), `put` on `[u8]` copies, `math::pi` without `()` passes the checker, and an FFT built from `¨` stages is 6× C.
- **The 0.4.0 corpus, graphs:** `bfs`, `components`, `topo` and `pagerank`, in Mimic, C and Rust with the same algorithms, checked against Python. Writing them found two more ways an update is copied instead of made in place (docs/gaps.md): an `if` inside `{ … with … }` (118 s against 7 s) and a helper that changes an array its caller's record holds.
- **The 0.4.0 corpus, interpreters and regex:** `regex` (a Thompson NFA engine, checked against Python's `re`), `brainf` (a Brainf**k interpreter) and `vm` (a stack bytecode VM), in Mimic, C and Rust. An interpreter's loop is 27–54× C today (docs/gaps.md).
- **The speed gate:** `bench/run.py --gate BEFORE` builds every benchmark with the changed compiler and with the one it changes, times them in turns on the same machine, and fails on a slowdown beyond that benchmark's tolerance (twice its noise measured on GitHub's machines, 3–25%, from `bench/noise/`) in 2 of 3 measurements. The bench workflow runs it on release pull requests, nightly on `dev`, and on pull requests that change what is measured, and checks each time that the compiler without its optimizer (`MIMIC_DEBUG_DISABLE=optimizer`) fails it.
- **The 0.4.0 corpus, hashing:** `crc32`, `adler32`, `fnv1a`, `murmur3`, `xxhash32` and `sha256` benchmarks in Mimic, C and Rust, checked against independent references. They show sequential passes over arrays 13–57× slower than C and SHA-256 170× (docs/gaps.md, with the 0.4 steps that fix them), and that Mimic has no `fold`.
- **The 0.4.0 corpus, parsing:** `csv`, `expr`, `tokenize`, `url` and `http_parse`, checked against Python's `csv`, `eval`, `re` and `urllib`. Parsing that splits into independent pieces runs in parallel and is at or near C's time on all cores (`csv` 0.90×, `expr` 0.95×); scanning characters one at a time is 25× C on one core (docs/gaps.md).
- **The 0.4.0 corpus, compression:** `rle`, `base64` and `lz77`, checked against Python (`base64`, and reference implementations). They show where growing a list in a loop copies it: the LZ77 decoder is 476× C (docs/gaps.md). `rle` found `flatten` quadratic (fixed above). `huffman` and `bwt` follow: `huffman` showed a `settle` step written as a lambda copying its state where a named step function changes it in place (37 s against 2.1 s).
- **The share of the machine:** `bench/machine/` measures the peak floating-point rate and the memory bandwidth, on one core and on all, and benchmarks that declare their work (`meta.json`: matmul, mandelbrot, prefix_sum so far) also show what share of that limit each version reached (`bench/run.py --machine` measures again).
- **`bench/run.py --json`** writes a run's times as JSON, and **`--noise N`** runs everything N times unchanged and records how much each ratio to C varies on the machine (`bench/noise/<platform>.json`; the bench workflow's `noise` input runs it on GitHub's machines). It is the first part of 0.4.0: the speed gate's tolerances come from this measured noise.

### Fixed
- **`put` on a list of bytes is in place:** an integer put into a `[u8]` (or any value whose type differs from the list's) copied the whole list every time, even when nothing else held it. The `sieve` benchmark with byte flags, as C keeps them: 2.9 s on one core (3.6 s with `[i64]` flags, over 2 minutes with bytes before) and 0.35 s on all cores.
- Builds running at once could fail with `redefinition of 'MIM_UI_JS'`: each rewrote the runtime's cached sources while others compiled from them, and the C compiler, which tells files apart by inode, could take one file for another once an inode was reused. The sources of each runtime version are now written once, into a directory moved into place whole.
- **`--preview` picks only a release with files:** the releases feed it reads also lists a tag whose release is still being built, so during the minutes a preview was building, `mimic upgrade --preview` chose it and failed with a 404. It now takes the newest preview or stable release whose files are there.
- **No release without its files:** a release is now published only after its binaries and checksums are uploaded. Before, for a moment a new release could be the latest with nothing to download, and the install command failed with "Not Found" (the install scripts' check on the 0.3.18 release did). That check now runs when the release workflow finishes; it never ran on a release before.
- **Builds at the same time no longer spoil the runtime:** files Mimic writes "atomically" (through a temporary file) used a temporary name per process, so two threads of one build writing the same runtime file shared it and could interleave their bytes; CI saw `redefinition of 'MIM_UI_JS'` once. Every write now has a temporary name of its own, the runtime's object file too.
- **`flatten` is linear:** it joined the lists two at a time, copying the growing result again for every list, so flattening many small lists was quadratic (200,000 pairs: about a minute; now milliseconds). Each element is now placed once, in parallel. The same in the `ui` module. Found by the 0.4.0 benchmark corpus.

## 0.3.18 — 2026-11-17

Release channels: previews of the next version and nightly builds, alongside stable releases.

### Added
- **Release channels:** `mimic upgrade --preview` installs the newest preview of the next version (or stable, when it is newer) and `mimic upgrade --nightly` the nightly build of the development branch; the channel is remembered, so later `mimic upgrade`s stay on it, and `--stable` goes back. The install commands take the same options (`sh -s -- --preview`, `$env:MIMIC_CHANNEL = "nightly"` on Windows). Stable stays the default, and previews and nightlies never reach the install command or the website unless asked for.

### Changed
- **How Mimic is developed:** work comes together on a `dev` branch through reviewed pull requests, `main` holds releases only, and a nightly build and weekly previews keep each long step visible ([docs/versions.md](docs/versions.md#how-a-version-is-made)).

## 0.3.17 — 2026-11-17

Install and update Mimic from the terminal, from Mimic itself, and the editor extension with one command.

### Added
- **Install and update from the terminal:** `curl -fsSL https://mimic.codes/install.sh | sh` (macOS, Linux, WSL) and `irm https://mimic.codes/install.ps1 | iex` (Windows) install the latest Mimic or update to it (or say it is the latest), checked against the release's `SHA256SUMS`, with `--version`, `--check`, `--force` and `--uninstall`. `curl -fsSL https://mimic.codes/vscode.sh | sh` (`vscode.ps1` on Windows) installs or updates the extension in VS Code, Cursor, Windsurf and VSCodium. They avoid the update problems met before: a running `mimic` replaced safely, Windows' `tar` instead of `Expand-Archive`, Rosetta terminals on Apple silicon, musl Linux reported clearly, no GitHub API limits. Tested in CI on Linux, macOS and Windows.
- **`mimic upgrade`**: the same from Mimic itself (`--check`, `--version`, `--force`; `--editors` updates the extension too).

### Changed
- The terminal library behind `mimic edit` is updated (ratatui 0.30), which also removes a low-severity advisory in one of its dependencies (`lru`).

## 0.3.16 — 2026-11-17

A speed round on byte, pixel and state-machine code (found with four new benchmarks and a video transcoding example), types on lambda parameters, and `os::run_in`.

**Upgrading from 0.3.15:** run the new release's install script (or update from the VS Code extension 0.10.5). Programs do not need to change. Release builds now change lists inside records in place where nothing else holds them; debug builds still copy, so a long state-machine loop is much faster with `mimic run --release`.

### Added
- **Example: transcoding a video into several resolutions** (`examples/transcode`): ffmpeg alone, and ffmpeg decoding and encoding around Mimic's own scaler (every plane of every frame, one parallel map), timed per resolution; the same in JavaScript (`transcode.mjs`, worker threads) and a comparison (`compare.mjs`) that checks both produced the same bytes. Scaling 60 frames of 1080p to 720p, 480p, 360p and 240p: Mimic 1.23 s on all cores, 6.43 s on one; JavaScript 4.06 s and 12.09 s; C on one thread 5.00 s.
- **Benchmark `scale`**: video frames resized bilinearly (C, Rust, Rust + Rayon, Go, Mimic).
- **Benchmarks `filters`, `qoi` and `paths`**: a 1080p image through box blur, Gaussian, Sobel, median and bilateral filters; a QOI encoder and decoder; Dijkstra and A* with a binary heap on a 600×600 grid (C, Rust, Rust + Rayon, Go, Mimic; the same output from each).
- **Types on a lambda's parameters**: `(w: f64, h: f64) => w * h`, checked like a function's (error E0401 names the parameter).
- **`os::run_in(dir, program, arguments)`**: `os::run` in another working directory; a missing folder is an `Err` that names it.

### Changed
- **Byte data is fast.** Lists of bytes made by a fused loop are written with SIMD too. Reading an element of a `[u8]` was a call into the runtime that allocated a number (now one load: the scale benchmark went from 10 s to 1.4 s); `bytes::from(…)` over a computed list writes the bytes in one pass, without a list of i64 in between; sums, products, maxima and minima of integer lists run as tight loops. The sum of a `[u8]` is an `i64` (it was typed as a list, which sent the code around it to slow paths).
- **Loops whose elements are big pieces of work use every core**: `⍳ 30 ¨ frame` where each `frame` works over a million pixels ran on one core, as 30 elements looked small. An element whose function itself loops over arrays is now timed and shared out (2 → 6 of 12 cores in the scale benchmark).
- **The function a fused `¨` applies goes inside the loop** (inlined), so its checks that do not depend on the element (divisors, the lists' headers) leave the loop.
- **Integer `div` and `%` of numbers that are not negative take a plain divide**, without the correction that rounding towards minus infinity needs (the results are the same); LLVM drops or hoists the test where it can. The transcode example's scaler: 8.8 s → 6.4 s on one core. (Replacing a divide by a precomputed reciprocal, as C compilers do for constant divisors, was tried for divisors fixed in a loop: slower on both Apple's arm64 cores and AMD's x86 ones, whose dividers are fast; left out.)
- **`let (a, b) = …` of numbers stays in registers**, from a tuple or an `if` whose branches are tuples: in a loop over pixels it made, checked and released a tuple per pixel (the transcode example's scaler: 116 s → 0.9 s).

- **State machines change their lists in place.** `{ s with xs: push(s.xs, x) }`, `concat` and `put` (and chains such as `put(put(s.xs, i, a), j, b)`) take the list out of a record held once and change it there, growing it with room to spare, instead of copying it each time; a list still held elsewhere is copied as before, so results never change. `settle` and `evolve` hand their state to each step (a record held once), `settle` with a typed step is a compiled loop (it went through the runtime), and a function takes its record parameter over when each `with` on it, or a call it is passed to, is its result on that path. Dijkstra with a heap on a 600×600 grid went from not finishing in 10 minutes to 0.4 s; a QOI encoder from hours to 1.4 s.
- **Small pure helpers stay native.** A pure function making several calls at once (`slot(p)` calling `red`, `green`, `blue`) went to the interaction nets even without any recursion in reach, where each call cost thread wake-ups; the nets are kept for work that can grow.
- **Lists of up to 16 numbers stay in registers** (it was 8): a 3×3 neighbourhood or a 4×4 matrix allocates nothing. `sort` of such a list sorts on the stack; `sort(⍳ 9 ¨ f)[4]` (a median) allocates nothing; the runtime sorts lists of up to 64 numbers with an insertion sort (the radix sort allocated and cleared buffers). The median filter: 5.5× → 1.6× C on one core, on par with Rust + Rayon on all cores.

### Fixed
- **A fused loop shared out over the cores could compute wrong numbers and crash** when it used a list or a record small enough to be kept in registers (`nbody`'s bodies): such a value is wider than one slot of the kernel's context and overwrote the next ones. It now goes by address. (Found when today's change sent more loops down that path; a small constant loop like `⍳ 5` is not counted as heavy work.)
- **`panic` and `os::exit` take no part in a value's type**: `match fs::read_bytes(p) { Ok(b) => b, Err(e) => panic(…) }` was dynamic (the arms joined bytes with nothing), so everything computed from it ran on slow paths; it is `[u8]`.

## 0.3.15 — 2026-11-16

Updating while an editor is open works again, `and`/`or` short-circuit, servers can be profiled and stopped, lists of number-only records are 9× faster, and fixes from first users' notes.

**Upgrading from 0.3.14:** if updating from the VS Code extension failed ("Killed: 9"), update again: this release's installer replaces `mimic` safely while the language server runs. In VS Code, install the extension 0.10.3 for the profiler's Cancel. One change can stop a program from compiling: standard functions now check their arguments' types (error E0401), so a call that passed the wrong type, and would have failed when run, is reported.

### Fixed
- **Standard functions check their arguments' types.** `to_float(5)` compiled and stopped the program when it ran (`to_float` reads text); it is now error E0401, as for the program's own functions.
- **`match` on a list of pairs**: `[]` and `[(k, v), ..rest]` cover every list, with no `_ => …` arm needed (a tuple of names matches every pair).
- **Clearer messages**: a `{` in a string that does not parse as code says that `{` starts an interpolation and to write `\{` for a brace; a syntax error says that names and types are checked once the file parses; `mimic check src/some_module.mim` inside a project checks the project (it said `mimic run <file>` expects `module main`); `mimic create mimic-ls` makes the project `mimic_ls` in the folder `mimic-ls` (it refused the name).
- **`and` and `or` short-circuit.** `n > 0 and xs[0] == "x"` read `xs[0]` even when `n` was 0, and crashed on an empty list; the right side now runs only when it decides the answer, as in C and Rust. (On lists they still work element by element.) A pure function whose `and`/`or` has a right side that could fail or recurse (an index, a call, a division) is no longer put on the interaction nets, which evaluate both sides.
- **Profiling a server or a long program can be stopped.** Ctrl-C during `mimic profile` stops the program and still shows the report of the time it ran (before, the profile was lost). `--stop-file FILE` does the same when a file appears: the VS Code extension's profiling notification has a Cancel that stops the program and shows its profile, and `mimic mcp`'s `profile` tool stops a program still running after its `timeout_s` and reports up to there.
- **Updating Mimic while an editor is open.** On macOS the installer copied the new `mimic` over the old file, which the editor's language server was running; macOS keeps the signature it checked for that file, so the new `mimic` was killed when it started ("Killed: 9", also when updating from the VS Code extension). The installer now writes a new file and moves it into place. On Windows, where a running `mimic.exe` cannot be overwritten, the old one is moved aside first.

### Added
- **`math::to_int(x)`**: a float's integer part as an `i64`, towards zero (`math::to_int(math::round(x))` rounds). There was no way to turn a float into an integer. NaN, or a number no `i64` holds, stops the program.
- **New tests**: image filters (box and Gaussian blur, sharpen, Sobel, Prewitt and Scharr edges, the Laplacian, median and bilateral filters, and pipelines of them), a QOI image encoder and decoder, and Dijkstra and A* shortest paths; each checked against an independent implementation (byte for byte for QOI).

### Changed
- **Lists of records with only number fields are 9× faster.** A record whose fields are all numbers (`{ id: i64, x: f64, v: f64 }`) lives in registers; in a list it was boxed field by field on every step (each number allocated on its own, then copied into the record), and `¨` over such a list was not fused. Now a chain such as `ps ¨ step ¨ step ¨ step` is one pass with each record in registers, the sums after it are fused too, and putting a record back into a list stores its numbers directly. A million records moved five times and summed: 2.09 s → 0.23 s on one core, 26 million allocations → 2 million. Found with `mimic profile`.

## 0.3.14 — 2026-11-16

Faster servers, parallel recursion and prefix sums, and a `mimic edit` where everything is clickable. On GitHub's machines Mimic on all cores is now even with or ahead of Rust + Rayon on most benchmarks ([BENCHMARK.md](BENCHMARK.md)).

**Upgrading from 0.3.13:** run the new release's install script. Nothing in programs needs to change.

### Changed
- **`mimic edit` is easier to use: everything is clickable.** A toolbar (Run, Test, Check, Audit, Find, Glyphs, REPL, Manual, ≡ Commands; icons only on a narrow terminal), a command list with every command and its key (F2, filtered as you type), folders in the file tree that open and close (`src/` and the open file's folders start open), tabs that switch and close with their ×, pane tabs, `file.mim:12` in any pane that jumps to the line, the error count and the hints in the status bar, the start screen's lines, and the items of every pop-up list. The wheel scrolls whatever is under it.
- **Prefix sums are faster: 0.28 s → 0.17 s** (100 million numbers, 12 cores). A scan's chunks keep their fixed boundaries (results never depend on the number of threads), but each thread now takes one contiguous run of them, so the fresh output's pages are faulted in one region per thread instead of interleaved.
- **Parallel recursion is faster: `fib` 103 → 80 ms** (median, 12 cores). A function that splits into calls that run at once (`fib(n - 1) + fib(n - 2)`) now runs as fork-join when called from ordinary code, with less overhead than the interaction net; called from code already on the net, it stays there. Below the fork depth its calls run the plain sequential version, as on one thread, not the one that counts its depth on every call: on x86 that counting made parallel `fib` spend ~13% more CPU than on one thread (found on GitHub's Linux machines with `perf stat` and pinned runs).
- **Servers are faster: 1.7–1.9× C → 1.10× C** (the HTTP benchmark, all cores, M3 Pro). Every task waiting on a socket took one lock shared by all the event loop's threads, and held it through system calls (duplicating the descriptor, registering it with kqueue or epoll, closing it); under load a third of a server's busy time went to waiting for it. Socket waits now register, fire and close outside the lock, which only guards the timer heap; a wait with no deadline needs no lock at all. Found with `mimic profile --for`.

## 0.3.13 — 2026-11-16

Stage 0.3 complete: the Workbench. The tools around the language, for people, editors and AI agents: the manual inside `mimic`, a language server, a VS Code extension, debugging from editors, an MCP server for agents, documentation written by people and agents, where code runs and why, an audit for quality and security, contracts and property tests, Mimic's own terminal editor, a REPL, a profiler, interaction nets you can see, and coverage.

**Upgrading from 0.2.9:** run the new release's install script (it replaces the old `mimic` and clears its build cache). In VS Code, install the [Mimic extension](https://github.com/mimiccodes/mimic-vscode/releases) 0.10.2 or newer; it finds this `mimic` by itself. Libraries can now be pinned to a tag or commit (`mimic install <git-url>@v1.2.0`, `rev =` in `mim.mim`).

### Added
- **The Workbench engine and `mimic query`** (step 0.3.0): one analysis of the project that every tool will ask. `mimic query` answers as JSON: `check`, `symbols`, `definition`, `hover`, `references`, `outline`, `docs`, `effects` (pure, concurrent, ordered or sync) and `stats`. `mimic query --serve` keeps it running, reading one JSON request per line; files sent by an editor (`open`, `change`, `close`) are used instead of the disk, and only changed files are parsed again: an edit of the showcase project is re-checked in about 25 ms. While the program has errors, lookups use the last program that checked. See [docs/workbench.md](docs/workbench.md).

- **Libraries at a chosen version, from git** (step 0.3.1). `mimic install <git-url>@v1.2.0` installs a tag, branch or commit next to other versions (`~/.mimic/libraries/<name>@v1.2.0`), and `rev = "v1.2.0"` in a dependency of `mim.mim` picks it. `mimic install` with no URL, inside a project, installs every missing dependency that has a `git` URL (and theirs), at its `rev` or at the commit `mod.mim` locked, so a fresh clone builds with the same library code. `mimic update` moves a library to the newest commit of its branch and leaves tags and commits where they are.
- **Workspaces:** a `mim.mim` with `workspace { members = [...] }` groups projects; `check`, `build`, `test`, `fmt`, `install` and `mod update` in its folder run in every member, and all members build into its one `target/`.

- **`mimic man`, the manual inside `mimic`** (step 0.3.2): the tutorial, the reference and new guides, embedded in the binary, plus a page made from the compiler for every standard function (243) and module, every glyph (`mimic man ⌿`, `mimic man keep`, `mimic man \k`) and every error code. `--search` finds every line that mentions a text, `--run` runs a page's examples and compares their output, `--json` prints a page for tools. Every error message now ends with the page to read (`For more about this error: mimic man E0303`).
- **Guides:** "Coming from C", "Coming from Python", "Coming from JavaScript", and "Writing Mimic for agents" (the workflow, the idioms, and the mistakes ported code makes, with their error codes), in `docs/manual/`. Their examples are checked like the tutorial's.
- **Examples in comments are tests:** a `// > expression` line and the value `print` would show on the next comment line run under `mimic test`.

- **Syntax highlighting** (step 0.3.3): TextMate grammars for `.mim` and `.mnet` in the new [mimic-syntax](https://github.com/mimiccodes/mimic-syntax) repository (a submodule at `editors/syntax`), for VS Code, Sublime Text and other editors, and later GitHub. Names from C (`c::`, `cpp::`, `as` names) get a scope of their own. A tree-sitter grammar with highlight queries for Neovim, Helix and Zed. A golden test checks that the grammar highlights every glyph, word, digraph and keyword of the compiler, that every test program and example tokenizes with nothing unrecognised, and that tree-sitter parses them all with no error. `mod.mim` is marked as generated for GitHub.

- **`mimic lsp`, the language server** (step 0.3.4): errors as you type with the compiler's fixes as quick fixes; hover with signature, comment and effects (for C functions, their header; for glyphs and their words, what they do and how to type them); go to definition and find references, into the standard library; glyph input (`\i` → `⍳`) and completion of names, modules, words and keywords; formatting in the style the file mostly uses; the outline and workspace symbols; names from C as semantic tokens with the modifier `foreign`. Set-up for VS Code, Neovim, Helix, Emacs, Sublime Text and Zed: `mimic man editors`.

- **The VS Code extension** (step 0.3.5), in the new [mimic-vscode](https://github.com/mimiccodes/mimic-vscode) repository (a submodule at `editors/vscode`): everything of `mimic lsp`, the highlighting and file icon, a glyph palette, **Run** and **Run test** above functions and tests, the Test Explorer (tests and examples in comments, with both sides of a failed `expect`), the manual beside the code, conversion between glyphs and words, and a check that `mimic` is new enough. CI runs it in a real VS Code against the `mimic` it builds.

- **Debugging shows Mimic's values** (step 0.3.6). `mimic lldb` shows text, lists, maps (`["a": 1, "b": 2]`), records (also those kept in registers, which had no debug information before: `Point { x: 1.5, y: 2.0 }`), lists of numbers as `[3 1 4]` and variants as `Some(7)`, without the handles' addresses. `mimic dap` debugs from editors over the Debug Adapter Protocol (through LLVM's `lldb-dap`): breakpoints on `.mim` lines, stepping, the call stack, and the same values; in VS Code, F5 or **Debug** above `fn main`.

- **`mimic mcp`, Mimic for AI agents** (step 0.3.7), over the Model Context Protocol: tools to check (every error with its exact fixes, as JSON), build, run (with input and a timeout), test, format, explain errors, read the manual and search it, look up symbols and effects, and read and write the project's files; resources (the guide for agents, the glyph table, every page of the manual) and prompts (write Mimic, port code to Mimic, fix the errors). Full access inside the project; outside it, files (`--allow-outside`), installing (`--allow-install`) and running programs that use the network or run other programs (`--allow-network`) stay off until allowed; `--read-only` and `--no-run` narrow it. Set-up for VS Code, Cursor, Windsurf, Zed, Codex CLI and any MCP app: `mimic man mcp`.

- **Documentation written by people or agents** (step 0.3.8). Guides, Markdown pages in the project's `docs/` folder, join `mimic doc` beside the API; their `mimic run` examples run under `mimic test` against their `output` blocks. The site is new: guides and modules in the sidebar, Mimic code coloured, every glyph explained on hover, Mimic's colours in light and dark; `mimic doc --std` includes the manual's guides. Through `mimic mcp`, agents see what is undocumented (`docs_status`), write guides whose examples are checked as they are written (`docs_write`), and build the site (`docs_build`); the `document_project` prompt starts the job.

- **Where code runs** (step 0.3.9). `mimic why` lists every decision the compiler makes about a program or one function: `¨` on all cores, fork-join recursion, interaction nets, fused loops, SIMD, the GPU, implicit tasks, effects kept in order and the places that wait for tasks, each with its reason and what would change it. Code generation records these decisions as it makes them, and `mimic why` reads them from a compile that stops before LLVM optimises. Editors show them as labels after the code (`∥ parallel`, `⇶ fused`, `▣ GPU`, `◷ task`, `→ in order`) with the reason on hover, and agents ask with the `why` tool of `mimic mcp`.

- **`mimic audit`** (step 0.3.10): quality rules (unused functions and variables, dropped `Result`s, long, complex and duplicated functions, C functions that could be `pure`, `trusted` C functions taking pointers) and security rules (`os::shell` with commands built from data, files opened with paths from HTTP requests, secrets in source, certificate trust outside tests, `unsafe` blocks, trusted headers, dependencies following a branch), each with a manual page (`mimic man S01`). Reports as text, JSON or SARIF; `--fail-on` makes it a gate for CI (errors by default); a baseline counts only new findings; each run's counts are kept and compared. Agents get it as the `audit` tool, and CI audits the examples.

- **Contracts and property tests** (step 0.3.11). `requires` and `ensures` lines before a function's body (`result` is the value returned) are checked on every call in debug builds and tests, also when the body leaves with `return`, and left out of release builds; a broken one stops with the condition, the function and the line, and they show in signatures (hover, `mimic doc`). `test "…" for xs: [i64] { … }` runs the body for 100 generated inputs (`i64`, `f64`, `bool`, `str` and lists of them), the same on every run, and shrinks a failure to its smallest case. Both grammars know `requires`, `ensures` and `for`.

- **`mimic repl`** (step 0.3.13): Mimic a line at a time. An expression shows its value, a `let` keeps it, a `fn` or `type` stays defined, and an open bracket continues on the next line. Glyph input as you type (`\i` → `⍳`) and history with Up and Down; `:show` prints the session as a program, `:reset`, `:man`, `:glyphs`, `:quit`. Every line is compiled to native code and run; a line that does not compile shows its errors and is not kept. In `mimic edit`, the same REPL is a pane: F4 evaluates a line you type, Ctrl-E the current line or the selection.

- **The language server finds its way around**: signature help, rename (the program's own names), the call hierarchy (who calls a function, what it calls), smart selection, the inferred type of each `let` as an inlay hint with a code action that writes it in, and *Extract into a function*. `mimic query types` gives those types to other tools.
- **`mimic profile --for SECONDS`**: profiles a server (or any program that does not end by itself) for that long, timed by the program. Servers are profiled properly: the event loop's threads and the tasks on them are sampled, and waiting for the network, for work or in `time::sleep` counts as waiting.
- **Targets:** `aarch64-windows` is stable (CI runs every golden program on a Windows arm64 machine).
- **`mimic test --coverage`**: how often the tests ran each line of the program's own code, lines of functions nobody called included: a summary per file with the lines not run, `coverage.json` for editors (the VS Code extension's Coverage run) and `lcov.info` for coverage services.
- **`mimic repl --json`**: requests and answers as JSON lines, for notebooks and tools (the VS Code extension's `.mimnb` notebooks).
- **Windows**: the language server compared Windows paths in their `\\?\` form with the paths editors send, so errors and lookups could miss open files; paths are now compared and reported as editors write them. A test of the language server that hung for two hours on Windows CI (it waited for an answer to a malformed path) now finishes, and any test talking to a server stops it after three minutes without an answer.
- **`mimic net show` and `mimic net trace`**: the interaction nets of a program's pure functions (or of a `.mnet` file) as text or as graphs (`--json`: nodes, ports, wires, active pairs), also as compiled before compile-time reduction (`--before`); and a call reduced round by round with the runtime's rules, giving the result, the interactions, the redexes of each round (the parallelism the net offers) and the rules that fired, with the net before each round for drawing (`--frames`). The VS Code extension draws and plays them.
- **The VS Code extension 0.4–0.10**: finds Mimic by itself (VS Code started from the Dock does not see the PATH of `~/.zshrc`, so it asked for `mimic.path`), installs and updates it from GitHub, shows errors on save with a Mimic without the language server, profiles (a panel, shares in the code, hot functions), lists where code runs, runs the REPL, and shows and traces interaction nets.
- **`mimic profile`** (step 0.3.13): runs the program and shows where its time goes: the share per function (self, and with what it calls) beside where the compiler runs it, the runtime and system functions that took it, each thread's share of time working, GPU kernels and their time, and memory allocated and copied. The runtime samples every thread about 1000 times a second (signals on Linux and macOS, suspending the thread on Windows) and follows the frame pointers a profiling build keeps; time on the pool's workers counts for the function that started the loop. The report is saved as JSON; `--json` prints it, and `mimic mcp` has a `profile` tool. `mimic man profile` explains it.
- **Tail-recursive loops stay native.** A pure function whose calls are all in different branches (a loop written as tail recursion) no longer runs on the interaction nets, where it gained nothing: it compiles to a loop, as fast as the same loop in C (a 400-million-step loop: 20 s on the nets on 8 cores' worth of CPU, now 10 s on one core, like C). Found with `mimic profile`.
- Fused kernels are named after the function they come from (`…main__f$i$kernel3`), in profiles and debuggers.

- **Rust + Rayon in the benchmarks** (step 0.3.13): `bench/<name>/rayon/` is the hand-parallelised Rust version of seven benchmarks, timed on all cores next to Mimic on all cores in `bench/run.py`, `BENCHMARK.md` and the weekly CI run. Cargo projects in the suite now build with `-C target-cpu=native`, as the single-file Rust versions do. Mimic is ahead on six of eight with no annotations; Rayon is ahead on prefix sum and fib.

- **`mimic edit`, Mimic's own terminal editor** (step 0.3.12): in Mimic's colours, with a MIMIC start screen; highlighting from Mimic's own lexer (names from C in their own colour); glyph input (`\i` → `⍳`, never inside strings) and a glyph palette; errors as you type from the engine (a mark, the code underlined, the message); panes for problems, the audit, where code runs (also as labels beside the code), the manual for the word under the cursor, and run and test output (in the background); tabs, a file tree, a fuzzy file opener, find and replace, undo and redo, the clipboard, the mouse, indentation that follows the code. Easy keys: Ctrl-S, Ctrl-Q (asking about unsaved files), Ctrl-O, Ctrl-G, F1, F5, F6.

### Changed
- **`mimic check` is about 2.5× faster** (the showcase: 90 ms → 35 ms of analysis). Name lookups are remembered once the module tree is built, and names that turn out to be local variables no longer run the "did you mean" search, which only an error needs.

## 0.2.9 — 2026-11-15

Stage 0.2 complete: native windows on Windows and Linux, shared libraries, thousands of waiting tasks on the event loop, terminal programs, text formats, collections, C constants, globals, structs and unions, and Windows at full speed.

**Upgrading from 0.2.0:** names from C are now written with `c::` (`c::printf`, `c::sqrt`). Run `mimic check --fix` in each project to add it where it is missing. The install script replaces the old `mimic` and clears its build cache.

### Changed (breaking)
- **Everything from C is written with the namespace `c::`:** `c::printf(…)`, `c::sqrt(2.0)`, `c::Vec2 { … }`, for functions from `extern "C"` blocks, imported headers and C struct records alike. It is required, so C code is always visible, and C names never collide with Mimic's (`c::abs` next to `abs`, `c::div` next to the `div` operator). `extern "C" include "sqlite3.h" as sql` picks another namespace. A C name used without it is error E0303 with the fix (`did you mean c::floor?`). C++ will use `cpp::`. To update code: put `c::` before each C function and struct name.

### Fixed
- **Windows programs run at full speed.** Native Windows builds compiled for the plain x86-64 baseline (no AVX2, no FMA), because the MinGW host triple picked up Clang's cross-compiling defaults; they now use the machine's own CPU, as on macOS and Linux. On a Zen 3 runner: `mandelbrot` 1.14× → 0.76× C on one core, `sum_of_squares` 4.1× → 1.1×. CI now checks which CPU Windows code generation used.
- Windows: idle worker threads napped with `Sleep(0)`, which returns at once, so they spun on the cores the working threads needed; they now sleep 20 µs on a high-resolution timer (`fib` on all cores 0.89× → 0.68× C).
- Parallel prefix sums (`trail`) were slower on several threads than on one on Windows: scans now work in larger chunks (Windows 1.46× → 0.84× C on 4 threads; macOS and Linux a little faster too).
- A rare hang on Windows: two tasks using one socket (a WebSocket's reader and writer) could switch its non-blocking mode under each other, and a read then blocked a whole event-loop thread. Sockets used by tasks now stay non-blocking.
- A coroutine's lock could be freed while the thread finishing it was still releasing it (harmless with glibc, not with Windows' threads library).
- `time::format`'s message for an unknown pattern code was cut off.
- `bench/run.py` writes its results as UTF-8 on Windows.
- Tests: `http_keep_alive` takes its port below the ones the OS hands out (it could race with a parallel test); `c_struct_pointers` uses `termios.h` only on Unix.
- `"\033"` and other C octal escapes were read as NUL followed by digits, silently; they are now an error that suggests `\x1b`.
- Importing a C header failed when it declared a name Mimic uses itself (`div` in `stdlib.h`, a field `type`, a function `sum`). Under `c::` (below) they keep their C spelling; a wrapper that cannot be written in Mimic is left out instead of failing the import.

### Added
- `MIMIC_DEBUG_WATCHDOG=<seconds>`: a program still running after that long prints every task and what it waits for (a channel, a join, a socket, a timer), to find hangs. `MIMIC_DEBUG_CPU=1` prints the CPU code generation chose. `tools/speed-probe.py` and the manual "speed probe" workflow time benchmarks against C by thread count on CI machines.
- Native windows on Windows and Linux: `ui::run(…, show: "window")` (`mimic run --window`) uses Win32 controls on Windows and GTK 3 on Linux (loaded when the window opens, so programs do not depend on it), as AppKit on macOS. The same window test runs on all three in CI, Linux under a virtual display.
- Shared libraries: `mimic build --shared` builds a library project as `lib<name>.dylib`, `lib<name>.so` or `<name>.dll` (with the import library `lib<name>.dll.a`), exporting only its `extern "C" fn`s, for any language that calls C. Cross builds too (`--target x86_64-windows`).
- `¨` over a function that waits on the network or sleeps runs every element as a task on the event loop instead of a core's worth at a time: 3000 WebSocket clients each holding a connection for half a second finish in about a second (was two minutes). Programs raise their open-file limit at start, so thousands of sockets fit.
- Speed checked on Windows: CI counts AVX2/AVX-512 instructions in a native Windows build of the SIMD fixture, and the benchmark workflow runs on Windows too (`bench/run.py` works there).
- C unions: a record with one field per member (`c::Num { f: 2.5 }`), by value, in structs and through pointers; the member set is the one C sees.
- C structs: array fields are lists; `c::T { … }` leaves fields out as zero (`c::Termios {}`); plain-data structs through pointers are records (`const`: copied in; otherwise copied in and returned, in a tuple with the result), so `c::tcgetattr(0, c::Termios {})` works. Records exist for every C struct the code names.
- `mimic check --fix` adds a missing `c::` (functions and constants).
- C constants and globals: `#define` constants and enum values (`c::SEEK_END`, `c::M_PI`, `c::TIOCGWINSZ`), computed by Clang and inlined; globals and expression macros (`c::stdout`, `c::errno`) read through generated getters; `#define` aliases of functions (`c::CreateFile`); C functions with uppercase names (the Windows API). Struct, callback and getter wrappers are generated only for the C names a program uses.
- Collections: `unique`, `sort_by` (stable, `descending: true`), `zip`, `enumerate`, `any`, `all`, `count`, `group_by`, `flatten`, `min_by`, `max_by`, `clamp`, `to_map`, and a `set` module (`set::of`, `has`, `add`, `remove`, `union`, `intersection`, `difference`).
- `panic(message)` and `assert(ok, message)` in ordinary code; `os::env_all()`; `fs::walk(dir)` and `fs::modified(path)`; `time::format(t, pattern)` (`%Y-%m-%d %H:%M`); `random::choice` and `random::shuffle`.
- Terminal programs without C: `io::write` (no newline), `io::flush`, `io::read_char`, `io::read_bytes`; the `term` module (`raw`, `read_key`, `size`, `is_terminal`, `clear`, `move_to`, `show_cursor`, `alternate_screen`, undone when the program ends), on Windows too (ANSI codes are switched on); `os::on_interrupt` for Ctrl-C.
- Formats in interpolation, as in Python and Rust: `"{x:.2}"`, `"{n:>8}"`, `"{n:06}"`, `"{n:#x}"`, `"{n:,}"`, `"{x:.1%}"`; `format(x, spec)`. A bad format is a compile error.
- `char_code`, `char_of`, `pad_start`, `pad_end`, `words`, `slice`, `parse_int(text, base)`.
- `docs/gaps.md` and `tools/gaps.sh`: everyday features checked against the compiler, each with the version that closes it.
- String escapes `\x1b` (an ASCII character by two hex digits) and `\u{1F600}` (any Unicode character), so terminal colour and cursor codes can be written.
- TLS in static Linux programs: `https://` and `tls::` work in `--target x86_64-linux` (and every other `*-linux-musl`) builds, through Mbed TLS 3.6 bundled in `mimic` and built for the target on first use. They trust the system's CA bundle, or `SSL_CERT_FILE`.
- The Linux `mimic` is built against glibc 2.28, so it runs on Linux systems from 2018 on (Ubuntu 20.04, Debian 10, RHEL 8 and later).
- `NOTICE` lists the software bundled in `mimic`; release downloads include it.
- The event loop on Windows: connections and the tasks they start run as coroutines (fibers), waiting on sockets through IOCP (wepoll) instead of a thread each, as on macOS and Linux.

## 0.2.0 — 2026-11-14

Downloadable builds for Linux, macOS and Windows; Windows as a target and as a host; static Linux and Windows programs proven in CI.

### Added
- `mimic --version` shows the commit and the LLVM version.
- `MIMIC_GPU_DEBUG=1` reports whether a Metal GPU was found.
- Windows programs from any machine: `--target x86_64-windows` (and `aarch64-windows`) builds `.exe` files with the bundled mingw-w64 (UCRT); `mimic cc` compiles and links C for Windows too. Threads, implicit futures, files, time and processes work.
- Networking on Windows: TCP, UDP, HTTP, WebSockets and the browser UI through Winsock; TLS (`https://`, `tls::`, `serve_tls`) through Schannel, checked against Windows' trust store; `tls::self_signed` keys through BCrypt.
- `os::shell` uses the system shell: `/bin/sh -c`, or `cmd.exe /c` on Windows.
- CI builds the golden programs for Windows on Linux and runs them on a Windows machine (`tools/build-fixtures.sh`, `tools/run-fixtures.sh`).
- Downloads: a version tag builds `mimic` for Linux x86_64, macOS arm64 and Windows x86_64 and publishes them as a GitHub release with checksums. Each download has an install script (`install.sh`, or `install.cmd` on Windows) that puts `mimic` on `PATH` and checks what the system needs.
- Stable targets: static Linux x86_64 and arm64 (`x86_64-linux-musl`, `aarch64-linux-musl`) and Windows x86_64, whose golden programs CI now runs. `mimic targets` lists the Windows targets.
- `mimic` builds on Windows (MSVC Rust, a Windows LLVM from the llvm workflow); native builds there target MinGW (`x86_64-windows-gnu`) with this machine's CPU features. CI builds it and runs the golden programs it makes.

### Fixed
- Windows: each socket call ran once, not twice (binding, sending and receiving failed or repeated); `os::pid()` is the process id; listening ports are claimed exclusively; Winsock errors read as words.
- `time::sleep` never returns early, also where the clock wakes a little early (WASI hosts).
- The C version of the HTTP benchmark runs on Linux (epoll).
- Linux: C code (the runtime, imported headers) is compiled by the embedded Clang; the system compiler only links. Header import failed with GCC.
- Linux: a runtime function clashed with a glibc maths function (`jnl`).
- Linux: unused runtime code is dropped at link time, so raw `.mnet` programs link.
- x86-64 release builds compile the runtime for this CPU with `-march=native`.
- LLVM's target list is registered once per process (concurrent registration could hang).
- compiler-rt builds for the same target in several processes at once no longer remove each other's files.

## 0.1.0 — 2026-11-14

The first version: the foundations listed in [docs/versions.md](docs/versions.md#01--foundations).

- **Language:** arrays with glyphs and words, records, unions, generics, closures, functions as values, loop words, patterns, implicit futures, `test` blocks.
- **Parallel by default:** fused array kernels on all cores, `¨` in parallel, fork-join recursion, interaction nets, SIMD across elements, Metal GPU kernels.
- **Standard library:** files, paths, processes, time, random numbers, JSON, HTTP and HTTPS, TCP, UDP, TLS, WebSockets, channels, an event loop, a browser UI.
- **C interop:** headers imported directly, structs by value, varargs, callbacks, C sources in projects, Mimic functions exported to C.
- **Toolchain:** one `mimic` binary with Clang and LLD inside; native macOS and Linux; cross builds to musl Linux targets, WebAssembly (WASI) and Apple platforms; `mimic test`, `fmt`, `doc`, `lldb`, `explain`; JSON diagnostics.
- **CI:** Linux x86_64 and macOS arm64 on every push; weekly benchmarks.
