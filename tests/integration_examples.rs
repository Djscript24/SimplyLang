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

fn check_source(source: &str) -> (bool, String, String) {
    let source_id = TEMP_SOURCE_ID.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "simply-check-{}-{source_id}.si",
        std::process::id()
    ));
    fs::write(&path, source).expect("failed to write temporary Simply source");
    let output = Command::new(env!("CARGO_BIN_EXE_simply"))
        .args([
            "check",
            path.to_str().expect("temporary path was not UTF-8"),
        ])
        .output()
        .expect("failed to run Simply check");
    let _ = fs::remove_file(path);
    (
        output.status.success(),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

#[test]
fn runs_basic_values() {
    let output = run_example("examples/01-basics/values.si");
    assert!(output.contains("Hello, Simply!"));
    assert!(output.contains("3.14159"));
}

#[test]
fn every_runnable_example_is_a_conformance_regression() {
    let runnable_examples = [
        "examples/01-basics/values.si",
        "examples/02-variables/assignment.si",
        "examples/03-operators/arithmetic.si",
        "examples/03-operators/logic.si",
        "examples/04-control-flow/break-continue.si",
        "examples/04-control-flow/conditionals.si",
        "examples/04-control-flow/loops.si",
        "examples/04-control-flow/while.si",
        "examples/05-functions/functions.si",
        "examples/06-collections/arrays-lists.si",
        "examples/06-collections/hash-tree.si",
        "examples/06-collections/matrices.si",
        "examples/06-collections/tuples.si",
        "examples/07-pipelines/collections.si",
        "examples/08-standard-library/builtins.si",
        "examples/08-standard-library/inspection.si",
        "examples/09-quality/message-objects.si",
        "examples/09-quality/scope-and-short-circuit.si",
        "examples/99-smoke/smoke.si",
    ];

    for path in runnable_examples {
        let output = run_example(path);
        assert!(!output.is_empty(), "example produced no output: {path}");
    }
}

#[test]
fn cli_commands_use_expected_exit_codes_and_streams() {
    let binary = env!("CARGO_BIN_EXE_simply");
    let commands = [
        vec!["examples/99-smoke/smoke.si"],
        vec!["check", "examples/99-smoke/smoke.si"],
        vec!["--check", "examples/99-smoke/smoke.si"],
        vec!["--tokens", "examples/01-basics/values.si"],
        vec!["--ast", "examples/01-basics/values.si"],
        vec!["--format", "examples/01-basics/values.si"],
        vec!["--help"],
    ];

    for arguments in commands {
        let output = Command::new(binary)
            .args(arguments)
            .output()
            .expect("failed to run Simply CLI command");
        assert!(output.status.success());
        assert!(output.stderr.is_empty());
    }

    let invalid = Command::new(binary)
        .arg("examples/does-not-exist.si")
        .output()
        .expect("failed to run invalid Simply CLI command");
    assert!(!invalid.status.success());
    assert!(!invalid.stderr.is_empty());
    assert!(invalid.stdout.is_empty());

    let invalid_option = Command::new(binary)
        .args(["--unknown", "examples/99-smoke/smoke.si"])
        .output()
        .expect("failed to run invalid option command");
    assert!(!invalid_option.status.success());
    assert!(
        String::from_utf8_lossy(&invalid_option.stderr)
            .contains("Error: Runtime error at simply: error[E0206]: unknown option `--unknown`")
    );

    let invalid_extension = Command::new(binary)
        .arg("README.md")
        .output()
        .expect("failed to run invalid extension command");
    assert!(!invalid_extension.status.success());
    assert!(String::from_utf8_lossy(&invalid_extension.stderr).contains(
        "Error: Runtime error at README.md: error[E0206]: Simply source files must use the .si extension"
    ));
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
    assert!(type_error.contains("expected Int, found String"));
    assert!(type_error.contains("1:1"));
    assert!(type_error.contains("error[E0003]"));

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
    assert!(overflow_error.contains("error[E0203]"));
}

#[test]
fn reports_stable_codes_for_lex_and_parse_errors() {
    let (success, lex_error) = run_source("Say @\n");
    assert!(!success);
    assert!(lex_error.contains("error[E0101]"));
    assert!(lex_error.contains("Say @"));

    let (success, parse_error) = run_source("Say\n");
    assert!(!success);
    assert!(parse_error.contains("error[E0104]"));
    assert!(parse_error.contains("1 | Say"));
}

#[test]
fn rejects_malformed_programs_with_parse_diagnostics() {
    for source in [
        "values is array [1\n",
        "fn add(value):\n    return value\n",
        "if true:\n    Say \"yes\"\n",
        "values is list [1]\nresult is pipeline:\n    values\n    filter item\n",
        "values is list [1]\nSay values[0\n",
    ] {
        let (success, error) = run_source(source);
        assert!(
            !success,
            "malformed source unexpectedly succeeded: {source}"
        );
        assert!(
            error.contains("Parse error"),
            "unexpected diagnostic: {error}"
        );
        assert!(error.contains("error[E010"), "missing parse code: {error}");
    }
}

#[test]
fn reports_structured_diagnostics_for_all_malformed_input_shapes() {
    let cases = [
        ("Say @\n", "Lex error", "error[E0101]", "1:5"),
        ("Say\n", "Parse error", "error[E0104]", "1:4"),
        ("values is array [1\n", "Parse error", "error[E0103]", "2:1"),
        ("Say \"unterminated\n", "Lex error", "error[E0102]", "1:5"),
        (
            "fn broken(value):\n    return value\n",
            "Parse error",
            "error[E0103]",
            "3:1",
        ),
        (
            "if true:\n    Say true\n",
            "Parse error",
            "error[E0103]",
            "3:1",
        ),
        (
            "values is list [1]\nresult is pipeline:\n    values\n    nope\nend\n",
            "Parse error",
            "error[E0103]",
            "4:5",
        ),
        (
            "values is list [1]\nSay values[0\n",
            "Parse error",
            "error[E0103]",
            "2:13",
        ),
    ];

    for (source, category, code, location) in cases {
        let (success, error) = run_source(source);
        assert!(
            !success,
            "malformed source unexpectedly succeeded: {source}"
        );
        assert!(error.contains(category), "missing category in: {error}");
        assert!(error.contains(code), "missing code in: {error}");
        assert!(error.contains(location), "missing location in: {error}");
    }
}

#[test]
fn reports_source_context_for_runtime_errors() {
    let (success, error) = run_source("value is 1\nSay missing\n");
    assert!(!success);
    assert!(error.contains("Runtime error"));
    assert!(error.contains("2:1"));
    assert!(error.contains("2 | Say missing"));
    assert!(error.contains("| ^"));
}

#[test]
fn infers_primitive_types_for_reassignment() {
    let (success, error) = run_source("age is 18\nage -> \"Ada\"\n");
    assert!(!success);
    assert!(error.contains("wrong type"));
}

#[test]
fn checks_programs_without_executing_them() {
    let (success, output, error) = check_source("Say missing\n");
    assert!(!success);
    assert!(output.contains("Checking"));
    assert!(error.contains("error[E0001]"));
    assert!(error.contains("unknown variable `missing`"));

    let (success, output, error) = check_source("Say \"no output during check\"\n");
    assert!(success, "unexpected check error: {error}");
    assert!(output.contains("No errors found."));
    assert!(!output.contains("no output during check"));
}

#[test]
fn diagnostics_include_source_path_and_context() {
    let (success, _, error) = check_source("Say missing\n");
    assert!(!success);
    assert!(error.contains("simply-check-"));
    assert!(error.contains("1 | Say missing"));
    assert!(error.contains("| ^"));
}

#[test]
fn checks_expression_function_and_control_flow_types() {
    let (success, _, error) = check_source("Say 10 + \"hello\"\n");
    assert!(!success);
    assert!(error.contains("error[E0003]"));

    let (success, _, error) = check_source(
        "fn add(left as Int, right as Int) gives Int:\n    return left + right\nend\nSay add(\"hello\", 2)\n",
    );
    assert!(!success);
    assert!(error.contains("expected Int, found String"));

    let (success, _, error) = check_source("break\n");
    assert!(!success);
    assert!(error.contains("error[E0007]"));

    let (success, _, error) = check_source("return 1\n");
    assert!(!success);
    assert!(error.contains("return used outside a function"));
}

#[test]
fn checks_builtin_collection_arguments() {
    let (success, _, error) = check_source("contains(10, 10)\n");
    assert!(!success);
    assert!(error.contains("error[E0012]"));

    let (success, _, error) = check_source("length(10)\n");
    assert!(!success);
    assert!(error.contains("error[E0012]"));

    let (success, _, error) = check_source("contains(\"Ada\", 10)\n");
    assert!(!success);
    assert!(error.contains("expected String, found Int"));
}

#[test]
fn checks_empty_tuple_iteration_without_rejecting_valid_code() {
    let (success, _, error) = check_source("for item in ():\nend\n");
    assert!(success, "unexpected check error: {error}");
}

#[test]
fn malformed_user_programs_return_errors_without_panicking() {
    for source in [
        "Say \"unterminated",
        "values is list [1,",
        "fn broken(value):\n    return value\n",
        "values is list [1]\nresult is pipeline:\n    values\n    nope\nend\n",
    ] {
        let (success, error) = run_source(source);
        assert!(
            !success,
            "malformed source unexpectedly succeeded: {source}"
        );
        assert!(
            !error.contains("panicked at"),
            "unexpected panic output: {error}"
        );
    }
}

#[test]
fn checks_tuple_and_matrix_index_shapes() {
    let (success, _, error) = check_source("point is (10, \"Ada\")\nSay point[2]\n");
    assert!(!success);
    assert!(error.contains("tuple index out of bounds"));

    let (success, _, error) = check_source("m is matrix [[1, 2]]\nSay m[0]\n");
    assert!(!success);
    assert!(
        error.contains("matrix index requires a tuple of two integers"),
        "unexpected matrix index error: {error}"
    );
}

#[test]
fn indexes_tree_values_consistently_with_hash_values() {
    let (success, error) =
        run_source("profile is tree:\n    name is \"Ada\"\nend\nSay profile[\"name\"]\n");
    assert!(success, "unexpected tree index error: {error}");
}

#[test]
fn keeps_function_bindings_local() {
    let (success, _, error) =
        check_source("fn make():\n    local is 42\n    return local\nend\nSay make()\nSay local\n");
    assert!(!success);
    assert!(error.contains("unknown variable `local`"));
}

#[test]
fn keeps_branch_bindings_local_at_runtime() {
    let (success, error) = run_source("if true:\n    branch_value is 42\nend\nSay branch_value\n");
    assert!(!success);
    assert!(error.contains("unknown variable `branch_value`"));
}

#[test]
fn keeps_runtime_types_aligned_with_shadowed_scopes() {
    let source = "value is 1\nif true:\n    value is \"inner\"\n    value -> \"updated\"\nend\nvalue -> 2\nfn update(value):\n    value -> \"local\"\n    return value\nend\nSay update(\"initial\")\nSay value\n";
    let (success, error) = run_source(source);
    assert!(success, "shadowed bindings produced an error: {error}");
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
fn resolves_nested_and_absolute_imports_without_using_the_working_directory() {
    let root = std::env::temp_dir().join(format!("simply-nested-import-{}", std::process::id()));
    let nested = root.join("nested");
    let leaf = root.join("leaf.si");
    let middle = nested.join("middle.si");
    let main = root.join("main.si");
    fs::create_dir_all(&nested).expect("failed to create nested import directory");
    fs::write(&leaf, "return list [11]\n").expect("failed to write leaf source");
    fs::write(&middle, "open \"../leaf.si\" as values\nreturn values\n")
        .expect("failed to write middle source");
    let absolute_leaf = leaf.to_string_lossy().replace('\\', "\\\\");
    fs::write(
        &main,
        format!(
            "open \"nested/middle.si\" as nested\nopen \"{absolute_leaf}\" as absolute\nSay nested[0]\nSay absolute[0]\n"
        ),
    )
    .expect("failed to write main source");

    let output = Command::new(env!("CARGO_BIN_EXE_simply"))
        .arg(&main)
        .current_dir(&nested)
        .output()
        .expect("failed to run nested import source");
    let _ = fs::remove_dir_all(root);

    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "11\n11");
    assert!(output.stderr.is_empty());
}

#[test]
fn reports_missing_imports_with_structured_diagnostics() {
    let root = std::env::temp_dir().join(format!("simply-missing-import-{}", std::process::id()));
    let main = root.join("main.si");
    fs::create_dir_all(&root).expect("failed to create missing import directory");
    fs::write(&main, "open \"missing.si\" as missing\n")
        .expect("failed to write missing import source");

    let output = Command::new(env!("CARGO_BIN_EXE_simply"))
        .arg(&main)
        .output()
        .expect("failed to run missing import source");
    let _ = fs::remove_dir_all(root);

    let error = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success());
    assert!(error.contains("Runtime error"));
    assert!(error.contains("error[E0204]"));
    assert!(error.contains("1 | open \"missing.si\" as missing"));
}

