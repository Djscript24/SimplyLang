//! evaluator.rs — runtime execution engine
//! Evaluates validated Simply programs, manages scopes and functions, and executes control flow and pipelines.
//! Key components: Evaluator, TypeScopes, Flow, and Function.
mod checkpoint;
mod csv;
mod parallel;
mod pattern;

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs,
    io::{self, BufRead, BufReader, IsTerminal, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::Arc,
};

use crate::{
    ast::{
        BinaryOperator, Expr, Literal, PartitionRule, PipelineStep, Program, Stmt, UnaryOperator,
        is_parallel_safe_expression,
    },
    error::{DiagnosticCode, SimplyError, Span},
    lexer::Lexer,
    parser::Parser,
    runtime::{
        arena::{Arena, ArenaRef, Handle},
        collections, files, limits, operations,
        scope::ScopeStack,
        storage::SharedCell,
        value::{
            CsvStreamVersion, EnumValue, FunctionValue, SourceContext, SourceText, StructInstance,
            Value, owned_map_values, owned_values, shared_map, shared_values,
        },
    },
    types::{DeclarationIdentity, DeclarationKind, Type},
};

type ImportCache = SharedCell<HashMap<PathBuf, (Arc<Program>, ArenaRef<SourceText>)>>;

fn enumerate_values(values: impl Iterator<Item = Value>) -> Result<Vec<Value>, String> {
    let mut indexed = Vec::new();
    let mut index = 0i64;
    for value in values {
        indexed
            .try_reserve(1)
            .map_err(|_| "`enumerate` result is too large to allocate".to_owned())?;
        indexed.push(Value::Tuple(shared_values(vec![Value::Int(index), value])));
        index = index
            .checked_add(1)
            .ok_or_else(|| "`enumerate` index exceeds the Int range".to_owned())?;
    }
    Ok(indexed)
}

fn sequence_values(value: Value) -> Result<Box<dyn Iterator<Item = Value>>, String> {
    match value {
        Value::Array(values) | Value::List(values) | Value::Tuple(values) => {
            let mut index = 0usize;
            Ok(Box::new(std::iter::from_fn(move || {
                let value = values.get(index)?.clone();
                index += 1;
                Some(value)
            })))
        }
        Value::Range { start, end, step } => Ok(Box::new(Value::range_values(start, end, step))),
        Value::String(text) => {
            let mut byte_index = 0usize;
            Ok(Box::new(std::iter::from_fn(move || {
                let character = text.get(byte_index..)?.chars().next()?;
                byte_index += character.len_utf8();
                Some(Value::String(character.to_string()))
            })))
        }
        _ => Err("`zip` requires arrays, lists, tuples, ranges, or strings".into()),
    }
}

fn zip_values(
    left: impl Iterator<Item = Value>,
    right: impl Iterator<Item = Value>,
) -> Result<Vec<Value>, String> {
    let mut pairs = Vec::new();
    for (left, right) in left.zip(right) {
        pairs
            .try_reserve(1)
            .map_err(|_| "`zip` result is too large to allocate".to_owned())?;
        pairs.push(Value::Tuple(shared_values(vec![left, right])));
    }
    Ok(pairs)
}

fn clamp_numeric(
    value: Value,
    minimum: Value,
    maximum: Value,
    span: Option<&Span>,
) -> Result<Value, SimplyError> {
    match (value, minimum, maximum) {
        (Value::Int(value), Value::Int(minimum), Value::Int(maximum)) if minimum <= maximum => {
            Ok(Value::Int(value.clamp(minimum, maximum)))
        }

        (value, minimum, maximum) => {
            let (value, minimum, maximum) = match (value, minimum, maximum) {
                (Value::Int(value), Value::Int(minimum), Value::Float(maximum)) => {
                    (value as f64, minimum as f64, maximum)
                }
                (Value::Int(value), Value::Float(minimum), Value::Int(maximum)) => {
                    (value as f64, minimum, maximum as f64)
                }
                (Value::Int(value), Value::Float(minimum), Value::Float(maximum)) => {
                    (value as f64, minimum, maximum)
                }
                (Value::Float(value), Value::Int(minimum), Value::Int(maximum)) => {
                    (value, minimum as f64, maximum as f64)
                }
                (Value::Float(value), Value::Int(minimum), Value::Float(maximum)) => {
                    (value, minimum as f64, maximum)
                }
                (Value::Float(value), Value::Float(minimum), Value::Int(maximum)) => {
                    (value, minimum, maximum as f64)
                }
                (Value::Float(value), Value::Float(minimum), Value::Float(maximum)) => {
                    (value, minimum, maximum)
                }
                _ => {
                    return Err(SimplyError::Runtime {
                        span: span.cloned().unwrap_or_else(|| Span::new(0, 0)),
                        code: DiagnosticCode::RuntimeTypeMismatch,
                        message: "`clamp` requires compatible numeric bounds".into(),
                    });
                }
            };
            if !value.is_finite()
                || !minimum.is_finite()
                || !maximum.is_finite()
                || minimum > maximum
            {
                return Err(SimplyError::Runtime {
                    span: span.cloned().unwrap_or_else(|| Span::new(0, 0)),
                    code: DiagnosticCode::RuntimeArithmetic,
                    message: "`clamp` requires finite values and minimum <= maximum".into(),
                });
            }
            Ok(Value::Float(value.clamp(minimum, maximum)))
        }
    }
}

