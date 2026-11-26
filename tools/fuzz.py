#!/usr/bin/env python3
"""Differential testing with random programs (docs/versions.md, 0.4.3).

Writes random valid Mimic programs (whole numbers, lists of them, text made
from them; functions that call themselves with a list they grow; maps,
filters, reductions, takes, sorts, copies changed in place), runs each in
the ways a program can be built, and requires the same output of all:

  release, debug, release without the Mimic IR (MIMIC_IR=0), release on one
  thread, debug on 8 threads with one element per chunk.

No floats: a parallel sum may round differently from a sequential one, and
that is allowed. Every index is kept in range and every division is by a
nonzero constant, so a correct compiler runs every program to the end.

A program whose outputs disagree, that a build rejects, or that crashes is
kept in the output directory (default fuzz-found/) with each way's output:
a new fixture to add once the cause is fixed. Exit status 1 when any was
kept.

  tools/fuzz.py --mimic target/release/mimic --count 200 --seed 1
"""

import argparse
import os
import random
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

WAYS = [
    ("release", ["--release"], {}),
    ("debug", ["--debug"], {}),
    ("release-no-ir", ["--release"], {"MIMIC_IR": "0"}),
    ("release-1-thread", ["--release"], {"MIMIC_THREADS": "1"}),
    ("debug-8-threads", ["--debug"], {"MIMIC_THREADS": "8", "MIMIC_PAR_GRAIN": "1"}),
]


