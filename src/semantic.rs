//! semantic.rs — static program analysis
//! Checks declarations, scopes, types, functions, and control-flow requirements before evaluation.
//! Key components: SemanticAnalyzer, FunctionSignature, and ScopeStack.
use std::collections::{HashMap, HashSet};

use crate::{
    ast::{
        BinaryOperator, Expr, Literal, PipelineStep, Program, Stmt, UnaryOperator,
        is_parallel_safe_expression,
    },
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
    inferred_return: Option<Type>,
    saw_return: bool,
}

#[derive(Clone)]
struct ScopeStack {
    scopes: Vec<HashMap<String, Type>>,
    bindings: HashMap<String, Vec<usize>>,
    mutability: HashMap<String, Vec<bool>>,
}

impl ScopeStack {
    fn new() -> Self {
        Self {
            scopes: vec![HashMap::new()],
            bindings: HashMap::new(),
            mutability: HashMap::new(),
        }
    }

    fn insert(
        &mut self,
        name: String,
        typ: Type,
        mutable: bool,
    ) -> Result<Option<Type>, SimplyError> {
        self.insert_at(name, typ, mutable, Span::new(0, 0))
    }

    fn insert_at(
        &mut self,
        name: String,
        typ: Type,
        mutable: bool,
        span: Span,
    ) -> Result<Option<Type>, SimplyError> {
        let current = self
            .scopes
            .last_mut()
            .expect("semantic analyzer always has a global scope");
        if current.contains_key(&name) {
            return Err(SimplyError::Semantic {
                span,
                code: DiagnosticCode::DuplicateDeclaration,
                message: format!(
                    "variable `{name}` is already declared in this scope; declare it with `mut` to reassign it"
                ),
            });
        }
        let previous = current.insert(name.clone(), typ);
        self.bindings
            .entry(name.clone())
            .or_default()
            .push(self.scopes.len() - 1);
        self.mutability.entry(name).or_default().push(mutable);
        Ok(previous)
    }

    fn define(&mut self, name: String, typ: Type, mutable: bool) -> Result<(), SimplyError> {
        self.define_at(name, typ, mutable, Span::new(0, 0))
    }

    fn define_at(
        &mut self,
        name: String,
        typ: Type,
        mutable: bool,
        span: Span,
    ) -> Result<(), SimplyError> {
        if self
            .scopes
            .last()
            .map(|scope| scope.contains_key(&name))
            .unwrap_or(false)
        {
            return Err(SimplyError::Semantic {
                span,
                code: DiagnosticCode::DuplicateDeclaration,
                message: format!(
                    "variable `{name}` is already declared in this scope; declare it with `mut` to reassign it"
                ),
            });
        }
        self.scopes
            .last_mut()
            .expect("semantic analyzer always has a global scope")
            .insert(name.clone(), typ);
        self.bindings
            .entry(name.clone())
            .or_default()
            .push(self.scopes.len() - 1);
        self.mutability.entry(name).or_default().push(mutable);
        Ok(())
    }

    fn get(&self, name: &str) -> Option<&Type> {
        let scope_index = self.bindings.get(name)?.last().copied()?;
        self.scopes.get(scope_index)?.get(name)
    }

    fn is_mutable(&self, name: &str) -> bool {
        self.mutability
            .get(name)
            .and_then(|values| values.last())
            .copied()
            .unwrap_or(false)
    }

    fn remove(&mut self, name: &str) -> Option<Type> {
        let value = self
            .scopes
            .last_mut()
            .expect("semantic analyzer always has a global scope")
            .remove(name)?;
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
        Some(value)
    }

    fn push(&mut self) {
        self.scopes.push(HashMap::new());
    }

    fn truncate(&mut self, depth: usize) {
        while self.scopes.len() > depth && self.scopes.len() > 1 {
            let scope = self.scopes.pop().expect("scope exists above global frame");
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
        }
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
        self.collect_functions(&program.statements)?;
        self.analyze_statements(&program.statements)
    }

    fn define_variable(
        &mut self,
        name: String,
        typ: Type,
        mutable: bool,
    ) -> Result<(), SimplyError> {
        let span = self.current_span.clone().unwrap_or_else(|| Span::new(0, 0));
        self.variables.define_at(name, typ, mutable, span)
    }

