#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

// Aven IDE — Tauri backend.
//
// A thin wrapper over `avenc::api` (the one shared surface used by the CLI, the
// WASM build, and this app). No checking or solving logic lives here, so there
// stays one source of truth for the math. The DTOs below are serde mirrors of
// `avenc::api`'s plain structs — defined here, not in `avenc`, so the compiler
// crate keeps its zero-dependency guarantee.

use avenc::api;
use calamine::Reader;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

#[derive(Serialize)]
struct PosDto {
    line: usize,
    col: usize,
}

#[derive(Serialize)]
struct DiagnosticDto {
    severity: String,
    message: String,
    start: PosDto,
    end: PosDto,
}

impl From<&api::Diagnostic> for DiagnosticDto {
    fn from(d: &api::Diagnostic) -> Self {
        let severity = match d.severity {
            api::Severity::Error => "error",
            api::Severity::Warning => "warning",
        };
        DiagnosticDto {
            severity: severity.to_string(),
            message: d.message.clone(),
            start: PosDto {
                line: d.start.line,
                col: d.start.col,
            },
            end: PosDto {
                line: d.end.line,
                col: d.end.col,
            },
        }
    }
}

#[derive(Serialize)]
struct SolvedDto {
    values: Vec<(String, f64)>,
    shown: Vec<(String, f64)>,
    units: Vec<(String, String)>,
    solving: Vec<String>,
    bound: Vec<String>,
    trace: Vec<String>,
    explanation: String,
}

#[derive(Serialize)]
struct ReportDto {
    source: String,
    source_sha256: String,
    values: Vec<(String, f64)>,
    derivations: Vec<(String, String)>,
    ledger_head: String,
    ledger_jsonl: String,
}

#[derive(Serialize)]
struct Template {
    name: String,
    source: String,
}

/// Check a model for lex, parse, and unit errors. Empty when clean.
#[tauri::command]
fn check_model(source: String) -> Vec<DiagnosticDto> {
    api::check(&source)
        .iter()
        .map(DiagnosticDto::from)
        .collect()
}

/// Solve a model in whatever direction its `solve` block leaves open.
#[tauri::command]
fn solve_model(source: String) -> Result<SolvedDto, DiagnosticDto> {
    match api::solve(&source) {
        Ok(s) => Ok(SolvedDto {
            values: s.values.into_iter().collect(),
            shown: s.shown.into_iter().collect(),
            units: s.units.into_iter().collect(),
            solving: s.solving,
            bound: s.bound,
            trace: s.trace,
            explanation: s.explanation,
        }),
        Err(d) => Err(DiagnosticDto::from(&d)),
    }
}

/// Causal derivation tree for one variable.
#[tauri::command]
fn why_model(
    source: String,
    variable: String,
    depth: Option<usize>,
) -> Result<String, DiagnosticDto> {
    api::why(&source, &variable, depth).map_err(|d| DiagnosticDto::from(&d))
}

#[derive(Serialize)]
struct SweepPointDto {
    x: f64,
    values: Vec<(String, f64)>,
}

#[derive(Serialize)]
struct SweepResultDto {
    var: String,
    points: Vec<SweepPointDto>,
    units: Vec<(String, String)>,
    varies: Vec<String>,
    knee: Option<usize>,
}

/// Sweep one solve-block input across a range, solving at each step, and return
/// the points for plotting a sensitivity curve.
#[tauri::command]
fn sweep_model(
    source: String,
    var: String,
    min: f64,
    max: f64,
    steps: usize,
) -> Result<SweepResultDto, DiagnosticDto> {
    match api::sweep(&source, &var, min, max, steps) {
        Ok(s) => Ok(SweepResultDto {
            var: s.var,
            points: s
                .points
                .into_iter()
                .map(|p| SweepPointDto {
                    x: p.x,
                    values: p.values.into_iter().collect(),
                })
                .collect(),
            units: s.units.into_iter().collect(),
            varies: s.varies,
            knee: s.knee,
        }),
        Err(d) => Err(DiagnosticDto::from(&d)),
    }
}

