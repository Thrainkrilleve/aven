// src/units.rs — Dimensional analysis, extensible unit algebra & compile-time checking
#![allow(dead_code)]
use crate::ast::*;
use std::collections::{BTreeMap, HashMap};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct Dim {
    pub length: i32,
    pub time: i32,
    pub mass: i32,
    pub current: i32,
    pub money: i32,
    pub temperature: i32,
    pub amount: i32,
    pub angle: i32,
    pub data: i32,        // Information (bits, bytes, KiB, MiB, GiB, KB, MB, GB)
    pub transaction: i32, // Discrete transactions (req, iop, pkt, pph)
    pub custom: BTreeMap<String, i32>,
}

impl Dim {
    pub const fn new_base(
        length: i32,
        time: i32,
        mass: i32,
        current: i32,
        money: i32,
        temperature: i32,
        amount: i32,
        angle: i32,
        data: i32,
        transaction: i32,
    ) -> Self {
        Dim {
            length,
            time,
            mass,
            current,
            money,
            temperature,
            amount,
            angle,
            data,
            transaction,
            custom: BTreeMap::new(),
        }
    }

    pub const SCALAR: Dim = Dim::new_base(0, 0, 0, 0, 0, 0, 0, 0, 0, 0);
    pub const LENGTH: Dim = Dim::new_base(1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
    pub const TIME: Dim = Dim::new_base(0, 1, 0, 0, 0, 0, 0, 0, 0, 0);
    pub const MASS: Dim = Dim::new_base(0, 0, 1, 0, 0, 0, 0, 0, 0, 0);
    pub const CURRENT: Dim = Dim::new_base(0, 0, 0, 1, 0, 0, 0, 0, 0, 0);
    pub const MONEY: Dim = Dim::new_base(0, 0, 0, 0, 1, 0, 0, 0, 0, 0);
    pub const TEMPERATURE: Dim = Dim::new_base(0, 0, 0, 0, 0, 1, 0, 0, 0, 0);
    pub const AMOUNT: Dim = Dim::new_base(0, 0, 0, 0, 0, 0, 1, 0, 0, 0);
    pub const ANGLE: Dim = Dim::new_base(0, 0, 0, 0, 0, 0, 0, 1, 0, 0);
    pub const DATA: Dim = Dim::new_base(0, 0, 0, 0, 0, 0, 0, 0, 1, 0);
    pub const TRANSACTION: Dim = Dim::new_base(0, 0, 0, 0, 0, 0, 0, 0, 0, 1);

    pub fn custom(name: &str) -> Dim {
        let mut custom = BTreeMap::new();
        custom.insert(name.to_lowercase(), 1);
        Dim {
            custom,
            ..Self::SCALAR
        }
    }

    pub fn is_dimensionless(&self) -> bool {
        self.length == 0
            && self.time == 0
            && self.mass == 0
            && self.current == 0
            && self.money == 0
            && self.temperature == 0
            && self.amount == 0
            && self.angle == 0
            && self.data == 0
            && self.transaction == 0
            && self.custom.is_empty()
    }

    pub fn mul(&self, rhs: &Dim) -> Dim {
        let mut custom = self.custom.clone();
        for (k, v) in &rhs.custom {
            *custom.entry(k.clone()).or_insert(0) += v;
        }
        custom.retain(|_, v| *v != 0);

        Dim {
            length: self.length + rhs.length,
            time: self.time + rhs.time,
            mass: self.mass + rhs.mass,
            current: self.current + rhs.current,
            money: self.money + rhs.money,
            temperature: self.temperature + rhs.temperature,
            amount: self.amount + rhs.amount,
            angle: self.angle + rhs.angle,
            data: self.data + rhs.data,
            transaction: self.transaction + rhs.transaction,
            custom,
        }
    }

    pub fn div(&self, rhs: &Dim) -> Dim {
        let mut custom = self.custom.clone();
        for (k, v) in &rhs.custom {
            *custom.entry(k.clone()).or_insert(0) -= v;
        }
        custom.retain(|_, v| *v != 0);

        Dim {
            length: self.length - rhs.length,
            time: self.time - rhs.time,
            mass: self.mass - rhs.mass,
            current: self.current - rhs.current,
            money: self.money - rhs.money,
            temperature: self.temperature - rhs.temperature,
            amount: self.amount - rhs.amount,
            angle: self.angle - rhs.angle,
            data: self.data - rhs.data,
            transaction: self.transaction - rhs.transaction,
            custom,
        }
    }

    pub fn pow(&self, exp: i32) -> Dim {
        let mut custom = BTreeMap::new();
        if exp != 0 {
            for (k, v) in &self.custom {
                custom.insert(k.clone(), v * exp);
            }
        }
        Dim {
            length: self.length * exp,
            time: self.time * exp,
            mass: self.mass * exp,
            current: self.current * exp,
            money: self.money * exp,
            temperature: self.temperature * exp,
            amount: self.amount * exp,
            angle: self.angle * exp,
            data: self.data * exp,
            transaction: self.transaction * exp,
            custom,
        }
    }

    /// Square root of a dimension: halves every exponent. Returns None when any exponent is
    /// odd (e.g. sqrt of a plain length has no physical dimension).
    pub fn sqrt(&self) -> Option<Dim> {
        let half = |e: i32| if e % 2 == 0 { Some(e / 2) } else { None };
        let mut custom = BTreeMap::new();
        for (k, v) in &self.custom {
            custom.insert(k.clone(), half(*v)?);
        }
        Some(Dim {
            length: half(self.length)?,
            time: half(self.time)?,
            mass: half(self.mass)?,
            current: half(self.current)?,
            money: half(self.money)?,
            temperature: half(self.temperature)?,
            amount: half(self.amount)?,
            angle: half(self.angle)?,
            data: half(self.data)?,
            transaction: half(self.transaction)?,
            custom,
        })
    }
}

impl std::ops::Mul for Dim {
    type Output = Dim;
    fn mul(self, rhs: Dim) -> Dim {
        Dim::mul(&self, &rhs)
    }
}

impl std::ops::Mul<&Dim> for Dim {
    type Output = Dim;
    fn mul(self, rhs: &Dim) -> Dim {
        Dim::mul(&self, rhs)
    }
}

impl std::ops::Mul<Dim> for &Dim {
    type Output = Dim;
    fn mul(self, rhs: Dim) -> Dim {
        Dim::mul(self, &rhs)
    }
}

impl std::ops::Mul<&Dim> for &Dim {
    type Output = Dim;
    fn mul(self, rhs: &Dim) -> Dim {
        Dim::mul(self, rhs)
    }
}

impl std::ops::Div for Dim {
    type Output = Dim;
    fn div(self, rhs: Dim) -> Dim {
        Dim::div(&self, &rhs)
    }
}

impl std::ops::Div<&Dim> for Dim {
    type Output = Dim;
    fn div(self, rhs: &Dim) -> Dim {
        Dim::div(&self, rhs)
    }
}

impl std::ops::Div<Dim> for &Dim {
    type Output = Dim;
    fn div(self, rhs: Dim) -> Dim {
        Dim::div(self, &rhs)
    }
}

impl std::ops::Div<&Dim> for &Dim {
    type Output = Dim;
    fn div(self, rhs: &Dim) -> Dim {
        Dim::div(self, rhs)
    }
}

impl fmt::Display for Dim {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_dimensionless() {
            return write!(f, "scalar");
        }
        let dims = [
            ("m", self.length),
            ("s", self.time),
            ("kg", self.mass),
            ("A", self.current),
            ("$", self.money),
            ("K", self.temperature),
            ("mol", self.amount),
            ("rad", self.angle),
            ("b", self.data),
            ("tx", self.transaction),
        ];
        let mut parts = Vec::new();
        for (sym, exp) in dims {
            if exp == 1 {
                parts.push(sym.to_string());
            } else if exp != 0 {
                parts.push(format!("{}^{}", sym, exp));
            }
        }
        for (name, exp) in &self.custom {
            if *exp == 1 {
                parts.push(name.clone());
            } else if *exp != 0 {
                parts.push(format!("{}^{}", name, exp));
            }
        }
        write!(f, "{}", parts.join("·"))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Unit {
    pub name: String,
    pub dimension: Dim,
    pub to_base: f64,
}

#[derive(Debug, Clone)]
pub struct UnitRegistry {
    pub units: HashMap<String, Unit>,
}

impl Default for UnitRegistry {
    fn default() -> Self {
        let mut reg = Self {
            units: HashMap::new(),
        };

        // Length (base: meter)
        reg.register("m", Dim::LENGTH, 1.0);
        reg.register("km", Dim::LENGTH, 1000.0);
        reg.register("cm", Dim::LENGTH, 0.01);
        reg.register("mm", Dim::LENGTH, 0.001);
        reg.register("in", Dim::LENGTH, 0.0254);
        reg.register("ft", Dim::LENGTH, 0.3048);
        reg.register("mi", Dim::LENGTH, 1609.344);

        // Time (base: second)
        reg.register("s", Dim::TIME, 1.0);
        reg.register("ms", Dim::TIME, 0.001);
        reg.register("us", Dim::TIME, 1e-6);
        reg.register("ns", Dim::TIME, 1e-9);
        reg.register("min", Dim::TIME, 60.0);
        reg.register("h", Dim::TIME, 3600.0);
        reg.register("hr", Dim::TIME, 3600.0);
        reg.register("day", Dim::TIME, 86400.0);

        // Mass (base: kg)
        reg.register("kg", Dim::MASS, 1.0);
        reg.register("g", Dim::MASS, 0.001);
        reg.register("mg", Dim::MASS, 1e-6);
        reg.register("lb", Dim::MASS, 0.45359237);

        // Electrical Current (base: Ampere)
        reg.register("a", Dim::CURRENT, 1.0);
        reg.register("ma", Dim::CURRENT, 0.001);

        // Money / Financial (base: $)
        reg.register("$", Dim::MONEY, 1.0);
        reg.register("usd", Dim::MONEY, 1.0);
        reg.register("eur", Dim::MONEY, 1.10);
        reg.register("gbp", Dim::MONEY, 1.28);

        // Temperature (base: Kelvin)
        reg.register("k", Dim::TEMPERATURE, 1.0);
        reg.register("degc", Dim::TEMPERATURE, 1.0);

        // Amount of Substance (base: mole)
        reg.register("mol", Dim::AMOUNT, 1.0);

        // Angle (base: radian)
        reg.register("rad", Dim::ANGLE, 1.0);
        reg.register("deg", Dim::ANGLE, std::f64::consts::PI / 180.0);

        // Power (W = kg*m^2/s^3)
        let power_dim =
            (Dim::MASS * Dim::LENGTH * Dim::LENGTH) / (Dim::TIME * Dim::TIME * Dim::TIME);
        reg.register("w", power_dim.clone(), 1.0);
        reg.register("kw", power_dim.clone(), 1000.0);
        reg.register("mw", power_dim, 0.001);

        // Energy (J = W*s)
        let energy_dim = (Dim::MASS * Dim::LENGTH * Dim::LENGTH) / (Dim::TIME * Dim::TIME);
        reg.register("j", energy_dim.clone(), 1.0);
        reg.register("kj", energy_dim.clone(), 1000.0);
        reg.register("kwh", energy_dim, 3600000.0);

        // Frequency (Hz = 1/s)
        let freq_dim = Dim::SCALAR / Dim::TIME;
        reg.register("hz", freq_dim.clone(), 1.0);
        reg.register("khz", freq_dim.clone(), 1000.0);
        reg.register("mhz", freq_dim.clone(), 1_000_000.0);
        reg.register("ghz", freq_dim, 1_000_000_000.0);

        // Systems Engineering: Data & Information (base: bit = 1.0)
        reg.register("b", Dim::DATA, 1.0);
        reg.register("bit", Dim::DATA, 1.0);
        reg.register("byte", Dim::DATA, 8.0);
        reg.register("bytes", Dim::DATA, 8.0);
        // Binary prefixes (IEC 60027-2)
        reg.register("kib", Dim::DATA, 1024.0 * 8.0);
        reg.register("mib", Dim::DATA, 1024.0 * 1024.0 * 8.0);
        reg.register("gib", Dim::DATA, 1024.0 * 1024.0 * 1024.0 * 8.0);
        reg.register("tib", Dim::DATA, 1024.0 * 1024.0 * 1024.0 * 1024.0 * 8.0);
        // Decimal SI prefixes
        reg.register("kb", Dim::DATA, 1000.0 * 8.0);
        reg.register("mb", Dim::DATA, 1000.0 * 1000.0 * 8.0);
        reg.register("gb", Dim::DATA, 1000.0 * 1000.0 * 1000.0 * 8.0);
        reg.register("tb", Dim::DATA, 1000.0 * 1000.0 * 1000.0 * 1000.0 * 8.0);
        reg.register("pb", Dim::DATA, 1e15 * 8.0);

        // Systems Engineering: Transactions & Operations
        reg.register("req", Dim::TRANSACTION, 1.0);
        reg.register("iop", Dim::TRANSACTION, 1.0);
        reg.register("pkt", Dim::TRANSACTION, 1.0);
        reg.register("tx", Dim::TRANSACTION, 1.0);

        // Network Bandwidth / Line Rates (Data / Time)
        let rate_dim = Dim::DATA / Dim::TIME;
        reg.register("bps", rate_dim.clone(), 1.0);
        reg.register("kbps", rate_dim.clone(), 1_000.0);
        reg.register("mbps", rate_dim.clone(), 1_000_000.0);
        reg.register("gbps", rate_dim.clone(), 1_000_000_000.0);
        reg.register("tbps", rate_dim, 1_000_000_000_000.0);

        // Throughput (Transactions / Time)
        let tx_rate_dim = Dim::TRANSACTION / Dim::TIME;
        reg.register("iops", tx_rate_dim.clone(), 1.0);
        reg.register("rps", tx_rate_dim.clone(), 1.0);
        reg.register("pph", tx_rate_dim, 1.0 / 3600.0); // parts per hour

        reg
    }
}

impl UnitRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, name: &str, dimension: Dim, to_base: f64) {
        self.units.insert(
            name.to_lowercase(),
            Unit {
                name: name.to_string(),
                dimension,
                to_base,
            },
        );
    }

