use std::{
    collections::{BTreeMap, HashMap},
    path::PathBuf,
    sync::Arc,
};

use crate::{
    ast::{Program, Stmt},
    evaluator::Evaluator,
    lexer::Lexer,
    parser::Parser,
    runtime::{
        heap::RuntimeHeap,
        value::{
            EnumValue, FunctionValue, SourceContext, SourceText, StructInstance, Value,
            shared_values,
        },
    },
    types::{DeclarationIdentity, DeclarationKind},
};

fn function(name: &str, captures: HashMap<String, Value>) -> FunctionValue {
    FunctionValue {
        name: Some(name.into()),
        parameters: Vec::new(),
        return_type: None,
        body: Arc::<[Stmt]>::from([]),
        captures,
        source: None,
    }
}

fn structure(name: &str, fields: BTreeMap<String, Value>) -> StructInstance {
    StructInstance {
        identity: DeclarationIdentity::new("probe", name, DeclarationKind::Struct),
        type_name: name.into(),
        fields,
    }
}

fn bind(evaluator: &mut Evaluator, name: &str, value: Value) {
    evaluator
        .scopes
        .define(name.into(), value, false)
        .expect("probe fixture bindings are unique");
}

#[test]
fn reachable_global_function_is_reported_as_reachable() {
    let mut evaluator = Evaluator::new();
    let function = evaluator
        .heap
        .insert_function(function("visible", HashMap::new()));
    bind(&mut evaluator, "visible", Value::Function(function));

    let report = evaluator.probe_reachability(&[]);

    assert!(report.complete, "{:?}", report.incomplete_reasons);
    assert_eq!(report.functions.reachable, 1);
    assert_eq!(report.functions.unreachable, 0);
    assert_eq!(report.functions.unclassified, 0);
    assert_eq!(report.roots.globals, 1);
}

#[test]
fn caller_supplied_return_value_is_a_reachability_root() {
    let evaluator = Evaluator::new();
    let function = evaluator
        .heap
        .insert_function(function("returned", HashMap::new()));

    let report = evaluator.probe_reachability(&[Value::Function(function)]);

    assert!(report.complete, "{:?}", report.incomplete_reasons);
    assert_eq!(report.functions.reachable, 1);
    assert_eq!(report.roots.active_execution, 1);
}

#[test]
fn function_call_return_value_remains_a_root_after_scope_unwinds() {
    let program = Parser::new(
        Lexer::new(
            "type Payload:\n\
                 name as String\n\
             end\n\
             fn identity(value as Payload) gives Payload:\n\
                 return value\n\
             end\n\
             payload is Payload(\"return value\")\n",
        )
        .tokenize()
        .expect("fixture tokenizes"),
    )
    .parse()
    .expect("fixture parses");
    let mut evaluator = Evaluator::new();
    evaluator.run(&program).expect("fixture declarations run");
    let payload = evaluator
        .scopes
        .lookup("payload")
        .expect("fixture payload binding exists")
        .clone();
    evaluator.scopes.remove_current("payload");

    let returned = evaluator
        .invoke_function("identity", vec![payload])
        .expect("function returns its argument");
    let report = evaluator.probe_reachability(std::slice::from_ref(&returned));

    assert!(report.complete, "{:?}", report.incomplete_reasons);
    assert_eq!(report.structs.reachable, 1);
    assert_eq!(report.roots.active_execution, 1);
}

