#!/usr/bin/env python3
"""quick_orchestrator_test.py — throws flame_eval into a tiny loop."""

import json
import subprocess
import random

def call_flame(x, y):
    payload = json.dumps({"x": x, "y": y})
    proc = subprocess.run(
        ["python3", "flame_eval.py"],
        input=payload,
        text=True,
        capture_output=True,
        timeout=2.0,
    )
    if proc.returncode != 0:
        return None, proc.stderr.strip()
    return json.loads(proc.stdout), None

def main():
    print("Throwing flame_eval into the fire...\n")
    successes = 0
    failures = 0
    for i in range(20):
        x = random.uniform(-2, 2)
        y = random.uniform(-2, 2)
        result, err = call_flame(x, y)
        if err:
            failures += 1
            print(f"[{i:02d}] FAIL  x={x:.3f} y={y:.3f}  {err}")
        else:
            successes += 1
            print(f"[{i:02d}] OK    x={x:.3f} y={y:.3f}  value={result['value']:.4f}  backend={result['backend_id']}")
    print(f"\nDone: {successes} ok, {failures} failures")

if __name__ == "__main__":
    main()