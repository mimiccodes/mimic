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
   * **Rust + Rayon, all cores**: the hand-parallelised Rust version (`bench/<name>/rayon/`, the same algorithm with Rayon's parallel iterators, `join` or `par_sort_unstable`), next to Mimic on all cores. It shows what parallel code written by hand gets, against Mimic's parallelism without annotations.
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
| 9 | `json` (encode and decode 300,000 records, 21 MB) | strings, numbers, records | `json::encode`, `json::decode[T]` |
| 10 | `http` (hello-world server, 400,000 requests) | I/O, concurrency | `http::serve`, event loop, keep-alive |
| 11 | `ui` (10k-row list, 20 frames of view + diff) | building and comparing screens | `view`, `ui::diff`, `¨` |
| 12 | `records` (1M records updated 5 times) | records, immutability | `with`, `¨` |
| 13 | `scale` (30 1080p frames resized to 4 resolutions) | bytes, integer arithmetic, index math | `[u8]`, fused `¨`, `bytes::from` |
| 14 | `filters` (a 1080p image through 5 filters) | stencils, small sorts | fused `¨`, lists in registers, `sort` |
| 15 | `qoi` (encode and decode a 1024×768 QOI image, 5 times) | sequential state machine, byte output | `settle`, records, `push`/`concat`/`put` in place |
| 16 | `paths` (Dijkstra and A* on a 600×600 grid) | a binary heap, branches | `settle`, records, `put` in place |

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

### On CI machines, with Rust + Rayon (byte and pixel round, 2026-11-17)

The bench workflow (run 43929837181, commit 3da7f96), after the round prompted by the new `scale` benchmark and the transcoding example: byte data read and written directly, heavy loop elements shared over the cores, element functions inlined into fused loops, a plain divide for operands that are not negative, tuples of numbers in registers. Ratios to C; Rust + Rayon and Mimic, all cores, are the parallel columns.

**macOS (arm64)**

| Benchmark | C | Rust | Go | Mimic, 1 core | Rust + Rayon, all cores | Mimic, all cores |
|---|---|---|---|---|---|---|
| fib | 0.133 s | 0.155 s (1.16×) | 0.173 s (1.30×) | 0.137 s (1.03×) | 0.049 s (0.37×) | 0.048 s (0.36×) |
| http | 0.303 s | 0.371 s (1.22×) | 3.420 s (11.29×) | 0.559 s (1.84×) | — | 0.451 s (1.49×) |
| json | 0.333 s | 0.209 s (0.63×) | 0.520 s (1.56×) | 0.287 s (0.86×) | — | 0.264 s (0.79×) |
| mandelbrot | 0.847 s | 0.806 s (0.95×) | 0.813 s (0.96×) | 0.508 s (0.60×) | 0.272 s (0.32×) | 0.187 s (0.22×) |
| matmul | 0.255 s | 0.273 s (1.07×) | 0.650 s (2.54×) | 0.039 s (0.15×) | 0.306 s (1.20×) | 0.037 s (0.15×) |
| nbody | 0.043 s | 0.058 s (1.34×) | 0.110 s (2.52×) | 0.052 s (1.19×) | — | 0.047 s (1.07×) |
| prefix_sum | 0.159 s | 0.277 s (1.74×) | 0.153 s (0.96×) | 0.131 s (0.83×) | 0.106 s (0.67×) | 0.078 s (0.49×) |
| records | 0.014 s | 0.039 s (2.82×) | 0.056 s (4.05×) | 0.036 s (2.56×) | 0.037 s (2.68×) | 0.023 s (1.63×) |
| scale | 0.171 s | 0.292 s (1.70×) | 0.427 s (2.49×) | 0.546 s (3.19×) | 0.172 s (1.01×) | 0.154 s (0.90×) |
| sort | 1.210 s | 0.259 s (0.21×) | 0.927 s (0.77×) | 0.161 s (0.13×) | 0.154 s (0.13×) | 0.112 s (0.09×) |
| sum_of_squares | 0.039 s | 0.049 s (1.26×) | 0.079 s (2.05×) | 0.032 s (0.82×) | 0.029 s (0.76×) | 0.013 s (0.33×) |
| tree | 0.345 s | 0.441 s (1.28×) | 0.589 s (1.71×) | 0.266 s (0.77×) | 0.321 s (0.93×) | 0.243 s (0.70×) |

**Linux (x86-64)**

| Benchmark | C | Rust | Go | Mimic, 1 core | Rust + Rayon, all cores | Mimic, all cores |
|---|---|---|---|---|---|---|
| fib | 0.113 s | 0.121 s (1.07×) | 0.229 s (2.03×) | 0.101 s (0.89×) | 0.048 s (0.43×) | 0.047 s (0.41×) |
| http | 0.298 s | 0.687 s (2.31×) | 7.937 s (26.67×) | 0.513 s (1.72×) | — | 0.454 s (1.52×) |
| json | 0.328 s | 0.263 s (0.80×) | 0.654 s (2.00×) | 0.313 s (0.96×) | — | 0.291 s (0.89×) |
| mandelbrot | 0.577 s | 0.655 s (1.13×) | 0.662 s (1.15×) | 0.446 s (0.77×) | 0.181 s (0.31×) | 0.184 s (0.32×) |
| matmul | 0.201 s | 0.198 s (0.99×) | 0.715 s (3.56×) | 0.077 s (0.38×) | 0.409 s (2.04×) | 0.044 s (0.22×) |
| nbody | 0.100 s | 0.111 s (1.11×) | 0.164 s (1.64×) | 0.066 s (0.66×) | — | 0.066 s (0.66×) |
| prefix_sum | 0.439 s | 0.488 s (1.11×) | 0.486 s (1.11×) | 0.443 s (1.01×) | 0.259 s (0.59×) | 0.247 s (0.56×) |
| records | 0.027 s | 0.083 s (3.09×) | 0.077 s (2.88×) | 0.084 s (3.15×) | 0.054 s (2.02×) | 0.049 s (1.85×) |
| scale | 0.212 s | 0.492 s (2.33×) | 0.620 s (2.93×) | 0.744 s (3.52×) | 0.349 s (1.65×) | 0.334 s (1.58×) |
| sort | 0.900 s | 0.231 s (0.26×) | 0.922 s (1.02×) | 0.212 s (0.24×) | 0.142 s (0.16×) | 0.133 s (0.15×) |
| sum_of_squares | 0.031 s | 0.058 s (1.86×) | 0.095 s (3.06×) | 0.031 s (1.01×) | 0.075 s (2.42×) | 0.016 s (0.51×) |
| tree | 0.528 s | 0.618 s (1.17×) | 0.658 s (1.25×) | 0.342 s (0.65×) | 0.446 s (0.84×) | 0.170 s (0.32×) |

**Windows (x86-64)** (no C version of the server: http has no ratio)

| Benchmark | C | Rust | Go | Mimic, 1 core | Rust + Rayon, all cores | Mimic, all cores |
|---|---|---|---|---|---|---|
| fib | 0.129 s | 0.112 s (0.87×) | 0.228 s (1.76×) | 0.114 s (0.88×) | 0.053 s (0.41×) | 0.058 s (0.45×) |
| http | — | 0.749 s | 6.485 s | 0.587 s | — | 0.667 s |
| json | 0.571 s | 0.338 s (0.59×) | 0.628 s (1.10×) | 0.389 s (0.68×) | — | 0.345 s (0.60×) |
| mandelbrot | 0.646 s | 0.684 s (1.06×) | 0.707 s (1.09×) | 0.220 s (0.34×) | 0.202 s (0.31×) | 0.082 s (0.13×) |
| matmul | 0.306 s | 0.300 s (0.98×) | 0.784 s (2.57×) | 0.105 s (0.34×) | 0.507 s (1.66×) | 0.064 s (0.21×) |
| nbody | 0.092 s | 0.101 s (1.09×) | 0.186 s (2.01×) | 0.076 s (0.82×) | — | 0.076 s (0.82×) |
| prefix_sum | 0.326 s | 0.743 s (2.28×) | 0.313 s (0.96×) | 0.337 s (1.03×) | 0.191 s (0.59×) | 0.204 s (0.63×) |
| records | 0.033 s | 0.099 s (3.03×) | 0.070 s (2.15×) | 0.114 s (3.49×) | 0.061 s (1.85×) | 0.076 s (2.33×) |
| scale | 0.506 s | 0.580 s (1.14×) | 0.742 s (1.47×) | 0.760 s (1.50×) | 0.372 s (0.74×) | 0.391 s (0.77×) |
| sort | 1.874 s | 0.256 s (0.14×) | 1.060 s (0.57×) | 0.505 s (0.27×) | 0.157 s (0.08×) | 0.279 s (0.15×) |
| sum_of_squares | 0.039 s | 0.071 s (1.83×) | 0.108 s (2.80×) | 0.045 s (1.16×) | 0.048 s (1.24×) | 0.028 s (0.73×) |
| tree | 1.214 s | 1.369 s (1.13×) | 0.682 s (0.56×) | 0.429 s (0.35×) | 0.817 s (0.67×) | 0.214 s (0.18×) |

Where Mimic is behind C: `scale` on one core (1.5× on Windows to 3.5× on Linux; C knows the four sizes at compile time and turns every division into a multiplication, and Mimic checks bounds and rounds `div` downwards; on all cores Mimic is ahead of Rayon on macOS and Windows), `records` (one heap block per record: 1.6× to 2.3× C on all cores), and `http` on one core (1.7× to 1.8×). The machines change from run to run (an AMD EPYC 7763, 9V45 or 9V74 for Linux and Windows), and so do C's times: compare within a table.

### On CI machines, with Rust + Rayon (after the speed round, 2026-11-16)

The bench workflow (run 43823317121, commit 33f5e2d) on GitHub's shared runners: compare the ratios, not the times, and expect a few percent of noise between runs. The ratio is to C; Rust + Rayon and Mimic, all cores, are the parallel columns.

**macOS arm64** (3 cores)

| Benchmark | C | Rust | Go | Mimic, 1 core | Rust + Rayon, all cores | Mimic, all cores |
|---|---|---|---|---|---|---|
| fib | 0.124 s | 0.132 s (1.07×) | 0.146 s (1.18×) | 0.132 s (1.07×) | 0.049 s (0.40×) | 0.046 s (0.37×) |
| http | 0.157 s | 0.285 s (1.81×) | 2.493 s (15.86×) | 0.396 s (2.52×) | — | 0.385 s (2.45×) |
| json | 0.239 s | 0.143 s (0.60×) | 0.381 s (1.59×) | 0.222 s (0.93×) | — | 0.184 s (0.77×) |
| mandelbrot | 0.678 s | 0.709 s (1.04×) | 0.730 s (1.08×) | 0.462 s (0.68×) | 0.252 s (0.37×) | 0.160 s (0.24×) |
| matmul | 0.206 s | 0.212 s (1.03×) | 0.585 s (2.83×) | 0.032 s (0.15×) | 0.274 s (1.33×) | 0.030 s (0.15×) |
| nbody | 0.040 s | 0.052 s (1.28×) | 0.091 s (2.26×) | 0.045 s (1.12×) | — | 0.041 s (1.02×) |
| prefix_sum | 0.113 s | 0.237 s (2.09×) | 0.140 s (1.24×) | 0.130 s (1.15×) | 0.085 s (0.75×) | 0.073 s (0.64×) |
| records | 0.013 s | 0.036 s (2.84×) | 0.037 s (2.91×) | 0.025 s (1.96×) | 0.027 s (2.12×) | 0.020 s (1.55×) |
| sort | 1.044 s | 0.161 s (0.15×) | 0.782 s (0.75×) | 0.120 s (0.11×) | 0.151 s (0.14×) | 0.078 s (0.07×) |
| sum_of_squares | 0.028 s | 0.042 s (1.51×) | 0.077 s (2.75×) | 0.028 s (1.00×) | 0.028 s (1.00×) | 0.014 s (0.50×) |
| tree | 0.320 s | 0.355 s (1.11×) | 0.388 s (1.22×) | 0.204 s (0.64×) | 0.200 s (0.63×) | 0.103 s (0.32×) |
| ui | — | — | — | 0.085 s | — | 0.044 s |

**Linux x86_64** (AMD EPYC, 4 logical CPUs: 2 cores × 2 threads)

| Benchmark | C | Rust | Go | Mimic, 1 core | Rust + Rayon, all cores | Mimic, all cores |
|---|---|---|---|---|---|---|
| fib | 0.076 s | 0.077 s (1.00×) | 0.136 s (1.78×) | 0.077 s (1.01×) | 0.031 s (0.40×) | 0.032 s (0.42×) |
| http | 0.188 s | 0.310 s (1.65×) | 3.784 s (20.09×) | 0.260 s (1.38×) | — | 0.246 s (1.30×) |
| json | 0.177 s | 0.157 s (0.89×) | 0.355 s (2.00×) | 0.175 s (0.99×) | — | 0.165 s (0.93×) |
| mandelbrot | 0.411 s | 0.422 s (1.03×) | 0.432 s (1.05×) | 0.141 s (0.34×) | 0.124 s (0.30×) | 0.043 s (0.11×) |
| matmul | 0.099 s | 0.097 s (0.98×) | 0.324 s (3.27×) | 0.052 s (0.53×) | 0.247 s (2.49×) | 0.030 s (0.30×) |
| nbody | 0.062 s | 0.071 s (1.14×) | 0.117 s (1.88×) | 0.036 s (0.58×) | — | 0.036 s (0.58×) |
| prefix_sum | 0.320 s | 0.352 s (1.10×) | 0.340 s (1.06×) | 0.313 s (0.98×) | 0.181 s (0.57×) | 0.165 s (0.52×) |
| records | 0.019 s | 0.075 s (4.02×) | 0.050 s (2.67×) | 0.060 s (3.22×) | 0.047 s (2.53×) | 0.035 s (1.89×) |
| sort | 0.421 s | 0.145 s (0.34×) | 0.745 s (1.77×) | 0.147 s (0.35×) | 0.095 s (0.22×) | 0.106 s (0.25×) |
| sum_of_squares | 0.009 s | 0.016 s (1.83×) | 0.056 s (6.33×) | 0.009 s (0.99×) | 0.009 s (1.04×) | 0.005 s (0.55×) |
| tree | 0.305 s | 0.404 s (1.32×) | 0.402 s (1.32×) | 0.227 s (0.74×) | 0.337 s (1.11×) | 0.155 s (0.51×) |
| ui | — | — | — | 0.072 s | — | 0.048 s |

**Windows x86_64** (4 logical CPUs)

| Benchmark | C | Rust | Go | Mimic, 1 core | Rust + Rayon, all cores | Mimic, all cores |
|---|---|---|---|---|---|---|
| fib | 0.140 s | 0.123 s (0.88—) | 0.259 s (1.85—) | 0.128 s (0.91—) | 0.054 s (0.39—) | 0.058 s (0.41—) |
| http | — | 0.731 s | 8.013 s | 0.616 s | — | 0.652 s |
| json | 0.594 s | 0.375 s (0.63—) | 0.541 s (0.91—) | 0.330 s (0.56—) | — | 0.296 s (0.50—) |
| mandelbrot | 0.640 s | 0.743 s (1.16—) | 0.720 s (1.13—) | 0.418 s (0.65—) | 0.204 s (0.32—) | 0.182 s (0.29—) |
| matmul | 0.137 s | 0.148 s (1.08—) | 0.797 s (5.83—) | 0.093 s (0.68—) | 0.456 s (3.34—) | 0.055 s (0.41—) |
| nbody | 0.119 s | 0.127 s (1.06—) | 0.185 s (1.55—) | 0.078 s (0.66—) | — | 0.081 s (0.68—) |
| prefix_sum | 0.228 s | 0.483 s (2.12—) | 0.237 s (1.04—) | 0.231 s (1.01—) | 0.173 s (0.76—) | 0.184 s (0.81—) |
| records | 0.021 s | 0.087 s (4.18—) | 0.078 s (3.76—) | 0.101 s (4.88—) | 0.058 s (2.83—) | 0.069 s (3.36—) |
| sort | 1.684 s | 0.233 s (0.14—) | 1.017 s (0.60—) | 0.155 s (0.09—) | 0.145 s (0.09—) | 0.124 s (0.07—) |
| sum_of_squares | 0.036 s | 0.071 s (1.94—) | 0.115 s (3.17—) | 0.042 s (1.15—) | 0.092 s (2.54—) | 0.025 s (0.69—) |
| tree | 1.129 s | 1.291 s (1.14—) | 0.662 s (0.59—) | 0.345 s (0.31—) | 0.786 s (0.70—) | 0.192 s (0.17—) |
| ui | — | — | — | 0.130 s | — | 0.088 s |

Since v0.3.13 (the section below): the HTTP server waits on no lock (Linux 1.71× → 1.30× C); prefix sums give each thread one contiguous run of chunks; parallel `fib` runs as fork-join with plain sequential leaves (Linux 0.55× → 0.42× C, Windows 0.61× → 0.41×). Mimic, all cores, against Rayon: ahead or even on every benchmark on macOS; on Linux ahead or even on all but sort (0.106 s against 0.095 s); on Windows ahead on mandelbrot, matmul, sort, sum of squares and tree, and behind on fib (0.058 / 0.054 s), prefix sum (0.184 / 0.173 s) and records (0.069 / 0.058 s, and 4.9× C on one core).

### On CI machines, with Rust + Rayon (v0.3.13, 2026-11-16)

The bench workflow on GitHub's shared runners (3 or 4 cores, other jobs beside them: compare the ratios, not the times). The ratio is to C; Rust + Rayon and Mimic, all cores, are the parallel columns.

**macOS arm64**: Apple M1 (Virtual), Darwin 24.6.0, 3 cores, load average 14.9 at the end.

| Benchmark | C | Rust | Go | Mimic, 1 core | Rust + Rayon, all cores | Mimic, all cores |
|---|---|---|---|---|---|---|
| fib | 0.140 s | 0.159 s (1.13×) | 0.192 s (1.37×) | 0.138 s (0.99×) | 0.065 s (0.46×) | 0.063 s (0.45×) |
| http | 0.286 s | 0.445 s (1.55×) | 3.579 s (12.51×) | 0.561 s (1.96×) | — | 0.554 s (1.94×) |
| json | 0.297 s | 0.162 s (0.54×) | 0.469 s (1.58×) | 0.276 s (0.93×) | — | 0.224 s (0.76×) |
| mandelbrot | 0.790 s | 0.833 s (1.05×) | 0.848 s (1.07×) | 0.568 s (0.72×) | 0.321 s (0.41×) | 0.199 s (0.25×) |
| matmul | 0.286 s | 0.288 s (1.00×) | 0.813 s (2.84×) | 0.050 s (0.17×) | 0.410 s (1.43×) | 0.048 s (0.17×) |
| nbody | 0.049 s | 0.059 s (1.22×) | 0.113 s (2.33×) | 0.049 s (1.00×) | — | 0.056 s (1.14×) |
| prefix_sum | 0.188 s | 0.311 s (1.65×) | 0.161 s (0.85×) | 0.156 s (0.83×) | 0.123 s (0.65×) | 0.106 s (0.57×) |
| records | 0.015 s | 0.047 s (3.10×) | 0.049 s (3.23×) | 0.034 s (2.23×) | 0.043 s (2.78×) | 0.023 s (1.53×) |
| sort | 1.246 s | 0.209 s (0.17×) | 0.969 s (0.78×) | 0.166 s (0.13×) | 0.181 s (0.15×) | 0.102 s (0.08×) |
| sum_of_squares | 0.031 s | 0.045 s (1.48×) | 0.098 s (3.19×) | 0.032 s (1.05×) | 0.038 s (1.24×) | 0.018 s (0.60×) |
| tree | 0.413 s | 0.511 s (1.24×) | 0.511 s (1.24×) | 0.258 s (0.62×) | 0.350 s (0.85×) | 0.106 s (0.26×) |
| ui | — | — | — | 0.119 s | — | 0.114 s |

**Linux x86_64**: AMD EPYC 7763 64-Core Processor, Linux 7.0.0-1017-azure, 4 cores, load average 1.7 at the end.

| Benchmark | C | Rust | Go | Mimic, 1 core | Rust + Rayon, all cores | Mimic, all cores |
|---|---|---|---|---|---|---|
| fib | 0.113 s | 0.121 s (1.07×) | 0.229 s (2.02×) | 0.131 s (1.16×) | 0.048 s (0.43×) | 0.062 s (0.55×) |
| http | 0.302 s | 0.732 s (2.43×) | 8.559 s (28.38×) | 0.585 s (1.94×) | — | 0.515 s (1.71×) |
| json | 0.336 s | 0.273 s (0.81×) | 0.654 s (1.95×) | 0.318 s (0.95×) | — | 0.291 s (0.86×) |
| mandelbrot | 0.578 s | 0.655 s (1.13×) | 0.661 s (1.14×) | 0.446 s (0.77×) | 0.180 s (0.31×) | 0.184 s (0.32×) |
| matmul | 0.203 s | 0.203 s (1.00×) | 0.715 s (3.52×) | 0.078 s (0.39×) | 0.411 s (2.02×) | 0.045 s (0.22×) |
| nbody | 0.100 s | 0.111 s (1.11×) | 0.164 s (1.63×) | 0.065 s (0.65×) | — | 0.065 s (0.65×) |
| prefix_sum | 0.456 s | 0.505 s (1.11×) | 0.506 s (1.11×) | 0.457 s (1.00×) | 0.262 s (0.57×) | 0.253 s (0.56×) |
| records | 0.029 s | 0.087 s (3.00×) | 0.081 s (2.78×) | 0.085 s (2.93×) | 0.054 s (1.87×) | 0.051 s (1.74×) |
| sort | 0.907 s | 0.236 s (0.26×) | 0.929 s (1.02×) | 0.224 s (0.25×) | 0.142 s (0.16×) | 0.142 s (0.16×) |
| sum_of_squares | 0.031 s | 0.058 s (1.86×) | 0.098 s (3.12×) | 0.032 s (1.01×) | 0.076 s (2.42×) | 0.016 s (0.51×) |
| tree | 0.528 s | 0.633 s (1.20×) | 0.691 s (1.31×) | 0.346 s (0.66×) | 0.491 s (0.93×) | 0.175 s (0.33×) |
| ui | — | — | — | 0.105 s | — | 0.069 s |

**Windows x86_64**: Intel64 Family 6 Model 207 Stepping 2, GenuineIntel, Windows 2025Server, 4 cores, load average 0.0 at the end.

| Benchmark | C | Rust | Go | Mimic, 1 core | Rust + Rayon, all cores | Mimic, all cores |
|---|---|---|---|---|---|---|
| fib | 0.109 s | 0.094 s (0.86×) | 0.191 s (1.76×) | 0.135 s (1.24×) | 0.043 s (0.40×) | 0.066 s (0.61×) |
| http | — | 0.550 s | 5.135 s | 0.477 s | — | 0.554 s |
| json | 0.480 s | 0.285 s (0.59×) | 0.520 s (1.08×) | 0.329 s (0.69×) | — | 0.301 s (0.63×) |
| mandelbrot | 0.541 s | 0.573 s (1.06×) | 0.591 s (1.09×) | 0.183 s (0.34×) | 0.168 s (0.31×) | 0.068 s (0.13×) |
| matmul | 0.264 s | 0.269 s (1.02×) | 0.653 s (2.47×) | 0.083 s (0.31×) | 0.421 s (1.59×) | 0.053 s (0.20×) |
| nbody | 0.076 s | 0.083 s (1.09×) | 0.156 s (2.04×) | 0.062 s (0.82×) | — | 0.062 s (0.81×) |
| prefix_sum | 0.288 s | 0.669 s (2.32×) | 0.269 s (0.93×) | 0.300 s (1.04×) | 0.161 s (0.56×) | 0.182 s (0.63×) |
| records | 0.025 s | 0.084 s (3.32×) | 0.058 s (2.28×) | 0.096 s (3.79×) | 0.050 s (1.98×) | 0.065 s (2.56×) |
| sort | 1.616 s | 0.220 s (0.14×) | 0.912 s (0.56×) | 0.414 s (0.26×) | 0.136 s (0.08×) | 0.243 s (0.15×) |
| sum_of_squares | 0.032 s | 0.060 s (1.86×) | 0.092 s (2.86×) | 0.037 s (1.14×) | 0.040 s (1.25×) | 0.023 s (0.72×) |
| tree | 1.049 s | 1.180 s (1.12×) | 0.584 s (0.56×) | 0.372 s (0.35×) | 0.682 s (0.65×) | 0.186 s (0.18×) |
| ui | — | — | — | 0.125 s | — | 0.084 s |

Mimic is ahead of Rayon on all eight benchmarks on macOS, on five of eight on Linux (fib behind, mandelbrot and sort even), and on four on Windows, where fib, prefix sum, records and sort are behind, and records on one core is 3.8× C (2.2–2.9× elsewhere). The HTTP server is 1.7–1.9× C everywhere: under load about 30% of its busy time waits on the event loop's lock ([gaps.md](docs/gaps.md)).

### Against Rust + Rayon (2026-11-16)

Apple M3 Pro, 12 cores, macOS, load average 11–15 (other work running). Best of 5 runs; the ratio is to C. Rust + Rayon and Mimic, all cores, are the two parallel columns; nbody (five bodies, one step after another) has no Rayon version.

| Benchmark | C | Rust | Go | Mimic, 1 core | Rust + Rayon, all cores | Mimic, all cores |
|---|---|---|---|---|---|---|
| fib(38) | 0.512 s | 1.11× | 1.17× | 1.02× | **0.16×** | 0.20× |
| mandelbrot | 2.406 s | 1.15× | 1.22× | 0.73× | 0.15× | **0.11×** |
| matmul 1024² | 0.778 s | 1.00× | 2.42× | 0.12× | 0.60× | **0.11×** |
| nbody, 1M steps | 0.173 s | 1.26× | 1.53× | 1.00× | — | 1.00× |
| prefix sum, 100M | 0.420 s | 2.44× | 1.04× | 0.97× | **0.37×** | 0.63× |
| records | 0.041 s | 3.30× | 3.51× | 2.46× | 2.13× | **1.14×** |
| sort 10M integers | 4.371 s | 0.15× | 0.74× | 0.09× | 0.06× | 0.06× |
| sum of squares, 100M | 0.120 s | 1.63× | 2.48× | 1.00× | 0.49× | **0.24×** |
| tree | 1.395 s | 1.14× | 1.26× | 0.57× | 0.52× | **0.13×** |

Mimic with no annotations is faster than hand-written Rayon on six of the eight, and ties on sort. Rayon wins two:
* **prefix sum (0.37× against 0.63×).** The Rayon version writes the running sums in place into one buffer, in chunks of a million; Mimic's two-pass scan makes the first pass's sums and then a second array.
* **fib (0.16× against 0.20×).** `rayon::join` steals work with less overhead per split than Mimic's fork-join, which also guards each native call's depth.

Matmul with Rayon is the plain loop split by rows (0.60×); Mimic's kernel is blocked and SIMD (0.11×) on one core already.

### Linux and Windows in CI (2026-11-14)

The bench workflow on GitHub's 4-core runners, commit 240d7d8, best of 5
runs; the ratio is to C (below 1.0 is faster than C). The two runners have
different CPUs, so compare each row with its own C, not across systems.

**Linux x86_64** (AMD EPYC 9V74, Zen 4 with AVX-512):

| Benchmark | C | Rust | Go | Mimic, 1 core | Mimic, all cores |
|---|---|---|---|---|---|
| fib | 0.091 s | 1.15× | 2.16× | 1.24× | **0.58×** |
| http | 0.224 s | 2.24× | 22.62× | 1.77× | 1.60× |
| json | 0.240 s | 0.89× | 1.90× | 1.04× | **0.95×** |
| mandelbrot | 0.490 s | 1.16× | 1.12× | **0.31×** | **0.12×** |
| matmul | 0.160 s | 1.00× | 3.88× | **0.39×** | **0.23×** |
| nbody | 0.088 s | 1.06× | 1.57× | **0.60×** | **0.60×** |
| prefix_sum | 0.364 s | 1.08× | 1.07× | 1.00× | **0.61×** |
| records | 0.021 s | 4.32× | 2.93× | 3.95× | 2.14× |
| sort | 0.705 s | 0.27× | 1.14× | **0.25×** | **0.16×** |
| sum_of_squares | 0.014 s | 2.66× | 6.03× | 1.01× | **0.56×** |
| tree | 0.468 s | 1.36× | 1.03× | **0.63×** | **0.45×** |
| ui | — | — | — | 0.084 s | 0.056 s |

**Windows x86_64** (AMD Zen 3, AVX2, no AVX-512; Windows Server 2025):

| Benchmark | C | Rust | Go | Mimic, 1 core | Mimic, all cores |
|---|---|---|---|---|---|
| fib | 0.127 s | 0.94× | 1.86× | 1.18× | 1.37× |
| http | — | 0.962 s | 12.236 s | 0.761 s | 0.819 s |
| json | 0.593 s | 0.59× | 1.02× | **0.56×** | **0.50×** |
| mandelbrot | 0.585 s | 1.13× | 1.15× | 1.07× | **0.50×** |
| matmul | 0.160 s | 0.88× | 4.48× | **0.51×** | **0.33×** |
| nbody | 0.106 s | 1.10× | 1.62× | **0.86×** | **0.86×** |
| prefix_sum | 0.188 s | 2.21× | 1.16× | 1.05× | 1.42× |
| records | 0.022 s | 3.91× | 3.03× | 3.72× | 2.37× |
| sort | 1.803 s | 0.12× | 0.50× | **0.08×** | **0.06×** |
| sum_of_squares | 0.035 s | 1.79× | 2.96× | 2.52× | 1.36× |
| tree | 1.064 s | 1.13× | 0.66× | **0.33×** | **0.22×** |
| ui | — | — | — | 0.124 s | 0.082 s |

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

| Benchmark | C | Rust | Go | Mimic, 1 core | Mimic, all cores |
|---|---|---|---|---|---|
| fib | 0.080 s | 1.09× | 1.85× | 1.35× | **0.67×** |
| http | — | 0.442 s | 4.547 s | 0.341 s | 0.318 s |
| json | 0.419 s | 0.67× | 0.80× | **0.52×** | **0.47×** |
| mandelbrot | 0.437 s | 0.99× | 1.03× | **0.33×** | **0.13×** |
| matmul | 0.090 s | 1.06× | 4.13× | **0.69×** | **0.42×** |
| nbody | 0.069 s | 1.13× | 1.82× | **0.70×** | **0.70×** |
| prefix_sum | 0.183 s | 2.09× | 0.92× | 1.05× | **0.79×** |
| records | 0.021 s | 3.95× | 2.11× | 3.47× | 2.53× |
| sort | 1.225 s | 0.14× | 0.65× | **0.10×** | **0.09×** |
| sum_of_squares | 0.014 s | 1.59× | 4.65× | 1.32× | 1.09× |
| tree | 0.866 s | 1.15× | 0.52× | **0.28×** | **0.21×** |
| ui | — | — | — | 0.106 s | 0.068 s |

### All benchmarks (after F, 2026-11-13 evening)

Apple M3 Pro, 12 cores, `bench/run.py`, load average 5 to 7 from other
programs; the ratio is to C.

| Benchmark | C | Rust | Go | Mimic, 1 core | Mimic, all cores |
|---|---|---|---|---|---|
| fib | 0.537 s | 1.07× | 1.15× | 1.01× | **0.15×** |
| json | 1.048 s | 0.59× | 1.75× | **0.85×** | **0.68×** |
| mandelbrot | 2.727 s | 1.16× | 1.23× | **0.71×** | **0.10×** |
| nbody | 0.174 s | 1.26× | 1.45× | 1.00× | 1.00× |
| prefix_sum | 0.429 s | 2.44× | 1.04× | **0.97×** | **0.66×** |
| records | 0.042 s | 3.40× | 3.38× | 2.24× | **1.00×** |
| sum_of_squares | 0.124 s | 1.70× | 2.58× | 1.00× | **0.24×** |
| tree | 1.444 s | 1.16× | 1.36× | **0.57×** | **0.12×** |

mandelbrot's one-core time is with SIMD across elements (8 points at once,
see design.md); C, Rust and Go do one point at a time.

