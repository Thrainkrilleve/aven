#!/usr/bin/env python3
"""
k8s/controller/aven_controller.py — Autonomous Aven Kubernetes Capacity Controller
---------------------------------------------------------------------------------
Translates declarative Aven relational capacity models (Little's Law, queueing
headroom, discrete ceiling) into live Kubernetes replica targets and resource boundaries.

Key Production Safeguards:
1. Native Aven Core: Drives solving via compiled avenc against hpa_scaling.aven.
2. Freshness Hard Enforcement: Blocks scaling if external metrics are stale (TTL=0).
3. Operator Safety / Freeze Switch: Respects 'aven.sh/freeze: "true"' annotation.
4. Complete Failure Isolation: Preserves current replica count on blocked decisions.
5. Observability & Custom Metrics API: Serves Prometheus and K8s Custom Metrics endpoints.
"""

from http.server import HTTPServer, BaseHTTPRequestHandler
import json
import os
import re
import subprocess
import sys
import threading
import time
from datetime import datetime, timezone

NAMESPACE = os.environ.get("AVEN_NAMESPACE", "aven-system")
DEPLOYMENT_NAME = os.environ.get("AVEN_DEPLOYMENT", "aven-workload")
_EXE = "avenc.exe" if os.name == "nt" else "avenc"
AVENC_BIN = os.environ.get("AVENC_BIN") or os.path.abspath(os.path.join(os.path.dirname(__file__), "../../target/release", _EXE))
if not os.path.exists(AVENC_BIN):
    AVENC_BIN = os.path.abspath(os.path.join(os.path.dirname(__file__), "../../target/debug", _EXE))

MODEL_PATH = os.path.abspath(os.path.join(os.path.dirname(__file__), "hpa_scaling.aven"))

# State cache for HTTP metrics server
STATE = {
    "target_pods": 3,
    "current_pods": 3,
    "allocated_workers": 286,
    "decision_blocked": False,
    "stale_facts": [],
    "solve_time_us": 0.0,
    "last_reconcile": datetime.now(timezone.utc).isoformat(),
    "spec_hash": "init",
}


def get_deployment_metadata(namespace: str, name: str) -> dict:
    """Reads deployment annotations and current replica count via kubectl."""
    cmd = ["kubectl", "get", "deployment", name, "-n", namespace, "-o", "json"]
    try:
        res = subprocess.run(cmd, capture_output=True, text=True, check=True)
        data = json.loads(res.stdout)
        annotations = data.get("metadata", {}).get("annotations", {})
        replicas = data.get("spec", {}).get("replicas", 1)
        return {
            "replicas": replicas,
            "annotations": annotations,
            "freeze": annotations.get("aven.sh/freeze", "false").lower() == "true",
        }
    except Exception:
        # Fallback if cluster is in offline simulation mode
        return {
            "replicas": STATE["current_pods"],
            "annotations": {"aven.sh/freeze": "false"},
            "freeze": False,
        }


def run_avenc_solver(reqs_per_second: float = 10000.0, avg_latency_ms: float = 20.0, inject_ttl_zero: bool = False) -> dict:
    """
    Executes compiled avenc against hpa_scaling.aven.
    Enforces freshness and schema validation at runtime.
    """
    env = os.environ.copy()
    env["REQS_PER_SECOND"] = str(reqs_per_second)
    env["AVG_LATENCY_MS"] = str(avg_latency_ms)
    if inject_ttl_zero:
        env["AVEN_INJECT_TTL_ZERO"] = "1"
    else:
        env.pop("AVEN_INJECT_TTL_ZERO", None)

    cmd = [AVENC_BIN, "run", MODEL_PATH]
    t0 = time.perf_counter()
    res = subprocess.run(cmd, env=env, capture_output=True, text=True, encoding="utf-8", errors="replace")
    elapsed_us = (time.perf_counter() - t0) * 1_000_000.0

    output = res.stdout + "\n" + res.stderr

    # Check for Decision Blocked / Stale facts
    if res.returncode != 0 or "DECISION BLOCKED" in output:
        stale_matches = re.findall(r"•\s+([a-zA-Z0-9_]+)", output)
        return {
            "is_actionable": False,
            "decision_blocked": True,
            "stale_facts": stale_matches or ["expired-ttl"],
            "target_pods": None,
            "solve_time_us": elapsed_us,
            "raw_output": output,
        }

    # Extract settled values
    settled = {}
    for line in res.stdout.splitlines():
        if "=" in line:
            parts = line.split("=")
            k = parts[0].strip()
            v_str = parts[1].strip()
            try:
                settled[k] = float(v_str)
            except ValueError:
                pass

    target_pods = int(settled.get("target_pods", 1))
    allocated_workers = int(settled.get("allocated_workers", target_pods * 100))

    return {
        "is_actionable": True,
        "decision_blocked": False,
        "stale_facts": [],
        "target_pods": target_pods,
        "allocated_workers": allocated_workers,
        "solve_time_us": elapsed_us,
        "settled": settled,
    }


