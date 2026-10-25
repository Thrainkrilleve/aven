#!/usr/bin/env python3
"""verify/oracle.py — Differential testing of avenc against reference solvers.

Generates random models, solves each with `avenc run --json` and with a reference
solver, and classifies every disagreement. Reference solvers:

  milp / lp : scipy.optimize.milp (HiGHS branch-and-cut / simplex)
  qp        : scipy.optimize.minimize (SLSQP, multi-start) on a convex objective
  eqsys     : equation systems built around a known solution; Aven's answer is checked
              by substituting it back into every equation (no reference solver needed)

Usage:
  .venv/Scripts/python verify/oracle.py milp --cases 200 --seed 1
  .venv/Scripts/python verify/oracle.py all --cases 100

Failing models are written to verify/failures/<kind>_<seed>_<case>.aven with the
reference answer in a header comment, so every failure is reproducible.
"""

import argparse
import json
import math
import os
import random
import subprocess
import sys
import tempfile
from pathlib import Path

import numpy as np
from scipy.optimize import Bounds, LinearConstraint, milp, minimize

ROOT = Path(__file__).resolve().parent.parent
AVENC = os.environ.get(
    "AVENC_BIN",
    str(ROOT / "target" / "release" / ("avenc.exe" if os.name == "nt" else "avenc")),
)
FAIL_DIR = ROOT / "verify" / "failures"
TIMEOUT_S = 30


def run_avenc(src):
    """Returns (status, env, detail). status: settled|underdetermined|error|violation|timeout|crash."""
    with tempfile.NamedTemporaryFile("w", suffix=".aven", delete=False) as f:
        f.write(src)
        path = f.name
    try:
        p = subprocess.run(
            [AVENC, "run", path, "--json"], capture_output=True, text=True, timeout=TIMEOUT_S
        )
    except subprocess.TimeoutExpired:
        return "timeout", {}, f"no answer within {TIMEOUT_S}s"
    finally:
        os.unlink(path)
    if p.returncode not in (0, 1, 2, 3):
        return "crash", {}, f"exit {p.returncode}: {p.stderr.strip()[-300:]}"
    out = p.stdout.strip().splitlines()
    if p.returncode == 2:
        return "violation", {}, p.stderr.strip()
    if p.returncode == 1 and not (p.stdout.strip().splitlines() or [""])[-1].startswith("{"):
        return "error", {}, p.stderr.strip()
    if out and out[-1].startswith("{"):
        doc = json.loads(out[-1])
        env = doc["env"]
        env["__optimality"] = doc.get("optimality")
        return doc["status"], env, p.stderr.strip()
    return "error", {}, (p.stderr.strip() or p.stdout.strip())[-300:]


def linear_expr(coeffs, names):
    terms = []
    for c, n in zip(coeffs, names):
        if c == 0:
            continue
        terms.append(f"{c} * {n}")
    return " + ".join(terms) if terms else "0"


def record_failure(kind, seed, case, src, reference, verdict, detail):
    FAIL_DIR.mkdir(parents=True, exist_ok=True)
    header = (
        f"// oracle verdict: {verdict}\n"
        f"// reference: {reference}\n"
        f"// avenc: {detail}\n"
    )
    (FAIL_DIR / f"{kind}_{seed}_{case}.aven").write_text(header + src)


# ---------------------------------------------------------------------------
# Mixed-integer and continuous linear programs
# ---------------------------------------------------------------------------

