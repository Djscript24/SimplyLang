// Internal unit tests for src/formatter.rs.
use std::fs;

use super::{format, statement_text};

#[test]
fn normalizes_block_indentation() {
    assert_eq!(
        format("if true:\nSay \"yes\"\nend\n"),
        "if true:\n    Say \"yes\"\nend\n"
    );
}

#[test]
fn does_not_treat_text_as_a_block() {
    assert_eq!(
        format("Say \"elsewhere: # still text\"\n"),
        "Say \"elsewhere: # still text\"\n"
    );
}

#[test]
fn formats_commented_block_closers() {
    assert_eq!(
        format("if true:\nSay \"yes\"\nend # done\n"),
        "if true:\n    Say \"yes\"\nend # done\n"
    );
}

#[test]
fn is_idempotent_for_nested_blocks_and_blank_lines() {
    let source =
        "fn greet(name):\nif true:\nSay \"hello, \" + name\nelse:\nSay \"no\"\nend\nend\n\n";
    let formatted = format(source);

    assert_eq!(format(&formatted), formatted);
    assert_eq!(
        formatted,
        "fn greet(name):\n    if true:\n        Say \"hello, \" + name\n    else:\n        Say \"no\"\n    end\nend\n\n"
    );
}

#[test]
fn preserves_mutable_ref_parameter_syntax() {
    let source = "fn update(mut ref values as List[Int]):\nvalues[0] -> 7\nend\n";
    let formatted = format(source);

    assert!(formatted.contains("fn update(mut ref values as List[Int]):"), "{formatted}");
    assert_eq!(format(&formatted), formatted);
}

#[test]
fn preserves_string_contents_and_comment_text() {
    let source = "Say \"  # not a comment  \" # keep  \n#  keep trailing spaces  \n";

    assert_eq!(
        format(source),
        "Say \"  # not a comment  \" # keep  \n#  keep trailing spaces  \n"
    );
}

#[test]
fn keeps_match_arm_header_comments_with_their_arms() {
    let source = "result is match 1:\n\
                      1: # first arm\n\
                      10\n\
                      _: # fallback arm\n\
                      0\n\
                      end\n\
                      Sayln result\n";
    let formatted = format(source);

    assert!(formatted.contains("    1: # first arm\n"), "{formatted}");
    assert!(formatted.contains("    _: # fallback arm\n"), "{formatted}");
    assert!(!formatted.contains("# first arm\nend"), "{formatted}");
    assert_eq!(format(&formatted), formatted);
}

#[test]
fn formats_nested_same_line_expressions_from_the_ast() {
    let source = "answer is outer(inner([1,2]),(3+4)*5)\n\
                      values is list [1, 2, 3]\n\
                      selected is match values[0]:\n\
                      [head, ...tail]:\n\
                      Sayln nested(head, tail)\n\
                      head + tail[0]\n\
                      _:\n\
                      0\n\
                      end\n";
    let formatted = format(source);
    let parse = |source: &str| {
        crate::parser::Parser::new(crate::lexer::Lexer::new(source).tokenize().unwrap())
            .parse()
            .unwrap()
    };
    let original = parse(source);
    let parsed = parse(&formatted);

    fn selected_match(program: &crate::ast::Program) -> &[crate::ast::MatchArm] {
        let crate::ast::Stmt::Located { statement, .. } = &program.statements[2] else {
            panic!("expected located assignment");
        };
        let crate::ast::Stmt::Assign {
            value: crate::ast::Expr::Match { arms, .. },
            ..
        } = statement.as_ref()
        else {
            panic!("expected assignment to a match expression");
        };
        arms
    }
    let original_arms = selected_match(&original);
    let formatted_arms = selected_match(&parsed);

    assert_eq!(format(&formatted), formatted);
    assert_eq!(parsed.statements.len(), 3);
    assert_eq!(original_arms.len(), formatted_arms.len());
    for (original, formatted) in original_arms.iter().zip(formatted_arms) {
        assert_eq!(original.pattern, formatted.pattern);
        assert_eq!(original.guard, formatted.guard);
        assert_eq!(original.result, formatted.result);
        assert_eq!(original.body.len(), formatted.body.len());
        for (original, formatted) in original.body.iter().zip(&formatted.body) {
            assert_eq!(statement_text(original), statement_text(formatted));
        }
    }
    assert!(formatted_arms[0].result.is_some());
    assert!(formatted.contains("outer(inner([1, 2]), (3 + 4) * 5)"));
    assert!(formatted.contains("match values[0]:\n"));
}

