#!/usr/bin/env python3
"""Stub worker for the iSharded mesh simulator (stage 1).

A dependency-free stand-in for a real inference worker. It models one route stage:
each /infer call sleeps a configured compute time; it reports iSharded-shaped health
telemetry. No model is loaded — stage 2 swaps this for real inference.

Env:
  WORKER_ID   name of this stage (default "worker")
  LAYERS      layers this stage "holds" (for the route model) (default 10)
  COMPUTE_MS  modelled per-token compute (default 15)
  HOP_MS      modelled downstream wait to the next stage (default 5)
  JITTER      +/- fraction applied to timings (default 0.1)
  PORT        listen port (default 8000)
"""
import json
import os
import random
import threading
import time
import urllib.request
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

WORKER_ID = os.environ.get("WORKER_ID", "worker")
LAYERS = int(os.environ.get("LAYERS", "10"))
COMPUTE_MS = float(os.environ.get("COMPUTE_MS", "15"))
HOP_MS = float(os.environ.get("HOP_MS", "5"))
JITTER = float(os.environ.get("JITTER", "0.1"))
PORT = int(os.environ.get("PORT", "8000"))
ACT_BYTES = int(os.environ.get("ACT_BYTES", "65536"))
# Stage 2: if set, /infer runs a real llama.cpp completion and times it (real compute).
# If unset, /infer sleeps the modelled COMPUTE_MS (stage-1 stub behaviour).
LLM_URL = os.environ.get("LLM_URL", "")
PROMPT = os.environ.get("PROMPT", "In one sentence, describe a mesh network.")
N_PREDICT = int(os.environ.get("N_PREDICT", "8"))

_state = {"compute_ms_ema": COMPUTE_MS, "calls": 0}
_lock = threading.Lock()


def jittered(v):
    return max(0.0, v * (1 + random.uniform(-JITTER, JITTER)))


def real_infer():
    """Run a real llama.cpp completion and return its wall time in ms."""
    payload = json.dumps({"prompt": PROMPT, "n_predict": N_PREDICT,
                          "temperature": 0, "cache_prompt": True}).encode()
    req = urllib.request.Request(LLM_URL.rstrip("/") + "/completion", data=payload, method="POST")
    req.add_header("Content-Type", "application/json")
    t0 = time.time()
    with urllib.request.urlopen(req, timeout=120) as r:
        r.read()
    return (time.time() - t0) * 1000.0


class Handler(BaseHTTPRequestHandler):
    def _send(self, obj, code=200):
        body = json.dumps(obj).encode()
        self.send_response(code)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_POST(self):
        if self.path == "/infer":
            if LLM_URL:
                c = real_infer()
            else:
                c = jittered(COMPUTE_MS)
                time.sleep(c / 1000.0)
            with _lock:
                _state["calls"] += 1
                _state["compute_ms_ema"] = 0.8 * _state["compute_ms_ema"] + 0.2 * c
            self._send({"worker": WORKER_ID, "compute_ms": round(c, 3), "layers": LAYERS})
        elif self.path == "/internal/diag/probe":
            self._send({"worker": WORKER_ID, "probe_ms": round(jittered(2.0), 3)})
        else:
            self._send({"error": "not found"}, 404)

    def do_GET(self):
        if self.path == "/telemetry":
            with _lock:
                comp = round(_state["compute_ms_ema"], 3)
            self._send({
                "worker": WORKER_ID,
                "layers": LAYERS,
                "runtime": {"telemetry": {
                    "compute_ms_ema": comp,
                    "downstream_ms_ema": round(jittered(HOP_MS), 3),
                    "phases_ms": {"receive": 0.5, "handler": 1.0, "compute": comp,
                                  "encode": 0.5, "downstream": round(HOP_MS, 3)},
                    "activation_bytes_sent": ACT_BYTES,
                }},
            })
        elif self.path == "/health":
            self._send({"ok": True, "worker": WORKER_ID})
        else:
            self._send({"error": "not found"}, 404)

    def log_message(self, *_):  # quiet
        pass


if __name__ == "__main__":
    mode = f"llm={LLM_URL}" if LLM_URL else f"stub compute={COMPUTE_MS}ms"
    print(f"[worker] {WORKER_ID} layers={LAYERS} {mode} hop={HOP_MS}ms on :{PORT}", flush=True)
    ThreadingHTTPServer(("0.0.0.0", PORT), Handler).serve_forever()
