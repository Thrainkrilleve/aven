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
                // "1429 (int)" or "250.0000 km/h": the number is the first field
                let v = v.split_whitespace().next().unwrap_or("");
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
    assert!(
        run.stdout.contains("#include"),
        "no C emitted: {}",
        run.stdout
    );
}

#[test]
fn toolchain_bench_command_exists() {
    let run = avenc(&["bench", "examples/server_capacity.aven"]);
    assert_eq!(run.code, 0, "stderr: {}", run.stderr);
}

#[test]
fn toolchain_pareto_command_exists() {
    let run = avenc(&[
        "pareto",
        "examples/server_capacity.aven",
        "reqs_per_second",
        "100",
        "50000",
    ]);
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
    assert_eq!(
        run.code, 2,
        "stdout: {}\nstderr: {}",
        run.stdout, run.stderr
    );
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
    assert_eq!(
        run.code, 3,
        "stdout: {}\nstderr: {}",
        run.stdout, run.stderr
    );
    assert!(
        run.stderr.contains("DECISION BLOCKED"),
        "stderr: {}",
        run.stderr
    );
}

#[test]
fn freshness_expired_timestamp_blocks_decision() {
    // timestamp in 1970 with a one-minute TTL: long expired
    let run = run_model(&stale_adapter_model(60_000, 1_000), &[]);
    assert_eq!(
        run.code, 3,
        "stdout: {}\nstderr: {}",
        run.stdout, run.stderr
    );
}

#[test]
fn freshness_fresh_data_is_actionable() {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    let run = run_model(&stale_adapter_model(600_000, now), &[]);
    assert_eq!(
        run.code, 0,
        "stdout: {}\nstderr: {}",
        run.stdout, run.stderr
    );
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
    assert_eq!(
        run.code, 0,
        "stdout: {}\nstderr: {}",
        run.stdout, run.stderr
    );
    let env = settled(&run);
    assert_close(&env, "cost", 360.0 * 0.07 * 1.75, 1e-4);
    assert_close(&env, "units", (500.0 + 44.1) / 25.0, 1e-4);
}

// Every shipped example must run to completion
#[test]
fn examples_all_settle() {
    for entry in std::fs::read_dir("examples").unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|e| e == "aven") {
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
    let deep = format!(
        "relation R {{\n y = {}x{}\n}}\nsolve R with {{\n x = 1\n}}\n",
        "(".repeat(20000),
        ")".repeat(20000)
    );
    let run = run_model(&deep, &[]);
    assert_eq!(run.code, 1, "stderr: {}", run.stderr);
    assert!(
        run.stderr.contains("nested more than"),
        "stderr: {}",
        run.stderr
    );

    let chain = format!(
        "relation R {{\n y = {}\n}}\nsolve R with {{\n x = 1\n}}\n",
        vec!["x"; 20000].join(" + ")
    );
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
            let i = line
                .find(&pat)
                .unwrap_or_else(|| panic!("{} missing: {}", k, line))
                + pat.len();
            line[i..]
                .split([',', '}'])
                .next()
                .unwrap()
                .trim()
                .parse()
                .unwrap()
        };
        let t = get("ambient_temp_k");
        let p = get("ambient_pressure_pa");
        assert!(
            (t - t_ref).abs() < 0.01,
            "h={} m: T={} K, USSA76 {} K",
            h,
            t,
            t_ref
        );
        assert!(
            (p - p_ref).abs() / p_ref < 1e-3,
            "h={} m: p={} Pa, USSA76 {} Pa",
            h,
            p,
            p_ref
        );
    }
}

// wiki/Fact-Ledgers-and-Provenance.md: tamper-evident, append-only audit trail
#[test]
fn ledger_hmac_matches_rfc4231_vector() {
    // RFC 4231 test case 2
    assert_eq!(
        avenc::ledger::hmac_sha256_hex(b"Jefe", b"what do ya want for nothing?"),
        "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
    );
}

#[test]
fn ledger_chain_is_deterministic_and_verifies() {
    use avenc::ledger::{Fact, Ledger, verify_jsonl};
    let build = |key: Option<&[u8]>| {
        let mut l = key.map_or_else(Ledger::new, Ledger::with_key);
        for i in 1..=4 {
            l.append(Fact {
                seq: i,
                tick: 0,
                kind: "derived".to_string(),
                name: format!("v{}", i),
                value: 0.1 * i as f64,
                causes: if i > 1 { vec![i - 1] } else { vec![] },
                origin: "= test \"quoted\"".to_string(),
                metadata: None,
            });
        }
        l
    };
    let a = build(None);
    assert_eq!(
        a.head(),
        build(None).head(),
        "same facts must give the same head hash"
    );
    assert!(a.verify().is_ok());
    let v = verify_jsonl(&a.to_jsonl(), None).expect("exported log must verify");
    assert_eq!(v.facts, 4);
    assert_eq!(v.head, a.head());

    let signed = build(Some(b"k"));
    assert!(verify_jsonl(&signed.to_jsonl(), Some(b"k")).is_ok());
    assert!(verify_jsonl(&signed.to_jsonl(), Some(b"wrong key")).is_err());
    assert!(
        verify_jsonl(&a.to_jsonl(), Some(b"k")).is_err(),
        "unsigned log must fail a signed check"
    );
}

