#!/usr/bin/env python3
"""Python backend + baseline for the ProductionProfile model.

Reads battery.txt, computes the 5 outputs for every row, and writes out_python.txt
as raw IEEE-754 bits (16 hex per value) for exact cross-backend comparison.
Also reports timing for the vectorized (numpy) path and a pure-Python loop.

The ceil-snap replicates Aven exactly: nearest = round-half-away-from-zero, snap
to it when within 1e-9, else ceil.
"""
import sys, time, json
import numpy as np

def load(path):
    with open(path) as f:
        n, nin, nout = map(int, f.readline().split())
        a = np.empty((n, nin), dtype=np.float64)
        for i, line in enumerate(f):
            a[i] = np.fromstring(line, sep=' ')
    return a, nout

def aven_ceil(x):
    # The snapped integer is only USED when x is within 1e-9 of an integer, where
    # there is no halfway ambiguity, so np.round (round-half-even) is safe here and
    # avoids the floor(|x|+0.5) idiom's large-magnitude rounding bug.
    nearest = np.round(x)
    snapped = np.where(np.abs(x - nearest) < 1e-9, nearest, np.ceil(x))
    return snapped

def solve_vec(a):
    lat, pay, fr, osk, rpw, reqs, sock, util = [a[:, i] for i in range(8)]
    raw = (reqs * lat) / 1000.0
    alloc = aven_ceil(raw / util)
    total = (alloc * (rpw + sock)) + osk
    payload = ((reqs * pay) * 8.0) / 1000000.0
    wire = payload * fr
    return np.stack([raw, alloc, total, payload, wire], axis=1)

def solve_scalar(row):
    lat, pay, fr, osk, rpw, reqs, sock, util = row
    import math
    raw = (reqs * lat) / 1000.0
    r = raw / util if util != 0 else math.inf
    if not math.isfinite(r):
        alloc = r                      # ceil(inf)=inf, ceil(nan)=nan
    else:
        nearest = math.copysign(math.floor(abs(r) + 0.5), r)
        alloc = nearest if abs(r - nearest) < 1e-9 else math.ceil(r)
    total = (alloc * (rpw + sock)) + osk
    payload = ((reqs * pay) * 8.0) / 1000000.0
    wire = payload * fr
    return (raw, alloc, total, payload, wire)

def main():
    sp = sys.argv[1]
    a, nout = load(f"{sp}/battery.txt")
    n = a.shape[0]
    np.seterr(all="ignore")   # edge cases deliberately overflow to Inf/NaN

    # correctness output (vectorized)
    out = solve_vec(a)
    bits = out.view(np.uint64)
    with open(f"{sp}/out_python.txt", "w") as f:
        for i in range(n):
            f.write(f"{i} " + " ".join("%016x" % b for b in bits[i]) + "\n")

    # timing: vectorized throughput (repeat to accumulate work)
    K = 50
    t0 = time.perf_counter()
    sink = 0.0
    for _ in range(K):
        o = solve_vec(a)
        sink += o[0, 1]
    t1 = time.perf_counter()
    numpy_ns = (t1 - t0) / (n * K) * 1e9

    # timing: pure-Python loop (slow baseline) over a subset
    sub = min(n, 50000)
    t0 = time.perf_counter()
    acc = 0.0
    for i in range(sub):
        acc += solve_scalar(a[i])[1]
    t1 = time.perf_counter()
    pure_ns = (t1 - t0) / sub * 1e9

    print(json.dumps({
        "backend": "python",
        "n": n,
        "numpy_ns_per_solve": round(numpy_ns, 2),
        "numpy_solves_per_sec": round(n * K / (numpy_ns * n * K / 1e9)) if numpy_ns else None,
        "pure_ns_per_solve": round(pure_ns, 1),
        "pure_solves_per_sec": round(1e9 / pure_ns),
        "_sink": sink + acc,
    }))

if __name__ == "__main__":
    main()
