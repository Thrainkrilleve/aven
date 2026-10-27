# Aven DC Digital Twin

`dc.html` is a 2,500-node data-center dashboard driven entirely by the real Aven toolchain.
A browser cannot run `avenc` or open a raw TCP socket, so `dc_bridge.py` (Python standard
library only) serves the page and answers its requests by running the real tools.

```bash
cargo build --release
python evaluations/flight_mesh/flight_mesh_daemon.py --port 9090   # live telemetry daemon
python evaluations/possible/dc_bridge.py --port 8765               # then open http://127.0.0.1:8765/
```

## Where every number comes from

| Panel | Source |
| :--- | :--- |
| Node grid, facility power, PUE, temperatures | `avenc run --json` on a generated 2,500-node model (about 12,700 settled values per tick, about 0.35 s) |
| Outside air | The model's `Weather` relation is `using ipc("127.0.0.1:9090")`: avenc queries the live daemon and uses its METAR surface temperature. The daemon's `ttl_ms` is enforced by avenc, so stale weather blocks the solve (exit 3) |
| Workload | Real hourly English-Wikipedia pageviews from the Wikimedia REST API (last 72 hours), replayed one hour per tick. `cluster_util = 0.9 × views / window peak` |
| IPC stream | A direct query to the live daemon each tick: the exact JSON sent and received, and the round-trip time |
| Relation sandbox | `avenc run --json` solving `temp = base + load·heat_factor − cooling·cooling_factor` for cooling (backward Newton) or temperature (forward); `cooling >= 0` is enforced, so an impossible target is reported as a constraint violation |
| Trade-off sweep | `avenc pareto dc_fleet.aven cluster_util 0.05 0.95 --steps 18` with the live outside temperature and CRAC state; the knee comes from avenc; "now" is the current tick |
| Fact ledger | The tick's real `--ledger` audit log, checked with `avenc verify-ledger`; facility-level facts with their chain hashes, origins and the daemon's TTLs |

When a source fails (daemon down, API unreachable, stale data, a constraint violated) the
page shows avenc's actual error and marks any earlier solve as not current. It never
substitutes made-up values.

## Model and assumptions

Per node *i*: `u_i = cluster_util · w_i`, `p_i = idle_w + u_i · dynamic_w`, and exhaust
temperature `t_i = supply_c + recirc · row_rise + p_i / (ρ · c_p · Q)` (airflow heat transfer,
with part of each row's heat rise recirculated to its inlets). Supply air is the CRAC setpoint
when healthy and outside air plus an economizer rise when the CRAC has failed. Chiller COP
falls with outside temperature; PUE = facility power / IT power. The UPS limit
`it_kw <= 800` is checked by avenc on every tick.

Stated assumptions (in `PLANT` in `dc_bridge.py` and in `dc_fleet.aven`): 110 W idle and
240 W dynamic draw per server, 0.05 m³/s airflow per server, 18 °C setpoint, 4 °C economizer
rise, 30 % recirculation, COP = 7.5 − 0.12·T_outside, fans at 6 % of IT power, 90 KB per
pageview. Each node's share of the load `w_i` is a fixed, deterministic placement pattern
(hotter middle aisle), not measured per-server data. The chiller duty shown in the IPC panel
is computed by the model and is not sent to any hardware.
