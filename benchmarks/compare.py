#!/usr/bin/env python3
"""Fast, rigorous differential comparator (numpy-vectorized).

Cross-backend: exact IEEE-754 bit identity (uint64 equality) over the finite
domain. vs oracle: integer output exact where the true value <= 2**53, and
correctly-rounded beyond; relative error on continuous outputs. The oracle file
holds 50-digit decimal values; for continuous checks we use their
correctly-rounded double (ample for a ULP-scale error bound), and for the
integer check we compare integers exactly.
"""
import sys, json
import numpy as np

OUT = ["raw_workers","allocated_workers","total_ram_needed","payload_gbps","wire_rate_gbps"]
TWO53 = float(2**53)

def load_bits(path):
    rows = []
    with open(path) as f:
        for line in f:
            p = line.split()
            rows.append([int(x, 16) for x in p[1:6]])
    return np.array(rows, dtype=np.uint64)

def main():
    sp = sys.argv[1]
    B = {nm: load_bits(f"{sp}/out_{nm}.txt") for nm in ["c","cpp","python","rust"]}
    n = B["c"].shape[0]
    fval = {nm: B[nm].view(np.float64) for nm in B}         # reinterpret bits as doubles

    finite = np.ones(n, dtype=bool)
    for nm in B:
        finite &= np.isfinite(fval[nm]).all(axis=1)
    n_fin = int(finite.sum())

    ref = B["c"]
    cross_fin, cross_nonfin = {}, {}
    for nm in ["cpp","python","rust"]:
        diff = (B[nm] != ref).any(axis=1)
        cross_fin[nm] = int((diff & finite).sum())
        cross_nonfin[nm] = int((diff & ~finite).sum())

    # ---- oracle ----
    present = np.zeros(n, dtype=bool)
    ora_alloc = np.full(n, np.nan)       # correctly-rounded double of the exact integer
    ora_cont = np.full((n, 4), np.nan)   # raw,total,payload,wire as doubles
    # oracle columns: i, raw, alloc, total, payload, wire
    with open(f"{sp}/oracle.txt") as fh:
        for line in fh:
            p = line.split()
            if len(p) < 6 or p[1] == "skip": continue
            i = int(p[0]); present[i] = True
            ora_alloc[i] = float(p[2])
            ora_cont[i,0]=float(p[1]); ora_cont[i,1]=float(p[3])
            ora_cont[i,2]=float(p[4]); ora_cont[i,3]=float(p[5])

    c_alloc = fval["c"][:,1]
    c_cont = fval["c"][:, [0,2,3,4]]

    mask = present & finite
    within = mask & (np.abs(ora_alloc) <= TWO53)
    beyond = mask & (np.abs(ora_alloc) >  TWO53)
    int_exact = int((c_alloc[within] == ora_alloc[within]).sum()); int_total = int(within.sum())
    beyond_ok = int((c_alloc[beyond] == ora_alloc[beyond]).sum()); beyond_total = int(beyond.sum())

    with np.errstate(all="ignore"):
        rel = np.abs(c_cont - ora_cont) / np.abs(ora_cont)
        rel[ora_cont == 0] = np.where(c_cont[ora_cont == 0] == 0, 0.0, 1.0)
        rel_masked = rel[mask]
    max_rel = float(np.nanmax(rel_masked)) if rel_masked.size else 0.0
    # c_cont columns are [raw(0), total(2), payload(3), wire(4)]; total inherits the
    # ceil off-by-one, so report the PURELY arithmetic outputs (raw,payload,wire) too.
    pure_cols = [0, 2, 3]  # positions of raw, payload, wire within c_cont
    rel_pure = rel[:, pure_cols][mask]
    max_rel_pure = float(np.nanmax(rel_pure)) if rel_pure.size else 0.0
    # Same, restricted to results in the normal float64 range (subnormals lose precision by design)
    normal = np.abs(ora_cont[:, pure_cols]) >= np.finfo(np.float64).tiny
    rel_pure_normal = np.where(normal, rel[:, pure_cols], np.nan)[mask]
    max_rel_pure_normal = float(np.nanmax(rel_pure_normal)) if np.isfinite(rel_pure_normal).any() else 0.0
    # Battery layout (gen_and_oracle.py): 121 edge rows, then 10,000 ceil-boundary rows, then random
    BOUNDARY = range(121, 121 + 10000)
    off = np.flatnonzero(within & (c_alloc != ora_alloc))
    off_outside_boundary = [int(i) for i in off if i not in BOUNDARY]

    result = {
        "n_vectors": n, "finite_rows": n_fin, "nonfinite_rows": n - n_fin,
        "cross_backend_finite_mismatches": cross_fin,
        "cross_backend_nonfinite_bit_diffs": cross_nonfin,
        "oracle_integer_within_2^53": {"exact": int_exact, "total": int_total},
        "oracle_integer_beyond_2^53": {"correctly_rounded": beyond_ok, "total": beyond_total},
        "max_rel_err_continuous_finite": max_rel,
        "max_rel_err_pure_arithmetic": max_rel_pure,
        "max_rel_err_pure_arithmetic_normal_range": max_rel_pure_normal,
        "allocated_workers_off_by_ceil_snap": len(off),
        "allocated_workers_off_outside_boundary_sweep": off_outside_boundary,
    }
    print(json.dumps(result, indent=2))

    allident = all(v == 0 for v in cross_fin.values())
    print("\n================  PROOF  ================")
    print(f"Battery: {n:,} vectors | finite-output domain: {n_fin:,} | built-in Inf/NaN pathologies: {n-n_fin}")
    print(f"\n1) CROSS-BACKEND IEEE-754 BIT IDENTITY on the finite domain ({n_fin:,} rows):")
    for k in ["cpp","python","rust"]:
        print(f"     Aven-C vs {k:<6}: " + ("BIT-IDENTICAL on all "+f"{n_fin:,} rows" if cross_fin[k]==0 else f"{cross_fin[k]} mismatches"))
    print(f"   => {'ALL FOUR BACKENDS AGREE TO THE BIT.' if allident else 'DIVERGENCE on finite rows.'}")
    print(f"\n2) vs 50-digit decimal oracle (finite rows):")
    print(f"     allocated_workers, |true| <= 2^53 : {int_exact:,}/{int_total:,} EXACT")
    print(f"     allocated_workers, |true|  > 2^53 : {beyond_ok:,}/{beyond_total:,} correctly-rounded (float64 cannot hold the exact integer)")
    print(f"     allocated_workers differing from the oracle: {len(off):,}, of which outside the ceil-boundary sweep: {len(off_outside_boundary)}")
    print(f"     max rel error, pure arithmetic (raw/payload/wire), normal results: {max_rel_pure_normal:.3e}")
    print(f"     max rel error, pure arithmetic, including subnormal results: {max_rel_pure:.3e}")
    print(f"     max rel error, all continuous outputs incl. total_ram_needed (inherits ceil off-by-one): {max_rel:.3e}")
    print(f"\n3) Non-finite pathologies ({n-n_fin} rows): bit-diffs vs Aven-C = {cross_nonfin}")
    print("     (Inf-sign / NaN-payload artifacts on overflow & div-by-zero inputs; not computational error.)")

if __name__ == "__main__":
    main()
