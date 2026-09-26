mod common;
use common::*;
use std::{fs, sync::atomic::Ordering};

#[test]
fn declarative_flow_lowers_to_pipeline() {
    let (success, output) = run_source_stdout(
        "values is list [1, 2, 3, 4]\n\
         flow total from values:\n\
             where item >= 2\n\
             derive item * 2\n\
             sum\n\
         end\n\
         Sayln total\n",
    );
    assert!(success, "{output}");
    assert!(output.contains("18"), "{output}");
}

#[test]
fn explain_flow_prints_deterministic_execution_plan() {
    let (success, output) = explain_flow_source(
        "values is list [1, 2, 3]\n\
         flow total from values:\n\
             where item > 1\n\
             derive item * 2\n\
             sum\n\
         end\n",
    );
    assert!(success, "{output}");
    assert!(output.contains("program kind: flow"), "{output}");
    assert!(output.contains("source kind: bound collection"), "{output}");
    assert!(
        output.contains("operators: where -> derive -> sum"),
        "{output}"
    );
    assert!(output.contains("terminal: sum"), "{output}");
}

#[test]
fn explain_flow_identifies_non_flow_programs() {
    let (success, output) = explain_flow_source("Sayln 42\n");
    assert!(success, "{output}");
    assert!(output.contains("program kind: non-flow"), "{output}");
    assert!(output.contains("flows: none"), "{output}");
}

#[test]
fn flow_chunk_and_checkpoint_preserve_results() {
    let id = TEMP_SOURCE_ID.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!("simply-chunk-checkpoint-{id}"));
    fs::create_dir_all(&root).expect("failed to create chunk/checkpoint test directory");
    let input = root.join("input.csv");
    let checkpoint = root.join("progress.state");
    let output = root.join("output.csv");
    fs::write(&input, "1\n2\n3\n").expect("failed to write checkpoint input");
    let source = format!(
        "flow result from csv_rows(\"{}\"):\n\
             chunk 2\n\
             checkpoint \"{}\"\n\
             derive list [to_int(item[0])]\n\
             write_csv(\"{}\")\n\
         end\n",
        input.display(),
        checkpoint.display(),
        output.display()
    );
    let (success, error) = run_source(&source);
    assert!(success, "{error}");
    assert_eq!(
        fs::read_to_string(&output).expect("failed to read chunked output"),
        "1\n2\n3\n"
    );
    assert!(!checkpoint.exists());
    fs::remove_dir_all(root).expect("failed to clean up chunk/checkpoint test directory");
}

#[test]
fn checkpoint_resume_does_not_duplicate_rows_after_filtered_items() {
    let id = TEMP_SOURCE_ID.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!("simply-checkpoint-resume-{id}"));
    fs::create_dir_all(&root).expect("failed to create checkpoint test directory");
    let input = root.join("input.csv");
    let checkpoint = root.join("progress.state");
    let output = root.join("output.csv");
    let source = || {
        format!(
            "flow result from csv_rows(\"{}\"):\n\
                 chunk 2\n\
                 checkpoint \"{}\"\n\
                 where to_int(item[0]) > 0 or 10 / to_int(item[0]) > 0\n\
                 write_csv(\"{}\")\n\
             end\n",
            input.display(),
            checkpoint.display(),
            output.display()
        )
    };

    fs::write(&input, "1\n2\n3\n-1\n0\n").expect("failed to write initial checkpoint input");
    let (success, error) = run_source(&source());
    assert!(!success, "the first run should fail on division by zero");
    assert!(error.contains("divided by zero"), "{error}");
    assert_eq!(
        fs::read_to_string(&output).expect("failed to read partial CSV output"),
        "1\n2\n3\n"
    );
    assert_eq!(
        fs::read_to_string(&checkpoint)
            .expect("failed to read checkpoint")
            .lines()
            .next(),
        Some("4")
    );

    let (success, error) = run_source(&source());
    assert!(!success, "the unchanged failing input should fail again");
    assert!(error.contains("divided by zero"), "{error}");
    assert_eq!(
        fs::read_to_string(&output).expect("failed to read resumed CSV output"),
        "1\n2\n3\n"
    );

    fs::remove_dir_all(root).expect("failed to clean up checkpoint test directory");
}

