// Internal unit tests for src/evaluator.rs.
use super::*;
use crate::runtime::value::shared_values;
use crate::semantic::SemanticAnalyzer;
use crate::types::DeclarationKind;

pub(super) static PARALLEL_THREAD_IDS: std::sync::LazyLock<
    std::sync::Mutex<std::collections::HashSet<std::thread::ThreadId>>,
> = std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashSet::new()));

impl Evaluator {
    fn evaluate_streaming_pipeline<I>(
        &mut self,
        values: I,
        steps: &[PipelineStep],
    ) -> Result<Value, SimplyError>
    where
        I: IntoIterator<Item = Value>,
    {
        self.evaluate_streaming_pipeline_with_sum_type(values, steps, crate::types::Type::Unknown)
    }
}

#[test]
fn in_memory_declarations_have_deterministic_module_identity() {
    let program = Parser::new(
        Lexer::new(
            "type Item:\n    value as Int\nend\n\
                 enum State:\n    Ready\nend\n\
                 item is Item(1)\nstate is State::Ready\n",
        )
        .tokenize()
        .expect("valid source should tokenize"),
    )
    .parse()
    .expect("valid source should parse");
    crate::semantic::test_support::analyze(&mut SemanticAnalyzer::new(), &program)
        .expect("in-memory declarations should type check");

    for _ in 0..2 {
        let mut evaluator = Evaluator::new();
        evaluator
            .run(&program)
            .expect("in-memory declarations should execute");
        let Value::Struct(item) = evaluator.lookup("item").expect("item should be bound") else {
            panic!("item should be a struct value");
        };
        let Value::Enum(state) = evaluator.lookup("state").expect("state should be bound") else {
            panic!("state should be an enum value");
        };
        assert_eq!(
            item.with(|item| item.identity.clone())
                .expect("struct handle should remain valid"),
            DeclarationIdentity::new("memory://root", "Item", DeclarationKind::Struct)
        );
        assert_eq!(
            state
                .with(|state| state.identity.clone())
                .expect("enum handle should remain valid"),
            DeclarationIdentity::new("memory://root", "State", DeclarationKind::Enum)
        );
    }
}

#[test]
fn runtime_checks_enum_payload_types_without_semantic_analysis() {
    let program = Parser::new(
        Lexer::new("enum Result:\n    Ok as Int\nend\nvalue is Result::Ok(\"bad\")\n")
            .tokenize()
            .expect("valid enum source should tokenize"),
    )
    .parse()
    .expect("valid enum syntax should parse");
    let error = Evaluator::new()
        .run(&program)
        .expect_err("runtime must reject a mismatched enum payload");
    assert!(error.to_string().contains("expected Int"), "{error}");
    assert!(error.to_string().contains("found String"), "{error}");
}

#[test]
fn runtime_rejects_enum_message_dispatch_even_if_a_global_function_exists() {
    let program = Parser::new(
        Lexer::new(
            "enum State:\n    Ready\nend\n\
                 fn greet(value):\n    return \"hello\"\nend\n\
                 state is State::Ready\nstate :: greet\n",
        )
        .tokenize()
        .expect("valid enum source should tokenize"),
    )
    .parse()
    .expect("valid enum syntax should parse");
    let error = Evaluator::new()
        .run(&program)
        .expect_err("enum values must not receive struct/global messages");
    assert!(
        error
            .to_string()
            .contains("does not support message dispatch"),
        "{error}"
    );
}

#[test]
fn recursive_tuple_matching_checks_arity_and_keeps_failed_bindings_local() {
    let source = "pair is (1, 2, 3)\n\
                     result is match pair:\n\
                         (first, second):\n    99\n\
                         _:\n    42\n\
                     end\n";
    let program = Parser::new(
        Lexer::new(source)
            .tokenize()
            .expect("valid tuple source should tokenize"),
    )
    .parse()
    .expect("valid tuple source should parse");
    let mut evaluator = Evaluator::new();
    evaluator
        .run(&program)
        .expect("wildcard should match after arity mismatch");
    assert_eq!(evaluator.lookup("result"), Some(&Value::Int(42)));

    let source = "pair is (1, 2)\n\
                     match pair:\n\
                         (first, (second, third)):\n    first\n\
                         _:\n    first\n\
                     end\n";
    let program = Parser::new(
        Lexer::new(source)
            .tokenize()
            .expect("valid nested tuple source should tokenize"),
    )
    .parse()
    .expect("valid nested tuple source should parse");
    let error = Evaluator::new()
        .run(&program)
        .expect_err("failed tuple pattern must not leak partial bindings");
    assert!(
        error.to_string().contains("unknown variable `first`"),
        "{error}"
    );
}

