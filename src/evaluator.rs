//! evaluator.rs — runtime execution engine
//! Evaluates validated Simply programs, manages scopes and functions, and executes control flow and pipelines.
//! Key components: Evaluator, TypeScopes, Flow, and Function.
use std::{
    cell::RefCell,
    collections::{BTreeMap, HashMap, HashSet},
    fs,
    io::{self, BufRead, BufReader, IsTerminal, Read, Write},
    path::{Path, PathBuf},
    rc::Rc,
    sync::Arc,
};

use crate::{
    ast::{
        BinaryOperator, Expr, Literal, PartitionRule, PipelineStep, Program, Stmt,
        is_parallel_safe_expression,
    },
    error::{DiagnosticCode, SimplyError, Span},
    lexer::Lexer,
    parser::Parser,
    runtime::{
        collections, operations,
        scope::ScopeStack,
        value::{FunctionValue, Value, owned_map_values, owned_values, shared_map, shared_values},
    },
    types::Type,
};

struct CheckpointState {
    position: usize,
    output_len: u64,
    output_checksum: u64,
    input_path: String,
    output_path: String,
    input_len: u64,
    input_modified_ns: u128,
}

fn modified_time_ns(path: &str) -> Result<u128, String> {
    let modified = fs::metadata(path)
        .map_err(|error| format!("could not inspect checkpoint input `{path}`: {error}"))?
        .modified()
        .map_err(|error| format!("could not read modification time for `{path}`: {error}"))?;
    modified
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .map_err(|_| format!("checkpoint input `{path}` has an invalid modification time"))
}

fn output_prefix_checksum(path: &str, length: u64) -> Result<u64, String> {
    let file = fs::File::open(path)
        .map_err(|error| format!("could not read checkpoint output `{path}`: {error}"))?;
    let mut reader = BufReader::new(file.take(length));
    let mut checksum = 0xcbf29ce484222325u64;
    let mut buffer = [0u8; 8192];
    loop {
        let count = reader
            .read(&mut buffer)
            .map_err(|error| format!("could not read checkpoint output `{path}`: {error}"))?;
        if count == 0 {
            break;
        }
        for byte in &buffer[..count] {
            checksum = (checksum ^ u64::from(*byte)).wrapping_mul(0x100000001b3);
        }
    }
    Ok(checksum)
}

fn encode_checkpoint_path(path: &str) -> String {
    path.as_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn decode_checkpoint_path(encoded: &str) -> Result<String, String> {
    if !encoded.len().is_multiple_of(2) {
        return Err("invalid encoded checkpoint path".into());
    }
    let bytes = (0..encoded.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&encoded[index..index + 2], 16))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| "invalid encoded checkpoint path")?;
    String::from_utf8(bytes).map_err(|_| "checkpoint path is not valid UTF-8".into())
}

fn read_checkpoint(path: &str) -> Result<Option<CheckpointState>, String> {
    match fs::read_to_string(path) {
        Ok(value) => {
            let invalid = || format!("checkpoint `{path}` contains invalid recovery state");
            let mut fields = value.lines();
            let position = fields
                .next()
                .ok_or_else(invalid)?
                .parse::<usize>()
                .map_err(|_| invalid())?;
            let output_len = fields
                .next()
                .ok_or_else(invalid)?
                .parse::<u64>()
                .map_err(|_| invalid())?;
            let output_checksum = fields
                .next()
                .ok_or_else(invalid)?
                .parse::<u64>()
                .map_err(|_| invalid())?;
            let input_len = fields
                .next()
                .ok_or_else(invalid)?
                .parse::<u64>()
                .map_err(|_| invalid())?;
            let input_modified_ns = fields
                .next()
                .ok_or_else(invalid)?
                .parse::<u128>()
                .map_err(|_| invalid())?;
            let input_path = decode_checkpoint_path(fields.next().ok_or_else(invalid)?)
                .map_err(|_| invalid())?;
            let output_path = decode_checkpoint_path(fields.next().ok_or_else(invalid)?)
                .map_err(|_| invalid())?;
            if fields.next().is_some() {
                return Err(invalid());
            }
            Ok(Some(CheckpointState {
                position,
                output_len,
                output_checksum,
                input_path,
                output_path,
                input_len,
                input_modified_ns,
            }))
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("could not read checkpoint `{path}`: {error}")),
    }
}

fn write_checkpoint(path: &str, state: &CheckpointState) -> Result<(), String> {
    let temporary = format!("{path}.tmp");
    let contents = format!(
        "{}\n{}\n{}\n{}\n{}\n{}\n{}\n",
        state.position,
        state.output_len,
        state.output_checksum,
        state.input_len,
        state.input_modified_ns,
        encode_checkpoint_path(&state.input_path),
        encode_checkpoint_path(&state.output_path),
    );
    fs::write(&temporary, contents)
        .map_err(|error| format!("could not write checkpoint `{path}`: {error}"))?;
    fs::rename(&temporary, path)
        .map_err(|error| format!("could not replace checkpoint `{path}`: {error}"))
}

fn checkpoint_progress(
    path: Option<&str>,
    processed: usize,
    interval: usize,
    output: Option<&mut fs::File>,
    input_path: &str,
    output_path: Option<&str>,
) -> Result<(), String> {
    if let Some(path) = path
        && processed.is_multiple_of(interval)
    {
        let output_len = if let Some(output) = output {
            output
                .flush()
                .map_err(|error| format!("could not flush checkpoint output: {error}"))?;
            output
                .metadata()
                .map_err(|error| format!("could not inspect checkpoint output: {error}"))?
                .len()
        } else {
            return Err("checkpoint requires a CSV output sink".into());
        };
        let output_path =
            output_path.ok_or_else(|| "checkpoint requires an output path".to_string())?;
        let output_checksum = output_prefix_checksum(output_path, output_len)?;
        let input_metadata = fs::metadata(input_path).map_err(|error| {
            format!("could not inspect checkpoint input `{input_path}`: {error}")
        })?;
        write_checkpoint(
            path,
            &CheckpointState {
                position: processed,
                output_len,
                output_checksum,
                input_path: input_path.into(),
                output_path: output_path.into(),
                input_len: input_metadata.len(),
                input_modified_ns: modified_time_ns(input_path)?,
            },
        )?;
    }
    Ok(())
}

fn remove_checkpoint(path: &str) -> Result<(), String> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("could not remove checkpoint `{path}`: {error}")),
    }
}

fn validate_checkpoint_source(
    state: &CheckpointState,
    input_path: &str,
    output_path: &str,
) -> Result<(), String> {
    if state.input_path != input_path || state.output_path != output_path {
        return Err("checkpoint input or output path does not match this Flow".into());
    }
    let input_len = fs::metadata(input_path)
        .map_err(|error| format!("could not inspect checkpoint input `{input_path}`: {error}"))?
        .len();
    if state.input_len != input_len || state.input_modified_ns != modified_time_ns(input_path)? {
        return Err(format!(
            "checkpoint input `{input_path}` changed since the checkpoint was written"
        ));
    }
    Ok(())
}

fn validate_checkpoint_output(state: &CheckpointState, output_path: &str) -> Result<(), String> {
    let output_len = fs::metadata(output_path)
        .map_err(|error| format!("could not inspect checkpoint output `{output_path}`: {error}"))?
        .len();
    if state.output_len > output_len {
        return Err(format!(
            "checkpoint output length exceeds the current output file `{output_path}`"
        ));
    }
    if output_prefix_checksum(output_path, state.output_len)? != state.output_checksum {
        return Err(format!(
            "checkpoint output `{output_path}` changed since the checkpoint was written"
        ));
    }
    Ok(())
}

fn resolved_path(path: &str) -> Result<PathBuf, String> {
    let path = Path::new(path);
    if let Ok(resolved) = fs::canonicalize(path) {
        return Ok(resolved);
    }
    let name = path
        .file_name()
        .ok_or_else(|| format!("path `{}` has no file name", path.display()))?;
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let parent = fs::canonicalize(parent).map_err(|error| {
        format!(
            "could not resolve directory `{}`: {error}",
            parent.display()
        )
    })?;
    Ok(parent.join(name))
}

fn paths_are_same(left: &str, right: &str) -> Result<bool, String> {
    Ok(resolved_path(left)? == resolved_path(right)?)
}

