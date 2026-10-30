# Aven

**Relational programming for physical systems**

[![CI](https://github.com/Thrainkrilleve/aven/actions/workflows/ci.yml/badge.svg)](https://github.com/Thrainkrilleve/aven/actions/workflows/ci.yml)
[![Zero Dependencies](https://img.shields.io/badge/dependencies-pure%20Rust%20std-blue.svg)](#key-capabilities)
[![License: MIT](https://img.shields.io/badge/License-MIT-green.svg)](#license)

Aven is a domain-specific relational programming language and deterministic solver engine designed for physical systems, telemetry fusion, automated control, and optimization.

Unlike traditional imperative languages where functions require hardcoded input/output parameters, Aven relations represent **omnidirectional mathematical invariants**. Define your physical laws once, and Aven solves for any unknown variable with Newton–Raphson using exact symbolic derivatives—or transpiles `forward` blocks directly to branchless, solver-free C99.

---

## Documentation & Learning Path

| Guide | Description | Target Audience |
|:---|:---|:---|
| **[How to write a model](docs/HOW_TO.md)** | Follow along: forward, backward, and the checklist that says a test file is right | Anyone writing a model |
| **[Getting Started Guide](docs/GETTING_STARTED.md)** | 15-minute quickstart from installation to first model solve | First-time visitors |
| **[Aven by Example (Cookbook)](docs/TUTORIAL.md)** | 10 progressive, executable code recipes covering units, C99/WASM codegen, and debugging | Hands-on learners |
| **[Quick Reference Cheat Sheet](docs/CHEAT_SHEET.md)** | Syntax tables, built-in functions, units, CLI commands, and exit codes | Daily reference |
| **[Engineering Standards](STANDARDS.md)** | Mathematical stability, IEEE-754 semantics, cryptographic ledger, and safety rules | Systems & compliance engineers |
| **[Package Specification](docs/PACKAGE_SPEC.md)** | `Aven.toml` schema and `.avenpkg` SHA-256 sealed distribution format | Package authors |
| **[Contributing Guide](CONTRIBUTING.md)** | Architectural ground rules ("Rust compiles, C runs") and PR guidelines | Contributors |
| **[RFC Process](RFC_PROCESS.md)** | Language evolution lifecycle and decision framework | Protocol & compiler designers |
| **[Product thesis](PRODUCT_THESIS.md)** | The desktop app: write a model, check it, solve it, keep the file | Anyone deciding what Aven is for |
| **[IDE plan](IDE_MVP_PLAN.md)** | How that app gets built, milestone by milestone | Contributors on the desktop shell |

---

## Key Capabilities

Aven is one small, auditable compiler (`avenc`) with a set of runtimes and integrations around it. The **core** is the language and solver you would reach for on their own; the **adjacent** pieces connect that core to live data, edge hardware, and other tools.

### Core — the `avenc` compiler

- **Omnidirectional Relational Solving:** Declare physical relations naturally without specifying solve directions. Write $F = m \cdot a$ once, and solve for force, mass, or acceleration automatically depending on which inputs are observed.
- **Pure-Rust Zero-Dependency Core:** The `avenc` compiler, lexer, parser, AST, evaluator, and fact ledger are written strictly in standard library Rust (`std`), requiring zero external crates.
- **Checked and converted units:** `avenc check` infers the dimension of every variable and rejects equations that mix incompatible ones. Values are converted to coherent base units before solving, so `velocity = 250 km/h` is solved as 69.44 m/s and shown back in km/h. Ambiguous spellings such as `Mb` (megabit or megabyte?) are refused rather than guessed.
- **Direction-Pinning & Code Generation (`avenc emit-c`, `avenc emit-wasm`):** Transpile `forward` blocks into standalone, branchless C99 or WebAssembly 1.0 with no runtime solver: straight-line code suited to embedded targets and fixed-rate control loops. Generated C is checked to match `avenc run` exactly.
- **Tamper-Evident Fact Ledgers:** Every settled fact extends a SHA-256 hash chain (optionally HMAC-signed with `AVEN_LEDGER_KEY`); facts from adapters record the SHA-256 of the adapter's raw output. `avenc run --ledger out.jsonl` writes the audit log and `avenc verify-ledger out.jsonl` detects any edit, deletion, reordering or truncation. Time-to-live (`ttl_ms`) freshness tracking blocks decisions on stale data (`DECISION BLOCKED`, exit code 3).
- **Optimization:** `minimize` / `maximize` over real and integer variables with simplex branch-and-bound; a result is labelled *proven optimal* only when its bound certifies it. `avenc pareto` sweeps one variable across a range and marks the knee point of the resulting trade-off curve.

### Adjacent — runtimes, adapters & integrations

These extend the core to real systems. They are useful, but the core compiler stands on its own without them.

- **AERIC edge runtime (`--aeric-shim`):** `avenc emit-c --aeric-shim --out-dir <dir>` emits a deployment bundle with a SHA-256 model-identity hash and non-aborting domain finiteness guards; `aeric/` is the companion dependency-free C99 runtime that calls the emitted model on a tick behind an enforced HOLD fail-safe.
- **External Adapters & Persistent IPC Daemons:** Connect relations to sensors, telemetry streams and black-box simulators via subprocess adapters (`using external(...)`) or persistent TCP daemons (`using ipc(...)`). Adapters exchange values in each variable's declared unit.
- **Shared Memory Rings & Native In-Process FFI (`using shm(...)`, `using native(...)`):** Memory-mapped ring buffers and in-process loading of C/Rust dynamic libraries (`.so` / `.dll`), with no subprocess or socket per solve. `tests/phase4_shm_ffi.rs` times one call of each: shared-memory reads take 5–10 µs and native library calls 40–50 µs on a 4-vCPU Linux container (timings vary by machine). **Caveat:** a loaded library runs in-process, outside Aven's deterministic solver and ledger guarantees — its code is not sandboxed and its contents are not yet hashed into the ledger, so `using native(...)` is for trusted libraries only. (Recording the library's SHA-256 in the ledger is planned.)
- **Formal evidence, packaging & shells:** `avenc emit-smt` / `safety-case` export a model to SMT-LIB2 and run Z3/CVC5 for formal-methods evidence (not certification); `Aven.toml` / `.avenpkg` describe a SHA-256-sealed package format (resolution not yet implemented). A Tauri desktop shell (`app/`) and a Kubernetes controller (`k8s/`) drive the same in-process compiler.

---

## System Architecture

```
┌────────────────────────────────────────────────────────────────┐
│ Aven Source  (*.aven specification)                            │
└────────────────────────────────┬───────────────────────────────┘
                                 ▼
┌────────────────────────────────────────────────────────────────┐
│ avenc Compiler   (pure std, zero dependencies)                 │
│ Lexer → AST & Parser → VarMap & Constraints                    │
└────────────────────────────────┬───────────────────────────────┘
                ┌────────────────┴──────────────────┐
                ▼                                   ▼
┌──────────────────────────────┐    ┌──────────────────────────────┐
│ Relational Solver Engine     │    │ Direction-Pinned             │
│ Newton + symbolic diff,      │    │ C / WASM Transpiler          │
│ bounds, optimization         │    │ branchless, solver-free      │
└───────────────┬──────────────┘    └───────────────┬──────────────┘
                ▼                                   ▼
┌──────────────────────────────┐    ┌──────────────────────────────┐
│ Telemetry & IPC Fusion       │    │ Embedded / Bare-Metal        │
│ external / ipc / shm /       │    │ MCUs, RTOS, PLC              │
│ native adapters              │    │ runtimes                     │
└───────────────┬──────────────┘    └──────────────────────────────┘
                ▼
┌────────────────────────────────────────────────────────────────┐
│ Cryptographic Fact Ledger & Provenance                         │
│ SHA-256 hash chain, HMAC signing, TTL freshness                │
└────────────────────────────────────────────────────────────────┘
```

---

## Language Tour & Syntax

### 1. Omnidirectional Physical Relations
A relation describes mathematical invariants that must hold true. Any unknown can be solved given sufficient inputs:

```aven
// examples/flight_mesh.aven — ISA atmosphere & flight dynamics
// Inputs declare their units, so Aven checks dimensions and converts on the
// way in (surface_temp in degC, solved in kelvin) and out (lift in newtons).
signal altitude     : m
signal surface_temp : degC
signal airspeed     : m/s
signal air_density  : kg/m^3
signal wing_area    : m^2
signal lift_coeff   : scalar

relation FlightDynamics {
    ambient_temp     = surface_temp - lapse_rate * altitude
    dynamic_pressure = 0.5 * air_density * airspeed^2
    lift             = dynamic_pressure * wing_area * lift_coeff
}

// Give a target lift and leave airspeed unknown — Aven solves it backward.
solve FlightDynamics with {
    surface_temp = 15 degC
    altitude     = 3000 m
    air_density  = 0.909 kg/m^3
    lapse_rate   = 0.0065
    wing_area    = 32 m^2
    lift_coeff   = 0.55
    lift         = 90000 N
}
```

```text
Solver Trace:
  [ambient_temp = surface_temp - lapse_rate * altitude] ambient_temp <- forward eval
  [lift = dynamic_pressure * wing_area * lift_coeff] dynamic_pressure <- backward Newton
  [dynamic_pressure = 0.5 * air_density * airspeed ^ 2] airspeed <- backward Newton

Settled Environment:
  airspeed         = 106.0713 m/s
  ambient_temp     = 268.6500 K
  dynamic_pressure = 5113.6364 Pa
  lift             = 90000.0000 N
  ...
```

### 2. High-Performance Persistent IPC Daemon
For real-time control and high-frequency streams, declare relations backed by persistent socket services:

```aven
// Connect directly to a persistent telemetry daemon via local TCP socket
relation FlightDynamics using ipc("127.0.0.1:9090") {
    // The daemon supplies the observed facts over TCP; these equations solve over them
    dynamic_pressure = 0.5 * air_density * airspeed^2
    lift             = dynamic_pressure * wing_area * lift_coeff
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
| **`avenc bench`, live IPC daemon (one client)** | 1414.9 µs median (p95 2120.8 µs) | 664.3 solves/sec | `evaluations/flight_mesh/runs/2026-10-27-linux/avenc_bench.txt`, 4-vCPU Linux container, 2026-10-27 |
| **Subprocess, `stream_ringer.py`, 50 live flights** | `70.8 ms` | 51.4 solves/sec | Same container and date |
| **IPC daemon, `stream_ringer.py`, 50 live flights** | `6.2 ms` | 570.9 solves/sec | Same container and date |

The `stream_ringer.py` rows include the harness's own process and worker overhead. The `avenc bench` row is the complete solve (TCP round trip to the daemon plus the relational solve of `flight_mesh_ipc.aven`), measured with `avenc bench evaluations/flight_mesh/flight_mesh_ipc.aven --iterations 2000` against `evaluations/flight_mesh/flight_mesh_daemon.py`. The numbers are the ones in `evaluations/flight_mesh/runs/2026-10-27-linux/avenc_bench.txt`. A Windows workstation run from 2026-10-25 has no log in this repository, so it is not stated here.

- **Standard Atmosphere Verified:** Temperature, pressure and Mach number match an independent ISA computation for all 150 live aircraft checked, including 30 cruising above the 11 km tropopause (up to 43,000 ft); see `verify/live_atmosphere.py`. Re-run on 2026-10-27: 150 aircraft, 32 above the tropopause (up to 45,000 ft), 0 mismatches (`evaluations/flight_mesh/runs/2026-10-27-linux/live_atmosphere.txt`). (An earlier model extended the tropospheric lapse rate above the tropopause and reported spurious −80 °C bound violations at cruise altitude; that model error is fixed. `evaluations/flight_mesh/flight_mesh_cycle_log.jsonl` is a run log from before the fix.)

### 8-Hour Overnight Weather Operations Monitor
`evaluations/weather/overnight_monitor.py` polled the National Weather Service for 5 airports (KDEN, KDFW, KJFK, KORD, KSEA) on 2026-10-25. Its per-cycle log, `evaluations/weather/weather_cycle_log.jsonl`, records:
- **486 evaluation cycles** over 8.0 hours.
- **410 actionable solves** (84.4%).
- **76 cycles held** on expired sensor TTLs (`DECISION BLOCKED`).

Both `*_cycle_log.jsonl` files are the harnesses' per-cycle logs, not `avenc` hash-chained ledgers, so `avenc verify-ledger` does not apply to them. (They were named `*_audit_ledger.jsonl` in earlier runs.)

---

## CLI Toolchain Reference (`avenc`)

Install the compiler (needs a Rust toolchain; builds from source, no external crates):
```bash
cargo install --git https://github.com/Thrainkrilleve/aven.git avenc
```

Or build the standalone binary from a checkout:
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
  lift             = 51986.8827 N
  lift_coeff       = 0.5500 scalar
  velocity         = 250.0000 km/h
  wing_area        = 32.0000 m^2

Fact Ledger: 6 facts settled.
```
Values print in their declared unit, or in the base unit of their inferred dimension. `lift_coeff` is declared `scalar` (dimensionless), so `lift` is inferred as newtons; a coefficient left without a unit would leave `lift`'s dimension unknown and unlabelled. `--json` prints base-unit values with a `units` map.

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
├── evaluations/                            # Live-data harnesses and test logs
├── verify/                                 # Checks against independent references
├── benchmarks/                             # Performance benchmarks & RHEL harnesses
├── web/                                    # avenc.dev site and live solver demo (WASM)
├── aeric/                                  # AERIC C runtime for edge deployment
├── app/                                    # Desktop IDE shell (Tauri v2)
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
