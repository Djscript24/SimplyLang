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

pub(crate) fn math_unary(
    name: &str,
    value: Value,
    span: Option<&Span>,
) -> Result<Value, SimplyError> {
    let value = match value {
        Value::Int(value) => value as f64,
        Value::Float(value) if value.is_finite() => value,
        _ => return Err(error(span, format!("`{name}` requires a finite number"))),
    };
    let result = match name {
        "sqrt" if value >= 0.0 => value.sqrt(),
        "sqrt" => return Err(error(span, "`sqrt` requires a non-negative number")),
        "exp" => value.exp(),
        "log" if value > 0.0 => value.ln(),
        "log" => return Err(error(span, "`log` requires a positive number")),
        "log10" if value > 0.0 => value.log10(),
        "log10" => return Err(error(span, "`log10` requires a positive number")),
        "sin" => value.sin(),
        "cos" => value.cos(),
        "tan" => value.tan(),
        "floor" => value.floor(),
        "ceil" => value.ceil(),
        _ => return Err(error(span, format!("unknown math operation `{name}`"))),
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
        _ => Err(error(span, "`sign` requires a finite number")),
    }
}

pub(crate) fn vector_add(
    left: &Value,
    right: &Value,
    subtract: bool,
    span: Option<&Span>,
) -> Result<Value, SimplyError> {
    let left = vector_values(left, span)?;
    let right = vector_values(right, span)?;
    if left.len() != right.len() {
        return Err(error(span, "vector dimensions do not match"));
    }
    let operator = if subtract {
        BinaryOperator::Subtract
    } else {
        BinaryOperator::Add
    };
    let values = left
        .iter()
        .zip(right)
        .map(|(left, right)| binary((*left).clone(), &operator, right.clone(), span))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Value::Array(shared_values(values)))
}

