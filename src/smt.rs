// src/smt.rs — SMT-LIB2 Transpiler & Formal Invariant Verification for Aven
// Milestone Phase 5: SMT-LIB2 (v2.6) emission for Z3 / CVC5 theorem proving.
#![allow(dead_code)]

use crate::ast::*;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::fmt;
use std::fs;
use std::path::Path;
use std::process::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SmtMode {
    #[default]
    Satisfy,
    Prove,
}

#[derive(Debug, Clone, Default)]
pub struct SmtOptions {
    pub mode: SmtMode,
    pub logic: Option<String>,
    pub prove_invariant: Option<String>,
    pub block_name: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SmtError(pub String);

impl fmt::Display for SmtError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SMT error: {}", self.0)
    }
}

impl std::error::Error for SmtError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SmtStatus {
    Sat,
    Unsat,
    Unknown(String),
}

#[derive(Debug, Clone)]
pub struct SmtResult {
    pub status: SmtStatus,
    pub model: HashMap<String, String>,
    pub raw_output: String,
    pub solver_used: String,
}

pub struct SmtTranspiler;

impl SmtTranspiler {
    /// Detects the most appropriate quantifier-free SMT-LIB2 logic for a block.
    pub fn detect_logic(block: &BlockDecl, has_integers: bool) -> &'static str {
        let mut uses_integers = has_integers;
        let mut has_nonlinear = false;
        for eq in &block.equations {
            if expr_uses_integers(&eq.rhs) {
                uses_integers = true;
            }
            if is_nonlinear_expr(&eq.rhs) {
                has_nonlinear = true;
            }
        }

