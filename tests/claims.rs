// tests/claims.rs — Executable checks for the behaviour promised in README.md and wiki/.
//
// Each test names the document and claim it checks. Tests drive the real `avenc`
// binary so they exercise exactly what a user runs. A failing test here means the
// code does not do what the documentation says; fix the code, not the test.

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

static FILE_COUNTER: AtomicUsize = AtomicUsize::new(0);

fn write_model(src: &str) -> PathBuf {
    let n = FILE_COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("aven_claims_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("model_{}.aven", n));
    std::fs::write(&path, src).unwrap();
    path
}

struct Run {
    code: i32,
    stdout: String,
    stderr: String,
}

fn avenc(args: &[&str]) -> Run {
    let out = Command::new(env!("CARGO_BIN_EXE_avenc"))
        .args(args)
        .output()
        .expect("failed to launch avenc");
    Run {
        code: out.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    }
}

fn run_model(src: &str, extra: &[&str]) -> Run {
    let path = write_model(src);
    let p = path.to_string_lossy().into_owned();
    let mut args = vec!["run", p.as_str()];
    args.extend_from_slice(extra);
    avenc(&args)
}

/// Parses the "Settled Environment:" section printed by `avenc run`.
fn settled(run: &Run) -> HashMap<String, f64> {
    let mut env = HashMap::new();
    let mut in_env = false;
    for line in run.stdout.lines() {
        if line.starts_with("Settled Environment:") {
            in_env = true;
            continue;
        }
        if in_env {
            let t = line.trim();
            if t.is_empty() || t.starts_with("Fact Ledger") {
                break;
            }
            if let Some((k, v)) = t.split_once('=') {
                let v = v.trim().trim_end_matches("(int)").trim();
                if let Ok(x) = v.parse::<f64>() {
                    env.insert(k.trim().to_string(), x);
                }
            }
        }
    }
    env
}

fn assert_close(env: &HashMap<String, f64>, var: &str, expected: f64, tol: f64) {
    let got = env
        .get(var)
        .unwrap_or_else(|| panic!("'{}' missing from settled environment: {:?}", var, env));
    assert!(
        (got - expected).abs() <= tol,
        "{} = {} (expected {} ± {})",
        var,
        got,
        expected,
        tol
    );
}

// ---------------------------------------------------------------------------
// README: "Write F = m·a once, and solve for force, mass, or acceleration"
// ---------------------------------------------------------------------------

#[test]
fn readme_omnidirectional_f_equals_ma_solves_each_unknown() {
    let cases = [
        ("mass = 2\n accel = 3", "force", 6.0),
        ("force = 6\n accel = 3", "mass", 2.0),
        ("force = 6\n mass = 2", "accel", 3.0),
    ];
    for (bindings, unknown, expected) in cases {
        let src = format!(
            "relation Newton {{\n force = mass * accel\n}}\nsolve Newton with {{\n {}\n}}\n",
            bindings
        );
        let run = run_model(&src, &[]);
        assert_eq!(run.code, 0, "stderr: {}", run.stderr);
        assert_close(&settled(&run), unknown, expected, 1e-4);
    }
}

// ---------------------------------------------------------------------------
// wiki/Language-Specification.md §1–2
// ---------------------------------------------------------------------------

#[test]
fn spec_block_comments_are_supported() {
    let src = "/* header\n spans lines */\nrelation R {\n y = 2 * x /* inline */\n}\nsolve R with {\n x = 4\n}\n";
    let run = run_model(src, &[]);
    assert_eq!(run.code, 0, "stderr: {}", run.stderr);
    assert_close(&settled(&run), "y", 8.0, 1e-9);
}

#[test]
fn spec_integer_keyword_declares_integer_variable() {
    let src = "relation R {\n integer n\n cap = n * 13.5\n cap >= 100\n}\nsolve R with {\n minimize(n)\n}\n";
    let run = run_model(src, &[]);
    assert_eq!(run.code, 0, "stderr: {}", run.stderr);
    assert_close(&settled(&run), "n", 8.0, 1e-9);
}

#[test]
fn spec_builtin_functions_evaluate() {
    // sqrt, abs, ceil, floor, sin, cos, tan, ln, exp
    let src = "relation F {
 a = sqrt(x)
 b = abs(0 - x)
 c = ceil(x + 0.5)
 d = floor(x + 0.5)
 s = sin(x)
 co = cos(x)
 t = tan(x)
 l = ln(x)
 ex = exp(x)
}
solve F with {
 x = 2
}
";
    let run = run_model(src, &[]);
    assert_eq!(run.code, 0, "stderr: {}", run.stderr);
    let env = settled(&run);
    let x: f64 = 2.0;
    assert_close(&env, "a", x.sqrt(), 1e-4);
    assert_close(&env, "b", 2.0, 1e-9);
    assert_close(&env, "c", 3.0, 1e-9);
    assert_close(&env, "d", 2.0, 1e-9);
    assert_close(&env, "s", x.sin(), 1e-4);
    assert_close(&env, "co", x.cos(), 1e-4);
    assert_close(&env, "t", x.tan(), 1e-4);
    assert_close(&env, "l", x.ln(), 1e-4);
    assert_close(&env, "ex", x.exp(), 1e-4);
}

