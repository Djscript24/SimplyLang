use super::*;

impl SemanticAnalyzer {
    pub(super) fn match_type(
        &mut self,
        value: &Expr,
        arms: &[crate::ast::MatchArm],
    ) -> Result<Type, SimplyError> {
        let value_type = self.analyze_expression(value)?;
        let mut coverage_matrix = Vec::<Vec<MatchPattern>>::new();
        let mut result_type: Option<Type> = None;

        for arm in arms {
            let mut bindings = Vec::new();
            self.validate_match_pattern(&arm.pattern, &value_type, &mut bindings)?;
            if !self.pattern_is_useful(&coverage_matrix, &arm.pattern, &value_type) {
                return Err(
                    self.error(DiagnosticCode::UnexpectedToken, "unreachable match pattern")
                );
            }
            if arm.guard.is_none() {
                coverage_matrix.push(vec![arm.pattern.clone()]);
            }

            let frame_start = self.variables.scopes.len();
            self.variables.push();
            self.function_scopes.push(HashMap::new());
            let saved_saw_return = self.saw_return;
            let saved_inferred_return = self.inferred_return.clone();
            let saved_module_return_type = self.module_return_type.clone();
            let saved_loop_depth = self.loop_depth;
            self.saw_return = false;
            self.loop_depth = 0;
            let mut pushed_ref_scope = false;
            let branch_result = (|| {
                let carries_borrow = !self.ref_parameter_scopes.is_empty()
                    && self.expression_contains_ref_parameter(value, &self.ref_parameter_scopes);
                let mut borrowed_bindings = HashSet::new();
                for (binding, binding_type) in bindings {
                    if carries_borrow && Self::type_may_contain_collection(&binding_type) {
                        borrowed_bindings.insert(binding.clone());
                    }
                    self.define_variable(binding, binding_type, false)?;
                }
                if !borrowed_bindings.is_empty() {
                    self.ref_parameter_scopes.push(borrowed_bindings);
                    pushed_ref_scope = true;
                }
                if let Some(guard) = &arm.guard {
                    let guard_type = self.analyze_expression(guard)?;
                    self.require_type(&Type::Bool, &guard_type)?;
                }
                self.collect_functions(&arm.body)?;
                self.analyze_statements(&arm.body)?;
                if self.saw_return {
                    Ok(if self.function_depth == 0 {
                        self.module_return_type.clone()
                    } else {
                        self.inferred_return.clone()
                    }
                    .unwrap_or(Type::Unit))
                } else {
                    arm.result
                        .as_ref()
                        .map(|result| self.analyze_expression(result))
                        .unwrap_or(Ok(Type::Unit))
                }
            })();
            if pushed_ref_scope {
                self.ref_parameter_scopes.pop();
            }
            self.variables.truncate(frame_start);
            self.function_scopes.pop();
            self.loop_depth = saved_loop_depth;
            self.saw_return = saved_saw_return;
            self.inferred_return = saved_inferred_return;
            self.module_return_type = saved_module_return_type;
            let branch_type = branch_result?;
            match &result_type {
                None => result_type = Some(branch_type),
                Some(expected) if *expected == Type::Unknown => result_type = Some(branch_type),
                Some(_) if branch_type == Type::Unknown => {}
                Some(expected) => {
                    result_type = Some(
                        Self::merge_compatible_types(expected, &branch_type)
                            .ok_or_else(|| self.type_error(expected, &branch_type, "match arm"))?,
                    );
                }
            }
        }

        if self.pattern_is_useful(&coverage_matrix, &MatchPattern::Wildcard, &value_type) {
            if let Type::Enum(enum_identity) = &value_type
                && let Some(enum_name) = self.enum_name_for_identity(enum_identity)
                && let Some(variants) = self.enums.get(enum_name)
            {
                for variant in variants {
                    let pattern = MatchPattern::EnumVariant {
                        enum_name: enum_name.clone(),
                        variant_name: variant.name.clone(),
                        payload: variant
                            .payload_type
                            .as_ref()
                            .map(|_| Box::new(MatchPattern::Wildcard)),
                    };
                    if self.pattern_is_useful(&coverage_matrix, &pattern, &value_type) {
                        let constructor = CoverageConstructor::Enum {
                            type_identity: enum_identity.clone(),
                            variant: variant.name.clone(),
                        };
                        let has_variant_arm = coverage_matrix.iter().any(|row| {
                            row.first()
                                .and_then(|pattern| self.pattern_constructor(pattern))
                                .is_some_and(|(found, _)| found == constructor)
                        });
                        if !has_variant_arm {
                            return Err(self.error(
                                DiagnosticCode::UnexpectedToken,
                                format!("missing variant `{}::{}`", enum_name, variant.name),
                            ));
                        }
                        break;
                    }
                }
            }
            return Err(self.error(
                DiagnosticCode::TypeMismatch,
                format!(
                    "non-exhaustive match: value of type {} is not fully covered",
                    value_type.name()
                ),
            ));
        }

        Ok(result_type.unwrap_or(Type::Unit))
    }

    pub(super) fn pattern_is_useful(
        &self,
        matrix: &[Vec<MatchPattern>],
        pattern: &MatchPattern,
        typ: &Type,
    ) -> bool {
        self.pattern_vector_is_useful(
            matrix,
            std::slice::from_ref(pattern),
            std::slice::from_ref(typ),
        )
    }

