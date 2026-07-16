//! Trusted refinement-subtyping typechecker for the tpt-eidos MVK.
//!
//! The kernel walks a parsed `Module` and discharges the proof obligations that
//! the language demands:
//!
//! * **Division safety** — every `a / b` must be provably non-zero:
//!   `unsat(context ∧ b == 0)` via the QF_LRA verifier. This is the obligation
//!   that separates `calibrate_gyro` (the `if mag > 0.0` guard discharges it)
//!   from `calibrate_gyro_broken` (no such guard).
//! * **Refinement subtyping** — `value as Type` and `ensures` obligations that
//!   are linear are discharged by the verifier; non-linear ones (e.g.
//!   `v.magnitude() <= 1.0`) are discharged via the trusted-lemma table, the
//!   Phase-3 domain-library boundary (see `TrustedLemmas`).
//! * **Termination** — non-recursive functions pass; a recursive call that
//!   passes its parameters unchanged (no decreasing metric) is rejected.

use std::collections::{HashMap, HashSet};

use tpt_eidos_parser::{BinOp, Expr, Fun, Item, Module, Pattern, Type, UnOp};
use tpt_eidos_verifier::{entails, unsat, Constraint, LinExpr, Rel};

#[derive(Clone, Debug)]
pub struct CheckError {
    pub message: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObligationStatus {
    Verified,
    Trusted,
    Unverified,
}

#[derive(Clone, Debug)]
pub struct Obligation {
    pub description: String,
    pub status: ObligationStatus,
}

#[derive(Clone, Debug, Default)]
pub struct Report {
    pub errors: Vec<CheckError>,
    pub obligations: Vec<Obligation>,
}

impl Report {
    pub fn ok(&self) -> bool {
        self.errors.is_empty()
    }
}

/// A trusted (non-linear) lemma the kernel may invoke to discharge an
/// obligation it cannot prove with the QF_LRA prover alone.
///
/// `apply` inspects the obligation predicate and the current linear context.
/// It returns:
/// * `Some(side_conditions)` — the lemma *applies*. The obligation is trusted,
///   provided every `Constraint` in `side_conditions` is itself provable by the
///   linear prover (`entails`). An empty `Vec` means the lemma is an *admitted
///   axiom* with no further proof required (the textbook fact is taken on
///   trust, as is the point of the Phase-3 domain-library boundary).
/// * `None` — the lemma does not match this obligation.
///
/// Lemmas are the only non-linear escape hatch. They are named and recorded so
/// every trusted obligation can be traced back to a specific, reviewable fact
/// (see `Report::obligations` and `tpt-eidos-flight-math`).
#[derive(Clone, Copy)]
pub struct Lemma {
    pub name: &'static str,
    pub apply: fn(&Expr, &[Constraint]) -> Option<Vec<Constraint>>,
}

impl Lemma {
    /// Returns the lemma's side conditions if it applies to `pred` under `ctx`.
    pub fn apply_to(&self, pred: &Expr, ctx: &[Constraint]) -> Option<Vec<Constraint>> {
        (self.apply)(pred, ctx)
    }
}

/// The lemmas the bare MVK ships with. Domain libraries (e.g. `tpt-eidos-flight-math`)
/// extend this set via `check_with`.
pub static DEFAULT_LEMMAS: &[Lemma] = &[Lemma {
    name: "normalized_vector",
    apply: lemma_normalized_vector,
}];

/// Type-check a whole module with the default lemma set. Equivalent to
/// `check_with(module, DEFAULT_LEMMAS)`.
pub fn check(module: &Module) -> Report {
    check_with(module, DEFAULT_LEMMAS)
}

/// Type-check a whole module with a caller-supplied trusted-lemma set (the
/// Phase-3 domain-library boundary). Returns a `Report`; the module is accepted
/// iff `report.ok()`.
pub fn check_with(module: &Module, lemmas: &[Lemma]) -> Report {
    let mut aliases: HashMap<String, Type> = HashMap::new();
    for it in &module.items {
        if let Item::TypeAlias { name, ty } = it {
            aliases.insert(name.clone(), ty.clone());
        }
    }
    let mut report = Report::default();
    for it in &module.items {
        if let Item::Fn(f) = it {
            let mut checker = Checker::new(&aliases, lemmas);
            checker.check_fun(f, &mut report);
        }
    }
    check_termination(module, &mut report);
    report
}

struct Checker<'a> {
    aliases: &'a HashMap<String, Type>,
    lemmas: &'a [Lemma],
    ensures: Option<Expr>,
    ret: Type,
    current_fn: String,
}

impl<'a> Checker<'a> {
    fn new(aliases: &'a HashMap<String, Type>, lemmas: &'a [Lemma]) -> Self {
        Checker {
            aliases,
            lemmas,
            ensures: None,
            ret: Type::Base("_".into()),
            current_fn: String::new(),
        }
    }