#[test]
fn spec_builtin_constants() {
    let src = "relation C {\n a = pi * k\n b = e * k\n c = tau * k\n d = PI * k\n}\nsolve C with {\n k = 1\n}\n";
    let run = run_model(src, &[]);
    assert_eq!(run.code, 0, "stderr: {}", run.stderr);
    let env = settled(&run);
    assert_close(&env, "a", std::f64::consts::PI, 1e-4);
    assert_close(&env, "b", std::f64::consts::E, 1e-4);
    assert_close(&env, "c", std::f64::consts::TAU, 1e-4);
    assert_close(&env, "d", std::f64::consts::PI, 1e-4);
}

#[test]
fn spec_operator_precedence_and_associativity() {
    // ^ binds tighter than unary minus and is right-associative.
    let src = "relation P {
 neg_sq = 0 - 2 ^ 2 + k
 unary = -2 ^ 2 + k
 right = 2 ^ 3 ^ 2 + k
 mixed = 2 + 3 * 4 - 6 / 2 + k
 sci = 1e-4 * 10000 + k
}
solve P with {
 k = 0
}
";
    let run = run_model(src, &[]);
    assert_eq!(run.code, 0, "stderr: {}", run.stderr);
    let env = settled(&run);
    assert_close(&env, "neg_sq", -4.0, 1e-9);
    assert_close(&env, "unary", -4.0, 1e-9);
    assert_close(&env, "right", 512.0, 1e-9);
    assert_close(&env, "mixed", 11.0, 1e-9);
    assert_close(&env, "sci", 1.0, 1e-9);
}

#[test]
fn spec_negative_numeric_literal_binding() {
    let src = "relation L {\n t = surface - rate * h\n}\nsolve L with {\n surface = 288.15\n rate = -0.0065\n h = 1000\n}\n";
    let run = run_model(src, &[]);
    assert_eq!(run.code, 0, "stderr: {}", run.stderr);
    assert_close(&settled(&run), "t", 294.65, 1e-4);
}

// wiki/Language-Specification.md §4.2: <=, >=, <, >, ==, !=
#[test]
fn spec_strict_inequalities_are_enforced() {
    let ok = "relation R {\n y = 2 * x\n}\nsolve R with {\n x = 3\n y > 5\n y < 7\n}\n";
    let run = run_model(ok, &[]);
    assert_eq!(run.code, 0, "stderr: {}", run.stderr);

    let bad = "relation R {\n y = 2 * x\n}\nsolve R with {\n x = 3\n y > 6\n}\n";
    let run = run_model(bad, &[]);
    assert_ne!(run.code, 0, "y = 6 must violate 'y > 6'");
}

// wiki/Language-Specification.md §4.4: `minimize total_cost_usd` (no parentheses)
#[test]
fn spec_objective_without_parentheses() {
    let src = "relation R {\n int n\n cost = n * 10\n cap = n * 3\n cap >= 10\n}\nsolve R with {\n minimize cost\n}\n";
    let run = run_model(src, &[]);
    assert_eq!(run.code, 0, "stderr: {}", run.stderr);
    assert_close(&settled(&run), "n", 4.0, 1e-9);
}

// ---------------------------------------------------------------------------
// README / wiki/Compiler-Toolchain-avenc.md: CLI commands
// ---------------------------------------------------------------------------

#[test]
fn toolchain_emit_c_command_exists() {
    let run = avenc(&["emit-c", "examples/server_capacity.aven"]);
    assert_eq!(run.code, 0, "stderr: {}", run.stderr);
    assert!(run.stdout.contains("#include"), "no C emitted: {}", run.stdout);
}

#[test]
fn toolchain_bench_command_exists() {
    let run = avenc(&["bench", "examples/server_capacity.aven"]);
    assert_eq!(run.code, 0, "stderr: {}", run.stderr);
}

#[test]
fn toolchain_pareto_command_exists() {
    let run = avenc(&["pareto", "examples/server_capacity.aven", "reqs_per_second", "100", "50000"]);
    assert_eq!(run.code, 0, "stderr: {}", run.stderr);
}

