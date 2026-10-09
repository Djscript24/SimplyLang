use super::*;

impl Evaluator {
    pub(super) fn runtime_error(&self, message: String) -> SimplyError {
        self.runtime_error_with_code(DiagnosticCode::RuntimeGeneral, message)
    }

    pub(super) fn runtime_argument_error(&self, message: impl Into<String>) -> SimplyError {
        self.runtime_error_with_code(DiagnosticCode::RuntimeArgument, message)
    }

    pub(super) fn runtime_type_error(&self, message: impl Into<String>) -> SimplyError {
        self.runtime_error_with_code(DiagnosticCode::RuntimeTypeMismatch, message)
    }

    pub(super) fn runtime_collection_error(&self, message: impl Into<String>) -> SimplyError {
        self.runtime_error_with_code(DiagnosticCode::RuntimeCollection, message)
    }

    pub(super) fn checkpoint_runtime_error(
        &self,
        error: checkpoint::CheckpointError,
    ) -> SimplyError {
        self.runtime_error_with_code(error.code, error.message)
    }

    pub(super) fn single_character(
        &self,
        value: Value,
        operation: &str,
    ) -> Result<char, SimplyError> {
        let Value::String(value) = value else {
            return Err(
                self.runtime_type_error(format!("`{operation}` expects a one-character string"))
            );
        };
        let mut characters = value.chars();
        match (characters.next(), characters.next()) {
            (Some(character), None) => Ok(character),
            _ => Err(self.runtime_type_error(format!(
                "`{operation}` expects a string containing exactly one Unicode scalar value"
            ))),
        }
    }

    pub(super) fn evaluate_enumerate(&mut self, arguments: &[Expr]) -> Result<Value, SimplyError> {
        if arguments.len() != 1 {
            return Err(self.runtime_argument_error("`enumerate` expects one argument"));
        }
        let sequence = self.evaluate(&arguments[0])?;
        let pairs = match sequence {
            Value::Array(values) | Value::List(values) => enumerate_values(values.iter()),
            Value::Tuple(values) => enumerate_values(values.iter().cloned()),
            Value::Range { start, end, step } => {
                enumerate_values(Value::range_values(start, end, step))
            }
            Value::String(text) => enumerate_values(
                text.chars()
                    .map(|character| Value::String(character.to_string())),
            ),
            _ => {
                return Err(self.runtime_type_error(
                    "`enumerate` requires an array, list, tuple, range, or string",
                ));
            }
        }
        .map_err(|message| self.runtime_error_with_code(DiagnosticCode::RuntimeLimit, message))?;
        Ok(self.make_array(pairs))
    }

    pub(super) fn evaluate_zip(&mut self, arguments: &[Expr]) -> Result<Value, SimplyError> {
        if arguments.len() != 2 {
            return Err(self.runtime_argument_error("`zip` expects two arguments"));
        }
        let left = self.evaluate(&arguments[0])?;
        let right = self.evaluate(&arguments[1])?;
        let left = sequence_values(left).map_err(|message| self.runtime_type_error(message))?;
        let right = sequence_values(right).map_err(|message| self.runtime_type_error(message))?;
        let pairs = zip_values(left, right).map_err(|message| {
            self.runtime_error_with_code(DiagnosticCode::RuntimeLimit, message)
        })?;
        Ok(self.make_array(pairs))
    }

