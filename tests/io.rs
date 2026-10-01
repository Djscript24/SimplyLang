mod common;
use common::*;
use std::{
    fs,
    io::Write,
    process::{Command, Stdio},
    sync::atomic::Ordering,
};

#[test]
fn asks_for_dynamic_strings_and_statically_typed_primitive_values() {
    let id = TEMP_SOURCE_ID.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!("simply-ask-{id}"));
    fs::create_dir_all(&root).expect("failed to create Ask test directory");
    let source = root.join("main.si");
    fs::write(
        &source,
        "name is Ask(\"Name: \")\n\
         age as Int is Ask(\"Age: \", Int)\n\
         ratio as Float is Ask(\"Ratio: \", Float)\n\
         active as Bool is Ask(\"Active: \", Bool)\n\
         Sayln name\n\
         Sayln age + 1\n\
         Sayln ratio\n\
         Sayln active\n",
    )
    .expect("failed to write Ask source");

    let mut child = Command::new(env!("CARGO_BIN_EXE_simply"))
        .args([
            "run",
            source.to_str().expect("Ask source path was not UTF-8"),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to start Ask source");
    child
        .stdin
        .take()
        .expect("Ask child should have piped stdin")
        .write_all(b"  Ada  \n42\n2.5\nTRUE\n")
        .expect("failed to provide Ask input");
    let output = child
        .wait_with_output()
        .expect("failed to wait for Ask source");
    fs::remove_dir_all(root).expect("failed to clean up Ask test directory");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "Name: Age: Ratio: Active:   Ada  \n43\n2.5\ntrue\n"
    );
}

#[test]
fn ask_rejects_unsupported_types_and_invalid_typed_input() {
    let (success, _, error) = check_source("value is Ask(\"Value: \", Date)\n");
    assert!(!success);
    assert!(error.contains("unsupported `Ask` type `Date`"), "{error}");

    let (success, _, error) = check_source("value is Ask(\"Value: \", \"Int\")\n");
    assert!(!success);
    assert!(error.contains("Ask` type must be"), "{error}");

    let id = TEMP_SOURCE_ID.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!("simply-ask-invalid-{id}"));
    fs::create_dir_all(&root).expect("failed to create invalid Ask test directory");
    let source = root.join("main.si");
    fs::write(&source, "value is Ask(\"Age: \", Int)\n")
        .expect("failed to write invalid Ask source");
    let mut child = Command::new(env!("CARGO_BIN_EXE_simply"))
        .args([
            "run",
            source.to_str().expect("Ask source path was not UTF-8"),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to start invalid Ask source");
    child
        .stdin
        .take()
        .expect("Ask child should have piped stdin")
        .write_all(b"not an integer\n")
        .expect("failed to provide invalid Ask input");
    let output = child
        .wait_with_output()
        .expect("failed to wait for invalid Ask source");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("expected an Int"), "{stderr}");

    let output = Command::new(env!("CARGO_BIN_EXE_simply"))
        .args([
            "run",
            source.to_str().expect("Ask source path was not UTF-8"),
        ])
        .stdin(Stdio::null())
        .output()
        .expect("failed to execute Ask source without input");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("no input received for `Ask`"), "{stderr}");
    fs::remove_dir_all(root).expect("failed to clean up invalid Ask test directory");
}

#[test]
fn streams_csv_rows_through_where_derive_and_writer() {
    let id = TEMP_SOURCE_ID.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!("simply-csv-{id}"));
    fs::create_dir_all(&root).expect("failed to create CSV test directory");
    let input = root.join("input.csv");
    let output = root.join("output.csv");
    let source = root.join("main.si");
    fs::write(
        &input,
        "name,note\r\nAda,\"hello, world\"\r\nLin,\"skip\nthis\"\r\n",
    )
    .expect("failed to write CSV input");
    fs::write(
        &source,
        format!(
            "cleaned is pipeline:\n    csv_rows(\"{}\")\n    where item[0] == \"Ada\"\n    derive item\n    write_csv(\"{}\")\nend\n",
            input.display(),
            output.display()
        ),
    )
    .expect("failed to write CSV source");

    let binary = env!("CARGO_BIN_EXE_simply");
    let result = Command::new(binary)
        .args([
            "run",
            source.to_str().expect("CSV source path was not UTF-8"),
        ])
        .output()
        .expect("failed to run CSV pipeline");
    assert!(result.status.success());
    assert_eq!(
        fs::read_to_string(&output).expect("failed to read CSV output"),
        "Ada,\"hello, world\"\n"
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn take_short_circuits_csv_before_writing_more_rows() {
    let id = TEMP_SOURCE_ID.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!("simply-csv-take-{id}"));
    fs::create_dir_all(&root).expect("failed to create CSV take test directory");
    let input = root.join("input.csv");
    let output = root.join("output.csv");
    let source = root.join("main.si");
    fs::write(&input, "Ada,keep\nLin,skip\nMira,keep\nbad\"quote,row\n")
        .expect("failed to write CSV input");
    fs::write(
        &source,
        format!(
            "cleaned is pipeline:\n\
                 csv_rows(\"{}\")\n\
                 where item[1] == \"keep\"\n\
                 take 2\n\
                 derive item\n\
                 write_csv(\"{}\")\n\
             end\n",
            input.display(),
            output.display()
        ),
    )
    .expect("failed to write CSV source");

    let result = Command::new(env!("CARGO_BIN_EXE_simply"))
        .args([
            "run",
            source.to_str().expect("CSV source path was not UTF-8"),
        ])
        .output()
        .expect("failed to run CSV take pipeline");
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(
        fs::read_to_string(&output).expect("failed to read CSV output"),
        "Ada,keep\nMira,keep\n"
    );
    fs::remove_dir_all(root).expect("failed to clean up CSV take test directory");
}

