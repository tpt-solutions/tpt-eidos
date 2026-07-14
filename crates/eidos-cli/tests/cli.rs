//! End-to-end CLI tests driving the `eidos` binary.

use std::path::PathBuf;
use std::process::Command;

fn example(name: &str) -> PathBuf {
    let dir = env!("CARGO_MANIFEST_DIR");
    PathBuf::from(format!("{dir}/../../examples/{name}"))
}

#[test]
fn cli_check_accepts_correct() {
    let out = Command::new(env!("CARGO_BIN_EXE_eidos"))
        .arg("check")
        .arg(example("calibrate_gyro.eidos"))
        .output()
        .expect("run eidos");
    assert!(out.status.success(), "stdout: {}", String::from_utf8_lossy(&out.stdout));
}

#[test]
fn cli_check_rejects_broken() {
    let out = Command::new(env!("CARGO_BIN_EXE_eidos"))
        .arg("check")
        .arg(example("calibrate_gyro_broken.eidos"))
        .output()
        .expect("run eidos");
    assert!(!out.status.success(), "broken example must fail verification");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("division by zero"), "stderr: {stderr}");
}
