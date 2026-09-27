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
fn dispatches_messages_to_value_objects() {
    let output = run_example("examples/09-quality/message-objects.si");
    assert!(output.contains("Hello Ada"));
}

#[test]
fn dispatches_arguments_and_expression_receivers() {
    let source = "fn rename(self, name):\n    return self + name\nend\n\
                  fn make_person():\n    return \"Ada\"\nend\n\
                  Sayln make_person() :: rename(\" Lovelace\")\n";
    let (success, output) = run_source_stdout(source);
    assert!(success, "message dispatch failed: {output}");
    assert_eq!(output, "Ada Lovelace\n");
}

#[test]
fn evaluates_receiver_then_arguments_then_message_behavior() {
    let source = "fn make_receiver():\n    Sayln \"receiver\"\n    return \"R\"\nend\n\
                  fn make_argument():\n    Sayln \"argument\"\n    return \"A\"\nend\n\
                  fn combine(self, value):\n    Sayln \"message\"\n    return self + value\nend\n\
                  result is make_receiver() :: combine(make_argument())\n\
                  Sayln result\n";
    let (success, output) = run_source_stdout(source);
    assert!(success, "ordered message dispatch failed: {output}");
    assert_eq!(output, "receiver\nargument\nmessage\nRA\n");
}

#[test]
fn supports_empty_argument_lists_and_nested_message_arguments() {
    let source = "fn echo(self):\n    return self\nend\n\
                  fn name(self):\n    return self[\"name\"]\nend\n\
                  fn write(self, value):\n    return value\nend\n\
                  person is hash:\n    name is \"Ada\"\nend\n\
                  Sayln \"first\" :: echo\n\
                  Sayln \"second\" :: echo()\n\
                  Sayln \"log\" :: write(person :: name)\n";
    let (success, output) = run_source_stdout(source);
    assert!(success, "message evaluation failed: {output}");
    assert_eq!(output, "first\nsecond\nAda\n");
}

#[test]
fn dispatches_inside_functions_and_nested_scopes() {
    let source = "fn name(self):\n    return self[\"name\"]\nend\n\
                  fn greet(person):\n    return person :: name\nend\n\
                  person is hash:\n    name is \"Ada\"\nend\n\
                  if true:\n    Sayln greet(person)\nend\n";
    let (success, output) = run_source_stdout(source);
    assert!(success, "nested message dispatch failed: {output}");
    assert_eq!(output, "Ada\n");
}

#[test]
fn supports_explicit_nested_dispatch_and_expression_statements() {
    let source = "fn address(self):\n    return hash:\n        city is \"Jakarta\"\n    end\nend\n\
                  fn city(self):\n    return self[\"city\"]\nend\n\
                  fn birthday(self):\n    Sayln self\nend\n\
                  person is \"Ada\"\n\
                  Sayln (person :: address) :: city\n\
                  person :: birthday\n";
    let (success, output) = run_source_stdout(source);
    assert!(success, "nested dispatch failed: {output}");
    assert_eq!(output, "Jakarta\nAda\n");
}

#[test]
fn unknown_messages_fail_at_runtime() {
    let (success, error) =
        run_source("person is hash:\n    name is \"Ada\"\nend\nSayln person :: missing\n");
    assert!(!success);
    assert!(error.contains("unknown message `missing`"));
    assert!(error.contains("Hash receiver"));
    assert!(error.contains("Runtime error"));

    let (success, _, error) = check_source(
        "fn greet(self as Hash):\n    return self[\"name\"]\nend\nperson is hash:\n    name is \"Ada\"\nend\nSayln person :: greet\n",
    );
    assert!(success, "valid message failed semantic checking: {error}");
}

#[test]
fn validates_message_argument_counts_using_function_signatures() {
    for (call, count) in [
        ("person :: rename()", "expects 2 arguments, got 1"),
        (
            "person :: rename(\"A\", \"B\")",
            "expects 2 arguments, got 3",
        ),
    ] {
        let source =
            format!("fn rename(self, name):\n    return name\nend\nperson is \"Ada\"\n{call}\n");
        let (success, _, error) = check_source(&source);
        assert!(!success, "invalid message arity passed checking: {call}");
        assert!(error.contains(count), "missing arity diagnostic: {error}");
    }
}

#[test]
fn send_is_not_a_public_language_function() {
    let (success, _, error) = check_source("send(1, \"missing\")\n");
    assert!(!success);
    assert!(error.contains("unknown function `send`"));
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
