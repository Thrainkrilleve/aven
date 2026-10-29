// src/api.rs — one stable surface the IDE, the CLI, and the WASM build all call.
//
// Everything here is a thin wrapper over the existing compiler pieces (lexer,
// parser, dimensional checker, solver). It adds no new checking or solving
// logic, so there stays exactly one source of truth for the math.

use crate::ast::BlockKind;
use crate::evaluator::sha256_hex;
use crate::ledger::{DEFAULT_MAX_DEPTH, Fact, Ledger};
use crate::lexer::Lexer;
use crate::parser::Parser;
use crate::solver::SolverEngine;
use crate::units::{DimensionalChecker, TypeError, normalize_bindings};
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
/// `end` is the exclusive end of the source line that contains `start` (one
/// column past the last character). Solver errors that have no source line
/// stay a point at 0,0. A tighter token span can replace the line span later
/// without changing this type.
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

/// A diagnostic anchored at `line`/`col`, with `end` at the end of that line so
/// an editor can underline the whole line. Solver errors reported at 0,0 stay
/// a point: they have no source line.
fn located(source: &str, message: impl Into<String>, line: usize, col: usize) -> Diagnostic {
    let mut d = Diagnostic::error(message, line, col);
    if line >= 1 {
        if let Some(text) = source.lines().nth(line - 1) {
            let end_col = text.chars().count() + 1;
            if end_col > col {
                d.end = Pos { line, col: end_col };
            }
        }
    }
    d
}

/// Check a model for lex, parse, and dimensional (unit) errors. Returns an
/// empty vec when the model is clean.
///
/// Degrees-of-freedom (under-determined) reporting currently surfaces through
/// `solve`'s error; folding it in here is the next increment.
pub fn check(source: &str) -> Vec<Diagnostic> {
    let tokens = match Lexer::new(source).tokenize() {
        Ok(t) => t,
        Err(e) => return vec![located(source, e.message.clone(), e.line, e.col)],
    };
    let module = match Parser::new(tokens).parse_module("model") {
        Ok(m) => m,
        Err(e) => return vec![located(source, e.message.clone(), e.line, e.col)],
    };
    match DimensionalChecker::new().check_module(&module) {
        Ok(_) => Vec::new(),
        Err(e) => {
            let (line, col) = e.position();
            vec![located(source, plain_type_message(&e), line, col)]
        }
    }
}

/// The sentence the IDE shows. Addition and subtraction name the two dimensions
/// the way a person would; every other checker error keeps its own wording,
/// without the "Type Error at line" prefix the range already carries.
fn plain_type_message(err: &TypeError) -> String {
    match err {
        TypeError::DimensionalMismatch {
            left_dim,
            right_dim,
            op,
            line,
            ..
        } if op == "+/-" => {
            format!("Can't add {left_dim} to {right_dim} (line {line}).")
        }
        other => strip_locator(&other.to_string()),
    }
}

