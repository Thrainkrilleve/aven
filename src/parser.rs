// src/parser.rs — Recursive-descent parser for Aven source files
use crate::ast::*;
use crate::lexer::{Token, TokenKind};

#[derive(Debug, Clone)]
pub struct ParseError {
    pub message: String,
    pub line: usize,
    pub col: usize,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Parse error at {}:{}: {}",
            self.line, self.col, self.message
        )
    }
}

impl std::error::Error for ParseError {}

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, pos: 0 }
    }

    fn peek(&self) -> &Token {
        if self.pos < self.tokens.len() {
            &self.tokens[self.pos]
        } else {
            &self.tokens[self.tokens.len() - 1]
        }
    }

    fn peek_at(&self, offset: usize) -> &Token {
        if self.pos + offset < self.tokens.len() {
            &self.tokens[self.pos + offset]
        } else {
            &self.tokens[self.tokens.len() - 1]
        }
    }

    fn take(&mut self) -> Token {
        let tok = self.peek().clone();
        if self.pos < self.tokens.len() {
            self.pos += 1;
        }
        tok
    }

    fn expect(&mut self, expected: &TokenKind, desc: &str) -> Result<Token, ParseError> {
        let tok = self.peek().clone();
        if &tok.kind == expected {
            Ok(self.take())
        } else {
            Err(ParseError {
                message: format!("expected {}, found '{}'", desc, tok.text),
                line: tok.line,
                col: tok.col,
            })
        }
    }

    fn expect_ident(&mut self) -> Result<(String, usize, usize), ParseError> {
        let tok = self.peek().clone();
        if let TokenKind::Ident(ref s) = tok.kind {
            self.take();
            Ok((s.clone(), tok.line, tok.col))
        } else {
            Err(ParseError {
                message: format!("expected identifier, found '{}'", tok.text),
                line: tok.line,
                col: tok.col,
            })
        }
    }

    pub fn parse_module(&mut self, name: &str) -> Result<AvenModule, ParseError> {
        let mut module = AvenModule {
            name: name.to_string(),
            ..Default::default()
        };

        while self.peek().kind != TokenKind::Eof {
            match self.peek().kind {
                TokenKind::KwRelation => {
                    let block = self.parse_block(BlockKind::Relation)?;
                    module.blocks.push(block);
                }
                TokenKind::KwForward => {
                    let block = self.parse_block(BlockKind::Forward)?;
                    module.blocks.push(block);
                }
                TokenKind::KwSolve => {
                    let solve = self.parse_solve()?;
                    module.solves.push(solve);
                }
                TokenKind::KwUnit => {
                    let unit_decl = self.parse_unit_decl()?;
                    module.units.push(unit_decl);
                }
                TokenKind::KwSignal => {
                    let signal = self.parse_signal()?;
                    module.signals.push(signal);
                }
                _ => {
                    let tok = self.peek();
                    return Err(ParseError {
                        message: format!(
                            "expected top-level declaration ('relation', 'forward', 'solve', 'unit', 'signal'), found '{}'",
                            tok.text
                        ),
                        line: tok.line,
                        col: tok.col,
                    });
                }
            }
        }

        Ok(module)
    }

    fn parse_block(&mut self, kind: BlockKind) -> Result<BlockDecl, ParseError> {
        let start_tok = self.take(); // KwRelation or KwForward
        let (name, _line, _col) = self.expect_ident()?;
        self.expect(&TokenKind::LeftBrace, "'{'")?;

        let mut equations = Vec::new();
        while self.peek().kind != TokenKind::RightBrace && self.peek().kind != TokenKind::Eof {
            let eq = self.parse_equation()?;
            equations.push(eq);
        }

        self.expect(&TokenKind::RightBrace, "'}'")?;

        Ok(BlockDecl {
            kind,
            name,
            equations,
            line: start_tok.line,
            col: start_tok.col,
        })
    }

    fn parse_equation(&mut self) -> Result<Equation, ParseError> {
        let (lhs, line, col) = self.expect_ident()?;
        self.expect(&TokenKind::Equal, "'='")?;
        let rhs = self.parse_expr()?;
        let original_text = format!("{} = {}", lhs, rhs);

        Ok(Equation {
            lhs,
            rhs,
            original_text,
            line,
            col,
        })
    }

    fn parse_solve(&mut self) -> Result<SolveDirective, ParseError> {
        let start_tok = self.take(); // KwSolve
        let (target, _, _) = self.expect_ident()?;
        self.expect(&TokenKind::KwWith, "'with'")?;
        self.expect(&TokenKind::LeftBrace, "'{'")?;

        let mut bindings = Vec::new();
        while self.peek().kind != TokenKind::RightBrace && self.peek().kind != TokenKind::Eof {
            let (name, b_line, b_col) = self.expect_ident()?;
            self.expect(&TokenKind::Equal, "'='")?;

            let val_tok = self.peek().clone();
            let value = if let TokenKind::Number(v) = val_tok.kind {
                self.take();
                v
            } else if val_tok.kind == TokenKind::Minus {
                self.take();
                let num_tok = self.peek().clone();
                if let TokenKind::Number(v) = num_tok.kind {
                    self.take();
                    -v
                } else {
                    return Err(ParseError {
                        message: format!("expected number after '-', found '{}'", num_tok.text),
                        line: num_tok.line,
                        col: num_tok.col,
                    });
                }
            } else {
                return Err(ParseError {
                    message: format!(
                        "expected number for binding '{}', found '{}'",
                        name, val_tok.text
                    ),
                    line: val_tok.line,
                    col: val_tok.col,
                });
            };

            // Optional unit literal: e.g. "km/h", "m", "s", "eur"
            let unit = self.parse_optional_unit();

            bindings.push(Binding {
                name,
                value,
                unit,
                line: b_line,
                col: b_col,
            });
        }

        self.expect(&TokenKind::RightBrace, "'}'")?;

        Ok(SolveDirective {
            target,
            bindings,
            line: start_tok.line,
            col: start_tok.col,
        })
    }

    fn parse_optional_unit(&mut self) -> Option<String> {
        let mut parts = String::new();
        while matches!(self.peek().kind, TokenKind::Ident(_) | TokenKind::Slash) {
            if matches!(self.peek().kind, TokenKind::Ident(_))
                && self.peek_at(1).kind == TokenKind::Equal
            {
                break;
            }
            let tok = self.take();
            parts.push_str(&tok.text);
        }
        if parts.is_empty() { None } else { Some(parts) }
    }

    fn parse_unit_decl(&mut self) -> Result<UnitDecl, ParseError> {
        let _start_tok = self.take(); // KwUnit
        let (dimension, line, col) = self.expect_ident()?;
        self.expect(&TokenKind::Colon, "':'")?;

        let mut units = Vec::new();
        let (first, _, _) = self.expect_ident()?;
        units.push(first);

        while self.peek().kind == TokenKind::Comma {
            self.take(); // comma
            let (next, _, _) = self.expect_ident()?;
            units.push(next);
        }

        Ok(UnitDecl {
            dimension,
            units,
            line,
            col,
        })
    }

    fn parse_signal(&mut self) -> Result<SignalDecl, ParseError> {
        let _start_tok = self.take(); // KwSignal
        let (name, line, col) = self.expect_ident()?;

        let mut unit = None;
        if self.peek().kind == TokenKind::Colon {
            self.take();
            let (u, _, _) = self.expect_ident()?;
            unit = Some(u);
        }

        Ok(SignalDecl {
            name,
            unit,
            line,
            col,
        })
    }

    // Expression parsing with operator precedence:
    // additive (+ -) < multiplicative (* /) < unary (-) < power (^) < atoms
    pub fn parse_expr(&mut self) -> Result<Expr, ParseError> {
        self.parse_additive()
    }

    fn parse_additive(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_multiplicative()?;
        while matches!(self.peek().kind, TokenKind::Plus | TokenKind::Minus) {
            let op = match self.take().kind {
                TokenKind::Plus => BinaryOp::Add,
                TokenKind::Minus => BinaryOp::Sub,
                _ => unreachable!(),
            };
            let right = self.parse_multiplicative()?;
            left = Expr::Bin(op, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_multiplicative(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_unary()?;
        while matches!(self.peek().kind, TokenKind::Star | TokenKind::Slash) {
            let op = match self.take().kind {
                TokenKind::Star => BinaryOp::Mul,
                TokenKind::Slash => BinaryOp::Div,
                _ => unreachable!(),
            };
            let right = self.parse_unary()?;
            left = Expr::Bin(op, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_unary(&mut self) -> Result<Expr, ParseError> {
        if self.peek().kind == TokenKind::Minus {
            self.take();
            let operand = self.parse_unary()?;
            Ok(Expr::UnaryMinus(Box::new(operand)))
        } else {
            self.parse_power()
        }
    }

    fn parse_power(&mut self) -> Result<Expr, ParseError> {
        let base = self.parse_atom()?;
        if self.peek().kind == TokenKind::Caret {
            self.take();
            // '^' is right-associative: x ^ y ^ z = x ^ (y ^ z)
            let exp = self.parse_unary()?;
            Ok(Expr::Bin(BinaryOp::Pow, Box::new(base), Box::new(exp)))
        } else {
            Ok(base)
        }
    }

    fn parse_atom(&mut self) -> Result<Expr, ParseError> {
        let tok = self.peek().clone();
        match tok.kind {
            TokenKind::Number(val) => {
                self.take();
                Ok(Expr::Num(val))
            }
            TokenKind::Ident(ref name) => {
                let name = name.clone();
                self.take();
                // Check if this is a function call: fn(arg)
                if self.peek().kind == TokenKind::LeftParen {
                    self.take(); // '('
                    let arg = self.parse_expr()?;
                    self.expect(&TokenKind::RightParen, "')'")?;
                    Ok(Expr::Call(name, Box::new(arg)))
                } else {
                    Ok(Expr::Var(name))
                }
            }
            TokenKind::LeftParen => {
                self.take(); // '('
                let inner = self.parse_expr()?;
                self.expect(&TokenKind::RightParen, "')'")?;
                Ok(inner)
            }
            _ => Err(ParseError {
                message: format!("expected expression, found '{}'", tok.text),
                line: tok.line,
                col: tok.col,
            }),
        }
    }
}
