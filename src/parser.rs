//! parser.rs — syntax parsing
//! Builds the abstract syntax tree from lexer tokens and reports parse diagnostics.
//! Key component: Parser handles declarations, expressions, blocks, and pipeline syntax.
use crate::{
    ast::{
        BinaryOperator, CatchClause, CollectionOperation, EnumVariant, Expr, Literal, MatchArm,
        MatchPattern, PartitionRule, PipelineStep, Program, Stmt, StructField, UnaryOperator,
    },
    error::{DiagnosticCode, SimplyError, Span},
    lexer::{Token, TokenKind},
    types::{DeclarationIdentity, DeclarationKind, Type},
};

const MAX_PARSER_NESTING: usize = 64;

pub struct Parser {
    tokens: Vec<Token>,
    current: usize,
    enum_names: std::collections::HashSet<String>,
    expression_depth: usize,
    statement_depth: usize,
    type_depth: usize,
    pattern_depth: usize,
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Self::new_with_enum_names(tokens, std::iter::empty())
    }

    pub fn new_with_enum_names(
        mut tokens: Vec<Token>,
        known_enum_names: impl IntoIterator<Item = String>,
    ) -> Self {
        let eof_span = tokens
            .last()
            .map(|token| token.span.clone())
            .unwrap_or_else(|| Span::new(0, 0));
        if !matches!(tokens.last().map(|token| &token.kind), Some(TokenKind::Eof)) {
            tokens.push(Token {
                kind: TokenKind::Eof,
                span: eof_span,
            });
        }
        let mut enum_names = tokens
            .windows(2)
            .filter_map(|pair| match (&pair[0].kind, &pair[1].kind) {
                (TokenKind::Enum, TokenKind::Identifier(name)) => Some(name.clone()),
                _ => None,
            })
            .collect::<std::collections::HashSet<_>>();
        enum_names.extend(known_enum_names);
        Self {
            tokens,
            current: 0,
            enum_names,
            expression_depth: 0,
            statement_depth: 0,
            type_depth: 0,
            pattern_depth: 0,
        }
    }

    pub fn parse(mut self) -> Result<Program, SimplyError> {
        let mut statements = Vec::with_capacity(self.tokens.len().saturating_div(2));

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
        if self.statement_depth >= MAX_PARSER_NESTING {
            return Err(self.error_here("maximum parser nesting depth exceeded"));
        }
        self.statement_depth += 1;
        let result = self.statement_inner();
        self.statement_depth -= 1;
        result
    }

    fn statement_inner(&mut self) -> Result<Stmt, SimplyError> {
        if self.match_kind(TokenKind::Say) {
            let expr = self.expression()?;
            return Ok(Stmt::Say(expr));
        }
        if self.match_kind(TokenKind::Sayln) {
            let expr = self.expression()?;
            return Ok(Stmt::Sayln(expr));
        }

        if self.match_kind(TokenKind::Open) {
            let token = self.advance().clone();
            let path = match token.kind {
                TokenKind::String(path) => path,
                _ => {
                    return Err(self.error_at(token.span, "expected a source path after `open`"));
                }
            };
            if self.match_kind(TokenKind::As) {
                let alias = self.expect_identifier("expected import alias")?;
                return Ok(Stmt::Import {
                    path,
                    alias: Some(alias),
                    exposing: Vec::new(),
                });
            }
            if self.match_kind(TokenKind::Exposing) {
                let mut exposing = Vec::new();
                loop {
                    let exported = self.expect_identifier("expected exported name")?;
                    let local = if self.match_kind(TokenKind::As) {
                        self.expect_identifier("expected local name after `as`")?
                    } else {
                        exported.clone()
                    };
                    exposing.push((exported, local));
                    if !self.match_kind(TokenKind::Comma) {
                        break;
                    }
                }
                return Ok(Stmt::Import {
                    path,
                    alias: None,
                    exposing,
                });
            }
            return Err(self.error_here("expected `as` or `exposing` after source path"));
        }

        if self.match_kind(TokenKind::Export) {
            let mut names = vec![self.expect_identifier("expected exported name")?];
            while self.match_kind(TokenKind::Comma) {
                names.push(self.expect_identifier("expected exported name after `,`")?);
            }
            return Ok(Stmt::Export { names });
        }

        if self.match_kind(TokenKind::Flow) {
            return self.flow_statement();
        }

        if self.match_kind(TokenKind::If) {
            return self.if_statement();
        }

        if self.match_kind(TokenKind::Try) {
            return self.try_statement();
        }

        if self.match_kind(TokenKind::Fn) {
            return self.function_statement();
        }

        if self.match_kind(TokenKind::Type) {
            return self.struct_statement();
        }

        if self.match_kind(TokenKind::Enum) {
            return self.enum_statement();
        }

        if self.match_kind(TokenKind::On) {
            return self.message_statement();
        }

        if self.match_kind(TokenKind::Match) {
            return Ok(Stmt::Expression(self.match_expression()?));
        }

        if self.match_kind(TokenKind::Return) {
            return Ok(Stmt::Return(self.expression()?));
        }

        if self.match_kind(TokenKind::Throw) {
            return Ok(Stmt::Throw(self.expression()?));
        }

        let mutable = self.match_kind(TokenKind::Mut);
        if self.is_destructure_assignment_statement() {
            let parenthesized = self.check(TokenKind::LeftParen);
            let mut pattern = self.match_pattern()?;
            if parenthesized && !matches!(&pattern, MatchPattern::Tuple(_)) {
                pattern = MatchPattern::Tuple(vec![pattern]);
            }
            self.expect(TokenKind::Arrow, "expected `->` after destructuring target")?;
            self.consume_newlines();
            if self.check(TokenKind::Eof) {
                return Err(self.error_here("expected a value after `->`"));
            }
            return Ok(Stmt::DestructureReassign {
                pattern,
                value: self.expression()?,
            });
        }
        if self.is_destructure_statement() {
            let parenthesized = self.check(TokenKind::LeftParen);
            let mut pattern = self.match_pattern()?;
            if parenthesized && !matches!(&pattern, MatchPattern::Tuple(_)) {
                pattern = MatchPattern::Tuple(vec![pattern]);
            }
            self.expect(TokenKind::Is, "expected `is` after destructured names")?;
            return Ok(Stmt::Destructure {
                pattern,
                mutable,
                value: self.expression()?,
            });
        }

        if self.match_kind(TokenKind::For) {
            let mutable = self.match_kind(TokenKind::Mut);
            let name = self.expect_identifier("expected loop variable")?;
            self.expect(TokenKind::In, "expected `in` after loop variable")?;
            let iterable = self.expression()?;
            self.expect(TokenKind::Colon, "expected `:` after loop expression")?;
            self.consume_newlines();
            let body = self.block_until(TokenKind::End, TokenKind::End)?;
            self.expect(TokenKind::End, "expected `end` after loop")?;
            return Ok(Stmt::For {
                name,
                mutable,
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

        if !mutable
            && matches!(
                self.peek().kind,
                TokenKind::String(_)
                    | TokenKind::Int(_)
                    | TokenKind::Float(_)
                    | TokenKind::True
                    | TokenKind::False
                    | TokenKind::LeftParen
                    | TokenKind::Minus
                    | TokenKind::Not
                    | TokenKind::Array
                    | TokenKind::List
                    | TokenKind::Hash
                    | TokenKind::Matrix
                    | TokenKind::Pipeline
            )
        {
            return Ok(Stmt::Expression(self.expression()?));
        }

        let next_kind = self.tokens.get(self.current + 1).map(|token| &token.kind);
        let assignment_like = matches!(
            next_kind,
            Some(TokenKind::As)
                | Some(TokenKind::Is)
                | Some(TokenKind::Arrow)
                | Some(TokenKind::LeftBracket)
        );
        let collection_operation = matches!(
            next_kind,
            Some(TokenKind::Identifier(value)) if value == "add" || value == "remove"
        );
        if !mutable
            && matches!(self.peek().kind, TokenKind::Identifier(_))
            && !assignment_like
            && !collection_operation
        {
            return Ok(Stmt::Expression(self.expression()?));
        }

        let token = self.advance().clone();
        let name = match token.kind {
            TokenKind::Identifier(name) => name,
            TokenKind::Count => "count".into(),
            TokenKind::Tree => "tree".into(),
            _ => return Err(self.error_at(token.span, "expected a statement")),
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
                    mutable,
                    declared_type,
                    value: self.assignment_value()?,
                });
            }
            if self.match_kind(TokenKind::Arrow) {
                let arrow_span = self.tokens[self.current - 1].span.clone();
                self.consume_newlines();
                if self.check(TokenKind::Eof) {
                    return Err(SimplyError::Parse {
                        span: arrow_span,
                        code: DiagnosticCode::ExpectedExpression,
                        message: format!(
                            "expected a value after `->`; put the value or calculation on this line or the next line, for example `{name} ->\\n    {name} + 10`"
                        ),
                    });
                }
                return Ok(Stmt::Reassign {
                    name,
                    value: self.expression()?,
                });
            }
            if let Some(declared_type) = declared_type {
                return Err(self.error_here(&format!(
                    "a typed declaration needs `is` before its initial value; write `{} as {} is <value>`",
                    name,
                    declared_type.name()
                )));
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
        let parameters = self.parameter_list()?;
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
            body: body.into(),
        })
    }

    fn parameter_list(&mut self) -> Result<Vec<(String, Option<Type>, bool)>, SimplyError> {
        let mut parameters = Vec::new();
        let mut names = std::collections::HashSet::new();
        if !self.check(TokenKind::RightParen) {
            loop {
                let mutable = self.match_kind(TokenKind::Mut);
                let parameter = self.expect_identifier("expected parameter name")?;
                if !names.insert(parameter.clone()) {
                    return Err(SimplyError::Parse {
                        span: self.tokens[self.current - 1].span.clone(),
                        code: DiagnosticCode::UnexpectedToken,
                        message: format!("duplicate parameter `{parameter}`"),
                    });
                }
                let parameter_type = if self.match_kind(TokenKind::As) {
                    Some(self.type_name()?)
                } else {
                    None
                };
                parameters.push((parameter, parameter_type, mutable));
                if !self.match_kind(TokenKind::Comma) {
                    break;
                }
                if self.check(TokenKind::RightParen) {
                    return Err(self.error_here("expected a parameter after `,`"));
                }
            }
        }
        self.expect(TokenKind::RightParen, "expected `)` after parameters")?;
        Ok(parameters)
    }

    fn struct_statement(&mut self) -> Result<Stmt, SimplyError> {
        let name = self.expect_identifier("expected a struct name after `type`")?;
        self.expect(TokenKind::Colon, "expected `:` after struct name")?;
        self.consume_newlines();
        let mut fields = Vec::new();
        let mut field_names = std::collections::HashSet::new();
        while !self.check(TokenKind::End) && !self.check(TokenKind::Eof) {
            if self.match_kind(TokenKind::Newline) {
                continue;
            }
            let field_name = self.expect_identifier("expected a field name")?;
            let field_span = self.tokens[self.current - 1].span.clone();
            if !field_names.insert(field_name.clone()) {
                return Err(SimplyError::Parse {
                    span: field_span,
                    code: DiagnosticCode::UnexpectedToken,
                    message: format!("duplicate field `{field_name}` in struct `{name}`"),
                });
            }
            self.expect(TokenKind::As, "expected `as` after struct field name")?;
            let field_type = self.type_name()?;
            fields.push(StructField {
                name: field_name,
                field_type,
            });
            self.expect(TokenKind::Newline, "expected a new line after struct field")?;
        }
        self.expect(TokenKind::End, "expected `end` after struct declaration")?;
        Ok(Stmt::Struct { name, fields })
    }

    fn enum_statement(&mut self) -> Result<Stmt, SimplyError> {
        let name = self.expect_identifier("expected an enum name after `enum`")?;
        self.expect(TokenKind::Colon, "expected `:` after enum name")?;
        self.consume_newlines();
        let mut variants = Vec::new();
        let mut variant_names = std::collections::HashSet::new();
        while !self.check(TokenKind::End) && !self.check(TokenKind::Eof) {
            if self.match_kind(TokenKind::Newline) {
                continue;
            }
            let variant = self.expect_identifier("expected an enum variant name")?;
            if !variant_names.insert(variant.clone()) {
                return Err(SimplyError::Parse {
                    span: self.tokens[self.current - 1].span.clone(),
                    code: DiagnosticCode::UnexpectedToken,
                    message: format!("duplicate variant `{name}::{variant}`"),
                });
            }
            let payload_type = if self.match_kind(TokenKind::As) {
                Some(self.type_name()?)
            } else {
                None
            };
            variants.push(EnumVariant {
                name: variant,
                payload_type,
            });
            self.expect(TokenKind::Newline, "expected a new line after enum variant")?;
        }
        self.expect(TokenKind::End, "expected `end` after enum declaration")?;
        Ok(Stmt::Enum { name, variants })
    }

    fn match_expression(&mut self) -> Result<Expr, SimplyError> {
        let value = self.expression()?;
        self.expect(TokenKind::Colon, "expected `:` after match value")?;
        self.consume_newlines();
        let mut arms = Vec::new();
        while !self.check(TokenKind::End) && !self.check(TokenKind::Eof) {
            if self.match_kind(TokenKind::Newline) {
                continue;
            }
            let pattern = self.match_pattern()?;
            let guard = if self.match_kind(TokenKind::If) {
                Some(self.expression()?)
            } else {
                None
            };
            self.expect(
                TokenKind::Colon,
                "expected `:` after match pattern or guard",
            )?;
            self.consume_newlines();
            let mut body = Vec::new();
            while !self.check(TokenKind::End) && !self.is_match_pattern_start() {
                if self.match_kind(TokenKind::Newline) {
                    continue;
                }
                let span = self.peek().span.clone();
                body.push(Stmt::Located {
                    span,
                    statement: Box::new(self.statement()?),
                });
                self.expect(
                    TokenKind::Newline,
                    "expected a new line after match arm statement",
                )?;
                self.consume_newlines();
            }
            let result = match body.last() {
                Some(Stmt::Located { statement, .. }) => match statement.as_ref() {
                    Stmt::Expression(expression) => Some(expression.clone()),
                    _ => None,
                },
                Some(Stmt::Expression(expression)) => Some(expression.clone()),
                _ => None,
            };
            if result.is_some() {
                body.pop();
            }
            arms.push(MatchArm {
                pattern,
                guard,
                body,
                result,
            });
        }
        self.expect(TokenKind::End, "expected `end` after match expression")?;
        Ok(Expr::Match {
            value: Box::new(value),
            arms,
        })
    }

    fn match_pattern(&mut self) -> Result<MatchPattern, SimplyError> {
        let first = self.match_pattern_atom()?;
        self.consume_newlines_before_pipe();
        if !self.match_kind(TokenKind::Pipe) {
            self.consume_newlines_before_match_header();
            return Ok(first);
        }

        let mut alternatives = vec![first];
        loop {
            self.consume_newlines();
            if self.check(TokenKind::Colon)
                || self.check(TokenKind::If)
                || self.check(TokenKind::RightParen)
                || self.check(TokenKind::Comma)
                || self.check(TokenKind::Eof)
                || !self.is_match_pattern_atom_start()
            {
                return Err(self.error_here("expected a pattern after `|`"));
            }
            alternatives.push(self.match_pattern_atom()?);
            self.consume_newlines_before_pipe();
            if !self.match_kind(TokenKind::Pipe) {
                break;
            }
        }
        self.consume_newlines_before_match_header();
        Ok(MatchPattern::Or(alternatives))
    }

    fn consume_newlines_before_pipe(&mut self) {
        let mut offset = 0usize;
        while matches!(
            self.tokens
                .get(self.current + offset)
                .map(|token| &token.kind),
            Some(TokenKind::Newline)
        ) {
            offset += 1;
        }
        if matches!(
            self.tokens
                .get(self.current + offset)
                .map(|token| &token.kind),
            Some(TokenKind::Pipe)
        ) {
            self.current += offset;
        }
    }

    fn consume_newlines_before_match_header(&mut self) {
        let mut offset = 0usize;
        while matches!(
            self.tokens
                .get(self.current + offset)
                .map(|token| &token.kind),
            Some(TokenKind::Newline)
        ) {
            offset += 1;
        }
        if matches!(
            self.tokens
                .get(self.current + offset)
                .map(|token| &token.kind),
            Some(TokenKind::If | TokenKind::Colon)
        ) {
            self.current += offset;
        }
    }

    fn is_match_pattern_atom_start(&self) -> bool {
        matches!(
            self.peek().kind,
            TokenKind::Identifier(_)
                | TokenKind::LeftParen
                | TokenKind::LeftBracket
                | TokenKind::LeftBrace
                | TokenKind::DotDot
                | TokenKind::String(_)
                | TokenKind::Int(_)
                | TokenKind::Float(_)
                | TokenKind::True
                | TokenKind::False
                | TokenKind::Minus
        )
    }

    fn match_pattern_atom(&mut self) -> Result<MatchPattern, SimplyError> {
        if self.pattern_depth >= MAX_PARSER_NESTING {
            return Err(self.error_here("maximum pattern nesting depth exceeded"));
        }
        self.pattern_depth += 1;
        let result = self.match_pattern_atom_inner();
        self.pattern_depth -= 1;
        result
    }

    fn match_pattern_atom_inner(&mut self) -> Result<MatchPattern, SimplyError> {
        if self.match_kind(TokenKind::DotDot) {
            let end = self.range_pattern_endpoint()?;
            if end.is_none() {
                return Err(self.error_here("a range pattern must have at least one bound"));
            }
            return Ok(MatchPattern::Range { start: None, end });
        }

        let literal = match self.peek().kind.clone() {
            TokenKind::String(value) => Some(Literal::String(value)),
            TokenKind::Int(value) => Some(Literal::Int(value)),
            TokenKind::Float(value) => Some(Literal::Float(value)),
            TokenKind::True => Some(Literal::Bool(true)),
            TokenKind::False => Some(Literal::Bool(false)),
            _ => None,
        };
        if let Some(literal) = literal {
            self.advance();
            return self.finish_literal_pattern(literal);
        }
        if self.match_kind(TokenKind::Minus) {
            let token = self.advance().clone();
            let literal = match token.kind {
                TokenKind::Int(value) => {
                    Literal::Int(value.checked_neg().ok_or_else(|| SimplyError::Parse {
                        span: token.span.clone(),
                        code: DiagnosticCode::InvalidNumber,
                        message: "integer pattern bound is out of range".into(),
                    })?)
                }
                TokenKind::Float(value) => Literal::Float(-value),
                _ => Err(SimplyError::Parse {
                    span: token.span,
                    code: DiagnosticCode::UnexpectedToken,
                    message: "expected a numeric literal after `-` in pattern".into(),
                })?,
            };
            return self.finish_literal_pattern(literal);
        }
        if self.match_kind(TokenKind::LeftParen) {
            let mut patterns = Vec::new();
            while !self.check(TokenKind::RightParen) {
                patterns.push(self.match_pattern()?);
                if !self.match_kind(TokenKind::Comma) {
                    break;
                }
            }
            self.expect(TokenKind::RightParen, "expected `)` after tuple pattern")?;
            return Ok(match patterns.len() {
                0 => MatchPattern::Tuple(patterns),
                1 => patterns.pop().expect("single pattern was checked"),
                _ => MatchPattern::Tuple(patterns),
            });
        }
        if self.match_kind(TokenKind::LeftBracket) {
            let mut patterns = Vec::new();
            let mut rest = None;
            while !self.check(TokenKind::RightBracket) {
                if self.match_kind(TokenKind::DotDotDot) {
                    let name = self.expect_identifier("expected a binding name after `...`")?;
                    if name == "_" {
                        return Err(self.error_here("rest pattern must bind an identifier"));
                    }
                    rest = Some(name);
                    if self.match_kind(TokenKind::Comma) && !self.check(TokenKind::RightBracket) {
                        return Err(
                            self.error_here("rest pattern must be the final sequence element")
                        );
                    }
                    if !self.check(TokenKind::RightBracket) {
                        return Err(self.error_here("expected `]` after rest pattern"));
                    }
                    break;
                }
                patterns.push(self.match_pattern()?);
                if !self.match_kind(TokenKind::Comma) {
                    break;
                }
            }
            self.expect(
                TokenKind::RightBracket,
                "expected `]` after sequence pattern",
            )?;
            return Ok(MatchPattern::Sequence { patterns, rest });
        }
        if self.match_kind(TokenKind::LeftBrace) {
            let mut entries = Vec::new();
            self.consume_newlines();
            while !self.check(TokenKind::RightBrace) {
                let key = match self.advance().clone() {
                    Token {
                        kind: TokenKind::String(key),
                        ..
                    } => key,
                    Token {
                        kind: TokenKind::Identifier(key),
                        ..
                    } => key,
                    token => {
                        return Err(SimplyError::Parse {
                            span: token.span,
                            code: DiagnosticCode::UnexpectedToken,
                            message: "hash pattern keys must be string literals or field names"
                                .into(),
                        });
                    }
                };
                if entries.iter().any(|(existing, _)| existing == &key) {
                    return Err(self.error_here("duplicate key in hash pattern"));
                }
                self.expect(TokenKind::Colon, "expected `:` after hash pattern key")?;
                entries.push((key, self.match_pattern()?));
                self.consume_newlines();
                if !self.match_kind(TokenKind::Comma) {
                    break;
                }
                self.consume_newlines();
            }
            self.expect(TokenKind::RightBrace, "expected `}` after hash pattern")?;
            return Ok(MatchPattern::Hash(entries));
        }

        let name = self.expect_identifier("expected a match pattern")?;
        if name == "_" {
            if self.check(TokenKind::DotDot) {
                return Err(self.error_here("range pattern bounds must be literal values"));
            }
            if self.check(TokenKind::At) {
                return Err(self.error_here("alias pattern requires a binding identifier"));
            }
            return Ok(MatchPattern::Wildcard);
        }
        if self.match_kind(TokenKind::At) {
            let pattern = self.match_pattern_atom()?;
            return Ok(MatchPattern::Alias {
                name,
                pattern: Box::new(pattern),
            });
        }
        if self.check(TokenKind::DotDot) {
            return Err(self.error_here("range pattern bounds must be literal values"));
        }
        if self.match_kind(TokenKind::LeftParen) {
            if matches!(self.peek().kind, TokenKind::Identifier(_))
                && self
                    .tokens
                    .get(self.current + 1)
                    .is_some_and(|token| token.kind == TokenKind::Colon)
            {
                let mut fields = Vec::new();
                while !self.check(TokenKind::RightParen) {
                    let field_name = self.expect_identifier("expected a struct field name")?;
                    if fields.iter().any(|(existing, _)| existing == &field_name) {
                        return Err(self.error_here("duplicate field in named struct pattern"));
                    }
                    self.expect(TokenKind::Colon, "expected `:` after struct field name")?;
                    fields.push((field_name, self.match_pattern()?));
                    if !self.match_kind(TokenKind::Comma) {
                        break;
                    }
                }
                self.expect(TokenKind::RightParen, "expected `)` after struct pattern")?;
                return Ok(MatchPattern::NamedStruct {
                    type_name: name,
                    fields,
                });
            }
            let mut fields = Vec::new();
            while !self.check(TokenKind::RightParen) {
                fields.push(self.match_pattern()?);
                if !self.match_kind(TokenKind::Comma) {
                    break;
                }
            }
            self.expect(TokenKind::RightParen, "expected `)` after struct pattern")?;
            return Ok(MatchPattern::Struct {
                type_name: name,
                fields,
            });
        }
        if !self.match_kind(TokenKind::DoubleColon) {
            return Ok(MatchPattern::Identifier(name));
        }

        let variant_name = self.expect_identifier("expected an enum variant name")?;
        let payload = if self.match_kind(TokenKind::LeftParen) {
            let pattern = self.match_pattern()?;
            self.expect(
                TokenKind::RightParen,
                "expected `)` after enum payload pattern",
            )?;
            Some(Box::new(pattern))
        } else {
            None
        };
        Ok(MatchPattern::EnumVariant {
            enum_name: name,
            variant_name,
            payload,
        })
    }

    fn finish_literal_pattern(&mut self, literal: Literal) -> Result<MatchPattern, SimplyError> {
        if self.match_kind(TokenKind::DotDot) {
            let end = self.range_pattern_endpoint()?;
            return Ok(MatchPattern::Range {
                start: Some(literal),
                end,
            });
        }
        Ok(MatchPattern::Literal(literal))
    }

    fn range_pattern_endpoint(&mut self) -> Result<Option<Literal>, SimplyError> {
        let literal = match self.peek().kind.clone() {
            TokenKind::Colon
            | TokenKind::If
            | TokenKind::Pipe
            | TokenKind::RightParen
            | TokenKind::RightBracket
            | TokenKind::RightBrace
            | TokenKind::Comma
            | TokenKind::Newline
            | TokenKind::Eof => return Ok(None),
            TokenKind::String(value) => Literal::String(value),
            TokenKind::Int(value) => Literal::Int(value),
            TokenKind::Float(value) => Literal::Float(value),
            TokenKind::True => Literal::Bool(true),
            TokenKind::False => Literal::Bool(false),
            TokenKind::Minus => {
                self.advance();
                let token = self.advance().clone();
                return match token.kind {
                    TokenKind::Int(value) => value
                        .checked_neg()
                        .map(|value| Some(Literal::Int(value)))
                        .ok_or_else(|| SimplyError::Parse {
                            span: token.span,
                            code: DiagnosticCode::InvalidNumber,
                            message: "integer pattern bound is out of range".into(),
                        }),
                    TokenKind::Float(value) => Ok(Some(Literal::Float(-value))),
                    _ => Err(SimplyError::Parse {
                        span: token.span,
                        code: DiagnosticCode::UnexpectedToken,
                        message: "range pattern bounds must be literal values".into(),
                    }),
                };
            }
            _ => {
                return Err(self.error_here("range pattern bounds must be literal values"));
            }
        };
        self.advance();
        Ok(Some(literal))
    }

    fn is_match_pattern_start(&self) -> bool {
        if !self.is_match_pattern_atom_start() {
            return false;
        }

        let mut depth = 0usize;
        for token in &self.tokens[self.current..] {
            match &token.kind {
                TokenKind::Newline | TokenKind::Eof => return false,
                TokenKind::LeftParen => depth += 1,
                TokenKind::LeftBracket | TokenKind::LeftBrace => depth += 1,
                TokenKind::RightParen => {
                    let Some(new_depth) = depth.checked_sub(1) else {
                        return false;
                    };
                    depth = new_depth;
                }
                TokenKind::RightBracket | TokenKind::RightBrace => {
                    let Some(new_depth) = depth.checked_sub(1) else {
                        return false;
                    };
                    depth = new_depth;
                }
                TokenKind::Colon if depth == 0 => return true,
                _ => {}
            }
        }
        false
    }

    fn message_statement(&mut self) -> Result<Stmt, SimplyError> {
        let receiver_type = self.expect_identifier("expected a receiver type after `on`")?;
        self.expect(TokenKind::Receive, "expected `receive` after receiver type")?;
        let name = self.expect_identifier("expected a message name after `receive`")?;
        let parameters = if self.match_kind(TokenKind::LeftParen) {
            self.parameter_list()?
        } else {
            Vec::new()
        };
        self.expect(TokenKind::Colon, "expected `:` after message declaration")?;
        self.consume_newlines();
        let body = self.block_until(TokenKind::End, TokenKind::End)?;
        self.expect(TokenKind::End, "expected `end` after message declaration")?;
        Ok(Stmt::Message {
            receiver_type,
            name,
            parameters,
            body: body.into(),
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

    fn try_statement(&mut self) -> Result<Stmt, SimplyError> {
        self.expect(TokenKind::Colon, "expected `:` after try")?;
        self.consume_newlines();
        let try_body = self.block_until_any(&[TokenKind::Catch, TokenKind::Finally])?;

        let mut catches = Vec::new();
        while self.match_kind(TokenKind::Catch) {
            let binding = if self.check(TokenKind::Colon) || self.check(TokenKind::As) {
                None
            } else {
                Some(self.expect_identifier("expected an error name or `:` after `catch`")?)
            };
            let code = if self.match_kind(TokenKind::As) {
                Some(self.expect_diagnostic_code()?)
            } else {
                None
            };
            self.expect(TokenKind::Colon, "expected `:` after catch")?;
            self.consume_newlines();
            let body =
                self.block_until_any(&[TokenKind::Catch, TokenKind::Finally, TokenKind::End])?;
            catches.push(CatchClause {
                binding,
                code,
                body,
            });
        }

        let finally_body = if self.match_kind(TokenKind::Finally) {
            self.expect(TokenKind::Colon, "expected `:` after finally")?;
            self.consume_newlines();
            self.block_until(TokenKind::End, TokenKind::End)?
        } else {
            Vec::new()
        };

        if catches.is_empty() && finally_body.is_empty() {
            return Err(self.error_here("try requires `catch` or `finally`"));
        }
        self.expect(TokenKind::End, "expected `end` after try statement")?;
        Ok(Stmt::Try {
            try_body,
            catches,
            finally_body,
        })
    }

    fn block_until(
        &mut self,
        first_end: TokenKind,
        second_end: TokenKind,
    ) -> Result<Vec<Stmt>, SimplyError> {
        self.block_until_any(&[first_end, second_end])
    }

    fn block_until_any(&mut self, terminators: &[TokenKind]) -> Result<Vec<Stmt>, SimplyError> {
        let mut statements = Vec::new();
        while !terminators.iter().any(|kind| self.check(kind.clone())) {
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

    fn is_destructure_statement(&self) -> bool {
        self.is_destructure_target_statement(TokenKind::Is)
    }

    fn is_destructure_assignment_statement(&self) -> bool {
        self.is_destructure_target_statement(TokenKind::Arrow)
    }

    fn is_destructure_target_statement(&self, terminator: TokenKind) -> bool {
        let Some(kind) = self.tokens.get(self.current).map(|token| &token.kind) else {
            return false;
        };
        let start = match kind {
            TokenKind::LeftParen | TokenKind::LeftBracket | TokenKind::LeftBrace => self.current,
            TokenKind::Identifier(_) => {
                let Some(next) = self.tokens.get(self.current + 1).map(|token| &token.kind) else {
                    return false;
                };
                match next {
                    TokenKind::LeftParen => self.current + 1,
                    TokenKind::DoubleColon => {
                        let variant = self.current + 2;
                        if !matches!(
                            self.tokens.get(variant).map(|token| &token.kind),
                            Some(TokenKind::Identifier(_))
                        ) {
                            return false;
                        }
                        if matches!(
                            self.tokens.get(variant + 1).map(|token| &token.kind),
                            Some(TokenKind::LeftParen)
                        ) {
                            variant + 1
                        } else {
                            return matches!(
                                self.tokens.get(variant + 1).map(|token| &token.kind),
                                Some(token) if token == &terminator
                            );
                        }
                    }
                    TokenKind::At => self.current + 2,
                    _ => return false,
                }
            }
            _ => return false,
        };
        if matches!(kind, TokenKind::Identifier(_))
            && matches!(
                self.tokens.get(self.current + 1).map(|token| &token.kind),
                Some(TokenKind::At)
            )
        {
            let mut depth = 0usize;
            for token in self.tokens.iter().skip(start) {
                match &token.kind {
                    TokenKind::LeftParen | TokenKind::LeftBracket | TokenKind::LeftBrace => {
                        depth += 1;
                    }
                    TokenKind::RightParen | TokenKind::RightBracket | TokenKind::RightBrace => {
                        depth = depth.saturating_sub(1);
                    }
                    token if *token == terminator && depth == 0 => return true,
                    TokenKind::Eof | TokenKind::Newline if depth == 0 => return false,
                    _ => {}
                }
            }
            return false;
        }
        let opening = &self.tokens[start].kind;
        let closing = match opening {
            TokenKind::LeftParen => TokenKind::RightParen,
            TokenKind::LeftBracket => TokenKind::RightBracket,
            TokenKind::LeftBrace => TokenKind::RightBrace,
            _ => return false,
        };
        let mut depth = 0usize;
        for (index, token) in self.tokens.iter().enumerate().skip(start) {
            match &token.kind {
                TokenKind::LeftParen | TokenKind::LeftBracket | TokenKind::LeftBrace => {
                    depth += 1;
                }
                TokenKind::RightParen | TokenKind::RightBracket | TokenKind::RightBrace => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        return token.kind == closing
                            && matches!(
                                self.tokens.get(index + 1).map(|token| &token.kind),
                                Some(token) if token == &terminator
                            );
                    }
                }
                TokenKind::Eof | TokenKind::Newline if depth > 0 => return false,
                _ => {}
            }
        }
        false
    }

    fn expression(&mut self) -> Result<Expr, SimplyError> {
        self.parse_binary(0)
    }

    fn type_name(&mut self) -> Result<Type, SimplyError> {
        if self.type_depth >= MAX_PARSER_NESTING {
            return Err(self.error_here("maximum type nesting depth exceeded"));
        }
        self.type_depth += 1;
        let result = self.type_name_inner();
        self.type_depth -= 1;
        result
    }

    fn type_name_inner(&mut self) -> Result<Type, SimplyError> {
        if self.match_kind(TokenKind::LeftParen) {
            let mut types = vec![self.type_name()?];
            while self.match_kind(TokenKind::Comma) {
                types.push(self.type_name()?);
            }
            self.expect(TokenKind::RightParen, "expected `)` after tuple type")?;
            return Ok(Type::Tuple(types));
        }

        let token = self.advance().clone();
        let name = match token.kind {
            TokenKind::Identifier(name) => name,
            TokenKind::Array => "Array".into(),
            TokenKind::List => "List".into(),
            TokenKind::Hash => "Hash".into(),
            TokenKind::Tree => "Tree".into(),
            TokenKind::Matrix => "Matrix".into(),
            _ => return Err(self.error_at(token.span, "expected a supported type")),
        };
        let base = match name.as_str() {
            "Unit" => Type::Unit,
            "String" => Type::String,
            "Int" => Type::Int,
            "Float" => Type::Float,
            "Bool" => Type::Bool,
            "Range" => Type::Range,
            "CsvStream" => Type::CsvStream,
            "Array" => Type::Array(Box::new(Type::Int)),
            "List" => Type::List(Box::new(Type::Int)),
            "Vector" => Type::Vector(Box::new(Type::Unknown), None),
            "Hash" => Type::Hash,
            "Tree" => Type::Tree,
            "Matrix" => Type::Matrix,
            "Tuple" => Type::Tuple(Vec::new()),
            _ if self.enum_names.contains(&name) => Type::Enum(DeclarationIdentity::unresolved(
                name.clone(),
                DeclarationKind::Enum,
            )),
            _ => Type::Struct(DeclarationIdentity::unresolved(
                name.clone(),
                DeclarationKind::Struct,
            )),
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
            let mut dimensions = Vec::new();
            while self.match_kind(TokenKind::Comma) {
                dimensions.push(self.type_dimension()?);
            }
            self.expect(TokenKind::RightBracket, "expected `]` after type")?;
            Ok(match name.as_str() {
                "Array" => Type::Array(Box::new(element)),
                "List" => Type::List(Box::new(element)),
                "Vector" if dimensions.len() <= 1 => {
                    Type::Vector(Box::new(element), dimensions.first().copied().flatten())
                }
                "Matrix" if dimensions.is_empty() || dimensions.len() == 2 => Type::TypedMatrix(
                    Box::new(element),
                    dimensions.first().copied().flatten(),
                    dimensions.get(1).copied().flatten(),
                ),
                "Vector" => {
                    return Err(self.error_here("Vector type accepts at most one dimension"));
                }
                "Matrix" => {
                    return Err(self.error_here("Matrix type dimensions must be rows and columns"));
                }
                _ => base,
            })
        } else {
            Ok(base)
        }
    }

    fn type_dimension(&mut self) -> Result<Option<usize>, SimplyError> {
        match self.advance().kind {
            TokenKind::Int(value) if value > 0 => usize::try_from(value)
                .map(Some)
                .map_err(|_| self.error_here("type dimension is too large")),
            TokenKind::Question => Ok(None),
            _ => Err(self.error_here("expected a positive dimension or `?`")),
        }
    }

    fn parse_binary(&mut self, minimum_precedence: u8) -> Result<Expr, SimplyError> {
        if self.expression_depth >= MAX_PARSER_NESTING {
            return Err(self.error_here("maximum expression nesting depth exceeded"));
        }
        self.expression_depth += 1;
        let result = self.parse_binary_inner(minimum_precedence);
        self.expression_depth -= 1;
        result
    }

    fn parse_binary_inner(&mut self, minimum_precedence: u8) -> Result<Expr, SimplyError> {
        let mut left = self.parse_unary_inner()?;
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
        if self.expression_depth >= MAX_PARSER_NESTING {
            return Err(self.error_here("maximum expression nesting depth exceeded"));
        }
        self.expression_depth += 1;
        let result = self.parse_unary_inner();
        self.expression_depth -= 1;
        result
    }

    fn parse_unary_inner(&mut self) -> Result<Expr, SimplyError> {
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
            TokenKind::MultiplyWord if self.check(TokenKind::LeftParen) => {
                Ok(Expr::Identifier("multiply".into()))
            }
            TokenKind::Transpose if self.check(TokenKind::LeftParen) => {
                Ok(Expr::Identifier("transpose".into()))
            }
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
            TokenKind::Match => self.match_expression(),
            TokenKind::Tree => Ok(Expr::Identifier("tree".into())),
            TokenKind::Count => Ok(Expr::Identifier("count".into())),
            TokenKind::LeftParen => {
                let values = self.expression_list(TokenKind::RightParen)?;
                if values.len() == 1 {
                    Ok(values.into_iter().next().expect("one value was checked"))
                } else {
                    Ok(Expr::Tuple(values))
                }
            }
            _ => Err(SimplyError::Parse {
                span: token.span,
                code: DiagnosticCode::ExpectedExpression,
                message: "expected a value after the statement".into(),
            }),
        }?;

        if let Expr::Identifier(name) = &expression
            && self.check(TokenKind::LeftParen)
        {
            expression = self.call_expression(name.clone())?;
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
        if self.match_kind(TokenKind::DoubleColon) {
            let message = self.expect_message_name()?;
            let arguments = if self.check(TokenKind::LeftParen) {
                self.advance();
                self.argument_list()?
            } else {
                Vec::new()
            };
            if let Expr::Identifier(enum_name) = &expression
                && (self.enum_names.contains(enum_name)
                    || enum_name.chars().next().is_some_and(char::is_uppercase))
            {
                expression = Expr::EnumVariant {
                    enum_name: enum_name.clone(),
                    variant_name: message,
                    arguments,
                };
            } else {
                expression = Expr::MessageDispatch {
                    receiver: Box::new(expression),
                    message,
                    arguments,
                };
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
                return Err(self.error_here("pipeline cannot continue after a terminal step"));
            }
            if self.match_kind(TokenKind::Where) {
                steps.push(PipelineStep::Where(self.expression()?));
            } else if self.match_kind(TokenKind::Derive) {
                steps.push(PipelineStep::Derive(self.expression()?));
            } else if self.match_kind(TokenKind::Take) {
                steps.push(self.take_step()?);
            } else if self.match_kind(TokenKind::Skip) {
                steps.push(self.skip_step()?);
            } else if self.match_kind(TokenKind::StepBy) {
                steps.push(self.step_by_step()?);
            } else if self.match_kind(TokenKind::TakeWhile) {
                steps.push(PipelineStep::TakeWhile(self.expression()?));
            } else if self.match_kind(TokenKind::DropWhile) {
                steps.push(PipelineStep::DropWhile(self.expression()?));
            } else if self.match_kind(TokenKind::Distinct) {
                steps.push(PipelineStep::Distinct);
            } else if self.match_kind(TokenKind::Partition) {
                steps.push(self.partition_step()?);
                terminal = true;
            } else if self.match_kind(TokenKind::Sum) {
                steps.push(PipelineStep::Sum);
                terminal = true;
            } else if self.match_kind(TokenKind::Count) {
                steps.push(PipelineStep::Count);
                terminal = true;
            } else if self.match_kind(TokenKind::Average) {
                steps.push(PipelineStep::Average);
                terminal = true;
            } else if self.match_kind(TokenKind::Min) {
                steps.push(PipelineStep::Min);
                terminal = true;
            } else if self.match_kind(TokenKind::Max) {
                steps.push(PipelineStep::Max);
                terminal = true;
            } else if self.match_word("any") {
                steps.push(PipelineStep::Any);
                terminal = true;
            } else if self.match_word("all") {
                steps.push(PipelineStep::All);
                terminal = true;
            } else if self.match_kind(TokenKind::WriteCsv) {
                self.expect(TokenKind::LeftParen, "expected `(` after write_csv")?;
                let path = self.expression()?;
                self.expect(TokenKind::RightParen, "expected `)` after write_csv path")?;
                steps.push(PipelineStep::WriteCsv(path));
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

    /// Parse the concise declarative flow form and lower it to the existing
    /// pipeline expression so it shares all runtime and type-checking behavior.
    fn flow_statement(&mut self) -> Result<Stmt, SimplyError> {
        let name = self.expect_identifier("expected a flow name")?;
        self.expect(TokenKind::From, "expected `from` after flow name")?;
        let source = self.expression()?;
        self.expect(TokenKind::Colon, "expected `:` after flow source")?;
        self.consume_newlines();

        let mut steps = Vec::new();
        let mut terminal = false;
        while !self.check(TokenKind::End) {
            if self.match_kind(TokenKind::Newline) {
                continue;
            }
            if terminal {
                return Err(self.error_here("flow cannot continue after a terminal step"));
            }
            if self.match_kind(TokenKind::Partition) {
                steps.push(self.partition_step()?);
                terminal = true;
            } else if self.match_kind(TokenKind::Where) {
                steps.push(PipelineStep::Where(self.expression()?));
            } else if self.match_kind(TokenKind::Derive) {
                steps.push(PipelineStep::Derive(self.expression()?));
            } else if self.match_kind(TokenKind::Take) {
                steps.push(self.take_step()?);
            } else if self.match_kind(TokenKind::Skip) {
                steps.push(self.skip_step()?);
            } else if self.match_kind(TokenKind::StepBy) {
                steps.push(self.step_by_step()?);
            } else if self.match_kind(TokenKind::TakeWhile) {
                steps.push(PipelineStep::TakeWhile(self.expression()?));
            } else if self.match_kind(TokenKind::DropWhile) {
                steps.push(PipelineStep::DropWhile(self.expression()?));
            } else if self.match_kind(TokenKind::Distinct) {
                steps.push(PipelineStep::Distinct);
            } else if self.match_kind(TokenKind::Sum) {
                steps.push(PipelineStep::Sum);
                terminal = true;
            } else if self.match_kind(TokenKind::Count) {
                steps.push(PipelineStep::Count);
                terminal = true;
            } else if self.match_kind(TokenKind::Average) {
                steps.push(PipelineStep::Average);
                terminal = true;
            } else if self.match_kind(TokenKind::Min) {
                steps.push(PipelineStep::Min);
                terminal = true;
            } else if self.match_kind(TokenKind::Max) {
                steps.push(PipelineStep::Max);
                terminal = true;
            } else if self.match_word("any") {
                steps.push(PipelineStep::Any);
                terminal = true;
            } else if self.match_word("all") {
                steps.push(PipelineStep::All);
                terminal = true;
            } else if self.match_kind(TokenKind::WriteCsv) {
                self.expect(TokenKind::LeftParen, "expected `(` after write_csv")?;
                let path = self.expression()?;
                self.expect(TokenKind::RightParen, "expected `)` after write_csv path")?;
                steps.push(PipelineStep::WriteCsv(path));
                terminal = true;
            } else if self.match_kind(TokenKind::Chunk) {
                let token = self.advance().clone();
                let size = match token.kind {
                    TokenKind::Int(value) if value > 0 => value,
                    _ => {
                        return Err(
                            self.error_at(token.span, "chunk size must be a positive integer")
                        );
                    }
                };
                steps.push(PipelineStep::Chunk(size));
            } else if self.match_word("parallel") {
                let token = self.advance().clone();
                let workers = match token.kind {
                    TokenKind::Int(value) if value > 0 => value,
                    _ => {
                        return Err(self.error_at(
                            token.span,
                            "parallel worker count must be a positive integer",
                        ));
                    }
                };
                steps.push(PipelineStep::Parallel(workers));
            } else if self.match_kind(TokenKind::Checkpoint) {
                steps.push(PipelineStep::Checkpoint(self.expression()?));
            } else {
                return Err(self.error_here("expected flow step"));
            }
            self.expect(TokenKind::Newline, "expected a new line after flow step")?;
        }
        self.expect(TokenKind::End, "expected `end` after flow")?;
        if !terminal {
            return Err(self.error_here("flow must end with a terminal step"));
        }
        Ok(Stmt::Flow {
            name,
            source,
            steps,
        })
    }

    fn take_step(&mut self) -> Result<PipelineStep, SimplyError> {
        let token = self.advance().clone();
        match token.kind {
            TokenKind::Int(count) if count >= 0 => Ok(PipelineStep::Take(count)),
            _ => Err(self.error_at(token.span, "take count must be a non-negative integer")),
        }
    }

    fn skip_step(&mut self) -> Result<PipelineStep, SimplyError> {
        let token = self.advance().clone();
        match token.kind {
            TokenKind::Int(count) if count >= 0 => Ok(PipelineStep::Skip(count)),
            _ => Err(self.error_at(token.span, "skip count must be a non-negative integer")),
        }
    }

    fn step_by_step(&mut self) -> Result<PipelineStep, SimplyError> {
        let token = self.advance().clone();
        match token.kind {
            TokenKind::Int(interval) if interval > 0 => Ok(PipelineStep::StepBy(interval)),
            _ => Err(self.error_at(token.span, "`step_by` interval must be a positive integer")),
        }
    }

    fn partition_step(&mut self) -> Result<PipelineStep, SimplyError> {
        let item = self.expect_identifier("expected an item name after `partition`")?;
        self.expect(TokenKind::Colon, "expected `:` after partition item")?;
        self.consume_newlines();
        let mut rules = Vec::new();
        let mut has_otherwise = false;
        while !self.check(TokenKind::End) {
            if self.match_kind(TokenKind::Newline) {
                continue;
            }
            if has_otherwise {
                return Err(self.error_here("`otherwise` must be the final partition rule"));
            }
            let is_otherwise = self.match_word("otherwise");
            let condition = if is_otherwise {
                has_otherwise = true;
                None
            } else {
                Some(self.expression()?)
            };
            self.expect(TokenKind::Arrow, "expected `->` before partition category")?;
            let category = self.expect_identifier("expected a category name after `->`")?;
            rules.push(PartitionRule {
                condition,
                category,
            });
            self.expect(
                TokenKind::Newline,
                "expected a new line after partition rule",
            )?;
        }
        self.expect(TokenKind::End, "expected `end` after partition rules")?;
        Ok(PipelineStep::Partition { item, rules })
    }

    fn call_expression(&mut self, name: String) -> Result<Expr, SimplyError> {
        self.expect(TokenKind::LeftParen, "expected `(` after function name")?;
        let arguments = self.argument_list()?;
        Ok(Expr::Call { name, arguments })
    }

    fn argument_list(&mut self) -> Result<Vec<Expr>, SimplyError> {
        let mut arguments = Vec::new();
        self.consume_newlines();
        if !self.check(TokenKind::RightParen) {
            loop {
                arguments.push(self.expression()?);
                self.consume_newlines();
                if !self.match_kind(TokenKind::Comma) {
                    break;
                }
                self.consume_newlines();
                if self.check(TokenKind::RightParen) {
                    return Err(self.error_here("expected an argument after `,`"));
                }
            }
        }
        self.expect(TokenKind::RightParen, "expected `)` after arguments")?;
        Ok(arguments)
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
        let token = self.advance().clone();
        match token.kind {
            TokenKind::Identifier(name) => Ok(name),
            _ => Err(self.error_at(token.span, message)),
        }
    }

    fn expect_diagnostic_code(&mut self) -> Result<String, SimplyError> {
        let mut code = self.expect_identifier("expected a diagnostic code after `as`")?;
        while self.match_kind(TokenKind::Dot) {
            code.push('.');
            code.push_str(&self.expect_identifier("expected a code segment after `.`")?);
            while self.match_kind(TokenKind::Minus) {
                code.push('-');
                code.push_str(&self.expect_identifier("expected a code segment after `-`")?);
            }
        }
        Ok(code)
    }

    fn expect_message_name(&mut self) -> Result<String, SimplyError> {
        let token = self.advance().clone();
        match token.kind {
            TokenKind::Identifier(name) => Ok(name),
            _ => Err(self.error_at(token.span, "expected an identifier after `::`")),
        }
    }

    fn error_here(&self, message: &str) -> SimplyError {
        self.error_at(self.peek().span.clone(), message)
    }

    fn error_at(&self, span: Span, message: &str) -> SimplyError {
        SimplyError::Parse {
            span,
            code: DiagnosticCode::UnexpectedToken,
            message: message.into(),
        }
    }

    #[allow(dead_code)]
    fn span_here(&self) -> Span {
        self.peek().span.clone()
    }
}