fn parse_csv_record(line: &str) -> Result<Vec<Value>, String> {
    #[derive(Clone, Copy)]
    enum FieldState {
        Start,
        Unquoted,
        Quoted,
        AfterQuote,
    }

    let mut fields = Vec::new();
    let mut field = String::new();
    let mut state = FieldState::Start;
    let mut chars = line.chars().peekable();
    while let Some(character) = chars.next() {
        match state {
            FieldState::Start => match character {
                '"' => state = FieldState::Quoted,
                ',' => fields.push(Value::String(String::new())),
                _ => {
                    field.push(character);
                    state = FieldState::Unquoted;
                }
            },
            FieldState::Unquoted => match character {
                '"' => return Err("quote inside an unquoted field".into()),
                ',' => {
                    fields.push(Value::String(std::mem::take(&mut field)));
                    state = FieldState::Start;
                }
                _ => field.push(character),
            },
            FieldState::Quoted => match character {
                '"' if chars.peek() == Some(&'"') => {
                    field.push('"');
                    chars.next();
                }
                '"' => state = FieldState::AfterQuote,
                _ => field.push(character),
            },
            FieldState::AfterQuote => match character {
                ',' => {
                    fields.push(Value::String(std::mem::take(&mut field)));
                    state = FieldState::Start;
                }
                _ => return Err("unexpected character after a quoted field".into()),
            },
        }
    }
    if matches!(state, FieldState::Quoted) {
        return Err("unterminated quoted field".into());
    }
    fields.push(Value::String(field));
    Ok(fields)
}

fn read_csv_record<R: BufRead>(reader: &mut R) -> Result<Option<String>, std::io::Error> {
    let mut record = String::new();
    let mut line = String::new();
    let mut quoted = false;
    let mut at_field_start = true;

    loop {
        line.clear();
        if reader.read_line(&mut line)? == 0 {
            return Ok((!record.is_empty()).then_some(record));
        }
        record.push_str(&line);
        let mut characters = line.chars().peekable();
        while let Some(character) = characters.next() {
            if quoted {
                if character == '"' {
                    if characters.peek() == Some(&'"') {
                        characters.next();
                    } else {
                        quoted = false;
                    }
                }
            } else if character == '"' && at_field_start {
                quoted = true;
                at_field_start = false;
            } else {
                at_field_start = character == ',';
            }
        }
        if !quoted {
            return Ok(Some(record));
        }
        at_field_start = true;
    }
}

fn csv_field(value: &Value) -> Result<String, String> {
    match value {
        Value::String(value) => {
            if value.contains([',', '"', '\n', '\r']) {
                Ok(format!("\"{}\"", value.replace('"', "\"\"")))
            } else {
                Ok(value.clone())
            }
        }
        Value::Int(value) => Ok(value.to_string()),
        Value::Float(value) if value.is_finite() => Ok(value.to_string()),
        Value::Bool(value) => Ok(value.to_string()),
        Value::Unit => Ok(String::new()),
        _ => Err("write_csv supports strings, numbers, booleans, and Unit fields".into()),
    }
}

fn write_csv_row(output: &mut impl Write, values: &[Value]) -> Result<(), String> {
    let mut fields = Vec::with_capacity(values.len());
    for value in values {
        fields.push(csv_field(value)?);
    }
    writeln!(output, "{}", fields.join(","))
        .map_err(|error| format!("could not write CSV: {error}"))
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
                        code: DiagnosticCode::RuntimeGeneral,
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
                    code: DiagnosticCode::RuntimeGeneral,
                    message: "`clamp` requires finite values and minimum <= maximum".into(),
                });
            }
            Ok(Value::Float(value.clamp(minimum, maximum)))
        }
    }
}

fn total_to_f64(value: Value, span: Option<&Span>) -> Result<f64, SimplyError> {
    match value {
        Value::Int(value) => Ok(value as f64),
        Value::Float(value) if value.is_finite() => Ok(value),
        _ => Err(SimplyError::Runtime {
            span: span.cloned().unwrap_or_else(|| Span::new(0, 0)),
            code: DiagnosticCode::RuntimeGeneral,
            message: "aggregate result must be numeric".into(),
        }),
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
                    code: DiagnosticCode::RuntimeGeneral,
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
                code: DiagnosticCode::RuntimeGeneral,
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
    let mut bound: HashSet<String> = parameters.iter().map(|(name, _, _)| name.clone()).collect();
    collect_declared_names(body, &mut bound);
    let mut dependencies = HashSet::new();
    collect_statement_dependencies(body, &bound, &mut dependencies);
    dependencies
}

fn collect_declared_names(statements: &[Stmt], names: &mut HashSet<String>) {
    for statement in statements {
        let statement = match statement {
            Stmt::Located { statement, .. } => statement.as_ref(),
            statement => statement,
        };
        match statement {
            Stmt::Assign { name, .. } | Stmt::Flow { name, .. } | Stmt::Function { name, .. } => {
                names.insert(name.clone());
            }
            Stmt::Destructure {
                names: bindings, ..
            } => {
                names.extend(bindings.iter().map(|(name, _)| name.clone()));
            }
            Stmt::For { name, .. } => {
                names.insert(name.clone());
            }
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                collect_declared_names(then_branch, names);
                collect_declared_names(else_branch, names);
            }
            Stmt::Try {
                try_body,
                catches,
                finally_body,
            } => {
                collect_declared_names(try_body, names);
                for catch in catches {
                    if let Some(binding) = &catch.binding {
                        names.insert(binding.clone());
                    }
                    collect_declared_names(&catch.body, names);
                }
                collect_declared_names(finally_body, names);
            }
            Stmt::While { body, .. } => collect_declared_names(body, names),
            _ => {}
        }
    }
}

fn collect_statement_dependencies(
    statements: &[Stmt],
    bound: &HashSet<String>,
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
            | Stmt::Expression(expression)
            | Stmt::Throw(expression)
            | Stmt::Return(expression) => {
                collect_expression_dependencies(expression, bound, dependencies)
            }
            Stmt::Assign { value, .. } | Stmt::Reassign { value, .. } => {
                collect_expression_dependencies(value, bound, dependencies)
            }
            Stmt::Flow { source, steps, .. } => {
                collect_expression_dependencies(source, bound, dependencies);
                collect_pipeline_step_dependencies(steps, bound, dependencies);
            }
            Stmt::SetIndex { index, value, .. } => {
                collect_expression_dependencies(index, bound, dependencies);
                collect_expression_dependencies(value, bound, dependencies);
            }
            Stmt::Destructure { value, .. } => {
                collect_expression_dependencies(value, bound, dependencies)
            }
            Stmt::CollectionOp { value, .. } => {
                collect_expression_dependencies(value, bound, dependencies)
            }
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                collect_expression_dependencies(condition, bound, dependencies);
                collect_statement_dependencies(then_branch, bound, dependencies);
                collect_statement_dependencies(else_branch, bound, dependencies);
            }
            Stmt::Try {
                try_body,
                catches,
                finally_body,
            } => {
                collect_statement_dependencies(try_body, bound, dependencies);
                for catch in catches {
                    collect_statement_dependencies(&catch.body, bound, dependencies);
                }
                collect_statement_dependencies(finally_body, bound, dependencies);
            }
            Stmt::Function {
                parameters, body, ..
            } => {
                let nested = closure_dependencies(body, parameters);
                dependencies.extend(nested.into_iter().filter(|name| !bound.contains(name)));
            }
            Stmt::For { iterable, body, .. } => {
                collect_expression_dependencies(iterable, bound, dependencies);
                collect_statement_dependencies(body, bound, dependencies);
            }
            Stmt::While { condition, body } => {
                collect_expression_dependencies(condition, bound, dependencies);
                collect_statement_dependencies(body, bound, dependencies);
            }
            Stmt::Import { .. } | Stmt::Break | Stmt::Continue => {}
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
            PipelineStep::Where(expression) | PipelineStep::Derive(expression) => {
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
            | PipelineStep::Chunk(_)
            | PipelineStep::Parallel(_) => {}
        }
    }
}

#[derive(Default)]
pub struct Evaluator {
    scopes: ScopeStack,
    variable_types: TypeScopes,
    function_scopes: Vec<HashMap<String, Arc<Function>>>,
    current_span: Option<Span>,
    current_file: Option<PathBuf>,
    import_stack: Vec<PathBuf>,
    import_cache: Rc<RefCell<HashMap<PathBuf, Arc<Program>>>>,
    output_enabled: bool,
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
        self.scopes.assign(name, value)
    }

    fn remove_current(&mut self, name: &str) -> Option<Value> {
        self.scopes.remove_current(name)
    }

    fn push_scope(&mut self) {
        self.scopes.push();
        self.variable_types.push();
        self.function_scopes.push(HashMap::new());
    }

    fn pop_scope(&mut self) {
        self.scopes.pop();
        self.variable_types.pop();
        if self.function_scopes.len() > 1 {
            self.function_scopes.pop();
        }
    }
}

