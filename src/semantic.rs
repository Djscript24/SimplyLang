use std::collections::HashMap;

use crate::{
    ast::{BinaryOperator, Expr, Literal, PipelineStep, Program, Stmt, UnaryOperator},
    error::{DiagnosticCode, SimplyError, Span},
    types::Type,
};

#[derive(Clone)]
struct FunctionSignature {
    parameters: Vec<Option<Type>>,
    return_type: Option<Type>,
}

#[derive(Default)]
pub struct SemanticAnalyzer {
    variables: ScopeStack,
    functions: HashMap<String, FunctionSignature>,
    current_span: Option<Span>,
    loop_depth: usize,
    function_depth: usize,
    function_return: Option<Type>,
    saw_return: bool,
}

#[derive(Clone)]
struct ScopeStack {
    scopes: Vec<HashMap<String, Type>>,
}

impl ScopeStack {
    fn new() -> Self {
        Self {
            scopes: vec![HashMap::new()],
        }
    }

    fn insert(&mut self, name: String, typ: Type) -> Option<Type> {
        self.scopes
            .last_mut()
            .expect("semantic analyzer always has a global scope")
            .insert(name, typ)
    }

    fn get(&self, name: &str) -> Option<&Type> {
        self.scopes.iter().rev().find_map(|scope| scope.get(name))
    }

    fn remove(&mut self, name: &str) -> Option<Type> {
        self.scopes
            .last_mut()
            .expect("semantic analyzer always has a global scope")
            .remove(name)
    }

    fn push(&mut self) {
        self.scopes.push(HashMap::new());
    }

    fn pop(&mut self) {
        debug_assert!(self.scopes.len() > 1);
        self.scopes.pop();
    }
}

impl Default for ScopeStack {
    fn default() -> Self {
        Self::new()
    }
}

impl SemanticAnalyzer {
    pub fn new() -> Self {
        Self {
            variables: ScopeStack::new(),
            ..Self::default()
        }
    }

    pub fn analyze(&mut self, program: &Program) -> Result<(), SimplyError> {
        self.collect_functions(&program.statements);
        self.analyze_statements(&program.statements)
    }

    fn collect_functions(&mut self, statements: &[Stmt]) {
        for statement in statements {
            let statement = match statement {
                Stmt::Located { statement, .. } => statement.as_ref(),
                statement => statement,
            };
            if let Stmt::Function {
                name,
                parameters,
                return_type,
                ..
            } = statement
            {
                self.functions.insert(
                    name.clone(),
                    FunctionSignature {
                        parameters: parameters
                            .iter()
                            .map(|(_, parameter_type)| parameter_type.clone())
                            .collect(),
                        return_type: return_type.clone(),
                    },
                );
                self.variables.insert(
                    name.clone(),
                    Type::Function {
                        parameters: parameters
                            .iter()
                            .map(|(_, parameter_type)| parameter_type.clone().map(Box::new))
                            .collect(),
                        return_type: return_type.clone().map(Box::new),
                    },
                );
            }
        }
    }

