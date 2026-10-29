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
    trace: Vec<String>,
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
            trace: s.trace,
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

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            check_model,
            solve_model,
            why_model,
            report_model,
            list_templates
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
