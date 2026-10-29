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
    python evaluations/weather/overnight_monitor.py --summary-only

Flags:
    --interval <sec>   Seconds between observation cycles (default: 60)
    --hours <num>      Total hours to run (default: 8.0, 0 for infinite until Ctrl+C)
    --stations <list>  Comma-separated station IDs (default: KORD,KDEN,KJFK,KSEA,KDFW)
    --ledger <path>    JSONL audit log destination (relative or absolute)
    --report <path>    Markdown summary report destination (relative or absolute)
    --summary-only     Compile markdown report from existing ledger and exit
"""

import argparse
import json
import os
import re
import shutil
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


# Platform-safe signal registrations (SIGINT is standard on Windows, others best-effort)
signal.signal(signal.SIGINT, signal_handler)
if hasattr(signal, "SIGTERM"):
    try:
        signal.signal(signal.SIGTERM, signal_handler)
    except (ValueError, OSError):
        pass
if hasattr(signal, "SIGBREAK"):
    try:
        signal.signal(signal.SIGBREAK, signal_handler)
    except (ValueError, OSError):
        pass


def parse_avenc_output(output: str):
    """
    Strictly parse key=value pairs from the 'Settled Environment:' section of Aven output.
    Avoids capturing lines from the Solver Trace or warning banners.
    """
    vars_map = {}
    is_blocked = "DECISION BLOCKED" in output
    in_settled = False

    # Regex matching variable assignments in Settled Environment:
    # e.g.: "  air_density_kg_m3 = 1.2589" or "  temp_c = 12.0000"
    kv_pattern = re.compile(r"^\s*([a-zA-Z_][a-zA-Z0-9_]*)\s*=\s*([-+]?[0-9]*\.?[0-9]+(?:[eE][-+]?[0-9]+)?|\S+)")

    for line in output.splitlines():
        line_clean = line.strip()
        if "Settled Environment:" in line:
            in_settled = True
            continue

        if in_settled:
            # End of settled environment section: empty line or next section header
            if not line_clean or line_clean.startswith("Fact Ledger:") or line_clean.startswith("---"):
                in_settled = False
                continue

            match = kv_pattern.match(line)
            if match:
                key, val_str = match.group(1), match.group(2)
                try:
                    vars_map[key] = float(val_str)
                except ValueError:
                    vars_map[key] = val_str

    return vars_map, is_blocked


def get_field_numeric(d: dict, *keys, default=None):
    """Safely retrieve the first matching numeric key from a dictionary."""
    for k in keys:
        if k in d and d[k] is not None:
            try:
                return float(d[k])
            except (ValueError, TypeError):
                pass
    return default


def format_stat_row(name: str, values: list, unit: str, precision: int = 2) -> str:
    """Format a table row with min, mean, max guarded against empty lists."""
    if values:
        val_min = f"{min(values):.{precision}f}"
        val_mean = f"{sum(values) / len(values):.{precision}f}"
        val_max = f"{max(values):.{precision}f}"
    else:
        val_min, val_mean, val_max = "n/a", "n/a", "n/a"
    return f"| **{name}** | {val_min} | {val_mean} | {val_max} | {unit} |"


def generate_summary_report(records, report_path, start_time, end_time):
    """Generate comprehensive markdown audit report when monitor completes."""
    total_cycles = len(records)
    if total_cycles == 0:
        print("[!] No records to generate report from.")
        return

    duration_sec = max(0.0, end_time - start_time)
    duration_str = f"{duration_sec / 3600:.2f} hours ({duration_sec / 60:.1f} minutes)"

    stations = sorted(list(set(r["station"] for r in records if "station" in r)))
    blocked_count = sum(1 for r in records if r.get("is_blocked", False))
    actionable_count = total_cycles - blocked_count

    actionable_records = [r for r in records if not r.get("is_blocked", False)]
    temps = [
        r["temp_c"]
        for r in actionable_records
        if "temp_c" in r and isinstance(r["temp_c"], (int, float))
    ]
    densities = [
        get_field_numeric(r, "air_density_kg_m3", "air_density")
        for r in actionable_records
        if get_field_numeric(r, "air_density_kg_m3", "air_density") is not None
    ]
    latencies = [
        r["elapsed_ms"]
        for r in records
        if "elapsed_ms" in r and isinstance(r["elapsed_ms"], (int, float))
    ]

    actionable_pct_str = f"{actionable_count / total_cycles * 100:.1f}%" if total_cycles > 0 else "0.0%"

    report_lines = [
        "# Aven Atmospheric Dynamics — Overnight Live Weather Audit Report",
        "",
        f"**Run Period:** {datetime.fromtimestamp(start_time, tz=timezone.utc).isoformat()} to {datetime.fromtimestamp(end_time, tz=timezone.utc).isoformat()} ({duration_str})  ",
        "**Telemetry Source:** National Weather Service API (`api.weather.gov`)  ",
        f"**Monitored Stations:** {', '.join(stations)}  ",
        f"**Total Evaluated Cycles:** {total_cycles}  ",
        f"**Actionable Solves:** {actionable_count} / {total_cycles} ({actionable_pct_str})  ",
        f"**Stale Blocks (`HOLD`):** {blocked_count}  ",
        "",
        "---",
        "",
        "## Key Atmospheric & Solver Metrics (Actionable Operations)",
        "",
        "| Metric | Minimum | Mean / Average | Maximum | Unit |",
        "| :--- | :--- | :--- | :--- | :--- |",
        format_stat_row("Ambient Temperature", temps, "°C", precision=2),
        format_stat_row("Ideal Gas Air Density", densities, "kg/m³", precision=4),
        format_stat_row("Process Settle Time", latencies, "ms", precision=1),
        "",
        "---",
        "",
        "## Per-Station Operational Summary (Latest Active Reading)",
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
        st_records = [r for r in records if r.get("station") == st and not r.get("is_blocked", False)]
        if not st_records:
            st_records = [r for r in records if r.get("station") == st]
        if st_records:
            latest = st_records[-1]
            loc = location_names.get(st, st)
            is_st_blocked = latest.get("is_blocked", False)

            t_c = latest.get("temp_c")
            rh = latest.get("relative_humidity")
            p_pa = latest.get("barometric_pressure_pa")
            rho = get_field_numeric(latest, "air_density_kg_m3", "air_density")
            wp = get_field_numeric(latest, "wind_power_density_w_m2", "wind_power_density")
            margin = latest.get("free_cooling_margin")

            t_str = f"{t_c:.1f}°C" if isinstance(t_c, (int, float)) else "n/a"
            rh_str = f"{rh:.1f}%" if isinstance(rh, (int, float)) else "n/a"
            p_str = f"{p_pa / 100.0:.1f} hPa" if isinstance(p_pa, (int, float)) else "n/a"
            rho_str = f"{rho:.3f} kg/m³" if isinstance(rho, (int, float)) else "n/a"
            wp_str = f"{wp:.1f} W/m²" if isinstance(wp, (int, float)) else "n/a"

            if is_st_blocked:
                margin_str = "n/a"
                status_text = "BLOCKED_STALE"
            elif isinstance(margin, (int, float)):
                margin_str = f"{margin:+.1f}°C"
                status_text = "FREE_COOLING" if margin >= 0 else "CHILLER_ACTIVE"
            else:
                margin_str = "n/a"
                status_text = "UNKNOWN"

            report_lines.append(
                f"| **{st}** | {loc} | {t_str} | {rh_str} | {p_str} | {rho_str} | {wp_str} | {margin_str} | `{status_text}` |"
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
        is_blk = r.get("is_blocked", False)
        t_c = r.get("temp_c")
        rh = r.get("relative_humidity")
        dew_c = r.get("dewpoint_c")
        dew_approx = r.get("dewpoint_approx")
        margin = r.get("free_cooling_margin")

        if is_blk:
            t_str = f"{t_c:.1f}°C" if isinstance(t_c, (int, float)) and t_c != 0.0 else "—"
            rh_str = f"{rh:.1f}%" if isinstance(rh, (int, float)) and rh != 0.0 else "—"
            err_str = "—"
            mode = "HOLD"
            fresh = "STALE (TTL=0)"
        else:
            t_str = f"{t_c:.1f}°C" if isinstance(t_c, (int, float)) else "n/a"
            rh_str = f"{rh:.1f}%" if isinstance(rh, (int, float)) else "n/a"
            if isinstance(dew_approx, (int, float)) and isinstance(dew_c, (int, float)):
                err = abs(dew_approx - dew_c)
                err_str = f"{err:.2f}°C"
            else:
                err_str = "n/a"
            mode = "FREE_COOLING" if (isinstance(margin, (int, float)) and margin >= 0) else "MECHANICAL"
            fresh = "VERIFIED"

        lat = r.get("elapsed_ms", 0.0)
        report_lines.append(
            f"| #{r.get('cycle', '?')} | {r.get('iso_time', '')} | {r.get('station', '')} | "
            f"{t_str} | {rh_str} | {err_str} | `{mode}` | {lat:.1f} ms | `{fresh}` |"
        )

    report_lines.append("\n*Generated autonomously by Aven Relational Toolchain.*")

    target_path = Path(report_path)
    target_path.parent.mkdir(parents=True, exist_ok=True)
    with open(target_path, "w", encoding="utf-8") as f:
        f.write("\n".join(report_lines) + "\n")
    print(f"\n[✓] Audit report generated and saved to: {target_path}")


def main():
    parser = argparse.ArgumentParser(description="Aven Live Weather Overnight Monitor")
    parser.add_argument("--interval", type=int, default=60, help="Polling interval in seconds (default: 60)")
    parser.add_argument("--hours", type=float, default=8.0, help="Total hours to monitor (default: 8.0, 0=infinite)")
    parser.add_argument("--stations", type=str, default="KORD,KDEN,KJFK,KSEA,KDFW", help="Comma-separated NWS stations")
    parser.add_argument("--ledger", type=str, default="evaluations/weather/weather_cycle_log.jsonl", help="Audit ledger output path (rel/abs)")
    parser.add_argument("--report", type=str, default="evaluations/weather/overnight_summary_report.md", help="Summary report output path (rel/abs)")
    parser.add_argument("--summary-only", action="store_true", help="Compile summary report from existing ledger without executing new cycles")
    args = parser.parse_args()

    root = Path(__file__).resolve().parent.parent.parent

    # Support absolute or relative paths
    ledger_arg = Path(args.ledger)
    ledger_path = ledger_arg if ledger_arg.is_absolute() else root / ledger_arg

    report_arg = Path(args.report)
    report_path = report_arg if report_arg.is_absolute() else root / report_arg

    # Summary only mode: read existing ledger and compile report
    if args.summary_only:
        if not ledger_path.exists():
            print(f"Error: ledger file not found at {ledger_path}")
            sys.exit(1)
        records = []
        with open(ledger_path, "r", encoding="utf-8") as f:
            for line in f:
                line = line.strip()
                if line:
                    try:
                        records.append(json.loads(line))
                    except Exception:
                        pass
        if not records:
            print(f"Error: no valid records in {ledger_path}")
            sys.exit(1)
        t_start = records[0].get("timestamp", time.time())
        t_end = records[-1].get("timestamp", time.time())
        print(f"Loaded {len(records)} records from {ledger_path}. Compiling summary report...")
        generate_summary_report(records, report_path, t_start, t_end)
        return

    # Cross-platform binary resolution (Windows / Linux / RHEL 9 / macOS)
    env_bin = os.environ.get("AVENC_BIN")
    if env_bin and Path(env_bin).exists():
        avenc_exe = Path(env_bin)
    else:
        exe_name = "avenc.exe" if os.name == "nt" else "avenc"
        avenc_exe = root / "target" / "release" / exe_name
        if not avenc_exe.exists():
            avenc_exe = root / "target" / "debug" / exe_name
        if not avenc_exe.exists():
            which_bin = shutil.which(exe_name) or shutil.which("avenc")
            if which_bin:
                avenc_exe = Path(which_bin)
            else:
                print(f"Error: Aven executable not found in target/release, target/debug, or PATH.")
                print("Tip: Build with 'cargo build --release' or set AVENC_BIN=/path/to/avenc")
                sys.exit(1)

    model_path = root / "evaluations" / "weather" / "atmospheric_dynamics.aven"
    stations = [s.strip().upper() for s in args.stations.split(",") if s.strip()]
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

        # Process exit code check: warn if avenc failed unexpectedly
        if proc.returncode != 0 and not is_blocked:
            print(f"[-] Warning: avenc process failed (exit code {proc.returncode})")
            err_snippet = proc.stderr.strip() or proc.stdout.strip()
            if err_snippet:
                print(f"    output: {err_snippet[:200]}")

        # Field retrieval with normalization between model output names and aliases
        temp = get_field_numeric(vars_map, "temp_c")
        rh = get_field_numeric(vars_map, "relative_humidity")
        density = get_field_numeric(vars_map, "air_density_kg_m3", "air_density")
        wind_kmh = get_field_numeric(vars_map, "wind_speed_kmh")
        wind_power = get_field_numeric(vars_map, "wind_power_density_w_m2", "wind_power_density")
        margin = get_field_numeric(vars_map, "free_cooling_margin")
        dew_c = get_field_numeric(vars_map, "dewpoint_c")
        dew_approx = get_field_numeric(vars_map, "dewpoint_approx")

        if is_blocked:
            mode = "HOLD"
            status_tag = "❌ BLOCKED (STALE)"
            display_temp = f"{temp:.1f}°C" if temp is not None else "—"
            display_rh = f"{rh:.1f}%" if rh is not None else "—"
            display_air = f"{density:.3f} kg/m³" if density is not None else "—"
            display_wind = f"{wind_kmh:.1f} km/h" if wind_kmh is not None else "—"
            display_margin = "HOLD"
        else:
            mode = "FREE_COOLING" if (margin is not None and margin >= 0) else "CHILLER"
            status_tag = "✓ OK"
            display_temp = f"{temp:.1f}°C" if temp is not None else "n/a"
            display_rh = f"{rh:.1f}%" if rh is not None else "n/a"
            display_air = f"{density:.3f} kg/m³" if density is not None else "n/a"
            display_wind = f"{wind_kmh:.1f} km/h ({wind_power:.1f} W/m²)" if (wind_kmh is not None and wind_power is not None) else "n/a"
            display_margin = f"{margin:+.1f}°C [{mode}]" if margin is not None else "n/a"

        # Console dashboard line
        print(
            f"[{now_iso}] Cycle #{cycle:3d} | {station:4s} | "
            f"Temp: {display_temp:>7s} | RH: {display_rh:>6s} | Air: {display_air} | "
            f"Wind: {display_wind} | Margin: {display_margin} | Settle: {elapsed_ms:5.1f}ms | {status_tag}"
        )

        record_entry = {
            "cycle": cycle,
            "iso_time": now_iso,
            "timestamp": current_time,
            "station": station,
            "temp_c": temp,
            "relative_humidity": rh,
            "dewpoint_c": dew_c,
            "dewpoint_approx": dew_approx,
            "barometric_pressure_pa": get_field_numeric(vars_map, "barometric_pressure_pa"),
            "air_density": density,
            "air_density_kg_m3": density,
            "wind_speed_kmh": wind_kmh,
            "wind_power_density": wind_power,
            "wind_power_density_w_m2": wind_power,
            "free_cooling_margin": margin,
            "api_latency_ms": get_field_numeric(vars_map, "api_latency_ms", default=0.0),
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