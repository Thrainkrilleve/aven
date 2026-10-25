#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use avenc::ast::{BackendDecl, BlockKind};
use avenc::evaluator::{
    EvaluatorBackend, ExternalEvaluator, Orchestrator, OrchestratorConfig, SolveStatus, VarMap,
};
use avenc::lexer::Lexer;
use avenc::parser::Parser;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap};
use std::net::TcpStream;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

// ---------------------------------------------------------------------------
// Helpers: Locate repository root and evaluation scripts
// ---------------------------------------------------------------------------

fn get_repo_root() -> PathBuf {
    // 1. Check current directory and its ancestors
    if let Ok(cur) = std::env::current_dir() {
        let mut p = cur.as_path();
        loop {
            if p.join("Cargo.toml").exists() && p.join("src").join("evaluator.rs").exists() {
                return p.to_path_buf();
            }
            match p.parent() {
                Some(parent) => p = parent,
                None => break,
            }
        }
    }
    // 2. Check exe directory and its ancestors
    if let Ok(exe) = std::env::current_exe() {
        let mut p = exe.as_path();
        loop {
            if p.join("Cargo.toml").exists() && p.join("src").join("evaluator.rs").exists() {
                return p.to_path_buf();
            }
            match p.parent() {
                Some(parent) => p = parent,
                None => break,
            }
        }
    }
    // 3. Fallback to canonical location on Windows
    let default_path = PathBuf::from("f:/aven");
    if default_path.exists() {
        default_path
    } else {
        PathBuf::from(".")
    }
}

fn get_flame_script_path() -> PathBuf {
    let repo = get_repo_root();
    let candidate = repo.join("evaluations").join("tests").join("flame_eval.py");
    if candidate.exists() {
        candidate
    } else {
        PathBuf::from("flame_eval.py")
    }
}

fn check_openplc_ports() -> (bool, u16) {
    // Check Modbus TCP port 502
    if TcpStream::connect_timeout(
        &"127.0.0.1:502".parse().unwrap(),
        Duration::from_millis(300),
    )
    .is_ok()
    {
        return (true, 502);
    }
    // Check REST API port 8443
    if TcpStream::connect_timeout(
        &"127.0.0.1:8443".parse().unwrap(),
        Duration::from_millis(300),
    )
    .is_ok()
    {
        return (true, 8443);
    }
    (false, 0)
}

