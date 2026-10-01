mod common;
use common::*;

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
fn catches_runtime_errors_and_always_runs_finally() {
    let (success, output) = run_source_stdout(
        "try:\n\
             Sayln 1 / 0\n\
         catch error:\n\
             Sayln \"caught: \" + error.message\n\
         finally:\n\
             Sayln \"cleanup\"\n\
         end\n",
    );

    assert!(success);
    assert_eq!(output, "caught: division by zero\ncleanup\n");
}

#[test]
fn supports_throw_structured_errors_and_code_filtered_catches() {
    let (success, output) = run_source_stdout(
        "enum Failure:\n\
             Custom as String\n\
         end\n\
         try:\n\
             throw Failure::Custom(\"custom failure\")\n\
         catch ignored as E.runtime.numeric.division-by-zero:\n\
             Sayln \"wrong handler\"\n\
         catch error:\n\
             match error:\n\
                 Failure::Custom(message):\n\
                     Sayln \"custom failure: \" + message\n\
                 _:\n\
                     Sayln \"other error\"\n\
             end\n\
         end\n",
    );

    assert!(success);
    assert_eq!(output, "custom failure: custom failure\n");
}

#[test]
fn throw_requires_a_tagged_enum_value() {
    let source = "throw \"not an enum\"\n";
    let (checked, _, diagnostic) = check_source(source);
    assert!(!checked, "check accepted a non-enum throw value");
    assert!(
        diagnostic.contains("`throw` requires an enum value, found String"),
        "{diagnostic}"
    );

    let (ran, diagnostic) = run_source(source);
    assert!(!ran, "runtime accepted a non-enum throw value");
    assert!(
        diagnostic.contains("`throw` requires an enum value, found String"),
        "{diagnostic}"
    );
}

#[test]
fn check_rejects_unknown_catch_diagnostic_codes() {
    let source = "enum Failure:\n\
                      Missing\n\
                  end\n\
                  try:\n\
                      throw Failure::Missing\n\
                 catch error as E9999:\n\
                     Sayln \"unreachable\"\n\
                 end\n";
    let (success, _, error) = check_source(source);

    assert!(!success, "check accepted an unknown diagnostic code");
    assert!(
        error.contains("unknown diagnostic code `E9999` in catch clause"),
        "{error}"
    );

    let valid_source = "enum Failure:\n\
                            Missing\n\
                        end\n\
                        try:\n\
                            throw Failure::Missing\n\
                        catch error as E.runtime.numeric.division-by-zero:\n\
                            Sayln \"caught\"\n\
                        end\n";
    let (success, _, error) = check_source(valid_source);
    assert!(success, "check rejected a known diagnostic code: {error}");
}

#[test]
fn catch_filters_accept_canonical_paths_and_legacy_aliases() {
    for code in ["E.runtime.numeric.division-by-zero", "E0202"] {
        let source = format!(
            "try:\n\
                 Sayln 1 / 0\n\
             catch error as {code}:\n\
                 Sayln error.code\n\
             end\n"
        );
        let (success, output) = run_source_stdout(&source);
        assert!(success, "catch filter `{code}` failed: {output}");
        assert_eq!(output, "E.runtime.numeric.division-by-zero\n");
    }
}

#[test]
fn supports_nested_try_blocks_and_propagates_to_outer_catch() {
    let (success, output) = run_source_stdout(
        "enum Failure:\n\
             Inner as String\n\
         end\n\
         try:\n\
             try:\n\
                 throw Failure::Inner(\"inner failure\")\n\
             finally:\n\
                 Sayln \"inner cleanup\"\n\
             end\n\
         catch error:\n\
             match error:\n\
                 Failure::Inner(message):\n\
                     Sayln \"outer caught: \" + message\n\
                 _:\n\
                     Sayln \"another runtime error\"\n\
             end\n\
         finally:\n\
             Sayln \"outer cleanup\"\n\
         end\n",
    );

    assert!(success, "{output}");
    assert_eq!(
        output,
        "inner cleanup\nouter caught: inner failure\nouter cleanup\n"
    );
}

