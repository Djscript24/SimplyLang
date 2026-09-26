mod common;
use common::*;

#[test]
fn rejects_same_scope_duplicate_declarations() {
    let (success, stderr) = run_source("x is 10\nx is 20\n");
    assert!(!success);
    assert!(stderr.contains("variable `x` is already declared in this scope"));
    assert!(stderr.contains("declare it with `mut` to reassign it"));
    assert!(stderr.contains("error[E0209] (Runtime error)"));
    assert!(stderr.contains(":2:1"));
    assert!(stderr.contains("2 | x is 20"));
}

#[test]
fn allows_reassignment_and_nested_shadowing() {
    let (success, stdout) = run_source_stdout("mut x is 10\nx -> 20\nSayln x\n");
    assert!(success, "reassignment should succeed");
    assert!(stdout.contains("20"));

    let (success, stdout) =
        run_source_stdout("x is 10\nif true:\n    x is 20\n    Sayln x\nend\nSayln x\n");
    assert!(success, "nested shadowing should succeed");
    assert!(stdout.contains("20"));
    assert!(stdout.contains("10"));
}

#[test]
fn immutable_bindings_reject_reassignment_and_collection_mutation() {
    let (success, error) = run_source("value is 1\nvalue -> 2\n");
    assert!(!success);
    assert!(error.contains("immutable variable `value`"));

    let (success, error) = run_source("values is list [1]\nvalues add 2\n");
    assert!(!success);
    assert!(error.contains("immutable variable `values`"));

    let (success, error) = run_source("values is list [1]\nvalues[0] is 2\n");
    assert!(!success);
    assert!(error.contains("immutable variable `values`"));
}

#[test]
fn loop_variables_are_rebound_without_bypassing_their_mutability_rules() {
    let (success, output) = run_source_stdout(
        "for item in [1, 2, 3]:\n\
             Sayln item\n\
         end\n",
    );

    assert!(success);
    assert_eq!(output, "1\n2\n3\n");

    let (success, error) = run_source(
        "for item in [1, 2]:\n\
             item -> 9\n\
         end\n",
    );
    assert!(!success);
    assert!(error.contains("immutable variable `item`"));
}

#[test]
fn mutable_bindings_support_typed_reassignment_and_collection_mutation() {
    let (success, stdout) = run_source_stdout(
        "mut value as Int is 1\nvalue -> 2\nmut values is list [1]\nvalues add 2\nSayln value\nSayln values\n",
    );
    assert!(success);
    assert_eq!(stdout.lines().collect::<Vec<_>>(), ["2", "[1, 2]"]);
}

#[test]
fn say_joins_output_and_sayln_terminates_the_line() {
    let (success, output) = run_source_stdout("Say \"Hello, \"\nSayln \"world\"\nSay \"!\"\n");

    assert!(success);
    assert_eq!(output, "Hello, world\n!");
}

#[test]
fn keeps_runtime_types_aligned_with_shadowed_scopes() {
    let source = "mut value is 1\nif true:\n    mut value is \"inner\"\n    value -> \"updated\"\nend\nvalue -> 2\nfn update(mut value):\n    value -> \"local\"\n    return value\nend\nSayln update(\"initial\")\nSayln value\n";
    let (success, error) = run_source(source);
    assert!(success, "shadowed bindings produced an error: {error}");
}
