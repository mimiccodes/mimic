# Mimic benchmarks

Mimic aims to be the fastest programming language. This file defines what that claim means and how it gets measured, and it records where Mimic stands today, including where it is slow. Numbers here are only ever **measured**, never estimated.

## Rules

1. **Like for like.** Every benchmark exists in Mimic, C, Rust and Go. Each version uses the same algorithm, written idiomatically for that language, with no hand-written assembly. Every version prints the same result, and the runner checks it.
2. **Fastest build of each language:**

   | Language | Build command |
   |---|---|
   | Mimic | `mimic build` (release is the default) |
   | C | `zig cc -O3 -mcpu=native` |
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

### Step 1: Harness (now)
* Create a `bench/` directory with one folder per benchmark, each holding `main.mim`, `main.c`, `main.rs`, `main.go` and an `expected.txt`.
* `bench/run.sh`:
  * builds every version;
  * checks each output against `expected.txt`;
  * runs the warm-up plus 5 timed runs, single-core and all cores;
  * writes `bench/results.md`.
* Start with benchmarks 1, 3, 5 and 7, which need no language features beyond today's.

**Done when:** `bench/run.sh` prints a table with C ratios on this machine.

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

## Current baseline (measured)

These were measured while M2 was being built. The machine was an Apple M3 Pro (6 performance + 6 efficiency cores) under background load (load average 7–14). Times are best of 3.

| Workload | Configuration | Time | vs C |
|---|---|---|---|
| fib(32) | C `-O3 -mcpu=native` | 0.03 s | 1.0× |
| fib(32) | Mimic `--no-net`, 1 core | 1.59 s | **~53× slower** |
| fib(32) | Mimic nets, 1 thread | 1.80 s | ~60× slower |
| fib(32) | Mimic nets, 12 threads | 0.38 s | **~13× slower** |
| fib(32), raw net | `mimic net run`, 12 threads | 1.07 s | 47.7M interactions/s |
| 50M-element f64 arithmetic plus sum | Mimic, 1 → 12 threads | 1.78 s → 0.59 s | not measured |
| `¨` over a costly pure function | Mimic, 1 → 12 threads | 7.1 s → 2.1 s | not measured |

**What this shows:**
* **Parallelism works.** Nets give 4.2× over Mimic's own native code with no annotations.
* **Single-core code quality is the main gap.** Every scalar is still a heap-allocated, refcounted value. Static types and unboxed scalars (Step 3 above) are the fix, and they come before any further speed claims.