    pub(super) fn pattern_contains_alias(pattern: &MatchPattern) -> bool {
        match pattern {
            MatchPattern::Alias { .. } => true,
            MatchPattern::Or(patterns)
            | MatchPattern::Tuple(patterns)
            | MatchPattern::Struct {
                fields: patterns, ..
            } => patterns.iter().any(Self::pattern_contains_alias),
            MatchPattern::NamedStruct { fields, .. } => fields
                .iter()
                .any(|(_, pattern)| Self::pattern_contains_alias(pattern)),
            MatchPattern::Sequence { patterns, .. } => {
                patterns.iter().any(Self::pattern_contains_alias)
            }
            MatchPattern::Hash(entries) => entries
                .iter()
                .any(|(_, pattern)| Self::pattern_contains_alias(pattern)),
            MatchPattern::EnumVariant {
                payload: Some(pattern),
                ..
            } => Self::pattern_contains_alias(pattern),
            MatchPattern::Identifier(_)
            | MatchPattern::Literal(_)
            | MatchPattern::Range { .. }
            | MatchPattern::EnumVariant { payload: None, .. }
            | MatchPattern::Wildcard => false,
        }
    }

    pub(super) fn without_aliases(pattern: &MatchPattern) -> MatchPattern {
        match pattern {
            MatchPattern::Alias { pattern, .. } => Self::without_aliases(pattern),
            MatchPattern::Or(patterns) => {
                MatchPattern::Or(patterns.iter().map(Self::without_aliases).collect())
            }
            MatchPattern::Tuple(patterns) => {
                MatchPattern::Tuple(patterns.iter().map(Self::without_aliases).collect())
            }
            MatchPattern::Sequence { patterns, rest } => MatchPattern::Sequence {
                patterns: patterns.iter().map(Self::without_aliases).collect(),
                rest: rest.clone(),
            },
            MatchPattern::Hash(entries) => MatchPattern::Hash(
                entries
                    .iter()
                    .map(|(key, pattern)| (key.clone(), Self::without_aliases(pattern)))
                    .collect(),
            ),
            MatchPattern::EnumVariant {
                enum_name,
                variant_name,
                payload,
            } => MatchPattern::EnumVariant {
                enum_name: enum_name.clone(),
                variant_name: variant_name.clone(),
                payload: payload
                    .as_ref()
                    .map(|pattern| Box::new(Self::without_aliases(pattern))),
            },
            MatchPattern::Struct { type_name, fields } => MatchPattern::Struct {
                type_name: type_name.clone(),
                fields: fields.iter().map(Self::without_aliases).collect(),
            },
            MatchPattern::NamedStruct { type_name, fields } => MatchPattern::NamedStruct {
                type_name: type_name.clone(),
                fields: fields
                    .iter()
                    .map(|(name, pattern)| (name.clone(), Self::without_aliases(pattern)))
                    .collect(),
            },
            MatchPattern::Identifier(_)
            | MatchPattern::Literal(_)
            | MatchPattern::Range { .. }
            | MatchPattern::Wildcard => pattern.clone(),
        }
    }

