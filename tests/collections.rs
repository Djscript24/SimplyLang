mod common;
use common::*;

#[test]
fn enforces_float_and_matrix_runtime_contracts() {
    let (success, float_error) = run_source("Sayln 1e308 * 1e308\n");
    assert!(!success);
    assert!(float_error.contains("floating-point result is not finite"));

    let (success, matrix_output) = run_source_stdout(
        "left is matrix [[1, 2], [3, 4]]\nright is matrix [[5, 6], [7, 8]]\nSayln left multiply right\n",
    );
    assert!(success);
    assert!(matrix_output.contains("19"));
    assert!(matrix_output.contains("50"));
}

#[test]
fn collection_aliases_observe_mutation_through_mutable_bindings() {
    let (success, stdout) = run_source_stdout(
        "values is list [1]\nmut copy is values\ncopy add 2\nSayln values\nSayln copy\n",
    );
    assert!(success);
    assert_eq!(stdout.lines().collect::<Vec<_>>(), ["[1, 2]", "[1, 2]"]);

    let (success, stdout) = run_source_stdout(
        "profile is hash:\n    name is \"Ada\"\nend\nmut copy is profile\ncopy[\"role\"] -> \"builder\"\nSayln profile\nSayln copy\n",
    );
    assert!(success);
    assert!(stdout.lines().all(|line| line.contains("role")));
}

#[test]
fn ref_markers_are_rejected_outside_function_parameters_and_calls() {
    for source in [
        "mut alias is ref values\n",
        "alias -> ref values\n",
        "enum Result:\n    Some as List[Int]\nend\nResult::Some(ref values)\n",
        "on List receive inspect(ref value as List[Int]):\n    return 1\nend\n",
        "values :: inspect(ref other)\n",
    ] {
        let (valid, _, error) = check_source(source);
        assert!(!valid, "non-function use accepted a `ref` marker");
        assert!(error.contains("error[E.semantic.ref.invalid]"), "{error}");
    }
}

#[test]
fn nested_collection_mutation_updates_all_aliases() {
    let source = r#"record is hash:
    details is hash:
        scores is list [10, 20]
    end
end
mut copy is record
copy["details"]["scores"][0] -> 99
Sayln record["details"]["scores"][0]
Sayln copy["details"]["scores"][0]
"#;
    let (success, output) = run_source_stdout(source);
    assert!(success, "nested collection mutation failed: {output}");
    assert_eq!(output, "99\n99\n");
}

#[test]
fn ref_collection_parameters_are_paired_read_only_and_non_escaping() {
    let source = r#"fn first(ref values as List[Int]) gives Int:
    return values[0]
end
items is list [42]
Sayln first(ref items)
profile is hash:
    scores is list [17, 23]
end
Sayln first(ref profile["scores"])
"#;
    let (success, output) = run_source_stdout(source);
    assert!(
        success,
        "read-only ref call or nested view failed: {output}"
    );
    assert_eq!(output, "42\n17\n");

    for source in [
        "fn first(ref values as List[Int]) gives Int:\n    return values[0]\nend\nitems is list [1]\nfirst(items)\n",
        "fn first(value as Int) gives Int:\n    return value\nend\nfirst(ref 1)\n",
    ] {
        let (valid, _, error) = check_source(source);
        assert!(!valid, "incorrect ref marker pairing was accepted");
        assert!(error.contains("error[E.semantic.ref.invalid]"), "{error}");
        assert!(error.contains("ref"), "{error}");
    }

    let (valid, _, error) =
        check_source("fn first(values as List[Int]) gives Int:\n    return values[0]\nend\n");
    assert!(!valid, "a collection parameter without `ref` was accepted");
    assert!(error.contains("must be declared `ref`"), "{error}");

    let (valid, _, error) = check_source(
        "fn first(values) gives Int:\n    return 0\nend\nitems is list [1]\nfirst(items)\n",
    );
    assert!(!valid, "an untyped parameter accepted a collection value");
    assert!(error.contains("requires a `ref` parameter"), "{error}");

    let (valid, _, error) =
        check_source("fn update(ref values as List[Int]) gives Unit:\n    values[0] -> 7\nend\n");
    assert!(!valid, "a ref parameter was mutable");
    assert!(error.contains("error[E.semantic.ref.invalid]"), "{error}");
    assert!(error.contains("read-only and cannot be mutated"), "{error}");

    for source in [
        "fn escape(ref values as List[Int]) gives List[Int]:\n    return values\nend\n",
        "fn escape(ref values as List[Int]) gives Int:\n    copy is values\n    return 0\nend\n",
        "fn escape(ref values as List[Int]) gives Int:\n    fn nested() gives Int:\n        return values[0]\n    end\n    return 0\nend\n",
        "fn escape(ref values as List[List[Int]]) gives List[Int]:\n    for item in values:\n        return item\n    end\n    return [0]\nend\n",
    ] {
        let (valid, _, error) = check_source(source);
        assert!(!valid, "a ref parameter escaped its call");
        assert!(error.contains("error[E.semantic.ref.invalid]"), "{error}");
        assert!(
            error.contains("ref parameter") || error.contains("ref` parameter"),
            "{error}"
        );
    }

    let (valid, _, error) = check_source(
        "mut stash is list []\nfn leak(ref values as List[Int]) gives Unit:\n    stash add values\nend\n",
    );
    assert!(!valid, "a ref parameter escaped through list add");
    assert!(error.contains("error[E.semantic.ref.invalid]"), "{error}");
    assert!(error.contains("ref` parameter"), "{error}");
}

