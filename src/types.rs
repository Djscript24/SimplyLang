//! types.rs — Simply type definitions
//! Defines the language's primitive, collection, function, and unit types used by analysis and runtime checks.
//! Key component: Type and its compatibility helpers.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum DeclarationKind {
    Struct,
    Enum,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DeclarationIdentity {
    pub module: String,
    pub local_name: String,
    pub kind: DeclarationKind,
}

impl DeclarationIdentity {
    pub fn new(
        module: impl Into<String>,
        local_name: impl Into<String>,
        kind: DeclarationKind,
    ) -> Self {
        Self {
            module: module.into(),
            local_name: local_name.into(),
            kind,
        }
    }

    pub fn unresolved(local_name: impl Into<String>, kind: DeclarationKind) -> Self {
        Self::new("memory://unresolved", local_name, kind)
    }

    pub fn is_unresolved(&self) -> bool {
        self.module == "memory://unresolved"
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Type {
    Unknown,
    Unit,
    String,
    Int,
    Float,
    Bool,
    Range,
    CsvStream,
    Array(Box<Type>),
    List(Box<Type>),
    Vector(Box<Type>, Option<usize>),
    Tuple(Vec<Type>),
    Hash,
    HashValues(Box<Type>),
    Tree,
    TreeValues(Box<Type>),
    Matrix,
    TypedMatrix(Box<Type>, Option<usize>, Option<usize>),
    Struct(DeclarationIdentity),
    Enum(DeclarationIdentity),
    Function {
        parameters: Vec<Option<Box<Type>>>,
        return_type: Option<Box<Type>>,
    },
}

impl Type {
    pub fn name(&self) -> String {
        match self {
            Self::Unknown => "Unknown".into(),
            Self::Unit => "Unit".into(),
            Self::String => "String".into(),
            Self::Int => "Int".into(),
            Self::Float => "Float".into(),
            Self::Bool => "Bool".into(),
            Self::Range => "Range".into(),
            Self::CsvStream => "CsvStream".into(),
            Self::Array(element) => format!("Array[{}]", element.name()),
            Self::List(element) => format!("List[{}]", element.name()),
            Self::Vector(element, Some(length)) => {
                format!("Vector[{}, {length}]", element.name())
            }
            Self::Vector(element, None) => format!("Vector[{}]", element.name()),
            Self::Tuple(types) => format!(
                "Tuple[{}]",
                types.iter().map(Self::name).collect::<Vec<_>>().join(", ")
            ),
            Self::Hash => "Hash".into(),
            Self::HashValues(_) => "Hash".into(),
            Self::Tree => "Tree".into(),
            Self::TreeValues(_) => "Tree".into(),
            Self::Matrix => "Matrix".into(),
            Self::TypedMatrix(element, Some(rows), Some(columns)) => {
                format!("Matrix[{}, {rows}, {columns}]", element.name())
            }
            Self::TypedMatrix(element, rows, columns) if rows.is_some() || columns.is_some() => {
                let rows = rows.map_or_else(|| "?".into(), |value| value.to_string());
                let columns = columns.map_or_else(|| "?".into(), |value| value.to_string());
                format!("Matrix[{}, {rows}, {columns}]", element.name())
            }
            Self::TypedMatrix(element, _, _) => format!("Matrix[{}]", element.name()),
            Self::Struct(identity) | Self::Enum(identity) => identity.local_name.clone(),
            Self::Function { .. } => "Function".into(),
        }
    }

    pub fn compatible_with(&self, expected: &Self) -> bool {
        self.compatible_at(expected, false)
    }

    fn compatible_at(&self, expected: &Self, nested: bool) -> bool {
        match (self, expected) {
            (Self::Unknown, Self::Unknown) => true,
            (Self::Unknown, _) | (_, Self::Unknown) => nested,
            (Self::Range, Self::Range) | (Self::CsvStream, Self::CsvStream) => true,
            (Self::HashValues(_), Self::Hash) | (Self::TreeValues(_), Self::Tree) => true,
            (Self::HashValues(actual), Self::HashValues(expected))
            | (Self::TreeValues(actual), Self::TreeValues(expected)) => {
                actual.compatible_at(expected, true)
            }
            (Self::Array(actual), Self::Array(expected))
            | (Self::List(actual), Self::List(expected))
            | (Self::Vector(actual, _), Self::Array(expected))
            | (Self::Vector(actual, _), Self::List(expected)) => {
                actual.compatible_at(expected, true)
            }
            (Self::Vector(actual, actual_len), Self::Vector(expected, expected_len)) => {
                actual.compatible_at(expected, true)
                    && dimensions_compatible(*actual_len, *expected_len)
            }
            (Self::Array(actual), Self::Vector(expected, _))
            | (Self::List(actual), Self::Vector(expected, _)) => {
                actual.compatible_at(expected, true)
            }
            (Self::Tuple(actual), Self::Vector(expected, expected_len)) => {
                actual
                    .iter()
                    .all(|element| element.compatible_at(expected, true))
                    && dimensions_compatible(Some(actual.len()), *expected_len)
            }
            (
                Self::TypedMatrix(actual, actual_rows, actual_columns),
                Self::TypedMatrix(expected, expected_rows, expected_columns),
            ) => {
                actual.compatible_at(expected, true)
                    && dimensions_compatible(*actual_rows, *expected_rows)
                    && dimensions_compatible(*actual_columns, *expected_columns)
            }
            (Self::Vector(row, actual_rows), Self::TypedMatrix(expected, rows, columns)) => {
                match row.as_ref() {
                    Self::Vector(actual, actual_columns) => {
                        actual.compatible_at(expected, true)
                            && dimensions_compatible(*actual_rows, *rows)
                            && dimensions_compatible(*actual_columns, *columns)
                    }
                    _ => false,
                }
            }
            (Self::Array(row), Self::TypedMatrix(expected, _, _))
            | (Self::List(row), Self::TypedMatrix(expected, _, _)) => match row.as_ref() {
                Self::Array(actual) | Self::List(actual) => actual.compatible_at(expected, true),
                _ => false,
            },
            (Self::Vector(row, _), Self::Matrix) => {
                matches!(
                    row.as_ref(),
                    Self::Vector(_, _) | Self::Array(_) | Self::List(_)
                )
            }
            (Self::Array(row), Self::Matrix) | (Self::List(row), Self::Matrix) => {
                matches!(row.as_ref(), Self::Array(_) | Self::List(_))
            }
            (Self::TypedMatrix(_, _, _), Self::Matrix)
            | (Self::Matrix, Self::TypedMatrix(_, _, _)) => true,
            (Self::Tuple(actual), Self::Tuple(expected)) => {
                actual.len() == expected.len()
                    && actual
                        .iter()
                        .zip(expected)
                        .all(|(actual, expected)| actual.compatible_at(expected, true))
            }
            (
                Self::Function {
                    parameters: actual_parameters,
                    return_type: actual_return,
                },
                Self::Function {
                    parameters: expected_parameters,
                    return_type: expected_return,
                },
            ) => {
                actual_parameters.len() == expected_parameters.len()
                    && actual_parameters.iter().zip(expected_parameters).all(
                        |(actual, expected)| match (actual, expected) {
                            (Some(actual), Some(expected)) => actual.compatible_at(expected, true),
                            (None, None) => true,
                            _ => false,
                        },
                    )
                    && match (actual_return, expected_return) {
                        (Some(actual), Some(expected)) => actual.compatible_at(expected, true),
                        (None, None) => true,
                        _ => false,
                    }
            }
            _ => self == expected,
        }
    }
}

fn dimensions_compatible(actual: Option<usize>, expected: Option<usize>) -> bool {
    match (actual, expected) {
        (Some(actual), Some(expected)) => actual == expected,
        _ => true,
    }
}

#[cfg(test)]
#[path = "../tests/internal/types.rs"]
mod tests;
