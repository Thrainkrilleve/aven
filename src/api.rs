// src/api.rs — one stable surface the IDE, the CLI, and the WASM build all call.
//
// Everything here is a thin wrapper over the existing compiler pieces (lexer,
// parser, dimensional checker, solver). It adds no new checking or solving
// logic, so there stays exactly one source of truth for the math.

use crate::ast::BlockKind;
use crate::evaluator::sha256_hex;
use crate::ledger::{Fact, Ledger, DEFAULT_MAX_DEPTH};
use crate::lexer::Lexer;
use crate::parser::Parser;
use crate::solver::SolverEngine;
use crate::units::{normalize_bindings, DimensionalChecker, TypeError, UnitScale};
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
    /// The same values in each variable's declared unit. Base units stay in `values`.
    pub shown: BTreeMap<String, f64>,
    /// Declared unit, or the base unit of a dimensioned result. Empty when dimensionless.
    pub units: BTreeMap<String, String>,
    /// Variables the solve block left blank.
    pub solving: Vec<String>,
    /// Variables the solve block binds, so the panel can blank one of them.
    pub bound: Vec<String>,
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
        Ok(settled) => {
            let values: BTreeMap<String, f64> = settled.into_iter().collect();
            let face = present(&module, &values);
            Ok(Solved {
                values,
                shown: face.shown,
                units: face.units,
                solving: face.solving,
                bound: face.bound,
                trace: solver.trace.clone(),
            })
        }
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
/// `source_sha256` is the hash of the exact solved source (raw UTF-8 bytes).
/// That hash is the ledger's first fact (`kind` `source`, `origin` the hex
/// digest) so a later check can see a swapped model that re-solves to the
/// same numbers.
#[derive(Debug, Clone)]
pub struct Report {
    pub source: String,
    pub source_sha256: String,
    pub values: BTreeMap<String, f64>,
    pub shown: BTreeMap<String, f64>,
    pub units: BTreeMap<String, String>,
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
    let face = present_source(source, &values);
    Ok(Report {
        source: source.to_string(),
        source_sha256: sha256_hex(source.as_bytes()),
        values,
        shown: face.shown,
        units: face.units,
        derivations,
        ledger_head: ledger.head().to_string(),
        ledger_jsonl: ledger.to_jsonl(),
    })
}

/// A self-contained HTML report. The model source is escaped, so a file
/// cannot inject a script into the page.
pub fn report_html(source: &str) -> Result<String, Diagnostic> {
    let report = report(source)?;
    Ok(render_report_html(&report))
}