#[test]
fn skip_and_take_stream_csv_rows_in_order() {
    let id = TEMP_SOURCE_ID.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!("simply-csv-skip-{id}"));
    fs::create_dir_all(&root).expect("failed to create CSV skip test directory");
    let input = root.join("input.csv");
    let output = root.join("output.csv");
    let source = root.join("main.si");
    fs::write(&input, "Ada,10\nLin,20\nMira,30\nNia,10\n").expect("failed to write CSV input");
    fs::write(
        &source,
        format!(
            "flow selected from csv_rows(\"{}\"):\n\
                 skip 1\n\
                 take 2\n\
                 derive item\n\
                 write_csv(\"{}\")\n\
             end\n",
            input.display(),
            output.display()
        ),
    )
    .expect("failed to write CSV source");

    let result = Command::new(env!("CARGO_BIN_EXE_simply"))
        .args([
            "run",
            source.to_str().expect("CSV source path was not UTF-8"),
        ])
        .output()
        .expect("failed to run CSV skip pipeline");
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(
        fs::read_to_string(&output).expect("failed to read CSV output"),
        "Lin,20\nMira,30\n"
    );
    fs::remove_dir_all(root).expect("failed to clean up CSV skip test directory");
}

#[test]
fn step_by_streams_every_nth_csv_row_to_the_writer() {
    let id = TEMP_SOURCE_ID.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!("simply-csv-step-by-{id}"));
    fs::create_dir_all(&root).expect("failed to create CSV step_by test directory");
    let input = root.join("input.csv");
    let output = root.join("output.csv");
    let source = root.join("main.si");
    fs::write(&input, "Ada,10\nLin,20\nMira,30\nNia,40\nOmar,50\n")
        .expect("failed to write CSV input");
    fs::write(
        &source,
        format!(
            "flow selected from csv_rows(\"{}\"):\n\
                 step_by 2\n\
                 derive item\n\
                 write_csv(\"{}\")\n\
             end\n",
            input.display(),
            output.display()
        ),
    )
    .expect("failed to write CSV source");

    let result = Command::new(env!("CARGO_BIN_EXE_simply"))
        .args([
            "run",
            source.to_str().expect("CSV source path was not UTF-8"),
        ])
        .output()
        .expect("failed to run CSV step_by pipeline");
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(
        fs::read_to_string(&output).expect("failed to read CSV output"),
        "Ada,10\nMira,30\nOmar,50\n"
    );
    fs::remove_dir_all(root).expect("failed to clean up CSV step_by test directory");
}

#[test]
fn take_while_stops_reading_csv_after_the_first_nonmatching_row() {
    let id = TEMP_SOURCE_ID.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!("simply-csv-take-while-{id}"));
    fs::create_dir_all(&root).expect("failed to create CSV take_while test directory");
    let input = root.join("input.csv");
    let output = root.join("output.csv");
    let source = root.join("main.si");
    fs::write(&input, "Ada,10\nLin,20\nMira,30\nbad\"quote,row\n")
        .expect("failed to write CSV input");
    fs::write(
        &source,
        format!(
            "flow selected from csv_rows(\"{}\"):\n\
                 take_while to_int(item[1]) < 30\n\
                 derive item\n\
                 write_csv(\"{}\")\n\
             end\n",
            input.display(),
            output.display()
        ),
    )
    .expect("failed to write CSV source");

    let result = Command::new(env!("CARGO_BIN_EXE_simply"))
        .args([
            "run",
            source.to_str().expect("CSV source path was not UTF-8"),
        ])
        .output()
        .expect("failed to run CSV take_while pipeline");
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(
        fs::read_to_string(&output).expect("failed to read CSV output"),
        "Ada,10\nLin,20\n"
    );
    fs::remove_dir_all(root).expect("failed to clean up CSV take_while test directory");
}