/// The data a shareable, re-runnable report renders from.
#[tauri::command]
fn report_model(source: String) -> Result<ReportDto, DiagnosticDto> {
    match api::report(&source) {
        Ok(r) => Ok(ReportDto {
            source: r.source,
            source_sha256: r.source_sha256,
            values: r.values.into_iter().collect(),
            derivations: r.derivations.into_iter().collect(),
            ledger_head: r.ledger_head,
            ledger_jsonl: r.ledger_jsonl,
        }),
        Err(d) => Err(DiagnosticDto::from(&d)),
    }
}

/// The model file this window last opened or saved. The webview never
/// chooses a path; only the native dialog does.
struct CurrentFile(Mutex<Option<PathBuf>>);

#[derive(Serialize)]
struct OpenedModel {
    name: String,
    source: String,
}

fn display_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Untitled".to_string())
}

fn ensure_extension(path: PathBuf, ext: &str) -> PathBuf {
    match path.extension().and_then(|e| e.to_str()) {
        Some(current) if current.eq_ignore_ascii_case(ext) => path,
        _ => {
            let mut named = path.into_os_string();
            named.push(".");
            named.push(ext);
            PathBuf::from(named)
        }
    }
}

fn ensure_aven_extension(path: PathBuf) -> PathBuf {
    ensure_extension(path, "aven")
}

fn ensure_html_extension(path: PathBuf) -> PathBuf {
    ensure_extension(path, "html")
}

fn ensure_pdf_extension(path: PathBuf) -> PathBuf {
    ensure_extension(path, "pdf")
}

fn ensure_sheet_extension(path: PathBuf) -> PathBuf {
    match path.extension().and_then(|ext| ext.to_str()) {
        Some(ext) if ext.eq_ignore_ascii_case("csv") || ext.eq_ignore_ascii_case("xlsx") => path,
        _ => ensure_extension(path, "csv"),
    }
}

struct Chooser {
    save: bool,
    title: &'static str,
    // Only the GTK dialog takes a custom confirm-button label; native
    // Windows/macOS dialogs supply their own, so this is read on Linux only.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    accept: &'static str,
    filter_name: &'static str,
    extensions: &'static [&'static str],
    suggested: Option<String>,
}

fn model_chooser(save: bool, suggested: Option<String>) -> Chooser {
    Chooser {
        save,
        title: if save { "Save model" } else { "Open model" },
        accept: if save { "Save" } else { "Open" },
        filter_name: "Aven model",
        extensions: &["aven"],
        suggested,
    }
}

fn report_chooser() -> Chooser {
    Chooser {
        save: true,
        title: "Export report",
        accept: "Export",
        filter_name: "HTML report",
        extensions: &["html"],
        suggested: Some("report.html".to_string()),
    }
}

fn pdf_chooser() -> Chooser {
    Chooser {
        save: true,
        title: "Export PDF",
        accept: "Export",
        filter_name: "PDF report",
        extensions: &["pdf"],
        suggested: Some("report.pdf".to_string()),
    }
}

fn sheet_chooser(save: bool) -> Chooser {
    Chooser {
        save,
        title: if save { "Export sheet" } else { "Import sheet" },
        accept: if save { "Export" } else { "Import" },
        filter_name: "Spreadsheet",
        extensions: &["csv", "xlsx"],
        suggested: if save {
            Some("values.csv".to_string())
        } else {
            None
        },
    }
}

fn html_open_chooser() -> Chooser {
    Chooser {
        save: false,
        title: "Open report",
        accept: "Open",
        filter_name: "HTML report",
        extensions: &["html"],
        suggested: None,
    }
}

fn read_model(path: &Path) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|err| format!("Could not read {}: {err}", path.display()))
}

