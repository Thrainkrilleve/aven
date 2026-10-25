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
                if !diag.unreachable_targets.is_empty() {
                    write!(
                        f,
                        "Determined system stalled (unreachable target):\n{}",
                        diag.report()
                    )
                } else if diag.dof == 0 {
                    write!(
                        f,
                        "Determined system stalled (0 degrees of freedom):\n{}",
                        diag.report()
                    )
                } else {
                    write!(
                        f,
                        "System is under-determined ({} degree(s) of freedom):\n{}",
                        diag.dof,
                        diag.report()
                    )
                }
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

    fn solve_equalities(
        &self,
        relations: &[Equation],
        env: &mut HashMap<String, f64>,
        mut trace: Option<&mut Vec<String>>,
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
                        if let Some(ref mut tr) = trace {
                            tr.push(format!("[{}] {} <- forward eval", eq.original_text, u));
                        }
                        progressed = true;
                        overall_progressed = true;
                    } else if env.contains_key(&eq.lhs) {
                        // 1D Backward solving: solve rhs(..., u=x, ...) = lhs
                        if let Some(x) = self.solve_one_unknown(eq, u, env) {
                            env.insert(u.clone(), x);
                            if let Some(ref mut tr) = trace {
                                tr.push(format!("[{}] {} <- backward Newton", eq.original_text, u));
                            }
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
                if let Some(solved_env) =
                    self.try_coupled_silent(&blocked, env, trace.as_deref_mut())
                {
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

    pub fn solve_pure_equalities(
        &mut self,
        relations: &[Equation],
        env: &mut HashMap<String, f64>,
    ) -> Result<bool, SolverError> {
        let mut tr = std::mem::take(&mut self.trace);
        let res = self.solve_equalities(relations, env, Some(&mut tr));
        self.trace = tr;
        res
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

        // 1. Separate decision variables from dependent derived variables
        let mut dec_vars: Vec<String> = self
            .integer_vars
            .iter()
            .filter(|iv| remaining_unknowns.contains(iv))
            .cloned()
            .collect();

        // Simulate forward/backward reachability from env + dec_vars to identify all independent decision variables
        loop {
            let mut simulated: BTreeSet<String> = env.keys().cloned().collect();
            simulated.extend(dec_vars.iter().cloned());
            loop {
                let mut progressed = false;
                for eq in &base_relations {
                    let mut unk = eq.rhs.vars();
                    unk.insert(eq.lhs.clone());
                    let missing: Vec<String> =
                        unk.into_iter().filter(|v| !simulated.contains(v)).collect();
                    if missing.len() == 1 {
                        simulated.insert(missing[0].clone());
                        progressed = true;
                    }
                }
                if !progressed {
                    break;
                }
            }
            let unreached: Vec<String> = remaining_unknowns
                .iter()
                .filter(|u| !simulated.contains(*u))
                .cloned()
                .collect();
            if unreached.is_empty() {
                break;
            }
            // Pick an unresolved variable as an additional decision variable (avoid picking the objective target if others exist)
            let candidate = if let Some(ref obj) = primary_obj {
                unreached
                    .iter()
                    .find(|u| *u != &obj.target)
                    .unwrap_or(&unreached[0])
                    .clone()
            } else {
                unreached[0].clone()
            };
            dec_vars.push(candidate);
        }

        let d = dec_vars.len();
        let is_maximize = matches!(
            primary_obj.as_ref().map(|o| o.kind),
            Some(ObjectiveKind::Maximize)
        );

        let compute_objective = |cand_env: &HashMap<String, f64>| -> Option<f64> {
            if let Some(ref obj) = primary_obj {
                cand_env.get(&obj.target).copied()
            } else {
                Some(0.0)
            }
        };

        let is_better = |new_score: f64, current_best: f64| -> bool {
            if is_maximize {
                new_score > current_best + 1e-7
            } else {
                new_score < current_best - 1e-7
            }
        };

        // Helper to evaluate a decision vector under given variable bounds
        let eval_point = |cand_x: &[f64],
                          bounds: &HashMap<String, (f64, f64)>|
         -> Option<(HashMap<String, f64>, f64, f64)> {
            let mut cand_env = env.clone();
            for (i, v) in dec_vars.iter().enumerate() {
                cand_env.insert(v.clone(), cand_x[i]);
            }
            if self
                .solve_equalities(&base_relations, &mut cand_env, None)
                .is_err()
            {
                return None;
            }
            for u in &remaining_unknowns {
                if !cand_env.contains_key(u) {
                    return None;
                }
            }
            let mut viol = 0.0;
            for (i, v) in dec_vars.iter().enumerate() {
                if let Some(&(lo, hi)) = bounds.get(v) {
                    if cand_x[i] < lo - 1e-6 {
                        viol += lo - cand_x[i];
                    }
                    if cand_x[i] > hi + 1e-6 {
                        viol += cand_x[i] - hi;
                    }
                }
            }
            for c in &constraints_list {
                let lhs_val = *cand_env.get(&c.lhs)?;
                let rhs_val = eval_expr(&c.rhs, &cand_env).ok()?;
                let v_diff = match c.op {
                    ConstraintOp::LessEqual => (lhs_val - rhs_val).max(0.0),
                    ConstraintOp::GreaterEqual => (rhs_val - lhs_val).max(0.0),
                    ConstraintOp::Less => (lhs_val - rhs_val + 1e-7).max(0.0),
                    ConstraintOp::Greater => (rhs_val - lhs_val + 1e-7).max(0.0),
                    ConstraintOp::Equal => (lhs_val - rhs_val).abs(),
                    ConstraintOp::NotEqual => {
                        if (lhs_val - rhs_val).abs() < 1e-7 {
                            1.0
                        } else {
                            0.0
                        }
                    }
                };
                viol += v_diff;
            }
            let score = compute_objective(&cand_env)?;
            Some((cand_env, score, viol))
        };

        // 2. Precompute seeds
        // 2a. Unconstrained stationary points from objective derivative (e.g. min (x-3)^2 + 1 => x = 3)
        let mut interior_seeds: Vec<Vec<f64>> = Vec::new();
        if let Some(ref obj) = primary_obj {
            for eq in &base_relations {
                if eq.lhs == obj.target {
                    for (i, v) in dec_vars.iter().enumerate() {
                        if let Ok(deriv) = diff(&eq.rhs, v) {
                            let dummy_eq = Equation {
                                lhs: "_deriv".to_string(),
                                op: ConstraintOp::Equal,
                                rhs: deriv,
                                original_text: "_deriv = 0".to_string(),
                                line: 0,
                                col: 0,
                            };
                            let mut d_env = env.clone();
                            d_env.insert("_deriv".to_string(), 0.0);
                            for (j, other_v) in dec_vars.iter().enumerate() {
                                if i != j {
                                    d_env.insert(other_v.clone(), 1.0);
                                }
                            }
                            if let Some(root) = self.solve_one_unknown(&dummy_eq, v, &d_env) {
                                if root.is_finite() {
                                    let mut seed = vec![1.0; d];
                                    seed[i] = root;
                                    interior_seeds.push(seed);
                                }
                            }
                        }
                    }
                }
            }
        }

        // 2b. Boundary equality solutions from constraints (active constraints)
        let mut boundary_seeds: Vec<Vec<f64>> = Vec::new();
        for c in &constraints_list {
            let mut test_rel = base_relations.clone();
            test_rel.push(Equation {
                lhs: c.lhs.clone(),
                op: ConstraintOp::Equal,
                rhs: c.rhs.clone(),
                original_text: format!("{} = {}", c.lhs, c.rhs),
                line: c.line,
                col: c.col,
            });
            let mut test_env = env.clone();
            if self
                .solve_equalities(&test_rel, &mut test_env, None)
                .is_ok()
            {
                if dec_vars.iter().all(|v| test_env.contains_key(v)) {
                    let pt: Vec<f64> = dec_vars.iter().map(|v| test_env[v]).collect();
                    boundary_seeds.push(pt);
                }
            }
        }

        // 2c. Pairs of constraints for multi-variable boundaries
        if constraints_list.len() > 1 && d >= 2 {
            for i in 0..constraints_list.len() {
                for j in (i + 1)..constraints_list.len() {
                    let mut test_rel = base_relations.clone();
                    test_rel.push(Equation {
                        lhs: constraints_list[i].lhs.clone(),
                        op: ConstraintOp::Equal,
                        rhs: constraints_list[i].rhs.clone(),
                        original_text: format!(
                            "{} = {}",
                            constraints_list[i].lhs, constraints_list[i].rhs
                        ),
                        line: constraints_list[i].line,
                        col: constraints_list[i].col,
                    });
                    test_rel.push(Equation {
                        lhs: constraints_list[j].lhs.clone(),
                        op: ConstraintOp::Equal,
                        rhs: constraints_list[j].rhs.clone(),
                        original_text: format!(
                            "{} = {}",
                            constraints_list[j].lhs, constraints_list[j].rhs
                        ),
                        line: constraints_list[j].line,
                        col: constraints_list[j].col,
                    });
                    let mut test_env = env.clone();
                    if self
                        .solve_equalities(&test_rel, &mut test_env, None)
                        .is_ok()
                    {
                        if dec_vars.iter().all(|v| test_env.contains_key(v)) {
                            let pt: Vec<f64> = dec_vars.iter().map(|v| test_env[v]).collect();
                            boundary_seeds.push(pt);
                        }
                    }
                }
            }
        }

        // 3. Set up Branch-and-Bound root node
        let mut root_bounds = HashMap::new();
        for v in &dec_vars {
            let mut explicit_lo = None;
            let mut explicit_hi = None;
            for c in &constraints_list {
                if &c.lhs == v {
                    if let Expr::Num(n) = c.rhs {
                        match c.op {
                            ConstraintOp::GreaterEqual | ConstraintOp::Greater => {
                                explicit_lo = Some(explicit_lo.map_or(n, |cur: f64| cur.max(n)));
                            }
                            ConstraintOp::LessEqual | ConstraintOp::Less => {
                                explicit_hi = Some(explicit_hi.map_or(n, |cur: f64| cur.min(n)));
                            }
                            ConstraintOp::Equal => {
                                explicit_lo = Some(explicit_lo.map_or(n, |cur: f64| cur.max(n)));
                                explicit_hi = Some(explicit_hi.map_or(n, |cur: f64| cur.min(n)));
                            }
                            _ => {}
                        }
                    }
                }
            }
            let lo = explicit_lo.unwrap_or(if self.integer_vars.contains(v) {
                0.0
            } else {
                -1e7
            });
            let hi = explicit_hi.unwrap_or(1e7);
            root_bounds.insert(v.clone(), (lo, hi));
        }

        struct BnBNode {
            bounds: HashMap<String, (f64, f64)>,
        }

        struct CandidateSolution {
            env: HashMap<String, f64>,
            objective_val: f64,
        }

        let mut queue = vec![BnBNode {
            bounds: root_bounds,
        }];
        let mut best_solution: Option<CandidateSolution> = None;
        let mut bnb_traces: Vec<String> = Vec::new();
        let mut max_nodes = 300;

        while let Some(node) = queue.pop() {
            if max_nodes == 0 {
                break;
            }
            max_nodes -= 1;

            let lower_vec: Vec<f64> = dec_vars.iter().map(|v| node.bounds[v].0).collect();
            let upper_vec: Vec<f64> = dec_vars.iter().map(|v| node.bounds[v].1).collect();

            // Gather candidate seeds for this subproblem
            let mut seeds: Vec<Vec<f64>> = Vec::new();
            for s in &interior_seeds {
                seeds.push(s.clone());
            }
            for s in &boundary_seeds {
                seeds.push(s.clone());
            }

            // Bound midpoints and endpoints
            let mid_pt: Vec<f64> = (0..d)
                .map(|i| {
                    let lo = lower_vec[i];
                    let hi = upper_vec[i];
                    if lo > -1e5 && hi < 1e5 {
                        (lo + hi) / 2.0
                    } else if lo > -1e5 {
                        lo + 1.0
                    } else if hi < 1e5 {
                        hi - 1.0
                    } else {
                        1.0
                    }
                })
                .collect();
            seeds.push(mid_pt);

            let low_clamped: Vec<f64> = (0..d).map(|i| lower_vec[i].max(-1e4)).collect();
            seeds.push(low_clamped);
            let up_clamped: Vec<f64> = (0..d).map(|i| upper_vec[i].min(1e4)).collect();
            seeds.push(up_clamped);

            seeds.push(vec![0.0; d]);
            seeds.push(vec![1.0; d]);
            seeds.push(vec![10.0; d]);

            // Polish each seed with Nelder-Mead on the penalized objective
            let mut best_relaxed_point: Option<(Vec<f64>, HashMap<String, f64>, f64)> = None;

            for raw_seed in seeds {
                let clamped_seed: Vec<f64> = (0..d)
                    .map(|i| raw_seed[i].max(lower_vec[i]).min(upper_vec[i]))
                    .collect();

                let (opt_x, _) =
                    nelder_mead(
                        &clamped_seed,
                        &lower_vec,
                        &upper_vec,
                        60,
                        |pt| match eval_point(pt, &node.bounds) {
                            Some((_, score, viol)) => {
                                let base = if is_maximize { -score } else { score };
                                base + 1e7 * viol + 1e5 * viol * viol
                            }
                            None => 1e15,
                        },
                    );

                for cand in &[opt_x, clamped_seed] {
                    if let Some((cand_env, score, viol)) = eval_point(cand, &node.bounds) {
                        if viol <= 1e-5 {
                            let update = match &best_relaxed_point {
                                None => true,
                                Some((_, _, best_score)) => is_better(score, *best_score),
                            };
                            if update {
                                best_relaxed_point = Some((cand.clone(), cand_env, score));
                            }
                        }
                    }
                }
            }

            let (relaxed_x, relaxed_env, relaxed_score) = match best_relaxed_point {
                Some(p) => p,
                None => continue, // Infeasible relaxation, prune branch
            };

            // Bound pruning: if relaxation cannot beat incumbent best, prune branch
            if let Some(ref best) = best_solution {
                if !is_better(relaxed_score, best.objective_val) {
                    continue;
                }
            }

            // Integrality check on decision integer variables
            let int_dec_indices: Vec<usize> = (0..d)
                .filter(|&i| self.integer_vars.contains(&dec_vars[i]))
                .collect();

            let mut fractional: Vec<(usize, f64, f64)> = Vec::new();
            for &idx in &int_dec_indices {
                let val = relaxed_x[idx];
                let round_val = val.round();
                let dist = (val - round_val).abs();
                if dist > 1e-4 {
                    fractional.push((idx, val, dist));
                }
            }

            if fractional.is_empty() {
                // Feasible integer solution found!
                let update = match &best_solution {
                    None => true,
                    Some(best) => is_better(relaxed_score, best.objective_val),
                };
                if update {
                    let combo_str = int_dec_indices
                        .iter()
                        .map(|&i| format!("{} = {:.0}", dec_vars[i], relaxed_x[i].round()))
                        .collect::<Vec<_>>()
                        .join(", ");
                    if !combo_str.is_empty() {
                        bnb_traces.push(format!(
                            "[branch-and-bound] candidate {} is feasible (objective = {:.4})",
                            combo_str, relaxed_score
                        ));
                    }
                    best_solution = Some(CandidateSolution {
                        env: relaxed_env,
                        objective_val: relaxed_score,
                    });
                }
            } else {
                // Branch on the integer variable with largest fractional distance from integer (closest to 0.5)
                fractional
                    .sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
                let (split_idx, split_val, _) = fractional[0];
                let split_var = &dec_vars[split_idx];
                let floor_val = split_val.floor();
                let ceil_val = split_val.ceil();

                // Left child: x <= floor
                let mut left_bounds = node.bounds.clone();
                let cur_lo = left_bounds[split_var].0;
                let cur_hi = left_bounds[split_var].1;
                let new_hi = cur_hi.min(floor_val);
                if cur_lo <= new_hi + 1e-7 {
                    left_bounds.get_mut(split_var).unwrap().1 = new_hi;
                    queue.push(BnBNode {
                        bounds: left_bounds,
                    });
                }

                // Right child: x >= ceil
                let mut right_bounds = node.bounds.clone();
                let cur_lo_r = right_bounds[split_var].0;
                let cur_hi_r = right_bounds[split_var].1;
                let new_lo = cur_lo_r.max(ceil_val);
                if new_lo <= cur_hi_r + 1e-7 {
                    right_bounds.get_mut(split_var).unwrap().0 = new_lo;
                    queue.push(BnBNode {
                        bounds: right_bounds,
                    });
                }
            }
        }

        self.trace.extend(bnb_traces);

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
                    Some(d) if d.is_finite() && d.abs() >= 1e-14 => d,
                    _ => break,
                };
                let step = fx / der;
                if !step.is_finite() {
                    break;
                }
                x -= step;
                if !x.is_finite() {
                    break;
                }
            }
        }

        None
    }

    fn try_coupled_silent(
        &self,
        blocked: &[Equation],
        env: &HashMap<String, f64>,
        mut trace: Option<&mut Vec<String>>,
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
                if let Some(ref mut tr) = trace {
                    tr.push(format!(
                        "[coupled] {} <- multidim Newton on system of {} equations",
                        unknowns.join(", "),
                        n
                    ));
                }
                return Some(out_env);
            }
        }

        None
    }

    #[allow(dead_code)]
    pub fn try_coupled(
        &mut self,
        blocked: &[Equation],
        env: &HashMap<String, f64>,
    ) -> Option<HashMap<String, f64>> {
        let mut tr = std::mem::take(&mut self.trace);
        let res = self.try_coupled_silent(blocked, env, Some(&mut tr));
        self.trace = tr;
        res
    }
}