    pub(super) fn evaluate_map_keys_or_values(
        &mut self,
        name: &str,
        arguments: &[Expr],
    ) -> Result<Value, SimplyError> {
        if name == "select_keys" {
            if arguments.len() != 2 {
                return Err(
                    self.runtime_argument_error("`select_keys` expects a map and key sequence")
                );
            }
            let collection = self.evaluate(&arguments[0])?;
            let keys = match self.evaluate(&arguments[1])? {
                value @ (Value::Array(_) | Value::List(_) | Value::Tuple(_)) => {
                    value.sequence_snapshot().unwrap_or_default()
                }
                _ => {
                    return Err(self.runtime_type_error(
                        "`select_keys` expects an array, list, or tuple of strings",
                    ));
                }
            };
            let mut selected = std::collections::HashSet::with_capacity(keys.len());
            for key in keys.iter() {
                let Value::String(key) = key else {
                    return Err(self.runtime_type_error("`select_keys` keys must be strings"));
                };
                selected.insert(key.clone());
            }
            return match collection {
                Value::Hash(entries) => Ok(self.make_hash(
                    entries
                        .iter()
                        .filter(|(key, _)| selected.contains(key))
                        .collect(),
                )),
                _ => Err(self.runtime_type_error("`select_keys` requires a Hash")),
            };
        }
        if name == "without_key" {
            if arguments.len() != 2 {
                return Err(self.runtime_argument_error("`without_key` expects a map and key"));
            }
            let collection = self.evaluate(&arguments[0])?;
            let key = match self.evaluate(&arguments[1])? {
                Value::String(key) => key,
                _ => return Err(self.runtime_type_error("`without_key` key must be a string")),
            };
            return match collection {
                Value::Hash(entries) => {
                    let mut entries = entries.get_cloned().unwrap_or_default();
                    entries.remove(&key);
                    Ok(self.make_hash(entries))
                }
                _ => Err(self.runtime_type_error("`without_key` requires a Hash")),
            };
        }
        if name == "get" {
            if arguments.len() != 3 {
                return Err(
                    self.runtime_argument_error("`get` expects a map, key, and default value")
                );
            }
            let collection = self.evaluate(&arguments[0])?;
            let key = match self.evaluate(&arguments[1])? {
                Value::String(key) => key,
                _ => return Err(self.runtime_type_error("`get` key must be a string")),
            };
            let entries = match collection {
                Value::Hash(entries) => entries,
                _ => return Err(self.runtime_type_error("`get` requires a Hash")),
            };
            if let Some(value) = entries.get(&key) {
                return Ok(value.clone());
            }
            return self.evaluate(&arguments[2]);
        }
        if name == "has_key" {
            if arguments.len() != 2 {
                return Err(self.runtime_argument_error("`has_key` expects two arguments"));
            }
            let collection = self.evaluate(&arguments[0])?;
            let key = match self.evaluate(&arguments[1])? {
                Value::String(key) => key,
                _ => return Err(self.runtime_type_error("`has_key` key must be a string")),
            };
            let result = match collection {
                Value::Hash(entries) => entries.contains_key(&key),
                _ => return Err(self.runtime_type_error("`has_key` requires a Hash")),
            };
            return Ok(Value::Bool(result));
        }
        if arguments.len() != 1 {
            return Err(self.runtime_argument_error(format!("`{name}` expects one argument")));
        }
        let collection = self.evaluate(&arguments[0])?;
        let result = match collection {
            Value::Hash(entries) if name == "keys" => entries.keys().map(Value::String).collect(),
            Value::Hash(entries) if name == "entries" => entries
                .iter()
                .map(|(key, value)| Value::Tuple(shared_values(vec![Value::String(key), value])))
                .collect(),
            Value::Hash(entries) => entries.values().collect(),
            _ => {
                return Err(self.runtime_type_error(format!("`{name}` requires a Hash")));
            }
        };
        Ok(self.make_array(result))
    }

    pub(super) fn runtime_error_with_code(
        &self,
        code: DiagnosticCode,
        message: impl Into<String>,
    ) -> SimplyError {
        let message = message.into();
        let span = self.current_span.clone().unwrap_or_else(|| Span::new(0, 0));
        let code = match code {
            DiagnosticCode::TypeMismatch => DiagnosticCode::RuntimeTypeMismatch,
            DiagnosticCode::DuplicateDeclaration => DiagnosticCode::RuntimeDeclaration,
            DiagnosticCode::InvalidReassignment => DiagnosticCode::RuntimeMutability,
            DiagnosticCode::InvalidRefUsage => DiagnosticCode::InvalidRefUsage,
            code if code.category() == crate::error::DiagnosticCategory::Runtime => code,
            _ => DiagnosticCode::RuntimeGeneral,
        };
        SimplyError::Runtime {
            span,
            code,
            message,
        }
    }