#[test]
fn checkpoint_resume_discards_output_after_the_last_committed_batch() {
    let id = TEMP_SOURCE_ID.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!("simply-checkpoint-rollback-{id}"));
    fs::create_dir_all(&root).expect("failed to create checkpoint rollback test directory");
    let input = root.join("input.csv");
    let checkpoint = root.join("progress.state");
    let output = root.join("output.csv");
    let source = || {
        format!(
            "flow result from csv_rows(\"{}\"):\n\
                 chunk 2\n\
                 checkpoint \"{}\"\n\
                 derive list [to_int(item[0])]\n\
                 write_csv(\"{}\")\n\
             end\n",
            input.display(),
            checkpoint.display(),
            output.display()
        )
    };

    fs::write(&input, "1\n2\n3\ninvalid\n5\n").expect("failed to write checkpoint input");
    let (success, error) = run_source(&source());
    assert!(!success);
    assert!(error.contains("cannot convert"), "{error}");
    assert_eq!(
        fs::read_to_string(&output).expect("failed to read partial output"),
        "1\n2\n3\n"
    );
    assert_eq!(
        fs::read_to_string(&checkpoint)
            .expect("failed to read checkpoint")
            .lines()
            .next(),
        Some("2")
    );

    let (success, error) = run_source(&source());
    assert!(!success, "the unchanged failing input should fail again");
    assert!(error.contains("cannot convert"), "{error}");
    assert_eq!(
        fs::read_to_string(&output).expect("failed to read resumed output"),
        "1\n2\n3\n"
    );
    fs::remove_dir_all(root).expect("failed to clean up checkpoint rollback test directory");
}

#[test]
fn csv_checkpoint_resume_does_not_duplicate_rows_after_filtered_items() {
    let id = TEMP_SOURCE_ID.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!("simply-csv-checkpoint-resume-{id}"));
    fs::create_dir_all(&root).expect("failed to create CSV checkpoint test directory");
    let input = root.join("input.csv");
    let checkpoint = root.join("progress.state");
    let output = root.join("output.csv");
    let source = format!(
        "flow result from csv_rows(\"{}\"):\n\
             chunk 2\n\
             checkpoint \"{}\"\n\
             where to_int(item[0]) > 0 or 10 / to_int(item[0]) > 0\n\
             write_csv(\"{}\")\n\
         end\n",
        input.display(),
        checkpoint.display(),
        output.display()
    );

    fs::write(&input, "1\n2\n3\n-1\n0\n").expect("failed to write initial CSV input");
    let (success, error) = run_source(&source);
    assert!(!success, "the first run should fail on division by zero");
    assert!(error.contains("divided by zero"), "{error}");
    assert_eq!(
        fs::read_to_string(&output).expect("failed to read partial CSV output"),
        "1\n2\n3\n"
    );
    assert_eq!(
        fs::read_to_string(&checkpoint)
            .expect("failed to read CSV checkpoint")
            .lines()
            .next(),
        Some("4")
    );

    let (success, error) = run_source(&source);
    assert!(!success, "the unchanged failing input should fail again");
    assert!(error.contains("divided by zero"), "{error}");
    assert_eq!(
        fs::read_to_string(&output).expect("failed to read resumed CSV output"),
        "1\n2\n3\n"
    );

    fs::remove_dir_all(root).expect("failed to clean up CSV checkpoint test directory");
}

#[test]
fn checkpoint_resume_uses_input_and_output_commit_identity() {
    let id = TEMP_SOURCE_ID.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!("simply-checkpoint-resume-state-{id}"));
    fs::create_dir_all(&root).expect("failed to create checkpoint resume test directory");
    let input = root.join("input.csv");
    let checkpoint = root.join("progress.state");
    let output = root.join("output.csv");
    fs::write(&input, "1\n2\n3\n").expect("failed to write checkpoint input");
    fs::write(&output, "1\n2\n").expect("failed to seed committed output");
    let input_metadata = fs::metadata(&input).expect("failed to inspect checkpoint input");
    let modified_ns = input_metadata
        .modified()
        .expect("failed to read input modification time")
        .duration_since(std::time::UNIX_EPOCH)
        .expect("input modification time predates epoch")
        .as_nanos();
    let output_checksum = b"1\n2\n".iter().fold(0xcbf29ce484222325u64, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
    });
    let encode_path = |path: &std::path::Path| {
        path.to_string_lossy()
            .as_bytes()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    };
    fs::write(
        &checkpoint,
        format!(
            "2\n4\n{}\n{}\n{}\n{}\n{}\n",
            output_checksum,
            input_metadata.len(),
            modified_ns,
            encode_path(&input),
            encode_path(&output)
        ),
    )
    .expect("failed to seed recovery checkpoint");

    let source = format!(
        "flow result from csv_rows(\"{}\"):\n\
             chunk 2\n\
             checkpoint \"{}\"\n\
             derive list [to_int(item[0])]\n\
             write_csv(\"{}\")\n\
         end\n",
        input.display(),
        checkpoint.display(),
        output.display()
    );
    let (success, error) = run_source(&source);
    assert!(success, "{error}");
    assert_eq!(
        fs::read_to_string(&output).expect("failed to read resumed output"),
        "1\n2\n3\n"
    );
    assert!(!checkpoint.exists());

    fs::remove_dir_all(root).expect("failed to clean up checkpoint resume test directory");
}