    fn analyze_statements(&mut self, statements: &[Stmt]) -> Result<(), SimplyError> {
        for statement in statements {
            match statement {
                Stmt::Located { span, statement } => {
                    self.current_span = Some(span.clone());
                    self.analyze_statements(std::slice::from_ref(statement.as_ref()))?;
                }
                Stmt::Say(expression) | Stmt::Expression(expression) => {
                    self.analyze_expression(expression)?;
                }
                Stmt::Import { alias, .. } => {
                    self.variables.insert(alias.clone(), Type::Unknown);
                }
                Stmt::Assign {
                    name,
                    declared_type,
                    value,
                } => {
                    let actual = self.analyze_expression(value)?;
                    let expected = declared_type.clone().unwrap_or_else(|| actual.clone());
                    if !actual.compatible_with(&expected) {
                        return Err(self.type_error(&expected, &actual, name));
                    }
                    self.variables.insert(name.clone(), expected);
                }
                Stmt::Reassign { name, value } => {
                    let expected = self.variables.get(name).cloned().ok_or_else(|| {
                        self.error(
                            DiagnosticCode::UndefinedVariable,
                            format!("unknown variable `{name}`"),
                        )
                    })?;
                    let actual = self.analyze_expression(value)?;
                    if !actual.compatible_with(&expected) {
                        return Err(self.type_error(&expected, &actual, name));
                    }
                }
                Stmt::SetIndex { name, index, value } => {
                    let target = self.variables.get(name).cloned().ok_or_else(|| {
                        self.error(
                            DiagnosticCode::UndefinedVariable,
                            format!("unknown variable `{name}`"),
                        )
                    })?;
                    let index_type = self.analyze_expression(index)?;
                    let value_type = self.analyze_expression(value)?;
                    match target {
                        Type::Array(element) | Type::List(element) => {
                            self.require_type(&Type::Int, &index_type)?;
                            self.require_type(&element, &value_type)?;
                        }
                        Type::Hash => self.require_type(&Type::String, &index_type)?,
                        _ => {
                            return Err(self.error(
                                DiagnosticCode::SemanticCollectionOperation,
                                format!("`{name}` is not mutable"),
                            ));
                        }
                    }
                }
                Stmt::Destructure { names, value } => {
                    let value_type = self.analyze_expression(value)?;
                    match value_type {
                        Type::Tuple(types) if types.len() == names.len() => {
                            for (name, value_type) in names.iter().zip(types) {
                                self.variables.insert(name.clone(), value_type);
                            }
                        }
                        Type::Tuple(types) => {
                            return Err(self.error(
                                DiagnosticCode::SemanticDestructure,
                                format!(
                                    "tuple has {} values, but {} names were provided",
                                    types.len(),
                                    names.len()
                                ),
                            ));
                        }
                        Type::Unknown => {}
                        _ => {
                            return Err(self.error(
                                DiagnosticCode::SemanticDestructure,
                                "destructuring requires a tuple",
                            ));
                        }
                    }
                }
                Stmt::CollectionOp {
                    name,
                    operation: _,
                    value,
                } => {
                    let target = self.variables.get(name).cloned().ok_or_else(|| {
                        self.error(
                            DiagnosticCode::UndefinedVariable,
                            format!("unknown variable `{name}`"),
                        )
                    })?;
                    let value_type = self.analyze_expression(value)?;
                    match target {
                        Type::List(element) => self.require_type(&element, &value_type)?,
                        _ => {
                            return Err(self.error(
                                DiagnosticCode::SemanticCollectionOperation,
                                format!("`{name}` is not a list"),
                            ));
                        }
                    }
                }
                Stmt::Function {
                    name,
                    parameters,
                    return_type,
                    body,
                } => {
                    let saved_function_depth = self.function_depth;
                    let saved_return = self.function_return.clone();
                    let saved_saw_return = self.saw_return;
                    self.variables.push();
                    self.function_return = return_type.clone();
                    self.function_depth += 1;
                    self.saw_return = false;
                    for (parameter, parameter_type) in parameters {
                        self.variables.insert(
                            parameter.clone(),
                            parameter_type.clone().unwrap_or(Type::Unknown),
                        );
                    }
                    if let Err(error) = self.analyze_statements(body) {
                        self.variables.pop();
                        self.function_depth = saved_function_depth;
                        self.function_return = saved_return;
                        self.saw_return = saved_saw_return;
                        return Err(error);
                    }
                    if let Some(expected) = return_type
                        && !self.saw_return
                        && *expected != Type::Unknown
                    {
                        self.variables.pop();
                        self.function_depth = saved_function_depth;
                        self.function_return = saved_return;
                        self.saw_return = saved_saw_return;
                        return Err(self.error(
                            DiagnosticCode::SemanticMissingReturn,
                            format!("function `{name}` must return {}", expected.name()),
                        ));
                    }
                    self.variables.pop();
                    self.function_depth = saved_function_depth;
                    self.function_return = saved_return;
                    self.saw_return = saved_saw_return;
                }
                Stmt::Return(expression) => {
                    let actual = self.analyze_expression(expression)?;
                    if self.function_depth == 0 {
                        return Err(self.error(
                            DiagnosticCode::InvalidReturn,
                            "return used outside a function",
                        ));
                    }
                    if let Some(expected) = self.function_return.clone() {
                        self.require_type(&expected, &actual)?;
                    }
                    self.saw_return = true;
                }
                Stmt::If {
                    condition,
                    then_branch,
                    else_branch,
                } => {
                    let condition_type = self.analyze_expression(condition)?;
                    self.require_type(&Type::Bool, &condition_type)?;
                    self.variables.push();
                    if let Err(error) = self.analyze_statements(then_branch) {
                        self.variables.pop();
                        return Err(error);
                    }
                    self.variables.pop();
                    self.variables.push();
                    if let Err(error) = self.analyze_statements(else_branch) {
                        self.variables.pop();
                        return Err(error);
                    }
                    self.variables.pop();
                }
                Stmt::For {
                    name,
                    iterable,
                    body,
                } => {
                    let iterable_type = self.analyze_expression(iterable)?;
                    let element = Self::element_type(&iterable_type).ok_or_else(|| {
                        self.error(
                            DiagnosticCode::SemanticCollection,
                            "for requires a collection",
                        )
                    })?;
                    self.variables.push();
                    self.variables.insert(name.clone(), element);
                    self.loop_depth += 1;
                    if let Err(error) = self.analyze_statements(body) {
                        self.loop_depth -= 1;
                        self.variables.pop();
                        return Err(error);
                    }
                    self.loop_depth -= 1;
                    self.variables.pop();
                }
                Stmt::While { condition, body } => {
                    let condition_type = self.analyze_expression(condition)?;
                    self.require_type(&Type::Bool, &condition_type)?;
                    self.loop_depth += 1;
                    self.analyze_statements(body)?;
                    self.loop_depth -= 1;
                }
                Stmt::Break | Stmt::Continue if self.loop_depth == 0 => {
                    return Err(self.error(
                        DiagnosticCode::InvalidBreakContinue,
                        "break or continue used outside a loop",
                    ));
                }
                Stmt::Break | Stmt::Continue => {}
            }
        }
        Ok(())
    }

