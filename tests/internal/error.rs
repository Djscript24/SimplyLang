// Internal unit tests for src/error.rs.
use super::{DiagnosticCategory, DiagnosticCode, SimplyError, Span};
use unicode_width::UnicodeWidthStr;

#[test]
fn renders_source_context() {
    let error = SimplyError::Runtime {
        span: Span::with_length(2, 5, 4),
        code: DiagnosticCode::RuntimeGeneral,
        message: "unknown variable `name`".into(),
    };

    let rendered = error.render("example.si", "Say \"ok\"\nSay name\n");
    assert!(
        rendered
            .starts_with("error[E.runtime.operation.failed] (Runtime error)\n  --> example.si:2:5")
    );
    assert!(rendered.contains("= What happened: The program could not complete this operation."));
    assert!(rendered.contains("= Try this: Review the operation"));
    assert!(rendered.contains("= Details: unknown variable `name`"));
    assert!(rendered.contains("\n  |\n2 | Say name\n  |     ^^^^"));
}

#[test]
fn renders_canonical_codes_instead_of_legacy_aliases() {
    let error = SimplyError::Runtime {
        span: Span::new(1, 1),
        code: DiagnosticCode::RuntimeDivision,
        message: "division by zero".into(),
    };

    let rendered = error.render("example.si", "Say 1 / 0\n");
    assert!(rendered.starts_with("error[E.runtime.numeric.division-by-zero]"));
    assert!(!rendered.contains("E0202"));
}

#[test]
fn diagnostic_codes_are_unique() {
    let mut unique = std::collections::HashSet::new();
    assert_eq!(DiagnosticCode::ALL.len(), 40);
    assert!(
        DiagnosticCode::ALL
            .iter()
            .all(|code| unique.insert(code.as_str()))
    );
    assert!(
        DiagnosticCode::ALL
            .iter()
            .all(|code| DiagnosticCode::from_code(code.as_str()) == Some(*code))
    );
    let legacy = DiagnosticCode::ALL
        .iter()
        .filter_map(|code| code.legacy_code())
        .collect::<std::collections::HashSet<_>>();
    assert_eq!(legacy.len(), DiagnosticCode::ALL.len());
    assert!(DiagnosticCode::ALL.iter().all(|code| {
        code.legacy_code()
            .is_some_and(|old| DiagnosticCode::from_code(old) == Some(*code))
    }));
    assert_eq!(DiagnosticCode::from_code("E9999"), None);
}

#[test]
fn diagnostic_paths_have_derivable_parents_and_variable_depth() {
    let shallow = DiagnosticCode::CliUsage;
    let deep = DiagnosticCode::SemanticMatrixIndex;
    assert_eq!(shallow.as_str(), "E.cli.usage");
    assert_eq!(shallow.depth(), 2);
    assert_eq!(shallow.parent_code().as_deref(), Some("E.cli"));
    assert_eq!(shallow.ancestors(), ["E", "E.cli"]);

    assert_eq!(deep.as_str(), "E.semantic.index.matrix.invalid");
    assert_eq!(deep.depth(), 4);
    assert_eq!(
        deep.parent_code().as_deref(),
        Some("E.semantic.index.matrix")
    );
    assert_eq!(
        deep.ancestors(),
        [
            "E",
            "E.semantic",
            "E.semantic.index",
            "E.semantic.index.matrix"
        ]
    );
    assert!(deep.is_descendant_of("E.semantic"));
    assert!(!shallow.is_descendant_of("E.semantic"));
}

#[test]
fn diagnostic_registry_covers_all_top_level_categories() {
    let categories = DiagnosticCode::ALL
        .iter()
        .map(|code| code.category())
        .collect::<std::collections::HashSet<_>>();
    assert_eq!(
        categories,
        [
            DiagnosticCategory::Lex,
            DiagnosticCategory::Parse,
            DiagnosticCategory::Semantic,
            DiagnosticCategory::Runtime,
            DiagnosticCategory::Command,
        ]
        .into_iter()
        .collect()
    );

    let paths = DiagnosticCode::ALL
        .iter()
        .map(|code| code.as_str())
        .collect::<std::collections::HashSet<_>>();
    assert_eq!(paths.len(), DiagnosticCode::ALL.len());
}

#[test]
fn every_diagnostic_has_registered_explanation_and_suggestion() {
    assert!(DiagnosticCode::ALL.iter().all(|code| {
        !code.explanation().trim().is_empty() && !code.suggestion().trim().is_empty()
    }));
}