def gen_linear(rng, integer):
    n = rng.randint(1, 4)
    m = rng.randint(1, 4)
    names = [f"x{j}" for j in range(n)]
    lb = [rng.randint(0, 3) for _ in range(n)]
    ub = [lb[j] + rng.randint(3, 20) for j in range(n)]
    # A feasible point guarantees the problem is not infeasible by construction
    x0 = [rng.randint(lb[j], ub[j]) for j in range(n)]
    rows = []
    for _ in range(m):
        a = [rng.randint(-4, 9) for _ in range(n)]
        if all(v == 0 for v in a):
            a[0] = 1
        val = sum(ai * xi for ai, xi in zip(a, x0))
        if rng.random() < 0.5:
            rows.append((a, ">=", val - rng.randint(0, 6)))
        else:
            rows.append((a, "<=", val + rng.randint(0, 6)))
    sense = rng.choice(["minimize", "maximize"])
    c = [rng.randint(1, 20) * rng.choice([1, 1, -1]) for _ in range(n)]

    lines = [f"relation Lp {{"]
    for nm in names:
        if integer:
            lines.append(f"    int {nm}")
    for i, (a, op, b) in enumerate(rows):
        lines.append(f"    r{i} = {linear_expr(a, names)}")
        lines.append(f"    r{i} {op} {b}")
    lines.append(f"    obj = {linear_expr(c, names)}")
    for j, nm in enumerate(names):
        lines.append(f"    {nm} >= {lb[j]}")
        lines.append(f"    {nm} <= {ub[j]}")
    lines.append("}")
    lines.append("solve Lp with {")
    lines.append(f"    {sense}(obj)")
    lines.append("}")
    src = "\n".join(lines) + "\n"

    # Reference: scipy.optimize.milp (HiGHS)
    sign = 1 if sense == "minimize" else -1
    A = np.array([r[0] for r in rows], dtype=float)
    lo = np.array([r[2] if r[1] == ">=" else -np.inf for r in rows], dtype=float)
    hi = np.array([r[2] if r[1] == "<=" else np.inf for r in rows], dtype=float)
    res = milp(
        c=sign * np.array(c, dtype=float),
        constraints=[LinearConstraint(A, lo, hi)],
        integrality=np.ones(n) if integer else np.zeros(n),
        bounds=Bounds(np.array(lb, float), np.array(ub, float)),
    )
    assert res.success, f"reference failed on a feasible-by-construction model: {res.message}"
    ref_obj = sign * res.fun

    def check(env):
        """Independent feasibility and objective check of Aven's answer."""
        try:
            x = [env[nm] for nm in names]
        except KeyError as e:
            return None, f"missing variable {e}"
        tol = 1e-6
        for j in range(n):
            if x[j] < lb[j] - tol or x[j] > ub[j] + tol:
                return None, f"{names[j]}={x[j]} outside [{lb[j]}, {ub[j]}]"
            if integer and abs(x[j] - round(x[j])) > 1e-6:
                return None, f"{names[j]}={x[j]} is not an integer"
        for i, (a, op, b) in enumerate(rows):
            v = sum(ai * xi for ai, xi in zip(a, x))
            if (op == ">=" and v < b - 1e-6 * (1 + abs(b))) or (op == "<=" and v > b + 1e-6 * (1 + abs(b))):
                return None, f"row {a} {op} {b} violated (value {v})"
            # The reported r_i must agree with its own equation r_i = a·x
            if f"r{i}" in env and abs(env[f"r{i}"] - v) > 1e-9 * (1 + abs(v)):
                return None, f"reported r{i}={env[f'r{i}']} but a·x={v} (self-contradicting answer)"
        obj = sum(ci * xi for ci, xi in zip(c, x))
        if "obj" in env and abs(env["obj"] - obj) > 1e-9 * (1 + abs(obj)):
            return None, f"reported obj={env['obj']} but c·x={obj} (self-contradicting answer)"
        return obj, None

    return src, sense, ref_obj, list(res.x), check


def judge_optimum(sense, ref_obj, aven_obj):
    tol = 1e-6 * (1 + abs(ref_obj))
    if abs(aven_obj - ref_obj) <= tol:
        return "ok"
    better = aven_obj < ref_obj if sense == "minimize" else aven_obj > ref_obj
    return "BEATS_REFERENCE" if better else "SUBOPTIMAL"


def run_linear(args, integer):
    kind = "milp" if integer else "lp"
    rng = random.Random(args.seed)
    tally = {}
    for case in range(args.cases):
        src, sense, ref_obj, ref_x, check = gen_linear(rng, integer)
        status, env, detail = run_avenc(src)
        if status != "settled":
            verdict = f"NO_ANSWER({status})"
            info = detail
        else:
            aven_obj, err = check(env)
            if err:
                verdict, info = "INFEASIBLE_ANSWER", err
            else:
                verdict = judge_optimum(sense, ref_obj, aven_obj)
                info = f"aven obj={aven_obj}"
        tally[verdict] = tally.get(verdict, 0) + 1
        if verdict != "ok":
            record_failure(kind, args.seed, case, src, f"{sense} obj={ref_obj} at x={ref_x}", verdict, info)
            if args.verbose:
                print(f"[{kind} case {case}] {verdict}: ref={ref_obj} {info}")
    return tally


# ---------------------------------------------------------------------------
# Status agreement: infeasible / unbounded / optimal on unrestricted linear models
# ---------------------------------------------------------------------------

