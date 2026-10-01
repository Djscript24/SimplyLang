// Internal unit tests for src/runtime/scope.rs.
use super::ScopeStack;
use crate::runtime::value::Value;

#[test]
fn resolves_locals_before_globals_and_assigns_nearest_binding() {
    let mut scopes = ScopeStack::new();
    scopes.define("value".into(), Value::Int(1), true).unwrap();
    scopes.push();
    scopes.define("value".into(), Value::Int(2), true).unwrap();

    assert_eq!(scopes.lookup("value"), Some(&Value::Int(2)));
    assert!(scopes.assign("value", Value::Int(3)));
    assert_eq!(scopes.lookup("value"), Some(&Value::Int(3)));

    scopes.pop();
    assert_eq!(scopes.lookup("value"), Some(&Value::Int(1)));
}

#[test]
fn preserves_global_scope_after_excessive_pops() {
    let mut scopes = ScopeStack::new();
    scopes
        .define("global".into(), Value::Int(1), false)
        .unwrap();
    scopes.push();
    scopes.push();

    scopes.pop();
    scopes.pop();
    scopes.pop();
    scopes.pop();

    assert_eq!(scopes.lookup("global"), Some(&Value::Int(1)));
    scopes
        .define("still_global".into(), Value::Bool(true), false)
        .unwrap();
    assert_eq!(scopes.lookup("still_global"), Some(&Value::Bool(true)));
}

#[test]
fn popping_a_scope_releases_its_arena_values() {
    let mut scopes = ScopeStack::new();
    scopes
        .define("global".into(), Value::Int(1), false)
        .expect("global binding should succeed");
    scopes.push();
    scopes
        .define("local".into(), Value::Int(2), false)
        .expect("local binding should succeed");
    assert_eq!(scopes.values.0.len(), 2);

    scopes.pop();

    assert_eq!(scopes.values.0.len(), 1);
    assert_eq!(scopes.lookup("local"), None);
    assert_eq!(scopes.lookup("global"), Some(&Value::Int(1)));
}

#[test]
fn shadowed_binding_cleanup_preserves_outer_slot_and_reuses_storage() {
    let mut scopes = ScopeStack::new();
    scopes
        .define("value".into(), Value::Int(1), true)
        .expect("outer binding should succeed");
    let outer_handle = scopes.scopes[0]["value"].0;

    let mut shadow_handles = Vec::new();
    for value in 2..=16 {
        scopes.push();
        scopes
            .define("value".into(), Value::Int(value), true)
            .expect("nested shadow should succeed");
        shadow_handles.push(scopes.scopes.last().expect("scope exists")["value"].0);
        assert_eq!(scopes.lookup("value"), Some(&Value::Int(value)));
    }

    for expected in (1..16).rev() {
        scopes.pop();
        assert_eq!(scopes.lookup("value"), Some(&Value::Int(expected)));
    }

    assert_eq!(scopes.values.0.get(outer_handle), Some(&Value::Int(1)));
    for handle in &shadow_handles {
        assert_eq!(scopes.values.0.get(*handle), None);
    }

    scopes.push();
    scopes
        .define("fresh".into(), Value::Int(3), false)
        .expect("new binding should succeed");
    let reused_handle = scopes.scopes[1]["fresh"].0;

    assert_ne!(
        reused_handle,
        *shadow_handles.last().expect("shadow handle exists")
    );
    assert_eq!(
        scopes
            .values
            .0
            .get(*shadow_handles.last().expect("shadow handle exists")),
        None
    );
    assert_eq!(scopes.values.0.get(reused_handle), Some(&Value::Int(3)));
    assert_eq!(scopes.lookup("fresh"), Some(&Value::Int(3)));
}

#[test]
fn removing_shadowed_binding_releases_only_the_inner_owner() {
    let mut scopes = ScopeStack::new();
    scopes
        .define("value".into(), Value::Int(1), true)
        .expect("outer binding should succeed");
    let outer = scopes.scopes[0]["value"].0;

    scopes.push();
    scopes
        .define("value".into(), Value::Int(2), true)
        .expect("inner binding should succeed");
    let inner = scopes.scopes[1]["value"].0;

    assert_eq!(scopes.remove_current("value"), Some(Value::Int(2)));
    assert_eq!(scopes.lookup("value"), Some(&Value::Int(1)));
    assert_eq!(scopes.values.0.get(outer), Some(&Value::Int(1)));
    assert_eq!(scopes.values.0.get(inner), None);

    scopes
        .define("replacement".into(), Value::Int(3), false)
        .expect("replacement binding should reuse available storage");
    let replacement = scopes.scopes[1]["replacement"].0;
    assert_ne!(replacement, inner);
    assert_eq!(scopes.values.0.get(inner), None);
    assert_eq!(scopes.values.0.get(replacement), Some(&Value::Int(3)));
}

#[test]
fn define_many_does_not_partially_bind_when_preflight_fails() {
    let mut scopes = ScopeStack::new();
    scopes
        .define("existing".into(), Value::Int(1), false)
        .expect("initial binding should succeed");

    let error = scopes
        .define_many(vec![
            ("new".into(), Value::Int(2), false),
            ("existing".into(), Value::Int(3), false),
        ])
        .expect_err("duplicate declaration should fail");

    assert!(error.contains("existing"));
    assert_eq!(scopes.lookup("existing"), Some(&Value::Int(1)));
    assert_eq!(scopes.lookup("new"), None);
}

#[test]
fn define_many_rejects_duplicate_targets_without_binding_anything() {
    let mut scopes = ScopeStack::new();
    let error = scopes
        .define_many(vec![
            ("same".into(), Value::Int(1), false),
            ("same".into(), Value::Int(2), false),
        ])
        .expect_err("duplicate targets should fail");

    assert!(error.contains("same"));
    assert_eq!(scopes.lookup("same"), None);
}

#[test]
fn assign_many_preflights_every_target_before_mutating() {
    let mut scopes = ScopeStack::new();
    scopes
        .define("first".into(), Value::Int(1), true)
        .expect("first binding should succeed");
    scopes
        .define("second".into(), Value::Int(2), false)
        .expect("second binding should succeed");
    let first_scope = scopes.binding_scope("first").expect("first binding exists");
    let second_scope = scopes
        .binding_scope("second")
        .expect("second binding exists");

    let error = scopes
        .assign_many(vec![
            (first_scope, "first".into(), Value::Int(10)),
            (second_scope, "second".into(), Value::Int(20)),
        ])
        .expect_err("immutable second target should fail preflight");

    assert!(error.contains("second"));
    assert_eq!(scopes.lookup("first"), Some(&Value::Int(1)));
    assert_eq!(scopes.lookup("second"), Some(&Value::Int(2)));
}