/// Nelder-Mead simplex search for continuous non-linear optimization with box bounds
fn nelder_mead<F>(
    x0: &[f64],
    lower: &[f64],
    upper: &[f64],
    max_iter: usize,
    mut f: F,
) -> (Vec<f64>, f64)
where
    F: FnMut(&[f64]) -> f64,
{
    let d = x0.len();
    if d == 0 {
        return (vec![], f(&[]));
    }

    let clamp_vec = |v: &[f64]| -> Vec<f64> {
        let mut out = v.to_vec();
        for i in 0..d {
            out[i] = out[i].max(lower[i]).min(upper[i]);
        }
        out
    };

    // Construct initial simplex with d + 1 vertices
    let p0 = clamp_vec(x0);
    let mut vertices = Vec::with_capacity(d + 1);
    let mut scores = Vec::with_capacity(d + 1);

    let f0 = f(&p0);
    vertices.push(p0.clone());
    scores.push(f0);

    for i in 0..d {
        let mut pi = p0.clone();
        let span = (upper[i] - lower[i]).abs();
        let step = if span < 1e6 && span > 1e-4 {
            0.05 * span
        } else {
            0.05 * 1.0f64.max(p0[i].abs())
        };
        if pi[i] + step <= upper[i] {
            pi[i] += step;
        } else {
            pi[i] -= step;
        }
        pi = clamp_vec(&pi);
        let score = f(&pi);
        vertices.push(pi);
        scores.push(score);
    }

    for _ in 0..max_iter {
        // Sort vertices so scores[0] <= scores[1] <= ... <= scores[d]
        let mut order: Vec<usize> = (0..=d).collect();
        order.sort_by(|&a, &b| {
            scores[a]
                .partial_cmp(&scores[b])
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let sorted_vertices: Vec<Vec<f64>> =
            order.iter().map(|&idx| vertices[idx].clone()).collect();
        let sorted_scores: Vec<f64> = order.iter().map(|&idx| scores[idx]).collect();
        vertices = sorted_vertices;
        scores = sorted_scores;

        if (scores[d] - scores[0]).abs() < 1e-9 {
            break;
        }

        // Compute centroid of all vertices except the worst (d)
        let mut c = vec![0.0; d];
        for i in 0..d {
            for j in 0..d {
                c[j] += vertices[i][j];
            }
        }
        for j in 0..d {
            c[j] /= d as f64;
        }

        // Reflection
        let mut xr = vec![0.0; d];
        for j in 0..d {
            xr[j] = c[j] + 1.0 * (c[j] - vertices[d][j]);
        }
        xr = clamp_vec(&xr);
        let fr = f(&xr);

        if scores[0] <= fr && fr < scores[d - 1] {
            vertices[d] = xr;
            scores[d] = fr;
            continue;
        }

        if fr < scores[0] {
            // Expansion
            let mut xe = vec![0.0; d];
            for j in 0..d {
                xe[j] = c[j] + 2.0 * (xr[j] - c[j]);
            }
            xe = clamp_vec(&xe);
            let fe = f(&xe);
            if fe < fr {
                vertices[d] = xe;
                scores[d] = fe;
            } else {
                vertices[d] = xr;
                scores[d] = fr;
            }
            continue;
        }

        // Contraction
        if fr < scores[d] {
            let mut xc = vec![0.0; d];
            for j in 0..d {
                xc[j] = c[j] + 0.5 * (xr[j] - c[j]);
            }
            xc = clamp_vec(&xc);
            let fc = f(&xc);
            if fc <= fr {
                vertices[d] = xc;
                scores[d] = fc;
                continue;
            }
        } else {
            let mut xc = vec![0.0; d];
            for j in 0..d {
                xc[j] = c[j] - 0.5 * (c[j] - vertices[d][j]);
            }
            xc = clamp_vec(&xc);
            let fc = f(&xc);
            if fc < scores[d] {
                vertices[d] = xc;
                scores[d] = fc;
                continue;
            }
        }

        // Shrink
        for i in 1..=d {
            for j in 0..d {
                vertices[i][j] = vertices[0][j] + 0.5 * (vertices[i][j] - vertices[0][j]);
            }
            vertices[i] = clamp_vec(&vertices[i]);
            scores[i] = f(&vertices[i]);
        }
    }

    let mut best_idx = 0;
    for i in 1..=d {
        if scores[i] < scores[best_idx] {
            best_idx = i;
        }
    }
    (vertices[best_idx].clone(), scores[best_idx])
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

        if max_val < 1e-12 || !max_val.is_finite() {
            return None; // Singular, rank-deficient, or non-finite
        }

        a.swap(col, max_row);

        for r in (col + 1)..n {
            let factor = a[r][col] / a[col][col];
            if !factor.is_finite() {
                return None;
            }
            for c in col..=n {
                let sub = factor * a[col][c];
                a[r][c] -= sub;
                if !a[r][c].is_finite() {
                    return None;
                }
            }
        }
    }

    let mut x = vec![0.0; n];
    for r in (0..n).rev() {
        let mut sum = a[r][n];
        for c in (r + 1)..n {
            sum -= a[r][c] * x[c];
        }
        if a[r][r].abs() < 1e-14 || !a[r][r].is_finite() {
            return None;
        }
        let val = sum / a[r][r];
        if !val.is_finite() {
            return None;
        }
        x[r] = val;
    }

    Some(x)
}
