# Mesh simulator — stage 1 (stub mesh)

A local, model-independent mesh that reproduces iSharded's observable interface, so the
whole Aven live-data path can run end-to-end on our hardware. See [PLAN.md](PLAN.md) for
the full staged design (this is stage 1: stub workers, no models yet).

```
 worker-1 ─┐
 worker-2 ─┤   coordinator ── /admin/status (iSharded-shaped) ──► isharded_poller.py ──► avenc stream
 worker-3 ─┤   (token loop,                                                               (route model +
 worker-4 ─┘    telemetry)                                                                 thresholds → ALARM)
```

Everything here is **Python stdlib only** — no dependencies, matching the `avenc` core.

## Run it (Docker)

```bash
docker compose up --build
```

Then, on the host (needs a built `avenc` on PATH, or use the full path):

```bash
python gen_route.py --from-url http://127.0.0.1:8080 --key dev-user-key -o route.aven
```

```bash
ISHARDED_URL=http://127.0.0.1:8080 ISHARDED_KEY=dev-user-key python ../isharded_poller.py --interval 2 | avenc stream route.aven
```

You'll see `SETTLED` lines with a live `avg_hop_ms` each tick. On Windows, `run-monitor.ps1`
wraps those two commands.

## Fault injection (the detect→alarm loop)

```bash
docker compose stop worker-3
```

The token loop can't complete, `tokens_per_second_ema` collapses, and `avenc stream` prints
`ALARM: observed_rate … violates bound …`. `docker compose start worker-3` recovers it.

## Run without Docker (quick check)

Each piece is a plain process, so you can smoke-test the pipeline directly. Use
`127.0.0.1`, not `localhost` — on Windows `localhost` resolves to IPv6 `::1` first and the
stdlib HTTP server binds IPv4, so each request stalls ~2 s before falling back. (Inside
Docker, service names resolve to IPv4, so this only affects local runs.)

```bash
WORKER_ID=w1 LAYERS=23 PORT=9001 python worker/worker.py &
WORKER_ID=w2 LAYERS=24 PORT=9002 python worker/worker.py &
WORKER_ID=w3 LAYERS=24 PORT=9003 python worker/worker.py &
WORKER_ID=w4 LAYERS=23 PORT=9004 python worker/worker.py &
ROUTE="http://127.0.0.1:9001,http://127.0.0.1:9002,http://127.0.0.1:9003,http://127.0.0.1:9004" PORT=9080 USER_KEY=dev python coordinator/coordinator.py &
python gen_route.py --from-url http://127.0.0.1:9080 --key dev -o /tmp/route.aven
ISHARDED_URL=http://127.0.0.1:9080 ISHARDED_KEY=dev python ../isharded_poller.py --interval 2 | avenc stream /tmp/route.aven
```

## What this proves / what's next

- **Proves:** the poller → `avenc stream` → threshold-`ALARM` pipeline works against an
  iSharded-shaped API, with no code changes to the poller — so swapping in the real mesh is
  an endpoint change.
- **Stage 2 (now in `docker-compose.models.yml`):** the worker proxies `/infer` to a `llama.cpp`
  server per model — real inference, real compute. See "Stage 2" below.
- **Stage 3:** move a worker + the coordinator + monitor to the Linux server over Tailscale,
  for real cross-host latency (and Nova posts the alarms to Discord).

## Stage 2 — real models (llama.cpp)

`docker-compose.models.yml` runs four `llama.cpp` servers (one GGUF each) behind the same worker
adapters, so `/infer` runs a real completion and reports its wall time. Point `MODELS_DIR` at the
folder holding `red/green/blue/gold/model.gguf`:

```bash
MODELS_DIR=C:/Users/attho/ai-hacking-arena/models docker compose -f docker-compose.models.yml up --build
```

The monitor is unchanged (coordinator still on `127.0.0.1:8080`): generate the route model with
`gen_route.py --from-url`, then `isharded_poller.py | avenc stream route.aven`. Now
`tokens_per_second_ema` reflects real inference.

Notes:
- CPU-only (Docker Desktop's VM has no GPU), so inference is real but slower than on the host. All
  four models together need ~9 GB RAM — comment stages out of the compose (and the coordinator
  `ROUTE`) to run fewer.
- Same worker image as stage 1; setting `LLM_URL` switches `/infer` from the modelled sleep to a
  real `llama.cpp` completion (`N_PREDICT` tokens per call, default 8). On the server with a GPU,
  point `LLM_URL` at a host `llama-server` (like Nova does) instead of the CPU container.

## Files

| Path | Role |
|---|---|
| `worker/worker.py` | one route stage: `/infer` (sleeps modelled compute), `/telemetry`, `/internal/diag/probe` |
| `coordinator/coordinator.py` | token loop + aggregated `/admin/status`, `/admin/inventory` (iSharded shape) |
| `gen_route.py` | emit an N-stage Aven model; `--from-url` matches the live route |
| `docker-compose.yml` | 4 workers + coordinator |
| `run-monitor.ps1` | host-side: generate the model + run the live monitor (Windows) |
