//! runtime/operations.rs — runtime operators
//! Implements numeric, boolean, comparison, arithmetic, and matrix operations for evaluated values.
//! Key components: numeric operations, matrix arithmetic, and operator error helpers.
use crate::{
    ast::{BinaryOperator, UnaryOperator},
    error::{DiagnosticCode, SimplyError, Span},
    runtime::value::{Value, shared_values},
};

pub(crate) fn unary(
    value: Value,
    operator: &UnaryOperator,
    span: Option<&Span>,
) -> Result<Value, SimplyError> {
    match operator {
        UnaryOperator::Not => match value {
            Value::Bool(value) => Ok(Value::Bool(!value)),
            _ => Err(error(span, "`not` requires a boolean")),
        },
        UnaryOperator::Negate => match value {
            Value::Int(value) => value
                .checked_neg()
                .map(Value::Int)
                .ok_or_else(|| error(span, "integer overflow")),
            Value::Float(value) => Ok(Value::Float(-value)),
            _ => Err(error(span, "unary `-` requires a number")),
        },
        UnaryOperator::Transpose => match value {
            Value::Matrix(rows) => matrix_transpose(&rows, span),
            _ => Err(error(span, "transpose requires a matrix")),
        },
    }
}

pub(crate) fn binary(
    left: Value,
    operator: &BinaryOperator,
    right: Value,
    span: Option<&Span>,
) -> Result<Value, SimplyError> {
    use BinaryOperator::*;

    match operator {
        Add => match (left, right) {
            (Value::Matrix(left), Value::Matrix(right)) => matrix_add(&left, &right, span),
            (Value::String(left), Value::String(right)) => Ok(Value::String(left + &right)),
            (Value::Int(left), Value::Int(right)) => left
                .checked_add(right)
                .map(Value::Int)
                .ok_or_else(|| arithmetic_error(span)),
            (Value::Float(left), Value::Float(right)) => float_result(left + right, span),
            (Value::Int(left), Value::Float(right)) => float_result(left as f64 + right, span),
            (Value::Float(left), Value::Int(right)) => float_result(left + right as f64, span),
            _ => Err(error(span, "`+` requires two compatible values")),
        },
        Subtract | Multiply | Divide | Remainder => numeric_operation(left, operator, right, span),
        MatrixMultiply => matrix_multiply(left, right, span),
        Greater | GreaterEqual | Less | LessEqual => {
            numeric_comparison(left, operator, right, span)
        }
        Equal => Ok(Value::Bool(left == right)),
        NotEqual => Ok(Value::Bool(left != right)),
        And => match (left, right) {
            (Value::Bool(left), Value::Bool(right)) => Ok(Value::Bool(left && right)),
            _ => Err(error(span, "`and` requires two booleans")),
        },
        Or => match (left, right) {
            (Value::Bool(left), Value::Bool(right)) => Ok(Value::Bool(left || right)),
            _ => Err(error(span, "`or` requires two booleans")),
        },
    }
}

fn numeric_operation(
    left: Value,
    operator: &BinaryOperator,
    right: Value,
    span: Option<&Span>,
) -> Result<Value, SimplyError> {
    if let (Value::Int(left), Value::Int(right)) = (&left, &right) {
        if *right == 0 && matches!(operator, BinaryOperator::Divide | BinaryOperator::Remainder) {
            return Err(division_error(span));
        }
        let result = match operator {
            BinaryOperator::Subtract => left.checked_sub(*right),
            BinaryOperator::Multiply => left.checked_mul(*right),
            BinaryOperator::Divide => left.checked_div(*right),
            BinaryOperator::Remainder => left.checked_rem(*right),
            // The caller restricts this helper to arithmetic operators.
            _ => unreachable!(),
        };
        return result.map(Value::Int).ok_or_else(|| arithmetic_error(span));
    }

    let (left, right) = match (left, right) {
        (Value::Float(left), Value::Float(right)) => (left, right),
        (Value::Int(left), Value::Float(right)) => (left as f64, right),
        (Value::Float(left), Value::Int(right)) => (left, right as f64),
        _ => return Err(error(span, "arithmetic requires two numbers")),
    };
    if right == 0.0 && matches!(operator, BinaryOperator::Divide | BinaryOperator::Remainder) {
        return Err(division_error(span));
    }
    let result = match operator {
        BinaryOperator::Subtract => left - right,
        BinaryOperator::Multiply => left * right,
        BinaryOperator::Divide => left / right,
        BinaryOperator::Remainder => left % right,
        // The caller restricts this helper to arithmetic operators.
        _ => unreachable!(),
    };
    float_result(result, span)
}

fn numeric_comparison(
    left: Value,
    operator: &BinaryOperator,
    right: Value,
    span: Option<&Span>,
) -> Result<Value, SimplyError> {
    if let (Value::Int(left), Value::Int(right)) = (&left, &right) {
        let result = match operator {
            BinaryOperator::Greater => left > right,
            BinaryOperator::GreaterEqual => left >= right,
            BinaryOperator::Less => left < right,
            BinaryOperator::LessEqual => left <= right,
            _ => unreachable!(),
        };
        return Ok(Value::Bool(result));
    }

    let (left, right) = match (left, right) {
        (Value::Float(left), Value::Float(right)) => (left, right),
        (Value::Int(left), Value::Float(right)) => (left as f64, right),
        (Value::Float(left), Value::Int(right)) => (left, right as f64),
        _ => return Err(error(span, "comparison requires two numbers")),
    };
    let result = match operator {
        BinaryOperator::Greater => left > right,
        BinaryOperator::GreaterEqual => left >= right,
        BinaryOperator::Less => left < right,
        BinaryOperator::LessEqual => left <= right,
        // The caller restricts this helper to comparison operators.
        _ => unreachable!(),
    };
    Ok(Value::Bool(result))
}

