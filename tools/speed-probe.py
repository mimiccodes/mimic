#!/usr/bin/env python3
"""Where does a benchmark's time go on this machine? For each benchmark in
bench/ (default: the ones that lag on Windows) this builds the Mimic and C
versions, then times them (best of 5) with 1, 2 and all threads, next to an
empty program (the cost of starting a process). It also prints the CPU that
code generation chose and how many vector instructions the program holds.

    tools/speed-probe.py                   # fib mandelbrot prefix_sum sum_of_squares
    tools/speed-probe.py nbody --mimic target/release/mimic
"""
import argparse
import json
import os
import pathlib
import re
import shutil
import subprocess
import sys
import tempfile
import time

ROOT = pathlib.Path(__file__).resolve().parent.parent
EXE = ".exe" if os.name == "nt" else ""


def sh(cmd, env=None):
    r = subprocess.run(cmd, capture_output=True, text=True, env={**os.environ, **(env or {})})
    if r.returncode != 0:
        sys.exit(f"failed: {' '.join(map(str, cmd))}\n{r.stdout}{r.stderr}")
    return r.stdout


def best(exe, env, runs=5):
    out = []
    for _ in range(runs):
        t = time.perf_counter()
        subprocess.run([str(exe)], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, env={**os.environ, **env}, check=True)
        out.append(time.perf_counter() - t)
    return min(out)


def mimic_build(mimic, src, out_dir):
    r = subprocess.run([mimic, "build", str(src), "--emit-json"], capture_output=True, text=True, env={**os.environ, "MIMIC_DEBUG_CPU": "1"})
    if r.returncode != 0:
        sys.exit(f"build failed: {src}\n{r.stdout}{r.stderr}")
    exe = pathlib.Path(json.loads(r.stdout)["artifacts"][0]["path"])
    dst = out_dir / (src.parent.name + "_mim" + EXE)
    shutil.copy(exe, dst)
    m = re.search(r"cpu=(\S+) features=(\S*)", r.stderr)
    cpu = m.group(1) if m else "(cached)"
    if m and m.group(2):
        f = m.group(2).split(",")
        cpu += " (" + " ".join(x for x in ("+avx2", "+fma", "+avx512f", "-avx2", "-avx") if x in f) + ")"
    return dst, cpu


def vector_counts(exe):
    objdump = shutil.which("llvm-objdump") or next((str(p) for p in [pathlib.Path("C:/Program Files/LLVM/bin/llvm-objdump.exe")] if p.exists()), None)
    if not objdump:
        return "no llvm-objdump"
    text = subprocess.run([objdump, "-d", "--no-show-raw-insn", str(exe)], capture_output=True, text=True, errors="replace").stdout
    return f"ymm {len(re.findall(r'%ymm', text))}, zmm {len(re.findall(r'%zmm', text))}"


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("benches", nargs="*", default=["fib", "mandelbrot", "prefix_sum", "sum_of_squares"])
    ap.add_argument("--mimic", default=str(ROOT / "target" / "release" / ("mimic" + EXE)))
    a = ap.parse_args()
    nproc = os.cpu_count() or 1
    print(f"{sys.platform}, {nproc} logical CPUs")
    with tempfile.TemporaryDirectory() as d:
        d = pathlib.Path(d)
        (d / "empty").mkdir()
        (d / "empty" / "main.mim").write_text("module main\n\nfn main() {\n    print(1)\n}\n", encoding="utf-8")
        (d / "empty" / "main.c").write_text("#include <stdio.h>\nint main(void) { puts(\"1\"); return 0; }\n", encoding="utf-8")
        rows = []
        for name in ["empty"] + a.benches:
            src = d / "empty" if name == "empty" else ROOT / "bench" / name
            mexe, cpu = mimic_build(a.mimic, src / "main.mim", d)
            cexe = d / (name + "_c" + EXE)
            sh([a.mimic, "cc", "-O3", "-march=native", str(src / "main.c"), "-o", str(cexe)])
            c = best(cexe, {})
            t = {k: best(mexe, {"MIMIC_THREADS": str(k)}) for k in sorted({1, 2, nproc})}
            rows.append((name, cpu, c, t, vector_counts(mexe)))
            print(f"{name:<15} cpu={cpu:<24} C {c:.3f} s | " + " | ".join(f"{k} thr {v:.3f} s ({v / c:.2f}x)" for k, v in t.items()) + f" | {rows[-1][4]}", flush=True)
        # prefix_sum's parts: chunk size, and the reduce pass alone
        if "prefix_sum" in a.benches:
            red = d / "reduce"
            red.mkdir()
            (red / "main.mim").write_text("module main\n\nfn main() {\n    print(⍳ 100000000 % 1000 ▷ +/)\n}\n", encoding="utf-8")
            rexe, _ = mimic_build(a.mimic, red / "main.mim", d)
            pexe = d / ("prefix_sum_mim" + EXE)
            for k in (1, nproc):
                print(f"reduce only     {k} thr {best(rexe, {'MIMIC_THREADS': str(k)}):.3f} s")
            for g in (16384, 262144, 4194304):
                print(f"prefix_sum      grain {g:>7}: " + " | ".join(f"{k} thr {best(pexe, {'MIMIC_THREADS': str(k), 'MIMIC_PAR_GRAIN': str(g)}):.3f} s" for k in (1, 2, nproc)))
        # all threads without the event loop: does it cost the parallel code?
        for name in a.benches:
            if name not in ("fib", "prefix_sum"):
                continue
            mexe = d / (name + "_mim" + EXE)
            v = best(mexe, {"MIMIC_NO_EVENT_LOOP": "1"})
            print(f"{name:<15} all threads, MIMIC_NO_EVENT_LOOP=1: {v:.3f} s")


main()