#[test]
fn ledger_cli_detects_tampering() {
    let dir = std::env::temp_dir().join(format!("aven_ledger_cli_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let log = dir.join("fleet.jsonl");
    let log_s = log.to_string_lossy().into_owned();
    let run = avenc(&["run", "examples/fleet.aven", "--ledger", log_s.as_str()]);
    assert_eq!(run.code, 0, "stderr: {}", run.stderr);
    assert_eq!(avenc(&["verify-ledger", log_s.as_str()]).code, 0);

    let text = std::fs::read_to_string(&log).unwrap();
    let forged = text.replacen(
        "\"name\": \"cost\", \"value\": 44.10000000000001",
        "\"name\": \"cost\", \"value\": 40",
        1,
    );
    assert_ne!(forged, text, "test must actually change the log");
    std::fs::write(&log, forged).unwrap();
    let check = avenc(&["verify-ledger", log_s.as_str()]);
    assert_eq!(check.code, 1, "forged log accepted: {}", check.stdout);
    assert!(
        check.stderr.contains("TAMPERED"),
        "stderr: {}",
        check.stderr
    );
}

// Non-convex integer models are certified globally optimal by interval branch-and-bound
#[test]
fn nonconvex_integer_optimum_is_proven() {
    let src = "relation Cluster {
    int nodes
    int drives
    capacity = nodes * drives * 13.5
    cost = nodes * 2000.0 + nodes * drives * 600.0
    capacity >= 100.0
    drives <= 12.0
}
solve Cluster with {
    minimize(cost)
}
";
    let run = run_model(src, &["--json"]);
    assert_eq!(run.code, 0, "stderr: {}", run.stderr);
    assert!(
        run.stdout.contains("\"optimality\": \"proven\""),
        "{}",
        run.stdout
    );
    assert!(run.stdout.contains("\"cost\": 6800"), "{}", run.stdout);
}

// An adapter only has to supply what nothing else in the model can compute: a constraint on
// a value another relation derives must not be demanded from the adapter.
#[test]
fn adapter_is_not_asked_for_values_other_relations_compute() {
    let py = if cfg!(windows) { "python" } else { "python3" };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    let script = format!(
        "import json; print(json.dumps({{'surface_temp_c': 20.0, 'timestamp_ms': {}, 'ttl_ms': 60000}}))",
        now
    );
    let src = format!(
        "relation Weather using external(\"{}\", \"-c\", \"{}\") {{\n outside_c = surface_temp_c\n}}\nrelation Plant {{\n it_kw = load * 2\n cop = 7.5 - 0.12 * outside_c\n}}\nsolve Plant, Weather with {{\n load = 100\n it_kw <= 800\n}}\n",
        py,
        script.replace('"', "\\\"")
    );
    let run = run_model(&src, &[]);
    assert_eq!(
        run.code, 0,
        "stdout: {}\nstderr: {}",
        run.stdout, run.stderr
    );
    let env = settled(&run);
    assert_close(&env, "it_kw", 200.0, 1e-9);
    assert_close(&env, "cop", 7.5 - 0.12 * 20.0, 1e-9);
}

// Backward solving through log and fractional powers must not give up when a full Newton
// step leaves the domain: 0 dBZ inverts to the Marshall-Palmer rain rate (200 R^1.6 = 1).
#[test]
fn backward_solve_is_damped_near_domain_edges() {
    for (dbz, expected) in [
        (0.0, (1.0f64 / 200.0).powf(1.0 / 1.6)),
        (45.0, (10f64.powf(4.5) / 200.0).powf(1.0 / 1.6)),
    ] {
        let src = format!(
            "relation R {{\n dbz = 10 * log(200 * rain ^ 1.6) / 2.302585092994046\n}}\nsolve R with {{\n dbz = {}\n}}\n",
            dbz
        );
        let run = run_model(&src, &[]);
        assert_eq!(run.code, 0, "dbz={} stderr: {}", dbz, run.stderr);
        assert_close(&settled(&run), "rain", expected, 1e-4);
    }
}

// Independent coupled subsystems are solved separately: an unsolvable equation elsewhere
// must not stop a solvable 2x2 system from settling.
#[test]
fn independent_subsystems_solve_separately() {
    let src = "relation R {\n total = a * b\n other = a + 2 * b\n y = sqrt(x) + sqrt(z)\n}\nsolve R with {\n total = 6\n other = 8\n y = -1\n}\n";
    let run = run_model(src, &["--json"]);
    assert_eq!(run.code, 1, "the sqrt equation has no real solution");
    let line = run.stdout.lines().last().unwrap_or("");
    assert!(line.contains("\"status\": \"underdetermined\""), "{}", line);
    assert!(
        line.contains("\"a\": ") && line.contains("\"b\": "),
        "a and b must settle: {}",
        line
    );
}

// An integer variable fixed by equations at a fractional value makes the model infeasible
#[test]
fn integer_determined_by_equations_must_be_whole() {
    let src = "relation R {\n int n\n total = 3 * n\n}\nsolve R with {\n total = 10\n}\n";
    let run = run_model(src, &[]);
    assert_ne!(
        run.code, 0,
        "n = 3.333 must not be accepted: {}",
        run.stdout
    );
    assert!(
        run.stderr.contains("not a whole number"),
        "stderr: {}",
        run.stderr
    );
}

// evaluations/possible/weather-predict.aven: thermodynamics (Ambaum 2020), kinematics and the
// Marshall-Palmer inverse, run on fixed observations without network access
#[test]
fn weather_cell_physics() {
    let model = std::fs::read_to_string("evaluations/possible/weather-predict.aven").unwrap();
    let start = model.find("relation WeatherCell").unwrap();
    let end = start + model[start..].find("\n}").unwrap() + 2;
    let ambaum = |t_c: f64| {
        let t = t_c + 273.15;
        let (cpl, cpv, rv, l0, t0) = (
            4219.4,
            1860.078011865639,
            461.52311572606084,
            2500840.0,
            273.16,
        );
        let latent = l0 - (cpl - cpv) * (t - t0);
        6.112 * (t0 / t).powf((cpl - cpv) / rv) * ((l0 / t0 - latent / t) / rv).exp()
    };
    let (t, td) = (21.0, 9.0);
    let rh = 100.0 * ambaum(td) / ambaum(t);
    // Winds on a known plane: du/dx = 2e-5, du/dy = -1e-5, dv/dx = 3e-5, dv/dy = 4e-5
    let pts = [(1000.0, 2000.0), (-30000.0, 15000.0), (20000.0, -40000.0)];
    let mut binds = format!(
        " temp_c = {}\n dewpoint_c = {}\n rh_reported = {}\n pressure_hpa = 1000\n wind_u = 0\n wind_v = 0\n aloft_u = 10\n aloft_v = 0\n aloft_height_m = 1000\n steer_u = 5\n steer_v = 5\n radar_dbz = 45\n gauge_available = 0\n gauge_rate_mmh = 0\n upstream_dbz_15 = 30\n upstream_dbz_30 = 20\n upstream_dbz_60 = 0\n layer_depth_m = 1000\n",
        t, td, rh
    );
    for (n, (x, y)) in pts.iter().enumerate() {
        let u = 3.0 + 2e-5 * x - 1e-5 * y;
        let v = -1.0 + 3e-5 * x + 4e-5 * y;
        binds.push_str(&format!(
            " x{0} = {1}\n y{0} = {2}\n u{0} = {3}\n v{0} = {4}\n",
            n + 1,
            x,
            y,
            u,
            v
        ));
    }
    let src = format!(
        "{}\nsolve WeatherCell with {{\n{}}}\n",
        &model[start..end],
        binds
    );
    let run = run_model(&src, &["--json"]);
    assert_eq!(run.code, 0, "stderr: {}", run.stderr);
    let line = run.stdout.lines().last().unwrap().to_string();
    let get = |k: &str| -> f64 {
        let pat = format!("\"{}\": ", k);
        let i = line.find(&pat).unwrap_or_else(|| panic!("{} missing", k)) + pat.len();
        line[i..]
            .split([',', '}'])
            .next()
            .unwrap()
            .trim()
            .parse()
            .unwrap()
    };
    assert!((get("e_sat_hpa") - ambaum(t)).abs() < 1e-9);
    assert!((get("relative_humidity") - rh).abs() < 1e-9);
    assert!(get("rh_residual") < 1e-9);
    assert!((get("divergence_per_s") - 6e-5).abs() < 1e-12);
    assert!((get("vorticity_per_s") - 4e-5).abs() < 1e-12);
    assert!((get("vertical_velocity_ms") + 0.06).abs() < 1e-9);
    assert!((get("vertical_shear_ms_per_km") - 10.0).abs() < 1e-9);
    assert!((get("radar_rain_mmh") - (10f64.powf(4.5) / 200.0).powf(1.0 / 1.6)).abs() < 1e-6);
}

// A MILP whose integer equality fails the GCD test is infeasible, not unbounded. This is the
// exact pathology that made avenc report "unbounded" for an infeasible model: an unbounded
// objective over a region that contains no integer point.
#[test]
fn milp_gcd_test_catches_integer_infeasibility() {
    // 2*x0 + 2*x1 is always even; it cannot equal 3. The objective is unbounded below.
    let src = "relation S {\n int x0\n int x1\n r = 2 * x0 + 2 * x1\n r == 3\n obj = 0 - x0\n}\nsolve S with {\n minimize(obj)\n}\n";
    let run = run_model(src, &[]);
    assert_ne!(run.code, 0, "must not settle: {}", run.stdout);
    assert!(
        run.stderr.contains("No feasible solution"),
        "stderr: {}",
        run.stderr
    );
    assert!(
        !run.stderr.contains("unbounded"),
        "must not claim unbounded: {}",
        run.stderr
    );
}

// A feasible integer model with an unbounded improving direction is correctly unbounded.
#[test]
fn milp_feasible_and_unbounded_is_unbounded() {
    let src = "relation S {\n int x\n obj = 0 - x\n x >= 0\n}\nsolve S with {\n minimize(obj)\n}\n";
    let run = run_model(src, &[]);
    assert_ne!(run.code, 0, "unbounded model must not settle");
    assert!(run.stderr.contains("unbounded"), "stderr: {}", run.stderr);
}

// Hard MILP over an unbounded integer domain: Gomory cuts close the gap so avenc now solves
// it and proves optimality (maximize obj = -8 at x=[6,12,3,0,0,1], matching HiGHS). It must
// never assert a false verdict.
#[test]
fn milp_unbounded_domain_solved_and_proven_by_cuts() {
    let src = "relation S {
    int x0
    int x1
    int x2
    int x3
    int x4
    int x5
    r0 = -4*x0 + 1*x1 + -3*x2 + 6*x3 + 5*x4 + 5*x5
    r0 <= -12
    r1 = 4*x0 + -5*x1 + 3*x2 + -6*x3 + -2*x4 + 3*x5
    r1 <= 26
    r2 = 4*x0 + -3*x1 + -4*x2 + -5*x3 + -3*x4 + 4*x5
    r2 <= -14
    r3 = -2*x0 + 5*x1 + -5*x2 + -2*x3 + -4*x4 + -6*x5
    r3 >= 9
    r4 = 2*x0 + -5*x1 + 4*x2 + 3*x3 + -6*x4 + -6*x5
    r4 <= 17
    r5 = -4*x0 + 6*x1 + 2*x2 + 3*x3 + 6*x4 + 3*x5
    r5 >= 29
    r6 = 2*x0 + 2*x1 + -2*x2 + 0*x3 + -6*x4 + -3*x5
    r6 == 27
    obj = 6*x0 + -2*x1 + -5*x2 + -8*x3 + -7*x4 + -5*x5
    x0 <= 6
}
solve S with {
    maximize(obj)
}
";
    let run = run_model(src, &["--json"]);
    assert_eq!(run.code, 0, "cuts should solve it: {}", run.stderr);
    let line = run.stdout.lines().last().unwrap();
    assert!(
        line.contains("\"optimality\": \"proven\""),
        "must be proven optimal: {}",
        line
    );
    assert!(line.contains("\"obj\": -8"), "optimum is -8: {}", line);
}