#[test]
fn any_terminal_stops_reading_csv_after_the_first_true_item() {
    let id = TEMP_SOURCE_ID.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!("simply-csv-any-{id}"));
    fs::create_dir_all(&root).expect("failed to create CSV any test directory");
    let input = root.join("input.csv");
    let source = root.join("main.si");
    fs::write(&input, "true\nbad\"quote,row\n").expect("failed to write CSV input");
    fs::write(
        &source,
        format!(
            "result is pipeline:\n\
                 csv_rows(\"{}\")\n\
                 derive item[0] == \"true\"\n\
                 any\n\
             end\n\
             Sayln result\n",
            input.display()
        ),
    )
    .expect("failed to write CSV source");

    let result = Command::new(env!("CARGO_BIN_EXE_simply"))
        .args([
            "run",
            source.to_str().expect("CSV source path was not UTF-8"),
        ])
        .output()
        .expect("failed to run CSV any pipeline");
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&result.stdout).trim(), "true");
    fs::remove_dir_all(root).expect("failed to clean up CSV any test directory");
}

#[test]
fn drop_while_skips_only_the_csv_prefix_that_matches() {
    let id = TEMP_SOURCE_ID.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!("simply-csv-drop-while-{id}"));
    fs::create_dir_all(&root).expect("failed to create CSV drop_while test directory");
    let input = root.join("input.csv");
    let output = root.join("output.csv");
    let source = root.join("main.si");
    fs::write(&input, "Ada,10\nLin,20\nMira,30\nNia,10\n").expect("failed to write CSV input");
    fs::write(
        &source,
        format!(
            "flow remaining_rows from csv_rows(\"{}\"):\n\
                 drop_while to_int(item[1]) < 30\n\
                 derive item\n\
                 write_csv(\"{}\")\n\
             end\n",
            input.display(),
            output.display()
        ),
    )
    .expect("failed to write CSV source");

    let result = Command::new(env!("CARGO_BIN_EXE_simply"))
        .args([
            "run",
            source.to_str().expect("CSV source path was not UTF-8"),
        ])
        .output()
        .expect("failed to run CSV drop_while pipeline");
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(
        fs::read_to_string(&output).expect("failed to read CSV output"),
        "Mira,30\nNia,10\n"
    );
    fs::remove_dir_all(root).expect("failed to clean up CSV drop_while test directory");
}

#[test]
fn distinct_keeps_first_duplicate_csv_row_in_output_order() {
    let id = TEMP_SOURCE_ID.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!("simply-csv-distinct-{id}"));
    fs::create_dir_all(&root).expect("failed to create CSV distinct test directory");
    let input = root.join("input.csv");
    let output = root.join("output.csv");
    let source = root.join("main.si");
    fs::write(&input, "Ada,10\nLin,20\nAda,10\nMira,30\nLin,20\n")
        .expect("failed to write CSV input");
    fs::write(
        &source,
        format!(
            "flow unique_rows from csv_rows(\"{}\"):\n\
                 distinct\n\
                 derive item\n\
                 write_csv(\"{}\")\n\
             end\n",
            input.display(),
            output.display()
        ),
    )
    .expect("failed to write CSV source");

    let result = Command::new(env!("CARGO_BIN_EXE_simply"))
        .args([
            "run",
            source.to_str().expect("CSV source path was not UTF-8"),
        ])
        .output()
        .expect("failed to run CSV distinct pipeline");
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(
        fs::read_to_string(&output).expect("failed to read CSV output"),
        "Ada,10\nLin,20\nMira,30\n"
    );
    fs::remove_dir_all(root).expect("failed to clean up CSV distinct test directory");
}

#[test]
fn partitions_csv_rows_without_materializing_them() {
    let id = TEMP_SOURCE_ID.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!("simply-csv-progress-{id}"));
    fs::create_dir_all(&root).expect("failed to create CSV progress directory");
    let input = root.join("input.csv");
    let source = root.join("main.si");
    fs::write(&input, "Ada,10\nLin,3\n").expect("failed to write CSV input");
    fs::write(
        &source,
        format!(
            "metrics is pipeline:\n    csv_rows(\"{}\")\n    partition item:\n        to_int(item[1]) >= 5 -> kept\n        otherwise -> dropped\n    end\nend\nSayln metrics\n",
            input.display()
        ),
    )
    .expect("failed to write CSV source");

    let output = Command::new(env!("CARGO_BIN_EXE_simply"))
        .args([
            "run",
            source.to_str().expect("CSV source path was not UTF-8"),
        ])
        .output()
        .expect("failed to run CSV progress pipeline");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("kept: [[Ada, 10]]"), "{stdout}");
    assert!(stdout.contains("dropped: [[Lin, 3]]"), "{stdout}");
    let _ = fs::remove_dir_all(root);
}

