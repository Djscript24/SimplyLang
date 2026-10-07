use super::*;

impl Evaluator {
    pub(super) fn evaluate_values(
        &mut self,
        expressions: &[Expr],
    ) -> Result<Vec<Value>, SimplyError> {
        expressions
            .iter()
            .map(|expression| self.evaluate(expression))
            .collect()
    }

    pub(super) fn evaluate_pipeline(
        &mut self,
        mut values: Vec<Value>,
        steps: &[PipelineStep],
        sum_type: Type,
    ) -> Result<Value, SimplyError> {
        if steps
            .iter()
            .any(|step| matches!(step, PipelineStep::Chunk(_)))
            && !steps.iter().any(|step| {
                matches!(
                    step,
                    PipelineStep::Parallel(_) | PipelineStep::Checkpoint(_)
                )
            })
        {
            return Err(self.runtime_argument_error("`chunk` requires `parallel` or `checkpoint`"));
        }
        if steps
            .iter()
            .any(|step| matches!(step, PipelineStep::Checkpoint(_)))
        {
            return Err(self.runtime_argument_error(
                "`checkpoint` requires a `csv_rows` source and `write_csv` terminal",
            ));
        }
        self.push_scope();
        let result = self.evaluate_pipeline_steps(&mut values, steps, sum_type);
        self.pop_scope();
        result
    }

    pub(super) fn evaluate_range_pipeline(
        &mut self,
        start: i64,
        end: i64,
        step: i64,
        steps: &[PipelineStep],
        sum_type: Type,
    ) -> Result<Value, SimplyError> {
        if steps
            .iter()
            .any(|step| matches!(step, PipelineStep::Chunk(_)))
            && !steps.iter().any(|step| {
                matches!(
                    step,
                    PipelineStep::Parallel(_) | PipelineStep::Checkpoint(_)
                )
            })
        {
            return Err(self.runtime_argument_error("`chunk` requires `parallel` or `checkpoint`"));
        }
        if steps
            .iter()
            .any(|step| matches!(step, PipelineStep::Checkpoint(_)))
        {
            return Err(self.runtime_argument_error(
                "`checkpoint` requires a `csv_rows` source and `write_csv` terminal",
            ));
        }
        if let Some(terminal) = steps.last()
            && matches!(
                terminal,
                PipelineStep::Count
                    | PipelineStep::Sum
                    | PipelineStep::Average
                    | PipelineStep::Min
                    | PipelineStep::Max
                    | PipelineStep::Any
                    | PipelineStep::All
            )
            && steps[..steps.len() - 1].iter().all(|step| match step {
                PipelineStep::Where(expression) | PipelineStep::Derive(expression) => {
                    is_parallel_safe_expression(expression)
                }
                _ => false,
            })
        {
            return self.evaluate_scalar_range_pipeline(
                start,
                end,
                step,
                &steps[..steps.len() - 1],
                terminal,
                sum_type,
            );
        }
        self.push_scope();
        let result = self.evaluate_streaming_pipeline_with_sum_type(
            Value::range_values(start, end, step),
            steps,
            sum_type,
        );
        self.pop_scope();
        result
    }

    pub(super) fn evaluate_scalar_range_pipeline(
        &self,
        start: i64,
        end: i64,
        step: i64,
        transforms: &[PipelineStep],
        terminal: &PipelineStep,
        sum_type: Type,
    ) -> Result<Value, SimplyError> {
        let mut count = 0i64;
        let mut total = Value::Int(0);
        let mut sum = SumAccumulator::new(sum_type);
        let mut minimum = None;
        let mut maximum = None;
        let mut any_result = false;
        let mut all_result = true;

        for value in Value::range_values(start, end, step) {
            let mut current = Some(value);
            for step in transforms {
                let Some(item) = current.take() else {
                    break;
                };
                match step {
                    PipelineStep::Where(expression) => {
                        match self.evaluate_scalar_pipeline_expression(expression, &item)? {
                            Value::Bool(true) => current = Some(item),
                            Value::Bool(false) => {}
                            _ => {
                                return Err(self.runtime_type_error(
                                    "pipeline `where` condition must return a boolean",
                                ));
                            }
                        }
                    }
                    PipelineStep::Derive(expression) => {
                        current =
                            Some(self.evaluate_scalar_pipeline_expression(expression, &item)?);
                    }
                    _ => unreachable!("range fast path accepts only pure scalar transforms"),
                }
            }
            let Some(value) = current else {
                continue;
            };
            match terminal {
                PipelineStep::Count => count += 1,
                PipelineStep::Sum => sum.add(&self.heap, value, self.current_span.as_ref())?,
                PipelineStep::Average => {
                    total = operations::binary(
                        &self.heap,
                        total,
                        &BinaryOperator::Add,
                        value,
                        self.current_span.as_ref(),
                    )?;
                    count += 1;
                }
                PipelineStep::Min => {
                    update_extreme(&mut minimum, value, false, self.current_span.as_ref())?;
                }
                PipelineStep::Max => {
                    update_extreme(&mut maximum, value, true, self.current_span.as_ref())?;
                }
                PipelineStep::Any => match value {
                    Value::Bool(true) => return Ok(Value::Bool(true)),
                    Value::Bool(false) => any_result = false,
                    _ => {
                        return Err(self
                            .runtime_type_error("`any` pipeline terminal requires boolean items"));
                    }
                },
                PipelineStep::All => match value {
                    Value::Bool(false) => return Ok(Value::Bool(false)),
                    Value::Bool(true) => all_result = true,
                    _ => {
                        return Err(self
                            .runtime_type_error("`all` pipeline terminal requires boolean items"));
                    }
                },
                _ => unreachable!("range fast path accepts only aggregate terminals"),
            }
        }

        match terminal {
            PipelineStep::Count => Ok(Value::Int(count)),
            PipelineStep::Sum => Ok(sum.finish()),
            PipelineStep::Average if count == 0 => {
                Err(self.runtime_collection_error("average requires at least one numeric value"))
            }
            PipelineStep::Average => Ok(Value::Float(
                total_to_f64(total, self.current_span.as_ref())? / count as f64,
            )),
            PipelineStep::Min => minimum.ok_or_else(|| {
                self.runtime_collection_error("min requires at least one numeric value")
            }),
            PipelineStep::Max => maximum.ok_or_else(|| {
                self.runtime_collection_error("max requires at least one numeric value")
            }),
            PipelineStep::Any => Ok(Value::Bool(any_result)),
            PipelineStep::All => Ok(Value::Bool(all_result)),
            _ => unreachable!("range fast path accepts only aggregate terminals"),
        }
    }

