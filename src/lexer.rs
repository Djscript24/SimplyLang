use crate::error::{DiagnosticCode, SimplyError, Span};

#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    Say,
    Open,
    Fn,
    Return,
    If,
    Else,
    End,
    And,
    Or,
    Is,
    As,
    Gives,
    Array,
    List,
    Hash,
    Tree,
    Matrix,
    Pipeline,
    Filter,
    Map,
    Sum,
    Count,
    MultiplyWord,
    Transpose,
    For,
    While,
    Break,
    Continue,
    In,
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
    chars: Vec<char>,
    index: usize,
    line: usize,
    column: usize,
}

impl<'a> Lexer<'a> {
    pub fn new(source: &'a str) -> Self {
        Self {
            source,
            chars: source.chars().collect(),
            index: 0,
            line: 1,
            column: 1,
        }
    }

    pub fn tokenize(mut self) -> Result<Vec<Token>, SimplyError> {
        let mut tokens = Vec::new();

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
                '=' if self.chars.get(self.index + 1) == Some(&'=') => {
                    tokens.push(self.read_double_char(TokenKind::EqualEqual));
                }
                '!' if self.chars.get(self.index + 1) == Some(&'=') => {
                    tokens.push(self.read_double_char(TokenKind::NotEqual));
                }
                '-' if self.chars.get(self.index + 1) == Some(&'>') => {
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
        self.chars.get(self.index).copied()
    }

    fn advance(&mut self) -> Option<char> {
        let ch = self.peek()?;
        self.index += 1;
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

        let word: String = self.chars[start..self.index].iter().collect();
        let kind = match word.as_str() {
            "Say" => TokenKind::Say,
            "open" => TokenKind::Open,
            "fn" => TokenKind::Fn,
            "return" => TokenKind::Return,
            "if" => TokenKind::If,
            "else" => TokenKind::Else,
            "end" => TokenKind::End,
            "and" => TokenKind::And,
            "or" => TokenKind::Or,
            "is" => TokenKind::Is,
            "as" => TokenKind::As,
            "gives" => TokenKind::Gives,
            "array" => TokenKind::Array,
            "list" => TokenKind::List,
            "hash" => TokenKind::Hash,
            "tree" => TokenKind::Tree,
            "matrix" => TokenKind::Matrix,
            "pipeline" => TokenKind::Pipeline,
            "filter" => TokenKind::Filter,
            "map" => TokenKind::Map,
            "sum" => TokenKind::Sum,
            "count" => TokenKind::Count,
            "multiply" => TokenKind::MultiplyWord,
            "transpose" => TokenKind::Transpose,
            "for" => TokenKind::For,
            "while" => TokenKind::While,
            "break" => TokenKind::Break,
            "continue" => TokenKind::Continue,
            "in" => TokenKind::In,
            "not" => TokenKind::Not,
            "true" => TokenKind::True,
            "false" => TokenKind::False,
            _ => TokenKind::Identifier(word),
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
        if self.chars.get(self.index + 1) == Some(&'=') {
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
                    code: DiagnosticCode::InvalidCharacter,
                    message: "expected digits after exponent".into(),
                });
            }
        }

        let text: String = self.chars[start..self.index].iter().collect();
        let kind = if is_float {
            match text.parse::<f64>() {
                Ok(value) => TokenKind::Float(value),
                Err(_) => {
                    return Err(SimplyError::Lex {
                        span,
                        code: DiagnosticCode::InvalidCharacter,
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
                        code: DiagnosticCode::InvalidCharacter,
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
        let tokens = Lexer::new("Say \"hello\"\nSay -42\nSay 3.14\nSay true\nSay false\n")
            .tokenize()
            .unwrap();
        let kinds: Vec<TokenKind> = tokens.into_iter().map(|t| t.kind).collect();
        assert_eq!(
            kinds,
            vec![
                TokenKind::Say,
                TokenKind::String("hello".into()),
                TokenKind::Newline,
                TokenKind::Say,
                TokenKind::Minus,
                TokenKind::Int(42),
                TokenKind::Newline,
                TokenKind::Say,
                TokenKind::Float(314.0 / 100.0),
                TokenKind::Newline,
                TokenKind::Say,
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
    fn tracks_scalar_columns_after_unicode_source() {
        let error = Lexer::new("Say \"é你好😀\"\nSay @\n")
            .tokenize()
            .unwrap_err();

        assert_eq!(error.code(), DiagnosticCode::InvalidCharacter);
        assert_eq!(error.span(), &Span::new(2, 5));
    }
}