class Gen:
    """One random program. Variables are typed: 'i' (i64) or 'l' ([i64])."""

    def __init__(self, rng: random.Random):
        self.r = rng
        self.fns = []  # (name, kind, text)
        self.n = 0

    def fresh(self, p: str) -> str:
        self.n += 1
        return f"{p}{self.n}"

    # ---- expressions --------------------------------------------------------
    def small(self) -> str:
        return str(self.r.randint(0, 9))

    def int_expr(self, env: dict, d: int) -> str:
        ints = [v for v, t in env.items() if t == "i"]
        lists = [v for v, t in env.items() if t == "l"]
        c = self.r.randint(0, 12) if d > 0 else self.r.randint(0, 1)
        if c == 0 or (c == 1 and not ints):
            return self.small()
        if c == 1:
            return self.r.choice(ints)
        if c in (2, 3):
            op = self.r.choice(["+", "-", "×"])
            if op == "×":
                # (kept small: debug and release need not agree on overflow)
                return f"(({self.int_expr(env, d - 1)} % 1000) × ({self.int_expr(env, d - 1)} % 1000))"
            return f"({self.int_expr(env, d - 1)} {op} {self.int_expr(env, d - 1)})"
        if c == 4:
            k = self.r.randint(2, 97)
            return f"({self.int_expr(env, d - 1)} {self.r.choice(['%', 'div'])} {k})"
        if c == 5:
            return f"(if {self.bool_expr(env, d - 1)} {{ {self.int_expr(env, d - 1)} }} else {{ {self.int_expr(env, d - 1)} }})"
        if c == 6:
            return f"(≢ {self.list_expr(env, d - 1)})"
        if c == 7:
            return f"(+/ {self.list_expr(env, d - 1)})"
        if c == 8:
            # the largest of a list never empty
            return f"(⌈/ push({self.list_expr(env, d - 1)}, {self.int_expr(env, d - 1)}))"
        if c == 9 and lists:
            xs = self.r.choice(lists)
            return f"(if ≢ {xs} > 0 {{ {xs}[{self.int_expr(env, d - 1)} % ≢ {xs}] }} else {{ 0 }})"
        if c == 10 and self.fns:
            name, kind, arity = self.r.choice(self.fns)
            args = ", ".join(self.int_expr(env, d - 1) if a == "i" else self.list_expr(env, d - 1) for a in arity)
            return f"{name}({args})" if kind == "i" else f"(+/ {name}({args}))"
        # keep numbers from growing without end
        return f"({self.int_expr(env, d - 1)} % 1000)"

    def bool_expr(self, env: dict, d: int) -> str:
        c = self.r.randint(0, 3) if d > 0 else 0
        if c <= 1:
            op = self.r.choice(["==", "!=", "<", "<=", ">", ">="])
            return f"{self.int_expr(env, d - 1)} {op} {self.int_expr(env, d - 1)}"
        if c == 2:
            return f"({self.bool_expr(env, d - 1)} {self.r.choice(['and', 'or'])} {self.bool_expr(env, d - 1)})"
        return f"not ({self.bool_expr(env, d - 1)})"

    def list_expr(self, env: dict, d: int) -> str:
        lists = [v for v, t in env.items() if t == "l"]
        c = self.r.randint(0, 12) if d > 0 else self.r.randint(0, 1)
        if c == 0 or (c == 1 and not lists):
            return "[" + " ".join(self.small() for _ in range(self.r.randint(1, 6))) + "]"
        if c == 1:
            return self.r.choice(lists)
        if c == 2:
            return f"⍳ ({self.int_expr(env, d - 1)} % 40)"
        if c in (3, 4):
            x = self.fresh("x")
            inner = dict(env)
            inner[x] = "i"
            return f"({self.list_expr(env, d - 1)} ¨ ({x} => {self.int_expr(inner, d - 1)}))"
        if c == 5:
            return f"(⌽ {self.list_expr(env, d - 1)})"
        if c == 6:
            xs = self.list_expr(env, d - 1)
            return f"(({self.int_expr(env, d - 1)} % 8) ↑ {xs})"
        if c == 7:
            return f"push({self.list_expr(env, d - 1)}, {self.int_expr(env, d - 1)})"
        if c == 8:
            x = self.fresh("x")
            inner = dict(env)
            inner[x] = "i"
            if lists:
                xs = self.r.choice(lists)
                return f"({xs} ⌿ ({xs} ¨ ({x} => {self.bool_expr(inner, d - 1)})))"
            return self.list_expr(env, d - 1)
        if c == 9:
            return f"sort({self.list_expr(env, d - 1)})"
        if c == 10:
            return f"concat({self.list_expr(env, d - 1)}, {self.list_expr(env, d - 1)})"
        if c == 11:
            return f"({self.list_expr(env, d - 1)} + {self.int_expr(env, d - 1)})"
        if c == 12 and self.fns:
            lfns = [f for f in self.fns if f[1] == "l"]
            if lfns:
                name, _, arity = self.r.choice(lfns)
                args = ", ".join(self.int_expr(env, d - 1) if a == "i" else self.list_expr(env, d - 1) for a in arity)
                return f"{name}({args})"
        return f"({self.list_expr(env, d - 1)} % 1000)"

    # ---- statements -----------------------------------------------------------
    def block(self, env: dict, lines: int, indent: str, result: str) -> list:
        out = []
        env = dict(env)
        for _ in range(lines):
            k = self.r.randint(0, 5)
            if k <= 1:
                v = self.fresh("n")
                out.append(f"{indent}let {v} = {self.int_expr(env, 3)}")
                env[v] = "i"
            elif k <= 3:
                v = self.fresh("xs")
                out.append(f"{indent}let {v} = {self.list_expr(env, 3)}")
                env[v] = "l"
            elif k == 4:
                # a copy changed in place, the original read again after
                lists = [v for v, t in env.items() if t == "l"]
                if not lists:
                    continue
                src = self.r.choice(lists)
                v = self.fresh("m")
                out.append(f"{indent}let mut {v} = {src}")
                out.append(f"{indent}if ≢ {v} > 0 {{ {v}[{self.int_expr(env, 2)} % ≢ {v}] = {self.int_expr(env, 2)} }}")
                env[v] = "l"
            else:
                if result == "print":
                    out.append(f"{indent}print({self.any_expr(env)})")
        if result == "print":
            for v, t in env.items():
                if self.r.random() < 0.5:
                    out.append(f"{indent}print(\"{v} = {{{v}}}\")")
            return out
        tail = self.int_expr(env, 3) if result == "i" else self.list_expr(env, 3)
        out.append(f"{indent}{tail}")
        return out

    def any_expr(self, env: dict) -> str:
        return self.int_expr(env, 3) if self.r.random() < 0.5 else self.list_expr(env, 3)

    def function(self):
        """A function that calls itself last, carrying a list it grows (or a
        number), and one that does not call itself."""
        name = self.fresh("f")
        if self.r.random() < 0.6:
            acc = self.fresh("acc")
            env = {"k": "i", acc: "l"}
            step = f"({self.int_expr(env, 3)} % 1000)"
            grow = self.r.choice([
                f"push({acc}, {step})",
                f"(if ≢ {acc} > 0 {{ put({acc}, k % ≢ {acc}, {step}) }} else {{ push({acc}, {step}) }})",
                f"({acc} ¨ (y => (y + {step}) % 1000))",
            ])
            text = [
                f"fn {name}(k: i64, {acc}: [i64]) -> [i64] {{",
                f"    if k <= 0 or k > 60 {{ {acc} }} else {{ {name}(k - 1, {grow}) }}",
                "}",
            ]
            self.fns.append((name, "l", ["i", "l"]))
            return "\n".join(text), name
        env = {"a": "i", "b": "l"}
        kind = self.r.choice(["i", "l"])
        body = self.block(env, self.r.randint(0, 3), "    ", kind)
        text = [f"fn {name}(a: i64, b: [i64]) -> {'i64' if kind == 'i' else '[i64]'} {{"] + body + ["}"]
        self.fns.append((name, kind, ["i", "l"]))
        return "\n".join(text), name

    def program(self) -> str:
        parts = ["module main", ""]
        for _ in range(self.r.randint(0, 3)):
            text, _ = self.function()
            parts += [text, ""]
        parts.append("fn main() {")
        parts += self.block({}, self.r.randint(3, 10), "    ", "print")
        parts.append("}")
        return "\n".join(parts) + "\n"


