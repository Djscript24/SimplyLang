use super::*;

impl SemanticAnalyzer {
    pub(super) fn is_mutable_collection_type(typ: &Type) -> bool {
        matches!(
            typ,
            Type::Array(_) | Type::List(_) | Type::Hash | Type::HashValues(_) | Type::Vector(_, _)
        )
    }

    pub(super) fn collect_structs_and_messages(
        &mut self,
        statements: &[Stmt],
    ) -> Result<(), SimplyError> {
        let statements = statements.iter().map(|statement| match statement {
            Stmt::Located { span, statement } => (Some(span.clone()), statement.as_ref()),
            statement => (None, statement),
        });
        let statements = statements.collect::<Vec<_>>();
        for (span, statement) in &statements {
            if let Some(span) = span {
                self.current_span = Some(span.clone());
            }
            if let Stmt::Enum { name, variants } = statement {
                if self.enums.contains_key(name) || self.structs.contains_key(name) {
                    return Err(self.error(
                        DiagnosticCode::DuplicateDeclaration,
                        format!("type `{name}` is already declared"),
                    ));
                }
                if variants.is_empty() {
                    return Err(self.error(
                        DiagnosticCode::UnexpectedToken,
                        format!("enum `{name}` must declare at least one variant"),
                    ));
                }
                self.enum_identities
                    .insert(name.clone(), self.identity(name, DeclarationKind::Enum));
                self.enums.insert(
                    name.clone(),
                    variants
                        .iter()
                        .cloned()
                        .map(|mut variant| {
                            variant.payload_type = variant
                                .payload_type
                                .as_ref()
                                .map(|typ| self.resolve_type_identity(typ));
                            variant
                        })
                        .collect(),
                );
            }
        }
        for (span, statement) in &statements {
            if let Some(span) = span {
                self.current_span = Some(span.clone());
            }
            if let Stmt::Struct { name, fields } = statement {
                if self.structs.contains_key(name) || self.enums.contains_key(name) {
                    return Err(self.error(
                        DiagnosticCode::DuplicateDeclaration,
                        format!("type `{name}` is already declared"),
                    ));
                }
                self.struct_identities
                    .insert(name.clone(), self.identity(name, DeclarationKind::Struct));
                self.structs.insert(
                    name.clone(),
                    fields
                        .iter()
                        .cloned()
                        .map(|mut field| {
                            field.field_type = self.resolve_type_identity(&field.field_type);
                            field
                        })
                        .collect(),
                );
            }
        }
        let enum_definitions = self
            .enums
            .iter()
            .map(|(name, variants)| (name.clone(), variants.clone()))
            .collect::<Vec<_>>();
        for (name, variants) in enum_definitions {
            self.enums.insert(
                name,
                variants
                    .into_iter()
                    .map(|mut variant| {
                        variant.payload_type = variant
                            .payload_type
                            .as_ref()
                            .map(|typ| self.resolve_type_identity(typ));
                        variant
                    })
                    .collect(),
            );
        }
        let struct_definitions = self
            .structs
            .iter()
            .map(|(name, fields)| (name.clone(), fields.clone()))
            .collect::<Vec<_>>();
        for (name, fields) in struct_definitions {
            self.structs.insert(
                name,
                fields
                    .into_iter()
                    .map(|mut field| {
                        field.field_type = self.resolve_type_identity(&field.field_type);
                        field
                    })
                    .collect(),
            );
        }
        for (span, statement) in statements {
            if let Some(span) = span {
                self.current_span = Some(span);
            }
            match statement {
                Stmt::Struct { fields, .. } => {
                    for field in fields {
                        self.validate_declared_type(&field.field_type)?;
                    }
                }
                Stmt::Enum { variants, .. } => {
                    for variant in variants {
                        if let Some(payload_type) = &variant.payload_type {
                            self.validate_declared_type(payload_type)?;
                        }
                    }
                }
                Stmt::Message {
                    receiver_type,
                    name,
                    parameters,
                    ..
                } => {
                    if !self.structs.contains_key(receiver_type) {
                        return Err(self.error(
                            DiagnosticCode::InvalidFunctionCall,
                            format!("unknown receiver type `{receiver_type}` for message `{name}`"),
                        ));
                    }
                    let key = (self.struct_identities[receiver_type].clone(), name.clone());
                    if self.messages.contains_key(&key) {
                        return Err(self.error(
                            DiagnosticCode::DuplicateDeclaration,
                            format!("message `{name}` is already defined for `{receiver_type}`"),
                        ));
                    }
                    let fields = &self.structs[receiver_type];
                    for (parameter, parameter_type, _, by_ref) in parameters {
                        if fields.iter().any(|field| &field.name == parameter) {
                            return Err(self.error(
                                DiagnosticCode::DuplicateDeclaration,
                                format!(
                                    "message parameter `{parameter}` conflicts with a field of `{receiver_type}`"
                                ),
                            ));
                        }
                        if let Some(parameter_type) = parameter_type {
                            self.validate_declared_type(parameter_type)?;
                        }
                        if *by_ref
                            && !parameter_type.as_ref().is_some_and(|typ| {
                                matches!(
                                    typ,
                                    Type::Array(_)
                                        | Type::List(_)
                                        | Type::Hash
                                        | Type::HashValues(_)
                                        | Type::Vector(_, _)
                                )
                            })
                        {
                            return Err(self.error(
                                DiagnosticCode::InvalidRefUsage,
                                format!(
                                    "`ref` parameter `{parameter}` must be an Array, List, or Hash"
                                ),
                            ));
                        }
                        if !*by_ref
                            && parameter_type
                                .as_ref()
                                .is_some_and(Self::is_mutable_collection_type)
                        {
                            return Err(self.error(
                                DiagnosticCode::InvalidRefUsage,
                                format!(
                                    "collection parameter `{parameter}` must be declared `ref`"
                                ),
                            ));
                        }
                    }
                    self.messages.insert(
                        key,
                        MessageSignature {
                            parameters: parameters
                                .iter()
                                .map(|(_, parameter_type, _, _)| {
                                    parameter_type
                                        .as_ref()
                                        .map(|typ| self.resolve_type_identity(typ))
                                })
                                .collect(),
                            ref_parameters: parameters
                                .iter()
                                .map(|(_, _, _, by_ref)| *by_ref)
                                .collect(),
                            return_type: None,
                        },
                    );
                }
                _ => {}
            }
        }
        Ok(())
    }

