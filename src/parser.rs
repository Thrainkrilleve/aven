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

struct NamedTwice<'a> {
    name: &'a str,
    line: usize,
    col: usize,
}

fn duplicate_name(equations: &[Equation]) -> Option<NamedTwice<'_>> {
    let mut seen: std::collections::BTreeMap<&str, (usize, usize)> =
        std::collections::BTreeMap::new();
    for eq in equations {
        if seen.contains_key(eq.lhs.as_str()) {
            return Some(NamedTwice {
                name: eq.lhs.as_str(),
                line: eq.line,
                col: eq.col,
            });
        }
        seen.insert(eq.lhs.as_str(), (eq.line, eq.col));
    }
    None
}

/// A name is defined when some other equation gives it a value. A name that
/// appears only on a right-hand side, or only in its own equation, is unknown.
fn undefined_names(equations: &[Equation]) -> Vec<String> {
    let mut unknowns = std::collections::BTreeSet::new();
    for (i, eq) in equations.iter().enumerate() {
        for name in eq.rhs.vars() {
            let defined_elsewhere = equations
                .iter()
                .enumerate()
                .any(|(j, other)| j != i && other.lhs == name);
            if !defined_elsewhere {
                unknowns.insert(name);
            }
        }
    }
    unknowns.into_iter().collect()
}

fn join_and(names: &[String]) -> String {
    match names {
        [] => String::new(),
        [one] => one.clone(),
        [a, b] => format!("{a} and {b}"),
        [head @ .., last] => format!("{}, and {last}", head.join(", ")),
    }
}

fn still_unknown(names: &[String]) -> String {
    let listed = join_and(names);
    let verb = if names.len() == 1 { "is" } else { "are" };
    format!(
        "{listed} {verb} still unknown. A calculation uses only numbers. Name unknowns in a relation and a solve block."
    )
}

fn still_unknown_in_equations(names: &[String]) -> String {
    let listed = join_and(names);
    let verb = if names.len() == 1 { "is" } else { "are" };
    let how = if names.len() == 1 {
        format!("Give {listed} an equation")
    } else {
        "Give each one an equation".to_string()
    };
    format!(
        "{listed} {verb} still unknown. {how}, or name unknowns in a relation and a solve block."
    )
}

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    /// Current expression nesting (parentheses, unary minus, exponents)
    nesting: usize,
}

