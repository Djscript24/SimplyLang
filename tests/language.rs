mod common;
use common::*;

use std::{fs, path::Path, process::Command};

fn run_fixture(command: &str, fixture: &str) -> std::process::Output {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    Command::new(env!("CARGO_BIN_EXE_simply"))
        .arg(command)
        .arg(root.join("tests/fixtures/nominal-identity").join(fixture))
        .current_dir(root)
        .output()
        .expect("failed to run Simply fixture")
}

#[test]
fn rejects_same_scope_duplicate_declarations() {
    let (success, stderr) = run_source("x is 10\nx is 20\n");
    assert!(!success);
    assert!(stderr.contains("variable `x` is already declared in this scope"));
    assert!(stderr.contains("declare it with `mut` to reassign it"));
    assert!(stderr.contains("error[E.runtime.declaration.conflict] (Runtime error)"));
    assert!(stderr.contains(":2:1"));
    assert!(stderr.contains("2 | x is 20"));
}

#[test]
fn allows_reassignment_and_nested_shadowing() {
    let (success, stdout) = run_source_stdout("mut x is 10\nx -> 20\nSayln x\n");
    assert!(success, "reassignment should succeed");
    assert!(stdout.contains("20"));

    let (success, stdout) =
        run_source_stdout("x is 10\nif true:\n    x is 20\n    Sayln x\nend\nSayln x\n");
    assert!(success, "nested shadowing should succeed");
    assert!(stdout.contains("20"));
    assert!(stdout.contains("10"));
}

#[test]
fn immutable_bindings_reject_reassignment_and_collection_mutation() {
    let (success, error) = run_source("value is 1\nvalue -> 2\n");
    assert!(!success);
    assert!(error.contains("immutable variable `value`"));

    let (success, error) = run_source("values is list [1]\nvalues add 2\n");
    assert!(!success);
    assert!(error.contains("immutable variable `values`"));

    let (success, error) = run_source("values is list [1]\nvalues[0] is 2\n");
    assert!(!success);
    assert!(error.contains("immutable variable `values`"));
}

#[test]
fn loop_variables_are_rebound_without_bypassing_their_mutability_rules() {
    let (success, output) = run_source_stdout("for item in [1, 2, 3]:\n    Sayln item\nend\n");
    assert!(success);
    assert_eq!(output, "1\n2\n3\n");

    let (success, error) = run_source("for item in [1, 2]:\n    item -> 9\nend\n");
    assert!(!success);
    assert!(error.contains("immutable variable `item`"));
}

#[test]
fn mutable_bindings_support_typed_reassignment_and_collection_mutation() {
    let (success, stdout) = run_source_stdout(
        "mut value as Int is 1\nvalue -> 2\nmut values is list [1]\nvalues add 2\nSayln value\nSayln values\n",
    );
    assert!(success);
    assert_eq!(stdout.lines().collect::<Vec<_>>(), ["2", "[1, 2]"]);
}

#[test]
fn say_joins_output_and_sayln_terminates_the_line() {
    let (success, output) = run_source_stdout("Say \"Hello, \"\nSayln \"world\"\nSay \"!\"\n");
    assert!(success);
    assert_eq!(output, "Hello, world\n!");
}

#[test]
fn parses_message_dispatch_as_an_expression() {
    let source = "fn make_value(value):\n    return value\nend\n\
                 fn name(self):\n    return self\nend\n\
                 result is (make_value(\"Ada\")) :: name\nSayln result\n";
    let (success, error) = run_source(source);
    assert!(success, "message expression failed: {error}");
    let (_, output) = run_source_stdout(source);
    assert_eq!(output, "Ada\n");
}

#[test]
fn keeps_runtime_types_aligned_with_shadowed_scopes() {
    let source = "mut value is 1\nif true:\n    mut value is \"inner\"\n    value -> \"updated\"\nend\nvalue -> 2\nfn update(mut value):\n    value -> \"local\"\n    return value\nend\nSayln update(\"initial\")\nSayln value\n";
    let (success, error) = run_source(source);
    assert!(success, "shadowed bindings produced an error: {error}");
}

