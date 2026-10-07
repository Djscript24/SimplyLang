mod common;
use common::*;
use std::fs;

#[test]
fn identifier_patterns_bind_the_whole_value_and_are_exhaustive() {
    let source = "value is 7\n\
                  result is match value:\n\
                      answer:\n    answer\n\
                  end\n\
                  Sayln result\n";
    let (valid, _, error) = check_source(source);
    assert!(valid, "identifier match did not type-check: {error}");
    let (success, output) = run_source_stdout(source);
    assert!(success, "identifier pattern failed: {output}");
    assert_eq!(output, "7\n");
}

#[test]
fn parses_and_matches_closed_open_negative_and_zero_width_int_ranges() {
    for pattern in ["0..10", "-10..10", "10..", "..10", "-10..", "..-10", "0..0"] {
        let source = format!(
            "value is 0\nmatch value:\n    {pattern}:\n        \"matched\"\n    _:\n        \"other\"\nend\n"
        );
        let (valid, _, error) = check_source(&source);
        assert!(
            valid,
            "range pattern {pattern} did not parse/type-check: {error}"
        );
    }

    for (value, pattern, expected) in [
        (0, "0..10", "matched"),
        (10, "0..10", "matched"),
        (-1, "0..10", "other"),
        (11, "0..10", "other"),
        (-5, "-10..-5", "matched"),
        (-4, "-10..-5", "other"),
        (10, "10..", "matched"),
        (9, "10..", "other"),
        (-10, "..-10", "matched"),
        (-9, "..-10", "other"),
        (0, "0..0", "matched"),
        (1, "0..0", "other"),
    ] {
        let source = format!(
            "value is {value}\nresult is match value:\n    {pattern}:\n        \"matched\"\n    _:\n        \"other\"\nend\nSayln result\n"
        );
        let (success, output) = run_source_stdout(&source);
        assert!(success, "range {pattern} failed for {value}: {output}");
        assert_eq!(output, format!("{expected}\n"), "{pattern} for {value}");
    }
}

#[test]
fn rejects_invalid_int_ranges_and_non_int_scrutinees() {
    for (pattern, expected) in [
        ("..", "range pattern must have at least one bound"),
        ("10..0", "lower bound must not exceed upper bound"),
        ("0.0..1.0", "range pattern bounds must be Int"),
        ("\"a\"..\"z\"", "range pattern bounds must be Int"),
        ("false..true", "range pattern bounds must be Int"),
        ("foo..10", "range pattern bounds must be literal values"),
        ("10..foo", "range pattern bounds must be literal values"),
        ("x..y", "range pattern bounds must be literal values"),
        (
            "0..some_function()",
            "range pattern bounds must be literal values",
        ),
    ] {
        let source = format!(
            "value is 1\nmatch value:\n    {pattern}:\n        1\n    _:\n        0\nend\n"
        );
        let (success, _, error) = check_source(&source);
        assert!(!success, "invalid range pattern {pattern} was accepted");
        assert!(error.contains(expected), "missing {expected:?}: {error}");
    }

    let source =
        "value is \"hello\"\nmatch value:\n    0..10:\n        1\n    _:\n        0\nend\n";
    let (success, _, error) = check_source(source);
    assert!(!success);
    assert!(
        error.contains("range pattern cannot match value of type String"),
        "{error}"
    );
}

#[test]
fn int_interval_analysis_checks_exhaustiveness_and_unreachable_ranges() {
    for patterns in [
        vec!["..0", "1.."],
        vec!["..10", "11.."],
        vec!["..-1", "0..10", "11.."],
        vec!["..-10", "-9..9", "10.."],
        vec!["..-1", "0..10", "11..20", "21.."],
    ] {
        let arms = patterns
            .iter()
            .map(|pattern| format!("    {pattern}:\n        1\n"))
            .collect::<String>();
        let source = format!("value is 7\nmatch value:\n{arms}end\n");
        let (valid, _, error) = check_source(&source);
        assert!(
            valid,
            "complete interval coverage failed: {error}\n{source}"
        );
    }

    for patterns in [["0..10", ""], ["..0", "2.."], ["..-2", "0.."]] {
        let arms = patterns
            .iter()
            .filter(|pattern| !pattern.is_empty())
            .map(|pattern| format!("    {pattern}:\n        1\n"))
            .collect::<String>();
        let source = format!("value is 7\nmatch value:\n{arms}end\n");
        let (valid, _, error) = check_source(&source);
        assert!(!valid, "incomplete interval coverage passed: {source}");
        assert!(error.contains("non-exhaustive match"), "{error}");
    }

    let range_with_wildcard =
        "value is 7\nmatch value:\n    0..10:\n        1\n    _:\n        0\nend\n";
    let (valid, _, error) = check_source(range_with_wildcard);
    assert!(valid, "range with wildcard should be exhaustive: {error}");

    for (arms, message) in [
        (
            "    0..10:\n        1\n    0..10:\n        2\n    _:\n        0\n",
            "unreachable match pattern",
        ),
        (
            "    0..100:\n        1\n    20..30:\n        2\n",
            "unreachable match pattern",
        ),
        (
            "    10..:\n        1\n    20..:\n        2\n    _:\n        0\n",
            "unreachable match pattern",
        ),
        (
            "    0..10:\n        1\n    5:\n        2\n    _:\n        0\n",
            "unreachable match pattern",
        ),
        (
            "    0:\n        1\n    0..10:\n        2\n    _:\n        0\n",
            "",
        ),
        (
            "    0..10:\n        1\n    5..20:\n        2\n    _:\n        0\n",
            "",
        ),
    ] {
        let source = format!("value is 7\nmatch value:\n{arms}end\n");
        let (valid, _, error) = check_source(&source);
        if message.is_empty() {
            assert!(valid, "useful overlapping range was rejected: {error}");
        } else {
            assert!(!valid, "redundant interval passed checking: {source}");
            assert!(error.contains(message), "{error}");
        }
    }
}

#[test]
fn range_patterns_compose_with_or_guards_and_nested_patterns() {
    let source = "enum Result:\n    Ok as Int\n    Error as String\nend\n\
                  type Person:\n    age as Int\n    name as String\nend\n\
                  enum_value is Result::Ok(7)\n\
                  tuple is (7, 25)\n\
                  person is Person(17, \"Ada\")\n\
                  enum_result is match enum_value:\n\
                      Result::Ok(..-1):\n    \"negative\"\n\
                      Result::Ok(0..10):\n    \"small\"\n\
                      Result::Ok(11..):\n    \"large\"\n\
                      Result::Error(_):\n    \"error\"\n\
                  end\n\
                  tuple_result is match tuple:\n\
                      (0..10, 20..30):\n    \"tuple\"\n\
                      _:\n    \"other\"\n\
                  end\n\
                  person_result is match person:\n\
                      Person(..-1, _):\n    \"negative age\"\n\
                      Person(0..17, name):\n    name\n\
                      Person(18.., _):\n    \"adult\"\n\
                  end\n\
                  or_result is match 12:\n\
                      0..5 | 10..15:\n    \"selected\"\n\
                      _:\n    \"other\"\n\
                  end\n\
                  guarded_result is match 7:\n\
                      0..10 if false:\n    \"guarded\"\n\
                      0..10:\n    \"fallback\"\n\
                      _:\n    \"other\"\n\
                  end\n\
                  Sayln enum_result\nSayln tuple_result\nSayln person_result\nSayln or_result\nSayln guarded_result\n";
    let (valid, _, error) = check_source(source);
    assert!(valid, "nested range patterns failed checking: {error}");
    let (success, output) = run_source_stdout(source);
    assert!(success, "nested range patterns failed at runtime: {output}");
    assert_eq!(output, "small\ntuple\nAda\nselected\nfallback\n");

    let overlapping_or =
        "value is 12\nmatch value:\n    0..10 | 5..15:\n        1\n    _:\n        0\nend\n";
    let (valid, _, error) = check_source(overlapping_or);
    assert!(
        valid,
        "partially overlapping OR ranges were rejected: {error}"
    );

    let literal_or_range = "result is match 12:\n    0 | 10..15:\n        \"selected\"\n    _:\n        \"other\"\nend\nSayln result\n";
    let (valid, _, error) = check_source(literal_or_range);
    assert!(valid, "literal/range OR-pattern was rejected: {error}");
    let (success, output) = run_source_stdout(literal_or_range);
    assert!(success, "literal/range OR-pattern failed: {output}");
    assert_eq!(output, "selected\n");

    let redundant_or =
        "value is 12\nmatch value:\n    0..10 | 5..8:\n        1\n    _:\n        0\nend\n";
    let (valid, _, error) = check_source(redundant_or);
    assert!(!valid);
    assert!(
        error.contains("redundant OR-pattern alternative"),
        "{error}"
    );

    let guarded_only = "value is 7\nmatch value:\n    0..10 if true:\n        1\nend\n";
    let (valid, _, error) = check_source(guarded_only);
    assert!(!valid);
    assert!(error.contains("non-exhaustive match"), "{error}");
}

#[test]
fn fixed_length_sequence_patterns_match_recursively_and_bind_transactionally() {
    for (value, expected) in [
        ("[]", "empty"),
        ("[1]", "one"),
        ("[1, 2]", "two"),
        ("[1, 2, 3]", "exact"),
        ("[1, 2, 3, 4]", "other"),
    ] {
        let source = format!(
            "numbers is {value}\nresult is match numbers:\n    []:\n        \"empty\"\n    [1]:\n        \"one\"\n    [1, 2, 3]:\n        \"exact\"\n    [first, second]:\n        \"two\"\n    _:\n        \"other\"\nend\nSayln result\n"
        );
        let (valid, _, error) = check_source(&source);
        assert!(valid, "sequence pattern failed to check: {error}");
        let (success, output) = run_source_stdout(&source);
        assert!(success, "sequence matching failed: {output}");
        assert_eq!(output, format!("{expected}\n"), "{value}");
    }

    let source = "values is [[1, 2], [3, 4]]\n\
                  result is match values:\n\
                      [[1, 2], [3, 4]]:\n    \"nested\"\n\
                      _:\n    \"other\"\n\
                  end\n\
                  Sayln result\n";
    let (success, output) = run_source_stdout(source);
    assert!(success, "nested sequence patterns failed: {output}");
    assert_eq!(output, "nested\n");

    let list_source = "values is list [1, 2]\nresult is match values:\n    [first, _]:\n        first\nend\nSayln result\n";
    let (success, output) = run_source_stdout(list_source);
    assert!(success, "sequence pattern failed on a List: {output}");
    assert_eq!(output, "1\n");

    let range_source = "values is range(1, 4)\nresult is match values:\n    [1, 2, 3]:\n        \"range\"\n    _:\n        \"other\"\nend\nSayln result\n";
    let (success, output) = run_source_stdout(range_source);
    assert!(
        success,
        "sequence pattern failed on a lazy integer range: {output}"
    );
    assert_eq!(output, "range\n");

    let open_range_source = "values is [1, 2]\nresult is match values:\n    [1.., 0..]:\n        \"open\"\n    _:\n        \"other\"\nend\nSayln result\n";
    let (success, output) = run_source_stdout(open_range_source);
    assert!(success, "open range in sequence pattern failed: {output}");
    assert_eq!(output, "open\n");

    let bindings_are_transactional = "values is [42, 20]\nresult is match values:\n    [first, 10]:\n        first\n    [first, second]:\n        first + second\nend\nSayln result\n";
    let (success, output) = run_source_stdout(bindings_are_transactional);
    assert!(
        success,
        "failed nested sequence match leaked bindings: {output}"
    );
    assert_eq!(output, "62\n");
}

#[test]
fn csv_stream_sequence_patterns_match_exact_lengths_without_materializing_the_stream() {
    let id = TEMP_SOURCE_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!(
        "simply-csv-sequence-patterns-{}-{id}",
        std::process::id()
    ));
    fs::create_dir_all(&root).expect("failed to create CSV sequence test directory");

    for (name, contents, pattern, expected) in [
        ("empty", "", "[]", "matched"),
        ("empty-single", "", "[row]", "fallback"),
        ("one", "Ada\n", "[row]", "matched"),
        ("one-multi", "Ada\n", "[row1, row2]", "fallback"),
        ("one-wildcard", "Ada\n", "[_]", "matched"),
        ("many", "Ada\nLin\n", "[row1, row2]", "matched"),
        ("many-single", "Ada\nLin\n", "[row]", "fallback"),
        ("nested", "Ada,30\n", "[[_, _]]", "matched"),
    ] {
        let path = root.join(format!("{name}.csv"));
        fs::write(&path, contents).expect("failed to write CSV sequence fixture");
        let source = format!(
            "rows as CsvStream is csv_rows({:?})\n\
             result is match rows:\n\
                 {pattern}:\n\
                     \"matched\"\n\
                 _:\n\
                     \"fallback\"\n\
             end\n\
             Sayln result\n",
            path.to_str().expect("CSV fixture path must be UTF-8")
        );
        let (valid, _, error) = check_source(&source);
        assert!(valid, "CSV sequence failed to type-check: {error}");
        let (success, output) = run_source_stdout(&source);
        assert!(success, "CSV sequence execution failed: {output}");
        assert_eq!(output, format!("{expected}\n"), "{name}");
    }

    fs::remove_dir_all(root).expect("failed to remove CSV sequence test directory");
}

