# iSharded route model (draft)

A first Aven model of where the time goes when [iSharded](https://github.com/AuXFire/iSharded) serves a model across many machines. It is a draft to discuss with iSharded's developer: several inputs are published specifications or assumptions, marked as such in [`isharded_route.aven`](isharded_route.aven), and should be replaced with what iSharded measures.

## What it models

For one chat stream, each token passes through every stage of the route in order, then goes from the exit back to the entry for the next token. So the time per token is every stage's compute time plus every network hop:

- **Stage compute:** decoding is limited by memory bandwidth, so a stage's time is the bytes it reads per token divided by how fast its memory delivers them. Qwen3-235B-A22B is a mixture-of-experts model, so a token reads its 22B active parameters, not all 235B. Weights a stage holds in system RAM (routed experts) are read by the CPU at RAM speed.
- **Network:** 7 links (6 between stages, plus exit to entry) at an average one-way delay per link, all overhead included.

The inputs are the 7-machine run of Qwen3-235B-A22B-Instruct-2507 Q6_K: layer split and RAM use from the dashboard, 172.52 GiB of weights, 1.01 tokens/s measured in chat, and about 4.9 tokens/s planned.

## What it says (with the draft's assumptions)

Run as written, Aven solves backward from the measured 1.01 tokens/s:

| | |
|---|---|
| Compute per token, all 7 stages | 106 ms (would allow about 9.4 tokens/s) |
| Network per token | 884 ms, **89% of the time** |
| Average delay per link that explains 1.01 tokens/s | **126 ms** |
| Average delay per link the planner's 4.9 tokens/s implies | 14 ms |
| Slowest stage | ro-pro4000-gpu, 66 ms of the 106: 14.77 GiB of its weights are in RAM |

Switching to prediction (give `avg_hop_ms`, remove `observed_rate`):

| Delay per link | Tokens/s | Network share |
|---|---|---|
| 126 ms (as measured) | 1.0 | 89% |
| 50 ms | 2.2 | 77% |
| 20 ms | 4.1 | 57% |
| 5 ms (same region) | 7.1 | 25% |
| 20 ms, and ro-pro4000-gpu's RAM share moved to VRAM | 5.3 | 74% |

So, if the assumptions hold: the route spends most of each token on the network, the planner's estimate assumes links about nine times faster than this run got, and the one stage that spills to RAM costs more than the other six together.

## Assumptions to replace

| Input | Draft value | Better source |
|---|---|---|
| Memory bandwidth per GPU | Published specs (A40 696 GB/s, RTX PRO 4000 672, RX 7900 XT 800, RTX 3060 Laptop 336); pa-mig-gpu assumed half of an RTX PRO 6000's 1792 | The bandwidth iSharded measures per pool (in `POST /admin/plan`) |
| `efficiency` | 0.6 of peak | Measured: a stage's compute time per token |
| `ram_bw` | 40 GB/s | Measured on the RAM-spilling workers |
| `avg_hop_ms` | Solved from the measured speed | Per-link timings between consecutive stages. The dashboard's "Control RTT" is each node's round trip to the coordinator, not to its neighbour |
| Bytes read per token | Active share of the whole file, spread evenly over 94 layers | Per-stage figures from the planner, which already separates dense weights, experts, the embedding and the output head |

## Using it in the IDE

Open `isharded_route.aven`. Every value in the solve panel can be toggled to "solve for this": for example, fix `observed_rate` at a target such as 5 token/s and solve for `avg_hop_ms`, or sweep `avg_hop_ms` from 5 to 150 ms and chart `observed_rate`. `avenc why ... avg_hop_ms` (or clicking the value) shows how each input contributes.

From the command line:

```bash
avenc check evaluations/possible/isharded/isharded_route.aven
avenc run   evaluations/possible/isharded/isharded_route.aven
avenc why   evaluations/possible/isharded/isharded_route.aven avg_hop_ms
```

## Questions for iSharded's developer

1. Does `POST /admin/plan` expose, per stage, the bytes read per token and the measured bandwidth of each pool? Those would replace most of the assumptions above.
2. How does the planner estimate link time, and from what measurement? Its 4.9 tokens/s implies about 14 ms per link.
3. Can workers report measured per-token compute time and per-hop transfer time? With those, Aven could check every live token against the plan (`avenc stream`) and flag a stage or link that falls behind.
4. Is the layer split for a fixed order of machines something you would want proven optimal? With the order fixed, choosing how many layers each stage gets is a small integer problem (one variable per stage) that Aven can solve and certify, while iSharded keeps searching the orders.