#[test]
fn diagnostic_category_matches_its_variant_and_code() {
    let error = SimplyError::Semantic {
        span: Span::new(2, 3),
        code: DiagnosticCode::TypeMismatch,
        message: "type mismatch".into(),
    };

    assert_eq!(error.code(), DiagnosticCode::TypeMismatch);
    assert_eq!(error.category(), DiagnosticCategory::Semantic);
    assert_eq!(error.span(), &Span::new(2, 3));
    assert!(
        error
            .render("program.si", "Say 1\nSay 2\n")
            .starts_with("error[E.semantic.type.mismatch] (Semantic error)\n  --> program.si:2:3")
    );
    assert_eq!(error.code().category(), error.category());
}

#[test]
fn renders_command_errors_as_command_errors() {
    let error = SimplyError::Command {
        span: Span::new(0, 0),
        code: DiagnosticCode::CliUsage,
        message: "use `simply --help`".into(),
    };

    assert_eq!(
        error.render("simply", ""),
        "error[E.cli.usage] (Command error)\n  --> simply\n   = What happened: The command or option was not used in a supported way.\n   = Try this: Run `simply --help` to see supported commands and options.\n   = Details: use `simply --help`"
    );
}

#[test]
fn renders_eof_unicode_windows_path_and_missing_context_safely() {
    let error = SimplyError::Parse {
        span: Span::new(3, 1),
        code: DiagnosticCode::UnexpectedToken,
        message: "expected an expression".into(),
    };
    let rendered = error.render(r"C:\projects\program.si", "é\nSay 1\n");
    assert!(rendered.contains(r"C:\projects\program.si:3:1"));
    assert!(rendered.contains("3 |"));
    assert!(rendered.contains("| ^"));

    let missing = SimplyError::Lex {
        span: Span::new(99, 4),
        code: DiagnosticCode::InvalidCharacter,
        message: "invalid character".into(),
    };
    assert!(
        missing
            .render("program.si", "é\n")
            .contains("program.si:99:4")
    );
}

#[test]
fn clamps_source_markers_to_the_available_line() {
    let error = SimplyError::Parse {
        span: Span::with_length(1, usize::MAX, usize::MAX),
        code: DiagnosticCode::UnexpectedToken,
        message: "unexpected token".into(),
    };
    let rendered = error.render("program.si", "é\n");
    assert!(rendered.contains("1 | é"));
    assert!(rendered.contains("  |  ^"));
    assert!(!rendered.contains(&"^".repeat(128)));
}

#[test]
fn aligns_markers_after_tabs() {
    let error = SimplyError::Parse {
        span: Span::new(1, 3),
        code: DiagnosticCode::UnexpectedToken,
        message: "unexpected token".into(),
    };
    let rendered = error.render("program.si", "a\tb\n");
    assert!(rendered.contains("1 | a   b\n  |     ^"));
}

#[test]
fn aligns_markers_using_unicode_terminal_cell_width() {
    let error = SimplyError::Parse {
        span: Span::new(1, 5),
        code: DiagnosticCode::UnexpectedToken,
        message: "unexpected token".into(),
    };
    let rendered = error.render("unicode.si", "a你e\u{301}😀z\n");

    assert!(rendered.contains("1 | a你e\u{301}😀z"));
    assert!(rendered.contains("  |     ^^"));
}

#[test]
fn clips_long_source_around_the_error_to_fit_terminal_width() {
    let source_line = format!("{}target{}", "left ".repeat(10), " right".repeat(10));
    let source = format!("Say {source_line}\n");
    let error = SimplyError::Parse {
        span: Span::new(1, 56),
        code: DiagnosticCode::UnexpectedToken,
        message: "unexpected token".into(),
    };
    let rendered = error.render_with_terminal_width("long.si", &source, Some(24));
    let rendered_source = rendered
        .lines()
        .find(|line| line.starts_with("1 | "))
        .expect("rendered source line should be present");

    assert!(rendered_source.contains("target"));
    assert!(rendered_source.starts_with("1 | ."));
    assert!(rendered_source.ends_with('.'));
    assert!(UnicodeWidthStr::width(rendered_source) <= 24);
    let marker_line = rendered
        .lines()
        .find(|line| line.starts_with("  | "))
        .expect("marker line should be present");
    assert!(UnicodeWidthStr::width(marker_line) <= 24);
}

