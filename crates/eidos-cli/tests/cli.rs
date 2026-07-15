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

#[test]
fn cli_no_args_prints_usage() {
    let out = Command::new(env!("CARGO_BIN_EXE_eidos"))
        .output()
        .expect("run eidos");
    assert!(!out.status.success(), "no args must fail");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("usage"), "stderr: {stderr}");
}

#[test]
fn cli_unknown_subcommand() {
    let out = Command::new(env!("CARGO_BIN_EXE_eidos"))
        .arg("frobnicate")
        .output()
        .expect("run eidos");
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("unknown subcommand"), "stderr: {stderr}");
}

#[test]
fn cli_check_missing_file_path() {
    let out = Command::new(env!("CARGO_BIN_EXE_eidos"))
        .arg("check")
        .output()
        .expect("run eidos");
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("check requires a file path"),
        "stderr: {stderr}"
    );
}

#[test]
fn cli_build_missing_out_dir() {
    let out = Command::new(env!("CARGO_BIN_EXE_eidos"))
        .arg("build")
        .arg(example("calibrate_gyro.eidos"))
        .output()
        .expect("run eidos");
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("build requires --out-dir"),
        "stderr: {stderr}"
    );
}

#[test]
fn cli_build_refuses_nonempty_out_dir_without_force() {
    let out_dir = std::env::temp_dir().join("eidos_cli_build_nonempty");
    let _ = std::fs::remove_dir_all(&out_dir);
    std::fs::create_dir_all(&out_dir).expect("create dir");
    std::fs::write(out_dir.join("preexisting.txt"), b"hi").expect("seed file");
    let out = Command::new(env!("CARGO_BIN_EXE_eidos"))
        .arg("build")
        .arg(example("calibrate_gyro.eidos"))
        .args(["--out-dir", &out_dir.to_string_lossy()])
        .output()
        .expect("run eidos");
    assert!(
        !out.status.success(),
        "must refuse to clobber non-empty dir"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("not empty"), "stderr: {stderr}");
    let _ = std::fs::remove_dir_all(&out_dir);
}