#[test]
fn ref_parameters_allow_nested_functions_that_do_not_capture_them() {
    let source = r#"fn first_plus_one(ref values as List[Int]) gives Int:
    fn increment(value as Int) gives Int:
        return value + 1
    end
    return increment(values[0])
end
items is list [41]
Sayln first_plus_one(ref items)
"#;
    let (success, output) = run_source_stdout(source);
    assert!(
        success,
        "non-capturing nested function was rejected: {output}"
    );
    assert_eq!(output, "42\n");
}

#[test]
fn ref_values_can_be_used_to_build_independent_key_collections() {
    let source = r#"fn profile_keys(ref profile as Hash) gives Array[String]:
    return keys(profile)
end
profile is hash:
    name is "Ada"
    role is "builder"
end
names is profile_keys(ref profile)
Sayln names
"#;
    let (success, output) = run_source_stdout(source);
    assert!(
        success,
        "independent result was rejected as a ref escape: {output}"
    );
    assert_eq!(output, "[name, role]\n");
}

#[test]
fn ref_scalar_reads_can_be_copied_into_escaping_collections() {
    let source = r#"fn first_snapshot(ref values as List[Int]) gives List[Int]:
    return list [values[0]]
end
items is list [42, 99]
snapshot is first_snapshot(ref items)
Sayln snapshot
"#;
    let (success, output) = run_source_stdout(source);
    assert!(
        success,
        "a fresh collection of copied scalar values was rejected: {output}"
    );
    assert_eq!(output, "[42]\n");

    let source = r#"fn nested_snapshot(ref values as List[List[Int]]) gives List[List[Int]]:
    return list [values[0]]
end
items is list [list [42]]
nested_snapshot(ref items)
"#;
    let (valid, _, error) = check_source(source);
    assert!(
        !valid,
        "a borrowed nested collection escaped inside a new list"
    );
    assert!(error.contains("error[E.semantic.ref.invalid]"), "{error}");
}

#[test]
fn ref_can_flow_through_fresh_builtin_collections_without_escaping_nested_handles() {
    let source = r#"fn reversed(ref values as List[Int]) gives List[Int]:
    return reverse(values)
end
items is list [10, 20, 30]
Sayln reversed(ref items)
"#;
    let (success, output) = run_source_stdout(source);
    assert!(
        success,
        "fresh reversed values were treated as borrowed: {output}"
    );
    assert_eq!(output, "[30, 20, 10]\n");

    let source = r#"fn reverse_nested(ref values as List[List[Int]]) gives List[List[Int]]:
    return reverse(values)
end
items is list [list [10]]
reverse_nested(ref items)
"#;
    let (valid, _, error) = check_source(source);
    assert!(!valid, "reverse let a nested borrowed collection escape");
    assert!(error.contains("error[E.semantic.ref.invalid]"), "{error}");
}

#[test]
fn invalid_ref_usage_uses_one_diagnostic_and_marks_its_source_location() {
    for (source, line, column, detail) in [
        (
            "mut values is list [1]\nvalues is ref values\n",
            2,
            11,
            "`ref` cannot be stored in a binding",
        ),
        (
            "fn invalid() gives List[Int]:\n    return ref values\nend\n",
            2,
            12,
            "`ref` cannot be returned from a function",
        ),
        (
            "fn escape(ref values as List[Int]) gives List[Int]:\n    return values\nend\n",
            2,
            5,
            "cannot escape through a return value",
        ),
        (
            "fn store(ref values as List[Int]) gives Unit:\n    copy is values\nend\n",
            2,
            5,
            "cannot escape through a variable binding",
        ),
        (
            "fn capture(ref values as List[Int]) gives Unit:\n    fn nested() gives Int:\n        return values[0]\n    end\nend\n",
            2,
            5,
            "closure cannot capture `ref` parameter `values`",
        ),
        (
            "fn mutate(ref values as List[Int]) gives Unit:\n    values[0] -> 2\nend\n",
            2,
            5,
            "read-only and cannot be mutated",
        ),
        (
            "fn mutate(ref values as List[Int]) gives Unit:\n    values add 2\nend\n",
            2,
            5,
            "read-only and cannot be mutated",
        ),
        (
            "fn reassign(ref values as List[Int]) gives Unit:\n    values -> list [2]\nend\n",
            2,
            5,
            "read-only and cannot be reassigned",
        ),
    ] {
        let (valid, _, error) = check_source(source);
        assert!(!valid, "invalid `ref` usage was accepted");
        assert!(error.contains("error[E.semantic.ref.invalid]"), "{error}");
        assert!(error.contains("(Semantic error)"), "{error}");
        assert!(error.contains(&format!(":{line}:{column}")), "{error}");
        assert!(error.contains(detail), "{error}");
    }
}

