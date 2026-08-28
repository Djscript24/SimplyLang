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
        self == &Self::Unknown || expected == &Self::Unknown || self == expected
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
}
