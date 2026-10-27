# Kubernetes Orchestration & Infrastructure Automation

Aven can act as an invariant supervisor and continuous optimization engine for cloud infrastructure, Kubernetes clusters, and datacenter operations.

---

## 1. Why Aven for Infrastructure Automation?

Traditional Kubernetes Horizontal Pod Autoscalers (HPA) rely on crude scalar thresholds (e.g. CPU > 80%). This approach has severe limitations:
1. **Reactive Lag:** HPAs only scale *after* nodes are already overwhelmed and latency SLA spikes have occurred.
2. **Ignorance of Physical Constraints:** Scalar autoscalers cannot account for network framing overhead, kernel buffer allocations, ambient weather cooling costs, or power caps.
3. **Ghost Pods:** Stale Prometheus metrics can cause autoscalers to thrash or scale down under heavy traffic.

Aven bridges this gap by enforcing **relational capacity invariants** directly over infrastructure state.

---

## 2. Relational Datacenter Sizing Model

The controller's model is [`k8s/controller/hpa_scaling.aven`](https://github.com/Thrainkrilleve/aven/blob/main/k8s/controller/hpa_scaling.aven), a Little's Law sizing with a queueing-headroom ceiling whose traffic figures come from `workload_adapter.py`:

```aven-fragment
relation WorkloadCapacity using external("python", "k8s/controller/workload_adapter.py") {
    raw_workers = (reqs_per_second * avg_latency_ms) / 1000.0
    allocated_workers = ceil(raw_workers / target_utilization)
    target_pods = ceil(allocated_workers / workers_per_pod)
}

solve WorkloadCapacity with {
    workers_per_pod = 100.0
    target_utilization = 0.70
    target_pods >= 1.0
}
```

### The Omnidirectional Advantage
- **Forward mode:** given traffic and latency, compute the pods to run (what the controller does each cycle).
- **Backward capacity mode:** fix the limits and maximize the traffic instead. With at most 64 pods of 100 workers, 20 ms latency, 70% utilization, 64 KB payloads and a 40 Gbps link, `avenc run` proves the maximum is **75,120 req/s**, set by the network (22 pods suffice):

```aven
relation ServerScaling {
    int allocated_workers
    int allocated_pods
    raw_workers = (reqs_per_second * avg_latency_ms) / 1000.0
    allocated_workers >= raw_workers / target_utilization
    allocated_pods >= allocated_workers / workers_per_pod
    payload_gbps = (reqs_per_second * avg_payload_kb * 8.0) / 1000000.0
    wire_rate_gbps = payload_gbps * framing_overhead_ratio
}

solve ServerScaling with {
    avg_latency_ms = 20
    target_utilization = 0.7
    workers_per_pod = 100
    avg_payload_kb = 64
    framing_overhead_ratio = 1.04
    allocated_pods <= 64
    wire_rate_gbps <= 40.0
    maximize(reqs_per_second)
}
```

---

## 3. Free-Cooling Margins (Not Yet Wired to Kubernetes)

The weather monitor in `evaluations/weather/` computes a free-cooling margin per airport from live NWS data (e.g. `free_cooling_margin = 18.0 - temp_c`). Using that margin to move workloads between regions is a possible application; the controller in `k8s/controller/` does not do it, and no energy saving has been measured.

---

## 4. Trace Replay

`benchmarks/replay_trace_bench.py` replays `evaluations/data/wikimedia_traffic_trace.json` through `evaluations/traces/traffic_replay.aven`, one `avenc run` per step. Results are in `benchmarks/trace_benchmark_results.json`:
- 40 steps from 7,100 to 98,200 req/s, sized to 2–80 pods.
- All 39 fresh steps sized; step 25 carries `ttl_ms = 0` and `avenc` blocked it (`DECISION BLOCKED`, exit code 3). The harness asserts that exactly the stale steps are blocked and nothing fails.
- Median 32.8 ms and p95 43.0 ms per `avenc run` including process start-up (4-vCPU Linux container, 2026-10-27; an earlier Windows run measured 47.7 / 56.5 ms with identical sizing).

The trace's origin is not recorded in the repository. Wikimedia publishes pageview counts, not per-request latency or payload size, so treat the file as an illustrative traffic profile rather than a production capture.
