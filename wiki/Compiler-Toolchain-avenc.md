# Compiler Toolchain (avenc)

The `avenc` compiler is the standalone command-line driver for the Aven relational language. Built in pure standard library Rust, it requires no external runtime dependencies.

---

## 1. Building the Toolchain

From the root of the repository:

```bash
# Debug build
cargo build

# Optimized release binary
cargo build --release
```

The compiled executable is placed at `target/release/avenc` (or `avenc.exe` on Windows).

---

## 2. Command-Line Reference

### 2.1 `avenc run <file.aven>`
Parses, type-checks, evaluates external/IPC backends, and solves all relational invariants. Outputs the settled environment variables and cryptographic fact ledger:

```bash
avenc run evaluations/flight_mesh/flight_mesh_ipc.aven
```

Sample output (needs `evaluations/flight_mesh/flight_mesh_daemon.py` running and live feeds):
```text
Connecting to persistent IPC evaluator at '127.0.0.1:9090' for relation 'FlightAtmosphericDynamics'...
  [ipc] FlightAtmosphericDynamics settled in 2950.5 µs (backend: ipc_UAL605_KORD_MissBasin)

Settled Environment:
  air_density_kg_m3 = 0.3311
  ambient_pressure_pa = 20207.4864
  ambient_temp_c   = -60.5668
  baro_altitude_m  = 11887.2000
  dynamic_pressure_pa = 9791.2787
  mach_number      = 0.8320
  speed_of_sound_ms = 292.2896
  terrain_clearance_m = 11683.2000
  velocity_ms      = 243.1800

Fact Ledger: 25 facts settled.
```

Options:
- `--json`: print `{"status", "optimality", "env", "units"}` with full-precision base-unit values instead of the table.
- `--ledger out.jsonl`: write the hash-chained audit log (signed when `AVEN_LEDGER_KEY` is set).
- `--allow-stale`: settle even when an input's TTL has expired (non-production use).

### 2.2 `avenc check <file.aven>`
Performs static analysis without solving:
- Validates syntax and equation structure.
- Infers and checks the dimension of every variable, printing each one.
- Reports under-determined relations (free variables and the bindings that would fix them).

Adapter output is validated against a sibling `*.schema.json` when the adapter runs (`avenc run`), not by `check`.

```bash
avenc check examples/server_capacity.aven
```

### 2.3 `avenc emit-c <file.aven>`
Transpiles direction-pinned relations and forward blocks into branchless, solver-free C99:

```bash
avenc emit-c examples/server_capacity.aven > server_capacity.c
```

Emitted C code includes:
- Topological forward dependency evaluation of the model's first `forward` block.
- `<math.h>` for `sqrt`, `ceil`, `floor`, `pow` and friends, and a demo `main` (omit with `--no-main`).
- No runtime solver: straight-line code suited to microcontrollers and fixed-rate control loops. Values are in base units.

### 2.4 `avenc pareto <file.aven> <var> <min> <max>`
Sweeps `<var>` from `<min>` to `<max>` (in its declared unit), tabulates every output at each step, and marks the knee point of the trade-off between the two most strongly conflicting outputs:

```bash
avenc pareto examples/server_capacity.aven reqs_per_second 100 50000
```

Flags:
- `--steps N`: Number of sweep intervals (N + 1 points; default 5).
- `--backend native|external`: Selects solver backend.
- `--cmd <command>`: Specifies command for external Pareto sweeps.

### 2.5 `avenc bench <file.aven>`
Times parsing and solving (including external/IPC round trips) over many iterations and prints min, median and p95. `--latency-profile` and `--target-hz` measure single-tick latency and jitter; `--json` / `--markdown` change the format:

```bash
avenc bench evaluations/flight_mesh/flight_mesh_ipc.aven
```

---

### 2.6 `avenc stream <file.aven>`
Reads one JSON object per line from stdin and solves the model once per line, with the line's fields added to the solve block's facts. Each cycle prints `SETTLED` with the derived values, `ALARM` naming the broken bound, or `UNSOLVED` with the reason (a reading that leaves the model under-determined is not filled in). The run ends with a count of each and exits 0 when stdin closes. See [Supervising a live feed](https://github.com/Thrainkrilleve/aven/blob/main/docs/TUTORIAL.md#supervising-a-live-feed-with-avenc-stream).

```bash
python evaluations/tests/PLC-test/openplc_poller.py --host <plc-address> | avenc stream examples/stream/openplc_supervisor.aven
```

---

### 2.7 `avenc verify-ledger <out.jsonl>`
Recomputes an audit log's hash chain, seal and (with `AVEN_LEDGER_KEY`) signatures. Exit 0 when intact; exit 1 naming the first tampered line.

## 3. Environment Variables

| Variable | Read by | Description | Default |
| :--- | :--- | :--- | :--- |
| `AVEN_LEDGER_KEY` | `avenc` | Key for HMAC-SHA256 signing of ledgers and for `verify-ledger` signature checks. | Unset (unsigned) |
| `AVEN_WORKSPACE` | `avenc`, harnesses | Root directory for resolving adapter scripts and sibling schemas. | Current working directory |
| `AVEN_FLIGHT_INDEX` | `avenc`, flight-mesh adapter | Aircraft index to query in the flight-mesh benchmark. | `0` |
| `AVEN_FLIGHT_CALLSIGN` | `avenc`, flight-mesh adapter | Callsign filter for the flight-mesh benchmark (e.g. `UAL605`). | Unset |
| `AVEN_SNAPSHOT_FILE` | flight-mesh harnesses | Pre-warmed telemetry snapshot JSON. | Unset |
| `AVENC_BIN` | Python harnesses | Path to the `avenc` binary under test. | Auto-discovered |

---

## 4. Exit Codes & Status Handling

- `0`: Model settled successfully; all equations and constraints satisfied.
- `1`: Semantic, syntax, or black-box evaluation error (e.g. connection timeout or malformed JSON).
- `2`: Constraint violation (an observed variable exceeded declared safety bounds).
- `3`: Decision blocked (stale TTL detected on an upstream sensor input).
