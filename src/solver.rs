// src/solver.rs — The Omnidirectional Relation Engine
// Combines forward propagation, 1D backward Newton, and coupled multi-variable Newton.
use crate::ast::*;
use crate::diff::{diff, eval_expr, simplify};
use std::collections::{BTreeSet, HashMap};
use std::fmt;

#[derive(Debug, Clone)]
pub enum SolverError {
    Inconsistency {
        relation: String,
        lhs: String,
        expected: f64,
        given: f64,
    },
    ConstraintViolation {
        constraint: String,
        lhs: String,
        actual: f64,
        bound: f64,
    },
    UnderDetermined(crate::diagnostics::Diagnosis),
    Infeasible(String),
    MathError(String),
}

impl fmt::Display for SolverError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SolverError::Inconsistency {
                relation,
                lhs,
                expected,
                given,
            } => write!(
                f,
                "Inconsistency in '{}': implies {} = {:.4}, but {} is bound to {:.4}",
                relation, lhs, expected, lhs, given
            ),
            SolverError::ConstraintViolation {
                constraint,
                lhs,
                actual,
                bound,
            } => write!(
                f,
                "Constraint violation in '{}': {} = {:.4} violates bound {:.4}",
                constraint, lhs, actual, bound
            ),
            SolverError::UnderDetermined(diag) => {
                write!(
                    f,
                    "System is under-determined ({} degree(s) of freedom):\n{}",
                    diag.dof,
                    diag.report()
                )
            }
            SolverError::Infeasible(msg) => write!(f, "No feasible solution found: {}", msg),
            SolverError::MathError(msg) => write!(f, "Solver math error: {}", msg),
        }
    }
}

impl std::error::Error for SolverError {}

const SEEDS_1D: &[f64] = &[1.0, 0.5, 2.0, 10.0, 0.1, -1.0, 100.0, -10.0];
const SEEDS_COUPLED: &[f64] = &[1.0, 0.5, 2.0, 10.0, 0.1, -1.0];

#[derive(Debug, Clone)]
pub struct SolverEngine {
    pub relations: Vec<Equation>,
    pub constraints: Vec<Constraint>,
    pub objectives: Vec<Objective>,
    pub integer_vars: BTreeSet<String>,
    pub trace: Vec<String>,
    pub diagnosis: Option<crate::diagnostics::Diagnosis>,
}

impl Default for SolverEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl SolverEngine {
    pub fn new() -> Self {
        Self {
            relations: Vec::new(),
            constraints: Vec::new(),
            objectives: Vec::new(),
            integer_vars: BTreeSet::new(),
            trace: Vec::new(),
            diagnosis: None,
        }
    }

    pub fn add_relation(&mut self, eq: Equation) {
        if eq.op == ConstraintOp::Equal {
            if !self
                .relations
                .iter()
                .any(|e| e.lhs == eq.lhs && format!("{}", e.rhs) == format!("{}", eq.rhs))
            {
                self.relations.push(eq);
            }
        } else {
            self.add_constraint(Constraint {
                lhs: eq.lhs,
                op: eq.op,
                rhs: eq.rhs,
                original_text: eq.original_text,
                line: eq.line,
                col: eq.col,
            });
        }
    }

    pub fn add_constraint(&mut self, c: Constraint) {
        if !self.constraints.iter().any(|existing| {
            existing.lhs == c.lhs
                && existing.op == c.op
                && format!("{}", existing.rhs) == format!("{}", c.rhs)
        }) {
            self.constraints.push(c);
        }
    }

    pub fn add_objective(&mut self, obj: Objective) {
        self.objectives.push(obj);
    }

    pub fn add_integer_var(&mut self, var: String) {
        self.integer_vars.insert(var);
    }

    pub fn check_constraint(
        c: &Constraint,
        env: &HashMap<String, f64>,
    ) -> Result<bool, SolverError> {
        let lhs_val = match env.get(&c.lhs) {
            Some(&v) => v,
            None => return Ok(true),
        };
        let rhs_val = match eval_expr(&c.rhs, env) {
            Ok(v) => v,
            Err(_) => return Ok(true),
        };
        let tol = 1e-6 * (1.0 + rhs_val.abs());
        let satisfied = match c.op {
            ConstraintOp::Equal => (lhs_val - rhs_val).abs() <= tol,
            ConstraintOp::NotEqual => (lhs_val - rhs_val).abs() > tol,
            ConstraintOp::LessEqual => lhs_val <= rhs_val + tol,
            ConstraintOp::GreaterEqual => lhs_val >= rhs_val - tol,
            ConstraintOp::Less => lhs_val < rhs_val - 1e-9,
            ConstraintOp::Greater => lhs_val > rhs_val + 1e-9,
        };
        Ok(satisfied)
    }

