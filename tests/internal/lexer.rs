// Internal unit tests for src/lexer.rs.
use super::*;

#[test]
fn lexes_all_base_values() {
    let tokens = Lexer::new("Say \"hello\"\nSayln -42\nSay 3.14\nSayln true\nSay false\n")
        .tokenize()
        .unwrap();
    let kinds: Vec<TokenKind> = tokens.into_iter().map(|t| t.kind).collect();
    assert_eq!(
        kinds,
        vec![
            TokenKind::Say,
            TokenKind::String("hello".into()),
            TokenKind::Newline,
            TokenKind::Sayln,
            TokenKind::Minus,
            TokenKind::Int(42),
            TokenKind::Newline,
            TokenKind::Say,
            TokenKind::Float(314.0 / 100.0),
            TokenKind::Newline,
            TokenKind::Sayln,
            TokenKind::True,
            TokenKind::Newline,
            TokenKind::Say,
            TokenKind::False,
            TokenKind::Newline,
            TokenKind::Eof,
        ]
    );
}

#[test]
fn lexes_assignment_tokens() {
    let tokens = Lexer::new("name is 10\nname -> 11\n").tokenize().unwrap();
    let kinds: Vec<TokenKind> = tokens.into_iter().map(|token| token.kind).collect();
    assert_eq!(
        kinds,
        vec![
            TokenKind::Identifier("name".into()),
            TokenKind::Is,
            TokenKind::Int(10),
            TokenKind::Newline,
            TokenKind::Identifier("name".into()),
            TokenKind::Arrow,
            TokenKind::Int(11),
            TokenKind::Newline,
            TokenKind::Eof,
        ]
    );
}

#[test]
fn lexes_range_operators_without_splitting_integer_literals_as_floats() {
    let tokens = Lexer::new("0..10 1.5..2.5 ..-1 0..=10 ...tail\n")
        .tokenize()
        .unwrap()
        .into_iter()
        .map(|token| token.kind)
        .collect::<Vec<_>>();
    assert_eq!(
        tokens,
        vec![
            TokenKind::Int(0),
            TokenKind::DotDot,
            TokenKind::Int(10),
            TokenKind::Float(1.5),
            TokenKind::DotDot,
            TokenKind::Float(2.5),
            TokenKind::DotDot,
            TokenKind::Minus,
            TokenKind::Int(1),
            TokenKind::Int(0),
            TokenKind::DotDotEqual,
            TokenKind::Int(10),
            TokenKind::DotDotDot,
            TokenKind::Identifier("tail".into()),
            TokenKind::Newline,
            TokenKind::Eof,
        ]
    );
}

#[test]
fn lexes_rest_patterns_before_range_and_field_access_tokens() {
    let kinds = Lexer::new("...tail ..10 . value.field 1...rest .... .....\n")
        .tokenize()
        .unwrap()
        .into_iter()
        .map(|token| token.kind)
        .collect::<Vec<_>>();
    assert_eq!(
        kinds,
        vec![
            TokenKind::DotDotDot,
            TokenKind::Identifier("tail".into()),
            TokenKind::DotDot,
            TokenKind::Int(10),
            TokenKind::Dot,
            TokenKind::Identifier("value".into()),
            TokenKind::Dot,
            TokenKind::Identifier("field".into()),
            TokenKind::Int(1),
            TokenKind::DotDotDot,
            TokenKind::Identifier("rest".into()),
            TokenKind::DotDotDot,
            TokenKind::Dot,
            TokenKind::DotDotDot,
            TokenKind::DotDot,
            TokenKind::Newline,
            TokenKind::Eof,
        ]
    );
}

#[test]
fn distinguishes_double_colon_from_block_colons() {
    let tokens = Lexer::new("person::greet:\n").tokenize().unwrap();
    assert_eq!(tokens[1].kind, TokenKind::DoubleColon);
    assert_eq!(tokens[3].kind, TokenKind::Colon);
}

#[test]
fn tracks_unicode_and_windows_newlines_without_losing_tokens() {
    let tokens = Lexer::new("Say \"é\"\r\nSay 2\r\n").tokenize().unwrap();
    assert_eq!(tokens[1].kind, TokenKind::String("é".into()));
    assert_eq!(tokens[2].span, Span::new(1, 9));
    assert_eq!(tokens[3].span, Span::new(2, 1));
}

#[test]
fn rejects_unterminated_strings_as_lex_errors() {
    let error = Lexer::new("Say \"unterminated").tokenize().unwrap_err();
    assert!(matches!(error, SimplyError::Lex { .. }));
    assert!(error.to_string().contains("E.lex.string.unterminated"));
}

#[test]
fn reports_invalid_numeric_literals_with_a_numeric_lex_error() {
    for source in ["Say 1e\n", "Say 9223372036854775808\n", "Say 1e9999\n"] {
        let error = Lexer::new(source).tokenize().unwrap_err();

        assert_eq!(error.code(), DiagnosticCode::InvalidNumber, "{source}");
        assert_eq!(error.category(), crate::error::DiagnosticCategory::Lex);
        assert!(
            error.to_string().contains("E.lex.number.invalid"),
            "{source}"
        );
        assert!(
            error
                .to_string()
                .contains("This number literal is not valid."),
            "{source}"
        );
    }
}

#[test]
fn renders_invalid_number_category_and_code() {
    let error = Lexer::new("Say 1e\n").tokenize().unwrap_err();
    let rendered = error.render("number.si", "Say 1e\n");

    assert!(rendered.starts_with("error[E.lex.number.invalid] (Lex error)"));
    assert!(rendered.contains("= What happened: This number literal is not valid."));
    assert!(rendered.contains("= Try this: Check the number's digits"));
    assert!(rendered.contains("= Details: expected digits after exponent"));
    assert!(rendered.contains("1 | Say 1e"));
}

#[test]
fn tracks_scalar_columns_after_unicode_source() {
    let error = Lexer::new("Say \"é你好😀\"\nSay §\n")
        .tokenize()
        .unwrap_err();

    assert_eq!(error.code(), DiagnosticCode::InvalidCharacter);
    assert_eq!(error.span(), &Span::new(2, 5));
}