    pub(super) fn pattern_vector_is_useful(
        &self,
        matrix: &[Vec<MatchPattern>],
        candidate: &[MatchPattern],
        types: &[Type],
    ) -> bool {
        if candidate.iter().any(Self::pattern_contains_alias)
            || matrix.iter().flatten().any(Self::pattern_contains_alias)
        {
            let normalized_matrix = matrix
                .iter()
                .map(|row| row.iter().map(Self::without_aliases).collect())
                .collect::<Vec<Vec<_>>>();
            let normalized_candidate = candidate
                .iter()
                .map(Self::without_aliases)
                .collect::<Vec<_>>();
            return self.pattern_vector_is_useful(&normalized_matrix, &normalized_candidate, types);
        }

        if candidate.is_empty() {
            return matrix.is_empty();
        }
        let Some((head, tail)) = candidate.split_first() else {
            return matrix.is_empty();
        };
        let Some(typ) = types.first() else {
            return false;
        };
        let rest_types = &types[1..];

        if let MatchPattern::Or(alternatives) = head {
            let mut alternatives_matrix = matrix.to_vec();
            let mut useful = false;
            for alternative in alternatives {
                let mut alternative_candidate = vec![alternative.clone()];
                alternative_candidate.extend_from_slice(tail);
                useful |= self.pattern_vector_is_useful(
                    &alternatives_matrix,
                    &alternative_candidate,
                    types,
                );
                alternatives_matrix.push(alternative_candidate);
            }
            return useful;
        }

        if typ == &Type::Int {
            let intervals = self.int_pattern_intervals(head);
            if !intervals.is_empty() {
                return self.int_pattern_vector_is_useful(matrix, &intervals, tail, rest_types);
            }
        }
        if typ == &Type::Unknown
            && (!self.int_pattern_intervals(head).is_empty())
            && (matches!(head, MatchPattern::Range { .. })
                || matches!(head, MatchPattern::Literal(Literal::Int(_))))
        {
            return self.int_pattern_vector_is_useful(
                matrix,
                &self.int_pattern_intervals(head),
                tail,
                rest_types,
            );
        }

        if let MatchPattern::Literal(literal) = head
            && (!matches!(literal, Literal::Bool(_)) || !matches!(typ, Type::Bool))
        {
            let specialized = self.specialize_literal_matrix(matrix, literal);
            return self.pattern_vector_is_useful(&specialized, tail, rest_types);
        }

        let sequence_element_type = match typ {
            Type::Array(element) | Type::List(element) | Type::Vector(element, _) => {
                Some((**element).clone())
            }
            Type::CsvStream => Some(Type::List(Box::new(Type::String))),
            Type::Unknown if matches!(head, MatchPattern::Sequence { .. }) => Some(Type::Unknown),
            _ => None,
        };
        if let Some(element_type) = sequence_element_type {
            if let MatchPattern::Sequence { patterns, rest } = head {
                return self.sequence_pattern_vector_is_useful(
                    matrix,
                    patterns,
                    rest.is_some(),
                    tail,
                    rest_types,
                    &element_type,
                );
            }
            if matches!(head, MatchPattern::Wildcard | MatchPattern::Identifier(_)) {
                let max_length = self.max_sequence_prefix_length(matrix, 0);
                return (0..=max_length + 1).any(|length| {
                    let candidate = vec![MatchPattern::Wildcard; length];
                    self.sequence_pattern_vector_is_useful(
                        matrix,
                        &candidate,
                        false,
                        tail,
                        rest_types,
                        &element_type,
                    )
                });
            }
        }

        if matches!(typ, Type::Hash | Type::HashValues(_) | Type::Unknown) {
            if let MatchPattern::Hash(entries) = head {
                let candidate = self.expand_hash_pattern(entries, tail);
                let specialized = matrix
                    .iter()
                    .flat_map(|row| self.expand_hash_row(row, entries))
                    .collect::<Vec<_>>();
                let mut candidate_types = vec![Type::Unknown; entries.len()];
                candidate_types.extend_from_slice(rest_types);
                return self.pattern_vector_is_useful(&specialized, &candidate, &candidate_types);
            }
            if matches!(head, MatchPattern::Wildcard | MatchPattern::Identifier(_))
                && matches!(typ, Type::Hash | Type::HashValues(_))
            {
                let defaults = self.hash_default_matrix(matrix);
                return self.pattern_vector_is_useful(&defaults, tail, rest_types);
            }
        }

        if let Some((constructor, arguments)) = self.pattern_constructor(head) {
            let Some(argument_types) = self.constructor_argument_types(&constructor, typ) else {
                return false;
            };
            let specialized = self.specialize_matrix(matrix, &constructor, arguments.len());
            let mut specialized_candidate = arguments;
            specialized_candidate.extend_from_slice(tail);
            let mut specialized_types = argument_types;
            specialized_types.extend_from_slice(rest_types);
            return self.pattern_vector_is_useful(
                &specialized,
                &specialized_candidate,
                &specialized_types,
            );
        }

        if let Some(constructors) = self.finite_constructors(typ) {
            let complete = constructors.iter().all(|(constructor, _)| {
                matrix.iter().any(|row| {
                    row.first().is_some_and(|pattern| {
                        self.pattern_may_match_constructor(pattern, constructor)
                    })
                })
            });
            if complete {
                for (constructor, argument_types) in constructors {
                    let arity = argument_types.len();
                    let specialized = self.specialize_matrix(matrix, &constructor, arity);
                    let mut specialized_candidate = vec![MatchPattern::Wildcard; arity];
                    specialized_candidate.extend_from_slice(tail);
                    let mut specialized_types = argument_types;
                    specialized_types.extend_from_slice(rest_types);
                    if self.pattern_vector_is_useful(
                        &specialized,
                        &specialized_candidate,
                        &specialized_types,
                    ) {
                        return true;
                    }
                }
                false
            } else {
                let defaults = self.default_matrix(matrix);
                self.pattern_vector_is_useful(&defaults, tail, rest_types)
            }
        } else {
            let defaults = self.default_matrix(matrix);
            self.pattern_vector_is_useful(&defaults, tail, rest_types)
        }
    }

    pub(super) fn int_pattern_intervals(&self, pattern: &MatchPattern) -> Vec<(i128, i128)> {
        match pattern {
            MatchPattern::Alias { pattern, .. } => self.int_pattern_intervals(pattern),
            MatchPattern::Literal(Literal::Int(value)) => {
                let value = i128::from(*value);
                vec![(value, value)]
            }
            MatchPattern::Range { start, end } => {
                let start = match start {
                    Some(Literal::Int(value)) => i128::from(*value),
                    Some(_) => return Vec::new(),
                    None => i128::from(i64::MIN),
                };
                let end = match end {
                    Some(Literal::Int(value)) => i128::from(*value),
                    Some(_) => return Vec::new(),
                    None => i128::from(i64::MAX),
                };
                (start <= end).then_some((start, end)).into_iter().collect()
            }
            MatchPattern::Wildcard | MatchPattern::Identifier(_) => {
                vec![(i128::from(i64::MIN), i128::from(i64::MAX))]
            }
            MatchPattern::Or(alternatives) => alternatives
                .iter()
                .flat_map(|alternative| self.int_pattern_intervals(alternative))
                .collect(),
            _ => Vec::new(),
        }
    }

