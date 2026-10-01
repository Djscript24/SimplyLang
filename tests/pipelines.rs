mod common;
use common::*;
use std::{fs, process::Command, sync::atomic::Ordering};

#[test]
fn any_and_all_terminals_reduce_boolean_pipeline_items() {
    let (success, output) = run_source_stdout(
        "flags is list [false, true, true]\n\
         any_match is pipeline:\n\
             flags\n\
             any\n\
         end\n\
         all_match is pipeline:\n\
             flags\n\
             all\n\
         end\n\
         no_match is pipeline:\n\
             list [false, false]\n\
             any\n\
         end\n\
         empty_match is pipeline:\n\
             list []\n\
             derive true\n\
             all\n\
         end\n\
         flow flow_match from flags:\n\
             all\n\
         end\n\
         Sayln any_match\n\
         Sayln all_match\n\
         Sayln no_match\n\
         Sayln empty_match\n\
         Sayln flow_match\n\
         Sayln any(flags)\n\
         Sayln all(flags)\n",
    );
    assert!(success, "{output}");
    assert!(
        output.contains("true\nfalse\nfalse\ntrue\nfalse\ntrue\nfalse\n"),
        "{output}"
    );
}

#[test]
fn any_and_all_require_boolean_pipeline_items() {
    let (success, _, error) = check_source(
        "values is list [1, 2]\n\
         result is pipeline:\n\
             values\n\
             any\n\
         end\n",
    );
    assert!(!success);
    assert!(error.contains("expected Bool"), "{error}");
}

#[test]
fn step_by_samples_at_its_position_in_pipelines_and_flows() {
    let (success, output) = run_source_stdout(
        "values is list [1, 2, 3, 4, 5, 6]\n\
         sampled is pipeline:\n\
             values\n\
             step_by 2\n\
         end\n\
         filtered_first is pipeline:\n\
             values\n\
             where item % 2 == 0\n\
             step_by 2\n\
         end\n\
         sampled_first is pipeline:\n\
             values\n\
             step_by 2\n\
             where item % 2 == 0\n\
         end\n\
         flow range_count from range(1, 10):\n\
             step_by 3\n\
             count\n\
         end\n\
         Sayln sampled\n\
         Sayln filtered_first\n\
         Sayln sampled_first\n\
         Sayln range_count\n",
    );
    assert!(success, "{output}");
    assert_eq!(output, "[1, 3, 5]\n[2, 6]\n[]\n3\n");
}

#[test]
fn step_by_requires_a_positive_interval_and_rejects_unordered_controls() {
    let (success, error) = run_source(
        "values is list [1, 2]\n\
         result is pipeline:\n\
             values\n\
             step_by 0\n\
         end\n",
    );
    assert!(!success);
    assert!(
        error.contains("step_by` interval must be a positive integer"),
        "{error}"
    );

    let (success, _, error) = check_source(
        "flow sampled from range(1, 10):\n\
             step_by 2\n\
             parallel 2\n\
             sum\n\
         end\n",
    );
    assert!(!success);
    assert!(error.contains("`step_by`"), "{error}");
    assert!(error.contains("`parallel`"), "{error}");

    let (success, _, error) = check_source(
        "flow sampled from csv_rows(\"input.csv\"):\n\
             step_by 2\n\
             checkpoint \"state.chk\"\n\
             write_csv(\"output.csv\")\n\
         end\n",
    );
    assert!(!success);
    assert!(error.contains("`step_by`"), "{error}");
    assert!(error.contains("`checkpoint`"), "{error}");
}

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
fn take_limits_pipeline_output_after_prior_filters_and_transforms() {
    let (success, output) = run_source_stdout(
        "values is range(0, 1000000000)\n\
         selected is pipeline:\n\
             values\n\
             where item % 3 == 0\n\
             derive item * 2\n\
             take 3\n\
         end\n\
         prefix is pipeline:\n\
             values\n\
             take 3\n\
             where item > 0\n\
         end\n\
         Sayln selected\n\
         Sayln prefix\n",
    );
    assert!(success, "{output}");
    assert_eq!(output, "[0, 6, 12]\n[1, 2]\n");
}

