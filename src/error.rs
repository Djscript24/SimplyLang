//! error.rs — diagnostics and source spans
//! Defines source locations, diagnostic categories/codes, and the SimplyError types used across the compiler and runtime.
//! Key components: Span, DiagnosticCode, DiagnosticCategory, and SimplyError.
use std::{error::Error, fmt};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::runtime::value::Value;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DiagnosticDefinition {
    identity: DiagnosticCode,
    path: &'static str,
    legacy_code: Option<&'static str>,
    category: DiagnosticCategory,
    explanation: &'static str,
    suggestion: &'static str,
}

macro_rules! define_diagnostic_registry {
    ($( $variant:ident => (
        $path:literal,
        $legacy:literal,
        $category:ident,
        $explanation:literal,
        $suggestion:literal
    ); )+) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum DiagnosticCode {
            $( $variant, )+
        }

        impl DiagnosticCode {
            #[allow(dead_code)]
            pub const ALL: &[Self] = &[$(Self::$variant,)+];
            const DEFINITIONS: &[DiagnosticDefinition] = &[
                $(DiagnosticDefinition {
                    identity: Self::$variant,
                    path: $path,
                    legacy_code: if $legacy.is_empty() { None } else { Some($legacy) },
                    category: DiagnosticCategory::$category,
                    explanation: $explanation,
                    suggestion: $suggestion,
                },)+
            ];

            fn definition(self) -> &'static DiagnosticDefinition {
                Self::DEFINITIONS
                    .iter()
                    .find(|definition| definition.identity == self)
                    .expect("every diagnostic identity has one registry entry")
            }

            pub fn from_code(value: &str) -> Option<Self> {
                Self::DEFINITIONS
                    .iter()
                    .find(|definition| {
                        definition.path == value || definition.legacy_code == Some(value)
                    })
                    .map(|definition| definition.identity)
            }

            pub fn category(self) -> DiagnosticCategory {
                self.definition().category
            }

            fn explanation(self) -> &'static str {
                self.definition().explanation
            }

            fn suggestion(self) -> &'static str {
                self.definition().suggestion
            }

            pub fn as_str(self) -> &'static str {
                self.definition().path
            }

            #[allow(dead_code)]
            pub fn legacy_code(self) -> Option<&'static str> {
                self.definition().legacy_code
            }

            #[allow(dead_code)]
            pub fn parent_code(self) -> Option<String> {
                self.as_str().rsplit_once('.').map(|(parent, _)| parent.to_owned())
            }

            #[allow(dead_code)]
            pub fn ancestors(self) -> Vec<String> {
                let mut result = Vec::new();
                let segments = self.as_str().split('.').collect::<Vec<_>>();
                let mut prefix = segments[0].to_owned();
                result.push(prefix.clone());
                for segment in segments.iter().take(segments.len().saturating_sub(1)).skip(1) {
                    prefix.push('.');
                    prefix.push_str(segment);
                    result.push(prefix.clone());
                }
                result
            }

            #[allow(dead_code)]
            pub fn depth(self) -> usize {
                self.as_str().matches('.').count()
            }

            #[allow(dead_code)]
            pub fn is_descendant_of(self, parent: &str) -> bool {
                self.as_str()
                    .strip_prefix(parent)
                    .is_some_and(|suffix| suffix.starts_with('.'))
            }
        }
    };
}

