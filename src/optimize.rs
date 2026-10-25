// src/optimize.rs — Constrained optimization for `solve` blocks with inequalities,
// objectives and integer variables.
//
// Two engines, chosen per model:
//   * Linear models (every equation, constraint and the objective are affine in the
//     unknowns): exact two-phase simplex with Bland's rule, and branch-and-bound on LP
//     relaxations for integer variables. Optimality, infeasibility and unboundedness are
//     proven, not estimated.
//   * Nonlinear models: augmented Lagrangian outer loop with a spectral projected gradient
//     (SPG) inner solver over the variable box, run from several starting points, with
//     branch-and-bound on those relaxations for integer variables. Results are locally
//     optimal; for convex models that is the global optimum.
// Pure std, no external crates.

use crate::ast::*;
use crate::diff::{diff, eval_expr};
use std::collections::{BTreeSet, HashMap};

/// One unknown of the optimization problem.
#[derive(Debug, Clone)]
pub struct Var {
    pub name: String,
    pub lo: f64,
    pub hi: f64,
    pub integer: bool,
}

/// g(x) <= 0, with `strict` meaning g(x) < 0.
#[derive(Debug, Clone)]
pub struct Ineq {
    pub expr: Expr,
    pub strict: bool,
}

#[derive(Debug, Clone)]
pub struct Problem {
    pub vars: Vec<Var>,
    /// h(x) = 0
    pub eqs: Vec<Expr>,
    pub ineqs: Vec<Ineq>,
    /// Expression to minimize (already negated for maximize)
    pub objective: Option<Expr>,
    /// Values of every known variable
    pub env: HashMap<String, f64>,
}

#[derive(Debug, Clone)]
pub enum Outcome {
    /// `proven` is true when the result is a certified global optimum (linear engine with a
    /// completed branch-and-bound search).
    Optimal {
        x: Vec<f64>,
        objective: f64,
        proven: bool,
        method: String,
    },
    Infeasible(String),
    Unbounded(String),
    /// Could not decide (e.g. an unbounded relaxation whose integer feasibility is undecided
    /// without cutting planes). Reported honestly rather than as a wrong verdict.
    Undetermined(String),
}

/// Margin used to turn a strict inequality g < 0 into g <= -margin.
const STRICT_MARGIN: f64 = 1e-7;

impl Problem {
    fn env_with(&self, x: &[f64]) -> HashMap<String, f64> {
        let mut env = self.env.clone();
        for (v, xi) in self.vars.iter().zip(x) {
            env.insert(v.name.clone(), *xi);
        }
        env
    }

    fn unknown_names(&self) -> BTreeSet<String> {
        self.vars.iter().map(|v| v.name.clone()).collect()
    }

    /// Solves the problem with the appropriate engine.
    pub fn solve(&self) -> Outcome {
        if let Some(lp) = self.linearize() {
            return solve_milp(self, &lp);
        }
        // Nonlinear: substitute explicitly defined variables away so the engine works on a
        // smaller problem without the equality constraints that condition it badly.
        let (reduced, defs) = self.eliminate_definitions();
        match solve_nonlinear(&reduced) {
            Outcome::Optimal {
                x,
                objective,
                proven,
                method,
            } => {
                let mut env = reduced.env_with(&x);
                for (name, rhs) in &defs {
                    let v = eval_expr(rhs, &env).unwrap_or(f64::NAN);
                    env.insert(name.clone(), v);
                }
                let full: Vec<f64> = self.vars.iter().map(|v| env[&v.name]).collect();
                Outcome::Optimal {
                    x: full,
                    objective,
                    proven,
                    method,
                }
            }
            other => other,
        }
    }

    /// Eliminates every equation `v - rhs = 0` where `v` is a continuous unknown not used in
    /// `rhs`, by substituting `rhs` for `v` everywhere. The variable's bounds become
    /// inequalities. Returns the reduced problem and the definitions (each expressed in the
    /// remaining unknowns) in the order they must be evaluated.
    fn eliminate_definitions(&self) -> (Problem, Vec<(String, Expr)>) {
        let mut p = self.clone();
        let mut defs: Vec<(String, Expr)> = Vec::new();
        loop {
            let pick = p.eqs.iter().enumerate().find_map(|(i, e)| match e {
                Expr::Bin(BinaryOp::Sub, l, r) => match l.as_ref() {
                    Expr::Var(name)
                        if !r.vars().contains(name)
                            && p.vars.iter().any(|v| &v.name == name && !v.integer) =>
                    {
                        Some((i, name.clone(), (**r).clone()))
                    }
                    _ => None,
                },
                _ => None,
            });
            let Some((i, name, rhs)) = pick else {
                break;
            };
            p.eqs.remove(i);
            let j = p.vars.iter().position(|v| v.name == name).unwrap();
            let var = p.vars.remove(j);
            let sub = |e: &Expr| substitute(e, &name, &rhs);
            p.eqs = p.eqs.iter().map(sub).collect();
            for g in p.ineqs.iter_mut() {
                g.expr = sub(&g.expr);
            }
            p.objective = p.objective.as_ref().map(sub);
            for d in defs.iter_mut() {
                d.1 = sub(&d.1);
            }
            let minus = |a: Expr, b: Expr| Expr::Bin(BinaryOp::Sub, Box::new(a), Box::new(b));
            if var.lo.is_finite() {
                p.ineqs.push(Ineq {
                    expr: minus(Expr::Num(var.lo), rhs.clone()),
                    strict: false,
                });
            }
            if var.hi.is_finite() {
                p.ineqs.push(Ineq {
                    expr: minus(rhs.clone(), Expr::Num(var.hi)),
                    strict: false,
                });
            }
            defs.push((name, rhs));
        }
        (p, defs)
    }

    // -----------------------------------------------------------------------
    // Linear detection
    // -----------------------------------------------------------------------

    /// Returns the affine coefficients of every expression if the whole problem is linear in
    /// the unknowns, otherwise None.
    fn linearize(&self) -> Option<LinearForm> {
        let unknowns = self.unknown_names();
        let affine = |e: &Expr| -> Option<(Vec<f64>, f64)> {
            if has_nonlinear_call(e, &unknowns) {
                return None;
            }
            let mut coeffs = Vec::with_capacity(self.vars.len());
            for v in &self.vars {
                let d = diff(e, &v.name).ok()?;
                if d.vars().iter().any(|dv| unknowns.contains(dv)) {
                    return None;
                }
                let c = eval_expr(&d, &self.env).ok()?;
                if !c.is_finite() {
                    return None;
                }
                coeffs.push(c);
            }
            let zero = vec![0.0; self.vars.len()];
            let constant = eval_expr(e, &self.env_with(&zero)).ok()?;
            if !constant.is_finite() {
                return None;
            }
            Some((coeffs, constant))
        };

        let mut rows = Vec::new();
        for h in &self.eqs {
            let (a, k) = affine(h)?;
            rows.push(LinRow { a, sense: Sense::Eq, b: -k });
        }
        for g in &self.ineqs {
            let (a, k) = affine(&g.expr)?;
            let margin = if g.strict { STRICT_MARGIN } else { 0.0 };
            rows.push(LinRow { a, sense: Sense::Le, b: -k - margin });
        }
        let (c, c0) = match &self.objective {
            Some(o) => affine(o)?,
            None => (vec![0.0; self.vars.len()], 0.0),
        };
        Some(LinearForm { rows, c, c0 })
    }
}

/// Replaces every occurrence of variable `name` in `e` with `with`.
fn substitute(e: &Expr, name: &str, with: &Expr) -> Expr {
    match e {
        Expr::Var(v) if v == name => with.clone(),
        Expr::Num(_) | Expr::Var(_) => e.clone(),
        Expr::UnaryMinus(a) => Expr::UnaryMinus(Box::new(substitute(a, name, with))),
        Expr::Bin(op, l, r) => Expr::Bin(
            *op,
            Box::new(substitute(l, name, with)),
            Box::new(substitute(r, name, with)),
        ),
        Expr::Call(f, a) => Expr::Call(f.clone(), Box::new(substitute(a, name, with))),
    }
}

/// True if `e` applies a function (ceil, sqrt, ...) to an expression containing an unknown.
fn has_nonlinear_call(e: &Expr, unknowns: &BTreeSet<String>) -> bool {
    match e {
        Expr::Num(_) | Expr::Var(_) => false,
        Expr::UnaryMinus(inner) => has_nonlinear_call(inner, unknowns),
        Expr::Bin(_, l, r) => has_nonlinear_call(l, unknowns) || has_nonlinear_call(r, unknowns),
        Expr::Call(_, arg) => arg.vars().iter().any(|v| unknowns.contains(v)),
    }
}

// ===========================================================================
// Linear engine: two-phase simplex + branch-and-bound
// ===========================================================================

#[derive(Debug, Clone, Copy, PartialEq)]
enum Sense {
    Le,
    Ge,
    Eq,
}

#[derive(Debug, Clone)]
struct LinRow {
    a: Vec<f64>,
    sense: Sense,
    b: f64,
}

#[derive(Debug, Clone)]
struct LinearForm {
    rows: Vec<LinRow>,
    /// minimize c·x + c0
    c: Vec<f64>,
    c0: f64,
}

enum LpResult {
    Optimal(Vec<f64>, f64),
    Infeasible,
    Unbounded,
}

const LP_EPS: f64 = 1e-9;

/// How an original variable maps onto non-negative simplex columns.
enum ColMap {
    /// x = offset + y
    Shift { col: usize, offset: f64 },
    /// x = offset - y
    Reflect { col: usize, offset: f64 },
    /// x = y1 - y2
    Free { pos: usize, neg: usize },
}

