use std::{
    fs,
    io::{BufReader, Seek},
    path::Path,
};

use crate::{
    ast::{Literal, MatchPattern},
    error::{DiagnosticCode, SimplyError},
    runtime::value::{CsvStreamVersion, Value},
};

use super::{Evaluator, csv};

type CsvSequencePrefix = (Vec<Value>, Option<Value>);

impl Evaluator {
    pub(super) fn match_value_pattern(
        &self,
        pattern: &MatchPattern,
        value: &Value,
    ) -> Result<Option<Vec<(String, Value)>>, SimplyError> {
        let matched = match pattern {
            MatchPattern::Wildcard => Some(Vec::new()),
            MatchPattern::Literal(literal) => {
                let literal = match literal {
                    Literal::String(value) => Value::String(value.clone()),
                    Literal::Int(value) => Value::Int(*value),
                    Literal::Float(value) => Value::Float(*value),
                    Literal::Bool(value) => Value::Bool(*value),
                };
                literal.eq(value).then(Vec::new)
            }
            MatchPattern::Range { start, end } => {
                let Value::Int(value) = value else {
                    return Ok(None);
                };
                let starts_before_or_at = match start {
                    Some(Literal::Int(start)) => *value >= *start,
                    Some(_) => return Ok(None),
                    None => true,
                };
                let ends_after_or_at = match end {
                    Some(Literal::Int(end)) => *value <= *end,
                    Some(_) => return Ok(None),
                    None => true,
                };
                (starts_before_or_at && ends_after_or_at).then(Vec::new)
            }
            MatchPattern::Or(alternatives) => {
                let mut bindings = None;
                for alternative in alternatives {
                    if let Some(matched) = self.match_value_pattern(alternative, value)? {
                        bindings = Some(matched);
                        break;
                    }
                }
                bindings
            }
            MatchPattern::Identifier(name) => Some(vec![(name.clone(), value.clone())]),
            MatchPattern::Alias { name, pattern } => {
                let Some(mut bindings) = self.match_value_pattern(pattern, value)? else {
                    return Ok(None);
                };
                bindings.push((name.clone(), value.clone()));
                Some(bindings)
            }
            MatchPattern::Tuple(patterns) => {
                let Value::Tuple(values) = value else {
                    return Ok(None);
                };
                if patterns.len() != values.len() {
                    return Ok(None);
                }
                let mut bindings = Vec::new();
                for (pattern, value) in patterns.iter().zip(values.iter()) {
                    let Some(nested) = self.match_value_pattern(pattern, &value)? else {
                        return Ok(None);
                    };
                    bindings.extend(nested);
                }
                Some(bindings)
            }
            MatchPattern::Sequence { patterns, rest } => {
                let (values, rest_value) = match value {
                    Value::Array(values) => {
                        let values = values.get_cloned().unwrap_or_default();
                        if values.len() < patterns.len()
                            || (rest.is_none() && values.len() != patterns.len())
                        {
                            return Ok(None);
                        }
                        let tail = rest
                            .as_ref()
                            .map(|_| self.make_array(values[patterns.len()..].to_vec()));
                        (values[..patterns.len()].to_vec(), tail)
                    }
                    Value::List(values) => {
                        let values = values.get_cloned().unwrap_or_default();
                        if values.len() < patterns.len()
                            || (rest.is_none() && values.len() != patterns.len())
                        {
                            return Ok(None);
                        }
                        let tail = rest
                            .as_ref()
                            .map(|_| self.make_list(values[patterns.len()..].to_vec()));
                        (values[..patterns.len()].to_vec(), tail)
                    }
                    Value::Range { start, end, step } => {
                        let length = Value::range_len(*start, *end, *step)
                            .map(i128::from)
                            .unwrap_or(i128::MAX);
                        if length < patterns.len() as i128
                            || (rest.is_none() && length != patterns.len() as i128)
                        {
                            return Ok(None);
                        }
                        let mut values = Vec::with_capacity(patterns.len());
                        for index in 0..patterns.len() {
                            let index = i128::try_from(index).ok();
                            let Some(value) = index.and_then(|index| {
                                i64::try_from(i128::from(*start) + index * i128::from(*step)).ok()
                            }) else {
                                return Ok(None);
                            };
                            values.push(Value::Int(value));
                        }
                        let tail = if rest.is_some() {
                            let rest_start = if length == 0 {
                                *start
                            } else {
                                let offset = i128::try_from(patterns.len()).map_err(|_| {
                                    self.runtime_error_with_code(
                                        DiagnosticCode::RuntimeLimit,
                                        "sequence pattern length exceeds the supported range",
                                    )
                                })?;
                                i64::try_from(i128::from(*start) + offset * i128::from(*step))
                                    .map_err(|_| {
                                        self.runtime_error_with_code(
                                            DiagnosticCode::RuntimeLimit,
                                            "range rest position exceeds the supported range",
                                        )
                                    })?
                            };
                            Some(Value::Range {
                                start: rest_start,
                                end: *end,
                                step: *step,
                            })
                        } else {
                            None
                        };
                        (values, tail)
                    }
                    Value::CsvStream {
                        path,
                        start_record,
                        start_offset,
                        source_version,
                    } => {
                        let Some((values, tail)) = self.read_csv_sequence_prefix(
                            path,
                            *start_record,
                            *start_offset,
                            source_version.as_ref(),
                            patterns.len(),
                            rest.is_some(),
                        )?
                        else {
                            return Ok(None);
                        };
                        (values, tail)
                    }
                    _ => return Ok(None),
                };
                let mut bindings = Vec::new();
                for (pattern, value) in patterns.iter().zip(values.iter()) {
                    let Some(nested) = self.match_value_pattern(pattern, &value)? else {
                        return Ok(None);
                    };
                    bindings.extend(nested);
                }
                if let (Some(name), Some(value)) = (rest, rest_value) {
                    bindings.push((name.clone(), value));
                }
                Some(bindings)
            }
            MatchPattern::Hash(patterns) => {
                let Value::Hash(values) = value else {
                    return Ok(None);
                };
                let mut bindings = Vec::new();
                for (key, pattern) in patterns {
                    let Some(value) = values.get(key) else {
                        return Ok(None);
                    };
                    let Some(nested) = self.match_value_pattern(pattern, &value)? else {
                        return Ok(None);
                    };
                    bindings.extend(nested);
                }
                Some(bindings)
            }
            MatchPattern::Struct { type_name, fields } => {
                let Value::Struct(instance) = value else {
                    return Ok(None);
                };
                let instance = self.tracked_struct(instance)?;
                let Some(definition) = self.lookup_struct(type_name) else {
                    return Ok(None);
                };
                if instance.identity != definition.identity {
                    return Ok(None);
                }
                if fields.len() != definition.fields.len() {
                    return Ok(None);
                }
                let mut bindings = Vec::new();
                for (pattern, field) in fields.iter().zip(&definition.fields) {
                    let Some(value) = instance.fields.get(&field.name) else {
                        return Ok(None);
                    };
                    let Some(nested) = self.match_value_pattern(pattern, value)? else {
                        return Ok(None);
                    };
                    bindings.extend(nested);
                }
                Some(bindings)
            }
            MatchPattern::NamedStruct { type_name, fields } => {
                let Value::Struct(instance) = value else {
                    return Ok(None);
                };
                let instance = self.tracked_struct(instance)?;
                let Some(definition) = self.lookup_struct(type_name) else {
                    return Ok(None);
                };
                if instance.identity != definition.identity {
                    return Ok(None);
                }
                let mut bindings = Vec::new();
                for (field_name, pattern) in fields {
                    if !definition
                        .fields
                        .iter()
                        .any(|field| field.name == *field_name)
                    {
                        return Ok(None);
                    }
                    let Some(value) = instance.fields.get(field_name) else {
                        return Ok(None);
                    };
                    let Some(nested) = self.match_value_pattern(pattern, value)? else {
                        return Ok(None);
                    };
                    bindings.extend(nested);
                }
                Some(bindings)
            }
            MatchPattern::EnumVariant {
                enum_name,
                variant_name,
                payload,
            } => {
                let Value::Enum(enum_value) = value else {
                    return Ok(None);
                };
                let enum_value = self.tracked_enum(enum_value)?;
                let Some(definition) = self.lookup_enum(enum_name) else {
                    return Ok(None);
                };
                if enum_value.identity != definition.identity
                    || enum_value.variant_name != *variant_name
                {
                    return Ok(None);
                }
                match (&enum_value.payload, payload) {
                    (None, None) => Some(Vec::new()),
                    (Some(value), Some(pattern)) => self.match_value_pattern(pattern, value)?,
                    _ => None,
                }
            }
        };
        Ok(matched)
    }

