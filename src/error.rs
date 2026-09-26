//! error.rs — diagnostics and source spans
//! Defines source locations, diagnostic categories/codes, and the SimplyError types used across the compiler and runtime.
//! Key components: Span, DiagnosticCode, DiagnosticCategory, and SimplyError.
use std::{error::Error, fmt};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

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
    Command,
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
    DuplicateDeclaration,
    InvalidFunctionCall,
    InvalidReturn,
    InvalidBreakContinue,
    RuntimeCollection,
    RuntimeDivision,
    RuntimeArithmetic,
    RuntimeImport,
    RuntimeMessage,
    RuntimeGeneral,
    RuntimeConversion,
    CliUsage,
    InvalidNumericLiteral,
    SemanticDestructure,
    SemanticCollection,
    SemanticField,
    SemanticIndex,
    SemanticTupleIndex,
    SemanticMatrixIndex,
    SemanticMissingReturn,
    SemanticCollectionOperation,
    InvalidNumber,
    RuntimeTypeMismatch,
    RuntimeDeclaration,
    RuntimeMutability,
}

impl DiagnosticCode {
    pub fn category(self) -> DiagnosticCategory {
        match self {
            Self::InvalidCharacter | Self::UnterminatedString | Self::InvalidNumber => {
                DiagnosticCategory::Lex
            }
            Self::UnexpectedToken | Self::ExpectedExpression => DiagnosticCategory::Parse,
            Self::UndefinedVariable
            | Self::TypeMismatch
            | Self::InvalidReassignment
            | Self::DuplicateDeclaration
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
            | Self::RuntimeGeneral
            | Self::RuntimeConversion
            | Self::RuntimeTypeMismatch
            | Self::RuntimeDeclaration
            | Self::RuntimeMutability => DiagnosticCategory::Runtime,
            Self::InvalidNumericLiteral => DiagnosticCategory::Semantic,
            Self::CliUsage => DiagnosticCategory::Command,
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
            Self::DuplicateDeclaration => "E0017",
            Self::InvalidFunctionCall => "E0002",
            Self::InvalidReturn => "E0006",
            Self::InvalidBreakContinue => "E0007",
            Self::RuntimeCollection => "E0201",
            Self::RuntimeDivision => "E0202",
            Self::RuntimeArithmetic => "E0203",
            Self::RuntimeImport => "E0204",
            Self::RuntimeMessage => "E0205",
            Self::RuntimeGeneral => "E0206",
            Self::RuntimeConversion => "E0207",
            Self::CliUsage => "E0301",
            Self::SemanticDestructure => "E0011",
            Self::SemanticCollection => "E0012",
            Self::SemanticField => "E0013",
            Self::SemanticIndex => "E0014",
            Self::SemanticTupleIndex => "E0015",
            Self::SemanticMatrixIndex => "E0016",
            Self::SemanticMissingReturn => "E0005",
            Self::SemanticCollectionOperation => "E0010",
            Self::InvalidNumericLiteral => "E0018",
            Self::InvalidNumber => "E0106",
            Self::RuntimeTypeMismatch => "E0208",
            Self::RuntimeDeclaration => "E0209",
            Self::RuntimeMutability => "E0210",
        }
    }

