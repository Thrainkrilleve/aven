#!/usr/bin/env python3
"""verify/weather_reference.py — Checks weather-predict.aven against independent references.

For random but realistic atmospheric states, solves the shipped WeatherCell relation with
`avenc run --json` and compares:

  thermodynamics   saturation/actual vapor pressure, relative humidity, specific humidity,
                   virtual temperature, density: exactly against the published formulas
                   (Ambaum 2020 saturation curve, exact q and T_v) and against MetPy (Unidata),
                   which implements the same physics with the same constants
  kinematics       the plane fit Aven solves through three stations: against an exact NumPy
                   solve of the same 3x3 systems, plus divergence/vorticity/deformation
  radar            Marshall-Palmer inverse (dBZ -> rain rate) against the closed form, now and
                   at each nowcast lead, with no rain below 10 dBZ (checked at exactly 10 and 0)

Usage:  .venv/Scripts/python verify/weather_reference.py --cases 150 --seed 1
"""

import argparse
import json
import math
import os
import random
import subprocess
import sys
import tempfile
from pathlib import Path

import numpy as np
import metpy.calc as mpcalc
from metpy.units import units

ROOT = Path(__file__).resolve().parent.parent
AVENC = str(ROOT / "target" / "release" / ("avenc.exe" if os.name == "nt" else "avenc"))
MODEL = ROOT / "evaluations" / "possible" / "weather-predict.aven"
FORWARD = ROOT / "examples" / "weather_nowcast.aven"


def physics_relation():
    src = MODEL.read_text(encoding="utf-8")
    start = src.index("relation WeatherCell")
    end = src.index("\n}", start) + 2
    return src[start:end]


def forward_block():
    src = FORWARD.read_text(encoding="utf-8")
    start = src.index("forward WeatherNowcast")
    end = src.index("\n}", start) + 2
    return src[start:end]


def run_env(src):
    with tempfile.NamedTemporaryFile("w", suffix=".aven", delete=False, encoding="utf-8") as f:
        f.write(src)
        path = f.name
    r = subprocess.run([AVENC, "run", path, "--json"], capture_output=True, text=True)
    os.unlink(path)
    if r.returncode != 0:
        return None, f"avenc exit {r.returncode}: {r.stderr.strip()[-200:]}"
    return json.loads(r.stdout.strip().splitlines()[-1])["env"], None


def ambaum(t_c):
    """Saturation vapor pressure over liquid water (hPa), Ambaum (2020) Eq. 13."""
    t = t_c + 273.15
    cpl, cpv, rv, l0, t0 = 4219.4, 1860.078011865639, 461.52311572606084, 2500840.0, 273.16
    latent = l0 - (cpl - cpv) * (t - t0)
    return 6.112 * (t0 / t) ** ((cpl - cpv) / rv) * math.exp((l0 / t0 - latent / t) / rv)


EPS, RD = 0.6219569100577033, 287.04749097718457


def marshall_palmer(dbz):
    """Rain rate (mm/h) from Z = 200 R^1.6; 0 below 10 dBZ, where there is no measurable rain."""
    return (10 ** (dbz / 10) / 200) ** (1 / 1.6) if dbz >= 10 else 0.0