#[test]
fn printed_version_matches_cargo_toml_and_the_site_badge() {
    let run = avenc(&["--version"]);
    assert_eq!(run.code, 0, "{}", run.stderr);
    let version = run
        .stdout
        .trim()
        .strip_prefix("avenc ")
        .expect("avenc --version prints 'avenc <version>'");
    assert!(!version.is_empty());
    let cargo = std::fs::read_to_string("Cargo.toml").expect("Cargo.toml");
    assert!(
        cargo.contains(&format!("version = \"{version}\"")),
        "Cargo.toml does not use {version}"
    );
    for page in ["index.html", "docs.html", "registry.html", "weather.html"] {
        let html = std::fs::read_to_string(format!("web/public/{page}")).expect(page);
        let badge = format!("<span class=\"brand-version\">v{version}</span>");
        assert!(html.contains(&badge), "{page} badge is not v{version}");
    }
}

// STANDARDS.md §2.1: non-finite values are rejected, never reported as results
#[test]
fn standards_non_finite_results_are_rejected() {
    for expr in ["exp(x * 1000)", "x / 0", "sqrt(x - 5)", "ln(x - 5)"] {
        let src = format!(
            "relation R {{\n    y = {}\n}}\nsolve R with {{\n    x = 1\n}}\n",
            expr
        );
        let run = run_model(&src, &[]);
        assert_eq!(
            run.code, 1,
            "{} must fail, got stdout: {}",
            expr, run.stdout
        );
        assert!(
            !run.stdout.contains("inf") && !run.stdout.contains("NaN"),
            "{}: {}",
            expr,
            run.stdout
        );
    }
}
