#!/usr/bin/env python3
"""Add one run to the benchmark history (the `bench` branch).

    bench/record.py RESULT.json RESULTS.md DIR MACHINE [PHASE]

RESULT.json is `bench/run.py --json`; RESULTS.md its table; DIR a
checkout of the `bench` branch; MACHINE a name from its machines.md
(github-linux-x86_64, aws-c7a.4xlarge, local-macos-arm64, …); PHASE a
version when the run is a release's or a preview's. It writes
runs/YYYY/MM/DD/<run>-<machine>.json and .md, adds a line per benchmark to
history/<machine>.csv (one file per machine, so machines that finish at
the same time never write the same file), and with PHASE,
phases/<PHASE>/<machine>.md.
"""

import csv
import json
import os
import subprocess
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent


def version() -> tuple[str, str]:
    exe = ROOT / "target" / "release" / ("mimic.exe" if os.name == "nt" else "mimic")
    try:
        line = subprocess.run([str(exe), "--version"], capture_output=True, text=True).stdout.split("\n")[0]
        parts = line.split()
        return parts[1], parts[2].strip("(,")
    except (OSError, IndexError):
        return "?", "?"


def main(argv) -> int:
    if len(argv) < 5:
        print(__doc__)
        return 2
    result = json.loads(Path(argv[1]).read_text(encoding="utf-8"))
    table = Path(argv[2]).read_text(encoding="utf-8")
    d, machine = Path(argv[3]), argv[4]
    phase = argv[5] if len(argv) > 5 and argv[5] else ""
    ver, commit = version()
    run_id = os.environ.get("GITHUB_RUN_ID", "local")
    day = time.strftime("%Y/%m/%d", time.gmtime())
    date = time.strftime("%Y-%m-%d", time.gmtime())
    entry = {"run_id": run_id, "date": date, "version": ver, "commit": commit, "machine": machine, **result}
    out = d / "runs" / day
    out.mkdir(parents=True, exist_ok=True)
    (out / f"{run_id}-{machine}.json").write_text(json.dumps(entry, indent=1) + "\n", encoding="utf-8")
    head = f"Mimic {ver} ({commit}) on `{machine}`, {date}, run [{run_id}](https://github.com/mimiccodes/mimic/actions/runs/{run_id}).\n\n"
    (out / f"{run_id}-{machine}.md").write_text(head + table, encoding="utf-8")
    (d / "history").mkdir(exist_ok=True)
    hist = d / "history" / f"{machine}.csv"
    fields = ["run_id", "date", "commit", "version", "machine", "benchmark", "c", "rust", "mimic", "rayon", "mimic_all"]
    new = not hist.exists() or hist.stat().st_size == 0
    with hist.open("a", newline="", encoding="utf-8") as f:
        w = csv.writer(f)
        if new:
            w.writerow(fields)
        for name, t in sorted(result.get("results", {}).items()):
            w.writerow([run_id, date, commit, ver, machine, name, *(f"{t[k]:.4f}" if k in t else "" for k in ("c", "rust", "mimic", "rayon", "mimic-all"))])
    if phase:
        p = d / "phases" / phase
        p.mkdir(parents=True, exist_ok=True)
        (p / f"{machine}.md").write_text(f"# Mimic {phase} on `{machine}`\n\n" + head + table, encoding="utf-8")
    print(f"recorded {len(result.get('results', {}))} benchmarks as {run_id}-{machine}" + (f", phase {phase}" if phase else ""))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