    fn check_fun(&mut self, f: &Fun, report: &mut Report) {
        self.ensures = f.ensures.clone();
        self.ret = f.ret.clone();
        self.current_fn = f.name.clone();

        let req_cs: Vec<Constraint> = f
            .requires
            .as_ref()
            .map(|r| self.path_constraints(r))
            .unwrap_or_default();
        if !req_cs.is_empty() && unsat(&req_cs) {
            report.errors.push(CheckError {
                message: "requires clause is contradictory (unsatisfiable)".into(),
            });
        }

        let ctx = req_cs;
        self.walk(&f.body, &ctx, report);
    }

    fn resolve(&self, ty: &Type) -> Type {
        match ty {
            Type::Named(n) => self
                .aliases
                .get(n)
                .map(|t| self.resolve(t))
                .unwrap_or_else(|| ty.clone()),
            other => other.clone(),
        }
    }

    fn as_refine(&self, ty: &Type) -> Option<Type> {
        match self.resolve(ty) {
            Type::Refine { .. } => Some(self.resolve(ty)),
            _ => None,
        }
    }

    /// Peel `Refine`/`Named` wrappers and return the declared element count of
    /// an `Array<_, N>` type, if `ty` ultimately denotes a fixed-length array.
    fn array_len_of(ty: &Type, aliases: &HashMap<String, Type>) -> Option<u64> {
        match ty {
            Type::Array(_, n) => Some(*n),
            Type::Refine { ty, .. } => Self::array_len_of(ty, aliases),
            Type::Named(n) => aliases.get(n).and_then(|t| Self::array_len_of(t, aliases)),
            _ => None,
        }
    }

    fn walk(&self, e: &Expr, ctx: &[Constraint], report: &mut Report) {
        match e {
            Expr::Num(_) | Expr::Bool(_) | Expr::Var(_) => {}
            Expr::ArrayLit(es) => {
                for x in es {
                    self.walk(x, ctx, report);
                }
            }
            Expr::Bin { op, a, b } => {
                self.walk(a, ctx, report);
                self.walk(b, ctx, report);
                if matches!(op, BinOp::Div | BinOp::Rem) {
                    let kind = if *op == BinOp::Div {
                        "division"
                    } else {
                        "remainder"
                    };
                    self.check_division(b, ctx, report, kind);
                }
            }
            Expr::Un { a, .. } => self.walk(a, ctx, report),
            Expr::If { cond, then, els } => {
                let mut then_ctx = ctx.to_vec();
                then_ctx.extend(self.path_constraints(cond));
                let mut else_ctx = ctx.to_vec();
                if let Some(neg) = self.negate_constraints(cond) {
                    else_ctx.extend(neg);
                }
                self.walk(then, &then_ctx, report);
                self.walk(els, &else_ctx, report);
            }
            Expr::Let { name, value, body } => {
                self.walk(value, ctx, report);
                let mut body_ctx = ctx.to_vec();
                if let Some(lv) = self.linearize(value) {
                    body_ctx.push(Constraint::eq(LinExpr::var(name.clone()).sub(&lv)));
                }
                self.walk(body, &body_ctx, report);
            }
            Expr::Call { args, .. } => {
                for a in args {
                    self.walk(a, ctx, report);
                }
            }
            Expr::Method { recv, args, .. } => {
                self.walk(recv, ctx, report);
                for a in args {
                    self.walk(a, ctx, report);
                }
            }
            Expr::Lambda { body, .. } => self.walk(body, ctx, report),
            Expr::Record(fields) => {
                for (_, v) in fields {
                    self.walk(v, ctx, report);
                }
            }
            Expr::Cast { value, ty } => {
                self.walk(value, ctx, report);
                if let Some(Type::Refine { bind, pred, .. }) = self.as_refine(ty) {
                    let target: &Expr = match value.as_ref() {
                        Expr::Record(fields) => fields
                            .iter()
                            .find(|(f, _)| f == &bind)
                            .map(|(_, v)| v)
                            .unwrap_or(value),
                        _ => value,
                    };
                    let inst = self.subst(&pred, &bind, target);
                    self.discharge(
                        &inst,
                        ctx,
                        &format!("refinement {}: {}", type_name(ty), expr_to_string(&inst)),
                        report,
                    );
                }
            }
            Expr::Return(e) => {
                self.walk(e, ctx, report);
                // Array-length soundness: a manifest array literal returned for
                // an `Array<_, N>` type must contain exactly `N` elements. This
                // is the only place the kernel enforces element count today.
                if let Expr::ArrayLit(es) = e.as_ref() {
                    if let Some(n) = Self::array_len_of(&self.ret, self.aliases) {
                        if (es.len() as u64) != n {
                            report.errors.push(CheckError {
                                message: format!(
                                    "function `{}` returns an array of length {} but its type requires length {}",
                                    self.current_fn,
                                    es.len(),
                                    n
                                ),
                            });
                        }
                    }
                }
                if self.as_refine(&self.ret).is_some() && !matches!(e.as_ref(), Expr::Cast { .. }) {
                    report.errors.push(CheckError {
                        message: format!(
                            "function `{}` returns a refinement type; the return value must be introduced with `as`",
                            self.current_fn
                        ),
                    });
                }
                if let Some(Expr::Lambda { params, body }) = &self.ensures {
                    if let Some(Pattern::Var(p)) = params.first() {
                        let inst = self.subst(body, p, e);
                        self.discharge(
                            &inst,
                            ctx,
                            &format!("ensures: {}", expr_to_string(&inst)),
                            report,
                        );
                    }
                }
            }
        }
    }

