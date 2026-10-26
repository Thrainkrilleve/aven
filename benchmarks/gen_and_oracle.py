#!/usr/bin/env python3
"""Generate the brutal input battery and a high-precision (decimal) oracle for
the server_capacity / ProductionProfile model.

All four backends (Aven-C, C++, Python, Rust evaluator) read battery.txt so they
run byte-identical inputs. oracle.txt holds the arbitrary-precision reference
computed with Python's `decimal` (50 digits), implementing Aven's exact
semantics -- including the ceil() 1e-9 snap used by both the Rust evaluator
(diff::eval_expr) and the generated C (aven_ceil).

Canonical input order (avenc sorts free inputs alphabetically):
  0 avg_latency_ms  1 avg_payload_kb  2 framing_overhead_ratio
  3 os_kernel_ram_mb 4 ram_per_worker 5 reqs_per_second
  6 socket_buffer_mb 7 target_utilization
Output order (forward-block equation order):
  0 raw_workers 1 allocated_workers 2 total_ram_needed 3 payload_gbps 4 wire_rate_gbps

Numbers are written with %.17g, which round-trips IEEE-754 double exactly in C
strtod, Rust parse::<f64>, and Python float(), so every backend reconstructs the
same bits.
"""
import sys, os, struct, random
from decimal import Decimal, getcontext, ROUND_HALF_UP

getcontext().prec = 50
N_IN, N_OUT = 8, 5
DEC_1E9 = Decimal(1) / Decimal(10**9)   # the ceil-snap threshold, exact

def fmt(x: float) -> str:
    return "%.17g" % x

# ---- edge / pathology cases (hand-built) -------------------------------------
def edge_cases():
    base = [20.0, 64.0, 1.04, 4096.0, 16.0, 50000.0, 4.0, 0.70]
    rows = [list(base)]
    specials = [0.0, -0.0, 1.0, -1.0, 1e-300, 5e-324, 1e-9, 1e9, 1e18,
                1.7976931348623157e308, 2.2250738585072014e-308,
                float('inf'), float('-inf'), float('nan')]
    # sweep each slot through the specials, holding the rest nominal
    for slot in range(N_IN):
        for s in specials:
            r = list(base); r[slot] = s; rows.append(r)
    # div-by-zero / tiny utilization family
    for u in [0.0, 1e-300, 5e-324, 1e-18, 1e-9]:
        r = list(base); r[7] = u; rows.append(r)
    # overflow family: huge reqs * payload
    for q in [1e200, 1e300, 1.7976931348623157e308]:
        r = list(base); r[5] = q; r[1] = 1e100; rows.append(r)
    return rows

# ---- ceil-boundary sweep -----------------------------------------------------
# Build inputs so raw_workers/target_utilization lands a controlled distance
# from an integer, stressing the 1e-9 snap from both sides.
def boundary_cases(n=10000, seed=1):
    rng = random.Random(seed)
    rows = []
    offs = [0.0, 1e-12, 1e-10, 1e-9, 2e-9, 1e-8, -1e-12, -1e-10, -1e-9, -2e-9, -1e-8]
    for _ in range(n):
        k = rng.randint(1, 10_000_000)          # target integer for the ratio
        util = rng.uniform(0.05, 0.99)
        off = rng.choice(offs)
        raw = (k + off) * util                   # so raw/util ~= k+off
        lat = rng.uniform(1.0, 100.0)
        reqs = raw * 1000.0 / lat                 # so (reqs*lat)/1000 == raw
        rows.append([lat, rng.uniform(1,128), rng.uniform(1.0,1.1),
                     rng.uniform(0,8192), rng.uniform(1,64), reqs,
                     rng.uniform(0,16), util])
    return rows

# ---- wide-range random -------------------------------------------------------
def random_cases(n=250000, seed=12345):
    rng = random.Random(seed)
    rows = []
    def logu(a, b):
        import math
        return math.exp(rng.uniform(math.log(a), math.log(b)))
    for _ in range(n):
        rows.append([
            logu(1e-3, 1e4),     # avg_latency_ms
            logu(1e-3, 1e6),     # avg_payload_kb
            rng.uniform(1.0, 1.2),
            logu(1.0, 1e7),      # os_kernel_ram_mb
            logu(1.0, 1e4),      # ram_per_worker
            logu(1.0, 1e9),      # reqs_per_second
            logu(1e-3, 1e3),     # socket_buffer_mb
            rng.uniform(1e-6, 1.0),  # target_utilization
        ])
    return rows

# ---- oracle (decimal, Aven semantics) ----------------------------------------
def oracle_row(v):
    # mirror emit-c parenthesization exactly, in Decimal
    lat, pay, fr, osk, rpw, reqs, sock, util = [Decimal(repr(x)) for x in v]
    nan = any(x != x for x in v)
    inf = any(x in (float('inf'), float('-inf')) for x in v)
    if nan or inf:
        return None  # non-finite inputs: compared separately (expect HOLD/NaN propagation)
    raw = (reqs * lat) / Decimal(1000)
    if util == 0:
        return None
    ratio = raw / util
    nearest = ratio.to_integral_value(rounding=ROUND_HALF_UP)
    if abs(ratio - nearest) < DEC_1E9:
        alloc = nearest
    else:
        alloc = ratio.to_integral_value(rounding="ROUND_CEILING")
    total = (alloc * (rpw + sock)) + osk
    payload = ((reqs * pay) * Decimal(8)) / Decimal(1000000)
    wire = payload * fr
    return [raw, alloc, total, payload, wire]

def main():
    out_dir = sys.argv[1] if len(sys.argv) > 1 else "."
    os.makedirs(out_dir, exist_ok=True)
    rows = edge_cases() + boundary_cases() + random_cases()
    n = len(rows)
    with open(os.path.join(out_dir, "battery.txt"), "w") as bf:
        bf.write(f"{n} {N_IN} {N_OUT}\n")
        for r in rows:
            bf.write(" ".join(fmt(x) for x in r) + "\n")
    # oracle: finite/defined rows only; index map lets comparators align
    n_ok = 0
    with open(os.path.join(out_dir, "oracle.txt"), "w") as of:
        for i, r in enumerate(rows):
            o = oracle_row(r)
            if o is None:
                of.write(f"{i} skip\n")
            else:
                of.write(f"{i} " + " ".join(format(x, 'f') for x in o) + "\n")
                n_ok += 1
    print(f"battery: {n} vectors ({len(edge_cases())} edge, {10000} boundary, {250000} random)")
    print(f"oracle:  {n_ok} finite/defined rows (decimal prec={getcontext().prec})")

if __name__ == "__main__":
    main()
