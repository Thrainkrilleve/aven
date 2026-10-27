# Flight-mesh runs, 2026-10-27 (Linux)

Machine: 4-vCPU Linux container, Intel Xeon @ 2.10 GHz. `avenc` 0.6.0 built from this
commit. Telemetry daemon `evaluations/flight_mesh/flight_mesh_daemon.py` on live OpenSky,
NOAA AWC and USGS feeds (first refresh: 4,513 cruising flights, 10 METAR hubs).

| File | Command | Result |
| :--- | :--- | :--- |
| `subprocess_report.md`, `subprocess_log.jsonl` | `stream_ringer.py --count 50 --concurrency 4 --model evaluations/flight_mesh/flight_mesh.aven` | 50/50 solved, 0 bound violations, median 70.8 ms, 51.4 solves/s |
| `ipc_report.md`, `ipc_log.jsonl` | same, `--model evaluations/flight_mesh/flight_mesh_ipc.aven` | 50/50 solved, 0 bound violations, median 6.2 ms, 570.9 solves/s |
| `live_atmosphere.txt` | `verify/live_atmosphere.py --flights 150` | see last line |
| `avenc_bench.txt` | `avenc bench evaluations/flight_mesh/flight_mesh_ipc.aven --iterations 2000` | see last lines |

The live feeds change from minute to minute, so re-runs give different aircraft and slightly
different timings.
