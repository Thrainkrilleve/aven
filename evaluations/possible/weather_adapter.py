#!/usr/bin/env python3
"""evaluations/possible/weather_adapter.py — Live U.S. government weather inputs for avenc.

External adapter for weather-predict.aven (`using external("python", ...)`). Reads
`site_lat` / `site_lon` from stdin (avenc sends the model's bindings) and returns one JSON
object of observations. Every value comes from a U.S. government source; nothing is
estimated or filled in. If a source fails, the adapter reports an error instead.

  NWS api.weather.gov      latest observations of the nearest stations: temperature,
                           dewpoint, reported RH, wind, altimeter setting (reduced
                           to the station's elevation for station pressure)
  NOAA AWC (METAR)         altimeter setting when NWS reports none
  NOAA AWC (FB winds)      winds aloft (vertical shear, storm steering flow)
  NOAA nowCOAST            CONUS base reflectivity mosaic (dBZ) at the site and upstream
  USGS NWIS                precipitation gauges near the site (independent rain rate)

Station geometry (x, y in metres east/north of the site) is computed here; the kinematics
(the plane fit through the three-station triangle) are solved by Aven.
"""

import json
import math
import re
import sys
import time
import urllib.request
from concurrent.futures import ThreadPoolExecutor

UA = {
    "User-Agent": "AvenWeatherCell/1.0 (https://github.com/Thrainkrilleve/aven)",
    "Accept": "application/geo+json, application/json, text/plain",
}
EARTH_M_PER_DEG = 111_320.0
KT_TO_MS = 0.514444
LEADS_MIN = (15, 30, 60)
FRESH_MS = 75 * 60 * 1000


def get(url, timeout=20):
    with urllib.request.urlopen(urllib.request.Request(url, headers=UA), timeout=timeout) as r:
        return r.read().decode("utf-8")


def get_json(url):
    return json.loads(get(url))


def offset_m(lat0, lon0, lat, lon):
    """East/north offset (m) of (lat, lon) from (lat0, lon0), equirectangular."""
    return ((lon - lon0) * EARTH_M_PER_DEG * math.cos(math.radians(lat0)),
            (lat - lat0) * EARTH_M_PER_DEG)


def wind_uv(speed_ms, from_deg):
    """Meteorological convention: direction the wind blows FROM."""
    rad = math.radians(from_deg)
    return -speed_ms * math.sin(rad), -speed_ms * math.cos(rad)


def iso_ms(ts):
    from datetime import datetime
    return int(datetime.fromisoformat(ts.replace("Z", "+00:00")).timestamp() * 1000)


# ---------------------------------------------------------------------------
# NWS surface observations
# ---------------------------------------------------------------------------

def nws_stations(lat, lon):
    pts = get_json(f"https://api.weather.gov/points/{lat:.4f},{lon:.4f}")
    stations_url = pts["properties"].get("observationStations")
    if not stations_url:
        raise RuntimeError(f"NWS lists no observation stations for {lat:.3f},{lon:.3f} (outside its land coverage)")
    feats = get_json(stations_url)["features"]
    out = []
    for f in feats:
        slon, slat = f["geometry"]["coordinates"][:2]
        out.append({"id": f["properties"]["stationIdentifier"], "lat": slat, "lon": slon,
                    "elev_m": (f["properties"].get("elevation") or {}).get("value")})
    out.sort(key=lambda s: math.hypot(*offset_m(lat, lon, s["lat"], s["lon"])))
    return out


def latest_obs(station):
    try:
        p = get_json(f"https://api.weather.gov/stations/{station['id']}/observations/latest")["properties"]
    except Exception:  # noqa: BLE001 - a missing station is skipped, not invented
        return None
    val = lambda k: (p.get(k) or {}).get("value")
    obs = dict(station)
    obs.update({
        "timestamp_ms": iso_ms(p["timestamp"]),
        "temp_c": val("temperature"),
        "dewpoint_c": val("dewpoint"),
        "rh": val("relativeHumidity"),
        # NWS "barometricPressure" is the altimeter setting (it equals the METAR A-group to
        # 0.1 hPa), i.e. reduced to sea level, not the pressure at the station
        "altimeter_pa": val("barometricPressure"),
        "wind_kmh": val("windSpeed"),
        "wind_dir": val("windDirection"),
    })
    if obs["wind_kmh"] is not None:
        speed = obs["wind_kmh"] / 3.6
        if speed == 0:
            obs["u"], obs["v"] = 0.0, 0.0
        elif obs["wind_dir"] is not None:
            obs["u"], obs["v"] = wind_uv(speed, obs["wind_dir"])
    return obs