fn evaluate_math_builtin(
    name: &str,
    arguments: &[Value],
    span: Option<&Span>,
) -> Result<Option<Value>, SimplyError> {
    let require_count = |expected: usize| {
        if arguments.len() == expected {
            Ok(())
        } else {
            Err(SimplyError::Runtime {
                span: span.cloned().unwrap_or_else(|| Span::new(0, 0)),
                code: DiagnosticCode::RuntimeArgument,
                message: format!("`{name}` expects {expected} arguments"),
            })
        }
    };
    let result = match name {
        "sqrt" | "exp" | "log" | "log10" | "sin" | "cos" | "tan" | "floor" | "ceil" => {
            require_count(1)?;
            Some(operations::math_unary(name, arguments[0].clone(), span)?)
        }
        "pow" => {
            require_count(2)?;
            Some(operations::math_pow(
                arguments[0].clone(),
                arguments[1].clone(),
                span,
            )?)
        }
        "sign" => {
            require_count(1)?;
            Some(operations::math_sign(arguments[0].clone(), span)?)
        }
        "vector_add" | "vector_subtract" => {
            require_count(2)?;
            Some(operations::vector_add(
                &arguments[0],
                &arguments[1],
                name == "vector_subtract",
                span,
            )?)
        }
        "vector_scale" => {
            require_count(2)?;
            Some(operations::vector_scale(
                &arguments[0],
                &arguments[1],
                span,
            )?)
        }
        "dot" => {
            require_count(2)?;
            Some(operations::vector_dot(&arguments[0], &arguments[1], span)?)
        }
        "cross" => {
            require_count(2)?;
            Some(operations::vector_cross(
                &arguments[0],
                &arguments[1],
                span,
            )?)
        }
        "norm" => {
            require_count(1)?;
            Some(operations::vector_norm(&arguments[0], span)?)
        }
        "distance" => {
            require_count(2)?;
            Some(operations::vector_distance(
                &arguments[0],
                &arguments[1],
                span,
            )?)
        }
        "normalize" => {
            require_count(1)?;
            Some(operations::vector_normalize(&arguments[0], span)?)
        }
        "shape" => {
            require_count(1)?;
            Some(operations::matrix_shape_value(&arguments[0], span)?)
        }
        "trace" => {
            require_count(1)?;
            Some(operations::matrix_trace(&arguments[0], span)?)
        }
        "rank" => {
            require_count(1)?;
            Some(operations::matrix_rank(&arguments[0], span)?)
        }
        "matvec" => {
            require_count(2)?;
            Some(operations::matrix_vector_multiply(
                &arguments[0],
                &arguments[1],
                span,
            )?)
        }
        "determinant" => {
            require_count(1)?;
            Some(operations::matrix_determinant(&arguments[0], span)?)
        }
        "inverse" => {
            require_count(1)?;
            Some(operations::matrix_inverse(&arguments[0], span)?)
        }
        "lu" => {
            require_count(1)?;
            Some(operations::matrix_lu(&arguments[0], span)?)
        }
        "qr" => {
            require_count(1)?;
            Some(operations::matrix_qr(&arguments[0], span)?)
        }
        "cholesky" => {
            require_count(1)?;
            Some(operations::matrix_cholesky(&arguments[0], span)?)
        }
        "solve" => {
            require_count(2)?;
            Some(operations::matrix_solve(
                &arguments[0],
                &arguments[1],
                span,
            )?)
        }
        "least_squares" => {
            require_count(2)?;
            Some(operations::matrix_least_squares(
                &arguments[0],
                &arguments[1],
                span,
            )?)
        }
        "transpose" => {
            require_count(1)?;
            Some(operations::matrix_transpose_value(&arguments[0], span)?)
        }
        "matrix_add" | "matrix_subtract" => {
            require_count(2)?;
            Some(operations::matrix_add_values(
                &arguments[0],
                &arguments[1],
                name == "matrix_subtract",
                span,
            )?)
        }
        "matrix_scale" => {
            require_count(2)?;
            Some(operations::matrix_scale(
                &arguments[0],
                &arguments[1],
                span,
            )?)
        }
        "multiply" => {
            require_count(2)?;
            Some(operations::matrix_multiply_values(
                &arguments[0],
                &arguments[1],
                span,
            )?)
        }
        "identity" => {
            require_count(1)?;
            let size = match arguments[0] {
                Value::Int(size) if size > 0 => {
                    usize::try_from(size).map_err(|_| SimplyError::Runtime {
                        span: span.cloned().unwrap_or_else(|| Span::new(0, 0)),
                        code: DiagnosticCode::RuntimeLimit,
                        message: "identity matrix size is out of bounds".into(),
                    })?
                }
                _ => {
                    return Err(SimplyError::Runtime {
                        span: span.cloned().unwrap_or_else(|| Span::new(0, 0)),
                        code: DiagnosticCode::RuntimeArgument,
                        message: "identity matrix size must be a positive integer".into(),
                    });
                }
            };
            Some(operations::matrix_identity(size, span)?)
        }
        "mean" | "median" | "variance" | "stddev" | "percentile" => {
            require_count(if name == "percentile" { 2 } else { 1 })?;
            let values = operations::statistics_values(&arguments[0], span)?;
            let percentile = if name == "percentile" {
                Some(match arguments[1] {
                    Value::Int(value) => value as f64,
                    Value::Float(value) if value.is_finite() => value,
                    _ => {
                        return Err(SimplyError::Runtime {
                            span: span.cloned().unwrap_or_else(|| Span::new(0, 0)),
                            code: DiagnosticCode::RuntimeTypeMismatch,
                            message: "`percentile` requires a finite numeric percentile".into(),
                        });
                    }
                })
            } else {
                None
            };
            Some(operations::statistics_unary(
                name, &values, percentile, span,
            )?)
        }
        "covariance" | "correlation" => {
            require_count(2)?;
            let left = operations::statistics_values(&arguments[0], span)?;
            let right = operations::statistics_values(&arguments[1], span)?;
            Some(operations::statistics_pair(name, &left, &right, span)?)
        }
        _ => None,
    };
    Ok(result)
}