    pub(super) fn int_pattern_vector_is_useful(
        &self,
        matrix: &[Vec<MatchPattern>],
        candidate_intervals: &[(i128, i128)],
        tail: &[MatchPattern],
        rest_types: &[Type],
    ) -> bool {
        let mut boundaries = Vec::new();
        for (start, end) in candidate_intervals {
            boundaries.push(*start);
            boundaries.push(*end + 1);
        }
        for row in matrix {
            let Some((head, _)) = row.split_first() else {
                continue;
            };
            for (start, end) in self.int_pattern_intervals(head) {
                boundaries.push(start);
                boundaries.push(end + 1);
            }
        }
        boundaries.sort_unstable();
        boundaries.dedup();

        for window in boundaries.windows(2) {
            let start = window[0];
            let end = window[1] - 1;
            if !candidate_intervals
                .iter()
                .any(|(lower, upper)| *lower <= start && end <= *upper)
            {
                continue;
            }

            let specialized = matrix
                .iter()
                .filter_map(|row| {
                    let (head, tail) = row.split_first()?;
                    self.int_pattern_intervals(head)
                        .iter()
                        .any(|(lower, upper)| *lower <= start && end <= *upper)
                        .then(|| tail.to_vec())
                })
                .collect::<Vec<_>>();
            if self.pattern_vector_is_useful(&specialized, tail, rest_types) {
                return true;
            }
        }
        false
    }

    pub(super) fn pattern_constructor(
        &self,
        pattern: &MatchPattern,
    ) -> Option<(CoverageConstructor, Vec<MatchPattern>)> {
        match pattern {
            MatchPattern::Alias { pattern, .. } => self.pattern_constructor(pattern),
            MatchPattern::EnumVariant {
                enum_name,
                variant_name,
                payload,
            } => Some((
                CoverageConstructor::Enum {
                    type_identity: self.enum_identities.get(enum_name)?.clone(),
                    variant: variant_name.clone(),
                },
                payload
                    .iter()
                    .map(|pattern| pattern.as_ref().clone())
                    .collect(),
            )),
            MatchPattern::Tuple(patterns) => {
                Some((CoverageConstructor::Tuple(patterns.len()), patterns.clone()))
            }
            MatchPattern::Struct { type_name, fields } => Some((
                CoverageConstructor::Struct(self.struct_identities.get(type_name)?.clone()),
                fields.clone(),
            )),
            MatchPattern::NamedStruct { type_name, fields } => {
                let struct_fields = self.structs.get(type_name)?;
                let mut positional = vec![MatchPattern::Wildcard; struct_fields.len()];
                for (field_name, pattern) in fields {
                    let index = struct_fields
                        .iter()
                        .position(|field| field.name == *field_name)?;
                    positional[index] = pattern.clone();
                }
                Some((
                    CoverageConstructor::Struct(self.struct_identities.get(type_name)?.clone()),
                    positional,
                ))
            }
            MatchPattern::Literal(Literal::Bool(value)) => {
                Some((CoverageConstructor::Bool(*value), Vec::new()))
            }
            MatchPattern::Range { .. }
            | MatchPattern::Literal(_)
            | MatchPattern::Sequence { .. }
            | MatchPattern::Hash(_)
            | MatchPattern::Wildcard
            | MatchPattern::Identifier(_)
            | MatchPattern::Or(_) => None,
        }
    }

    pub(super) fn pattern_may_match_constructor(
        &self,
        pattern: &MatchPattern,
        constructor: &CoverageConstructor,
    ) -> bool {
        match pattern {
            MatchPattern::Alias { pattern, .. } => {
                self.pattern_may_match_constructor(pattern, constructor)
            }
            MatchPattern::Or(alternatives) => alternatives
                .iter()
                .any(|alternative| self.pattern_may_match_constructor(alternative, constructor)),
            MatchPattern::Literal(Literal::Bool(value)) => {
                *constructor == CoverageConstructor::Bool(*value)
            }
            MatchPattern::Wildcard | MatchPattern::Identifier(_) => true,
            _ => self
                .pattern_constructor(pattern)
                .is_some_and(|(found, _)| found == *constructor),
        }
    }

    pub(super) fn finite_constructors(
        &self,
        typ: &Type,
    ) -> Option<Vec<(CoverageConstructor, Vec<Type>)>> {
        match typ {
            Type::Enum(identity) => {
                let name = self.enum_name_for_identity(identity)?;
                Some(
                    self.enums
                        .get(name)?
                        .iter()
                        .map(|variant| {
                            (
                                CoverageConstructor::Enum {
                                    type_identity: identity.clone(),
                                    variant: variant.name.clone(),
                                },
                                variant.payload_type.iter().cloned().collect(),
                            )
                        })
                        .collect(),
                )
            }
            Type::Tuple(types) => Some(vec![(
                CoverageConstructor::Tuple(types.len()),
                types.clone(),
            )]),
            Type::Struct(identity) => {
                let name = self.struct_name_for_identity(identity)?;
                Some(vec![(
                    CoverageConstructor::Struct(identity.clone()),
                    self.structs
                        .get(name)?
                        .iter()
                        .map(|field| field.field_type.clone())
                        .collect(),
                )])
            }
            Type::Bool => Some(vec![
                (CoverageConstructor::Bool(true), Vec::new()),
                (CoverageConstructor::Bool(false), Vec::new()),
            ]),
            _ => None,
        }
    }