def reconcile(reqs_per_second: float = 10000.0, avg_latency_ms: float = 20.0, inject_ttl_zero: bool = False, dry_run: bool = False) -> dict:
    """Executes an autonomous reconciliation loop against the Kubernetes deployment."""
    meta = get_deployment_metadata(NAMESPACE, DEPLOYMENT_NAME)
    current_replicas = meta["replicas"]
    is_frozen = meta["freeze"]

    now_iso = datetime.now(timezone.utc).isoformat()

    # Operator Freeze Switch Guard
    if is_frozen:
        event = {
            "timestamp": now_iso,
            "event": "RECONCILE_SKIPPED",
            "reason": "OPERATOR_FREEZE_ENABLED",
            "current_replicas": current_replicas,
        }
        print(json.dumps(event))
        return event

    # Execute native Aven solver
    sol = run_avenc_solver(reqs_per_second, avg_latency_ms, inject_ttl_zero)

    if not sol["is_actionable"]:
        # Safety Failure Isolation: Hold existing replicas, refuse scaling on stale metrics
        STATE["decision_blocked"] = True
        STATE["stale_facts"] = sol["stale_facts"]
        event = {
            "timestamp": now_iso,
            "event": "SCALE_BLOCKED_STALE_DATA",
            "reason": "FRESHNESS_ENFORCEMENT_FAILURE",
            "stale_facts": sol["stale_facts"],
            "current_replicas": current_replicas,
            "action": "HOLD_REPLICAS",
            "solve_time_us": sol["solve_time_us"],
        }
        print(json.dumps(event))
        return event

    target_replicas = sol["target_pods"]
    allocated_workers = sol["allocated_workers"]
    STATE["target_pods"] = target_replicas
    STATE["allocated_workers"] = allocated_workers
    STATE["decision_blocked"] = False
    STATE["stale_facts"] = []
    STATE["solve_time_us"] = sol["solve_time_us"]
    STATE["last_reconcile"] = now_iso

    action = "NO_CHANGE"
    if target_replicas != current_replicas:
        action = "SCALE_UP" if target_replicas > current_replicas else "SCALE_DOWN"
        if not dry_run:
            try:
                subprocess.run(
                    ["kubectl", "scale", f"deployment/{DEPLOYMENT_NAME}", f"--replicas={target_replicas}", "-n", NAMESPACE],
                    capture_output=True, check=True
                )
                subprocess.run(
                    [
                        "kubectl", "annotate", f"deployment/{DEPLOYMENT_NAME}", "-n", NAMESPACE, "--overwrite",
                        f"aven.sh/target-pods={target_replicas}",
                        f"aven.sh/allocated-workers={allocated_workers}",
                        f"aven.sh/last-reconcile={now_iso}",
                        "aven.sh/freshness=enforced",
                    ],
                    capture_output=True, check=True
                )
                STATE["current_pods"] = target_replicas
            except Exception:
                pass  # in simulation or offline mode

    event = {
        "timestamp": now_iso,
        "action": action,
        "current_replicas": current_replicas,
        "target_replicas": target_replicas,
        "allocated_workers": allocated_workers,
        "solve_time_us": sol["solve_time_us"],
        "telemetry": {
            "reqs_per_second": reqs_per_second,
            "avg_latency_ms": avg_latency_ms,
        },
        "dry_run": dry_run,
    }
    print(json.dumps(event))
    return event