    pub(super) fn ensure_type(
        &self,
        value: &Value,
        expected: &Type,
        name: &str,
    ) -> Result<(), SimplyError> {
        if *expected == Type::Unknown {
            return Ok(());
        }
        if self.value_matches_type(value, expected) {
            Ok(())
        } else {
            Err(self.runtime_error_with_code(
                DiagnosticCode::RuntimeTypeMismatch,
                format!(
                    "cannot assign a {} value to `{name}`: wrong type; expected {}, found {}; use `{name} as {} is ...` or provide a {} value",
                    Self::value_type_name(value),
                    Self::type_name(expected),
                    Self::value_type_name(value),
                    Self::type_name(expected),
                    Self::type_name(expected)
                ),
            ))
        }
    }

    pub(super) fn ensure_reassignment_type(
        &self,
        value: &Value,
        expected: &Type,
        name: &str,
    ) -> Result<(), SimplyError> {
        if *expected == Type::Unknown {
            return Ok(());
        }
        if self.value_matches_type(value, expected) {
            return Ok(());
        }

        Err(self.runtime_error_with_code(
            DiagnosticCode::RuntimeTypeMismatch,
            format!(
                "cannot reassign `{name}` with type {}; variable `{name}` remains type {}",
                Self::value_type_name(value),
                Self::type_name(expected)
            ),
        ))
    }

    pub(super) fn type_name(expected: &Type) -> String {
        expected.name()
    }

    pub(super) fn value_type_name(value: &Value) -> String {
        match value {
            Value::Unit => "Unit".into(),
            Value::String(_) => "String".into(),
            Value::Int(_) => "Int".into(),
            Value::Float(_) => "Float".into(),
            Value::Bool(_) => "Bool".into(),
            Value::Range { .. } => "Range".into(),
            Value::CsvStream { .. } => "CsvStream".into(),
            Value::Array(_) => "Array".into(),
            Value::List(_) => "List".into(),
            Value::Tuple(_) => "Tuple".into(),
            Value::Hash(_) => "Hash".into(),
            Value::Matrix(_) => "Matrix".into(),
            Value::Function(_) => "Function".into(),
            Value::Struct(instance) => instance
                .with(|instance| instance.type_name.clone())
                .unwrap_or_else(|| "<invalid struct>".into()),
            Value::Enum(value) => value
                .with(|value| value.enum_name.clone())
                .unwrap_or_else(|| "<invalid enum>".into()),
        }
    }

    pub(super) fn value_matches_type(&self, value: &Value, expected: &Type) -> bool {
        match (value, expected) {
            (Value::String(_), Type::String)
            | (Value::Int(_), Type::Int)
            | (Value::Float(_), Type::Float)
            | (Value::Bool(_), Type::Bool)
            | (Value::Unit, Type::Unit)
            | (Value::Hash(_), Type::Hash)
            | (Value::Function(_), Type::Function { .. }) => true,
            (Value::Matrix(_) | Value::Array(_) | Value::List(_), Type::Matrix) => {
                let rows = value.sequence_snapshot().unwrap_or_default();
                let mut expected_columns = None;
                !rows.is_empty()
                    && rows.iter().all(|row| match row {
                        Value::Array(_) | Value::List(_)
                            if !row.sequence_snapshot().unwrap_or_default().is_empty() =>
                        {
                            let values = row.sequence_snapshot().unwrap_or_default();
                            if expected_columns.is_some_and(|columns| columns != values.len()) {
                                return false;
                            }
                            expected_columns = Some(values.len());
                            values
                                .iter()
                                .all(|value| matches!(value, Value::Int(_) | Value::Float(_)))
                        }
                        _ => false,
                    })
            }
            (
                Value::Matrix(_) | Value::Array(_) | Value::List(_),
                Type::TypedMatrix(element, expected_rows, expected_columns),
            ) => {
                let rows = value.sequence_snapshot().unwrap_or_default();
                !rows.is_empty()
                    && rows.iter().all(|row| {
                        matches!(row, Value::Array(_) | Value::List(_))
                            && row.sequence_snapshot().is_some_and(|values| {
                                !values.is_empty()
                                    && values
                                        .iter()
                                        .all(|value| self.value_matches_type(value, element))
                            })
                    })
                    && expected_rows.is_none_or(|expected| rows.len() == expected)
                    && expected_columns.is_none_or(|expected| {
                        rows.first().is_some_and(|row| {
                            row.sequence_snapshot()
                                .is_some_and(|values| values.len() == expected)
                        })
                    })
                    && rows.first().is_some_and(|first_row| {
                        let Some(first_values) = first_row.sequence_snapshot() else {
                            return false;
                        };
                        let width = first_values.len();
                        rows.iter().all(|row| {
                            row.sequence_snapshot()
                                .is_some_and(|values| values.len() == width)
                        })
                    })
            }
            (Value::Struct(instance), Type::Struct(expected)) => instance
                .with(|instance| &instance.identity == expected)
                .unwrap_or(false),
            (Value::Enum(value), Type::Enum(expected)) => value
                .with(|value| &value.identity == expected)
                .unwrap_or(false),
            (Value::Range { .. }, Type::Range) => true,
            (Value::CsvStream { .. }, Type::CsvStream) => true,
            (Value::Array(_), Type::Array(element)) | (Value::List(_), Type::List(element))
                if **element == Type::Unknown =>
            {
                true
            }
            (Value::Array(values), Type::Array(element))
            | (Value::List(values), Type::List(element)) => values
                .iter()
                .all(|value| self.value_matches_type(&value, element)),
            (Value::Array(_), Type::Vector(element, expected_length))
            | (Value::List(_), Type::Vector(element, expected_length))
            | (Value::Tuple(_), Type::Vector(element, expected_length)) => {
                let values = value.sequence_snapshot().unwrap_or_default();
                expected_length.is_none_or(|expected| values.len() == expected)
                    && values
                        .iter()
                        .all(|value| self.value_matches_type(value, element))
            }
            (Value::Hash(values), Type::HashValues(element)) => values.values().all(|value| {
                **element == Type::Unknown || self.value_matches_type(&value, element)
            }),
            (Value::Tuple(values), Type::Tuple(types)) => {
                values.len() == types.len()
                    && values
                        .iter()
                        .zip(types)
                        .all(|(value, expected)| self.value_matches_type(value, expected))
            }
            _ => false,
        }
    }