All of these are f64 or i64 programs, so the GPU takes no part (Apple
GPUs have no f64). An f32 Mandelbrot of 4M points: 0.30 s on all CPU
cores, 0.15 s with the GPU (kernel 18 ms).

### All benchmarks (after F3)

Apple M3 Pro, 12 cores, 2026-11-13, best of 5 runs; the ratio is to C.
Other programs kept the load average near 10, so all-core times are on the
slow side.

| Benchmark | C | Rust | Go | Mimic, 1 core | Mimic, all cores |
|---|---|---|---|---|---|
| fib | 0.567 s | 1.08× | 1.17× | 1.03× | **0.19×** |
| http | 0.706 s | 2.16× | 19.38× | 1.88× | 1.27× |
| json | 1.134 s | 0.60× | 1.76× | **0.97×** | **0.75×** |
| mandelbrot | 2.639 s | 1.16× | 1.22× | 1.10× | **0.13×** |
| matmul | 0.895 s | 1.01× | 2.42× | **0.09×** | **0.08×** |
| nbody | 0.192 s | 1.31× | 1.57× | 1.03× | 1.04× |
| prefix_sum | 0.475 s | 2.32× | 1.06× | **0.99×** | **0.63×** |
| records | 0.044 s | 3.41× | 3.77× | 7.43× | 3.50× |
| sort | 4.894 s | 0.15× | 0.72× | **0.07×** | **0.03×** |
| sum_of_squares | 0.134 s | 1.60× | 2.52× | 1.01× | **0.23×** |
| tree | 1.566 s | 1.15× | 1.28× | **0.60×** | **0.14×** |

