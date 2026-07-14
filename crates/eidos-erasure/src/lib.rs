//! Proof-term erasure for the tpt-eidos MVK.
//!
//! The kernel has verified a `Module`. Erasure removes everything that exists
//! only to satisfy the type system — refinement predicates, `as` casts,
//! `requires`/`ensures` contracts, and `effects` labels — leaving a
//! *computational core* that preserves the exact runtime behaviour of the
//! source. This core is what `eidos-codegen` lowers to `no_std` Rust.
//!
//! Erasure is total and type-directed: every surface `Expr` is rewritten to a
//! `CExpr` annotated with its erased `CoreType`, so the code generator never
//! has to re-derive types.

use std::collections::HashMap;

use eidos_parser::{BinOp, Expr, Fun, Item, Module, Pattern, Type, UnOp};

/// A type with all refinement information stripped.
#[derive(Clone, Debug, PartialEq)]
pub enum CoreType {
    /// Primitive/base type, e.g. `f64`, `bool`, `usize`.
    Base(String),
    /// Fixed-length array `[T; N]`.
    Array(Box<CoreType>, u64),
    /// A named type: either a base alias or a generated struct (a refinement
    /// witness). Pointed at by `StructDef` in the module.
    Named(String),
}

/// A generated struct, produced from a refinement type `{ x: T | ... }`.
/// The field `x` carries the computational value of the refinement base `T`.
#[derive(Clone, Debug, PartialEq)]
pub struct StructDef {
    pub name: String,
    pub fields: Vec<(String, CoreType)>,
}

#[derive(Clone, Debug)]
pub struct CExpr {
    pub ty: CoreType,
    pub kind: CExprKind,
}

#[derive(Clone, Debug)]
pub enum CExprKind {
    Num(f64),
    Bool(bool),
    Var(String),
    ArrayLit(Vec<CExpr>),
    Bin {
        op: BinOp,
        a: Box<CExpr>,
        b: Box<CExpr>,
    },
    Un {
        op: UnOp,
        a: Box<CExpr>,
    },
    If {
        cond: Box<CExpr>,
        then: Box<CExpr>,
        els: Box<CExpr>,
    },
    Let {
        name: String,
        value: Box<CExpr>,
        body: Box<CExpr>,
    },
    Call {
        func: String,
        args: Vec<CExpr>,
    },
    /// A method call `recv.name(args)` or a field projection `recv.name` when
    /// `args` is empty and the name is not a built-in helper.
    Method {
        recv: Box<CExpr>,
        name: String,
        args: Vec<CExpr>,
    },
    /// A closure `|params| body`, only ever emitted as a Rust closure inside a
    /// `map`/`zip` chain.
    Lambda {
        params: Vec<Pattern>,
        body: Box<CExpr>,
    },
    /// A record literal. When its type is a generated struct it lowers to a
    /// struct literal; otherwise to an anonymous aggregate.
    Record(Vec<(String, CExpr)>),
    Return(Box<CExpr>),
}

#[derive(Clone, Debug)]
pub struct CoreFun {
    pub name: String,
    pub params: Vec<(String, CoreType)>,
    pub ret: CoreType,
    pub body: CExpr,
}

#[derive(Clone, Debug, Default)]
pub struct CoreModule {
    pub structs: Vec<StructDef>,
    pub fns: Vec<CoreFun>,
}

/// Erase a kernel-checked module to its computational core.
pub fn erase(module: &Module) -> CoreModule {
    let er = Eraser::new(module);
    er.run()
}

struct Eraser<'a> {
    module: &'a Module,
    aliases: HashMap<String, Type>,
    /// alias name -> generated struct name (same string for named refinements).
    struct_of: HashMap<String, String>,
    structs: Vec<StructDef>,
    /// function return types, for `Call` resolution.
    ret_of: HashMap<String, CoreType>,
    counter: usize,
}

impl<'a> Eraser<'a> {
    fn new(module: &'a Module) -> Self {
        let mut aliases = HashMap::new();
        for it in &module.items {
            if let Item::TypeAlias { name, ty } = it {
                aliases.insert(name.clone(), ty.clone());
            }
        }
        Eraser {
            module,
            aliases,
            struct_of: HashMap::new(),
            structs: Vec::new(),
            ret_of: HashMap::new(),
            counter: 0,
        }
    }

    fn run(mut self) -> CoreModule {
        for it in &self.module.items {
            if let Item::TypeAlias { name, ty } = it {
                self.declare_struct_for(name, ty);
            }
        }
        for it in &self.module.items {
            if let Item::Fn(f) = it {
                let ret = self.erase_type(&f.ret);
                self.ret_of.insert(f.name.clone(), ret);
            }
        }
        let mut fns = Vec::new();
        for it in &self.module.items {
            if let Item::Fn(f) = it {
                fns.push(self.erase_fun(f));
            }
        }
        CoreModule {
            structs: self.structs,
            fns,
        }
    }

