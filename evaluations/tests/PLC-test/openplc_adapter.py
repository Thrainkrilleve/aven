#!/usr/bin/env python3
"""
openplc_adapter.py — External evaluator adapter for Aven.

Reads live telemetry from openplc_dummy.py (or Modbus poller), merges any decision
variables supplied by Aven, and emits a structured JSON dictionary back to Aven's
ExternalEvaluator with backend_id: "openplc".
"""
import sys
import json
import time
import math
import random
import os

SCRIPT_DIR = os.path.dirname(os.path.abspath(__file__))
CACHE_FILE = os.path.join(SCRIPT_DIR, "latest_plc.json")

import threading

def read_inputs():
    """Reads JSON inputs supplied by Aven on stdin with a short timeout."""
    buf = []
    def reader():
        try:
            content = sys.stdin.read()
            if content.strip():
                buf.append(content)
        except Exception:
            pass

    t = threading.Thread(target=reader, daemon=True)
    t.start()
    t.join(timeout=0.15)

    if buf:
        try:
            return json.loads(buf[0])
        except Exception:
            return {}
    return {}

def get_latest_telemetry(inputs):
    """
    Retrieves the latest telemetry dictionary.
    First checks if the background openplc_dummy.py cache is active.
    If not running or stale, generates a live cycle matching test.st dynamic equations.
    """
    telemetry = None

    # Check cache file written by openplc_dummy.py
    if os.path.exists(CACHE_FILE):
        try:
            mtime = os.path.getmtime(CACHE_FILE)
            if time.time() - mtime < 5.0:
                with open(CACHE_FILE, "r") as f:
                    telemetry = json.load(f)
        except Exception:
            telemetry = None

    # Fallback to direct computation if dummy daemon isn't running
    if not telemetry:
        t = time.time()
        telemetry = {
            "scan_ms": round(8.0 + 2 * math.sin(t / 5) + random.uniform(-0.1, 0.1), 2),
            "io_points": 24 + int(t) % 10,
            "power_w": round(180 + 20 * math.sin(t / 8), 1),
            "temp_c": round(38 + 3 * math.sin(t / 12), 1),
            "throughput_pph": round(120 + 15 * math.sin(t / 6), 1),
            "alarm_count": 0,
            "cpu_load_pct": round(35 + 10 * math.sin(t / 7), 1),
            "network_load_pct": round(12 + 5 * math.sin(t / 9), 1),
            "quality_pct": round(99.2 - 0.5 * math.sin(t / 10), 2),
            "ts": t,
        }

    # Override with decision variables provided by Aven if supplied
    for k, v in inputs.items():
        if isinstance(v, (int, float)):
            telemetry[k] = float(v)

    # Recalculate dependent outputs if Aven varied io_points or cpu_load_pct
    if "io_points" in inputs or "cpu_load_pct" in inputs:
        io_pts = telemetry["io_points"]
        cpu_load = telemetry["cpu_load_pct"]
        telemetry["power_w"] = round(120.0 + io_pts * 2.8, 1)
        telemetry["scan_ms"] = round(max(5.0, min(25.0, 7.5 + cpu_load * 0.04)), 2)
        telemetry["throughput_pph"] = round(max(0.0, 140.0 - telemetry["scan_ms"] * 2.0), 1)
        telemetry["quality_pct"] = round(max(90.0, min(100.0, 99.8 - telemetry["scan_ms"] * 0.05)), 2)

    return telemetry

def main():
    inputs = read_inputs()
    data = get_latest_telemetry(inputs)

    # Contract metadata for Aven
    data["backend_id"] = "openplc"
    data["value"] = data.get("throughput_pph", 120.0)

    # Output JSON line to stdout
    print(json.dumps(data))
    sys.stdout.flush()

if __name__ == "__main__":
    main()