fn write_model(path: &Path, source: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .map_err(|err| format!("Could not create {}: {err}", parent.display()))?;
        }
    }
    std::fs::write(path, source).map_err(|err| format!("Could not write {}: {err}", path.display()))
}

fn ask_path(app: &tauri::AppHandle, chooser: Chooser) -> Result<Option<PathBuf>, String> {
    let (tx, rx) = std::sync::mpsc::channel();
    app.run_on_main_thread(move || {
        let _ = tx.send(pick_dialog(chooser));
    })
    .map_err(|err| format!("Could not open the file dialog: {err}"))?;
    rx.recv()
        .map_err(|err| format!("The file dialog closed without a choice: {err}"))?
}

#[cfg(target_os = "linux")]
fn pick_dialog(chooser: Chooser) -> Result<Option<PathBuf>, String> {
    use gtk::prelude::*;
    let dialog = gtk::FileChooserNative::new(
        Some(chooser.title),
        None::<&gtk::Window>,
        if chooser.save {
            gtk::FileChooserAction::Save
        } else {
            gtk::FileChooserAction::Open
        },
        Some(chooser.accept),
        Some("Cancel"),
    );
    let filter = gtk::FileFilter::new();
    filter.set_name(Some(chooser.filter_name));
    for ext in chooser.extensions {
        filter.add_pattern(&format!("*.{ext}"));
    }
    dialog.add_filter(filter);
    if chooser.save {
        dialog.set_current_name(chooser.suggested.as_deref().unwrap_or("model.aven"));
        dialog.set_do_overwrite_confirmation(true);
    }
    dialog.set_modal(true);
    let accepted = dialog.run() == gtk::ResponseType::Accept;
    let path = if accepted { dialog.filename() } else { None };
    dialog.hide();
    Ok(path)
}

#[cfg(any(windows, target_os = "macos"))]
fn pick_dialog(chooser: Chooser) -> Result<Option<PathBuf>, String> {
    let mut dialog = rfd::FileDialog::new()
        .set_title(chooser.title)
        .add_filter(chooser.filter_name, chooser.extensions);
    if chooser.save {
        dialog = dialog.set_file_name(
            chooser
                .suggested
                .unwrap_or_else(|| "model.aven".to_string()),
        );
        Ok(dialog.save_file())
    } else {
        Ok(dialog.pick_file())
    }
}

#[cfg(not(any(target_os = "linux", windows, target_os = "macos")))]
fn pick_dialog(chooser: Chooser) -> Result<Option<PathBuf>, String> {
    let _ = chooser;
    Err("Opening a model needs a desktop file dialog.".to_string())
}

/// Open a `.aven` file. The source comes back unchanged. Cancel returns nothing.
#[tauri::command]
fn open_model(
    app: tauri::AppHandle,
    state: tauri::State<CurrentFile>,
) -> Result<Option<OpenedModel>, String> {
    let Some(path) = ask_path(&app, model_chooser(false, None))? else {
        return Ok(None);
    };
    let source = read_model(&path)?;
    let name = display_name(&path);
    *state
        .0
        .lock()
        .map_err(|_| "The editor lost track of the open file.".to_string())? = Some(path);
    Ok(Some(OpenedModel { name, source }))
}

/// Write the editor source to the open file, or ask for a path the first time.
/// Cancel returns nothing. The file is the source text and nothing else.
#[tauri::command]
fn save_model(
    app: tauri::AppHandle,
    state: tauri::State<CurrentFile>,
    source: String,
) -> Result<Option<String>, String> {
    let mut slot = state
        .0
        .lock()
        .map_err(|_| "The editor lost track of the open file.".to_string())?;
    let path = if let Some(path) = slot.clone() {
        path
    } else {
        let Some(picked) = ask_path(&app, model_chooser(true, Some("model.aven".to_string())))?
        else {
            return Ok(None);
        };
        ensure_aven_extension(picked)
    };
    write_model(&path, &source)?;
    let name = display_name(&path);
    *slot = Some(path);
    Ok(Some(name))
}

