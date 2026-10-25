#!/usr/bin/env python3
"""
evaluations/weather/overnight_monitor.py
-----------------------------------------
Autonomous Multi-Hour Live Weather & Atmospheric Physics Monitor for Aven.

Pulls real physical observations from the National Weather Service (NWS) API
(https://api.weather.gov), solves atmospheric and thermodynamic equations in Aven
(ideal gas air density, aerodynamic wind power, dewpoint validation, HVAC free cooling),
and records an immutable audit ledger.

Usage:
    python evaluations/weather/overnight_monitor.py
    python evaluations/weather/overnight_monitor.py --interval 120 --hours 8 --stations KORD,KDEN,KJFK,KSEA,KDFW

Flags:
    --interval <sec>   Seconds between observation cycles (default: 60)
    --hours <num>      Total hours to run (default: 8.0, 0 for infinite until Ctrl+C)
    --stations <list>  Comma-separated station IDs (default: KORD,KDEN,KJFK,KSEA,KDFW)
    --ledger <path>    JSONL audit log destination
    --report <path>    Markdown summary report destination
"""

import argparse
import json
import os
import signal
import subprocess
import sys
import time
from datetime import datetime, timezone
from pathlib import Path

# Fix Windows console UTF-8 output
if hasattr(sys.stdout, "reconfigure"):
    try:
        sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    except Exception:
        pass

RUNNING = True


def signal_handler(sig, frame):
    global RUNNING
    print("\n[!] Shutdown signal received. Finalizing audit summary report...")
    RUNNING = False


signal.signal(signal.SIGINT, signal_handler)
signal.signal(signal.SIGTERM, signal_handler)


def parse_avenc_output(output: str):
    """Parse key=value pairs from Aven settled environment."""
    vars_map = {}
    is_blocked = "DECISION BLOCKED" in output
    for line in output.splitlines():
        line = line.strip()
        if "=" in line and not line.startswith("//") and not line.startswith("#"):
            parts = line.split("=", 1)
            k = parts[0].strip()
            v_str = parts[1].strip()
            try:
                vars_map[k] = float(v_str)
            except ValueError:
                vars_map[k] = v_str
    return vars_map, is_blocked


