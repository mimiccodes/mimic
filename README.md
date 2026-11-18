# Mimic's benchmark history

This branch holds data, not code: every measurement of Mimic's benchmark
suite, kept for good. The suite itself (the programs in Mimic, C and Rust,
the runner and the rules) lives on `main` in
[bench/](https://github.com/mimiccodes/mimic/tree/main/bench) and
[BENCHMARK.md](https://github.com/mimiccodes/mimic/blob/main/BENCHMARK.md).

| Path | What |
|---|---|
| `phases/` | One summary per stage, preview and release: every benchmark against C and Rust, and what changed since the phase before. `0.1-0.3.md` is the history up to 0.3.18. |
| `runs/YYYY/MM/DD/` | Every run, as JSON (the machine, the compiler's commit, every time) and as a table: nightly runs, release runs, the speed gate and the runs on dedicated machines. |
| `history/<machine>.csv` | One line per benchmark per run on that machine, for charts over time. |
| `machines.md` | The machines the numbers come from. |

Written by CI (`bench` workflow); nothing here is edited by hand. Times
are wall-clock seconds, best of several runs; ratios are to C, and below
1.0 is faster than C. Shared CI machines are noisy: compare ratios, and
runs on the same machine.
