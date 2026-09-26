mod common;
use common::*;
use std::{fs, process::Command, sync::atomic::Ordering};

#[test]
fn partition_collects_each_category_in_order() {
    let (success, output) = run_source_stdout(
        "scores is array [125, 240, 375, 410, 560, 685, 790, 825, 930, 1045]\n\
         value is pipeline:\n\
             scores\n\
             partition item:\n\
                 item < 300 -> low\n\
                 item <= 700 -> medium\n\
                 otherwise -> high\n\
             end\n\
         end\n\
         Sayln value\n",
    );
    assert!(success, "{output}");
    assert!(output.contains("low: [125, 240]"), "{output}");
    assert!(output.contains("medium: [375, 410, 560, 685]"), "{output}");
    assert!(output.contains("high: [790, 825, 930, 1045]"), "{output}");
}

#[test]
fn flow_partition_uses_the_shared_partition_semantics() {
    let (success, output) = run_source_stdout(
        "scores is array [125, 240, 375, 410, 560, 685, 790, 825, 930, 1045]\n\
         flow quality from scores:\n\
             partition item:\n\
                 item < 300 -> low\n\
                 item <= 700 -> medium\n\
                 otherwise -> high\n\
             end\n\
         end\n\
         Sayln quality\n\
         Sayln scores\n",
    );
    assert!(success, "{output}");
    assert!(output.contains("low: [125, 240]"), "{output}");
    assert!(output.contains("medium: [375, 410, 560, 685]"), "{output}");
    assert!(output.contains("high: [790, 825, 930, 1045]"), "{output}");
    assert!(output.contains("[125, 240, 375, 410, 560, 685, 790, 825, 930, 1045]"));
}

#[test]
fn partition_uses_first_match_wins() {
    let (success, output) = run_source_stdout(
        "values is list [10, 20, 30, 40]\n\
         result is pipeline:\n\
             values\n\
             partition item:\n\
                 item < 15 -> small\n\
                 item < 35 -> medium\n\
                 otherwise -> large\n\
             end\n\
         end\n\
         Sayln result\n",
    );
    assert!(success, "{output}");
    assert!(output.contains("small: [10]"), "{output}");
    assert!(output.contains("medium: [20, 30]"), "{output}");
    assert!(output.contains("large: [40]"), "{output}");
}

#[test]
fn partition_supports_two_categories_and_omits_unmatched_items() {
    let (success, output) = run_source_stdout(
        "values is list [5, 10, 20]\n\
         result is pipeline:\n\
             values\n\
             partition item:\n\
                 item >= 10 -> large\n\
                 otherwise -> small\n\
             end\n\
         end\n\
         Sayln result\n\
         Sayln values\n",
    );
    assert!(success, "{output}");
    assert!(output.contains("large: [10, 20]"), "{output}");
    assert!(output.contains("small: [5]"), "{output}");
    assert!(output.contains("[5, 10, 20]"), "{output}");
}

#[test]
fn partition_handles_empty_input_and_rules_without_otherwise() {
    let (success, output) = run_source_stdout(
        "empty is list []\n\
         empty_result is pipeline:\n\
             empty\n\
             partition item:\n\
                 item > 0 -> positive\n\
                 otherwise -> other\n\
             end\n\
         end\n\
         values is list [1, 2]\n\
         unmatched_result is pipeline:\n\
             values\n\
             partition item:\n\
                 item > 10 -> large\n\
                 item > 5 -> medium\n\
             end\n\
         end\n\
         Sayln empty_result\n\
         Sayln unmatched_result\n",
    );
    assert!(success, "{output}");
    assert!(output.contains("positive: []"), "{output}");
    assert!(output.contains("other: []"), "{output}");
    assert!(output.contains("large: []"), "{output}");
    assert!(output.contains("medium: []"), "{output}");
}

#[test]
fn partition_merges_rules_with_the_same_category_in_input_order() {
    let (success, output) = run_source_stdout(
        "values is list [1, 2, 3, 4]\n\
         result is pipeline:\n\
             values\n\
             partition item:\n\
                 item == 1 -> shared\n\
                 item == 2 -> shared\n\
                 otherwise -> other\n\
             end\n\
         end\n\
         Sayln result\n",
    );
    assert!(success, "{output}");
    assert!(output.contains("shared: [1, 2]"), "{output}");
    assert!(output.contains("other: [3, 4]"), "{output}");
}

#[test]
fn where_and_derive_compose_before_partition() {
    let (success, output) = run_source_stdout(
        "values is list [1, 2, 3, 4]\n\
         selected is pipeline:\n\
             values\n\
             where item > 1\n\
             derive item * 2\n\
             partition value:\n\
                 value < 7 -> small\n\
                 otherwise -> large\n\
             end\n\
         end\n\
         Sayln selected\n",
    );
    assert!(success, "{output}");
    assert!(output.contains("small: [4, 6]"), "{output}");
    assert!(output.contains("large: [8]"), "{output}");
}

