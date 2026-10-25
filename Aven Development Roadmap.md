# Aven Development Roadmap
### From prototype core (v0.2) to 1.0 release

*Aven: relations settle; time flows; you can always row back.*

---

## Where We Are — Honest Status

| Capability | Status |
|---|---|
| Expression parsing, evaluation | ✅ Solid |
| Symbolic differentiation (product/chain/power rule, functions) | ✅ Solid |
| Single-unknown backward solving (Newton + symbolic derivative) | ✅ Solid |
| Coupled multi-unknown system solving (multidim Newton, analytic Jacobian) | ✅ v0.2 (rank-deficiency reporting still weak) |
| Dimensional/unit algebra | ✅ Basic (4 dimensions, hardcoded) |
| Direction-pinned C transpilation | ✅ Prototype (timing-parity benchmark passed at phase 2) |
| Fact ledger, provenance (`why`), rewind/fork/branch, irreversibility guards | ✅ Prototype |
| **Not yet:** real `.aven` grammar, modules, strings/data structures, IO, package manager, error taxonomy, docs, tests, community | ❌ All ahead |

**Blunt assessment:** the research risk is retired — the paradigm computes, compiles, and time-travels. What remains is 90% engineering and 10% formalization. Languages die from the engineering and community parts, not the ideas.

---

## Phase 4 — Realism (target: v0.3)
*Goal: Aven stops being a demo and becomes a tool you'd actually reach for.*

1. **Full grammar & front-end.** A real lexer/parser for `.aven` files: `relation`/`forward` blocks, `solve ... with`, `signal` declarations, `when` clauses, comments. ANTLR or hand-written; generate good error messages from day one (they are a feature, not polish).
2. **Type checker.** The unit system generalizes beyond 4 hardcoded dimensions: extensible dimension registries, unit literals (`90 km/h`), full compile-time propagation through relation graphs.
3. **Under-determination reporting.** The coupled solver currently stalls silently on rank-deficient systems. It must *explain*: which variables remain free, why, and what additional binding would pin them. An under-constrained program should feel like an unbalanced equation in math class — obvious and educational.
4. **Test suite.** Golden-file tests for the solver (analytic cross-checks), transpiler (compare emitted C vs handwritten), and debugger (deterministic replay fuzzing).
5. **CLI:** `avenc run file.aven`, `avenc build file.aven --emit-c`, `avenc repl`.

---

## Phase 5 — The Contract with the Machine (target: v0.4)
*Goal: performance claims become enforced guarantees, not benchmarks.*

1. **`extern "C"` interop.** Bind C libraries directly — instant stdlib by theft. FFI bindings with unit-safe wrappers for common crates (libc, SDL, sqlite).
2. **Tiered compilation formalized.** The escape ladder (solver → pinned → `@region` → `extern`) becomes an explicit annotation-checked contract. The compiler *refuses* to emit solver-runtime calls in pinned blocks — already true in the prototype; now enforced uniformly with clear diagnostics.
3. **Benchmark suite in CI.** The phase-2 timing-parity test plus SIMD-bound and allocation-heavy workloads. Regression budgets: pinned code within 10% of handwritten C, always.
4. **Determinism spec.** Document and test: identical ledger + identical inputs ⟹ identical replay, bit-for-bit, across platforms. This is the load-bearing wall for the debugger; it must never crack.

---

## Phase 6 — Time Made Rigorous (target: v0.5)
*Goal: the temporal semantics stop being intuitive and start being specified.*

1. **Signal calculus formalization.** Write down the denotational semantics of `Signal<T>`: retention windows, staleness, `when`-filtering, cross-signal alignment. Publish it — a small paper or long spec chapter. If it can't be formalized, that's a design flaw to fix, not to hide.
2. **Rewind guarantees, specified.** Exact semantics of the irreversibility boundary: what `commit` means, what rewinding across sinks may and may not do, and the explicit-override protocol.
3. **Ledger storage & compaction.** Long runs need bounded memory: segment files, fact compression (structural sharing of repeated derivation shapes), snapshotting of settled subgraphs.
4. **The debugger UI.** `aven debug` — timeline scrubbing, provenance tree rendering, branch diffing. The killer app; incumbents genuinely cannot retrofit this.

---

## Phase 7 — Ecosystem Bootstrap (target: v0.6–0.9)
*Goal: someone chooses Aven for a real project and doesn't regret it.*

1. **Package manager & registry.** Namespaced by unit-verified relation packages — a physics package where every equation ships with its dimensions is a uniquely Aven-shaped niche.
2. **Kill-switch applications.** Target domains where Aven's model is *structurally* superior, not merely comparable: financial modeling (bidirectional spreadsheets-with-types), scientific computing, simulation/debugging-heavy work, configuration with constraints.
3. **Docs with a weekend-first promise.** The "motivated beginner ships something in a weekend" bar becomes an actual measured onboarding study, not a vibe.
4. **Interop bridge.** Transpiler targets initially C; a WASM backend unlocks browser experiments cheaply.
5. **Community scaffolding.** Governance, RFC process, code of conduct, contribution guide — boring, essential, started early because rewriting culture late never works.

---

## Phase 8 — 1.0 (no date promised)
Ship when: semantics frozen, debugger flagship-quality, two real production users who aren't us, and the spec passes its own test suite under fuzzing. **No dates.** Date-driven language releases ship their roadmap; quality-driven ones ship their promises.

---

## Standing Risks (tracked, not solved)

- **Newton fragility.** Multistart Newton finds roots, not *the* root you meant. Mitigation trajectory: constraint intervals (interval arithmetic for bounds), then SMT-assisted uniqueness proofs where possible. Long-term this is the deepest theoretical gap.
- **Rank deficiency / under-determination.** Identified in v0.2's coupled solver. Must become first-class diagnostics, per Phase 4.3.
- **The coup problem.** Excel dominates the "bidirectional arithmetic for normal humans" niche. Aven's wedge is typed, compilable, testable, and debuggable in time. We don't beat spreadsheets at spreadsheets; we replace them where they hurt.
- **Solo-project gravity well.** Everything above assumes sustained effort and eventually collaborators. The roadmap is deliberately ordered so each phase leaves a *useful artifact even if the project stops there* — v0.3 alone is a decent constraint-solving toolkit.

---

## What I'd Do Next, Concretely

The single highest-leverage next task is **Phase 4.3 (under-determination reporting)**, because it converts the coupled solver's biggest weakness into the language's best teaching feature — errors that explain themselves. Second choice: the real `.aven` grammar, because everything downstream of the front-end gets harder the longer it's fed by demo strings.

---

*Aven v0.2 — the paradigm computes. Now we make it a product.*