    fn explanation(self) -> &'static str {
        match self {
            Self::InvalidCharacter => "This character is not part of Simply's language.",
            Self::UnterminatedString => "A text value starts with a quote but does not close.",
            Self::UnexpectedToken => "The code contains something where it was not expected.",
            Self::ExpectedExpression => "A value or calculation is missing from this line.",
            Self::UndefinedVariable => "This name has not been defined where it is used.",
            Self::TypeMismatch => "The value here is not the kind of value this code requires.",
            Self::InvalidReassignment => "This value cannot be changed in the way requested.",
            Self::DuplicateDeclaration => "This name has already been declared in this scope.",
            Self::InvalidFunctionCall => "The function call does not match a known function.",
            Self::InvalidReturn => "This return statement is not valid in this function.",
            Self::InvalidBreakContinue => "Break and continue can only be used inside a loop.",
            Self::RuntimeCollection => "This operation could not be completed on the collection.",
            Self::RuntimeDivision => "A number was divided by zero.",
            Self::RuntimeArithmetic => "This calculation could not produce a valid number.",
            Self::RuntimeImport => "Simply could not load a required file.",
            Self::RuntimeMessage => "This message cannot be sent to that value.",
            Self::RuntimeGeneral => "The program could not complete this operation.",
            Self::RuntimeConversion => {
                "Text supplied for a number conversion is not a valid number."
            }
            Self::CliUsage => "The command or option was not used in a supported way.",
            Self::InvalidNumericLiteral => {
                "This text cannot be read as the requested kind of number."
            }
            Self::SemanticDestructure => "The value does not match the names used to unpack it.",
            Self::SemanticCollection => {
                "The value does not have the collection shape this operation needs."
            }
            Self::SemanticField => "This value does not have the requested field.",
            Self::SemanticIndex => "This value cannot be accessed with an index.",
            Self::SemanticTupleIndex => "This tuple index is not valid.",
            Self::SemanticMatrixIndex => "This matrix index or shape is not valid.",
            Self::SemanticMissingReturn => {
                "This function promises a value but may finish without returning one."
            }
            Self::SemanticCollectionOperation => {
                "This collection operation is not allowed for this value or binding."
            }
            Self::InvalidNumber => "This number literal is not valid.",
            Self::RuntimeTypeMismatch => {
                "A value has a different type from the one this operation requires."
            }
            Self::RuntimeDeclaration => "A name could not be declared in the current scope.",
            Self::RuntimeMutability => "This binding cannot be changed in the requested way.",
        }
    }

    fn suggestion(self) -> &'static str {
        match self {
            Self::InvalidCharacter => {
                "Remove the character or replace it with valid Simply syntax."
            }
            Self::UnterminatedString => "Add the missing closing double quote.",
            Self::UnexpectedToken => "Check the syntax immediately around this location.",
            Self::ExpectedExpression => {
                "Add the missing value or calculation; after `->`, it may start on the next line."
            }
            Self::UndefinedVariable => "Check the spelling, or define the name before using it.",
            Self::TypeMismatch => {
                "Make the value's type match the required type. Text-to-number conversion only works when the text contains a valid number."
            }
            Self::InvalidReassignment => {
                "Declare the variable with `mut` if it should be changeable."
            }
            Self::DuplicateDeclaration => {
                "Choose a different name, or reassign the existing mutable variable."
            }
            Self::InvalidFunctionCall => {
                "Check the function name and the number and types of its arguments."
            }
            Self::InvalidReturn => {
                "Put `return` inside a function and return the type declared by that function."
            }
            Self::InvalidBreakContinue => {
                "Move `break` or `continue` inside a `for` or `while` loop."
            }
            Self::RuntimeCollection => {
                "Check the collection contents, index, and operation being used."
            }
            Self::RuntimeDivision => "Make sure the divisor is not zero.",
            Self::RuntimeArithmetic => {
                "Check the operands and make sure the result stays within a valid numeric range."
            }
            Self::RuntimeImport => "Check that the file exists and that its path is correct.",
            Self::RuntimeMessage => {
                "Check that the receiver supports this message and that its arguments are valid."
            }
            Self::RuntimeGeneral => {
                "Review the operation and the values it uses; the detail above has more context."
            }
            Self::RuntimeConversion => {
                "Use digits only for `to_int` (for example, `\"42\"`) or a valid decimal/exponent for `to_float` (for example, `\"3.5\"`)."
            }
            Self::CliUsage => "Run `simply --help` to see supported commands and options.",
            Self::InvalidNumericLiteral => {
                "Replace this text with a valid number, or keep it as text instead of converting it."
            }
            Self::SemanticDestructure => {
                "Make the number and order of names match the value being unpacked."
            }
            Self::SemanticCollection => {
                "Use a supported collection with the shape and element types this operation expects."
            }
            Self::SemanticField => "Check the field name, or use a value that contains that field.",
            Self::SemanticIndex => {
                "Use an array, list, tuple, hash, tree, or matrix with a valid index."
            }
            Self::SemanticTupleIndex => {
                "Use an integer index within the tuple's available positions."
            }
            Self::SemanticMatrixIndex => {
                "Check the matrix dimensions and use valid row and column indexes."
            }
            Self::SemanticMissingReturn => {
                "Add a return value on every possible path through the function."
            }
            Self::SemanticCollectionOperation => {
                "Check the collection type and declare it with `mut` before changing it."
            }
            Self::InvalidNumber => {
                "Check the number's digits, decimal point, and exponent, and make sure it fits the supported numeric range."
            }
            Self::RuntimeTypeMismatch => {
                "Use a value with the required type; if it comes from dynamic data, check or convert it before this operation."
            }
            Self::RuntimeDeclaration => {
                "Use a new name in this scope, or update the existing mutable binding."
            }
            Self::RuntimeMutability => {
                "Declare the binding with `mut` before reassigning it or changing its contents."
            }
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
    Command {
        span: Span,
        code: DiagnosticCode,
        message: String,
    },
}

