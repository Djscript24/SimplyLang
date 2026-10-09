//! runtime/collections.rs — collection operations
//! Implements indexing and mutation behavior for Simply arrays, lists, tuples, hashes, and matrices.
//! Key component: collection index and mutation helpers.
use crate::{
    ast::CollectionOperation,
    error::{SimplyError, Span},
    runtime::value::Value,
};

pub(crate) fn index(
    target: &Value,
    index: &Value,
    span: Option<&Span>,
) -> Result<Value, SimplyError> {
    if let (Value::Matrix(rows), Value::Tuple(coordinates)) = (target, index) {
        if coordinates.len() != 2 {
            return Err(error(span, "matrix index requires row and column"));
        }
        let row = coordinate(&coordinates[0], span)?;
        let column = coordinate(&coordinates[1], span)?;
        return match rows.get(row) {
            Some(Value::Array(values)) | Some(Value::List(values)) => values
                .get(column)
                .ok_or_else(|| error(span, "matrix index out of bounds")),
            Some(_) => Err(error(span, "matrix row is not an array or list")),
            None => Err(error(span, "matrix index out of bounds")),
        };
    }
    if let (Value::Array(rows) | Value::List(rows), Value::Tuple(coordinates)) = (target, index) {
        if coordinates.len() != 2 {
            return Err(error(span, "matrix index requires row and column"));
        }
        let row = coordinate(&coordinates[0], span)?;
        let column = coordinate(&coordinates[1], span)?;
        let row_value = rows
            .get(row)
            .ok_or_else(|| error(span, "matrix index out of bounds"))?;
        return match row_value {
            Value::Array(values) | Value::List(values) => values
                .get(column)
                .ok_or_else(|| error(span, "matrix index out of bounds")),
            _ => Err(error(span, "matrix row is not an array or list")),
        };
    }
    if let Value::Hash(values) = target {
        if let Value::String(key) = index {
            return values
                .with(|values| values.get(key).cloned())
                .flatten()
                .ok_or_else(|| error(span, format!("unknown key `{key}`")));
        }
        return Err(error(span, "hash key must be a string"));
    }
    if let Value::Struct(instance) = target {
        if let Value::String(name) = index {
            return instance
                .with(|instance| instance.fields.get(name).cloned())
                .flatten()
                .ok_or_else(|| error(span, format!("unknown field `{name}`")));
        }
        return Err(error(span, "struct field name must be a string"));
    }
    if let Value::Range { start, end, step } = target {
        let index = match index {
            Value::Int(index) if *index >= 0 => *index,
            _ => {
                return Err(error(
                    span,
                    "collection index must be a non-negative integer",
                ));
            }
        };
        let value = i128::from(*start) + i128::from(index) * i128::from(*step);
        let in_bounds = if *step > 0 {
            value < i128::from(*end)
        } else {
            value > i128::from(*end)
        };
        if !in_bounds {
            return Err(error(span, "collection index out of bounds"));
        }
        return i64::try_from(value)
            .map(Value::Int)
            .map_err(|_| error(span, "collection index out of bounds"));
    }
    let index = collection_index(index, span)?;
    match target {
        Value::String(value) => value
            .chars()
            .nth(index)
            .map(|character| Value::String(character.to_string()))
            .ok_or_else(|| error(span, "string index out of bounds")),
        Value::Array(values) | Value::List(values) => values
            .with(|values| values.get(index).cloned())
            .flatten()
            .ok_or_else(|| error(span, "collection index out of bounds")),
        Value::Tuple(values) | Value::Matrix(values) => values
            .get(index)
            .cloned()
            .ok_or_else(|| error(span, "collection index out of bounds")),
        _ => Err(error(span, "value is not indexable")),
    }
}

pub(crate) fn contains(
    target: &Value,
    searched: &Value,
    span: Option<&Span>,
    invalid_target_message: &str,
) -> Result<bool, SimplyError> {
    match target {
        Value::String(value) => Ok(match searched {
            Value::String(searched) => value.contains(searched),
            _ => false,
        }),
        Value::Array(values) | Value::List(values) => values
            .get_cloned()
            .map(|values| values.iter().any(|value| value == searched))
            .ok_or_else(|| error(span, "collection storage is no longer available")),
        Value::Tuple(values) => Ok(values.iter().any(|value| value == searched)),
        Value::Range { start, end, step } => {
            let Value::Int(value) = searched else {
                return Ok(false);
            };
            if *step == 0 {
                return Err(error(span, "range step cannot be zero"));
            }
            let in_bounds = if *step > 0 {
                *value >= *start && *value < *end
            } else {
                *value <= *start && *value > *end
            };
            Ok(in_bounds && (i128::from(*value) - i128::from(*start)) % i128::from(*step) == 0)
        }
        Value::Hash(values) => values
            .get_cloned()
            .map(|values| values.values().any(|value| value == searched))
            .ok_or_else(|| error(span, "hash storage is no longer available")),
        _ => Err(error(span, invalid_target_message)),
    }
}

