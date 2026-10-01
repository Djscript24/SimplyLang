//! runtime/value.rs — runtime value representation
//! Defines Simply values and their display/equality behavior at evaluation time.
//! Key component: Value covers primitives, collections, functions, and unit results.
use std::sync::Arc;
use std::{
    collections::{BTreeMap, HashMap},
    fmt::Write,
    time::SystemTime,
};

use crate::{
    ast::Stmt,
    runtime::{
        arena::{ArenaRef, Handle},
        storage::{SharedMap, SharedVec},
    },
    types::{DeclarationIdentity, Type},
};

const RANGE_DISPLAY_LIMIT: i64 = 100;

#[derive(Debug, Clone)]
pub(crate) struct FunctionValue {
    pub name: Option<String>,
    pub parameters: Vec<(String, Option<Type>, bool)>,
    pub return_type: Option<Type>,
    pub body: Arc<[Stmt]>,
    pub captures: HashMap<String, Value>,
    pub source: Option<SourceContext>,
}

impl PartialEq for FunctionValue {
    fn eq(&self, other: &Self) -> bool {
        self.parameters == other.parameters
            && self.return_type == other.return_type
            && self.body == other.body
            && self.captures == other.captures
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SourceText {
    pub text: String,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SourceContext {
    pub filename: String,
    pub source: ArenaRef<SourceText>,
}

#[derive(Debug, Clone)]
pub struct StructInstance {
    pub identity: DeclarationIdentity,
    pub type_name: String,
    pub fields: BTreeMap<String, Value>,
}

impl PartialEq for StructInstance {
    fn eq(&self, other: &Self) -> bool {
        self.identity == other.identity && self.fields == other.fields
    }
}

#[derive(Debug, Clone)]
pub struct EnumValue {
    pub identity: DeclarationIdentity,
    pub enum_name: String,
    pub variant_name: String,
    pub payload: Option<Box<Value>>,
}

impl PartialEq for EnumValue {
    fn eq(&self, other: &Self) -> bool {
        self.identity == other.identity
            && self.variant_name == other.variant_name
            && self.payload == other.payload
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CsvStreamVersion {
    pub length: u64,
    pub modified: SystemTime,
}

#[derive(Debug, Clone)]
pub enum Value {
    Unit,
    String(String),
    Int(i64),
    Float(f64),
    Bool(bool),
    Range {
        start: i64,
        end: i64,
        step: i64,
    },
    CsvStream {
        path: String,
        start_record: usize,
        start_offset: u64,
        source_version: Option<CsvStreamVersion>,
    },
    Array(SharedVec<Value>),
    List(SharedVec<Value>),
    Tuple(SharedVec<Value>),
    Hash(SharedMap<String, Value>),
    Tree(SharedMap<String, Value>),
    Matrix(SharedVec<Value>),
    Function(Handle<FunctionValue>),
    Struct(ArenaRef<StructInstance>),
    Enum(ArenaRef<EnumValue>),
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Unit, Self::Unit) => true,
            (Self::String(left), Self::String(right)) => left == right,
            (Self::Int(left), Self::Int(right)) => left == right,
            (Self::Float(left), Self::Float(right)) => left == right,
            (Self::Bool(left), Self::Bool(right)) => left == right,
            (
                Self::Range {
                    start: left_start,
                    end: left_end,
                    step: left_step,
                },
                Self::Range {
                    start: right_start,
                    end: right_end,
                    step: right_step,
                },
            ) => left_start == right_start && left_end == right_end && left_step == right_step,
            (
                Self::CsvStream {
                    path: left_path,
                    start_record: left_record,
                    start_offset: left_offset,
                    source_version: left_version,
                },
                Self::CsvStream {
                    path: right_path,
                    start_record: right_record,
                    start_offset: right_offset,
                    source_version: right_version,
                },
            ) => {
                left_path == right_path
                    && left_record == right_record
                    && left_offset == right_offset
                    && left_version == right_version
            }
            (Self::Array(left), Self::Array(right))
            | (Self::List(left), Self::List(right))
            | (Self::Tuple(left), Self::Tuple(right))
            | (Self::Matrix(left), Self::Matrix(right)) => left == right,
            (Self::Hash(left), Self::Hash(right)) | (Self::Tree(left), Self::Tree(right)) => {
                left == right
            }
            (Self::Function(left), Self::Function(right)) => left == right,
            (Self::Struct(left), Self::Struct(right)) => left.same_instance(right),
            (Self::Enum(left), Self::Enum(right)) => left
                .get_cloned()
                .zip(right.get_cloned())
                .is_some_and(|(left, right)| left == right),
            _ => false,
        }
    }
}

pub(crate) fn shared_values(values: Vec<Value>) -> SharedVec<Value> {
    SharedVec::new(values)
}

pub(crate) fn owned_values(values: SharedVec<Value>) -> Vec<Value> {
    values.into_owned()
}

pub(crate) fn owned_map_values(values: SharedMap<String, Value>) -> Vec<Value> {
    values.into_owned().into_values().collect()
}

pub(crate) fn shared_map(values: BTreeMap<String, Value>) -> SharedMap<String, Value> {
    SharedMap::new(values)
}

impl Value {
    pub(crate) fn display(&self) -> String {
        let mut output = String::new();
        self.write_display(&mut output);
        output
    }

    fn write_display(&self, output: &mut String) {
        match self {
            Self::Unit => {}
            Self::String(value) => output.push_str(value),
            Self::Int(value) => write!(output, "{value}").expect("writing to String cannot fail"),
            Self::Float(value) => write!(output, "{value}").expect("writing to String cannot fail"),
            Self::Bool(value) => write!(output, "{value}").expect("writing to String cannot fail"),
            Self::Range { start, end, step } => {
                let length = Self::range_len(*start, *end, *step).unwrap_or(i64::MAX);
                if length > RANGE_DISPLAY_LIMIT {
                    if *step == 1 {
                        write!(output, "Range({start}..{end})")
                            .expect("writing to String cannot fail");
                    } else {
                        write!(output, "Range({start}..{end} by {step})")
                            .expect("writing to String cannot fail");
                    }
                    return;
                }
                output.push('[');
                for (index, value) in Self::range_values(*start, *end, *step).enumerate() {
                    if index > 0 {
                        output.push_str(", ");
                    }
                    value.write_display(output);
                }
                output.push(']');
            }
            Self::CsvStream { path, .. } => {
                write!(output, "<csv stream: {path}>").expect("writing to String cannot fail")
            }
            Self::Array(values)
            | Self::List(values)
            | Self::Tuple(values)
            | Self::Matrix(values) => {
                output.push('[');
                for (index, value) in values.iter().enumerate() {
                    if index > 0 {
                        output.push_str(", ");
                    }
                    value.write_display(output);
                }
                output.push(']');
            }
            Self::Hash(values) | Self::Tree(values) => {
                if matches!(self, Self::Tree(_)) {
                    output.push_str("tree ");
                }
                output.push('{');
                for (index, (key, value)) in values.iter().enumerate() {
                    if index > 0 {
                        output.push_str(", ");
                    }
                    write!(output, "{key}: ").expect("writing to String cannot fail");
                    value.write_display(output);
                }
                output.push('}');
            }
            Self::Function(_) => output.push_str("<function>"),
            Self::Struct(instance) => {
                if instance
                    .with(|instance| output.push_str(&instance.type_name))
                    .is_none()
                {
                    output.push_str("<invalid struct>");
                }
            }
            Self::Enum(value) => {
                if instance_display(value, output).is_none() {
                    output.push_str("<invalid enum>");
                }
            }
        }
    }

    pub(crate) fn range_values(start: i64, end: i64, step: i64) -> impl Iterator<Item = Value> {
        let end = i128::from(end);
        let step = i128::from(step);
        let mut current = Some(i128::from(start));
        std::iter::from_fn(move || {
            let value = current?;
            if step == 0 || !((step > 0 && value < end) || (step < 0 && value > end)) {
                current = None;
                return None;
            }

            current = Some(value + step);
            i64::try_from(value).ok().map(Value::Int)
        })
    }

    pub(crate) fn range_len(start: i64, end: i64, step: i64) -> Option<i64> {
        if step == 0 {
            return None;
        }
        let distance = if step > 0 {
            if start >= end {
                return Some(0);
            }
            i128::from(end) - i128::from(start)
        } else {
            if start <= end {
                return Some(0);
            }
            i128::from(start) - i128::from(end)
        };
        let magnitude = i128::from(step).abs();
        let length = (distance + magnitude - 1) / magnitude;
        i64::try_from(length).ok()
    }
}

fn instance_display(value: &ArenaRef<EnumValue>, output: &mut String) -> Option<()> {
    value.with(|value| {
        write!(output, "{}::{}", value.enum_name, value.variant_name)
            .expect("writing to String cannot fail");
        if let Some(payload) = &value.payload {
            output.push('(');
            payload.write_display(output);
            output.push(')');
        }
    })
}

#[cfg(test)]
#[path = "../../tests/internal/runtime_value.rs"]
mod tests;