def triangle(site_lat, site_lon, obs):
    """Three stations with winds whose triangle is well conditioned for a plane fit, preferring
    triangles that contain the site and are small."""
    cands = [o for o in obs if "u" in o][:10]
    best = None
    for i in range(len(cands)):
        for j in range(i + 1, len(cands)):
            for k in range(j + 1, len(cands)):
                tri = [cands[i], cands[j], cands[k]]
                pts = [offset_m(site_lat, site_lon, t["lat"], t["lon"]) for t in tri]
                (x1, y1), (x2, y2), (x3, y3) = pts
                area = abs((x2 - x1) * (y3 - y1) - (x3 - x1) * (y2 - y1)) / 2
                sides = [math.dist(pts[a], pts[b]) for a, b in ((0, 1), (1, 2), (0, 2))]
                if area < 1e7 or max(sides) > 150_000:
                    continue  # degenerate or too wide for a local gradient
                quality = area / max(sides) ** 2  # 0.43 for equilateral
                # contains site?
                d = lambda p, q, r: (q[0] - p[0]) * (r[1] - p[1]) - (r[0] - p[0]) * (q[1] - p[1])
                s1, s2, s3 = d(pts[0], pts[1], (0, 0)), d(pts[1], pts[2], (0, 0)), d(pts[2], pts[0], (0, 0))
                inside = (s1 >= 0 and s2 >= 0 and s3 >= 0) or (s1 <= 0 and s2 <= 0 and s3 <= 0)
                score = quality * (2.0 if inside else 1.0) / max(sides)
                if best is None or score > best[0]:
                    best = (score, tri, pts)
    if not best:
        raise RuntimeError("no well-conditioned triangle of wind-reporting NWS stations near the site")
    return best[1], best[2]


# ---------------------------------------------------------------------------
# METAR altimeter (pressure fallback) and FB winds aloft
# ---------------------------------------------------------------------------

def metar_station_pressure(station):
    body = get(f"https://aviationweather.gov/api/data/metar?ids={station['id']}&format=json")
    data = json.loads(body) if body.strip() else None  # empty reply: not a METAR station
    if not data or data[0].get("altim") is None:
        return None
    altim = float(data[0]["altim"])  # hPa
    elev = station["elev_m"] or float(data[0].get("elev") or 0)
    # Altimeter setting to station pressure (standard atmosphere reduction)
    return altim * ((288.15 - 0.0065 * elev) / 288.15) ** 5.2561, elev


def fb_winds(site_lat, site_lon, site_elev_m):
    text = get("https://aviationweather.gov/api/data/windtemp?region=all&level=low&fcst=06")
    lines = text.splitlines()
    head = next(i for i, l in enumerate(lines) if l.startswith("FT"))
    header = lines[head]
    # Fixed-width table: each group is right-aligned to the end of its level header, so a
    # level's field is the text between the previous header's end and its own end
    levels = [(int(m.group()), m.end()) for m in re.finditer(r"\d+", header)]
    rows = {}
    for l in lines[head + 1:]:
        if not l.strip() or not re.match(r"^[A-Z0-9]{3}\b", l):
            continue
        sid = l[:3]
        groups = {}
        prev_end = 3
        for ft, end in levels:
            field = l[prev_end:end].strip()
            prev_end = end
            if len(field) >= 4 and field[:4].isdigit():
                groups[ft] = field[:4]
        rows[sid] = groups
    ids = ",".join("K" + s for s in rows)
    info = get_json(f"https://aviationweather.gov/api/data/stationinfo?ids={ids}&format=json")
    coords = {d["icaoId"][1:]: (d["lat"], d["lon"]) for d in info if d.get("icaoId")}
    near = min((s for s in rows if s in coords),
               key=lambda s: math.hypot(*offset_m(site_lat, site_lon, *coords[s])))

    def decode(g):
        dd, ss = int(g[:2]), int(g[2:4])
        if dd == 99 and ss == 0:
            return 0.0, 0.0                       # light and variable
        if dd > 36:                               # speeds of 100 kt or more
            dd, ss = dd - 50, ss + 100
        return wind_uv(ss * KT_TO_MS, dd * 10)

    winds = {ft: decode(g) for ft, g in rows[near].items()}
    above = sorted(ft for ft in winds if ft * 0.3048 > site_elev_m + 300)
    if not above:
        raise RuntimeError(f"FB station {near} has no winds above the site")
    low = above[0]
    steer = [ft for ft in winds if 9000 <= ft <= 18000] or above[:2]
    su = sum(winds[f][0] for f in steer) / len(steer)
    sv = sum(winds[f][1] for f in steer) / len(steer)
    return {"fb_station": near, "aloft_ft": low, "aloft_u": winds[low][0], "aloft_v": winds[low][1],
            "aloft_height_m": low * 0.3048 - site_elev_m, "steer_u": su, "steer_v": sv,
            "steer_levels_ft": steer}


