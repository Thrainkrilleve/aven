// src/api.rs — one stable surface the IDE, the CLI, and the WASM build all call.
//
// Everything here is a thin wrapper over the existing compiler pieces (lexer,
// parser, dimensional checker, solver). It adds no new checking or solving
// logic, so there stays exactly one source of truth for the math.

use crate::ast::BlockKind;
use crate::evaluator::{knee_of_series, sha256_hex};
use crate::ledger::{DEFAULT_MAX_DEPTH, Fact, Ledger, from_jsonl};
use crate::lexer::Lexer;
use crate::parser::Parser;
use crate::solver::SolverEngine;
use crate::units::{DimensionalChecker, TypeError, UnitScale, module_registry, normalize_bindings};
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
    /// Plain sentences for the guide: which rule ran, why that direction, and
    /// the pinned numbers that produced each result.
    pub explanation: String,
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
        TypeError::UnknownFunction { name, line, .. } => {
            if name == "log10" {
                format!("No log10 function (line {line}). Base-10 log is ln(x) / ln(10).")
            } else {
                format!("Unknown function '{name}' (line {line}).")
            }
        }
        TypeError::UndeclaredSignal { name, line, .. } => {
            format!(
                "'{name}' has no declared unit (line {line}). Add 'signal {name} : <unit>' (or ': scalar')."
            )
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
            let explanation = explain_solution(&solver.trace, &face.shown, &face.units);
            Ok(Solved {
                values,
                shown: face.shown,
                units: face.units,
                solving: face.solving,
                bound: face.bound,
                trace: solver.trace.clone(),
                explanation,
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

fn unescape_html(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('&') {
        out.push_str(&rest[..start]);
        rest = &rest[start..];
        if let Some(end) = rest.find(';') {
            let entity = &rest[..=end];
            let ch = match entity {
                "&lt;" => Some('<'),
                "&gt;" => Some('>'),
                "&quot;" => Some('"'),
                "&#39;" => Some('\''),
                "&amp;" => Some('&'),
                _ => None,
            };
            if let Some(ch) = ch {
                out.push(ch);
                rest = &rest[end + 1..];
                continue;
            }
        }
        out.push('&');
        rest = &rest[1..];
    }
    out.push_str(rest);
    out
}

/// What `verify-report` reports when a file passes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReportVerification {
    pub source_sha256: String,
    pub ledger_head: String,
    pub facts: usize,
}

/// Check an HTML report: the embedded source matches the ledger's first fact,
/// re-solving that source reproduces each fact's `value_bits`, and the chain
/// is intact. The values table has to match the re-solved numbers too.
/// The model text embedded in an HTML report.
pub fn embedded_source(html: &str) -> Result<String, String> {
    Ok(unescape_html(&section_pre(html, "Source")?))
}

pub fn verify_report_html(html: &str) -> Result<ReportVerification, String> {
    let source = unescape_html(&section_pre(html, "Source")?);
    let ledger_text = unescape_html(&section_pre(html, "Ledger")?);
    let digest = sha256_hex(source.as_bytes());
    let loaded = from_jsonl(&ledger_text, None)
        .map_err(|err| format!("The ledger does not verify: {err}"))?;
    let first = loaded
        .facts()
        .first()
        .ok_or_else(|| "The ledger has no facts.".to_string())?;
    if first.kind != "source" || first.origin != digest {
        return Err(format!(
            "The embedded source does not match the ledger's first fact. The file hashes to {digest}."
        ));
    }
    let (fresh, values) = run_with_ledger(&source).map_err(|d| d.message)?;
    for fact in fresh.facts() {
        if fact.kind == "source" {
            continue;
        }
        let Some(stored) = loaded.latest(&fact.name) else {
            return Err(format!("The ledger has no fact for {}.", fact.name));
        };
        if stored.kind != fact.kind || stored.value.to_bits() != fact.value.to_bits() {
            return Err(format!(
                "Re-solving {} does not reproduce the report.",
                fact.name
            ));
        }
    }
    for fact in loaded.facts() {
        if fact.kind == "source" {
            continue;
        }
        let Some(again) = fresh.latest(&fact.name) else {
            return Err(format!(
                "The ledger records {}, which this model does not settle.",
                fact.name
            ));
        };
        if again.kind != fact.kind || again.value.to_bits() != fact.value.to_bits() {
            return Err(format!(
                "Re-solving {} does not reproduce the report.",
                fact.name
            ));
        }
    }
    let face = present_source(&source, &values);
    check_value_table(html, &face)?;
    Ok(ReportVerification {
        source_sha256: digest,
        ledger_head: loaded.head().to_string(),
        facts: loaded.len(),
    })
}

fn section_pre(html: &str, heading: &str) -> Result<String, String> {
    let marker = format!("<h2>{heading}</h2>");
    let Some(at) = html.find(&marker) else {
        return Err(format!("The report has no {heading} section."));
    };
    let rest = &html[at + marker.len()..];
    let Some(open) = rest.find("<pre>") else {
        return Err(format!("The report's {heading} section has no text."));
    };
    let rest = &rest[open + 5..];
    let Some(close) = rest.find("</pre>") else {
        return Err(format!("The report's {heading} section is cut off."));
    };
    Ok(rest[..close].to_string())
}

fn check_value_table(html: &str, face: &Face) -> Result<(), String> {
    let marker = "<h2>Values</h2>";
    let Some(at) = html.find(marker) else {
        return Err("The report has no Values section.".to_string());
    };
    let rest = &html[at + marker.len()..];
    let Some(table_end) = rest.find("</table>") else {
        return Err("The report's Values section is cut off.".to_string());
    };
    let table = &rest[..table_end];
    let mut seen = std::collections::BTreeSet::new();
    let mut search = table;
    while let Some(row_at) = search.find("<tr>") {
        let search_from = &search[row_at + 4..];
        let Some(row_end) = search_from.find("</tr>") else {
            break;
        };
        let row = &search_from[..row_end];
        search = &search_from[row_end + 5..];
        let cells = html_cells(row);
        if cells.first().map(String::as_str) == Some("name") {
            continue;
        }
        let name = cells
            .first()
            .map(|s| s.trim().to_string())
            .unwrap_or_default();
        if name.is_empty() {
            continue;
        }
        let shown = face
            .shown
            .get(&name)
            .ok_or_else(|| format!("The report lists {name}, which this model does not settle."))?;
        let expect = format_number(*shown);
        let got = cells.get(1).map(|s| s.trim()).unwrap_or("");
        if got != expect {
            return Err(format!(
                "The report shows {name} as {got} but the model settles at {expect}."
            ));
        }
        let unit = face.units.get(&name).map(String::as_str).unwrap_or("");
        let got_unit = cells.get(2).map(|s| s.trim()).unwrap_or("");
        if got_unit != unit {
            return Err(format!(
                "The report shows {name} in \"{got_unit}\" but the model uses \"{unit}\"."
            ));
        }
        seen.insert(name);
    }
    for name in face.shown.keys() {
        if !seen.contains(name) {
            return Err(format!("The report omits {name}."));
        }
    }
    Ok(())
}

fn html_cells(row: &str) -> Vec<String> {
    let mut cells = Vec::new();
    let mut rest = row;
    while let Some(at) = rest.find("<td>") {
        rest = &rest[at + 4..];
        let end = rest.find("</td>").unwrap_or(rest.len());
        cells.push(unescape_html(rest[..end].trim()));
        rest = &rest[end..];
    }
    cells
}

/// A printable report. The HTML file remains the one `verify-report` checks.
pub fn report_pdf(source: &str) -> Result<Vec<u8>, Diagnostic> {
    let report = report(source)?;
    Ok(render_report_pdf(&report))
}

pub fn render_report_pdf(report: &Report) -> Vec<u8> {
    let mut lines = Vec::new();
    lines.push("Aven report".to_string());
    lines.push(String::new());
    lines.push(format!("source sha256 {}", report.source_sha256));
    lines.push(format!("ledger head {}", report.ledger_head));
    lines.push(String::new());
    lines.push("Source".to_string());
    for line in report.source.lines() {
        push_wrapped(&mut lines, line, 90);
    }
    lines.push(String::new());
    lines.push("Values".to_string());
    for (name, value) in &report.values {
        let shown = report.shown.get(name).copied().unwrap_or(*value);
        let unit = report.units.get(name).map(String::as_str).unwrap_or("");
        let gap = if unit.is_empty() { "" } else { " " };
        push_wrapped(
            &mut lines,
            &format!("{name}  {}{gap}{unit}", format_number(shown)),
            90,
        );
    }
    lines.push(String::new());
    lines.push("Derivation".to_string());
    for (name, tree) in &report.derivations {
        push_wrapped(&mut lines, name, 90);
        for line in tree.lines() {
            push_wrapped(&mut lines, line, 90);
        }
    }
    pdf_from_lines(&lines)
}

fn push_wrapped(lines: &mut Vec<String>, text: &str, width: usize) {
    if text.is_empty() {
        lines.push(String::new());
        return;
    }
    let mut rest = text;
    while rest.chars().count() > width {
        let end = rest
            .char_indices()
            .nth(width)
            .map(|(index, _)| index)
            .unwrap_or(rest.len());
        let (head, tail) = rest.split_at(end);
        lines.push(head.to_string());
        rest = tail;
    }
    lines.push(rest.to_string());
}

fn pdf_from_lines(lines: &[String]) -> Vec<u8> {
    const PER_PAGE: usize = 46;
    let chunks: Vec<&[String]> = if lines.is_empty() {
        vec![&[][..]]
    } else {
        lines.chunks(PER_PAGE).collect()
    };
    let page_count = chunks.len();
    let font_id = 3;
    let mut objects: Vec<Vec<u8>> = vec![Vec::new(), Vec::new(), Vec::new(), Vec::new()];
    let mut kids = String::new();
    for (index, chunk) in chunks.iter().enumerate() {
        let page_id = 4 + index * 2;
        let content_id = page_id + 1;
        kids.push_str(&format!("{page_id} 0 R "));
        let stream = pdf_page_stream(chunk);
        objects.push(format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents {content_id} 0 R /Resources << /Font << /F1 {font_id} 0 R >> >> >>"
        ).into_bytes());
        let mut content = format!("<< /Length {} >>\nstream\n", stream.len()).into_bytes();
        content.extend_from_slice(&stream);
        content.extend_from_slice(b"\nendstream");
        objects.push(content);
    }
    objects[1] = b"<< /Type /Catalog /Pages 2 0 R >>".to_vec();
    objects[2] = format!("<< /Type /Pages /Kids [{kids}] /Count {page_count} >>").into_bytes();
    objects[3] = b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec();

    let mut out = Vec::new();
    out.extend_from_slice(b"%PDF-1.4\n");
    let mut offsets = vec![0u32; objects.len()];
    for (id, data) in objects.iter().enumerate().skip(1) {
        offsets[id] = out.len() as u32;
        out.extend_from_slice(format!("{id} 0 obj\n").as_bytes());
        out.extend_from_slice(data);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len() as u32;
    let count = objects.len();
    out.extend_from_slice(format!("xref\n0 {count}\n").as_bytes());
    out.extend_from_slice(b"0000000000 65535 f \n");
    for offset in offsets.iter().skip(1) {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!("trailer\n<< /Size {count} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
    );
    out
}

fn pdf_page_stream(lines: &[String]) -> Vec<u8> {
    let mut stream = Vec::new();
    stream.extend_from_slice(b"BT\n/F1 10 Tf\n54 750 Td\n14 TL\n");
    for line in lines {
        stream.extend_from_slice(b"(");
        stream.extend_from_slice(&pdf_text(line));
        stream.extend_from_slice(b") Tj\nT*\n");
    }
    stream.extend_from_slice(b"ET");
    stream
}

fn pdf_text(text: &str) -> Vec<u8> {
    let mut out = Vec::new();
    for ch in text.chars() {
        match ch {
            '(' | ')' | '\\' => {
                out.push(b'\\');
                out.push(ch as u8);
            }
            c if (c as u32) < 128 => out.push(c as u8),
            '\u{00b7}' => out.push(0xB7),
            _ => out.push(b'?'),
        }
    }
    out
}

/// One row of a values sheet. `value` is the number as text, in `unit` when
/// the variable has one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SheetRow {
    pub name: String,
    pub value: String,
    pub unit: String,
}

/// Settled values, in declared units, ready to write as CSV or a spreadsheet.
pub fn sheet_rows(source: &str) -> Result<Vec<SheetRow>, Diagnostic> {
    let solved = solve(source)?;
    let mut rows = Vec::new();
    for (name, value) in &solved.values {
        let shown = solved.shown.get(name).copied().unwrap_or(*value);
        rows.push(SheetRow {
            name: name.clone(),
            value: format_number(shown),
            unit: solved.units.get(name).cloned().unwrap_or_default(),
        });
    }
    Ok(rows)
}

pub fn render_csv(rows: &[SheetRow]) -> String {
    let mut out = String::from("name,value,unit\n");
    for row in rows {
        out.push_str(&csv_field(&row.name));
        out.push(',');
        out.push_str(&csv_field(&row.value));
        out.push(',');
        out.push_str(&csv_field(&row.unit));
        out.push('\n');
    }
    out
}

fn csv_field(text: &str) -> String {
    if text.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", text.replace('"', "\"\""))
    } else {
        text.to_string()
    }
}

/// Read a headered sheet. The header must name `name` and `value`. `unit` is optional.
pub fn parse_csv(text: &str) -> Result<Vec<SheetRow>, String> {
    let records = csv_records(text)?;
    if records.is_empty() {
        return Err("The sheet is empty.".to_string());
    }
    let header: Vec<String> = records[0]
        .iter()
        .map(|cell| cell.trim().to_ascii_lowercase())
        .collect();
    let name_at = header
        .iter()
        .position(|cell| cell == "name")
        .ok_or_else(|| "The sheet needs a name column.".to_string())?;
    let value_at = header
        .iter()
        .position(|cell| cell == "value")
        .ok_or_else(|| "The sheet needs a value column.".to_string())?;
    let unit_at = header.iter().position(|cell| cell == "unit");
    let mut rows = Vec::new();
    for (index, record) in records.iter().skip(1).enumerate() {
        let name = record.get(name_at).map(|s| s.trim()).unwrap_or("");
        if name.is_empty() {
            continue;
        }
        let value = record.get(value_at).map(|s| s.trim()).unwrap_or("");
        if value.is_empty() {
            return Err(format!("Row {} ({name}) has no value.", index + 2));
        }
        let unit = unit_at
            .and_then(|at| record.get(at))
            .map(|s| s.trim().to_string())
            .unwrap_or_default();
        rows.push(SheetRow {
            name: name.to_string(),
            value: value.to_string(),
            unit,
        });
    }
    if rows.is_empty() {
        return Err("The sheet has no rows.".to_string());
    }
    Ok(rows)
}

fn csv_records(text: &str) -> Result<Vec<Vec<String>>, String> {
    let chars: Vec<char> = text.chars().collect();
    let mut records = Vec::new();
    let mut row = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut i = 0;
    while i < chars.len() {
        let ch = chars[i];
        if quoted {
            if ch == '"' {
                if chars.get(i + 1) == Some(&'"') {
                    field.push('"');
                    i += 2;
                    continue;
                }
                quoted = false;
                i += 1;
                continue;
            }
            field.push(ch);
            i += 1;
            continue;
        }
        match ch {
            '"' if field.is_empty() => {
                quoted = true;
                i += 1;
            }
            ',' => {
                row.push(std::mem::take(&mut field));
                i += 1;
            }
            '\n' | '\r' => {
                if ch == '\r' && chars.get(i + 1) == Some(&'\n') {
                    i += 1;
                }
                row.push(std::mem::take(&mut field));
                if row.iter().any(|cell| !cell.trim().is_empty()) {
                    records.push(std::mem::take(&mut row));
                } else {
                    row.clear();
                }
                i += 1;
            }
            _ => {
                field.push(ch);
                i += 1;
            }
        }
    }
    if quoted {
        return Err("A quoted field is missing its closing quote.".to_string());
    }
    if !field.is_empty() || !row.is_empty() {
        row.push(field);
        if row.iter().any(|cell| !cell.trim().is_empty()) {
            records.push(row);
        }
    }
    Ok(records)
}

/// Rewritten source after a sheet import, plus the names import did not apply.
#[derive(Debug)]
pub struct SheetImport {
    pub source: String,
    /// Names that belong to the model but are not solve-block inputs.
    /// Import leaves those blanks alone; their sheet cells are ignored.
    pub skipped: Vec<String>,
}

/// Replace solve-block inputs with the matching rows. Solved blanks stay blank,
/// so a round trip does not pin the variable the model is solving for.
/// A row that names a solved variable is reported in [`SheetImport::skipped`]
/// and is not parsed as a number, because that cell is never written.
pub fn apply_sheet(source: &str, rows: &[SheetRow]) -> Result<SheetImport, Diagnostic> {
    if let Some(diag) = check(source).into_iter().next() {
        return Err(diag);
    }
    let module = parse_source(source)?;
    if module.solves.is_empty() {
        return Err(located(
            source,
            "This model has no solve block. Import fills inputs that are already in a solve block.",
            1,
            1,
        ));
    }
    let vars = relation_vars(&module);
    let mut lines = split_keep(source);
    let mut updated = 0usize;
    let mut skipped = Vec::new();
    for row in rows {
        if !vars.contains(&row.name) {
            return Err(located(
                source,
                format!("{} is not a variable in this model.", row.name),
                1,
                1,
            ));
        }
        let Some(binding) = module
            .solves
            .iter()
            .flat_map(|solve| solve.bindings.iter())
            .find(|binding| binding.name == row.name)
        else {
            if !skipped.iter().any(|name| name == &row.name) {
                skipped.push(row.name.clone());
            }
            continue;
        };
        let value: f64 = row.value.trim().parse().map_err(|_| {
            located(
                source,
                format!("{} is not a number.", row.value.trim()),
                1,
                1,
            )
        })?;
        if !value.is_finite() {
            return Err(located(
                source,
                format!("{} is not a finite number.", row.name),
                1,
                1,
            ));
        }
        let unit = if row.unit.trim().is_empty() {
            binding.unit.clone().unwrap_or_default()
        } else {
            row.unit.trim().to_string()
        };
        let index = binding.line - 1;
        if index >= lines.len() {
            return Err(located(
                source,
                format!("Could not find the binding for {}.", row.name),
                binding.line,
                binding.col,
            ));
        }
        lines[index] = rewrite_binding_value(&lines[index], &format_number(value), &unit);
        updated += 1;
    }
    if updated == 0 {
        let msg = if skipped.is_empty() {
            "None of these rows match an input in the solve block.".to_string()
        } else {
            skipped_rows_message(&skipped)
        };
        return Err(located(source, msg, 1, 1));
    }
    Ok(SheetImport {
        source: lines.concat(),
        skipped,
    })
}

/// Turn a value in the unit Solve shows into the number written on the binding.
///
/// An empty binding unit means the source already speaks the shown unit (the
/// signal unit, or the base unit when there is no signal), so the number is
/// kept. A binding written in another unit of the same dimension is converted
/// so `apply_sheet` can stamp that unit on the number unchanged.
fn shown_to_binding_number(
    module: &crate::ast::AvenModule,
    name: &str,
    shown: f64,
    binding_unit: Option<&str>,
) -> Result<f64, String> {
    let Some(unit) = binding_unit.map(str::trim).filter(|unit| !unit.is_empty()) else {
        return Ok(shown);
    };
    let scale = UnitScale::for_module(module).map_err(|e| strip_locator(&e.to_string()))?;
    let registry = module_registry(module);
    let binding = registry
        .lookup(unit)
        .ok_or_else(|| format!("The binding unit '{unit}' is not a unit this model knows."))?;
    if let Some(signal) = module.signals.iter().find(|signal| signal.name == name) {
        if let Some(declared) = signal.unit.as_deref() {
            let shown_unit = registry.lookup(declared).ok_or_else(|| {
                format!("The signal unit '{declared}' is not a unit this model knows.")
            })?;
            if shown_unit.dimension != binding.dimension {
                return Err(format!(
                    "Can't sweep {name} in {declared} while its binding is in {unit}."
                ));
            }
        }
    }
    Ok(binding.to_unit_value(scale.to_base(name, shown)))
}

/// One solved point of a sweep: the swept input value and every variable's
/// shown value at that point.
#[derive(Debug, Clone)]
pub struct SweepPoint {
    pub x: f64,
    pub values: BTreeMap<String, f64>,
}

/// The result of sweeping one input across a range: the points that solved, the
/// units for labeling, the names whose value actually changes across the sweep
/// (what's worth plotting), and an optional knee index (objective models only).
#[derive(Debug, Clone)]
pub struct SweepResult {
    pub var: String,
    pub points: Vec<SweepPoint>,
    pub units: BTreeMap<String, String>,
    pub varies: Vec<String>,
    pub knee: Option<usize>,
}

/// Sweep one solve-block input across `min..=max` in `steps` intervals, solving
/// the model at each value and recording every settled variable. `min` and `max`
/// are in the unit Solve shows (the signal unit, otherwise the base unit). Each
/// sample is converted into the binding's own unit before it is written, so a
/// panel value in km/h is not stored as that many m/s. This loops the same
/// `solve` the rest of the surface uses, so a sweep curve can never diverge
/// from what Solve shows. Points that do not solve are skipped.
pub fn sweep(
    source: &str,
    var: &str,
    min: f64,
    max: f64,
    steps: usize,
) -> Result<SweepResult, Diagnostic> {
    if let Some(diag) = check(source).into_iter().next() {
        return Err(diag);
    }
    let module = parse_source(source)?;
    let is_input = module
        .solves
        .iter()
        .flat_map(|solve| solve.bindings.iter())
        .any(|binding| binding.name == var);
    if !is_input {
        return Err(located(
            source,
            format!(
                "{var} is not a solve-block input. Sweep an input and watch the outputs; use 'Solve for this' to make {var} an input first."
            ),
            1,
            1,
        ));
    }
    if !min.is_finite() || !max.is_finite() {
        return Err(located(
            source,
            "The sweep range must be two finite numbers.",
            1,
            1,
        ));
    }
    let steps = steps.clamp(1, 1000);
    let binding_unit = module
        .solves
        .iter()
        .flat_map(|solve| solve.bindings.iter())
        .find(|binding| binding.name == var)
        .and_then(|binding| binding.unit.clone());
    let mut points: Vec<SweepPoint> = Vec::new();
    let mut units: BTreeMap<String, String> = BTreeMap::new();
    for i in 0..=steps {
        let shown = min + (i as f64 / steps as f64) * (max - min);
        let written = shown_to_binding_number(&module, var, shown, binding_unit.as_deref())
            .map_err(|message| located(source, message, 1, 1))?;
        if !written.is_finite() {
            return Err(located(
                source,
                format!("Sweeping {var} produced a non-finite binding."),
                1,
                1,
            ));
        }
        let row = SheetRow {
            name: var.to_string(),
            value: format_number(written),
            unit: String::new(),
        };
        let edited = apply_sheet(source, std::slice::from_ref(&row))?;
        let Ok(solved) = solve(&edited.source) else {
            continue;
        };
        for (name, unit) in &solved.units {
            units.entry(name.clone()).or_insert_with(|| unit.clone());
        }
        let x = solved.shown.get(var).copied().unwrap_or(shown);
        let values: BTreeMap<String, f64> = solved
            .values
            .iter()
            .map(|(name, v)| (name.clone(), solved.shown.get(name).copied().unwrap_or(*v)))
            .collect();
        points.push(SweepPoint { x, values });
    }
    if points.is_empty() {
        return Err(located(
            source,
            "No value in that range solved. Try a different range.",
            1,
            1,
        ));
    }
    // A name is worth plotting only if it actually moves across the sweep.
    let mut varies: Vec<String> = Vec::new();
    if let Some(first) = points.first() {
        for name in first.values.keys() {
            if name == var {
                continue;
            }
            let mut lo = f64::INFINITY;
            let mut hi = f64::NEG_INFINITY;
            for point in &points {
                if let Some(v) = point.values.get(name) {
                    lo = lo.min(*v);
                    hi = hi.max(*v);
                }
            }
            if (hi - lo).abs() > 1e-9 {
                varies.push(name.clone());
            }
        }
    }
    varies.sort();
    // When the model declares two or more objectives, mark the knee of their
    // trade-off across the sweep — the balanced-compromise point.
    let mut targets: Vec<String> = Vec::new();
    for solve in &module.solves {
        for obj in &solve.objectives {
            if !targets.contains(&obj.target) {
                targets.push(obj.target.clone());
            }
        }
    }
    let knee = sweep_knee(&targets, &points);
    Ok(SweepResult {
        var: var.to_string(),
        points,
        units,
        varies,
        knee,
    })
}

/// The knee of a trade-off across the sweep. Same geometry as
/// [`knee_of_series`] (and therefore `find_knee_point`), measured on the
/// sweep's own solved points rather than an epsilon-filtered frontier, so the
/// index matches the samples the chart draws. `None` when fewer than two
/// objectives move, or fewer than three points solved. An objective that is
/// missing or non-finite is left out of the comparison instead of erasing the
/// knee of the objectives that did move.
fn sweep_knee(targets: &[String], points: &[SweepPoint]) -> Option<usize> {
    if targets.len() < 2 || points.len() <= 2 {
        return None;
    }
    let series: Vec<Vec<f64>> = targets
        .iter()
        .map(|t| {
            points
                .iter()
                .map(|p| p.values.get(t).copied().unwrap_or(f64::NAN))
                .collect()
        })
        .collect();
    knee_of_series(&series)
}

/// "Solve for this" is the button on a pinned input. It makes that input the
/// blank and pins the name that was solved, which is how a skipped cell becomes
/// settable. The button is not on the solved name itself.
fn skipped_rows_message(skipped: &[String]) -> String {
    let verb = if skipped.len() == 1 { "is" } else { "are" };
    let kind = if skipped.len() == 1 {
        "an input"
    } else {
        "inputs"
    };
    let pronoun = if skipped.len() == 1 { "it" } else { "them" };
    format!(
        "{} {verb} solved, not {kind}. Choose Solve for this on a pinned name to make that name the blank, then set {pronoun}.",
        join_names(skipped)
    )
}

fn rewrite_binding_value(line: &str, number: &str, unit: &str) -> String {
    let ending = if line.ends_with("\r\n") {
        "\r\n"
    } else if line.ends_with('\n') {
        "\n"
    } else {
        ""
    };
    let content = line.trim_end_matches(['\n', '\r']);
    let Some(eq) = content.find('=') else {
        return line.to_string();
    };
    let prefix = &content[..=eq];
    if unit.is_empty() {
        format!("{prefix} {number}{ending}")
    } else {
        format!("{prefix} {number} {unit}{ending}")
    }
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
    // Name order, so the same source always builds the same chain.
    let mut derived: Vec<(String, f64)> = solved
        .iter()
        .filter(|(name, _)| !env.contains_key(*name))
        .map(|(name, value)| (name.clone(), *value))
        .collect();
    derived.sort_by(|a, b| a.0.cmp(&b.0));
    for (k, v) in derived {
        seq += 1;
        let mut causes = Vec::new();
        let mut origin = "derived".to_string();
        if let Some(eq) = solver.relations.iter().find(|e| e.lhs == k) {
            origin = format!("= {}", eq.rhs);
            for input_var in eq.rhs.vars() {
                if let Some(fact) = ledger.latest(&input_var) {
                    causes.push(fact.seq);
                }
            }
        }
        causes.sort_unstable();
        causes.dedup();
        ledger.append(Fact {
            seq,
            tick: 0,
            kind: "derived".to_string(),
            name: k,
            value: v,
            causes,
            origin,
            metadata: None,
        });
    }
    Ok((ledger, solved.into_iter().collect()))
}

/// One sentence per solver step: the rule, why it ran forward or backward,
/// and the pinned numbers that produced the result.
fn explain_solution(
    trace: &[String],
    shown: &BTreeMap<String, f64>,
    units: &BTreeMap<String, String>,
) -> String {
    let mut sentences = Vec::new();
    for line in trace {
        if let Some(rest) = line.strip_prefix("[optimize] ") {
            sentences.push(format!("An objective was optimized: {rest}"));
            continue;
        }
        let Some((equation, name, kind)) = parse_trace_step(line) else {
            if !line.trim().is_empty() {
                sentences.push(line.clone());
            }
            continue;
        };
        let value = shown
            .get(&name)
            .copied()
            .map(format_number)
            .unwrap_or_else(|| "—".to_string());
        let unit = units
            .get(&name)
            .map(|u| format!(" {u}"))
            .unwrap_or_default();
        let name_cap = capitalize(&name);
        let mut others = Vec::new();
        let mut seen_names = std::collections::BTreeSet::new();
        for found in equation_names(&equation) {
            if found != name && seen_names.insert(found.clone()) {
                others.push(found);
            }
        }
        let causes: Vec<String> = others
            .iter()
            .filter_map(|n| {
                let v = shown.get(n).copied()?;
                let u = units
                    .get(n)
                    .map(|unit| format!(" {unit}"))
                    .unwrap_or_default();
                Some(format!("{} is {}{u}", capitalize(n), format_number(v)))
            })
            .collect();
        let sentence = if kind == "backward Newton" {
            let mut text =
                format!("{name_cap} was left blank, so `{equation}` was solved for {name}.");
            if !causes.is_empty() {
                text.push(' ');
                text.push_str(&join_phrases(&causes));
                text.push('.');
            }
            text.push(' ');
            text.push_str(&format!("{name_cap} comes out {value}{unit}."));
            text
        } else if causes.is_empty() {
            format!("`{equation}` computes {name} directly. {name_cap} comes out {value}{unit}.")
        } else {
            format!(
                "`{equation}` computes {name} from {}. {}. {name_cap} comes out {value}{unit}.",
                join_names(&others),
                join_phrases(&causes)
            )
        };
        sentences.push(sentence);
    }
    sentences.join("\n")
}

fn parse_trace_step(line: &str) -> Option<(String, String, String)> {
    let rest = line.strip_prefix('[')?;
    let (equation, after) = rest.split_once("] ")?;
    let (name, kind) = after.split_once(" <- ")?;
    if name.is_empty() || equation.is_empty() {
        return None;
    }
    if kind != "forward eval" && kind != "backward Newton" {
        return None;
    }
    Some((equation.to_string(), name.to_string(), kind.to_string()))
}

fn equation_names(equation: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut current = String::new();
    for ch in equation.chars() {
        let keep =
            ch.is_ascii_alphabetic() || ch == '_' || (!current.is_empty() && ch.is_ascii_digit());
        if keep {
            current.push(ch);
        } else if !current.is_empty() {
            names.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        names.push(current);
    }
    names
}

fn capitalize(name: &str) -> String {
    let mut chars = name.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

fn join_names(names: &[String]) -> String {
    join_phrases(names)
}

fn join_phrases(parts: &[String]) -> String {
    match parts {
        [] => String::new(),
        [only] => only.clone(),
        [a, b] => format!("{a} and {b}"),
        _ => {
            let (last, rest) = parts.split_last().unwrap();
            format!("{}, and {last}", rest.join(", "))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn check_clean_model_has_no_diagnostics() {
        let src = "signal x : scalar\nsignal y : scalar\nrelation R {\n y = 2 * x\n}\nsolve R with {\n x = 3\n}\n";
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
        let src = "signal force : N\nsignal mass : kg\nsignal accel : m/s^2\nrelation N {\n force = mass * accel\n}\nsolve N with {\n force = 4200\n accel = 5\n}\n";
        let solved = solve(src).expect("should solve");
        let mass = solved.values.get("mass").copied().expect("mass settled");
        assert!((mass - 840.0).abs() < 1e-9, "mass = {mass}");
        let why = &solved.explanation;
        assert!(
            why.contains("Mass was left blank"),
            "guide should say why mass was solved: {why}"
        );
        assert!(
            why.contains("force = mass * accel"),
            "guide should name the rule: {why}"
        );
        assert!(why.contains("Force is 4200"), "{why}");
        assert!(why.contains("Accel is 5"), "{why}");
        assert!(why.contains("Mass comes out 840"), "{why}");
    }

    #[test]
    fn explanation_says_a_calculation_was_computed_directly() {
        let solved = solve("x = 1 + 10").expect("named sum");
        let why = &solved.explanation;
        assert!(why.contains("computes x directly"), "{why}");
        assert!(why.contains("X comes out 11"), "{why}");
    }

    #[test]
    fn explanation_says_a_forward_product_used_the_pinned_numbers() {
        let src = "signal area : m^2\nsignal length : m\nsignal width : m\nrelation Field {\n area = length * width\n}\nsolve Field with {\n length = 8\n width = 5\n}\n";
        let solved = solve(src).expect("forward field");
        let why = &solved.explanation;
        assert!(why.contains("computes area from length and width"), "{why}");
        assert!(why.contains("Length is 8"), "{why}");
        assert!(why.contains("Width is 5"), "{why}");
        assert!(why.contains("Area comes out 40"), "{why}");
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

        // Unknown functions are now caught at check time (capitalized message
        // from plain_type_message), so solve refuses them there.
        let unknown_fn = solve("bogus(1)").expect_err("unknown function");
        assert!(
            unknown_fn
                .message
                .to_lowercase()
                .contains("unknown function"),
            "{}",
            unknown_fn.message
        );
    }

    #[test]
    fn log10_is_rejected_at_check_with_a_hint() {
        let diags = check("x = log10(100)");
        assert!(!diags.is_empty(), "log10 should fail check, not just solve");
        let msg = diags[0].message.to_lowercase();
        assert!(msg.contains("log10"), "{}", diags[0].message);
        assert!(
            msg.contains("ln(x) / ln(10)"),
            "should hint the ln form: {}",
            diags[0].message
        );
    }

    #[test]
    fn check_rejects_an_unknown_function() {
        let diags = check("y = arctan(2)");
        assert!(!diags.is_empty(), "an unknown function should fail check");
        assert!(
            diags[0].message.to_lowercase().contains("arctan"),
            "{}",
            diags[0].message
        );
    }

    #[test]
    fn sign_is_available_in_the_solver() {
        assert!((settled("x = sign(-5)")["x"] + 1.0).abs() < 1e-9);
        assert!((settled("x = sign(7)")["x"] - 1.0).abs() < 1e-9);
        assert!(settled("x = sign(0)")["x"].abs() < 1e-9);
    }

    #[test]
    fn a_numeric_bound_carries_and_converts_its_unit() {
        let src = "relation D {\n    dist = speed * time\n}\nsolve D with {\n    speed = 10\n    time = 5\n    dist <= 1 km\n}\n";
        let tokens = Lexer::new(src).tokenize().expect("lex");
        let mut module = Parser::new(tokens).parse_module("m").expect("parse");
        assert_eq!(
            module.solves[0].constraints[0].unit.as_deref(),
            Some("km"),
            "the bound keeps its unit"
        );
        normalize_bindings(&mut module).expect("normalize");
        match module.solves[0].constraints[0].rhs {
            crate::ast::Expr::Num(v) => {
                assert!((v - 1000.0).abs() < 1e-6, "1 km -> 1000 m, got {v}")
            }
            _ => panic!("bound rhs should be a number"),
        }
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
        assert!(!values.contains_key("answer"), "{values:?}");
        assert!(check("x = 1 + 10").is_empty());
        let reported = report("x = 1 + 10").expect("a named equation has a report");
        assert_eq!(reported.values["x"], 11.0);
        let tree = why("x = 1 + 10", "x", None).expect("why");
        assert!(tree.contains("x"), "{tree}");
    }

    #[test]
    fn solve_accepts_reversed_equation() {
        // `<expr> = <name>` settles the name just like `<name> = <expr>`.
        let values = settled("1 + 2 = x");
        assert_eq!(values.get("x").copied(), Some(3.0));
        assert!(!values.contains_key("answer"), "{values:?}");
        assert!(check("1 + 2 = x").is_empty());
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
        let src = "signal force : N\nsignal mass : kg\nsignal accel : m/s^2\nrelation N {\n force = mass * accel\n}\nsolve N with {\n mass = 2\n accel = 3\n}\n";
        let solved = solve(src).expect("should solve");
        assert!((solved.values["force"] - 6.0).abs() < 1e-9);
    }

    #[test]
    fn why_traces_a_derived_value_to_its_inputs() {
        let src = "signal force : N\nsignal mass : kg\nsignal accel : m/s^2\nrelation N {\n force = mass * accel\n}\nsolve N with {\n mass = 2\n accel = 3\n}\n";
        let tree = why(src, "force", None).expect("why");
        assert!(tree.contains("force"), "{tree}");
        assert!(tree.contains("mass") || tree.contains("accel"), "{tree}");
    }

    #[test]
    fn report_has_source_hash_values_derivation_and_head() {
        let src = "signal force : N\nsignal mass : kg\nsignal accel : m/s^2\nrelation N {\n force = mass * accel\n}\nsolve N with {\n mass = 2\n accel = 3\n}\n";
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
        let a = report("signal x : scalar\nsignal y : scalar\nrelation N {\n y = 2 * x\n}\nsolve N with {\n x = 3\n}\n").unwrap();
        let b = report("signal x : scalar\nsignal y : scalar\nrelation N {\n y = 2 * x\n}\nsolve N with {\n x = 4\n}\n").unwrap();
        assert_ne!(a.source_sha256, b.source_sha256);
        assert_ne!(a.ledger_head, b.ledger_head);
    }

    #[test]
    fn report_ledger_starts_with_the_source_hash() {
        let src = "signal x : scalar\nsignal y : scalar\nrelation N {\n y = 2 * x\n}\nsolve N with {\n x = 3\n}\n";
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
        let src = "signal x : scalar\nsignal y : scalar\nrelation N {\n // <script>alert(1)</script>\n y = 2 * x\n}\nsolve N with {\n x = 3\n}\n";
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
        let src = "signal force : N\nsignal mass : kg\nsignal acceleration : m/s^2\nrelation Newton {\n force = mass * acceleration\n}\nsolve Newton with {\n force = 4200\n acceleration = 5\n}\n";
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

    fn newton() -> &'static str {
        "signal force : N\nsignal mass : kg\nsignal acceleration : m/s^2\nrelation Newton {\n force = mass * acceleration\n}\nsolve Newton with {\n force = 4200\n acceleration = 5\n}\n"
    }

    #[test]
    fn verify_report_accepts_a_fresh_html_report() {
        let html = report_html(newton()).expect("report");
        let verdict = verify_report_html(&html).expect("verified");
        assert_eq!(verdict.source_sha256, sha256_hex(newton().as_bytes()));
        assert!(verdict.facts >= 3, "{}", verdict.facts);
        let again = verify_report_html(&html).expect("stable");
        assert_eq!(verdict.ledger_head, again.ledger_head);
    }

    #[test]
    fn verify_report_rejects_a_swapped_source() {
        let html = report_html(newton()).expect("report");
        let swapped = html.replacen(
            "force = mass * acceleration",
            "force = mass + acceleration",
            1,
        );
        let err = verify_report_html(&swapped).expect_err("swapped source");
        assert!(err.contains("does not match the ledger"), "{err}");
    }

    #[test]
    fn verify_report_rejects_a_displayed_number_that_was_edited() {
        let html = report_html(newton()).expect("report");
        let edited = html.replacen("<td>840</td>", "<td>1</td>", 1);
        let err = verify_report_html(&edited).expect_err("edited table");
        assert!(err.contains("settles at"), "{err}");
    }

    #[test]
    fn report_pdf_carries_the_source_hash() {
        let pdf = report_pdf(newton()).expect("pdf");
        assert!(pdf.starts_with(b"%PDF-1.4"));
        assert!(pdf.windows(5).any(|window| window == b"%%EOF"));
        let digest = sha256_hex(newton().as_bytes());
        assert!(
            pdf.windows(digest.len())
                .any(|window| window == digest.as_bytes()),
            "pdf should contain the source hash"
        );
    }

    #[test]
    fn sheet_round_trip_updates_inputs_and_leaves_the_blank_alone() {
        let rows = sheet_rows(newton()).expect("rows");
        assert!(
            rows.iter()
                .any(|row| row.name == "mass" && row.value == "840")
        );
        let csv = render_csv(&rows);
        let parsed = parse_csv(&csv).expect("csv");
        let mut edited = parsed;
        for row in &mut edited {
            if row.name == "force" {
                row.value = "8400".to_string();
            }
        }
        let imported = apply_sheet(newton(), &edited).expect("apply");
        let next = imported.source;
        assert!(
            imported.skipped.contains(&"mass".to_string()),
            "mass is solved, so its row is skipped: {:?}",
            imported.skipped
        );
        assert!(
            next.contains("force = 8400")
                || next.contains("force        = 8400")
                || next.contains("= 8400"),
            "{next}"
        );
        assert!(
            !next.contains("mass = 840"),
            "the blank stays blank:\n{next}"
        );
        let solved = solve(&next).expect("solve");
        assert!((solved.values["mass"] - 1680.0).abs() < 1e-6);
    }

    #[test]
    fn sheet_import_keeps_a_declared_unit() {
        let src = "signal speed : km/h\nsignal time : h\nrelation Travel {\n distance = speed * time\n}\nsolve Travel with {\n speed = 90 km/h\n time = 2.5 h\n}\n";
        let csv = "name,value,unit\nspeed,100,km/h\ntime,2.5,h\ndistance,1,m\n";
        let rows = parse_csv(csv).expect("csv");
        let imported = apply_sheet(src, &rows).expect("apply");
        let next = imported.source;
        assert!(
            imported.skipped.contains(&"distance".to_string()),
            "distance is solved, so its row is skipped: {:?}",
            imported.skipped
        );
        assert!(next.contains("speed = 100 km/h"), "{next}");
        assert!(
            next.contains("time = 2.5 h") || next.contains("time = 2.50 h"),
            "{next}"
        );
        let solve_block = next.split("solve Travel").nth(1).unwrap_or("");
        assert!(
            !solve_block.contains("distance"),
            "distance stays blank:\n{next}"
        );
        let solved = solve(&next).expect("solve");
        assert!((solved.shown["speed"] - 100.0).abs() < 1e-6);
    }

    #[test]
    fn sheet_import_refuses_when_every_row_is_solved() {
        let rows = parse_csv("name,value,unit\nmass,1,\n").expect("csv");
        let err = apply_sheet(newton(), &rows).expect_err("mass is not an input");
        assert_eq!(
            err.message,
            "mass is solved, not an input. Choose Solve for this on a pinned name to make that name the blank, then set it."
        );
    }

    #[test]
    fn sheet_import_names_every_solved_row_when_none_are_inputs() {
        let src = "signal x : scalar\nsignal y : scalar\nsignal z : scalar\nrelation Chain {\n y = 2 * x\n z = y + 1\n}\nsolve Chain with {\n x = 3\n}\n";
        let rows = parse_csv("name,value,unit\nz,9,\ny,1,\n").expect("csv");
        let err = apply_sheet(src, &rows).expect_err("both solved");
        assert_eq!(
            err.message,
            "z and y are solved, not inputs. Choose Solve for this on a pinned name to make that name the blank, then set them."
        );
    }

    #[test]
    fn sheet_import_ignores_a_non_numeric_solved_cell() {
        let rows =
            parse_csv("name,value,unit\nforce,8400,\nmass,not-a-number,\nmass,1,\n").expect("csv");
        let imported = apply_sheet(newton(), &rows).expect("apply");
        assert_eq!(imported.skipped, vec!["mass".to_string()]);
        assert!(
            imported.source.contains("force = 8400") || imported.source.contains("= 8400"),
            "{}",
            imported.source
        );
        assert!(!imported.source.contains("mass ="), "{}", imported.source);
    }

    #[test]
    fn sweep_traces_an_output_across_an_input() {
        let src = "signal force : N\nsignal mass : kg\nsignal acceleration : m/s^2\nrelation Newton {\n    force = mass * acceleration\n}\n\nsolve Newton with {\n    force = 4200\n    acceleration = 100\n}\n";
        let result = sweep(src, "force", 1000.0, 8000.0, 7).expect("sweep");
        assert_eq!(result.points.len(), 8, "steps + 1 points");
        let first = &result.points[0];
        let last = result.points.last().unwrap();
        assert!((first.x - 1000.0).abs() < 1e-6, "{}", first.x);
        assert!((last.x - 8000.0).abs() < 1e-6, "{}", last.x);
        // mass = force / acceleration = force / 100
        assert!(
            (first.values["mass"] - 10.0).abs() < 1e-6,
            "{:?}",
            first.values
        );
        assert!(
            (last.values["mass"] - 80.0).abs() < 1e-6,
            "{:?}",
            last.values
        );
        assert!(
            result.varies.contains(&"mass".to_string()),
            "{:?}",
            result.varies
        );
        assert!(result.knee.is_none(), "no objectives, no knee");
    }

    #[test]
    fn sweep_marks_a_knee_for_two_objectives() {
        // quality has diminishing returns while cost is linear, so the
        // trade-off curve bows and has an interior knee.
        let src = "signal effort : scalar\nrelation Plan {\n    quality = sqrt(effort)\n    cost = effort * 2\n}\n\nsolve Plan with {\n    effort = 25\n    maximize quality\n    minimize cost\n}\n";
        let result = sweep(src, "effort", 1.0, 100.0, 20).expect("sweep");
        let knee = result
            .knee
            .expect("two moving objectives should mark a knee");
        // Diminishing returns peak near effort 30.25 (sample 6 of 0..=20).
        // A straight-line tie would land on the midpoint, effort 50.5.
        let effort = result.points[knee].x;
        assert!(
            (effort - 30.25).abs() < 3.0,
            "knee effort {effort} at index {knee}, expected the bow near 30.25"
        );
    }

    #[test]
    fn sweep_knee_of_a_straight_tradeoff_is_the_midpoint() {
        let src = "signal effort : scalar\nrelation Plan {\n    quality = effort\n    cost = effort * 2\n}\n\nsolve Plan with {\n    effort = 25\n    maximize quality\n    minimize cost\n}\n";
        let result = sweep(src, "effort", 1.0, 100.0, 20).expect("sweep");
        assert_eq!(result.points.len(), 21);
        assert_eq!(result.knee, Some(result.points.len() / 2));
    }

    #[test]
    fn sweep_knee_follows_the_bow_not_the_largest_objective() {
        // noise is collinear with cost and a thousand times larger. Ranking
        // pairs by raw span would mark that straight pair and miss the bow.
        let src = "signal effort : scalar\nrelation Plan {\n    quality = sqrt(effort)\n    cost = effort * 2\n    noise = effort * 1000\n}\n\nsolve Plan with {\n    effort = 25\n    maximize quality\n    minimize cost\n    minimize noise\n}\n";
        let result = sweep(src, "effort", 1.0, 100.0, 20).expect("sweep");
        let knee = result.knee.expect("bowed pair should mark a knee");
        let effort = result.points[knee].x;
        assert!(
            (effort - 30.25).abs() < 3.0,
            "knee effort {effort} at index {knee}, the large linear objective hid the bow"
        );
    }

    #[test]
    fn sweep_knee_ignores_an_objective_that_is_not_on_the_curve() {
        let mut points = Vec::new();
        for i in 0..=20 {
            let effort = 1.0 + (i as f64 / 20.0) * 99.0;
            let mut values = BTreeMap::new();
            values.insert("quality".to_string(), effort.sqrt());
            values.insert("cost".to_string(), effort * 2.0);
            points.push(SweepPoint { x: effort, values });
        }
        let targets = vec![
            "quality".to_string(),
            "cost".to_string(),
            "ghost".to_string(),
        ];
        assert_eq!(sweep_knee(&targets, &points), Some(6));
    }

    #[test]
    fn sweep_knee_marks_a_spike_when_the_endpoints_meet() {
        let mut points = Vec::new();
        for i in 0..5 {
            let spike = if i == 1 { 5.0 } else { 0.0 };
            let mut values = BTreeMap::new();
            values.insert("a".to_string(), spike);
            values.insert("b".to_string(), spike / 5.0);
            points.push(SweepPoint {
                x: i as f64,
                values,
            });
        }
        assert_eq!(
            sweep_knee(&["a".to_string(), "b".to_string()], &points),
            Some(1),
            "the excursion is the first interior sample, not the midpoint"
        );
    }

    #[test]
    fn sweep_converts_the_shown_unit_into_the_binding_unit() {
        // Solve shows speed in km/h. The binding is written in m/s. A sweep
        // from 90 to 180 is 90 km/h, not 90 m/s.
        let src = "signal speed : km/h\nsignal time : h\nrelation Travel {\n distance = speed * time\n}\nsolve Travel with {\n speed = 25 m/s\n time = 2 h\n}\n";
        let result = sweep(src, "speed", 90.0, 180.0, 1).expect("sweep");
        assert_eq!(result.points.len(), 2);
        let first = &result.points[0];
        let last = &result.points[1];
        assert!(
            (first.x - 90.0).abs() < 1e-6,
            "chart x stays in km/h: {}",
            first.x
        );
        assert!(
            (last.x - 180.0).abs() < 1e-6,
            "chart x stays in km/h: {}",
            last.x
        );
        // 90 km/h = 25 m/s, 2 h = 7200 s, distance = 180000 m.
        // Writing 90 as m/s would have produced 648000 m.
        assert!(
            (first.values["distance"] - 180_000.0).abs() < 1e-3,
            "distance {:?}, expected 180000 m",
            first.values.get("distance")
        );
        assert!(
            (last.values["distance"] - 360_000.0).abs() < 1e-3,
            "distance {:?}, expected 360000 m",
            last.values.get("distance")
        );
    }

    #[test]
    fn sweep_rejects_a_variable_that_is_not_an_input() {
        let src = "signal force : N\nsignal mass : kg\nsignal acceleration : m/s^2\nrelation Newton {\n    force = mass * acceleration\n}\n\nsolve Newton with {\n    force = 4200\n    acceleration = 100\n}\n";
        // mass is the solved blank, not an input.
        let err = sweep(src, "mass", 1.0, 10.0, 5).expect_err("mass is not an input");
        assert!(err.message.contains("mass"), "{}", err.message);
    }
}
