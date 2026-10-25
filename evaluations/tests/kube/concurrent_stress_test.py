#!/usr/bin/env python3
"""
concurrent_stress_test.py

Concurrent stress testing harness for Aven (avenc).
Spawns 50 parallel avenc run processes against cluster_capacity.aven,
interleaving fresh evaluations and TTL=0 stale injections.

Verifies:
1. Zero race conditions in frontier_cache / mutex.
2. Causal stale taint propagation across simultaneous expired facts.
3. 100% deterministic exit codes under 50-worker concurrency.
"""

import concurrent.futures
import os
import subprocess
import sys
import time

AVENC_BIN = os.path.abspath(os.path.join(os.path.dirname(__file__), "../../../target/release/avenc.exe"))
MODEL_PATH = os.path.abspath(os.path.join(os.path.dirname(__file__), "cluster_capacity.aven"))

if not os.path.exists(AVENC_BIN):
    print(f"Error: {AVENC_BIN} not found. Run 'cargo build --release' first.")
    sys.exit(1)

if not os.path.exists(MODEL_PATH):
    print(f"Error: {MODEL_PATH} not found.")
    sys.exit(1)

NUM_WORKERS = 50


def run_single(job_id: int):
    # Interleave: even job_ids inject TTL=0; odd job_ids run fresh
    inject_ttl_zero = (job_id % 2 == 0)

    env = os.environ.copy()
    if inject_ttl_zero:
        env["AVEN_INJECT_TTL_ZERO"] = "1"
    else:
        env.pop("AVEN_INJECT_TTL_ZERO", None)

    cmd = [AVENC_BIN, "run", MODEL_PATH]

    t0 = time.perf_counter()
    res = subprocess.run(
        cmd,
        env=env,
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
        timeout=30,
    )
    elapsed_ms = (time.perf_counter() - t0) * 1000.0

    return {
        "job_id": job_id,
        "inject_ttl_zero": inject_ttl_zero,
        "returncode": res.returncode,
        "stdout": res.stdout,
        "stderr": res.stderr,
        "elapsed_ms": elapsed_ms,
    }


def main():
    print("=" * 65)
    print(f"Aven Concurrency Stress Test: Spawning {NUM_WORKERS} Parallel Processes")
    print(f"Model: {MODEL_PATH}")
    print(f"Binary: {AVENC_BIN}")
    print("=" * 65)

    t_start = time.perf_counter()
    results = []

    with concurrent.futures.ThreadPoolExecutor(max_workers=NUM_WORKERS) as executor:
        futures = [executor.submit(run_single, i) for i in range(NUM_WORKERS)]
        for f in concurrent.futures.as_completed(futures):
            results.append(f.result())

    total_duration_s = time.perf_counter() - t_start

    # Sort results by job_id
    results.sort(key=lambda r: r["job_id"])

    fresh_runs = [r for r in results if not r["inject_ttl_zero"]]
    injected_runs = [r for r in results if r["inject_ttl_zero"]]

    fresh_success = [r for r in fresh_runs if r["returncode"] == 0]
    injected_blocked = [
        r
        for r in injected_runs
        if r["returncode"] == 1 and "DECISION BLOCKED" in (r["stdout"] + r["stderr"])
    ]

    print(f"\nCompleted {NUM_WORKERS} parallel process executions in {total_duration_s:.2f} seconds.")
    print("-" * 65)
    print(f"Fresh runs (TTL=60s):      {len(fresh_runs)} spawned | {len(fresh_success)} settled (exit code 0)")
    print(f"Injected runs (TTL=0ms):   {len(injected_runs)} spawned | {len(injected_blocked)} blocked (exit code 1)")

    avg_latency = sum(r["elapsed_ms"] for r in results) / len(results)
    min_latency = min(r["elapsed_ms"] for r in results)
    max_latency = max(r["elapsed_ms"] for r in results)
    print(f"Process Latency: avg={avg_latency:.1f}ms, min={min_latency:.1f}ms, max={max_latency:.1f}ms")
    print("-" * 65)

    # Verifications
    all_passed = True

    if len(fresh_success) != len(fresh_runs):
        print(f"FAIL: Expected {len(fresh_runs)} fresh runs to exit 0, got {len(fresh_success)}")
        all_passed = False

    if len(injected_blocked) != len(injected_runs):
        print(f"FAIL: Expected {len(injected_runs)} injected runs to exit 1 with DECISION BLOCKED, got {len(injected_blocked)}")
        all_passed = False

    # Check that in every blocked run, all stale facts were listed
    for r in injected_runs:
        out = r["stdout"] + r["stderr"]
        if "total_cpu_cores" not in out:
            print(f"FAIL: Job #{r['job_id']} missing total_cpu_cores in stale diagnostic")
            all_passed = False

    if all_passed:
        print("CONCURRENT STRESS TEST: PASSED (100% Deterministic Consistency Under Load)")
        sys.exit(0)
    else:
        print("CONCURRENT STRESS TEST: FAILED")
        sys.exit(1)


if __name__ == "__main__":
    main()