# ---------------------------------------------------------------------------
# NOAA nowCOAST radar and USGS gauges
# ---------------------------------------------------------------------------

RADAR_WMS = "https://nowcoast.noaa.gov/geoserver/observations/weather_radar"


def radar_times():
    """Times (ms) of the radar mosaic images nowCOAST holds, oldest first."""
    xml = get(f"{RADAR_WMS}/conus_base_reflectivity_mosaic/ows?service=WMS&version=1.3.0&request=GetCapabilities")
    dim = re.search(r'<Dimension name="time"[^>]*>([^<]*)<', xml)
    times = sorted(iso_ms(t.strip()) for t in (dim.group(1) if dim else "").split(",") if t.strip())
    if not times:
        raise RuntimeError("radar capabilities list no image times")
    return times


def radar_dbz(lat, lon, image_ms=None):
    """Reflectivity (dBZ, 0 for no echo) at a point, and the time of the image read: the
    latest image, or the one taken at image_ms (a time from radar_times())."""
    if image_ms is None:
        image_ms = radar_times()[-1]
    stamp = time.strftime("%Y-%m-%dT%H:%M:%S", time.gmtime(image_ms / 1000)) + "Z"
    d = 0.005
    url = (f"{RADAR_WMS}/ows?service=WMS&version=1.3.0"
           "&request=GetFeatureInfo&layers=conus_base_reflectivity_mosaic"
           "&query_layers=conus_base_reflectivity_mosaic&crs=EPSG:4326"
           f"&bbox={lat - d},{lon - d},{lat + d},{lon + d}&width=3&height=3&i=1&j=1"
           f"&info_format=application/json&time={stamp}")
    doc = json.loads(get(url))
    vals = [v for f in doc.get("features", []) for v in f.get("properties", {}).values()
            if isinstance(v, (int, float))]
    if not vals:
        raise RuntimeError(f"no radar value at {lat:.3f},{lon:.3f}")
    # doc["timeStamp"] is when the server answered, not when the image was taken
    return max(0.0, float(vals[0])), image_ms


def usgs_rain(lat, lon):
    """Most recent precipitation increment from the nearest USGS gauge within ~25 km."""
    d = 0.25
    url = ("https://waterservices.usgs.gov/nwis/iv/?format=json&parameterCd=00045&siteStatus=active"
           f"&period=PT3H&bBox={lon - d:.3f},{lat - d:.3f},{lon + d:.3f},{lat + d:.3f}")
    try:
        series = get_json(url)["value"]["timeSeries"]
    except Exception:  # noqa: BLE001 - gauges are optional; report unavailability
        return None
    best = None
    for s in series:
        loc = s["sourceInfo"]["geoLocation"]["geogLocation"]
        dist = math.hypot(*offset_m(lat, lon, loc["latitude"], loc["longitude"]))
        vals = [v for v in s["values"][0]["value"] if v["value"] not in ("", None) and float(v["value"]) >= 0]
        if len(vals) < 2:
            continue
        a, b = vals[-2], vals[-1]
        minutes = (iso_ms(b["dateTime"]) - iso_ms(a["dateTime"])) / 60000
        if minutes <= 0:
            continue
        rate = float(b["value"]) * 25.4 * 60 / minutes  # inches per interval -> mm/h
        if best is None or dist < best[0]:
            best = (dist, rate, s["sourceInfo"]["siteCode"][0]["value"], iso_ms(b["dateTime"]))
    return best