#[test]
fn function_argument_is_visible_in_the_live_parameter_scope() {
    let program = Parser::new(
        Lexer::new(
            "type Payload:\n\
                 name as String\n\
             end\n\
             fn identity(value as Payload) gives Payload:\n\
                 return value\n\
             end\n\
             payload is Payload(\"argument\")\n",
        )
        .tokenize()
        .expect("fixture tokenizes"),
    )
    .parse()
    .expect("fixture parses");
    let mut evaluator = Evaluator::new();
    evaluator.run(&program).expect("fixture runs");
    let argument = evaluator
        .scopes
        .lookup("payload")
        .expect("payload binding exists")
        .clone();
    evaluator.scopes.remove_current("payload");

    evaluator
        .invoke_function("identity", vec![argument])
        .expect("function call succeeds");

    let (checkpoint, report) = evaluator
        .reachability_observations
        .iter()
        .find(|(checkpoint, _)| {
            *checkpoint == super::ReachabilityCheckpoint::FunctionArgumentsBound
        })
        .expect("function invocation records its bound arguments");
    assert_eq!(
        *checkpoint,
        super::ReachabilityCheckpoint::FunctionArgumentsBound
    );
    assert_eq!(report.structs.reachable, 1);
    assert!(!report.complete);
    assert_eq!(report.structs.unreachable, 0);
}

#[test]
fn closure_capture_is_visible_in_the_live_invocation_scope() {
    let mut evaluator = Evaluator::new();
    let captured = evaluator
        .heap
        .insert_struct(structure("CapturedDuringCall", BTreeMap::new()));
    let closure = evaluator.heap.insert_function(function(
        "closure",
        HashMap::from([("captured".into(), Value::Struct(captured))]),
    ));

    evaluator
        .invoke_function_value("closure", closure, Vec::new())
        .expect("closure invocation succeeds");

    let (_, report) = evaluator
        .reachability_observations
        .first()
        .expect("closure invocation records its active scope");
    assert_eq!(report.structs.reachable, 1);
    assert!(!report.complete);
}

#[test]
fn active_message_receiver_is_enumerated_during_dispatch() {
    let mut evaluator = Evaluator::new();
    let receiver = evaluator
        .heap
        .insert_struct(structure("TemporaryReceiver", BTreeMap::new()));
    let behavior = evaluator
        .heap
        .insert_function(function("message", HashMap::new()));
    evaluator
        .invoke_message_value("message", behavior, Vec::new(), receiver)
        .expect("message call succeeds");

    let (_, report) = evaluator
        .reachability_observations
        .iter()
        .find(|(checkpoint, _)| *checkpoint == super::ReachabilityCheckpoint::MessageReceiverActive)
        .expect("message invocation records its active receiver");
    assert_eq!(report.structs.reachable, 1);
    assert!(!report.complete);
}

#[test]
fn pipeline_item_is_enumerated_while_a_transform_function_runs() {
    let program = Parser::new(
        Lexer::new(
            "type Payload:\n\
                 name as String\n\
             end\n\
             fn inspect(value as Payload) gives Int:\n\
                 return 1\n\
             end\n\
             result is pipeline:\n\
                 list [Payload(\"pipeline item\")]\n\
                 derive inspect(item)\n\
                 sum\n\
             end\n",
        )
        .tokenize()
        .expect("fixture tokenizes"),
    )
    .parse()
    .expect("fixture parses");
    let mut evaluator = Evaluator::new();

    evaluator.run(&program).expect("pipeline executes");

    let (_, report) = evaluator
        .reachability_observations
        .iter()
        .find(|(checkpoint, _)| {
            *checkpoint == super::ReachabilityCheckpoint::FunctionArgumentsBound
        })
        .expect("transform invocation records its argument scope");
    assert_eq!(report.structs.reachable, 1);
    assert!(!report.complete);
}

#[test]
fn uncaught_thrown_value_is_a_root_while_the_error_is_held_by_the_caller() {
    let program = Parser::new(
        Lexer::new(
            "type Payload:\n\
                 name as String\n\
             end\n\
             enum Failure:\n\
                 Failed as Payload\n\
             end\n\
             payload is Payload(\"kept by thrown value\")\n\
             throw Failure::Failed(payload)\n",
        )
        .tokenize()
        .expect("fixture tokenizes"),
    )
    .parse()
    .expect("fixture parses");
    let mut evaluator = Evaluator::new();
    let error = evaluator
        .run(&program)
        .expect_err("fixture throws an enum value");
    let thrown = error
        .thrown_value()
        .expect("runtime error preserves thrown enum")
        .clone();
    evaluator.scopes.remove_current("payload");

    let report = evaluator.probe_reachability(&[thrown]);

    assert!(report.complete, "{:?}", report.incomplete_reasons);
    assert_eq!(report.enums.reachable, 1);
    assert_eq!(report.structs.reachable, 1);
}

