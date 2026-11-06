use std::process::Command;

fn newton() -> &'static str {
    "signal force : N\nsignal mass : kg\nsignal acceleration : m/s^2\nrelation Newton {\n force = mass * acceleration\n}\nsolve Newton with {\n force = 4200\n acceleration = 5\n}\n"
}

#[test]
fn cli_verify_report_accepts_a_real_file_and_rejects_a_swap() {
    let html = avenc::api::report_html(newton()).expect("report");
    let path = std::env::temp_dir().join(format!("aven-verify-{}.html", std::process::id()));
    std::fs::write(&path, &html).unwrap();
    let exe = env!("CARGO_BIN_EXE_avenc");
    let ok = Command::new(exe)
        .args(["verify-report", path.to_str().unwrap()])
        .output()
        .expect("run");
    assert!(
        ok.status.success(),
        "stdout {}\nstderr {}",
        String::from_utf8_lossy(&ok.stdout),
        String::from_utf8_lossy(&ok.stderr)
    );
    let text = String::from_utf8_lossy(&ok.stdout);
    assert!(text.contains("Report OK"), "{text}");
    assert!(text.contains("source sha256"), "{text}");

    let swapped = html.replacen(
        "force = mass * acceleration",
        "force = mass + acceleration",
        1,
    );
    let bad_path =
        std::env::temp_dir().join(format!("aven-verify-bad-{}.html", std::process::id()));
    std::fs::write(&bad_path, swapped).unwrap();
    let bad = Command::new(exe)
        .args(["verify-report", bad_path.to_str().unwrap()])
        .output()
        .expect("run");
    assert!(!bad.status.success(), "a swapped source must fail");
    let err = String::from_utf8_lossy(&bad.stderr);
    assert!(err.contains("does not match the ledger"), "{err}");
    std::fs::remove_file(&path).ok();
    std::fs::remove_file(&bad_path).ok();
}
