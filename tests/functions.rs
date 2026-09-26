mod common;
use common::*;

#[test]
fn runs_functions_with_typed_parameters() {
    let output = run_example("examples/05-functions/functions.si");
    assert!(output.contains("Hello Alex"));
    assert!(output.contains("30"));
}

#[test]
fn nested_functions_resolve_lexical_bindings() {
    let (success, output) = run_source_stdout(
        "fn make_adder(base as Int):\n\
             fn add(value as Int) gives Int:\n\
                 return base + value\n\
             end\n\
             return add\n\
         end\n\
         adder is make_adder(40)\n\
         Sayln adder(2)\n",
    );

    assert!(success);
    assert_eq!(output, "42\n");
}

#[test]
fn nested_closures_preserve_dependencies_and_shadowing() {
    let (success, output) = run_source_stdout(
        "fn make_outer(base as Int):\n\
             fn make_middle(offset as Int):\n\
                 shadow is 1000\n\
                 fn add(value as Int) gives Int:\n\
                     return base + offset + value\n\
                 end\n\
                 return add\n\
             end\n\
             return make_middle\n\
         end\n\
         make_adder is make_outer(10)\n\
         adder is make_adder(20)\n\
         Sayln adder(2)\n",
    );

    assert!(success);
    assert_eq!(output, "32\n");
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
fn checks_literal_messages_before_runtime() {
    let (success, _, error) = check_source(
        "person is hash:\n    name is \"Ada\"\nend\nSayln send(person, \"missing\")\n",
    );
    assert!(!success);
    assert!(error.contains("unknown message `missing`"));

    let (success, _, error) = check_source(
        "fn greet(self as Hash):\n    return self[\"name\"]\nend\nperson is hash:\n    name is \"Ada\"\nend\nSayln send(person, \"greet\")\n",
    );
    assert!(success, "valid message failed semantic checking: {error}");
}

#[test]
fn rejects_duplicate_functions_without_panicking() {
    let (success, _, error) =
        check_source("fn greet():\n    return 1\nend\nfn greet():\n    return 2\nend\n");
    assert!(!success);
    assert!(error.contains("function `greet` is already declared"));
    assert!(!error.contains("panicked at"));
}

#[test]
fn locates_duplicate_function_errors_at_the_duplicate_declaration() {
    let (success, _, error) =
        check_source("fn greet():\n    return 1\nend\nfn greet():\n    return 2\nend\n");
    assert!(!success);
    assert!(error.contains("4:1"), "duplicate location missing: {error}");
    assert!(error.contains("error[E0017]"));
}

#[test]
fn requires_typed_functions_to_return_on_all_paths() {
    let (success, _, error) = check_source(
        "fn choose(flag as Bool) gives Int:\n    if flag:\n        return 1\n    else:\n        Sayln \"missing return\"\n    end\nend\n",
    );
    assert!(!success);
    assert!(error.contains("must return Int"));

    let (success, _, error) = check_source(
        "fn choose(flag as Bool) gives Int:\n    if flag:\n        return 1\n    else:\n        return 2\n    end\nend\n",
    );
    assert!(success, "all-path return was rejected: {error}");
}

#[test]
fn keeps_function_bindings_local() {
    let (success, _, error) = check_source(
        "fn make():\n    local is 42\n    return local\nend\nSayln make()\nSayln local\n",
    );
    assert!(!success);
    assert!(error.contains("unknown variable `local`"));
}
