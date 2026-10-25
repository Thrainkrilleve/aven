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

Sample output:
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
- `--json`: print `{"status", "optimality", "env"}` with full-precision values instead of the table.
- `--ledger out.jsonl`: write the hash-chained audit log (signed when `AVEN_LEDGER_KEY` is set).
- `--allow-stale`: settle even when an input's TTL has expired (non-production use).

### 2.2 `avenc check <file.aven>`
Performs static analysis without solving:
- Validates syntax, token spans, and equation structure.
- Verifies that external adapters declare valid sibling schemas (`*.schema.json`).
- Checks equation solvability and warns on unbound cycles.

```bash
avenc check examples/server_capacity.aven
```

### 2.3 `avenc emit-c <file.aven>`
Transpiles direction-pinned relations and forward blocks into lean, branchless, zero-overhead ANSI C code:

```bash
avenc emit-c examples/server_capacity.aven > server_capacity.c
```

Emitted C code includes:
- Topological forward dependency evaluation.
- Inclusion of `<math.h>` for `sqrt`, `ceil`, `floor`, `pow`.
- Zero runtime solver overhead (suitable for bare-metal microcontrollers and automotive ECUs).

### 2.4 `avenc pareto <file.aven> [var] [min] [max]`
Explores multi-objective trade-off frontiers across competing objectives:

```bash
avenc pareto examples/server_capacity.aven reqs_per_second 100 50000
```

Flags:
- `--backend native|external`: Selects solver backend.
- `--cmd <command>`: Specifies command for external Pareto sweeps.

### 2.5 `avenc bench <file.aven>`
Runs microsecond-precision benchmarks across relation solving, parsing, and IPC evaluations:

```bash
avenc bench evaluations/flight_mesh/flight_mesh_ipc.aven
```

---

### 2.6 `avenc verify-ledger <out.jsonl>`
Recomputes an audit log's hash chain, seal and (with `AVEN_LEDGER_KEY`) signatures. Exit 0 when intact; exit 1 naming the first tampered line.

## 3. Environment Variables

| Variable | Description | Default |
| :--- | :--- | :--- |
| `AVEN_WORKSPACE` | Canonical root directory for resolving sibling schemas and scripts. | Current working directory |
| `AVEN_SNAPSHOT_FILE` | Path to pre-warmed telemetry snapshot JSON for zero-lock solves. | Unset |
| `AVEN_FLIGHT_INDEX` | Target aircraft index when querying multi-stream flight meshes. | `0` |
| `AVEN_FLIGHT_CALLSIGN` | Target flight callsign filter (e.g. `UAL605`). | Unset |
| `AVEN_LEDGER_KEY` | Key for HMAC-SHA256 signing of ledgers and for `verify-ledger` signature checks. | Unset (unsigned) |
| `AVENC_BIN` | Explicit path to compiled `avenc` binary for testing harnesses. | Auto-discovered |

---

## 4. Exit Codes & Status Handling

- `0`: Model settled successfully; all equations and constraints satisfied.
- `1`: Semantic, syntax, or black-box evaluation error (e.g. connection timeout or malformed JSON).
- `2`: Constraint violation (an observed variable exceeded declared safety bounds).
- `3`: Decision blocked (stale TTL detected on an upstream sensor input).
