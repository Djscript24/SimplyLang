use super::*;

impl SemanticAnalyzer {
    pub(super) fn analyze_expression(&mut self, expression: &Expr) -> Result<Type, SimplyError> {
        match expression {
            Expr::Literal(Literal::String(_)) => Ok(Type::String),
            Expr::Literal(Literal::Int(_)) => Ok(Type::Int),
            Expr::Literal(Literal::Float(_)) => Ok(Type::Float),
            Expr::Literal(Literal::Bool(_)) => Ok(Type::Bool),
            Expr::Identifier(name) => {
                if let Some(typ) = self.variables.get(name).cloned() {
                    if matches!(typ, Type::Function { .. }) {
                        self.check_function_reference_order(name)?;
                    }
                    Ok(typ)
                } else if let Some(function) = self
                    .function_scopes
                    .iter()
                    .rev()
                    .find_map(|scope| scope.get(name))
                {
                    if function.ref_parameters.iter().any(|by_ref| *by_ref) {
                        return Err(self.error(
                            DiagnosticCode::InvalidRefUsage,
                            "a function with `ref` parameters cannot be used as a value",
                        ));
                    }
                    self.check_function_reference_order(name)?;
                    Ok(Type::Function {
                        parameters: function
                            .parameters
                            .iter()
                            .map(|parameter| parameter.clone().map(Box::new))
                            .collect(),
                        return_type: function.return_type.clone().map(Box::new),
                    })
                } else {
                    Err(self.error(
                        DiagnosticCode::UndefinedVariable,
                        format!("unknown variable `{name}`"),
                    ))
                }
            }
            Expr::Array(values) => self.collection_type(values, true),
            Expr::List(values) => self.collection_type(values, false),
            Expr::Tuple(values) => Ok(Type::Tuple(
                values
                    .iter()
                    .map(|value| self.analyze_expression(value))
                    .collect::<Result<_, _>>()?,
            )),
            Expr::Matrix(values) => {
                let mut element_type = Type::Unknown;
                let mut column_count = None;
                for value in values {
                    let row_type = self.analyze_expression(value)?;
                    let row_length = Self::vector_length(&row_type);
                    let row_element = match row_type {
                        Type::Array(element) | Type::List(element) | Type::Vector(element, _) => {
                            *element
                        }
                        Type::Unknown => Type::Unknown,
                        _ => {
                            return Err(self.error(
                                DiagnosticCode::SemanticCollection,
                                "matrix rows must be numeric sequences",
                            ));
                        }
                    };
                    self.require_numeric(&row_element)?;
                    element_type = Self::merge_numeric_type(&element_type, &row_element);
                    if let Some(row_length) = row_length {
                        if column_count.is_some_and(|columns| columns != row_length) {
                            return Err(self.error(
                                DiagnosticCode::SemanticCollection,
                                "matrix rows must have equal widths",
                            ));
                        }
                        column_count = Some(row_length);
                    }
                }
                Ok(Type::TypedMatrix(
                    Box::new(element_type),
                    Some(values.len()),
                    column_count,
                ))
            }
            Expr::Hash(entries) => {
                let mut value_type = None;
                for (_, value) in entries {
                    let actual = self.analyze_expression(value)?;
                    value_type = Some(match value_type {
                        Some(current) => Self::merge_collection_type(&current, &actual),
                        None => actual,
                    });
                }
                Ok(Type::HashValues(Box::new(
                    value_type.unwrap_or(Type::Unknown),
                )))
            }
            Expr::Unary { operator, operand } => {
                let operand_type = self.analyze_expression(operand)?;
                match operator {
                    UnaryOperator::Not => {
                        self.require_type(&Type::Bool, &operand_type)?;
                        Ok(Type::Bool)
                    }
                    UnaryOperator::Negate => {
                        self.require_numeric(&operand_type)?;
                        Ok(operand_type)
                    }
                    UnaryOperator::Transpose => {
                        self.require_type(&Type::Matrix, &operand_type)?;
                        Ok(operand_type)
                    }
                }
            }
            Expr::Binary {
                left,
                operator,
                right,
            } => {
                let left_type = self.analyze_expression(left)?;
                if matches!(
                    (operator, left.as_ref()),
                    (BinaryOperator::And, Expr::Literal(Literal::Bool(false)))
                        | (BinaryOperator::Or, Expr::Literal(Literal::Bool(true)))
                ) {
                    return Ok(Type::Bool);
                }
                let right_type = self.analyze_expression(right)?;
                self.binary_type(operator, &left_type, &right_type)
            }
            Expr::Call { name, arguments } => self.call_type(name, arguments),
            Expr::MessageDispatch {
                receiver,
                message,
                arguments,
            } => self.message_type(receiver, message, arguments),
            Expr::EnumVariant {
                enum_name,
                variant_name,
                arguments,
            } => {
                if self.enums.contains_key(enum_name) {
                    self.enum_variant_type(enum_name, variant_name, arguments)
                } else if self.variables.get(enum_name).is_none() {
                    Err(self.error(
                        DiagnosticCode::UndefinedVariable,
                        format!("unknown enum type `{enum_name}`"),
                    ))
                } else {
                    self.message_type(
                        &Expr::Identifier(enum_name.clone()),
                        variant_name,
                        arguments,
                    )
                }
            }
            Expr::Match { value, arms } => self.match_type(value, arms),
            Expr::Index { target, index } => {
                let target_type = self.analyze_expression(target)?;
                let index_type = self.analyze_expression(index)?;
                self.index_type(&target_type, &index_type, index)
            }
            Expr::Field { target, name } => {
                let target_type = self.analyze_expression(target)?;
                match target_type {
                    Type::Hash | Type::Unknown => Ok(Type::Unknown),
                    Type::HashValues(value) => Ok(*value),
                    _ => Err(self.error(
                        DiagnosticCode::SemanticField,
                        format!("value has no field `{name}`"),
                    )),
                }
            }
            Expr::Pipeline { source, steps } => self.pipeline_type(source, steps),
        }
    }

    pub(super) fn collection_type(
        &mut self,
        values: &[Expr],
        array: bool,
    ) -> Result<Type, SimplyError> {
        let mut element = None;
        for value in values {
            let actual = self.analyze_expression(value)?;
            element = Some(match element {
                Some(current) => Self::merge_compatible_types(&current, &actual)
                    .ok_or_else(|| self.type_error(&current, &actual, "collection element"))?,
                None => actual,
            });
        }
        let element = element.unwrap_or(Type::Unknown);
        Ok(if array {
            Type::Vector(Box::new(element), Some(values.len()))
        } else {
            Type::List(Box::new(element))
        })
    }

    pub(super) fn merge_collection_type(current: &Type, actual: &Type) -> Type {
        Self::merge_compatible_types(current, actual).unwrap_or(Type::Unknown)
    }

