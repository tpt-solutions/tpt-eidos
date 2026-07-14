//! Integration tests over the worked examples in `examples/`.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

use eidos_codegen::codegen;
use eidos_erasure::erase;
use eidos_flight_math::check_module;
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

/// Phase 3 milestone: a real flight-control control law, written against the
/// domain library, verifies under the domain-library lemma set.
#[test]
fn attitude_control_verifies_with_domain_library() {
    let src = fs::read_to_string(example_path("attitude_control.eidos"))
        .unwrap_or_else(|e| panic!("read attitude_control: {e}"));
    let module = parse(&src).unwrap_or_else(|e| panic!("parse attitude_control: {e}"));
    let report = check_module(&module);
    assert!(
        report.ok(),
        "attitude_control.eidos should verify, errors: {:?}",
        report.errors
    );
    let trusted = report
        .obligations
        .iter()
        .any(|o| matches!(o.status, ObligationStatus::Trusted));
    assert!(
        trusted,
        "expected the normalization postcondition to use a trusted lemma"
    );
}

/// Phase 3 milestone: the verified control law erases to clean `no_std` Rust
/// with no kernel/verifier types leaking into the generated source.
#[test]
fn attitude_control_emits_no_std_rust() {
    let rust = codegen_file("attitude_control.eidos");
    assert!(rust.contains("#![no_std]"));
    assert!(rust.contains("pub fn attitude_control"));
    assert!(rust.contains("pub struct UnitVec3"));
    for leak in [
        "Refine",
        "Constraint",
        "eidos_kernel",
        "eidos_verifier",
        "eidos_flight_math",
        "Obligation",
    ] {
        assert!(!rust.contains(leak), "generated source leaked `{leak}`");
    }
}

/// Phase 4 milestone: an LLM-suggested proof step is mechanically verified or
/// rejected by the kernel — never trusted without kernel approval.
#[test]
fn proof_suggestion_accepted_and_rejected() {
    use eidos_flight_math::{suggest_and_verify, ProofStep};

    // A function that divides by its parameter with no guard: rejected.
    let src = "fn div(x: f64) -> f64 { return x / x; }";

    // A sound suggestion (strengthen the precondition) is accepted by the kernel.
    let accepted = suggest_and_verify(
        src,
        &[ProofStep::StrengthenRequires {
            fn_name: "div".into(),
            extra: "x > 0.0".into(),
        }],
    )
    .unwrap();
    assert!(accepted[0].accepted, "kernel must accept the sound step");

    // A useless suggestion that still allows x == 0 is rejected by the kernel.
    let rejected = suggest_and_verify(
        src,
        &[ProofStep::StrengthenRequires {
            fn_name: "div".into(),
            extra: "x > -100.0".into(),
        }],
    )
    .unwrap();
    assert!(!rejected[0].accepted, "kernel must reject the unsound step");
}