#[test]
fn rest_sequence_patterns_bind_typed_suffixes_and_match_minimum_lengths() {
    for (value, expected) in [
        ("[]", "empty\n"),
        ("[10]", "10\nArray\n[]\n"),
        ("[10, 20, 30]", "10\nArray\n[20, 30]\n"),
    ] {
        let source = format!(
            "values is {value}\n\
             match values:\n\
                 []:\n\
                     Sayln \"empty\"\n\
                 [head, ...tail]:\n\
                     Sayln head\n\
                     Sayln type_of(tail)\n\
                     Sayln tail\n\
             end\n"
        );
        let (valid, _, error) = check_source(&source);
        assert!(valid, "rest sequence did not prove exhaustive: {error}");
        let (success, output) = run_source_stdout(&source);
        assert!(success, "rest sequence failed at runtime: {output}");
        assert_eq!(output, expected, "{value}");
    }

    let list_source = "fn expect_list(ref values as List[Int]) gives Int:\n\
                           return length(values)\n\
                       end\n\
                       values is list [1, 2, 3]\n\
                       match values:\n\
                           []:\n\
                               Sayln \"empty\"\n\
                           [first, second, ...tail]:\n\
                               Sayln expect_list(ref tail)\n\
                               Sayln type_of(tail)\n\
                               Sayln tail\n\
                           _:\n\
                               Sayln \"other\"\n\
                       end\n";
    let (valid, _, error) = check_source(list_source);
    assert!(
        valid,
        "List rest binding had the wrong static type: {error}"
    );
    let (success, output) = run_source_stdout(list_source);
    assert!(success, "List rest binding failed: {output}");
    assert_eq!(output, "1\nList\n[3]\n");

    let empty_list_suffix = "values is list [10, 20]\n\
                             match values:\n\
                                 [first, second, ...tail]:\n\
                                     Sayln type_of(tail)\n\
                                     Sayln tail\n\
                             end\n";
    let (success, output) = run_source_stdout(empty_list_suffix);
    assert!(success, "empty List suffix failed: {output}");
    assert_eq!(output, "List\n[]\n");

    let whole_source = "values is [1, 2]\n\
                        result is match values:\n\
                            [...everything]:\n    everything\n\
                        end\n\
                        Sayln result\n";
    let (valid, _, error) = check_source(whole_source);
    assert!(
        valid,
        "empty-prefix rest did not cover every length: {error}"
    );
    let (success, output) = run_source_stdout(whole_source);
    assert!(success, "empty-prefix rest did not match: {output}");
    assert_eq!(output, "[1, 2]\n");

    let range_source = "values is range(1, 5)\n\
                        match values:\n\
                            [first, ...tail]:\n\
                                Sayln type_of(tail)\n\
                                Sayln tail\n\
                        end\n";
    let (success, output) = run_source_stdout(range_source);
    assert!(success, "range rest binding failed: {output}");
    assert_eq!(output, "Range\n[2, 3, 4]\n");

    let typed_array_suffix = "fn expect_array(ref values as Array[Int]) gives Int:\n\
                                  return length(values)\n\
                              end\n\
                              values is [10, 20]\n\
                              match values:\n\
                                  [first, second, ...tail]:\n\
                                      Sayln expect_array(ref tail)\n\
                                      Sayln type_of(tail)\n\
                                      Sayln tail\n\
                                  _:\n\
                                      Sayln \"other\"\n\
                              end\n";
    let (valid, _, error) = check_source(typed_array_suffix);
    assert!(
        valid,
        "Array rest binding had the wrong static type: {error}"
    );
    let (success, output) = run_source_stdout(typed_array_suffix);
    assert!(success, "typed Array suffix failed: {output}");
    assert_eq!(output, "0\nArray\n[]\n");

    let typed_range_suffix = "values is range(1, 3)\n\
                              match values:\n\
                                  [first, second, ...tail]:\n\
                                      Sayln type_of(tail)\n\
                                      Sayln tail\n\
                              end\n";
    let (success, output) = run_source_stdout(typed_range_suffix);
    assert!(success, "empty Range suffix failed: {output}");
    assert_eq!(output, "Range\n[]\n");
}

#[test]
fn rest_sequence_patterns_compose_with_nested_patterns_ranges_or_and_guards() {
    let nested = "type Person:\n    name as String\n    age as Int\nend\n\
                  people is [Person(\"Ada\", 17), Person(\"Lin\", 20)]\n\
                  result is match people:\n\
                      [Person(name, 0..18), ...others]:\n    name + type_of(others)\n\
                      _:\n    \"fallback\"\n\
                  end\n\
                  Sayln result\n";
    let (valid, _, error) = check_source(nested);
    assert!(valid, "nested rest pattern failed checking: {error}");
    let (success, output) = run_source_stdout(nested);
    assert!(success, "nested rest pattern failed: {output}");
    assert_eq!(output, "AdaArray\n");

    let ranged = "values is [-1, 4, 12]\n\
                  match values:\n\
                      []:\n    Sayln \"empty\"\n\
                      [..-1, ...rest]:\n    Sayln \"negative\"\n\
                      [0..10, ...rest]:\n    Sayln \"small\"\n\
                      [11.., ...rest]:\n    Sayln \"large\"\n\
                  end\n";
    let (valid, _, error) = check_source(ranged);
    assert!(valid, "range/rest coverage was rejected: {error}");
    let (success, output) = run_source_stdout(ranged);
    assert!(success, "range/rest pattern failed: {output}");
    assert_eq!(output, "negative\n");

    let or_guarded = "values is [1, 2]\n\
                      result is match values:\n\
                          [0, ...rest] | [1, ...rest] if rest == [2]:\n    rest\n\
                          _:\n    array []\n\
                      end\n\
                      Sayln result\n";
    let (valid, _, error) = check_source(or_guarded);
    assert!(valid, "OR/rest guard failed checking: {error}");
    let (success, output) = run_source_stdout(or_guarded);
    assert!(success, "OR/rest guard failed at runtime: {output}");
    assert_eq!(output, "[2]\n");

    let nested_or = "values is [[1, 2], [3]]\n\
                     result is match values:\n\
                         []:\n    \"empty\"\n\
                         [[0 | 1, ...inner], ...outer]:\n    \"matched\"\n\
                         _:\n    \"fallback\"\n\
                     end\n\
                     Sayln result\n";
    let (valid, _, error) = check_source(nested_or);
    assert!(valid, "nested OR with rest did not type-check: {error}");
    let (success, output) = run_source_stdout(nested_or);
    assert!(success, "nested OR with rest failed: {output}");
    assert_eq!(output, "matched\n");

    let nested_rest = "fn expect_int_array(ref values as Array[Int]) gives Int:\n\
                           return length(values)\n\
                       end\n\
                       fn expect_nested_array(ref values as Array[Array[Int]]) gives Int:\n\
                           return length(values)\n\
                       end\n\
                       values is [[1, 2, 3], [4, 5], [6]]\n\
                       match values:\n\
                           []:\n    Sayln \"empty\"\n\
                           [[], ...outer]:\n    Sayln \"empty inner sequence\"\n\
                           [[x, ...inner], ...outer]:\n\
                               Sayln x\n\
                               Sayln expect_int_array(ref inner)\n\
                               Sayln expect_nested_array(ref outer)\n\
                               Sayln inner\n\
                               Sayln outer\n\
                       end\n";
    let (valid, _, error) = check_source(nested_rest);
    assert!(valid, "nested rest levels failed type checking: {error}");
    let (success, output) = run_source_stdout(nested_rest);
    assert!(success, "nested rest levels failed: {output}");
    assert_eq!(output, "1\n2\n2\n[2, 3]\n[[4, 5], [6]]\n");
}

#[test]
fn rest_patterns_example_is_exhaustive_and_runs() {
    let source = include_str!("../examples/15-patterns/rest-patterns.si");
    let (valid, _, error) = check_source(source);
    assert!(valid, "rest-patterns example failed checking: {error}");

    let output = run_example("examples/15-patterns/rest-patterns.si");
    assert_eq!(output, "10\n[20, 30]\n1\n2\n[[3, 4]]\n");
}

#[test]
fn rest_sequence_usefulness_and_exhaustiveness_respect_lengths_and_prefixes() {
    let duplicate_prefix = "values is [1, 2]\n\
                            match values:\n\
                                [head, ...tail]:\n    1\n\
                                [other, ...rest]:\n    2\n\
                            end\n";
    let (valid, _, error) = check_source(duplicate_prefix);
    assert!(!valid, "a redundant rest pattern was accepted");
    assert!(error.contains("unreachable match pattern"), "{error}");

    let nonzero_prefix_remains_useful = "values is [0, 2]\n\
                                         match values:\n\
                                             [0, ...tail]:\n    1\n\
                                             [head, ...rest]:\n    2\n\
                                             _:\n    0\n\
                                         end\n";
    let (valid, _, error) = check_source(nonzero_prefix_remains_useful);
    assert!(
        valid,
        "a rest pattern with an uncovered prefix value was rejected: {error}"
    );

    let prefix_still_useful = "values is [0, 2]\n\
                               match values:\n\
                                   []:\n    0\n\
                                   [0, ...tail]:\n    1\n\
                                   [1.., ...rest]:\n    2\n\
                                   _:\n    3\n\
                               end\n";
    let (valid, _, error) = check_source(prefix_still_useful);
    assert!(valid, "disjoint interval prefix was rejected: {error}");

    let int_domain = "values is [0, 10]\n\
                      match values:\n\
                          []:\n    0\n\
                          [..-1, ...negative]:\n    1\n\
                          [0..10, ...middle]:\n    2\n\
                          [11.., ...large]:\n    3\n\
                      end\n";
    let (valid, _, error) = check_source(int_domain);
    assert!(valid, "complete rest-pattern Int coverage failed: {error}");

    let guarded = "values is [1, 2]\n\
                   result is match values:\n\
                       []:\n    array []\n\
                       [head, ...tail] if head > 10:\n    tail\n\
                       [head, ...tail]:\n    tail\n\
                   end\n\
                   Sayln result\n";
    let (valid, _, error) = check_source(guarded);
    assert!(valid, "guarded rest pattern was mishandled: {error}");
    let (success, output) = run_source_stdout(guarded);
    assert!(success, "guarded rest pattern failed: {output}");
    assert_eq!(output, "[2]\n");

    let guarded_tail = "values is [1, 2]\n\
                        match values:\n\
                            []:\n    Sayln \"empty\"\n\
                            [head, ...tail] if length(tail) > 0:\n    Sayln head\n\
                            [head, ...tail]:\n    Sayln type_of(tail)\n\
                        end\n";
    let (valid, _, error) = check_source(guarded_tail);
    assert!(valid, "guard could not inspect the rest binding: {error}");
    let (success, output) = run_source_stdout(guarded_tail);
    assert!(success, "guard on rest binding failed: {output}");
    assert_eq!(output, "1\n");
}

#[test]
fn rest_sequence_exhaustiveness_matrix_covers_minimum_lengths_and_fixed_arms() {
    let exhaustive_cases = [
        "match xs:\n    [...rest]:\n        1\nend\n",
        "match xs:\n    []:\n        0\n    [head, ...rest]:\n        1\nend\n",
        "match xs:\n    []:\n        0\n    [only]:\n        1\n    [head, ...rest]:\n        2\nend\n",
        "match xs:\n    []:\n        0\n    [only]:\n        1\n    [head, ...rest]:\n        2\nend\n",
        "match xs:\n    []:\n        0\n    [first, second]:\n        1\n    [head, ...rest]:\n        2\nend\n",
        "match xs:\n    []:\n        0\n    [only]:\n        1\n    [head, ...rest]:\n        2\nend\n",
    ];
    for source in exhaustive_cases {
        let source = format!("xs is [1, 2]\n{source}");
        let (valid, _, error) = check_source(&source);
        assert!(
            valid,
            "expected rest matrix to be exhaustive: {error}\n{source}"
        );
    }

    for source in [
        "match xs:\n    [head, ...rest]:\n        1\nend\n",
        "match xs:\n    [first, second, ...rest]:\n        1\nend\n",
    ] {
        let source = format!("xs is [1, 2]\n{source}");
        let (valid, _, error) = check_source(&source);
        assert!(!valid, "incomplete rest matrix was accepted: {source}");
        assert!(error.contains("non-exhaustive match"), "{error}");
    }

    let source = "xs is [1, 2]\n\
        match xs:\n\
            [head, ...rest]:\n                1\n\
            [first, second]:\n                2\n\
            _:\n                0\n\
        end\n";
    let (valid, _, error) = check_source(source);
    assert!(
        !valid,
        "unreachable fixed/rest matrix was accepted: {source}"
    );
    assert!(error.contains("unreachable match pattern"), "{error}");

    let empty_after_nonempty_rest = "xs is [1, 2]\n\
        match xs:\n\
            [head, ...rest]:\n                1\n\
            []:\n                0\n\
        end\n";
    let (valid, _, error) = check_source(empty_after_nonempty_rest);
    assert!(
        valid,
        "an empty arm after a nonempty rest prefix should remain useful: {error}"
    );

    let complete_integer_prefixes = "xs is [0, 2]\n\
        match xs:\n\
            []:\n                0\n\
            [..0, ...negative]:\n                1\n\
            [1.., ...nonnegative]:\n                2\n\
        end\n";
    let (valid, _, error) = check_source(complete_integer_prefixes);
    assert!(valid, "open Int prefixes did not cover the domain: {error}");

    let disjoint_or_union = "xs is [15, 2]\n\
        match xs:\n\
            []:\n                0\n\
            [..-1, ...negative]:\n                1\n\
            [0..10, ...first]:\n                1\n\
            [20..30, ...second]:\n                2\n\
            [11..19, ...gap]:\n                3\n\
            [31.., ...large]:\n                4\n\
        end\n";
    let (valid, _, error) = check_source(disjoint_or_union);
    assert!(
        valid,
        "rest interval union failed to preserve gaps: {error}"
    );

    let rest_or_union = "xs is [15, 2]\n\
        match xs:\n\
            []:\n                0\n\
            [..-1, ...negative]:\n                1\n\
            [0..10, ...rest] | [20..30, ...rest]:\n                2\n\
            [11..19, ...gap]:\n                3\n\
            [31.., ...large]:\n                4\n\
        end\n";
    let (valid, _, error) = check_source(rest_or_union);
    assert!(
        valid,
        "OR rest union did not preserve its interval gap: {error}"
    );

    let redundant_or_overlap = "xs is [7, 2]\n\
        match xs:\n\
            [0..10, ...rest] | [5..15, ...rest]:\n                1\n\
            [7..9, ...covered]:\n                2\n\
            _:\n                0\n\
        end\n";
    let (valid, _, error) = check_source(redundant_or_overlap);
    assert!(!valid, "covered range rest arm was considered useful");
    assert!(error.contains("unreachable match pattern"), "{error}");
}