#[test]
fn take_zero_short_circuits_ranges_and_take_composes_with_aggregates() {
    let (success, output) = run_source_stdout(
        "values is range(0, 1000000000)\n\
         empty is pipeline:\n\
             values\n\
             derive 1 / item\n\
             take 0\n\
         end\n\
         total is pipeline:\n\
             values\n\
             take 2\n\
             derive item * 10\n\
             sum\n\
         end\n\
         amount is pipeline:\n\
             values\n\
             take 2\n\
             count\n\
         end\n\
         mean is pipeline:\n\
             values\n\
             take 2\n\
             average\n\
         end\n\
         minimum is pipeline:\n\
             values\n\
             take 2\n\
             min\n\
         end\n\
         maximum is pipeline:\n\
             values\n\
             take 2\n\
             max\n\
         end\n\
         Sayln empty\n\
         Sayln total\n\
         Sayln amount\n\
         Sayln mean\n\
         Sayln minimum\n\
         Sayln maximum\n",
    );
    assert!(success, "{output}");
    assert_eq!(output, "[]\n10\n2\n0.5\n0\n1\n");
}

#[test]
fn skip_discards_items_at_its_pipeline_position() {
    let (success, output) = run_source_stdout(
        "values is range(0, 1000000000)\n\
         selected is pipeline:\n\
             values\n\
             where item % 2 == 0\n\
             skip 2\n\
             take 2\n\
             derive item * 10\n\
         end\n\
         after_prefix is pipeline:\n\
             values\n\
             skip 2\n\
             where item % 2 == 0\n\
             take 4\n\
         end\n\
         skipped_total is pipeline:\n\
             values\n\
             skip 3\n\
             take 2\n\
             sum\n\
         end\n\
         unchanged is pipeline:\n\
             values\n\
             skip 0\n\
             take 2\n\
         end\n\
         Sayln selected\n\
         Sayln after_prefix\n\
         Sayln skipped_total\n\
         Sayln unchanged\n",
    );
    assert!(success, "{output}");
    assert_eq!(output, "[40, 60]\n[2, 4, 6, 8]\n7\n[0, 1]\n");
}

#[test]
fn take_while_keeps_only_the_matching_prefix_and_stops_streaming() {
    let (success, output) = run_source_stdout(
        "values is range(0, 1000000000)\n\
         prefix is pipeline:\n\
             values\n\
             take_while item < 4\n\
             derive item * 2\n\
         end\n\
         filtered_prefix is pipeline:\n\
             values\n\
             where item % 2 == 0\n\
             take_while item < 6\n\
             count\n\
         end\n\
         empty is pipeline:\n\
             values\n\
             take_while item < 0\n\
         end\n\
         Sayln prefix\n\
         Sayln filtered_prefix\n\
         Sayln empty\n",
    );
    assert!(success, "{output}");
    assert_eq!(output, "[0, 2, 4, 6]\n3\n[]\n");
}

#[test]
fn take_while_checks_boolean_conditions_and_rejects_unordered_controls() {
    let (success, _, error) = check_source(
        "values is list [1, 2]\n\
         result is pipeline:\n\
             values\n\
             take_while item\n\
         end\n",
    );
    assert!(!success);
    assert!(error.contains("expected Bool, found Int"), "{error}");

    for controls in [
        "take_while item > 0\nparallel 2\nsum",
        "take_while item > 0\ncheckpoint \"state.json\"\nwrite_csv(\"out.csv\")",
    ] {
        let (success, _, error) = check_source(&format!(
            "flow result from list [1, 2]:\n    {controls}\nend\n"
        ));
        assert!(!success);
        assert!(
            error.contains("take_while") && error.contains("cannot be combined"),
            "{error}"
        );
    }
}

#[test]
fn drop_while_discards_only_the_matching_prefix() {
    let (success, output) = run_source_stdout(
        "values is list [1, 2, 3, 1, 4]\n\
         remaining is pipeline:\n\
             values\n\
             drop_while item < 3\n\
         end\n\
         filtered is pipeline:\n\
             values\n\
             where item % 2 == 1\n\
             drop_while item < 3\n\
             take 2\n\
         end\n\
         range_result is pipeline:\n\
             range(0, 1000000000)\n\
             drop_while item < 3\n\
             take 2\n\
         end\n\
         Sayln remaining\n\
         Sayln filtered\n\
         Sayln range_result\n",
    );
    assert!(success, "{output}");
    assert_eq!(output, "[3, 1, 4]\n[3, 1]\n[3, 4]\n");
}

