mod common;
use common::*;
use std::{fs, io::Write, path::Path, process::Command, process::Stdio, sync::atomic::Ordering};

#[test]
fn runs_basic_values() {
    let output = run_example("examples/01-basics/values.si");
    assert!(output.contains("Hello, Simply!"));
    assert!(output.contains("3.14159"));
}

#[test]
fn compiler_foundation_example_scans_its_source_file() {
    let output = run_example("examples/11-compiler-foundations/mini-lexer.si");
    for token in [
        "identifier: Say",
        "identifier: total",
        "number: 42",
        "operator: +",
        "string: Ada",
        "whitespace",
    ] {
        assert!(output.contains(token), "missing {token} in {output}");
    }
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
        "examples/05-functions/closures.si",
        "examples/06-collections/arrays-lists.si",
        "examples/06-collections/hash-tree.si",
        "examples/06-collections/matrices.si",
        "examples/06-collections/tuples.si",
        "examples/07-pipelines/collections.si",
        "examples/08-standard-library/builtins.si",
        "examples/08-standard-library/collections.si",
        "examples/08-standard-library/inspection.si",
        "examples/08-standard-library/strings.si",
        "examples/09-quality/message-objects.si",
        "examples/09-quality/scope-and-short-circuit.si",
        "examples/04-control-flow/try-catch-finally.si",
        "examples/10-flow/overview.si",
        "examples/10-flow/aggregates.si",
        "examples/10-flow/quality-partition.si",
        "examples/10-flow/partition-categories.si",
        "examples/10-flow/parallel-scalar.si",
        "examples/10-flow/checkpoint-write.si",
        "examples/10-flow/csv-cleanup.si",
        "examples/11-compiler-foundations/mini-lexer.si",
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
        vec!["run", "examples/99-smoke/smoke.si"],
        vec!["check", "examples/99-smoke/smoke.si"],
        vec!["tokens", "examples/01-basics/values.si"],
        vec!["bench", "examples/99-smoke/smoke.si"],
        vec!["explain", "examples/05-functions/closures.si"],
        vec!["ast", "examples/01-basics/values.si"],
        vec!["fmt", "examples/01-basics/values.si"],
        vec!["test"],
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

    let no_arguments = Command::new(binary)
        .output()
        .expect("failed to run Simply without arguments");
    assert!(no_arguments.status.success());
    assert!(String::from_utf8_lossy(&no_arguments.stdout).contains("USAGE:"));
    assert!(
        !String::from_utf8_lossy(&no_arguments.stdout)
            .contains("test               Run tests/*.si files")
    );
    assert!(no_arguments.stderr.is_empty());

    let version = Command::new(binary)
        .arg("--version")
        .output()
        .expect("failed to run version command");
    assert!(version.status.success());
    assert_eq!(
        String::from_utf8_lossy(&version.stdout).trim(),
        "Simply 0.9.0"
    );
    assert!(version.stderr.is_empty());

    let invalid = Command::new(binary)
        .args(["run", "examples/does-not-exist.si"])
        .output()
        .expect("failed to run invalid Simply CLI command");
    assert!(!invalid.status.success());
    assert!(!invalid.stderr.is_empty());
    assert!(invalid.stdout.is_empty());

    let shorthand = Command::new(binary)
        .arg("examples/99-smoke/smoke.si")
        .output()
        .expect("failed to run removed shorthand command");
    assert!(!shorthand.status.success());
    assert!(
        String::from_utf8_lossy(&shorthand.stderr)
            .contains("source file `examples/99-smoke/smoke.si` was provided without a command")
    );
    assert!(
        String::from_utf8_lossy(&shorthand.stderr)
            .contains("use `simply run examples/99-smoke/smoke.si`")
    );
    assert!(shorthand.stdout.is_empty());

    let invalid_option = Command::new(binary)
        .args(["--unknown", "examples/99-smoke/smoke.si"])
        .output()
        .expect("failed to run invalid option command");
    assert!(!invalid_option.status.success());
    assert!(
        String::from_utf8_lossy(&invalid_option.stderr).contains("error[E0301] (Command error)")
            && String::from_utf8_lossy(&invalid_option.stderr)
                .contains("Details: unknown option `--unknown`")
    );

    let abbreviated_help = Command::new(binary)
        .arg("--h")
        .output()
        .expect("failed to run abbreviated help option");
    assert!(!abbreviated_help.status.success());
    assert!(
        String::from_utf8_lossy(&abbreviated_help.stderr)
            .contains("unknown command or option `--h`; use `simply --help`")
    );
    assert!(abbreviated_help.stdout.is_empty());

    for removed_alias in ["--format", "--check", "--tokens", "--ast"] {
        let output = Command::new(binary)
            .args([removed_alias, "examples/99-smoke/smoke.si"])
            .output()
            .expect("failed to run removed CLI alias");
        assert!(
            !output.status.success(),
            "alias still accepted: {removed_alias}"
        );
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("unknown option"),
            "unexpected error for removed alias {removed_alias}"
        );
    }

    let invalid_extension = Command::new(binary)
        .args(["run", "README.md"])
        .output()
        .expect("failed to run invalid extension command");
    assert!(!invalid_extension.status.success());
    let invalid_extension_error = String::from_utf8_lossy(&invalid_extension.stderr);
    assert!(invalid_extension_error.contains("error[E0301] (Command error)"));
    assert!(invalid_extension_error.contains("  --> README.md"));
    assert!(
        invalid_extension_error.contains("Details: Simply source files must use the .si extension")
    );

    let malformed_format = Command::new(binary)
        .args(["fmt", "examples/09-quality/scope-and-short-circuit.si"])
        .output()
        .expect("failed to run formatter command");
    assert!(malformed_format.status.success());

    let malformed_path = std::env::temp_dir().join(format!(
        "simply-format-error-{}-{}.si",
        std::process::id(),
        TEMP_SOURCE_ID.fetch_add(1, Ordering::Relaxed)
    ));
    fs::write(&malformed_path, "Say\n").expect("failed to write malformed source");
    let malformed_format = Command::new(binary)
        .args(["fmt", malformed_path.to_str().expect("non-UTF-8 path")])
        .output()
        .expect("failed to run malformed formatter command");
    let _ = fs::remove_file(malformed_path);
    assert!(!malformed_format.status.success());
    assert!(String::from_utf8_lossy(&malformed_format.stderr).contains("error[E0104]"));
}