#[test]
fn runtime_accepts_unit_range_and_csv_stream_declared_types() {
    let id = TEMP_SOURCE_ID.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!("simply-declared-runtime-types-{id}"));
    fs::create_dir_all(&root).expect("failed to create declared type test directory");
    let input = root.join("input.csv");
    let source = root.join("main.si");
    fs::write(&input, "Ada\nLin\n").expect("failed to write typed CSV input");
    fs::write(
        &source,
        format!(
            "fn no_op() gives Unit:\n\
                 return print(\"unit\")\n\
             end\n\
             no_op()\n\
             values as Range is range(1, 3)\n\
             rows as CsvStream is csv_rows(\"{}\")\n\
             flow total from rows:\n\
                 count\n\
             end\n\
             Sayln values\n\
             Sayln total\n",
            input.display()
        ),
    )
    .expect("failed to write typed declaration source");

    let output = Command::new(env!("CARGO_BIN_EXE_simply"))
        .args(["run", source.to_str().expect("source path must be UTF-8")])
        .output()
        .expect("failed to execute typed declaration source");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .collect::<Vec<_>>(),
        ["unit", "[1, 2]", "2"]
    );
    fs::remove_dir_all(root).expect("failed to clean up declared type test directory");
}

#[test]
fn csv_parser_rejects_malformed_quote_placement() {
    let id = TEMP_SOURCE_ID.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!("simply-csv-invalid-quote-{id}"));
    fs::create_dir_all(&root).expect("failed to create malformed CSV test directory");
    let input = root.join("input.csv");
    let source = root.join("main.si");
    fs::write(&input, "Ada\"oops,10\n").expect("failed to write malformed CSV");
    fs::write(
        &source,
        format!(
            "result is pipeline:\n    csv_rows(\"{}\")\n    count\nend\n",
            input.display()
        ),
    )
    .expect("failed to write malformed CSV source");

    let output = Command::new(env!("CARGO_BIN_EXE_simply"))
        .args(["run", source.to_str().expect("source path must be UTF-8")])
        .output()
        .expect("failed to execute malformed CSV source");
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("quote inside an unquoted field"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    fs::remove_dir_all(root).expect("failed to clean up malformed CSV test directory");
}

#[test]
fn csv_pipeline_rejects_input_output_and_checkpoint_path_collisions() {
    let id = TEMP_SOURCE_ID.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!("simply-csv-path-collision-{id}"));
    fs::create_dir_all(&root).expect("failed to create CSV collision test directory");
    let input = root.join("input.csv");
    let output = root.join("output.csv");
    let checkpoint = root.join("checkpoint.state");
    let checkpoint_temporary_output = root.join("checkpoint.state.tmp");
    let source = root.join("main.si");
    fs::write(&input, "preserve input\n").expect("failed to write collision input");
    fs::write(&output, "preserve output\n").expect("failed to seed collision output");

    for (checkpoint_path, output_path, expected) in [
        (None, input.as_path(), "input and `write_csv` output"),
        (
            Some(output.as_path()),
            output.as_path(),
            "`checkpoint` and its temporary file",
        ),
        (
            Some(checkpoint.as_path()),
            checkpoint_temporary_output.as_path(),
            "`checkpoint` and its temporary file",
        ),
    ] {
        let checkpoint_step = checkpoint_path
            .map(|path| format!("    checkpoint \"{}\"\n", path.display()))
            .unwrap_or_default();
        fs::write(
            &source,
            format!(
                "flow result from csv_rows(\"{}\"):\n\
                     {checkpoint_step}\
                     derive item\n\
                     write_csv(\"{}\")\n\
                 end\n",
                input.display(),
                output_path.display()
            ),
        )
        .expect("failed to write collision source");
        let result = Command::new(env!("CARGO_BIN_EXE_simply"))
            .args(["run", source.to_str().expect("source path must be UTF-8")])
            .output()
            .expect("failed to execute collision source");
        assert!(!result.status.success());
        assert!(
            String::from_utf8_lossy(&result.stderr).contains(expected),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert_eq!(
            fs::read_to_string(&input).expect("failed to read collision input"),
            "preserve input\n"
        );
        if output_path == output.as_path() {
            assert_eq!(
                fs::read_to_string(&output).expect("failed to read collision output"),
                "preserve output\n"
            );
        }
    }
    fs::remove_dir_all(root).expect("failed to clean up CSV collision test directory");
}

