# Wiring in Nova

Nova plugs into the mesh-sim at two independent points. Neither changes the
AVEN core, the AERIC control law, or the dashboard logic.

```
                        ┌─────────────── Role A: alarms ───────────────┐
  coordinator ──/admin/status──►  control_panel.py ──► nova_bridge ──► Discord
   (token loop)                   (event detector)      (webhook)
        │
        └──/infer──►  worker ──LLM_URL──►  llama.cpp
                                              ▲
                     Role B: point this at Nova's GPU llama-server
```

## Role A — Nova as the Discord alarm sink

`control_panel.py` watches the live signals and fires a **debounced, edge-triggered**
event (once on entering/leaving a bad state, not every tick):

| Health | Condition |
|---|---|
| `RATE_LOW` | observed `< NOVA_MIN_RATE` (default 0.1 tok/s) |
| `PACE_SAT` | throttle pinned at the 5000 ms envelope cap — out of downward authority |
| `CEILING` | loop running and target > `model_rate` ceiling — target unreachable |
| `DISCONNECTED` | coordinator unreachable |
| `OK` | recovered from any of the above |

### Go live
The webhook is a **secret** — it is read from the environment, never committed,
never written to the ledger or any CSV. Set it in the shell that launches the panel:

```bash
export NOVA_WEBHOOK="https://discord.com/api/webhooks/XXXX/YYYY"   # your channel webhook
export NOVA_MODE=live        # 'dry' (default) only logs what it would post
python control_panel.py --port 8090 --mesh http://127.0.0.1:8080 --key dev-user-key
```

On Windows PowerShell:

```powershell
$env:NOVA_WEBHOOK = "https://discord.com/api/webhooks/XXXX/YYYY"
$env:NOVA_MODE = "live"
python control_panel.py --port 8090
```

Tunables (env): `NOVA_MIN_RATE` (rate floor), `NOVA_DEBOUNCE` (polls before firing, default 3).

Use the dashboard's **Nova alarms → Send test ping** to confirm delivery before
relying on it. In `dry` mode the ping is logged in the panel (and nothing leaves
the machine); in `live` mode it posts to the channel.

## Role B — Nova as the GPU inference backend

[`docker-compose.nova.yml`](docker-compose.nova.yml) drops the four CPU
`llama.cpp` containers and points every worker's `/infer` at Nova's host
`llama-server` over Tailscale:

```bash
NOVA_LLM_URL=http://nova:8080 docker compose -f docker-compose.nova.yml up -d --build
```

- GPU inference raises `tokens_per_second`, so the `model_rate` **ceiling climbs**
  and the controller can hold higher targets (the 0.6 that railed on CPU).
- If each colour is a separate server on Nova, set `NOVA_LLM_URL_GREEN`, `…_BLUE`,
  `…_GOLD` (each falls back to `NOVA_LLM_URL`).
- If containers can't resolve `nova`, use its `100.x` Tailscale IP.

## Split-host chain — data crossing machine to machine

To show the token physically flowing stage -> stage -> host -> host (not a star
through the coordinator), run the stages as a **chain** split across two machines.
Each worker's `NEXT_URL` points at the next stage; the coordinator (`CHAIN=1`)
kicks stage 1 and the token propagates down and unwinds back. The per-stage hop
is then the real RTT to the *next stage's machine*, so a cross-host link shows up
as one big bar while co-located hops stay sub-ms.

```
 coordinator(Win) -> red(Win) -> green(Win) --tailnet--> blue(Nova) -> gold(Nova)
                                             ^ the machine-to-machine crossing
```

**On Nova** (needs this repo checked out and the local llama-server on :8780):

```bash
cd <repo>/evaluations/possible/isharded/mesh-sim/worker
bash ../nova_chain_stages.sh        # starts blue(:9303) + gold(:9304), bound 0.0.0.0
```

**On Windows** (after the Nova stages are listening):

```bash
NOVA_IP=100.85.239.122 NOVA_LLM_URL=http://100.85.239.122:8781 \
  docker compose -f docker-compose.chain.yml up -d --build
```

`green` forwards to `blue` at `NOVA_IP:9303` over the tailnet; the coordinator
pulls telemetry from all four. In the dashboard the per-stage hop chart shows
`red->green` and `blue->gold` sub-ms (same host) and `green->blue` at the tailnet
RTT -- the activation data crossing from the Windows box to Nova. Stop the Nova
stages with `bash nova_chain_stages.sh stop`.

## Stage 3 — the distributed milestone (runs on your side)

To get *real* cross-host hop latency instead of localhost ~5 ms:

1. **Nova** runs the GPU `llama-server` (Role B) and is reachable over Tailscale.
2. Run one or more **workers** on Nova (or other tailnet hosts); point the
   coordinator `ROUTE` at their tailnet addresses. Now `downstream_ms_ema` reflects
   true network latency, and the measured model's `network_ms` becomes meaningful.
3. Run the **coordinator + control panel** wherever you want the operator view;
   set `NOVA_WEBHOOK`/`NOVA_MODE=live` so alarms reach Discord (Role A).
4. The AERIC daemon + loop are unchanged — still solving `mesh_pace.aven` each tick
   and driving `/admin/control`, now across the real tailnet.

What I need from you to finish wiring: Nova's `llama-server` base URL (Tailscale
name/IP + port) for Role B, and the Discord webhook set as `NOVA_WEBHOOK` (in the
shell, not pasted anywhere) for Role A.