    pub(super) fn evaluate_scalar_pipeline_expression(
        &self,
        expression: &Expr,
        item: &Value,
    ) -> Result<Value, SimplyError> {
        match expression {
            Expr::Literal(Literal::String(value)) => Ok(Value::String(value.clone())),
            Expr::Literal(Literal::Int(value)) => Ok(Value::Int(*value)),
            Expr::Literal(Literal::Float(value)) => Ok(Value::Float(*value)),
            Expr::Literal(Literal::Bool(value)) => Ok(Value::Bool(*value)),
            Expr::Identifier(name) if name == "item" => Ok(item.clone()),
            Expr::Unary { operator, operand } => operations::unary(
                &self.heap,
                self.evaluate_scalar_pipeline_expression(operand, item)?,
                operator,
                self.current_span.as_ref(),
            ),
            Expr::Binary {
                left,
                operator,
                right,
            } => {
                let left = self.evaluate_scalar_pipeline_expression(left, item)?;
                if matches!(operator, BinaryOperator::And | BinaryOperator::Or)
                    && let Value::Bool(value) = left
                    && ((*operator == BinaryOperator::And && !value)
                        || (*operator == BinaryOperator::Or && value))
                {
                    return Ok(Value::Bool(value));
                }
                operations::binary(
                    &self.heap,
                    left,
                    operator,
                    self.evaluate_scalar_pipeline_expression(right, item)?,
                    self.current_span.as_ref(),
                )
            }
            _ => Err(SimplyError::Runtime {
                span: self.current_span.clone().unwrap_or_else(|| Span::new(0, 0)),
                code: DiagnosticCode::RuntimeGeneral,
                message: "expression is outside the scalar pipeline fast path".into(),
            }),
        }
    }

