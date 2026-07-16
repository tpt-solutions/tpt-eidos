//! Transparent QF_LRA decision procedure for tpt-eidos.
//!
//! Implements a self-contained Fourier-Motzkin solver over linear real
//! arithmetic (no external SMT dependency, so the trusted computing base
//! stays auditable and CI stays offline). Exposes the four operations the
//! kernel needs: `unsat`, `entails`, `model`, `counterexample`.
//!
//! The elimination itself (`fm_unsat`/`solve`) runs on exact rational
//! arithmetic (see the `rat` module below), not `f64`: every `f64` literal
//! reaching the solver is losslessly decomposed into an exact fraction
//! (IEEE-754 doubles are themselves rationals, `mantissa * 2^exponent`), so
//! satisfiability decisions never depend on floating-point rounding. This
//! removes the old fixed-epsilon fudge factor as the soundness oracle for the
//! decision procedure itself; `f64`/`EPS` only remain at the edges, for
//! converting external literals in and reported model values back out.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

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

/// Exact rational arithmetic used internally by the decision procedure.
///
/// `f64` addition/scaling accumulates rounding error across the many
/// elimination rounds Fourier-Motzkin performs, which is exactly why the
/// solver used to need a fixed-epsilon fudge factor to decide satisfiability.
/// `Rat` sidesteps that: every arithmetic step is exact, and every operation
/// is `checked_*` — on the rare case a computation would overflow `i128`
/// (astronomically large or absurdly precise literals, or many elimination
/// rounds compounding denominators), the caller bails conservatively toward
/// "unverified" rather than silently losing precision, matching the
/// `MAX_CONSTRAINTS` guard's philosophy elsewhere in this module.
mod rat {
    use std::cmp::Ordering;

    /// An exact rational number, always kept reduced to lowest terms with a
    /// strictly positive denominator (`den == 1` when `num == 0`).
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Rat {
        num: i128,
        den: i128,
    }

    fn gcd(a: u128, b: u128) -> u128 {
        let (mut a, mut b) = (a, b);
        while b != 0 {
            let t = b;
            b = a % b;
            a = t;
        }
        if a == 0 {
            1
        } else {
            a
        }
    }

    impl Rat {
        pub fn zero() -> Rat {
            Rat { num: 0, den: 1 }
        }

        fn new(num: i128, den: i128) -> Option<Rat> {
            if den == 0 {
                return None;
            }
            let (n, d) = if den < 0 {
                (num.checked_neg()?, den.checked_neg()?)
            } else {
                (num, den)
            };
            if n == 0 {
                return Some(Rat { num: 0, den: 1 });
            }
            let g = gcd(n.unsigned_abs(), d.unsigned_abs()) as i128;
            Some(Rat {
                num: n / g,
                den: d / g,
            })
        }

        /// Losslessly decompose a finite `f64` into an exact fraction. Every
        /// finite IEEE-754 double is exactly `sign * mantissa * 2^exponent`
        /// for integer mantissa/exponent, so this is exact, not an
        /// approximation — it just makes the value's own precision explicit.
        /// Returns `None` for non-finite input or when the resulting
        /// numerator/denominator can't fit in `i128` (extreme subnormals);
        /// callers treat that as "cannot represent exactly" and bail
        /// conservatively.
        pub fn from_f64(f: f64) -> Option<Rat> {
            if !f.is_finite() {
                return None;
            }
            if f == 0.0 {
                return Some(Rat::zero());
            }
            let bits = f.to_bits();
            let sign: i128 = if bits >> 63 == 1 { -1 } else { 1 };
            let exp_bits = ((bits >> 52) & 0x7ff) as i32;
            let frac = (bits & 0xF_FFFF_FFFF_FFFF) as i128;
            let (mantissa, exp): (i128, i32) = if exp_bits == 0 {
                (frac, -1074)
            } else {
                ((1i128 << 52) | frac, exp_bits - 1075)
            };
            let signed_mantissa = sign.checked_mul(mantissa)?;
            if exp >= 0 {
                let factor = 2i128.checked_pow(exp as u32)?;
                let num = signed_mantissa.checked_mul(factor)?;
                Rat::new(num, 1)
            } else {
                let den = 2i128.checked_pow((-exp) as u32)?;
                Rat::new(signed_mantissa, den)
            }
        }