/// Solves min c·x subject to rows and lo <= x <= hi.
fn solve_lp(lf: &LinearForm, lo: &[f64], hi: &[f64]) -> LpResult {
    let n = lf.c.len();
    for j in 0..n {
        if lo[j] > hi[j] + LP_EPS {
            return LpResult::Infeasible;
        }
    }

    // Map variables onto non-negative columns; finite two-sided bounds add a row.
    let mut maps = Vec::with_capacity(n);
    let mut ncol = 0;
    let mut extra_rows: Vec<(usize, f64)> = Vec::new(); // (column, upper limit on y)
    for j in 0..n {
        if lo[j].is_finite() {
            maps.push(ColMap::Shift { col: ncol, offset: lo[j] });
            if hi[j].is_finite() {
                extra_rows.push((ncol, hi[j] - lo[j]));
            }
            ncol += 1;
        } else if hi[j].is_finite() {
            maps.push(ColMap::Reflect { col: ncol, offset: hi[j] });
            ncol += 1;
        } else {
            maps.push(ColMap::Free { pos: ncol, neg: ncol + 1 });
            ncol += 2;
        }
    }

    // Express each row over the y columns
    let mut rows: Vec<(Vec<f64>, Sense, f64)> = Vec::new();
    for r in &lf.rows {
        let mut a = vec![0.0; ncol];
        let mut b = r.b;
        for j in 0..n {
            let aj = r.a[j];
            if aj == 0.0 {
                continue;
            }
            match maps[j] {
                ColMap::Shift { col, offset } => {
                    a[col] += aj;
                    b -= aj * offset;
                }
                ColMap::Reflect { col, offset } => {
                    a[col] -= aj;
                    b -= aj * offset;
                }
                ColMap::Free { pos, neg } => {
                    a[pos] += aj;
                    a[neg] -= aj;
                }
            }
        }
        rows.push((a, r.sense, b));
    }
    for (col, limit) in extra_rows {
        let mut a = vec![0.0; ncol];
        a[col] = 1.0;
        rows.push((a, Sense::Le, limit));
    }
    let mut cost = vec![0.0; ncol];
    let mut cost0 = lf.c0;
    for j in 0..n {
        let cj = lf.c[j];
        match maps[j] {
            ColMap::Shift { col, offset } => {
                cost[col] += cj;
                cost0 += cj * offset;
            }
            ColMap::Reflect { col, offset } => {
                cost[col] -= cj;
                cost0 += cj * offset;
            }
            ColMap::Free { pos, neg } => {
                cost[pos] += cj;
                cost[neg] -= cj;
            }
        }
    }

    // Normalize to b >= 0
    for row in rows.iter_mut() {
        if row.2 < 0.0 {
            for v in row.0.iter_mut() {
                *v = -*v;
            }
            row.2 = -row.2;
            row.1 = match row.1 {
                Sense::Le => Sense::Ge,
                Sense::Ge => Sense::Le,
                Sense::Eq => Sense::Eq,
            };
        }
    }

    // Columns: [structural | slack/surplus | artificial] then RHS
    let m = rows.len();
    let n_slack = rows.iter().filter(|r| r.1 != Sense::Eq).count();
    let n_art = rows.iter().filter(|r| r.1 != Sense::Le).count();
    let total = ncol + n_slack + n_art;
    let rhs = total;
    let mut t = vec![vec![0.0; total + 1]; m + 1];
    let mut basis = vec![0usize; m];
    let art_start = ncol + n_slack;
    let (mut s, mut a_idx) = (ncol, art_start);
    for (i, (a, sense, b)) in rows.iter().enumerate() {
        t[i][..ncol].copy_from_slice(a);
        t[i][rhs] = *b;
        match sense {
            Sense::Le => {
                t[i][s] = 1.0;
                basis[i] = s;
                s += 1;
            }
            Sense::Ge => {
                t[i][s] = -1.0;
                s += 1;
                t[i][a_idx] = 1.0;
                basis[i] = a_idx;
                a_idx += 1;
            }
            Sense::Eq => {
                t[i][a_idx] = 1.0;
                basis[i] = a_idx;
                a_idx += 1;
            }
        }
    }

    // Phase 1: minimize the sum of artificials
    if n_art > 0 {
        let mut c1 = vec![0.0; total];
        for c in c1.iter_mut().skip(art_start) {
            *c = 1.0;
        }
        set_objective_row(&mut t, &basis, &c1, m, total);
        if !simplex_iterate(&mut t, &mut basis, m, total, total) {
            // Phase 1 is bounded below by 0; failing here means cycling or numerical trouble
            return LpResult::Infeasible;
        }
        let scale = 1.0 + rows.iter().map(|r| r.2.abs()).fold(0.0, f64::max);
        if -t[m][rhs] > 1e-7 * scale {
            return LpResult::Infeasible;
        }
        // Drive zero-valued artificials out of the basis where possible
        for i in 0..m {
            if basis[i] >= art_start {
                if let Some(j) = (0..art_start).find(|&j| t[i][j].abs() > LP_EPS) {
                    pivot(&mut t, i, j, m, total);
                    basis[i] = j;
                }
            }
        }
    }

    // Phase 2: original objective over non-artificial columns
    let mut c2 = vec![0.0; total];
    c2[..ncol].copy_from_slice(&cost);
    set_objective_row(&mut t, &basis, &c2, m, total);
    if !simplex_iterate(&mut t, &mut basis, m, total, art_start) {
        return LpResult::Unbounded;
    }

    let mut y = vec![0.0; total];
    for i in 0..m {
        y[basis[i]] = t[i][rhs];
    }
    let mut x = vec![0.0; n];
    for j in 0..n {
        x[j] = match maps[j] {
            ColMap::Shift { col, offset } => offset + y[col],
            ColMap::Reflect { col, offset } => offset - y[col],
            ColMap::Free { pos, neg } => y[pos] - y[neg],
        };
    }
    let obj = lf.c.iter().zip(&x).map(|(c, xi)| c * xi).sum::<f64>() + lf.c0;
    let _ = cost0;
    LpResult::Optimal(x, obj)
}

fn set_objective_row(t: &mut [Vec<f64>], basis: &[usize], c: &[f64], m: usize, total: usize) {
    for j in 0..=total {
        t[m][j] = if j < total { c[j] } else { 0.0 };
    }
    for i in 0..m {
        let cb = c[basis[i]];
        if cb != 0.0 {
            for j in 0..=total {
                t[m][j] -= cb * t[i][j];
            }
        }
    }
}

fn pivot(t: &mut [Vec<f64>], r: usize, c: usize, m: usize, total: usize) {
    let p = t[r][c];
    for j in 0..=total {
        t[r][j] /= p;
    }
    for i in 0..=m {
        if i != r {
            let f = t[i][c];
            if f != 0.0 {
                for j in 0..=total {
                    t[i][j] -= f * t[r][j];
                    if t[i][j].abs() < 1e-13 {
                        t[i][j] = 0.0;
                    }
                }
            }
        }
    }
}

/// Runs simplex pivots with Bland's rule over columns < `allowed`. Returns false if the
/// objective is unbounded below.
fn simplex_iterate(
    t: &mut [Vec<f64>],
    basis: &mut [usize],
    m: usize,
    total: usize,
    allowed: usize,
) -> bool {
    for _ in 0..50_000 {
        let Some(enter) = (0..allowed).find(|&j| t[m][j] < -LP_EPS) else {
            return true;
        };
        let mut leave: Option<usize> = None;
        let mut best = f64::INFINITY;
        for i in 0..m {
            if t[i][enter] > LP_EPS {
                let ratio = t[i][total] / t[i][enter];
                let better = ratio < best - 1e-12
                    || (ratio <= best + 1e-12 && leave.map_or(true, |l| basis[i] < basis[l]));
                if better {
                    best = ratio;
                    leave = Some(i);
                }
            }
        }
        let Some(r) = leave else {
            return false;
        };
        pivot(t, r, enter, m, total);
        basis[r] = enter;
    }
    true
}

/// Greatest common divisor of two non-negative integers.
fn gcd_u64(mut a: u64, mut b: u64) -> u64 {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}

/// Integer-infeasibility certificate (the GCD test for linear Diophantine equations): an
/// equality `sum a_j x_j = b` whose a_j and b are all integers and whose variables are all
/// integer has no integer solution when gcd(a_j) does not divide b. Sound (never a false
/// infeasible); it does not catch every infeasibility.
fn gcd_proves_infeasible(p: &Problem, lf: &LinearForm) -> bool {
    let near_int = |v: f64| (v - v.round()).abs() < 1e-9;
    for row in &lf.rows {
        if row.sense != Sense::Eq {
            continue;
        }
        let terms: Vec<usize> = (0..row.a.len()).filter(|&j| row.a[j] != 0.0).collect();
        if terms.is_empty() || !terms.iter().all(|&j| p.vars[j].integer) {
            continue;
        }
        if !terms.iter().all(|&j| near_int(row.a[j])) || !near_int(row.b) {
            continue;
        }
        let g = terms
            .iter()
            .fold(0u64, |g, &j| gcd_u64(g, row.a[j].round().abs() as u64));
        if g > 0 && (row.b.round() as i64).rem_euclid(g as i64) != 0 {
            return true;
        }
    }
    false
}

/// True if `x` satisfies every row of `lf` and the given bounds, within tolerance.
fn point_feasible(lf: &LinearForm, x: &[f64], lo: &[f64], hi: &[f64]) -> bool {
    for j in 0..x.len() {
        if x[j] < lo[j] - 1e-6 || x[j] > hi[j] + 1e-6 {
            return false;
        }
    }
    for row in &lf.rows {
        let s: f64 = row.a.iter().zip(x).map(|(a, xi)| a * xi).sum();
        let tol = 1e-6 * (1.0 + row.b.abs());
        let ok = match row.sense {
            Sense::Le => s <= row.b + tol,
            Sense::Ge => s >= row.b - tol,
            Sense::Eq => (s - row.b).abs() <= tol,
        };
        if !ok {
            return false;
        }
    }
    true
}

fn objective_at(lf: &LinearForm, x: &[f64]) -> f64 {
    lf.c.iter().zip(x).map(|(c, xi)| c * xi).sum::<f64>() + lf.c0
}

/// Budgeted fix-and-branch feasibility search: repeatedly solve the LP and FIX the
/// most-fractional integer variable to a whole number (trying the nearer rounding first, then
/// the other), with limited backtracking. Returns a feasible integer point if one is found
/// within the node budget. Finds points the plain fractional dive misses (e.g. over large or
/// unbounded integer domains), without guaranteeing one exists.
fn dive(lf: &LinearForm, lo0: &[f64], hi0: &[f64], int_idx: &[usize]) -> Option<Vec<f64>> {
    fn search(
        lf: &LinearForm,
        lo: &[f64],
        hi: &[f64],
        lo0: &[f64],
        hi0: &[f64],
        int_idx: &[usize],
        budget: &mut i32,
    ) -> Option<Vec<f64>> {
        if *budget <= 0 {
            return None;
        }
        *budget -= 1;
        let x = match solve_lp(lf, lo, hi) {
            LpResult::Optimal(x, _) => x,
            // Unbounded relaxation: clamp to look for a nearby feasible point
            LpResult::Unbounded => {
                let clo: Vec<f64> = lo.iter().map(|&b| if b.is_finite() { b } else { -1e6 }).collect();
                let chi: Vec<f64> = hi.iter().map(|&b| if b.is_finite() { b } else { 1e6 }).collect();
                match solve_lp(lf, &clo, &chi) {
                    LpResult::Optimal(x, _) => x,
                    _ => return None,
                }
            }
            LpResult::Infeasible => return None,
        };
        let frac = int_idx
            .iter()
            .map(|&j| (j, (x[j] - x[j].round()).abs()))
            .filter(|(_, d)| *d > 1e-6)
            .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
        match frac {
            None => {
                let mut xr = x.clone();
                for &j in int_idx {
                    xr[j] = xr[j].round();
                }
                point_feasible(lf, &xr, lo0, hi0).then_some(xr)
            }
            Some((j, _)) => {
                let near = x[j].round();
                let far = if x[j] - x[j].floor() < 0.5 { x[j].ceil() } else { x[j].floor() };
                for val in [near, far] {
                    if val < lo0[j] - 1e-9 || val > hi0[j] + 1e-9 {
                        continue;
                    }
                    let mut nlo = lo.to_vec();
                    let mut nhi = hi.to_vec();
                    nlo[j] = val;
                    nhi[j] = val;
                    if let Some(r) = search(lf, &nlo, &nhi, lo0, hi0, int_idx, budget) {
                        return Some(r);
                    }
                    if *budget <= 0 {
                        break;
                    }
                }
                None
            }
        }
    }
    let mut budget = 4000;
    search(lf, lo0, hi0, lo0, hi0, int_idx, &mut budget)
}

