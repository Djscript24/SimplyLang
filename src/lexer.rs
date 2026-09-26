//! lexer.rs — lexical analysis
//! Converts Simply source text into tokens and reports lexical diagnostics with source spans.
//! Key components: TokenKind, Token, and Lexer.
use crate::error::{DiagnosticCode, SimplyError, Span};

#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    Say,
    Sayln,
    Open,
    Fn,
    Return,
    If,
    Else,
    End,
    And,
    Or,
    Is,
    Mut,
    As,
    Gives,
    Array,
    List,
    Hash,
    Tree,
    Matrix,
    Pipeline,
    Flow,
    From,
    Partition,
    Where,
    Derive,
    Sum,
    Count,
    Average,
    Min,
    Max,
    WriteCsv,
    Chunk,
    Checkpoint,
    MultiplyWord,
    Transpose,
    For,
    While,
    Break,
    Continue,
    In,
    Throw,
    Try,
    Catch,
    Finally,
    Arrow,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Greater,
    GreaterEqual,
    Less,
    LessEqual,
    EqualEqual,
    NotEqual,
    Not,
    Colon,
    LeftParen,
    RightParen,
    Comma,
    LeftBracket,
    RightBracket,
    Dot,
    True,
    False,
    String(String),
    Int(i64),
    Float(f64),
    Identifier(String),
    Newline,
    Eof,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

pub struct Lexer<'a> {
    source: &'a str,
    index: usize,
    line: usize,
    column: usize,
}

impl<'a> Lexer<'a> {
    pub fn new(source: &'a str) -> Self {
        Self {
            source,
            index: 0,
            line: 1,
            column: 1,
        }
    }

    pub fn tokenize(mut self) -> Result<Vec<Token>, SimplyError> {
        let mut tokens = Vec::with_capacity(self.source.len().saturating_div(2));

        while let Some(ch) = self.peek() {
            match ch {
                ' ' | '\t' | '\r' => {
                    self.advance();
                }
                '\n' => {
                    let span = Span::new(self.line, self.column);
                    self.advance();
                    tokens.push(Token {
                        kind: TokenKind::Newline,
                        span,
                    });
                }
                '#' => self.skip_comment(),
                '"' => tokens.push(self.read_string()?),
                ':' => tokens.push(self.single_char(TokenKind::Colon)),
                '(' => tokens.push(self.single_char(TokenKind::LeftParen)),
                ')' => tokens.push(self.single_char(TokenKind::RightParen)),
                ',' => tokens.push(self.single_char(TokenKind::Comma)),
                '[' => tokens.push(self.single_char(TokenKind::LeftBracket)),
                ']' => tokens.push(self.single_char(TokenKind::RightBracket)),
                '.' => tokens.push(self.single_char(TokenKind::Dot)),
                '+' => tokens.push(self.single_char(TokenKind::Plus)),
                '*' => tokens.push(self.single_char(TokenKind::Star)),
                '/' => tokens.push(self.single_char(TokenKind::Slash)),
                '%' => tokens.push(self.single_char(TokenKind::Percent)),
                '>' => tokens.push(self.read_comparison(
                    '>',
                    TokenKind::Greater,
                    TokenKind::GreaterEqual,
                )?),
                '<' => {
                    tokens.push(self.read_comparison('<', TokenKind::Less, TokenKind::LessEqual)?)
                }
                '=' if self.next_is('=') => {
                    tokens.push(self.read_double_char(TokenKind::EqualEqual));
                }
                '!' if self.next_is('=') => {
                    tokens.push(self.read_double_char(TokenKind::NotEqual));
                }
                '-' if self.next_is('>') => {
                    tokens.push(self.read_arrow()?);
                }
                '-' => tokens.push(self.single_char(TokenKind::Minus)),
                '0'..='9' => tokens.push(self.read_number()?),
                'a'..='z' | 'A'..='Z' | '_' => tokens.push(self.read_word()),
                _ => {
                    return Err(SimplyError::Lex {
                        span: Span::new(self.line, self.column),
                        code: DiagnosticCode::InvalidCharacter,
                        message: format!("unexpected character `{ch}`"),
                    });
                }
            }
        }

        tokens.push(Token {
            kind: TokenKind::Eof,
            span: Span::new(self.line, self.column),
        });
        Ok(tokens)
    }

    fn peek(&self) -> Option<char> {
        self.source.get(self.index..)?.chars().next()
    }

    fn next_is(&self, expected: char) -> bool {
        self.source
            .get(self.index..)
            .and_then(|text| text.chars().nth(1))
            == Some(expected)
    }

    fn advance(&mut self) -> Option<char> {
        let ch = self.peek()?;
        self.index += ch.len_utf8();
        if ch == '\n' {
            self.line += 1;
            self.column = 1;
        } else {
            self.column += 1;
        }
        Some(ch)
    }

    fn skip_comment(&mut self) {
        while !matches!(self.peek(), None | Some('\n')) {
            self.advance();
        }
    }