#[test]
fn rest_patterns_match_generated_short_sequence_lengths_and_prefixes() {
    for length in 0..=4 {
        let values = (0..length)
            .map(|value| value.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        for prefix_length in 0..=3 {
            let prefix = vec!["_"; prefix_length].join(", ");
            let pattern = if prefix.is_empty() {
                "[...rest]".to_owned()
            } else {
                format!("[{prefix}, ...rest]")
            };
            let fallback = if prefix_length == 0 {
                String::new()
            } else {
                "    _:\n        -1\n".to_owned()
            };
            let source = format!(
                "values is [{values}]\n\
                 result is match values:\n\
                     {pattern}:\n\
                         {prefix_length}\n\
                 {fallback}\
                 end\n\
                 Sayln result\n"
            );
            let (valid, _, error) = check_source(&source);
            assert!(
                valid,
                "generated rest case length={length}, prefix={prefix_length} failed: {error}\n{source}"
            );
            let (success, output) = run_source_stdout(&source);
            assert!(
                success,
                "generated rest case length={length}, prefix={prefix_length} failed at runtime: {output}"
            );
            let expected = if length >= prefix_length {
                prefix_length as i64
            } else {
                -1
            };
            assert_eq!(output, format!("{expected}\n"), "{source}");
        }
    }
}

#[test]
fn rest_bindings_do_not_leak_after_failed_prefixes_or_or_alternatives() {
    let failed_prefix = "values is [42, 20, 30]\n\
                         result is match values:\n\
                             []:\n    0\n\
                             [only]:\n    only\n\
                             [first, 10, ...tail]:\n    first\n\
                             [first, second, ...tail]:\n    first + second\n\
                         end\n\
                         Sayln result\n";
    let (valid, _, error) = check_source(failed_prefix);
    assert!(
        valid,
        "failed-prefix rest pattern did not type-check: {error}"
    );
    let (success, output) = run_source_stdout(failed_prefix);
    assert!(success, "failed-prefix rest pattern failed: {output}");
    assert_eq!(output, "62\n");

    let failed_nested_prefix = "values is [[1, 2, 3], [4]]\n\
                                result is match values:\n\
                                    []:\n    \"empty\"\n\
                                    [[first, 99, ...inner], ...outer]:\n    \"incorrect\"\n\
                                    [fallback, ...remaining]:\n    type_of(fallback)\n\
                                end\n\
                                Sayln result\n";
    let (valid, _, error) = check_source(failed_nested_prefix);
    assert!(valid, "nested failure fallback did not type-check: {error}");
    let (success, output) = run_source_stdout(failed_nested_prefix);
    assert!(success, "nested failure leaked or failed: {output}");
    assert_eq!(output, "Array\n");

    let inconsistent_or = "values is [1, 2]\n\
                           match values:\n\
                               [0, ...rest] | [1, ...other]:\n    rest\n\
                               _:\n    array []\n\
                           end\n";
    let (valid, _, error) = check_source(inconsistent_or);
    assert!(
        !valid,
        "OR alternatives with different rest names were accepted"
    );
    assert!(
        error.contains("OR-pattern alternatives must bind the same names"),
        "{error}"
    );
}

#[test]
fn csv_stream_rest_bindings_remain_lazy_and_start_at_the_suffix() {
    let id = TEMP_SOURCE_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!(
        "simply-csv-rest-patterns-{}-{id}",
        std::process::id()
    ));
    fs::create_dir_all(&root).expect("failed to create CSV rest test directory");

    for (name, contents, expected) in [
        ("empty", "", "[]\n"),
        ("one", "Ada\n", "[]\n"),
        ("many", "Ada\nLin\nGrace\n", "[Lin]\n"),
    ] {
        let path = root.join(format!("{name}.csv"));
        fs::write(&path, contents).expect("failed to write CSV rest fixture");
        let source = format!(
            "rows as CsvStream is csv_rows({:?})\n\
             result is match rows:\n\
                 []:\n    list []\n\
                 [first, ...tail]:\n\
                     match tail:\n\
                         []:\n    list []\n\
                         [next, ...remaining]:\n    next\n\
                     end\n\
             end\n\
             Sayln result\n",
            path.to_str().expect("CSV fixture path must be UTF-8")
        );
        let (valid, _, error) = check_source(&source);
        assert!(valid, "CSV rest pattern failed to type-check: {error}");
        let (success, output) = run_source_stdout(&source);
        assert!(success, "CSV rest pattern failed: {output}");
        assert_eq!(output, expected, "{name}");
    }

    let pipeline_path = root.join("pipeline.csv");
    fs::write(&pipeline_path, "Ada\nLin\nGrace\n").expect("failed to write CSV pipeline fixture");
    let pipeline_source = format!(
        "rows as CsvStream is csv_rows({:?})\n\
         tail is match rows:\n\
             []:\n    rows\n\
             [first, ...rest]:\n    rest\n\
         end\n\
         flow result from tail:\n\
             count\n\
         end\n\
         Sayln result\n",
        pipeline_path
            .to_str()
            .expect("CSV fixture path must be UTF-8")
    );
    let (valid, _, error) = check_source(&pipeline_source);
    assert!(valid, "CSV suffix pipeline failed to type-check: {error}");
    let (success, output) = run_source_stdout(&pipeline_source);
    assert!(success, "CSV suffix pipeline failed: {output}");
    assert_eq!(output, "2\n");

    let repeated_pipeline_source = format!(
        "rows as CsvStream is csv_rows({:?})\n\
         tail is match rows:\n\
             []:\n    rows\n\
             [first, ...rest]:\n    rest\n\
         end\n\
         flow first_count from tail:\n             count\n         end\n\
         flow second_count from tail:\n             count\n         end\n\
         Sayln first_count\nSayln second_count\n",
        pipeline_path
            .to_str()
            .expect("CSV fixture path must be UTF-8")
    );
    let (success, output) = run_source_stdout(&repeated_pipeline_source);
    assert!(
        success,
        "independent suffix pipeline consumers failed: {output}"
    );
    assert_eq!(output, "2\n2\n");

    let full_suffix_source = format!(
        "rows as CsvStream is csv_rows({:?})\n\
         tail is match rows:\n\
             [...rest]:\n    rest\n\
         end\n\
         flow result from tail:\n             count\n         end\n\
         Sayln result\n",
        pipeline_path
            .to_str()
            .expect("CSV fixture path must be UTF-8")
    );
    let (success, output) = run_source_stdout(&full_suffix_source);
    assert!(success, "zero-prefix suffix stream failed: {output}");
    assert_eq!(output, "3\n");

    let two_record_suffix_source = format!(
        "rows as CsvStream is csv_rows({:?})\n\
         tail is match rows:\n\
             [first, second, ...rest]:\n    rest\n\
         end\n\
         flow result from tail:\n             count\n         end\n\
         Sayln result\n",
        pipeline_path
            .to_str()
            .expect("CSV fixture path must be UTF-8")
    );
    let (success, output) = run_source_stdout(&two_record_suffix_source);
    assert!(success, "two-record suffix stream failed: {output}");
    assert_eq!(output, "1\n");

    let malformed_path = root.join("malformed.csv");
    fs::write(&malformed_path, "Ada\n\"unterminated\n")
        .expect("failed to write malformed CSV fixture");
    let malformed_source = format!(
        "rows as CsvStream is csv_rows({:?})\n\
         result is match rows:\n\
             [first, ...tail]:\n    tail\n\
             _:\n    rows\n\
         end\n\
         flow count_rows from result:\n             count\n         end\n",
        malformed_path
            .to_str()
            .expect("CSV fixture path must be UTF-8")
    );
    let (success, error) = run_source(&malformed_source);
    assert!(!success, "malformed suffix CSV was silently treated as EOF");
    assert!(error.contains("unterminated quoted field"), "{error}");

    fs::remove_dir_all(root).expect("failed to remove CSV rest test directory");
}

#[test]
fn malformed_rest_sequence_patterns_report_parse_diagnostics() {
    for (pattern, expected) in [
        (
            "[...rest, head]",
            "rest pattern must be the final sequence element",
        ),
        (
            "[head, ...rest, next]",
            "rest pattern must be the final sequence element",
        ),
        (
            "[head, ...first, ...second]",
            "rest pattern must be the final sequence element",
        ),
        ("[head, ...]", "expected a binding name after `...`"),
        ("[head, ...1]", "expected a binding name after `...`"),
        ("[head, ...foo()]", "expected `]` after rest pattern"),
        ("[head, ..._]", "rest pattern must bind an identifier"),
    ] {
        let source = format!(
            "values is [1, 2]\nmatch values:\n    {pattern}:\n        1\n    _:\n        0\nend\n"
        );
        let (valid, _, error) = check_source(&source);
        assert!(!valid, "malformed rest pattern was accepted: {pattern}");
        assert!(error.contains(expected), "missing {expected:?}: {error}");
    }
}

#[test]
fn sequence_patterns_compose_with_enum_struct_tuple_and_range_patterns() {
    let source = "enum Result:\n    Ok as Int\n    Err as String\nend\n\
                  type Person:\n    name as String\n    age as Int\nend\n\
                  values is [Result::Ok(7), Result::Err(\"failed\")]\n\
                  people is [Person(\"Ada\", 17), Person(\"Lin\", 20)]\n\
                  pairs is [(1, 2), (3, 4)]\n\
                  enum_result is match values:\n\
                      [Result::Ok(0..10), Result::Err(_)]:\n    \"enum\"\n\
                      _:\n    \"other\"\n\
                  end\n\
                  person_result is match people:\n\
                      [Person(first_name, 0..17), Person(second_name, 18..)]:\n    first_name + second_name\n\
                      _:\n    \"other\"\n\
                  end\n\
                  tuple_result is match pairs:\n\
                      [(1, 2), (3, 4)]:\n    \"tuple\"\n\
                      _:\n    \"other\"\n\
                  end\n\
                  Sayln enum_result\nSayln person_result\nSayln tuple_result\n";
    let (valid, _, error) = check_source(source);
    assert!(
        valid,
        "nested sequence patterns did not type-check: {error}"
    );
    let (success, output) = run_source_stdout(source);
    assert!(success, "nested sequence patterns failed: {output}");
    assert_eq!(output, "enum\nAdaLin\ntuple\n");
}

#[test]
fn sequence_patterns_check_types_usefulness_exhaustiveness_and_guards() {
    for (source, expected) in [
        (
            "value is 42\nmatch value:\n    [1, 2]:\n        1\nend\n",
            "sequence pattern cannot match value of type Int",
        ),
        (
            "values is [1, 2]\nmatch values:\n    [\"a\", \"b\"]:\n        1\nend\n",
            "literal pattern of type String cannot match value of type Int",
        ),
        (
            "value is (1, 2)\nmatch value:\n    [1, 2]:\n        1\nend\n",
            "sequence pattern cannot match value of type Tuple",
        ),
    ] {
        let (success, _, error) = check_source(source);
        assert!(!success, "invalid sequence pattern passed checking");
        assert!(error.contains(expected), "missing {expected:?}: {error}");
    }

    for source in [
        "values is [1, 2]\nmatch values:\n    []:\n        1\n    _:\n        0\nend\n",
        "values is [1, 2]\nmatch values:\n    [1, 2]:\n        1\n    [3, 4]:\n        2\n    _:\n        0\nend\n",
    ] {
        let (success, _, error) = check_source(source);
        assert!(success, "wildcard did not close sequence coverage: {error}");
    }

    for source in [
        "values is [1, 2]\nmatch values:\n    [1, 2]:\n        1\nend\n",
        "values is [1, 2]\nmatch values:\n    []:\n        1\n    [first]:\n        first\nend\n",
    ] {
        let (success, _, error) = check_source(source);
        assert!(!success, "fixed-length patterns proved full exhaustiveness");
        assert!(error.contains("non-exhaustive match"), "{error}");
    }

    for source in [
        "values is [1, 2]\nmatch values:\n    _:\n        1\n    [1, 2]:\n        0\nend\n",
        "values is [1, 2]\nmatch values:\n    [1, 2]:\n        1\n    [1, 2]:\n        0\nend\n",
        "values is [[1, 2]]\nmatch values:\n    [[1, _]]:\n        1\n    [[1, 2]]:\n        0\n    _:\n        0\nend\n",
        "values is [[5, 1]]\nmatch values:\n    [[0..10, _]]:\n        1\n    [[5, _]]:\n        0\n    _:\n        0\nend\n",
    ] {
        let (success, _, error) = check_source(source);
        assert!(!success, "redundant sequence pattern passed checking");
        assert!(error.contains("unreachable match pattern"), "{error}");
    }

    let useful_range_element = "values is [[11, 1]]\nmatch values:\n    [[0..10, _]]:\n        1\n    [[11, _]]:\n        2\n    _:\n        0\nend\n";
    let (success, _, error) = check_source(useful_range_element);
    assert!(success, "useful nested range was rejected: {error}");

    let guarded = "values is [1, 2]\nmatch values:\n    [1, 2] if false:\n        1\n    [1, 2]:\n        2\n    _:\n        0\nend\n";
    let (success, _, error) = check_source(guarded);
    assert!(success, "guarded sequence caused false redundancy: {error}");

    let guarded_only = "values is [1, 2]\nmatch values:\n    [1, 2] if true:\n        1\nend\n";
    let (success, _, error) = check_source(guarded_only);
    assert!(!success);
    assert!(error.contains("non-exhaustive match"), "{error}");
}

