#!/usr/bin/env python3
"""Mimic's benchmark suite (see BENCHMARK.md).

Every folder here holds one benchmark in Mimic, C, Rust and Go (main.mim,
main.c, main.rs, main.go) and the output they must all print (expected.txt).
A Rust version that needs crates is a cargo project in rust/ instead of
main.rs (built --offline: the crates must be in the local cargo cache).
This builds each version the fastest way for its language, checks its output,
then times a warm-up run and 5 measured runs, on one core and on all cores,
and writes results.md with each time as a ratio to C.

    bench/run.py                 # everything
    bench/run.py fib sort        # some benchmarks
    bench/run.py --runs 3        # fewer timed runs
"""

import argparse
import json
import os
import platform
import shutil
import subprocess
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent
OUT = HERE / "target"


def mimic_exe() -> str:
    built = ROOT / "target" / "release" / "mimic"
    return str(built) if built.exists() else "mimic"


def run(cmd, **kw):
    r = subprocess.run(cmd, capture_output=True, text=True, **kw)
    if r.returncode != 0:
        raise RuntimeError(f"{' '.join(map(str, cmd))} failed:\n{r.stdout}{r.stderr}")
    return r


def build(lang: str, src: Path, name: str) -> Path | None:
    """The executable for one version, or None when its compiler is missing."""
    exe = OUT / f"{name}-{lang}"
    if lang == "mimic":
        r = run([mimic_exe(), "build", str(src), "--emit-json"])
        return Path(json.loads(r.stdout)["artifacts"][0]["path"])
    if lang == "c":
        run([mimic_exe(), "cc", "-O3", "-mcpu=native", str(src), "-o", str(exe)])
    elif lang == "rust" and src.is_dir():
        # a cargo project (crates such as serde_json, from the local cache)
        if not shutil.which("cargo"):
            return None
        target = OUT / f"{name}-rust-cargo"
        run(["cargo", "build", "--release", "--offline", "--quiet"], cwd=src, env={**os.environ, "CARGO_TARGET_DIR": str(target)})
        meta = json.loads(run(["cargo", "metadata", "--no-deps", "--format-version", "1", "--offline"], cwd=src).stdout)
        return target / "release" / meta["packages"][0]["targets"][0]["name"]
    elif lang == "rust":
        if not shutil.which("rustc"):
            return None
        run(["rustc", "-C", "opt-level=3", "-C", "target-cpu=native", "--edition", "2021", str(src), "-o", str(exe)])
    elif lang == "go":
        if not shutil.which("go"):
            return None
        run(["go", "build", "-o", str(exe), str(src)], env={**os.environ, "GO111MODULE": "off"})
    return exe


def time_runs(exe: Path, env: dict, runs: int, expected: str) -> float:
    """Best wall-clock time in seconds, after one warm-up that also checks the output."""
    warm = run([str(exe)], env={**os.environ, **env})
    got = warm.stdout.strip()
    if got != expected:
        raise RuntimeError(f"{exe.name}: printed {got!r}, expected {expected!r}")
    best = float("inf")
    for _ in range(runs):
        t0 = time.perf_counter()
        run([str(exe)], env={**os.environ, **env})
        best = min(best, time.perf_counter() - t0)
    return best


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("names", nargs="*", help="benchmarks to run (default: all)")
    ap.add_argument("--runs", type=int, default=5)
    a = ap.parse_args()
    OUT.mkdir(exist_ok=True)
    benches = sorted(p for p in HERE.iterdir() if (p / "expected.txt").exists())
    if a.names:
        benches = [b for b in benches if b.name in a.names]
    one = {"MIMIC_THREADS": "1", "GOMAXPROCS": "1"}
    rows = []
    for b in benches:
        expected = (b / "expected.txt").read_text().strip()
        times = {}
        for lang, file in [("c", "main.c"), ("rust", "main.rs"), ("go", "main.go"), ("mimic", "main.mim")]:
            src = b / file
            if lang == "rust" and (b / "rust" / "Cargo.toml").exists():
                src = b / "rust"
            if not src.exists():
                continue
            exe = build(lang, src, b.name)
            if exe is None:
                continue
            print(f"  {b.name:<16} {lang:<6}", end="", flush=True)
            times[lang] = time_runs(exe, one, a.runs, expected)
            if lang == "mimic":
                times["mimic-all"] = time_runs(exe, {}, a.runs, expected)
                print(f" {times['mimic']:.3f} s (1 core), {times['mimic-all']:.3f} s (all cores)")
            else:
                print(f" {times[lang]:.3f} s")
        rows.append((b.name, times))

    c = lambda t, k: f"{t[k]:.3f} s ({t[k] / t['c']:.2f}×)" if k in t and "c" in t else "—"
    load = os.getloadavg()[0]
    cpu = subprocess.run(["sysctl", "-n", "machdep.cpu.brand_string"], capture_output=True, text=True).stdout.strip() or platform.processor()
    lines = [
        "# Benchmark results",
        "",
        f"{cpu}, {platform.system()} {platform.release()}, {os.cpu_count()} cores, load average {load:.1f} at the end.",
        f"Best of {a.runs} runs after a warm-up; the ratio is to C (below 1.0 is faster than C).",
        "C, Rust and Go run their straightforward (sequential) versions on one core.",
        "",
        "| Benchmark | C | Rust | Go | Mimic, 1 core | Mimic, all cores |",
        "|---|---|---|---|---|---|",
    ]
    for name, t in rows:
        lines.append(f"| {name} | {t['c']:.3f} s | {c(t, 'rust')} | {c(t, 'go')} | {c(t, 'mimic')} | {c(t, 'mimic-all')} |")
    (HERE / "results.md").write_text("\n".join(lines) + "\n")
    print("\n".join(lines[6:]))
    return 0


if __name__ == "__main__":
    sys.exit(main())