#[test]
fn mutable_collection_cycles_remain_safe_in_the_evaluator_arena() {
    let source = r#"mut first as Hash is hash:
    value is 1
end
mut second as Hash is hash:
    value is 1
end
first["self"] -> first
second["self"] -> second
Sayln first == second
Sayln first
mut items is [1]
alias is items
items[0] -> 2
Sayln alias[0]
"#;
    let (success, output) = run_source_stdout(source);
    assert!(success, "cyclic collection operations failed: {output}");
    assert_eq!(output, "true\n{self: <cycle>, value: 1}\n2\n");
}

#[test]
fn removing_nested_collections_does_not_hold_a_mutable_arena_borrow() {
    let source = r#"mut values is list [list [1], list [2]]
mut target is list [1]
values remove target
Sayln values
"#;
    let (success, output) = run_source_stdout(source);
    assert!(success, "removing a nested collection failed: {output}");
    assert_eq!(output, "[[2]]\n");
}

#[test]
fn runs_standard_library_builtins() {
    let output = run_example("examples/08-standard-library/builtins.si");
    assert!(output.contains("[1, 2, 3, 4]"));
    assert!(output.matches("4").count() >= 2);
    assert!(output.contains("6"));
}

#[test]
fn assertions_validate_conditions_and_report_optional_messages() {
    let source = "assert(true)\nassert(true, \"unused\")\nassert(true, substring(\"x\", 5, 1))\nSayln \"continued\"\n";
    let (valid, _, error) = check_source(source);
    assert!(valid, "valid assertions did not type-check: {error}");
    let (success, output) = run_source_stdout(source);
    assert!(success, "successful assertions failed: {output}");
    assert_eq!(output, "continued\n");

    let (success, error) = run_source("assert(false, \"expected a positive value\")\n");
    assert!(!success);
    assert!(
        error.contains("expected a positive value"),
        "missing assertion message: {error}"
    );

    let (valid, _, error) = check_source("assert(1)\n");
    assert!(!valid);
    assert!(error.contains("expected Bool, found Int"), "{error}");

    let (valid, _, error) = check_source("assert(true, 1)\n");
    assert!(!valid);
    assert!(error.contains("expected String, found Int"), "{error}");
}

#[test]
fn runs_boolean_and_string_collection_builtins() {
    let (success, stdout) = run_source_stdout(
        "flags is list [true, false]\nwords is list [\"Ada\", \"Lin\"]\nSayln any(flags)\nSayln all(flags)\nSayln join(words, \"-\")\n",
    );
    assert!(success);
    assert_eq!(
        stdout.lines().collect::<Vec<_>>(),
        ["true", "false", "Ada-Lin"]
    );

    let (success, _, error) = check_source("Sayln join(list [1], \",\")\n");
    assert!(!success);
    assert!(error.contains("expected String, found Int"));
}

#[test]
fn hash_keys_and_values_follow_deterministic_key_order() {
    let source = "profile is hash:\n\
                      name is \"Ada\"\n\
                      age is 36\n\
                  end\n\
                  scores is hash:\n\
                      z is 3\n\
                      a is 1\n\
                  end\n\
                  Sayln keys(profile)\n\
                  Sayln values(profile)\n\
                  Sayln keys(scores)\n\
                  Sayln values(scores)\n\
                  Sayln entries(profile)\n\
                  Sayln has_key(profile, \"name\")\n\
                  Sayln has_key(profile, \"missing\")\n\
                  Sayln has_key(scores, \"z\")\n\
                  for entry in entries(scores):\n\
                      Sayln entry[0] + \":\" + type_of(entry[1])\n\
                  end\n";
    let (valid, _, error) = check_source(source);
    assert!(valid, "map keys/values did not type-check: {error}");
    let (success, output) = run_source_stdout(source);
    assert!(success, "map keys/values failed: {output}");
    assert_eq!(
        output,
        "[age, name]\n[36, Ada]\n[a, z]\n[1, 3]\n[[age, 36], [name, Ada]]\ntrue\nfalse\ntrue\na:Int\nz:Int\n"
    );

    for source in [
        "keys([1, 2])\n",
        "values(\"text\")\n",
        "entries([1, 2])\n",
        "has_key([1, 2], \"key\")\n",
        "has_key(hash:\n    value is 1\nend, 1)\n",
    ] {
        let (valid, _, error) = check_source(source);
        assert!(!valid);
        assert!(
            error.contains("requires a Hash") || error.contains("expected String, found Int"),
            "{error}"
        );
    }

    let (success, output) = run_source_stdout(
        "fn entries(value as Int) gives Int:\n    return value\nend\nSayln entries(7)\n",
    );
    assert!(
        success,
        "user-defined entries function was shadowed: {output}"
    );
    assert_eq!(output, "7\n");
}

