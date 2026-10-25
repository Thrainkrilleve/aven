// src/diff.rs — Symbolic differentiation, evaluation, and algebraic simplification
use crate::ast::*;
use std::collections::HashMap;
use std::fmt;

#[derive(Debug, Clone)]
pub enum DiffError {
    Unsupported(String),
    EvaluationError(String),
}

impl fmt::Display for DiffError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DiffError::Unsupported(msg) => write!(f, "Symbolic differentiation error: {}", msg),
            DiffError::EvaluationError(msg) => write!(f, "Evaluation error: {}", msg),
        }
    }
}

impl std::error::Error for DiffError {}

pub fn lookup_constant(name: &str) -> Option<f64> {
    match name {
        "pi" | "PI" => Some(std::f64::consts::PI),
        "e" | "E" => Some(std::f64::consts::E),
        "tau" | "TAU" => Some(std::f64::consts::TAU),
        _ => None,
    }
}

pub fn eval_expr(expr: &Expr, env: &HashMap<String, f64>) -> Result<f64, DiffError> {
    match expr {
        Expr::Num(n) => Ok(*n),
        Expr::Var(name) => {
            if let Some(&v) = env.get(name) {
                Ok(v)
            } else if let Some(c) = lookup_constant(name) {
                Ok(c)
            } else {
                Err(DiffError::EvaluationError(format!(
                    "unbound variable '{}'",
                    name
                )))
            }
        }
        Expr::UnaryMinus(inner) => Ok(-eval_expr(inner, env)?),
        Expr::Bin(op, left, right) => {
            let l = eval_expr(left, env)?;
            let r = eval_expr(right, env)?;
            let res = match op {
                BinaryOp::Add => l + r,
                BinaryOp::Sub => l - r,
                BinaryOp::Mul => l * r,
                BinaryOp::Div => {
                    if r == 0.0 {
                        return Err(DiffError::EvaluationError("division by zero".to_string()));
                    }
                    l / r
                }
                BinaryOp::Pow => l.powf(r),
            };
            if res.is_nan() || res.is_infinite() {
                return Err(DiffError::EvaluationError(format!(
                    "math domain error in ({} {} {})",
                    l, op, r
                )));
            }
            Ok(res)
        }
        Expr::Call(fn_name, arg) => {
            let val = eval_expr(arg, env)?;
            let res = match fn_name.as_str() {
                "sqrt" => {
                    if val < 0.0 {
                        return Err(DiffError::EvaluationError(format!(
                            "sqrt of negative value {}",
                            val
                        )));
                    }
                    val.sqrt()
                }
                "ceil" => {
                    let rounded = val.round();
                    if (val - rounded).abs() < 1e-9 {
                        rounded
                    } else {
                        val.ceil()
                    }
                }
                "floor" => {
                    let rounded = val.round();
                    if (val - rounded).abs() < 1e-9 {
                        rounded
                    } else {
                        val.floor()
                    }
                }
                "round" => val.round(),
                "abs" => val.abs(),
                "sign" => {
                    if val > 0.0 {
                        1.0
                    } else if val < 0.0 {
                        -1.0
                    } else {
                        0.0
                    }
                }
                "log" => {
                    if val <= 0.0 {
                        return Err(DiffError::EvaluationError(format!(
                            "log of non-positive value {}",
                            val
                        )));
                    }
                    val.ln()
                }
                "exp" => val.exp(),
                "sin" => val.sin(),
                "cos" => val.cos(),
                other => {
                    return Err(DiffError::EvaluationError(format!(
                        "unknown function '{}'",
                        other
                    )));
                }
            };
            Ok(res)
        }
    }
}