fn print_value(value: &Value) {
    let rendered = value.display();
    if io::stdout().is_terminal() {
        println!("\x1b[36m{rendered}\x1b[0m");
    } else {
        println!("{rendered}");
    }
}

enum Flow {
    None,
    Return(Value),
    Break,
    Continue,
}

/// Values that can cross a parallel worker boundary.  Functions and mutable
/// collections deliberately do not implement this subset: they would either
/// require sharing the evaluator's Rc state or make evaluation order visible.
#[derive(Clone)]
enum ParallelValue {
    String(String),
    Int(i64),
    Float(f64),
    Bool(bool),
}

#[cfg(test)]
static PARALLEL_THREAD_IDS: std::sync::LazyLock<std::sync::Mutex<HashSet<std::thread::ThreadId>>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(HashSet::new()));

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

fn evaluate_parallel_expression(
    expression: &Expr,
    item: &ParallelValue,
) -> Result<ParallelValue, String> {
    let value = match expression {
        Expr::Literal(Literal::String(value)) => Value::String(value.clone()),
        Expr::Literal(Literal::Int(value)) => Value::Int(*value),
        Expr::Literal(Literal::Float(value)) => Value::Float(*value),
        Expr::Literal(Literal::Bool(value)) => Value::Bool(*value),
        Expr::Identifier(name) if name == "item" => item.as_value(),
        Expr::Unary { operator, operand } => {
            let operand = evaluate_parallel_expression(operand, item)?.into_value();
            operations::unary(operand, operator, None).map_err(|error| error.to_string())?
        }
        Expr::Binary {
            left,
            operator,
            right,
        } => {
            let left = evaluate_parallel_expression(left, item)?.into_value();
            let right = evaluate_parallel_expression(right, item)?.into_value();
            operations::binary(left, operator, right, None).map_err(|error| error.to_string())?
        }
        _ => return Err("expression is outside the parallel-safe subset".into()),
    };
    ParallelValue::from_value(&value)
        .ok_or_else(|| "parallel expressions must produce scalar values".into())
}