#[test]
fn gets_map_values_or_lazy_defaults_with_type_checks() {
    let source = "profile is hash:\n\
                      name is \"Ada\"\n\
                  end\n\
                  Sayln get(profile, \"name\", read_file(\"missing-default.txt\"))\n\
                  Sayln get(profile, \"role\", \"unknown\")\n\
                  Sayln type_of(get(profile, \"role\", \"unknown\"))\n";
    let (valid, _, error) = check_source(source);
    assert!(valid, "map get did not type-check: {error}");
    let (success, output) = run_source_stdout(source);
    assert!(success, "map get evaluated an unused default: {output}");
    assert_eq!(output, "Ada\nunknown\nString\n");

    let (valid, _, error) = check_source("get(hash:\n    age is 36\nend, \"age\", \"unknown\")\n");
    assert!(!valid);
    assert!(error.contains("expected Int, found String"), "{error}");

    let (valid, _, error) = check_source("get(hash:\n    age is 36\nend, 1, 0)\n");
    assert!(!valid);
    assert!(error.contains("expected String, found Int"), "{error}");

    let (success, output) =
        run_source_stdout("fn get(value as Int) gives Int:\n    return value\nend\nSayln get(7)\n");
    assert!(success, "user-defined get function was shadowed: {output}");
    assert_eq!(output, "7\n");
}

#[test]
fn without_key_returns_an_independent_hash() {
    let source = "profile is hash:\n\
                      name is \"Ada\"\n\
                      role is \"builder\"\n\
                  end\n\
                  filtered is without_key(profile, \"role\")\n\
                  missing is without_key(filtered, \"unknown\")\n\
                  catalog is hash:\n\
                      first is 1\n\
                      second is 2\n\
                  end\n\
                  filtered_map is without_key(catalog, \"first\")\n\
                  Sayln keys(filtered)\n\
                  Sayln keys(profile)\n\
                  Sayln keys(missing)\n\
                  Sayln type_of(filtered_map)\n\
                  Sayln keys(filtered_map)\n";
    let (valid, _, error) = check_source(source);
    assert!(valid, "without_key did not type-check: {error}");
    let (success, output) = run_source_stdout(source);
    assert!(success, "without_key failed: {output}");
    assert_eq!(output, "[name]\n[name, role]\n[name]\nHash\n[second]\n");

    for source in [
        "without_key([1], \"key\")\n",
        "without_key(hash:\n    age is 36\nend, 1)\n",
    ] {
        let (valid, _, error) = check_source(source);
        assert!(!valid);
        assert!(
            error.contains("requires a Hash") || error.contains("expected String, found Int"),
            "{error}"
        );
    }
}

#[test]
fn select_keys_filters_maps_without_changing_type_or_source() {
    let source = "profile is hash:\n\
                      name is \"Ada\"\n\
                      role is \"builder\"\n\
                      age is 36\n\
                  end\n\
                  selected is select_keys(profile, [\"role\", \"missing\", \"name\", \"role\"])\n\
                  catalog is hash:\n\
                      first is 1\n\
                      second is 2\n\
                  end\n\
                  selected_map is select_keys(catalog, (\"second\", \"missing\"))\n\
                  Sayln entries(selected)\n\
                  Sayln keys(profile)\n\
                  Sayln type_of(selected_map)\n\
                  Sayln entries(selected_map)\n";
    let (valid, _, error) = check_source(source);
    assert!(valid, "select_keys did not type-check: {error}");
    let (success, output) = run_source_stdout(source);
    assert!(success, "select_keys failed: {output}");
    assert_eq!(
        output,
        "[[name, Ada], [role, builder]]\n[age, name, role]\nHash\n[[second, 2]]\n"
    );

    for source in [
        "select_keys([1], [\"key\"])\n",
        "select_keys(hash:\n    value is 1\nend, [1])\n",
        "select_keys(hash:\n    value is 1\nend, \"value\")\n",
    ] {
        let (valid, _, error) = check_source(source);
        assert!(!valid);
        assert!(
            error.contains("requires a Hash")
                || error.contains("expected String, found Int")
                || error.contains("expects an array, list, or tuple of strings"),
            "{error}"
        );
    }
}

#[test]
fn runs_direct_sum_builtin() {
    let (success, stdout) = run_source_stdout(
        "values is list [10, 20, 30]\nSayln total(values)\nSayln total(array [1.5, 2.5])\n",
    );
    assert!(success);
    assert_eq!(stdout.lines().collect::<Vec<_>>(), ["60", "4"]);

    let (success, _, error) = check_source("Sayln total(list [1, \"two\"])\n");
    assert!(!success);
    assert!(error.contains("expected a number") || error.contains("String"));

    let (success, _, error) = check_source(
        "fn needs_text(value as String):\n    return value\nend\n\
         numbers is [1, 2]\nneeds_text(total(numbers))\n",
    );
    assert!(
        !success,
        "numeric total must not remain semantically Unknown"
    );
    assert!(error.contains("expected String, found Int"), "{error}");
}

#[test]
fn runs_string_builtins() {
    let (success, stdout) = run_source_stdout(
        "text is \"  Ada,Lin  \"\nSayln trim(text)\nSayln split(trim(text), \",\")\nSayln replace(text, \"Ada\", \"Citra\")\nSayln starts_with(trim(text), \"Ada\")\nSayln ends_with(trim(text), \"Lin\")\n",
    );
    assert!(success);
    assert_eq!(
        stdout.lines().collect::<Vec<_>>(),
        ["Ada,Lin", "[Ada, Lin]", "  Citra,Lin  ", "true", "true"]
    );
}

#[test]
fn runs_collection_utility_builtins() {
    let (success, stdout) = run_source_stdout(
        "values is list [1, 2, 3]\nempty is list []\nSayln reverse(values)\nSayln is_empty(empty)\nSayln is_empty(values)\nSayln is_empty(\"\")\n",
    );
    assert!(success);
    assert_eq!(
        stdout.lines().collect::<Vec<_>>(),
        ["[3, 2, 1]", "true", "false", "true"]
    );
}

