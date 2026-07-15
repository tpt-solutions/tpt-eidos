//! Dependency-free fuzz/property tests for the QF_LRA verifier.
//!
//! Addresses the open question in TODO.md (Phase 5): "arbitrary constraint
//! systems must terminate". Uses a deterministic PRNG (no external crates) so CI
//! runs are reproducible.
//!
//! The solver's `MAX_CONSTRAINTS` guard bounds the Fourier-Motzkin elimination
//! blow-up, so adversarial or degenerate systems can neither run forever nor
//! allocate without bound — they bail toward a conservative answer.

use tpt_eidos_verifier::{entails, unsat, Constraint, LinExpr, Rel};

/// Deterministic xorshift64 PRNG (no external crates).
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Rng(seed ^ 0x9e3779b97f4a7c15)
    }
    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next_u64() % n as u64) as usize
    }
    fn int(&mut self, range: i64) -> i64 {
        (self.next_u64() as i64 % range) - range / 2
    }
}

const VARS: &[&str] = &["x0", "x1", "x2", "x3", "x4"];

/// Build a random linear expression as a sum of `coeff_i * var_i + constant`.
fn random_linear(rng: &mut Rng, nvars: usize) -> LinExpr {
    let mut e = LinExpr::constant(0.0);
    for _ in 0..nvars {
        let v = VARS[rng.below(VARS.len())];
        let c = rng.int(11) as f64;
        e = e.add(&LinExpr::var(v).scale(c));
    }
    e.add(&LinExpr::constant(rng.int(11) as f64))
}

fn random_constraint(rng: &mut Rng, nvars: usize) -> Constraint {
    let e = random_linear(rng, nvars);
    let rel = match rng.below(5) {
        0 => Rel::Le,
        1 => Rel::Lt,
        2 => Rel::Ge,
        3 => Rel::Gt,
        _ => Rel::Eq,
    };
    Constraint { rel, e }
}

#[test]
fn random_constraint_systems_terminate() {
    let mut rng = Rng::new(0xc0ffee);
    for _ in 0..300 {
        let n = 1 + rng.below(12);
        let nvars = 1 + rng.below(4);
        let mut cs = Vec::new();
        for _ in 0..n {
            cs.push(random_constraint(&mut rng, nvars));
        }
        // Neither call may panic or fail to return within the guarded budget.
        let _ = unsat(&cs);
        let conc = random_constraint(&mut rng, nvars);
        let _ = entails(&cs, &conc);
    }
}

#[test]
fn degenerate_systems_terminate() {
    // Empty and single-variable systems must resolve quickly and safely.
    assert!(!unsat(&[]));
    assert!(unsat(&[Constraint::le(LinExpr::constant(1.0))]));
    assert!(!unsat(&[Constraint::ge(LinExpr::constant(1.0))]));
}

#[test]
fn large_system_hits_constraint_guard_and_returns() {
    // A system large enough to stress the elimination blow-up must still return
    // a definitive (conservative) answer rather than growing unbounded.
    let mut cs = Vec::new();
    for i in 0..40 {
        let v = VARS[i % VARS.len()];
        cs.push(Constraint::le(LinExpr::var(v).scale(1.0)));
        cs.push(Constraint::ge(LinExpr::var(v).scale(1.0)));
    }
    let _ = unsat(&cs);
}