#[test]
fn drop_while_requires_a_boolean_and_rejects_unordered_controls() {
    let (success, _, error) = check_source(
        "values is list [1, 2]\n\
         result is pipeline:\n\
             values\n\
             drop_while item\n\
         end\n",
    );
    assert!(!success);
    assert!(error.contains("expected Bool, found Int"), "{error}");

    for steps in [
        "drop_while item > 0\nparallel 2\nsum",
        "drop_while item > 0\ncheckpoint \"state.json\"\nwrite_csv(\"out.csv\")",
    ] {
        let (success, _, error) = check_source(&format!(
            "flow result from list [1, 2]:\n    {steps}\nend\n"
        ));
        assert!(!success);
        assert!(
            error.contains("drop_while") && error.contains("cannot be combined"),
            "{error}"
        );
    }
}

#[test]
fn distinct_preserves_first_occurrences_and_uses_value_equality() {
    let (success, output) = run_source_stdout(
        "values is list [1, 2, 1, 3, 2]\n\
         unique is pipeline:\n\
             values\n\
             distinct\n\
         end\n\
         nested is pipeline:\n\
             array [[1, 2], [1, 2], [2, 1]]\n\
             distinct\n\
         end\n\
         mapped is pipeline:\n\
             list [1, 2, 3, 4]\n\
             derive item % 2\n\
             distinct\n\
         end\n\
         total is pipeline:\n\
             values\n\
             where item > 1\n\
             distinct\n\
             sum\n\
         end\n\
         flow amount from values:\n\
             distinct\n\
             count\n\
         end\n\
         Sayln unique\n\
         Sayln nested\n\
         Sayln mapped\n\
         Sayln total\n\
         Sayln amount\n",
    );
    assert!(success, "{output}");
    assert_eq!(output, "[1, 2, 3]\n[[1, 2], [2, 1]]\n[1, 0]\n5\n3\n");
}

#[test]
fn flow_take_composes_with_terminals_and_rejects_incompatible_controls() {
    let (success, output) = run_source_stdout(
        "flow total from range(1, 1000000000):\n\
             take 3\n\
             sum\n\
         end\n\
         Sayln total\n",
    );
    assert!(success, "{output}");
    assert_eq!(output, "6\n");

    for (steps, diagnostic) in [
        (
            "take 1\nparallel 2\nsum",
            "cannot be combined with `parallel`",
        ),
        (
            "take 1\ncheckpoint \"state.json\"\nwrite_csv(\"out.csv\")",
            "cannot be combined with `checkpoint`",
        ),
        (
            "skip 1\nparallel 2\nsum",
            "cannot be combined with `parallel`",
        ),
        (
            "skip 1\ncheckpoint \"state.json\"\nwrite_csv(\"out.csv\")",
            "cannot be combined with `checkpoint`",
        ),
        (
            "distinct\nparallel 2\nsum",
            "cannot be combined with `parallel`",
        ),
        (
            "drop_while item > 0\nparallel 2\nsum",
            "cannot be combined with `parallel`",
        ),
        (
            "drop_while item > 0\ncheckpoint \"state.json\"\nwrite_csv(\"out.csv\")",
            "cannot be combined with `checkpoint`",
        ),
        (
            "distinct\ncheckpoint \"state.json\"\nwrite_csv(\"out.csv\")",
            "cannot be combined with `checkpoint`",
        ),
    ] {
        let (success, _, error) = check_source(&format!(
            "flow result from list [1, 2]:\n    {steps}\nend\n"
        ));
        assert!(!success);
        assert!(error.contains(diagnostic), "{error}");
    }

    let (success, error) =
        run_source("values is list [1, 2]\nresult is pipeline:\n    values\n    take -1\nend\n");
    assert!(!success);
    assert!(
        error.contains("take count must be a non-negative integer"),
        "{error}"
    );

    let (success, error) =
        run_source("values is list [1, 2]\nresult is pipeline:\n    values\n    skip -1\nend\n");
    assert!(!success);
    assert!(
        error.contains("skip count must be a non-negative integer"),
        "{error}"
    );
}

