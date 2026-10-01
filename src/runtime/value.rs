//! runtime/value.rs — runtime value representation
//! Defines Simply values and their display/equality behavior at evaluation time.
//! Key component: Value covers primitives, collections, functions, and unit results.
use std::{
    cell::RefCell,
    collections::{BTreeMap, HashMap},
    fmt::Write,
    time::SystemTime,
};
use std::{rc::Rc, sync::Arc};

use crate::{
    ast::Stmt,
    types::{DeclarationIdentity, Type},
};

const RANGE_DISPLAY_LIMIT: i64 = 100;

#[derive(Debug, Clone)]
pub(crate) struct FunctionValue {
    pub name: Option<String>,
    pub parameters: Vec<(String, Option<Type>, bool)>,
    pub return_type: Option<Type>,
    pub body: Arc<[Stmt]>,
    pub captures: RefCell<HashMap<String, Value>>,
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
pub(crate) struct SourceContext {
    pub filename: String,
    pub source: Rc<str>,
}

#[derive(Debug, Clone)]
pub struct StructInstance {
    pub identity: DeclarationIdentity,
    pub type_name: String,
    pub fields: Rc<RefCell<BTreeMap<String, Value>>>,
}

impl PartialEq for StructInstance {
    fn eq(&self, other: &Self) -> bool {
        self.identity == other.identity
            && self.fields.borrow().iter().eq(other.fields.borrow().iter())
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

#[derive(Debug, Clone, PartialEq)]
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
    Array(Rc<Vec<Value>>),
    List(Rc<Vec<Value>>),
    Tuple(Rc<Vec<Value>>),
    Hash(Rc<BTreeMap<String, Value>>),
    Tree(Rc<BTreeMap<String, Value>>),
    Matrix(Rc<Vec<Value>>),
    Function(Rc<FunctionValue>),
    Struct(Rc<StructInstance>),
    Enum(Rc<EnumValue>),
}

pub(crate) fn shared_values(values: Vec<Value>) -> Rc<Vec<Value>> {
    Rc::new(values)
}

pub(crate) fn owned_values(values: Rc<Vec<Value>>) -> Vec<Value> {
    Rc::try_unwrap(values).unwrap_or_else(|values| (*values).clone())
}

pub(crate) fn owned_map_values(values: Rc<BTreeMap<String, Value>>) -> Vec<Value> {
    match Rc::try_unwrap(values) {
        Ok(values) => values.into_values().collect(),
        Err(values) => values.values().cloned().collect(),
    }
}

pub(crate) fn shared_map(values: BTreeMap<String, Value>) -> Rc<BTreeMap<String, Value>> {
    Rc::new(values)
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
            Self::Struct(instance) => output.push_str(&instance.type_name),
            Self::Enum(value) => {
                write!(output, "{}::{}", value.enum_name, value.variant_name)
                    .expect("writing to String cannot fail");
                if let Some(payload) = &value.payload {
                    output.push('(');
                    payload.write_display(output);
                    output.push(')');
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

#[cfg(test)]
mod tests {
    use super::{Value, shared_values};

    #[test]
    fn displays_nested_values_without_evaluator() {
        let value = Value::List(shared_values(vec![
            Value::Int(1),
            Value::String("two".into()),
        ]));
        assert_eq!(value.display(), "[1, two]");
    }
}
