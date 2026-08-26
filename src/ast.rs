use crate::error::Span;

#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    pub statements: Vec<Stmt>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Type {
    String,
    Int,
    Float,
    Bool,
    Array(Box<Type>),
    List(Box<Type>),
    Tuple(Vec<Type>),
    Hash,
    Tree,
    Matrix,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CollectionOperation {
    Add,
    Remove,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    Located {
        span: Span,
        statement: Box<Stmt>,
    },
    Say(Expr),
    Expression(Expr),
    Import {
        path: String,
        alias: String,
    },
    Assign {
        name: String,
        declared_type: Option<Type>,
        value: Expr,
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
        names: Vec<String>,
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
    Function {
        name: String,
        parameters: Vec<(String, Option<Type>)>,
        return_type: Option<Type>,
        body: Vec<Stmt>,
    },
    Return(Expr),
    For {
        name: String,
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

#[derive(Debug, Clone, PartialEq)]
pub enum UnaryOperator {
    Not,
    Negate,
    Transpose,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PipelineStep {
    Filter(Expr),
    Map(Expr),
    Sum,
    Count,
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
