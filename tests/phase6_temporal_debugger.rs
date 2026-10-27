// tests/phase6_temporal_debugger.rs -- Milestone Phase 6 Test Suite
// Rigorous verification of Temporal Signal Calculus, Segmented Ledger Compaction & Cryptographic Chaining,
// and Interactive Time-Travel Debugger with Counterfactual Branch Diffing.
// Includes minimum 3 randomized tests using OS entropy + PID so data is never recycled.

use avenc::ast::AvenModule;
use avenc::compaction::{
    LedgerCompactor, SegmentConfig, SegmentedLedgerWriter, verify_segmented_directory,
};
use avenc::debugger::DebuggerSession;
use avenc::ledger::{Fact, FactMetadata, Ledger};
use avenc::lexer::Lexer;
use avenc::parser::Parser;
use avenc::temporal::{
    RetentionWindow, StalenessDecay, TemporalSample, TemporalSupervisor, align_signals,
};
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn parse_module_from_str(src: &str, name: &str) -> AvenModule {
    let tokens = Lexer::new(src).tokenize().expect("tokenize");
    Parser::new(tokens).parse_module(name).expect("parse")
}

// OS entropy + PID PRNG
struct Phase6Prng(u64);

impl Phase6Prng {
    fn from_entropy() -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let pid = std::process::id() as u128;
        let seed = ((nanos ^ (pid << 32)) as u64) | 0xDEADBEEF01234567;
        Phase6Prng(seed)
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    fn next_f64_range(&mut self, min: f64, max: f64) -> f64 {
        let r = (self.next_u64() % 1_000_000) as f64 / 1_000_000.0;
        min + r * (max - min)
    }

    fn next_u64_range(&mut self, min: u64, max: u64) -> u64 {
        if min >= max {
            return min;
        }
        min + (self.next_u64() % (max - min + 1))
    }
}

