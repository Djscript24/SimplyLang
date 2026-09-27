//! formatter.rs — Simply source formatter
//! Formats Simply source while preserving comments and applying consistent indentation around blocks.
//! Key components: format and block/comment parsing helpers.
pub fn format(source: &str) -> String {
    let mut output = String::new();
    let mut blocks = Vec::new();
    let mut pending_blank = false;
    let enum_names = source
        .lines()
        .filter_map(|line| {
            let (code, _) = split_code_comment(line.trim_start());
            let declaration = code.trim();
            let name = declaration.strip_prefix("enum ")?;
            let name = name.split_once(':')?.0.trim();
            (!name.is_empty()).then_some(name.to_owned())
        })
        .collect::<std::collections::HashSet<_>>();

    for raw_line in source.lines() {
        let line = raw_line.trim_start();
        if line.trim().is_empty() {
            pending_blank = !output.is_empty();
            continue;
        }

        if pending_blank && !output.ends_with("\n\n") {
            output.push('\n');
        }
        pending_blank = false;

        let (raw_code, comment) = split_code_comment(line);
        let trimmed_code = raw_code.trim_end();
        let code = format_message_spacing(trimmed_code, &enum_names);
        if matches!(blocks.last(), Some(BlockKind::MatchArm)) && is_match_arm_line(&code) {
            blocks.pop();
        }
        if closes_block(&code) {
            close_block(&mut blocks, &code);
        }
        output.push_str(&"    ".repeat(blocks.len()));
        output.push_str(&code);
        if let Some(comment) = comment {
            output.push_str(&raw_code[trimmed_code.len()..]);
            output.push_str(comment);
        }
        output.push('\n');

        if opens_block(&code) {
            let kind = if is_match_header(&code) {
                BlockKind::Match
            } else if matches!(blocks.last(), Some(BlockKind::Match)) && is_match_arm_line(&code) {
                BlockKind::MatchArm
            } else {
                BlockKind::Normal
            };
            blocks.push(kind);
        }
    }

    if pending_blank && !output.is_empty() && !output.ends_with("\n\n") {
        output.push('\n');
    }
    if !output.is_empty() && !output.ends_with('\n') {
        output.push('\n');
    }
    output
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum BlockKind {
    Normal,
    Match,
    MatchArm,
}

fn close_block(blocks: &mut Vec<BlockKind>, line: &str) {
    if line.trim() == "end" {
        match blocks.pop() {
            Some(BlockKind::MatchArm) => {
                if matches!(blocks.last(), Some(BlockKind::Match)) {
                    blocks.pop();
                }
            }
            Some(BlockKind::Match) | Some(BlockKind::Normal) | None => {}
        }
    } else {
        blocks.pop();
    }
}

fn is_match_header(line: &str) -> bool {
    line.ends_with(':') && line.contains("match ")
}

fn is_match_arm_line(line: &str) -> bool {
    let line = line.trim();
    line.ends_with(':')
        && line.chars().next().is_some_and(|first| {
            first == '|'
                || first == '_'
                || first == '('
                || first == '['
                || first == '{'
                || first == '.'
                || first == '"'
                || first == '-'
                || first.is_ascii_digit()
                || first.is_alphabetic()
        })
}

fn format_message_spacing(line: &str, enum_names: &std::collections::HashSet<String>) -> String {
    let mut output = String::with_capacity(line.len());
    let mut characters = line.char_indices().peekable();
    let mut in_string = false;
    let mut escaped = false;

    while let Some((_, character)) = characters.next() {
        if !in_string && character == '|' {
            while output.chars().last().is_some_and(char::is_whitespace) {
                output.pop();
            }
            output.push_str(" | ");
            while matches!(characters.peek(), Some((_, next)) if next.is_whitespace()) {
                characters.next();
            }
            continue;
        }
        if !in_string && character == '@' {
            while output.chars().last().is_some_and(char::is_whitespace) {
                output.pop();
            }
            output.push_str(" @ ");
            while matches!(characters.peek(), Some((_, next)) if next.is_whitespace()) {
                characters.next();
            }
            continue;
        }
        if !in_string && character == '.' {
            let mut lookahead = characters.clone();
            if matches!(lookahead.next(), Some((_, '.')))
                && matches!(lookahead.next(), Some((_, '.')))
            {
                while output.chars().last().is_some_and(char::is_whitespace) {
                    output.pop();
                }
                if output.ends_with(',') {
                    output.push(' ');
                }
                output.push_str("...");
                characters.next();
                characters.next();
                while matches!(characters.peek(), Some((_, next)) if next.is_whitespace()) {
                    characters.next();
                }
                continue;
            }
            if matches!(characters.peek(), Some((_, '.'))) {
                while output.chars().last().is_some_and(char::is_whitespace) {
                    output.pop();
                }
                output.push_str("..");
                characters.next();
                while matches!(characters.peek(), Some((_, next)) if next.is_whitespace()) {
                    characters.next();
                }
                continue;
            }
        }
        if !in_string && character == ':' && matches!(characters.peek(), Some((_, ':'))) {
            while output.chars().last().is_some_and(char::is_whitespace) {
                output.pop();
            }
            let receiver = output
                .rsplit(|character: char| !(character.is_alphanumeric() || character == '_'))
                .next()
                .unwrap_or_default();
            let enum_variant = enum_names.contains(receiver);
            if !enum_variant && !output.is_empty() {
                output.push(' ');
            }
            output.push_str("::");
            characters.next();
            if enum_variant {
                while matches!(characters.peek(), Some((_, next)) if next.is_whitespace()) {
                    characters.next();
                }
            } else {
                while matches!(characters.peek(), Some((_, next)) if next.is_whitespace()) {
                    characters.next();
                }
                output.push(' ');
            }
            continue;
        }

        output.push(character);
        if character == '"' && !escaped {
            in_string = !in_string;
        }
        escaped = character == '\\' && !escaped;
        if character != '\\' {
            escaped = false;
        }
    }
    output
}

fn opens_block(line: &str) -> bool {
    line.ends_with(':')
}

fn closes_block(line: &str) -> bool {
    let line = line.trim();
    line == "end"
        || line == "else:"
        || line.starts_with("else if ")
        || line == "catch:"
        || line.starts_with("catch ")
        || line == "finally:"
}

fn split_code_comment(line: &str) -> (&str, Option<&str>) {
    let mut in_string = false;
    let mut escaped = false;
    for (index, character) in line.char_indices() {
        if character == '#' && !in_string {
            return (&line[..index], Some(&line[index..]));
        }
        if character == '"' && !escaped {
            in_string = !in_string;
        }
        escaped = character == '\\' && !escaped;
        if character != '\\' {
            escaped = false;
        }
    }
    (line, None)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::format;

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
    fn preserves_string_contents_and_comment_text() {
        let source = "Say \"  # not a comment  \" # keep  \n#  keep trailing spaces  \n";

        assert_eq!(
            format(source),
            "Say \"  # not a comment  \" # keep  \n#  keep trailing spaces  \n"
        );
    }

    #[test]
    fn normalizes_eof_without_adding_duplicate_newlines() {
        let formatted = format("if true:\n  Say \"yes\"\nend");

        assert!(formatted.ends_with('\n'));
        assert_eq!(format(&formatted), formatted);
    }

    #[test]
    fn canonicalizes_message_dispatch_spacing_without_changing_strings() {
        assert_eq!(
            format("person::greet\nperson::rename(\"a::b\")\n"),
            "person :: greet\nperson :: rename(\"a::b\")\n"
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
            "enum Result:\n    Ok as (Int, Int)\nend\nmatch value:\n    (a,(b,c)):\n        a+b+c\n    Result::Ok((left,right)):\n        left+right\n    _:\n        0\nend\n"
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
            "enum Result:\n    Ok as Int\nend\nmatch result:\n    Result::Ok(value) if value>10:\n        value\n    _:\n        0\nend\n"
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
        let source = "match value:\n0 .. 10:\n1\n10 ..:\n2\n.. 10:\n3\n-10 .. 10:\n4\nend\n";
        let formatted = format(source);
        assert_eq!(
            formatted,
            "match value:\n    0..10:\n        1\n    10..:\n        2\n    ..10:\n        3\n    -10..10:\n        4\nend\n"
        );
        assert_eq!(format(&formatted), formatted);
    }

    #[test]
    fn preserves_canonical_sequence_pattern_spacing_and_formats_nested_ranges() {
        let source = "match values:\n[]:\n\"empty\"\n[1, 2, 3]:\n\"exact\"\n[[0 .. 10, 20 .. 30], [1, 2] | [3, 4]]:\n\"nested\"\nend\n";
        let formatted = format(source);
        assert_eq!(
            formatted,
            "match values:\n    []:\n        \"empty\"\n    [1, 2, 3]:\n        \"exact\"\n    [[0..10, 20..30], [1, 2] | [3, 4]]:\n        \"nested\"\nend\n"
        );
        assert_eq!(format(&formatted), formatted);
    }

    #[test]
    fn formats_rest_patterns_canonically_and_keeps_formatted_source_parseable() {
        let source = "match values:\n[...rest]:\nrest\n[head,...tail]:\nhead\n[a, b, ...rest]:\nrest\n[0 .. 10,...rest]:\nrest\n[Person(name, age),...people]:\nname\n[0,...rest]|[1,...rest]:\nrest\nend\n";
        let formatted = format(source);
        assert_eq!(
            formatted,
            "match values:\n    [...rest]:\n        rest\n    [head, ...tail]:\n        head\n    [a, b, ...rest]:\n        rest\n    [0..10, ...rest]:\n        rest\n    [Person(name, age), ...people]:\n        name\n    [0, ...rest] | [1, ...rest]:\n        rest\nend\n"
        );
        assert_eq!(format(&formatted), formatted);
        let parsed =
            crate::parser::Parser::new(crate::lexer::Lexer::new(source).tokenize().unwrap())
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
        let source = "match record:\n{\"name\": person, extra: [0 .. 10, _]}|{name: person, extra: [11 .., _]}:\nperson\n{}:\n\"other\"\nend\n";
        let formatted = format(source);
        assert_eq!(
            formatted,
            "match record:\n    {\"name\": person, extra: [0..10, _]} | {name: person, extra: [11.., _]}:\n        person\n    {}:\n        \"other\"\nend\n"
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
            "{\"name\": name, \"age\": 18..}",
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
        let source = "enum Result:\n    Ok as Int\nend\nmatch value:\nx@18 ..:\nx\nitem@Result::Ok(value):\nitem\nwhole@{name: name}:\nwhole\nrow@[head,...tail]:\nrow\nend\n";
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
            "examples/06-collections/arrays-lists.si",
            "examples/06-collections/hash-tree.si",
            "examples/06-collections/matrices.si",
            "examples/06-collections/tuples.si",
            "examples/07-pipelines/collections.si",
            "examples/08-standard-library/builtins.si",
            "examples/08-standard-library/imported-values.si",
            "examples/08-standard-library/inspection.si",
            "examples/09-quality/message-objects.si",
            "examples/09-quality/scope-and-short-circuit.si",
            "examples/13-objects/person.si",
            "examples/14-enums/result.si",
            "examples/patterns/literal-patterns.si",
            "examples/19-user-input/ask.si",
            "examples/patterns/range-patterns.si",
            "examples/patterns/sequence-patterns.si",
            "examples/patterns/rest-patterns.si",
            "examples/patterns/hash-patterns.si",
            "examples/patterns/alias-patterns.si",
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
}