    pub(super) fn evaluate_csv_pipeline(
        &mut self,
        input_path: &str,
        start_record: usize,
        start_offset: u64,
        source_version: Option<&CsvStreamVersion>,
        steps: &[PipelineStep],
        sum_type: Type,
    ) -> Result<Value, SimplyError> {
        if steps
            .iter()
            .any(|step| matches!(step, PipelineStep::Chunk(_)))
            && !steps
                .iter()
                .any(|step| matches!(step, PipelineStep::Checkpoint(_)))
        {
            return Err(self.runtime_argument_error("`chunk` requires `parallel` or `checkpoint`"));
        }
        if steps
            .iter()
            .any(|step| matches!(step, PipelineStep::Parallel(_)))
        {
            return Err(self.runtime_argument_error(
                "`parallel` does not support CSV row collections or output terminals",
            ));
        }
        if steps.iter().any(|step| {
            matches!(
                step,
                PipelineStep::Take(_)
                    | PipelineStep::Skip(_)
                    | PipelineStep::StepBy(_)
                    | PipelineStep::TakeWhile(_)
                    | PipelineStep::DropWhile(_)
                    | PipelineStep::Distinct
            )
        }) && steps
            .iter()
            .any(|step| matches!(step, PipelineStep::Checkpoint(_)))
        {
            return Err(self.runtime_argument_error(
                "`take`, `skip`, `step_by`, `take_while`, `drop_while`, and `distinct` cannot be combined with `checkpoint`"
            ));
        }
        let terminal = steps.last().ok_or_else(|| {
            self.runtime_argument_error("csv_rows pipeline requires a terminal step")
        })?;
        let output_path = match terminal {
            PipelineStep::WriteCsv(path_expression) => {
                let output_path = match self.evaluate(path_expression)? {
                    Value::String(path) => path,
                    _ => return Err(self.runtime_type_error("write_csv path must be a string")),
                };
                Some(output_path)
            }
            PipelineStep::Sum
            | PipelineStep::Count
            | PipelineStep::Average
            | PipelineStep::Min
            | PipelineStep::Max
            | PipelineStep::Any
            | PipelineStep::All
            | PipelineStep::Partition { .. } => None,
            _ => {
                return Err(self.runtime_argument_error(
                    "csv_rows pipeline must end with an aggregate, partition, or write_csv(\"path\")",
                ));
            }
        };
        let input = fs::File::open(input_path)
            .map_err(|error| self.file_error(Path::new(input_path), error))?;
        let chunk_size = steps.iter().find_map(|step| match step {
            PipelineStep::Chunk(size) => Some(*size),
            _ => None,
        });
        let chunk_size = chunk_size
            .map(|size| {
                usize::try_from(size)
                    .map_err(|_| self.runtime_argument_error("chunk size is out of bounds"))
            })
            .transpose()?;
        if let Some(size) = chunk_size
            && (size == 0 || size > limits::MAX_CHUNK_SIZE)
        {
            return Err(self.runtime_argument_error(format!(
                "chunk size must be between 1 and {}",
                limits::MAX_CHUNK_SIZE
            )));
        }
        let checkpoint_path = steps.iter().find_map(|step| match step {
            PipelineStep::Checkpoint(path) => Some(path),
            _ => None,
        });
        self.push_scope();
        let result = (|| {
            let checkpoint_path = match checkpoint_path {
                Some(path) => match self.evaluate(path)? {
                    Value::String(path) => Some(path),
                    _ => return Err(self.runtime_type_error("checkpoint path must be a string")),
                },
                None => None,
            };
            if let Some(output_path) = output_path.as_deref()
                && checkpoint::paths_are_same(input_path, output_path)
                    .map_err(|error| self.checkpoint_runtime_error(error))?
            {
                return Err(self.runtime_argument_error(
                    "`csv_rows` input and `write_csv` output must be different files",
                ));
            }
            if let Some(checkpoint_path) = checkpoint_path.as_deref() {
                let temporary_checkpoint = format!("{checkpoint_path}.tmp");
                for protected_path in [Some(input_path), output_path.as_deref()]
                    .into_iter()
                    .flatten()
                {
                    if checkpoint::paths_are_same(checkpoint_path, protected_path)
                        .map_err(|error| self.checkpoint_runtime_error(error))?
                        || checkpoint::paths_are_same(&temporary_checkpoint, protected_path)
                            .map_err(|error| self.checkpoint_runtime_error(error))?
                    {
                        return Err(self.runtime_argument_error(
                            "`checkpoint` and its temporary file must not overlap the CSV input or output"
                        ));
                    }
                }
            }
            let checkpoint_state = checkpoint_path
                .as_deref()
                .map(checkpoint::read_checkpoint)
                .transpose()
                .map_err(|error| self.checkpoint_runtime_error(error))?
                .flatten();
            if checkpoint_path.is_some() && output_path.is_none() {
                return Err(
                    self.runtime_argument_error("`checkpoint` requires a `write_csv` terminal")
                );
            }
            let resume_at = checkpoint_state
                .as_ref()
                .map_or(0, |checkpoint| checkpoint.position);
            if let Some(state) = checkpoint_state.as_ref() {
                let output_path = output_path.as_deref().ok_or_else(|| {
                    self.runtime_argument_error("checkpoint output path is missing")
                })?;
                checkpoint::validate_checkpoint_source(state, input_path, output_path)
                    .map_err(|error| self.checkpoint_runtime_error(error))?;
                checkpoint::validate_checkpoint_output(state, output_path)
                    .map_err(|error| self.checkpoint_runtime_error(error))?;
            }
            if resume_at > 0 && !matches!(terminal, PipelineStep::WriteCsv(_)) {
                return Err(self.runtime_argument_error(
                    "checkpoint resume requires a `write_csv` terminal; aggregate state is not checkpointed yet",
                ));
            }
            let mut output = match output_path.as_deref() {
                Some(path) if resume_at > 0 => Some(
                    fs::OpenOptions::new()
                        .append(true)
                        .open(path)
                        .map_err(|error| self.file_error(Path::new(path), error))?,
                ),
                Some(path) => Some(
                    fs::File::create(path)
                        .map_err(|error| self.file_error(Path::new(path), error))?,
                ),
                None => None,
            };
            if let (Some(output), Some(output_len), Some(path)) = (
                output.as_mut(),
                checkpoint_state
                    .as_ref()
                    .map(|checkpoint| checkpoint.output_len),
                output_path.as_deref(),
            ) {
                let output_metadata = output
                    .metadata()
                    .map_err(|error| self.file_error(Path::new(path), error))?;
                if output_len > output_metadata.len() {
                    return Err(self.runtime_error_with_code(
                        DiagnosticCode::RuntimeGeneral,
                        format!(
                            "checkpoint output length exceeds the current output file `{path}`"
                        ),
                    ));
                }
                output
                    .set_len(output_len)
                    .map_err(|error| self.file_error(Path::new(path), error))?;
            }
            let interval = chunk_size.unwrap_or(1).max(1);
            let mut total = Value::Int(0);
            let mut sum = SumAccumulator::new(sum_type.clone());
            let mut count = 0i64;
            let mut minimum: Option<Value> = None;
            let mut maximum: Option<Value> = None;
            let mut any_result = false;
            let mut all_result = true;
            let mut partitioned = match terminal {
                PipelineStep::Partition { rules, .. } => Some(empty_partition_categories(rules)),
                _ => None,
            };
            let mut reader = BufReader::new(input);
            let current_version = Self::csv_stream_version(reader.get_ref(), input_path, self)?;
            Self::seek_csv_stream(
                &mut reader,
                input_path,
                start_record,
                start_offset,
                source_version,
                &current_version,
                self,
            )?;
            let mut index = start_record;
            let mut take_counts = vec![0i64; steps.len() - 1];
            let mut skip_counts = vec![0i64; steps.len() - 1];
            let mut step_by_counts = vec![0i64; steps.len() - 1];
            let mut seen_values = vec![Vec::new(); steps.len() - 1];
            let mut drop_while_done = vec![false; steps.len() - 1];
            loop {
                if steps[..steps.len() - 1]
                    .iter()
                    .any(|step| matches!(step, PipelineStep::Take(0)))
                {
                    break;
                }
                let Some(record) = csv::read_record(&mut reader)
                    .map_err(|error| self.file_error(Path::new(input_path), error))?
                else {
                    break;
                };
                if index < resume_at {
                    index += 1;
                    continue;
                }
                let line = record.strip_suffix('\n').unwrap_or(&record);
                let line = line.strip_suffix('\r').unwrap_or(line);
                let row = csv::parse_record(line).map_err(|message| {
                    self.runtime_error(format!("invalid CSV row in `{input_path}`: {message}"))
                })?;
                let mut current = Some(self.make_list(row));
                let mut stop_after_record = false;
                let mut stop_source = false;
                for (step_index, step) in steps[..steps.len() - 1].iter().enumerate() {
                    match step {
                        PipelineStep::Where(expression) => {
                            let item = current.take().ok_or_else(|| {
                                self.runtime_error("pipeline item was lost".into())
                            })?;
                            current = self.evaluate_pipeline_where_item(item, expression)?;
                            if current.is_none() {
                                break;
                            }
                        }
                        PipelineStep::Derive(expression) => {
                            let item = current.take().ok_or_else(|| {
                                self.runtime_error("pipeline item was lost".into())
                            })?;
                            current = Some(self.evaluate_pipeline_item(item, expression)?);
                        }
                        PipelineStep::Take(limit) => {
                            take_counts[step_index] += 1;
                            stop_after_record |= take_counts[step_index] >= *limit;
                        }
                        PipelineStep::Skip(limit) => {
                            if skip_counts[step_index] < *limit {
                                skip_counts[step_index] += 1;
                                current = None;
                                break;
                            }
                        }
                        PipelineStep::StepBy(interval) => {
                            if step_by_counts[step_index] > 0 {
                                step_by_counts[step_index] -= 1;
                                current = None;
                                break;
                            }
                            step_by_counts[step_index] = interval - 1;
                        }
                        PipelineStep::TakeWhile(expression) => {
                            let item = current.take().ok_or_else(|| {
                                self.runtime_error("pipeline item was lost".into())
                            })?;
                            match self.evaluate_pipeline_item(item.clone(), expression)? {
                                Value::Bool(true) => current = Some(item),
                                Value::Bool(false) => {
                                    current = None;
                                    stop_source = true;
                                    break;
                                }
                                _ => {
                                    return Err(self.runtime_type_error(
                                        "pipeline `take_while` condition must return a boolean",
                                    ));
                                }
                            }
                        }
                        PipelineStep::DropWhile(expression) => {
                            if !drop_while_done[step_index] {
                                let item = current.take().ok_or_else(|| {
                                    self.runtime_error("pipeline item was lost".into())
                                })?;
                                match self.evaluate_pipeline_item(item.clone(), expression)? {
                                    Value::Bool(true) => {
                                        current = None;
                                        break;
                                    }
                                    Value::Bool(false) => {
                                        drop_while_done[step_index] = true;
                                        current = Some(item);
                                    }
                                    _ => {
                                        return Err(self.runtime_type_error(
                                            "pipeline `drop_while` condition must return a boolean",
                                        ));
                                    }
                                }
                            }
                        }
                        PipelineStep::Distinct => {
                            let item = current.take().ok_or_else(|| {
                                self.runtime_error("pipeline item was lost".into())
                            })?;
                            if seen_values[step_index].contains(&item) {
                                current = None;
                                break;
                            }
                            seen_values[step_index].push(item.clone());
                            current = Some(item);
                        }
                        PipelineStep::Sum
                        | PipelineStep::Count
                        | PipelineStep::Average
                        | PipelineStep::Min
                        | PipelineStep::Max
                        | PipelineStep::Any
                        | PipelineStep::All
                        | PipelineStep::Partition { .. }
                        | PipelineStep::WriteCsv(_) => {
                            unreachable!("terminal step is excluded from transforms")
                        }
                        PipelineStep::Chunk(_)
                        | PipelineStep::Parallel(_)
                        | PipelineStep::Checkpoint(_) => {}
                    }
                }
                let Some(value) = current else {
                    if stop_source {
                        break;
                    }
                    index += 1;
                    checkpoint::checkpoint_progress(
                        checkpoint_path.as_deref(),
                        index,
                        interval,
                        output.as_mut(),
                        input_path,
                        output_path.as_deref(),
                    )
                    .map_err(|error| self.checkpoint_runtime_error(error))?;
                    if stop_after_record {
                        break;
                    }
                    continue;
                };
                if let PipelineStep::Partition { item, rules } = terminal {
                    if let Some((category, value)) =
                        self.evaluate_partition_item(value, item, rules)?
                    {
                        let values = partitioned
                            .as_mut()
                            .and_then(|categories| categories.get_mut(&category))
                            .ok_or_else(|| {
                                self.runtime_error(format!(
                                    "partition category `{category}` was not initialized"
                                ))
                            })?;
                        values.push(value);
                    }
                    index += 1;
                    checkpoint::checkpoint_progress(
                        checkpoint_path.as_deref(),
                        index,
                        interval,
                        output.as_mut(),
                        input_path,
                        output_path.as_deref(),
                    )
                    .map_err(|error| self.checkpoint_runtime_error(error))?;
                    if stop_after_record {
                        break;
                    }
                    continue;
                }
                match terminal {
                    PipelineStep::Sum => {
                        sum.add(&self.heap, value, self.current_span.as_ref())?;
                    }
                    PipelineStep::Count => count += 1,
                    PipelineStep::Average => {
                        total = operations::binary(
                            &self.heap,
                            total,
                            &BinaryOperator::Add,
                            value,
                            self.current_span.as_ref(),
                        )?;
                        count += 1;
                    }
                    PipelineStep::Min => {
                        update_extreme(&mut minimum, value, false, self.current_span.as_ref())?
                    }
                    PipelineStep::Max => {
                        update_extreme(&mut maximum, value, true, self.current_span.as_ref())?
                    }
                    PipelineStep::Any => match value {
                        Value::Bool(value) => any_result |= value,
                        _ => {
                            return Err(self.runtime_type_error(
                                "`any` pipeline terminal requires boolean items",
                            ));
                        }
                    },
                    PipelineStep::All => match value {
                        Value::Bool(value) => all_result &= value,
                        _ => {
                            return Err(self.runtime_type_error(
                                "`all` pipeline terminal requires boolean items",
                            ));
                        }
                    },
                    PipelineStep::WriteCsv(_) => {
                        let values = match value.sequence_snapshot() {
                            Some(values) => values,
                            _ => {
                                return Err(self.runtime_type_error(
                                    "write_csv requires derive to produce a row collection",
                                ));
                            }
                        };
                        csv::write_row(
                            output
                                .as_mut()
                                .expect("write_csv output should be initialized"),
                            &values,
                        )
                        .map_err(|message| {
                            self.runtime_error_with_code(DiagnosticCode::RuntimeIo, message)
                        })?;
                        if stop_after_record {
                            break;
                        }
                    }
                    _ => unreachable!(),
                }
                if (matches!(terminal, PipelineStep::Any) && any_result)
                    || (matches!(terminal, PipelineStep::All) && !all_result)
                {
                    break;
                }
                index += 1;
                checkpoint::checkpoint_progress(
                    checkpoint_path.as_deref(),
                    index,
                    interval,
                    output.as_mut(),
                    input_path,
                    output_path.as_deref(),
                )
                .map_err(|error| self.checkpoint_runtime_error(error))?;
            }
            let terminal_result = if let Some(output) = output.as_mut() {
                output.flush().map_err(|error| {
                    self.runtime_error_with_code(
                        DiagnosticCode::RuntimeIo,
                        format!("could not flush CSV: {error}"),
                    )
                })?;
                Ok(Value::Unit)
            } else if matches!(terminal, PipelineStep::Count) {
                Ok(Value::Int(count))
            } else if matches!(terminal, PipelineStep::Average) {
                if count == 0 {
                    Err(self
                        .runtime_collection_error("average requires at least one numeric value"))
                } else {
                    Ok(Value::Float(
                        total_to_f64(total, self.current_span.as_ref())? / count as f64,
                    ))
                }
            } else if matches!(terminal, PipelineStep::Min) {
                minimum.ok_or_else(|| {
                    self.runtime_collection_error("min requires at least one numeric value")
                })
            } else if matches!(terminal, PipelineStep::Max) {
                maximum.ok_or_else(|| {
                    self.runtime_collection_error("max requires at least one numeric value")
                })
            } else if matches!(terminal, PipelineStep::Any) {
                Ok(Value::Bool(any_result))
            } else if matches!(terminal, PipelineStep::All) {
                Ok(Value::Bool(all_result))
            } else if let PipelineStep::Partition { .. } = terminal {
                match partitioned.take() {
                    Some(categories) => Ok(partition_result(&self.heap, categories)),
                    None => Err(self.runtime_error("partition result was not initialized".into())),
                }
            } else if matches!(terminal, PipelineStep::Sum) {
                Ok(sum.finish())
            } else {
                Ok(total)
            };
            if terminal_result.is_ok()
                && let Some(path) = checkpoint_path.as_deref()
            {
                checkpoint::remove_checkpoint(path)
                    .map_err(|error| self.checkpoint_runtime_error(error))?;
            }
            terminal_result
        })();
        self.pop_scope();
        result
    }

