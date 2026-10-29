// src/api.rs — one stable surface the IDE, the CLI, and the WASM build all call.
//
// Everything here is a thin wrapper over the existing compiler pieces (lexer,
// parser, dimensional checker, solver). It adds no new checking or solving
// logic, so there stays exactly one source of truth for the math.

use crate::ast::BlockKind;
use crate::lexer::Lexer;
use crate::parser::Parser;
use crate::solver::SolverEngine;
use crate::units::DimensionalChecker;
use std::collections::{BTreeMap, HashMap};

/// A source position: 1-based line, 1-based column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pos {
    pub line: usize,
    pub col: usize,
}

/// Diagnostic severity. Only `Error` today; `Warning` is reserved so the shape
/// doesn't change when non-fatal hints arrive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

/// One diagnostic with a start–end range, so an editor can underline the span.
///
/// `end` is best-effort today (equal to `start` until the lexer carries token
/// spans); the range is in the type from day one so filling it in later is not
/// an API change.
#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub severity: Severity,
    pub message: String,
    pub start: Pos,
    pub end: Pos,
}

impl Diagnostic {
    fn error(message: impl Into<String>, line: usize, col: usize) -> Self {
        let p = Pos { line, col };
        Diagnostic {
            severity: Severity::Error,
            message: message.into(),
            start: p,
            end: p,
        }
    }
}

/// The result of solving a model: every settled value (sorted by name) and the
/// solver's step trace (which variable was found forward vs. backward).
#[derive(Debug, Clone)]
pub struct Solved {
    pub values: BTreeMap<String, f64>,
    pub trace: Vec<String>,
}

/// Check a model for lex, parse, and dimensional (unit) errors. Returns an
/// empty vec when the model is clean.
///
/// Degrees-of-freedom (under-determined) reporting currently surfaces through
/// `solve`'s error; folding it in here is the next increment.
pub fn check(source: &str) -> Vec<Diagnostic> {
    let tokens = match Lexer::new(source).tokenize() {
        Ok(t) => t,
        Err(e) => return vec![Diagnostic::error(e.message.clone(), e.line, e.col)],
    };
    let module = match Parser::new(tokens).parse_module("model") {
        Ok(m) => m,
        Err(e) => return vec![Diagnostic::error(e.message.clone(), e.line, e.col)],
    };
    match DimensionalChecker::new().check_module(&module) {
        Ok(_) => Vec::new(),
        Err(e) => {
            let (line, col) = e.position();
            vec![Diagnostic::error(strip_locator(&e.to_string()), line, col)]
        }
    }
}

/// Solve a model in whatever direction its `solve` block leaves open — forward
/// where it can, backward (Newton) where it must. Mirrors `avenc run`'s pure
/// path; external / IPC / shared-memory backends are not invoked here.
pub fn solve(source: &str) -> Result<Solved, Diagnostic> {
    let tokens = Lexer::new(source)
        .tokenize()
        .map_err(|e| Diagnostic::error(e.message.clone(), e.line, e.col))?;
    let module = Parser::new(tokens)
        .parse_module("model")
        .map_err(|e| Diagnostic::error(e.message.clone(), e.line, e.col))?;

    let mut solver = SolverEngine::new();
    for blk in &module.blocks {
        if blk.kind == BlockKind::Relation {
            for eq in &blk.equations {
                solver.add_relation(eq.clone());
            }
            for iv in &blk.integer_vars {
                solver.add_integer_var(iv.clone());
            }
        }
    }
    let mut env: HashMap<String, f64> = HashMap::new();
    for s in &module.solves {
        for c in &s.constraints {
            solver.add_constraint(c.clone());
        }
        for o in &s.objectives {
            solver.add_objective(o.clone());
        }
        for iv in &s.integer_vars {
            solver.add_integer_var(iv.clone());
        }
        for b in &s.bindings {
            env.insert(b.name.clone(), b.value);
        }
    }

    match solver.solve(&env) {
        Ok(settled) => Ok(Solved {
            values: settled.into_iter().collect(),
            trace: solver.trace.clone(),
        }),
        // Solver errors (under-determined, infeasible, …) have no single source
        // position; report at 0,0 with the engine's own message.
        Err(e) => Err(Diagnostic::error(format!("{e}"), 0, 0)),
    }
}

/// Strip a leading "Type Error at line N, col M: " locator — the Diagnostic's
/// range already conveys where, so the message reads more plainly without it.
fn strip_locator(msg: &str) -> String {
    if msg.starts_with("Type Error at line") {
        if let Some(idx) = msg.find(": ") {
            return msg[idx + 2..].to_string();
        }
    }
    msg.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_clean_model_has_no_diagnostics() {
        let src = "relation R {\n y = 2 * x\n}\nsolve R with {\n x = 3\n}\n";
        assert!(check(src).is_empty());
    }

    #[test]
    fn check_flags_a_unit_mismatch_with_a_position() {
        let src = "signal v : km/h\nsignal t : s\nrelation R {\n bad = v + t\n}\nsolve R with {\n v = 10 km/h\n t = 2 s\n}\n";
        let diags = check(src);
        assert!(!diags.is_empty(), "expected a dimensional error");
        assert!(diags[0].start.line >= 1);
    }

    #[test]
    fn check_reports_parse_errors() {
        let diags = check("relation R {\n y = \n}\n");
        assert!(!diags.is_empty());
    }

    #[test]
    fn solve_runs_backward_for_the_blank_variable() {
        // force = mass * accel; give force + accel, solve mass backward.
        let src = "relation N {\n force = mass * accel\n}\nsolve N with {\n force = 4200\n accel = 5\n}\n";
        let solved = solve(src).expect("should solve");
        let mass = solved.values.get("mass").copied().expect("mass settled");
        assert!((mass - 840.0).abs() < 1e-9, "mass = {mass}");
    }

    #[test]
    fn solve_runs_forward() {
        let src =
            "relation N {\n force = mass * accel\n}\nsolve N with {\n mass = 2\n accel = 3\n}\n";
        let solved = solve(src).expect("should solve");
        assert!((solved.values["force"] - 6.0).abs() < 1e-9);
    }
}