#[derive(Serialize)]
struct ExportedReport {
    name: String,
    source_sha256: String,
    ledger_head: String,
}

/// Write a self-contained HTML report. This does not change the open model path.
#[tauri::command]
fn export_report(app: tauri::AppHandle, source: String) -> Result<Option<ExportedReport>, String> {
    let report = api::report(&source).map_err(|d| d.message)?;
    let html = api::render_report_html(&report);
    let Some(picked) = ask_path(&app, report_chooser())? else {
        return Ok(None);
    };
    let path = ensure_html_extension(picked);
    write_model(&path, &html)?;
    Ok(Some(ExportedReport {
        name: display_name(&path),
        source_sha256: report.source_sha256,
        ledger_head: report.ledger_head,
    }))
}

/// Blank `target` in the solve block and pin the variable that was blank.
#[tauri::command]
fn solve_for_variable(source: String, target: String) -> Result<String, DiagnosticDto> {
    api::solve_for(&source, &target).map_err(|d| DiagnosticDto::from(&d))
}

#[derive(Serialize)]
struct VerifiedReport {
    name: String,
    source: String,
    source_sha256: String,
    ledger_head: String,
    facts: usize,
}

/// Open an HTML report, re-solve it, and check the ledger. Cancel returns nothing.
#[tauri::command]
fn open_report(app: tauri::AppHandle) -> Result<Option<VerifiedReport>, String> {
    let Some(path) = ask_path(&app, html_open_chooser())? else {
        return Ok(None);
    };
    let html = read_model(&path)?;
    let verdict = api::verify_report_html(&html)?;
    let source = api::embedded_source(&html)?;
    Ok(Some(VerifiedReport {
        name: display_name(&path),
        source,
        source_sha256: verdict.source_sha256,
        ledger_head: verdict.ledger_head,
        facts: verdict.facts,
    }))
}

/// Write the printable report. This does not change the open model path.
#[tauri::command]
fn export_pdf(app: tauri::AppHandle, source: String) -> Result<Option<String>, String> {
    let bytes = api::report_pdf(&source).map_err(|diag| diag.message)?;
    let Some(picked) = ask_path(&app, pdf_chooser())? else {
        return Ok(None);
    };
    let path = ensure_pdf_extension(picked);
    write_bytes(&path, &bytes)?;
    Ok(Some(display_name(&path)))
}

/// Write settled values as CSV or xlsx. The extension chooses the format.
#[tauri::command]
fn export_sheet(app: tauri::AppHandle, source: String) -> Result<Option<String>, String> {
    let rows = api::sheet_rows(&source).map_err(|diag| diag.message)?;
    let Some(picked) = ask_path(&app, sheet_chooser(true))? else {
        return Ok(None);
    };
    let path = ensure_sheet_extension(picked);
    if is_xlsx(&path) {
        write_bytes(&path, &xlsx_bytes(&rows)?)?;
    } else {
        write_model(&path, &api::render_csv(&rows))?;
    }
    Ok(Some(display_name(&path)))
}

#[derive(Serialize)]
struct SheetImportDto {
    source: String,
    /// Names whose rows were skipped because they're solved, not inputs.
    skipped: Vec<String>,
}

/// Fill solve-block inputs from a CSV or xlsx file. `range` limits an xlsx sheet, like `A1:C20`.
#[tauri::command]
fn import_sheet(
    app: tauri::AppHandle,
    source: String,
    range: Option<String>,
) -> Result<Option<SheetImportDto>, String> {
    let Some(path) = ask_path(&app, sheet_chooser(false))? else {
        return Ok(None);
    };
    let rows = if is_xlsx(&path) {
        let bytes = std::fs::read(&path)
            .map_err(|err| format!("Could not read {}: {err}", path.display()))?;
        rows_from_xlsx(&bytes, range.as_deref())?
    } else {
        api::parse_csv(&read_model(&path)?)?
    };
    let imported = api::apply_sheet(&source, &rows).map_err(|diag| diag.message)?;
    Ok(Some(SheetImportDto {
        source: imported.source,
        skipped: imported.skipped,
    }))
}