        if uses_integers {
            if has_nonlinear {
                "QF_NIRA"
            } else {
                "QF_LIRA"
            }
        } else if has_nonlinear {
            "QF_NRA"
        } else {
            "QF_LRA"
        }
    }

    /// Formats a float64 into SMT-LIB2 compliant syntax (with decimal point and (- x) for negatives).
    pub fn format_real(val: f64) -> String {
        if val.is_nan() {
            "0.0".to_string()
        } else if val.is_infinite() {
            if val.is_sign_positive() {
                "1e300".to_string()
            } else {
                "(- 1e300)".to_string()
            }
        } else if val < 0.0 {
            format!("(- {})", Self::format_real(-val))
        } else if val.fract() == 0.0 {
            format!("{:.1}", val)
        } else {
            format!("{}", val)
        }
    }

    /// Translates an AST Expr into SMT-LIB2 prefix S-expression notation.
    pub fn expr_to_smt(expr: &Expr, int_vars: &HashSet<String>) -> String {
        match expr {
            Expr::Num(n) => Self::format_real(*n),
            Expr::Var(v) => match v.as_str() {
                "pi" | "PI" => "3.141592653589793".to_string(),
                "e" | "E" => "2.718281828459045".to_string(),
                "tau" | "TAU" => "6.283185307179586".to_string(),
                other => other.to_string(),
            },
            Expr::UnaryMinus(inner) => {
                format!("(- {})", Self::expr_to_smt(inner, int_vars))
            }
            Expr::Bin(op, left, right) => {
                let l = Self::expr_to_smt(left, int_vars);
                let r = Self::expr_to_smt(right, int_vars);
                match op {
                    BinaryOp::Add => format!("(+ {} {})", l, r),
                    BinaryOp::Sub => format!("(- {} {})", l, r),
                    BinaryOp::Mul => format!("(* {} {})", l, r),
                    BinaryOp::Div => format!("(/ {} {})", l, r),
                    BinaryOp::Pow => {
                        // In SMT QF_NRA, square and cube can be expanded directly into multiplication
                        if let Expr::Num(n) = **right {
                            if (n - 2.0).abs() < 1e-9 {
                                return format!("(* {} {})", l, l);
                            } else if (n - 3.0).abs() < 1e-9 {
                                return format!("(* (* {} {}) {})", l, l, l);
                            }
                        }
                        format!("(^ {} {})", l, r)
                    }
                }
            }
            Expr::Call(name, arg) => {
                let a = Self::expr_to_smt(arg, int_vars);
                match name.as_str() {
                    "abs" => format!("(ite (>= {} 0.0) {} (- {}))", a, a, a),
                    "sign" => format!(
                        "(ite (> {} 0.0) 1.0 (ite (< {} 0.0) (- 1.0) 0.0))",
                        a, a
                    ),
                    "floor" => format!("(to_real (to_int {}))", a),
                    "ceil" => {
                        // SMT-LIB encoding of ceil: if a == floor(a) then a else floor(a) + 1.0
                        format!(
                            "(ite (= {} (to_real (to_int {}))) {} (+ (to_real (to_int {})) 1.0))",
                            a, a, a, a
                        )
                    }
                    "round" => format!("(to_real (to_int (+ {} 0.5)))", a),
                    "sqrt" => format!("(^ {} 0.5)", a),
                    "ln" | "log" => format!("(ln {})", a),
                    other => format!("({} {})", other, a),
                }
            }
        }
    }

    /// Formats a constraint relation as an SMT assertion clause.
    pub fn constraint_clause(op: ConstraintOp, lhs: &str, rhs_smt: &str) -> String {
        match op {
            ConstraintOp::Equal => format!("(= {} {})", lhs, rhs_smt),
            ConstraintOp::NotEqual => format!("(not (= {} {}))", lhs, rhs_smt),
            ConstraintOp::LessEqual => format!("(<= {} {})", lhs, rhs_smt),
            ConstraintOp::GreaterEqual => format!("(>= {} {})", lhs, rhs_smt),
            ConstraintOp::Less => format!("(< {} {})", lhs, rhs_smt),
            ConstraintOp::Greater => format!("(> {} {})", lhs, rhs_smt),
        }
    }

    /// Transpiles an Aven block and optional solve directives into SMT-LIB2 format.
    pub fn transpile_block(
        block: &BlockDecl,
        solve: Option<&SolveDirective>,
        options: &SmtOptions,
    ) -> Result<String, SmtError> {
        let mut lines = Vec::new();

        lines.push(format!("; SMT-LIB2 translation for Aven block '{}'", block.name));
        lines.push("; Emitted by avenc v0.6.0 -- SMT Formal Verification Engine".to_string());
        lines.push(format!("; Mode: {:?}", options.mode));
        lines.push("".to_string());

        // Collect integer variables
        let mut int_vars: HashSet<String> = block.integer_vars.iter().cloned().collect();
        if let Some(s) = solve {
            for iv in &s.integer_vars {
                int_vars.insert(iv.clone());
            }
        }

        // Collect all variables
        let mut all_vars = BTreeSet::new();
        for eq in &block.equations {
            all_vars.insert(eq.lhs.clone());
            for v in eq.rhs.vars() {
                all_vars.insert(v);
            }
        }
        if let Some(s) = solve {
            for b in &s.bindings {
                all_vars.insert(b.name.clone());
            }
            for c in &s.constraints {
                all_vars.insert(c.lhs.clone());
                for v in c.rhs.vars() {
                    all_vars.insert(v);
                }
            }
        }

        // Set Logic
        let logic = options.logic.as_deref().unwrap_or_else(|| {
            Self::detect_logic(block, !int_vars.is_empty())
        });
        lines.push(format!("(set-logic {})", logic));
        lines.push("(set-option :produce-models true)".to_string());
        lines.push("".to_string());

        // Declare variables
        lines.push("; Variable declarations".to_string());
        for v in &all_vars {
            if int_vars.contains(v) {
                lines.push(format!("(declare-const {} Int)", v));
            } else {
                lines.push(format!("(declare-const {} Real)", v));
            }
        }
        lines.push("".to_string());

        // Assert block equations
        lines.push("; Physical relations and invariant equations".to_string());
        for eq in &block.equations {
            let rhs_smt = Self::expr_to_smt(&eq.rhs, &int_vars);
            let clause = Self::constraint_clause(eq.op, &eq.lhs, &rhs_smt);
            lines.push(format!("(assert {}) ; {}", clause, eq.original_text));
        }
        lines.push("".to_string());

        // Assert solve bindings (fixed values or observed facts)
        if let Some(s) = solve {
            if !s.bindings.is_empty() {
                lines.push("; Ingress observed bindings".to_string());
                for b in &s.bindings {
                    let val_str = if int_vars.contains(&b.name) {
                        format!("{}", b.value as i64)
                    } else if b.value < 0.0 {
                        format!("(- {})", Self::format_real(-b.value))
                    } else {
                        Self::format_real(b.value)
                    };
                    lines.push(format!("(assert (= {} {}))", b.name, val_str));
                }
                lines.push("".to_string());
            }

            // Invariant constraints
            if !s.constraints.is_empty() {
                match options.mode {
                    SmtMode::Satisfy => {
                        lines.push("; Safety and boundary constraints (Satisfy mode)".to_string());
                        for c in &s.constraints {
                            let rhs_smt = Self::expr_to_smt(&c.rhs, &int_vars);
                            let clause = Self::constraint_clause(c.op, &c.lhs, &rhs_smt);
                            lines.push(format!("(assert {}) ; {}", clause, c.original_text));
                        }
                    }
                    SmtMode::Prove => {
                        lines.push("; Invariant Refutation Assertion (Prove mode: UNSAT => invariant holds unconditionally)".to_string());
                        let mut clauses = Vec::new();
                        for c in &s.constraints {
                            let rhs_smt = Self::expr_to_smt(&c.rhs, &int_vars);
                            clauses.push(Self::constraint_clause(c.op, &c.lhs, &rhs_smt));
                        }
                        if clauses.len() == 1 {
                            lines.push(format!("(assert (not {}))", clauses[0]));
                        } else {
                            lines.push(format!("(assert (not (and {})))", clauses.join(" ")));
                        }
                    }
                }
                lines.push("".to_string());
            }
        }

        // Custom invariant provided via options
        if let Some(inv_str) = &options.prove_invariant {
            lines.push(format!("; User-specified invariant to prove: (assert (not {}))", inv_str));
            lines.push(format!("(assert (not {}))", inv_str));
            lines.push("".to_string());
        }

        // Verification commands
        lines.push("(check-sat)".to_string());
        lines.push("(get-model)".to_string());

        Ok(lines.join("\n"))
    }

    /// Transpiles an entire Aven module to SMT-LIB2.
    pub fn transpile_module(module: &AvenModule, options: &SmtOptions) -> Result<String, SmtError> {
        // Find targeted block or first block
        let target_block = if let Some(ref name) = options.block_name {
            module.blocks.iter().find(|b| &b.name == name)
        } else if let Some(fb) = module.blocks.iter().find(|b| b.kind == BlockKind::Forward) {
            Some(fb)
        } else {
            module.blocks.first()
        };

        let Some(block) = target_block else {
            return Err(SmtError(format!(
                "module '{}' contains no relation or forward blocks to transpile",
                module.name
            )));
        };

        // Find matching solve directive if present
        let solve = module
            .solves
            .iter()
            .find(|s| s.target == block.name || s.targets.contains(&block.name))
            .or_else(|| module.solves.first());

        Self::transpile_block(block, solve, options)
    }

    /// Executes an SMT solver (Z3, CVC5, or python z3-solver) against the given SMT-LIB2 script.
    pub fn run_solver(smt_script: &str, custom_solver: Option<&str>) -> Result<SmtResult, SmtError> {
        let (raw_output, solver_used) = if let Some(cmd) = custom_solver {
            run_cmd_solver(cmd, smt_script)?
        } else if is_cmd_available("z3") {
            run_cmd_solver("z3 -smt2", smt_script)?
        } else if is_cmd_available("cvc5") {
            run_cmd_solver("cvc5 --lang=smt2", smt_script)?
        } else if is_python_z3_available() {
            run_python_z3(smt_script)?
        } else {
            return Err(SmtError(
                "no SMT solver found. Install Z3 / CVC5 or pip install z3-solver in .venv".to_string(),
            ));
        };

        let status = if raw_output.contains("unsat") {
            SmtStatus::Unsat
        } else if raw_output.contains("sat") {
            SmtStatus::Sat
        } else {
            SmtStatus::Unknown(raw_output.trim().to_string())
        };

        let model = parse_smt_model(&raw_output);

        Ok(SmtResult {
            status,
            model,
            raw_output,
            solver_used,
        })
    }
}