#[test]
fn where_can_select_before_partition() {
    let (success, output) = run_source_stdout(
        "values is list [1, 2, 3, 4]\n\
         selected is pipeline:\n\
             values\n\
             where item > 1\n\
             partition item:\n\
                 item < 4 -> small\n\
                 otherwise -> large\n\
             end\n\
         end\n\
         Sayln selected\n",
    );
    assert!(success, "{output}");
    assert!(output.contains("small: [2, 3]"), "{output}");
    assert!(output.contains("large: [4]"), "{output}");
}

#[test]
fn partition_rejects_non_boolean_conditions_and_invalid_categories() {
    let (success, _, error) = check_source(
        "values is list [1, 2]\n\
         result is pipeline:\n\
             values\n\
             partition item:\n\
                 item + 1 -> positive\n\
                 otherwise -> other\n\
             end\n\
         end\n",
    );
    assert!(!success);
    assert!(error.contains("expected Bool, found Int"), "{error}");

    let (success, error) = run_source(
        "values is list [1, 2]\n\
         result is pipeline:\n\
             values\n\
             partition item:\n\
                 item > 1 -> 2invalid\n\
                 otherwise -> other\n\
             end\n\
         end\n",
    );
    assert!(!success);
    assert!(error.contains("Parse error"), "{error}");
}

#[test]
fn pipeline_semantics_reject_sources_not_supported_by_the_runtime() {
    let (success, _, error) = check_source(
        "values is (1, 2)\n\
         result is pipeline:\n\
             values\n\
             where item > 0\n\
         end\n",
    );
    assert!(!success);
    assert!(
        error.contains("pipeline source must be an array or list"),
        "{error}"
    );
}

#[test]
fn semantic_checker_rejects_scalar_rows_before_write_csv() {
    let (success, _, error) = check_source(
        "values is list [1, 2]\n\
         result is pipeline:\n\
             values\n\
             derive item\n\
             write_csv(\"out.csv\")\n\
         end\n",
    );
    assert!(!success);
    assert!(
        error.contains("write_csv` requires `derive` to produce a row collection, found Int"),
        "{error}"
    );
}

#[test]
fn semantic_checker_rejects_csv_output_for_in_memory_sources() {
    let (success, _, error) = check_source(
        "rows is list [list [\"Ada\"]]\n\
         flow result from rows:\n\
             derive item\n\
             write_csv(\"out.csv\")\n\
         end\n",
    );
    assert!(!success);
    assert!(
        error.contains("`write_csv` requires a `csv_rows` source"),
        "{error}"
    );
}

#[test]
fn partition_must_remain_the_pipeline_terminal() {
    for terminal in [
        "sum",
        "write_csv(\"out.csv\")",
        "derive item",
        "partition other:\n                    other > 0 -> positive\n                    otherwise -> negative\n                end",
    ] {
        let (success, error) = run_source(&format!(
            "values is list [1, 2]\n\
             result is pipeline:\n\
                 values\n\
                 partition item:\n\
                     item > 1 -> large\n\
                     otherwise -> small\n\
                 end\n\
                 {terminal}\n\
             end\n"
        ));
        assert!(!success);
        assert!(
            error.contains("cannot continue after a terminal"),
            "{error}"
        );
    }
}

#[test]
fn removed_pipeline_aliases_are_not_accepted() {
    for source in [
        "values is list [1]\nflow result from values:\n    keep item\n    count\nend\n",
        "values is list [1]\nresult is pipeline:\n    values\n    map item\nend\n",
        "values is list [1]\nresult is pipeline:\n    values\n    filter item > 0\nend\n",
        "values is list [1]\nflow result from values:\n    write \"out.csv\"\nend\n",
    ] {
        let (success, error) = run_source(source);
        assert!(!success, "removed alias unexpectedly parsed: {source}");
        assert!(
            error.contains("expected flow step")
                || error.contains("expected pipeline step")
                || error.contains("not available in Flow"),
            "unexpected error for removed alias: {error}"
        );
    }
}

#[test]
fn runs_collections_and_pipelines() {
    let collections = run_example("examples/06-collections/arrays-lists.si");
    let pipeline = run_example("examples/07-pipelines/collections.si");
    assert!(collections.contains("Citra"));
    assert!(pipeline.contains("180"));
}

#[test]
fn fuses_multi_stage_pipeline_terminals() {
    let (success, stdout) = run_source_stdout(
        "values is list [1, 2, 3, 4]\n\
         total is pipeline:\n\
             values\n\
             where item >= 2\n\
             derive item * 3\n\
             where item > 6\n\
             sum\n\
         end\n\
         count is pipeline:\n\
             values\n\
             derive item * 2\n\
             where item >= 6\n\
             count\n\
         end\n\
         Sayln total\n\
         Sayln count\n",
    );
    assert!(success);
    assert_eq!(stdout.lines().collect::<Vec<_>>(), ["21", "2"]);
}

#[test]
fn ranges_are_lazy_but_keep_array_semantics() {
    let (success, output) = run_source_stdout(
        "values is range(0, 5)\n\
         Sayln values[3]\n\
         Sayln length(values)\n\
         total is pipeline:\n\
             values\n\
             derive item * 2\n\
             sum\n\
         end\n\
         Sayln total\n",
    );
    assert!(success);
    assert_eq!(output, "3\n5\n20\n");
}

