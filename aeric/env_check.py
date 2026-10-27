#!/usr/bin/env python3
"""Independent checker for test_envelope: clamp + rate-limit + fault taxonomy.

Reproduces the clamp([0,1000]) + rate-limit(50/tick) recurrence from the fail-safe
seed (1), confirms the published allocated_workers matches exactly and that it
ramps then plateaus at the ceiling, that `clamped` stays set (1429 > 1000), and
that each fault class reports the right reason name.

Usage: env_check.py <path-to-test_envelope.exe>
"""
import sys, subprocess

SOLVED, LO, HI, RATE, SEED = 1429.0, 0.0, 1000.0, 50.0, 1.0

def main():
    out = subprocess.run([sys.argv[1]], capture_output=True, text=True).stdout
    ticks, faults, counts = [], {}, {}
    for line in out.splitlines():
        if line.startswith("tick="):
            d = dict(kv.split("=") for kv in line.split())
            ticks.append((int(d["tick"]), int(d["clamped"]), float(d["alloc"])))
        elif line.startswith("fault="):
            d = dict(kv.split("=") for kv in line.split())
            faults[d["fault"]] = d["name"]
        elif line.startswith("counts "):
            counts = dict(kv.split("=") for kv in line.split()[1:])

    prev, ok, clamp_ok = SEED, True, True
    for tick, clamped, alloc in ticks:
        v = min(max(SOLVED, LO), HI)          # clamp
        d = v - prev
        if d >  RATE: v = prev + RATE         # rate-limit
        if d < -RATE: v = prev - RATE
        if alloc != v: ok = False; print(f"  tick{tick}: alloc={alloc} expected {v}")
        if clamped != 1: clamp_ok = False     # 1429>1000 -> always clamped
        prev = v

    allocs = [a for _,_,a in ticks]
    ramps = allocs[0] < allocs[5] and abs(allocs[-1]-HI) < 1e-9 and allocs[-1] == allocs[-2]
    print(f"clamp+rate matches reference exactly: {ok}")
    print(f"ramps from safe then plateaus at ceiling {HI:.0f}: {ramps}  "
          f"(first={allocs[0]:.0f} last={allocs[-1]:.0f})")
    print(f"clamped flag set on every clamped tick: {clamp_ok}")
    tax = (faults.get("sensor")=="sensor" and faults.get("solver")=="solver" and
           faults.get("deadline")=="deadline" and faults.get("transport")=="transport")
    print(f"fault taxonomy names correct: {tax}  {faults}")
    cnt = (int(counts.get("sensor",0))>=1 and int(counts.get("solver",0))>=1 and
           int(counts.get("deadline",0))>=1 and int(counts.get("transport",0))>=1 and
           int(counts.get("clamp",0))>=25)
    print(f"per-class fault counts + clamp events: {cnt}  {counts}")
    return 0 if (ok and ramps and clamp_ok and tax and cnt) else 1

if __name__ == "__main__":
    sys.exit(main())
