mod common;
use common::*;

#[test]
fn indexes_strings_by_unicode_scalar_value() {
    let (success, output) = run_source_stdout(
        "text is \"Aé🦀Z\"\n\
         Say text[0]\n\
         Say text[1]\n\
         Say text[3]\n",
    );
    assert!(success, "{output}");
    assert_eq!(output, "A\né\nZ\n");

    let (success, output) = run_source_stdout("Say length(\"é\")\nSay substring(\"é\", 0, 1)\n");
    assert!(success, "{output}");
    assert_eq!(output, "2\ne\n");

    let (success, error) = run_source("text is \"\"\nSay text[0]\n");
    assert!(!success);
    assert!(error.contains("string index out of bounds"), "{error}");

    let (success, error) = run_source("text is \"abc\"\nSay text[3]\n");
    assert!(!success);
    assert!(error.contains("string index out of bounds"), "{error}");

    let (success, _, error) = check_source("Say \"abc\"[0]\n");
    assert!(success, "{error}");
}

#[test]
fn substrings_use_unicode_scalar_offsets_and_allow_empty_boundaries() {
    let (success, output) = run_source_stdout(
        "Say substring(\"Aé🦀Z\", 0, 1)\n\
         Say substring(\"Aé🦀Z\", 1, 2)\n\
         Say substring(\"Aé🦀Z\", 3, 1)\n\
         Say substring(\"Aé🦀Z\", 4, 0)\n\
         Say substring(\"\", 0, 0)\n",
    );
    assert!(success, "{output}");
    assert_eq!(output, "A\né🦀\nZ\n\n\n");

    for source in [
        "Say substring(\"abc\", 4, 0)\n",
        "Say substring(\"abc\", 1, 3)\n",
        "Say substring(\"abc\", -1, 1)\n",
        "Say substring(\"abc\", 0, -1)\n",
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
        "Say is_ascii_alpha(\"A\")\n\
         Say is_ascii_alpha(\"é\")\n\
         Say is_ascii_digit(\"7\")\n\
         Say is_ascii_digit(\"٣\")\n\
         Say is_whitespace(\" \")\n\
         Say is_whitespace(\"\\n\")\n",
    );
    assert!(success, "{output}");
    assert_eq!(output, "true\nfalse\ntrue\nfalse\ntrue\ntrue\n");

    for source in ["Say is_ascii_alpha(\"\")\n", "Say is_ascii_digit(\"ab\")\n"] {
        let (success, error) = run_source(source);
        assert!(!success, "{source}");
        assert!(
            error.contains("exactly one Unicode scalar value"),
            "{error}"
        );
    }

    let (success, _, error) = check_source("Say is_ascii_digit(1)\n");
    assert!(!success);
    assert!(error.contains("expected String, found Int"), "{error}");
}

#[test]
fn characters_materializes_unicode_scalars_for_linear_traversal() {
    let (success, output) =
        run_source_stdout("Say characters(\"Aé🦀é\")\nSay type_of(characters(\"\"))\n");
    assert!(success, "{output}");
    assert_eq!(output, "[A, é, 🦀, e, ́]\nArray\n");

    let (success, output) = run_source_stdout("Say characters(\"\")\n");
    assert!(success, "{output}");
    assert_eq!(output, "[]\n");

    let (success, _, error) = check_source("Say characters(10)\n");
    assert!(!success);
    assert!(error.contains("expected String, found Int"), "{error}");
}
