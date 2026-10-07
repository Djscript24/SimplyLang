//! runtime/operations.rs — runtime operators
//! Implements numeric, boolean, comparison, arithmetic, and matrix operations for evaluated values.
//! Key components: numeric operations, matrix arithmetic, and operator error helpers.
use crate::{
    ast::{BinaryOperator, UnaryOperator},
    error::{DiagnosticCode, SimplyError, Span},
    runtime::{
        heap::RuntimeHeap,
        value::{Value, shared_values},
    },
};

pub(crate) fn unary(
    heap: &RuntimeHeap,
    value: Value,
    operator: &UnaryOperator,
    span: Option<&Span>,
) -> Result<Value, SimplyError> {
    match operator {
        UnaryOperator::Not => match value {
            Value::Bool(value) => Ok(Value::Bool(!value)),
            _ => Err(type_error(span, "`not` requires a boolean")),
        },
        UnaryOperator::Negate => match value {
            Value::Int(value) => value
                .checked_neg()
                .map(Value::Int)
                .ok_or_else(|| arithmetic_error(span)),
            Value::Float(value) => Ok(Value::Float(-value)),
            _ => Err(type_error(span, "unary `-` requires a number")),
        },
        UnaryOperator::Transpose => match value {
            Value::Matrix(rows) => matrix_transpose(heap, &rows, span),
            _ => Err(type_error(span, "transpose requires a matrix")),
        },
    }
}