fn write_bytes(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .map_err(|err| format!("Could not create {}: {err}", parent.display()))?;
        }
    }
    std::fs::write(path, bytes).map_err(|err| format!("Could not write {}: {err}", path.display()))
}

fn is_xlsx(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("xlsx"))
}

fn xlsx_bytes(rows: &[api::SheetRow]) -> Result<Vec<u8>, String> {
    let mut workbook = rust_xlsxwriter::Workbook::new();
    let sheet = workbook.add_worksheet();
    sheet
        .write_string(0, 0, "name")
        .map_err(|err| format!("Could not write the sheet: {err}"))?;
    sheet
        .write_string(0, 1, "value")
        .map_err(|err| format!("Could not write the sheet: {err}"))?;
    sheet
        .write_string(0, 2, "unit")
        .map_err(|err| format!("Could not write the sheet: {err}"))?;
    for (index, row) in rows.iter().enumerate() {
        let at = (index + 1) as u32;
        sheet
            .write_string(at, 0, &row.name)
            .map_err(|err| format!("Could not write the sheet: {err}"))?;
        sheet
            .write_string(at, 1, &row.value)
            .map_err(|err| format!("Could not write the sheet: {err}"))?;
        sheet
            .write_string(at, 2, &row.unit)
            .map_err(|err| format!("Could not write the sheet: {err}"))?;
    }
    workbook
        .save_to_buffer()
        .map_err(|err| format!("Could not write the sheet: {err}"))
}

fn rows_from_xlsx(bytes: &[u8], range: Option<&str>) -> Result<Vec<api::SheetRow>, String> {
    let mut book = calamine::Xlsx::new(std::io::Cursor::new(bytes))
        .map_err(|err| format!("Could not read this workbook: {err}"))?;
    let sheet = book
        .worksheet_range_at(0)
        .ok_or_else(|| "The workbook has no sheet.".to_string())?
        .map_err(|err| format!("Could not read the first sheet: {err}"))?;
    let (start, end) = match range.map(str::trim).filter(|text| !text.is_empty()) {
        None => {
            let start = sheet
                .start()
                .ok_or_else(|| "The first sheet is empty.".to_string())?;
            let end = sheet
                .end()
                .ok_or_else(|| "The first sheet is empty.".to_string())?;
            (start, end)
        }
        Some(text) => parse_a1_range(text)?,
    };
    let mut csv = String::new();
    for row in start.0..=end.0 {
        for col in start.1..=end.1 {
            if col != start.1 {
                csv.push(',');
            }
            let cell = sheet
                .get_value((row, col))
                .map(cell_text)
                .unwrap_or_default();
            csv.push_str(&quote_csv(&cell));
        }
        csv.push('\n');
    }
    api::parse_csv(&csv)
}

fn cell_text(data: &calamine::Data) -> String {
    match data {
        calamine::Data::Empty => String::new(),
        calamine::Data::Int(value) => value.to_string(),
        calamine::Data::Float(value) => number_text(*value),
        calamine::Data::String(value) => value.trim().to_string(),
        calamine::Data::Bool(value) => value.to_string(),
        other => other.to_string(),
    }
}

fn number_text(value: f64) -> String {
    if !value.is_finite() {
        return String::new();
    }
    let rounded = value.round();
    if (value - rounded).abs() < 1e-9 && rounded.abs() < 1e15 {
        return format!("{}", rounded as i64);
    }
    let text = format!("{value:.12}");
    let trimmed = text.trim_end_matches('0').trim_end_matches('.');
    if trimmed.is_empty() || trimmed == "-" {
        "0".to_string()
    } else {
        trimmed.to_string()
    }
}

