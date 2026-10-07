use super::*;

impl SemanticAnalyzer {
    pub(super) fn index_type(
        &self,
        target: &Type,
        index: &Type,
        index_expression: &Expr,
    ) -> Result<Type, SimplyError> {
        match target {
            Type::Array(element) | Type::List(element) | Type::Vector(element, _) => {
                self.require_type(&Type::Int, index)?;
                Ok((**element).clone())
            }
            Type::Range => {
                self.require_type(&Type::Int, index)?;
                Ok(Type::Int)
            }
            Type::Tuple(types) => {
                self.require_type(&Type::Int, index)?;
                match index_expression {
                    Expr::Literal(Literal::Int(value)) if *value >= 0 => usize::try_from(*value)
                        .ok()
                        .and_then(|index| types.get(index).cloned())
                        .ok_or_else(|| {
                            self.error(
                                DiagnosticCode::SemanticTupleIndex,
                                "tuple index out of bounds",
                            )
                        }),
                    Expr::Literal(Literal::Int(_)) => Err(self.error(
                        DiagnosticCode::SemanticTupleIndex,
                        "tuple index must be non-negative",
                    )),
                    _ => Ok(Type::Unknown),
                }
            }
            Type::Hash | Type::HashValues(_) => {
                self.require_type(&Type::String, index)?;
                Ok(match target {
                    Type::HashValues(value) => (**value).clone(),
                    _ => Type::Unknown,
                })
            }
            Type::String => {
                self.require_type(&Type::Int, index)?;
                Ok(Type::String)
            }
            Type::Matrix | Type::TypedMatrix(_, _, _) => match index {
                Type::Tuple(types)
                    if types.len() == 2
                        && types.iter().all(|value| value.compatible_with(&Type::Int)) =>
                {
                    Ok(match target {
                        Type::TypedMatrix(element, _, _) => (**element).clone(),
                        _ => Type::Unknown,
                    })
                }
                Type::Unknown => Ok(Type::Unknown),
                _ => Err(self.error(
                    DiagnosticCode::SemanticMatrixIndex,
                    "matrix index requires a tuple of two integers",
                )),
            },
            Type::Unknown => Ok(Type::Unknown),
            _ => Err(self.error(DiagnosticCode::SemanticIndex, "value is not indexable")),
        }
    }
    pub(super) fn element_type(typ: &Type) -> Option<Type> {
        match typ {
            Type::Array(element) | Type::List(element) | Type::Vector(element, _) => {
                Some((**element).clone())
            }
            Type::Range => Some(Type::Int),
            Type::Tuple(types) => Some(types.first().cloned().unwrap_or(Type::Unknown)),
            Type::Hash => Some(Type::Unknown),
            Type::HashValues(value) => Some((**value).clone()),
            Type::Unknown => Some(Type::Unknown),
            _ => None,
        }
    }

    pub(super) fn require_collection_or_string(&self, typ: &Type) -> Result<(), SimplyError> {
        if matches!(
            typ,
            Type::String
                | Type::Range
                | Type::Array(_)
                | Type::List(_)
                | Type::Vector(_, _)
                | Type::Matrix
                | Type::TypedMatrix(_, _, _)
                | Type::Tuple(_)
                | Type::Hash
                | Type::HashValues(_)
                | Type::Unknown
        ) {
            Ok(())
        } else {
            Err(self.error(
                DiagnosticCode::SemanticCollection,
                format!("expected a collection or string, found {}", typ.name()),
            ))
        }
    }

    pub(super) fn require_string_iterable(&self, typ: &Type) -> Result<(), SimplyError> {
        match typ {
            Type::Array(element) | Type::List(element) | Type::Vector(element, _) => {
                self.require_type(&Type::String, element)
            }
            Type::Hash => Ok(()),
            Type::HashValues(element) => self.require_type(&Type::String, element),
            Type::Tuple(types) => {
                for element in types {
                    self.require_type(&Type::String, element)?;
                }
                Ok(())
            }
            Type::Unknown => Ok(()),
            _ => Err(self.error(
                DiagnosticCode::SemanticCollection,
                format!("expected a string collection, found {}", typ.name()),
            )),
        }
    }

    pub(super) fn require_boolean_iterable(&self, typ: &Type) -> Result<(), SimplyError> {
        match typ {
            Type::Array(element) | Type::List(element) | Type::Vector(element, _) => {
                self.require_type(&Type::Bool, element)
            }
            Type::Tuple(types) => {
                for element in types {
                    self.require_type(&Type::Bool, element)?;
                }
                Ok(())
            }
            Type::Hash | Type::Unknown => Ok(()),
            Type::HashValues(element) => self.require_type(&Type::Bool, element),
            _ => Err(self.error(
                DiagnosticCode::SemanticCollection,
                format!("expected a boolean collection, found {}", typ.name()),
            )),
        }
    }