    pub(super) fn evaluate_pipeline_steps(
        &mut self,
        values: &mut Vec<Value>,
        steps: &[PipelineStep],
        sum_type: Type,
    ) -> Result<Value, SimplyError> {
        if steps.iter().any(|step| {
            matches!(
                step,
                PipelineStep::Chunk(_)
                    | PipelineStep::Checkpoint(_)
                    | PipelineStep::Take(_)
                    | PipelineStep::Skip(_)
                    | PipelineStep::StepBy(_)
                    | PipelineStep::TakeWhile(_)
                    | PipelineStep::Distinct
            )
        }) {
            return self.evaluate_streaming_pipeline_with_sum_type(
                std::mem::take(values),
                steps,
                sum_type,
            );
        }
        if matches!(steps.last(), Some(PipelineStep::Partition { .. })) {
            return self.evaluate_streaming_pipeline_with_sum_type(
                std::mem::take(values),
                steps,
                sum_type,
            );
        }
        if !matches!(
            steps.last(),
            Some(
                PipelineStep::Count
                    | PipelineStep::Sum
                    | PipelineStep::Average
                    | PipelineStep::Min
                    | PipelineStep::Max
                    | PipelineStep::Any
                    | PipelineStep::All
                    | PipelineStep::Partition { .. }
            )
        ) {
            return self.evaluate_streaming_pipeline_with_sum_type(
                std::mem::take(values),
                steps,
                sum_type,
            );
        }
        if steps.len() >= 2
            && matches!(
                steps.last(),
                Some(
                    PipelineStep::Count
                        | PipelineStep::Sum
                        | PipelineStep::Average
                        | PipelineStep::Min
                        | PipelineStep::Max
                )
            )
            && steps[..steps.len() - 1]
                .iter()
                .all(|step| matches!(step, PipelineStep::Where(_) | PipelineStep::Derive(_)))
        {
            return self.evaluate_fused_pipeline(
                values,
                &steps[..steps.len() - 1],
                steps.last(),
                sum_type,
            );
        }
        if steps.len() == 2 {
            match (&steps[0], &steps[1]) {
                (PipelineStep::Where(expression), PipelineStep::Count) => {
                    let mut count = 0;
                    for value in values.drain(..) {
                        match self.evaluate_pipeline_item(value, expression)? {
                            Value::Bool(true) => count += 1,
                            Value::Bool(false) => {}
                            _ => {
                                return Err(self.runtime_type_error(
                                    "pipeline `where` condition must return a boolean",
                                ));
                            }
                        }
                    }
                    return Ok(Value::Int(count));
                }
                (PipelineStep::Derive(expression), PipelineStep::Count) => {
                    let mut count = 0;
                    for value in values.drain(..) {
                        self.evaluate_pipeline_item(value, expression)?;
                        count += 1;
                    }
                    return Ok(Value::Int(count));
                }
                (PipelineStep::Derive(expression), PipelineStep::Sum) => {
                    let mut total = SumAccumulator::new(sum_type.clone());
                    for value in values.drain(..) {
                        let mapped = self.evaluate_pipeline_item(value, expression)?;
                        total.add(&self.heap, mapped, self.current_span.as_ref())?;
                    }
                    return Ok(total.finish());
                }
                _ => {}
            }
        }
        for step in steps {
            match step {
                PipelineStep::Where(expression) => {
                    let mut kept = Vec::new();
                    for value in values.drain(..) {
                        if let Some(item) = self.evaluate_pipeline_where_item(value, expression)? {
                            kept.push(item);
                        }
                    }
                    *values = kept;
                }
                PipelineStep::Derive(expression) => {
                    let mut mapped = Vec::new();
                    for value in values.drain(..) {
                        mapped.push(self.evaluate_pipeline_item(value, expression)?);
                    }
                    *values = mapped;
                }
                PipelineStep::Sum => {
                    let mut total = SumAccumulator::new(sum_type.clone());
                    for value in values.drain(..) {
                        total.add(&self.heap, value, self.current_span.as_ref())?;
                    }
                    return Ok(total.finish());
                }
                PipelineStep::Count => return Ok(Value::Int(values.len() as i64)),
                PipelineStep::Average | PipelineStep::Min | PipelineStep::Max => {
                    return self.evaluate_streaming_pipeline_with_sum_type(
                        std::mem::take(values),
                        steps,
                        sum_type.clone(),
                    );
                }
                PipelineStep::Any | PipelineStep::All => {
                    return self.evaluate_streaming_pipeline_with_sum_type(
                        std::mem::take(values),
                        steps,
                        sum_type.clone(),
                    );
                }
                PipelineStep::Partition { .. } => {
                    return self.evaluate_streaming_pipeline_with_sum_type(
                        std::mem::take(values),
                        steps,
                        sum_type.clone(),
                    );
                }
                PipelineStep::WriteCsv(_) => {
                    return Err(self.runtime_argument_error(
                        "`write_csv` requires a `csv_rows` streaming source",
                    ));
                }
                PipelineStep::Chunk(_)
                | PipelineStep::Parallel(_)
                | PipelineStep::Checkpoint(_)
                | PipelineStep::Take(_)
                | PipelineStep::Skip(_)
                | PipelineStep::StepBy(_)
                | PipelineStep::TakeWhile(_)
                | PipelineStep::DropWhile(_)
                | PipelineStep::Distinct => {
                    // These controls are consumed by the streaming path.
                }
            }
        }
        Ok(self.make_list(std::mem::take(values)))
    }

