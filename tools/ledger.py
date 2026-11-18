#!/usr/bin/env python3
"""Mimic's CI spending ledger (the private repo mimiccodes/mimic-ledger).

    tools/ledger.py DIR run DATE RUN WORKFLOW PURPOSE INSTANCE TYPE LIFECYCLE MINUTES ESTIMATE
    tools/ledger.py DIR billing COSTS.json      # AWS Cost Explorer, daily by service
    tools/ledger.py DIR render                  # rewrite the month sheets

DIR is a checkout of the ledger. `run` adds one machine (from the
aws-runner workflow, when it ends one); `billing` replaces the billed
amounts of the days in a Cost Explorer answer; both rewrite the month's
balance sheet (YYYY-MM.md): machines, minutes, estimated and billed cost,
and what is left of the budget.
"""

import csv
import json
import sys
from collections import defaultdict
from pathlib import Path

BUDGET = 100.0
RUNS = "ledger.csv"
BILLS = "billing.csv"
RUN_FIELDS = ["date", "run_id", "workflow", "purpose", "instance", "type", "lifecycle", "minutes", "estimated_usd"]
BILL_FIELDS = ["date", "service", "usd"]


def read(path: Path, fields):
    if not path.exists():
        return []
    with path.open(newline="", encoding="utf-8") as f:
        return [{k: r.get(k, "") for k in fields} for r in csv.DictReader(f)]


def write(path: Path, fields, rows):
    with path.open("w", newline="", encoding="utf-8") as f:
        w = csv.DictWriter(f, fieldnames=fields)
        w.writeheader()
        w.writerows(rows)


def render(d: Path):
    runs = read(d / RUNS, RUN_FIELDS)
    bills = read(d / BILLS, BILL_FIELDS)
    months = sorted({r["date"][:7] for r in runs} | {b["date"][:7] for b in bills})
    for month in months:
        mr = [r for r in runs if r["date"].startswith(month)]
        mb = [b for b in bills if b["date"].startswith(month)]
        est = sum(float(r["estimated_usd"] or 0) for r in mr)
        mins = sum(int(float(r["minutes"] or 0)) for r in mr)
        billed = sum(float(b["usd"] or 0) for b in mb)
        by_type = defaultdict(lambda: [0, 0, 0.0])
        by_purpose = defaultdict(lambda: [0, 0, 0.0])
        for r in mr:
            for key, table in ((f'{r["type"]} ({r["lifecycle"] or "on-demand"})', by_type), (r["purpose"] or r["workflow"], by_purpose)):
                table[key][0] += 1
                table[key][1] += int(float(r["minutes"] or 0))
                table[key][2] += float(r["estimated_usd"] or 0)
        by_service = defaultdict(float)
        for b in mb:
            by_service[b["service"]] += float(b["usd"] or 0)
        lines = [
            f"# {month}",
            "",
            f"Budget: ${BUDGET:.0f} (AWS budget `mimic-monthly`). Billed so far: **${billed:.2f}**, left: **${BUDGET - billed:.2f}**.",
            "",
            "| | Machines | Minutes | Estimated | Billed |",
            "|---|---|---|---|---|",
            f"| Total | {len(mr)} | {mins} | ${est:.2f} | ${billed:.2f} |",
            "",
            "Estimated: each machine's minutes at its price (spot as AWS charged it then); billed: what AWS reports, about a day later, for the whole account.",
            "",
            "## By machine type",
            "",
            "| Type | Machines | Minutes | Estimated |",
            "|---|---|---|---|",
        ]
        lines += [f"| {k} | {v[0]} | {v[1]} | ${v[2]:.2f} |" for k, v in sorted(by_type.items())] or ["| — | | | |"]
        lines += ["", "## By purpose", "", "| Purpose | Machines | Minutes | Estimated |", "|---|---|---|---|"]
        lines += [f"| {k} | {v[0]} | {v[1]} | ${v[2]:.2f} |" for k, v in sorted(by_purpose.items())] or ["| — | | | |"]
        lines += ["", "## Billed by AWS service", "", "| Service | Billed |", "|---|---|"]
        lines += [f"| {k} | ${v:.2f} |" for k, v in sorted(by_service.items(), key=lambda kv: -kv[1]) if v >= 0.005] or ["| — | $0.00 |"]
        lines += ["", "## Machines", "", "| Date | Run | Purpose | Type | Minutes | Estimated |", "|---|---|---|---|---|---|"]
        lines += [f'| {r["date"]} | [{r["run_id"]}](https://github.com/mimiccodes/mimic/actions/runs/{r["run_id"]}) | {r["purpose"] or r["workflow"]} | {r["type"]} ({r["lifecycle"] or "on-demand"}) | {r["minutes"]} | ${float(r["estimated_usd"] or 0):.3f} |' for r in mr] or ["| — | | | | | |"]
        (d / f"{month}.md").write_text("\n".join(lines) + "\n", encoding="utf-8")


def main(argv) -> int:
    if len(argv) < 3:
        print(__doc__)
        return 2
    d, cmd = Path(argv[1]), argv[2]
    if cmd == "run":
        rows = read(d / RUNS, RUN_FIELDS)
        rows.append(dict(zip(RUN_FIELDS, argv[3:12])))
        write(d / RUNS, RUN_FIELDS, rows)
    elif cmd == "billing":
        answer = json.loads(Path(argv[3]).read_text(encoding="utf-8"))
        days = {}
        for r in answer.get("ResultsByTime", []):
            day = r["TimePeriod"]["Start"]
            days[day] = [{"date": day, "service": g["Keys"][0], "usd": f'{float(g["Metrics"]["UnblendedCost"]["Amount"]):.4f}'} for g in r.get("Groups", [])]
        rows = [b for b in read(d / BILLS, BILL_FIELDS) if b["date"] not in days]
        for day in sorted(days):
            rows += days[day]
        write(d / BILLS, BILL_FIELDS, sorted(rows, key=lambda b: (b["date"], b["service"])))
    elif cmd != "render":
        print(f"unknown command {cmd}", file=sys.stderr)
        return 2
    render(d)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