    pub fn register_unit_decl(&mut self, decl: &UnitDecl) {
        let dim = match decl.dimension.to_lowercase().as_str() {
            "length" => Dim::LENGTH,
            "time" => Dim::TIME,
            "mass" => Dim::MASS,
            "current" => Dim::CURRENT,
            "money" | "currency" => Dim::MONEY,
            "temperature" => Dim::TEMPERATURE,
            "amount" => Dim::AMOUNT,
            "angle" => Dim::ANGLE,
            "data" => Dim::DATA,
            "transaction" => Dim::TRANSACTION,
            custom_name => Dim::custom(custom_name),
        };
        for u in &decl.units {
            self.register(u, dim.clone(), 1.0);
        }
    }

    /// Convert a value between compatible units
    pub fn convert(&self, val: f64, from_unit: &str, to_unit: &str) -> Result<f64, String> {
        let u_from = self
            .lookup(from_unit)
            .ok_or_else(|| format!("unknown unit '{}'", from_unit))?;
        let u_to = self
            .lookup(to_unit)
            .ok_or_else(|| format!("unknown unit '{}'", to_unit))?;

        if u_from.dimension != u_to.dimension {
            return Err(format!(
                "cannot convert '{}' [{}] to '{}' [{}] (incompatible dimensions)",
                from_unit, u_from.dimension, to_unit, u_to.dimension
            ));
        }

        let base_val = val * u_from.to_base;
        let target_val = base_val / u_to.to_base;
        Ok(target_val)
    }