fn float_result(value: f64, span: Option<&Span>) -> Result<Value, SimplyError> {
    if value.is_finite() {
        Ok(Value::Float(value))
    } else {
        Err(SimplyError::Runtime {
            span: span.cloned().unwrap_or_else(|| Span::new(0, 0)),
            code: DiagnosticCode::RuntimeArithmetic,
            message: "floating-point result is not finite".into(),
        })
    }
}

fn matrix_add(left: &[Value], right: &[Value], span: Option<&Span>) -> Result<Value, SimplyError> {
    let left_width = matrix_shape(left, span)?;
    let right_width = matrix_shape(right, span)?;
    if left.len() != right.len() || left_width != right_width {
        return Err(error(span, "matrix dimensions do not match"));
    }
    let mut rows = Vec::new();
    for (left_row, right_row) in left.iter().zip(right) {
        match (left_row, right_row) {
            (Value::Array(left), Value::Array(right)) if left.len() == right.len() => {
                let mut row = Vec::new();
                for (left, right) in left.iter().zip(right.iter()) {
                    row.push(binary(
                        left.clone(),
                        &BinaryOperator::Add,
                        right.clone(),
                        span,
                    )?);
                }
                rows.push(Value::Array(shared_values(row)));
            }
            _ => return Err(error(span, "invalid matrix rows")),
        }
    }
    Ok(Value::Matrix(shared_values(rows)))
}

fn matrix_multiply(left: Value, right: Value, span: Option<&Span>) -> Result<Value, SimplyError> {
    let (left, right) = match (left, right) {
        (Value::Matrix(left), Value::Matrix(right)) => (left, right),
        _ => return Err(error(span, "matrix multiply requires matrices")),
    };
    let left_width = matrix_shape(&left, span)?;
    let right_width = matrix_shape(&right, span)?;
    if left.is_empty() || right.is_empty() || left_width != right.len() {
        return Err(error(span, "matrix dimensions do not match"));
    }
    let mut result = Vec::with_capacity(left.len());
    for left_row in left.iter() {
        let Value::Array(left_row) = left_row else {
            return Err(error(span, "matrix rows must be arrays"));
        };
        let mut output = Vec::with_capacity(right_width);
        for column in 0..right_width {
            let mut total = 0.0;
            for (index, value) in left_row.iter().enumerate() {
                let Value::Array(right_row) = &right[index] else {
                    return Err(error(span, "matrix rows must be arrays"));
                };
                total += numeric_value(value, span)? * numeric_value(&right_row[column], span)?;
            }
            if !total.is_finite() {
                return Err(arithmetic_error(span));
            }
            output.push(Value::Float(total));
        }
        result.push(Value::Array(shared_values(output)));
    }
    Ok(Value::Matrix(shared_values(result)))
}

fn matrix_shape(matrix: &[Value], span: Option<&Span>) -> Result<usize, SimplyError> {
    let width = match matrix.first() {
        Some(Value::Array(values)) => values.len(),
        Some(_) => return Err(error(span, "matrix rows must be arrays")),
        None => return Ok(0),
    };
    for row in matrix {
        let values = match row {
            Value::Array(values) if values.len() == width => values,
            Value::Array(_) => return Err(error(span, "matrix rows must have equal widths")),
            _ => return Err(error(span, "matrix rows must be arrays")),
        };
        if values
            .iter()
            .any(|value| !matches!(value, Value::Int(_) | Value::Float(_)))
        {
            return Err(error(span, "matrix values must be numeric"));
        }
    }
    Ok(width)
}

fn matrix_transpose(rows: &[Value], span: Option<&Span>) -> Result<Value, SimplyError> {
    let width = matrix_shape(rows, span)?;
    if rows.is_empty() {
        return Ok(Value::Matrix(shared_values(Vec::new())));
    }
    let mut result = Vec::with_capacity(width);
    for column in 0..width {
        let mut output = Vec::with_capacity(rows.len());
        for row in rows {
            let Value::Array(values) = row else {
                return Err(error(span, "matrix rows must be arrays"));
            };
            output.push(Value::Float(numeric_value(&values[column], span)?));
        }
        result.push(Value::Array(shared_values(output)));
    }
    Ok(Value::Matrix(shared_values(result)))
}

fn numeric_value(value: &Value, span: Option<&Span>) -> Result<f64, SimplyError> {
    match value {
        Value::Int(value) => Ok(*value as f64),
        Value::Float(value) => Ok(*value),
        _ => Err(error(span, "matrix values must be numeric")),
    }
}

fn error(span: Option<&Span>, message: impl Into<String>) -> SimplyError {
    SimplyError::Runtime {
        span: span.cloned().unwrap_or_else(|| Span::new(0, 0)),
        code: DiagnosticCode::RuntimeGeneral,
        message: message.into(),
    }
}

fn arithmetic_error(span: Option<&Span>) -> SimplyError {
    SimplyError::Runtime {
        span: span.cloned().unwrap_or_else(|| Span::new(0, 0)),
        code: DiagnosticCode::RuntimeArithmetic,
        message: "integer arithmetic error".into(),
    }
}

fn division_error(span: Option<&Span>) -> SimplyError {
    SimplyError::Runtime {
        span: span.cloned().unwrap_or_else(|| Span::new(0, 0)),
        code: DiagnosticCode::RuntimeDivision,
        message: "division by zero".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::{binary, numeric_comparison};
    use crate::{ast::BinaryOperator, runtime::value::Value};

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
}