/// Solve a model in whatever direction its `solve` block leaves open — forward
/// where it can, backward (Newton) where it must. Mirrors `avenc run`'s pure
/// path; external / IPC / shared-memory backends are not invoked here.
///
/// Solve-block numbers are converted to base units first, the same conversion
/// `why` and `report` already apply, so a binding written `90 km/h` is solved
/// as metres per second.
pub fn solve(source: &str) -> Result<Solved, Diagnostic> {
    // A model that fails check must not come back as a settled number. The
    // solver adds the raw magnitudes, so km/h + s would otherwise "solve".
    if let Some(d) = check(source).into_iter().next() {
        return Err(d);
    }
    let tokens = Lexer::new(source)
        .tokenize()
        .map_err(|e| located(source, e.message.clone(), e.line, e.col))?;
    let mut module = Parser::new(tokens)
        .parse_module("model")
        .map_err(|e| located(source, e.message.clone(), e.line, e.col))?;
    normalize_bindings(&mut module).map_err(|e| {
        let (line, col) = e.position();
        located(source, strip_locator(&e.to_string()), line, col)
    })?;

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
        Err(e) => Err(located(source, format!("{e}"), 0, 0)),
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

/// The data a shareable, re-runnable report renders from.
///
/// `source_sha256` is the hash of the exact solved source (raw UTF-8 bytes);
/// M1/M2 bind it into the report as the ledger's first fact so `verify-report`
/// can detect a swapped model that merely re-solves to the same numbers.
#[derive(Debug, Clone)]
pub struct Report {
    pub source: String,
    pub source_sha256: String,
    pub values: BTreeMap<String, f64>,
    pub derivations: BTreeMap<String, String>,
    pub ledger_head: String,
    pub ledger_jsonl: String,
}

/// The causal derivation tree for `var`, as `avenc why` prints it.
pub fn why(source: &str, var: &str, depth: Option<usize>) -> Result<String, Diagnostic> {
    let (ledger, _values) = run_with_ledger(source)?;
    Ok(ledger.why_with_depth(var, depth.or(Some(DEFAULT_MAX_DEPTH))))
}

/// Assemble everything a report needs: the exact source and its byte-hash, the
/// settled values, the derivation of each derived value, and the ledger head.
pub fn report(source: &str) -> Result<Report, Diagnostic> {
    let (ledger, values) = run_with_ledger(source)?;
    let mut derivations = BTreeMap::new();
    for name in values.keys() {
        if ledger
            .latest(name)
            .map(|f| f.kind == "derived")
            .unwrap_or(false)
        {
            derivations.insert(
                name.clone(),
                ledger.why_with_depth(name, Some(DEFAULT_MAX_DEPTH)),
            );
        }
    }
    Ok(Report {
        source: source.to_string(),
        source_sha256: sha256_hex(source.as_bytes()),
        values,
        derivations,
        ledger_head: ledger.head().to_string(),
        ledger_jsonl: ledger.to_jsonl(),
    })
}

/// Solve the module and build its fact ledger — input facts from the solve
/// bindings, then derived facts carrying their causes — the way the CLI's run
/// path does, for the pure-relation case (no external/IPC/SHM backends).
/// Shared by `why` and `report`.
fn run_with_ledger(source: &str) -> Result<(Ledger, BTreeMap<String, f64>), Diagnostic> {
    if let Some(d) = check(source).into_iter().next() {
        return Err(d);
    }
    let tokens = Lexer::new(source)
        .tokenize()
        .map_err(|e| Diagnostic::error(e.message.clone(), e.line, e.col))?;
    let mut module = Parser::new(tokens)
        .parse_module("model")
        .map_err(|e| Diagnostic::error(e.message.clone(), e.line, e.col))?;
    // Convert bound values to base units (unit errors surface via `check`).
    let _ = normalize_bindings(&mut module);

    let mut ledger = Ledger::new();
    let mut env: HashMap<String, f64> = HashMap::new();
    let mut seq: usize = 0;
    for solve in &module.solves {
        for b in &solve.bindings {
            seq += 1;
            env.insert(b.name.clone(), b.value);
            ledger.append(Fact {
                seq,
                tick: 0,
                kind: "input".to_string(),
                name: b.name.clone(),
                value: b.value,
                causes: Vec::new(),
                origin: "solve binding".to_string(),
                metadata: None,
            });
        }
    }

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
    for solve in &module.solves {
        for c in &solve.constraints {
            solver.add_constraint(c.clone());
        }
        for o in &solve.objectives {
            solver.add_objective(o.clone());
        }
        for iv in &solve.integer_vars {
            solver.add_integer_var(iv.clone());
        }
    }

    let solved = solver
        .solve(&env)
        .map_err(|e| Diagnostic::error(format!("{e}"), 0, 0))?;
    for (k, v) in &solved {
        if !env.contains_key(k) {
            seq += 1;
            let mut causes = Vec::new();
            let mut origin = "derived".to_string();
            if let Some(eq) = solver.relations.iter().find(|e| &e.lhs == k) {
                origin = format!("= {}", eq.rhs);
                for input_var in eq.rhs.vars() {
                    if let Some(fact) = ledger.latest(&input_var) {
                        causes.push(fact.seq);
                    }
                }
            }
            ledger.append(Fact {
                seq,
                tick: 0,
                kind: "derived".to_string(),
                name: k.clone(),
                value: *v,
                causes,
                origin,
                metadata: None,
            });
        }
    }
    Ok((ledger, solved.into_iter().collect()))
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
        assert_eq!(diags[0].end.line, diags[0].start.line);
        assert!(
            diags[0].end.col > diags[0].start.col,
            "span should cover the rest of the line, got {:?}",
            (diags[0].start, diags[0].end)
        );
    }

    #[test]
    fn solve_converts_declared_units_before_solving() {
        // 90 km/h = 25 m/s, 2.5 h = 9000 s, distance = 225_000 m.
        let src = "signal speed : km/h\nsignal time : h\nrelation Travel {\n distance = speed * time\n}\nsolve Travel with {\n speed = 90 km/h\n time = 2.5 h\n}\n";
        let solved = solve(src).expect("should solve");
        let distance = solved.values.get("distance").copied().expect("distance");
        assert!((distance - 225_000.0).abs() < 1e-6, "distance = {distance}");
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
    fn solve_refuses_a_unit_mismatch_instead_of_returning_a_number() {
        let src = "signal v : km/h\nsignal t : s\nrelation R {\n bad = v + t\n}\nsolve R with {\n v = 10 km/h\n t = 2 s\n}\n";
        let err = solve(src).expect_err("adding km/h to s must not settle a value");
        assert_eq!(err.message, "Can't add m·s^-1 to s (line 4).");
        assert_eq!(check(src)[0].message, err.message);
        assert_eq!(
            report(src).expect_err("a bad model has no report").message,
            err.message
        );
        assert_eq!(
            why(src, "bad", None)
                .expect_err("a bad model has no derivation")
                .message,
            err.message
        );
    }

    #[test]
    fn solve_refuses_text_that_is_not_a_model() {
        let err = solve("not a model").expect_err("prose must not solve");
        assert!(err.message.contains("expected"), "{}", err.message);
    }

    #[test]
    fn solve_runs_forward() {
        let src =
            "relation N {\n force = mass * accel\n}\nsolve N with {\n mass = 2\n accel = 3\n}\n";
        let solved = solve(src).expect("should solve");
        assert!((solved.values["force"] - 6.0).abs() < 1e-9);
    }

    #[test]
    fn why_traces_a_derived_value_to_its_inputs() {
        let src =
            "relation N {\n force = mass * accel\n}\nsolve N with {\n mass = 2\n accel = 3\n}\n";
        let tree = why(src, "force", None).expect("why");
        assert!(tree.contains("force"), "{tree}");
        assert!(tree.contains("mass") || tree.contains("accel"), "{tree}");
    }

    #[test]
    fn report_has_source_hash_values_derivation_and_head() {
        let src =
            "relation N {\n force = mass * accel\n}\nsolve N with {\n mass = 2\n accel = 3\n}\n";
        let r = report(src).expect("report");
        assert_eq!(r.source_sha256.len(), 64);
        assert!((r.values["force"] - 6.0).abs() < 1e-9);
        assert!(!r.ledger_head.is_empty());
        assert!(
            r.derivations.contains_key("force"),
            "force should be derived"
        );
    }

    #[test]
    fn report_source_hash_tracks_the_source() {
        let a = report("relation N {\n y = 2 * x\n}\nsolve N with {\n x = 3\n}\n").unwrap();
        let b = report("relation N {\n y = 2 * x\n}\nsolve N with {\n x = 4\n}\n").unwrap();
        assert_ne!(a.source_sha256, b.source_sha256);
    }
}
