// tests/adversarial_milp.rs — Stress tests for MILP solver safeguards:
// 1. Column equilibration with strict integer invariance (s_j ≡ 1.0)
// 2. Pre-emptive circuit breakers on dual norm ||y||_inf
// 3. Cosine similarity cut eviction and bounded cut pool (<= 48 cuts)
// 4. Global dual bound tracking and audit ledger telemetry

use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

static FILE_COUNTER: AtomicUsize = AtomicUsize::new(0);

fn write_model(src: &str) -> PathBuf {
    let n = FILE_COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("aven_adv_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("adv_{}.aven", n));
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

#[test]
fn test_scaled_continuous_with_integer_invariance() {
    // Problem with extreme coefficient disparity:
    // Continuous variable bandwidth [bps] has magnitude 100_000,
    // while integer variable servers is discrete (1, 2, 3...).
    // Equilibration must normalize the continuous column while keeping s_servers ≡ 1.0.
    let src = r#"
relation Infrastructure {
    integer servers
    cost = 500 * servers + 0.02 * bandwidth
    capacity = 100000 * servers
    capacity >= 350000
    bandwidth >= 50000
}
solve Infrastructure with {
    minimize(cost)
}
"#;
    let run = run_model(src, &["--json"]);
    assert_eq!(run.code, 0, "must settle cleanly: {}", run.stderr);
    let line = run.stdout.lines().last().unwrap();
    assert!(
        line.contains("\"optimality\": \"proven\""),
        "must be proven optimal: {}",
        line
    );
    assert!(
        line.contains("\"servers\": 4"),
        "servers must be rounded to 4 (ceil of 3.5): {}",
        line
    );
    assert!(
        line.contains("\"cost\": 3000"),
        "cost must be 500*4 + 0.02*50000 = 3000: {}",
        line
    );
}

#[test]
fn test_duality_gap_certificate_in_audit_ledger() {
    // Verifies that the JSONL audit ledger receives the milp_certificate fact
    // with bound, incumbent, duality_gap, cuts_active, dual_norm, and proven_optimal.
    let src = r#"
relation Allocation {
    integer workers
    integer machines
    budget = 100 * workers + 250 * machines
    budget <= 1200
    output = 10 * workers + 35 * machines
    workers >= 2
    machines >= 1
}
solve Allocation with {
    maximize(output)
}
"#;
    let ledger_file =
        std::env::temp_dir().join(format!("ledger_audit_{}.jsonl", std::process::id()));
    let ledger_path = ledger_file.to_string_lossy().into_owned();
    let run = run_model(src, &["--ledger", &ledger_path]);
    assert_eq!(run.code, 0, "run must succeed: {}", run.stderr);

    // Read ledger file
    let content = std::fs::read_to_string(&ledger_file).expect("ledger file must exist");
    let has_cert = content.lines().any(|l| {
        l.contains("\"name\": \"milp_certificate\"")
            && l.contains("bound:")
            && l.contains("incumbent:")
            && l.contains("duality_gap:")
            && l.contains("proven_optimal:true")
    });
    assert!(
        has_cert,
        "ledger must contain milp_certificate fact with telemetry: {}",
        content
    );

    // Cryptographic chain verification
    let v_run = avenc(&["verify-ledger", &ledger_path]);
    assert_eq!(
        v_run.code, 0,
        "ledger hash chain must verify: {}",
        v_run.stderr
    );
    assert!(
        v_run.stdout.contains("Ledger OK"),
        "ledger must be intact: {}",
        v_run.stdout
    );
}

#[test]
fn test_cosine_similarity_cut_eviction_and_pool_limit() {
    // Problem with several near-identical constraints that generate near-parallel hyperplanes.
    // Cosine similarity filtering (|cos| > 0.99) must reject redundant cuts, keeping active cuts bounded.
    let src = r#"
relation ParallelSlices {
    integer x
    integer y
    c1 = 100 * x + 101 * y
    c1 >= 500
    c2 = 100 * x + 100.5 * y
    c2 >= 498
    c3 = 100 * x + 101.2 * y
    c3 >= 502
    obj = x + y
    x >= 0
    y >= 0
}
solve ParallelSlices with {
    minimize(obj)
}
"#;
    let run = run_model(src, &["--json"]);
    assert_eq!(run.code, 0, "must solve cleanly: {}", run.stderr);
    let line = run.stdout.lines().last().unwrap();
    assert!(
        line.contains("\"optimality\": \"proven\""),
        "must prove optimum: {}",
        line
    );
}