def run_linstatus(args):
    """Larger linear models that may be infeasible or unbounded. Aven must report the same
    status as HiGHS, and the same optimum when one exists."""
    rng = random.Random(args.seed)
    tally = {}
    for case in range(args.cases):
        n = rng.randint(2, 8)
        m = rng.randint(1, 8)
        integer = rng.random() < 0.4
        names = [f"x{j}" for j in range(n)]
        lb = [rng.choice([None, rng.randint(-5, 3)]) for _ in range(n)]
        ub = [None if l is None and rng.random() < 0.5 else (l or 0) + rng.randint(2, 15) for l in lb]
        ub = [rng.choice([u, None]) for u in ub]
        rows = []
        for _ in range(m):
            a = [rng.randint(-6, 6) for _ in range(n)]
            if all(v == 0 for v in a):
                a[rng.randrange(n)] = 1
            rows.append((a, rng.choice(["<=", ">=", "=="]), rng.randint(-20, 40)))
        c = [rng.randint(-10, 10) for _ in range(n)]
        sense = rng.choice(["minimize", "maximize"])

        lines = ["relation S {"]
        for nm in names:
            if integer:
                lines.append(f"    int {nm}")
        for i, (a, op, b) in enumerate(rows):
            lines.append(f"    r{i} = {linear_expr(a, names)}")
            lines.append(f"    r{i} {op} {b}")
        lines.append(f"    obj = {linear_expr(c, names) if any(c) else '0 * x0'}")
        for j, nm in enumerate(names):
            if lb[j] is not None:
                lines.append(f"    {nm} >= {lb[j]}")
            if ub[j] is not None:
                lines.append(f"    {nm} <= {ub[j]}")
        lines += ["}", "solve S with {", f"    {sense}(obj)", "}"]
        src = "\n".join(lines) + "\n"

        # Aven treats integer variables without a lower bound as non-negative counts
        lbs = [(0 if integer and l is None else (l if l is not None else -np.inf)) for l in lb]
        ubs = [u if u is not None else np.inf for u in ub]
        sign = 1 if sense == "minimize" else -1
        A = np.array([r[0] for r in rows], dtype=float)
        lo = np.array([r[2] if r[1] in (">=", "==") else -np.inf for r in rows], dtype=float)
        hi = np.array([r[2] if r[1] in ("<=", "==") else np.inf for r in rows], dtype=float)
        res = milp(
            c=sign * np.array(c, dtype=float),
            constraints=[LinearConstraint(A, lo, hi)],
            integrality=np.ones(n) if integer else np.zeros(n),
            bounds=Bounds(np.array(lbs, float), np.array(ubs, float)),
        )
        # scipy milp status: 0 optimal, 2 infeasible, 3 unbounded
        ref = {0: "optimal", 2: "infeasible", 3: "unbounded"}.get(res.status, f"status{res.status}")
        if ref.startswith("status"):
            continue  # reference itself did not settle; not a usable case

        status, env, detail = run_avenc(src)
        # A declared variable that appears in no row and no objective term is genuinely free;
        # avenc leaves it unsettled, and it has coefficient 0 everywhere, so treat it as 0.
        used = {f"x{j}" for i, (a, _, _) in enumerate(rows) for j in range(n) if a[j]} | {f"x{j}" for j in range(n) if c[j]}
        # avenc hitting its branch-and-bound node limit is an honest "couldn't solve", not a
        # proof of infeasibility: classify it separately from a wrong answer.
        node_limit = "node limit" in detail
        if status == "settled":
            got = "optimal"
        elif node_limit:
            got = "node_limit"
        elif any(t in detail for t in ("No feasible solution", "Inconsistency in", "Constraint violation in")):
            # Specific contradictions found while propagating equalities also prove there is
            # no solution
            got = "infeasible"
        elif "unbounded" in detail:
            got = "unbounded"
        else:
            got = f"other({status})"

        if got == "node_limit":
            verdict, info = "NODE_LIMIT", f"ref={ref}; {detail[-120:]}"
        elif got != ref:
            verdict, info = f"STATUS_{got.upper()}_EXPECTED_{ref.upper()}", detail[-200:]
        elif got == "optimal":
            x = [env[nm] if nm in env else (0.0 if nm not in used else float("nan")) for nm in names]
            obj = sum(ci * xi for ci, xi in zip(c, x))
            ref_obj = sign * res.fun
            ok_rows = all(
                (op == "<=" and sum(ai * xi for ai, xi in zip(a, x)) <= b + 1e-6 * (1 + abs(b)))
                or (op == ">=" and sum(ai * xi for ai, xi in zip(a, x)) >= b - 1e-6 * (1 + abs(b)))
                or (op == "==" and abs(sum(ai * xi for ai, xi in zip(a, x)) - b) <= 1e-6 * (1 + abs(b)))
                for a, op, b in rows
            )
            if not ok_rows:
                verdict, info = "INFEASIBLE_ANSWER", f"x={x}"
            else:
                verdict = judge_optimum(sense, ref_obj, obj)
                info = f"aven obj={obj} ref={ref_obj}"
        else:
            verdict, info = "ok", ""
        tally[verdict] = tally.get(verdict, 0) + 1
        if verdict != "ok":
            record_failure("linstatus", args.seed, case, src, f"{ref} {getattr(res, 'fun', None)}", verdict, info)
            if args.verbose:
                print(f"[linstatus case {case}] {verdict}: {info}")
    return tally


