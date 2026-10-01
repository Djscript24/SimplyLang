use super::*;

impl SemanticAnalyzer {
    pub(super) fn pipeline_type(
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
        let has_take = steps
            .iter()
            .any(|step| matches!(step, PipelineStep::Take(_)));
        let has_skip = steps
            .iter()
            .any(|step| matches!(step, PipelineStep::Skip(_)));
        let has_step_by = steps
            .iter()
            .any(|step| matches!(step, PipelineStep::StepBy(_)));
        let has_take_while = steps
            .iter()
            .any(|step| matches!(step, PipelineStep::TakeWhile(_)));
        let has_drop_while = steps
            .iter()
            .any(|step| matches!(step, PipelineStep::DropWhile(_)));
        let has_distinct = steps
            .iter()
            .any(|step| matches!(step, PipelineStep::Distinct));
        let writes_csv = matches!(steps.last(), Some(PipelineStep::WriteCsv(_)));
        let reads_csv_rows = matches!(source, Expr::Call { name, .. } if name == "csv_rows");
        if has_parallel && has_checkpoint {
            return Err(self.error(
                DiagnosticCode::SemanticCollection,
                "`parallel` cannot be combined with `checkpoint`",
            ));
        }
        if (has_take || has_skip || has_step_by || has_take_while || has_drop_while || has_distinct)
            && has_parallel
        {
            return Err(self.error(
                DiagnosticCode::SemanticCollection,
                "`take`, `skip`, `step_by`, `take_while`, `drop_while`, and `distinct` cannot be combined with `parallel` because they require ordered input",
            ));
        }
        if (has_take || has_skip || has_step_by || has_take_while || has_drop_while || has_distinct)
            && has_checkpoint
        {
            return Err(self.error(
                DiagnosticCode::SemanticCollection,
                "`take`, `skip`, `step_by`, `take_while`, `drop_while`, and `distinct` cannot be combined with `checkpoint`",
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
                        | PipelineStep::Any
                        | PipelineStep::All
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
            Type::Array(element) | Type::List(element) | Type::Vector(element, _) => *element,
            Type::Range => Type::Int,
            Type::CsvStream => Type::List(Box::new(Type::String)),
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
                PipelineStep::Take(count) => {
                    if *count < 0 {
                        return Err(self.error(
                            DiagnosticCode::SemanticCollection,
                            "`take` count must be a non-negative integer",
                        ));
                    }
                }
                PipelineStep::Skip(count) => {
                    if *count < 0 {
                        return Err(self.error(
                            DiagnosticCode::SemanticCollection,
                            "`skip` count must be a non-negative integer",
                        ));
                    }
                }
                PipelineStep::StepBy(interval) => {
                    if *interval <= 0 {
                        return Err(self.error(
                            DiagnosticCode::SemanticCollection,
                            "`step_by` interval must be a positive integer",
                        ));
                    }
                }
                PipelineStep::TakeWhile(expression) => {
                    let previous =
                        self.variables
                            .insert("item".into(), item_type.clone(), false)?;
                    let condition_type = self.analyze_expression(expression)?;
                    self.require_type(&Type::Bool, &condition_type)?;
                    Self::restore_item(&mut self.variables, "item", previous);
                }
                PipelineStep::DropWhile(expression) => {
                    let previous =
                        self.variables
                            .insert("item".into(), item_type.clone(), false)?;
                    let condition_type = self.analyze_expression(expression)?;
                    self.require_type(&Type::Bool, &condition_type)?;
                    Self::restore_item(&mut self.variables, "item", previous);
                }
                PipelineStep::Distinct => {}
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
                    return Ok(Type::HashValues(Box::new(Type::List(Box::new(
                        item_type.clone(),
                    )))));
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
                PipelineStep::Any | PipelineStep::All => {
                    self.require_type(&Type::Bool, &item_type)?;
                    return Ok(Type::Bool);
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
                    if usize::try_from(*size).map_or(true, |size| size > limits::MAX_CHUNK_SIZE) {
                        return Err(self.error(
                            DiagnosticCode::SemanticCollection,
                            format!("chunk size cannot exceed {}", limits::MAX_CHUNK_SIZE),
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
                    if usize::try_from(*workers)
                        .map_or(true, |workers| workers > limits::MAX_PARALLEL_WORKERS)
                    {
                        return Err(self.error(
                            DiagnosticCode::SemanticCollection,
                            format!(
                                "parallel worker count cannot exceed {}",
                                limits::MAX_PARALLEL_WORKERS
                            ),
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

    pub(super) fn restore_item(variables: &mut ScopeStack, name: &str, previous: Option<Type>) {
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
}
