// Internal unit tests for src/parser.rs.
use super::*;
use crate::lexer::Lexer;

fn inner(statement: &Stmt) -> &Stmt {
    match statement {
        Stmt::Located { statement, .. } => statement,
        statement => statement,
    }
}

#[test]
fn parses_say_statements() {
    let tokens = Lexer::new("Say \"Hello\"\nSay 42\nSay 3.5\nSay true\n")
        .tokenize()
        .unwrap();
    let program = Parser::new(tokens).parse().unwrap();
    assert_eq!(program.statements.len(), 4);
    assert_eq!(
        inner(&program.statements[1]),
        &Stmt::Say(Expr::Literal(Literal::Int(42)))
    );
}

#[test]
fn parses_sayln_statements() {
    let tokens = Lexer::new("Sayln \"Hello\"\n").tokenize().unwrap();
    let program = Parser::new(tokens).parse().unwrap();
    assert_eq!(
        inner(&program.statements[0]),
        &Stmt::Sayln(Expr::Literal(Literal::String("Hello".into())))
    );
}

#[test]
fn parses_assignments_and_reassignments() {
    let tokens = Lexer::new("name is \"Simply\"\nage -> \"Rust\"\nSay name\n")
        .tokenize()
        .unwrap();
    let program = Parser::new(tokens).parse().unwrap();
    assert_eq!(
        inner(&program.statements[0]),
        &Stmt::Assign {
            name: "name".into(),
            mutable: false,
            declared_type: None,
            value: Expr::Literal(Literal::String("Simply".into())),
        }
    );
    assert_eq!(
        inner(&program.statements[1]),
        &Stmt::Reassign {
            name: "age".into(),
            value: Expr::Literal(Literal::String("Rust".into())),
        }
    );
}

#[test]
fn parses_destructuring_assignment_without_changing_tuple_or_sequence_expressions() {
    let program = Parser::new(
        Lexer::new(
            "(left, right) -> (1, 2)\n\
                        [head, ...tail] -> values\n\
                        (1, 2)\n\
                        nums is [3, 4]\n",
        )
        .tokenize()
        .unwrap(),
    )
    .parse()
    .unwrap();

    assert!(matches!(
        inner(&program.statements[0]),
        Stmt::DestructureReassign {
            pattern: MatchPattern::Tuple(patterns),
            value: Expr::Tuple(values),
        } if patterns.len() == 2 && values.len() == 2
    ));
    assert!(matches!(
        inner(&program.statements[1]),
        Stmt::DestructureReassign {
            pattern: MatchPattern::Sequence { patterns, rest: Some(rest) },
            value: Expr::Identifier(value),
        } if patterns.len() == 1 && rest == "tail" && value == "values"
    ));
    assert!(matches!(
        inner(&program.statements[2]),
        Stmt::Expression(Expr::Tuple(values)) if values.len() == 2
    ));
    assert!(matches!(
        inner(&program.statements[3]),
        Stmt::Assign {
            value: Expr::Array(values),
            ..
        } if values.len() == 2
    ));
}

#[test]
fn explains_that_typed_assignments_need_is() {
    let tokens = Lexer::new("name as String \"Simply\"\n")
        .tokenize()
        .unwrap();
    let error = Parser::new(tokens).parse().unwrap_err();
    assert!(error.message().contains("typed declaration needs `is`"));
    assert!(error.message().contains("name as String is <value>"));
}

#[test]
fn parses_if_else_blocks() {
    let tokens = Lexer::new("if true:\n    Say \"yes\"\nelse:\n    Say \"no\"\nend\n")
        .tokenize()
        .unwrap();
    let program = Parser::new(tokens).parse().unwrap();
    assert_eq!(program.statements.len(), 1);
    assert!(matches!(inner(&program.statements[0]), Stmt::If { .. }));
}

#[test]
fn parses_functions_and_calls() {
    let tokens = Lexer::new("fn add(a, b):\n    return a + b\nend\nSay add(2, 3)\n")
        .tokenize()
        .unwrap();
    let program = Parser::new(tokens).parse().unwrap();
    assert!(matches!(
        inner(&program.statements[0]),
        Stmt::Function { .. }
    ));
    assert!(matches!(
        inner(&program.statements[1]),
        Stmt::Say(Expr::Call { .. })
    ));
}