# ---------------------------------------------------------------------------
# Nonconvex integer models solved exactly by enumeration
# ---------------------------------------------------------------------------

def run_minlp(args):
    """Products and squares of integer variables over small boxes. The exact optimum comes
    from enumerating every integer point, so this measures how often Aven's (unproven)
    nonlinear branch-and-bound finds the true global optimum."""
    import itertools

    rng = random.Random(args.seed)
    tally = {}
    for case in range(args.cases):
        n = rng.randint(2, 3)
        names = [f"k{j}" for j in range(n)]
        ub = [rng.randint(3, 12) for _ in range(n)]
        lb = [rng.randint(0, 1) for _ in range(n)]
        i, j = rng.sample(range(n), 2)
        cap_coef = round(rng.uniform(5, 20), 1)
        need = round(rng.uniform(20, 0.6 * cap_coef * ub[i] * ub[j]), 1)
        fixed = [rng.randint(100, 3000) for _ in range(n)]
        unit = round(rng.uniform(50, 700), 1)
        quad = round(rng.uniform(0, 30), 1)
        cost_terms = " + ".join(f"{fixed[t]} * {names[t]}" for t in range(n))
        lines = ["relation M {"] + [f"    int {nm}" for nm in names]
        lines += [
            f"    capacity = {names[i]} * {names[j]} * {cap_coef}",
            f"    cost = {cost_terms} + {unit} * {names[i]} * {names[j]} + {quad} * {names[j]} ^ 2",
            f"    capacity >= {need}",
        ]
        for t, nm in enumerate(names):
            lines.append(f"    {nm} >= {lb[t]}")
            lines.append(f"    {nm} <= {ub[t]}")
        lines += ["}", "solve M with {", "    minimize(cost)", "}"]
        src = "\n".join(lines) + "\n"

        def cost_of(k):
            return sum(fixed[t] * k[t] for t in range(n)) + unit * k[i] * k[j] + quad * k[j] ** 2

        best = None
        for k in itertools.product(*[range(lb[t], ub[t] + 1) for t in range(n)]):
            if k[i] * k[j] * cap_coef >= need - 1e-9:
                cst = cost_of(k)
                if best is None or cst < best[0] - 1e-9:
                    best = (cst, k)
        if best is None:
            continue

        status, env, detail = run_avenc(src)
        if status != "settled":
            verdict, info = f"NO_ANSWER({status})", detail[-200:]
        else:
            k = [env.get(nm, float("nan")) for nm in names]
            if any(abs(v - round(v)) > 1e-6 for v in k) or k[i] * k[j] * cap_coef < need - 1e-6:
                verdict, info = "INFEASIBLE_ANSWER", f"k={k}"
            else:
                got = cost_of([round(v) for v in k])
                optimal = got <= best[0] + 1e-6 * (1 + abs(best[0]))
                proven = env.get("__optimality") == "proven"
                verdict = "ok" if optimal else ("FALSE_PROOF" if proven else "SUBOPTIMAL")
                if optimal:
                    tally["proven optimal" if proven else "unproven"] = tally.get("proven optimal" if proven else "unproven", 0) + 1
                info = f"aven k={k} cost={got} vs optimum {best} ({env.get('__optimality')})"
        tally[verdict] = tally.get(verdict, 0) + 1
        if verdict != "ok":
            record_failure("minlp", args.seed, case, src, f"optimum {best}", verdict, info)
            if args.verbose:
                print(f"[minlp case {case}] {verdict}: {info}")
    return tally