#[test]
fn formats_flow_steps_and_preserves_their_ast() {
    let source = "flow totals from [1,2,3]:\n\
                      where item>1\n\
                      derive item*2\n\
                      sum\n\
                      end\n";
    let formatted = format(source);
    let parse = |source: &str| {
        crate::parser::Parser::new(crate::lexer::Lexer::new(source).tokenize().unwrap())
            .parse()
            .unwrap()
    };

    assert_eq!(format(&formatted), formatted);
    assert_eq!(parse(source), parse(&formatted));
    assert_eq!(
        formatted,
        "flow totals from [1, 2, 3]:\n    where item > 1\n    derive item * 2\n    sum\nend\n"
    );
}

#[test]
fn invalid_source_returns_unchanged_without_panicking() {
    let source = "if true:\nSay \"unterminated\n";
    assert_eq!(format(source), source);
}

#[test]
fn normalizes_eof_without_adding_duplicate_newlines() {
    let formatted = format("if true:\n  Say \"yes\"\nend");

    assert!(formatted.ends_with('\n'));
    assert_eq!(format(&formatted), formatted);
}

#[test]
fn canonicalizes_message_dispatch_spacing_without_changing_strings() {
    for source in [
        "person::greet\n",
        "person ::greet\n",
        "person:: greet\n",
        "person :: greet\n",
    ] {
        assert_eq!(format(source), "person :: greet\n");
    }
    assert_eq!(
        format("person::rename(\"a::b\") # note\n"),
        "person :: rename(\"a::b\") # note\n"
    );
}

#[test]
fn canonicalizes_import_spacing_and_preserves_trailing_comments() {
    assert_eq!(
        format(
            "open   \"nested/math.si\"   as   math # library\n\
                 open \"values.si\" as values\n"
        ),
        "open \"nested/math.si\" as math # library\nopen \"values.si\" as values\n"
    );
}

#[test]
fn preserves_enum_variant_separator_and_canonicalizes_match_blocks() {
    let source = "enum Result:\nOk as Int\nError as String\nend\n\
                     result is Result :: Ok(42)\n\
                     match result:\n\
                     Result :: Ok(value):\n\
                     value\n\
                     Result::Error(message):\n\
                     message\n\
                     end\n";
    let formatted = format(source);
    assert!(formatted.contains("result is Result::Ok(42)\n"));
    assert!(formatted.contains("Result::Ok(value):\n"));
    assert!(formatted.contains("Result::Error(message):\n"));
    assert_eq!(format(&formatted), formatted);
}

#[test]
fn preserves_nested_tuple_and_enum_match_pattern_layout() {
    let source = "enum Result:\n    Ok as (Int, Int)\nend\n\
                     match value:\n\
                     (a,(b,c)):\n\
                     a+b+c\n\
                     Result::Ok((left,right)):\n\
                     left+right\n\
                     _:\n\
                     0\n\
                     end\n";
    let formatted = format(source);
    assert_eq!(
        formatted,
        "enum Result:\n    Ok as (Int, Int)\nend\nmatch value:\n    (a,(b,c)):\n        a + b + c\n    Result::Ok((left,right)):\n        left + right\n    _:\n        0\nend\n"
    );
    assert_eq!(format(&formatted), formatted);
}

#[test]
fn preserves_guard_spacing_in_match_arm_headers() {
    let source = "enum Result:\n    Ok as Int\nend\n\
                     match result:\n\
                     Result::Ok(value) if value>10:\n\
                     value\n\
                     _:\n\
                     0\n\
                     end\n";
    let formatted = format(source);
    assert_eq!(
        formatted,
        "enum Result:\n    Ok as Int\nend\nmatch result:\n    Result::Ok(value) if value > 10:\n        value\n    _:\n        0\nend\n"
    );
    assert_eq!(format(&formatted), formatted);
}

#[test]
fn canonicalizes_spacing_around_or_pattern_alternatives() {
    let source = "enum Result:\n    Ok as Int\n    Error as String\nend\n\
                     match result:\n\
                     Result::Ok(value)|Result::Error(_):\n\
                     value\n\
                     end\n";
    let formatted = format(source);
    assert!(formatted.contains("Result::Ok(value) | Result::Error(_):\n"));
    assert_eq!(format(&formatted), formatted);
}