fn is_nonlinear_expr(expr: &Expr) -> bool {
    match expr {
        Expr::Num(_) | Expr::Var(_) => false,
        Expr::UnaryMinus(inner) => is_nonlinear_expr(inner),
        Expr::Call(name, inner) => {
            matches!(name.as_str(), "ceil" | "floor" | "round" | "sqrt" | "ln" | "log")
                || is_nonlinear_expr(inner)
        }
        Expr::Bin(BinaryOp::Pow, _, _) => true,
        Expr::Bin(BinaryOp::Div, _, right) => !right.vars().is_empty(),
        Expr::Bin(BinaryOp::Mul, left, right) => {
            !left.vars().is_empty() && !right.vars().is_empty()
        }
        Expr::Bin(_, left, right) => is_nonlinear_expr(left) || is_nonlinear_expr(right),
    }
}

fn expr_uses_integers(expr: &Expr) -> bool {
    match expr {
        Expr::Num(_) | Expr::Var(_) => false,
        Expr::UnaryMinus(inner) => expr_uses_integers(inner),
        Expr::Call(name, inner) => {
            matches!(name.as_str(), "ceil" | "floor" | "round") || expr_uses_integers(inner)
        }
        Expr::Bin(_, left, right) => expr_uses_integers(left) || expr_uses_integers(right),
    }
}

