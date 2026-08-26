use std::{
    fs,
    path::Path,
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};

static TEMP_SOURCE_ID: AtomicUsize = AtomicUsize::new(0);

fn run_example(path: &str) -> String {
    let binary = env!("CARGO_BIN_EXE_simply");
    let output = Command::new(binary)
        .arg(path)
        .output()
        .expect("failed to run Simply example");
    assert!(output.status.success(), "example failed: {path}");
    String::from_utf8(output.stdout).expect("example output was not UTF-8")
}

fn run_source(source: &str) -> (bool, String) {
    let source_id = TEMP_SOURCE_ID.fetch_add(1, Ordering::Relaxed);
    let path =
        std::env::temp_dir().join(format!("simply-test-{}-{source_id}.si", std::process::id()));
    fs::write(&path, source).expect("failed to write temporary Simply source");
    let output = Command::new(env!("CARGO_BIN_EXE_simply"))
        .arg(&path)
        .output()
        .expect("failed to run temporary Simply source");
    let _ = fs::remove_file(path);
    let message = String::from_utf8_lossy(&output.stderr).into_owned();
    (output.status.success(), message)
}

#[test]
fn runs_basic_values() {
    let output = run_example("examples/01-basics/values.si");
    assert!(output.contains("Hello, Simply!"));
    assert!(output.contains("3.14159"));
}

#[test]
fn runs_functions_with_typed_parameters() {
    let output = run_example("examples/05-functions/functions.si");
    assert!(output.contains("Hello Alex"));
    assert!(output.contains("30"));
}

#[test]
fn runs_collections_and_pipelines() {
    let collections = run_example("examples/06-collections/arrays-lists.si");
    let pipeline = run_example("examples/07-pipelines/collections.si");
    assert!(collections.contains("Citra"));
    assert!(pipeline.contains("180"));
}

#[test]
fn runs_standard_library_builtins() {
    let output = run_example("examples/08-standard-library/builtins.si");
    assert!(output.contains("[1, 2, 3, 4]"));
    assert!(output.matches("4").count() >= 2);
    assert!(output.contains("6"));
}

#[test]
fn runs_collection_inspection_builtins() {
    let output = run_example("examples/08-standard-library/inspection.si");
    assert!(output.contains("true"));
    assert!(output.contains("false"));
    assert!(output.contains("List"));
    assert!(output.contains("Printed with the print function"));
}

#[test]
fn preserves_function_globals_and_short_circuits() {
    let output = run_example("examples/09-quality/scope-and-short-circuit.si");
    assert!(output.contains("21"));
    assert!(output.contains("false"));
    assert!(output.contains("true"));
}

#[test]
fn sends_messages_to_value_objects() {
    let output = run_example("examples/09-quality/message-objects.si");
    assert!(output.contains("Hello Ada"));
}

#[test]
fn runs_while_loop_and_typed_collections() {
    let output = run_example("examples/04-control-flow/while.si");
    let typed = run_example("examples/02-variables/assignment.si");
    assert!(output.contains("0") && output.contains("1") && output.contains("2"));
    assert!(typed.contains("[1, 2, 3]"));
}

#[test]
fn runs_break_and_continue() {
    let output = run_example("examples/04-control-flow/break-continue.si");
    assert_eq!(output.lines().collect::<Vec<_>>(), ["1", "3", "4"]);
}

#[test]
fn reports_runtime_errors() {
    let (success, type_error) = run_source("value as Int is \"wrong\"\n");
    assert!(!success);
    assert!(type_error.contains("wrong type"));
    assert!(type_error.contains("1:1"));

    let (success, index_error) = run_source("values is array [1]\nSay values[2]\n");
    assert!(!success);
    assert!(index_error.contains("out of bounds"));
    assert!(index_error.contains("2:1"));

    let (success, argument_error) =
        run_source("fn one(value):\n    return value\nend\nSay one()\n");
    assert!(!success);
    assert!(argument_error.contains("expects 1 arguments"));

    let (success, collection_type_error) = run_source("values as List[String] is list [1]\n");
    assert!(!success);
    assert!(collection_type_error.contains("wrong type"));

    let (success, division_error) = run_source("Say 10 / 0\n");
    assert!(!success);
    assert!(division_error.contains("division by zero"));

    let (success, overflow_error) = run_source("Say 9223372036854775807 + 1\n");
    assert!(!success);
    assert!(overflow_error.contains("integer arithmetic error"));
}

#[test]
fn accepts_compact_minus_expressions_and_inline_comments() {
    let (success, message) = run_source("value is 5 # keep the newline\nSay value-2\nSay -value\n");
    assert!(success, "unexpected error: {message}");
}

#[test]
fn rejects_ragged_matrices() {
    let (success, error) =
        run_source("left is matrix [[1, 2], [3]]\nright is matrix [[1], [2]]\nSay left + right\n");
    assert!(!success);
    assert!(error.contains("equal widths"));
}

#[test]
fn reports_failure_for_missing_example() {
    assert!(!Path::new("examples/does-not-exist.si").exists());
}

#[test]
fn imports_a_returned_value_relative_to_the_source_file() {
    let directory = std::env::temp_dir().join(format!("simply-import-{}", std::process::id()));
    fs::create_dir_all(&directory).expect("failed to create import directory");
    let imported = directory.join("values.si");
    let main = directory.join("main.si");
    fs::write(&imported, "return list [4, 8, 15]\n").expect("failed to write imported source");
    fs::write(&main, "open \"values.si\" as values\nSay values[1]\n")
        .expect("failed to write importing source");

    let output = Command::new(env!("CARGO_BIN_EXE_simply"))
        .arg(&main)
        .output()
        .expect("failed to run importing source");
    fs::remove_dir_all(&directory).expect("failed to remove import directory");

    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "8");
}

#[test]
fn restores_item_after_pipeline_evaluation() {
    let path = std::env::temp_dir().join(format!("simply-pipeline-{}.si", std::process::id()));
    fs::write(
        &path,
        "item is 99\nvalues is list [1, 2]\nresult is pipeline:\n    values\n    map item * 2\n    count\nend\nSay item\n",
    )
    .expect("failed to write pipeline source");
    let output = Command::new(env!("CARGO_BIN_EXE_simply"))
        .arg(&path)
        .output()
        .expect("failed to run pipeline source");
    let _ = fs::remove_file(path);

    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "99");
}

#[test]
fn rejects_ambiguous_collection_definitions() {
    let (success, duplicate_error) =
        run_source("settings is hash:\n    mode is \"a\"\n    mode is \"b\"\nend\n");
    assert!(!success);
    assert!(duplicate_error.contains("duplicate field"));

    let (success, filter_error) =
        run_source("values is list [1]\nresult is pipeline:\n    values\n    filter item\nend\n");
    assert!(!success);
    assert!(
        filter_error.contains("must return a boolean"),
        "unexpected filter error: {filter_error}"
    );

    let (success, terminal_error) = run_source(
        "values is list [1]\nresult is pipeline:\n    values\n    count\n    map item\nend\n",
    );
    assert!(!success);
    assert!(terminal_error.contains("cannot continue"));
}