impl SimplyError {
    pub fn category(&self) -> DiagnosticCategory {
        match self {
            Self::Lex { .. } => DiagnosticCategory::Lex,
            Self::Parse { .. } => DiagnosticCategory::Parse,
            Self::Semantic { .. } => DiagnosticCategory::Semantic,
            Self::Runtime { .. } => DiagnosticCategory::Runtime,
            Self::Command { .. } => DiagnosticCategory::Command,
        }
    }

    pub fn code(&self) -> DiagnosticCode {
        match self {
            Self::Lex { code, .. }
            | Self::Parse { code, .. }
            | Self::Semantic { code, .. }
            | Self::Runtime { code, .. }
            | Self::Command { code, .. } => *code,
        }
    }

    pub fn span(&self) -> &Span {
        match self {
            Self::Lex { span, .. }
            | Self::Parse { span, .. }
            | Self::Semantic { span, .. }
            | Self::Runtime { span, .. }
            | Self::Command { span, .. } => span,
        }
    }

    pub fn message(&self) -> &str {
        match self {
            Self::Lex { message, .. }
            | Self::Parse { message, .. }
            | Self::Semantic { message, .. }
            | Self::Runtime { message, .. }
            | Self::Command { message, .. } => message,
        }
    }

    pub fn render(&self, filename: &str, source: &str) -> String {
        self.render_with_terminal_width(filename, source, None)
    }

    pub fn render_with_terminal_width(
        &self,
        filename: &str,
        source: &str,
        terminal_width: Option<usize>,
    ) -> String {
        let code = self.code();
        let category = match self.category() {
            DiagnosticCategory::Lex => "Lex error",
            DiagnosticCategory::Parse => "Parse error",
            DiagnosticCategory::Semantic => "Semantic error",
            DiagnosticCategory::Runtime => "Runtime error",
            DiagnosticCategory::Command => "Command error",
        };
        let span = self.span();
        let message = match self {
            Self::Lex { message, .. }
            | Self::Parse { message, .. }
            | Self::Semantic { message, .. }
            | Self::Runtime { message, .. }
            | Self::Command { message, .. } => message,
        };
        let mut rendered = format!("error[{}] ({category})", code.as_str());
        let filename = escape_control_characters(filename);
        let message = escape_control_characters(message);

        let lines: Vec<&str> = source.lines().collect();
        let source_line = lines
            .get(span.line.saturating_sub(1))
            .copied()
            .or_else(|| (span.line == lines.len() + 1).then_some(""));
        let location = if span.line == 0 {
            format!("  --> {filename}")
        } else {
            format!("  --> {filename}:{}:{}", span.line, span.column)
        };
        rendered.push_str(&format!("\n{location}"));
        if let Some(line) = source_line {
            let line_number_width = span.line.to_string().len();
            let source_width =
                terminal_width.map(|width| width.saturating_sub(line_number_width + 3).max(1));
            let (rendered_line, marker_start, marker_length) =
                source_line_with_marker(line, span, source_width);
            rendered.push_str(&format!(
                "\n{:>width$} |\n{:>width$} | {rendered_line}\n{:>width$} | {}{}",
                "",
                span.line,
                "",
                " ".repeat(marker_start),
                "^".repeat(marker_length),
                width = line_number_width,
            ));
        }
        rendered.push_str(&format!("\n   = What happened: {}", code.explanation()));
        rendered.push_str(&format!("\n   = Try this: {}", code.suggestion()));
        rendered.push_str(&format!("\n   = Details: {message}"));

        rendered
    }
}

impl fmt::Display for SimplyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.render("<unknown>", ""))
    }
}

impl Error for SimplyError {}

#[derive(Debug)]
struct SourceChunk {
    text: String,
    source_start: usize,
    source_end: usize,
    cell_start: usize,
    cell_end: usize,
}

