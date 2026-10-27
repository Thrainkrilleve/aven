// tests/safety_case_and_timing.rs — Formal Safety Case & RT Timing Profiler Verification
// Tests DO-178C / ISO 26262 safety-case generation and high-resolution latency/jitter profiling.
// Verification constraint: Minimum of 3 randomized generated test cases using OS entropy + pid.

use avenc::ast::*;
use avenc::lexer::Lexer;
use avenc::parser::Parser;
use avenc::smt::*;
use std::fs;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn get_entropy() -> u64 {
    let t = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u64;
    let pid = std::process::id() as u64;
    t ^ (pid << 16)
}

fn parse_aven_string(source: &str, name: &str) -> AvenModule {
    let tokens = Lexer::new(source).tokenize().expect("lexer failed");
    Parser::new(tokens)
        .parse_module(name)
        .expect("parser failed")
}

#[test]
fn test_safety_case_golden_server_capacity() {
    let path = "examples/server_capacity.aven";
    let src = fs::read_to_string(path).expect("read file");
    let module = parse_aven_string(&src, path);

    let report = SafetyCaseGenerator::generate(
        &module,
        path,
        SafetyStandard::All,
        &["wire_rate_gbps <= 50.0".to_string()],
        None,
    )
    .expect("safety case generation should succeed");

    assert_eq!(report.model_name, path);
    assert_eq!(report.target_block, "ProductionProfile");
    assert_eq!(report.logic, "QF_NIRA");
    assert_eq!(report.satisfiability_status, SmtStatus::Sat);
    assert!(
        report.all_passed,
        "all safety invariants should be discharged"
    );
    assert_eq!(report.proofs.len(), 1);
    assert!(
        report.proofs[0].proven,
        "wire_rate_gbps <= 50.0 must be proven"
    );
    assert_eq!(report.proofs[0].status, SmtStatus::Unsat);

    // Verify Markdown generation
    let md = report.to_markdown();
    assert!(md.contains("# Formal Safety Case & Compliance Audit"));
    assert!(md.contains("DO-178C / DO-333"));
    assert!(md.contains("ISO 26262 Part 6 (ASIL-D"));
    assert!(md.contains("A5-1: HLR Accuracy & Consistency"));
    assert!(md.contains("Table 1 (1e): Formal Verification"));
    assert!(md.contains("VERDICT: ✓ FORMALLY DISCHARGED"));

    // Verify JSON generation
    let json = report.to_json();
    assert!(json.contains("\"all_passed\": true"));
    assert!(json.contains("\"logic\": \"QF_NIRA\""));
    assert!(json.contains("\"proven\": true"));
}

#[test]
fn test_safety_case_violation_detection_and_counterexample() {
    let path = "examples/server_capacity.aven";
    let src = fs::read_to_string(path).expect("read file");
    let module = parse_aven_string(&src, path);

    // Intentionally pass an invalid invariant: wire_rate_gbps is 26.624, so <= 20.0 must fail
    let report = SafetyCaseGenerator::generate(
        &module,
        path,
        SafetyStandard::All,
        &["wire_rate_gbps <= 20.0".to_string()],
        None,
    )
    .expect("safety case generation should complete");

    assert!(
        !report.all_passed,
        "violated invariant must fail safety case"
    );
    assert_eq!(report.proofs.len(), 1);
    assert!(!report.proofs[0].proven);
    assert_eq!(report.proofs[0].status, SmtStatus::Sat);
    assert!(report.proofs[0].counterexample.is_some());

    let md = report.to_markdown();
    assert!(md.contains("VERDICT: ✗ INVARIANTS VIOLATED"));
    assert!(md.contains("✗ VIOLATED"));
}

#[test]
fn test_randomized_1_aerodynamics_flight_envelope_safety_case() {
    let entropy = get_entropy();
    let altitude_m = 1000.0 + ((entropy % 5000) as f64);
    let speed_mps = 150.0 + (((entropy >> 8) % 100) as f64);
    let wing_area_m2 = 25.0 + (((entropy >> 16) % 15) as f64);
    let cl_nominal = 0.45;
    let cd_nominal = 0.035;

    // Invariant to prove: dynamic_pressure <= 60000.0 and lift >= 5000.0
    let model_src = format!(
        r#"
relation AeroForces {{
    air_density      = 1.225 - (0.0001 * altitude_m)
    dynamic_pressure = 0.5 * air_density * (speed_mps * speed_mps)
    lift_force       = dynamic_pressure * wing_area_m2 * lift_coeff
    drag_force       = dynamic_pressure * wing_area_m2 * drag_coeff
}}

forward FlightProfile {{
    air_density      = 1.225 - (0.0001 * altitude_m)
    dynamic_pressure = 0.5 * air_density * (speed_mps * speed_mps)
    lift_force       = dynamic_pressure * wing_area_m2 * lift_coeff
    drag_force       = dynamic_pressure * wing_area_m2 * drag_coeff
}}

solve AeroForces with {{
    altitude_m = {:.2}
    speed_mps = {:.2}
    wing_area_m2 = {:.2}
    lift_coeff = {:.3}
    drag_coeff = {:.3}
}}
"#,
        altitude_m, speed_mps, wing_area_m2, cl_nominal, cd_nominal
    );

    let temp_name = format!("random_aero_{}", entropy % 100000);
    let module = parse_aven_string(&model_src, &temp_name);

    let report = SafetyCaseGenerator::generate(
        &module,
        &temp_name,
        SafetyStandard::Do178c,
        &[
            "dynamic_pressure <= 60000.0".to_string(),
            "lift_force >= 5000.0".to_string(),
        ],
        None,
    )
    .expect("safety case generation should succeed for aero");

    assert_eq!(report.satisfiability_status, SmtStatus::Sat);
    assert_eq!(report.proofs.len(), 2);
    assert!(report.proofs[0].proven, "dynamic_pressure bound must hold");
    assert!(report.proofs[1].proven, "lift_force bound must hold");
    assert!(report.all_passed);

    let md = report.to_markdown();
    assert!(md.contains("A5-7: Algorithm Verification"));
    assert!(md.contains("✓ FORMALLY DISCHARGED"));
}