fn quote_csv(text: &str) -> String {
    if text.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", text.replace('"', "\"\""))
    } else {
        text.to_string()
    }
}

fn parse_a1_range(text: &str) -> Result<((u32, u32), (u32, u32)), String> {
    let Some((left, right)) = text.split_once(':') else {
        return Err("A range looks like A1:C20.".to_string());
    };
    let start = parse_a1(left)?;
    let end = parse_a1(right)?;
    if start.0 > end.0 || start.1 > end.1 {
        return Err("The range is backwards. Put the top-left cell first.".to_string());
    }
    Ok((start, end))
}

fn parse_a1(cell: &str) -> Result<(u32, u32), String> {
    let cell = cell.trim();
    let bytes = cell.as_bytes();
    let mut index = 0;
    let mut col: u32 = 0;
    while index < bytes.len() && bytes[index].is_ascii_alphabetic() {
        col = col
            .saturating_mul(26)
            .saturating_add((bytes[index].to_ascii_uppercase() - b'A' + 1) as u32);
        index += 1;
    }
    if col == 0 || index == bytes.len() {
        return Err(format!("{cell} is not a cell. Use a range like A1:C20."));
    }
    let row: u32 = cell[index..]
        .parse()
        .map_err(|_| format!("{cell} is not a cell. Use a range like A1:C20."))?;
    if row == 0 {
        return Err(format!("{cell} is not a cell. Use a range like A1:C20."));
    }
    Ok((row - 1, col - 1))
}

/// Forget the open path so the next save asks for a file.
#[tauri::command]
fn new_model(state: tauri::State<CurrentFile>) -> Result<(), String> {
    *state
        .0
        .lock()
        .map_err(|_| "The editor lost track of the open file.".to_string())? = None;
    Ok(())
}

/// Checked starter models, so no one begins from a blank editor.
#[tauri::command]
fn list_templates() -> Vec<Template> {
    vec![
        Template {
            name: "Newton's second law (solve mass backward)".to_string(),
            source: NEWTON.to_string(),
        },
        Template {
            name: "Server capacity (Little's Law)".to_string(),
            source: SERVER_CAPACITY.to_string(),
        },
        Template {
            name: "Flight dynamics (solve airspeed from lift)".to_string(),
            source: FLIGHT.to_string(),
        },
    ]
}

const NEWTON: &str = r#"// Equations are invariants: leave a variable out of the
// solve block and Aven solves for it. Here mass is found backward.
signal force        : N
signal mass         : kg
signal acceleration : m/s^2

relation Newton {
    force = mass * acceleration
}

solve Newton with {
    force        = 4200
    acceleration = 5
}
"#;

const SERVER_CAPACITY: &str = r#"// Little's Law sizing with queueing headroom.
// Rates, counts and sizes are abstract numbers, so inputs are : scalar.
signal reqs_per_second        : scalar
signal avg_latency_ms         : scalar
signal avg_payload_kb         : scalar
signal ram_per_worker         : scalar
signal socket_buffer_mb       : scalar
signal os_kernel_ram_mb       : scalar
signal target_utilization     : scalar
signal framing_overhead_ratio : scalar

relation WorkerScaling {
    raw_workers       = (reqs_per_second * avg_latency_ms) / 1000
    allocated_workers = ceil(raw_workers / target_utilization)
    total_ram_needed  = (allocated_workers * (ram_per_worker + socket_buffer_mb)) + os_kernel_ram_mb
    payload_gbps      = (reqs_per_second * avg_payload_kb * 8) / 1000000
    wire_rate_gbps    = payload_gbps * framing_overhead_ratio
}