// ---------------------------------------------------------------------------
// Gomory mixed-integer (GMI) cutting planes
// ---------------------------------------------------------------------------

/// Optimal simplex state needed to generate cuts. `col_affine[j]` expresses simplex column j
/// (structural or slack/surplus) as an affine function of the original variables x, together
/// with whether that column is integer-valued; `None` marks a column that cannot be expressed
/// in x (a split free variable), which disables any cut using it.
struct LpState {
    t: Vec<Vec<f64>>,
    basis: Vec<usize>,
    col_affine: Vec<Option<ColAffine>>,
    ncol: usize,
    art_start: usize,
    total: usize,
}

#[derive(Clone)]
struct ColAffine {
    coeffs: Vec<f64>, // over original x
    constant: f64,
    integer: bool,
}

/// Like `solve_lp`, but on an optimal solution also returns the tableau state for cut
/// generation. `integer_mask[j]` says whether original variable j is integer-constrained.
fn solve_lp_capture(
    lf: &LinearForm,
    lo: &[f64],
    hi: &[f64],
    integer_mask: &[bool],
) -> (LpResult, Option<LpState>) {
    let n = lf.c.len();
    for j in 0..n {
        if lo[j] > hi[j] + LP_EPS {
            return (LpResult::Infeasible, None);
        }
    }

    let mut maps = Vec::with_capacity(n);
    let mut ncol = 0;
    let mut extra_rows: Vec<(usize, f64, usize)> = Vec::new(); // (column, limit, original var)
    for j in 0..n {
        if lo[j].is_finite() {
            maps.push(ColMap::Shift { col: ncol, offset: lo[j] });
            if hi[j].is_finite() {
                extra_rows.push((ncol, hi[j] - lo[j], j));
            }
            ncol += 1;
        } else if hi[j].is_finite() {
            maps.push(ColMap::Reflect { col: ncol, offset: hi[j] });
            ncol += 1;
        } else {
            maps.push(ColMap::Free { pos: ncol, neg: ncol + 1 });
            ncol += 2;
        }
    }

    // Per structural column: the x-affine form and integrality.
    let mut col_affine: Vec<Option<ColAffine>> = vec![None; ncol];
    for j in 0..n {
        match maps[j] {
            ColMap::Shift { col, offset } => {
                let mut coeffs = vec![0.0; n];
                coeffs[j] = 1.0;
                col_affine[col] = Some(ColAffine { coeffs, constant: -offset, integer: integer_mask[j] });
            }
            ColMap::Reflect { col, offset } => {
                let mut coeffs = vec![0.0; n];
                coeffs[j] = -1.0;
                col_affine[col] = Some(ColAffine { coeffs, constant: offset, integer: integer_mask[j] });
            }
            ColMap::Free { .. } => {}
        }
    }

    // x-space slack affine for each logical row, recorded in build order (lf.rows then extra).
    let int_coeffs = |a: &[f64], b: f64| {
        a.iter().all(|v| (v - v.round()).abs() < 1e-9)
            && (b - b.round()).abs() < 1e-9
            && (0..n).all(|j| a[j] == 0.0 || integer_mask[j])
    };
    let mut row_slack: Vec<ColAffine> = Vec::new();
    let mut rows: Vec<(Vec<f64>, Sense, f64)> = Vec::new();
    for r in &lf.rows {
        // Le row slack = b - a.x ; Ge row surplus = a.x - b ; Eq has none (placeholder).
        let (coeffs, constant) = match r.sense {
            Sense::Le => (r.a.iter().map(|v| -v).collect::<Vec<_>>(), r.b),
            Sense::Ge => (r.a.clone(), -r.b),
            Sense::Eq => (vec![0.0; n], 0.0),
        };
        row_slack.push(ColAffine { coeffs, constant, integer: int_coeffs(&r.a, r.b) });
        // Build the y-space row (same as solve_lp)
        let mut a = vec![0.0; ncol];
        let mut b = r.b;
        for j in 0..n {
            let aj = r.a[j];
            if aj == 0.0 {
                continue;
            }
            match maps[j] {
                ColMap::Shift { col, offset } => {
                    a[col] += aj;
                    b -= aj * offset;
                }
                ColMap::Reflect { col, offset } => {
                    a[col] -= aj;
                    b -= aj * offset;
                }
                ColMap::Free { pos, neg } => {
                    a[pos] += aj;
                    a[neg] -= aj;
                }
            }
        }
        rows.push((a, r.sense, b));
    }
    for (col, limit, j) in &extra_rows {
        // x_j <= hi_j : slack = hi_j - x_j
        let mut coeffs = vec![0.0; n];
        coeffs[*j] = -1.0;
        row_slack.push(ColAffine { coeffs, constant: hi[*j], integer: integer_mask[*j] });
        let mut a = vec![0.0; ncol];
        a[*col] = 1.0;
        rows.push((a, Sense::Le, *limit));
    }

    let mut cost = vec![0.0; ncol];
    let mut cost0 = lf.c0;
    for j in 0..n {
        let cj = lf.c[j];
        match maps[j] {
            ColMap::Shift { col, offset } => {
                cost[col] += cj;
                cost0 += cj * offset;
            }
            ColMap::Reflect { col, offset } => {
                cost[col] -= cj;
                cost0 += cj * offset;
            }
            ColMap::Free { pos, neg } => {
                cost[pos] += cj;
                cost[neg] -= cj;
            }
        }
    }
    let _ = cost0;

    // Normalize to b >= 0, tracking slack affine per row by index.
    for (i, row) in rows.iter_mut().enumerate() {
        if row.2 < 0.0 {
            for v in row.0.iter_mut() {
                *v = -*v;
            }
            row.2 = -row.2;
            row.1 = match row.1 {
                Sense::Le => Sense::Ge,
                Sense::Ge => Sense::Le,
                Sense::Eq => Sense::Eq,
            };
            let _ = i; // slack affine is orientation-independent (equals the original slack)
        }
    }

    let m = rows.len();
    let n_slack = rows.iter().filter(|r| r.1 != Sense::Eq).count();
    let n_art = rows.iter().filter(|r| r.1 != Sense::Le).count();
    let total = ncol + n_slack + n_art;
    let rhs = total;
    let mut t = vec![vec![0.0; total + 1]; m + 1];
    let mut basis = vec![0usize; m];
    let art_start = ncol + n_slack;
    // Map each slack/surplus column back to its row's slack affine.
    let mut col_slack: Vec<Option<ColAffine>> = vec![None; art_start];
    let (mut s, mut a_idx) = (ncol, art_start);
    for (i, (a, sense, b)) in rows.iter().enumerate() {
        t[i][..ncol].copy_from_slice(a);
        t[i][rhs] = *b;
        match sense {
            Sense::Le => {
                t[i][s] = 1.0;
                basis[i] = s;
                col_slack[s] = Some(row_slack[i].clone());
                s += 1;
            }
            Sense::Ge => {
                t[i][s] = -1.0;
                col_slack[s] = Some(row_slack[i].clone());
                s += 1;
                t[i][a_idx] = 1.0;
                basis[i] = a_idx;
                a_idx += 1;
            }
            Sense::Eq => {
                t[i][a_idx] = 1.0;
                basis[i] = a_idx;
                a_idx += 1;
            }
        }
    }

    if n_art > 0 {
        let mut c1 = vec![0.0; total];
        for c in c1.iter_mut().skip(art_start) {
            *c = 1.0;
        }
        set_objective_row(&mut t, &basis, &c1, m, total);
        if !simplex_iterate(&mut t, &mut basis, m, total, total) {
            return (LpResult::Infeasible, None);
        }
        let scale = 1.0 + rows.iter().map(|r| r.2.abs()).fold(0.0, f64::max);
        if -t[m][rhs] > 1e-7 * scale {
            return (LpResult::Infeasible, None);
        }
        for i in 0..m {
            if basis[i] >= art_start {
                if let Some(j) = (0..art_start).find(|&j| t[i][j].abs() > LP_EPS) {
                    pivot(&mut t, i, j, m, total);
                    basis[i] = j;
                }
            }
        }
    }

    let mut c2 = vec![0.0; total];
    c2[..ncol].copy_from_slice(&cost);
    set_objective_row(&mut t, &basis, &c2, m, total);
    if !simplex_iterate(&mut t, &mut basis, m, total, art_start) {
        return (LpResult::Unbounded, None);
    }

    let mut y = vec![0.0; total];
    for i in 0..m {
        y[basis[i]] = t[i][rhs];
    }
    let mut x = vec![0.0; n];
    for j in 0..n {
        x[j] = match maps[j] {
            ColMap::Shift { col, offset } => offset + y[col],
            ColMap::Reflect { col, offset } => offset - y[col],
            ColMap::Free { pos, neg } => y[pos] - y[neg],
        };
    }
    let obj = lf.c.iter().zip(&x).map(|(c, xi)| c * xi).sum::<f64>() + lf.c0;

    // Full column affine map (artificial columns are excluded from cuts): structural columns
    // 0..ncol, then slack/surplus columns ncol..art_start.
    let mut col_affine_full: Vec<Option<ColAffine>> = vec![None; art_start];
    for (j, ca) in col_affine.into_iter().enumerate() {
        col_affine_full[j] = ca;
    }
    for s in ncol..art_start {
        col_affine_full[s] = col_slack[s].take();
    }

    let state = LpState {
        t,
        basis,
        col_affine: col_affine_full,
        ncol,
        art_start,
        total,
    };
    (LpResult::Optimal(x, obj), Some(state))
}