#[test]
fn formats_literal_match_arms_without_changing_literal_spelling() {
    let source = "match value:\n0:\n\"zero\"\n1|2|3:\n\"small\"\n\"hello\":\n\"greeting\"\ntrue:\n\"yes\"\nend\n";
    let formatted = format(source);
    assert_eq!(
        formatted,
        "match value:\n    0:\n        \"zero\"\n    1 | 2 | 3:\n        \"small\"\n    \"hello\":\n        \"greeting\"\n    true:\n        \"yes\"\nend\n"
    );
    assert_eq!(format(&formatted), formatted);
}

#[test]
fn formats_range_pattern_bounds_without_surrounding_spaces() {
    let source = "match value:\n0 ..= 10:\n1\n10 ..=:\n2\n..= 10:\n3\n-10 ..= 10:\n4\nend\n";
    let formatted = format(source);
    assert_eq!(
        formatted,
        "match value:\n    0..=10:\n        1\n    10..:\n        2\n    ..=10:\n        3\n    -10..=10:\n        4\nend\n"
    );
    assert_eq!(format(&formatted), formatted);
}

#[test]
fn formatter_preserves_half_open_and_inclusive_pattern_endpoints() {
    let source = "match value:\n1 .. 5:\n0\n1 ..= 5:\n1\nend\n";
    let formatted = format(source);
    assert_eq!(
        formatted,
        "match value:\n    1..5:\n        0\n    1..=5:\n        1\nend\n"
    );
    assert_eq!(format(&formatted), formatted);
}

#[test]
fn preserves_canonical_sequence_pattern_spacing_and_formats_nested_ranges() {
    let source = "match values:\n[]:\n\"empty\"\n[1, 2, 3]:\n\"exact\"\n[[0 ..= 10, 20 ..= 30], [1, 2] | [3, 4]]:\n\"nested\"\nend\n";
    let formatted = format(source);
    assert_eq!(
        formatted,
        "match values:\n    []:\n        \"empty\"\n    [1, 2, 3]:\n        \"exact\"\n    [[0..=10, 20..=30], [1, 2] | [3, 4]]:\n        \"nested\"\nend\n"
    );
    assert_eq!(format(&formatted), formatted);
}

#[test]
fn formats_rest_patterns_canonically_and_keeps_formatted_source_parseable() {
    let source = "match values:\n[...rest]:\nrest\n[head,...tail]:\nhead\n[a, b, ...rest]:\nrest\n[0 ..= 10,...rest]:\nrest\n[Person(name, age),...people]:\nname\n[0,...rest]|[1,...rest]:\nrest\nend\n";
    let formatted = format(source);
    assert_eq!(
        formatted,
        "match values:\n    [...rest]:\n        rest\n    [head, ...tail]:\n        head\n    [a, b, ...rest]:\n        rest\n    [0..=10, ...rest]:\n        rest\n    [Person(name, age), ...people]:\n        name\n    [0, ...rest] | [1, ...rest]:\n        rest\nend\n"
    );
    assert_eq!(format(&formatted), formatted);
    let parsed = crate::parser::Parser::new(crate::lexer::Lexer::new(source).tokenize().unwrap())
        .parse()
        .expect("rest patterns should parse");
    let formatted_parsed =
        crate::parser::Parser::new(crate::lexer::Lexer::new(&formatted).tokenize().unwrap())
            .parse()
            .expect("formatted rest patterns should parse");
    assert_eq!(parsed, formatted_parsed);
}

#[test]
fn formats_hash_patterns_and_preserves_their_parsed_structure() {
    let source = "match record:\n{\"name\": person, extra: [0 ..= 10, _]}|{name: person, extra: [11 ..=, _]}:\nperson\n{}:\n\"other\"\nend\n";
    let formatted = format(source);
    assert_eq!(
        formatted,
        "match record:\n    {name: person, extra: [0..=10, _]} | {name: person, extra: [11.., _]}:\n        person\n    {}:\n        \"other\"\nend\n"
    );
    assert_eq!(format(&formatted), formatted);
    let parse = |source: &str| {
        crate::parser::Parser::new(crate::lexer::Lexer::new(source).tokenize().unwrap())
            .parse()
            .unwrap()
    };
    assert_eq!(parse(source), parse(&formatted));

    for pattern in [
        "{}",
        "{\"name\": name}",
        "{name: name}",
        "{\"name\": name, \"age\": 18..=}",
        "{user: {name: name}}",
        "{\"items\": [head, ...tail]}",
        "{\"point\": (x, 0..)}",
    ] {
        let source = format!("match record:\n{pattern}:\n0\nend\n");
        let formatted = format(&source);
        assert_eq!(format(&formatted), formatted, "{pattern}");
        assert_eq!(parse(&source), parse(&formatted), "{pattern}");
    }
}