    pub fn solve(
        &mut self,
        initial_bindings: &HashMap<String, f64>,
    ) -> Result<HashMap<String, f64>, SolverError> {
        let mut env = initial_bindings.clone();
        self.trace.clear();

        // 1. Solve pure equality relations
        let pure_eqs: Vec<Equation> = self
            .relations
            .iter()
            .filter(|e| e.op == ConstraintOp::Equal)
            .cloned()
            .collect();

        self.solve_pure_equalities(&pure_eqs, &mut env)?;

        // 2. If constraints or objectives exist, run optimization
        if !self.constraints.is_empty() || !self.objectives.is_empty() {
            self.solve_with_optimization(&mut env)?;
        }

        // 3. Verify feasibility of all constraints against settled env
        for c in &self.constraints {
            if !Self::check_constraint(c, &env)? {
                let lhs_val = env.get(&c.lhs).copied().unwrap_or(0.0);
                let rhs_val = eval_expr(&c.rhs, &env).unwrap_or(0.0);
                return Err(SolverError::ConstraintViolation {
                    constraint: c.original_text.clone(),
                    lhs: c.lhs.clone(),
                    actual: lhs_val,
                    bound: rhs_val,
                });
            }
        }

        // 4. Clean integer variable values
        for iv in &self.integer_vars {
            if let Some(val) = env.get_mut(iv) {
                let rounded = val.round();
                if (*val - rounded).abs() < 1e-4 {
                    *val = rounded;
                }
            }
        }

        // 5. Check if any unknowns remain unconstrained
        let mut all_vars = BTreeSet::new();
        for eq in &self.relations {
            all_vars.insert(eq.lhs.clone());
            for v in eq.rhs.vars() {
                all_vars.insert(v);
            }
        }
        let remaining_unknowns: Vec<String> = all_vars
            .into_iter()
            .filter(|v| !env.contains_key(v))
            .collect();

        if !remaining_unknowns.is_empty() {
            let blocked: Vec<Equation> = self
                .relations
                .iter()
                .filter(|eq| {
                    let mut v = eq.rhs.vars();
                    v.insert(eq.lhs.clone());
                    v.iter().any(|var| remaining_unknowns.contains(var))
                })
                .cloned()
                .collect();

            let d = crate::diagnostics::Diagnostician::new().diagnose(
                &remaining_unknowns,
                &blocked,
                &env,
            );
            self.diagnosis = Some(d.clone());
            return Err(SolverError::UnderDetermined(d));
        }

        Ok(env)
    }

    fn solve_pure_equalities(
        &mut self,
        relations: &[Equation],
        env: &mut HashMap<String, f64>,
    ) -> Result<bool, SolverError> {
        let mut overall_progressed = false;
        for _round in 0..300 {
            let mut progressed = false;
            let mut blocked = Vec::new();

            for eq in relations {
                let mut vars = eq.rhs.vars();
                vars.insert(eq.lhs.clone());

                let unknowns: Vec<String> =
                    vars.into_iter().filter(|v| !env.contains_key(v)).collect();

                if unknowns.is_empty() {
                    // All variables known: verify mathematical consistency
                    let expected = eval_expr(&eq.rhs, env)
                        .map_err(|e| SolverError::MathError(e.to_string()))?;
                    let given = *env.get(&eq.lhs).unwrap();
                    let tol = 1e-6 * 1.0f64.max(given.abs());
                    if (expected - given).abs() > tol {
                        return Err(SolverError::Inconsistency {
                            relation: eq.original_text.clone(),
                            lhs: eq.lhs.clone(),
                            expected,
                            given,
                        });
                    }
                } else if unknowns.len() == 1 {
                    let u = &unknowns[0];
                    if u == &eq.lhs {
                        // Forward evaluation: lhs is unknown, rhs is completely known
                        let val = eval_expr(&eq.rhs, env)
                            .map_err(|e| SolverError::MathError(e.to_string()))?;
                        env.insert(u.clone(), val);
                        self.trace
                            .push(format!("[{}] {} <- forward eval", eq.original_text, u));
                        progressed = true;
                        overall_progressed = true;
                    } else if env.contains_key(&eq.lhs) {
                        // 1D Backward solving: solve rhs(..., u=x, ...) = lhs
                        if let Some(x) = self.solve_one_unknown(eq, u, env) {
                            env.insert(u.clone(), x);
                            self.trace
                                .push(format!("[{}] {} <- backward Newton", eq.original_text, u));
                            progressed = true;
                            overall_progressed = true;
                        } else {
                            blocked.push(eq.clone());
                        }
                    } else {
                        blocked.push(eq.clone());
                    }
                } else {
                    blocked.push(eq.clone());
                }
            }

            // Coupled multi-unknown simultaneous solving attempt
            if !progressed && !blocked.is_empty() {
                if let Some(solved_env) = self.try_coupled(&blocked, env) {
                    *env = solved_env;
                    progressed = true;
                    overall_progressed = true;
                }
            }

            if !progressed {
                break;
            }
        }

        Ok(overall_progressed)
    }

