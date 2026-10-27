#!/usr/bin/env python3
"""evaluations/possible/dc_bridge.py — Live bridge between the DC digital twin page and Aven.

A browser cannot start `avenc` or open a raw TCP socket, so this standard-library HTTP
server serves dc.html and answers its requests by running the real toolchain:

  GET /api/tick?crac=1   one simulation tick. Writes a 2,500-node data-center model and runs
                         `avenc run --json --ledger`. The model's Weather relation is
                         `using ipc("127.0.0.1:9090")`, so avenc itself queries the live
                         telemetry daemon (its METAR surface temperature becomes the outside
                         air temperature, and its TTL is enforced by avenc). Cluster load
                         replays real hourly English-Wikipedia pageviews, one hour per tick.
                         The ledger is checked with `avenc verify-ledger`. A direct query to
                         the daemon supplies the raw RX payload and round-trip time.
  GET /api/sandbox?mode=cooling&load=50&temp=30
  GET /api/sandbox?mode=temp&load=50&cooling=20
                         solves the sandbox relation forwards or backwards with avenc.
  GET /api/pareto?crac=1&outside=15
                         `avenc pareto` on dc_fleet.aven with the current live inputs.

Nothing is simulated in the browser. If a source fails (daemon down, API unreachable,
stale data, constraint violation) the response says so and the page shows it.

Usage (from the repository root, with the daemon running):
  python evaluations/flight_mesh/flight_mesh_daemon.py --port 9090
  python evaluations/possible/dc_bridge.py --port 8765
  then open http://127.0.0.1:8765/
"""

import argparse
import datetime
import json
import math
import os
import re
import socket
import subprocess
import tempfile
import threading
import time
import urllib.parse
import urllib.request
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent.parent
AVENC = str(ROOT / "target" / "release" / ("avenc.exe" if os.name == "nt" else "avenc"))
DAEMON = ("127.0.0.1", 9090)
WORK = Path(tempfile.mkdtemp(prefix="aven_dc_"))

GRID = 50
NODES = GRID * GRID

# Physical constants and plant parameters (stated assumptions, shared with dc_fleet.aven)
PLANT = {
    "idle_w": 110,             # server idle draw (W)
    "dynamic_w": 240,          # additional draw at 100% utilization (W)
    "setpoint_c": 18,          # CRAC supply-air setpoint (°C)
    "economizer_rise_c": 4,    # supply air above outside air with the CRAC failed (°C)
    "recirc": 0.3,             # fraction of the row's heat rise recirculated to inlets
    "air_rho": 1.2,            # air density (kg/m^3)
    "air_cp": 1005,            # air specific heat (J/kg/K)
    "airflow_m3s": 0.05,       # airflow per server (~106 CFM)
    "fan_fraction": 0.06,      # CRAC/fan power as a fraction of IT power
    "chiller_capacity_kw": 900,
    "ups_capacity_kw": 800,    # facility constraint enforced by avenc
    "page_kb": 90,             # average bytes per pageview served (KB)
}


def placement_weight(r, c):
    """Fixed, documented share of the cluster load each node carries (0.55–1.05): a smooth
    rack-placement pattern with a hotter middle aisle. Deterministic — never random."""
    aisle = math.exp(-((c - GRID / 2) / (GRID / 4)) ** 2)
    wave = 0.5 + 0.5 * math.sin(r * 0.31) * math.cos(c * 0.17)
    return round(0.55 + 0.25 * aisle + 0.25 * wave, 4)