#[test]
fn finally_runs_before_propagating_an_uncaught_error() {
    let (success, output) = run_source_stdout(
        "try:\n\
             Sayln 1 / 0\n\
         finally:\n\
             Sayln \"cleanup\"\n\
         end\n",
    );

    assert!(!success);
    assert_eq!(output, "cleanup\n");
}

#[test]
fn finally_control_flow_overrides_pending_control_flow() {
    let (success, output) = run_source_stdout(
        "fn value() gives Int:\n\
             try:\n\
                 return 1\n\
             finally:\n\
                 return 2\n\
             end\n\
         end\n\
         Sayln value()\n",
    );

    assert!(success);
    assert_eq!(output, "2\n");
}

#[test]
fn keeps_branch_bindings_local_at_runtime() {
    let (success, error) =
        run_source("if true:\n    branch_value is 42\nend\nSayln branch_value\n");
    assert!(!success);
    assert!(error.contains("unknown variable `branch_value`"));
}

#[test]
fn for_loop_scope_is_restored_when_an_error_escapes_the_loop() {
    let source = "enum Failure:\n\
                      Stop\n\
                  end\n\
                  try:\n\
                      hidden is 9\n\
                      for item in [1]:\n\
                          throw Failure::Stop\n\
                      end\n\
                  catch error:\n\
                      Sayln \"caught\"\n\
                  end\n\
                  Sayln hidden\n";

    let (success, error) = run_source(source);

    assert!(!success, "try scope leaked after loop error cleanup");
    assert!(error.contains("unknown variable `hidden`"), "{error}");
}

#[test]
fn check_does_not_allow_loop_control_to_cross_a_function_boundary() {
    let source = "while true:\n\
                      fn invalid():\n\
                          break\n\
                      end\n\
                      invalid()\n\
                      break\n\
                  end\n";
    let (success, _, error) = check_source(source);

    assert!(
        !success,
        "check accepted break outside the nested function's loop"
    );
    assert!(
        error.contains("break or continue used outside a loop"),
        "{error}"
    );

    let (ran, error) = run_source(source);
    assert!(
        !ran,
        "runtime accepted loop control across a function boundary"
    );
    assert!(
        error.contains("break/continue used outside a loop"),
        "{error}"
    );
}

#[test]
fn check_accepts_a_returning_try_with_a_non_returning_finally() {
    let source = "fn value() gives Int:\n\
                      try:\n\
                          return 1\n\
                      finally:\n\
                          Sayln \"cleanup\"\n\
                      end\n\
                  end\n\
                  Sayln value()\n";
    let (checked, _, error) = check_source(source);
    assert!(checked, "check rejected a returning try/finally: {error}");

    let (ran, output) = run_source_stdout(source);
    assert!(ran, "returning try/finally failed at runtime: {output}");
    assert_eq!(output, "cleanup\n1\n");
}

#[test]
fn check_requires_every_handled_catch_path_to_return() {
    let source = "fn value() gives Int:\n\
                      try:\n\
                          return 1\n\
                      catch arithmetic as E.runtime.numeric.division-by-zero:\n\
                          return 2\n\
                      catch other:\n\
                          Sayln \"handled without returning\"\n\
                      end\n\
                  end\n\
                  Sayln value()\n";
    let (checked, _, error) = check_source(source);

    assert!(!checked, "check accepted a non-returning catch path");
    assert!(error.contains("must return Int"), "{error}");
}

#[test]
fn short_circuit_checks_dynamic_operands_and_skips_literal_unreachable_operands() {
    let dynamic = "flag is false\nSayln flag and missing_value\n";
    let (checked, _, error) = check_source(dynamic);
    assert!(!checked, "check skipped a dynamically reachable operand");
    assert!(
        error.contains("unknown variable `missing_value`"),
        "{error}"
    );

    let literal = "Sayln false and missing_value\n\
                   Sayln true or missing_value\n";
    let (checked, _, error) = check_source(literal);
    assert!(
        checked,
        "check did not preserve literal short-circuit semantics: {error}"
    );

    let source = "Sayln false and (1 / 0 == 0)\n\
                  Sayln true or (1 / 0 == 0)\n";
    let (checked, _, error) = check_source(source);
    assert!(
        checked,
        "valid short-circuit expressions failed check: {error}"
    );

    let (ran, output) = run_source_stdout(source);
    assert!(ran, "short-circuited division was evaluated: {output}");
    assert_eq!(output, "false\ntrue\n");
}

