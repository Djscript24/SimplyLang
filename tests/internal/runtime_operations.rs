// Internal unit tests for src/runtime/operations.rs.
use super::{binary, ensure_matrix_allocation, numeric_comparison};
use crate::{ast::BinaryOperator, error::DiagnosticCode, runtime::value::Value};

#[test]
fn matrix_result_allocations_obey_the_cell_limit() {
    assert!(ensure_matrix_allocation(1_000, 1_000, None, "too large").is_ok());

    let too_many_cells = ensure_matrix_allocation(1_001, 1_000, None, "too large").unwrap_err();
    assert_eq!(too_many_cells.code(), DiagnosticCode::RuntimeLimit);

    let overflowing_dimensions =
        ensure_matrix_allocation(usize::MAX, 2, None, "too large").unwrap_err();
    assert_eq!(overflowing_dimensions.code(), DiagnosticCode::RuntimeLimit);
}

#[test]
fn compares_large_integers_without_float_precision_loss() {
    let result = numeric_comparison(
        Value::Int(9_007_199_254_740_993),
        &BinaryOperator::Greater,
        Value::Int(9_007_199_254_740_992),
        None,
    )
    .expect("integer comparison should succeed");

    assert_eq!(result, Value::Bool(true));
}

#[test]
fn rejects_non_finite_float_results() {
    let result = binary(
        Value::Float(1.0e308),
        &BinaryOperator::Multiply,
        Value::Float(1.0e308),
        None,
    );

    assert!(result.is_err());
}

#[test]
fn rejects_non_finite_matrix_results() {
    let matrix = Value::Matrix(crate::runtime::value::shared_values(vec![Value::Array(
        crate::runtime::value::shared_values(vec![Value::Float(1.0e308)]),
    )]));

    let result = binary(
        matrix.clone(),
        &BinaryOperator::MatrixMultiply,
        matrix,
        None,
    );

    assert!(result.is_err());
}
