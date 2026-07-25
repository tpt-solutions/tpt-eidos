//! Kernel-gated proof-step suggester (Phase 4 of the roadmap).
//!
//! An external agent (an LLM proof synthesizer, per `spec.txt` §7 / TODO.md
//! Phase 4) proposes *candidate proof steps*. The eidos kernel is the gate: a
//! step is only "accepted" if the resulting module verifies, and even then any
//! lemma it invokes is checked by the kernel (`entails` on its side conditions).
//! A suggestion is never trusted blindly — the compiler mathematically verifies
//! or rejects it.
//!
//! Phase 7c adds `ProposeNonlinearCertificate`: instead of admitting an
//! axiom via a `Lemma`, the agent supplies a sum-of-squares polynomial
//! certificate that the verifier checks *exactly* over rationals — the
//! certificate itself is the only new trusted surface.

use tpt_eidos_kernel::{CheckError, Obligation, Report};
use tpt_eidos_parser::{parse, parse_expr, BinOp, Expr, ExprKind, Fun, Item, Module};
use tpt_eidos_verifier::{check_sos_certificate, Poly, Rat, SosCertificate};

use super::{Lemma, AGENT_LEMMAS};

/// A candidate proof step proposed by an external agent.
#[derive(Clone, Debug, PartialEq)]
pub enum ProofStep {
    /// Strengthen a function's `requires` with an extra (linear) bound, given as
    /// an eidos expression string.
    StrengthenRequires { fn_name: String, extra: String },
    /// Propose that an agent-gated trusted lemma (by name, from `AGENT_LEMMAS`)
    /// be admitted for this verification.
    ApplyLemma(String),
    /// Propose a sum-of-squares certificate proving a polynomial identity.
    /// The verifier checks the certificate *exactly* over rationals — the
    /// certificate itself is the only new trusted surface.
    ProposeNonlinearCertificate {
        /// The polynomial identity target: `target == Σ cᵢ · sᵢ²`.
        target: Poly,
        /// `(coefficient cᵢ, witness polynomial sᵢ)` pairs.
        terms: Vec<(Rat, Poly)>,
    },
}

/// The kernel's verdict on a single proposed step.
#[derive(Clone, Debug)]
pub struct SuggestOutcome {
    pub step: ProofStep,
    /// True iff the kernel verified the module *after* applying the step.
    pub accepted: bool,
    pub errors: Vec<CheckError>,
    pub obligations: Vec<Obligation>,
}

/// Run an agent-proposed sequence of proof steps against `src`. Each step is
/// applied to a fresh copy of the parsed module and re-verified with the kernel.
/// Proposing a step never mutates `src`; the returned outcomes record, per step,
/// whether the kernel accepted the resulting module.
pub fn suggest_and_verify(src: &str, steps: &[ProofStep]) -> Result<Vec<SuggestOutcome>, String> {
    let module = parse(src).map_err(|e| format!("parse error: {e}"))?;
    let mut out = Vec::new();
    for step in steps {
        match step {
            ProofStep::StrengthenRequires { fn_name, extra } => {
                let extra_expr = match parse_expr(extra) {
                    Ok(e) => e,
                    Err(err) => {
                        out.push(SuggestOutcome {
                            step: step.clone(),
                            accepted: false,
                            errors: vec![CheckError {
                                message: format!("bad step expression `{extra}`: {err}"),
                                span: None,
                            }],
                            obligations: vec![],
                        });
                        continue;
                    }
                };
                let applied = apply_strengthen(&module, fn_name, &extra_expr);
                let report = super::check_module_with(&applied, &[]);
                push_outcome(&mut out, step, &report);
            }
            ProofStep::ApplyLemma(name) => {
                let lemma = match find_agent_lemma(name) {
                    Some(l) => *l,
                    None => {
                        out.push(SuggestOutcome {
                            step: step.clone(),
                            accepted: false,
                            errors: vec![CheckError {
                                message: format!("unknown lemma `{name}`"),
                                span: None,
                            }],
                            obligations: vec![],
                        });
                        continue;
                    }
                };
                let report = super::check_module_with(&module, &[lemma]);
                push_outcome(&mut out, step, &report);
            }
            ProofStep::ProposeNonlinearCertificate { target, terms } => {
                let cert = SosCertificate {
                    target: target.clone(),
                    terms: terms.clone(),
                };
                let accepted = check_sos_certificate(&cert);
                out.push(SuggestOutcome {
                    step: step.clone(),
                    accepted,
                    errors: if accepted {
                        vec![]
                    } else {
                        vec![CheckError {
                            message: "SoS certificate check failed: \
                                     identity does not hold exactly, or \
                                     a coefficient is negative"
                                .into(),
                            span: None,
                        }]
                    },
                    obligations: vec![],
                });
            }
        }
    }
    Ok(out)
}

fn push_outcome(out: &mut Vec<SuggestOutcome>, step: &ProofStep, report: &Report) {
    out.push(SuggestOutcome {
        step: step.clone(),
        accepted: report.ok(),
        errors: report.errors.clone(),
        obligations: report.obligations.clone(),
    });
}

fn find_agent_lemma(name: &str) -> Option<&'static Lemma> {
    AGENT_LEMMAS.iter().find(|l| l.name == name)
}