        /// Lossy conversion back to `f64`, used only for reported model/
        /// counterexample values, never for the satisfiability decision.
        pub fn to_f64(self) -> f64 {
            self.num as f64 / self.den as f64
        }

        pub fn is_zero(self) -> bool {
            self.num == 0
        }

        pub fn is_pos(self) -> bool {
            self.num > 0
        }

        pub fn is_neg(self) -> bool {
            self.num < 0
        }

        pub fn checked_neg(self) -> Option<Rat> {
            Some(Rat {
                num: self.num.checked_neg()?,
                den: self.den,
            })
        }

        // `checked_add`/`checked_mul` combine via LCM/cross-GCD-reduction
        // rather than a naive `d1 * d2` cross product. Fourier-Motzkin
        // repeatedly re-derives bounds that share large common denominator
        // factors (e.g. two bounds both traced back to the same source
        // literal), and a naive product needlessly squares that shared
        // factor at every combination step — enough to blow past `i128`
        // after a single elimination round even for a 3-constraint,
        // 1-variable system. Reducing through the GCD first keeps the result
        // as small as the *reduced* fraction actually needs, which is what
        // an exact-rational implementation has to do to be viable on `i128`
        // instead of requiring arbitrary-precision integers.
        pub fn checked_add(self, other: Rat) -> Option<Rat> {
            let g = gcd(self.den.unsigned_abs(), other.den.unsigned_abs()) as i128;
            let self_den_over_g = self.den / g;
            let other_den_over_g = other.den / g;
            let num = self
                .num
                .checked_mul(other_den_over_g)?
                .checked_add(other.num.checked_mul(self_den_over_g)?)?;
            let den = self_den_over_g.checked_mul(other.den)?;
            Rat::new(num, den)
        }

        pub fn checked_sub(self, other: Rat) -> Option<Rat> {
            self.checked_add(other.checked_neg()?)
        }

        pub fn checked_mul(self, other: Rat) -> Option<Rat> {
            let g1 = gcd(self.num.unsigned_abs(), other.den.unsigned_abs()) as i128;
            let g2 = gcd(other.num.unsigned_abs(), self.den.unsigned_abs()) as i128;
            let num = (self.num / g1).checked_mul(other.num / g2)?;
            let den = (self.den / g2).checked_mul(other.den / g1)?;
            Rat::new(num, den)
        }

        /// Multiplicative inverse; `None` for zero (the caller never invokes
        /// this on a zero coefficient) or on overflow.
        pub fn recip(self) -> Option<Rat> {
            if self.num == 0 {
                return None;
            }
            Rat::new(self.den, self.num)
        }

        pub fn checked_cmp(self, other: Rat) -> Option<Ordering> {
            let diff = self.checked_sub(other)?;
            Some(if diff.is_pos() {
                Ordering::Greater
            } else if diff.is_neg() {
                Ordering::Less
            } else {
                Ordering::Equal
            })
        }
    }
}

use rat::Rat;

/// A linear expression `Σ cᵢ·xᵢ + k`. Variables are identified by name.
#[derive(Clone, Debug, PartialEq)]
pub struct LinExpr {
    pub coeffs: BTreeMap<String, f64>,
    pub constant: f64,
}

impl LinExpr {
    pub fn zero() -> Self {
        LinExpr {
            coeffs: BTreeMap::new(),
            constant: 0.0,
        }
    }

    pub fn var(name: impl Into<String>) -> Self {
        let mut coeffs = BTreeMap::new();
        coeffs.insert(name.into(), 1.0);
        LinExpr {
            coeffs,
            constant: 0.0,
        }
    }

    pub fn constant(v: f64) -> Self {
        LinExpr {
            coeffs: BTreeMap::new(),
            constant: v,
        }
    }

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

    pub fn sub(&self, other: &LinExpr) -> LinExpr {
        self.add(&other.neg())
    }

    pub fn neg(&self) -> LinExpr {
        let coeffs = self.coeffs.iter().map(|(k, v)| (k.clone(), -v)).collect();
        LinExpr {
            coeffs,
            constant: -self.constant,
        }
    }

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
    Le,
    Lt,
    Ge,
    Gt,
    Eq,
}