class MetricsHandler(BaseHTTPRequestHandler):
    """HTTP server exposing Prometheus metrics and K8s Custom Metrics API."""

    def do_GET(self):
        if self.path == "/healthz":
            self.send_response(200)
            self.end_headers()
            self.wfile.write(b"OK")
        elif self.path == "/metrics":
            # Prometheus Metrics format
            self.send_response(200)
            self.send_header("Content-Type", "text/plain; version=0.0.4")
            self.end_headers()
            blocked_val = 1 if STATE["decision_blocked"] else 0
            body = (
                f"# HELP aven_target_pods Target pod replicas calculated by Aven Newton solver\n"
                f"# TYPE aven_target_pods gauge\n"
                f"aven_target_pods {STATE['target_pods']}\n"
                f"# HELP aven_current_pods Current pod replicas active in deployment\n"
                f"# TYPE aven_current_pods gauge\n"
                f"aven_current_pods {STATE['current_pods']}\n"
                f"# HELP aven_decision_blocked Indicator if scaling decision is blocked due to stale facts\n"
                f"# TYPE aven_decision_blocked gauge\n"
                f"aven_decision_blocked {blocked_val}\n"
                f"# HELP aven_solve_latency_us Solver latency in microseconds\n"
                f"# TYPE aven_solve_latency_us gauge\n"
                f"aven_solve_latency_us {STATE['solve_time_us']}\n"
            )
            self.wfile.write(body.encode("utf-8"))
        elif "custom.metrics.k8s.io" in self.path or "/apis/custom.metrics" in self.path:
            # Kubernetes Custom Metrics API format
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.end_headers()
            resp = {
                "kind": "MetricValueList",
                "apiVersion": "custom.metrics.k8s.io/v1beta1",
                "metadata": {"selfLink": self.path},
                "items": [
                    {
                        "describedObject": {
                            "kind": "Service",
                            "namespace": NAMESPACE,
                            "name": "aven-workload-svc",
                            "apiVersion": "/v1",
                        },
                        "metricName": "aven_target_pods",
                        "timestamp": STATE["last_reconcile"],
                        "value": str(STATE["target_pods"]),
                    }
                ],
            }
            self.wfile.write(json.dumps(resp).encode("utf-8"))
        else:
            self.send_response(404)
            self.end_headers()

    def log_message(self, format, *args):
        pass  # Suppress HTTP server noisy console logging


def start_server(port: int = 8080):
    server = HTTPServer(("0.0.0.0", port), MetricsHandler)
    t = threading.Thread(target=server.serve_forever, daemon=True)
    t.start()
    return server


def main():
    import argparse
    parser = argparse.ArgumentParser(description="Aven Kubernetes Capacity Controller")
    parser.add_argument("rps", type=float, nargs="?", default=10000.0, help="Requests per second baseline")
    parser.add_argument("--latency", type=float, default=20.0, help="Average request latency in ms")
    parser.add_argument("--inject-stale", action="store_true", help="Simulate malicious / expired TTL=0 input")
    parser.add_argument("--dry-run", action="store_true", help="Simulate scaling decision without touching cluster")
    parser.add_argument("--serve", type=int, nargs="?", const=8080, help="Start HTTP Prometheus/Custom Metrics server on port (default: 8080)")
    parser.add_argument("--loop", type=float, help="Run continuous reconciliation loop with interval in seconds")
    args = parser.parse_args()

    if args.serve:
        start_server(args.serve)
        print(f"Aven Metrics Server listening on :http://0.0.0.0:{args.serve}/metrics")

    if args.loop:
        print(f"Starting continuous reconciliation loop (interval: {args.loop}s)...")
        while True:
            reconcile(args.rps, args.latency, args.inject_stale, args.dry_run)
            time.sleep(args.loop)
    else:
        reconcile(args.rps, args.latency, args.inject_stale, args.dry_run)


if __name__ == "__main__":
    main()