#[test]
fn formatter_preserves_pipeline_steps_for_pipelines_and_flows() {
    let id = TEMP_SOURCE_ID.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!("simply-take-format-{id}"));
    fs::create_dir_all(&root).expect("failed to create take format test directory");
    let source = root.join("source.si");
    let formatted_source = root.join("formatted.si");
    fs::write(
        &source,
        "values is range(1, 8)\n\
         selected is pipeline:\n\
             values\n\
             take 2\n\
             skip 1\n\
             distinct\n\
             take_while item < 5\n\
             derive item > 0\n\
             any\n\
         end\n\
         flow total from values:\n\
             skip 1\n\
             take 3\n\
             distinct\n\
             take_while item < 7\n\
             drop_while item < 3\n\
             step_by 2\n\
             sum\n\
         end\n\
         Sayln selected\n\
         Sayln total\n",
    )
    .expect("failed to write take format source");
    let formatted = Command::new(env!("CARGO_BIN_EXE_simply"))
        .args(["fmt", source.to_str().expect("source path must be UTF-8")])
        .output()
        .expect("failed to format take source");
    assert!(
        formatted.status.success(),
        "{}",
        String::from_utf8_lossy(&formatted.stderr)
    );
    let formatted_text = String::from_utf8_lossy(&formatted.stdout);
    assert!(formatted_text.contains("take 2"), "{formatted_text}");
    assert!(formatted_text.contains("take 3"), "{formatted_text}");
    assert!(formatted_text.contains("skip 1"), "{formatted_text}");
    assert!(formatted_text.contains("step_by 2"), "{formatted_text}");
    assert!(formatted_text.contains("distinct"), "{formatted_text}");
    assert!(
        formatted_text.contains("take_while item < 5")
            && formatted_text.contains("take_while item < 7"),
        "{formatted_text}"
    );
    assert!(formatted_text.contains("    any"), "{formatted_text}");
    assert!(
        formatted_text.contains("drop_while item < 3"),
        "{formatted_text}"
    );
    fs::write(&formatted_source, formatted.stdout).expect("failed to write formatted source");

    let output = Command::new(env!("CARGO_BIN_EXE_simply"))
        .args([
            "run",
            formatted_source
                .to_str()
                .expect("formatted source path must be UTF-8"),
        ])
        .output()
        .expect("failed to run formatted take source");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "true\n3\n");
    fs::remove_dir_all(root).expect("failed to clean up take format test directory");
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
fn range_aggregate_pipeline_handles_scalar_transforms_and_short_circuiting() {
    let (success, output) = run_source_stdout(
        "values is range(0, 6)\n\
         total is pipeline:\n\
             values\n\
             where item % 2 == 0\n\
             derive item * 3\n\
             where item > 0\n\
             sum\n\
         end\n\
         amount is pipeline:\n\
             values\n\
             where item % 2 == 0\n\
             derive item * 3\n\
             where item > 0\n\
             count\n\
         end\n\
         mean is pipeline:\n\
             values\n\
             where item % 2 == 0\n\
             derive item * 3\n\
             where item > 0\n\
             average\n\
         end\n\
         minimum is pipeline:\n\
             values\n\
             where item % 2 == 0\n\
             derive item * 3\n\
             where item > 0\n\
             min\n\
         end\n\
         maximum is pipeline:\n\
             values\n\
             where item % 2 == 0\n\
             derive item * 3\n\
             where item > 0\n\
             max\n\
         end\n\
         zero_safe is pipeline:\n\
             values\n\
             where item == 0 or 10 / item > 1\n\
             count\n\
         end\n\
         Sayln total\n\
         Sayln amount\n\
         Sayln mean\n\
         Sayln minimum\n\
         Sayln maximum\n\
         Sayln zero_safe\n",
    );
    assert!(success, "{output}");
    assert_eq!(output, "18\n2\n9\n6\n12\n6\n");
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