    pub(super) fn evaluate_streaming_pipeline_with_sum_type<I>(
        &mut self,
        values: I,
        steps: &[PipelineStep],
        sum_type: Type,
    ) -> Result<Value, SimplyError>
    where
        I: IntoIterator<Item = Value>,
    {
        if steps
            .iter()
            .any(|step| matches!(step, PipelineStep::Checkpoint(_)))
        {
            return Err(self.runtime_argument_error(
                "`checkpoint` requires a `csv_rows` source and `write_csv` terminal",
            ));
        }
        let terminal = match steps.last() {
            Some(PipelineStep::Count) => Some(PipelineStep::Count),
            Some(PipelineStep::Sum) => Some(PipelineStep::Sum),
            Some(PipelineStep::Average) => Some(PipelineStep::Average),
            Some(PipelineStep::Min) => Some(PipelineStep::Min),
            Some(PipelineStep::Max) => Some(PipelineStep::Max),
            Some(PipelineStep::Any) => Some(PipelineStep::Any),
            Some(PipelineStep::All) => Some(PipelineStep::All),
            Some(PipelineStep::Partition { item, rules }) => Some(PipelineStep::Partition {
                item: item.clone(),
                rules: rules.clone(),
            }),
            Some(PipelineStep::WriteCsv(path)) => Some(PipelineStep::WriteCsv(path.clone())),
            _ => None,
        };
        let transforms = if terminal.is_some() {
            &steps[..steps.len() - 1]
        } else {
            steps
        };
        let parallel_workers = steps
            .iter()
            .find_map(|step| match step {
                PipelineStep::Parallel(workers) => Some(*workers),
                _ => None,
            })
            .map(|workers| {
                usize::try_from(workers).map_err(|_| {
                    self.runtime_argument_error("parallel worker count is out of bounds")
                })
            })
            .transpose()?;
        let chunk_size = steps
            .iter()
            .find_map(|step| match step {
                PipelineStep::Chunk(size) => Some(*size),
                _ => None,
            })
            .map(|size| {
                usize::try_from(size)
                    .map_err(|_| self.runtime_argument_error("chunk size is out of bounds"))
            })
            .transpose()?;
        if let Some(workers) = parallel_workers {
            if workers == 0 {
                return Err(self.runtime_argument_error("parallel worker count must be positive"));
            }
            if workers > limits::MAX_PARALLEL_WORKERS {
                return Err(self.runtime_argument_error(format!(
                    "parallel worker count cannot exceed {}",
                    limits::MAX_PARALLEL_WORKERS
                )));
            }
        }
        if let Some(size) = chunk_size
            && (size == 0 || size > limits::MAX_CHUNK_SIZE)
        {
            return Err(self.runtime_argument_error(format!(
                "chunk size must be between 1 and {}",
                limits::MAX_CHUNK_SIZE
            )));
        }
        if let Some(workers) = parallel_workers {
            if !matches!(
                terminal,
                Some(
                    PipelineStep::Count
                        | PipelineStep::Sum
                        | PipelineStep::Average
                        | PipelineStep::Min
                        | PipelineStep::Max
                )
            ) {
                return Err(
                    self.runtime_argument_error("`parallel` requires an aggregate terminal")
                );
            }
            if !transforms.iter().all(|step| match step {
                PipelineStep::Where(expression) | PipelineStep::Derive(expression) => {
                    is_parallel_safe_expression(expression)
                }
                PipelineStep::Parallel(_) | PipelineStep::Chunk(_) => true,
                _ => false,
            }) {
                return Err(self.runtime_argument_error(
                    "`parallel` requires only parallel-safe `where` and `derive` expressions",
                ));
            }
            let input = values.into_iter().collect::<Vec<_>>();
            if input.iter().any(|value| {
                !matches!(
                    value,
                    Value::String(_) | Value::Int(_) | Value::Float(_) | Value::Bool(_)
                )
            }) {
                return Err(
                    self.runtime_collection_error("`parallel` requires scalar source items")
                );
            }
            match parallel::evaluate_parallel(input, transforms, workers, chunk_size) {
                Ok(output) => {
                    let sequential_steps: Vec<_> = steps
                        .last()
                        .filter(|step| {
                            matches!(
                                step,
                                PipelineStep::Count
                                    | PipelineStep::Sum
                                    | PipelineStep::Average
                                    | PipelineStep::Min
                                    | PipelineStep::Max
                            )
                        })
                        .cloned()
                        .into_iter()
                        .collect();
                    return self.evaluate_streaming_pipeline_with_sum_type(
                        output,
                        &sequential_steps,
                        sum_type,
                    );
                }
                Err(error) => {
                    return Err(self.runtime_error_with_code(error.code, error.message));
                }
            }
        }
        let mut output = Vec::new();
        let mut partitioned = match &terminal {
            Some(PipelineStep::Partition { rules, .. }) => Some(empty_partition_categories(rules)),
            _ => None,
        };
        let mut count = 0i64;
        let mut total = Value::Int(0);
        let mut sum = SumAccumulator::new(sum_type);
        let mut minimum = None;
        let mut maximum = None;
        let mut any_result = false;
        let mut all_result = true;
        let mut output_file = match terminal {
            Some(PipelineStep::WriteCsv(ref path_expression)) => {
                let path = match self.evaluate(path_expression)? {
                    Value::String(path) => path,
                    _ => return Err(self.runtime_type_error("write_csv path must be a string")),
                };
                Some(
                    fs::File::create(&path)
                        .map_err(|error| self.file_error(Path::new(&path), error))?,
                )
            }
            _ => None,
        };

        let mut take_counts = vec![0i64; transforms.len()];
        let mut skip_counts = vec![0i64; transforms.len()];
        let mut step_by_counts = vec![0i64; transforms.len()];
        let mut seen_values = vec![Vec::new(); transforms.len()];
        let mut drop_while_done = vec![false; transforms.len()];
        let mut values = values.into_iter();
        loop {
            if transforms
                .iter()
                .any(|step| matches!(step, PipelineStep::Take(0)))
            {
                break;
            }
            let Some(value) = values.next() else {
                break;
            };
            let mut current = Some(value);
            let mut stop_after_item = false;
            for (step_index, step) in transforms.iter().enumerate() {
                match step {
                    PipelineStep::Where(expression) => {
                        let item = current
                            .take()
                            .ok_or_else(|| self.runtime_error("pipeline item was lost".into()))?;
                        current = self.evaluate_pipeline_where_item(item, expression)?;
                        if current.is_none() {
                            break;
                        }
                    }
                    PipelineStep::Derive(expression) => {
                        let item = current
                            .take()
                            .ok_or_else(|| self.runtime_error("pipeline item was lost".into()))?;
                        current = Some(self.evaluate_pipeline_item(item, expression)?);
                    }
                    PipelineStep::Take(limit) => {
                        take_counts[step_index] += 1;
                        stop_after_item |= take_counts[step_index] >= *limit;
                    }
                    PipelineStep::Skip(limit) => {
                        if skip_counts[step_index] < *limit {
                            skip_counts[step_index] += 1;
                            current = None;
                            break;
                        }
                    }
                    PipelineStep::StepBy(interval) => {
                        if step_by_counts[step_index] > 0 {
                            step_by_counts[step_index] -= 1;
                            current = None;
                            break;
                        }
                        step_by_counts[step_index] = interval - 1;
                    }
                    PipelineStep::TakeWhile(expression) => {
                        let item = current
                            .take()
                            .ok_or_else(|| self.runtime_error("pipeline item was lost".into()))?;
                        match self.evaluate_pipeline_item(item.clone(), expression)? {
                            Value::Bool(true) => current = Some(item),
                            Value::Bool(false) => {
                                current = None;
                                stop_after_item = true;
                                break;
                            }
                            _ => {
                                return Err(self.runtime_type_error(
                                    "pipeline `take_while` condition must return a boolean",
                                ));
                            }
                        }
                    }
                    PipelineStep::DropWhile(expression) => {
                        if !drop_while_done[step_index] {
                            let item = current.take().ok_or_else(|| {
                                self.runtime_error("pipeline item was lost".into())
                            })?;
                            match self.evaluate_pipeline_item(item.clone(), expression)? {
                                Value::Bool(true) => {
                                    current = None;
                                    break;
                                }
                                Value::Bool(false) => {
                                    drop_while_done[step_index] = true;
                                    current = Some(item);
                                }
                                _ => {
                                    return Err(self.runtime_type_error(
                                        "pipeline `drop_while` condition must return a boolean",
                                    ));
                                }
                            }
                        }
                    }
                    PipelineStep::Distinct => {
                        let item = current
                            .take()
                            .ok_or_else(|| self.runtime_error("pipeline item was lost".into()))?;
                        if seen_values[step_index].contains(&item) {
                            current = None;
                            break;
                        }
                        seen_values[step_index].push(item.clone());
                        current = Some(item);
                    }
                    PipelineStep::Sum
                    | PipelineStep::Count
                    | PipelineStep::Average
                    | PipelineStep::Min
                    | PipelineStep::Max
                    | PipelineStep::Any
                    | PipelineStep::All
                    | PipelineStep::Partition { .. }
                    | PipelineStep::WriteCsv(_) => {
                        unreachable!()
                    }
                    PipelineStep::Chunk(_)
                    | PipelineStep::Parallel(_)
                    | PipelineStep::Checkpoint(_) => {}
                }
            }
            let Some(value) = current else {
                if stop_after_item {
                    break;
                }
                continue;
            };
            if let Some(PipelineStep::Partition { item, rules }) = &terminal {
                if let Some((category, value)) = self.evaluate_partition_item(value, item, rules)? {
                    let values = partitioned
                        .as_mut()
                        .and_then(|categories| categories.get_mut(&category))
                        .ok_or_else(|| {
                            self.runtime_error(format!(
                                "partition category `{category}` was not initialized"
                            ))
                        })?;
                    values.push(value);
                }
                if stop_after_item {
                    break;
                }
                continue;
            }
            match terminal {
                Some(PipelineStep::Count) => count += 1,
                Some(PipelineStep::Sum) => {
                    sum.add(&self.heap, value, self.current_span.as_ref())?;
                }
                Some(PipelineStep::Average) => {
                    total = operations::binary(
                        &self.heap,
                        total,
                        &BinaryOperator::Add,
                        value,
                        self.current_span.as_ref(),
                    )?;
                    count += 1;
                }
                Some(PipelineStep::Min) => {
                    update_extreme(&mut minimum, value, false, self.current_span.as_ref())?;
                }
                Some(PipelineStep::Max) => {
                    update_extreme(&mut maximum, value, true, self.current_span.as_ref())?;
                }
                Some(PipelineStep::Any) => match value {
                    Value::Bool(value) => any_result |= value,
                    _ => {
                        return Err(self
                            .runtime_type_error("`any` pipeline terminal requires boolean items"));
                    }
                },
                Some(PipelineStep::All) => match value {
                    Value::Bool(value) => all_result &= value,
                    _ => {
                        return Err(self
                            .runtime_type_error("`all` pipeline terminal requires boolean items"));
                    }
                },
                Some(PipelineStep::WriteCsv(_)) => {
                    let values = match value.sequence_snapshot() {
                        Some(values) => values,
                        _ => {
                            return Err(self.runtime_type_error(
                                "write_csv requires derive to produce a row collection",
                            ));
                        }
                    };
                    csv::write_row(
                        output_file
                            .as_mut()
                            .expect("write_csv output should be initialized"),
                        &values,
                    )
                    .map_err(|message| {
                        self.runtime_error_with_code(DiagnosticCode::RuntimeIo, message)
                    })?;
                }
                None => output.push(value),
                Some(PipelineStep::Where(_)) | Some(PipelineStep::Derive(_)) => unreachable!(),
                Some(PipelineStep::Chunk(_))
                | Some(PipelineStep::Parallel(_))
                | Some(PipelineStep::Checkpoint(_))
                | Some(PipelineStep::Take(_))
                | Some(PipelineStep::Skip(_))
                | Some(PipelineStep::StepBy(_))
                | Some(PipelineStep::DropWhile(_))
                | Some(PipelineStep::TakeWhile(_))
                | Some(PipelineStep::Distinct) => {
                    unreachable!()
                }
                Some(PipelineStep::Partition { .. }) => {
                    unreachable!("partition is handled before the terminal match")
                }
            }
            if (matches!(terminal, Some(PipelineStep::Any)) && any_result)
                || (matches!(terminal, Some(PipelineStep::All)) && !all_result)
            {
                break;
            }
            if stop_after_item {
                break;
            }
        }

        match terminal {
            Some(PipelineStep::Count) => Ok(Value::Int(count)),
            Some(PipelineStep::Sum) => Ok(sum.finish()),
            Some(PipelineStep::Average) => {
                if count == 0 {
                    Err(self
                        .runtime_collection_error("average requires at least one numeric value"))
                } else {
                    Ok(Value::Float(
                        total_to_f64(total, self.current_span.as_ref())? / count as f64,
                    ))
                }
            }
            Some(PipelineStep::Min) => minimum.ok_or_else(|| {
                self.runtime_collection_error("min requires at least one numeric value")
            }),
            Some(PipelineStep::Max) => maximum.ok_or_else(|| {
                self.runtime_collection_error("max requires at least one numeric value")
            }),
            Some(PipelineStep::Any) => Ok(Value::Bool(any_result)),
            Some(PipelineStep::All) => Ok(Value::Bool(all_result)),
            Some(PipelineStep::Partition { .. }) => match partitioned {
                Some(categories) => Ok(partition_result(&self.heap, categories)),
                None => Err(self.runtime_error("partition result was not initialized".into())),
            },
            Some(PipelineStep::WriteCsv(_)) => {
                if let Some(output_file) = output_file.as_mut() {
                    output_file.flush().map_err(|error| {
                        self.runtime_error_with_code(
                            DiagnosticCode::RuntimeIo,
                            format!("could not flush CSV: {error}"),
                        )
                    })?;
                }
                Ok(Value::Unit)
            }
            None => Ok(self.make_list(output)),
            Some(PipelineStep::Where(_)) | Some(PipelineStep::Derive(_)) => {
                unreachable!("pipeline terminal is normalized before evaluation")
            }
            Some(PipelineStep::Chunk(_))
            | Some(PipelineStep::Parallel(_))
            | Some(PipelineStep::Checkpoint(_))
            | Some(PipelineStep::Take(_))
            | Some(PipelineStep::Skip(_))
            | Some(PipelineStep::StepBy(_))
            | Some(PipelineStep::DropWhile(_))
            | Some(PipelineStep::TakeWhile(_))
            | Some(PipelineStep::Distinct) => {
                unreachable!("control steps are not terminals")
            }
        }
    }