    fn check_division(&self, denom: &Expr, ctx: &[Constraint], report: &mut Report, kind: &str) {
        let desc = format!("{kind} by zero: {} != 0", expr_to_string(denom));
        match self.linearize(denom) {
            Some(d) => {
                let ob = Constraint::eq(d);
                let mut cs = ctx.to_vec();
                cs.push(ob.clone());
                if unsat(&cs) {
                    report.obligations.push(Obligation {
                        description: desc,
                        status: ObligationStatus::Verified,
                    });
                } else {
                    let ce = tpt_eidos_verifier::find_model(&cs);
                    let detail = ce
                        .map(|m| format!("counterexample: {:?}", m))
                        .unwrap_or_default();
                    report.errors.push(CheckError {
                        message: format!(
                            "possible {kind} by zero: denominator could be zero. {detail}"
                        ),
                    });
                    report.obligations.push(Obligation {
                        description: desc,
                        status: ObligationStatus::Unverified,
                    });
                }
            }
            None => {
                report.errors.push(CheckError {
                    message: format!(
                        "cannot prove denominator {} is non-zero (non-linear); {kind} rejected",
                        expr_to_string(denom)
                    ),
                });
                report.obligations.push(Obligation {
                    description: desc,
                    status: ObligationStatus::Unverified,
                });
            }
        }
    }

    fn discharge(&self, pred: &Expr, ctx: &[Constraint], desc: &str, report: &mut Report) {
        let pred = self.simplify(pred);
        if let Expr::Bin {
            op: BinOp::And,
            a,
            b,
        } = &pred
        {
            self.discharge(a, ctx, &format!("{desc} (conjunct 1)"), report);
            self.discharge(b, ctx, &format!("{desc} (conjunct 2)"), report);
            return;
        }

        if let Some(c) = self.to_constraint(&pred) {
            if entails(ctx, &c) {
                report.obligations.push(Obligation {
                    description: desc.into(),
                    status: ObligationStatus::Verified,
                });
                return;
            }
            if let Some(name) = self.try_lemma(&pred, ctx) {
                report.obligations.push(Obligation {
                    description: format!("{desc} (trusted lemma: {name})"),
                    status: ObligationStatus::Trusted,
                });
                return;
            }
            report.errors.push(CheckError {
                message: format!("could not verify obligation: {desc}"),
            });
            report.obligations.push(Obligation {
                description: desc.into(),
                status: ObligationStatus::Unverified,
            });
            return;
        }

        if let Some(name) = self.try_lemma(&pred, ctx) {
            report.obligations.push(Obligation {
                description: format!("{desc} (trusted lemma: {name})"),
                status: ObligationStatus::Trusted,
            });
            return;
        }
        report.errors.push(CheckError {
            message: format!("non-linear obligation not discharged by trusted lemmas: {desc}"),
        });
        report.obligations.push(Obligation {
            description: desc.into(),
            status: ObligationStatus::Unverified,
        });
    }