def generate_summary_report(records, report_path, start_time, end_time):
    """Generate comprehensive markdown audit report when monitor completes."""
    total_cycles = len(records)
    if total_cycles == 0:
        return

    duration_sec = end_time - start_time
    duration_str = f"{duration_sec / 3600:.2f} hours ({duration_sec / 60:.1f} minutes)"

    stations = sorted(list(set(r["station"] for r in records)))
    blocked_count = sum(1 for r in records if r["is_blocked"])
    actionable_count = total_cycles - blocked_count

    temps = [r["temp_c"] for r in records if "temp_c" in r and isinstance(r["temp_c"], (int, float))]
    densities = [r["air_density"] for r in records if "air_density" in r and isinstance(r["air_density"], (int, float))]
    latencies = [r["elapsed_ms"] for r in records]

    report_lines = [
        "# Aven Atmospheric Dynamics — Overnight Live Weather Audit Report",
        "",
        f"**Run Period:** {datetime.fromtimestamp(start_time, tz=timezone.utc).isoformat()} to {datetime.fromtimestamp(end_time, tz=timezone.utc).isoformat()} ({duration_str})  ",
        f"**Telemetry Source:** National Weather Service API (`api.weather.gov`)  ",
        f"**Monitored Stations:** {', '.join(stations)}  ",
        f"**Total Evaluated Cycles:** {total_cycles}  ",
        f"**Actionable Solves:** {actionable_count} / {total_cycles} ({actionable_count/total_cycles*100:.1f}%)  ",
        f"**Stale Blocks (`HOLD`):** {blocked_count}  ",
        "",
        "---",
        "",
        "## Key Atmospheric & Solver Metrics",
        "",
        "| Metric | Minimum | Mean / Average | Maximum | Unit |",
        "| :--- | :--- | :--- | :--- | :--- |",
        f"| **Ambient Temperature** | {min(temps):.2f} | {sum(temps)/len(temps):.2f} | {max(temps):.2f} | °C |",
        f"| **Ideal Gas Air Density** | {min(densities):.4f} | {sum(densities)/len(densities):.4f} | {max(densities):.4f} | kg/m³ |",
        f"| **Process Settle Time** | {min(latencies):.1f} | {sum(latencies)/len(latencies):.1f} | {max(latencies):.1f} | ms |",
        "",
        "---",
        "",
        "## Per-Station Operational Summary",
        "",
        "| Station | Location | Latest Temp | Humidity | Pressure | Air Density | Wind Power | Free Cooling Margin | Status |",
        "| :--- | :--- | :--- | :--- | :--- | :--- | :--- | :--- | :--- |",
    ]

    location_names = {
        "KORD": "Chicago O'Hare, IL",
        "KDEN": "Denver Intl, CO",
        "KJFK": "New York JFK, NY",
        "KSEA": "Seattle-Tacoma, WA",
        "KDFW": "Dallas/Fort Worth, TX",
    }

    for st in stations:
        st_records = [r for r in records if r["station"] == st]
        if st_records:
            latest = st_records[-1]
            loc = location_names.get(st, st)
            margin = latest.get("free_cooling_margin", 0.0)
            status_text = "FREE_COOLING" if margin >= 0 else "CHILLER_ACTIVE"
            report_lines.append(
                f"| **{st}** | {loc} | {latest.get('temp_c', 0.0):.1f}°C | "
                f"{latest.get('relative_humidity', 0.0):.1f}% | "
                f"{latest.get('barometric_pressure_pa', 0.0)/100:.1f} hPa | "
                f"{latest.get('air_density', 0.0):.3f} kg/m³ | "
                f"{latest.get('wind_power_density', 0.0):.1f} W/m² | "
                f"{margin:+.1f}°C | `{status_text}` |"
            )

    report_lines.extend([
        "",
        "---",
        "",
        "## Recent Cycle Log Samples",
        "",
        "| Cycle | UTC Timestamp | Station | Temp | RH | Dewpoint Error | Mode | Settle Latency | Freshness |",
        "| :--- | :--- | :--- | :--- | :--- | :--- | :--- | :--- | :--- |",
    ])

    for r in records[-15:]:
        err = abs(r.get("dewpoint_approx", 0.0) - r.get("dewpoint_c", 0.0))
        fresh = "STALE (TTL=0)" if r["is_blocked"] else "VERIFIED"
        mode = "FREE_COOLING" if r.get("free_cooling_margin", 0.0) >= 0 else "MECHANICAL"
        report_lines.append(
            f"| #{r['cycle']} | {r['iso_time']} | {r['station']} | "
            f"{r.get('temp_c', 0.0):.1f}°C | {r.get('relative_humidity', 0.0):.1f}% | "
            f"{err:.2f}°C | `{mode}` | {r['elapsed_ms']:.1f} ms | `{fresh}` |"
        )

    report_lines.append("\n*Generated autonomously by Aven Relational Toolchain.*")

    Path(report_path).parent.mkdir(parents=True, exist_ok=True)
    with open(report_path, "w", encoding="utf-8") as f:
        f.write("\n".join(report_lines) + "\n")
    print(f"\n[✓] Audit report generated and saved to: {report_path}")


