# Roadmap and Future Vision

From an experimental relational solver toward a multi-physics compiler, Aven aims to provide **deterministic, checkable models for physical and autonomous systems**.

---

## 1. Development Milestones

### Phase 1: Relational Core & Pure Rust Architecture (Completed)
- [x] Omnidirectional mathematical solver (Newton–Raphson with symbolic derivatives).
- [x] Zero-dependency standard library Rust compiler (`avenc`).
- [x] Comprehensive AST with precise line and column span diagnostics.
- [x] Immutable, append-only Fact Ledger with native SHA-256 cryptographic provenance.
- [x] Sibling JSON Schema contract validator (`*.schema.json`).

### Phase 2: Direction-Pinning & C Codegen (Completed)
- [x] Topological sort of relational dependency graphs.
- [x] Direction-pinned forward blocks (`forward`).
- [x] C99 code emission (`avenc emit-c`) for solver-free embedded targets.
- [x] Trade-off sweeps with knee-point detection (`avenc pareto`).

### Phase 3: Telemetry Mesh & Persistent IPC Daemons (Completed)
- [x] Subprocess external evaluators (`using external(...)`) with stdin/stdout JSON contracts.
- [x] Multi-stream physics ingestion (OpenSky ADS-B + NOAA METAR + USGS River Basins).
- [x] 8-hour live overnight weather audit across 5 major US airport hubs (486 cycles).
- [x] First-class persistent loopback IPC (`using ipc(...)`) via pure Rust `std::net::TcpStream`.
- [x] IPC pipeline at **9.7 ms median latency** and **352.9 solves/second** (multi-worker harness, Windows; `flight_mesh_summary_report.md`).

---

## 2. Future Roadmap

### Phase 4: Shared-Memory Rings & Native FFI (Completed)
- [x] **Shared Memory Rings (`shm`):** Bypassing loopback sockets entirely by memory-mapping telemetry buffers into shared ring buffers (`using shm("path")`), with the phase 4 test asserting reads under **$100\,\mu\text{s}$** (5–10 µs per read on a 4-vCPU Linux container).
- [x] **C / Rust Native FFI Plugins (`using native(...)`):** Dynamic library loading (`.so` / `.dll`) for in-process evaluation (40–50 µs per call on the same container).
- [x] **Bidirectional IPC Daemon Control:** After settling, `avenc run` sends the solved values back to `ipc` and `shm` backends as actuation commands, for the daemon to pass on to actuators.

### Phase 5: Formal Verification, SMT Export & Safety Cases (Completed)
- [x] **SMT-LIB2 Export & Prover:** Transpile Aven invariant specifications to SMT-LIB2 v2.6 for Z3 / CVC5 theorem proving with refutation-based invariant verification (`avenc emit-smt`, `avenc verify-smt`).
- [x] **Safety Case Generation:** Draft safety-case reports structured around DO-178C (Table A-5 / DO-333) and ISO 26262, with SMT proof results per invariant (`avenc safety-case`). They are inputs to a certification effort, not certification.
- [x] **High-Resolution RT Timing & Jitter Profiler:** Real-time single-tick WCET latency quantiles (min/p50/p90/p95/p99/p99.9/max) and cycle jitter reporting (`avenc bench --latency-profile`).

### Phase 6: Temporal Rigor, Ledger Compaction & Interactive Debugger (Completed)
- [x] **Signal Calculus (library API):** Bounded retention windows, staleness decay (Linear, Exponential, Step), rate of change, moving average, EMA, variance and cross-signal alignment in `src/temporal.rs`. Not yet exposed as language syntax.
- [x] **Ledger Compaction & Segmented Audit Logs:** `avenc compact-ledger`; segment rotation (`SegmentedLedgerWriter`) and cross-segment verification (`verify_segmented_directory`) as library APIs.
- [x] **Interactive Time-Travel Debugger (`avenc debug`):** Interactive REPL and headless CLI, timeline scrubbing (`goto`, `step`, `prev`), irreversible commit boundary enforcement, causal provenance graph rendering (`why`), and counterfactual branching with output diffing (`what-if`, `diff`).

### Phase 7: Ecosystem Bootstrap, Package Registry & WebAssembly (Completed)
- [x] **Package Manager & Cryptographic Registry:** Unit-verified relation packages with `Aven.toml`, `.avenpkg` archives with SHA-256 integrity checks, local installation, listing and `import` resolution. Manifest dependencies are recorded but not yet fetched (`avenc pkg init|pack|install|list`).
- [x] **WebAssembly Core 1.0 Transpiler:** Standard `.wasm` binary bytecode and `.wat` text emission (`avenc emit-wasm`) implementing pure-`std` LEB128 encoding, terminal calculation exports, and zero-copy memory-mapped `aven_solve` solver interfaces verified via Node.js V8 execution.
- [x] **Reference Models:** Portfolio allocation (`examples/portfolio_rebalancing.aven`) and drone avionics (`examples/quadcopter_avionics.aven`), also running in the website playground.
- [x] **Documentation & Community Governance:** Weekend-first onboarding guide (`docs/GETTING_STARTED.md`), package specification (`docs/PACKAGE_SPEC.md`), language RFC lifecycle (`RFC_PROCESS.md`), Contributor Covenant v2.1 (`CODE_OF_CONDUCT.md`), and contribution ground rules (`CONTRIBUTING.md`).

---

## 3. The Long-Term Vision

Modern software is drowning in runtime exceptions, race conditions, and drift because systems are programmed as imperative sequences of fragile steps.

**Aven replaces procedural scripts with declared mathematical invariants.** Whether sizing a Kubernetes cluster, checking a flight envelope, or cooling a datacenter, Aven checks every decision against the model's relations and bounds, refuses to act on stale inputs, and records each decision in a tamper-evident audit log.