#[test]
fn length_measures_collections_and_pipeline_count_counts_survivors() {
    let source = "values is list [4, 5, 6]\n\
                  Sayln length(values)\n\
                  Sayln length(\"aé\")\n\
                  selected is pipeline:\n\
                      values\n\
                      where item > 4\n\
                      count\n\
                  end\n\
                  Sayln selected\n";
    let (valid, _, error) = check_source(source);
    assert!(valid, "size/count distinction did not type-check: {error}");
    let (success, output) = run_source_stdout(source);
    assert!(success, "{output}");
    assert_eq!(output, "3\n2\n2\n");
}

#[test]
fn enumerate_indexes_sequences_ranges_and_unicode_strings() {
    let source = "numbers is list [7, 9]\n\
                  point is (\"Ada\", 36)\n\
                  Sayln enumerate(numbers)\n\
                  Sayln enumerate(range(3, 6))\n\
                  Sayln enumerate(\"aé\")\n\
                  Sayln enumerate(point)\n\
                  for pair in enumerate(numbers):\n\
                      Sayln pair[0] + pair[1]\n\
                  end\n";
    let (valid, _, error) = check_source(source);
    assert!(valid, "enumerate did not type-check: {error}");
    let (success, output) = run_source_stdout(source);
    assert!(success, "{output}");
    assert_eq!(
        output,
        "[[0, 7], [1, 9]]\n[[0, 3], [1, 4], [2, 5]]\n[[0, a], [1, é]]\n[[0, Ada], [1, 36]]\n7\n10\n"
    );
}

#[test]
fn enumerate_checks_arguments_and_preserves_user_function_shadowing() {
    let (valid, _, error) = check_source("enumerate(10)\n");
    assert!(!valid);
    assert!(
        error.contains("`enumerate` requires a sequence or string"),
        "{error}"
    );

    let (success, output) = run_source_stdout(
        "fn enumerate(value as Int) gives Int:\n\
             return value + 1\n\
         end\n\
         Sayln enumerate(4)\n",
    );
    assert!(success, "{output}");
    assert_eq!(output, "5\n");
}

#[test]
fn user_functions_shadow_builtins_consistently() {
    let source = "fn length(value as Int) gives Int:\n\
                      return value + 2\n\
                  end\n\
                  fn count(value as Int) gives Int:\n\
                      return value + 1\n\
                  end\n\
                  values is [3, 4]\n\
                  Sayln length(5)\n\
                  Sayln count(5)\n\
                  selected is pipeline:\n\
                      values\n\
                      count\n\
                  end\n\
                  Sayln selected\n";
    let (valid, _, error) = check_source(source);
    assert!(valid, "shadowed builtin did not type-check: {error}");
    let (success, output) = run_source_stdout(source);
    assert!(success, "{output}");
    assert_eq!(output, "7\n6\n2\n");
}

#[test]
fn zip_pairs_sequences_to_the_shorter_input() {
    let source = "numbers is list [7, 9, 11]\n\
                  letters is array [\"a\", \"b\"]\n\
                  point is (true, 4, \"extra\")\n\
                  Sayln zip(numbers, letters)\n\
                  Sayln zip(range(3, 7), \"xy\")\n\
                  Sayln zip(point, list [1, 2])\n\
                  Sayln zip(list [], range(0, 1000000000))\n\
                  for pair in zip(numbers, letters):\n\
                      Sayln pair\n\
                  end\n";
    let (valid, _, error) = check_source(source);
    assert!(valid, "zip did not type-check: {error}");
    let (success, output) = run_source_stdout(source);
    assert!(success, "{output}");
    assert_eq!(
        output,
        "[[7, a], [9, b]]\n[[3, x], [4, y]]\n[[true, 1], [4, 2]]\n[]\n[7, a]\n[9, b]\n"
    );
}

#[test]
fn zip_checks_arguments_and_preserves_user_function_shadowing() {
    let (valid, _, error) = check_source("zip([1], 2)\n");
    assert!(!valid);
    assert!(
        error.contains("`zip` requires sequences or strings"),
        "{error}"
    );

    let (success, output) = run_source_stdout(
        "fn zip(value as Int, other as Int) gives Int:\n\
             return value + other\n\
         end\n\
         Sayln zip(4, 5)\n",
    );
    assert!(success, "{output}");
    assert_eq!(output, "9\n");
}

#[test]
fn empty_collections_keep_unknown_element_type_for_mutation() {
    let (success, output) =
        run_source_stdout("mut values is list []\nvalues add \"ready\"\nSayln values\n");
    assert!(success, "{output}");
    assert_eq!(output, "[ready]\n");
}

