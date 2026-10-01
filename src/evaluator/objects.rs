use super::*;

impl Evaluator {
    pub(super) fn dispatch_message(
        &mut self,
        receiver: Value,
        message: &str,
        arguments: Vec<Value>,
    ) -> Result<Value, SimplyError> {
        if let Value::Enum(value) = &receiver {
            let value = self.tracked_enum(value)?;
            return Err(self.runtime_error_with_code(
                DiagnosticCode::RuntimeMessage,
                format!(
                    "enum value `{}::{}` does not support message dispatch",
                    value.enum_name, value.variant_name
                ),
            ));
        }
        if let Value::Struct(instance) = &receiver {
            let instance_value = self.tracked_struct(instance)?;
            let behavior = self
                .lookup_message(&instance_value.identity, message)
                .cloned()
                .ok_or_else(|| {
                    self.runtime_error_with_code(
                        DiagnosticCode::RuntimeMessage,
                        format!(
                            "message `{message}` is not understood by `{}`",
                            instance_value.type_name
                        ),
                    )
                })?;
            let behavior_function = self.tracked_function(behavior.function)?;
            let fields = self
                .lookup_struct_identity(&instance_value.identity)
                .ok_or_else(|| {
                    self.runtime_error_with_code(
                        DiagnosticCode::RuntimeMessage,
                        format!("unknown struct type `{}`", instance_value.type_name),
                    )
                })?
                .fields
                .iter()
                .map(|field| {
                    instance_value
                        .fields
                        .get(&field.name)
                        .cloned()
                        .ok_or_else(|| {
                            self.runtime_error_with_code(
                                DiagnosticCode::RuntimeMessage,
                                format!(
                                    "struct `{}` is missing declared field `{}`",
                                    instance_value.type_name, field.name
                                ),
                            )
                        })
                })
                .collect::<Result<Vec<_>, _>>()?;
            if arguments.len() + behavior.field_count != behavior_function.parameters.len() {
                return Err(self.runtime_error_with_code(
                    DiagnosticCode::InvalidFunctionCall,
                    format!(
                        "message `{message}` expects {} arguments, got {}",
                        behavior_function.parameters.len() - behavior.field_count,
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

    pub(super) fn evaluate_match(
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
                        self.type_of_value(&bound_value),
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

    pub(super) fn csv_stream_version(
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

    pub(super) fn seek_csv_stream(
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

    pub(super) fn construct_struct(
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
        Ok(Value::Struct(ArenaRef::insert(
            self.struct_values.clone(),
            StructInstance {
                identity: definition.identity.clone(),
                type_name: name.into(),
                fields,
            },
        )))
    }

    pub(super) fn invoke_function(
        &mut self,
        name: &str,
        values: Vec<Value>,
    ) -> Result<Value, SimplyError> {
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
                    .and_then(|handle| self.function_arena.get(*handle))
                    .cloned()
                    .ok_or_else(|| {
                        let description = if name.chars().next().is_some_and(char::is_uppercase) {
                            format!("unknown struct type `{name}`")
                        } else {
                            format!("unknown function `{name}`")
                        };
                        self.runtime_error_with_code(DiagnosticCode::RuntimeName, description)
                    })?;
                self.track_function(FunctionValue {
                    name: Some(name.to_owned()),
                    parameters: function.parameters.clone(),
                    return_type: function.return_type.clone(),
                    body: Arc::clone(&function.body),
                    captures: HashMap::new(),
                    source: function.source.clone(),
                })
            }
        };
        self.invoke_function_value(name, function, values)
    }

    pub(super) fn invoke_function_value(
        &mut self,
        name: &str,
        function: Handle<FunctionValue>,
        values: Vec<Value>,
    ) -> Result<Value, SimplyError> {
        self.invoke_function_value_with_message_state(name, function, values, None)
    }

    pub(super) fn invoke_message_value(
        &mut self,
        name: &str,
        function: Handle<FunctionValue>,
        values: Vec<Value>,
        instance: ArenaRef<StructInstance>,
    ) -> Result<Value, SimplyError> {
        self.invoke_function_value_with_message_state(name, function, values, Some(instance))
    }

    pub(super) fn invoke_function_value_with_message_state(
        &mut self,
        name: &str,
        function: Handle<FunctionValue>,
        values: Vec<Value>,
        message_instance: Option<ArenaRef<StructInstance>>,
    ) -> Result<Value, SimplyError> {
        let function_value = self.tracked_function(function)?;
        if function_value.parameters.len() != values.len() {
            return Err(self.runtime_error_with_code(
                DiagnosticCode::RuntimeArgument,
                format!(
                    "function `{name}` expects {} arguments, got {}",
                    function_value.parameters.len(),
                    values.len()
                ),
            ));
        }
        for ((parameter, expected, _), value) in function_value.parameters.iter().zip(&values) {
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
        let needs_function_scope = !function_value.captures.is_empty()
            || function_value.name.as_ref().is_some_and(|function_name| {
                !matches!(
                    self.lookup(function_name),
                    Some(Value::Function(current)) if *current == function
                )
            });
        self.call_depth += 1;
        let function_source = function_value.source.clone();
        let previous_source = std::mem::replace(
            &mut self.current_source,
            function_source
                .as_ref()
                .map(|context| context.source.clone()),
        );
        let mut scopes_pushed = 0;
        let result = (|| {
            if needs_function_scope {
                self.push_scope();
                scopes_pushed += 1;
                if let Some(function_name) = &function_value.name {
                    let value = Value::Function(function);
                    self.variable_types.define(
                        function_name.clone(),
                        self.type_of_value(&value),
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
                for (capture, value) in &function_value.captures {
                    self.variable_types
                        .define(capture.clone(), self.type_of_value(value), false);
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
            for ((parameter, _, mutable), value) in function_value.parameters.iter().zip(values) {
                self.variable_types
                    .define(parameter.clone(), self.type_of_value(&value), *mutable);
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
            let result = self.execute_statements(&function_value.body);
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
            if let Some(expected) = &function_value.return_type {
                self.ensure_type(&result, expected, name)?;
            }
            Ok(result)
        });
        match function_source {
            Some(context) => {
                let Some(source) = context.source.with(|source| source.text.clone()) else {
                    return Err(self.runtime_error_with_code(
                        DiagnosticCode::RuntimeImport,
                        "function source text handle is no longer valid",
                    ));
                };
                result.map_err(|error| error.in_source(context.filename, source))
            }
            None => result,
        }
    }
}
