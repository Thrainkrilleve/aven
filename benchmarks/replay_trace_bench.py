#!/usr/bin/env python3
"""
benchmarks/replay_trace_bench.py
--------------------------------
Full Real Production Web Traffic Trace Benchmark Harness.
Replays the 40-step Wikimedia traffic trace through Aven's Little's Law capacity engine.
Evaluates:
  1. Dynamic capacity scaling under non-linear queueing curves (8.2k -> 98.2k req/s).
  2. Sub-millisecond solver throughput & process latency.
  3. Hard freshness enforcement: confirms automatic DECISION BLOCKED on step 25 (TTL=0).
"""

import json
import os
import subprocess
import sys
import time
from pathlib import Path

# Ensure UTF-8 output on Windows consoles
if hasattr(sys.stdout, "reconfigure"):
    try:
        sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    except Exception:
        pass


def main():
    root = Path(__file__).resolve().parent.parent
    exe = "avenc.exe" if os.name == "nt" else "avenc"
    avenc_exe = root / "target" / "debug" / exe
    if not avenc_exe.exists():
        avenc_exe = root / "target" / "release" / exe
    if not avenc_exe.exists():
        print(f"Error: avenc executable not found at {avenc_exe}")
        sys.exit(1)

    trace_file = root / "evaluations" / "data" / "wikimedia_traffic_trace.json"
    with open(trace_file, "r", encoding="utf-8") as f:
        trace = json.load(f)

    model_file = root / "evaluations" / "traces" / "traffic_replay.aven"

    print("=" * 78)
    print(" Aven Production Traffic Trace Benchmark — Little's Law Scaling Engine")
    print(f" Dataset: {trace_file.name} (40 discrete production time-series steps)")
    print(f" Target Model: {model_file.name}")
    print("=" * 78)

    results = []
    latencies_ms = []

    for item in trace:
        step = item["step"]
        expected_ttl = item.get("ttl_ms", 30000)

        env = os.environ.copy()
        env["AVEN_TRACE_STEP"] = str(step)
        env["AVEN_WORKSPACE"] = str(root)

        t0 = time.perf_counter()
        proc = subprocess.run(
            [str(avenc_exe), "run", str(model_file)],
            env=env,
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            cwd=str(root)
        )
        elapsed_ms = (time.perf_counter() - t0) * 1000.0
        latencies_ms.append(elapsed_ms)

        combined = proc.stdout + proc.stderr
        # Exit 3 is avenc's freshness block; any other non-zero exit is a real failure
        is_blocked = proc.returncode == 3 and "DECISION BLOCKED" in combined
        failed = proc.returncode != 0 and not is_blocked

        # Extract target pods and wire rate from output
        target_pods = None
        allocated_workers = None
        wire_rate_gbps = None

        for line in combined.splitlines():
            line = line.strip()
            if line.startswith("target_pods"):
                target_pods = float(line.split("=")[-1].strip())
            elif line.startswith("allocated_workers"):
                allocated_workers = float(line.split("=")[-1].strip())
            elif line.startswith("wire_rate_gbps"):
                wire_rate_gbps = float(line.split("=")[-1].strip())

        res_entry = {
            "step": step,
            "time_offset_s": item["time_offset_s"],
            "reqs_per_second": item["reqs_per_second"],
            "avg_latency_ms": item["avg_latency_ms"],
            "expected_ttl": expected_ttl,
            "exit_code": proc.returncode,
            "is_blocked": is_blocked,
            "failed": failed,
            "target_pods": target_pods,
            "allocated_workers": allocated_workers,
            "wire_rate_gbps": wire_rate_gbps,
            "elapsed_ms": elapsed_ms
        }
        results.append(res_entry)

    # Print Formatted Table
    print("\nStep | Time (s) | Reqs/Sec | Latency | Target Pods | Workers | Wire Gbps | Status")
    print("-" * 78)
    for r in results:
        t_s = r["time_offset_s"]
        rps = r["reqs_per_second"]
        lat = r["avg_latency_ms"]
        pods = f"{r['target_pods']:.0f}" if r["target_pods"] is not None else "BLOCKED"
        wrk = f"{r['allocated_workers']:.0f}" if r["allocated_workers"] is not None else "HOLD"
        wire = f"{r['wire_rate_gbps']:.2f}" if r["wire_rate_gbps"] is not None else "—"
        if r["is_blocked"]:
            status = "❌ DECISION BLOCKED (stale input)"
        elif r["failed"]:
            status = f"✗ FAILED (exit {r['exit_code']})"
        else:
            status = f"✓ Sized ({r['elapsed_ms']:.1f}ms)"
        print(f"{r['step']:4d} | {t_s:8d} | {rps:8.0f} | {lat:5.1f}ms | {pods:>11s} | {wrk:>7s} | {wire:>9s} | {status}")

    # ASCII Traffic & Sizing Curve Visualization
    print("\n" + "=" * 78)
    print(" Workload Traffic Surge vs. Autoscaled Target Pods (ASCII Trace Curve)")
    print("=" * 78)
    max_rps = max(r["reqs_per_second"] for r in results)
    for r in results:
        step = r["step"]
        rps = r["reqs_per_second"]
        bar_len = int((rps / max_rps) * 36)
        bar = "█" * bar_len + "░" * (36 - bar_len)
        if r["is_blocked"]:
            annotation = "[STALE LOCK]"
        else:
            annotation = f"{r['target_pods']:.0f} pods ({r['allocated_workers']:.0f} w)"
        print(f"t={r['time_offset_s']:4d}s | {bar} | {rps:6.0f} rps -> {annotation}")

    # Verification Checks
    blocked_count = sum(1 for r in results if r["is_blocked"])
    actionable_count = sum(1 for r in results if not r["is_blocked"] and r["exit_code"] == 0)

    print("\n" + "=" * 78)
    print(" Production Trace Benchmark Summary")
    print("=" * 78)
    print(f"Total Steps Evaluated:        {len(results)}")
    stale_steps = [r["step"] for r in results if r["expected_ttl"] == 0]
    blocked_steps = [r["step"] for r in results if r["is_blocked"]]
    failed_steps = [r["step"] for r in results if r["failed"]]
    fresh_total = len(results) - len(stale_steps)
    print(f"Actionable Solutions Sized:   {actionable_count} / {fresh_total} fresh steps")
    print(f"Stale Steps Blocked:          {blocked_count} / {len(stale_steps)} (blocked steps {blocked_steps}, stale in trace {stale_steps})")
    print(f"Failed Steps:                 {len(failed_steps)} {failed_steps}")
    print(f"Traffic Range:                {min(r['reqs_per_second'] for r in results):.0f} to {max(r['reqs_per_second'] for r in results):.0f} req/s")
    valid_pods = [r["target_pods"] for r in results if r["target_pods"] is not None]
    print(f"Target Pod Range:             {min(valid_pods):.0f} pods to {max(valid_pods):.0f} pods")
    print(f"Mean Run Time (incl shell):   {sum(latencies_ms)/len(latencies_ms):.2f} ms")
    latencies_sorted = sorted(latencies_ms)
    p50 = latencies_sorted[len(latencies_sorted) // 2]
    p95 = latencies_sorted[int(len(latencies_sorted) * 0.95)]
    print(f"Median Execution Latency:     {p50:.2f} ms")
    print(f"95th Percentile Latency:      {p95:.2f} ms")

    # Export benchmark results
    out_file = root / "benchmarks" / "trace_benchmark_results.json"
    with open(out_file, "w", encoding="utf-8") as f:
        json.dump({
            "dataset": "wikimedia_traffic_trace.json",
            "total_steps": len(results),
            "actionable_count": actionable_count,
            "blocked_count": blocked_count,
            "failed_count": len(failed_steps),
            "avenc_version": subprocess.run([str(avenc_exe), "--version"], capture_output=True, text=True).stdout.strip(),
            "platform": sys.platform,
            "min_reqs_per_second": min(r["reqs_per_second"] for r in results),
            "max_reqs_per_second": max(r["reqs_per_second"] for r in results),
            "min_pods": min(valid_pods),
            "max_pods": max(valid_pods),
            "median_ms": p50,
            "p95_ms": p95,
            "results": results
        }, f, indent=2)
    print(f"\nArtifact saved to: {out_file}")

    assert actionable_count == fresh_total, f"Expected {fresh_total} actionable steps, got {actionable_count}"
    assert blocked_steps == stale_steps, f"Blocked steps {blocked_steps} differ from stale steps {stale_steps}"
    assert not failed_steps, f"Steps failed: {failed_steps}"
    assert blocked_count == 1, f"Expected 1 blocked step, got {blocked_count}"
    print("✓ All trace assertions passed cleanly.")


if __name__ == "__main__":
    main()
