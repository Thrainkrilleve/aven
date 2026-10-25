# Aven

**Declarative Invariant Programming & Relational Multi-Physics Engine**

[![Build & Test](https://img.shields.io/badge/tests-78%20passed-brightgreen.svg)](#testing--verification)
[![Zero Dependencies](https://img.shields.io/badge/dependencies-pure%20Rust%20std-blue.svg)](#zero-dependency-architecture)
[![Throughput](https://img.shields.io/badge/IPC%20Throughput-352.9%20solves%2Fsec-orange.svg)](#high-performance-persistent-ipc-daemon)
[![Median Latency](https://img.shields.io/badge/Median%20Latency-9.7%20ms-purple.svg)](#high-performance-persistent-ipc-daemon)
[![License: MIT](https://img.shields.io/badge/License-MIT-green.svg)](#license)

Aven is a domain-specific relational programming language and deterministic solver engine designed for physical systems, telemetry fusion, automated control, and multi-objective optimization. 

Unlike traditional imperative languages where functions require hardcoded input/output parameters, Aven relations represent **omnidirectional mathematical invariants**. Define your physical laws once, and Aven dynamically solves for any unknown variable via symbolic differentiation, analytical substitution, and Newton-Raphson numerical relaxation—or transpiles directly to branchless, solver-free ANSI C code.

---

## Key Capabilities

- **Omnidirectional Relational Solving:** Declare physical relations naturally without specifying solve directions. Write $F = m \cdot a$ once, and solve for force, mass, or acceleration automatically depending on which inputs are observed.
- **Pure-Rust Zero-Dependency Core:** The `avenc` compiler, lexer, parser, AST, evaluator, and fact ledger are written strictly in standard library Rust (`std`), requiring zero external crates.
- **Direction-Pinning & C Code Generation (`avenc emit-c`):** Transpile declarative Aven blocks into standalone, branchless ANSI C code with no runtime solver overhead, suitable for bare-metal embedded targets and hard real-time control loops.
- **First-Class External Adapters & Persistent IPC Daemons:** Connect relations to real-world sensors, telemetry streams, and black-box simulators via standard I/O subshells (`using external(...)`) or high-throughput persistent TCP/IPC daemons (`using ipc(...)`) executing in $< 3\,\text{ms}$.
- **Tamper-Evident Fact Ledgers:** Every settled fact extends a SHA-256 hash chain (optionally HMAC-signed with `AVEN_LEDGER_KEY`); facts from adapters record the SHA-256 of the adapter's raw output. `avenc run --ledger out.jsonl` writes the audit log and `avenc verify-ledger out.jsonl` detects any edit, deletion, reordering or truncation. Time-to-live (`ttl_ms`) freshness tracking blocks decisions on stale data (`DECISION BLOCKED`, exit code 3).
- **Multi-Objective Pareto Optimization:** Explore multi-dimensional tradeoff frontiers across competing constraints (e.g., latency vs. compute capacity vs. cost).

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
│  - Symbolic differentiation & analytical backward solving   │       │  - Topological sort of dependency graph                      │
│  - Guarded Newton-Raphson relaxation                         │       │  - Zero-solver, deterministic forward evaluation              │
│  - Inequality bounds enforcement (min / max / domain)        │       │  - Emits clean, dependency-free ANSI C code                  │
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
    // Mathematical invariants solved over persistent memory-mapped stream
    ...
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

Aven has been validated against massive live multi-stream real-world datasets:

### Live Multi-Stream Flight Dynamics Mesh
Fusing real-time commercial transponder data from **OpenSky Network** (4,700+ active aircraft), surface aviation weather from **NOAA Aviation Weather Center** (AWC METARs), and instantaneous hydrology from **USGS Water Services**:

| Benchmark Pipeline | Median Settle Latency | Multi-Worker Throughput | Improvement |
| :--- | :--- | :--- | :--- |
| **Subprocess Mode (`using external(...)`)** | `125.1 ms` | $7.4\,\text{solves/sec}$ | Baseline |
| **Lazy-Import Subprocess** | `71.5 ms` | $54.1\,\text{solves/sec}$ | $1.7\times$ faster |
| **Persistent IPC Daemon (`using ipc(...)`)** | **`9.7 ms`** | **`352.9 solves/sec`** | **$12.9\times$ faster / $47\times$ throughput** |

- **In-Memory Physics Resolution:** $144.3\,\mu\text{s}$ compute time inside daemon.
- **Round-Trip Loopback Settle:** $2.95\,\text{ms}$ total connection and solve duration.
- **Standard Atmosphere Verified:** Temperature, pressure and Mach number match an independent ISA computation for all 150 live aircraft checked, including 30 cruising above the 11 km tropopause (up to $43{,}000\,\text{ft}$); see `verify/live_atmosphere.py`. (An earlier model extended the tropospheric lapse rate above the tropopause and reported spurious $-80^\circ\text{C}$ bound violations at cruise altitude; that model error is fixed.)

### 8-Hour Overnight Weather Operations Monitor
Continuous live validation across 5 major US airport hubs (Denver KDEN, Dallas KDFW, New York KJFK, Chicago KORD, Seattle KSEA):
- **486 Evaluation Cycles** completed over 8 continuous hours.
- **Actionable Free-Cooling Solves:** $410 / 486$ ($84.4\%$).
- **Stale Block Enforcements:** 76 cycles correctly held on expired sensor TTLs (`DECISION BLOCKED`).

---

## CLI Toolchain Reference (`avenc`)

Build the standalone compiler binary:
```bash
cargo build --release
```

### Execute a Model
```bash
./target/release/avenc run evaluations/flight_mesh/flight_mesh_ipc.aven
```
Output:
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

### Check Model Invariants & Constraints
```bash
./target/release/avenc check examples/server_capacity.aven
```

### Transpile to ANSI C Code
```bash
./target/release/avenc emit-c examples/server_capacity.aven > server_capacity.c
```

### Multi-Objective Pareto Optimization
```bash
./target/release/avenc pareto examples/server_capacity.aven reqs_per_second 100 50000
```

---

## Repository Structure

```
f:\aven/
├── Cargo.toml                              # Root package manifest (pure Rust std)
├── Aven Development Roadmap.md             # Architecture roadmap & milestones
├── src/                                    # Core compiler & relational engine
│   ├── ast.rs                              # AST definitions & BackendDecl variants
│   ├── lexer.rs                            # Tokenizer with line/col tracking
│   ├── parser.rs                           # Recursive descent parser
│   ├── evaluator.rs                        # Solver engine, SHA-256 hasher & IPC evaluator
│   ├── codegen_c.rs                        # ANSI C code emitter
│   ├── optimize.rs                         # Simplex, nonlinear and interval branch-and-bound optimizer
│   ├── ledger.rs                           # Hash-chained, signable fact ledger
│   └── main.rs                             # CLI entrypoint & Pareto orchestrator
├── tests/                                  # Integration and documentation-claim tests (78)
│   └── test_suite.rs                       # Comprehensive verification suite
├── examples/                               # Canonical domain specifications
│   ├── aerodynamics.aven                   # Compressible flight dynamics
│   ├── fleet.aven                          # Multi-agent vehicle constraints
│   ├── flame.aven                          # Combustion dynamics
│   ├── openplc.aven                        # Industrial automation & ladder logic
│   └── server_capacity.aven                # Datacenter capacity planning
├── evaluations/                            # Physical benchmarks & telemetry fusion
│   ├── flight_mesh/                        # OpenSky + NOAA + USGS flight mesh
│   │   ├── flight_mesh_daemon.py           # Sub-millisecond persistent telemetry daemon
│   │   ├── flight_mesh_ipc.aven            # High-performance IPC model
│   │   ├── flight_mesh_adapter.py          # Standalone subprocess adapter
│   │   └── stream_ringer.py                # Parallel multi-worker stress harness
│   ├── weather/                            # 8-hour live NWS NOAA audit monitor
│   ├── reports/                            # Technical reviews & architectural findings
│   └── traces/                             # Wikimedia production trace replay
├── benchmarks/                             # Performance benchmarks & RHEL harnesses
├── app/                                    # Desktop orchestrator UI (Tauri v2 + frontend)
├── k8s/                                    # Kubernetes controllers, CRDs & manifests
├── scripts/                                # CI and validation scripts
└── archive/                                # Archived development prototypes
```

---

## Testing & Verification

Aven includes a comprehensive test suite verifying forward evaluation, backward solving through non-linear functions (`sqrt`, `ceil`, `floor`, exponents), Newton-Raphson relaxation, C code generation, fact ledger hashing, and IPC networking:

```bash
cargo test
```
```text
test result: ok. 29 passed; 0 failed   (tests/claims.rs: every documented claim)
test result: ok. 49 passed; 0 failed   (tests/test_suite.rs)
```

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

This project is licensed under the MIT License — see the repository for details.