#[test]
fn unreachable_function_is_distinguished_from_physically_retained_entry() {
    let evaluator = Evaluator::new();
    let function = evaluator
        .heap
        .insert_function(function("temporary", HashMap::new()));
    assert!(evaluator.heap.function(function).is_some());

    let report = evaluator.probe_reachability(&[]);

    assert!(report.complete, "{:?}", report.incomplete_reasons);
    assert_eq!(report.functions.reachable, 0);
    assert_eq!(report.functions.unreachable, 1);
    assert_eq!(report.functions.unclassified, 0);
    assert!(evaluator.heap.function(function).is_some());
}

#[test]
fn unreachable_entries_remain_physical_but_are_unreachable_in_all_arenas() {
    let evaluator = Evaluator::new();
    let function = evaluator
        .heap
        .insert_function(function("temporary", HashMap::new()));
    {
        let _structure = evaluator
            .heap
            .insert_struct(structure("Temporary", BTreeMap::new()));
        let _enumeration = evaluator.heap.insert_enum(EnumValue {
            identity: DeclarationIdentity::new("probe", "Temporary", DeclarationKind::Enum),
            enum_name: "Temporary".into(),
            variant_name: "Unused".into(),
            payload: None,
        });
        let _source = evaluator.heap.insert_source(SourceText {
            text: "unused".into(),
        });
    }

    assert!(evaluator.heap.function(function).is_some());
    let report = evaluator.probe_reachability(&[]);

    assert!(report.complete, "{:?}", report.incomplete_reasons);
    assert_eq!(report.functions.unreachable, 1);
    assert_eq!(report.structs.unreachable, 1);
    assert_eq!(report.enums.unreachable, 1);
    assert_eq!(report.sources.unreachable, 1);
}

#[test]
fn reachable_struct_is_reported_from_a_global_binding() {
    let mut evaluator = Evaluator::new();
    let structure = evaluator
        .heap
        .insert_struct(structure("Person", BTreeMap::new()));
    bind(&mut evaluator, "person", Value::Struct(structure));

    let report = evaluator.probe_reachability(&[]);

    assert!(report.complete, "{:?}", report.incomplete_reasons);
    assert_eq!(report.structs.reachable, 1);
    assert_eq!(report.structs.unreachable, 0);
}

#[test]
fn struct_collection_cycle_terminates_and_is_detected() {
    let mut evaluator = Evaluator::new();
    let node = evaluator
        .heap
        .insert_struct(structure("Node", BTreeMap::new()));
    node.with_mut(|node_value| {
        node_value.fields.insert(
            "links".into(),
            Value::List(shared_values(vec![Value::Struct(node.clone())])),
        );
    })
    .expect("new struct handle is valid");
    bind(&mut evaluator, "node", Value::Struct(node));

    let report = evaluator.probe_reachability(&[]);

    assert!(report.complete, "{:?}", report.incomplete_reasons);
    assert_eq!(report.structs.reachable, 1);
    assert_eq!(report.cycles_detected, 1);
}

#[test]
fn reachable_closure_traverses_its_captured_struct() {
    let mut evaluator = Evaluator::new();
    let captured = evaluator
        .heap
        .insert_struct(structure("Captured", BTreeMap::new()));
    let closure = evaluator.heap.insert_function(function(
        "closure",
        HashMap::from([("captured".into(), Value::Struct(captured))]),
    ));
    bind(&mut evaluator, "closure", Value::Function(closure));

    let report = evaluator.probe_reachability(&[]);

    assert!(report.complete, "{:?}", report.incomplete_reasons);
    assert_eq!(report.functions.reachable, 1);
    assert_eq!(report.structs.reachable, 1);
}