    pub(super) fn specialize_literal_matrix(
        &self,
        matrix: &[Vec<MatchPattern>],
        literal: &Literal,
    ) -> Vec<Vec<MatchPattern>> {
        let mut specialized = Vec::new();
        for row in matrix {
            let Some((head, tail)) = row.split_first() else {
                continue;
            };
            match head {
                MatchPattern::Literal(found) if found == literal => {
                    specialized.push(tail.to_vec());
                }
                MatchPattern::Wildcard | MatchPattern::Identifier(_) => {
                    specialized.push(tail.to_vec());
                }
                MatchPattern::Or(alternatives) => {
                    for alternative in alternatives {
                        let expanded = std::iter::once(alternative.clone())
                            .chain(tail.iter().cloned())
                            .collect::<Vec<_>>();
                        specialized.extend(
                            self.specialize_literal_matrix(
                                std::slice::from_ref(&expanded),
                                literal,
                            ),
                        );
                    }
                }
                _ => {}
            }
        }
        specialized
    }

    pub(super) fn constructor_argument_types(
        &self,
        constructor: &CoverageConstructor,
        typ: &Type,
    ) -> Option<Vec<Type>> {
        if typ == &Type::Unknown {
            return Some(match constructor {
                CoverageConstructor::Tuple(arity) => vec![Type::Unknown; *arity],
                CoverageConstructor::Struct(identity) => self
                    .structs
                    .get(self.struct_name_for_identity(identity)?)?
                    .iter()
                    .map(|field| field.field_type.clone())
                    .collect(),
                CoverageConstructor::Enum {
                    type_identity,
                    variant,
                } => self
                    .enums
                    .get(self.enum_name_for_identity(type_identity)?)?
                    .iter()
                    .find(|definition| definition.name == *variant)?
                    .payload_type
                    .iter()
                    .cloned()
                    .collect(),
                CoverageConstructor::Bool(_) => Vec::new(),
            });
        }
        self.finite_constructors(typ)?
            .into_iter()
            .find_map(|(found, arguments)| (found == *constructor).then_some(arguments))
    }

    pub(super) fn sequence_pattern_vector_is_useful(
        &self,
        matrix: &[Vec<MatchPattern>],
        patterns: &[MatchPattern],
        has_rest: bool,
        tail: &[MatchPattern],
        rest_types: &[Type],
        element_type: &Type,
    ) -> bool {
        let max_length = self.max_sequence_prefix_length(matrix, patterns.len());
        let lengths = if has_rest {
            (patterns.len()..=max_length.max(patterns.len()) + 1).collect::<Vec<_>>()
        } else {
            vec![patterns.len()]
        };
        for length in lengths {
            let mut candidate = patterns.to_vec();
            candidate.resize(length, MatchPattern::Wildcard);
            if !has_rest && length != patterns.len() {
                continue;
            }
            candidate.extend_from_slice(tail);
            let mut specialized = Vec::new();
            for row in matrix {
                let Some((head, row_tail)) = row.split_first() else {
                    continue;
                };
                for mut expanded in self.expand_sequence_pattern(head, length) {
                    expanded.extend_from_slice(row_tail);
                    specialized.push(expanded);
                }
            }
            let mut types = vec![element_type.clone(); length];
            types.extend_from_slice(rest_types);
            if self.pattern_vector_is_useful(&specialized, &candidate, &types) {
                return true;
            }
        }
        false
    }

    pub(super) fn expand_hash_pattern(
        &self,
        entries: &[(String, MatchPattern)],
        tail: &[MatchPattern],
    ) -> Vec<MatchPattern> {
        let mut expanded = entries
            .iter()
            .map(|(_, pattern)| pattern.clone())
            .collect::<Vec<_>>();
        expanded.extend_from_slice(tail);
        expanded
    }

    pub(super) fn expand_hash_row(
        &self,
        row: &[MatchPattern],
        candidate_entries: &[(String, MatchPattern)],
    ) -> Vec<Vec<MatchPattern>> {
        let Some((head, tail)) = row.split_first() else {
            return Vec::new();
        };
        match head {
            MatchPattern::Wildcard | MatchPattern::Identifier(_) => {
                let mut expanded = vec![MatchPattern::Wildcard; candidate_entries.len()];
                expanded.extend_from_slice(tail);
                vec![expanded]
            }
            MatchPattern::Hash(entries)
                if entries
                    .iter()
                    .all(|(key, _)| candidate_entries.iter().any(|(found, _)| found == key)) =>
            {
                let mut expanded = candidate_entries
                    .iter()
                    .map(|(key, _)| {
                        entries
                            .iter()
                            .find(|(found, _)| found == key)
                            .map(|(_, pattern)| pattern.clone())
                            .unwrap_or(MatchPattern::Wildcard)
                    })
                    .collect::<Vec<_>>();
                expanded.extend_from_slice(tail);
                vec![expanded]
            }
            MatchPattern::Or(alternatives) => alternatives
                .iter()
                .flat_map(|alternative| {
                    self.expand_hash_row(
                        &std::iter::once(alternative.clone())
                            .chain(tail.iter().cloned())
                            .collect::<Vec<_>>(),
                        candidate_entries,
                    )
                })
                .collect(),
            _ => Vec::new(),
        }
    }

