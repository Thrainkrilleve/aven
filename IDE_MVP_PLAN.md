# Aven IDE — MVP Build Plan

*Turns [PRODUCT_THESIS.md](PRODUCT_THESIS.md) §8 into a concrete build. The MVP is thesis §8 **step 1 — "the loop"**; later steps are scoped here but gated behind it. Acceptance is the thesis's two done-tests, not a feature count.*

---

## 0. What we're building on (not from scratch)

The desktop app already exists: `app/aven-orchestrator`, a **Tauri v2** shell we hardened this cycle (compiles against `avenc` 0.9.0, version-gated, CI-checked). It is a **thin GUI over the one `avenc`** — a path dependency on the crate, no second solver — and the MVP keeps it that way.

**Today it is the wrong app.** The frontend is a single vanilla `dist/index.html` oriented around an orchestrator/flame/OpenPLC/telemetry demo. The backend already exposes commands we can reuse: `validate_model(source)` (unit/DOF check), `explain_why(...)` (derivation tree), `solve_point` / `solve_pareto`, `get_sample_models`. The MVP **reorients** this shell into an editor; the demo surface moves to an "Advanced" area or is removed.

Desktop is **native**, so the IDE calls `avenc` as a Rust library directly — full access to check, solve, `why`, and the ledger. (The `web/solver-wasm` build stays for the website; it is not used by the desktop app.)

## 1. Architecture

```
Editor UI (dist/)  ──Tauri commands──►  avenc::api (one surface)  ──►  avenc crate
  CodeMirror 6         check/solve/            check() solve()            units / solver /
  diagnostics          why/report/io           why()  report()           debugger / ledger
```

**Foundational refactor (M0):** today the richest "run + ledger" path lives in `src/main.rs` (the binary), not the library. Promote a small, stable **`avenc::api`** surface into the crate so the IDE, the CLI, and the wasm build all call one path:
- `check(source) -> Diagnostics` — unit + degrees-of-freedom errors, each with a **start–end source range** (not a single point, so CodeMirror can underline the span) and a **plain-language message**. Wraps `DimensionalChecker` + the determinacy check. Return ranges from day one: adding them later touches the parser.
- `solve(source) -> Solved { values, units, trace, ledger }` — wraps `SolverEngine` (the same path `web/solver-wasm` already proved).
- `why(source, var) -> Tree`.
- `report(source) -> ReportModel` (model + inputs + settled values with units + derivation + ledger head) — the data the HTML/PDF report renders from.

This is the "promote run-logic to the lib" item flagged earlier; doing it here pays off three ways at once.

**Two decisions to lock in M0 (cheap now, expensive later):**
- **File format.** A `.aven` file holds **source only** — the `solve` block already carries the inputs and solve settings, so there's nothing else to store. UI state (which variable is "solve for", panel layout) lives *outside* the file. Save (M1) and the re-runnable report (M2) both center on that one source text, so they can't drift apart.
- **Deterministic hashing.** `verify-report` only works if the hash is identical on every machine. Two rules that keep it honest:
  - The report embeds and verifies the **exact source that was solved** — including whatever the "solve for this" toggle generated — **not the file on disk**. Toggling a variable changes that source, and that's expected; embed it **verbatim** and have `verify-report` re-run *that* text.
  - The chain is over the settled facts (bit-exact `value_bits`), so verification doesn't hinge on source whitespace at all. Any on-disk normalization (LF endings, UTF-8 without BOM, trailing-whitespace trim) is **editor hygiene only**, and the trim is **scoped to line-ends outside quoted literals** (adapter paths, IPC endpoints) — Aven has no whitespace-sensitive literals today, and scoping the trim keeps it that way.

  Pin both before M1 ships the HTML export, or old reports fail to verify later.

## 2. Milestones

### M0 — Foundation
- Add `avenc::api` (above) with tests; keep `main.rs` calling it so CLI behavior is unchanged.
- Strip `app/aven-orchestrator` to an IDE shell: remove/relocate the flame/OpenPLC/backends/telemetry demo behind an "Advanced" toggle.
- Introduce a minimal **offline** frontend build (esbuild) and **vendor CodeMirror 6 locally** (no CDN — offline-first is a product principle). A `.aven` language mode: comments, keywords (`relation`/`forward`/`solve`/`signal`/`unit`/`using`), units, numbers.
- **Exit:** empty editor loads, types, and highlights; `avenc::api` has green tests; app still builds in CI.