// wiki/Compiler-Toolchain-avenc.md §4: exit codes
#[test]
fn toolchain_exit_code_0_only_when_fully_settled() {
    let under = "relation R {\n v = l * w * h\n}\nsolve R with {\n v = 1000\n h = 10\n}\n";
    let run = run_model(under, &[]);
    assert_ne!(
        run.code, 0,
        "an under-determined model did not settle and must not exit 0"
    );
}

#[test]
fn toolchain_exit_code_2_on_constraint_violation() {
    let src = "relation R {\n y = 2 * x\n}\nsolve R with {\n x = 10\n y <= 5\n}\n";
    let run = run_model(src, &[]);
    assert_eq!(run.code, 2, "stdout: {}\nstderr: {}", run.stdout, run.stderr);
}

fn stale_adapter_model(ttl_ms: u64, timestamp_ms: u64) -> String {
    let script = format!(
        "import json; print(json.dumps({{'x': 4.0, 'ttl_ms': {}, 'timestamp_ms': {}}}))",
        ttl_ms, timestamp_ms
    );
    let py = if cfg!(windows) { "python" } else { "python3" };
    format!(
        "relation Sensor using external(\"{}\", \"-c\", \"{}\") {{\n y = x * 2\n z = y + 1\n}}\nsolve Sensor with {{\n}}\n",
        py,
        script.replace('"', "\\\"")
    )
}

#[test]
fn toolchain_exit_code_3_on_stale_input() {
    let run = run_model(&stale_adapter_model(0, 0), &[]);
    assert_eq!(run.code, 3, "stdout: {}\nstderr: {}", run.stdout, run.stderr);
    assert!(run.stderr.contains("DECISION BLOCKED"), "stderr: {}", run.stderr);
}

#[test]
fn freshness_expired_timestamp_blocks_decision() {
    // timestamp in 1970 with a one-minute TTL: long expired
    let run = run_model(&stale_adapter_model(60_000, 1_000), &[]);
    assert_eq!(run.code, 3, "stdout: {}\nstderr: {}", run.stdout, run.stderr);
}

#[test]
fn freshness_fresh_data_is_actionable() {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    let run = run_model(&stale_adapter_model(600_000, now), &[]);
    assert_eq!(run.code, 0, "stdout: {}\nstderr: {}", run.stdout, run.stderr);
    let env = settled(&run);
    assert_close(&env, "z", 9.0, 1e-9);
}

// wiki/Fact-Ledgers-and-Provenance.md §3: stale inputs taint every value derived from them,
// through any number of hops, on every run.
#[test]
fn freshness_taint_reaches_every_derived_value_deterministically() {
    let path = write_model(&stale_adapter_model(0, 0));
    let p = path.to_string_lossy().into_owned();
    for _ in 0..15 {
        let run = avenc(&["why", p.as_str(), "z"]);
        assert!(
            run.stdout.contains("z") && run.stdout.contains("STALE"),
            "z (two hops from stale x) must be tainted:\n{}",
            run.stdout
        );
    }
}

#[test]
fn freshness_taint_follows_backward_solved_values() {
    // x comes from a stale sensor; q is solved *backward* from x = 2 * q.
    let script = "import json; print(json.dumps({'x': 4.0, 'ttl_ms': 0, 'timestamp_ms': 0}))";
    let py = if cfg!(windows) { "python" } else { "python3" };
    let src = format!(
        "relation Sensor using external(\"{}\", \"-c\", \"{}\") {{\n x = 2 * q\n}}\nsolve Sensor with {{\n}}\n",
        py,
        script.replace('"', "\\\"")
    );
    let path = write_model(&src);
    let p = path.to_string_lossy().into_owned();
    let run = avenc(&["why", p.as_str(), "q"]);
    assert!(
        run.stdout.contains("STALE"),
        "q is derived from stale x and must be tainted:\n{}",
        run.stdout
    );
}

// wiki/Fact-Ledgers-and-Provenance.md §2: every externally sourced fact carries a SHA-256
// fingerprint of the raw source payload.
#[test]
fn provenance_external_facts_carry_payload_sha256() {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    let path = write_model(&stale_adapter_model(600_000, now));
    let p = path.to_string_lossy().into_owned();
    let run = avenc(&["why", p.as_str(), "x"]);
    assert!(
        run.stdout.contains("sha256:"),
        "external fact must show its payload SHA-256:\n{}",
        run.stdout
    );
}

