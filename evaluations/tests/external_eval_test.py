#!/usr/bin/env python3
"""external_eval_test.py — Python stand-in for Aven's ExternalEvaluator."""

import json
import subprocess
import sys

def external_evaluate(vars_dict, timeout=2.0):
    """Call flame_eval.py exactly as an ExternalEvaluator would."""
    payload = json.dumps(vars_dict)
    try:
        proc = subprocess.run(
            [sys.executable, "flame_eval.py"],
            input=payload,
            text=True,
            capture_output=True,
            timeout=timeout,
        )
    except subprocess.TimeoutExpired:
        return None, "timeout"

    if proc.returncode != 0:
        return None, proc.stderr.strip() or f"exit {proc.returncode}"

    try:
        result = json.loads(proc.stdout)
        return result, None
    except json.JSONDecodeError as e:
        return None, f"bad json: {e}"

def main():
    tests = [
        {"x": 0.5, "y": -0.2},
        {"x": 1.0},                    # missing y → should fail
        {"x": 0.3, "y": -0.7},         # spike region
        {"x": 2.0, "y": 0.0},          # discontinuity wall
    ]

    for i, vars_dict in enumerate(tests):
        result, err = external_evaluate(vars_dict)
        if err:
            print(f"[{i}] FAIL  {vars_dict}  ->  {err}")
        else:
            print(f"[{i}] OK    {vars_dict}  ->  value={result['value']:.4f}  "
                  f"backend={result['backend_id']}")

if __name__ == "__main__":
    main()