    fn declare_struct_for(&mut self, alias: &str, ty: &Type) {
        if let Type::Refine { bind, ty: base, .. } = ty {
            let field_ty = self.erase_type(base);
            self.structs.push(StructDef {
                name: alias.to_string(),
                fields: vec![(bind.clone(), field_ty)],
            });
            self.struct_of.insert(alias.to_string(), alias.to_string());
        }
    }

    fn erase_type(&mut self, ty: &Type) -> CoreType {
        match ty {
            Type::Named(n) => {
                if let Some(aliased) = self.aliases.get(n).cloned() {
                    if matches!(aliased, Type::Refine { .. }) && self.struct_of.contains_key(n) {
                        return CoreType::Named(n.clone());
                    }
                    return self.erase_type(&aliased);
                }
                CoreType::Named(n.clone())
            }
            Type::Base(s) => CoreType::Base(s.clone()),
            Type::Array(inner, n) => CoreType::Array(Box::new(self.erase_type(inner)), *n),
            Type::Refine { bind, ty: base, .. } => {
                let field_ty = self.erase_type(base);
                let name = format!("_Ref{}", self.counter);
                self.counter += 1;
                self.structs.push(StructDef {
                    name: name.clone(),
                    fields: vec![(bind.clone(), field_ty)],
                });
                CoreType::Named(name)
            }
        }
    }

    fn erase_fun(&mut self, f: &Fun) -> CoreFun {
        let mut env: HashMap<String, CoreType> = HashMap::new();
        let mut params = Vec::new();
        for (pname, pty) in &f.params {
            let ct = self.erase_type(pty);
            env.insert(pname.clone(), ct.clone());
            params.push((pname.clone(), ct));
        }
        let ret = self
            .ret_of
            .get(&f.name)
            .cloned()
            .unwrap_or_else(|| self.erase_type(&f.ret));
        let body = self.erase_expr(&f.body, &mut env);
        CoreFun {
            name: f.name.clone(),
            params,
            ret,
            body,
        }
    }

    fn erase_expr(&mut self, e: &Expr, env: &mut HashMap<String, CoreType>) -> CExpr {
        match e {
            Expr::Num(n) => CExpr {
                ty: CoreType::Base("f64".into()),
                kind: CExprKind::Num(*n),
            },
            Expr::Bool(b) => CExpr {
                ty: CoreType::Base("bool".into()),
                kind: CExprKind::Bool(*b),
            },
            Expr::Var(v) => {
                let ty = env.get(v).cloned().unwrap_or(CoreType::Base("_".into()));
                CExpr {
                    ty,
                    kind: CExprKind::Var(v.clone()),
                }
            }
            Expr::ArrayLit(es) => {
                let mut cs = Vec::new();
                let mut elem = CoreType::Base("_".into());
                for x in es {
                    let c = self.erase_expr(x, env);
                    elem = c.ty.clone();
                    cs.push(c);
                }
                let n = cs.len() as u64;
                CExpr {
                    ty: CoreType::Array(Box::new(elem), n),
                    kind: CExprKind::ArrayLit(cs),
                }
            }
            Expr::Bin { op, a, b } => {
                let ca = self.erase_expr(a, env);
                let cb = self.erase_expr(b, env);
                let ty = if matches!(
                    op,
                    BinOp::Lt
                        | BinOp::Gt
                        | BinOp::Le
                        | BinOp::Ge
                        | BinOp::Eq
                        | BinOp::Ne
                        | BinOp::And
                        | BinOp::Or
                ) {
                    CoreType::Base("bool".into())
                } else {
                    CoreType::Base("f64".into())
                };
                CExpr {
                    ty,
                    kind: CExprKind::Bin {
                        op: *op,
                        a: Box::new(ca),
                        b: Box::new(cb),
                    },
                }
            }
            Expr::Un { op, a } => {
                let ca = self.erase_expr(a, env);
                let ty = match op {
                    UnOp::Neg => CoreType::Base("f64".into()),
                    UnOp::Not => CoreType::Base("bool".into()),
                };
                CExpr {
                    ty,
                    kind: CExprKind::Un {
                        op: *op,
                        a: Box::new(ca),
                    },
                }
            }
            Expr::If { cond, then, els } => {
                let cc = self.erase_expr(cond, env);
                let ct = self.erase_expr(then, env);
                let ce = self.erase_expr(els, env);
                let ty = ct.ty.clone();
                CExpr {
                    ty,
                    kind: CExprKind::If {
                        cond: Box::new(cc),
                        then: Box::new(ct),
                        els: Box::new(ce),
                    },
                }
            }
            Expr::Let { name, value, body } => {
                let cv = self.erase_expr(value, env);
                let vty = cv.ty.clone();
                env.insert(name.clone(), vty);
                let cb = self.erase_expr(body, env);
                env.remove(name);
                CExpr {
                    ty: cb.ty.clone(),
                    kind: CExprKind::Let {
                        name: name.clone(),
                        value: Box::new(cv),
                        body: Box::new(cb),
                    },
                }
            }
            Expr::Call { func, args } => {
                let cargs: Vec<CExpr> = args.iter().map(|a| self.erase_expr(a, env)).collect();
                let ty = self
                    .ret_of
                    .get(func)
                    .cloned()
                    .unwrap_or(CoreType::Base("_".into()));
                CExpr {
                    ty,
                    kind: CExprKind::Call {
                        func: func.clone(),
                        args: cargs,
                    },
                }
            }
            Expr::Method { recv, name, args } => {
                let cr = self.erase_expr(recv, env);
                let cargs: Vec<CExpr> = args.iter().map(|a| self.erase_expr(a, env)).collect();
                let ty = self.method_type(&cr, name);
                CExpr {
                    ty,
                    kind: CExprKind::Method {
                        recv: Box::new(cr),
                        name: name.clone(),
                        args: cargs,
                    },
                }
            }
            Expr::Lambda { params, body } => {
                let cb = self.erase_expr(body, env);
                let ty = cb.ty.clone();
                CExpr {
                    ty,
                    kind: CExprKind::Lambda {
                        params: params.clone(),
                        body: Box::new(cb),
                    },
                }
            }
            Expr::Record(fields) => {
                let mut cf = Vec::new();
                let mut ty = CoreType::Base("_".into());
                for (fnm, fv) in fields {
                    let c = self.erase_expr(fv, env);
                    if fnm == "v" {
                        ty = c.ty.clone();
                    }
                    cf.push((fnm.clone(), c));
                }
                CExpr {
                    ty,
                    kind: CExprKind::Record(cf),
                }
            }
            Expr::Cast { value, ty } => {
                let target = self.erase_type(ty);
                let mut cv = self.erase_expr(value, env);
                cv.ty = target;
                cv
            }
            Expr::Return(e) => {
                let ce = self.erase_expr(e, env);
                let ty = ce.ty.clone();
                CExpr {
                    ty,
                    kind: CExprKind::Return(Box::new(ce)),
                }
            }
        }
    }

