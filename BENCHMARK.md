# Mimic benchmarks

Mimic aims to be the fastest programming language. This file defines what that claim means and how it gets measured, and it records where Mimic stands today, including where it is slow. Numbers here are only ever **measured**, never estimated.

## Rules

1. **Like for like.** Every benchmark exists in Mimic, C and Rust. Each version uses the same algorithm, written idiomatically for that language, with no hand-written assembly. Every version prints the same result, and the runner checks it.
2. **Fastest build of each language:**

   | Language | Build command |
   |---|---|
   | Mimic | `mimic build` (release is the default) |
   | C | `mimic cc -O3 -mcpu=native` (the Clang inside `mimic`) |
   | Rust | `cargo build --release` with `-C target-cpu=native` |

3. **Two scores per benchmark:**
   * **Single-core** (`MIMIC_THREADS=1`), which measures code quality.
   * **All cores**, the default, which measures what users actually get. The other languages run their straightforward version, which for C and Rust is usually sequential. Mimic's parallelism is automatic, so it counts.
   * **Rust + Rayon, all cores**: the hand-parallelised Rust version (`bench/<name>/rayon/`, the same algorithm with Rayon's parallel iterators, `join` or `par_sort_unstable`), next to Mimic on all cores. It shows what parallel code written by hand gets, against Mimic's parallelism without annotations.
4. **Statistics:** each measurement is the best of 5 wall-clock runs, after one warm-up run. On macOS the first launch of a new binary is slowed by a security scan, and the warm-up absorbs it. The machine, OS, CPU model and load average are recorded with every table.
5. **Score:** each result is reported as a ratio to C (`time / C time`). Below 1.0 means faster than C.
6. **Noise:** `bench/run.py --noise N` runs the speed gate's own comparison (rule 8) N times with the same compiler on both sides, so every difference is noise, and records each benchmark's widest move on the machine (`bench/noise/<platform>.json`; the bench workflow's `noise` input runs it on GitHub's machines). A first measurement of the ratio to C across whole rounds, minutes apart, overstated it (a median of 26% on macOS): the gate compares runs seconds apart. Measured this way on GitHub's machines (8 rounds, each machine a third of the corpus, 2026-11-19): a median of 1.7% on Linux, 2.0% on Windows and 5.2% on macOS (virtual M1 machines), so the gate is tight on Linux and Windows; 17 of macOS's 108 measurements (54 benchmarks, one core and all) are too noisy to gate there, and 2 of Linux's and 1 of Windows's.
7. **Share of the machine:** `bench/machine/` measures the machine's limits once: the peak floating-point rate (fused multiply-adds on the widest registers) and the memory bandwidth (the STREAM triad), on one core and on all. A benchmark whose work counts against one of them declares it in `meta.json` (`{"bound": "compute", "flops": N}` or `{"bound": "memory", "bytes": N}`), and its results also show as a share of that limit, so a result that matches C but leaves most of the machine idle is visible. Benchmarks limited by latency, branches or allocation (fib, tree, http, json, …) have no such share. Above 100% means hardware beyond the CPU's vector units did the work (Apple's matrix unit, later the GPU).
8. **The speed gate:** a change's compiler is compared with the compiler it changes, both built in the same CI job and timed in turns (before, after, before, after, …) on that machine, on one core and on all; `bench/run.py --gate BEFORE` does it. A benchmark slower by more than its tolerance in 2 of 3 measurements is measured 3 times more at the end of the run, minutes later, and fails only if slower in 2 of those 3 too: the shared machines slow down for a while now and then, and measurements seconds apart all fall into such a spell, where a real slowdown is still there minutes later. Each measurement is the best of at least 3 runs per side, and of more for a short benchmark: until each side has run for a second (at most 30 runs), since the best of 3 runs of 50 ms could still move 20%. The tolerance is twice that benchmark's measured noise on that platform, at least 5% (two builds of the same code differ by a few percent, from where the code lands in memory; at least 15% on all cores on macOS, whose shared three-core machines swung 10 to 16% from run to run on a different benchmark each time, six false alarms in one week) and at most 25%; a benchmark whose noise needs more than 25% there is measured and reported but not gated, since no tolerance would tell its slowdowns from its noise. Numbers recorded in another run are not compared: each CI job gets a different machine, and the same code measured 52% apart on two of them. The gate runs on release pull requests (against `main`, required), every night on `dev` (against `main`), and on pull requests into `dev` that change the compiler, the runtime or the benchmarks (against `dev`, reported). Each run also checks itself: the new compiler building without LLVM's optimizer (`--inject`, `MIMIC_DEBUG_DISABLE=optimizer`) must fail it. In CI each system's benchmarks are split across three machines, each comparing both compilers on its own share, so every comparison still happens on one machine; the check against a build without LLVM's optimizer runs on pull requests into `main` and every night.

## Benchmark suite

| # | Benchmark | Stresses | Mimic features |
|---|---|---|---|
| 1 | `fib(35)` | calls, scalar arithmetic | recursion → nets, native fallback |
| 2 | `nbody` (1M steps) | floating point, small records | `evolve`, records, unboxed f64 |
| 3 | `mandelbrot` (4000²) | per-pixel compute | `each`, data parallelism, SIMD |
| 4 | `matmul` (1024²) | memory bandwidth, blocking | arrays, kernels, fusion |
| 5 | `sum-of-squares` (100M) | memory wall | kernel fusion (`xs × xs ▷ gather(+)`) |
| 6 | `prefix-sum` (100M) | scan | `trail` |
| 7 | `sort` (10M ints) | branches, memory | array primitive |
| 8 | `tree` (build and fold a depth-22 tree) | allocation, sharing | `arms`, nets, reference-count elision |
| 9 | `json` (encode and decode 300,000 records, 21 MB) | strings, numbers, records | `json::encode`, `json::decode[T]` |
| 10 | `http` (hello-world server, 400,000 requests) | I/O, concurrency | `http::serve`, event loop, keep-alive |
| 11 | `ui` (10k-row list, 20 frames of view + diff) | building and comparing screens | `view`, `ui::diff`, `¨` |
| 12 | `records` (1M records updated 5 times) | records, immutability | `with`, `¨` |
| 13 | `scale` (30 1080p frames resized to 4 resolutions) | bytes, integer arithmetic, index math | `[u8]`, fused `¨`, `bytes::from` |
| 14 | `filters` (a 1080p image through 5 filters) | stencils, small sorts | fused `¨`, lists in registers, `sort` |
| 15 | `qoi` (encode and decode a 1024×768 QOI image, 5 times) | sequential state machine, byte output | `settle`, records, `push`/`concat`/`put` in place |
| 16 | `paths` (Dijkstra and A* on a 600×600 grid) | a binary heap, branches | `settle`, records, `put` in place |

**The 0.4.0 corpus:** everyday algorithms, each in Mimic, C and Rust, each checked against an independent reference (Python's `zlib`, `hashlib`, the `xxhash` and `mmh3` libraries, or a reference implementation). What is awkward or slow in Mimic goes into [docs/gaps.md](docs/gaps.md) with the 0.4 step that fixes it; once an algorithm is fast and tested, the same code goes into the standard library (ciphers excepted).

| Group | Benchmarks |
|---|---|
| Hashing and checksums | `crc32`, `adler32`, `fnv1a`, `murmur3`, `xxhash32` (8 MiB each), `sha256` (2 MiB), `xxhash64` (8 MiB), `md5` (8 MiB, against `hashlib`), `siphash` (SipHash-2-4 of 2,000,000 16-byte keys, checked against the paper's test vector), `crc64` (CRC-64/XZ, 8 MiB), `sha1` (4 MiB, against `hashlib`), `chacha20` (the RFC 8439 cipher over 8 MiB, blocks in parallel; checked against the RFC's test vector) |
| Parsing | `csv` (400,000 rows, quoted fields), `expr` (300,000 expressions, precedence climbing), `tokenize` (200,000 lines of C-like code), `url` (300,000 URLs), `http_parse` (200,000 requests in one stream), `regex` (a Thompson NFA: 4 patterns × 50,000 strings, against Python's `re`), `numparse` (2,000,000 integers from text), `semver` (1,000,000 versions parsed into one sortable number each, sorted, written back), `ini` (100,000 sections, every line on its own: its section is a `trail(+)` of the headers; against Python's `configparser`) |
| Graphs | `bfs` (a queue, 200,000 nodes, 1,000,000 edges), `components` (union-find), `topo` (Kahn's algorithm by levels), `pagerank` (20 iterations), on random graphs from splitmix64; `paths` (Dijkstra and A*) is above, `mst` (Kruskal, 200,000 nodes, 1,000,000 edges), `bellman` (Bellman-Ford rounds, 50,000 nodes), `bipartite` (two-colouring by BFS, 300,000 nodes), `flood` (BFS flood fill, 2,000²), `maze` (carved by DFS with a stack, solved by BFS, 500²) |
| Text | `levenshtein` (20,000 pairs of 30–60 letters), `kmp` (8 patterns in 4 MiB, overlapping), `utf8` (20,000 documents, strict validation; Python's decoder is the reference), `aho` (Aho-Corasick, 45 patterns over 4 MiB in 64 pieces), `diff` (Myers, 1,000 pairs of files), `trie` (200,000 words in, 200,000 looked up), `wordfreq` (1,000,000 words: split, FNV-1a, sort, count), `zalgo` (the Z-array of 4,000,000 letters), `manacher` (palindromes in 2,000,000 letters), `anagram` (500,000 words grouped by their letters: one number a word, sorted, runs counted), `roman` (1,000,000 numbers as Roman numerals and back), `soundex` (1,000,000 names), `wordwrap` (2,000,000 words at 72 columns, against `textwrap`), `xmltags` (300,000 elements, depth by `trail(+)`; against `ElementTree`), `markdown` (400,000 lines; against `re`), `template` (300,000 renders), `lisp` (an s-expression evaluator, 50,000 expressions) |
| Interpreters | `brainf` (a Brainf**k interpreter, 15.8 million instructions), `vm` (a stack bytecode VM, 7.6 million instructions) |
| Compression and encoding | `rle` (8 MiB, run-length), `base64` (6 MiB, encode and decode), `lz77` (512 KiB, greedy LZ77 and its decoder; the reference in `bench/lz77/ref.py`), `huffman` (256 KiB, canonical codes, bit packing and decoding), `bwt` (256 KiB in 8 KiB blocks: the Burrows-Wheeler transform by prefix doubling, move-to-front and the inverse), `varint` (4,000,000 numbers as zigzag LEB128 deltas and back), `bitpack` (8,000,000 13-bit values into 64-bit words and back, both one parallel map), `lzw` (4,000,000 bytes, 12-bit codes, and back), `base32` (4,000,000 bytes, RFC 4648, both ways one parallel map; against `base64`) |
| Numbers and simulations | `sieve` (primes below 100,000,000, segmented), `montecarlo` (π from 200,000,000 points), `blackscholes` (20,000,000 options), `heat` (a 1,024² plate, 400 steps), `life` (Game of Life, 1,024², 150 generations), `fft` (a polynomial product by radix-2 FFT, 2^20 points), `kmeans` (1,000,000 points, 16 clusters, 25 rounds), `bigmul` (two 120,000-digit numbers, schoolbook); results in whole numbers or rounded per element, so they do not depend on the order of a parallel sum, `gauss` (rank and determinant of 500 × 500 matrices modulo a prime), `millerrabin` (primes among 3,000,000 numbers below 2^31), `coprime` (gcds of 16,000,000 pairs), `totient` (Euler's phi up to 10^7 by a sieve), `partitions` (p(n) up to 200,000 by Euler's recurrence), `fibmod` (F(n) mod p for 500,000 n below 10^18), `catalan` (Catalan numbers by convolution), `polyeval` (Horner over 1,000,000 polynomials), `bigfact` (20,000! in limbs of 9 digits), `shuffle` (Fisher-Yates over 10^7), `randwalk` (200,000 walkers × 1,000 steps), `ising` (512² torus, 100 sweeps), `sandpile` (128², toppled until stable), `rule110` (500,000 cells × 1,000 generations), `traffic` (Nagel-Schreckenberg, 200,000 cars × 500 steps), `langton` (Langton's ant, 2 × 10^7 steps) |
| Puzzles and tables | `nqueens` (15 queens, bit masks, 2,279,184 solutions), `knapsack` (0/1, 2,000 items, capacity 100,000, a row per item), `floyd` (Floyd–Warshall, 800 nodes; numpy is the reference), `collatz` (every number below 3,000,000), `tsp` (Held-Karp over 20 cities), `sudoku` (200 puzzles by backtracking), `knight` (every knight's tour on 5 × 5: 1,728), `matchain` (matrix-chain order for 600 matrices) |
| Data structures | `bloom` (a Bloom filter, 8 MiB, 1,000,000 keys in, 2,000,000 lookups), `hashmap` (open addressing, 1,000,000 inserts, 2,000,000 lookups), `heap` (a binary heap: 1,000,000 pushes and pops), `fenwick` (a Fenwick tree: 1,000,000 updates, 2,000,000 prefix sums), `lru` (an LRU cache as a linked list through arrays, 4,000,000 accesses), `rangemin` (a sparse table, 4,000,000 range-minimum queries), `intervals` (2,000,000 intervals merged: groups from a `trail(⌈)`), `bitset` (Jaccard sizes of 4,950 pairs of 2^20-bit sets) |
| Images | `histeq` (histogram equalization, six 3840×2160 frames), `rotate` (90° turns, six 3840×2160 frames, four each), `dither` (Floyd–Steinberg, 24 frames of 1280×720), `integral` (a summed-area table of 2048×2048 by two `trail(+)` scans, 8,000,000 box sums); `filters` (box, Gaussian, Sobel, median, bilateral) is above, `dct` (the JPEG 8 × 8 DCT in fixed point over ten 1080p frames) |
| Sorting and searching | `radix` (LSD radix sort, 8,000,000 32-bit keys, four passes of 8 bits), `bsearch` (1,000,000 sorted keys, 4,000,000 lower-bound lookups), `select` (quickselect: five percentiles of 20,000,000 values); `sort` (the built-in) is above |
| Dynamic programming | `lcs` (longest common subsequence of two 20,000-letter sequences, each row a map and `trail(⌈)`), `coins` (ways to make change: 30 coins, amounts up to 3,000,000), `lis` (longest increasing subsequence of 4,000,000 values, patience sorting); `knapsack`, `levenshtein` and `floyd` are above |
| Servers | `http_route` (a JSON API: `GET /users/{id}`, 400,000 requests with varying ids, 1 in 16 a 404), `http_echo` (`POST /echo` with 16-400-byte bodies answered in upper case); each with its own load generator that checks every answer, as `http` does |
| Geometry | `hull` (Andrew's monotone chain over 2,000,000 points, the chains as stacks), `pip` (1,000,000 points against a 500-vertex polygon, ray casting in exact integers), `segments` (8,000 segments, every pair tested by orientations) |

## Building the suite, step by step

### Step 1: Harness ✅
* Create a `bench/` directory with one folder per benchmark, each holding `main.mim`, `main.c`, `main.rs` and an `expected.txt`. (Go versions were dropped before 0.4.0: C and Rust are the comparison.)
* `bench/run.sh`:
  * builds every version;
  * checks each output against `expected.txt`;
  * runs the warm-up plus 5 timed runs, single-core and all cores;
  * writes `bench/results.md`.
* Start with benchmarks 1, 3, 5 and 7, which need no language features beyond today's.

**Done when:** `bench/run.sh` prints a table with C ratios on this machine. Done: the runner is `bench/run.py` (`python3 bench/run.py [names…] [--runs N]`), and it writes `bench/results.md`.

### Step 2: CI
* Run the suite on every merge to `main`, on fixed hardware: Linux x86-64 and macOS arm64.
* Fail CI when any benchmark slows down beyond its tolerance against the compiler the change goes into (rule 8).
* Publish `bench/results.md` as a build artifact.

**Done when:** a deliberate slowdown makes CI fail.

### Step 3: Static types and unboxed scalars (M3 step 2)
* This closes today's biggest gap; see the baseline below.

**Done when:** `fib` single-core is within **1.5× of C**.

### Step 4: Loop words and fusion (M3 steps 5 and 7)
* Benchmarks 2, 5 and 6 come online.

**Done when:** fused `sum-of-squares` makes one pass over memory, at least 2× faster than unfused.

### Step 5: Data and strings (M3 step 3)
* Benchmarks 8 and 9 come online.

**Done when:** `json` is within 2× of the fastest C parser written in the same plain style.

### Step 6: SIMD and GPU (M4)
* `mandelbrot`, `matmul` and `sum-of-squares` get explicit SIMD widths.
* Large arrays are offloaded to the GPU.

**Done when:** all-core scores beat C on 3, 4 and 5.

### Step 7: Networking and UI (M3 step 9, then M5/M6)
* Benchmarks 10 and 11 come online.

**Done when:** `http` requests/s is within 1.2× of the fastest Rust server, and `ui` frame time is under 16 ms.

## Targets

| Benchmark group | Single-core (vs C) | All cores (vs C) |
|---|---|---|
| Scalar (1, 2) | ≤ 1.5× | < 1.0× (faster than C) |
| Data-parallel (3–7) | ≤ 1.2× | ≤ 0.3× |
| Structures (8, 9) | ≤ 2.0× | ≤ 1.0× |
| I/O and UI (10, 11) | not applicable | within 1.2× of the best Rust result |

## Current results (measured)

### The whole corpus on CI machines (0.4.2, 2026-11-24)

All 130 benchmarks of the corpus on GitHub's CI machines, run [45273951342](https://github.com/mimiccodes/mimic/actions/runs/45273951342): best of 5 runs after a warm-up; each ratio is to C (below 1.0 is faster than C). C and Rust run their sequential versions on one core; Rust + Rayon is the parallel Rust version, where there is one.

Against 0.4.1 (the same machines, [phase pages](https://github.com/mimiccodes/mimic/tree/bench/phases/0.4.1)), Mimic's ratio to C, geometric mean over every benchmark measured in both:

| System | 0.4.1, 1 core | 0.4.2, 1 core | 0.4.1, all cores | 0.4.2, all cores |
|---|---|---|---|---|
| Linux x86-64 | 2.73× | 2.58× | 1.71× | 1.61× |
| macOS arm64 | 2.37× | 2.30× | 1.49× | 1.45× |
| Windows x86-64 | 2.54× | 2.38× | 1.74× | 1.66× |

Faster on one core on all three systems by more than 10%: `sudoku` 1.77×, `lru` 1.75×, `chacha20` 1.64×, `tokenize` 1.52×, `vm` 1.50×, `lz77` 1.49×, `hull` 1.45×, `dither` 1.31×, `paths` 1.31× (times faster, geometric mean of the three).
Slower on all three by more than 10%: none.

**Linux x86-64** (AMD EPYC 7763 64-Core Processor, Linux 7.0.0-1017-azure, 4 cores, load average 1.5 at the end)

| Benchmark | C | Rust | Mimic, 1 core | Rust + Rayon, all cores | Mimic, all cores |
|---|---|---|---|---|---|
| adler32 | 0.034 s | 0.034 s (0.99×) | 0.073 s (2.14×) | — | 0.055 s (1.61×) |
| aho | 0.026 s | 0.025 s (0.98×) | 0.283 s (10.89×) | — | 0.183 s (7.05×) |
| anagram | 0.073 s | 0.063 s (0.86×) | 0.304 s (4.15×) | — | 0.165 s (2.26×) |
| base32 | 0.017 s | 0.035 s (2.04×) | 0.047 s (2.71×) | — | 0.023 s (1.35×) |
| base64 | 0.019 s | 0.022 s (1.13×) | 0.127 s (6.62×) | — | 0.064 s (3.35×) |
| bellman | 0.020 s | 0.023 s (1.17×) | 0.084 s (4.21×) | — | 0.050 s (2.50×) |
| bfs | 0.024 s | 0.037 s (1.55×) | 0.195 s (8.07×) | — | 0.173 s (7.16×) |
| bigfact | 0.203 s | 0.194 s (0.95×) | 0.230 s (1.13×) | — | 0.230 s (1.13×) |
| bigmul | 0.225 s | 0.224 s (1.00×) | 0.784 s (3.49×) | — | 0.390 s (1.74×) |
| bipartite | 0.018 s | 0.037 s (2.09×) | 0.045 s (2.54×) | — | 0.031 s (1.77×) |
| bitpack | 0.059 s | 0.059 s (1.00×) | 0.112 s (1.88×) | — | 0.054 s (0.90×) |
| bitset | 0.044 s | 0.045 s (1.02×) | 0.045 s (1.03×) | — | 0.024 s (0.54×) |
| blackscholes | 1.438 s | 0.950 s (0.66×) | 1.508 s (1.05×) | — | 0.542 s (0.38×) |
| bloom | 0.041 s | 0.042 s (1.03×) | 0.102 s (2.47×) | — | 0.082 s (1.98×) |
| brainf | 0.027 s | 0.036 s (1.32×) | 0.541 s (19.93×) | — | 0.546 s (20.10×) |
| bsearch | 0.810 s | 0.670 s (0.83×) | 0.704 s (0.87×) | — | 0.226 s (0.28×) |
| bwt | 0.167 s | 0.057 s (0.34×) | 0.312 s (1.87×) | — | 0.119 s (0.71×) |
| catalan | 0.289 s | 0.273 s (0.94×) | 0.486 s (1.68×) | — | 0.559 s (1.93×) |
| chacha20 | 0.022 s | 0.026 s (1.19×) | 0.224 s (10.15×) | — | 0.146 s (6.60×) |
| coins | 0.184 s | 0.184 s (1.00×) | 0.397 s (2.16×) | — | 0.391 s (2.13×) |
| collatz | 0.428 s | 0.451 s (1.05×) | 0.858 s (2.00×) | — | 0.328 s (0.77×) |
| components | 0.018 s | 0.020 s (1.09×) | 0.022 s (1.23×) | — | 0.024 s (1.35×) |
| coprime | 0.425 s | 0.428 s (1.01×) | 0.548 s (1.29×) | — | 0.185 s (0.44×) |
| crc32 | 0.027 s | 0.027 s (1.01×) | 0.057 s (2.11×) | — | 0.039 s (1.44×) |
| crc64 | 0.027 s | 0.027 s (1.00×) | 0.028 s (1.04×) | — | 0.025 s (0.93×) |
| csv | 0.078 s | 0.062 s (0.79×) | 0.379 s (4.84×) | — | 0.198 s (2.53×) |
| dct | 0.153 s | 0.235 s (1.54×) | 0.483 s (3.16×) | — | 0.257 s (1.68×) |
| diff | 0.056 s | 0.065 s (1.17×) | 0.172 s (3.07×) | — | 0.078 s (1.40×) |
| dither | 0.176 s | 0.125 s (0.71×) | 0.513 s (2.91×) | — | 0.211 s (1.20×) |
| expr | 0.150 s | 0.103 s (0.69×) | 0.379 s (2.54×) | — | 0.176 s (1.18×) |
| fenwick | 0.074 s | 0.077 s (1.05×) | 0.099 s (1.34×) | — | 0.073 s (0.99×) |
| fft | 0.242 s | 0.239 s (0.99×) | 1.316 s (5.45×) | — | 0.675 s (2.80×) |
| fib | 0.112 s | 0.120 s (1.07×) | 0.101 s (0.90×) | 0.048 s (0.43×) | 0.046 s (0.41×) |
| fibmod | 0.335 s | 0.334 s (1.00×) | 0.286 s (0.85×) | — | 0.093 s (0.28×) |
| filters | 0.298 s | 0.282 s (0.95×) | 0.300 s (1.01×) | 0.155 s (0.52×) | 0.138 s (0.46×) |
| flood | 0.120 s | 0.121 s (1.01×) | 0.267 s (2.22×) | — | 0.253 s (2.11×) |
| floyd | 0.250 s | 0.169 s (0.68×) | 1.105 s (4.41×) | — | 0.589 s (2.35×) |
| fnv1a | 0.017 s | 0.017 s (1.00×) | 0.047 s (2.80×) | — | 0.029 s (1.75×) |
| gauss | 0.366 s | 0.355 s (0.97×) | 0.922 s (2.52×) | — | 0.492 s (1.34×) |
| hashmap | 0.086 s | 0.088 s (1.03×) | 0.142 s (1.65×) | — | 0.100 s (1.16×) |
| heap | 0.125 s | 0.139 s (1.11×) | 0.179 s (1.44×) | — | 0.178 s (1.43×) |
| heat | 0.313 s | 0.506 s (1.62×) | 1.742 s (5.57×) | — | 0.886 s (2.83×) |
| histeq | 0.085 s | 0.087 s (1.03×) | 0.319 s (3.77×) | — | 0.158 s (1.87×) |
| http | 0.368 s | 0.696 s (1.89×) | 0.473 s (1.29×) | — | 0.414 s (1.13×) |
| http_echo | 0.548 s | 1.028 s (1.88×) | 0.677 s (1.24×) | — | 0.618 s (1.13×) |
| http_parse | 0.072 s | 0.065 s (0.90×) | 0.463 s (6.46×) | — | 0.426 s (5.94×) |
| http_route | 0.523 s | 0.878 s (1.68×) | 0.696 s (1.33×) | — | 0.607 s (1.16×) |
| huffman | 0.004 s | 0.005 s (1.09×) | 0.070 s (15.73×) | — | 0.066 s (15.00×) |
| hull | 0.475 s | 0.134 s (0.28×) | 0.160 s (0.34×) | — | 0.126 s (0.26×) |
| ini | 0.068 s | 0.093 s (1.37×) | 0.459 s (6.73×) | — | 0.263 s (3.86×) |
| integral | 0.339 s | 0.339 s (1.00×) | 0.546 s (1.61×) | — | 0.208 s (0.61×) |
| intervals | 0.314 s | 0.113 s (0.36×) | 0.089 s (0.28×) | — | 0.056 s (0.18×) |
| ising | 0.186 s | 0.180 s (0.97×) | 0.282 s (1.52×) | — | 0.108 s (0.58×) |
| json | 0.329 s | 0.263 s (0.80×) | 0.314 s (0.95×) | — | 0.294 s (0.90×) |
| kmeans | 0.749 s | 0.486 s (0.65×) | 0.905 s (1.21×) | — | 0.409 s (0.55×) |
| kmp | 0.096 s | 0.063 s (0.66×) | 0.292 s (3.05×) | — | 0.179 s (1.87×) |
| knapsack | 0.127 s | 0.127 s (1.00×) | 0.255 s (2.01×) | — | 0.201 s (1.58×) |
| knight | 0.442 s | 0.689 s (1.56×) | 0.856 s (1.94×) | — | 0.416 s (0.94×) |
| langton | 0.146 s | 0.143 s (0.98×) | 0.180 s (1.23×) | — | 0.180 s (1.23×) |
| lcs | 0.557 s | 0.523 s (0.94×) | 1.023 s (1.84×) | — | 1.113 s (2.00×) |
| levenshtein | 0.131 s | 0.064 s (0.49×) | 0.617 s (4.70×) | — | 0.516 s (3.93×) |
| life | 0.599 s | 0.860 s (1.44×) | 2.285 s (3.82×) | — | 0.886 s (1.48×) |
| lis | 0.238 s | 0.218 s (0.92×) | 0.228 s (0.96×) | — | 0.227 s (0.96×) |
| lisp | 0.170 s | 0.132 s (0.78×) | 0.434 s (2.55×) | — | 0.201 s (1.18×) |
| lru | 0.033 s | 0.037 s (1.14×) | 0.078 s (2.37×) | — | 0.077 s (2.37×) |
| lz77 | 0.003 s | 0.003 s (1.23×) | 5.648 s (1999.59×) | — | 5.579 s (1975.08×) |
| lzw | 0.059 s | 0.060 s (1.02×) | 0.098 s (1.67×) | — | 0.079 s (1.35×) |
| manacher | 0.050 s | 0.052 s (1.02×) | 0.058 s (1.14×) | — | 0.046 s (0.92×) |
| mandelbrot | 0.578 s | 0.655 s (1.13×) | 0.446 s (0.77×) | 0.181 s (0.31×) | 0.183 s (0.32×) |
| markdown | 0.116 s | 0.184 s (1.59×) | 1.201 s (10.38×) | — | 0.563 s (4.87×) |
| matchain | 0.029 s | 0.029 s (0.98×) | 0.069 s (2.33×) | — | 0.049 s (1.67×) |
| matmul | 0.198 s | 0.198 s (1.00×) | 0.077 s (0.39×) | 0.410 s (2.07×) | 0.044 s (0.22×) |
| maze | 0.014 s | 0.021 s (1.47×) | 0.027 s (1.87×) | — | 0.026 s (1.82×) |
| md5 | 0.027 s | 0.027 s (0.99×) | 0.039 s (1.43×) | — | 0.034 s (1.25×) |
| millerrabin | 0.583 s | 0.674 s (1.16×) | 1.012 s (1.74×) | — | 0.294 s (0.51×) |
| montecarlo | 0.568 s | 0.585 s (1.03×) | 0.570 s (1.00×) | — | 0.239 s (0.42×) |
| mst | 0.186 s | 0.067 s (0.36×) | 0.073 s (0.39×) | — | 0.072 s (0.39×) |
| murmur3 | 0.009 s | 0.009 s (1.01×) | 0.040 s (4.56×) | — | 0.023 s (2.59×) |
| nbody | 0.100 s | 0.111 s (1.11×) | 0.067 s (0.67×) | — | 0.066 s (0.67×) |
| nqueens | 1.138 s | 1.131 s (0.99×) | 1.355 s (1.19×) | — | 0.456 s (0.40×) |
| numparse | 0.216 s | 0.310 s (1.44×) | 0.593 s (2.75×) | — | 0.386 s (1.79×) |
| pagerank | 0.112 s | 0.056 s (0.50×) | 0.139 s (1.24×) | — | 0.101 s (0.90×) |
| partitions | 0.112 s | 0.098 s (0.88×) | 0.102 s (0.92×) | — | 0.103 s (0.92×) |
| paths | 0.052 s | 0.057 s (1.09×) | 0.096 s (1.83×) | 0.037 s (0.70×) | 0.095 s (1.82×) |
| pip | 0.379 s | 0.325 s (0.86×) | 0.578 s (1.53×) | — | 0.281 s (0.74×) |
| polyeval | 0.141 s | 0.142 s (1.01×) | 0.197 s (1.40×) | — | 0.078 s (0.56×) |
| prefix_sum | 0.450 s | 0.490 s (1.09×) | 0.450 s (1.00×) | 0.261 s (0.58×) | 0.246 s (0.55×) |
| qoi | 0.027 s | 0.036 s (1.36×) | 0.382 s (14.29×) | 0.028 s (1.06×) | 0.238 s (8.92×) |
| radix | 0.139 s | 0.140 s (1.01×) | 0.417 s (2.99×) | — | 0.285 s (2.05×) |
| randwalk | 0.231 s | 0.230 s (1.00×) | 0.412 s (1.78×) | — | 0.184 s (0.80×) |
| rangemin | 0.289 s | 0.342 s (1.18×) | 1.300 s (4.50×) | — | 1.132 s (3.92×) |
| records | 0.026 s | 0.082 s (3.20×) | 0.086 s (3.35×) | 0.057 s (2.23×) | 0.051 s (1.99×) |
| regex | 0.053 s | 0.056 s (1.06×) | 0.123 s (2.32×) | — | 0.067 s (1.26×) |
| rle | 0.024 s | 0.025 s (1.03×) | 0.392 s (16.03×) | — | 0.253 s (10.32×) |
| roman | 0.072 s | 0.169 s (2.34×) | 0.473 s (6.55×) | — | 0.232 s (3.22×) |
| rotate | 0.270 s | 0.364 s (1.35×) | 0.860 s (3.19×) | — | 0.461 s (1.71×) |
| rule110 | 0.360 s | 0.591 s (1.64×) | 1.875 s (5.20×) | — | 1.025 s (2.84×) |
| sandpile | 0.285 s | 0.762 s (2.67×) | 0.733 s (2.57×) | — | 0.733 s (2.57×) |
| scale | 0.212 s | 0.493 s (2.33×) | 0.737 s (3.48×) | 0.347 s (1.64×) | 0.334 s (1.58×) |
| segments | 0.166 s | 0.157 s (0.95×) | 0.137 s (0.82×) | — | 0.069 s (0.41×) |
| select | 0.896 s | 1.306 s (1.46×) | 2.197 s (2.45×) | — | 1.207 s (1.35×) |
| semver | 0.317 s | 0.339 s (1.07×) | 0.953 s (3.01×) | — | 0.484 s (1.53×) |
| sha1 | 0.013 s | 0.014 s (1.10×) | 0.055 s (4.30×) | — | 0.053 s (4.16×) |
| sha256 | 0.010 s | 0.010 s (1.05×) | 0.094 s (9.36×) | — | 0.090 s (8.96×) |
| shuffle | 0.146 s | 0.152 s (1.04×) | 0.251 s (1.72×) | — | 0.225 s (1.54×) |
| sieve | 0.163 s | 0.186 s (1.14×) | 0.866 s (5.32×) | — | 0.436 s (2.68×) |
| siphash | 0.037 s | 0.037 s (1.02×) | 0.109 s (2.99×) | — | 0.061 s (1.67×) |
| sort | 0.900 s | 0.234 s (0.26×) | 0.218 s (0.24×) | 0.142 s (0.16×) | 0.135 s (0.15×) |
| soundex | 0.250 s | 0.168 s (0.67×) | 1.304 s (5.22×) | — | 0.610 s (2.44×) |
| sudoku | 0.245 s | 0.302 s (1.24×) | 0.759 s (3.10×) | — | 0.504 s (2.06×) |
| sum_of_squares | 0.031 s | 0.058 s (1.87×) | 0.032 s (1.02×) | 0.075 s (2.43×) | 0.016 s (0.51×) |
| template | 0.110 s | 0.166 s (1.51×) | 0.536 s (4.87×) | — | 0.307 s (2.79×) |
| tokenize | 0.040 s | 0.033 s (0.83×) | 0.346 s (8.72×) | — | 0.165 s (4.15×) |
| topo | 0.028 s | 0.034 s (1.19×) | 0.129 s (4.57×) | — | 0.104 s (3.67×) |
| totient | 0.192 s | 0.193 s (1.00×) | 0.250 s (1.30×) | — | 0.222 s (1.16×) |
| traffic | 0.444 s | 0.883 s (1.99×) | 1.510 s (3.40×) | — | 0.653 s (1.47×) |
| tree | 0.520 s | 0.616 s (1.19×) | 0.343 s (0.66×) | 0.477 s (0.92×) | 0.232 s (0.45×) |
| trie | 0.045 s | 0.035 s (0.79×) | 0.274 s (6.12×) | — | 0.230 s (5.14×) |
| tsp | 0.229 s | 0.306 s (1.34×) | 1.477 s (6.46×) | — | 0.668 s (2.92×) |
| ui | — | — | 0.093 s | — | 0.067 s |
| url | 0.095 s | 0.106 s (1.11×) | 1.062 s (11.16×) | — | 0.473 s (4.97×) |
| utf8 | 0.057 s | 0.068 s (1.19×) | 0.204 s (3.59×) | — | 0.078 s (1.37×) |
| varint | 0.071 s | 0.091 s (1.28×) | 0.192 s (2.70×) | — | 0.166 s (2.34×) |
| vm | 0.016 s | 0.022 s (1.38×) | 0.832 s (51.34×) | — | 0.836 s (51.59×) |
| wordfreq | 0.153 s | 0.056 s (0.37×) | 0.522 s (3.41×) | — | 0.291 s (1.90×) |
| wordwrap | 0.053 s | 0.206 s (3.90×) | 0.370 s (7.00×) | — | 0.260 s (4.92×) |
| xmltags | 0.070 s | 0.054 s (0.76×) | 0.425 s (6.03×) | — | 0.268 s (3.81×) |
| xxhash32 | 0.007 s | 0.007 s (1.02×) | 0.040 s (5.61×) | — | 0.023 s (3.18×) |
| xxhash64 | 0.007 s | 0.007 s (1.05×) | 0.010 s (1.50×) | — | 0.007 s (1.07×) |
| zalgo | 0.033 s | 0.039 s (1.20×) | 0.052 s (1.59×) | — | 0.036 s (1.11×) |

**macOS arm64** (Apple M1 (Virtual), Darwin 24.6.0, 3 cores, load average 1.7 at the end)

| Benchmark | C | Rust | Mimic, 1 core | Rust + Rayon, all cores | Mimic, all cores |
|---|---|---|---|---|---|
| adler32 | 0.033 s | 0.032 s (0.97×) | 0.077 s (2.36×) | — | 0.059 s (1.82×) |
| aho | 0.024 s | 0.022 s (0.94×) | 0.247 s (10.51×) | — | 0.163 s (6.93×) |
| anagram | 0.042 s | 0.058 s (1.37×) | 0.334 s (7.89×) | — | 0.163 s (3.85×) |
| base32 | 0.018 s | 0.028 s (1.55×) | 0.038 s (2.08×) | — | 0.019 s (1.06×) |
| base64 | 0.014 s | 0.014 s (0.97×) | 0.058 s (4.05×) | — | 0.043 s (3.02×) |
| bellman | 0.019 s | 0.019 s (1.00×) | 0.059 s (3.04×) | — | 0.038 s (1.97×) |
| bfs | 0.017 s | 0.026 s (1.55×) | 0.116 s (6.94×) | — | 0.117 s (6.98×) |
| bigfact | 0.325 s | 0.319 s (0.98×) | 0.906 s (2.79×) | — | 0.913 s (2.81×) |
| bigmul | 0.371 s | 0.378 s (1.02×) | 0.800 s (2.16×) | — | 0.373 s (1.00×) |
| bipartite | 0.027 s | 0.061 s (2.24×) | 0.061 s (2.26×) | — | 0.048 s (1.77×) |
| bitpack | 0.029 s | 0.032 s (1.09×) | 0.055 s (1.91×) | — | 0.034 s (1.17×) |
| bitset | 0.051 s | 0.052 s (1.02×) | 0.056 s (1.09×) | — | 0.027 s (0.52×) |
| blackscholes | 0.850 s | 0.927 s (1.09×) | 1.132 s (1.33×) | — | 0.356 s (0.42×) |
| bloom | 0.050 s | 0.062 s (1.23×) | 0.193 s (3.84×) | — | 0.157 s (3.13×) |
| brainf | 0.020 s | 0.024 s (1.18×) | 0.613 s (30.18×) | — | 0.654 s (32.18×) |
| bsearch | 0.770 s | 0.710 s (0.92×) | 0.862 s (1.12×) | — | 0.206 s (0.27×) |
| bwt | 0.186 s | 0.054 s (0.29×) | 0.294 s (1.58×) | — | 0.104 s (0.56×) |
| catalan | 0.224 s | 0.259 s (1.16×) | 0.412 s (1.84×) | — | 0.480 s (2.15×) |
| chacha20 | 0.026 s | 0.026 s (1.01×) | 0.181 s (6.91×) | — | 0.088 s (3.36×) |
| coins | 0.163 s | 0.168 s (1.03×) | 0.576 s (3.53×) | — | 0.570 s (3.49×) |
| collatz | 0.572 s | 0.579 s (1.01×) | 1.230 s (2.15×) | — | 0.419 s (0.73×) |
| components | 0.018 s | 0.019 s (1.03×) | 0.027 s (1.46×) | — | 0.026 s (1.41×) |
| coprime | 0.472 s | 0.405 s (0.86×) | 0.489 s (1.04×) | — | 0.181 s (0.38×) |
| crc32 | 0.031 s | 0.029 s (0.92×) | 0.036 s (1.14×) | — | 0.040 s (1.27×) |
| crc64 | 0.032 s | 0.029 s (0.92×) | 0.029 s (0.91×) | — | 0.028 s (0.88×) |
| csv | 0.081 s | 0.059 s (0.72×) | 0.333 s (4.10×) | — | 0.148 s (1.82×) |
| dct | 0.188 s | 0.239 s (1.27×) | 0.360 s (1.91×) | — | 0.158 s (0.84×) |
| diff | 0.054 s | 0.071 s (1.31×) | 0.149 s (2.75×) | — | 0.059 s (1.09×) |
| dither | 0.166 s | 0.169 s (1.02×) | 0.415 s (2.50×) | — | 0.183 s (1.10×) |
| expr | 0.098 s | 0.105 s (1.08×) | 0.359 s (3.67×) | — | 0.146 s (1.50×) |
| fenwick | 0.088 s | 0.090 s (1.03×) | 0.169 s (1.93×) | — | 0.142 s (1.62×) |
| fft | 0.167 s | 0.165 s (0.99×) | 0.894 s (5.36×) | — | 0.583 s (3.49×) |
| fib | 0.140 s | 0.159 s (1.14×) | 0.132 s (0.94×) | 0.063 s (0.45×) | 0.054 s (0.38×) |
| fibmod | 0.318 s | 0.328 s (1.03×) | 0.298 s (0.94×) | — | 0.120 s (0.38×) |
| filters | 0.180 s | 0.189 s (1.05×) | 0.257 s (1.43×) | 0.114 s (0.63×) | 0.110 s (0.61×) |
| flood | 0.098 s | 0.100 s (1.02×) | 0.204 s (2.10×) | — | 0.232 s (2.38×) |
| floyd | 0.323 s | 0.147 s (0.46×) | 0.918 s (2.84×) | — | 0.597 s (1.85×) |
| fnv1a | 0.018 s | 0.020 s (1.11×) | 0.022 s (1.23×) | — | 0.023 s (1.26×) |
| gauss | 0.201 s | 0.193 s (0.96×) | 0.718 s (3.57×) | — | 0.569 s (2.83×) |
| hashmap | 0.064 s | 0.063 s (0.99×) | 0.107 s (1.68×) | — | 0.088 s (1.38×) |
| heap | 0.133 s | 0.149 s (1.12×) | 0.234 s (1.76×) | — | 0.222 s (1.67×) |
| heat | 0.631 s | 0.601 s (0.95×) | 1.452 s (2.30×) | — | 0.878 s (1.39×) |
| histeq | 0.110 s | 0.117 s (1.06×) | 0.288 s (2.61×) | — | 0.136 s (1.23×) |
| http | 0.352 s | 0.605 s (1.72×) | 0.519 s (1.47×) | — | 0.474 s (1.35×) |
| http_echo | 0.705 s | 0.982 s (1.39×) | 0.896 s (1.27×) | — | 0.640 s (0.91×) |
| http_parse | 0.085 s | 0.055 s (0.65×) | 0.332 s (3.93×) | — | 0.297 s (3.51×) |
| http_route | 0.580 s | 0.848 s (1.46×) | 0.900 s (1.55×) | — | 0.731 s (1.26×) |
| huffman | 0.006 s | 0.006 s (1.06×) | 0.065 s (11.21×) | — | 0.069 s (11.87×) |
| hull | 0.251 s | 0.096 s (0.38×) | 0.104 s (0.41×) | — | 0.094 s (0.38×) |
| ini | 0.083 s | 0.082 s (0.99×) | 0.315 s (3.80×) | — | 0.188 s (2.27×) |
| integral | 0.220 s | 0.225 s (1.03×) | 0.305 s (1.39×) | — | 0.152 s (0.69×) |
| intervals | 0.211 s | 0.072 s (0.34×) | 0.052 s (0.24×) | — | 0.042 s (0.20×) |
| ising | 0.171 s | 0.169 s (0.99×) | 0.130 s (0.76×) | — | 0.095 s (0.56×) |
| json | 0.245 s | 0.155 s (0.63×) | 0.215 s (0.88×) | — | 0.188 s (0.77×) |
| kmeans | 0.668 s | 0.489 s (0.73×) | 0.689 s (1.03×) | — | 0.536 s (0.80×) |
| kmp | 0.101 s | 0.092 s (0.91×) | 0.224 s (2.22×) | — | 0.138 s (1.37×) |
| knapsack | 0.096 s | 0.099 s (1.04×) | 0.228 s (2.38×) | — | 0.197 s (2.06×) |
| knight | 0.333 s | 0.559 s (1.68×) | 0.852 s (2.56×) | — | 0.388 s (1.17×) |
| langton | 0.134 s | 0.136 s (1.01×) | 0.275 s (2.06×) | — | 0.277 s (2.07×) |
| lcs | 0.901 s | 0.808 s (0.90×) | 0.892 s (0.99×) | — | 0.841 s (0.93×) |
| levenshtein | 0.051 s | 0.065 s (1.27×) | 0.361 s (7.02×) | — | 0.299 s (5.82×) |
| life | 0.536 s | 0.795 s (1.48×) | 1.430 s (2.67×) | — | 0.537 s (1.00×) |
| lis | 0.187 s | 0.093 s (0.50×) | 0.101 s (0.54×) | — | 0.101 s (0.54×) |
| lisp | 0.130 s | 0.102 s (0.79×) | 0.340 s (2.61×) | — | 0.138 s (1.06×) |
| lru | 0.027 s | 0.029 s (1.08×) | 0.071 s (2.67×) | — | 0.072 s (2.71×) |
| lz77 | 0.003 s | 0.004 s (1.25×) | 1.165 s (341.48×) | — | 1.270 s (372.12×) |
| lzw | 0.049 s | 0.043 s (0.90×) | 0.088 s (1.82×) | — | 0.083 s (1.71×) |
| manacher | 0.043 s | 0.040 s (0.93×) | 0.040 s (0.94×) | — | 0.040 s (0.93×) |
| mandelbrot | 0.731 s | 0.706 s (0.97×) | 0.463 s (0.63×) | 0.256 s (0.35×) | 0.176 s (0.24×) |
| markdown | 0.120 s | 0.172 s (1.43×) | 0.974 s (8.10×) | — | 0.487 s (4.04×) |
| matchain | 0.020 s | 0.022 s (1.11×) | 0.049 s (2.46×) | — | 0.047 s (2.37×) |
| matmul | 0.192 s | 0.192 s (1.00×) | 0.036 s (0.19×) | 0.302 s (1.57×) | 0.032 s (0.17×) |
| maze | 0.011 s | 0.020 s (1.74×) | 0.022 s (2.00×) | — | 0.023 s (2.03×) |
| md5 | 0.036 s | 0.032 s (0.89×) | 0.038 s (1.04×) | — | 0.034 s (0.95×) |
| millerrabin | 0.496 s | 0.513 s (1.03×) | 1.176 s (2.37×) | — | 0.440 s (0.89×) |
| montecarlo | 0.299 s | 0.227 s (0.76×) | 0.298 s (0.99×) | — | 0.104 s (0.35×) |
| mst | 0.128 s | 0.047 s (0.37×) | 0.051 s (0.40×) | — | 0.053 s (0.41×) |
| murmur3 | 0.008 s | 0.009 s (1.04×) | 0.015 s (1.79×) | — | 0.012 s (1.49×) |
| nbody | 0.041 s | 0.053 s (1.29×) | 0.040 s (0.97×) | — | 0.040 s (0.97×) |
| nqueens | 1.003 s | 1.008 s (1.00×) | 1.049 s (1.05×) | — | 0.364 s (0.36×) |
| numparse | 0.185 s | 0.207 s (1.12×) | 0.407 s (2.20×) | — | 0.242 s (1.31×) |
| pagerank | 0.075 s | 0.035 s (0.47×) | 0.071 s (0.94×) | — | 0.048 s (0.64×) |
| partitions | 0.063 s | 0.055 s (0.87×) | 0.060 s (0.94×) | — | 0.061 s (0.96×) |
| paths | 0.046 s | 0.048 s (1.06×) | 0.080 s (1.76×) | 0.030 s (0.67×) | 0.078 s (1.72×) |
| pip | 0.393 s | 0.512 s (1.30×) | 0.598 s (1.52×) | — | 0.215 s (0.55×) |
| polyeval | 0.104 s | 0.125 s (1.20×) | 0.221 s (2.11×) | — | 0.080 s (0.77×) |
| prefix_sum | 0.122 s | 0.233 s (1.90×) | 0.115 s (0.94×) | 0.080 s (0.65×) | 0.070 s (0.57×) |
| qoi | 0.022 s | 0.026 s (1.20×) | 0.262 s (12.17×) | 0.015 s (0.70×) | 0.161 s (7.48×) |
| radix | 0.067 s | 0.070 s (1.05×) | 0.406 s (6.10×) | — | 0.348 s (5.21×) |
| randwalk | 0.257 s | 0.258 s (1.00×) | 0.531 s (2.07×) | — | 0.191 s (0.74×) |
| rangemin | 0.153 s | 0.159 s (1.04×) | 0.359 s (2.35×) | — | 0.246 s (1.61×) |
| records | 0.013 s | 0.035 s (2.73×) | 0.026 s (2.06×) | 0.029 s (2.28×) | 0.017 s (1.35×) |
| regex | 0.061 s | 0.066 s (1.08×) | 0.118 s (1.93×) | — | 0.056 s (0.92×) |
| rle | 0.018 s | 0.019 s (1.06×) | 0.238 s (13.06×) | — | 0.121 s (6.64×) |
| roman | 0.058 s | 0.118 s (2.03×) | 0.334 s (5.72×) | — | 0.139 s (2.38×) |
| rotate | 0.229 s | 0.375 s (1.64×) | 0.469 s (2.05×) | — | 0.199 s (0.87×) |
| rule110 | 0.418 s | 0.480 s (1.15×) | 0.873 s (2.09×) | — | 0.327 s (0.78×) |
| sandpile | 0.502 s | 0.640 s (1.28×) | 0.580 s (1.16×) | — | 0.580 s (1.16×) |
| scale | 0.167 s | 0.274 s (1.64×) | 0.379 s (2.27×) | 0.137 s (0.82×) | 0.137 s (0.82×) |
| segments | 0.111 s | 0.164 s (1.48×) | 0.101 s (0.92×) | — | 0.038 s (0.34×) |
| select | 0.700 s | 0.739 s (1.06×) | 1.252 s (1.79×) | — | 0.654 s (0.93×) |
| semver | 0.254 s | 0.227 s (0.89×) | 0.774 s (3.05×) | — | 0.317 s (1.25×) |
| sha1 | 0.022 s | 0.023 s (1.05×) | 0.045 s (2.10×) | — | 0.043 s (1.97×) |
| sha256 | 0.013 s | 0.014 s (1.07×) | 0.120 s (8.92×) | — | 0.121 s (9.01×) |
| shuffle | 0.082 s | 0.088 s (1.07×) | 0.247 s (3.00×) | — | 0.246 s (2.99×) |
| sieve | 0.193 s | 0.241 s (1.25×) | 0.680 s (3.53×) | — | 0.238 s (1.23×) |
| siphash | 0.035 s | 0.035 s (1.00×) | 0.105 s (3.03×) | — | 0.046 s (1.34×) |
| sort | 1.001 s | 0.157 s (0.16×) | 0.092 s (0.09×) | 0.115 s (0.12×) | 0.059 s (0.06×) |
| soundex | 0.183 s | 0.150 s (0.82×) | 1.007 s (5.50×) | — | 0.377 s (2.06×) |
| sudoku | 0.261 s | 0.245 s (0.94×) | 0.836 s (3.21×) | — | 0.523 s (2.01×) |
| sum_of_squares | 0.028 s | 0.043 s (1.53×) | 0.029 s (1.01×) | 0.028 s (0.99×) | 0.012 s (0.41×) |
| template | 0.099 s | 0.158 s (1.60×) | 0.434 s (4.38×) | — | 0.209 s (2.11×) |
| tokenize | 0.037 s | 0.032 s (0.85×) | 0.290 s (7.76×) | — | 0.131 s (3.51×) |
| topo | 0.021 s | 0.030 s (1.48×) | 0.074 s (3.60×) | — | 0.058 s (2.83×) |
| totient | 0.159 s | 0.144 s (0.91×) | 0.379 s (2.39×) | — | 0.373 s (2.35×) |
| traffic | 0.258 s | 0.286 s (1.11×) | 0.486 s (1.89×) | — | 0.234 s (0.91×) |
| tree | 0.302 s | 0.342 s (1.13×) | 0.186 s (0.62×) | 0.197 s (0.65×) | 0.098 s (0.33×) |
| trie | 0.019 s | 0.025 s (1.29×) | 0.188 s (9.72×) | — | 0.174 s (8.99×) |
| tsp | 0.143 s | 0.270 s (1.89×) | 0.589 s (4.13×) | — | 0.222 s (1.55×) |
| ui | — | — | 0.065 s | — | 0.043 s |
| url | 0.100 s | 0.083 s (0.84×) | 0.887 s (8.91×) | — | 0.320 s (3.22×) |
| utf8 | 0.049 s | 0.057 s (1.17×) | 0.154 s (3.16×) | — | 0.057 s (1.18×) |
| varint | 0.036 s | 0.044 s (1.22×) | 0.114 s (3.20×) | — | 0.103 s (2.88×) |
| vm | 0.015 s | 0.015 s (1.04×) | 0.662 s (45.50×) | — | 0.668 s (45.90×) |
| wordfreq | 0.076 s | 0.048 s (0.63×) | 0.435 s (5.71×) | — | 0.209 s (2.75×) |
| wordwrap | 0.051 s | 0.121 s (2.37×) | 0.239 s (4.69×) | — | 0.157 s (3.08×) |
| xmltags | 0.069 s | 0.045 s (0.65×) | 0.331 s (4.78×) | — | 0.177 s (2.55×) |
| xxhash32 | 0.007 s | 0.008 s (1.12×) | 0.014 s (1.98×) | — | 0.012 s (1.76×) |
| xxhash64 | 0.006 s | 0.006 s (1.01×) | 0.008 s (1.46×) | — | 0.007 s (1.30×) |
| zalgo | 0.021 s | 0.025 s (1.21×) | 0.034 s (1.65×) | — | 0.028 s (1.37×) |

**Windows x86-64** (AMD64 Family 25 Model 1 Stepping 1, AuthenticAMD, Windows 2025Server, 4 cores, load average 0.0 at the end)

| Benchmark | C | Rust | Mimic, 1 core | Rust + Rayon, all cores | Mimic, all cores |
|---|---|---|---|---|---|
| adler32 | 0.036 s | 0.036 s (1.00×) | 0.060 s (1.65×) | — | 0.070 s (1.94×) |
| aho | 0.036 s | 0.036 s (1.00×) | 0.317 s (8.79×) | — | 0.211 s (5.84×) |
| anagram | 0.063 s | 0.074 s (1.17×) | 0.395 s (6.26×) | — | 0.222 s (3.52×) |
| base32 | 0.019 s | 0.038 s (1.94×) | 0.049 s (2.53×) | — | 0.035 s (1.78×) |
| base64 | 0.019 s | 0.023 s (1.18×) | 0.082 s (4.20×) | — | 0.081 s (4.18×) |
| bellman | 0.027 s | 0.025 s (0.94×) | 0.093 s (3.45×) | — | 0.061 s (2.26×) |
| bfs | 0.028 s | 0.036 s (1.31×) | 0.173 s (6.21×) | — | 0.168 s (6.04×) |
| bigfact | 0.203 s | 0.204 s (1.00×) | 0.218 s (1.07×) | — | 0.217 s (1.07×) |
| bigmul | 0.211 s | 0.212 s (1.00×) | 0.800 s (3.80×) | — | 0.416 s (1.98×) |
| bipartite | 0.027 s | 0.070 s (2.62×) | 0.054 s (2.03×) | — | 0.047 s (1.75×) |
| bitpack | 0.038 s | 0.038 s (1.00×) | 0.072 s (1.89×) | — | 0.068 s (1.78×) |
| bitset | 0.046 s | 0.047 s (1.03×) | 0.052 s (1.14×) | — | 0.035 s (0.76×) |
| blackscholes | 1.324 s | 0.964 s (0.73×) | 1.576 s (1.19×) | — | 0.590 s (0.45×) |
| bloom | 0.054 s | 0.056 s (1.04×) | 0.108 s (2.00×) | — | 0.104 s (1.93×) |
| brainf | 0.047 s | 0.033 s (0.70×) | 0.598 s (12.78×) | — | 0.600 s (12.82×) |
| bsearch | 0.822 s | 0.681 s (0.83×) | 0.680 s (0.83×) | — | 0.216 s (0.26×) |
| bwt | 0.226 s | 0.085 s (0.37×) | 0.326 s (1.44×) | — | 0.141 s (0.63×) |
| catalan | 0.296 s | 0.281 s (0.95×) | 0.282 s (0.95×) | — | 0.461 s (1.56×) |
| chacha20 | 0.029 s | 0.031 s (1.06×) | 0.205 s (7.07×) | — | 0.132 s (4.56×) |
| coins | 0.180 s | 0.180 s (1.00×) | 0.479 s (2.66×) | — | 0.475 s (2.64×) |
| collatz | 0.444 s | 0.459 s (1.03×) | 0.864 s (1.95×) | — | 0.383 s (0.86×) |
| components | 0.023 s | 0.024 s (1.04×) | 0.031 s (1.36×) | — | 0.039 s (1.72×) |
| coprime | 0.433 s | 0.432 s (1.00×) | 0.514 s (1.19×) | — | 0.189 s (0.44×) |
| crc32 | 0.031 s | 0.031 s (1.00×) | 0.046 s (1.50×) | — | 0.057 s (1.87×) |
| crc64 | 0.031 s | 0.031 s (1.01×) | 0.037 s (1.20×) | — | 0.036 s (1.19×) |
| csv | 0.118 s | 0.081 s (0.68×) | 0.413 s (3.50×) | — | 0.230 s (1.94×) |
| dct | 0.140 s | 0.221 s (1.58×) | 0.469 s (3.35×) | — | 0.246 s (1.76×) |
| diff | 0.063 s | 0.109 s (1.72×) | 0.219 s (3.47×) | — | 0.097 s (1.54×) |
| dither | 0.180 s | 0.127 s (0.71×) | 0.453 s (2.52×) | — | 0.211 s (1.17×) |
| expr | 0.220 s | 0.147 s (0.66×) | 0.559 s (2.53×) | — | 0.257 s (1.17×) |
| fenwick | 0.094 s | 0.095 s (1.01×) | 0.113 s (1.20×) | — | 0.091 s (0.96×) |
| fft | 0.242 s | 0.238 s (0.98×) | 1.445 s (5.96×) | — | 0.952 s (3.93×) |
| fib | 0.126 s | 0.120 s (0.95×) | 0.125 s (0.99×) | 0.056 s (0.44×) | 0.058 s (0.46×) |
| fibmod | 0.341 s | 0.344 s (1.01×) | 0.299 s (0.88×) | — | 0.106 s (0.31×) |
| filters | 0.315 s | 0.306 s (0.97×) | 0.379 s (1.21×) | 0.161 s (0.51×) | 0.181 s (0.57×) |
| flood | 0.103 s | 0.133 s (1.29×) | 0.235 s (2.28×) | — | 0.242 s (2.34×) |
| floyd | 0.276 s | 0.174 s (0.63×) | 1.681 s (6.08×) | — | 1.339 s (4.84×) |
| fnv1a | 0.020 s | 0.020 s (1.03×) | 0.036 s (1.82×) | — | 0.046 s (2.34×) |
| gauss | 0.373 s | 0.363 s (0.97×) | 1.253 s (3.36×) | — | 0.910 s (2.44×) |
| hashmap | 0.109 s | 0.113 s (1.04×) | 0.133 s (1.22×) | — | 0.104 s (0.95×) |
| heap | 0.122 s | 0.135 s (1.10×) | 0.188 s (1.54×) | — | 0.202 s (1.65×) |
| heat | 0.236 s | 0.511 s (2.17×) | 2.519 s (10.69×) | — | 1.577 s (6.69×) |
| histeq | 0.088 s | 0.091 s (1.04×) | 0.311 s (3.55×) | — | 0.153 s (1.75×) |
| http | — | 1.152 s | 1.376 s | — | 0.887 s |
| http_echo | — | 1.640 s | 1.735 s | — | 1.248 s |
| http_parse | 0.229 s | 0.074 s (0.32×) | 0.434 s (1.89×) | — | 0.391 s (1.71×) |
| http_route | — | 1.490 s | 1.770 s | — | 1.249 s |
| huffman | 0.009 s | 0.010 s (1.11×) | 0.088 s (9.48×) | — | 0.082 s (8.75×) |
| hull | 0.467 s | 0.130 s (0.28×) | 0.153 s (0.33×) | — | 0.133 s (0.29×) |
| ini | 0.144 s | 0.141 s (0.98×) | 0.497 s (3.45×) | — | 0.278 s (1.93×) |
| integral | 0.448 s | 0.461 s (1.03×) | 0.681 s (1.52×) | — | 0.281 s (0.63×) |
| intervals | 0.361 s | 0.107 s (0.30×) | 0.077 s (0.21×) | — | 0.059 s (0.16×) |
| ising | 0.191 s | 0.194 s (1.02×) | 0.365 s (1.91×) | — | 0.200 s (1.05×) |
| json | 0.612 s | 0.361 s (0.59×) | 0.336 s (0.55×) | — | 0.303 s (0.50×) |
| kmeans | 0.758 s | 0.499 s (0.66×) | 0.935 s (1.23×) | — | 0.463 s (0.61×) |
| kmp | 0.105 s | 0.060 s (0.57×) | 0.321 s (3.04×) | — | 0.208 s (1.98×) |
| knapsack | 0.133 s | 0.133 s (1.00×) | 0.267 s (2.01×) | — | 0.274 s (2.06×) |
| knight | 0.455 s | 0.742 s (1.63×) | 0.973 s (2.14×) | — | 0.501 s (1.10×) |
| langton | 0.154 s | 0.150 s (0.97×) | 0.191 s (1.24×) | — | 0.191 s (1.24×) |
| lcs | 0.553 s | 0.467 s (0.84×) | 1.194 s (2.16×) | — | 1.022 s (1.85×) |
| levenshtein | 0.141 s | 0.079 s (0.56×) | 0.643 s (4.58×) | — | 0.535 s (3.81×) |
| life | 0.609 s | 0.868 s (1.43×) | 2.593 s (4.26×) | — | 1.159 s (1.90×) |
| lis | 0.245 s | 0.219 s (0.89×) | 0.241 s (0.98×) | — | 0.240 s (0.98×) |
| lisp | 0.197 s | 0.155 s (0.79×) | 0.552 s (2.81×) | — | 0.260 s (1.32×) |
| lru | 0.040 s | 0.041 s (1.02×) | 0.087 s (2.19×) | — | 0.088 s (2.22×) |
| lz77 | 0.008 s | 0.009 s (1.19×) | 4.272 s (551.74×) | — | 4.273 s (551.90×) |
| lzw | 0.058 s | 0.059 s (1.02×) | 0.101 s (1.75×) | — | 0.092 s (1.59×) |
| manacher | 0.045 s | 0.048 s (1.05×) | 0.056 s (1.24×) | — | 0.057 s (1.26×) |
| mandelbrot | 0.586 s | 0.663 s (1.13×) | 0.446 s (0.76×) | 0.188 s (0.32×) | 0.193 s (0.33×) |
| markdown | 0.201 s | 0.319 s (1.59×) | 1.459 s (7.26×) | — | 0.719 s (3.58×) |
| matchain | 0.035 s | 0.035 s (0.98×) | 0.079 s (2.24×) | — | 0.070 s (1.99×) |
| matmul | 0.158 s | 0.145 s (0.92×) | 0.082 s (0.52×) | 0.530 s (3.36×) | 0.053 s (0.34×) |
| maze | 0.022 s | 0.035 s (1.61×) | 0.038 s (1.75×) | — | 0.039 s (1.80×) |
| md5 | 0.031 s | 0.036 s (1.18×) | 0.048 s (1.56×) | — | 0.044 s (1.44×) |
| millerrabin | 0.591 s | 0.668 s (1.13×) | 1.032 s (1.75×) | — | 0.312 s (0.53×) |
| montecarlo | 0.578 s | 0.592 s (1.02×) | 0.583 s (1.01×) | — | 0.249 s (0.43×) |
| mst | 0.217 s | 0.071 s (0.33×) | 0.080 s (0.37×) | — | 0.100 s (0.46×) |
| murmur3 | 0.012 s | 0.012 s (1.04×) | 0.031 s (2.63×) | — | 0.042 s (3.49×) |
| nbody | 0.107 s | 0.117 s (1.10×) | 0.077 s (0.72×) | — | 0.077 s (0.72×) |
| nqueens | 1.160 s | 1.183 s (1.02×) | 1.278 s (1.10×) | — | 0.517 s (0.45×) |
| numparse | 0.274 s | 0.331 s (1.21×) | 0.707 s (2.58×) | — | 0.438 s (1.60×) |
| pagerank | 0.120 s | 0.059 s (0.49×) | 0.127 s (1.06×) | — | 0.104 s (0.87×) |
| partitions | 0.104 s | 0.093 s (0.90×) | 0.104 s (1.01×) | — | 0.104 s (1.01×) |
| paths | 0.062 s | 0.065 s (1.04×) | 0.108 s (1.74×) | 0.042 s (0.68×) | 0.109 s (1.75×) |
| pip | 0.390 s | 0.334 s (0.86×) | 0.595 s (1.52×) | — | 0.296 s (0.76×) |
| polyeval | 0.147 s | 0.147 s (1.00×) | 0.216 s (1.47×) | — | 0.097 s (0.66×) |
| prefix_sum | 0.209 s | 0.456 s (2.18×) | 0.210 s (1.00×) | 0.171 s (0.82×) | 0.166 s (0.79×) |
| qoi | 0.032 s | 0.043 s (1.36×) | 0.402 s (12.68×) | 0.033 s (1.04×) | 0.237 s (7.46×) |
| radix | 0.109 s | 0.112 s (1.03×) | 0.339 s (3.11×) | — | 0.356 s (3.26×) |
| randwalk | 0.237 s | 0.238 s (1.00×) | 0.424 s (1.79×) | — | 0.194 s (0.82×) |
| rangemin | 0.324 s | 0.489 s (1.51×) | 0.693 s (2.14×) | — | 0.527 s (1.62×) |
| records | 0.024 s | 0.088 s (3.75×) | 0.093 s (3.94×) | 0.059 s (2.50×) | 0.062 s (2.63×) |
| regex | 0.059 s | 0.071 s (1.20×) | 0.137 s (2.31×) | — | 0.077 s (1.30×) |
| rle | 0.027 s | 0.032 s (1.17×) | 0.395 s (14.50×) | — | 0.249 s (9.12×) |
| roman | 0.071 s | 0.205 s (2.88×) | 0.548 s (7.69×) | — | 0.265 s (3.72×) |
| rotate | 0.272 s | 0.364 s (1.34×) | 0.787 s (2.90×) | — | 0.432 s (1.59×) |
| rule110 | 0.368 s | 0.522 s (1.42×) | 1.899 s (5.16×) | — | 1.065 s (2.89×) |
| sandpile | 0.324 s | 0.832 s (2.57×) | 0.756 s (2.33×) | — | 0.755 s (2.33×) |
| scale | 0.434 s | 0.512 s (1.18×) | 0.756 s (1.74×) | 0.354 s (0.82×) | 0.343 s (0.79×) |
| segments | 0.169 s | 0.162 s (0.96×) | 0.149 s (0.88×) | — | 0.080 s (0.47×) |
| select | 0.757 s | 0.945 s (1.25×) | 1.657 s (2.19×) | — | 0.907 s (1.20×) |
| semver | 0.410 s | 0.418 s (1.02×) | 1.150 s (2.81×) | — | 0.602 s (1.47×) |
| sha1 | 0.018 s | 0.020 s (1.11×) | 0.068 s (3.86×) | — | 0.067 s (3.82×) |
| sha256 | 0.015 s | 0.017 s (1.08×) | 0.138 s (9.01×) | — | 0.140 s (9.16×) |
| shuffle | 0.152 s | 0.139 s (0.92×) | 0.288 s (1.90×) | — | 0.315 s (2.08×) |
| sieve | 0.169 s | 0.238 s (1.41×) | 0.891 s (5.27×) | — | 0.447 s (2.64×) |
| siphash | 0.044 s | 0.043 s (0.98×) | 0.128 s (2.90×) | — | 0.071 s (1.60×) |
| sort | 1.806 s | 0.219 s (0.12×) | 0.151 s (0.08×) | 0.138 s (0.08×) | 0.121 s (0.07×) |
| soundex | 0.249 s | 0.309 s (1.24×) | 1.513 s (6.08×) | — | 0.683 s (2.75×) |
| sudoku | 0.251 s | 0.302 s (1.21×) | 0.877 s (3.50×) | — | 0.592 s (2.36×) |
| sum_of_squares | 0.037 s | 0.065 s (1.77×) | 0.043 s (1.16×) | 0.083 s (2.24×) | 0.027 s (0.74×) |
| template | 0.131 s | 0.260 s (1.99×) | 0.661 s (5.06×) | — | 0.355 s (2.71×) |
| tokenize | 0.091 s | 0.047 s (0.52×) | 0.453 s (4.99×) | — | 0.217 s (2.39×) |
| topo | 0.034 s | 0.058 s (1.72×) | 0.120 s (3.56×) | — | 0.103 s (3.06×) |
| totient | 0.180 s | 0.181 s (1.01×) | 0.257 s (1.43×) | — | 0.275 s (1.53×) |
| traffic | 0.453 s | 0.925 s (2.04×) | 1.476 s (3.26×) | — | 0.861 s (1.90×) |
| tree | 1.117 s | 1.272 s (1.14×) | 0.362 s (0.32×) | 0.768 s (0.69×) | 0.196 s (0.18×) |
| trie | 0.034 s | 0.060 s (1.77×) | 0.313 s (9.23×) | — | 0.303 s (8.95×) |
| tsp | 0.211 s | 0.292 s (1.39×) | 0.887 s (4.20×) | — | 0.653 s (3.10×) |
| ui | — | — | 0.119 s | — | 0.089 s |
| url | 0.252 s | 0.131 s (0.52×) | 1.174 s (4.65×) | — | 0.547 s (2.17×) |
| utf8 | 0.064 s | 0.076 s (1.19×) | 0.241 s (3.78×) | — | 0.098 s (1.53×) |
| varint | 0.053 s | 0.073 s (1.37×) | 0.172 s (3.23×) | — | 0.167 s (3.14×) |
| vm | 0.022 s | 0.030 s (1.37×) | 1.078 s (49.79×) | — | 1.078 s (49.80×) |
| wordfreq | 0.125 s | 0.060 s (0.48×) | 0.624 s (5.00×) | — | 0.373 s (2.99×) |
| wordwrap | 0.065 s | 0.236 s (3.65×) | 0.366 s (5.67×) | — | 0.250 s (3.87×) |
| xmltags | 0.096 s | 0.064 s (0.67×) | 0.480 s (5.00×) | — | 0.292 s (3.05×) |
| xxhash32 | 0.010 s | 0.011 s (1.08×) | 0.031 s (3.02×) | — | 0.040 s (3.96×) |
| xxhash64 | 0.010 s | 0.010 s (0.96×) | 0.019 s (1.80×) | — | 0.018 s (1.70×) |
| zalgo | 0.027 s | 0.033 s (1.21×) | 0.052 s (1.88×) | — | 0.051 s (1.87×) |

### filters, qoi and paths on CI machines (2026-11-17)

The bench workflow (run 43975091821, commit 6024f9c). Ratios to C; Rust + Rayon and Mimic, all cores, are the parallel columns.

| | C | Rust | Mimic, 1 core | Rust + Rayon, all cores | Mimic, all cores |
|---|---|---|---|---|---|
| filters, macOS | 0.198 s | 0.97× | 1.35× | 0.59× | 0.46× |
| filters, Linux | 0.215 s | 0.90× | 1.30× | 0.67× | 0.61× |
| filters, Windows | 0.126 s | 0.77× | 1.78× | 0.74× | 0.81× |
| paths, macOS | 0.053 s | 0.93× | 2.48× | 0.57× | 2.28× |
| paths, Linux | 0.064 s | 1.06× | 2.70× | 0.66× | 2.70× |
| paths, Windows | 0.044 s | 1.02× | 1.94× | 0.60× | 1.95× |
| qoi, macOS | 0.052 s | 0.64× | 11.01× | 0.77× | 5.77× |
| qoi, Linux | 0.025 s | 1.24× | 14.65× | 0.91× | 8.70× |
| qoi, Windows | 0.016 s | 1.38× | 14.29× | 1.00× | 8.42× |

`filters` is ahead of Rayon on all cores on macOS and Linux. `paths` (a binary heap in a record stepped by `settle`) is 2–2.7× C: each step goes through the record and the heap's lists, where C writes into arrays; the two searches run one after the other (Rayon runs them at once). `qoi` is the largest gap left: each pixel builds a few small lists (a run byte, the pixel's bytes) before they join the output, which C writes byte by byte.

### On CI machines, with Rust + Rayon (byte and pixel round, 2026-11-17)

The bench workflow (run 43929837181, commit 3da7f96), after the round prompted by the new `scale` benchmark and the transcoding example: byte data read and written directly, heavy loop elements shared over the cores, element functions inlined into fused loops, a plain divide for operands that are not negative, tuples of numbers in registers. Ratios to C; Rust + Rayon and Mimic, all cores, are the parallel columns.

**macOS (arm64)**

| Benchmark | C | Rust | Mimic, 1 core | Rust + Rayon, all cores | Mimic, all cores |
|---|---|---|---|---|---|
| fib | 0.133 s | 0.155 s (1.16×) | 0.137 s (1.03×) | 0.049 s (0.37×) | 0.048 s (0.36×) |
| http | 0.303 s | 0.371 s (1.22×) | 0.559 s (1.84×) | — | 0.451 s (1.49×) |
| json | 0.333 s | 0.209 s (0.63×) | 0.287 s (0.86×) | — | 0.264 s (0.79×) |
| mandelbrot | 0.847 s | 0.806 s (0.95×) | 0.508 s (0.60×) | 0.272 s (0.32×) | 0.187 s (0.22×) |
| matmul | 0.255 s | 0.273 s (1.07×) | 0.039 s (0.15×) | 0.306 s (1.20×) | 0.037 s (0.15×) |
| nbody | 0.043 s | 0.058 s (1.34×) | 0.052 s (1.19×) | — | 0.047 s (1.07×) |
| prefix_sum | 0.159 s | 0.277 s (1.74×) | 0.131 s (0.83×) | 0.106 s (0.67×) | 0.078 s (0.49×) |
| records | 0.014 s | 0.039 s (2.82×) | 0.036 s (2.56×) | 0.037 s (2.68×) | 0.023 s (1.63×) |
| scale | 0.171 s | 0.292 s (1.70×) | 0.546 s (3.19×) | 0.172 s (1.01×) | 0.154 s (0.90×) |
| sort | 1.210 s | 0.259 s (0.21×) | 0.161 s (0.13×) | 0.154 s (0.13×) | 0.112 s (0.09×) |
| sum_of_squares | 0.039 s | 0.049 s (1.26×) | 0.032 s (0.82×) | 0.029 s (0.76×) | 0.013 s (0.33×) |
| tree | 0.345 s | 0.441 s (1.28×) | 0.266 s (0.77×) | 0.321 s (0.93×) | 0.243 s (0.70×) |

**Linux (x86-64)**

| Benchmark | C | Rust | Mimic, 1 core | Rust + Rayon, all cores | Mimic, all cores |
|---|---|---|---|---|---|
| fib | 0.113 s | 0.121 s (1.07×) | 0.101 s (0.89×) | 0.048 s (0.43×) | 0.047 s (0.41×) |
| http | 0.298 s | 0.687 s (2.31×) | 0.513 s (1.72×) | — | 0.454 s (1.52×) |
| json | 0.328 s | 0.263 s (0.80×) | 0.313 s (0.96×) | — | 0.291 s (0.89×) |
| mandelbrot | 0.577 s | 0.655 s (1.13×) | 0.446 s (0.77×) | 0.181 s (0.31×) | 0.184 s (0.32×) |
| matmul | 0.201 s | 0.198 s (0.99×) | 0.077 s (0.38×) | 0.409 s (2.04×) | 0.044 s (0.22×) |
| nbody | 0.100 s | 0.111 s (1.11×) | 0.066 s (0.66×) | — | 0.066 s (0.66×) |
| prefix_sum | 0.439 s | 0.488 s (1.11×) | 0.443 s (1.01×) | 0.259 s (0.59×) | 0.247 s (0.56×) |
| records | 0.027 s | 0.083 s (3.09×) | 0.084 s (3.15×) | 0.054 s (2.02×) | 0.049 s (1.85×) |
| scale | 0.212 s | 0.492 s (2.33×) | 0.744 s (3.52×) | 0.349 s (1.65×) | 0.334 s (1.58×) |
| sort | 0.900 s | 0.231 s (0.26×) | 0.212 s (0.24×) | 0.142 s (0.16×) | 0.133 s (0.15×) |
| sum_of_squares | 0.031 s | 0.058 s (1.86×) | 0.031 s (1.01×) | 0.075 s (2.42×) | 0.016 s (0.51×) |
| tree | 0.528 s | 0.618 s (1.17×) | 0.342 s (0.65×) | 0.446 s (0.84×) | 0.170 s (0.32×) |

**Windows (x86-64)** (no C version of the server: http has no ratio)

| Benchmark | C | Rust | Mimic, 1 core | Rust + Rayon, all cores | Mimic, all cores |
|---|---|---|---|---|---|
| fib | 0.129 s | 0.112 s (0.87×) | 0.114 s (0.88×) | 0.053 s (0.41×) | 0.058 s (0.45×) |
| http | — | 0.749 s | 0.587 s | — | 0.667 s |
| json | 0.571 s | 0.338 s (0.59×) | 0.389 s (0.68×) | — | 0.345 s (0.60×) |
| mandelbrot | 0.646 s | 0.684 s (1.06×) | 0.220 s (0.34×) | 0.202 s (0.31×) | 0.082 s (0.13×) |
| matmul | 0.306 s | 0.300 s (0.98×) | 0.105 s (0.34×) | 0.507 s (1.66×) | 0.064 s (0.21×) |
| nbody | 0.092 s | 0.101 s (1.09×) | 0.076 s (0.82×) | — | 0.076 s (0.82×) |
| prefix_sum | 0.326 s | 0.743 s (2.28×) | 0.337 s (1.03×) | 0.191 s (0.59×) | 0.204 s (0.63×) |
| records | 0.033 s | 0.099 s (3.03×) | 0.114 s (3.49×) | 0.061 s (1.85×) | 0.076 s (2.33×) |
| scale | 0.506 s | 0.580 s (1.14×) | 0.760 s (1.50×) | 0.372 s (0.74×) | 0.391 s (0.77×) |
| sort | 1.874 s | 0.256 s (0.14×) | 0.505 s (0.27×) | 0.157 s (0.08×) | 0.279 s (0.15×) |
| sum_of_squares | 0.039 s | 0.071 s (1.83×) | 0.045 s (1.16×) | 0.048 s (1.24×) | 0.028 s (0.73×) |
| tree | 1.214 s | 1.369 s (1.13×) | 0.429 s (0.35×) | 0.817 s (0.67×) | 0.214 s (0.18×) |

Where Mimic is behind C: `scale` on one core (1.5× on Windows to 3.5× on Linux; C knows the four sizes at compile time and turns every division into a multiplication, and Mimic checks bounds and rounds `div` downwards; on all cores Mimic is ahead of Rayon on macOS and Windows), `records` (one heap block per record: 1.6× to 2.3× C on all cores), and `http` on one core (1.7× to 1.8×). The machines change from run to run (an AMD EPYC 7763, 9V45 or 9V74 for Linux and Windows), and so do C's times: compare within a table.

### On CI machines, with Rust + Rayon (after the speed round, 2026-11-16)

The bench workflow (run 43823317121, commit 33f5e2d) on GitHub's shared runners: compare the ratios, not the times, and expect a few percent of noise between runs. The ratio is to C; Rust + Rayon and Mimic, all cores, are the parallel columns.

**macOS arm64** (3 cores)

| Benchmark | C | Rust | Mimic, 1 core | Rust + Rayon, all cores | Mimic, all cores |
|---|---|---|---|---|---|
| fib | 0.124 s | 0.132 s (1.07×) | 0.132 s (1.07×) | 0.049 s (0.40×) | 0.046 s (0.37×) |
| http | 0.157 s | 0.285 s (1.81×) | 0.396 s (2.52×) | — | 0.385 s (2.45×) |
| json | 0.239 s | 0.143 s (0.60×) | 0.222 s (0.93×) | — | 0.184 s (0.77×) |
| mandelbrot | 0.678 s | 0.709 s (1.04×) | 0.462 s (0.68×) | 0.252 s (0.37×) | 0.160 s (0.24×) |
| matmul | 0.206 s | 0.212 s (1.03×) | 0.032 s (0.15×) | 0.274 s (1.33×) | 0.030 s (0.15×) |
| nbody | 0.040 s | 0.052 s (1.28×) | 0.045 s (1.12×) | — | 0.041 s (1.02×) |
| prefix_sum | 0.113 s | 0.237 s (2.09×) | 0.130 s (1.15×) | 0.085 s (0.75×) | 0.073 s (0.64×) |
| records | 0.013 s | 0.036 s (2.84×) | 0.025 s (1.96×) | 0.027 s (2.12×) | 0.020 s (1.55×) |
| sort | 1.044 s | 0.161 s (0.15×) | 0.120 s (0.11×) | 0.151 s (0.14×) | 0.078 s (0.07×) |
| sum_of_squares | 0.028 s | 0.042 s (1.51×) | 0.028 s (1.00×) | 0.028 s (1.00×) | 0.014 s (0.50×) |
| tree | 0.320 s | 0.355 s (1.11×) | 0.204 s (0.64×) | 0.200 s (0.63×) | 0.103 s (0.32×) |
| ui | — | — | 0.085 s | — | 0.044 s |

**Linux x86_64** (AMD EPYC, 4 logical CPUs: 2 cores × 2 threads)

| Benchmark | C | Rust | Mimic, 1 core | Rust + Rayon, all cores | Mimic, all cores |
|---|---|---|---|---|---|
| fib | 0.076 s | 0.077 s (1.00×) | 0.077 s (1.01×) | 0.031 s (0.40×) | 0.032 s (0.42×) |
| http | 0.188 s | 0.310 s (1.65×) | 0.260 s (1.38×) | — | 0.246 s (1.30×) |
| json | 0.177 s | 0.157 s (0.89×) | 0.175 s (0.99×) | — | 0.165 s (0.93×) |
| mandelbrot | 0.411 s | 0.422 s (1.03×) | 0.141 s (0.34×) | 0.124 s (0.30×) | 0.043 s (0.11×) |
| matmul | 0.099 s | 0.097 s (0.98×) | 0.052 s (0.53×) | 0.247 s (2.49×) | 0.030 s (0.30×) |
| nbody | 0.062 s | 0.071 s (1.14×) | 0.036 s (0.58×) | — | 0.036 s (0.58×) |
| prefix_sum | 0.320 s | 0.352 s (1.10×) | 0.313 s (0.98×) | 0.181 s (0.57×) | 0.165 s (0.52×) |
| records | 0.019 s | 0.075 s (4.02×) | 0.060 s (3.22×) | 0.047 s (2.53×) | 0.035 s (1.89×) |
| sort | 0.421 s | 0.145 s (0.34×) | 0.147 s (0.35×) | 0.095 s (0.22×) | 0.106 s (0.25×) |
| sum_of_squares | 0.009 s | 0.016 s (1.83×) | 0.009 s (0.99×) | 0.009 s (1.04×) | 0.005 s (0.55×) |
| tree | 0.305 s | 0.404 s (1.32×) | 0.227 s (0.74×) | 0.337 s (1.11×) | 0.155 s (0.51×) |
| ui | — | — | 0.072 s | — | 0.048 s |

**Windows x86_64** (4 logical CPUs)

| Benchmark | C | Rust | Mimic, 1 core | Rust + Rayon, all cores | Mimic, all cores |
|---|---|---|---|---|---|
| fib | 0.140 s | 0.123 s (0.88—) | 0.128 s (0.91—) | 0.054 s (0.39—) | 0.058 s (0.41—) |
| http | — | 0.731 s | 0.616 s | — | 0.652 s |
| json | 0.594 s | 0.375 s (0.63—) | 0.330 s (0.56—) | — | 0.296 s (0.50—) |
| mandelbrot | 0.640 s | 0.743 s (1.16—) | 0.418 s (0.65—) | 0.204 s (0.32—) | 0.182 s (0.29—) |
| matmul | 0.137 s | 0.148 s (1.08—) | 0.093 s (0.68—) | 0.456 s (3.34—) | 0.055 s (0.41—) |
| nbody | 0.119 s | 0.127 s (1.06—) | 0.078 s (0.66—) | — | 0.081 s (0.68—) |
| prefix_sum | 0.228 s | 0.483 s (2.12—) | 0.231 s (1.01—) | 0.173 s (0.76—) | 0.184 s (0.81—) |
| records | 0.021 s | 0.087 s (4.18—) | 0.101 s (4.88—) | 0.058 s (2.83—) | 0.069 s (3.36—) |
| sort | 1.684 s | 0.233 s (0.14—) | 0.155 s (0.09—) | 0.145 s (0.09—) | 0.124 s (0.07—) |
| sum_of_squares | 0.036 s | 0.071 s (1.94—) | 0.042 s (1.15—) | 0.092 s (2.54—) | 0.025 s (0.69—) |
| tree | 1.129 s | 1.291 s (1.14—) | 0.345 s (0.31—) | 0.786 s (0.70—) | 0.192 s (0.17—) |
| ui | — | — | 0.130 s | — | 0.088 s |

Since v0.3.13 (the section below): the HTTP server waits on no lock (Linux 1.71× → 1.30× C); prefix sums give each thread one contiguous run of chunks; parallel `fib` runs as fork-join with plain sequential leaves (Linux 0.55× → 0.42× C, Windows 0.61× → 0.41×). Mimic, all cores, against Rayon: ahead or even on every benchmark on macOS; on Linux ahead or even on all but sort (0.106 s against 0.095 s); on Windows ahead on mandelbrot, matmul, sort, sum of squares and tree, and behind on fib (0.058 / 0.054 s), prefix sum (0.184 / 0.173 s) and records (0.069 / 0.058 s, and 4.9× C on one core).

### On CI machines, with Rust + Rayon (v0.3.13, 2026-11-16)

The bench workflow on GitHub's shared runners (3 or 4 cores, other jobs beside them: compare the ratios, not the times). The ratio is to C; Rust + Rayon and Mimic, all cores, are the parallel columns.

**macOS arm64**: Apple M1 (Virtual), Darwin 24.6.0, 3 cores, load average 14.9 at the end.

| Benchmark | C | Rust | Mimic, 1 core | Rust + Rayon, all cores | Mimic, all cores |
|---|---|---|---|---|---|
| fib | 0.140 s | 0.159 s (1.13×) | 0.138 s (0.99×) | 0.065 s (0.46×) | 0.063 s (0.45×) |
| http | 0.286 s | 0.445 s (1.55×) | 0.561 s (1.96×) | — | 0.554 s (1.94×) |
| json | 0.297 s | 0.162 s (0.54×) | 0.276 s (0.93×) | — | 0.224 s (0.76×) |
| mandelbrot | 0.790 s | 0.833 s (1.05×) | 0.568 s (0.72×) | 0.321 s (0.41×) | 0.199 s (0.25×) |
| matmul | 0.286 s | 0.288 s (1.00×) | 0.050 s (0.17×) | 0.410 s (1.43×) | 0.048 s (0.17×) |
| nbody | 0.049 s | 0.059 s (1.22×) | 0.049 s (1.00×) | — | 0.056 s (1.14×) |
| prefix_sum | 0.188 s | 0.311 s (1.65×) | 0.156 s (0.83×) | 0.123 s (0.65×) | 0.106 s (0.57×) |
| records | 0.015 s | 0.047 s (3.10×) | 0.034 s (2.23×) | 0.043 s (2.78×) | 0.023 s (1.53×) |
| sort | 1.246 s | 0.209 s (0.17×) | 0.166 s (0.13×) | 0.181 s (0.15×) | 0.102 s (0.08×) |
| sum_of_squares | 0.031 s | 0.045 s (1.48×) | 0.032 s (1.05×) | 0.038 s (1.24×) | 0.018 s (0.60×) |
| tree | 0.413 s | 0.511 s (1.24×) | 0.258 s (0.62×) | 0.350 s (0.85×) | 0.106 s (0.26×) |
| ui | — | — | 0.119 s | — | 0.114 s |

**Linux x86_64**: AMD EPYC 7763 64-Core Processor, Linux 7.0.0-1017-azure, 4 cores, load average 1.7 at the end.

| Benchmark | C | Rust | Mimic, 1 core | Rust + Rayon, all cores | Mimic, all cores |
|---|---|---|---|---|---|
| fib | 0.113 s | 0.121 s (1.07×) | 0.131 s (1.16×) | 0.048 s (0.43×) | 0.062 s (0.55×) |
| http | 0.302 s | 0.732 s (2.43×) | 0.585 s (1.94×) | — | 0.515 s (1.71×) |
| json | 0.336 s | 0.273 s (0.81×) | 0.318 s (0.95×) | — | 0.291 s (0.86×) |
| mandelbrot | 0.578 s | 0.655 s (1.13×) | 0.446 s (0.77×) | 0.180 s (0.31×) | 0.184 s (0.32×) |
| matmul | 0.203 s | 0.203 s (1.00×) | 0.078 s (0.39×) | 0.411 s (2.02×) | 0.045 s (0.22×) |
| nbody | 0.100 s | 0.111 s (1.11×) | 0.065 s (0.65×) | — | 0.065 s (0.65×) |
| prefix_sum | 0.456 s | 0.505 s (1.11×) | 0.457 s (1.00×) | 0.262 s (0.57×) | 0.253 s (0.56×) |
| records | 0.029 s | 0.087 s (3.00×) | 0.085 s (2.93×) | 0.054 s (1.87×) | 0.051 s (1.74×) |
| sort | 0.907 s | 0.236 s (0.26×) | 0.224 s (0.25×) | 0.142 s (0.16×) | 0.142 s (0.16×) |
| sum_of_squares | 0.031 s | 0.058 s (1.86×) | 0.032 s (1.01×) | 0.076 s (2.42×) | 0.016 s (0.51×) |
| tree | 0.528 s | 0.633 s (1.20×) | 0.346 s (0.66×) | 0.491 s (0.93×) | 0.175 s (0.33×) |
| ui | — | — | 0.105 s | — | 0.069 s |

**Windows x86_64**: Intel64 Family 6 Model 207 Stepping 2, GenuineIntel, Windows 2025Server, 4 cores, load average 0.0 at the end.

| Benchmark | C | Rust | Mimic, 1 core | Rust + Rayon, all cores | Mimic, all cores |
|---|---|---|---|---|---|
| fib | 0.109 s | 0.094 s (0.86×) | 0.135 s (1.24×) | 0.043 s (0.40×) | 0.066 s (0.61×) |
| http | — | 0.550 s | 0.477 s | — | 0.554 s |
| json | 0.480 s | 0.285 s (0.59×) | 0.329 s (0.69×) | — | 0.301 s (0.63×) |
| mandelbrot | 0.541 s | 0.573 s (1.06×) | 0.183 s (0.34×) | 0.168 s (0.31×) | 0.068 s (0.13×) |
| matmul | 0.264 s | 0.269 s (1.02×) | 0.083 s (0.31×) | 0.421 s (1.59×) | 0.053 s (0.20×) |
| nbody | 0.076 s | 0.083 s (1.09×) | 0.062 s (0.82×) | — | 0.062 s (0.81×) |
| prefix_sum | 0.288 s | 0.669 s (2.32×) | 0.300 s (1.04×) | 0.161 s (0.56×) | 0.182 s (0.63×) |
| records | 0.025 s | 0.084 s (3.32×) | 0.096 s (3.79×) | 0.050 s (1.98×) | 0.065 s (2.56×) |
| sort | 1.616 s | 0.220 s (0.14×) | 0.414 s (0.26×) | 0.136 s (0.08×) | 0.243 s (0.15×) |
| sum_of_squares | 0.032 s | 0.060 s (1.86×) | 0.037 s (1.14×) | 0.040 s (1.25×) | 0.023 s (0.72×) |
| tree | 1.049 s | 1.180 s (1.12×) | 0.372 s (0.35×) | 0.682 s (0.65×) | 0.186 s (0.18×) |
| ui | — | — | 0.125 s | — | 0.084 s |

Mimic is ahead of Rayon on all eight benchmarks on macOS, on five of eight on Linux (fib behind, mandelbrot and sort even), and on four on Windows, where fib, prefix sum, records and sort are behind, and records on one core is 3.8× C (2.2–2.9× elsewhere). The HTTP server is 1.7–1.9× C everywhere: under load about 30% of its busy time waits on the event loop's lock ([gaps.md](docs/gaps.md)).

### Against Rust + Rayon (2026-11-16)

Apple M3 Pro, 12 cores, macOS, load average 11–15 (other work running). Best of 5 runs; the ratio is to C. Rust + Rayon and Mimic, all cores, are the two parallel columns; nbody (five bodies, one step after another) has no Rayon version.

| Benchmark | C | Rust | Mimic, 1 core | Rust + Rayon, all cores | Mimic, all cores |
|---|---|---|---|---|---|
| fib(38) | 0.512 s | 1.11× | 1.02× | **0.16×** | 0.20× |
| mandelbrot | 2.406 s | 1.15× | 0.73× | 0.15× | **0.11×** |
| matmul 1024² | 0.778 s | 1.00× | 0.12× | 0.60× | **0.11×** |
| nbody, 1M steps | 0.173 s | 1.26× | 1.00× | — | 1.00× |
| prefix sum, 100M | 0.420 s | 2.44× | 0.97× | **0.37×** | 0.63× |
| records | 0.041 s | 3.30× | 2.46× | 2.13× | **1.14×** |
| sort 10M integers | 4.371 s | 0.15× | 0.09× | 0.06× | 0.06× |
| sum of squares, 100M | 0.120 s | 1.63× | 1.00× | 0.49× | **0.24×** |
| tree | 1.395 s | 1.14× | 0.57× | 0.52× | **0.13×** |

Mimic with no annotations is faster than hand-written Rayon on six of the eight, and ties on sort. Rayon wins two:
* **prefix sum (0.37× against 0.63×).** The Rayon version writes the running sums in place into one buffer, in chunks of a million; Mimic's two-pass scan makes the first pass's sums and then a second array.
* **fib (0.16× against 0.20×).** `rayon::join` steals work with less overhead per split than Mimic's fork-join, which also guards each native call's depth.

Matmul with Rayon is the plain loop split by rows (0.60×); Mimic's kernel is blocked and SIMD (0.11×) on one core already.

### Linux and Windows in CI (2026-11-14)

The bench workflow on GitHub's 4-core runners, commit 240d7d8, best of 5
runs; the ratio is to C (below 1.0 is faster than C). The two runners have
different CPUs, so compare each row with its own C, not across systems.

**Linux x86_64** (AMD EPYC 9V74, Zen 4 with AVX-512):

| Benchmark | C | Rust | Mimic, 1 core | Mimic, all cores |
|---|---|---|---|---|
| fib | 0.091 s | 1.15× | 1.24× | **0.58×** |
| http | 0.224 s | 2.24× | 1.77× | 1.60× |
| json | 0.240 s | 0.89× | 1.04× | **0.95×** |
| mandelbrot | 0.490 s | 1.16× | **0.31×** | **0.12×** |
| matmul | 0.160 s | 1.00× | **0.39×** | **0.23×** |
| nbody | 0.088 s | 1.06× | **0.60×** | **0.60×** |
| prefix_sum | 0.364 s | 1.08× | 1.00× | **0.61×** |
| records | 0.021 s | 4.32× | 3.95× | 2.14× |
| sort | 0.705 s | 0.27× | **0.25×** | **0.16×** |
| sum_of_squares | 0.014 s | 2.66× | 1.01× | **0.56×** |
| tree | 0.468 s | 1.36× | **0.63×** | **0.45×** |
| ui | — | — | 0.084 s | 0.056 s |

**Windows x86_64** (AMD Zen 3, AVX2, no AVX-512; Windows Server 2025):

| Benchmark | C | Rust | Mimic, 1 core | Mimic, all cores |
|---|---|---|---|---|
| fib | 0.127 s | 0.94× | 1.18× | 1.37× |
| http | — | 0.962 s | 0.761 s | 0.819 s |
| json | 0.593 s | 0.59× | **0.56×** | **0.50×** |
| mandelbrot | 0.585 s | 1.13× | 1.07× | **0.50×** |
| matmul | 0.160 s | 0.88× | **0.51×** | **0.33×** |
| nbody | 0.106 s | 1.10× | **0.86×** | **0.86×** |
| prefix_sum | 0.188 s | 2.21× | 1.05× | 1.42× |
| records | 0.022 s | 3.91× | 3.72× | 2.37× |
| sort | 1.803 s | 0.12× | **0.08×** | **0.06×** |
| sum_of_squares | 0.035 s | 1.79× | 2.52× | 1.36× |
| tree | 1.064 s | 1.13× | **0.33×** | **0.22×** |
| ui | — | — | 0.124 s | 0.082 s |

http has no C row on Windows (the C server is POSIX-only); the times are
the server's for 400,000 requests.

Windows was behind Linux in four places; the speed probe
(`tools/speed-probe.py`, a manual workflow) found why, and all four are
fixed (2026-11-15):
* **Windows code was built for the 2003 x86-64 baseline** (no AVX2, no
  FMA): the MinGW host triple took Clang's cross-compiling defaults. That
  was mandelbrot without SIMD and sum_of_squares at 2.5× C.
* **Idle workers spun** (`Sleep(0)` returns at once), taking the cores the
  working threads needed: fib on all cores slower than on one.
* **The parallel scan's small chunks** cost more than they gave on Windows:
  prefix_sum on 4 threads slower than on one.

**Windows x86_64 after the fixes** (commit ea6c780; this run's runner had an
AMD Zen 5, so compare ratios, not times, with the table above):

| Benchmark | C | Rust | Mimic, 1 core | Mimic, all cores |
|---|---|---|---|---|
| fib | 0.080 s | 1.09× | 1.35× | **0.67×** |
| http | — | 0.442 s | 0.341 s | 0.318 s |
| json | 0.419 s | 0.67× | **0.52×** | **0.47×** |
| mandelbrot | 0.437 s | 0.99× | **0.33×** | **0.13×** |
| matmul | 0.090 s | 1.06× | **0.69×** | **0.42×** |
| nbody | 0.069 s | 1.13× | **0.70×** | **0.70×** |
| prefix_sum | 0.183 s | 2.09× | 1.05× | **0.79×** |
| records | 0.021 s | 3.95× | 3.47× | 2.53× |
| sort | 1.225 s | 0.14× | **0.10×** | **0.09×** |
| sum_of_squares | 0.014 s | 1.59× | 1.32× | 1.09× |
| tree | 0.866 s | 1.15× | **0.28×** | **0.21×** |
| ui | — | — | 0.106 s | 0.068 s |

### All benchmarks (after F, 2026-11-13 evening)

Apple M3 Pro, 12 cores, `bench/run.py`, load average 5 to 7 from other
programs; the ratio is to C.

| Benchmark | C | Rust | Mimic, 1 core | Mimic, all cores |
|---|---|---|---|---|
| fib | 0.537 s | 1.07× | 1.01× | **0.15×** |
| json | 1.048 s | 0.59× | **0.85×** | **0.68×** |
| mandelbrot | 2.727 s | 1.16× | **0.71×** | **0.10×** |
| nbody | 0.174 s | 1.26× | 1.00× | 1.00× |
| prefix_sum | 0.429 s | 2.44× | **0.97×** | **0.66×** |
| records | 0.042 s | 3.40× | 2.24× | **1.00×** |
| sum_of_squares | 0.124 s | 1.70× | 1.00× | **0.24×** |
| tree | 1.444 s | 1.16× | **0.57×** | **0.12×** |

mandelbrot's one-core time is with SIMD across elements (8 points at once,
see design.md); C and Rust do one point at a time.

All of these are f64 or i64 programs, so the GPU takes no part (Apple
GPUs have no f64). An f32 Mandelbrot of 4M points: 0.30 s on all CPU
cores, 0.15 s with the GPU (kernel 18 ms).

### All benchmarks (after F3)

Apple M3 Pro, 12 cores, 2026-11-13, best of 5 runs; the ratio is to C.
Other programs kept the load average near 10, so all-core times are on the
slow side.

| Benchmark | C | Rust | Mimic, 1 core | Mimic, all cores |
|---|---|---|---|---|
| fib | 0.567 s | 1.08× | 1.03× | **0.19×** |
| http | 0.706 s | 2.16× | 1.88× | 1.27× |
| json | 1.134 s | 0.60× | **0.97×** | **0.75×** |
| mandelbrot | 2.639 s | 1.16× | 1.10× | **0.13×** |
| matmul | 0.895 s | 1.01× | **0.09×** | **0.08×** |
| nbody | 0.192 s | 1.31× | 1.03× | 1.04× |
| prefix_sum | 0.475 s | 2.32× | **0.99×** | **0.63×** |
| records | 0.044 s | 3.41× | 7.43× | 3.50× |
| sort | 4.894 s | 0.15× | **0.07×** | **0.03×** |
| sum_of_squares | 0.134 s | 1.60× | 1.01× | **0.23×** |
| tree | 1.566 s | 1.15× | **0.60×** | **0.14×** |

(http's C is a floor that never parses requests; json's C a hand-written
parser for one record shape. records measured 5.9× on its own run.)

### records: reuse in place (27× → 5.9× C)

A million records `{id, name, x, v}` (a string and numbers), updated five
times with `{ p with x: …, v: … }` over the whole list, then summed. C and
Rust update their arrays in place; Mimic's values are immutable,
so in principle each step builds a new list of new records. Apple M3 Pro,
2026-11-13.

| Benchmark | C | Rust | Mimic, 1 core | Mimic, all cores |
|---|---|---|---|---|
| records | 0.046 s | 3.33× | **5.89×** | **2.53×** |

| Step | 1 core | All cores |
|---|---|---|
| before | 1.94 s CPU (about 40× C) | — |
| numbers stored raw in records; `with` inline; `¨` lends and moves | 1.208 s (26.7×) | 0.542 s (12.0×) |
| reuse in place (consumed parameters, moving locals, `¨` hands elements over) | 0.538 s (11.9×) | 0.276 s (6.1×) |
| `¨` over numbers without boxing them | 0.407 s (9.0×) | 0.212 s (4.7×) |
| `¨` over a list of records fused into the loop around it | 0.270 s (5.9×) | 0.116 s (2.5×) |
| `with` checks for the only reference inline | 0.189 s (4.6×) | 0.087 s (2.1×) |
| chains `xs ¨ f ¨ g ¨ …` fused: each record through every step while it is at hand | 0.185 s (3.9×) | 0.077 s (1.6×) |
| `xs ¨ f` making records fused (no `⍳` list, no boxed call, in parallel); nothing freed one by one at exit; a moved-out local costs no release call | **0.094 s (2.2×)** | **0.042 s (1.0×)** |

* **Raw number fields:** numbers in records and variants are stored in
  the object itself (8 bytes), not as a boxed value per field (the
  program's own types on 64-bit targets; generic fields and the standard
  library's types keep handles).
* **Reuse in place:** a value used once is handed over at that use instead
  of shared; a function whose record parameter goes into `{ p with … }`
  takes it owned; `xs ¨ f` hands the elements of a list nothing else holds
  to such an `f`. `with` then finds the only reference and writes in
  place: after the first step no record is allocated or freed.
* **`¨` without boxes:** over a list of numbers, each element reaches the
  function as it is; over a list of records feeding `+/` (or any fused
  expression), the records are read in the loop and nothing is built.

A chain of `¨` with pure functions is now one pass: `ps ¨ step ¨ step`
is `ps ¨ (p => step(step(p)))`, with no list between the steps, and each
record goes through all five steps while it is in the cache (updated in
place, the list's elements handed over). Rust (3.3×) is
close; what is left is making a million separate records (C has one
array of structs).

Making the records is now one fused parallel pass too, and in release
builds the records `main` holds at the end are not freed one by one (the
process exits next; debug builds still free all and check for leaks):
that freeing was two thirds of the time. All cores now match C (0.042 s);
one core is 2.2× C, the cost of separate records with headers and
reference counts against one array of structs.

### http: a hello-world server (faster than hyper on one core)

Each version is a server answering `Hello, World!`; a load generator
(`bench/http/load`, in Rust) sends 400,000 requests over 64 keep-alive connections,
16 pipelined per write, and checks every response. The time is the load
generator's; it runs on the same machine. Apple M3 Pro, 2026-11-13.

| Benchmark | C | Rust (hyper) | Mimic, 1 core | Mimic, all cores |
|---|---|---|---|---|
| http | 0.712 s | 1.523 s (2.14×) | **1.336 s (1.88×)** | **0.962 s (1.35×)** |

The C version is a floor rather than an HTTP server: one kqueue loop that
finds the end of each request and copies a fixed answer, without reading
the request line or headers. Against real servers, Mimic on one core
(about 300,000 requests a second) is ahead of hyper (260,000).

What it took:

* **keep-alive:** the server used to answer one request per connection
  (`Connection: close`); it now serves request after request on a
  connection, keeping pipelined bytes for the next request;
* **one write for pipelined answers:** while the next request is already
  read, answers wait and then leave together;
* the status line and `Content-Length` are written without printf.

All cores gain less (0.96 s) because the load generator shares the
machine; one server thread already keeps up with most of what it sends.

### json: encode and decode (1.10× C)

Build 300,000 records `{id, name, score, tags}`, encode them to one JSON
text (21 MB), decode it back into typed records, check them. C uses a
hand-written encoder and parser for this one shape (snprintf/strtod), Rust
serde + serde_json. Apple M3 Pro, 2026-11-13.

| Benchmark | C | Rust | Mimic, 1 core | Mimic, all cores |
|---|---|---|---|---|
| json | 1.238 s | 0.59× | **1.10×** | **0.79×** |

From 2.9× C (3.1 s of CPU) to 1.10×:

* `json::decode[T]` reads the text straight into typed values, following
  T's schema: no intermediate tree, no copied keys, strings without
  escapes taken from the text in one piece. On any error it re-reads the
  text the old way, so the messages are unchanged;
* integers are formatted and parsed without printf/strtoll, and decimals
  that are exact in a double (`0.5`, `123.456`) without printf/strtod;
* lists are encoded element by element in place (no boxing), strings in runs;
* `"item-{i}"` with an integer formats it straight to text.

serde_json is still faster (0.59×): it writes into one buffer with no
reference counts at all. All cores: only building the records runs in
parallel; encoding and decoding one text are sequential.

### ui: a 10,000-row list, frame by frame

`view` builds the screen of a 10,000-row todo list and `ui::diff`
compares it with the previous one, as `ui::run` does for every message;
each of 20 frames ticks one row. There is no C or Rust counterpart
(it measures Mimic's own ui module). Apple M3 Pro, 2026-11-13.

| Benchmark | Mimic, 1 core | Mimic, all cores |
|---|---|---|
| ui (20 frames), every row built | 3.483 s (174 ms a frame) | 0.786 s (39 ms a frame) |
| ui (20 frames), unchanged rows reused | **0.363 s** (including the first screen) | **0.226 s** |

Measuring it first took 250 ms a frame on one core. General compiler
work brought it to 174 ms: `≢` of a list reads its length; indexing a
list of records, strings or tuples is a bounds check and a load; list
literals of those are built in place with their elements moved in.
Then rows whose data did not change are not built again (decided
2026-11-13: automatic, nothing to write): `xs ¨ row` over a list of
values, with a pure `row` capturing nothing, keeps the last run's rows by
element (mim_each_memo), and an element that is the very same value gets
its old row back; `ui::diff` skips identical parts before even making
their paths. 3.48 s → 0.36 s for the 20 frames (about 10×).

### Performance and efficiency cores: nothing to change

The M3 Pro has 6 performance and 6 efficiency cores. Work is handed out
dynamically everywhere (loop chunks claimed one by one, fork-join tasks
stolen), so slower cores simply take fewer pieces. Asking macOS to run
the workers on performance cores (quality of service "user interactive")
changed nothing measurable (best of 14 runs: mandelbrot 0.275 → 0.272 s,
sum_of_squares 0.024 s both, fib 0.075 → 0.099 s): macOS already favours
them for a busy command-line program. Not adopted.

### Non-atomic reference counts: no gain

Reference counts are updated with atomic instructions, so values can be
shared between threads. Replacing them with plain increments and
decrements (unsafe across threads; a measurement only) changed nothing on
one core: json 0.90 s, tree 0.73 s, records 0.19 s of CPU either way.
Uncontended atomics cost about nothing on Apple silicon, and the compiler
already avoids most counting (borrowed parameters and bindings, moves,
values built in place). Not adopted.

### Link-time optimisation (F4): no gain

Linking the runtime into each program as LLVM bitcode (full LTO, so
runtime functions can be inlined) changed nothing measurable: fib,
nbody, sum_of_squares, prefix_sum and tree all stayed within noise
(±5%). The hot runtime paths (allocation, release, tag reads) are already
generated inline by the compiler. Builds stay as they are: a cached
runtime object, linked as is.

### Fork-join (F6): independent calls on all cores

Apple M3 Pro, 12 cores, 2026-11-13, best of 5 runs; the machine was busy
(load average about 10 from other programs), so all-core times are on the
slow side.

| Benchmark | C | Rust | Mimic, 1 core | Mimic, all cores |
|---|---|---|---|---|
| fib | 0.590 s | 1.04× | 1.02× | **0.18×** |
| mandelbrot | 2.968 s | 1.02× | 1.01× | **0.11×** |
| matmul | 0.871 s | 1.14× | 0.09× | **0.08×** |
| nbody | 0.209 s | 1.23× | 1.01× | 0.95× |
| prefix_sum | 0.552 s | 2.16× | 0.82× | **0.54×** |
| sort | 4.842 s | 0.15× | 0.07× | **0.03×** |
| sum_of_squares | 0.132 s | 1.62× | 1.04× | **0.23×** |
| tree | 1.578 s | 1.18× | **0.54×** | **0.15×** |

tree went from 0.96× / 0.74× to **0.54× / 0.15×**:

* `Node(make(d - 1), make(d - 1))` and `1 + check(l) + check(r)` fork and
  join: the big tree is now built and counted on all cores;
* small blocks are carved from per-thread 256 KB chunks with a shared pool,
  so threads never wait on `malloc`;
* values of 8-byte elements no longer carry a NUL byte: a two-field node is
  a 56-byte block instead of 112 (memory 474 MB → 238 MB);
* freeing a large value shares its subtrees out to the same pool;
* `match` borrows the fields it binds instead of retaining them.

### tree: allocation-heavy code (0.96× C)

Build and count binary trees: 64 trees of depth 16, then one of depth 22
(8.4 million nodes). Apple M3 Pro, 2026-11-13.

| Benchmark | C | Rust | Mimic, 1 core | Mimic, all cores |
|---|---|---|---|---|
| tree | 1.414 s | 1.13× | **0.96×** | **0.74×** |

From 2.3× C to 0.96×:

* a variant without fields (`Leaf`, `None`) is one shared value, never
  allocated again (half the nodes of a tree are leaves);
* records and variants are built inline, and fields that are already owned
  move in without a retain/release pair;
* `match` reads the tag and the fields in place;
* each thread keeps up to 2^18 freed blocks per size class, so a freed
  tree's nodes serve the next tree instead of going back to `malloc`;
* a compiled `¨` whose function can recurse times its first element and
  shares out the rest, even when there are only a few elements: the 64
  small trees now build on all cores (0.41 s → 0.11 s).

Still to do: the one big tree is built on one core (its two halves are
independent: general fork-join, roadmap F6), and Mimic's nodes take 64
bytes against C's 16 (memory 474 MB vs 136 MB).

### Single core within 10% of C on every benchmark

Apple M3 Pro, 12 cores, 2026-11-13. Best of 5 runs; the ratio is to C.

| Benchmark | C | Rust | Mimic, 1 core | Mimic, all cores |
|---|---|---|---|---|
| fib(38) | 0.512 s | 1.13× | 1.27× | **0.19×** |
| mandelbrot | 2.407 s | 1.15× | 1.09× | **0.11×** |
| matmul 1024² | 0.779 s | 1.00× | **0.09×** | **0.08×** |
| nbody, 1M steps | 0.172 s | 1.27× | **1.02×** | **1.03×** |
| prefix sum, 100M | 0.410 s | 2.48× | **0.98×** | **0.68×** |
| sort 10M integers | 4.396 s | 0.14× | **0.06×** | **0.03×** |
| sum of squares, 100M | 0.119 s | 1.64× | **1.01×** | **0.21×** |

Mimic is now at least as fast as Rust on every benchmark but fib,
and within a few percent of C on one core on all but fib and mandelbrot.

* **32-bit kernel indices.** When the program states a list's length and
  it fits in 32 bits (`⍳ 100000000`), a kernel computes with its index as a
  visibly small number, so LLVM works in 32-bit SIMD lanes, as C does
  (sum of squares 2.40× → 1.01×).
* **One-pass scans on one thread.** A scan running on one thread (or one
  chunk) skips the chunk-totals pass (prefix sum 1.45× → 0.98×).
* **Fused multiply-add by default.** Like C compilers, `a * b + c` in one
  expression may become one fused multiply-add; `--strict-math` turns it
  off for bit-identical results on every CPU (mandelbrot 1.18× → 1.09×).
* **fib**, measured after the table: **1.01×** C on one core (0.526 s vs
  0.518 s; Rust 1.10×) and 0.15× on all cores. Native recursion
  guards its depth so very deep recursion moves to the interaction-net
  runtime instead of overflowing the stack; a function of plain numbers
  now checks only right before it calls itself and, past the limit,
  starts the whole call over on the net (it is pure, and holds nothing on
  the heap), so calls that end the recursion pay nothing.

### Small lists and records in registers (nbody 8.6× → 1.09× C)

Apple M3 Pro, 12 cores, 2026-11-13. Best of 5 runs; the ratio is to C.

| Benchmark | C | Rust | Mimic, 1 core | Mimic, all cores |
|---|---|---|---|---|
| fib(38) | 0.527 s | 1.09× | 1.23× | **0.21×** |
| mandelbrot | 2.414 s | 1.15× | 1.18× | **0.12×** |
| matmul 1024² | 0.778 s | 1.03× | **0.09×** | **0.08×** |
| nbody, 1M steps | 0.172 s | 1.27× | **1.09×** | **1.09×** |
| prefix sum, 100M | 0.408 s | 2.49× | 1.45× | **0.69×** |
| sort 10M integers | 4.370 s | 0.15× | **0.08×** | **0.03×** |
| sum of squares, 100M | 0.122 s | 1.61× | 2.40× | **0.36×** |

nbody now runs as fast as C on one core and faster than Rust, with
the program unchanged. What got it there:

* **Lists of known length.** A list of numbers whose length the compiler
  can see (a literal, `⍳ 5`, elementwise results, up to 8 elements) has
  that length in its type, and functions are specialised on it. Such
  lists live in registers: no heap, no header, indexing checked against a
  constant.
* **Shaped and flat records.** A record built from such lists remembers
  their lengths. When all its fields are numbers or such lists, the record
  itself is a plain struct in registers; `Sys` (6 × 5 doubles) never
  touches the heap.
* **`evolve` as a loop.** With a step that keeps the state's type, the
  loop is compiled in place and calls the step as a typed function.
* **Full unrolling of short loops** of a known length (after inlining):
  indices become constants, as in C's `for (i = 0; i < 5; i++)`.
* Along the way: a value's static type now says how it is stored (a record
  field declared `[f64]` holds floats), indexing is one compare against
  the header, and allocation and freeing of short vectors are inline.

fib(38) also improved (1.90× → 1.23×) from the inline allocator.

### Smaller overheads (nbody 16.6× → 8.6× C)

Same machine, load average about 7:

| Benchmark | C | Rust | Mimic, 1 core | Mimic, all cores |
|---|---|---|---|---|
| fib(38) | 0.509 s | 1.08× | 1.90× | **0.35×** |
| mandelbrot | 2.475 s | 1.14× | 1.16× | **0.13×** |
| matmul 1024² | 0.794 s | 1.01× | **0.09×** | **0.08×** |
| nbody, 1M steps | 0.178 s | 1.23× | 8.98× | 8.56× |
| prefix sum, 100M | 0.425 s | 2.46× | 1.43× | **0.64×** |
| sort 10M integers | 4.485 s | 0.15× | **0.06×** | **0.03×** |
| sum of squares, 100M | 0.122 s | 1.63× | 2.48× | **0.38×** |

What changed:
* **Small loops run directly.** A fused loop that fits in one chunk calls its kernel directly, with its partial results on the stack. Larger ones still go to the thread pool.
* **A second free list** (blocks up to 64 bytes) recycles short vectors, small records and short strings per thread instead of calling `malloc` and `free`.
* **Fewer runtime calls.** Kernels read vector lengths, data pointers and the chunk size inline. Array reads in compiled code have a never-returning error path, so LLVM keeps array headers in registers.
* **nbody in Mimic** now computes each pair's `m / d³` once per step and uses it for x, y and z, as the C version does.

The rest of nbody's gap is the boxed small arrays and records that every step allocates.

### The suite with seven benchmarks

Same machine (Apple M3 Pro, 12 cores), load average 6–10, best of 5 after a warm-up. The ratio is to C:

| Benchmark | C | Rust | Mimic, 1 core | Mimic, all cores |
|---|---|---|---|---|
| fib(38) | 0.525 s | 1.14× | 1.96× | **0.31×** |
| mandelbrot 2000², 200 steps | 2.386 s | 1.16× | 1.17× | **0.12×** |
| matmul 1024² | 0.780 s | 0.99× | **0.09×** | **0.09×** |
| nbody, 1M steps | 0.199 s | 1.07× | 16.6× | 16.9× |
| prefix sum, 100M | 0.443 s | 2.45× | 1.45× | **0.65×** |
| sort 10M integers | 4.706 s | 0.14× | **0.07×** | **0.03×** |
| sum of squares, 100M | 0.130 s | 1.63× | 2.40× | **0.37×** |

Notes:
* **matmul.** Mimic's `⊠` uses the system's BLAS (Apple Accelerate here) instead of the triple loop the other versions run.
* **nbody is the weak spot.** It runs a million tiny steps over 5 bodies, with records and closures in every step. It started out about 13 000× slower than C. The fixes so far:
  * small `¨` loops run on the calling thread instead of waking the pool;
  * fused `¨` lambdas may capture records;
  * the fields of records of a known type are read directly, borrowed with no reference count.
  
  What remains is boxing and allocation per step: records and short arrays are heap values. Unboxed records and fewer reference-count updates are the next steps.

Changes for these results:
* **`math` module.** `math::sqrt`, the trigonometric, exponential and logarithmic functions, `pow`, rounding, `pi()` and `e()`. On known numbers each one is an LLVM intrinsic or a libm call, so it fuses into kernels.
* **`trail` and `gather` of `+ × max min` over numbers** run as a two-pass parallel scan without boxing: prefix sum 51 s → 0.64 s. Over a fusible expression, `trail` builds no input array at all.
* **`¨` on small inputs** times its first element and only spreads out the rest when that pays off.

### The suite, first run (`bench/run.py`)

Apple M3 Pro (12 cores), macOS 26.6, load average 6–10 (other work running), best of 5 after a warm-up. The ratio is to C. C and Rust run their straightforward sequential versions.

| Benchmark | C | Rust | Mimic, 1 core | Mimic, all cores |
|---|---|---|---|---|
| fib(38) | 0.556 s | 1.11× | 1.85× | **0.23×** |
| mandelbrot 2000², 200 steps | 2.607 s | 1.14× | 1.16× | **0.12×** |
| sort 10M integers | 4.726 s | 0.15× | **0.07×** | **0.04×** |
| sum of squares, 100M | 0.126 s | 1.64× | 2.47× | **0.37×** |

On all cores Mimic beats C on every benchmark. On one core it beats C at sorting and is close on mandelbrot. Where it is still behind:
* **fib on one core (1.85×)** runs through the interaction nets that make it parallel. Plain native code was 1.42× earlier (`--no-net`).
* **sum of squares on one core (2.47×)**: Clang knows this loop runs fewer than 2³² times, so it narrows the counter to 32 bits and computes `% 1000` in NEON vector registers. A Mimic kernel only learns its bounds when it runs. A 32-bit copy of each kernel loop would close this gap.

Fixes made while building the suite:
* **Sorting numbers** used a merge sort over boxed values: 24.4 s. It is now a parallel radix sort: 0.33 s on one core, 0.17 s on all cores.
* **Functions called from a fused `¨` kernel** went through the interaction nets once per element: mandelbrot took minutes. They now stay native.
* **Numbers written in the program** are constants inside kernels, so `x % 1000` is a multiply and a shift instead of a division. Kernels also tell LLVM that their indices are never negative, which drops the sign fix-ups of floored `%`.

### Phase F step 1: typed `¨` kernels and direct indexing

`xs ¨ f` with a pure `f` on numbers now compiles into the fused kernels: `f` becomes a native function on unboxed numbers, called (and usually inlined) inside one parallel loop, with no boxing and no temporary arrays. Lambdas may capture numbers and numeric arrays. `xs[i]` on a numeric vector reads the element directly after a bounds check, and falls back to the general path for errors and matrices.

20 million elements on the M3 Pro, best of 3. The C version is a plain `-O3 -mcpu=native` loop:

| Expression | Before | Now, all cores | Now, 1 thread | C, 1 thread |
|---|---|---|---|---|
| `⍳ n ¨ (x => (x × x + 7) % 1000) ▷ +/` | 1437 ms | **9 ms** | 63 ms | 59 ms |
| `⍳ n ¨ step ▷ +/` (a named function) | 1412 ms | **10 ms** | 61 ms | 59 ms |
| `⍳ n ¨ (i => xs[i] × 2) ▷ +/` | 7129 ms | **58 ms** | 131 ms | — |

So `¨` went from 24× slower than C to C's speed on one core, and about 6× faster on all cores. `¨` over records, strings or other boxed values still takes the general path.

### After M3 performance step 1: fusion, cheaper division, real loops

These are the showcase's compute benchmarks (`examples/showcase`, run with `make compare` and `make parallel`). Measured on the same M3 Pro under load average 9. Times are best of 3:

| Algorithm | C `-O3 -mcpu=native` | Mimic, 1 thread | Mimic, all cores |
|---|---|---|---|
| Mandelbrot (1000², limit 500) | 1.38 s | 1.41 s | 0.20 s |
| Primes below 3M (trial division) | 1.48 s | **1.43 s** | 0.32 s |
| Longest Collatz chain below 3M | 2.21 s | **2.14 s** | 0.33 s |
| π, Monte Carlo (10M points) | 0.07 s | 0.17 s | 0.03 s |
| π, integration (20M slices) | 0.09 s | 0.10 s | 0.02 s |
| **Total** | **4.81 s** | **4.61 s (≈ C)** | **1.04 s (4.6× faster)** |

Before this step the totals were 10.3 s on 1 thread and 1.7 s on all cores. The changes:

| Change | Effect |
|---|---|
| **Kernel fusion.** Elementwise chains, reductions, and `let`s used only inside them compile to one parallel loop with no temporary arrays. | Monte Carlo 0.96 → 0.17 s, integration 0.81 → 0.10 s |
| **Floored `%` with one division** instead of two | primes ≈ 2× faster |
| **`÷`/`%` by a power of two become a shift / mask** (exact for negatives) | Collatz step |
| **Self-tail-recursion without the native depth check** in optimised builds (LLVM turns it into a loop) | tight loops, same shape as C |

Results stay identical between 1 thread and all cores. A fixture runs every fused kernel with one element per chunk, to test that the partial results combine correctly.



### After M3 step 2: static types and unboxed scalars

Measured on an Apple M3 Pro (6 performance + 6 efficiency cores) under background load (load average 6–10). Times are best of 5. The code is unmodified Mimic with one annotation (`n: i64`):

```mimic
fn fib(n: i64) -> i64 {
    if n < 2 {
        return n
    }
    fib(n - 1) + fib(n - 2)
}
```

| fib(38) | Time | vs C |
|---|---|---|
| C `-O3 -mcpu=native` | 0.65 s | 1.0× |
| Mimic `--no-net`, 1 core | 0.92 s | **1.42×** (target ≤ 1.5× ✅) |
| Mimic nets, 1 thread | 1.07 s | 1.65× |
| Mimic nets, 2 threads | 0.69 s | 1.06× |
| Mimic nets, 4 threads | 0.31 s | **0.48×** |
| Mimic nets, 6 threads | 0.21 s | **0.32×** |
| Mimic nets, 12 threads | 0.12 s | **0.18×, 5.4× faster than C** |

The scalar group's targets, ≤ 1.5× of C on one core and < 1.0× on all cores, are both met for `fib`.

### History

| Milestone | fib(32), 1 core | fib(32), 12 threads | Main change |
|---|---|---|---|
| M2 phase 3 | 1.59 s | 0.38 s | every scalar boxed on the heap |
| M3 step 2 | **0.05 s** | **0.02 s** | static types; scalars in registers; hidden native depth argument |
| C reference | 0.03 s | 0.03 s (sequential) | |

### Earlier array-layer measurements (M2)

| Workload | Configuration | Time |
|---|---|---|
| 50M-element f64 arithmetic plus sum | 1 → 12 threads | 1.78 s → 0.59 s |
| `¨` over a costly pure function | 1 → 12 threads | 7.1 s → 2.1 s |

### Gaps that remain
* **Array kernels are not fused yet** (M3 step 7): each operation in a chain like `xs × xs + 1 ▷ +/` still makes a full pass over memory.
* **Elements are boxed on the way in and out of arrays.** Indexing and `¨` callbacks box and unbox each element.
* **Net-only overhead:** with 1 thread, nets cost about 16% more than plain native code.
