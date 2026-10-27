# Aven

**Declarative Invariant Programming & Relational Multi-Physics Engine**

[![CI](https://github.com/Thrainkrilleve/aven/actions/workflows/ci.yml/badge.svg)](https://github.com/Thrainkrilleve/aven/actions/workflows/ci.yml)
[![Zero Dependencies](https://img.shields.io/badge/dependencies-pure%20Rust%20std-blue.svg)](#key-capabilities)
[![License: MIT](https://img.shields.io/badge/License-MIT-green.svg)](#license)

Aven is a domain-specific relational programming language and deterministic solver engine designed for physical systems, telemetry fusion, automated control, and optimization.

Unlike traditional imperative languages where functions require hardcoded input/output parameters, Aven relations represent **omnidirectional mathematical invariants**. Define your physical laws once, and Aven solves for any unknown variable with Newton–Raphson using exact symbolic derivatives—or transpiles `forward` blocks directly to branchless, solver-free C99.

---

## Documentation & Learning Path

| Guide | Description | Target Audience |
|:---|:---|:---|
| **[Getting Started Guide](docs/GETTING_STARTED.md)** | 15-minute quickstart from installation to first model solve | First-time visitors |
| **[Aven by Example (Cookbook)](docs/TUTORIAL.md)** | 10 progressive, executable code recipes covering units, C99/WASM codegen, and debugging | Hands-on learners |
| **[Quick Reference Cheat Sheet](docs/CHEAT_SHEET.md)** | Syntax tables, built-in functions, units, CLI commands, and exit codes | Daily reference |
| **[Engineering Standards](STANDARDS.md)** | Mathematical stability, IEEE-754 semantics, cryptographic ledger, and safety rules | Systems & compliance engineers |
| **[Package Specification](docs/PACKAGE_SPEC.md)** | `Aven.toml` schema and `.avenpkg` SHA-256 sealed distribution format | Package authors |
| **[Contributing Guide](CONTRIBUTING.md)** | Architectural ground rules ("Rust compiles, C runs") and PR guidelines | Contributors |
| **[RFC Process](RFC_PROCESS.md)** | Language evolution lifecycle and decision framework | Protocol & compiler designers |

---

## Key Capabilities

- **Omnidirectional Relational Solving:** Declare physical relations naturally without specifying solve directions. Write $F = m \cdot a$ once, and solve for force, mass, or acceleration automatically depending on which inputs are observed.
- **Pure-Rust Zero-Dependency Core:** The `avenc` compiler, lexer, parser, AST, evaluator, and fact ledger are written strictly in standard library Rust (`std`), requiring zero external crates.
- **Checked and converted units:** `avenc check` infers the dimension of every variable and rejects equations that mix incompatible ones. Values are converted to coherent base units before solving, so `velocity = 250 km/h` is solved as 69.44 m/s and shown back in km/h. Ambiguous spellings such as `Mb` (megabit or megabyte?) are refused rather than guessed.
- **Shared Memory Rings & Native In-Process FFI (`using shm(...)`, `using native(...)`):** Memory-mapped ring buffers and in-process loading of C/Rust dynamic libraries (`.so` / `.dll`), with no subprocess or socket per solve. `tests/phase4_shm_ffi.rs` times one call of each: shared-memory reads take 5–10 µs and native library calls 40–50 µs on a 4-vCPU Linux container (timings vary by machine).
- **Direction-Pinning & C Code Generation (`avenc emit-c`, `--aeric-shim`):** Transpile `forward` blocks into standalone, branchless C99 with no runtime solver: straight-line code suited to embedded targets and fixed-rate control loops. `--aeric-shim --out-dir <dir>` emits an AERIC edge deployment bundle with a SHA-256 model-identity hash and non-aborting domain finiteness guards.
- **External Adapters & Persistent IPC Daemons:** Connect relations to sensors, telemetry streams and black-box simulators via subprocess adapters (`using external(...)`) or persistent TCP daemons (`using ipc(...)`). Adapters exchange values in each variable's declared unit.
- **Tamper-Evident Fact Ledgers:** Every settled fact extends a SHA-256 hash chain (optionally HMAC-signed with `AVEN_LEDGER_KEY`); facts from adapters record the SHA-256 of the adapter's raw output. `avenc run --ledger out.jsonl` writes the audit log and `avenc verify-ledger out.jsonl` detects any edit, deletion, reordering or truncation. Time-to-live (`ttl_ms`) freshness tracking blocks decisions on stale data (`DECISION BLOCKED`, exit code 3).
- **Optimization:** `minimize` / `maximize` over real and integer variables with simplex branch-and-bound; a result is labelled *proven optimal* only when its bound certifies it. `avenc pareto` sweeps one variable across a range and marks the knee point of the resulting trade-off curve.

---

## System Architecture

```
                       ┌────────────────────────────────────────────────────────┐
                       │                     Aven Source                        │
                       │                (*.aven specification)                  │
                       └───────────────────────────┬────────────────────────────┘
                                                   │
                                                   ▼
                       ┌────────────────────────────────────────────────────────┐
                       │                     avenc Compiler                     │
                       │  ┌───────────────┐  ┌──────────────┐  ┌─────────────┐  │
                       │  │ Pure Std Lexer│─►│ AST & Parser │─►│  VarMap &   │  │
                       │  │ & Tokenizer   │  │  Validation  │  │ Constraints │  │
                       │  └───────────────┘  └──────────────┘  └──────┬──────┘  │
                       └──────────────────────────────────────────────┼─────────┘
                                                                      │
                                   ┌──────────────────────────────────┴──────────────────────────────────┐
                                   ▼                                                                     ▼
┌──────────────────────────────────────────────────────────────┐       ┌──────────────────────────────────────────────────────────────┐
│                  Relational Solver Engine                    │       │                 Direction-Pinned C Transpiler                │
│  - Symbolic differentiation & Newton backward solving       │       │  - Topological sort of dependency graph                      │
│  - Guarded Newton-Raphson relaxation                         │       │  - Zero-solver, deterministic forward evaluation              │
│  - Inequality bounds enforcement (min / max / domain)        │       │  - Emits clean, dependency-free C99 code                     │
└──────────────────────────────┬───────────────────────────────┘       └──────────────────────────────┬───────────────────────────────┘
                               │                                                                      │
                               ▼                                                                      ▼
┌──────────────────────────────────────────────────────────────┐       ┌──────────────────────────────────────────────────────────────┐
│                  Telemetry & Invariant Fusion                │       │                  Embedded / Bare-Metal Binary                │
│  - Subprocess Adapters: using external("python", "sim.py")   │       │  - Microcontrollers, RTOS kernels, PLC runtimes             │
│  - Persistent IPC Daemons: using ipc("127.0.0.1:9090")       │       │  - Microsecond deterministic execution                       │
└──────────────────────────────┬───────────────────────────────┘       └──────────────────────────────────────────────────────────────┘
                               │
                               ▼
┌──────────────────────────────────────────────────────────────┐
│             Cryptographic Fact Ledger & Provenance           │
│  - Immutable append-only audit trail                         │
│  - SHA-256 hash chain, HMAC signing, payload hashes          │
│  - Freshness (TTL) enforcement & stale state protection      │
└──────────────────────────────────────────────────────────────┘
```

---

## Language Tour & Syntax

### 1. Omnidirectional Physical Relations
A relation describes mathematical invariants that must hold true. Any unknown can be solved given sufficient inputs:

```aven
// flight_mesh.aven — Atmospheric lapse & flight dynamics
relation FlightAtmosphericDynamics {
    // 1. ISA atmosphere: troposphere lapse capped at the 11 km tropopause, isothermal above
    surface_temp_k = surface_temp_c + 273.15
    lapse_rate = 0.0065
    tropopause_m = 11000.0
    troposphere_m = (baro_altitude_m + tropopause_m - abs(baro_altitude_m - tropopause_m)) / 2.0
    above_tropopause_m = baro_altitude_m - troposphere_m
    ambient_temp_k = surface_temp_k - (lapse_rate * troposphere_m)
    ambient_temp_c = ambient_temp_k - 273.15

    // 2. Barometric Pressure & Air Density
    pressure_ratio = 1.0 - ((lapse_rate * troposphere_m) / surface_temp_k)
    ambient_pressure_pa = (surface_altim_hpa * 100.0) * (pressure_ratio ^ 5.2561) * exp(-(9.80665 / 287.058) * above_tropopause_m / ambient_temp_k)
    air_density_kg_m3 = ambient_pressure_pa / (287.058 * ambient_temp_k)

    // 3. High-Speed Aerodynamics & Compressibility
    safe_temp_k = ambient_temp_k + 0.0001
    speed_of_sound_ms = sqrt(1.4 * 287.058 * safe_temp_k)
    mach_number = velocity_ms / (speed_of_sound_ms + 0.0001)
    dynamic_pressure_pa = 0.5 * air_density_kg_m3 * (velocity_ms ^ 2)

    // 4. Specific Excess Climb Power & Terrain Clearance
    specific_excess_power_w_kg = 9.80665 * vertical_rate_ms
    terrain_clearance_m = baro_altitude_m - gauge_elevation_m
}

solve FlightAtmosphericDynamics with {
    velocity_ms >= 0.0
    velocity_ms <= 400.0
    mach_number >= 0.0
    mach_number <= 1.2
    ambient_temp_c >= -80.0
    ambient_temp_c <= 60.0
    terrain_clearance_m >= -100.0
}
```

### 2. High-Performance Persistent IPC Daemon
For real-time control and high-frequency streams, declare relations backed by persistent socket services:

```aven
// Connect directly to a persistent telemetry daemon via local TCP socket
relation FlightAtmosphericDynamics using ipc("127.0.0.1:9090") {
    // The daemon supplies facts over TCP; these equations are solved over them
    speed_of_sound_ms = sqrt(1.4 * 287.058 * ambient_temp_k)
    mach_number = velocity_ms / speed_of_sound_ms
}
```

### 3. Direction-Pinned Forward Blocks
When all inputs are known and execution must be deterministic without iterative relaxation, use `forward`:

```aven
forward ServerSizing {
    raw_workers = (reqs_per_second * avg_latency_ms) / 1000.0
    allocated_workers = ceil(raw_workers / target_utilization)
    total_ram_mb = allocated_workers * (ram_per_worker + socket_buffer_mb) + os_kernel_ram_mb
}
```

---

## Multi-Physics Telemetry Benchmarks

These runs used live public feeds, so they cannot be reproduced exactly; the harnesses and their logs are in `evaluations/`. Timings depend on the machine.

### Live Multi-Stream Flight Dynamics Mesh
Fusing transponder data from the **OpenSky Network**, surface weather from the **NOAA Aviation Weather Center** (METARs), and hydrology from **USGS Water Services**:

| Benchmark Pipeline | Median Settle Latency | Throughput | Notes |
| :--- | :--- | :--- | :--- |
| **Subprocess Mode (`using external(...)`)** | `125.1 ms` | 7.4 solves/sec | `stream_ringer.py`, multi-worker |
| **Lazy-Import Subprocess** | `71.5 ms` | 54.1 solves/sec | `stream_ringer.py`, multi-worker |
| **Persistent IPC Daemon (`using ipc(...)`)** | `9.7 ms` | 352.9 solves/sec | `stream_ringer.py`, multi-worker |
| **`avenc bench`, live IPC daemon (one client)** | 1414.9 µs median (p95 2120.8 µs) | 664.3 solves/sec | `evaluations/flight_mesh/runs/2026-10-27-linux/avenc_bench.txt`, 4-vCPU Linux container, 2026-10-27 |
| **Subprocess, `stream_ringer.py`, 50 live flights** | `70.8 ms` | 51.4 solves/sec | Same container and date |
| **IPC daemon, `stream_ringer.py`, 50 live flights** | `6.2 ms` | 570.9 solves/sec | Same container and date |

The `stream_ringer.py` rows include the harness's own process and worker overhead. The `avenc bench` row is the complete solve (TCP round trip to the daemon plus the relational solve of `flight_mesh_ipc.aven`), measured with `avenc bench evaluations/flight_mesh/flight_mesh_ipc.aven --iterations 2000` against `evaluations/flight_mesh/flight_mesh_daemon.py`. The numbers are the ones in `evaluations/flight_mesh/runs/2026-10-27-linux/avenc_bench.txt`. A Windows workstation run from 2026-10-25 has no log in this repository, so it is not stated here.

- **Standard Atmosphere Verified:** Temperature, pressure and Mach number match an independent ISA computation for all 150 live aircraft checked, including 30 cruising above the 11 km tropopause (up to 43,000 ft); see `verify/live_atmosphere.py`. Re-run on 2026-10-27: 150 aircraft, 32 above the tropopause (up to 45,000 ft), 0 mismatches (`evaluations/flight_mesh/runs/2026-10-27-linux/live_atmosphere.txt`). (An earlier model extended the tropospheric lapse rate above the tropopause and reported spurious −80 °C bound violations at cruise altitude; that model error is fixed. `evaluations/flight_mesh/flight_mesh_audit_ledger.jsonl` is a run log from before the fix.)

### 8-Hour Overnight Weather Operations Monitor
`evaluations/weather/overnight_monitor.py` polled the National Weather Service for 5 airports (KDEN, KDFW, KJFK, KORD, KSEA) on 2026-10-25. Its per-cycle log, `evaluations/weather/weather_audit_ledger.jsonl`, records:
- **486 evaluation cycles** over 8.0 hours.
- **410 actionable solves** (84.4%).
- **76 cycles held** on expired sensor TTLs (`DECISION BLOCKED`).

Both `*_audit_ledger.jsonl` files are the harnesses' per-cycle logs, not `avenc` hash-chained ledgers, so `avenc verify-ledger` does not apply to them.

### Live Radar Nowcast ([avenc.dev/weather](https://avenc.dev/weather))
Every five minutes the website's scheduled job picks the strongest storm cells tracked by the NEXRAD radar network, gathers data for each from NOAA (nowCOAST radar mosaic, METAR, winds aloft), the NWS (observations, alerts, hourly forecast) and USGS (rain gauges), and runs [`examples/weather_nowcast.aven`](examples/weather_nowcast.aven) compiled to WebAssembly. Its forecasts of radar reflectivity 15, 30 and 60 minutes ahead (along the steering wind, along the radar-measured storm motion, and persistence as the baseline) are scored against the archived radar image nearest each valid time, and the page shows every input, request, cross-check and score. Any stored run can be re-run in the browser from the same module. `verify/weather_reference.py` checks the model against its relational form, MetPy and NumPy (150 random states, 0 failures). The job and page are in `web/src/weather/` and `web/public/weather.*`; `web/scripts/weather-local.mjs` runs them locally.

---

## CLI Toolchain Reference (`avenc`)

Build the standalone compiler binary:
```bash
cargo build --release
```

### Execute a Model
```bash
./target/release/avenc run examples/aerodynamics.aven
```
Output:
```text
Solver Trace:
  [dynamic_pressure = 0.5 * air_density * velocity ^ 2] dynamic_pressure <- forward eval
  [lift = dynamic_pressure * wing_area * lift_coeff] lift <- forward eval

Settled Environment:
  air_density      = 1.2250 kg/m^3
  dynamic_pressure = 2953.8002 Pa
  lift             = 51986.8827
  lift_coeff       = 0.5500
  velocity         = 250.0000 km/h
  wing_area        = 32.0000 m^2

Fact Ledger: 6 facts settled.
```
Values print in their declared unit, or in the base unit of their inferred dimension. `lift` has no label because `lift_coeff` was given no unit, so the dimension of `lift` is unknown. `--json` prints base-unit values with a `units` map.

### Check Model Invariants & Constraints
```bash
./target/release/avenc check examples/server_capacity.aven
```

### Transpile to C99
```bash
./target/release/avenc emit-c examples/server_capacity.aven -o server_capacity.c
```

### Trade-off Sweep
```bash
./target/release/avenc pareto examples/server_capacity.aven reqs_per_second 100 50000
```

---

## Repository Structure

```
aven/
├── Cargo.toml                              # Root package manifest (no dependencies)
├── src/                                    # Compiler and relational engine
│   ├── lexer.rs, parser.rs, ast.rs         # Front end
│   ├── units.rs                            # Unit registry, dimensional checking, unit conversion
│   ├── solver.rs, diff.rs, optimize.rs     # Newton solving, symbolic derivatives, simplex / branch-and-bound
│   ├── evaluator.rs                        # External, IPC, shared-memory and FFI backends
│   ├── codegen_c.rs, codegen_wasm.rs       # C and WebAssembly emitters
│   ├── smt.rs                              # SMT-LIB2 export, solver runs, safety-case reports
│   ├── ledger.rs, compaction.rs            # Hash-chained, signable fact ledger
│   ├── debugger.rs, temporal.rs            # Time-travel debugger, signal streams
│   ├── package.rs                          # Aven.toml and .avenpkg packaging
│   └── main.rs                             # CLI entry point
├── tests/                                  # Integration tests, incl. claims.rs (documented claims)
├── examples/                               # Example models (aerodynamics, server capacity, ...)
├── evaluations/                            # Live-data harnesses, logs and reports
├── verify/                                 # Checks against independent references
├── benchmarks/                             # Performance benchmarks & RHEL harnesses
├── web/                                    # avenc.dev site and live solver demo (WASM)
├── aeric/                                  # AERIC C runtime for edge deployment
├── app/                                    # Desktop orchestrator UI (Tauri v2)
├── k8s/                                    # Kubernetes controller and manifests
├── editors/vscode/                         # VS Code syntax highlighting
└── scripts/                                # CI and documentation checks
```

---

## Testing & Verification

The test suite covers forward evaluation, backward solving through non-linear functions, Newton–Raphson relaxation, units, optimization, C and WebAssembly code generation, ledger hashing, IPC, SMT export and safety cases:

```bash
cargo test
```

The SMT and safety-case tests need `z3` (or `cvc5`) on `PATH`; CI installs it. `tests/claims.rs` turns statements made in this README and the wiki into tests, and `scripts/check_docs.py` compiles every Aven example in the documentation and the website.

---

### Verifying the claims

`verify/` checks Aven against independent references (install `scipy` and `highspy` in a Python environment first):

| Harness | What it proves |
| :--- | :--- |
| `verify/oracle.py all` | Answers match HiGHS and SciPy on random integer, linear, infeasible/unbounded, nonconvex-integer and smooth models; optimization results are reported `proven` only when certified |
| `verify/c_diff.py` | `avenc emit-c` output compiles warning-free, is branchless, and matches `avenc run` exactly |
| `verify/fuzz.py` | Malformed and adversarial input never crashes, panics or hangs |
| `verify/ledger_tamper.py` | Every tampering technique on an audit log is detected |
| `verify/live_atmosphere.py` | The flight model matches the standard atmosphere on live aircraft |

---

## License

Aven is released under the [MIT License](LICENSE). Copyright © 2026 Austin Thompson.