# ---------------------------------------------------------------------------

def main():
    req = json.loads(sys.stdin.read() or "{}")
    lat, lon = float(req["site_lat"]), float(req["site_lon"])
    try:
        stations = nws_stations(lat, lon)
        with ThreadPoolExecutor(8) as pool:
            obs = [o for o in pool.map(latest_obs, stations[:16]) if o]
        # Only current reports: a station whose "latest" observation is hours old would make
        # avenc block the whole solve as stale (and rightly so)
        now_ms = time.time() * 1000
        obs = [o for o in obs if now_ms - o["timestamp_ms"] <= FRESH_MS]
        if not obs:
            raise RuntimeError("no NWS station near the site has reported in the last 75 minutes")
        complete = [o for o in obs if None not in (o["temp_c"], o["dewpoint_c"], o["rh"]) and "u" in o]
        tri, pts = triangle(lat, lon, obs)

        # Primary station: the nearest complete report with an altimeter setting, from NWS or
        # else METAR, reduced to station pressure
        primary = None
        for o in complete:
            if o["altimeter_pa"] is not None and o["elev_m"] is not None:
                elev = o["elev_m"]
                pressure_hpa = o["altimeter_pa"] / 100.0 * ((288.15 - 0.0065 * elev) / 288.15) ** 5.2561
                primary, pressure_src = o, "nws_altimeter"
                break
            res = metar_station_pressure(o)
            if res is not None:
                primary, (pressure_hpa, elev), pressure_src = o, res, "metar_altimeter"
                break
        if primary is None:
            raise RuntimeError("no recent NWS station near the site reports temperature, dewpoint, "
                               "humidity, wind and a pressure (NWS or METAR)")

        fb = fb_winds(lat, lon, elev)
        image_ms = radar_times()[-1]  # every read uses the same image
        site_dbz, radar_ms = radar_dbz(lat, lon, image_ms)
        upstream = {}
        for lead in LEADS_MIN:
            dx, dy = fb["steer_u"] * lead * 60, fb["steer_v"] * lead * 60
            ulat = lat - dy / EARTH_M_PER_DEG
            ulon = lon - dx / (EARTH_M_PER_DEG * math.cos(math.radians(lat)))
            upstream[lead] = (radar_dbz(ulat, ulon, image_ms)[0], ulat, ulon)
        gauge = usgs_rain(lat, lon)
    except Exception as e:  # noqa: BLE001 - reported to avenc as an adapter error
        print(json.dumps({"error": f"{type(e).__name__}: {e}"}))
        return

    out = {
        "temp_c": primary["temp_c"],
        "dewpoint_c": primary["dewpoint_c"],
        "rh_reported": primary["rh"],
        "pressure_hpa": pressure_hpa,
        "wind_u": primary["u"],
        "wind_v": primary["v"],
        "aloft_u": fb["aloft_u"],
        "aloft_v": fb["aloft_v"],
        "aloft_height_m": fb["aloft_height_m"],
        "steer_u": fb["steer_u"],
        "steer_v": fb["steer_v"],
        "radar_dbz": site_dbz,
        "gauge_available": 1.0 if gauge else 0.0,
        "gauge_rate_mmh": gauge[1] if gauge else 0.0,
    }
    for n, (o, (x, y)) in enumerate(zip(tri, pts), start=1):
        out.update({f"x{n}": x, f"y{n}": y, f"u{n}": o["u"], f"v{n}": o["v"]})
    for lead, (dbz, _, _) in upstream.items():
        out[f"upstream_dbz_{lead}"] = dbz
    # Freshness: the oldest observation sets the timestamp; hourly ASOS reports stay valid 75 min
    out["timestamp_ms"] = min([primary["timestamp_ms"], radar_ms] + [o["timestamp_ms"] for o in tri])
    out["ttl_ms"] = FRESH_MS
    out["backend_id"] = (f"nws_{primary['id']}_tri_{'-'.join(o['id'] for o in tri)}"
                         f"_fb_{fb['fb_station']}{fb['aloft_ft']}_{pressure_src}"
                         + (f"_usgs_{gauge[2]}" if gauge else ""))
    print(json.dumps(out))


if __name__ == "__main__":
    main()
