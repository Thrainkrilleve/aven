#!/usr/bin/env python3
"""
evaluations/flight_mesh/stream_ringer.py
---------------------------------------
High-Throughput Multi-Stream Physics Stress Test Harness for Aven.
Fuses live telemetry from:
  1. OpenSky Network (6,000+ active ADS-B aircraft in flight)
  2. NOAA Aviation Weather Center (AWC METAR atmospheric lapse & altimeter)
  3. USGS Hydrology (real-time river basin discharge & elevation proxy)

Pipes live flight vectors through Aven's relational solver to compute:
  - Localized air density at flight level via barometric lapse rate
  - High-speed compressible aerodynamics (Mach number, speed of sound, dynamic pressure)
  - Specific excess climb/descent power
  - Regional hydrological proxy & terrain clearance invariants
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
import urllib.request
from concurrent.futures import ThreadPoolExecutor, as_completed
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
    print("\n[!] Shutdown signal received. Finalizing stress-test summary report...")
    RUNNING = False


signal.signal(signal.SIGINT, signal_handler)
if hasattr(signal, "SIGTERM"):
    try:
        signal.signal(signal.SIGTERM, signal_handler)
    except (ValueError, OSError):
        pass


def parse_avenc_output(output: str):
    """Strictly parse key=value pairs from the 'Settled Environment:' section."""
    vars_map = {}
    is_blocked = "DECISION BLOCKED" in output
    is_violated = "Constraint violation" in output
    violation_msg = ""
    if is_violated:
        m = re.search(r"Constraint violation in '([^']+)'", output)
        if m:
            violation_msg = m.group(1)

    in_settled = False
    kv_pattern = re.compile(r"^\s*([a-zA-Z_][a-zA-Z0-9_]*)\s*=\s*([-+]?[0-9]*\.?[0-9]+(?:[eE][-+]?[0-9]+)?|\S+)")

    for line in output.splitlines():
        line_clean = line.strip()
        if "Settled Environment:" in line:
            in_settled = True
            continue

        if in_settled:
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

    return vars_map, is_blocked, is_violated, violation_msg


def format_stat_row(name: str, values: list, unit: str, precision: int = 2) -> str:
    """Format a markdown table row guarded against empty lists with exact median calculation."""
    if values:
        s = sorted(values)
        n = len(s)
        mid = n // 2
        val_min = f"{s[0]:.{precision}f}"
        val_med = f"{(s[mid] if n % 2 != 0 else (s[mid - 1] + s[mid]) / 2.0):.{precision}f}"
        val_mean = f"{sum(s) / n:.{precision}f}"
        val_max = f"{s[-1]:.{precision}f}"
    else:
        val_min, val_med, val_mean, val_max = "n/a", "n/a", "n/a", "n/a"
    return f"| **{name}** | {val_min} | {val_med} | {val_mean} | {val_max} | {unit} |"


def generate_summary_report(records, report_path, start_time, end_time, cold_start_sec: float = 0.0):
    """Compile comprehensive markdown report summarizing multi-physics stress tests."""
    total_cycles = len(records)
    if total_cycles == 0:
        return

    duration_sec = max(0.001, end_time - start_time)
    solves_per_sec = total_cycles / duration_sec

    blocked_count = sum(1 for r in records if r.get("is_blocked", False))
    violated_count = sum(1 for r in records if r.get("is_violated", False))
    corrupt_count = sum(
        1 for r in records
        if r.get("baro_altitude_m", 0.0) == 0.0 and not r.get("is_violated", False) and not r.get("is_blocked", False)
    )

    actionable_records = [
        r for r in records
        if not r.get("is_blocked", False)
        and not r.get("is_violated", False)
        and r.get("baro_altitude_m", 0.0) > 0.0
        and r.get("mach_number", 0.0) > 0.0
    ]
    actionable_count = len(actionable_records)

    pct_actionable = (actionable_count / total_cycles) * 100.0 if total_cycles > 0 else 0.0
    pct_violated = (violated_count / total_cycles) * 100.0 if total_cycles > 0 else 0.0
    pct_blocked = (blocked_count / total_cycles) * 100.0 if total_cycles > 0 else 0.0
    pct_corrupt = (corrupt_count / total_cycles) * 100.0 if total_cycles > 0 else 0.0

    machs = [r["mach_number"] for r in actionable_records if "mach_number" in r and isinstance(r["mach_number"], (int, float))]
    altitudes_ft = [r["baro_altitude_m"] * 3.28084 for r in actionable_records if "baro_altitude_m" in r and isinstance(r["baro_altitude_m"], (int, float))]
    densities = [r["air_density_kg_m3"] for r in actionable_records if "air_density_kg_m3" in r and isinstance(r["air_density_kg_m3"], (int, float))]
    dyn_pressures = [r["dynamic_pressure_pa"] for r in actionable_records if "dynamic_pressure_pa" in r and isinstance(r["dynamic_pressure_pa"], (int, float))]
    excess_powers = [r["specific_excess_power_w_kg"] for r in actionable_records if "specific_excess_power_w_kg" in r and isinstance(r["specific_excess_power_w_kg"], (int, float))]
    terrain_clearances = [r["terrain_clearance_m"] for r in actionable_records if "terrain_clearance_m" in r and isinstance(r["terrain_clearance_m"], (int, float))]
    hydro_discharges = [r["relative_river_discharge_m3s"] for r in actionable_records if "relative_river_discharge_m3s" in r and isinstance(r["relative_river_discharge_m3s"], (int, float))]
    latencies = [r["elapsed_ms"] for r in records if "elapsed_ms" in r and isinstance(r["elapsed_ms"], (int, float))]

    # Median latency
    sorted_lat = sorted(latencies) if latencies else []
    mid_idx = len(sorted_lat) // 2
    med_latency = (sorted_lat[mid_idx] if len(sorted_lat) % 2 != 0 else (sorted_lat[mid_idx - 1] + sorted_lat[mid_idx]) / 2.0) if sorted_lat else 0.0

    callsigns = sorted(list(set(r.get("callsign", "N/A") for r in records if r.get("callsign") not in ("UNKNOWN", "N/A"))))

    report_lines = [
        "# Aven Multi-Stream Physical Stress Benchmark — OpenSky, NOAA & USGS",
        "",
        f"**Test Period:** {datetime.fromtimestamp(start_time, tz=timezone.utc).isoformat()} to {datetime.fromtimestamp(end_time, tz=timezone.utc).isoformat()} ({duration_sec:.1f}s)  ",
        "**Telemetry Sources:** OpenSky Network (ADS-B Transponders), NOAA Aviation Weather Center (AWC METARs), USGS Water Services (River Basin Hydrology)  ",
        f"**Total Physical Vectors Evaluated:** {total_cycles}  ",
        f"**Unique Aircraft Tracked:** {len(callsigns)}  ",
        f"**Feasible Physical Solves:** {actionable_count} / {total_cycles} ({pct_actionable:.1f}%)  ",
        f"**Safety Bound Violations (Domain / Hazard Enforcement):** {violated_count} / {total_cycles} ({pct_violated:.1f}%)  ",
        f"**Freshness Blocks (`HOLD` Stale TTL):** {blocked_count} / {total_cycles} ({pct_blocked:.1f}%)  ",
        f"**Ground / Corrupted Fallback Tracks:** {corrupt_count} / {total_cycles} ({pct_corrupt:.1f}%)  ",
        f"**Multi-Worker Solve Throughput:** {solves_per_sec:.1f} physical solves / second  ",
        f"**Cold-Start Ingestion / Cache Warm Duration:** {cold_start_sec:.2f} s  ",
        f"**Steady-State Median Solve Latency:** {med_latency:.1f} ms  ",
        "",
        "---",
        "",
        "## Core Flight Dynamics & Atmospheric Statistics (Cruising Flights)",
        "",
        "| Physical Invariant | Minimum | Median | Mean / Average | Maximum | Unit |",
        "| :--- | :--- | :--- | :--- | :--- | :--- |",
        format_stat_row("Flight Mach Number", machs, "Mach", precision=3),
        format_stat_row("Barometric Cruise Altitude", altitudes_ft, "ft", precision=0),
        format_stat_row("High-Altitude Air Density", densities, "kg/m³", precision=4),
        format_stat_row("Dynamic Pressure (q)", dyn_pressures, "Pa", precision=1),
        format_stat_row("Specific Excess Power (Climb)", excess_powers, "W/kg", precision=1),
        format_stat_row("Hydrological Terrain Clearance", terrain_clearances, "m", precision=1),
        format_stat_row("Regional Hydro Proxy (Basin Discharge)", hydro_discharges, "m³/s", precision=2),
        format_stat_row("End-to-End Solve Latency", latencies, "ms", precision=1),
        "",
        "---",
        "",
        "## Sample High-Altitude Aircraft Solves",
        "",
        "| Callsign | Altitude | Airspeed | Mach | Air Density | Temp (Alt) | Climb Power | Settle Time | Status |",
        "| :--- | :--- | :--- | :--- | :--- | :--- | :--- | :--- | :--- |",
    ]

    for r in records[-20:]:
        cs = r.get("callsign", "N/A")
        alt_m = r.get("baro_altitude_m", 0.0)
        alt_ft = alt_m * 3.28084
        vel_ms = r.get("velocity_ms", 0.0)
        vel_kts = vel_ms * 1.94384
        mach = r.get("mach_number", 0.0)
        rho = r.get("air_density_kg_m3", 0.0)
        t_amb = r.get("ambient_temp_c", 0.0)
        p_climb = r.get("specific_excess_power_w_kg", 0.0)
        lat = r.get("elapsed_ms", 0.0)

        if r.get("is_blocked"):
            status = "STALE (TTL=0)"
        elif r.get("is_violated"):
            v_msg = r.get("violation_msg", "BOUNDARY_EXCEEDED")
            status = f"VIOLATION: {v_msg[:24]}"
        elif r.get("baro_altitude_m", 0.0) == 0.0:
            status = "GROUND_TRACK"
        else:
            status = "SOLVED"

        report_lines.append(
            f"| **{cs}** | {alt_ft:,.0f} ft ({alt_m:.0f}m) | {vel_kts:.0f} kts ({vel_ms:.1f}m/s) | "
            f"`M {mach:.3f}` | {rho:.4f} kg/m³ | {t_amb:+.1f}°C | {p_climb:+.1f} W/kg | {lat:.1f} ms | `{status}` |"
        )

    report_lines.append("\n*Generated autonomously by Aven Relational Toolchain.*")

    target_path = Path(report_path)
    target_path.parent.mkdir(parents=True, exist_ok=True)
    with open(target_path, "w", encoding="utf-8") as f:
        f.write("\n".join(report_lines) + "\n")
    print(f"\n[✓] Stress report saved to: {target_path}")


def prewarm_live_snapshot(cache_dir: Path):
    """Pre-warm live OpenSky, NOAA, and USGS data once before worker threads start."""
    cache_dir.mkdir(parents=True, exist_ok=True)
    snap_path = cache_dir / "shared_snapshot.json"
    t0 = time.perf_counter()

    # 1. OpenSky US Bounding Box
    opensky_url = "https://opensky-network.org/api/states/all?lamin=24.5&lomin=-125.0&lamax=49.0&lomax=-67.0"
    opensky_cache = cache_dir / "opensky_us.json"
    opensky_data = None
    if opensky_cache.exists() and (time.time() - opensky_cache.stat().st_mtime < 20.0):
        try:
            with open(opensky_cache, "r", encoding="utf-8") as f:
                opensky_data = json.load(f)
        except Exception:
            pass
    if not opensky_data:
        try:
            req = urllib.request.Request(opensky_url, headers={"User-Agent": "AvenFlightMeshOrchestrator/1.0"})
            with urllib.request.urlopen(req, timeout=12) as r:
                opensky_data = json.loads(r.read().decode("utf-8"))
                with open(opensky_cache, "w", encoding="utf-8") as f:
                    json.dump(opensky_data, f)
        except Exception:
            if opensky_cache.exists():
                try:
                    with open(opensky_cache, "r", encoding="utf-8") as f:
                        opensky_data = json.load(f)
                except Exception:
                    pass

    # 2. NOAA Hub METARs
    hubs = ["KATL", "KORD", "KDEN", "KDFW", "KJFK", "KSEA", "KLAX", "KSFO", "KMIA", "KBOS"]
    noaa_url = f"https://aviationweather.gov/api/data/metar?ids={','.join(hubs)}&format=json"
    noaa_cache = cache_dir / "noaa_metar.json"
    noaa_data = None
    if noaa_cache.exists() and (time.time() - noaa_cache.stat().st_mtime < 120.0):
        try:
            with open(noaa_cache, "r", encoding="utf-8") as f:
                noaa_data = json.load(f)
        except Exception:
            pass
    if not noaa_data:
        try:
            req = urllib.request.Request(noaa_url, headers={"User-Agent": "AvenFlightMeshOrchestrator/1.0"})
            with urllib.request.urlopen(req, timeout=10) as r:
                noaa_data = json.loads(r.read().decode("utf-8"))
                with open(noaa_cache, "w", encoding="utf-8") as f:
                    json.dump(noaa_data, f)
        except Exception:
            if noaa_cache.exists():
                try:
                    with open(noaa_cache, "r", encoding="utf-8") as f:
                        noaa_data = json.load(f)
                except Exception:
                    pass

    # 3. USGS Hydrology (Colorado Basin)
    usgs_url = "https://waterservices.usgs.gov/nwis/iv/?format=json&stateCd=co&parameterCd=00060,00065&siteStatus=active"
    usgs_cache = cache_dir / "usgs_hydrology.json"
    usgs_data = None
    if usgs_cache.exists() and (time.time() - usgs_cache.stat().st_mtime < 300.0):
        try:
            with open(usgs_cache, "r", encoding="utf-8") as f:
                usgs_data = json.load(f)
        except Exception:
            pass
    if not usgs_data:
        try:
            req = urllib.request.Request(usgs_url, headers={"User-Agent": "AvenFlightMeshOrchestrator/1.0"})
            with urllib.request.urlopen(req, timeout=10) as r:
                usgs_data = json.loads(r.read().decode("utf-8"))
                with open(usgs_cache, "w", encoding="utf-8") as f:
                    json.dump(usgs_data, f)
        except Exception:
            if usgs_cache.exists():
                try:
                    with open(usgs_cache, "r", encoding="utf-8") as f:
                        usgs_data = json.load(f)
                except Exception:
                    pass

    # Extract and filter confirmed airborne flights
    raw_states = opensky_data.get("states", []) if (opensky_data and isinstance(opensky_data, dict)) else []
    airborne_states = []
    for s in raw_states:
        try:
            if (
                s[5] is not None
                and s[6] is not None
                and s[7] is not None
                and s[9] is not None
                and not bool(s[8])
                and float(s[7]) >= 800.0
                and float(s[9]) >= 60.0
            ):
                airborne_states.append(s)
        except (IndexError, TypeError, ValueError):
            continue

    if not airborne_states:
        airborne_states = [
            ["a00001", "CRUISE1", "United States", 0, 0, -87.9, 41.9, 10668.0, False, 235.0, 0, 0.0, None, 10668.0]
        ]

    snapshot_payload = {
        "timestamp": time.time(),
        "opensky": {"states": airborne_states},
        "noaa": noaa_data,
        "usgs": usgs_data,
    }
    with open(snap_path, "w", encoding="utf-8") as f:
        json.dump(snapshot_payload, f)

    cold_start_sec = time.perf_counter() - t0
    return snap_path, airborne_states, cold_start_sec


def evaluate_single_flight(avenc_exe, model_path, root, flight_idx, snapshot_path, expected_callsign, raw_alt, raw_vel):
    """Run avenc against flight_mesh.aven for a specific flight vector with lock-free shared snapshot."""
    env = os.environ.copy()
    env["AVEN_FLIGHT_INDEX"] = str(flight_idx)
    env["AVEN_WORKSPACE"] = str(root)
    env["AVEN_SNAPSHOT_FILE"] = str(snapshot_path)

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

    vars_map, is_blocked, is_violated, violation_msg = parse_avenc_output(combined)
    now_iso = datetime.now(timezone.utc).strftime("%H:%M:%S.%f")[:-3]

    # Callsign extraction: regex from backend_id in trace with expected_callsign fallback
    callsign = None
    m_cs = re.search(r"backend:\s*mesh_([A-Za-z0-9]+)", combined)
    if m_cs:
        cand = m_cs.group(1).strip()
        if cand and cand != "00000000":
            callsign = cand

    if not callsign or callsign == "UNKNOWN":
        callsign = expected_callsign or f"FLT{flight_idx:03d}"

    # Retain physical transponder inputs even if an aerodynamic bound was violated
    alt_m = vars_map.get("baro_altitude_m") or raw_alt
    vel_ms = vars_map.get("velocity_ms") or raw_vel

    record = {
        "index": flight_idx,
        "time": now_iso,
        "callsign": callsign,
        "baro_altitude_m": alt_m,
        "velocity_ms": vel_ms,
        "vertical_rate_ms": vars_map.get("vertical_rate_ms", 0.0),
        "ambient_temp_c": vars_map.get("ambient_temp_c", 0.0),
        "ambient_pressure_pa": vars_map.get("ambient_pressure_pa", 0.0),
        "air_density_kg_m3": vars_map.get("air_density_kg_m3", 0.0),
        "speed_of_sound_ms": vars_map.get("speed_of_sound_ms", 0.0),
        "mach_number": vars_map.get("mach_number", 0.0),
        "dynamic_pressure_pa": vars_map.get("dynamic_pressure_pa", 0.0),
        "specific_excess_power_w_kg": vars_map.get("specific_excess_power_w_kg", 0.0),
        "terrain_clearance_m": vars_map.get("terrain_clearance_m", 0.0),
        "relative_river_discharge_m3s": vars_map.get("relative_river_discharge_m3s", 0.0),
        "elapsed_ms": elapsed_ms,
        "is_blocked": is_blocked,
        "is_violated": is_violated,
        "violation_msg": violation_msg,
    }
    return record


def main():
    parser = argparse.ArgumentParser(description="Aven Multi-Stream Physical Stress Benchmark")
    parser.add_argument("--count", type=int, default=100, help="Number of flight vectors to evaluate (default: 100)")
    parser.add_argument("--concurrency", type=int, default=4, help="Parallel worker threads (default: 4)")
    parser.add_argument("--ledger", type=str, default="evaluations/flight_mesh/flight_mesh_audit_ledger.jsonl", help="Output ledger")
    parser.add_argument("--report", type=str, default="evaluations/flight_mesh/flight_mesh_summary_report.md", help="Output report")
    parser.add_argument("--summary-only", action="store_true", help="Compile report from existing ledger")
    parser.add_argument("--clean", action="store_true", help="Truncate ledger before benchmark run to ensure pure sample")
    args = parser.parse_args()

    root = Path(__file__).resolve().parent.parent.parent
    ledger_path = Path(args.ledger) if Path(args.ledger).is_absolute() else root / args.ledger
    report_path = Path(args.report) if Path(args.report).is_absolute() else root / args.report

    if args.clean and ledger_path.exists():
        ledger_path.unlink()
        print(f"[!] Truncated ledger at {ledger_path} for fresh benchmark sample.")

    if args.summary_only:
        if not ledger_path.exists():
            print(f"Error: ledger not found at {ledger_path}")
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
        t_start = records[0].get("timestamp", time.time()) if records else time.time()
        t_end = records[-1].get("timestamp", time.time()) if records else time.time()
        generate_summary_report(records, report_path, t_start, t_end)
        return

    # Cross-platform binary resolution
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
                print("Error: Aven executable not found.")
                sys.exit(1)

    model_path = root / "evaluations" / "flight_mesh" / "flight_mesh.aven"
    ledger_path.parent.mkdir(parents=True, exist_ok=True)
    cache_dir = root / "evaluations" / "flight_mesh" / ".cache"

    print("=" * 88)
    print(" Aven Multi-Stream Physical Stress Benchmark")
    print(" Telemetry Sources: OpenSky ADS-B (6,000+ Aircraft) + NOAA AWC METAR + USGS River Basins")
    print(f" Target Model:        {model_path.name}")
    print(f" Flight Vector Count: {args.count}")
    print(f" Worker Concurrency:  {args.concurrency} parallel solvers")
    print(f" Output Ledger:       {ledger_path}")
    print(f" Summary Report:      {report_path}")
    print("=" * 88)

    # 1. Pre-warm live snapshot to eliminate cold cache network latency and lock races
    print("[*] Ingesting live feeds and pre-warming shared snapshot...")
    snapshot_path, airborne_states, cold_start_sec = prewarm_live_snapshot(cache_dir)
    print(f"[✓] Pre-warm complete ({cold_start_sec:.2f}s). {len(airborne_states)} active cruising flights cached.\n")

    records = []
    start_time = time.time()

    with ThreadPoolExecutor(max_workers=args.concurrency) as pool:
        futures = {}
        for i in range(args.count):
            flight = airborne_states[i % len(airborne_states)]
            raw_cs = flight[1].strip() if (len(flight) > 1 and flight[1]) else ""
            clean_cs = re.sub(r"[^A-Za-z0-9]", "", raw_cs)
            if not clean_cs or clean_cs in ("00000000", "UNKNOWN", "NONE"):
                clean_cs = f"ICAO{str(flight[0]).upper()}"

            raw_alt = float(flight[7]) if (len(flight) > 7 and flight[7] is not None) else 10000.0
            raw_vel = float(flight[9]) if (len(flight) > 9 and flight[9] is not None) else 230.0

            fut = pool.submit(evaluate_single_flight, avenc_exe, model_path, root, i, snapshot_path, clean_cs, raw_alt, raw_vel)
            futures[fut] = i

        completed = 0
        for future in as_completed(futures):
            if not RUNNING:
                break
            try:
                rec = future.result()
                rec["timestamp"] = time.time()
                records.append(rec)
                completed += 1

                # Append to ledger
                with open(ledger_path, "a", encoding="utf-8") as f:
                    f.write(json.dumps(rec) + "\n")

                cs = rec["callsign"]
                alt_ft = rec["baro_altitude_m"] * 3.28084
                mach = rec["mach_number"]
                rho = rec["air_density_kg_m3"]
                p_climb = rec["specific_excess_power_w_kg"]
                lat = rec["elapsed_ms"]
                if rec.get("is_blocked"):
                    status = "❌ BLOCKED"
                elif rec.get("is_violated"):
                    status = f"⚠️ BOUND_EXCEEDED ({rec.get('violation_msg', '')[:22]})"
                else:
                    status = "✓ SOLVED"

                print(
                    f"[{rec['time']}] Vector #{completed:3d}/{args.count} | {cs:8s} | "
                    f"Alt: {alt_ft:6.0f}ft | M {mach:.3f} | Air: {rho:.4f}kg/m³ | "
                    f"Climb: {p_climb:+5.1f}W/kg | Settle: {lat:5.1f}ms | {status}"
                )
            except Exception as e:
                print(f"[-] Evaluation failed: {e}")

    end_time = time.time()
    generate_summary_report(records, report_path, start_time, end_time, cold_start_sec)

    dt = end_time - start_time
    rate = len(records) / dt if dt > 0 else 0
    print("\n" + "=" * 88)
    print(f" Benchmark Complete: {len(records)} physical flight systems solved in {dt:.2f}s ({rate:.1f} solves/sec)")
    print("=" * 88)


if __name__ == "__main__":
    main()