#[test]
fn keeps_tabbed_source_within_terminal_width_when_clipped() {
    let source_line = format!("{}target{}", "prefix\t".repeat(8), "\t suffix".repeat(8));
    let source = format!("{source_line}\n");
    let column = source_line
        .chars()
        .position(|character| character == 't')
        .expect("target should be present")
        + 1;
    let error = SimplyError::Parse {
        span: Span::new(1, column),
        code: DiagnosticCode::UnexpectedToken,
        message: "unexpected token".into(),
    };

    for terminal_width in 8..40 {
        let rendered = error.render_with_terminal_width("tabs.si", &source, Some(terminal_width));
        for line in rendered.lines().filter(|line| line.starts_with("1 | ")) {
            assert!(
                UnicodeWidthStr::width(line) <= terminal_width,
                "source line exceeds {terminal_width} cells: {line}"
            );
        }
        for line in rendered.lines().filter(|line| line.starts_with("  | ")) {
            assert!(
                UnicodeWidthStr::width(line) <= terminal_width,
                "marker line exceeds {terminal_width} cells: {line}"
            );
        }
    }
}

#[test]
fn leaves_source_unclipped_when_terminal_width_is_unknown() {
    let source_line = "value ".repeat(20);
    let source = format!("Say {source_line}\n");
    let error = SimplyError::Parse {
        span: Span::new(1, 10),
        code: DiagnosticCode::UnexpectedToken,
        message: "unexpected token".into(),
    };
    let rendered = error.render_with_terminal_width("pipe.si", &source, None);

    assert!(rendered.contains(&source_line));
}

#[test]
fn keeps_control_characters_from_breaking_the_diagnostic_layout() {
    let error = SimplyError::Runtime {
        span: Span::new(1, 1),
        code: DiagnosticCode::RuntimeGeneral,
        message: "first line\nsecond line\t\u{1b}[31m".into(),
    };
    let rendered = error.render("bad\n\u{1b}[32m.si", "Say \u{1b}[31mname\n");

    assert!(rendered.contains("  --> bad\\n\\u{1b}[32m.si:1:1"));
    assert!(rendered.contains("1 | Say \\u{1b}[31mname"));
    assert!(rendered.contains("= Details: first line\\nsecond line\\t\\u{1b}[31m"));
    assert_eq!(rendered.lines().count(), 8);
    assert!(!rendered.contains('\u{1b}'));
}

#[test]
fn every_error_category_uses_the_same_diagnostic_sections() {
    let errors = [
        SimplyError::Lex {
            span: Span::new(1, 1),
            code: DiagnosticCode::InvalidCharacter,
            message: "bad character".into(),
        },
        SimplyError::Lex {
            span: Span::new(1, 1),
            code: DiagnosticCode::InvalidNumber,
            message: "bad number".into(),
        },
        SimplyError::Parse {
            span: Span::new(1, 1),
            code: DiagnosticCode::UnexpectedToken,
            message: "unexpected token".into(),
        },
        SimplyError::Semantic {
            span: Span::new(1, 1),
            code: DiagnosticCode::UndefinedVariable,
            message: "unknown name".into(),
        },
        SimplyError::Runtime {
            span: Span::new(1, 1),
            code: DiagnosticCode::RuntimeGeneral,
            message: "operation failed".into(),
        },
        SimplyError::Runtime {
            span: Span::new(1, 1),
            code: DiagnosticCode::RuntimeTypeMismatch,
            message: "wrong runtime type".into(),
        },
        SimplyError::Runtime {
            span: Span::new(1, 1),
            code: DiagnosticCode::RuntimeDeclaration,
            message: "duplicate runtime binding".into(),
        },
        SimplyError::Runtime {
            span: Span::new(1, 1),
            code: DiagnosticCode::RuntimeMutability,
            message: "immutable runtime binding".into(),
        },
        SimplyError::Command {
            span: Span::new(0, 0),
            code: DiagnosticCode::CliUsage,
            message: "invalid command".into(),
        },
    ];

    for error in errors {
        let rendered = error.render("example.si", "source\n");
        assert!(rendered.starts_with("error["));
        assert!(rendered.contains("\n  --> "));
        assert!(rendered.contains("= What happened: "));
        assert!(rendered.contains("= Try this: "));
        assert!(rendered.contains("= Details: "));
        assert_eq!(error.category(), error.code().category());
    }
}
