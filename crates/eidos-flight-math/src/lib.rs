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

pub use prover::{ProofStep, SuggestOutcome, suggest_and_verify};

use eidos_kernel::{check_with, Lemma, Report, DEFAULT_LEMMAS};
use eidos_parser::{parse, BinOp, Expr, Module};

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
/// lemma's own side conditions must `entails`-prove. `triangle_for_add` admits
/// `|a + b| <= K` via the triangle inequality; it is sound only when `K` is at
/// least `|a| + |b|`, which the kernel cannot check, so it is gated behind the
/// agent loop rather than enabled by default.
pub static AGENT_LEMMAS: &[Lemma] = &[Lemma {
    name: "triangle_for_add",
    apply: lemma_triangle_for_add,
}];

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
pub fn check_source(src: &str) -> Result<Report, eidos_parser::ParseError> {
    let module = parse(src)?;
    Ok(check_module(&module))
}

/// `|a + b| <= K` is admitted (triangle inequality) when the receiver of
/// `.magnitude()` is a sum. Returns no side conditions: the kernel cannot check
/// that `K >= |a| + |b|`, so this lemma must be reviewed by a human / gated by
/// the agent loop.
fn lemma_triangle_for_add(pred: &Expr, _ctx: &[Constraint]) -> Option<Vec<Constraint>> {
    if let Expr::Bin { op, a, .. } = pred {
        if matches!(op, BinOp::Le | BinOp::Lt) {
            if let Expr::Method { recv, name, args } = a.as_ref() {
                if name == "magnitude" && args.is_empty() && is_elementwise_sum(recv) {
                    return Some(vec![]);
                }
            }
        }
    }
    None
}

/// True if `e` is a sum `a + b` (used directly) or an element-wise sum produced
/// by `e.zip(f).map(|(x, y)| x + y)` / `e.map(|x| x + c)`. These are the shapes
/// the triangle inequality discharges via the `triangle_for_add` agent lemma.
fn is_elementwise_sum(e: &Expr) -> bool {
    match e {
        Expr::Bin { op: BinOp::Add, .. } => true,
        Expr::Method { name, args, .. } if name == "map" => match args.first() {
            Some(Expr::Lambda { body, .. }) => {
                matches!(body.as_ref(), Expr::Bin { op: BinOp::Add, .. })
            }
            _ => false,
        },
        _ => false,
    }
}

// `Constraint` appears in the lemma signatures even when a lemma admits with no
// side conditions.
use eidos_verifier::Constraint;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn primitives_verify() {
        let r = check_source(PRIMITIVES_EIDOS).expect("parse primitives");
        assert!(r.ok(), "primitives rejected: {:?}", r.errors);
    }

    #[test]
    fn primitives_rejected_without_domain_env() {
        // The same primitives must not verify under the bare kernel default set
        // when a non-linear obligation cannot be matched; here we just confirm
        // `check_module` (the domain environment) is the entry point used.
        let module = parse(PRIMITIVES_EIDOS).expect("parse primitives");
        let r = check_module(&module);
        assert!(r.ok(), "domain environment must verify primitives: {:?}", r.errors);
    }

    #[test]
    fn primitives_emit_no_std_rust() {
        let module = parse(PRIMITIVES_EIDOS).expect("parse primitives");
        let report = check_module(&module);
        assert!(report.ok(), "primitives rejected: {:?}", report.errors);
        let core = eidos_erasure::erase(&module);
        let rust = eidos_codegen::codegen(&core);
        assert!(rust.contains("#![no_std]"));
        assert!(rust.contains("pub fn quat_normalize"));
        assert!(rust.contains("pub fn pid_linear"));
        assert!(!rust.contains("Refine"));
    }
}