def main():
    parser = argparse.ArgumentParser(description="Aven Live Weather Overnight Monitor")
    parser.add_argument("--interval", type=int, default=60, help="Polling interval in seconds (default: 60)")
    parser.add_argument("--hours", type=float, default=8.0, help="Total hours to monitor (default: 8.0, 0=infinite)")
    parser.add_argument("--stations", type=str, default="KORD,KDEN,KJFK,KSEA,KDFW", help="Comma-separated NWS stations")
    parser.add_argument("--ledger", type=str, default="evaluations/weather/weather_audit_ledger.jsonl", help="Audit ledger output path")
    parser.add_argument("--report", type=str, default="evaluations/weather/overnight_summary_report.md", help="Summary report output path")
    args = parser.parse_args()

    root = Path(__file__).resolve().parent.parent.parent
    avenc_exe = root / "target" / "debug" / "avenc.exe"
    if not avenc_exe.exists():
        avenc_exe = root / "target" / "release" / "avenc.exe"
    if not avenc_exe.exists():
        print(f"Error: avenc executable not found at {avenc_exe}")
        sys.exit(1)

    model_path = root / "evaluations" / "weather" / "atmospheric_dynamics.aven"
    stations = [s.strip().upper() for s in args.stations.split(",") if s.strip()]
    ledger_path = root / args.ledger
    report_path = root / args.report
    ledger_path.parent.mkdir(parents=True, exist_ok=True)

    max_duration_sec = args.hours * 3600.0 if args.hours > 0 else float("inf")
    start_time = time.time()

    print("=" * 80)
    print(" Aven Overnight Live Weather Monitor (NWS api.weather.gov)")
    print(f" Target Model:       {model_path.name}")
    print(f" Polling Interval:   Every {args.interval} seconds")
    print(f" Stations Monitored: {', '.join(stations)} (rotating)")
    print(f" Duration Planned:   {args.hours:.1f} hours ({max_duration_sec:.0f} seconds)")
    print(f" Audit Ledger Path:  {ledger_path}")
    print(f" Summary Report:     {report_path}")
    print("=" * 80)
    print("Press Ctrl+C at any time to pause and compile the summary report.\n")

    cycle = 0
    records = []

    while RUNNING:
        current_time = time.time()
        elapsed_total = current_time - start_time
        if elapsed_total >= max_duration_sec:
            print(f"\n[✓] Planned duration of {args.hours:.1f} hours completed.")
            break

        station = stations[cycle % len(stations)]
        cycle += 1

        env = os.environ.copy()
        env["AVEN_NWS_STATION"] = station
        env["AVEN_NWS_STEP"] = str(cycle)
        env["AVEN_WORKSPACE"] = str(root)

        t0 = time.perf_counter()
        proc = subprocess.run(
            [str(avenc_exe), "run", str(model_path)],
            env=env,
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            cwd=str(root),
        )
        elapsed_ms = (time.perf_counter() - t0) * 1000.0
        combined = proc.stdout + proc.stderr

        vars_map, is_blocked = parse_avenc_output(combined)
        now_iso = datetime.now(timezone.utc).strftime("%Y-%m-%d %H:%M:%S UTC")

        temp = vars_map.get("temp_c", 0.0)
        rh = vars_map.get("relative_humidity", 0.0)
        density = vars_map.get("air_density_kg_m3", 0.0)
        wind_kmh = vars_map.get("wind_speed_kmh", 0.0)
        wind_power = vars_map.get("wind_power_density_w_m2", 0.0)
        margin = vars_map.get("free_cooling_margin", 0.0)
        dew_err = abs(vars_map.get("dewpoint_approx", 0.0) - vars_map.get("dewpoint_c", 0.0))

        mode = "FREE_COOLING" if margin >= 0 else "CHILLER"
        status_tag = "❌ BLOCKED" if is_blocked else "✓ OK"

        # Console dashboard line
        print(
            f"[{now_iso}] Cycle #{cycle:3d} | {station:4s} | "
            f"Temp: {temp:4.1f}°C | RH: {rh:4.1f}% | Air: {density:5.3f} kg/m³ | "
            f"Wind: {wind_kmh:4.1f} km/h ({wind_power:4.1f} W/m²) | "
            f"Margin: {margin:+4.1f}°C [{mode}] | Settle: {elapsed_ms:5.1f}ms | {status_tag}"
        )

        record_entry = {
            "cycle": cycle,
            "iso_time": now_iso,
            "timestamp": current_time,
            "station": station,
            "temp_c": temp,
            "relative_humidity": rh,
            "dewpoint_c": vars_map.get("dewpoint_c", 0.0),
            "dewpoint_approx": vars_map.get("dewpoint_approx", 0.0),
            "barometric_pressure_pa": vars_map.get("barometric_pressure_pa", 0.0),
            "air_density": density,
            "wind_speed_kmh": wind_kmh,
            "wind_power_density": wind_power,
            "free_cooling_margin": margin,
            "api_latency_ms": vars_map.get("api_latency_ms", 0.0),
            "elapsed_ms": elapsed_ms,
            "is_blocked": is_blocked,
        }
        records.append(record_entry)

        # Append to audit ledger file
        try:
            with open(ledger_path, "a", encoding="utf-8") as f:
                f.write(json.dumps(record_entry) + "\n")
        except Exception as e:
            print(f"Warning: failed to append to ledger: {e}")

        # Periodic summary checkpoint every 30 cycles
        if cycle % 30 == 0:
            generate_summary_report(records, report_path, start_time, time.time())

        # Sleep remaining interval time
        sleep_time = max(1.0, args.interval - (elapsed_ms / 1000.0))
        for _ in range(int(sleep_time)):
            if not RUNNING:
                break
            time.sleep(1.0)

    # Final summary report at loop exit
    end_time = time.time()
    generate_summary_report(records, report_path, start_time, end_time)
    print("\nMonitor process finished successfully.")


if __name__ == "__main__":
    main()
