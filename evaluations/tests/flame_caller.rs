use std::io::Write;
use std::process::{Command, Stdio};
use std::time::Duration;

fn main() {
    let cases = [
        r#"{"x": 0.5, "y": -0.2}"#,
        r#"{"x": 1.0}"#,            // missing y
        r#"{"x": 0.3, "y": -0.7}"#, // spike
        r#"{"x": 2.0, "y": 0.0}"#,  // wall
    ];

    for (i, payload) in cases.iter().enumerate() {
        let mut child = Command::new("python")
            .arg("flame_eval.py")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("failed to spawn");

        child
            .stdin
            .take()
            .unwrap()
            .write_all(payload.as_bytes())
            .unwrap();

        let output = child.wait_with_output().expect("failed to wait");

        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            println!("[{}] OK   {}", i, stdout.trim());
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            println!("[{}] FAIL {}", i, stderr.trim());
        }
    }
}