#[test]
fn sequence_or_patterns_union_coverage_and_check_binding_compatibility() {
    let source = "result is match [3, 4]:\n\
                      [1, 2] | [3, 4]:\n    \"matched\"\n\
                      _:\n    \"other\"\n\
                  end\n\
                  Sayln result\n";
    let (success, output) = run_source_stdout(source);
    assert!(success, "sequence OR-pattern failed: {output}");
    assert_eq!(output, "matched\n");

    for (pattern, expected_error) in [
        ("[first, 2] | [first, 3]", None),
        ("[first, 1] | [_, 2]", Some("must bind the same names")),
        ("[1, 2] | [1, 2]", Some("redundant OR-pattern alternative")),
        ("[1, _] | [1, 2]", Some("redundant OR-pattern alternative")),
    ] {
        let source = format!(
            "values is [1, 2]\nmatch values:\n    {pattern}:\n        1\n    _:\n        0\nend\n"
        );
        let (success, _, error) = check_source(&source);
        match expected_error {
            Some(message) => {
                assert!(!success, "invalid OR sequence passed: {pattern}");
                assert!(error.contains(message), "{error}");
            }
            None => assert!(success, "compatible OR sequence failed: {error}"),
        }
    }
}

#[test]
fn matches_tuples_with_bindings_nested_tuples_and_wildcards() {
    let source = "pair is (10, 20)\n\
                  total is match pair:\n\
                      (left, right):\n    left + right\n\
                  end\n\
                  nested is (10, (20, 30))\n\
                  nested_sum is match nested:\n\
                      (first, (second, third)):\n    first + second + third\n\
                  end\n\
                  wildcard is match pair:\n\
                      (head, _):\n    head\n\
                  end\n\
                  Sayln total\nSayln nested_sum\nSayln wildcard\n";
    let (valid, _, error) = check_source(source);
    assert!(valid, "tuple patterns did not type-check: {error}");
    let (success, output) = run_source_stdout(source);
    assert!(success, "tuple matching failed: {output}");
    assert_eq!(output, "30\n60\n10\n");
}

#[test]
fn recursively_matches_enums_inside_tuples_and_tuple_enum_payloads() {
    let source = "enum Result:\n    Ok as Int\n    Error as String\nend\n\
                  value is (Result::Ok(42), Result::Error(\"failed\"))\n\
                  matched is match value:\n\
                      (Result::Ok(number), Result::Error(message)):\n    number\n\
                      _:\n    0\n\
                  end\n\
                  enum PairResult:\n    Ok as (Int, Int)\n    Error as String\nend\n\
                  pair is PairResult::Ok((10, 20))\n\
                  pair_sum is match pair:\n\
                      PairResult::Ok((left, right)):\n    left + right\n\
                      PairResult::Error(_):\n    0\n\
                  end\n\
                  Sayln matched\nSayln pair_sum\n";
    let (valid, _, error) = check_source(source);
    assert!(valid, "enum/tuple patterns did not type-check: {error}");
    let (success, output) = run_source_stdout(source);
    assert!(success, "recursive pattern matching failed: {output}");
    assert_eq!(output, "42\n30\n");
}

#[test]
fn supports_nested_enum_payload_patterns_and_payload_wildcards() {
    let source = "enum Inner:\n    Value as Int\n    Missing\nend\n\
                  enum Outer:\n    Wrapped as Inner\n    Empty\nend\n\
                  value is Outer::Wrapped(Inner::Value(17))\n\
                  message is match value:\n\
                      Outer::Wrapped(Inner::Value(number)):\n    number\n\
                      _:\n    0\n\
                  end\n\
                  ignored is match value:\n\
                      Outer::Wrapped(_):\n    1\n\
                      Outer::Empty:\n    0\n\
                  end\n\
                  Sayln message\nSayln ignored\n";
    let (valid, _, error) = check_source(source);
    assert!(valid, "nested enum patterns did not type-check: {error}");
    let (success, output) = run_source_stdout(source);
    assert!(success, "nested enum payload patterns failed: {output}");
    assert_eq!(output, "17\n1\n");
}

#[test]
fn nested_bindings_are_arm_local_and_names_can_be_reused() {
    let source = "enum Result:\n    Ok as (Int, Int)\n    Error as String\nend\n\
                  result is Result::Ok((4, 5))\n\
                  matched is match result:\n\
                      Result::Ok((value, other)):\n    value + other\n\
                      Result::Error(value):\n    0\n\
                  end\n\
                  Sayln matched\n\
                  Sayln value\n";
    let (success, error) = run_source(source);
    assert!(!success, "pattern bindings leaked outside the match arm");
    assert!(error.contains("unknown variable `value`"), "{error}");
}

#[test]
fn type_checks_recursive_patterns_and_rejects_incompatible_shapes() {
    let cases = [
        (
            "value is 1\nmatch value:\n    (item, _):\n        item\n    _:\n        0\nend\n",
            "tuple pattern cannot match value of type Int",
        ),
        (
            "value is (1, 2)\nmatch value:\n    (first, second, third):\n        first\n    _:\n        0\nend\n",
            "tuple pattern expects 3 elements, found 2",
        ),
        (
            "enum A:\n    Value as Int\nend\n\
             enum B:\n    Value as Int\nend\n\
             value is A::Value(1)\n\
             match value:\n    B::Value(item):\n        item\n    _:\n        0\nend\n",
            "pattern `B::Value` cannot match value of type A",
        ),
        (
            "enum Result:\n    Ok as Int\nend\n\
             value is Result::Ok(1)\n\
             match value:\n    Result::Ok((left, right)):\n        left\nend\n",
            "tuple pattern cannot match value of type Int",
        ),
    ];
    for (source, expected) in cases {
        let (success, _, error) = check_source(source);
        assert!(!success, "invalid pattern passed checking");
        assert!(
            error.contains(expected),
            "missing {expected:?}: {error}\n{}",
            run_source(source).1
        );
    }
}

#[test]
fn tuple_binding_patterns_are_exhaustive_and_later_wildcards_are_unreachable() {
    let source = "pair is (10, 20)\n\
                  result is match pair:\n\
                      (left, right):\n    left + right\n\
                  end\n\
                  Sayln result\n";
    let (success, output) = run_source_stdout(source);
    assert!(success, "irrefutable tuple pattern failed: {output}");
    assert_eq!(output, "30\n");

    let unreachable = "pair is (10, 20)\n\
                       match pair:\n\
                           (left, right):\n    1\n\
                           _:\n    0\n\
                       end\n";
    let (success, _, error) = check_source(unreachable);
    assert!(!success);
    assert!(error.contains("unreachable match pattern"), "{error}");
}

#[test]
fn struct_identity_survives_recursive_tuple_and_enum_destructuring() {
    let source = "type User:\n    name as String\nend\n\
                  on User receive rename(next as String):\n    name -> next\nend\n\
                  on User receive get_name:\n    return name\nend\n\
                  enum Entry:\n    Present as User\n    Missing\nend\n\
                  user is User(\"Ada\")\n\
                  value is (Entry::Present(user), 1)\n\
                  user :: rename(\"Grace\")\n\
                  result is match value:\n\
                      (Entry::Present(person), _):\n    person :: get_name\n\
                      _:\n    \"missing\"\n\
                  end\n\
                  Sayln result\n";
    let (success, output) = run_source_stdout(source);
    assert!(success, "recursive Struct extraction failed: {output}");
    assert_eq!(output, "Grace\n");
}

#[test]
fn matches_struct_fields_positionally_and_supports_field_wildcards() {
    let source = "type Person:\n    name as String\n    age as Int\nend\n\
                  person is Person(\"Andi\", 17)\n\
                  first is match person:\n    Person(name, age):\n        name\nend\n\
                  second is match person:\n    Person(_, age):\n        age\nend\n\
                  third is match person:\n    Person(name, _):\n        name\nend\n\
                  ignored is match person:\n    Person(_, _):\n        \"matched\"\nend\n\
                  Sayln first\nSayln second\nSayln third\nSayln ignored\n";
    let (valid, _, error) = check_source(source);
    assert!(valid, "struct patterns did not type-check: {error}");
    let (success, output) = run_source_stdout(source);
    assert!(success, "struct matching failed: {output}");
    assert_eq!(output, "Andi\n17\nAndi\nmatched\n");
}

#[test]
fn matches_named_struct_fields_partially_and_recursively() {
    let source = "type Address:\n    city as String\n    country as String\nend\n\
                  type Person:\n    name as String\n    age as Int\n    address as Address\nend\n\
                  person is Person(\"Andi\", 17, Address(\"Bandung\", \"Indonesia\"))\n\
                  result is match person:\n\
                      Person(address: Address(city: city), age: 17):\n    city\n\
                      _:\n    \"other\"\n\
                  end\n\
                  exhaustive is match person:\n\
                      Person(name: _, age: _, address: _):\n    \"person\"\n\
                  end\n\
                  Sayln result\nSayln exhaustive\n";
    let (valid, _, error) = check_source(source);
    assert!(valid, "named struct patterns did not type-check: {error}");
    let (success, output) = run_source_stdout(source);
    assert!(success, "named struct matching failed: {output}");
    assert_eq!(output, "Bandung\nperson\n");
}

#[test]
fn named_struct_patterns_reject_unknown_and_duplicate_fields() {
    let unknown_field = "type Person:\n    name as String\nend\n\
                         person is Person(\"Andi\")\n\
                         match person:\n    Person(age: years):\n        years\nend\n";
    let (success, _, error) = check_source(unknown_field);
    assert!(!success);
    assert!(
        error.contains("struct pattern `Person` has no field `age`"),
        "{error}"
    );

    let duplicate_field = "type Person:\n    name as String\nend\n\
                           person is Person(\"Andi\")\n\
                           match person:\n    Person(name: first, name: second):\n        first\nend\n";
    let (success, _, error) = check_source(duplicate_field);
    assert!(!success);
    assert!(
        error.contains("duplicate field in named struct pattern"),
        "{error}"
    );
}

#[test]
fn recursively_matches_nested_structs_and_tuple_structs() {
    let source = "type Address:\n    city as String\nend\n\
                  type Person:\n    name as String\n    address as Address\nend\n\
                  pair is (Person(\"Andi\", Address(\"Surabaya\")), Person(\"Budi\", Address(\"Jakarta\")))\n\
                  result is match pair:\n\
                      (Person(first, Address(city)), Person(second, _)):\n    first + \" \" + city + \" \" + second\n\
                  end\n\
                  Sayln result\n";
    let (valid, _, error) = check_source(source);
    assert!(valid, "nested Struct patterns did not type-check: {error}");
    let (success, output) = run_source_stdout(source);
    assert!(success, "nested Struct matching failed: {output}");
    assert_eq!(output, "Andi Surabaya Budi\n");
}

#[test]
fn matches_structs_nested_in_enum_payloads_and_deep_tuple_payloads() {
    let source = "type Address:\n    city as String\nend\n\
                  type Person:\n    name as String\n    address as Address\nend\n\
                  enum Result:\n    Ok as Person\n    Error as String\nend\n\
                  enum Deep:\n    Value as (Person, Int)\nend\n\
                  person is Person(\"Andi\", Address(\"Surabaya\"))\n\
                  result is Result::Ok(person)\n\
                  deep is Deep::Value((person, 7))\n\
                  message is match result:\n\
                      Result::Ok(Person(name, Address(city))):\n    name + \" \" + city\n\
                      Result::Error(error):\n    error\n\
                  end\n\
                  number is match deep:\n\
                      Deep::Value((Person(_, Address(city)), qty)):\n    qty\n\
                  end\n\
                  Sayln message\nSayln number\n";
    let (valid, _, error) = check_source(source);
    assert!(
        valid,
        "nested enum/Struct patterns failed checking: {error}"
    );
    let (success, output) = run_source_stdout(source);
    assert!(success, "nested enum/Struct matching failed: {output}");
    assert_eq!(output, "Andi Surabaya\n7\n");
}

