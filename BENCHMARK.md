# Mimic benchmarks

Mimic aims to be the fastest programming language. This file defines what that claim means and how it gets measured, and it records where Mimic stands today, including where it is slow. Numbers here are only ever **measured**, never estimated.

## Rules

1. **Like for like.** Every benchmark exists in Mimic, C, Rust and Go. Each version uses the same algorithm, written idiomatically for that language, with no hand-written assembly. Every version prints the same result, and the runner checks it.
2. **Fastest build of each language:**

   | Language | Build command |
   |---|---|
   | Mimic | `mimic build` (release is the default) |
   | C | `mimic cc -O3 -mcpu=native` (the Clang inside `mimic`) |
   | Rust | `cargo build --release` with `-C target-cpu=native` |
   | Go | `go build` |

3. **Two scores per benchmark:**
   * **Single-core** (`MIMIC_THREADS=1`), which measures code quality.
   * **All cores**, the default, which measures what users actually get. The other languages run their straightforward version, which for C, Rust and Go is usually sequential. Mimic's parallelism is automatic, so it counts.
4. **Statistics:** each measurement is the best of 5 wall-clock runs, after one warm-up run. On macOS the first launch of a new binary is slowed by a security scan, and the warm-up absorbs it. The machine, OS, CPU model and load average are recorded with every table.
5. **Score:** each result is reported as a ratio to C (`time / C time`). Below 1.0 means faster than C.

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
| 9 | `json` (parse 100 MB) | strings, unions | records, `match`, strings |
| 10 | `http` (hello-world requests/s) | I/O, concurrency | concurrent I/O, event loop |
| 11 | `ui` (frame time, 10k-row list) | diffing, rendering | `view`, `each` |

## Building the suite, step by step

### Step 1: Harness ✅
* Create a `bench/` directory with one folder per benchmark, each holding `main.mim`, `main.c`, `main.rs`, `main.go` and an `expected.txt`.
* `bench/run.sh`:
  * builds every version;
  * checks each output against `expected.txt`;
  * runs the warm-up plus 5 timed runs, single-core and all cores;
  * writes `bench/results.md`.
* Start with benchmarks 1, 3, 5 and 7, which need no language features beyond today's.

**Done when:** `bench/run.sh` prints a table with C ratios on this machine. Done: the runner is `bench/run.py` (`python3 bench/run.py [names…] [--runs N]`), and it writes `bench/results.md`.

### Step 2: CI
* Run the suite on every merge to `main`, on fixed hardware: Linux x86-64 and macOS arm64.
* Fail CI when any benchmark slows down by more than 5% against the stored baseline.
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

### Single core within 10% of C on every benchmark

Apple M3 Pro, 12 cores, 2026-11-13. Best of 5 runs; the ratio is to C.

| Benchmark | C | Rust | Go | Mimic, 1 core | Mimic, all cores |
|---|---|---|---|---|---|
| fib(38) | 0.512 s | 1.13× | 1.16× | 1.27× | **0.19×** |
| mandelbrot | 2.407 s | 1.15× | 1.22× | 1.09× | **0.11×** |
| matmul 1024² | 0.779 s | 1.00× | 2.40× | **0.09×** | **0.08×** |
| nbody, 1M steps | 0.172 s | 1.27× | 1.48× | **1.02×** | **1.03×** |
| prefix sum, 100M | 0.410 s | 2.48× | 1.05× | **0.98×** | **0.68×** |
| sort 10M integers | 4.396 s | 0.14× | 0.74× | **0.06×** | **0.03×** |
| sum of squares, 100M | 0.119 s | 1.64× | 2.53× | **1.01×** | **0.21×** |

Mimic is now at least as fast as Rust and Go on every benchmark but fib,
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
  0.518 s; Rust 1.10×, Go 1.18×) and 0.15× on all cores. Native recursion
  guards its depth so very deep recursion moves to the interaction-net
  runtime instead of overflowing the stack; a function of plain numbers
  now checks only right before it calls itself and, past the limit,
  starts the whole call over on the net (it is pure, and holds nothing on
  the heap), so calls that end the recursion pay nothing.

### Small lists and records in registers (nbody 8.6× → 1.09× C)

Apple M3 Pro, 12 cores, 2026-11-13. Best of 5 runs; the ratio is to C.

