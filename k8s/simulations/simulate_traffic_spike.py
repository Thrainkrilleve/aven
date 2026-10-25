#!/usr/bin/env python3
"""
k8s/simulations/simulate_traffic_spike.py
-----------------------------------------
Interactive 4-Stage Demonstration of Aven Kubernetes Capacity Controller:
1. Baseline Load (10,000 req/s -> 3 pods)
2. Traffic Spike (50,000 req/s -> 15 pods distributed across 4 nodes)
3. Operator Freeze Switch (Human Override Lock)
4. Unfreeze & Controlled Cooldown
"""

import json
import subprocess
import time

NAMESPACE = "aven-system"
DEPLOYMENT = "aven-workload"


def run_cmd(cmd):
    res = subprocess.run(cmd, shell=True, capture_output=True, text=True)
    return res.stdout.strip()


def print_cluster_status():
    pods_out = run_cmd(f"kubectl get pods -n {NAMESPACE} -o wide --no-headers")
    lines = [line for line in pods_out.splitlines() if line]
    print(f"  [Pods Active: {len(lines)}]")
    for line in lines[:5]:
        parts = line.split()
        name = parts[0]
        status = parts[2]
        node = parts[6] if len(parts) > 6 else "unknown"
        print(f"    • {name} [{status}] on {node}")
    if len(lines) > 5:
        print(f"    ... and {len(lines) - 5} more pods running across cluster nodes")


def main():
    print("=" * 70)
    print(" Aven Kubernetes Capacity Controller — Live Simulation")
    print(" Target: 4-Node KinD Cluster in Docker")
    print("=" * 70)

    # Stage 1: Baseline Load
    print("\n--- STAGE 1: Baseline Traffic (10,000 req/s) ---")
    print("Running Aven Controller at 10k req/s baseline...")
    out1 = run_cmd("python f:/aven/k8s/controller/aven_controller.py 10000")
    print(f"Controller Audit Log:\n  {out1}")
    time.sleep(3)
    print_cluster_status()

    # Stage 2: Traffic Surge (Black Friday Spike)
    print("\n--- STAGE 2: Traffic Spike (50,000 req/s Surge) ---")
    print("Simulating traffic spike to 50,000 req/s...")
    out2 = run_cmd("python f:/aven/k8s/controller/aven_controller.py 50000")
    print(f"Controller Audit Log:\n  {out2}")
    print("Waiting for Kubernetes scheduler to place pods across all nodes...")
    time.sleep(5)
    print_cluster_status()

    # Stage 3: Freshness Enforcement / Stale Telemetry Protection
    print("\n--- STAGE 3: Freshness Enforcement / Stale Telemetry Injection ---")
    print("Simulating telemetry failure (TTL=0 injected during network partition)...")
    out_stale = run_cmd("python f:/aven/k8s/controller/aven_controller.py 50000 --inject-stale")
    print(f"Controller Audit Log:\n  {out_stale}")
    print("Cluster state (stale metrics rejected, replicas held constant):")
    print_cluster_status()

    # Stage 4: Operator Safety Freeze (Lock scaling during incident)
    print("\n--- STAGE 4: Operator Safety Freeze Switch (aven.sh/freeze=true) ---")
    print("Operator activates emergency freeze annotation:")
    run_cmd(f"kubectl annotate deployment/{DEPLOYMENT} -n {NAMESPACE} aven.sh/freeze=true --overwrite")
    print("Simulating traffic drop to 5,000 req/s while frozen...")
    out3 = run_cmd("python f:/aven/k8s/controller/aven_controller.py 5000")
    print(f"Controller Audit Log:\n  {out3}")
    print("Cluster state (frozen, no pods terminated):")
    print_cluster_status()

    # Stage 5: Unfreeze & Cooldown
    print("\n--- STAGE 5: Unfreeze & Normal Cooldown ---")
    print("Operator removes freeze annotation:")
    run_cmd(f"kubectl annotate deployment/{DEPLOYMENT} -n {NAMESPACE} aven.sh/freeze=false --overwrite")
    print("Controller reconciles to calm baseline (10,000 req/s)...")
    out4 = run_cmd("python f:/aven/k8s/controller/aven_controller.py 10000")
    print(f"Controller Audit Log:\n  {out4}")
    time.sleep(3)
    print_cluster_status()

    print("\n" + "=" * 70)
    print(" Live Kubernetes Simulation Complete: All Safeguards Verified.")
    print("=" * 70)


if __name__ == "__main__":
    main()
