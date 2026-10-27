# AERIC Roadmap & Engineering Standard

**Status:** historical detail · **Active plan:** [Aven roadmap](<../Aven Development Roadmap.md>)

Level 1, and the finished Level 2 behavior (state, clamp, rate limit, named faults, HOLD), stay part of the product. Level 3 and the unfinished real-time items are parked until a named machine and a named deadline ask for them. Checkboxes below are evidence notes, not the plan.

**Rev:** 0.1 · **Last updated:** 2026-10-25
**Scope:** the standalone C99 runtime in `aeric/` that executes AVEN's AOT-compiled
models (`avenc emit-c`) on edge/embedded targets.

This document is both a **roadmap** (where AERIC is going) and a **standard**
(the bar every step must clear before we claim it is done). The two are
inseparable here on purpose: the project's guiding rule is that *nothing is
"true" until it is tested or measured*. The roadmap therefore defines
capabilities **and** the evidence that graduates each one from "aspirational" to
"stated."

---

## 1. Vision & non-goals

**Vision.** A dependency-free, deterministic, auditable control runtime that you
flash onto a device. It takes live sensor inputs, runs AVEN-compiled invariants,
publishes setpoints, degrades safely under fault, and produces a tamper-evident
record of everything it did — with no heap, no runtime, and no surprises.

**Positioning (not duplication).** Aven already has a Rust orchestrator with IPC
and shared-memory rings (`src/evaluator.rs`). AERIC is **not** a replacement for
it. The division of labor:

| | Rust daemon (host) | AERIC (edge) |
|---|---|---|
| Role | dev/host orchestration, tooling, IPC hub | the artifact flashed on the device |
| Deps | full Rust runtime | C99 + libm only |
| Ledger | HMAC-SHA256 chained JSONL | must emit the **same** format |

The two must **interoperate** (shared ledger format, and ideally a shared IPC
frame), so a run recorded on the edge is verifiable by the host's existing
`avenc verify-ledger`.

**Non-goals.** AERIC is not a general-purpose RTOS, not a solver (the solving is
AVEN's job, compiled ahead of time), and not a network service framework. It
does one thing: run a fixed control loop safely and provably.

---

## 2. Guiding principles (the standard)

1. **Evidence or it didn't happen.** No performance, safety, or security claim
   appears in docs or comments without a reproducible test or measurement behind
   it. Every such claim is tracked in the Claims Register (§6).
2. **Determinism is a feature.** Same inputs → bit-identical outputs **and**
   bit-identical ledger hashes, across runs, compilers, and platforms. This is
   verified by record-replay, not asserted.
3. **The real-time core never allocates and never blocks.** No `malloc`, no
   locks, no unbounded loops, no variable-time parsing on the tick path.
4. **The core is model-agnostic.** Model-specific knowledge lives only in the
   generated model unit and its shim. `aeric.c`/`aeric.h` never change to add a
   model.
5. **Fail safe, never fail silent.** Every fault class has a defined response;
   the floor is HOLD with configured fail-safe setpoints. Faults are recorded.
6. **Interop over reinvention.** Match the repo's existing formats (ledger JSONL,
   SHM layout) rather than inventing parallel ones.

---

## 3. Current baseline — Level 0 (done, verified)

The embeddable core, as of this revision.

**Capabilities**
- Single compiled model executed through a stable, model-agnostic ABI
  (`aven_solve(const double*, size_t, double*, size_t)`).
- Strict ingress: arity + finiteness validation.
- Enforced HOLD with configured fail-safe setpoints and an auto-recovery latch
  (`recover_ticks`, or manual `AERIC_ClearFault`).
- Dual-layer ledger: chained FNV-1a heartbeat + streaming SHA-256 seal.
- Zero dynamic allocation; all state in `aeric_runtime_t`.

**Evidence on record**
- Compiles with **zero warnings, MSVC `/W4 /O2`** (verified on this machine).
- Demo runs correctly: nominal solves match `avenc run` (1429 workers / 32676
  MB); `u=0` domain fault and wrong-arity tick both drop to HOLD; auto-recovers
  after exactly 3 clean solves; hashes chain.
- SHA-256 verified against FIPS-180-4 `abc` and empty-string vectors.

**Known limitations (honest list)**
- No clock, no I/O, no process lifecycle — it is a library, not a backend.
- Memoryless: no state carried across ticks (no PID/filter/estimator yet).
- Seal is **unkeyed** SHA-256 → not forgery-resistant; does not yet match the
  Rust side's HMAC.