| Benchmark | C | Rust | Go | Mimic, 1 core | Mimic, all cores |
|---|---|---|---|---|---|
| fib(38) | 0.527 s | 1.09× | 1.14× | 1.23× | **0.21×** |
| mandelbrot | 2.414 s | 1.15× | 1.22× | 1.18× | **0.12×** |
| matmul 1024² | 0.778 s | 1.03× | 2.41× | **0.09×** | **0.08×** |
| nbody, 1M steps | 0.172 s | 1.27× | 1.49× | **1.09×** | **1.09×** |
| prefix sum, 100M | 0.408 s | 2.49× | 1.07× | 1.45× | **0.69×** |
| sort 10M integers | 4.370 s | 0.15× | 0.74× | **0.08×** | **0.03×** |
| sum of squares, 100M | 0.122 s | 1.61× | 2.45× | 2.40× | **0.36×** |

nbody now runs as fast as C on one core and faster than Rust and Go, with
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

| Benchmark | C | Rust | Go | Mimic, 1 core | Mimic, all cores |
|---|---|---|---|---|---|
| fib(38) | 0.509 s | 1.08× | 1.18× | 1.90× | **0.35×** |
| mandelbrot | 2.475 s | 1.14× | 1.21× | 1.16× | **0.13×** |
| matmul 1024² | 0.794 s | 1.01× | 2.40× | **0.09×** | **0.08×** |
| nbody, 1M steps | 0.178 s | 1.23× | 1.45× | 8.98× | 8.56× |
| prefix sum, 100M | 0.425 s | 2.46× | 1.05× | 1.43× | **0.64×** |
| sort 10M integers | 4.485 s | 0.15× | 0.74× | **0.06×** | **0.03×** |
| sum of squares, 100M | 0.122 s | 1.63× | 2.47× | 2.48× | **0.38×** |

What changed:
* **Small loops run directly.** A fused loop that fits in one chunk calls its kernel directly, with its partial results on the stack. Larger ones still go to the thread pool.
* **A second free list** (blocks up to 64 bytes) recycles short vectors, small records and short strings per thread instead of calling `malloc` and `free`.
* **Fewer runtime calls.** Kernels read vector lengths, data pointers and the chunk size inline. Array reads in compiled code have a never-returning error path, so LLVM keeps array headers in registers.
* **nbody in Mimic** now computes each pair's `m / d³` once per step and uses it for x, y and z, as the C version does.

The rest of nbody's gap is the boxed small arrays and records that every step allocates.

### The suite with seven benchmarks

Same machine (Apple M3 Pro, 12 cores), load average 6–10, best of 5 after a warm-up. The ratio is to C:

| Benchmark | C | Rust | Go | Mimic, 1 core | Mimic, all cores |
|---|---|---|---|---|---|
| fib(38) | 0.525 s | 1.14× | 1.24× | 1.96× | **0.31×** |
| mandelbrot 2000², 200 steps | 2.386 s | 1.16× | 1.22× | 1.17× | **0.12×** |
| matmul 1024² | 0.780 s | 0.99× | 2.69× | **0.09×** | **0.09×** |
| nbody, 1M steps | 0.199 s | 1.07× | 1.33× | 16.6× | 16.9× |
| prefix sum, 100M | 0.443 s | 2.45× | 1.07× | 1.45× | **0.65×** |
| sort 10M integers | 4.706 s | 0.14× | 0.74× | **0.07×** | **0.03×** |
| sum of squares, 100M | 0.130 s | 1.63× | 2.48× | 2.40× | **0.37×** |

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

Apple M3 Pro (12 cores), macOS 26.6, load average 6–10 (other work running), best of 5 after a warm-up. The ratio is to C. C, Rust and Go run their straightforward sequential versions.

| Benchmark | C | Rust | Go | Mimic, 1 core | Mimic, all cores |
|---|---|---|---|---|---|
| fib(38) | 0.556 s | 1.11× | 1.15× | 1.85× | **0.23×** |
| mandelbrot 2000², 200 steps | 2.607 s | 1.14× | 1.21× | 1.16× | **0.12×** |
| sort 10M integers | 4.726 s | 0.15× | 0.74× | **0.07×** | **0.04×** |
| sum of squares, 100M | 0.126 s | 1.64× | 2.52× | 2.47× | **0.37×** |

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