#[test]
fn closure_struct_cycle_terminates_and_is_detected() {
    let mut evaluator = Evaluator::new();
    let captured = evaluator
        .heap
        .insert_struct(structure("ClosureCycle", BTreeMap::new()));
    let closure = evaluator.heap.insert_function(function(
        "cycle",
        HashMap::from([("captured".into(), Value::Struct(captured.clone()))]),
    ));
    captured
        .with_mut(|instance| {
            instance
                .fields
                .insert("callback".into(), Value::Function(closure));
        })
        .expect("new struct handle is valid");
    bind(&mut evaluator, "cycle", Value::Function(closure));

    let report = evaluator.probe_reachability(&[]);

    assert!(report.complete, "{:?}", report.incomplete_reasons);
    assert_eq!(report.functions.reachable, 1);
    assert_eq!(report.structs.reachable, 1);
    assert_eq!(report.cycles_detected, 1);
}

#[test]
fn nested_cow_collections_traverse_heap_backed_values() {
    let mut evaluator = Evaluator::new();
    let nested = evaluator
        .heap
        .insert_struct(structure("Nested", BTreeMap::new()));
    bind(
        &mut evaluator,
        "values",
        Value::List(shared_values(vec![Value::Tuple(shared_values(vec![
            Value::Struct(nested),
        ]))])),
    );

    let report = evaluator.probe_reachability(&[]);

    assert!(report.complete, "{:?}", report.incomplete_reasons);
    assert_eq!(report.structs.reachable, 1);
    assert!(report.values_visited >= 3);
}

#[test]
fn reachable_enum_traverses_heap_backed_payload() {
    let mut evaluator = Evaluator::new();
    let payload = evaluator
        .heap
        .insert_struct(structure("Payload", BTreeMap::new()));
    let enumeration = evaluator.heap.insert_enum(EnumValue {
        identity: DeclarationIdentity::new("probe", "Result", DeclarationKind::Enum),
        enum_name: "Result".into(),
        variant_name: "Ok".into(),
        payload: Some(Box::new(Value::Struct(payload))),
    });
    bind(&mut evaluator, "result", Value::Enum(enumeration));

    let report = evaluator.probe_reachability(&[]);

    assert!(report.complete, "{:?}", report.incomplete_reasons);
    assert_eq!(report.enums.reachable, 1);
    assert_eq!(report.structs.reachable, 1);
}

#[test]
fn source_roots_include_current_source_function_metadata_and_import_cache() {
    let mut evaluator = Evaluator::new();
    let current = evaluator.heap.insert_source(SourceText {
        text: "current".into(),
    });
    evaluator.current_source = Some(current.clone());
    let imported = evaluator.heap.insert_source(SourceText {
        text: "cached import".into(),
    });
    evaluator.import_cache.borrow_mut().insert(
        PathBuf::from("/probe/import.si"),
        (
            Arc::new(Program {
                statements: Vec::new(),
            }),
            imported.clone(),
        ),
    );
    let metadata_source = evaluator.heap.insert_source(SourceText {
        text: "function source".into(),
    });
    let function_metadata = evaluator.function_arena.insert(super::Function {
        parameters: Vec::new(),
        return_type: None,
        body: Arc::<[Stmt]>::from([]),
        source: Some(SourceContext {
            filename: "metadata.si".into(),
            source: metadata_source.clone(),
        }),
    });
    evaluator
        .function_scopes
        .last_mut()
        .expect("global function scope exists")
        .insert("metadata".into(), function_metadata);

    let report = evaluator.probe_reachability(&[]);

    assert!(report.complete, "{:?}", report.incomplete_reasons);
    assert_eq!(report.sources.reachable, 3);
    assert_eq!(report.sources.unreachable, 0);
    assert_eq!(report.roots.imports, 1);
}

