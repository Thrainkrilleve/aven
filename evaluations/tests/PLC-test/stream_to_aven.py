#!/usr/bin/env python3
"""
stream_to_aven.py — Live streaming ingest pipeline from OpenPLC to Aven.

Usage:
    python openplc_dummy.py | python stream_to_aven.py
    python openplc_dummy.py | python stream_to_aven.py --limit 10
    python openplc_poller.py | python stream_to_aven.py
"""
import sys
import json
import hashlib
import time
import argparse

def compute_spec_hash(var_dict):
    """Computes deterministic 16-character SHA-256 specification hash identical to Aven VarMap."""
    sorted_items = sorted(var_dict.items())
    serialized = "".join(f"{k}:{v:.6f};" for k, v in sorted_items if isinstance(v, (int, float)))
    return hashlib.sha256(serialized.encode("utf-8")).hexdigest()[:16]

def evaluate_aven_relations(vars_map):
    """
    Evaluates Aven relational constraints and alarm rules against the live PLC VarMap.
    Returns: (is_valid, residual, binding_constraints, alarms)
    """
    scan_ms = vars_map.get("scan_ms", 8.0)
    temp_c = vars_map.get("temp_c", 38.0)
    power_w = vars_map.get("power_w", 180.0)
    io_points = vars_map.get("io_points", 24)
    quality_pct = vars_map.get("quality_pct", 99.2)
    alarm_count = vars_map.get("alarm_count", 0)

    alarms = []
    binding = []

    # Relational model equation: power_w = 120.0 + io_points * 2.8
    expected_power = 120.0 + io_points * 2.8
    residual = abs(power_w - expected_power)

    # Constraint 1: scan_ms <= 18.0 ms
    if scan_ms > 18.0:
        alarms.append(f"HIGH_SCAN_TIME: {scan_ms:.2f} ms > 18.0 ms")
    elif scan_ms > 15.0:
        binding.append("scan_ms < 18.0 (near threshold)")

    # Constraint 2: temp_c <= 55.0 °C
    if temp_c > 55.0:
        alarms.append(f"CABINET_OVERHEAT: {temp_c:.1f} °C > 55.0 °C")
    elif temp_c > 50.0:
        binding.append("temp_c < 55.0 (near threshold)")

    # Constraint 3: alarm_count == 0
    if alarm_count > 0:
        alarms.append(f"PLC_FAULT: alarm_count = {alarm_count}")

    # Constraint 4: quality_pct >= 90.0
    if quality_pct < 90.0:
        alarms.append(f"DEGRADED_QUALITY: {quality_pct:.2f}% < 90.0%")

    is_valid = len(alarms) == 0
    return is_valid, residual, binding, alarms

def main():
    parser = argparse.ArgumentParser(description="Aven OpenPLC Stream Ingest")
    parser.add_argument("--limit", type=int, default=0, help="Stop after N cycles (0 = infinite)")
    args = parser.parse_args()

    print("=" * 80)
    print("Aven Relational Orchestrator — Live OpenPLC Stream Ingest")
    print("Ingesting JSON telemetry line-by-line into Aven VarMap & Relational Engine")
    print("=" * 80)
    print(f"{'Cycle':<7} | {'Spec Hash':<16} | {'Scan(ms)':<8} | {'Power(W)':<8} | {'Temp(C)':<7} | {'Qual(%)':<7} | {'Rank':<4} | Status")
    print("-" * 80)

    cycle = 0
    try:
        for line in sys.stdin:
            line = line.strip()
            if not line:
                continue

            try:
                data = json.loads(line)
            except Exception:
                continue

            if "error" in data:
                print(f"[{cycle:04d}] ERROR: {data['error']}")
                continue

            cycle += 1
            spec_hash = compute_spec_hash(data)
            is_valid, residual, binding, alarms = evaluate_aven_relations(data)

            scan_ms = data.get("scan_ms", 0.0)
            power_w = data.get("power_w", 0.0)
            temp_c = data.get("temp_c", 0.0)
            quality_pct = data.get("quality_pct", 0.0)
            rank = len([k for k, v in data.items() if isinstance(v, (int, float))])

            if is_valid:
                status_str = f"CONVERGED (Res: {residual:.2f})"
            else:
                status_str = f"ALARM: {'; '.join(alarms)}"

            print(f"#{cycle:<6} | {spec_hash:<16} | {scan_ms:<8.2f} | {power_w:<8.1f} | {temp_c:<7.1f} | {quality_pct:<7.2f} | {rank:<4} | {status_str}")
            sys.stdout.flush()

            if args.limit > 0 and cycle >= args.limit:
                break
    except KeyboardInterrupt:
        pass

    print("-" * 80)
    print(f"Stream completed: {cycle} cycles ingested and verified by Aven.")

if __name__ == "__main__":
    main()
