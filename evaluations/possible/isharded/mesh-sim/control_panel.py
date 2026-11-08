#!/usr/bin/env python3
"""control_panel.py -- live dashboard + control host for the iSharded mesh sim.

A single dependency-free (stdlib only) server that:
  * serves the dashboard UI (panel.html) + its data API,
  * proxies the coordinator's authed /admin API (so the browser needs no key),
  * computes the measured-model breakdown (compute/network/unexplained) live,
  * hosts the AERIC control loop: it launches the compiled aeric_mesh_daemon and
    acts as its sensor/actuator peer (status -> sensor frame -> setpoint -> POST
    /admin/control), so you can start/stop closed-loop timing control from the UI,
  * runs the bounded "sealed run" that produces a verify-ledger-clean artifact.

Nothing here touches avenc's zero-dependency core; the control *law* still lives
in mesh_pace.aven, compiled to C and solved inside the AERIC daemon. This panel
is the peer + UI around it.

Usage:
  python control_panel.py --port 8090 \
      --mesh http://127.0.0.1:8080 --key dev-user-key \
      --aeric-dir ../../../../aeric
"""
import argparse
import json
import os
import socket
import struct
import subprocess
import sys
import threading
import time
import urllib.error
import urllib.request
from collections import deque
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

HERE = os.path.dirname(os.path.abspath(__file__))

# ---- shared state ---------------------------------------------------------
LOCK = threading.Lock()
STATE = {
    "mesh": "http://127.0.0.1:8080",
    "key": "dev-user-key",
    "aeric_dir": os.path.abspath(os.path.join(HERE, "..", "..", "..", "..", "aeric")),
    "daemon_exe": "aeric_mesh_daemon.exe",
    "daemon_port": 5556,
    # controller params (editable from the UI)
    "target": 0.40,
    "gain": 500.0,            # ms per tok/s; gentler than the initial 2000 to avoid
    "interval": 2.5,          # overshoot under the (now faster) rate EMA
    # live snapshot
    "connected": False,
    "live": {},
    "history": deque(maxlen=600),   # [{t, rate, target, pace, model_rate}]
    "ticks": deque(maxlen=40),      # recent controller tick log
    # loop control
    "mode": "idle",                 # idle | manual | auto | sealed
    "loop_state": "IDLE",           # IDLE | RUNNING | HOLD | STOPPED
    "sealed": None,                 # last sealed-run result
    # Nova alarm bridge
    "nova_health": "OK",            # OK | RATE_LOW | PACE_SAT | CEILING | DISCONNECTED
    "nova_log": deque(maxlen=30),   # recent events we (would have) sent
}
_stop = threading.Event()          # signals the active control loop to stop

# ---- Nova alarm bridge config (env; secret webhook never in the repo) -------
def _read_webhook():
    """Webhook from $NOVA_WEBHOOK, else a gitignored local file. Keeps the
    secret out of the shell history / process args and out of the repo."""
    env = os.environ.get("NOVA_WEBHOOK", "").strip()
    if env:
        return env
    path = os.environ.get("NOVA_WEBHOOK_FILE", os.path.join(HERE, "nova_webhook.txt"))
    try:
        # utf-8-sig tolerates a BOM (PowerShell's `-Encoding utf8` writes one).
        with open(path, encoding="utf-8-sig") as f:
            return f.read().strip().lstrip("﻿")
    except OSError:
        return ""


NOVA = {
    "webhook": _read_webhook(),                         # Discord webhook URL (secret)
    "mode": os.environ.get("NOVA_MODE", "dry"),         # dry (log only) | live (POST)
    "min_rate": float(os.environ.get("NOVA_MIN_RATE", "0.1")),   # rate floor alarm
    "pace_cap": 5000.0,                                 # matches the daemon envelope
    "debounce": int(os.environ.get("NOVA_DEBOUNCE", "3")),       # polls before firing
}
_nova_pending = {"cond": "OK", "count": 0}              # transition debounce state


def log(*a):
    print(*a, file=sys.stderr, flush=True)


# ---- mesh HTTP helpers ----------------------------------------------------
def mesh_get(path, timeout=5):
    url = STATE["mesh"].rstrip("/") + path
    req = urllib.request.Request(url, method="GET")
    req.add_header("Accept", "application/json")
    if STATE["key"]:
        req.add_header("Authorization", f"Bearer {STATE['key']}")
    with urllib.request.urlopen(req, timeout=timeout) as r:
        return json.loads(r.read().decode())