    fn analyze_expression(&mut self, expression: &Expr) -> Result<Type, SimplyError> {
        match expression {
            Expr::Literal(Literal::String(_)) => Ok(Type::String),
            Expr::Literal(Literal::Int(_)) => Ok(Type::Int),
            Expr::Literal(Literal::Float(_)) => Ok(Type::Float),
            Expr::Literal(Literal::Bool(_)) => Ok(Type::Bool),
            Expr::Identifier(name) => self.variables.get(name).cloned().ok_or_else(|| {
                self.error(
                    DiagnosticCode::UndefinedVariable,
                    format!("unknown variable `{name}`"),
                )
            }),
            Expr::Array(values) => self.collection_type(values, true),
            Expr::List(values) => self.collection_type(values, false),
            Expr::Tuple(values) => Ok(Type::Tuple(
                values
                    .iter()
                    .map(|value| self.analyze_expression(value))
                    .collect::<Result<_, _>>()?,
            )),
            Expr::Matrix(values) => {
                for value in values {
                    self.analyze_expression(value)?;
                }
                Ok(Type::Matrix)
            }
            Expr::Hash(entries) | Expr::Tree(entries) => {
                for (_, value) in entries {
                    self.analyze_expression(value)?;
                }
                Ok(if matches!(expression, Expr::Hash(_)) {
                    Type::Hash
                } else {
                    Type::Tree
                })
            }
            Expr::Unary { operator, operand } => {
                let operand_type = self.analyze_expression(operand)?;
                match operator {
                    UnaryOperator::Not => {
                        self.require_type(&Type::Bool, &operand_type)?;
                        Ok(Type::Bool)
                    }
                    UnaryOperator::Negate => {
                        self.require_numeric(&operand_type)?;
                        Ok(operand_type)
                    }
                    UnaryOperator::Transpose => {
                        self.require_type(&Type::Matrix, &operand_type)?;
                        Ok(Type::Matrix)
                    }
                }
            }
            Expr::Binary {
                left,
                operator,
                right,
            } => {
                let left_type = self.analyze_expression(left)?;
                if matches!(
                    (operator, left.as_ref()),
                    (BinaryOperator::And, Expr::Literal(Literal::Bool(false)))
                        | (BinaryOperator::Or, Expr::Literal(Literal::Bool(true)))
                ) {
                    return Ok(Type::Bool);
                }
                let right_type = self.analyze_expression(right)?;
                self.binary_type(operator, &left_type, &right_type)
            }
            Expr::Call { name, arguments } => self.call_type(name, arguments),
            Expr::Index { target, index } => {
                let target_type = self.analyze_expression(target)?;
                let index_type = self.analyze_expression(index)?;
                self.index_type(&target_type, &index_type, index)
            }
            Expr::Field { target, name } => {
                let target_type = self.analyze_expression(target)?;
                match target_type {
                    Type::Hash | Type::Tree | Type::Unknown => Ok(Type::Unknown),
                    _ => Err(self.error(
                        DiagnosticCode::SemanticField,
                        format!("value has no field `{name}`"),
                    )),
                }
            }
            Expr::Pipeline { source, steps } => self.pipeline_type(source, steps),
        }
    }

