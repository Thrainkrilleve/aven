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
| One version string | `Cargo.toml` `version`, `avenc --version`, and each `brand-version` badge are one string. The binary reads `CARGO_PKG_VERSION`. | `printed_version_matches_cargo_toml_and_the_site_badge` in `tests/claims.rs` runs `avenc --version` and requires `Cargo.toml` and `web/public/{index,docs}.html` to use that string. |
| Safety-case wording | The report and the `safety-case` help line say the result is evidence about the model and not a certification. The report does not say CERTIFIABLE, SATISFIED, or PROVEN UNCONDITIONAL. Division by zero is marked not checked. | `safety_case_is_evidence_not_a_certification` is covered by `tests/safety_case_and_timing.rs`: the golden report, the CLI report, and `avenc --help` fail on those words and fail without that sentence. |
| Homepage figures | The key figures are 1414.9 µs and 664.3 solves/sec from `evaluations/flight_mesh/runs/2026-10-27-linux/avenc_bench.txt`. | `scripts/check_docs.py` fails if a homepage key figure's visible value is not the `data-figure` copied from its `data-log`. |
| OpenPLC stdin | `evaluations/tests/PLC-test/openplc_adapter.py` reads stdin through EOF. It does not invent `io_points` when the read is slow. | `cargo test --release --test test_suite test_openplc_external_adapter_and_stream_varmap` passes with the default thread pool. |
| Lint matches CI | `rust-toolchain.toml` pins channel 1.99.1 with rustfmt and clippy. The GitHub and Forgejo workflows install that file before `cargo` runs. The sources match that rustfmt, and the 1.99 clippy findings are fixed. Edition 2024 still needs rustc 1.85 or newer. | `cargo fmt --check` and `cargo clippy --all-targets -- -D warnings` exit 0 on that pin. `scripts/check_docs.py` runs the format check and fails if a linter workflow stops reading `rust-toolchain.toml`, or if `docs/proof/lint-toolchain-1.99.1.txt` does not record both exits for this channel. |
| Hash chain, not a Merkle tree | `README.md`, `STANDARDS.md`, and `web/public/` call the ledger a SHA-256 hash chain. | `scripts/check_docs.py` fails if the README, the site, or `STANDARDS.md` says Merkle. |

## Build next

One feature, then the runtime that wraps it.

1. **Derived C.** For a `relation` whose equations are already assignments, `avenc emit-c` emits C from that relation. Start with `examples/server_capacity.aven`: `WorkerScaling` is the source. The test compiles the C and compares its printed numbers to `avenc run`. Delete the hand-written `forward` copy of those equations once the numbers match. Do not add a second model until this one is green.
2. **Generated tick loop.** `avenc emit-c --aeric-shim` for that model, then `examples/pi_controller.aven`. The test builds the generated files, checks HOLD on a non-finite input, checks recovery on the next clean ticks, and runs `avenc verify-ledger` on the C ledger. The test rebuilds the shim. A hand-edited shim is a failure.

## Suggestions

- Keep the public pages to the README, `docs/GETTING_STARTED.md`, the site, and this file. The wiki either restates a tested sentence or it is marked historical.
- Treat the private Forgejo branch as a lab. The shared-memory round trip there showed that the C side can read the Rust ring layout. It also showed `aeric_shm_demo.c` does not build with `-Wall -Wextra -Wpedantic -Werror` as committed, and that the ring publishes the sequence number before the length. Leave it private until a test in this repository owns it.
- Solver uniqueness is still the deep limit. Newton finds a root near the start. Say that next to any solve, and add a test only when a model has more than one root and the compiler reports which one it took. Do not paper over it with a safety-case sentence.

## Recorded

A line is recorded only when a command in this repository exited 0 and the transcript is in `docs/proof/`.

On channel 1.99.1, `docs/proof/lint-toolchain-1.99.1.txt`:

- `cargo fmt --check`
- `cargo clippy --all-targets -- -D warnings`
- `cargo test --release` (152 passed, 0 failed). Full output: `docs/proof/cargo-test-release-1.99.1.txt`
- `python3 scripts/check_docs.py` (45 examples compiled, 2 documented outputs compared, 0 problems). Output: `docs/proof/check-docs-1.99.1.txt`

On channel 1.99.1, `docs/proof/remaining-fixes-1.99.1.txt`:

- `cargo test --release` (154 passed, 0 failed), including the version-string test, the safety-case wording test, and `test_openplc_external_adapter_and_stream_varmap`. Output: `docs/proof/cargo-test-release-remaining-fixes.txt`
- `python3 scripts/check_docs.py` again, now also checking the homepage figures and the Merkle wording. Output: `docs/proof/check-docs-remaining-fixes.txt`
- 32 parallel runs of `openplc_adapter.py`, each given `io_points` 30, each returning 30. Output: `docs/proof/openplc-stdin-parallel.txt`

## Not recorded yet

- C emitted from the `relation` alone, with the hand-written `forward` copy deleted, compared with `avenc run`
- a shim the test rebuilds from `avenc emit-c`, which latches HOLD and whose ledger `avenc verify-ledger` accepts
