//! tpt-eidos flight-control domain library (Phase 3 of the roadmap).
//!
//! The MVK kernel's QF_LRA prover handles linear arithmetic. Genuinely
//! non-linear textbook facts — e.g. "a vector normalized by its own magnitude
//! has magnitude 1" — are admitted as *named, reviewable* trusted lemmas, which
//! is the Phase-3 boundary described in `spec.txt` §6 and TODO.md.
//!
//! This crate is the standard verification environment for flight-control code:
//! `check_module` verifies an eidos `Module` using the kernel's default lemmas
//! plus the domain-lemmas shipped here. It also ships reusable, pre-proved
//! primitive definitions ([`PRIMITIVES_EIDOS`]) and a kernel-gated proof-step
//! suggester ([`prover`]) for the Phase-4 AI-assist workflow.

mod prover;

pub use prover::{suggest_and_verify, ProofStep, SuggestOutcome};

use tpt_eidos_kernel::{check_with, Lemma, Report, DEFAULT_LEMMAS};
use tpt_eidos_parser::{parse, Module};

/// The reusable flight-control primitives, as eidos source. Feed this to
/// `parse` and `check_module` to confirm the domain library verifies.
pub const PRIMITIVES_EIDOS: &str = include_str!("primitives.eidos");

/// Domain-specific lemmas that are always on in the flight-control
/// verification environment. The non-linear normalization facts already live in
/// the kernel's `DEFAULT_LEMMAS` (`normalized_vector`); this set is where
/// additional, flight-specific trusted facts are registered as the library
/// grows.
pub static FLIGHT_LEMMAS: &[Lemma] = &[];

/// Lemmas an external agent (e.g. an LLM proof synthesizer, see Phase 4) may
/// *propose*. They are never trusted blindly: `suggest_and_verify` only accepts
/// a step if the kernel re-verifies the resulting module, and even then the
/// lemma's own side conditions must `entails`-prove.
///
/// As of Phase 7c, `triangle_for_add` has been retired: its admission of
/// `|a + b| <= K` for *any* K was unsound (bug #6). Non-linear obligations
/// like the triangle inequality are now discharged via
/// [`ProofStep::ProposeNonlinearCertificate`], which the verifier checks
/// exactly over rationals.
pub static AGENT_LEMMAS: &[Lemma] = &[];

/// Combine the kernel default lemmas, the domain lemmas, and any extra
/// agent-suggested lemmas into one registry.
fn combined(extra: &[Lemma]) -> Vec<Lemma> {
    let mut v: Vec<Lemma> = DEFAULT_LEMMAS.to_vec();
    v.extend(FLIGHT_LEMMAS.iter().copied());
    v.extend(extra.iter().copied());
    v
}

/// Verify a flight-control module with the standard domain-library lemma set.
pub fn check_module(module: &Module) -> Report {
    check_with(module, &combined(&[]))
}

/// Verify a flight-control module, additionally trusting the given
/// agent-suggested lemmas (used by the proof-step suggester).
pub fn check_module_with(module: &Module, extra: &[Lemma]) -> Report {
    check_with(module, &combined(extra))
}

/// Parse and verify a flight-control eidos source string.
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

    // --- Phase 7c: triangle_for_add retired ---

    #[test]
    fn retired_triangle_for_add_rejects_false_bound() {
        // With triangle_for_add retired (bug #6 fixed), the false bound is
        // now correctly rejected — no agent lemma admits it any more.
        let src = "type Zero = { s: Array<f64, 3> | s.magnitude() <= 0.0 };
fn f(a: Array<f64, 3>, b: Array<f64, 3>) -> Zero {
    return { s: a.zip(b).map(|(x, y)| x + y) } as Zero;
}";
        let module = parse(src).expect("parse");
        let r = check_module_with(&module, &[]);
        assert!(
            !r.ok(),
            "retired lemma must not admit false bound (bug #6 fixed)"
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
        assert!(rust.contains("pub fn quat_normalize"));
        assert!(rust.contains("pub fn pid_linear"));
        assert!(!rust.contains("Refine"));
    }
}