#[test]
fn reads_and_overwrites_text_files() {
    let id = TEMP_SOURCE_ID.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!("simply-file-api-{id}"));
    fs::create_dir_all(&root).expect("failed to create file API test directory");
    let input = root.join("input.txt");
    let output = root.join("output.txt");
    let source = root.join("main.si");
    fs::write(&input, "existing source\n").expect("failed to write file API input");
    fs::write(
        &source,
        format!(
            "Sayln read_file(\"{}\")\n\
             write_file(\"{}\", \"first\")\n\
             write_file(\"{}\", \"overwritten\")\n\
             Sayln read_file(\"{}\")\n",
            input.display(),
            output.display(),
            output.display(),
            output.display()
        ),
    )
    .expect("failed to write file API source");

    let result = Command::new(env!("CARGO_BIN_EXE_simply"))
        .args(["run", source.to_str().expect("source path must be UTF-8")])
        .output()
        .expect("failed to execute file API source");
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&result.stdout),
        "existing source\n\noverwritten\n"
    );
    assert_eq!(
        fs::read_to_string(&output).expect("failed to read overwritten output"),
        "overwritten"
    );
    fs::remove_dir_all(root).expect("failed to clean up file API test directory");
}

#[test]
fn file_io_failures_return_runtime_diagnostics() {
    let id = TEMP_SOURCE_ID.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!("simply-file-api-errors-{id}"));
    fs::create_dir_all(&root).expect("failed to create file API error directory");
    let missing = root.join("missing.txt");
    let absent_parent = root.join("missing-parent").join("output.txt");
    let directory = root.join("directory");
    let invalid_utf8 = root.join("invalid-utf8.txt");
    fs::create_dir(&directory).expect("failed to create directory for write error test");
    fs::write(&invalid_utf8, [0xff]).expect("failed to create invalid UTF-8 input");

    for (source, expected) in [
        (
            format!("Sayln read_file(\"{}\")\n", missing.display()),
            "could not read file",
        ),
        (
            format!("Sayln read_file(\"{}\")\n", invalid_utf8.display()),
            "could not read file",
        ),
        (
            format!("write_file(\"{}\", \"content\")\n", absent_parent.display()),
            "could not write file",
        ),
        (
            format!("write_file(\"{}\", \"content\")\n", directory.display()),
            "could not write file",
        ),
    ] {
        let (success, error) = run_source(&source);
        assert!(!success, "{source}");
        assert!(
            error.contains("error[E.runtime.io.operation-failed]"),
            "{error}"
        );
        assert!(error.contains(expected), "{error}");
    }

    let source = format!(
        "try:\n\
             Sayln read_file(\"{}\")\n\
         catch failure as E.runtime.io.operation-failed:\n\
             Sayln failure.code\n\
         end\n",
        missing.display()
    );
    let (success, output) = run_source_stdout(&source);
    assert!(success, "{output}");
    assert_eq!(output, "E.runtime.io.operation-failed\n");

    let (success, _, error) = check_source("Sayln read_file(10)\n");
    assert!(!success);
    assert!(error.contains("expected String, found Int"), "{error}");
    let (success, _, error) = check_source("write_file(\"out.txt\", 10)\n");
    assert!(!success);
    assert!(error.contains("expected String, found Int"), "{error}");

    fs::remove_dir_all(root).expect("failed to clean up file API error directory");
}

#[test]
fn imports_a_returned_value_relative_to_the_source_file() {
    let directory = std::env::temp_dir().join(format!("simply-import-{}", std::process::id()));
    fs::create_dir_all(&directory).expect("failed to create import directory");
    let imported = directory.join("values.si");
    let main = directory.join("main.si");
    fs::write(&imported, "return list [4, 8, 15]\n").expect("failed to write imported source");
    fs::write(&main, "open \"values.si\" as values\nSayln values[1]\n")
        .expect("failed to write importing source");

    let output = Command::new(env!("CARGO_BIN_EXE_simply"))
        .args(["run", main.to_str().expect("temporary path was not UTF-8")])
        .output()
        .expect("failed to run importing source");
    fs::remove_dir_all(&directory).expect("failed to remove import directory");

    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "8");
}

#[test]
fn imported_modules_and_checker_reject_forward_function_calls_consistently() {
    let id = TEMP_SOURCE_ID.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!("simply-module-function-order-{id}"));
    fs::create_dir_all(&root).expect("failed to create module function-order directory");
    let module = root.join("module.si");
    let main = root.join("main.si");
    fs::write(
        &module,
        "Sayln first()\n\
         fn first() gives Int:\n\
             return 1\n\
         end\n\
         return 0\n",
    )
    .expect("failed to write module function-order fixture");
    fs::write(&main, "open \"module.si\" as imported\nSayln imported\n")
        .expect("failed to write function-order entrypoint");

    let checked = Command::new(env!("CARGO_BIN_EXE_simply"))
        .args(["check", main.to_str().expect("path was not UTF-8")])
        .output()
        .expect("failed to check module function-order fixture");
    assert!(!checked.status.success());
    assert!(
        String::from_utf8_lossy(&checked.stderr).contains("function `first` is declared later"),
        "{}",
        String::from_utf8_lossy(&checked.stderr)
    );

    let executed = Command::new(env!("CARGO_BIN_EXE_simply"))
        .args(["run", main.to_str().expect("path was not UTF-8")])
        .output()
        .expect("failed to run module function-order fixture");
    assert!(!executed.status.success());
    assert!(
        String::from_utf8_lossy(&executed.stderr).contains("unknown function `first`"),
        "{}",
        String::from_utf8_lossy(&executed.stderr)
    );
    fs::remove_dir_all(root).expect("failed to clean module function-order directory");
}