#[test]
fn test_command_discovers_only_direct_si_files_in_tests() {
    let (success, stdout, stderr) = run_test_project(&[
        ("tests/01-basic.si", "Say \"basic\"\n"),
        ("tests/02-functions.si", "Say \"functions\"\n"),
        ("tests/ignored.txt", "Say @\n"),
        ("tests/nested/ignored.si", "Say @\n"),
        ("examples/imported-values.si", "return list [1]\n"),
    ]);

    assert!(success, "test command failed: {stderr}");
    assert!(stdout.contains("PASS tests/01-basic.si"));
    assert!(stdout.contains("PASS tests/02-functions.si"));
    assert!(!stdout.contains("ignored"));
    assert!(!stdout.contains("imported-values"));
    assert!(stdout.contains("2 passed; 0 failed"));
    assert!(stderr.is_empty());
}

#[test]
fn test_command_continues_after_malformed_test_and_returns_failure() {
    let (success, stdout, stderr) = run_test_project(&[
        ("tests/01-broken.si", "Say\n"),
        ("tests/02-after-failure.si", "Say \"still runs\"\n"),
    ]);

    assert!(!success);
    assert!(stdout.contains("FAIL tests/01-broken.si"));
    assert!(stdout.contains("PASS tests/02-after-failure.si"));
    assert!(stdout.contains("test result: FAILED"));
    assert!(stdout.contains("1 passed; 1 failed"));
    assert!(stderr.contains("error[E0104]"));
    assert!(stderr.contains("1 | Say"));
}

#[test]
fn repl_preserves_state_prints_expressions_and_recovers_from_errors() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_simply"))
        .arg("repl")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to start REPL");
    child
        .stdin
        .as_mut()
        .expect("REPL stdin was unavailable")
        .write_all(b"x is 10\nSay \"from say\"\nx + 5\nSay missing\nx\n")
        .expect("failed to write REPL input");
    let output = child
        .wait_with_output()
        .expect("failed to read REPL output");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Simply 0.9.0"));
    assert!(stdout.contains("from say"));
    assert!(stdout.contains("15"));
    assert!(stdout.ends_with("10\n> "));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("unknown variable `missing`"));
    assert!(stderr.contains("error[E0206]"));
}

#[test]
fn repl_evaluates_pasted_multiline_blocks_as_single_statements() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_simply"))
        .arg("repl")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to start REPL");
    child
        .stdin
        .as_mut()
        .expect("REPL stdin was unavailable")
        .write_all(
            b"# global and function-local scope\n\
              factor is 3\n\
              value is 10\n\
              \n\
              fn scale(value as Int) gives Int:\n\
                  local is value\n\
                  return local * factor\n\
              end\n\
              \n\
              if true:\n\
                  value is \"inner\"\n\
                  Say value\n\
              end\n\
              \n\
              Say scale(7)\n\
              Say value\n\
              Say false and missing_value\n\
              Say true or missing_value\n",
        )
        .expect("failed to write REPL input");
    drop(child.stdin.take());
    let output = child
        .wait_with_output()
        .expect("failed to read REPL output");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("inner\n"));
    assert!(stdout.contains("21\n"));
    assert!(stdout.contains("10\n"));
    assert!(stdout.contains("false\n"));
    assert!(stdout.contains("true\n"));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.is_empty(), "{stderr}");
}

#[test]
fn repl_exit_aliases_quit_without_evaluating_later_input() {
    for command in [":q", ":quit", ":exit", "quit", "exit"] {
        let mut child = Command::new(env!("CARGO_BIN_EXE_simply"))
            .arg("repl")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("failed to start REPL");
        child
            .stdin
            .as_mut()
            .expect("REPL stdin was unavailable")
            .write_all(format!("{command}\nSay \"should not run\"\n").as_bytes())
            .expect("failed to write REPL input");
        let output = child
            .wait_with_output()
            .expect("failed to read REPL output");

        assert!(
            output.status.success(),
            "{command} did not exit successfully"
        );
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            !stdout.contains("should not run"),
            "{command} did not stop the REPL"
        );
        assert!(output.stderr.is_empty(), "{command} wrote to stderr");
    }
}

#[test]
fn reports_failure_for_missing_example() {
    assert!(!Path::new("examples/does-not-exist.si").exists());
}
