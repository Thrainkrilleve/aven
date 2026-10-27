# External Evaluators and Persistent IPC Daemons

Aven provides two primary mechanisms to connect declarative relational models to the outside physical world:
1. **Subprocess External Evaluators** (`using external(...)`): Isolated standard I/O subshells.
2. **Persistent IPC Daemons** (`using ipc(...)`): High-throughput, memory-speed TCP socket streaming.

---

## 1. Subprocess External Evaluators (`using external(...)`)

### 1.1 Declaration Syntax
```aven
relation AtmosphericDynamics using external("python", "evaluations/weather/nws_adapter.py") {
    dewpoint_spread_c = ambient_temp_c - dewpoint_temp_c
}
```

### 1.2 Execution Protocol
1. When `avenc` settles the block, it spawns the specified command (`python evaluations/weather/nws_adapter.py`).
2. Current known variables are serialized to standard input as a JSON object of numbers, each in its declared unit:
   ```json
   {"station_index": 0}
   ```
3. The adapter executes its telemetry fetch and writes a JSON object to standard output:
   ```json
   {
     "ambient_temp_c": 12.0,
     "dewpoint_temp_c": 4.5,
     "surface_pressure_hpa": 1024.1,
     "timestamp_ms": 1792942000000,
     "ttl_ms": 120000,
     "backend_id": "nws_metar_kden"
   }
   ```
4. `avenc` validates the response against the sibling schema (`nws_adapter.schema.json`), records the SHA-256 of the raw output, and binds the values into the relational solver.
   - Only fields the model uses become variables: fields that appear in an equation, constraint or objective, or are declared with `signal name`. Other numeric fields (e.g. an adapter's own timing) are ignored and listed in verbose output. `timestamp_ms` / `ttl_ms` are freshness metadata, never variables.
   - A variable the block needs but the adapter omits is an error only if the block's equations cannot derive it from what the adapter returned.

### 1.3 Latency Characteristics
- Subprocess mode provides complete isolation, making it ideal for scheduled cron checks, CI runs, and batch operations.
- However, spawning a child Python interpreter on Windows introduces **$\approx 70 - 120\,\text{ms}$** of process creation and standard library import overhead.

---

## 2. Persistent IPC Daemons (`using ipc(...)`)

To push telemetry ingestion into the **sub-10ms regime** (and achieve hundreds of solves per second), Aven introduces persistent socket IPC.

### 2.1 Declaration Syntax
```aven
relation FlightAtmosphericDynamics using ipc("127.0.0.1:9090") {
    terrain_clearance_m = baro_altitude_m - gauge_elevation_m
}
```

### 2.2 Daemon Architecture ([`flight_mesh_daemon.py`](https://github.com/Thrainkrilleve/aven/blob/main/evaluations/flight_mesh/flight_mesh_daemon.py))
Instead of booting an interpreter on each solve:
1. **Permanent RAM Residency:** The daemon runs continuously in the background, keeping the latest OpenSky aircraft states, NOAA METARs and USGS river readings resident in memory.
2. **Asynchronous Background Ingestion:** Telemetry feeds are refreshed in a background thread every 30 seconds. Inbound client queries **never block** on external REST APIs.
3. **Microsecond Compute Speed:** Correlates flight coordinates, nearest weather stations, and basin proxies in **$130 - 150\,\mu\text{s}$**.
4. **Persistent Socket Stream:** Listens on `tcp://127.0.0.1:9090`. `avenc` opens a direct TCP loopback stream, exchanges JSON lines, and settles in **$< 3\,\text{ms}$**.

```
Client Solver (avenc)                       Persistent Daemon (Python/RAM)
       │                                                   │
       ├─── Connect tcp://127.0.0.1:9090 ─────────────────►│ (Pre-warmed in RAM)
       │                                                   │
       ├─── Send query: {"flight_index": 4} ──────────────►│ (In-memory lookup: 140 µs)
       │                                                   │
       │◄── Read payload: {"baro_altitude_m": ...} ────────┤
       │                                                   │
       ▼                                                   ▼
 Settle Invariants (< 3 ms)                         Serve next query
```

---

## 3. Empirical Performance Comparison

Windows workstation, earlier flight model. Only the IPC row has a report in the repository (`flight_mesh_summary_report.md`). A 2026-10-27 Linux re-run on 50 live flights measured 70.8 ms (subprocess) against 6.2 ms (IPC), with logs in `evaluations/flight_mesh/runs/2026-10-27-linux/`; see [[Multi-Stream Physics Benchmarks|Multi-Stream-Physics-Benchmarks]] §4:

| Metric | Subprocess (`using external`) | Persistent Daemon (`using ipc`) | Improvement |
| :--- | :--- | :--- | :--- |
| **Median Settle Latency** | `125.1 ms` | **`9.7 ms`** | **$12.9\times$ faster** |
| **Multi-Worker Throughput** | $7.4\,\text{solves/sec}$ | **`352.9 solves/sec`** | **$47.7\times$ higher throughput** |
| **Internal Compute Overhead**| ~97 ms (Python bootstrap) | **`144.3 µs`** (In-RAM resolution) | **$670\times$ faster** |

---

## 4. Adapter Output Fields

An adapter or daemon prints one JSON object. Every numeric field the model uses becomes a fact (fields it does not use are ignored). These fields are recognised specially, and all are optional:

| Field | Meaning |
| :--- | :--- |
| `timestamp_ms`, `ttl_ms` | Freshness: facts expire `ttl_ms` after `timestamp_ms` (`ttl_ms = 0` means already stale). |
| `backend_id` | Name shown in traces; defaults to the command. |
| `warnings` | Array of strings recorded with the facts. |
| `error` | A message that makes the evaluation fail. |

If a sibling `<adapter>.schema.json` exists, the output is validated against it first.
