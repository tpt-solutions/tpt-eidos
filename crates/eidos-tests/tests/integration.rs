//! Integration tests over the worked examples in `examples/`.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

use eidos_codegen::codegen;
use eidos_erasure::erase;
use eidos_kernel::{check, ObligationStatus};
use eidos_parser::parse;

fn example_path(name: &str) -> PathBuf {
    let dir = env!("CARGO_MANIFEST_DIR");
    PathBuf::from(format!("{dir}/../../examples/{name}"))
}

fn check_file(name: &str) -> eidos_kernel::Report {
    let src = fs::read_to_string(example_path(name)).unwrap_or_else(|e| panic!("read {name}: {e}"));
    let module = parse(&src).unwrap_or_else(|e| panic!("parse {name}: {e}"));
    check(&module)
}

/// Erase + codegen a verified example to a `no_std` Rust source string.
fn codegen_file(name: &str) -> String {
    let src = fs::read_to_string(example_path(name)).unwrap_or_else(|e| panic!("read {name}: {e}"));
    let module = parse(&src).unwrap_or_else(|e| panic!("parse {name}: {e}"));
    let report = check(&module);
    assert!(
        report.ok(),
        "{name} should verify, errors: {:?}",
        report.errors
    );
    let core = erase(&module);
    codegen(&core)
}

#[test]
fn calibrate_gyro_verifies() {
    let report = check_file("calibrate_gyro.eidos");
    assert!(
        report.ok(),
        "calibrate_gyro.eidos should verify, errors: {:?}",
        report.errors
    );
    let trusted = report
        .obligations
        .iter()
        .any(|o| matches!(o.status, ObligationStatus::Trusted));
    assert!(
        trusted,
        "expected the magnitude postcondition to use a trusted lemma"
    );
}

#[test]
fn calibrate_gyro_broken_is_rejected() {
    let report = check_file("calibrate_gyro_broken.eidos");
    assert!(!report.ok(), "broken example must be rejected");
    assert!(
        report
            .errors
            .iter()
            .any(|e| e.message.contains("division by zero")),
        "rejection should be the missing division-by-zero guard, errors: {:?}",
        report.errors
    );
}

#[test]
fn build_emits_no_std_crate_without_kernel_types() {
    let rust = codegen_file("calibrate_gyro.eidos");
    // The erasure target must not leak any verification machinery.
    assert!(rust.contains("#![no_std]"));
    for leak in [
        "Refine",
        "Constraint",
        "eidos_kernel",
        "eidos_verifier",
        "Obligation",
    ] {
        assert!(!rust.contains(leak), "generated source leaked `{leak}`");
    }
    // The computational core must be present and self-contained.
    assert!(rust.contains("pub fn calibrate_gyro"));
    assert!(rust.contains("pub struct NormalizedVector3"));
    assert!(rust.contains("eidos_map(eidos_zip("));
}

/// The milestone for Phase 2: a verified eidos function must erasure-compile
/// to `no_std` Rust with no runtime cost from verification. We emit the
/// generated source and compile it with `rustc` (skipped if rustc is absent).
#[test]
fn generated_rust_compiles_no_std() {
    let rust = codegen_file("calibrate_gyro.eidos");
    let out_dir = std::env::temp_dir().join("eidos_codegen_test");
    fs::create_dir_all(&out_dir).expect("create temp dir");
    let lib = out_dir.join("lib.rs");
    fs::write(&lib, &rust).expect("write generated lib.rs");

    let rustc = match Command::new("rustc").arg("--version").output() {
        Ok(o) if o.status.success() => "rustc",
        _ => {
            eprintln!("skipping generated-rust compile test: rustc not found");
            return;
        }
    };
    let status = Command::new(rustc)
        .args(["--edition", "2021", "--crate-type", "lib"])
        .arg(&lib)
        .output()
        .expect("run rustc on generated source");
    let stderr = String::from_utf8_lossy(&status.stderr);
    assert!(
        status.status.success(),
        "generated no_std Rust must compile:\n{stderr}"
    );
}