/// Generates Gomory mixed-integer cuts from fractional integer-basic rows of an optimal
/// tableau, translated into x-space inequalities (`a·x >= b`). Each cut is valid for every
/// integer-feasible point. Returns at most `max_cuts` of the most-violated cuts.
fn gmi_cuts(state: &LpState, x: &[f64], max_cuts: usize) -> Vec<LinRow> {
    let n = x.len();
    let frac = |v: f64| v - v.floor();
    // Which structural/slack columns are integer (for GMI's integer-column rule)?
    let mut col_is_int = vec![false; state.art_start];
    for (j, aff) in state.col_affine.iter().enumerate() {
        if let Some(a) = aff {
            col_is_int[j] = a.integer;
        }
    }
    let nonbasic: Vec<usize> = {
        let mut inb = vec![false; state.total];
        for &bcol in &state.basis {
            if bcol < state.total {
                inb[bcol] = true;
            }
        }
        (0..state.art_start).filter(|&j| !inb[j]).collect()
    };

    let mut cuts: Vec<(f64, LinRow)> = Vec::new();
    for (i, &bcol) in state.basis.iter().enumerate() {
        // Source row: a structural integer column that is fractional.
        if bcol >= state.ncol || !col_is_int[bcol] {
            continue;
        }
        let beta = state.t[i][state.total];
        let f0 = frac(beta);
        if f0 < 1e-6 || f0 > 1.0 - 1e-6 {
            continue;
        }
        let mut coeffs = vec![0.0; n];
        let mut constant = 0.0;
        let mut ok = true;
        for &j in &nonbasic {
            let a = state.t[i][j];
            if a.abs() < 1e-11 {
                continue;
            }
            let Some(aff) = &state.col_affine[j] else {
                ok = false;
                break;
            };
            let g = if col_is_int[j] {
                let f = frac(a);
                if f <= f0 {
                    f
                } else {
                    f0 * (1.0 - f) / (1.0 - f0)
                }
            } else if a >= 0.0 {
                a
            } else {
                f0 * (-a) / (1.0 - f0)
            };
            if g.abs() < 1e-12 {
                continue;
            }
            for k in 0..n {
                coeffs[k] += g * aff.coeffs[k];
            }
            constant += g * aff.constant;
        }
        if !ok {
            continue;
        }
        // Cut: sum g_j * affine_j(x) >= f0  ->  coeffs·x >= f0 - constant
        let b = f0 - constant;
        let max_c = coeffs.iter().map(|v| v.abs()).fold(0.0, f64::max);
        if max_c > 1e7 || !b.is_finite() {
            continue; // ill-conditioned cut; skip to protect the simplex
        }
        let lhs_at_x: f64 = coeffs.iter().zip(x).map(|(c, xi)| c * xi).sum();
        let violation = b - lhs_at_x; // > 0 means the cut removes x*
        let scale = 1.0 + max_c;
        if violation > 1e-5 * scale && max_c > 1e-9 {
            cuts.push((violation, LinRow { a: coeffs, sense: Sense::Ge, b }));
        }
    }
    cuts.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
    cuts.into_iter().take(max_cuts).map(|(_, r)| r).collect()
}

/// Branch-and-bound over LP relaxations.
///
/// Correctness: a completed search proves optimality or infeasibility. For models with
/// unbounded integer domains the pure-B&B search cannot always close the gap (that needs
/// cutting planes), so the result is then reported honestly as not proven, or as
/// undetermined, rather than as a wrong verdict:
///   * an equality failing the GCD test is proven infeasible;
///   * an unbounded LP relaxation yields "unbounded" only with an integer-feasible witness,
///     otherwise "undetermined" (never a false "unbounded");
///   * a feasible incumbent found past the node limit is returned as "not proven optimal".
fn solve_milp(p: &Problem, lf: &LinearForm) -> Outcome {
    let n = p.vars.len();
    let int_idx: Vec<usize> = (0..n).filter(|&j| p.vars[j].integer).collect();

    if gcd_proves_infeasible(p, lf) {
        return Outcome::Infeasible(
            "no integer solution: an equality's coefficients share a factor that does not \
             divide its constant (GCD test)"
                .to_string(),
        );
    }

    // Root cutting-plane loop (cut-and-branch): tighten the relaxation with Gomory
    // mixed-integer cuts so the branch-and-bound below converges and can prove optimality,
    // including over unbounded integer domains where plain branching cannot. GMI cuts are valid
    // for every integer-feasible point, so this never removes a real solution.
    let integer_mask: Vec<bool> = (0..n).map(|j| p.vars[j].integer).collect();
    let mut lf_cut = lf.clone();
    if !int_idx.is_empty() {
        let mut lo: Vec<f64> = p.vars.iter().map(|v| v.lo).collect();
        let mut hi: Vec<f64> = p.vars.iter().map(|v| v.hi).collect();
        for &j in &int_idx {
            lo[j] = (lo[j] - 1e-9).ceil();
            hi[j] = (hi[j] + 1e-9).floor();
        }
        let base_rows = lf_cut.rows.len();
        let mut prev_obj = f64::INFINITY;
        for _round in 0..12 {
            let (res, state) = solve_lp_capture(&lf_cut, &lo, &hi, &integer_mask);
            let (x, obj) = match res {
                LpResult::Optimal(x, o) => (x, o),
                _ => break,
            };
            if int_idx.iter().all(|&j| (x[j] - x[j].round()).abs() < 1e-6) {
                break; // relaxation already integral
            }
            // Stop once cuts stop tightening the bound: adding more only risks numerical harm.
            if prev_obj.is_finite() && (obj - prev_obj).abs() <= 1e-7 * (1.0 + obj.abs()) {
                break;
            }
            prev_obj = obj;
            let Some(state) = state else { break };
            let cuts = gmi_cuts(&state, &x, 6);
            if cuts.is_empty() || lf_cut.rows.len() - base_rows > 60 {
                break;
            }
            lf_cut.rows.extend(cuts);
        }
    }

    // Zero-objective form: the dive seeks any feasible integer point, so it should follow the
    // feasible polytope rather than chase the (possibly unbounded) objective.
    let lf_feas = LinearForm {
        rows: lf_cut.rows.clone(),
        c: vec![0.0; n],
        c0: 0.0,
    };

    let root_lo: Vec<f64> = p.vars.iter().map(|v| v.lo).collect();
    let root_hi: Vec<f64> = p.vars.iter().map(|v| v.hi).collect();
    let tighten = |lo: &mut Vec<f64>, hi: &mut Vec<f64>| {
        for &j in &int_idx {
            lo[j] = (lo[j] - 1e-9).ceil();
            hi[j] = (hi[j] + 1e-9).floor();
        }
    };

    // Seed an incumbent with a diving heuristic, so weak-gap searches still return the best
    // point they can find rather than giving up.
    let mut best: Option<(Vec<f64>, f64)> = None;
    if !int_idx.is_empty() {
        let (mut lo, mut hi) = (root_lo.clone(), root_hi.clone());
        tighten(&mut lo, &mut hi);
        if let Some(xr) = dive(&lf_feas, &lo, &hi, &int_idx) {
            best = Some((xr.clone(), objective_at(lf, &xr)));
        }
    }

    let mut stack = vec![(root_lo, root_hi)];
    let mut nodes = 0usize;
    const NODE_LIMIT: usize = 200_000;
    let mut relaxation_unbounded = false;
    let mut complete = true; // fully explored: optimal/infeasible provable

    while let Some((mut lo, mut hi)) = stack.pop() {
        nodes += 1;
        if nodes > NODE_LIMIT {
            complete = false;
            break;
        }
        tighten(&mut lo, &mut hi);
        match solve_lp(&lf_cut, &lo, &hi) {
            LpResult::Infeasible => continue,
            LpResult::Unbounded => {
                // Cannot bound this subtree; a feasible witness decides unbounded vs not.
                relaxation_unbounded = true;
                complete = false;
                if best.is_none() {
                    let clamp = |b: f64, d: f64| if b.is_finite() { b } else { d };
                    let clo: Vec<f64> = lo.iter().map(|&b| clamp(b, -1e6)).collect();
                    let chi: Vec<f64> = hi.iter().map(|&b| clamp(b, 1e6)).collect();
                    if let Some(xr) = dive(&lf_feas, &clo, &chi, &int_idx) {
                        best = Some((xr.clone(), objective_at(lf, &xr)));
                    }
                }
                continue;
            }
            LpResult::Optimal(x, obj) => {
                if let Some((_, best_obj)) = &best {
                    if obj >= best_obj - 1e-9 * (1.0 + best_obj.abs()) {
                        continue; // this node cannot beat the incumbent
                    }
                }
                let frac = int_idx
                    .iter()
                    .map(|&j| (j, (x[j] - x[j].round()).abs()))
                    .filter(|(_, d)| *d > 1e-6)
                    .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
                match frac {
                    None => {
                        let mut xr = x.clone();
                        for &j in &int_idx {
                            xr[j] = xr[j].round();
                        }
                        best = Some((xr, obj));
                    }
                    Some((j, _)) => {
                        let mut up_lo = lo.clone();
                        up_lo[j] = x[j].ceil();
                        let mut down_hi = hi.clone();
                        down_hi[j] = x[j].floor();
                        if x[j] - x[j].floor() < 0.5 {
                            stack.push((up_lo, hi.clone()));
                            stack.push((lo.clone(), down_hi));
                        } else {
                            stack.push((lo.clone(), down_hi));
                            stack.push((up_lo, hi.clone()));
                        }
                    }
                }
            }
        }
    }

    let nodes_shown = nodes.min(NODE_LIMIT);
    match best {
        Some((x, objective)) => {
            if relaxation_unbounded {
                // Feasible point exists and the relaxation is unbounded ⟹ the MILP is unbounded
                Outcome::Unbounded(
                    "the objective improves without bound over the integer-feasible region"
                        .to_string(),
                )
            } else {
                Outcome::Optimal {
                    x,
                    objective,
                    proven: complete,
                    method: if int_idx.is_empty() {
                        "simplex".to_string()
                    } else if complete {
                        format!("simplex branch-and-bound ({} nodes)", nodes_shown)
                    } else {
                        format!(
                            "simplex branch-and-bound ({} nodes; node limit reached)",
                            nodes_shown
                        )
                    },
                }
            }
        }
        None => {
            if relaxation_unbounded {
                Outcome::Undetermined(
                    "the continuous relaxation is unbounded and no integer-feasible point was \
                     found; infeasibility is not proven (needs cutting planes)"
                        .to_string(),
                )
            } else if complete {
                Outcome::Infeasible("no point satisfies every constraint (proven by branch-and-bound)".to_string())
            } else {
                Outcome::Undetermined(format!(
                    "no feasible point found within the branch-and-bound node limit ({} nodes)",
                    NODE_LIMIT
                ))
            }
        }
    }
}