WEIGHTS = [placement_weight(i // GRID, i % GRID) for i in range(NODES)]


# ---------------------------------------------------------------------------
# Real workload: hourly English-Wikipedia pageviews (Wikimedia REST API)
# ---------------------------------------------------------------------------

class Pageviews:
    URL = ("https://wikimedia.org/api/rest_v1/metrics/pageviews/aggregate/"
           "en.wikipedia/all-access/user/hourly/{start}/{end}")

    def __init__(self):
        self.items = []
        self.fetched_at = 0.0
        self.error = None
        self.step = 0
        self.lock = threading.Lock()

    def refresh(self):
        end = datetime.datetime.now(datetime.timezone.utc).replace(minute=0, second=0, microsecond=0)
        start = end - datetime.timedelta(hours=72)
        url = self.URL.format(start=f"{start:%Y%m%d%H}", end=f"{end:%Y%m%d%H}")
        req = urllib.request.Request(url, headers={
            "User-Agent": "AvenDCDigitalTwin/1.0 (https://github.com/Thrainkrilleve/aven)"})
        try:
            with urllib.request.urlopen(req, timeout=20) as r:
                items = json.load(r)["items"]
            self.items = [(i["timestamp"], i["views"]) for i in items]
            self.fetched_at = time.time()
            self.error = None
        except Exception as e:  # noqa: BLE001 - surfaced to the page
            self.error = f"Wikimedia pageviews unavailable: {e}"

    def next_hour(self):
        """Returns (hour stamp, views in that hour, peak views in the window) and advances."""
        with self.lock:
            if not self.items or time.time() - self.fetched_at > 1800:
                self.refresh()
            if not self.items:
                return None
            stamp, views = self.items[self.step % len(self.items)]
            self.step += 1
            return stamp, views, max(v for _, v in self.items)


PAGEVIEWS = Pageviews()


# ---------------------------------------------------------------------------
# Running avenc
# ---------------------------------------------------------------------------

def run_avenc(args, timeout=60):
    t0 = time.perf_counter()
    p = subprocess.run([AVENC, *args], capture_output=True, text=True, timeout=timeout)
    return p, (time.perf_counter() - t0) * 1000.0


def parse_json_line(stdout):
    lines = [l for l in stdout.strip().splitlines() if l.startswith("{")]
    return json.loads(lines[-1]) if lines else None


def build_dc_model(crac_ok, cluster_util, views_per_s):
    L = [
        "// Generated by dc_bridge.py each tick — 2,500-node data center",
        "relation Weather using ipc(\"127.0.0.1:9090\") {",
        "    // live METAR surface temperature reported by the telemetry daemon",
        "    outside_c = surface_temp_c",
        "}",
        "relation DataCenter {",
    ]
    heat = "(air_rho * air_cp * airflow_m3s)"
    for r in range(GRID):
        for c in range(GRID):
            i = r * GRID + c
            L.append(f"    u{i} = cluster_util * w{i}")
            L.append(f"    p{i} = idle_w + u{i} * dynamic_w")
            L.append(f"    t{i} = supply_c + recirc * row{r}_rise + p{i} / {heat}")
            L.append(f"    g{i} = total_gbps * u{i} / util_sum")
        cells = range(r * GRID, (r + 1) * GRID)
        L.append(f"    row{r}_w = " + " + ".join(f"p{i}" for i in cells))
        L.append(f"    row{r}_u = " + " + ".join(f"u{i}" for i in cells))
        L.append(f"    row{r}_t = " + " + ".join(f"t{i}" for i in cells))
        L.append(f"    row{r}_rise = row{r}_w / ({heat} * {GRID})")
    rows = range(GRID)
    L += [
        "    it_w = " + " + ".join(f"row{r}_w" for r in rows),
        "    util_sum = " + " + ".join(f"row{r}_u" for r in rows),
        "    temp_sum = " + " + ".join(f"row{r}_t" for r in rows),
        f"    avg_temp_c = temp_sum / {NODES}",
        "    it_kw = it_w / 1000.0",
        "    supply_c = crac_ok * setpoint_c + (1.0 - crac_ok) * (outside_c + economizer_rise_c)",
        "    cop = 7.5 - 0.12 * outside_c",
        "    cooling_kw = crac_ok * it_kw / cop + fan_fraction * it_kw",
        "    facility_kw = it_kw + cooling_kw",
        "    pue = facility_kw / it_kw",
        "    chiller_duty = crac_ok * it_kw / chiller_capacity_kw",
        "    total_gbps = views_per_s * page_kb * 8.0 / 1000000.0",
        "}",
        "solve DataCenter, Weather with {",
        "    flight_index = 0",
        f"    crac_ok = {crac_ok}",
        f"    cluster_util = {cluster_util!r}",
        f"    views_per_s = {views_per_s!r}",
    ]
    L += [f"    {k} = {v}" for k, v in PLANT.items() if k != "ups_capacity_kw"]
    L += [f"    w{i} = {w}" for i, w in enumerate(WEIGHTS)]
    L += [f"    it_kw <= {PLANT['ups_capacity_kw']}", "}"]
    return "\n".join(L) + "\n"


NODE_VAR = re.compile(r"^([uptgw]\d+|row\d+_\w+)$")


def daemon_probe():
    """Direct query to the daemon so the page can show the raw RX payload and RTT."""
    t0 = time.perf_counter()
    try:
        with socket.create_connection(DAEMON, timeout=5) as s:
            s.sendall((json.dumps({"flight_index": 0}) + "\n").encode())
            buf = b""
            while not buf.endswith(b"\n"):
                chunk = s.recv(65536)
                if not chunk:
                    break
                buf += chunk
        return {"rtt_ms": (time.perf_counter() - t0) * 1000.0,
                "tx": {"flight_index": 0}, "rx": json.loads(buf.decode())}
    except Exception as e:  # noqa: BLE001 - surfaced to the page
        return {"error": f"telemetry daemon unreachable at {DAEMON[0]}:{DAEMON[1]}: {e}"}


TICK_LOCK = threading.Lock()


def tick(crac_ok):
    hour = PAGEVIEWS.next_hour()
    if hour is None:
        return {"error": PAGEVIEWS.error}
    stamp, views, peak = hour
    views_per_s = views / 3600.0
    cluster_util = round(0.9 * views / peak, 6)

    with TICK_LOCK:
        model = WORK / "dc_tick.aven"
        ledger = WORK / "dc_tick.jsonl"
        model.write_text(build_dc_model(crac_ok, cluster_util, views_per_s), encoding="utf-8")
        if ledger.exists():
            ledger.unlink()
        p, solve_ms = run_avenc(["run", str(model), "--json", "--ledger", str(ledger)])
        verdict = {0: "settled", 1: "error", 2: "constraint violation", 3: "decision blocked"}.get(
            p.returncode, f"exit {p.returncode}")
        doc = parse_json_line(p.stdout) if p.returncode == 0 else None

        facts, chain = [], None
        if ledger.exists():
            v, _ = run_avenc(["verify-ledger", str(ledger)])
            chain = {"ok": v.returncode == 0, "message": (v.stdout or v.stderr).strip()}
            for line in ledger.read_text(encoding="utf-8").splitlines():
                f = json.loads(line)
                if f.get("seal"):
                    chain["facts"] = f["facts"]
                    chain["head"] = f["head"]
                elif not NODE_VAR.match(f["name"]):
                    facts.append({k: f[k] for k in ("seq", "kind", "name", "value", "origin", "metadata", "prev", "hash")})

    out = {
        "source": {
            "pageviews_hour_utc": stamp,
            "views": views,
            "views_per_s": views_per_s,
            "cluster_util": cluster_util,
            "window_peak_views": peak,
        },
        "avenc": {"exit_code": p.returncode, "verdict": verdict, "wall_ms": solve_ms,
                  "stderr": p.stderr.strip()[-600:]},
        "ledger": {"chain": chain, "facts": facts},
        "daemon": daemon_probe(),
    }
    if doc:
        env = doc["env"]
        out["nodes"] = {k: [round(env[f"{k}{i}"], 4) for i in range(NODES)] for k in "uptg"}
        out["facility"] = {k: env[k] for k in (
            "it_kw", "cooling_kw", "facility_kw", "pue", "avg_temp_c", "supply_c", "outside_c",
            "cop", "chiller_duty", "total_gbps", "crac_ok")}
    return out


def sandbox(q):
    mode = q.get("mode", "cooling")
    load = float(q.get("load", 50))
    lines = ["relation Thermal {",
             "    temp = base + load * heat_factor - cooling * cooling_factor",
             "}", "solve Thermal with {",
             "    base = 20", "    heat_factor = 0.5", "    cooling_factor = 0.8",
             f"    load = {load!r}"]
    if mode == "cooling":
        lines.append(f"    temp = {float(q.get('temp', 30))!r}")
        unknown = "cooling"
    else:
        lines.append(f"    cooling = {float(q.get('cooling', 20))!r}")
        unknown = "temp"
    lines += ["    cooling >= 0", "}"]
    src = "\n".join(lines) + "\n"
    model = WORK / f"sandbox_{threading.get_ident()}.aven"
    model.write_text(src, encoding="utf-8")
    p, ms = run_avenc(["run", str(model), "--json"])
    trace = run_avenc(["run", str(model)])[0].stdout
    steps = [l.strip() for l in trace.splitlines() if "<-" in l]
    doc = parse_json_line(p.stdout) if p.returncode == 0 else None
    return {"unknown": unknown, "model": src, "wall_ms": ms, "exit_code": p.returncode,
            "env": doc["env"] if doc else None, "trace": steps, "stderr": p.stderr.strip()[-400:]}


def pareto(q):
    crac = 1 if q.get("crac", "1") == "1" else 0
    outside = float(q.get("outside", 15))
    peak_views = float(q.get("peak_views", 0)) or None
    src = (HERE / "dc_fleet.aven").read_text(encoding="utf-8")
    src = re.sub(r"crac_ok = \S+", f"crac_ok = {crac}", src)
    src = re.sub(r"outside_c = \S+", f"outside_c = {outside!r}", src)
    src = re.sub(r"w_mean = \S+", f"w_mean = {sum(WEIGHTS) / NODES!r}", src)
    if peak_views:
        # capacity such that 90% utilization serves the window's peak pageview rate
        rps = peak_views / 3600.0 / 0.9 / NODES
        src = re.sub(r"rps_per_server = \S+", f"rps_per_server = {rps!r}", src)
    model = WORK / "dc_fleet_live.aven"
    model.write_text(src, encoding="utf-8")
    p, ms = run_avenc(["pareto", str(model), "cluster_util", "0.05", "0.95", "--steps", "18"])
    if p.returncode != 0:
        return {"error": p.stderr.strip()[-400:], "exit_code": p.returncode}
    lines = p.stdout.splitlines()
    head = next(i for i, l in enumerate(lines) if l.startswith("Step "))
    cols = lines[head].split()
    points = []
    for l in lines[head + 2:]:
        parts = l.split()
        if not parts or not parts[0].isdigit():
            break
        points.append(dict(zip(cols, (float(x) for x in parts))))
    knee = re.search(r"Knee Point Detected: Step (\d+)", p.stdout)
    return {"points": points, "knee_step": int(knee.group(1)) if knee else None,
            "wall_ms": ms, "command": f"avenc pareto dc_fleet.aven cluster_util 0.05 0.95 --steps 18"}


class Handler(BaseHTTPRequestHandler):
    def _send(self, code, body, ctype="application/json"):
        data = body if isinstance(body, bytes) else json.dumps(body).encode()
        self.send_response(code)
        self.send_header("Content-Type", ctype)
        self.send_header("Cache-Control", "no-store")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def do_GET(self):  # noqa: N802 - http.server API
        url = urllib.parse.urlparse(self.path)
        q = dict(urllib.parse.parse_qsl(url.query))
        try:
            if url.path in ("/", "/dc.html"):
                self._send(200, (HERE / "dc.html").read_bytes(), "text/html; charset=utf-8")
            elif url.path == "/api/tick":
                self._send(200, tick(1 if q.get("crac", "1") == "1" else 0))
            elif url.path == "/api/sandbox":
                self._send(200, sandbox(q))
            elif url.path == "/api/pareto":
                self._send(200, pareto(q))
            else:
                self._send(404, {"error": "not found"})
        except Exception as e:  # noqa: BLE001 - report to the page instead of dropping
            self._send(500, {"error": f"{type(e).__name__}: {e}"})

    def log_message(self, fmt, *args):
        pass


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--port", type=int, default=8765)
    args = ap.parse_args()
    if not Path(AVENC).exists():
        raise SystemExit(f"avenc not found at {AVENC}; run `cargo build --release` first")
    PAGEVIEWS.refresh()
    print(f"Aven DC bridge on http://127.0.0.1:{args.port}/  (avenc: {AVENC})")
    if PAGEVIEWS.error:
        print(PAGEVIEWS.error)
    ThreadingHTTPServer(("127.0.0.1", args.port), Handler).serve_forever()


if __name__ == "__main__":
    main()
