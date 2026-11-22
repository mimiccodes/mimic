#!/usr/bin/env python3
"""Count the reference-count operations of the benchmark corpus.

Builds each benchmark in bench/ with MIMIC_COUNT_RC=1 (a runtime that counts
every retain, release and value freed, docs/design/ownership.md), runs it on
one core, checks its output, and prints a table with the totals (server
benchmarks, which need their load generator, are left out): how 0.4.2's
ownership analysis is measured ("reference-count operations in the corpus at
least halved").

    python3 tools/rc-count.py --mimic target/release/mimic [names…] [--out file.tsv]
"""

import argparse
import json
import os
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
LINE = re.compile(r"mimic: reference counts: (\d+) retains, (\d+) releases, (\d+) values freed")


def count(mimic: str, name: str) -> tuple[int, int, int] | str:
    # a server benchmark needs its load generator (bench/run.py): left out
    if (ROOT / "bench" / name / "load").is_dir():
        return "server (left out)"
    src = ROOT / "bench" / name / "main.mim"
    env = {**os.environ, "MIMIC_COUNT_RC": "1", "MIMIC_THREADS": "1"}
    r = subprocess.run([mimic, "build", "--release", str(src), "--emit-json"], capture_output=True, text=True, env=env)
    try:
        exe = json.loads(r.stdout)["artifacts"][0]["path"]
    except (ValueError, KeyError, IndexError):
        return "build failed"
    try:
        run = subprocess.run([exe], capture_output=True, text=True, env=env, timeout=600)
    except subprocess.TimeoutExpired:
        return "timeout (10 min)"
    want = (ROOT / "bench" / name / "expected.txt").read_text().strip()
    if run.stdout.strip() != want:
        return "wrong output"
    m = LINE.search(run.stderr)
    if not m:
        return "no counts"
    return int(m[1]), int(m[2]), int(m[3])


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--mimic", required=True)
    ap.add_argument("--out")
    ap.add_argument("names", nargs="*")
    a = ap.parse_args()
    names = a.names or sorted(p.name for p in (ROOT / "bench").iterdir() if (p / "main.mim").exists() and (p / "expected.txt").exists())
    rows, total = [], [0, 0, 0]
    for n in names:
        c = count(a.mimic, n)
        if isinstance(c, str):
            rows.append(f"{n}\t{c}")
        else:
            total = [t + x for t, x in zip(total, c)]
            rows.append(f"{n}\t{c[0]}\t{c[1]}\t{c[2]}\t{c[0] + c[1]}")
        print(rows[-1], flush=True)
    summary = f"TOTAL\t{total[0]}\t{total[1]}\t{total[2]}\t{total[0] + total[1]}"
    print(summary)
    if a.out:
        Path(a.out).write_text("name\tretains\treleases\tfreed\tops\n" + "\n".join(rows + [summary]) + "\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
