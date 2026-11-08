#!/usr/bin/env python3
"""Coordinator for the iSharded mesh simulator (stage 1).

Drives the token loop across the route (POST /infer to each stage in order), measures
tokens/s, gathers each worker's telemetry, and serves the iSharded-shaped admin API so
`isharded_poller.py` reads it unmodified. Dependency-free (stdlib only).

Env:
  ROUTE        comma-separated worker base URLs in order,
               e.g. "http://worker-1:8000,http://worker-2:8000"
  PORT         listen port (default 8080)
  PLANNED_TPS  the "plan" estimate reported in active_plan (default 4.9)
  USER_KEY     if set, require "Authorization: Bearer <USER_KEY>" (else open)
  POLL_EVERY   seconds between token-loop cycles (default 1.0)
"""
import json
import os
import threading
import time
import urllib.request
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

ROUTE = [u.strip() for u in os.environ.get("ROUTE", "").split(",") if u.strip()]
# Chain mode: the stages forward stage->stage themselves (each worker's NEXT_URL),
# so the coordinator only kicks stage 1 and times the whole pipeline. Star mode
# (default): the coordinator POSTs each stage in turn.
CHAIN = os.environ.get("CHAIN", "").lower() not in ("", "0", "false", "no")
PORT = int(os.environ.get("PORT", "8080"))
PLANNED_TPS = float(os.environ.get("PLANNED_TPS", "4.9"))
USER_KEY = os.environ.get("USER_KEY", "")
POLL_EVERY = float(os.environ.get("POLL_EVERY", "1.0"))

_state = {"tps_ema": None, "latency_ema": None, "workers": {}, "route": [], "up": False,
          "pace_ms": 0.0}
_lock = threading.Lock()

# Actuator bounds: a controller (e.g. AERIC) sets a per-token throttle via
# POST /admin/control; clamp it here so a bad setpoint can never wedge the loop.
PACE_MIN_MS = 0.0
PACE_MAX_MS = 10000.0


def http_json(url, method="GET", data=None, timeout=5):
    body = json.dumps(data).encode() if data is not None else None
    req = urllib.request.Request(url, method=method, data=body)
    if body is not None:
        req.add_header("Content-Type", "application/json")
    with urllib.request.urlopen(req, timeout=timeout) as r:
        return json.loads(r.read().decode())


def ema(prev, x, a=0.2):
    return x if prev is None else (1 - a) * prev + a * x


def token_loop():
    while True:
        try:
            t0 = time.time()
            if CHAIN:
                # Kick stage 1; it forwards down the chain and unwinds back.
                http_json(ROUTE[0] + "/infer", "POST", {"token": 1}, timeout=180)
            else:
                for base in ROUTE:
                    http_json(base + "/infer", "POST", {"token": 1}, timeout=2)
            compute_ms = (time.time() - t0) * 1000.0
            # Apply the control throttle: actually slow the token loop by pace_ms,
            # so the reported rate reflects the setpoint the controller published.
            with _lock:
                pace_ms = _state["pace_ms"]
            if pace_ms > 0:
                time.sleep(pace_ms / 1000.0)
            latency_ms = compute_ms + pace_ms
            tps = 1000.0 / latency_ms if latency_ms > 0 else 0.0
            workers, route = {}, []
            for base in ROUTE:
                try:
                    tel = http_json(base + "/telemetry")
                    wid = tel.get("worker", base)
                    workers[wid] = tel
                    route.append({"worker": wid, "layers": tel.get("layers", 0)})
                except Exception:
                    pass
            with _lock:
                _state["tps_ema"] = ema(_state["tps_ema"], tps)
                _state["latency_ema"] = ema(_state["latency_ema"], latency_ms)
                _state["workers"] = workers
                _state["route"] = route
                _state["up"] = True
        except Exception:
            # a stage is unreachable -> the token can't complete -> speed collapses.
            # Decay faster than the healthy EMA so a sustained outage is caught promptly.
            with _lock:
                _state["tps_ema"] = ema(_state["tps_ema"], 0.01, a=0.5)
                _state["latency_ema"] = ema(_state["latency_ema"], 100000.0, a=0.5)
        time.sleep(POLL_EVERY)


def status_doc():
    with _lock:
        tps = _state["tps_ema"]
        lat = _state["latency_ema"]
        route = list(_state["route"])
        workers = dict(_state["workers"])
        pace = _state["pace_ms"]
    return {
        "metrics": {
            "tokens_per_second_ema": round(tps, 4) if tps is not None else None,
            "token_latency_ms_ema": round(lat, 3) if lat is not None else None,
        },
        "active_plan": {"estimated_tokens_per_second": PLANNED_TPS},
        "route": route,
        "workers": [dict(id=wid, **tel) for wid, tel in workers.items()],
        # Live actuator state: the throttle currently applied per token. A
        # controller reads this back as pace_prev (back-calculation anti-windup).
        "control": {"pace_ms": round(pace, 3)},
    }


def inventory_doc():
    with _lock:
        workers = dict(_state["workers"])
    nodes = []
    for base in ROUTE:
        try:
            probe = http_json(base + "/internal/diag/probe", "POST", {"bytes": 100}, timeout=3)
            nodes.append({"worker": probe.get("worker", base), "peer_rtt_ms": probe.get("probe_ms")})
        except Exception:
            nodes.append({"worker": base, "peer_rtt_ms": None})
    return {"nodes": nodes, "worker_count": len(workers)}


class Handler(BaseHTTPRequestHandler):
    def _authed(self):
        if not USER_KEY:
            return True
        return self.headers.get("Authorization", "") == f"Bearer {USER_KEY}"

    def _send(self, obj, code=200):
        body = json.dumps(obj).encode()
        self.send_response(code)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self):
        if self.path.startswith("/admin/") and not self._authed():
            self._send({"error": "unauthorized"}, 401)
            return
        if self.path == "/admin/status":
            self._send(status_doc())
        elif self.path == "/admin/inventory":
            self._send(inventory_doc())
        elif self.path == "/health":
            self._send({"ok": _state["up"], "stages": len(ROUTE)})
        else:
            self._send({"error": "not found"}, 404)

    def do_POST(self):
        if self.path.startswith("/admin/") and not self._authed():
            self._send({"error": "unauthorized"}, 401)
            return
        if self.path == "/admin/control":
            try:
                n = int(self.headers.get("Content-Length", "0"))
                body = json.loads(self.rfile.read(n).decode()) if n else {}
                pace = float(body.get("pace_ms", 0.0))
            except (ValueError, TypeError, json.JSONDecodeError):
                self._send({"error": "bad request: expected {\"pace_ms\": <number>}"}, 400)
                return
            if pace != pace:  # NaN guard
                self._send({"error": "pace_ms must be finite"}, 400)
                return
            pace = max(PACE_MIN_MS, min(PACE_MAX_MS, pace))
            with _lock:
                _state["pace_ms"] = pace
            self._send({"pace_ms": round(pace, 3)})
        else:
            self._send({"error": "not found"}, 404)

    def log_message(self, *_):
        pass


if __name__ == "__main__":
    print(f"[coordinator] {len(ROUTE)} stages, plan={PLANNED_TPS} tok/s, auth={'on' if USER_KEY else 'off'} on :{PORT}", flush=True)
    threading.Thread(target=token_loop, daemon=True).start()
    ThreadingHTTPServer(("0.0.0.0", PORT), Handler).serve_forever()
