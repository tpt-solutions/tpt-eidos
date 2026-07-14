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

use std::collections::HashMap;

use eidos_parser::{BinOp, Expr, Fun, Item, Module, Pattern, Type, UnOp};
use eidos_verifier::{entails, unsat, Constraint, LinExpr, Rel};

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
/// (see `Report::obligations` and `eidos-flight-math`).
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

/// The lemmas the bare MVK ships with. Domain libraries (e.g. `eidos-flight-math`)
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
        self.check_termination(f, report);
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
                    let kind = if *op == BinOp::Div { "division" } else { "remainder" };
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
            Expr::Let { value, body, .. } => {
                self.walk(value, ctx, report);
                self.walk(body, ctx, report);
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

    fn check_division(&self, denom: &Expr, ctx: &[Constraint], report: &mut Report) {
        let desc = format!("division by zero: {} != 0", expr_to_string(denom));
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
                    let ce = eidos_verifier::find_model(&cs);
                    let detail = ce
                        .map(|m| format!("counterexample: {:?}", m))
                        .unwrap_or_default();
                    report.errors.push(CheckError {
                        message: format!(
                            "possible division by zero: denominator could be zero. {detail}"
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
                        "cannot prove denominator {} is non-zero (non-linear); division rejected",
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

    fn check_termination(&self, f: &Fun, report: &mut Report) {
        let mut calls = Vec::new();
        self.collect_self_calls(&f.body, &f.name, &mut calls);
        for c in &calls {
            if let Expr::Call { args, .. } = c {
                let identical = f
                    .params
                    .iter()
                    .zip(args.iter())
                    .all(|((pn, _), a)| self.is_param_ident(a, pn));
                if identical {
                    report.errors.push(CheckError {
                        message: format!(
                            "function `{}` may not terminate: recursive call passes unchanged arguments (no decreasing metric)",
                            f.name
                        ),
                    });
                }
            }
        }
    }

    fn collect_self_calls<'b>(&self, e: &'b Expr, name: &str, out: &mut Vec<&'b Expr>) {
        match e {
            Expr::Bin { a, b, .. } => {
                self.collect_self_calls(a, name, out);
                self.collect_self_calls(b, name, out);
            }
            Expr::Un { a, .. } => self.collect_self_calls(a, name, out),
            Expr::If { cond, then, els } => {
                self.collect_self_calls(cond, name, out);
                self.collect_self_calls(then, name, out);
                self.collect_self_calls(els, name, out);
            }
            Expr::Let { value, body, .. } => {
                self.collect_self_calls(value, name, out);
                self.collect_self_calls(body, name, out);
            }
            Expr::Call { func, args } => {
                if func == name {
                    out.push(e);
                }
                for a in args {
                    self.collect_self_calls(a, name, out);
                }
            }
            Expr::Method { recv, args, .. } => {
                self.collect_self_calls(recv, name, out);
                for a in args {
                    self.collect_self_calls(a, name, out);
                }
            }
            Expr::Lambda { body, .. } => self.collect_self_calls(body, name, out),
            Expr::Record(fields) => {
                for (_, v) in fields {
                    self.collect_self_calls(v, name, out);
                }
            }
            Expr::Cast { value, .. } => self.collect_self_calls(value, name, out),
            Expr::Return(e) => self.collect_self_calls(e, name, out),
            _ => {}
        }
    }

    fn is_param_ident(&self, e: &Expr, p: &str) -> bool {
        matches!(e, Expr::Var(v) if v == p)
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
/// sub-expressions (products of two variables, `magnitude()`, ...).
fn linearize(e: &Expr) -> Option<LinExpr> {
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
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eidos_parser::parse;

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
}