    pub(super) fn merge_compatible_types(current: &Type, actual: &Type) -> Option<Type> {
        if current == &Type::Unknown || actual == &Type::Unknown {
            return Some(Type::Unknown);
        }
        match (current, actual) {
            (
                Type::Vector(current_element, current_length),
                Type::Vector(actual_element, actual_length),
            ) => {
                let element = Self::merge_compatible_types(current_element, actual_element)?;
                let length = if current_length == actual_length {
                    *current_length
                } else {
                    None
                };
                Some(Type::Vector(Box::new(element), length))
            }
            _ if actual.compatible_with(current) => Some(current.clone()),
            _ if current.compatible_with(actual) => Some(actual.clone()),
            _ => None,
        }
    }

    pub(super) fn merge_numeric_type(current: &Type, actual: &Type) -> Type {
        if current == &Type::Unknown {
            actual.clone()
        } else if actual == &Type::Unknown {
            current.clone()
        } else if current == &Type::Float || actual == &Type::Float {
            Type::Float
        } else {
            Type::Int
        }
    }

    pub(super) fn matrix_element_type(typ: &Type) -> Type {
        match typ {
            Type::TypedMatrix(element, _, _) => (**element).clone(),
            Type::Vector(row, _) => match row.as_ref() {
                Type::Vector(element, _) | Type::Array(element) | Type::List(element) => {
                    (**element).clone()
                }
                _ => Type::Unknown,
            },
            _ => Type::Unknown,
        }
    }

    pub(super) fn matrix_dimensions(typ: &Type) -> (Option<usize>, Option<usize>) {
        match typ {
            Type::TypedMatrix(_, rows, columns) => (*rows, *columns),
            Type::Vector(row, rows) => match row.as_ref() {
                Type::Vector(_, columns) => (*rows, *columns),
                _ => (*rows, None),
            },
            _ => (None, None),
        }
    }

    pub(super) fn is_matrix_type(typ: &Type) -> bool {
        matches!(typ, Type::Matrix | Type::TypedMatrix(_, _, _))
            || matches!(typ, Type::Vector(row, _) if matches!(row.as_ref(), Type::Vector(_, _) | Type::Array(_) | Type::List(_)))
    }

    pub(super) fn require_rectangular_matrix_literal(
        &self,
        expression: &Expr,
    ) -> Result<(), SimplyError> {
        let rows = match expression {
            Expr::Array(rows) | Expr::List(rows) | Expr::Matrix(rows) => rows,
            _ => return Ok(()),
        };
        let mut expected_width = None;
        for row in rows {
            let width = match row {
                Expr::Array(values) | Expr::List(values) => values.len(),
                _ => return Ok(()),
            };
            if expected_width.is_some_and(|expected| expected != width) {
                return Err(self.error(
                    DiagnosticCode::SemanticCollection,
                    "matrix rows must have equal widths",
                ));
            }
            expected_width = Some(width);
        }
        Ok(())
    }