    fn read_word(&mut self) -> Token {
        let span = Span::new(self.line, self.column);
        let start = self.index;

        while matches!(self.peek(), Some('a'..='z' | 'A'..='Z' | '0'..='9' | '_')) {
            self.advance();
        }

        let word = &self.source[start..self.index];
        let kind = match word {
            "Say" => TokenKind::Say,
            "Sayln" => TokenKind::Sayln,
            "open" => TokenKind::Open,
            "fn" => TokenKind::Fn,
            "return" => TokenKind::Return,
            "if" => TokenKind::If,
            "else" => TokenKind::Else,
            "end" => TokenKind::End,
            "and" => TokenKind::And,
            "or" => TokenKind::Or,
            "is" => TokenKind::Is,
            "mut" => TokenKind::Mut,
            "as" => TokenKind::As,
            "gives" => TokenKind::Gives,
            "array" => TokenKind::Array,
            "list" => TokenKind::List,
            "hash" => TokenKind::Hash,
            "tree" => TokenKind::Tree,
            "matrix" => TokenKind::Matrix,
            "pipeline" => TokenKind::Pipeline,
            "flow" => TokenKind::Flow,
            "from" => TokenKind::From,
            "partition" => TokenKind::Partition,
            "where" => TokenKind::Where,
            "derive" => TokenKind::Derive,
            "sum" => TokenKind::Sum,
            "count" => TokenKind::Count,
            "average" => TokenKind::Average,
            "min" => TokenKind::Min,
            "max" => TokenKind::Max,
            "write_csv" => TokenKind::WriteCsv,
            "chunk" => TokenKind::Chunk,
            "checkpoint" => TokenKind::Checkpoint,
            "multiply" => TokenKind::MultiplyWord,
            "transpose" => TokenKind::Transpose,
            "for" => TokenKind::For,
            "while" => TokenKind::While,
            "break" => TokenKind::Break,
            "continue" => TokenKind::Continue,
            "in" => TokenKind::In,
            "throw" => TokenKind::Throw,
            "try" => TokenKind::Try,
            "catch" => TokenKind::Catch,
            "finally" => TokenKind::Finally,
            "not" => TokenKind::Not,
            "true" => TokenKind::True,
            "false" => TokenKind::False,
            _ => TokenKind::Identifier(word.to_owned()),
        };

        Token { kind, span }
    }

    fn read_arrow(&mut self) -> Result<Token, SimplyError> {
        let span = Span::new(self.line, self.column);
        self.advance();
        self.advance();
        Ok(Token {
            kind: TokenKind::Arrow,
            span,
        })
    }

    fn single_char(&mut self, kind: TokenKind) -> Token {
        let span = Span::new(self.line, self.column);
        self.advance();
        Token { kind, span }
    }

    fn read_double_char(&mut self, kind: TokenKind) -> Token {
        let span = Span::new(self.line, self.column);
        self.advance();
        self.advance();
        Token { kind, span }
    }

    fn read_comparison(
        &mut self,
        _character: char,
        single: TokenKind,
        double: TokenKind,
    ) -> Result<Token, SimplyError> {
        if self.next_is('=') {
            Ok(self.read_double_char(double))
        } else {
            Ok(self.single_char(single))
        }
    }

    fn read_number(&mut self) -> Result<Token, SimplyError> {
        let span = Span::new(self.line, self.column);
        let start = self.index;

        while matches!(self.peek(), Some('0'..='9')) {
            self.advance();
        }

        let mut is_float = false;
        if self.peek() == Some('.') {
            is_float = true;
            self.advance();
            while matches!(self.peek(), Some('0'..='9')) {
                self.advance();
            }
        }

        if matches!(self.peek(), Some('e' | 'E')) {
            is_float = true;
            self.advance();
            if matches!(self.peek(), Some('+' | '-')) {
                self.advance();
            }
            let exp_start = self.index;
            while matches!(self.peek(), Some('0'..='9')) {
                self.advance();
            }
            if self.index == exp_start {
                return Err(SimplyError::Lex {
                    span,
                    code: DiagnosticCode::InvalidNumber,
                    message: "expected digits after exponent".into(),
                });
            }
        }

        let text = &self.source[start..self.index];
        let kind = if is_float {
            match text.parse::<f64>() {
                Ok(value) if value.is_finite() => TokenKind::Float(value),
                Ok(_) => {
                    return Err(SimplyError::Lex {
                        span,
                        code: DiagnosticCode::InvalidNumber,
                        message: format!("floating-point number `{text}` is not finite"),
                    });
                }
                Err(_) => {
                    return Err(SimplyError::Lex {
                        span,
                        code: DiagnosticCode::InvalidNumber,
                        message: format!("invalid floating-point number `{text}`"),
                    });
                }
            }
        } else {
            match text.parse::<i64>() {
                Ok(value) => TokenKind::Int(value),
                Err(_) => {
                    return Err(SimplyError::Lex {
                        span,
                        code: DiagnosticCode::InvalidNumber,
                        message: format!("invalid integer `{text}`"),
                    });
                }
            }
        };

        Ok(Token { kind, span })
    }

