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
    assert!(
        out.status.success(),
        "stdout: {}",
        String::from_utf8_lossy(&out.stdout)
    );
}

#[test]
fn cli_check_rejects_broken() {
    let out = Command::new(env!("CARGO_BIN_EXE_eidos"))
        .arg("check")
        .arg(example("calibrate_gyro_broken.eidos"))
        .output()
        .expect("run eidos");
    assert!(
        !out.status.success(),
        "broken example must fail verification"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("division by zero"), "stderr: {stderr}");
}

#[test]
fn cli_build_emits_crate() {
    let out_dir = std::env::temp_dir().join("eidos_cli_build_test");
    let _ = std::fs::remove_dir_all(&out_dir);
    let out = Command::new(env!("CARGO_BIN_EXE_eidos"))
        .arg("build")
        .arg(example("calibrate_gyro.eidos"))
        .args(["--out-dir", &out_dir.to_string_lossy()])
        .output()
        .expect("run eidos build");
    assert!(
        out.status.success(),
        "build should succeed for a verified example; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out_dir.join("lib.rs").exists(), "lib.rs not emitted");
    assert!(
        out_dir.join("Cargo.toml").exists(),
        "Cargo.toml not emitted"
    );
}