#[test]
fn test_randomized_2_mixed_integer_cluster_sizing_safety_case() {
    let entropy = get_entropy();
    let tasks = 50 + ((entropy % 150) as usize);
    let tasks_per_worker = 8;
    let ram_per_worker_gb = 4;
    let base_system_ram_gb = 16;

    let model_src = format!(
        r#"
relation ClusterSizing {{
    raw_workers       = total_tasks / tasks_per_worker
    allocated_workers = ceil(raw_workers)
    total_ram_gb      = (allocated_workers * ram_per_worker_gb) + base_system_ram_gb
}}

forward SizingProfile {{
    raw_workers       = total_tasks / tasks_per_worker
    allocated_workers = ceil(raw_workers)
    total_ram_gb      = (allocated_workers * ram_per_worker_gb) + base_system_ram_gb
}}

solve ClusterSizing with {{
    total_tasks        = {}
    tasks_per_worker   = {}
    ram_per_worker_gb  = {}
    base_system_ram_gb = {}
}}
"#,
        tasks, tasks_per_worker, ram_per_worker_gb, base_system_ram_gb
    );

    let temp_name = format!("random_cluster_{}", entropy % 100000);
    let module = parse_aven_string(&model_src, &temp_name);

    let report = SafetyCaseGenerator::generate(
        &module,
        &temp_name,
        SafetyStandard::Iso26262,
        &[
            "total_ram_gb <= 200.0".to_string(),
            "allocated_workers >= 1.0".to_string(),
        ],
        None,
    )
    .expect("mixed-integer cluster safety case should succeed");

    assert_eq!(report.logic, "QF_NIRA");
    assert_eq!(report.satisfiability_status, SmtStatus::Sat);
    assert!(report.all_passed);

    let json = report.to_json();
    assert!(json.contains("\"all_passed\": true"));
    assert!(json.contains("\"allocated_workers\""));
}

#[test]
fn test_randomized_3_realtime_timing_and_jitter_profiler() {
    let entropy = get_entropy();
    let a = 1.0 + ((entropy % 10) as f64) * 0.1;
    let b = 2.0 + (((entropy >> 4) % 10) as f64) * 0.1;
    let c = 3.0 + (((entropy >> 8) % 10) as f64) * 0.1;

    let model_src = format!(
        r#"
forward ControlFilter {{
    filtered_pos = ({:.2} * raw_pos) + ({:.2} * prev_pos)
    filtered_vel = filtered_pos / {:.2}
    control_out  = filtered_vel * 1.5
}}

solve ControlFilter with {{
    raw_pos  = 12.5
    prev_pos = 12.0
}}
"#,
        a, b, c
    );

    let temp_path = std::env::temp_dir().join(format!("aven_timing_{}.aven", entropy % 100000));
    fs::write(&temp_path, &model_src).expect("write temp aven");

    // Run timing profiler via avenc bench --latency-profile --json
    let output = Command::new(env!("CARGO_BIN_EXE_avenc"))
        .args([
            "bench",
            temp_path.to_str().unwrap(),
            "--latency-profile",
            "--iterations",
            "1000",
            "--target-hz",
            "100",
            "--json",
        ])
        .output()
        .expect("invoke avenc bench");

    let _ = fs::remove_file(&temp_path);

    assert!(
        output.status.success(),
        "bench --latency-profile should succeed"
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("\"min_ns\":"), "json must include min_ns");
    assert!(stdout.contains("\"p50_ns\":"), "json must include p50_ns");
    assert!(stdout.contains("\"p99_ns\":"), "json must include p99_ns");
    assert!(stdout.contains("\"max_ns\":"), "json must include max_ns");
    assert!(stdout.contains("\"mean_ns\":"), "json must include mean_ns");
    assert!(
        stdout.contains("\"jitter_range_ns\":"),
        "json must include jitter_range_ns"
    );
    assert!(
        stdout.contains("\"target_deadline_ns\": 10000000"),
        "100 Hz = 10,000,000 ns"
    );
}

#[test]
fn test_safety_case_cli_file_emission() {
    let entropy = get_entropy();
    let temp_out = std::env::temp_dir().join(format!("aven_sc_{}.md", entropy % 100000));

    let output = Command::new(env!("CARGO_BIN_EXE_avenc"))
        .args([
            "safety-case",
            "examples/server_capacity.aven",
            "-o",
            temp_out.to_str().unwrap(),
            "--invariant",
            "wire_rate_gbps <= 50.0",
        ])
        .output()
        .expect("invoke avenc safety-case");

    assert!(output.status.success(), "safety-case CLI must succeed");
    assert!(temp_out.exists(), "output markdown report must be written");

    let content = fs::read_to_string(&temp_out).expect("read generated report");
    let _ = fs::remove_file(&temp_out);

    assert!(content.contains("# Formal Safety Case & Compliance Audit"));
    assert!(content.contains("wire_rate_gbps <= 50.0"));
    assert!(content.contains("✓ FORMALLY DISCHARGED"));
}