# ---------------------------------------------------------------------------
# Smooth (convex quadratic) objectives with box bounds
# ---------------------------------------------------------------------------

def run_qp(args):
    rng = random.Random(args.seed)
    tally = {}
    for case in range(args.cases):
        n = rng.randint(1, 3)
        names = [f"x{j}" for j in range(n)]
        centers = [round(rng.uniform(-8, 8), 2) for _ in range(n)]
        weights = [round(rng.uniform(0.5, 5), 2) for _ in range(n)]
        lb = [round(rng.uniform(-10, 0), 1) for _ in range(n)]
        ub = [round(lb[j] + rng.uniform(2, 15), 1) for j in range(n)]
        terms = " + ".join(
            f"{w} * ({nm} - ({cc})) * ({nm} - ({cc}))" for w, nm, cc in zip(weights, names, centers)
        )
        lines = ["relation Qp {", f"    cost = {terms} + 1"]
        for j, nm in enumerate(names):
            lines.append(f"    {nm} >= {lb[j]}")
            lines.append(f"    {nm} <= {ub[j]}")
        lines += ["}", "solve Qp with {", "    minimize(cost)", "}"]
        src = "\n".join(lines) + "\n"

        # Separable convex objective: the exact optimum clips each center into its box.
        x_star = [min(max(cc, lb[j]), ub[j]) for j, cc in enumerate(centers)]
        f = lambda x: sum(w * (xi - cc) ** 2 for w, xi, cc in zip(weights, x, centers)) + 1
        ref = minimize(f, [0.5 * (l + u) for l, u in zip(lb, ub)], bounds=list(zip(lb, ub)), method="SLSQP")
        ref_obj = min(f(x_star), ref.fun)

        status, env, detail = run_avenc(src)
        if status != "settled":
            verdict, info = f"NO_ANSWER({status})", detail
        else:
            try:
                x = [env[nm] for nm in names]
            except KeyError as e:
                x = None
                verdict, info = "INFEASIBLE_ANSWER", f"missing {e}"
            if x is not None:
                if any(x[j] < lb[j] - 1e-6 or x[j] > ub[j] + 1e-6 for j in range(n)):
                    verdict, info = "INFEASIBLE_ANSWER", f"x={x} outside box"
                else:
                    aven_obj = f(x)
                    optimal = aven_obj <= ref_obj + 1e-4 * (1 + abs(ref_obj))
                    proven = env.get("__optimality") == "proven"
                    verdict = "ok" if optimal else ("FALSE_PROOF" if proven else "SUBOPTIMAL")
                    if optimal:
                        key = "proven optimal" if proven else "unproven"
                        tally[key] = tally.get(key, 0) + 1
                    info = f"aven x={x} obj={aven_obj}"
        tally[verdict] = tally.get(verdict, 0) + 1
        if verdict != "ok":
            record_failure("qp", args.seed, case, src, f"min obj={ref_obj} at x={x_star}", verdict, info)
            if args.verbose:
                print(f"[qp case {case}] {verdict}: ref={ref_obj} {info}")
    return tally


# ---------------------------------------------------------------------------
# Nonlinear equation systems with a known solution (omnidirectional solving)
# ---------------------------------------------------------------------------

UNARY = [
    ("sqrt", lambda v: math.sqrt(v), lambda v: v > 0.05),
    ("exp", lambda v: math.exp(v), lambda v: -3 < v < 3),
    ("ln", lambda v: math.log(v), lambda v: v > 0.05),
    ("sin", lambda v: math.sin(v), lambda v: -1.2 < v < 1.2),
]