#[test]
fn indexes_strings_by_unicode_scalar_value() {
    let (success, output) =
        run_source_stdout("text is \"Aé🦀Z\"\nSayln text[0]\nSayln text[1]\nSayln text[3]\n");
    assert!(success, "{output}");
    assert_eq!(output, "A\né\nZ\n");

    let (success, output) =
        run_source_stdout("Sayln length(\"é\")\nSayln substring(\"é\", 0, 1)\n");
    assert!(success, "{output}");
    assert_eq!(output, "2\ne\n");

    let (success, error) = run_source("text is \"\"\nSayln text[0]\n");
    assert!(!success);
    assert!(error.contains("string index out of bounds"), "{error}");

    let (success, error) = run_source("text is \"abc\"\nSayln text[3]\n");
    assert!(!success);
    assert!(error.contains("string index out of bounds"), "{error}");

    let (success, _, error) = check_source("Sayln \"abc\"[0]\n");
    assert!(success, "{error}");
}

#[test]
fn substrings_use_unicode_scalar_offsets_and_allow_empty_boundaries() {
    let (success, output) = run_source_stdout(
        "Sayln substring(\"Aé🦀Z\", 0, 1)\n\
         Sayln substring(\"Aé🦀Z\", 1, 2)\n\
         Sayln substring(\"Aé🦀Z\", 3, 1)\n\
         Sayln substring(\"Aé🦀Z\", 4, 0)\n\
         Sayln substring(\"\", 0, 0)\n",
    );
    assert!(success, "{output}");
    assert_eq!(output, "A\né🦀\nZ\n\n\n");

    for source in [
        "Sayln substring(\"abc\", 4, 0)\n",
        "Sayln substring(\"abc\", 1, 3)\n",
        "Sayln substring(\"abc\", -1, 1)\n",
        "Sayln substring(\"abc\", 0, -1)\n",
    ] {
        let (success, error) = run_source(source);
        assert!(!success, "{source}");
        assert!(
            error.contains("substring is out of bounds")
                || error.contains("start must be a non-negative integer")
                || error.contains("length must be a non-negative integer"),
            "{error}"
        );
    }
}

#[test]
fn character_predicates_cover_lexer_needs_and_validate_scalar_count() {
    let (success, output) = run_source_stdout(
        "Sayln is_ascii_alpha(\"A\")\n\
         Sayln is_ascii_alpha(\"é\")\n\
         Sayln is_ascii_digit(\"7\")\n\
         Sayln is_ascii_digit(\"٣\")\n\
         Sayln is_whitespace(\" \")\n\
         Sayln is_whitespace(\"\\n\")\n",
    );
    assert!(success, "{output}");
    assert_eq!(output, "true\nfalse\ntrue\nfalse\ntrue\ntrue\n");

    for source in [
        "Sayln is_ascii_alpha(\"\")\n",
        "Sayln is_ascii_digit(\"ab\")\n",
    ] {
        let (success, error) = run_source(source);
        assert!(!success, "{source}");
        assert!(
            error.contains("exactly one Unicode scalar value"),
            "{error}"
        );
    }

    let (success, _, error) = check_source("Sayln is_ascii_digit(1)\n");
    assert!(!success);
    assert!(error.contains("expected String, found Int"), "{error}");
}

#[test]
fn characters_materializes_unicode_scalars_for_linear_traversal() {
    let (success, output) =
        run_source_stdout("Sayln characters(\"Aé🦀é\")\nSayln type_of(characters(\"\"))\n");
    assert!(success, "{output}");
    assert_eq!(output, "[A, é, 🦀, e, ́]\nArray\n");

    let (success, output) = run_source_stdout("Sayln characters(\"\")\n");
    assert!(success, "{output}");
    assert_eq!(output, "[]\n");

    let (success, _, error) = check_source("Sayln characters(10)\n");
    assert!(!success);
    assert!(error.contains("expected String, found Int"), "{error}");
}

