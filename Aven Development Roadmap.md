# Aven Development Roadmap
### From prototype core (v0.2) to 1.0 release

*Aven: relations settle; time flows; you can always row back.*

> This is the plan written at v0.2, kept for history. Each phase item is marked with its status in `avenc` v0.6.0: ✅ done, ⚠️ partly done, ❌ not done. The status table below is current.

---

## Where We Are — Honest Status

| Capability | Status |
|---|---|
| Expression parsing, evaluation | ✅ Solid |
| Symbolic differentiation (product/chain/power rule, functions) | ✅ Solid |
| Single-unknown backward solving (Newton + symbolic derivative) | ✅ Solid |
| Coupled multi-unknown system solving (multidim Newton, analytic Jacobian) | ✅ Solid (with rank-deficiency diagnostics & minimal fixes) |
| Dimensional/unit algebra | ✅ Ten base dimensions (six SI plus angle, money, data, transactions), custom dimensions, run-time unit conversion |
| Direction-pinned C transpilation | ✅ C99 output checked against `avenc run` (`verify/c_diff.py`), plus AERIC deployment bundles |
| WebAssembly backend | ✅ Completed (WASM Core 1.0 binary & WAT text emission) |
| Fact ledger, provenance (`why`), rewind/fork/branch, irreversibility guards | ✅ Completed (with segmented compaction & time-travel debugger) |
| Package manager & registry | ⚠️ Local packages done (Aven.toml, .avenpkg with SHA-256 integrity check, install, import); dependencies are not fetched and the online index is a preview |
| Formal verification & safety cases | ✅ SMT-LIB2 export and Z3/CVC5 runs; ⚠️ safety-case reports are drafts structured for DO-178C / ISO 26262, not certification evidence |

---

## Phase 4 — Realism (v0.3)
*Goal: Aven stops being a demo and becomes a tool you'd actually reach for.*

1. ⚠️ **Full grammar & front-end.** (Done except `when` clauses, which were never implemented.) A real lexer/parser for `.aven` files: `relation`/`forward` blocks, `solve ... with`, `signal` declarations, `when` clauses, comments. ANTLR or hand-written; generate good error messages from day one (they are a feature, not polish).
2. ✅ **Type checker.** The unit system generalizes beyond 4 hardcoded dimensions: extensible dimension registries, unit literals (`90 km/h`), full compile-time propagation through relation graphs.
3. ✅ **Under-determination reporting.** The coupled solver currently stalls silently on rank-deficient systems. It must *explain*: which variables remain free, why, and what additional binding would pin them. An under-constrained program should feel like an unbalanced equation in math class — obvious and educational.
4. ✅ **Test suite.** Golden-file tests for the solver (analytic cross-checks), transpiler (compare emitted C vs handwritten), and debugger (deterministic replay fuzzing).
5. ✅ **CLI:** `avenc run file.aven`, `avenc build file.aven --emit-c`, and an interactive REPL (shipped as `avenc debug`).

---

## Phase 5 — The Contract with the Machine (v0.4)
*Goal: performance claims become enforced guarantees, not benchmarks.*

1. ⚠️ **`extern "C"` interop.** (Shipped as `using native("lib.so")`, which calls a C/Rust library function in process; no unit-safe wrappers for libc, SDL or sqlite exist.) Bind C libraries directly — instant stdlib by theft. FFI bindings with unit-safe wrappers for common crates (libc, SDL, sqlite).
2. ⚠️ **Tiered compilation formalized.** (`forward` blocks compile without solver calls; the `@region` tier and annotation-checked contract were not built.) The escape ladder (solver → pinned → `@region` → `extern`) becomes an explicit annotation-checked contract. The compiler *refuses* to emit solver-runtime calls in pinned blocks — already true in the prototype; now enforced uniformly with clear diagnostics.
3. ❌ **Benchmark suite in CI.** (`avenc bench` and `benchmarks/` exist, but no timing regression budget runs in CI.) The phase-2 timing-parity test plus SIMD-bound and allocation-heavy workloads. Regression budgets: pinned code within 10% of handwritten C, always.
4. ⚠️ **Determinism spec.** (Fixed solver seeds and bit-exact ledger values make replay deterministic on one machine; cross-platform bit-for-bit replay is not tested.) Document and test: identical ledger + identical inputs ⟹ identical replay, bit-for-bit, across platforms. This is the load-bearing wall for the debugger; it must never crack.

---

## Phase 6 — Time Made Rigorous (v0.5)
*Goal: the temporal semantics stop being intuitive and start being specified.*

