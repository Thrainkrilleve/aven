# Aven roadmap

**Theme:** provide the proof in the code, not with words.

The public tree is GitHub. Forgejo stays private. An experiment becomes a public claim only when a test in this repository fails if the claim is false.

## Goal

Replace one spreadsheet or Python sizing script with one `.aven` file. `avenc` and AERIC ship as one toolchain. The proof is this sequence, run by a test:

1. The equations exist once, as a `relation`.
2. `avenc check` accepts that file and rejects a unit that does not match.
3. `avenc run` prints a settled value the test names.
4. `avenc run --ledger` writes a chain, `avenc verify-ledger` accepts it, and a one-byte edit fails verification.
5. `avenc emit-c` compiles to a program that prints the same values as `avenc run` on the same inputs. The compiler derives that causal form from the `relation`. A second, hand-written `forward` copy is not the source.
6. The AERIC loop generated with that C ticks the function. A non-finite input or the wrong arity latches HOLD and publishes the fail-safe. A later clean tick publishes the solve again.

The site may show this. It may not show a number the repository cannot regenerate.

## Parked

Do not build these until a named machine and a named deadline ask for them.

- AERIC Level 3: `SCHED_FIFO`, worst-case execution time, MISRA, and a DO-178C or ISO 26262 traceability matrix.
- The unfinished Level 2 real-time items: a dedicated real-time thread, `mlockall`, and a measured shared-memory cycle.
- A shared-memory cycle as evidence that the solver is fast. Solver time is the emitted function, or one `avenc` solve with parsing left out. Loop time is a different measurement, and only after the payload is raw doubles in memory.
- A general-purpose language, and any new adapter, desktop shell, or live-data demo.

Level 1 and the finished Level 2 behavior stay. That is state carried tick to tick, clamp and rate limit, faults named sensor, solver, deadline, or transport, and HOLD under all of them.

## Rule

A sentence in the README, the site, the wiki, or a checkbox on this page is allowed only when a test or a script in this repository fails if the sentence is wrong. `scripts/check_docs.py` is that script for documented solves. A checkbox with no test is a sentence. Delete it.

When a sentence and a test disagree, change the sentence.

## Fixes

Do these before the new compiler work. Each one is done when the command in the last column fails on today's tree and passes after the change.

| Fix | What the tree does now | Proof |
| --- | --- | --- |
| One version string | `Cargo.toml` says `0.1.0`. `src/main.rs` `VERSION` says `0.6.0`. The site badge says v0.6.0. | A test runs `avenc`, reads the printed version, and requires `Cargo.toml` and the site badge to use that same string. |
| Safety-case wording | `src/smt.rs` emits `FORMALLY DISCHARGED / CERTIFIABLE` and `PROVEN UNCONDITIONAL`. The standards table marks objectives satisfied, including division-by-zero, on every report. `src/main.rs` describes the command as a DO-178C / ASIL-D audit. | `tests/safety_case_and_timing.rs` fails if the report contains `CERTIFIABLE`, `SATISFIED`, or `PROVEN UNCONDITIONAL`, and fails if the report lacks a sentence that the result is evidence about the model and not a certification. The same test covers the CLI help line. |
| Homepage figures | `web/public/index.html` states 0.56 ms and 1,720/s from a Windows IPC bench. The caption names the machine. Nothing in CI regenerates the figures. | The page quotes a log committed beside the command that produced it, or the figures are removed. `scripts/check_docs.py` fails if the page states a throughput the log does not contain. |
| OpenPLC stdin | `evaluations/tests/PLC-test/openplc_adapter.py` waits 150 ms for stdin, then invents `io_points`. Under the default test thread pool, `test_openplc_external_adapter_and_stream_varmap` has failed with 28 and with 32 against the required 30. Alone, it passes. | The adapter reads stdin to completion. `cargo test --release --test test_suite test_openplc_external_adapter_and_stream_varmap` passes with the default thread pool. |
| Lint matches CI | `.github/workflows/linter.yml` runs `cargo fmt --check` and `cargo clippy --all-targets -- -D warnings` on stable. Both fail on rustc 1.99.1. Edition 2024 also needs rustc 1.85 or newer; 1.83 cannot build the tree. | `rust-toolchain.toml` pins a compiler for which both commands pass. The workflow installs that pin. |
| Hash chain, not a Merkle tree | Public docs still say Merkle in places. The ledger is a linear SHA-256 chain. | `scripts/check_docs.py` fails if the README, the site, or `STANDARDS.md` says Merkle. |

## Build next

One feature, then the runtime that wraps it.

1. **Derived C.** For a `relation` whose equations are already assignments, `avenc emit-c` emits C from that relation. Start with `examples/server_capacity.aven`: `WorkerScaling` is the source. The test compiles the C and compares its printed numbers to `avenc run`. Delete the hand-written `forward` copy of those equations once the numbers match. Do not add a second model until this one is green.
2. **Generated tick loop.** `avenc emit-c --aeric-shim` for that model, then `examples/pi_controller.aven`. The test builds the generated files, checks HOLD on a non-finite input, checks recovery on the next clean ticks, and runs `avenc verify-ledger` on the C ledger. The test rebuilds the shim. A hand-edited shim is a failure.

## Suggestions

- Keep the public pages to the README, `docs/GETTING_STARTED.md`, the site, and this file. The wiki either restates a tested sentence or it is marked historical.
- Treat the private Forgejo branch as a lab. The shared-memory round trip there showed that the C side can read the Rust ring layout. It also showed `aeric_shm_demo.c` does not build with `-Wall -Wextra -Wpedantic -Werror` as committed, and that the ring publishes the sequence number before the length. Leave it private until a test in this repository owns it.
- Solver uniqueness is still the deep limit. Newton finds a root near the start. Say that next to any solve, and add a test only when a model has more than one root and the compiler reports which one it took. Do not paper over it with a safety-case sentence.

## Done

CI on the pinned toolchain runs all of these, and this page lists no capability they do not cover:

- `cargo test --release`
- `cargo fmt --check`
- `cargo clippy --all-targets -- -D warnings`
- `python3 scripts/check_docs.py`
- the derived-C comparison
- the generated AERIC HOLD and ledger check