solve WorkerScaling with {
    reqs_per_second        = 50000
    avg_latency_ms         = 20
    avg_payload_kb         = 64
    ram_per_worker         = 16
    socket_buffer_mb       = 4
    os_kernel_ram_mb       = 4096
    target_utilization     = 0.70
    framing_overhead_ratio = 1.04
}
"#;

const FLIGHT: &str = r#"// Declared units; solves airspeed backward from a target lift.
signal altitude     : m
signal surface_temp : degC
signal airspeed     : m/s
signal air_density  : kg/m^3
signal wing_area    : m^2
signal lift_coeff   : scalar
signal lapse_rate   : degC/m

relation FlightDynamics {
    ambient_temp     = surface_temp - lapse_rate * altitude
    dynamic_pressure = 0.5 * air_density * airspeed^2
    lift             = dynamic_pressure * wing_area * lift_coeff
}

solve FlightDynamics with {
    surface_temp = 15 degC
    altitude     = 3000 m
    air_density  = 0.909 kg/m^3
    lapse_rate   = 0.0065
    wing_area    = 32 m^2
    lift_coeff   = 0.55
    lift         = 90000 N
}
"#;

#[cfg(test)]
mod ide_tests {
    use super::{
        check_model, display_name, ensure_aven_extension, ensure_html_extension,
        ensure_sheet_extension, parse_a1_range, read_model, rows_from_xlsx, solve_for_variable,
        solve_model, write_model, xlsx_bytes, FLIGHT, NEWTON, SERVER_CAPACITY,
    };

    #[test]
    fn shipped_templates_check_clean_and_solve() {
        for src in [NEWTON, SERVER_CAPACITY, FLIGHT] {
            let diags = check_model(src.to_string());
            assert!(
                diags.is_empty(),
                "{src}\n{}",
                diags
                    .iter()
                    .map(|d| d.message.clone())
                    .collect::<Vec<_>>()
                    .join("\n")
            );
            match solve_model(src.to_string()) {
                Ok(_) => {}
                Err(err) => panic!("template failed to solve: {}\n{src}", err.message),
            }
        }
    }

    #[test]
    fn solve_model_settles_named_equations() {
        let src = "y = x * 2\nx = 1 + 10";
        match solve_model(src.to_string()) {
            Ok(solved) => {
                let value = |name: &str| {
                    solved
                        .values
                        .iter()
                        .find(|(n, _)| n == name)
                        .map(|(_, v)| *v)
                };
                assert_eq!(value("x"), Some(11.0), "{:?}", solved.values);
                assert_eq!(value("y"), Some(22.0), "{:?}", solved.values);
                assert!(
                    solved.explanation.contains("computes x directly"),
                    "{}",
                    solved.explanation
                );
                assert!(
                    solved.explanation.contains("Y comes out 22")
                        || solved.explanation.contains("computes y from x"),
                    "{}",
                    solved.explanation
                );
            }
            Err(err) => panic!("{src} refused: {}", err.message),
        }
    }

    #[test]
    fn solve_model_evaluates_basic_arithmetic() {
        for (src, expect) in [("1+2", 3.0), ("32 ^ 3", 32768.0)] {
            match solve_model(src.to_string()) {
                Ok(solved) => {
                    let answer = solved
                        .values
                        .iter()
                        .find(|(name, _)| name == "answer")
                        .map(|(_, value)| *value);
                    assert_eq!(answer, Some(expect), "{src} -> {:?}", solved.values);
                }
                Err(err) => panic!("{src} refused: {}", err.message),
            }
        }
    }

    #[test]
    fn solve_model_refuses_adding_a_speed_to_a_time() {
        let src = "signal v : km/h\nsignal t : s\nrelation R {\n bad = v + t\n}\nsolve R with {\n v = 10 km/h\n t = 2 s\n}\n";
        let err = match solve_model(src.to_string()) {
            Err(err) => err,
            Ok(_) => panic!("the IDE must not call this valid"),
        };
        assert_eq!(err.message, "Can't add m·s^-1 to s (line 4).");
        assert_eq!(check_model(src.to_string())[0].message, err.message);
    }