#[test]
fn checkpoint_resume_rejects_modified_committed_output() {
    let id = TEMP_SOURCE_ID.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!("simply-checkpoint-output-change-{id}"));
    fs::create_dir_all(&root).expect("failed to create checkpoint output test directory");
    let input = root.join("input.csv");
    let checkpoint = root.join("progress.state");
    let output = root.join("output.csv");
    fs::write(&input, "1\n2\n3\ninvalid\n").expect("failed to write checkpoint input");
    let source = format!(
        "flow result from csv_rows(\"{}\"):\n\
             chunk 2\n\
             checkpoint \"{}\"\n\
             derive list [to_int(item[0])]\n\
             write_csv(\"{}\")\n\
         end\n",
        input.display(),
        checkpoint.display(),
        output.display()
    );
    let (success, error) = run_source(&source);
    assert!(!success);
    assert!(error.contains("cannot convert"), "{error}");
    fs::write(&output, "9\n2\n").expect("failed to alter committed CSV output");

    let (success, error) = run_source(&source);
    assert!(!success);
    assert!(error.contains("changed since the checkpoint"), "{error}");
    assert_eq!(
        fs::read_to_string(&output).expect("failed to read preserved modified output"),
        "9\n2\n"
    );
    fs::remove_dir_all(root).expect("failed to clean up checkpoint output test directory");
}

#[test]
fn flow_parallel_hint_preserves_deterministic_result() {
    let (success, output) = run_source_stdout(
        "values is list [1, 2, 3, 4]\n\
         flow total from values:\n\
             chunk 2\n\
             parallel 2\n\
             derive item * 2\n\
             sum\n\
         end\n\
         Sayln total\n",
    );
    assert!(success, "{output}");
    assert!(output.contains("20"), "{output}");
}

#[test]
fn parallel_rejects_unsupported_expressions_and_execution_combinations() {
    let invalid_flows = [
        (
            "values is list [1, 2]\n\
             flow total from values:\n\
                 parallel 2\n\
                 derive abs(item)\n\
                 sum\n\
             end\n",
            "parallel-safe `derive` expression",
        ),
        (
            "values is list [list [1], list [2]]\n\
             flow total from values:\n\
                 parallel 2\n\
                 count\n\
             end\n",
            "scalar source item type",
        ),
        (
            "values is list [list [1], list [2]]\n\
             flow output from values:\n\
                 parallel 2\n\
                 derive item\n\
                 write_csv(\"out.csv\")\n\
             end\n",
            "scalar source item type",
        ),
        (
            "values is list [1, 2]\n\
             flow output from values:\n\
                 parallel 2\n\
                 derive list [item]\n\
                 write_csv(\"out.csv\")\n\
             end\n",
            "`parallel` requires an aggregate terminal",
        ),
    ];
    for (source, expected_error) in invalid_flows {
        let (success, _, error) = check_source(source);
        assert!(!success);
        assert!(error.contains(expected_error), "{error}");
    }
}

#[test]
fn parallel_checkpoint_combination_is_rejected_explicitly() {
    let (success, _, error) = check_source(
        "values is list [list [1]]\n\
         flow output from values:\n\
             chunk 2\n\
             parallel 2\n\
             checkpoint \"state.tmp\"\n\
             derive item\n\
             write_csv(\"output.tmp\")\n\
         end\n",
    );
    assert!(!success);
    assert!(
        error.contains("`parallel` cannot be combined with `checkpoint`"),
        "{error}"
    );
    let (success, _, error) = check_source(
        "values is list [1, 2]\n\
         flow total from values:\n\
             parallel 2\n\
             checkpoint \"state.tmp\"\n\
             sum\n\
         end\n",
    );
    assert!(!success);
    assert!(
        error.contains("`parallel` cannot be combined with `checkpoint`"),
        "{error}"
    );
}

