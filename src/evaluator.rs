//! evaluator.rs — runtime execution engine
//! Evaluates validated Simply programs, manages scopes and functions, and executes control flow and pipelines.
//! Key components: Evaluator, TypeScopes, Flow, and Function.
mod checkpoint;
mod csv;
mod parallel;
mod pattern;

use std::{
    cell::RefCell,
    collections::{BTreeMap, HashMap, HashSet},
    fs,
    io::{self, BufRead, BufReader, IsTerminal, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    rc::Rc,
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
        collections, files, limits, operations,
        scope::ScopeStack,
        value::{
            CsvStreamVersion, EnumValue, FunctionValue, SourceContext, StructInstance, Value,
            owned_map_values, owned_values, shared_map, shared_values,
        },
    },
    types::{DeclarationIdentity, DeclarationKind, Type},
};

type ImportCache = Rc<RefCell<HashMap<PathBuf, (Arc<Program>, Rc<str>)>>>;

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
            | "norm"
            | "distance"
            | "normalize"
            | "shape"
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
    function: Rc<FunctionValue>,
    field_count: usize,
}

struct ActiveMessageState {
    instance: Rc<StructInstance>,
    scope_index: usize,
}

#[derive(Default)]
pub struct Evaluator {
    scopes: ScopeStack,
    variable_types: TypeScopes,
    function_scopes: Vec<HashMap<String, Rc<Function>>>,
    struct_scopes: Vec<HashMap<String, StructDefinition>>,
    enum_scopes: Vec<HashMap<String, EnumDefinition>>,
    message_scopes: Vec<HashMap<(DeclarationIdentity, String), MessageBehavior>>,
    hoisted_functions: HashSet<(usize, String)>,
    active_message_states: Vec<ActiveMessageState>,
    current_span: Option<Span>,
    current_file: Option<PathBuf>,
    module_identity: String,
    current_source: Option<Rc<str>>,
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

