mod common;
use common::*;
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::Command,
    process::Stdio,
    sync::atomic::Ordering,
};

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
        "identifier: Sayln",
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
    fn copy_tree(source: &Path, destination: &Path) {
        fs::create_dir_all(destination).expect("failed to create copied examples directory");
        for entry in fs::read_dir(source).expect("failed to list examples") {
            let entry = entry.expect("failed to read examples entry");
            let path = entry.path();
            let target = destination.join(entry.file_name());
            if path.is_dir() {
                copy_tree(&path, &target);
            } else {
                fs::copy(&path, &target).expect("failed to copy example fixture");
            }
        }
    }

    fn collect_sources(directory: &Path, examples: &Path, paths: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(directory).expect("failed to list examples directory") {
            let path = entry
                .expect("failed to read example directory entry")
                .path();
            if path.is_dir() {
                collect_sources(&path, examples, paths);
            } else if path.extension().is_some_and(|extension| extension == "si") {
                let relative = path
                    .strip_prefix(examples)
                    .expect("example source should be under examples");
                if relative.starts_with("99-bench")
                    || relative == Path::new("08-standard-library/imported-values.si")
                    || relative == Path::new("11-compiler-foundations/sample-source.si")
                {
                    continue;
                }
                paths.push(relative.to_path_buf());
            }
        }
    }

    let root = std::env::temp_dir().join(format!(
        "simply-example-conformance-{}-{}",
        std::process::id(),
        TEMP_SOURCE_ID.fetch_add(1, Ordering::Relaxed)
    ));
    let examples = Path::new("examples");
    copy_tree(examples, &root.join(examples));
    let mut paths = Vec::new();
    collect_sources(examples, examples, &mut paths);
    paths.sort();

    for path in paths {
        let source_path = root.join(examples).join(&path);
        let output = Command::new(env!("CARGO_BIN_EXE_simply"))
            .args([
                "run",
                source_path.to_str().expect("test path was not UTF-8"),
            ])
            .current_dir(&root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("failed to start example");
        let mut child = output;
        if path == Path::new("16-user-input/ask.si") {
            child
                .stdin
                .as_mut()
                .expect("interactive example stdin should be piped")
                .write_all(b"Ada\n25\ntrue\n")
                .expect("failed to provide deterministic example input");
        }
        drop(child.stdin.take());
        let output = child.wait_with_output().expect("failed to run example");
        assert!(
            output.status.success(),
            "example failed: {}\n{}",
            path.display(),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            !output.stdout.is_empty(),
            "example produced no output: {}",
            path.display()
        );
    }
    fs::remove_dir_all(root).expect("failed to clean copied examples");
}