    /// Lookup simple or compound units like "km/h", "m/s^2", "req/s", or "kW*h"
    pub fn lookup(&self, name: &str) -> Option<Unit> {
        let lower = name.to_lowercase().trim().to_string();
        if let Some(u) = self.units.get(&lower) {
            return Some(u.clone());
        }

        // Parse compound division: num / denom
        if lower.contains('/') {
            let parts: Vec<&str> = lower.split('/').collect();
            if parts.len() == 2 {
                let num = self.lookup(parts[0])?;
                let den = self.lookup(parts[1])?;
                return Some(Unit {
                    name: name.to_string(),
                    dimension: num.dimension / den.dimension,
                    to_base: num.to_base / den.to_base,
                });
            }
        }

        // Parse compound multiplication: u1 * u2 or u1·u2
        if lower.contains('*') || lower.contains('·') {
            let sep = if lower.contains('*') { '*' } else { '·' };
            let parts: Vec<&str> = lower.split(sep).collect();
            if parts.len() == 2 {
                let u1 = self.lookup(parts[0])?;
                let u2 = self.lookup(parts[1])?;
                return Some(Unit {
                    name: name.to_string(),
                    dimension: u1.dimension * u2.dimension,
                    to_base: u1.to_base * u2.to_base,
                });
            }
        }

        // Parse exponents: e.g. s^2 or m^3
        if lower.contains('^') {
            let parts: Vec<&str> = lower.split('^').collect();
            if parts.len() == 2 {
                let base_u = self.lookup(parts[0])?;
                let exp = parts[1].parse::<i32>().ok()?;
                return Some(Unit {
                    name: name.to_string(),
                    dimension: base_u.dimension.pow(exp),
                    to_base: base_u.to_base.powi(exp),
                });
            }
        }

        None
    }
}