// ===========================================================================
// Nonlinear engine: augmented Lagrangian + spectral projected gradient
// ===========================================================================

/// Expression compiled for fast repeated evaluation: known variables are folded into
/// constants and unknowns are addressed by index. Evaluation follows `eval_expr` exactly,
/// returning NaN where `eval_expr` would return an error.
#[derive(Debug, Clone)]
enum CExpr {
    Const(f64),
    Var(usize),
    Neg(Box<CExpr>),
    Bin(BinaryOp, Box<CExpr>, Box<CExpr>),
    Call(String, Box<CExpr>),
}

impl CExpr {
    fn compile(e: &Expr, index: &HashMap<String, usize>, env: &HashMap<String, f64>) -> CExpr {
        match e {
            Expr::Num(n) => CExpr::Const(*n),
            Expr::Var(name) => match index.get(name) {
                Some(&i) => CExpr::Var(i),
                None => CExpr::Const(eval_expr(e, env).unwrap_or(f64::NAN)),
            },
            Expr::UnaryMinus(inner) => CExpr::Neg(Box::new(Self::compile(inner, index, env))),
            Expr::Bin(op, l, r) => CExpr::Bin(
                *op,
                Box::new(Self::compile(l, index, env)),
                Box::new(Self::compile(r, index, env)),
            ),
            Expr::Call(f, arg) => CExpr::Call(f.clone(), Box::new(Self::compile(arg, index, env))),
        }
    }

    fn eval(&self, x: &[f64]) -> f64 {
        match self {
            CExpr::Const(c) => *c,
            CExpr::Var(i) => x[*i],
            CExpr::Neg(a) => -a.eval(x),
            CExpr::Bin(op, l, r) => {
                let (a, b) = (l.eval(x), r.eval(x));
                let v = match op {
                    BinaryOp::Add => a + b,
                    BinaryOp::Sub => a - b,
                    BinaryOp::Mul => a * b,
                    BinaryOp::Div => {
                        if b == 0.0 {
                            return f64::NAN;
                        }
                        a / b
                    }
                    BinaryOp::Pow => a.powf(b),
                };
                if v.is_finite() { v } else { f64::NAN }
            }
            CExpr::Call(f, arg) => {
                let v = arg.eval(x);
                let snap = |v: f64, r: f64| if (v - v.round()).abs() < 1e-9 { v.round() } else { r };
                match f.as_str() {
                    "sqrt" if v >= 0.0 => v.sqrt(),
                    "ceil" => snap(v, v.ceil()),
                    "floor" => snap(v, v.floor()),
                    "round" => v.round(),
                    "abs" => v.abs(),
                    "sign" => {
                        if v > 0.0 {
                            1.0
                        } else if v < 0.0 {
                            -1.0
                        } else {
                            0.0
                        }
                    }
                    "log" | "ln" if v > 0.0 => v.ln(),
                    "exp" => v.exp(),
                    "sin" => v.sin(),
                    "cos" => v.cos(),
                    "tan" if v.cos().abs() >= 1e-12 => v.tan(),
                    _ => f64::NAN,
                }
            }
        }
    }
}

/// A compiled function with its compiled partial derivatives (None = use differences).
struct CFun {
    f: CExpr,
    partials: Vec<Option<CExpr>>,
}

impl CFun {
    fn new(e: &Expr, p: &Problem, index: &HashMap<String, usize>) -> CFun {
        CFun {
            f: CExpr::compile(e, index, &p.env),
            partials: p
                .vars
                .iter()
                .map(|v| diff(e, &v.name).ok().map(|d| CExpr::compile(&d, index, &p.env)))
                .collect(),
        }
    }

    fn grad_into(&self, x: &[f64], weight: f64, out: &mut [f64]) {
        for j in 0..x.len() {
            let sym = self.partials[j].as_ref().map(|d| d.eval(x));
            let gj = match sym {
                Some(v) if v.is_finite() => v,
                _ => {
                    let h = 1e-7 * (1.0 + x[j].abs());
                    let mut xp = x.to_vec();
                    xp[j] += h;
                    let mut xm = x.to_vec();
                    xm[j] -= h;
                    (self.f.eval(&xp) - self.f.eval(&xm)) / (2.0 * h)
                }
            };
            out[j] += weight * gj;
        }
    }

    fn grad_norm(&self, x: &[f64]) -> f64 {
        let mut g = vec![0.0; x.len()];
        self.grad_into(x, 1.0, &mut g);
        g.iter().map(|v| v.abs()).filter(|v| v.is_finite()).fold(0.0, f64::max)
    }
}

struct Compiled {
    obj: Option<CFun>,
    eqs: Vec<CFun>,
    ineqs: Vec<(CFun, bool)>,
    /// Scale factors (objective, equations, inequalities) for balanced conditioning
    s_obj: f64,
    s_eq: Vec<f64>,
    s_in: Vec<f64>,
}

impl Compiled {
    fn new(p: &Problem) -> Self {
        let index: HashMap<String, usize> =
            p.vars.iter().enumerate().map(|(i, v)| (v.name.clone(), i)).collect();
        let obj = p.objective.as_ref().map(|o| CFun::new(o, p, &index));
        let eqs: Vec<CFun> = p.eqs.iter().map(|h| CFun::new(h, p, &index)).collect();
        let ineqs: Vec<(CFun, bool)> = p
            .ineqs
            .iter()
            .map(|g| (CFun::new(&g.expr, p, &index), g.strict))
            .collect();
        let n_eq = eqs.len();
        let n_in = ineqs.len();
        Compiled {
            obj,
            eqs,
            ineqs,
            s_obj: 1.0,
            s_eq: vec![1.0; n_eq],
            s_in: vec![1.0; n_in],
        }
    }

    /// Scales every function by 1 / max(1, |gradient|) at x so no term dominates.
    fn rescale_at(&mut self, x: &[f64]) {
        let scale = |f: &CFun| 1.0 / f.grad_norm(x).max(1.0);
        self.s_obj = self.obj.as_ref().map(scale).unwrap_or(1.0);
        self.s_eq = self.eqs.iter().map(scale).collect();
        self.s_in = self.ineqs.iter().map(|(g, _)| scale(g)).collect();
    }

    fn ineq_value(&self, i: usize, x: &[f64]) -> f64 {
        let (g, strict) = &self.ineqs[i];
        g.f.eval(x) + if *strict { STRICT_MARGIN } else { 0.0 }
    }

    /// Augmented Lagrangian value and gradient (scaled functions).
    fn lagrangian(&self, x: &[f64], lam: &[f64], mu: &[f64], rho: f64) -> (f64, Vec<f64>) {
        let n = x.len();
        let mut val = 0.0;
        let mut grad = vec![0.0; n];
        if let Some(o) = &self.obj {
            val += self.s_obj * o.f.eval(x);
            o.grad_into(x, self.s_obj, &mut grad);
        }
        for (i, h) in self.eqs.iter().enumerate() {
            let hv = self.s_eq[i] * h.f.eval(x);
            val += lam[i] * hv + 0.5 * rho * hv * hv;
            let w = lam[i] + rho * hv;
            if w != 0.0 {
                h.grad_into(x, w * self.s_eq[i], &mut grad);
            }
        }
        for i in 0..self.ineqs.len() {
            let gv = self.s_in[i] * self.ineq_value(i, x);
            let s = (mu[i] + rho * gv).max(0.0);
            val += (s * s - mu[i] * mu[i]) / (2.0 * rho);
            if s > 0.0 {
                self.ineqs[i].0.grad_into(x, s * self.s_in[i], &mut grad);
            }
        }
        (val, grad)
    }

    /// Sum of squared (scaled) constraint violations and its gradient: zero exactly on the
    /// feasible set.
    fn violation(&self, x: &[f64]) -> (f64, Vec<f64>) {
        let mut val = 0.0;
        let mut grad = vec![0.0; x.len()];
        for (i, h) in self.eqs.iter().enumerate() {
            let hv = self.s_eq[i] * h.f.eval(x);
            val += hv * hv;
            h.grad_into(x, 2.0 * hv * self.s_eq[i], &mut grad);
        }
        for i in 0..self.ineqs.len() {
            let gv = self.s_in[i] * self.ineq_value(i, x);
            if gv > 0.0 {
                val += gv * gv;
                self.ineqs[i].0.grad_into(x, 2.0 * gv * self.s_in[i], &mut grad);
            }
        }
        (val, grad)
    }

    /// Max unscaled constraint violation at x.
    fn infeasibility(&self, x: &[f64]) -> f64 {
        let mut v: f64 = 0.0;
        for h in &self.eqs {
            let hv = h.f.eval(x);
            v = v.max(if hv.is_finite() { hv.abs() } else { f64::INFINITY });
        }
        for i in 0..self.ineqs.len() {
            let gv = self.ineq_value(i, x);
            v = v.max(if gv.is_finite() { gv.max(0.0) } else { f64::INFINITY });
        }
        v
    }

    fn objective(&self, x: &[f64]) -> f64 {
        self.obj.as_ref().map_or(0.0, |o| o.f.eval(x))
    }
}

fn project(x: &mut [f64], lo: &[f64], hi: &[f64]) {
    for j in 0..x.len() {
        x[j] = x[j].max(lo[j]).min(hi[j]);
    }
}