define_diagnostic_registry! {
    InvalidCharacter => (
        "E.lex.character.invalid",
        "E0101",
        Lex,
        "This character is not part of Simply's language.",
        "Remove the character or replace it with valid Simply syntax."
    );
    UnterminatedString => (
        "E.lex.string.unterminated",
        "E0102",
        Lex,
        "A text value starts with a quote but does not close.",
        "Add the missing closing double quote."
    );
    InvalidNumber => (
        "E.lex.number.invalid",
        "E0106",
        Lex,
        "This number literal is not valid.",
        "Check the number's digits, decimal point, and exponent, and make sure it fits the supported numeric range."
    );
    UnexpectedToken => (
        "E.syntax.token.unexpected",
        "E0103",
        Parse,
        "The code contains something where it was not expected.",
        "Check the syntax immediately around this location."
    );
    ExpectedExpression => (
        "E.syntax.expression.missing",
        "E0104",
        Parse,
        "A value or calculation is missing from this line.",
        "Add the missing value or calculation; after `->`, it may start on the next line."
    );
    UndefinedVariable => (
        "E.semantic.name.undefined",
        "E0001",
        Semantic,
        "This name has not been defined where it is used.",
        "Check the spelling, or define the name before using it."
    );
    TypeMismatch => (
        "E.semantic.type.mismatch",
        "E0003",
        Semantic,
        "The value here is not the kind of value this code requires.",
        "Make the value's type match the required type. Text-to-number conversion only works when the text contains a valid number."
    );
    InvalidReassignment => (
        "E.semantic.binding.reassignment",
        "E0105",
        Semantic,
        "This value cannot be changed in the way requested.",
        "Declare the variable with `mut` if it should be changeable."
    );
    DuplicateDeclaration => (
        "E.semantic.declaration.duplicate",
        "E0017",
        Semantic,
        "This name has already been declared in this scope.",
        "Choose a different name, or reassign the existing mutable variable."
    );
    InvalidFunctionCall => (
        "E.semantic.call.invalid",
        "E0002",
        Semantic,
        "The function call does not match a known function.",
        "Check the function name and the number and types of its arguments."
    );
    InvalidReturn => (
        "E.semantic.control.return-invalid",
        "E0006",
        Semantic,
        "This return statement is not valid in this function.",
        "Put `return` inside a function and return the type declared by that function."
    );
    InvalidBreakContinue => (
        "E.semantic.control.loop-transfer-invalid",
        "E0007",
        Semantic,
        "Break and continue can only be used inside a loop.",
        "Move `break` or `continue` inside a `for` or `while` loop."
    );
    RuntimeCollection => (
        "E.runtime.collection.operation",
        "E0201",
        Runtime,
        "This operation could not be completed on the collection.",
        "Check the collection contents, index, and operation being used."
    );
    RuntimeDivision => (
        "E.runtime.numeric.division-by-zero",
        "E0202",
        Runtime,
        "A number was divided by zero.",
        "Make sure the divisor is not zero."
    );
    RuntimeArithmetic => (
        "E.runtime.numeric.arithmetic-invalid",
        "E0203",
        Runtime,
        "This calculation could not produce a valid number.",
        "Check the operands and make sure the result stays within a valid numeric range."
    );
    RuntimeImport => (
        "E.runtime.module.import",
        "E0204",
        Runtime,
        "Simply could not load a required file.",
        "Check that the file exists and that its path is correct."
    );
    RuntimeMessage => (
        "E.runtime.message.dispatch",
        "E0205",
        Runtime,
        "This message cannot be sent to that value.",
        "Check that the receiver supports this message and that its arguments are valid."
    );
    RuntimeGeneral => (
        "E.runtime.operation.failed",
        "E0206",
        Runtime,
        "The program could not complete this operation.",
        "Review the operation and the values it uses; the detail above has more context."
    );
    RuntimeArgument => (
        "E.runtime.argument.invalid",
        "E0211",
        Runtime,
        "A runtime operation received arguments it cannot accept.",
        "Check the number, order, and values of the arguments."
    );
    RuntimeAssertion => (
        "E.runtime.assertion.failed",
        "E0212",
        Runtime,
        "A program assertion did not hold.",
        "Check the asserted condition, or provide a message that explains the required condition."
    );
    RuntimeIo => (
        "E.runtime.io.operation-failed",
        "E0213",
        Runtime,
        "A runtime input/output operation could not be completed.",
        "Check the path, permissions, and availability of the requested input or output."
    );
    RuntimeLimit => (
        "E.runtime.limit.exceeded",
        "E0214",
        Runtime,
        "A runtime resource limit was exceeded.",
        "Reduce the operation's size or complexity and try again."
    );
    RuntimeEnumVariant => (
        "E.runtime.enum.variant-unknown",
        "E0215",
        Runtime,
        "The requested enum variant does not exist.",
        "Check the enum and variant names against the enum declaration."
    );
    RuntimeName => (
        "E.runtime.name.undefined",
        "E0216",
        Runtime,
        "A runtime name does not resolve to a declared value or callable.",
        "Check the spelling and ensure the declaration is in scope before use."
    );
    RuntimeControl => (
        "E.runtime.control.invalid",
        "E0217",
        Runtime,
        "A control-flow action reached a context where it is not valid.",
        "Place the control statement inside its required function, loop, or module context."
    );
    RuntimeConversion => (
        "E.runtime.conversion.failed",
        "E0207",
        Runtime,
        "Text supplied for a number conversion is not a valid number.",
        "Use digits only for `to_int` (for example, `\"42\"`) or a valid decimal/exponent for `to_float` (for example, `\"3.5\"`)."
    );
    CliUsage => (
        "E.cli.usage",
        "E0301",
        Command,
        "The command or option was not used in a supported way.",
        "Run `simply --help` to see supported commands and options."
    );
    InvalidNumericLiteral => (
        "E.semantic.conversion.literal-invalid",
        "E0018",
        Semantic,
        "This text cannot be read as the requested kind of number.",
        "Replace this text with a valid number, or keep it as text instead of converting it."
    );
    SemanticDestructure => (
        "E.semantic.pattern.destructure-invalid",
        "E0011",
        Semantic,
        "The value does not match the names used to unpack it.",
        "Make the number and order of names match the value being unpacked."
    );
    SemanticCollection => (
        "E.semantic.collection.shape-invalid",
        "E0012",
        Semantic,
        "The value does not have the collection shape this operation needs.",
        "Use a supported collection with the shape and element types this operation expects."
    );
    SemanticField => (
        "E.semantic.field.missing",
        "E0013",
        Semantic,
        "This value does not have the requested field.",
        "Check the field name, or use a value that contains that field."
    );
    SemanticIndex => (
        "E.semantic.index.invalid",
        "E0014",
        Semantic,
        "This value cannot be accessed with an index.",
        "Use an array, list, tuple, hash, tree, or matrix with a valid index."
    );
    SemanticTupleIndex => (
        "E.semantic.index.tuple.invalid",
        "E0015",
        Semantic,
        "This tuple index is not valid.",
        "Use an integer index within the tuple's available positions."
    );
    SemanticMatrixIndex => (
        "E.semantic.index.matrix.invalid",
        "E0016",
        Semantic,
        "This matrix index or shape is not valid.",
        "Check the matrix dimensions and use valid row and column indexes."
    );
    SemanticMissingReturn => (
        "E.semantic.control.return-missing",
        "E0005",
        Semantic,
        "This function promises a value but may finish without returning one.",
        "Add a return value on every possible path through the function."
    );
    SemanticCollectionOperation => (
        "E.semantic.collection.operation-invalid",
        "E0010",
        Semantic,
        "This collection operation is not allowed for this value or binding.",
        "Check the collection type and declare it with `mut` before changing it."
    );
    RuntimeTypeMismatch => (
        "E.runtime.type.mismatch",
        "E0208",
        Runtime,
        "A value has a different type from the one this operation requires.",
        "Use a value with the required type; if it comes from dynamic data, check or convert it before this operation."
    );
    RuntimeDeclaration => (
        "E.runtime.declaration.conflict",
        "E0209",
        Runtime,
        "A name could not be declared in the current scope.",
        "Use a new name in this scope, or update the existing mutable binding."
    );
    RuntimeMutability => (
        "E.runtime.binding.immutable",
        "E0210",
        Runtime,
        "This binding cannot be changed in the requested way.",
        "Declare the binding with `mut` before reassigning it or changing its contents."
    );
    InvalidCatchCode => (
        "E.semantic.catch.code-unknown",
        "E0019",
        Semantic,
        "This catch clause refers to an unknown diagnostic code.",
        "Use a diagnostic code listed by Simply, such as `E.runtime.numeric.division-by-zero`."
    );
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
    Thrown {
        span: Span,
        value: Box<Value>,
        message: String,
    },
    InSource {
        error: Box<SimplyError>,
        filename: String,
        source: String,
    },
    Command {
        span: Span,
        code: DiagnosticCode,
        message: String,
    },
}