    /// Try the trusted-lemma registry. Returns the name of the first lemma that
    /// applies to `pred` and whose side conditions all `entails`-prove under
    /// `ctx`. `None` if no lemma discharges the obligation.
    fn try_lemma(&self, pred: &Expr, ctx: &[Constraint]) -> Option<&'static str> {
        for lemma in self.lemmas {
            if let Some(side) = lemma.apply_to(pred, ctx) {
                let provable = side.iter().all(|c| entails(ctx, c));
                if provable {
                    return Some(lemma.name);
                }
            }
        }
        None
    }

    fn simplify(&self, e: &Expr) -> Expr {
        match e {
            Expr::Cast { value, .. } => self.simplify(value),
            Expr::Method { recv, name, args } => {
                let r = self.simplify(recv);
                if args.is_empty() {
                    if let Expr::Record(fields) = &r {
                        if let Some((_, v)) = fields.iter().find(|(f, _)| f == name) {
                            return self.simplify(v);
                        }
                    }
                }
                let sargs: Vec<Expr> = args.iter().map(|a| self.simplify(a)).collect();
                Expr::Method {
                    recv: Box::new(r),
                    name: name.clone(),
                    args: sargs,
                }
            }
            Expr::Bin { op, a, b } => Expr::Bin {
                op: *op,
                a: Box::new(self.simplify(a)),
                b: Box::new(self.simplify(b)),
            },
            Expr::Un { op, a } => Expr::Un {
                op: *op,
                a: Box::new(self.simplify(a)),
            },
            Expr::ArrayLit(es) => Expr::ArrayLit(es.iter().map(|x| self.simplify(x)).collect()),
            Expr::Record(fields) => Expr::Record(
                fields
                    .iter()
                    .map(|(f, v)| (f.clone(), self.simplify(v)))
                    .collect(),
            ),
            Expr::If { cond, then, els } => Expr::If {
                cond: Box::new(self.simplify(cond)),
                then: Box::new(self.simplify(then)),
                els: Box::new(self.simplify(els)),
            },
            other => other.clone(),
        }
    }

    fn subst(&self, e: &Expr, var: &str, val: &Expr) -> Expr {
        match e {
            Expr::Var(v) if v == var => val.clone(),
            Expr::Var(v) => Expr::Var(v.clone()),
            Expr::Num(n) => Expr::Num(*n),
            Expr::Bool(b) => Expr::Bool(*b),
            Expr::Bin { op, a, b } => Expr::Bin {
                op: *op,
                a: Box::new(self.subst(a, var, val)),
                b: Box::new(self.subst(b, var, val)),
            },
            Expr::Un { op, a } => Expr::Un {
                op: *op,
                a: Box::new(self.subst(a, var, val)),
            },
            Expr::If { cond, then, els } => Expr::If {
                cond: Box::new(self.subst(cond, var, val)),
                then: Box::new(self.subst(then, var, val)),
                els: Box::new(self.subst(els, var, val)),
            },
            Expr::ArrayLit(es) => {
                Expr::ArrayLit(es.iter().map(|x| self.subst(x, var, val)).collect())
            }
            Expr::Method { recv, name, args } => Expr::Method {
                recv: Box::new(self.subst(recv, var, val)),
                name: name.clone(),
                args: args.iter().map(|a| self.subst(a, var, val)).collect(),
            },
            Expr::Call { func, args } => Expr::Call {
                func: func.clone(),
                args: args.iter().map(|a| self.subst(a, var, val)).collect(),
            },
            Expr::Lambda { params, body } => {
                if pattern_binds(params, var) {
                    Expr::Lambda {
                        params: params.clone(),
                        body: body.clone(),
                    }
                } else {
                    Expr::Lambda {
                        params: params.clone(),
                        body: Box::new(self.subst(body, var, val)),
                    }
                }
            }
            Expr::Record(fields) => Expr::Record(
                fields
                    .iter()
                    .map(|(f, v)| (f.clone(), self.subst(v, var, val)))
                    .collect(),
            ),
            Expr::Cast { value, ty } => Expr::Cast {
                value: Box::new(self.subst(value, var, val)),
                ty: ty.clone(),
            },
            Expr::Return(_) | Expr::Let { .. } => e.clone(),
        }
    }

    fn linearize(&self, e: &Expr) -> Option<LinExpr> {
        linearize(e)
    }

    fn to_constraint(&self, e: &Expr) -> Option<Constraint> {
        match e {
            Expr::Bin { op, a, b } => {
                let la = self.linearize(a)?;
                let lb = self.linearize(b)?;
                let rel = match op {
                    BinOp::Gt => Rel::Gt,
                    BinOp::Ge => Rel::Ge,
                    BinOp::Lt => Rel::Lt,
                    BinOp::Le => Rel::Le,
                    BinOp::Eq => Rel::Eq,
                    _ => return None,
                };
                Some(Constraint {
                    rel,
                    e: la.sub(&lb),
                })
            }
            _ => None,
        }
    }

    fn path_constraints(&self, e: &Expr) -> Vec<Constraint> {
        match e {
            Expr::Bin { op, a, b } => match op {
                BinOp::And => {
                    let mut v = self.path_constraints(a);
                    v.extend(self.path_constraints(b));
                    v
                }
                BinOp::Gt => self.cmp(a, b, Rel::Gt),
                BinOp::Ge => self.cmp(a, b, Rel::Ge),
                BinOp::Lt => self.cmp(a, b, Rel::Lt),
                BinOp::Le => self.cmp(a, b, Rel::Le),
                BinOp::Eq => self.cmp(a, b, Rel::Eq),
                _ => vec![],
            },
            _ => vec![],
        }
    }

    fn cmp(&self, a: &Expr, b: &Expr, rel: Rel) -> Vec<Constraint> {
        match (self.linearize(a), self.linearize(b)) {
            (Some(la), Some(lb)) => vec![Constraint {
                rel,
                e: la.sub(&lb),
            }],
            _ => vec![],
        }
    }

    fn negate_constraints(&self, e: &Expr) -> Option<Vec<Constraint>> {
        match e {
            Expr::Bin { op, a, b } => match op {
                BinOp::And => {
                    let na = self.negate_constraints(a)?;
                    let nb = self.negate_constraints(b)?;
                    let mut v = na;
                    v.extend(nb);
                    Some(v)
                }
                BinOp::Gt => Some(self.cmp(a, b, Rel::Le)),
                BinOp::Ge => Some(self.cmp(a, b, Rel::Lt)),
                BinOp::Lt => Some(self.cmp(a, b, Rel::Ge)),
                BinOp::Le => Some(self.cmp(a, b, Rel::Gt)),
                BinOp::Eq => {
                    let mut v = self.cmp(a, b, Rel::Lt);
                    v.extend(self.cmp(a, b, Rel::Gt));
                    Some(v)
                }
                BinOp::Ne => Some(self.cmp(a, b, Rel::Eq)),
                _ => None,
            },
            Expr::Un { op: UnOp::Not, a } => Some(self.path_constraints(a)),
            _ => None,
        }
    }
}

