// Internal unit tests for src/runtime/value.rs.
use std::{collections::HashMap, sync::Arc};

use super::{EnumValue, FunctionValue, Value, shared_values};
use crate::{
    runtime::{
        arena::{Arena, ArenaRef},
        storage::SharedCell,
    },
    types::{DeclarationIdentity, DeclarationKind},
};

#[test]
fn displays_nested_values_without_evaluator() {
    let value = Value::List(shared_values(vec![
        Value::Int(1),
        Value::String("two".into()),
    ]));
    assert_eq!(value.display(), "[1, two]");
}

#[test]
fn enum_equality_does_not_depend_on_evaluator_arena_root() {
    let identity = DeclarationIdentity::new("module.si", "Result", DeclarationKind::Enum);
    let left_root = SharedCell::new(Arena::new());
    let right_root = SharedCell::new(Arena::new());
    let left = ArenaRef::insert(
        left_root.clone(),
        EnumValue {
            identity: identity.clone(),
            enum_name: "Result".into(),
            variant_name: "Ok".into(),
            payload: Some(Box::new(Value::Int(42))),
        },
    );
    let right = ArenaRef::insert(
        right_root.clone(),
        EnumValue {
            identity,
            enum_name: "Result".into(),
            variant_name: "Ok".into(),
            payload: Some(Box::new(Value::Int(42))),
        },
    );

    assert_eq!(Value::Enum(left), Value::Enum(right));
}

#[test]
fn function_equality_preserves_callable_handle_identity() {
    let function = |name: &str| FunctionValue {
        name: Some(name.into()),
        parameters: Vec::new(),
        return_type: None,
        body: Arc::from([]),
        captures: HashMap::new(),
        source: None,
    };
    let mut arena = Arena::new();
    let first = arena.insert(function("first"));
    let second = arena.insert(function("second"));

    assert_eq!(Value::Function(first), Value::Function(first));
    assert_ne!(Value::Function(first), Value::Function(second));
}
