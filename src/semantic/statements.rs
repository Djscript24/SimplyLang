use super::*;

impl SemanticAnalyzer {
    pub(super) fn analyze_statements(&mut self, statements: &[Stmt]) -> Result<(), SimplyError> {
        for (index, statement) in statements.iter().enumerate() {
            let following: HashSet<String> = statements[index + 1..]
                .iter()
                .filter_map(|statement| match statement {
                    Stmt::Located { statement, .. } => match statement.as_ref() {
                        Stmt::Function { name, .. } => Some(name.clone()),
                        _ => None,
                    },
                    Stmt::Function { name, .. } => Some(name.clone()),
                    _ => None,
                })
                .collect();
            let parent_scope = self.function_scopes.len().saturating_sub(1);
            let ordered = if self.function_scopes.len() > 1 || self.module_return_allowed {
                following
                    .iter()
                    .map(|name| (parent_scope, name.clone()))
                    .collect()
            } else {
                HashSet::new()
            };
            self.ordered_function_references.push(ordered);
            self.following_functions.push(following);
            let result = self.analyze_statement(statement);
            self.following_functions.pop();
            self.ordered_function_references.pop();
            result?;
        }
        Ok(())
    }

    pub(super) fn analyze_statement(&mut self, statement: &Stmt) -> Result<(), SimplyError> {
        match statement {
            Stmt::Located { span, statement } => {
                self.current_span = Some(span.clone());
                self.analyze_statement(statement)?;
            }
            Stmt::Say(expression) | Stmt::Sayln(expression) | Stmt::Expression(expression) => {
                self.analyze_expression(expression)?;
            }
            Stmt::Import {
                alias, exposing, ..
            } => {
                if let Some(alias) = alias {
                    self.define_variable(
                        alias.clone(),
                        self.imported_types
                            .get(alias)
                            .cloned()
                            .unwrap_or(Type::Unknown),
                        false,
                    )?;
                } else {
                    for (exported, local) in exposing {
                        let Some(typ) = self.imported_types.get(local).cloned() else {
                            if self.struct_identities.contains_key(local)
                                || self.enum_identities.contains_key(local)
                            {
                                continue;
                            }
                            return Err(self.error(
                                DiagnosticCode::UndefinedVariable,
                                format!("module does not export `{exported}`"),
                            ));
                        };
                        self.define_variable(local.clone(), typ, false)?;
                    }
                }
            }
            Stmt::Export { names } => {
                if !self.module_return_allowed
                    || self.function_depth > 0
                    || self.variables.scopes.len() != 1
                {
                    return Err(self.error(
                        DiagnosticCode::InvalidReturn,
                        "`export` is only allowed at the top level of an imported module",
                    ));
                }
                if let Some(name) = names
                    .iter()
                    .find(|name| !self.module_export_names.contains(*name))
                {
                    return Err(self.error(
                        DiagnosticCode::UndefinedVariable,
                        format!("invalid exported name `{name}`"),
                    ));
                }
            }
            Stmt::Struct { .. } => {}
            Stmt::Enum { .. } => {}
            Stmt::Message {
                receiver_type,
                name,
                parameters,
                body,
            } => {
                let fields = self.structs[receiver_type].clone();
                let saved_function_depth = self.function_depth;
                let saved_return = self.function_return.clone();
                let saved_inferred_return = self.inferred_return.clone();
                let saved_saw_return = self.saw_return;
                let saved_loop_depth = self.loop_depth;
                let frame_start = self.variables.scopes.len();
                self.variables.push();
                self.function_scopes.push(HashMap::new());
                self.function_return = None;
                self.inferred_return = None;
                self.function_depth += 1;
                self.loop_depth = 0;
                self.saw_return = false;
                for field in &fields {
                    self.define_variable(field.name.clone(), field.field_type.clone(), true)?;
                }
                for (parameter, parameter_type, mutable) in parameters {
                    self.define_variable(
                        parameter.clone(),
                        parameter_type
                            .as_ref()
                            .map(|typ| self.resolve_type_identity(typ))
                            .unwrap_or(Type::Unknown),
                        *mutable,
                    )?;
                }
                let body_result = self
                    .collect_functions(body)
                    .and_then(|()| self.analyze_statements(body));
                let inferred_return = self.inferred_return.clone();
                self.variables.truncate(frame_start);
                self.function_scopes.pop();
                self.function_depth = saved_function_depth;
                self.loop_depth = saved_loop_depth;
                self.function_return = saved_return;
                self.inferred_return = saved_inferred_return;
                self.saw_return = saved_saw_return;
                body_result?;
                let key = (self.struct_identities[receiver_type].clone(), name.clone());
                if let Some(signature) = self.messages.get_mut(&key) {
                    signature.return_type = inferred_return;
                }
            }
            Stmt::Assign {
                name,
                mutable,
                declared_type,
                value,
            } => {
                let actual = self.analyze_expression(value)?;
                let expected = declared_type
                    .as_ref()
                    .map(|typ| self.resolve_type_identity(typ))
                    .unwrap_or_else(|| {
                        if *mutable {
                            Self::erase_inferred_dimensions(actual.clone())
                        } else {
                            actual.clone()
                        }
                    });
                if matches!(expected, Type::Matrix | Type::TypedMatrix(_, _, _)) {
                    self.require_rectangular_matrix_literal(value)?;
                }
                self.require_type(&expected, &actual)?;
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
                if self.is_snapshot_capture(name) || !self.variables.is_mutable(name) {
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
                self.require_type(&expected, &actual)?;
            }
            Stmt::DestructureReassign { pattern, value } => {
                let actual = self.analyze_expression(value)?;
                let mut targets = Vec::new();
                self.validate_destructure_assignment_pattern(pattern, &actual, &mut targets)?;
            }
            Stmt::SetIndex { name, index, value } => {
                if self.is_snapshot_capture(name) || !self.variables.is_mutable(name) {
                    return Err(self.error(
                        DiagnosticCode::SemanticCollectionOperation,
                        format!("cannot mutate immutable variable `{name}`; declare it with `mut`"),
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
                    Type::Array(element) | Type::List(element) | Type::Vector(element, _) => {
                        self.require_type(&Type::Int, &index_type)?;
                        self.require_type(&element, &value_type)?;
                    }
                    Type::Hash => self.require_type(&Type::String, &index_type)?,
                    Type::HashValues(element) => {
                        self.require_type(&Type::String, &index_type)?;
                        self.require_type(&element, &value_type)?;
                    }
                    _ => {
                        return Err(self.error(
                            DiagnosticCode::SemanticCollectionOperation,
                            format!("`{name}` is not mutable"),
                        ));
                    }
                }
            }
            Stmt::Destructure {
                pattern,
                mutable,
                value,
            } => {
                let value_type = self.analyze_expression(value)?;
                let mut bindings = Vec::new();
                self.validate_destructure_pattern(pattern, &value_type, &mut bindings)?;

                let mut names = HashSet::new();
                for (name, _) in &bindings {
                    if !names.insert(name.as_str())
                        || self
                            .variables
                            .scopes
                            .last()
                            .is_some_and(|scope| scope.contains_key(name))
                    {
                        return Err(self.error(
                            DiagnosticCode::DuplicateDeclaration,
                            format!(
                                "variable `{name}` is already declared or bound more than once"
                            ),
                        ));
                    }
                }

                for (name, typ) in bindings {
                    self.define_variable(name, typ, *mutable)?;
                }
            }
            Stmt::CollectionOp {
                name,
                operation: _,
                value,
            } => {
                if self.is_snapshot_capture(name) || !self.variables.is_mutable(name) {
                    return Err(self.error(
                        DiagnosticCode::SemanticCollectionOperation,
                        format!("cannot mutate immutable variable `{name}`; declare it with `mut`"),
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
                    Type::List(element) if *element == Type::Unknown => {
                        self.variables
                            .replace_visible(name, Type::List(Box::new(value_type)));
                    }
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
                let saved_loop_depth = self.loop_depth;
                let captures_outer = self.function_scopes.len() > 1 || self.module_return_allowed;
                let parent_function_scope = self.function_scopes.len().saturating_sub(1);
                let frame_start = self.variables.scopes.len();
                self.variables.push();
                self.function_scopes.push(HashMap::new());
                self.function_capture_frames
                    .push((frame_start, captures_outer));
                self.function_return = return_type
                    .as_ref()
                    .map(|typ| self.resolve_type_identity(typ));
                self.inferred_return = None;
                self.function_depth += 1;
                self.loop_depth = 0;
                self.saw_return = false;
                let blocked = if captures_outer {
                    self.following_functions
                        .last()
                        .into_iter()
                        .flatten()
                        .map(|name| (parent_function_scope, name.clone()))
                        .collect()
                } else {
                    HashSet::new()
                };
                self.blocked_function_references.push(blocked);
                for (parameter, parameter_type, mutable) in parameters {
                    self.define_variable(
                        parameter.clone(),
                        parameter_type
                            .as_ref()
                            .map(|typ| self.resolve_type_identity(typ))
                            .unwrap_or(Type::Unknown),
                        *mutable,
                    )?;
                }
                let body_result = self
                    .collect_functions(body)
                    .and_then(|()| self.analyze_statements(body));
                self.blocked_function_references.pop();
                if let Err(error) = body_result {
                    self.variables.truncate(frame_start);
                    self.function_scopes.pop();
                    self.function_capture_frames.pop();
                    self.function_depth = saved_function_depth;
                    self.loop_depth = saved_loop_depth;
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
                    self.function_scopes.pop();
                    self.function_capture_frames.pop();
                    self.function_depth = saved_function_depth;
                    self.loop_depth = saved_loop_depth;
                    self.function_return = saved_return;
                    self.inferred_return = saved_inferred_return;
                    self.saw_return = saved_saw_return;
                    return Err(self.error(
                        DiagnosticCode::SemanticMissingReturn,
                        format!("function `{name}` must return {}", expected.name()),
                    ));
                }
                self.variables.truncate(frame_start);
                self.function_scopes.pop();
                self.function_capture_frames.pop();
                self.function_depth = saved_function_depth;
                self.loop_depth = saved_loop_depth;
                self.function_return = saved_return;
                if return_type.is_none() {
                    let inferred_return = self.inferred_return.clone();
                    if let Some(signature) = self
                        .function_scopes
                        .iter_mut()
                        .rev()
                        .find_map(|scope| scope.get_mut(name))
                    {
                        signature.return_type = inferred_return.clone();
                    }
                    if let Some(Type::Function { parameters, .. }) =
                        self.variables.get(name).cloned()
                    {
                        self.variables.replace_visible(
                            name,
                            Type::Function {
                                parameters,
                                return_type: inferred_return.map(Box::new),
                            },
                        );
                    }
                }
                self.inferred_return = saved_inferred_return;
                self.saw_return = saved_saw_return;
            }
            Stmt::Return(expression) => {
                let actual = self.analyze_expression(expression)?;
                if self.function_depth == 0 && !self.module_return_allowed {
                    return Err(self.error(
                        DiagnosticCode::InvalidReturn,
                        "return used outside a function",
                    ));
                }
                if self.function_depth == 0 {
                    if let Some(previous) = self.module_return_type.clone() {
                        self.require_type(&previous, &actual)?;
                    } else {
                        self.module_return_type = Some(actual);
                    }
                    self.saw_return = true;
                    return Ok(());
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
                let thrown_type = self.analyze_expression(expression)?;
                if !matches!(thrown_type, Type::Enum(_) | Type::Unknown) {
                    return Err(self.error(
                        DiagnosticCode::TypeMismatch,
                        format!(
                            "`throw` requires an enum value, found {}",
                            thrown_type.name()
                        ),
                    ));
                }
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
                self.function_scopes.push(HashMap::new());
                self.saw_return = false;
                let branch_result = self
                    .collect_functions(then_branch)
                    .and_then(|()| self.analyze_statements(then_branch));
                if let Err(error) = branch_result {
                    self.variables.truncate(frame_start);
                    self.function_scopes.pop();
                    self.saw_return = saved_saw_return;
                    return Err(error);
                }
                let then_returns = self.saw_return;
                self.variables.truncate(frame_start);
                self.function_scopes.pop();
                let frame_start = self.variables.scopes.len();
                self.variables.push();
                self.function_scopes.push(HashMap::new());
                self.saw_return = false;
                let branch_result = self
                    .collect_functions(else_branch)
                    .and_then(|()| self.analyze_statements(else_branch));
                if let Err(error) = branch_result {
                    self.variables.truncate(frame_start);
                    self.function_scopes.pop();
                    self.saw_return = saved_saw_return;
                    return Err(error);
                }
                let else_returns = self.saw_return;
                self.variables.truncate(frame_start);
                self.function_scopes.pop();
                self.saw_return = saved_saw_return || (then_returns && else_returns);
            }
            Stmt::Try {
                try_body,
                catches,
                finally_body,
            } => {
                let saved_saw_return = self.saw_return;
                let try_returns = self.analyze_scoped_block(try_body)?;
                let mut catch_returns = true;
                for catch in catches {
                    if let Some(code) = catch.code.as_deref()
                        && DiagnosticCode::from_code(code).is_none()
                    {
                        return Err(self.error(
                            DiagnosticCode::InvalidCatchCode,
                            format!("unknown diagnostic code `{code}` in catch clause"),
                        ));
                    }
                    let frame_start = self.variables.scopes.len();
                    self.variables.push();
                    self.function_scopes.push(HashMap::new());
                    if let Some(name) = &catch.binding {
                        self.define_variable(name.clone(), Type::Unknown, false)?;
                    }
                    self.saw_return = false;
                    let result = self
                        .collect_functions(&catch.body)
                        .and_then(|()| self.analyze_statements(&catch.body));
                    catch_returns &= self.saw_return;
                    self.variables.truncate(frame_start);
                    self.function_scopes.pop();
                    result?;
                }
                let finally_returns = self.analyze_scoped_block(finally_body)?;
                self.saw_return =
                    saved_saw_return || finally_returns || (try_returns && catch_returns);
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
                self.function_scopes.push(HashMap::new());
                let span = self.current_span.clone().unwrap_or_else(|| Span::new(0, 0));
                self.variables
                    .insert_at(name.clone(), element, *mutable, span)?;
                self.loop_depth += 1;
                let body_result = self
                    .collect_functions(body)
                    .and_then(|()| self.analyze_statements(body));
                if let Err(error) = body_result {
                    self.loop_depth -= 1;
                    self.variables.truncate(frame_start);
                    self.function_scopes.pop();
                    self.saw_return = saved_saw_return;
                    return Err(error);
                }
                self.loop_depth -= 1;
                self.variables.truncate(frame_start);
                self.function_scopes.pop();
                self.saw_return = saved_saw_return;
            }
            Stmt::While { condition, body } => {
                let condition_type = self.analyze_expression(condition)?;
                self.require_type(&Type::Bool, &condition_type)?;
                let saved_saw_return = self.saw_return;
                self.loop_depth += 1;
                let result = self
                    .collect_functions(body)
                    .and_then(|()| self.analyze_statements(body));
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
        Ok(())
    }

    pub(super) fn analyze_scoped_block(
        &mut self,
        statements: &[Stmt],
    ) -> Result<bool, SimplyError> {
        if statements.is_empty() {
            return Ok(false);
        }
        let frame_start = self.variables.scopes.len();
        let saved_saw_return = self.saw_return;
        self.variables.push();
        self.function_scopes.push(HashMap::new());
        self.saw_return = false;
        let result = self
            .collect_functions(statements)
            .and_then(|()| self.analyze_statements(statements));
        let returns = self.saw_return;
        self.variables.truncate(frame_start);
        self.function_scopes.pop();
        self.saw_return = saved_saw_return;
        result.map(|()| returns)
    }
}