    fn collection_type(&mut self, values: &[Expr], array: bool) -> Result<Type, SimplyError> {
        let mut element = Type::Unknown;
        for value in values {
            let actual = self.analyze_expression(value)?;
            if element == Type::Unknown {
                element = actual;
            } else if !actual.compatible_with(&element) {
                return Err(self.type_error(&element, &actual, "collection element"));
            }
        }
        Ok(if array {
            Type::Array(Box::new(element))
        } else {
            Type::List(Box::new(element))
        })
    }

    fn binary_type(
        &self,
        operator: &BinaryOperator,
        left: &Type,
        right: &Type,
    ) -> Result<Type, SimplyError> {
        use BinaryOperator::*;
        if left == &Type::Unknown || right == &Type::Unknown {
            return Ok(Type::Unknown);
        }
        match operator {
            Add if left == &Type::Matrix && right == &Type::Matrix => Ok(Type::Matrix),
            Add if left == &Type::String && right == &Type::String => Ok(Type::String),
            Add => self.numeric_result(left, right),
            Subtract | Multiply | Divide | Remainder => self.numeric_result(left, right),
            MatrixMultiply => {
                self.require_type(&Type::Matrix, left)?;
                self.require_type(&Type::Matrix, right)?;
                Ok(Type::Matrix)
            }
            Greater | GreaterEqual | Less | LessEqual => {
                self.require_numeric(left)?;
                self.require_numeric(right)?;
                Ok(Type::Bool)
            }
            Equal | NotEqual => Ok(Type::Bool),
            And | Or => {
                self.require_type(&Type::Bool, left)?;
                self.require_type(&Type::Bool, right)?;
                Ok(Type::Bool)
            }
        }
    }

