#!/usr/bin/env python3
"""Mimic's benchmark suite (see BENCHMARK.md).

Every folder here holds one benchmark in Mimic, C and Rust (main.mim,
main.c, main.rs) and the output they must all print (expected.txt).
A Rust version that needs crates is a cargo project in rust/ instead of
main.rs (built --offline: the crates must be in the local cargo cache).
A rayon/ cargo project is the parallel Rust version (with Rayon), timed on
all cores next to Mimic on all cores.

A folder with a load/ directory (load/main.rs, the load generator) is a
server benchmark: each
version is a server on port 18432, and the time is how long the load
generator takes to send its requests and check every response body
(expected.txt).
This builds each version the fastest way for its language, checks its output,
then times a warm-up run and 5 measured runs, on one core and on all cores,
and writes results.md with each time as a ratio to C.

    bench/run.py                 # everything
    bench/run.py fib sort        # some benchmarks
    bench/run.py --runs 3        # fewer timed runs
    bench/run.py --json out.json # also write the times as JSON
    bench/run.py --noise 10      # everything 10 times: how much each
                                 # ratio to C varies on this machine
                                 # (bench/noise/<platform>.json)

Every run also appends its times to bench/history.jsonl (with the date, the
commit and the machine) and reports any Mimic time more than 10% slower
than the last run on the same machine.
"""

import argparse
import socket
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


WINDOWS = sys.platform == "win32"
EXE = ".exe" if WINDOWS else ""


def mimic_exe() -> str:
    built = ROOT / "target" / "release" / f"mimic{EXE}"
    return str(built) if built.exists() else "mimic"


def run(cmd, **kw):
    r = subprocess.run(cmd, capture_output=True, text=True, **kw)
    if r.returncode != 0:
        raise RuntimeError(f"{' '.join(map(str, cmd))} failed:\n{r.stdout}{r.stderr}")
    return r


def build(lang: str, src: Path, name: str) -> Path | None:
    """The executable for one version, or None when its compiler is missing."""
    exe = OUT / f"{name}-{lang}{EXE}"
    if lang == "mimic":
        r = run([mimic_exe(), "build", str(src), "--emit-json"])
        return Path(json.loads(r.stdout)["artifacts"][0]["path"])
    if lang == "c":
        # this CPU's features: x86 names the CPU with -march, arm64 with -mcpu
        cpu = "-march=native" if platform.machine() in ("x86_64", "AMD64") else "-mcpu=native"
        libs = ["-lm"] if sys.platform.startswith("linux") else []  # glibc keeps maths apart
        run([mimic_exe(), "cc", "-O3", cpu, str(src), "-o", str(exe)] + libs)
    elif lang in ("rust", "rayon") and src.is_dir():
        # a cargo project (crates such as serde_json, from the local cache)
        if not shutil.which("cargo"):
            return None
        target = OUT / f"{name}-{lang}-cargo"
        # this CPU's features, as for the single-file Rust versions
        flags = (os.environ.get("RUSTFLAGS", "") + " -C target-cpu=native").strip()
        run(["cargo", "build", "--release", "--offline", "--quiet"], cwd=src, env={**os.environ, "CARGO_TARGET_DIR": str(target), "RUSTFLAGS": flags})
        meta = json.loads(run(["cargo", "metadata", "--no-deps", "--format-version", "1", "--offline"], cwd=src).stdout)
        return target / "release" / (meta["packages"][0]["targets"][0]["name"] + EXE)
    elif lang == "rust":
        if not shutil.which("rustc"):
            return None
        run(["rustc", "-C", "opt-level=3", "-C", "target-cpu=native", "--edition", "2021", str(src), "-o", str(exe)])
    return exe


def platform_id() -> str:
    """linux-x86_64, macos-arm64, windows-x86_64, …"""
    system = {"darwin": "macos", "win32": "windows"}.get(sys.platform, "linux" if sys.platform.startswith("linux") else sys.platform)
    arch = {"amd64": "x86_64", "aarch64": "arm64"}.get(platform.machine().lower(), platform.machine().lower())
    return f"{system}-{arch}"


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


# server benchmarks: requests, keep-alive connections, requests per write
LOAD = {"port": 18432, "conns": 64, "requests": 400000, "depth": 16}


def wait_port(port: int, proc, seconds: float = 10.0) -> None:
    t0 = time.perf_counter()
    while time.perf_counter() - t0 < seconds:
        if proc.poll() is not None:
            raise RuntimeError(f"the server exited ({proc.returncode})")
        try:
            with socket.create_connection(("127.0.0.1", port), timeout=0.2):
                return
        except OSError:
            time.sleep(0.05)
    raise RuntimeError("the server did not start listening")


