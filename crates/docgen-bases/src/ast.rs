//! Expression AST for the Bases language. A `Call` whose `callee` is a `Member`
//! is a *method* call (`list.contains(x)`); a `Call` on a bare `Ident` is a
//! *global function* call (`link("x")`). The evaluator makes that distinction.

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Null,
    Bool(bool),
    Number(f64),
    Str(String),
    Regex(String, String),
    /// A bare identifier: a namespace (`file`/`note`/`formula`/`this`), a global
    /// function name (when directly called), or a note property.
    Ident(String),
    /// `object.member` — property/method access.
    Member(Box<Expr>, String),
    /// `object[index]` — dynamic index/property access.
    Index(Box<Expr>, Box<Expr>),
    /// `callee(args...)` — function or method call.
    Call(Box<Expr>, Vec<Expr>),
    Unary(UnaryOp, Box<Expr>),
    Binary(BinaryOp, Box<Expr>, Box<Expr>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Not,
    Neg,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Eq,
    NotEq,
    Lt,
    Gt,
    LtEq,
    GtEq,
    And,
    Or,
}
