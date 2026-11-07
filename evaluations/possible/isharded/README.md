# iSharded route model (draft)

A first Aven model of where the time goes when [iSharded](https://github.com/AuXFire/iSharded) serves a model across many machines. It is a draft to discuss with iSharded's developer: several inputs are published specifications or assumptions, marked as such in [`isharded_route.aven`](isharded_route.aven), and should be replaced with what iSharded measures.

## What it models

For one chat stream, each token passes through every stage of the route in order. The entry worker runs the token loop, and each stage POSTs its activations to the next and waits for the reply, so a sampled token unwinds back along the same links. The time per token is every stage's compute time plus every link's round trip:

- **Stage compute:** decoding is limited by memory bandwidth, so a stage's time is the bytes it reads per token divided by how fast its memory delivers them. Qwen3-235B-A22B is a mixture-of-experts model, so a token reads its 22B active parameters, not all 235B. Weights a stage holds in system RAM (routed experts) are read by the CPU at RAM speed.
- **Network:** 6 links between consecutive stages, each a round trip (activations out, reply back — there is no separate exit-to-entry hop), at an average round-trip time per link, all overhead included.

The inputs are the 7-machine run of Qwen3-235B-A22B-Instruct-2507 Q6_K: layer split and RAM use from the dashboard, 172.52 GiB of weights, 1.01 tokens/s measured in chat, and about 4.9 tokens/s planned.

## What it says (with the draft's assumptions)

Run as written, Aven solves backward from the measured 1.01 tokens/s:

| | |
|---|---|
| Compute per token, all 7 stages | 106 ms (would allow about 9.4 tokens/s) |
| Network per token | 884 ms, **89% of the time** |
| Average round-trip per link that explains 1.01 tokens/s | **147 ms** |
| Average round-trip per link the planner's 4.9 tokens/s implies | 16 ms |
| Slowest stage | ro-pro4000-gpu, 66 ms of the 106: 14.77 GiB of its weights are in RAM |

Switching to prediction (give `avg_hop_ms`, remove `observed_rate`):

| Round-trip per link | Tokens/s | Network share |
|---|---|---|
| 147 ms (as measured) | 1.0 | 89% |
| 50 ms | 2.5 | 74% |
| 20 ms | 4.4 | 53% |
| 5 ms (same region) | 7.3 | 22% |
| 20 ms, and ro-pro4000-gpu's RAM share moved to VRAM | 5.9 | 71% |

So, if the assumptions hold: the route spends most of each token on the network, the planner's estimate assumes links about nine times faster than this run got, and the one stage that spills to RAM costs more than the other six together.

## Assumptions to replace

| Input | Draft value | Better source |
|---|---|---|
| Memory bandwidth per GPU | Published specs (A40 696 GB/s, RTX PRO 4000 672, RX 7900 XT 800, RTX 3060 Laptop 336); pa-mig-gpu assumed half of an RTX PRO 6000's 1792 | The bandwidth iSharded measures per pool (in `POST /admin/plan`) |
| `efficiency` | 0.6 of peak | Measured compute time per stage — already in `/admin/status` as `workers[].runtime.telemetry.compute_ms_ema` (user key) |
| `ram_bw` | 40 GB/s | Measured on the RAM-spilling workers |
| `avg_hop_ms` | Solved from the measured speed | Per-link round-trip timings between consecutive stages (iSharded measures these but does not yet publish them — see the plan). The dashboard's "Control RTT" is each node's round trip to the coordinator, not to its neighbour |
| Bytes read per token | Active share of the whole file, spread evenly over 94 layers | Per-stage bytes-read-per-token from the planner (dense weights, experts, embedding, head), if it exposes them — see the questions below |

## Using it in the IDE

Open `isharded_route.aven`. Every value in the solve panel can be toggled to "solve for this": for example, fix `observed_rate` at a target such as 5 token/s and solve for `avg_hop_ms`, or sweep `avg_hop_ms` from 5 to 150 ms and chart `observed_rate`. `avenc why ... avg_hop_ms` (or clicking the value) shows how each input contributes.

From the command line:

```bash
avenc run   evaluations/possible/isharded/isharded_route.aven
avenc why   evaluations/possible/isharded/isharded_route.aven avg_hop_ms
avenc check evaluations/possible/isharded/isharded_route.aven
```

`avenc run` produces the breakdown above. `avenc check` passes the dimensional analysis (all 17 equations) and then reports the model as **solved backward** — because the measured `observed_rate` is pinned and `avg_hop_ms` is left for Aven to find, the relations alone have one degree of freedom, which `avenc run` settles. That note is expected here, not a fault.

## Live data

[LIVE_DATA_PLAN.md](LIVE_DATA_PLAN.md) reviews what iSharded already exposes, what is missing, and the options for feeding live readings into this model, the IDE and `avenc stream`.

## Questions for iSharded's developer

1. Does `POST /admin/plan` expose, per stage, the bytes read per token and the measured bandwidth of each pool? Those would replace most of the assumptions above.
2. How does the planner estimate link time, and from what measurement? Its 4.9 tokens/s implies about 16 ms round trip per link.
3. Workers already report their compute and their downstream-wait time (`compute_ms_ema`, `downstream_ms_ema`, `phases_ms`) in `/admin/status`, so a per-hop transfer time can be derived. Decode-only values of those (and, if you'd rather publish a figure than have Aven derive one, an explicit per-hop network timer) would let Aven check every live token against the plan (`avenc stream`) and flag a stage or link that falls behind.
4. Is the layer split for a fixed order of machines something you would want proven optimal? With the order fixed, choosing how many layers each stage gets is a small integer problem (one variable per stage) that Aven can solve and certify, while iSharded keeps searching the orders.
