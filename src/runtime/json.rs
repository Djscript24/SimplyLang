//! Conversion between Simply values and JSON.

use std::collections::BTreeMap;

use serde_json::{Map, Number, Value as JsonValue};

use super::{heap::RuntimeHeap, value::Value};

const MAX_JSON_DEPTH: usize = 128;

pub(crate) fn parse(heap: &RuntimeHeap, source: &str) -> Result<Value, String> {
    let value: JsonValue = serde_json::from_str(source).map_err(|error| error.to_string())?;
    from_json(heap, value, 0)
}

pub(crate) fn parse_lines(heap: &RuntimeHeap, source: &str) -> Result<Value, String> {
    let values = source
        .lines()
        .enumerate()
        .filter(|(_, line)| !line.trim().is_empty())
        .map(|(index, line)| {
            parse(heap, line)
                .map_err(|error| format!("invalid JSON on line {}: {error}", index + 1))
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Value::Array(heap.insert_sequence(values)))
}

pub(crate) fn serialize(value: &Value) -> Result<String, String> {
    let value = to_json(value, 0)?;
    serde_json::to_string(&value).map_err(|error| error.to_string())
}

pub(crate) fn serialize_lines(value: &Value) -> Result<String, String> {
    let values = value
        .sequence_snapshot()
        .ok_or_else(|| "JSON Lines output expects an Array, List, or Tuple".to_owned())?;
    let mut output = String::new();
    for value in values.iter() {
        output.push_str(&serialize(value)?);
        output.push('\n');
    }
    Ok(output)
}

pub(crate) fn serialize_pretty(value: &Value) -> Result<String, String> {
    let value = to_json(value, 0)?;
    serde_json::to_string_pretty(&value).map_err(|error| error.to_string())
}

fn from_json(heap: &RuntimeHeap, value: JsonValue, depth: usize) -> Result<Value, String> {
    if depth > MAX_JSON_DEPTH {
        return Err(format!(
            "JSON nesting exceeds the limit of {MAX_JSON_DEPTH}"
        ));
    }
    match value {
        JsonValue::Null => Ok(Value::Unit),
        JsonValue::Bool(value) => Ok(Value::Bool(value)),
        JsonValue::Number(value) => {
            if let Some(value) = value.as_i64() {
                Ok(Value::Int(value))
            } else {
                value
                    .as_f64()
                    .filter(|value| value.is_finite())
                    .map(Value::Float)
                    .ok_or_else(|| "JSON number is outside the supported numeric range".into())
            }
        }
        JsonValue::String(value) => Ok(Value::String(value)),
        JsonValue::Array(values) => values
            .into_iter()
            .map(|value| from_json(heap, value, depth + 1))
            .collect::<Result<Vec<_>, _>>()
            .map(|values| Value::Array(heap.insert_sequence(values))),
        JsonValue::Object(values) => values
            .into_iter()
            .map(|(key, value)| Ok((key, from_json(heap, value, depth + 1)?)))
            .collect::<Result<BTreeMap<_, _>, String>>()
            .map(|values| Value::Hash(heap.insert_hash(values))),
    }
}

fn to_json(value: &Value, depth: usize) -> Result<JsonValue, String> {
    if depth > MAX_JSON_DEPTH {
        return Err(format!(
            "JSON nesting exceeds the limit of {MAX_JSON_DEPTH}"
        ));
    }
    match value {
        Value::Unit => Ok(JsonValue::Null),
        Value::Bool(value) => Ok(JsonValue::Bool(*value)),
        Value::Int(value) => Ok(JsonValue::Number(Number::from(*value))),
        Value::Float(value) => Number::from_f64(*value)
            .map(JsonValue::Number)
            .ok_or_else(|| "cannot serialize a non-finite Float to JSON".into()),
        Value::String(value) => Ok(JsonValue::String(value.clone())),
        Value::Array(_) | Value::List(_) | Value::Tuple(_) | Value::Matrix(_) => value
            .sequence_snapshot()
            .ok_or_else(|| "collection storage is no longer available".to_owned())?
            .iter()
            .map(|value| to_json(value, depth + 1))
            .collect::<Result<Vec<_>, _>>()
            .map(JsonValue::Array),
        Value::Hash(_) => value
            .hash_snapshot()
            .ok_or_else(|| "hash storage is no longer available".to_owned())?
            .iter()
            .map(|(key, value)| Ok((key.clone(), to_json(value, depth + 1)?)))
            .collect::<Result<Map<_, _>, String>>()
            .map(JsonValue::Object),
        Value::Range { .. } => {
            Err("cannot serialize a Range to JSON; convert it to a collection first".into())
        }
        Value::CsvStream { .. } => Err("cannot serialize a CsvStream to JSON".into()),
        Value::Function(_) => Err("cannot serialize a Function to JSON".into()),
        Value::Struct(_) => Err("cannot serialize a Struct to JSON".into()),
        Value::Enum(_) => Err("cannot serialize an Enum to JSON".into()),
    }
}
