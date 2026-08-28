use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Unit,
    String(String),
    Int(i64),
    Float(f64),
    Bool(bool),
    Array(Vec<Value>),
    List(Vec<Value>),
    Tuple(Vec<Value>),
    Hash(BTreeMap<String, Value>),
    Tree(BTreeMap<String, Value>),
    Matrix(Vec<Value>),
}

impl Value {
    pub(crate) fn display(&self) -> String {
        match self {
            Self::Unit => String::new(),
            Self::String(value) => value.clone(),
            Self::Int(value) => value.to_string(),
            Self::Float(value) => value.to_string(),
            Self::Bool(value) => value.to_string(),
            Self::Array(values)
            | Self::List(values)
            | Self::Tuple(values)
            | Self::Matrix(values) => format!(
                "[{}]",
                values
                    .iter()
                    .map(Value::display)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Self::Hash(values) => format!(
                "{{{}}}",
                values
                    .iter()
                    .map(|(key, value)| format!("{key}: {}", value.display()))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Self::Tree(values) => format!(
                "tree {{{}}}",
                values
                    .iter()
                    .map(|(key, value)| format!("{key}: {}", value.display()))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Value;

    #[test]
    fn displays_nested_values_without_evaluator() {
        let value = Value::List(vec![Value::Int(1), Value::String("two".into())]);
        assert_eq!(value.display(), "[1, two]");
    }
}