#[test]
fn regex_builtins_extract_matches_replace_text_and_report_invalid_patterns() {
    let source = r#"text is "Ada 42 and Zoë 7"
Sayln regex_find_all(text, "\\p{L}+")
Sayln regex_find_all(text, "[0-9]+")
Sayln regex_replace(text, "[0-9]+", "[number]")
"#;
    let (success, output) = run_source_stdout(source);
    assert!(success, "{output}");
    assert_eq!(
        output,
        "[Ada, and, Zoë]\n[42, 7]\nAda [number] and Zoë [number]\n"
    );

    let (success, error) = run_source("regex_find_all(\"text\", \"[\")\n");
    assert!(!success);
    assert!(error.contains("invalid regular expression"), "{error}");

    let (success, _, error) = check_source("regex_replace(\"text\", 1, \"value\")\n");
    assert!(!success);
    assert!(error.contains("expected String, found Int"), "{error}");
}

#[test]
fn score_rules_returns_weighted_total_and_explanations() {
    let source = r#"rules is [
    ("income", true, 3),
    ("debt", false, 2),
    ("history", true, -1)
]
result is score_rules(rules)
Sayln result["score"]
Sayln result["matched"]
Sayln result["unmatched"]
"#;
    let (success, output) = run_source_stdout(source);
    assert!(success, "{output}");
    assert_eq!(output, "2\n[income, history]\n[debt]\n");

    let (success, error) = run_source("score_rules([(1, true, 2)])\n");
    assert!(!success);
    assert!(
        error.contains("label in rule 1 must be a String"),
        "{error}"
    );

    let (success, _, error) = check_source("score_rules(1)\n");
    assert!(!success);
    assert!(
        error.contains("expects an Array, List, or Tuple"),
        "{error}"
    );
}

#[test]
fn json_builtins_round_trip_values_and_report_unsupported_or_invalid_input() {
    let source = r#"data is parse_json("{\"name\":\"Mina\",\"ok\":true,\"items\":[null,2.5]}")
Sayln data["name"]
Sayln data["ok"]
Sayln to_json(data)
"#;
    let (success, output) = run_source_stdout(source);
    assert!(success, "{output}");
    assert_eq!(
        output,
        "Mina\ntrue\n{\"items\":[null,2.5],\"name\":\"Mina\",\"ok\":true}\n"
    );

    let (success, error) = run_source("parse_json(\"{\")\n");
    assert!(!success);
    assert!(error.contains("could not parse JSON"), "{error}");

    let (success, error) = run_source("to_json(range(1, 3))\n");
    assert!(!success);
    assert!(
        error.contains("cannot serialize a Range to JSON"),
        "{error}"
    );

    let (success, _, error) = check_source("parse_json(1)\n");
    assert!(!success);
    assert!(error.contains("expected String, found Int"), "{error}");

    let (success, _, error) = check_source("Sayln read_json(\"data.json\")[\"name\"]\n");
    assert!(success, "{error}");
}

#[test]
fn pretty_json_builtin_formats_nested_values() {
    let source = r#"data is parse_json("{\"values\":[1,true]}")
Sayln to_json_pretty(data)
"#;
    let (success, output) = run_source_stdout(source);
    assert!(success, "{output}");
    assert_eq!(output, "{\n  \"values\": [\n    1,\n    true\n  ]\n}\n");
}

#[test]
fn json_file_builtins_read_and_atomically_write_json_values() {
    let id = TEMP_SOURCE_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!("simply-json-files-{}-{id}", std::process::id()));
    fs::create_dir_all(&root).expect("failed to create JSON test directory");
    let input = root.join("input.json");
    let output = root.join("output.json");
    let pretty_output = root.join("pretty-output.json");
    fs::write(&input, r#"{"count":3,"ok":true}"#).expect("failed to write JSON test input");
    let source = format!(
        "value is read_json({input:?})\n\
         write_json({output:?}, value)\n\
         write_json_pretty({pretty_output:?}, value)\n\
         Sayln to_json(read_json({output:?}))\n"
    );

    let (success, stdout) = run_source_stdout(&source);
    assert!(success, "{stdout}");
    assert_eq!(stdout, "{\"count\":3,\"ok\":true}\n");
    assert_eq!(
        fs::read_to_string(&output).expect("failed to read JSON output"),
        r#"{"count":3,"ok":true}"#
    );
    assert_eq!(
        fs::read_to_string(&pretty_output).expect("failed to read pretty JSON output"),
        "{\n  \"count\": 3,\n  \"ok\": true\n}"
    );
    fs::remove_dir_all(root).expect("failed to remove JSON test directory");
}