def run(mimic: str, path: Path, args: list, env: dict, timeout: int):
    e = dict(os.environ)
    e.update(env)
    try:
        p = subprocess.run([mimic, "run", str(path)] + args, capture_output=True, text=True, env=e, timeout=timeout)
    except subprocess.TimeoutExpired:
        return None, "timed out"
    return p.returncode, p.stdout, p.stderr


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--mimic", default="target/release/mimic")
    ap.add_argument("--count", type=int, default=100)
    ap.add_argument("--seed", type=int, default=None)
    ap.add_argument("--out", default="fuzz-found")
    ap.add_argument("--timeout", type=int, default=120)
    ap.add_argument("--keep", action="store_true", help="keep every program written (in --out/all)")
    a = ap.parse_args()
    seed = a.seed if a.seed is not None else random.randrange(1 << 30)
    mimic = str(Path(a.mimic).resolve())
    out = Path(a.out)
    work = Path(tempfile.mkdtemp(prefix="mimic-fuzz-"))
    found = 0
    rejected = 0
    print(f"seed {seed}, {a.count} programs, {len(WAYS)} ways each")
    for i in range(a.count):
        rng = random.Random(seed * 1_000_003 + i)
        src = Gen(rng).program()
        d = work / f"p{i}"
        d.mkdir()
        f = d / "main.mim"
        f.write_text(src)
        if a.keep:
            (out / "all").mkdir(parents=True, exist_ok=True)
            shutil.copy(f, out / "all" / f"seed{seed}-{i}.mim")
        chk = subprocess.run([mimic, "check", str(f)], capture_output=True, text=True)
        if chk.returncode != 0:
            # the generator wrote something the language does not accept:
            # its own mistake, reported but not a finding
            rejected += 1
            print(f"  program {i}: not accepted by `mimic check` (a generator mistake)\n{chk.stderr[:600]}")
            continue
        results = []
        for name, args, env in WAYS:
            r = run(mimic, f, args, env, a.timeout)
            results.append((name, r))
        outs = {r[1] if r[0] == 0 else None for _, r in results if len(r) == 3}
        bad = any(len(r) != 3 or r[0] != 0 for _, r in results) or len(outs) != 1
        if bad:
            found += 1
            keep = out / f"seed{seed}-{i}"
            keep.mkdir(parents=True, exist_ok=True)
            shutil.copy(f, keep / "main.mim")
            for name, r in results:
                text = "timed out\n" if len(r) != 3 else f"exit {r[0]}\n--- stdout\n{r[1]}--- stderr\n{r[2]}"
                (keep / f"{name}.txt").write_text(text)
            print(f"  program {i}: the ways disagree or one failed: kept in {keep}")
            for name, r in results:
                print(f"    {name}: " + ("timed out" if len(r) != 3 else f"exit {r[0]}, {len(r[1].splitlines())} lines"))
        if (i + 1) % 25 == 0:
            print(f"  {i + 1} programs, {found} kept")
    shutil.rmtree(work, ignore_errors=True)
    print(f"done: {a.count} programs (seed {seed}), {found} kept, {rejected} not accepted")
    sys.exit(1 if found or rejected else 0)


if __name__ == "__main__":
    main()
