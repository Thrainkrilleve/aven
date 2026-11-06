// tests/phase4_shm_ffi.rs — Phase 4: Shared-Memory Rings, Native Dynamic FFI, & Bidirectional Actuation
//
// Milestone Phase 4:
//   1. Shared Memory Rings (`using shm(...)`) with sub-100µs ingestion latency.
//   2. Native C/Rust Dynamic Library FFI (`using native(...)`) with zero-copy in-process evaluation.
//   3. Bidirectional IPC Daemon Control with real-time actuation streaming back to actuators.
//   4. Multi-round randomized tests with fresh OS entropy to ensure data is not reused.

use avenc::evaluator::{Evaluator, NativeFfiEvaluator, ShmEvaluator, ShmRingBuffer, VarMap};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// High-entropy pseudo-random number generator (pure std XorShift64*).
struct EntropyRng {
    state: u64,
}

impl EntropyRng {
    fn new() -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos() as u64;
        let pid = std::process::id() as u64;
        static COUNTER: AtomicU64 = AtomicU64::new(1);
        let c = COUNTER.fetch_add(1, Ordering::SeqCst);
        let seed = nanos ^ (pid.rotate_left(17)) ^ (c.rotate_left(33)) ^ 0x9e3779b97f4a7c15;
        Self {
            state: if seed == 0 { 0xa5a5a5a512345678 } else { seed },
        }
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.state = x;
        x.wrapping_mul(0x2545F4914F6CDD1D)
    }

    fn next_f64(&mut self, min: f64, max: f64) -> f64 {
        let frac = (self.next_u64() % 1_000_000) as f64 / 1_000_000.0;
        min + frac * (max - min)
    }

    fn rand_ident(&mut self, prefix: &str) -> String {
        format!("{}_{:08x}", prefix, (self.next_u64() & 0xffffffff))
    }
}

