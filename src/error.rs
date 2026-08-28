use std::{error::Error, fmt};

#[derive(Debug, Clone, PartialEq)]
pub struct Span {
    pub line: usize,
    pub column: usize,
    pub length: usize,
}

impl Span {
    pub fn new(line: usize, column: usize) -> Self {
        Self {
            line,
            column,
            length: 1,
        }
    }

    #[allow(dead_code)]
    pub fn with_length(line: usize, column: usize, length: usize) -> Self {
        Self {
            line,
            column,
            length: length.max(1),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DiagnosticCategory {
    Lex,
    Parse,
    Semantic,
    Runtime,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DiagnosticCode {
    InvalidCharacter,
    UnterminatedString,
    UnexpectedToken,
    ExpectedExpression,
    UndefinedVariable,
    TypeMismatch,
    InvalidReassignment,
    InvalidFunctionCall,
    InvalidReturn,
    InvalidBreakContinue,
    RuntimeCollection,
    RuntimeDivision,
    RuntimeArithmetic,
    RuntimeImport,
    RuntimeMessage,
    RuntimeGeneral,
    SemanticDestructure,
    SemanticCollection,
    SemanticField,
    SemanticIndex,
    SemanticTupleIndex,
    SemanticMatrixIndex,
    SemanticMissingReturn,
    SemanticCollectionOperation,
}

impl DiagnosticCode {
    pub fn category(self) -> DiagnosticCategory {
        match self {
            Self::InvalidCharacter | Self::UnterminatedString => DiagnosticCategory::Lex,
            Self::UnexpectedToken | Self::ExpectedExpression => DiagnosticCategory::Parse,
            Self::UndefinedVariable
            | Self::TypeMismatch
            | Self::InvalidReassignment
            | Self::InvalidFunctionCall
            | Self::InvalidReturn
            | Self::InvalidBreakContinue
            | Self::SemanticDestructure
            | Self::SemanticCollection
            | Self::SemanticField
            | Self::SemanticIndex
            | Self::SemanticTupleIndex
            | Self::SemanticMatrixIndex
            | Self::SemanticMissingReturn
            | Self::SemanticCollectionOperation => DiagnosticCategory::Semantic,
            Self::RuntimeCollection
            | Self::RuntimeDivision
            | Self::RuntimeArithmetic
            | Self::RuntimeImport
            | Self::RuntimeMessage
            | Self::RuntimeGeneral => DiagnosticCategory::Runtime,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::InvalidCharacter => "E0101",
            Self::UnterminatedString => "E0102",
            Self::UnexpectedToken => "E0103",
            Self::ExpectedExpression => "E0104",
            Self::UndefinedVariable => "E0001",
            Self::TypeMismatch => "E0003",
            Self::InvalidReassignment => "E0105",
            Self::InvalidFunctionCall => "E0002",
            Self::InvalidReturn => "E0006",
            Self::InvalidBreakContinue => "E0007",
            Self::RuntimeCollection => "E0201",
            Self::RuntimeDivision => "E0202",
            Self::RuntimeArithmetic => "E0203",
            Self::RuntimeImport => "E0204",
            Self::RuntimeMessage => "E0205",
            Self::RuntimeGeneral => "E0206",
            Self::SemanticDestructure => "E0011",
            Self::SemanticCollection => "E0012",
            Self::SemanticField => "E0013",
            Self::SemanticIndex => "E0014",
            Self::SemanticTupleIndex => "E0015",
            Self::SemanticMatrixIndex => "E0016",
            Self::SemanticMissingReturn => "E0005",
            Self::SemanticCollectionOperation => "E0010",
        }
    }
}

#[derive(Debug)]
pub enum SimplyError {
    Lex {
        span: Span,
        code: DiagnosticCode,
        message: String,
    },
    Parse {
        span: Span,
        code: DiagnosticCode,
        message: String,
    },
    Semantic {
        span: Span,
        code: DiagnosticCode,
        message: String,
    },
    Runtime {
        span: Span,
        code: DiagnosticCode,
        message: String,
    },
}

impl SimplyError {
    pub fn category(&self) -> DiagnosticCategory {
        self.code().category()
    }

    pub fn code(&self) -> DiagnosticCode {
        match self {
            Self::Lex { code, .. }
            | Self::Parse { code, .. }
            | Self::Semantic { code, .. }
            | Self::Runtime { code, .. } => *code,
        }
    }

    pub fn span(&self) -> &Span {
        match self {
            Self::Lex { span, .. }
            | Self::Parse { span, .. }
            | Self::Semantic { span, .. }
            | Self::Runtime { span, .. } => span,
        }
    }

    pub fn render(&self, filename: &str, source: &str) -> String {
        let code = self.code();
        let category = match self.category() {
            DiagnosticCategory::Lex => "Lex error",
            DiagnosticCategory::Parse => "Parse error",
            DiagnosticCategory::Semantic => "Semantic error",
            DiagnosticCategory::Runtime => "Runtime error",
        };
        let span = self.span();
        let message = match self {
            Self::Lex { message, .. }
            | Self::Parse { message, .. }
            | Self::Semantic { message, .. }
            | Self::Runtime { message, .. } => message,
        };
        let location = if span.line == 0 {
            filename.to_string()
        } else {
            format!("{filename}:{}:{}", span.line, span.column)
        };
        let mut rendered = format!(
            "Error: {category} at {location}: error[{}]: {message}",
            code.as_str()
        );

        let lines: Vec<&str> = source.lines().collect();
        let source_line = lines
            .get(span.line.saturating_sub(1))
            .copied()
            .or_else(|| (span.line == lines.len() + 1).then_some(""));
        if let Some(line) = source_line {
            let column = span.column.max(1);
            rendered.push_str(&format!(
                "\n\n  {} | {}\n\t| {}{}",
                span.line,
                line,
                " ".repeat(column.saturating_sub(1)),
                "^".repeat(span.length.max(1))
            ));
        }

        rendered
    }
}

impl fmt::Display for SimplyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.render("<unknown>", ""))
    }
}

impl Error for SimplyError {}

#[cfg(test)]
mod tests {
    use super::{DiagnosticCategory, DiagnosticCode, SimplyError, Span};