#[test]
fn chunk_requires_parallel_or_checkpoint() {
    let (success, _, error) = check_source(
        "values is list [1, 2]\n\
         flow total from values:\n\
             chunk 2\n\
             sum\n\
         end\n",
    );
    assert!(!success);
    assert!(
        error.contains("`chunk` requires `parallel` or `checkpoint`"),
        "{error}"
    );
}

#[test]
fn runtime_rejects_chunk_without_an_execution_control() {
    let (success, error) = run_source(
        "values is list [1, 2]\n\
         flow total from values:\n\
             chunk 2\n\
             sum\n\
         end\n",
    );
    assert!(!success);
    assert!(
        error.contains("`chunk` requires `parallel` or `checkpoint`"),
        "{error}"
    );
}

#[test]
fn invalid_parallel_worker_count_is_reported() {
    let (success, _, stderr) = check_source(
        "values is list [1, 2]\n\
         flow total from values:\n\
             parallel 0\n\
             sum\n\
         end\n",
    );
    assert!(!success);
    assert!(
        stderr.contains("parallel worker count must be a positive integer"),
        "{stderr}"
    );
}

#[test]
fn invalid_flow_checkpoint_is_reported() {
    let root = std::env::temp_dir().join(format!(
        "simply-invalid-checkpoint-{}-{}",
        std::process::id(),
        TEMP_SOURCE_ID.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&root).expect("failed to create invalid checkpoint test directory");
    let input = root.join("input.csv");
    let checkpoint = root.join("progress.state");
    let output_path = root.join("output.csv");
    fs::write(&input, "1\n2\n3\n").expect("failed to write invalid checkpoint input");
    fs::write(&checkpoint, "not-a-position").expect("failed to seed checkpoint");
    let source = format!(
        "flow total from csv_rows(\"{}\"):\n\
             chunk 2\n\
             checkpoint \"{}\"\n\
             derive item\n\
             write_csv(\"{}\")\n\
         end\n\
         Sayln total\n",
        input.display(),
        checkpoint.display(),
        output_path.display()
    );
    let (success, output) = run_source(&source);
    let _ = fs::remove_dir_all(root);
    assert!(!success);
    assert!(output.contains("invalid recovery state"), "{output}");
}

#[test]
fn checkpoint_is_limited_to_resumable_csv_output() {
    let unsupported_flows = [
        "flow result from csv_rows(\"input.csv\"):\n\
             checkpoint \"state.tmp\"\n\
             partition item:\n\
                 item[0] == \"1\" -> one\n\
                 otherwise -> other\n\
             end\n\
         end\n",
        "values is list [1, 2]\n\
         flow result from values:\n\
             checkpoint \"state.tmp\"\n\
             derive list [item]\n\
             write_csv(\"out.csv\")\n\
         end\n",
        "flow result from csv_rows(\"input.csv\"):\n\
             checkpoint \"state.tmp\"\n\
             derive item\n\
             partition item:\n\
                 item[0] == \"1\" -> one\n\
                 otherwise -> other\n\
             end\n\
         end\n",
    ];
    for (source, expected) in unsupported_flows.iter().map(|source| {
        let expected = if source.contains("values is list") {
            "`checkpoint` currently requires a `csv_rows` source"
        } else {
            "`checkpoint` requires a `write_csv` terminal"
        };
        (*source, expected)
    }) {
        let (success, _, error) = check_source(source);
        assert!(!success);
        assert!(error.contains(expected), "{error}\nsource:\n{source}");
    }
}

#[test]
fn legacy_position_only_checkpoint_is_not_resumed_unsafely() {
    let id = TEMP_SOURCE_ID.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!("simply-legacy-checkpoint-{id}"));
    fs::create_dir_all(&root).expect("failed to create legacy checkpoint test directory");
    let checkpoint = root.join("progress.state");
    let output = root.join("output.csv");
    let input = root.join("input.csv");
    fs::write(&input, "1\n2\n").expect("failed to write legacy checkpoint input");
    fs::write(&checkpoint, "1").expect("failed to create legacy checkpoint");
    fs::write(&output, "old\n").expect("failed to seed output");
    let source = format!(
        "flow result from csv_rows(\"{}\"):\n\
             checkpoint \"{}\"\n\
             derive item\n\
             write_csv(\"{}\")\n\
         end\n",
        input.display(),
        checkpoint.display(),
        output.display()
    );
    let (success, error) = run_source(&source);
    assert!(!success);
    assert!(error.contains("invalid recovery state"), "{error}");
    assert_eq!(
        fs::read_to_string(&output).expect("failed to read preserved output"),
        "old\n"
    );
    fs::remove_dir_all(root).expect("failed to clean up legacy checkpoint test directory");
}