    pub(super) fn hash_default_matrix(
        &self,
        matrix: &[Vec<MatchPattern>],
    ) -> Vec<Vec<MatchPattern>> {
        let mut defaults = Vec::new();
        for row in matrix {
            let Some((head, tail)) = row.split_first() else {
                continue;
            };
            match head {
                MatchPattern::Wildcard | MatchPattern::Identifier(_) => {
                    defaults.push(tail.to_vec())
                }
                MatchPattern::Hash(entries) if entries.is_empty() => defaults.push(tail.to_vec()),
                MatchPattern::Or(alternatives)
                    if alternatives.iter().any(|alternative| {
                        matches!(
                            alternative,
                            MatchPattern::Wildcard | MatchPattern::Identifier(_)
                        ) || matches!(alternative, MatchPattern::Hash(entries) if entries.is_empty())
                    }) =>
                {
                    defaults.push(tail.to_vec());
                }
                _ => {}
            }
        }
        defaults
    }

    pub(super) fn max_sequence_prefix_length(
        &self,
        matrix: &[Vec<MatchPattern>],
        current: usize,
    ) -> usize {
        matrix.iter().fold(current, |maximum, row| {
            row.first().map_or(maximum, |pattern| {
                self.sequence_pattern_prefix_max(pattern, maximum)
            })
        })
    }

    pub(super) fn sequence_pattern_prefix_max(
        &self,
        pattern: &MatchPattern,
        current: usize,
    ) -> usize {
        match pattern {
            MatchPattern::Sequence { patterns, .. } => current.max(patterns.len()),
            MatchPattern::Or(alternatives) => {
                alternatives.iter().fold(current, |maximum, pattern| {
                    self.sequence_pattern_prefix_max(pattern, maximum)
                })
            }
            _ => current,
        }
    }

    pub(super) fn expand_sequence_pattern(
        &self,
        pattern: &MatchPattern,
        length: usize,
    ) -> Vec<Vec<MatchPattern>> {
        match pattern {
            MatchPattern::Wildcard | MatchPattern::Identifier(_) => {
                vec![vec![MatchPattern::Wildcard; length]]
            }
            MatchPattern::Sequence { patterns, rest } => {
                if patterns.len() > length || (rest.is_none() && patterns.len() != length) {
                    return Vec::new();
                }
                let mut expanded = patterns.clone();
                expanded.resize(length, MatchPattern::Wildcard);
                vec![expanded]
            }
            MatchPattern::Or(alternatives) => alternatives
                .iter()
                .flat_map(|alternative| self.expand_sequence_pattern(alternative, length))
                .collect(),
            _ => Vec::new(),
        }
    }

    pub(super) fn specialize_matrix(
        &self,
        matrix: &[Vec<MatchPattern>],
        constructor: &CoverageConstructor,
        arity: usize,
    ) -> Vec<Vec<MatchPattern>> {
        let mut specialized = Vec::new();
        for row in matrix {
            let Some((head, tail)) = row.split_first() else {
                continue;
            };
            match self.pattern_constructor(head) {
                Some((found, arguments)) if found == *constructor => {
                    let mut new_row = arguments;
                    new_row.extend_from_slice(tail);
                    specialized.push(new_row);
                }
                None => {
                    if matches!(head, MatchPattern::Wildcard | MatchPattern::Identifier(_)) {
                        let mut new_row = vec![MatchPattern::Wildcard; arity];
                        new_row.extend_from_slice(tail);
                        specialized.push(new_row);
                    } else if let MatchPattern::Or(alternatives) = head {
                        for alternative in alternatives {
                            let expanded = self.specialize_matrix(
                                &[std::iter::once(alternative.clone())
                                    .chain(tail.iter().cloned())
                                    .collect()],
                                constructor,
                                arity,
                            );
                            specialized.extend(expanded);
                        }
                    }
                }
                _ => {}
            }
        }
        specialized
    }

    pub(super) fn default_matrix(&self, matrix: &[Vec<MatchPattern>]) -> Vec<Vec<MatchPattern>> {
        let mut defaults = Vec::new();
        for row in matrix {
            let Some((head, tail)) = row.split_first() else {
                continue;
            };
            match head {
                MatchPattern::Wildcard | MatchPattern::Identifier(_) => {
                    defaults.push(tail.to_vec())
                }
                MatchPattern::Or(alternatives)
                    if alternatives.iter().any(|alternative| {
                        matches!(
                            alternative,
                            MatchPattern::Wildcard | MatchPattern::Identifier(_)
                        )
                    }) =>
                {
                    defaults.push(tail.to_vec());
                }
                _ => {}
            }
        }
        defaults
    }