#[test]
fn empty_list_mutation_infers_and_enforces_its_element_type() {
    for (source, expected_output) in [
        (
            "mut values is list []\nvalues add \"hello\"\nSayln values\n",
            "[hello]\n",
        ),
        (
            "mut values is list []\nvalues add 1\nSayln values\n",
            "[1]\n",
        ),
        (
            "mut values is list []\nvalues add \"hello\"\nvalues add \"world\"\nSayln values\n",
            "[hello, world]\n",
        ),
    ] {
        let (valid, _, check_error) = check_source(source);
        assert!(
            valid,
            "check rejected inferred list element type: {check_error}"
        );
        let (success, output) = run_source_stdout(source);
        assert!(success, "run rejected inferred list element type: {output}");
        assert_eq!(output, expected_output);
    }

    let incompatible = "mut values is list []\nvalues add \"hello\"\nvalues add 42\n";
    let (valid, _, check_error) = check_source(incompatible);
    assert!(!valid, "check accepted an incompatible list mutation");
    assert!(
        check_error.contains("expected String, found Int"),
        "{check_error}"
    );
    let (success, runtime_error) = run_source(incompatible);
    assert!(!success, "run accepted an incompatible list mutation");
    assert!(
        runtime_error.contains("expected String, found Int"),
        "{runtime_error}"
    );
}

#[test]
fn empty_float_sums_and_totals_preserve_the_inferred_float_type() {
    let cases = [
        (
            "mut values as List[Float] is list []\n\
             fn result() gives Float:\n\
                 return total(values)\n\
             end\n\
             Sayln type_of(result())\n",
            "Float\n",
        ),
        (
            "mut values as List[Float] is list []\n\
             result is pipeline:\n\
                 values\n\
                 sum\n\
             end\n\
             fn as_float() gives Float:\n\
                 return result\n\
             end\n\
             Sayln type_of(as_float())\n",
            "Float\n",
        ),
        (
            "mut values as List[Float] is list [1.0, 2.0]\n\
             result is pipeline:\n\
                 values\n\
                 where item > 10.0\n\
                 sum\n\
             end\n\
             fn as_float() gives Float:\n\
                 return result\n\
             end\n\
             Sayln type_of(as_float())\n",
            "Float\n",
        ),
        (
            "values as List[Float] is list [1.25, 2.5]\n\
             result is pipeline:\n\
                 values\n\
                 sum\n\
             end\n\
             Sayln result\n\
             Sayln type_of(result)\n",
            "3.75\nFloat\n",
        ),
        (
            "values as List[Int] is list []\n\
             result is pipeline:\n\
                 values\n\
                 derive item * 1.0\n\
                 sum\n\
             end\n\
             fn as_float() gives Float:\n\
                 return result\n\
             end\n\
             Sayln type_of(as_float())\n",
            "Float\n",
        ),
        (
            "values as List[Int] is list [1, 2]\n\
             result is pipeline:\n\
                 values\n\
                 where item > 10\n\
                 derive item * 1.0\n\
                 sum\n\
             end\n\
             fn as_float() gives Float:\n\
                 return result\n\
             end\n\
             Sayln type_of(as_float())\n",
            "Float\n",
        ),
        (
            "fn floats() gives List[Float]:\n\
                 return list []\n\
             end\n\
             result is pipeline:\n\
                 floats()\n\
                 sum\n\
             end\n\
             fn as_float() gives Float:\n\
                 return result\n\
             end\n\
             Sayln type_of(as_float())\n\
             Sayln type_of(total(floats()))\n",
            "Float\nFloat\n",
        ),
        (
            "mut values as List[Int] is list []\n\
             Sayln type_of(total(values))\n\
             result is pipeline:\n\
                 values\n\
                 sum\n\
             end\n\
             Sayln type_of(result)\n",
            "Int\nInt\n",
        ),
        (
            "values as List[Int] is list [1, 2]\n\
             result is pipeline:\n\
                 values\n\
                 where item > 10\n\
                 sum\n\
             end\n\
             fn as_int() gives Int:\n\
                 return result\n\
             end\n\
             Sayln type_of(as_int())\n",
            "Int\n",
        ),
    ];

    for (source, expected_output) in cases {
        let (valid, _, check_error) = check_source(source);
        assert!(valid, "check rejected the sum result type: {check_error}");
        let (success, output) = run_source_stdout(source);
        assert!(success, "run disagreed with the checked sum type: {output}");
        assert_eq!(output, expected_output);
    }

    let (valid, _, check_error) = check_source(
        "empty as List[Float] is list []\nflow result from empty:\n    parallel 2\n    sum\nend\n",
    );
    assert!(valid, "check rejected parallel Float sum: {check_error}");
    let (success, output) = run_source_stdout(
        "empty as List[Float] is list []\n\
         flow result from empty:\n\
             parallel 2\n\
             sum\n\
         end\n\
         Sayln type_of(result)\n",
    );
    assert!(success, "parallel Float sum failed at runtime: {output}");
    assert_eq!(output, "Float\n");

    let (valid, _, check_error) =
        check_source("result is pipeline:\n    range(4, 2)\n    sum\nend\nSayln type_of(result)\n");
    assert!(valid, "check rejected an empty range sum: {check_error}");
    let (success, output) = run_source_stdout(
        "result is pipeline:\n    range(4, 2)\n    sum\nend\nSayln type_of(result)\n",
    );
    assert!(success, "empty range sum failed at runtime: {output}");
    assert_eq!(output, "Int\n");
}