    fn collect_functions(&mut self, statements: &[Stmt]) -> Result<(), SimplyError> {
        for statement in statements {
            let statement_span = match statement {
                Stmt::Located { span, .. } => Some(span.clone()),
                _ => None,
            };
            let previous_span = self.current_span.clone();
            if let Some(span) = statement_span {
                self.current_span = Some(span);
            }
            let statement = match statement {
                Stmt::Located { statement, .. } => statement.as_ref(),
                statement => statement,
            };
            if let Stmt::Function {
                name,
                parameters,
                return_type,
                body,
            } = statement
            {
                if self.functions.contains_key(name) {
                    return Err(self.error(
                        DiagnosticCode::DuplicateDeclaration,
                        format!("function `{name}` is already declared in this scope"),
                    ));
                }
                self.functions.insert(
                    name.clone(),
                    FunctionSignature {
                        parameters: parameters
                            .iter()
                            .map(|(_, parameter_type, _)| parameter_type.clone())
                            .collect(),
                        return_type: return_type.clone(),
                    },
                );
                self.variables.define(
                    name.clone(),
                    Type::Function {
                        parameters: parameters
                            .iter()
                            .map(|(_, parameter_type, _)| parameter_type.clone().map(Box::new))
                            .collect(),
                        return_type: return_type.clone().map(Box::new),
                    },
                    false,
                )?;
                self.collect_functions(body)?;
            }
            self.current_span = previous_span;
        }
        Ok(())
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
                    self.define_variable(alias.clone(), Type::Unknown, false)?;
                }
                Stmt::Assign {
                    name,
                    mutable,
                    declared_type,
                    value,
                } => {
                    let actual = self.analyze_expression(value)?;
                    let expected = declared_type.clone().unwrap_or_else(|| actual.clone());
                    if !actual.compatible_with(&expected) {
                        return Err(self.type_error(&expected, &actual, name));
                    }
                    self.define_variable(name.clone(), expected, *mutable)?;
                }
                Stmt::Flow {
                    name,
                    source,
                    steps,
                } => {
                    let actual = self.analyze_expression(&Expr::Pipeline {
                        source: Box::new(source.clone()),
                        steps: steps.clone(),
                    })?;
                    self.define_variable(name.clone(), actual, false)?;
                }
                Stmt::Reassign { name, value } => {
                    if !self.variables.is_mutable(name) {
                        return Err(self.error(
                            DiagnosticCode::InvalidReassignment,
                            format!(
                                "cannot reassign immutable variable `{name}`; declare it with `mut`"
                            ),
                        ));
                    }
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
                    if !self.variables.is_mutable(name) {
                        return Err(self.error(
                            DiagnosticCode::SemanticCollectionOperation,
                            format!(
                                "cannot mutate immutable variable `{name}`; declare it with `mut`"
                            ),
                        ));
                    }
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
                            for ((name, mutable), value_type) in names.iter().zip(types) {
                                self.define_variable(name.clone(), value_type, *mutable)?;
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
                    if !self.variables.is_mutable(name) {
                        return Err(self.error(
                            DiagnosticCode::SemanticCollectionOperation,
                            format!(
                                "cannot mutate immutable variable `{name}`; declare it with `mut`"
                            ),
                        ));
                    }
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
                    let saved_inferred_return = self.inferred_return.clone();
                    let saved_saw_return = self.saw_return;
                    let frame_start = self.variables.scopes.len();
                    self.variables.push();
                    self.function_return = return_type.clone();
                    self.inferred_return = None;
                    self.function_depth += 1;
                    self.saw_return = false;
                    for (parameter, parameter_type, mutable) in parameters {
                        self.define_variable(
                            parameter.clone(),
                            parameter_type.clone().unwrap_or(Type::Unknown),
                            *mutable,
                        )?;
                    }
                    if let Err(error) = self.analyze_statements(body) {
                        self.variables.truncate(frame_start);
                        self.function_depth = saved_function_depth;
                        self.function_return = saved_return;
                        self.inferred_return = saved_inferred_return;
                        self.saw_return = saved_saw_return;
                        return Err(error);
                    }
                    if let Some(expected) = return_type
                        && !self.saw_return
                        && *expected != Type::Unknown
                    {
                        self.variables.truncate(frame_start);
                        self.function_depth = saved_function_depth;
                        self.function_return = saved_return;
                        self.inferred_return = saved_inferred_return;
                        self.saw_return = saved_saw_return;
                        return Err(self.error(
                            DiagnosticCode::SemanticMissingReturn,
                            format!("function `{name}` must return {}", expected.name()),
                        ));
                    }
                    self.variables.truncate(frame_start);
                    self.function_depth = saved_function_depth;
                    self.function_return = saved_return;
                    if return_type.is_none()
                        && let Some(signature) = self.functions.get_mut(name)
                    {
                        signature.return_type = self.inferred_return.clone();
                    }
                    self.inferred_return = saved_inferred_return;
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
                    if let Some(previous) = self.inferred_return.clone() {
                        self.require_type(&previous, &actual)?;
                    } else {
                        self.inferred_return = Some(actual);
                    }
                    self.saw_return = true;
                }
                Stmt::Throw(expression) => {
                    self.analyze_expression(expression)?;
                }
                Stmt::If {
                    condition,
                    then_branch,
                    else_branch,
                } => {
                    let condition_type = self.analyze_expression(condition)?;
                    self.require_type(&Type::Bool, &condition_type)?;
                    let saved_saw_return = self.saw_return;
                    let frame_start = self.variables.scopes.len();
                    self.variables.push();
                    self.saw_return = false;
                    if let Err(error) = self.analyze_statements(then_branch) {
                        self.variables.truncate(frame_start);
                        self.saw_return = saved_saw_return;
                        return Err(error);
                    }
                    let then_returns = self.saw_return;
                    self.variables.truncate(frame_start);
                    let frame_start = self.variables.scopes.len();
                    self.variables.push();
                    self.saw_return = false;
                    if let Err(error) = self.analyze_statements(else_branch) {
                        self.variables.truncate(frame_start);
                        self.saw_return = saved_saw_return;
                        return Err(error);
                    }
                    let else_returns = self.saw_return;
                    self.variables.truncate(frame_start);
                    self.saw_return = saved_saw_return || (then_returns && else_returns);
                }
                Stmt::Try {
                    try_body,
                    catches,
                    finally_body,
                } => {
                    let saved_saw_return = self.saw_return;
                    let try_returns = self.analyze_scoped_block(try_body)?;
                    let mut catch_returns = false;
                    for catch in catches {
                        let frame_start = self.variables.scopes.len();
                        self.variables.push();
                        if let Some(name) = &catch.binding {
                            self.define_variable(name.clone(), Type::Tree, false)?;
                        }
                        self.saw_return = false;
                        let result = self.analyze_statements(&catch.body);
                        catch_returns |= self.saw_return;
                        self.variables.truncate(frame_start);
                        result?;
                    }
                    let finally_returns = self.analyze_scoped_block(finally_body)?;
                    self.saw_return = saved_saw_return
                        || finally_returns
                        || (try_returns && !catches.is_empty() && catch_returns);
                }
                Stmt::For {
                    name,
                    mutable,
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
                    let frame_start = self.variables.scopes.len();
                    let saved_saw_return = self.saw_return;
                    self.variables.push();
                    let span = self.current_span.clone().unwrap_or_else(|| Span::new(0, 0));
                    self.variables
                        .insert_at(name.clone(), element, *mutable, span)?;
                    self.loop_depth += 1;
                    if let Err(error) = self.analyze_statements(body) {
                        self.loop_depth -= 1;
                        self.variables.truncate(frame_start);
                        self.saw_return = saved_saw_return;
                        return Err(error);
                    }
                    self.loop_depth -= 1;
                    self.variables.truncate(frame_start);
                    self.saw_return = saved_saw_return;
                }
                Stmt::While { condition, body } => {
                    let condition_type = self.analyze_expression(condition)?;
                    self.require_type(&Type::Bool, &condition_type)?;
                    let saved_saw_return = self.saw_return;
                    self.loop_depth += 1;
                    let result = self.analyze_statements(body);
                    self.loop_depth -= 1;
                    self.saw_return = saved_saw_return;
                    result?;
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

    fn analyze_scoped_block(&mut self, statements: &[Stmt]) -> Result<bool, SimplyError> {
        if statements.is_empty() {
            return Ok(false);
        }
        let frame_start = self.variables.scopes.len();
        let saved_saw_return = self.saw_return;
        self.variables.push();
        self.saw_return = false;
        let result = self.analyze_statements(statements);
        let returns = self.saw_return;
        self.variables.truncate(frame_start);
        self.saw_return = saved_saw_return;
        result.map(|()| returns)
    }

    fn analyze_expression(&mut self, expression: &Expr) -> Result<Type, SimplyError> {
        match expression {
            Expr::Literal(Literal::String(_)) => Ok(Type::String),
            Expr::Literal(Literal::Int(_)) => Ok(Type::Int),
            Expr::Literal(Literal::Float(_)) => Ok(Type::Float),
            Expr::Literal(Literal::Bool(_)) => Ok(Type::Bool),
            Expr::Identifier(name) => {
                if let Some(typ) = self.variables.get(name).cloned() {
                    Ok(typ)
                } else if let Some(function) = self.functions.get(name) {
                    Ok(Type::Function {
                        parameters: function
                            .parameters
                            .iter()
                            .map(|parameter| parameter.clone().map(Box::new))
                            .collect(),
                        return_type: function.return_type.clone().map(Box::new),
                    })
                } else {
                    Err(self.error(
                        DiagnosticCode::UndefinedVariable,
                        format!("unknown variable `{name}`"),
                    ))
                }
            }
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
            "any" | "all" => {
                self.expect_count(name, &argument_types, 1)?;
                self.require_boolean_iterable(&argument_types[0])?;
                Ok(Type::Bool)
            }
            "join" => {
                self.expect_count(name, &argument_types, 2)?;
                self.require_string_iterable(&argument_types[0])?;
                self.require_type(&Type::String, &argument_types[1])?;
                Ok(Type::String)
            }
            "total" => {
                self.expect_count(name, &argument_types, 1)?;
                self.require_numeric_iterable(&argument_types[0])?;
                Ok(Type::Unknown)
            }
            "trim" => {
                self.expect_count(name, &argument_types, 1)?;
                self.require_type(&Type::String, &argument_types[0])?;
                Ok(Type::String)
            }
            "read_file" => {
                self.expect_count(name, &argument_types, 1)?;
                self.require_type(&Type::String, &argument_types[0])?;
                Ok(Type::String)
            }
            "write_file" => {
                self.expect_count(name, &argument_types, 2)?;
                self.require_type(&Type::String, &argument_types[0])?;
                self.require_type(&Type::String, &argument_types[1])?;
                Ok(Type::Unit)
            }
            "substring" => {
                self.expect_count(name, &argument_types, 3)?;
                self.require_type(&Type::String, &argument_types[0])?;
                self.require_type(&Type::Int, &argument_types[1])?;
                self.require_type(&Type::Int, &argument_types[2])?;
                Ok(Type::String)
            }
            "characters" => {
                self.expect_count(name, &argument_types, 1)?;
                self.require_type(&Type::String, &argument_types[0])?;
                Ok(Type::Array(Box::new(Type::String)))
            }
            "is_ascii_alpha" | "is_ascii_digit" | "is_whitespace" => {
                self.expect_count(name, &argument_types, 1)?;
                self.require_type(&Type::String, &argument_types[0])?;
                Ok(Type::Bool)
            }
            "to_float" => {
                self.expect_count(name, &argument_types, 1)?;
                self.require_type(&Type::String, &argument_types[0])?;
                if let Expr::Literal(Literal::String(value)) = &arguments[0]
                    && !matches!(
                        value.trim().parse::<f64>(),
                        Ok(number) if number.is_finite()
                    )
                {
                    return Err(self.error(
                        DiagnosticCode::InvalidNumericLiteral,
                        format!("cannot convert `{value}` to a finite Float"),
                    ));
                }
                Ok(Type::Float)
            }
            "to_int" => {
                self.expect_count(name, &argument_types, 1)?;
                self.require_type(&Type::String, &argument_types[0])?;
                if let Expr::Literal(Literal::String(value)) = &arguments[0]
                    && value.trim().parse::<i64>().is_err()
                {
                    return Err(self.error(
                        DiagnosticCode::InvalidNumericLiteral,
                        format!("cannot convert `{value}` to an Int"),
                    ));
                }
                Ok(Type::Int)
            }
            "abs" | "round" => {
                self.expect_count(name, &argument_types, if name == "round" { 2 } else { 1 })?;
                self.require_numeric(&argument_types[0])?;
                if name == "round" {
                    self.require_type(&Type::Int, &argument_types[1])?;
                    if let Expr::Literal(Literal::Int(decimals)) = arguments[1]
                        && !(0..=15).contains(&decimals)
                    {
                        return Err(self.error(
                            DiagnosticCode::InvalidFunctionCall,
                            "`round` decimal count must be an integer from 0 to 15",
                        ));
                    }
                }
                Ok(argument_types[0].clone())
            }
            "sqrt" | "exp" | "log" | "log10" | "sin" | "cos" | "tan" | "floor" | "ceil" => {
                self.expect_count(name, &argument_types, 1)?;
                self.require_numeric(&argument_types[0])?;
                Ok(Type::Float)
            }
            "sign" => {
                self.expect_count(name, &argument_types, 1)?;
                self.require_numeric(&argument_types[0])?;
                Ok(Type::Int)
            }
            "pow" => {
                self.expect_count(name, &argument_types, 2)?;
                self.require_numeric(&argument_types[0])?;
                self.require_numeric(&argument_types[1])?;
                Ok(Type::Float)
            }
            "vector_add" | "vector_subtract" => {
                self.expect_count(name, &argument_types, 2)?;
                let left = self.numeric_sequence_element(&argument_types[0])?;
                let right = self.numeric_sequence_element(&argument_types[1])?;
                let element = self.numeric_result(&left, &right)?;
                Ok(Type::Array(Box::new(element)))
            }
            "vector_scale" => {
                self.expect_count(name, &argument_types, 2)?;
                self.require_numeric(&argument_types[1])?;
                let element = self.numeric_sequence_element(&argument_types[0])?;
                Ok(Type::Array(Box::new(
                    self.numeric_result(&element, &argument_types[1])?,
                )))
            }
            "dot" => {
                self.expect_count(name, &argument_types, 2)?;
                let left = self.numeric_sequence_element(&argument_types[0])?;
                let right = self.numeric_sequence_element(&argument_types[1])?;
                self.numeric_result(&left, &right)
            }
            "norm" => {
                self.expect_count(name, &argument_types, 1)?;
                self.numeric_sequence_element(&argument_types[0])?;
                Ok(Type::Float)
            }
            "distance" => {
                self.expect_count(name, &argument_types, 2)?;
                self.numeric_sequence_element(&argument_types[0])?;
                self.numeric_sequence_element(&argument_types[1])?;
                Ok(Type::Float)
            }
            "normalize" => {
                self.expect_count(name, &argument_types, 1)?;
                self.numeric_sequence_element(&argument_types[0])?;
                Ok(Type::Array(Box::new(Type::Float)))
            }
            "shape" => {
                self.expect_count(name, &argument_types, 1)?;
                self.require_matrix(&argument_types[0])?;
                Ok(Type::Tuple(vec![Type::Int, Type::Int]))
            }
            "transpose" => {
                self.expect_count(name, &argument_types, 1)?;
                self.require_matrix(&argument_types[0])?;
                Ok(Type::Matrix)
            }
            "matrix_add" | "matrix_subtract" | "multiply" => {
                self.expect_count(name, &argument_types, 2)?;
                self.require_matrix(&argument_types[0])?;
                self.require_matrix(&argument_types[1])?;
                Ok(Type::Matrix)
            }
            "matrix_scale" => {
                self.expect_count(name, &argument_types, 2)?;
                self.require_matrix(&argument_types[0])?;
                self.require_numeric(&argument_types[1])?;
                Ok(Type::Matrix)
            }
            "identity" => {
                self.expect_count(name, &argument_types, 1)?;
                self.require_type(&Type::Int, &argument_types[0])?;
                Ok(Type::Matrix)
            }
            "mean" | "median" | "variance" | "stddev" => {
                self.expect_count(name, &argument_types, 1)?;
                self.require_numeric_iterable(&argument_types[0])?;
                Ok(Type::Float)
            }
            "percentile" => {
                self.expect_count(name, &argument_types, 2)?;
                self.require_numeric_iterable(&argument_types[0])?;
                self.require_numeric(&argument_types[1])?;
                Ok(Type::Float)
            }
            "covariance" | "correlation" => {
                self.expect_count(name, &argument_types, 2)?;
                self.require_numeric_iterable(&argument_types[0])?;
                self.require_numeric_iterable(&argument_types[1])?;
                Ok(Type::Float)
            }
            "clamp" => {
                self.expect_count(name, &argument_types, 3)?;
                for argument in &argument_types {
                    self.require_numeric(argument)?;
                }
                if argument_types.contains(&Type::Unknown) {
                    Ok(Type::Unknown)
                } else if argument_types.contains(&Type::Float) {
                    Ok(Type::Float)
                } else {
                    Ok(Type::Int)
                }
            }
            "split" => {
                self.expect_count(name, &argument_types, 2)?;
                self.require_type(&Type::String, &argument_types[0])?;
                self.require_type(&Type::String, &argument_types[1])?;
                Ok(Type::List(Box::new(Type::String)))
            }
            "csv_rows" => {
                self.expect_count(name, &argument_types, 1)?;
                self.require_type(&Type::String, &argument_types[0])?;
                Ok(Type::List(Box::new(Type::List(Box::new(Type::String)))))
            }
            "csv_row" => Ok(Type::List(Box::new(Type::Unknown))),
            "replace" => {
                self.expect_count(name, &argument_types, 3)?;
                for argument in &argument_types {
                    self.require_type(&Type::String, argument)?;
                }
                Ok(Type::String)
            }
            "starts_with" | "ends_with" => {
                self.expect_count(name, &argument_types, 2)?;
                for argument in &argument_types {
                    self.require_type(&Type::String, argument)?;
                }
                Ok(Type::Bool)
            }
            "is_empty" => {
                self.expect_count(name, &argument_types, 1)?;
                self.require_collection_or_string(&argument_types[0])?;
                Ok(Type::Bool)
            }
            "reverse" => {
                self.expect_count(name, &argument_types, 1)?;
                if !matches!(
                    argument_types[0],
                    Type::Array(_) | Type::List(_) | Type::Tuple(_) | Type::Unknown
                ) {
                    return Err(self.error(
                        DiagnosticCode::SemanticCollection,
                        format!(
                            "`reverse` requires a sequence, found {}",
                            argument_types[0].name()
                        ),
                    ));
                }
                Ok(argument_types[0].clone())
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
                let message = match &arguments[1] {
                    Expr::Literal(Literal::String(message)) => Some(message.as_str()),
                    _ => None,
                };
                let Some(message) = message else {
                    return Ok(Type::Unknown);
                };
                let function = self.functions.get(message).cloned().ok_or_else(|| {
                    self.error(
                        DiagnosticCode::InvalidFunctionCall,
                        format!("unknown message `{message}`"),
                    )
                })?;
                if function.parameters.len() != argument_types.len() - 1 {
                    return Err(self.error(
                        DiagnosticCode::InvalidFunctionCall,
                        format!(
                            "message `{message}` expects {} arguments, got {}",
                            function.parameters.len(),
                            argument_types.len() - 1
                        ),
                    ));
                }
                for (expected, actual) in function.parameters.iter().zip(
                    argument_types
                        .iter()
                        .take(1)
                        .chain(argument_types.iter().skip(2)),
                ) {
                    if let Some(expected) = expected {
                        self.require_type(expected, actual)?;
                    }
                }
                Ok(function.return_type.unwrap_or(Type::Unit))
            }
            _ => {
                let Some(function) = self.functions.get(name).cloned() else {
                    if let Some(Type::Function { return_type, .. }) = self.variables.get(name) {
                        return Ok(return_type.as_deref().cloned().unwrap_or(Type::Unknown));
                    }
                    if matches!(self.variables.get(name), Some(&Type::Unknown)) {
                        return Ok(Type::Unknown);
                    }
                    return Err(self.error(
                        DiagnosticCode::InvalidFunctionCall,
                        format!("unknown function `{name}`"),
                    ));
                };
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
        let has_chunk = steps
            .iter()
            .any(|step| matches!(step, PipelineStep::Chunk(_)));
        let has_parallel = steps
            .iter()
            .any(|step| matches!(step, PipelineStep::Parallel(_)));
        let has_checkpoint = steps
            .iter()
            .any(|step| matches!(step, PipelineStep::Checkpoint(_)));
        let writes_csv = matches!(steps.last(), Some(PipelineStep::WriteCsv(_)));
        let reads_csv_rows = matches!(source, Expr::Call { name, .. } if name == "csv_rows");
        if has_parallel && has_checkpoint {
            return Err(self.error(
                DiagnosticCode::SemanticCollection,
                "`parallel` cannot be combined with `checkpoint`",
            ));
        }
        if has_chunk && !has_parallel && !has_checkpoint {
            return Err(self.error(
                DiagnosticCode::SemanticCollection,
                "`chunk` requires `parallel` or `checkpoint` in the same Flow",
            ));
        }
        if has_checkpoint && !writes_csv {
            return Err(self.error(
                DiagnosticCode::SemanticCollection,
                "`checkpoint` requires a `write_csv` terminal",
            ));
        }
        if has_checkpoint && !matches!(source, Expr::Call { name, .. } if name == "csv_rows") {
            return Err(self.error(
                DiagnosticCode::SemanticCollection,
                "`checkpoint` currently requires a `csv_rows` source",
            ));
        }
        if reads_csv_rows
            && !matches!(
                steps.last(),
                Some(
                    PipelineStep::Sum
                        | PipelineStep::Count
                        | PipelineStep::Average
                        | PipelineStep::Min
                        | PipelineStep::Max
                        | PipelineStep::Partition { .. }
                        | PipelineStep::WriteCsv(_)
                )
            )
        {
            return Err(self.error(
                DiagnosticCode::SemanticCollection,
                "`csv_rows` pipelines must end with an aggregate, `partition`, or `write_csv` terminal",
            ));
        }
        let source_type = self.analyze_expression(source)?;
        let mut item_type = match source_type {
            Type::Array(element) | Type::List(element) => *element,
            Type::Unknown => Type::Unknown,
            _ => {
                return Err(self.error(
                    DiagnosticCode::SemanticCollection,
                    "pipeline source must be an array or list",
                ));
            }
        };
        if has_parallel
            && !matches!(
                item_type,
                Type::String | Type::Int | Type::Float | Type::Bool
            )
        {
            return Err(self.error(
                DiagnosticCode::SemanticCollection,
                "`parallel` requires a scalar source item type",
            ));
        }
        if has_parallel
            && !matches!(
                steps.last(),
                Some(
                    PipelineStep::Sum
                        | PipelineStep::Count
                        | PipelineStep::Average
                        | PipelineStep::Min
                        | PipelineStep::Max
                )
            )
        {
            return Err(self.error(
                DiagnosticCode::SemanticCollection,
                "`parallel` requires an aggregate terminal",
            ));
        }
        for step in steps {
            match step {
                PipelineStep::Where(expression) => {
                    if has_parallel && !is_parallel_safe_expression(expression) {
                        return Err(self.error(
                            DiagnosticCode::SemanticCollection,
                            "`parallel` requires a parallel-safe `where` expression",
                        ));
                    }
                    let previous =
                        self.variables
                            .insert("item".into(), item_type.clone(), false)?;
                    let condition_type = self.analyze_expression(expression)?;
                    self.require_type(&Type::Bool, &condition_type)?;
                    Self::restore_item(&mut self.variables, "item", previous);
                }
                PipelineStep::Derive(expression) => {
                    if has_parallel && !is_parallel_safe_expression(expression) {
                        return Err(self.error(
                            DiagnosticCode::SemanticCollection,
                            "`parallel` requires a parallel-safe `derive` expression",
                        ));
                    }
                    let previous =
                        self.variables
                            .insert("item".into(), item_type.clone(), false)?;
                    item_type = self.analyze_expression(expression)?;
                    Self::restore_item(&mut self.variables, "item", previous);
                }
                PipelineStep::Partition { item, rules } => {
                    if has_parallel {
                        return Err(self.error(
                            DiagnosticCode::SemanticCollection,
                            "`parallel` does not support `partition`",
                        ));
                    }
                    let mut categories = HashSet::new();
                    for rule in rules {
                        categories.insert(rule.category.as_str());
                        if let Some(condition) = &rule.condition {
                            let previous =
                                self.variables
                                    .insert(item.clone(), item_type.clone(), false)?;
                            let condition_type = self.analyze_expression(condition)?;
                            self.require_type(&Type::Bool, &condition_type)?;
                            Self::restore_item(&mut self.variables, item, previous);
                        }
                    }
                    if categories.len() < 2 {
                        return Err(self.error(
                            DiagnosticCode::SemanticCollection,
                            "partition must define at least two distinct categories",
                        ));
                    }
                    return Ok(Type::Hash);
                }
                PipelineStep::Sum => {
                    self.require_numeric(&item_type)?;
                    return Ok(item_type);
                }
                PipelineStep::Count => return Ok(Type::Int),
                PipelineStep::Average => {
                    self.require_numeric(&item_type)?;
                    return Ok(Type::Float);
                }
                PipelineStep::Min | PipelineStep::Max => {
                    self.require_numeric(&item_type)?;
                    return Ok(item_type);
                }
                PipelineStep::WriteCsv(path) => {
                    if has_parallel {
                        return Err(self.error(
                            DiagnosticCode::SemanticCollection,
                            "`parallel` does not support `write_csv`",
                        ));
                    }
                    let path_type = self.analyze_expression(path)?;
                    self.require_type(&Type::String, &path_type)?;
                    let invalid_row_field = match &item_type {
                        Type::Array(fields) | Type::List(fields) => !matches!(
                            fields.as_ref(),
                            Type::String
                                | Type::Int
                                | Type::Float
                                | Type::Bool
                                | Type::Unit
                                | Type::Unknown
                        ),
                        Type::Tuple(fields) => fields.iter().any(|field| {
                            !matches!(
                                field,
                                Type::String
                                    | Type::Int
                                    | Type::Float
                                    | Type::Bool
                                    | Type::Unit
                                    | Type::Unknown
                            )
                        }),
                        Type::Unknown => false,
                        _ => {
                            return Err(self.error(
                                DiagnosticCode::SemanticCollection,
                                format!(
                                    "`write_csv` requires `derive` to produce a row collection, found {}",
                                    item_type.name()
                                ),
                            ));
                        }
                    };
                    if invalid_row_field {
                        return Err(self.error(
                            DiagnosticCode::SemanticCollection,
                            "`write_csv` row fields must be scalar values",
                        ));
                    }
                    if !reads_csv_rows {
                        return Err(self.error(
                            DiagnosticCode::SemanticCollection,
                            "`write_csv` requires a `csv_rows` source",
                        ));
                    }
                    return Ok(Type::Unit);
                }
                PipelineStep::Chunk(size) => {
                    if *size <= 0 {
                        return Err(self.error(
                            DiagnosticCode::SemanticCollection,
                            "chunk size must be a positive integer",
                        ));
                    }
                }
                PipelineStep::Parallel(workers) => {
                    if *workers <= 0 {
                        return Err(self.error(
                            DiagnosticCode::SemanticCollection,
                            "parallel worker count must be a positive integer",
                        ));
                    }
                }
                PipelineStep::Checkpoint(path) => {
                    let path_type = self.analyze_expression(path)?;
                    self.require_type(&Type::String, &path_type)?;
                }
            }
        }
        Ok(Type::List(Box::new(item_type)))
    }

    fn restore_item(variables: &mut ScopeStack, name: &str, previous: Option<Type>) {
        match previous {
            Some(value) => {
                variables.insert(name.into(), value, false).expect(
                    "restoring previous pipeline item should not collide with an in-scope binding",
                );
            }
            None => {
                variables.remove(name);
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
            Type::String => {
                self.require_type(&Type::Int, index)?;
                Ok(Type::String)
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

    fn require_string_iterable(&self, typ: &Type) -> Result<(), SimplyError> {
        match typ {
            Type::Array(element) | Type::List(element) => self.require_type(&Type::String, element),
            Type::Tuple(types) => {
                for element in types {
                    self.require_type(&Type::String, element)?;
                }
                Ok(())
            }
            Type::Unknown => Ok(()),
            _ => Err(self.error(
                DiagnosticCode::SemanticCollection,
                format!("expected a string collection, found {}", typ.name()),
            )),
        }
    }

    fn require_boolean_iterable(&self, typ: &Type) -> Result<(), SimplyError> {
        match typ {
            Type::Array(element) | Type::List(element) => self.require_type(&Type::Bool, element),
            Type::Tuple(types) => {
                for element in types {
                    self.require_type(&Type::Bool, element)?;
                }
                Ok(())
            }
            Type::Hash | Type::Tree | Type::Unknown => Ok(()),
            _ => Err(self.error(
                DiagnosticCode::SemanticCollection,
                format!("expected a boolean collection, found {}", typ.name()),
            )),
        }
    }

    fn require_numeric_iterable(&self, typ: &Type) -> Result<(), SimplyError> {
        match typ {
            Type::Array(element) | Type::List(element) => self.require_numeric(element),
            Type::Tuple(types) => {
                for element in types {
                    self.require_numeric(element)?;
                }

                Ok(())
            }
            Type::Unknown => Ok(()),
            _ => Err(self.error(
                DiagnosticCode::SemanticCollection,
                format!("expected a numeric collection, found {}", typ.name()),
            )),
        }
    }

    fn numeric_sequence_element(&self, typ: &Type) -> Result<Type, SimplyError> {
        let element = self.numeric_sequence_element_option(typ).ok_or_else(|| {
            self.error(
                DiagnosticCode::SemanticCollection,
                format!("expected a numeric sequence, found {}", typ.name()),
            )
        })?;
        self.require_numeric(&element)?;
        Ok(element)
    }

    fn numeric_sequence_element_option(&self, typ: &Type) -> Option<Type> {
        match typ {
            Type::Array(element) | Type::List(element) => Some((**element).clone()),
            Type::Tuple(elements) => {
                let first = elements.first().cloned().unwrap_or(Type::Unknown);
                if elements
                    .iter()
                    .all(|element| element.compatible_with(&first))
                {
                    Some(first)
                } else {
                    Some(Type::Unknown)
                }
            }
            Type::Unknown => Some(Type::Unknown),
            _ => None,
        }
    }

    fn require_matrix(&self, typ: &Type) -> Result<(), SimplyError> {
        let row_type = match typ {
            Type::Matrix => return Ok(()),
            Type::Array(row) | Type::List(row) => row,
            Type::Unknown => return Ok(()),
            _ => {
                return Err(self.error(
                    DiagnosticCode::SemanticCollection,
                    format!("expected a matrix, found {}", typ.name()),
                ));
            }
        };
        match row_type.as_ref() {
            Type::Array(element) | Type::List(element) => self.require_numeric(element),
            Type::Unknown => Ok(()),
            _ => Err(self.error(
                DiagnosticCode::SemanticCollection,
                "matrix rows must be numeric sequences",
            )),
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