fn is_cmd_available(cmd: &str) -> bool {
    Command::new("where.exe")
        .arg(cmd)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn is_python_z3_available() -> bool {
    let py = get_python_exe();
    Command::new(&py)
        .args(["-c", "import z3"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn get_python_exe() -> String {
    if Path::new(".venv/Scripts/python.exe").exists() {
        ".venv/Scripts/python.exe".to_string()
    } else {
        "python".to_string()
    }
}

fn run_cmd_solver(cmd: &str, smt_script: &str) -> Result<(String, String), SmtError> {
    let parts: Vec<&str> = cmd.split_whitespace().collect();
    if parts.is_empty() {
        return Err(SmtError("empty solver command".to_string()));
    }

    let temp_file = std::env::temp_dir().join(format!("aven_smt_{}.smt2", std::process::id()));
    fs::write(&temp_file, smt_script)
        .map_err(|e| SmtError(format!("failed to write temp smt file: {}", e)))?;

    let mut proc = Command::new(parts[0]);
    for p in &parts[1..] {
        proc.arg(p);
    }
    proc.arg(temp_file.to_str().unwrap());

    let output = proc
        .output()
        .map_err(|e| SmtError(format!("failed to invoke solver '{}': {}", cmd, e)))?;

    let _ = fs::remove_file(&temp_file);

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let out = format!("{}\n{}", stdout, stderr);

    Ok((out, cmd.to_string()))
}

fn run_python_z3(smt_script: &str) -> Result<(String, String), SmtError> {
    let py = get_python_exe();
    let code = r#"
import sys, z3
script = sys.stdin.read()
s = z3.Solver()
try:
    s.from_string(script)
    res = s.check()
    if res == z3.sat:
        print("sat")
        m = s.model()
        for d in m.decls():
            print(f"(define-fun {d.name()} () Real {m[d]})")
    elif res == z3.unsat:
        print("unsat")
    else:
        print("unknown")
except Exception as e:
    print(f"error: {e}")
"#;

    use std::io::Write;
    let mut child = Command::new(&py)
        .args(["-c", code])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| SmtError(format!("failed to invoke python z3: {}", e)))?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(smt_script.as_bytes())
            .map_err(|e| SmtError(format!("failed to write stdin to python z3: {}", e)))?;
    }

    let output = child
        .wait_with_output()
        .map_err(|e| SmtError(format!("python z3 execution failed: {}", e)))?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let combined = format!("{}\n{}", stdout, stderr);

    Ok((combined, format!("{} (z3-solver)", py)))
}

/// Parses variable assignments from SMT `(get-model)` output.
pub fn parse_smt_model(output: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for line in output.lines() {
        let trimmed = line.trim();
        // Patterns like: (define-fun x () Real 6.0) or (define-fun y () Int 10)
        if trimmed.starts_with("(define-fun ") {
            let parts: Vec<&str> = trimmed.split_whitespace().collect();
            if parts.len() >= 5 {
                let name = parts[1].to_string();
                let val_part = parts[4..].join(" ");
                let clean_val = val_part.trim_end_matches(')').trim().to_string();
                map.insert(name, clean_val);
            }
        }
    }
    map
}
