//! ast.rs — abstract syntax tree definitions
//! Defines the parsed program, statements, expressions, literals, operators, and pipeline steps.
//! Key components: Program, Stmt, Expr, and their operator/collection enums.
use std::sync::Arc;

use crate::{error::Span, types::Type};

#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    pub statements: Vec<Stmt>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CollectionOperation {
    Add,
    Remove,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CatchClause {
    pub binding: Option<String>,
    pub code: Option<String>,
    pub body: Vec<Stmt>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    Located {
        span: Span,
        statement: Box<Stmt>,
    },
    Say(Expr),
    Sayln(Expr),
    Expression(Expr),
    Import {
        path: String,
        alias: String,
    },
    Assign {
        name: String,
        mutable: bool,
        declared_type: Option<Type>,
        value: Expr,
    },
    Flow {
        name: String,
        source: Expr,
        steps: Vec<PipelineStep>,
    },
    Reassign {
        name: String,
        value: Expr,
    },
    SetIndex {
        name: String,
        index: Expr,
        value: Expr,
    },
    Destructure {
        names: Vec<(String, bool)>,
        value: Expr,
    },
    CollectionOp {
        name: String,
        operation: CollectionOperation,
        value: Expr,
    },
    If {
        condition: Expr,
        then_branch: Vec<Stmt>,
        else_branch: Vec<Stmt>,
    },
    Throw(Expr),
    Try {
        try_body: Vec<Stmt>,
        catches: Vec<CatchClause>,
        finally_body: Vec<Stmt>,
    },
    Function {
        name: String,
        parameters: Vec<(String, Option<Type>, bool)>,
        return_type: Option<Type>,
        body: Arc<[Stmt]>,
    },
    Return(Expr),
    For {
        name: String,
        mutable: bool,
        iterable: Expr,
        body: Vec<Stmt>,
    },
    While {
        condition: Expr,
        body: Vec<Stmt>,
    },
    Break,
    Continue,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Literal(Literal),
    Identifier(String),
    Unary {
        operator: UnaryOperator,
        operand: Box<Expr>,
    },
    Binary {
        left: Box<Expr>,
        operator: BinaryOperator,
        right: Box<Expr>,
    },
    Call {
        name: String,
        arguments: Vec<Expr>,
    },
    Array(Vec<Expr>),
    List(Vec<Expr>),
    Tuple(Vec<Expr>),
    Index {
        target: Box<Expr>,
        index: Box<Expr>,
    },
    Field {
        target: Box<Expr>,
        name: String,
    },
    Hash(Vec<(String, Expr)>),
    Tree(Vec<(String, Expr)>),
    Matrix(Vec<Expr>),
    Pipeline {
        source: Box<Expr>,
        steps: Vec<PipelineStep>,
    },
}

pub fn is_parallel_safe_expression(expression: &Expr) -> bool {
    match expression {
        Expr::Literal(_) => true,
        Expr::Identifier(name) => name == "item",
        Expr::Unary { operator, operand } => {
            !matches!(operator, UnaryOperator::Transpose) && is_parallel_safe_expression(operand)
        }
        Expr::Binary { left, right, .. } => {
            is_parallel_safe_expression(left) && is_parallel_safe_expression(right)
        }
        _ => false,
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum UnaryOperator {
    Not,
    Negate,
    Transpose,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PipelineStep {
    Where(Expr),
    Derive(Expr),
    Partition {
        item: String,
        rules: Vec<PartitionRule>,
    },
    Sum,
    Count,
    Average,
    Min,
    Max,
    WriteCsv(Expr),
    /// Process items in batches and persist progress at each batch boundary.
    Chunk(i64),
    /// Request up to this many workers for pure scalar where/derive transforms.
    /// Unsupported expressions are rejected instead of silently running sequentially.
    Parallel(i64),
    /// Path for the resumable progress checkpoint.
    Checkpoint(Expr),
}

#[derive(Debug, Clone, PartialEq)]
pub struct PartitionRule {
    pub condition: Option<Expr>,
    pub category: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum BinaryOperator {
    And,
    Or,
    Add,
    Subtract,
    Multiply,
    Divide,
    Remainder,
    MatrixMultiply,
    Greater,
    GreaterEqual,
    Less,
    LessEqual,
    Equal,
    NotEqual,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Literal {
    String(String),
    Int(i64),
    Float(f64),
    Bool(bool),
}
