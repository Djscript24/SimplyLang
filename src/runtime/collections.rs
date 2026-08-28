use crate::{
    ast::CollectionOperation,
    error::{SimplyError, Span},
    runtime::value::Value,
};

pub(crate) fn index(
    target: Value,
    index: Value,
    span: Option<&Span>,
) -> Result<Value, SimplyError> {
    if let (Value::Matrix(rows), Value::Tuple(coordinates)) = (&target, &index) {
        if coordinates.len() != 2 {
            return Err(error(span, "matrix index requires row and column"));
        }
        let row = coordinate(&coordinates[0], span)?;
        let column = coordinate(&coordinates[1], span)?;
        return match rows.get(row) {
            Some(Value::Array(values)) => values
                .get(column)
                .cloned()
                .ok_or_else(|| error(span, "matrix index out of bounds")),
            _ => Err(error(span, "matrix row is not an array")),
        };
    }
    if let Value::Hash(values) | Value::Tree(values) = &target {
        if let Value::String(key) = index {
            return values
                .get(&key)
                .cloned()
                .ok_or_else(|| error(span, format!("unknown key `{key}`")));
        }
        return Err(error(span, "hash key must be a string"));
    }
    let index = collection_index(index, span)?;
    match target {
        Value::Array(values)
        | Value::List(values)
        | Value::Tuple(values)
        | Value::Matrix(values) => values
            .get(index)
            .cloned()
            .ok_or_else(|| error(span, "collection index out of bounds")),
        _ => Err(error(span, "value is not indexable")),
    }
}

pub(crate) fn set_index(
    target: &mut Value,
    index: Value,
    value: Value,
    span: Option<&Span>,
) -> Result<(), SimplyError> {
    if let (Value::Hash(values), Value::String(key)) = (&mut *target, &index) {
        values.insert(key.clone(), value);
        return Ok(());
    }
    let index = collection_index(index, span)?;
    match target {
        Value::Array(values) | Value::List(values) => {
            let slot = values
                .get_mut(index)
                .ok_or_else(|| error(span, "collection index out of bounds"))?;
            *slot = value;
            Ok(())
        }
        _ => Err(error(span, "value is not a mutable collection")),
    }
}

pub(crate) fn mutate_list(
    target: &mut Value,
    operation: &CollectionOperation,
    value: Value,
    name: &str,
    span: Option<&Span>,
) -> Result<(), SimplyError> {
    match target {
        Value::List(values) => {
            match operation {
                CollectionOperation::Add => values.push(value),
                CollectionOperation::Remove => {
                    if let Some(position) = values.iter().position(|item| item == &value) {
                        values.remove(position);
                    }
                }
            }
            Ok(())
        }
        _ => Err(SimplyError::Runtime {
            span: span.cloned().unwrap_or_else(|| Span::new(0, 0)),
            code: crate::error::DiagnosticCode::RuntimeCollection,
            message: format!("`{name}` is not a list"),
        }),
    }
}

fn coordinate(value: &Value, span: Option<&Span>) -> Result<usize, SimplyError> {
    match value {
        Value::Int(value) if *value >= 0 => Ok(*value as usize),
        _ => Err(error(span, "matrix index must be integer")),
    }
}

fn collection_index(value: Value, span: Option<&Span>) -> Result<usize, SimplyError> {
    match value {
        Value::Int(value) if value >= 0 => Ok(value as usize),
        _ => Err(error(
            span,
            "collection index must be a non-negative integer",
        )),
    }
}

fn error(span: Option<&Span>, message: impl Into<String>) -> SimplyError {
    SimplyError::Runtime {
        span: span.cloned().unwrap_or_else(|| Span::new(0, 0)),
        code: crate::error::DiagnosticCode::RuntimeGeneral,
        message: message.into(),
    }
}