1. ⚠️ **Signal calculus formalization.** (Implemented as a library API in `src/temporal.rs`; not part of the language syntax, and no denotational semantics document exists.) Denotational semantics of temporal streams: bounded FIFO retention windows, staleness decay models (Linear, Exponential, Step), discrete rate-of-change ($dv/dt$), moving average, EMA, variance, and cross-signal alignment (`src/temporal.rs`).
2. ✅ **Rewind guarantees, specified.** Exact semantics of the irreversibility boundary: what `committed-effect` means, enforcing commit barrier guards against accidental time-travel rewind without explicit `--force` override.
3. ⚠️ **Ledger storage & compaction.** (`avenc compact-ledger` ships; segment rotation and segment verification are library APIs not used by the CLI.) Bounded memory via segmented JSONL audit logs (`SegmentedLedgerWriter`), cross-segment cryptographic chain continuity audits (`verify_segmented_directory`), and historical snapshot compaction (`LedgerCompactor`, `avenc compact-ledger`).
4. ✅ **Interactive debugger UI & REPL.** `avenc debug` — timeline scrubbing (`goto`, `step`, `prev`), causal provenance DAG rendering (`why`), counterfactual what-if branching (`what-if`), and timeline diffing (`diff`).

---

## Phase 7 — Ecosystem Bootstrap (v0.6)
*Goal: someone chooses Aven for a real project and doesn't regret it.*

1. ⚠️ **Package manager & registry.** Dimension-checked relation packages with `Aven.toml` manifests, packed with a SHA-256 integrity digest (`avenc pkg pack`), installed from a local archive (`avenc pkg install`), listed (`avenc pkg list`), and imported (`import <pkg>;`). Manifest dependencies are not fetched, and the avenc.dev index is a read-only preview.
2. ✅ **Reference applications.**
   - Financial Engineering: three-asset allocation with capital conservation, expected return, a diversification-discounted volatility estimate and the Sharpe ratio (`examples/portfolio_rebalancing.aven`). It does not model covariances or Black–Litterman views, and computes rather than maximizes the Sharpe ratio.
   - Avionics & Aerospace: multi-rotor drone aerodynamics $T=4 \cdot c_T \cdot \rho \cdot \omega^2 \cdot D^4$, thrust-to-weight ratio, vertical climb rate, battery internal resistance voltage sag ($V_{term} = V_{oc} - I \cdot R_{int}$), and electrical power budgeting (`examples/quadcopter_avionics.aven`).
3. ✅ **Docs with a weekend-first promise.** Onboarding tutorial (`docs/GETTING_STARTED.md`) and package manifest specification (`docs/PACKAGE_SPEC.md`) enabling motivated practitioners to build and deploy verified relation packages in under 15 minutes.
4. ✅ **Interop bridge & WebAssembly.** Dual-mode WebAssembly emission (`avenc emit-wasm`): human-readable WebAssembly Text (`.wat`) and standard binary WebAssembly Core 1.0 bytecode (`.wasm`) with LEB128 encoding, zero-overhead direct calculation exports, and zero-copy memory-mapped `aven_solve(in_ptr, out_ptr)` buffers runnable directly in web browsers and Node.js V8 engines without external dependencies.
5. ✅ **Community scaffolding.** Formal governance lifecycle (`RFC_PROCESS.md`), Contributor Covenant v2.1 (`CODE_OF_CONDUCT.md`), and standard development guidelines (`CONTRIBUTING.md`) codifying zero external crate dependencies and deterministic PRNG testing.

---

## Phase 8 — 1.0 (no date promised)
Ship when: semantics frozen, debugger flagship-quality, two real production users who aren't us, and the spec passes its own test suite under fuzzing. **No dates.** Date-driven language releases ship their roadmap; quality-driven ones ship their promises.

---

## Standing Risks (tracked, not solved)

- **Newton fragility.** Multistart Newton finds roots, not *the* root you meant. Mitigation trajectory: constraint intervals (interval arithmetic for bounds), then SMT-assisted uniqueness proofs where possible. Long-term this is the deepest theoretical gap.
- **Rank deficiency / under-determination.** Identified in v0.2's coupled solver; now reported with free variables and minimal fixes (Phase 4.3).
- **The coup problem.** Excel dominates the "bidirectional arithmetic for normal humans" niche. Aven's wedge is typed, compilable, testable, and debuggable in time. We don't beat spreadsheets at spreadsheets; we replace them where they hurt.
- **Solo-project gravity well.** Everything above assumes sustained effort and eventually collaborators. The roadmap is deliberately ordered so each phase leaves a *useful artifact even if the project stops there* — v0.3 alone is a decent constraint-solving toolkit.

---

## What I'd Do Next, Concretely

*(Written at v0.2; both items below are now done.)* The single highest-leverage next task is **Phase 4.3 (under-determination reporting)**, because it converts the coupled solver's biggest weakness into the language's best teaching feature — errors that explain themselves. Second choice: the real `.aven` grammar, because everything downstream of the front-end gets harder the longer it's fed by demo strings.

---

*Aven v0.2 — the paradigm computes. Now we make it a product.*