#[test]
fn parses_dispatch_calls_and_nested_argument_expressions() {
    let tokens = Lexer::new("result is get_person() :: rename(user :: name, \"Ada\")\n")
        .tokenize()
        .unwrap();
    let program = Parser::new(tokens).parse().unwrap();
    let Stmt::Assign { value, .. } = inner(&program.statements[0]) else {
        panic!("expected assignment");
    };
    assert_eq!(
        value,
        &Expr::MessageDispatch {
            receiver: Box::new(Expr::Call {
                name: "get_person".into(),
                arguments: vec![],
            }),
            message: "rename".into(),
            arguments: vec![
                Expr::MessageDispatch {
                    receiver: Box::new(Expr::Identifier("user".into())),
                    message: "name".into(),
                    arguments: vec![],
                },
                Expr::Literal(Literal::String("Ada".into())),
            ],
        }
    );
}

#[test]
fn treats_omitted_and_empty_message_arguments_equally() {
    let program = Parser::new(
        Lexer::new("person :: greet\nperson :: greet()\n")
            .tokenize()
            .unwrap(),
    )
    .parse()
    .unwrap();
    assert_eq!(inner(&program.statements[0]), inner(&program.statements[1]));
}

#[test]
fn parses_parenthesized_compound_and_nested_receivers() {
    let tokens = Lexer::new("(left + right) :: combine\n(person :: address) :: city\n")
        .tokenize()
        .unwrap();
    let program = Parser::new(tokens).parse().unwrap();
    assert!(matches!(
        inner(&program.statements[0]),
        Stmt::Expression(Expr::MessageDispatch { receiver, .. })
            if matches!(receiver.as_ref(), Expr::Binary { .. })
    ));
    assert!(matches!(
        inner(&program.statements[1]),
        Stmt::Expression(Expr::MessageDispatch {
            receiver,
            message,
            ..
        }) if message == "city"
            && matches!(receiver.as_ref(), Expr::MessageDispatch { message, .. } if message == "address")
    ));
}

#[test]
fn bounds_expression_nesting_with_a_located_parse_error() {
    let nested = format!("Sayln {}1{}\n", "(".repeat(32), ")".repeat(32));
    let program = Parser::new(Lexer::new(&nested).tokenize().unwrap())
        .parse()
        .expect("ordinary nested expressions should parse");
    assert_eq!(program.statements.len(), 1);

    let deeply_nested = format!("Sayln {}1{}\n", "(".repeat(1024), ")".repeat(1024));
    let error = Parser::new(Lexer::new(&deeply_nested).tokenize().unwrap())
        .parse()
        .expect_err("excessive expression nesting should be diagnosed");
    assert_eq!(error.code(), DiagnosticCode::UnexpectedToken);
    assert_eq!(error.span().line, 1);
    assert!(error.message().contains("nesting depth"));
}

#[test]
fn bounds_nested_blocks_without_rejecting_flat_operator_chains() {
    let ordinary_blocks = format!("{}Sayln 1\n{}", "if true:\n".repeat(16), "end\n".repeat(16));
    Parser::new(Lexer::new(&ordinary_blocks).tokenize().unwrap())
        .parse()
        .expect("ordinary nested blocks should parse");

    let nested_blocks = format!(
        "{}Sayln 1\n{}",
        "if true:\n".repeat(1024),
        "end\n".repeat(1024)
    );
    let error = Parser::new(Lexer::new(&nested_blocks).tokenize().unwrap())
        .parse()
        .expect_err("excessive block nesting should be diagnosed");
    assert_eq!(error.code(), DiagnosticCode::UnexpectedToken);
    assert_eq!(error.span().line, 65);
    assert!(error.message().contains("maximum parser nesting depth"));

    let flat_chain = format!("Sayln {}\n", vec!["1"; 2049].join(" + "));
    Parser::new(Lexer::new(&flat_chain).tokenize().unwrap())
        .parse()
        .expect("a long flat operator chain should not count as nested syntax");
}

#[test]
fn bounds_nested_match_expressions() {
    let nested_matches = format!(
        "Sayln {}{}",
        "match true:\ntrue:\n".repeat(1024),
        "0\nfalse:\n0\nend\n".repeat(1024)
    );
    let error = Parser::new(Lexer::new(&nested_matches).tokenize().unwrap())
        .parse()
        .expect_err("excessive match nesting should be diagnosed");
    assert_eq!(error.code(), DiagnosticCode::UnexpectedToken);
    assert!(error.message().contains("nesting depth"));
}

