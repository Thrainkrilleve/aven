# Roadmap and Future Vision

From its inception as an experimental relational solver to a production-grade multi-physics compiler, Aven is built to provide **mathematical determinism for physical and autonomous systems**.

---

## 1. Development Milestones

### Phase 1: Relational Core & Pure Rust Architecture (Completed)
- [x] Omnidirectional mathematical solver (symbolic differentiation + Newton-Raphson relaxation).
- [x] Zero-dependency standard library Rust compiler (`avenc`).
- [x] Comprehensive AST with precise line and column span diagnostics.
- [x] Immutable, append-only Fact Ledger with native SHA-256 cryptographic provenance.
- [x] Sibling JSON Schema contract validator (`*.schema.json`).

### Phase 2: Direction-Pinning & C Codegen (Completed)
- [x] Topological sort of relational dependency graphs.
- [x] Direction-pinned forward blocks (`forward`).
- [x] Clean ANSI C code emission (`avenc emit-c`) for zero-overhead embedded targets.
- [x] Multi-objective Pareto frontier exploration (`avenc pareto`).

### Phase 3: Telemetry Mesh & Persistent IPC Daemons (Completed)
- [x] Subprocess external evaluators (`using external(...)`) with stdin/stdout JSON contracts.
- [x] Multi-stream physics ingestion (OpenSky ADS-B + NOAA METAR + USGS River Basins).
- [x] 8-hour live overnight weather audit across 5 major US airport hubs (486 cycles).
- [x] First-class persistent loopback IPC (`using ipc(...)`) via pure Rust `std::net::TcpStream`.
- [x] Sub-10ms performance ceiling shattered: **9.7 ms median latency** and **352.9 physical solves/second**.

---

## 2. Future Roadmap

### Phase 4: Shared-Memory Rings & Native FFI (Completed)
- [x] **Shared Memory Rings (`shm`):** Bypassing loopback sockets entirely by memory-mapping telemetry buffers into shared ring buffers (`using shm("path")`), driving latency below **$100\,\mu\text{s}$** ($10.1\,\mu\text{s}$ measured).
- [x] **C / Rust Native FFI Plugins (`using native(...)`):** Dynamic library loading (`.so` / `.dll`) for zero-copy in-process evaluation ($5.5\,\mu\text{s}$ measured).
- [x] **Bidirectional IPC Daemon Control:** Real-time actuation commands streamed from Aven directly to physical actuators (valves, flaps, throttling governors).

### Phase 5: Formal Verification, SMT Export & Safety Cases (Completed)
- [x] **SMT-LIB2 Export & Prover:** Transpile Aven invariant specifications to SMT-LIB2 v2.6 for Z3 / CVC5 theorem proving with refutation-based invariant verification (`avenc emit-smt`, `avenc verify-smt`).
- [x] **Safety Case Generation:** Automated compliance documentation for FAA DO-178C (Table A-5 / DO-333) and ISO 26262 ASIL-D functional safety audits (`avenc safety-case`).
- [x] **High-Resolution RT Timing & Jitter Profiler:** Real-time single-tick WCET latency quantiles (min/p50/p90/p95/p99/p99.9/max) and cycle jitter reporting (`avenc bench --latency-profile`).

### Phase 6: Temporal Rigor, Ledger Compaction & Interactive Debugger (Completed)
- [x] **Signal Calculus Formalization:** Bounded FIFO retention windows, continuous staleness decay (Linear, Exponential, Step), discrete rate-of-change ($dv/dt$), moving average, EMA, variance, and cross-signal temporal alignment (`src/temporal.rs`).
- [x] **Ledger Storage Compaction & Segmented Audit Logs:** Segment rotation (`SegmentedLedgerWriter`), cross-segment cryptographic chain verification (`verify_segmented_directory`), and settled historical snapshot compaction (`LedgerCompactor`, `avenc compact-ledger`).
- [x] **Interactive Time-Travel Debugger (`avenc debug`):** Interactive REPL and headless CLI, timeline scrubbing (`goto`, `step`, `prev`), irreversible commit boundary enforcement, causal provenance graph rendering (`why`), and counterfactual branching with output diffing (`what-if`, `diff`).

### Phase 7: Ecosystem Bootstrap, Package Registry & WebAssembly (Completed)
- [x] **Package Manager & Cryptographic Registry:** Unit-verified relation packages with `Aven.toml`, `.avenpkg` distribution archives with SHA-256 seal integrity verification, dependency installation, package enumeration, and namespaced import resolution (`avenc pkg init|pack|install|list`).
- [x] **WebAssembly Core 1.0 Transpiler:** Standard `.wasm` binary bytecode and `.wat` text emission (`avenc emit-wasm`) implementing pure-`std` LEB128 encoding, terminal calculation exports, and zero-copy memory-mapped `aven_solve` solver interfaces verified via Node.js V8 execution.
- [x] **Kill-Switch Domain Applications:** Structurally superior reference models in financial engineering (`examples/portfolio_rebalancing.aven`) and drone avionics (`examples/quadcopter_avionics.aven`).
- [x] **Documentation & Community Governance:** Weekend-first onboarding guide (`docs/GETTING_STARTED.md`), package specification (`docs/PACKAGE_SPEC.md`), language RFC lifecycle (`RFC_PROCESS.md`), Contributor Covenant v2.1 (`CODE_OF_CONDUCT.md`), and contribution ground rules (`CONTRIBUTING.md`).

---

## 3. The Long-Term Vision

Modern software is drowning in runtime exceptions, race conditions, and drift because systems are programmed as imperative sequences of fragile steps.

**Aven replaces fragile procedural scripts with unbreakable mathematical invariants.** Whether sizing a Kubernetes cluster, controlling a hypersonic flight envelope, or cooling a regional datacenter, Aven guarantees that decisions adhere to physical laws, respect safety boundaries, and carry verifiable cryptographic audit trails.
