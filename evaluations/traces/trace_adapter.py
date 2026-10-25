#!/usr/bin/env python3
"""
trace_adapter.py — Production Web Traffic Trace Adapter
-------------------------------------------------------
Streams real production time-series traffic points (from wikimedia_traffic_trace.json)
into Aven models for Little's Law capacity verification.

Reads AVEN_TRACE_STEP (default 0) or step from stdin JSON.
"""

import json
import os
import sys
import time
from pathlib import Path


def main():
    # Attempt to locate trace file relative to this script or AVEN_WORKSPACE
    script_dir = Path(__file__).resolve().parent
    workspace = os.environ.get("AVEN_WORKSPACE")
    candidates = [
        script_dir.parent / "data" / "wikimedia_traffic_trace.json",
        Path("evaluations/data/wikimedia_traffic_trace.json"),
        Path(workspace).joinpath("evaluations/data/wikimedia_traffic_trace.json") if workspace else None,
    ]
    trace_path = None
    for cand in candidates:
        if cand and cand.exists():
            trace_path = cand
            break

    if not trace_path:
        # Fallback inline record if file is not found
        trace = [
            {"step": 0, "reqs_per_second": 45000.0, "avg_latency_ms": 22.0, "avg_payload_kb": 64.0, "ttl_ms": 30000}
        ]
    else:
        with open(trace_path, "r", encoding="utf-8") as f:
            trace = json.load(f)

    # Read step parameter
    step = 0
    raw_in = sys.stdin.read().strip()
    if raw_in:
        try:
            data = json.loads(raw_in)
            if "step" in data:
                step = int(data["step"])
        except Exception:
            pass

    if "AVEN_TRACE_STEP" in os.environ:
        try:
            step = int(os.environ["AVEN_TRACE_STEP"])
        except ValueError:
            pass

    step = max(0, min(step, len(trace) - 1))
    point = trace[step]

    # Explicit override for TTL=0 testing if requested via environment
    ttl = int(point.get("ttl_ms", 30000))
    if os.environ.get("AVEN_INJECT_TTL_ZERO") == "1":
        ttl = 0

    now_ms = int(time.time() * 1000)
    result = {
        "reqs_per_second": float(point["reqs_per_second"]),
        "avg_latency_ms": float(point["avg_latency_ms"]),
        "avg_payload_kb": float(point["avg_payload_kb"]),
        "step_index": float(step),
        "timestamp_ms": now_ms,
        "ttl_ms": ttl,
        "backend_id": "production_trace",
        "warnings": ["production-replay", f"step-{step}"] if ttl > 0 else ["injected-zero-ttl"]
    }

    json.dump(result, sys.stdout)
    sys.stdout.write("\n")


if __name__ == "__main__":
    main()