fn is_math_builtin(name: &str) -> bool {
    matches!(
        name,
        "sqrt"
            | "pow"
            | "exp"
            | "log"
            | "log10"
            | "sin"
            | "cos"
            | "tan"
            | "floor"
            | "ceil"
            | "sign"
            | "vector_add"
            | "vector_subtract"
            | "vector_scale"
            | "dot"
            | "cross"
            | "norm"
            | "distance"
            | "normalize"
            | "shape"
            | "trace"
            | "rank"
            | "matvec"
            | "determinant"
            | "inverse"
            | "lu"
            | "qr"
            | "cholesky"
            | "solve"
            | "least_squares"
            | "transpose"
            | "matrix_add"
            | "matrix_subtract"
            | "matrix_scale"
            | "multiply"
            | "identity"
            | "mean"
            | "median"
            | "variance"
            | "stddev"
            | "percentile"
            | "covariance"
            | "correlation"
    )
}

fn total_to_f64(value: Value, span: Option<&Span>) -> Result<f64, SimplyError> {
    match value {
        Value::Int(value) => Ok(value as f64),
        Value::Float(value) if value.is_finite() => Ok(value),
        _ => Err(SimplyError::Runtime {
            span: span.cloned().unwrap_or_else(|| Span::new(0, 0)),
            code: DiagnosticCode::RuntimeTypeMismatch,
            message: "aggregate result must be numeric".into(),
        }),
    }
}

struct SumAccumulator {
    total: Value,
    has_values: bool,
    result_type: Type,
}

impl SumAccumulator {
    fn new(result_type: Type) -> Self {
        Self {
            total: Value::Int(0),
            has_values: false,
            result_type,
        }
    }

    fn add(&mut self, value: Value, span: Option<&Span>) -> Result<(), SimplyError> {
        self.total = operations::binary(self.total.clone(), &BinaryOperator::Add, value, span)?;
        self.has_values = true;
        Ok(())
    }

    fn finish(self) -> Value {
        if !self.has_values && self.result_type == Type::Float {
            Value::Float(0.0)
        } else {
            self.total
        }
    }
}

fn empty_partition_categories(rules: &[PartitionRule]) -> BTreeMap<String, Vec<Value>> {
    rules
        .iter()
        .map(|rule| (rule.category.clone(), Vec::new()))
        .collect()
}

fn partition_result(categories: BTreeMap<String, Vec<Value>>) -> Value {
    let categories = categories
        .into_iter()
        .map(|(name, values)| (name, Value::List(shared_values(values))))
        .collect();
    Value::Hash(shared_map(categories))
}