#[test]
fn message_definition_is_a_function_root_without_an_ordinary_binding() {
    let mut evaluator = Evaluator::new();
    let behavior = evaluator
        .heap
        .insert_function(function("message", HashMap::new()));
    let identity = DeclarationIdentity::new("probe", "Receiver", DeclarationKind::Struct);
    evaluator.message_scopes[0].insert(
        (identity, "visit".into()),
        super::MessageBehavior {
            function: behavior,
            field_count: 0,
        },
    );

    let report = evaluator.probe_reachability(&[]);

    assert!(report.complete, "{:?}", report.incomplete_reasons);
    assert_eq!(report.functions.reachable, 1);
    assert_eq!(report.roots.evaluator, 2);
}

#[test]
fn dead_arena_ref_does_not_make_an_entry_reachable() {
    let foreign_heap = RuntimeHeap::default();
    let dead_ref = foreign_heap.insert_struct(structure("Dead", BTreeMap::new()));
    drop(foreign_heap);

    let evaluator = Evaluator::new();
    let report = evaluator.probe_reachability(&[Value::Struct(dead_ref)]);

    assert!(report.complete, "{:?}", report.incomplete_reasons);
    assert_eq!(report.structs.reachable, 0);
    assert_eq!(report.stale_weak_references, 1);
}

#[test]
fn live_reference_from_another_heap_is_not_followed() {
    let foreign_heap = RuntimeHeap::default();
    let foreign_ref = foreign_heap.insert_struct(structure("Foreign", BTreeMap::new()));

    let evaluator = Evaluator::new();
    evaluator
        .heap
        .insert_struct(structure("UnseenLocal", BTreeMap::new()));
    let report = evaluator.probe_reachability(&[Value::Struct(foreign_ref)]);

    assert!(!report.complete);
    assert_eq!(report.structs.reachable, 0);
    assert_eq!(report.structs.unreachable, 0);
    assert_eq!(report.structs.unclassified, 1);
    assert_eq!(report.foreign_weak_references, 1);
}

#[test]
fn active_execution_marks_report_incomplete_instead_of_false_unreachable() {
    let mut evaluator = Evaluator::new();
    evaluator.scopes.push();
    evaluator
        .heap
        .insert_struct(structure("Unknown", BTreeMap::new()));

    let report = evaluator.probe_reachability(&[]);

    assert!(!report.complete);
    assert_eq!(report.structs.reachable, 0);
    assert_eq!(report.structs.unreachable, 0);
    assert_eq!(report.structs.unclassified, 1);
    assert!(
        report
            .incomplete_reasons
            .iter()
            .any(|reason| reason.contains("scope"))
    );
}

#[test]
fn multiple_evaluators_sharing_heap_leave_unseen_roots_unclassified() {
    let shared = RuntimeHeap::default();
    let mut first = Evaluator::new();
    let mut second = Evaluator::new();
    first.heap = shared.clone();
    second.heap = shared.clone();
    drop(shared);

    let function = first
        .heap
        .insert_function(function("owned-by-second", HashMap::new()));
    bind(&mut second, "owned", Value::Function(function));

    let report = first.probe_reachability(&[]);

    assert!(!report.complete);
    assert_eq!(report.functions.reachable, 0);
    assert_eq!(report.functions.unreachable, 0);
    assert_eq!(report.functions.unclassified, 1);
    assert!(
        report
            .incomplete_reasons
            .iter()
            .any(|reason| reason.contains("shared owner counts"))
    );
}

#[test]
fn another_evaluators_binding_keeps_shared_heap_entry_unclassified() {
    let shared = RuntimeHeap::default();
    let mut owner = Evaluator::new();
    let mut observer = Evaluator::new();
    owner.heap = shared.clone();
    observer.heap = shared.clone();
    drop(shared);

    let object = owner
        .heap
        .insert_struct(structure("OwnedElsewhere", BTreeMap::new()));
    bind(&mut owner, "object", Value::Struct(object));
    drop(owner.scopes.remove_current("object"));

    let report = observer.probe_reachability(&[]);

    assert!(!report.complete);
    assert_eq!(report.structs.reachable, 0);
    assert_eq!(report.structs.unreachable, 0);
    assert_eq!(report.structs.unclassified, 1);
}
