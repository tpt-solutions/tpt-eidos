//! Abstract syntax for the tpt-eidos MVK surface language.

/// A type in the tpt-eidos surface language.
#[derive(Clone, Debug, PartialEq)]
pub enum Type {
    /// A primitive/base type, e.g. `f64`, `i64`, `bool`.
    Base(String),
    /// `Array<T, N>` with a compile-time length `N`.
    Array(Box<Type>, u64),
    /// Refinement type `{ x: T | predicate }`.
    Refine {
        /// The binder name (the `x` in `{ x: T | p }`).
        bind: String,
        /// The base type being refined.
        ty: Box<Type>,
        /// The refinement predicate.
        pred: Box<Expr>,
    },
    /// A named (aliased) type or a bare type identifier.
    Named(String),
}

/// Binary operators in the tpt-eidos surface language.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinOp {
    /// `+`
    Add,
    /// `-`
    Sub,
    /// `*`
    Mul,
    /// `/`
    Div,
    /// `%`
    Rem,
    /// `<`
    Lt,
    /// `>`
    Gt,
    /// `<=`
    Le,
    /// `>=`
    Ge,
    /// `==`
    Eq,
    /// `!=`
    Ne,
    /// `&&`
    And,
    /// `||`
    Or,
}

/// Unary operators in the tpt-eidos surface language.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnOp {
    /// Arithmetic negation `-x`.
    Neg,
    /// Boolean negation `!x`.
    Not,
}

/// A lambda parameter pattern. Supports nested tuples so that `zip` chains can
/// be destructured, e.g. `zip(zip(a, b), c).map(|((x, y), z)| ...)`.
#[derive(Clone, Debug, PartialEq)]
pub enum Pattern {
    /// A simple variable binding.
    Var(String),
    /// A tuple pattern `(p1, p2, ...)`.
    Tuple(Vec<Pattern>),
}

/// An expression in the tpt-eidos surface language.
#[derive(Clone, Debug, PartialEq)]
pub enum Expr {
    /// A numeric literal.
    Num(f64),
    /// A boolean literal.
    Bool(bool),
    /// A variable reference.
    Var(String),
    /// `[e1, e2, ...]`
    ArrayLit(Vec<Expr>),
    /// Binary operator application.
    Bin {
        /// The operator.
        op: BinOp,
        /// Left operand.
        a: Box<Expr>,
        /// Right operand.
        b: Box<Expr>,
    },
    /// Unary operator application.
    Un {
        /// The operator.
        op: UnOp,
        /// Operand.
        a: Box<Expr>,
    },
    /// `if cond { then } else { els }`
    If {
        /// Condition.
        cond: Box<Expr>,
        /// Then-branch.
        then: Box<Expr>,
        /// Else-branch.
        els: Box<Expr>,
    },
    /// `let x = value; body`
    Let {
        /// Binding name.
        name: String,
        /// Bound value.
        value: Box<Expr>,
        /// Body expression.
        body: Box<Expr>,
    },
    /// `f(args)`
    Call {
        /// Callee name.
        func: String,
        /// Arguments.
        args: Vec<Expr>,
    },
    /// `recv.method(args)`
    Method {
        /// Receiver expression.
        recv: Box<Expr>,
        /// Method name.
        name: String,
        /// Arguments.
        args: Vec<Expr>,
    },
    /// `|p1, p2| body`
    Lambda {
        /// Parameter patterns.
        params: Vec<Pattern>,
        /// Body expression.
        body: Box<Expr>,
    },
    /// `{ field: value, ... }`
    Record(Vec<(String, Expr)>),
    /// `value as Type`
    Cast {
        /// The value being cast.
        value: Box<Expr>,
        /// The target type.
        ty: Box<Type>,
    },
    /// `return e`
    Return(Box<Expr>),
}

/// A function definition, including its optional contracts.
#[derive(Clone, Debug, PartialEq)]
pub struct Fun {
    /// The function name.
    pub name: String,
    /// Parameter list as `(name, type)` pairs.
    pub params: Vec<(String, Type)>,
    /// Declared return type.
    pub ret: Type,
    /// Optional `requires` precondition expression.
    pub requires: Option<Expr>,
    /// Optional `ensures` postcondition expression.
    pub ensures: Option<Expr>,
    /// `effects [...]` label list (e.g. `["Pure", "IO"]`).
    pub effects: Vec<String>,
    /// Function body expression.
    pub body: Expr,
}

/// A top-level item in a tpt-eidos module.
#[derive(Clone, Debug, PartialEq)]
pub enum Item {
    /// `type Name = Type;`
    TypeAlias {
        /// The alias name.
        name: String,
        /// The aliased type.
        ty: Type,
    },
    /// A function definition.
    Fn(Box<Fun>),
}

/// A parsed tpt-eidos source file.
#[derive(Clone, Debug, PartialEq)]
pub struct Module {
    /// All top-level items in source order.
    pub items: Vec<Item>,
}
