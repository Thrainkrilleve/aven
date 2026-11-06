// tests/units_runtime.rs — Units are converted, not just checked.
//
// Every value inside the solver is in coherent base units; numbers cross into and out of
// the solver in each variable's own unit (see units::UnitScale). These tests pin that
// behaviour through the CLI, where users see it.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

static FILE_COUNTER: AtomicUsize = AtomicUsize::new(0);

fn temp_dir() -> PathBuf {
    let n = FILE_COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("aven_units_{}_{}", std::process::id(), n));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn write_model(src: &str) -> PathBuf {
    let path = temp_dir().join("model.aven");
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

fn run_file(path: &Path, extra: &[&str]) -> Run {
    let p = path.to_string_lossy().into_owned();
    let mut args = vec!["run", p.as_str()];
    args.extend_from_slice(extra);
    avenc(&args)
}

/// `name -> (value, unit label)` from the "Settled Environment:" section.
fn settled(run: &Run) -> HashMap<String, (f64, String)> {
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
                let mut parts = v.trim().splitn(2, ' ');
                let num = parts.next().unwrap_or("").parse::<f64>();
                let unit = parts.next().unwrap_or("").trim().to_string();
                if let Ok(x) = num {
                    env.insert(k.trim().to_string(), (x, unit));
                }
            }
        }
    }
    env
}

fn assert_value(env: &HashMap<String, (f64, String)>, var: &str, expected: f64, unit: &str) {
    let (got, got_unit) = env
        .get(var)
        .unwrap_or_else(|| panic!("'{}' missing from settled environment: {:?}", var, env));
    assert!(
        (got - expected).abs() <= 1e-3 * (1.0 + expected.abs()),
        "{} = {} {} (expected {} {})",
        var,
        got,
        got_unit,
        expected,
        unit
    );
    assert_eq!(got_unit, unit, "unit label for {}", var);
}

#[test]
fn shipped_aerodynamics_example_converts_km_per_hour() {
    // 250 km/h = 69.444 m/s; q = 0.5 * 1.225 * 69.444^2 = 2953.80 Pa; lift = q * 32 * 0.55
    let run = run_file(&PathBuf::from("examples/aerodynamics.aven"), &[]);
    assert_eq!(run.code, 0, "stderr: {}", run.stderr);
    let env = settled(&run);
    assert_value(&env, "velocity", 250.0, "km/h");
    assert_value(&env, "dynamic_pressure", 2953.8002, "Pa");
    // lift_coeff is declared `scalar` (dimensionless), so lift is inferred in newtons.
    assert_value(&env, "lift", 2953.8002 * 32.0 * 0.55, "N");
}

#[test]
fn shipped_units_tutorial_converts_to_metres() {
    // 100 km/h for 10 s is 277.78 m, not 1000
    let run = run_file(
        &PathBuf::from("examples/tutorial/02_units_and_dimensions.aven"),
        &[],
    );
    assert_eq!(run.code, 0, "stderr: {}", run.stderr);
    let env = settled(&run);
    assert_value(&env, "distance_travelled", 277.7778, "m");
    assert_value(&env, "acceleration", 2.7778, "m·s^-2");
    assert_value(&env, "speed", 100.0, "km/h");
}

#[test]
fn json_output_is_base_units_with_a_units_map() {
    let run = run_file(&PathBuf::from("examples/aerodynamics.aven"), &["--json"]);
    assert_eq!(run.code, 0, "stderr: {}", run.stderr);
    assert!(
        run.stdout.contains("\"velocity\": 69.44444444444"),
        "{}",
        run.stdout
    );
    assert!(
        run.stdout.contains("\"velocity\": \"m·s^-1\""),
        "{}",
        run.stdout
    );
}

#[test]
fn bare_numbers_take_the_declared_unit() {
    let path = write_model(
        "signal speed : km/h\nsignal t : min\n\nrelation Trip {\n    distance = speed * t\n}\n\nsolve Trip with {\n    speed = 90\n    t = 20\n}\n",
    );
    let run = run_file(&path, &[]);
    assert_eq!(run.code, 0, "stderr: {}", run.stderr);
    let env = settled(&run);
    // 90 km/h for 20 min = 30 km
    assert_value(&env, "distance", 30000.0, "m");
    assert_value(&env, "t", 20.0, "min");
}

#[test]
fn celsius_and_fahrenheit_are_absolute_temperatures() {
    let path = write_model(
        "signal room : degC\nsignal oven : degF\n\nrelation Temps {\n    room_k = room * 1\n    oven_k = oven * 1\n}\n\nsolve Temps with {\n    room = 20\n    oven = 212\n}\n",
    );
    let run = run_file(&path, &[]);
    assert_eq!(run.code, 0, "stderr: {}", run.stderr);
    let env = settled(&run);
    assert_value(&env, "room", 20.0, "degC");
    assert_value(&env, "room_k", 293.15, "K");
    assert_value(&env, "oven_k", 373.15, "K");
}

