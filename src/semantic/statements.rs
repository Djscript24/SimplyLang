use super::*;

impl SemanticAnalyzer {
    pub(super) fn expression_contains_borrowed_collection(
        &self,
        expression: &Expr,
        value_type: &Type,
        scopes: &[HashSet<String>],
    ) -> Result<bool, SimplyError> {
        if !Self::type_may_contain_collection(value_type) {
            return Ok(false);
        }
        match expression {
            Expr::Identifier(name) => Ok(self.variables.is_reference(name)
                || scopes.iter().any(|scope| scope.contains(name))),
            Expr::Array(values) | Expr::List(values) => {
                let element_type = match value_type {
                    Type::Array(element) | Type::List(element) => element.as_ref(),
                    _ => return Ok(self.expression_contains_ref_parameter(expression, scopes)),
                };
                for value in values {
                    if self.expression_contains_borrowed_collection(value, element_type, scopes)? {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
            Expr::Tuple(values) => {
                let Type::Tuple(element_types) = value_type else {
                    return Ok(self.expression_contains_ref_parameter(expression, scopes));
                };
                for (value, element_type) in values.iter().zip(element_types) {
                    if self.expression_contains_borrowed_collection(value, element_type, scopes)? {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
            Expr::Matrix(rows) => {
                let element_type = match value_type {
                    Type::Matrix => Type::Unknown,
                    Type::TypedMatrix(element, _, _) => element.as_ref().clone(),
                    _ => return Ok(self.expression_contains_ref_parameter(expression, scopes)),
                };
                for row in rows {
                    let row_type = Type::Array(Box::new(element_type.clone()));
                    if self.expression_contains_borrowed_collection(row, &row_type, scopes)? {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
            Expr::Hash(entries) => {
                let element_type = match value_type {
                    Type::HashValues(element) => element.as_ref().clone(),
                    Type::Hash => Type::Unknown,
                    _ => return Ok(self.expression_contains_ref_parameter(expression, scopes)),
                };
                for (_, value) in entries {
                    if self.expression_contains_borrowed_collection(value, &element_type, scopes)? {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
            Expr::Index { target, .. } | Expr::Field { target, .. } => {
                Ok(self.expression_contains_ref_parameter(target, scopes))
            }
            Expr::Call { name, .. }
                if matches!(
                    name.as_str(),
                    "cross"
                        | "entries"
                        | "enumerate"
                        | "identity"
                        | "keys"
                        | "least_squares"
                        | "matvec"
                        | "matrix_add"
                        | "matrix_scale"
                        | "matrix_subtract"
                        | "multiply"
                        | "reverse"
                        | "select_keys"
                        | "values"
                        | "vector_add"
                        | "vector_scale"
                        | "vector_subtract"
                        | "without_key"
                        | "zip"
                ) && !self
                    .function_scopes
                    .iter()
                    .rev()
                    .any(|scope| scope.contains_key(name))
                    && !matches!(self.variables.get(name), Some(Type::Function { .. }))
                    && !Self::collection_may_contain_nested_collection(value_type) =>
            {
                Ok(false)
            }
            _ => Ok(self.expression_contains_ref_parameter(expression, scopes)),
        }
    }

    fn collection_may_contain_nested_collection(value_type: &Type) -> bool {
        match value_type {
            Type::Array(element) | Type::List(element) | Type::Vector(element, _) => {
                Self::type_may_contain_collection(element)
            }
            Type::HashValues(element) => Self::type_may_contain_collection(element),
            Type::Hash | Type::Struct(_) | Type::Enum(_) | Type::Unknown => true,
            Type::Tuple(elements) => elements.iter().any(Self::type_may_contain_collection),
            _ => false,
        }
    }

    pub(super) fn reject_borrow_escape(
        &self,
        expression: &Expr,
        value_type: &Type,
        destination: &str,
    ) -> Result<(), SimplyError> {
        if self.expression_contains_borrowed_collection(
            expression,
            value_type,
            &self.ref_parameter_scopes,
        )? {
            let borrowed_from = if destination == "a match result" {
                "a `ref` match binding"
            } else {
                "a `ref` parameter"
            };
            return Err(self.error(
                DiagnosticCode::InvalidRefUsage,
                format!("{borrowed_from} cannot escape through {destination}"),
            ));
        }
        Ok(())
    }

    pub(super) fn is_ref_borrowed(&self, name: &str) -> bool {
        self.variables.is_reference(name)
            || self
                .ref_parameter_scopes
                .iter()
                .any(|scope| scope.contains(name))
    }

    fn is_mut_ref_borrowed(&self, name: &str) -> bool {
        self.variables.is_exclusive_reference(name)
            || self
                .mut_ref_parameter_scopes
                .iter()
                .any(|scope| scope.contains(name))
    }

    pub(super) fn type_may_contain_collection(value_type: &Type) -> bool {
        match value_type {
            Type::Array(_)
            | Type::List(_)
            | Type::Hash
            | Type::HashValues(_)
            | Type::Vector(_, _) => true,
            Type::Tuple(elements) => elements.iter().any(Self::type_may_contain_collection),
            Type::Struct(_) | Type::Enum(_) | Type::Unknown => true,
            _ => false,
        }
    }

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

    pub(super) fn expression_contains_ref_parameter(
        &self,
        expression: &Expr,
        scopes: &[HashSet<String>],
    ) -> bool {
        match expression {
            Expr::Identifier(name) => {
                self.variables.is_reference(name) || scopes.iter().any(|scope| scope.contains(name))
            }
            Expr::Array(values)
            | Expr::List(values)
            | Expr::Tuple(values)
            | Expr::Matrix(values) => values
                .iter()
                .any(|value| self.expression_contains_ref_parameter(value, scopes)),
            Expr::Hash(entries) => entries
                .iter()
                .any(|(_, value)| self.expression_contains_ref_parameter(value, scopes)),
            Expr::Unary { operand, .. } => self.expression_contains_ref_parameter(operand, scopes),
            Expr::Binary { left, right, .. } => {
                self.expression_contains_ref_parameter(left, scopes)
                    || self.expression_contains_ref_parameter(right, scopes)
            }
            Expr::EnumVariant { arguments, .. } => arguments
                .iter()
                .any(|argument| self.expression_contains_ref_parameter(argument, scopes)),
            Expr::Call { name, arguments } => {
                let user_function = self
                    .function_scopes
                    .iter()
                    .rev()
                    .any(|scope| scope.contains_key(name))
                    || matches!(self.variables.get(name), Some(Type::Function { .. }));
                let returns_independent_collection =
                    matches!(name.as_str(), "least_squares" | "matvec" | "cross" | "keys");
                let safe_builtin = !user_function && returns_independent_collection;
                !safe_builtin
                    && arguments
                        .iter()
                        .any(|argument| self.expression_contains_ref_parameter(argument, scopes))
            }
            Expr::MessageDispatch {
                receiver,
                arguments,
                ..
            } => {
                self.expression_contains_ref_parameter(receiver, scopes)
                    || arguments
                        .iter()
                        .any(|argument| self.expression_contains_ref_parameter(argument, scopes))
            }
            Expr::Index { target, index } => {
                self.expression_contains_ref_parameter(target, scopes)
                    || self.expression_contains_ref_parameter(index, scopes)
            }
            Expr::Field { target, .. } => self.expression_contains_ref_parameter(target, scopes),
            Expr::Pipeline { source, steps } => {
                self.expression_contains_ref_parameter(source, scopes)
                    || steps.iter().any(|step| match step {
                        PipelineStep::Where(expression)
                        | PipelineStep::Derive(expression)
                        | PipelineStep::TakeWhile(expression)
                        | PipelineStep::DropWhile(expression)
                        | PipelineStep::WriteCsv(expression) => {
                            self.expression_contains_ref_parameter(expression, scopes)
                        }
                        PipelineStep::Partition { rules, .. } => rules.iter().any(|rule| {
                            rule.condition.as_ref().is_some_and(|condition| {
                                self.expression_contains_ref_parameter(condition, scopes)
                            })
                        }),
                        _ => false,
                    })
            }
            Expr::Match { value, arms } => {
                self.expression_contains_ref_parameter(value, scopes)
                    || arms.iter().any(|arm| {
                        arm.guard.as_ref().is_some_and(|guard| {
                            self.expression_contains_ref_parameter(guard, scopes)
                        }) || arm.result.as_ref().is_some_and(|result| {
                            self.expression_contains_ref_parameter(result, scopes)
                        })
                    })
            }
            Expr::Literal(_) => false,
        }
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
                let ref_names = parameters
                    .iter()
                    .filter(|(_, _, _, by_ref)| *by_ref)
                    .map(|(parameter, _, _, _)| parameter.clone())
                    .collect();
                self.ref_parameter_scopes.push(ref_names);
                for (parameter, parameter_type, mutable, by_ref) in parameters {
                    self.define_variable(
                        parameter.clone(),
                        parameter_type
                            .as_ref()
                            .map(|typ| self.resolve_type_identity(typ))
                            .unwrap_or(Type::Unknown),
                        *mutable && !*by_ref,
                    )?;
                }
                let body_result = self
                    .collect_functions(body)
                    .and_then(|()| self.analyze_statements(body));
                let inferred_return = self.inferred_return.clone();
                self.ref_parameter_scopes.pop();
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
                let vector_provenance =
                    self.expression_vector_provenance(value, &actual, &expected);
                self.reject_borrow_escape(value, &expected, "a variable binding")?;
                if matches!(expected, Type::Matrix | Type::TypedMatrix(_, _, _)) {
                    self.require_rectangular_matrix_literal(value)?;
                }
                self.require_type(&expected, &actual)?;
                self.define_variable(name.clone(), expected, *mutable)?;
                self.variables
                    .set_vector_provenance(name, vector_provenance);
            }
            Stmt::Borrow {
                name,
                mutable,
                value,
            } => {
                let actual = self.analyze_expression(value)?;
                let mut root = value;
                let mut element_borrow = false;
                while let Expr::Index { target, .. } = root {
                    element_borrow = true;
                    root = target;
                }
                let Expr::Identifier(source) = root else {
                    return Err(self.error(
                        DiagnosticCode::InvalidRefUsage,
                        "a local `ref` binding must target a named collection or a nested location within a collection or Struct",
                    ));
                };
                let owner_type = self.analyze_expression(root)?;
                let owner_is_struct = matches!(&owner_type, Type::Struct(_));
                if !Self::is_mutable_collection_type(&owner_type)
                    && !(element_borrow && owner_is_struct)
                {
                    return Err(self.error(
                        DiagnosticCode::InvalidRefUsage,
                        format!(
                            "a local `ref` binding requires an Array, List, Hash, Vector, or a field of a Struct, found {}",
                            owner_type.name()
                        ),
                    ));
                }
                let mut path_expressions = Vec::new();
                let mut path_root = value;
                while let Expr::Index { target, index } = path_root {
                    path_expressions.push(index.as_ref());
                    path_root = target;
                }
                path_expressions.reverse();
                let mut current_type = owner_type.clone();
                let mut contains_struct_field = false;
                for index in path_expressions {
                    contains_struct_field |= matches!(&current_type, Type::Struct(_));
                    let index_type = self.analyze_expression(index)?;
                    current_type = self.index_type(&current_type, &index_type, index)?;
                }
                let supported_type = if element_borrow {
                    matches!(&actual, Type::Int | Type::Float | Type::Bool | Type::String)
                        || (contains_struct_field && Self::is_mutable_collection_type(&actual))
                } else {
                    Self::is_mutable_collection_type(&actual)
                };
                if !supported_type {
                    return Err(self.error(
                        DiagnosticCode::InvalidRefUsage,
                        if element_borrow {
                            format!(
                                "element `ref` bindings support scalar values and Struct fields containing collections, found {}",
                                actual.name()
                            )
                        } else {
                            format!(
                                "a local `ref` binding requires an Array, List, Hash, or Vector, found {}",
                                actual.name()
                            )
                        },
                    ));
                }
                if *mutable && !self.variables.is_mutable(source) {
                    return Err(self.error(
                        DiagnosticCode::InvalidRefUsage,
                        format!("mutable `ref` binding `{name}` requires a `mut` owner"),
                    ));
                }
                if *mutable && self.is_ref_borrowed(source) && !self.is_mut_ref_borrowed(source) {
                    return Err(self.error(
                        DiagnosticCode::InvalidRefUsage,
                        "cannot create a mutable `ref` from a shared borrow",
                    ));
                }
                self.define_variable(name.clone(), actual, *mutable)?;
                self.variables.mark_reference(name, *mutable);
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
                self.reject_borrow_escape(source, &actual, "a flow binding")?;
                self.define_variable(name.clone(), actual, false)?;
            }
            Stmt::Reassign { name, value } => {
                let expected = self.variables.get(name).cloned().ok_or_else(|| {
                    self.error(
                        DiagnosticCode::UndefinedVariable,
                        format!("unknown variable `{name}`"),
                    )
                })?;
                let mutable_element_reference = self.variables.is_exclusive_reference(name)
                    && matches!(
                        &expected,
                        Type::Int | Type::Float | Type::Bool | Type::String
                    );
                if self.is_ref_borrowed(name) && !mutable_element_reference {
                    let message = if self.is_mut_ref_borrowed(name) {
                        format!("`mut ref` parameter binding `{name}` cannot be reassigned")
                    } else {
                        format!("`ref` value `{name}` is read-only and cannot be reassigned")
                    };
                    return Err(self.error(DiagnosticCode::InvalidRefUsage, message));
                }
                if self.is_snapshot_capture(name) || !self.variables.is_mutable(name) {
                    return Err(self.error(
                        DiagnosticCode::InvalidReassignment,
                        format!(
                            "cannot reassign immutable variable `{name}`; declare it with `mut`"
                        ),
                    ));
                }
                let actual = self.analyze_expression(value)?;
                let vector_provenance =
                    self.expression_vector_provenance(value, &actual, &expected);
                self.reject_borrow_escape(value, &expected, "a variable assignment")?;
                self.require_type(&expected, &actual)?;
                self.variables
                    .set_vector_provenance(name, vector_provenance);
            }
            Stmt::DestructureReassign { pattern, value } => {
                let actual = self.analyze_expression(value)?;
                self.reject_borrow_escape(value, &actual, "a destructuring assignment")?;
                let mut targets = Vec::new();
                self.validate_destructure_assignment_pattern(pattern, &actual, &mut targets)?;
                for target in targets {
                    if matches!(self.variables.get(&target), Some(Type::Vector(_, _))) {
                        self.variables
                            .set_vector_provenance(&target, VectorProvenance::Unknown);
                    }
                }
            }
            Stmt::SetIndex {
                name,
                indices,
                value,
            } => {
                if self.is_ref_borrowed(name) && !self.is_mut_ref_borrowed(name) {
                    return Err(self.error(
                        DiagnosticCode::InvalidRefUsage,
                        format!("`ref` value `{name}` is read-only and cannot be mutated"),
                    ));
                }
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
                let mut current_type = target;
                for index in indices {
                    let index_type = self.analyze_expression(index)?;
                    current_type = match current_type {
                        Type::Array(element) | Type::List(element) | Type::Vector(element, _) => {
                            self.require_type(&Type::Int, &index_type)?;
                            *element
                        }
                        Type::Hash => {
                            self.require_type(&Type::String, &index_type)?;
                            Type::Unknown
                        }
                        Type::HashValues(element) => {
                            self.require_type(&Type::String, &index_type)?;
                            *element
                        }
                        Type::Unknown => Type::Unknown,
                        _ => {
                            return Err(self.error(
                                DiagnosticCode::SemanticCollectionOperation,
                                format!("`{name}` is not mutable"),
                            ));
                        }
                    };
                }
                let value_type = self.analyze_expression(value)?;
                self.reject_borrow_escape(value, &value_type, "a collection mutation")?;
                self.require_type(&current_type, &value_type)?;
            }
            Stmt::Destructure {
                pattern,
                mutable,
                value,
            } => {
                let value_type = self.analyze_expression(value)?;
                self.reject_borrow_escape(value, &value_type, "a destructuring binding")?;
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
                operation,
                value,
            } => {
                if self.is_ref_borrowed(name) && !self.is_mut_ref_borrowed(name) {
                    return Err(self.error(
                        DiagnosticCode::InvalidRefUsage,
                        format!("`ref` value `{name}` is read-only and cannot be mutated"),
                    ));
                }
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
                if *operation == CollectionOperation::Add {
                    self.reject_borrow_escape(value, &value_type, "a collection mutation")?;
                }
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
                if captures_outer
                    && let Some(captured_name) = closure_dependencies(body, parameters)
                        .iter()
                        .filter(|captured_name| self.variables.is_reference(captured_name))
                        .min()
                {
                    return Err(self.error(
                        DiagnosticCode::InvalidRefUsage,
                        format!(
                            "a closure cannot capture a `ref` parameter or local reference binding `{captured_name}`"
                        ),
                    ));
                }
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
                let ref_names = parameters
                    .iter()
                    .filter(|(_, _, _, by_ref)| *by_ref)
                    .map(|(parameter, _, _, _)| parameter.clone())
                    .collect();
                let mut_ref_names = parameters
                    .iter()
                    .filter(|(_, _, mutable, by_ref)| *mutable && *by_ref)
                    .map(|(parameter, _, _, _)| parameter.clone())
                    .collect();
                self.ref_parameter_scopes.push(ref_names);
                self.mut_ref_parameter_scopes.push(mut_ref_names);
                for (parameter, parameter_type, mutable, _by_ref) in parameters {
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
                    self.ref_parameter_scopes.pop();
                    self.mut_ref_parameter_scopes.pop();
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
                    self.ref_parameter_scopes.pop();
                    self.mut_ref_parameter_scopes.pop();
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
                self.ref_parameter_scopes.pop();
                self.mut_ref_parameter_scopes.pop();
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
                let escape_type = self
                    .function_return
                    .clone()
                    .unwrap_or_else(|| actual.clone());
                self.reject_borrow_escape(expression, &escape_type, "a return value")?;
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
                let provenance_before = self.variables.snapshot_vector_provenance();
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
                let then_provenance = self.variables.snapshot_vector_provenance();
                self.variables.restore_vector_provenance(&provenance_before);
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
                let else_provenance = self.variables.snapshot_vector_provenance();
                match condition {
                    Expr::Literal(Literal::Bool(true)) => {
                        self.variables.restore_vector_provenance(&then_provenance)
                    }
                    Expr::Literal(Literal::Bool(false)) => {
                        self.variables.restore_vector_provenance(&else_provenance)
                    }
                    _ => self
                        .variables
                        .merge_vector_provenance(&[then_provenance, else_provenance]),
                }
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
                by_ref,
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
                if *by_ref
                    && (!matches!(iterable, Expr::Identifier(_))
                        || !matches!(
                            iterable_type,
                            Type::Array(_)
                                | Type::List(_)
                                | Type::Vector(_, _)
                                | Type::Hash
                                | Type::HashValues(_)
                        ))
                {
                    return Err(self.error(
                        DiagnosticCode::InvalidRefUsage,
                        "`for ref` requires a named Array, List, Vector, or Hash",
                    ));
                }
                let frame_start = self.variables.scopes.len();
                let saved_saw_return = self.saw_return;
                let provenance_before = self.variables.snapshot_vector_provenance();
                self.variables.push();
                self.function_scopes.push(HashMap::new());
                let span = self.current_span.clone().unwrap_or_else(|| Span::new(0, 0));
                self.variables
                    .insert_at(name.clone(), element, *mutable, span)?;
                if *by_ref {
                    self.variables.mark_reference(name, false);
                }
                let borrowed_element = Self::type_may_contain_collection(
                    self.variables.get(name).unwrap_or(&Type::Unknown),
                ) && !self.ref_parameter_scopes.is_empty()
                    && self.expression_contains_ref_parameter(iterable, &self.ref_parameter_scopes);
                if borrowed_element {
                    self.ref_parameter_scopes
                        .push(HashSet::from([name.clone()]));
                }
                self.loop_depth += 1;
                let body_result = self
                    .collect_functions(body)
                    .and_then(|()| self.analyze_statements(body));
                if let Err(error) = body_result {
                    self.loop_depth -= 1;
                    if borrowed_element {
                        self.ref_parameter_scopes.pop();
                    }
                    self.variables.truncate(frame_start);
                    self.function_scopes.pop();
                    self.saw_return = saved_saw_return;
                    return Err(error);
                }
                self.loop_depth -= 1;
                if borrowed_element {
                    self.ref_parameter_scopes.pop();
                }
                self.variables.truncate(frame_start);
                self.function_scopes.pop();
                let provenance_after_body = self.variables.snapshot_vector_provenance();
                self.variables
                    .merge_vector_provenance(&[provenance_before, provenance_after_body]);
                self.saw_return = saved_saw_return;
            }
            Stmt::While { condition, body } => {
                let condition_type = self.analyze_expression(condition)?;
                self.require_type(&Type::Bool, &condition_type)?;
                let saved_saw_return = self.saw_return;
                let provenance_before = self.variables.snapshot_vector_provenance();
                self.loop_depth += 1;
                let result = self
                    .collect_functions(body)
                    .and_then(|()| self.analyze_statements(body));
                self.loop_depth -= 1;
                self.saw_return = saved_saw_return;
                result?;
                let provenance_after_body = self.variables.snapshot_vector_provenance();
                self.variables
                    .merge_vector_provenance(&[provenance_before, provenance_after_body]);
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
