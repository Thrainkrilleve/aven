#!/usr/bin/env python3
import json, sys, subprocess

# Read the latest settled value (or hard-code for the demo)
replicas = 10   # start small; change to int(settled["max_100m_pods"]) later

yaml = f"""
apiVersion: apps/v1
kind: Deployment
metadata:
  name: aven-recommended
spec:
  replicas: {replicas}
  selector:
    matchLabels:
      app: aven-demo
  template:
    metadata:
      labels:
        app: aven-demo
    spec:
      containers:
      - name: pause
        image: registry.k8s.io/pause:3.9
        resources:
          requests:
            cpu: "100m"
            memory: "64Mi"
"""

print(yaml)
# Optional: apply it
# subprocess.run(["kubectl", "apply", "-f", "-"], input=yaml, text=True)