(http's C is a floor that never parses requests; json's C a hand-written
parser for one record shape. records measured 5.9× on its own run.)

### records: reuse in place (27× → 5.9× C)

A million records `{id, name, x, v}` (a string and numbers), updated five
times with `{ p with x: …, v: … }` over the whole list, then summed. C,
Rust and Go update their arrays in place; Mimic's values are immutable,
so in principle each step builds a new list of new records. Apple M3 Pro,
2026-11-13.

| Benchmark | C | Rust | Go | Mimic, 1 core | Mimic, all cores |
|---|---|---|---|---|---|
| records | 0.046 s | 3.33× | 3.59× | **5.89×** | **2.53×** |

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
place, the list's elements handed over). Rust (3.3×) and Go (3.4×) are
close; what is left is making a million separate records (C has one
array of structs).

Making the records is now one fused parallel pass too, and in release
builds the records `main` holds at the end are not freed one by one (the
process exits next; debug builds still free all and check for leaks):
that freeing was two thirds of the time. All cores now match C (0.042 s);
one core is 2.2× C, the cost of separate records with headers and
reference counts against one array of structs.

### http: a hello-world server (faster than hyper on one core)

Each version is a server answering `Hello, World!`; a Go load generator
(`bench/http/load`) sends 400,000 requests over 64 keep-alive connections,
16 pipelined per write, and checks every response. The time is the load
generator's; it runs on the same machine. Apple M3 Pro, 2026-11-13.

