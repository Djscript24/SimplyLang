mod common;
use common::*;
use std::{fs, process::Command, sync::atomic::Ordering};

#[test]
fn destructures_nested_tuples_sequences_and_wildcards() {
    let (success, output) = run_source_stdout(
        "pair is (10, 20)\n\
         (first, second) is pair\n\
         (_, (third, fourth)) is (99, (2, 3))\n\
         (number, [left, right]) is (1, [4, 5])\n\
         ([a, b], (c, d)) is ([6, 7], (8, 9))\n\
         Sayln first\n\
         Sayln second\n\
         Sayln third + fourth\n\
         Sayln number + left + right\n\
         Sayln a + b + c + d\n",
    );
    assert!(success, "{output}");
    assert_eq!(output, "10\n20\n5\n10\n30\n");
}

#[test]
fn sequence_rest_preserves_collection_kind_and_supports_ranges() {
    let (success, output) = run_source_stdout(
        "values is [1, 2, 3]\n\
         [head, ...tail] is values\n\
         [...all] is list [4, 5]\n\
        [...empty] is []\n\
        [first, ...range_tail] is range(6, 9)\n\
        mut (mutable, ignored) is (7, 8)\n\
        mutable -> 9\n\
        Sayln head + tail[0]\n\
        Sayln all[1]\n\
        Sayln range_tail[0]\n\
        Sayln mutable\n",
    );
    assert!(success, "{output}");
    assert_eq!(output, "3\n5\n7\n9\n");
    let (success, error) = run_source("(_, _) is (1, 2)\nSayln _\n");
    assert!(!success);
    assert!(error.contains("unknown variable `_`"), "{error}");
}

#[test]
fn nested_sequence_rest_extracts_rows() {
    let (success, output) = run_source_stdout(
        "grid is [[1, 2, 3], [4, 5]]\n\
         [[head, ...tail], ...rows] is grid\n\
         Sayln head + tail[0]\n\
         Sayln rows[0][0]\n",
    );
    assert!(success, "{output}");
    assert_eq!(output, "3\n4\n");
}

#[test]
fn destructuring_assignment_updates_existing_mutable_bindings() {
    let (success, output) = run_source_stdout(
        "mut first is 1\n\
         mut second is 2\n\
         mut third is 3\n\
        mut tail is [0]\n\
         (first, (second, third)) -> (10, (20, 30))\n\
         Sayln first + second + third\n\
         ([first, ...tail], second) -> ([40, 50, 60], 70)\n\
         Sayln first + tail[0] + second\n\
         (_, second) -> (999, 80)\n\
         Sayln first\n\
         Sayln second\n",
    );
    assert!(success, "{output}");
    assert_eq!(output, "60\n160\n40\n80\n");
}

#[test]
fn destructuring_assignment_evaluates_rhs_once_and_preserves_mutability() {
    let (success, output) = run_source_stdout(
        "mut calls is 0\n\
         mut first is 0\n\
         mut second is 0\n\
         fn make_pair():\n\
             calls -> calls + 1\n\
             return (4, 5)\n\
         end\n\
         (first, second) -> make_pair()\n\
         Sayln calls\n\
         Sayln first + second\n",
    );
    assert!(success, "{output}");
    assert_eq!(output, "1\n9\n");

    let (success, _, error) =
        check_source("first is 1\nmut second is 2\n(first, second) -> (3, 4)\n");
    assert!(!success);
    assert!(error.contains("immutable variable `first`"), "{error}");
}

#[test]
fn destructuring_assignment_reuses_existing_type_checks() {
    let (success, output) = run_source_stdout(
        "mut first as Int is 1\n\
         mut second as String is \"old\"\n\
         (first, second) -> (10, \"hello\")\n\
         Sayln first\n\
         Sayln second\n",
    );
    assert!(success, "{output}");
    assert_eq!(output, "10\nhello\n");

    let (success, _, error) = check_source(
        "mut first as Int is 1\n\
         mut second as String is \"old\"\n\
         (first, second) -> (\"hello\", 10)\n",
    );
    assert!(!success);
    assert!(error.contains("type mismatch"), "{error}");
}

#[test]
fn destructuring_assignment_preflights_nested_extraction_and_runtime_types() {
    let (success, output) = run_source_stdout(
        "fn identity(value):\n\
             return value\n\
         end\n\
         mut first as Int is 1\n\
         mut second as String is \"old\"\n\
         mut third as Int is 3\n\
         try:\n\
             (first, second) -> identity((10, 20))\n\
         catch error:\n\
             Sayln first\n\
             Sayln second\n\
         end\n\
         try:\n\
             (first, (second, third)) -> identity((10, (\"new\", 30, 40)))\n\
         catch error:\n\
             Sayln first\n\
             Sayln second\n\
             Sayln third\n\
         end\n",
    );
    assert!(success, "{output}");
    assert_eq!(output, "1\nold\n1\nold\n3\n");
}

