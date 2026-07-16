//! Transparent QF_LRA decision procedure for tpt-eidos.
//!
//! Implements a self-contained Fourier-Motzkin solver over linear real
//! arithmetic (no external SMT dependency, so the trusted computing base
//! stays auditable and CI stays offline). Exposes the four operations the
//! kernel needs: `unsat`, `entails`, `model`, `counterexample`.

#![warn(missing_docs)]

use std::collections::{BTreeMap, BTreeSet};

const EPS: f64 = 1e-9;

/// Soundness-preserving bound on the number of normalized constraints produced
/// by a single Fourier-Motzkin elimination round. The elimination can roughly
/// square the constraint count at each variable-elimination step, which is a
/// denial-of-service vector when called once per `requires`/`if`/division/
/// `ensures` obligation derived from source text. If a round would exceed this
/// bound the solver bails out *toward satisfiability* (so `unsat` returns
/// `false` and `solve`/`find_model` return `None`), which keeps the kernel's
/// safety checks (division-by-zero, etc.) honest: a failed proof is conservatively
/// treated as "unverified", never as "provably safe".
const MAX_CONSTRAINTS: usize = 200_000;

/// A linear expression `Σ cᵢ·xᵢ + k`. Variables are identified by name.
#[derive(Clone, Debug, PartialEq)]
pub struct LinExpr {
    /// Coefficient map: variable name → coefficient value.
    pub coeffs: BTreeMap<String, f64>,
    /// Constant offset `k`.
    pub constant: f64,
}

impl LinExpr {
    /// Constructs the zero expression `0`.
    pub fn zero() -> Self {
        LinExpr {
            coeffs: BTreeMap::new(),
            constant: 0.0,
        }
    }

    /// Constructs a single-variable expression `1·name`.
    pub fn var(name: impl Into<String>) -> Self {
        let mut coeffs = BTreeMap::new();
        coeffs.insert(name.into(), 1.0);
        LinExpr {
            coeffs,
            constant: 0.0,
        }
    }

    /// Constructs a constant expression `v`.
    pub fn constant(v: f64) -> Self {
        LinExpr {
            coeffs: BTreeMap::new(),
            constant: v,
        }
    }

    /// Returns `self + other`.
    pub fn add(&self, other: &LinExpr) -> LinExpr {
        let mut coeffs = self.coeffs.clone();
        for (k, v) in &other.coeffs {
            *coeffs.entry(k.clone()).or_insert(0.0) += v;
        }
        LinExpr {
            coeffs,
            constant: self.constant + other.constant,
        }
    }

    /// Returns `self - other`.
    pub fn sub(&self, other: &LinExpr) -> LinExpr {
        self.add(&other.neg())
    }

    /// Returns the negation `-self`.
    pub fn neg(&self) -> LinExpr {
        let coeffs = self.coeffs.iter().map(|(k, v)| (k.clone(), -v)).collect();
        LinExpr {
            coeffs,
            constant: -self.constant,
        }
    }

    /// Returns `self * s`.
    pub fn scale(&self, s: f64) -> LinExpr {
        let coeffs = self
            .coeffs
            .iter()
            .map(|(k, v)| (k.clone(), v * s))
            .collect();
        LinExpr {
            coeffs,
            constant: self.constant * s,
        }
    }

    /// Evaluates the expression given a variable assignment.
    pub fn evaluate(&self, model: &BTreeMap<String, f64>) -> f64 {
        let mut s = self.constant;
        for (k, v) in &self.coeffs {
            s += v * model.get(k).copied().unwrap_or(0.0);
        }
        s
    }
}

/// Relations supported by the solver.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rel {
    /// `≤`
    Le,
    /// `<`
    Lt,
    /// `≥`
    Ge,
    /// `>`
    Gt,
    /// `=`
    Eq,
}

/// A single linear constraint `expr rel 0`.
#[derive(Clone, Debug)]
pub struct Constraint {
    /// The relation (e.g. `≤`, `<`, `=`).
    pub rel: Rel,
    /// The left-hand-side linear expression; the right-hand side is always 0.
    pub e: LinExpr,
}

impl Constraint {
    /// `e ≤ 0`
    pub fn le(e: LinExpr) -> Self {
        Constraint { rel: Rel::Le, e }
    }
    /// `e < 0`
    pub fn lt(e: LinExpr) -> Self {
        Constraint { rel: Rel::Lt, e }
    }
    /// `e ≥ 0`
    pub fn ge(e: LinExpr) -> Self {
        Constraint { rel: Rel::Ge, e }
    }
    /// `e > 0`
    pub fn gt(e: LinExpr) -> Self {
        Constraint { rel: Rel::Gt, e }
    }
    /// `e = 0`
    pub fn eq(e: LinExpr) -> Self {
        Constraint { rel: Rel::Eq, e }
    }

