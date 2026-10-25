// src/ast.rs — AST definitions for Aven
#![allow(dead_code)]
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Pow,
}

impl fmt::Display for BinaryOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let op = match self {
            BinaryOp::Add => "+",
            BinaryOp::Sub => "-",
            BinaryOp::Mul => "*",
            BinaryOp::Div => "/",
            BinaryOp::Pow => "^",
        };
        write!(f, "{}", op)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Num(f64),
    Var(String),
    Bin(BinaryOp, Box<Expr>, Box<Expr>),
    UnaryMinus(Box<Expr>),
    Call(String, Box<Expr>),
}

pub fn is_builtin_constant(name: &str) -> bool {
    matches!(name, "pi" | "PI" | "e" | "E" | "tau" | "TAU")
}

impl Expr {
    pub fn collect_vars(&self, set: &mut std::collections::BTreeSet<String>) {
        match self {
            Expr::Num(_) => {}
            Expr::Var(name) => {
                if !is_builtin_constant(name) {
                    set.insert(name.clone());
                }
            }
            Expr::Bin(_, left, right) => {
                left.collect_vars(set);
                right.collect_vars(set);
            }
            Expr::UnaryMinus(inner) => {
                inner.collect_vars(set);
            }
            Expr::Call(_, arg) => {
                arg.collect_vars(set);
            }
        }
    }

    pub fn vars(&self) -> std::collections::BTreeSet<String> {
        let mut set = std::collections::BTreeSet::new();
        self.collect_vars(&mut set);
        set
    }

    pub fn to_string_prec(&self, parent_prec: u8) -> String {
        match self {
            Expr::Num(n) => {
                if n.fract() == 0.0 && n.abs() < 1e12 {
                    format!("{}", *n as i64)
                } else if n.abs() > 0.0 && (n.abs() < 1e-4 || n.abs() >= 1e12) {
                    format!("{:e}", n)
                } else {
                    let s = format!("{:.6}", n);
                    let trimmed = s.trim_end_matches('0');
                    if trimmed.ends_with('.') {
                        format!("{}0", trimmed)
                    } else {
                        trimmed.to_string()
                    }
                }
            }
            Expr::Var(v) => v.clone(),
            Expr::Call(func, arg) => format!("{}({})", func, arg.to_string_prec(0)),
            Expr::UnaryMinus(inner) => format!("-{}", inner.to_string_prec(3)),
            Expr::Bin(op, left, right) => {
                let (prec, right_assoc) = match op {
                    BinaryOp::Add | BinaryOp::Sub => (1, false),
                    BinaryOp::Mul | BinaryOp::Div => (2, false),
                    BinaryOp::Pow => (4, true),
                };
                let s = format!(
                    "{} {} {}",
                    left.to_string_prec(prec),
                    op,
                    right.to_string_prec(if right_assoc { prec } else { prec + 1 })
                );
                if prec < parent_prec {
                    format!("({})", s)
                } else {
                    s
                }
            }
        }
    }
}

impl fmt::Display for Expr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_string_prec(0))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConstraintOp {
    Equal,
    LessEqual,
    GreaterEqual,
    Less,
    Greater,
}

impl fmt::Display for ConstraintOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            ConstraintOp::Equal => "=",
            ConstraintOp::LessEqual => "<=",
            ConstraintOp::GreaterEqual => ">=",
            ConstraintOp::Less => "<",
            ConstraintOp::Greater => ">",
        };
        write!(f, "{}", s)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectiveKind {
    Minimize,
    Maximize,
}

impl fmt::Display for ObjectiveKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ObjectiveKind::Minimize => write!(f, "minimize"),
            ObjectiveKind::Maximize => write!(f, "maximize"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Objective {
    pub kind: ObjectiveKind,
    pub target: String,
    pub line: usize,
    pub col: usize,
}

#[derive(Debug, Clone)]
pub struct Constraint {
    pub lhs: String,
    pub op: ConstraintOp,
    pub rhs: Expr,
    pub original_text: String,
    pub line: usize,
    pub col: usize,
}

#[derive(Debug, Clone)]
pub struct Equation {
    pub lhs: String,
    pub op: ConstraintOp,
    pub rhs: Expr,
    pub original_text: String,
    pub line: usize,
    pub col: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockKind {
    Relation, // Omnidirectional solver
    Forward,  // Direction-pinned compiler
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum BackendDecl {
    #[default]
    Native,
    External {
        command: String,
        args: Vec<String>,
    },
}

#[derive(Debug, Clone)]
pub struct BlockDecl {
    pub kind: BlockKind,
    pub name: String,
    pub backend: BackendDecl,
    pub equations: Vec<Equation>,
    pub integer_vars: Vec<String>,
    pub line: usize,
    pub col: usize,
}

#[derive(Debug, Clone)]
pub struct Binding {
    pub name: String,
    pub value: f64,
    pub unit: Option<String>,
    pub line: usize,
    pub col: usize,
}

#[derive(Debug, Clone)]
pub struct SolveDirective {
    pub target: String,
    pub bindings: Vec<Binding>,
    pub constraints: Vec<Constraint>,
    pub objectives: Vec<Objective>,
    pub integer_vars: Vec<String>,
    pub line: usize,
    pub col: usize,
}

#[derive(Debug, Clone)]
pub struct UnitDecl {
    pub dimension: String,
    pub units: Vec<String>,
    pub line: usize,
    pub col: usize,
}

#[derive(Debug, Clone)]
pub struct SignalDecl {
    pub name: String,
    pub unit: Option<String>,
    pub line: usize,
    pub col: usize,
}

#[derive(Debug, Clone, Default)]
pub struct AvenModule {
    pub name: String,
    pub blocks: Vec<BlockDecl>,
    pub solves: Vec<SolveDirective>,
    pub units: Vec<UnitDecl>,
    pub signals: Vec<SignalDecl>,
}