fn evaluate_parallel_chunk(
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
                    let result = evaluate_parallel_expression(expression, item)?;
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
                    current = Some(evaluate_parallel_expression(expression, item)?);
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

fn evaluate_parallel(
    values: Vec<Value>,
    transforms: &[PipelineStep],
    requested_workers: usize,
    requested_chunk_size: Option<usize>,
) -> Result<Vec<Value>, String> {
    let input: Vec<ParallelValue> = values
        .iter()
        .map(|value| {
            ParallelValue::from_value(value).ok_or_else(|| {
                "values or expressions are outside the parallel-safe subset".to_string()
            })
        })
        .collect::<Result<_, _>>()?;
    let requested_workers = requested_workers.max(1);
    let chunk_size = requested_chunk_size
        .unwrap_or_else(|| input.len().div_ceil(requested_workers))
        .max(1);
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
                    let result = evaluate_parallel_chunk(&chunks[index], transforms)?;
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

#[derive(Clone)]
struct Function {
    parameters: Vec<(String, Option<Type>, bool)>,
    return_type: Option<Type>,
    body: Arc<[Stmt]>,
}

impl Evaluator {
    pub fn new() -> Self {
        Self {
            scopes: ScopeStack::new(),
            variable_types: TypeScopes::new(),
            function_scopes: vec![HashMap::new()],
            output_enabled: true,
            ..Self::default()
        }
    }

    pub fn run(&mut self, program: &Program) -> Result<(), SimplyError> {
        self.run_with_output(program, true)
    }

    pub fn run_silent_resolved(
        &mut self,
        program: &Program,
        resolved: &Path,
    ) -> Result<(), SimplyError> {
        self.current_file = Some(resolved.to_path_buf());
        self.import_stack = vec![resolved.to_path_buf()];
        self.run_with_output(program, false)
    }

    fn run_with_output(
        &mut self,
        program: &Program,
        output_enabled: bool,
    ) -> Result<(), SimplyError> {
        self.output_enabled = output_enabled;
        match self.execute_statements(&program.statements)? {
            Flow::None => Ok(()),
            _ => Err(self.runtime_error("control statement is outside its valid context".into())),
        }
    }

    pub fn run_repl(&mut self, program: &Program) -> Result<(), SimplyError> {
        for statement in &program.statements {
            let (span, statement) = match statement {
                Stmt::Located { span, statement } => (Some(span.clone()), statement.as_ref()),
                statement => (None, statement),
            };
            self.current_span = span;
            if let Stmt::Expression(expression) = statement {
                let value = self.evaluate(expression)?;
                if !matches!(value, Value::Unit) {
                    print_value(&value);
                }
                continue;
            }
            match self.execute_statements(std::slice::from_ref(statement))? {
                Flow::None => {}
                _ => {
                    return Err(
                        self.runtime_error("control statement is outside its valid context".into())
                    );
                }
            }
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
        self.import_stack = vec![resolved];
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
                        print_value(&value);
                    }
                }
                Stmt::Import { path, alias } => {
                    let value = self.load_import(path)?;
                    self.define(alias.clone(), value).map_err(|error| {
                        self.runtime_error_with_code(DiagnosticCode::DuplicateDeclaration, error)
                    })?;
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
                        self.ensure_type(&value, expected, name)?;
                        self.variable_types
                            .define(name.clone(), expected.clone(), *mutable);
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
                            DiagnosticCode::InvalidReassignment,
                            format!("cannot reassign unknown variable `{name}`"),
                        ));
                    }
                    let value = self.evaluate(value)?;
                    if let Some(expected) = self.variable_types.lookup(name).cloned() {
                        self.ensure_reassignment_type(&value, &expected, name)?;
                    }
                    if !self.assign(name, value) {
                        return Err(self
                            .runtime_error(format!("cannot reassign unknown variable `{name}`")));
                    }
                }
                Stmt::CollectionOp {
                    name,
                    operation,
                    value,
                } => {
                    if !self.scopes.is_mutable(name) {
                        return Err(self.runtime_error_with_code(
                            DiagnosticCode::RuntimeCollection,
                            format!(
                                "cannot mutate immutable variable `{name}`; declare it with `mut`"
                            ),
                        ));
                    }
                    let value = self.evaluate(value)?;
                    if let Some(expected) = self.variable_types.lookup(name)
                        && let Type::List(element) = expected
                    {
                        self.ensure_type(&value, element, name)?;
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
                }
                Stmt::SetIndex { name, index, value } => {
                    if !self.scopes.is_mutable(name) {
                        return Err(self.runtime_error_with_code(
                            DiagnosticCode::RuntimeCollection,
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
                    }
                    let span = self.current_span.clone();
                    let target = match self.lookup_mut(name) {
                        Some(target) => target,
                        None => {
                            return Err(
                                self.runtime_error(format!("`{name}` is not a mutable collection"))
                            );
                        }
                    };
                    collections::set_index(target, index_value, value, span.as_ref())?;
                }
                Stmt::Destructure { names, value } => {
                    let values = match self.evaluate(value)? {
                        Value::Tuple(values) => owned_values(values),
                        _ => {
                            return Err(self.runtime_error("destructuring requires a tuple".into()));
                        }
                    };
                    if names.len() != values.len() {
                        return Err(
                            self.runtime_error("tuple and names have different lengths".into())
                        );
                    }
                    for ((name, mutable), value) in names.iter().zip(values) {
                        self.variable_types.define(
                            name.clone(),
                            Self::type_of_value(&value),
                            *mutable,
                        );
                        self.scopes
                            .define(name.clone(), value, *mutable)
                            .map_err(|error| {
                                self.runtime_error_with_code(
                                    DiagnosticCode::DuplicateDeclaration,
                                    error,
                                )
                            })?;
                    }
                }
                Stmt::Function {
                    name,
                    parameters,
                    return_type,
                    body,
                } => {
                    let function = self.track_function(Rc::new(FunctionValue {
                        parameters: parameters.clone(),
                        return_type: return_type.clone(),
                        body: Arc::clone(body),
                        captures: RefCell::new(if self.function_scopes.len() > 1 {
                            let dependencies = closure_dependencies(body, parameters);
                            self.scopes.values_for(&dependencies)
                        } else {
                            HashMap::new()
                        }),
                    }));
                    let function_value = Value::Function(function);
                    self.function_scopes
                        .last_mut()
                        .expect("function scope stack always has a global scope")
                        .insert(
                            name.clone(),
                            Arc::new(Function {
                                parameters: parameters.clone(),
                                return_type: return_type.clone(),
                                body: Arc::clone(body),
                            }),
                        );
                    self.variable_types.define(
                        name.clone(),
                        Type::Function {
                            parameters: parameters
                                .iter()
                                .map(|(_, typ, _)| typ.clone().map(Box::new))
                                .collect(),
                            return_type: return_type.clone().map(Box::new),
                        },
                        false,
                    );
                    self.scopes
                        .define(name.clone(), function_value, false)
                        .map_err(|error| {
                            self.runtime_error_with_code(
                                DiagnosticCode::DuplicateDeclaration,
                                error,
                            )
                        })?;
                }
                Stmt::Return(expr) => return Ok(Flow::Return(self.evaluate(expr)?)),
                Stmt::Throw(expr) => {
                    let value = self.evaluate(expr)?;
                    return Err(self
                        .runtime_error_with_code(DiagnosticCode::RuntimeGeneral, value.display()));
                }
                Stmt::Break => return Ok(Flow::Break),
                Stmt::Continue => return Ok(Flow::Continue),
                Stmt::For {
                    name,
                    mutable,
                    iterable,
                    body,
                } => {
                    let values: Box<dyn Iterator<Item = Value>> = match self.evaluate(iterable)? {
                        Value::Range { start, end } => Box::new(Value::range_values(start, end)),
                        Value::Array(values) | Value::List(values) | Value::Tuple(values) => {
                            Box::new(owned_values(values).into_iter())
                        }
                        Value::Hash(values) | Value::Tree(values) => {
                            Box::new(owned_map_values(values).into_iter())
                        }
                        _ => return Err(self.runtime_error("for requires a collection".into())),
                    };
                    self.push_scope();
                    for value in values {
                        if self.scopes.has_in_current_scope(name) {
                            self.scopes.assign_current(name, value);
                        } else {
                            self.scopes
                                .define(name.clone(), value, *mutable)
                                .map_err(|error| {
                                    self.runtime_error_with_code(
                                        DiagnosticCode::DuplicateDeclaration,
                                        error,
                                    )
                                })?;
                        }
                        match self.execute_statements(body)? {
                            Flow::None | Flow::Continue => {}
                            Flow::Break => break,
                            Flow::Return(value) => {
                                self.pop_scope();
                                return Ok(Flow::Return(value));
                            }
                        }
                    }
                    self.pop_scope();
                }
                Stmt::While { condition, body } => {
                    while match self.evaluate(condition)? {
                        Value::Bool(value) => value,
                        _ => {
                            return Err(
                                self.runtime_error("while condition must be a boolean".into())
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
                            return Err(self.runtime_error("if condition must be a boolean".into()));
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
                                catch
                                    .code
                                    .as_deref()
                                    .is_none_or(|code| code == error.code().as_str())
                            });
                            if let Some(catch) = catch {
                                self.push_scope();
                                let result = if let Some(name) = &catch.binding {
                                    self.variable_types.define(name.clone(), Type::Tree, false);
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

    fn load_import(&mut self, path: &str) -> Result<Value, SimplyError> {
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
        if self.import_stack.contains(&resolved) {
            return Err(self.runtime_error_with_code(
                DiagnosticCode::RuntimeImport,
                format!("cyclic import of `{path}`"),
            ));
        }
        let program = if let Some(program) = self.import_cache.borrow().get(&resolved) {
            Arc::clone(program)
        } else {
            let source =
                fs::read_to_string(&resolved).map_err(|error| self.file_error(&resolved, error))?;
            let tokens = Lexer::new(&source).tokenize()?;
            let program = Parser::new(tokens).parse()?;
            let program = Arc::new(program);
            self.import_cache
                .borrow_mut()
                .insert(resolved.clone(), Arc::clone(&program));
            program
        };
        let mut module = Self {
            current_file: Some(resolved.clone()),
            import_stack: self
                .import_stack
                .iter()
                .cloned()
                .chain(std::iter::once(resolved.clone()))
                .collect(),
            import_cache: Rc::clone(&self.import_cache),
            ..Self::new()
        };
        match module.execute_statements(&program.statements)? {
            Flow::Return(value) => Ok(value),
            Flow::None => {
                Err(self.runtime_error(format!("imported file `{path}` must return a value")))
            }
            Flow::Break | Flow::Continue => {
                Err(self.runtime_error("control statement is outside its valid context".into()))
            }
        }
    }

    fn file_error(&self, path: &Path, error: std::io::Error) -> SimplyError {
        self.runtime_error_with_code(
            DiagnosticCode::RuntimeImport,
            format!("could not open `{}`: {error}", path.display()),
        )
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
                let source = match self.evaluate(source)? {
                    Value::CsvStream(path) => return self.evaluate_csv_pipeline(&path, steps),
                    Value::Range { start, end } => {
                        return self.evaluate_range_pipeline(start, end, steps);
                    }
                    Value::Array(values) | Value::List(values) => owned_values(values),
                    _ => {
                        return Err(
                            self.runtime_error("pipeline source must be an array or list".into())
                        );
                    }
                };
                self.evaluate_pipeline(source, steps)
            }
            Expr::Identifier(name) => self
                .lookup(name)
                .cloned()
                .ok_or_else(|| self.runtime_error(format!("unknown variable `{name}`"))),
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
                if name == "send" {
                    if arguments.len() < 2 {
                        return Err(self.runtime_error_with_code(
                            DiagnosticCode::RuntimeMessage,
                            "`send` expects a receiver and a message",
                        ));
                    }
                    let receiver = self.evaluate(&arguments[0])?;
                    let message = match self.evaluate(&arguments[1])? {
                        Value::String(message) => message,
                        _ => {
                            return Err(self.runtime_error_with_code(
                                DiagnosticCode::RuntimeMessage,
                                "`send` message must be a string",
                            ));
                        }
                    };
                    let mut values = vec![receiver];
                    values.extend(
                        arguments[2..]
                            .iter()
                            .map(|argument| self.evaluate(argument))
                            .collect::<Result<Vec<_>, _>>()?,
                    );
                    return self.invoke_function(&message, values);
                }
                if name == "print" {
                    if arguments.len() != 1 {
                        return Err(self.runtime_error("`print` expects one argument".into()));
                    }
                    let value = self.evaluate(&arguments[0])?;
                    if self.output_enabled {
                        print_value(&value);
                    }
                    return Ok(Value::Unit);
                }
                if name == "type_of" {
                    if arguments.len() != 1 {
                        return Err(self.runtime_error("`type_of` expects one argument".into()));
                    }
                    let value = self.evaluate(&arguments[0])?;
                    let type_name = match value {
                        Value::Unit => "Unit",
                        Value::String(_) => "String",
                        Value::Int(_) => "Int",
                        Value::Float(_) => "Float",
                        Value::Bool(_) => "Bool",
                        Value::Range { .. } => "Array",
                        Value::CsvStream(_) => "List",
                        Value::Array(_) => "Array",
                        Value::List(_) => "List",
                        Value::Tuple(_) => "Tuple",
                        Value::Hash(_) => "Hash",
                        Value::Tree(_) => "Tree",
                        Value::Matrix(_) => "Matrix",
                        Value::Function(_) => "Function",
                    };
                    return Ok(Value::String(type_name.into()));
                }
                if name == "contains" {
                    if arguments.len() != 2 {
                        return Err(self.runtime_error("`contains` expects two arguments".into()));
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
                        Value::Range { start, end } => {
                            matches!(searched, Value::Int(value) if value >= start && value < end)
                        }
                        Value::Hash(values) | Value::Tree(values) => {
                            values.values().any(|value| value == &searched)
                        }
                        _ => {
                            return Err(self.runtime_error(
                                "`contains` requires a collection or string".into(),
                            ));
                        }
                    };
                    return Ok(Value::Bool(result));
                }
                if name == "any" || name == "all" {
                    if arguments.len() != 1 {
                        return Err(self.runtime_error(format!("`{name}` expects one argument")));
                    }
                    let collection = self.evaluate(&arguments[0])?;
                    let values = match collection {
                        Value::Array(values) | Value::List(values) | Value::Tuple(values) => values,
                        Value::Hash(values) | Value::Tree(values) => {
                            shared_values(values.values().cloned().collect())
                        }
                        _ => {
                            return Err(
                                self.runtime_error(format!("`{name}` requires a collection"))
                            );
                        }
                    };
                    let mut result = name == "all";
                    for value in values.iter() {
                        let Value::Bool(value) = value else {
                            return Err(self.runtime_error(format!(
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
                        return Err(self.runtime_error("`join` expects two arguments".into()));
                    }
                    let collection = self.evaluate(&arguments[0])?;
                    let separator = match self.evaluate(&arguments[1])? {
                        Value::String(value) => value,
                        _ => {
                            return Err(
                                self.runtime_error("`join` separator must be a string".into())
                            );
                        }
                    };
                    let values = match collection {
                        Value::Array(values) | Value::List(values) | Value::Tuple(values) => values,
                        Value::Hash(values) | Value::Tree(values) => {
                            shared_values(values.values().cloned().collect())
                        }
                        _ => {
                            return Err(self
                                .runtime_error("`join` requires an array, list, or tuple".into()));
                        }
                    };
                    let mut parts = Vec::with_capacity(values.len());
                    for value in values.iter() {
                        let Value::String(value) = value else {
                            return Err(self
                                .runtime_error("`join` requires a collection of strings".into()));
                        };
                        parts.push(value.as_str());
                    }
                    return Ok(Value::String(parts.join(&separator)));
                }
                if name == "total" {
                    if arguments.len() != 1 {
                        return Err(self.runtime_error("`total` expects one argument".into()));
                    }
                    let collection = self.evaluate(&arguments[0])?;
                    let values = match collection {
                        Value::Array(values) | Value::List(values) | Value::Tuple(values) => {
                            values.iter().cloned().collect::<Vec<_>>()
                        }
                        Value::Range { start, end } => Value::range_values(start, end).collect(),
                        _ => {
                            return Err(self.runtime_error(
                                "`total` requires an array, list, tuple, or range".into(),
                            ));
                        }
                    };
                    let mut total = Value::Int(0);
                    for value in values {
                        total = operations::binary(
                            total,
                            &BinaryOperator::Add,
                            value,
                            self.current_span.as_ref(),
                        )?;
                    }
                    return Ok(total);
                }
                if name == "trim" {
                    if arguments.len() != 1 {
                        return Err(self.runtime_error("`trim` expects one argument".into()));
                    }
                    let value = match self.evaluate(&arguments[0])? {
                        Value::String(value) => value,
                        _ => return Err(self.runtime_error("`trim` expects a string".into())),
                    };
                    return Ok(Value::String(value.trim().into()));
                }
                if name == "to_float" {
                    if arguments.len() != 1 {
                        return Err(self.runtime_error("`to_float` expects one argument".into()));
                    }
                    let value = match self.evaluate(&arguments[0])? {
                        Value::String(value) => value,
                        _ => {
                            return Err(self.runtime_error_with_code(
                                DiagnosticCode::RuntimeConversion,
                                "`to_float` needs text containing a valid number",
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
                        return Err(self.runtime_error("`to_int` expects one argument".into()));
                    }
                    let value = match self.evaluate(&arguments[0])? {
                        Value::String(value) => value,
                        _ => {
                            return Err(self.runtime_error_with_code(
                                DiagnosticCode::RuntimeConversion,
                                "`to_int` needs text containing a whole number",
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
                        return Err(self.runtime_error("`abs` expects one numeric argument".into()));
                    }
                    return match self.evaluate(&arguments[0])? {
                        Value::Int(value) => value.checked_abs().map(Value::Int).ok_or_else(|| {
                            self.runtime_error("integer absolute value overflow".into())
                        }),
                        Value::Float(value) if value.is_finite() => Ok(Value::Float(value.abs())),
                        _ => Err(self.runtime_error("`abs` expects an integer or float".into())),
                    };
                }
                if name == "round" {
                    if arguments.len() != 2 {
                        return Err(
                            self.runtime_error("`round` expects a number and decimal count".into())
                        );
                    }
                    let value = self.evaluate(&arguments[0])?;
                    let decimals = match self.evaluate(&arguments[1])? {
                        Value::Int(value) if (0..=15).contains(&value) => value as i32,
                        _ => {
                            return Err(self.runtime_error(
                                "`round` decimal count must be an integer from 0 to 15".into(),
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
                                Err(self.runtime_error(
                                    "`round` result is outside the finite Float range".into(),
                                ))
                            }
                        }
                        _ => {
                            Err(self
                                .runtime_error("`round` expects an integer or finite float".into()))
                        }
                    };
                }
                if name == "clamp" {
                    if arguments.len() != 3 {
                        return Err(self
                            .runtime_error("`clamp` expects value, minimum, and maximum".into()));
                    }
                    let value = self.evaluate(&arguments[0])?;
                    let minimum = self.evaluate(&arguments[1])?;
                    let maximum = self.evaluate(&arguments[2])?;
                    return clamp_numeric(value, minimum, maximum, self.current_span.as_ref());
                }
                if name == "split" {
                    if arguments.len() != 2 {
                        return Err(self.runtime_error("`split` expects two arguments".into()));
                    }
                    let value = match self.evaluate(&arguments[0])? {
                        Value::String(value) => value,
                        _ => return Err(self.runtime_error("`split` expects a string".into())),
                    };
                    let separator = match self.evaluate(&arguments[1])? {
                        Value::String(separator) => separator,
                        _ => {
                            return Err(
                                self.runtime_error("`split` separator must be a string".into())
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
                        return Err(self.runtime_error("`replace` expects three arguments".into()));
                    }
                    let value = match self.evaluate(&arguments[0])? {
                        Value::String(value) => value,
                        _ => return Err(self.runtime_error("`replace` expects strings".into())),
                    };
                    let from = match self.evaluate(&arguments[1])? {
                        Value::String(from) => from,
                        _ => return Err(self.runtime_error("`replace` expects strings".into())),
                    };
                    let to = match self.evaluate(&arguments[2])? {
                        Value::String(to) => to,
                        _ => return Err(self.runtime_error("`replace` expects strings".into())),
                    };
                    return Ok(Value::String(value.replace(&from, &to)));
                }
                if name == "starts_with" || name == "ends_with" {
                    if arguments.len() != 2 {
                        return Err(self.runtime_error(format!("`{name}` expects two arguments")));
                    }
                    let value = match self.evaluate(&arguments[0])? {
                        Value::String(value) => value,
                        _ => return Err(self.runtime_error(format!("`{name}` expects strings"))),
                    };
                    let part = match self.evaluate(&arguments[1])? {
                        Value::String(part) => part,
                        _ => return Err(self.runtime_error(format!("`{name}` expects strings"))),
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
                        return Err(self.runtime_error("`is_empty` expects one argument".into()));
                    }
                    let value = self.evaluate(&arguments[0])?;
                    let result = match value {
                        Value::Array(values) | Value::List(values) | Value::Tuple(values) => {
                            values.is_empty()
                        }
                        Value::Range { start, end } => start >= end,
                        Value::Hash(values) | Value::Tree(values) => values.is_empty(),
                        Value::String(value) => value.is_empty(),
                        _ => {
                            return Err(self.runtime_error(
                                "`is_empty` requires a collection or string".into(),
                            ));
                        }
                    };
                    return Ok(Value::Bool(result));
                }
                if name == "reverse" {
                    if arguments.len() != 1 {
                        return Err(self.runtime_error("`reverse` expects one argument".into()));
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
                        Value::Range { start, end } => {
                            Value::Array(shared_values(Value::range_values(start, end).collect()))
                        }
                        _ => {
                            return Err(self.runtime_error(
                                "`reverse` requires an array, list, or tuple".into(),
                            ));
                        }
                    });
                }
                if name == "length" || name == "count" {
                    if arguments.len() != 1 {
                        return Err(self.runtime_error(format!("`{name}` expects one argument")));
                    }
                    return match self.evaluate(&arguments[0])? {
                        Value::Array(values) | Value::List(values) | Value::Tuple(values) => {
                            Ok(Value::Int(values.len() as i64))
                        }
                        Value::Range { start, end } => Ok(Value::Int(Value::range_len(start, end))),
                        Value::Hash(values) | Value::Tree(values) => {
                            Ok(Value::Int(values.len() as i64))
                        }
                        Value::String(value) => Ok(Value::Int(value.chars().count() as i64)),
                        _ => {
                            Err(self
                                .runtime_error(format!("`{name}` requires a collection or string")))
                        }
                    };
                }
                if name == "range" {
                    if arguments.len() != 2 {
                        return Err(
                            self.runtime_error("`range` expects two integer arguments".into())
                        );
                    }
                    let start = self.evaluate(&arguments[0])?;
                    let end = self.evaluate(&arguments[1])?;
                    if let (Value::Int(start), Value::Int(end)) = (start, end) {
                        return Ok(Value::Range { start, end });
                    }
                    return Err(self.runtime_error("`range` expects two integer arguments".into()));
                }
                if name == "csv_rows" {
                    if arguments.len() != 1 {
                        return Err(self.runtime_error("`csv_rows` expects one path".into()));
                    }
                    let path = match self.evaluate(&arguments[0])? {
                        Value::String(path) => path,
                        _ => {
                            return Err(
                                self.runtime_error("`csv_rows` path must be a string".into())
                            );
                        }
                    };
                    return Ok(Value::CsvStream(path));
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
            Expr::Index { target, index } => {
                let target = self.evaluate(target)?;
                let index_value = self.evaluate(index)?;
                collections::index(&target, &index_value, self.current_span.as_ref())
            }
            Expr::Field { target, name } => match self.evaluate(target)? {
                Value::Hash(values) | Value::Tree(values) => values
                    .get(name)
                    .cloned()
                    .ok_or_else(|| self.runtime_error(format!("unknown field `{name}`"))),
                _ => Err(self.runtime_error("value has no fields".into())),
            },
        }
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
                        self.runtime_error_with_code(
                            DiagnosticCode::RuntimeMessage,
                            format!("unknown function `{name}`"),
                        )
                    })?;
                self.track_function(Rc::new(FunctionValue {
                    parameters: function.parameters.clone(),
                    return_type: function.return_type.clone(),
                    body: Arc::clone(&function.body),
                    captures: RefCell::new(HashMap::new()),
                }))
            }
        };
        if function.parameters.len() != values.len() {
            return Err(self.runtime_error(format!(
                "function `{name}` expects {} arguments, got {}",
                function.parameters.len(),
                values.len()
            )));
        }
        for ((parameter, expected, _), value) in function.parameters.iter().zip(&values) {
            if let Some(expected) = expected {
                self.ensure_type(value, expected, parameter)?;
            }
        }
        self.push_scope();
        for (capture, value) in function.captures.borrow().iter() {
            self.variable_types
                .define(capture.clone(), Self::type_of_value(value), false);
            self.scopes
                .define(capture.clone(), value.clone(), false)
                .map_err(|error| {
                    self.runtime_error_with_code(DiagnosticCode::DuplicateDeclaration, error)
                })?;
        }
        for ((parameter, _, mutable), value) in function.parameters.iter().zip(values) {
            self.variable_types
                .define(parameter.clone(), Self::type_of_value(&value), *mutable);
            self.scopes
                .define(parameter.clone(), value, *mutable)
                .map_err(|error| {
                    self.runtime_error_with_code(DiagnosticCode::DuplicateDeclaration, error)
                })?;
        }
        let result = self.execute_statements(&function.body);
        let result = match result {
            Ok(flow) => match flow {
                Flow::None => Value::Unit,
                Flow::Return(value) => value,
                Flow::Break | Flow::Continue => {
                    self.pop_scope();
                    return Err(self.runtime_error("break/continue used outside a loop".into()));
                }
            },
            Err(error) => {
                self.pop_scope();
                return Err(error);
            }
        };
        self.pop_scope();
        if let Some(expected) = &function.return_type {
            self.ensure_type(&result, expected, name)?;
        }
        Ok(result)
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
            return Err(self.runtime_error("`chunk` requires `parallel` or `checkpoint`".into()));
        }
        if steps
            .iter()
            .any(|step| matches!(step, PipelineStep::Checkpoint(_)))
        {
            return Err(self.runtime_error(
                "`checkpoint` requires a `csv_rows` source and `write_csv` terminal".into(),
            ));
        }
        self.push_scope();
        let result = self.evaluate_pipeline_steps(&mut values, steps);
        self.pop_scope();
        result
    }

    fn evaluate_range_pipeline(
        &mut self,
        start: i64,
        end: i64,
        steps: &[PipelineStep],
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
            return Err(self.runtime_error("`chunk` requires `parallel` or `checkpoint`".into()));
        }
        if steps
            .iter()
            .any(|step| matches!(step, PipelineStep::Checkpoint(_)))
        {
            return Err(self.runtime_error(
                "`checkpoint` requires a `csv_rows` source and `write_csv` terminal".into(),
            ));
        }
        self.push_scope();
        let result = self.evaluate_streaming_pipeline(Value::range_values(start, end), steps);
        self.pop_scope();
        result
    }

    fn evaluate_csv_pipeline(
        &mut self,
        input_path: &str,
        steps: &[PipelineStep],
    ) -> Result<Value, SimplyError> {
        if steps
            .iter()
            .any(|step| matches!(step, PipelineStep::Chunk(_)))
            && !steps
                .iter()
                .any(|step| matches!(step, PipelineStep::Checkpoint(_)))
        {
            return Err(self.runtime_error("`chunk` requires `parallel` or `checkpoint`".into()));
        }
        if steps
            .iter()
            .any(|step| matches!(step, PipelineStep::Parallel(_)))
        {
            return Err(self.runtime_error(
                "`parallel` does not support CSV row collections or output terminals".into(),
            ));
        }
        let terminal = steps.last().ok_or_else(|| {
            self.runtime_error("csv_rows pipeline requires a terminal step".into())
        })?;
        let output_path = match terminal {
            PipelineStep::WriteCsv(path_expression) => {
                let output_path = match self.evaluate(path_expression)? {
                    Value::String(path) => path,
                    _ => return Err(self.runtime_error("write_csv path must be a string".into())),
                };
                Some(output_path)
            }
            PipelineStep::Sum
            | PipelineStep::Count
            | PipelineStep::Average
            | PipelineStep::Min
            | PipelineStep::Max
            | PipelineStep::Partition { .. } => None,
            _ => {
                return Err(self.runtime_error(
                    "csv_rows pipeline must end with an aggregate, partition, or write_csv(\"path\")"
                        .into(),
                ));
            }
        };
        let input = fs::File::open(input_path)
            .map_err(|error| self.file_error(Path::new(input_path), error))?;
        let chunk_size = steps.iter().find_map(|step| match step {
            PipelineStep::Chunk(size) => Some(*size as usize),
            _ => None,
        });
        let checkpoint_path = steps.iter().find_map(|step| match step {
            PipelineStep::Checkpoint(path) => Some(path),
            _ => None,
        });
        self.push_scope();
        let result = (|| {
            let checkpoint_path = match checkpoint_path {
                Some(path) => match self.evaluate(path)? {
                    Value::String(path) => Some(path),
                    _ => return Err(self.runtime_error("checkpoint path must be a string".into())),
                },
                None => None,
            };
            if let Some(output_path) = output_path.as_deref()
                && paths_are_same(input_path, output_path)
                    .map_err(|message| self.runtime_error(message))?
            {
                return Err(self.runtime_error(
                    "`csv_rows` input and `write_csv` output must be different files".into(),
                ));
            }
            if let Some(checkpoint_path) = checkpoint_path.as_deref() {
                let temporary_checkpoint = format!("{checkpoint_path}.tmp");
                for protected_path in [Some(input_path), output_path.as_deref()]
                    .into_iter()
                    .flatten()
                {
                    if paths_are_same(checkpoint_path, protected_path)
                        .map_err(|message| self.runtime_error(message))?
                        || paths_are_same(&temporary_checkpoint, protected_path)
                            .map_err(|message| self.runtime_error(message))?
                    {
                        return Err(self.runtime_error(
                            "`checkpoint` and its temporary file must not overlap the CSV input or output"
                                .into(),
                        ));
                    }
                }
            }
            let checkpoint_state = checkpoint_path
                .as_deref()
                .map(read_checkpoint)
                .transpose()
                .map_err(|message| self.runtime_error(message))?
                .flatten();
            if checkpoint_path.is_some() && output_path.is_none() {
                return Err(
                    self.runtime_error("`checkpoint` requires a `write_csv` terminal".into())
                );
            }
            let resume_at = checkpoint_state
                .as_ref()
                .map_or(0, |checkpoint| checkpoint.position);
            if let Some(state) = checkpoint_state.as_ref() {
                let output_path = output_path.as_deref().ok_or_else(|| {
                    self.runtime_error("checkpoint output path is missing".into())
                })?;
                validate_checkpoint_source(state, input_path, output_path)
                    .map_err(|message| self.runtime_error(message))?;
                validate_checkpoint_output(state, output_path)
                    .map_err(|message| self.runtime_error(message))?;
            }
            if resume_at > 0 && !matches!(terminal, PipelineStep::WriteCsv(_)) {
                return Err(self.runtime_error(
                    "checkpoint resume requires a `write_csv` terminal; aggregate state is not checkpointed yet"
                        .into(),
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
                    return Err(self.runtime_error(format!(
                        "checkpoint output length exceeds the current output file `{path}`"
                    )));
                }
                output
                    .set_len(output_len)
                    .map_err(|error| self.file_error(Path::new(path), error))?;
            }
            let interval = chunk_size.unwrap_or(1).max(1);
            let mut total = Value::Int(0);
            let mut count = 0i64;
            let mut minimum: Option<Value> = None;
            let mut maximum: Option<Value> = None;
            let mut partitioned = match terminal {
                PipelineStep::Partition { rules, .. } => Some(empty_partition_categories(rules)),
                _ => None,
            };
            let mut reader = BufReader::new(input);
            let mut index = 0usize;
            while let Some(record) = read_csv_record(&mut reader)
                .map_err(|error| self.file_error(Path::new(input_path), error))?
            {
                if index < resume_at {
                    index += 1;
                    continue;
                }
                let line = record.strip_suffix('\n').unwrap_or(&record);
                let line = line.strip_suffix('\r').unwrap_or(line);
                let row = parse_csv_record(line).map_err(|message| {
                    self.runtime_error(format!("invalid CSV row in `{input_path}`: {message}"))
                })?;
                let mut current = Some(Value::List(shared_values(row)));
                for step in &steps[..steps.len() - 1] {
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
                        PipelineStep::Sum
                        | PipelineStep::Count
                        | PipelineStep::Average
                        | PipelineStep::Min
                        | PipelineStep::Max
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
                    index += 1;
                    checkpoint_progress(
                        checkpoint_path.as_deref(),
                        index,
                        interval,
                        output.as_mut(),
                        input_path,
                        output_path.as_deref(),
                    )
                    .map_err(|message| self.runtime_error(message))?;
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
                    checkpoint_progress(
                        checkpoint_path.as_deref(),
                        index,
                        interval,
                        output.as_mut(),
                        input_path,
                        output_path.as_deref(),
                    )
                    .map_err(|message| self.runtime_error(message))?;
                    continue;
                }
                match terminal {
                    PipelineStep::Sum => {
                        total = operations::binary(
                            total,
                            &BinaryOperator::Add,
                            value,
                            self.current_span.as_ref(),
                        )?;
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
                    PipelineStep::WriteCsv(_) => {
                        let values = match value {
                            Value::Array(values) | Value::List(values) | Value::Tuple(values) => {
                                values
                            }
                            _ => {
                                return Err(self.runtime_error(
                                    "write_csv requires derive to produce a row collection".into(),
                                ));
                            }
                        };
                        write_csv_row(
                            output
                                .as_mut()
                                .expect("write_csv output should be initialized"),
                            values.as_slice(),
                        )
                        .map_err(|message| self.runtime_error(message))?;
                    }
                    _ => unreachable!(),
                }
                index += 1;
                checkpoint_progress(
                    checkpoint_path.as_deref(),
                    index,
                    interval,
                    output.as_mut(),
                    input_path,
                    output_path.as_deref(),
                )
                .map_err(|message| self.runtime_error(message))?;
            }
            let terminal_result = if let Some(output) = output.as_mut() {
                output
                    .flush()
                    .map_err(|error| self.runtime_error(format!("could not flush CSV: {error}")))?;
                Ok(Value::Unit)
            } else if matches!(terminal, PipelineStep::Count) {
                Ok(Value::Int(count))
            } else if matches!(terminal, PipelineStep::Average) {
                if count == 0 {
                    Err(self.runtime_error("average requires at least one numeric value".into()))
                } else {
                    Ok(Value::Float(
                        total_to_f64(total, self.current_span.as_ref())? / count as f64,
                    ))
                }
            } else if matches!(terminal, PipelineStep::Min) {
                minimum.ok_or_else(|| {
                    self.runtime_error("min requires at least one numeric value".into())
                })
            } else if matches!(terminal, PipelineStep::Max) {
                maximum.ok_or_else(|| {
                    self.runtime_error("max requires at least one numeric value".into())
                })
            } else if let PipelineStep::Partition { .. } = terminal {
                match partitioned.take() {
                    Some(categories) => Ok(partition_result(categories)),
                    None => Err(self.runtime_error("partition result was not initialized".into())),
                }
            } else {
                Ok(total)
            };
            if terminal_result.is_ok()
                && let Some(path) = checkpoint_path.as_deref()
            {
                remove_checkpoint(path).map_err(|message| self.runtime_error(message))?;
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
    ) -> Result<Value, SimplyError> {
        if steps
            .iter()
            .any(|step| matches!(step, PipelineStep::Chunk(_) | PipelineStep::Checkpoint(_)))
        {
            return self.evaluate_streaming_pipeline(std::mem::take(values), steps);
        }
        if matches!(steps.last(), Some(PipelineStep::Partition { .. })) {
            return self.evaluate_streaming_pipeline(std::mem::take(values), steps);
        }
        if !matches!(
            steps.last(),
            Some(
                PipelineStep::Count
                    | PipelineStep::Sum
                    | PipelineStep::Average
                    | PipelineStep::Min
                    | PipelineStep::Max
                    | PipelineStep::Partition { .. }
            )
        ) {
            return self.evaluate_streaming_pipeline(std::mem::take(values), steps);
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
            return self.evaluate_fused_pipeline(values, &steps[..steps.len() - 1], steps.last());
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
                                return Err(self.runtime_error(
                                    "pipeline `where` condition must return a boolean".into(),
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
                    let mut total = Value::Int(0);
                    for value in values.drain(..) {
                        let mapped = self.evaluate_pipeline_item(value, expression)?;
                        total = operations::binary(
                            total,
                            &BinaryOperator::Add,
                            mapped,
                            self.current_span.as_ref(),
                        )?;
                    }
                    return Ok(total);
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
                    let mut total = Value::Int(0);
                    for value in values.drain(..) {
                        total = operations::binary(
                            total,
                            &BinaryOperator::Add,
                            value,
                            self.current_span.as_ref(),
                        )?;
                    }
                    return Ok(total);
                }
                PipelineStep::Count => return Ok(Value::Int(values.len() as i64)),
                PipelineStep::Average | PipelineStep::Min | PipelineStep::Max => {
                    return self.evaluate_streaming_pipeline(std::mem::take(values), steps);
                }
                PipelineStep::Partition { .. } => {
                    return self.evaluate_streaming_pipeline(std::mem::take(values), steps);
                }
                PipelineStep::WriteCsv(_) => {
                    return Err(self.runtime_error(
                        "`write_csv` requires a `csv_rows` streaming source".into(),
                    ));
                }
                PipelineStep::Chunk(_)
                | PipelineStep::Parallel(_)
                | PipelineStep::Checkpoint(_) => {
                    // These controls are consumed by the streaming path.
                }
            }
        }
        Ok(Value::List(shared_values(std::mem::take(values))))
    }

    fn evaluate_streaming_pipeline<I>(
        &mut self,
        values: I,
        steps: &[PipelineStep],
    ) -> Result<Value, SimplyError>
    where
        I: IntoIterator<Item = Value>,
    {
        if steps
            .iter()
            .any(|step| matches!(step, PipelineStep::Checkpoint(_)))
        {
            return Err(self.runtime_error(
                "`checkpoint` requires a `csv_rows` source and `write_csv` terminal".into(),
            ));
        }
        let terminal = match steps.last() {
            Some(PipelineStep::Count) => Some(PipelineStep::Count),
            Some(PipelineStep::Sum) => Some(PipelineStep::Sum),
            Some(PipelineStep::Average) => Some(PipelineStep::Average),
            Some(PipelineStep::Min) => Some(PipelineStep::Min),
            Some(PipelineStep::Max) => Some(PipelineStep::Max),
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
        let parallel_workers = steps.iter().find_map(|step| match step {
            PipelineStep::Parallel(workers) => Some(*workers as usize),
            _ => None,
        });
        let chunk_size = steps.iter().find_map(|step| match step {
            PipelineStep::Chunk(size) => Some(*size as usize),
            _ => None,
        });
        if let Some(workers) = parallel_workers {
            if workers == 0 {
                return Err(self.runtime_error("parallel worker count must be positive".into()));
            }
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
                return Err(self.runtime_error("`parallel` requires an aggregate terminal".into()));
            }
            if !transforms.iter().all(|step| match step {
                PipelineStep::Where(expression) | PipelineStep::Derive(expression) => {
                    is_parallel_safe_expression(expression)
                }
                PipelineStep::Parallel(_) | PipelineStep::Chunk(_) => true,
                _ => false,
            }) {
                return Err(self.runtime_error(
                    "`parallel` requires only parallel-safe `where` and `derive` expressions"
                        .into(),
                ));
            }
            let input = values.into_iter().collect::<Vec<_>>();
            if input.iter().any(|value| {
                !matches!(
                    value,
                    Value::String(_) | Value::Int(_) | Value::Float(_) | Value::Bool(_)
                )
            }) {
                return Err(self.runtime_error("`parallel` requires scalar source items".into()));
            }
            match evaluate_parallel(input, transforms, workers, chunk_size) {
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
                    return self.evaluate_streaming_pipeline(output, &sequential_steps);
                }
                Err(message) => return Err(self.runtime_error(message)),
            }
        }
        let mut output = Vec::new();
        let mut partitioned = match &terminal {
            Some(PipelineStep::Partition { rules, .. }) => Some(empty_partition_categories(rules)),
            _ => None,
        };
        let mut count = 0i64;
        let mut total = Value::Int(0);
        let mut minimum = None;
        let mut maximum = None;
        let mut output_file = match terminal {
            Some(PipelineStep::WriteCsv(ref path_expression)) => {
                let path = match self.evaluate(path_expression)? {
                    Value::String(path) => path,
                    _ => return Err(self.runtime_error("write_csv path must be a string".into())),
                };
                Some(
                    fs::File::create(&path)
                        .map_err(|error| self.file_error(Path::new(&path), error))?,
                )
            }
            _ => None,
        };

        for value in values {
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
                continue;
            }
            match terminal {
                Some(PipelineStep::Count) => count += 1,
                Some(PipelineStep::Sum) => {
                    total = operations::binary(
                        total,
                        &BinaryOperator::Add,
                        value,
                        self.current_span.as_ref(),
                    )?;
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
                Some(PipelineStep::WriteCsv(_)) => {
                    let values = match value {
                        Value::Array(values) | Value::List(values) | Value::Tuple(values) => values,
                        _ => {
                            return Err(self.runtime_error(
                                "write_csv requires derive to produce a row collection".into(),
                            ));
                        }
                    };
                    write_csv_row(
                        output_file
                            .as_mut()
                            .expect("write_csv output should be initialized"),
                        values.as_slice(),
                    )
                    .map_err(|message| self.runtime_error(message))?;
                }
                None => output.push(value),
                Some(PipelineStep::Where(_)) | Some(PipelineStep::Derive(_)) => unreachable!(),
                Some(PipelineStep::Chunk(_))
                | Some(PipelineStep::Parallel(_))
                | Some(PipelineStep::Checkpoint(_)) => {
                    unreachable!()
                }
                Some(PipelineStep::Partition { .. }) => {
                    unreachable!("partition is handled before the terminal match")
                }
            }
        }

        match terminal {
            Some(PipelineStep::Count) => Ok(Value::Int(count)),
            Some(PipelineStep::Sum) => Ok(total),
            Some(PipelineStep::Average) => {
                if count == 0 {
                    Err(self.runtime_error("average requires at least one numeric value".into()))
                } else {
                    Ok(Value::Float(
                        total_to_f64(total, self.current_span.as_ref())? / count as f64,
                    ))
                }
            }
            Some(PipelineStep::Min) => minimum.ok_or_else(|| {
                self.runtime_error("min requires at least one numeric value".into())
            }),
            Some(PipelineStep::Max) => maximum.ok_or_else(|| {
                self.runtime_error("max requires at least one numeric value".into())
            }),
            Some(PipelineStep::Partition { .. }) => match partitioned {
                Some(categories) => Ok(partition_result(categories)),
                None => Err(self.runtime_error("partition result was not initialized".into())),
            },
            Some(PipelineStep::WriteCsv(_)) => {
                if let Some(output_file) = output_file.as_mut() {
                    output_file.flush().map_err(|error| {
                        self.runtime_error(format!("could not flush CSV: {error}"))
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
            | Some(PipelineStep::Checkpoint(_)) => {
                unreachable!("control steps are not terminals")
            }
        }
    }

    fn evaluate_fused_pipeline(
        &mut self,
        values: &mut Vec<Value>,
        transforms: &[PipelineStep],
        terminal: Option<&PipelineStep>,
    ) -> Result<Value, SimplyError> {
        let mut total = Value::Int(0);
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
                    | PipelineStep::WriteCsv(_)
                    | PipelineStep::Partition { .. } => {
                        unreachable!()
                    }
                    PipelineStep::Chunk(_)
                    | PipelineStep::Parallel(_)
                    | PipelineStep::Checkpoint(_) => {}
                }
            }

            let Some(value) = current else {
                continue;
            };
            match terminal {
                Some(PipelineStep::Count) => count += 1,
                Some(PipelineStep::Sum) => {
                    total = operations::binary(
                        total,
                        &BinaryOperator::Add,
                        value,
                        self.current_span.as_ref(),
                    )?;
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
            Some(PipelineStep::Sum) => Ok(total),
            Some(PipelineStep::Average) => {
                if count == 0 {
                    Err(self.runtime_error("average requires at least one numeric value".into()))
                } else {
                    Ok(Value::Float(
                        total_to_f64(total, self.current_span.as_ref())? / count as f64,
                    ))
                }
            }
            Some(PipelineStep::Min) => minimum.ok_or_else(|| {
                self.runtime_error("min requires at least one numeric value".into())
            }),
            Some(PipelineStep::Max) => maximum.ok_or_else(|| {
                self.runtime_error("max requires at least one numeric value".into())
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
            _ => Err(self.runtime_error("pipeline `where` condition must return a boolean".into())),
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
                            return Err(self.runtime_error(
                                "partition condition must return a boolean".into(),
                            ));
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

    fn value_type_name(value: &Value) -> &'static str {
        match value {
            Value::Unit => "Unit",
            Value::String(_) => "String",
            Value::Int(_) => "Int",
            Value::Float(_) => "Float",
            Value::Bool(_) => "Bool",
            Value::Range { .. } => "Array",
            Value::CsvStream(_) => "List",
            Value::Array(_) => "Array",
            Value::List(_) => "List",
            Value::Tuple(_) => "Tuple",
            Value::Hash(_) => "Hash",
            Value::Tree(_) => "Tree",
            Value::Matrix(_) => "Matrix",
            Value::Function(_) => "Function",
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
            | (Value::Function(_), Type::Function { .. })
            | (_, Type::Unknown) => true,
            (Value::Range { .. }, Type::Array(element)) => Type::Int.compatible_with(element),
            (Value::CsvStream(_), Type::List(row)) if matches!(row.as_ref(), Type::List(field) if matches!(field.as_ref(), Type::String)) => {
                true
            }
            (Value::Array(values), Type::Array(element))
            | (Value::List(values), Type::List(element)) => values
                .iter()
                .all(|value| self.value_matches_type(value, element)),
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
            Value::Range { .. } => Type::Array(Box::new(Type::Int)),
            Value::CsvStream(_) => Type::List(Box::new(Type::List(Box::new(Type::String)))),
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
            Value::Hash(_) => Type::Hash,
            Value::Tree(_) => Type::Tree,
            Value::Matrix(_) => Type::Matrix,
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::value::shared_values;

    #[test]
    fn parallel_workers_execute_scalar_chunks_and_preserve_order() {
        PARALLEL_THREAD_IDS
            .lock()
            .expect("parallel test lock")
            .clear();
        let values = (0..16).map(Value::Int).collect();
        let transforms = [PipelineStep::Derive(Expr::Binary {
            left: Box::new(Expr::Identifier("item".into())),
            operator: BinaryOperator::Multiply,
            right: Box::new(Expr::Literal(Literal::Int(2))),
        })];
        let output =
            evaluate_parallel(values, &transforms, 4, Some(8)).expect("parallel evaluation");
        assert_eq!(
            output,
            (0..16)
                .map(|value| Value::Int(value * 2))
                .collect::<Vec<_>>()
        );
        assert_eq!(
            PARALLEL_THREAD_IDS
                .lock()
                .expect("parallel test lock")
                .len(),
            2
        );
    }

    #[test]
    fn parallel_safe_subset_rejects_calls_and_collections() {
        assert!(!is_parallel_safe_expression(&Expr::Call {
            name: "abs".into(),
            arguments: vec![Expr::Identifier("item".into())],
        }));
        assert!(
            evaluate_parallel(
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
}
