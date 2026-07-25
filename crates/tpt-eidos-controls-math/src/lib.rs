//! tpt-eidos generic control-systems domain library.
//!
//! Provides pre-proved control-systems primitives: output clamping, bounded
//! increments, and rate limiting. Reuses the same `Lemma`/`TrustedLemmas`
//! pattern as `tpt-eidos-flight-math` for non-linear obligations.
//!
//! `check_module` verifies an eidos `Module` using the kernel's default
//! lemmas. It also ships reusable, pre-proved primitive definitions
//! ([`PRIMITIVES_EIDOS`]).

use tpt_eidos_kernel::{check_with, Lemma, Report, DEFAULT_LEMMAS};
use tpt_eidos_parser::{parse, Module};

/// The reusable control-systems primitives, as eidos source. Feed this to
/// `parse` and `check_module` to confirm the domain library verifies.
pub const PRIMITIVES_EIDOS: &str = include_str!("primitives.eidos");

/// Domain-specific lemmas for the control-systems verification environment.
/// Currently empty; the kernel's `DEFAULT_LEMMAS` cover the non-linear
/// obligations in the primitives. This set is where additional,
/// control-systems-specific trusted facts are registered as the library grows.
pub static CONTROLS_LEMMAS: &[Lemma] = &[];

/// Combine the kernel default lemmas and the domain lemmas into one registry.
fn combined(extra: &[Lemma]) -> Vec<Lemma> {
    let mut v: Vec<Lemma> = DEFAULT_LEMMAS.to_vec();
    v.extend(CONTROLS_LEMMAS.iter().copied());
    v.extend(extra.iter().copied());
    v
}

/// Verify a control-systems module with the standard domain-library lemma set.
pub fn check_module(module: &Module) -> Report {
    check_with(module, &combined(&[]))
}

/// Verify a control-systems module, additionally trusting the given extra
/// lemmas.
pub fn check_module_with(module: &Module, extra: &[Lemma]) -> Report {
    check_with(module, &combined(extra))
}

/// Parse and verify a control-systems eidos source string.
pub fn check_source(src: &str) -> Result<Report, tpt_eidos_parser::ParseError> {
    let module = parse(src)?;
    Ok(check_module(&module))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn primitives_verify() {
        let r = check_source(PRIMITIVES_EIDOS).expect("parse primitives");
        assert!(r.ok(), "primitives rejected: {:?}", r.errors);
    }

    #[test]
    fn primitives_verify_under_domain_env() {
        let module = parse(PRIMITIVES_EIDOS).expect("parse primitives");
        let r = check_module(&module);
        assert!(
            r.ok(),
            "domain environment must verify primitives: {:?}",
            r.errors
        );
    }

    #[test]
    fn primitives_emit_no_std_rust() {
        let module = parse(PRIMITIVES_EIDOS).expect("parse primitives");
        let report = check_module(&module);
        assert!(report.ok(), "primitives rejected: {:?}", report.errors);
        let core = tpt_eidos_erasure::erase(&module);
        let rust = tpt_eidos_codegen::codegen(&core).expect("codegen");
        assert!(rust.contains("#![no_std]"));
        assert!(rust.contains("pub fn clamp"));
        assert!(rust.contains("pub fn bounded_add"));
        assert!(rust.contains("pub fn rate_limit"));
        assert!(!rust.contains("Refine"));
    }
}
