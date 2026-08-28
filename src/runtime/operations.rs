use crate::{
    ast::{BinaryOperator, UnaryOperator},
    error::{DiagnosticCode, SimplyError, Span},
    runtime::value::Value,
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
            Value::Matrix(rows) => matrix_transpose(rows, span),
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
            (Value::Matrix(left), Value::Matrix(right)) => matrix_add(left, right, span),
            (Value::String(left), Value::String(right)) => Ok(Value::String(left + &right)),
            (Value::Int(left), Value::Int(right)) => left
                .checked_add(right)
                .map(Value::Int)
                .ok_or_else(|| arithmetic_error(span)),
            (Value::Float(left), Value::Float(right)) => Ok(Value::Float(left + right)),
            (Value::Int(left), Value::Float(right)) => Ok(Value::Float(left as f64 + right)),
            (Value::Float(left), Value::Int(right)) => Ok(Value::Float(left + right as f64)),
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
    Ok(Value::Float(result))
}

fn numeric_comparison(
    left: Value,
    operator: &BinaryOperator,
    right: Value,
    span: Option<&Span>,
) -> Result<Value, SimplyError> {
    let (left, right) = match (left, right) {
        (Value::Int(left), Value::Int(right)) => (left as f64, right as f64),
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

fn matrix_add(
    left: Vec<Value>,
    right: Vec<Value>,
    span: Option<&Span>,
) -> Result<Value, SimplyError> {
    matrix_numbers(&left, span)?;
    matrix_numbers(&right, span)?;
    if left.len() != right.len() {
        return Err(error(span, "matrix dimensions do not match"));
    }
    let mut rows = Vec::new();
    for (left_row, right_row) in left.into_iter().zip(right) {
        match (left_row, right_row) {
            (Value::Array(left), Value::Array(right)) if left.len() == right.len() => {
                let mut row = Vec::new();
                for (left, right) in left.into_iter().zip(right) {
                    row.push(binary(left, &BinaryOperator::Add, right, span)?);
                }
                rows.push(Value::Array(row));
            }
            _ => return Err(error(span, "invalid matrix rows")),
        }
    }
    Ok(Value::Matrix(rows))
}

fn matrix_multiply(left: Value, right: Value, span: Option<&Span>) -> Result<Value, SimplyError> {
    let (left, right) = match (left, right) {
        (Value::Matrix(left), Value::Matrix(right)) => (left, right),
        _ => return Err(error(span, "matrix multiply requires matrices")),
    };
    let left_rows = matrix_numbers(&left, span)?;
    let right_rows = matrix_numbers(&right, span)?;
    if left_rows.is_empty()
        || right_rows.is_empty()
        || left_rows.iter().any(|row| row.len() != left_rows[0].len())
        || right_rows
            .iter()
            .any(|row| row.len() != right_rows[0].len())
        || left_rows[0].len() != right_rows.len()
    {
        return Err(error(span, "matrix dimensions do not match"));
    }
    let mut result = Vec::new();
    for row in &left_rows {
        let mut output = Vec::new();
        for (column, _) in right_rows[0].iter().enumerate() {
            output.push(
                row.iter()
                    .enumerate()
                    .map(|(index, value)| value * right_rows[index][column])
                    .sum::<f64>(),
            );
        }
        result.push(Value::Array(output.into_iter().map(Value::Float).collect()));
    }
    Ok(Value::Matrix(result))
}

fn matrix_numbers(matrix: &[Value], span: Option<&Span>) -> Result<Vec<Vec<f64>>, SimplyError> {
    let numbers = matrix
        .iter()
        .map(|row| match row {
            Value::Array(values) => values
                .iter()
                .map(|value| match value {
                    Value::Int(value) => Ok(*value as f64),
                    Value::Float(value) => Ok(*value),
                    _ => Err(error(span, "matrix values must be numeric")),
                })
                .collect(),
            _ => Err(error(span, "matrix rows must be arrays")),
        })
        .collect::<Result<Vec<Vec<f64>>, _>>()?;
    if let Some(first_width) = numbers.first().map(Vec::len)
        && numbers.iter().any(|row| row.len() != first_width)
    {
        return Err(error(span, "matrix rows must have equal widths"));
    }
    Ok(numbers)
}

fn matrix_transpose(rows: Vec<Value>, span: Option<&Span>) -> Result<Value, SimplyError> {
    let numbers = matrix_numbers(&rows, span)?;
    if numbers.is_empty() {
        return Ok(Value::Matrix(Vec::new()));
    }
    let width = numbers[0].len();
    let result = (0..width)
        .map(|column| {
            Value::Array(
                numbers
                    .iter()
                    .map(|row| Value::Float(row[column]))
                    .collect(),
            )
        })
        .collect();
    Ok(Value::Matrix(result))
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