    pub(super) fn type_of_value(&self, value: &Value) -> Type {
        match value {
            Value::String(_) => Type::String,
            Value::Int(_) => Type::Int,
            Value::Float(_) => Type::Float,
            Value::Bool(_) => Type::Bool,
            Value::Range { .. } => Type::Range,
            Value::CsvStream { .. } => Type::CsvStream,
            Value::Array(values) => Type::Vector(
                Box::new(
                    values
                        .first()
                        .map(|value| self.type_of_value(&value))
                        .unwrap_or(Type::Unknown),
                ),
                Some(values.len()),
            ),
            Value::List(values) => Type::List(Box::new(
                values
                    .first()
                    .map(|value| self.type_of_value(&value))
                    .unwrap_or(Type::Unknown),
            )),
            Value::Tuple(values) => Type::Tuple(
                values
                    .iter()
                    .map(|value| self.type_of_value(value))
                    .collect(),
            ),
            Value::Hash(values) => {
                let mut types = values.values().map(|value| self.type_of_value(&value));
                let first = types.next().unwrap_or(Type::Unknown);
                if types.all(|typ| typ.compatible_with(&first)) {
                    Type::HashValues(Box::new(first))
                } else {
                    Type::HashValues(Box::new(Type::Unknown))
                }
            }
            Value::Matrix(rows) => {
                let mut element_type = Type::Unknown;
                for row in rows.iter() {
                    let Some(values) = row.sequence_snapshot() else {
                        return Type::Matrix;
                    };
                    for value in values.iter() {
                        let value_type = self.type_of_value(value);
                        element_type = if element_type == Type::Unknown {
                            value_type
                        } else if value_type == Type::Float || element_type == Type::Float {
                            Type::Float
                        } else if value_type == element_type {
                            element_type
                        } else {
                            Type::Unknown
                        };
                    }
                }
                Type::TypedMatrix(
                    Box::new(element_type),
                    Some(rows.len()),
                    rows.first()
                        .and_then(Value::sequence_snapshot)
                        .map(|values| values.len()),
                )
            }
            Value::Struct(instance) => instance
                .with(|instance| Type::Struct(instance.identity.clone()))
                .unwrap_or(Type::Unknown),
            Value::Enum(value) => value
                .with(|value| Type::Enum(value.identity.clone()))
                .unwrap_or(Type::Unknown),
            Value::Function(function) => self
                .heap
                .function(*function)
                .map(|function| Type::Function {
                    parameters: function
                        .parameters
                        .iter()
                        .map(|(_, typ, _, _)| typ.clone().map(Box::new))
                        .collect(),
                    return_type: function
                        .return_type
                        .as_ref()
                        .map(|typ| Box::new(typ.clone())),
                })
                .unwrap_or(Type::Unknown),
            Value::Unit => Type::Unit,
        }
    }