    pub(super) fn validate_destructure_assignment_pattern(
        &self,
        pattern: &MatchPattern,
        actual: &Type,
        targets: &mut Vec<String>,
    ) -> Result<(), SimplyError> {
        match pattern {
            MatchPattern::Identifier(name) => {
                self.validate_destructure_assignment_identifier(name, actual, targets)?;
            }
            MatchPattern::Wildcard => {}
            MatchPattern::Tuple(patterns) => match actual {
                Type::Tuple(types) if types.len() == patterns.len() => {
                    for (pattern, typ) in patterns.iter().zip(types) {
                        self.validate_destructure_assignment_pattern(pattern, typ, targets)?;
                    }
                }
                Type::Tuple(types) => {
                    return Err(self.error(
                        DiagnosticCode::SemanticDestructure,
                        format!(
                            "tuple has {} values, but destructuring assignment target has {} elements",
                            types.len(),
                            patterns.len()
                        ),
                    ));
                }
                Type::Unknown => {
                    for pattern in patterns {
                        self.validate_destructure_assignment_pattern(
                            pattern,
                            &Type::Unknown,
                            targets,
                        )?;
                    }
                }
                _ => {
                    return Err(self.error(
                        DiagnosticCode::SemanticDestructure,
                        "tuple destructuring assignment requires a tuple value",
                    ));
                }
            },
            MatchPattern::Sequence { patterns, rest } => {
                let (element_type, rest_type) = match actual {
                    Type::Array(element) | Type::Vector(element, _) => {
                        (element.as_ref().clone(), Type::Array(element.clone()))
                    }
                    Type::List(element) => (element.as_ref().clone(), Type::List(element.clone())),
                    Type::Unknown => (Type::Unknown, Type::Unknown),
                    _ => {
                        return Err(self.error(
                            DiagnosticCode::SemanticDestructure,
                            format!(
                                "sequence destructuring assignment requires an Array or List value, found {}",
                                actual.name()
                            ),
                        ));
                    }
                };
                for pattern in patterns {
                    self.validate_destructure_assignment_pattern(pattern, &element_type, targets)?;
                }
                if let Some(name) = rest {
                    self.validate_destructure_assignment_identifier(name, &rest_type, targets)?;
                }
            }
            MatchPattern::Literal(_)
            | MatchPattern::Range { .. }
            | MatchPattern::Or(_)
            | MatchPattern::Hash(_)
            | MatchPattern::Alias { .. }
            | MatchPattern::EnumVariant { .. }
            | MatchPattern::Struct { .. }
            | MatchPattern::NamedStruct { .. } => {
                return Err(self.error(
                    DiagnosticCode::SemanticDestructure,
                    "destructuring assignment targets support only identifiers, `_`, tuples, sequences, and rest bindings",
                ));
            }
        }
        Ok(())
    }

