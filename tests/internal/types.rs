// Internal unit tests for src/types.rs.
use super::Type;

#[test]
fn formats_nested_types_consistently() {
    let typ = Type::List(Box::new(Type::Tuple(vec![Type::Int, Type::String])));
    assert_eq!(typ.name(), "List[Tuple[Int, String]]");
}

#[test]
fn unknown_does_not_override_concrete_type_compatibility() {
    assert!(!Type::Unknown.compatible_with(&Type::Int));
    assert!(!Type::Int.compatible_with(&Type::Unknown));
    assert!(!Type::Int.compatible_with(&Type::String));
    assert!(Type::Unknown.compatible_with(&Type::Unknown));
}

#[test]
fn range_and_csv_stream_types_are_distinct_runtime_types() {
    assert_eq!(Type::Range.name(), "Range");
    assert_eq!(Type::CsvStream.name(), "CsvStream");
    assert!(Type::Range.compatible_with(&Type::Range));
    assert!(Type::CsvStream.compatible_with(&Type::CsvStream));
    assert!(!Type::Range.compatible_with(&Type::CsvStream));
}

#[test]
fn nested_unknown_is_compatible_with_concrete_collection_types() {
    assert!(Type::List(Box::new(Type::Unknown)).compatible_with(&Type::List(Box::new(Type::Int))));
    assert!(
        Type::Tuple(vec![Type::Unknown, Type::String])
            .compatible_with(&Type::Tuple(vec![Type::Int, Type::String]))
    );
}

#[test]
fn unknown_is_not_a_universal_type_compatibility_fallback() {
    assert!(!Type::Unknown.compatible_with(&Type::Int));
    assert!(!Type::Int.compatible_with(&Type::Unknown));
    assert!(Type::Unknown.compatible_with(&Type::Unknown));
}
