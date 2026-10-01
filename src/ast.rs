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
pub struct StructField {
    pub name: String,
    pub field_type: Type,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EnumVariant {
    pub name: String,
    pub payload_type: Option<Type>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MatchArm {
    pub pattern: MatchPattern,
    pub guard: Option<Expr>,
    pub body: Vec<Stmt>,
    pub result: Option<Expr>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum MatchPattern {
    Identifier(String),
    Literal(Literal),
    Range {
        start: Option<Literal>,
        end: Option<Literal>,
    },
    Or(Vec<MatchPattern>),
    Tuple(Vec<MatchPattern>),
    Sequence {
        patterns: Vec<MatchPattern>,
        rest: Option<String>,
    },
    Hash(Vec<(String, MatchPattern)>),
    Alias {
        name: String,
        pattern: Box<MatchPattern>,
    },
    EnumVariant {
        enum_name: String,
        variant_name: String,
        payload: Option<Box<MatchPattern>>,
    },
    Struct {
        type_name: String,
        fields: Vec<MatchPattern>,
    },
    NamedStruct {
        type_name: String,
        fields: Vec<(String, MatchPattern)>,
    },
    Wildcard,
}

impl MatchPattern {
    pub(crate) fn destructure_binding_names(&self) -> Vec<&str> {
        fn collect<'a>(pattern: &'a MatchPattern, names: &mut Vec<&'a str>) {
            match pattern {
                MatchPattern::Identifier(name) => names.push(name),
                MatchPattern::Tuple(patterns) => {
                    for pattern in patterns {
                        collect(pattern, names);
                    }
                }
                MatchPattern::Sequence { patterns, rest } => {
                    for pattern in patterns {
                        collect(pattern, names);
                    }
                    if let Some(name) = rest {
                        names.push(name);
                    }
                }
                MatchPattern::Wildcard
                | MatchPattern::Literal(_)
                | MatchPattern::Range { .. }
                | MatchPattern::Or(_)
                | MatchPattern::Hash(_)
                | MatchPattern::Alias { .. }
                | MatchPattern::EnumVariant { .. }
                | MatchPattern::Struct { .. }
                | MatchPattern::NamedStruct { .. } => {}
            }
        }

        let mut names = Vec::new();
        collect(self, &mut names);
        names
    }
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
        alias: Option<String>,
        exposing: Vec<(String, String)>,
    },
    Export {
        names: Vec<String>,
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
    DestructureReassign {
        pattern: MatchPattern,
        value: Expr,
    },
    SetIndex {
        name: String,
        index: Expr,
        value: Expr,
    },
    Destructure {
        pattern: MatchPattern,
        mutable: bool,
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
    Struct {
        name: String,
        fields: Vec<StructField>,
    },
    Enum {
        name: String,
        variants: Vec<EnumVariant>,
    },
    Message {
        receiver_type: String,
        name: String,
        parameters: Vec<(String, Option<Type>, bool)>,
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
    MessageDispatch {
        receiver: Box<Expr>,
        message: String,
        arguments: Vec<Expr>,
    },
    EnumVariant {
        enum_name: String,
        variant_name: String,
        arguments: Vec<Expr>,
    },
    Match {
        value: Box<Expr>,
        arms: Vec<MatchArm>,
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
    Take(i64),
    Skip(i64),
    StepBy(i64),
    TakeWhile(Expr),
    DropWhile(Expr),
    Distinct,
    Partition {
        item: String,
        rules: Vec<PartitionRule>,
    },
    Sum,
    Count,
    Average,
    Min,
    Max,
    Any,
    All,
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
