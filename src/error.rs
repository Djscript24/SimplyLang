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
    Lex {
        span: Span,
        message: String,
    },
    Parse {
        span: Span,
        message: String,
    },
    Semantic {
        span: Span,
        code: String,
        message: String,
    },
    Runtime {
        span: Span,
        message: String,
    },
}

impl SimplyError {
    pub fn render(&self, filename: &str, source: &str) -> String {
        let (category, span, message) = match self {
            Self::Lex { span, message } => ("Lex error", span, message.clone()),
            Self::Parse { span, message } => ("Parse error", span, message.clone()),
            Self::Semantic {
                span,
                code,
                message,
            } => ("Semantic error", span, format!("error[{code}]: {message}")),
            Self::Runtime { span, message } => ("Runtime error", span, message.clone()),
        };
        let location = if span.line == 0 {
            format!("{filename}")
        } else {
            format!("{filename}:{}:{}", span.line, span.column)
        };
        let mut rendered = format!("Error: {category} at {location}: {message}");

        if let Some(line) = source.lines().nth(span.line.saturating_sub(1)) {
            let column = span.column.max(1);
            rendered.push_str(&format!(
                "\n\n  {} | {}\n    | {}^",
                span.line,
                line,
                " ".repeat(column.saturating_sub(1))
            ));
        }

        rendered
    }
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
            Self::Semantic {
                span,
                code,
                message,
            } => write!(
                f,
                "semantic error[{code}] at {}:{}: {message}",
                span.line, span.column
            ),
            Self::Runtime { span, message } => write!(
                f,
                "runtime error at {}:{}: {message}",
                span.line, span.column
            ),
        }
    }
}

impl Error for SimplyError {}

#[cfg(test)]
mod tests {
    use super::{SimplyError, Span};

    #[test]
    fn renders_source_context() {
        let error = SimplyError::Runtime {
            span: Span::new(2, 6),
            message: "unknown variable `name`".into(),
        };

        assert_eq!(
            error.render("example.si", "Say \"ok\"\nSay name\n"),
            "Error: Runtime error at example.si:2:6: unknown variable `name`\n\n  2 | Say name\n    |      ^"
        );
    }
}
