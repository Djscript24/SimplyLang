use super::*;

impl Evaluator {
    pub(super) fn collect_reachable_storage_identities(
        &self,
        value: &Value,
        identities: &mut HashSet<(u64, usize, u32)>,
    ) -> Result<(), SimplyError> {
        match value {
            Value::Array(values) | Value::List(values) => {
                let Some(identity) = values.identity_key() else {
                    return Err(self.runtime_error_with_code(
                        DiagnosticCode::RuntimeCollection,
                        "collection storage is no longer available",
                    ));
                };
                if !identities.insert(identity) {
                    return Ok(());
                }
                let values = values.get_cloned().ok_or_else(|| {
                    self.runtime_error_with_code(
                        DiagnosticCode::RuntimeCollection,
                        "collection storage is no longer available",
                    )
                })?;
                for value in &values {
                    self.collect_reachable_storage_identities(value, identities)?;
                }
            }
            Value::Hash(values) => {
                let Some(identity) = values.identity_key() else {
                    return Err(self.runtime_error_with_code(
                        DiagnosticCode::RuntimeCollection,
                        "collection storage is no longer available",
                    ));
                };
                if !identities.insert(identity) {
                    return Ok(());
                }
                let values = values.get_cloned().ok_or_else(|| {
                    self.runtime_error_with_code(
                        DiagnosticCode::RuntimeCollection,
                        "collection storage is no longer available",
                    )
                })?;
                for value in values.values() {
                    self.collect_reachable_storage_identities(value, identities)?;
                }
            }
            Value::Tuple(values) | Value::Matrix(values) => {
                for value in values.iter() {
                    self.collect_reachable_storage_identities(value, identities)?;
                }
            }
            Value::Struct(instance) => {
                let Some(identity) = instance.identity_key() else {
                    return Err(self.runtime_error_with_code(
                        DiagnosticCode::RuntimeMessage,
                        "struct instance is no longer available",
                    ));
                };
                if !identities.insert(identity) {
                    return Ok(());
                }
                let instance = self.tracked_struct(instance)?;
                for value in instance.fields.values() {
                    self.collect_reachable_storage_identities(value, identities)?;
                }
            }
            Value::Enum(value) => {
                let value = self.tracked_enum(value)?;
                if let Some(payload) = value.payload {
                    self.collect_reachable_storage_identities(&payload, identities)?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    pub(super) fn ensure_no_scoped_shared_borrow_escape(
        &self,
        value: &Value,
        destination: &str,
    ) -> Result<(), SimplyError> {
        if !self
            .active_collection_borrows
            .iter()
            .any(|borrow| borrow.escape_kind.is_some())
        {
            return Ok(());
        }
        let mut identities = HashSet::new();
        self.collect_reachable_storage_identities(value, &mut identities)?;
        if self.active_collection_borrows.iter().any(|borrow| {
            borrow.escape_kind.is_some()
                && borrow
                    .locations
                    .iter()
                    .any(|location| identities.contains(&location.identity))
        }) {
            let description =
                if self.active_collection_borrows.iter().any(|borrow| {
                    matches!(borrow.escape_kind, Some(SharedBorrowEscapeKind::Iteration))
                }) {
                    "shared iteration value"
                } else {
                    "shared match reference"
                };
            return Err(self.runtime_error_with_code(
                DiagnosticCode::InvalidRefUsage,
                format!("{description} cannot escape through {destination}"),
            ));
        }
        Ok(())
    }

    pub(super) fn dispatch_message(
        &mut self,
        receiver: Value,
        message: &str,
        arguments: Vec<Value>,
        receiver_source: &Expr,
        argument_sources: &[Expr],
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
            let mut receiver_identities = HashSet::new();
            self.collect_reachable_storage_identities(&receiver, &mut receiver_identities)?;
            if self.active_collection_borrows.iter().any(|borrow| {
                borrow.mode == RuntimeBorrowMode::Exclusive
                    && borrow
                        .locations
                        .iter()
                        .any(|location| receiver_identities.contains(&location.identity))
            }) {
                return Err(self.runtime_error_with_code(
                    DiagnosticCode::InvalidRefUsage,
                    format!(
                        "message `{message}` conflicts with an active exclusive borrow of its receiver"
                    ),
                ));
            }
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
        let mut sources = Vec::with_capacity(argument_sources.len() + 1);
        sources.push(receiver_source.clone());
        sources.extend_from_slice(argument_sources);
        self.invoke_function_with_sources(message, values, Some(&sources))
    }

    pub(super) fn evaluate_match(
        &mut self,
        value: Value,
        arms: &[crate::ast::MatchArm],
        source_name: Option<&str>,
    ) -> Result<Value, SimplyError> {
        for arm in arms {
            let Some(bindings) = self.match_value_pattern(&arm.pattern, &value)? else {
                continue;
            };
            self.push_scope();
            let result = (|| {
                let reference_binding = match &arm.pattern {
                    crate::ast::MatchPattern::ReferenceIdentifier(name) => Some(name.as_str()),
                    _ => None,
                };
                if let Some(name) = reference_binding {
                    let source_name = source_name.ok_or_else(|| {
                        self.runtime_error_with_code(
                            DiagnosticCode::InvalidRefUsage,
                            "`ref` match binding requires a named collection scrutinee",
                        )
                    })?;
                    if !matches!(&value, Value::Array(_) | Value::List(_) | Value::Hash(_)) {
                        return Err(self.runtime_error_with_code(
                            DiagnosticCode::InvalidRefUsage,
                            "`ref` match binding requires an Array, List, or Hash value",
                        ));
                    }
                    let mut identities = HashSet::new();
                    self.collect_reachable_storage_identities(&value, &mut identities)?;
                    let locations = identities
                        .into_iter()
                        .map(|identity| CollectionBorrowLocation {
                            identity,
                            path: Vec::new(),
                        })
                        .collect();
                    self.begin_shared_match_borrow(name, source_name, locations)?;
                }
                for (binding, bound_value) in bindings {
                    self.variable_types.define(
                        binding.clone(),
                        self.type_of_value(&bound_value),
                        false,
                    );
                    self.scopes
                        .define_with_borrow_mode(
                            binding.clone(),
                            bound_value,
                            false,
                            reference_binding == Some(binding.as_str()),
                            false,
                        )
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
            let result = match result {
                Ok(Some(value)) => self
                    .ensure_no_scoped_shared_borrow_escape(&value, "a match result")
                    .map(|()| Some(value)),
                result => result,
            };
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
        Ok(Value::Struct(self.heap.insert_struct(StructInstance {
            identity: definition.identity.clone(),
            type_name: name.into(),
            fields,
        })))
    }

    pub(super) fn invoke_function_with_sources(
        &mut self,
        name: &str,
        values: Vec<Value>,
        sources: Option<&[Expr]>,
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
        self.invoke_function_value_with_sources(name, function, values, sources)
    }

    pub(super) fn invoke_function_value_with_sources(
        &mut self,
        name: &str,
        function: Handle<FunctionValue>,
        values: Vec<Value>,
        sources: Option<&[Expr]>,
    ) -> Result<Value, SimplyError> {
        self.invoke_function_value_with_context(name, function, values, None, sources)
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
        self.invoke_function_value_with_context(name, function, values, message_instance, None)
    }

    fn invoke_function_value_with_context(
        &mut self,
        name: &str,
        function: Handle<FunctionValue>,
        values: Vec<Value>,
        message_instance: Option<ArenaRef<StructInstance>>,
        argument_sources: Option<&[Expr]>,
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
        for ((parameter, expected, _, _), value) in function_value.parameters.iter().zip(&values) {
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
        let borrow_checkpoint = self.active_collection_borrows.len();
        for (index, ((parameter, _, mutable, by_ref), value)) in
            function_value.parameters.iter().zip(&values).enumerate()
        {
            if !by_ref {
                continue;
            }
            let Some(identity) = Self::collection_identity(value) else {
                self.active_collection_borrows.truncate(borrow_checkpoint);
                return Err(self.runtime_error_with_code(
                    DiagnosticCode::InvalidRefUsage,
                    format!("`ref` parameter `{parameter}` requires a collection value"),
                ));
            };
            let mode = if *mutable {
                RuntimeBorrowMode::Exclusive
            } else {
                RuntimeBorrowMode::Shared
            };
            let source_name = argument_sources
                .and_then(|sources| sources.get(index))
                .and_then(|source| match source {
                    Expr::Identifier(name) => Some(name.as_str()),
                    _ => None,
                });
            if mode == RuntimeBorrowMode::Exclusive {
                let Some(source_name) = source_name else {
                    self.active_collection_borrows.truncate(borrow_checkpoint);
                    return Err(self.runtime_error_with_code(
                        DiagnosticCode::InvalidRefUsage,
                        format!("mutable `ref` argument to `{name}` must be a mutable variable"),
                    ));
                };
                if !self.scopes.is_mutable(source_name) {
                    self.active_collection_borrows.truncate(borrow_checkpoint);
                    return Err(self.runtime_error_with_code(
                        DiagnosticCode::InvalidRefUsage,
                        format!(
                            "mutable `ref` argument `{source_name}` to `{name}` requires a `mut` owner"
                        ),
                    ));
                }
            }
            let has_active_borrow = self.active_collection_borrows.iter().any(|borrow| {
                borrow.locations.iter().any(|location| {
                    Self::borrow_locations_overlap(
                        &CollectionBorrowLocation {
                            identity,
                            path: Vec::new(),
                        },
                        location,
                    )
                })
            });
            if has_active_borrow {
                let compatible_shared_borrows = mode == RuntimeBorrowMode::Shared
                    && self
                        .active_collection_borrows
                        .iter()
                        .filter(|borrow| {
                            borrow.locations.iter().any(|location| {
                                Self::borrow_locations_overlap(
                                    &CollectionBorrowLocation {
                                        identity,
                                        path: Vec::new(),
                                    },
                                    location,
                                )
                            })
                        })
                        .all(|borrow| borrow.mode == RuntimeBorrowMode::Shared);
                let forwarded = source_name.is_some_and(|source_name| {
                    self.active_collection_borrows
                        .iter()
                        .filter(|borrow| {
                            borrow.locations.iter().any(|location| {
                                Self::borrow_locations_overlap(
                                    &CollectionBorrowLocation {
                                        identity,
                                        path: Vec::new(),
                                    },
                                    location,
                                )
                            })
                        })
                        .any(|borrow| {
                            borrow.parameter == source_name
                                && match (borrow.mode, mode) {
                                    (RuntimeBorrowMode::Shared, RuntimeBorrowMode::Shared)
                                    | (RuntimeBorrowMode::Exclusive, RuntimeBorrowMode::Shared)
                                    | (
                                        RuntimeBorrowMode::Exclusive,
                                        RuntimeBorrowMode::Exclusive,
                                    ) => true,
                                    (RuntimeBorrowMode::Shared, RuntimeBorrowMode::Exclusive) => {
                                        false
                                    }
                                }
                        })
                });
                if !compatible_shared_borrows
                    && (!forwarded
                        || (mode == RuntimeBorrowMode::Exclusive
                            && self
                                .active_collection_borrows
                                .iter()
                                .filter(|borrow| {
                                    borrow.locations.iter().any(|location| {
                                        Self::borrow_locations_overlap(
                                            &CollectionBorrowLocation {
                                                identity,
                                                path: Vec::new(),
                                            },
                                            location,
                                        )
                                    })
                                })
                                .any(|borrow| borrow.mode == RuntimeBorrowMode::Shared)))
                {
                    self.active_collection_borrows.truncate(borrow_checkpoint);
                    return Err(self.runtime_error_with_code(
                        DiagnosticCode::InvalidRefUsage,
                        format!("conflicting borrow of collection passed to `{name}`"),
                    ));
                }
            }
            self.active_collection_borrows.push(ActiveCollectionBorrow {
                locations: vec![CollectionBorrowLocation {
                    identity,
                    path: Vec::new(),
                }],
                mode,
                parameter: parameter.clone(),
                owner: None,
                path: Vec::new(),
                scope_index: None,
                escape_kind: None,
            });
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
            for ((parameter, _, mutable, by_ref), value) in
                function_value.parameters.iter().zip(values)
            {
                let is_mutable = *mutable;
                self.variable_types.define(
                    parameter.clone(),
                    self.type_of_value(&value),
                    is_mutable,
                );
                self.scopes
                    .define_with_borrow_mode(
                        parameter.clone(),
                        value,
                        is_mutable,
                        *by_ref,
                        is_mutable && *by_ref,
                    )
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
        self.active_collection_borrows.truncate(borrow_checkpoint);
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

    pub(super) fn collection_identity(value: &Value) -> Option<(u64, usize, u32)> {
        match value {
            Value::Array(values) | Value::List(values) => values.identity_key(),
            Value::Hash(values) => values.identity_key(),
            _ => None,
        }
    }

    pub(super) fn borrow_location_identity(&self, value: &Value) -> Option<(u64, usize, u32)> {
        Self::collection_identity(value).or_else(|| match value {
            Value::Struct(instance) => instance.identity_key(),
            _ => None,
        })
    }

    pub(super) fn ensure_collection_mutation_allowed(
        &self,
        name: &str,
        target: &Value,
    ) -> Result<(), SimplyError> {
        let Some(identity) = Self::collection_identity(target) else {
            return Ok(());
        };
        let mut has_shared = false;
        let mut most_recent = None;
        for borrow in self.active_collection_borrows.iter().filter(|borrow| {
            borrow
                .locations
                .iter()
                .any(|location| location.identity == identity)
        }) {
            has_shared |= borrow.mode == RuntimeBorrowMode::Shared;
            most_recent = Some(borrow);
        }
        let Some(most_recent) = most_recent else {
            return Ok(());
        };
        if has_shared
            || most_recent.mode != RuntimeBorrowMode::Exclusive
            || most_recent.parameter != name
            || !self.scopes.is_exclusive_borrow(name)
        {
            return Err(self.runtime_error_with_code(
                DiagnosticCode::InvalidRefUsage,
                format!("mutation conflicts with an active borrow of `{name}`"),
            ));
        }
        Ok(())
    }

    pub(super) fn ensure_collection_reassignment_allowed(
        &self,
        name: &str,
        current_value: &Value,
    ) -> Result<(), SimplyError> {
        let Some(identity) = self.borrow_location_identity(current_value) else {
            return Ok(());
        };
        if self.active_collection_borrows.iter().any(|borrow| {
            borrow
                .locations
                .iter()
                .any(|location| location.identity == identity)
        }) {
            return Err(self.runtime_error_with_code(
                DiagnosticCode::InvalidRefUsage,
                format!("cannot reassign `{name}` while its collection is borrowed"),
            ));
        }
        Ok(())
    }

    pub(super) fn write_through_local_element_borrow(
        &mut self,
        name: &str,
        value: Value,
    ) -> Result<bool, SimplyError> {
        let Some(borrow) = self.active_collection_borrows.iter().rev().find(|borrow| {
            borrow.parameter == name
                && borrow.mode == RuntimeBorrowMode::Exclusive
                && borrow.scope_index.is_some()
                && !borrow.path.is_empty()
        }) else {
            return Ok(false);
        };
        let owner = borrow.owner.clone().ok_or_else(|| {
            self.runtime_error_with_code(
                DiagnosticCode::InvalidRefUsage,
                format!("exclusive element reference `{name}` has no live owner"),
            )
        })?;
        let path = borrow.path.clone();
        let alias_value = value.clone();
        let span = self.current_span.clone();
        let Some(target) = self.lookup_mut(&owner) else {
            return Err(self.runtime_error_with_code(
                DiagnosticCode::InvalidRefUsage,
                format!("owner `{owner}` of element reference `{name}` is no longer available"),
            ));
        };
        Self::set_borrow_location_path(target, &path, value, span.as_ref())?;
        if !self.scopes.assign(name, alias_value) {
            return Err(self.runtime_error_with_code(
                DiagnosticCode::InvalidRefUsage,
                format!("element reference `{name}` is no longer available"),
            ));
        }
        self.persist_active_message_field(&owner);
        Ok(true)
    }

    fn set_borrow_location_path(
        target: &mut Value,
        path: &[CollectionBorrowSegment],
        value: Value,
        span: Option<&Span>,
    ) -> Result<(), SimplyError> {
        let Some((segment, remaining)) = path.split_first() else {
            return Err(Self::borrow_path_error(
                span,
                "empty reference location path",
            ));
        };

        if let CollectionBorrowSegment::Field(field) = segment {
            let Value::Struct(instance) = target else {
                return Err(Self::borrow_path_error(
                    span,
                    "field reference no longer points to a Struct",
                ));
            };
            if remaining.is_empty() {
                return instance
                    .with_mut(|instance| instance.fields.get_mut(field).map(|slot| *slot = value))
                    .flatten()
                    .ok_or_else(|| Self::borrow_path_error(span, "unknown Struct field"));
            }
            let mut nested = instance
                .with(|instance| instance.fields.get(field).cloned())
                .flatten()
                .ok_or_else(|| Self::borrow_path_error(span, "unknown Struct field"))?;
            return Self::set_borrow_location_path(&mut nested, remaining, value, span);
        }

        let index = match segment {
            CollectionBorrowSegment::Index(index) => Value::Int(*index),
            CollectionBorrowSegment::Key(key) => Value::String(key.clone()),
            CollectionBorrowSegment::Field(_) => unreachable!(),
        };
        if remaining.is_empty() {
            return collections::set_index(target, index, value, span);
        }
        let mut nested = collections::index(target, &index, span)?;
        Self::set_borrow_location_path(&mut nested, remaining, value, span)
    }

    fn borrow_path_error(span: Option<&Span>, message: &str) -> SimplyError {
        SimplyError::Runtime {
            span: span.cloned().unwrap_or_else(|| Span::new(0, 0)),
            code: DiagnosticCode::InvalidRefUsage,
            message: message.to_owned(),
        }
    }

    pub(super) fn ensure_active_message_field_mutation_allowed(
        &self,
        name: &str,
    ) -> Result<(), SimplyError> {
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
            return Ok(());
        };
        let Some(identity) = instance.identity_key() else {
            return Err(self.runtime_error_with_code(
                DiagnosticCode::RuntimeMessage,
                "struct instance is no longer available",
            ));
        };
        let candidate = CollectionBorrowLocation {
            identity,
            path: vec![CollectionBorrowSegment::Field(name.to_owned())],
        };
        if self.active_collection_borrows.iter().any(|borrow| {
            borrow
                .locations
                .iter()
                .any(|location| Self::borrow_locations_overlap(&candidate, location))
        }) {
            return Err(self.runtime_error_with_code(
                DiagnosticCode::InvalidRefUsage,
                format!("mutation of Struct field `{name}` conflicts with an active borrow"),
            ));
        }
        Ok(())
    }

    pub(super) fn has_writable_local_element_borrow(&self, name: &str) -> bool {
        self.active_collection_borrows.iter().any(|borrow| {
            borrow.parameter == name
                && borrow.mode == RuntimeBorrowMode::Exclusive
                && borrow.scope_index.is_some()
                && !borrow.path.is_empty()
        })
    }

    pub(super) fn begin_local_collection_borrow(
        &mut self,
        name: &str,
        source_name: &str,
        locations: Vec<CollectionBorrowLocation>,
        path: Vec<CollectionBorrowSegment>,
        exclusive: bool,
    ) -> Result<(), SimplyError> {
        if locations.is_empty() {
            return Err(self.runtime_error_with_code(
                DiagnosticCode::InvalidRefUsage,
                format!("local `ref` binding `{name}` requires a collection"),
            ));
        }
        if exclusive && !self.scopes.is_mutable(source_name) {
            return Err(self.runtime_error_with_code(
                DiagnosticCode::InvalidRefUsage,
                format!("mutable `ref` binding `{name}` requires a `mut` owner"),
            ));
        }
        let mode = if exclusive {
            RuntimeBorrowMode::Exclusive
        } else {
            RuntimeBorrowMode::Shared
        };
        let mut has_active = false;
        let mut has_shared = false;
        let mut source_is_exclusive = false;
        for borrow in &self.active_collection_borrows {
            let overlaps = locations.iter().any(|candidate| {
                borrow
                    .locations
                    .iter()
                    .any(|existing| Self::borrow_locations_overlap(candidate, existing))
            });
            if !overlaps {
                continue;
            }
            has_active = true;
            has_shared |= borrow.mode == RuntimeBorrowMode::Shared;
            source_is_exclusive |=
                borrow.parameter == source_name && borrow.mode == RuntimeBorrowMode::Exclusive;
        }
        let valid_reborrow =
            locations.first().is_some_and(|candidate_root| {
                source_is_exclusive
                    && self.active_collection_borrows.iter().any(|borrow| {
                        borrow.parameter == source_name
                            && borrow.mode == RuntimeBorrowMode::Exclusive
                            && borrow.locations.iter().any(|existing| {
                                existing.identity == candidate_root.identity
                                    && existing.path.len() <= candidate_root.path.len()
                                    && existing.path.iter().zip(&candidate_root.path).all(
                                        |(left, right)| Self::borrow_segments_equal(left, right),
                                    )
                            })
                    })
            });
        if has_active
            && (mode == RuntimeBorrowMode::Exclusive && (!valid_reborrow || has_shared)
                || mode == RuntimeBorrowMode::Shared
                    && self.active_collection_borrows.iter().any(|borrow| {
                        borrow.mode == RuntimeBorrowMode::Exclusive
                            && locations.iter().any(|candidate| {
                                borrow.locations.iter().any(|existing| {
                                    Self::borrow_locations_overlap(candidate, existing)
                                })
                            })
                    })
                    && !valid_reborrow)
        {
            return Err(self.runtime_error_with_code(
                DiagnosticCode::InvalidRefUsage,
                format!("conflicting borrow of collection for local binding `{name}`"),
            ));
        }
        self.active_collection_borrows.push(ActiveCollectionBorrow {
            locations,
            mode,
            parameter: name.to_owned(),
            owner: Some(source_name.to_owned()),
            path,
            scope_index: Some(self.scopes.current_scope_index()),
            escape_kind: None,
        });
        Ok(())
    }

    pub(super) fn begin_shared_iteration_borrow(
        &mut self,
        name: &str,
        source_name: &str,
        locations: Vec<CollectionBorrowLocation>,
    ) -> Result<(), SimplyError> {
        if locations.is_empty() {
            return Err(self.runtime_error_with_code(
                DiagnosticCode::InvalidRefUsage,
                format!("`for ref {name}` requires a borrowable collection"),
            ));
        }

        for borrow in &self.active_collection_borrows {
            if borrow.mode != RuntimeBorrowMode::Exclusive
                || !locations.iter().any(|candidate| {
                    borrow
                        .locations
                        .iter()
                        .any(|existing| Self::borrow_locations_overlap(candidate, existing))
                })
            {
                continue;
            }

            let valid_reborrow = borrow.parameter == source_name
                && borrow.locations.iter().any(|existing| {
                    locations.iter().any(|candidate| {
                        existing.identity == candidate.identity
                            && existing.path.is_empty()
                            && candidate.path.is_empty()
                    })
                });
            if !valid_reborrow {
                return Err(self.runtime_error_with_code(
                    DiagnosticCode::InvalidRefUsage,
                    format!("conflicting borrow for `for ref {name}`"),
                ));
            }
        }

        self.active_collection_borrows.push(ActiveCollectionBorrow {
            locations,
            mode: RuntimeBorrowMode::Shared,
            parameter: name.to_owned(),
            owner: Some(source_name.to_owned()),
            path: Vec::new(),
            scope_index: Some(self.scopes.current_scope_index()),
            escape_kind: Some(SharedBorrowEscapeKind::Iteration),
        });
        Ok(())
    }

    pub(super) fn begin_shared_match_borrow(
        &mut self,
        name: &str,
        source_name: &str,
        locations: Vec<CollectionBorrowLocation>,
    ) -> Result<(), SimplyError> {
        if locations.is_empty() {
            return Err(self.runtime_error_with_code(
                DiagnosticCode::InvalidRefUsage,
                "a `ref` match binding requires a borrowable collection",
            ));
        }

        for borrow in &self.active_collection_borrows {
            if borrow.mode != RuntimeBorrowMode::Exclusive
                || !locations.iter().any(|candidate| {
                    borrow
                        .locations
                        .iter()
                        .any(|existing| Self::borrow_locations_overlap(candidate, existing))
                })
            {
                continue;
            }

            let valid_reborrow = borrow.parameter == source_name
                && borrow.locations.iter().any(|existing| {
                    locations.iter().any(|candidate| {
                        existing.identity == candidate.identity
                            && existing.path.is_empty()
                            && candidate.path.is_empty()
                    })
                });
            if !valid_reborrow {
                return Err(self.runtime_error_with_code(
                    DiagnosticCode::InvalidRefUsage,
                    format!("conflicting borrow for match binding `{name}`"),
                ));
            }
        }

        self.active_collection_borrows.push(ActiveCollectionBorrow {
            locations,
            mode: RuntimeBorrowMode::Shared,
            parameter: name.to_owned(),
            owner: Some(source_name.to_owned()),
            path: Vec::new(),
            scope_index: Some(self.scopes.current_scope_index()),
            escape_kind: Some(SharedBorrowEscapeKind::MatchPattern),
        });
        Ok(())
    }

    fn borrow_segments_equal(
        left: &CollectionBorrowSegment,
        right: &CollectionBorrowSegment,
    ) -> bool {
        match (left, right) {
            (CollectionBorrowSegment::Index(left), CollectionBorrowSegment::Index(right)) => {
                left == right
            }
            (CollectionBorrowSegment::Key(left), CollectionBorrowSegment::Key(right)) => {
                left == right
            }
            (CollectionBorrowSegment::Field(left), CollectionBorrowSegment::Field(right)) => {
                left == right
            }
            _ => false,
        }
    }

    fn borrow_locations_overlap(
        left: &CollectionBorrowLocation,
        right: &CollectionBorrowLocation,
    ) -> bool {
        if left.identity != right.identity {
            return false;
        }
        for (left, right) in left.path.iter().zip(&right.path) {
            if !Self::borrow_segments_equal(left, right) {
                return !matches!(
                    (left, right),
                    (
                        CollectionBorrowSegment::Index(_),
                        CollectionBorrowSegment::Index(_)
                    ) | (
                        CollectionBorrowSegment::Key(_),
                        CollectionBorrowSegment::Key(_)
                    ) | (
                        CollectionBorrowSegment::Field(_),
                        CollectionBorrowSegment::Field(_)
                    )
                );
            }
        }
        true
    }
}
