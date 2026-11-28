#!/usr/bin/env python3
"""The size of every benchmark's release executable (and of hello world).

Builds each benchmark in bench/ with --release and prints its executable's
size in bytes, sorted, with the total and the median: how the binaries Mimic
makes are kept small (docs/gaps.md, binary size).

    python3 tools/sizes.py --mimic target/release/mimic [--out sizes.tsv] [names…]
"""

import argparse
import json
import os
import statistics
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent


def size(mimic: str, src: Path) -> int | None:
    r = subprocess.run([mimic, "build", "--release", str(src), "--emit-json"], capture_output=True, text=True)
    try:
        return os.path.getsize(json.loads(r.stdout)["artifacts"][0]["path"])
    except (ValueError, KeyError, IndexError, OSError):
        return None


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--mimic", required=True)
    ap.add_argument("--out")
    ap.add_argument("names", nargs="*")
    a = ap.parse_args()
    names = a.names or sorted(p.name for p in (ROOT / "bench").iterdir() if (p / "main.mim").exists())
    rows = []
    with tempfile.TemporaryDirectory() as d:
        hello = Path(d) / "hello.mim"
        hello.write_text('module main\nfn main() { print("hello") }\n')
        rows.append(("(hello world)", size(a.mimic, hello)))
    for n in names:
        rows.append((n, size(a.mimic, ROOT / "bench" / n / "main.mim")))
    ok = [s for _, s in rows if s is not None]
    rows.sort(key=lambda r: -(r[1] or 0))
    lines = [f"{n}\t{s if s is not None else 'build failed'}" for n, s in rows]
    summary = f"TOTAL\t{sum(ok)}\nMEDIAN\t{int(statistics.median(ok)) if ok else 0}"
    print("\n".join(lines + [summary]))
    if a.out:
        Path(a.out).write_text("name\tbytes\n" + "\n".join(lines + [summary]) + "\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