impl SimplyError {
    pub(crate) fn in_source(self, filename: String, source: String) -> Self {
        if matches!(self, Self::InSource { .. }) {
            self
        } else {
            Self::InSource {
                error: Box::new(self),
                filename,
                source,
            }
        }
    }

    pub(crate) fn with_span(self, span: Span) -> Self {
        match self {
            Self::InSource {
                error,
                filename,
                source,
            } => Self::InSource {
                error: Box::new(error.with_span(span)),
                filename,
                source,
            },
            Self::Lex { code, message, .. } => Self::Lex {
                span,
                code,
                message,
            },
            Self::Parse { code, message, .. } => Self::Parse {
                span,
                code,
                message,
            },
            Self::Semantic { code, message, .. } => Self::Semantic {
                span,
                code,
                message,
            },
            Self::Runtime { code, message, .. } => Self::Runtime {
                span,
                code,
                message,
            },
            Self::Thrown { value, message, .. } => Self::Thrown {
                span,
                value,
                message,
            },
            Self::Command { code, message, .. } => Self::Command {
                span,
                code,
                message,
            },
        }
    }

    pub(crate) fn with_context(self, context: String) -> Self {
        match self {
            Self::InSource {
                error,
                filename,
                source,
            } => Self::InSource {
                error: Box::new(error.with_context(context)),
                filename,
                source,
            },
            Self::Lex {
                span,
                code,
                message,
            } => Self::Lex {
                span,
                code,
                message: format!("{context}: {message}"),
            },
            Self::Parse {
                span,
                code,
                message,
            } => Self::Parse {
                span,
                code,
                message: format!("{context}: {message}"),
            },
            Self::Semantic {
                span,
                code,
                message,
            } => Self::Semantic {
                span,
                code,
                message: format!("{context}: {message}"),
            },
            Self::Runtime {
                span,
                code,
                message,
            } => Self::Runtime {
                span,
                code,
                message: format!("{context}: {message}"),
            },
            Self::Thrown {
                span,
                value,
                message,
            } => Self::Thrown {
                span,
                value,
                message: format!("{context}: {message}"),
            },
            Self::Command {
                span,
                code,
                message,
            } => Self::Command {
                span,
                code,
                message: format!("{context}: {message}"),
            },
        }
    }

