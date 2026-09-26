//! types.rs — Simply type definitions
//! Defines the language's primitive, collection, function, and unit types used by analysis and runtime checks.
//! Key component: Type and its compatibility helpers.
#[derive(Debug, Clone, PartialEq)]
pub enum Type {
    Unknown,
    Unit,
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
            Self::Array(element) => format!("Array[{}]", element.name()),
            Self::List(element) => format!("List[{}]", element.name()),
            Self::Tuple(types) => format!(
                "Tuple[{}]",
                types.iter().map(Self::name).collect::<Vec<_>>().join(", ")
            ),
            Self::Hash => "Hash".into(),
            Self::Tree => "Tree".into(),
            Self::Matrix => "Matrix".into(),
            Self::Function { .. } => "Function".into(),
        }
    }

    pub fn compatible_with(&self, expected: &Self) -> bool {
        match (self, expected) {
            (Self::Unknown, _) | (_, Self::Unknown) => true,
            (Self::Array(actual), Self::Array(expected))
            | (Self::List(actual), Self::List(expected)) => actual.compatible_with(expected),
            (Self::Tuple(actual), Self::Tuple(expected)) => {
                actual.len() == expected.len()
                    && actual
                        .iter()
                        .zip(expected)
                        .all(|(actual, expected)| actual.compatible_with(expected))
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
                            (Some(actual), Some(expected)) => actual.compatible_with(expected),
                            (None, None) => true,
                            _ => false,
                        },
                    )
                    && match (actual_return, expected_return) {
                        (Some(actual), Some(expected)) => actual.compatible_with(expected),
                        (None, None) => true,
                        _ => false,
                    }
            }
            _ => self == expected,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Type;

    #[test]
    fn formats_nested_types_consistently() {
        let typ = Type::List(Box::new(Type::Tuple(vec![Type::Int, Type::String])));
        assert_eq!(typ.name(), "List[Tuple[Int, String]]");
    }

    #[test]
    fn unknown_is_compatible_without_changing_concrete_types() {
        assert!(Type::Unknown.compatible_with(&Type::Int));
        assert!(Type::Int.compatible_with(&Type::Unknown));
        assert!(!Type::Int.compatible_with(&Type::String));
    }

    #[test]
    fn nested_unknown_is_compatible_with_concrete_collection_types() {
        assert!(
            Type::List(Box::new(Type::Unknown)).compatible_with(&Type::List(Box::new(Type::Int)))
        );
        assert!(
            Type::Tuple(vec![Type::Unknown, Type::String])
                .compatible_with(&Type::Tuple(vec![Type::Int, Type::String]))
        );
    }
}