#[test]
fn imported_function_values_run_and_are_checked_against_their_signature() {
    let id = TEMP_SOURCE_ID.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!("simply-import-function-{id}"));
    fs::create_dir_all(root.join("nested")).expect("failed to create function import directory");
    fs::write(
        root.join("math.si"),
        "fn add(left as Int, right as Int) gives Int:\n\
             return left + right\n\
         end\n\
         return add\n",
    )
    .expect("failed to write imported function module");
    let main = root.join("main.si");
    fs::write(
        &main,
        "open \"math.si\" as add\n\
         open \"nested/../math.si\" as add_again\n\
         Sayln add(2, 3)\n\
         Sayln add_again(4, 5)\n",
    )
    .expect("failed to write function importer");

    let output = Command::new(env!("CARGO_BIN_EXE_simply"))
        .args(["run", main.to_str().expect("test path was not UTF-8")])
        .output()
        .expect("failed to run imported function");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "5\n9");

    fs::write(&main, "open \"math.si\" as add\nadd(\"wrong\", 3)\n")
        .expect("failed to write invalid function importer");
    let checked = Command::new(env!("CARGO_BIN_EXE_simply"))
        .args(["check", main.to_str().expect("test path was not UTF-8")])
        .output()
        .expect("failed to check imported function arguments");
    assert!(!checked.status.success());
    assert!(
        String::from_utf8_lossy(&checked.stderr).contains("expected Int, found String"),
        "{}",
        String::from_utf8_lossy(&checked.stderr)
    );
    fs::write(&main, "open \"math.si\" as add\nadd(2)\n")
        .expect("failed to write invalid-arity function importer");
    let checked = Command::new(env!("CARGO_BIN_EXE_simply"))
        .args(["check", main.to_str().expect("test path was not UTF-8")])
        .output()
        .expect("failed to check imported function arity");
    assert!(!checked.status.success());
    assert!(
        String::from_utf8_lossy(&checked.stderr).contains("expects 2 arguments, got 1"),
        "{}",
        String::from_utf8_lossy(&checked.stderr)
    );

    fs::remove_dir_all(root).expect("failed to remove function import directory");
}

#[test]
fn imported_function_values_capture_module_bindings() {
    let id = TEMP_SOURCE_ID.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!("simply-import-function-capture-{id}"));
    fs::create_dir_all(&root).expect("failed to create function capture directory");
    fs::write(
        root.join("module.si"),
        "base is 40\n\
         fn add(value as Int) gives Int:\n\
             return base + value\n\
         end\n\
         return add\n",
    )
    .expect("failed to write closure module");
    let main = root.join("main.si");
    fs::write(&main, "open \"module.si\" as add\nSayln add(2)\n")
        .expect("failed to write closure importer");

    let checked = Command::new(env!("CARGO_BIN_EXE_simply"))
        .args(["check", main.to_str().expect("test path was not UTF-8")])
        .output()
        .expect("failed to check imported closure");
    assert!(
        checked.status.success(),
        "{}",
        String::from_utf8_lossy(&checked.stderr)
    );

    let output = Command::new(env!("CARGO_BIN_EXE_simply"))
        .args(["run", main.to_str().expect("test path was not UTF-8")])
        .output()
        .expect("failed to run imported closure");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "42");

    fs::remove_dir_all(root).expect("failed to remove function capture directory");
}

#[test]
fn returned_recursive_module_functions_keep_their_definition_scope() {
    let id = TEMP_SOURCE_ID.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!("simply-import-recursive-function-{id}"));
    fs::create_dir_all(&root).expect("failed to create recursive function directory");
    fs::write(
        root.join("module.si"),
        "fn countdown(value as Int) gives Int:\n\
             if value == 0:\n\
                 return 0\n\
             else:\n\
                 return countdown(value - 1)\n\
             end\n\
         end\n\
         return countdown\n",
    )
    .expect("failed to write recursive function module");
    let main = root.join("main.si");
    fs::write(
        &main,
        "open \"module.si\" as run_countdown\nSayln run_countdown(3)\n",
    )
    .expect("failed to write recursive function importer");

    let checked = Command::new(env!("CARGO_BIN_EXE_simply"))
        .args(["check", main.to_str().expect("test path was not UTF-8")])
        .output()
        .expect("failed to check imported recursive function");
    assert!(
        checked.status.success(),
        "{}",
        String::from_utf8_lossy(&checked.stderr)
    );

    let output = Command::new(env!("CARGO_BIN_EXE_simply"))
        .args(["run", main.to_str().expect("test path was not UTF-8")])
        .output()
        .expect("failed to run imported recursive function");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "0");

    fs::remove_dir_all(root).expect("failed to remove recursive function directory");
}