def time_server(exe: Path, load: Path, env: dict, runs: int, expected: str) -> float:
    """Best load-generator time against a running server (a warm-up first)."""
    proc = subprocess.Popen([str(exe)], env={**os.environ, **env}, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    try:
        wait_port(LOAD["port"], proc)
        args = [str(load), str(LOAD["port"]), str(LOAD["conns"]), str(LOAD["requests"]), str(LOAD["depth"]), expected]
        best = float("inf")
        for i in range(runs + 1):
            t = float(run(args).stdout.strip())
            if i:
                best = min(best, t)
        return best
    finally:
        proc.kill()
        proc.wait()


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("names", nargs="*", help="benchmarks to run (default: all)")
    ap.add_argument("--runs", type=int, default=5)
    ap.add_argument("--history", default=str(HERE / "history.jsonl"), help="where to append this run's times ('' for nowhere)")
    ap.add_argument("--json", default="", help="also write this run's times to this JSON file")
    ap.add_argument("--noise", type=int, default=0, metavar="N", help="run everything N times and write how much each ratio to C varies (bench/noise/<platform>.json)")
    a = ap.parse_args()
    OUT.mkdir(exist_ok=True)
    benches = sorted(p for p in HERE.iterdir() if (p / "expected.txt").exists())
    if a.names:
        benches = [b for b in benches if b.name in a.names]
    if a.noise:
        return noise(benches, a.runs, a.noise)
    rows = measure(benches, a.runs)
    report(rows, a.runs, a.history, a.json)
    return 0


def measure(benches, runs: int):
    """Build, check and time every version of each benchmark: [(name, {lang: seconds})]."""
    one = {"MIMIC_THREADS": "1", "TOKIO_WORKER_THREADS": "1"}
    rows = []
    for b in benches:
        expected = (b / "expected.txt").read_text().strip()
        times = {}
        load = None
        if (b / "load").is_dir():
            if not shutil.which("rustc"):
                print(f"  {b.name}: skipped (the load generator needs Rust)")
                continue
            load = OUT / f"{b.name}-load{EXE}"
            run(["rustc", "-C", "opt-level=3", "--edition", "2024", str(b / "load" / "main.rs"), "-o", str(load)])
        timed = (lambda exe, env: time_server(exe, load, env, runs, expected)) if load else (lambda exe, env: time_runs(exe, env, runs, expected))
        for lang, file in [("c", "main.c"), ("rust", "main.rs"), ("rayon", "rayon"), ("mimic", "main.mim")]:
            src = b / file
            if lang == "rust" and (b / "rust" / "Cargo.toml").exists():
                src = b / "rust"
            if not src.exists() or (lang == "rayon" and not (src / "Cargo.toml").exists()):
                continue
            # the C HTTP server is written for epoll and kqueue
            if WINDOWS and lang == "c" and load:
                continue
            exe = build(lang, src, b.name)
            if exe is None:
                continue
            print(f"  {b.name:<16} {lang:<6}", end="", flush=True)
            # Rayon is the parallel version: all cores
            times[lang] = timed(exe, {} if lang == "rayon" else one)
            if lang == "mimic":
                times["mimic-all"] = timed(exe, {})
                print(f" {times['mimic']:.3f} s (1 core), {times['mimic-all']:.3f} s (all cores)")
            elif lang == "rayon":
                print(f" {times[lang]:.3f} s (all cores)")
            else:
                print(f" {times[lang]:.3f} s")
        rows.append((b.name, times))
    return rows


def machine() -> tuple[str, float]:
    """This machine's CPU model, and the load average now."""
    load = os.getloadavg()[0] if hasattr(os, "getloadavg") else 0.0
    try:
        cpu = subprocess.run(["sysctl", "-n", "machdep.cpu.brand_string"], capture_output=True, text=True).stdout.strip()
    except OSError:
        cpu = ""
    if not cpu and Path("/proc/cpuinfo").exists():
        cpu = next((l.split(":", 1)[1].strip() for l in Path("/proc/cpuinfo").read_text().splitlines() if l.startswith("model name")), "")
    return cpu or platform.processor() or platform.machine(), load


def report(rows, runs: int, history: str, json_out: str) -> None:
    """results.md, the table on the terminal, the history, and JSON if asked."""
    c = lambda t, k: f"{t[k]:.3f} s ({t[k] / t['c']:.2f}×)" if k in t and "c" in t else (f"{t[k]:.3f} s" if k in t else "—")
    cpu, load = machine()
    lines = [
        "# Benchmark results",
        "",
        f"{cpu}, {platform.system()} {platform.release()}, {os.cpu_count()} cores, load average {load:.1f} at the end.",
        f"Best of {runs} runs after a warm-up; the ratio is to C (below 1.0 is faster than C).",
        "C and Rust run their straightforward (sequential) versions on one core; Rust + Rayon is the parallel Rust version on all cores.",
        "",
        "| Benchmark | C | Rust | Mimic, 1 core | Rust + Rayon, all cores | Mimic, all cores |",
        "|---|---|---|---|---|---|",
    ]
    for name, t in rows:
        cc = f"{t['c']:.3f} s" if "c" in t else "—"  # (ui: Mimic only)
        lines.append(f"| {name} | {cc} | {c(t, 'rust')} | {c(t, 'mimic')} | {c(t, 'rayon')} | {c(t, 'mimic-all')} |")
    (HERE / "results.md").write_text("\n".join(lines) + "\n", encoding="utf-8")
    print("\n".join(lines[6:]))
    if json_out:
        Path(json_out).write_text(json.dumps({"platform": platform_id(), "cpu": cpu, "cores": os.cpu_count(), "runs": runs, "results": {n: t for n, t in rows}}, indent=1) + "\n", encoding="utf-8")
    if history:
        record(Path(history), cpu, runs, load, rows)


def noise(benches, runs: int, times: int) -> int:
    """Run everything `times` times, unchanged, and record how much each
    ratio to C moves between rounds: the noise a speed gate has to allow
    for on this machine (bench/noise/<platform>.json)."""
    rounds = []
    for i in range(times):
        print(f"round {i + 1} of {times}")
        rounds.append(dict(measure(benches, runs)))
    out = {}
    for name in rounds[0]:
        for k in ("mimic", "mimic-all"):
            ratios = [r[name][k] / r[name]["c"] for r in rounds if name in r and k in r[name] and r[name].get("c")]
            if len(ratios) < 2:
                continue
            mid = sorted(ratios)[len(ratios) // 2]
            # the widest move from the median, as a share of it
            spread = max(abs(x - mid) for x in ratios) / mid
            out.setdefault(name, {})[k] = {"median": round(mid, 4), "spread": round(spread, 4), "ratios": [round(x, 4) for x in ratios]}
    cpu, _ = machine()
    path = HERE / "noise" / f"{platform_id()}.json"
    path.parent.mkdir(exist_ok=True)
    path.write_text(json.dumps({"platform": platform_id(), "cpu": cpu, "cores": os.cpu_count(), "rounds": times, "runs": runs, "date": time.strftime("%Y-%m-%d"), "benchmarks": out}, indent=1) + "\n", encoding="utf-8")
    print(f"\n{'benchmark':<16} {'1 core':>8} {'all cores':>10}   (widest move of the ratio to C)")
    for name, v in out.items():
        cell = lambda k: f"{v[k]['spread'] * 100:.1f}%" if k in v else "—"
        print(f"{name:<16} {cell('mimic'):>8} {cell('mimic-all'):>10}")
    print(f"\nwritten to {path.relative_to(ROOT)}")
    return 0


def record(path: Path, cpu: str, runs: int, load: float, rows) -> None:
    """Append this run to the history; report Mimic times >10% slower than
    the last run on the same machine (same CPU and core count)."""
    try:
        commit = subprocess.run(["git", "rev-parse", "--short", "HEAD"], cwd=ROOT, capture_output=True, text=True).stdout.strip()
    except OSError:
        commit = ""
    entry = {
        "date": time.strftime("%Y-%m-%dT%H:%M:%S"),
        "commit": commit,
        "cpu": cpu,
        "os": f"{platform.system()} {platform.release()}",
        "cores": os.cpu_count(),
        "load": round(load, 1),
        "runs": runs,
        "results": {name: {k: round(v, 4) for k, v in t.items()} for name, t in rows},
    }
    last = None
    if path.exists():
        for line in path.read_text().splitlines():
            try:
                e = json.loads(line)
            except ValueError:
                continue
            if e.get("cpu") == cpu and e.get("cores") == entry["cores"]:
                last = e
    if last:
        slower = []
        for name, t in entry["results"].items():
            old = last["results"].get(name, {})
            for k in ("mimic", "mimic-all"):
                if k in t and k in old and old[k] > 0 and t[k] > old[k] * 1.10:
                    slower.append(f"  {name} ({k}): {old[k]:.3f} s -> {t[k]:.3f} s (+{(t[k] / old[k] - 1) * 100:.0f}%)")
        print(f"\ncompared with {last['date']} ({last['commit']}):")
        print("\n".join(slower) if slower else "  no Mimic time is more than 10% slower")
    with path.open("a", encoding="utf-8") as f:
        f.write(json.dumps(entry) + "\n")


if __name__ == "__main__":
    sys.exit(main())