#[test]
fn reports_circular_imports_without_recursing_forever() {
    let root = std::env::temp_dir().join(format!("simply-circular-import-{}", std::process::id()));
    let first = root.join("a.si");
    let second = root.join("b.si");
    fs::create_dir_all(&root).expect("failed to create circular import directory");
    fs::write(&first, "open \"b.si\" as b\nreturn b\n")
        .expect("failed to write first circular source");
    fs::write(&second, "open \"a.si\" as a\nreturn a\n")
        .expect("failed to write second circular source");

    let output = Command::new(env!("CARGO_BIN_EXE_simply"))
        .arg(&first)
        .output()
        .expect("failed to run circular import source");
    let _ = fs::remove_dir_all(root);

    let error = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success());
    assert!(error.contains("cyclic import"));
    assert!(error.contains("error[E0204]"));
}

#[test]
fn resolves_nested_imports_independently_of_working_directory() {
    let root = std::env::temp_dir().join(format!("simply-cwd-import-{}", std::process::id()));
    let source_dir = root.join("sources");
    let unrelated_dir = root.join("unrelated");
    fs::create_dir_all(&source_dir).expect("failed to create source directory");
    fs::create_dir_all(&unrelated_dir).expect("failed to create unrelated directory");
    let imported = source_dir.join("values.si");
    let main = source_dir.join("main.si");
    fs::write(&imported, "return list [7, 8, 9]\n").expect("failed to write imported source");
    fs::write(&main, "open \"values.si\" as values\nSay values[1]\n")
        .expect("failed to write main source");

    let output = Command::new(env!("CARGO_BIN_EXE_simply"))
        .arg(&main)
        .current_dir(&unrelated_dir)
        .output()
        .expect("failed to run source from unrelated directory");
    let _ = fs::remove_dir_all(root);

    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "8");
    assert!(output.stderr.is_empty());
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
