#!/usr/bin/env python3
"""
flame_eval.py — nasty external evaluator for Aven orchestrator tests.

Protocol:
  stdin:  JSON object of variable name -> float
  stdout: JSON {"value": float, "jacobian": null, "backend_id": "flame_eval", "warnings": [...]}
  exit 0 on success, non-zero on hard failure

Intentional pain:
  - 50–300 ms sleep (simulates heavy solver)
  - discontinuous / multi-modal function
  - occasional random failure (5 %)
  - rejects NaN/Inf and missing required vars
  - no Jacobian ever
"""

import sys
import json
import time
import random
import math

REQUIRED = ["x", "y"]          # orchestrator must supply these
BACKEND_ID = "flame_eval"

def nasty_function(x: float, y: float) -> float:
    """Multi-modal, discontinuous, mildly evil."""
    # base multimodal
    val = math.sin(5 * x) * math.cos(3 * y) + 0.1 * (x**2 + y**2)
    # discontinuity wall
    if x > 1.5:
        val += 10.0
    # narrow spike of misery
    if abs(x - 0.3) < 0.05 and abs(y + 0.7) < 0.05:
        val += 50.0
    # cheap noise
    val += random.uniform(-0.05, 0.05)
    return val

def main() -> None:
    try:
        raw = sys.stdin.read()
        if not raw.strip():
            raise ValueError("empty stdin")
        vars = json.loads(raw)
    except Exception as e:
        print(json.dumps({"error": f"bad input: {e}"}), file=sys.stderr)
        sys.exit(2)

    # required variables
    missing = [k for k in REQUIRED if k not in vars]
    if missing:
        print(json.dumps({"error": f"missing vars: {missing}"}), file=sys.stderr)
        sys.exit(3)

    try:
        x = float(vars["x"])
        y = float(vars["y"])
    except (TypeError, ValueError) as e:
        print(json.dumps({"error": f"non-numeric: {e}"}), file=sys.stderr)
        sys.exit(4)

    if not (math.isfinite(x) and math.isfinite(y)):
        print(json.dumps({"error": "NaN/Inf rejected"}), file=sys.stderr)
        sys.exit(5)

    # simulate heavy / flaky external solver
    time.sleep(random.uniform(0.05, 0.30))

    # 5 % hard failure
    if random.random() < 0.05:
        print(json.dumps({"error": "simulated solver crash"}), file=sys.stderr)
        sys.exit(6)

    value = nasty_function(x, y)

    result = {
        "value": value,
        "jacobian": None,
        "backend_id": BACKEND_ID,
        "warnings": ["black-box", "noisy", "discontinuous"]
    }
    json.dump(result, sys.stdout)
    sys.stdout.write("\n")

if __name__ == "__main__":
    main()