fn apply_strengthen(module: &Module, fn_name: &str, extra: &Expr) -> Module {
    let items = module
        .items
        .iter()
        .map(|it| match it {
            Item::Fn(f) if f.name == fn_name => {
                let new_req = match &f.requires {
                    Some(old) => Some(Expr::new(ExprKind::Bin {
                        op: BinOp::And,
                        a: Box::new(old.clone()),
                        b: Box::new(extra.clone()),
                    })),
                    None => Some(extra.clone()),
                };
                Item::Fn(Box::new(Fun {
                    requires: new_req,
                    ..(**f).clone()
                }))
            }
            other => other.clone(),
        })
        .collect();
    Module { items }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tpt_eidos_verifier::Poly;

    #[test]
    fn strengthen_requires_accepted() {
        let src = "fn div(x: f64) -> f64 { return x / x; }";
        let steps = vec![ProofStep::StrengthenRequires {
            fn_name: "div".into(),
            extra: "x > 0.0".into(),
        }];
        let out = suggest_and_verify(src, &steps).unwrap();
        assert_eq!(out.len(), 1);
        assert!(
            out[0].accepted,
            "kernel should accept the strengthened requires: {:?}",
            out[0].errors
        );
    }

    #[test]
    fn weak_strengthen_rejected() {
        let src = "fn div(x: f64) -> f64 { return x / x; }";
        let steps = vec![ProofStep::StrengthenRequires {
            fn_name: "div".into(),
            extra: "x > -100.0".into(),
        }];
        let out = suggest_and_verify(src, &steps).unwrap();
        assert!(
            !out[0].accepted,
            "a bound that still allows x == 0 must be rejected by the kernel"
        );
    }

    #[test]
    fn retired_triangle_for_add_is_unknown() {
        let src = "fn f(a: Array<f64, 3>) -> Array<f64, 3> { return a; }";
        let out =
            suggest_and_verify(src, &[ProofStep::ApplyLemma("triangle_for_add".into())]).unwrap();
        assert!(!out[0].accepted);
        assert!(out[0]
            .errors
            .iter()
            .any(|e| e.message.contains("unknown lemma")));
    }

    #[test]
    fn sos_certificate_accepted() {
        // (a - b)^2 = a^2 - 2ab + b^2  (correct SoS identity)
        let one = Rat::from_f64(1.0).unwrap();
        let neg_two = Rat::from_f64(-2.0).unwrap();
        let target = Poly::monomial(vec![("a".into(), 2)], one)
            .checked_add(&Poly::monomial(
                vec![("a".into(), 1), ("b".into(), 1)],
                neg_two,
            ))
            .unwrap()
            .checked_add(&Poly::monomial(vec![("b".into(), 2)], one))
            .unwrap();
        let s = Poly::monomial(vec![("a".into(), 1)], one)
            .checked_add(&Poly::monomial(
                vec![("b".into(), 1)],
                one.checked_neg().unwrap(),
            ))
            .unwrap();
        let src = "fn f(a: f64) -> f64 { return a; }";
        let out = suggest_and_verify(
            src,
            &[ProofStep::ProposeNonlinearCertificate {
                target,
                terms: vec![(one, s)],
            }],
        )
        .unwrap();
        assert_eq!(out.len(), 1);
        assert!(
            out[0].accepted,
            "correct SoS certificate must be accepted: {:?}",
            out[0].errors
        );
    }

    #[test]
    fn sos_certificate_negative_coeff_rejected() {
        let one = Rat::from_f64(1.0).unwrap();
        let neg_one = one.checked_neg().unwrap();
        let target = Poly::monomial(vec![("x".into(), 2)], one);
        let x = Poly::monomial(vec![("x".into(), 1)], one);
        let src = "fn f(x: f64) -> f64 { return x; }";
        let out = suggest_and_verify(
            src,
            &[ProofStep::ProposeNonlinearCertificate {
                target,
                terms: vec![(neg_one, x)],
            }],
        )
        .unwrap();
        assert!(!out[0].accepted);
        assert!(out[0]
            .errors
            .iter()
            .any(|e| e.message.contains("SoS certificate check failed")));
    }

    #[test]
    fn sos_certificate_mismatched_expansion_rejected() {
        // Claim: 5 = 1 * x^2  (identity doesn't hold)
        let one = Rat::from_f64(1.0).unwrap();
        let five = Rat::from_f64(5.0).unwrap();
        let target = Poly::from_rat(five);
        let x = Poly::monomial(vec![("x".into(), 1)], one);
        let src = "fn f(x: f64) -> f64 { return x; }";
        let out = suggest_and_verify(
            src,
            &[ProofStep::ProposeNonlinearCertificate {
                target,
                terms: vec![(one, x)],
            }],
        )
        .unwrap();
        assert!(!out[0].accepted);
    }

    #[test]
    fn unknown_lemma_rejected() {
        let src = "fn f(a: Array<f64, 3>) -> Array<f64, 3> { return a; }";
        let out = suggest_and_verify(src, &[ProofStep::ApplyLemma("nope".into())]).unwrap();
        assert!(!out[0].accepted);
        assert!(out[0]
            .errors
            .iter()
            .any(|e| e.message.contains("unknown lemma")));
    }

    #[test]
    fn malformed_extra_expr_reaches_error_path() {
        let src = "fn div(x: f64) -> f64 { return x / x; }";
        let out = suggest_and_verify(
            src,
            &[ProofStep::StrengthenRequires {
                fn_name: "div".into(),
                extra: "1 + * 2".into(),
            }],
        )
        .unwrap();
        assert_eq!(out.len(), 1);
        assert!(!out[0].accepted);
        assert!(out[0]
            .errors
            .iter()
            .any(|e| e.message.contains("bad step expression")));
    }
}