    pub(super) fn require_numeric_iterable(&self, typ: &Type) -> Result<(), SimplyError> {
        match typ {
            Type::Array(element) | Type::List(element) | Type::Vector(element, _) => {
                self.require_numeric(element)
            }
            Type::Range => Ok(()),
            Type::Tuple(types) => {
                for element in types {
                    self.require_numeric(element)?;
                }

                Ok(())
            }
            Type::Unknown => Ok(()),
            _ => Err(self.error(
                DiagnosticCode::SemanticCollection,
                format!("expected a numeric collection, found {}", typ.name()),
            )),
        }
    }

    pub(super) fn numeric_sequence_element(&self, typ: &Type) -> Result<Type, SimplyError> {
        let element = self.numeric_sequence_element_option(typ).ok_or_else(|| {
            self.error(
                DiagnosticCode::SemanticCollection,
                format!("expected a numeric sequence, found {}", typ.name()),
            )
        })?;
        self.require_numeric(&element)?;
        Ok(element)
    }

    pub(super) fn numeric_sequence_element_option(&self, typ: &Type) -> Option<Type> {
        match typ {
            Type::Array(element) | Type::List(element) | Type::Vector(element, _) => {
                Some((**element).clone())
            }
            Type::Tuple(elements) => {
                let first = elements.first().cloned().unwrap_or(Type::Unknown);
                if elements
                    .iter()
                    .all(|element| element.compatible_with(&first))
                {
                    Some(first)
                } else {
                    Some(Type::Unknown)
                }
            }
            Type::Unknown => Some(Type::Unknown),
            _ => None,
        }
    }

    pub(super) fn numeric_sum_type(&self, typ: &Type) -> Type {
        let elements: Vec<&Type> = match typ {
            Type::Array(element) | Type::List(element) | Type::Vector(element, _) => vec![element],
            Type::Tuple(elements) => elements.iter().collect(),
            Type::Range => return Type::Int,
            _ => return Type::Unknown,
        };
        if elements.iter().any(|element| **element == Type::Float) {
            Type::Float
        } else if elements.iter().any(|element| **element == Type::Unknown) {
            Type::Unknown
        } else {
            Type::Int
        }
    }

    pub(super) fn require_matrix(&self, typ: &Type) -> Result<(), SimplyError> {
        let row_type = match typ {
            Type::Matrix => return Ok(()),
            Type::TypedMatrix(element, _, _) => return self.require_numeric(element),
            Type::Array(row) | Type::List(row) | Type::Vector(row, _) => row,
            Type::Unknown => return Ok(()),
            _ => {
                return Err(self.error(
                    DiagnosticCode::SemanticCollection,
                    format!("expected a matrix, found {}", typ.name()),
                ));
            }
        };
        match row_type.as_ref() {
            Type::Array(element) | Type::List(element) | Type::Vector(element, _) => {
                self.require_numeric(element)
            }
            Type::HashValues(element) => self.require_numeric(element),
            Type::Unknown => Ok(()),
            _ => Err(self.error(
                DiagnosticCode::SemanticCollection,
                "matrix rows must be numeric sequences",
            )),
        }
    }

    pub(super) fn numeric_result(&self, left: &Type, right: &Type) -> Result<Type, SimplyError> {
        self.require_numeric(left)?;
        self.require_numeric(right)?;
        Ok(if left == &Type::Float || right == &Type::Float {
            Type::Float
        } else {
            Type::Int
        })
    }
    pub(super) fn require_numeric(&self, typ: &Type) -> Result<(), SimplyError> {
        if matches!(typ, Type::Int | Type::Float | Type::Unknown) {
            Ok(())
        } else {
            Err(self.error(
                DiagnosticCode::TypeMismatch,
                format!("expected a number, found {}", typ.name()),
            ))
        }
    }
    pub(super) fn require_type(&self, expected: &Type, actual: &Type) -> Result<(), SimplyError> {
        if actual == &Type::Unknown {
            return Ok(());
        }
        if actual.compatible_with(expected) {
            Ok(())
        } else {
            Err(self.type_error(expected, actual, "expression"))
        }
    }
    pub(super) fn expect_count(
        &self,
        name: &str,
        arguments: &[Type],
        expected: usize,
    ) -> Result<(), SimplyError> {
        if arguments.len() == expected {
            Ok(())
        } else {
            Err(self.error(
                DiagnosticCode::InvalidFunctionCall,
                format!(
                    "`{name}` expects {expected} arguments, got {}",
                    arguments.len()
                ),
            ))
        }
    }
    pub(super) fn type_error(&self, expected: &Type, actual: &Type, subject: &str) -> SimplyError {
        self.error(
            DiagnosticCode::TypeMismatch,
            format!(
                "type mismatch for `{subject}`: expected {}, found {}",
                expected.name(),
                actual.name()
            ),
        )
    }
    pub(super) fn error(&self, code: DiagnosticCode, message: impl Into<String>) -> SimplyError {
        SimplyError::Semantic {
            span: self.current_span.clone().unwrap_or_else(|| Span::new(0, 0)),
            code,
            message: message.into(),
        }
    }
}