/// Spectral projected gradient (Birgin–Martínez–Raydan) with a non-monotone line search.
fn spg<F: Fn(&[f64]) -> (f64, Vec<f64>)>(
    f: &F,
    x0: &[f64],
    lo: &[f64],
    hi: &[f64],
    tol: f64,
    max_iter: usize,
) -> Vec<f64> {
    let n = x0.len();
    let mut x = x0.to_vec();
    project(&mut x, lo, hi);
    let (mut fx, mut g) = f(&x);
    if !fx.is_finite() {
        return x;
    }
    let mut history = vec![fx];
    let pg_norm = |x: &[f64], g: &[f64]| {
        let mut m: f64 = 0.0;
        for j in 0..n {
            let p = (x[j] - g[j]).max(lo[j]).min(hi[j]) - x[j];
            m = m.max(p.abs());
        }
        m
    };
    let mut lambda = {
        let pn = pg_norm(&x, &g);
        if pn > 0.0 { (1.0 / pn).clamp(1e-10, 1e10) } else { 1.0 }
    };
    for _ in 0..max_iter {
        if pg_norm(&x, &g) <= tol {
            break;
        }
        let mut d = vec![0.0; n];
        for j in 0..n {
            d[j] = (x[j] - lambda * g[j]).max(lo[j]).min(hi[j]) - x[j];
        }
        let gd: f64 = (0..n).map(|j| g[j] * d[j]).sum();
        let f_ref = history.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        let mut alpha = 1.0;
        let mut accepted = None;
        for _ in 0..60 {
            let xn: Vec<f64> = (0..n).map(|j| x[j] + alpha * d[j]).collect();
            let (fn_, gn) = f(&xn);
            if fn_.is_finite() && fn_ <= f_ref + 1e-4 * alpha * gd {
                accepted = Some((xn, fn_, gn));
                break;
            }
            alpha *= 0.5;
        }
        let Some((xn, fn_, gn)) = accepted else {
            break;
        };
        let s: Vec<f64> = (0..n).map(|j| xn[j] - x[j]).collect();
        let y: Vec<f64> = (0..n).map(|j| gn[j] - g[j]).collect();
        let sy: f64 = (0..n).map(|j| s[j] * y[j]).sum();
        let ss: f64 = (0..n).map(|j| s[j] * s[j]).sum();
        lambda = if sy > 0.0 { (ss / sy).clamp(1e-10, 1e10) } else { 1e10 };
        x = xn;
        fx = fn_;
        g = gn;
        history.push(fx);
        if history.len() > 10 {
            history.remove(0);
        }
        if ss.sqrt() < 1e-15 * (1.0 + x.iter().map(|v| v.abs()).fold(0.0, f64::max)) {
            break;
        }
    }
    x
}

/// Augmented Lagrangian from one starting point. Returns (x, infeasibility).
fn augmented_lagrangian(c: &Compiled, x0: &[f64], lo: &[f64], hi: &[f64]) -> (Vec<f64>, f64) {
    let mut lam = vec![0.0; c.eqs.len()];
    let mut mu = vec![0.0; c.ineqs.len()];
    let mut rho = 100.0;
    let mut x = x0.to_vec();
    let mut prev_infeas = f64::INFINITY;
    let mut prev_obj = f64::INFINITY;
    for outer in 0..40 {
        let tol = if outer < 3 { 1e-5 } else { 1e-9 };
        x = spg(&|z: &[f64]| c.lagrangian(z, &lam, &mu, rho), &x, lo, hi, tol, 800);
        let infeas = c.infeasibility(&x);
        for (i, h) in c.eqs.iter().enumerate() {
            lam[i] += rho * c.s_eq[i] * h.f.eval(&x);
        }
        for i in 0..c.ineqs.len() {
            mu[i] = (mu[i] + rho * c.s_in[i] * c.ineq_value(i, &x)).max(0.0);
        }
        let obj = c.objective(&x);
        let stalled = (obj - prev_obj).abs() <= 1e-10 * (1.0 + obj.abs());
        if infeas <= 1e-10 && outer >= 2 && stalled {
            return (x, infeas);
        }
        prev_obj = obj;
        if infeas > 0.25 * prev_infeas {
            rho = (rho * 10.0).min(1e12);
        }
        prev_infeas = infeas;
        if !lam.iter().chain(mu.iter()).all(|v| v.is_finite()) {
            break;
        }
    }
    let infeas = c.infeasibility(&x);
    (x, infeas)
}

/// Deterministic pseudo-random sequence for reproducible multi-start points.
struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> f64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((self.0 >> 11) as f64) / ((1u64 << 53) as f64)
    }
}

fn start_points(lo: &[f64], hi: &[f64]) -> Vec<Vec<f64>> {
    let n = lo.len();
    let pick = |f: &dyn Fn(usize) -> f64| -> Vec<f64> { (0..n).map(f).collect() };
    let mut pts = vec![
        pick(&|j| 0.0f64.max(lo[j]).min(hi[j])),
        pick(&|j| 1.0f64.max(lo[j]).min(hi[j])),
        pick(&|j| {
            if lo[j].is_finite() && hi[j].is_finite() {
                0.5 * (lo[j] + hi[j])
            } else if lo[j].is_finite() {
                lo[j] + 1.0
            } else if hi[j].is_finite() {
                hi[j] - 1.0
            } else {
                0.0
            }
        }),
    ];
    let mut rng = Lcg(0x5eed);
    for _ in 0..6 {
        pts.push(pick(&|_| 0.0));
        let last = pts.last_mut().unwrap();
        for j in 0..n {
            let u = rng.next();
            last[j] = if lo[j].is_finite() && hi[j].is_finite() {
                lo[j] + u * (hi[j] - lo[j])
            } else if lo[j].is_finite() {
                lo[j] + 20.0 * u
            } else if hi[j].is_finite() {
                hi[j] - 20.0 * u
            } else {
                40.0 * u - 20.0
            };
        }
    }
    pts
}

/// Best feasible local optimum of the continuous relaxation over [lo, hi].
fn relax_nonlinear(c: &mut Compiled, lo: &[f64], hi: &[f64]) -> Option<(Vec<f64>, f64)> {
    let mut best: Option<(Vec<f64>, f64)> = None;
    let starts = start_points(lo, hi);
    c.rescale_at(&starts[2]);
    let c = &*c;
    let feasible = |x: &[f64]| {
        let scale = 1.0 + x.iter().map(|v| v.abs()).fold(0.0, f64::max);
        c.infeasibility(x) <= 1e-7 * scale
    };
    let consider = |x: Vec<f64>, best: &mut Option<(Vec<f64>, f64)>| {
        if !feasible(&x) {
            return;
        }
        let obj = c.objective(&x);
        if obj.is_finite() && best.as_ref().map_or(true, |(_, b)| obj < b - 1e-12 * (1.0 + b.abs())) {
            *best = Some((x, obj));
        }
    };
    for x0 in starts {
        // Phase 1: reach the feasible region first, so the objective cannot drag the iterate
        // into a point where the constraint gradients vanish (e.g. a product of zeros).
        let x_feas = spg(&|z: &[f64]| c.violation(z), &x0, lo, hi, 1e-12, 2000);
        consider(x_feas.clone(), &mut best);
        // Phase 2: optimize from the feasible point
        let (x, _) = augmented_lagrangian(c, &x_feas, lo, hi);
        consider(x, &mut best);
    }
    best
}

enum BnbResult {
    Found(Option<(Vec<f64>, f64)>, usize),
    Unbounded,
}

/// Depth-first branch-and-bound over nonlinear relaxations within [lo, hi].
fn bnb_nonlinear(
    c: &mut Compiled,
    lo: Vec<f64>,
    hi: Vec<f64>,
    int_idx: &[usize],
    incumbent: Option<(Vec<f64>, f64)>,
    node_limit: usize,
) -> BnbResult {
    let n = lo.len();
    let mut stack = vec![(lo, hi)];
    let mut best = incumbent;
    let mut nodes = 0usize;
    while let Some((mut lo, mut hi)) = stack.pop() {
        nodes += 1;
        if nodes > node_limit {
            break;
        }
        for &j in int_idx {
            lo[j] = (lo[j] - 1e-9).ceil();
            hi[j] = (hi[j] + 1e-9).floor();
        }
        if (0..n).any(|j| lo[j] > hi[j]) {
            continue;
        }
        let Some((x, obj)) = relax_nonlinear(c, &lo, &hi) else {
            continue;
        };
        if x.iter().any(|v| v.abs() > 1e12) || obj < -1e15 {
            return BnbResult::Unbounded;
        }
        if let Some((_, b)) = &best {
            if obj >= b - 1e-9 * (1.0 + b.abs()) {
                continue;
            }
        }
        let frac = int_idx
            .iter()
            .map(|&j| (j, (x[j] - x[j].round()).abs()))
            .filter(|(_, d)| *d > 1e-6)
            .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
        match frac {
            None => {
                let mut xr = x.clone();
                for &j in int_idx {
                    xr[j] = xr[j].round();
                }
                let obj_r = c.objective(&xr);
                if c.infeasibility(&xr) <= 1e-7 * (1.0 + obj_r.abs()) {
                    best = Some((xr, obj_r));
                }
            }
            Some((j, _)) => {
                let mut up_lo = lo.clone();
                up_lo[j] = x[j].ceil();
                let mut down_hi = hi.clone();
                down_hi[j] = x[j].floor();
                // Explore the nearer side first (pushed last)
                if x[j] - x[j].floor() < 0.5 {
                    stack.push((up_lo, hi.clone()));
                    stack.push((lo.clone(), down_hi));
                } else {
                    stack.push((lo.clone(), down_hi));
                    stack.push((up_lo, hi.clone()));
                }
            }
        }
    }
    BnbResult::Found(best, nodes)
}

/// Improves an integer incumbent: fixes one integer variable at its value ±1 and re-solves
/// the rest by branch-and-bound, repeating while the objective improves. Catches optima that
/// nonconvex pruning skipped (e.g. trading one node for more drives per node).
fn integer_neighborhood_search(
    c: &mut Compiled,
    lo0: &[f64],
    hi0: &[f64],
    int_idx: &[usize],
    mut best: (Vec<f64>, f64),
) -> (Vec<f64>, f64) {
    for _round in 0..20 {
        let mut improved = false;
        for &j in int_idx {
            for delta in [-1.0, 1.0] {
                let v = best.0[j] + delta;
                if v < lo0[j] - 1e-9 || v > hi0[j] + 1e-9 {
                    continue;
                }
                let mut lo = lo0.to_vec();
                let mut hi = hi0.to_vec();
                lo[j] = v;
                hi[j] = v;
                if let BnbResult::Found(Some((x, obj)), _) =
                    bnb_nonlinear(c, lo, hi, int_idx, None, 200)
                {
                    if obj < best.1 - 1e-9 * (1.0 + best.1.abs()) {
                        best = (x, obj);
                        improved = true;
                    }
                }
            }
        }
        if !improved {
            break;
        }
    }
    best
}