#[test]
fn json_lines_builtins_round_trip_sequences_and_report_invalid_line_numbers() {
    let id = TEMP_SOURCE_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!("simply-json-lines-{}-{id}", std::process::id()));
    fs::create_dir_all(&root).expect("failed to create JSON Lines test directory");
    let input = root.join("input.jsonl");
    let output = root.join("output.jsonl");
    let append_target = root.join("append.jsonl");
    fs::write(&input, "{\"id\":1}\n\n{\"id\":2}\n").expect("failed to write JSON Lines input");
    fs::write(&append_target, "{\"id\":1}").expect("failed to write append test input");
    let source = format!(
        "records is read_json_lines({input:?})\n\
         Sayln records[1][\"id\"]\n\
         write_json_lines({output:?}, records)\n\
         append_json_line({append_target:?}, records[1])\n"
    );

    let (success, stdout) = run_source_stdout(&source);
    assert!(success, "{stdout}");
    assert_eq!(stdout, "2\n");
    assert_eq!(
        fs::read_to_string(&output).expect("failed to read JSON Lines output"),
        "{\"id\":1}\n{\"id\":2}\n"
    );
    assert_eq!(
        fs::read_to_string(&append_target).expect("failed to read appended JSON Lines output"),
        "{\"id\":1}\n{\"id\":2}\n"
    );

    fs::write(&input, "{\"id\":1}\nnot-json\n").expect("failed to write malformed JSON Lines");
    let malformed = format!("read_json_lines({input:?})\n");
    let (success, error) = run_source(&malformed);
    assert!(!success);
    assert!(error.contains("invalid JSON on line 2"), "{error}");
    fs::remove_dir_all(root).expect("failed to remove JSON Lines test directory");
}

#[test]
fn same_named_structs_from_distinct_modules_are_incompatible() {
    let output = run_fixture("check", "check-struct-identity.si");
    let diagnostic = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "{diagnostic}");
    assert!(
        diagnostic.contains("expected Item, found Item"),
        "{diagnostic}"
    );
    assert!(!diagnostic.contains("memory://"), "{diagnostic}");
}

#[test]
fn same_named_enums_from_distinct_modules_are_incompatible() {
    let output = run_fixture("check", "check-enum-identity.si");
    let diagnostic = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "{diagnostic}");
    assert!(
        diagnostic.contains("expected State, found State"),
        "{diagnostic}"
    );
    assert!(!diagnostic.contains("memory://"), "{diagnostic}");
}

#[test]
fn module_returned_values_retain_identity_for_struct_and_enum_patterns() {
    for (fixture, expected_pattern_error, expected_output) in [
        (
            "struct-pattern.si",
            "struct pattern `Item` cannot match value of type Item",
            "foreign\n",
        ),
        (
            "enum-pattern.si",
            "pattern `State::Ready` cannot match value of type State",
            "foreign\n",
        ),
    ] {
        let check = run_fixture("check", fixture);
        let diagnostic = String::from_utf8_lossy(&check.stderr);
        assert!(!check.status.success(), "{diagnostic}");
        assert!(diagnostic.contains(expected_pattern_error), "{diagnostic}");

        let run = run_fixture("run", fixture);
        assert!(
            run.status.success(),
            "{}",
            String::from_utf8_lossy(&run.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&run.stdout), expected_output);
    }
}

#[test]
fn message_dispatch_does_not_cross_dispatch_same_named_structs() {
    let output = run_fixture("run", "message-dispatch.si");
    let diagnostic = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "{diagnostic}");
    assert!(
        diagnostic.contains("message `kind` is not understood by `Item`"),
        "{diagnostic}"
    );
    assert!(!diagnostic.contains("memory://"), "{diagnostic}");
}

#[test]
fn equal_payloads_from_distinct_modules_are_not_equal_values() {
    let output = run_fixture("run", "module-value-equality.si");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "false\nfalse\n");
}

#[test]
fn aliased_struct_types_create_distinct_instances_but_enum_values_compare_structurally() {
    let output = run_fixture("run", "alias-value-equality.si");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "false\ntrue\n");
}

#[test]
fn values_declared_in_one_module_keep_nominal_types_and_equality_rules() {
    let output = run_fixture("run", "same-module.si");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "Item\nState\nfalse\ntrue\n"
    );
}