#[test]
fn struct_patterns_preserve_nominal_identity_and_check_arity_and_types() {
    let wrong_nominal = "type Person:\n    name as String\nend\n\
                         type User:\n    name as String\nend\n\
                         user is User(\"Andi\")\n\
                         match user:\n    Person(name):\n        name\nend\n";
    let (success, _, error) = check_source(wrong_nominal);
    assert!(!success);
    assert!(
        error.contains("struct pattern `Person` cannot match value of type User"),
        "{error}"
    );

    for (pattern, expected) in [
        (
            "Person(name)",
            "struct pattern `Person` expects 2 fields, found 1",
        ),
        (
            "Person(name, age, extra)",
            "struct pattern `Person` expects 2 fields, found 3",
        ),
    ] {
        let source = format!(
            "type Person:\n    name as String\n    age as Int\nend\n\
             person is Person(\"Andi\", 17)\n\
             match person:\n    {pattern}:\n        1\nend\n"
        );
        let (success, _, error) = check_source(&source);
        assert!(!success);
        assert!(error.contains(expected), "missing {expected:?}: {error}");
    }

    let incompatible = "value is 1\n\
                        match value:\n    Person(name, age):\n        name\n    _:\n        \"no\"\nend\n";
    let (success, _, error) = check_source(incompatible);
    assert!(!success);
    assert!(
        error.contains("struct pattern `Person` cannot match value of type Int"),
        "{error}"
    );
}

#[test]
fn failed_nested_struct_patterns_do_not_leak_bindings_and_mutations_are_visible() {
    let source = "type Address:\n    city as String\nend\n\
                  type Person:\n    name as String\n    address as Address\nend\n\
                  on Person receive rename(next as String):\n    name -> next\nend\n\
                  on Person receive get_name:\n    return name\nend\n\
                  person is Person(\"Andi\", Address(\"Surabaya\"))\n\
                  alias is person\n\
                  alias :: rename(\"Ada\")\n\
                  result is match person:\n\
                      Person(name, Address(city)):\n    name + \" \" + city\n\
                      _:\n    \"unknown\"\n\
                  end\n\
                  Sayln result\n";
    let (success, output) = run_source_stdout(source);
    assert!(success, "shared Struct matching failed: {output}");
    assert_eq!(output, "Ada Surabaya\n");

    let failed = "enum State:\n    Ready\n    Busy\nend\n\
                  type Person:\n    name as String\n    state as State\nend\n\
                  person is Person(\"Andi\", State::Busy)\n\
                  match person:\n\
                      Person(name, State::Ready):\n    \"matched\"\n\
                      _:\n    name\n\
                  end\n";
    let (success, error) = run_source(failed);
    assert!(
        !success,
        "a failed Struct pattern exposed a partial binding"
    );
    assert!(error.contains("unknown variable `name`"), "{error}");
}

#[test]
fn matching_struct_patterns_is_read_only_and_scrutinee_is_evaluated_once() {
    let source = "type Person:\n    name as String\nend\n\
                  type Counter:\n    value as Int\nend\n\
                  on Person receive rename(next as String):\n    name -> next\nend\n\
                  on Person receive get_name:\n    return name\nend\n\
                  on Counter receive next:\n    value -> value + 1\n\
                    return Person(\"Andi\")\nend\n\
                  on Counter receive get_value:\n    return value\nend\n\
                  person is Person(\"Ada\")\n\
                  counter is Counter(0)\n\
                  result is match counter :: next:\n\
                      Person(name):\n    name\n\
                  end\n\
                  message is match person:\n\
                      Person(name):\n    name\n\
                  end\n\
                  Sayln result\nSayln message\nSayln person :: get_name\nSayln counter :: get_value\n";
    let (success, output) = run_source_stdout(source);
    assert!(success, "single-evaluation Struct pattern failed: {output}");
    assert_eq!(output, "Andi\nAda\nAda\n1\n");
}

#[test]
fn guards_use_pattern_bindings_and_continue_after_false() {
    let source = "enum Result:\n    Ok as Int\n    Error as String\nend\n\
                  result is Result::Ok(42)\n\
                  message is match result:\n\
                      Result::Ok(value) if value > 100:\n    \"large\"\n\
                      Result::Ok(value) if value > 0:\n    \"positive\"\n\
                      Result::Ok(value):\n    \"non-positive\"\n\
                      Result::Error(error):\n    error\n\
                  end\n\
                  pair is (2, 5)\n\
                  tuple_result is match pair:\n\
                      (left, right) if left < right:\n    left + right\n\
                      _:\n    0\n\
                  end\n\
                  Sayln message\nSayln tuple_result\n";
    let (valid, _, error) = check_source(source);
    assert!(valid, "guarded patterns failed semantic checking: {error}");
    let (success, output) = run_source_stdout(source);
    assert!(success, "guarded matching failed: {output}");
    assert_eq!(output, "positive\n7\n");
}

#[test]
fn guards_run_only_for_matching_patterns_and_stop_after_first_true_guard() {
    let source = "enum Result:\n    Ok as Int\n    Error as String\nend\n\
                  type Counter:\n    calls as Int\nend\n\
                  on Counter receive check:\n\
                      calls -> calls + 1\n\
                      return calls == 1\n\
                  end\n\
                  on Counter receive get_calls:\n    return calls\nend\n\
                  counter is Counter(0)\n\
                  result is Result::Ok(8)\n\
                  selected is match result:\n\
                      Result::Ok(value) if counter :: check:\n    \"first\"\n\
                      Result::Ok(value) if counter :: check:\n    \"second\"\n\
                      _:\n    \"fallback\"\n\
                  end\n\
                  Sayln selected\nSayln counter :: get_calls\n\
                  failed_pattern is match Result::Error(\"no\"):\n\
                      Result::Ok(value) if counter :: check:\n    value\n\
                      Result::Ok(value):\n    0\n\
                      Result::Error(message):\n    0\n\
                  end\n\
                  Sayln failed_pattern\nSayln counter :: get_calls\n";
    let (valid, _, error) = check_source(source);
    assert!(valid, "guard side-effect test failed checking: {error}");
    let (success, output) = run_source_stdout(source);
    assert!(success, "guard ordering failed: {output}");
    assert_eq!(output, "first\n1\n0\n1\n");
}

#[test]
fn guarded_bindings_are_discarded_on_false_and_after_selected_arm() {
    let source = "enum Result:\n    Ok as Int\n    Error as String\nend\n\
                  result is Result::Ok(5)\n\
                  selected is match result:\n\
                      Result::Ok(value) if value > 10:\n    value\n\
                      Result::Ok(value):\n    value + 1\n\
                      Result::Error(message):\n    0\n\
                  end\n\
                  Sayln selected\n\
                  Sayln value\n";
    let (success, error) = run_source(source);
    assert!(!success, "pattern binding leaked outside guarded match");
    assert!(error.contains("unknown variable `value`"), "{error}");

    let false_wildcard = "answer is match 3:\n\
                          _ if false:\n    1\n\
                          _:\n    2\n\
                          end\nSayln answer\n";
    let (valid, _, error) = check_source(false_wildcard);
    assert!(valid, "guarded wildcard fallback failed checking: {error}");
    let (success, output) = run_source_stdout(false_wildcard);
    assert!(success, "guarded wildcard did not fall through: {output}");
    assert_eq!(output, "2\n");
}

#[test]
fn guards_must_be_boolean_and_do_not_prove_enum_exhaustiveness() {
    for guard in ["1", "\"true\""] {
        let source =
            format!("value is 2\nmatch value:\n    number if {guard}:\n        number\nend\n");
        let (success, _, error) = check_source(&source);
        assert!(!success, "non-Bool guard {guard} was accepted");
        assert!(error.contains("expected Bool"), "{error}");
    }

    let incomplete = "enum Result:\n    Ok as Int\n    Error as String\nend\n\
                      result is Result::Ok(1)\n\
                      match result:\n\
                          Result::Ok(value) if value > 0:\n    value\n\
                          Result::Error(message):\n    0\n\
                      end\n";
    let (success, _, error) = check_source(incomplete);
    assert!(
        !success,
        "guarded enum variant incorrectly counted as exhaustive"
    );
    assert!(error.contains("missing variant `Result::Ok`"), "{error}");

    let guarded_wildcard = "value is 2\n\
                            match value:\n\
                                _ if value > 0:\n    value\n\
                            end\n";
    let (success, _, error) = check_source(guarded_wildcard);
    assert!(
        !success,
        "guarded wildcard incorrectly counted as exhaustive"
    );
    assert!(error.contains("non-exhaustive match"), "{error}");

    let exhaustive = incomplete.replace(
        "Result::Error(message):",
        "Result::Ok(value):\n    value\n    Result::Error(message):",
    );
    let (success, _, error) = check_source(&exhaustive);
    assert!(
        success,
        "unguarded fallback did not restore exhaustiveness: {error}"
    );
}

#[test]
fn usefulness_analysis_recurses_through_enum_payloads_and_nominal_structs() {
    let exhaustive = "enum Inner:\n    A as Int\n    B as String\nend\n\
                      enum Outer:\n    Value as Inner\n    Error\nend\n\
                      outer is Outer::Value(Inner::A(1))\n\
                      match outer:\n\
                          Outer::Value(Inner::A(number)):\n    number\n\
                          Outer::Value(Inner::B(text)):\n    0\n\
                          Outer::Error:\n    0\n\
                      end\n";
    let (success, _, error) = check_source(exhaustive);
    assert!(success, "nested enum coverage was not exhaustive: {error}");

    let missing_nested = exhaustive.replace("Outer::Value(Inner::B(text)):\n    0\n", "");
    let (success, _, error) = check_source(&missing_nested);
    assert!(!success, "nested enum coverage omitted an Inner variant");
    assert!(
        error.contains("non-exhaustive match"),
        "nested missing witness should not mislabel an outer variant: {error}"
    );

    let duplicate = "type Address:\n    city as String\nend\n\
                     type Person:\n    address as Address\nend\n\
                     person is Person(Address(\"Surabaya\"))\n\
                     match person:\n\
                         Person(Address(city)):\n    city\n\
                         Person(Address(other)):\n    other\n\
                     end\n";
    let (success, _, error) = check_source(duplicate);
    assert!(
        !success,
        "a fully covered Struct pattern was considered useful"
    );
    assert!(error.contains("unreachable match pattern"), "{error}");

    let duplicate_variant = "enum Result:\n    Ok as Int\n    Error as String\nend\n\
                             result is Result::Ok(1)\n\
                             match result:\n\
                                 Result::Ok(value):\n    value\n\
                                 Result::Ok(other):\n    other\n\
                                 Result::Error(message):\n    0\n\
                             end\n";
    let (success, _, error) = check_source(duplicate_variant);
    assert!(!success, "duplicate enum coverage was not diagnosed");
    assert!(error.contains("unreachable match pattern"), "{error}");

    let identifier_then_wildcard = "value is 1\n\
                                    match value:\n\
                                        bound:\n    bound\n\
                                        _:\n    0\n\
                                    end\n";
    let (success, _, error) = check_source(identifier_then_wildcard);
    assert!(!success, "wildcard after identifier was not diagnosed");
    assert!(error.contains("unreachable match pattern"), "{error}");
}

#[test]
fn runtime_errors_in_guards_propagate_instead_of_becoming_false() {
    let source = "result is match 1:\n\
                  _ if 1 / 0 == 0:\n    1\n\
                  _:\n    2\n\
                  end\n";
    let (valid, _, error) = check_source(source);
    assert!(
        valid,
        "guard error test should pass static checking: {error}"
    );
    let (success, error) = run_source(source);
    assert!(!success);
    assert!(error.contains("division by zero"), "{error}");
}

#[test]
fn or_patterns_parse_flatten_and_match_alternatives_left_to_right() {
    let source = "enum Choice:\n    First as Int\n    Second as Int\n    Third as Int\nend\n\
                  choice is Choice::Second(12)\n\
                  result is match choice:\n\
                      Choice::First(value) | Choice::Second(value) | Choice::Third(value):\n    value\n\
                  end\n\
                  Sayln result\n";
    let (valid, _, error) = check_source(source);
    assert!(valid, "valid OR-pattern failed checking: {error}");
    let (success, output) = run_source_stdout(source);
    assert!(success, "OR-pattern failed at runtime: {output}");
    assert_eq!(output, "12\n");

    let alternative_order = "enum Choice:\n    First as Int\n    Second as Int\nend\n\
                             choice is Choice::Second(9)\n\
                             result is match choice:\n\
                                 Choice::First(value) | Choice::Second(value):\n    value\n\
                                 _:\n    0\n\
                             end\n\
                             Sayln result\n";
    let (success, output) = run_source_stdout(alternative_order);
    assert!(success, "second OR alternative was not tried: {output}");
    assert_eq!(output, "9\n");
}

#[test]
fn or_patterns_compose_inside_enum_payload_tuple_and_struct_patterns() {
    let source = "enum Choice:\n    First as Int\n    Second as Int\n    Third as Int\nend\n\
                  enum Envelope:\n    Value as Choice\n    Empty\nend\n\
                  type Boxed:\n    choice as Choice\nend\n\
                  envelope is Envelope::Value(Choice::Second(4))\n\
                  box is Boxed(Choice::First(7))\n\
                  tuple is (Choice::Second(11), 2)\n\
                  nested is match envelope:\n\
                      Envelope::Value(Choice::First(value) | Choice::Second(value)):\n    value\n\
                      _:\n    0\n\
                  end\n\
                  in_tuple is match tuple:\n\
                      (Choice::First(value) | Choice::Second(value), _):\n    value\n\
                      _:\n    0\n\
                  end\n\
                  in_struct is match box:\n\
                      Boxed(Choice::First(value) | Choice::Second(value)):\n    value\n\
                      _:\n    0\n\
                  end\n\
                  parenthesized is match envelope:\n\
                      (Envelope::Value(Choice::First(value)) | Envelope::Value(Choice::Second(value))):\n    value\n\
                      _:\n    0\n\
                  end\n\
                  Sayln nested\nSayln in_tuple\nSayln in_struct\nSayln parenthesized\n";
    let (valid, _, error) = check_source(source);
    assert!(valid, "nested OR-pattern failed checking: {error}");
    let (success, output) = run_source_stdout(source);
    assert!(success, "nested OR-pattern failed at runtime: {output}");
    assert_eq!(output, "4\n11\n7\n4\n");
}