#[test]
fn destructuring_assignment_rejects_missing_duplicate_and_invalid_targets() {
    for (source, expected) in [
        (
            "mut first is 1\n(first, second) -> (2, 3)\n",
            "cannot destructure-assign unknown variable `second`",
        ),
        (
            "mut first is 1\n(first, first) -> (2, 3)\n",
            "duplicate destructuring assignment target `first`",
        ),
        (
            "mut first is 1\nmut second is 2\n(1, second) -> (3, 4)\n",
            "support only identifiers",
        ),
        (
            "mut first is 1\nmut second is 2\n(first | second, second) -> (3, 4)\n",
            "support only identifiers",
        ),
        (
            "mut first is 1\nmut second is 2\n(whole @ first, second) -> (3, 4)\n",
            "support only identifiers",
        ),
        (
            "mut first is 1\nmut second is 2\n{name: first} -> first\n",
            "support only identifiers",
        ),
        (
            "type Point:\n    x as Int\n    y as Int\nend\n\
             mut first is 1\n\
             mut second is 2\n\
             Point(first, second) -> Point(3, 4)\n",
            "support only identifiers",
        ),
        (
            "enum Result:\n    Ok as Int\nend\n\
             mut first is 1\n\
             Result::Ok(first) -> Result::Ok(3)\n",
            "support only identifiers",
        ),
    ] {
        let (success, _, error) = check_source(source);
        assert!(!success, "{source} unexpectedly passed semantic checking");
        assert!(error.contains(expected), "{source}\n{error}");
    }

    let (success, _, error) =
        check_source("mut first is 1\nmut second is 2\n[first, second] -> (3, 4)\n");
    assert!(!success);
    assert!(
        error.contains("sequence destructuring assignment requires"),
        "{error}"
    );

    let (success, error) = run_source("mut first is 1\nmut second is 2\n(first, second) -> (3)\n");
    assert!(!success);
    assert!(
        error.contains("destructuring assignment target does not match"),
        "{error}"
    );

    let (success, output) = run_source_stdout(
        "mut first is 1\n\
         mut second is 2\n\
         values is [3]\n\
         try:\n\
             [first, second] -> values\n\
         catch error:\n\
             Sayln first + second\n\
         end\n",
    );
    assert!(success, "{output}");
    assert_eq!(output, "3\n");
}

#[test]
fn destructuring_rejects_shape_errors_duplicate_names_and_assertion_patterns() {
    for (source, expected) in [
        (
            "value is (1, 2, 3)\n(a, b) is value\n",
            "tuple has 3 values",
        ),
        ("value is 1\n(a, b) is value\n", "requires a tuple"),
        ("value is 1\n[a] is value\n", "requires an Array or List"),
        (
            "value is (1, 2)\n[a] is value\n",
            "requires an Array or List",
        ),
        (
            "value is ((1, 2), (3, 4))\n(a, (b, a)) is value\n",
            "already declared or bound more than once",
        ),
        (
            "value is (1, 2)\n(1, b) is value\n",
            "support only identifiers",
        ),
        (
            "value is (1, 2)\n(0..10, b) is value\n",
            "support only identifiers",
        ),
        (
            "value is (1, 2)\n(a | b, c) is value\n",
            "support only identifiers",
        ),
        (
            "value is ((1, 2), 5)\n((whole @ (a, b)), c) is value\n",
            "support only identifiers",
        ),
        (
            "value is [1, 2]\nwhole @ [a, b] is value\n",
            "support only identifiers",
        ),
        (
            "value is [1]\n{item: name} is value\n",
            "support only identifiers",
        ),
        (
            "type Point:\n    x as Int\n    y as Int\nend\n\
             point is Point(1, 2)\n\
             Point(x, y) is point\n",
            "support only identifiers",
        ),
        (
            "enum Result:\n    Ok as Int\nend\n\
             value is Result::Ok(1)\n\
             Result::Ok(item) is value\n",
            "support only identifiers",
        ),
    ] {
        let (success, _, error) = check_source(source);
        assert!(!success, "{source} unexpectedly passed semantic checking");
        assert!(error.contains(expected), "{source}\n{error}");
    }

    let (success, error) = run_source("value is [1]\n[a, b] is value\n");
    assert!(!success);
    assert!(
        error.contains("destructuring target does not match"),
        "{error}"
    );
    let (success, error) = run_source("value is [1, 2, 3]\n[a, b] is value\n");
    assert!(!success);
    assert!(
        error.contains("destructuring target does not match"),
        "{error}"
    );
}

#[test]
fn malformed_rest_targets_are_rejected_by_the_parser() {
    for source in [
        "value is [1, 2]\n[a, ...tail, b] is value\n",
        "value is [1, 2]\n[...first, ...second] is value\n",
        "value is [1]\n[..._] is value\n",
    ] {
        let (success, _, error) = check_source(source);
        assert!(!success, "{source} unexpectedly parsed");
        assert!(
            error.contains("rest pattern") || error.contains("expected `]`"),
            "{source}\n{error}"
        );
    }
}

