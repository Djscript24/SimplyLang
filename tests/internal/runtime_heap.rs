use std::{
    collections::{BTreeMap, HashMap},
    sync::Arc,
};

use crate::{
    ast::Stmt,
    runtime::{
        heap::{ReachabilityRoots, RuntimeHeap},
        value::{EnumValue, FunctionValue, SourceText, StructInstance, Value},
    },
    types::{DeclarationIdentity, DeclarationKind},
};

#[test]
fn cloned_heap_keeps_runtime_objects_alive_until_the_last_owner_drops() {
    let heap = RuntimeHeap::default();
    let function = heap.insert_function(FunctionValue {
        name: Some("read".into()),
        parameters: Vec::new(),
        return_type: None,
        body: Arc::<[Stmt]>::from([]),
        captures: Default::default(),
        source: None,
    });
    let structure = heap.insert_struct(StructInstance {
        identity: DeclarationIdentity::new("test", "Person", DeclarationKind::Struct),
        type_name: "Person".into(),
        fields: BTreeMap::new(),
    });
    let enumeration = heap.insert_enum(EnumValue {
        identity: DeclarationIdentity::new("test", "Result", DeclarationKind::Enum),
        enum_name: "Result".into(),
        variant_name: "Ready".into(),
        payload: None,
    });
    let source = heap.insert_source(SourceText {
        text: "Sayln 1".into(),
    });

    let imported_evaluator_heap = heap.clone();
    drop(heap);

    assert!(imported_evaluator_heap.function(function).is_some());
    assert!(structure.with(|_| ()).is_some());
    assert!(enumeration.with(|_| ()).is_some());
    assert!(source.with(|_| ()).is_some());

    drop(imported_evaluator_heap);

    assert!(structure.with(|_| ()).is_none());
    assert!(enumeration.with(|_| ()).is_none());
    assert!(source.with(|_| ()).is_none());
}

#[test]
fn independent_runtime_heaps_do_not_share_runtime_objects() {
    let heap = RuntimeHeap::default();
    let other_heap = RuntimeHeap::default();
    let function = heap.insert_function(FunctionValue {
        name: None,
        parameters: Vec::new(),
        return_type: Some(crate::types::Type::Unit),
        body: Arc::<[Stmt]>::from([]),
        captures: Default::default(),
        source: None,
    });

    assert!(heap.function(function).is_some());
    assert!(other_heap.function(function).is_none());

    let source = heap.insert_source(SourceText {
        text: String::new(),
    });
    drop(heap);
    assert!(source.with(|_| ()).is_none());
}

#[test]
fn dropped_handles_do_not_reclaim_entries_while_the_heap_remains_alive() {
    const TEMPORARY_VALUES: usize = 64;
    let heap = RuntimeHeap::default();

    for index in 0..TEMPORARY_VALUES {
        let _function = heap.insert_function(FunctionValue {
            name: Some(format!("temporary_{index}")),
            parameters: Vec::new(),
            return_type: None,
            body: Arc::<[Stmt]>::from([]),
            captures: HashMap::from([(
                "captured".into(),
                Value::String("retained by the unreachable closure".into()),
            )]),
            source: None,
        });
        let _structure = heap.insert_struct(StructInstance {
            identity: DeclarationIdentity::new(
                "test",
                format!("Temporary{index}"),
                DeclarationKind::Struct,
            ),
            type_name: format!("Temporary{index}"),
            fields: BTreeMap::new(),
        });
        let _enumeration = heap.insert_enum(EnumValue {
            identity: DeclarationIdentity::new("test", "Temporary", DeclarationKind::Enum),
            enum_name: "Temporary".into(),
            variant_name: format!("Variant{index}"),
            payload: Some(Box::new(Value::String("temporary payload".into()))),
        });
        let _source = heap.insert_source(SourceText {
            text: format!("temporary source {index}"),
        });
    }

    assert_eq!(
        heap.entry_counts(),
        (
            TEMPORARY_VALUES,
            TEMPORARY_VALUES,
            TEMPORARY_VALUES,
            TEMPORARY_VALUES
        )
    );
}

#[test]
fn stale_function_generation_does_not_mark_a_live_entry_reachable() {
    let heap = RuntimeHeap::default();
    let live = heap.insert_function(FunctionValue {
        name: Some("live".into()),
        parameters: Vec::new(),
        return_type: None,
        body: Arc::<[Stmt]>::from([]),
        captures: Default::default(),
        source: None,
    });
    let stale = live
        .stale_for_probe()
        .expect("fresh test handle has a non-maximal generation");

    let report = heap.probe_reachability(ReachabilityRoots {
        external_values: vec![Value::Function(stale)],
        ..ReachabilityRoots::default()
    });

    assert!(report.complete, "{:?}", report.incomplete_reasons);
    assert_eq!(report.functions.reachable, 0);
    assert_eq!(report.functions.unreachable, 1);
    assert_eq!(report.invalid_handles, 1);
}

#[test]
fn stale_weak_generation_does_not_mark_a_live_struct_reachable() {
    let heap = RuntimeHeap::default();
    let live = heap.insert_struct(StructInstance {
        identity: DeclarationIdentity::new("test", "Person", DeclarationKind::Struct),
        type_name: "Person".into(),
        fields: BTreeMap::new(),
    });
    let stale = live
        .stale_for_probe()
        .expect("fresh test reference has a non-maximal generation");

    let report = heap.probe_reachability(ReachabilityRoots {
        external_values: vec![Value::Struct(stale)],
        ..ReachabilityRoots::default()
    });

    assert!(report.complete, "{:?}", report.incomplete_reasons);
    assert_eq!(report.structs.reachable, 0);
    assert_eq!(report.structs.unreachable, 1);
    assert_eq!(report.stale_weak_references, 1);
}

#[test]
fn foreign_function_handle_makes_unseen_local_edges_unclassified() {
    let local_heap = RuntimeHeap::default();
    let captured = local_heap.insert_struct(StructInstance {
        identity: DeclarationIdentity::new("test", "Captured", DeclarationKind::Struct),
        type_name: "Captured".into(),
        fields: BTreeMap::new(),
    });
    let foreign_heap = RuntimeHeap::default();
    let foreign_function = foreign_heap.insert_function(FunctionValue {
        name: Some("foreign".into()),
        parameters: Vec::new(),
        return_type: None,
        body: Arc::<[Stmt]>::from([]),
        captures: HashMap::from([("captured".into(), Value::Struct(captured))]),
        source: None,
    });

    let report = local_heap.probe_reachability(ReachabilityRoots {
        external_values: vec![Value::Function(foreign_function)],
        ..ReachabilityRoots::default()
    });

    assert!(!report.complete);
    assert_eq!(report.structs.reachable, 0);
    assert_eq!(report.structs.unreachable, 0);
    assert_eq!(report.structs.unclassified, 1);
    assert_eq!(report.foreign_function_handles, 1);
}