#[test]
fn bounds_recursive_type_and_pattern_parsing() {
    let nested_type = format!(
        "fn accept(value as {}Int{}):\n    return 1\nend\n",
        "List[".repeat(1024),
        "]".repeat(1024)
    );
    let type_error = Parser::new(Lexer::new(&nested_type).tokenize().unwrap())
        .parse()
        .expect_err("excessive type nesting should be diagnosed");
    assert_eq!(type_error.code(), DiagnosticCode::UnexpectedToken);
    assert!(type_error.message().contains("maximum type nesting depth"));

    let nested_pattern = format!(
        "Sayln match 0:\n{}:\n    1\n_:\n    0\nend\n",
        "(".repeat(1024) + "value" + &")".repeat(1024)
    );
    let pattern_error = Parser::new(Lexer::new(&nested_pattern).tokenize().unwrap())
        .parse()
        .expect_err("excessive pattern nesting should be diagnosed");
    assert_eq!(pattern_error.code(), DiagnosticCode::UnexpectedToken);
    assert!(
        pattern_error
            .message()
            .contains("maximum pattern nesting depth")
    );
}

#[test]
fn distinguishes_enum_construction_from_identifier_message_dispatch() {
    let source = "enum Result:\n    Ok as Int\nend\n\
                      result is Result::Ok(42)\n\
                      person :: greet\n";
    let program = Parser::new(Lexer::new(source).tokenize().unwrap())
        .parse()
        .unwrap();
    assert!(matches!(
        inner(&program.statements[1]),
        Stmt::Assign {
            value: Expr::EnumVariant { enum_name, variant_name, .. },
            ..
        } if enum_name == "Result" && variant_name == "Ok"
    ));
    assert!(matches!(
        inner(&program.statements[2]),
        Stmt::Expression(Expr::MessageDispatch { .. })
    ));
}

#[test]
fn rejects_chained_and_malformed_message_dispatch() {
    for source in [
        "person :: address :: city\n",
        "person ::\n",
        ":: greet\n",
        "person :: \"greet\"\n",
        "person :: 123\n",
        "person :: true\n",
        "person :: rename(,)\n",
        "person :: rename(\"Ada\"\n",
        "person :: rename(\n",
        "person :: rename(\"Ada\",)\n",
    ] {
        assert!(
            Parser::new(Lexer::new(source).tokenize().unwrap())
                .parse()
                .is_err(),
            "accepted invalid dispatch syntax: {source}"
        );
    }
}

#[test]
fn locates_invalid_message_selectors_at_the_selector() {
    let error = Parser::new(Lexer::new("person :: \"greet\"\n").tokenize().unwrap())
        .parse()
        .unwrap_err();
    assert!(matches!(
        error,
        SimplyError::Parse {
            span: Span {
                line: 1,
                column: 11,
                ..
            },
            ..
        }
    ));
}

#[test]
fn parses_struct_fields_and_typed_messages_as_dedicated_nodes() {
    let source = "type Person:\n    name as String\n    age as Int\nend\n\
                      on Person receive rename(value as String):\n    return value\nend\n";
    let program = Parser::new(Lexer::new(source).tokenize().unwrap())
        .parse()
        .unwrap();
    assert!(matches!(
        inner(&program.statements[0]),
        Stmt::Struct { name, fields }
            if name == "Person"
                && fields.len() == 2
                && fields[0].name == "name"
                && fields[0].field_type == Type::String
                && fields[1].name == "age"
                && fields[1].field_type == Type::Int
    ));
    assert!(matches!(
        inner(&program.statements[1]),
        Stmt::Message {
            receiver_type,
            name,
            parameters,
            ..
        } if receiver_type == "Person"
            && name == "rename"
            && parameters == &vec![("value".into(), Some(Type::String), false)]
    ));
}

#[test]
fn rejects_duplicate_fields_and_malformed_object_declarations() {
    for source in [
        "type Person:\n    name as String\n    name as String\nend\n",
        "type Person:\n    name String\nend\n",
        "type Person:\n    name as\nend\n",
        "type Person:\n    name as String\n",
        "on Person greet:\nend\n",
        "on Person receive:\nend\n",
        "on Person receive greet(value,):\nend\n",
    ] {
        assert!(
            Parser::new(Lexer::new(source).tokenize().unwrap())
                .parse()
                .is_err(),
            "accepted invalid object declaration: {source}"
        );
    }
}

#[test]
fn handles_empty_or_eof_less_token_streams_without_panicking() {
    assert!(Parser::new(Vec::new()).parse().is_ok());

    let tokens = vec![Token {
        kind: TokenKind::Say,
        span: Span::new(1, 1),
    }];
    assert!(matches!(
        Parser::new(tokens).parse(),
        Err(SimplyError::Parse {
            code: DiagnosticCode::ExpectedExpression,
            ..
        })
    ));
}