pub fn simplify(expr: &Expr) -> Expr {
    match expr {
        Expr::Num(_) | Expr::Var(_) => expr.clone(),
        Expr::UnaryMinus(inner) => {
            let s = simplify(inner);
            if let Expr::Num(n) = s {
                Expr::Num(-n)
            } else if let Expr::UnaryMinus(double_neg) = s {
                *double_neg
            } else {
                Expr::UnaryMinus(Box::new(s))
            }
        }
        Expr::Call(name, arg) => {
            let s = simplify(arg);
            if let Expr::Num(n) = s {
                let folded = match name.as_str() {
                    "sqrt" if n >= 0.0 => Some(n.sqrt()),
                    "ceil" => Some(n.ceil()),
                    "floor" => Some(n.floor()),
                    "round" => Some(n.round()),
                    "abs" => Some(n.abs()),
                    "log" if n > 0.0 => Some(n.ln()),
                    "exp" => Some(n.exp()),
                    "sin" => Some(n.sin()),
                    "cos" => Some(n.cos()),
                    _ => None,
                };
                if let Some(v) = folded {
                    if v.is_finite() {
                        return Expr::Num(v);
                    }
                }
            }
            Expr::Call(name.clone(), Box::new(s))
        }
        Expr::Bin(op, left, right) => {
            let l = simplify(left);
            let r = simplify(right);

            // Constant folding
            if let (Expr::Num(lv), Expr::Num(rv)) = (&l, &r) {
                let val = match op {
                    BinaryOp::Add => Some(lv + rv),
                    BinaryOp::Sub => Some(lv - rv),
                    BinaryOp::Mul => Some(lv * rv),
                    BinaryOp::Div => {
                        if *rv != 0.0 {
                            Some(lv / rv)
                        } else {
                            None
                        }
                    }
                    BinaryOp::Pow => {
                        if *lv == 0.0 && *rv <= 0.0 {
                            None
                        } else {
                            let p = lv.powf(*rv);
                            if p.is_finite() { Some(p) } else { None }
                        }
                    }
                };
                if let Some(v) = val {
                    return Expr::Num(v);
                }
            }

            match op {
                BinaryOp::Mul => {
                    // 0 * x -> 0, x * 0 -> 0
                    if matches!(&l, Expr::Num(n) if *n == 0.0)
                        || matches!(&r, Expr::Num(n) if *n == 0.0)
                    {
                        return Expr::Num(0.0);
                    }
                    // 1 * x -> x
                    if matches!(&l, Expr::Num(n) if *n == 1.0) {
                        return r;
                    }
                    // x * 1 -> x
                    if matches!(&r, Expr::Num(n) if *n == 1.0) {
                        return l;
                    }
                }
                BinaryOp::Add => {
                    // 0 + x -> x
                    if matches!(&l, Expr::Num(n) if *n == 0.0) {
                        return r;
                    }
                    // x + 0 -> x
                    if matches!(&r, Expr::Num(n) if *n == 0.0) {
                        return l;
                    }
                }
                BinaryOp::Sub => {
                    // x - 0 -> x
                    if matches!(&r, Expr::Num(n) if *n == 0.0) {
                        return l;
                    }
                    // 0 - x -> -x
                    if matches!(&l, Expr::Num(n) if *n == 0.0) {
                        return simplify(&Expr::UnaryMinus(Box::new(r)));
                    }
                }
                BinaryOp::Div => {
                    // 0 / x -> 0
                    if matches!(&l, Expr::Num(n) if *n == 0.0) {
                        return Expr::Num(0.0);
                    }
                    // x / 1 -> x
                    if matches!(&r, Expr::Num(n) if *n == 1.0) {
                        return l;
                    }
                }
                BinaryOp::Pow => {
                    // x ^ 0 -> 1
                    if matches!(&r, Expr::Num(n) if *n == 0.0) {
                        return Expr::Num(1.0);
                    }
                    // x ^ 1 -> x
                    if matches!(&r, Expr::Num(n) if *n == 1.0) {
                        return l;
                    }
                }
            }

            Expr::Bin(*op, Box::new(l), Box::new(r))
        }
    }
}