def mesh_post(path, obj, timeout=5):
    url = STATE["mesh"].rstrip("/") + path
    body = json.dumps(obj).encode()
    req = urllib.request.Request(url, method="POST", data=body)
    req.add_header("Content-Type", "application/json")
    if STATE["key"]:
        req.add_header("Authorization", f"Bearer {STATE['key']}")
    with urllib.request.urlopen(req, timeout=timeout) as r:
        return json.loads(r.read().decode())


def dig(obj, *paths):
    for path in paths:
        cur, ok = obj, True
        for part in path.split("."):
            if isinstance(cur, dict) and part in cur:
                cur = cur[part]
            else:
                ok = False
                break
        if ok:
            return cur
    return None


def derive(status):
    """Compute the measured-model breakdown from telemetry (same math as
    mesh_pace / measured.aven; for display)."""
    out = {
        "observed_rate": dig(status, "metrics.tokens_per_second_ema"),
        "planned_rate": dig(status, "active_plan.estimated_tokens_per_second"),
        "pace_ms": dig(status, "control.pace_ms") or 0.0,
        "stages": [],
    }
    route = dig(status, "route") or []
    wmap = {}
    bmap = {}
    backend = None
    for w in (status.get("workers") or []):
        wid = w.get("id") if isinstance(w, dict) else None
        if wid is not None:
            wmap[wid] = dig(w, "runtime.telemetry") or {}
            if isinstance(w, dict) and w.get("backend"):
                bmap[wid] = w["backend"]
        if backend is None and isinstance(w, dict) and w.get("backend"):
            backend = w["backend"]
    out["backend"] = backend
    compute = network = 0.0
    for stage in route:
        wid = stage.get("worker") if isinstance(stage, dict) else None
        tel = wmap.get(wid, {})
        c = tel.get("compute_ms_ema")
        h = tel.get("downstream_ms_ema")
        out["stages"].append({
            "worker": wid, "layers": stage.get("layers"),
            "compute_ms": c, "hop_ms": h,
            "model": (bmap.get(wid) or {}).get("model"),
        })
        if c is not None:
            compute += float(c)
        if h is not None:
            network += float(h)
    token_ms = compute + network
    out["compute_ms"] = round(compute, 2)
    out["network_ms"] = round(network, 2)
    out["token_ms"] = round(token_ms, 2)
    out["model_rate"] = round(1000.0 / token_ms, 4) if token_ms > 0 else None
    r = out["observed_rate"]
    if r:
        obs_tok_ms = 1000.0 / r
        out["observed_token_ms"] = round(obs_tok_ms, 2)
        out["unexplained_ms"] = round(obs_tok_ms - token_ms, 2)
        out["accounted_fraction"] = round(token_ms / obs_tok_ms, 4) if obs_tok_ms > 0 else None
    return out


# ---- Nova alarm bridge ----------------------------------------------------
def _nova_emoji(health):
    return {"OK": "✅", "RATE_LOW": "\U0001f534", "PACE_SAT": "\U0001f7e0",
            "CEILING": "\U0001f7e1", "DISCONNECTED": "⚫"}.get(health, "ℹ️")


def _nova_send(health, msg, data):
    """Record the event, and POST it to the Discord webhook when live."""
    entry = {"t": int(time.time() * 1000), "health": health, "msg": msg,
             "sent": False, "mode": NOVA["mode"]}
    if NOVA["mode"] == "live" and NOVA["webhook"]:
        content = f"{_nova_emoji(health)} **iSharded / Aeric** — {msg}\n```{json.dumps(data)}```"
        try:
            req = urllib.request.Request(
                NOVA["webhook"], method="POST",
                data=json.dumps({"content": content[:1900]}).encode(),
                headers={"Content-Type": "application/json",
                         # Discord/Cloudflare 403s the default urllib UA.
                         "User-Agent": "iSharded-Aeric-Monitor/1.0 (+control_panel.py)"})
            with urllib.request.urlopen(req, timeout=6):
                entry["sent"] = True
        except Exception as e:  # noqa: BLE001
            entry["error"] = str(e)
            log(f"nova: webhook POST failed: {e}")
    with LOCK:
        STATE["nova_log"].appendleft(entry)


def _nova_condition(connected, live, mode, target, model_rate):
    """Classify the current health from observable signals (no side effects)."""
    if not connected:
        return "DISCONNECTED"
    rate = live.get("observed_rate")
    pace = live.get("pace_ms") or 0.0
    if rate is not None and rate < NOVA["min_rate"]:
        return "RATE_LOW"
    if pace >= 0.98 * NOVA["pace_cap"]:
        return "PACE_SAT"           # controller out of downward authority
    if (mode == "auto" and model_rate and target > model_rate * 1.03
            and pace <= 1.0):
        return "CEILING"            # target above hardware ceiling, can't reach
    return "OK"