fn source_line_with_marker(
    line: &str,
    span: &Span,
    max_width: Option<usize>,
) -> (String, usize, usize) {
    let mut chunks = Vec::new();
    let mut source_column = 0;
    let mut cell_column = 0;
    for grapheme in line.graphemes(true) {
        let rendered = if grapheme == "\t" {
            " ".repeat(4 - cell_column % 4)
        } else if grapheme.chars().any(char::is_control) {
            grapheme
                .chars()
                .map(|character| character.escape_default().to_string())
                .collect::<String>()
        } else {
            grapheme.to_owned()
        };
        let width = UnicodeWidthStr::width(rendered.as_str());
        let source_length = grapheme.chars().count();
        chunks.push(SourceChunk {
            text: rendered,
            source_start: source_column,
            source_end: source_column + source_length,
            cell_start: cell_column,
            cell_end: cell_column + width,
        });
        source_column += source_length;
        cell_column += width;
    }

    let start_index = span.column.saturating_sub(1).min(line.chars().count());
    let end_index = start_index
        .saturating_add(span.length.max(1))
        .min(line.chars().count());
    let marker_start = cell_column_at(&chunks, start_index, cell_column);
    let marker_end = cell_column_at(&chunks, end_index, cell_column);
    let marker_length = marker_end.saturating_sub(marker_start).max(1);
    let Some(max_width) = max_width.filter(|width| cell_column > *width) else {
        return (
            chunks.into_iter().map(|chunk| chunk.text).collect(),
            marker_start,
            marker_length,
        );
    };

    let (window_start, window_end) =
        source_window(&chunks, marker_start, marker_length, cell_column, max_width);
    let mut rendered_line = String::new();
    if window_start > 0 {
        rendered_line.push('.');
    }
    for chunk in &chunks {
        if chunk.cell_start >= window_start && chunk.cell_end <= window_end {
            rendered_line.push_str(&chunk.text);
        }
    }
    if window_end < cell_column {
        rendered_line.push('.');
    }
    let visible_marker_start =
        marker_start.saturating_sub(window_start) + usize::from(window_start > 0);
    let visible_marker_end =
        marker_end.min(window_end).saturating_sub(window_start) + usize::from(window_start > 0);
    (
        rendered_line,
        visible_marker_start,
        visible_marker_end
            .saturating_sub(visible_marker_start)
            .max(1),
    )
}

fn cell_column_at(chunks: &[SourceChunk], source_column: usize, total_width: usize) -> usize {
    for chunk in chunks {
        if source_column <= chunk.source_start {
            return chunk.cell_start;
        }
        if source_column < chunk.source_end {
            return chunk.cell_start;
        }
    }
    total_width
}

fn source_window(
    chunks: &[SourceChunk],
    marker_start: usize,
    marker_length: usize,
    total_width: usize,
    max_width: usize,
) -> (usize, usize) {
    let mut left_clipped = false;
    let mut right_clipped = false;
    let mut window_start = 0;
    let mut window_end = total_width;

    for _ in 0..4 {
        let content_width = max_width
            .saturating_sub(usize::from(left_clipped) + usize::from(right_clipped))
            .max(1);
        window_start = marker_start
            .saturating_sub(content_width.saturating_sub(marker_length.min(content_width)) / 2);
        window_start = window_start.min(total_width.saturating_sub(content_width));
        window_end = (window_start + content_width).min(total_width);

        if let Some(chunk) = chunks
            .iter()
            .find(|chunk| chunk.cell_start < window_start && window_start < chunk.cell_end)
        {
            window_start = chunk.cell_end;
        }
        if let Some(chunk) = chunks
            .iter()
            .find(|chunk| chunk.cell_start < window_end && window_end < chunk.cell_end)
        {
            window_end = chunk.cell_start;
        }
        window_end = window_end.max(window_start);

        let new_left_clipped = window_start > 0;
        let new_right_clipped = window_end < total_width;
        if new_left_clipped == left_clipped && new_right_clipped == right_clipped {
            break;
        }
        left_clipped = new_left_clipped;
        right_clipped = new_right_clipped;
    }
    (window_start, window_end)
}