    /// Reduce to a conjunction of `{Le, Lt}` constraints only.
    fn normalize(&self) -> Vec<(Rel, LinExpr)> {
        match self.rel {
            Rel::Le => vec![(Rel::Le, self.e.clone())],
            Rel::Lt => vec![(Rel::Lt, self.e.clone())],
            Rel::Ge => vec![(Rel::Le, self.e.neg())],
            Rel::Gt => vec![(Rel::Lt, self.e.neg())],
            Rel::Eq => vec![(Rel::Le, self.e.clone()), (Rel::Le, self.e.neg())],
        }
    }
}

type Norm = Vec<(Rel, LinExpr)>;

fn normalize_all(constraints: &[Constraint]) -> Norm {
    let mut out = Norm::new();
    for c in constraints {
        out.extend(c.normalize());
    }
    out
}

fn collect_vars(exprs: &Norm) -> BTreeSet<String> {
    let mut vars = BTreeSet::new();
    for (_r, e) in exprs {
        for (k, v) in &e.coeffs {
            if v.abs() > EPS {
                vars.insert(k.clone());
            }
        }
    }
    vars
}

/// Fourier-Motzkin elimination. Returns `true` iff the normalized system is
/// unsatisfiable. Only `Le`/`Lt` constraints reach this function.
fn fm_unsat(exprs: &Norm) -> bool {
    let vars = collect_vars(exprs);
    if vars.is_empty() {
        return exprs.iter().any(|(r, e)| match r {
            Rel::Le => e.constant > EPS,
            Rel::Lt => e.constant >= -EPS,
            _ => unreachable!("normalize produces only Le/Lt"),
        });
    }

    let v = vars.iter().next().unwrap().clone();
    let mut uppers: Vec<(LinExpr, bool)> = Vec::new();
    let mut lowers: Vec<(LinExpr, bool)> = Vec::new();
    let mut rest: Norm = Vec::new();

    for (r, e) in exprs {
        let cv = e.coeffs.get(&v).copied().unwrap_or(0.0);
        if cv.abs() < EPS {
            rest.push((*r, e.clone()));
            continue;
        }
        let mut a = e.clone();
        a.coeffs.remove(&v);
        let bound_expr = a.neg().scale(1.0 / cv);
        let strict = *r == Rel::Lt;
        if cv > 0.0 {
            uppers.push((bound_expr, strict));
        } else {
            lowers.push((bound_expr, strict));
        }
    }

    // Fourier-Motzkin can produce `uppers.len() * lowers.len()` new
    // constraints in a single round. That product is exactly the cost of the
    // nested construction below, so check it *before* building `next`:
    // otherwise an adversarial system would allocate hundreds of millions of
    // constraints before the post-build guard could fire (the original DoS
    // vector, bug #3). Bail toward satisfiable, which keeps safety checks
    // conservative ("unverified", never "provably safe").
    if rest.len() + uppers.len() * lowers.len() > MAX_CONSTRAINTS {
        return false;
    }

    let mut next: Norm = rest;
    for (ue, us) in &uppers {
        for (le, ls) in &lowers {
            let combined = le.sub(ue);
            if *us || *ls {
                next.push((Rel::Lt, combined));
            } else {
                next.push((Rel::Le, combined));
            }
        }
    }

    fm_unsat(&next)
}

/// Decide whether a constraint set is unsatisfiable.
///
/// ```
/// use tpt_eidos_verifier::{Constraint, LinExpr, unsat};
///
/// // x > 0 and x <= 0 is unsatisfiable
/// let cs = vec![
///     Constraint::gt(LinExpr::var("x")),
///     Constraint::le(LinExpr::var("x")),
/// ];
/// assert!(unsat(&cs));
/// assert!(!unsat(&cs[..1])); // x > 0 alone is satisfiable
/// ```
pub fn unsat(constraints: &[Constraint]) -> bool {
    fm_unsat(&normalize_all(constraints))
}

/// Decide whether `premises` entails `conclusion`, i.e.
/// `unsat(premises ∧ ¬conclusion)`.
///
/// ```
/// use tpt_eidos_verifier::{Constraint, LinExpr, entails};
///
/// // x > 0 entails x >= 0
/// let premises = vec![Constraint::gt(LinExpr::var("x"))];
/// let conclusion = Constraint::ge(LinExpr::var("x"));
/// assert!(entails(&premises, &conclusion));
/// ```
pub fn entails(premises: &[Constraint], conclusion: &Constraint) -> bool {
    match conclusion.rel {
        Rel::Eq => {
            let e = conclusion.e.clone();
            let lt_c = Constraint::lt(e.clone());
            let gt_c = Constraint::gt(e);
            unsat(&append(premises, &lt_c)) && unsat(&append(premises, &gt_c))
        }
        _ => {
            let mut cs: Vec<Constraint> = premises.to_vec();
            cs.extend(negate(conclusion));
            unsat(&cs)
        }
    }
}

