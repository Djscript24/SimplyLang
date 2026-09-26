//! runtime/value.rs — runtime value representation
//! Defines Simply values and their display/equality behavior at evaluation time.
//! Key component: Value covers primitives, collections, functions, and unit results.
use std::{
    cell::RefCell,
    collections::{BTreeMap, HashMap},
    fmt::Write,
};
use std::{rc::Rc, sync::Arc};

use crate::{ast::Stmt, types::Type};

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct FunctionValue {
    pub parameters: Vec<(String, Option<Type>, bool)>,
    pub return_type: Option<Type>,
    pub body: Arc<[Stmt]>,
    pub captures: RefCell<HashMap<String, Value>>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Unit,
    String(String),
    Int(i64),
    Float(f64),
    Bool(bool),
    Range { start: i64, end: i64 },
    CsvStream(String),
    Array(Rc<Vec<Value>>),
    List(Rc<Vec<Value>>),
    Tuple(Rc<Vec<Value>>),
    Hash(Rc<BTreeMap<String, Value>>),
    Tree(Rc<BTreeMap<String, Value>>),
    Matrix(Rc<Vec<Value>>),
    Function(Rc<FunctionValue>),
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
            Self::Range { start, end } => {
                output.push('[');
                for (index, value) in (*start..*end).map(Value::Int).enumerate() {
                    if index > 0 {
                        output.push_str(", ");
                    }
                    value.write_display(output);
                }
                output.push(']');
            }
            Self::CsvStream(path) => {
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
        }
    }

    pub(crate) fn range_values(start: i64, end: i64) -> impl Iterator<Item = Value> {
        (start..end).map(Value::Int)
    }

    pub(crate) fn range_len(start: i64, end: i64) -> i64 {
        end.saturating_sub(start).max(0)
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