pub(crate) fn set_index(
    target: &mut Value,
    index: Value,
    value: Value,
    span: Option<&Span>,
) -> Result<(), SimplyError> {
    if let (Value::Hash(values), Value::String(key)) = (&mut *target, &index) {
        return values
            .with_mut(|values| values.insert(key.clone(), value))
            .map(|_| ())
            .ok_or_else(|| error(span, "hash storage is no longer available"));
    }
    let index = collection_index(&index, span)?;
    match target {
        Value::Array(values) | Value::List(values) => values
            .with_mut(|values| values.get_mut(index).map(|slot| *slot = value))
            .flatten()
            .ok_or_else(|| error(span, "collection index out of bounds")),
        _ => Err(error(span, "value is not a mutable collection")),
    }
}

pub(crate) fn set_index_path(
    target: &mut Value,
    indices: Vec<Value>,
    value: Value,
    span: Option<&Span>,
) -> Result<(), SimplyError> {
    let Some((index, remaining)) = indices.split_first() else {
        return Err(error(span, "missing collection index"));
    };
    if remaining.is_empty() {
        return set_index(target, index.clone(), value, span);
    }

    let nested = match (&*target, index) {
        (Value::Hash(values), Value::String(key)) => values
            .with(|values| values.get(key).cloned())
            .flatten()
            .ok_or_else(|| error(span, format!("unknown key `{key}`")))?,
        (Value::Hash(_), _) => return Err(error(span, "hash key must be a string")),
        (Value::Array(values) | Value::List(values), index) => {
            let index = collection_index(index, span)?;
            values
                .with(|values| values.get(index).cloned())
                .flatten()
                .ok_or_else(|| error(span, "collection index out of bounds"))?
        }
        _ => return Err(error(span, "value is not a mutable collection")),
    };
    let mut nested = nested;
    set_index_path(&mut nested, remaining.to_vec(), value, span)
}

pub(crate) fn mutate_list(
    target: &mut Value,
    operation: &CollectionOperation,
    value: Value,
    name: &str,
    span: Option<&Span>,
) -> Result<(), SimplyError> {
    match target {
        Value::List(values) => match operation {
            CollectionOperation::Add => values
                .with_mut(|values| values.push(value))
                .map(|()| ())
                .ok_or_else(|| error(span, "list storage is no longer available")),
            CollectionOperation::Remove => {
                let position = values
                    .get_cloned()
                    .and_then(|values| values.iter().position(|item| item == &value));
                if let Some(position) = position {
                    values
                        .with_mut(|values| {
                            if position < values.len() {
                                values.remove(position);
                            }
                        })
                        .ok_or_else(|| error(span, "list storage is no longer available"))?;
                }
                Ok(())
            }
        },
        _ => Err(SimplyError::Runtime {
            span: span.cloned().unwrap_or_else(|| Span::new(0, 0)),
            code: crate::error::DiagnosticCode::RuntimeCollection,
            message: format!("`{name}` is not a list"),
        }),
    }
}

fn coordinate(value: &Value, span: Option<&Span>) -> Result<usize, SimplyError> {
    match value {
        Value::Int(value) if *value >= 0 => {
            usize::try_from(*value).map_err(|_| error(span, "matrix index is out of bounds"))
        }
        Value::Int(_) => Err(error(span, "matrix index must be non-negative")),
        _ => Err(error(span, "matrix index must be integer")),
    }
}

fn collection_index(value: &Value, span: Option<&Span>) -> Result<usize, SimplyError> {
    match value {
        Value::Int(value) if *value >= 0 => {
            usize::try_from(*value).map_err(|_| error(span, "collection index out of bounds"))
        }
        _ => Err(error(
            span,
            "collection index must be a non-negative integer",
        )),
    }
}

fn error(span: Option<&Span>, message: impl Into<String>) -> SimplyError {
    SimplyError::Runtime {
        span: span.cloned().unwrap_or_else(|| Span::new(0, 0)),
        code: crate::error::DiagnosticCode::RuntimeCollection,
        message: message.into(),
    }
}
