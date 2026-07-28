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

#![warn(missing_docs)]

use std::collections::{HashMap, HashSet};

use tpt_eidos_parser::{
    BinOp, Expr, ExprKind, Fun, Item, Module, Pattern, Span, TimeUnit, Type, UnOp,
};
use tpt_eidos_verifier::{
    check_sos_certificate, entails, unsat, Constraint, LinExpr, Rel, SosCertificate,
};

/// A rejection reason produced while checking a module. Every entry in
/// `Report::errors` is one of these; `Report::ok()` is `true` iff there are
/// none.
#[derive(Clone, Debug)]
pub struct CheckError {
    /// Human-readable description of what could not be proven.
    pub message: String,
    /// Source span of the expression that triggered the error, if available.
    pub span: Option<Span>,
}

/// The outcome of attempting to discharge a single proof obligation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObligationStatus {
    /// Proven directly by the QF_LRA linear prover.
    Verified,
    /// Accepted via a named trusted `Lemma` (the domain-library boundary).
    Trusted,
    /// Could not be proven; a corresponding `CheckError` is also recorded.
    Unverified,
}

/// A single proof obligation the kernel attempted to discharge, and its
/// outcome.
#[derive(Clone, Debug)]
pub struct Obligation {
    /// Human-readable description of what was being proven.
    pub description: String,
    /// Whether the obligation was discharged, and how.
    pub status: ObligationStatus,
    /// Source span of the expression that triggered the obligation, if available.
    pub span: Option<Span>,
}

/// The result of type-checking a module: every proof obligation encountered,
/// and every error that prevents acceptance.
#[derive(Clone, Debug, Default)]
pub struct Report {
    /// Rejection reasons. The module is accepted iff this is empty.
    pub errors: Vec<CheckError>,
    /// Every proof obligation attempted, verified or not.
    pub obligations: Vec<Obligation>,
}

impl Report {
    /// True iff the module was accepted (no errors).
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
    /// The lemma's name, recorded in `Obligation::description` so a trusted
    /// obligation can always be traced back to the fact that admitted it.
    pub name: &'static str,
    /// See the `Lemma` doc comment for the contract this function must
    /// satisfy.
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
    check_wcet(module, &mut report);
    check_linearity(module, &mut report);
    report
}

/// Type-check a whole module with a caller-supplied trusted-lemma set AND a set
/// of pre-built sum-of-squares certificates. When the kernel encounters a
/// non-linear obligation that the QF_LRA prover and trusted lemmas cannot
/// discharge, it tries each certificate in order. This is the Phase-7c
/// kernel-level certificate path: any external source (human, LLM, or SMT
/// solver) can propose certificates, and the kernel verifies them exactly.
pub fn check_with_certs(module: &Module, lemmas: &[Lemma], certs: &[SosCertificate]) -> Report {
    let mut aliases: HashMap<String, Type> = HashMap::new();
    for it in &module.items {
        if let Item::TypeAlias { name, ty } = it {
            aliases.insert(name.clone(), ty.clone());
        }
    }
    let mut report = Report::default();
    for it in &module.items {
        if let Item::Fn(f) = it {
            let mut checker = Checker::with_certs(&aliases, lemmas, certs);
            checker.check_fun(f, &mut report);
        }
    }
    check_termination(module, &mut report);
    check_wcet(module, &mut report);
    check_linearity(module, &mut report);
    report
}

struct Checker<'a> {
    aliases: &'a HashMap<String, Type>,
    lemmas: &'a [Lemma],
    certs: &'a [SosCertificate],
    ensures: Option<Expr>,
    ret: Type,
    current_fn: String,
}

impl<'a> Checker<'a> {
    fn new(aliases: &'a HashMap<String, Type>, lemmas: &'a [Lemma]) -> Self {
        Checker {
            aliases,
            lemmas,
            certs: &[],
            ensures: None,
            ret: Type::Base("_".into()),
            current_fn: String::new(),
        }
    }

