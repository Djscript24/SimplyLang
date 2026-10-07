use super::*;

impl Evaluator {
    pub(super) fn run_with_output(
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

    pub(super) fn register_top_level_functions(
        &mut self,
        statements: &[Stmt],
    ) -> Result<(), SimplyError> {
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
            let function_value = self.track_function(FunctionValue {
                name: Some(name.clone()),
                parameters: resolved_parameters.clone(),
                return_type: resolved_return_type.clone(),
                body: Arc::clone(body),
                captures: HashMap::new(),
                source: source.clone(),
            });
            self.function_scopes
                .last_mut()
                .expect("function scope stack always has a global scope")
                .insert(
                    name.clone(),
                    self.function_arena.insert(Function {
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
        self.current_source = Some(self.heap.insert_source(SourceText { text: source }));
        self.import_stack = vec![resolved];
        self.module_is_imported = false;
        self.run(&program)
    }

    pub(super) fn execute_statements(&mut self, statements: &[Stmt]) -> Result<Flow, SimplyError> {
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
                                self.type_of_value(&value),
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
                        let typ = self.type_of_value(&value);
                        self.variable_types.define(
                            name.clone(),
                            if *mutable {
                                Self::erase_inferred_dimensions(typ)
                            } else {
                                typ
                            },
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
                        .define(name.clone(), self.type_of_value(&value), false);
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
                                Type::List(Box::new(self.type_of_value(&value))),
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
                        && let Type::Array(element) | Type::List(element) | Type::Vector(element, _) =
                            expected
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
                            let typ = self.type_of_value(&value);
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
                    let function = self.track_function(FunctionValue {
                        name: Some(name.clone()),
                        parameters: resolved_parameters.clone(),
                        return_type: resolved_return_type.clone(),
                        body: Arc::clone(body),
                        captures: if self.function_scopes.len() > 1 || self.import_stack.len() > 1 {
                            let mut dependencies = closure_dependencies(body, parameters);
                            dependencies.remove(name);
                            self.scopes.values_for(&dependencies)
                        } else {
                            HashMap::new()
                        },
                        source: source.clone(),
                    });
                    let function_value = Value::Function(function);
                    self.function_scopes
                        .last_mut()
                        .expect("function scope stack always has a global scope")
                        .insert(
                            name.clone(),
                            self.function_arena.insert(Function {
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
                                        .map(|value| self.type_of_value(value))
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

    pub(super) fn evaluate_throw(&mut self, expression: &Expr) -> Result<Flow, SimplyError> {
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

    pub(super) fn execute_scoped(&mut self, statements: &[Stmt]) -> Result<Flow, SimplyError> {
        if statements.is_empty() {
            return Ok(Flow::None);
        }
        self.push_scope();
        let result = self.execute_statements(statements);
        self.pop_scope();
        result
    }

    pub(super) fn error_value(&self, error: &SimplyError) -> Value {
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
}