#[test]
fn check_requires_a_module_return_outside_a_discarded_match_expression() {
    let id = TEMP_SOURCE_ID.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!("simply-check-match-module-return-{id}"));
    fs::create_dir_all(&root).expect("failed to create match module directory");
    fs::write(
        root.join("module.si"),
        "match true:\n\
             true:\n\
                 return 1\n\
             false:\n\
                 return 2\n\
         end\n",
    )
    .expect("failed to write match module");
    let main = root.join("main.si");
    fs::write(&main, "open \"module.si\" as value\n")
        .expect("failed to write match module importer");

    let checked = Command::new(env!("CARGO_BIN_EXE_simply"))
        .args(["check", main.to_str().expect("test path was not UTF-8")])
        .output()
        .expect("failed to check match module");
    assert!(!checked.status.success());
    assert!(
        String::from_utf8_lossy(&checked.stderr).contains("imported module must return a value"),
        "{}",
        String::from_utf8_lossy(&checked.stderr)
    );

    let output = Command::new(env!("CARGO_BIN_EXE_simply"))
        .args(["run", main.to_str().expect("test path was not UTF-8")])
        .output()
        .expect("failed to run match module");
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("must return a value"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    fs::remove_dir_all(root).expect("failed to remove match module directory");
}

#[test]
fn imported_struct_and_enum_values_keep_their_runtime_type_identity() {
    let id = TEMP_SOURCE_ID.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!("simply-import-types-{id}"));
    fs::create_dir_all(&root).expect("failed to create typed import directory");
    fs::write(
        root.join("person.si"),
        "type Person:\n\
             name as String\n\
         end\n\
         return Person(\"Ada\")\n",
    )
    .expect("failed to write struct module");
    fs::write(
        root.join("state.si"),
        "enum State:\n\
             Ready\n\
         end\n\
         return State::Ready\n",
    )
    .expect("failed to write enum module");
    let main = root.join("main.si");
    fs::write(
        &main,
        "open \"person.si\" as person\n\
         open \"state.si\" as state\n\
         Sayln type_of(person)\n\
         Sayln type_of(state)\n",
    )
    .expect("failed to write typed importer");

    let checked = Command::new(env!("CARGO_BIN_EXE_simply"))
        .args(["check", main.to_str().expect("test path was not UTF-8")])
        .output()
        .expect("failed to check typed imports");
    assert!(
        checked.status.success(),
        "{}",
        String::from_utf8_lossy(&checked.stderr)
    );

    let output = Command::new(env!("CARGO_BIN_EXE_simply"))
        .args(["run", main.to_str().expect("test path was not UTF-8")])
        .output()
        .expect("failed to run typed imports");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "Person\nState"
    );

    fs::remove_dir_all(root).expect("failed to remove typed import directory");
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
            "open \"nested/middle.si\" as nested\nopen \"{absolute_leaf}\" as absolute\nSayln nested[0]\nSayln absolute[0]\n"
        ),
    )
    .expect("failed to write main source");

    let output = Command::new(env!("CARGO_BIN_EXE_simply"))
        .args(["run", main.to_str().expect("temporary path was not UTF-8")])
        .current_dir(&nested)
        .output()
        .expect("failed to run nested import source");
    let _ = fs::remove_dir_all(root);

    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "11\n11");
    assert!(output.stderr.is_empty());
}

#[test]
fn canonical_import_paths_reuse_parsing_but_execute_each_import() {
    let id = TEMP_SOURCE_ID.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!("simply-import-identity-{id}"));
    fs::create_dir_all(root.join("nested")).expect("failed to create import identity directory");
    fs::write(
        root.join("value.si"),
        "value is Ask(\"value\", Int)\nreturn value\n",
    )
    .expect("failed to write interactive imported module");
    let main = root.join("main.si");
    fs::write(
        &main,
        "open \"value.si\" as first\n\
         open \"nested/../value.si\" as second\n\
         Sayln first + second\n",
    )
    .expect("failed to write canonical-path importer");

    let mut child = Command::new(env!("CARGO_BIN_EXE_simply"))
        .args(["run", main.to_str().expect("test path was not UTF-8")])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to start canonical-path importer");
    child
        .stdin
        .take()
        .expect("import stdin should be piped")
        .write_all(b"1\n2\n")
        .expect("failed to provide import input");
    let output = child
        .wait_with_output()
        .expect("failed to run canonical-path importer");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout)
            .trim()
            .ends_with('3'),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );

    fs::remove_dir_all(root).expect("failed to remove import identity directory");
}