#[test]
fn formats_alias_patterns_and_preserves_their_parsed_structure() {
    let source = "enum Result:\n    Ok as Int\nend\nmatch value:\nx@18 ..=:\nx\nitem@Result::Ok(value):\nitem\nwhole@{name: name}:\nwhole\nrow@[head,...tail]:\nrow\nend\n";
    let formatted = format(source);
    assert_eq!(
        formatted,
        "enum Result:\n    Ok as Int\nend\nmatch value:\n    x @ 18..:\n        x\n    item @ Result::Ok(value):\n        item\n    whole @ {name: name}:\n        whole\n    row @ [head, ...tail]:\n        row\nend\n"
    );
    assert_eq!(format(&formatted), formatted);
    let parse = |source: &str| {
        crate::parser::Parser::new(crate::lexer::Lexer::new(source).tokenize().unwrap())
            .parse()
            .unwrap()
    };
    assert_eq!(parse(source), parse(&formatted));
}

#[test]
fn formats_destructuring_targets_and_round_trips_their_ast() {
    let source = "pair is (1, 2)\n\
                      (a, b) is pair\n\
                      (a, (b, c)) is nested\n\
                      (_, value) is pair\n\
                      (a, b) -> pair\n\
                      (a, (b, c)) -> nested\n\
                      (_, b) -> pair\n\
                      [a, b] is values\n\
                      [a, b] -> values\n\
                      [head,...tail] is values\n\
                      [head,...tail] -> values\n\
                      [...rest] is values\n\
                      [...rest] -> values\n\
                      [[a, b], [c, d]] is grid\n\
                      [[head,...tail],...rows] is grid\n";
    let formatted = format(source);
    assert!(formatted.contains("[head, ...tail] is values\n"));
    assert!(formatted.contains("[[head, ...tail], ...rows] is grid\n"));
    assert_eq!(format(&formatted), formatted);
    let parse = |source: &str| {
        crate::parser::Parser::new(crate::lexer::Lexer::new(source).tokenize().unwrap())
            .parse()
            .unwrap()
    };
    assert_eq!(parse(source), parse(&formatted));
}

#[test]
fn formats_all_examples_idempotently() {
    let examples = [
        "examples/01-basics/values.si",
        "examples/02-variables/assignment.si",
        "examples/03-operators/arithmetic.si",
        "examples/03-operators/logic.si",
        "examples/04-control-flow/break-continue.si",
        "examples/04-control-flow/conditionals.si",
        "examples/04-control-flow/loops.si",
        "examples/04-control-flow/while.si",
        "examples/05-functions/functions.si",
        "examples/05-functions/closures.si",
        "examples/06-collections/arrays-lists.si",
        "examples/06-collections/hashes.si",
        "examples/06-collections/matrices.si",
        "examples/06-collections/tuples.si",
        "examples/07-pipelines/collections.si",
        "examples/08-standard-library/builtins.si",
        "examples/08-standard-library/collections.si",
        "examples/08-standard-library/imported-values.si",
        "examples/08-standard-library/inspection.si",
        "examples/08-standard-library/strings.si",
        "examples/09-quality/message-objects.si",
        "examples/09-quality/scope-and-short-circuit.si",
        "examples/10-flow/overview.si",
        "examples/10-flow/aggregates.si",
        "examples/10-flow/checkpoint-write.si",
        "examples/10-flow/csv-cleanup.si",
        "examples/10-flow/parallel-scalar.si",
        "examples/10-flow/partition-categories.si",
        "examples/10-flow/quality-partition.si",
        "examples/11-compiler-foundations/mini-lexer.si",
        "examples/13-objects/person.si",
        "examples/14-enums/result.si",
        "examples/15-patterns/alias-patterns.si",
        "examples/15-patterns/hash-patterns.si",
        "examples/15-patterns/literal-patterns.si",
        "examples/15-patterns/or-patterns.si",
        "examples/15-patterns/pattern-guards.si",
        "examples/15-patterns/range-patterns.si",
        "examples/15-patterns/rest-patterns.si",
        "examples/15-patterns/sequence-patterns.si",
        "examples/15-patterns/struct-patterns.si",
        "examples/16-user-input/ask.si",
        "examples/99-smoke/smoke.si",
    ];

    for path in examples {
        let source = fs::read_to_string(path).expect("failed to read example source");
        let formatted = format(&source);
        assert_eq!(
            format(&formatted),
            formatted,
            "formatter is not idempotent: {path}"
        );
    }
}