/// A single linear constraint `expr rel 0`.
#[derive(Clone, Debug)]
pub struct Constraint {
    pub rel: Rel,
    pub e: LinExpr,
}

impl Constraint {
    pub fn le(e: LinExpr) -> Self {
        Constraint { rel: Rel::Le, e }
    }
    pub fn lt(e: LinExpr) -> Self {
        Constraint { rel: Rel::Lt, e }
    }
    pub fn ge(e: LinExpr) -> Self {
        Constraint { rel: Rel::Ge, e }
    }
    pub fn gt(e: LinExpr) -> Self {
        Constraint { rel: Rel::Gt, e }
    }
    pub fn eq(e: LinExpr) -> Self {
        Constraint { rel: Rel::Eq, e }
    }

    /// Reduce to a conjunction of `{Le, Lt}` constraints, converting to exact
    /// rational form. Returns `None` if `self.e`'s coefficients/constant
    /// can't be represented exactly (non-finite, or overflow — see
    /// `Rat::from_f64`); callers treat that as "cannot verify", not "cannot
    /// falsify".
    fn normalize(&self) -> Option<Vec<(NormRel, RatExpr)>> {
        let re = RatExpr::from_lin(&self.e)?;
        Some(match self.rel {
            Rel::Le => vec![(NormRel::Le, re)],
            Rel::Lt => vec![(NormRel::Lt, re)],
            Rel::Ge => vec![(NormRel::Le, re.checked_neg()?)],
            Rel::Gt => vec![(NormRel::Lt, re.checked_neg()?)],
            Rel::Eq => {
                let neg = re.checked_neg()?;
                vec![(NormRel::Le, re), (NormRel::Le, neg)]
            }
        })
    }
}

/// The two relations a constraint can be reduced to by [`Constraint::normalize`].
/// Kept as its own type (rather than reusing `Rel`) so the solver's internal
/// `Norm` representation carries the `Le`/`Lt`-only invariant at the type
/// level instead of relying on `unreachable!()` arms.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NormRel {
    Le,
    Lt,
}

/// A linear expression over exact rational coefficients — the internal,
/// solver-side counterpart of `LinExpr`. Coefficients that reduce to zero are
/// never stored, so `collect_vars` needs no epsilon filter.
#[derive(Clone, Debug)]
struct RatExpr {
    coeffs: BTreeMap<String, Rat>,
    constant: Rat,
}

impl RatExpr {
    fn from_lin(e: &LinExpr) -> Option<RatExpr> {
        let mut coeffs = BTreeMap::new();
        for (k, v) in &e.coeffs {
            let r = Rat::from_f64(*v)?;
            if !r.is_zero() {
                coeffs.insert(k.clone(), r);
            }
        }
        let constant = Rat::from_f64(e.constant)?;
        Some(RatExpr { coeffs, constant })
    }

    fn checked_add(&self, other: &RatExpr) -> Option<RatExpr> {
        let mut coeffs = self.coeffs.clone();
        for (k, v) in &other.coeffs {
            let entry = coeffs.entry(k.clone()).or_insert_with(Rat::zero);
            *entry = entry.checked_add(*v)?;
        }
        coeffs.retain(|_, v| !v.is_zero());
        let constant = self.constant.checked_add(other.constant)?;
        Some(RatExpr { coeffs, constant })
    }

    fn checked_neg(&self) -> Option<RatExpr> {
        let mut coeffs = BTreeMap::new();
        for (k, v) in &self.coeffs {
            coeffs.insert(k.clone(), v.checked_neg()?);
        }
        Some(RatExpr {
            coeffs,
            constant: self.constant.checked_neg()?,
        })
    }

    fn checked_sub(&self, other: &RatExpr) -> Option<RatExpr> {
        self.checked_add(&other.checked_neg()?)
    }

    fn checked_scale(&self, s: Rat) -> Option<RatExpr> {
        let mut coeffs = BTreeMap::new();
        for (k, v) in &self.coeffs {
            let r = v.checked_mul(s)?;
            if !r.is_zero() {
                coeffs.insert(k.clone(), r);
            }
        }
        let constant = self.constant.checked_mul(s)?;
        Some(RatExpr { coeffs, constant })
    }