/// Produce the negation of a constraint (single relation, non-Eq) as a set.
fn negate(c: &Constraint) -> Vec<Constraint> {
    match c.rel {
        Rel::Le => vec![Constraint::gt(c.e.clone())],
        Rel::Lt => vec![Constraint::ge(c.e.clone())],
        Rel::Ge => vec![Constraint::lt(c.e.clone())],
        Rel::Gt => vec![Constraint::le(c.e.clone())],
        Rel::Eq => vec![Constraint::lt(c.e.clone()), Constraint::gt(c.e.clone())],
    }
}

fn append(premises: &[Constraint], extra: &Constraint) -> Vec<Constraint> {
    let mut cs = premises.to_vec();
    cs.push(extra.clone());
    cs
}

/// Find a satisfying model of the constraint set, if one exists.
/// Used to produce counterexample witnesses.
pub fn find_model(constraints: &[Constraint]) -> Option<BTreeMap<String, f64>> {
    solve(&normalize_all(constraints))
}

fn solve(exprs: &Norm) -> Option<BTreeMap<String, f64>> {
    let vars = collect_vars(exprs);
    if vars.is_empty() {
        let ok = exprs.iter().all(|(r, e)| match r {
            Rel::Le => e.constant <= EPS,
            Rel::Lt => e.constant < -EPS,
            _ => unreachable!(),
        });
        return if ok { Some(BTreeMap::new()) } else { None };
    }

    let v = vars.iter().next().unwrap().clone();
    let mut uppers: Vec<(LinExpr, bool)> = Vec::new();
    let mut lowers: Vec<(LinExpr, bool)> = Vec::new();
    let mut rest: Norm = Vec::new();

    for (r, e) in exprs {
        let cv = e.coeffs.get(&v).copied().unwrap_or(0.0);
        if cv.abs() < EPS {
            rest.push((*r, e.clone()));
            continue;
        }
        let mut a = e.clone();
        a.coeffs.remove(&v);
        let bound_expr = a.neg().scale(1.0 / cv);
        let strict = *r == Rel::Lt;
        if cv > 0.0 {
            uppers.push((bound_expr, strict));
        } else {
            lowers.push((bound_expr, strict));
        }
    }

    // Same pre-construction bail as `fm_unsat`: the `uppers.len() *
    // lowers.len()` product bounds the work about to be done, so check it
    // before allocating (bug #3 DoS guard).
    if rest.len() + uppers.len() * lowers.len() > MAX_CONSTRAINTS {
        return None;
    }

    let mut next: Norm = rest;
    for (ue, us) in &uppers {
        for (le, ls) in &lowers {
            let combined = le.sub(ue);
            if *us || *ls {
                next.push((Rel::Lt, combined));
            } else {
                next.push((Rel::Le, combined));
            }
        }
    }

    let mut model = solve(&next)?;

    let mut upper_val = f64::INFINITY;
    for (ue, _) in &uppers {
        let val = ue.evaluate(&model);
        if val < upper_val {
            upper_val = val;
        }
    }
    let mut lower_val = f64::NEG_INFINITY;
    for (le, _) in &lowers {
        let val = le.evaluate(&model);
        if val > lower_val {
            lower_val = val;
        }
    }

    let value = if upper_val.is_finite() && lower_val.is_finite() {
        (upper_val + lower_val) / 2.0
    } else if upper_val.is_finite() {
        upper_val - 1.0
    } else if lower_val.is_finite() {
        lower_val + 1.0
    } else {
        0.0
    };
    model.insert(v, value);
    Some(model)
}