    #[test]
    fn renders_source_context() {
        let error = SimplyError::Runtime {
            span: Span::with_length(2, 6, 4),
            code: DiagnosticCode::RuntimeGeneral,
            message: "unknown variable `name`".into(),
        };

        assert_eq!(
            error.render("example.si", "Say \"ok\"\nSay name\n"),
            "Error: Runtime error at example.si:2:6: error[E0206]: unknown variable `name`\n\n  2 | Say name\n\t|      ^^^^"
        );
    }

    #[test]
    fn diagnostic_codes_are_unique() {
        let codes = [
            DiagnosticCode::InvalidCharacter,
            DiagnosticCode::UnterminatedString,
            DiagnosticCode::UnexpectedToken,
            DiagnosticCode::ExpectedExpression,
            DiagnosticCode::UndefinedVariable,
            DiagnosticCode::TypeMismatch,
            DiagnosticCode::InvalidReassignment,
            DiagnosticCode::InvalidFunctionCall,
            DiagnosticCode::InvalidReturn,
            DiagnosticCode::InvalidBreakContinue,
            DiagnosticCode::RuntimeCollection,
            DiagnosticCode::RuntimeDivision,
            DiagnosticCode::RuntimeArithmetic,
            DiagnosticCode::RuntimeImport,
            DiagnosticCode::RuntimeMessage,
            DiagnosticCode::RuntimeGeneral,
            DiagnosticCode::SemanticDestructure,
            DiagnosticCode::SemanticCollection,
            DiagnosticCode::SemanticField,
            DiagnosticCode::SemanticIndex,
            DiagnosticCode::SemanticTupleIndex,
            DiagnosticCode::SemanticMatrixIndex,
            DiagnosticCode::SemanticMissingReturn,
            DiagnosticCode::SemanticCollectionOperation,
        ];
        let mut unique = std::collections::HashSet::new();
        assert!(codes.iter().all(|code| unique.insert(code.as_str())));
    }

    #[test]
    fn diagnostic_code_is_the_source_of_truth_for_category() {
        let error = SimplyError::Runtime {
            span: Span::new(2, 3),
            code: DiagnosticCode::TypeMismatch,
            message: "type mismatch".into(),
        };

        assert_eq!(error.code(), DiagnosticCode::TypeMismatch);
        assert_eq!(error.category(), DiagnosticCategory::Semantic);
        assert_eq!(error.span(), &Span::new(2, 3));
        assert!(
            error
                .render("program.si", "Say 1\nSay 2\n")
                .starts_with("Error: Semantic error at program.si:2:3: error[E0003]:")
        );
    }

    #[test]
    fn renders_eof_unicode_windows_path_and_missing_context_safely() {
        let error = SimplyError::Parse {
            span: Span::new(3, 1),
            code: DiagnosticCode::UnexpectedToken,
            message: "expected an expression".into(),
        };
        let rendered = error.render(r"C:\projects\program.si", "é\nSay 1\n");
        assert!(rendered.contains(r"C:\projects\program.si:3:1"));
        assert!(rendered.contains("3 |"));
        assert!(rendered.contains("| ^"));

        let missing = SimplyError::Lex {
            span: Span::new(99, 4),
            code: DiagnosticCode::InvalidCharacter,
            message: "invalid character".into(),
        };
        assert!(
            missing
                .render("program.si", "é\n")
                .contains("program.si:99:4")
        );
    }
}