fn update_extreme(
    current: &mut Option<Value>,
    candidate: Value,
    maximum: bool,
    span: Option<&Span>,
) -> Result<(), SimplyError> {
    let replace = match (current.as_ref(), &candidate) {
        (None, _) => true,
        (Some(Value::Int(left)), Value::Int(right)) => {
            if maximum {
                right > left
            } else {
                right < left
            }
        }
        (Some(Value::Float(left)), Value::Float(right)) => {
            if !left.is_finite() || !right.is_finite() {
                return Err(SimplyError::Runtime {
                    span: span.cloned().unwrap_or_else(|| Span::new(0, 0)),
                    code: DiagnosticCode::RuntimeArithmetic,
                    message: "aggregate values must be finite".into(),
                });
            }
            if maximum { right > left } else { right < left }
        }
        (Some(Value::Int(left)), Value::Float(right)) => {
            if maximum {
                *right > *left as f64
            } else {
                *right < *left as f64
            }
        }
        (Some(Value::Float(left)), Value::Int(right)) => {
            if maximum {
                *left < *right as f64
            } else {
                *left > *right as f64
            }
        }
        _ => {
            return Err(SimplyError::Runtime {
                span: span.cloned().unwrap_or_else(|| Span::new(0, 0)),
                code: DiagnosticCode::RuntimeTypeMismatch,
                message: "aggregate values must be numeric".into(),
            });
        }
    };
    if replace {
        *current = Some(candidate);
    }
    Ok(())
}

fn closure_dependencies(
    body: &[Stmt],
    parameters: &[(String, Option<Type>, bool)],
) -> HashSet<String> {
    let mut bound = parameters
        .iter()
        .map(|(name, _, _)| name.clone())
        .collect::<HashSet<_>>();
    let mut dependencies = HashSet::new();
    collect_statement_dependencies(body, &mut bound, &mut dependencies);
    dependencies
}

fn collect_statement_dependencies(
    statements: &[Stmt],
    bound: &mut HashSet<String>,
    dependencies: &mut HashSet<String>,
) {
    for statement in statements {
        let statement = match statement {
            Stmt::Located { statement, .. } => statement.as_ref(),
            statement => statement,
        };
        match statement {
            Stmt::Located { statement, .. } => collect_statement_dependencies(
                std::slice::from_ref(statement.as_ref()),
                bound,
                dependencies,
            ),
            Stmt::Say(expression)
            | Stmt::Sayln(expression)
            | Stmt::Expression(expression)
            | Stmt::Throw(expression)
            | Stmt::Return(expression) => {
                collect_expression_dependencies(expression, bound, dependencies)
            }
            Stmt::Assign { name, value, .. } => {
                collect_expression_dependencies(value, bound, dependencies);
                bound.insert(name.clone());
            }
            Stmt::Reassign { name, value } => {
                collect_expression_dependencies(value, bound, dependencies);
                if !bound.contains(name) {
                    dependencies.insert(name.clone());
                }
            }
            Stmt::DestructureReassign { pattern, value } => {
                collect_expression_dependencies(value, bound, dependencies);
                for name in pattern.destructure_binding_names() {
                    if !bound.contains(name) {
                        dependencies.insert(name.to_owned());
                    }
                }
            }
            Stmt::Flow {
                name,
                source,
                steps,
            } => {
                collect_expression_dependencies(source, bound, dependencies);
                collect_pipeline_step_dependencies(steps, bound, dependencies);
                bound.insert(name.clone());
            }
            Stmt::SetIndex {
                name, index, value, ..
            } => {
                if !bound.contains(name) {
                    dependencies.insert(name.clone());
                }
                collect_expression_dependencies(index, bound, dependencies);
                collect_expression_dependencies(value, bound, dependencies);
            }
            Stmt::Destructure { pattern, value, .. } => {
                collect_expression_dependencies(value, bound, dependencies);
                bound.extend(
                    pattern
                        .destructure_binding_names()
                        .into_iter()
                        .map(str::to_owned),
                );
            }
            Stmt::CollectionOp { name, value, .. } => {
                if !bound.contains(name) {
                    dependencies.insert(name.clone());
                }
                collect_expression_dependencies(value, bound, dependencies)
            }
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                collect_expression_dependencies(condition, bound, dependencies);
                let mut then_bound = bound.clone();
                collect_statement_dependencies(then_branch, &mut then_bound, dependencies);
                let mut else_bound = bound.clone();
                collect_statement_dependencies(else_branch, &mut else_bound, dependencies);
            }
            Stmt::Try {
                try_body,
                catches,
                finally_body,
            } => {
                let mut try_bound = bound.clone();
                collect_statement_dependencies(try_body, &mut try_bound, dependencies);
                for catch in catches {
                    let mut catch_bound = bound.clone();
                    if let Some(binding) = &catch.binding {
                        catch_bound.insert(binding.clone());
                    }
                    collect_statement_dependencies(&catch.body, &mut catch_bound, dependencies);
                }
                let mut finally_bound = bound.clone();
                collect_statement_dependencies(finally_body, &mut finally_bound, dependencies);
            }
            Stmt::Function {
                parameters, body, ..
            } => {
                let nested = closure_dependencies(body, parameters);
                dependencies.extend(nested.into_iter().filter(|name| !bound.contains(name)));
                if let Stmt::Function { name, .. } = statement {
                    bound.insert(name.clone());
                }
            }
            Stmt::For {
                name,
                iterable,
                body,
                ..
            } => {
                collect_expression_dependencies(iterable, bound, dependencies);
                let mut body_bound = bound.clone();
                body_bound.insert(name.clone());
                collect_statement_dependencies(body, &mut body_bound, dependencies);
            }
            Stmt::While { condition, body } => {
                collect_expression_dependencies(condition, bound, dependencies);
                collect_statement_dependencies(body, bound, dependencies);
            }
            Stmt::Import { .. }
            | Stmt::Export { .. }
            | Stmt::Struct { .. }
            | Stmt::Enum { .. }
            | Stmt::Message { .. }
            | Stmt::Break
            | Stmt::Continue => {}
        }
    }
}

