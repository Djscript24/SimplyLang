mod common;
use common::*;

#[test]
fn enforces_float_and_matrix_runtime_contracts() {
    let (success, float_error) = run_source("Sayln 1e308 * 1e308\n");
    assert!(!success);
    assert!(float_error.contains("floating-point result is not finite"));

    let (success, matrix_output) = run_source_stdout(
        "left is matrix [[1, 2], [3, 4]]\nright is matrix [[5, 6], [7, 8]]\nSayln left multiply right\n",
    );
    assert!(success);
    assert!(matrix_output.contains("19"));
    assert!(matrix_output.contains("50"));
}

#[test]
fn collection_aliases_detach_before_mutation() {
    let (success, stdout) = run_source_stdout(
        "values is list [1]\nmut copy is values\ncopy add 2\nSayln values\nSayln copy\n",
    );
    assert!(success);
    assert_eq!(stdout.lines().collect::<Vec<_>>(), ["[1]", "[1, 2]"]);

    let (success, stdout) = run_source_stdout(
        "profile is hash:\n    name is \"Ada\"\nend\nmut copy is profile\ncopy[\"role\"] -> \"builder\"\nSayln profile\nSayln copy\n",
    );
    assert!(success);
    assert!(
        stdout
            .lines()
            .next()
            .is_some_and(|line| !line.contains("role"))
    );
    assert!(
        stdout
            .lines()
            .nth(1)
            .is_some_and(|line| line.contains("role"))
    );
}

#[test]
fn runs_standard_library_builtins() {
    let output = run_example("examples/08-standard-library/builtins.si");
    assert!(output.contains("[1, 2, 3, 4]"));
    assert!(output.matches("4").count() >= 2);
    assert!(output.contains("6"));
}

#[test]
fn runs_boolean_and_string_collection_builtins() {
    let (success, stdout) = run_source_stdout(
        "flags is list [true, false]\nwords is list [\"Ada\", \"Lin\"]\nSayln any(flags)\nSayln all(flags)\nSayln join(words, \"-\")\n",
    );
    assert!(success);
    assert_eq!(
        stdout.lines().collect::<Vec<_>>(),
        ["true", "false", "Ada-Lin"]
    );

    let (success, _, error) = check_source("Sayln join(list [1], \",\")\n");
    assert!(!success);
    assert!(error.contains("expected String, found Int"));
}

#[test]
fn runs_direct_sum_builtin() {
    let (success, stdout) = run_source_stdout(
        "values is list [10, 20, 30]\nSayln total(values)\nSayln total(array [1.5, 2.5])\n",
    );
    assert!(success);
    assert_eq!(stdout.lines().collect::<Vec<_>>(), ["60", "4"]);

    let (success, _, error) = check_source("Sayln total(list [1, \"two\"])\n");
    assert!(!success);
    assert!(error.contains("expected a number") || error.contains("String"));
}

#[test]
fn runs_string_builtins() {
    let (success, stdout) = run_source_stdout(
        "text is \"  Ada,Lin  \"\nSayln trim(text)\nSayln split(trim(text), \",\")\nSayln replace(text, \"Ada\", \"Citra\")\nSayln starts_with(trim(text), \"Ada\")\nSayln ends_with(trim(text), \"Lin\")\n",
    );
    assert!(success);
    assert_eq!(
        stdout.lines().collect::<Vec<_>>(),
        ["Ada,Lin", "[Ada, Lin]", "  Citra,Lin  ", "true", "true"]
    );
}

#[test]
fn runs_collection_utility_builtins() {
    let (success, stdout) = run_source_stdout(
        "values is list [1, 2, 3]\nempty is list []\nSayln reverse(values)\nSayln is_empty(empty)\nSayln is_empty(values)\nSayln is_empty(\"\")\n",
    );
    assert!(success);
    assert_eq!(
        stdout.lines().collect::<Vec<_>>(),
        ["[3, 2, 1]", "true", "false", "true"]
    );
}

#[test]
fn empty_collections_keep_unknown_element_type_for_mutation() {
    let (success, output) =
        run_source_stdout("mut values is list []\nvalues add \"ready\"\nSayln values\n");
    assert!(success, "{output}");
    assert_eq!(output, "[ready]\n");
}

#[test]
fn builtin_semantics_match_runtime_collection_and_numeric_support() {
    let (success, output) = run_source_stdout(
        "Sayln total(range(1, 4))\n\
         Sayln clamp(5, 0.5, 4)\n\
         Sayln clamp(5.0, 0, 4)\n",
    );
    assert!(success, "{output}");
    assert_eq!(output, "6\n4\n4\n");
    let (success, output) =
        run_source_stdout("result is clamp(5, 0.5, 4)\nSayln type_of(result)\n");
    assert!(success, "{output}");
    assert_eq!(output, "Float\n");

    for source in [
        "Sayln any(list [1, 2])\n",
        "Sayln join(hash:\n    name is \"Ada\"\nend, \",\")\n",
        "Sayln round(1e308, 15)\n",
    ] {
        let (success, _, error) = check_source(source);
        if source.contains("round") {
            assert!(success, "{error}");
            let (ran, runtime_error) = run_source(source);
            assert!(!ran);
            assert!(
                runtime_error.contains("finite Float range"),
                "{runtime_error}"
            );
        } else {
            assert!(!success, "{error}");
        }
    }
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
fn checks_tuple_and_matrix_index_shapes() {
    let (success, _, error) = check_source("point is (10, \"Ada\")\nSayln point[2]\n");
    assert!(!success);
    assert!(error.contains("tuple index out of bounds"));

    let (success, _, error) = check_source("m is matrix [[1, 2]]\nSayln m[0]\n");
    assert!(!success);
    assert!(
        error.contains("matrix index requires a tuple of two integers"),
        "unexpected matrix index error: {error}"
    );
}

#[test]
fn indexes_tree_values_consistently_with_hash_values() {
    let (success, error) =
        run_source("profile is tree:\n    name is \"Ada\"\nend\nSayln profile[\"name\"]\n");
    assert!(success, "unexpected tree index error: {error}");
}

#[test]
fn rejects_ragged_matrices() {
    let (success, error) = run_source(
        "left is matrix [[1, 2], [3]]\nright is matrix [[1], [2]]\nSayln left + right\n",
    );
    assert!(!success);
    assert!(error.contains("equal widths"));
}

#[test]
fn rejects_ambiguous_collection_definitions() {
    let (success, duplicate_error) =
        run_source("settings is hash:\n    mode is \"a\"\n    mode is \"b\"\nend\n");
    assert!(!success);
    assert!(duplicate_error.contains("duplicate field"));

    let (success, filter_error) =
        run_source("values is list [1]\nresult is pipeline:\n    values\n    where item\nend\n");
    assert!(!success);
    assert!(
        filter_error.contains("must return a boolean"),
        "unexpected filter error: {filter_error}"
    );

    let (success, terminal_error) = run_source(
        "values is list [1]\nresult is pipeline:\n    values\n    count\n    derive item\nend\n",
    );
    assert!(!success);
    assert!(terminal_error.contains("cannot continue"));
}
