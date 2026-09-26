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
        "try:\n\
             throw \"custom failure\"\n\
         catch ignored as E0202:\n\
             Sayln \"wrong handler\"\n\
         catch error:\n\
             Sayln error.code + \": \" + error.message\n\
         end\n",
    );

    assert!(success);
    assert_eq!(output, "E0206: custom failure\n");
}

#[test]
fn supports_nested_try_blocks_and_propagates_to_outer_catch() {
    let (success, output) = run_source_stdout(
        "try:\n\
             try:\n\
                 throw \"inner failure\"\n\
             finally:\n\
                 Sayln \"inner cleanup\"\n\
             end\n\
         catch error:\n\
             Sayln \"outer caught: \" + error.message\n\
         finally:\n\
             Sayln \"outer cleanup\"\n\
         end\n",
    );

    assert!(success);
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