    fn method_type(&self, recv: &CExpr, name: &str) -> CoreType {
        match name {
            "len" => CoreType::Base("usize".into()),
            "magnitude" => CoreType::Base("f64".into()),
            "map" => {
                if let CoreType::Array(inner, n) = &recv.ty {
                    CoreType::Array(inner.clone(), *n)
                } else {
                    CoreType::Array(Box::new(CoreType::Base("f64".into())), 0)
                }
            }
            "zip" => {
                if let CoreType::Array(_, n) = &recv.ty {
                    CoreType::Array(Box::new(CoreType::Base("f64".into())), *n)
                } else {
                    CoreType::Array(Box::new(CoreType::Base("f64".into())), 0)
                }
            }
            _ => CoreType::Base("f64".into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eidos_parser::parse;

    fn erase_src(src: &str) -> CoreModule {
        let m = parse(src).expect("parse");
        erase(&m)
    }

    #[test]
    fn erases_refinement_alias_to_struct() {
        let m = erase_src("type NV = { v: Array<f64, 3> | v.magnitude() <= 1.0 };");
        assert_eq!(m.structs.len(), 1);
        assert_eq!(m.structs[0].name, "NV");
        assert_eq!(m.structs[0].fields[0].0, "v");
        assert_eq!(
            m.structs[0].fields[0].1,
            CoreType::Array(Box::new(CoreType::Base("f64".into())), 3)
        );
    }

    #[test]
    fn erases_requires_ensures_effects() {
        let src = "fn f(a: f64) -> f64 requires a > 0.0 ensures |r| r >= 0.0 effects [Pure] {
            if a > 0.0 { return a; } else { return 0.0; }
        }";
        let m = erase_src(src);
        assert_eq!(m.fns.len(), 1);
        assert!(matches!(m.fns[0].body.kind, CExprKind::If { .. }));
    }

    #[test]
    fn erases_cast_to_witness_struct() {
        let src = "type NV = { v: Array<f64, 1> | v.magnitude() <= 1.0 };
        fn f(x: f64) -> NV { return { v: [x] } as NV; }";
        let m = erase_src(src);
        let body = &m.fns[0].body;
        assert!(matches!(body.kind, CExprKind::Return(_)));
        if let CExprKind::Return(inner) = &body.kind {
            assert!(matches!(inner.kind, CExprKind::Record(_)));
            assert_eq!(inner.ty, CoreType::Named("NV".into()));
        }
    }

    #[test]
    fn erases_division_without_refinement() {
        let src = "fn f(a: f64, b: f64) -> f64 { return a / b; }";
        let m = erase_src(src);
        match &m.fns[0].body.kind {
            CExprKind::Return(inner) => match &inner.kind {
                CExprKind::Bin { op, .. } => assert_eq!(*op, BinOp::Div),
                other => panic!("expected bin: {other:?}"),
            },
            other => panic!("expected return: {other:?}"),
        }
    }
}