#[test]
fn csv_suffix_remains_lazy_and_can_feed_an_existing_pipeline() {
    let id = TEMP_SOURCE_ID.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!("simply-destructure-csv-{id}"));
    fs::create_dir_all(&root).expect("failed to create CSV destructuring directory");
    let input = root.join("input.csv");
    let source = root.join("main.si");

    fs::write(&input, "good,1\nbad\"quote,2\n").expect("failed to write malformed suffix");
    fs::write(
        &source,
        format!(
            "rows is csv_rows(\"{}\")\n\
             [head, ...tail] is rows\n\
             Sayln head[0]\n",
            input.display()
        ),
    )
    .expect("failed to write lazy CSV destructuring source");
    let lazy = Command::new(env!("CARGO_BIN_EXE_simply"))
        .args(["run", source.to_str().expect("source path must be UTF-8")])
        .output()
        .expect("failed to execute lazy CSV destructuring");
    assert!(
        lazy.status.success(),
        "{}",
        String::from_utf8_lossy(&lazy.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&lazy.stdout), "good\n");

    fs::write(&input, "first,1\nsecond,2\nthird,3\n").expect("failed to write pipeline CSV input");
    fs::write(
        &source,
        format!(
            "rows is csv_rows(\"{}\")\n\
             [head, ...tail] is rows\n\
             total is pipeline:\n\
                 tail\n\
                 count\n\
             end\n\
             Sayln head[0]\n\
             Sayln total\n",
            input.display()
        ),
    )
    .expect("failed to write CSV suffix pipeline source");
    let pipeline = Command::new(env!("CARGO_BIN_EXE_simply"))
        .args(["run", source.to_str().expect("source path must be UTF-8")])
        .output()
        .expect("failed to execute CSV suffix pipeline");
    assert!(
        pipeline.status.success(),
        "{}",
        String::from_utf8_lossy(&pipeline.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&pipeline.stdout), "first\n2\n");

    fs::write(
        &source,
        format!(
            "mut head is list [\"old\"]\n\
         mut tail is csv_rows(\"{}\")\n\
         [head, ...tail] -> csv_rows(\"{}\")\n\
         total is pipeline:\n\
             tail\n\
             count\n\
         end\n\
         Sayln head[0]\n\
         Sayln total\n",
            input.display(),
            input.display()
        ),
    )
    .expect("failed to write CSV suffix assignment source");
    let reassigned = Command::new(env!("CARGO_BIN_EXE_simply"))
        .args(["run", source.to_str().expect("source path must be UTF-8")])
        .output()
        .expect("failed to execute CSV suffix assignment");
    assert!(
        reassigned.status.success(),
        "{}",
        String::from_utf8_lossy(&reassigned.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&reassigned.stdout), "first\n2\n");

    fs::write(&input, "bad\"quote,2\n").expect("failed to write malformed prefix");
    fs::write(
        &source,
        format!(
            "rows is csv_rows(\"{}\")\n\
             [head, ...tail] is rows\n\
             Sayln head[0]\n",
            input.display()
        ),
    )
    .expect("failed to write malformed prefix source");
    let malformed = Command::new(env!("CARGO_BIN_EXE_simply"))
        .args(["run", source.to_str().expect("source path must be UTF-8")])
        .output()
        .expect("failed to execute malformed CSV destructuring");
    assert!(!malformed.status.success());
    assert!(
        String::from_utf8_lossy(&malformed.stderr).contains("quote inside an unquoted field"),
        "{}",
        String::from_utf8_lossy(&malformed.stderr)
    );

    fs::write(
        &source,
        format!(
            "fn identity(value):\n\
                 return value\n\
             end\n\
             mut head is list [\"old\"]\n\
             mut tail is list [9]\n\
             try:\n\
                 [head, ...tail] -> identity(csv_rows(\"{}\"))\n\
             catch error:\n\
                 Sayln head[0]\n\
                 Sayln tail[0]\n\
             end\n",
            input.display()
        ),
    )
    .expect("failed to write malformed CSV assignment source");
    let failed_assignment = Command::new(env!("CARGO_BIN_EXE_simply"))
        .args(["run", source.to_str().expect("source path must be UTF-8")])
        .output()
        .expect("failed to execute malformed CSV assignment");
    assert!(
        failed_assignment.status.success(),
        "{}",
        String::from_utf8_lossy(&failed_assignment.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&failed_assignment.stdout),
        "old\n9\n"
    );

    fs::remove_dir_all(root).expect("failed to clean up CSV destructuring directory");
}

#[test]
fn destructuring_with_unknown_rhs_type_is_checked_at_runtime() {
    let id = TEMP_SOURCE_ID.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!("simply-destructure-unknown-{id}"));
    fs::create_dir_all(&root).expect("failed to create unknown type test directory");
    let source = root.join("main.si");
    fs::write(
        &source,
        "fn identity(value):\n\
             return value\n\
         end\n\
         source is identity(42)\n\
         (first, second) is source\n",
    )
    .expect("failed to write unknown type destructuring source");

    let output = Command::new(env!("CARGO_BIN_EXE_simply"))
        .args(["run", source.to_str().expect("source path must be UTF-8")])
        .output()
        .expect("failed to run unknown type destructuring");
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("destructuring target does not match"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    fs::remove_dir_all(root).expect("failed to clean up unknown type test directory");
}