#[test]
fn runtime_struct_matching_checks_nominal_identity_and_field_arity() {
    let source = "type Person:\n    name as String\n    age as Int\nend\n\
                     person is Person(\"Ada\", 37)\n\
                     result is match person:\n\
                         Person(name):\n    \"incorrect arity\"\n\
                         _:\n    \"fallback\"\n\
                     end\n";
    let program = Parser::new(
        Lexer::new(source)
            .tokenize()
            .expect("valid Struct pattern source should tokenize"),
    )
    .parse()
    .expect("valid Struct pattern source should parse");
    let mut evaluator = Evaluator::new();
    evaluator
        .run(&program)
        .expect("runtime matcher should fall through on incorrect field arity");
    assert_eq!(
        evaluator.lookup("result"),
        Some(&Value::String("fallback".into()))
    );

    let source = "type Person:\n    name as String\nend\n\
                     type User:\n    name as String\nend\n\
                     user is User(\"Ada\")\n\
                     result is match user:\n\
                         Person(name):\n    \"incorrect nominal type\"\n\
                         _:\n    \"fallback\"\n\
                     end\n";
    let program = Parser::new(
        Lexer::new(source)
            .tokenize()
            .expect("valid nominal pattern source should tokenize"),
    )
    .parse()
    .expect("valid nominal pattern source should parse");
    let mut evaluator = Evaluator::new();
    evaluator
        .run(&program)
        .expect("runtime matcher should compare nominal Struct identities");
    assert_eq!(
        evaluator.lookup("result"),
        Some(&Value::String("fallback".into()))
    );
}

#[test]
fn runtime_rejects_non_boolean_match_guards_without_semantic_analysis() {
    let source = "result is match 1:\n    _ if 1:\n        \"invalid\"\nend\n";
    let program = Parser::new(
        Lexer::new(source)
            .tokenize()
            .expect("guard source should tokenize"),
    )
    .parse()
    .expect("guard source should parse");
    let error = Evaluator::new()
        .run(&program)
        .expect_err("runtime must reject a non-Bool guard");
    assert!(
        error
            .to_string()
            .contains("match guard must evaluate to Bool"),
        "{error}"
    );
}

#[test]
fn parallel_workers_execute_scalar_chunks_and_preserve_order() {
    PARALLEL_THREAD_IDS
        .lock()
        .expect("parallel test lock")
        .clear();
    let values = (0..16).map(Value::Int).collect();
    let transforms = [PipelineStep::Derive(Expr::Binary {
        left: Box::new(Expr::Identifier("item".into())),
        operator: BinaryOperator::Multiply,
        right: Box::new(Expr::Literal(Literal::Int(2))),
    })];
    let output =
        parallel::evaluate_parallel(values, &transforms, 4, Some(8)).expect("parallel evaluation");
    assert_eq!(
        output,
        (0..16)
            .map(|value| Value::Int(value * 2))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        PARALLEL_THREAD_IDS
            .lock()
            .expect("parallel test lock")
            .len(),
        2
    );
}

#[test]
fn parallel_execution_rejects_excessive_workers_and_chunk_counts() {
    assert_eq!(
        parallel::evaluate_parallel(Vec::new(), &[], 1, None).expect("empty input"),
        Vec::<Value>::new()
    );
    assert!(
        parallel::evaluate_parallel(
            vec![Value::Int(1)],
            &[],
            limits::MAX_PARALLEL_WORKERS + 1,
            None
        )
        .expect_err("worker limit should be enforced")
        .message
        .contains("worker count")
    );
    assert!(
        parallel::evaluate_parallel(
            vec![Value::Int(1)],
            &[],
            1,
            Some(limits::MAX_CHUNK_SIZE + 1)
        )
        .expect_err("chunk size limit should be enforced")
        .message
        .contains("chunk size")
    );
    let input = (0..=limits::MAX_PARALLEL_CHUNKS)
        .map(|value| Value::Int(value as i64))
        .collect();
    assert!(
        parallel::evaluate_parallel(input, &[], 1, Some(1))
            .expect_err("chunk count limit should be enforced")
            .message
            .contains("chunk count")
    );
}