fn collect_expression_dependencies(
    expression: &Expr,
    bound: &HashSet<String>,
    dependencies: &mut HashSet<String>,
) {
    match expression {
        Expr::Identifier(name) => {
            if !bound.contains(name) {
                dependencies.insert(name.clone());
            }
        }
        Expr::Unary { operand, .. } => {
            collect_expression_dependencies(operand, bound, dependencies)
        }
        Expr::Binary { left, right, .. } => {
            collect_expression_dependencies(left, bound, dependencies);
            collect_expression_dependencies(right, bound, dependencies);
        }
        Expr::Call { name, arguments } => {
            if !bound.contains(name) {
                dependencies.insert(name.clone());
            }
            for argument in arguments {
                collect_expression_dependencies(argument, bound, dependencies);
            }
        }
        Expr::MessageDispatch {
            receiver,
            message,
            arguments,
        } => {
            collect_expression_dependencies(receiver, bound, dependencies);
            if !bound.contains(message) {
                dependencies.insert(message.clone());
            }
            for argument in arguments {
                collect_expression_dependencies(argument, bound, dependencies);
            }
        }
        Expr::EnumVariant {
            enum_name,
            arguments,
            ..
        } => {
            if !bound.contains(enum_name) {
                dependencies.insert(enum_name.clone());
            }
            for argument in arguments {
                collect_expression_dependencies(argument, bound, dependencies);
            }
        }
        Expr::Match { value, arms } => {
            collect_expression_dependencies(value, bound, dependencies);
            for arm in arms {
                let mut arm_bound = bound.clone();
                collect_pattern_bindings(&arm.pattern, &mut arm_bound);
                if let Some(guard) = &arm.guard {
                    collect_expression_dependencies(guard, &arm_bound, dependencies);
                }
                collect_statement_dependencies(&arm.body, &mut arm_bound, dependencies);
                if let Some(result) = &arm.result {
                    collect_expression_dependencies(result, &arm_bound, dependencies);
                }
            }

            fn collect_pattern_bindings(
                pattern: &crate::ast::MatchPattern,
                bindings: &mut HashSet<String>,
            ) {
                match pattern {
                    crate::ast::MatchPattern::Literal(_)
                    | crate::ast::MatchPattern::Range { .. } => {}
                    crate::ast::MatchPattern::Or(alternatives) => {
                        for alternative in alternatives {
                            collect_pattern_bindings(alternative, bindings);
                        }
                    }
                    crate::ast::MatchPattern::Identifier(name) => {
                        bindings.insert(name.clone());
                    }
                    crate::ast::MatchPattern::Alias { name, pattern } => {
                        bindings.insert(name.clone());
                        collect_pattern_bindings(pattern, bindings);
                    }
                    crate::ast::MatchPattern::Tuple(patterns) => {
                        for pattern in patterns {
                            collect_pattern_bindings(pattern, bindings);
                        }
                    }
                    crate::ast::MatchPattern::Sequence { patterns, rest } => {
                        for pattern in patterns {
                            collect_pattern_bindings(pattern, bindings);
                        }
                        if let Some(rest) = rest {
                            bindings.insert(rest.clone());
                        }
                    }
                    crate::ast::MatchPattern::EnumVariant {
                        payload: Some(payload),
                        ..
                    } => collect_pattern_bindings(payload, bindings),
                    crate::ast::MatchPattern::Struct { fields, .. } => {
                        for pattern in fields {
                            collect_pattern_bindings(pattern, bindings);
                        }
                    }
                    crate::ast::MatchPattern::NamedStruct { fields, .. } => {
                        for (_, pattern) in fields {
                            collect_pattern_bindings(pattern, bindings);
                        }
                    }
                    crate::ast::MatchPattern::Hash(entries) => {
                        for (_, pattern) in entries {
                            collect_pattern_bindings(pattern, bindings);
                        }
                    }
                    crate::ast::MatchPattern::EnumVariant { payload: None, .. }
                    | crate::ast::MatchPattern::Wildcard => {}
                }
            }
        }
        Expr::Array(values) | Expr::List(values) | Expr::Tuple(values) | Expr::Matrix(values) => {
            for value in values {
                collect_expression_dependencies(value, bound, dependencies);
            }
        }
        Expr::Index { target, index } => {
            collect_expression_dependencies(target, bound, dependencies);
            collect_expression_dependencies(index, bound, dependencies);
        }
        Expr::Field { target, .. } => collect_expression_dependencies(target, bound, dependencies),
        Expr::Hash(entries) | Expr::Tree(entries) => {
            for (_, value) in entries {
                collect_expression_dependencies(value, bound, dependencies);
            }
        }
        Expr::Pipeline { source, steps } => {
            collect_expression_dependencies(source, bound, dependencies);
            collect_pipeline_step_dependencies(steps, bound, dependencies);
        }
        Expr::Literal(_) => {}
    }
}