def nova_evaluate(connected, live, mode, target):
    """Debounced transition detector: fire once on entering/leaving a bad state."""
    model_rate = live.get("model_rate")
    cond = _nova_condition(connected, live, mode, target, model_rate)
    if cond == _nova_pending["cond"]:
        _nova_pending["count"] += 1
    else:
        _nova_pending["cond"] = cond
        _nova_pending["count"] = 1
    # only act once the candidate condition has held for `debounce` polls
    if _nova_pending["count"] != NOVA["debounce"]:
        return
    with LOCK:
        prev = STATE["nova_health"]
    if cond == prev:
        return
    with LOCK:
        STATE["nova_health"] = cond
    data = {"observed": live.get("observed_rate"), "target": target,
            "model_rate": model_rate, "pace_ms": live.get("pace_ms"), "mode": mode}
    msgs = {
        "RATE_LOW": f"throughput {live.get('observed_rate')} tok/s below floor {NOVA['min_rate']}",
        "PACE_SAT": f"throttle saturated at {NOVA['pace_cap']:.0f} ms cap — out of control authority",
        "CEILING": f"target {target} tok/s above model ceiling {model_rate} — unreachable",
        "DISCONNECTED": "coordinator unreachable",
        "OK": f"recovered — {prev} cleared, observed {live.get('observed_rate')} tok/s",
    }
    _nova_send(cond, msgs.get(cond, cond), data)


# ---- background poller ----------------------------------------------------
def poller():
    while True:
        try:
            status = mesh_get("/admin/status")
            d = derive(status)
            with LOCK:
                STATE["connected"] = True
                STATE["live"] = d
                STATE["history"].append({
                    "t": int(time.time() * 1000),
                    "rate": d.get("observed_rate"),
                    "target": STATE["target"],
                    "pace": d.get("pace_ms"),
                    "model_rate": d.get("model_rate"),
                })
                target, mode = STATE["target"], STATE["mode"]
            nova_evaluate(True, d, mode, target)
        except Exception as e:  # noqa: BLE001
            with LOCK:
                STATE["connected"] = False
                target, mode, live = STATE["target"], STATE["mode"], dict(STATE["live"])
            nova_evaluate(False, live, mode, target)
            log(f"poller: {e}")
        time.sleep(1.0)


# ---- AERIC control loop (this panel is the daemon's peer) -----------------
def _connect_daemon(port, tries=120):
    for _ in range(tries):
        try:
            return socket.create_connection(("127.0.0.1", port), timeout=2)
        except OSError:
            time.sleep(0.05)
    return None


def _recv_all(s, n):
    b = b""
    while len(b) < n:
        c = s.recv(n - len(b))
        if not c:
            raise ConnectionError("daemon closed")
        b += c
    return b


def _exchange(s, sensors):
    payload = struct.pack("<4d", *sensors)
    s.sendall(struct.pack("<I", len(payload)) + payload)
    (ln,) = struct.unpack("<I", _recv_all(s, 4))
    return struct.unpack("<1d", _recv_all(s, ln))[0]