pub(crate) fn vector_scale(
    vector: &Value,
    scalar: &Value,
    span: Option<&Span>,
) -> Result<Value, SimplyError> {
    numeric_value(scalar, span)?;
    let values = vector_values(vector, span)?
        .iter()
        .map(|value| {
            binary(
                (*value).clone(),
                &BinaryOperator::Multiply,
                scalar.clone(),
                span,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Value::Array(shared_values(values)))
}

pub(crate) fn vector_dot(
    left: &Value,
    right: &Value,
    span: Option<&Span>,
) -> Result<Value, SimplyError> {
    let left = vector_values(left, span)?;
    let right = vector_values(right, span)?;
    if left.len() != right.len() {
        return Err(error(span, "vector dimensions do not match"));
    }
    let mut total = Value::Int(0);
    for (left, right) in left.iter().zip(right) {
        let product = binary(
            (*left).clone(),
            &BinaryOperator::Multiply,
            right.clone(),
            span,
        )?;
        total = binary(total, &BinaryOperator::Add, product, span)?;
    }
    Ok(total)
}

pub(crate) fn vector_norm(vector: &Value, span: Option<&Span>) -> Result<Value, SimplyError> {
    let mut norm = 0.0_f64;
    for value in vector_values(vector, span)? {
        norm = norm.hypot(numeric_value(value, span)?);
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
        return Err(error(span, "vector dimensions do not match"));
    }
    let mut distance = 0.0_f64;
    for (left, right) in left.iter().zip(right) {
        let difference = numeric_value(left, span)? - numeric_value(right, span)?;
        distance = distance.hypot(difference);
    }
    float_result(distance, span)
}

pub(crate) fn vector_normalize(vector: &Value, span: Option<&Span>) -> Result<Value, SimplyError> {
    let values = vector_values(vector, span)?;
    let mut norm = 0.0_f64;
    for value in &values {
        norm = norm.hypot(numeric_value(value, span)?);
    }
    if norm == 0.0 {
        return Err(error(span, "cannot normalize a zero vector"));
    }
    let result = values
        .iter()
        .map(|value| float_result(numeric_value(value, span)? / norm, span))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Value::Array(shared_values(result)))
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

pub(crate) fn matrix_transpose_value(
    matrix: &Value,
    span: Option<&Span>,
) -> Result<Value, SimplyError> {
    let rows = matrix_rows(matrix, span)?;
    let height = rows.len();
    let width = rows[0].len();
    let mut transposed = Vec::with_capacity(width);
    for column in 0..width {
        let mut row = Vec::with_capacity(height);
        for source_row in &rows {
            row.push(numeric_result_value(source_row[column].clone(), span)?);
        }
        transposed.push(Value::Array(shared_values(row)));
    }
    Ok(Value::Matrix(shared_values(transposed)))
}

pub(crate) fn matrix_add_values(
    left: &Value,
    right: &Value,
    subtract: bool,
    span: Option<&Span>,
) -> Result<Value, SimplyError> {
    let left = matrix_rows(left, span)?;
    let right = matrix_rows(right, span)?;
    if left.len() != right.len() || left[0].len() != right[0].len() {
        return Err(error(span, "matrix dimensions do not match"));
    }
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
            .map(|(left, right)| binary((*left).clone(), &operator, right.clone(), span))
            .collect::<Result<Vec<_>, _>>()?;
        result.push(Value::Array(shared_values(row)));
    }
    Ok(Value::Matrix(shared_values(result)))
}

pub(crate) fn matrix_scale(
    matrix: &Value,
    scalar: &Value,
    span: Option<&Span>,
) -> Result<Value, SimplyError> {
    numeric_value(scalar, span)?;
    let rows = matrix_rows(matrix, span)?;
    let result = rows
        .iter()
        .map(|row| {
            row.iter()
                .map(|value| {
                    binary(
                        (*value).clone(),
                        &BinaryOperator::Multiply,
                        scalar.clone(),
                        span,
                    )
                })
                .collect::<Result<Vec<_>, _>>()
                .map(|row| Value::Array(shared_values(row)))
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Value::Matrix(shared_values(result)))
}

pub(crate) fn matrix_multiply_values(
    left: &Value,
    right: &Value,
    span: Option<&Span>,
) -> Result<Value, SimplyError> {
    let left = matrix_rows(left, span)?;
    let right = matrix_rows(right, span)?;
    if left[0].len() != right.len() {
        return Err(error(span, "matrix dimensions do not match"));
    }
    let mut result = Vec::with_capacity(left.len());
    for left_row in &left {
        let mut row = vec![0.0; right[0].len()];
        for (index, left_value) in left_row.iter().enumerate() {
            let left_value = numeric_value(left_value, span)?;
            for (total, right_value) in row.iter_mut().zip(right[index]) {
                *total += left_value * numeric_value(right_value, span)?;
                if !total.is_finite() {
                    return Err(arithmetic_error(span));
                }
            }
        }
        result.push(Value::Array(shared_values(
            row.into_iter().map(Value::Float).collect(),
        )));
    }
    Ok(Value::Matrix(shared_values(result)))
}

pub(crate) fn matrix_identity(size: usize, span: Option<&Span>) -> Result<Value, SimplyError> {
    if size == 0 {
        return Err(error(span, "identity matrix size must be positive"));
    }
    let cells = size
        .checked_mul(size)
        .ok_or_else(|| error(span, "identity matrix size is too large"))?;
    if cells > isize::MAX as usize / std::mem::size_of::<Value>() {
        return Err(error(span, "identity matrix is too large to allocate"));
    }
    let mut rows = Vec::new();
    rows.try_reserve_exact(size)
        .map_err(|_| error(span, "identity matrix is too large to allocate"))?;
    for row_index in 0..size {
        let mut row = Vec::new();
        row.try_reserve_exact(size)
            .map_err(|_| error(span, "identity matrix is too large to allocate"))?;
        for column_index in 0..size {
            row.push(Value::Int(i64::from(row_index == column_index)));
        }
        rows.push(Value::Array(shared_values(row)));
    }
    Ok(Value::Matrix(shared_values(rows)))
}

pub(crate) fn statistics_values(
    values: &Value,
    span: Option<&Span>,
) -> Result<Vec<f64>, SimplyError> {
    match values {
        Value::Array(values) | Value::List(values) | Value::Tuple(values) => values
            .iter()
            .map(|value| numeric_value(value, span))
            .collect(),
        Value::Range { start, end } => (*start..*end).map(|value| Ok(value as f64)).collect(),
        _ => Err(error(span, "expected a numeric sequence")),
    }
}

pub(crate) fn population_moments(
    values: &[f64],
    span: Option<&Span>,
) -> Result<(f64, f64), SimplyError> {
    if values.is_empty() {
        return Err(error(span, "statistic requires at least one value"));
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
        return Err(error(
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
                percentile.ok_or_else(|| error(span, "`percentile` requires a percentile"))?
            };
            if !(0.0..=100.0).contains(&percent) {
                return Err(error(
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
        _ => Err(error(span, format!("unknown statistic `{name}`"))),
    }
}

pub(crate) fn statistics_pair(
    name: &str,
    left: &[f64],
    right: &[f64],
    span: Option<&Span>,
) -> Result<Value, SimplyError> {
    if left.is_empty() || right.is_empty() {
        return Err(error(
            span,
            format!("`{name}` requires non-empty sequences"),
        ));
    }
    if left.len() != right.len() {
        return Err(error(span, "statistic sequence lengths do not match"));
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
                return Err(error(
                    span,
                    "`correlation` requires non-constant input sequences",
                ));
            }
            co_moment / (left_moment * right_moment).sqrt()
        }
        _ => return Err(error(span, format!("unknown statistic `{name}`"))),
    };
    float_result(result, span)
}

fn vector_values<'a>(value: &'a Value, span: Option<&Span>) -> Result<Vec<&'a Value>, SimplyError> {
    let values = match value {
        Value::Array(values) | Value::List(values) | Value::Tuple(values) => values,
        _ => return Err(error(span, "expected a vector sequence")),
    };
    if values.is_empty() {
        return Err(error(span, "vector must not be empty"));
    }
    for value in values.iter() {
        numeric_value(value, span)?;
    }
    Ok(values.iter().collect())
}

fn matrix_rows<'a>(value: &'a Value, span: Option<&Span>) -> Result<Vec<&'a [Value]>, SimplyError> {
    let values = match value {
        Value::Matrix(values) | Value::Array(values) | Value::List(values) => values,
        _ => return Err(error(span, "expected a matrix of numeric rows")),
    };
    if values.is_empty() {
        return Err(error(span, "matrix must not be empty"));
    }
    let mut rows = Vec::with_capacity(values.len());
    let mut width = None;
    for row in values.iter() {
        let row = match row {
            Value::Array(row) | Value::List(row) => row.as_slice(),
            _ => return Err(error(span, "matrix rows must be arrays or lists")),
        };
        if row.is_empty() {
            return Err(error(span, "matrix rows must not be empty"));
        }
        if let Some(width) = width
            && width != row.len()
        {
            return Err(error(span, "matrix rows must have equal widths"));
        }
        width = Some(row.len());
        for value in row {
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
        Value::Float(value) if value.is_finite() => Ok(*value),
        _ => Err(error(span, "expected a finite numeric value")),
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
