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
    bench/run.py --noise 8       # the gate's comparison with the same
                                 # compiler on both sides, 8 times: the
                                 # noise each benchmark's tolerance allows
                                 # (bench/noise/<platform>.json)
    bench/run.py --machine       # measure this machine's limits again
    bench/run.py --gate OLD      # the speed gate: Mimic as built here
                                 # against the mimic at OLD (the code it
                                 # changes), on this machine, in turns

The gate: each benchmark is built with both compilers and timed with
each in turn (old, new, old, new, …), on one core and on all, so both
meet the same machine and the same moment. A new time worse than the old
by more than the benchmark's tolerance is measured twice more and fails
if it is worse in 2 of the 3. The tolerance is twice the noise measured
for the benchmark on this platform (bench/noise/), at least 5% and at
most 25%. Comparing with numbers recorded on another run does not work on
shared machines: a CI job gets a different machine each time. --inject
builds the new side without LLVM's optimizer (MIMIC_DEBUG_DISABLE), a
real slowdown the gate must catch.

The machine's limits (peak floating-point rate and memory bandwidth, on
one core and on all, from bench/machine/) are measured once per machine
and kept in bench/target/. A benchmark whose work counts against one of
them says so in meta.json: {"bound": "compute", "flops": N} or
{"bound": "memory", "bytes": N}; its results then also show as a share
of what the machine can do.