    pub(super) fn vector_length(typ: &Type) -> Option<usize> {
        match typ {
            Type::Vector(_, length) => *length,
            Type::Tuple(values) => Some(values.len()),
            _ => None,
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

    pub(super) fn dimensions_match(
        left: (Option<usize>, Option<usize>),
        right: (Option<usize>, Option<usize>),
    ) -> bool {
        left.0
            .is_none_or(|value| right.0.is_none_or(|other| value == other))
            && left
                .1
                .is_none_or(|value| right.1.is_none_or(|other| value == other))
    }

    pub(super) fn require_square_dimensions(
        &self,
        name: &str,
        shape: (Option<usize>, Option<usize>),
    ) -> Result<(), SimplyError> {
        if shape.0.is_some() && shape.1.is_some() && shape.0 != shape.1 {
            return Err(self.error(
                DiagnosticCode::SemanticCollection,
                format!("`{name}` requires a square matrix"),
            ));
        }
        Ok(())
    }

    pub(super) fn require_matching_vector_lengths(
        &self,
        name: &str,
        left: Option<usize>,
        right: Option<usize>,
    ) -> Result<(), SimplyError> {
        if left.is_some() && right.is_some() && left != right {
            return Err(self.error(
                DiagnosticCode::SemanticCollection,
                format!("`{name}` requires matching vector dimensions"),
            ));
        }
        Ok(())
    }

    pub(super) fn binary_type(
        &self,
        operator: &BinaryOperator,
        left: &Type,
        right: &Type,
    ) -> Result<Type, SimplyError> {
        use BinaryOperator::*;
        if left == &Type::Unknown || right == &Type::Unknown {
            return Ok(Type::Unknown);
        }
        match operator {
            Add if Self::is_matrix_type(left) && Self::is_matrix_type(right) => {
                let left_shape = Self::matrix_dimensions(left);
                let right_shape = Self::matrix_dimensions(right);
                if !Self::dimensions_match(left_shape, right_shape) {
                    return Err(self.error(
                        DiagnosticCode::SemanticCollection,
                        "matrix addition requires matching dimensions",
                    ));
                }
                Ok(Type::TypedMatrix(
                    Box::new(Self::merge_numeric_type(
                        &Self::matrix_element_type(left),
                        &Self::matrix_element_type(right),
                    )),
                    left_shape.0,
                    left_shape.1,
                ))
            }
            Add if left == &Type::String && right == &Type::String => Ok(Type::String),
            Add => self.numeric_result(left, right),
            Subtract | Multiply | Divide | Remainder => self.numeric_result(left, right),
            MatrixMultiply => {
                self.require_type(&Type::Matrix, left)?;
                self.require_type(&Type::Matrix, right)?;
                let left_shape = Self::matrix_dimensions(left);
                let right_shape = Self::matrix_dimensions(right);
                if left_shape.1.is_some()
                    && right_shape.0.is_some()
                    && left_shape.1 != right_shape.0
                {
                    return Err(self.error(
                        DiagnosticCode::SemanticCollection,
                        "matrix multiplication dimensions do not match",
                    ));
                }
                Ok(Type::TypedMatrix(
                    Box::new(Type::Float),
                    left_shape.0,
                    right_shape.1,
                ))
            }
            Greater | GreaterEqual | Less | LessEqual => {
                self.require_numeric(left)?;
                self.require_numeric(right)?;
                Ok(Type::Bool)
            }
            Range => {
                self.require_type(&Type::Int, left)?;
                self.require_type(&Type::Int, right)?;
                Ok(Type::Range)
            }
            In => {
                if !matches!(
                    right,
                    Type::String
                        | Type::Range
                        | Type::Array(_)
                        | Type::List(_)
                        | Type::Vector(_, _)
                        | Type::Tuple(_)
                        | Type::Hash
                        | Type::HashValues(_)
                        | Type::Unknown
                ) {
                    return Err(self.error(
                        DiagnosticCode::SemanticCollection,
                        format!(
                            "`in` requires a collection, range, or string, found {}",
                            right.name()
                        ),
                    ));
                }
                if right == &Type::String {
                    self.require_type(&Type::String, left)?;
                } else if right == &Type::Range {
                    self.require_type(&Type::Int, left)?;
                }
                Ok(Type::Bool)
            }
            Equal | NotEqual => Ok(Type::Bool),
            And | Or => {
                self.require_type(&Type::Bool, left)?;
                self.require_type(&Type::Bool, right)?;
                Ok(Type::Bool)
            }
        }
    }

    pub(super) fn call_type(
        &mut self,
        name: &str,
        arguments: &[Expr],
    ) -> Result<Type, SimplyError> {
        self.check_function_reference_order(name)?;
        if let Some(fields) = self.structs.get(name).cloned() {
            let argument_types = arguments
                .iter()
                .map(|argument| self.analyze_expression(argument))
                .collect::<Result<Vec<_>, _>>()?;
            self.validate_ref_arguments(
                arguments,
                &argument_types,
                &vec![false; fields.len()],
                &vec![false; fields.len()],
                name,
                false,
            )?;
            if fields.len() != argument_types.len() {
                return Err(self.error(
                    DiagnosticCode::InvalidFunctionCall,
                    format!(
                        "struct `{name}` expects {} fields, got {}",
                        fields.len(),
                        argument_types.len()
                    ),
                ));
            }
            for (field, actual) in fields.iter().zip(&argument_types) {
                self.require_type(&field.field_type, actual)?;
            }
            return Ok(Type::Struct(self.struct_identities[name].clone()));
        }
        let user_function = self
            .function_scopes
            .iter()
            .rev()
            .find_map(|scope| scope.get(name))
            .cloned();
        let variable_function = match self.variables.get(name) {
            Some(Type::Function {
                parameters,
                return_type,
            }) => Some((parameters.clone(), return_type.clone())),
            _ => None,
        };
        if user_function.is_some() || variable_function.is_some() {
            let argument_types = arguments
                .iter()
                .map(|argument| self.analyze_expression(argument))
                .collect::<Result<Vec<_>, _>>()?;
            if let Some(function) = user_function {
                self.validate_ref_arguments(
                    arguments,
                    &argument_types,
                    &function.ref_parameters,
                    &function.mut_ref_parameters,
                    name,
                    true,
                )?;
                if function.parameters.len() != argument_types.len() {
                    return Err(self.error(
                        DiagnosticCode::InvalidFunctionCall,
                        format!(
                            "function `{name}` expects {} arguments, got {}",
                            function.parameters.len(),
                            argument_types.len()
                        ),
                    ));
                }
                for (expected, actual) in function.parameters.iter().zip(&argument_types) {
                    if let Some(expected) = expected {
                        self.require_type(expected, actual)?;
                    }
                }
                return Ok(function.return_type.unwrap_or(Type::Unit));
            }
            let (parameters, return_type) =
                variable_function.expect("function presence was checked above");
            self.validate_ref_arguments(
                arguments,
                &argument_types,
                &vec![false; parameters.len()],
                &vec![false; parameters.len()],
                name,
                true,
            )?;
            if parameters.len() != argument_types.len() {
                return Err(self.error(
                    DiagnosticCode::InvalidFunctionCall,
                    format!(
                        "function `{name}` expects {} arguments, got {}",
                        parameters.len(),
                        argument_types.len()
                    ),
                ));
            }
            for (expected, actual) in parameters.iter().zip(&argument_types) {
                if let Some(expected) = expected {
                    self.require_type(expected, actual)?;
                }
            }
            return Ok(return_type.as_deref().cloned().unwrap_or(Type::Unknown));
        }
        if name == "Ask" {
            if !(1..=2).contains(&arguments.len()) {
                return Err(self.error(
                    DiagnosticCode::InvalidFunctionCall,
                    "`Ask` expects a prompt and an optional type (`Int`, `Float`, `String`, or `Bool`)",
                ));
            }
            let prompt_type = self.analyze_expression(&arguments[0])?;
            self.require_type(&Type::String, &prompt_type)?;
            return match arguments.get(1) {
                None => Ok(Type::String),
                Some(Expr::Identifier(type_name)) => match type_name.as_str() {
                    "String" => Ok(Type::String),
                    "Int" => Ok(Type::Int),
                    "Float" => Ok(Type::Float),
                    "Bool" => Ok(Type::Bool),
                    _ => Err(self.error(
                        DiagnosticCode::InvalidFunctionCall,
                        format!(
                            "unsupported `Ask` type `{type_name}`; expected `Int`, `Float`, `String`, or `Bool`"
                        ),
                    )),
                },
                Some(_) => Err(self.error(
                    DiagnosticCode::InvalidFunctionCall,
                    "`Ask` type must be `Int`, `Float`, `String`, or `Bool`",
                )),
            };
        }
        let argument_types = arguments
            .iter()
            .map(|argument| self.analyze_expression(argument))
            .collect::<Result<Vec<_>, _>>()?;
        if name == "enumerate" {
            self.expect_count(name, &argument_types, 1)?;
            let element_type = match &argument_types[0] {
                Type::Array(element) | Type::List(element) | Type::Vector(element, _) => {
                    (**element).clone()
                }
                Type::Range => Type::Int,
                Type::String => Type::String,
                Type::Tuple(elements) => elements
                    .iter()
                    .cloned()
                    .reduce(|current, next| Self::merge_collection_type(&current, &next))
                    .unwrap_or(Type::Unknown),
                Type::Unknown => Type::Unknown,
                other => {
                    return Err(self.error(
                        DiagnosticCode::SemanticCollection,
                        format!(
                            "`enumerate` requires a sequence or string, found {}",
                            other.name()
                        ),
                    ));
                }
            };
            return Ok(Type::Array(Box::new(Type::Tuple(vec![
                Type::Int,
                element_type,
            ]))));
        }
        if name == "zip" {
            self.expect_count(name, &argument_types, 2)?;
            let element_type = |typ: &Type| match typ {
                Type::Array(element) | Type::List(element) | Type::Vector(element, _) => {
                    Some((**element).clone())
                }
                Type::Range => Some(Type::Int),
                Type::String => Some(Type::String),
                Type::Tuple(elements) => Some(
                    elements
                        .iter()
                        .cloned()
                        .reduce(|current, next| Self::merge_collection_type(&current, &next))
                        .unwrap_or(Type::Unknown),
                ),
                Type::Unknown => Some(Type::Unknown),
                _ => None,
            };
            let left = element_type(&argument_types[0]).ok_or_else(|| {
                self.error(
                    DiagnosticCode::SemanticCollection,
                    format!(
                        "`zip` requires sequences or strings, found {}",
                        argument_types[0].name()
                    ),
                )
            })?;
            let right = element_type(&argument_types[1]).ok_or_else(|| {
                self.error(
                    DiagnosticCode::SemanticCollection,
                    format!(
                        "`zip` requires sequences or strings, found {}",
                        argument_types[1].name()
                    ),
                )
            })?;
            return Ok(Type::Array(Box::new(Type::Tuple(vec![left, right]))));
        }
        match name {
            "assert" => {
                if !(1..=2).contains(&argument_types.len()) {
                    return Err(self.error(
                        DiagnosticCode::InvalidFunctionCall,
                        "`assert` expects a condition and an optional message",
                    ));
                }
                self.require_type(&Type::Bool, &argument_types[0])?;
                if let Some(message_type) = argument_types.get(1) {
                    self.require_type(&Type::String, message_type)?;
                }
                Ok(Type::Unit)
            }
            "type_of" => {
                self.expect_count(name, &argument_types, 1)?;
                Ok(Type::String)
            }
            "length" => {
                self.expect_count(name, &argument_types, 1)?;
                self.require_collection_or_string(&argument_types[0])?;
                Ok(Type::Int)
            }
            "contains" => {
                self.expect_count(name, &argument_types, 2)?;
                self.require_collection_or_string(&argument_types[0])?;
                if matches!(argument_types[0], Type::String) {
                    self.require_type(&Type::String, &argument_types[1])?;
                }
                Ok(Type::Bool)
            }
            "has_key" => {
                self.expect_count(name, &argument_types, 2)?;
                if !matches!(argument_types[0], Type::Hash | Type::HashValues(_)) {
                    return Err(self.error(
                        DiagnosticCode::SemanticCollection,
                        format!(
                            "`has_key` requires a Hash, found {}",
                            argument_types[0].name()
                        ),
                    ));
                }
                self.require_type(&Type::String, &argument_types[1])?;
                Ok(Type::Bool)
            }
            "get" => {
                self.expect_count(name, &argument_types, 3)?;
                let value_type = match &argument_types[0] {
                    Type::HashValues(value_type) => (**value_type).clone(),
                    Type::Hash | Type::Unknown => Type::Unknown,
                    _ => {
                        return Err(self.error(
                            DiagnosticCode::SemanticCollection,
                            format!("`get` requires a Hash, found {}", argument_types[0].name()),
                        ));
                    }
                };
                self.require_type(&Type::String, &argument_types[1])?;
                if value_type == Type::Unknown {
                    Ok(argument_types[2].clone())
                } else {
                    self.require_type(&value_type, &argument_types[2])?;
                    Ok(value_type)
                }
            }
            "without_key" => {
                self.expect_count(name, &argument_types, 2)?;
                if !matches!(argument_types[0], Type::Hash | Type::HashValues(_)) {
                    return Err(self.error(
                        DiagnosticCode::SemanticCollection,
                        format!(
                            "`without_key` requires a Hash, found {}",
                            argument_types[0].name()
                        ),
                    ));
                }
                self.require_type(&Type::String, &argument_types[1])?;
                Ok(argument_types[0].clone())
            }
            "select_keys" => {
                self.expect_count(name, &argument_types, 2)?;
                if !matches!(argument_types[0], Type::Hash | Type::HashValues(_)) {
                    return Err(self.error(
                        DiagnosticCode::SemanticCollection,
                        format!(
                            "`select_keys` requires a Hash, found {}",
                            argument_types[0].name()
                        ),
                    ));
                }
                match &argument_types[1] {
                    Type::Array(element) | Type::List(element) | Type::Vector(element, _) => {
                        self.require_type(&Type::String, element)?;
                    }
                    Type::Tuple(elements) => {
                        for element in elements {
                            self.require_type(&Type::String, element)?;
                        }
                    }
                    Type::Unknown => {}
                    other => {
                        return Err(self.error(
                            DiagnosticCode::SemanticCollection,
                            format!(
                                "`select_keys` expects an array, list, or tuple of strings, found {}",
                                other.name()
                            ),
                        ));
                    }
                }
                Ok(argument_types[0].clone())
            }
            "keys" => {
                self.expect_count(name, &argument_types, 1)?;
                if !matches!(argument_types[0], Type::Hash | Type::HashValues(_)) {
                    return Err(self.error(
                        DiagnosticCode::SemanticCollection,
                        format!("`keys` requires a Hash, found {}", argument_types[0].name()),
                    ));
                }
                Ok(Type::Array(Box::new(Type::String)))
            }
            "values" => {
                self.expect_count(name, &argument_types, 1)?;
                let value_type = match &argument_types[0] {
                    Type::HashValues(value_type) => (**value_type).clone(),
                    Type::Hash | Type::Unknown => Type::Unknown,
                    _ => {
                        return Err(self.error(
                            DiagnosticCode::SemanticCollection,
                            format!(
                                "`values` requires a Hash, found {}",
                                argument_types[0].name()
                            ),
                        ));
                    }
                };
                Ok(Type::Array(Box::new(value_type)))
            }
            "entries" => {
                self.expect_count(name, &argument_types, 1)?;
                let value_type = match &argument_types[0] {
                    Type::HashValues(value_type) => (**value_type).clone(),
                    Type::Hash | Type::Unknown => Type::Unknown,
                    _ => {
                        return Err(self.error(
                            DiagnosticCode::SemanticCollection,
                            format!(
                                "`entries` requires a Hash, found {}",
                                argument_types[0].name()
                            ),
                        ));
                    }
                };
                Ok(Type::Array(Box::new(Type::Tuple(vec![
                    Type::String,
                    value_type,
                ]))))
            }
            "any" | "all" => {
                self.expect_count(name, &argument_types, 1)?;
                self.require_boolean_iterable(&argument_types[0])?;
                Ok(Type::Bool)
            }
            "join" => {
                self.expect_count(name, &argument_types, 2)?;
                self.require_string_iterable(&argument_types[0])?;
                self.require_type(&Type::String, &argument_types[1])?;
                Ok(Type::String)
            }
            "total" => {
                self.expect_count(name, &argument_types, 1)?;
                self.require_numeric_iterable(&argument_types[0])?;
                Ok(self.numeric_sum_type(&argument_types[0]))
            }
            "trim" => {
                self.expect_count(name, &argument_types, 1)?;
                self.require_type(&Type::String, &argument_types[0])?;
                Ok(Type::String)
            }
            "read_file" => {
                self.expect_count(name, &argument_types, 1)?;
                self.require_type(&Type::String, &argument_types[0])?;
                Ok(Type::String)
            }
            "parse_json" => {
                self.expect_count(name, &argument_types, 1)?;
                self.require_type(&Type::String, &argument_types[0])?;
                Ok(Type::Unknown)
            }
            "read_json" => {
                self.expect_count(name, &argument_types, 1)?;
                self.require_type(&Type::String, &argument_types[0])?;
                Ok(Type::Unknown)
            }
            "read_json_lines" => {
                self.expect_count(name, &argument_types, 1)?;
                self.require_type(&Type::String, &argument_types[0])?;
                Ok(Type::Array(Box::new(Type::Unknown)))
            }
            "to_json" | "to_json_pretty" => {
                self.expect_count(name, &argument_types, 1)?;
                Ok(Type::String)
            }
            "write_json" | "write_json_pretty" => {
                self.expect_count(name, &argument_types, 2)?;
                self.require_type(&Type::String, &argument_types[0])?;
                Ok(Type::Unit)
            }
            "write_json_lines" => {
                self.expect_count(name, &argument_types, 2)?;
                self.require_type(&Type::String, &argument_types[0])?;
                if !matches!(
                    argument_types[1],
                    Type::Array(_)
                        | Type::List(_)
                        | Type::Tuple(_)
                        | Type::Vector(_, _)
                        | Type::Unknown
                ) {
                    return Err(self.error(
                        DiagnosticCode::SemanticCollection,
                        format!(
                            "`write_json_lines` expects an Array, List, or Tuple, found {}",
                            argument_types[1].name()
                        ),
                    ));
                }
                Ok(Type::Unit)
            }
            "append_json_line" => {
                self.expect_count(name, &argument_types, 2)?;
                self.require_type(&Type::String, &argument_types[0])?;
                Ok(Type::Unit)
            }
            "write_file" => {
                self.expect_count(name, &argument_types, 2)?;
                self.require_type(&Type::String, &argument_types[0])?;
                self.require_type(&Type::String, &argument_types[1])?;
                Ok(Type::Unit)
            }
            "substring" => {
                self.expect_count(name, &argument_types, 3)?;
                self.require_type(&Type::String, &argument_types[0])?;
                self.require_type(&Type::Int, &argument_types[1])?;
                self.require_type(&Type::Int, &argument_types[2])?;
                Ok(Type::String)
            }
            "characters" => {
                self.expect_count(name, &argument_types, 1)?;
                self.require_type(&Type::String, &argument_types[0])?;
                Ok(Type::Array(Box::new(Type::String)))
            }
            "is_ascii_alpha" | "is_ascii_digit" | "is_whitespace" => {
                self.expect_count(name, &argument_types, 1)?;
                self.require_type(&Type::String, &argument_types[0])?;
                Ok(Type::Bool)
            }
            "to_float" => {
                self.expect_count(name, &argument_types, 1)?;
                self.require_type(&Type::String, &argument_types[0])?;
                if let Expr::Literal(Literal::String(value)) = &arguments[0]
                    && !matches!(
                        value.trim().parse::<f64>(),
                        Ok(number) if number.is_finite()
                    )
                {
                    return Err(self.error(
                        DiagnosticCode::InvalidNumericLiteral,
                        format!("cannot convert `{value}` to a finite Float"),
                    ));
                }
                Ok(Type::Float)
            }
            "to_int" => {
                self.expect_count(name, &argument_types, 1)?;
                self.require_type(&Type::String, &argument_types[0])?;
                if let Expr::Literal(Literal::String(value)) = &arguments[0]
                    && value.trim().parse::<i64>().is_err()
                {
                    return Err(self.error(
                        DiagnosticCode::InvalidNumericLiteral,
                        format!("cannot convert `{value}` to an Int"),
                    ));
                }
                Ok(Type::Int)
            }
            "abs" | "round" => {
                self.expect_count(name, &argument_types, if name == "round" { 2 } else { 1 })?;
                self.require_numeric(&argument_types[0])?;
                if name == "round" {
                    self.require_type(&Type::Int, &argument_types[1])?;
                    if let Expr::Literal(Literal::Int(decimals)) = arguments[1]
                        && !(0..=15).contains(&decimals)
                    {
                        return Err(self.error(
                            DiagnosticCode::InvalidFunctionCall,
                            "`round` decimal count must be an integer from 0 to 15",
                        ));
                    }
                }
                Ok(argument_types[0].clone())
            }
            "sqrt" | "exp" | "log" | "log10" | "sin" | "cos" | "tan" | "floor" | "ceil" => {
                self.expect_count(name, &argument_types, 1)?;
                self.require_numeric(&argument_types[0])?;
                Ok(Type::Float)
            }
            "sign" => {
                self.expect_count(name, &argument_types, 1)?;
                self.require_numeric(&argument_types[0])?;
                Ok(Type::Int)
            }
            "pow" => {
                self.expect_count(name, &argument_types, 2)?;
                self.require_numeric(&argument_types[0])?;
                self.require_numeric(&argument_types[1])?;
                Ok(Type::Float)
            }
            "vector_add" | "vector_subtract" => {
                self.expect_count(name, &argument_types, 2)?;
                let left = self.numeric_sequence_element(&argument_types[0])?;
                let right = self.numeric_sequence_element(&argument_types[1])?;
                let element = self.numeric_result(&left, &right)?;
                let left_length = Self::vector_length(&argument_types[0]);
                let right_length = Self::vector_length(&argument_types[1]);
                self.require_matching_vector_lengths(name, left_length, right_length)?;
                Ok(Type::Vector(
                    Box::new(element),
                    left_length.or(right_length),
                ))
            }
            "vector_scale" => {
                self.expect_count(name, &argument_types, 2)?;
                self.require_numeric(&argument_types[1])?;
                let element = self.numeric_sequence_element(&argument_types[0])?;
                Ok(Type::Vector(
                    Box::new(self.numeric_result(&element, &argument_types[1])?),
                    Self::vector_length(&argument_types[0]),
                ))
            }
            "dot" => {
                self.expect_count(name, &argument_types, 2)?;
                let left = self.numeric_sequence_element(&argument_types[0])?;
                let right = self.numeric_sequence_element(&argument_types[1])?;
                self.require_matching_vector_lengths(
                    name,
                    Self::vector_length(&argument_types[0]),
                    Self::vector_length(&argument_types[1]),
                )?;
                self.numeric_result(&left, &right)
            }
            "cross" => {
                self.expect_count(name, &argument_types, 2)?;
                self.numeric_sequence_element(&argument_types[0])?;
                self.numeric_sequence_element(&argument_types[1])?;
                for typ in &argument_types {
                    if Self::vector_length(typ).is_some_and(|length| length != 3) {
                        return Err(self.error(
                            DiagnosticCode::SemanticCollection,
                            "`cross` requires vectors with exactly three elements",
                        ));
                    }
                }
                Ok(Type::Vector(Box::new(Type::Float), Some(3)))
            }
            "norm" => {
                self.expect_count(name, &argument_types, 1)?;
                self.numeric_sequence_element(&argument_types[0])?;
                Ok(Type::Float)
            }
            "distance" => {
                self.expect_count(name, &argument_types, 2)?;
                self.numeric_sequence_element(&argument_types[0])?;
                self.numeric_sequence_element(&argument_types[1])?;
                self.require_matching_vector_lengths(
                    name,
                    Self::vector_length(&argument_types[0]),
                    Self::vector_length(&argument_types[1]),
                )?;
                Ok(Type::Float)
            }
            "normalize" => {
                self.expect_count(name, &argument_types, 1)?;
                self.numeric_sequence_element(&argument_types[0])?;
                Ok(Type::Vector(
                    Box::new(Type::Float),
                    Self::vector_length(&argument_types[0]),
                ))
            }
            "shape" => {
                self.expect_count(name, &argument_types, 1)?;
                self.require_matrix(&argument_types[0])?;
                Ok(Type::Tuple(vec![Type::Int, Type::Int]))
            }
            "trace" => {
                self.expect_count(name, &argument_types, 1)?;
                self.require_matrix(&argument_types[0])?;
                self.require_square_dimensions(name, Self::matrix_dimensions(&argument_types[0]))?;
                Ok(Type::Float)
            }
            "rank" => {
                self.expect_count(name, &argument_types, 1)?;
                self.require_matrix(&argument_types[0])?;
                Ok(Type::Int)
            }
            "matvec" => {
                self.expect_count(name, &argument_types, 2)?;
                self.require_matrix(&argument_types[0])?;
                self.numeric_sequence_element(&argument_types[1])?;
                let shape = Self::matrix_dimensions(&argument_types[0]);
                self.require_matching_vector_lengths(
                    name,
                    shape.1,
                    Self::vector_length(&argument_types[1]),
                )?;
                Ok(Type::Vector(Box::new(Type::Float), shape.0))
            }
            "determinant" => {
                self.expect_count(name, &argument_types, 1)?;
                self.require_matrix(&argument_types[0])?;
                self.require_square_dimensions(name, Self::matrix_dimensions(&argument_types[0]))?;
                Ok(Type::Float)
            }
            "inverse" => {
                self.expect_count(name, &argument_types, 1)?;
                self.require_matrix(&argument_types[0])?;
                let shape = Self::matrix_dimensions(&argument_types[0]);
                self.require_square_dimensions(name, shape)?;
                Ok(Type::TypedMatrix(Box::new(Type::Float), shape.0, shape.1))
            }
            "lu" => {
                self.expect_count(name, &argument_types, 1)?;
                self.require_matrix(&argument_types[0])?;
                let (rows, columns) = Self::matrix_dimensions(&argument_types[0]);
                let rank_bound = rows.zip(columns).map(|(rows, columns)| rows.min(columns));
                Ok(Type::Tuple(vec![
                    Type::TypedMatrix(Box::new(Type::Float), rows, rank_bound),
                    Type::TypedMatrix(Box::new(Type::Float), rank_bound, columns),
                    Type::TypedMatrix(Box::new(Type::Float), rows, rows),
                ]))
            }
            "qr" => {
                self.expect_count(name, &argument_types, 1)?;
                self.require_matrix(&argument_types[0])?;
                let (rows, columns) = Self::matrix_dimensions(&argument_types[0]);
                Ok(Type::Tuple(vec![
                    Type::TypedMatrix(Box::new(Type::Float), rows, rows),
                    Type::TypedMatrix(Box::new(Type::Float), rows, columns),
                ]))
            }
            "cholesky" => {
                self.expect_count(name, &argument_types, 1)?;
                self.require_matrix(&argument_types[0])?;
                let shape = Self::matrix_dimensions(&argument_types[0]);
                self.require_square_dimensions(name, shape)?;
                Ok(Type::TypedMatrix(Box::new(Type::Float), shape.0, shape.1))
            }
            "solve" => {
                self.expect_count(name, &argument_types, 2)?;
                self.require_matrix(&argument_types[0])?;
                let shape = Self::matrix_dimensions(&argument_types[0]);
                self.require_square_dimensions(name, shape)?;
                if Self::is_matrix_type(&argument_types[1]) {
                    self.require_matrix(&argument_types[1])?;
                    let rhs_shape = Self::matrix_dimensions(&argument_types[1]);
                    if shape.0.is_some() && rhs_shape.0.is_some() && shape.0 != rhs_shape.0 {
                        return Err(self.error(
                            DiagnosticCode::SemanticCollection,
                            "`solve` requires the right-hand-side row count to match the matrix",
                        ));
                    }
                    Ok(Type::TypedMatrix(
                        Box::new(Type::Float),
                        shape.1,
                        rhs_shape.1,
                    ))
                } else if argument_types[1] == Type::Unknown {
                    Ok(Type::Unknown)
                } else {
                    self.numeric_sequence_element(&argument_types[1])?;
                    self.require_matching_vector_lengths(
                        name,
                        shape.0,
                        Self::vector_length(&argument_types[1]),
                    )?;
                    Ok(Type::Vector(Box::new(Type::Float), shape.1))
                }
            }
            "least_squares" => {
                self.expect_count(name, &argument_types, 2)?;
                self.require_matrix(&argument_types[0])?;
                self.numeric_sequence_element(&argument_types[1])?;
                let shape = Self::matrix_dimensions(&argument_types[0]);
                if shape
                    .0
                    .is_some_and(|rows| shape.1.is_some_and(|columns| rows < columns))
                {
                    return Err(self.error(
                        DiagnosticCode::SemanticCollection,
                        "`least_squares` requires at least as many rows as columns",
                    ));
                }
                self.require_matching_vector_lengths(
                    name,
                    shape.0,
                    Self::vector_length(&argument_types[1]),
                )?;
                Ok(Type::Vector(Box::new(Type::Float), shape.1))
            }
            "transpose" => {
                self.expect_count(name, &argument_types, 1)?;
                self.require_matrix(&argument_types[0])?;
                let (rows, columns) = Self::matrix_dimensions(&argument_types[0]);
                Ok(Type::TypedMatrix(
                    Box::new(Self::matrix_element_type(&argument_types[0])),
                    columns,
                    rows,
                ))
            }
            "matrix_add" | "matrix_subtract" | "multiply" => {
                self.expect_count(name, &argument_types, 2)?;
                self.require_matrix(&argument_types[0])?;
                self.require_matrix(&argument_types[1])?;
                let left_shape = Self::matrix_dimensions(&argument_types[0]);
                let right_shape = Self::matrix_dimensions(&argument_types[1]);
                let output_shape = if name == "multiply" {
                    if left_shape.1.is_some()
                        && right_shape.0.is_some()
                        && left_shape.1 != right_shape.0
                    {
                        return Err(self.error(
                            DiagnosticCode::SemanticCollection,
                            "matrix multiplication dimensions do not match",
                        ));
                    }
                    (left_shape.0, right_shape.1)
                } else {
                    if !Self::dimensions_match(left_shape, right_shape) {
                        return Err(self.error(
                            DiagnosticCode::SemanticCollection,
                            "matrix operation requires matching dimensions",
                        ));
                    }
                    left_shape
                };
                Ok(Type::TypedMatrix(
                    Box::new(if name == "multiply" {
                        Type::Float
                    } else {
                        Self::merge_numeric_type(
                            &Self::matrix_element_type(&argument_types[0]),
                            &Self::matrix_element_type(&argument_types[1]),
                        )
                    }),
                    output_shape.0,
                    output_shape.1,
                ))
            }
            "matrix_scale" => {
                self.expect_count(name, &argument_types, 2)?;
                self.require_matrix(&argument_types[0])?;
                self.require_numeric(&argument_types[1])?;
                let (rows, columns) = Self::matrix_dimensions(&argument_types[0]);
                Ok(Type::TypedMatrix(
                    Box::new(self.numeric_result(
                        &Self::matrix_element_type(&argument_types[0]),
                        &argument_types[1],
                    )?),
                    rows,
                    columns,
                ))
            }
            "identity" => {
                self.expect_count(name, &argument_types, 1)?;
                self.require_type(&Type::Int, &argument_types[0])?;
                let size = match &arguments[0] {
                    Expr::Literal(Literal::Int(size)) if *size > 0 => usize::try_from(*size).ok(),
                    Expr::Literal(Literal::Int(_)) => {
                        return Err(self.error(
                            DiagnosticCode::InvalidFunctionCall,
                            "`identity` size must be positive",
                        ));
                    }
                    _ => None,
                };
                Ok(Type::TypedMatrix(Box::new(Type::Int), size, size))
            }
            "mean" | "median" | "variance" | "stddev" => {
                self.expect_count(name, &argument_types, 1)?;
                self.require_numeric_iterable(&argument_types[0])?;
                Ok(Type::Float)
            }
            "percentile" => {
                self.expect_count(name, &argument_types, 2)?;
                self.require_numeric_iterable(&argument_types[0])?;
                self.require_numeric(&argument_types[1])?;
                Ok(Type::Float)
            }
            "covariance" | "correlation" => {
                self.expect_count(name, &argument_types, 2)?;
                self.require_numeric_iterable(&argument_types[0])?;
                self.require_numeric_iterable(&argument_types[1])?;
                Ok(Type::Float)
            }
            "clamp" => {
                self.expect_count(name, &argument_types, 3)?;
                for argument in &argument_types {
                    self.require_numeric(argument)?;
                }
                if argument_types.contains(&Type::Unknown) {
                    Ok(Type::Unknown)
                } else if argument_types.contains(&Type::Float) {
                    Ok(Type::Float)
                } else {
                    Ok(Type::Int)
                }
            }
            "split" => {
                self.expect_count(name, &argument_types, 2)?;
                self.require_type(&Type::String, &argument_types[0])?;
                self.require_type(&Type::String, &argument_types[1])?;
                Ok(Type::List(Box::new(Type::String)))
            }
            "csv_rows" => {
                self.expect_count(name, &argument_types, 1)?;
                self.require_type(&Type::String, &argument_types[0])?;
                Ok(Type::CsvStream)
            }
            "csv_row" => Ok(Type::List(Box::new(Type::Unknown))),
            "replace" => {
                self.expect_count(name, &argument_types, 3)?;
                for argument in &argument_types {
                    self.require_type(&Type::String, argument)?;
                }
                Ok(Type::String)
            }
            "regex_find_all" => {
                self.expect_count(name, &argument_types, 2)?;
                self.require_type(&Type::String, &argument_types[0])?;
                self.require_type(&Type::String, &argument_types[1])?;
                Ok(Type::Array(Box::new(Type::String)))
            }
            "regex_replace" => {
                self.expect_count(name, &argument_types, 3)?;
                for argument in &argument_types {
                    self.require_type(&Type::String, argument)?;
                }
                Ok(Type::String)
            }
            "score_rules" => {
                self.expect_count(name, &argument_types, 1)?;
                if !matches!(
                    argument_types[0],
                    Type::Array(_)
                        | Type::List(_)
                        | Type::Vector(_, _)
                        | Type::Tuple(_)
                        | Type::Unknown
                ) {
                    return Err(self.error(
                        DiagnosticCode::SemanticCollection,
                        format!(
                            "`score_rules` expects an Array, List, or Tuple, found {}",
                            argument_types[0].name()
                        ),
                    ));
                }
                Ok(Type::Hash)
            }
            "starts_with" | "ends_with" => {
                self.expect_count(name, &argument_types, 2)?;
                for argument in &argument_types {
                    self.require_type(&Type::String, argument)?;
                }
                Ok(Type::Bool)
            }
            "is_empty" => {
                self.expect_count(name, &argument_types, 1)?;
                self.require_collection_or_string(&argument_types[0])?;
                Ok(Type::Bool)
            }
            "reverse" => {
                self.expect_count(name, &argument_types, 1)?;
                if !matches!(
                    argument_types[0],
                    Type::Array(_) | Type::List(_) | Type::Tuple(_) | Type::Range | Type::Unknown
                ) {
                    return Err(self.error(
                        DiagnosticCode::SemanticCollection,
                        format!(
                            "`reverse` requires a sequence, found {}",
                            argument_types[0].name()
                        ),
                    ));
                }
                Ok(if argument_types[0] == Type::Range {
                    Type::Array(Box::new(Type::Int))
                } else {
                    argument_types[0].clone()
                })
            }
            "range" => {
                if !(2..=3).contains(&argument_types.len()) {
                    return Err(self.error(
                        DiagnosticCode::InvalidFunctionCall,
                        "`range` expects start, end, and an optional step",
                    ));
                }
                for argument in &argument_types {
                    self.require_type(&Type::Int, argument)?;
                }
                if matches!(arguments.get(2), Some(Expr::Literal(Literal::Int(0)))) {
                    return Err(self.error(
                        DiagnosticCode::InvalidFunctionCall,
                        "`range` step cannot be zero",
                    ));
                }
                Ok(Type::Range)
            }
            _ => {
                let Some(function) = self
                    .function_scopes
                    .iter()
                    .rev()
                    .find_map(|scope| scope.get(name))
                    .cloned()
                else {
                    if let Some(Type::Function {
                        parameters,
                        return_type,
                    }) = self.variables.get(name)
                    {
                        if parameters.len() != argument_types.len() {
                            return Err(self.error(
                                DiagnosticCode::InvalidFunctionCall,
                                format!(
                                    "function `{name}` expects {} arguments, got {}",
                                    parameters.len(),
                                    argument_types.len()
                                ),
                            ));
                        }
                        for (expected, actual) in parameters.iter().zip(&argument_types) {
                            if let Some(expected) = expected {
                                self.require_type(expected, actual)?;
                            }
                        }
                        return Ok(return_type.as_deref().cloned().unwrap_or(Type::Unknown));
                    }
                    if matches!(self.variables.get(name), Some(&Type::Unknown)) {
                        return Ok(Type::Unknown);
                    }
                    if name.chars().next().is_some_and(char::is_uppercase) {
                        return Err(self.error(
                            DiagnosticCode::UndefinedVariable,
                            format!("unknown struct type `{name}`"),
                        ));
                    }
                    return Err(self.error(
                        DiagnosticCode::InvalidFunctionCall,
                        format!("unknown function `{name}`"),
                    ));
                };
                if function.parameters.len() != argument_types.len() {
                    return Err(self.error(
                        DiagnosticCode::InvalidFunctionCall,
                        format!(
                            "function `{name}` expects {} arguments, got {}",
                            function.parameters.len(),
                            argument_types.len()
                        ),
                    ));
                }
                for (expected, actual) in function.parameters.iter().zip(argument_types.iter()) {
                    if let Some(expected) = expected {
                        self.require_type(expected, actual)?;
                    }
                }
                Ok(function.return_type.unwrap_or(Type::Unit))
            }
        }
    }

    pub(super) fn message_type(
        &mut self,
        receiver: &Expr,
        message: &str,
        arguments: &[Expr],
    ) -> Result<Type, SimplyError> {
        let receiver_type = self.analyze_expression(receiver)?;
        if matches!(receiver_type, Type::Enum(_)) {
            return Err(self.error(
                DiagnosticCode::InvalidFunctionCall,
                "enum values do not support message dispatch",
            ));
        }
        let argument_types = arguments
            .iter()
            .map(|argument| self.analyze_expression(argument))
            .collect::<Result<Vec<_>, _>>()?;
        if let Type::Struct(type_identity) = &receiver_type {
            let Some(signature) = self
                .messages
                .get(&(type_identity.clone(), message.to_owned()))
                .cloned()
            else {
                return Err(self.error(
                    DiagnosticCode::InvalidFunctionCall,
                    format!(
                        "message `{message}` is not defined for `{}`",
                        type_identity.local_name
                    ),
                ));
            };
            self.validate_ref_arguments(
                arguments,
                &argument_types,
                &signature.ref_parameters,
                &signature.mut_ref_parameters,
                message,
                true,
            )?;
            if signature.parameters.len() != argument_types.len() {
                return Err(self.error(
                    DiagnosticCode::InvalidFunctionCall,
                    format!(
                        "message `{message}` expects {} arguments, got {}",
                        signature.parameters.len(),
                        argument_types.len()
                    ),
                ));
            }

            for (expected, actual) in signature.parameters.iter().zip(&argument_types) {
                if let Some(expected) = expected {
                    self.require_type(expected, actual)?;
                }
            }
            return Ok(signature.return_type.clone().unwrap_or(Type::Unknown));
        }
        let (
            parameters,
            return_type,
            ref_parameters,
            mut_ref_parameters,
            receiver_ref,
            receiver_mut_ref,
        ) = if let Some(function) = self
            .function_scopes
            .iter()
            .rev()
            .find_map(|scope| scope.get(message))
        {
            (
                function.parameters.clone(),
                function.return_type.clone().unwrap_or(Type::Unit),
                function
                    .ref_parameters
                    .iter()
                    .skip(1)
                    .copied()
                    .collect::<Vec<_>>(),
                function
                    .mut_ref_parameters
                    .iter()
                    .skip(1)
                    .copied()
                    .collect::<Vec<_>>(),
                function.ref_parameters.first().copied().unwrap_or(false),
                function
                    .mut_ref_parameters
                    .first()
                    .copied()
                    .unwrap_or(false),
            )
        } else if let Some(Type::Function {
            parameters,
            return_type,
        }) = self.variables.get(message)
        {
            (
                parameters
                    .iter()
                    .map(|parameter| parameter.as_deref().cloned())
                    .collect(),
                return_type.as_deref().cloned().unwrap_or(Type::Unknown),
                vec![false; parameters.len().saturating_sub(1)],
                vec![false; parameters.len().saturating_sub(1)],
                false,
                false,
            )
        } else if receiver_type == Type::Unknown
            || matches!(self.variables.get(message), Some(&Type::Unknown))
        {
            return Ok(Type::Unknown);
        } else {
            return Err(self.error(
                DiagnosticCode::InvalidFunctionCall,
                format!(
                    "unknown message `{message}` for a {} receiver",
                    receiver_type.name()
                ),
            ));
        };
        let actual_types = std::iter::once(&receiver_type)
            .chain(argument_types.iter())
            .collect::<Vec<_>>();
        if Self::is_mutable_collection_type(&receiver_type) && !receiver_ref {
            return Err(self.error(
                DiagnosticCode::InvalidFunctionCall,
                "collection message receivers require a `ref` parameter",
            ));
        }
        if parameters.len() != actual_types.len() {
            return Err(self.error(
                DiagnosticCode::InvalidFunctionCall,
                format!(
                    "message `{message}` expects {} arguments, got {}",
                    parameters.len(),
                    actual_types.len()
                ),
            ));
        }
        if receiver_mut_ref {
            let Expr::Identifier(name) = receiver else {
                return Err(self.error(
                    DiagnosticCode::InvalidRefUsage,
                    format!("mutable `ref` receiver for `{message}` must be a mutable variable"),
                ));
            };
            if !self.variables.is_mutable(name) {
                return Err(self.error(
                    DiagnosticCode::InvalidRefUsage,
                    format!("mutable `ref` receiver `{name}` requires a `mut` owner"),
                ));
            }
        }
        self.validate_ref_arguments(
            arguments,
            &argument_types,
            &ref_parameters,
            &mut_ref_parameters,
            message,
            true,
        )?;
        for (expected, actual) in parameters.iter().zip(actual_types) {
            if let Some(expected) = expected {
                self.require_type(expected, actual)?;
            }
        }
        Ok(return_type)
    }

    fn validate_ref_arguments(
        &self,
        arguments: &[Expr],
        argument_types: &[Type],
        expected: &[bool],
        expected_mut: &[bool],
        callee: &str,
        require_collection_ref: bool,
    ) -> Result<(), SimplyError> {
        for (index, argument) in arguments.iter().enumerate() {
            let required = expected.get(index).copied().unwrap_or(false);
            let requires_mutable = expected_mut.get(index).copied().unwrap_or(false);
            if requires_mutable {
                let Expr::Identifier(name) = argument else {
                    return Err(self.error(
                        DiagnosticCode::InvalidRefUsage,
                        format!(
                            "mutable `ref` argument {} to `{callee}` must be a mutable variable",
                            index + 1
                        ),
                    ));
                };
                if !self.variables.is_mutable(name) {
                    return Err(self.error(
                        DiagnosticCode::InvalidRefUsage,
                        format!(
                            "mutable `ref` argument `{name}` to `{callee}` requires a `mut` owner"
                        ),
                    ));
                }
            }
            if !required
                && require_collection_ref
                && argument_types
                    .get(index)
                    .is_some_and(Self::is_mutable_collection_type)
            {
                return Err(self.error(
                    DiagnosticCode::InvalidRefUsage,
                    format!(
                        "collection argument {} to `{callee}` requires a `ref` parameter",
                        index + 1
                    ),
                ));
            }
            if !required
                && self
                    .ref_parameter_scopes
                    .iter()
                    .any(|scope| !scope.is_empty())
            {
                let actual_type = argument_types.get(index).unwrap_or(&Type::Unknown);
                if self.expression_contains_borrowed_collection(
                    argument,
                    actual_type,
                    &self.ref_parameter_scopes,
                )? {
                    return Err(self.error(
                        DiagnosticCode::InvalidRefUsage,
                        format!(
                            "a `ref` parameter cannot escape through argument {} to `{callee}`",
                            index + 1
                        ),
                    ));
                }
            }
        }
        Ok(())
    }

    pub(super) fn enum_variant_type(
        &mut self,
        enum_name: &str,
        variant_name: &str,
        arguments: &[Expr],
    ) -> Result<Type, SimplyError> {
        let Some(variants) = self.enums.get(enum_name).cloned() else {
            return Err(self.error(
                DiagnosticCode::UndefinedVariable,
                format!("unknown enum type `{enum_name}`"),
            ));
        };
        let Some(variant) = variants.iter().find(|variant| variant.name == variant_name) else {
            return Err(self.error(
                DiagnosticCode::UndefinedVariable,
                format!("unknown variant `{enum_name}::{variant_name}`"),
            ));
        };
        match (&variant.payload_type, arguments) {
            (Some(payload_type), [payload]) => {
                let actual = self.analyze_expression(payload)?;
                self.require_type(payload_type, &actual)?;
            }
            (Some(payload_type), _) => {
                return Err(self.error(
                    DiagnosticCode::InvalidFunctionCall,
                    format!(
                        "variant `{enum_name}::{variant_name}` expects a payload of type {}",
                        payload_type.name()
                    ),
                ));
            }
            (None, []) => {}
            (None, _) => {
                return Err(self.error(
                    DiagnosticCode::InvalidFunctionCall,
                    format!("unit variant `{enum_name}::{variant_name}` does not accept a payload"),
                ));
            }
        }
        Ok(Type::Enum(self.enum_identities[enum_name].clone()))
    }
}