// README §3 "Direction-Pinned Forward Blocks": `forward` blocks are evaluated by `avenc run`
#[test]
fn readme_forward_block_is_evaluated_by_run() {
    let src = "forward ServerSizing {
    raw_workers = (reqs_per_second * avg_latency_ms) / 1000.0
    allocated_workers = ceil(raw_workers / target_utilization)
}
solve ServerSizing with {
    reqs_per_second = 50000
    avg_latency_ms = 20
    target_utilization = 0.7
}
";
    let run = run_model(src, &[]);
    assert_eq!(run.code, 0, "stderr: {}", run.stderr);
    let env = settled(&run);
    assert_close(&env, "raw_workers", 1000.0, 1e-9);
    assert_close(&env, "allocated_workers", 1429.0, 1e-9);
}

#[test]
fn readme_forward_results_feed_relations() {
    let run = avenc(&["run", "examples/fleet.aven"]);
    assert_eq!(run.code, 0, "stdout: {}\nstderr: {}", run.stdout, run.stderr);
    let env = settled(&run);
    assert_close(&env, "cost", 360.0 * 0.07 * 1.75, 1e-4);
    assert_close(&env, "units", (500.0 + 44.1) / 25.0, 1e-4);
}

// Every shipped example must run to completion
#[test]
fn examples_all_settle() {
    for entry in std::fs::read_dir("examples").unwrap() {
        let path = entry.unwrap().path();
        if path.extension().map_or(false, |e| e == "aven") {
            let p = path.to_string_lossy().into_owned();
            // flame_eval.py fails 5% of calls by design (hostile black box), so external
            // backends get up to three attempts
            let mut run = avenc(&["run", p.as_str()]);
            for _ in 0..2 {
                if run.code == 0 || !run.stderr.contains("external evaluation failed") {
                    break;
                }
                run = avenc(&["run", p.as_str()]);
            }
            assert_eq!(run.code, 0, "{} failed:\n{}\n{}", p, run.stdout, run.stderr);
        }
    }
}

// Robustness: pathological input is rejected with an error, never a crash
#[test]
fn deeply_nested_expression_is_a_clean_error() {
    let deep = format!("relation R {{\n y = {}x{}\n}}\nsolve R with {{\n x = 1\n}}\n", "(".repeat(20000), ")".repeat(20000));
    let run = run_model(&deep, &[]);
    assert_eq!(run.code, 1, "stderr: {}", run.stderr);
    assert!(run.stderr.contains("nested more than"), "stderr: {}", run.stderr);

    let chain = format!("relation R {{\n y = {}\n}}\nsolve R with {{\n x = 1\n}}\n", vec!["x"; 20000].join(" + "));
    let run = run_model(&chain, &[]);
    assert_eq!(run.code, 1, "stderr: {}", run.stderr);
}

// README "Live Multi-Stream Flight Dynamics Mesh": the flight model's atmosphere must match the
// U.S. Standard Atmosphere 1976 (geopotential altitude, standard sea-level day). The relation
// body is read from the shipped model so this checks the real file.
#[test]
fn flight_model_matches_us_standard_atmosphere_1976() {
    let model = std::fs::read_to_string("evaluations/flight_mesh/flight_mesh_ipc.aven").unwrap();
    let start = model.find('{').unwrap();
    let end = start + model[start..].find("\n}").unwrap();
    let body = &model[start + 1..end];

    // (altitude m, temperature K, pressure Pa) — USSA76 layers 0 (troposphere) and 1
    let reference = [
        (0.0, 288.15, 101325.0),
        (5000.0, 255.65, 54019.9),
        (11000.0, 216.65, 22632.1),
        (13000.0, 216.65, 16510.4),
        (15000.0, 216.65, 12044.6),
        (20000.0, 216.65, 5474.9),
    ];
    for (h, t_ref, p_ref) in reference {
        let src = format!(
            "relation Atmos {{{}\n}}\nsolve Atmos with {{\n surface_temp_c = 15\n surface_altim_hpa = 1013.25\n baro_altitude_m = {}\n velocity_ms = 200\n vertical_rate_ms = 0\n gauge_elevation_m = 0\n river_discharge_cfs = 0\n}}\n",
            body, h
        );
        let run = run_model(&src, &["--json"]);
        assert_eq!(run.code, 0, "h={} stderr: {}", h, run.stderr);
        let line = run.stdout.lines().last().unwrap_or("");
        let get = |k: &str| -> f64 {
            let pat = format!("\"{}\": ", k);
            let i = line.find(&pat).unwrap_or_else(|| panic!("{} missing: {}", k, line)) + pat.len();
            line[i..].split(|c| c == ',' || c == '}').next().unwrap().trim().parse().unwrap()
        };
        let t = get("ambient_temp_k");
        let p = get("ambient_pressure_pa");
        assert!((t - t_ref).abs() < 0.01, "h={} m: T={} K, USSA76 {} K", h, t, t_ref);
        assert!((p - p_ref).abs() / p_ref < 1e-3, "h={} m: p={} Pa, USSA76 {} Pa", h, p, p_ref);
    }
}
