#!/usr/bin/env python3
"""highs_proof.py -- reproducible differential-verification campaign of avenc's
optimizer against HiGHS (via scipy.optimize) and the other reference solvers in
oracle.py, aggregated across seeds into one summary.

This formalizes the HiGHS check as a first-class part of the proof suite: it runs
every problem family over several seeds and reports, per family, how many of
avenc's answers agree with the independent reference solver.

Run (needs the .venv that has scipy + highspy):
    .venv/Scripts/python verify/highs_proof.py --cases 100 --seeds 5
"""
import argparse, json, re, subprocess, sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
ORACLE = ROOT / "verify" / "oracle.py"
LINE = re.compile(r"^\s*(\w+)\s+(\d+)/(\d+)\s+agree")

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--cases", type=int, default=100, help="cases per family per seed")
    ap.add_argument("--seeds", type=int, default=5, help="number of seeds (1..N)")
    args = ap.parse_args()

    totals = {}   # family -> [pass, total]
    for seed in range(1, args.seeds + 1):
        out = subprocess.run(
            [sys.executable, str(ORACLE), "all", "--cases", str(args.cases), "--seed", str(seed)],
            capture_output=True, text=True, cwd=str(ROOT),
        )
        for line in (out.stdout + out.stderr).splitlines():
            m = LINE.match(line)
            if m:
                fam, p, t = m.group(1), int(m.group(2)), int(m.group(3))
                totals.setdefault(fam, [0, 0])
                totals[fam][0] += p
                totals[fam][1] += t
        print(f"seed {seed} done", file=sys.stderr)

    grand_p = sum(v[0] for v in totals.values())
    grand_t = sum(v[1] for v in totals.values())
    print("\n==========  avenc vs HiGHS / reference solvers  ==========")
    print(f"{'family':<12}{'agree':>10}{'total':>10}{'rate':>9}")
    for fam in sorted(totals):
        p, t = totals[fam]
        print(f"{fam:<12}{p:>10}{t:>10}{(100.0*p/t if t else 0):>8.1f}%")
    print("-" * 41)
    print(f"{'ALL':<12}{grand_p:>10}{grand_t:>10}{(100.0*grand_p/grand_t if grand_t else 0):>8.1f}%")
    print(json.dumps({"by_family": totals, "total_agree": grand_p, "total": grand_t}))

if __name__ == "__main__":
    main()
