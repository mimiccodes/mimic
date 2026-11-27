#!/usr/bin/env python3
"""Count the reference-count operations of the benchmark corpus.

Builds each benchmark in bench/ with MIMIC_COUNT_RC=1 (a runtime that counts
every retain, release and value freed, docs/design/ownership.md), runs it on
one core, checks its output, and prints a table with the totals (server
benchmarks, which need their load generator, are left out): how 0.4.2's
ownership analysis is measured ("reference-count operations in the corpus at
least halved"). Also the values made in loop steps (evolve, settle; past
each loop's first step, which may copy a start still shared): 0.4.3's
"steady-state loops allocate nothing", the loops that do listed at the end.

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
STEPS = re.compile(r"mimic: loop steps: (\d+) steps, (\d+) values made in them")


def count(mimic: str, name: str) -> tuple[int, int, int, int, int] | str:
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
    st = STEPS.search(run.stderr)
    steps, made = (int(st[1]), int(st[2])) if st else (0, 0)
    return int(m[1]), int(m[2]), int(m[3]), steps, made


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--mimic", required=True)
    ap.add_argument("--out")
    ap.add_argument("names", nargs="*")
    a = ap.parse_args()
    names = a.names or sorted(p.name for p in (ROOT / "bench").iterdir() if (p / "main.mim").exists() and (p / "expected.txt").exists())
    rows, total = [], [0, 0, 0, 0, 0]
    # loops that make values in their steps (evolve, settle): more than one
    # value per 100 steps, past a loop's first steps (a copy when its start
    # is shared): the steady state should make none
    making = []
    for n in names:
        c = count(a.mimic, n)
        if isinstance(c, str):
            rows.append(f"{n}\t{c}")
        else:
            total = [t + x for t, x in zip(total, c)]
            rows.append(f"{n}\t{c[0]}\t{c[1]}\t{c[2]}\t{c[0] + c[1]}\t{c[3]}\t{c[4]}")
            if c[4] > 1000 and c[4] * 100 > c[3]:
                making.append(f"{n}\t{c[3]} steps\t{c[4]} made\t{c[4] / max(c[3], 1):.2f} per step")
        print(rows[-1], flush=True)
    summary = f"TOTAL\t{total[0]}\t{total[1]}\t{total[2]}\t{total[0] + total[1]}\t{total[3]}\t{total[4]}"
    print(summary)
    print("loops making values in their steps:" if making else "loops making values in their steps: none")
    for m in making:
        print("  " + m)
    if a.out:
        Path(a.out).write_text("name\tretains\treleases\tfreed\tops\tsteps\tmade_in_steps\n" + "\n".join(rows + [summary]) + "\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