Every run also appends its times to bench/history.jsonl (with the date, the
commit and the machine) and reports any Mimic time more than 10% slower
than the last run on the same machine.
"""

import argparse
import socket
import json
import math
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
    if os.environ.get("MIMIC_EXE"):
        return os.environ["MIMIC_EXE"]
    built = ROOT / "target" / "release" / f"mimic{EXE}"
    return str(built) if built.exists() else "mimic"


def run(cmd, timeout=None, **kw):
    r = subprocess.run(cmd, capture_output=True, text=True, timeout=timeout, **kw)
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


def time_runs(exe: Path, env: dict, runs: int, expected: str, limit: float | None = None) -> float:
    """Best wall-clock time in seconds, after one warm-up that also checks
    the output; infinite when a run takes longer than `limit` seconds."""
    try:
        warm = run([str(exe)], env={**os.environ, **env}, timeout=limit)
    except subprocess.TimeoutExpired:
        return float("inf")
    got = warm.stdout.strip()
    if got != expected:
        raise RuntimeError(f"{exe.name}: printed {got!r}, expected {expected!r}")
    best = float("inf")
    for _ in range(runs):
        t0 = time.perf_counter()
        try:
            run([str(exe)], env={**os.environ, **env}, timeout=limit)
        except subprocess.TimeoutExpired:
            continue
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


def time_server(exe: Path, load: Path, env: dict, runs: int, expected: str, limit: float | None = None) -> float:
    """Best load-generator time against a running server (a warm-up
    first); infinite when a load takes longer than `limit` seconds."""
    proc = subprocess.Popen([str(exe)], env={**os.environ, **env}, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    try:
        wait_port(LOAD["port"], proc)
        args = [str(load), str(LOAD["port"]), str(LOAD["conns"]), str(LOAD["requests"]), str(LOAD["depth"]), expected]
        best = float("inf")
        for i in range(runs + 1):
            try:
                t = float(run(args, timeout=limit).stdout.strip())
            except subprocess.TimeoutExpired:
                return float("inf")
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
    ap.add_argument("--noise", type=int, default=0, metavar="N", help="the gate's comparison with the same compiler on both sides, N times: the noise behind each tolerance (bench/noise/<platform>.json)")
    ap.add_argument("--machine", action="store_true", help="measure this machine's limits again (and only that, without benchmark names)")
    ap.add_argument("--gate", default="", metavar="OLD", help="the speed gate: compare Mimic as built here with the mimic at OLD, on this machine, in turns")
    ap.add_argument("--inject", action="store_true", help="with --gate: build the new side without LLVM's optimizer (the gate must fail)")
    ap.add_argument("--shard", default="", metavar="I/N", help="only every N-th benchmark, starting with the I-th (from 0): CI splits the gate across machines")
    a = ap.parse_args()
    OUT.mkdir(exist_ok=True)
    if a.machine:
        limits(fresh=True)
        if not a.names:
            return 0
    benches = sorted(p for p in HERE.iterdir() if (p / "expected.txt").exists())
    if a.names:
        benches = [b for b in benches if b.name in a.names]
    if a.shard:
        i, n = map(int, a.shard.split("/"))
        benches = benches[i::n]
        print(f"shard {i}/{n}: {' '.join(b.name for b in benches)}")
    if a.noise:
        return noise(benches, a.runs, a.noise)
    if a.gate:
        return gate(benches, Path(a.gate), a.runs, a.inject)
    rows = measure(benches, a.runs)
    report(rows, a.runs, a.history, a.json)
    return 0


def measure(benches, runs: int):
    """Build, check and time every version of each benchmark: [(name, {lang: seconds})]."""
    one = {"MIMIC_THREADS": "1", "TOKIO_WORKER_THREADS": "1"}
    rows = []
    for b in benches:
        expected = (b / "expected.txt").read_text(encoding="utf-8").strip()
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
        cpu = next((l.split(":", 1)[1].strip() for l in Path("/proc/cpuinfo").read_text(encoding="utf-8").splitlines() if l.startswith("model name")), "")
    return cpu or platform.processor() or platform.machine(), load


def wall(cmds) -> float:
    """Seconds for these commands, all started at once, to finish."""
    t0 = time.perf_counter()
    procs = [subprocess.Popen(c, stdout=subprocess.DEVNULL) for c in cmds]
    if any(p.wait() != 0 for p in procs):
        raise RuntimeError(f"{cmds[0][0]} failed")
    return time.perf_counter() - t0


def limits(fresh: bool = False) -> dict:
    """The machine's limits: peak floating-point operations a second and
    memory bandwidth, on one core and on all (all: one copy per core at
    once). Each is timed at two sizes, the difference taking start-up
    away; best of 5. Kept per machine in bench/target/. A share above
    100% means hardware beyond the CPU's vector units did the work (such
    as Apple's matrix unit), or a busy machine measured its limits low."""
    cpu, _ = machine()
    cores = os.cpu_count() or 1
    path = OUT / f"machine-{platform_id()}.json"
    if not fresh and path.exists():
        try:
            m = json.loads(path.read_text(encoding="utf-8"))
            if m.get("cpu") == cpu and m.get("cores") == cores:
                return m
        except ValueError:
            pass
    print("measuring this machine's limits (bench/machine)")
    peak = build("c", HERE / "machine" / "peak.c", "machine-peak")
    stream = build("c", HERE / "machine" / "stream.c", "machine-stream")

    def rate(cmd, small, large, work_per_step, copies):
        best = float("inf")
        for _ in range(5):
            dt = wall([cmd(large)] * copies) - wall([cmd(small)] * copies)
            best = min(best, dt) if dt > 0 else best
        return work_per_step * (large - small) * copies / best

    # peak: accumulators × lanes × 2 operations per round (the probe says)
    flops = int(run([str(peak), "w"]).stdout)
    rounds = 2_000_000
    while wall([[str(peak), str(rounds)]]) < 0.25 and rounds < 1 << 34:
        rounds *= 2
    p_cmd = lambda r: [str(peak), str(r)]
    m = {"cpu": cpu, "cores": cores, "platform": platform_id(), "date": time.strftime("%Y-%m-%d")}
    m["flops_1"] = rate(p_cmd, rounds // 4, rounds, flops, 1)
    m["flops_all"] = rate(p_cmd, rounds // 4, rounds, flops, cores)
    # bandwidth: 24 bytes per element per round; arrays far beyond the caches
    n1, nall = 1 << 24, 1 << 22
    m["bytes_1"] = rate(lambda r: [str(stream), str(n1), str(r)], 2, 12, 24 * n1, 1)
    m["bytes_all"] = rate(lambda r: [str(stream), str(nall), str(r)], 2, 12, 24 * nall, cores)
    path.write_text(json.dumps(m, indent=1) + "\n", encoding="utf-8")
    print(f"  peak {m['flops_1'] / 1e9:.1f} GFLOP/s on one core, {m['flops_all'] / 1e9:.1f} on all {cores}")
    print(f"  memory {m['bytes_1'] / 1e9:.1f} GB/s on one core, {m['bytes_all'] / 1e9:.1f} on all {cores}")
    return m


def shares(rows) -> list:
    """For benchmarks with a meta.json: the share of the machine's limit
    each version reached, [(name, bound, {lang: share})]."""
    out = []
    m = None
    for name, t in rows:
        f = HERE / name / "meta.json"
        if not f.exists():
            continue
        meta = json.loads(f.read_text(encoding="utf-8"))
        bound = meta.get("bound")
        if bound not in ("compute", "memory"):
            continue
        m = m or limits()
        work = meta["flops"] if bound == "compute" else meta["bytes"]
        key = "flops" if bound == "compute" else "bytes"
        got = {}
        for lang in ("c", "rust", "mimic", "rayon", "mimic-all"):
            if lang in t and t[lang] > 0:
                limit = m[f"{key}_all"] if lang in ("rayon", "mimic-all") else m[f"{key}_1"]
                got[lang] = work / t[lang] / limit
        out.append((name, bound, got))
    return out


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
    table = len(lines)
    sh = shares(rows)
    if sh:
        m = limits()
        pct = lambda d, k: f"{d[k] * 100:.0f}%" if k in d else "—"
        lines += [
            "",
            "## Share of the machine",
            "",
            f"What each version reached of this machine's limit: peak {m['flops_1'] / 1e9:.1f} GFLOP/s on one core and {m['flops_all'] / 1e9:.1f} on all; memory {m['bytes_1'] / 1e9:.1f} GB/s on one core and {m['bytes_all'] / 1e9:.1f} on all (bench/machine).",
            "",
            "| Benchmark | Limited by | C | Rust | Mimic, 1 core | Rust + Rayon, all cores | Mimic, all cores |",
            "|---|---|---|---|---|---|---|",
        ]
        for name, bound, d in sh:
            lines.append(f"| {name} | {bound} | {pct(d, 'c')} | {pct(d, 'rust')} | {pct(d, 'mimic')} | {pct(d, 'rayon')} | {pct(d, 'mimic-all')} |")
    (HERE / "results.md").write_text("\n".join(lines) + "\n", encoding="utf-8")
    print("\n".join(lines[6:]))
    if json_out:
        Path(json_out).write_text(json.dumps({"platform": platform_id(), "cpu": cpu, "cores": os.cpu_count(), "runs": runs, "results": {n: t for n, t in rows}, "machine": limits() if sh else None, "shares": {n: {"bound": b, **d} for n, b, d in sh}}, indent=1) + "\n", encoding="utf-8")
    if history:
        record(Path(history), cpu, runs, load, rows)


# at least 5%: two builds of the same code differ by a few percent (where
# the code lands in memory), which one binary measured twice cannot show
LOWEST, HIGHEST, DEFAULT = 0.05, 0.25, 0.10
# the gate times a benchmark until each side has run this long (seconds),
# in at most MOST_RUNS runs
SPAN, MOST_RUNS = 1.0, 30
# the old side's runs stop after OLD_TIMES times the new side's time (at
# least OLD_CAP seconds): a release pull request measures against main,
# whose compiler can be hundreds of times slower on a newer benchmark, and
# such a benchmark cannot fail the gate. Neither can one whose old side is
# FAR times slower, so it is not timed further.
OLD_CAP, OLD_TIMES, FAR = 5.0, 10, 2.0


def tolerance(name: str, key: str) -> tuple[float, bool]:
    """Twice the noise measured on this platform, within 5% … 25%, and
    whether the benchmark is gated here: one whose noise needs more than
    25% is measured and reported but cannot fail the gate (no tolerance
    would tell its slowdowns from its noise)."""
    f = HERE / "noise" / f"{platform_id()}.json"
    try:
        spread = json.loads(f.read_text(encoding="utf-8"))["benchmarks"][name][key]["spread"]
    except (OSError, ValueError, KeyError):
        return DEFAULT, True
    return min(HIGHEST, max(lowest(key), 2 * spread)), 2 * spread <= HIGHEST


def lowest(key: str) -> float:
    """The least tolerance for a measurement. On all cores of the shared
    macOS runners (three cores, other jobs beside), a run-to-run swing of 10
    to 16% came and went on a different benchmark each time: six false
    alarms in one week (2026-11-18/19), where eight rounds on one machine
    measured 1 to 5%. The two real slowdowns the gate caught there in that
    week showed on one core (vm +11%, diff +18%). So all cores on macOS
    allow at least 15%."""
    return 0.15 if key == "mimic-all" and platform_id() == "macos-arm64" else LOWEST


def build_with(compiler: Path, src: Path, env: dict, side: str) -> Path:
    """The program built by `compiler`, copied to its own name (both
    compilers build to the same place)."""
    r = run([str(compiler), "build", str(src), "--emit-json"], env={**os.environ, **env})
    built = Path(json.loads(r.stdout)["artifacts"][0]["path"])
    own = OUT / f"{src.parent.name}-{side}{EXE}"
    shutil.copy2(built, own)
    return own


def pair(b: Path, old: Path, new: Path, runs: int, inject: bool) -> dict:
    """One benchmark built with both compilers and timed in turns:
    {"mimic": (old, new), "mimic-all": (old, new)} in seconds (best of; the
    old side infinite past its limit), or the reason there is nothing to
    compare: the old compiler cannot build it, or its build fails."""
    expected = (b / "expected.txt").read_text(encoding="utf-8").strip()
    load = None
    if (b / "load").is_dir():
        load = OUT / f"{b.name}-load{EXE}"
        if not load.exists():
            run(["rustc", "-C", "opt-level=3", "--edition", "2024", str(b / "load" / "main.rs"), "-o", str(load)])
    src = b / "main.mim"
    try:
        before = build_with(old, src, {}, "old")
    except RuntimeError:
        return "new: the old compiler cannot build it"
    exes = (before, build_with(new, src, {"MIMIC_DEBUG_DISABLE": "optimizer"} if inject else {}, "new"))
    once = (lambda exe, env, limit=None: time_server(exe, load, env, 1, expected, limit)) if load else (lambda exe, env, limit=None: time_runs(exe, env, 1, expected, limit))
    out = {}
    for key, env in (("mimic", {"MIMIC_THREADS": "1", "TOKIO_WORKER_THREADS": "1"}), ("mimic-all", {})):
        best = [float("inf"), float("inf")]
        done, want = 0, runs
        while done < want:
            best[1] = min(best[1], once(exes[1], env))
            try:
                best[0] = min(best[0], once(exes[0], env, max(OLD_CAP, OLD_TIMES * best[1])))
            except RuntimeError:
                # it crashed or printed something else: a bug the new
                # compiler fixed, not a time to compare
                return "the old build fails (a bug since fixed)"
            done += 1
            if best[0] > FAR * best[1]:
                break
            if done == runs:
                # a short benchmark is timed until each side has run for
                # SPAN seconds: the best of 3 runs of 50 ms still moved 20%
                want = max(runs, min(MOST_RUNS, math.ceil(SPAN / max(min(best), 1e-3))))
        out[key] = tuple(best)
    return out


def gate(benches, old: Path, runs: int, inject: bool) -> int:
    """Mimic as built here against the mimic at `old`, benchmark by
    benchmark on this machine; slower beyond the tolerance in 2 of 3
    measurements, and again in 2 of 3 at the end of the run, fails. Writes bench/gate.md; returns the exit code."""
    new = Path(mimic_exe())
    if not old.exists():
        print(f"error: no mimic at {old}", file=sys.stderr)
        return 2
    title = "the new side built without the optimizer" if inject else f"against {old}"
    lines = [f"## Speed gate ({platform_id()}, {title})", "", "| Benchmark | | Before | After | Change | Tolerance | |", "|---|---|---|---|---|---|---|"]
    failed, faster, suspects = [], [], []
    for b in benches:
        if not (b / "main.mim").exists():
            continue
        print(f"  {b.name}", flush=True)
        got = pair(b, old, new, runs, inject)
        if isinstance(got, str):
            lines.append(f"| {b.name} | | | | | | {got} |")
            continue
        for key, (t_old, t_new) in got.items():
            where = "1 core" if key == "mimic" else "all cores"
            tol, gated = tolerance(b.name, key)
            change = t_new / t_old
            verdict = "ok"
            if change > 1 + tol and not gated:
                verdict = "too noisy to gate here"
            elif change > 1 + tol:
                # measured twice more, with more runs: worse in 2 of 3 fails
                again = [pair(b, old, new, runs + 2, inject)[key] for _ in range(2)]
                worse = 1 + sum(n / o > 1 + tol for o, n in again)
                change = sorted([change, *(n / o for o, n in again)])[1]
                verdict = "**slower**" if worse >= 2 else "ok (noise)"
                if worse >= 2:
                    # confirmed at the end of the run, minutes later
                    suspects.append((b, key, tol, where, len(lines)))
            elif change < 1 - tol:
                verdict = "faster"
                faster.append(f"{b.name} ({where})")
            limit = f"{tol * 100:.0f}%" if gated else "not gated"
            if t_old == float("inf"):
                cap = max(OLD_CAP, OLD_TIMES * t_new)
                lines.append(f"| {b.name} | {where} | over {cap:.0f} s | {t_new:.3f} s | over {cap / t_new:.0f}× faster | {limit} | {verdict} |")
                continue
            lines.append(f"| {b.name} | {where} | {t_old:.3f} s | {t_new:.3f} s | {(change - 1) * 100:+.0f}% | {limit} | {verdict} |")
    # A benchmark slower here is measured 3 times more once every other has
    # run, minutes later: it fails only if slower in 2 of those 3 too. The
    # shared machines (macOS above all) slow down for a while now and then,
    # and the three measurements above, seconds apart, all fall into such a
    # spell; a real slowdown is still there minutes later.
    # (not in the self-check, where nearly everything is slower: it only
    # has to fail)
    for b, key, tol, where, at in suspects:
        if inject:
            failed.append(f"{b.name} ({where})")
            continue
        print(f"  {b.name} ({where}) again", flush=True)
        later = [pair(b, old, new, runs + 2, inject)[key] for _ in range(3)]
        worse = sum(n / o > 1 + tol for o, n in later)
        mid = sorted(n / o for o, n in later)[1]
        if worse >= 2:
            failed.append(f"{b.name} ({where})")
            lines[at] = lines[at].replace("| **slower** |", f"| **slower** (again later: {(mid - 1) * 100:+.0f}%) |")
        else:
            lines[at] = lines[at].replace("| **slower** |", f"| ok (noise: {(mid - 1) * 100:+.0f}% later) |")
    lines.append("")
    if failed:
        lines.append(f"**Slower:** {', '.join(failed)}.")
    if faster:
        lines.append(f"Faster: {', '.join(faster)}.")
    if not failed and not faster:
        lines.append("Every benchmark is within its tolerance.")
    (HERE / "gate.md").write_text("\n".join(lines) + "\n", encoding="utf-8")
    print("\n" + "\n".join(lines))
    return 1 if failed else 0


def noise(benches, runs: int, times: int) -> int:
    """The gate's own comparison with the same compiler on both sides,
    `times` times per benchmark: every difference is noise. Its widest
    move is what the gate must allow for on this machine
    (bench/noise/<platform>.json)."""
    me = Path(mimic_exe())
    path = HERE / "noise" / f"{platform_id()}.json"
    # what is already measured stays: CI measures a third of the benchmarks
    # on each machine (--shard), and each benchmark carries its own date
    try:
        out = json.loads(path.read_text(encoding="utf-8"))["benchmarks"]
    except (OSError, ValueError, KeyError):
        out = {}
    measured = []
    for b in benches:
        if not (b / "main.mim").exists():
            continue
        print(f"  {b.name}", flush=True)
        changes = {}
        for _ in range(times):
            for key, (t_old, t_new) in pair(b, me, me, runs, False).items():
                changes.setdefault(key, []).append(t_new / t_old)
        out[b.name] = {"date": time.strftime("%Y-%m-%d")}
        for key, rs in changes.items():
            out[b.name][key] = {"spread": round(max(abs(r - 1) for r in rs), 4), "ratios": [round(r, 4) for r in rs]}
        measured.append(b.name)
    cpu, _ = machine()
    path.parent.mkdir(exist_ok=True)
    path.write_text(json.dumps({"platform": platform_id(), "cpu": cpu, "cores": os.cpu_count(), "rounds": times, "runs": runs, "date": time.strftime("%Y-%m-%d"), "how": "the same compiler on both sides of the gate's comparison", "benchmarks": out}, indent=1) + "\n", encoding="utf-8")
    print(f"\n{'benchmark':<16} {'1 core':>8} {'all cores':>10}   (widest move, same compiler both sides)")
    for name, v in ((n, out[n]) for n in measured):
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
        for line in path.read_text(encoding="utf-8").splitlines():
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