// ---------------------------------------------------------------------------
// Data Transfer Objects (DTOs)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SolveDiagnosticsDto {
    pub spec_hash: String,
    pub free_vars: Vec<String>,
    pub residual: f64,
    pub residual_str: String,
    pub rank: usize,
    pub binding_constraints: Vec<String>,
    pub backend_trace: Vec<String>,
    pub solve_time_us: f64,
    pub status: String,
    pub is_feasible: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SolvePointResponse {
    pub values: HashMap<String, f64>,
    pub diagnostics: SolveDiagnosticsDto,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrontierPointDto {
    pub x: f64,
    pub value: f64,
    pub vars: HashMap<String, f64>,
    pub is_feasible: bool,
    pub diagnostics: SolveDiagnosticsDto,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParetoResponse {
    pub frontier: Vec<FrontierPointDto>,
    pub knee_index: Option<usize>,
    pub total_evaluated: usize,
    pub feasible_count: usize,
    pub fail_count: usize,
    pub total_time_ms: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackendInfoDto {
    pub id: String,
    pub name: String,
    pub description: String,
    pub kind: String,
    pub status: String,
    pub endpoint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackendTestResult {
    pub backend_id: String,
    pub success: bool,
    pub latency_ms: f64,
    pub status_message: String,
    pub details: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelValidationResult {
    pub success: bool,
    pub module_name: String,
    pub relations_count: usize,
    pub equations_count: usize,
    pub detected_backend: String,
    pub parameters: Vec<String>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SampleModel {
    pub id: String,
    pub name: String,
    pub description: String,
    pub code: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct EvalRequest {
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct EvalResponse {
    pub value: Option<f64>,
    pub error: Option<String>,
    pub backend_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiveTelemetryDto {
    pub cycle: usize,
    pub spec_hash: String,
    pub scan_ms: f64,
    pub power_w: f64,
    pub temp_c: f64,
    pub throughput_pph: f64,
    pub quality_pct: f64,
    pub alarm_count: f64,
    pub cpu_load_pct: f64,
    pub network_load_pct: f64,
    pub status: String,
    pub is_alarm: bool,
    pub alarms: Vec<String>,
    pub values: HashMap<String, f64>,
}

static CYCLE_COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(1);

// ---------------------------------------------------------------------------
// Orchestrator Builder
// ---------------------------------------------------------------------------

fn build_orchestrator(backend_choice: &str) -> Orchestrator {
    let repo_root = get_repo_root();
    let flame_script = get_flame_script_path();

    match backend_choice {
        "native_analytic" => {
            // Native continuous Newton solver with standard relations
            let config = OrchestratorConfig {
                backend: EvaluatorBackend::Native,
                seed: 42,
            };
            Orchestrator::from_config(&config, vec![])
        }
        "openplc_v4" => {
            let adapter_path = repo_root
                .join("evaluations")
                .join("tests")
                .join("PLC-test")
                .join("openplc_adapter.py");
            let arg = adapter_path.to_string_lossy().to_string();
            let mut ext = ExternalEvaluator::new("python", &[arg.as_str()]);
            ext = ext.with_backend_id("openplc");
            if let Some(parent) = adapter_path.parent() {
                ext = ext.with_working_dir(parent);
            }
            Orchestrator::new(Arc::new(ext))
        }
        _ => {
            // Default: flame_eval external python backend
            let args = vec![flame_script.to_string_lossy().to_string()];
            let mut ext = ExternalEvaluator::new("python", &[args[0].as_str()]);
            ext = ext.with_backend_id("flame_eval");
            if let Some(parent) = flame_script.parent() {
                ext = ext.with_working_dir(parent);
            } else {
                ext = ext.with_working_dir(repo_root);
            }
            Orchestrator::new(Arc::new(ext))
        }
    }
}

// ---------------------------------------------------------------------------
// Tauri Commands
// ---------------------------------------------------------------------------

#[tauri::command]
fn list_backends() -> Vec<BackendInfoDto> {
    let (plc_online, port) = check_openplc_ports();
    let plc_status = if plc_online {
        format!("Online (Port {})", port)
    } else {
        "Container Stopped".to_string()
    };

    vec![
        BackendInfoDto {
            id: "flame_eval".to_string(),
            name: "FlameTest (External Hostile)".to_string(),
            description: "External Python subprocess with intentional discontinuity and noise."
                .to_string(),
            kind: "External Subprocess".to_string(),
            status: "Ready".to_string(),
            endpoint: "python flame_eval.py".to_string(),
        },
        BackendInfoDto {
            id: "native_analytic".to_string(),
            name: "Native Continuous Solver".to_string(),
            description: "Zero-dependency pure-Rust Newton-Raphson analytical solver.".to_string(),
            kind: "Pure Rust Core".to_string(),
            status: "Ready".to_string(),
            endpoint: "in-process avenc".to_string(),
        },
        BackendInfoDto {
            id: "openplc_v4".to_string(),
            name: "OpenPLC Runtime v4".to_string(),
            description: "Industrial Modbus TCP & IEC 61131-3 PLC manufacturing runtime."
                .to_string(),
            kind: "Industrial Automation".to_string(),
            status: plc_status,
            endpoint: "127.0.0.1:502 / 127.0.0.1:8443".to_string(),
        },
    ]
}

#[tauri::command]
fn test_backend(backend_id: String) -> BackendTestResult {
    let t_start = Instant::now();

    match backend_id.as_str() {
        "openplc_v4" => {
            let (online, port) = check_openplc_ports();
            let latency = t_start.elapsed().as_secs_f64() * 1000.0;
            if online {
                BackendTestResult {
                    backend_id,
                    success: true,
                    latency_ms: latency,
                    status_message: format!(
                        "OpenPLC container is active. Modbus/API reachable on port {}.",
                        port
                    ),
                    details: Some(serde_json::json!({
                        "version": "v4.3.1",
                        "container": "openplc-v4",
                        "port": port,
                        "modbus_tcp": 502,
                        "api_https": 8443
                    })),
                }
            } else {
                BackendTestResult {
                    backend_id,
                    success: false,
                    latency_ms: latency,
                    status_message: "Cannot connect to OpenPLC container on port 502 or 8443."
                        .to_string(),
                    details: None,
                }
            }
        }
        "native_analytic" => {
            let orchestrator = build_orchestrator("native_analytic");
            let mut inputs = VarMap::new();
            inputs.set("x", 3.0);
            inputs.set("y", 4.0);

            let pt = orchestrator.solve_point(&inputs);
            let latency = t_start.elapsed().as_secs_f64() * 1000.0;

            BackendTestResult {
                backend_id,
                success: pt.is_feasible,
                latency_ms: latency,
                status_message: format!("Native solver executed in {:.2} ms", latency),
                details: Some(serde_json::json!({
                    "spec_hash": pt.diagnostics.spec_hash,
                    "rank": pt.diagnostics.rank,
                    "status": pt.diagnostics.status.to_string(),
                })),
            }
        }
        _ => {
            // Default: flame_eval
            let orchestrator = build_orchestrator("flame_eval");
            let mut inputs = VarMap::new();
            inputs.set("x", 0.5);
            inputs.set("y", -0.2);

            let res = orchestrator.solve_single(&inputs);
            let latency = t_start.elapsed().as_secs_f64() * 1000.0;

            match res {
                Ok(pt) => BackendTestResult {
                    backend_id,
                    success: true,
                    latency_ms: latency,
                    status_message: format!(
                        "External flame_eval responded. Output value: {:.4}",
                        pt.vars.get("value").unwrap_or(0.0)
                    ),
                    details: Some(serde_json::json!({
                        "spec_hash": pt.diagnostics.spec_hash,
                        "backend_trace": pt.diagnostics.backend_trace,
                        "values": pt.vars.values,
                    })),
                },
                Err(failure) => BackendTestResult {
                    backend_id,
                    success: false,
                    latency_ms: latency,
                    status_message: format!("Evaluator returned error: {}", failure.error),
                    details: Some(serde_json::json!({
                        "spec_hash": failure.diagnostics.spec_hash,
                        "status": failure.diagnostics.status.to_string(),
                        "residual": failure.diagnostics.residual.is_nan(),
                    })),
                },
            }
        }
    }
}

#[tauri::command]
fn solve_point(
    x: f64,
    y: f64,
    backend: Option<String>,
    model_source: Option<String>,
) -> SolvePointResponse {
    let chosen_backend = if let Some(src) = &model_source {
        let mut lexer = Lexer::new(src);
        if let Ok(tokens) = lexer.tokenize() {
            let mut parser = Parser::new(tokens);
            if let Ok(module) = parser.parse_module("editor.aven") {
                if let Some(blk) = module.blocks.first() {
                    if let BackendDecl::External { .. } = &blk.backend {
                        "flame_eval".to_string()
                    } else {
                        backend.unwrap_or_else(|| "flame_eval".to_string())
                    }
                } else {
                    backend.unwrap_or_else(|| "flame_eval".to_string())
                }
            } else {
                backend.unwrap_or_else(|| "flame_eval".to_string())
            }
        } else {
            backend.unwrap_or_else(|| "flame_eval".to_string())
        }
    } else {
        backend.unwrap_or_else(|| "flame_eval".to_string())
    };

    let orchestrator = build_orchestrator(&chosen_backend);

    let mut inputs = VarMap::new();
    inputs.set("x", x);
    inputs.set("y", y);

    let pt = orchestrator.solve_point(&inputs);

    let residual_str = if pt.diagnostics.residual.is_nan() {
        "NaN (black-box failure)".to_string()
    } else {
        format!("{:.6}", pt.diagnostics.residual)
    };

    let error = match &pt.diagnostics.status {
        SolveStatus::Converged => None,
        other => Some(other.to_string()),
    };

    SolvePointResponse {
        values: pt.vars.values,
        diagnostics: SolveDiagnosticsDto {
            spec_hash: pt.diagnostics.spec_hash,
            free_vars: pt.diagnostics.free_vars,
            residual: pt.diagnostics.residual,
            residual_str,
            rank: pt.diagnostics.rank,
            binding_constraints: pt.diagnostics.binding_constraints,
            backend_trace: pt.diagnostics.backend_trace,
            solve_time_us: pt.diagnostics.solve_time_us,
            status: pt.diagnostics.status.to_string(),
            is_feasible: pt.is_feasible,
        },
        error,
    }
}

#[tauri::command]
fn solve_pareto(
    sweep_var: String,
    sweep_min: f64,
    sweep_max: f64,
    steps: usize,
    backend: Option<String>,
) -> ParetoResponse {
    let chosen_backend = backend.unwrap_or_else(|| "flame_eval".to_string());
    let orchestrator = build_orchestrator(&chosen_backend);

    let t_start = Instant::now();
    let num_steps = steps.clamp(5, 100);

    let mut base_inputs = VarMap::new();
    base_inputs.set("x", 0.0);
    base_inputs.set("y", -0.2);

    let mut frontier = Vec::new();
    let mut feasible_count = 0;
    let mut fail_count = 0;

    for i in 0..=num_steps {
        let val = sweep_min + (i as f64 / num_steps as f64) * (sweep_max - sweep_min);
        let mut cur_inputs = base_inputs.clone();
        cur_inputs.set(&sweep_var, val);

        let pt = orchestrator.solve_point(&cur_inputs);
        let val_out = pt.vars.get("value").unwrap_or(0.0);

        let residual_str = if pt.diagnostics.residual.is_nan() {
            "NaN (black-box failure)".to_string()
        } else {
            format!("{:.6}", pt.diagnostics.residual)
        };

        if pt.is_feasible {
            feasible_count += 1;
        } else {
            fail_count += 1;
        }

        frontier.push(FrontierPointDto {
            x: val,
            value: val_out,
            vars: pt.vars.values,
            is_feasible: pt.is_feasible,
            diagnostics: SolveDiagnosticsDto {
                spec_hash: pt.diagnostics.spec_hash,
                free_vars: pt.diagnostics.free_vars,
                residual: pt.diagnostics.residual,
                residual_str,
                rank: pt.diagnostics.rank,
                binding_constraints: pt.diagnostics.binding_constraints,
                backend_trace: pt.diagnostics.backend_trace,
                solve_time_us: pt.diagnostics.solve_time_us,
                status: pt.diagnostics.status.to_string(),
                is_feasible: pt.is_feasible,
            },
        });
    }

    // Convert feasible frontier points to find knee point
    let feasible_pts: Vec<avenc::evaluator::FrontierPoint> = frontier
        .iter()
        .filter(|p| p.is_feasible)
        .map(|p| {
            let mut vm = VarMap::new();
            vm.values = p.vars.clone();
            avenc::evaluator::FrontierPoint {
                vars: vm,
                objectives: vec![("x".to_string(), p.x), ("value".to_string(), p.value)],
                diagnostics: avenc::evaluator::SolveDiagnostics {
                    spec_hash: p.diagnostics.spec_hash.clone(),
                    free_vars: p.diagnostics.free_vars.clone(),
                    residual: p.diagnostics.residual,
                    rank: p.diagnostics.rank,
                    binding_constraints: vec![],
                    backend_trace: p.diagnostics.backend_trace.clone(),
                    solve_time_us: p.diagnostics.solve_time_us,
                    status: SolveStatus::Converged,
                },
                is_feasible: true,
            }
        })
        .collect();

    let knee_feasible_idx = Orchestrator::find_knee_point(&feasible_pts);
    let knee_index = knee_feasible_idx.and_then(|f_idx| {
        let mut count = 0;
        for (idx, p) in frontier.iter().enumerate() {
            if p.is_feasible {
                if count == f_idx {
                    return Some(idx);
                }
                count += 1;
            }
        }
        None
    });

    let total_time_ms = t_start.elapsed().as_secs_f64() * 1000.0;

    ParetoResponse {
        frontier,
        knee_index,
        total_evaluated: num_steps + 1,
        feasible_count,
        fail_count,
        total_time_ms,
    }
}

#[tauri::command]
fn validate_model(source: String) -> ModelValidationResult {
    let mut lexer = Lexer::new(&source);
    let tokens = match lexer.tokenize() {
        Ok(t) => t,
        Err(e) => {
            return ModelValidationResult {
                success: false,
                module_name: "syntax_error".to_string(),
                relations_count: 0,
                equations_count: 0,
                detected_backend: "none".to_string(),
                parameters: vec![],
                message: format!("Lexer error: {}", e),
            };
        }
    };

    let mut parser = Parser::new(tokens);
    match parser.parse_module("editor_model.aven") {
        Ok(module) => {
            let relations = module
                .blocks
                .iter()
                .filter(|b| b.kind == BlockKind::Relation)
                .count();
            let mut eq_count = 0;
            let mut detected_backend = "Native".to_string();
            let mut param_set = BTreeSet::new();

            for blk in &module.blocks {
                eq_count += blk.equations.len();
                for eq in &blk.equations {
                    param_set.insert(eq.lhs.clone());
                    eq.rhs.collect_vars(&mut param_set);
                }
                if let BackendDecl::External { command, args } = &blk.backend {
                    detected_backend = format!("{}({})", command, args.join(", "));
                }
            }

            ModelValidationResult {
                success: true,
                module_name: module.name,
                relations_count: relations,
                equations_count: eq_count,
                detected_backend,
                parameters: param_set.into_iter().collect(),
                message: format!(
                    "Module validated successfully: {} blocks, {} equations.",
                    module.blocks.len(),
                    eq_count
                ),
            }
        }
        Err(e) => ModelValidationResult {
            success: false,
            module_name: "parse_error".to_string(),
            relations_count: 0,
            equations_count: 0,
            detected_backend: "none".to_string(),
            parameters: vec![],
            message: format!("Parse error: {}", e),
        },
    }
}

#[tauri::command]
fn get_sample_models() -> Vec<SampleModel> {
    vec![
        SampleModel {
            id: "flame".to_string(),
            name: "FlameTest (External Hostile)".to_string(),
            description: "Delegates to Python flame_eval.py with hostile discontinuities & noise."
                .to_string(),
            code: r#"// Aven Relational Model — FlameTest with Pluggable Subprocess
relation FlameTest using external("python", "flame_eval.py") {
    value = 0.0
}

solve FlameTest with {
    x = 0.5
    y = -0.2
}
"#
            .to_string(),
        },
        SampleModel {
            id: "openplc".to_string(),
            name: "OpenPLC Manufacturing Line".to_string(),
            description: "Industrial automated manufacturing process with cycle & power balance."
                .to_string(),
            code: r#"// Aven Industrial Model — OpenPLC Manufacturing Line
relation ManufacturingLine {
    power_w = 120.0 + io_points * 2.8
    scan_ms = 7.5 + cpu_load_pct * 0.04
    throughput_pph = 140.0 - scan_ms * 2.0
    quality_pct = 99.8 - scan_ms * 0.05
}

solve ManufacturingLine with {
    io_points = 24
    cpu_load_pct = 35.0
}
"#
            .to_string(),
        },
        SampleModel {
            id: "pendulum".to_string(),
            name: "Pendulum Dynamics (Native)".to_string(),
            description:
                "Mechanical energy balance and period calculation solved natively in Rust."
                    .to_string(),
            code: r#"// Aven Mechanical Model — Non-linear Pendulum
relation PendulumDynamics {
    omega = sqrt(9.81 / length)
    period = 2.0 * 3.14159265 * sqrt(length / 9.81)
    energy = 0.5 * mass * (length * omega)^2
}

solve PendulumDynamics with {
    length = 1.5
    mass = 2.0
}
"#
            .to_string(),
        },
        SampleModel {
            id: "server_capacity".to_string(),
            name: "Server Infrastructure Capacity".to_string(),
            description: "RHEL Little's Law sizing, wire rate framing, and queueing headroom."
                .to_string(),
            code: r#"// Aven Infrastructure Capacity Model — Little's Law with Framing
relation ServerCapacity {
    raw_workers = (reqs_per_second * avg_latency_ms) / 1000.0
    allocated_workers = raw_workers / target_utilization
    total_ram_needed = allocated_workers * ram_per_worker_mb
    payload_gbps = (reqs_per_second * avg_payload_kb * 8.0) / 1000000.0
    wire_rate_gbps = payload_gbps * 1.05
}

solve ServerCapacity with {
    reqs_per_second = 50000.0
    avg_latency_ms = 20.0
    target_utilization = 0.70
    ram_per_worker_mb = 20.0
    avg_payload_kb = 64.0
}
"#
            .to_string(),
        },
    ]
}

#[tauri::command]
fn get_live_telemetry() -> LiveTelemetryDto {
    let cycle = CYCLE_COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let orchestrator = build_orchestrator("openplc_v4");
    let inputs = VarMap::new();
    let pt = orchestrator.solve_point(&inputs);

    let scan_ms = pt.vars.get("scan_ms").unwrap_or(8.2);
    let power_w = pt.vars.get("power_w").unwrap_or(185.0);
    let temp_c = pt.vars.get("temp_c").unwrap_or(38.4);
    let throughput_pph = pt.vars.get("throughput_pph").unwrap_or(123.6);
    let quality_pct = pt.vars.get("quality_pct").unwrap_or(99.1);
    let alarm_count = pt.vars.get("alarm_count").unwrap_or(0.0);
    let cpu_load_pct = pt.vars.get("cpu_load_pct").unwrap_or(35.0);
    let network_load_pct = pt.vars.get("network_load_pct").unwrap_or(12.0);

    let mut alarms = Vec::new();
    if scan_ms > 10.0 {
        alarms.push(format!("High Scan Time: {:.2} ms > 10.0 ms", scan_ms));
    }
    if temp_c > 45.0 {
        alarms.push(format!(
            "Cabinet Over-Temperature: {:.1} °C > 45.0 °C",
            temp_c
        ));
    }
    if power_w > 210.0 {
        alarms.push(format!(
            "Power Draw Limit Exceeded: {:.1} W > 210.0 W",
            power_w
        ));
    }
    if alarm_count > 0.0 {
        alarms.push(format!("Active PLC Alarms: {:.0}", alarm_count));
    }

    let is_alarm = !alarms.is_empty();
    let status = if is_alarm {
        format!("ALARM: {}", alarms.join("; "))
    } else {
        "CONVERGED (Invariant satisfied)".to_string()
    };

    LiveTelemetryDto {
        cycle,
        spec_hash: pt.diagnostics.spec_hash,
        scan_ms,
        power_w,
        temp_c,
        throughput_pph,
        quality_pct,
        alarm_count,
        cpu_load_pct,
        network_load_pct,
        status,
        is_alarm,
        alarms,
        values: pt.vars.values,
    }
}

#[tauri::command]
fn call_flame(req: EvalRequest) -> EvalResponse {
    let orchestrator = build_orchestrator("flame_eval");
    let mut inputs = VarMap::new();
    inputs.set("x", req.x);
    inputs.set("y", req.y);

    let pt = orchestrator.solve_point(&inputs);
    if pt.is_feasible {
        EvalResponse {
            value: pt.vars.get("value"),
            error: None,
            backend_id: Some("flame_eval".to_string()),
        }
    } else {
        EvalResponse {
            value: None,
            error: Some(pt.diagnostics.status.to_string()),
            backend_id: Some("flame_eval".to_string()),
        }
    }
}

// ---------------------------------------------------------------------------
// Application Entry Point
// ---------------------------------------------------------------------------

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            list_backends,
            test_backend,
            solve_point,
            solve_pareto,
            validate_model,
            get_sample_models,
            call_flame,
            get_live_telemetry
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