def run_eqsys(args):
    """Builds a chain of equations around known values, then hides an input and binds an
    output so Aven must solve backward. Any answer is accepted if it satisfies every equation."""
    rng = random.Random(args.seed)
    tally = {}
    for case in range(args.cases):
        n_in = rng.randint(1, 3)
        vals = {f"a{i}": round(rng.uniform(0.3, 2.5), 3) for i in range(n_in)}
        eqs = []
        for k in range(rng.randint(1, 4)):
            pool = list(vals.keys())
            u, w = rng.choice(pool), rng.choice(pool)
            kind = rng.choice(["mul", "add", "pow", "fn"])
            name = f"y{k}"
            if kind == "mul":
                c = round(rng.uniform(0.5, 3), 2)
                expr, v = f"{c} * {u} * {w}", c * vals[u] * vals[w]
            elif kind == "add":
                c = round(rng.uniform(-2, 2), 2)
                expr, v = f"{u} + {w} + ({c})", vals[u] + vals[w] + c
            elif kind == "pow":
                p = rng.choice([2, 3])
                expr, v = f"{u} ^ {p} + {w}", vals[u] ** p + vals[w]
            else:
                fn, fv, ok = rng.choice(UNARY)
                if not ok(vals[u]):
                    continue
                expr, v = f"{fn}({u}) + {w}", fv(vals[u]) + vals[w]
            eqs.append((name, expr))
            vals[name] = v
        if not eqs:
            continue

        all_vars = sorted(vals)
        outputs = [e[0] for e in eqs]
        # Hide an input the bound output actually depends on; otherwise the model really is
        # under-determined and no solver could answer it.
        deps = {}
        for nm, ex in eqs:
            direct = {t for t in vals if t in ex.replace("(", " ").replace(")", " ").split()}
            deps[nm] = set().union(*[deps.get(d, {d}) for d in direct]) if direct else set()
        reachable = sorted(d for d in deps[outputs[-1]] if d.startswith("a"))
        if not reachable:
            continue
        hidden = rng.choice(reachable)
        # Bind every input except the hidden one, plus the last output so the system is square
        bound = {k: vals[k] for k in all_vars if k.startswith("a") and k != hidden}
        bound[outputs[-1]] = vals[outputs[-1]]
        # Only meaningful if the hidden input reaches the bound output
        lines = ["relation Sys {"] + [f"    {nm} = {ex}" for nm, ex in eqs] + ["}"]
        lines += ["solve Sys with {"] + [f"    {k} = {repr(v)}" for k, v in bound.items()] + ["}"]
        src = "\n".join(lines) + "\n"

        status, env, detail = run_avenc(src)
        if status != "settled":
            verdict, info = f"NO_ANSWER({status})", detail
        else:
            # Substitute Aven's answer back into every equation
            ns = {**env, "sqrt": math.sqrt, "exp": math.exp, "ln": math.log, "sin": math.sin}
            worst = 0.0
            bad = None
            for nm, ex in eqs:
                try:
                    lhs = env[nm]
                    rhs = eval(ex.replace("^", "**"), {"__builtins__": {}}, ns)
                except Exception as e:  # noqa: BLE001 - report any evaluation failure
                    bad = f"{nm}: {e}"
                    break
                err = abs(lhs - rhs) / (1 + abs(rhs))
                worst = max(worst, err)
            if bad:
                verdict, info = "INCONSISTENT_ANSWER", bad
            elif worst > 1e-6:
                verdict, info = "INCONSISTENT_ANSWER", f"max relative residual {worst:.3e}"
            else:
                verdict, info = "ok", ""
        tally[verdict] = tally.get(verdict, 0) + 1
        if verdict != "ok":
            record_failure("eqsys", args.seed, case, src, f"known solution {vals}", verdict, info)
            if args.verbose:
                print(f"[eqsys case {case}] {verdict}: {info}")
    return tally


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("kind", choices=["milp", "lp", "qp", "eqsys", "linstatus", "minlp", "all"])
    ap.add_argument("--cases", type=int, default=100)
    ap.add_argument("--seed", type=int, default=1)
    ap.add_argument("--verbose", action="store_true")
    args = ap.parse_args()
    if not Path(AVENC).exists():
        sys.exit(f"avenc binary not found at {AVENC}; run `cargo build --release` first")

    kinds = ["milp", "lp", "linstatus", "minlp", "qp", "eqsys"] if args.kind == "all" else [args.kind]
    any_fail = False
    for k in kinds:
        if k == "milp":
            tally = run_linear(args, integer=True)
        elif k == "lp":
            tally = run_linear(args, integer=False)
        elif k == "linstatus":
            tally = run_linstatus(args)
        elif k == "minlp":
            tally = run_minlp(args)
        elif k == "qp":
            tally = run_qp(args)
        else:
            tally = run_eqsys(args)
        notes = {k: tally.pop(k) for k in ("proven optimal", "unproven") if k in tally}
        total = sum(tally.values())
        ok = tally.get("ok", 0)
        any_fail |= ok != total
        summary = ", ".join(f"{v} {kk}" for kk, v in sorted(tally.items(), key=lambda t: -t[1]))
        extra = f" ({', '.join(f'{v} {kk}' for kk, v in notes.items())})" if notes else ""
        print(f"{k:9s} {ok}/{total} agree with reference - {summary}{extra}")
    sys.exit(1 if any_fail else 0)


if __name__ == "__main__":
    main()