    pub fn category(&self) -> DiagnosticCategory {
        match self {
            Self::InSource { error, .. } => error.category(),
            Self::Lex { .. } => DiagnosticCategory::Lex,
            Self::Parse { .. } => DiagnosticCategory::Parse,
            Self::Semantic { .. } => DiagnosticCategory::Semantic,
            Self::Runtime { .. } | Self::Thrown { .. } => DiagnosticCategory::Runtime,
            Self::Command { .. } => DiagnosticCategory::Command,
        }
    }

    pub fn code(&self) -> DiagnosticCode {
        match self {
            Self::InSource { error, .. } => error.code(),
            Self::Lex { code, .. }
            | Self::Parse { code, .. }
            | Self::Semantic { code, .. }
            | Self::Runtime { code, .. }
            | Self::Command { code, .. } => *code,
            Self::Thrown { .. } => DiagnosticCode::RuntimeGeneral,
        }
    }

    pub fn span(&self) -> &Span {
        match self {
            Self::InSource { error, .. } => error.span(),
            Self::Lex { span, .. }
            | Self::Parse { span, .. }
            | Self::Semantic { span, .. }
            | Self::Runtime { span, .. }
            | Self::Thrown { span, .. }
            | Self::Command { span, .. } => span,
        }
    }

    pub fn message(&self) -> &str {
        match self {
            Self::InSource { error, .. } => error.message(),
            Self::Lex { message, .. }
            | Self::Parse { message, .. }
            | Self::Semantic { message, .. }
            | Self::Runtime { message, .. }
            | Self::Thrown { message, .. }
            | Self::Command { message, .. } => message,
        }
    }

    pub(crate) fn thrown_value(&self) -> Option<&Value> {
        match self {
            Self::InSource { error, .. } => error.thrown_value(),
            Self::Thrown { value, .. } => Some(value),
            _ => None,
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
        if let Self::InSource {
            error,
            filename,
            source,
        } = self
        {
            return error.render_with_terminal_width(filename, source, terminal_width);
        }
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
            Self::InSource { error, .. } => error.message(),
            Self::Lex { message, .. }
            | Self::Parse { message, .. }
            | Self::Semantic { message, .. }
            | Self::Runtime { message, .. }
            | Self::Thrown { message, .. }
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