#[test]
fn or_alternatives_require_the_same_binding_names_and_compatible_types() {
    let different_names = "enum Choice:\n    First as Int\n    Second as Int\nend\n\
                           choice is Choice::First(1)\n\
                           match choice:\n\
                               Choice::First(value) | Choice::Second(other):\n    value\n\
                           end\n";
    let (success, _, error) = check_source(different_names);
    assert!(!success);
    assert!(
        error.contains("OR-pattern alternatives must bind the same names"),
        "{error}"
    );

    let literal_and_binding = "value is 1\nmatch value:\n    1 | other:\n        other\nend\n";
    let (success, _, error) = check_source(literal_and_binding);
    assert!(!success);
    assert!(
        error.contains("OR-pattern alternatives must bind the same names"),
        "{error}"
    );

    let incompatible_types = "enum Choice:\n    First as Int\n    Second as String\nend\n\
                              choice is Choice::First(1)\n\
                              match choice:\n\
                                  Choice::First(value) | Choice::Second(value):\n    value\n\
                              end\n";
    let (success, _, error) = check_source(incompatible_types);
    assert!(!success);
    assert!(
        error.contains("OR-pattern alternatives must bind compatible types"),
        "{error}"
    );

    let mismatched_wildcards = "enum Choice:\n    First as Int\n    Second as Int\nend\n\
                                choice is Choice::First(1)\n\
                                match choice:\n\
                                    Choice::First(value) | Choice::Second(_):\n    value\n\
                                end\n";
    let (success, _, error) = check_source(mismatched_wildcards);
    assert!(!success);
    assert!(
        error.contains("OR-pattern alternatives must bind the same names"),
        "{error}"
    );
}

#[test]
fn or_coverage_is_a_union_and_guarded_or_patterns_add_no_coverage() {
    let exhaustive = "enum Result:\n    Ok as Int\n    Error as String\nend\n\
                      result is Result::Ok(4)\n\
                      match result:\n\
                          Result::Ok(_) | Result::Error(_):\n    1\n\
                      end\n";
    let (success, _, error) = check_source(exhaustive);
    assert!(success, "unguarded OR did not cover both variants: {error}");

    let after_union = "enum Result:\n    Ok as Int\n    Error as String\nend\n\
                       result is Result::Ok(4)\n\
                       match result:\n\
                           Result::Ok(_) | Result::Error(_):\n    1\n\
                           Result::Ok(value):\n    value\n\
                       end\n";
    let (success, _, error) = check_source(after_union);
    assert!(!success, "arm after exhaustive OR was not unreachable");
    assert!(error.contains("unreachable match pattern"), "{error}");

    let guarded = "enum Result:\n    Ok as Int\n    Error as String\nend\n\
                   result is Result::Ok(4)\n\
                   match result:\n\
                       Result::Ok(_) | Result::Error(_) if true:\n    1\n\
                   end\n";
    let (success, _, error) = check_source(guarded);
    assert!(
        !success,
        "guarded OR incorrectly counted toward exhaustiveness"
    );
    assert!(error.contains("missing variant `Result::Ok`"), "{error}");
}

#[test]
fn or_pattern_bindings_are_transactional_and_guards_run_once_after_match() {
    let source = "enum Inner:\n    First as Int\n    Second as Int\nend\n\
                  enum Outer:\n    Left as Inner\n    Right as Int\nend\n\
                  type Counter:\n    calls as Int\nend\n\
                  on Counter receive guard:\n\
                      calls -> calls + 1\n\
                      return false\n\
                  end\n\
                  on Counter receive get_calls:\n    return calls\nend\n\
                  counter is Counter(0)\n\
                  value is (Outer::Left(Inner::Second(5)), 8)\n\
                  result is match value:\n\
                      (Outer::Left(Inner::First(number)), _)\n\
                          | (Outer::Left(Inner::Second(number)), _)\n\
                          if counter :: guard:\n    number\n\
                      _:\n    0\n\
                  end\n\
                  Sayln result\nSayln counter :: get_calls\n";
    let (valid, _, error) = check_source(source);
    assert!(valid, "transactional OR guard failed checking: {error}");
    let (success, output) = run_source_stdout(source);
    assert!(
        success,
        "transactional OR guard failed at runtime: {output}"
    );
    assert_eq!(output, "0\n1\n");
}

#[test]
fn malformed_or_patterns_require_a_second_alternative() {
    for source in [
        "value is match 1:\n    _ |:\n        1\nend\n",
        "value is match 1:\n    _ |\n        :\n        1\nend\n",
    ] {
        let (success, error) = run_source(source);
        assert!(!success);
        assert!(error.contains("expected a pattern after `|`"), "{error}");
    }
}

#[test]
fn literal_patterns_match_exact_supported_values_and_negative_numbers() {
    let source = "integer is 2\n\
                  float is 3.14\n\
                  text is \"start now\"\n\
                  flag is false\n\
                  negative is -1\n\
                  integer_result is match integer:\n\
                      0:\n    \"zero\"\n\
                      1 | 2 | 3:\n    \"small\"\n\
                      _:\n    \"large\"\n\
                  end\n\
                  float_result is match float:\n\
                      3.14:\n    \"pi-ish\"\n\
                      _:\n    \"other\"\n\
                  end\n\
                  text_result is match text:\n\
                      \"start\":\n    \"prefix\"\n\
                      \"start now\":\n    \"exact\"\n\
                      _:\n    \"other\"\n\
                  end\n\
                  flag_result is match flag:\n\
                      true | false:\n    \"boolean\"\n\
                  end\n\
                  negative_result is match negative:\n\
                      -1:\n    \"negative one\"\n\
                      _:\n    \"other\"\n\
                  end\n\
                  Sayln integer_result\n\
                  Sayln float_result\n\
                  Sayln text_result\n\
                  Sayln flag_result\n\
                  Sayln negative_result\n";
    let (valid, _, error) = check_source(source);
    assert!(valid, "literal patterns did not type-check: {error}");
    let (success, output) = run_source_stdout(source);
    assert!(success, "literal pattern matching failed: {output}");
    assert_eq!(output, "small\npi-ish\nexact\nboolean\nnegative one\n");
}

#[test]
fn bool_literals_prove_exhaustiveness_but_open_literal_domains_do_not() {
    for source in [
        "flag is true\nmatch flag:\n    true:\n        1\n    false:\n        0\nend\n",
        "flag is false\nmatch flag:\n    false:\n        0\n    true:\n        1\nend\n",
        "flag is true\nmatch flag:\n    true | false:\n        1\nend\n",
    ] {
        let (valid, _, error) = check_source(source);
        assert!(valid, "complete Bool match was rejected: {error}");
    }

    for source in [
        "value is 1\nmatch value:\n    0:\n        0\n    1:\n        1\nend\n",
        "value is 1.0\nmatch value:\n    0.0:\n        0\n    1.0:\n        1\nend\n",
        "value is \"Alice\"\nmatch value:\n    \"Alice\":\n        1\n    \"Bob\":\n        2\nend\n",
    ] {
        let (valid, _, error) = check_source(source);
        assert!(!valid, "open literal domain was treated as exhaustive");
        assert!(
            error.contains("non-exhaustive match"),
            "wrong error for open literal domain: {error}"
        );
    }

    let identifier_fallback = "value is 5\nresult is match value:\n    0:\n        0\n    remaining:\n        remaining\nend\nSayln result\n";
    let (valid, _, error) = check_source(identifier_fallback);
    assert!(
        valid,
        "identifier did not cover values remaining after literals: {error}"
    );
    let (success, output) = run_source_stdout(identifier_fallback);
    assert!(success, "identifier fallback failed at runtime: {output}");
    assert_eq!(output, "5\n");
}

#[test]
fn literal_patterns_reject_incompatible_types_and_arbitrary_expressions() {
    for (source, expected) in [
        (
            "value is 1.0\nmatch value:\n    1:\n        1.0\n    _:\n        0.0\nend\n",
            "literal pattern of type Int cannot match value of type Float",
        ),
        (
            "value is 1\nmatch value:\n    1 | \"one\":\n        1\n    _:\n        0\nend\n",
            "literal pattern of type String cannot match value of type Int",
        ),
    ] {
        let (valid, _, error) = check_source(source);
        assert!(!valid, "incompatible literal pattern was accepted");
        assert!(error.contains(expected), "{error}");
    }

    let (success, error) =
        run_source("value is 1\nmatch value:\n    value + 1:\n        1\n    _:\n        0\nend\n");
    assert!(!success, "arbitrary expression was accepted as a pattern");
    assert!(
        error.contains("expected `:` after match pattern"),
        "{error}"
    );
}

#[test]
fn literal_patterns_compose_with_nested_enum_tuple_and_struct_patterns() {
    let source = "enum Result:\n    Ok as (Int, String)\n    Error as String\nend\n\
                  type Person:\n    name as String\n    age as Int\nend\n\
                  result is Result::Ok((1, \"ok\"))\n\
                  person is Person(\"Andi\", 17)\n\
                  nested is match result:\n\
                      Result::Ok((1 | 2, \"ok\")):\n    \"one-ok\"\n\
                      Result::Ok((_, _)):\n    \"other-ok\"\n\
                      Result::Error(_):\n    \"error\"\n\
                  end\n\
                  student is match person:\n\
                      Person(\"Andi\", 17):\n    \"student\"\n\
                      Person(_, _):\n    \"other\"\n\
                  end\n\
                  Sayln nested\nSayln student\n";
    let (valid, _, error) = check_source(source);
    assert!(valid, "nested literal patterns did not type-check: {error}");
    let (success, output) = run_source_stdout(source);
    assert!(
        success,
        "nested literal patterns failed at runtime: {output}"
    );
    assert_eq!(output, "one-ok\nstudent\n");
}

#[test]
fn literal_usefulness_detects_duplicates_and_respects_guards() {
    let duplicate =
        "value is 1\nmatch value:\n    1:\n        1\n    1:\n        2\n    _:\n        0\nend\n";
    let (valid, _, error) = check_source(duplicate);
    assert!(!valid, "duplicate literal arm was considered useful");
    assert!(error.contains("unreachable match pattern"), "{error}");

    let redundant_or = "value is 1\nmatch value:\n    1 | 1:\n        1\n    _:\n        0\nend\n";
    let (valid, _, error) = check_source(redundant_or);
    assert!(!valid, "duplicate OR alternative was not diagnosed");
    assert!(
        error.contains("redundant OR-pattern alternative"),
        "{error}"
    );

    let after_wildcard = "value is 1\nmatch value:\n    _:\n        0\n    1:\n        1\nend\n";
    let (valid, _, error) = check_source(after_wildcard);
    assert!(!valid, "literal after wildcard was considered useful");
    assert!(error.contains("unreachable match pattern"), "{error}");

    let after_or = "value is 1\nmatch value:\n    1 | 2:\n        1\n    1:\n        2\n    _:\n        0\nend\n";
    let (valid, _, error) = check_source(after_or);
    assert!(
        !valid,
        "literal covered by an OR-pattern was considered useful"
    );
    assert!(error.contains("unreachable match pattern"), "{error}");

    let guarded_duplicate = "value is 1\nmatch value:\n    1 if true:\n        1\n    1:\n        2\n    _:\n        0\nend\n";
    let (valid, _, error) = check_source(guarded_duplicate);
    assert!(
        valid,
        "guarded literal incorrectly hid a later arm: {error}"
    );

    let guarded_bool =
        "flag is true\nmatch flag:\n    true if true:\n        1\n    false:\n        0\nend\n";
    let (valid, _, error) = check_source(guarded_bool);
    assert!(!valid, "guarded Bool pattern counted toward exhaustiveness");
    assert!(error.contains("non-exhaustive match"), "{error}");
}

#[test]
fn nested_struct_enum_guard_reads_typed_bindings() {
    let source = "type Person:\n    name as String\n    age as Int\nend\n\
                  enum Result:\n    Ok as Person\n    Error as String\nend\n\
                  result is Result::Ok(Person(\"Andi\", 17))\n\
                  message is match result:\n\
                      Result::Ok(Person(name, age)) if age >= 18:\n    name\n\
                      Result::Ok(Person(name, _)):\n    \"minor\"\n\
                      Result::Error(error):\n    error\n\
                  end\n\
                  Sayln message\n";
    let (valid, _, error) = check_source(source);
    assert!(valid, "nested Struct guard failed checking: {error}");
    let (success, output) = run_source_stdout(source);
    assert!(success, "nested Struct guard failed at runtime: {output}");
    assert_eq!(output, "minor\n");
}