fn collect_pipeline_step_dependencies(
    steps: &[PipelineStep],
    bound: &HashSet<String>,
    dependencies: &mut HashSet<String>,
) {
    for step in steps {
        match step {
            PipelineStep::Where(expression)
            | PipelineStep::Derive(expression)
            | PipelineStep::TakeWhile(expression)
            | PipelineStep::DropWhile(expression) => {
                let mut item_bound = bound.clone();
                item_bound.insert("item".into());
                collect_expression_dependencies(expression, &item_bound, dependencies);
            }
            PipelineStep::Partition { item, rules } => {
                let mut item_bound = bound.clone();
                item_bound.insert(item.clone());
                for condition in rules.iter().filter_map(|rule| rule.condition.as_ref()) {
                    collect_expression_dependencies(condition, &item_bound, dependencies);
                }
            }
            PipelineStep::WriteCsv(expression) | PipelineStep::Checkpoint(expression) => {
                collect_expression_dependencies(expression, bound, dependencies);
            }
            PipelineStep::Sum
            | PipelineStep::Count
            | PipelineStep::Average
            | PipelineStep::Min
            | PipelineStep::Max
            | PipelineStep::Any
            | PipelineStep::All
            | PipelineStep::Chunk(_)
            | PipelineStep::Parallel(_)
            | PipelineStep::Take(_)
            | PipelineStep::Skip(_)
            | PipelineStep::StepBy(_)
            | PipelineStep::Distinct => {}
        }
    }
}

#[derive(Clone)]
struct StructDefinition {
    identity: DeclarationIdentity,
    fields: Vec<crate::ast::StructField>,
}

#[derive(Clone)]
struct EnumDefinition {
    identity: DeclarationIdentity,
    variants: Vec<crate::ast::EnumVariant>,
}

#[derive(Clone)]
enum ExportedRuntimeType {
    Struct {
        definition: StructDefinition,
        messages: HashMap<(DeclarationIdentity, String), MessageBehavior>,
    },
    Enum(EnumDefinition),
}

type ImportedModule = (
    Value,
    HashMap<String, Value>,
    HashMap<String, ExportedRuntimeType>,
);

#[derive(Clone)]
struct MessageBehavior {
    function: Handle<FunctionValue>,
    field_count: usize,
}

struct ActiveMessageState {
    instance: ArenaRef<StructInstance>,
    scope_index: usize,
}

#[derive(Default)]
pub struct Evaluator {
    scopes: ScopeStack,
    variable_types: TypeScopes,
    function_values: SharedCell<Arena<FunctionValue>>,
    struct_values: SharedCell<Arena<StructInstance>>,
    enum_values: SharedCell<Arena<EnumValue>>,
    source_values: SharedCell<Arena<SourceText>>,
    function_arena: Arena<Function>,
    function_scopes: Vec<HashMap<String, Handle<Function>>>,
    struct_scopes: Vec<HashMap<String, StructDefinition>>,
    enum_scopes: Vec<HashMap<String, EnumDefinition>>,
    message_scopes: Vec<HashMap<(DeclarationIdentity, String), MessageBehavior>>,
    hoisted_functions: HashSet<(usize, String)>,
    active_message_states: Vec<ActiveMessageState>,
    current_span: Option<Span>,
    current_file: Option<PathBuf>,
    module_identity: String,
    current_source: Option<ArenaRef<SourceText>>,
    import_stack: Vec<PathBuf>,
    module_is_imported: bool,
    import_cache: ImportCache,
    output_enabled: bool,
    call_depth: usize,
}

struct TypeScopes {
    scopes: Vec<HashMap<String, Type>>,
    bindings: HashMap<String, Vec<usize>>,
    mutability: HashMap<String, Vec<bool>>,
    reusable_scopes: Vec<HashMap<String, Type>>,
}

impl TypeScopes {
    fn new() -> Self {
        Self {
            scopes: vec![HashMap::new()],
            bindings: HashMap::new(),
            mutability: HashMap::new(),
            reusable_scopes: Vec::new(),
        }
    }