#[test]
fn multi_stage_pipeline_avoids_stage_materialization() {
    let (success, output) = run_source_stdout(
        "values is range(0, 6)\n\
         result is pipeline:\n\
             values\n\
             where item % 2 == 0\n\
             derive item * 10\n\
             where item > 10\n\
         end\n\
         Sayln result[0]\n\
         Sayln result[1]\n",
    );
    assert!(success);
    assert_eq!(output, "20\n40\n");
}

#[test]
fn numeric_cleanup_formulas_are_composable() {
    let (success, output) = run_source_stdout(
        "a is to_int(\"42\")\n\
         b is abs(-3)\n\
         c is round(12.3456, 2)\n\
         d is clamp(150.0, 0.0, 100.0)\n\
         Sayln a\n\
         Sayln b\n\
         Sayln c\n\
         Sayln d\n",
    );
    assert!(success);
    assert_eq!(output, "42\n3\n12.35\n100\n");
}

#[test]
fn pipeline_numeric_aggregates_stream_without_materializing() {
    let (success, output) = run_source_stdout(
        "values is range(1, 6)\n\
         avg is pipeline:\n\
             values\n\
             average\n\
         end\n\
         minimum is pipeline:\n\
             values\n\
             min\n\
         end\n\
         maximum is pipeline:\n\
             values\n\
             max\n\
         end\n\
         Sayln avg\n\
         Sayln minimum\n\
         Sayln maximum\n",
    );
    assert!(success);
    assert_eq!(output, "3\n1\n5\n");
}

#[test]
fn aggregates_compose_with_where_and_derive_and_define_empty_behavior() {
    let (success, output) = run_source_stdout(
        "values is list [1, 2, 3, 4]\n\
         total is pipeline:\n\
             values\n\
             where item >= 2\n\
             derive item * 2\n\
             sum\n\
         end\n\
         amount is pipeline:\n\
             values\n\
             where item >= 2\n\
             derive item * 2\n\
             count\n\
         end\n\
         mean is pipeline:\n\
             values\n\
             where item >= 2\n\
             derive item * 2\n\
             average\n\
         end\n\
         minimum is pipeline:\n\
             values\n\
             where item >= 2\n\
             derive item * 2\n\
             min\n\
         end\n\
         maximum is pipeline:\n\
             values\n\
             where item >= 2\n\
             derive item * 2\n\
             max\n\
         end\n\
         empty is list []\n\
         empty_total is pipeline:\n\
             empty\n\
             sum\n\
         end\n\
         empty_count is pipeline:\n\
             empty\n\
             count\n\
         end\n\
         Sayln total\n\
         Sayln amount\n\
         Sayln mean\n\
         Sayln minimum\n\
         Sayln maximum\n\
         Sayln empty_total\n\
         Sayln empty_count\n",
    );
    assert!(success, "{output}");
    assert_eq!(
        output.lines().collect::<Vec<_>>(),
        ["18", "3", "6", "4", "8", "0", "0"]
    );

    for terminal in ["average", "min", "max"] {
        let (success, error) = run_source(&format!(
            "empty is list []\nresult is pipeline:\n    empty\n    {terminal}\nend\n"
        ));
        assert!(!success);
        assert!(
            error.contains("requires at least one numeric value"),
            "{error}"
        );
    }
}

#[test]
fn nested_closure_captures_partition_condition_dependencies() {
    let source = "fn make_classifier(limit):\n\
             fn classify(values):\n\
                 result is pipeline:\n\
                     values\n\
                     partition value:\n\
                         value < limit -> lower\n\
                         otherwise -> upper\n\
                     end\n\
                 end\n\
                 return result\n\
             end\n\
             return classify\n\
         end\n\
         classifier is make_classifier(3)\n\
         Sayln classifier(list [1, 4])\n";
    let source_id = TEMP_SOURCE_ID.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "simply-closure-partition-{}-{source_id}.si",
        std::process::id()
    ));
    fs::write(&path, source).expect("failed to write closure partition source");
    let output = Command::new(env!("CARGO_BIN_EXE_simply"))
        .args(["run", path.to_str().expect("temporary path must be UTF-8")])
        .output()
        .expect("failed to execute closure partition source");
    let _ = fs::remove_file(path);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stderr}");
    assert!(stdout.contains("lower: [1]"), "{stdout}");
    assert!(stdout.contains("upper: [4]"), "{stdout}");
}

#[test]
fn restores_item_after_pipeline_evaluation() {
    let path = std::env::temp_dir().join(format!("simply-pipeline-{}.si", std::process::id()));
    fs::write(
        &path,
        "item is 99\nvalues is list [1, 2]\nresult is pipeline:\n    values\n    derive item * 2\n    count\nend\nSayln item\n",
    )
    .expect("failed to write pipeline source");
    let output = Command::new(env!("CARGO_BIN_EXE_simply"))
        .args(["run", path.to_str().expect("temporary path was not UTF-8")])
        .output()
        .expect("failed to run pipeline source");
    let _ = fs::remove_file(path);

    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "99");
}