    pub(super) fn evaluate_fused_pipeline(
        &mut self,
        values: &mut Vec<Value>,
        transforms: &[PipelineStep],
        terminal: Option<&PipelineStep>,
        sum_type: Type,
    ) -> Result<Value, SimplyError> {
        let mut total = Value::Int(0);
        let mut sum = SumAccumulator::new(sum_type);
        let mut count = 0i64;
        let mut minimum = None;
        let mut maximum = None;

        for value in values.drain(..) {
            let mut current = Some(value);
            for step in transforms {
                match step {
                    PipelineStep::Where(expression) => {
                        let item = current
                            .take()
                            .ok_or_else(|| self.runtime_error("pipeline item was lost".into()))?;
                        current = self.evaluate_pipeline_where_item(item, expression)?;
                        if current.is_none() {
                            break;
                        }
                    }
                    PipelineStep::Derive(expression) => {
                        let item = current
                            .take()
                            .ok_or_else(|| self.runtime_error("pipeline item was lost".into()))?;
                        current = Some(self.evaluate_pipeline_item(item, expression)?);
                    }
                    PipelineStep::Sum
                    | PipelineStep::Count
                    | PipelineStep::Average
                    | PipelineStep::Min
                    | PipelineStep::Max
                    | PipelineStep::Any
                    | PipelineStep::All
                    | PipelineStep::WriteCsv(_)
                    | PipelineStep::Partition { .. } => {
                        unreachable!()
                    }
                    PipelineStep::Chunk(_)
                    | PipelineStep::Parallel(_)
                    | PipelineStep::Checkpoint(_)
                    | PipelineStep::Take(_)
                    | PipelineStep::Skip(_)
                    | PipelineStep::StepBy(_)
                    | PipelineStep::DropWhile(_)
                    | PipelineStep::TakeWhile(_)
                    | PipelineStep::Distinct => {}
                }
            }

            let Some(value) = current else {
                continue;
            };
            match terminal {
                Some(PipelineStep::Count) => count += 1,
                Some(PipelineStep::Sum) => {
                    sum.add(&self.heap, value, self.current_span.as_ref())?;
                }
                Some(PipelineStep::Average) => {
                    total = operations::binary(
                        &self.heap,
                        total,
                        &BinaryOperator::Add,
                        value,
                        self.current_span.as_ref(),
                    )?;
                    count += 1;
                }
                Some(PipelineStep::Min) => {
                    update_extreme(&mut minimum, value, false, self.current_span.as_ref())?;
                }
                Some(PipelineStep::Max) => {
                    update_extreme(&mut maximum, value, true, self.current_span.as_ref())?;
                }
                _ => unreachable!(),
            }
        }

        match terminal {
            Some(PipelineStep::Count) => Ok(Value::Int(count)),
            Some(PipelineStep::Sum) => Ok(sum.finish()),
            Some(PipelineStep::Average) => {
                if count == 0 {
                    Err(self
                        .runtime_collection_error("average requires at least one numeric value"))
                } else {
                    Ok(Value::Float(
                        total_to_f64(total, self.current_span.as_ref())? / count as f64,
                    ))
                }
            }
            Some(PipelineStep::Min) => minimum.ok_or_else(|| {
                self.runtime_collection_error("min requires at least one numeric value")
            }),
            Some(PipelineStep::Max) => maximum.ok_or_else(|| {
                self.runtime_collection_error("max requires at least one numeric value")
            }),
            _ => unreachable!(),
        }
    }