#[derive(Debug, Clone)]
pub enum TypeError {
    DimensionalMismatch {
        context: String,
        left_dim: Dim,
        right_dim: Dim,
        op: String,
        line: usize,
        col: usize,
    },
    InvalidFunctionArgument {
        func: String,
        expected_dim: String,
        actual_dim: Dim,
        line: usize,
        col: usize,
    },
    UnknownUnit {
        unit: String,
        line: usize,
        col: usize,
    },
}

impl fmt::Display for TypeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TypeError::DimensionalMismatch {
                context,
                left_dim,
                right_dim,
                op,
                line,
                col,
            } => write!(
                f,
                "Type Error at line {}, col {}: dimensional mismatch in {} — cannot perform '{}' between [{}] and [{}]",
                line, col, context, op, left_dim, right_dim
            ),
            TypeError::InvalidFunctionArgument {
                func,
                expected_dim,
                actual_dim,
                line,
                col,
            } => write!(
                f,
                "Type Error at line {}, col {}: invalid argument for function '{}' — expected {}, got [{}]",
                line, col, func, expected_dim, actual_dim
            ),
            TypeError::UnknownUnit { unit, line, col } => write!(
                f,
                "Type Error at line {}, col {}: unrecognized unit '{}'",
                line, col, unit
            ),
        }
    }
}