    pub(super) fn validate_destructure_assignment_identifier(
        &self,
        name: &str,
        actual: &Type,
        targets: &mut Vec<String>,
    ) -> Result<(), SimplyError> {
        let expected = self.variables.get(name).ok_or_else(|| {
            self.error(
                DiagnosticCode::UndefinedVariable,
                format!("cannot destructure-assign unknown variable `{name}`"),
            )
        })?;
        if self.is_snapshot_capture(name) || !self.variables.is_mutable(name) {
            return Err(self.error(
                DiagnosticCode::InvalidReassignment,
                format!("cannot reassign immutable variable `{name}`; declare it with `mut`"),
            ));
        }
        if targets.iter().any(|target| target == name) {
            return Err(self.error(
                DiagnosticCode::SemanticDestructure,
                format!("duplicate destructuring assignment target `{name}`"),
            ));
        }
        self.require_type(expected, actual)?;
        targets.push(name.to_owned());
        Ok(())
    }

    pub(super) fn validate_declared_type(&self, typ: &Type) -> Result<(), SimplyError> {
        match typ {
            Type::Struct(identity)
                if !identity.is_unresolved()
                    && !self
                        .struct_identities
                        .values()
                        .any(|known| known == identity) =>
            {
                Err(self.error(
                    DiagnosticCode::UndefinedVariable,
                    format!("unknown type `{}`", identity.local_name),
                ))
            }
            Type::Struct(identity) if !self.structs.contains_key(&identity.local_name) => Err(self
                .error(
                    DiagnosticCode::UndefinedVariable,
                    format!("unknown type `{}`", identity.local_name),
                )),
            Type::Enum(identity)
                if !identity.is_unresolved()
                    && !self.enum_identities.values().any(|known| known == identity) =>
            {
                Err(self.error(
                    DiagnosticCode::UndefinedVariable,
                    format!("unknown enum type `{}`", identity.local_name),
                ))
            }
            Type::Enum(identity) if !self.enums.contains_key(&identity.local_name) => Err(self
                .error(
                    DiagnosticCode::UndefinedVariable,
                    format!("unknown enum type `{}`", identity.local_name),
                )),
            Type::Array(element)
            | Type::List(element)
            | Type::Vector(element, _)
            | Type::TypedMatrix(element, _, _) => self.validate_declared_type(element),
            Type::Tuple(elements) => {
                for element in elements {
                    self.validate_declared_type(element)?;
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    pub(super) fn define_variable(
        &mut self,
        name: String,
        typ: Type,
        mutable: bool,
    ) -> Result<(), SimplyError> {
        let span = self.current_span.clone().unwrap_or_else(|| Span::new(0, 0));
        self.variables.define_at(name, typ, mutable, span)
    }

    pub(super) fn validate_destructure_pattern(
        &self,
        pattern: &MatchPattern,
        expected: &Type,
        bindings: &mut Vec<(String, Type)>,
    ) -> Result<(), SimplyError> {
        match pattern {
            MatchPattern::Identifier(name) => bindings.push((name.clone(), expected.clone())),
            MatchPattern::Wildcard => {}
            MatchPattern::Tuple(patterns) => match expected {
                Type::Tuple(types) if types.len() == patterns.len() => {
                    for (pattern, typ) in patterns.iter().zip(types) {
                        self.validate_destructure_pattern(pattern, typ, bindings)?;
                    }
                }
                Type::Tuple(types) => {
                    return Err(self.error(
                        DiagnosticCode::SemanticDestructure,
                        format!(
                            "tuple has {} values, but destructuring target has {} elements",
                            types.len(),
                            patterns.len()
                        ),
                    ));
                }
                Type::Unknown => {
                    for pattern in patterns {
                        self.validate_destructure_pattern(pattern, &Type::Unknown, bindings)?;
                    }
                }
                _ => {
                    return Err(self.error(
                        DiagnosticCode::SemanticDestructure,
                        "tuple destructuring requires a tuple value",
                    ));
                }
            },
            MatchPattern::Sequence { patterns, rest } => {
                let (element_type, rest_type) = match expected {
                    Type::Array(element) | Type::Vector(element, _) => {
                        (element.as_ref().clone(), Type::Array(element.clone()))
                    }
                    Type::List(element) => (element.as_ref().clone(), Type::List(element.clone())),
                    Type::Range => (Type::Int, Type::Range),
                    Type::Unknown => (Type::Unknown, Type::Unknown),
                    _ => {
                        return Err(self.error(
                            DiagnosticCode::SemanticDestructure,
                            format!(
                                "sequence destructuring requires an Array or List value, found {}",
                                expected.name()
                            ),
                        ));
                    }
                };
                for pattern in patterns {
                    self.validate_destructure_pattern(pattern, &element_type, bindings)?;
                }
                if let Some(name) = rest {
                    bindings.push((name.clone(), rest_type));
                }
            }
            MatchPattern::Literal(_)
            | MatchPattern::Range { .. }
            | MatchPattern::Or(_)
            | MatchPattern::Hash(_)
            | MatchPattern::Alias { .. }
            | MatchPattern::EnumVariant { .. }
            | MatchPattern::Struct { .. }
            | MatchPattern::NamedStruct { .. } => {
                return Err(self.error(
                    DiagnosticCode::SemanticDestructure,
                    "destructuring targets support only identifiers, `_`, tuples, sequences, and rest bindings",
                ));
            }
        }
        Ok(())
    }

    pub(super) fn collect_functions(&mut self, statements: &[Stmt]) -> Result<(), SimplyError> {
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
                let borrowed_capture = if self
                    .ref_parameter_scopes
                    .iter()
                    .any(|scope| !scope.is_empty())
                {
                    let captured_dependencies = closure_dependencies(body, parameters);
                    self.ref_parameter_scopes
                        .iter()
                        .flat_map(|scope| scope.iter())
                        .filter(|name| captured_dependencies.contains(name.as_str()))
                        .min()
                } else {
                    None
                };
                if let Some(captured_name) = borrowed_capture {
                    return Err(self.error(
                        DiagnosticCode::InvalidRefUsage,
                        format!("a closure cannot capture `ref` parameter `{captured_name}`"),
                    ));
                }
                if self.structs.contains_key(name) || self.enums.contains_key(name) {
                    return Err(self.error(
                        DiagnosticCode::DuplicateDeclaration,
                        format!("function `{name}` conflicts with a struct type"),
                    ));
                }
                for (_, parameter_type, _, by_ref) in parameters {
                    if let Some(parameter_type) = parameter_type {
                        self.validate_declared_type(parameter_type)?;
                    }
                    if *by_ref
                        && !parameter_type.as_ref().is_some_and(|typ| {
                            matches!(
                                typ,
                                Type::Array(_)
                                    | Type::List(_)
                                    | Type::Hash
                                    | Type::HashValues(_)
                                    | Type::Vector(_, _)
                            )
                        })
                    {
                        return Err(self.error(
                            DiagnosticCode::InvalidRefUsage,
                            "a `ref` parameter must be an Array, List, or Hash",
                        ));
                    }
                    if !*by_ref
                        && parameter_type
                            .as_ref()
                            .is_some_and(Self::is_mutable_collection_type)
                    {
                        return Err(self.error(
                            DiagnosticCode::InvalidRefUsage,
                            "collection parameters must be declared `ref`",
                        ));
                    }
                }
                if let Some(return_type) = return_type {
                    self.validate_declared_type(return_type)?;
                }
                if self
                    .function_scopes
                    .last()
                    .is_some_and(|scope| scope.contains_key(name))
                {
                    return Err(self.error(
                        DiagnosticCode::DuplicateDeclaration,
                        format!("function `{name}` is already declared in this scope"),
                    ));
                }
                let resolved_parameters: Vec<Option<Type>> = parameters
                    .iter()
                    .map(|(_, parameter_type, _, _)| {
                        parameter_type
                            .as_ref()
                            .map(|typ| self.resolve_type_identity(typ))
                    })
                    .collect();
                let resolved_return_type = return_type
                    .as_ref()
                    .map(|typ| self.resolve_type_identity(typ));
                self.function_scopes
                    .last_mut()
                    .expect("semantic function scope has a global frame")
                    .insert(
                        name.clone(),
                        FunctionSignature {
                            parameters: resolved_parameters.clone(),
                            ref_parameters: parameters
                                .iter()
                                .map(|(_, _, _, by_ref)| *by_ref)
                                .collect(),
                            return_type: resolved_return_type.clone(),
                        },
                    );
                self.variables.define(
                    name.clone(),
                    Type::Function {
                        parameters: resolved_parameters
                            .into_iter()
                            .map(|typ| typ.map(Box::new))
                            .collect(),
                        return_type: resolved_return_type.map(Box::new),
                    },
                    false,
                )?;
            }
            self.current_span = previous_span;
        }
        Ok(())
    }
}