fn create_temp_dir(prefix: &str) -> PathBuf {
    let mut prng = Phase6Prng::from_entropy();
    let rand_suffix = prng.next_u64();
    let pid = std::process::id();
    let dir = std::env::temp_dir().join(format!("{}_{}_{}", prefix, pid, rand_suffix));
    fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

// ---------------------------------------------------------------------------
// Test 1: Golden Temporal Signal Calculus
// ---------------------------------------------------------------------------
#[test]
fn test_temporal_signal_calculus_golden() {
    let mut window = RetentionWindow::new(Some(5000), 10);

    // Push 5 samples at 1-second intervals with slope 10.0 units/sec
    for i in 1..=5 {
        let t = i * 1000;
        let val = (i as f64) * 10.0;
        window.push(TemporalSample::new(t, val, Some(2000)));
    }

    assert_eq!(window.len(), 5);
    let roc = window.rate_of_change().expect("rate of change");
    assert!(
        (roc - 10.0).abs() < 1e-6,
        "slope should be 10.0, got {}",
        roc
    );

    let avg = window.moving_average().expect("moving average");
    assert!(
        (avg - 30.0).abs() < 1e-6,
        "average should be 30.0, got {}",
        avg
    );

    let ema = window.exponential_smoothing(0.5).expect("ema");
    assert!(ema > 10.0 && ema < 50.0);

    let var = window.variance().expect("variance");
    assert!(var > 0.0);
    let sd = window.std_dev().expect("std dev");
    assert!((sd - var.sqrt()).abs() < 1e-9);

    // Eviction test: push sample at t = 7000 (span = 7000 - 1000 = 6000 > 5000ms max_duration)
    window.push(TemporalSample::new(7000, 70.0, Some(2000)));
    assert!(window.oldest().unwrap().timestamp_ms >= 2000);

    // Staleness decay models
    let step = StalenessDecay::Step;
    assert_eq!(step.confidence(500, 1000), 1.0);
    assert_eq!(step.confidence(1500, 1000), 0.0);

    let linear = StalenessDecay::Linear;
    assert_eq!(linear.confidence(0, 1000), 1.0);
    assert!((linear.confidence(500, 1000) - 0.5).abs() < 1e-6);
    assert_eq!(linear.confidence(1200, 1000), 0.0);

    let exp = StalenessDecay::Exponential {
        half_life_ms: 500.0,
    };
    assert!((exp.confidence(500, 1000) - 0.5).abs() < 1e-3);

    // Cross-signal alignment
    let stream_a = vec![
        TemporalSample::new(100, 1.0, None),
        TemporalSample::new(200, 2.0, None),
        TemporalSample::new(300, 3.0, None),
    ];
    let stream_b = vec![
        TemporalSample::new(105, 10.0, None),
        TemporalSample::new(195, 20.0, None),
        TemporalSample::new(310, 30.0, None),
    ];
    let aligned = align_signals(&stream_a, &stream_b, 10);
    assert_eq!(aligned.len(), 3);
    for pair in &aligned {
        assert!(pair.within_tolerance);
        assert!(pair.skew_ms.abs() <= 10);
    }

    // Supervisor actionability gate
    let mut sup = TemporalSupervisor::new(0.6, StalenessDecay::Linear);
    sup.register_signal("temperature", Some(10000), 50);
    sup.ingest("temperature", TemporalSample::new(1000, 25.0, Some(1000)));

    assert!(sup.is_actionable("temperature", 1200)); // age 200ms -> confidence 0.8 >= 0.6
    assert!(!sup.is_actionable("temperature", 1600)); // age 600ms -> confidence 0.4 < 0.6
}

// ---------------------------------------------------------------------------
// Test 2: Golden Segmented Ledger Rotation, Cross-Segment Chaining & Compaction
// ---------------------------------------------------------------------------
#[test]
fn test_segmented_ledger_compaction_golden() {
    let dir = create_temp_dir("phase6_seg_ledger");
    let key = b"phase6_super_secret_audit_key_32";

    let config = SegmentConfig {
        segment_dir: dir.clone(),
        max_facts_per_segment: 3,
        key: Some(key.to_vec()),
    };

    let mut writer = SegmentedLedgerWriter::new(config).expect("writer init");

    // Append 8 facts across segments
    for i in 0..8 {
        let fact = Fact {
            seq: i,
            tick: i / 2,
            kind: if i % 3 == 0 {
                "input".to_string()
            } else {
                "derived".to_string()
            },
            name: format!("var_{}", i),
            value: (i as f64) * 1.5,
            causes: if i > 0 { vec![i - 1] } else { vec![] },
            origin: "test_segmented".to_string(),
            metadata: Some(FactMetadata {
                timestamp_ms: Some(1000 + (i as u64) * 100),
                ttl_ms: Some(5000),
                is_stale: false,
                warnings: vec![],
            }),
        };
        writer.append(fact).expect("append fact");
    }

    writer.close().expect("close writer");

    // Verify segments were written: 3 facts per segment -> 8 facts = 3 segments (3, 3, 2)
    let s0 = dir.join("segment_0000.jsonl");
    let s1 = dir.join("segment_0001.jsonl");
    let s2 = dir.join("segment_0002.jsonl");
    assert!(s0.exists(), "segment 0 exists");
    assert!(s1.exists(), "segment 1 exists");
    assert!(s2.exists(), "segment 2 exists");

    // Cross-segment chained verification
    let audit = verify_segmented_directory(&dir, Some(key)).expect("audit verification");
    assert_eq!(audit.total_segments, 3);
    assert_eq!(audit.total_facts, 8);
    assert!(audit.signed);
    assert_eq!(audit.head, writer.current_head());

    // Compaction test
    let mut full_ledger = Ledger::new();
    for i in 0..8 {
        full_ledger.append(Fact {
            seq: i,
            tick: i / 2,
            kind: if i == 7 {
                "committed-effect".to_string()
            } else if i % 2 == 0 {
                "input".to_string()
            } else {
                "derived".to_string()
            },
            name: format!("v_{}", i),
            value: i as f64,
            causes: vec![],
            origin: "orig".to_string(),
            metadata: None,
        });
    }

    let compacted = LedgerCompactor::compact_ledger(&full_ledger);
    assert!(compacted.len() <= full_ledger.len());
    // All inputs and committed effects are preserved
    for f in full_ledger.facts() {
        if f.kind == "input" || f.kind == "committed-effect" {
            assert!(compacted.get(f.seq).is_some());
        }
    }

    let _ = fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// Test 3: Golden Time-Travel Debugger & Counterfactual What-If
// ---------------------------------------------------------------------------
#[test]
fn test_debugger_session_what_if_golden() {
    let src = fs::read_to_string("examples/server_capacity.aven").expect("read server_capacity");
    let module = parse_module_from_str(&src, "server_capacity");

    let mut ledger = Ledger::new();
    let inputs = [
        ("reqs_per_second", 20000.0),
        ("avg_latency_ms", 20.0),
        ("avg_payload_kb", 64.0),
        ("ram_per_worker", 16.0),
        ("socket_buffer_mb", 4.0),
        ("os_kernel_ram_mb", 4096.0),
        ("target_utilization", 0.70),
        ("framing_overhead_ratio", 1.04),
    ];
    for (i, (name, val)) in inputs.iter().enumerate() {
        ledger.append(Fact {
            seq: i,
            tick: 0,
            kind: "input".to_string(),
            name: name.to_string(),
            value: *val,
            causes: vec![],
            origin: "test_input".to_string(),
            metadata: None,
        });
    }
    // Derived raw_workers at tick 0
    let rw_seq0 = inputs.len();
    ledger.append(Fact {
        seq: rw_seq0,
        tick: 0,
        kind: "derived".to_string(),
        name: "raw_workers".to_string(),
        value: 400.0,
        causes: vec![0, 1],
        origin: "eval".to_string(),
        metadata: None,
    });

    // Simulate tick 1: reqs_per_second increases to 40000.0
    let req_seq1 = rw_seq0 + 1;
    ledger.append(Fact {
        seq: req_seq1,
        tick: 1,
        kind: "input".to_string(),
        name: "reqs_per_second".to_string(),
        value: 40000.0,
        causes: vec![],
        origin: "test_input".to_string(),
        metadata: None,
    });
    let rw_seq1 = req_seq1 + 1;
    ledger.append(Fact {
        seq: rw_seq1,
        tick: 1,
        kind: "derived".to_string(),
        name: "raw_workers".to_string(),
        value: 800.0,
        causes: vec![req_seq1, 1],
        origin: "eval".to_string(),
        metadata: None,
    });

    let mut session = DebuggerSession::new("examples/server_capacity.aven", module, ledger);
    assert_eq!(session.current_tick, 1);
    assert_eq!(session.max_tick, 1);

    // Scrub timeline to tick 0
    let st0 = session.goto_tick(0, false).expect("goto 0");
    assert_eq!(st0.tick, 0);
    assert_eq!(st0.vars.get("reqs_per_second").unwrap().value, 20000.0);
    assert_eq!(st0.vars.get("raw_workers").unwrap().value, 400.0);

    // Step forward to tick 1
    let st1 = session.step_forward(1).expect("step 1");
    assert_eq!(st1.tick, 1);
    assert_eq!(st1.vars.get("reqs_per_second").unwrap().value, 40000.0);
    assert_eq!(st1.vars.get("raw_workers").unwrap().value, 800.0);

    // Diff ticks
    let diffs = session.diff_ticks(0, 1);
    let req_diff = diffs
        .iter()
        .find(|d| d.name == "reqs_per_second")
        .expect("req diff");
    assert_eq!(req_diff.old_value, Some(20000.0));
    assert_eq!(req_diff.new_value, Some(40000.0));
    assert_eq!(req_diff.delta, Some(20000.0));

    // Causal why
    let why_workers = session.why("raw_workers", None);
    assert!(why_workers.contains("raw_workers"));

    // Counterfactual what-if branch: mutate reqs_per_second to 60000
    let what_if = session
        .what_if(&[("reqs_per_second".to_string(), 60000.0)])
        .expect("what if solve");
    assert_eq!(what_if.baseline_tick, 1);
    assert_eq!(
        what_if.mutated_inputs.get("reqs_per_second"),
        Some(&60000.0)
    );

    // Counterfactual raw_workers should be (60000 * 20) / 1000 = 1200.0
    let rw_diff = what_if
        .diffs
        .iter()
        .find(|d| d.name == "raw_workers")
        .expect("rw diff");
    assert_eq!(rw_diff.old_value, Some(800.0));
    assert_eq!(rw_diff.new_value, Some(1200.0));
    assert_eq!(rw_diff.delta, Some(400.0));

    // Counterfactual allocated_workers should be ceil(1200 / 0.70) = 1715.0
    let alloc_diff = what_if
        .diffs
        .iter()
        .find(|d| d.name == "allocated_workers")
        .expect("alloc diff");
    assert_eq!(alloc_diff.new_value, Some(1715.0));
}

// ---------------------------------------------------------------------------
// Test 4: Irreversible Rewind Boundary Guard
// ---------------------------------------------------------------------------
#[test]
fn test_irreversible_rewind_guard() {
    let src = fs::read_to_string("examples/server_capacity.aven").expect("read server_capacity");
    let module = parse_module_from_str(&src, "server_capacity");

    let mut ledger = Ledger::new();
    ledger.append(Fact {
        seq: 0,
        tick: 0,
        kind: "input".to_string(),
        name: "sensor".to_string(),
        value: 10.0,
        causes: vec![],
        origin: "hw".to_string(),
        metadata: None,
    });
    // Committed effect at tick 1 (e.g. Modbus actuator fire)
    ledger.append(Fact {
        seq: 1,
        tick: 1,
        kind: "committed-effect".to_string(),
        name: "actuator_fired".to_string(),
        value: 1.0,
        causes: vec![0],
        origin: "hw_out".to_string(),
        metadata: None,
    });
    // Subsequent derived fact at tick 2
    ledger.append(Fact {
        seq: 2,
        tick: 2,
        kind: "derived".to_string(),
        name: "sensor_post".to_string(),
        value: 15.0,
        causes: vec![1],
        origin: "hw".to_string(),
        metadata: None,
    });

    let mut session = DebuggerSession::new("examples/server_capacity.aven", module, ledger);
    assert_eq!(session.current_tick, 2);

    // Attempt rewind to tick 0 without override -> MUST fail due to committed effect at tick 1
    let err = session.goto_tick(0, false);
    assert!(
        err.is_err(),
        "rewind crossing committed effect without allow_irreversible must fail"
    );
    let msg = err.unwrap_err();
    assert!(msg.contains("committed") || msg.contains("irreversible"));

    // Attempt rewind to tick 0 WITH override -> succeeds
    let ok = session.goto_tick(0, true);
    assert!(
        ok.is_ok(),
        "rewind with allow_irreversible override must succeed"
    );
    assert_eq!(session.current_tick, 0);
}

// ---------------------------------------------------------------------------
// Test 5 (Randomized 1): Temporal Streams, Monotonic Decay & Bounded FIFO
// ---------------------------------------------------------------------------
#[test]
fn test_randomized_1_temporal_streams() {
    let mut prng = Phase6Prng::from_entropy();
    let num_samples = prng.next_u64_range(25, 60) as usize;
    let max_samples = prng.next_u64_range(5, 15) as usize;
    let window_duration = prng.next_u64_range(5000, 20000);

    let mut window = RetentionWindow::new(Some(window_duration), max_samples);

    let slope = prng.next_f64_range(-5.0, 15.0);
    let intercept = prng.next_f64_range(10.0, 100.0);

    let mut current_t = 1000_u64;
    for _ in 0..num_samples {
        let dt = prng.next_u64_range(100, 800);
        current_t += dt;
        let t_sec = (current_t as f64) / 1000.0;
        let val = slope * t_sec + intercept;
        let ttl = prng.next_u64_range(500, 3000);

        window.push(TemporalSample::new(current_t, val, Some(ttl)));

        // Invariant: window capacity strictly respected
        assert!(window.len() <= max_samples);
        if let Some(oldest) = window.oldest() {
            assert!(current_t - oldest.timestamp_ms <= window_duration + dt);
        }
    }

    // Invariant: rate of change on linear trend matches slope within precision
    if window.len() >= 2 {
        let est_roc = window.rate_of_change().expect("rate of change");
        assert!(
            (est_roc - slope).abs() < 1e-4,
            "estimated slope {} should match true slope {}",
            est_roc,
            slope
        );
    }

    // Invariant: exponential and linear decay monotonicity
    let decay_exp = StalenessDecay::Exponential {
        half_life_ms: 1000.0,
    };
    let decay_lin = StalenessDecay::Linear;
    let ttl = 2000;

    for _ in 0..20 {
        let t1 = prng.next_u64_range(0, 3000);
        let t2 = t1 + prng.next_u64_range(1, 1000);

        let c1_exp = decay_exp.confidence(t1, ttl);
        let c2_exp = decay_exp.confidence(t2, ttl);
        assert!(c1_exp >= c2_exp, "monotonic exponential decay violated");

        let c1_lin = decay_lin.confidence(t1, ttl);
        let c2_lin = decay_lin.confidence(t2, ttl);
        assert!(c1_lin >= c2_lin, "monotonic linear decay violated");
    }
}

// ---------------------------------------------------------------------------
// Test 6 (Randomized 2): Segmented Rotation & Tamper Detection
// ---------------------------------------------------------------------------
#[test]
fn test_randomized_2_segmented_rotation_and_tamper_detection() {
    let mut prng = Phase6Prng::from_entropy();
    let dir = create_temp_dir("phase6_rand_rot");

    let mut key = vec![0u8; 32];
    for b in key.iter_mut() {
        *b = (prng.next_u64() % 256) as u8;
    }

    let max_facts_per_seg = prng.next_u64_range(3, 6) as usize;
    let total_facts = prng.next_u64_range(12, 25) as usize;

    let config = SegmentConfig {
        segment_dir: dir.clone(),
        max_facts_per_segment: max_facts_per_seg,
        key: Some(key.clone()),
    };

    let mut writer = SegmentedLedgerWriter::new(config).expect("writer init");

    for i in 0..total_facts {
        let val = prng.next_f64_range(-100.0, 500.0);
        let fact = Fact {
            seq: i,
            tick: i / 3,
            kind: if i % 4 == 0 {
                "input".to_string()
            } else {
                "derived".to_string()
            },
            name: format!("rand_var_{}", i),
            value: val,
            causes: if i > 0 { vec![i - 1] } else { vec![] },
            origin: "rand_test".to_string(),
            metadata: None,
        };
        writer.append(fact).expect("append");
    }
    writer.close().expect("close writer");

    // Unmodified directory must verify 100% cleanly
    let audit = verify_segmented_directory(&dir, Some(&key)).expect("valid audit");
    assert_eq!(audit.total_facts, total_facts);
    assert!(audit.signed);

    // Tamper test: Pick a segment file, read it, flip 1 byte, write back
    let mut seg_files: Vec<PathBuf> = fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok().map(|ent| ent.path()))
        .filter(|p| p.to_string_lossy().contains("segment_"))
        .collect();
    seg_files.sort();
    assert!(!seg_files.is_empty());

    let target_file = &seg_files[prng.next_u64_range(0, (seg_files.len() - 1) as u64) as usize];
    let original_bytes = fs::read(target_file).expect("read target segment");

    // Mutate a character in the middle
    let mut tampered_bytes = original_bytes.clone();
    let tamper_idx = prng.next_u64_range(10, (original_bytes.len() - 10) as u64) as usize;
    tampered_bytes[tamper_idx] ^= 0x42;
    fs::write(target_file, &tampered_bytes).expect("write tampered file");

    // Audit MUST catch the tamper and fail
    let tampered_res = verify_segmented_directory(&dir, Some(&key));
    assert!(
        tampered_res.is_err(),
        "verify_segmented_directory must detect modified byte in {:?}",
        target_file
    );

    // Restore original file
    fs::write(target_file, &original_bytes).expect("restore file");
    assert!(verify_segmented_directory(&dir, Some(&key)).is_ok());

    let _ = fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// Test 7 (Randomized 3): Counterfactual Branch Diffing
// ---------------------------------------------------------------------------
#[test]
fn test_randomized_3_counterfactual_branch_diffing() {
    let mut prng = Phase6Prng::from_entropy();
    let src = fs::read_to_string("examples/server_capacity.aven").expect("read server_capacity");
    let module = parse_module_from_str(&src, "server_capacity");

    for _ in 0..5 {
        let baseline_reqs = prng.next_f64_range(10000.0, 50000.0);
        let latency = prng.next_f64_range(5.0, 50.0);
        let payload = prng.next_f64_range(10.0, 128.0);
        let util = prng.next_f64_range(0.5, 0.9);

        let mut ledger = Ledger::new();
        let inputs = [
            ("reqs_per_second", baseline_reqs),
            ("avg_latency_ms", latency),
            ("avg_payload_kb", payload),
            ("ram_per_worker", 16.0),
            ("socket_buffer_mb", 4.0),
            ("os_kernel_ram_mb", 4096.0),
            ("target_utilization", util),
            ("framing_overhead_ratio", 1.04),
        ];
        for (i, (name, val)) in inputs.iter().enumerate() {
            ledger.append(Fact {
                seq: i,
                tick: 0,
                kind: "input".to_string(),
                name: name.to_string(),
                value: *val,
                causes: vec![],
                origin: "input".to_string(),
                metadata: None,
            });
        }

        let baseline_raw = (baseline_reqs * latency) / 1000.0;
        ledger.append(Fact {
            seq: inputs.len(),
            tick: 0,
            kind: "derived".to_string(),
            name: "raw_workers".to_string(),
            value: baseline_raw,
            causes: vec![0, 1],
            origin: "eval".to_string(),
            metadata: None,
        });

        let session = DebuggerSession::new("examples/server_capacity.aven", module.clone(), ledger);

        // Counterfactual mutation
        let mutated_reqs = prng.next_f64_range(60000.0, 120000.0);
        let what_if = session
            .what_if(&[("reqs_per_second".to_string(), mutated_reqs)])
            .expect("what if branch");

        for d in &what_if.diffs {
            if let (Some(old), Some(new), Some(delta)) = (d.old_value, d.new_value, d.delta) {
                assert!(
                    (delta - (new - old)).abs() < 1e-6,
                    "delta invariant violated: delta={}, new-old={}",
                    delta,
                    new - old
                );
            }
        }

        // Verify raw_workers output matches new value
        let expected_new_raw = (mutated_reqs * latency) / 1000.0;
        let rw_diff = what_if
            .diffs
            .iter()
            .find(|d| d.name == "raw_workers")
            .expect("raw_workers diff");
        assert!(
            (rw_diff.new_value.unwrap() - expected_new_raw).abs() < 1e-4,
            "counterfactual raw_workers {} != expected {}",
            rw_diff.new_value.unwrap(),
            expected_new_raw
        );
    }
}

// ---------------------------------------------------------------------------
// Test 8: CLI Debug & Compaction Subprocesses
// ---------------------------------------------------------------------------
#[test]
fn test_cli_debug_and_compaction_subprocesses() {
    let cargo = env!("CARGO_BIN_EXE_avenc");

    // 1. Test avenc debug CLI non-interactive what-if branch
    let output = Command::new(cargo)
        .args([
            "debug",
            "examples/server_capacity.aven",
            "--what-if",
            "reqs_per_second=60000",
        ])
        .output()
        .expect("run avenc debug");

    assert!(
        output.status.success(),
        "avenc debug command should succeed"
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Counterfactual What-If Result") && stdout.contains("allocated_workers")
    );

    // 2. Test avenc compact-ledger CLI
    let dir = create_temp_dir("cli_compact_test");
    let in_log = dir.join("input.jsonl");
    let out_log = dir.join("compacted.jsonl");

    let mut l = Ledger::new();
    l.append(Fact {
        seq: 0,
        tick: 0,
        kind: "input".to_string(),
        name: "test_in".to_string(),
        value: 123.45,
        causes: vec![],
        origin: "cli_test".to_string(),
        metadata: None,
    });
    l.append(Fact {
        seq: 1,
        tick: 0,
        kind: "derived".to_string(),
        name: "test_out".to_string(),
        value: 246.90,
        causes: vec![0],
        origin: "cli_test".to_string(),
        metadata: None,
    });
    fs::write(&in_log, l.to_jsonl()).expect("write in_log");

    let compact_output = Command::new(cargo)
        .args([
            "compact-ledger",
            in_log.to_str().unwrap(),
            out_log.to_str().unwrap(),
        ])
        .output()
        .expect("run avenc compact-ledger");

    assert!(
        compact_output.status.success(),
        "avenc compact-ledger should succeed"
    );
    assert!(out_log.exists());
    let compacted_content = fs::read_to_string(&out_log).expect("read compacted");
    assert!(compacted_content.contains("test_in"));

    let _ = fs::remove_dir_all(&dir);
}