    pub(super) fn erase_inferred_dimensions(typ: Type) -> Type {
        match typ {
            Type::Vector(element, _) => {
                Type::Vector(Box::new(Self::erase_inferred_dimensions(*element)), None)
            }
            Type::TypedMatrix(element, _, _) => Type::TypedMatrix(
                Box::new(Self::erase_inferred_dimensions(*element)),
                None,
                None,
            ),
            Type::Array(element) => {
                Type::Array(Box::new(Self::erase_inferred_dimensions(*element)))
            }
            Type::List(element) => Type::List(Box::new(Self::erase_inferred_dimensions(*element))),
            other => other,
        }
    }

    pub(super) fn sequence_sum_type(&self, expression: &Expr) -> Type {
        self.sequence_sum_type_from_type(&self.runtime_expression_type(expression, None))
    }

    pub(super) fn pipeline_sum_type(&self, source: &Expr, steps: &[PipelineStep]) -> Type {
        if !matches!(steps.last(), Some(PipelineStep::Sum)) {
            return Type::Unknown;
        }
        let mut item_type = self.sequence_item_type(&self.runtime_expression_type(source, None));
        for step in &steps[..steps.len() - 1] {
            if let PipelineStep::Derive(expression) = step {
                item_type = self.runtime_expression_type(expression, Some(&item_type));
            }
        }
        item_type
    }

    pub(super) fn sequence_item_type(&self, typ: &Type) -> Type {
        match typ {
            Type::Array(element) | Type::List(element) | Type::Vector(element, _) => {
                (**element).clone()
            }
            Type::Tuple(elements) => {
                if elements.contains(&Type::Float) {
                    Type::Float
                } else if elements.contains(&Type::Unknown) {
                    Type::Unknown
                } else {
                    Type::Int
                }
            }
            Type::Range => Type::Int,
            Type::CsvStream => Type::List(Box::new(Type::String)),
            _ => Type::Unknown,
        }
    }

    pub(super) fn sequence_sum_type_from_type(&self, typ: &Type) -> Type {
        match self.sequence_item_type(typ) {
            Type::Float => Type::Float,
            Type::Int => Type::Int,
            _ => Type::Unknown,
        }
    }