#[test]
fn builtin_semantics_match_runtime_collection_and_numeric_support() {
    let (success, output) = run_source_stdout(
        "Sayln total(range(1, 4))\n\
         Sayln clamp(5, 0.5, 4)\n\
         Sayln clamp(5.0, 0, 4)\n",
    );
    assert!(success, "{output}");
    assert_eq!(output, "6\n4\n4\n");
    let (success, error) =
        run_source("Sayln total(range(4611686018427387904, 4611686018427387906))\n");
    assert!(!success, "overflowing range total unexpectedly succeeded");
    assert!(error.contains("integer arithmetic error"), "{error}");

    let (success, output) =
        run_source_stdout("result is clamp(5, 0.5, 4)\nSayln type_of(result)\n");
    assert!(success, "{output}");
    assert_eq!(output, "Float\n");

    let (success, stdout) = run_source_stdout(
        "profile is hash:\n    name is \"Ada\"\nend\nSayln join(profile, \",\")\n",
    );
    assert!(success);
    assert_eq!(stdout, "Ada\n");

    for source in ["Sayln any(list [1, 2])\n", "Sayln round(1e308, 15)\n"] {
        let (success, output, error) = check_source(source);
        if source.contains("round") {
            assert!(success, "{output}{error}");
            let (ran, runtime_error) = run_source(source);
            assert!(!ran);
            assert!(
                runtime_error.contains("finite Float range"),
                "{runtime_error}"
            );
        } else {
            assert!(!success, "{error}");
        }
    }
}

#[test]
fn type_of_reports_range_and_csv_stream_as_distinct_runtime_kinds() {
    let (success, stdout) =
        run_source_stdout("Sayln type_of(range(1, 3))\nSayln type_of(csv_rows(\"unused.csv\"))\n");
    assert!(success);
    assert_eq!(stdout, "Range\nCsvStream\n");

    let (success, _, error) = check_source("value as Array[Int] is range(1, 3)\n");
    assert!(!success, "Range must not be an Array type alias");
    assert!(
        error.contains("expected Array[Int], found Range"),
        "{error}"
    );
}

#[test]
fn range_display_is_bounded_for_large_ranges_and_unchanged_for_small_ranges() {
    let (success, stdout) = run_source_stdout("Sayln range(0, 1000000000000)\nSayln range(1, 4)\n");
    assert!(success, "{stdout}");
    assert_eq!(stdout, "Range(0..1000000000000)\n[1, 2, 3]\n");
}

#[test]
fn check_accepts_reversing_a_range_as_supported_by_runtime() {
    let source = "reversed as Array[Int] is reverse(range(0, 3))\nSayln reversed\n";
    let (success, _, error) = check_source(source);
    assert!(
        success,
        "check rejected a runtime-supported sequence: {error}"
    );

    let (success, output) = run_source_stdout(source);
    assert!(success, "{output}");
    assert_eq!(output, "[2, 1, 0]\n");
}

#[test]
fn runs_collection_inspection_builtins() {
    let output = run_example("examples/08-standard-library/inspection.si");
    assert!(output.contains("true"));
    assert!(output.contains("false"));
    assert!(output.contains("List"));
    assert!(output.contains("Output uses Sayln"));
}

#[test]
fn checks_builtin_collection_arguments() {
    let (success, _, error) = check_source("contains(10, 10)\n");
    assert!(!success);
    assert!(error.contains("error[E.semantic.collection.shape-invalid]"));

    let (success, _, error) = check_source("length(10)\n");
    assert!(!success);
    assert!(error.contains("error[E.semantic.collection.shape-invalid]"));

    let (success, _, error) = check_source("contains(\"Ada\", 10)\n");
    assert!(!success);
    assert!(error.contains("expected String, found Int"));
}

#[test]
fn checks_empty_tuple_iteration_without_rejecting_valid_code() {
    let (success, _, error) = check_source("for item in ():\nend\n");
    assert!(success, "unexpected check error: {error}");
}

#[test]
fn checks_tuple_and_matrix_index_shapes() {
    let (success, _, error) = check_source("point is (10, \"Ada\")\nSayln point[2]\n");
    assert!(!success);
    assert!(error.contains("tuple index out of bounds"));

    let (success, _, error) = check_source("m is matrix [[1, 2]]\nSayln m[0]\n");
    assert!(!success);
    assert!(
        error.contains("matrix index requires a tuple of two integers"),
        "unexpected matrix index error: {error}"
    );
}

#[test]
fn indexes_matrix_rows_constructed_from_lists() {
    let source = "grid is matrix [list [1, 2], list [3, 4]]\nSayln grid[1, 0]\n";
    let (checked, _, error) = check_source(source);
    assert!(
        checked,
        "matrix list rows should be statically valid: {error}"
    );

    let (success, output) = run_source_stdout(source);
    assert!(success, "matrix list row indexing failed: {output}");
    assert_eq!(output, "3\n");
}

#[test]
fn matrix_operators_accept_list_rows_like_matrix_builtins() {
    let source = "left is matrix [list [1, 2]]\n\
                  right is matrix [list [3, 4]]\n\
                  total is left + right\n\
                  product_right is matrix [list [3], list [4]]\n\
                  product is left multiply product_right\n\
                  Sayln total[0, 1]\n\
                  Sayln product[0, 0]\n";
    let (success, output) = run_source_stdout(source);
    assert!(success, "matrix operator rejected list rows: {output}");
    assert_eq!(output, "6\n11\n");
}