#[test]
fn hash_patterns_match_required_keys_recursively_and_bind_transactionally() {
    let source = "enum Result:\n\
                      Ok as Int\n\
                      Error as String\n\
                  end\n\
                  type Person:\n\
                      name as String\n\
                      age as Int\n\
                  end\n\
                  record is hash:\n\
                      result is Result::Ok(7)\n\
                      person is Person(\"Ada\", 36)\n\
                      values is [1, 2]\n\
                      extra is true\n\
                  end\n\
                  answer is match record:\n\
                      {\"result\": Result::Ok(0..10), person: Person(name, 18..), values: [1, 2]}:\n\
                          name\n\
                      {\"person\": Person(leaked, _), \"result\": Result::Error(_)}:\n\
                          leaked\n\
                      _:\n\
                          \"fallback\"\n\
                  end\n\
                  Sayln answer\n";
    let (valid, _, error) = check_source(source);
    assert!(valid, "nested Hash pattern failed checking: {error}");
    let (success, output) = run_source_stdout(source);
    assert!(success, "nested Hash pattern failed at runtime: {output}");
    assert_eq!(output, "Ada\n");

    let missing_key = "record is hash:\n    name is \"Ada\"\nend\n\
                       answer is match record:\n\
                           {\"missing\": value}:\n    value\n\
                           _:\n    \"fallback\"\n\
                       end\nSayln answer\n";
    let (success, output) = run_source_stdout(missing_key);
    assert!(success, "missing key should select fallback: {output}");
    assert_eq!(output, "fallback\n");

    let failed_nested_pattern = "enum Result:\n\
                                    Ok as Int\n\
                                    Error as String\n\
                                end\n\
                                type Person:\n\
                                    name as String\n\
                                    age as Int\n\
                                end\n\
                                record is hash:\n\
                                    person is Person(\"Ada\", 36)\n\
                                    result is Result::Error(\"no\")\n\
                                end\n\
                                answer is match record:\n\
                                    {person: Person(name, _), result: Result::Ok(_)}:\n\
                                        name\n\
                                    {person: Person(name, _), result: Result::Error(_)} if name == \"Ada\":\n\
                                        name\n\
                                    {}:\n\
                                        \"fallback\"\n\
                                end\n\
                                Sayln answer\n";
    let (success, output) = run_source_stdout(failed_nested_pattern);
    assert!(
        success,
        "bindings from the failed Hash arm leaked into the next arm: {output}"
    );
    assert_eq!(output, "Ada\n");

    let guarded = "record is hash:\n    name is \"Ada\"\nend\n\
                   answer is match record:\n\
                       {name: person} if false:\n    \"wrong\"\n\
                       {\"name\": person} if person == \"Ada\":\n    person\n\
                       {}:\n    \"fallback\"\n\
                   end\nSayln answer\n";
    let (valid, _, error) = check_source(guarded);
    assert!(valid, "Hash guard pattern failed checking: {error}");
    let (success, output) = run_source_stdout(guarded);
    assert!(success, "Hash guard pattern failed at runtime: {output}");
    assert_eq!(output, "Ada\n");
}

#[test]
fn hash_pattern_usefulness_and_exhaustiveness_are_partial_and_key_sensitive() {
    let exhaustive = "record is hash:\n    value is 1\nend\n\
                      result is match record:\n\
                          {value: 0..10}:\n    1\n\
                          {\"value\": 11..}:\n    2\n\
                          {}:\n    3\n\
                      end\n";
    let (valid, _, error) = check_source(exhaustive);
    assert!(
        valid,
        "empty Hash pattern should cover all remaining maps: {error}"
    );

    for source in [
        "record is hash:\n    value is 1\nend\n\
         match record:\n\
             {value: 1}:\n    1\n\
             {\"value\": 1}:\n    2\n\
             {}:\n    3\n\
         end\n",
        "record is hash:\n    value is 1\nend\n\
         match record:\n\
             {}:\n    1\n\
             {value: _}:\n    2\n\
         end\n",
    ] {
        let (valid, _, error) = check_source(source);
        assert!(!valid, "covered Hash pattern was accepted");
        assert!(error.contains("unreachable match pattern"), "{error}");
    }

    let not_exhaustive = "record is hash:\n    value is 1\nend\n\
                          match record:\n\
                              {value: 0..10 | 20..30}:\n    1\n\
                          end\n";
    let (valid, _, error) = check_source(not_exhaustive);
    assert!(!valid, "keyed Hash patterns cannot cover arbitrary maps");
    assert!(error.contains("non-exhaustive match"), "{error}");

    let disjoint_or_union = "record is hash:\n    value is 15\nend\n\
                             match record:\n\
                                 {value: 0..10 | 20..30}:\n    1\n\
                                 {value: 15}:\n    2\n\
                                 {}:\n    3\n\
                             end\n";
    let (valid, _, error) = check_source(disjoint_or_union);
    assert!(
        valid,
        "OR range coverage incorrectly included the gap around 15: {error}"
    );

    let distinct_keys = "record is hash:\n    a is 1\nend\n\
                         match record:\n\
                             {a: 1, b: _}:\n    1\n\
                             {a: 1}:\n    2\n                             {}:\n    3\n\
                         end\n";
    let (valid, _, error) = check_source(distinct_keys);
    assert!(
        valid,
        "more-specific earlier key set hid a broader pattern: {error}"
    );

    let subset_is_unreachable = "record is hash:\n    a is 1\n    b is 2\nend\n\
                                 match record:\n\
                                     {a: 1}:\n    1\n\
                                     {a: 1, b: _}:\n    2\n\
                                     {}:\n    3\n\
                                 end\n";
    let (valid, _, error) = check_source(subset_is_unreachable);
    assert!(
        !valid,
        "a keyed pattern covered by a less restrictive key set was accepted"
    );
    assert!(error.contains("unreachable match pattern"), "{error}");
}

#[test]
fn hash_pattern_keys_are_literal_unique_and_or_patterns_round_trip() {
    for (pattern, diagnostic) in [
        (
            "{1: value}",
            "hash pattern keys must be string literals or field names",
        ),
        ("{name: _, \"name\": _}", "duplicate key in hash pattern"),
        ("{\"name\" value}", "expected `:` after hash pattern key"),
    ] {
        let source = format!(
            "record is hash:\n    name is \"Ada\"\nend\nmatch record:\n    {pattern}:\n        1\n    _:\n        0\nend\n"
        );
        let (success, _, error) = check_source(&source);
        assert!(!success, "invalid Hash pattern {pattern} was accepted");
        assert!(error.contains(diagnostic), "{error}");
    }

    let source = "record as Hash is hash:\n    name is \"Ada\"\nend\n\
                  result is match record:\n\
                      {\"name\": person, extra: [0 .. 10, _]} | {name: person, extra: [11 .., _]}:\n\
                          person\n\
                      {}:\n\
                          \"other\"\n\
                  end\nSayln result\n";
    let (valid, _, error) = check_source(source);
    assert!(valid, "OR Hash pattern failed checking: {error}");
    let (success, output) = run_source_stdout(source);
    assert!(success, "OR Hash pattern failed at runtime: {output}");
    assert_eq!(output, "other\n");

    let non_hash = "value is 1\nmatch value:\n    {}:\n        1\n    _:\n        0\nend\n";
    let (success, _, error) = check_source(non_hash);
    assert!(!success, "Hash pattern was accepted for Int");
    assert!(
        error.contains("hash pattern cannot match value of type Int"),
        "{error}"
    );
}

#[test]
fn hash_pattern_coverage_matrix_handles_empty_hash_and_overlapping_keys() {
    let cases = [
        (
            "record is hash:\n    name is \"A\"\nend\n\
             match record:\n    {}:\n        1\nend\n",
            true,
            None,
        ),
        (
            "record is hash:\n    name is \"A\"\nend\n\
             match record:\n    {name: value}:\n        value\nend\n",
            false,
            Some("non-exhaustive match"),
        ),
        (
            "record is hash:\n    name is \"A\"\nend\n\
             match record:\n    {name: value}:\n        value\n    {}:\n        \"zero\"\nend\n",
            true,
            None,
        ),
        (
            "record is hash:\n    name is \"A\"\nend\n\
             match record:\n    {}:\n        0\n    {name: value}:\n        value\nend\n",
            false,
            Some("unreachable match pattern"),
        ),
        (
            "record is hash:\n    name is \"A\"\nend\n\
             match record:\n    {name: \"A\"}:\n        1\n    {name: \"B\"}:\n        2\nend\n",
            false,
            Some("non-exhaustive match"),
        ),
        (
            "record is hash:\n    name is \"A\"\nend\n\
             match record:\n    {name: \"A\"}:\n        1\n    {}:\n        2\nend\n",
            true,
            None,
        ),
        (
            "record is hash:\n    name is \"A\"\nend\n\
             match record:\n    {name: value}:\n        value\n    {name: \"Budi\"}:\n        \"zero\"\n    {}:\n        \"one\"\nend\n",
            false,
            Some("unreachable match pattern"),
        ),
        (
            "record is hash:\n    name is \"A\"\nend\n\
             match record:\n    {name: \"A\"}:\n        \"one\"\n    {name: value}:\n        value\n    {}:\n        \"two\"\nend\n",
            true,
            None,
        ),
        (
            "record is hash:\n    name is \"A\"\n    age is 17\nend\n\
             match record:\n    {name: value}:\n        value\n    {name: value, age: age}:\n        type_of(age)\n    {}:\n        \"zero\"\nend\n",
            false,
            Some("unreachable match pattern"),
        ),
        (
            "record is hash:\n    name is \"A\"\n    age is 17\nend\n\
             match record:\n    {name: value, age: age}:\n        type_of(age)\n    {name: value}:\n        value\n    {}:\n        \"zero\"\nend\n",
            true,
            None,
        ),
    ];

    for (source, expected_valid, diagnostic) in cases {
        let (valid, _, error) = check_source(source);
        assert_eq!(
            valid, expected_valid,
            "unexpected Hash coverage result: {error}\n{source}"
        );
        if let Some(diagnostic) = diagnostic {
            assert!(
                error.contains(diagnostic),
                "missing {diagnostic:?}: {error}"
            );
        }
    }
}

#[test]
fn nested_hash_patterns_compose_with_struct_enum_tuple_sequence_and_or() {
    let source = "enum Result:\n\
                      Ok as Int\n\
                      Err as String\n\
                  end\n\
                  type Person:\n\
                      name as String\n\
                      age as Int\n\
                  end\n\
                  record as Hash is hash:\n\
                      user is hash:\n\
                          name is \"Budi\"\n\
                          age is 20\n\
                      end\n\
                      person is Person(\"Sari\", 31)\n\
                      result is Result::Ok(8)\n\
                      point is (3, 4)\n\
                      items is [5, 6, 7]\n\
                  end\n\
                  output is match record:\n\
                      {user: {name: name}, person: Person(person_name, age), result: Result::Ok(0..10), point: (x, 0..), items: [head, ...tail]}:\n\
                          name\n\
                      {user: {}, result: Result::Err(error)}:\n\
                          error\n\
                      {}:\n\
                          \"fallback\"\n\
                  end\n\
                  Sayln output\n";
    let (valid, _, error) = check_source(source);
    assert!(valid, "nested mixed Hash pattern failed checking: {error}");
    let (success, output) = run_source_stdout(source);
    assert!(
        success,
        "nested mixed Hash pattern failed at runtime: {output}"
    );
    assert_eq!(output, "Budi\n");

    let nested_missing_key = "record is hash:\n\
                                  user is hash:\n\
                                      age is 17\n\
                                  end\n\
                              end\n\
                              result is match record:\n\
                                  {user: {name: name}}:\n    name\n\
                                  {}:\n    \"fallback\"\n\
                              end\nSayln result\n";
    let (success, output) = run_source_stdout(nested_missing_key);
    assert!(
        success,
        "missing nested key should fail the first arm: {output}"
    );
    assert_eq!(output, "fallback\n");
}

#[test]
fn hash_runtime_lookup_preserves_exact_string_keys_and_partial_matching() {
    for (entries, expected) in [
        ("", "other\n"),
        ("name is \"A\"\n", "exact-A\n"),
        ("name is \"B\"\n", "exact-B\n"),
        ("age is 17\n", "other\n"),
        ("age is 18\n", "adult\n"),
        ("name is \"A\"\nage is 20\n", "exact-A\n"),
    ] {
        let source = format!(
            "record is hash:\n{entries}end\n\
             result is match record:\n\
                 {{name: \"A\"}}:\n    \"exact-A\"\n\
                 {{\"name\": \"B\"}}:\n    \"exact-B\"\n\
                 {{name: name}}:\n    \"named-\" + name\n\
                 {{age: 18..}}:\n    \"adult\"\n\
                 {{}}:\n    \"other\"\n\
             end\nSayln result\n"
        );
        let (success, output) = run_source_stdout(&source);
        assert!(success, "Hash runtime case failed: {output}\n{source}");
        assert_eq!(output, expected, "{source}");
    }

    for (key, expected) in [
        ("name", "plain\n"),
        ("name ", "spaced\n"),
        ("Name", "capital\n"),
    ] {
        let exact_keys = format!(
            "mut record is hash:\n    name is \"plain\"\nend\n\
             record[\"name \"] -> \"spaced\"\n\
             record[\"Name\"] -> \"capital\"\n\
             result is match record:\n\
                 {{\"{key}\": value}}:\n    value\n\
                 {{}}:\n    \"missing\"\n\
             end\nSayln result\n"
        );
        let (valid, _, error) = check_source(&exact_keys);
        assert!(valid, "exact Hash key {key:?} failed checking: {error}");
        let (success, output) = run_source_stdout(&exact_keys);
        assert!(
            success,
            "exact Hash key {key:?} failed: {}\n{exact_keys}",
            run_source(&exact_keys).1
        );
        assert_eq!(output, expected, "key {key:?}");
    }
}

#[test]
fn malformed_hash_patterns_report_parse_errors_without_panicking() {
    for (pattern, diagnostic) in [
        (
            "{",
            "hash pattern keys must be string literals or field names",
        ),
        ("{\"name\":", "expected `:` after hash pattern key"),
        ("{\"name\"}", "expected `:` after hash pattern key"),
        (
            "{: value}",
            "hash pattern keys must be string literals or field names",
        ),
        (
            "{123: value}",
            "hash pattern keys must be string literals or field names",
        ),
        (
            "{\"name\": value,, \"age\": age}",
            "hash pattern keys must be string literals or field names",
        ),
    ] {
        let source = format!(
            "record is hash:\n    name is \"Ada\"\nend\nmatch record:\n    {pattern}:\n        1\n    _:\n        0\nend\n"
        );
        let (success, _, error) = check_source(&source);
        assert!(!success, "malformed Hash pattern {pattern} was accepted");
        assert!(
            error.contains(diagnostic),
            "diagnostic did not mention {diagnostic:?}: {error}"
        );
    }
}

