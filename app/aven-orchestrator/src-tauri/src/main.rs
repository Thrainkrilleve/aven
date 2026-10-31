#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

// Aven IDE — Tauri backend.
//
// A thin wrapper over `avenc::api` (the one shared surface used by the CLI, the
// WASM build, and this app). No checking or solving logic lives here, so there
// stays one source of truth for the math. The DTOs below are serde mirrors of
// `avenc::api`'s plain structs — defined here, not in `avenc`, so the compiler
// crate keeps its zero-dependency guarantee.

use avenc::api;
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
relation Newton {
    force = mass * acceleration
}

solve Newton with {
    force        = 4200
    acceleration = 5
}
"#;

const SERVER_CAPACITY: &str = r#"// Little's Law sizing with queueing headroom.
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
        check_model, display_name, ensure_aven_extension, ensure_html_extension, read_model,
        solve_for_variable, solve_model, write_model, FLIGHT, NEWTON, SERVER_CAPACITY,
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
            solve_for_variable
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
