//! Abstract syntax for the tpt-eidos MVK surface language.

/// A source location span: byte offset range within the source text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    /// Start byte offset (inclusive, 0-based).
    pub lo: usize,
    /// End byte offset (exclusive, 0-based).
    pub hi: usize,
}

impl Span {
    /// The zero span (used as a default when no source location is available).
    pub fn none() -> Self {
        Span { lo: 0, hi: 0 }
    }
}

/// Time unit for WCET budget specifications.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimeUnit {
    /// Microseconds.
    Us,
    /// Milliseconds.
    Ms,
    /// Seconds.
    S,
}

/// A single effect annotation on a function.
#[derive(Clone, Debug, PartialEq)]
pub struct Effect {
    /// The effect name (e.g. "Pure", "IO", "RealTime").
    pub name: String,
    /// Optional WCET budget: `(value, unit)` for parameterized effects
    /// like `RealTime<2ms>`.
    pub budget: Option<(f64, TimeUnit)>,
}

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
    /// A linear (affine) type: the value must be used exactly once on every
    /// code path. `linear T` wraps an inner type `T`.
    Linear(Box<Type>),
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

/// A lambda parameter pattern. Supports nested tuples so that `zip` chains can
/// be destructured, e.g. `zip(zip(a, b), c).map(|((x, y), z)| ...)`.
#[derive(Clone, Debug, PartialEq)]
pub enum Pattern {
    Var(String),
    Tuple(Vec<Pattern>),
}

/// The kind of an expression (all variants that used to be on `Expr`).
#[derive(Clone, Debug, PartialEq)]
pub enum ExprKind {
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
        params: Vec<Pattern>,
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

/// A source-annotated expression: wraps an [`ExprKind`] with a [`Span`].
#[derive(Clone, Debug, PartialEq)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
}

impl Expr {
    /// Build an expression with a zero span (for generated / synthetic exprs).
    pub fn new(kind: ExprKind) -> Self {
        Expr {
            kind,
            span: Span::none(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Fun {
    pub name: String,
    pub params: Vec<(String, Type)>,
    pub ret: Type,
    pub requires: Option<Expr>,
    pub ensures: Option<Expr>,
    pub effects: Vec<Effect>,
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