    pub(super) fn read_csv_sequence_prefix(
        &self,
        path: &str,
        start_record: usize,
        start_offset: u64,
        source_version: Option<&CsvStreamVersion>,
        expected_length: usize,
        has_rest: bool,
    ) -> Result<Option<CsvSequencePrefix>, SimplyError> {
        let file = fs::File::open(path).map_err(|error| self.file_error(Path::new(path), error))?;
        let current_version = Self::csv_stream_version(&file, path, self)?;
        let mut reader = BufReader::new(file);
        Self::seek_csv_stream(
            &mut reader,
            path,
            start_record,
            start_offset,
            source_version,
            &current_version,
            self,
        )?;
        let mut rows = Vec::with_capacity(expected_length);
        for _ in 0..expected_length {
            let Some(record) = csv::read_record(&mut reader)
                .map_err(|error| self.file_error(Path::new(path), error))?
            else {
                return Ok(None);
            };
            let line = record.strip_suffix('\n').unwrap_or(&record);
            let line = line.strip_suffix('\r').unwrap_or(line);
            let row = csv::parse_record(line).map_err(|message| {
                self.runtime_error(format!("invalid CSV row in `{path}`: {message}"))
            })?;
            rows.push(self.make_list(row));
        }
        let tail = if has_rest {
            let suffix_record = start_record.checked_add(expected_length).ok_or_else(|| {
                self.runtime_error_with_code(
                    DiagnosticCode::RuntimeLimit,
                    "CSV stream position exceeds the supported range",
                )
            })?;
            let suffix_offset = reader
                .stream_position()
                .map_err(|error| self.file_error(Path::new(path), error))?;
            Some(Value::CsvStream {
                path: path.to_owned(),
                start_record: suffix_record,
                start_offset: suffix_offset,
                source_version: Some(current_version),
            })
        } else {
            if csv::read_record(&mut reader)
                .map_err(|error| self.file_error(Path::new(path), error))?
                .is_some()
            {
                return Ok(None);
            }
            None
        };
        Ok(Some((rows, tail)))
    }
}