#[test]
fn directory_and_malformed_import_paths_fail_with_runtime_diagnostics() {
    let id = TEMP_SOURCE_ID.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!("simply-invalid-import-path-{id}"));
    fs::create_dir_all(root.join("folder")).expect("failed to create import path directory");
    let main = root.join("main.si");

    for (source, runtime_error, check_error) in [
        (
            "open \"folder\" as value\n",
            "could not open",
            "could not read",
        ),
        (
            "open \"bad\0path.si\" as value\n",
            "could not open",
            "could not open",
        ),
    ] {
        fs::write(&main, source).expect("failed to write invalid import source");
        let output = Command::new(env!("CARGO_BIN_EXE_simply"))
            .args(["run", main.to_str().expect("test path was not UTF-8")])
            .output()
            .expect("failed to run invalid import source");
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(!output.status.success());
        assert!(error.contains(runtime_error), "{error}");
        assert!(error.contains("Runtime error"), "{error}");

        let checked = Command::new(env!("CARGO_BIN_EXE_simply"))
            .args(["check", main.to_str().expect("test path was not UTF-8")])
            .output()
            .expect("failed to check invalid import source");
        let error = String::from_utf8_lossy(&checked.stderr);
        assert!(!checked.status.success());
        assert!(error.contains(check_error), "{error}");
    }

    fs::remove_dir_all(root).expect("failed to remove invalid import directory");
}

#[test]
fn reports_missing_imports_with_structured_diagnostics() {
    let root = std::env::temp_dir().join(format!("simply-missing-import-{}", std::process::id()));
    let main = root.join("main.si");
    fs::create_dir_all(&root).expect("failed to create missing import directory");
    fs::write(&main, "open \"missing.si\" as missing\n")
        .expect("failed to write missing import source");

    let output = Command::new(env!("CARGO_BIN_EXE_simply"))
        .args(["run", main.to_str().expect("temporary path was not UTF-8")])
        .output()
        .expect("failed to run missing import source");
    let _ = fs::remove_dir_all(root);

    let error = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success());
    assert!(error.contains("Runtime error"));
    assert!(error.contains("error[E.runtime.module.import]"));
    assert!(error.contains("1 | open \"missing.si\" as missing"));
}

#[test]
fn reports_circular_imports_without_recursing_forever() {
    let id = TEMP_SOURCE_ID.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!("simply-circular-import-{id}"));
    let first = root.join("a.si");
    let second = root.join("b.si");
    let third = root.join("c.si");
    fs::create_dir_all(&root).expect("failed to create circular import directory");
    fs::write(&first, "open \"b.si\" as b\nreturn b\n")
        .expect("failed to write first circular source");
    fs::write(&second, "open \"c.si\" as c\nreturn c\n")
        .expect("failed to write second circular source");
    fs::write(&third, "open \"a.si\" as a\nreturn a\n")
        .expect("failed to write third circular source");

    let output = Command::new(env!("CARGO_BIN_EXE_simply"))
        .args(["run", first.to_str().expect("temporary path was not UTF-8")])
        .output()
        .expect("failed to run circular import source");
    let _ = fs::remove_dir_all(root);

    let error = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success());
    assert!(error.contains("cyclic import"));
    assert!(error.contains("a.si") && error.contains("b.si") && error.contains("c.si"));
    assert!(error.contains("error[E.runtime.module.import]"));
}

#[cfg(unix)]
#[test]
fn symlinked_imports_share_canonical_cycle_identity() {
    use std::os::unix::fs::symlink;

    let id = TEMP_SOURCE_ID.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!("simply-symlink-import-cycle-{id}"));
    fs::create_dir_all(&root).expect("failed to create symlink import directory");
    let module = root.join("module.si");
    let alias = root.join("alias.si");
    fs::write(&module, "open \"alias.si\" as again\nreturn 1\n")
        .expect("failed to write symlink cycle source");
    symlink(&module, &alias).expect("failed to create module symlink");

    let output = Command::new(env!("CARGO_BIN_EXE_simply"))
        .args(["run", module.to_str().expect("test path was not UTF-8")])
        .output()
        .expect("failed to run symlink import cycle");
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success());
    assert!(error.contains("cyclic import"), "{error}");
    assert!(error.contains("module.si"), "{error}");

    fs::remove_dir_all(root).expect("failed to remove symlink import directory");
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
    fs::write(&main, "open \"values.si\" as values\nSayln values[1]\n")
        .expect("failed to write main source");

    let output = Command::new(env!("CARGO_BIN_EXE_simply"))
        .args(["run", main.to_str().expect("temporary path was not UTF-8")])
        .current_dir(&unrelated_dir)
        .output()
        .expect("failed to run source from unrelated directory");
    let _ = fs::remove_dir_all(root);

    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "8");
    assert!(output.stderr.is_empty());
}