    fn call_type(&mut self, name: &str, arguments: &[Expr]) -> Result<Type, SimplyError> {
        let argument_types = arguments
            .iter()
            .map(|argument| self.analyze_expression(argument))
            .collect::<Result<Vec<_>, _>>()?;
        match name {
            "print" => {
                self.expect_count(name, &argument_types, 1)?;
                Ok(Type::Unit)
            }
            "type_of" => {
                self.expect_count(name, &argument_types, 1)?;
                Ok(Type::String)
            }
            "length" | "count" => {
                self.expect_count(name, &argument_types, 1)?;
                self.require_collection_or_string(&argument_types[0])?;
                Ok(Type::Int)
            }
            "contains" => {
                self.expect_count(name, &argument_types, 2)?;
                self.require_collection_or_string(&argument_types[0])?;
                if matches!(argument_types[0], Type::String) {
                    self.require_type(&Type::String, &argument_types[1])?;
                }
                Ok(Type::Bool)
            }
            "range" => {
                self.expect_count(name, &argument_types, 2)?;
                for argument in &argument_types {
                    self.require_type(&Type::Int, argument)?;
                }
                Ok(Type::Array(Box::new(Type::Int)))
            }
            "send" => {
                if argument_types.len() < 2 {
                    return Err(self.error(
                        DiagnosticCode::InvalidFunctionCall,
                        "`send` expects a receiver and a message",
                    ));
                }
                self.require_type(&Type::String, &argument_types[1])?;
                Ok(Type::Unknown)
            }
            _ => {
                let function = self.functions.get(name).cloned().ok_or_else(|| {
                    self.error(
                        DiagnosticCode::InvalidFunctionCall,
                        format!("unknown function `{name}`"),
                    )
                })?;
                if function.parameters.len() != argument_types.len() {
                    return Err(self.error(
                        DiagnosticCode::InvalidFunctionCall,
                        format!(
                            "function `{name}` expects {} arguments, got {}",
                            function.parameters.len(),
                            argument_types.len()
                        ),
                    ));
                }
                for (expected, actual) in function.parameters.iter().zip(argument_types.iter()) {
                    if let Some(expected) = expected {
                        self.require_type(expected, actual)?;
                    }
                }
                Ok(function.return_type.unwrap_or(Type::Unit))
            }
        }
    }

    fn pipeline_type(
        &mut self,
        source: &Expr,
        steps: &[PipelineStep],
    ) -> Result<Type, SimplyError> {
        let source_type = self.analyze_expression(source)?;
        let mut item_type = Self::element_type(&source_type).ok_or_else(|| {
            self.error(
                DiagnosticCode::SemanticCollection,
                "pipeline source must be an array or list",
            )
        })?;
        for step in steps {
            match step {
                PipelineStep::Filter(expression) => {
                    let previous = self.variables.insert("item".into(), item_type.clone());
                    let filter_type = self.analyze_expression(expression)?;
                    self.require_type(&Type::Bool, &filter_type)?;
                    Self::restore_item(&mut self.variables, previous);
                }
                PipelineStep::Map(expression) => {
                    let previous = self.variables.insert("item".into(), item_type.clone());
                    item_type = self.analyze_expression(expression)?;
                    Self::restore_item(&mut self.variables, previous);
                }
                PipelineStep::Sum => {
                    self.require_numeric(&item_type)?;
                    return Ok(item_type);
                }
                PipelineStep::Count => return Ok(Type::Int),
            }
        }
        Ok(Type::List(Box::new(item_type)))
    }