    pub(super) fn runtime_expression_type(
        &self,
        expression: &Expr,
        item_type: Option<&Type>,
    ) -> Type {
        match expression {
            Expr::Literal(literal) => match literal {
                Literal::String(_) => Type::String,
                Literal::Int(_) => Type::Int,
                Literal::Float(_) => Type::Float,
                Literal::Bool(_) => Type::Bool,
            },
            Expr::Identifier(name) if name == "item" => item_type.cloned().unwrap_or(Type::Unknown),
            Expr::Identifier(name) => self
                .variable_types
                .lookup(name)
                .cloned()
                .unwrap_or(Type::Unknown),
            Expr::Unary { operator, operand } => match operator {
                UnaryOperator::Negate => self.runtime_expression_type(operand, item_type),
                UnaryOperator::Not => Type::Bool,
                UnaryOperator::Transpose => self.runtime_expression_type(operand, item_type),
            },
            Expr::Binary {
                left,
                operator,
                right,
            } => {
                use BinaryOperator::{
                    Add, And, Divide, Equal, Greater, GreaterEqual, Less, LessEqual,
                    MatrixMultiply, Multiply, NotEqual, Or, Remainder, Subtract,
                };
                let left = self.runtime_expression_type(left, item_type);
                let right = self.runtime_expression_type(right, item_type);
                match operator {
                    Greater | GreaterEqual | Less | LessEqual | Equal | NotEqual | And | Or => {
                        Type::Bool
                    }
                    MatrixMultiply => Type::TypedMatrix(Box::new(Type::Float), None, None),
                    Add if left == Type::String && right == Type::String => Type::String,
                    Add | Divide | Multiply | Remainder | Subtract
                        if matches!(left, Type::Int | Type::Float)
                            && matches!(right, Type::Int | Type::Float) =>
                    {
                        if left == Type::Float || right == Type::Float {
                            Type::Float
                        } else {
                            Type::Int
                        }
                    }
                    _ => Type::Unknown,
                }
            }
            Expr::Call { name, arguments } => {
                if let Some(Type::Function {
                    return_type: Some(return_type),
                    ..
                }) = self.variable_types.lookup(name)
                {
                    return (**return_type).clone();
                }
                match name.as_str() {
                    "to_float" | "sqrt" | "exp" | "log" | "log10" | "sin" | "cos" | "tan"
                    | "floor" | "ceil" | "pow" | "norm" | "distance" | "mean" => Type::Float,
                    "to_int" | "sign" | "length" => Type::Int,
                    "type_of" | "read_file" | "to_json" | "to_json_pretty" | "trim"
                    | "substring" | "replace" | "regex_replace" | "join" => Type::String,
                    "regex_find_all" => Type::Array(Box::new(Type::String)),
                    "score_rules" => Type::Hash,
                    "parse_json" => Type::Unknown,
                    "read_json" => Type::Unknown,
                    "read_json_lines" => Type::Array(Box::new(Type::Unknown)),
                    "range" => Type::Range,
                    "csv_rows" => Type::CsvStream,
                    "total" if arguments.len() == 1 => self.sequence_sum_type(&arguments[0]),
                    "abs" | "round" if !arguments.is_empty() => {
                        self.runtime_expression_type(&arguments[0], item_type)
                    }
                    _ => Type::Unknown,
                }
            }
            Expr::Array(values) => Type::Vector(
                Box::new(self.collection_element_type(values, item_type)),
                Some(values.len()),
            ),
            Expr::List(values) => {
                Type::List(Box::new(self.collection_element_type(values, item_type)))
            }
            Expr::Tuple(values) => Type::Tuple(
                values
                    .iter()
                    .map(|value| self.runtime_expression_type(value, item_type))
                    .collect(),
            ),
            Expr::Index { target, .. } => match self.runtime_expression_type(target, item_type) {
                Type::Array(element) | Type::List(element) | Type::Vector(element, _) => *element,
                Type::Tuple(elements) if !elements.is_empty() => {
                    if elements.iter().all(|typ| typ == &elements[0]) {
                        elements[0].clone()
                    } else {
                        Type::Unknown
                    }
                }
                _ => Type::Unknown,
            },
            Expr::Matrix(rows) => {
                let mut element_type = Type::Unknown;
                for row in rows {
                    let row_type = self.runtime_expression_type(row, item_type);
                    let row_element = match row_type {
                        Type::Array(element) | Type::List(element) | Type::Vector(element, _) => {
                            *element
                        }
                        _ => return Type::Matrix,
                    };
                    element_type = if element_type == Type::Unknown {
                        row_element
                    } else if element_type == Type::Float || row_element == Type::Float {
                        Type::Float
                    } else if element_type == row_element {
                        element_type
                    } else {
                        Type::Unknown
                    };
                }
                Type::TypedMatrix(
                    Box::new(element_type),
                    Some(rows.len()),
                    rows.first().and_then(|row| {
                        match self.runtime_expression_type(row, item_type) {
                            Type::Vector(_, length) => length,
                            _ => None,
                        }
                    }),
                )
            }
            Expr::Pipeline { .. }
            | Expr::MessageDispatch { .. }
            | Expr::EnumVariant { .. }
            | Expr::Match { .. }
            | Expr::Field { .. }
            | Expr::Hash(_) => Type::Unknown,
        }
    }

    pub(super) fn collection_element_type(
        &self,
        values: &[Expr],
        item_type: Option<&Type>,
    ) -> Type {
        let mut element_type = Type::Unknown;
        for value in values {
            let value_type = self.runtime_expression_type(value, item_type);
            if element_type == Type::Unknown {
                element_type = value_type;
            } else if value_type == Type::Float && matches!(element_type, Type::Int | Type::Float) {
                element_type = Type::Float;
            } else if value_type != element_type {
                return Type::Unknown;
            }
        }
        element_type
    }
}