### M1 — The Loop  *(the MVP — thesis §8 step 1)*
- **Inline check** — on edit (debounced), call `check` as an **async Tauri command, off the main thread**, so a slow solve never freezes the editor. Each request carries a **version number**; results that arrive out of order are dropped, so fast typing never shows stale errors. Underline the error **span** (start–end) with its message.
- **Plain diagnostics** — budget time to phrase errors the way a person would: *"Can't add m/s to s (line 4)"*, not the checker's internal form. The done-test's "points at where" is an acceptance criterion, not a nicety.
- **Solve panel** — a field per variable with a **"solve for this" toggle**: toggling one blanks it in the generated `solve` block and `solve` returns it (backward), so a new user does the 30-second demo **without editing code**. (The website demo's forward/reverse control is the working prototype.) Show settled values **with units** and the forward/backward trace.
- **Why tree** — click a value → `why` derivation tree.
- **Files** — new / open / save `.aven` via the Tauri dialog + fs; dirty-state indicator.
- **HTML report export** — render `report()` to a self-contained HTML file: model source, inputs, settled values with units, derivation, and the **ledger hash**, with the source embedded (**HTML-escaped**, so a model file can't inject a script into the report) so it is **re-runnable** (`avenc verify-report`, M2).
- **One excellent template** + **one guided walkthrough** (build → read results → break it down).
- **Exit = the thesis done-tests:**
  1. The 30-second demo lands: type a relation, blank a variable → it solves; break a unit → it refuses and points at where.
  2. A new user goes *open template → change numbers → solve backward → export report* in **under five minutes, no docs**.

### M2 — Share
- **Spreadsheet import:** CSV first, then `.xlsx` cell ranges (formulas/live links out of scope). Export values back to CSV/`.xlsx`.
- **PDF report export** (HTML report is already in M1).
- **`avenc verify-report`** CLI command: re-run an exported report and confirm the numbers + ledger hash — the mechanism that makes a report *defensible*, usable by anyone with just the CLI.

### M3 — See
- **Graphs:** sweep a variable (reuse `solve_pareto`) and plot the trade-off / sensitivity curve, with the knee marked. Offline charting lib, vendored.

### M4 — Pull / Learn
- **Optional live-data template** (the one validated reference example) — clearly marked optional; **the core loop never touches the network.**
- **Full guided series** and the **template library** (pipe sizing, battery runtime, Little's Law, …), each a checked, runnable starter.

## 3. Cross-cutting

- **Offline-first.** Everything bundled; no CDN, no network in the core loop. This is a selling point, not a constraint to fight.
- **One solver.** The IDE never reimplements solving — it calls `avenc::api`. The existing version gate (shell version must match the workspace) and the CI `cargo check` of the shell stay green.
- **Security.** Treat a model file as untrusted input: scope Tauri `fs` permissions to **user-picked paths only** (dialog-granted, not blanket), set a **strict Content-Security-Policy** on the web view, and **HTML-escape** any model source embedded in exported reports.
- **Testing.** Test `avenc::api` directly in Rust — most of the safety lives there — and add a test that each Tauri command is wired to it. That gets most of the coverage without a browser harness.
- **Distribution** (thesis §9): the installer bundles the WebView2 runtime; it ships from **GitHub Releases and/or avenc.dev**. Decide the channel before the first external user; `cargo install --git` and site installs need the repo public.

## 4. Explicitly out of the MVP

- The orchestrator/flame/OpenPLC/backends/telemetry demo (relocate to "Advanced" or remove).
- Level-3 real-time control (AERIC RT split, MISRA, WCET).
- A package *registry* — templates ship with the app; no resolution.
- Any live-data dependency in the core loop.

## 5. Honest notes

- **The frontend gains a dev-time node/esbuild step.** The *shipped* output stays bundled and offline; the toolchain is only for building the app, and it stays off the `avenc` crate (which remains zero-dependency).
- **M0's `avenc::api` is the keystone.** It is modest, but the loop, the report, and `verify-report` all stand on it; build and test it first.
- **Keep the GUI thin.** The moment the IDE starts computing things `avenc` doesn't, the single-source-of-truth guarantee (and the audit story) breaks.
