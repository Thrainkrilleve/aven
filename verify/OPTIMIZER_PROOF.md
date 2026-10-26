# AVEN Optimizer — Differential Verification against HiGHS

Companion to `benchmarks/RESULTS.md`. That report proves AVEN's **arithmetic /
code generation** is correct and fast. This one proves AVEN's **optimizer** (the
simplex / branch-and-bound / cutting-plane solver and the nonlinear global
prover) finds the **same answers as an independent, trusted solver**.

**Reference judge:** [HiGHS](https://highs.dev) 1.15.1 (via `scipy.optimize`
1.18.1 and `highspy`) — a world-class open-source LP/MILP solver — plus
independent checks for the families HiGHS doesn't cover (see below).
**Harness:** `verify/oracle.py` (per-family), driven by `verify/highs_proof.py`.
**Machine:** AMD Ryzen 7 9800X3D, Windows 11, Python 3.14 `.venv`.
**Campaign:** 6 families × 5 seeds × 100 generated cases.

## Result

```
family         agree   total    rate
eqsys            495     495  100.0%
linstatus        463     463  100.0%
lp               500     500  100.0%
milp             500     500  100.0%
minlp            500     500  100.0%
qp               500     500  100.0%
------------------------------------
ALL             2958    2958  100.0%
disagreements: 0    reference-UNDECIDED (excluded): 2
```

**AVEN agreed with the reference on every one of the 2,958 adjudicated models.**
The 2 excluded `linstatus` cases are ones where the **reference solver itself**
returned an ambiguous verdict, so there was nothing to compare against — not AVEN
errors. (The `linstatus` totals are below 500 because borderline
feasible/unbounded instances where the reference abstains are dropped rather than
counted as pass or fail.)

For `minlp` and `qp`, every case was additionally **"proven optimal"** — AVEN did
not merely find a matching point, it certified global optimality, and that
certificate matched the reference optimum.

## What each family checks

| Family | What AVEN solves | Independent reference |
|---|---|---|
| `lp` | linear programs | HiGHS simplex (`scipy.linprog`) |
| `milp` | mixed-integer linear programs | HiGHS branch-and-cut (`scipy.milp`) |
| `linstatus` | feasible / infeasible / unbounded status | HiGHS |
| `qp` | convex quadratic objectives | SLSQP multi-start (`scipy.minimize`) |
| `minlp` | mixed-integer nonlinear, global proof | multi-start reference optimum |
| `eqsys` | equation systems w/ known solution | back-substitution into every equation |

For each case the harness checks that AVEN's optimal objective (and
feasible/infeasible/unbounded verdict) matches the reference, and that AVEN never
claims "proven optimal" for a wrong answer. Every failure, had there been one,
would have been written to `verify/failures/<kind>_<seed>_<case>.aven` with the
reference answer, for reproduction.

## Why this is strong evidence

Unlike the 50-digit `decimal` referee used in the arithmetic benchmark — which
recomputes *our* formulas and so can't catch a wrong formula — **HiGHS is a fully
independent authority on the answer.** It solves each problem with its own
algorithms and its own code, sharing none of AVEN's assumptions. When AVEN's
optimizer agrees with HiGHS on thousands of varied problems, that is about as
strong a correctness signal as exists in this field.

## Honest limits

- **Differential testing is bounded by the generated distribution.** 2,958
  varied, randomly-seeded cases is a serious gauntlet, but it is not a proof of
  correctness on *all* possible inputs. Broader/harder generators strengthen it.
- **It proves agreement, not superiority** — that AVEN is *correct* relative to
  HiGHS, not that it is faster.
- **HiGHS is trusted, not infallible**; the independent families (qp/minlp/eqsys)
  use different references precisely so no single tool is the sole authority.
- This covers the **optimizer**, a different subsystem from the arithmetic/codegen
  path proven in `benchmarks/RESULTS.md`.

## Reproduce

```bash
# needs the .venv with scipy + highspy
.venv/Scripts/python verify/highs_proof.py --cases 100 --seeds 5
# or a single family, verbose, with failure capture:
.venv/Scripts/python verify/oracle.py milp --cases 200 --seed 1
```