- **No timing has been measured.** Any "microsecond" claim is currently unfounded.
- Tested only under MSVC; no gcc/clang `-Wpedantic`, no static analysis, no MISRA.
- Shim is hand-written (one manual, error-prone coupling).

---

## 4. The milestone ladder

Each level has a **Definition of Done (DoD)**: it is not reached until every
exit criterion is backed by an artifact (test, harness output, or report).

### Level 1 — Standalone daemon  *(turns the library into a backend)*

The spine. A long-running, config-driven process with its own clock and one
real transport.

**Capabilities**
- **Config-driven startup** — schema, model binding, I/O map, safe setpoints,
  tick rate, and output limits loaded from an `.aer` runtime config file (this
  is what `.aer` was always reaching for). No hardcoded `main.c`.
- **Timed loop** — fixed-rate ticks via `clock_nanosleep(TIMER_ABSTIME)`;
  measured jitter; **a missed deadline is a fault → HOLD.**
- **One transport driver** — start with length-prefixed TCP (trivial to test
  from Python); a narrow source/sink interface so Modbus/MQTT/OPC-UA/CAN slot in
  later. HOLD on disconnect or downstream NAK.
- **Ledger to disk** in the repo's existing JSONL format.
- **Graceful lifecycle** — clean startup/shutdown, final seal flushed on exit.

**Definition of Done**
- [x] A config file (`daemon.aer`) fully specifies a run — schema, safe state,
      rate, port, ledger; no hardcoded `main`. (Demonstrated for server_capacity.)
- [x] Loopback integration test: `daemon_peer.py` feeds inputs and reads setpoints
      over the socket; disconnect → HOLD.
- [x] Deadline-miss injection (peer pauses) → HOLD, recorded in the ledger.
      *(A distinct fault taxonomy — sensor vs deadline vs transport — is Level 2.)*