    fn evaluate(&self, model: &BTreeMap<String, Rat>) -> Option<Rat> {
        let mut s = self.constant;
        for (k, v) in &self.coeffs {
            let mv = model.get(k).copied().unwrap_or_else(Rat::zero);
            s = s.checked_add(v.checked_mul(mv)?)?;
        }
        Some(s)
    }
}

type Norm = Vec<(NormRel, RatExpr)>;

fn normalize_all(constraints: &[Constraint]) -> Option<Norm> {
    let mut out = Norm::new();
    for c in constraints {
        out.extend(c.normalize()?);
    }
    Some(out)
}

fn collect_vars(exprs: &Norm) -> BTreeSet<String> {
    let mut vars = BTreeSet::new();
    for (_r, e) in exprs {
        for k in e.coeffs.keys() {
            vars.insert(k.clone());
        }
    }
    vars
}

/// Fourier-Motzkin elimination. Returns `true` iff the normalized system is
/// unsatisfiable. Only `Le`/`Lt` constraints reach this function. Runs on
/// exact rational arithmetic throughout; any internal overflow (see the
/// `rat` module) bails toward `false` (satisfiable), the same conservative
/// direction as the `MAX_CONSTRAINTS` guard below.
fn fm_unsat(exprs: &Norm) -> bool {
    let vars = collect_vars(exprs);
    if vars.is_empty() {
        return exprs.iter().any(|(r, e)| match r {
            NormRel::Le => e.constant.is_pos(),
            NormRel::Lt => !e.constant.is_neg(),
        });
    }

    let v = vars.iter().next().unwrap().clone();
    let mut uppers: Vec<(RatExpr, bool)> = Vec::new();
    let mut lowers: Vec<(RatExpr, bool)> = Vec::new();
    let mut rest: Norm = Vec::new();

    for (r, e) in exprs {
        let cv = e.coeffs.get(&v).copied().unwrap_or_else(Rat::zero);
        if cv.is_zero() {
            rest.push((*r, e.clone()));
            continue;
        }
        let mut a = e.clone();
        a.coeffs.remove(&v);
        let inv = match cv.recip() {
            Some(x) => x,
            None => return false,
        };
        let bound_expr = match a.checked_neg().and_then(|x| x.checked_scale(inv)) {
            Some(x) => x,
            None => return false,
        };
        let strict = *r == NormRel::Lt;
        if cv.is_pos() {
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
            let combined = match le.checked_sub(ue) {
                Some(x) => x,
                None => return false,
            };
            if *us || *ls {
                next.push((NormRel::Lt, combined));
            } else {
                next.push((NormRel::Le, combined));
            }
        }
    }

    fm_unsat(&next)
}

/// Decide whether a constraint set is unsatisfiable.
pub fn unsat(constraints: &[Constraint]) -> bool {
    match normalize_all(constraints) {
        Some(norm) => fm_unsat(&norm),
        // Can't represent a literal exactly: conservatively "not proven
        // unsatisfiable", matching the rest of this module's bias toward
        // "unverified" over "provably safe".
        None => false,
    }
}

/// Decide whether `premises` entails `conclusion`, i.e.
/// `unsat(premises ∧ ¬conclusion)`.
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

/// Find a satisfying model of the constraint set, if one exists. Used to
/// produce counterexample witnesses. The search runs in exact rational
/// arithmetic; the returned model is converted to `f64` only at this final
/// boundary, for display/consumption purposes.
pub fn find_model(constraints: &[Constraint]) -> Option<BTreeMap<String, f64>> {
    let norm = normalize_all(constraints)?;
    let model = solve(&norm)?;
    Some(model.into_iter().map(|(k, v)| (k, v.to_f64())).collect())
}