fn avenc(args: &[&str]) -> (i32, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_avenc"))
        .args(args)
        .output()
        .expect("Failed to launch avenc binary");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

fn parse_settled(stdout: &str) -> HashMap<String, f64> {
    let mut env = HashMap::new();
    let mut in_env = false;
    for line in stdout.lines() {
        if line.starts_with("Settled Environment:") {
            in_env = true;
            continue;
        }
        if in_env {
            let t = line.trim();
            if t.is_empty() || t.starts_with("Fact Ledger") || t.starts_with("Ledger OK:") {
                break;
            }
            if let Some((k, v)) = t.split_once('=').or_else(|| t.split_once(':')) {
                let v = v.trim().trim_end_matches("(int)").trim();
                let val_str = v.split_whitespace().next().unwrap_or("0");
                if let Ok(val) = val_str.parse::<f64>() {
                    env.insert(k.trim().to_string(), val);
                }
            }
        }
    }
    env
}

// ============================================================================
// TEST 1: Randomized Shared-Memory Ring Buffer (`using shm(...)`) & Sub-100µs Ingestion
// ============================================================================
#[test]
fn test_phase4_randomized_shm_ring_buffer_and_latency() {
    let mut rng = EntropyRng::new();

    // 1. Generate randomized telemetry parameters and file path
    let shm_name = rng.rand_ident("aven_shm_test");
    let shm_path = std::env::temp_dir().join(format!("{}.shm", shm_name));
    let _ = std::fs::remove_file(&shm_path);

    let temp_k = rng.next_f64(260.0, 320.0);
    let pressure_pa = rng.next_f64(85000.0, 105000.0);
    let flow_rate = rng.next_f64(1.0, 50.0);

    // 2. Initialize 8-slot SHM ring buffer
    let mut ring = ShmRingBuffer::create_or_open(&shm_path, 8, 2048)
        .expect("Failed to create SHM ring buffer");

    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;

    // Write multiple frames across the ring to test wrap-around
    for cycle in 1..=20 {
        let frame_payload = format!(
            "{{\"ambient_k\": {:.4}, \"baro_pa\": {:.2}, \"flow_lps\": {:.4}, \"timestamp_ms\": {}, \"ttl_ms\": 60000}}",
            temp_k + (cycle as f64) * 0.05,
            pressure_pa + (cycle as f64) * 1.5,
            flow_rate + (cycle as f64) * 0.1,
            now_ms + (cycle as u64) * 10
        );
        let seq = ring
            .write_telemetry(&frame_payload, (cycle as u64) * 1_000_000)
            .expect("write_telemetry failed");
        assert_eq!(seq, cycle as u64);
    }

    // 3. Test latency and payload extraction using ShmEvaluator
    let shm_eval = ShmEvaluator::new(shm_path.to_str().unwrap());
    let mut inputs = VarMap::new();
    inputs.set("static_const", 1.0);

    // Initial evaluation connects and maps the memory buffer
    let _ = shm_eval
        .evaluate(&inputs)
        .expect("ShmEvaluator initial evaluation failed");

    // Measure steady-state ingestion latency over the open memory ring
    let t0 = Instant::now();
    let res = shm_eval
        .evaluate(&inputs)
        .expect("ShmEvaluator evaluation failed");
    let read_latency_us = t0.elapsed().as_secs_f64() * 1_000_000.0;

    println!(
        "SHM Ring Buffer Ingestion Latency: {:.2} µs (Roadmap goal: < 100.0 µs)",
        read_latency_us
    );

    // Verify sub-100µs performance milestone
    assert!(
        read_latency_us < 100.0,
        "SHM ingestion took {:.2} µs, exceeding 100 µs target",
        read_latency_us
    );

    // Verify ingested values from frame 20
    let last_temp_k = temp_k + 20.0 * 0.05;
    let last_pressure_pa = pressure_pa + 20.0 * 1.5;
    let last_flow_rate = flow_rate + 20.0 * 0.1;

    let amb = res.values.get("ambient_k").copied().unwrap();
    let baro = res.values.get("baro_pa").copied().unwrap();
    let flow = res.values.get("flow_lps").copied().unwrap();

    assert!((amb - last_temp_k).abs() < 1e-3);
    assert!((baro - last_pressure_pa).abs() < 1e-2);
    assert!((flow - last_flow_rate).abs() < 1e-3);
    assert!(res.payload_sha256.is_some());

    // 4. Test Reverse Actuation through SHM
    let mut act_cmds = HashMap::new();
    let target_valve_pos = rng.next_f64(0.1, 0.9);
    act_cmds.insert("valve_pos".to_string(), target_valve_pos);
    act_cmds.insert("throttle_pct".to_string(), 75.5);

    let act_seq = shm_eval.actuate(&act_cmds).expect("actuate failed");
    assert_eq!(act_seq, 1);

    let (read_act_seq, act_json) = ring
        .read_latest_actuation()
        .expect("read_latest_actuation failed");
    assert_eq!(read_act_seq, 1);
    assert!(act_json.contains("\"valve_pos\""));
    assert!(act_json.contains("\"throttle_pct\""));

    // 5. Test End-to-End Compiler Execution with `using shm(...)`
    let model_src = format!(
        r#"
signal baro_pa : scalar
signal ambient_k : scalar
signal flow_lps : scalar
relation FlowInference using shm("{path}") {{
    gas_density = baro_pa / (287.05 * ambient_k)
    mass_flow = gas_density * (flow_lps / 1000.0)
}}

solve FlowInference with {{}}
"#,
        path = shm_path.to_str().unwrap().replace('\\', "/")
    );

    let model_path = std::env::temp_dir().join(format!("{}.aven", shm_name));
    std::fs::write(&model_path, model_src).unwrap();

    let (code, stdout, stderr) = avenc(&["run", model_path.to_str().unwrap()]);
    assert_eq!(
        code, 0,
        "avenc run failed on SHM model: stdout: {}, stderr: {}",
        stdout, stderr
    );

    println!("SHM Model stdout:\n{}", stdout);
    println!("SHM Model stderr:\n{}", stderr);
    let settled = parse_settled(&stdout);
    assert!(settled.contains_key("gas_density"));
    assert!(settled.contains_key("mass_flow"));

    let expected_density = last_pressure_pa / (287.05 * last_temp_k);
    let expected_mass_flow = expected_density * (last_flow_rate / 1000.0);

    let actual_density = settled["gas_density"];
    let actual_mass_flow = settled["mass_flow"];

    assert!((actual_density - expected_density).abs() < 1e-3);
    assert!((actual_mass_flow - expected_mass_flow).abs() < 1e-3);

    // Verify reverse actuation was updated during solve
    let (final_act_seq, final_act_json) = ring
        .read_latest_actuation()
        .expect("read_latest_actuation failed after solve");
    assert!(final_act_seq >= 1);
    assert!(final_act_json.contains("gas_density"));
    assert!(final_act_json.contains("mass_flow"));

    // Cleanup
    let _ = std::fs::remove_file(&shm_path);
    let _ = std::fs::remove_file(&model_path);
}

// ============================================================================
// TEST 2: Randomized Native Dynamic Library FFI Plugin (`using native(...)`)
// ============================================================================
#[test]
fn test_phase4_randomized_native_ffi_plugin() {
    let mut rng = EntropyRng::new();

    // 1. Generate randomized polynomial coefficients
    let coeff_a = rng.next_f64(-5.0, 5.0);
    let coeff_b = rng.next_f64(-5.0, 5.0);
    let coeff_c = rng.next_f64(10.0, 50.0);
    let test_x = rng.next_f64(1.0, 20.0);
    let test_y = rng.next_f64(1.0, 20.0);

    let plugin_name = rng.rand_ident("aven_plugin");
    let lib_stem = plugin_name.clone();
    let src_path = std::env::temp_dir().join(format!("{}.rs", lib_stem));
    let lib_ext = if cfg!(windows) { "dll" } else { "so" };
    let lib_path = std::env::temp_dir().join(format!("{}.{}", lib_stem, lib_ext));

    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;

    // 2. Generate Rust native plugin source exporting C ABI function
    let rust_code = format!(
        r#"
#![allow(unused)]
use std::ffi::c_char;

#[no_mangle]
pub unsafe extern "C" fn aven_solve_hydro(
    input_json: *const c_char,
    output_buf: *mut c_char,
    output_buf_len: usize,
) -> i32 {{
    let c_str = unsafe {{ std::ffi::CStr::from_ptr(input_json) }};
    let input = c_str.to_str().unwrap_or("");

    // Simple numeric parser for input variables
    let extract = |key: &str| -> f64 {{
        if let Some(pos) = input.find(key) {{
            let rest = &input[pos + key.len()..];
            if let Some(colon) = rest.find(':') {{
                let val_part = rest[colon + 1..]
                    .trim_start()
                    .trim_matches(|c: char| !c.is_ascii_digit() && c != '-' && c != '+' && c != '.');
                let end = val_part
                    .find(|c: char| c == ',' || c == '}}' || c.is_whitespace())
                    .unwrap_or(val_part.len());
                return val_part[..end].parse::<f64>().unwrap_or(0.0);
            }}
        }}
        0.0
    }};

    let x = extract("\"in_x\"");
    let y = extract("\"in_y\"");

    let z = ({coeff_a:.6} * x) + ({coeff_b:.6} * y) + {coeff_c:.6};

    let mut resp = String::from("{{\"out_z\": ");
    use std::fmt::Write as FmtWrite;
    let _ = write!(resp, "{{:.6}}", z);
    resp.push_str(", \"timestamp_ms\": ");
    resp.push_str("{now_ms}");
    resp.push_str(", \"ttl_ms\": 60000}}\0");

    let bytes = resp.as_bytes();
    if bytes.len() > output_buf_len {{
        return 1;
    }}

    std::ptr::copy_nonoverlapping(bytes.as_ptr() as *const c_char, output_buf, bytes.len());
    0
}}
"#
    );

    std::fs::write(&src_path, rust_code).expect("Failed to write plugin source");

    // 3. Compile dynamic library using rustc --crate-type cdylib
    let rustc_status = Command::new("rustc")
        .args([
            "--crate-type",
            "cdylib",
            "--crate-name",
            &lib_stem,
            src_path.to_str().unwrap(),
            "-o",
            lib_path.to_str().unwrap(),
            "-O",
        ])
        .output()
        .expect("Failed to compile native dynamic library with rustc");

    assert!(
        rustc_status.status.success(),
        "rustc compilation failed: {}",
        String::from_utf8_lossy(&rustc_status.stderr)
    );

    // 4. Test NativeFfiEvaluator directly
    let ffi_eval = NativeFfiEvaluator::new(lib_path.to_str().unwrap(), "aven_solve_hydro");
    let mut inputs = VarMap::new();
    inputs.set("in_x", test_x);
    inputs.set("in_y", test_y);

    // Initial evaluation dynamically loads the plugin into process memory
    let _ = ffi_eval
        .evaluate(&inputs)
        .expect("NativeFfiEvaluator initial load failed");

    // Measure steady-state zero-copy in-process evaluation time
    let t0 = Instant::now();
    let res = ffi_eval
        .evaluate(&inputs)
        .expect("NativeFfiEvaluator evaluation failed");
    let eval_time_us = t0.elapsed().as_secs_f64() * 1_000_000.0;

    println!(
        "Native In-Process FFI Execution Time: {:.2} µs (Zero Subprocess Overhead)",
        eval_time_us
    );

    let expected_z = coeff_a * test_x + coeff_b * test_y + coeff_c;
    let actual_z = res.values["out_z"];
    assert!(
        (actual_z - expected_z).abs() < 1e-4,
        "Expected z = {:.4}, got {:.4}",
        expected_z,
        actual_z
    );
    assert!(res.payload_sha256.is_some());

    // 5. Test End-to-End Compiler Execution with `using native(...)`
    let model_src = format!(
        r#"
signal out_z : scalar
signal target_margin : scalar
relation NativeHydro using native("{lib_path}", "aven_solve_hydro") {{
    out_z = target_margin * 1.0
}}

solve NativeHydro with {{
    in_x = {test_x:.4}
    in_y = {test_y:.4}
}}
"#,
        lib_path = lib_path.to_str().unwrap().replace('\\', "/")
    );

    let model_path = std::env::temp_dir().join(format!("{}.aven", plugin_name));
    std::fs::write(&model_path, model_src).unwrap();

    let (code, stdout, stderr) = avenc(&["run", model_path.to_str().unwrap()]);
    assert_eq!(
        code, 0,
        "avenc run failed on native model: stdout: {}, stderr: {}",
        stdout, stderr
    );

    let settled = parse_settled(&stdout);
    println!("Test 2 stdout:\n{}", stdout);
    println!("Test 2 settled: {:?}, expected_z: {}", settled, expected_z);
    assert!(settled.contains_key("out_z"));
    assert!(settled.contains_key("target_margin"));
    assert!((settled["target_margin"] - expected_z).abs() < 1e-3);

    // Cleanup
    let _ = std::fs::remove_file(&src_path);
    let _ = std::fs::remove_file(&lib_path);
    let _ = std::fs::remove_file(&model_path);
}

// ============================================================================
// TEST 3: Randomized Bidirectional IPC Daemon Control & Actuator Feedback
// ============================================================================
#[test]
fn test_phase4_randomized_bidirectional_ipc_daemon_control() {
    let mut rng = EntropyRng::new();

    // 1. Bind to port 0 to get an OS-assigned random free TCP port
    let listener = TcpListener::bind("127.0.0.1:0").expect("Failed to bind to free port");
    let port = listener.local_addr().unwrap().port();

    let sensor_inflow = rng.next_f64(10.0, 50.0);
    let target_tank_level = rng.next_f64(100.0, 200.0);

    let actuation_received = Arc::new(AtomicBool::new(false));
    let received_valve_cmd = Arc::new(AtomicU64::new(0));

    let act_flag = Arc::clone(&actuation_received);
    let cmd_store = Arc::clone(&received_valve_cmd);

    // 2. Spawn mock physical actuator daemon thread
    let server_handle = std::thread::spawn(move || {
        let _ = listener.set_nonblocking(false);
        // Serve up to 2 incoming connections: 1 for telemetry ingestion, 1 for actuation streaming
        for _ in 0..2 {
            if let Ok((mut stream, _)) = listener.accept() {
                let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                if reader.read_line(&mut line).is_ok() {
                    let trimmed = line.trim();
                    if trimmed.contains("\"action\": \"actuate\"") {
                        // Received reverse actuation command from Aven!
                        if let Some(pos) = trimmed.find("\"actuator_valve\"") {
                            let rest = &trimmed[pos + 16..];
                            if let Some(colon) = rest.find(':') {
                                let val_part =
                                    rest[colon + 1..].trim_start().trim_matches(|c: char| {
                                        !c.is_ascii_digit() && c != '-' && c != '+' && c != '.'
                                    });
                                let end = val_part
                                    .find(|c: char| c == ',' || c == '}' || c.is_whitespace())
                                    .unwrap_or(val_part.len());
                                if let Ok(val) = val_part[..end].parse::<f64>() {
                                    cmd_store.store(val.to_bits(), Ordering::SeqCst);
                                    act_flag.store(true, Ordering::SeqCst);
                                }
                            }
                        }
                    } else {
                        // Query for telemetry: reply with randomized sensor data
                        let now_ms = SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .unwrap()
                            .as_millis() as u64;
                        let resp = format!(
                            "{{\"inflow_rate\": {:.4}, \"tank_level\": {:.4}, \"timestamp_ms\": {}, \"ttl_ms\": 60000}}\n",
                            sensor_inflow, target_tank_level, now_ms
                        );
                        let _ = stream.write_all(resp.as_bytes());
                        let _ = stream.flush();
                    }
                }
            }
        }
    });

    // 3. Construct Aven model with bidirectional IPC relation
    let model_name = rng.rand_ident("ipc_bidirectional");
    let model_src = format!(
        r#"
signal inflow_rate : scalar
signal tank_level : scalar
relation ActuatorLoop using ipc("127.0.0.1:{port}") {{
    target_outflow = inflow_rate * 1.25
    actuator_valve = (target_outflow / tank_level) * 100.0
}}

solve ActuatorLoop with {{}}
"#
    );

    let model_path = std::env::temp_dir().join(format!("{}.aven", model_name));
    std::fs::write(&model_path, model_src).unwrap();

    // 4. Run Aven compiler to solve and stream actuation commands
    let (code, stdout, stderr) = avenc(&["run", model_path.to_str().unwrap()]);
    assert_eq!(
        code, 0,
        "avenc run failed on bidirectional IPC model: stdout: {}, stderr: {}",
        stdout, stderr
    );

    let _ = server_handle.join();

    // 5. Verify the daemon received the exact settled actuator command
    let expected_target_outflow = sensor_inflow * 1.25;
    let expected_valve = (expected_target_outflow / target_tank_level) * 100.0;

    assert!(
        actuation_received.load(Ordering::SeqCst),
        "Actuator daemon never received actuation packet from Aven"
    );

    let received_bits = received_valve_cmd.load(Ordering::SeqCst);
    let actual_valve = f64::from_bits(received_bits);

    println!(
        "Actuator Received Valve Command: {:.4} (Expected: {:.4})",
        actual_valve, expected_valve
    );
    assert!(
        (actual_valve - expected_valve).abs() < 1e-4,
        "Actuator received {:.4}, expected {:.4}",
        actual_valve,
        expected_valve
    );

    // Cleanup
    let _ = std::fs::remove_file(&model_path);
}

// ============================================================================
// TEST 4: Repeated Random Verification (Verify Again Requirement)
// ============================================================================
#[test]
fn test_phase4_verification_again_rapid_cycling() {
    let mut rng = EntropyRng::new();

    // Run 5 rapid random cycles through SHM ring buffer to guarantee no state retention
    let shm_name = rng.rand_ident("aven_cycle_shm");
    let shm_path = std::env::temp_dir().join(format!("{}.shm", shm_name));
    let mut ring =
        ShmRingBuffer::create_or_open(&shm_path, 16, 1024).expect("create_or_open failed");

    let shm_eval = ShmEvaluator::new(shm_path.to_str().unwrap());

    for cycle in 1..=5 {
        let v1 = rng.next_f64(10.0, 100.0);
        let v2 = rng.next_f64(200.0, 500.0);
        let payload = format!("{{\"val1\": {:.4}, \"val2\": {:.4}}}", v1, v2);

        ring.write_telemetry(&payload, cycle as u64)
            .expect("write_telemetry failed");

        let res = shm_eval.evaluate(&VarMap::new()).expect("eval failed");

        let actual_v1 = res.values.get("val1").copied().unwrap();
        let actual_v2 = res.values.get("val2").copied().unwrap();
        assert!((actual_v1 - v1).abs() < 1e-3);
        assert!((actual_v2 - v2).abs() < 1e-3);
        assert!(res.solve_time_us < 100.0);
    }

    let _ = std::fs::remove_file(&shm_path);
}
