#!/usr/bin/env python3
"""
k8s/controller/workload_adapter.py
----------------------------------
External evaluator for Aven HPA capacity models.
Reads live request traffic metrics and feeds them to Aven.

Supports:
- Environment variable overrides: REQS_PER_SECOND, AVG_LATENCY_MS, AVG_PAYLOAD_KB
- Malicious/stale injection: AVEN_INJECT_TTL_ZERO=1
- Freshness metadata (timestamp_ms, ttl_ms)
"""

import json
import os
import sys
import time


def main():
    # Consume stdin if piped from Aven
    if not sys.stdin.isatty():
        try:
            _ = sys.stdin.read()
        except Exception:
            pass

    # Read traffic metrics from env (or default to 10k req/s baseline)
    reqs_per_sec = float(os.environ.get("REQS_PER_SECOND", "10000.0"))
    avg_latency_ms = float(os.environ.get("AVG_LATENCY_MS", "20.0"))
    avg_payload_kb = float(os.environ.get("AVG_PAYLOAD_KB", "64.0"))

    now_ms = int(time.time() * 1000)

    # Freshness control
    if os.environ.get("AVEN_INJECT_TTL_ZERO") == "1":
        ttl_ms = 0
        warnings = ["injected-stale-traffic-data"]
    else:
        ttl_ms = 15000  # 15 seconds TTL for live HPA traffic metrics
        warnings = ["live-traffic-telemetry"]

    data = {
        "reqs_per_second": reqs_per_sec,
        "avg_latency_ms": avg_latency_ms,
        "avg_payload_kb": avg_payload_kb,
        "timestamp_ms": now_ms,
        "ttl_ms": ttl_ms,
        "backend_id": "workload-telemetry",
        "value": reqs_per_sec,
        "warnings": warnings,
    }

    print(json.dumps(data))


if __name__ == "__main__":
    main()
