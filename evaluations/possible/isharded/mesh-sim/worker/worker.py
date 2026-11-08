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
# Use the chat endpoint (applies each model's chat template) by default, so
# instruct/chat models generate real output instead of emitting EOS at token 1 on
# the raw /completion path. Sampling knobs tune output quality.
CHAT_API = os.environ.get("CHAT_API", "1").lower() not in ("", "0", "false", "no")
TEMP = float(os.environ.get("SAMPLE_TEMP", "0.7"))   # not TEMP: Windows reserves it
TOP_P = float(os.environ.get("TOP_P", "0.9"))
MIN_P = float(os.environ.get("MIN_P", "0.05"))
REPEAT_PENALTY = float(os.environ.get("REPEAT_PENALTY", "1.1"))
# Optional friendly model label. Needed when several stages mount different GGUFs
# all named model.gguf (llama.cpp then reports the same path for each). Overrides
# the name auto-detected from the backend's /completion response.
MODEL_NAME = os.environ.get("MODEL_NAME", "")
# Chain mode: if set, after local compute this stage POSTs the activation to the
# NEXT stage and waits (the token flows stage->stage and unwinds back). The hop
# we report is then the real round-trip to the next stage's machine -- so a hop
# between hosts shows the data physically crossing the network. Empty = last stage.
NEXT_URL = os.environ.get("NEXT_URL", "")

_state = {"compute_ms_ema": COMPUTE_MS, "hop_ms_ema": HOP_MS, "calls": 0, "model": None}
_lock = threading.Lock()


def jittered(v):
    return max(0.0, v * (1 + random.uniform(-JITTER, JITTER)))


def rtt_prober():
    """Background: measure the real network round-trip for this stage's hop and
    EMA it. In chain mode that is the trip to the NEXT stage's machine (the
    inter-stage, machine-to-machine link); otherwise it is the trip to the model
    backend. Either way it is separate from this stage's inference compute."""
    url = (NEXT_URL or LLM_URL).rstrip("/") + "/health"
    while True:
        try:
            t0 = time.time()
            with urllib.request.urlopen(url, timeout=10) as r:
                r.read()
            rtt = (time.time() - t0) * 1000.0
            with _lock:
                _state["hop_ms_ema"] = 0.8 * _state["hop_ms_ema"] + 0.2 * rtt
        except Exception:  # noqa: BLE001
            pass
        time.sleep(3.0)


def real_infer():
    """Run a real inference and return its wall time in ms. Chat endpoint by
    default (applies the model's template, with sampling); raw /completion if
    CHAT_API is off."""
    if CHAT_API:
        url = LLM_URL.rstrip("/") + "/v1/chat/completions"
        payload = {"messages": [{"role": "user", "content": PROMPT}],
                   "max_tokens": N_PREDICT, "temperature": TEMP, "top_p": TOP_P,
                   "min_p": MIN_P, "repeat_penalty": REPEAT_PENALTY, "stream": False,
                   # Qwen3-family: answer directly, don't spend tokens on <think>.
                   "chat_template_kwargs": {"enable_thinking": False}}
    else:
        url = LLM_URL.rstrip("/") + "/completion"
        payload = {"prompt": PROMPT, "n_predict": N_PREDICT, "temperature": TEMP,
                   "top_p": TOP_P, "min_p": MIN_P, "cache_prompt": True}
    req = urllib.request.Request(url, data=json.dumps(payload).encode(), method="POST")
    req.add_header("Content-Type", "application/json")
    t0 = time.time()
    with urllib.request.urlopen(req, timeout=120) as r:
        body = r.read()
    dt = (time.time() - t0) * 1000.0
    if _state["model"] is None:
        try:
            m = json.loads(body).get("model")      # both shapes carry "model"
            if m:
                with _lock:
                    _state["model"] = os.path.basename(m)   # file name only
        except Exception:  # noqa: BLE001
            pass
    return dt


def forward_next():
    """Chain mode: pass the activation to the next stage and wait for it (the
    token flows downstream and unwinds back up). Returns the next stage's body,
    or an error marker, so the whole route's result returns to the coordinator."""
    try:
        payload = json.dumps({"token": 1}).encode()
        req = urllib.request.Request(NEXT_URL.rstrip("/") + "/infer", data=payload, method="POST")
        req.add_header("Content-Type", "application/json")
        with urllib.request.urlopen(req, timeout=120) as r:
            return json.loads(r.read().decode())
    except Exception as e:  # noqa: BLE001
        return {"error": str(e)}


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
                dt = real_infer()                 # completion wall time (incl. one RTT)
                if NEXT_URL:
                    c = dt                        # chain: hop is to the next stage, not the model
                else:
                    with _lock:
                        hop = _state["hop_ms_ema"]
                    c = max(0.0, dt - hop)        # star: inference, net of the model leg
            else:
                c = jittered(COMPUTE_MS)
                time.sleep(c / 1000.0)
            with _lock:
                _state["calls"] += 1
                _state["compute_ms_ema"] = 0.8 * _state["compute_ms_ema"] + 0.2 * c
            downstream = None
            if NEXT_URL:
                downstream = forward_next()       # real data flow to the next stage's machine
            self._send({"worker": WORKER_ID, "compute_ms": round(c, 3),
                        "layers": LAYERS, "next": downstream})
        elif self.path == "/internal/diag/probe":
            self._send({"worker": WORKER_ID, "probe_ms": round(jittered(2.0), 3)})
        else:
            self._send({"error": "not found"}, 404)

    def do_GET(self):
        if self.path == "/telemetry":
            with _lock:
                comp = round(_state["compute_ms_ema"], 3)
                model = _state["model"]
                # Measured RTT (to next stage in chain mode, else to the model);
                # modelled hop only when neither a next stage nor a backend is set.
                hop = (round(_state["hop_ms_ema"], 3) if (NEXT_URL or LLM_URL)
                       else round(jittered(HOP_MS), 3))
            self._send({
                "worker": WORKER_ID,
                "layers": LAYERS,
                # Which inference backend this stage is actually using.
                "backend": {
                    "kind": "llm" if LLM_URL else "stub",
                    "endpoint": LLM_URL or None,
                    "model": MODEL_NAME or model,
                },
                "runtime": {"telemetry": {
                    "compute_ms_ema": comp,
                    "downstream_ms_ema": hop,
                    "phases_ms": {"receive": 0.5, "handler": 1.0, "compute": comp,
                                  "encode": 0.5, "downstream": hop},
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
    chain = f" -> next={NEXT_URL}" if NEXT_URL else " (last/exit)"
    print(f"[worker] {WORKER_ID} layers={LAYERS} {mode}{chain} on :{PORT}", flush=True)
    if NEXT_URL or LLM_URL:
        threading.Thread(target=rtt_prober, daemon=True).start()   # measure real hop
    ThreadingHTTPServer(("0.0.0.0", PORT), Handler).serve_forever()