| Benchmark | C | Rust (hyper) | Go (net/http) | Mimic, 1 core | Mimic, all cores |
|---|---|---|---|---|---|
| http | 0.712 s | 1.523 s (2.14×) | 13.952 s (19.60×) | **1.336 s (1.88×)** | **0.962 s (1.35×)** |

The C version is a floor rather than an HTTP server: one kqueue loop that
finds the end of each request and copies a fixed answer, without reading
the request line or headers. Against real servers, Mimic on one core
(about 300,000 requests a second) is ahead of hyper (260,000) and far
ahead of Go's net/http, which flushes every pipelined answer on its own.

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
serde + serde_json, Go encoding/json. Apple M3 Pro, 2026-11-13.

| Benchmark | C | Rust | Go | Mimic, 1 core | Mimic, all cores |
|---|---|---|---|---|---|
| json | 1.238 s | 0.59× | 1.77× | **1.10×** | **0.79×** |

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
each of 20 frames ticks one row. There is no C, Rust or Go counterpart
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

| Benchmark | C | Rust | Go | Mimic, 1 core | Mimic, all cores |
|---|---|---|---|---|---|
| fib | 0.590 s | 1.04× | 1.15× | 1.02× | **0.18×** |
| mandelbrot | 2.968 s | 1.02× | 1.07× | 1.01× | **0.11×** |
| matmul | 0.871 s | 1.14× | 2.69× | 0.09× | **0.08×** |
| nbody | 0.209 s | 1.23× | 1.56× | 1.01× | 0.95× |
| prefix_sum | 0.552 s | 2.16× | 0.97× | 0.82× | **0.54×** |
| sort | 4.842 s | 0.15× | 0.74× | 0.07× | **0.03×** |
| sum_of_squares | 0.132 s | 1.62× | 2.63× | 1.04× | **0.23×** |
| tree | 1.578 s | 1.18× | 1.32× | **0.54×** | **0.15×** |

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

| Benchmark | C | Rust | Go | Mimic, 1 core | Mimic, all cores |
|---|---|---|---|---|---|
| tree | 1.414 s | 1.13× | 1.28× | **0.96×** | **0.74×** |

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