fn solve_nonlinear(p: &Problem) -> Outcome {
    let mut c = Compiled::new(p);
    let n = p.vars.len();
    let int_idx: Vec<usize> = (0..n).filter(|&j| p.vars[j].integer).collect();
    let lo0: Vec<f64> = p.vars.iter().map(|v| v.lo).collect();
    let hi0: Vec<f64> = p.vars.iter().map(|v| v.hi).collect();
    const NODE_LIMIT: usize = 2_000;

    let (mut best, nodes) =
        match bnb_nonlinear(&mut c, lo0.clone(), hi0.clone(), &int_idx, None, NODE_LIMIT) {
            BnbResult::Unbounded => {
                return Outcome::Unbounded(
                    "the objective keeps improving as variables grow without limit; add bounds"
                        .to_string(),
                );
            }
            BnbResult::Found(best, nodes) => (best, nodes),
        };

    if let Some(b) = best.clone() {
        if !int_idx.is_empty() {
            best = Some(integer_neighborhood_search(&mut c, &lo0, &hi0, &int_idx, b));
        }
    }

    // Certify (or improve) the result with interval branch-and-bound when the box is finite
    let local_method = if int_idx.is_empty() {
        "augmented Lagrangian".to_string()
    } else {
        format!(
            "augmented Lagrangian branch-and-bound ({} nodes)",
            nodes.min(NODE_LIMIT)
        )
    };
    match interval_global_search(&c, &lo0, &hi0, &int_idx, best.clone()) {
        Certificate::Optimal {
            x,
            objective,
            nodes: boxes,
        } => Outcome::Optimal {
            x,
            objective,
            proven: true,
            method: format!(
                "{}, certified by interval branch-and-bound ({} boxes, gap <= {:e} relative)",
                local_method, boxes, INTERVAL_GAP
            ),
        },
        Certificate::Infeasible { nodes: boxes } => Outcome::Infeasible(format!(
            "no point satisfies every constraint (proven by interval branch-and-bound over {} boxes)",
            boxes
        )),
        Certificate::Unproven(improved) => match improved.or(best) {
            Some((x, objective)) => Outcome::Optimal {
                x,
                objective,
                proven: false,
                method: format!("{} (local optimum)", local_method),
            },
            None => Outcome::Infeasible(
                "no feasible point found from any starting point (nonlinear model; not a proof of infeasibility)"
                    .to_string(),
            ),
        },
    }
}

// ===========================================================================
// Interval branch-and-bound: global optimality / infeasibility certificates
// ===========================================================================

/// Relative optimality gap the interval search certifies.
const INTERVAL_GAP: f64 = 1e-6;

/// A closed interval [lo, hi]; may be unbounded.
#[derive(Debug, Clone, Copy)]
struct Iv {
    lo: f64,
    hi: f64,
}

impl Iv {
    const ENTIRE: Iv = Iv {
        lo: f64::NEG_INFINITY,
        hi: f64::INFINITY,
    };

    fn point(v: f64) -> Iv {
        Iv { lo: v, hi: v }
    }

    /// Widen outward by a few ulps so floating-point rounding cannot make a bound too tight.
    fn widen(self) -> Iv {
        if self.lo.is_nan() || self.hi.is_nan() {
            return Iv::ENTIRE;
        }
        // One IEEE operation errs by at most half an ulp of its result, so a relative pad
        // (plus the smallest subnormal) is sound, and an exact zero stays exact. Padding a
        // zero would turn 0 * inf into -inf and destroy bounds on unbounded variables.
        let pad = |v: f64| {
            if v == 0.0 {
                0.0
            } else {
                4.0 * f64::EPSILON * v.abs() + 5e-324
            }
        };
        Iv {
            lo: self.lo - pad(self.lo),
            hi: self.hi + pad(self.hi),
        }
    }

    fn contains_zero(&self) -> bool {
        self.lo <= 0.0 && self.hi >= 0.0
    }

    fn monotone(self, f: impl Fn(f64) -> f64) -> Iv {
        Iv {
            lo: f(self.lo),
            hi: f(self.hi),
        }
    }

    fn mul(a: Iv, b: Iv) -> Iv {
        // 0 * inf is taken as 0: a zero endpoint times an unbounded factor
        let m = |x: f64, y: f64| if x == 0.0 || y == 0.0 { 0.0 } else { x * y };
        let c = [m(a.lo, b.lo), m(a.lo, b.hi), m(a.hi, b.lo), m(a.hi, b.hi)];
        Iv {
            lo: c.iter().cloned().fold(f64::INFINITY, f64::min),
            hi: c.iter().cloned().fold(f64::NEG_INFINITY, f64::max),
        }
    }

    fn powi(a: Iv, n: i32) -> Iv {
        if n == 0 {
            return Iv::point(1.0);
        }
        if n < 0 {
            let p = Iv::powi(a, -n);
            if p.contains_zero() {
                return Iv::ENTIRE;
            }
            return Iv {
                lo: 1.0 / p.hi,
                hi: 1.0 / p.lo,
            };
        }
        if n % 2 == 1 {
            return a.monotone(|v| v.powi(n));
        }
        let (l, h) = (a.lo.abs().powi(n), a.hi.abs().powi(n));
        if a.contains_zero() {
            Iv { lo: 0.0, hi: l.max(h) }
        } else {
            Iv {
                lo: l.min(h),
                hi: l.max(h),
            }
        }
    }

    /// Range of sin over the interval (cos uses a phase shift).
    fn sin(a: Iv) -> Iv {
        use std::f64::consts::{FRAC_PI_2, TAU};
        if !(a.hi - a.lo < TAU) {
            return Iv { lo: -1.0, hi: 1.0 };
        }
        let mut lo = a.lo.sin().min(a.hi.sin());
        let mut hi = a.lo.sin().max(a.hi.sin());
        // maxima at pi/2 + 2k pi, minima at -pi/2 + 2k pi
        let has = |center: f64| {
            let k = ((a.lo - center) / TAU).ceil();
            center + k * TAU <= a.hi
        };
        if has(FRAC_PI_2) {
            hi = 1.0;
        }
        if has(-FRAC_PI_2) {
            lo = -1.0;
        }
        Iv { lo, hi }
    }
}

impl CExpr {
    /// Guaranteed enclosure of the expression's values over the box `b`.
    fn ieval(&self, b: &[Iv]) -> Iv {
        let r = match self {
            CExpr::Const(c) => Iv::point(*c),
            CExpr::Var(i) => b[*i],
            CExpr::Neg(a) => {
                let v = a.ieval(b);
                Iv { lo: -v.hi, hi: -v.lo }
            }
            CExpr::Bin(op, l, r) => {
                let (x, y) = (l.ieval(b), r.ieval(b));
                match op {
                    BinaryOp::Add => Iv {
                        lo: x.lo + y.lo,
                        hi: x.hi + y.hi,
                    },
                    BinaryOp::Sub => Iv {
                        lo: x.lo - y.hi,
                        hi: x.hi - y.lo,
                    },
                    BinaryOp::Mul => Iv::mul(x, y),
                    BinaryOp::Div => {
                        if y.contains_zero() {
                            Iv::ENTIRE
                        } else {
                            Iv::mul(
                                x,
                                Iv {
                                    lo: 1.0 / y.hi,
                                    hi: 1.0 / y.lo,
                                },
                            )
                        }
                    }
                    BinaryOp::Pow => {
                        // Exponents are read from the expression: an interval of a constant
                        // has been widened and would no longer look like an exact power
                        let exponent = match r.as_ref() {
                            CExpr::Const(p) => Some(*p),
                            _ => None,
                        };
                        if let Some(p) = exponent.filter(|p| p.fract() == 0.0 && p.abs() < 64.0) {
                            Iv::powi(x, p as i32)
                        } else if let Some(p) = exponent.filter(|_| x.lo >= 0.0) {
                            // real exponent on a non-negative base: monotone in the base
                            if p >= 0.0 {
                                x.monotone(|v| v.powf(p))
                            } else if x.lo > 0.0 {
                                Iv {
                                    lo: x.hi.powf(p),
                                    hi: x.lo.powf(p),
                                }
                            } else {
                                Iv::ENTIRE
                            }
                        } else {
                            Iv::ENTIRE
                        }
                    }
                }
            }
            CExpr::Call(f, arg) => {
                let a = arg.ieval(b);
                let snap = |v: f64, r: f64| {
                    if (v - v.round()).abs() < 1e-9 {
                        v.round()
                    } else {
                        r
                    }
                };
                let sign = |v: f64| {
                    if v > 0.0 {
                        1.0
                    } else if v < 0.0 {
                        -1.0
                    } else {
                        0.0
                    }
                };
                match f.as_str() {
                    "sqrt" if a.hi >= 0.0 => Iv {
                        lo: a.lo.max(0.0).sqrt(),
                        hi: a.hi.sqrt(),
                    },
                    "exp" => a.monotone(f64::exp),
                    "log" | "ln" if a.hi > 0.0 => Iv {
                        lo: if a.lo > 0.0 {
                            a.lo.ln()
                        } else {
                            f64::NEG_INFINITY
                        },
                        hi: a.hi.ln(),
                    },
                    "abs" => {
                        let (l, h) = (a.lo.abs(), a.hi.abs());
                        if a.contains_zero() {
                            Iv { lo: 0.0, hi: l.max(h) }
                        } else {
                            Iv {
                                lo: l.min(h),
                                hi: l.max(h),
                            }
                        }
                    }
                    "ceil" => Iv {
                        lo: snap(a.lo, a.lo.ceil()),
                        hi: snap(a.hi, a.hi.ceil()),
                    },
                    "floor" => Iv {
                        lo: snap(a.lo, a.lo.floor()),
                        hi: snap(a.hi, a.hi.floor()),
                    },
                    "round" => a.monotone(f64::round),
                    "sign" => a.monotone(sign),
                    "sin" => Iv::sin(a),
                    "cos" => Iv::sin(Iv {
                        lo: a.lo + std::f64::consts::FRAC_PI_2,
                        hi: a.hi + std::f64::consts::FRAC_PI_2,
                    }),
                    "tan" => {
                        // monotone between poles; a pole inside gives no bound
                        let k = ((a.lo - std::f64::consts::FRAC_PI_2) / std::f64::consts::PI).ceil();
                        if std::f64::consts::FRAC_PI_2 + k * std::f64::consts::PI <= a.hi {
                            Iv::ENTIRE
                        } else {
                            a.monotone(f64::tan)
                        }
                    }
                    _ => Iv::ENTIRE,
                }
            }
        };
        match self {
            CExpr::Const(_) | CExpr::Var(_) => r,
            _ => r.widen(),
        }
    }
}

enum Certificate {
    /// Global optimum within INTERVAL_GAP over the whole box
    Optimal {
        x: Vec<f64>,
        objective: f64,
        nodes: usize,
    },
    /// No point of the box satisfies the constraints
    Infeasible { nodes: usize },
    /// Search incomplete; carries a better point if one was found
    Unproven(Option<(Vec<f64>, f64)>),
}