- [x] A daemon-produced ledger passes **`avenc verify-ledger`** unmodified
      (600 facts, chain intact, every signature valid; wrong key / tampered
      `value_bits` both rejected; a gcc-built daemon's ledger verifies identically).
- [~] Jitter: max measured (~17 ms across transitions, ~1.7 ms headless, 50 Hz);
      full p50/p99 over ≥10⁵ ticks and sub-ms RT still pending (Level 2/3).

**Status: met** — the daemon runs, holds rate, degrades to HOLD on
deadline-miss/disconnect, and writes a host-verifiable sealed ledger that the Rust
`avenc verify-ledger` accepts (and rejects when tampered), clean on MSVC/gcc/clang.
Remaining: only RT-grade (sub-ms) jitter, which is the Level 2/3 track.

**Effort:** L · **Depends on:** Level 0, Toolchain-A (nice-to-have), Verif-ledger.

---

### Level 2 — Control runtime  *(makes it genuinely capable)*

**Capabilities**
- **Statefulness** ✅ — a persistent state vector carried tick→tick and fed back
  as inputs (`x[t+1] = f(x[t], sensors)`). Unlocks PID, filters, estimators.
  *Done:* schema gains `n_state`; the caller supplies only sensors and AERIC
  appends `x[t]` as `[sensors | state]`, publishes the leading setpoints, and
  advances `x[t+1]` from the trailing outputs on a live tick (frozen on HOLD).
  Backward-compatible (`n_state = 0` is the old behavior). The daemon carries it
  through config + wire framing.
- **Graded safety envelope** ✅ (mostly) — per-output min/max **clamp** and
  setpoint **rate limiting** on the live path (`AERIC_SetEnvelope`), plus a
  **fault taxonomy** (sensor / solver / deadline / transport) tagged per tick and
  counted. HOLD remains the floor. *Remaining:* a **hardware watchdog kick**
  (target-specific).
- **Concurrency split** — a non-blocking, non-allocating RT control thread plus a
  lower-priority I/O/telemetry thread, joined by a *genuine* SPSC ring with
  atomics and memory barriers. (This is where a correct ring buffer belongs —
  unlike the original draft's.)
- **SHM interop with the Rust daemon** — couple to the Phase-4 ring
  (`AVEN_SHM_RING_01`, 64-byte header, 448-byte actuation zone at offset 64,
  slots at offset 512). **Note:** that ring is **JSON-framed**, not raw doubles,
  so this requires either (a) JSON↔double marshalling on the *non-RT* thread, or
  (b) a new **binary double-slot** format added to the ring. Pick (b) for a true
  zero-copy path.

**Definition of Done**
- [x] A PID controller runs statefully and matches a Python reference **exactly**
      over a trace: the integral ramps (memory), a HOLD tick freezes the state and
      publishes fail-safe, and the controller resumes from the frozen integral
      (`examples/pi_controller.aven`, `test_state.c`, `pi_check.py`). The stateful
      daemon's ledger passes `avenc verify-ledger`; the stateless path is unchanged.
- [x] Clamp and rate-limit proven against an out-of-range solve: the published
      output ramps from fail-safe by the rate limit then plateaus at the clamp
      ceiling, matching a Python reference exactly (`test_envelope.c`, `env_check.py`).
- [x] Each fault class (sensor / solver / deadline / transport) is injected,
      tagged by name, and counted per class.
- [ ] RT thread shows **zero page faults after init** and zero allocations on the
      tick path (verified with instrumentation / `mlockall` + counters).
- [ ] End-to-end SHM loop with the Rust daemon: telemetry in, setpoints out,
      with a **measured** cycle time (replacing any guessed "<15µs").

**Effort:** L–XL · **Depends on:** Level 1, Security-HMAC.

---

### Level 3 — Certifiable hard real-time  *(mostly process, not code)*

Pursue only if a target application demands it; it is a program, not a sprint.

**Capabilities**
- Linux `SCHED_FIFO` + CPU pinning + `mlockall`; bounded worst-case execution.
- WCET analysis of the tick path; redundancy / voting where required.
- Traceability to DO-178C / ISO 26262 / IEC 61508 objectives as applicable.

**Definition of Done**
- [ ] Measured worst-case tick latency under sustained load, with margin to the
      cycle budget, on the target hardware.
- [ ] Static analysis (MISRA addon) clean or with documented, reviewed deviations.
- [ ] Requirements↔code↔test traceability matrix for the safety functions.

**Effort:** XXL (ongoing) · **Depends on:** Level 2, all cross-cutting tracks green.

---

## 5. Cross-cutting tracks

These run in parallel with the ladder; several are prerequisites for honest
claims at Levels 1–2.

- **Toolchain-A — `avenc emit-c --aeric-shim`.** Generate the model unit *and* a
  correctly-ordered `aven_solve` shim, plus a **model-identity hash** stamped
  into the ledger. Removes the one hand-written, error-prone coupling.
  *DoD:* generated shim round-trips (compile + run) for ≥3 example models; its
  input order is diff-checked against the canonical order; ledger records the
  model hash. **Effort: M. Recommended first.**

- **Verif-ledger — shared ledger format.** C ledger emits byte-compatible JSONL;
  `avenc verify-ledger` validates it. *DoD:* a C-produced ledger verifies; a
  one-byte tamper is detected 20/20. **Effort: M.**

- **Security-HMAC — keyed seal.** Replace/augment the unkeyed SHA-256 seal with
  **HMAC-SHA256** to match the Rust side; key handling documented. *DoD:* host
  verifies an edge-signed ledger with the shared key; forgery without the key
  fails. **Effort: M.**

- **Timing harness.** A repeatable benchmark that reports p50/p99/max tick
  latency and jitter over a fixed workload. *DoD:* committed numbers; **doc rule:
  no latency claim may exist without a value from this harness.** **Effort: S–M.**

- **Determinism / record-replay.** Capture an input trace; replay → identical
  chained hashes and identical final seal, bit-for-bit, across ≥2 compilers.
  *DoD:* replay test in CI. **Effort: M.**

- **Portability & static analysis.** Build clean under gcc/clang
  `-std=c99 -Wall -Wextra -Wpedantic` and cppcheck/clang-tidy (MISRA addon when
  available). *DoD:* CI matrix green across MSVC + gcc + clang. **Effort: S–M.**

- **Fuzzing.** Fuzz the ingress and (once present) the transport/config parsers.
  *DoD:* no crash/UB over a sustained campaign (ASan/UBSan). **Effort: M.**

---

## 6. Claims Register (the evidence gate)

The standard in one table. A claim may be written in docs/marketing **only** when
its status is `verified`. Update this table in the same change that adds the
evidence.

| Claim | Status | Evidence / gate |
|---|---|---|
| Zero warnings, MSVC `/W4` | ✅ verified | built on this machine |
| Correct HOLD / recovery / chaining behavior | ✅ verified | demo run output |
| SHA-256 correct | ✅ verified | FIPS `abc` + empty vectors |
| Nominal solve matches `avenc run` | ✅ verified | 1429 / 32676 match |
| 4 independent backends bit-identical on 260k adversarial inputs | ✅ verified | `benchmarks/RESULTS.md` |
| Arithmetic correctly-rounded vs 50-digit oracle (~2 ULP) | ✅ verified | `benchmarks/RESULTS.md` |
| Generated C as fast as hand-written C++ (no codegen overhead) | ✅ verified | 5.49 vs 5.67 ns/solve (LTO), `benchmarks/RESULTS.md` |
| Compiled vs interpreted speedup | ✅ verified | ~133× (5.5 ns vs 729 ns/solve) |
| Optimizer agrees with HiGHS / reference solvers | ✅ verified | 2,958/2,958, `verify/OPTIMIZER_PROOF.md` |
| Compute-kernel throughput | ✅ verified | ~5.5 ns/solve compiled (amortized), one machine/compiler |
| Deterministic across compilers (x86-64) | 🟡 partial | MSVC + gcc 14.2 + clang 19.1 bit-identical on 260,085 rows; non-x86 / non-Windows still pending |
| `avenc emit-c --aeric-shim` generator | ✅ verified | 6 tests in `tests/toolchain_aeric_shim.rs` |
| AERIC Level-1 daemon (config + 50 Hz loop + TCP + ledger-to-disk) | ✅ verified | `aeric_daemon.c`; peer demo, 120 ticks |
| Deadline-miss / disconnect → HOLD (recorded) | ✅ verified | daemon demo: 56 live / 64 hold |
| Daemon ledger chain + HMAC seal independently verifiable | ✅ verified | `verify_ledger.py`: chain+seal PASS, tamper detected, wrong key rejected |
| Daemon jitter (50 Hz) | 🟡 partial | ~17 ms max across transitions, ~1.7 ms headless; p50/p99 over 10⁵ ticks pending |
| Worst-case tick latency (RT, sub-ms) | ⬜ unverified | needs SCHED_FIFO/busy-spin (Level 2/3) |
| Clean under gcc/clang `-Wpedantic` | ✅ verified | gcc 14.2 + clang 19.1, zero diagnostics (portable `AVEN_EXPORT`) |
| Forgery-resistant seal (AERIC) | ✅ verified | HMAC-SHA256 keyed; matches Python `hmac` ref + Rust `hmac_sha256_hex` construction; key-dependent |
| Host verifies edge-signed ledger (JSONL interop) | ✅ verified | `avenc verify-ledger` accepts the daemon ledger (600 facts, all sigs valid); wrong key / tamper rejected |
| Stateful models (state fed back tick→tick) | ✅ verified | PI controller matches Python reference exactly; integral ramps; HOLD freezes & resumes (`pi_check.py`) |
| Stateful daemon end-to-end | ✅ verified | PI daemon (`n_state=1`) ledger passes `avenc verify-ledger`; stateless path unchanged |
| Safety envelope (clamp + rate-limit) | ✅ verified | `test_envelope` vs reference: ramps 51→1000, plateaus at ceiling, `clamped` flagged; stateless hash unchanged |
| Fault taxonomy (sensor/solver/deadline/transport) | ✅ verified | each class injected, named, and counted (`AERIC_FaultName`, `fault_counts`) |
| "MISRA-grade" | ⬜ **unfounded** | static-analysis track |
| SHM end-to-end cycle time | ⬜ unverified | measure on real loop (not guessed) |

---

## 7. Recommended near-term sequence

Ordered by leverage, not size. Each item ends with its evidence committed.

1. **Toolchain-A: `--aeric-shim` generator** — kills the only manual coupling;
   everything downstream benefits. *(M)*
2. **Timing harness** — replaces the "microsecond" guess with real p50/p99/max.
   Cheap, and it retires a currently-unfounded claim. *(S–M)*
3. **Security-HMAC + Verif-ledger** — makes the audit trail actually
   tamper-evident and host-verifiable. *(M+M)*
4. **Level 1 daemon** — config + timed loop + TCP transport + ledger-to-disk.
   This is the step that earns the word "backend." *(L)*
5. **Determinism/record-replay + portability CI** — lock in the determinism
   claim and multi-compiler cleanliness. *(M+M)*
6. **Level 2: statefulness first**, then the safety envelope, then SHM interop. *(L–XL)*

> Rule of thumb for any PR against this roadmap: it is not "done" until the
> relevant Claims Register row can move to ✅ — with the artifact that moves it.