pub fn render_report_html(report: &Report) -> String {
    let mut values = String::new();
    for (name, value) in &report.values {
        let shown = report.shown.get(name).copied().unwrap_or(*value);
        let unit = report.units.get(name).map(String::as_str).unwrap_or("");
        values.push_str(&format!(
            "<tr><td>{}</td><td>{}</td><td>{}</td></tr>\n",
            escape_html(name),
            escape_html(&format_number(shown)),
            escape_html(unit)
        ));
    }
    let mut derivations = String::new();
    for (name, tree) in &report.derivations {
        derivations.push_str(&format!(
            "<h3>{}</h3>\n<pre>{}</pre>\n",
            escape_html(name),
            escape_html(tree)
        ));
    }
    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>Aven report</title>
<style>
body {{ font-family: ui-sans-serif, system-ui, sans-serif; margin: 2rem; color: #14202b; }}
pre, code, td {{ font-family: ui-monospace, Menlo, Consolas, monospace; }}
table {{ border-collapse: collapse; }}
td {{ border-bottom: 1px solid #d5dde6; padding: .25rem .8rem; }}
.hash {{ word-break: break-all; color: #445; }}
</style>
</head>
<body>
<h1>Aven report</h1>
<p class="hash">source sha256<br>{sha}</p>
<p class="hash">ledger head<br>{head}</p>
<h2>Source</h2>
<pre>{source}</pre>
<h2>Values</h2>
<table>
<tr><td>name</td><td>value</td><td>unit</td></tr>
{values}</table>
<h2>Derivation</h2>
{derivations}
<h2>Ledger</h2>
<pre>{ledger}</pre>
</body>
</html>
"#,
        sha = escape_html(&report.source_sha256),
        head = escape_html(&report.ledger_head),
        source = escape_html(&report.source),
        values = values,
        derivations = derivations,
        ledger = escape_html(&report.ledger_jsonl),
    )
}

fn escape_html(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

/// Rewrite `source` so `target` is the blank variable. Other blanks are pinned
/// to the values this source just settled, in their declared units.
pub fn solve_for(source: &str, target: &str) -> Result<String, Diagnostic> {
    if let Some(d) = check(source).into_iter().next() {
        return Err(d);
    }
    let module = parse_source(source)?;
    if module.solves.is_empty() {
        return Err(located(
            source,
            "This model has no solve block. A solve block is where a variable can be left blank.",
            1,
            1,
        ));
    }
    let solved = solve(source)?;
    let vars = relation_vars(&module);
    if !vars.contains(target) {
        return Err(located(
            source,
            format!("{target} is not a variable in this model."),
            1,
            1,
        ));
    }
    let bound: std::collections::BTreeSet<String> = module
        .solves
        .iter()
        .flat_map(|s| s.bindings.iter().map(|b| b.name.clone()))
        .collect();
    let unbound: Vec<String> = vars
        .iter()
        .filter(|name| !bound.contains(*name))
        .cloned()
        .collect();
    if unbound.len() == 1 && unbound[0] == target {
        return Ok(source.to_string());
    }
    let scale = UnitScale::for_module(&module).map_err(|e| {
        let (line, col) = e.position();
        located(source, strip_locator(&e.to_string()), line, col)
    })?;
    let mut pins = Vec::new();
    for name in &unbound {
        if name == target {
            continue;
        }
        let base =
            solved.values.get(name).copied().ok_or_else(|| {
                located(source, format!("{name} has no settled value to pin."), 1, 1)
            })?;
        let shown = scale.to_declared(name, base);
        let unit = scale.label(name, None);
        pins.push(binding_line(name, shown, &unit));
    }
    let solve_block = module
        .solves
        .iter()
        .find(|s| s.bindings.iter().any(|b| b.name == target))
        .or_else(|| module.solves.first())
        .unwrap();
    let close = solve_close_line(source, solve_block.line).ok_or_else(|| {
        located(
            source,
            "The solve block has no closing brace.",
            solve_block.line,
            solve_block.col,
        )
    })?;
    let drop_lines: Vec<usize> = solve_block
        .bindings
        .iter()
        .filter(|b| b.name == target)
        .map(|b| b.line - 1)
        .collect();
    Ok(rewrite_solve(source, &drop_lines, close, &pins))
}

struct Face {
    shown: BTreeMap<String, f64>,
    units: BTreeMap<String, String>,
    solving: Vec<String>,
    bound: Vec<String>,
}

fn present(module: &crate::ast::AvenModule, values: &BTreeMap<String, f64>) -> Face {
    let scale = UnitScale::for_module(module).unwrap_or_default();
    let dims = DimensionalChecker::new()
        .check_module(module)
        .map(|r| r.variable_dimensions)
        .unwrap_or_default();
    let mut shown = BTreeMap::new();
    let mut units = BTreeMap::new();
    for (name, value) in values {
        shown.insert(name.clone(), scale.to_declared(name, *value));
        let label = scale.label(name, dims.get(name));
        if !label.is_empty() {
            units.insert(name.clone(), label);
        }
    }
    let vars = relation_vars(module);
    let bound_set: std::collections::BTreeSet<String> = module
        .solves
        .iter()
        .flat_map(|s| s.bindings.iter().map(|b| b.name.clone()))
        .collect();
    let solving = vars
        .iter()
        .filter(|name| !bound_set.contains(*name))
        .cloned()
        .collect();
    let bound = bound_set
        .into_iter()
        .filter(|name| vars.contains(name))
        .collect();
    Face {
        shown,
        units,
        solving,
        bound,
    }
}

fn present_source(source: &str, values: &BTreeMap<String, f64>) -> Face {
    match parse_source(source) {
        Ok(module) => present(&module, values),
        Err(_) => Face {
            shown: values.clone(),
            units: BTreeMap::new(),
            solving: Vec::new(),
            bound: Vec::new(),
        },
    }
}

fn parse_source(source: &str) -> Result<crate::ast::AvenModule, Diagnostic> {
    let tokens = Lexer::new(source)
        .tokenize()
        .map_err(|e| located(source, e.message.clone(), e.line, e.col))?;
    Parser::new(tokens)
        .parse_module("model")
        .map_err(|e| located(source, e.message.clone(), e.line, e.col))
}

fn relation_vars(module: &crate::ast::AvenModule) -> std::collections::BTreeSet<String> {
    let mut vars = std::collections::BTreeSet::new();
    for blk in &module.blocks {
        if blk.kind != BlockKind::Relation {
            continue;
        }
        for eq in &blk.equations {
            vars.insert(eq.lhs.clone());
            vars.extend(eq.rhs.vars());
        }
    }
    vars
}

fn binding_line(name: &str, value: f64, unit: &str) -> String {
    if unit.is_empty() {
        format!("    {name} = {}\n", format_number(value))
    } else {
        format!("    {name} = {} {unit}\n", format_number(value))
    }
}

fn format_number(v: f64) -> String {
    if !v.is_finite() {
        return "0".to_string();
    }
    let rounded = v.round();
    if (v - rounded).abs() < 1e-9 && rounded.abs() < 1e15 {
        return format!("{}", rounded as i64);
    }
    let text = format!("{v:.12}");
    let trimmed = text.trim_end_matches('0').trim_end_matches('.');
    if trimmed.is_empty() || trimmed == "-" {
        "0".to_string()
    } else {
        trimmed.to_string()
    }
}

fn solve_close_line(source: &str, solve_start_line: usize) -> Option<usize> {
    let mut depth = 0i32;
    for (index, line) in source
        .lines()
        .enumerate()
        .skip(solve_start_line.saturating_sub(1))
    {
        for ch in line.chars() {
            if ch == '{' {
                depth += 1;
            } else if ch == '}' {
                depth -= 1;
                if depth == 0 {
                    return Some(index);
                }
            }
        }
    }
    None
}

fn rewrite_solve(source: &str, drop_lines: &[usize], close: usize, pins: &[String]) -> String {
    let mut lines = split_keep(source);
    let mut close = close;
    let mut drop: Vec<usize> = drop_lines.to_vec();
    drop.sort_unstable();
    drop.dedup();
    for index in drop.into_iter().rev() {
        if index < lines.len() {
            lines.remove(index);
            if index < close {
                close -= 1;
            }
        }
    }
    if close > lines.len() {
        close = lines.len();
    }
    for (offset, pin) in pins.iter().enumerate() {
        lines.insert(close + offset, pin.clone());
    }
    lines.concat()
}

fn split_keep(source: &str) -> Vec<String> {
    let mut lines = Vec::new();
    let mut rest = source;
    while let Some(i) = rest.find('\n') {
        let (line, tail) = rest.split_at(i + 1);
        lines.push(line.to_string());
        rest = tail;
    }
    if !rest.is_empty() {
        lines.push(rest.to_string());
    }
    lines
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
    // The solved source is the first fact. Later facts chain from it, so a
    // swapped model cannot keep this head.
    seq += 1;
    ledger.append(Fact {
        seq,
        tick: 0,
        kind: "source".to_string(),
        name: "source_sha256".to_string(),
        value: 0.0,
        causes: Vec::new(),
        origin: sha256_hex(source.as_bytes()),
        metadata: None,
    });
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
        assert!((solved.shown["speed"] - 90.0).abs() < 1e-6);
        assert_eq!(solved.units.get("speed").map(String::as_str), Some("km/h"));
        assert_eq!(solved.units.get("time").map(String::as_str), Some("h"));
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
        assert_eq!(
            err.message,
            "a calculation is one expression; found 'a' after it"
        );
    }

    fn calculation(src: &str) -> f64 {
        let solved = solve(src).unwrap_or_else(|e| panic!("{src} refused: {}", e.message));
        *solved
            .values
            .get("answer")
            .unwrap_or_else(|| panic!("{src} settled {:?}, no answer", solved.values))
    }

    #[test]
    fn solve_evaluates_basic_arithmetic() {
        assert_eq!(calculation("1+2"), 3.0);
        assert_eq!(calculation("32 ^ 3"), 32.0_f64.powi(3));
        assert_eq!(calculation("32^3"), 32768.0);
        assert_eq!(calculation("1 + 2 * 3"), 7.0);
        assert_eq!(calculation("(1 + 2) * 3"), 9.0);
        assert_eq!(calculation("2 ^ 3 ^ 2"), 512.0);
        assert_eq!(calculation("sqrt(9)"), 3.0);
        assert_eq!(calculation("1/2"), 0.5);
        assert_eq!(calculation("-8 / 2"), -4.0);
        assert!((calculation("2 * pi") - std::f64::consts::TAU).abs() < 1e-12);
        assert!(check("1+2").is_empty());
        let reported = report("1+2").expect("a calculation has a report");
        assert_eq!(reported.values["answer"], 3.0);
    }

    #[test]
    fn solve_refuses_a_calculation_that_is_not_one_closed_expression() {
        let unknown = solve("x + 1").expect_err("a free name is not a number");
        assert_eq!(
            unknown.message,
            "x is still unknown. A calculation uses only numbers. Name unknowns in a relation and a solve block."
        );
        assert_eq!(check("x + 1")[0].message, unknown.message);

        let two = solve("y + x").expect_err("two free names");
        assert_eq!(
            two.message,
            "x and y are still unknown. A calculation uses only numbers. Name unknowns in a relation and a solve block."
        );
        let three = solve("z + y + x").expect_err("three free names");
        assert!(
            three.message.starts_with("x, y, and z are still unknown."),
            "{}",
            three.message
        );

        let second = solve("1 + 2\n3 + 4").expect_err("two expressions");
        assert_eq!(
            second.message,
            "a calculation is one expression; found '3' after it"
        );

        let unit_word = solve("1 m + 2").expect_err("a unit word is not part of the expression");
        assert_eq!(
            unit_word.message,
            "a calculation is one expression; found 'm' after it"
        );

        let div = solve("1/0").expect_err("division by zero");
        assert!(div.message.contains("division by zero"), "{}", div.message);
        assert!(check("1/0").is_empty(), "the expression is well formed");

        let neg_sqrt = solve("sqrt(-1)").expect_err("sqrt of a negative");
        assert!(
            neg_sqrt.message.contains("sqrt of negative"),
            "{}",
            neg_sqrt.message
        );

        let unknown_fn = solve("bogus(1)").expect_err("unknown function");
        assert!(
            unknown_fn.message.contains("unknown function"),
            "{}",
            unknown_fn.message
        );
    }

    fn settled(src: &str) -> BTreeMap<String, f64> {
        solve(src)
            .unwrap_or_else(|e| panic!("{src} refused: {}", e.message))
            .values
    }

    #[test]
    fn solve_names_an_equation() {
        let values = settled("x = 1 + 10");
        assert_eq!(values.get("x").copied(), Some(11.0));
        assert!(values.get("answer").is_none(), "{values:?}");
        assert!(check("x = 1 + 10").is_empty());
        let reported = report("x = 1 + 10").expect("a named equation has a report");
        assert_eq!(reported.values["x"], 11.0);
        let tree = why("x = 1 + 10", "x", None).expect("why");
        assert!(tree.contains("x"), "{tree}");
    }

    #[test]
    fn solve_chains_named_equations_in_either_order() {
        for src in [
            "x = 1 + 10\ny = x * 2",
            "y = x * 2\nx = 1 + 10",
            "z = y + x\ny = x * 2\nx = 1 + 10",
        ] {
            let values = settled(src);
            assert_eq!(values["x"], 11.0, "{src}");
            assert_eq!(values["y"], 22.0, "{src}");
        }
        assert_eq!(settled("z = y + x\ny = x * 2\nx = 1 + 10")["z"], 33.0);
        assert_eq!(settled("x = -1 + 10")["x"], 9.0);
    }

    #[test]
    fn solve_refuses_a_named_equation_that_does_not_close() {
        let missing = solve("x = y + 1").expect_err("y has no equation");
        assert_eq!(
            missing.message,
            "y is still unknown. Give y an equation, or name unknowns in a relation and a solve block."
        );
        assert_eq!(check("x = y + 1")[0].message, missing.message);

        let two = solve("x = a + 1\ny = b + 2").expect_err("two missing names");
        assert_eq!(
            two.message,
            "a and b are still unknown. Give each one an equation, or name unknowns in a relation and a solve block."
        );

        let itself = solve("x = x + 1").expect_err("x never receives a number");
        assert_eq!(
            itself.message,
            "x is still unknown. Give x an equation, or name unknowns in a relation and a solve block."
        );

        let twice = solve("x = 1\nx = 2").expect_err("one name, two expressions");
        assert_eq!(
            twice.message,
            "x is named twice. A calculation gives each name one expression."
        );

        let unit_word = solve("x = 1 m + 10 m").expect_err("unit words are leftover tokens");
        assert_eq!(
            unit_word.message,
            "a calculation is one expression; found 'm' after it"
        );

        let div = solve("x = 1 / 0").expect_err("division by zero");
        assert!(div.message.contains("division by zero"), "{}", div.message);

        let cycle = solve("x = y + 1\ny = x + 1").expect_err("a cycle has no number");
        assert!(
            cycle.message.contains("under-determined"),
            "{}",
            cycle.message
        );
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
        assert_ne!(a.ledger_head, b.ledger_head);
    }

    #[test]
    fn report_ledger_starts_with_the_source_hash() {
        let src = "relation N {\n y = 2 * x\n}\nsolve N with {\n x = 3\n}\n";
        let r = report(src).unwrap();
        let first = r.ledger_jsonl.lines().next().expect("a fact");
        assert!(first.contains("\"seq\": 1"), "{first}");
        assert!(first.contains("\"kind\": \"source\""), "{first}");
        assert!(first.contains("\"name\": \"source_sha256\""), "{first}");
        assert!(
            first.contains(&format!("\"origin\": \"{}\"", r.source_sha256)),
            "{first}"
        );
        assert_eq!(r.source_sha256, sha256_hex(src.as_bytes()));
    }

    #[test]
    fn report_html_escapes_the_source_and_names_the_hash() {
        let src = "relation N {\n // <script>alert(1)</script>\n y = 2 * x\n}\nsolve N with {\n x = 3\n}\n";
        let html = report_html(src).expect("html");
        assert!(
            html.contains("&lt;script&gt;alert(1)&lt;/script&gt;"),
            "{html}"
        );
        assert!(!html.contains("<script>alert(1)</script>"), "{html}");
        let digest = sha256_hex(src.as_bytes());
        assert!(html.contains(&digest), "report should show the source hash");
        assert!(html.contains("&quot;kind&quot;: &quot;source&quot;"));
    }

    #[test]
    fn solve_for_blanks_the_chosen_variable_and_pins_the_old_one() {
        let src = "relation Newton {\n force = mass * acceleration\n}\nsolve Newton with {\n force = 4200\n acceleration = 5\n}\n";
        let first = solve(src).expect("mass solves backward");
        assert!((first.values["mass"] - 840.0).abs() < 1e-6);
        assert_eq!(first.solving, vec!["mass".to_string()]);
        assert!(first.bound.contains(&"force".to_string()));

        let rewritten = solve_for(src, "force").expect("retarget");
        assert!(
            rewritten.contains("mass = 840"),
            "the old blank is pinned:\n{rewritten}"
        );
        assert!(
            !rewritten.contains("force = 4200"),
            "force is blank:\n{rewritten}"
        );
        assert!(rewritten.contains("force = mass * acceleration"));
        let second = solve(&rewritten).expect("force solves backward");
        assert!((second.values["force"] - 4200.0).abs() < 1e-6);
        assert!((second.values["mass"] - 840.0).abs() < 1e-6);
        assert_eq!(second.solving, vec!["force".to_string()]);
    }
}
