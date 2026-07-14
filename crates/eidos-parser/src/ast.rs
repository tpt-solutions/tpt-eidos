//! Abstract syntax for the tpt-eidos MVK surface language.

#[derive(Clone, Debug, PartialEq)]
pub enum Type {
    /// A primitive/base type, e.g. `f64`, `i64`, `bool`.
    Base(String),
    /// `Array<T, N>` with a compile-time length `N`.
    Array(Box<Type>, u64),
    /// Refinement type `{ x: T | predicate }`.
    Refine {
        bind: String,
        ty: Box<Type>,
        pred: Box<Expr>,
    },
    /// A named (aliased) type or a bare type identifier.
    Named(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Lt,
    Gt,
    Le,
    Ge,
    Eq,
    Ne,
    And,
    Or,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnOp {
    Neg,
    Not,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Expr {
    Num(f64),
    Bool(bool),
    Var(String),
    /// `[e1, e2, ...]`
    ArrayLit(Vec<Expr>),
    /// Binary operator application.
    Bin {
        op: BinOp,
        a: Box<Expr>,
        b: Box<Expr>,
    },
    /// Unary operator application.
    Un {
        op: UnOp,
        a: Box<Expr>,
    },
    /// `if cond { then } else { els }`
    If {
        cond: Box<Expr>,
        then: Box<Expr>,
        els: Box<Expr>,
    },
    /// `let x = value; body`
    Let {
        name: String,
        value: Box<Expr>,
        body: Box<Expr>,
    },
    /// `f(args)`
    Call {
        func: String,
        args: Vec<Expr>,
    },
    /// `recv.method(args)`
    Method {
        recv: Box<Expr>,
        name: String,
        args: Vec<Expr>,
    },
    /// `|p1, p2| body`
    Lambda {
        params: Vec<String>,
        body: Box<Expr>,
    },
    /// `{ field: value, ... }`
    Record(Vec<(String, Expr)>),
    /// `value as Type`
    Cast {
        value: Box<Expr>,
        ty: Box<Type>,
    },
    /// `return e`
    Return(Box<Expr>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Fun {
    pub name: String,
    pub params: Vec<(String, Type)>,
    pub ret: Type,
    pub requires: Option<Expr>,
    pub ensures: Option<Expr>,
    pub effects: Vec<String>,
    pub body: Expr,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Item {
    TypeAlias { name: String, ty: Type },
    Fn(Box<Fun>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Module {
    pub items: Vec<Item>,
}