/// Deepest expression accepted. Deeper input is rejected with a parse error instead of
/// exhausting the stack in later recursive passes (evaluation, differentiation, units).
pub const MAX_EXPR_DEPTH: usize = 1000;

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Self {
            tokens,
            pos: 0,
            nesting: 0,
        }
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

    fn at_declaration(&self) -> bool {
        matches!(
            self.peek().kind,
            TokenKind::KwRelation
                | TokenKind::KwForward
                | TokenKind::KwSolve
                | TokenKind::KwUnit
                | TokenKind::KwSignal
        )
    }

    pub fn parse_module(&mut self, name: &str) -> Result<AvenModule, ParseError> {
        let mut module = AvenModule {
            name: name.to_string(),
            ..Default::default()
        };

        // A file that starts with arithmetic or `name = expression` is one
        // calculation. It becomes a relation the existing solver
        // forward-evaluates. A file that starts with a declaration stays a model.
        if !self.at_declaration() && self.peek().kind != TokenKind::Eof {
            module.blocks.push(self.parse_calculation()?);
            return Ok(module);
        }

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

    fn at_named_equation(&self) -> bool {
        matches!(self.peek().kind, TokenKind::Ident(_))
            && matches!(
                self.peek_at(1).kind,
                TokenKind::Equal | TokenKind::DoubleEqual
            )
    }

    /// A bare expression becomes `answer = expr`. Lines of `name = expr`
    /// become the equations of one relation, so `x = 1 + 10` and
    /// `y = x * 2` settle through the same solver.
    fn parse_calculation(&mut self) -> Result<BlockDecl, ParseError> {
        let start = self.peek().clone();
        let equations = if self.at_named_equation() {
            self.parse_named_equations()?
        } else {
            vec![self.parse_answer_equation()?]
        };
        Ok(BlockDecl {
            kind: BlockKind::Relation,
            name: "Calc".to_string(),
            backend: BackendDecl::Native,
            equations,
            integer_vars: vec![],
            line: start.line,
            col: start.col,
        })
    }

    fn parse_answer_equation(&mut self) -> Result<Equation, ParseError> {
        let start = self.peek().clone();
        let expr = self.parse_expr()?;

        // Reversed form: `<expr> = <name>` means the same as `<name> = <expr>`,
        // so `1 + 2 = x` settles x just like `x = 1 + 2`.
        if matches!(self.peek().kind, TokenKind::Equal | TokenKind::DoubleEqual) {
            self.take(); // '='
            let (name, _line, _col) = self.expect_ident()?;
            if self.peek().kind != TokenKind::Eof {
                let tok = self.peek();
                return Err(ParseError {
                    message: format!(
                        "after '{} = {}', a calculation ends; found '{}'",
                        expr.to_string_prec(0),
                        name,
                        tok.text
                    ),
                    line: tok.line,
                    col: tok.col,
                });
            }
            return Ok(Equation {
                lhs: name.clone(),
                op: ConstraintOp::Equal,
                rhs: expr.clone(),
                original_text: format!("{name} = {}", expr.to_string_prec(0)),
                line: start.line,
                col: start.col,
            });
        }

        if self.peek().kind != TokenKind::Eof {
            let tok = self.peek();
            return Err(ParseError {
                message: format!(
                    "a calculation is one expression; found '{}' after it",
                    tok.text
                ),
                line: tok.line,
                col: tok.col,
            });
        }
        let unknowns: Vec<String> = expr.vars().into_iter().collect();
        if !unknowns.is_empty() {
            return Err(ParseError {
                message: still_unknown(&unknowns),
                line: start.line,
                col: start.col,
            });
        }
        Ok(Equation {
            lhs: "answer".to_string(),
            op: ConstraintOp::Equal,
            rhs: expr.clone(),
            original_text: format!("answer = {}", expr.to_string_prec(0)),
            line: start.line,
            col: start.col,
        })
    }

    fn parse_named_equations(&mut self) -> Result<Vec<Equation>, ParseError> {
        let mut equations = Vec::new();
        loop {
            equations.push(self.parse_named_equation()?);
            if self.peek().kind == TokenKind::Eof {
                break;
            }
            if !self.at_named_equation() {
                let tok = self.peek();
                return Err(ParseError {
                    message: format!(
                        "a calculation is one expression; found '{}' after it",
                        tok.text
                    ),
                    line: tok.line,
                    col: tok.col,
                });
            }
        }
        if let Some(dup) = duplicate_name(&equations) {
            return Err(ParseError {
                message: format!(
                    "{} is named twice. A calculation gives each name one expression.",
                    dup.name
                ),
                line: dup.line,
                col: dup.col,
            });
        }
        let unknowns = undefined_names(&equations);
        if !unknowns.is_empty() {
            let site = equations
                .iter()
                .find(|eq| {
                    eq.rhs
                        .vars()
                        .iter()
                        .any(|v| unknowns.iter().any(|u| u == v))
                })
                .unwrap_or(&equations[0]);
            return Err(ParseError {
                message: still_unknown_in_equations(&unknowns),
                line: site.line,
                col: site.col,
            });
        }
        Ok(equations)
    }

    fn parse_named_equation(&mut self) -> Result<Equation, ParseError> {
        let (lhs, line, col) = self.expect_ident()?;
        let tok = self.peek().clone();
        if matches!(tok.kind, TokenKind::Equal | TokenKind::DoubleEqual) {
            self.take();
        } else {
            return Err(ParseError {
                message: format!("expected '=', found '{}'", tok.text),
                line: tok.line,
                col: tok.col,
            });
        }
        let rhs = self.parse_expr()?;
        Ok(Equation {
            lhs: lhs.clone(),
            op: ConstraintOp::Equal,
            rhs: rhs.clone(),
            original_text: format!("{lhs} = {rhs}"),
            line,
            col,
        })
    }

    fn parse_block(&mut self, kind: BlockKind) -> Result<BlockDecl, ParseError> {
        let start_tok = self.take(); // KwRelation or KwForward
        let (name, _line, _col) = self.expect_ident()?;

        let mut backend = BackendDecl::Native;
        if self.peek().kind == TokenKind::KwUsing {
            self.take(); // consume 'using'
            backend = self.parse_backend_decl()?;
        }

        self.expect(&TokenKind::LeftBrace, "'{'")?;

        let mut equations = Vec::new();
        let mut integer_vars = Vec::new();
        while self.peek().kind != TokenKind::RightBrace && self.peek().kind != TokenKind::Eof {
            if self.peek().kind == TokenKind::KwInt {
                self.take();
                let (ident, _, _) = self.expect_ident()?;
                integer_vars.push(ident);
                continue;
            }
            let eq = self.parse_equation()?;
            equations.push(eq);
        }

        self.expect(&TokenKind::RightBrace, "'}'")?;

        Ok(BlockDecl {
            kind,
            name,
            backend,
            equations,
            integer_vars,
            line: start_tok.line,
            col: start_tok.col,
        })
    }

    fn parse_backend_decl(&mut self) -> Result<BackendDecl, ParseError> {
        let (tok_kind, tok_line, tok_col, tok_text) = {
            let t = self.peek();
            (t.kind.clone(), t.line, t.col, t.text.clone())
        };
        match tok_kind {
            TokenKind::KwExternal => {
                self.take(); // consume 'external'
                self.expect(&TokenKind::LeftParen, "'('")?;
                let mut args = Vec::new();
                while self.peek().kind != TokenKind::RightParen
                    && self.peek().kind != TokenKind::Eof
                {
                    match self.peek().kind.clone() {
                        TokenKind::Str(s) => {
                            args.push(s);
                            self.take();
                        }
                        _ => {
                            let (t_line, t_col, t_text) = {
                                let t = self.peek();
                                (t.line, t.col, t.text.clone())
                            };
                            return Err(ParseError {
                                message: format!(
                                    "expected string literal in external(...) declaration, found '{}'",
                                    t_text
                                ),
                                line: t_line,
                                col: t_col,
                            });
                        }
                    }
                    if self.peek().kind == TokenKind::Comma {
                        self.take();
                    } else if self.peek().kind != TokenKind::RightParen {
                        let (t_line, t_col, t_text) = {
                            let t = self.peek();
                            (t.line, t.col, t.text.clone())
                        };
                        return Err(ParseError {
                            message: format!(
                                "expected ',' or ')' in external(...) declaration, found '{}'",
                                t_text
                            ),
                            line: t_line,
                            col: t_col,
                        });
                    }
                }
                self.expect(&TokenKind::RightParen, "')'")?;

                if args.is_empty() {
                    return Err(ParseError {
                        message: "external(...) backend requires at least a command string literal (e.g. external(\"python\", \"sim.py\"))".to_string(),
                        line: tok_line,
                        col: tok_col,
                    });
                }

                let command = args.remove(0);
                Ok(BackendDecl::External { command, args })
            }
            TokenKind::KwIpc => {
                self.take(); // consume 'ipc'
                self.expect(&TokenKind::LeftParen, "'('")?;
                let endpoint = match self.peek().kind.clone() {
                    TokenKind::Str(s) => {
                        self.take();
                        s
                    }
                    _ => {
                        let (t_line, t_col, t_text) = {
                            let t = self.peek();
                            (t.line, t.col, t.text.clone())
                        };
                        return Err(ParseError {
                            message: format!(
                                "expected string literal address in ipc(...) declaration (e.g. ipc(\"127.0.0.1:9090\")), found '{}'",
                                t_text
                            ),
                            line: t_line,
                            col: t_col,
                        });
                    }
                };
                self.expect(&TokenKind::RightParen, "')'")?;
                Ok(BackendDecl::Ipc { endpoint })
            }
            TokenKind::KwShm => {
                self.take(); // consume 'shm'
                self.expect(&TokenKind::LeftParen, "'('")?;
                let path = match self.peek().kind.clone() {
                    TokenKind::Str(s) => {
                        self.take();
                        s
                    }
                    _ => {
                        let (t_line, t_col, t_text) = {
                            let t = self.peek();
                            (t.line, t.col, t.text.clone())
                        };
                        return Err(ParseError {
                            message: format!(
                                "expected string literal path/name in shm(...) declaration (e.g. shm(\"aven_shm\")), found '{}'",
                                t_text
                            ),
                            line: t_line,
                            col: t_col,
                        });
                    }
                };
                self.expect(&TokenKind::RightParen, "')'")?;
                Ok(BackendDecl::Shm { path })
            }
            TokenKind::KwNative => {
                self.take(); // consume 'native'
                self.expect(&TokenKind::LeftParen, "'('")?;
                let lib_path = match self.peek().kind.clone() {
                    TokenKind::Str(s) => {
                        self.take();
                        s
                    }
                    _ => {
                        let (t_line, t_col, t_text) = {
                            let t = self.peek();
                            (t.line, t.col, t.text.clone())
                        };
                        return Err(ParseError {
                            message: format!(
                                "expected library path string in native(...) declaration, found '{}'",
                                t_text
                            ),
                            line: t_line,
                            col: t_col,
                        });
                    }
                };
                self.expect(&TokenKind::Comma, "','")?;
                let symbol = match self.peek().kind.clone() {
                    TokenKind::Str(s) => {
                        self.take();
                        s
                    }
                    _ => {
                        let (t_line, t_col, t_text) = {
                            let t = self.peek();
                            (t.line, t.col, t.text.clone())
                        };
                        return Err(ParseError {
                            message: format!(
                                "expected symbol name string in native(...) declaration, found '{}'",
                                t_text
                            ),
                            line: t_line,
                            col: t_col,
                        });
                    }
                };
                self.expect(&TokenKind::RightParen, "')'")?;
                Ok(BackendDecl::NativeFfi { lib_path, symbol })
            }
            _ => Err(ParseError {
                message: format!(
                    "expected 'external(...)', 'ipc(...)', 'shm(...)', or 'native(...)', found '{}'",
                    tok_text
                ),
                line: tok_line,
                col: tok_col,
            }),
        }
    }

    fn parse_equation(&mut self) -> Result<Equation, ParseError> {
        let (lhs, line, col) = self.expect_ident()?;
        let op = match self.peek().kind {
            TokenKind::Equal | TokenKind::DoubleEqual => {
                self.take();
                ConstraintOp::Equal
            }
            TokenKind::NotEqual => {
                self.take();
                ConstraintOp::NotEqual
            }
            TokenKind::LessEqual => {
                self.take();
                ConstraintOp::LessEqual
            }
            TokenKind::GreaterEqual => {
                self.take();
                ConstraintOp::GreaterEqual
            }
            TokenKind::Less => {
                self.take();
                ConstraintOp::Less
            }
            TokenKind::Greater => {
                self.take();
                ConstraintOp::Greater
            }
            _ => {
                return Err(ParseError {
                    message: format!(
                        "expected '=', '==', '!=', '<=', '>=', '<', or '>', found '{}'",
                        self.peek().text
                    ),
                    line: self.peek().line,
                    col: self.peek().col,
                });
            }
        };
        let rhs = self.parse_expr()?;
        let original_text = format!("{} {} {}", lhs, op, rhs);

        Ok(Equation {
            lhs,
            op,
            rhs,
            original_text,
            line,
            col,
        })
    }

    fn parse_solve(&mut self) -> Result<SolveDirective, ParseError> {
        let start_tok = self.take(); // KwSolve
        let (first_target, _, _) = self.expect_ident()?;
        let mut targets = vec![first_target.clone()];
        while self.peek().kind == TokenKind::Comma {
            self.take(); // consume comma
            let (next_target, _, _) = self.expect_ident()?;
            targets.push(next_target);
        }
        self.expect(&TokenKind::KwWith, "'with'")?;
        self.expect(&TokenKind::LeftBrace, "'{'")?;

        let mut bindings = Vec::new();
        let mut constraints = Vec::new();
        let mut objectives = Vec::new();
        let mut integer_vars = Vec::new();

        while self.peek().kind != TokenKind::RightBrace && self.peek().kind != TokenKind::Eof {
            // Check for integer variable declarations: int drives or integer drives
            if self.peek().kind == TokenKind::KwInt {
                self.take();
                let (target_var, _, _) = self.expect_ident()?;
                integer_vars.push(target_var);
                continue;
            }

            // Check for objectives: minimize(x) / maximize(x), or minimize x / maximize x
            if self.peek().kind == TokenKind::KwMinimize
                || self.peek().kind == TokenKind::KwMaximize
            {
                let kind = match self.take().kind {
                    TokenKind::KwMinimize => ObjectiveKind::Minimize,
                    TokenKind::KwMaximize => ObjectiveKind::Maximize,
                    _ => unreachable!(),
                };
                let parenthesized = self.peek().kind == TokenKind::LeftParen;
                if parenthesized {
                    self.take();
                }
                let (target_var, o_line, o_col) = self.expect_ident()?;
                if parenthesized {
                    self.expect(&TokenKind::RightParen, "')'")?;
                }
                objectives.push(Objective {
                    kind,
                    target: target_var,
                    line: o_line,
                    col: o_col,
                });
                continue;
            }

            let (name, b_line, b_col) = self.expect_ident()?;

            // Check for equality and inequality constraints: e.g. latency <= 20 ms, report_high_count == 0
            if matches!(
                self.peek().kind,
                TokenKind::DoubleEqual
                    | TokenKind::NotEqual
                    | TokenKind::LessEqual
                    | TokenKind::GreaterEqual
                    | TokenKind::Less
                    | TokenKind::Greater
            ) {
                let op = match self.take().kind {
                    TokenKind::DoubleEqual => ConstraintOp::Equal,
                    TokenKind::NotEqual => ConstraintOp::NotEqual,
                    TokenKind::LessEqual => ConstraintOp::LessEqual,
                    TokenKind::GreaterEqual => ConstraintOp::GreaterEqual,
                    TokenKind::Less => ConstraintOp::Less,
                    TokenKind::Greater => ConstraintOp::Greater,
                    _ => unreachable!(),
                };
                let rhs = self.parse_expr()?;
                let original_text = format!("{} {} {}", name, op, rhs);
                constraints.push(Constraint {
                    lhs: name,
                    op,
                    rhs,
                    original_text,
                    line: b_line,
                    col: b_col,
                });
                continue;
            }

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

            // Optional unit literal on same line: e.g. "km/h", "m", "s", "eur"
            let unit = self.parse_optional_unit(val_tok.line);

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
            target: first_target,
            targets,
            bindings,
            constraints,
            objectives,
            integer_vars,
            line: start_tok.line,
            col: start_tok.col,
        })
    }

    fn parse_optional_unit(&mut self, line: usize) -> Option<String> {
        let mut parts = String::new();
        let mut depth = 0i32;
        loop {
            if self.peek().line != line {
                break;
            }
            // A minus is part of a unit only as the sign of an exponent: s^-1.
            if matches!(self.peek().kind, TokenKind::Minus) && (depth == 0 && !parts.ends_with('^'))
            {
                break;
            }
            if depth == 0
                && matches!(self.peek().kind, TokenKind::Ident(_))
                && matches!(
                    self.peek_at(1).kind,
                    TokenKind::Equal
                        | TokenKind::DoubleEqual
                        | TokenKind::NotEqual
                        | TokenKind::LessEqual
                        | TokenKind::GreaterEqual
                        | TokenKind::Less
                        | TokenKind::Greater
                )
            {
                break;
            }
            match self.peek().kind {
                TokenKind::Ident(_)
                | TokenKind::Slash
                | TokenKind::Star
                | TokenKind::Caret
                | TokenKind::Number(_)
                | TokenKind::Minus
                | TokenKind::LeftParen
                | TokenKind::RightParen => {}
                _ => break,
            }
            if matches!(self.peek().kind, TokenKind::RightParen) && depth == 0 {
                break;
            }
            let tok = self.take();
            match tok.kind {
                TokenKind::LeftParen => depth += 1,
                TokenKind::RightParen => depth -= 1,
                _ => {}
            }
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
            let col_tok = self.take();
            unit = self.parse_optional_unit(col_tok.line);
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
        let top_level = self.nesting == 0;
        let start = self.peek().clone();
        let expr = self.parse_additive()?;
        // Long operator chains are built iteratively, so check the finished tree's depth too
        if top_level && expr.depth() > MAX_EXPR_DEPTH {
            return Err(ParseError {
                message: format!(
                    "expression is nested more than {} levels deep",
                    MAX_EXPR_DEPTH
                ),
                line: start.line,
                col: start.col,
            });
        }
        Ok(expr)
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
        // Every recursive path (unary minus, exponents, parentheses) passes through here
        self.nesting += 1;
        if self.nesting > MAX_EXPR_DEPTH {
            let tok = self.peek().clone();
            return Err(ParseError {
                message: format!(
                    "expression is nested more than {} levels deep",
                    MAX_EXPR_DEPTH
                ),
                line: tok.line,
                col: tok.col,
            });
        }
        let result = if self.peek().kind == TokenKind::Minus {
            self.take();
            self.parse_unary()
                .map(|operand| Expr::UnaryMinus(Box::new(operand)))
        } else {
            self.parse_power()
        };
        self.nesting -= 1;
        result
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