    fn define(&mut self, name: String, typ: Type, mutable: bool) {
        let current = self
            .scopes
            .last_mut()
            .expect("type scope stack always has a global scope");
        let inserted_name = match current.entry(name) {
            std::collections::hash_map::Entry::Occupied(mut entry) => {
                entry.insert(typ);
                None
            }
            std::collections::hash_map::Entry::Vacant(entry) => {
                let name = entry.key().clone();
                entry.insert(typ);
                Some(name)
            }
        };
        if let Some(name) = inserted_name {
            self.bindings
                .entry(name.clone())
                .or_default()
                .push(self.scopes.len() - 1);
            self.mutability.entry(name).or_default().push(mutable);
        }
    }

    fn lookup(&self, name: &str) -> Option<&Type> {
        if let Some(typ) = self.scopes.last().and_then(|scope| scope.get(name)) {
            return Some(typ);
        }
        let scope_index = self.bindings.get(name)?.last().copied()?;
        self.scopes.get(scope_index)?.get(name)
    }

    fn replace_visible(&mut self, name: &str, typ: Type) {
        let Some(scope_index) = self
            .bindings
            .get(name)
            .and_then(|indices| indices.last())
            .copied()
        else {
            return;
        };
        if let Some(scope) = self.scopes.get_mut(scope_index) {
            scope.insert(name.to_owned(), typ);
        }
    }

    fn push(&mut self) {
        self.scopes
            .push(self.reusable_scopes.pop().unwrap_or_default());
    }

    fn pop(&mut self) {
        if self.scopes.len() > 1 {
            let mut scope = self.scopes.pop().expect("scope exists after length check");
            for name in scope.keys() {
                if let Some(scope_indices) = self.bindings.get_mut(name) {
                    scope_indices.pop();
                    if scope_indices.is_empty() {
                        self.bindings.remove(name);
                    }
                    if let Some(values) = self.mutability.get_mut(name) {
                        values.pop();
                        if values.is_empty() {
                            self.mutability.remove(name);
                        }
                    }
                }
            }
            scope.clear();
            self.reusable_scopes.push(scope);
        }
    }
}

impl Default for TypeScopes {
    fn default() -> Self {
        Self::new()
    }
}

impl Evaluator {
    fn identity(&self, name: &str, kind: DeclarationKind) -> DeclarationIdentity {
        DeclarationIdentity::new(self.module_identity.clone(), name, kind)
    }

    fn resolve_type_identity(&self, typ: &Type) -> Type {
        match typ {
            Type::Struct(identity) if identity.is_unresolved() => {
                Type::Struct(self.identity(&identity.local_name, DeclarationKind::Struct))
            }
            Type::Enum(identity) if identity.is_unresolved() => {
                Type::Enum(self.identity(&identity.local_name, DeclarationKind::Enum))
            }
            Type::Array(element) => Type::Array(Box::new(self.resolve_type_identity(element))),
            Type::List(element) => Type::List(Box::new(self.resolve_type_identity(element))),
            Type::Vector(element, length) => {
                Type::Vector(Box::new(self.resolve_type_identity(element)), *length)
            }
            Type::TypedMatrix(element, rows, columns) => Type::TypedMatrix(
                Box::new(self.resolve_type_identity(element)),
                *rows,
                *columns,
            ),
            Type::Tuple(elements) => Type::Tuple(
                elements
                    .iter()
                    .map(|element| self.resolve_type_identity(element))
                    .collect(),
            ),
            Type::HashValues(element) => {
                Type::HashValues(Box::new(self.resolve_type_identity(element)))
            }
            Type::TreeValues(element) => {
                Type::TreeValues(Box::new(self.resolve_type_identity(element)))
            }
            Type::Function {
                parameters,
                return_type,
            } => Type::Function {
                parameters: parameters
                    .iter()
                    .map(|parameter| {
                        parameter
                            .as_ref()
                            .map(|typ| Box::new(self.resolve_type_identity(typ)))
                    })
                    .collect(),
                return_type: return_type
                    .as_ref()
                    .map(|typ| Box::new(self.resolve_type_identity(typ))),
            },
            _ => typ.clone(),
        }
    }

    fn track_function(&mut self, function: FunctionValue) -> Handle<FunctionValue> {
        self.function_values.borrow_mut().insert(function)
    }

    fn tracked_function(
        &self,
        handle: Handle<FunctionValue>,
    ) -> Result<FunctionValue, SimplyError> {
        self.function_values
            .borrow()
            .get(handle)
            .cloned()
            .ok_or_else(|| {
                self.runtime_error_with_code(
                    DiagnosticCode::RuntimeName,
                    "function handle is no longer valid",
                )
            })
    }

    fn tracked_struct(
        &self,
        instance: &ArenaRef<StructInstance>,
    ) -> Result<StructInstance, SimplyError> {
        instance.get_cloned().ok_or_else(|| {
            self.runtime_error_with_code(
                DiagnosticCode::RuntimeName,
                "struct handle is no longer valid",
            )
        })
    }

    fn tracked_enum(&self, value: &ArenaRef<EnumValue>) -> Result<EnumValue, SimplyError> {
        value.get_cloned().ok_or_else(|| {
            self.runtime_error_with_code(
                DiagnosticCode::RuntimeName,
                "enum handle is no longer valid",
            )
        })
    }

