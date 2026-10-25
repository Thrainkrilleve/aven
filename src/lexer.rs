// src/lexer.rs — Tokenizer with accurate line/column tracking

#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    // Keywords
    KwRelation,
    KwForward,
    KwSolve,
    KwWith,
    KwUnit,
    KwSignal,
    KwMinimize,
    KwMaximize,

    // Literals & Identifiers
    Ident(String),
    Number(f64),

    // Symbols & Operators
    Plus,
    Minus,
    Star,
    Slash,
    Caret,
    Equal,
    LessEqual,
    GreaterEqual,
    Less,
    Greater,
    Colon,
    Comma,
    LeftBrace,
    RightBrace,
    LeftParen,
    RightParen,

    Eof,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub text: String,
    pub line: usize,
    pub col: usize,
}

#[derive(Debug, Clone)]
pub struct LexError {
    pub message: String,
    pub line: usize,
    pub col: usize,
}

impl std::fmt::Display for LexError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Syntax error at {}:{}: {}",
            self.line, self.col, self.message
        )
    }
}

impl std::error::Error for LexError {}

pub struct Lexer<'a> {
    chars: Vec<char>,
    pos: usize,
    line: usize,
    col: usize,
    _source: &'a str,
}

impl<'a> Lexer<'a> {
    pub fn new(source: &'a str) -> Self {
        Self {
            chars: source.chars().collect(),
            pos: 0,
            line: 1,
            col: 1,
            _source: source,
        }
    }

    fn peek(&self) -> Option<char> {
        if self.pos < self.chars.len() {
            Some(self.chars[self.pos])
        } else {
            None
        }
    }

    fn peek_next(&self) -> Option<char> {
        if self.pos + 1 < self.chars.len() {
            Some(self.chars[self.pos + 1])
        } else {
            None
        }
    }

    fn advance(&mut self) -> Option<char> {
        if self.pos < self.chars.len() {
            let c = self.chars[self.pos];
            self.pos += 1;
            if c == '\n' {
                self.line += 1;
                self.col = 1;
            } else {
                self.col += 1;
            }
            Some(c)
        } else {
            None
        }
    }

    pub fn tokenize(&mut self) -> Result<Vec<Token>, LexError> {
        let mut tokens = Vec::new();

        while let Some(c) = self.peek() {
            // Skip whitespace
            if c.is_whitespace() {
                self.advance();
                continue;
            }

            // Skip line comments: // ...
            if c == '/' && self.peek_next() == Some('/') {
                while let Some(ch) = self.peek() {
                    if ch == '\n' {
                        break;
                    }
                    self.advance();
                }
                continue;
            }

            let start_line = self.line;
            let start_col = self.col;

            // Numbers
            if c.is_ascii_digit()
                || (c == '.' && self.peek_next().map_or(false, |n| n.is_ascii_digit()))
            {
                let tok = self.lex_number(start_line, start_col)?;
                tokens.push(tok);
                continue;
            }

            // Identifiers / Keywords
            if c.is_alphabetic() || c == '_' {
                let tok = self.lex_ident(start_line, start_col);
                tokens.push(tok);
                continue;
            }

            // Multi-char operators: <= and >=
            if c == '<' {
                if self.peek_next() == Some('=') {
                    self.advance();
                    self.advance();
                    tokens.push(Token {
                        kind: TokenKind::LessEqual,
                        text: "<=".to_string(),
                        line: start_line,
                        col: start_col,
                    });
                } else {
                    self.advance();
                    tokens.push(Token {
                        kind: TokenKind::Less,
                        text: "<".to_string(),
                        line: start_line,
                        col: start_col,
                    });
                }
                continue;
            }

            if c == '>' {
                if self.peek_next() == Some('=') {
                    self.advance();
                    self.advance();
                    tokens.push(Token {
                        kind: TokenKind::GreaterEqual,
                        text: ">=".to_string(),
                        line: start_line,
                        col: start_col,
                    });
                } else {
                    self.advance();
                    tokens.push(Token {
                        kind: TokenKind::Greater,
                        text: ">".to_string(),
                        line: start_line,
                        col: start_col,
                    });
                }
                continue;
            }

            // Punctuation & Operators
            self.advance();
            let kind = match c {
                '+' => TokenKind::Plus,
                '-' => TokenKind::Minus,
                '*' => TokenKind::Star,
                '/' => TokenKind::Slash,
                '^' => TokenKind::Caret,
                '=' => TokenKind::Equal,
                ':' => TokenKind::Colon,
                ',' => TokenKind::Comma,
                '{' => TokenKind::LeftBrace,
                '}' => TokenKind::RightBrace,
                '(' => TokenKind::LeftParen,
                ')' => TokenKind::RightParen,
                _ => {
                    return Err(LexError {
                        message: format!("unexpected character '{}'", c),
                        line: start_line,
                        col: start_col,
                    });
                }
            };

            tokens.push(Token {
                kind,
                text: c.to_string(),
                line: start_line,
                col: start_col,
            });
        }

        tokens.push(Token {
            kind: TokenKind::Eof,
            text: "<eof>".to_string(),
            line: self.line,
            col: self.col,
        });

        Ok(tokens)
    }

    fn lex_number(&mut self, start_line: usize, start_col: usize) -> Result<Token, LexError> {
        let mut s = String::new();
        let mut seen_dot = false;
        let mut seen_exp = false;

        while let Some(c) = self.peek() {
            if c.is_ascii_digit() {
                s.push(c);
                self.advance();
            } else if c == '.' && !seen_dot && !seen_exp {
                seen_dot = true;
                s.push(c);
                self.advance();
            } else if (c == 'e' || c == 'E') && !seen_exp {
                seen_exp = true;
                s.push(c);
                self.advance();
                if let Some(sign) = self.peek() {
                    if sign == '+' || sign == '-' {
                        s.push(sign);
                        self.advance();
                    }
                }
            } else {
                break;
            }
        }

        let val: f64 = s.parse().map_err(|e| LexError {
            message: format!("invalid numeric literal '{}': {}", s, e),
            line: start_line,
            col: start_col,
        })?;

        Ok(Token {
            kind: TokenKind::Number(val),
            text: s,
            line: start_line,
            col: start_col,
        })
    }

    fn lex_ident(&mut self, start_line: usize, start_col: usize) -> Token {
        let mut s = String::new();
        while let Some(c) = self.peek() {
            if c.is_alphanumeric() || c == '_' {
                s.push(c);
                self.advance();
            } else {
                break;
            }
        }

        let kind = match s.as_str() {
            "relation" => TokenKind::KwRelation,
            "forward" => TokenKind::KwForward,
            "solve" => TokenKind::KwSolve,
            "with" => TokenKind::KwWith,
            "unit" => TokenKind::KwUnit,
            "signal" => TokenKind::KwSignal,
            "minimize" => TokenKind::KwMinimize,
            "maximize" => TokenKind::KwMaximize,
            _ => TokenKind::Ident(s.clone()),
        };

        Token {
            kind,
            text: s,
            line: start_line,
            col: start_col,
        }
    }
}
