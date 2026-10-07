use std::sync::Arc;

use crate::{
    ast::{BinaryOperator, Expr, Literal, PipelineStep},
    error::{DiagnosticCode, SimplyError},
    runtime::{limits, operations, value::Value},
};

#[derive(Debug)]
pub(super) struct ParallelError {
    pub(super) code: DiagnosticCode,
    pub(super) message: String,
}

type ParallelResult<T> = Result<T, ParallelError>;

fn runtime_error(code: DiagnosticCode, message: impl Into<String>) -> ParallelError {
    ParallelError {
        code,
        message: message.into(),
    }
}

fn operation_error(error: SimplyError) -> ParallelError {
    ParallelError {
        code: error.code(),
        message: error.message().to_owned(),
    }
}

#[derive(Clone)]
enum ParallelValue {
    String(String),
    Int(i64),
    Float(f64),
    Bool(bool),
}

impl ParallelValue {
    fn from_value(value: &Value) -> Option<Self> {
        match value {
            Value::String(value) => Some(Self::String(value.clone())),
            Value::Int(value) => Some(Self::Int(*value)),
            Value::Float(value) => Some(Self::Float(*value)),
            Value::Bool(value) => Some(Self::Bool(*value)),
            _ => None,
        }
    }

    fn into_value(self) -> Value {
        match self {
            Self::String(value) => Value::String(value),
            Self::Int(value) => Value::Int(value),
            Self::Float(value) => Value::Float(value),
            Self::Bool(value) => Value::Bool(value),
        }
    }

    fn as_value(&self) -> Value {
        self.clone().into_value()
    }
}

fn evaluate_expression(expression: &Expr, item: &ParallelValue) -> ParallelResult<ParallelValue> {
    let value = match expression {
        Expr::Literal(Literal::String(value)) => Value::String(value.clone()),
        Expr::Literal(Literal::Int(value)) => Value::Int(*value),
        Expr::Literal(Literal::Float(value)) => Value::Float(*value),
        Expr::Literal(Literal::Bool(value)) => Value::Bool(*value),
        Expr::Identifier(name) if name == "item" => item.as_value(),
        Expr::Unary { operator, operand } => {
            let operand = evaluate_expression(operand, item)?.into_value();
            operations::unary(operand, operator, None).map_err(operation_error)?
        }
        Expr::Binary {
            left,
            operator,
            right,
        } => {
            let left = evaluate_expression(left, item)?.into_value();
            if matches!(operator, BinaryOperator::And | BinaryOperator::Or)
                && let Value::Bool(value) = left
                && ((*operator == BinaryOperator::And && !value)
                    || (*operator == BinaryOperator::Or && value))
            {
                return Ok(ParallelValue::Bool(value));
            }
            let right = evaluate_expression(right, item)?.into_value();
            operations::binary(left, operator, right, None).map_err(operation_error)?
        }
        _ => {
            return Err(runtime_error(
                DiagnosticCode::RuntimeArgument,
                "expression is outside the parallel-safe subset",
            ));
        }
    };
    ParallelValue::from_value(&value).ok_or_else(|| {
        runtime_error(
            DiagnosticCode::RuntimeTypeMismatch,
            "parallel expressions must produce scalar values",
        )
    })
}

fn evaluate_chunk(
    input: &[ParallelValue],
    transforms: &[PipelineStep],
) -> ParallelResult<Vec<ParallelValue>> {
    let mut output = Vec::with_capacity(input.len());
    for input in input {
        let mut current = Some(input.clone());
        for step in transforms {
            if current.is_none() {
                break;
            }
            match step {
                PipelineStep::Where(expression) => {
                    let item = current.as_ref().ok_or_else(|| {
                        runtime_error(DiagnosticCode::RuntimeGeneral, "pipeline item was lost")
                    })?;
                    let result = evaluate_expression(expression, item)?;
                    match result {
                        ParallelValue::Bool(true) => {}
                        ParallelValue::Bool(false) => current = None,
                        _ => {
                            return Err(runtime_error(
                                DiagnosticCode::RuntimeTypeMismatch,
                                "pipeline `where` condition must return a boolean",
                            ));
                        }
                    }
                }
                PipelineStep::Derive(expression) => {
                    let item = current.as_ref().ok_or_else(|| {
                        runtime_error(DiagnosticCode::RuntimeGeneral, "pipeline item was lost")
                    })?;
                    current = Some(evaluate_expression(expression, item)?);
                }
                _ => {}
            }
        }
        if let Some(current) = current {
            output.push(current);
        }
    }
    Ok(output)
}

pub(super) fn evaluate_parallel(
    values: Vec<Value>,
    transforms: &[PipelineStep],
    requested_workers: usize,
    requested_chunk_size: Option<usize>,
) -> ParallelResult<Vec<Value>> {
    if requested_workers == 0 || requested_workers > limits::MAX_PARALLEL_WORKERS {
        return Err(runtime_error(
            DiagnosticCode::RuntimeArgument,
            format!(
                "parallel worker count must be between 1 and {}",
                limits::MAX_PARALLEL_WORKERS
            ),
        ));
    }
    let input: Vec<ParallelValue> = values
        .iter()
        .map(|value| {
            ParallelValue::from_value(value).ok_or_else(|| {
                runtime_error(
                    DiagnosticCode::RuntimeTypeMismatch,
                    "values or expressions are outside the parallel-safe subset",
                )
            })
        })
        .collect::<Result<_, _>>()?;
    let chunk_size =
        requested_chunk_size.unwrap_or_else(|| input.len().div_ceil(requested_workers).max(1));
    if chunk_size == 0 || chunk_size > limits::MAX_CHUNK_SIZE {
        return Err(runtime_error(
            DiagnosticCode::RuntimeArgument,
            format!(
                "chunk size must be between 1 and {}",
                limits::MAX_CHUNK_SIZE
            ),
        ));
    }
    let chunk_count = input.len().div_ceil(chunk_size);
    if chunk_count > limits::MAX_PARALLEL_CHUNKS {
        return Err(runtime_error(
            DiagnosticCode::RuntimeLimit,
            format!(
                "parallel chunk count exceeds the limit of {}",
                limits::MAX_PARALLEL_CHUNKS
            ),
        ));
    }
    if chunk_count == 0 {
        return Ok(Vec::new());
    }
    let chunks: Vec<Vec<ParallelValue>> = input
        .chunks(chunk_size)
        .map(<[ParallelValue]>::to_vec)
        .collect();
    let workers = requested_workers.min(chunks.len().max(1));
    let chunks = Arc::new(chunks);
    let mut results = (0..chunks.len()).map(|_| None).collect::<Vec<_>>();
    std::thread::scope(|scope| {
        let mut handles = Vec::with_capacity(workers);
        for worker_index in 0..workers {
            let chunks = Arc::clone(&chunks);
            handles.push(scope.spawn(move || {
                let mut completed = Vec::new();
                for index in (worker_index..chunks.len()).step_by(workers) {
                    let result = evaluate_chunk(&chunks[index], transforms)?;
                    completed.push((index, result));
                }
                Ok::<_, ParallelError>(completed)
            }));
        }
        for handle in handles {
            for (index, result) in handle.join().map_err(|_| {
                runtime_error(DiagnosticCode::RuntimeGeneral, "parallel worker panicked")
            })?? {
                results[index] = Some(result);
            }
        }
        Ok::<_, ParallelError>(())
    })?;
    Ok(results
        .into_iter()
        .flatten()
        .flatten()
        .map(ParallelValue::into_value)
        .collect())
}
