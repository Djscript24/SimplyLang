use std::{error::Error, fmt};

#[derive(Debug, Clone, PartialEq)]
pub struct Span {
    pub line: usize,
    pub column: usize,
}

impl Span {
    pub fn new(line: usize, column: usize) -> Self {
        Self { line, column }
    }
}

#[derive(Debug)]
pub enum SimplyError {
    Lex { span: Span, message: String },
    Parse { span: Span, message: String },
    Runtime { span: Span, message: String },
}

impl fmt::Display for SimplyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Lex { span, message } => {
                write!(f, "lex error at {}:{}: {message}", span.line, span.column)
            }
            Self::Parse { span, message } => {
                write!(f, "parse error at {}:{}: {message}", span.line, span.column)
            }
            Self::Runtime { span, message } => write!(
                f,
                "runtime error at {}:{}: {message}",
                span.line, span.column
            ),
        }
    }
}

impl Error for SimplyError {}