#[test]
fn matrix_operations_reject_zero_sized_shapes_consistently() {
    for source in [
        "left is matrix []\nright is matrix []\nSayln left + right\n",
        "grid is matrix [[]]\nSayln transpose(grid)\n",
    ] {
        let (success, error) = run_source(source);
        assert!(
            !success,
            "zero-sized matrix operation unexpectedly succeeded"
        );
        assert!(
            error.contains("matrix must not be empty")
                || error.contains("matrix rows must not be empty"),
            "{error}"
        );
    }
}

#[test]
fn matrix_indices_reject_negative_and_unrepresentable_coordinates_safely() {
    for source in [
        "grid is matrix [[1]]\nSayln grid[-1, 0]\n",
        "grid is matrix [[1]]\nSayln grid[9223372036854775807, 0]\n",
    ] {
        let (success, error) = run_source(source);
        assert!(!success, "invalid matrix coordinate unexpectedly succeeded");
        assert!(
            error.contains("matrix index") || error.contains("out of bounds"),
            "{error}"
        );
        assert!(error.contains("error[E"), "{error}");
    }
}

#[test]
fn range_length_reports_unrepresentable_integer_lengths() {
    let (success, output) = run_source_stdout(
        "Sayln length(range(4, 4))\nSayln length(range(4, 2))\nSayln length(range(-2, 3))\n",
    );
    assert!(success, "{output}");
    assert_eq!(output, "0\n0\n5\n");

    let source = "Sayln length(range(-9223372036854775807, 9223372036854775807))\n";
    let (success, error) = run_source(source);
    assert!(
        !success,
        "an unrepresentable range length was silently saturated"
    );
    assert!(
        error.contains("range length exceeds the Int range"),
        "{error}"
    );
}

#[test]
fn stepped_ranges_are_lazy_and_support_both_directions() {
    let source = "up is range(0, 10, 3)\n\
                  down is range(10, 0, -3)\n\
                  Sayln up\n\
                  Sayln down\n\
                  Sayln length(up)\n\
                  Sayln up[2]\n\
                  Sayln range(-9223372036854775807, 9223372036854775807)[1]\n\
                  Sayln contains(down, 4)\n\
                  Sayln contains(down, 5)\n\
                  Sayln reverse(down)\n\
                  [first, ...tail] is down\n\
                  Sayln tail\n\
                  Sayln total(up)\n\
                  Sayln is_empty(range(0, 10, -1))\n\
                  flow stepped_total from range(0, 10, 3):\n\
                      sum\n\
                  end\n\
                  Sayln stepped_total\n";
    let (valid, _, error) = check_source(source);
    assert!(valid, "stepped ranges did not type-check: {error}");
    let (success, output) = run_source_stdout(source);
    assert!(success, "stepped range operations failed: {output}");
    assert_eq!(
        output,
        "[0, 3, 6, 9]\n[10, 7, 4, 1]\n4\n6\n-9223372036854775806\ntrue\nfalse\n[1, 4, 7, 10]\n[7, 4, 1]\n18\ntrue\n18\n"
    );

    let (success, error) = run_source("range(0, 10, 0)\n");
    assert!(!success);
    assert!(error.contains("`range` step cannot be zero"), "{error}");

    let (valid, _, error) = check_source("range(0, 10, 0)\n");
    assert!(!valid);
    assert!(error.contains("`range` step cannot be zero"), "{error}");
}

#[test]
fn hash_indexed_writes_follow_binding_mutability() {
    let source = r#"mut profile is hash:
    name is "Ada"
end
profile["name"] -> "Lin"
Sayln profile.name
"#;
    let (valid, _, error) = check_source(source);
    assert!(
        valid,
        "mutable hash write did not type-check:\n{source}\n{error}"
    );
    let (success, output) = run_source_stdout(source);
    assert!(success, "{output}");
    assert_eq!(output, "Lin\n");

    let source = r#"profile is hash:
    name is "Ada"
end
profile["name"] -> "Lin"
"#;
    let (valid, _, error) = check_source(source);
    assert!(!valid, "immutable hash write was accepted");
    assert!(
        error.contains("cannot mutate immutable variable"),
        "{error}"
    );
}

#[test]
fn rejects_ragged_matrices() {
    let (success, error) = run_source(
        "left is matrix [[1, 2], [3]]\nright is matrix [[1], [2]]\nSayln left + right\n",
    );
    assert!(!success);
    assert!(error.contains("equal widths"));
}

#[test]
fn rejects_ambiguous_collection_definitions() {
    let (success, duplicate_error) =
        run_source("settings is hash:\n    mode is \"a\"\n    mode is \"b\"\nend\n");
    assert!(!success);
    assert!(duplicate_error.contains("duplicate field"));

    let (success, filter_error) =
        run_source("values is list [1]\nresult is pipeline:\n    values\n    where item\nend\n");
    assert!(!success);
    assert!(
        filter_error.contains("must return a boolean"),
        "unexpected filter error: {filter_error}"
    );

    let (success, terminal_error) = run_source(
        "values is list [1]\nresult is pipeline:\n    values\n    count\n    derive item\nend\n",
    );
    assert!(!success);
    assert!(terminal_error.contains("cannot continue"));
}