/// Return a counterexample witnessing the failure of `entails(premises,
/// conclusion)`: a model of `premises ∧ ¬conclusion`. `None` when the
/// entailment actually holds.
pub fn counterexample(
    premises: &[Constraint],
    conclusion: &Constraint,
) -> Option<BTreeMap<String, f64>> {
    match conclusion.rel {
        Rel::Eq => {
            let e = conclusion.e.clone();
            if let Some(m) = find_model(&append(premises, &Constraint::lt(e.clone()))) {
                return Some(m);
            }
            if let Some(m) = find_model(&append(premises, &Constraint::gt(e))) {
                return Some(m);
            }
            None
        }
        _ => {
            let mut cs: Vec<Constraint> = premises.to_vec();
            cs.extend(negate(conclusion));
            find_model(&cs)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(name: &str) -> LinExpr {
        LinExpr::var(name)
    }

    #[test]
    fn unsat_trivial_contradiction() {
        let cs = vec![Constraint::le(v("x")), Constraint::lt(v("x").neg())];
        assert!(unsat(&cs));
    }

    #[test]
    fn sat_simple_bounds() {
        let cs = vec![Constraint::le(v("x").sub(&LinExpr::constant(5.0)))];
        assert!(!unsat(&cs));
    }

    #[test]
    fn entails_mag_positive_excludes_zero() {
        let premises = vec![Constraint::gt(v("mag"))];
        let cs = append(&premises, &Constraint::eq(v("mag")));
        assert!(unsat(&cs), "mag > 0 must exclude mag == 0");
    }

    #[test]
    fn not_entailed_without_guard() {
        let premises: Vec<Constraint> = vec![];
        let conc = Constraint::eq(v("mag"));
        assert!(!entails(&premises, &conc));
    }

    #[test]
    fn counterexample_reports_nonzero() {
        let premises: Vec<Constraint> = vec![];
        let conc = Constraint::eq(v("mag"));
        let ce = counterexample(&premises, &conc).expect("must have a model");
        assert!((ce["mag"]).abs() > EPS, "witness must violate mag == 0");
    }

    #[test]
    fn model_satisfies_constraints() {
        let cs = vec![
            Constraint::gt(v("a")),
            Constraint::le(v("a").sub(&v("b"))),
            Constraint::le(v("b").sub(&LinExpr::constant(1.0))),
        ];
        let m = find_model(&cs).expect("should be sat");
        assert!(m["a"] > -EPS);
        assert!(m["a"] - m["b"] <= EPS);
        assert!(m["b"] - 1.0 <= EPS);
    }

    // --- Three-or-more variables, degenerate / unbounded systems, EPS edge. ---

    #[test]
    fn entails_with_three_variables() {
        // x >= 0, y >= 0, z >= 0, x + y + z <= 1  |-  x <= 1
        let premises = vec![
            Constraint::ge(v("x")),
            Constraint::ge(v("y")),
            Constraint::ge(v("z")),
            Constraint::le(
                v("x")
                    .add(&v("y"))
                    .add(&v("z"))
                    .sub(&LinExpr::constant(1.0)),
            ),
        ];
        let conc = Constraint::le(v("x").sub(&LinExpr::constant(1.0)));
        assert!(entails(&premises, &conc), "x is bounded by the sum");
    }

    #[test]
    fn unsat_three_variable_cycle() {
        // x < y, y < z, z < x  is a strict cycle -> unsatisfiable.
        let cs = vec![
            Constraint::lt(v("x").sub(&v("y"))),
            Constraint::lt(v("y").sub(&v("z"))),
            Constraint::lt(v("z").sub(&v("x"))),
        ];
        assert!(unsat(&cs), "strict cycle is unsatisfiable");
    }

    #[test]
    fn degenerate_empty_system_is_sat() {
        let cs: Vec<Constraint> = vec![];
        assert!(!unsat(&cs), "the empty constraint system is satisfiable");
        assert!(
            find_model(&cs).is_some(),
            "empty system has a (empty) model"
        );
    }

    #[test]
    fn unbounded_single_variable_is_sat() {
        // A single free variable with no bounds is trivially satisfiable.
        let cs = vec![Constraint::ge(v("x"))];
        assert!(!unsat(&cs));
        let m = find_model(&cs).expect("should be sat");
        assert!(m["x"] >= -EPS);
    }

    #[test]
    fn eps_boundary_lt_excludes_eps() {
        // x < 0 strictly excludes x == EPS (= 1e-9) because EPS is positive.
        let cs = vec![
            Constraint::lt(v("x")),
            Constraint::eq(v("x").sub(&LinExpr::constant(EPS))),
        ];
        assert!(unsat(&cs), "x < 0 must exclude x == EPS");
    }

    #[test]
    fn eps_boundary_le_allows_eps() {
        // x <= 0 with the non-strict relation keeps x == EPS within tolerance,
        // so the system is satisfiable. This pins the fixed-epsilon behaviour.
        let cs = vec![
            Constraint::le(v("x")),
            Constraint::eq(v("x").sub(&LinExpr::constant(EPS))),
        ];
        assert!(!unsat(&cs), "x <= 0 (non-strict) tolerates x == EPS");
    }
}
