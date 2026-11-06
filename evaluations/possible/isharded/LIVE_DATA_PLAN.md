# Live data from iSharded into Aven: review and options

A review of what it would take for Aven to read live numbers from an [iSharded](https://github.com/AuXFire/iSharded) mesh, to discuss with iSharded's developer. Nothing here is built yet. It goes with the draft model in this folder ([`isharded_route.aven`](isharded_route.aven), explained in [README.md](README.md)).

iSharded was reviewed at commit `063e3da` (2026-11-06); file and line references are to that version. Aven was reviewed on `main` at `014981e`.

## Contents

1. [Why Aven, and where it fits](#1-why-aven-and-where-it-fits)
2. [What the draft model found](#2-what-the-draft-model-found)
3. [What iSharded already exposes](#3-what-isharded-already-exposes)
4. [Gaps that need iSharded's developer](#4-gaps-that-need-isharded-s-developer)
5. [Ways to get the data into Aven](#5-ways-to-get-the-data-into-aven)
6. [Changes to the Aven model](#6-changes-to-the-aven-model)
7. [Access and security](#7-access-and-security)
8. [Suggested order of work](#8-suggested-order-of-work)
9. [Decisions and questions](#9-decisions-and-questions)

---

## 1. Why Aven, and where it fits

iSharded splits a model's layers across machines and sends each token through every one of them in turn. Deciding where the layers go, and checking that the result performs as expected, is a set of equations with hard limits (memory per machine, layers placed exactly once) and an objective (time per token). That is what Aven is for.

**Where Aven fits:**

| Use | What it gives | Status |
|---|---|---|
| Explain the route's speed | Expected tokens/s from layer sizes, memory speed and hop times, solvable in any direction ("what hop time do I need for 5 tokens/s?") | Draft model exists |
| Check the plan against reality | Measured speed and per-stage time against the planner's estimate, with the gap traced to a stage or link | Needs live data (this document) |
| Watch a running mesh | Each reading checked against limits; a stage or link that falls behind is flagged as an alarm | Needs live data (this document) |
| Prove a split optimal | For a fixed order of machines, how many layers each stage gets so the slowest stage is as fast as possible: a small integer problem (one variable per stage) that Aven solves and certifies | Not started |

**Where it does not fit:** running the model, moving weights between peers, networking, and recovering from failures. Those stay in iSharded (Python and llama.cpp).

**Limits to keep in mind:**

- Aven has no lists or arrays. A model over many stages is written with one set of variables per stage, which suits generated files better than hand-written ones.
- Aven's optimizer has been checked on small problems. Placing every individual layer (about 660 whole-number variables for 94 layers on 7 machines) has not been tried. The per-stage split (about 8 variables) is safe ground.

## 2. What the draft model found

From the 7-machine run of Qwen3-235B-A22B-Instruct-2507 Q6_K (172.52 GiB of weights, 1.01 tokens/s measured in chat, about 4.9 tokens/s planned), solving backward from the measured speed:

| | |
|---|---|
| Compute per token, all 7 stages | about 106 ms |
| Network per token | about 884 ms, **89% of the time** |
| Average delay per link that explains 1.01 tokens/s | **about 126 ms** |
| Delay per link the planner's 4.9 tokens/s implies | about 14 ms |
| Slowest stage | ro-pro4000-gpu: 66 ms of the 106, because 14.77 GiB of its weights are in system RAM |

These rest on stand-ins: GPU spec-sheet memory speeds, an assumed 60% efficiency and 40 GB/s RAM speed, and one average hop time solved from the measured speed. Live data replaces all of them.

## 3. What iSharded already exposes

All over its HTTP API (README, "HTTP API"), read-only:

| Data | Endpoint | Key | Source in iSharded |
|---|---|---|---|
| Measured tokens/s and ms per token (moving averages) | `GET /admin/status` → `metrics` | user | `orchestrator.py` `status()`, line 2384 |
| The route: each stage's worker and layers, the bytes and memory pool of every weight unit, and the planner's per-stage time estimate (`estimated_ms_per_token`) | `GET /admin/status` → `route`, `active_plan` | user | `protocol.py` `StageAssignment`, line 108 |
| The planner's overall estimate (`estimated_tokens_per_second`) and memory report | `GET /admin/status` → `active_plan` | user | `orchestrator.py` `describe_plan()`, line 454 |
| Measured compute time per pass for each worker (moving average) | `GET /metrics` → `isharded_worker_compute_ms{worker=…}` | admin | `orchestrator.py` `metrics_text()`, line 2115 |
| Moving average of time per token | `GET /metrics` → `isharded_token_latency_ms` | admin | same |
| Each node's round trip to the node you ask; GPU temperature, power, load | `GET /admin/inventory` | user | README |
| What a plan would be, without moving anything | `POST /admin/plan` | admin | README |

With these, measured per-stage compute replaces the draft's guessed memory speed, efficiency and RAM speed.

## 4. Gaps that need iSharded's developer

1. **Link times between workers are not published.** iSharded measures worker-to-worker round trips (`measure_links`, `orchestrator.py` line 1195, the median of a 100-byte probe) and the planner routes by them, but they stay inside the orchestrator (`runtime.links`). The only way to them is `/internal/diag/probe`, which needs the pool token. Asking for: the measured links in `/admin/status`, per pair of workers, with when each was measured.
2. **No timing of real hops.** The link measurements use 100-byte probes. Nothing records how long real activations take from one stage to the next while tokens are generated, including serialization and request overhead. That is the number that explains 1.01 against 4.9 tokens/s. Asking for: per-hop transfer time in each worker's telemetry, next to `compute_ms_ema`.
3. **The planner's estimate leaves out one hop.** `describe_plan()` (line 454) adds each stage's time and the links *between* consecutive stages, but not the trip from the exit back to the entry for the next token. It also assumes 25 ms (`HOP_MS`, `planner.py` line 33) for any link not yet measured. Both could make the estimate optimistic on a mesh that spans countries.
4. **Per-stage compute needs an admin key.** It is only in `/metrics`. Asking for: the same number in `/admin/status`, so a read-only user key is enough for monitoring.

## 5. Ways to get the data into Aven

What Aven supports today:

- `avenc stream` reads one JSON object per line, solves the model for each, and reports `SETTLED`, `ALARM` (a limit broken) or `UNSOLVED` (data missing; never filled in).
- `using external(...)` adapters feed `avenc run`; every value's source output is hashed into the audit log, and values past their time-to-live block the result (exit code 3).
- The IDE imports a CSV or xlsx of `name, value, unit` rows into a model's solve block (`import_sheet`).
- The IDE does **not** run adapters: its solve path deliberately skips external, IPC and shared-memory backends (`src/api.rs`, line 147).

| Option | Works today? | What it gives | Work needed |
|---|---|---|---|
| **A. Snapshot into the IDE** | Yes | Real numbers in the IDE, refreshed by hand: run the poller, then "import sheet" | A small poller that writes the CSV |
| **B. Live monitor** | Yes, command line | A reading every 10–30 s into `avenc stream`: alarms when measured speed falls well below plan, a stage runs slower than its estimate, or a link exceeds its budget | The same poller, printing JSON lines |
| **C. Live inside the IDE** | No | The IDE re-solves as new readings arrive | IDE feature. Safest form: "watch this CSV and re-import when it changes", rather than letting the editor run programs |
| **D. Adapter for `avenc run`** | Yes, command line | Each run records a hash of what the mesh reported, and refuses stale data | The poller packaged as an adapter |

**Recommendation:** start with A and B. They share one poller (it writes a CSV or prints JSON lines from the same readings) and need no changes to Aven. Add C if live updates in the IDE turn out to matter, and D if an audit trail of mesh readings is wanted.

## 6. Changes to the Aven model

- **Measurements instead of estimates.** Each stage's measured compute time becomes an input. The model then reports each stage's effective memory bandwidth (solved backward from its bytes per token) and its gap from the planner's `estimated_ms_per_token`.
- **Per-link hop times** instead of one average, once iSharded publishes them (gap 1), and real per-hop transfer times if they become available (gap 2).
- **Bytes per token from the plan.** The route lists each weight unit's bytes and memory pool, so per-stage reads can come from it instead of the draft's even split over 94 layers.
- **Limits for monitoring:** for example, measured speed at least some share of the planned speed; each stage within some margin of its estimate; no link over a set time. The margins are a choice to make with the developer.
- **Route shape.** The number of stages changes when the mesh rebalances, and Aven has no lists. Two ways:
  - **Fixed slots** (say 12 stages; unused ones set to zero): one stable file, simple for the IDE.
  - **Generated per route:** the poller writes a model that matches the current route exactly; a new file whenever the route changes.

## 7. Access and security

- **Keys:** a user key covers `/admin/status` and `/admin/inventory`. Per-stage compute is only in `/metrics`, which needs an admin key (see gap 4).
- **Where the key lives:** an environment variable on the machine running the poller. Never in the `.aven` file, the CSV or the repository.
- **Reaching the mesh:** the poller must reach a node: same LAN, Tailscale, or the mesh's TLS endpoint. Any node answers `/admin` requests (it passes them to the coordinator).
- **Load:** polling every 10–30 s is light. Requests without credentials are rate-limited; authenticated reads are not described as limited.
- **Testing:** a cloud session cannot reach a private mesh, so the first runs happen on a machine that can.

## 8. Suggested order of work

1. Agree with the developer on gaps 1–4 (section 4) and on the monitoring limits.
2. Write the poller: read `/admin/status` (and `/metrics` with an admin key), convert to the model's units, and write a CSV snapshot or JSON lines.
3. Update the model for measured inputs and the chosen route shape (section 6).
4. Try option A: import a snapshot in the IDE and compare measured against planned stage by stage.
5. Run option B on a live mesh during a test like the 7-machine run.
6. Once link times are published, add per-link hops and re-check the 1.01 vs 4.9 tokens/s gap.
7. Then decide on C (live IDE) and D (audit trail), and whether to try the provably optimal split.

## 9. Decisions and questions

**For you:**

1. Is a snapshot in the IDE (A) enough for now, or do you want live updates in the IDE (C)?
2. Fixed stage slots, or a model generated per route?
3. Which limits should raise an alarm while monitoring?

**For iSharded's developer:**

1. Can the measured worker-to-worker link times appear in `/admin/status`?
2. Can per-stage compute appear in `/admin/status`, so a read-only key is enough?
3. Could workers record real per-hop transfer time during generation?
4. Should the planner's estimate include the hop from the exit back to the entry, and is 25 ms the right fallback for unmeasured links?
5. Would a provably optimal layer split for a fixed order of machines be useful to the planner?