def check(name, got, want, rel, failures, case):
    err = abs(got - want) / max(abs(want), 1e-30)
    if err > rel:
        failures.append(f"case {case} {name}: avenc {got!r} vs {want!r} (rel err {err:.2e} > {rel:.0e})")
    return err


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--cases", type=int, default=100)
    ap.add_argument("--seed", type=int, default=1)
    args = ap.parse_args()
    rng = random.Random(args.seed)
    relation = physics_relation()
    forward = forward_block()
    failures = []
    worst = {}

    for case in range(args.cases):
        t = rng.uniform(-30, 40)
        td = t - rng.uniform(0, 25)
        p = rng.uniform(600, 1040)
        rh_rep = 100 * ambaum(td) / ambaum(t) + rng.uniform(-1, 1)
        # Triangle of stations within ~60 km and a smooth wind field with known gradients
        pts = [(rng.uniform(-60e3, 60e3), rng.uniform(-60e3, 60e3)) for _ in range(3)]
        g = {k: rng.uniform(-3e-4, 3e-4) for k in ("du_dx", "du_dy", "dv_dx", "dv_dy")}
        u0, v0 = rng.uniform(-15, 15), rng.uniform(-15, 15)
        winds = [(u0 + g["du_dx"] * x + g["du_dy"] * y, v0 + g["dv_dx"] * x + g["dv_dy"] * y) for x, y in pts]
        dbz = rng.uniform(5, 60)
        if case < 2:
            dbz = (10.0, 0.0)[case]  # the rain threshold itself, and no echo
        b = {
            "temp_c": t, "dewpoint_c": td, "rh_reported": rh_rep, "pressure_hpa": p,
            "wind_u": winds[0][0], "wind_v": winds[0][1], "aloft_u": rng.uniform(-30, 30),
            "aloft_v": rng.uniform(-30, 30), "aloft_height_m": rng.uniform(500, 3000),
            "steer_u": rng.uniform(-25, 25), "steer_v": rng.uniform(-25, 25),
            "radar_dbz": dbz, "gauge_available": 0, "gauge_rate_mmh": 0,
            "upstream_dbz_15": rng.uniform(5, 60), "upstream_dbz_30": rng.uniform(5, 60),
            "upstream_dbz_60": rng.uniform(5, 60), "layer_depth_m": 1000.0,
        }
        for n, ((x, y), (u, v)) in enumerate(zip(pts, winds), start=1):
            b.update({f"x{n}": x, f"y{n}": y, f"u{n}": u, f"v{n}": v})
        src = relation + "\nsolve WeatherCell with {\n" + "\n".join(f"    {k} = {v!r}" for k, v in b.items()) + "\n}\n"
        env, err = run_env(src)
        if err:
            failures.append(f"case {case}: {err}")
            continue

        # The forward block (what avenc.dev/weather runs as WebAssembly) on the same state
        fb = dict(b)
        fb.update({
            "metar_altimeter_hpa": rng.uniform(980, 1040), "station_elev_m": rng.uniform(0, 2500),
            "nws_pressure_hpa": rng.uniform(600, 1040),
            "cell_u": rng.uniform(-25, 25), "cell_v": rng.uniform(-25, 25),
            **{f"cell_upstream_dbz_{l}": rng.uniform(0, 60) for l in (15, 30, 60)},
        })
        fwd, err = run_env(forward + "\nsolve WeatherNowcast with {\n" + "\n".join(f"    {k} = {v!r}" for k, v in fb.items()) + "\n}\n")
        if err:
            failures.append(f"case {case} forward block: {err}")
            continue

        def note(name, err):
            worst[name] = max(worst.get(name, 0.0), err)

        # Published formulas: must match to floating-point precision
        e_s, e = ambaum(t), ambaum(td)
        q = EPS * e / (p - (1 - EPS) * e)
        tv = (t + 273.15) * (1 - q + q / EPS)
        exact = {
            "e_sat_hpa": e_s, "vapor_pressure_hpa": e, "relative_humidity": 100 * e / e_s,
            "specific_humidity": q, "virtual_temp_k": tv,
            "air_density_kg_m3": 100 * p / (RD * tv),
            "radar_rain_mmh": marshall_palmer(dbz),
            **{f"nowcast_rain_{l}_mmh": marshall_palmer(b[f"upstream_dbz_{l}"]) for l in (15, 30, 60)},
        }
        for k, want in exact.items():
            note(f"{k} (formula)", check(k, env[k], want, 1e-9, failures, case))

        # MetPy: independent implementation (Ambaum 2020 saturation curve) within tolerances
        T, Td, P = t * units.degC, td * units.degC, p * units.hPa
        mp = {
            "e_sat_hpa": (mpcalc.saturation_vapor_pressure(T).m_as("hPa"), 1e-9),
            "relative_humidity": (100 * mpcalc.relative_humidity_from_dewpoint(T, Td).m_as(""), 1e-9),
            "specific_humidity": (mpcalc.specific_humidity_from_dewpoint(P, Td).m_as(""), 1e-9),
            "virtual_temp_k": (mpcalc.virtual_temperature_from_dewpoint(P, T, Td).m_as("K"), 1e-9),
            "air_density_kg_m3": (mpcalc.density(P, T, mpcalc.mixing_ratio_from_specific_humidity(
                mpcalc.specific_humidity_from_dewpoint(P, Td))).m_as("kg/m^3"), 1e-9),
        }
        for k, (want, tol) in mp.items():
            note(f"{k} (MetPy)", check(f"{k} vs MetPy", env[k], want, tol, failures, case))

        # Kinematics: exact plane fit through the three stations
        A = np.array([[1.0, x, y] for x, y in pts])
        cu = np.linalg.solve(A, [w[0] for w in winds])
        cv = np.linalg.solve(A, [w[1] for w in winds])
        kin = {
            "du_dx": cu[1], "du_dy": cu[2], "dv_dx": cv[1], "dv_dy": cv[2],
            "divergence_per_s": cu[1] + cv[2], "vorticity_per_s": cv[1] - cu[2],
            "deformation_per_s": math.hypot(cu[1] - cv[2], cv[1] + cu[2]),
        }
        for k, want in kin.items():
            err = abs(env[k] - want) / max(abs(want), 1e-6)
            note(f"{k} (NumPy)", err)
            if err > 1e-6:
                failures.append(f"case {case} {k}: avenc {env[k]!r} vs NumPy {want!r}")

        # Forward block vs the relational model: same physics, closed forms instead of solving
        shared = ["e_sat_hpa", "vapor_pressure_hpa", "relative_humidity", "rh_residual", "specific_humidity",
                  "virtual_temp_k", "air_density_kg_m3", "dp_dz_pa_per_m", "vertical_shear_ms_per_km",
                  "steering_speed_ms", "radar_rain_mmh", "radar_minus_gauge_mmh",
                  "nowcast_rain_15_mmh", "nowcast_rain_30_mmh", "nowcast_rain_60_mmh"]
        for k in shared:
            note(f"{k} (forward vs relation)", check(f"{k} forward vs relation", fwd[k], env[k], 1e-9, failures, case))
        for k in ("du_dx", "du_dy", "dv_dx", "dv_dy", "divergence_per_s", "vorticity_per_s", "deformation_per_s", "vertical_velocity_ms"):
            err = abs(fwd[k] - kin.get(k, env[k])) / max(abs(kin.get(k, env[k])), 1e-6)
            note(f"{k} (forward vs NumPy)", err)
            if err > 1e-6:
                failures.append(f"case {case} forward {k}: {fwd[k]!r} vs {kin.get(k, env[k])!r}")
        extra = {
            "metar_station_pressure_hpa": fb["metar_altimeter_hpa"] * ((288.15 - 0.0065 * fb["station_elev_m"]) / 288.15) ** 5.2561,
            "steering_vs_cell_ms": math.hypot(fb["steer_u"] - fb["cell_u"], fb["steer_v"] - fb["cell_v"]),
            "pressure_residual_hpa": abs(fb["nws_pressure_hpa"] - fb["metar_altimeter_hpa"] * ((288.15 - 0.0065 * fb["station_elev_m"]) / 288.15) ** 5.2561),
            **{f"cell_nowcast_rain_{l}_mmh": marshall_palmer(fb[f"cell_upstream_dbz_{l}"]) for l in (15, 30, 60)},
        }
        for k, want in extra.items():
            note(f"{k} (formula)", check(k, fwd[k], want, 1e-9, failures, case))

    print(f"{args.cases} random states; worst relative error per quantity:")
    for k, v in sorted(worst.items()):
        print(f"  {k:36s} {v:.2e}")
    print(f"failures: {len(failures)}")
    for f in failures[:10]:
        print("  " + f)
    sys.exit(1 if failures else 0)


if __name__ == "__main__":
    main()
