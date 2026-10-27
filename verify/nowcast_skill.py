#!/usr/bin/env python3
"""verify/nowcast_skill.py — Scores the weather-predict.aven radar nowcast against reality.

Finds CONUS locations with radar echoes, runs the shipped model there (live government data
through weather_adapter.py), then reads the NOAA radar mosaic at each site when each forecast
becomes valid (+15, +30, +60 min). Every forecast is compared with the standard baseline,
Eulerian persistence (the echo at the site stays as it is now):

  MAE (dBZ)   mean absolute error of predicted vs observed reflectivity
  CSI         critical success index for rain/no-rain at 20 dBZ (hits / (hits + misses + FA))

Skill > 0 means the Lagrangian nowcast beats persistence. Results are appended to
verify/nowcast_results.jsonl as they arrive, so a long run can be inspected while going.

Usage:  .venv/Scripts/python verify/nowcast_skill.py --sites 8 --cycles 3 --interval-min 20
"""

import argparse
import json
import math
import os
import re
import subprocess
import sys
import tempfile
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "evaluations" / "possible"))
import weather_adapter as wa  # noqa: E402 - reuse the adapter's radar query

AVENC = str(ROOT / "target" / "release" / ("avenc.exe" if os.name == "nt" else "avenc"))
MODEL = ROOT / "evaluations" / "possible" / "weather-predict.aven"
RESULTS = ROOT / "verify" / "nowcast_results.jsonl"
LEADS = (15, 30, 60)
RAIN_DBZ = 20.0


def find_echo_sites(n):
    """Scan a coarse CONUS grid for echoes of at least 25 dBZ; keep sites spread apart."""
    from concurrent.futures import ThreadPoolExecutor

    grid = [(lat / 2, -lon / 2) for lat in range(52, 96, 3) for lon in range(140, 246, 3)]

    def probe(pt):
        try:
            return wa.radar_dbz(*pt)[0], pt[0], pt[1]
        except Exception:  # noqa: BLE001 - outside coverage
            return None

    with ThreadPoolExecutor(12) as pool:
        hits = [h for h in pool.map(probe, grid) if h and h[0] >= 25]
    hits.sort(reverse=True)
    chosen = []
    for dbz, lat, lon in hits:
        if all(math.hypot(lat - a, lon - b) > 2.5 for _, a, b in chosen):
            chosen.append((dbz, lat, lon))
        if len(chosen) == n:
            break
    return [(lat, lon) for _, lat, lon in chosen]


def forecast(lat, lon):
    src = MODEL.read_text(encoding="utf-8")
    src = re.sub(r"site_lat = \S+", f"site_lat = {lat}", src)
    src = re.sub(r"site_lon = \S+", f"site_lon = {lon}", src)
    path = MODEL.parent / f"_nowcast_{os.getpid()}.aven"   # adapter path is relative to the model
    path.write_text(src, encoding="utf-8")
    try:
        p = subprocess.run([AVENC, "run", str(path), "--json"], capture_output=True, text=True, timeout=180)
    finally:
        path.unlink(missing_ok=True)
    if p.returncode != 0:
        return None, p.stderr.strip()[-200:]
    return json.loads(p.stdout.strip().splitlines()[-1])["env"], None


def log(rec):
    with RESULTS.open("a", encoding="utf-8") as f:
        f.write(json.dumps(rec) + "\n")


def summarize():
    recs = [json.loads(l) for l in RESULTS.read_text(encoding="utf-8").splitlines() if l.strip()]
    obs = [r for r in recs if r.get("type") == "verification"]
    print(f"\n{len(obs)} verified forecasts")
    for lead in LEADS:
        rows = [r for r in obs if r["lead_min"] == lead]
        if not rows:
            continue

        def stats(key):
            mae = sum(abs(r[key] - r["observed_dbz"]) for r in rows) / len(rows)
            hit = sum(r[key] >= RAIN_DBZ and r["observed_dbz"] >= RAIN_DBZ for r in rows)
            miss = sum(r[key] < RAIN_DBZ <= r["observed_dbz"] for r in rows)
            fa = sum(r[key] >= RAIN_DBZ > r["observed_dbz"] for r in rows)
            csi = hit / (hit + miss + fa) if hit + miss + fa else float("nan")
            return mae, csi

        mae_n, csi_n = stats("nowcast_dbz")
        mae_p, csi_p = stats("persistence_dbz")
        skill = 1 - mae_n / mae_p if mae_p else float("nan")
        print(f"  +{lead:2d} min  n={len(rows):3d}  nowcast MAE {mae_n:5.2f} dBZ CSI {csi_n:.2f} | "
              f"persistence MAE {mae_p:5.2f} dBZ CSI {csi_p:.2f} | MAE skill vs persistence {skill:+.2f}")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--sites", type=int, default=8)
    ap.add_argument("--cycles", type=int, default=3)
    ap.add_argument("--interval-min", type=float, default=20)
    ap.add_argument("--summary", action="store_true", help="only print the summary of past results")
    args = ap.parse_args()
    if args.summary:
        summarize()
        return

    pending = []  # (valid_ms, issue record, lead)
    for cycle in range(args.cycles):
        sites = find_echo_sites(args.sites)
        print(f"[cycle {cycle}] {len(sites)} echo sites: {sites}", flush=True)
        for lat, lon in sites:
            env, err = forecast(lat, lon)
            issued = time.time() * 1000
            if env is None:
                print(f"  {lat},{lon}: no forecast ({err})", flush=True)
                log({"type": "error", "issued_ms": issued, "lat": lat, "lon": lon, "error": err})
                continue
            rec = {"type": "forecast", "issued_ms": issued, "lat": lat, "lon": lon,
                   "now_dbz": env["radar_dbz"], "steering_speed_ms": env["steering_speed_ms"],
                   **{f"nowcast_dbz_{l}": env[f"nowcast_dbz_{l}"] for l in LEADS}}
            log(rec)
            print(f"  {lat},{lon}: now {env['radar_dbz']:.0f} dBZ -> "
                  + ", ".join(f"+{l}m {env[f'nowcast_dbz_{l}']:.0f}" for l in LEADS), flush=True)
            for l in LEADS:
                pending.append((issued + l * 60_000, rec, l))
        next_cycle = time.time() + args.interval_min * 60
        # Verify everything that falls due before the next cycle (or all, after the last)
        while pending and (cycle == args.cycles - 1 or min(p[0] for p in pending) < next_cycle * 1000):
            pending.sort(key=lambda p: p[0])
            due, rec, lead = pending.pop(0)
            time.sleep(max(0.0, due / 1000 - time.time()))
            try:
                observed, radar_ms = wa.radar_dbz(rec["lat"], rec["lon"])
            except Exception as e:  # noqa: BLE001 - recorded, not invented
                log({"type": "error", "lat": rec["lat"], "lon": rec["lon"], "error": str(e)})
                continue
            log({"type": "verification", "lat": rec["lat"], "lon": rec["lon"], "lead_min": lead,
                 "issued_ms": rec["issued_ms"], "radar_ms": radar_ms, "observed_dbz": observed,
                 "nowcast_dbz": rec[f"nowcast_dbz_{lead}"], "persistence_dbz": rec["now_dbz"]})
        time.sleep(max(0.0, next_cycle - time.time()))
    summarize()


if __name__ == "__main__":
    main()
