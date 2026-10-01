use super::*;

impl Evaluator {
    pub(super) fn evaluate(&mut self, expr: &Expr) -> Result<Value, SimplyError> {
        match expr {
            Expr::Literal(literal) => Ok(match literal {
                Literal::String(value) => Value::String(value.clone()),
                Literal::Int(value) => Value::Int(*value),
                Literal::Float(value) => Value::Float(*value),
                Literal::Bool(value) => Value::Bool(*value),
            }),
            Expr::Array(values) => Ok(Value::Array(shared_values(self.evaluate_values(values)?))),
            Expr::List(values) => Ok(Value::List(shared_values(self.evaluate_values(values)?))),
            Expr::Tuple(values) => Ok(Value::Tuple(shared_values(self.evaluate_values(values)?))),
            Expr::Matrix(values) => Ok(Value::Matrix(shared_values(self.evaluate_values(values)?))),
            Expr::Hash(entries) => {
                let mut values = BTreeMap::new();
                for (name, expression) in entries {
                    values.insert(name.clone(), self.evaluate(expression)?);
                }
                Ok(Value::Hash(shared_map(values)))
            }
            Expr::Tree(entries) => {
                let mut values = BTreeMap::new();
                for (name, expression) in entries {
                    values.insert(name.clone(), self.evaluate(expression)?);
                }
                Ok(Value::Tree(shared_map(values)))
            }
            Expr::Pipeline { source, steps } => {
                let sum_type = self.pipeline_sum_type(source, steps);
                let result = match self.evaluate(source)? {
                    Value::CsvStream {
                        path,
                        start_record,
                        start_offset,
                        source_version,
                    } => self.evaluate_csv_pipeline(
                        &path,
                        start_record,
                        start_offset,
                        source_version.as_ref(),
                        steps,
                        sum_type.clone(),
                    )?,
                    Value::Range { start, end, step } => {
                        self.evaluate_range_pipeline(start, end, step, steps, sum_type.clone())?
                    }
                    Value::Array(values) | Value::List(values) => {
                        self.evaluate_pipeline(owned_values(values), steps, sum_type.clone())?
                    }
                    _ => {
                        return Err(self
                            .runtime_collection_error("pipeline source must be an array or list"));
                    }
                };
                Ok(result)
            }
            Expr::Identifier(name) => self.lookup(name).cloned().ok_or_else(|| {
                self.runtime_error_with_code(
                    DiagnosticCode::RuntimeName,
                    format!("unknown variable `{name}`"),
                )
            }),
            Expr::Unary { operator, operand } => {
                let value = self.evaluate(operand)?;
                operations::unary(value, operator, self.current_span.as_ref())
            }
            Expr::Binary {
                left,
                operator,
                right,
            } => {
                let left = self.evaluate(left)?;
                if matches!(operator, BinaryOperator::And | BinaryOperator::Or)
                    && let Value::Bool(value) = left
                    && ((*operator == BinaryOperator::And && !value)
                        || (*operator == BinaryOperator::Or && value))
                {
                    return Ok(Value::Bool(value));
                }
                let right = self.evaluate(right)?;
                operations::binary(left, operator, right, self.current_span.as_ref())
            }
            Expr::Call { name, arguments } => {
                if let Some(definition) = self.lookup_struct(name).cloned() {
                    let values = self.evaluate_values(arguments)?;
                    return self.construct_struct(name, &definition, values);
                }
                if name == "Ask" {
                    if !(1..=2).contains(&arguments.len()) {
                        return Err(self.runtime_error_with_code(
                            DiagnosticCode::RuntimeArgument,
                            "`Ask` expects a prompt and an optional type",
                        ));
                    }
                    let prompt = match self.evaluate(&arguments[0])? {
                        Value::String(prompt) => prompt,
                        _ => {
                            return Err(self.runtime_error_with_code(
                                DiagnosticCode::RuntimeTypeMismatch,
                                "`Ask` prompt must be a string",
                            ));
                        }
                    };
                    let target_type = match arguments.get(1) {
                        None => Type::String,
                        Some(Expr::Identifier(type_name)) => match type_name.as_str() {
                            "String" => Type::String,
                            "Int" => Type::Int,
                            "Float" => Type::Float,
                            "Bool" => Type::Bool,
                            _ => {
                                return Err(self.runtime_error_with_code(
                                    DiagnosticCode::RuntimeArgument,
                                    format!("unsupported `Ask` type `{type_name}`"),
                                ));
                            }
                        },
                        Some(_) => {
                            return Err(self.runtime_error_with_code(
                                DiagnosticCode::RuntimeArgument,
                                "`Ask` type must be `Int`, `Float`, `String`, or `Bool`",
                            ));
                        }
                    };
                    if self.output_enabled {
                        let mut stdout = io::stdout().lock();
                        stdout
                            .write_all(prompt.as_bytes())
                            .and_then(|()| stdout.flush())
                            .map_err(|error| {
                                self.runtime_error_with_code(
                                    DiagnosticCode::RuntimeIo,
                                    format!("could not write `Ask` prompt: {error}"),
                                )
                            })?;
                    }
                    let mut input = String::new();
                    let bytes_read = io::stdin().lock().read_line(&mut input).map_err(|error| {
                        self.runtime_error_with_code(
                            DiagnosticCode::RuntimeIo,
                            format!("could not read `Ask` input: {error}"),
                        )
                    })?;
                    if bytes_read == 0 {
                        return Err(self.runtime_error_with_code(
                            DiagnosticCode::RuntimeIo,
                            "no input received for `Ask`",
                        ));
                    }
                    let input = input.trim_end_matches(['\r', '\n']);
                    return match target_type {
                        Type::String => Ok(Value::String(input.to_owned())),
                        Type::Int => input.trim().parse::<i64>().map(Value::Int).map_err(|_| {
                            self.runtime_error_with_code(
                                DiagnosticCode::RuntimeConversion,
                                format!("`Ask` expected an Int, got `{input}`"),
                            )
                        }),
                        Type::Float => match input.trim().parse::<f64>() {
                            Ok(value) if value.is_finite() => Ok(Value::Float(value)),
                            _ => Err(self.runtime_error_with_code(
                                DiagnosticCode::RuntimeConversion,
                                format!("`Ask` expected a finite Float, got `{input}`"),
                            )),
                        },
                        Type::Bool if input.trim().eq_ignore_ascii_case("true") => {
                            Ok(Value::Bool(true))
                        }
                        Type::Bool if input.trim().eq_ignore_ascii_case("false") => {
                            Ok(Value::Bool(false))
                        }
                        Type::Bool => Err(self.runtime_error_with_code(
                            DiagnosticCode::RuntimeConversion,
                            format!("`Ask` expected Bool (`true` or `false`), got `{input}`"),
                        )),
                        _ => unreachable!("Ask target type is validated above"),
                    };
                }
                if is_math_builtin(name) {
                    let values = arguments
                        .iter()
                        .map(|argument| self.evaluate(argument))
                        .collect::<Result<Vec<_>, _>>()?;
                    return evaluate_math_builtin(name, &values, self.current_span.as_ref())?
                        .ok_or_else(|| {
                            self.runtime_error(format!("unknown numerical builtin `{name}`"))
                        });
                }
                if name == "print" {
                    if arguments.len() != 1 {
                        return Err(self.runtime_error_with_code(
                            DiagnosticCode::RuntimeArgument,
                            "`print` expects one argument",
                        ));
                    }
                    let value = self.evaluate(&arguments[0])?;
                    if self.output_enabled {
                        print_value(&value, true);
                    }
                    return Ok(Value::Unit);
                }
                if name == "assert" {
                    if !(1..=2).contains(&arguments.len()) {
                        return Err(self.runtime_error_with_code(
                            DiagnosticCode::RuntimeArgument,
                            "`assert` expects a condition and an optional message",
                        ));
                    }
                    let condition = self.evaluate(&arguments[0])?;
                    let Value::Bool(condition) = condition else {
                        return Err(self.runtime_error_with_code(
                            DiagnosticCode::RuntimeTypeMismatch,
                            "`assert` condition must be Bool",
                        ));
                    };
                    if condition {
                        return Ok(Value::Unit);
                    }
                    let message = match arguments.get(1) {
                        Some(expression) => match self.evaluate(expression)? {
                            Value::String(message) => message,
                            _ => {
                                return Err(self.runtime_error_with_code(
                                    DiagnosticCode::RuntimeTypeMismatch,
                                    "`assert` message must be a string",
                                ));
                            }
                        },
                        None => "assertion failed".into(),
                    };
                    return Err(
                        self.runtime_error_with_code(DiagnosticCode::RuntimeAssertion, message)
                    );
                }
                if name == "type_of" {
                    if arguments.len() != 1 {
                        return Err(self.runtime_error_with_code(
                            DiagnosticCode::RuntimeArgument,
                            "`type_of` expects one argument",
                        ));
                    }
                    let value = self.evaluate(&arguments[0])?;
                    let type_name = match value {
                        Value::Unit => "Unit",
                        Value::String(_) => "String",
                        Value::Int(_) => "Int",
                        Value::Float(_) => "Float",
                        Value::Bool(_) => "Bool",
                        Value::Range { .. } => "Range",
                        Value::CsvStream { .. } => "CsvStream",
                        Value::Array(_) => "Array",
                        Value::List(_) => "List",
                        Value::Tuple(_) => "Tuple",
                        Value::Hash(_) => "Hash",
                        Value::Tree(_) => "Tree",
                        Value::Matrix(_) => "Matrix",
                        Value::Function(_) => "Function",
                        Value::Struct(instance) => {
                            return Ok(Value::String(self.tracked_struct(&instance)?.type_name));
                        }
                        Value::Enum(value) => {
                            return Ok(Value::String(self.tracked_enum(&value)?.enum_name));
                        }
                    };
                    return Ok(Value::String(type_name.into()));
                }
                if name == "read_file" {
                    if arguments.len() != 1 {
                        return Err(self.runtime_error_with_code(
                            DiagnosticCode::RuntimeArgument,
                            "`read_file` expects one path",
                        ));
                    }
                    let path = match self.evaluate(&arguments[0])? {
                        Value::String(path) => path,
                        _ => {
                            return Err(self.runtime_error_with_code(
                                DiagnosticCode::RuntimeTypeMismatch,
                                "`read_file` path must be a string",
                            ));
                        }
                    };
                    return fs::read_to_string(&path)
                        .map(Value::String)
                        .map_err(|error| {
                            self.runtime_error_with_code(
                                DiagnosticCode::RuntimeIo,
                                format!("could not read file `{path}`: {error}"),
                            )
                        });
                }
                if name == "write_file" {
                    if arguments.len() != 2 {
                        return Err(self.runtime_error_with_code(
                            DiagnosticCode::RuntimeArgument,
                            "`write_file` expects a path and string content",
                        ));
                    }
                    let path = match self.evaluate(&arguments[0])? {
                        Value::String(path) => path,
                        _ => {
                            return Err(self.runtime_error_with_code(
                                DiagnosticCode::RuntimeTypeMismatch,
                                "`write_file` path must be a string",
                            ));
                        }
                    };
                    let content = match self.evaluate(&arguments[1])? {
                        Value::String(content) => content,
                        _ => {
                            return Err(self.runtime_error_with_code(
                                DiagnosticCode::RuntimeTypeMismatch,
                                "`write_file` content must be a string",
                            ));
                        }
                    };
                    files::atomic_write(Path::new(&path), content.as_bytes()).map_err(|error| {
                        self.runtime_error_with_code(
                            DiagnosticCode::RuntimeIo,
                            format!("could not write file `{path}`: {error}"),
                        )
                    })?;
                    return Ok(Value::Unit);
                }
                if name == "substring" {
                    if arguments.len() != 3 {
                        return Err(self.runtime_argument_error(
                            "`substring` expects a string, start index, and length",
                        ));
                    }
                    let value = match self.evaluate(&arguments[0])? {
                        Value::String(value) => value,
                        _ => return Err(self.runtime_type_error("`substring` expects a string")),
                    };
                    let start = match self.evaluate(&arguments[1])? {
                        Value::Int(start) if start >= 0 => {
                            usize::try_from(start).map_err(|_| {
                                self.runtime_error_with_code(
                                    DiagnosticCode::RuntimeLimit,
                                    "`substring` start is out of bounds",
                                )
                            })?
                        }
                        _ => {
                            return Err(self.runtime_argument_error(
                                "`substring` start must be a non-negative integer",
                            ));
                        }
                    };
                    let length = match self.evaluate(&arguments[2])? {
                        Value::Int(length) if length >= 0 => {
                            usize::try_from(length).map_err(|_| {
                                self.runtime_error_with_code(
                                    DiagnosticCode::RuntimeLimit,
                                    "`substring` length is out of bounds",
                                )
                            })?
                        }
                        _ => {
                            return Err(self.runtime_argument_error(
                                "`substring` length must be a non-negative integer",
                            ));
                        }
                    };
                    let character_count = value.chars().count();
                    if start > character_count || length > character_count - start {
                        return Err(self.runtime_argument_error("substring is out of bounds"));
                    }
                    return Ok(Value::String(
                        value.chars().skip(start).take(length).collect(),
                    ));
                }
                if name == "characters" {
                    if arguments.len() != 1 {
                        return Err(self.runtime_argument_error("`characters` expects one string"));
                    }
                    let value = match self.evaluate(&arguments[0])? {
                        Value::String(value) => value,
                        _ => {
                            return Err(self.runtime_type_error("`characters` expects a string"));
                        }
                    };
                    return Ok(Value::Array(shared_values(
                        value
                            .chars()
                            .map(|character| Value::String(character.to_string()))
                            .collect(),
                    )));
                }
                if matches!(
                    name.as_str(),
                    "is_ascii_alpha" | "is_ascii_digit" | "is_whitespace"
                ) {
                    if arguments.len() != 1 {
                        return Err(
                            self.runtime_argument_error(format!("`{name}` expects one character"))
                        );
                    }
                    let character = self.evaluate(&arguments[0])?;
                    let character = self.single_character(character, name)?;
                    return Ok(Value::Bool(match name.as_str() {
                        "is_ascii_alpha" => character.is_ascii_alphabetic(),
                        "is_ascii_digit" => character.is_ascii_digit(),
                        _ => character.is_whitespace(),
                    }));
                }
                if name == "contains" {
                    if arguments.len() != 2 {
                        return Err(self.runtime_argument_error("`contains` expects two arguments"));
                    }
                    let collection = self.evaluate(&arguments[0])?;
                    let searched = self.evaluate(&arguments[1])?;
                    let result = match collection {
                        Value::String(value) => match searched {
                            Value::String(searched) => value.contains(&searched),
                            _ => false,
                        },
                        Value::Array(values) | Value::List(values) | Value::Tuple(values) => {
                            values.iter().any(|value| value == &searched)
                        }
                        Value::Range { start, end, step } => {
                            matches!(searched, Value::Int(value)
                                if (step > 0 && value >= start && value < end
                                    || step < 0 && value <= start && value > end)
                                    && (i128::from(value) - i128::from(start))
                                        % i128::from(step)
                                        == 0)
                        }
                        Value::Hash(values) | Value::Tree(values) => {
                            values.values().any(|value| value == &searched)
                        }
                        _ => {
                            return Err(self.runtime_collection_error(
                                "`contains` requires a collection or string",
                            ));
                        }
                    };
                    return Ok(Value::Bool(result));
                }
                if (name == "keys"
                    || name == "values"
                    || name == "entries"
                    || name == "has_key"
                    || name == "get"
                    || name == "without_key"
                    || name == "select_keys")
                    && !matches!(self.lookup(name), Some(Value::Function(_)))
                {
                    return self.evaluate_map_keys_or_values(name, arguments);
                }
                if name == "any" || name == "all" {
                    if arguments.len() != 1 {
                        return Err(
                            self.runtime_argument_error(format!("`{name}` expects one argument"))
                        );
                    }
                    let collection = self.evaluate(&arguments[0])?;
                    let values = match collection {
                        Value::Array(values) | Value::List(values) | Value::Tuple(values) => values,
                        Value::Hash(values) | Value::Tree(values) => {
                            shared_values(values.values().cloned().collect())
                        }
                        _ => {
                            return Err(self.runtime_collection_error(format!(
                                "`{name}` requires a collection"
                            )));
                        }
                    };
                    let mut result = name == "all";
                    for value in values.iter() {
                        let Value::Bool(value) = value else {
                            return Err(self.runtime_type_error(format!(
                                "`{name}` requires a collection of booleans"
                            )));
                        };
                        if (name == "any" && *value) || (name == "all" && !*value) {
                            result = name == "any";
                            break;
                        }
                    }
                    return Ok(Value::Bool(result));
                }
                if name == "join" {
                    if arguments.len() != 2 {
                        return Err(self.runtime_argument_error("`join` expects two arguments"));
                    }
                    let collection = self.evaluate(&arguments[0])?;
                    let separator = match self.evaluate(&arguments[1])? {
                        Value::String(value) => value,
                        _ => {
                            return Err(
                                self.runtime_type_error("`join` separator must be a string")
                            );
                        }
                    };
                    let values = match collection {
                        Value::Array(values) | Value::List(values) | Value::Tuple(values) => values,
                        Value::Hash(values) | Value::Tree(values) => {
                            shared_values(values.values().cloned().collect())
                        }
                        _ => {
                            return Err(self.runtime_collection_error(
                                "`join` requires an array, list, or tuple",
                            ));
                        }
                    };
                    let mut parts = Vec::with_capacity(values.len());
                    for value in values.iter() {
                        let Value::String(value) = value else {
                            return Err(
                                self.runtime_type_error("`join` requires a collection of strings")
                            );
                        };
                        parts.push(value.as_str());
                    }
                    return Ok(Value::String(parts.join(&separator)));
                }
                if name == "total" {
                    if arguments.len() != 1 {
                        return Err(self.runtime_argument_error("`total` expects one argument"));
                    }
                    let sum_type = self.sequence_sum_type(&arguments[0]);
                    let collection = self.evaluate(&arguments[0])?;
                    let span = self.current_span.clone();
                    let mut total = SumAccumulator::new(sum_type);
                    let mut add_value =
                        |value| -> Result<(), SimplyError> { total.add(value, span.as_ref()) };
                    match collection {
                        Value::Array(values) | Value::List(values) | Value::Tuple(values) => {
                            for value in values.iter() {
                                add_value(value.clone())?;
                            }
                        }
                        Value::Range { start, end, step } => {
                            for value in Value::range_values(start, end, step) {
                                add_value(value)?;
                            }
                        }
                        _ => {
                            return Err(self.runtime_collection_error(
                                "`total` requires an array, list, tuple, or range",
                            ));
                        }
                    }
                    let total = total.finish();
                    return Ok(total);
                }
                if name == "trim" {
                    if arguments.len() != 1 {
                        return Err(self.runtime_argument_error("`trim` expects one argument"));
                    }
                    let value = match self.evaluate(&arguments[0])? {
                        Value::String(value) => value,
                        _ => return Err(self.runtime_type_error("`trim` expects a string")),
                    };
                    return Ok(Value::String(value.trim().into()));
                }
                if name == "to_float" {
                    if arguments.len() != 1 {
                        return Err(self.runtime_argument_error("`to_float` expects one argument"));
                    }
                    let value = match self.evaluate(&arguments[0])? {
                        Value::String(value) => value,
                        _ => {
                            return Err(self.runtime_type_error(
                                "`to_float` expects a string containing a number",
                            ));
                        }
                    };
                    let value = value.trim().parse::<f64>().map_err(|_| {
                        self.runtime_error_with_code(
                            DiagnosticCode::RuntimeConversion,
                            format!("cannot convert `{value}` to a Float"),
                        )
                    })?;
                    if !value.is_finite() {
                        return Err(self.runtime_error_with_code(
                            DiagnosticCode::RuntimeConversion,
                            "the converted Float must be finite",
                        ));
                    }
                    return Ok(Value::Float(value));
                }
                if name == "to_int" {
                    if arguments.len() != 1 {
                        return Err(self.runtime_argument_error("`to_int` expects one argument"));
                    }
                    let value = match self.evaluate(&arguments[0])? {
                        Value::String(value) => value,
                        _ => {
                            return Err(self.runtime_type_error(
                                "`to_int` expects a string containing a whole number",
                            ));
                        }
                    };
                    let value = value.trim().parse::<i64>().map_err(|_| {
                        self.runtime_error_with_code(
                            DiagnosticCode::RuntimeConversion,
                            format!("cannot convert `{value}` to an Int"),
                        )
                    })?;
                    return Ok(Value::Int(value));
                }
                if name == "abs" {
                    if arguments.len() != 1 {
                        return Err(
                            self.runtime_argument_error("`abs` expects one numeric argument")
                        );
                    }
                    return match self.evaluate(&arguments[0])? {
                        Value::Int(value) => value.checked_abs().map(Value::Int).ok_or_else(|| {
                            self.runtime_error_with_code(
                                DiagnosticCode::RuntimeArithmetic,
                                "integer absolute value overflow",
                            )
                        }),
                        Value::Float(value) if value.is_finite() => Ok(Value::Float(value.abs())),
                        _ => Err(self.runtime_type_error("`abs` expects an integer or float")),
                    };
                }
                if name == "round" {
                    if arguments.len() != 2 {
                        return Err(self
                            .runtime_argument_error("`round` expects a number and decimal count"));
                    }
                    let value = self.evaluate(&arguments[0])?;
                    let decimals = match self.evaluate(&arguments[1])? {
                        Value::Int(value) if (0..=15).contains(&value) => value as i32,
                        _ => {
                            return Err(self.runtime_argument_error(
                                "`round` decimal count must be an integer from 0 to 15",
                            ));
                        }
                    };
                    let factor = 10f64.powi(decimals);
                    return match value {
                        Value::Int(value) => Ok(Value::Int(value)),
                        Value::Float(value) if value.is_finite() => {
                            let rounded = (value * factor).round() / factor;
                            if rounded.is_finite() {
                                Ok(Value::Float(rounded))
                            } else {
                                Err(self.runtime_error_with_code(
                                    DiagnosticCode::RuntimeArithmetic,
                                    "`round` result is outside the finite Float range",
                                ))
                            }
                        }
                        _ => {
                            Err(self
                                .runtime_type_error("`round` expects an integer or finite float"))
                        }
                    };
                }
                if name == "clamp" {
                    if arguments.len() != 3 {
                        return Err(self.runtime_argument_error(
                            "`clamp` expects value, minimum, and maximum",
                        ));
                    }
                    let value = self.evaluate(&arguments[0])?;
                    let minimum = self.evaluate(&arguments[1])?;
                    let maximum = self.evaluate(&arguments[2])?;
                    return clamp_numeric(value, minimum, maximum, self.current_span.as_ref());
                }
                if name == "split" {
                    if arguments.len() != 2 {
                        return Err(self.runtime_argument_error("`split` expects two arguments"));
                    }
                    let value = match self.evaluate(&arguments[0])? {
                        Value::String(value) => value,
                        _ => return Err(self.runtime_type_error("`split` expects a string")),
                    };
                    let separator = match self.evaluate(&arguments[1])? {
                        Value::String(separator) => separator,
                        _ => {
                            return Err(
                                self.runtime_type_error("`split` separator must be a string")
                            );
                        }
                    };
                    return Ok(Value::List(shared_values(
                        value
                            .split(&separator)
                            .map(|part| Value::String(part.into()))
                            .collect(),
                    )));
                }
                if name == "replace" {
                    if arguments.len() != 3 {
                        return Err(
                            self.runtime_argument_error("`replace` expects three arguments")
                        );
                    }
                    let value = match self.evaluate(&arguments[0])? {
                        Value::String(value) => value,
                        _ => return Err(self.runtime_type_error("`replace` expects strings")),
                    };
                    let from = match self.evaluate(&arguments[1])? {
                        Value::String(from) => from,
                        _ => return Err(self.runtime_type_error("`replace` expects strings")),
                    };
                    let to = match self.evaluate(&arguments[2])? {
                        Value::String(to) => to,
                        _ => return Err(self.runtime_type_error("`replace` expects strings")),
                    };
                    return Ok(Value::String(value.replace(&from, &to)));
                }
                if name == "starts_with" || name == "ends_with" {
                    if arguments.len() != 2 {
                        return Err(
                            self.runtime_argument_error(format!("`{name}` expects two arguments"))
                        );
                    }
                    let value = match self.evaluate(&arguments[0])? {
                        Value::String(value) => value,
                        _ => {
                            return Err(
                                self.runtime_type_error(format!("`{name}` expects strings"))
                            );
                        }
                    };
                    let part = match self.evaluate(&arguments[1])? {
                        Value::String(part) => part,
                        _ => {
                            return Err(
                                self.runtime_type_error(format!("`{name}` expects strings"))
                            );
                        }
                    };
                    let result = if name == "starts_with" {
                        value.starts_with(&part)
                    } else {
                        value.ends_with(&part)
                    };
                    return Ok(Value::Bool(result));
                }
                if name == "is_empty" {
                    if arguments.len() != 1 {
                        return Err(self.runtime_argument_error("`is_empty` expects one argument"));
                    }
                    let value = self.evaluate(&arguments[0])?;
                    let result = match value {
                        Value::Array(values) | Value::List(values) | Value::Tuple(values) => {
                            values.is_empty()
                        }
                        Value::Range { start, end, step } => {
                            Value::range_len(start, end, step) == Some(0)
                        }
                        Value::Hash(values) | Value::Tree(values) => values.is_empty(),
                        Value::String(value) => value.is_empty(),
                        _ => {
                            return Err(self
                                .runtime_type_error("`is_empty` requires a collection or string"));
                        }
                    };
                    return Ok(Value::Bool(result));
                }
                if name == "reverse" {
                    if arguments.len() != 1 {
                        return Err(self.runtime_argument_error("`reverse` expects one argument"));
                    }
                    let value = self.evaluate(&arguments[0])?;
                    return Ok(match value {
                        Value::Array(values) => {
                            let mut values = owned_values(values);
                            values.reverse();
                            Value::Array(shared_values(values))
                        }
                        Value::List(values) => {
                            let mut values = owned_values(values);
                            values.reverse();
                            Value::List(shared_values(values))
                        }
                        Value::Tuple(values) => {
                            let mut values = owned_values(values);
                            values.reverse();
                            Value::Tuple(shared_values(values))
                        }
                        Value::Range { start, end, step } => {
                            let length = Value::range_len(start, end, step)
                                .and_then(|length| usize::try_from(length).ok())
                                .ok_or_else(|| {
                                    self.runtime_error_with_code(
                                        DiagnosticCode::RuntimeLimit,
                                        "range is too large to reverse",
                                    )
                                })?;
                            let mut values = Vec::new();
                            values.try_reserve_exact(length).map_err(|_| {
                                self.runtime_error_with_code(
                                    DiagnosticCode::RuntimeLimit,
                                    "range is too large to reverse",
                                )
                            })?;
                            values.extend(Value::range_values(start, end, step));
                            values.reverse();
                            Value::Array(shared_values(values))
                        }
                        _ => {
                            return Err(self.runtime_type_error(
                                "`reverse` requires an array, list, tuple, or range",
                            ));
                        }
                    });
                }
                if name == "enumerate" && !matches!(self.lookup(name), Some(Value::Function(_))) {
                    return self.evaluate_enumerate(arguments);
                }
                if name == "zip" && !matches!(self.lookup(name), Some(Value::Function(_))) {
                    return self.evaluate_zip(arguments);
                }
                if name == "length" || name == "count" {
                    if arguments.len() != 1 {
                        return Err(
                            self.runtime_argument_error(format!("`{name}` expects one argument"))
                        );
                    }
                    return match self.evaluate(&arguments[0])? {
                        Value::Array(values) | Value::List(values) | Value::Tuple(values) => {
                            Ok(Value::Int(values.len() as i64))
                        }
                        Value::Range { start, end, step } => Value::range_len(start, end, step)
                            .map(Value::Int)
                            .ok_or_else(|| {
                                self.runtime_error_with_code(
                                    DiagnosticCode::RuntimeArithmetic,
                                    "range length exceeds the Int range",
                                )
                            }),
                        Value::Hash(values) | Value::Tree(values) => {
                            Ok(Value::Int(values.len() as i64))
                        }
                        Value::String(value) => Ok(Value::Int(value.chars().count() as i64)),
                        _ => Err(self.runtime_type_error(format!(
                            "`{name}` requires a collection or string"
                        ))),
                    };
                }
                if name == "range" {
                    if !(2..=3).contains(&arguments.len()) {
                        return Err(self.runtime_argument_error(
                            "`range` expects start, end, and an optional step",
                        ));
                    }
                    let start = self.evaluate(&arguments[0])?;
                    let end = self.evaluate(&arguments[1])?;
                    let step = match arguments.get(2) {
                        Some(step) => self.evaluate(step)?,
                        None => Value::Int(1),
                    };
                    if let (Value::Int(start), Value::Int(end), Value::Int(step)) =
                        (start, end, step)
                    {
                        if step == 0 {
                            return Err(self.runtime_argument_error("`range` step cannot be zero"));
                        }
                        return Ok(Value::Range { start, end, step });
                    }
                    return Err(self.runtime_type_error(
                        "`range` expects integer bounds and an optional integer step",
                    ));
                }
                if name == "csv_rows" {
                    if arguments.len() != 1 {
                        return Err(self.runtime_argument_error("`csv_rows` expects one path"));
                    }
                    let path = match self.evaluate(&arguments[0])? {
                        Value::String(path) => path,
                        _ => {
                            return Err(self.runtime_type_error("`csv_rows` path must be a string"));
                        }
                    };
                    return Ok(Value::CsvStream {
                        path,
                        start_record: 0,
                        start_offset: 0,
                        source_version: None,
                    });
                }
                if name == "csv_row" {
                    let values = arguments
                        .iter()
                        .map(|argument| self.evaluate(argument))
                        .collect::<Result<Vec<_>, _>>()?;
                    return Ok(Value::List(shared_values(values)));
                }
                let values = arguments
                    .iter()
                    .map(|argument| self.evaluate(argument))
                    .collect::<Result<Vec<_>, _>>()?;
                self.invoke_function(name, values)
            }
            Expr::MessageDispatch {
                receiver,
                message,
                arguments,
            } => {
                let receiver = self.evaluate(receiver)?;
                let argument_values = arguments
                    .iter()
                    .map(|argument| self.evaluate(argument))
                    .collect::<Result<Vec<_>, _>>()?;
                self.dispatch_message(receiver, message, argument_values)
            }
            Expr::EnumVariant {
                enum_name,
                variant_name,
                arguments,
            } => {
                if let Some(definition) = self.lookup_enum(enum_name).cloned() {
                    let Some(variant) = definition
                        .variants
                        .iter()
                        .find(|variant| variant.name == *variant_name)
                    else {
                        return Err(self.runtime_error_with_code(
                            DiagnosticCode::RuntimeEnumVariant,
                            format!("unknown variant `{enum_name}::{variant_name}`"),
                        ));
                    };
                    let values = self.evaluate_values(arguments)?;
                    let payload = match (&variant.payload_type, values.as_slice()) {
                        (Some(expected), [value]) => {
                            self.ensure_type(value, expected, variant_name)?;
                            Some(Box::new(value.clone()))
                        }
                        (Some(expected), _) => {
                            return Err(self.runtime_error_with_code(
                                DiagnosticCode::InvalidFunctionCall,
                                format!(
                                    "variant `{enum_name}::{variant_name}` expects a payload of type {}",
                                    expected.name()
                                ),
                            ));
                        }
                        (None, []) => None,
                        (None, _) => {
                            return Err(self.runtime_error_with_code(
                                DiagnosticCode::InvalidFunctionCall,
                                format!(
                                    "unit variant `{enum_name}::{variant_name}` does not accept a payload"
                                ),
                            ));
                        }
                    };
                    Ok(Value::Enum(ArenaRef::insert(
                        self.enum_values.clone(),
                        EnumValue {
                            enum_name: enum_name.clone(),
                            identity: definition.identity,
                            variant_name: variant_name.clone(),
                            payload,
                        },
                    )))
                } else {
                    Err(self.runtime_error_with_code(
                        DiagnosticCode::RuntimeMessage,
                        format!("unknown enum type `{enum_name}`"),
                    ))
                }
            }
            Expr::Match { value, arms } => {
                let value = self.evaluate(value)?;
                self.evaluate_match(value, arms)
            }
            Expr::Index { target, index } => {
                let target = self.evaluate(target)?;
                let index_value = self.evaluate(index)?;
                collections::index(&target, &index_value, self.current_span.as_ref())
            }
            Expr::Field { target, name } => match self.evaluate(target)? {
                Value::Hash(values) | Value::Tree(values) => {
                    values.get(name).cloned().ok_or_else(|| {
                        self.runtime_collection_error(format!("unknown field `{name}`"))
                    })
                }
                _ => Err(self.runtime_type_error("value has no fields")),
            },
        }
    }
}