    fn solve_with_optimization(
        &mut self,
        env: &mut HashMap<String, f64>,
    ) -> Result<bool, SolverError> {
        let mut all_vars = BTreeSet::new();
        for eq in &self.relations {
            all_vars.insert(eq.lhs.clone());
            for v in eq.rhs.vars() {
                all_vars.insert(v);
            }
        }
        for c in &self.constraints {
            all_vars.insert(c.lhs.clone());
            for v in c.rhs.vars() {
                all_vars.insert(v);
            }
        }
        for obj in &self.objectives {
            all_vars.insert(obj.target.clone());
        }

        let remaining_unknowns: Vec<String> = all_vars
            .into_iter()
            .filter(|v| !env.contains_key(v))
            .collect();

        if remaining_unknowns.is_empty() {
            return Ok(false);
        }

        let primary_obj = self.objectives.first().cloned();
        let base_relations = self.relations.clone();
        let constraints_list = self.constraints.clone();

        let compute_objective = |cand_env: &HashMap<String, f64>| -> Option<f64> {
            if let Some(ref obj) = primary_obj {
                cand_env.get(&obj.target).copied()
            } else {
                Some(0.0)
            }
        };

        let is_better = |new_score: f64, current_best: f64| -> bool {
            if let Some(ref obj) = primary_obj {
                match obj.kind {
                    ObjectiveKind::Minimize => new_score < current_best - 1e-9,
                    ObjectiveKind::Maximize => new_score > current_best + 1e-9,
                }
            } else {
                false
            }
        };

        struct CandidateSolution {
            env: HashMap<String, f64>,
            objective_val: f64,
        }
        let mut best_solution: Option<CandidateSolution> = None;

        // Form active boundary candidate sets from inequality constraints
        let mut boundary_sets: Vec<Vec<Equation>> = Vec::new();

        // 1-boundary sets
        for c in &constraints_list {
            boundary_sets.push(vec![Equation {
                lhs: c.lhs.clone(),
                op: ConstraintOp::Equal,
                rhs: c.rhs.clone(),
                original_text: format!("{} = {}", c.lhs, c.rhs),
                line: c.line,
                col: c.col,
            }]);
        }

        // 2-boundary sets if multiple constraints exist
        if constraints_list.len() > 1 {
            for i in 0..constraints_list.len() {
                for j in (i + 1)..constraints_list.len() {
                    let c1 = &constraints_list[i];
                    let c2 = &constraints_list[j];
                    boundary_sets.push(vec![
                        Equation {
                            lhs: c1.lhs.clone(),
                            op: ConstraintOp::Equal,
                            rhs: c1.rhs.clone(),
                            original_text: format!("{} = {}", c1.lhs, c1.rhs),
                            line: c1.line,
                            col: c1.col,
                        },
                        Equation {
                            lhs: c2.lhs.clone(),
                            op: ConstraintOp::Equal,
                            rhs: c2.rhs.clone(),
                            original_text: format!("{} = {}", c2.lhs, c2.rhs),
                            line: c2.line,
                            col: c2.col,
                        },
                    ]);
                }
            }
        }

        for boundary_eqs in boundary_sets {
            let mut test_relations = base_relations.clone();
            for b in &boundary_eqs {
                test_relations.push(b.clone());
            }

            let mut relaxed_env = env.clone();
            if self
                .solve_pure_equalities(&test_relations, &mut relaxed_env)
                .is_ok()
            {
                let still_unknown: Vec<&String> = remaining_unknowns
                    .iter()
                    .filter(|u| !relaxed_env.contains_key(*u))
                    .collect();

                if !still_unknown.is_empty() {
                    continue;
                }

                let free_int_vars: Vec<String> = self
                    .integer_vars
                    .iter()
                    .filter(|iv| remaining_unknowns.contains(iv))
                    .cloned()
                    .collect();

                if free_int_vars.is_empty() {
                    let mut feasible = true;
                    for c in &constraints_list {
                        if !Self::check_constraint(c, &relaxed_env)? {
                            feasible = false;
                            break;
                        }
                    }
                    if feasible {
                        if let Some(score) = compute_objective(&relaxed_env) {
                            let update = match &best_solution {
                                None => true,
                                Some(best) => is_better(score, best.objective_val),
                            };
                            if update {
                                best_solution = Some(CandidateSolution {
                                    env: relaxed_env.clone(),
                                    objective_val: score,
                                });
                            }
                        }
                    }
                } else {
                    self.trace.push(format!(
                        "[boundary] relaxed boundary {} => continuous values: {}",
                        boundary_eqs
                            .iter()
                            .map(|b| b.original_text.as_str())
                            .collect::<Vec<_>>()
                            .join(", "),
                        free_int_vars
                            .iter()
                            .map(|v| format!("{} = {:.4}", v, relaxed_env[v]))
                            .collect::<Vec<_>>()
                            .join(", ")
                    ));

                    let mut int_combinations: Vec<Vec<(String, f64)>> = vec![vec![]];
                    for var in &free_int_vars {
                        let x = relaxed_env[var];
                        let center = x.round() as i64;
                        let mut cands = vec![x.floor() as i64, x.ceil() as i64];
                        for delta in -2..=2 {
                            cands.push(center + delta);
                        }
                        cands.sort();
                        cands.dedup();

                        let mut next_combinations = Vec::new();
                        for combo in &int_combinations {
                            for &cand in &cands {
                                let mut new_combo = combo.clone();
                                new_combo.push((var.clone(), cand as f64));
                                next_combinations.push(new_combo);
                            }
                        }
                        int_combinations = next_combinations;
                    }

                    for combo in int_combinations {
                        let mut cand_env = env.clone();
                        for (v, val) in &combo {
                            cand_env.insert(v.clone(), *val);
                        }

                        if self
                            .solve_pure_equalities(&base_relations, &mut cand_env)
                            .is_ok()
                        {
                            let unresolved: Vec<&String> = remaining_unknowns
                                .iter()
                                .filter(|u| !cand_env.contains_key(*u))
                                .collect();

                            if unresolved.is_empty() {
                                let mut feasible = true;
                                for c in &constraints_list {
                                    if !Self::check_constraint(c, &cand_env)? {
                                        feasible = false;
                                        break;
                                    }
                                }
                                if feasible {
                                    if let Some(score) = compute_objective(&cand_env) {
                                        let update = match &best_solution {
                                            None => true,
                                            Some(best) => is_better(score, best.objective_val),
                                        };
                                        if update {
                                            let combo_str = combo
                                                .iter()
                                                .map(|(v, val)| format!("{} = {:.0}", v, val))
                                                .collect::<Vec<_>>()
                                                .join(", ");
                                            self.trace.push(format!(
                                                "[branch-and-bound] candidate {} is feasible (objective = {:.4})",
                                                combo_str, score
                                            ));
                                            best_solution = Some(CandidateSolution {
                                                env: cand_env.clone(),
                                                objective_val: score,
                                            });
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        if let Some(best) = best_solution {
            *env = best.env;
            self.trace.push(format!(
                "[optimize] settled optimal solution with objective = {:.4}",
                best.objective_val
            ));
            Ok(true)
        } else {
            Err(SolverError::Infeasible(
                "constraints could not be satisfied by any candidate integer or boundary values"
                    .to_string(),
            ))
        }
    }

    fn solve_one_unknown(&self, eq: &Equation, u: &str, env: &HashMap<String, f64>) -> Option<f64> {
        let target = *env.get(&eq.lhs)?;

        // Discrete wrapper direct inversion:
        // ceil(inner) = target, floor(inner) = target, round(inner) = target
        if let Expr::Call(ref fn_name, ref inner) = eq.rhs {
            if matches!(fn_name.as_str(), "ceil" | "floor" | "round") {
                // Ceil, floor, round only return whole numbers.
                if target.fract().abs() > 1e-6 {
                    return None;
                }
                // Invert wrapper: for ceil(u) = k, solve inner = k
                let unwrap_eq = Equation {
                    lhs: eq.lhs.clone(),
                    op: eq.op,
                    rhs: (**inner).clone(),
                    original_text: format!("{} = {}", eq.lhs, inner),
                    line: eq.line,
                    col: eq.col,
                };
                return self.solve_one_unknown(&unwrap_eq, u, env);
            }
        }

        let g = |x: f64| -> f64 {
            let mut local = env.clone();
            local.insert(u.to_string(), x);
            match eval_expr(&eq.rhs, &local) {
                Ok(val) => val - target,
                Err(_) => f64::NAN,
            }
        };

        let deriv_expr = diff(&eq.rhs, u).ok();

        let dg = |x: f64| -> Option<f64> {
            if let Some(ref d) = deriv_expr {
                let mut local = env.clone();
                local.insert(u.to_string(), x);
                if let Ok(val) = eval_expr(d, &local) {
                    if val.is_finite() && val.abs() > 1e-12 {
                        return Some(val);
                    }
                }
            }
            // Central-difference numerical fallback
            let h = 1e-6 * 1.0f64.max(x.abs());
            let gp = g(x + h);
            let gm = g(x - h);
            if gp.is_finite() && gm.is_finite() {
                let d = (gp - gm) / (2.0 * h);
                if d.is_finite() && d.abs() > 1e-12 {
                    return Some(d);
                }
            }
            None
        };

        for &x0 in SEEDS_1D {
            let mut x = x0;
            for _ in 0..60 {
                let fx = g(x);
                if !fx.is_finite() {
                    break;
                }
                if fx.abs() < 1e-9 {
                    return Some(x);
                }
                let der = match dg(x) {
                    Some(d) => d,
                    None => break,
                };
                let step = fx / der;
                x -= step;
                if !x.is_finite() {
                    break;
                }
            }
        }

        None
    }

    fn try_coupled(
        &mut self,
        blocked: &[Equation],
        env: &HashMap<String, f64>,
    ) -> Option<HashMap<String, f64>> {
        let mut unknowns_set = BTreeSet::new();
        for eq in blocked {
            let mut vars = eq.rhs.vars();
            vars.insert(eq.lhs.clone());
            for v in vars {
                if !env.contains_key(&v) {
                    unknowns_set.insert(v);
                }
            }
        }
        let unknowns: Vec<String> = unknowns_set.into_iter().collect();
        let n = unknowns.len();

        if n == 0 {
            return None;
        }

        // Gather all equations that constrain these unknowns
        let active_eqs: Vec<&Equation> = blocked
            .iter()
            .filter(|eq| {
                let mut vars = eq.rhs.vars();
                vars.insert(eq.lhs.clone());
                vars.iter().any(|v| unknowns.contains(v))
            })
            .collect();

        // Must be a square system (n equations for n unknowns)
        if active_eqs.len() != n {
            return None;
        }

        // Precompute symbolic partial derivatives for the Jacobian:
        // F_i(x) = rhs_i(x) - lhs_i(x)
        // dF_i/du_j = d(rhs_i)/du_j - (1.0 if u_j == lhs_i else 0.0)
        let mut jac_exprs: Vec<Vec<Option<Expr>>> = Vec::new();
        for eq in &active_eqs {
            let mut row = Vec::new();
            for u in &unknowns {
                match diff(&eq.rhs, u) {
                    Ok(d_rhs) => {
                        let final_expr = if u == &eq.lhs {
                            // Subtract 1.0 for d(lhs)/du
                            simplify(&Expr::Bin(
                                BinaryOp::Sub,
                                Box::new(d_rhs),
                                Box::new(Expr::Num(1.0)),
                            ))
                        } else {
                            d_rhs
                        };
                        row.push(Some(final_expr));
                    }
                    Err(_) => {
                        row.push(None);
                    }
                }
            }
            jac_exprs.push(row);
        }

        let residual = |vec: &[f64]| -> Option<Vec<f64>> {
            let mut local = env.clone();
            for (u, val) in unknowns.iter().zip(vec.iter()) {
                local.insert(u.clone(), *val);
            }
            let mut f_vals = Vec::with_capacity(n);
            for eq in &active_eqs {
                let rv = eval_expr(&eq.rhs, &local).ok()?;
                let lv = match local.get(&eq.lhs) {
                    Some(v) => *v,
                    None => rv,
                };
                f_vals.push(rv - lv);
            }
            Some(f_vals)
        };

        let jacobian = |vec: &[f64]| -> Option<Vec<Vec<f64>>> {
            let mut local = env.clone();
            for (u, val) in unknowns.iter().zip(vec.iter()) {
                local.insert(u.clone(), *val);
            }
            let mut mat = vec![vec![0.0; n]; n];
            for i in 0..n {
                for j in 0..n {
                    if let Some(ref d_expr) = jac_exprs[i][j] {
                        if let Ok(val) = eval_expr(d_expr, &local) {
                            if val.is_finite() {
                                mat[i][j] = val;
                                continue;
                            }
                        }
                    }
                    // Central-difference fallback
                    let h = 1e-6 * 1.0f64.max(vec[j].abs());
                    let mut vp = vec.to_vec();
                    let mut vm = vec.to_vec();
                    vp[j] += h;
                    vm[j] -= h;
                    let fp = residual(&vp)?[i];
                    let fm = residual(&vm)?[i];
                    let der = (fp - fm) / (2.0 * h);
                    if !der.is_finite() {
                        return None;
                    }
                    mat[i][j] = der;
                }
            }
            Some(mat)
        };

        // Multistart Newton-Raphson with asymmetric initial vectors to prevent symmetric Jacobian singularities
        for &seed in SEEDS_COUPLED {
            let mut vec: Vec<f64> = (0..n)
                .map(|i| seed * (1.0 + 0.37 * (i as f64) + 0.11 * ((i % 3) as f64)))
                .collect();
            let mut converged = false;

            for _ in 0..80 {
                let f_vec = match residual(&vec) {
                    Some(f) => f,
                    None => break,
                };

                let max_res = f_vec.iter().map(|v| v.abs()).fold(0.0f64, f64::max);
                if max_res < 1e-9 {
                    converged = true;
                    break;
                }

                let j_mat = match jacobian(&vec) {
                    Some(j) => j,
                    None => break,
                };

                let neg_f: Vec<f64> = f_vec.iter().map(|v| -v).collect();
                let delta = match solve_linear(&j_mat, &neg_f) {
                    Some(d) => d,
                    None => break,
                };

                for i in 0..n {
                    vec[i] += delta[i];
                    if !vec[i].is_finite() {
                        converged = false;
                        break;
                    }
                }
            }

            if converged {
                let mut out_env = env.clone();
                for (u, val) in unknowns.iter().zip(vec.iter()) {
                    out_env.insert(u.clone(), *val);
                }
                self.trace.push(format!(
                    "[coupled] {} <- multidim Newton on system of {} equations",
                    unknowns.join(", "),
                    n
                ));
                return Some(out_env);
            }
        }

        None
    }
}

/// Gaussian elimination with partial pivoting: M * x = b
fn solve_linear(mat: &[Vec<f64>], b: &[f64]) -> Option<Vec<f64>> {
    let n = b.len();
    let mut a: Vec<Vec<f64>> = Vec::with_capacity(n);
    for i in 0..n {
        let mut row = mat[i].clone();
        row.push(b[i]);
        a.push(row);
    }

    for col in 0..n {
        // Find pivot
        let mut max_row = col;
        let mut max_val = a[col][col].abs();
        for r in (col + 1)..n {
            let val = a[r][col].abs();
            if val > max_val {
                max_val = val;
                max_row = r;
            }
        }

        if max_val < 1e-12 {
            return None; // Singular or rank-deficient
        }

        a.swap(col, max_row);

        for r in (col + 1)..n {
            let factor = a[r][col] / a[col][col];
            for c in col..=n {
                let sub = factor * a[col][c];
                a[r][c] -= sub;
            }
        }
    }

    let mut x = vec![0.0; n];
    for r in (0..n).rev() {
        let mut sum = a[r][n];
        for c in (r + 1)..n {
            sum -= a[r][c] * x[c];
        }
        x[r] = sum / a[r][r];
    }

    Some(x)
}
