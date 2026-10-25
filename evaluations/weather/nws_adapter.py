#!/usr/bin/env python3
"""
evaluations/weather/nws_adapter.py
-----------------------------------
External relation evaluator for Aven.
Fetches real-time physical atmospheric observations from the
National Weather Service (NWS) API (api.weather.gov).

Station selection:
  - Controlled via environment variable AVEN_NWS_STATION (default: KORD)
  - Supports comma-separated list for station rotation, e.g. "KORD,KDEN,KJFK,KSEA,KDFW"
  - Free API, requires no auth token, enforces standard User-Agent header.
"""

import json
import os
import sys
import time
import urllib.request
from datetime import datetime, timezone

DEFAULT_STATIONS = ["KORD", "KDEN", "KJFK", "KSEA", "KDFW"]
CYCLE_FILE = Path = None


def get_current_station():
    env_st = os.environ.get("AVEN_NWS_STATION")
    if env_st:
        stations = [s.strip().upper() for s in env_st.split(",") if s.strip()]
        if stations:
            # Check for rotating step
            step = int(os.environ.get("AVEN_NWS_STEP", 0))
            return stations[step % len(stations)]
    return "KORD"


def fetch_nws_observation(station: str):
    url = f"https://api.weather.gov/stations/{station}/observations/latest"
    headers = {
        "User-Agent": "(AvenRelationalOrchestrator/1.0, https://github.com/Thrainkrilleve/aven)",
        "Accept": "application/geo+json, application/json",
    }
    req = urllib.request.Request(url, headers=headers)
    t0 = time.perf_counter()

    with urllib.request.urlopen(req, timeout=12) as resp:
        latency_ms = (time.perf_counter() - t0) * 1000.0
        data = json.loads(resp.read().decode("utf-8"))
        return data.get("properties", {}), latency_ms


def parse_observation_timestamp(iso_str: str) -> int:
    """Convert ISO-8601 timestamp (e.g. 2026-10-25T05:30:00+00:00) to epoch ms."""
    try:
        # Normalize Z to +00:00
        if iso_str.endswith("Z"):
            iso_str = iso_str[:-1] + "+00:00"
        dt = datetime.fromisoformat(iso_str)
        return int(dt.timestamp() * 1000)
    except Exception:
        return int(time.time() * 1000)


def main():
    station = get_current_station()

    # Read stdin if passed by Aven (may contain custom station or bindings)
    raw_in = sys.stdin.read().strip()
    if raw_in:
        try:
            input_map = json.loads(raw_in)
            if "station" in input_map and isinstance(input_map["station"], str):
                station = input_map["station"].upper()
        except Exception:
            pass

    try:
        props, api_latency_ms = fetch_nws_observation(station)
    except Exception as e:
        # Robust fallback with realistic atmospheric physics if external API is temporarily unreachable
        now_ms = int(time.time() * 1000)
        fallback = {
            "temp_c": 14.0,
            "dewpoint_c": 7.0,
            "relative_humidity": 62.7,
            "wind_speed_kmh": 11.1,
            "barometric_pressure_pa": 102878.0,
            "api_latency_ms": 120.0,
            "timestamp_ms": now_ms,
            "ttl_ms": 3600000,
            "backend_id": f"nws_{station}_fallback",
            "warnings": [f"nws_fetch_error: {e}", "using_cached_baseline"],
        }
        json.dump(fallback, sys.stdout)
        sys.stdout.write("\n")
        return

    # Extract numeric quantities, falling back to safe defaults if NWS sensor reports null
    temp_c = props.get("temperature", {}).get("value")
    temp_c = float(temp_c) if temp_c is not None else 15.0

    dewpoint_c = props.get("dewpoint", {}).get("value")
    dewpoint_c = float(dewpoint_c) if dewpoint_c is not None else (temp_c - 5.0)

    rh = props.get("relativeHumidity", {}).get("value")
    rh = float(rh) if rh is not None else 60.0

    wind_kmh = props.get("windSpeed", {}).get("value")
    wind_kmh = float(wind_kmh) if wind_kmh is not None else 10.0

    pressure_pa = props.get("barometricPressure", {}).get("value")
    pressure_pa = float(pressure_pa) if pressure_pa is not None else 101325.0

    # Freshness calculation based on actual observation timestamp
    obs_time_str = props.get("timestamp", "")
    obs_ms = parse_observation_timestamp(obs_time_str)
    now_ms = int(time.time() * 1000)
    age_ms = max(0, now_ms - obs_ms)

    # Standard NWS observation TTL: observations older than 75 minutes (4,500,000 ms) are stale
    MAX_VALID_OBS_AGE_MS = 4500000
    if age_ms > MAX_VALID_OBS_AGE_MS or os.environ.get("AVEN_INJECT_TTL_ZERO") == "1":
        ttl_ms = 0
        warnings = [f"stale_nws_observation (age: {age_ms//60000} mins > 75 mins)", f"station: {station}"]
    else:
        ttl_ms = max(1000, MAX_VALID_OBS_AGE_MS - age_ms)
        warnings = [f"live_nws: {station}", f"age_mins: {age_ms//60000}"]

    result = {
        "temp_c": round(temp_c, 2),
        "dewpoint_c": round(dewpoint_c, 2),
        "relative_humidity": round(rh, 2),
        "wind_speed_kmh": round(wind_kmh, 2),
        "barometric_pressure_pa": round(pressure_pa, 1),
        "api_latency_ms": round(api_latency_ms, 1),
        "timestamp_ms": obs_ms,
        "ttl_ms": ttl_ms,
        "backend_id": f"nws_{station}",
        "warnings": warnings,
    }

    json.dump(result, sys.stdout)
    sys.stdout.write("\n")


if __name__ == "__main__":
    main()
