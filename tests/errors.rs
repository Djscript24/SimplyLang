mod common;
use common::*;

#[test]
fn reports_runtime_errors() {
    let (success, type_error) = run_source("value as Int is \"wrong\"\n");
    assert!(!success);
    assert!(type_error.contains("cannot assign a String value to `value`"));
    assert!(type_error.contains("expected Int, found String"));
    assert!(type_error.contains("1:1"));
    assert!(type_error.contains("error[E.runtime.type.mismatch] (Runtime error)"));

    let (success, index_error) = run_source("values is array [1]\nSayln values[2]\n");
    assert!(!success);
    assert!(index_error.contains("out of bounds"));
    assert!(index_error.contains("2:1"));

    let (success, argument_error) =
        run_source("fn one(value):\n    return value\nend\nSayln one()\n");
    assert!(!success);
    assert!(argument_error.contains("expects 1 arguments"));
    assert!(argument_error.contains("error[E.runtime.argument.invalid]"));

    let (success, collection_type_error) = run_source("values as List[String] is list [1]\n");
    assert!(!success);
    assert!(collection_type_error.contains("wrong type"));

    let (success, division_error) = run_source("Sayln 10 / 0\n");
    assert!(!success);
    assert!(division_error.contains("division by zero"));

    let (success, overflow_error) = run_source("Sayln 9223372036854775807 + 1\n");
    assert!(!success);
    assert!(overflow_error.contains("integer arithmetic error"));
    assert!(overflow_error.contains("error[E.runtime.numeric.arithmetic-invalid]"));
}

#[test]
fn runtime_assertions_use_a_catchable_diagnostic_identity() {
    let (success, output) = run_source_stdout(
        "try:\n\
             assert(false, \"condition did not hold\")\n\
         catch failure as E.runtime.assertion.failed:\n\
             Sayln failure.code\n\
         end\n",
    );

    assert!(success, "{output}");
    assert_eq!(output, "E.runtime.assertion.failed\n");
}

#[test]
fn runtime_control_errors_have_a_specific_diagnostic() {
    let (success, error) = run_source("break\n");

    assert!(!success);
    assert!(error.contains("control statement is outside its valid context"));
    assert!(
        error.contains("error[E.runtime.control.invalid]"),
        "{error}"
    );
}

#[test]
fn validates_numeric_conversion_literals_and_runtime_values() {
    let (success, _, error) =
        check_source("mut name is to_int(\"Djoest\")\nname -> 10\nSayln name\n");
    assert!(!success);
    assert!(error.contains("cannot convert `Djoest` to an Int"));
    assert!(error.contains("error[E.semantic.conversion.literal-invalid]"));
    assert!(error.contains("Replace this text with a valid number"));

    let (success, runtime_error) =
        run_source("text is \"Djoest\"\nmut name is to_int(text)\nname -> 10\nSayln name\n");
    assert!(!success);
    assert!(runtime_error.contains("cannot convert `Djoest` to an Int"));
    assert!(runtime_error.contains("error[E.runtime.conversion.failed]"));
    assert!(runtime_error.contains("to_int"));
    assert!(!runtime_error.contains("convert it explicitly"));

    let (success, output) = run_source_stdout(
        "Sayln to_int(\" 42 \")\nSayln to_float(\"3.5\")\nSayln to_float(\"1e2\")\n",
    );
    assert!(success);
    assert_eq!(output.lines().collect::<Vec<_>>(), ["42", "3.5", "100"]);

    let (success, _, error) = check_source("Sayln to_float(\"NaN\")\n");
    assert!(!success);
    assert!(error.contains("cannot convert `NaN` to a finite Float"));
    assert!(error.contains("error[E.semantic.conversion.literal-invalid]"));
}

#[test]
fn reports_stable_codes_for_lex_and_parse_errors() {
    let (success, lex_error) = run_source("Sayln ?\n");
    assert!(!success);
    assert!(lex_error.contains("error[E.lex.character.invalid]"));
    assert!(lex_error.contains("Sayln ?"));

    let (success, parse_error) = run_source("Sayln\n");
    assert!(!success);
    assert!(parse_error.contains("error[E.syntax.expression.missing]"));
    assert!(parse_error.contains("1 | Sayln"));

    let (success, number_error) = run_source("Sayln 1e\n");
    assert!(!success);
    assert!(number_error.contains("error[E.lex.number.invalid] (Lex error)"));
    assert!(number_error.contains("This number literal is not valid."));
    assert!(number_error.contains("1 | Sayln 1e"));
}

#[test]
fn allows_reassignment_values_to_start_on_the_next_line() {
    let source = "mut name is 10\nname -> \nname + 10\nSayln name\n";
    let (success, output) = run_source_stdout(source);

    assert!(success);
    assert_eq!(output, "20\n");
}

#[test]
fn explains_reassignment_with_no_value() {
    let (success, error) = run_source("mut name is 10\nname ->\n");

    assert!(!success);
    assert!(error.contains("error[E.syntax.expression.missing]"));
    assert!(error.contains("expected a value after `->`"));
    assert!(error.contains("on this line or the next line"));
    assert!(error.contains("2 | name ->"));
}