    #[test]
    fn model_file_round_trip_is_the_exact_source() {
        let dir = std::env::temp_dir().join(format!("aven-save-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("model.aven");
        let source = "x = 1 + 10\ny = x * 2\n";
        write_model(&path, source).unwrap();
        assert_eq!(read_model(&path).unwrap(), source);
        assert_eq!(display_name(&path), "model.aven");
        assert_eq!(
            ensure_aven_extension(dir.join("notes")),
            dir.join("notes.aven")
        );
        let kept = dir.join("Already.AVEN");
        assert_eq!(ensure_aven_extension(kept.clone()), kept);
        let missing = read_model(&dir.join("missing.aven"));
        assert!(missing.is_err(), "{missing:?}");
        assert_eq!(
            ensure_html_extension(dir.join("report")),
            dir.join("report.html")
        );
        let html = avenc::api::report_html(NEWTON).expect("newton report");
        let report_path = dir.join("report.html");
        write_model(&report_path, &html).unwrap();
        let saved = read_model(&report_path).unwrap();
        assert!(saved.contains("force = mass * acceleration"));
        assert!(saved.contains("&quot;kind&quot;: &quot;source&quot;"));
        assert!(!saved.contains("<script>"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn solve_for_variable_rewrites_newton() {
        let rewritten = match solve_for_variable(NEWTON.to_string(), "force".to_string()) {
            Ok(source) => source,
            Err(err) => panic!("{}", err.message),
        };
        assert!(rewritten.contains("mass = 840"), "{rewritten}");
        assert!(!rewritten.contains("force        = 4200"), "{rewritten}");
        match solve_model(rewritten) {
            Ok(solved) => {
                let force = solved
                    .values
                    .iter()
                    .find(|(name, _)| name == "force")
                    .map(|(_, value)| *value);
                assert!((force.unwrap() - 4200.0).abs() < 1e-6);
                assert_eq!(solved.solving, vec!["force".to_string()]);
            }
            Err(err) => panic!("{}", err.message),
        }
    }

    #[test]
    fn xlsx_round_trip_respects_a_cell_range() {
        let rows = avenc::api::sheet_rows(NEWTON).expect("rows");
        let bytes = xlsx_bytes(&rows).expect("xlsx");
        let all = rows_from_xlsx(&bytes, None).expect("read");
        assert!(all
            .iter()
            .any(|row| row.name == "force" && row.value == "4200"));
        let ranged = rows_from_xlsx(&bytes, Some("A1:C3")).expect("range");
        assert!(ranged.iter().any(|row| row.name == "force"));
        assert!(
            ranged.len() < all.len(),
            "a short range drops later rows: {ranged:?} vs {all:?}"
        );
        assert!(parse_a1_range("A1:C20").is_ok());
        assert!(parse_a1_range("C20:A1").is_err());
        let csv_path = std::env::temp_dir().join(format!("aven-sheet-{}", std::process::id()));
        assert_eq!(
            ensure_sheet_extension(csv_path.join("values")),
            csv_path.join("values.csv")
        );
        let kept = csv_path.join("values.xlsx");
        assert_eq!(ensure_sheet_extension(kept.clone()), kept);
    }
}

fn main() {
    tauri::Builder::default()
        .manage(CurrentFile(Mutex::new(None)))
        .invoke_handler(tauri::generate_handler![
            check_model,
            solve_model,
            why_model,
            report_model,
            list_templates,
            open_model,
            save_model,
            new_model,
            export_report,
            solve_for_variable,
            open_report,
            export_pdf,
            export_sheet,
            import_sheet,
            sweep_model
        ])
        .setup(|_app| {
            #[cfg(debug_assertions)]
            {
                use tauri::Manager;
                if let Some(window) = _app.get_webview_window("main") {
                    window.open_devtools();
                }
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
