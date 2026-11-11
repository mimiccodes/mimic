#!/usr/bin/env python3
"""Check (or refresh) the tutorial: every ```mimic run``` block is compiled and
run with the current `mimic`, and its stdout must equal the ```output``` block
that follows it. Prose is also checked for characters MDX would treat as code
(`{`, `}`, `<` outside backticks).

    tools/check-tutorial.py            # check; exit 1 on any mismatch
    tools/check-tutorial.py --update   # rewrite the output blocks from real runs
    tools/check-tutorial.py --mimic path/to/mimic
"""
import argparse
import pathlib
import re
import subprocess
import sys
import tempfile

ROOT = pathlib.Path(__file__).resolve().parent.parent
BLOCK = re.compile(r"```mimic run\n(.*?)```\n\n```output\n(.*?)```", re.S)


def prose_problems(text):
    """MDX parses `{…}` as expressions and `<x` as JSX outside code."""
    out = []
    in_fence = False
    for n, line in enumerate(text.splitlines(), 1):
        if line.startswith("```"):
            in_fence = not in_fence
            continue
        if in_fence or line.startswith("---") or line.startswith("import "):
            continue
        bare = re.sub(r"`[^`]*`", "", line)
        if re.search(r"[{}]|<(?=[A-Za-z/!])", bare):
            out.append(f"line {n}: `{{`, `}}` or `<` outside backticks: {line.strip()}")
    return out


def run(mimic, src):
    with tempfile.TemporaryDirectory() as d:
        f = pathlib.Path(d) / "example.mim"
        f.write_text(src)
        r = subprocess.run([mimic, "run", str(f)], capture_output=True, text=True, timeout=300)
        return r.returncode, r.stdout, r.stderr


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--update", action="store_true")
    ap.add_argument("--mimic", default=str(ROOT / "target" / "debug" / "mimic"))
    ap.add_argument("files", nargs="*")
    a = ap.parse_args()
    files = [pathlib.Path(f) for f in a.files] or sorted((ROOT / "tutorial").glob("*.mdx"))
    failures = 0
    total = 0
    for path in files:
        text = path.read_text()
        for p in prose_problems(text):
            print(f"{path.name}: {p}")
            failures += 1
        new = text
        for m in BLOCK.finditer(text):
            total += 1
            src, want = m.group(1), m.group(2)
            code, got, err = run(a.mimic, src)
            if code != 0:
                print(f"{path.name}: example failed (exit {code}):\n{src}\n{err}")
                failures += 1
                continue
            if a.update:
                new = new.replace(m.group(0), f"```mimic run\n{src}```\n\n```output\n{got}```", 1)
            elif got != want:
                print(f"{path.name}: output differs for:\n{src}--- expected\n{want}--- got\n{got}")
                failures += 1
        if a.update and new != text:
            path.write_text(new)
            print(f"updated {path.name}")
    print(f"{total} examples in {len(files)} files, {failures} problem(s)")
    sys.exit(1 if failures else 0)


if __name__ == "__main__":
    main()
