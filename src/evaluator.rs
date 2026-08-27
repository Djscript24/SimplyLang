use std::{
    collections::{BTreeMap, HashMap},
    fs,
    path::{Path, PathBuf},
};

use crate::{
    ast::{
        BinaryOperator, CollectionOperation, Expr, Literal, PipelineStep, Program, Stmt, Type,
        UnaryOperator,
    },
    error::{SimplyError, Span},
    lexer::Lexer,
    parser::Parser,
};

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Unit,
    String(String),
    Int(i64),
    Float(f64),
    Bool(bool),
    Array(Vec<Value>),
    List(Vec<Value>),
    Tuple(Vec<Value>),
    Hash(BTreeMap<String, Value>),
    Tree(BTreeMap<String, Value>),
    Matrix(Vec<Value>),
}

impl Value {
    fn display(&self) -> String {
        match self {
            Self::Unit => String::new(),
            Self::String(value) => value.clone(),
            Self::Int(value) => value.to_string(),
            Self::Float(value) => value.to_string(),
            Self::Bool(value) => value.to_string(),
            Self::Array(values) | Self::List(values) | Self::Tuple(values) => format!(
                "[{}]",
                values
                    .iter()
                    .map(Value::display)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Self::Hash(values) => format!(
                "{{{}}}",
                values
                    .iter()
                    .map(|(key, value)| format!("{key}: {}", value.display()))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Self::Tree(values) => format!(
                "tree {{{}}}",
                values
                    .iter()
                    .map(|(key, value)| format!("{key}: {}", value.display()))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Self::Matrix(values) => format!(
                "[{}]",
                values
                    .iter()
                    .map(Value::display)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        }
    }
}

#[derive(Default)]
pub struct Evaluator {
    scopes: Vec<HashMap<String, Value>>,
    variable_types: HashMap<String, Type>,
    functions: HashMap<String, Function>,
    current_span: Option<Span>,
    current_file: Option<PathBuf>,
    import_stack: Vec<PathBuf>,
}

impl Evaluator {
    fn define(&mut self, name: String, value: Value) {
        self.scopes
            .last_mut()
            .expect("evaluator always has a global scope")
            .insert(name, value);
    }

    fn lookup(&self, name: &str) -> Option<&Value> {
        self.scopes.iter().rev().find_map(|scope| scope.get(name))
    }

    fn lookup_mut(&mut self, name: &str) -> Option<&mut Value> {
        self.scopes
            .iter_mut()
            .rev()
            .find_map(|scope| scope.get_mut(name))
    }

    fn contains(&self, name: &str) -> bool {
        self.lookup(name).is_some()
    }

    fn assign(&mut self, name: &str, value: Value) -> bool {
        if let Some(binding) = self.lookup_mut(name) {
            *binding = value;
            true
        } else {
            false
        }
    }

    fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }

    fn pop_scope(&mut self) {
        debug_assert!(self.scopes.len() > 1);
        self.scopes.pop();
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
    parameters: Vec<(String, Option<Type>)>,
    return_type: Option<Type>,
    body: Vec<Stmt>,
}

impl Evaluator {
    pub fn new() -> Self {
        Self {
            scopes: vec![HashMap::new()],
            ..Self::default()
        }
    }

    pub fn run(&mut self, program: &Program) -> Result<(), SimplyError> {
        match self.execute_statements(&program.statements)? {
            Flow::None => Ok(()),
            _ => Err(self.runtime_error("control statement is outside its valid context".into())),
        }
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
                    println!("{}", value.display());
                }
                Stmt::Import { path, alias } => {
                    let value = self.load_import(path)?;
                    self.define(alias.clone(), value);
                }
                Stmt::Expression(expr) => {
                    self.evaluate(expr)?;
                }
                Stmt::Assign {
                    name,
                    declared_type,
                    value,
                } => {
                    let value = self.evaluate(value)?;
                    if let Some(expected) = declared_type {
                        self.ensure_type(&value, expected, name)?;
                        self.variable_types.insert(name.clone(), expected.clone());
                    } else if let Some(expected) = self.variable_types.get(name).cloned() {
                        self.ensure_type(&value, &expected, name)?;
                    } else {
                        self.variable_types
                            .insert(name.clone(), Self::type_of_value(&value));
                    }
                    self.define(name.clone(), value);
                }
                Stmt::Reassign { name, value } => {
                    if !self.contains(name) {
                        return Err(self
                            .runtime_error(format!("cannot reassign unknown variable `{name}`")));
                    }
                    let value = self.evaluate(value)?;
                    if let Some(expected) = self.variable_types.get(name).cloned() {
                        self.ensure_type(&value, &expected, name)?;
                    }
                    if !self.assign(name, value) {
                        return Err(
                            self.runtime_error(format!("cannot reassign unknown variable `{name}"))
                        );
                    }
                }
                Stmt::CollectionOp {
                    name,
                    operation,
                    value,
                } => {
                    let value = self.evaluate(value)?;
                    if let Some(expected) = self.variable_types.get(name) {
                        match expected {
                            Type::List(element) => self.ensure_type(&value, element, name)?,
                            _ => {}
                        }
                    }
                    match self.lookup_mut(name) {
                        Some(Value::List(values)) => match operation {
                            CollectionOperation::Add => values.push(value),
                            CollectionOperation::Remove => {
                                if let Some(position) =
                                    values.iter().position(|item| item == &value)
                                {
                                    values.remove(position);
                                }
                            }
                        },
                        _ => return Err(self.runtime_error(format!("`{name}` is not a list"))),
                    }
                }
                Stmt::SetIndex { name, index, value } => {
                    let index_value = self.evaluate(index)?;
                    let value = self.evaluate(value)?;
                    if let Some(expected) = self.variable_types.get(name) {
                        match expected {
                            Type::Array(element) | Type::List(element) => {
                                self.ensure_type(&value, element, name)?
                            }
                            _ => {}
                        }
                    }
                    if let Value::String(key) = &index_value {
                        if let Some(Value::Hash(values)) = self.lookup_mut(name) {
                            values.insert(key.clone(), value);
                            continue;
                        }
                    }
                    let index = match index_value {
                        Value::Int(index) if index >= 0 => index as usize,
                        _ => {
                            return Err(self.runtime_error(
                                "collection index must be a non-negative integer".into(),
                            ));
                        }
                    };
                    match self.lookup_mut(name) {
                        Some(Value::Array(values)) | Some(Value::List(values)) => {
                            if index >= values.len() {
                                return Err(
                                    self.runtime_error("collection index out of bounds".into())
                                );
                            }
                            values[index] = value;
                        }
                        _ => {
                            return Err(
                                self.runtime_error(format!("`{name}` is not a mutable collection"))
                            );
                        }
                    }
                }
                Stmt::Destructure { names, value } => {
                    let values = match self.evaluate(value)? {
                        Value::Tuple(values) => values,
                        _ => {
                            return Err(self.runtime_error("destructuring requires a tuple".into()));
                        }
                    };
                    if names.len() != values.len() {
                        return Err(
                            self.runtime_error("tuple and names have different lengths".into())
                        );
                    }
                    for (name, value) in names.iter().zip(values) {
                        self.define(name.clone(), value);
                    }
                }
                Stmt::Function {
                    name,
                    parameters,
                    return_type,
                    body,
                } => {
                    self.functions.insert(
                        name.clone(),
                        Function {
                            parameters: parameters.clone(),
                            return_type: return_type.clone(),
                            body: body.clone(),
                        },
                    );
                }
                Stmt::Return(expr) => return Ok(Flow::Return(self.evaluate(expr)?)),
                Stmt::Break => return Ok(Flow::Break),
                Stmt::Continue => return Ok(Flow::Continue),
                Stmt::For {
                    name,
                    iterable,
                    body,
                } => {
                    let values = match self.evaluate(iterable)? {
                        Value::Array(values) | Value::List(values) | Value::Tuple(values) => values,
                        Value::Hash(values) | Value::Tree(values) => values.into_values().collect(),
                        _ => return Err(self.runtime_error("for requires a collection".into())),
                    };
                    self.push_scope();
                    for value in values {
                        self.define(name.clone(), value);
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
                    match self.execute_statements(branch)? {
                        Flow::None => {}
                        flow => return Ok(flow),
                    }
                }
            }
        }
        Ok(Flow::None)
    }

    fn load_import(&self, path: &str) -> Result<Value, SimplyError> {
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
            return Err(self.runtime_error(format!("cyclic import of `{path}`")));
        }
        let source =
            fs::read_to_string(&resolved).map_err(|error| self.file_error(&resolved, error))?;
        let tokens = Lexer::new(&source).tokenize()?;
        let program = Parser::new(tokens).parse()?;
        let mut module = Self {
            current_file: Some(resolved.clone()),
            import_stack: self
                .import_stack
                .iter()
                .cloned()
                .chain(std::iter::once(resolved))
                .collect(),
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
        self.runtime_error(format!("could not open `{}`: {error}", path.display()))
    }

    fn evaluate(&mut self, expr: &Expr) -> Result<Value, SimplyError> {
        match expr {
            Expr::Literal(literal) => Ok(match literal {
                Literal::String(value) => Value::String(value.clone()),
                Literal::Int(value) => Value::Int(*value),
                Literal::Float(value) => Value::Float(*value),
                Literal::Bool(value) => Value::Bool(*value),
            }),
            Expr::Array(values) => Ok(Value::Array(self.evaluate_values(values)?)),
            Expr::List(values) => Ok(Value::List(self.evaluate_values(values)?)),
            Expr::Tuple(values) => Ok(Value::Tuple(self.evaluate_values(values)?)),
            Expr::Matrix(values) => Ok(Value::Matrix(self.evaluate_values(values)?)),
            Expr::Hash(entries) => {
                let mut values = BTreeMap::new();
                for (name, expression) in entries {
                    values.insert(name.clone(), self.evaluate(expression)?);
                }
                Ok(Value::Hash(values))
            }
            Expr::Tree(entries) => {
                let mut values = BTreeMap::new();
                for (name, expression) in entries {
                    values.insert(name.clone(), self.evaluate(expression)?);
                }
                Ok(Value::Tree(values))
            }
            Expr::Pipeline { source, steps } => {
                let source = match self.evaluate(source)? {
                    Value::Array(values) | Value::List(values) => values,
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
                match operator {
                    UnaryOperator::Not => match value {
                        Value::Bool(value) => Ok(Value::Bool(!value)),
                        _ => Err(self.runtime_error("`not` requires a boolean".into())),
                    },
                    UnaryOperator::Negate => match value {
                        Value::Int(value) => value
                            .checked_neg()
                            .map(Value::Int)
                            .ok_or_else(|| self.runtime_error("integer overflow".into())),
                        Value::Float(value) => Ok(Value::Float(-value)),
                        _ => Err(self.runtime_error("unary `-` requires a number".into())),
                    },
                    UnaryOperator::Transpose => match value {
                        Value::Matrix(rows) => self.matrix_transpose(rows),
                        _ => Err(self.runtime_error("transpose requires a matrix".into())),
                    },
                }
            }
            Expr::Binary {
                left,
                operator,
                right,
            } => {
                let left = self.evaluate(left)?;
                if matches!(operator, BinaryOperator::And | BinaryOperator::Or) {
                    if let Value::Bool(value) = left {
                        if (*operator == BinaryOperator::And && !value)
                            || (*operator == BinaryOperator::Or && value)
                        {
                            return Ok(Value::Bool(value));
                        }
                    }
                }
                let right = self.evaluate(right)?;
                self.evaluate_binary(left, operator, right)
            }
            Expr::Call { name, arguments } => {
                if name == "send" {
                    if arguments.len() < 2 {
                        return Err(
                            self.runtime_error("`send` expects a receiver and a message".into())
                        );
                    }
                    let receiver = self.evaluate(&arguments[0])?;
                    let message = match self.evaluate(&arguments[1])? {
                        Value::String(message) => message,
                        _ => {
                            return Err(
                                self.runtime_error("`send` message must be a string".into())
                            );
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
                    println!("{}", value.display());
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
                        Value::Array(_) => "Array",
                        Value::List(_) => "List",
                        Value::Tuple(_) => "Tuple",
                        Value::Hash(_) => "Hash",
                        Value::Tree(_) => "Tree",
                        Value::Matrix(_) => "Matrix",
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
                if name == "length" || name == "count" {
                    if arguments.len() != 1 {
                        return Err(self.runtime_error(format!("`{name}` expects one argument")));
                    }
                    return match self.evaluate(&arguments[0])? {
                        Value::Array(values) | Value::List(values) | Value::Tuple(values) => {
                            Ok(Value::Int(values.len() as i64))
                        }
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
                        return Ok(Value::Array((start..end).map(Value::Int).collect()));
                    }
                    return Err(self.runtime_error("`range` expects two integer arguments".into()));
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
                if let (Value::Matrix(rows), Value::Tuple(coordinates)) = (&target, &index_value) {
                    if coordinates.len() != 2 {
                        return Err(
                            self.runtime_error("matrix index requires row and column".into())
                        );
                    }
                    let row = match &coordinates[0] {
                        Value::Int(value) if *value >= 0 => *value as usize,
                        _ => return Err(self.runtime_error("matrix index must be integer".into())),
                    };
                    let column = match &coordinates[1] {
                        Value::Int(value) if *value >= 0 => *value as usize,
                        _ => return Err(self.runtime_error("matrix index must be integer".into())),
                    };
                    return match rows.get(row) {
                        Some(Value::Array(values)) => values
                            .get(column)
                            .cloned()
                            .ok_or_else(|| self.runtime_error("matrix index out of bounds".into())),
                        _ => Err(self.runtime_error("matrix row is not an array".into())),
                    };
                }
                if let Value::Hash(values) | Value::Tree(values) = target.clone() {
                    if let Value::String(key) = index_value {
                        return values
                            .get(&key)
                            .cloned()
                            .ok_or_else(|| self.runtime_error(format!("unknown key `{key}`")));
                    }
                    return Err(self.runtime_error("hash key must be a string".into()));
                }
                let index = match index_value {
                    Value::Int(index) if index >= 0 => index as usize,
                    _ => {
                        return Err(self.runtime_error(
                            "collection index must be a non-negative integer".into(),
                        ));
                    }
                };
                match target {
                    Value::Array(values)
                    | Value::List(values)
                    | Value::Tuple(values)
                    | Value::Matrix(values) => values
                        .get(index)
                        .cloned()
                        .ok_or_else(|| self.runtime_error("collection index out of bounds".into())),
                    _ => Err(self.runtime_error("value is not indexable".into())),
                }
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
        let function = self
            .functions
            .get(name)
            .cloned()
            .ok_or_else(|| self.runtime_error(format!("unknown function `{name}`")))?;
        if function.parameters.len() != values.len() {
            return Err(self.runtime_error(format!(
                "function `{name}` expects {} arguments, got {}",
                function.parameters.len(),
                values.len()
            )));
        }
        for ((parameter, expected), value) in function.parameters.iter().zip(&values) {
            if let Some(expected) = expected {
                self.ensure_type(value, expected, parameter)?;
            }
        }
        self.push_scope();
        let saved_variable_types = self.variable_types.clone();
        for ((parameter, _), value) in function.parameters.iter().zip(values) {
            self.define(parameter.clone(), value);
        }
        let result = self.execute_statements(&function.body);
        self.variable_types = saved_variable_types;
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
        self.push_scope();
        let result = self.evaluate_pipeline_steps(&mut values, steps);
        self.pop_scope();
        result
    }

    fn evaluate_pipeline_steps(
        &mut self,
        values: &mut Vec<Value>,
        steps: &[PipelineStep],
    ) -> Result<Value, SimplyError> {
        for step in steps {
            match step {
                PipelineStep::Filter(expression) => {
                    let mut kept = Vec::new();
                    for value in values.drain(..) {
                        self.define("item".into(), value.clone());
                        match self.evaluate(expression)? {
                            Value::Bool(true) => kept.push(value),
                            Value::Bool(false) => {}
                            _ => {
                                return Err(self.runtime_error(
                                    "pipeline filter must return a boolean".into(),
                                ));
                            }
                        }
                    }
                    *values = kept;
                }
                PipelineStep::Map(expression) => {
                    let mut mapped = Vec::new();
                    for value in values.drain(..) {
                        self.define("item".into(), value);
                        mapped.push(self.evaluate(expression)?);
                    }
                    *values = mapped;
                }
                PipelineStep::Sum => {
                    let mut total = Value::Int(0);
                    for value in values.drain(..) {
                        total = self.evaluate_binary(total, &BinaryOperator::Add, value)?;
                    }
                    return Ok(total);
                }
                PipelineStep::Count => return Ok(Value::Int(values.len() as i64)),
            }
        }
        Ok(Value::List(std::mem::take(values)))
    }

    fn evaluate_binary(
        &self,
        left: Value,
        operator: &BinaryOperator,
        right: Value,
    ) -> Result<Value, SimplyError> {
        use BinaryOperator::*;
        match operator {
            Add => match (left, right) {
                (Value::Matrix(left), Value::Matrix(right)) => self.matrix_add(left, right),
                (Value::String(left), Value::String(right)) => Ok(Value::String(left + &right)),
                (Value::Int(left), Value::Int(right)) => left
                    .checked_add(right)
                    .map(Value::Int)
                    .ok_or_else(|| self.runtime_error("integer arithmetic error".into())),
                (Value::Float(left), Value::Float(right)) => Ok(Value::Float(left + right)),
                (Value::Int(left), Value::Float(right)) => Ok(Value::Float(left as f64 + right)),
                (Value::Float(left), Value::Int(right)) => Ok(Value::Float(left + right as f64)),
                _ => Err(self.runtime_error("`+` requires two compatible values".into())),
            },
            Subtract | Multiply | Divide | Remainder => {
                self.numeric_operation(left, operator, right)
            }
            MatrixMultiply => self.matrix_multiply(left, right),
            Greater | GreaterEqual | Less | LessEqual => {
                self.numeric_comparison(left, operator, right)
            }
            Equal => Ok(Value::Bool(left == right)),
            NotEqual => Ok(Value::Bool(left != right)),
            And => match (left, right) {
                (Value::Bool(left), Value::Bool(right)) => Ok(Value::Bool(left && right)),
                _ => Err(self.runtime_error("`and` requires two booleans".into())),
            },
            Or => match (left, right) {
                (Value::Bool(left), Value::Bool(right)) => Ok(Value::Bool(left || right)),
                _ => Err(self.runtime_error("`or` requires two booleans".into())),
            },
        }
    }

    fn numeric_operation(
        &self,
        left: Value,
        operator: &BinaryOperator,
        right: Value,
    ) -> Result<Value, SimplyError> {
        if let (Value::Int(left), Value::Int(right)) = (&left, &right) {
            if *right == 0 && matches!(operator, BinaryOperator::Divide | BinaryOperator::Remainder)
            {
                return Err(self.runtime_error("division by zero".into()));
            }
            let result = match operator {
                BinaryOperator::Subtract => left.checked_sub(*right),
                BinaryOperator::Multiply => left.checked_mul(*right),
                BinaryOperator::Divide => left.checked_div(*right),
                BinaryOperator::Remainder => left.checked_rem(*right),
                _ => unreachable!(),
            };
            return result
                .map(Value::Int)
                .ok_or_else(|| self.runtime_error("integer arithmetic error".into()));
        }
        let (left, right) = match (left, right) {
            (Value::Float(left), Value::Float(right)) => (left, right),
            (Value::Int(left), Value::Float(right)) => (left as f64, right),
            (Value::Float(left), Value::Int(right)) => (left, right as f64),
            _ => return Err(self.runtime_error("arithmetic requires two numbers".into())),
        };
        if right == 0.0 && matches!(operator, BinaryOperator::Divide | BinaryOperator::Remainder) {
            return Err(self.runtime_error("division by zero".into()));
        }
        let result = match operator {
            BinaryOperator::Subtract => left - right,
            BinaryOperator::Multiply => left * right,
            BinaryOperator::Divide => left / right,
            BinaryOperator::Remainder => left % right,
            _ => unreachable!(),
        };
        Ok(Value::Float(result))
    }

    fn matrix_add(&self, left: Vec<Value>, right: Vec<Value>) -> Result<Value, SimplyError> {
        self.matrix_numbers(&left)?;
        self.matrix_numbers(&right)?;
        if left.len() != right.len() {
            return Err(self.runtime_error("matrix dimensions do not match".into()));
        }
        let mut rows = Vec::new();
        for (left_row, right_row) in left.into_iter().zip(right) {
            match (left_row, right_row) {
                (Value::Array(left), Value::Array(right)) if left.len() == right.len() => {
                    let mut row = Vec::new();
                    for (a, b) in left.into_iter().zip(right) {
                        row.push(self.evaluate_binary(a, &BinaryOperator::Add, b)?);
                    }
                    rows.push(Value::Array(row));
                }
                _ => return Err(self.runtime_error("invalid matrix rows".into())),
            }
        }
        Ok(Value::Matrix(rows))
    }

    fn matrix_multiply(&self, left: Value, right: Value) -> Result<Value, SimplyError> {
        let (left, right) = match (left, right) {
            (Value::Matrix(left), Value::Matrix(right)) => (left, right),
            _ => return Err(self.runtime_error("matrix multiply requires matrices".into())),
        };
        let left_rows = self.matrix_numbers(&left)?;
        let right_rows = self.matrix_numbers(&right)?;
        if left_rows.is_empty()
            || right_rows.is_empty()
            || left_rows.iter().any(|row| row.len() != left_rows[0].len())
            || right_rows
                .iter()
                .any(|row| row.len() != right_rows[0].len())
            || left_rows[0].len() != right_rows.len()
        {
            return Err(self.runtime_error("matrix dimensions do not match".into()));
        }
        let mut result = Vec::new();
        for row in &left_rows {
            let mut output = Vec::new();
            for column in 0..right_rows[0].len() {
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

    fn matrix_numbers(&self, matrix: &[Value]) -> Result<Vec<Vec<f64>>, SimplyError> {
        let numbers: Vec<Vec<f64>> = matrix
            .iter()
            .map(|row| match row {
                Value::Array(values) => values
                    .iter()
                    .map(|value| match value {
                        Value::Int(value) => Ok(*value as f64),
                        Value::Float(value) => Ok(*value),
                        _ => Err(self.runtime_error("matrix values must be numeric".into())),
                    })
                    .collect(),
                _ => Err(self.runtime_error("matrix rows must be arrays".into())),
            })
            .collect::<Result<_, _>>()?;
        if let Some(first_width) = numbers.first().map(Vec::len) {
            if numbers.iter().any(|row| row.len() != first_width) {
                return Err(self.runtime_error("matrix rows must have equal widths".into()));
            }
        }
        Ok(numbers)
    }

    fn matrix_transpose(&self, rows: Vec<Value>) -> Result<Value, SimplyError> {
        let numbers = self.matrix_numbers(&rows)?;
        if numbers.is_empty() {
            return Ok(Value::Matrix(Vec::new()));
        }
        let width = numbers[0].len();
        if numbers.iter().any(|row| row.len() != width) {
            return Err(self.runtime_error("invalid matrix rows".into()));
        }
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
    fn numeric_comparison(
        &self,
        left: Value,
        operator: &BinaryOperator,
        right: Value,
    ) -> Result<Value, SimplyError> {
        let (left, right) = match (left, right) {
            (Value::Int(left), Value::Int(right)) => (left as f64, right as f64),
            (Value::Float(left), Value::Float(right)) => (left, right),
            (Value::Int(left), Value::Float(right)) => (left as f64, right),
            (Value::Float(left), Value::Int(right)) => (left, right as f64),
            _ => return Err(self.runtime_error("comparison requires two numbers".into())),
        };
        let result = match operator {
            BinaryOperator::Greater => left > right,
            BinaryOperator::GreaterEqual => left >= right,
            BinaryOperator::Less => left < right,
            BinaryOperator::LessEqual => left <= right,
            _ => unreachable!(),
        };
        Ok(Value::Bool(result))
    }

    fn runtime_error(&self, message: String) -> SimplyError {
        SimplyError::Runtime {
            span: self.current_span.clone().unwrap_or_else(|| Span::new(0, 0)),
            message,
        }
    }

    fn ensure_type(&self, value: &Value, expected: &Type, name: &str) -> Result<(), SimplyError> {
        let valid = matches!(
            (value, expected),
            (Value::String(_), Type::String)
                | (Value::Int(_), Type::Int)
                | (Value::Float(_), Type::Float)
                | (Value::Bool(_), Type::Bool)
                | (Value::Hash(_), Type::Hash)
                | (Value::Tree(_), Type::Tree)
                | (Value::Matrix(_), Type::Matrix)
        );
        let valid = valid
            || match (value, expected) {
                (Value::Array(values), Type::Array(element))
                | (Value::List(values), Type::List(element)) => values
                    .iter()
                    .all(|item| self.value_matches_type(item, element)),
                (Value::Tuple(values), Type::Tuple(types)) => {
                    values.len() == types.len()
                        && values
                            .iter()
                            .zip(types)
                            .all(|(item, expected)| self.value_matches_type(item, expected))
                }
                _ => false,
            };
        if valid {
            Ok(())
        } else {
            Err(self.runtime_error(format!(
                "value assigned to `{name}` has the wrong type: expected {}, found {}",
                Self::type_name(expected),
                Self::value_type_name(value)
            )))
        }
    }

    fn type_name(expected: &Type) -> String {
        match expected {
            Type::Unknown => "Unknown".into(),
            Type::Unit => "Unit".into(),
            Type::String => "String".into(),
            Type::Int => "Int".into(),
            Type::Float => "Float".into(),
            Type::Bool => "Bool".into(),
            Type::Array(element) => format!("Array[{}]", Self::type_name(element)),
            Type::List(element) => format!("List[{}]", Self::type_name(element)),
            Type::Tuple(types) => format!(
                "Tuple[{}]",
                types
                    .iter()
                    .map(Self::type_name)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Type::Hash => "Hash".into(),
            Type::Tree => "Tree".into(),
            Type::Matrix => "Matrix".into(),
            Type::Function { .. } => "Function".into(),
        }
    }

    fn value_type_name(value: &Value) -> &'static str {
        match value {
            Value::Unit => "Unit",
            Value::String(_) => "String",
            Value::Int(_) => "Int",
            Value::Float(_) => "Float",
            Value::Bool(_) => "Bool",
            Value::Array(_) => "Array",
            Value::List(_) => "List",
            Value::Tuple(_) => "Tuple",
            Value::Hash(_) => "Hash",
            Value::Tree(_) => "Tree",
            Value::Matrix(_) => "Matrix",
        }
    }

    fn value_matches_type(&self, value: &Value, expected: &Type) -> bool {
        match (value, expected) {
            (Value::String(_), Type::String)
            | (Value::Int(_), Type::Int)
            | (Value::Float(_), Type::Float)
            | (Value::Bool(_), Type::Bool)
            | (Value::Hash(_), Type::Hash)
            | (Value::Tree(_), Type::Tree)
            | (Value::Matrix(_), Type::Matrix) => true,
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
            Value::Array(values) => Type::Array(Box::new(
                values.first().map(Self::type_of_value).unwrap_or(Type::Int),
            )),
            Value::List(values) => Type::List(Box::new(
                values.first().map(Self::type_of_value).unwrap_or(Type::Int),
            )),
            Value::Tuple(values) => Type::Tuple(values.iter().map(Self::type_of_value).collect()),
            Value::Hash(_) => Type::Hash,
            Value::Tree(_) => Type::Tree,
            Value::Matrix(_) => Type::Matrix,
            Value::Unit => Type::Unit,
        }
    }
}