/// Interval branch-and-bound over [lo, hi]. Discards boxes whose objective lower bound cannot
/// beat the incumbent by more than the gap, or whose constraints cannot all hold; a finished
/// search certifies the incumbent as globally optimal (or the model as infeasible).
fn interval_global_search(
    c: &Compiled,
    lo: &[f64],
    hi: &[f64],
    int_idx: &[usize],
    incumbent: Option<(Vec<f64>, f64)>,
) -> Certificate {
    const BOX_LIMIT: usize = 200_000;
    let n = lo.len();
    let is_int: Vec<bool> = (0..n).map(|j| int_idx.contains(&j)).collect();
    let feasible = |x: &[f64]| {
        let scale = 1.0 + x.iter().map(|v| v.abs()).fold(0.0, f64::max);
        c.infeasibility(x) <= 1e-7 * scale
    };
    // Matches the point-feasibility tolerance (1e-7 relative to the largest coordinate), so
    // pruning never discards a point the solver would accept
    let feas_tol = |b: &[Iv]| {
        let big = b
            .iter()
            .flat_map(|v| [v.lo.abs(), v.hi.abs()])
            .fold(0.0, f64::max);
        if big.is_finite() { 1e-7 * (1.0 + big) } else { 0.0 }
    };

    let mut best = incumbent;
    let mut improved = false;
    let mut certified = true;
    let root: Vec<Iv> = (0..n)
        .map(|j| {
            if is_int[j] {
                Iv {
                    lo: (lo[j] - 1e-9).ceil(),
                    hi: (hi[j] + 1e-9).floor(),
                }
            } else {
                Iv { lo: lo[j], hi: hi[j] }
            }
        })
        .collect();
    let mut stack = vec![root];
    let mut nodes = 0usize;

    while let Some(mut bx) = stack.pop() {
        nodes += 1;
        if nodes > BOX_LIMIT {
            certified = false;
            break;
        }
        if bx.iter().any(|v| v.lo > v.hi) {
            continue;
        }
        // Constraint propagation (with the objective cut) shrinks the box, or proves it empty
        let tol = feas_tol(&bx);
        if !contract(c, &mut bx, &is_int, tol, best.as_ref().map(|(_, ub)| *ub)) {
            continue;
        }
        if bx.iter().any(|v| !v.lo.is_finite() || !v.hi.is_finite()) {
            // Unbounded even after propagation: no finite search can cover it
            certified = false;
            continue;
        }
        let tol = feas_tol(&bx);
        // Constraint pruning
        if c.eqs.iter().any(|h| {
            let v = h.f.ieval(&bx);
            v.lo > tol || v.hi < -tol
        }) {
            continue;
        }
        if (0..c.ineqs.len()).any(|i| {
            let (g, strict) = &c.ineqs[i];
            g.f.ieval(&bx).lo + if *strict { STRICT_MARGIN } else { 0.0 } > tol
        }) {
            continue;
        }
        // Objective bound pruning
        let bound = c.obj.as_ref().map_or(Iv::point(0.0), |o| o.f.ieval(&bx));
        if let Some((_, ub)) = &best {
            if bound.lo >= ub - INTERVAL_GAP * (1.0 + ub.abs()) {
                continue;
            }
        }
        // Probe the box centre for a better feasible point
        let mid: Vec<f64> = (0..n)
            .map(|j| {
                let m = 0.5 * (bx[j].lo + bx[j].hi);
                if is_int[j] {
                    m.round().clamp(bx[j].lo, bx[j].hi)
                } else {
                    m
                }
            })
            .collect();
        if feasible(&mid) {
            let f = c.objective(&mid);
            let better = best
                .as_ref()
                .map_or(true, |(_, ub)| f < *ub - 1e-12 * (1.0 + ub.abs()));
            if f.is_finite() && better {
                best = Some((mid.clone(), f));
                improved = true;
                if bound.lo >= f - INTERVAL_GAP * (1.0 + f.abs()) {
                    continue;
                }
            }
        }
        // Split the widest dimension (integers split between whole numbers)
        let (j, width) = (0..n)
            .map(|j| (j, bx[j].hi - bx[j].lo))
            .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
            .unwrap_or((0, 0.0));
        let tiny = 1e-10 * (1.0 + bx[j].lo.abs().max(bx[j].hi.abs()));
        if (is_int[j] && width < 0.5) || (!is_int[j] && width <= tiny) {
            // A point-sized box that can be neither pruned nor improved: the remaining doubt
            // is numerical, so the search cannot certify
            certified = false;
            continue;
        }
        let (mut left, mut right) = (bx.clone(), bx);
        if is_int[j] {
            let cut = (0.5 * (left[j].lo + left[j].hi)).floor();
            left[j].hi = cut;
            right[j].lo = cut + 1.0;
        } else {
            let m = 0.5 * (left[j].lo + left[j].hi);
            left[j].hi = m;
            right[j].lo = m;
        }
        stack.push(right);
        stack.push(left);
    }

    if !certified {
        return Certificate::Unproven(if improved { best } else { None });
    }
    match best {
        Some((x, objective)) => Certificate::Optimal {
            x,
            objective,
            nodes,
        },
        None => Certificate::Infeasible { nodes },
    }
}

// ---------------------------------------------------------------------------
// HC4 constraint propagation: narrows a box to the values a constraint allows
// ---------------------------------------------------------------------------

impl Iv {
    fn meet(self, o: Iv) -> Iv {
        Iv {
            lo: self.lo.max(o.lo),
            hi: self.hi.min(o.hi),
        }
    }

    fn is_empty(&self) -> bool {
        self.lo > self.hi
    }

    fn excludes_zero(&self) -> bool {
        self.lo > 0.0 || self.hi < 0.0
    }

    /// Division for narrowing; None when the divisor contains zero (no information)
    fn div(a: Iv, b: Iv) -> Option<Iv> {
        if b.excludes_zero() {
            Some(Iv::mul(
                a,
                Iv {
                    lo: 1.0 / b.hi,
                    hi: 1.0 / b.lo,
                },
            ))
        } else {
            None
        }
    }
}

impl CExpr {
    /// Backward pass of HC4Revise: given that this expression's value lies in `z`, narrows
    /// the variables in `b`. Returns false if the box holds no consistent value.
    fn narrow(&self, b: &mut [Iv], z: Iv) -> bool {
        let z = z.meet(self.ieval(b));
        if z.is_empty() {
            return false;
        }
        match self {
            CExpr::Const(_) => true,
            CExpr::Var(i) => {
                b[*i] = b[*i].meet(z);
                !b[*i].is_empty()
            }
            CExpr::Neg(a) => a.narrow(b, Iv { lo: -z.hi, hi: -z.lo }),
            CExpr::Bin(op, l, r) => {
                let (lv, rv) = (l.ieval(b), r.ieval(b));
                match op {
                    BinaryOp::Add => {
                        l.narrow(b, Iv { lo: z.lo - rv.hi, hi: z.hi - rv.lo }.widen())
                            && r.narrow(b, {
                                let lv = l.ieval(b);
                                Iv { lo: z.lo - lv.hi, hi: z.hi - lv.lo }.widen()
                            })
                    }
                    BinaryOp::Sub => {
                        l.narrow(b, Iv { lo: z.lo + rv.lo, hi: z.hi + rv.hi }.widen())
                            && r.narrow(b, {
                                let lv = l.ieval(b);
                                Iv { lo: lv.lo - z.hi, hi: lv.hi - z.lo }.widen()
                            })
                    }
                    BinaryOp::Mul => {
                        if let Some(t) = Iv::div(z, rv) {
                            if !l.narrow(b, t.widen()) {
                                return false;
                            }
                        }
                        match Iv::div(z, l.ieval(b)) {
                            Some(t) => r.narrow(b, t.widen()),
                            None => true,
                        }
                    }
                    BinaryOp::Div => {
                        // l = z * r ; r = l / z
                        if !l.narrow(b, Iv::mul(z, rv).widen()) {
                            return false;
                        }
                        match Iv::div(l.ieval(b), z) {
                            Some(t) if lv.excludes_zero() => r.narrow(b, t.widen()),
                            _ => true,
                        }
                    }
                    BinaryOp::Pow => {
                        let n = match r.as_ref() {
                            CExpr::Const(c) if c.fract() == 0.0 && *c >= 1.0 && *c < 64.0 => *c as i32,
                            _ => return true,
                        };
                        let root = |v: f64| {
                            if v.is_infinite() {
                                v
                            } else if v < 0.0 {
                                -(-v).powf(1.0 / n as f64)
                            } else {
                                v.powf(1.0 / n as f64)
                            }
                        };
                        if n % 2 == 1 {
                            l.narrow(b, Iv { lo: root(z.lo), hi: root(z.hi) }.widen())
                        } else {
                            if z.hi < 0.0 {
                                return false;
                            }
                            let m = root(z.hi);
                            l.narrow(b, Iv { lo: -m, hi: m }.widen())
                        }
                    }
                }
            }
            CExpr::Call(f, a) => match f.as_str() {
                "exp" => {
                    if z.hi <= 0.0 {
                        return false;
                    }
                    let lo = if z.lo > 0.0 { z.lo.ln() } else { f64::NEG_INFINITY };
                    a.narrow(b, Iv { lo, hi: z.hi.ln() }.widen())
                }
                "ln" | "log" => a.narrow(b, z.monotone(f64::exp).widen()),
                "sqrt" => {
                    if z.hi < 0.0 {
                        return false;
                    }
                    let lo = z.lo.max(0.0);
                    a.narrow(b, Iv { lo: lo * lo, hi: z.hi * z.hi }.widen())
                }
                _ => true,
            },
        }
    }
}

/// Narrows `b` with every constraint (and the objective cut `obj <= cut`) until it stops
/// shrinking. Integer variables are rounded inward. Returns false if the box is infeasible.
fn contract(c: &Compiled, b: &mut [Iv], is_int: &[bool], tol: f64, cut: Option<f64>) -> bool {
    for _ in 0..30 {
        let before: Vec<Iv> = b.to_vec();
        for h in &c.eqs {
            if !h.f.narrow(b, Iv { lo: -tol, hi: tol }) {
                return false;
            }
        }
        for (g, strict) in &c.ineqs {
            let limit = tol - if *strict { STRICT_MARGIN } else { 0.0 };
            if !g.f.narrow(b, Iv { lo: f64::NEG_INFINITY, hi: limit }) {
                return false;
            }
        }
        if let (Some(o), Some(ub)) = (&c.obj, cut) {
            if !o.f.narrow(b, Iv { lo: f64::NEG_INFINITY, hi: ub }) {
                return false;
            }
        }
        for j in 0..b.len() {
            if is_int[j] {
                b[j].lo = (b[j].lo - 1e-9).ceil();
                b[j].hi = (b[j].hi + 1e-9).floor();
            }
            if b[j].is_empty() {
                return false;
            }
        }
        let shrank = b.iter().zip(&before).any(|(n, o)| {
            let w_old = o.hi - o.lo;
            let w_new = n.hi - n.lo;
            (w_old.is_infinite() && w_new.is_finite()) || w_new < 0.99 * w_old
        });
        if !shrank {
            break;
        }
    }
    true
}