#[test]
fn rejects_malformed_programs_with_parse_diagnostics() {
    for source in [
        "values is array [1\n",
        "fn add(value):\n    return value\n",
        "if true:\n    Sayln \"yes\"\n",
        "values is list [1]\nresult is pipeline:\n    values\n    where item\n",
        "values is list [1]\nSayln values[0\n",
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
        assert!(
            error.contains("error[E.syntax."),
            "missing parse code: {error}"
        );
    }
}

#[test]
fn reports_structured_diagnostics_for_all_malformed_input_shapes() {
    let cases = [
        (
            "Sayln ?\n",
            "Lex error",
            "error[E.lex.character.invalid]",
            "1:7",
        ),
        (
            "Sayln 1e\n",
            "Lex error",
            "error[E.lex.number.invalid]",
            "1:7",
        ),
        (
            "Sayln\n",
            "Parse error",
            "error[E.syntax.expression.missing]",
            "1:6",
        ),
        (
            "open \"m.si\" as 42\n",
            "Parse error",
            "error[E.syntax.token.unexpected]",
            "1:16",
        ),
        (
            "values is array [1\n",
            "Parse error",
            "error[E.syntax.token.unexpected]",
            "2:1",
        ),
        (
            "Sayln \"unterminated\n",
            "Lex error",
            "error[E.lex.string.unterminated]",
            "1:7",
        ),
        (
            "fn broken(value):\n    return value\n",
            "Parse error",
            "error[E.syntax.token.unexpected]",
            "3:1",
        ),
        (
            "if true:\n    Sayln true\n",
            "Parse error",
            "error[E.syntax.token.unexpected]",
            "3:1",
        ),
        (
            "values is list [1]\nresult is pipeline:\n    values\n    nope\nend\n",
            "Parse error",
            "error[E.syntax.token.unexpected]",
            "4:5",
        ),
        (
            "values is list [1]\nSayln values[0\n",
            "Parse error",
            "error[E.syntax.token.unexpected]",
            "2:15",
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
    let (success, error) = run_source("value is 1\nSayln missing\n");
    assert!(!success);
    assert!(error.contains("Runtime error"));
    assert!(error.contains("2:1"));
    assert!(error.contains("2 | Sayln missing"));
    assert!(error.contains("| ^"));
}

#[test]
fn infers_primitive_types_for_reassignment() {
    let (success, error) = run_source("mut age is 18\nage -> \"Ada\"\n");
    assert!(!success);
    assert!(error.contains("cannot reassign `age` with type String"));
    assert!(error.contains("variable `age` remains type Int"));
}

#[test]
fn reports_the_original_type_for_mutable_reassignment() {
    let (success, error) = run_source("mut name as String is \"Djoest\"\nname -> 10\n");
    assert!(!success);
    assert!(error.contains("2:1"));
    assert!(error.contains("cannot reassign `name` with type Int"));
    assert!(error.contains("variable `name` remains type String"));
    assert!(error.contains("error[E.runtime.type.mismatch] (Runtime error)"));
}

#[test]
fn inferred_types_are_preserved_for_mutable_reassignment() {
    let (success, error) = run_source("mut name is \"Djoest\"\nname -> 10\n");
    assert!(!success);
    assert!(error.contains("2:1"));
    assert!(error.contains("cannot reassign `name` with type Int"));
    assert!(error.contains("variable `name` remains type String"));
    assert!(error.contains("error[E.runtime.type.mismatch] (Runtime error)"));
}

#[test]
fn checks_programs_without_executing_them() {
    let (success, output, error) = check_source("Sayln missing\n");
    assert!(!success);
    assert!(output.contains("Checking"));
    assert!(error.contains("error[E.semantic.name.undefined]"));
    assert!(error.contains("unknown variable `missing`"));

    let (success, _, error) = check_source("value as Int is \"wrong\"\n");
    assert!(!success);
    assert!(error.contains("error[E.semantic.type.mismatch] (Semantic error)"));

    let (success, output, error) = check_source("Sayln \"no output during check\"\n");
    assert!(success, "unexpected check error: {error}");
    assert!(output.contains("No errors found."));
    assert!(!output.contains("no output during check"));
}

#[test]
fn diagnostics_include_source_path_and_context() {
    let (success, _, error) = check_source("Sayln missing\n");
    assert!(!success);
    assert!(error.contains("simply-check-"));
    assert!(error.contains("1 | Sayln missing"));
    assert!(error.contains("| ^"));
}

#[test]
fn checks_expression_function_and_control_flow_types() {
    let (success, _, error) = check_source("Sayln 10 + \"hello\"\n");
    assert!(!success);
    assert!(error.contains("error[E.semantic.type.mismatch]"));

    let (success, _, error) = check_source(
        "fn add(left as Int, right as Int) gives Int:\n    return left + right\nend\nSayln add(\"hello\", 2)\n",
    );
    assert!(!success);
    assert!(error.contains("expected Int, found String"));

    let (success, _, error) = check_source("break\n");
    assert!(!success);
    assert!(error.contains("error[E.semantic.control.loop-transfer-invalid]"));

    let (success, _, error) = check_source("return 1\n");
    assert!(!success);
    assert!(error.contains("return used outside a function"));
}

#[test]
fn malformed_user_programs_return_errors_without_panicking() {
    for source in [
        "Sayln \"unterminated",
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
fn accepts_compact_minus_expressions_and_inline_comments() {
    let (success, message) =
        run_source("value is 5 # keep the newline\nSayln value-2\nSayln -value\n");
    assert!(success, "unexpected error: {message}");
}