#[test]
fn binding_unit_must_match_the_declared_signal_unit() {
    let path = write_model(
        "signal speed : km/h\n\nrelation R {\n    y = speed * 2\n}\n\nsolve R with {\n    speed = 10 s\n}\n",
    );
    let run = run_file(&path, &[]);
    assert_eq!(run.code, 1, "stdout: {}", run.stdout);
    assert!(
        run.stderr.contains("dimensional mismatch"),
        "{}",
        run.stderr
    );
}

#[test]
fn currencies_do_not_convert_at_hidden_exchange_rates() {
    let path = write_model(
        "signal a : usd\nsignal b : eur\n\nrelation Sum {\n    total = a + b\n}\n\nsolve Sum with {\n    a = 100\n    b = 100\n}\n",
    );
    let run = run_file(&path, &[]);
    assert_eq!(
        run.code, 1,
        "usd + eur must be rejected, got: {}",
        run.stdout
    );
    assert!(
        run.stderr.contains("dimensional mismatch"),
        "{}",
        run.stderr
    );
}

#[test]
fn declared_units_without_a_scale_never_equal_a_base_unit() {
    // `unit length : furlong` has no scale, so a furlong must not silently be a metre
    let path = write_model(
        "unit length : furlong\nsignal a : m\nsignal b : furlong\n\nrelation Sum {\n    total = a + b\n}\n\nsolve Sum with {\n    a = 1\n    b = 1\n}\n",
    );
    let run = run_file(&path, &[]);
    assert_eq!(
        run.code, 1,
        "m + furlong must be rejected, got: {}",
        run.stdout
    );
}

#[test]
fn adapters_exchange_values_in_declared_units() {
    let dir = temp_dir();
    // Reports a speed 14 below the limit it receives, both in km/h. If Aven sent the limit
    // in m/s (13.89) the speed would come back negative.
    std::fs::write(
        dir.join("sensor.py"),
        "import json, sys\nreq = json.load(sys.stdin)\nprint(json.dumps({\"speed\": req[\"limit\"] - 14.0}))\n",
    )
    .unwrap();
    let model = dir.join("model.aven");
    std::fs::write(
        &model,
        "signal speed : km/h\nsignal limit : km/h\n\nrelation Sensor using external(\"python3\", \"sensor.py\") {\n    margin = limit - speed\n}\n\nsolve Sensor with {\n    limit = 50\n}\n",
    )
    .unwrap();
    let run = run_file(&model, &[]);
    assert_eq!(
        run.code, 0,
        "stdout: {}\nstderr: {}",
        run.stdout, run.stderr
    );
    let env = settled(&run);
    assert_value(&env, "speed", 36.0, "km/h");
    assert_value(&env, "limit", 50.0, "km/h");
    // (50 - 36) km/h = 3.8889 m/s
    assert_value(&env, "margin", 14.0 / 3.6, "m·s^-1");
}

#[test]
fn debug_what_if_reads_values_in_the_declared_unit() {
    let run = avenc(&[
        "debug",
        "examples/aerodynamics.aven",
        "--what-if",
        "velocity=300",
    ]);
    assert_eq!(run.code, 0, "stderr: {}", run.stderr);
    assert!(run.stdout.contains("300.0000 km/h"), "{}", run.stdout);
    // q scales with v^2: 2953.83 * (300/250)^2 = 4253.51
    assert!(run.stdout.contains("4253.5148"), "{}", run.stdout);
}

#[test]
fn debug_what_if_rejects_a_value_that_is_not_a_number() {
    let run = avenc(&[
        "debug",
        "examples/aerodynamics.aven",
        "--what-if",
        "velocity=fast",
    ]);
    assert_eq!(run.code, 1, "stdout: {}", run.stdout);
    assert!(run.stderr.contains("invalid number"), "{}", run.stderr);
}

#[test]
fn pareto_range_is_in_the_declared_unit_and_required() {
    let run = avenc(&[
        "pareto",
        "examples/aerodynamics.aven",
        "velocity",
        "100",
        "300",
        "--steps",
        "2",
    ]);
    assert_eq!(run.code, 0, "stderr: {}", run.stderr);
    assert!(
        run.stdout
            .contains("velocity in [100.0000 km/h, 300.0000 km/h]"),
        "{}",
        run.stdout
    );

    let missing = avenc(&["pareto", "examples/aerodynamics.aven"]);
    assert_eq!(missing.code, 1);
    assert!(
        missing.stderr.contains("Usage: avenc pareto"),
        "{}",
        missing.stderr
    );
}