fn solve(exprs: &Norm) -> Option<BTreeMap<String, Rat>> {
    let vars = collect_vars(exprs);
    if vars.is_empty() {
        let ok = exprs.iter().all(|(r, e)| match r {
            NormRel::Le => !e.constant.is_pos(),
            NormRel::Lt => e.constant.is_neg(),
        });
        return if ok { Some(BTreeMap::new()) } else { None };
    }

    let v = vars.iter().next().unwrap().clone();
    let mut uppers: Vec<(RatExpr, bool)> = Vec::new();
    let mut lowers: Vec<(RatExpr, bool)> = Vec::new();
    let mut rest: Norm = Vec::new();

    for (r, e) in exprs {
        let cv = e.coeffs.get(&v).copied().unwrap_or_else(Rat::zero);
        if cv.is_zero() {
            rest.push((*r, e.clone()));
            continue;
        }
        let mut a = e.clone();
        a.coeffs.remove(&v);
        let inv = cv.recip()?;
        let bound_expr = a.checked_neg()?.checked_scale(inv)?;
        let strict = *r == NormRel::Lt;
        if cv.is_pos() {
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
            let combined = le.checked_sub(ue)?;
            if *us || *ls {
                next.push((NormRel::Lt, combined));
            } else {
                next.push((NormRel::Le, combined));
            }
        }
    }

    let mut model = solve(&next)?;

    let mut upper_val: Option<Rat> = None;
    for (ue, _) in &uppers {
        let val = ue.evaluate(&model)?;
        upper_val = Some(match upper_val {
            None => val,
            Some(cur) => {
                if val.checked_cmp(cur)? == Ordering::Less {
                    val
                } else {
                    cur
                }
            }
        });
    }
    let mut lower_val: Option<Rat> = None;
    for (le, _) in &lowers {
        let val = le.evaluate(&model)?;
        lower_val = Some(match lower_val {
            None => val,
            Some(cur) => {
                if val.checked_cmp(cur)? == Ordering::Greater {
                    val
                } else {
                    cur
                }
            }
        });
    }

    let one = Rat::from_f64(1.0)?;
    let half = Rat::from_f64(0.5)?;
    let value = match (upper_val, lower_val) {
        (Some(u), Some(l)) => u.checked_add(l)?.checked_mul(half)?,
        (Some(u), None) => u.checked_sub(one)?,
        (None, Some(l)) => l.checked_add(one)?,
        (None, None) => Rat::zero(),
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

    /// Tolerance for comparing reported `f64` witness values in tests. This
    /// is *not* the solver's decision tolerance any more (the decision
    /// procedure is exact, see the `rat` module) — it only accounts for the
    /// single `Rat -> f64` conversion applied to the final reported model.
    const TEST_TOL: f64 = 1e-9;

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
        assert!(ce["mag"] != 0.0, "witness must violate mag == 0");
    }

    #[test]
    fn model_satisfies_constraints() {
        let cs = vec![
            Constraint::gt(v("a")),
            Constraint::le(v("a").sub(&v("b"))),
            Constraint::le(v("b").sub(&LinExpr::constant(1.0))),
        ];
        let m = find_model(&cs).expect("should be sat");
        assert!(m["a"] > 0.0);
        assert!(m["a"] - m["b"] <= TEST_TOL);
        assert!(m["b"] - 1.0 <= TEST_TOL);
    }

    // --- Three-or-more variables, degenerate / unbounded systems. ---

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
        assert!(m["x"] >= 0.0);
    }

    // --- Exact-arithmetic boundary: a value that used to sit inside the old
    // fixed EPS tolerance is now correctly distinguished from zero. ---

    #[test]
    fn exact_boundary_lt_excludes_tiny_positive() {
        // x < 0 strictly excludes x == 1e-9, exactly as it would exclude any
        // other positive value.
        let cs = vec![
            Constraint::lt(v("x")),
            Constraint::eq(v("x").sub(&LinExpr::constant(1e-9))),
        ];
        assert!(unsat(&cs), "x < 0 must exclude x == 1e-9");
    }

    #[test]
    fn exact_boundary_le_rejects_tiny_positive() {
        // x <= 0 also excludes x == 1e-9: with exact rational arithmetic
        // there is no fudge-factor tolerance any more (regression test for
        // the old fixed-epsilon soundness gap — 1e-9 used to be silently
        // treated as "close enough" to zero).
        let cs = vec![
            Constraint::le(v("x")),
            Constraint::eq(v("x").sub(&LinExpr::constant(1e-9))),
        ];
        assert!(
            unsat(&cs),
            "x <= 0 must exactly exclude x == 1e-9, no epsilon tolerance"
        );
    }

    #[test]
    fn exact_boundary_le_allows_exact_zero() {
        // x <= 0 with x == 0 exactly is satisfiable, still.
        let cs = vec![Constraint::le(v("x")), Constraint::eq(v("x"))];
        assert!(!unsat(&cs), "x <= 0 must allow x == 0 exactly");
    }
}