impl std::error::Error for TypeError {}

#[derive(Debug, Clone, Default)]
pub struct TypeReport {
    pub variable_dimensions: HashMap<String, Dim>,
    pub verified_equations: usize,
}

pub struct DimensionalChecker {
    pub registry: UnitRegistry,
}

impl Default for DimensionalChecker {
    fn default() -> Self {
        Self::new()
    }
}

impl DimensionalChecker {
    pub fn new() -> Self {
        Self {
            registry: UnitRegistry::new(),
        }
    }

    pub fn with_registry(registry: UnitRegistry) -> Self {
        Self { registry }
    }

    /// Check module for dimensional consistency and propagate dimensions across relation graphs
    pub fn check_module(&mut self, module: &AvenModule) -> Result<TypeReport, TypeError> {
        // Register any module-defined unit declarations
        for u in &module.units {
            self.registry.register_unit_decl(u);
        }

        let mut var_dims: HashMap<String, Dim> = HashMap::new();

        // 1. Seed from Signal declarations
        for sig in &module.signals {
            if let Some(ref u_str) = sig.unit {
                if let Some(u) = self.registry.lookup(u_str) {
                    var_dims.insert(sig.name.clone(), u.dimension);
                } else {
                    return Err(TypeError::UnknownUnit {
                        unit: u_str.clone(),
                        line: sig.line,
                        col: sig.col,
                    });
                }
            }
        }

        // 2. Seed from Solve directive bindings with unit literals
        for solve in &module.solves {
            for b in &solve.bindings {
                if let Some(ref u_str) = b.unit {
                    if let Some(u) = self.registry.lookup(u_str) {
                        var_dims.insert(b.name.clone(), u.dimension);
                    } else {
                        return Err(TypeError::UnknownUnit {
                            unit: u_str.clone(),
                            line: b.line,
                            col: b.col,
                        });
                    }
                }
            }
        }

        let mut total_equations = 0;

        // 3. Propagate dimensions across all blocks
        for blk in &module.blocks {
            total_equations += blk.equations.len();
            for _round in 0..20 {
                let mut progressed = false;
                for eq in &blk.equations {
                    let inferred_rhs = self.infer_expr(&eq.rhs, &var_dims, eq.line, eq.col)?;
                    if let Some(ref d_rhs) = inferred_rhs {
                        if let Some(d_lhs) = var_dims.get(&eq.lhs) {
                            if d_lhs != d_rhs {
                                return Err(TypeError::DimensionalMismatch {
                                    context: format!("equation '{}'", eq.original_text),
                                    left_dim: d_lhs.clone(),
                                    right_dim: d_rhs.clone(),
                                    op: format!("{}", eq.op),
                                    line: eq.line,
                                    col: eq.col,
                                });
                            }
                        } else {
                            var_dims.insert(eq.lhs.clone(), d_rhs.clone());
                            progressed = true;
                        }
                    } else if let Some(d_lhs) = var_dims.get(&eq.lhs).cloned() {
                        // Omnidirectional reverse propagation: lhs is known, deduce unknown in RHS
                        if self.propagate_reverse(
                            &eq.rhs,
                            &d_lhs,
                            &mut var_dims,
                            eq.line,
                            eq.col,
                        )? {
                            progressed = true;
                        }
                    }
                }
                if !progressed {
                    break;
                }
            }
        }

        // 4. Check constraints in solve directives
        for solve in &module.solves {
            for c in &solve.constraints {
                let inferred_rhs = self.infer_expr(&c.rhs, &var_dims, c.line, c.col)?;
                if let Some(ref d_rhs) = inferred_rhs {
                    if let Some(d_lhs) = var_dims.get(&c.lhs) {
                        if d_lhs != d_rhs {
                            return Err(TypeError::DimensionalMismatch {
                                context: format!("constraint '{}'", c.original_text),
                                left_dim: d_lhs.clone(),
                                right_dim: d_rhs.clone(),
                                op: format!("{}", c.op),
                                line: c.line,
                                col: c.col,
                            });
                        }
                    }
                }
            }
        }

        Ok(TypeReport {
            variable_dimensions: var_dims,
            verified_equations: total_equations,
        })
    }

