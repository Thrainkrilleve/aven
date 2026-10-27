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
    Unbounded(String),
    Undetermined(String),
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
            SolverError::Unbounded(msg) => write!(f, "Objective is unbounded: {}", msg),
            SolverError::Undetermined(msg) => write!(f, "Undetermined: {}", msg),
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
    /// Values that did settle when a solve stops as under-determined
    pub partial: HashMap<String, f64>,
    pub opt_telemetry: Option<crate::optimize::OptimizationTelemetry>,
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
            partial: HashMap::new(),
            opt_telemetry: None,
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

        // 2. With an objective, the optimizer chooses the remaining unknowns. Without one,
        //    bounds only check values that are determined: picking an arbitrary feasible point
        //    would report values nothing in the model or its inputs fixed.
        if !self.objectives.is_empty() {
            self.solve_with_optimization(&mut env)?;
        }

        // 3. Every equation must hold for the settled values
        for eq in &self.relations {
            let mut vs = eq.rhs.vars();
            vs.insert(eq.lhs.clone());
            if vs.iter().all(|v| env.contains_key(v)) {
                let expected = eval_expr(&eq.rhs, &env)
                    .map_err(|e| SolverError::MathError(e.to_string()))?;
                let given = env[&eq.lhs];
                if (expected - given).abs() > 1e-6 * (1.0 + expected.abs()) {
                    return Err(SolverError::Inconsistency {
                        relation: eq.original_text.clone(),
                        lhs: eq.lhs.clone(),
                        expected,
                        given,
                    });
                }
            }
        }

        // 4. Verify feasibility of all constraints against settled env
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

        // 4. Integer variables must hold whole numbers, even when equations (not the optimizer)
        //    determined them; snap away floating-point noise, reject anything else
        for iv in &self.integer_vars {
            if let Some(val) = env.get_mut(iv) {
                let rounded = val.round();
                if (*val - rounded).abs() < 1e-4 {
                    *val = rounded;
                } else {
                    return Err(SolverError::Infeasible(format!(
                        "integer variable '{}' is determined by the equations as {}, which is not a whole number",
                        iv, val
                    )));
                }
            }
        }

        // 5. Check if any unknowns remain undetermined, including variables a bound refers
        //    to (a bound cannot be checked on a value nobody supplied)
        let mut all_vars = BTreeSet::new();
        for eq in &self.relations {
            all_vars.insert(eq.lhs.clone());
            for v in eq.rhs.vars() {
                all_vars.insert(v);
            }
        }
        for c in &self.constraints {
            all_vars.insert(c.lhs.clone());
            all_vars.extend(c.rhs.vars());
        }
        all_vars.retain(|v| !crate::ast::is_builtin_constant(v));
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
            self.partial = env.clone();
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

            // Coupled multi-unknown simultaneous solving: split the blocked equations into
            // independent subsystems (connected through shared unknowns) and try them smallest
            // first, so one unsolvable subsystem does not prevent solving the others
            if !progressed && !blocked.is_empty() {
                let mut groups = coupled_components(&blocked, env);
                groups.sort_by_key(|g| g.len());
                for group in groups {
                    if let Some(solved_env) =
                        self.try_coupled_silent(&group, env, trace.as_deref_mut())
                    {
                        *env = solved_env;
                        progressed = true;
                        overall_progressed = true;
                        break;
                    }
                }
            }

            if !progressed {
                break;
            }
        }

        Ok(overall_progressed)
    }

    /// Reconstructs how each solved value was derived, in derivation order.
    ///
    /// Mirrors the propagation rule used by `solve`: an equation with exactly one unknown
    /// determines that unknown, and its other variables are the direct causes. Values settled
    /// jointly (coupled systems, constrained optimization) list every already-known variable of
    /// the equations, constraints and objectives involved. Returns (variable, causes) pairs.
    pub fn derivation_order(
        &self,
        initial: &HashMap<String, f64>,
        solved: &HashMap<String, f64>,
    ) -> Vec<(String, Vec<String>)> {
        let mut known: BTreeSet<String> = initial.keys().cloned().collect();
        let mut order = Vec::new();
        let pending = |known: &BTreeSet<String>| {
            solved
                .keys()
                .filter(|k| !known.contains(*k))
                .cloned()
                .collect::<BTreeSet<String>>()
        };

        loop {
            let remaining = pending(&known);
            if remaining.is_empty() {
                break;
            }

            let mut progressed = false;
            for eq in &self.relations {
                let mut vars = eq.rhs.vars();
                vars.insert(eq.lhs.clone());
                let unknown: Vec<&String> = vars.iter().filter(|v| !known.contains(*v)).collect();
                if unknown.len() == 1 && solved.contains_key(unknown[0]) {
                    let u = unknown[0].clone();
                    let causes: Vec<String> = vars.iter().filter(|v| **v != u).cloned().collect();
                    known.insert(u.clone());
                    order.push((u, causes));
                    progressed = true;
                }
            }
            if progressed {
                continue;
            }

            // Jointly settled values: depend on every known variable that constrains them.
            let mut joint_causes = BTreeSet::new();
            for eq in &self.relations {
                let mut vars = eq.rhs.vars();
                vars.insert(eq.lhs.clone());
                if vars.iter().any(|v| remaining.contains(v)) {
                    joint_causes.extend(vars.into_iter().filter(|v| known.contains(v)));
                }
            }
            for c in &self.constraints {
                let mut vars = c.rhs.vars();
                vars.insert(c.lhs.clone());
                joint_causes.extend(vars.into_iter().filter(|v| known.contains(v)));
            }
            let causes: Vec<String> = joint_causes.into_iter().collect();
            for u in &remaining {
                order.push((u.clone(), causes.clone()));
            }
            known.extend(remaining);
        }
        order
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
        use crate::optimize::{Ineq, Outcome, Problem, Var};

        let mut all_vars = BTreeSet::new();
        for eq in &self.relations {
            all_vars.insert(eq.lhs.clone());
            all_vars.extend(eq.rhs.vars());
        }
        for c in &self.constraints {
            all_vars.insert(c.lhs.clone());
            all_vars.extend(c.rhs.vars());
        }
        for obj in &self.objectives {
            all_vars.insert(obj.target.clone());
        }
        let unknowns: Vec<String> = all_vars
            .into_iter()
            .filter(|v| !env.contains_key(v))
            .collect();
        if unknowns.is_empty() {
            return Ok(false);
        }
        let is_unknown = |v: &String| unknowns.contains(v);

        // Simple bounds (x >= const, x <= const, x == const) become variable bounds; every other
        // constraint becomes an inequality g(x) <= 0 or an equation.
        let mut vars: Vec<Var> = unknowns
            .iter()
            .map(|u| Var {
                name: u.clone(),
                lo: f64::NEG_INFINITY,
                hi: f64::INFINITY,
                integer: self.integer_vars.contains(u),
            })
            .collect();
        let mut ineqs = Vec::new();
        let mut eqs = Vec::new();
        let sub = |a: Expr, b: Expr| Expr::Bin(BinaryOp::Sub, Box::new(a), Box::new(b));
        for c in &self.constraints {
            let rhs_has_unknown = c.rhs.vars().iter().any(|v| is_unknown(v));
            let rhs_const = if rhs_has_unknown {
                None
            } else {
                eval_expr(&c.rhs, env).ok()
            };
            if let (Some(j), Some(b)) = (unknowns.iter().position(|u| u == &c.lhs), rhs_const) {
                match c.op {
                    ConstraintOp::GreaterEqual => {
                        vars[j].lo = vars[j].lo.max(b);
                        continue;
                    }
                    ConstraintOp::LessEqual => {
                        vars[j].hi = vars[j].hi.min(b);
                        continue;
                    }
                    ConstraintOp::Equal => {
                        vars[j].lo = vars[j].lo.max(b);
                        vars[j].hi = vars[j].hi.min(b);
                        continue;
                    }
                    _ => {}
                }
            }
            if !is_unknown(&c.lhs) && !rhs_has_unknown {
                continue; // fully known: verified after solving
            }
            let lhs = Expr::Var(c.lhs.clone());
            match c.op {
                ConstraintOp::LessEqual | ConstraintOp::Less => ineqs.push(Ineq {
                    expr: sub(lhs, c.rhs.clone()),
                    strict: c.op == ConstraintOp::Less,
                }),
                ConstraintOp::GreaterEqual | ConstraintOp::Greater => ineqs.push(Ineq {
                    expr: sub(c.rhs.clone(), lhs),
                    strict: c.op == ConstraintOp::Greater,
                }),
                ConstraintOp::Equal => eqs.push(sub(lhs, c.rhs.clone())),
                // != cannot be optimized over; it is verified after solving
                ConstraintOp::NotEqual => {}
            }
        }
        // Integer quantities (servers, drives, pods) are non-negative unless bounded otherwise
        for v in vars.iter_mut() {
            if v.integer && v.lo == f64::NEG_INFINITY {
                v.lo = 0.0;
            }
        }
        for eq in &self.relations {
            if is_unknown(&eq.lhs) || eq.rhs.vars().iter().any(|v| is_unknown(v)) {
                eqs.push(sub(Expr::Var(eq.lhs.clone()), eq.rhs.clone()));
            }
        }

        let objective = self.objectives.first().map(|o| match o.kind {
            ObjectiveKind::Minimize => Expr::Var(o.target.clone()),
            ObjectiveKind::Maximize => Expr::UnaryMinus(Box::new(Expr::Var(o.target.clone()))),
        });
        if self.objectives.len() > 1 {
            self.trace.push(format!(
                "[optimize] {} objectives declared; optimizing the first ('{}'). Use `avenc pareto` for trade-offs.",
                self.objectives.len(),
                self.objectives[0].target
            ));
        }

        let problem = Problem {
            vars,
            eqs,
            ineqs,
            objective,
            env: env.clone(),
        };
        match problem.solve() {
            Outcome::Optimal {
                x,
                objective,
                proven,
                method,
                telemetry,
            } => {
                self.opt_telemetry = telemetry.clone();
                for (v, xi) in problem.vars.iter().zip(&x) {
                    env.insert(v.name.clone(), *xi);
                }
                self.recompute_forward_values(env, &unknowns);
                let shown = match self.objectives.first() {
                    Some(o) if o.kind == ObjectiveKind::Maximize => -objective,
                    _ => objective,
                };
                if let Some(telem) = &telemetry {
                    self.trace.push(format!(
                        "[optimize] {} — objective = {:.6} ({}) [bound = {:.6}, duality_gap = {:.2e}, cuts = {}, dual_norm = {:.2e}]",
                        method,
                        shown,
                        if proven {
                            "proven optimal"
                        } else {
                            "not proven globally optimal"
                        },
                        telem.bound,
                        telem.duality_gap,
                        telem.cuts_active,
                        telem.dual_norm
                    ));
                } else {
                    self.trace.push(format!(
                        "[optimize] {} — objective = {:.6} ({})",
                        method,
                        shown,
                        if proven {
                            "proven optimal"
                        } else {
                            "not proven globally optimal"
                        }
                    ));
                }
                Ok(true)
            }
            Outcome::Infeasible(msg) => Err(SolverError::Infeasible(msg)),
            Outcome::Unbounded(msg) => Err(SolverError::Unbounded(msg)),
            Outcome::Undetermined(msg) => Err(SolverError::Undetermined(msg)),
        }
    }

    /// Recomputes every unknown defined by an equation whose right-hand side is already
    /// settled, so derived values match their equations exactly instead of carrying the
    /// optimizer's tolerance.
    fn recompute_forward_values(&self, env: &mut HashMap<String, f64>, unknowns: &[String]) {
        let lhs_vars: BTreeSet<&String> = self.relations.iter().map(|e| &e.lhs).collect();
        let mut settled: BTreeSet<String> = env
            .keys()
            .filter(|k| !unknowns.contains(*k) || !lhs_vars.contains(*k))
            .cloned()
            .collect();
        loop {
            let mut progressed = false;
            for eq in &self.relations {
                if settled.contains(&eq.lhs) || !unknowns.contains(&eq.lhs) {
                    continue;
                }
                if eq.rhs.vars().iter().all(|v| settled.contains(v)) {
                    if let Ok(v) = eval_expr(&eq.rhs, env) {
                        env.insert(eq.lhs.clone(), v);
                        settled.insert(eq.lhs.clone());
                        progressed = true;
                    }
                }
            }
            if !progressed {
                break;
            }
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
                // Damped step: halve it while it leaves the function's domain (e.g. a negative
                // base under a fractional power, a log of a non-positive value) or makes the
                // residual worse. A full Newton step from a poor seed often overshoots there.
                let mut lambda = 1.0;
                let mut next = x - step;
                for _ in 0..40 {
                    let fn_ = g(next);
                    if next.is_finite() && fn_.is_finite() && fn_.abs() < fx.abs() {
                        break;
                    }
                    lambda *= 0.5;
                    next = x - lambda * step;
                }
                if !next.is_finite() || !g(next).is_finite() || next == x {
                    break;
                }
                x = next;
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

/// Splits equations into groups connected through shared unknowns (variables not in `env`).
fn coupled_components(eqs: &[Equation], env: &HashMap<String, f64>) -> Vec<Vec<Equation>> {
    let unknowns_of = |eq: &Equation| -> BTreeSet<String> {
        let mut vars = eq.rhs.vars();
        vars.insert(eq.lhs.clone());
        vars.into_iter().filter(|v| !env.contains_key(v)).collect()
    };
    let sets: Vec<BTreeSet<String>> = eqs.iter().map(unknowns_of).collect();
    let mut group_of: Vec<usize> = (0..eqs.len()).collect();
    let find = |g: &mut Vec<usize>, mut i: usize| {
        while g[i] != i {
            g[i] = g[g[i]];
            i = g[i];
        }
        i
    };
    for i in 0..eqs.len() {
        for j in (i + 1)..eqs.len() {
            if !sets[i].is_disjoint(&sets[j]) {
                let (a, b) = (find(&mut group_of, i), find(&mut group_of, j));
                if a != b {
                    group_of[a] = b;
                }
            }
        }
    }
    let mut groups: HashMap<usize, Vec<Equation>> = HashMap::new();
    for i in 0..eqs.len() {
        let root = find(&mut group_of, i);
        groups.entry(root).or_default().push(eqs[i].clone());
    }
    groups.into_values().collect()
}
