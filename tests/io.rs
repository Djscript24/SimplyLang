mod common;
use common::*;
use std::{fs, process::Command, sync::atomic::Ordering};

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
            "metrics is pipeline:\n    csv_rows(\"{}\")\n    partition item:\n        to_int(item[1]) >= 5 -> kept\n        otherwise -> dropped\n    end\nend\nSay metrics\n",
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
             values as Array[Int] is range(1, 3)\n\
             rows as List[List[String]] is csv_rows(\"{}\")\n\
             flow total from rows:\n\
                 count\n\
             end\n\
             Say values\n\
             Say total\n",
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
fn imports_a_returned_value_relative_to_the_source_file() {
    let directory = std::env::temp_dir().join(format!("simply-import-{}", std::process::id()));
    fs::create_dir_all(&directory).expect("failed to create import directory");
    let imported = directory.join("values.si");
    let main = directory.join("main.si");
    fs::write(&imported, "return list [4, 8, 15]\n").expect("failed to write imported source");
    fs::write(&main, "open \"values.si\" as values\nSay values[1]\n")
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
            "open \"nested/middle.si\" as nested\nopen \"{absolute_leaf}\" as absolute\nSay nested[0]\nSay absolute[0]\n"
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
    assert!(error.contains("error[E0204]"));
    assert!(error.contains("1 | open \"missing.si\" as missing"));
}

#[test]
fn reports_circular_imports_without_recursing_forever() {
    let root = std::env::temp_dir().join(format!("simply-circular-import-{}", std::process::id()));
    let first = root.join("a.si");
    let second = root.join("b.si");
    fs::create_dir_all(&root).expect("failed to create circular import directory");
    fs::write(&first, "open \"b.si\" as b\nreturn b\n")
        .expect("failed to write first circular source");
    fs::write(&second, "open \"a.si\" as a\nreturn a\n")
        .expect("failed to write second circular source");

    let output = Command::new(env!("CARGO_BIN_EXE_simply"))
        .args(["run", first.to_str().expect("temporary path was not UTF-8")])
        .output()
        .expect("failed to run circular import source");
    let _ = fs::remove_dir_all(root);

    let error = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success());
    assert!(error.contains("cyclic import"));
    assert!(error.contains("error[E0204]"));
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
    fs::write(&main, "open \"values.si\" as values\nSay values[1]\n")
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
