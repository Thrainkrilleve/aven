#!/usr/bin/env python3
"""verify/live_atmosphere.py — Checks the flight model against live aircraft.

Requires the telemetry daemon (evaluations/flight_mesh/flight_mesh_daemon.py) on
127.0.0.1:9090. For each live aircraft index, runs the shipped flight_mesh_ipc.aven model
through avenc and recomputes temperature, pressure and Mach number independently from the
same raw telemetry with the two-layer International Standard Atmosphere (USSA76 layers 0–1).

Usage:  .venv/Scripts/python verify/live_atmosphere.py --flights 150
"""

import argparse
import json
import math
import os
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
AVENC = str(ROOT / "target" / "release" / ("avenc.exe" if os.name == "nt" else "avenc"))
MODEL = ROOT / "evaluations" / "flight_mesh" / "flight_mesh_ipc.aven"

G, R, L, GAMMA = 9.80665, 287.058, 0.0065, 1.4


def isa(surface_c, altim_hpa, h):
    """Independent two-layer ISA using the actual surface temperature and altimeter."""
    t0 = surface_c + 273.15
    h_trop = min(h, 11000.0)
    t = t0 - L * h_trop
    p = altim_hpa * 100.0 * (1 - L * h_trop / t0) ** 5.2561 * math.exp(-G * max(h - 11000.0, 0.0) / (R * t))
    return t, p


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--flights", type=int, default=100)
    args = ap.parse_args()

    text = MODEL.read_text()
    relation = text[text.index("relation"):text.index("\n}", text.index("relation")) + 2]
    solve = text[text.index("solve"):]
    constraints = solve[solve.index("{") + 1:solve.rindex("}")]

    checked = high = mismatches = errors = 0
    seen = set()
    for i in range(args.flights):
        src = f"{relation}\nsolve FlightAtmosphericDynamics with {{\n    flight_index = {i}\n{constraints}}}\n"
        with tempfile.NamedTemporaryFile("w", suffix=".aven", delete=False) as f:
            f.write(src)
            path = f.name
        p = subprocess.run([AVENC, "run", path, "--json"], capture_output=True, text=True)
        os.unlink(path)
        lines = p.stdout.strip().splitlines()
        if p.returncode != 0 or not lines or not lines[-1].startswith("{"):
            errors += 1
            print(f"#{i}: exit {p.returncode}: {p.stderr.strip()[-160:]}")
            continue
        env = json.loads(lines[-1])["env"]
        key = (env["baro_altitude_m"], env["velocity_ms"])
        if key in seen:
            continue  # daemon wraps around once it runs out of aircraft
        seen.add(key)
        h = env["baro_altitude_m"]
        t_ref, p_ref = isa(env["surface_temp_c"], env["surface_altim_hpa"], h)
        mach_ref = env["velocity_ms"] / math.sqrt(GAMMA * R * t_ref)
        bad = []
        if abs(env["ambient_temp_k"] - t_ref) > 1e-6:
            bad.append(f"T {env['ambient_temp_k']:.4f} vs {t_ref:.4f}")
        if abs(env["ambient_pressure_pa"] - p_ref) / p_ref > 1e-9:
            bad.append(f"p {env['ambient_pressure_pa']:.3f} vs {p_ref:.3f}")
        if abs(env["mach_number"] - mach_ref) > 1e-5:
            bad.append(f"M {env['mach_number']:.6f} vs {mach_ref:.6f}")
        checked += 1
        if h > 11000:
            high += 1
            print(f"#{i}: {h:.0f} m ({h / 0.3048:.0f} ft)  T={env['ambient_temp_c']:.2f} C  "
                  f"p={env['ambient_pressure_pa']:.0f} Pa  M={env['mach_number']:.3f}")
        if bad:
            mismatches += 1
            print(f"#{i} MISMATCH at {h:.0f} m: {'; '.join(bad)}")

    print(f"live aircraft checked: {checked} ({high} above the 11 km tropopause), "
          f"physics mismatches: {mismatches}, run errors: {errors}")
    sys.exit(1 if mismatches or errors else 0)


if __name__ == "__main__":
    main()
