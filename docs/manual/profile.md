---
title: Profiling
description: mimic profile: time per function and per core, CPU against GPU, memory, tied to where the compiler runs each function.
---

# Profiling

`mimic profile` runs a program and shows where its time goes: in which functions, on how many cores, on the CPU or the GPU, and how much memory it allocated and copied. Each function is shown with where the compiler runs it (the decisions `mimic why` explains), so a slow function and the reason it runs on one core sit side by side.

```sh
mimic profile                     # the project in this folder
mimic profile tool.mim            # one file
mimic profile tool.mim -- input.txt   # with arguments for the program
mimic profile --json              # the report as JSON
```

The program's own output comes first, then the report. This is `bench/tree` (building and counting binary trees) on a 12-core M3 Pro:

```text
0.181 s on the clock, 1.147 s of CPU time on 12 thread(s) (406 samples at 354.0 Hz)

Functions (self: time in the function and the runtime calls it makes; total: with what it calls)
   self   total  function                               where it runs
 49.8%  49.8%  main::make                             ∥ parallel
                 bench/tree/main.mim:8
 30.5%  60.6%  main::main                             ⇶ fused
                 bench/tree/main.mim:19
 11.3%  60.6%  main::main (kernel)                    ⇶ fused
                 bench/tree/main.mim:19
  8.4%   8.4%  main::check                            ∥ parallel
                 bench/tree/main.mim:12

Inside the runtime and the system (part of the self time above)
 40.4%  alloc
 27.6%  release_all
 14.3%  free_block
  2.0%  tlv_get_addr (libdyld.dylib)

Cores (each thread: the share of its time spent working, not waiting for work)
   0 main    █████████████████·············  55.6%
   1 worker  █████████████·················  42.9%
   2 worker  ████████████████████··········  66.7%
   …
  11 worker  ███████████████···············  49.2%
  average  53.7%: 6.3 of 12 cores kept busy

CPU and GPU
  everything ran on the CPU (1.147 s); no kernel went to the GPU

Memory
  134.2 MB allocated in 8388547 value(s) (742.3 MB/s); 0 B copied in 0 copy-on-write copies
```

## Reading the report

* **self** is the share of the CPU time spent in a function, counting the runtime functions it calls (allocation, freeing, sorting) as its own: they are what its code asked for. **total** adds the Mimic functions it calls.
* **(kernel)** is a fused loop made from the function's array expressions; **(task)** is a call of the function run by fork-join on another core. A parallel loop's time on the worker threads counts for the function that started the loop.
* **where it runs** lists the compiler's decisions about the function: `∥ parallel`, `∥ net`, `⇶ fused`, `≋ SIMD`, `▣ GPU`, `◷ task`, `→ in order`. "one core, in order" means there was nothing to split. `mimic why NAME` gives the reason for each, and what would change it.
* **Inside the runtime and the system** shows the runtime's and the system's own functions that took the time, as a hint: a lot of `alloc` and `release_all` means many small values are made and dropped; `radix_scatter` is sorting.
* **Cores** shows, for every thread the runtime started, the share of its time it was working rather than waiting for work. A function marked `∥ parallel` with idle workers has too little work per call, or waits on a part that runs in order.
* **CPU and GPU** gives the time GPU kernels took, their number, and the bytes they moved.
* **Memory** counts the values allocated and their bytes, and the copies made because a value was changed while something else still used it (copy on write). Many copies point at a value kept alive longer than needed.

The report is also saved as JSON next to the program (`target/profiling/NAME.profile.json`) for editors and scripts; `--json` prints it instead.

## How it works

The program is built for release, as `mimic build` would build it, with two differences (in `target/profiling/`): frame pointers are kept, so the stack can be read, and every allocation goes through the runtime's allocator, which counts them. The runtime then wakes about a thousand times a second and records where each of its threads is: the instruction and the calls that led there. A thread waiting for work only counts as waiting. At exit, the samples are named with the program's symbol table and joined with the compiler's decisions. Nothing is added to the program's own code, so it runs at nearly full speed.

Flags:

| Flag | Meaning |
|---|---|
| `--hz N` | Samples per second on each thread (default 1000). When every core is busy the sampler may fall behind; the report uses the rate it reached. |
| `--top N` | How many functions to list (default 15) |
| `--json` | The report as JSON; the program's own output goes to stderr |
| `-- ARGS` | Arguments for the program |

## For agents

`mimic mcp` has a `profile` tool that returns the JSON report. Like `run`, it is off with `--no-run` or `--read-only`, and a program that uses the network runs only with `--allow-network`.