pub(crate) fn binary(
    heap: &RuntimeHeap,
    left: Value,
    operator: &BinaryOperator,
    right: Value,
    span: Option<&Span>,
) -> Result<Value, SimplyError> {
    use BinaryOperator::*;

    match operator {
        Add => match (left, right) {
            (
                left @ (Value::Matrix(_) | Value::Array(_) | Value::List(_)),
                right @ (Value::Matrix(_) | Value::Array(_) | Value::List(_)),
            ) => matrix_add_values(heap, &left, &right, false, span),
            (Value::String(left), Value::String(right)) => Ok(Value::String(left + &right)),
            (Value::Int(left), Value::Int(right)) => left
                .checked_add(right)
                .map(Value::Int)
                .ok_or_else(|| arithmetic_error(span)),
            (Value::Float(left), Value::Float(right)) => float_result(left + right, span),
            (Value::Int(left), Value::Float(right)) => float_result(left as f64 + right, span),
            (Value::Float(left), Value::Int(right)) => float_result(left + right as f64, span),
            _ => Err(type_error(span, "`+` requires two compatible values")),
        },
        Subtract | Multiply | Divide | Remainder => numeric_operation(left, operator, right, span),
        MatrixMultiply => matrix_multiply_values(heap, &left, &right, span),
        Greater | GreaterEqual | Less | LessEqual => {
            numeric_comparison(left, operator, right, span)
        }
        Equal => Ok(Value::Bool(left == right)),
        NotEqual => Ok(Value::Bool(left != right)),
        And => match (left, right) {
            (Value::Bool(left), Value::Bool(right)) => Ok(Value::Bool(left && right)),
            _ => Err(type_error(span, "`and` requires two booleans")),
        },
        Or => match (left, right) {
            (Value::Bool(left), Value::Bool(right)) => Ok(Value::Bool(left || right)),
            _ => Err(type_error(span, "`or` requires two booleans")),
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
        _ => return Err(type_error(span, "arithmetic requires two numbers")),
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
        _ => return Err(type_error(span, "comparison requires two numbers")),
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

pub(crate) fn math_unary(
    name: &str,
    value: Value,
    span: Option<&Span>,
) -> Result<Value, SimplyError> {
    let value = match value {
        Value::Int(value) => value as f64,
        Value::Float(value) if value.is_finite() => value,
        _ => {
            return Err(type_error(
                span,
                format!("`{name}` requires a finite number"),
            ));
        }
    };
    let result = match name {
        "sqrt" if value >= 0.0 => value.sqrt(),
        "sqrt" => {
            return Err(arithmetic_error_with_message(
                span,
                "`sqrt` requires a non-negative number",
            ));
        }
        "exp" => value.exp(),
        "log" if value > 0.0 => value.ln(),
        "log" => {
            return Err(arithmetic_error_with_message(
                span,
                "`log` requires a positive number",
            ));
        }
        "log10" if value > 0.0 => value.log10(),
        "log10" => {
            return Err(arithmetic_error_with_message(
                span,
                "`log10` requires a positive number",
            ));
        }
        "sin" => value.sin(),
        "cos" => value.cos(),
        "tan" => value.tan(),
        "floor" => value.floor(),
        "ceil" => value.ceil(),
        _ => {
            return Err(argument_error(
                span,
                format!("unknown math operation `{name}`"),
            ));
        }
    };
    float_result(result, span)
}

pub(crate) fn math_pow(
    base: Value,
    exponent: Value,
    span: Option<&Span>,
) -> Result<Value, SimplyError> {
    let base = numeric_value(&base, span)?;
    let exponent = numeric_value(&exponent, span)?;
    float_result(base.powf(exponent), span)
}

pub(crate) fn math_sign(value: Value, span: Option<&Span>) -> Result<Value, SimplyError> {
    match value {
        Value::Int(value) => Ok(Value::Int(value.signum())),
        Value::Float(value) if value.is_finite() => Ok(Value::Int(if value > 0.0 {
            1
        } else if value < 0.0 {
            -1
        } else {
            0
        })),
        _ => Err(type_error(span, "`sign` requires a finite number")),
    }
}

pub(crate) fn vector_add(
    heap: &RuntimeHeap,
    left: &Value,
    right: &Value,
    subtract: bool,
    span: Option<&Span>,
) -> Result<Value, SimplyError> {
    let left = vector_values(left, span)?;
    let right = vector_values(right, span)?;
    if left.len() != right.len() {
        return Err(collection_error(span, "vector dimensions do not match"));
    }
    let operator = if subtract {
        BinaryOperator::Subtract
    } else {
        BinaryOperator::Add
    };
    let values = left
        .iter()
        .zip(right)
        .map(|(left, right)| binary(heap, (*left).clone(), &operator, right.clone(), span))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Value::Array(heap.insert_sequence(values)))
}

pub(crate) fn vector_scale(
    heap: &RuntimeHeap,
    vector: &Value,
    scalar: &Value,
    span: Option<&Span>,
) -> Result<Value, SimplyError> {
    numeric_value(scalar, span)?;
    let values = vector_values(vector, span)?
        .iter()
        .map(|value| {
            binary(
                heap,
                (*value).clone(),
                &BinaryOperator::Multiply,
                scalar.clone(),
                span,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Value::Array(heap.insert_sequence(values)))
}

pub(crate) fn vector_dot(
    heap: &RuntimeHeap,
    left: &Value,
    right: &Value,
    span: Option<&Span>,
) -> Result<Value, SimplyError> {
    let left = vector_values(left, span)?;
    let right = vector_values(right, span)?;
    if left.len() != right.len() {
        return Err(collection_error(span, "vector dimensions do not match"));
    }
    let mut total = Value::Int(0);
    for (left, right) in left.iter().zip(right) {
        let product = binary(
            heap,
            left.clone(),
            &BinaryOperator::Multiply,
            right.clone(),
            span,
        )?;
        total = binary(heap, total, &BinaryOperator::Add, product, span)?;
    }
    Ok(total)
}

pub(crate) fn vector_cross(
    heap: &RuntimeHeap,
    left: &Value,
    right: &Value,
    span: Option<&Span>,
) -> Result<Value, SimplyError> {
    let left = vector_values(left, span)?;
    let right = vector_values(right, span)?;
    if left.len() != 3 || right.len() != 3 {
        return Err(collection_error(
            span,
            "cross requires two vectors with exactly three elements",
        ));
    }
    let left = left
        .iter()
        .map(|value| numeric_value(value, span))
        .collect::<Result<Vec<_>, _>>()?;
    let right = right
        .iter()
        .map(|value| numeric_value(value, span))
        .collect::<Result<Vec<_>, _>>()?;
    let values = [
        left[1] * right[2] - left[2] * right[1],
        left[2] * right[0] - left[0] * right[2],
        left[0] * right[1] - left[1] * right[0],
    ];
    values
        .into_iter()
        .map(|value| float_result(value, span))
        .collect::<Result<Vec<_>, _>>()
        .map(|values| Value::Array(heap.insert_sequence(values)))
}

pub(crate) fn vector_norm(vector: &Value, span: Option<&Span>) -> Result<Value, SimplyError> {
    let mut norm = 0.0_f64;
    for value in vector_values(vector, span)? {
        norm = norm.hypot(numeric_value(&value, span)?);
    }
    float_result(norm, span)
}

pub(crate) fn vector_distance(
    left: &Value,
    right: &Value,
    span: Option<&Span>,
) -> Result<Value, SimplyError> {
    let left = vector_values(left, span)?;
    let right = vector_values(right, span)?;
    if left.len() != right.len() {
        return Err(collection_error(span, "vector dimensions do not match"));
    }
    let mut distance = 0.0_f64;
    for (left, right) in left.iter().zip(right) {
        let difference = numeric_value(left, span)? - numeric_value(&right, span)?;
        distance = distance.hypot(difference);
    }
    float_result(distance, span)
}

pub(crate) fn vector_normalize(
    heap: &RuntimeHeap,
    vector: &Value,
    span: Option<&Span>,
) -> Result<Value, SimplyError> {
    let values = vector_values(vector, span)?;
    let mut norm = 0.0_f64;
    for value in &values {
        norm = norm.hypot(numeric_value(value, span)?);
    }
    if norm == 0.0 {
        return Err(arithmetic_error_with_message(
            span,
            "cannot normalize a zero vector",
        ));
    }
    let result = values
        .iter()
        .map(|value| float_result(numeric_value(value, span)? / norm, span))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Value::Array(heap.insert_sequence(result)))
}

pub(crate) fn matrix_shape_value(
    matrix: &Value,
    span: Option<&Span>,
) -> Result<Value, SimplyError> {
    let rows = matrix_rows(matrix, span)?;
    Ok(Value::Tuple(shared_values(vec![
        Value::Int(rows.len() as i64),
        Value::Int(rows[0].len() as i64),
    ])))
}

pub(crate) fn matrix_trace(matrix: &Value, span: Option<&Span>) -> Result<Value, SimplyError> {
    let rows = matrix_rows(matrix, span)?;
    if rows.len() != rows[0].len() {
        return Err(collection_error(span, "trace requires a square matrix"));
    }
    let mut trace = 0.0;
    for (index, row) in rows.iter().enumerate() {
        trace += numeric_value(&row[index], span)?;
        if !trace.is_finite() {
            return Err(arithmetic_error(span));
        }
    }
    float_result(trace, span)
}

pub(crate) fn matrix_rank(matrix: &Value, span: Option<&Span>) -> Result<Value, SimplyError> {
    let rows = matrix_rows(matrix, span)?;
    let row_count = rows.len();
    let column_count = rows[0].len();
    ensure_matrix_allocation(
        row_count,
        column_count,
        span,
        "matrix rank workspace is too large",
    )?;
    let mut values = rows
        .iter()
        .map(|row| row.iter().map(|value| numeric_value(value, span)).collect())
        .collect::<Result<Vec<Vec<_>>, _>>()?;
    let scale = values
        .iter()
        .flatten()
        .map(|value| value.abs())
        .fold(0.0_f64, f64::max);
    let tolerance = scale * (row_count.max(column_count) as f64 * f64::EPSILON);
    let mut pivot_row = 0;

    for column in 0..column_count {
        if pivot_row == row_count {
            break;
        }
        let selected_row = (pivot_row..row_count)
            .max_by(|left, right| {
                values[*left][column]
                    .abs()
                    .total_cmp(&values[*right][column].abs())
            })
            .expect("remaining rows are non-empty");
        if values[selected_row][column].abs() <= tolerance {
            continue;
        }
        values.swap(pivot_row, selected_row);
        let pivot = values[pivot_row][column];
        let (pivot_rows, remaining_rows) = values.split_at_mut(pivot_row + 1);
        let pivot_values = &pivot_rows[pivot_row];
        for row in remaining_rows {
            let factor = row[column] / pivot;
            row[column] = 0.0;
            for (cell, pivot_cell) in row.iter_mut().zip(pivot_values).skip(column + 1) {
                *cell -= factor * pivot_cell;
                if !cell.is_finite() {
                    return Err(arithmetic_error(span));
                }
            }
        }
        pivot_row += 1;
    }
    Ok(Value::Int(pivot_row as i64))
}

pub(crate) fn matrix_vector_multiply(
    heap: &RuntimeHeap,
    matrix: &Value,
    vector: &Value,
    span: Option<&Span>,
) -> Result<Value, SimplyError> {
    let rows = matrix_rows(matrix, span)?;
    let vector = vector_values(vector, span)?;
    if rows[0].len() != vector.len() {
        return Err(collection_error(
            span,
            "matrix and vector dimensions do not match",
        ));
    }
    ensure_matrix_allocation(rows.len(), 1, span, "matrix-vector result is too large")?;
    let result = rows
        .iter()
        .map(|row| {
            let mut total = 0.0;
            for (matrix_value, vector_value) in row.iter().zip(&vector) {
                total += numeric_value(matrix_value, span)? * numeric_value(vector_value, span)?;
                if !total.is_finite() {
                    return Err(arithmetic_error(span));
                }
            }
            float_result(total, span)
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Value::Array(heap.insert_sequence(result)))
}

pub(crate) fn matrix_determinant(
    matrix: &Value,
    span: Option<&Span>,
) -> Result<Value, SimplyError> {
    let rows = matrix_rows(matrix, span)?;
    let size = rows.len();
    if rows[0].len() != size {
        return Err(collection_error(
            span,
            "determinant requires a square matrix",
        ));
    }
    ensure_matrix_allocation(size, size, span, "determinant workspace is too large")?;

    let mut values = rows
        .iter()
        .map(|row| row.iter().map(|value| numeric_value(value, span)).collect())
        .collect::<Result<Vec<Vec<_>>, _>>()?;
    let mut determinant = 1.0;
    let mut sign = 1.0;

    for pivot_column in 0..size {
        let pivot_row = (pivot_column..size)
            .max_by(|left, right| {
                values[*left][pivot_column]
                    .abs()
                    .total_cmp(&values[*right][pivot_column].abs())
            })
            .expect("square matrix has at least one row");
        let pivot = values[pivot_row][pivot_column];
        if pivot == 0.0 {
            return Ok(Value::Float(0.0));
        }
        if pivot_row != pivot_column {
            values.swap(pivot_row, pivot_column);
            sign = -sign;
        }
        let pivot = values[pivot_column][pivot_column];
        determinant *= pivot;
        if !determinant.is_finite() {
            return Err(arithmetic_error(span));
        }

        let (pivot_rows, remaining_rows) = values.split_at_mut(pivot_column + 1);
        let pivot_values = &pivot_rows[pivot_column];
        for row in remaining_rows {
            let factor = row[pivot_column] / pivot;
            row[pivot_column] = 0.0;
            for (cell, pivot_cell) in row.iter_mut().zip(pivot_values).skip(pivot_column + 1) {
                *cell -= factor * pivot_cell;
                if !cell.is_finite() {
                    return Err(arithmetic_error(span));
                }
            }
        }
    }

    float_result(sign * determinant, span)
}

pub(crate) fn matrix_inverse(
    heap: &RuntimeHeap,
    matrix: &Value,
    span: Option<&Span>,
) -> Result<Value, SimplyError> {
    let rows = matrix_rows(matrix, span)?;
    let size = rows.len();
    if rows[0].len() != size {
        return Err(collection_error(span, "inverse requires a square matrix"));
    }
    ensure_matrix_allocation(size, size, span, "matrix inverse is too large to allocate")?;

    let mut left = rows
        .iter()
        .map(|row| row.iter().map(|value| numeric_value(value, span)).collect())
        .collect::<Result<Vec<Vec<_>>, _>>()?;
    let mut right = (0..size)
        .map(|row| {
            (0..size)
                .map(|column| if row == column { 1.0 } else { 0.0 })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();

    for pivot_column in 0..size {
        let pivot_row = (pivot_column..size)
            .max_by(|left_row, right_row| {
                left[*left_row][pivot_column]
                    .abs()
                    .total_cmp(&left[*right_row][pivot_column].abs())
            })
            .expect("square matrix has at least one row");
        if left[pivot_row][pivot_column] == 0.0 {
            return Err(arithmetic_error_with_message(
                span,
                "matrix is singular and cannot be inverted",
            ));
        }
        if pivot_row != pivot_column {
            left.swap(pivot_row, pivot_column);
            right.swap(pivot_row, pivot_column);
        }

        let pivot = left[pivot_column][pivot_column];
        for column in 0..size {
            left[pivot_column][column] /= pivot;
            right[pivot_column][column] /= pivot;
        }
        for row in 0..size {
            if row == pivot_column {
                continue;
            }
            let factor = left[row][pivot_column];
            for column in 0..size {
                left[row][column] -= factor * left[pivot_column][column];
                right[row][column] -= factor * right[pivot_column][column];
            }
        }
    }

    let result = right
        .into_iter()
        .map(|row| {
            row.into_iter()
                .map(|value| float_result(value, span))
                .collect::<Result<Vec<_>, _>>()
                .map(|row| Value::Array(heap.insert_sequence(row)))
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Value::Matrix(shared_values(result)))
}

pub(crate) fn matrix_solve(
    heap: &RuntimeHeap,
    matrix: &Value,
    right_hand_side: &Value,
    span: Option<&Span>,
) -> Result<Value, SimplyError> {
    let rows = matrix_rows(matrix, span)?;
    let size = rows.len();
    if rows[0].len() != size {
        return Err(collection_error(span, "solve requires a square matrix"));
    }
    let rhs_is_matrix = matches!(right_hand_side, Value::Matrix(_))
        || matches!(
            right_hand_side,
            Value::Array(values) | Value::List(values)
                if values.first().is_some_and(|value| matches!(value, Value::Array(_) | Value::List(_)))
        );
    let (rhs_columns, mut values) = if rhs_is_matrix {
        let rhs_rows = matrix_rows(right_hand_side, span)?;
        if rhs_rows.len() != size {
            return Err(collection_error(
                span,
                "solve requires one right-hand-side row per matrix row",
            ));
        }
        (
            rhs_rows[0].len(),
            rhs_rows
                .iter()
                .map(|row| row.iter().map(|value| numeric_value(value, span)).collect())
                .collect::<Result<Vec<Vec<_>>, _>>()?,
        )
    } else {
        let vector = vector_values(right_hand_side, span)?;
        if vector.len() != size {
            return Err(collection_error(
                span,
                "solve requires one right-hand-side value per matrix row",
            ));
        }
        (
            1,
            vector
                .iter()
                .map(|value| numeric_value(value, span).map(|value| vec![value]))
                .collect::<Result<Vec<Vec<_>>, _>>()?,
        )
    };
    ensure_matrix_allocation(
        size,
        size.checked_add(rhs_columns)
            .ok_or_else(|| limit_error(span, "linear system workspace is too large"))?,
        span,
        "linear system workspace is too large",
    )?;

    let mut coefficients = rows
        .iter()
        .map(|row| row.iter().map(|value| numeric_value(value, span)).collect())
        .collect::<Result<Vec<Vec<_>>, _>>()?;

    for pivot_column in 0..size {
        let pivot_row = (pivot_column..size)
            .max_by(|left, right| {
                coefficients[*left][pivot_column]
                    .abs()
                    .total_cmp(&coefficients[*right][pivot_column].abs())
            })
            .expect("square matrix has at least one row");
        if coefficients[pivot_row][pivot_column] == 0.0 {
            return Err(arithmetic_error_with_message(
                span,
                "linear system has no unique solution",
            ));
        }
        if pivot_row != pivot_column {
            coefficients.swap(pivot_row, pivot_column);
            values.swap(pivot_row, pivot_column);
        }

        let pivot = coefficients[pivot_column][pivot_column];
        let (pivot_rows, remaining_rows) = coefficients.split_at_mut(pivot_column + 1);
        let pivot_values = &pivot_rows[pivot_column];
        let pivot_rhs = values[pivot_column].clone();
        let (_, remaining_rhs) = values.split_at_mut(pivot_column + 1);
        for (row, rhs) in remaining_rows.iter_mut().zip(remaining_rhs) {
            let factor = row[pivot_column] / pivot;
            if !factor.is_finite() {
                return Err(arithmetic_error(span));
            }
            row[pivot_column] = 0.0;
            for (cell, pivot_cell) in row.iter_mut().zip(pivot_values).skip(pivot_column + 1) {
                *cell -= factor * pivot_cell;
                if !cell.is_finite() {
                    return Err(arithmetic_error(span));
                }
            }
            for (rhs_value, pivot_rhs_value) in rhs.iter_mut().zip(&pivot_rhs) {
                *rhs_value -= factor * pivot_rhs_value;
                if !rhs_value.is_finite() {
                    return Err(arithmetic_error(span));
                }
            }
        }
    }

    let mut solution = vec![vec![0.0; rhs_columns]; size];
    for row in (0..size).rev() {
        for column in 0..rhs_columns {
            let mut value = values[row][column];
            for (coefficient, solved) in coefficients[row][row + 1..]
                .iter()
                .zip(&solution[row + 1..])
            {
                value -= coefficient * solved[column];
            }
            solution[row][column] = value / coefficients[row][row];
            if !solution[row][column].is_finite() {
                return Err(arithmetic_error(span));
            }
        }
    }

    let output = solution
        .into_iter()
        .map(|row| {
            row.into_iter()
                .map(|value| float_result(value, span))
                .collect::<Result<Vec<_>, _>>()
        })
        .collect::<Result<Vec<_>, _>>()?;
    if rhs_is_matrix {
        Ok(Value::Matrix(shared_values(
            output
                .into_iter()
                .map(|row| Value::Array(heap.insert_sequence(row)))
                .collect(),
        )))
    } else {
        let solution = output
            .into_iter()
            .map(|mut row| row.pop().expect("vector solve has one RHS column"))
            .collect();
        Ok(Value::Array(heap.insert_sequence(solution)))
    }
}

pub(crate) fn matrix_lu(
    heap: &RuntimeHeap,
    matrix: &Value,
    span: Option<&Span>,
) -> Result<Value, SimplyError> {
    let rows = matrix_rows(matrix, span)?;
    let row_count = rows.len();
    let column_count = rows[0].len();
    let rank_bound = row_count.min(column_count);
    ensure_matrix_allocation(row_count, column_count, span, "LU workspace is too large")?;
    ensure_matrix_allocation(row_count, row_count, span, "LU permutation is too large")?;

    let mut factors = rows
        .iter()
        .map(|row| row.iter().map(|value| numeric_value(value, span)).collect())
        .collect::<Result<Vec<Vec<_>>, _>>()?;
    let mut permutation = (0..row_count).collect::<Vec<_>>();

    for pivot_column in 0..rank_bound {
        let pivot_row = (pivot_column..row_count)
            .max_by(|left, right| {
                factors[*left][pivot_column]
                    .abs()
                    .total_cmp(&factors[*right][pivot_column].abs())
            })
            .expect("pivot column has at least one candidate row");
        if factors[pivot_row][pivot_column] == 0.0 {
            continue;
        }
        if pivot_row != pivot_column {
            factors.swap(pivot_row, pivot_column);
            permutation.swap(pivot_row, pivot_column);
        }
        let pivot = factors[pivot_column][pivot_column];
        let (pivot_rows, remaining_rows) = factors.split_at_mut(pivot_column + 1);
        let pivot_values = &pivot_rows[pivot_column][pivot_column + 1..];
        for row in remaining_rows {
            let factor = row[pivot_column] / pivot;
            if !factor.is_finite() {
                return Err(arithmetic_error(span));
            }
            row[pivot_column] = factor;
            for (cell, pivot_cell) in row[pivot_column + 1..].iter_mut().zip(pivot_values) {
                *cell -= factor * pivot_cell;
                if !cell.is_finite() {
                    return Err(arithmetic_error(span));
                }
            }
        }
    }

    let lower = (0..row_count)
        .map(|row| {
            (0..rank_bound)
                .map(|column| {
                    if row == column {
                        1.0
                    } else if row > column {
                        factors[row][column]
                    } else {
                        0.0
                    }
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let upper = (0..rank_bound)
        .map(|row| {
            (0..column_count)
                .map(|column| {
                    if row <= column {
                        factors[row][column]
                    } else {
                        0.0
                    }
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let permutation_matrix = (0..row_count)
        .map(|row| {
            (0..row_count)
                .map(|column| f64::from(permutation[row] == column))
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();

    Ok(Value::Tuple(shared_values(vec![
        float_matrix_value(heap, lower, span)?,
        float_matrix_value(heap, upper, span)?,
        float_matrix_value(heap, permutation_matrix, span)?,
    ])))
}

pub(crate) fn matrix_qr(
    heap: &RuntimeHeap,
    matrix: &Value,
    span: Option<&Span>,
) -> Result<Value, SimplyError> {
    let rows = matrix_rows(matrix, span)?;
    let row_count = rows.len();
    let column_count = rows[0].len();
    ensure_matrix_allocation(row_count, column_count, span, "QR workspace is too large")?;
    ensure_matrix_allocation(
        row_count,
        row_count,
        span,
        "QR orthogonal factor is too large",
    )?;

    let mut upper = rows
        .iter()
        .map(|row| row.iter().map(|value| numeric_value(value, span)).collect())
        .collect::<Result<Vec<Vec<_>>, _>>()?;
    let mut orthogonal = (0..row_count)
        .map(|row| {
            (0..row_count)
                .map(|column| f64::from(row == column))
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();

    for pivot_column in 0..row_count.min(column_count) {
        let norm = upper[pivot_column..]
            .iter()
            .fold(0.0_f64, |norm, row| norm.hypot(row[pivot_column]));
        if norm == 0.0 {
            continue;
        }
        let sign = if upper[pivot_column][pivot_column].is_sign_negative() {
            -1.0
        } else {
            1.0
        };
        let mut reflector = upper[pivot_column..]
            .iter()
            .map(|row| row[pivot_column] / norm)
            .collect::<Vec<_>>();
        reflector[0] += sign;
        let norm_squared = reflector.iter().map(|value| value * value).sum::<f64>();
        if !norm_squared.is_finite() || norm_squared == 0.0 {
            return Err(arithmetic_error(span));
        }
        let beta = 2.0 / norm_squared;

        for column in pivot_column..column_count {
            let projection = reflector
                .iter()
                .zip(&upper[pivot_column..])
                .map(|(reflector, row)| reflector * row[column])
                .sum::<f64>()
                * beta;
            for (reflector, row) in reflector.iter().zip(&mut upper[pivot_column..]) {
                row[column] -= projection * reflector;
                if !row[column].is_finite() {
                    return Err(arithmetic_error(span));
                }
            }
        }
        for row in &mut orthogonal {
            let projection = reflector
                .iter()
                .zip(&row[pivot_column..])
                .map(|(reflector, value)| reflector * value)
                .sum::<f64>()
                * beta;
            for (reflector, value) in reflector.iter().zip(&mut row[pivot_column..]) {
                *value -= projection * reflector;
                if !value.is_finite() {
                    return Err(arithmetic_error(span));
                }
            }
        }
        upper[pivot_column][pivot_column] = -sign * norm;
        for row in &mut upper[pivot_column + 1..] {
            row[pivot_column] = 0.0;
        }
    }

    Ok(Value::Tuple(shared_values(vec![
        float_matrix_value(heap, orthogonal, span)?,
        float_matrix_value(heap, upper, span)?,
    ])))
}

pub(crate) fn matrix_cholesky(
    heap: &RuntimeHeap,
    matrix: &Value,
    span: Option<&Span>,
) -> Result<Value, SimplyError> {
    let rows = matrix_rows(matrix, span)?;
    let size = rows.len();
    if rows[0].len() != size {
        return Err(collection_error(span, "cholesky requires a square matrix"));
    }
    ensure_matrix_allocation(size, size, span, "Cholesky workspace is too large")?;

    let values = rows
        .iter()
        .map(|row| row.iter().map(|value| numeric_value(value, span)).collect())
        .collect::<Result<Vec<Vec<_>>, _>>()?;
    let scale = values
        .iter()
        .flatten()
        .fold(0.0_f64, |maximum, value| maximum.max(value.abs()));
    let symmetry_tolerance = scale * size as f64 * f64::EPSILON;
    for (row, row_values) in values.iter().enumerate() {
        for (column, column_values) in values.iter().enumerate().take(row) {
            if (row_values[column] - column_values[row]).abs() > symmetry_tolerance {
                return Err(arithmetic_error_with_message(
                    span,
                    "cholesky requires a symmetric positive-definite matrix",
                ));
            }
        }
    }

    let mut lower = vec![vec![0.0; size]; size];
    for row in 0..size {
        for column in 0..=row {
            let mut value = values[row][column];
            for (row_value, column_value) in
                lower[row][..column].iter().zip(&lower[column][..column])
            {
                value -= row_value * column_value;
            }
            if row == column {
                if !value.is_finite() || value <= 0.0 {
                    return Err(arithmetic_error_with_message(
                        span,
                        "cholesky requires a symmetric positive-definite matrix",
                    ));
                }
                lower[row][column] = value.sqrt();
            } else {
                lower[row][column] = value / lower[column][column];
            }
            if !lower[row][column].is_finite() {
                return Err(arithmetic_error(span));
            }
        }
    }

    float_matrix_value(heap, lower, span)
}

fn float_matrix_value(
    heap: &RuntimeHeap,
    rows: Vec<Vec<f64>>,
    span: Option<&Span>,
) -> Result<Value, SimplyError> {
    rows.into_iter()
        .map(|row| {
            row.into_iter()
                .map(|value| float_result(value, span))
                .collect::<Result<Vec<_>, _>>()
                .map(|row| Value::Array(heap.insert_sequence(row)))
        })
        .collect::<Result<Vec<_>, _>>()
        .map(|rows| Value::Matrix(shared_values(rows)))
}

pub(crate) fn matrix_least_squares(
    heap: &RuntimeHeap,
    matrix: &Value,
    right_hand_side: &Value,
    span: Option<&Span>,
) -> Result<Value, SimplyError> {
    let rows = matrix_rows(matrix, span)?;
    let row_count = rows.len();
    let column_count = rows[0].len();
    if row_count < column_count {
        return Err(collection_error(
            span,
            "least_squares requires at least as many rows as columns",
        ));
    }
    let right_hand_side = vector_values(right_hand_side, span)?;
    if right_hand_side.len() != row_count {
        return Err(collection_error(
            span,
            "least_squares requires one right-hand-side value per matrix row",
        ));
    }
    ensure_matrix_allocation(
        row_count,
        column_count,
        span,
        "least-squares workspace is too large",
    )?;

    let mut coefficients = rows
        .iter()
        .map(|row| row.iter().map(|value| numeric_value(value, span)).collect())
        .collect::<Result<Vec<Vec<_>>, _>>()?;
    let mut values = right_hand_side
        .iter()
        .map(|value| numeric_value(value, span))
        .collect::<Result<Vec<_>, _>>()?;
    let scale = coefficients
        .iter()
        .flatten()
        .fold(0.0_f64, |maximum, value| maximum.max(value.abs()));
    let tolerance = scale * row_count.max(column_count) as f64 * f64::EPSILON;

    for pivot_column in 0..column_count {
        let mut column_norm = 0.0_f64;
        for row in &coefficients[pivot_column..] {
            column_norm = column_norm.hypot(row[pivot_column]);
        }
        if column_norm <= tolerance {
            return Err(arithmetic_error_with_message(
                span,
                "least_squares matrix must have full column rank",
            ));
        }

        let diagonal = coefficients[pivot_column][pivot_column];
        let sign = if diagonal.is_sign_negative() {
            -1.0
        } else {
            1.0
        };
        let mut reflector = coefficients[pivot_column..]
            .iter()
            .map(|row| row[pivot_column] / column_norm)
            .collect::<Vec<_>>();
        reflector[0] += sign;
        let reflector_norm_squared = reflector.iter().map(|value| value * value).sum::<f64>();
        if !reflector_norm_squared.is_finite() || reflector_norm_squared == 0.0 {
            return Err(arithmetic_error(span));
        }
        let beta = 2.0 / reflector_norm_squared;

        for column in pivot_column..column_count {
            let projection = reflector
                .iter()
                .zip(&coefficients[pivot_column..])
                .map(|(reflector_value, row)| reflector_value * row[column])
                .sum::<f64>()
                * beta;
            if !projection.is_finite() {
                return Err(arithmetic_error(span));
            }
            for (reflector_value, row) in reflector.iter().zip(&mut coefficients[pivot_column..]) {
                row[column] -= projection * reflector_value;
                if !row[column].is_finite() {
                    return Err(arithmetic_error(span));
                }
            }
        }

        let projection = reflector
            .iter()
            .zip(&values[pivot_column..])
            .map(|(reflector_value, value)| reflector_value * value)
            .sum::<f64>()
            * beta;
        if !projection.is_finite() {
            return Err(arithmetic_error(span));
        }
        for (reflector_value, value) in reflector.iter().zip(&mut values[pivot_column..]) {
            *value -= projection * reflector_value;
            if !value.is_finite() {
                return Err(arithmetic_error(span));
            }
        }
        coefficients[pivot_column][pivot_column] = -sign * column_norm;
        for row in &mut coefficients[pivot_column + 1..] {
            row[pivot_column] = 0.0;
        }
    }

    let mut solution = vec![0.0; column_count];
    for row in (0..column_count).rev() {
        let mut value = values[row];
        for (coefficient, solved) in coefficients[row][row + 1..]
            .iter()
            .zip(&solution[row + 1..])
        {
            value -= coefficient * solved;
        }
        solution[row] = value / coefficients[row][row];
        if !solution[row].is_finite() {
            return Err(arithmetic_error(span));
        }
    }

    solution
        .into_iter()
        .map(|value| float_result(value, span))
        .collect::<Result<Vec<_>, _>>()
        .map(|solution| Value::Array(heap.insert_sequence(solution)))
}

pub(crate) fn matrix_transpose_value(
    heap: &RuntimeHeap,
    matrix: &Value,
    span: Option<&Span>,
) -> Result<Value, SimplyError> {
    let rows = matrix_rows(matrix, span)?;
    let height = rows.len();
    let width = rows[0].len();
    ensure_matrix_allocation(
        width,
        height,
        span,
        "matrix transpose is too large to allocate",
    )?;
    let mut transposed = Vec::with_capacity(width);
    for column in 0..width {
        let mut row = Vec::with_capacity(height);
        for source_row in &rows {
            row.push(numeric_result_value(source_row[column].clone(), span)?);
        }
        transposed.push(Value::Array(heap.insert_sequence(row)));
    }
    Ok(Value::Matrix(shared_values(transposed)))
}

pub(crate) fn matrix_add_values(
    heap: &RuntimeHeap,
    left: &Value,
    right: &Value,
    subtract: bool,
    span: Option<&Span>,
) -> Result<Value, SimplyError> {
    let left = matrix_rows(left, span)?;
    let right = matrix_rows(right, span)?;
    if left.len() != right.len() || left[0].len() != right[0].len() {
        return Err(collection_error(span, "matrix dimensions do not match"));
    }
    ensure_matrix_allocation(
        left.len(),
        left[0].len(),
        span,
        "matrix result is too large to allocate",
    )?;
    let operator = if subtract {
        BinaryOperator::Subtract
    } else {
        BinaryOperator::Add
    };
    let mut result = Vec::with_capacity(left.len());
    for (left_row, right_row) in left.iter().zip(right) {
        let row = left_row
            .iter()
            .zip(right_row)
            .map(|(left, right)| binary(heap, (*left).clone(), &operator, right.clone(), span))
            .collect::<Result<Vec<_>, _>>()?;
        result.push(Value::Array(heap.insert_sequence(row)));
    }
    Ok(Value::Matrix(shared_values(result)))
}

pub(crate) fn matrix_scale(
    heap: &RuntimeHeap,
    matrix: &Value,
    scalar: &Value,
    span: Option<&Span>,
) -> Result<Value, SimplyError> {
    numeric_value(scalar, span)?;
    let rows = matrix_rows(matrix, span)?;
    ensure_matrix_allocation(
        rows.len(),
        rows[0].len(),
        span,
        "matrix result is too large to allocate",
    )?;
    let result = rows
        .iter()
        .map(|row| {
            row.iter()
                .map(|value| {
                    binary(
                        heap,
                        (*value).clone(),
                        &BinaryOperator::Multiply,
                        scalar.clone(),
                        span,
                    )
                })
                .collect::<Result<Vec<_>, _>>()
                .map(|row| Value::Array(heap.insert_sequence(row)))
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Value::Matrix(shared_values(result)))
}

pub(crate) fn matrix_multiply_values(
    heap: &RuntimeHeap,
    left: &Value,
    right: &Value,
    span: Option<&Span>,
) -> Result<Value, SimplyError> {
    let left = matrix_rows(left, span)?;
    let right = matrix_rows(right, span)?;
    if left[0].len() != right.len() {
        return Err(collection_error(span, "matrix dimensions do not match"));
    }
    ensure_matrix_allocation(
        left.len(),
        right[0].len(),
        span,
        "matrix result is too large to allocate",
    )?;
    let mut result = Vec::with_capacity(left.len());
    for left_row in &left {
        let mut row = vec![0.0; right[0].len()];
        for (index, left_value) in left_row.iter().enumerate() {
            let left_value = numeric_value(left_value, span)?;
            for (total, right_value) in row.iter_mut().zip(&right[index]) {
                *total += left_value * numeric_value(right_value, span)?;
                if !total.is_finite() {
                    return Err(arithmetic_error(span));
                }
            }
        }
        result.push(Value::Array(
            heap.insert_sequence(row.into_iter().map(Value::Float).collect()),
        ));
    }
    Ok(Value::Matrix(shared_values(result)))
}

pub(crate) fn matrix_identity(
    heap: &RuntimeHeap,
    size: usize,
    span: Option<&Span>,
) -> Result<Value, SimplyError> {
    if size == 0 {
        return Err(SimplyError::Runtime {
            span: span.cloned().unwrap_or_else(|| Span::new(0, 0)),
            code: DiagnosticCode::RuntimeArgument,
            message: "identity matrix size must be positive".into(),
        });
    }
    ensure_matrix_allocation(size, size, span, "identity matrix is too large to allocate")?;
    let mut rows = Vec::new();
    rows.try_reserve_exact(size)
        .map_err(|_| limit_error(span, "identity matrix is too large to allocate"))?;
    for row_index in 0..size {
        let mut row = Vec::new();
        row.try_reserve_exact(size)
            .map_err(|_| limit_error(span, "identity matrix is too large to allocate"))?;
        for column_index in 0..size {
            row.push(Value::Int(i64::from(row_index == column_index)));
        }
        rows.push(Value::Array(heap.insert_sequence(row)));
    }
    Ok(Value::Matrix(shared_values(rows)))
}

pub(crate) fn statistics_values(
    values: &Value,
    span: Option<&Span>,
) -> Result<Vec<f64>, SimplyError> {
    match values {
        Value::Array(_) | Value::List(_) | Value::Tuple(_) => values
            .sequence_snapshot()
            .unwrap_or_default()
            .iter()
            .map(|value| numeric_value(value, span))
            .collect(),
        Value::Range { start, end, step } => Value::range_values(*start, *end, *step)
            .map(|value| match value {
                Value::Int(value) => Ok(value as f64),
                _ => unreachable!("range values are integers"),
            })
            .collect(),
        _ => Err(type_error(span, "expected a numeric sequence")),
    }
}

pub(crate) fn population_moments(
    values: &[f64],
    span: Option<&Span>,
) -> Result<(f64, f64), SimplyError> {
    if values.is_empty() {
        return Err(argument_error(
            span,
            "statistic requires at least one value",
        ));
    }
    let mut mean = 0.0;
    let mut sum_squared_deviations = 0.0;
    for (index, value) in values.iter().enumerate() {
        let count = (index + 1) as f64;
        let delta = value - mean;
        mean += delta / count;
        sum_squared_deviations += delta * (value - mean);
    }
    float_result(mean, span)?;
    let variance = float_result(sum_squared_deviations / values.len() as f64, span)?;
    let Value::Float(variance) = variance else {
        unreachable!()
    };
    Ok((mean, variance.max(0.0)))
}

pub(crate) fn statistics_unary(
    name: &str,
    values: &[f64],
    percentile: Option<f64>,
    span: Option<&Span>,
) -> Result<Value, SimplyError> {
    if values.is_empty() {
        return Err(argument_error(
            span,
            format!("`{name}` requires at least one numeric value"),
        ));
    }
    match name {
        "mean" => {
            let (mean, _) = population_moments(values, span)?;
            float_result(mean, span)
        }
        "variance" => {
            let (_, variance) = population_moments(values, span)?;
            float_result(variance, span)
        }
        "stddev" => {
            let (_, variance) = population_moments(values, span)?;
            float_result(variance.sqrt(), span)
        }
        "median" | "percentile" => {
            let percent = if name == "median" {
                50.0
            } else {
                percentile
                    .ok_or_else(|| argument_error(span, "`percentile` requires a percentile"))?
            };
            if !(0.0..=100.0).contains(&percent) {
                return Err(argument_error(
                    span,
                    "`percentile` must be between 0 and 100 inclusive",
                ));
            }
            let mut sorted = values.to_vec();
            sorted.sort_by(f64::total_cmp);
            let position = percent / 100.0 * (sorted.len() - 1) as f64;
            let lower = position.floor() as usize;
            let upper = position.ceil() as usize;
            let fraction = position - lower as f64;
            float_result(
                sorted[lower] + (sorted[upper] - sorted[lower]) * fraction,
                span,
            )
        }
        _ => Err(argument_error(span, format!("unknown statistic `{name}`"))),
    }
}

pub(crate) fn statistics_pair(
    name: &str,
    left: &[f64],
    right: &[f64],
    span: Option<&Span>,
) -> Result<Value, SimplyError> {
    if left.is_empty() || right.is_empty() {
        return Err(argument_error(
            span,
            format!("`{name}` requires non-empty sequences"),
        ));
    }
    if left.len() != right.len() {
        return Err(collection_error(
            span,
            "statistic sequence lengths do not match",
        ));
    }
    let mut mean_left = 0.0;
    let mut mean_right = 0.0;
    let mut co_moment = 0.0;
    let mut left_moment = 0.0;
    let mut right_moment = 0.0;
    for (index, (left, right)) in left.iter().zip(right).enumerate() {
        let count = (index + 1) as f64;
        let delta_left = left - mean_left;
        let delta_right = right - mean_right;
        mean_left += delta_left / count;
        mean_right += delta_right / count;
        co_moment += delta_left * (right - mean_right);
        left_moment += delta_left * (left - mean_left);
        right_moment += delta_right * (right - mean_right);
    }
    let result = match name {
        "covariance" => co_moment / left.len() as f64,
        "correlation" => {
            if left_moment <= 0.0 || right_moment <= 0.0 {
                return Err(arithmetic_error_with_message(
                    span,
                    "`correlation` requires non-constant input sequences",
                ));
            }
            co_moment / (left_moment * right_moment).sqrt()
        }
        _ => return Err(argument_error(span, format!("unknown statistic `{name}`"))),
    };
    float_result(result, span)
}

fn vector_values(value: &Value, span: Option<&Span>) -> Result<Vec<Value>, SimplyError> {
    let values = value
        .sequence_snapshot()
        .ok_or_else(|| type_error(span, "expected a vector sequence"))?;
    if values.is_empty() {
        return Err(argument_error(span, "vector must not be empty"));
    }
    for value in &values {
        numeric_value(value, span)?;
    }
    Ok(values)
}

fn matrix_rows(value: &Value, span: Option<&Span>) -> Result<Vec<Vec<Value>>, SimplyError> {
    let values = value
        .sequence_snapshot()
        .ok_or_else(|| type_error(span, "expected a matrix of numeric rows"))?;
    if values.is_empty() {
        return Err(collection_error(span, "matrix must not be empty"));
    }
    let mut rows = Vec::with_capacity(values.len());
    let mut width = None;
    for row in values.iter() {
        let row = row
            .sequence_snapshot()
            .ok_or_else(|| type_error(span, "matrix rows must be arrays or lists"))?;
        if row.is_empty() {
            return Err(collection_error(span, "matrix rows must not be empty"));
        }
        if let Some(width) = width
            && width != row.len()
        {
            return Err(collection_error(span, "matrix rows must have equal widths"));
        }
        width = Some(row.len());
        for value in &row {
            numeric_value(value, span)?;
        }
        rows.push(row);
    }
    Ok(rows)
}

fn numeric_result_value(value: Value, span: Option<&Span>) -> Result<Value, SimplyError> {
    numeric_value(&value, span)?;
    Ok(value)
}

fn matrix_shape(matrix: &[Value], span: Option<&Span>) -> Result<usize, SimplyError> {
    if matrix.is_empty() {
        return Err(collection_error(span, "matrix must not be empty"));
    }
    let width = matrix_row_values(&matrix[0], span)?.len();
    if width == 0 {
        return Err(collection_error(span, "matrix rows must not be empty"));
    }
    for row in matrix {
        let values = matrix_row_values(row, span)?;
        if values.len() != width {
            return Err(collection_error(span, "matrix rows must have equal widths"));
        }
        if values
            .iter()
            .any(|value| !matches!(value, Value::Int(_) | Value::Float(_)))
        {
            return Err(type_error(span, "matrix values must be numeric"));
        }
    }
    Ok(width)
}

fn matrix_row_values(row: &Value, span: Option<&Span>) -> Result<Vec<Value>, SimplyError> {
    row.sequence_snapshot()
        .ok_or_else(|| type_error(span, "matrix rows must be arrays or lists"))
}

fn matrix_transpose(
    heap: &RuntimeHeap,
    rows: &[Value],
    span: Option<&Span>,
) -> Result<Value, SimplyError> {
    let width = matrix_shape(rows, span)?;
    ensure_matrix_allocation(
        width,
        rows.len(),
        span,
        "matrix transpose is too large to allocate",
    )?;
    let mut result = Vec::with_capacity(width);
    for column in 0..width {
        let mut output = Vec::with_capacity(rows.len());
        for row in rows {
            let values = matrix_row_values(row, span)?;
            output.push(Value::Float(numeric_value(&values[column], span)?));
        }
        result.push(Value::Array(heap.insert_sequence(output)));
    }
    Ok(Value::Matrix(shared_values(result)))
}

fn numeric_value(value: &Value, span: Option<&Span>) -> Result<f64, SimplyError> {
    match value {
        Value::Int(value) => Ok(*value as f64),
        Value::Float(value) if value.is_finite() => Ok(*value),
        _ => Err(type_error(span, "expected a finite numeric value")),
    }
}

fn type_error(span: Option<&Span>, message: impl Into<String>) -> SimplyError {
    SimplyError::Runtime {
        span: span.cloned().unwrap_or_else(|| Span::new(0, 0)),
        code: DiagnosticCode::RuntimeTypeMismatch,
        message: message.into(),
    }
}

fn collection_error(span: Option<&Span>, message: impl Into<String>) -> SimplyError {
    SimplyError::Runtime {
        span: span.cloned().unwrap_or_else(|| Span::new(0, 0)),
        code: DiagnosticCode::RuntimeCollection,
        message: message.into(),
    }
}

fn argument_error(span: Option<&Span>, message: impl Into<String>) -> SimplyError {
    SimplyError::Runtime {
        span: span.cloned().unwrap_or_else(|| Span::new(0, 0)),
        code: DiagnosticCode::RuntimeArgument,
        message: message.into(),
    }
}

fn arithmetic_error_with_message(span: Option<&Span>, message: impl Into<String>) -> SimplyError {
    SimplyError::Runtime {
        span: span.cloned().unwrap_or_else(|| Span::new(0, 0)),
        code: DiagnosticCode::RuntimeArithmetic,
        message: message.into(),
    }
}

fn limit_error(span: Option<&Span>, message: impl Into<String>) -> SimplyError {
    SimplyError::Runtime {
        span: span.cloned().unwrap_or_else(|| Span::new(0, 0)),
        code: DiagnosticCode::RuntimeLimit,
        message: message.into(),
    }
}

fn ensure_matrix_allocation(
    rows: usize,
    columns: usize,
    span: Option<&Span>,
    message: &str,
) -> Result<(), SimplyError> {
    let cells = rows
        .checked_mul(columns)
        .ok_or_else(|| limit_error(span, message))?;
    if cells > crate::runtime::limits::MAX_MATRIX_CELLS
        || cells > isize::MAX as usize / std::mem::size_of::<Value>()
    {
        return Err(limit_error(span, message));
    }
    Ok(())
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