fn escape_control_characters(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_control() {
                character.escape_default().to_string()
            } else {
                character.to_string()
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{DiagnosticCategory, DiagnosticCode, SimplyError, Span};
    use unicode_width::UnicodeWidthStr;

    #[test]
    fn renders_source_context() {
        let error = SimplyError::Runtime {
            span: Span::with_length(2, 5, 4),
            code: DiagnosticCode::RuntimeGeneral,
            message: "unknown variable `name`".into(),
        };

        let rendered = error.render("example.si", "Say \"ok\"\nSay name\n");
        assert!(rendered.starts_with("error[E0206] (Runtime error)\n  --> example.si:2:5"));
        assert!(
            rendered.contains("= What happened: The program could not complete this operation.")
        );
        assert!(rendered.contains("= Try this: Review the operation"));
        assert!(rendered.contains("= Details: unknown variable `name`"));
        assert!(rendered.contains("\n  |\n2 | Say name\n  |     ^^^^"));
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
            DiagnosticCode::RuntimeConversion,
            DiagnosticCode::CliUsage,
            DiagnosticCode::InvalidNumericLiteral,
            DiagnosticCode::SemanticDestructure,
            DiagnosticCode::SemanticCollection,
            DiagnosticCode::SemanticField,
            DiagnosticCode::SemanticIndex,
            DiagnosticCode::SemanticTupleIndex,
            DiagnosticCode::SemanticMatrixIndex,
            DiagnosticCode::SemanticMissingReturn,
            DiagnosticCode::SemanticCollectionOperation,
            DiagnosticCode::InvalidNumber,
            DiagnosticCode::RuntimeTypeMismatch,
            DiagnosticCode::RuntimeDeclaration,
            DiagnosticCode::RuntimeMutability,
        ];
        let mut unique = std::collections::HashSet::new();
        assert!(codes.iter().all(|code| unique.insert(code.as_str())));
    }

    #[test]
    fn diagnostic_category_matches_its_variant_and_code() {
        let error = SimplyError::Semantic {
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
                .starts_with("error[E0003] (Semantic error)\n  --> program.si:2:3")
        );
        assert_eq!(error.code().category(), error.category());
    }

    #[test]
    fn renders_command_errors_as_command_errors() {
        let error = SimplyError::Command {
            span: Span::new(0, 0),
            code: DiagnosticCode::CliUsage,
            message: "use `simply --help`".into(),
        };

        assert_eq!(
            error.render("simply", ""),
            "error[E0301] (Command error)\n  --> simply\n   = What happened: The command or option was not used in a supported way.\n   = Try this: Run `simply --help` to see supported commands and options.\n   = Details: use `simply --help`"
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

    #[test]
    fn clamps_source_markers_to_the_available_line() {
        let error = SimplyError::Parse {
            span: Span::with_length(1, usize::MAX, usize::MAX),
            code: DiagnosticCode::UnexpectedToken,
            message: "unexpected token".into(),
        };
        let rendered = error.render("program.si", "é\n");
        assert!(rendered.contains("1 | é"));
        assert!(rendered.contains("  |  ^"));
        assert!(!rendered.contains(&"^".repeat(128)));
    }

    #[test]
    fn aligns_markers_after_tabs() {
        let error = SimplyError::Parse {
            span: Span::new(1, 3),
            code: DiagnosticCode::UnexpectedToken,
            message: "unexpected token".into(),
        };
        let rendered = error.render("program.si", "a\tb\n");
        assert!(rendered.contains("1 | a   b\n  |     ^"));
    }

    #[test]
    fn aligns_markers_using_unicode_terminal_cell_width() {
        let error = SimplyError::Parse {
            span: Span::new(1, 5),
            code: DiagnosticCode::UnexpectedToken,
            message: "unexpected token".into(),
        };
        let rendered = error.render("unicode.si", "a你e\u{301}😀z\n");

        assert!(rendered.contains("1 | a你e\u{301}😀z"));
        assert!(rendered.contains("  |     ^^"));
    }

    #[test]
    fn clips_long_source_around_the_error_to_fit_terminal_width() {
        let source_line = format!("{}target{}", "left ".repeat(10), " right".repeat(10));
        let source = format!("Say {source_line}\n");
        let error = SimplyError::Parse {
            span: Span::new(1, 56),
            code: DiagnosticCode::UnexpectedToken,
            message: "unexpected token".into(),
        };
        let rendered = error.render_with_terminal_width("long.si", &source, Some(24));
        let rendered_source = rendered
            .lines()
            .find(|line| line.starts_with("1 | "))
            .expect("rendered source line should be present");

        assert!(rendered_source.contains("target"));
        assert!(rendered_source.starts_with("1 | ."));
        assert!(rendered_source.ends_with('.'));
        assert!(UnicodeWidthStr::width(rendered_source) <= 24);
        let marker_line = rendered
            .lines()
            .find(|line| line.starts_with("  | "))
            .expect("marker line should be present");
        assert!(UnicodeWidthStr::width(marker_line) <= 24);
    }

    #[test]
    fn keeps_tabbed_source_within_terminal_width_when_clipped() {
        let source_line = format!("{}target{}", "prefix\t".repeat(8), "\t suffix".repeat(8));
        let source = format!("{source_line}\n");
        let column = source_line
            .chars()
            .position(|character| character == 't')
            .expect("target should be present")
            + 1;
        let error = SimplyError::Parse {
            span: Span::new(1, column),
            code: DiagnosticCode::UnexpectedToken,
            message: "unexpected token".into(),
        };

        for terminal_width in 8..40 {
            let rendered =
                error.render_with_terminal_width("tabs.si", &source, Some(terminal_width));
            for line in rendered.lines().filter(|line| line.starts_with("1 | ")) {
                assert!(
                    UnicodeWidthStr::width(line) <= terminal_width,
                    "source line exceeds {terminal_width} cells: {line}"
                );
            }
            for line in rendered.lines().filter(|line| line.starts_with("  | ")) {
                assert!(
                    UnicodeWidthStr::width(line) <= terminal_width,
                    "marker line exceeds {terminal_width} cells: {line}"
                );
            }
        }
    }

    #[test]
    fn leaves_source_unclipped_when_terminal_width_is_unknown() {
        let source_line = "value ".repeat(20);
        let source = format!("Say {source_line}\n");
        let error = SimplyError::Parse {
            span: Span::new(1, 10),
            code: DiagnosticCode::UnexpectedToken,
            message: "unexpected token".into(),
        };
        let rendered = error.render_with_terminal_width("pipe.si", &source, None);

        assert!(rendered.contains(&source_line));
    }

    #[test]
    fn keeps_control_characters_from_breaking_the_diagnostic_layout() {
        let error = SimplyError::Runtime {
            span: Span::new(1, 1),
            code: DiagnosticCode::RuntimeGeneral,
            message: "first line\nsecond line\t\u{1b}[31m".into(),
        };
        let rendered = error.render("bad\n\u{1b}[32m.si", "Say \u{1b}[31mname\n");

        assert!(rendered.contains("  --> bad\\n\\u{1b}[32m.si:1:1"));
        assert!(rendered.contains("1 | Say \\u{1b}[31mname"));
        assert!(rendered.contains("= Details: first line\\nsecond line\\t\\u{1b}[31m"));
        assert_eq!(rendered.lines().count(), 8);
        assert!(!rendered.contains('\u{1b}'));
    }

    #[test]
    fn every_error_category_uses_the_same_diagnostic_sections() {
        let errors = [
            SimplyError::Lex {
                span: Span::new(1, 1),
                code: DiagnosticCode::InvalidCharacter,
                message: "bad character".into(),
            },
            SimplyError::Lex {
                span: Span::new(1, 1),
                code: DiagnosticCode::InvalidNumber,
                message: "bad number".into(),
            },
            SimplyError::Parse {
                span: Span::new(1, 1),
                code: DiagnosticCode::UnexpectedToken,
                message: "unexpected token".into(),
            },
            SimplyError::Semantic {
                span: Span::new(1, 1),
                code: DiagnosticCode::UndefinedVariable,
                message: "unknown name".into(),
            },
            SimplyError::Runtime {
                span: Span::new(1, 1),
                code: DiagnosticCode::RuntimeGeneral,
                message: "operation failed".into(),
            },
            SimplyError::Runtime {
                span: Span::new(1, 1),
                code: DiagnosticCode::RuntimeTypeMismatch,
                message: "wrong runtime type".into(),
            },
            SimplyError::Runtime {
                span: Span::new(1, 1),
                code: DiagnosticCode::RuntimeDeclaration,
                message: "duplicate runtime binding".into(),
            },
            SimplyError::Runtime {
                span: Span::new(1, 1),
                code: DiagnosticCode::RuntimeMutability,
                message: "immutable runtime binding".into(),
            },
            SimplyError::Command {
                span: Span::new(0, 0),
                code: DiagnosticCode::CliUsage,
                message: "invalid command".into(),
            },
        ];

        for error in errors {
            let rendered = error.render("example.si", "source\n");
            assert!(rendered.starts_with("error["));
            assert!(rendered.contains("\n  --> "));
            assert!(rendered.contains("= What happened: "));
            assert!(rendered.contains("= Try this: "));
            assert!(rendered.contains("= Details: "));
            assert_eq!(error.category(), error.code().category());
        }
    }
}
