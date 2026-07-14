//! Integration tests over the worked examples in `examples/`.

use std::fs;
use std::path::PathBuf;

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
    assert!(trusted, "expected the magnitude postcondition to use a trusted lemma");
}

#[test]
fn calibrate_gyro_broken_is_rejected() {
    let report = check_file("calibrate_gyro_broken.eidos");
    assert!(!report.ok(), "broken example must be rejected");
    assert!(
        report.errors.iter().any(|e| e.message.contains("division by zero")),
        "rejection should be the missing division-by-zero guard, errors: {:?}",
        report.errors
    );
}