/// Module-level termination check. Builds a call graph over the functions in the
/// module and rejects any recursive cycle (self- or mutual recursion) that lacks
/// a well-founded metric: every recursive call must pass at least one argument
/// that is strictly decreasing with respect to the corresponding formal
/// parameter, provably under the linear prover (unconditionally).
fn check_termination(module: &Module, report: &mut Report) {
    let mut params_of: HashMap<String, Vec<String>> = HashMap::new();
    let mut calls_of: HashMap<String, Vec<(String, Vec<Expr>)>> = HashMap::new();
    let mut callees: HashMap<String, Vec<String>> = HashMap::new();

    for it in &module.items {
        if let Item::Fn(f) = it {
            let pnames = f.params.iter().map(|(n, _)| n.clone()).collect();
            params_of.insert(f.name.clone(), pnames);
            let mut direct = Vec::new();
            let mut adj = Vec::new();
            collect_calls(&f.body, &mut direct, &mut adj);
            calls_of.insert(f.name.clone(), direct);
            callees.insert(f.name.clone(), adj);
        }
    }

    for it in &module.items {
        if let Item::Fn(f) = it {
            let reach = reachable(&callees, &f.name);
            if !reach.contains(&f.name) {
                continue;
            }
            let mut bad = false;
            for (callee, args) in &calls_of[&f.name] {
                if !reach.contains(callee) {
                    continue;
                }
                let cparams = match params_of.get(callee) {
                    Some(p) => p,
                    None => continue,
                };
                let decreases = args
                    .iter()
                    .zip(cparams.iter())
                    .any(|(a, p)| expr_decreases(a, p));
                if !decreases {
                    bad = true;
                    break;
                }
            }
            if bad {
                report.errors.push(CheckError {
                    message: format!(
                        "function `{}` may not terminate: a recursive call has no strictly-decreasing argument (no well-founded metric)",
                        f.name
                    ),
                });
            }
        }
    }
}

/// Collect every `Call` in `e`, recording `(callee, args)` pairs and the direct
/// callee names (used to build the call graph).
fn collect_calls(e: &Expr, out: &mut Vec<(String, Vec<Expr>)>, adj: &mut Vec<String>) {
    match e {
        Expr::Bin { a, b, .. } => {
            collect_calls(a, out, adj);
            collect_calls(b, out, adj);
        }
        Expr::Un { a, .. } => collect_calls(a, out, adj),
        Expr::If { cond, then, els } => {
            collect_calls(cond, out, adj);
            collect_calls(then, out, adj);
            collect_calls(els, out, adj);
        }
        Expr::Let { value, body, .. } => {
            collect_calls(value, out, adj);
            collect_calls(body, out, adj);
        }
        Expr::Call { func, args } => {
            out.push((func.clone(), args.clone()));
            if !adj.contains(func) {
                adj.push(func.clone());
            }
            for a in args {
                collect_calls(a, out, adj);
            }
        }
        Expr::Method { recv, args, .. } => {
            collect_calls(recv, out, adj);
            for a in args {
                collect_calls(a, out, adj);
            }
        }
        Expr::Lambda { body, .. } => collect_calls(body, out, adj),
        Expr::Record(fields) => {
            for (_, v) in fields {
                collect_calls(v, out, adj);
            }
        }
        Expr::Cast { value, .. } => collect_calls(value, out, adj),
        Expr::Return(e) => collect_calls(e, out, adj),
        _ => {}
    }
}