    fn restore_item(variables: &mut ScopeStack, previous: Option<Type>) {
        match previous {
            Some(value) => {
                variables.insert("item".into(), value);
            }
            None => {
                variables.remove("item");
            }
        }
    }
    fn index_type(
        &self,
        target: &Type,
        index: &Type,
        index_expression: &Expr,
    ) -> Result<Type, SimplyError> {
        match target {
            Type::Array(element) | Type::List(element) => {
                self.require_type(&Type::Int, index)?;
                Ok((**element).clone())
            }
            Type::Tuple(types) => {
                self.require_type(&Type::Int, index)?;
                match index_expression {
                    Expr::Literal(Literal::Int(value)) if *value >= 0 => {
                        types.get(*value as usize).cloned().ok_or_else(|| {
                            self.error(
                                DiagnosticCode::SemanticTupleIndex,
                                "tuple index out of bounds",
                            )
                        })
                    }
                    Expr::Literal(Literal::Int(_)) => Err(self.error(
                        DiagnosticCode::SemanticTupleIndex,
                        "tuple index must be non-negative",
                    )),
                    _ => Ok(Type::Unknown),
                }
            }
            Type::Hash | Type::Tree => {
                self.require_type(&Type::String, index)?;
                Ok(Type::Unknown)
            }
            Type::Matrix => match index {
                Type::Tuple(types)
                    if types.len() == 2
                        && types.iter().all(|value| value.compatible_with(&Type::Int)) =>
                {
                    Ok(Type::Unknown)
                }
                Type::Unknown => Ok(Type::Unknown),
                _ => Err(self.error(
                    DiagnosticCode::SemanticMatrixIndex,
                    "matrix index requires a tuple of two integers",
                )),
            },
            Type::Unknown => Ok(Type::Unknown),
            _ => Err(self.error(DiagnosticCode::SemanticIndex, "value is not indexable")),
        }
    }
    fn element_type(typ: &Type) -> Option<Type> {
        match typ {
            Type::Array(element) | Type::List(element) => Some((**element).clone()),
            Type::Tuple(types) => Some(types.first().cloned().unwrap_or(Type::Unknown)),
            Type::Hash | Type::Tree => Some(Type::Unknown),
            Type::Unknown => Some(Type::Unknown),
            _ => None,
        }
    }

    fn require_collection_or_string(&self, typ: &Type) -> Result<(), SimplyError> {
        if matches!(
            typ,
            Type::String
                | Type::Array(_)
                | Type::List(_)
                | Type::Tuple(_)
                | Type::Hash
                | Type::Tree
                | Type::Unknown
        ) {
            Ok(())
        } else {
            Err(self.error(
                DiagnosticCode::SemanticCollection,
                format!("expected a collection or string, found {}", typ.name()),
            ))
        }
    }
    fn numeric_result(&self, left: &Type, right: &Type) -> Result<Type, SimplyError> {
        self.require_numeric(left)?;
        self.require_numeric(right)?;
        Ok(if left == &Type::Float || right == &Type::Float {
            Type::Float
        } else {
            Type::Int
        })
    }
    fn require_numeric(&self, typ: &Type) -> Result<(), SimplyError> {
        if matches!(typ, Type::Int | Type::Float | Type::Unknown) {
            Ok(())
        } else {
            Err(self.error(
                DiagnosticCode::TypeMismatch,
                format!("expected a number, found {}", typ.name()),
            ))
        }
    }
    fn require_type(&self, expected: &Type, actual: &Type) -> Result<(), SimplyError> {
        if actual.compatible_with(expected) {
            Ok(())
        } else {
            Err(self.type_error(expected, actual, "expression"))
        }
    }
    fn expect_count(
        &self,
        name: &str,
        arguments: &[Type],
        expected: usize,
    ) -> Result<(), SimplyError> {
        if arguments.len() == expected {
            Ok(())
        } else {
            Err(self.error(
                DiagnosticCode::InvalidFunctionCall,
                format!(
                    "`{name}` expects {expected} arguments, got {}",
                    arguments.len()
                ),
            ))
        }
    }
    fn type_error(&self, expected: &Type, actual: &Type, subject: &str) -> SimplyError {
        self.error(
            DiagnosticCode::TypeMismatch,
            format!(
                "type mismatch for `{subject}`: expected {}, found {}",
                expected.name(),
                actual.name()
            ),
        )
    }
    fn error(&self, code: DiagnosticCode, message: impl Into<String>) -> SimplyError {
        SimplyError::Semantic {
            span: self.current_span.clone().unwrap_or_else(|| Span::new(0, 0)),
            code,
            message: message.into(),
        }
    }
}