    /// Infer dimension of an expression given known variable dimensions
    pub fn infer_expr(
        &self,
        expr: &Expr,
        var_dims: &HashMap<String, Dim>,
        line: usize,
        col: usize,
    ) -> Result<Option<Dim>, TypeError> {
        match expr {
            Expr::Num(_) => Ok(Some(Dim::SCALAR)),
            Expr::Var(name) => Ok(var_dims.get(name).cloned()),
            Expr::UnaryMinus(inner) => self.infer_expr(inner, var_dims, line, col),
            Expr::Bin(BinaryOp::Add | BinaryOp::Sub, left, right) => {
                let dl = self.infer_expr(left, var_dims, line, col)?;
                let dr = self.infer_expr(right, var_dims, line, col)?;
                match (dl, dr) {
                    (Some(l), Some(r)) => {
                        if l != r {
                            return Err(TypeError::DimensionalMismatch {
                                context: format!("expression '({} + {})'", left, right),
                                left_dim: l,
                                right_dim: r,
                                op: "+/-".to_string(),
                                line,
                                col,
                            });
                        }
                        Ok(Some(l))
                    }
                    (Some(l), None) => Ok(Some(l)),
                    (None, Some(r)) => Ok(Some(r)),
                    (None, None) => Ok(None),
                }
            }
            Expr::Bin(BinaryOp::Mul, left, right) => {
                let dl = self.infer_expr(left, var_dims, line, col)?;
                let dr = self.infer_expr(right, var_dims, line, col)?;
                match (dl, dr) {
                    (Some(l), Some(r)) => Ok(Some(l * r)),
                    _ => Ok(None),
                }
            }
            Expr::Bin(BinaryOp::Div, left, right) => {
                let dl = self.infer_expr(left, var_dims, line, col)?;
                let dr = self.infer_expr(right, var_dims, line, col)?;
                match (dl, dr) {
                    (Some(l), Some(r)) => Ok(Some(l / r)),
                    _ => Ok(None),
                }
            }
            Expr::Bin(BinaryOp::Pow, base, exp) => {
                let d_base = self.infer_expr(base, var_dims, line, col)?;
                let d_exp = self.infer_expr(exp, var_dims, line, col)?;
                if let Some(ref de) = d_exp {
                    if !de.is_dimensionless() {
                        return Err(TypeError::DimensionalMismatch {
                            context: format!("power exponent in '({} ^ {})'", base, exp),
                            left_dim: de.clone(),
                            right_dim: Dim::SCALAR,
                            op: "^".to_string(),
                            line,
                            col,
                        });
                    }
                }
                if let Expr::Num(n) = **exp {
                    if (n.fract() == 0.0) && (n.abs() < 100.0) {
                        if let Some(db) = d_base {
                            return Ok(Some(db.pow(n as i32)));
                        }
                    }
                }
                Ok(d_base)
            }
            Expr::Call(fn_name, inner) => {
                let d_inner = self.infer_expr(inner, var_dims, line, col)?;
                if matches!(fn_name.as_str(), "sin" | "cos" | "tan") {
                    if let Some(ref d) = d_inner {
                        if !d.is_dimensionless() && *d != Dim::ANGLE {
                            return Err(TypeError::InvalidFunctionArgument {
                                func: fn_name.clone(),
                                expected_dim: "scalar or angle (rad/deg)".to_string(),
                                actual_dim: d.clone(),
                                line,
                                col,
                            });
                        }
                    }
                    Ok(Some(Dim::SCALAR))
                } else if fn_name == "sqrt" {
                    match d_inner {
                        Some(ref d) => match d.sqrt() {
                            Some(root) => Ok(Some(root)),
                            None => Err(TypeError::InvalidFunctionArgument {
                                func: fn_name.clone(),
                                expected_dim: "a dimension with even exponents (e.g. m^2)"
                                    .to_string(),
                                actual_dim: d.clone(),
                                line,
                                col,
                            }),
                        },
                        None => Ok(None),
                    }
                } else if matches!(fn_name.as_str(), "exp" | "ln" | "log" | "log10") {
                    if let Some(ref d) = d_inner {
                        if !d.is_dimensionless() {
                            return Err(TypeError::InvalidFunctionArgument {
                                func: fn_name.clone(),
                                expected_dim: "dimensionless scalar".to_string(),
                                actual_dim: d.clone(),
                                line,
                                col,
                            });
                        }
                    }
                    Ok(Some(Dim::SCALAR))
                } else {
                    // abs, ceil, floor, round preserve dimension
                    Ok(d_inner)
                }
            }
        }
    }