#[test]
fn check_recursively_analyzes_imported_modules_and_their_returned_types() {
    let root = std::env::temp_dir().join(format!(
        "simply-check-imports-{}-{}",
        std::process::id(),
        TEMP_SOURCE_ID.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(root.join("nested")).expect("failed to create import test directory");
    fs::write(root.join("leaf.si"), "return list [4, 8]\n").expect("failed to write imported leaf");
    fs::write(
        root.join("nested/middle.si"),
        "open \"../leaf.si\" as values\nreturn values\n",
    )
    .expect("failed to write nested import");
    let main = root.join("main.si");
    fs::write(
        &main,
        "open \"nested/middle.si\" as values\n\
             open \"leaf.si\" as duplicate\n\
             Sayln values[0] + duplicate[1]\n",
    )
    .expect("failed to write import entry point");

    let output = Command::new(env!("CARGO_BIN_EXE_simply"))
        .args(["check", main.to_str().expect("test path was not UTF-8")])
        .output()
        .expect("failed to check imported program");
    let _ = fs::remove_dir_all(root);

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn check_propagates_semantic_errors_from_imported_modules() {
    let root = std::env::temp_dir().join(format!(
        "simply-check-invalid-import-{}-{}",
        std::process::id(),
        TEMP_SOURCE_ID.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&root).expect("failed to create import test directory");
    fs::write(root.join("bad.si"), "return 2 + \"bad\"\n").expect("failed to write invalid module");
    let main = root.join("main.si");
    fs::write(&main, "open \"bad.si\" as value\n").expect("failed to write import entry point");

    let output = Command::new(env!("CARGO_BIN_EXE_simply"))
        .args(["check", main.to_str().expect("test path was not UTF-8")])
        .output()
        .expect("failed to check invalid imported program");
    let error = String::from_utf8_lossy(&output.stderr);

    assert!(!output.status.success());
    assert!(error.contains("error[E0003]"), "{error}");
    assert!(error.contains("bad.si"), "{error}");

    fs::write(root.join("values.si"), "return list [2, 4]\n")
        .expect("failed to write typed imported module");
    fs::write(
        &main,
        "open \"values.si\" as values\nSayln values[0] + \"wrong\"\n",
    )
    .expect("failed to write invalid imported value use");
    let output = Command::new(env!("CARGO_BIN_EXE_simply"))
        .args(["check", main.to_str().expect("test path was not UTF-8")])
        .output()
        .expect("failed to check imported value type");
    let _ = fs::remove_dir_all(root);
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success());
    assert!(error.contains("expected"), "{error}");
}

#[test]
fn check_parses_imported_sources_without_executing_them() {
    let root = std::env::temp_dir().join(format!(
        "simply-check-import-no-exec-{}-{}",
        std::process::id(),
        TEMP_SOURCE_ID.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&root).expect("failed to create import test directory");
    let marker = root.join("executed.txt");
    fs::write(
        root.join("side-effect.si"),
        format!(
            "write_file({:?}, \"executed\")\nreturn 7\n",
            marker.to_str().expect("marker path must be UTF-8")
        ),
    )
    .expect("failed to write side-effect module");
    let main = root.join("main.si");
    fs::write(&main, "open \"side-effect.si\" as value\n")
        .expect("failed to write import entry point");

    let checked = Command::new(env!("CARGO_BIN_EXE_simply"))
        .args(["check", main.to_str().expect("test path was not UTF-8")])
        .output()
        .expect("failed to check side-effect import");
    assert!(
        checked.status.success(),
        "{}",
        String::from_utf8_lossy(&checked.stderr)
    );
    assert!(
        !marker.exists(),
        "static checking executed the imported module"
    );

    fs::write(root.join("invalid.si"), "return (1 +\n")
        .expect("failed to write syntactically invalid module");
    fs::write(&main, "open \"invalid.si\" as value\n")
        .expect("failed to write invalid import entry point");
    let invalid = Command::new(env!("CARGO_BIN_EXE_simply"))
        .args(["check", main.to_str().expect("test path was not UTF-8")])
        .output()
        .expect("failed to check syntactically invalid import");
    let error = String::from_utf8_lossy(&invalid.stderr);
    assert!(!invalid.status.success());
    assert!(error.contains("invalid.si"), "{error}");
    assert!(error.contains("Parse error"), "{error}");

    fs::remove_dir_all(root).expect("failed to remove import test directory");
}

#[test]
fn check_reports_import_cycles_and_missing_modules() {
    let root = std::env::temp_dir().join(format!(
        "simply-check-import-errors-{}-{}",
        std::process::id(),
        TEMP_SOURCE_ID.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&root).expect("failed to create import test directory");
    let main = root.join("main.si");
    fs::write(
        root.join("cycle.si"),
        "open \"main.si\" as main\nreturn main\n",
    )
    .expect("failed to write cyclic module");
    fs::write(&main, "open \"cycle.si\" as cycle\n").expect("failed to write cycle entry point");
    let cycle = Command::new(env!("CARGO_BIN_EXE_simply"))
        .args(["check", main.to_str().expect("test path was not UTF-8")])
        .output()
        .expect("failed to check cyclic imports");
    let cycle_error = String::from_utf8_lossy(&cycle.stderr);
    assert!(!cycle.status.success());
    assert!(cycle_error.contains("cyclic import"), "{cycle_error}");

    fs::write(&main, "open \"missing.si\" as missing\n")
        .expect("failed to write missing import entry point");
    let missing = Command::new(env!("CARGO_BIN_EXE_simply"))
        .args(["check", main.to_str().expect("test path was not UTF-8")])
        .output()
        .expect("failed to check missing import");
    let missing_error = String::from_utf8_lossy(&missing.stderr);
    let _ = fs::remove_dir_all(root);
    assert!(!missing.status.success());
    assert!(missing_error.contains("could not open"), "{missing_error}");
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
        vec!["explain-flow", "examples/10-flow/overview.si"],
        vec!["ast", "examples/01-basics/values.si"],
        vec!["fmt", "examples/01-basics/values.si"],
        vec!["test"],
        vec!["--help"],
    ];

    for arguments in commands {
        let output = Command::new(binary)
            .args(&arguments)
            .output()
            .expect("failed to run Simply CLI command");
        assert!(
            output.status.success(),
            "command failed: {arguments:?}\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            output.stderr.is_empty(),
            "unexpected stderr for {arguments:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let no_arguments = Command::new(binary)
        .output()
        .expect("failed to run Simply without arguments");
    assert!(no_arguments.status.success());
    let help = String::from_utf8_lossy(&no_arguments.stdout);
    assert!(help.contains("USAGE:"));
    assert!(help.contains("test               Run direct tests/*.si files"));
    assert!(help.contains("explain-flow <file.si>"));
    assert!(no_arguments.stderr.is_empty());

    let version = Command::new(binary)
        .arg("--version")
        .output()
        .expect("failed to run version command");
    assert!(version.status.success());
    assert_eq!(
        String::from_utf8_lossy(&version.stdout).trim(),
        "Simply 0.10.0"
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
    fs::write(&malformed_path, "Sayln\n").expect("failed to write malformed source");
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
        ("tests/01-basic.si", "Sayln \"basic\"\n"),
        ("tests/02-functions.si", "Sayln \"functions\"\n"),
        ("tests/ignored.txt", "Sayln @\n"),
        ("tests/nested/ignored.si", "Sayln @\n"),
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
        ("tests/01-broken.si", "Sayln\n"),
        ("tests/02-after-failure.si", "Sayln \"still runs\"\n"),
    ]);

    assert!(!success);
    assert!(stdout.contains("FAIL tests/01-broken.si"));
    assert!(stdout.contains("PASS tests/02-after-failure.si"));
    assert!(stdout.contains("test result: FAILED"));
    assert!(stdout.contains("1 passed; 1 failed"));
    assert!(stderr.contains("error[E0104]"));
    assert!(stderr.contains("1 | Sayln"));
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
        .write_all(b"x is 10\nSay \"from \"\nSayln \"say\"\nx + 5\nSayln missing\nx\n")
        .expect("failed to write REPL input");
    let output = child
        .wait_with_output()
        .expect("failed to read REPL output");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Simply 0.10.0"));
    assert!(stdout.contains("from say"));
    assert!(stdout.contains("15"));
    assert!(stdout.contains("from say\n15\n"));
    assert!(stdout.ends_with("10\n"));
    assert!(!stdout.contains("> "));
    assert!(!stdout.contains("... "));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("unknown variable `missing`"));
    assert!(stderr.contains("error[E0206]"));
}

#[test]
fn repl_preserves_struct_state_across_evaluations() {
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
            b"type Counter:\n\
              value as Int\n\
              end\n\
              on Counter receive increment:\n\
                  value -> value + 1\n\
              end\n\
              on Counter receive get:\n\
                  return value\n\
              end\n\
              counter is Counter(0)\n\
              \n\
              counter :: increment\n\
              \n\
              counter :: increment()\n\
              \n\
              counter :: get\n",
        )
        .expect("failed to write REPL input");
    drop(child.stdin.take());
    let output = child
        .wait_with_output()
        .expect("failed to read REPL output");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.lines().any(|line| line == "2"), "{stdout}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.is_empty(), "{stderr}");
}

#[test]
fn repl_preserves_enum_declarations_and_matches() {
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
            b"enum State:\n\
              Ready\n\
              Running\n\
              end\n\
              state is State::Ready\n\
              match state:\n\
                  State::Ready:\n\
                      Sayln \"ready\"\n\
                  State::Running:\n\
                      Sayln \"running\"\n\
                  end\n",
        )
        .expect("failed to write REPL input");
    drop(child.stdin.take());
    let output = child
        .wait_with_output()
        .expect("failed to read REPL output");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.lines().any(|line| line == "ready"), "{stdout}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.is_empty(), "{stderr}");
}

#[test]
fn repl_preserves_struct_identity_inside_enum_payloads() {
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
            b"type User:\n\
              name as String\n\
              end\n\
              on User receive rename(new_name as String):\n\
                  name -> new_name\n\
              end\n\
              on User receive describe:\n\
                  return name\n\
              end\n\
              enum Response:\n\
                  Success as User\n\
                  Error as String\n\
              end\n\
              user is User(\"Budi\")\n\
              response is Response::Success(user)\n\
              \n\
              user :: rename(\"Andi\")\n\
              \n\
              match response:\n\
                  Response::Success(person):\n\
                      person :: describe\n\
                  Response::Error(error):\n\
                      error\n\
              end\n",
        )
        .expect("failed to write REPL input");
    drop(child.stdin.take());
    let output = child
        .wait_with_output()
        .expect("failed to read REPL output");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.lines().any(|line| line == "Andi"), "{stdout}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.is_empty(), "{stderr}");
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
              Sayln 7 :: scale\n\
              \n\
              if true:\n\
                  value is \"inner\"\n\
                  Sayln value\n\
              end\n\
              \n\
              Sayln scale(7)\n\
              Sayln value\n\
              Sayln false and missing_value\n\
              Sayln true or missing_value\n",
        )
        .expect("failed to write REPL input");
    drop(child.stdin.take());
    let output = child
        .wait_with_output()
        .expect("failed to read REPL output");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(
        stdout.lines().collect::<Vec<_>>(),
        ["Simply 0.10.0", "21", "inner", "21", "10", "false", "true"]
    );
    assert!(!stdout.contains("> "));
    assert!(!stdout.contains("... "));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.is_empty(), "{stderr}");
}

#[test]
fn repl_supports_struct_declarations_construction_and_message_dispatch() {
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
            b"type Person:\n\
              name as String\n\
              age as Int\n\
              end\n\
              on Person receive greet:\n\
                  return \"Hello \" + name\n\
              end\n\
              person is Person(\"Ada\", 37)\n\
              Sayln person :: greet\n",
        )
        .expect("failed to write REPL input");
    drop(child.stdin.take());
    let output = child
        .wait_with_output()
        .expect("failed to read REPL output");

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .collect::<Vec<_>>(),
        ["Simply 0.10.0", "Hello Ada"]
    );
    assert!(output.stderr.is_empty());
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
            .write_all(format!("{command}\nSayln \"should not run\"\n").as_bytes())
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