    fn define(&mut self, name: String, value: Value) -> Result<(), String> {
        self.scopes.define(name, value, false)
    }

    fn lookup(&self, name: &str) -> Option<&Value> {
        self.scopes.lookup(name)
    }

    fn lookup_mut(&mut self, name: &str) -> Option<&mut Value> {
        self.scopes.lookup_mut(name)
    }

    fn contains(&self, name: &str) -> bool {
        self.scopes.contains(name)
    }

    fn assign(&mut self, name: &str, value: Value) -> bool {
        let binding_scope = self.scopes.binding_scope(name);
        if !self.scopes.assign(name, value.clone()) {
            return false;
        }
        if let Some(instance) = self
            .active_message_states
            .iter()
            .rev()
            .find(|state| {
                Some(state.scope_index) == binding_scope
                    && state
                        .instance
                        .with(|instance| instance.fields.contains_key(name))
                        .unwrap_or(false)
            })
            .map(|state| state.instance.clone())
        {
            instance.with_mut(|instance| {
                instance.fields.insert(name.to_owned(), value);
            });
        }
        true
    }

    fn assign_many(&mut self, bindings: Vec<(String, Value)>) -> Result<(), SimplyError> {
        let mut resolved = Vec::with_capacity(bindings.len());
        for (name, value) in bindings {
            let scope_index = self.scopes.binding_scope(&name).ok_or_else(|| {
                self.runtime_error_with_code(
                    DiagnosticCode::RuntimeName,
                    format!("cannot reassign unknown variable `{name}`"),
                )
            })?;
            if !self.scopes.is_mutable(&name) {
                return Err(self.runtime_error_with_code(
                    DiagnosticCode::InvalidReassignment,
                    format!("cannot reassign immutable variable `{name}`; declare it with `mut`"),
                ));
            }
            if let Some(expected) = self.variable_types.lookup(&name).cloned() {
                self.ensure_reassignment_type(&value, &expected, &name)?;
            }
            resolved.push((scope_index, name, value));
        }

        let names = resolved
            .iter()
            .map(|(_, name, _)| name.clone())
            .collect::<Vec<_>>();
        self.scopes.assign_many(resolved).map_err(|error| {
            self.runtime_error_with_code(DiagnosticCode::InvalidReassignment, error)
        })?;
        for name in names {
            self.persist_active_message_field(&name);
        }
        Ok(())
    }

    fn persist_active_message_field(&self, name: &str) {
        let binding_scope = self.scopes.binding_scope(name);
        let Some(instance) = self
            .active_message_states
            .iter()
            .rev()
            .find(|state| {
                Some(state.scope_index) == binding_scope
                    && state
                        .instance
                        .with(|instance| instance.fields.contains_key(name))
                        .unwrap_or(false)
            })
            .map(|state| state.instance.clone())
        else {
            return;
        };
        if let Some(value) = self.lookup(name).cloned() {
            instance.with_mut(|instance| {
                instance.fields.insert(name.to_owned(), value);
            });
        }
    }

    fn remove_current(&mut self, name: &str) -> Option<Value> {
        self.scopes.remove_current(name)
    }

    fn push_scope(&mut self) {
        self.scopes.push();
        self.variable_types.push();
        self.function_scopes.push(HashMap::new());
        self.struct_scopes.push(HashMap::new());
        self.enum_scopes.push(HashMap::new());
        self.message_scopes.push(HashMap::new());
    }

    fn pop_scope(&mut self) {
        self.scopes.pop();
        self.variable_types.pop();
        if self.function_scopes.len() > 1
            && let Some(functions) = self.function_scopes.pop()
        {
            for handle in functions.into_values() {
                self.function_arena.remove(handle);
            }
        }
        if self.struct_scopes.len() > 1 {
            self.struct_scopes.pop();
        }
        if self.enum_scopes.len() > 1 {
            self.enum_scopes.pop();
        }
        if self.message_scopes.len() > 1 {
            self.message_scopes.pop();
        }
    }
}

fn print_value(value: &Value, newline: bool) {
    let rendered = value.display();
    if io::stdout().is_terminal() {
        if newline {
            println!("\x1b[36m{rendered}\x1b[0m");
        } else {
            print!("\x1b[36m{rendered}\x1b[0m");
        }
    } else if newline {
        println!("{rendered}");
    } else {
        print!("{rendered}");
    }
}

enum Flow {
    None,
    Return(Value),
    Break,
    Continue,
}

#[derive(Clone)]
struct Function {
    parameters: Vec<(String, Option<Type>, bool)>,
    return_type: Option<Type>,
    body: Arc<[Stmt]>,
    source: Option<SourceContext>,
}

mod declarations;
mod execution;
mod expressions;
mod imports;
mod objects;
mod pipelines;
mod types;

#[cfg(test)]
#[path = "../tests/internal/evaluator.rs"]
mod tests;