    fn read_string(&mut self) -> Result<Token, SimplyError> {
        let span = Span::new(self.line, self.column);
        self.advance(); // opening quote
        let mut value = String::new();

        loop {
            match self.peek() {
                None => {
                    return Err(SimplyError::Lex {
                        span,
                        code: DiagnosticCode::UnterminatedString,
                        message: "unterminated string".into(),
                    });
                }
                Some('"') => {
                    self.advance();
                    return Ok(Token {
                        kind: TokenKind::String(value),
                        span,
                    });
                }
                Some('\\') => {
                    self.advance();
                    let escaped = self.peek().ok_or_else(|| SimplyError::Lex {
                        span: span.clone(),
                        code: DiagnosticCode::UnterminatedString,
                        message: "unterminated escape sequence".into(),
                    })?;
                    self.advance();
                    match escaped {
                        'n' => value.push('\n'),
                        't' => value.push('\t'),
                        'r' => value.push('\r'),
                        '"' => value.push('"'),
                        '\\' => value.push('\\'),
                        other => {
                            return Err(SimplyError::Lex {
                                span,
                                code: DiagnosticCode::InvalidCharacter,
                                message: format!("unknown escape sequence `\\{other}`"),
                            });
                        }
                    }
                }
                Some('\n') => {
                    return Err(SimplyError::Lex {
                        span,
                        code: DiagnosticCode::UnterminatedString,
                        message: "strings cannot contain a raw newline".into(),
                    });
                }
                Some(ch) => {
                    value.push(ch);
                    self.advance();
                }
            }
        }
    }

    #[allow(dead_code)]
    pub fn source(&self) -> &str {
        self.source
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lexes_all_base_values() {
        let tokens = Lexer::new("Say \"hello\"\nSayln -42\nSay 3.14\nSayln true\nSay false\n")
            .tokenize()
            .unwrap();
        let kinds: Vec<TokenKind> = tokens.into_iter().map(|t| t.kind).collect();
        assert_eq!(
            kinds,
            vec![
                TokenKind::Say,
                TokenKind::String("hello".into()),
                TokenKind::Newline,
                TokenKind::Sayln,
                TokenKind::Minus,
                TokenKind::Int(42),
                TokenKind::Newline,
                TokenKind::Say,
                TokenKind::Float(314.0 / 100.0),
                TokenKind::Newline,
                TokenKind::Sayln,
                TokenKind::True,
                TokenKind::Newline,
                TokenKind::Say,
                TokenKind::False,
                TokenKind::Newline,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn lexes_assignment_tokens() {
        let tokens = Lexer::new("name is 10\nname -> 11\n").tokenize().unwrap();
        let kinds: Vec<TokenKind> = tokens.into_iter().map(|token| token.kind).collect();
        assert_eq!(
            kinds,
            vec![
                TokenKind::Identifier("name".into()),
                TokenKind::Is,
                TokenKind::Int(10),
                TokenKind::Newline,
                TokenKind::Identifier("name".into()),
                TokenKind::Arrow,
                TokenKind::Int(11),
                TokenKind::Newline,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn tracks_unicode_and_windows_newlines_without_losing_tokens() {
        let tokens = Lexer::new("Say \"é\"\r\nSay 2\r\n").tokenize().unwrap();
        assert_eq!(tokens[1].kind, TokenKind::String("é".into()));
        assert_eq!(tokens[2].span, Span::new(1, 9));
        assert_eq!(tokens[3].span, Span::new(2, 1));
    }

    #[test]
    fn rejects_unterminated_strings_as_lex_errors() {
        let error = Lexer::new("Say \"unterminated").tokenize().unwrap_err();
        assert!(matches!(error, SimplyError::Lex { .. }));
        assert!(error.to_string().contains("E0102"));
    }

    #[test]
    fn reports_invalid_numeric_literals_with_a_numeric_lex_error() {
        for source in ["Say 1e\n", "Say 9223372036854775808\n", "Say 1e9999\n"] {
            let error = Lexer::new(source).tokenize().unwrap_err();

            assert_eq!(error.code(), DiagnosticCode::InvalidNumber, "{source}");
            assert_eq!(error.category(), crate::error::DiagnosticCategory::Lex);
            assert!(error.to_string().contains("E0106"), "{source}");
            assert!(
                error
                    .to_string()
                    .contains("This number literal is not valid."),
                "{source}"
            );
        }
    }

    #[test]
    fn renders_invalid_number_category_and_code() {
        let error = Lexer::new("Say 1e\n").tokenize().unwrap_err();
        let rendered = error.render("number.si", "Say 1e\n");

        assert!(rendered.starts_with("error[E0106] (Lex error)"));
        assert!(rendered.contains("= What happened: This number literal is not valid."));
        assert!(rendered.contains("= Try this: Check the number's digits"));
        assert!(rendered.contains("= Details: expected digits after exponent"));
        assert!(rendered.contains("1 | Say 1e"));
    }

    #[test]
    fn tracks_scalar_columns_after_unicode_source() {
        let error = Lexer::new("Say \"é你好😀\"\nSay @\n")
            .tokenize()
            .unwrap_err();

        assert_eq!(error.code(), DiagnosticCode::InvalidCharacter);
        assert_eq!(error.span(), &Span::new(2, 5));
    }
}