    fn track_function(&mut self, function: Rc<FunctionValue>) -> Rc<FunctionValue> {
        function
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
                    && state.instance.fields.borrow().contains_key(name)
            })
            .map(|state| state.instance.clone())
        {
            instance.fields.borrow_mut().insert(name.to_owned(), value);
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
                    && state.instance.fields.borrow().contains_key(name)
            })
            .map(|state| state.instance.clone())
        else {
            return;
        };
        if let Some(value) = self.lookup(name).cloned() {
            instance.fields.borrow_mut().insert(name.to_owned(), value);
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
        if self.function_scopes.len() > 1 {
            self.function_scopes.pop();
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

impl Evaluator {
    pub fn new() -> Self {
        Self {
            scopes: ScopeStack::new(),
            variable_types: TypeScopes::new(),
            function_scopes: vec![HashMap::new()],
            struct_scopes: vec![HashMap::new()],
            enum_scopes: vec![HashMap::new()],
            message_scopes: vec![HashMap::new()],
            module_identity: "memory://root".into(),
            module_is_imported: false,
            output_enabled: true,
            ..Self::default()
        }
    }

    pub fn run(&mut self, program: &Program) -> Result<(), SimplyError> {
        self.run_with_output(program, true)
    }

    fn register_declarations(&mut self, statements: &[Stmt]) -> Result<(), SimplyError> {
        for statement in statements.iter().map(|statement| match statement {
            Stmt::Located { statement, .. } => statement.as_ref(),
            statement => statement,
        }) {
            if let Stmt::Enum { name, variants } = statement {
                let existing_type = self.lookup_struct(name).is_some()
                    || self
                        .enum_scopes
                        .iter()
                        .any(|scope| scope.contains_key(name));
                if existing_type {
                    return Err(self.runtime_error_with_code(
                        DiagnosticCode::DuplicateDeclaration,
                        format!("type `{name}` is already declared"),
                    ));
                }
                let identity = self.identity(name, DeclarationKind::Enum);
                let resolved_variants = variants
                    .iter()
                    .cloned()
                    .map(|mut variant| {
                        variant.payload_type = variant
                            .payload_type
                            .as_ref()
                            .map(|typ| self.resolve_type_identity(typ));
                        variant
                    })
                    .collect();
                self.enum_scopes
                    .last_mut()
                    .expect("enum scope stack always has a scope")
                    .insert(
                        name.clone(),
                        EnumDefinition {
                            identity,
                            variants: resolved_variants,
                        },
                    );
            }
        }
        for statement in statements.iter().map(|statement| match statement {
            Stmt::Located { statement, .. } => statement.as_ref(),
            statement => statement,
        }) {
            if let Stmt::Struct { name, fields } = statement {
                if self
                    .struct_scopes
                    .last()
                    .is_some_and(|scope| scope.contains_key(name))
                {
                    return Err(self.runtime_error_with_code(
                        DiagnosticCode::DuplicateDeclaration,
                        format!("type `{name}` is already declared"),
                    ));
                }
                if self
                    .enum_scopes
                    .iter()
                    .any(|scope| scope.contains_key(name))
                {
                    return Err(self.runtime_error_with_code(
                        DiagnosticCode::DuplicateDeclaration,
                        format!("type `{name}` is already declared"),
                    ));
                }
                let identity = self.identity(name, DeclarationKind::Struct);
                let resolved_fields = fields
                    .iter()
                    .cloned()
                    .map(|mut field| {
                        field.field_type = self.resolve_type_identity(&field.field_type);
                        field
                    })
                    .collect();
                self.struct_scopes
                    .last_mut()
                    .expect("struct scope stack always has a scope")
                    .insert(
                        name.clone(),
                        StructDefinition {
                            identity,
                            fields: resolved_fields,
                        },
                    );
            }
        }
        for statement in statements.iter().map(|statement| match statement {
            Stmt::Located { statement, .. } => statement.as_ref(),
            statement => statement,
        }) {
            if let Stmt::Message {
                receiver_type,
                name,
                parameters,
                body,
            } = statement
            {
                let definition = self.lookup_struct(receiver_type).cloned().ok_or_else(|| {
                    self.runtime_error_with_code(
                        DiagnosticCode::RuntimeMessage,
                        format!("unknown receiver type `{receiver_type}` for message `{name}`"),
                    )
                })?;
                let key = (definition.identity.clone(), name.clone());
                if self
                    .message_scopes
                    .last()
                    .is_some_and(|scope| scope.contains_key(&key))
                {
                    return Err(self.runtime_error_with_code(
                        DiagnosticCode::DuplicateDeclaration,
                        format!("message `{name}` is already defined for `{receiver_type}`"),
                    ));
                }
                let mut function_parameters = definition
                    .fields
                    .iter()
                    .map(|field| (field.name.clone(), Some(field.field_type.clone()), true))
                    .collect::<Vec<_>>();
                function_parameters.extend(parameters.iter().map(|(parameter, typ, mutable)| {
                    (
                        parameter.clone(),
                        typ.as_ref().map(|typ| self.resolve_type_identity(typ)),
                        *mutable,
                    )
                }));
                let function = Rc::new(FunctionValue {
                    name: None,
                    parameters: function_parameters,
                    return_type: None,
                    body: Arc::clone(body),
                    captures: RefCell::new(HashMap::new()),
                    source: self.function_source_context(),
                });
                self.message_scopes
                    .last_mut()
                    .expect("message scope stack always has a scope")
                    .insert(
                        key,
                        MessageBehavior {
                            function,
                            field_count: definition.fields.len(),
                        },
                    );
            }
        }
        Ok(())
    }

    fn lookup_struct(&self, name: &str) -> Option<&StructDefinition> {
        self.struct_scopes
            .iter()
            .rev()
            .find_map(|scope| scope.get(name))
    }

    fn lookup_enum(&self, name: &str) -> Option<&EnumDefinition> {
        self.enum_scopes
            .iter()
            .rev()
            .find_map(|scope| scope.get(name))
    }

    fn lookup_struct_identity(&self, identity: &DeclarationIdentity) -> Option<&StructDefinition> {
        self.struct_scopes
            .iter()
            .rev()
            .flat_map(|scope| scope.values())
            .find(|definition| &definition.identity == identity)
    }

    pub(crate) fn enum_names(&self) -> impl Iterator<Item = String> + '_ {
        self.enum_scopes
            .iter()
            .flat_map(|scope| scope.keys().cloned())
    }

    fn lookup_message(
        &self,
        type_identity: &DeclarationIdentity,
        message: &str,
    ) -> Option<&MessageBehavior> {
        let key = (type_identity.clone(), message.to_owned());
        self.message_scopes
            .iter()
            .rev()
            .find_map(|scope| scope.get(&key))
    }

    pub fn run_silent_resolved(
        &mut self,
        program: &Program,
        resolved: &Path,
    ) -> Result<(), SimplyError> {
        self.current_file = Some(resolved.to_path_buf());
        self.module_identity = resolved.display().to_string();
        self.import_stack = vec![resolved.to_path_buf()];
        self.module_is_imported = false;
        self.run_with_output(program, false)
    }

    fn run_with_output(
        &mut self,
        program: &Program,
        output_enabled: bool,
    ) -> Result<(), SimplyError> {
        self.output_enabled = output_enabled;
        self.register_declarations(&program.statements)?;
        self.register_top_level_functions(&program.statements)?;
        match self.execute_statements(&program.statements)? {
            Flow::None => Ok(()),
            _ => Err(self.runtime_error_with_code(
                DiagnosticCode::RuntimeControl,
                "control statement is outside its valid context",
            )),
        }
    }

    pub fn run_repl(&mut self, program: &Program) -> Result<(), SimplyError> {
        self.register_declarations(&program.statements)?;
        for statement in &program.statements {
            let (span, statement) = match statement {
                Stmt::Located { span, statement } => (Some(span.clone()), statement.as_ref()),
                statement => (None, statement),
            };
            self.current_span = span;
            if let Stmt::Expression(expression) = statement {
                let value = self.evaluate(expression)?;
                if !matches!(value, Value::Unit) {
                    print_value(&value, true);
                }
                continue;
            }
            match self.execute_statements(std::slice::from_ref(statement))? {
                Flow::None => {}
                _ => {
                    return Err(self.runtime_error_with_code(
                        DiagnosticCode::RuntimeControl,
                        "control statement is outside its valid context",
                    ));
                }
            }
        }
        Ok(())
    }

    fn register_top_level_functions(&mut self, statements: &[Stmt]) -> Result<(), SimplyError> {
        if self.import_stack.len() > 1 || self.function_scopes.len() != 1 {
            return Ok(());
        }
        for statement in statements.iter().map(|statement| match statement {
            Stmt::Located { statement, .. } => statement.as_ref(),
            statement => statement,
        }) {
            let Stmt::Function {
                name,
                parameters,
                return_type,
                body,
            } = statement
            else {
                continue;
            };
            if self.scopes.has_in_current_scope(name)
                || self
                    .function_scopes
                    .last()
                    .is_some_and(|scope| scope.contains_key(name))
            {
                return Err(self.runtime_error_with_code(
                    DiagnosticCode::DuplicateDeclaration,
                    format!("function `{name}` is already declared in this scope"),
                ));
            }
            let source = self.function_source_context();
            let resolved_parameters = parameters
                .iter()
                .map(|(name, typ, mutable)| {
                    (
                        name.clone(),
                        typ.as_ref().map(|typ| self.resolve_type_identity(typ)),
                        *mutable,
                    )
                })
                .collect::<Vec<_>>();
            let resolved_return_type = return_type
                .as_ref()
                .map(|typ| self.resolve_type_identity(typ));
            let function_value = self.track_function(Rc::new(FunctionValue {
                name: Some(name.clone()),
                parameters: resolved_parameters.clone(),
                return_type: resolved_return_type.clone(),
                body: Arc::clone(body),
                captures: RefCell::new(HashMap::new()),
                source: source.clone(),
            }));
            self.function_scopes
                .last_mut()
                .expect("function scope stack always has a global scope")
                .insert(
                    name.clone(),
                    Rc::new(Function {
                        parameters: resolved_parameters.clone(),
                        return_type: resolved_return_type.clone(),
                        body: Arc::clone(body),
                        source,
                    }),
                );
            self.variable_types.define(
                name.clone(),
                Type::Function {
                    parameters: resolved_parameters
                        .iter()
                        .map(|(_, typ, _)| typ.clone().map(Box::new))
                        .collect(),
                    return_type: resolved_return_type.map(Box::new),
                },
                false,
            );
            self.scopes
                .define(name.clone(), Value::Function(function_value), false)
                .map_err(|error| {
                    self.runtime_error_with_code(DiagnosticCode::DuplicateDeclaration, error)
                })?;
            self.hoisted_functions
                .insert((self.scopes.current_scope_index(), name.clone()));
        }
        Ok(())
    }

    pub fn run_file(&mut self, path: &Path) -> Result<(), SimplyError> {
        let resolved = fs::canonicalize(path).map_err(|error| self.file_error(path, error))?;
        let source =
            fs::read_to_string(&resolved).map_err(|error| self.file_error(&resolved, error))?;
        let tokens = Lexer::new(&source).tokenize()?;
        let program = Parser::new(tokens).parse()?;
        self.current_file = Some(resolved.clone());
        self.module_identity = resolved.display().to_string();
        self.current_source = Some(Rc::from(source));
        self.import_stack = vec![resolved];
        self.module_is_imported = false;
        self.run(&program)
    }

    fn execute_statements(&mut self, statements: &[Stmt]) -> Result<Flow, SimplyError> {
        for statement in statements {
            match statement {
                Stmt::Located { span, statement } => {
                    self.current_span = Some(span.clone());
                    match self.execute_statements(std::slice::from_ref(statement.as_ref()))? {
                        Flow::None => {}
                        flow => return Ok(flow),
                    }
                }
                Stmt::Say(expr) => {
                    let value = self.evaluate(expr)?;
                    if self.output_enabled {
                        print_value(&value, false);
                    }
                }
                Stmt::Sayln(expr) => {
                    let value = self.evaluate(expr)?;
                    if self.output_enabled {
                        print_value(&value, true);
                    }
                }
                Stmt::Import {
                    path,
                    alias,
                    exposing,
                } => {
                    let (value, exports, exported_types) = self.load_import(path)?;
                    if let Some(alias) = alias {
                        self.define(alias.clone(), value).map_err(|error| {
                            self.runtime_error_with_code(
                                DiagnosticCode::DuplicateDeclaration,
                                error,
                            )
                        })?;
                    } else {
                        let mut bindings = Vec::with_capacity(exposing.len());
                        let mut local_names = HashSet::with_capacity(exposing.len());
                        for (exported, local) in exposing {
                            if !local_names.insert(local) {
                                return Err(self.runtime_error_with_code(
                                    DiagnosticCode::DuplicateDeclaration,
                                    format!("`{local}` is imported more than once"),
                                ));
                            }
                            let Some(value) = exports.get(exported) else {
                                if exported_types.contains_key(exported) {
                                    continue;
                                }
                                return Err(self.runtime_error_with_code(
                                    DiagnosticCode::RuntimeImport,
                                    format!("module does not export `{exported}`"),
                                ));
                            };
                            if self.scopes.has_in_current_scope(local)
                                || self.lookup_struct(local).is_some()
                                || self.lookup_enum(local).is_some()
                            {
                                return Err(self.runtime_error_with_code(
                                    DiagnosticCode::DuplicateDeclaration,
                                    format!("`{local}` is already declared in this scope"),
                                ));
                            }
                            bindings.push((local.clone(), value.clone()));
                        }
                        for (name, value) in bindings {
                            self.variable_types.define(
                                name.clone(),
                                Self::type_of_value(&value),
                                false,
                            );
                            self.define(name, value).map_err(|error| {
                                self.runtime_error_with_code(
                                    DiagnosticCode::DuplicateDeclaration,
                                    error,
                                )
                            })?;
                        }
                        for (exported, local) in exposing {
                            let Some(exported_type) = exported_types.get(exported) else {
                                continue;
                            };
                            match exported_type {
                                ExportedRuntimeType::Struct {
                                    definition,
                                    messages,
                                } => {
                                    if self.lookup_struct(local).is_some()
                                        || self.lookup_enum(local).is_some()
                                    {
                                        return Err(self.runtime_error_with_code(
                                            DiagnosticCode::DuplicateDeclaration,
                                            format!("type `{local}` is already declared"),
                                        ));
                                    }
                                    self.struct_scopes
                                        .last_mut()
                                        .expect("struct scope stack always has a scope")
                                        .insert(local.clone(), definition.clone());
                                    self.message_scopes
                                        .last_mut()
                                        .expect("message scope stack always has a scope")
                                        .extend(messages.clone());
                                }
                                ExportedRuntimeType::Enum(definition) => {
                                    if self.lookup_struct(local).is_some()
                                        || self.lookup_enum(local).is_some()
                                    {
                                        return Err(self.runtime_error_with_code(
                                            DiagnosticCode::DuplicateDeclaration,
                                            format!("type `{local}` is already declared"),
                                        ));
                                    }
                                    self.enum_scopes
                                        .last_mut()
                                        .expect("enum scope stack always has a scope")
                                        .insert(local.clone(), definition.clone());
                                }
                            }
                        }
                    }
                }
                Stmt::Export { .. } => {
                    if !self.module_is_imported || self.scopes.current_scope_index() != 0 {
                        return Err(self.runtime_error_with_code(
                            DiagnosticCode::RuntimeImport,
                            "`export` is only allowed at the top level of an imported module",
                        ));
                    }
                }
                Stmt::Expression(expr) => {
                    self.evaluate(expr)?;
                }
                Stmt::Assign {
                    name,
                    mutable,
                    declared_type,
                    value,
                } => {
                    let value = self.evaluate(value)?;
                    if let Some(expected) = declared_type {
                        let expected = self.resolve_type_identity(expected);
                        self.ensure_type(&value, &expected, name)?;
                        self.variable_types.define(name.clone(), expected, *mutable);
                    } else {
                        self.variable_types.define(
                            name.clone(),
                            Self::type_of_value(&value),
                            *mutable,
                        );
                    }
                    self.scopes
                        .define(name.clone(), value, *mutable)
                        .map_err(|error| {
                            self.runtime_error_with_code(
                                DiagnosticCode::DuplicateDeclaration,
                                error,
                            )
                        })?;
                }
                Stmt::Flow {
                    name,
                    source,
                    steps,
                } => {
                    let value = self.evaluate(&Expr::Pipeline {
                        source: Box::new(source.clone()),
                        steps: steps.clone(),
                    })?;
                    self.variable_types
                        .define(name.clone(), Self::type_of_value(&value), false);
                    self.scopes
                        .define(name.clone(), value, false)
                        .map_err(|error| {
                            self.runtime_error_with_code(
                                DiagnosticCode::DuplicateDeclaration,
                                error,
                            )
                        })?;
                }
                Stmt::Reassign { name, value } => {
                    if !self.scopes.is_mutable(name) {
                        return Err(self.runtime_error_with_code(
                            DiagnosticCode::InvalidReassignment,
                            format!(
                                "cannot reassign immutable variable `{name}`; declare it with `mut`"
                            ),
                        ));
                    }
                    if !self.contains(name) {
                        return Err(self.runtime_error_with_code(
                            DiagnosticCode::RuntimeName,
                            format!("cannot reassign unknown variable `{name}`"),
                        ));
                    }
                    let value = self.evaluate(value)?;
                    if let Some(expected) = self.variable_types.lookup(name).cloned() {
                        self.ensure_reassignment_type(&value, &expected, name)?;
                    }
                    if !self.assign(name, value) {
                        return Err(self.runtime_error_with_code(
                            DiagnosticCode::RuntimeName,
                            format!("cannot reassign unknown variable `{name}`"),
                        ));
                    }
                }
                Stmt::DestructureReassign { pattern, value } => {
                    let value = self.evaluate(value)?;
                    let Some(bindings) = self.match_value_pattern(pattern, &value)? else {
                        return Err(self.runtime_collection_error(
                            "destructuring assignment target does not match the value",
                        ));
                    };
                    self.assign_many(bindings)?;
                }
                Stmt::CollectionOp {
                    name,
                    operation,
                    value,
                } => {
                    if !self.scopes.is_mutable(name) {
                        return Err(self.runtime_error_with_code(
                            DiagnosticCode::RuntimeMutability,
                            format!(
                                "cannot mutate immutable variable `{name}`; declare it with `mut`"
                            ),
                        ));
                    }
                    let value = self.evaluate(value)?;
                    if let Some(Type::List(element)) = self.variable_types.lookup(name).cloned() {
                        if *element == Type::Unknown {
                            self.variable_types.replace_visible(
                                name,
                                Type::List(Box::new(Self::type_of_value(&value))),
                            );
                        } else {
                            self.ensure_type(&value, &element, name)?;
                        }
                    }
                    let span = self.current_span.clone();
                    let target = match self.lookup_mut(name) {
                        Some(target) => target,
                        None => {
                            return Err(self.runtime_error_with_code(
                                DiagnosticCode::RuntimeCollection,
                                format!("`{name}` is not a list"),
                            ));
                        }
                    };
                    collections::mutate_list(target, operation, value, name, span.as_ref())?;
                    self.persist_active_message_field(name);
                }
                Stmt::SetIndex { name, index, value } => {
                    if !self.scopes.is_mutable(name) {
                        return Err(self.runtime_error_with_code(
                            DiagnosticCode::RuntimeMutability,
                            format!(
                                "cannot mutate immutable variable `{name}`; declare it with `mut`"
                            ),
                        ));
                    }
                    let index_value = self.evaluate(index)?;
                    let value = self.evaluate(value)?;
                    if let Some(expected) = self.variable_types.lookup(name)
                        && let Type::Array(element) | Type::List(element) = expected
                    {
                        self.ensure_type(&value, element, name)?;
                    } else if let Some(Type::HashValues(element)) = self.variable_types.lookup(name)
                    {
                        self.ensure_type(&value, element, name)?;
                    }
                    let span = self.current_span.clone();
                    let target = match self.lookup_mut(name) {
                        Some(target) => target,
                        None => {
                            return Err(self.runtime_collection_error(format!(
                                "`{name}` is not a mutable collection"
                            )));
                        }
                    };
                    collections::set_index(target, index_value, value, span.as_ref())?;
                    self.persist_active_message_field(name);
                }
                Stmt::Destructure {
                    pattern,
                    mutable,
                    value,
                } => {
                    let value = self.evaluate(value)?;
                    let Some(bindings) = self.match_value_pattern(pattern, &value)? else {
                        return Err(self.runtime_collection_error(
                            "destructuring target does not match the value",
                        ));
                    };
                    let typed_bindings = bindings
                        .into_iter()
                        .map(|(name, value)| {
                            let typ = Self::type_of_value(&value);
                            (name, value, typ, *mutable)
                        })
                        .collect::<Vec<_>>();
                    self.scopes
                        .define_many(
                            typed_bindings
                                .iter()
                                .map(|(name, value, _, mutable)| {
                                    (name.clone(), value.clone(), *mutable)
                                })
                                .collect(),
                        )
                        .map_err(|error| {
                            self.runtime_error_with_code(
                                DiagnosticCode::DuplicateDeclaration,
                                error,
                            )
                        })?;
                    for (name, _, typ, mutable) in typed_bindings {
                        self.variable_types.define(name, typ, mutable);
                    }
                }
                Stmt::Function {
                    name,
                    parameters,
                    return_type,
                    body,
                } => {
                    let source = self.function_source_context();
                    let resolved_parameters = parameters
                        .iter()
                        .map(|(name, typ, mutable)| {
                            (
                                name.clone(),
                                typ.as_ref().map(|typ| self.resolve_type_identity(typ)),
                                *mutable,
                            )
                        })
                        .collect::<Vec<_>>();
                    let resolved_return_type = return_type
                        .as_ref()
                        .map(|typ| self.resolve_type_identity(typ));
                    let function = self.track_function(Rc::new(FunctionValue {
                        name: Some(name.clone()),
                        parameters: resolved_parameters.clone(),
                        return_type: resolved_return_type.clone(),
                        body: Arc::clone(body),
                        captures: RefCell::new(
                            if self.function_scopes.len() > 1 || self.import_stack.len() > 1 {
                                let mut dependencies = closure_dependencies(body, parameters);
                                dependencies.remove(name);
                                self.scopes.values_for(&dependencies)
                            } else {
                                HashMap::new()
                            },
                        ),
                        source: source.clone(),
                    }));
                    let function_value = Value::Function(function);
                    self.function_scopes
                        .last_mut()
                        .expect("function scope stack always has a global scope")
                        .insert(
                            name.clone(),
                            Rc::new(Function {
                                parameters: resolved_parameters.clone(),
                                return_type: resolved_return_type.clone(),
                                body: Arc::clone(body),
                                source,
                            }),
                        );
                    self.variable_types.define(
                        name.clone(),
                        Type::Function {
                            parameters: resolved_parameters
                                .iter()
                                .map(|(_, typ, _)| typ.clone().map(Box::new))
                                .collect(),
                            return_type: resolved_return_type.clone().map(Box::new),
                        },
                        false,
                    );
                    if self
                        .hoisted_functions
                        .remove(&(self.scopes.current_scope_index(), name.clone()))
                    {
                        if !self.scopes.assign_current(name, function_value) {
                            return Err(self.runtime_error(format!(
                                "hoisted function `{name}` is missing its runtime binding"
                            )));
                        }
                    } else {
                        self.scopes
                            .define(name.clone(), function_value, false)
                            .map_err(|error| {
                                self.runtime_error_with_code(
                                    DiagnosticCode::DuplicateDeclaration,
                                    error,
                                )
                            })?;
                    }
                }
                Stmt::Struct { .. } | Stmt::Enum { .. } | Stmt::Message { .. } => {}
                Stmt::Return(expr) => return Ok(Flow::Return(self.evaluate(expr)?)),
                Stmt::Throw(expr) => return self.evaluate_throw(expr),
                Stmt::Break => return Ok(Flow::Break),
                Stmt::Continue => return Ok(Flow::Continue),
                Stmt::For {
                    name,
                    mutable,
                    iterable,
                    body,
                } => {
                    let values: Box<dyn Iterator<Item = Value>> = match self.evaluate(iterable)? {
                        Value::Range { start, end, step } => {
                            Box::new(Value::range_values(start, end, step))
                        }
                        Value::Array(values) | Value::List(values) | Value::Tuple(values) => {
                            Box::new(owned_values(values).into_iter())
                        }
                        Value::Hash(values) | Value::Tree(values) => {
                            Box::new(owned_map_values(values).into_iter())
                        }
                        _ => {
                            return Err(self.runtime_collection_error("for requires a collection"));
                        }
                    };
                    self.push_scope();
                    let result = (|| {
                        for value in values {
                            if self.scopes.has_in_current_scope(name) {
                                self.scopes.assign_current(name, value);
                            } else {
                                self.scopes.define(name.clone(), value, *mutable).map_err(
                                    |error| {
                                        self.runtime_error_with_code(
                                            DiagnosticCode::DuplicateDeclaration,
                                            error,
                                        )
                                    },
                                )?;
                            }
                            match self.execute_statements(body)? {
                                Flow::None | Flow::Continue => {}
                                Flow::Break => break,
                                Flow::Return(value) => return Ok(Some(value)),
                            }
                        }
                        Ok(None)
                    })();
                    self.pop_scope();
                    if let Some(value) = result? {
                        return Ok(Flow::Return(value));
                    }
                }
                Stmt::While { condition, body } => {
                    while match self.evaluate(condition)? {
                        Value::Bool(value) => value,
                        _ => {
                            return Err(
                                self.runtime_type_error("while condition must be a boolean")
                            );
                        }
                    } {
                        match self.execute_statements(body)? {
                            Flow::None | Flow::Continue => {}
                            Flow::Break => break,
                            Flow::Return(value) => return Ok(Flow::Return(value)),
                        }
                    }
                }
                Stmt::If {
                    condition,
                    then_branch,
                    else_branch,
                } => {
                    let branch = match self.evaluate(condition)? {
                        Value::Bool(true) => then_branch,
                        Value::Bool(false) => else_branch,
                        _ => {
                            return Err(self.runtime_type_error("if condition must be a boolean"));
                        }
                    };
                    self.push_scope();
                    let result = self.execute_statements(branch);
                    self.pop_scope();
                    match result? {
                        Flow::None => {}
                        flow => return Ok(flow),
                    }
                }
                Stmt::Try {
                    try_body,
                    catches,
                    finally_body,
                } => {
                    let primary = match self.execute_scoped(try_body) {
                        Ok(flow) => Ok(flow),
                        Err(error) => {
                            let catch = catches.iter().find(|catch| {
                                catch.code.as_deref().is_none_or(|code| {
                                    DiagnosticCode::from_code(code) == Some(error.code())
                                })
                            });
                            if let Some(catch) = catch {
                                self.push_scope();
                                let result = if let Some(name) = &catch.binding {
                                    let error_type = error
                                        .thrown_value()
                                        .map(Self::type_of_value)
                                        .unwrap_or(Type::Tree);
                                    self.variable_types.define(name.clone(), error_type, false);
                                    self.scopes
                                        .define(name.clone(), self.error_value(&error), false)
                                        .map_err(|definition_error| {
                                            self.runtime_error_with_code(
                                                DiagnosticCode::DuplicateDeclaration,
                                                definition_error,
                                            )
                                        })
                                        .and_then(|()| self.execute_statements(&catch.body))
                                } else {
                                    self.execute_statements(&catch.body)
                                };
                                self.pop_scope();
                                result
                            } else {
                                Err(error)
                            }
                        }
                    };

                    let finally_result = self.execute_scoped(finally_body);
                    match finally_result {
                        Err(error) => return Err(error),
                        Ok(flow @ (Flow::Return(_) | Flow::Break | Flow::Continue)) => {
                            return Ok(flow);
                        }
                        Ok(Flow::None) => {
                            let flow = primary?;
                            if !matches!(flow, Flow::None) {
                                return Ok(flow);
                            }
                        }
                    }
                }
            }
        }
        Ok(Flow::None)
    }

    fn evaluate_throw(&mut self, expression: &Expr) -> Result<Flow, SimplyError> {
        let value = self.evaluate(expression)?;
        if !matches!(value, Value::Enum(_)) {
            return Err(self.runtime_error_with_code(
                DiagnosticCode::RuntimeTypeMismatch,
                format!(
                    "`throw` requires an enum value, found {}",
                    Self::value_type_name(&value)
                ),
            ));
        }
        let message = value.display();
        Err(SimplyError::Thrown {
            span: self.current_span.clone().unwrap_or_else(|| Span::new(0, 0)),
            value: Box::new(value),
            message,
        })
    }

    fn execute_scoped(&mut self, statements: &[Stmt]) -> Result<Flow, SimplyError> {
        if statements.is_empty() {
            return Ok(Flow::None);
        }
        self.push_scope();
        let result = self.execute_statements(statements);
        self.pop_scope();
        result
    }

    fn error_value(&self, error: &SimplyError) -> Value {
        if let Some(value) = error.thrown_value() {
            return value.clone();
        }
        let mut fields = BTreeMap::new();
        fields.insert("message".into(), Value::String(error.message().into()));
        fields.insert("code".into(), Value::String(error.code().as_str().into()));
        fields.insert(
            "category".into(),
            Value::String(format!("{:?}", error.category())),
        );
        fields.insert("line".into(), Value::Int(error.span().line as i64));
        fields.insert("column".into(), Value::Int(error.span().column as i64));
        Value::Tree(shared_map(fields))
    }

    fn load_import(&mut self, path: &str) -> Result<ImportedModule, SimplyError> {
        let base = self
            .current_file
            .as_deref()
            .and_then(Path::parent)
            .unwrap_or_else(|| Path::new("."));
        let requested = Path::new(path);
        let resolved = if requested.is_absolute() {
            requested.to_path_buf()
        } else {
            base.join(requested)
        };
        let resolved =
            fs::canonicalize(&resolved).map_err(|error| self.file_error(&resolved, error))?;
        if let Some(cycle_start) = self
            .import_stack
            .iter()
            .position(|imported| imported == &resolved)
        {
            let mut chain = self.import_stack[cycle_start..]
                .iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>();
            chain.push(resolved.display().to_string());
            return Err(self.runtime_error_with_code(
                DiagnosticCode::RuntimeImport,
                format!("cyclic import: {}", chain.join(" -> ")),
            ));
        }
        let (program, source) = if let Some(cached) = self.import_cache.borrow().get(&resolved) {
            (Arc::clone(&cached.0), Rc::clone(&cached.1))
        } else {
            let source =
                fs::read_to_string(&resolved).map_err(|error| self.file_error(&resolved, error))?;
            let tokens = Lexer::new(&source)
                .tokenize()
                .map_err(|error| error.in_source(resolved.display().to_string(), source.clone()))?;
            let program = Parser::new(tokens)
                .parse()
                .map_err(|error| error.in_source(resolved.display().to_string(), source.clone()))?;
            let program = Arc::new(program);
            let source: Rc<str> = Rc::from(source);
            self.import_cache
                .borrow_mut()
                .insert(resolved.clone(), (Arc::clone(&program), Rc::clone(&source)));
            (program, source)
        };
        let mut module = Self {
            current_file: Some(resolved.clone()),
            module_identity: resolved.display().to_string(),
            current_source: Some(Rc::clone(&source)),
            import_stack: self
                .import_stack
                .iter()
                .cloned()
                .chain(std::iter::once(resolved.clone()))
                .collect(),
            module_is_imported: true,
            import_cache: Rc::clone(&self.import_cache),
            ..Self::new()
        };
        let mut export_names = Vec::new();
        let mut seen_exports = HashSet::new();
        for statement in &program.statements {
            let statement = match statement {
                Stmt::Located { statement, .. } => statement.as_ref(),
                statement => statement,
            };
            if let Stmt::Export { names } = statement {
                for name in names {
                    if !seen_exports.insert(name.clone()) {
                        return Err(module.runtime_error_with_code(
                            DiagnosticCode::RuntimeImport,
                            format!("`{name}` is exported more than once"),
                        ));
                    }
                    export_names.push(name.clone());
                }
            }
        }
        let result = (|| {
            module.register_declarations(&program.statements)?;
            let returned_value = match module.execute_statements(&program.statements)? {
                Flow::Return(value) => value,
                Flow::None => {
                    return Err(module.runtime_error_with_code(
                        DiagnosticCode::RuntimeImport,
                        format!("imported file `{path}` must return a value"),
                    ));
                }
                Flow::Break | Flow::Continue => {
                    return Err(module.runtime_error_with_code(
                        DiagnosticCode::RuntimeControl,
                        "control statement is outside its valid context",
                    ));
                }
            };
            let mut exports = HashMap::new();
            let mut exported_types = HashMap::new();
            for name in export_names {
                if let Some(value) = module.lookup(&name).cloned() {
                    if let Value::Function(function) = &value
                        && function.captures.borrow().is_empty()
                    {
                        let mut dependencies =
                            closure_dependencies(&function.body, &function.parameters);
                        if let Some(function_name) = &function.name {
                            dependencies.remove(function_name);
                        }
                        function
                            .captures
                            .replace(module.scopes.values_for(&dependencies));
                    }
                    exports.insert(name, value);
                    continue;
                }
                if let Some(definition) = module.lookup_struct(&name).cloned() {
                    let messages = module
                        .message_scopes
                        .iter()
                        .flat_map(|scope| scope.iter())
                        .filter(|((identity, _), _)| identity == &definition.identity)
                        .map(|(key, behavior)| {
                            let behavior = behavior.clone();
                            let mut dependencies = closure_dependencies(
                                &behavior.function.body,
                                &behavior.function.parameters,
                            );
                            for (parameter, _, _) in &behavior.function.parameters {
                                dependencies.remove(parameter);
                            }
                            behavior
                                .function
                                .captures
                                .replace(module.scopes.values_for(&dependencies));
                            (key.clone(), behavior)
                        })
                        .collect();
                    exported_types.insert(
                        name,
                        ExportedRuntimeType::Struct {
                            definition,
                            messages,
                        },
                    );
                    continue;
                }
                if let Some(definition) = module.lookup_enum(&name).cloned() {
                    exported_types.insert(name, ExportedRuntimeType::Enum(definition));
                    continue;
                }
                return Err(module.runtime_error_with_code(
                    DiagnosticCode::RuntimeImport,
                    format!("cannot export unknown value `{name}` or type"),
                ));
            }
            Ok((returned_value, exports, exported_types))
        })();
        result.map_err(|error: SimplyError| {
            error
                .in_source(resolved.display().to_string(), source.as_ref().to_owned())
                .with_context(format!("in imported module `{}`", resolved.display()))
        })
    }

    fn file_error(&self, path: &Path, error: std::io::Error) -> SimplyError {
        self.runtime_error_with_code(
            DiagnosticCode::RuntimeImport,
            format!("could not open `{}`: {error}", path.display()),
        )
    }

    fn function_source_context(&self) -> Option<SourceContext> {
        if self.import_stack.len() <= 1 {
            return None;
        }
        Some(SourceContext {
            filename: self.current_file.as_ref()?.display().to_string(),
            source: Rc::clone(self.current_source.as_ref()?),
        })
    }

    fn evaluate(&mut self, expr: &Expr) -> Result<Value, SimplyError> {
        match expr {
            Expr::Literal(literal) => Ok(match literal {
                Literal::String(value) => Value::String(value.clone()),
                Literal::Int(value) => Value::Int(*value),
                Literal::Float(value) => Value::Float(*value),
                Literal::Bool(value) => Value::Bool(*value),
            }),
            Expr::Array(values) => Ok(Value::Array(shared_values(self.evaluate_values(values)?))),
            Expr::List(values) => Ok(Value::List(shared_values(self.evaluate_values(values)?))),
            Expr::Tuple(values) => Ok(Value::Tuple(shared_values(self.evaluate_values(values)?))),
            Expr::Matrix(values) => Ok(Value::Matrix(shared_values(self.evaluate_values(values)?))),
            Expr::Hash(entries) => {
                let mut values = BTreeMap::new();
                for (name, expression) in entries {
                    values.insert(name.clone(), self.evaluate(expression)?);
                }
                Ok(Value::Hash(shared_map(values)))
            }
            Expr::Tree(entries) => {
                let mut values = BTreeMap::new();
                for (name, expression) in entries {
                    values.insert(name.clone(), self.evaluate(expression)?);
                }
                Ok(Value::Tree(shared_map(values)))
            }
            Expr::Pipeline { source, steps } => {
                let sum_type = self.pipeline_sum_type(source, steps);
                let result = match self.evaluate(source)? {
                    Value::CsvStream {
                        path,
                        start_record,
                        start_offset,
                        source_version,
                    } => self.evaluate_csv_pipeline(
                        &path,
                        start_record,
                        start_offset,
                        source_version.as_ref(),
                        steps,
                        sum_type.clone(),
                    )?,
                    Value::Range { start, end, step } => {
                        self.evaluate_range_pipeline(start, end, step, steps, sum_type.clone())?
                    }
                    Value::Array(values) | Value::List(values) => {
                        self.evaluate_pipeline(owned_values(values), steps, sum_type.clone())?
                    }
                    _ => {
                        return Err(self
                            .runtime_collection_error("pipeline source must be an array or list"));
                    }
                };
                Ok(result)
            }
            Expr::Identifier(name) => self.lookup(name).cloned().ok_or_else(|| {
                self.runtime_error_with_code(
                    DiagnosticCode::RuntimeName,
                    format!("unknown variable `{name}`"),
                )
            }),
            Expr::Unary { operator, operand } => {
                let value = self.evaluate(operand)?;
                operations::unary(value, operator, self.current_span.as_ref())
            }
            Expr::Binary {
                left,
                operator,
                right,
            } => {
                let left = self.evaluate(left)?;
                if matches!(operator, BinaryOperator::And | BinaryOperator::Or)
                    && let Value::Bool(value) = left
                    && ((*operator == BinaryOperator::And && !value)
                        || (*operator == BinaryOperator::Or && value))
                {
                    return Ok(Value::Bool(value));
                }
                let right = self.evaluate(right)?;
                operations::binary(left, operator, right, self.current_span.as_ref())
            }
            Expr::Call { name, arguments } => {
                if let Some(definition) = self.lookup_struct(name).cloned() {
                    let values = self.evaluate_values(arguments)?;
                    return self.construct_struct(name, &definition, values);
                }
                if name == "Ask" {
                    if !(1..=2).contains(&arguments.len()) {
                        return Err(self.runtime_error_with_code(
                            DiagnosticCode::RuntimeArgument,
                            "`Ask` expects a prompt and an optional type",
                        ));
                    }
                    let prompt = match self.evaluate(&arguments[0])? {
                        Value::String(prompt) => prompt,
                        _ => {
                            return Err(self.runtime_error_with_code(
                                DiagnosticCode::RuntimeTypeMismatch,
                                "`Ask` prompt must be a string",
                            ));
                        }
                    };
                    let target_type = match arguments.get(1) {
                        None => Type::String,
                        Some(Expr::Identifier(type_name)) => match type_name.as_str() {
                            "String" => Type::String,
                            "Int" => Type::Int,
                            "Float" => Type::Float,
                            "Bool" => Type::Bool,
                            _ => {
                                return Err(self.runtime_error_with_code(
                                    DiagnosticCode::RuntimeArgument,
                                    format!("unsupported `Ask` type `{type_name}`"),
                                ));
                            }
                        },
                        Some(_) => {
                            return Err(self.runtime_error_with_code(
                                DiagnosticCode::RuntimeArgument,
                                "`Ask` type must be `Int`, `Float`, `String`, or `Bool`",
                            ));
                        }
                    };
                    if self.output_enabled {
                        let mut stdout = io::stdout().lock();
                        stdout
                            .write_all(prompt.as_bytes())
                            .and_then(|()| stdout.flush())
                            .map_err(|error| {
                                self.runtime_error_with_code(
                                    DiagnosticCode::RuntimeIo,
                                    format!("could not write `Ask` prompt: {error}"),
                                )
                            })?;
                    }
                    let mut input = String::new();
                    let bytes_read = io::stdin().lock().read_line(&mut input).map_err(|error| {
                        self.runtime_error_with_code(
                            DiagnosticCode::RuntimeIo,
                            format!("could not read `Ask` input: {error}"),
                        )
                    })?;
                    if bytes_read == 0 {
                        return Err(self.runtime_error_with_code(
                            DiagnosticCode::RuntimeIo,
                            "no input received for `Ask`",
                        ));
                    }
                    let input = input.trim_end_matches(['\r', '\n']);
                    return match target_type {
                        Type::String => Ok(Value::String(input.to_owned())),
                        Type::Int => input.trim().parse::<i64>().map(Value::Int).map_err(|_| {
                            self.runtime_error_with_code(
                                DiagnosticCode::RuntimeConversion,
                                format!("`Ask` expected an Int, got `{input}`"),
                            )
                        }),
                        Type::Float => match input.trim().parse::<f64>() {
                            Ok(value) if value.is_finite() => Ok(Value::Float(value)),
                            _ => Err(self.runtime_error_with_code(
                                DiagnosticCode::RuntimeConversion,
                                format!("`Ask` expected a finite Float, got `{input}`"),
                            )),
                        },
                        Type::Bool if input.trim().eq_ignore_ascii_case("true") => {
                            Ok(Value::Bool(true))
                        }
                        Type::Bool if input.trim().eq_ignore_ascii_case("false") => {
                            Ok(Value::Bool(false))
                        }
                        Type::Bool => Err(self.runtime_error_with_code(
                            DiagnosticCode::RuntimeConversion,
                            format!("`Ask` expected Bool (`true` or `false`), got `{input}`"),
                        )),
                        _ => unreachable!("Ask target type is validated above"),
                    };
                }
                if is_math_builtin(name) {
                    let values = arguments
                        .iter()
                        .map(|argument| self.evaluate(argument))
                        .collect::<Result<Vec<_>, _>>()?;
                    return evaluate_math_builtin(name, &values, self.current_span.as_ref())?
                        .ok_or_else(|| {
                            self.runtime_error(format!("unknown numerical builtin `{name}`"))
                        });
                }
                if name == "print" {
                    if arguments.len() != 1 {
                        return Err(self.runtime_error_with_code(
                            DiagnosticCode::RuntimeArgument,
                            "`print` expects one argument",
                        ));
                    }
                    let value = self.evaluate(&arguments[0])?;
                    if self.output_enabled {
                        print_value(&value, true);
                    }
                    return Ok(Value::Unit);
                }
                if name == "assert" {
                    if !(1..=2).contains(&arguments.len()) {
                        return Err(self.runtime_error_with_code(
                            DiagnosticCode::RuntimeArgument,
                            "`assert` expects a condition and an optional message",
                        ));
                    }
                    let condition = self.evaluate(&arguments[0])?;
                    let Value::Bool(condition) = condition else {
                        return Err(self.runtime_error_with_code(
                            DiagnosticCode::RuntimeTypeMismatch,
                            "`assert` condition must be Bool",
                        ));
                    };
                    if condition {
                        return Ok(Value::Unit);
                    }
                    let message = match arguments.get(1) {
                        Some(expression) => match self.evaluate(expression)? {
                            Value::String(message) => message,
                            _ => {
                                return Err(self.runtime_error_with_code(
                                    DiagnosticCode::RuntimeTypeMismatch,
                                    "`assert` message must be a string",
                                ));
                            }
                        },
                        None => "assertion failed".into(),
                    };
                    return Err(
                        self.runtime_error_with_code(DiagnosticCode::RuntimeAssertion, message)
                    );
                }
                if name == "type_of" {
                    if arguments.len() != 1 {
                        return Err(self.runtime_error_with_code(
                            DiagnosticCode::RuntimeArgument,
                            "`type_of` expects one argument",
                        ));
                    }
                    let value = self.evaluate(&arguments[0])?;
                    let type_name = match value {
                        Value::Unit => "Unit",
                        Value::String(_) => "String",
                        Value::Int(_) => "Int",
                        Value::Float(_) => "Float",
                        Value::Bool(_) => "Bool",
                        Value::Range { .. } => "Range",
                        Value::CsvStream { .. } => "CsvStream",
                        Value::Array(_) => "Array",
                        Value::List(_) => "List",
                        Value::Tuple(_) => "Tuple",
                        Value::Hash(_) => "Hash",
                        Value::Tree(_) => "Tree",
                        Value::Matrix(_) => "Matrix",
                        Value::Function(_) => "Function",
                        Value::Struct(instance) => {
                            return Ok(Value::String(instance.type_name.clone()));
                        }
                        Value::Enum(value) => {
                            return Ok(Value::String(value.enum_name.clone()));
                        }
                    };
                    return Ok(Value::String(type_name.into()));
                }
                if name == "read_file" {
                    if arguments.len() != 1 {
                        return Err(self.runtime_error_with_code(
                            DiagnosticCode::RuntimeArgument,
                            "`read_file` expects one path",
                        ));
                    }
                    let path = match self.evaluate(&arguments[0])? {
                        Value::String(path) => path,
                        _ => {
                            return Err(self.runtime_error_with_code(
                                DiagnosticCode::RuntimeTypeMismatch,
                                "`read_file` path must be a string",
                            ));
                        }
                    };
                    return fs::read_to_string(&path)
                        .map(Value::String)
                        .map_err(|error| {
                            self.runtime_error_with_code(
                                DiagnosticCode::RuntimeIo,
                                format!("could not read file `{path}`: {error}"),
                            )
                        });
                }
                if name == "write_file" {
                    if arguments.len() != 2 {
                        return Err(self.runtime_error_with_code(
                            DiagnosticCode::RuntimeArgument,
                            "`write_file` expects a path and string content",
                        ));
                    }
                    let path = match self.evaluate(&arguments[0])? {
                        Value::String(path) => path,
                        _ => {
                            return Err(self.runtime_error_with_code(
                                DiagnosticCode::RuntimeTypeMismatch,
                                "`write_file` path must be a string",
                            ));
                        }
                    };
                    let content = match self.evaluate(&arguments[1])? {
                        Value::String(content) => content,
                        _ => {
                            return Err(self.runtime_error_with_code(
                                DiagnosticCode::RuntimeTypeMismatch,
                                "`write_file` content must be a string",
                            ));
                        }
                    };
                    files::atomic_write(Path::new(&path), content.as_bytes()).map_err(|error| {
                        self.runtime_error_with_code(
                            DiagnosticCode::RuntimeIo,
                            format!("could not write file `{path}`: {error}"),
                        )
                    })?;
                    return Ok(Value::Unit);
                }
                if name == "substring" {
                    if arguments.len() != 3 {
                        return Err(self.runtime_argument_error(
                            "`substring` expects a string, start index, and length",
                        ));
                    }
                    let value = match self.evaluate(&arguments[0])? {
                        Value::String(value) => value,
                        _ => return Err(self.runtime_type_error("`substring` expects a string")),
                    };
                    let start = match self.evaluate(&arguments[1])? {
                        Value::Int(start) if start >= 0 => {
                            usize::try_from(start).map_err(|_| {
                                self.runtime_error_with_code(
                                    DiagnosticCode::RuntimeLimit,
                                    "`substring` start is out of bounds",
                                )
                            })?
                        }
                        _ => {
                            return Err(self.runtime_argument_error(
                                "`substring` start must be a non-negative integer",
                            ));
                        }
                    };
                    let length = match self.evaluate(&arguments[2])? {
                        Value::Int(length) if length >= 0 => {
                            usize::try_from(length).map_err(|_| {
                                self.runtime_error_with_code(
                                    DiagnosticCode::RuntimeLimit,
                                    "`substring` length is out of bounds",
                                )
                            })?
                        }
                        _ => {
                            return Err(self.runtime_argument_error(
                                "`substring` length must be a non-negative integer",
                            ));
                        }
                    };
                    let character_count = value.chars().count();
                    if start > character_count || length > character_count - start {
                        return Err(self.runtime_argument_error("substring is out of bounds"));
                    }
                    return Ok(Value::String(
                        value.chars().skip(start).take(length).collect(),
                    ));
                }
                if name == "characters" {
                    if arguments.len() != 1 {
                        return Err(self.runtime_argument_error("`characters` expects one string"));
                    }
                    let value = match self.evaluate(&arguments[0])? {
                        Value::String(value) => value,
                        _ => {
                            return Err(self.runtime_type_error("`characters` expects a string"));
                        }
                    };
                    return Ok(Value::Array(shared_values(
                        value
                            .chars()
                            .map(|character| Value::String(character.to_string()))
                            .collect(),
                    )));
                }
                if matches!(
                    name.as_str(),
                    "is_ascii_alpha" | "is_ascii_digit" | "is_whitespace"
                ) {
                    if arguments.len() != 1 {
                        return Err(
                            self.runtime_argument_error(format!("`{name}` expects one character"))
                        );
                    }
                    let character = self.evaluate(&arguments[0])?;
                    let character = self.single_character(character, name)?;
                    return Ok(Value::Bool(match name.as_str() {
                        "is_ascii_alpha" => character.is_ascii_alphabetic(),
                        "is_ascii_digit" => character.is_ascii_digit(),
                        _ => character.is_whitespace(),
                    }));
                }
                if name == "contains" {
                    if arguments.len() != 2 {
                        return Err(self.runtime_argument_error("`contains` expects two arguments"));
                    }
                    let collection = self.evaluate(&arguments[0])?;
                    let searched = self.evaluate(&arguments[1])?;
                    let result = match collection {
                        Value::String(value) => match searched {
                            Value::String(searched) => value.contains(&searched),
                            _ => false,
                        },
                        Value::Array(values) | Value::List(values) | Value::Tuple(values) => {
                            values.iter().any(|value| value == &searched)
                        }
                        Value::Range { start, end, step } => {
                            matches!(searched, Value::Int(value)
                                if (step > 0 && value >= start && value < end
                                    || step < 0 && value <= start && value > end)
                                    && (i128::from(value) - i128::from(start))
                                        % i128::from(step)
                                        == 0)
                        }
                        Value::Hash(values) | Value::Tree(values) => {
                            values.values().any(|value| value == &searched)
                        }
                        _ => {
                            return Err(self.runtime_collection_error(
                                "`contains` requires a collection or string",
                            ));
                        }
                    };
                    return Ok(Value::Bool(result));
                }
                if (name == "keys"
                    || name == "values"
                    || name == "entries"
                    || name == "has_key"
                    || name == "get"
                    || name == "without_key"
                    || name == "select_keys")
                    && !matches!(self.lookup(name), Some(Value::Function(_)))
                {
                    return self.evaluate_map_keys_or_values(name, arguments);
                }
                if name == "any" || name == "all" {
                    if arguments.len() != 1 {
                        return Err(
                            self.runtime_argument_error(format!("`{name}` expects one argument"))
                        );
                    }
                    let collection = self.evaluate(&arguments[0])?;
                    let values = match collection {
                        Value::Array(values) | Value::List(values) | Value::Tuple(values) => values,
                        Value::Hash(values) | Value::Tree(values) => {
                            shared_values(values.values().cloned().collect())
                        }
                        _ => {
                            return Err(self.runtime_collection_error(format!(
                                "`{name}` requires a collection"
                            )));
                        }
                    };
                    let mut result = name == "all";
                    for value in values.iter() {
                        let Value::Bool(value) = value else {
                            return Err(self.runtime_type_error(format!(
                                "`{name}` requires a collection of booleans"
                            )));
                        };
                        if (name == "any" && *value) || (name == "all" && !*value) {
                            result = name == "any";
                            break;
                        }
                    }
                    return Ok(Value::Bool(result));
                }
                if name == "join" {
                    if arguments.len() != 2 {
                        return Err(self.runtime_argument_error("`join` expects two arguments"));
                    }
                    let collection = self.evaluate(&arguments[0])?;
                    let separator = match self.evaluate(&arguments[1])? {
                        Value::String(value) => value,
                        _ => {
                            return Err(
                                self.runtime_type_error("`join` separator must be a string")
                            );
                        }
                    };
                    let values = match collection {
                        Value::Array(values) | Value::List(values) | Value::Tuple(values) => values,
                        Value::Hash(values) | Value::Tree(values) => {
                            shared_values(values.values().cloned().collect())
                        }
                        _ => {
                            return Err(self.runtime_collection_error(
                                "`join` requires an array, list, or tuple",
                            ));
                        }
                    };
                    let mut parts = Vec::with_capacity(values.len());
                    for value in values.iter() {
                        let Value::String(value) = value else {
                            return Err(
                                self.runtime_type_error("`join` requires a collection of strings")
                            );
                        };
                        parts.push(value.as_str());
                    }
                    return Ok(Value::String(parts.join(&separator)));
                }
                if name == "total" {
                    if arguments.len() != 1 {
                        return Err(self.runtime_argument_error("`total` expects one argument"));
                    }
                    let sum_type = self.sequence_sum_type(&arguments[0]);
                    let collection = self.evaluate(&arguments[0])?;
                    let span = self.current_span.clone();
                    let mut total = SumAccumulator::new(sum_type);
                    let mut add_value =
                        |value| -> Result<(), SimplyError> { total.add(value, span.as_ref()) };
                    match collection {
                        Value::Array(values) | Value::List(values) | Value::Tuple(values) => {
                            for value in values.iter() {
                                add_value(value.clone())?;
                            }
                        }
                        Value::Range { start, end, step } => {
                            for value in Value::range_values(start, end, step) {
                                add_value(value)?;
                            }
                        }
                        _ => {
                            return Err(self.runtime_collection_error(
                                "`total` requires an array, list, tuple, or range",
                            ));
                        }
                    }
                    let total = total.finish();
                    return Ok(total);
                }
                if name == "trim" {
                    if arguments.len() != 1 {
                        return Err(self.runtime_argument_error("`trim` expects one argument"));
                    }
                    let value = match self.evaluate(&arguments[0])? {
                        Value::String(value) => value,
                        _ => return Err(self.runtime_type_error("`trim` expects a string")),
                    };
                    return Ok(Value::String(value.trim().into()));
                }
                if name == "to_float" {
                    if arguments.len() != 1 {
                        return Err(self.runtime_argument_error("`to_float` expects one argument"));
                    }
                    let value = match self.evaluate(&arguments[0])? {
                        Value::String(value) => value,
                        _ => {
                            return Err(self.runtime_type_error(
                                "`to_float` expects a string containing a number",
                            ));
                        }
                    };
                    let value = value.trim().parse::<f64>().map_err(|_| {
                        self.runtime_error_with_code(
                            DiagnosticCode::RuntimeConversion,
                            format!("cannot convert `{value}` to a Float"),
                        )
                    })?;
                    if !value.is_finite() {
                        return Err(self.runtime_error_with_code(
                            DiagnosticCode::RuntimeConversion,
                            "the converted Float must be finite",
                        ));
                    }
                    return Ok(Value::Float(value));
                }
                if name == "to_int" {
                    if arguments.len() != 1 {
                        return Err(self.runtime_argument_error("`to_int` expects one argument"));
                    }
                    let value = match self.evaluate(&arguments[0])? {
                        Value::String(value) => value,
                        _ => {
                            return Err(self.runtime_type_error(
                                "`to_int` expects a string containing a whole number",
                            ));
                        }
                    };
                    let value = value.trim().parse::<i64>().map_err(|_| {
                        self.runtime_error_with_code(
                            DiagnosticCode::RuntimeConversion,
                            format!("cannot convert `{value}` to an Int"),
                        )
                    })?;
                    return Ok(Value::Int(value));
                }
                if name == "abs" {
                    if arguments.len() != 1 {
                        return Err(
                            self.runtime_argument_error("`abs` expects one numeric argument")
                        );
                    }
                    return match self.evaluate(&arguments[0])? {
                        Value::Int(value) => value.checked_abs().map(Value::Int).ok_or_else(|| {
                            self.runtime_error_with_code(
                                DiagnosticCode::RuntimeArithmetic,
                                "integer absolute value overflow",
                            )
                        }),
                        Value::Float(value) if value.is_finite() => Ok(Value::Float(value.abs())),
                        _ => Err(self.runtime_type_error("`abs` expects an integer or float")),
                    };
                }
                if name == "round" {
                    if arguments.len() != 2 {
                        return Err(self
                            .runtime_argument_error("`round` expects a number and decimal count"));
                    }
                    let value = self.evaluate(&arguments[0])?;
                    let decimals = match self.evaluate(&arguments[1])? {
                        Value::Int(value) if (0..=15).contains(&value) => value as i32,
                        _ => {
                            return Err(self.runtime_argument_error(
                                "`round` decimal count must be an integer from 0 to 15",
                            ));
                        }
                    };
                    let factor = 10f64.powi(decimals);
                    return match value {
                        Value::Int(value) => Ok(Value::Int(value)),
                        Value::Float(value) if value.is_finite() => {
                            let rounded = (value * factor).round() / factor;
                            if rounded.is_finite() {
                                Ok(Value::Float(rounded))
                            } else {
                                Err(self.runtime_error_with_code(
                                    DiagnosticCode::RuntimeArithmetic,
                                    "`round` result is outside the finite Float range",
                                ))
                            }
                        }
                        _ => {
                            Err(self
                                .runtime_type_error("`round` expects an integer or finite float"))
                        }
                    };
                }
                if name == "clamp" {
                    if arguments.len() != 3 {
                        return Err(self.runtime_argument_error(
                            "`clamp` expects value, minimum, and maximum",
                        ));
                    }
                    let value = self.evaluate(&arguments[0])?;
                    let minimum = self.evaluate(&arguments[1])?;
                    let maximum = self.evaluate(&arguments[2])?;
                    return clamp_numeric(value, minimum, maximum, self.current_span.as_ref());
                }
                if name == "split" {
                    if arguments.len() != 2 {
                        return Err(self.runtime_argument_error("`split` expects two arguments"));
                    }
                    let value = match self.evaluate(&arguments[0])? {
                        Value::String(value) => value,
                        _ => return Err(self.runtime_type_error("`split` expects a string")),
                    };
                    let separator = match self.evaluate(&arguments[1])? {
                        Value::String(separator) => separator,
                        _ => {
                            return Err(
                                self.runtime_type_error("`split` separator must be a string")
                            );
                        }
                    };
                    return Ok(Value::List(shared_values(
                        value
                            .split(&separator)
                            .map(|part| Value::String(part.into()))
                            .collect(),
                    )));
                }
                if name == "replace" {
                    if arguments.len() != 3 {
                        return Err(
                            self.runtime_argument_error("`replace` expects three arguments")
                        );
                    }
                    let value = match self.evaluate(&arguments[0])? {
                        Value::String(value) => value,
                        _ => return Err(self.runtime_type_error("`replace` expects strings")),
                    };
                    let from = match self.evaluate(&arguments[1])? {
                        Value::String(from) => from,
                        _ => return Err(self.runtime_type_error("`replace` expects strings")),
                    };
                    let to = match self.evaluate(&arguments[2])? {
                        Value::String(to) => to,
                        _ => return Err(self.runtime_type_error("`replace` expects strings")),
                    };
                    return Ok(Value::String(value.replace(&from, &to)));
                }
                if name == "starts_with" || name == "ends_with" {
                    if arguments.len() != 2 {
                        return Err(
                            self.runtime_argument_error(format!("`{name}` expects two arguments"))
                        );
                    }
                    let value = match self.evaluate(&arguments[0])? {
                        Value::String(value) => value,
                        _ => {
                            return Err(
                                self.runtime_type_error(format!("`{name}` expects strings"))
                            );
                        }
                    };
                    let part = match self.evaluate(&arguments[1])? {
                        Value::String(part) => part,
                        _ => {
                            return Err(
                                self.runtime_type_error(format!("`{name}` expects strings"))
                            );
                        }
                    };
                    let result = if name == "starts_with" {
                        value.starts_with(&part)
                    } else {
                        value.ends_with(&part)
                    };
                    return Ok(Value::Bool(result));
                }
                if name == "is_empty" {
                    if arguments.len() != 1 {
                        return Err(self.runtime_argument_error("`is_empty` expects one argument"));
                    }
                    let value = self.evaluate(&arguments[0])?;
                    let result = match value {
                        Value::Array(values) | Value::List(values) | Value::Tuple(values) => {
                            values.is_empty()
                        }
                        Value::Range { start, end, step } => {
                            Value::range_len(start, end, step) == Some(0)
                        }
                        Value::Hash(values) | Value::Tree(values) => values.is_empty(),
                        Value::String(value) => value.is_empty(),
                        _ => {
                            return Err(self
                                .runtime_type_error("`is_empty` requires a collection or string"));
                        }
                    };
                    return Ok(Value::Bool(result));
                }
                if name == "reverse" {
                    if arguments.len() != 1 {
                        return Err(self.runtime_argument_error("`reverse` expects one argument"));
                    }
                    let value = self.evaluate(&arguments[0])?;
                    return Ok(match value {
                        Value::Array(values) => {
                            let mut values = owned_values(values);
                            values.reverse();
                            Value::Array(shared_values(values))
                        }
                        Value::List(values) => {
                            let mut values = owned_values(values);
                            values.reverse();
                            Value::List(shared_values(values))
                        }
                        Value::Tuple(values) => {
                            let mut values = owned_values(values);
                            values.reverse();
                            Value::Tuple(shared_values(values))
                        }
                        Value::Range { start, end, step } => {
                            let length = Value::range_len(start, end, step)
                                .and_then(|length| usize::try_from(length).ok())
                                .ok_or_else(|| {
                                    self.runtime_error_with_code(
                                        DiagnosticCode::RuntimeLimit,
                                        "range is too large to reverse",
                                    )
                                })?;
                            let mut values = Vec::new();
                            values.try_reserve_exact(length).map_err(|_| {
                                self.runtime_error_with_code(
                                    DiagnosticCode::RuntimeLimit,
                                    "range is too large to reverse",
                                )
                            })?;
                            values.extend(Value::range_values(start, end, step));
                            values.reverse();
                            Value::Array(shared_values(values))
                        }
                        _ => {
                            return Err(self.runtime_type_error(
                                "`reverse` requires an array, list, tuple, or range",
                            ));
                        }
                    });
                }
                if name == "enumerate" && !matches!(self.lookup(name), Some(Value::Function(_))) {
                    return self.evaluate_enumerate(arguments);
                }
                if name == "zip" && !matches!(self.lookup(name), Some(Value::Function(_))) {
                    return self.evaluate_zip(arguments);
                }
                if name == "length" || name == "count" {
                    if arguments.len() != 1 {
                        return Err(
                            self.runtime_argument_error(format!("`{name}` expects one argument"))
                        );
                    }
                    return match self.evaluate(&arguments[0])? {
                        Value::Array(values) | Value::List(values) | Value::Tuple(values) => {
                            Ok(Value::Int(values.len() as i64))
                        }
                        Value::Range { start, end, step } => Value::range_len(start, end, step)
                            .map(Value::Int)
                            .ok_or_else(|| {
                                self.runtime_error_with_code(
                                    DiagnosticCode::RuntimeArithmetic,
                                    "range length exceeds the Int range",
                                )
                            }),
                        Value::Hash(values) | Value::Tree(values) => {
                            Ok(Value::Int(values.len() as i64))
                        }
                        Value::String(value) => Ok(Value::Int(value.chars().count() as i64)),
                        _ => Err(self.runtime_type_error(format!(
                            "`{name}` requires a collection or string"
                        ))),
                    };
                }
                if name == "range" {
                    if !(2..=3).contains(&arguments.len()) {
                        return Err(self.runtime_argument_error(
                            "`range` expects start, end, and an optional step",
                        ));
                    }
                    let start = self.evaluate(&arguments[0])?;
                    let end = self.evaluate(&arguments[1])?;
                    let step = match arguments.get(2) {
                        Some(step) => self.evaluate(step)?,
                        None => Value::Int(1),
                    };
                    if let (Value::Int(start), Value::Int(end), Value::Int(step)) =
                        (start, end, step)
                    {
                        if step == 0 {
                            return Err(self.runtime_argument_error("`range` step cannot be zero"));
                        }
                        return Ok(Value::Range { start, end, step });
                    }
                    return Err(self.runtime_type_error(
                        "`range` expects integer bounds and an optional integer step",
                    ));
                }
                if name == "csv_rows" {
                    if arguments.len() != 1 {
                        return Err(self.runtime_argument_error("`csv_rows` expects one path"));
                    }
                    let path = match self.evaluate(&arguments[0])? {
                        Value::String(path) => path,
                        _ => {
                            return Err(self.runtime_type_error("`csv_rows` path must be a string"));
                        }
                    };
                    return Ok(Value::CsvStream {
                        path,
                        start_record: 0,
                        start_offset: 0,
                        source_version: None,
                    });
                }
                if name == "csv_row" {
                    let values = arguments
                        .iter()
                        .map(|argument| self.evaluate(argument))
                        .collect::<Result<Vec<_>, _>>()?;
                    return Ok(Value::List(shared_values(values)));
                }
                let values = arguments
                    .iter()
                    .map(|argument| self.evaluate(argument))
                    .collect::<Result<Vec<_>, _>>()?;
                self.invoke_function(name, values)
            }
            Expr::MessageDispatch {
                receiver,
                message,
                arguments,
            } => {
                let receiver = self.evaluate(receiver)?;
                let argument_values = arguments
                    .iter()
                    .map(|argument| self.evaluate(argument))
                    .collect::<Result<Vec<_>, _>>()?;
                self.dispatch_message(receiver, message, argument_values)
            }
            Expr::EnumVariant {
                enum_name,
                variant_name,
                arguments,
            } => {
                if let Some(definition) = self.lookup_enum(enum_name).cloned() {
                    let Some(variant) = definition
                        .variants
                        .iter()
                        .find(|variant| variant.name == *variant_name)
                    else {
                        return Err(self.runtime_error_with_code(
                            DiagnosticCode::RuntimeEnumVariant,
                            format!("unknown variant `{enum_name}::{variant_name}`"),
                        ));
                    };
                    let values = self.evaluate_values(arguments)?;
                    let payload = match (&variant.payload_type, values.as_slice()) {
                        (Some(expected), [value]) => {
                            self.ensure_type(value, expected, variant_name)?;
                            Some(Box::new(value.clone()))
                        }
                        (Some(expected), _) => {
                            return Err(self.runtime_error_with_code(
                                DiagnosticCode::InvalidFunctionCall,
                                format!(
                                    "variant `{enum_name}::{variant_name}` expects a payload of type {}",
                                    expected.name()
                                ),
                            ));
                        }
                        (None, []) => None,
                        (None, _) => {
                            return Err(self.runtime_error_with_code(
                                DiagnosticCode::InvalidFunctionCall,
                                format!(
                                    "unit variant `{enum_name}::{variant_name}` does not accept a payload"
                                ),
                            ));
                        }
                    };
                    Ok(Value::Enum(Rc::new(EnumValue {
                        enum_name: enum_name.clone(),
                        identity: definition.identity,
                        variant_name: variant_name.clone(),
                        payload,
                    })))
                } else {
                    Err(self.runtime_error_with_code(
                        DiagnosticCode::RuntimeMessage,
                        format!("unknown enum type `{enum_name}`"),
                    ))
                }
            }
            Expr::Match { value, arms } => {
                let value = self.evaluate(value)?;
                self.evaluate_match(value, arms)
            }
            Expr::Index { target, index } => {
                let target = self.evaluate(target)?;
                let index_value = self.evaluate(index)?;
                collections::index(&target, &index_value, self.current_span.as_ref())
            }
            Expr::Field { target, name } => match self.evaluate(target)? {
                Value::Hash(values) | Value::Tree(values) => {
                    values.get(name).cloned().ok_or_else(|| {
                        self.runtime_collection_error(format!("unknown field `{name}`"))
                    })
                }
                _ => Err(self.runtime_type_error("value has no fields")),
            },
        }
    }

    fn dispatch_message(
        &mut self,
        receiver: Value,
        message: &str,
        arguments: Vec<Value>,
    ) -> Result<Value, SimplyError> {
        if let Value::Enum(value) = &receiver {
            return Err(self.runtime_error_with_code(
                DiagnosticCode::RuntimeMessage,
                format!(
                    "enum value `{}::{}` does not support message dispatch",
                    value.enum_name, value.variant_name
                ),
            ));
        }
        if let Value::Struct(instance) = &receiver {
            let behavior = self
                .lookup_message(&instance.identity, message)
                .cloned()
                .ok_or_else(|| {
                    self.runtime_error_with_code(
                        DiagnosticCode::RuntimeMessage,
                        format!(
                            "message `{message}` is not understood by `{}`",
                            instance.type_name
                        ),
                    )
                })?;
            let fields = self
                .lookup_struct_identity(&instance.identity)
                .ok_or_else(|| {
                    self.runtime_error_with_code(
                        DiagnosticCode::RuntimeMessage,
                        format!("unknown struct type `{}`", instance.type_name),
                    )
                })?
                .fields
                .iter()
                .map(|field| {
                    instance
                        .fields
                        .borrow()
                        .get(&field.name)
                        .cloned()
                        .ok_or_else(|| {
                            self.runtime_error_with_code(
                                DiagnosticCode::RuntimeMessage,
                                format!(
                                    "struct `{}` is missing declared field `{}`",
                                    instance.type_name, field.name
                                ),
                            )
                        })
                })
                .collect::<Result<Vec<_>, _>>()?;
            if arguments.len() + behavior.field_count != behavior.function.parameters.len() {
                return Err(self.runtime_error_with_code(
                    DiagnosticCode::InvalidFunctionCall,
                    format!(
                        "message `{message}` expects {} arguments, got {}",
                        behavior.function.parameters.len() - behavior.field_count,
                        arguments.len()
                    ),
                ));
            }
            let mut values = fields;
            values.extend(arguments);
            return self.invoke_message_value(message, behavior.function, values, instance.clone());
        }

        let message_exists = self.lookup(message).is_some()
            || self
                .function_scopes
                .iter()
                .rev()
                .any(|scope| scope.contains_key(message));
        if !message_exists {
            return Err(self.runtime_error_with_code(
                DiagnosticCode::RuntimeMessage,
                format!(
                    "unknown message `{message}` for a {} receiver",
                    Self::value_type_name(&receiver)
                ),
            ));
        }

        let mut values = Vec::with_capacity(arguments.len() + 1);
        values.push(receiver);
        values.extend(arguments);
        self.invoke_function(message, values)
    }

    fn evaluate_match(
        &mut self,
        value: Value,
        arms: &[crate::ast::MatchArm],
    ) -> Result<Value, SimplyError> {
        for arm in arms {
            let Some(bindings) = self.match_value_pattern(&arm.pattern, &value)? else {
                continue;
            };
            self.push_scope();
            let result = (|| {
                for (binding, bound_value) in bindings {
                    self.variable_types.define(
                        binding.clone(),
                        Self::type_of_value(&bound_value),
                        false,
                    );
                    self.scopes
                        .define(binding, bound_value, false)
                        .map_err(|error| {
                            self.runtime_error_with_code(
                                DiagnosticCode::DuplicateDeclaration,
                                error,
                            )
                        })?;
                }
                if let Some(guard) = &arm.guard {
                    match self.evaluate(guard)? {
                        Value::Bool(true) => {}
                        Value::Bool(false) => return Ok(None),
                        other => {
                            return Err(self.runtime_type_error(format!(
                                "match guard must evaluate to Bool, found {}",
                                Self::value_type_name(&other)
                            )));
                        }
                    }
                }
                self.execute_statements(&arm.body)
                    .and_then(|flow| match flow {
                        Flow::None => arm
                            .result
                            .as_ref()
                            .map(|result| self.evaluate(result))
                            .unwrap_or(Ok(Value::Unit)),
                        Flow::Return(value) => Ok(value),
                        Flow::Break | Flow::Continue => Err(self.runtime_error_with_code(
                            DiagnosticCode::RuntimeControl,
                            "break/continue used outside a loop",
                        )),
                    })
                    .map(Some)
            })();
            self.pop_scope();
            if let Some(result) = result? {
                return Ok(result);
            }
        }
        Err(self.runtime_error(format!(
            "no match arm for {}",
            Self::value_type_name(&value)
        )))
    }

    fn csv_stream_version(
        file: &fs::File,
        path: &str,
        evaluator: &Self,
    ) -> Result<CsvStreamVersion, SimplyError> {
        let metadata = file
            .metadata()
            .map_err(|error| evaluator.file_error(Path::new(path), error))?;
        let modified = metadata
            .modified()
            .map_err(|error| evaluator.file_error(Path::new(path), error))?;
        Ok(CsvStreamVersion {
            length: metadata.len(),
            modified,
        })
    }

    fn seek_csv_stream(
        reader: &mut BufReader<fs::File>,
        path: &str,
        start_record: usize,
        start_offset: u64,
        source_version: Option<&CsvStreamVersion>,
        current_version: &CsvStreamVersion,
        evaluator: &Self,
    ) -> Result<(), SimplyError> {
        if source_version.is_none_or(|source| source == current_version) {
            reader
                .seek(SeekFrom::Start(start_offset))
                .map_err(|error| evaluator.file_error(Path::new(path), error))?;
            return Ok(());
        }

        reader
            .seek(SeekFrom::Start(0))
            .map_err(|error| evaluator.file_error(Path::new(path), error))?;
        for _ in 0..start_record {
            if csv::read_record(reader)
                .map_err(|error| evaluator.file_error(Path::new(path), error))?
                .is_none()
            {
                break;
            }
        }
        Ok(())
    }

    fn construct_struct(
        &self,
        name: &str,
        definition: &StructDefinition,
        values: Vec<Value>,
    ) -> Result<Value, SimplyError> {
        if values.len() != definition.fields.len() {
            return Err(self.runtime_error_with_code(
                DiagnosticCode::InvalidFunctionCall,
                format!(
                    "struct `{name}` expects {} fields, got {}",
                    definition.fields.len(),
                    values.len()
                ),
            ));
        }
        let mut fields = BTreeMap::new();
        for (field, value) in definition.fields.iter().zip(values) {
            self.ensure_type(&value, &field.field_type, &field.name)?;
            fields.insert(field.name.clone(), value);
        }
        Ok(Value::Struct(Rc::new(StructInstance {
            identity: definition.identity.clone(),
            type_name: name.into(),
            fields: Rc::new(RefCell::new(fields)),
        })))
    }

    fn invoke_function(&mut self, name: &str, values: Vec<Value>) -> Result<Value, SimplyError> {
        let function = match self.lookup(name).cloned() {
            Some(Value::Function(function)) => function,
            Some(_) => {
                return Err(self.runtime_error_with_code(
                    DiagnosticCode::RuntimeMessage,
                    format!("`{name}` is not a function"),
                ));
            }
            None => {
                let function = self
                    .function_scopes
                    .iter()
                    .rev()
                    .find_map(|scope| scope.get(name))
                    .cloned()
                    .ok_or_else(|| {
                        let description = if name.chars().next().is_some_and(char::is_uppercase) {
                            format!("unknown struct type `{name}`")
                        } else {
                            format!("unknown function `{name}`")
                        };
                        self.runtime_error_with_code(DiagnosticCode::RuntimeName, description)
                    })?;
                self.track_function(Rc::new(FunctionValue {
                    name: Some(name.to_owned()),
                    parameters: function.parameters.clone(),
                    return_type: function.return_type.clone(),
                    body: Arc::clone(&function.body),
                    captures: RefCell::new(HashMap::new()),
                    source: function.source.clone(),
                }))
            }
        };
        self.invoke_function_value(name, function, values)
    }

    fn invoke_function_value(
        &mut self,
        name: &str,
        function: Rc<FunctionValue>,
        values: Vec<Value>,
    ) -> Result<Value, SimplyError> {
        self.invoke_function_value_with_message_state(name, function, values, None)
    }

    fn invoke_message_value(
        &mut self,
        name: &str,
        function: Rc<FunctionValue>,
        values: Vec<Value>,
        instance: Rc<StructInstance>,
    ) -> Result<Value, SimplyError> {
        self.invoke_function_value_with_message_state(name, function, values, Some(instance))
    }

    fn invoke_function_value_with_message_state(
        &mut self,
        name: &str,
        function: Rc<FunctionValue>,
        values: Vec<Value>,
        message_instance: Option<Rc<StructInstance>>,
    ) -> Result<Value, SimplyError> {
        if function.parameters.len() != values.len() {
            return Err(self.runtime_error_with_code(
                DiagnosticCode::RuntimeArgument,
                format!(
                    "function `{name}` expects {} arguments, got {}",
                    function.parameters.len(),
                    values.len()
                ),
            ));
        }
        for ((parameter, expected, _), value) in function.parameters.iter().zip(&values) {
            if let Some(expected) = expected {
                self.ensure_type(value, expected, parameter)?;
            }
        }
        if self.call_depth >= limits::MAX_CALL_DEPTH {
            return Err(self.runtime_error_with_code(
                DiagnosticCode::RuntimeLimit,
                format!(
                    "function call depth exceeds the limit of {}",
                    limits::MAX_CALL_DEPTH
                ),
            ));
        }
        let needs_function_scope = !function.captures.borrow().is_empty()
            || function.name.as_ref().is_some_and(|function_name| {
                !matches!(
                    self.lookup(function_name),
                    Some(Value::Function(current)) if Rc::ptr_eq(current, &function)
                )
            });
        self.call_depth += 1;
        let function_source = function.source.clone();
        let previous_source = std::mem::replace(
            &mut self.current_source,
            function_source
                .as_ref()
                .map(|context| Rc::clone(&context.source)),
        );
        let mut scopes_pushed = 0;
        let result = (|| {
            if needs_function_scope {
                self.push_scope();
                scopes_pushed += 1;
                if let Some(function_name) = &function.name {
                    let value = Value::Function(function.clone());
                    self.variable_types.define(
                        function_name.clone(),
                        Self::type_of_value(&value),
                        false,
                    );
                    self.scopes
                        .define(function_name.clone(), value, false)
                        .map_err(|error| {
                            self.runtime_error_with_code(
                                DiagnosticCode::DuplicateDeclaration,
                                error,
                            )
                        })?;
                }
                for (capture, value) in function.captures.borrow().iter() {
                    self.variable_types
                        .define(capture.clone(), Self::type_of_value(value), false);
                    self.scopes
                        .define(capture.clone(), value.clone(), false)
                        .map_err(|error| {
                            self.runtime_error_with_code(
                                DiagnosticCode::DuplicateDeclaration,
                                error,
                            )
                        })?;
                }
            }
            self.push_scope();
            scopes_pushed += 1;
            for ((parameter, _, mutable), value) in function.parameters.iter().zip(values) {
                self.variable_types.define(
                    parameter.clone(),
                    Self::type_of_value(&value),
                    *mutable,
                );
                self.scopes
                    .define(parameter.clone(), value, *mutable)
                    .map_err(|error| {
                        self.runtime_error_with_code(DiagnosticCode::DuplicateDeclaration, error)
                    })?;
            }
            let is_message_invocation = message_instance.is_some();
            if let Some(instance) = message_instance.as_ref() {
                self.active_message_states.push(ActiveMessageState {
                    instance: instance.clone(),
                    scope_index: self.scopes.current_scope_index(),
                });
            }
            let result = self.execute_statements(&function.body);
            if is_message_invocation {
                self.active_message_states.pop();
            }
            Ok(match result {
                Ok(flow) => match flow {
                    Flow::None => Value::Unit,
                    Flow::Return(value) => value,
                    Flow::Break | Flow::Continue => {
                        return Err(self.runtime_error_with_code(
                            DiagnosticCode::RuntimeControl,
                            "break/continue used outside a loop",
                        ));
                    }
                },
                Err(error) => return Err(error),
            })
        })();
        for _ in 0..scopes_pushed {
            self.pop_scope();
        }
        self.current_source = previous_source;
        self.call_depth -= 1;
        let result = result.and_then(|result| {
            if let Some(expected) = &function.return_type {
                self.ensure_type(&result, expected, name)?;
            }
            Ok(result)
        });
        match function_source {
            Some(context) => result.map_err(|error| {
                error.in_source(context.filename, context.source.as_ref().to_owned())
            }),
            None => result,
        }
    }

    fn evaluate_values(&mut self, expressions: &[Expr]) -> Result<Vec<Value>, SimplyError> {
        expressions
            .iter()
            .map(|expression| self.evaluate(expression))
            .collect()
    }

    fn evaluate_pipeline(
        &mut self,
        mut values: Vec<Value>,
        steps: &[PipelineStep],
        sum_type: Type,
    ) -> Result<Value, SimplyError> {
        if steps
            .iter()
            .any(|step| matches!(step, PipelineStep::Chunk(_)))
            && !steps.iter().any(|step| {
                matches!(
                    step,
                    PipelineStep::Parallel(_) | PipelineStep::Checkpoint(_)
                )
            })
        {
            return Err(self.runtime_argument_error("`chunk` requires `parallel` or `checkpoint`"));
        }
        if steps
            .iter()
            .any(|step| matches!(step, PipelineStep::Checkpoint(_)))
        {
            return Err(self.runtime_argument_error(
                "`checkpoint` requires a `csv_rows` source and `write_csv` terminal",
            ));
        }
        self.push_scope();
        let result = self.evaluate_pipeline_steps(&mut values, steps, sum_type);
        self.pop_scope();
        result
    }

    fn evaluate_range_pipeline(
        &mut self,
        start: i64,
        end: i64,
        step: i64,
        steps: &[PipelineStep],
        sum_type: Type,
    ) -> Result<Value, SimplyError> {
        if steps
            .iter()
            .any(|step| matches!(step, PipelineStep::Chunk(_)))
            && !steps.iter().any(|step| {
                matches!(
                    step,
                    PipelineStep::Parallel(_) | PipelineStep::Checkpoint(_)
                )
            })
        {
            return Err(self.runtime_argument_error("`chunk` requires `parallel` or `checkpoint`"));
        }
        if steps
            .iter()
            .any(|step| matches!(step, PipelineStep::Checkpoint(_)))
        {
            return Err(self.runtime_argument_error(
                "`checkpoint` requires a `csv_rows` source and `write_csv` terminal",
            ));
        }
        if let Some(terminal) = steps.last()
            && matches!(
                terminal,
                PipelineStep::Count
                    | PipelineStep::Sum
                    | PipelineStep::Average
                    | PipelineStep::Min
                    | PipelineStep::Max
                    | PipelineStep::Any
                    | PipelineStep::All
            )
            && steps[..steps.len() - 1].iter().all(|step| match step {
                PipelineStep::Where(expression) | PipelineStep::Derive(expression) => {
                    is_parallel_safe_expression(expression)
                }
                _ => false,
            })
        {
            return self.evaluate_scalar_range_pipeline(
                start,
                end,
                step,
                &steps[..steps.len() - 1],
                terminal,
                sum_type,
            );
        }
        self.push_scope();
        let result = self.evaluate_streaming_pipeline_with_sum_type(
            Value::range_values(start, end, step),
            steps,
            sum_type,
        );
        self.pop_scope();
        result
    }

    fn evaluate_scalar_range_pipeline(
        &self,
        start: i64,
        end: i64,
        step: i64,
        transforms: &[PipelineStep],
        terminal: &PipelineStep,
        sum_type: Type,
    ) -> Result<Value, SimplyError> {
        let mut count = 0i64;
        let mut total = Value::Int(0);
        let mut sum = SumAccumulator::new(sum_type);
        let mut minimum = None;
        let mut maximum = None;
        let mut any_result = false;
        let mut all_result = true;

        for value in Value::range_values(start, end, step) {
            let mut current = Some(value);
            for step in transforms {
                let Some(item) = current.take() else {
                    break;
                };
                match step {
                    PipelineStep::Where(expression) => {
                        match self.evaluate_scalar_pipeline_expression(expression, &item)? {
                            Value::Bool(true) => current = Some(item),
                            Value::Bool(false) => {}
                            _ => {
                                return Err(self.runtime_type_error(
                                    "pipeline `where` condition must return a boolean",
                                ));
                            }
                        }
                    }
                    PipelineStep::Derive(expression) => {
                        current =
                            Some(self.evaluate_scalar_pipeline_expression(expression, &item)?);
                    }
                    _ => unreachable!("range fast path accepts only pure scalar transforms"),
                }
            }
            let Some(value) = current else {
                continue;
            };
            match terminal {
                PipelineStep::Count => count += 1,
                PipelineStep::Sum => sum.add(value, self.current_span.as_ref())?,
                PipelineStep::Average => {
                    total = operations::binary(
                        total,
                        &BinaryOperator::Add,
                        value,
                        self.current_span.as_ref(),
                    )?;
                    count += 1;
                }
                PipelineStep::Min => {
                    update_extreme(&mut minimum, value, false, self.current_span.as_ref())?;
                }
                PipelineStep::Max => {
                    update_extreme(&mut maximum, value, true, self.current_span.as_ref())?;
                }
                PipelineStep::Any => match value {
                    Value::Bool(true) => return Ok(Value::Bool(true)),
                    Value::Bool(false) => any_result = false,
                    _ => {
                        return Err(self
                            .runtime_type_error("`any` pipeline terminal requires boolean items"));
                    }
                },
                PipelineStep::All => match value {
                    Value::Bool(false) => return Ok(Value::Bool(false)),
                    Value::Bool(true) => all_result = true,
                    _ => {
                        return Err(self
                            .runtime_type_error("`all` pipeline terminal requires boolean items"));
                    }
                },
                _ => unreachable!("range fast path accepts only aggregate terminals"),
            }
        }

        match terminal {
            PipelineStep::Count => Ok(Value::Int(count)),
            PipelineStep::Sum => Ok(sum.finish()),
            PipelineStep::Average if count == 0 => {
                Err(self.runtime_collection_error("average requires at least one numeric value"))
            }
            PipelineStep::Average => Ok(Value::Float(
                total_to_f64(total, self.current_span.as_ref())? / count as f64,
            )),
            PipelineStep::Min => minimum.ok_or_else(|| {
                self.runtime_collection_error("min requires at least one numeric value")
            }),
            PipelineStep::Max => maximum.ok_or_else(|| {
                self.runtime_collection_error("max requires at least one numeric value")
            }),
            PipelineStep::Any => Ok(Value::Bool(any_result)),
            PipelineStep::All => Ok(Value::Bool(all_result)),
            _ => unreachable!("range fast path accepts only aggregate terminals"),
        }
    }

    fn evaluate_scalar_pipeline_expression(
        &self,
        expression: &Expr,
        item: &Value,
    ) -> Result<Value, SimplyError> {
        match expression {
            Expr::Literal(Literal::String(value)) => Ok(Value::String(value.clone())),
            Expr::Literal(Literal::Int(value)) => Ok(Value::Int(*value)),
            Expr::Literal(Literal::Float(value)) => Ok(Value::Float(*value)),
            Expr::Literal(Literal::Bool(value)) => Ok(Value::Bool(*value)),
            Expr::Identifier(name) if name == "item" => Ok(item.clone()),
            Expr::Unary { operator, operand } => operations::unary(
                self.evaluate_scalar_pipeline_expression(operand, item)?,
                operator,
                self.current_span.as_ref(),
            ),
            Expr::Binary {
                left,
                operator,
                right,
            } => {
                let left = self.evaluate_scalar_pipeline_expression(left, item)?;
                if matches!(operator, BinaryOperator::And | BinaryOperator::Or)
                    && let Value::Bool(value) = left
                    && ((*operator == BinaryOperator::And && !value)
                        || (*operator == BinaryOperator::Or && value))
                {
                    return Ok(Value::Bool(value));
                }
                operations::binary(
                    left,
                    operator,
                    self.evaluate_scalar_pipeline_expression(right, item)?,
                    self.current_span.as_ref(),
                )
            }
            _ => Err(SimplyError::Runtime {
                span: self.current_span.clone().unwrap_or_else(|| Span::new(0, 0)),
                code: DiagnosticCode::RuntimeGeneral,
                message: "expression is outside the scalar pipeline fast path".into(),
            }),
        }
    }

    fn evaluate_csv_pipeline(
        &mut self,
        input_path: &str,
        start_record: usize,
        start_offset: u64,
        source_version: Option<&CsvStreamVersion>,
        steps: &[PipelineStep],
        sum_type: Type,
    ) -> Result<Value, SimplyError> {
        if steps
            .iter()
            .any(|step| matches!(step, PipelineStep::Chunk(_)))
            && !steps
                .iter()
                .any(|step| matches!(step, PipelineStep::Checkpoint(_)))
        {
            return Err(self.runtime_argument_error("`chunk` requires `parallel` or `checkpoint`"));
        }
        if steps
            .iter()
            .any(|step| matches!(step, PipelineStep::Parallel(_)))
        {
            return Err(self.runtime_argument_error(
                "`parallel` does not support CSV row collections or output terminals",
            ));
        }
        if steps.iter().any(|step| {
            matches!(
                step,
                PipelineStep::Take(_)
                    | PipelineStep::Skip(_)
                    | PipelineStep::StepBy(_)
                    | PipelineStep::TakeWhile(_)
                    | PipelineStep::DropWhile(_)
                    | PipelineStep::Distinct
            )
        }) && steps
            .iter()
            .any(|step| matches!(step, PipelineStep::Checkpoint(_)))
        {
            return Err(self.runtime_argument_error(
                "`take`, `skip`, `step_by`, `take_while`, `drop_while`, and `distinct` cannot be combined with `checkpoint`"
            ));
        }
        let terminal = steps.last().ok_or_else(|| {
            self.runtime_argument_error("csv_rows pipeline requires a terminal step")
        })?;
        let output_path = match terminal {
            PipelineStep::WriteCsv(path_expression) => {
                let output_path = match self.evaluate(path_expression)? {
                    Value::String(path) => path,
                    _ => return Err(self.runtime_type_error("write_csv path must be a string")),
                };
                Some(output_path)
            }
            PipelineStep::Sum
            | PipelineStep::Count
            | PipelineStep::Average
            | PipelineStep::Min
            | PipelineStep::Max
            | PipelineStep::Any
            | PipelineStep::All
            | PipelineStep::Partition { .. } => None,
            _ => {
                return Err(self.runtime_argument_error(
                    "csv_rows pipeline must end with an aggregate, partition, or write_csv(\"path\")",
                ));
            }
        };
        let input = fs::File::open(input_path)
            .map_err(|error| self.file_error(Path::new(input_path), error))?;
        let chunk_size = steps.iter().find_map(|step| match step {
            PipelineStep::Chunk(size) => Some(*size),
            _ => None,
        });
        let chunk_size = chunk_size
            .map(|size| {
                usize::try_from(size)
                    .map_err(|_| self.runtime_argument_error("chunk size is out of bounds"))
            })
            .transpose()?;
        if let Some(size) = chunk_size
            && (size == 0 || size > limits::MAX_CHUNK_SIZE)
        {
            return Err(self.runtime_argument_error(format!(
                "chunk size must be between 1 and {}",
                limits::MAX_CHUNK_SIZE
            )));
        }
        let checkpoint_path = steps.iter().find_map(|step| match step {
            PipelineStep::Checkpoint(path) => Some(path),
            _ => None,
        });
        self.push_scope();
        let result = (|| {
            let checkpoint_path = match checkpoint_path {
                Some(path) => match self.evaluate(path)? {
                    Value::String(path) => Some(path),
                    _ => return Err(self.runtime_type_error("checkpoint path must be a string")),
                },
                None => None,
            };
            if let Some(output_path) = output_path.as_deref()
                && checkpoint::paths_are_same(input_path, output_path)
                    .map_err(|error| self.checkpoint_runtime_error(error))?
            {
                return Err(self.runtime_argument_error(
                    "`csv_rows` input and `write_csv` output must be different files",
                ));
            }
            if let Some(checkpoint_path) = checkpoint_path.as_deref() {
                let temporary_checkpoint = format!("{checkpoint_path}.tmp");
                for protected_path in [Some(input_path), output_path.as_deref()]
                    .into_iter()
                    .flatten()
                {
                    if checkpoint::paths_are_same(checkpoint_path, protected_path)
                        .map_err(|error| self.checkpoint_runtime_error(error))?
                        || checkpoint::paths_are_same(&temporary_checkpoint, protected_path)
                            .map_err(|error| self.checkpoint_runtime_error(error))?
                    {
                        return Err(self.runtime_argument_error(
                            "`checkpoint` and its temporary file must not overlap the CSV input or output"
                        ));
                    }
                }
            }
            let checkpoint_state = checkpoint_path
                .as_deref()
                .map(checkpoint::read_checkpoint)
                .transpose()
                .map_err(|error| self.checkpoint_runtime_error(error))?
                .flatten();
            if checkpoint_path.is_some() && output_path.is_none() {
                return Err(
                    self.runtime_argument_error("`checkpoint` requires a `write_csv` terminal")
                );
            }
            let resume_at = checkpoint_state
                .as_ref()
                .map_or(0, |checkpoint| checkpoint.position);
            if let Some(state) = checkpoint_state.as_ref() {
                let output_path = output_path.as_deref().ok_or_else(|| {
                    self.runtime_argument_error("checkpoint output path is missing")
                })?;
                checkpoint::validate_checkpoint_source(state, input_path, output_path)
                    .map_err(|error| self.checkpoint_runtime_error(error))?;
                checkpoint::validate_checkpoint_output(state, output_path)
                    .map_err(|error| self.checkpoint_runtime_error(error))?;
            }
            if resume_at > 0 && !matches!(terminal, PipelineStep::WriteCsv(_)) {
                return Err(self.runtime_argument_error(
                    "checkpoint resume requires a `write_csv` terminal; aggregate state is not checkpointed yet",
                ));
            }
            let mut output = match output_path.as_deref() {
                Some(path) if resume_at > 0 => Some(
                    fs::OpenOptions::new()
                        .append(true)
                        .open(path)
                        .map_err(|error| self.file_error(Path::new(path), error))?,
                ),
                Some(path) => Some(
                    fs::File::create(path)
                        .map_err(|error| self.file_error(Path::new(path), error))?,
                ),
                None => None,
            };
            if let (Some(output), Some(output_len), Some(path)) = (
                output.as_mut(),
                checkpoint_state
                    .as_ref()
                    .map(|checkpoint| checkpoint.output_len),
                output_path.as_deref(),
            ) {
                let output_metadata = output
                    .metadata()
                    .map_err(|error| self.file_error(Path::new(path), error))?;
                if output_len > output_metadata.len() {
                    return Err(self.runtime_error_with_code(
                        DiagnosticCode::RuntimeGeneral,
                        format!(
                            "checkpoint output length exceeds the current output file `{path}`"
                        ),
                    ));
                }
                output
                    .set_len(output_len)
                    .map_err(|error| self.file_error(Path::new(path), error))?;
            }
            let interval = chunk_size.unwrap_or(1).max(1);
            let mut total = Value::Int(0);
            let mut sum = SumAccumulator::new(sum_type.clone());
            let mut count = 0i64;
            let mut minimum: Option<Value> = None;
            let mut maximum: Option<Value> = None;
            let mut any_result = false;
            let mut all_result = true;
            let mut partitioned = match terminal {
                PipelineStep::Partition { rules, .. } => Some(empty_partition_categories(rules)),
                _ => None,
            };
            let mut reader = BufReader::new(input);
            let current_version = Self::csv_stream_version(reader.get_ref(), input_path, self)?;
            Self::seek_csv_stream(
                &mut reader,
                input_path,
                start_record,
                start_offset,
                source_version,
                &current_version,
                self,
            )?;
            let mut index = start_record;
            let mut take_counts = vec![0i64; steps.len() - 1];
            let mut skip_counts = vec![0i64; steps.len() - 1];
            let mut step_by_counts = vec![0i64; steps.len() - 1];
            let mut seen_values = vec![Vec::new(); steps.len() - 1];
            let mut drop_while_done = vec![false; steps.len() - 1];
            loop {
                if steps[..steps.len() - 1]
                    .iter()
                    .any(|step| matches!(step, PipelineStep::Take(0)))
                {
                    break;
                }
                let Some(record) = csv::read_record(&mut reader)
                    .map_err(|error| self.file_error(Path::new(input_path), error))?
                else {
                    break;
                };
                if index < resume_at {
                    index += 1;
                    continue;
                }
                let line = record.strip_suffix('\n').unwrap_or(&record);
                let line = line.strip_suffix('\r').unwrap_or(line);
                let row = csv::parse_record(line).map_err(|message| {
                    self.runtime_error(format!("invalid CSV row in `{input_path}`: {message}"))
                })?;
                let mut current = Some(Value::List(shared_values(row)));
                let mut stop_after_record = false;
                let mut stop_source = false;
                for (step_index, step) in steps[..steps.len() - 1].iter().enumerate() {
                    match step {
                        PipelineStep::Where(expression) => {
                            let item = current.take().ok_or_else(|| {
                                self.runtime_error("pipeline item was lost".into())
                            })?;
                            current = self.evaluate_pipeline_where_item(item, expression)?;
                            if current.is_none() {
                                break;
                            }
                        }
                        PipelineStep::Derive(expression) => {
                            let item = current.take().ok_or_else(|| {
                                self.runtime_error("pipeline item was lost".into())
                            })?;
                            current = Some(self.evaluate_pipeline_item(item, expression)?);
                        }
                        PipelineStep::Take(limit) => {
                            take_counts[step_index] += 1;
                            stop_after_record |= take_counts[step_index] >= *limit;
                        }
                        PipelineStep::Skip(limit) => {
                            if skip_counts[step_index] < *limit {
                                skip_counts[step_index] += 1;
                                current = None;
                                break;
                            }
                        }
                        PipelineStep::StepBy(interval) => {
                            if step_by_counts[step_index] > 0 {
                                step_by_counts[step_index] -= 1;
                                current = None;
                                break;
                            }
                            step_by_counts[step_index] = interval - 1;
                        }
                        PipelineStep::TakeWhile(expression) => {
                            let item = current.take().ok_or_else(|| {
                                self.runtime_error("pipeline item was lost".into())
                            })?;
                            match self.evaluate_pipeline_item(item.clone(), expression)? {
                                Value::Bool(true) => current = Some(item),
                                Value::Bool(false) => {
                                    current = None;
                                    stop_source = true;
                                    break;
                                }
                                _ => {
                                    return Err(self.runtime_type_error(
                                        "pipeline `take_while` condition must return a boolean",
                                    ));
                                }
                            }
                        }
                        PipelineStep::DropWhile(expression) => {
                            if !drop_while_done[step_index] {
                                let item = current.take().ok_or_else(|| {
                                    self.runtime_error("pipeline item was lost".into())
                                })?;
                                match self.evaluate_pipeline_item(item.clone(), expression)? {
                                    Value::Bool(true) => {
                                        current = None;
                                        break;
                                    }
                                    Value::Bool(false) => {
                                        drop_while_done[step_index] = true;
                                        current = Some(item);
                                    }
                                    _ => {
                                        return Err(self.runtime_type_error(
                                            "pipeline `drop_while` condition must return a boolean",
                                        ));
                                    }
                                }
                            }
                        }
                        PipelineStep::Distinct => {
                            let item = current.take().ok_or_else(|| {
                                self.runtime_error("pipeline item was lost".into())
                            })?;
                            if seen_values[step_index].contains(&item) {
                                current = None;
                                break;
                            }
                            seen_values[step_index].push(item.clone());
                            current = Some(item);
                        }
                        PipelineStep::Sum
                        | PipelineStep::Count
                        | PipelineStep::Average
                        | PipelineStep::Min
                        | PipelineStep::Max
                        | PipelineStep::Any
                        | PipelineStep::All
                        | PipelineStep::Partition { .. }
                        | PipelineStep::WriteCsv(_) => {
                            unreachable!("terminal step is excluded from transforms")
                        }
                        PipelineStep::Chunk(_)
                        | PipelineStep::Parallel(_)
                        | PipelineStep::Checkpoint(_) => {}
                    }
                }
                let Some(value) = current else {
                    if stop_source {
                        break;
                    }
                    index += 1;
                    checkpoint::checkpoint_progress(
                        checkpoint_path.as_deref(),
                        index,
                        interval,
                        output.as_mut(),
                        input_path,
                        output_path.as_deref(),
                    )
                    .map_err(|error| self.checkpoint_runtime_error(error))?;
                    if stop_after_record {
                        break;
                    }
                    continue;
                };
                if let PipelineStep::Partition { item, rules } = terminal {
                    if let Some((category, value)) =
                        self.evaluate_partition_item(value, item, rules)?
                    {
                        let values = partitioned
                            .as_mut()
                            .and_then(|categories| categories.get_mut(&category))
                            .ok_or_else(|| {
                                self.runtime_error(format!(
                                    "partition category `{category}` was not initialized"
                                ))
                            })?;
                        values.push(value);
                    }
                    index += 1;
                    checkpoint::checkpoint_progress(
                        checkpoint_path.as_deref(),
                        index,
                        interval,
                        output.as_mut(),
                        input_path,
                        output_path.as_deref(),
                    )
                    .map_err(|error| self.checkpoint_runtime_error(error))?;
                    if stop_after_record {
                        break;
                    }
                    continue;
                }
                match terminal {
                    PipelineStep::Sum => {
                        sum.add(value, self.current_span.as_ref())?;
                    }
                    PipelineStep::Count => count += 1,
                    PipelineStep::Average => {
                        total = operations::binary(
                            total,
                            &BinaryOperator::Add,
                            value,
                            self.current_span.as_ref(),
                        )?;
                        count += 1;
                    }
                    PipelineStep::Min => {
                        update_extreme(&mut minimum, value, false, self.current_span.as_ref())?
                    }
                    PipelineStep::Max => {
                        update_extreme(&mut maximum, value, true, self.current_span.as_ref())?
                    }
                    PipelineStep::Any => match value {
                        Value::Bool(value) => any_result |= value,
                        _ => {
                            return Err(self.runtime_type_error(
                                "`any` pipeline terminal requires boolean items",
                            ));
                        }
                    },
                    PipelineStep::All => match value {
                        Value::Bool(value) => all_result &= value,
                        _ => {
                            return Err(self.runtime_type_error(
                                "`all` pipeline terminal requires boolean items",
                            ));
                        }
                    },
                    PipelineStep::WriteCsv(_) => {
                        let values = match value {
                            Value::Array(values) | Value::List(values) | Value::Tuple(values) => {
                                values
                            }
                            _ => {
                                return Err(self.runtime_type_error(
                                    "write_csv requires derive to produce a row collection",
                                ));
                            }
                        };
                        csv::write_row(
                            output
                                .as_mut()
                                .expect("write_csv output should be initialized"),
                            values.as_slice(),
                        )
                        .map_err(|message| {
                            self.runtime_error_with_code(DiagnosticCode::RuntimeIo, message)
                        })?;
                        if stop_after_record {
                            break;
                        }
                    }
                    _ => unreachable!(),
                }
                if (matches!(terminal, PipelineStep::Any) && any_result)
                    || (matches!(terminal, PipelineStep::All) && !all_result)
                {
                    break;
                }
                index += 1;
                checkpoint::checkpoint_progress(
                    checkpoint_path.as_deref(),
                    index,
                    interval,
                    output.as_mut(),
                    input_path,
                    output_path.as_deref(),
                )
                .map_err(|error| self.checkpoint_runtime_error(error))?;
            }
            let terminal_result = if let Some(output) = output.as_mut() {
                output.flush().map_err(|error| {
                    self.runtime_error_with_code(
                        DiagnosticCode::RuntimeIo,
                        format!("could not flush CSV: {error}"),
                    )
                })?;
                Ok(Value::Unit)
            } else if matches!(terminal, PipelineStep::Count) {
                Ok(Value::Int(count))
            } else if matches!(terminal, PipelineStep::Average) {
                if count == 0 {
                    Err(self
                        .runtime_collection_error("average requires at least one numeric value"))
                } else {
                    Ok(Value::Float(
                        total_to_f64(total, self.current_span.as_ref())? / count as f64,
                    ))
                }
            } else if matches!(terminal, PipelineStep::Min) {
                minimum.ok_or_else(|| {
                    self.runtime_collection_error("min requires at least one numeric value")
                })
            } else if matches!(terminal, PipelineStep::Max) {
                maximum.ok_or_else(|| {
                    self.runtime_collection_error("max requires at least one numeric value")
                })
            } else if matches!(terminal, PipelineStep::Any) {
                Ok(Value::Bool(any_result))
            } else if matches!(terminal, PipelineStep::All) {
                Ok(Value::Bool(all_result))
            } else if let PipelineStep::Partition { .. } = terminal {
                match partitioned.take() {
                    Some(categories) => Ok(partition_result(categories)),
                    None => Err(self.runtime_error("partition result was not initialized".into())),
                }
            } else if matches!(terminal, PipelineStep::Sum) {
                Ok(sum.finish())
            } else {
                Ok(total)
            };
            if terminal_result.is_ok()
                && let Some(path) = checkpoint_path.as_deref()
            {
                checkpoint::remove_checkpoint(path)
                    .map_err(|error| self.checkpoint_runtime_error(error))?;
            }
            terminal_result
        })();
        self.pop_scope();
        result
    }

    fn evaluate_pipeline_steps(
        &mut self,
        values: &mut Vec<Value>,
        steps: &[PipelineStep],
        sum_type: Type,
    ) -> Result<Value, SimplyError> {
        if steps.iter().any(|step| {
            matches!(
                step,
                PipelineStep::Chunk(_)
                    | PipelineStep::Checkpoint(_)
                    | PipelineStep::Take(_)
                    | PipelineStep::Skip(_)
                    | PipelineStep::StepBy(_)
                    | PipelineStep::TakeWhile(_)
                    | PipelineStep::Distinct
            )
        }) {
            return self.evaluate_streaming_pipeline_with_sum_type(
                std::mem::take(values),
                steps,
                sum_type,
            );
        }
        if matches!(steps.last(), Some(PipelineStep::Partition { .. })) {
            return self.evaluate_streaming_pipeline_with_sum_type(
                std::mem::take(values),
                steps,
                sum_type,
            );
        }
        if !matches!(
            steps.last(),
            Some(
                PipelineStep::Count
                    | PipelineStep::Sum
                    | PipelineStep::Average
                    | PipelineStep::Min
                    | PipelineStep::Max
                    | PipelineStep::Any
                    | PipelineStep::All
                    | PipelineStep::Partition { .. }
            )
        ) {
            return self.evaluate_streaming_pipeline_with_sum_type(
                std::mem::take(values),
                steps,
                sum_type,
            );
        }
        if steps.len() >= 2
            && matches!(
                steps.last(),
                Some(
                    PipelineStep::Count
                        | PipelineStep::Sum
                        | PipelineStep::Average
                        | PipelineStep::Min
                        | PipelineStep::Max
                )
            )
            && steps[..steps.len() - 1]
                .iter()
                .all(|step| matches!(step, PipelineStep::Where(_) | PipelineStep::Derive(_)))
        {
            return self.evaluate_fused_pipeline(
                values,
                &steps[..steps.len() - 1],
                steps.last(),
                sum_type,
            );
        }
        if steps.len() == 2 {
            match (&steps[0], &steps[1]) {
                (PipelineStep::Where(expression), PipelineStep::Count) => {
                    let mut count = 0;
                    for value in values.drain(..) {
                        match self.evaluate_pipeline_item(value, expression)? {
                            Value::Bool(true) => count += 1,
                            Value::Bool(false) => {}
                            _ => {
                                return Err(self.runtime_type_error(
                                    "pipeline `where` condition must return a boolean",
                                ));
                            }
                        }
                    }
                    return Ok(Value::Int(count));
                }
                (PipelineStep::Derive(expression), PipelineStep::Count) => {
                    let mut count = 0;
                    for value in values.drain(..) {
                        self.evaluate_pipeline_item(value, expression)?;
                        count += 1;
                    }
                    return Ok(Value::Int(count));
                }
                (PipelineStep::Derive(expression), PipelineStep::Sum) => {
                    let mut total = SumAccumulator::new(sum_type.clone());
                    for value in values.drain(..) {
                        let mapped = self.evaluate_pipeline_item(value, expression)?;
                        total.add(mapped, self.current_span.as_ref())?;
                    }
                    return Ok(total.finish());
                }
                _ => {}
            }
        }
        for step in steps {
            match step {
                PipelineStep::Where(expression) => {
                    let mut kept = Vec::new();
                    for value in values.drain(..) {
                        if let Some(item) = self.evaluate_pipeline_where_item(value, expression)? {
                            kept.push(item);
                        }
                    }
                    *values = kept;
                }
                PipelineStep::Derive(expression) => {
                    let mut mapped = Vec::new();
                    for value in values.drain(..) {
                        mapped.push(self.evaluate_pipeline_item(value, expression)?);
                    }
                    *values = mapped;
                }
                PipelineStep::Sum => {
                    let mut total = SumAccumulator::new(sum_type.clone());
                    for value in values.drain(..) {
                        total.add(value, self.current_span.as_ref())?;
                    }
                    return Ok(total.finish());
                }
                PipelineStep::Count => return Ok(Value::Int(values.len() as i64)),
                PipelineStep::Average | PipelineStep::Min | PipelineStep::Max => {
                    return self.evaluate_streaming_pipeline_with_sum_type(
                        std::mem::take(values),
                        steps,
                        sum_type.clone(),
                    );
                }
                PipelineStep::Any | PipelineStep::All => {
                    return self.evaluate_streaming_pipeline_with_sum_type(
                        std::mem::take(values),
                        steps,
                        sum_type.clone(),
                    );
                }
                PipelineStep::Partition { .. } => {
                    return self.evaluate_streaming_pipeline_with_sum_type(
                        std::mem::take(values),
                        steps,
                        sum_type.clone(),
                    );
                }
                PipelineStep::WriteCsv(_) => {
                    return Err(self.runtime_argument_error(
                        "`write_csv` requires a `csv_rows` streaming source",
                    ));
                }
                PipelineStep::Chunk(_)
                | PipelineStep::Parallel(_)
                | PipelineStep::Checkpoint(_)
                | PipelineStep::Take(_)
                | PipelineStep::Skip(_)
                | PipelineStep::StepBy(_)
                | PipelineStep::TakeWhile(_)
                | PipelineStep::DropWhile(_)
                | PipelineStep::Distinct => {
                    // These controls are consumed by the streaming path.
                }
            }
        }
        Ok(Value::List(shared_values(std::mem::take(values))))
    }

    #[cfg(test)]
    fn evaluate_streaming_pipeline<I>(
        &mut self,
        values: I,
        steps: &[PipelineStep],
    ) -> Result<Value, SimplyError>
    where
        I: IntoIterator<Item = Value>,
    {
        self.evaluate_streaming_pipeline_with_sum_type(values, steps, Type::Unknown)
    }

    fn evaluate_streaming_pipeline_with_sum_type<I>(
        &mut self,
        values: I,
        steps: &[PipelineStep],
        sum_type: Type,
    ) -> Result<Value, SimplyError>
    where
        I: IntoIterator<Item = Value>,
    {
        if steps
            .iter()
            .any(|step| matches!(step, PipelineStep::Checkpoint(_)))
        {
            return Err(self.runtime_argument_error(
                "`checkpoint` requires a `csv_rows` source and `write_csv` terminal",
            ));
        }
        let terminal = match steps.last() {
            Some(PipelineStep::Count) => Some(PipelineStep::Count),
            Some(PipelineStep::Sum) => Some(PipelineStep::Sum),
            Some(PipelineStep::Average) => Some(PipelineStep::Average),
            Some(PipelineStep::Min) => Some(PipelineStep::Min),
            Some(PipelineStep::Max) => Some(PipelineStep::Max),
            Some(PipelineStep::Any) => Some(PipelineStep::Any),
            Some(PipelineStep::All) => Some(PipelineStep::All),
            Some(PipelineStep::Partition { item, rules }) => Some(PipelineStep::Partition {
                item: item.clone(),
                rules: rules.clone(),
            }),
            Some(PipelineStep::WriteCsv(path)) => Some(PipelineStep::WriteCsv(path.clone())),
            _ => None,
        };
        let transforms = if terminal.is_some() {
            &steps[..steps.len() - 1]
        } else {
            steps
        };
        let parallel_workers = steps
            .iter()
            .find_map(|step| match step {
                PipelineStep::Parallel(workers) => Some(*workers),
                _ => None,
            })
            .map(|workers| {
                usize::try_from(workers).map_err(|_| {
                    self.runtime_argument_error("parallel worker count is out of bounds")
                })
            })
            .transpose()?;
        let chunk_size = steps
            .iter()
            .find_map(|step| match step {
                PipelineStep::Chunk(size) => Some(*size),
                _ => None,
            })
            .map(|size| {
                usize::try_from(size)
                    .map_err(|_| self.runtime_argument_error("chunk size is out of bounds"))
            })
            .transpose()?;
        if let Some(workers) = parallel_workers {
            if workers == 0 {
                return Err(self.runtime_argument_error("parallel worker count must be positive"));
            }
            if workers > limits::MAX_PARALLEL_WORKERS {
                return Err(self.runtime_argument_error(format!(
                    "parallel worker count cannot exceed {}",
                    limits::MAX_PARALLEL_WORKERS
                )));
            }
        }
        if let Some(size) = chunk_size
            && (size == 0 || size > limits::MAX_CHUNK_SIZE)
        {
            return Err(self.runtime_argument_error(format!(
                "chunk size must be between 1 and {}",
                limits::MAX_CHUNK_SIZE
            )));
        }
        if let Some(workers) = parallel_workers {
            if !matches!(
                terminal,
                Some(
                    PipelineStep::Count
                        | PipelineStep::Sum
                        | PipelineStep::Average
                        | PipelineStep::Min
                        | PipelineStep::Max
                )
            ) {
                return Err(
                    self.runtime_argument_error("`parallel` requires an aggregate terminal")
                );
            }
            if !transforms.iter().all(|step| match step {
                PipelineStep::Where(expression) | PipelineStep::Derive(expression) => {
                    is_parallel_safe_expression(expression)
                }
                PipelineStep::Parallel(_) | PipelineStep::Chunk(_) => true,
                _ => false,
            }) {
                return Err(self.runtime_argument_error(
                    "`parallel` requires only parallel-safe `where` and `derive` expressions",
                ));
            }
            let input = values.into_iter().collect::<Vec<_>>();
            if input.iter().any(|value| {
                !matches!(
                    value,
                    Value::String(_) | Value::Int(_) | Value::Float(_) | Value::Bool(_)
                )
            }) {
                return Err(
                    self.runtime_collection_error("`parallel` requires scalar source items")
                );
            }
            match parallel::evaluate_parallel(input, transforms, workers, chunk_size) {
                Ok(output) => {
                    let sequential_steps: Vec<_> = steps
                        .last()
                        .filter(|step| {
                            matches!(
                                step,
                                PipelineStep::Count
                                    | PipelineStep::Sum
                                    | PipelineStep::Average
                                    | PipelineStep::Min
                                    | PipelineStep::Max
                            )
                        })
                        .cloned()
                        .into_iter()
                        .collect();
                    return self.evaluate_streaming_pipeline_with_sum_type(
                        output,
                        &sequential_steps,
                        sum_type,
                    );
                }
                Err(error) => {
                    return Err(self.runtime_error_with_code(error.code, error.message));
                }
            }
        }
        let mut output = Vec::new();
        let mut partitioned = match &terminal {
            Some(PipelineStep::Partition { rules, .. }) => Some(empty_partition_categories(rules)),
            _ => None,
        };
        let mut count = 0i64;
        let mut total = Value::Int(0);
        let mut sum = SumAccumulator::new(sum_type);
        let mut minimum = None;
        let mut maximum = None;
        let mut any_result = false;
        let mut all_result = true;
        let mut output_file = match terminal {
            Some(PipelineStep::WriteCsv(ref path_expression)) => {
                let path = match self.evaluate(path_expression)? {
                    Value::String(path) => path,
                    _ => return Err(self.runtime_type_error("write_csv path must be a string")),
                };
                Some(
                    fs::File::create(&path)
                        .map_err(|error| self.file_error(Path::new(&path), error))?,
                )
            }
            _ => None,
        };

        let mut take_counts = vec![0i64; transforms.len()];
        let mut skip_counts = vec![0i64; transforms.len()];
        let mut step_by_counts = vec![0i64; transforms.len()];
        let mut seen_values = vec![Vec::new(); transforms.len()];
        let mut drop_while_done = vec![false; transforms.len()];
        let mut values = values.into_iter();
        loop {
            if transforms
                .iter()
                .any(|step| matches!(step, PipelineStep::Take(0)))
            {
                break;
            }
            let Some(value) = values.next() else {
                break;
            };
            let mut current = Some(value);
            let mut stop_after_item = false;
            for (step_index, step) in transforms.iter().enumerate() {
                match step {
                    PipelineStep::Where(expression) => {
                        let item = current
                            .take()
                            .ok_or_else(|| self.runtime_error("pipeline item was lost".into()))?;
                        current = self.evaluate_pipeline_where_item(item, expression)?;
                        if current.is_none() {
                            break;
                        }
                    }
                    PipelineStep::Derive(expression) => {
                        let item = current
                            .take()
                            .ok_or_else(|| self.runtime_error("pipeline item was lost".into()))?;
                        current = Some(self.evaluate_pipeline_item(item, expression)?);
                    }
                    PipelineStep::Take(limit) => {
                        take_counts[step_index] += 1;
                        stop_after_item |= take_counts[step_index] >= *limit;
                    }
                    PipelineStep::Skip(limit) => {
                        if skip_counts[step_index] < *limit {
                            skip_counts[step_index] += 1;
                            current = None;
                            break;
                        }
                    }
                    PipelineStep::StepBy(interval) => {
                        if step_by_counts[step_index] > 0 {
                            step_by_counts[step_index] -= 1;
                            current = None;
                            break;
                        }
                        step_by_counts[step_index] = interval - 1;
                    }
                    PipelineStep::TakeWhile(expression) => {
                        let item = current
                            .take()
                            .ok_or_else(|| self.runtime_error("pipeline item was lost".into()))?;
                        match self.evaluate_pipeline_item(item.clone(), expression)? {
                            Value::Bool(true) => current = Some(item),
                            Value::Bool(false) => {
                                current = None;
                                stop_after_item = true;
                                break;
                            }
                            _ => {
                                return Err(self.runtime_type_error(
                                    "pipeline `take_while` condition must return a boolean",
                                ));
                            }
                        }
                    }
                    PipelineStep::DropWhile(expression) => {
                        if !drop_while_done[step_index] {
                            let item = current.take().ok_or_else(|| {
                                self.runtime_error("pipeline item was lost".into())
                            })?;
                            match self.evaluate_pipeline_item(item.clone(), expression)? {
                                Value::Bool(true) => {
                                    current = None;
                                    break;
                                }
                                Value::Bool(false) => {
                                    drop_while_done[step_index] = true;
                                    current = Some(item);
                                }
                                _ => {
                                    return Err(self.runtime_type_error(
                                        "pipeline `drop_while` condition must return a boolean",
                                    ));
                                }
                            }
                        }
                    }
                    PipelineStep::Distinct => {
                        let item = current
                            .take()
                            .ok_or_else(|| self.runtime_error("pipeline item was lost".into()))?;
                        if seen_values[step_index].contains(&item) {
                            current = None;
                            break;
                        }
                        seen_values[step_index].push(item.clone());
                        current = Some(item);
                    }
                    PipelineStep::Sum
                    | PipelineStep::Count
                    | PipelineStep::Average
                    | PipelineStep::Min
                    | PipelineStep::Max
                    | PipelineStep::Any
                    | PipelineStep::All
                    | PipelineStep::Partition { .. }
                    | PipelineStep::WriteCsv(_) => {
                        unreachable!()
                    }
                    PipelineStep::Chunk(_)
                    | PipelineStep::Parallel(_)
                    | PipelineStep::Checkpoint(_) => {}
                }
            }
            let Some(value) = current else {
                if stop_after_item {
                    break;
                }
                continue;
            };
            if let Some(PipelineStep::Partition { item, rules }) = &terminal {
                if let Some((category, value)) = self.evaluate_partition_item(value, item, rules)? {
                    let values = partitioned
                        .as_mut()
                        .and_then(|categories| categories.get_mut(&category))
                        .ok_or_else(|| {
                            self.runtime_error(format!(
                                "partition category `{category}` was not initialized"
                            ))
                        })?;
                    values.push(value);
                }
                if stop_after_item {
                    break;
                }
                continue;
            }
            match terminal {
                Some(PipelineStep::Count) => count += 1,
                Some(PipelineStep::Sum) => {
                    sum.add(value, self.current_span.as_ref())?;
                }
                Some(PipelineStep::Average) => {
                    total = operations::binary(
                        total,
                        &BinaryOperator::Add,
                        value,
                        self.current_span.as_ref(),
                    )?;
                    count += 1;
                }
                Some(PipelineStep::Min) => {
                    update_extreme(&mut minimum, value, false, self.current_span.as_ref())?;
                }
                Some(PipelineStep::Max) => {
                    update_extreme(&mut maximum, value, true, self.current_span.as_ref())?;
                }
                Some(PipelineStep::Any) => match value {
                    Value::Bool(value) => any_result |= value,
                    _ => {
                        return Err(self
                            .runtime_type_error("`any` pipeline terminal requires boolean items"));
                    }
                },
                Some(PipelineStep::All) => match value {
                    Value::Bool(value) => all_result &= value,
                    _ => {
                        return Err(self
                            .runtime_type_error("`all` pipeline terminal requires boolean items"));
                    }
                },
                Some(PipelineStep::WriteCsv(_)) => {
                    let values = match value {
                        Value::Array(values) | Value::List(values) | Value::Tuple(values) => values,
                        _ => {
                            return Err(self.runtime_type_error(
                                "write_csv requires derive to produce a row collection",
                            ));
                        }
                    };
                    csv::write_row(
                        output_file
                            .as_mut()
                            .expect("write_csv output should be initialized"),
                        values.as_slice(),
                    )
                    .map_err(|message| {
                        self.runtime_error_with_code(DiagnosticCode::RuntimeIo, message)
                    })?;
                }
                None => output.push(value),
                Some(PipelineStep::Where(_)) | Some(PipelineStep::Derive(_)) => unreachable!(),
                Some(PipelineStep::Chunk(_))
                | Some(PipelineStep::Parallel(_))
                | Some(PipelineStep::Checkpoint(_))
                | Some(PipelineStep::Take(_))
                | Some(PipelineStep::Skip(_))
                | Some(PipelineStep::StepBy(_))
                | Some(PipelineStep::DropWhile(_))
                | Some(PipelineStep::TakeWhile(_))
                | Some(PipelineStep::Distinct) => {
                    unreachable!()
                }
                Some(PipelineStep::Partition { .. }) => {
                    unreachable!("partition is handled before the terminal match")
                }
            }
            if (matches!(terminal, Some(PipelineStep::Any)) && any_result)
                || (matches!(terminal, Some(PipelineStep::All)) && !all_result)
            {
                break;
            }
            if stop_after_item {
                break;
            }
        }

        match terminal {
            Some(PipelineStep::Count) => Ok(Value::Int(count)),
            Some(PipelineStep::Sum) => Ok(sum.finish()),
            Some(PipelineStep::Average) => {
                if count == 0 {
                    Err(self
                        .runtime_collection_error("average requires at least one numeric value"))
                } else {
                    Ok(Value::Float(
                        total_to_f64(total, self.current_span.as_ref())? / count as f64,
                    ))
                }
            }
            Some(PipelineStep::Min) => minimum.ok_or_else(|| {
                self.runtime_collection_error("min requires at least one numeric value")
            }),
            Some(PipelineStep::Max) => maximum.ok_or_else(|| {
                self.runtime_collection_error("max requires at least one numeric value")
            }),
            Some(PipelineStep::Any) => Ok(Value::Bool(any_result)),
            Some(PipelineStep::All) => Ok(Value::Bool(all_result)),
            Some(PipelineStep::Partition { .. }) => match partitioned {
                Some(categories) => Ok(partition_result(categories)),
                None => Err(self.runtime_error("partition result was not initialized".into())),
            },
            Some(PipelineStep::WriteCsv(_)) => {
                if let Some(output_file) = output_file.as_mut() {
                    output_file.flush().map_err(|error| {
                        self.runtime_error_with_code(
                            DiagnosticCode::RuntimeIo,
                            format!("could not flush CSV: {error}"),
                        )
                    })?;
                }
                Ok(Value::Unit)
            }
            None => Ok(Value::List(shared_values(output))),
            Some(PipelineStep::Where(_)) | Some(PipelineStep::Derive(_)) => {
                unreachable!("pipeline terminal is normalized before evaluation")
            }
            Some(PipelineStep::Chunk(_))
            | Some(PipelineStep::Parallel(_))
            | Some(PipelineStep::Checkpoint(_))
            | Some(PipelineStep::Take(_))
            | Some(PipelineStep::Skip(_))
            | Some(PipelineStep::StepBy(_))
            | Some(PipelineStep::DropWhile(_))
            | Some(PipelineStep::TakeWhile(_))
            | Some(PipelineStep::Distinct) => {
                unreachable!("control steps are not terminals")
            }
        }
    }

    fn evaluate_fused_pipeline(
        &mut self,
        values: &mut Vec<Value>,
        transforms: &[PipelineStep],
        terminal: Option<&PipelineStep>,
        sum_type: Type,
    ) -> Result<Value, SimplyError> {
        let mut total = Value::Int(0);
        let mut sum = SumAccumulator::new(sum_type);
        let mut count = 0i64;
        let mut minimum = None;
        let mut maximum = None;

        for value in values.drain(..) {
            let mut current = Some(value);
            for step in transforms {
                match step {
                    PipelineStep::Where(expression) => {
                        let item = current
                            .take()
                            .ok_or_else(|| self.runtime_error("pipeline item was lost".into()))?;
                        current = self.evaluate_pipeline_where_item(item, expression)?;
                        if current.is_none() {
                            break;
                        }
                    }
                    PipelineStep::Derive(expression) => {
                        let item = current
                            .take()
                            .ok_or_else(|| self.runtime_error("pipeline item was lost".into()))?;
                        current = Some(self.evaluate_pipeline_item(item, expression)?);
                    }
                    PipelineStep::Sum
                    | PipelineStep::Count
                    | PipelineStep::Average
                    | PipelineStep::Min
                    | PipelineStep::Max
                    | PipelineStep::Any
                    | PipelineStep::All
                    | PipelineStep::WriteCsv(_)
                    | PipelineStep::Partition { .. } => {
                        unreachable!()
                    }
                    PipelineStep::Chunk(_)
                    | PipelineStep::Parallel(_)
                    | PipelineStep::Checkpoint(_)
                    | PipelineStep::Take(_)
                    | PipelineStep::Skip(_)
                    | PipelineStep::StepBy(_)
                    | PipelineStep::DropWhile(_)
                    | PipelineStep::TakeWhile(_)
                    | PipelineStep::Distinct => {}
                }
            }

            let Some(value) = current else {
                continue;
            };
            match terminal {
                Some(PipelineStep::Count) => count += 1,
                Some(PipelineStep::Sum) => {
                    sum.add(value, self.current_span.as_ref())?;
                }
                Some(PipelineStep::Average) => {
                    total = operations::binary(
                        total,
                        &BinaryOperator::Add,
                        value,
                        self.current_span.as_ref(),
                    )?;
                    count += 1;
                }
                Some(PipelineStep::Min) => {
                    update_extreme(&mut minimum, value, false, self.current_span.as_ref())?;
                }
                Some(PipelineStep::Max) => {
                    update_extreme(&mut maximum, value, true, self.current_span.as_ref())?;
                }
                _ => unreachable!(),
            }
        }

        match terminal {
            Some(PipelineStep::Count) => Ok(Value::Int(count)),
            Some(PipelineStep::Sum) => Ok(sum.finish()),
            Some(PipelineStep::Average) => {
                if count == 0 {
                    Err(self
                        .runtime_collection_error("average requires at least one numeric value"))
                } else {
                    Ok(Value::Float(
                        total_to_f64(total, self.current_span.as_ref())? / count as f64,
                    ))
                }
            }
            Some(PipelineStep::Min) => minimum.ok_or_else(|| {
                self.runtime_collection_error("min requires at least one numeric value")
            }),
            Some(PipelineStep::Max) => maximum.ok_or_else(|| {
                self.runtime_collection_error("max requires at least one numeric value")
            }),
            _ => unreachable!(),
        }
    }

    fn evaluate_pipeline_where_item(
        &mut self,
        value: Value,
        expression: &Expr,
    ) -> Result<Option<Value>, SimplyError> {
        self.define("item".into(), value).map_err(|error| {
            self.runtime_error_with_code(DiagnosticCode::DuplicateDeclaration, error)
        })?;
        let result = self.evaluate(expression);
        let item = self
            .remove_current("item")
            .ok_or_else(|| self.runtime_error("pipeline item binding was lost".into()))?;
        match (result?, item) {
            (Value::Bool(true), item) => Ok(Some(item)),
            (Value::Bool(false), _) => Ok(None),
            _ => Err(self.runtime_type_error("pipeline `where` condition must return a boolean")),
        }
    }

    fn evaluate_partition_item(
        &mut self,
        value: Value,
        item_name: &str,
        rules: &[PartitionRule],
    ) -> Result<Option<(String, Value)>, SimplyError> {
        for rule in rules {
            let matches = match &rule.condition {
                Some(condition) => {
                    self.define(item_name.into(), value.clone())
                        .map_err(|error| {
                            self.runtime_error_with_code(
                                DiagnosticCode::DuplicateDeclaration,
                                error,
                            )
                        })?;
                    let result = self.evaluate(condition);
                    self.remove_current(item_name).ok_or_else(|| {
                        self.runtime_error("partition item binding was lost".into())
                    })?;
                    match result? {
                        Value::Bool(matches) => matches,
                        _ => {
                            return Err(self
                                .runtime_type_error("partition condition must return a boolean"));
                        }
                    }
                }
                None => true,
            };
            if matches {
                return Ok(Some((rule.category.clone(), value)));
            }
        }
        Ok(None)
    }

    fn evaluate_pipeline_item(
        &mut self,
        value: Value,
        expression: &Expr,
    ) -> Result<Value, SimplyError> {
        self.define("item".into(), value).map_err(|error| {
            self.runtime_error_with_code(DiagnosticCode::DuplicateDeclaration, error)
        })?;
        let result = self.evaluate(expression);
        self.remove_current("item")
            .ok_or_else(|| self.runtime_error("pipeline item binding was lost".into()))?;
        result
    }

    fn runtime_error(&self, message: String) -> SimplyError {
        self.runtime_error_with_code(DiagnosticCode::RuntimeGeneral, message)
    }

    fn runtime_argument_error(&self, message: impl Into<String>) -> SimplyError {
        self.runtime_error_with_code(DiagnosticCode::RuntimeArgument, message)
    }

    fn runtime_type_error(&self, message: impl Into<String>) -> SimplyError {
        self.runtime_error_with_code(DiagnosticCode::RuntimeTypeMismatch, message)
    }

    fn runtime_collection_error(&self, message: impl Into<String>) -> SimplyError {
        self.runtime_error_with_code(DiagnosticCode::RuntimeCollection, message)
    }

    fn checkpoint_runtime_error(&self, error: checkpoint::CheckpointError) -> SimplyError {
        self.runtime_error_with_code(error.code, error.message)
    }

    fn single_character(&self, value: Value, operation: &str) -> Result<char, SimplyError> {
        let Value::String(value) = value else {
            return Err(
                self.runtime_type_error(format!("`{operation}` expects a one-character string"))
            );
        };
        let mut characters = value.chars();
        match (characters.next(), characters.next()) {
            (Some(character), None) => Ok(character),
            _ => Err(self.runtime_type_error(format!(
                "`{operation}` expects a string containing exactly one Unicode scalar value"
            ))),
        }
    }

    fn evaluate_enumerate(&mut self, arguments: &[Expr]) -> Result<Value, SimplyError> {
        if arguments.len() != 1 {
            return Err(self.runtime_argument_error("`enumerate` expects one argument"));
        }
        let sequence = self.evaluate(&arguments[0])?;
        let pairs = match sequence {
            Value::Array(values) | Value::List(values) | Value::Tuple(values) => {
                enumerate_values(values.iter().cloned())
            }
            Value::Range { start, end, step } => {
                enumerate_values(Value::range_values(start, end, step))
            }
            Value::String(text) => enumerate_values(
                text.chars()
                    .map(|character| Value::String(character.to_string())),
            ),
            _ => {
                return Err(self.runtime_type_error(
                    "`enumerate` requires an array, list, tuple, range, or string",
                ));
            }
        }
        .map_err(|message| self.runtime_error_with_code(DiagnosticCode::RuntimeLimit, message))?;
        Ok(Value::Array(shared_values(pairs)))
    }

    fn evaluate_zip(&mut self, arguments: &[Expr]) -> Result<Value, SimplyError> {
        if arguments.len() != 2 {
            return Err(self.runtime_argument_error("`zip` expects two arguments"));
        }
        let left = self.evaluate(&arguments[0])?;
        let right = self.evaluate(&arguments[1])?;
        let left = sequence_values(left).map_err(|message| self.runtime_type_error(message))?;
        let right = sequence_values(right).map_err(|message| self.runtime_type_error(message))?;
        let pairs = zip_values(left, right).map_err(|message| {
            self.runtime_error_with_code(DiagnosticCode::RuntimeLimit, message)
        })?;
        Ok(Value::Array(shared_values(pairs)))
    }

    fn evaluate_map_keys_or_values(
        &mut self,
        name: &str,
        arguments: &[Expr],
    ) -> Result<Value, SimplyError> {
        if name == "select_keys" {
            if arguments.len() != 2 {
                return Err(
                    self.runtime_argument_error("`select_keys` expects a map and key sequence")
                );
            }
            let collection = self.evaluate(&arguments[0])?;
            let keys = match self.evaluate(&arguments[1])? {
                Value::Array(keys) | Value::List(keys) | Value::Tuple(keys) => keys,
                _ => {
                    return Err(self.runtime_type_error(
                        "`select_keys` expects an array, list, or tuple of strings",
                    ));
                }
            };
            let mut selected = std::collections::HashSet::with_capacity(keys.len());
            for key in keys.iter() {
                let Value::String(key) = key else {
                    return Err(self.runtime_type_error("`select_keys` keys must be strings"));
                };
                selected.insert(key.as_str());
            }
            return match collection {
                Value::Hash(entries) => Ok(Value::Hash(shared_map(
                    entries
                        .iter()
                        .filter(|(key, _)| selected.contains(key.as_str()))
                        .map(|(key, value)| (key.clone(), value.clone()))
                        .collect(),
                ))),
                Value::Tree(entries) => Ok(Value::Tree(shared_map(
                    entries
                        .iter()
                        .filter(|(key, _)| selected.contains(key.as_str()))
                        .map(|(key, value)| (key.clone(), value.clone()))
                        .collect(),
                ))),
                _ => Err(self.runtime_type_error("`select_keys` requires a Hash or Tree")),
            };
        }
        if name == "without_key" {
            if arguments.len() != 2 {
                return Err(self.runtime_argument_error("`without_key` expects a map and key"));
            }
            let collection = self.evaluate(&arguments[0])?;
            let key = match self.evaluate(&arguments[1])? {
                Value::String(key) => key,
                _ => return Err(self.runtime_type_error("`without_key` key must be a string")),
            };
            return match collection {
                Value::Hash(entries) => {
                    let mut entries = (*entries).clone();
                    entries.remove(&key);
                    Ok(Value::Hash(shared_map(entries)))
                }
                Value::Tree(entries) => {
                    let mut entries = (*entries).clone();
                    entries.remove(&key);
                    Ok(Value::Tree(shared_map(entries)))
                }
                _ => Err(self.runtime_type_error("`without_key` requires a Hash or Tree")),
            };
        }
        if name == "get" {
            if arguments.len() != 3 {
                return Err(
                    self.runtime_argument_error("`get` expects a map, key, and default value")
                );
            }
            let collection = self.evaluate(&arguments[0])?;
            let key = match self.evaluate(&arguments[1])? {
                Value::String(key) => key,
                _ => return Err(self.runtime_type_error("`get` key must be a string")),
            };
            let entries = match collection {
                Value::Hash(entries) | Value::Tree(entries) => entries,
                _ => return Err(self.runtime_type_error("`get` requires a Hash or Tree")),
            };
            if let Some(value) = entries.get(&key) {
                return Ok(value.clone());
            }
            return self.evaluate(&arguments[2]);
        }
        if name == "has_key" {
            if arguments.len() != 2 {
                return Err(self.runtime_argument_error("`has_key` expects two arguments"));
            }
            let collection = self.evaluate(&arguments[0])?;
            let key = match self.evaluate(&arguments[1])? {
                Value::String(key) => key,
                _ => return Err(self.runtime_type_error("`has_key` key must be a string")),
            };
            let result = match collection {
                Value::Hash(entries) | Value::Tree(entries) => entries.contains_key(&key),
                _ => return Err(self.runtime_type_error("`has_key` requires a Hash or Tree")),
            };
            return Ok(Value::Bool(result));
        }
        if arguments.len() != 1 {
            return Err(self.runtime_argument_error(format!("`{name}` expects one argument")));
        }
        let collection = self.evaluate(&arguments[0])?;
        let result = match collection {
            Value::Hash(entries) | Value::Tree(entries) if name == "keys" => {
                entries.keys().cloned().map(Value::String).collect()
            }
            Value::Hash(entries) | Value::Tree(entries) if name == "entries" => entries
                .iter()
                .map(|(key, value)| {
                    Value::Tuple(shared_values(vec![
                        Value::String(key.clone()),
                        value.clone(),
                    ]))
                })
                .collect(),
            Value::Hash(entries) | Value::Tree(entries) => entries.values().cloned().collect(),
            _ => {
                return Err(self.runtime_type_error(format!("`{name}` requires a Hash or Tree")));
            }
        };
        Ok(Value::Array(shared_values(result)))
    }

    fn runtime_error_with_code(
        &self,
        code: DiagnosticCode,
        message: impl Into<String>,
    ) -> SimplyError {
        let message = message.into();
        let span = self.current_span.clone().unwrap_or_else(|| Span::new(0, 0));
        let code = match code {
            DiagnosticCode::TypeMismatch => DiagnosticCode::RuntimeTypeMismatch,
            DiagnosticCode::DuplicateDeclaration => DiagnosticCode::RuntimeDeclaration,
            DiagnosticCode::InvalidReassignment => DiagnosticCode::RuntimeMutability,
            code if code.category() == crate::error::DiagnosticCategory::Runtime => code,
            _ => DiagnosticCode::RuntimeGeneral,
        };
        SimplyError::Runtime {
            span,
            code,
            message,
        }
    }

    fn ensure_type(&self, value: &Value, expected: &Type, name: &str) -> Result<(), SimplyError> {
        if *expected == Type::Unknown {
            return Ok(());
        }
        if self.value_matches_type(value, expected) {
            Ok(())
        } else {
            Err(self.runtime_error_with_code(
                DiagnosticCode::RuntimeTypeMismatch,
                format!(
                    "cannot assign a {} value to `{name}`: wrong type; expected {}, found {}; use `{name} as {} is ...` or provide a {} value",
                    Self::value_type_name(value),
                    Self::type_name(expected),
                    Self::value_type_name(value),
                    Self::type_name(expected),
                    Self::type_name(expected)
                ),
            ))
        }
    }

    fn ensure_reassignment_type(
        &self,
        value: &Value,
        expected: &Type,
        name: &str,
    ) -> Result<(), SimplyError> {
        if *expected == Type::Unknown {
            return Ok(());
        }
        if self.value_matches_type(value, expected) {
            return Ok(());
        }

        Err(self.runtime_error_with_code(
            DiagnosticCode::RuntimeTypeMismatch,
            format!(
                "cannot reassign `{name}` with type {}; variable `{name}` remains type {}",
                Self::value_type_name(value),
                Self::type_name(expected)
            ),
        ))
    }

    fn type_name(expected: &Type) -> String {
        expected.name()
    }

    fn value_type_name(value: &Value) -> String {
        match value {
            Value::Unit => "Unit".into(),
            Value::String(_) => "String".into(),
            Value::Int(_) => "Int".into(),
            Value::Float(_) => "Float".into(),
            Value::Bool(_) => "Bool".into(),
            Value::Range { .. } => "Range".into(),
            Value::CsvStream { .. } => "CsvStream".into(),
            Value::Array(_) => "Array".into(),
            Value::List(_) => "List".into(),
            Value::Tuple(_) => "Tuple".into(),
            Value::Hash(_) => "Hash".into(),
            Value::Tree(_) => "Tree".into(),
            Value::Matrix(_) => "Matrix".into(),
            Value::Function(_) => "Function".into(),
            Value::Struct(instance) => instance.type_name.clone(),
            Value::Enum(value) => value.enum_name.clone(),
        }
    }

    fn value_matches_type(&self, value: &Value, expected: &Type) -> bool {
        match (value, expected) {
            (Value::String(_), Type::String)
            | (Value::Int(_), Type::Int)
            | (Value::Float(_), Type::Float)
            | (Value::Bool(_), Type::Bool)
            | (Value::Unit, Type::Unit)
            | (Value::Hash(_), Type::Hash)
            | (Value::Tree(_), Type::Tree)
            | (Value::Matrix(_), Type::Matrix)
            | (Value::Function(_), Type::Function { .. }) => true,
            (Value::Struct(instance), Type::Struct(expected)) => &instance.identity == expected,
            (Value::Enum(value), Type::Enum(expected)) => &value.identity == expected,
            (Value::Range { .. }, Type::Range) => true,
            (Value::CsvStream { .. }, Type::CsvStream) => true,
            (Value::Array(_), Type::Array(element)) | (Value::List(_), Type::List(element))
                if **element == Type::Unknown =>
            {
                true
            }
            (Value::Array(values), Type::Array(element))
            | (Value::List(values), Type::List(element)) => values
                .iter()
                .all(|value| self.value_matches_type(value, element)),
            (Value::Hash(values), Type::HashValues(element))
            | (Value::Tree(values), Type::TreeValues(element)) => values
                .values()
                .all(|value| **element == Type::Unknown || self.value_matches_type(value, element)),
            (Value::Tuple(values), Type::Tuple(types)) => {
                values.len() == types.len()
                    && values
                        .iter()
                        .zip(types)
                        .all(|(value, expected)| self.value_matches_type(value, expected))
            }
            _ => false,
        }
    }

    fn type_of_value(value: &Value) -> Type {
        match value {
            Value::String(_) => Type::String,
            Value::Int(_) => Type::Int,
            Value::Float(_) => Type::Float,
            Value::Bool(_) => Type::Bool,
            Value::Range { .. } => Type::Range,
            Value::CsvStream { .. } => Type::CsvStream,
            Value::Array(values) => Type::Array(Box::new(
                values
                    .first()
                    .map(Self::type_of_value)
                    .unwrap_or(Type::Unknown),
            )),
            Value::List(values) => Type::List(Box::new(
                values
                    .first()
                    .map(Self::type_of_value)
                    .unwrap_or(Type::Unknown),
            )),
            Value::Tuple(values) => Type::Tuple(values.iter().map(Self::type_of_value).collect()),
            Value::Hash(values) => {
                let mut types = values.values().map(Self::type_of_value);
                let first = types.next().unwrap_or(Type::Unknown);
                if types.all(|typ| typ.compatible_with(&first)) {
                    Type::HashValues(Box::new(first))
                } else {
                    Type::HashValues(Box::new(Type::Unknown))
                }
            }
            Value::Tree(values) => {
                let mut types = values.values().map(Self::type_of_value);
                let first = types.next().unwrap_or(Type::Unknown);
                if types.all(|typ| typ.compatible_with(&first)) {
                    Type::TreeValues(Box::new(first))
                } else {
                    Type::TreeValues(Box::new(Type::Unknown))
                }
            }
            Value::Matrix(_) => Type::Matrix,
            Value::Struct(instance) => Type::Struct(instance.identity.clone()),
            Value::Enum(value) => Type::Enum(value.identity.clone()),
            Value::Function(function) => Type::Function {
                parameters: function
                    .parameters
                    .iter()
                    .map(|(_, typ, _)| typ.clone().map(Box::new))
                    .collect(),
                return_type: function.return_type.clone().map(Box::new),
            },
            Value::Unit => Type::Unit,
        }
    }

    fn sequence_sum_type(&self, expression: &Expr) -> Type {
        self.sequence_sum_type_from_type(&self.runtime_expression_type(expression, None))
    }

    fn pipeline_sum_type(&self, source: &Expr, steps: &[PipelineStep]) -> Type {
        if !matches!(steps.last(), Some(PipelineStep::Sum)) {
            return Type::Unknown;
        }
        let mut item_type = self.sequence_item_type(&self.runtime_expression_type(source, None));
        for step in &steps[..steps.len() - 1] {
            if let PipelineStep::Derive(expression) = step {
                item_type = self.runtime_expression_type(expression, Some(&item_type));
            }
        }
        item_type
    }

    fn sequence_item_type(&self, typ: &Type) -> Type {
        match typ {
            Type::Array(element) | Type::List(element) => (**element).clone(),
            Type::Tuple(elements) => {
                if elements.contains(&Type::Float) {
                    Type::Float
                } else if elements.contains(&Type::Unknown) {
                    Type::Unknown
                } else {
                    Type::Int
                }
            }
            Type::Range => Type::Int,
            Type::CsvStream => Type::List(Box::new(Type::String)),
            _ => Type::Unknown,
        }
    }

    fn sequence_sum_type_from_type(&self, typ: &Type) -> Type {
        match self.sequence_item_type(typ) {
            Type::Float => Type::Float,
            Type::Int => Type::Int,
            _ => Type::Unknown,
        }
    }

    fn runtime_expression_type(&self, expression: &Expr, item_type: Option<&Type>) -> Type {
        match expression {
            Expr::Literal(literal) => match literal {
                Literal::String(_) => Type::String,
                Literal::Int(_) => Type::Int,
                Literal::Float(_) => Type::Float,
                Literal::Bool(_) => Type::Bool,
            },
            Expr::Identifier(name) if name == "item" => item_type.cloned().unwrap_or(Type::Unknown),
            Expr::Identifier(name) => self
                .variable_types
                .lookup(name)
                .cloned()
                .unwrap_or(Type::Unknown),
            Expr::Unary { operator, operand } => match operator {
                UnaryOperator::Negate => self.runtime_expression_type(operand, item_type),
                UnaryOperator::Not => Type::Bool,
                UnaryOperator::Transpose => Type::Matrix,
            },
            Expr::Binary {
                left,
                operator,
                right,
            } => {
                use BinaryOperator::{
                    Add, And, Divide, Equal, Greater, GreaterEqual, Less, LessEqual,
                    MatrixMultiply, Multiply, NotEqual, Or, Remainder, Subtract,
                };
                let left = self.runtime_expression_type(left, item_type);
                let right = self.runtime_expression_type(right, item_type);
                match operator {
                    Greater | GreaterEqual | Less | LessEqual | Equal | NotEqual | And | Or => {
                        Type::Bool
                    }
                    MatrixMultiply => Type::Matrix,
                    Add if left == Type::String && right == Type::String => Type::String,
                    Add | Divide | Multiply | Remainder | Subtract
                        if matches!(left, Type::Int | Type::Float)
                            && matches!(right, Type::Int | Type::Float) =>
                    {
                        if left == Type::Float || right == Type::Float {
                            Type::Float
                        } else {
                            Type::Int
                        }
                    }
                    _ => Type::Unknown,
                }
            }
            Expr::Call { name, arguments } => {
                if let Some(Type::Function {
                    return_type: Some(return_type),
                    ..
                }) = self.variable_types.lookup(name)
                {
                    return (**return_type).clone();
                }
                match name.as_str() {
                    "to_float" | "sqrt" | "exp" | "log" | "log10" | "sin" | "cos" | "tan"
                    | "floor" | "ceil" | "pow" | "norm" | "distance" | "mean" => Type::Float,
                    "to_int" | "sign" | "length" | "count" => Type::Int,
                    "type_of" | "read_file" | "trim" | "substring" | "replace" | "join" => {
                        Type::String
                    }
                    "range" => Type::Range,
                    "csv_rows" => Type::CsvStream,
                    "total" if arguments.len() == 1 => self.sequence_sum_type(&arguments[0]),
                    "abs" | "round" if !arguments.is_empty() => {
                        self.runtime_expression_type(&arguments[0], item_type)
                    }
                    _ => Type::Unknown,
                }
            }
            Expr::Array(values) => {
                Type::Array(Box::new(self.collection_element_type(values, item_type)))
            }
            Expr::List(values) => {
                Type::List(Box::new(self.collection_element_type(values, item_type)))
            }
            Expr::Tuple(values) => Type::Tuple(
                values
                    .iter()
                    .map(|value| self.runtime_expression_type(value, item_type))
                    .collect(),
            ),
            Expr::Index { target, .. } => match self.runtime_expression_type(target, item_type) {
                Type::Array(element) | Type::List(element) => *element,
                Type::Tuple(elements) if !elements.is_empty() => {
                    if elements.iter().all(|typ| typ == &elements[0]) {
                        elements[0].clone()
                    } else {
                        Type::Unknown
                    }
                }
                _ => Type::Unknown,
            },
            Expr::Pipeline { .. }
            | Expr::MessageDispatch { .. }
            | Expr::EnumVariant { .. }
            | Expr::Match { .. }
            | Expr::Field { .. }
            | Expr::Hash(_)
            | Expr::Tree(_)
            | Expr::Matrix(_) => Type::Unknown,
        }
    }

    fn collection_element_type(&self, values: &[Expr], item_type: Option<&Type>) -> Type {
        let mut element_type = Type::Unknown;
        for value in values {
            let value_type = self.runtime_expression_type(value, item_type);
            if element_type == Type::Unknown {
                element_type = value_type;
            } else if value_type == Type::Float && matches!(element_type, Type::Int | Type::Float) {
                element_type = Type::Float;
            } else if value_type != element_type {
                return Type::Unknown;
            }
        }
        element_type
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::value::shared_values;
    use crate::semantic::SemanticAnalyzer;
    use crate::types::DeclarationKind;

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
        SemanticAnalyzer::new()
            .analyze(&program)
            .expect("in-memory declarations should type check");

        for _ in 0..2 {
            let mut evaluator = Evaluator::new();
            evaluator
                .run(&program)
                .expect("in-memory declarations should execute");
            let Value::Struct(item) = evaluator.lookup("item").expect("item should be bound")
            else {
                panic!("item should be a struct value");
            };
            let Value::Enum(state) = evaluator.lookup("state").expect("state should be bound")
            else {
                panic!("state should be an enum value");
            };
            assert_eq!(
                item.identity,
                DeclarationIdentity::new("memory://root", "Item", DeclarationKind::Struct)
            );
            assert_eq!(
                state.identity,
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
        parallel::PARALLEL_THREAD_IDS
            .lock()
            .expect("parallel test lock")
            .clear();
        let values = (0..16).map(Value::Int).collect();
        let transforms = [PipelineStep::Derive(Expr::Binary {
            left: Box::new(Expr::Identifier("item".into())),
            operator: BinaryOperator::Multiply,
            right: Box::new(Expr::Literal(Literal::Int(2))),
        })];
        let output = parallel::evaluate_parallel(values, &transforms, 4, Some(8))
            .expect("parallel evaluation");
        assert_eq!(
            output,
            (0..16)
                .map(|value| Value::Int(value * 2))
                .collect::<Vec<_>>()
        );
        assert_eq!(
            parallel::PARALLEL_THREAD_IDS
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
        let source =
            "fn recurse(value as Int) gives Int:\n    return recurse(value + 1)\nend\nrecurse(0)\n";
        let program = Parser::new(Lexer::new(source).tokenize().expect("source should lex"))
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
}
