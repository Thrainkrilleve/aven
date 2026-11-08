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
# Aggregate-throughput knobs (like iSharded's --parallel/slots + speculative draft):
#   STREAMS         how many replies run through the mesh at once (concurrency)
#   TOKENS_PER_PASS real tokens each pipeline pass yields (speculative decoding;
#                   iSharded measures ~2.9-3.8 accepted per pass)
# Single-stream rate is latency-bound; aggregate = passes/s * tokens_per_pass and
# climbs with concurrency until the hardware saturates.
MAX_STREAMS = int(os.environ.get("MAX_STREAMS", "32"))
STREAMS = max(1, min(MAX_STREAMS, int(os.environ.get("STREAMS", "1"))))
TOKENS_PER_PASS = float(os.environ.get("TOKENS_PER_PASS", "3.0"))

_state = {"tps_ema": None, "latency_ema": None, "agg_tps_ema": None,
          "passes_total": 0, "workers": {}, "route": [], "up": False,
          "pace_ms": 0.0, "streams": STREAMS, "tokens_per_pass": TOKENS_PER_PASS}
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


def ema(prev, x, a=0.4):
    # a=0.4 (was 0.2): track pace changes ~2x faster so the measured rate keeps
    # up with the controller's command and the loop winds up less under lag.
    return x if prev is None else (1 - a) * prev + a * x


def one_pass():
    """Run one token through the whole route and return its wall time in ms."""
    t0 = time.time()
    if CHAIN:
        # Kick stage 1; it forwards down the chain and unwinds back.
        http_json(ROUTE[0] + "/infer", "POST", {"token": 1}, timeout=180)
    else:
        for base in ROUTE:
            http_json(base + "/infer", "POST", {"token": 1}, timeout=10)
    return (time.time() - t0) * 1000.0


def stream_worker(idx):
    """One concurrent reply stream. Runs passes back-to-back while it is within
    the current concurrency; idles when concurrency is dialled below its index.
    Streams overlap on the mesh, so aggregate throughput rises with concurrency."""
    while True:
        with _lock:
            active = _state["streams"]
            pace_ms = _state["pace_ms"]
        if idx >= active:
            time.sleep(0.2)
            continue
        try:
            compute_ms = one_pass()
            if pace_ms > 0:                     # per-stream control throttle
                time.sleep(pace_ms / 1000.0)
            latency_ms = compute_ms + pace_ms
            with _lock:
                _state["passes_total"] += 1
                _state["latency_ema"] = ema(_state["latency_ema"], latency_ms)
        except Exception:
            time.sleep(0.5)                     # stage unreachable; back off, retry


def metrics_loop():
    """Sample aggregate throughput once a second and refresh telemetry.
    aggregate tok/s = passes-per-second (across all streams) * tokens_per_pass."""
    last_n, last_t = 0, time.time()
    while True:
        time.sleep(1.0)
        now = time.time()
        with _lock:
            n = _state["passes_total"]
            lat = _state["latency_ema"]
            tpp = _state["tokens_per_pass"]
        passes_per_sec = (n - last_n) / max(1e-6, now - last_t)
        last_n, last_t = n, now
        agg = passes_per_sec * tpp                       # aggregate real tokens/s
        single = (1000.0 / lat) if lat else 0.0          # one stream's pass rate
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
            _state["tps_ema"] = ema(_state["tps_ema"], single)
            _state["agg_tps_ema"] = ema(_state["agg_tps_ema"], agg)
            if workers:
                _state["workers"] = workers
                _state["route"] = route
            _state["up"] = bool(workers)


def status_doc():
    with _lock:
        tps = _state["tps_ema"]
        agg = _state["agg_tps_ema"]
        lat = _state["latency_ema"]
        route = list(_state["route"])
        workers = dict(_state["workers"])
        pace = _state["pace_ms"]
        streams = _state["streams"]
        tpp = _state["tokens_per_pass"]
    return {
        "metrics": {
            # Single-stream decode rate (one reply) -- what the controller targets.
            "tokens_per_second_ema": round(tps, 4) if tps is not None else None,
            # Aggregate across all concurrent streams -- the real serving throughput.
            "aggregate_tokens_per_second_ema": round(agg, 4) if agg is not None else None,
            "token_latency_ms_ema": round(lat, 3) if lat is not None else None,
        },
        "active_plan": {"estimated_tokens_per_second": PLANNED_TPS},
        "route": route,
        "workers": [dict(id=wid, **tel) for wid, tel in workers.items()],
        # Live actuator + concurrency state. pace: the per-token throttle (a
        # controller reads it back as pace_prev). streams: concurrency.
        # tokens_per_pass: real tokens each pass yields (speculative decoding).
        "control": {"pace_ms": round(pace, 3), "streams": streams,
                    "tokens_per_pass": tpp, "max_streams": MAX_STREAMS},
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
            except (ValueError, TypeError, json.JSONDecodeError):
                self._send({"error": "bad request: expected JSON object"}, 400)
                return
            with _lock:
                if "pace_ms" in body:
                    try:
                        p = float(body["pace_ms"])
                        if p == p:  # not NaN
                            _state["pace_ms"] = max(PACE_MIN_MS, min(PACE_MAX_MS, p))
                    except (ValueError, TypeError):
                        pass
                if "streams" in body:
                    try:
                        _state["streams"] = max(1, min(MAX_STREAMS, int(body["streams"])))
                    except (ValueError, TypeError):
                        pass
                if "tokens_per_pass" in body:
                    try:
                        _state["tokens_per_pass"] = max(1.0, min(16.0, float(body["tokens_per_pass"])))
                    except (ValueError, TypeError):
                        pass
                resp = {"pace_ms": round(_state["pace_ms"], 3), "streams": _state["streams"],
                        "tokens_per_pass": _state["tokens_per_pass"]}
            self._send(resp)
        else:
            self._send({"error": "not found"}, 404)

    def log_message(self, *_):
        pass


if __name__ == "__main__":
    print(f"[coordinator] {len(ROUTE)} stages, streams={STREAMS}/{MAX_STREAMS}, "
          f"tokens_per_pass={TOKENS_PER_PASS}, plan={PLANNED_TPS} tok/s, "
          f"auth={'on' if USER_KEY else 'off'} on :{PORT}", flush=True)
    # A fixed pool of stream threads; each idles unless within the live concurrency.
    for i in range(MAX_STREAMS):
        threading.Thread(target=stream_worker, args=(i,), daemon=True).start()
    threading.Thread(target=metrics_loop, daemon=True).start()
    ThreadingHTTPServer(("0.0.0.0", PORT), Handler).serve_forever()