#[test]
fn returns_in_match_arms_supply_the_match_expression_value() {
    let source = "fn choose() gives Int:\n\
                      return match true:\n\
                          true:\n\
                              return 1\n\
                          false:\n\
                              return 2\n\
                      end\n\
                  end\n\
                  Sayln choose()\n";
    let (checked, _, error) = check_source(source);

    assert!(checked, "check rejected match-arm return values: {error}");
    let (ran, output) = run_source_stdout(source);
    assert!(ran, "match-arm return failed at runtime: {output}");
    assert_eq!(output, "1\n");
}

#[test]
fn check_rejects_loop_control_inside_match_arms() {
    let source = "while true:\n\
                      match 1:\n\
                          _:\n\
                              break\n\
                      end\n\
                  end\n";
    let (checked, _, error) = check_source(source);

    assert!(!checked, "check accepted loop control intercepted by match");
    assert!(
        error.contains("break or continue used outside a loop"),
        "{error}"
    );

    let (ran, error) = run_source(source);
    assert!(!ran, "runtime accepted loop control intercepted by match");
    assert!(
        error.contains("break/continue used outside a loop"),
        "{error}"
    );
}

#[test]
fn loops_inside_match_arms_consume_their_own_control_flow() {
    let source = "match 1:\n\
                      _:\n\
                          for item in [1, 2]:\n\
                              Sayln item\n\
                              break\n\
                          end\n\
                  end\n";
    let (checked, _, error) = check_source(source);
    assert!(checked, "nested loop control failed check: {error}");

    let (ran, output) = run_source_stdout(source);
    assert!(
        ran,
        "loop control escaped its loop in the match arm: {output}"
    );
    assert_eq!(output, "1\n");
}

#[test]
fn nested_loops_consume_only_their_own_break_and_continue() {
    let source = "for outer in [1, 2, 3]:\n\
                      for inner in [1, 2, 3]:\n\
                          if inner == 2:\n\
                              continue\n\
                          end\n\
                          Sayln outer * 10 + inner\n\
                          if outer == 2 and inner == 3:\n\
                              break\n\
                          end\n\
                      end\n\
                  end\n";
    let (success, output) = run_source_stdout(source);

    assert!(success, "nested loop control failed: {output}");
    assert_eq!(output, "11\n13\n21\n23\n31\n33\n");
}

#[test]
fn iterable_is_evaluated_once_and_return_exits_the_enclosing_function() {
    let source = "fn values():\n\
                      Sayln \"iterable\"\n\
                      return [1, 2, 3]\n\
                  end\n\
                  fn find() gives Int:\n\
                      for item in values():\n\
                          if item == 2:\n\
                              return item\n\
                          end\n\
                      end\n\
                      return 0\n\
                  end\n\
                  Sayln find()\n";
    let (checked, _, error) = check_source(source);
    assert!(checked, "valid loop return failed check: {error}");

    let (success, output) = run_source_stdout(source);
    assert!(success, "return from a loop failed: {output}");
    assert_eq!(output, "iterable\n2\n");
}

#[test]
fn if_evaluates_only_the_selected_branch_and_mutates_outer_bindings() {
    let source = "mut value is 0\n\
                  if true:\n\
                      value -> 1\n\
                  else:\n\
                      Sayln 1 / 0\n\
                      value -> 2\n\
                  end\n\
                  Sayln value\n";
    let (checked, _, error) = check_source(source);
    assert!(checked, "valid branch mutation failed check: {error}");

    let (ran, output) = run_source_stdout(source);
    assert!(ran, "unselected branch was evaluated: {output}");
    assert_eq!(output, "1\n");
}