/// Set of all functions reachable (transitively) from `start`, including
/// `start` itself.
fn reachable(callees: &HashMap<String, Vec<String>>, start: &str) -> HashSet<String> {
    let mut seen = HashSet::new();
    let mut stack = vec![start.to_string()];
    while let Some(n) = stack.pop() {
        if !seen.insert(n.clone()) {
            continue;
        }
        if let Some(cs) = callees.get(&n) {
            for c in cs {
                if !seen.contains(c) {
                    stack.push(c.clone());
                }
            }
        }
    }
    seen
}

/// True iff `arg` is strictly smaller than the parameter named `param`,
/// unconditionally provable from the linear arithmetic prover (e.g. `n - 1.0`
/// is strictly smaller than `n`). Used as the well-founded metric for
/// termination.
fn expr_decreases(arg: &Expr, param: &str) -> bool {
    match linearize(arg) {
        Some(la) => {
            let diff = la.sub(&LinExpr::var(param));
            entails(&[], &Constraint::lt(diff))
        }
        None => false,
    }
}

fn patterns_to_string(params: &[Pattern]) -> String {
    params
        .iter()
        .map(pattern_to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

fn pattern_to_string(p: &Pattern) -> String {
    match p {
        Pattern::Var(v) => v.clone(),
        Pattern::Tuple(ps) => format!("({})", patterns_to_string(ps)),
    }
}

fn expr_to_string(e: &Expr) -> String {
    match e {
        Expr::Num(n) => format!("{n}"),
        Expr::Bool(b) => format!("{b}"),
        Expr::Var(v) => v.clone(),
        Expr::Bin { op, a, b } => format!(
            "({} {} {})",
            expr_to_string(a),
            binop_str(*op),
            expr_to_string(b)
        ),
        Expr::Un { op, a } => format!("{}{}", unop_str(*op), expr_to_string(a)),
        Expr::ArrayLit(es) => format!(
            "[{}]",
            es.iter().map(expr_to_string).collect::<Vec<_>>().join(", ")
        ),
        Expr::Call { func, args } => format!(
            "{}({})",
            func,
            args.iter()
                .map(expr_to_string)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Expr::Method { recv, name, args } => {
            if args.is_empty() {
                format!("{}.{}", expr_to_string(recv), name)
            } else {
                format!(
                    "{}.{}({})",
                    expr_to_string(recv),
                    name,
                    args.iter()
                        .map(expr_to_string)
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }
        }
        Expr::Lambda { params, .. } => {
            format!("|{}| ..", patterns_to_string(params))
        }
        Expr::Record(fields) => format!(
            "{{{}}}",
            fields
                .iter()
                .map(|(f, _)| f.clone())
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Expr::Cast { .. } => "cast".into(),
        Expr::Return(_) => "return".into(),
        Expr::If { .. } => "if".into(),
        Expr::Let { .. } => "let".into(),
    }
}

fn binop_str(op: BinOp) -> &'static str {
    match op {
        BinOp::Add => "+",
        BinOp::Sub => "-",
        BinOp::Mul => "*",
        BinOp::Div => "/",
        BinOp::Rem => "%",
        BinOp::Lt => "<",
        BinOp::Gt => ">",
        BinOp::Le => "<=",
        BinOp::Ge => ">=",
        BinOp::Eq => "==",
        BinOp::Ne => "!=",
        BinOp::And => "&&",
        BinOp::Or => "||",
    }
}

fn unop_str(op: UnOp) -> &'static str {
    match op {
        UnOp::Neg => "-",
        UnOp::Not => "!",
    }
}

/// Trusted `normalized_vector` lemma (the Phase-3 domain-library boundary).
/// The MVK's linear prover cannot handle non-linear arithmetic, so the textbook
/// fact "a vector normalized by its own magnitude has magnitude 1" is admitted
/// as an axiom (no further side conditions). See `spec.txt` §6 and TODO.md
/// Phase 3.
///
/// Matches obligations of the form `<recv>.magnitude() <= 1.0` (or `< 1.0`)
/// where `<recv>` is either a zero literal `[0.0, ...]`, or a `map` whose body
/// divides each element by the magnitude of the array being normalized (i.e.
/// `x / mag` with `mag > 0` provable in context).
pub fn lemma_normalized_vector(pred: &Expr, ctx: &[Constraint]) -> Option<Vec<Constraint>> {
    if let Expr::Bin { op, a, b } = pred {
        if matches!(op, BinOp::Le | BinOp::Lt) {
            if let Expr::Num(1.0) = b.as_ref() {
                if let Expr::Method { recv, name, args } = a.as_ref() {
                    if name == "magnitude" && args.is_empty() && normalized_vector_shape(recv, ctx)
                    {
                        return Some(vec![]);
                    }
                }
            }
        }
    }
    None
}

/// True if any identifier bound by `params` equals `var`.
fn pattern_binds(params: &[Pattern], var: &str) -> bool {
    params.iter().any(|p| pattern_binds_one(p, var))
}

fn pattern_binds_one(p: &Pattern, var: &str) -> bool {
    match p {
        Pattern::Var(v) => v == var,
        Pattern::Tuple(ps) => pattern_binds(ps, var),
    }
}

fn normalized_vector_shape(x: &Expr, ctx: &[Constraint]) -> bool {
    match x {
        Expr::ArrayLit(elems) => literal_magnitude_le(elems, 1.0),
        Expr::Method { name, args, .. } if name == "map" => {
            if let Some(Expr::Lambda { params, body }) = args.first() {
                if params.len() == 1 {
                    if let Expr::Bin {
                        op: BinOp::Div, b, ..
                    } = body.as_ref()
                    {
                        if let Some(mc) = linearize(b) {
                            return entails(ctx, &Constraint::gt(mc));
                        }
                    }
                }
            }
            false
        }
        _ => false,
    }
}

/// True iff `elems` is a literal numeric array whose Euclidean norm is `<= bound`
/// (exact, since the components are constants). Used so literals such as the
/// quaternion identity `[0.0, 0.0, 0.0, 1.0]` are accepted by the
/// `normalized_vector` lemma.
fn literal_magnitude_le(elems: &[Expr], bound: f64) -> bool {
    let mut sum_sq = 0.0f64;
    for e in elems {
        if let Expr::Num(n) = e {
            sum_sq += n * n;
        } else {
            return false;
        }
    }
    sum_sq.sqrt() <= bound + 1e-9
}

fn type_name(ty: &Type) -> String {
    match ty {
        Type::Base(s) => s.clone(),
        Type::Named(s) => s.clone(),
        Type::Array(inner, n) => format!("Array<{}, {}>", type_name(inner), n),
        Type::Refine { bind, ty, .. } => format!("{{ {}: {} | .. }}", bind, type_name(ty)),
    }
}

/// Linearize an expression into a `LinExpr` (used by `to_constraint`, `cmp`, and
/// the `normalized_vector` lemma). Returns `None` for genuinely non-linear
/// sub-expressions the kernel has no other way to reason about.
///
/// `.magnitude()` calls are the one exception: rather than giving up, they're
/// admitted as an *opaque atom* — a fresh linear variable keyed by the call's
/// canonical string form (`expr_to_string`), so `a.magnitude()` used in two
/// places (e.g. a `requires` clause and a lemma's side condition) refers to
/// the same atom, giving congruence "for free" from structural equality. This
/// doesn't make magnitude computable — the atom carries no numeric value
/// unless something else in the linear context bounds it (a `requires
/// a.magnitude() <= 1.0`, for instance) — it just lets such bounds actually
/// enter the linear context instead of being silently dropped. This is what
/// lets domain lemmas like `tpt-eidos-flight-math`'s `triangle_for_add`
/// derive a real, checked side condition (`K >= a.magnitude() + b.magnitude()`)
/// instead of admitting unconditionally.
pub fn linearize(e: &Expr) -> Option<LinExpr> {
    match e {
        Expr::Num(n) => Some(LinExpr::constant(*n)),
        Expr::Var(v) => Some(LinExpr::var(v.clone())),
        Expr::Un { op: UnOp::Neg, a } => Some(linearize(a)?.neg()),
        Expr::Bin { op, a, b } => {
            let la = linearize(a)?;
            let lb = linearize(b)?;
            match op {
                BinOp::Add => Some(la.add(&lb)),
                BinOp::Sub => Some(la.sub(&lb)),
                BinOp::Mul => {
                    if let Expr::Num(k) = a.as_ref() {
                        Some(lb.scale(*k))
                    } else if let Expr::Num(k) = b.as_ref() {
                        Some(la.scale(*k))
                    } else {
                        None
                    }
                }
                BinOp::Div => {
                    if let Expr::Num(k) = b.as_ref() {
                        if k.abs() > 1e-12 {
                            Some(la.scale(1.0 / *k))
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                }
                _ => None,
            }
        }
        Expr::Method { name, args, .. } if name == "magnitude" && args.is_empty() => {
            Some(LinExpr::var(expr_to_string(e)))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tpt_eidos_parser::{parse, parse_expr};

    fn check_src(src: &str) -> Report {
        let m = parse(src).expect("parse");
        check(&m)
    }

    #[test]
    fn accepts_guarded_division() {
        let src = "fn f(a: f64) -> f64 requires a > 0.0 {
            if a > 0.0 { return a / a; } else { return 0.0; }
        }";
        let r = check_src(src);
        assert!(r.ok(), "errors: {:?}", r.errors);
    }

    #[test]
    fn rejects_unguarded_division() {
        let src = "fn f(a: f64) -> f64 { return a / a; }";
        let r = check_src(src);
        assert!(!r.ok(), "expected rejection");
        assert!(r
            .errors
            .iter()
            .any(|e| e.message.contains("division by zero")));
    }

    #[test]
    fn accepts_normalized_return() {
        let src = "type NV = { v: f64 | v.magnitude() <= 1.0 };
        fn f(vec: Array<f64, 1>, mag: f64) -> NV requires mag > 0.0 {
            if mag > 0.0 { return { v: vec.map(|x| x / mag) } as NV; }
            else { return { v: [0.0] } as NV; }
        }";
        let r = check_src(src);
        assert!(r.ok(), "errors: {:?}", r.errors);
    }

    // --- Bug #1: `%` (remainder) must also be guarded by division safety. ---

    #[test]
    fn rejects_unguarded_remainder() {
        let src = "fn f(a: f64) -> f64 { return a % a; }";
        let r = check_src(src);
        assert!(!r.ok(), "expected rejection of unguarded remainder");
        assert!(r
            .errors
            .iter()
            .any(|e| e.message.contains("remainder by zero")));
    }

    #[test]
    fn accepts_guarded_remainder() {
        let src = "fn f(a: f64) -> f64 requires a > 0.0 {
            if a > 0.0 { return a % a; } else { return 0.0; }
        }";
        let r = check_src(src);
        assert!(r.ok(), "errors: {:?}", r.errors);
    }

    // --- Bug #8: `let`-bound manifest values must enter the proof context. ---

    #[test]
    fn let_bound_nonzero_enters_context() {
        let src = "fn f(a: f64) -> f64 {
            let x = 5.0;
            return a / x;
        }";
        let r = check_src(src);
        assert!(
            r.ok(),
            "let-bound 5.0 should prove the divisor non-zero: {:?}",
            r.errors
        );
    }

    #[test]
    fn let_bound_zero_is_rejected() {
        let src = "fn f(a: f64) -> f64 {
            let x = 0.0;
            return a / x;
        }";
        let r = check_src(src);
        assert!(!r.ok(), "dividing by a let-bound 0 must be rejected");
    }

    // --- Bug #2: real termination checking (self + mutual recursion). ---

    #[test]
    fn accepts_structurally_decreasing_recursion() {
        let src = "fn f(n: f64) -> f64 {
            if n > 0.0 { return f(n - 1.0); } else { return 0.0; }
        }";
        let r = check_src(src);
        assert!(
            r.ok(),
            "decreasing recursion should be accepted: {:?}",
            r.errors
        );
    }

    #[test]
    fn rejects_non_decreasing_self_call() {
        let src = "fn f(n: f64) -> f64 { return f(n + 1.0); }";
        let r = check_src(src);
        assert!(!r.ok(), "non-decreasing self call must be rejected");
        assert!(r
            .errors
            .iter()
            .any(|e| e.message.contains("may not terminate")));
    }

    #[test]
    fn rejects_mutual_recursion() {
        let src = "fn a(n: f64) -> f64 { return b(n); }
        fn b(n: f64) -> f64 { return a(n); }";
        let r = check_src(src);
        assert!(!r.ok(), "mutual recursion must be rejected");
    }

    // --- Phase 5: path-constraint propagation, contradictory requires, lemma. ---

    #[test]
    fn if_else_propagates_path_constraints() {
        let src = "fn f(a: f64) -> f64 requires a > 0.0 {
            if a > 10.0 { return a / a; }
            else { return a / a; }
        }";
        let r = check_src(src);
        assert!(
            r.ok(),
            "both branches should inherit a > 0.0: {:?}",
            r.errors
        );
    }

    #[test]
    fn contradictory_requires_is_rejected() {
        let src = "fn f(a: f64) -> f64 requires a > 0.0 && a < 0.0 { return a; }";
        let r = check_src(src);
        assert!(!r.ok(), "contradictory requires must be rejected");
        assert!(r.errors.iter().any(|e| e.message.contains("contradictory")));
    }

    #[test]
    fn isolated_lemma_apply_to() {
        let nv = Lemma {
            name: "normalized_vector",
            apply: lemma_normalized_vector,
        };
        let ctx: Vec<Constraint> = vec![];
        let pred = parse_expr("[0.0, 0.0].magnitude() <= 1.0").unwrap();
        let sides = nv.apply_to(&pred, &ctx);
        assert!(
            sides.is_some(),
            "normalized_vector should match magnitude <= 1.0"
        );
        assert!(sides.unwrap().is_empty(), "no side conditions expected");
    }
}
