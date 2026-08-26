pub fn format(source: &str) -> String {
    let mut output = String::new();
    let mut indent = 0usize;

    for raw_line in source.lines() {
        let line = raw_line.trim();
        if line.is_empty() {
            if !output.ends_with("\n\n") {
                output.push('\n');
            }
            continue;
        }

        if closes_block(line) {
            indent = indent.saturating_sub(1);
        }
        output.push_str(&"    ".repeat(indent));
        output.push_str(line);
        output.push('\n');

        if opens_block(line) {
            indent += 1;
        }
    }

    output
}

fn opens_block(line: &str) -> bool {
    code_without_comment(line).trim_end().ends_with(':')
}

fn closes_block(line: &str) -> bool {
    let code = code_without_comment(line).trim();
    code == "end" || code == "else:" || code.starts_with("else if ")
}

fn code_without_comment(line: &str) -> &str {
    let mut in_string = false;
    let mut escaped = false;
    for (index, character) in line.char_indices() {
        if character == '#' && !in_string {
            return &line[..index];
        }
        if character == '"' && !escaped {
            in_string = !in_string;
        }
        escaped = character == '\\' && !escaped;
        if character != '\\' {
            escaped = false;
        }
    }
    line
}

#[cfg(test)]
mod tests {
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
}