pub fn diff(expr: &Expr, target_var: &str) -> Result<Expr, DiffError> {
    let unsimplified = match expr {
        Expr::Num(_) => Expr::Num(0.0),
        Expr::Var(v) => {
            if is_builtin_constant(v) {
                Expr::Num(0.0)
            } else if v == target_var {
                Expr::Num(1.0)
            } else {
                Expr::Num(0.0)
            }
        }
        Expr::UnaryMinus(inner) => {
            let d_inner = diff(inner, target_var)?;
            Expr::UnaryMinus(Box::new(d_inner))
        }
        Expr::Call(fn_name, arg) => {
            let du = diff(arg, target_var)?;
            match fn_name.as_str() {
                "sqrt" => {
                    // d(sqrt(u))/dx = du / (2 * sqrt(u))
                    let two_sqrt_u = Expr::Bin(
                        BinaryOp::Mul,
                        Box::new(Expr::Num(2.0)),
                        Box::new(Expr::Call("sqrt".to_string(), arg.clone())),
                    );
                    Expr::Bin(BinaryOp::Div, Box::new(du), Box::new(two_sqrt_u))
                }
                "log" => {
                    // d(ln(u))/dx = du / u
                    Expr::Bin(BinaryOp::Div, Box::new(du), arg.clone())
                }
                "exp" => {
                    // d(exp(u))/dx = exp(u) * du
                    Expr::Bin(
                        BinaryOp::Mul,
                        Box::new(Expr::Call("exp".to_string(), arg.clone())),
                        Box::new(du),
                    )
                }
                "sin" => {
                    // d(sin(u))/dx = cos(u) * du
                    Expr::Bin(
                        BinaryOp::Mul,
                        Box::new(Expr::Call("cos".to_string(), arg.clone())),
                        Box::new(du),
                    )
                }
                "cos" => {
                    // d(cos(u))/dx = -sin(u) * du
                    let sin_u_du = Expr::Bin(
                        BinaryOp::Mul,
                        Box::new(Expr::Call("sin".to_string(), arg.clone())),
                        Box::new(du),
                    );
                    Expr::UnaryMinus(Box::new(sin_u_du))
                }
                "ceil" | "floor" | "round" => {
                    // Piecewise constant functions have derivative 0.0 almost everywhere
                    Expr::Num(0.0)
                }
                "abs" => {
                    // Analytical derivative: d/dx |u| = sign(u) * du
                    Expr::Bin(
                        BinaryOp::Mul,
                        Box::new(Expr::Call("sign".to_string(), arg.clone())),
                        Box::new(du),
                    )
                }
                other => {
                    return Err(DiffError::Unsupported(format!(
                        "differentiation of function '{}'",
                        other
                    )));
                }
            }
        }
        Expr::Bin(op, left, right) => {
            let du = diff(left, target_var)?;
            let dv = diff(right, target_var)?;
            match op {
                BinaryOp::Add => Expr::Bin(BinaryOp::Add, Box::new(du), Box::new(dv)),
                BinaryOp::Sub => Expr::Bin(BinaryOp::Sub, Box::new(du), Box::new(dv)),
                BinaryOp::Mul => {
                    // Product rule: du * v + u * dv
                    let term1 = Expr::Bin(BinaryOp::Mul, Box::new(du), right.clone());
                    let term2 = Expr::Bin(BinaryOp::Mul, left.clone(), Box::new(dv));
                    Expr::Bin(BinaryOp::Add, Box::new(term1), Box::new(term2))
                }
                BinaryOp::Div => {
                    // Quotient rule: (du * v - u * dv) / (v ^ 2)
                    let num1 = Expr::Bin(BinaryOp::Mul, Box::new(du), right.clone());
                    let num2 = Expr::Bin(BinaryOp::Mul, left.clone(), Box::new(dv));
                    let numerator = Expr::Bin(BinaryOp::Sub, Box::new(num1), Box::new(num2));
                    let denominator =
                        Expr::Bin(BinaryOp::Pow, right.clone(), Box::new(Expr::Num(2.0)));
                    Expr::Bin(BinaryOp::Div, Box::new(numerator), Box::new(denominator))
                }
                BinaryOp::Pow => {
                    // If exponent is constant: d(u^c) = c * u^(c - 1) * du
                    if matches!(dv, Expr::Num(n) if n == 0.0) {
                        let c_minus_1 =
                            Expr::Bin(BinaryOp::Sub, right.clone(), Box::new(Expr::Num(1.0)));
                        let u_pow = Expr::Bin(BinaryOp::Pow, left.clone(), Box::new(c_minus_1));
                        let c_u_pow = Expr::Bin(BinaryOp::Mul, right.clone(), Box::new(u_pow));
                        Expr::Bin(BinaryOp::Mul, Box::new(c_u_pow), Box::new(du))
                    } else {
                        return Err(DiffError::Unsupported(
                            "variable exponent u^v in symbolic differentiation".to_string(),
                        ));
                    }
                }
            }
        }
    };

    Ok(simplify(&unsimplified))
}
