mod common;
use common::*;

#[test]
fn runs_functions_with_typed_parameters() {
    let output = run_example("examples/05-functions/functions.si");
    assert!(output.contains("Hello Alex"));
    assert!(output.contains("30"));
}

#[test]
fn direct_and_mutual_recursion_stop_at_the_call_depth_limit() {
    for source in [
        "fn recurse(value as Int) gives Int:\n    return recurse(value + 1)\nend\nrecurse(0)\n",
        "fn first(value as Int) gives Int:\n    return second(value + 1)\nend\n\
         fn second(value as Int) gives Int:\n    return first(value + 1)\nend\n\
         first(0)\n",
    ] {
        let (checked, _, error) = check_source(source);
        assert!(checked, "recursive functions failed check: {error}");
        let (success, error) = run_source(source);
        assert!(!success, "unbounded recursion unexpectedly succeeded");
        assert!(
            error.contains("function call depth exceeds the limit"),
            "{error}"
        );
        assert!(error.contains("error[E.runtime.limit.exceeded]"), "{error}");
    }
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
fn top_level_functions_are_hoisted_but_later_nested_siblings_are_not_captured() {
    let top_level_forward = "Sayln first()\n\
                             saved is second\n\
                             fn first() gives Int:\n\
                                 return second()\n\
                             end\n\
                             fn second() gives Int:\n\
                                 return 42\n\
                             end\n\
                             Sayln saved()\n";
    let (checked, _, error) = check_source(top_level_forward);
    assert!(checked, "top-level forward call failed check: {error}");
    let (success, output) = run_source_stdout(top_level_forward);
    assert!(
        success,
        "runtime rejected a top-level forward call: {output}"
    );
    assert_eq!(output, "42\n42\n");

    let declared_before_use = "fn first() gives Int:\n\
                                   return second()\n\
                               end\n\
                               fn second() gives Int:\n\
                                   return 42\n\
                               end\n\
                               Sayln first()\n";
    let (checked, _, error) = check_source(declared_before_use);
    assert!(
        checked,
        "functions declared before use failed check: {error}"
    );
    let (success, output) = run_source_stdout(declared_before_use);
    assert!(
        success,
        "functions declared before use failed at runtime: {output}"
    );
    assert_eq!(output, "42\n");

    let later_sibling = "fn outer():\n\
                             fn use_later() gives Int:\n\
                                 return later()\n\
                             end\n\
                             fn later() gives Int:\n\
                                 return 1\n\
                             end\n\
                             return use_later\n\
                         end\n\
                         f is outer()\n\
                         Sayln f()\n";
    let (checked, _, error) = check_source(later_sibling);
    assert!(
        !checked,
        "check accepted a later sibling closure dependency"
    );
    assert!(
        error.contains("declared later and is not captured by this closure"),
        "{error}"
    );
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
fn returned_nested_functions_can_recurse() {
    let source = "fn make_countdown():\n\
                      fn countdown(value as Int) gives Int:\n\
                          if value == 0:\n\
                              return 0\n\
                          else:\n\
                              return countdown(value - 1)\n\
                          end\n\
                      end\n\
                      return countdown\n\
                  end\n\
                  run_countdown is make_countdown()\n\
                  Sayln run_countdown(3)\n";
    let (success, output) = run_source_stdout(source);

    assert!(success, "returned recursive closure failed: {output}");
    assert_eq!(output, "0\n");
}

#[test]
fn closure_capture_analysis_respects_nested_block_shadowing() {
    let source = "fn make_reader():\n\
                      value is 7\n\
                      fn read(flag as Bool) gives Int:\n\
                          if flag:\n\
                              value is 100\n\
                          end\n\
                          return value\n\
                      end\n\
                      return read\n\
                  end\n\
                  read is make_reader()\n\
                  Sayln read(false)\n";
    let (checked, _, error) = check_source(source);
    assert!(checked, "closure did not pass semantic checking: {error}");
    let (success, output) = run_source_stdout(source);

    assert!(success, "closure lost its outer binding: {output}");
    assert_eq!(output, "7\n");
}

#[test]
fn closures_keep_creation_time_snapshots_across_rebinding() {
    let source = "fn make_reader():\n\
                      mut value is 1\n\
                      fn read() gives Int:\n\
                          return value\n\
                      end\n\
                      value -> 2\n\
                      return read\n\
                  end\n\
                  read is make_reader()\n\
                  Sayln read()\n\
                  Sayln read()\n";
    let (success, output) = run_source_stdout(source);

    assert!(success, "snapshot closure failed: {output}");
    assert_eq!(output, "1\n1\n");
}

#[test]
fn returned_closure_keeps_collection_snapshot_after_scope_cleanup() {
    let source = "fn make_reader():\n\
                      mut values is list [1]\n\
                      fn read() gives List[Int]:\n\
                          return values\n\
                      end\n\
                      values add 2\n\
                      return (read, values)\n\
                  end\n\
                  mut (read, values) is make_reader()\n\
                  values add 3\n\
                  Sayln read()\n\
                  Sayln values\n";
    let (success, output) = run_source_stdout(source);

    assert!(
        success,
        "returned closure lost its captured value: {output}"
    );
    assert_eq!(output, "[1]\n[1, 2, 3]\n");
}

#[test]
fn closure_keeps_captured_struct_identity_after_mutation_and_rebinding() {
    let source = "type Person:\n    name as String\nend\n\
                  on Person receive rename(next as String):\n\
                      name -> next\n\
                  end\n\
                  on Person receive get_name:\n\
                      return name\n\
                  end\n\
                  fn make_reader(person as Person):\n\
                      fn read():\n\
                          return person\n\
                      end\n\
                      return read\n\
                  end\n\
                  mut person is Person(\"Ada\")\n\
                  read is make_reader(person)\n\
                  person :: rename(\"Alicia\")\n\
                  captured_after_mutation is read()\n\
                  Sayln captured_after_mutation :: get_name\n\
                  person -> Person(\"Grace\")\n\
                  captured_after_rebinding is read()\n\
                  Sayln captured_after_rebinding :: get_name\n";
    let (success, output) = run_source_stdout(source);

    assert!(success, "struct closure capture failed: {output}");
    assert_eq!(output, "Alicia\nAlicia\n");
}

#[test]
fn closures_can_capture_pattern_arm_bindings() {
    let source = "fn make_reader(value as Int):\n\
                      return match value:\n\
                          captured:\n\
                              fn read() gives Int:\n\
                                  return captured\n\
                              end\n\
                              read\n\
                      end\n\
                  end\n\
                  read is make_reader(42)\n\
                  Sayln read()\n";
    let (checked, _, error) = check_source(source);
    assert!(checked, "pattern-binding closure failed check: {error}");
    let (success, output) = run_source_stdout(source);

    assert!(success, "closure lost a pattern-arm binding: {output}");
    assert_eq!(output, "42\n");
}

#[test]
fn check_rejects_reassignment_of_snapshot_captures() {
    let source = "fn make_setter():\n\
                      mut value is 1\n\
                      fn set_value():\n\
                          value -> 2\n\
                          return value\n\
                      end\n\
                      return set_value\n\
                  end\n";
    let (success, _, error) = check_source(source);

    assert!(!success, "check accepted mutation of an immutable capture");
    assert!(
        error.contains("cannot reassign immutable variable `value`"),
        "{error}"
    );

    let source = format!("{source}setter is make_setter()\nsetter()\n");
    let (success, error) = run_source(&source);
    assert!(!success, "runtime allowed mutation of a snapshot capture");
    assert!(
        error.contains("cannot reassign immutable variable `value`"),
        "{error}"
    );
}

#[test]
fn nested_function_names_remain_in_their_lexical_scope() {
    let hidden = "fn outer():\n\
                      fn local():\n\
                          return 1\n\
                      end\n\
                      return 0\n\
                  end\n\
                  local()\n";
    let (success, _, error) = check_source(hidden);
    assert!(
        !success,
        "check exposed a nested function outside its scope"
    );
    assert!(error.contains("unknown function `local`"), "{error}");

    let same_name_in_separate_functions = "fn first():\n\
                                               fn local():\n\
                                                   return 1\n\
                                               end\n\
                                               return local()\n\
                                           end\n\
                                           fn second():\n\
                                               fn local():\n\
                                                   return 2\n\
                                               end\n\
                                               return local()\n\
                                           end\n\
                                           Sayln first() + second()\n";
    let (checked, _, error) = check_source(same_name_in_separate_functions);
    assert!(
        checked,
        "same-scope function names conflicted in check: {error}"
    );
    let (success, output) = run_source_stdout(same_name_in_separate_functions);
    assert!(success, "separate lexical functions conflicted: {output}");
    assert_eq!(output, "3\n");
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
fn rejects_unknown_dispatch_targets_for_statically_known_receivers() {
    let (success, _, error) =
        check_source("person is hash:\n    name is \"Ada\"\nend\nSayln person :: missing\n");

    assert!(!success, "unknown dispatch target passed semantic checking");
    assert!(error.contains("unknown message `missing` for a Hash receiver"));

    let wrong_receiver = "fn greet(value as String):\n    return value\nend\n\
                          person is hash:\n    name is \"Ada\"\nend\nperson :: greet\n";
    let (success, _, error) = check_source(wrong_receiver);
    assert!(
        !success,
        "incompatible receiver type passed semantic checking"
    );
    assert!(error.contains("expected String, found Hash"), "{error}");
}

#[test]
fn dispatches_to_function_values_when_their_type_is_known() {
    let source = "fn greet(person as String, suffix as String) gives String:\n\
                      return person + suffix\n\
                  end\n\
                  alias is greet\n\
                  Sayln \"Ada\" :: alias(\"!\")\n";
    let (success, output) = run_source_stdout(source);
    assert!(success, "function-value dispatch failed: {output}");
    assert_eq!(output, "Ada!\n");

    let inferred_source = "fn greet(person as String, suffix as String):\n\
                               return person + suffix\n\
                           end\n\
                           alias is greet\n\
                           value as String is \"Ada\" :: alias(\"!\")\n\
                           Sayln value\n";
    let (success, _, error) = check_source(inferred_source);
    assert!(
        success,
        "inferred function-value return was rejected: {error}"
    );
    let (success, output) = run_source_stdout(inferred_source);
    assert!(
        success,
        "inferred function-value return was rejected: {output}"
    );
    assert_eq!(output, "Ada!\n");
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
    assert!(error.contains("error[E.semantic.declaration.duplicate]"));
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