    /// Omnidirectional reverse dimensional propagation: lhs = rhs
    fn propagate_reverse(
        &self,
        expr: &Expr,
        target_dim: &Dim,
        var_dims: &mut HashMap<String, Dim>,
        line: usize,
        col: usize,
    ) -> Result<bool, TypeError> {
        match expr {
            Expr::Var(name) => {
                if !var_dims.contains_key(name) {
                    var_dims.insert(name.clone(), target_dim.clone());
                    return Ok(true);
                }
            }
            Expr::Bin(BinaryOp::Mul, left, right) => {
                let dl = self.infer_expr(left, var_dims, line, col)?;
                let dr = self.infer_expr(right, var_dims, line, col)?;
                match (dl, dr) {
                    (Some(l), None) => {
                        let needed_r = target_dim / l;
                        return self.propagate_reverse(right, &needed_r, var_dims, line, col);
                    }
                    (None, Some(r)) => {
                        let needed_l = target_dim / r;
                        return self.propagate_reverse(left, &needed_l, var_dims, line, col);
                    }
                    _ => {}
                }
            }
            Expr::Bin(BinaryOp::Div, left, right) => {
                let dl = self.infer_expr(left, var_dims, line, col)?;
                let dr = self.infer_expr(right, var_dims, line, col)?;
                match (dl, dr) {
                    (Some(l), None) => {
                        // target = l / r => r = l / target
                        let needed_r = l / target_dim;
                        return self.propagate_reverse(right, &needed_r, var_dims, line, col);
                    }
                    (None, Some(r)) => {
                        // target = l / r => l = target * r
                        let needed_l = target_dim * r;
                        return self.propagate_reverse(left, &needed_l, var_dims, line, col);
                    }
                    _ => {}
                }
            }
            Expr::Bin(BinaryOp::Add | BinaryOp::Sub, left, right) => {
                let dl = self.infer_expr(left, var_dims, line, col)?;
                let dr = self.infer_expr(right, var_dims, line, col)?;
                let mut p = false;
                if dl.is_none() {
                    p |= self.propagate_reverse(left, target_dim, var_dims, line, col)?;
                }
                if dr.is_none() {
                    p |= self.propagate_reverse(right, target_dim, var_dims, line, col)?;
                }
                return Ok(p);
            }
            _ => {}
        }
        Ok(false)
    }
}
