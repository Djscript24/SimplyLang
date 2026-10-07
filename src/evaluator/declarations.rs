use super::*;

impl Evaluator {
    pub fn new() -> Self {
        Self {
            scopes: ScopeStack::new(),
            variable_types: TypeScopes::new(),
            heap: RuntimeHeap::default(),
            function_arena: Arena::new(),
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

    pub(super) fn register_declarations(&mut self, statements: &[Stmt]) -> Result<(), SimplyError> {
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
                let function = self.track_function(FunctionValue {
                    name: None,
                    parameters: function_parameters,
                    return_type: None,
                    body: Arc::clone(body),
                    captures: HashMap::new(),
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

    pub(super) fn lookup_struct(&self, name: &str) -> Option<&StructDefinition> {
        self.struct_scopes
            .iter()
            .rev()
            .find_map(|scope| scope.get(name))
    }

    pub(super) fn lookup_enum(&self, name: &str) -> Option<&EnumDefinition> {
        self.enum_scopes
            .iter()
            .rev()
            .find_map(|scope| scope.get(name))
    }

    pub(super) fn lookup_struct_identity(
        &self,
        identity: &DeclarationIdentity,
    ) -> Option<&StructDefinition> {
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

    pub(super) fn lookup_message(
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
}