def control_loop(mode, max_ticks, aer_name, ledger_name, interval):
    """Run the AERIC daemon + act as its peer until stopped or max_ticks.
    mode: 'auto' (runs until stopped) or 'sealed' (bounded, then verify)."""
    adir = STATE["aeric_dir"]
    daemon = os.path.join(adir, STATE["daemon_exe"])
    aer = os.path.join(adir, aer_name)
    proc = None
    s = None
    sealed_result = None
    try:
        proc = subprocess.Popen([daemon, aer], cwd=adir,
                                stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        s = _connect_daemon(STATE["daemon_port"])
        if s is None:
            with LOCK:
                STATE["loop_state"] = "STOPPED"
                STATE["ticks"].appendleft({"t": int(time.time()*1000), "err": "daemon connect failed"})
            return
        s.settimeout(6.0)
        with LOCK:
            STATE["loop_state"] = "RUNNING"
        i = 0
        while not _stop.is_set() and (max_ticks == 0 or i < max_ticks):
            i += 1
            with LOCK:
                target, gain = STATE["target"], STATE["gain"]
                live = dict(STATE["live"])
            y = live.get("observed_rate")
            pace_prev = live.get("pace_ms", 0.0) or 0.0
            if y is None:
                time.sleep(interval)
                continue
            try:
                pace_cmd = _exchange(s, [gain, float(y), float(pace_prev), target])
            except (ConnectionError, struct.error, socket.timeout) as e:
                # daemon sealed+closed (sealed run) or link lost
                log(f"loop: link ended: {e}")
                break
            try:
                applied = mesh_post("/admin/control", {"pace_ms": pace_cmd}).get("pace_ms", pace_cmd)
            except Exception as e:  # noqa: BLE001
                applied = None
                log(f"loop: actuate failed: {e}")
            err = float(y) - target
            with LOCK:
                STATE["ticks"].appendleft({
                    "t": int(time.time()*1000), "i": i,
                    "rate": round(float(y), 4), "pace_prev": round(float(pace_prev), 1),
                    "pace_cmd": round(float(pace_cmd), 1),
                    "applied": round(applied, 1) if applied is not None else None,
                    "err": round(err, 4),
                })
            time.sleep(interval)
    finally:
        if s is not None:
            try:
                s.close()
            except OSError:
                pass
        # sealed: wait for daemon to seal + exit, then verify the ledger
        if mode == "sealed" and proc is not None:
            try:
                proc.wait(timeout=10)
            except subprocess.TimeoutExpired:
                proc.terminate()
            sealed_result = _verify_ledger(ledger_name)
        elif proc is not None:
            proc.terminate()
            try:
                proc.wait(timeout=5)
            except subprocess.TimeoutExpired:
                proc.kill()
        with LOCK:
            STATE["mode"] = "idle"
            STATE["loop_state"] = "STOPPED"
            if sealed_result is not None:
                STATE["sealed"] = sealed_result


def _verify_ledger(ledger_name):
    adir = STATE["aeric_dir"]
    path = os.path.join(adir, ledger_name)
    facts = 0
    try:
        with open(path, encoding="utf-8") as f:
            facts = sum(1 for ln in f if ln.strip())
    except OSError:
        pass
    verify = "unknown"
    detail = ""
    try:
        r = subprocess.run(["avenc", "verify-ledger", path, "--key", "aven-aeric-mesh-demo-v1"],
                           cwd=adir, capture_output=True, text=True, timeout=30)
        detail = (r.stdout + r.stderr).strip().splitlines()[-1] if (r.stdout or r.stderr) else ""
        verify = "VALID" if r.returncode == 0 else "INVALID"
    except Exception as e:  # noqa: BLE001
        detail = str(e)
    return {"t": int(time.time()*1000), "ledger": ledger_name, "facts": facts,
            "verify": verify, "detail": detail, "path": path}


# ---- HTTP API -------------------------------------------------------------
class Handler(BaseHTTPRequestHandler):
    def _send(self, obj, code=200, ctype="application/json"):
        body = obj if isinstance(obj, bytes) else json.dumps(obj).encode()
        self.send_response(code)
        self.send_header("Content-Type", ctype)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def _body(self):
        n = int(self.headers.get("Content-Length", "0"))
        return json.loads(self.rfile.read(n).decode()) if n else {}

    def do_GET(self):
        if self.path in ("/", "/index.html", "/panel.html"):
            try:
                with open(os.path.join(HERE, "panel.html"), "rb") as f:
                    self._send(f.read(), ctype="text/html; charset=utf-8")
            except OSError:
                self._send(b"panel.html not found", 404, "text/plain")
            return
        if self.path == "/api/state":
            with LOCK:
                self._send({
                    "connected": STATE["connected"],
                    "mesh": STATE["mesh"],
                    "target": STATE["target"],
                    "gain": STATE["gain"],
                    "interval": STATE["interval"],
                    "mode": STATE["mode"],
                    "loop_state": STATE["loop_state"],
                    "live": STATE["live"],
                    "history": list(STATE["history"]),
                    "ticks": list(STATE["ticks"]),
                    "sealed": STATE["sealed"],
                    "nova": {
                        "health": STATE["nova_health"],
                        "mode": NOVA["mode"],
                        "configured": bool(NOVA["webhook"]),
                        "log": list(STATE["nova_log"]),
                    },
                })
            return
        if self.path == "/api/ledger":
            name = None
            with LOCK:
                if STATE["sealed"]:
                    name = STATE["sealed"]["ledger"]
            if name:
                p = os.path.join(STATE["aeric_dir"], name)
                try:
                    with open(p, "rb") as f:
                        self._send(f.read(), ctype="application/x-ndjson")
                        return
                except OSError:
                    pass
            self._send({"error": "no sealed ledger yet"}, 404)
            return
        self._send({"error": "not found"}, 404)

    def do_POST(self):
        try:
            body = self._body()
        except (ValueError, json.JSONDecodeError):
            self._send({"error": "bad json"}, 400)
            return

        if self.path == "/api/config":
            with LOCK:
                if "target" in body:
                    STATE["target"] = max(0.01, float(body["target"]))
                if "gain" in body:
                    STATE["gain"] = max(0.0, float(body["gain"]))
                if "interval" in body:
                    STATE["interval"] = max(0.5, float(body["interval"]))
                self._send({"target": STATE["target"], "gain": STATE["gain"],
                            "interval": STATE["interval"]})
            return

        if self.path == "/api/pace":
            with LOCK:
                busy = STATE["mode"] in ("auto", "sealed")
            if busy:
                self._send({"error": "control loop running; stop it for manual pace"}, 409)
                return
            try:
                applied = mesh_post("/admin/control", {"pace_ms": float(body.get("pace_ms", 0))})
                with LOCK:
                    STATE["mode"] = "manual" if float(body.get("pace_ms", 0)) > 0 else "idle"
                self._send(applied)
            except Exception as e:  # noqa: BLE001
                self._send({"error": str(e)}, 502)
            return

        if self.path == "/api/loop":
            action = body.get("action")
            with LOCK:
                mode = STATE["mode"]
            if action == "start":
                if mode in ("auto", "sealed"):
                    self._send({"error": "already running"}, 409)
                    return
                _stop.clear()
                with LOCK:
                    STATE["mode"] = "auto"
                    STATE["loop_state"] = "STARTING"
                t = threading.Thread(target=control_loop,
                                     args=("auto", 0, "mesh_pace.aer", "mesh_pace_ledger.jsonl",
                                           STATE["interval"]), daemon=True)
                t.start()
                self._send({"mode": "auto"})
            elif action == "stop":
                _stop.set()
                self._send({"stopping": True})
            else:
                self._send({"error": "action must be start|stop"}, 400)
            return

        if self.path == "/api/nova/test":
            _nova_send("OK", "test ping from the control panel", {"test": True})
            self._send({"mode": NOVA["mode"], "configured": bool(NOVA["webhook"])})
            return

        if self.path == "/api/sealed":
            with LOCK:
                busy = STATE["mode"] in ("auto", "sealed")
            if busy:
                self._send({"error": "stop the running loop first"}, 409)
                return
            ticks = int(body.get("ticks", 30))
            _write_sealed_aer(ticks)
            _stop.clear()
            with LOCK:
                STATE["mode"] = "sealed"
                STATE["loop_state"] = "STARTING"
                STATE["sealed"] = None
            t = threading.Thread(target=control_loop,
                                 args=("sealed", ticks, "mesh_pace_sealed.aer",
                                       "mesh_pace_sealed_ledger.jsonl", 2.0), daemon=True)
            t.start()
            self._send({"mode": "sealed", "ticks": ticks})
            return

        self._send({"error": "not found"}, 404)

    def log_message(self, *_):
        pass


def _write_sealed_aer(ticks):
    """Write a bounded daemon config that seals its ledger after `ticks`."""
    src = os.path.join(STATE["aeric_dir"], "mesh_pace.aer")
    dst = os.path.join(STATE["aeric_dir"], "mesh_pace_sealed.aer")
    with open(src, encoding="utf-8") as f:
        text = f.read()
    out = []
    for line in text.splitlines():
        k = line.split("=")[0].strip() if "=" in line else ""
        if k == "max_ticks":
            out.append(f"max_ticks     = {ticks}")
        elif k == "ledger":
            out.append("ledger        = mesh_pace_sealed_ledger.jsonl")
        elif k == "deadline_ms":
            out.append("deadline_ms   = 8000")   # generous: bounded run, slow plant
        else:
            out.append(line)
    with open(dst, "w", encoding="utf-8") as f:
        f.write("\n".join(out) + "\n")


def main():
    p = argparse.ArgumentParser(description="iSharded mesh control panel.")
    p.add_argument("--port", type=int, default=8090)
    p.add_argument("--mesh", default=os.environ.get("ISHARDED_URL", "http://127.0.0.1:8080"))
    p.add_argument("--key", default=os.environ.get("ISHARDED_KEY", "dev-user-key"))
    p.add_argument("--aeric-dir", default=STATE["aeric_dir"])
    p.add_argument("--daemon-port", type=int, default=5556)
    a = p.parse_args()
    STATE["mesh"] = a.mesh
    STATE["key"] = a.key
    STATE["aeric_dir"] = os.path.abspath(a.aeric_dir)
    STATE["daemon_port"] = a.daemon_port

    threading.Thread(target=poller, daemon=True).start()
    log(f"control panel on http://127.0.0.1:{a.port}  (mesh {a.mesh}, aeric {STATE['aeric_dir']})")
    ThreadingHTTPServer(("127.0.0.1", a.port), Handler).serve_forever()


if __name__ == "__main__":
    main()
