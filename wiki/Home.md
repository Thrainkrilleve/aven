# Welcome to the Aven Wiki

**Aven** is a relational, invariant-driven programming language and deterministic solver engine designed for complex multi-physics systems, telemetry ingestion, automated control, and multi-objective optimization.

Unlike traditional imperative languages where functions require hardcoded input/output parameters, Aven relations represent **omnidirectional mathematical invariants**. Write your physical laws once, and Aven solves for any unknown variable with Newton–Raphson using exact symbolic derivatives—or transpiles `forward` blocks directly to branchless, solver-free C99.

---

## Quick Navigation

- **Core Concepts:**
  - [[Architecture and Design|Architecture-and-Design]]: The fundamental philosophy of omnidirectional solving and zero-dependency compilation.
  - [[Language Specification|Language-Specification]]: Comprehensive syntax, grammar, operators, functions, and relational blocks.
  - [[Compiler Toolchain (avenc)|Compiler-Toolchain-avenc]]: Complete CLI reference (`run`, `check`, `emit-c`, `pareto`, `bench`).
- **Telemetry & Systems Integration:**
  - [[External Evaluators & IPC Daemons|External-Evaluators-and-IPC-Daemons]]: Subprocess black-boxes vs. persistent loopback IPC daemons (sub-10ms solving).
  - [[Fact Ledgers & Provenance|Fact-Ledgers-and-Provenance]]: Append-only audit trails, SHA-256 fact hashing, and TTL freshness enforcement.
  - [[Kubernetes Orchestration|Kubernetes-Orchestrator-Integration]]: Automated pod sizing, datacenter telemetry, and infrastructure controllers.
- **Verification & Benchmarks:**
  - [[Multi-Stream Physics Benchmarks|Multi-Stream-Physics-Benchmarks]]: Empirical results from live OpenSky ADS-B, NOAA METARs, and USGS Hydrology.
  - [[Roadmap & Future Vision|Roadmap-and-Future-Vision]]: Evolution from Aven 0.1 to industrial autonomy.

---

## High-Level Architectural Summary

```
                       ┌──────────────────────────────────────────────┐
                       │               Aven Source Code               │
                       │             *.aven specification             │
                       └──────────────────────┬───────────────────────┘
                                              │
                                              ▼
                       ┌──────────────────────────────────────────────┐
                       │           avenc Compiler (Rust Std)          │
                       │  • Pure-std recursive descent parser         │
                       │  • AST validation & span diagnostics         │
                       │  • Direction-pinning topological sort        │
                       └──────────────────────┬───────────────────────┘
                                              │
                     ┌────────────────────────┴────────────────────────┐
                     ▼                                                 ▼
┌───────────────────────────────────────────┐     ┌───────────────────────────────────────────┐
│         Relational Solver Engine          │     │        C99 Emitter (avenc emit-c)         │
│  • Omnidirectional equation solving       │     │  • Pure forward evaluation                │
│  • Guarded Newton-Raphson relaxation      │     │  • Branchless, zero-solver C code         │
│  • Inequality & domain bound enforcement  │     │  • Straight-line, fixed-cost C99 code     │
└────────────────────┬──────────────────────┘     └───────────────────────────────────────────┘
                     │
                     ▼
┌───────────────────────────────────────────┐
│        Telemetry Ingestion & IPC          │
│  • Subprocess: using external(...)        │
│  • Persistent Daemon: using ipc(...)      │
│  • IPC: 9.7 ms median, 352.9 solves/s     │
└────────────────────┬──────────────────────┘
                     │
                     ▼
┌───────────────────────────────────────────┐
│     Cryptographic Audit Ledger            │
│  • Append-only fact log                   │
│  • SHA-256 hash chain, optional HMAC      │
│  • TTL staleness protection (exit 3)      │
└───────────────────────────────────────────┘
```

---

## Key Benchmarks at a Glance

| Benchmark Environment | Evaluation Model | Median / Mean Latency | Throughput | Source |
| :--- | :--- | :--- | :--- | :--- |
| **Persistent Loopback IPC** | `flight_mesh_ipc.aven` | `9.7 ms` median | `352.9 solves/sec` | `evaluations/flight_mesh/flight_mesh_summary_report.md` (Windows) |
| **Optimized Subprocess** | `flight_mesh.aven` | `71.5 ms` median | 54.1 solves/sec | Windows run; no log in the repository |
| **8-Hour Weather Audit** | `atmospheric_dynamics.aven` | `316.0 ms` mean | 486 cycles in 8 h | `evaluations/weather/overnight_summary_report.md` and its log |
