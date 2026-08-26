use crate::{
    ast::{
        BinaryOperator, CollectionOperation, Expr, Literal, PipelineStep, Program, Stmt, Type,
        UnaryOperator,
    },
    error::{SimplyError, Span},
    lexer::{Token, TokenKind},
};

pub struct Parser {
    tokens: Vec<Token>,
    current: usize,
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, current: 0 }
    }

    pub fn parse(mut self) -> Result<Program, SimplyError> {
        let mut statements = Vec::new();

        while !self.check(TokenKind::Eof) {
            if self.match_kind(TokenKind::Newline) {
                continue;
            }

            let span = self.peek().span.clone();
            statements.push(Stmt::Located {
                span,
                statement: Box::new(self.statement()?),
            });

            if self.match_kind(TokenKind::Newline) {
                while self.match_kind(TokenKind::Newline) {}
            } else if !self.check(TokenKind::Eof) {
                return Err(self.error_here("expected a new line after statement"));
            }
        }

        Ok(Program { statements })
    }

    fn statement(&mut self) -> Result<Stmt, SimplyError> {
        if self.match_kind(TokenKind::Say) {
            let expr = self.expression()?;
            return Ok(Stmt::Say(expr));
        }

        if self.match_kind(TokenKind::Open) {
            let path = match self.advance().kind.clone() {
                TokenKind::String(path) => path,
                _ => return Err(self.error_here("expected a source path after `open`")),
            };
            self.expect(TokenKind::As, "expected `as` after source path")?;
            let alias = self.expect_identifier("expected import alias")?;
            return Ok(Stmt::Import { path, alias });
        }

        if self.match_kind(TokenKind::If) {
            return self.if_statement();
        }

        if self.match_kind(TokenKind::Fn) {
            return self.function_statement();
        }

        if self.match_kind(TokenKind::Return) {
            return Ok(Stmt::Return(self.expression()?));
        }

        if self.match_kind(TokenKind::LeftParen) {
            let mut names = Vec::new();
            loop {
                names.push(self.expect_identifier("expected destructured name")?);
                if !self.match_kind(TokenKind::Comma) {
                    break;
                }
            }
            self.expect(
                TokenKind::RightParen,
                "expected `)` after destructured names",
            )?;
            self.expect(TokenKind::Is, "expected `is` after destructured names")?;
            return Ok(Stmt::Destructure {
                names,
                value: self.expression()?,
            });
        }

        if self.match_kind(TokenKind::For) {
            let name = self.expect_identifier("expected loop variable")?;
            self.expect(TokenKind::In, "expected `in` after loop variable")?;
            let iterable = self.expression()?;
            self.expect(TokenKind::Colon, "expected `:` after loop expression")?;
            self.consume_newlines();
            let body = self.block_until(TokenKind::End, TokenKind::End)?;
            self.expect(TokenKind::End, "expected `end` after loop")?;
            return Ok(Stmt::For {
                name,
                iterable,
                body,
            });
        }

        if self.match_kind(TokenKind::While) {
            let condition = self.expression()?;
            self.expect(TokenKind::Colon, "expected `:` after while condition")?;
            self.consume_newlines();
            let body = self.block_until(TokenKind::End, TokenKind::End)?;
            self.expect(TokenKind::End, "expected `end` after while")?;
            return Ok(Stmt::While { condition, body });
        }

        if self.match_kind(TokenKind::Break) {
            return Ok(Stmt::Break);
        }
        if self.match_kind(TokenKind::Continue) {
            return Ok(Stmt::Continue);
        }

        let name = match self.advance().kind.clone() {
            TokenKind::Identifier(name) => name,
            TokenKind::Count => "count".into(),
            TokenKind::Tree => "tree".into(),
            _ => return Err(self.error_here("expected a statement")),
        };
        {
            let declared_type = if self.match_kind(TokenKind::As) {
                Some(self.type_name()?)
            } else {
                None
            };
            if self.match_kind(TokenKind::Is) {
                return Ok(Stmt::Assign {
                    name,
                    declared_type,
                    value: self.assignment_value()?,
                });
            }
            if self.match_kind(TokenKind::Arrow) {
                return Ok(Stmt::Reassign {
                    name,
                    value: self.expression()?,
                });
            }
            if self.match_kind(TokenKind::LeftBracket) {
                let first = self.expression()?;
                let index = if self.match_kind(TokenKind::Comma) {
                    let second = self.expression()?;
                    Expr::Tuple(vec![first, second])
                } else {
                    first
                };
                self.expect(TokenKind::RightBracket, "expected `]`")?;
                if !self.match_kind(TokenKind::Arrow) && !self.match_kind(TokenKind::Is) {
                    return Err(self.error_here("expected `->` or `is` after index"));
                }
                return Ok(Stmt::SetIndex {
                    name,
                    index,
                    value: self.expression()?,
                });
            }
            if self.check(TokenKind::LeftParen) {
                return Ok(Stmt::Expression(self.call_expression(name)?));
            }
            if self.match_word("add") {
                return Ok(Stmt::CollectionOp {
                    name,
                    operation: CollectionOperation::Add,
                    value: self.expression()?,
                });
            }
            if self.match_word("remove") {
                return Ok(Stmt::CollectionOp {
                    name,
                    operation: CollectionOperation::Remove,
                    value: self.expression()?,
                });
            }
        }

        Err(self.error_here("expected `Say`, an assignment with `is`, or reassignment with `->`"))
    }

    fn function_statement(&mut self) -> Result<Stmt, SimplyError> {
        let name = self.expect_identifier("expected function name")?;
        self.expect(TokenKind::LeftParen, "expected `(` after function name")?;
        let mut parameters = Vec::new();
        if !self.check(TokenKind::RightParen) {
            loop {
                let parameter = self.expect_identifier("expected parameter name")?;
                let parameter_type = if self.match_kind(TokenKind::As) {
                    Some(self.type_name()?)
                } else {
                    None
                };
                parameters.push((parameter, parameter_type));
                if !self.match_kind(TokenKind::Comma) {
                    break;
                }
            }
        }
        self.expect(TokenKind::RightParen, "expected `)` after parameters")?;
        let return_type = if self.match_kind(TokenKind::Gives) {
            Some(self.type_name()?)
        } else {
            None
        };
        self.expect(TokenKind::Colon, "expected `:` after function declaration")?;
        self.consume_newlines();
        let body = self.block_until(TokenKind::End, TokenKind::End)?;
        self.expect(TokenKind::End, "expected `end` after function")?;
        Ok(Stmt::Function {
            name,
            parameters,
            return_type,
            body,
        })
    }

    fn if_statement(&mut self) -> Result<Stmt, SimplyError> {
        let condition = self.expression()?;
        let statement = self.if_body(condition)?;
        self.expect(TokenKind::End, "expected `end` after if statement")?;
        Ok(statement)
    }

    fn if_body(&mut self, condition: Expr) -> Result<Stmt, SimplyError> {
        self.expect(TokenKind::Colon, "expected `:` after if condition")?;
        self.consume_newlines();
        let then_branch = self.block_until(TokenKind::Else, TokenKind::End)?;
        let else_branch = if self.match_kind(TokenKind::Else) {
            if self.match_kind(TokenKind::If) {
                let condition = self.expression()?;
                vec![self.if_body(condition)?]
            } else {
                self.expect(TokenKind::Colon, "expected `:` after else")?;
                self.consume_newlines();
                self.block_until(TokenKind::End, TokenKind::End)?
            }
        } else {
            Vec::new()
        };
        Ok(Stmt::If {
            condition,
            then_branch,
            else_branch,
        })
    }

    fn block_until(
        &mut self,
        first_end: TokenKind,
        second_end: TokenKind,
    ) -> Result<Vec<Stmt>, SimplyError> {
        let mut statements = Vec::new();
        while !self.check(first_end.clone()) && !self.check(second_end.clone()) {
            if self.match_kind(TokenKind::Newline) {
                continue;
            }
            let span = self.peek().span.clone();
            statements.push(Stmt::Located {
                span,
                statement: Box::new(self.statement()?),
            });
            self.expect(TokenKind::Newline, "expected a new line after statement")?;
            self.consume_newlines();
        }
        Ok(statements)
    }

    fn consume_newlines(&mut self) {
        while self.match_kind(TokenKind::Newline) {}
    }

    fn expression(&mut self) -> Result<Expr, SimplyError> {
        self.parse_binary(0)
    }

    fn type_name(&mut self) -> Result<Type, SimplyError> {
        let name = match self.advance().kind.clone() {
            TokenKind::Identifier(name) => name,
            TokenKind::Array => "Array".into(),
            TokenKind::List => "List".into(),
            TokenKind::Hash => "Hash".into(),
            TokenKind::Tree => "Tree".into(),
            TokenKind::Matrix => "Matrix".into(),
            _ => return Err(self.error_here("expected a supported type")),
        };
        let base = match name.as_str() {
            "String" => Type::String,
            "Int" => Type::Int,
            "Float" => Type::Float,
            "Bool" => Type::Bool,
            "Array" => Type::Array(Box::new(Type::Int)),
            "List" => Type::List(Box::new(Type::Int)),
            "Hash" => Type::Hash,
            "Tree" => Type::Tree,
            "Matrix" => Type::Matrix,
            "Tuple" => Type::Tuple(Vec::new()),
            _ => return Err(self.error_here("expected a supported type")),
        };
        if self.match_kind(TokenKind::LeftBracket) {
            if name == "Tuple" {
                let mut types = vec![self.type_name()?];
                while self.match_kind(TokenKind::Comma) {
                    types.push(self.type_name()?);
                }
                self.expect(TokenKind::RightBracket, "expected `]` after tuple type")?;
                return Ok(Type::Tuple(types));
            }
            let element = self.type_name()?;
            self.expect(TokenKind::RightBracket, "expected `]` after type")?;
            Ok(match name.as_str() {
                "Array" => Type::Array(Box::new(element)),
                "List" => Type::List(Box::new(element)),
                _ => base,
            })
        } else {
            Ok(base)
        }
    }

    fn parse_binary(&mut self, minimum_precedence: u8) -> Result<Expr, SimplyError> {
        let mut left = self.parse_unary()?;
        while let Some((precedence, operator)) = self.binary_operator() {
            if precedence < minimum_precedence {
                break;
            }
            self.advance();
            let right = self.parse_binary(precedence + 1)?;
            left = Expr::Binary {
                left: Box::new(left),
                operator,
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_unary(&mut self) -> Result<Expr, SimplyError> {
        if self.match_kind(TokenKind::Not) {
            return Ok(Expr::Unary {
                operator: UnaryOperator::Not,
                operand: Box::new(self.parse_unary()?),
            });
        }
        if self.match_kind(TokenKind::Minus) {
            return Ok(Expr::Unary {
                operator: UnaryOperator::Negate,
                operand: Box::new(self.parse_unary()?),
            });
        }

        let token = self.advance().clone();
        let is_list = token.kind == TokenKind::List;
        let mut expression = match token.kind {
            TokenKind::String(value) => Ok(Expr::Literal(Literal::String(value))),
            TokenKind::Int(value) => Ok(Expr::Literal(Literal::Int(value))),
            TokenKind::Float(value) => Ok(Expr::Literal(Literal::Float(value))),
            TokenKind::True => Ok(Expr::Literal(Literal::Bool(true))),
            TokenKind::False => Ok(Expr::Literal(Literal::Bool(false))),
            TokenKind::Identifier(name) => Ok(Expr::Identifier(name)),
            TokenKind::Array | TokenKind::List => {
                self.expect(TokenKind::LeftBracket, "expected `[` after collection type")?;
                let values = self.expression_list(TokenKind::RightBracket)?;
                Ok(if is_list {
                    Expr::List(values)
                } else {
                    Expr::Array(values)
                })
            }
            TokenKind::LeftBracket => {
                Ok(Expr::Array(self.expression_list(TokenKind::RightBracket)?))
            }
            TokenKind::Matrix => {
                self.expect(TokenKind::LeftBracket, "expected `[` after matrix")?;
                Ok(Expr::Matrix(self.expression_list(TokenKind::RightBracket)?))
            }
            TokenKind::Hash => Ok(Expr::Hash(self.named_block(TokenKind::Hash)?)),
            TokenKind::Pipeline => self.pipeline_expression(),
            TokenKind::Tree => Ok(Expr::Identifier("tree".into())),
            TokenKind::Count => Ok(Expr::Identifier("count".into())),
            TokenKind::LeftParen => {
                let values = self.expression_list(TokenKind::RightParen)?;
                Ok(Expr::Tuple(values))
            }
            _ => Err(SimplyError::Parse {
                span: token.span,
                message: "expected a value after the statement".into(),
            }),
        }?;

        if let Expr::Identifier(name) = &expression {
            if self.check(TokenKind::LeftParen) {
                expression = self.call_expression(name.clone())?;
            }
        }
        loop {
            if self.match_kind(TokenKind::Transpose) {
                expression = Expr::Unary {
                    operator: UnaryOperator::Transpose,
                    operand: Box::new(expression),
                };
            } else if self.match_kind(TokenKind::LeftBracket) {
                let first = self.expression()?;
                let index = if self.match_kind(TokenKind::Comma) {
                    let second = self.expression()?;
                    Expr::Tuple(vec![first, second])
                } else {
                    first
                };
                self.expect(TokenKind::RightBracket, "expected `]`")?;
                expression = Expr::Index {
                    target: Box::new(expression),
                    index: Box::new(index),
                };
            } else if self.match_kind(TokenKind::Dot) {
                let name = self.expect_identifier("expected field name")?;
                expression = Expr::Field {
                    target: Box::new(expression),
                    name,
                };
            } else {
                break;
            }
        }
        Ok(expression)
    }

    fn expression_list(&mut self, closing: TokenKind) -> Result<Vec<Expr>, SimplyError> {
        let mut values = Vec::new();
        while !self.check(closing.clone()) {
            if self.match_kind(TokenKind::Newline) {
                continue;
            }
            values.push(self.expression()?);
            if !self.match_kind(TokenKind::Comma) {
                break;
            }
        }
        self.consume_newlines();
        self.expect(closing, "expected collection closing delimiter")?;
        Ok(values)
    }

    fn assignment_value(&mut self) -> Result<Expr, SimplyError> {
        if self.match_kind(TokenKind::Tree) {
            return Ok(Expr::Tree(self.named_block(TokenKind::Tree)?));
        }
        self.expression()
    }

    fn named_block(&mut self, _kind: TokenKind) -> Result<Vec<(String, Expr)>, SimplyError> {
        self.expect(TokenKind::Colon, "expected `:` after collection type")?;
        self.consume_newlines();
        let mut entries = Vec::new();
        while !self.check(TokenKind::End) {
            let name = self.expect_identifier("expected field name")?;
            if entries.iter().any(|(field, _)| field == &name) {
                return Err(self.error_here("duplicate field in named collection"));
            }
            self.expect(TokenKind::Is, "expected `is` after field name")?;
            entries.push((name, self.assignment_value()?));
            self.expect(TokenKind::Newline, "expected a new line after field")?;
            self.consume_newlines();
        }
        self.expect(TokenKind::End, "expected `end` after collection")?;
        Ok(entries)
    }

    fn pipeline_expression(&mut self) -> Result<Expr, SimplyError> {
        self.expect(TokenKind::Colon, "expected `:` after pipeline")?;
        self.consume_newlines();
        let source = self.expression()?;
        self.expect(
            TokenKind::Newline,
            "expected a new line after pipeline source",
        )?;
        let mut steps = Vec::new();
        let mut terminal = false;
        while !self.check(TokenKind::End) {
            if self.match_kind(TokenKind::Newline) {
                continue;
            }
            if terminal {
                return Err(self.error_here("pipeline cannot continue after `sum` or `count`"));
            }
            if self.match_kind(TokenKind::Filter) {
                steps.push(PipelineStep::Filter(self.expression()?));
            } else if self.match_kind(TokenKind::Map) {
                steps.push(PipelineStep::Map(self.expression()?));
            } else if self.match_kind(TokenKind::Sum) {
                steps.push(PipelineStep::Sum);
                terminal = true;
            } else if self.match_kind(TokenKind::Count) {
                steps.push(PipelineStep::Count);
                terminal = true;
            } else {
                return Err(self.error_here("expected pipeline step"));
            }
            self.expect(
                TokenKind::Newline,
                "expected a new line after pipeline step",
            )?;
        }
        self.expect(TokenKind::End, "expected `end` after pipeline")?;
        Ok(Expr::Pipeline {
            source: Box::new(source),
            steps,
        })
    }

    fn call_expression(&mut self, name: String) -> Result<Expr, SimplyError> {
        self.expect(TokenKind::LeftParen, "expected `(` after function name")?;
        let mut arguments = Vec::new();
        if !self.check(TokenKind::RightParen) {
            loop {
                arguments.push(self.expression()?);
                if !self.match_kind(TokenKind::Comma) {
                    break;
                }
            }
        }
        self.expect(TokenKind::RightParen, "expected `)` after arguments")?;
        Ok(Expr::Call { name, arguments })
    }

    fn binary_operator(&self) -> Option<(u8, BinaryOperator)> {
        match self.peek().kind {
            TokenKind::Plus => Some((5, BinaryOperator::Add)),
            TokenKind::Minus => Some((5, BinaryOperator::Subtract)),
            TokenKind::Star => Some((6, BinaryOperator::Multiply)),
            TokenKind::Slash => Some((6, BinaryOperator::Divide)),
            TokenKind::Percent => Some((6, BinaryOperator::Remainder)),
            TokenKind::MultiplyWord => Some((5, BinaryOperator::MatrixMultiply)),
            TokenKind::Greater => Some((4, BinaryOperator::Greater)),
            TokenKind::GreaterEqual => Some((4, BinaryOperator::GreaterEqual)),
            TokenKind::Less => Some((4, BinaryOperator::Less)),
            TokenKind::LessEqual => Some((4, BinaryOperator::LessEqual)),
            TokenKind::EqualEqual => Some((3, BinaryOperator::Equal)),
            TokenKind::NotEqual => Some((3, BinaryOperator::NotEqual)),
            TokenKind::And => Some((2, BinaryOperator::And)),
            TokenKind::Or => Some((1, BinaryOperator::Or)),
            _ => None,
        }
    }

    fn advance(&mut self) -> &Token {
        if !self.check(TokenKind::Eof) {
            self.current += 1;
            &self.tokens[self.current - 1]
        } else {
            &self.tokens[self.current]
        }
    }

    fn peek(&self) -> &Token {
        &self.tokens[self.current]
    }

    fn check(&self, kind: TokenKind) -> bool {
        self.peek().kind == kind
    }

    fn match_kind(&mut self, kind: TokenKind) -> bool {
        if self.check(kind) {
            self.advance();
            true
        } else {
            false
        }
    }

    fn match_word(&mut self, word: &str) -> bool {
        match &self.peek().kind {
            TokenKind::Identifier(value) if value == word => {
                self.advance();
                true
            }
            _ => false,
        }
    }

    fn expect(&mut self, kind: TokenKind, message: &str) -> Result<(), SimplyError> {
        if self.match_kind(kind) {
            Ok(())
        } else {
            Err(self.error_here(message))
        }
    }

    fn expect_identifier(&mut self, message: &str) -> Result<String, SimplyError> {
        match self.advance().kind.clone() {
            TokenKind::Identifier(name) => Ok(name),
            _ => Err(self.error_here(message)),
        }
    }

    fn error_here(&self, message: &str) -> SimplyError {
        SimplyError::Parse {
            span: self.peek().span.clone(),
            message: message.into(),
        }
    }

    #[allow(dead_code)]
    fn span_here(&self) -> Span {
        self.peek().span.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::Lexer;

    fn inner(statement: &Stmt) -> &Stmt {
        match statement {
            Stmt::Located { statement, .. } => statement,
            statement => statement,
        }
    }

    #[test]
    fn parses_say_statements() {
        let tokens = Lexer::new("Say \"Hello\"\nSay 42\nSay 3.5\nSay true\n")
            .tokenize()
            .unwrap();
        let program = Parser::new(tokens).parse().unwrap();
        assert_eq!(program.statements.len(), 4);
        assert_eq!(
            inner(&program.statements[1]),
            &Stmt::Say(Expr::Literal(Literal::Int(42)))
        );
    }

    #[test]
    fn parses_assignments_and_reassignments() {
        let tokens = Lexer::new("name is \"Simply\"\nage -> \"Rust\"\nSay name\n")
            .tokenize()
            .unwrap();
        let program = Parser::new(tokens).parse().unwrap();
        assert_eq!(
            inner(&program.statements[0]),
            &Stmt::Assign {
                name: "name".into(),
                declared_type: None,
                value: Expr::Literal(Literal::String("Simply".into())),
            }
        );
        assert_eq!(
            inner(&program.statements[1]),
            &Stmt::Reassign {
                name: "age".into(),
                value: Expr::Literal(Literal::String("Rust".into())),
            }
        );
    }

    #[test]
    fn parses_if_else_blocks() {
        let tokens = Lexer::new("if true:\n    Say \"yes\"\nelse:\n    Say \"no\"\nend\n")
            .tokenize()
            .unwrap();
        let program = Parser::new(tokens).parse().unwrap();
        assert_eq!(program.statements.len(), 1);
        assert!(matches!(inner(&program.statements[0]), Stmt::If { .. }));
    }

    #[test]
    fn parses_functions_and_calls() {
        let tokens = Lexer::new("fn add(a, b):\n    return a + b\nend\nSay add(2, 3)\n")
            .tokenize()
            .unwrap();
        let program = Parser::new(tokens).parse().unwrap();
        assert!(matches!(
            inner(&program.statements[0]),
            Stmt::Function { .. }
        ));
        assert!(matches!(
            inner(&program.statements[1]),
            Stmt::Say(Expr::Call { .. })
        ));
    }
}