#[test]
fn function_call_depth_is_restored_after_recursion_failure() {
    std::thread::Builder::new()
            .name("recursion-depth-test".into())
            .stack_size(8 * 1024 * 1024)
            .spawn(|| {
                let source = "fn recurse(value as Int) gives Int:\n    return recurse(value + 1)\nend\nrecurse(0)\n";
                let program =
                    Parser::new(Lexer::new(source).tokenize().expect("source should lex"))
                        .parse()
                        .expect("source should parse");
                let mut evaluator = Evaluator::new();
                let error = evaluator
                    .run(&program)
                    .expect_err("excessive recursion should fail with a diagnostic");
                assert!(
                    error
                        .to_string()
                        .contains("function call depth exceeds the limit")
                );
                assert_eq!(evaluator.call_depth, 0);

                let recovery = Parser::new(
                    Lexer::new(
                        "fn identity(value as Int) gives Int:\n    return value\nend\nidentity(7)\n",
                    )
                    .tokenize()
                    .expect("recovery source should lex"),
                )
                .parse()
                .expect("recovery source should parse");
                evaluator
                    .run(&recovery)
                    .expect("successful calls should work after recursion failure");
                assert_eq!(evaluator.call_depth, 0);
            })
            .expect("recursion test thread should spawn")
            .join()
            .expect("recursion test thread should complete");
}

#[test]
fn parallel_safe_subset_rejects_calls_and_collections() {
    assert!(!is_parallel_safe_expression(&Expr::Call {
        name: "abs".into(),
        arguments: vec![Expr::Identifier("item".into())],
    }));
    assert!(
        parallel::evaluate_parallel(
            vec![Value::List(shared_values(vec![Value::Int(1)]))],
            &[],
            2,
            None,
        )
        .is_err()
    );
}

#[test]
fn parallel_pipeline_rejects_unsafe_transforms_at_runtime() {
    let steps = [
        PipelineStep::Parallel(2),
        PipelineStep::Derive(Expr::Call {
            name: "abs".into(),
            arguments: vec![Expr::Identifier("item".into())],
        }),
        PipelineStep::Sum,
    ];
    let error = Evaluator::new()
        .evaluate_streaming_pipeline(vec![Value::Int(1)], &steps)
        .expect_err("unsafe parallel transforms must not fall back to sequential execution");
    assert!(error.to_string().contains("parallel-safe"), "{error}");
}

#[test]
fn parallel_pipeline_rejects_non_scalar_items_at_runtime() {
    let steps = [PipelineStep::Parallel(2), PipelineStep::Count];
    let error = Evaluator::new()
        .evaluate_streaming_pipeline(
            vec![Value::List(shared_values(vec![Value::Int(1)]))],
            &steps,
        )
        .expect_err("non-scalar items must not fall back to sequential execution");
    assert!(error.to_string().contains("scalar source items"), "{error}");
}

#[test]
fn csv_rest_suffix_records_the_byte_cursor_after_its_prefix() {
    static NEXT_ID: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let id = NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let path =
        std::env::temp_dir().join(format!("simply-csv-cursor-{}-{id}.csv", std::process::id()));
    let contents = "first\n\"second\nline\"\nthird\n";
    fs::write(&path, contents).expect("failed to write CSV cursor fixture");
    let path_string = path.to_str().expect("CSV cursor path must be UTF-8");
    let suffix_offset = "first\n".len() as u64;

    let (_, suffix) = Evaluator::new()
        .read_csv_sequence_prefix(path_string, 0, 0, None, 1, true)
        .expect("prefix read should succeed")
        .expect("first record should exist");
    let Some(Value::CsvStream {
        start_record,
        start_offset,
        source_version,
        ..
    }) = suffix
    else {
        panic!("rest binding should preserve a lazy CSV stream");
    };
    assert_eq!(start_record, 1);
    assert_eq!(start_offset, suffix_offset);

    let (rows, next_suffix) = Evaluator::new()
        .read_csv_sequence_prefix(
            path_string,
            start_record,
            start_offset,
            source_version.as_ref(),
            1,
            true,
        )
        .expect("suffix read should succeed")
        .expect("second record should exist");
    assert_eq!(
        rows,
        vec![Value::List(shared_values(vec![Value::String(
            "second\nline".into(),
        )]))]
    );
    let Some(Value::CsvStream {
        start_record,
        start_offset,
        source_version,
        ..
    }) = next_suffix
    else {
        panic!("nested rest binding should preserve a lazy CSV stream");
    };
    assert_eq!(start_record, 2);
    assert_eq!(start_offset, "first\n\"second\nline\"\n".len() as u64);

    fs::write(&path, "new\nreplacement\nlast\n").expect("failed to replace CSV cursor fixture");
    let (rows, _) = Evaluator::new()
        .read_csv_sequence_prefix(
            path_string,
            1,
            suffix_offset,
            source_version.as_ref(),
            1,
            true,
        )
        .expect("modified CSV source should use record-position fallback")
        .expect("replacement record should exist");
    assert_eq!(
        rows,
        vec![Value::List(shared_values(vec![Value::String(
            "replacement".into(),
        )]))]
    );

    fs::remove_file(path).expect("failed to remove CSV cursor fixture");
}