fn stream(model: &str, lines: &[&str]) -> String {
    stream_file(&write_model(model), lines)
}

fn stream_file(path: &std::path::Path, lines: &[&str]) -> String {
    let mut child = Command::new(env!("CARGO_BIN_EXE_avenc"))
        .args(["stream", path.to_str().unwrap()])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    {
        use std::io::Write;
        let stdin = child.stdin.as_mut().unwrap();
        for l in lines {
            writeln!(stdin, "{}", l).unwrap();
        }
    }
    let out = child.wait_with_output().unwrap();
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[test]
fn stream_converts_input_and_reports_derived_values() {
    let out = stream(
        "signal speed : km/h\n\nforward R {\n    speed_ms = speed * 1\n}\n",
        &["{\"speed\": 72}"],
    );
    // 72 km/h = 20 m/s
    assert!(out.contains("SETTLED"), "{}", out);
    assert!(out.contains("speed_ms = 20.0000 m·s^-1"), "{}", out);
    assert!(out.contains("1 settled, 0 alarm(s), 0 unsolved"), "{}", out);
}

#[test]
fn stream_does_not_claim_convergence_when_the_solve_fails() {
    let out = stream(
        "signal a : scalar\nsignal b : scalar\nrelation R {\n    c = a + b\n    d = c * 2\n    d = a + b\n}\n",
        &["{\"a\": 1, \"b\": 2}"],
    );
    assert!(!out.contains("SETTLED"), "{}", out);
    assert!(!out.contains("CONVERGED"), "{}", out);
    assert!(out.contains("UNSOLVED"), "{}", out);
}

#[test]
fn adapters_start_when_the_model_is_named_without_a_directory() {
    // `avenc run model.aven` from the model's own directory: the model directory is the
    // current directory, not an empty path the adapter cannot be started in.
    let dir = temp_dir();
    std::fs::write(
        dir.join("sensor.py"),
        "import json\nprint(json.dumps({\"x\": 2.0}))\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("model.aven"),
        "signal x : scalar\nrelation Sensor using external(\"python3\", \"sensor.py\") {\n    y = x * 3\n}\n",
    )
    .unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_avenc"))
        .args(["run", "model.aven"])
        .current_dir(&dir)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "{}{}",
        stdout,
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(stdout.contains("y                = 6.0000"), "{}", stdout);
}

#[test]
fn fractional_powers_have_the_right_dimension_or_are_refused() {
    let ok = write_model(
        "signal area : m^2\n\nrelation R {\n    side = area ^ 0.5\n}\n\nsolve R with {\n    area = 16\n}\n",
    );
    let run = run_file(&ok, &[]);
    assert_eq!(run.code, 0, "stderr: {}", run.stderr);
    assert_value(&settled(&run), "side", 4.0, "m");

    // The square root of a length has no dimension: refuse rather than mislabel it
    let bad = write_model(
        "signal l : m\n\nrelation R {\n    y = l ^ 0.5\n}\n\nsolve R with {\n    l = 16\n}\n",
    );
    let run = run_file(&bad, &[]);
    assert_eq!(run.code, 1, "stdout: {}", run.stdout);
    assert!(run.stderr.contains("power"), "{}", run.stderr);
}

#[test]
fn stream_reports_broken_limits_as_alarms() {
    let out = stream(
        "signal limit : scalar\nsignal reading : scalar\nrelation S {\n    margin = limit - reading\n}\nsolve S with {\n    limit = 18\n    margin >= 0\n}\n",
        &["{\"reading\": 21}", "{\"reading\": 9}"],
    );
    assert!(out.contains("ALARM"), "{}", out);
    assert!(out.contains("1 settled, 1 alarm(s), 0 unsolved"), "{}", out);
}

#[test]
fn openplc_supervisor_example_alarms_and_never_fills_in_missing_readings() {
    let model = std::path::Path::new("examples/stream/openplc_supervisor.aven");
    let out = stream_file(
        model,
        &[
            "{\"scan_ms\": 9.2, \"power_w\": 211, \"temp_c\": 43.1, \"quality_pct\": 98.7}",
            "{\"scan_ms\": 9.4, \"power_w\": 215, \"temp_c\": 61.0, \"quality_pct\": 98.5}",
            // a reading without the fields the limits need is not a pass
            "{\"scan_ms\": 9.3}",
        ],
    );
    assert!(
        out.contains("3 cycle(s), 1 settled, 1 alarm(s), 1 unsolved"),
        "{}",
        out
    );
}