    pub(super) fn validate_match_pattern(
        &self,
        pattern: &MatchPattern,
        expected: &Type,
        bindings: &mut Vec<(String, Type)>,
    ) -> Result<bool, SimplyError> {
        match pattern {
            MatchPattern::Wildcard => Ok(true),
            MatchPattern::Identifier(name) => {
                bindings.push((name.clone(), expected.clone()));
                Ok(true)
            }
            MatchPattern::Alias { name, pattern } => {
                bindings.push((name.clone(), expected.clone()));
                self.validate_match_pattern(pattern, expected, bindings)
            }
            MatchPattern::Or(alternatives) => {
                if alternatives.len() < 2 {
                    return Err(self.error(
                        DiagnosticCode::UnexpectedToken,
                        "OR-pattern requires at least two alternatives",
                    ));
                }
                let mut canonical_bindings: Option<Vec<(String, Type)>> = None;
                let mut irrefutable = false;
                let mut alternatives_matrix = Vec::new();
                for alternative in alternatives {
                    if !self.pattern_is_useful(&alternatives_matrix, alternative, expected) {
                        return Err(self.error(
                            DiagnosticCode::UnexpectedToken,
                            "redundant OR-pattern alternative",
                        ));
                    }
                    let mut alternative_bindings = Vec::new();
                    irrefutable |= self.validate_match_pattern(
                        alternative,
                        expected,
                        &mut alternative_bindings,
                    )?;
                    alternative_bindings.sort_by(|left, right| left.0.cmp(&right.0));
                    if let Some(expected_bindings) = &mut canonical_bindings {
                        let same_names = expected_bindings.len() == alternative_bindings.len()
                            && expected_bindings
                                .iter()
                                .zip(&alternative_bindings)
                                .all(|((left_name, _), (right_name, _))| left_name == right_name);
                        let compatible_types = same_names
                            && expected_bindings.iter().zip(&alternative_bindings).all(
                                |((_, left_type), (_, right_type))| {
                                    left_type.compatible_with(right_type)
                                        && right_type.compatible_with(left_type)
                                },
                            );
                        if !compatible_types {
                            let message = if !same_names {
                                "OR-pattern alternatives must bind the same names"
                            } else {
                                "OR-pattern alternatives must bind compatible types"
                            };
                            return Err(self.error(DiagnosticCode::TypeMismatch, message));
                        }
                        for ((_, common_type), (_, alternative_type)) in
                            expected_bindings.iter_mut().zip(&alternative_bindings)
                        {
                            if *common_type == Type::Unknown {
                                *common_type = alternative_type.clone();
                            }
                        }
                    } else {
                        canonical_bindings = Some(alternative_bindings);
                    }
                    alternatives_matrix.push(vec![alternative.clone()]);
                }
                bindings.extend(canonical_bindings.unwrap_or_default());
                Ok(irrefutable)
            }
            MatchPattern::Hash(entries) => {
                if !matches!(expected, Type::Hash | Type::HashValues(_) | Type::Unknown) {
                    return Err(self.error(
                        DiagnosticCode::TypeMismatch,
                        format!(
                            "hash pattern cannot match value of type {}",
                            expected.name()
                        ),
                    ));
                }
                let irrefutable = entries.is_empty();
                let value_type = match expected {
                    Type::HashValues(value_type) => (**value_type).clone(),
                    _ => Type::Unknown,
                };
                for (_, pattern) in entries {
                    self.validate_match_pattern(pattern, &value_type, bindings)?;
                }
                Ok(irrefutable)
            }
            MatchPattern::Literal(literal) => {
                let literal_type = match literal {
                    Literal::String(_) => Type::String,
                    Literal::Int(_) => Type::Int,
                    Literal::Float(_) => Type::Float,
                    Literal::Bool(_) => Type::Bool,
                };
                if expected != &Type::Unknown && !literal_type.compatible_with(expected) {
                    return Err(self.error(
                        DiagnosticCode::TypeMismatch,
                        format!(
                            "literal pattern of type {} cannot match value of type {}",
                            literal_type.name(),
                            expected.name()
                        ),
                    ));
                }
                Ok(false)
            }
            MatchPattern::Range { start, end } => {
                for bound in start.iter().chain(end.iter()) {
                    if !matches!(bound, Literal::Int(_)) {
                        return Err(self.error(
                            DiagnosticCode::TypeMismatch,
                            "range pattern bounds must be Int",
                        ));
                    }
                }
                if expected != &Type::Int && expected != &Type::Unknown {
                    return Err(self.error(
                        DiagnosticCode::TypeMismatch,
                        format!(
                            "range pattern cannot match value of type {}",
                            expected.name()
                        ),
                    ));
                }
                if let (Some(Literal::Int(start)), Some(Literal::Int(end))) = (start, end)
                    && start > end
                {
                    return Err(self.error(
                        DiagnosticCode::TypeMismatch,
                        "range pattern lower bound must not exceed upper bound",
                    ));
                }
                Ok(false)
            }
            MatchPattern::Tuple(patterns) => {
                let unknown_types;
                let types = if let Type::Tuple(types) = expected {
                    types
                } else if expected == &Type::Unknown {
                    unknown_types = vec![Type::Unknown; patterns.len()];
                    &unknown_types
                } else {
                    return Err(self.error(
                        DiagnosticCode::TypeMismatch,
                        format!(
                            "tuple pattern cannot match value of type {}",
                            expected.name()
                        ),
                    ));
                };
                if patterns.len() != types.len() {
                    return Err(self.error(
                        DiagnosticCode::TypeMismatch,
                        format!(
                            "tuple pattern expects {} elements, found {}",
                            patterns.len(),
                            types.len()
                        ),
                    ));
                }
                let mut irrefutable = true;
                for (pattern, typ) in patterns.iter().zip(types) {
                    irrefutable &= self.validate_match_pattern(pattern, typ, bindings)?;
                }
                Ok(irrefutable)
            }
            MatchPattern::Sequence { patterns, rest } => {
                let (element_type, sequence_type) = match expected {
                    Type::Array(element) | Type::List(element) | Type::Vector(element, _) => {
                        ((**element).clone(), expected.clone())
                    }
                    Type::Range => (Type::Int, Type::Range),
                    Type::CsvStream => (Type::List(Box::new(Type::String)), Type::CsvStream),
                    Type::Unknown => (Type::Unknown, Type::Unknown),
                    _ => {
                        return Err(self.error(
                            DiagnosticCode::TypeMismatch,
                            format!(
                                "sequence pattern cannot match value of type {}",
                                expected.name()
                            ),
                        ));
                    }
                };
                for pattern in patterns {
                    self.validate_match_pattern(pattern, &element_type, bindings)?;
                }
                if let Some(name) = rest {
                    bindings.push((name.clone(), sequence_type));
                }
                Ok(patterns.is_empty() && rest.is_some())
            }
            MatchPattern::Struct { type_name, fields } => {
                if let Type::Struct(actual_identity) = expected
                    && self.struct_identities.get(type_name) != Some(actual_identity)
                {
                    return Err(self.error(
                        DiagnosticCode::TypeMismatch,
                        format!(
                            "struct pattern `{type_name}` cannot match value of type {}",
                            expected.name()
                        ),
                    ));
                } else if expected != &Type::Unknown && !matches!(expected, Type::Struct(_)) {
                    return Err(self.error(
                        DiagnosticCode::TypeMismatch,
                        format!(
                            "struct pattern `{type_name}` cannot match value of type {}",
                            expected.name()
                        ),
                    ));
                }
                let Some(struct_fields) = self.structs.get(type_name) else {
                    return Err(self.error(
                        DiagnosticCode::UndefinedVariable,
                        format!("unknown struct type `{type_name}`"),
                    ));
                };
                if fields.len() != struct_fields.len() {
                    return Err(self.error(
                        DiagnosticCode::InvalidFunctionCall,
                        format!(
                            "struct pattern `{type_name}` expects {} fields, found {}",
                            struct_fields.len(),
                            fields.len()
                        ),
                    ));
                }
                let mut irrefutable = true;
                for (pattern, field) in fields.iter().zip(struct_fields) {
                    irrefutable &=
                        self.validate_match_pattern(pattern, &field.field_type, bindings)?;
                }
                Ok(irrefutable)
            }
            MatchPattern::NamedStruct { type_name, fields } => {
                if let Type::Struct(actual_identity) = expected
                    && self.struct_identities.get(type_name) != Some(actual_identity)
                {
                    return Err(self.error(
                        DiagnosticCode::TypeMismatch,
                        format!(
                            "struct pattern `{type_name}` cannot match value of type {}",
                            expected.name()
                        ),
                    ));
                } else if expected != &Type::Unknown && !matches!(expected, Type::Struct(_)) {
                    return Err(self.error(
                        DiagnosticCode::TypeMismatch,
                        format!(
                            "struct pattern `{type_name}` cannot match value of type {}",
                            expected.name()
                        ),
                    ));
                }
                let Some(struct_fields) = self.structs.get(type_name) else {
                    return Err(self.error(
                        DiagnosticCode::UndefinedVariable,
                        format!("unknown struct type `{type_name}`"),
                    ));
                };
                let mut irrefutable = true;
                for (field_name, pattern) in fields {
                    let Some(field) = struct_fields.iter().find(|field| field.name == *field_name)
                    else {
                        return Err(self.error(
                            DiagnosticCode::UndefinedVariable,
                            format!("struct pattern `{type_name}` has no field `{field_name}`"),
                        ));
                    };
                    irrefutable &=
                        self.validate_match_pattern(pattern, &field.field_type, bindings)?;
                }
                Ok(irrefutable)
            }
            MatchPattern::EnumVariant {
                enum_name,
                variant_name,
                payload,
            } => {
                if expected
                    != &Type::Enum(self.enum_identities.get(enum_name).cloned().unwrap_or_else(
                        || DeclarationIdentity::unresolved(enum_name, DeclarationKind::Enum),
                    ))
                    && expected != &Type::Unknown
                {
                    return Err(self.error(
                        DiagnosticCode::TypeMismatch,
                        format!(
                            "pattern `{enum_name}::{variant_name}` cannot match value of type {}",
                            expected.name()
                        ),
                    ));
                }
                let Some(variants) = self.enums.get(enum_name) else {
                    return Err(self.error(
                        DiagnosticCode::UndefinedVariable,
                        format!("unknown enum type `{enum_name}`"),
                    ));
                };
                let Some(variant) = variants
                    .iter()
                    .find(|variant| variant.name == *variant_name)
                else {
                    return Err(self.error(
                        DiagnosticCode::UndefinedVariable,
                        format!("unknown variant `{enum_name}::{variant_name}`"),
                    ));
                };
                let payload_irrefutable = match (&variant.payload_type, payload) {
                    (Some(payload_type), Some(payload_pattern)) => {
                        self.validate_match_pattern(payload_pattern, payload_type, bindings)?
                    }
                    (Some(payload_type), None) => {
                        return Err(self.error(
                            DiagnosticCode::InvalidFunctionCall,
                            format!(
                                "pattern `{enum_name}::{variant_name}` requires a payload binding (pattern) of type {}",
                                payload_type.name()
                            ),
                        ));
                    }
                    (None, Some(_)) => {
                        return Err(self.error(
                            DiagnosticCode::InvalidFunctionCall,
                            format!("unit variant `{enum_name}::{variant_name}` has no payload"),
                        ));
                    }
                    (None, None) => true,
                };
                Ok(variants.len() == 1 && payload_irrefutable)
            }
        }
    }
}