#[test]
fn hash_or_bindings_and_guards_follow_existing_pattern_rules() {
    let compatible = "record is hash:\n    user is \"Ada\"\nend\n\
                      result is match record:\n\
                          {name: value} | {user: value}:\n    value\n\
                          {}:\n    \"missing\"\n\
                      end\nSayln result\n";
    let (valid, _, error) = check_source(compatible);
    assert!(
        valid,
        "compatible Hash OR bindings failed checking: {error}"
    );
    let (success, output) = run_source_stdout(compatible);
    assert!(success, "compatible Hash OR failed at runtime: {output}");
    assert_eq!(output, "Ada\n");

    let incompatible = "record is hash:\n    name is \"Ada\"\nend\n\
                        match record:\n\
                            {name: value} | {age: other}:\n    value\n\
                            {}:\n    0\n\
                        end\n";
    let (valid, _, error) = check_source(incompatible);
    assert!(!valid, "incompatible Hash OR bindings were accepted");
    assert!(
        error.contains("OR-pattern alternatives must bind the same names"),
        "{error}"
    );

    let guarded_only = "record is hash:\n    age is 20\nend\n\
                        match record:\n\
                            {age: age} if age >= 18:\n    age\n\
                        end\n";
    let (valid, _, error) = check_source(guarded_only);
    assert!(!valid, "guarded Hash pattern was considered exhaustive");
    assert!(error.contains("non-exhaustive match"), "{error}");

    let non_bool_guard = "record is hash:\n    age is 20\nend\n\
                          match record:\n\
                              {age: age} if 1:\n    age\n\
                              {}:\n    0\n\
                          end\n";
    let (valid, _, error) = check_source(non_bool_guard);
    assert!(!valid, "non-Bool Hash guard was accepted");
    assert!(error.contains("expected Bool"), "{error}");

    let nested_or = "record is hash:\n    status is \"success\"\nend\n\
                     result is match record:\n\
                         {status: \"ok\" | \"success\"}:\n    \"matched\"\n\
                         {}:\n    \"fallback\"\n\
                     end\nSayln result\n";
    let (valid, _, error) = check_source(nested_or);
    assert!(valid, "nested value OR pattern failed checking: {error}");
    let (success, output) = run_source_stdout(nested_or);
    assert!(
        success,
        "nested value OR pattern failed at runtime: {output}"
    );
    assert_eq!(output, "matched\n");
}

#[test]
fn alias_patterns_bind_the_whole_value_and_nested_bindings() {
    let source = "enum Result:\n\
                      Ok as Int\n\
                      Error as String\n\
                  end\n\
                  age is 21\n\
                  result is Result::Ok(7)\n\
                  point is (3, 4)\n\
                  record is hash:\n\
                      name is \"Ada\"\n\
                      extra is true\n\
                  end\n\
                  numbers is [1, 2, 3]\n\
                  Sayln match age:\n\
                      adult @ 18..:\n    adult\n\
                      _:\n    0\n\
                  end\n\
                  Sayln match result:\n\
                      whole @ Result::Ok(payload @ 0..10):\n    payload\n\
                      whole @ Result::Ok(_):\n    0\n\
                      whole @ Result::Error(message):\n    0\n\
                  end\n\
                  Sayln match point:\n\
                      whole @ (x, y):\n    whole == point\n\
                  end\n\
                  Sayln match record:\n\
                      whole @ {name: name}:\n    whole == record\n\
                      {}:\n    false\n\
                  end\n\
                  Sayln match numbers:\n\
                      whole @ [head, ...tail]:\n    head + tail[0]\n\
                      []:\n    0\n\
                  end\n\
                  Sayln match 2:\n\
                      whole @ (1 | 2):\n    whole\n\
                      _:\n    0\n\
                  end\n";
    let (valid, _, error) = check_source(source);
    assert!(valid, "Alias patterns failed semantic analysis: {error}");
    let (success, output) = run_source_stdout(source);
    assert!(success, "Alias pattern run failed: {output}");
    assert_eq!(output, "21\n7\ntrue\ntrue\n3\n2\n");
}

#[test]
fn alias_pattern_failure_bindings_guards_and_coverage_are_transactional() {
    let source = "record is hash:\n\
                      name is \"Ada\"\n\
                      age is 10\n\
                  end\n\
                  result is match record:\n\
                      whole @ {name: name, age: 18..}:\n    \"wrong\"\n\
                      whole @ {name: name, age: _} if name == \"Ada\":\n    name\n\
                      {}:\n    \"fallback\"\n\
                  end\nSayln result\n";
    let (valid, _, error) = check_source(source);
    assert!(valid, "Hash alias failure case did not type-check: {error}");
    let (success, output) = run_source_stdout(source);
    assert!(
        success,
        "Hash alias failed-pattern binding leaked: {output}"
    );
    assert_eq!(output, "Ada\n");

    let duplicate = "value is 5\nmatch value:\n    whole @ 1..:\n        1\n    whole @ 2..:\n        2\n    _:\n        0\nend\n";
    let (valid, _, error) = check_source(duplicate);
    assert!(
        !valid,
        "alias coverage was not shared with its nested pattern"
    );
    assert!(error.contains("unreachable match pattern"), "{error}");

    let nested_or = "value is 2\nmatch value:\n    whole @ (1 | 2):\n        whole\n    2:\n        2\n    _:\n        0\nend\n";
    let (valid, _, error) = check_source(nested_or);
    assert!(
        !valid,
        "alias over OR coverage was not analyzed as its union"
    );
    assert!(error.contains("unreachable match pattern"), "{error}");

    let alias_wildcard = "value is 5\n\
                          result is match value:\n\
                              whole @ _:\n    whole\n\
                          end\nSayln result\n";
    let (valid, _, error) = check_source(alias_wildcard);
    assert!(
        valid,
        "alias over wildcard should remain exhaustive: {error}"
    );
    let (success, output) = run_source_stdout(alias_wildcard);
    assert!(success, "alias over wildcard failed at runtime: {output}");
    assert_eq!(output, "5\n");
}

#[test]
fn alias_patterns_validate_bindings_or_alternatives_and_syntax() {
    let same_names = "value is 2\n\
                      result is match value:\n\
                          whole @ 1 | whole @ 2:\n    whole\n\
                          _:\n    0\n\
                      end\nSayln result\n";
    let (valid, _, error) = check_source(same_names);
    assert!(
        valid,
        "OR alternatives with the same alias binding failed: {error}"
    );
    let (success, output) = run_source_stdout(same_names);
    assert!(success, "OR alias binding failed: {output}");
    assert_eq!(output, "2\n");

    for pattern in ["_ @ 1", "whole @"] {
        let source = format!(
            "value is 2\nmatch value:\n    {pattern}:\n        1\n    _:\n        0\nend\n"
        );
        let (valid, _, error) = check_source(&source);
        assert!(!valid, "invalid alias {pattern} was accepted");
        assert!(!error.is_empty(), "invalid alias lacked a diagnostic");
    }

    let expression_operator = "value is 1\nresult is value @ 1\n";
    let (valid, _, error) = check_source(expression_operator);
    assert!(!valid, "`@` was accepted as an expression operator");
    assert!(
        !error.is_empty(),
        "invalid expression use lacked a diagnostic"
    );

    let incompatible = "value is 2\nmatch value:\n    first @ 1 | second @ 2:\n        first\n    _:\n        0\nend\n";
    let (valid, _, error) = check_source(incompatible);
    assert!(!valid, "OR aliases with different names were accepted");
    assert!(
        error.contains("OR-pattern alternatives must bind the same names"),
        "{error}"
    );
}

#[test]
fn pattern_type_checking_rejects_provably_incompatible_shapes_recursively() {
    let cases = [
        (
            "value is \"hello\"\nmatch value:\n    18..:\n        1\n    _:\n        0\nend\n",
            "range pattern cannot match value of type String",
        ),
        (
            "value is \"hello\"\nmatch value:\n    18:\n        1\n    _:\n        0\nend\n",
            "literal pattern of type Int cannot match value of type String",
        ),
        (
            "value is 18\nmatch value:\n    \"eighteen\":\n        1\n    _:\n        0\nend\n",
            "literal pattern of type String cannot match value of type Int",
        ),
        (
            "value is true\nmatch value:\n    0..10:\n        1\n    _:\n        0\nend\n",
            "range pattern cannot match value of type Bool",
        ),
        (
            "value is 18\nmatch value:\n    (age, _):\n        age\n    _:\n        0\nend\n",
            "tuple pattern cannot match value of type Int",
        ),
        (
            "point is (1, \"north\")\nmatch point:\n    (x, 0..):\n        x\n    _:\n        0\nend\n",
            "range pattern cannot match value of type String",
        ),
        (
            "values is [1, 2]\nmatch values:\n    [\"one\", ...tail]:\n        1\n    _:\n        0\nend\n",
            "literal pattern of type String cannot match value of type Int",
        ),
        (
            "value is 18\nmatch value:\n    [head, ...tail]:\n        head\n    _:\n        0\nend\n",
            "sequence pattern cannot match value of type Int",
        ),
        (
            "type Person:\n    name as String\n    age as Int\nend\n\
             person is Person(\"Ada\", 18)\n\
             match person:\n    Person(1, _):\n        1\n    _:\n        0\nend\n",
            "literal pattern of type Int cannot match value of type String",
        ),
        (
            "enum Result:\n    Ok as Int\nend\n\
             value is Result::Ok(18)\n\
             match value:\n    Result::Ok(\"wrong\"):\n        1\n    _:\n        0\nend\n",
            "literal pattern of type String cannot match value of type Int",
        ),
        (
            "record is list [18]\n\
             match record:\n    {age: age}:\n        age\n    _:\n        0\nend\n",
            "hash pattern cannot match value of type List[Int]",
        ),
    ];

    for (source, expected) in cases {
        let (valid, _, error) = check_source(source);
        assert!(
            !valid,
            "provably incompatible pattern was accepted:\n{source}"
        );
        assert!(error.contains(expected), "missing {expected:?}: {error}");
    }
}

#[test]
fn unknown_scrutinee_types_remain_permissive_for_dynamic_nested_patterns() {
    let source = "fn inspect(value):\n\
                      result is match value:\n\
                          whole @ (Result::Ok(_) | {age: 18..} | [_, _]):\n\
                              1\n\
                          whole @ _:\n\
                              0\n\
                      end\n\
                      return result\n\
                  end\n\
                  fn inspect_array(ref value as Array[Int]) gives Int:\n\
                      result is match value:\n\
                          [_, _]:\n\
                              1\n\
                          _:\n\
                              0\n\
                      end\n\
                      return result\n\
                  end\n\
                  enum Result:\n\
                      Ok as Int\n\
                      Error as String\n\
                  end\n\
                  Sayln inspect(Result::Ok(3))\n\
                  Sayln inspect(\"dynamic\")\n\
                  Sayln inspect_array(ref [1, 2])\n";
    let (valid, _, error) = check_source(source);
    assert!(
        valid,
        "patterns on Unknown scrutinees should remain dynamically checked: {error}"
    );
    let (success, output) = run_source_stdout(source);
    assert!(
        success,
        "Unknown scrutinee patterns failed at runtime: {output}"
    );
    assert_eq!(output, "1\n0\n1\n");

    let dynamic_hash_value = "type Person:\n\
                                  name as String\n\
                                  age as Int\n\
                              end\n\
                              record is hash:\n\
                                  user is 7\n\
                              end\n\
                              result is match record:\n\
                                  {user: Person(name, age)}:\n    name\n\
                                  {}:\n    \"dynamic mismatch\"\n\
                              end\nSayln result\n";
    let (valid, _, error) = check_source(dynamic_hash_value);
    assert!(
        !valid,
        "known Int values must reject impossible Struct patterns"
    );
    assert!(error.contains("cannot match value of type Int"), "{error}");

    let dynamic_hash_value =
        dynamic_hash_value.replace("record is hash:", "record as Hash is hash:");
    let (valid, _, error) = check_source(&dynamic_hash_value);
    assert!(
        valid,
        "explicitly untyped Hash should defer nested checks: {error}"
    );
    let (success, output) = run_source_stdout(&dynamic_hash_value);
    assert!(success, "dynamic Hash nested value failed: {output}");
    assert_eq!(output, "dynamic mismatch\n");
}

#[test]
fn or_and_guard_patterns_validate_every_known_nested_type() {
    let incompatible_or = "value is 1\n\
                           match value:\n\
                               1 | [head, _]:\n    1\n\
                               _:\n    0\n\
                           end\n";
    let (valid, _, error) = check_source(incompatible_or);
    assert!(!valid, "incompatible nested OR alternative was accepted");
    assert!(
        error.contains("sequence pattern cannot match value of type Int"),
        "{error}"
    );

    let non_boolean_guard = "value is 1\n\
                             match value:\n\
                                 alias @ 1 if \"yes\":\n    alias\n\
                                 _:\n    0\n\
                             end\n";
    let (valid, _, error) = check_source(non_boolean_guard);
    assert!(!valid, "non-Bool guard on an alias pattern was accepted");
    assert!(error.contains("expected Bool"), "{error}");
}
