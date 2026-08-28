pub fn format(source: &str) -> String {
    let mut output = String::new();
    let mut indent = 0usize;

    for raw_line in source.lines() {
        let line = raw_line.trim_start();
        if line.trim().is_empty() {
            if !output.ends_with("\n\n") {
                output.push('\n');
            }
            continue;
        }

        let (raw_code, comment) = split_code_comment(line);
        let code = raw_code.trim_end();
        if closes_block(code) {
            indent = indent.saturating_sub(1);
        }
        output.push_str(&"    ".repeat(indent));
        output.push_str(code);
        if let Some(comment) = comment {
            output.push_str(&raw_code[code.len()..]);
            output.push_str(comment);
        }
        output.push('\n');

        if opens_block(code) {
            indent += 1;
        }
    }

    output
}

fn opens_block(line: &str) -> bool {
    line.ends_with(':')
}

fn closes_block(line: &str) -> bool {
    let line = line.trim();
    line == "end" || line == "else:" || line.starts_with("else if ")
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