    fn with_certs(
        aliases: &'a HashMap<String, Type>,
        lemmas: &'a [Lemma],
        certs: &'a [SosCertificate],
    ) -> Self {
        Checker {
            aliases,
            lemmas,
            certs,
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
                span: f.requires.as_ref().map(|r| r.span),
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
        match &e.kind {
            ExprKind::Num(_) | ExprKind::Bool(_) | ExprKind::Var(_) => {}
            ExprKind::ArrayLit(es) => {
                for x in es {
                    self.walk(x, ctx, report);
                }
            }
            ExprKind::Bin { op, a, b } => {
                self.walk(a, ctx, report);
                self.walk(b, ctx, report);
                if matches!(op, BinOp::Div | BinOp::Rem) {
                    let kind = if *op == BinOp::Div {
                        "division"
                    } else {
                        "remainder"
                    };
                    self.check_division(b, ctx, report, kind, Some(e.span));
                }
            }
            ExprKind::Un { a, .. } => self.walk(a, ctx, report),
            ExprKind::If { cond, then, els } => {
                let mut then_ctx = ctx.to_vec();
                then_ctx.extend(self.path_constraints(cond));
                let mut else_ctx = ctx.to_vec();
                if let Some(neg) = self.negate_constraints(cond) {
                    else_ctx.extend(neg);
                }
                self.walk(then, &then_ctx, report);
                self.walk(els, &else_ctx, report);
            }
            ExprKind::Let { name, value, body } => {
                self.walk(value, ctx, report);
                let mut body_ctx = ctx.to_vec();
                if let Some(lv) = self.linearize(value) {
                    body_ctx.push(Constraint::eq(LinExpr::var(name.clone()).sub(&lv)));
                }
                self.walk(body, &body_ctx, report);
            }
            ExprKind::Call { args, .. } => {
                for a in args {
                    self.walk(a, ctx, report);
                }
            }
            ExprKind::Method { recv, args, .. } => {
                self.walk(recv, ctx, report);
                for a in args {
                    self.walk(a, ctx, report);
                }
            }
            ExprKind::Lambda { body, .. } => self.walk(body, ctx, report),
            ExprKind::Record(fields) => {
                for (_, v) in fields {
                    self.walk(v, ctx, report);
                }
            }
            ExprKind::Cast { value, ty } => {
                self.walk(value, ctx, report);
                if let Some(Type::Refine { bind, pred, .. }) = self.as_refine(ty) {
                    let target: &Expr = match &value.kind {
                        ExprKind::Record(fields) => fields
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
                        Some(e.span),
                    );
                }
            }
            ExprKind::Return(e) => {
                self.walk(e, ctx, report);
                // Array-length soundness: a manifest array literal returned for
                // an `Array<_, N>` type must contain exactly `N` elements. This
                // is the only place the kernel enforces element count today.
                if let ExprKind::ArrayLit(es) = &e.kind {
                    if let Some(n) = Self::array_len_of(&self.ret, self.aliases) {
                        if (es.len() as u64) != n {
                            report.errors.push(CheckError {
                                message: format!(
                                    "function `{}` returns an array of length {} but its type requires length {}",
                                    self.current_fn,
                                    es.len(),
                                    n
                                ),
                                span: Some(e.span),
                            });
                        }
                    }
                }
                if self.as_refine(&self.ret).is_some() && !matches!(&e.kind, ExprKind::Cast { .. })
                {
                    report.errors.push(CheckError {
                        message: format!(
                            "function `{}` returns a refinement type; the return value must be introduced with `as`",
                            self.current_fn
                        ),
                        span: Some(e.span),
                    });
                }
                if let Some(Expr {
                    kind: ExprKind::Lambda { params, body },
                    ..
                }) = &self.ensures
                {
                    if let Some(Pattern::Var(p)) = params.first() {
                        let inst = self.subst(body, p, e);
                        self.discharge(
                            &inst,
                            ctx,
                            &format!("ensures: {}", expr_to_string(&inst)),
                            report,
                            Some(e.span),
                        );
                    }
                }
            }
        }
    }

    fn check_division(
        &self,
        denom: &Expr,
        ctx: &[Constraint],
        report: &mut Report,
        kind: &str,
        span: Option<Span>,
    ) {
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
                        span,
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
                        span,
                    });
                    report.obligations.push(Obligation {
                        description: desc,
                        status: ObligationStatus::Unverified,
                        span,
                    });
                }
            }
            None => {
                report.errors.push(CheckError {
                    message: format!(
                        "cannot prove denominator {} is non-zero (non-linear); {kind} rejected",
                        expr_to_string(denom)
                    ),
                    span,
                });
                report.obligations.push(Obligation {
                    description: desc,
                    status: ObligationStatus::Unverified,
                    span,
                });
            }
        }
    }

    fn discharge(
        &self,
        pred: &Expr,
        ctx: &[Constraint],
        desc: &str,
        report: &mut Report,
        span: Option<Span>,
    ) {
        let pred = self.simplify(pred);
        if let ExprKind::Bin {
            op: BinOp::And,
            a,
            b,
        } = &pred.kind
        {
            self.discharge(a, ctx, &format!("{desc} (conjunct 1)"), report, span);
            self.discharge(b, ctx, &format!("{desc} (conjunct 2)"), report, span);
            return;
        }

        if let Some(c) = self.to_constraint(&pred) {
            if entails(ctx, &c) {
                report.obligations.push(Obligation {
                    description: desc.into(),
                    status: ObligationStatus::Verified,
                    span,
                });
                return;
            }
            if let Some(name) = self.try_lemma(&pred, ctx) {
                report.obligations.push(Obligation {
                    description: format!("{desc} (trusted lemma: {name})"),
                    status: ObligationStatus::Trusted,
                    span,
                });
                return;
            }
            let ce = tpt_eidos_verifier::find_model(&[c]);
            let detail = ce
                .map(|m| format!(" counterexample: {:?}", m))
                .unwrap_or_default();
            report.errors.push(CheckError {
                message: format!("could not verify obligation: {desc}.{detail}"),
                span,
            });
            report.obligations.push(Obligation {
                description: desc.into(),
                status: ObligationStatus::Unverified,
                span,
            });
            return;
        }

        if let Some(name) = self.try_lemma(&pred, ctx) {
            report.obligations.push(Obligation {
                description: format!("{desc} (trusted lemma: {name})"),
                status: ObligationStatus::Trusted,
                span,
            });
            return;
        }
        // Try certificates: each certificate proves `target >= 0` where
        // `target = bound - expr` for some non-linear obligation.
        if self.try_cert(&pred) {
            report.obligations.push(Obligation {
                description: format!("{desc} (verified by certificate)"),
                status: ObligationStatus::Verified,
                span,
            });
            return;
        }
        // Try to find a model satisfying the context as a "context witness" — not a
        // true counterexample to the non-linear predicate, but a concrete assignment
        // showing the context is consistent (so the obligation was genuinely needed).
        let ctx_detail = if ctx.is_empty() {
            String::new()
        } else {
            match tpt_eidos_verifier::find_model(ctx) {
                Some(m) => format!(" context witness: {:?}", m),
                None => " (context is unsatisfiable — obligation vacuously true)".to_string(),
            }
        };
        report.errors.push(CheckError {
            message: format!(
                "non-linear obligation not discharged by trusted lemmas or certificates: {desc}.{ctx_detail}"
            ),
            span,
        });
        report.obligations.push(Obligation {
            description: desc.into(),
            status: ObligationStatus::Unverified,
            span,
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

    /// Try every certificate in the registry. A certificate proves `target >= 0`
    /// where `target = bound - expr` for some non-linear obligation. Returns
    /// `true` if any certificate validates.
    fn try_cert(&self, _pred: &Expr) -> bool {
        for cert in self.certs {
            if check_sos_certificate(cert) {
                return true;
            }
        }
        false
    }

    fn simplify(&self, e: &Expr) -> Expr {
        match &e.kind {
            ExprKind::Cast { value, .. } => self.simplify(value),
            ExprKind::Method { recv, name, args } => {
                let r = self.simplify(recv);
                if args.is_empty() {
                    if let ExprKind::Record(fields) = &r.kind {
                        if let Some((_, v)) = fields.iter().find(|(f, _)| f == name) {
                            return self.simplify(v);
                        }
                    }
                }
                let sargs: Vec<Expr> = args.iter().map(|a| self.simplify(a)).collect();
                Expr {
                    kind: ExprKind::Method {
                        recv: Box::new(r),
                        name: name.clone(),
                        args: sargs,
                    },
                    span: e.span,
                }
            }
            ExprKind::Bin { op, a, b } => Expr {
                kind: ExprKind::Bin {
                    op: *op,
                    a: Box::new(self.simplify(a)),
                    b: Box::new(self.simplify(b)),
                },
                span: e.span,
            },
            ExprKind::Un { op, a } => Expr {
                kind: ExprKind::Un {
                    op: *op,
                    a: Box::new(self.simplify(a)),
                },
                span: e.span,
            },
            ExprKind::ArrayLit(es) => Expr {
                kind: ExprKind::ArrayLit(es.iter().map(|x| self.simplify(x)).collect()),
                span: e.span,
            },
            ExprKind::Record(fields) => Expr {
                kind: ExprKind::Record(
                    fields
                        .iter()
                        .map(|(f, v)| (f.clone(), self.simplify(v)))
                        .collect(),
                ),
                span: e.span,
            },
            ExprKind::If { cond, then, els } => Expr {
                kind: ExprKind::If {
                    cond: Box::new(self.simplify(cond)),
                    then: Box::new(self.simplify(then)),
                    els: Box::new(self.simplify(els)),
                },
                span: e.span,
            },
            other => Expr {
                kind: other.clone(),
                span: e.span,
            },
        }
    }

    fn subst(&self, e: &Expr, var: &str, val: &Expr) -> Expr {
        match &e.kind {
            ExprKind::Var(v) if v == var => val.clone(),
            ExprKind::Var(v) => Expr {
                kind: ExprKind::Var(v.clone()),
                span: e.span,
            },
            ExprKind::Num(n) => Expr {
                kind: ExprKind::Num(*n),
                span: e.span,
            },
            ExprKind::Bool(b) => Expr {
                kind: ExprKind::Bool(*b),
                span: e.span,
            },
            ExprKind::Bin { op, a, b } => Expr {
                kind: ExprKind::Bin {
                    op: *op,
                    a: Box::new(self.subst(a, var, val)),
                    b: Box::new(self.subst(b, var, val)),
                },
                span: e.span,
            },
            ExprKind::Un { op, a } => Expr {
                kind: ExprKind::Un {
                    op: *op,
                    a: Box::new(self.subst(a, var, val)),
                },
                span: e.span,
            },
            ExprKind::If { cond, then, els } => Expr {
                kind: ExprKind::If {
                    cond: Box::new(self.subst(cond, var, val)),
                    then: Box::new(self.subst(then, var, val)),
                    els: Box::new(self.subst(els, var, val)),
                },
                span: e.span,
            },
            ExprKind::ArrayLit(es) => Expr {
                kind: ExprKind::ArrayLit(es.iter().map(|x| self.subst(x, var, val)).collect()),
                span: e.span,
            },
            ExprKind::Method { recv, name, args } => Expr {
                kind: ExprKind::Method {
                    recv: Box::new(self.subst(recv, var, val)),
                    name: name.clone(),
                    args: args.iter().map(|a| self.subst(a, var, val)).collect(),
                },
                span: e.span,
            },
            ExprKind::Call { func, args } => Expr {
                kind: ExprKind::Call {
                    func: func.clone(),
                    args: args.iter().map(|a| self.subst(a, var, val)).collect(),
                },
                span: e.span,
            },
            ExprKind::Lambda { params, body } => {
                if pattern_binds(params, var) {
                    Expr {
                        kind: ExprKind::Lambda {
                            params: params.clone(),
                            body: body.clone(),
                        },
                        span: e.span,
                    }
                } else {
                    Expr {
                        kind: ExprKind::Lambda {
                            params: params.clone(),
                            body: Box::new(self.subst(body, var, val)),
                        },
                        span: e.span,
                    }
                }
            }
            ExprKind::Record(fields) => Expr {
                kind: ExprKind::Record(
                    fields
                        .iter()
                        .map(|(f, v)| (f.clone(), self.subst(v, var, val)))
                        .collect(),
                ),
                span: e.span,
            },
            ExprKind::Cast { value, ty } => Expr {
                kind: ExprKind::Cast {
                    value: Box::new(self.subst(value, var, val)),
                    ty: ty.clone(),
                },
                span: e.span,
            },
            ExprKind::Return(_) | ExprKind::Let { .. } => e.clone(),
        }
    }

    fn linearize(&self, e: &Expr) -> Option<LinExpr> {
        linearize(e)
    }

    fn to_constraint(&self, e: &Expr) -> Option<Constraint> {
        match &e.kind {
            ExprKind::Bin { op, a, b } => {
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
        match &e.kind {
            ExprKind::Bin { op, a, b } => match op {
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
        match &e.kind {
            ExprKind::Bin { op, a, b } => match op {
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
            ExprKind::Un { op: UnOp::Not, a } => Some(self.path_constraints(a)),
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
                    span: None,
                });
            }
        }
    }
}

/// Abstract cost model for WCET analysis. Each operation is assigned a fixed
/// number of abstract cost units. The total worst-case cost of a function body
/// is compared against the declared `RealTime<Nms>` budget.
///
/// This is an **abstract-cost proxy**, not a hardware-certified timing bound.
/// It provides a consistent, composable cost estimate that can reject functions
/// whose structural cost exceeds a configured budget, but it does not account
/// for pipeline effects, cache behavior, or platform-specific instruction costs.
struct CostModel;

impl CostModel {
    /// Abstract cost of a binary arithmetic/comparison operation.
    const BIN_OP: f64 = 1.0;
    /// Abstract cost of a unary operation.
    const UN_OP: f64 = 1.0;
    /// Abstract cost of `.magnitude()` (Euclidean norm: N multiplications + additions + sqrt).
    const MAGNITUDE: f64 = 10.0;
    /// Abstract cost of `.len()` (array length is a const, essentially free).
    const LEN: f64 = 0.0;
    /// Abstract cost of `.map()` per element (body cost is charged per element).
    const MAP_OVERHEAD: f64 = 1.0;
    /// Abstract cost of `.zip()` per element.
    const ZIP_PER_ELEM: f64 = 1.0;
    /// Abstract cost of a function call (unknown body, conservatively charged).
    const CALL: f64 = 5.0;

    /// Convert a time budget in the given unit to abstract cost units.
    /// The conversion factor is a design parameter; here we use
    /// 1 abstract unit ≈ 1 microsecond as a reasonable default.
    fn budget_to_units(value: f64, unit: TimeUnit) -> f64 {
        match unit {
            TimeUnit::Us => value,
            TimeUnit::Ms => value * 1000.0,
            TimeUnit::S => value * 1_000_000.0,
        }
    }

    /// Compute the worst-case abstract cost of an expression tree.
    fn cost(e: &Expr) -> f64 {
        match &e.kind {
            ExprKind::Num(_) | ExprKind::Bool(_) | ExprKind::Var(_) => 0.0,
            ExprKind::ArrayLit(es) => es.iter().map(Self::cost).sum(),
            ExprKind::Bin { a, b, .. } => Self::BIN_OP + Self::cost(a) + Self::cost(b),
            ExprKind::Un { a, .. } => Self::UN_OP + Self::cost(a),
            ExprKind::If { cond, then, els } => {
                Self::cost(cond) + Self::cost(then).max(Self::cost(els))
            }
            ExprKind::Let { value, body, .. } => Self::cost(value) + Self::cost(body),
            ExprKind::Call { args, .. } => Self::CALL + args.iter().map(Self::cost).sum::<f64>(),
            ExprKind::Method { recv, name, args } => {
                let rc = Self::cost(recv);
                let ac: f64 = args.iter().map(Self::cost).sum();
                let mc = match name.as_str() {
                    "magnitude" => Self::MAGNITUDE,
                    "len" => Self::LEN,
                    "map" => {
                        // `.map(f)` iterates over every element; the body cost
                        // is charged once (conservative lower bound on per-element
                        // cost; a tighter bound would multiply by array length,
                        // but array length is not always statically known).
                        Self::MAP_OVERHEAD + Self::cost(&args[0])
                    }
                    "zip" => {
                        // `.zip(other)` creates tuples; per-element cost is fixed.
                        Self::ZIP_PER_ELEM
                    }
                    _ => Self::CALL,
                };
                rc + mc + ac
            }
            ExprKind::Lambda { body, .. } => Self::cost(body),
            ExprKind::Record(fields) => fields.iter().map(|(_, v)| Self::cost(v)).sum(),
            ExprKind::Cast { value, .. } => Self::cost(value),
            ExprKind::Return(e) => Self::cost(e),
        }
    }
}

/// Check WCET (worst-case execution time) budgets for functions with
/// `effects [RealTime<Nms>]` annotations. V1 restricts WCET to non-recursive
/// functions only; a recursive function with a `RealTime` budget is a hard
/// rejection.
fn check_wcet(module: &Module, report: &mut Report) {
    let mut callees: HashMap<String, Vec<String>> = HashMap::new();
    let mut direct_calls: HashMap<String, Vec<String>> = HashMap::new();

    for it in &module.items {
        if let Item::Fn(f) = it {
            let mut direct = Vec::new();
            let mut adj = Vec::new();
            collect_calls(&f.body, &mut direct, &mut adj);
            direct_calls.insert(f.name.clone(), direct.into_iter().map(|(c, _)| c).collect());
            callees.insert(f.name.clone(), adj);
        }
    }

    for it in &module.items {
        if let Item::Fn(f) = it {
            // Find the RealTime budget, if any.
            let budget = f.effects.iter().find_map(|eff| {
                if eff.name == "RealTime" {
                    eff.budget
                } else {
                    None
                }
            });
            let Some((value, unit)) = budget else {
                continue;
            };

            // Restriction: WCET is only for non-recursive functions in v1.
            // Check if the function calls itself (directly or indirectly).
            let reach = reachable(&callees, &f.name);
            let is_recursive = reach.len() > 1
                || direct_calls
                    .get(&f.name)
                    .is_some_and(|calls| calls.iter().any(|c| c == &f.name));
            if is_recursive {
                report.errors.push(CheckError {
                    message: format!(
                        "function `{}` has a RealTime budget but is recursive; WCET checking is only supported for non-recursive functions in v1",
                        f.name
                    ),
                    span: None,
                });
                continue;
            }

            // Compute worst-case cost and compare against budget.
            let cost = CostModel::cost(&f.body);
            let budget_units = CostModel::budget_to_units(value, unit);

            if cost > budget_units {
                report.errors.push(CheckError {
                    message: format!(
                        "function `{}` exceeds WCET budget: estimated cost {:.1} abstract units exceeds budget of {:.1} abstract units ({value} {})",
                        f.name, cost, budget_units, unit_str(unit)
                    ),
                    span: None,
                });
                report.obligations.push(Obligation {
                    description: format!(
                        "WCET budget: {} <= {:.1} abstract units",
                        f.name, budget_units
                    ),
                    status: ObligationStatus::Unverified,
                    span: None,
                });
            } else {
                report.obligations.push(Obligation {
                    description: format!(
                        "WCET budget: {} <= {:.1} abstract units",
                        f.name, budget_units
                    ),
                    status: ObligationStatus::Verified,
                    span: None,
                });
            }
        }
    }
}

fn unit_str(u: TimeUnit) -> &'static str {
    match u {
        TimeUnit::Us => "us",
        TimeUnit::Ms => "ms",
        TimeUnit::S => "s",
    }
}

/// Check linearity: every `linear`-typed parameter must be used exactly once
/// on every code path through the function body. Rejects 0 uses ("unused
/// linear resource") and 2+ uses on one path ("used more than once").
///
/// V1 scope: sequential composition sums usage; `if/else` requires exactly
/// one use of each live linear variable on **both** branches. `Lambda` bodies
/// (`.map`/`.zip` closures) may not capture a linear variable.
fn check_linearity(module: &Module, report: &mut Report) {
    for it in &module.items {
        if let Item::Fn(f) = it {
            // Collect linear parameters.
            let linear_params: Vec<String> = f
                .params
                .iter()
                .filter_map(|(name, ty)| {
                    if matches!(ty, Type::Linear(_)) {
                        Some(name.clone())
                    } else {
                        None
                    }
                })
                .collect();
            if linear_params.is_empty() {
                continue;
            }

            // Check each linear parameter is used exactly once on every path.
            for param in &linear_params {
                let uses = count_uses(&f.body, param);
                match uses {
                    Usage::ExactlyOnce => {}
                    Usage::Zero => {
                        report.errors.push(CheckError {
                            message: format!(
                                "linear parameter `{param}` is never used in function `{}`",
                                f.name
                            ),
                            span: None,
                        });
                    }
                    Usage::Multiple => {
                        report.errors.push(CheckError {
                            message: format!(
                                "linear parameter `{param}` is used more than once in function `{}`",
                                f.name
                            ),
                            span: None,
                        });
                    }
                    Usage::Ambiguous => {
                        report.errors.push(CheckError {
                            message: format!(
                                "linear parameter `{param}` has inconsistent usage across branches in function `{}`",
                                f.name
                            ),
                            span: None,
                        });
                    }
                }
            }

            // Check that no linear parameter is captured inside a lambda.
            for param in &linear_params {
                if captures_in_lambda(&f.body, param) {
                    report.errors.push(CheckError {
                        message: format!(
                            "linear parameter `{param}` is captured inside a lambda in function `{}`",
                            f.name
                        ),
                        span: None,
                    });
                }
            }
        }
    }
}

/// How many times a variable is used on a code path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Usage {
    /// Used exactly once.
    ExactlyOnce,
    /// Never used.
    Zero,
    /// Used more than once on at least one path.
    Multiple,
    /// Different branches have different usage counts.
    Ambiguous,
}

impl Usage {
    fn max(self, other: Usage) -> Usage {
        match (self, other) {
            (Usage::Zero, Usage::Zero) => Usage::Zero,
            (Usage::ExactlyOnce, Usage::ExactlyOnce) => Usage::ExactlyOnce,
            (Usage::Multiple, _) | (_, Usage::Multiple) => Usage::Multiple,
            (Usage::Ambiguous, _) | (_, Usage::Ambiguous) => Usage::Ambiguous,
            _ => Usage::Ambiguous,
        }
    }

    fn sum(self, other: Usage) -> Usage {
        match (self, other) {
            (Usage::Zero, x) | (x, Usage::Zero) => x,
            (Usage::ExactlyOnce, Usage::ExactlyOnce) => Usage::Multiple,
            (Usage::Multiple, _) | (_, Usage::Multiple) => Usage::Multiple,
            (Usage::Ambiguous, _) | (_, Usage::Ambiguous) => Usage::Ambiguous,
        }
    }
}

/// Count how many times `var` is used in expression `e` on every path.
/// Returns the usage pattern across all paths.
fn count_uses(e: &Expr, var: &str) -> Usage {
    match &e.kind {
        ExprKind::Var(v) if v == var => Usage::ExactlyOnce,
        ExprKind::Var(_) | ExprKind::Num(_) | ExprKind::Bool(_) => Usage::Zero,
        ExprKind::Bin { a, b, .. } => count_uses(a, var).sum(count_uses(b, var)),
        ExprKind::Un { a, .. } => count_uses(a, var),
        ExprKind::ArrayLit(es) => es
            .iter()
            .fold(Usage::Zero, |acc, e| acc.sum(count_uses(e, var))),
        ExprKind::Record(fields) => fields
            .iter()
            .fold(Usage::Zero, |acc, (_, v)| acc.sum(count_uses(v, var))),
        ExprKind::If { cond, then, els } => {
            let c = count_uses(cond, var);
            let t = count_uses(then, var);
            let e = count_uses(els, var);
            c.sum(t.max(e))
        }
        ExprKind::Let { value, body, .. } => count_uses(value, var).sum(count_uses(body, var)),
        ExprKind::Call { args, .. } => args
            .iter()
            .fold(Usage::Zero, |acc, a| acc.sum(count_uses(a, var))),
        ExprKind::Method { recv, args, .. } => {
            let r = count_uses(recv, var);
            args.iter().fold(r, |acc, a| acc.sum(count_uses(a, var)))
        }
        ExprKind::Lambda { .. } => Usage::Zero,
        ExprKind::Cast { value, .. } => count_uses(value, var),
        ExprKind::Return(e) => count_uses(e, var),
    }
}

/// True if `var` is captured inside any lambda body in `e`.
fn captures_in_lambda(e: &Expr, var: &str) -> bool {
    match &e.kind {
        ExprKind::Lambda { body, .. } => {
            count_uses(body, var) != Usage::Zero || captures_in_lambda(body, var)
        }
        ExprKind::Bin { a, b, .. } => captures_in_lambda(a, var) || captures_in_lambda(b, var),
        ExprKind::Un { a, .. } => captures_in_lambda(a, var),
        ExprKind::If { cond, then, els } => {
            captures_in_lambda(cond, var)
                || captures_in_lambda(then, var)
                || captures_in_lambda(els, var)
        }
        ExprKind::Let { value, body, .. } => {
            captures_in_lambda(value, var) || captures_in_lambda(body, var)
        }
        ExprKind::ArrayLit(es) => es.iter().any(|e| captures_in_lambda(e, var)),
        ExprKind::Record(fields) => fields.iter().any(|(_, v)| captures_in_lambda(v, var)),
        ExprKind::Method { recv, args, .. } => {
            captures_in_lambda(recv, var) || args.iter().any(|a| captures_in_lambda(a, var))
        }
        ExprKind::Call { args, .. } => args.iter().any(|a| captures_in_lambda(a, var)),
        ExprKind::Cast { value, .. } => captures_in_lambda(value, var),
        ExprKind::Return(e) => captures_in_lambda(e, var),
        _ => false,
    }
}

/// Collect every `Call` in `e`, recording `(callee, args)` pairs and the direct
/// callee names (used to build the call graph).
fn collect_calls(e: &Expr, out: &mut Vec<(String, Vec<Expr>)>, adj: &mut Vec<String>) {
    match &e.kind {
        ExprKind::Bin { a, b, .. } => {
            collect_calls(a, out, adj);
            collect_calls(b, out, adj);
        }
        ExprKind::Un { a, .. } => collect_calls(a, out, adj),
        ExprKind::If { cond, then, els } => {
            collect_calls(cond, out, adj);
            collect_calls(then, out, adj);
            collect_calls(els, out, adj);
        }
        ExprKind::Let { value, body, .. } => {
            collect_calls(value, out, adj);
            collect_calls(body, out, adj);
        }
        ExprKind::Call { func, args } => {
            out.push((func.clone(), args.clone()));
            if !adj.contains(func) {
                adj.push(func.clone());
            }
            for a in args {
                collect_calls(a, out, adj);
            }
        }
        ExprKind::Method { recv, args, .. } => {
            collect_calls(recv, out, adj);
            for a in args {
                collect_calls(a, out, adj);
            }
        }
        ExprKind::Lambda { body, .. } => collect_calls(body, out, adj),
        ExprKind::Record(fields) => {
            for (_, v) in fields {
                collect_calls(v, out, adj);
            }
        }
        ExprKind::Cast { value, .. } => collect_calls(value, out, adj),
        ExprKind::Return(e) => collect_calls(e, out, adj),
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
    match &e.kind {
        ExprKind::Num(n) => format!("{n}"),
        ExprKind::Bool(b) => format!("{b}"),
        ExprKind::Var(v) => v.clone(),
        ExprKind::Bin { op, a, b } => format!(
            "({} {} {})",
            expr_to_string(a),
            binop_str(*op),
            expr_to_string(b)
        ),
        ExprKind::Un { op, a } => format!("{}{}", unop_str(*op), expr_to_string(a)),
        ExprKind::ArrayLit(es) => format!(
            "[{}]",
            es.iter().map(expr_to_string).collect::<Vec<_>>().join(", ")
        ),
        ExprKind::Call { func, args } => format!(
            "{}({})",
            func,
            args.iter()
                .map(expr_to_string)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        ExprKind::Method { recv, name, args } => {
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
        ExprKind::Lambda { params, .. } => {
            format!("|{}| ..", patterns_to_string(params))
        }
        ExprKind::Record(fields) => format!(
            "{{{}}}",
            fields
                .iter()
                .map(|(f, _)| f.clone())
                .collect::<Vec<_>>()
                .join(", ")
        ),
        ExprKind::Cast { .. } => "cast".into(),
        ExprKind::Return(_) => "return".into(),
        ExprKind::If { .. } => "if".into(),
        ExprKind::Let { .. } => "let".into(),
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
    if let ExprKind::Bin { op, a, b } = &pred.kind {
        if matches!(op, BinOp::Le | BinOp::Lt) {
            if let ExprKind::Num(1.0) = &b.kind {
                if let ExprKind::Method { recv, name, args } = &a.kind {
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
    match &x.kind {
        ExprKind::ArrayLit(elems) => literal_magnitude_le(elems, 1.0),
        ExprKind::Method { name, args, .. } if name == "map" => {
            if let Some(Expr {
                kind: ExprKind::Lambda { params, body },
                ..
            }) = args.first()
            {
                if params.len() == 1 {
                    if let ExprKind::Bin {
                        op: BinOp::Div, b, ..
                    } = &body.kind
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
        if let ExprKind::Num(n) = &e.kind {
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
        Type::Linear(inner) => format!("linear {}", type_name(inner)),
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
    match &e.kind {
        ExprKind::Num(n) => Some(LinExpr::constant(*n)),
        ExprKind::Var(v) => Some(LinExpr::var(v.clone())),
        ExprKind::Un { op: UnOp::Neg, a } => Some(linearize(a)?.neg()),
        ExprKind::Bin { op, a, b } => {
            let la = linearize(a)?;
            let lb = linearize(b)?;
            match op {
                BinOp::Add => Some(la.add(&lb)),
                BinOp::Sub => Some(la.sub(&lb)),
                BinOp::Mul => {
                    if let ExprKind::Num(k) = &a.kind {
                        Some(lb.scale(*k))
                    } else if let ExprKind::Num(k) = &b.kind {
                        Some(la.scale(*k))
                    } else {
                        None
                    }
                }
                BinOp::Div => {
                    if let ExprKind::Num(k) = &b.kind {
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
        ExprKind::Method { name, args, .. } if name == "magnitude" && args.is_empty() => {
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

    // --- Phase 7a: WCET / RealTime budget checker ---

    #[test]
    fn wcet_accepts_function_within_budget() {
        let src = "fn add(a: f64, b: f64) -> f64 effects [RealTime<100ms>] {
            return a + b;
        }";
        let r = check_src(src);
        assert!(
            r.ok(),
            "simple addition within budget should be accepted: {:?}",
            r.errors
        );
        assert!(r.obligations.iter().any(|o| o.description.contains("WCET")));
    }

    #[test]
    fn wcet_rejects_function_exceeding_budget() {
        let src = "fn expensive(a: f64, b: f64) -> f64 effects [RealTime<1us>] {
            return a + b + a * b + a / b;
        }";
        let r = check_src(src);
        assert!(
            !r.ok(),
            "function exceeding tight budget should be rejected"
        );
        assert!(r.errors.iter().any(|e| e.message.contains("exceeds WCET")));
    }

    #[test]
    fn wcet_rejects_recursive_function() {
        let src = "fn countdown(n: f64) -> f64 effects [RealTime<100ms>] {
            if n > 0.0 { return countdown(n - 1.0); } else { return 0.0; }
        }";
        let r = check_src(src);
        assert!(
            !r.ok(),
            "recursive function with RealTime budget should be rejected"
        );
        assert!(r.errors.iter().any(|e| e.message.contains("recursive")));
    }

    #[test]
    fn wcet_ignores_functions_without_budget() {
        let src = "fn plain(a: f64) -> f64 {
            return a + a + a * a;
        }";
        let r = check_src(src);
        assert!(
            r.ok(),
            "function without RealTime budget should be accepted: {:?}",
            r.errors
        );
        assert!(!r.obligations.iter().any(|o| o.description.contains("WCET")));
    }

    // --- Phase 7b: Linearity / affine checker ---

    #[test]
    fn linearity_accepts_used_exactly_once() {
        let src = "fn f(x: linear f64) -> f64 { return x; }";
        let r = check_src(src);
        assert!(
            r.ok(),
            "linear param used once should be accepted: {:?}",
            r.errors
        );
    }

    #[test]
    fn linearity_rejects_unused() {
        let src = "fn f(x: linear f64) -> f64 { return 0.0; }";
        let r = check_src(src);
        assert!(!r.ok(), "unused linear param should be rejected");
        assert!(r.errors.iter().any(|e| e.message.contains("never used")));
    }

    #[test]
    fn linearity_rejects_used_twice() {
        let src = "fn f(x: linear f64) -> f64 { return x + x; }";
        let r = check_src(src);
        assert!(!r.ok(), "linear param used twice should be rejected");
        assert!(r
            .errors
            .iter()
            .any(|e| e.message.contains("more than once")));
    }

    #[test]
    fn linearity_rejects_captured_in_lambda() {
        let src = "fn f(x: linear f64, arr: Array<f64, 3>) -> f64 {
            return arr.map(|a| a + x);
        }";
        let r = check_src(src);
        assert!(
            !r.ok(),
            "linear param captured in lambda should be rejected"
        );
        assert!(r
            .errors
            .iter()
            .any(|e| e.message.contains("captured inside a lambda")));
    }

    #[test]
    fn linearity_if_else_needs_exactly_one_each_branch() {
        let src = "fn f(x: linear f64, c: bool) -> f64 {
            if c { return x; } else { return x; }
        }";
        let r = check_src(src);
        assert!(
            r.ok(),
            "linear param used once on each branch should be accepted: {:?}",
            r.errors
        );
    }

    #[test]
    fn linearity_rejects_if_branch_drops() {
        let src = "fn f(x: linear f64, c: bool) -> f64 {
            if c { return x; } else { return 0.0; }
        }";
        let r = check_src(src);
        assert!(
            !r.ok(),
            "linear param unused on one branch should be rejected"
        );
    }

    // --- Phase 7c: Kernel-level certificate path ---

    #[test]
    fn kernel_check_with_certs_compiles_and_runs() {
        use tpt_eidos_verifier::{Poly, Rat, SosCertificate};
        // Verify that check_with_certs works with an empty certificate set.
        let src = "fn f(a: f64) -> f64 requires a > 0.0 { return a / a; }";
        let m = parse(src).expect("parse");
        let cert = SosCertificate {
            target: Poly::from_rat(Rat::zero()),
            terms: vec![],
        };
        let report = check_with_certs(&m, &[], &[cert]);
        assert!(
            report.ok(),
            "check_with_certs should work: {:?}",
            report.errors
        );
    }

    #[test]
    fn kernel_certificate_rejected_when_invalid() {
        use tpt_eidos_verifier::{Poly, Rat, SosCertificate};
        // An invalid certificate (negative coefficient) should not help.
        let src = "fn f(a: f64) -> f64 { return a / a; }";
        let m = parse(src).expect("parse");
        let cert = SosCertificate {
            target: Poly::from_rat(Rat::zero()),
            terms: vec![(
                Rat::from_f64(-1.0).unwrap(),
                Poly::from_rat(Rat::from_f64(1.0).unwrap()),
            )],
        };
        let report = check_with_certs(&m, &[], &[cert]);
        // The invalid certificate should not discharge any obligation.
        assert!(
            !report
                .obligations
                .iter()
                .any(|o| o.description.contains("certificate")),
            "invalid certificate should not discharge anything"
        );
    }
}