    pub(super) fn evaluate_pipeline_where_item(
        &mut self,
        value: Value,
        expression: &Expr,
    ) -> Result<Option<Value>, SimplyError> {
        self.define("item".into(), value).map_err(|error| {
            self.runtime_error_with_code(DiagnosticCode::DuplicateDeclaration, error)
        })?;
        let result = self.evaluate(expression);
        let item = self
            .remove_current("item")
            .ok_or_else(|| self.runtime_error("pipeline item binding was lost".into()))?;
        match (result?, item) {
            (Value::Bool(true), item) => Ok(Some(item)),
            (Value::Bool(false), _) => Ok(None),
            _ => Err(self.runtime_type_error("pipeline `where` condition must return a boolean")),
        }
    }

    pub(super) fn evaluate_partition_item(
        &mut self,
        value: Value,
        item_name: &str,
        rules: &[PartitionRule],
    ) -> Result<Option<(String, Value)>, SimplyError> {
        for rule in rules {
            let matches = match &rule.condition {
                Some(condition) => {
                    self.define(item_name.into(), value.clone())
                        .map_err(|error| {
                            self.runtime_error_with_code(
                                DiagnosticCode::DuplicateDeclaration,
                                error,
                            )
                        })?;
                    let result = self.evaluate(condition);
                    self.remove_current(item_name).ok_or_else(|| {
                        self.runtime_error("partition item binding was lost".into())
                    })?;
                    match result? {
                        Value::Bool(matches) => matches,
                        _ => {
                            return Err(self
                                .runtime_type_error("partition condition must return a boolean"));
                        }
                    }
                }
                None => true,
            };
            if matches {
                return Ok(Some((rule.category.clone(), value)));
            }
        }
        Ok(None)
    }

    pub(super) fn evaluate_pipeline_item(
        &mut self,
        value: Value,
        expression: &Expr,
    ) -> Result<Value, SimplyError> {
        self.define("item".into(), value).map_err(|error| {
            self.runtime_error_with_code(DiagnosticCode::DuplicateDeclaration, error)
        })?;
        let result = self.evaluate(expression);
        self.remove_current("item")
            .ok_or_else(|| self.runtime_error("pipeline item binding was lost".into()))?;
        result
    }
}
