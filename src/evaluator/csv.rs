use std::io::{BufRead, Write};

use crate::runtime::value::Value;

pub(super) fn parse_record(line: &str) -> Result<Vec<Value>, String> {
    #[derive(Clone, Copy)]
    enum FieldState {
        Start,
        Unquoted,
        Quoted,
        AfterQuote,
    }

    let mut fields = Vec::new();
    let mut field = String::new();
    let mut state = FieldState::Start;
    let mut chars = line.chars().peekable();
    while let Some(character) = chars.next() {
        match state {
            FieldState::Start => match character {
                '"' => state = FieldState::Quoted,
                ',' => fields.push(Value::String(String::new())),
                _ => {
                    field.push(character);
                    state = FieldState::Unquoted;
                }
            },
            FieldState::Unquoted => match character {
                '"' => return Err("quote inside an unquoted field".into()),
                ',' => {
                    fields.push(Value::String(std::mem::take(&mut field)));
                    state = FieldState::Start;
                }
                _ => field.push(character),
            },
            FieldState::Quoted => match character {
                '"' if chars.peek() == Some(&'"') => {
                    field.push('"');
                    chars.next();
                }
                '"' => state = FieldState::AfterQuote,
                _ => field.push(character),
            },
            FieldState::AfterQuote => match character {
                ',' => {
                    fields.push(Value::String(std::mem::take(&mut field)));
                    state = FieldState::Start;
                }
                _ => return Err("unexpected character after a quoted field".into()),
            },
        }
    }
    if matches!(state, FieldState::Quoted) {
        return Err("unterminated quoted field".into());
    }
    fields.push(Value::String(field));
    Ok(fields)
}

pub(super) fn read_record<R: BufRead>(reader: &mut R) -> Result<Option<String>, std::io::Error> {
    let mut record = String::new();
    let mut line = String::new();
    let mut quoted = false;
    let mut at_field_start = true;

    loop {
        line.clear();
        if reader.read_line(&mut line)? == 0 {
            return Ok((!record.is_empty()).then_some(record));
        }
        record.push_str(&line);
        let mut characters = line.chars().peekable();
        while let Some(character) = characters.next() {
            if quoted {
                if character == '"' {
                    if characters.peek() == Some(&'"') {
                        characters.next();
                    } else {
                        quoted = false;
                    }
                }
            } else if character == '"' && at_field_start {
                quoted = true;
                at_field_start = false;
            } else {
                at_field_start = character == ',';
            }
        }
        if !quoted {
            return Ok(Some(record));
        }
        at_field_start = true;
    }
}

fn field(value: &Value) -> Result<String, String> {
    match value {
        Value::String(value) => {
            if value.contains([',', '"', '\n', '\r']) {
                Ok(format!("\"{}\"", value.replace('"', "\"\"")))
            } else {
                Ok(value.clone())
            }
        }
        Value::Int(value) => Ok(value.to_string()),
        Value::Float(value) if value.is_finite() => Ok(value.to_string()),
        Value::Bool(value) => Ok(value.to_string()),
        Value::Unit => Ok(String::new()),
        _ => Err("write_csv supports strings, numbers, booleans, and Unit fields".into()),
    }
}

pub(super) fn write_row(output: &mut impl Write, values: &[Value]) -> Result<(), String> {
    let mut fields = Vec::with_capacity(values.len());
    for value in values {
        fields.push(field(value)?);
    }
    writeln!(output, "{}", fields.join(","))
        .map_err(|error| format!("could not write CSV: {error}"))
}
