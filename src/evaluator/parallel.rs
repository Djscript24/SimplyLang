use std::sync::Arc;

use crate::{
    ast::{BinaryOperator, Expr, Literal, PipelineStep},
    runtime::{limits, operations, value::Value},
};

#[derive(Clone)]
enum ParallelValue {
    String(String),
    Int(i64),
    Float(f64),
    Bool(bool),
}

#[cfg(test)]
pub(super) static PARALLEL_THREAD_IDS: std::sync::LazyLock<
    std::sync::Mutex<std::collections::HashSet<std::thread::ThreadId>>,
> = std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashSet::new()));

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

fn evaluate_expression(expression: &Expr, item: &ParallelValue) -> Result<ParallelValue, String> {
    let value = match expression {
        Expr::Literal(Literal::String(value)) => Value::String(value.clone()),
        Expr::Literal(Literal::Int(value)) => Value::Int(*value),
        Expr::Literal(Literal::Float(value)) => Value::Float(*value),
        Expr::Literal(Literal::Bool(value)) => Value::Bool(*value),
        Expr::Identifier(name) if name == "item" => item.as_value(),
        Expr::Unary { operator, operand } => {
            let operand = evaluate_expression(operand, item)?.into_value();
            operations::unary(operand, operator, None).map_err(|error| error.to_string())?
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
            operations::binary(left, operator, right, None).map_err(|error| error.to_string())?
        }
        _ => return Err("expression is outside the parallel-safe subset".into()),
    };
    ParallelValue::from_value(&value)
        .ok_or_else(|| "parallel expressions must produce scalar values".into())
}

fn evaluate_chunk(
    input: &[ParallelValue],
    transforms: &[PipelineStep],
) -> Result<Vec<ParallelValue>, String> {
    let mut output = Vec::with_capacity(input.len());
    for input in input {
        let mut current = Some(input.clone());
        for step in transforms {
            if current.is_none() {
                break;
            }
            match step {
                PipelineStep::Where(expression) => {
                    let item = current
                        .as_ref()
                        .ok_or_else(|| "pipeline item was lost".to_string())?;
                    let result = evaluate_expression(expression, item)?;
                    match result {
                        ParallelValue::Bool(true) => {}
                        ParallelValue::Bool(false) => current = None,
                        _ => return Err("pipeline `where` condition must return a boolean".into()),
                    }
                }
                PipelineStep::Derive(expression) => {
                    let item = current
                        .as_ref()
                        .ok_or_else(|| "pipeline item was lost".to_string())?;
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
) -> Result<Vec<Value>, String> {
    if requested_workers == 0 || requested_workers > limits::MAX_PARALLEL_WORKERS {
        return Err(format!(
            "parallel worker count must be between 1 and {}",
            limits::MAX_PARALLEL_WORKERS
        ));
    }
    let input: Vec<ParallelValue> = values
        .iter()
        .map(|value| {
            ParallelValue::from_value(value).ok_or_else(|| {
                "values or expressions are outside the parallel-safe subset".to_string()
            })
        })
        .collect::<Result<_, _>>()?;
    let chunk_size =
        requested_chunk_size.unwrap_or_else(|| input.len().div_ceil(requested_workers).max(1));
    if chunk_size == 0 || chunk_size > limits::MAX_CHUNK_SIZE {
        return Err(format!(
            "chunk size must be between 1 and {}",
            limits::MAX_CHUNK_SIZE
        ));
    }
    let chunk_count = input.len().div_ceil(chunk_size);
    if chunk_count > limits::MAX_PARALLEL_CHUNKS {
        return Err(format!(
            "parallel chunk count exceeds the limit of {}",
            limits::MAX_PARALLEL_CHUNKS
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
                #[cfg(test)]
                PARALLEL_THREAD_IDS
                    .lock()
                    .expect("parallel test lock")
                    .insert(std::thread::current().id());
                let mut completed = Vec::new();
                for index in (worker_index..chunks.len()).step_by(workers) {
                    let result = evaluate_chunk(&chunks[index], transforms)?;
                    completed.push((index, result));
                }
                Ok::<_, String>(completed)
            }));
        }
        for handle in handles {
            for (index, result) in handle.join().map_err(|_| "parallel worker panicked")?? {
                results[index] = Some(result);
            }
        }
        Ok::<_, String>(())
    })?;
    Ok(results
        .into_iter()
        .flatten()
        .flatten()
        .map(ParallelValue::into_value)
        .collect())
}
