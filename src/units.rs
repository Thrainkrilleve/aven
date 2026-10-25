// src/units.rs — Dimensional analysis, unit algebra & compile-time checking
#![allow(dead_code)]
use std::collections::HashMap;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Dim {
    pub length: i32,
    pub time: i32,
    pub mass: i32,
    pub money: i32,
    pub temperature: i32,
    pub amount: i32,
    pub angle: i32,
    pub data: i32,        // Information (bits, bytes, KiB, MiB, GiB, KB, MB, GB)
    pub transaction: i32, // Discrete transactions (req, iop, pkt)
}

impl Dim {
    pub const SCALAR: Dim = Dim {
        length: 0,
        time: 0,
        mass: 0,
        money: 0,
        temperature: 0,
        amount: 0,
        angle: 0,
        data: 0,
        transaction: 0,
    };
    pub const LENGTH: Dim = Dim {
        length: 1,
        ..Self::SCALAR
    };
    pub const TIME: Dim = Dim {
        time: 1,
        ..Self::SCALAR
    };
    pub const MASS: Dim = Dim {
        mass: 1,
        ..Self::SCALAR
    };
    pub const MONEY: Dim = Dim {
        money: 1,
        ..Self::SCALAR
    };
    pub const TEMPERATURE: Dim = Dim {
        temperature: 1,
        ..Self::SCALAR
    };
    pub const AMOUNT: Dim = Dim {
        amount: 1,
        ..Self::SCALAR
    };
    pub const ANGLE: Dim = Dim {
        angle: 1,
        ..Self::SCALAR
    };
    pub const DATA: Dim = Dim {
        data: 1,
        ..Self::SCALAR
    };
    pub const TRANSACTION: Dim = Dim {
        transaction: 1,
        ..Self::SCALAR
    };

    pub fn is_dimensionless(&self) -> bool {
        self.length == 0
            && self.time == 0
            && self.mass == 0
            && self.money == 0
            && self.temperature == 0
            && self.amount == 0
            && self.angle == 0
            && self.data == 0
            && self.transaction == 0
    }

    pub fn mul(self, rhs: Dim) -> Dim {
        Dim {
            length: self.length + rhs.length,
            time: self.time + rhs.time,
            mass: self.mass + rhs.mass,
            money: self.money + rhs.money,
            temperature: self.temperature + rhs.temperature,
            amount: self.amount + rhs.amount,
            angle: self.angle + rhs.angle,
            data: self.data + rhs.data,
            transaction: self.transaction + rhs.transaction,
        }
    }

    pub fn div(self, rhs: Dim) -> Dim {
        Dim {
            length: self.length - rhs.length,
            time: self.time - rhs.time,
            mass: self.mass - rhs.mass,
            money: self.money - rhs.money,
            temperature: self.temperature - rhs.temperature,
            amount: self.amount - rhs.amount,
            angle: self.angle - rhs.angle,
            data: self.data - rhs.data,
            transaction: self.transaction - rhs.transaction,
        }
    }

    pub fn pow(self, exp: i32) -> Dim {
        Dim {
            length: self.length * exp,
            time: self.time * exp,
            mass: self.mass * exp,
            money: self.money * exp,
            temperature: self.temperature * exp,
            amount: self.amount * exp,
            angle: self.angle * exp,
            data: self.data * exp,
            transaction: self.transaction * exp,
        }
    }
}

impl std::ops::Mul for Dim {
    type Output = Dim;
    fn mul(self, rhs: Dim) -> Dim {
        self.mul(rhs)
    }
}

impl std::ops::Div for Dim {
    type Output = Dim;
    fn div(self, rhs: Dim) -> Dim {
        self.div(rhs)
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
    units: HashMap<String, Unit>,
}

impl Default for UnitRegistry {
    fn default() -> Self {
        let mut reg = Self {
            units: HashMap::new(),
        };

        // Length
        reg.register("m", Dim::LENGTH, 1.0);
        reg.register("km", Dim::LENGTH, 1000.0);
        reg.register("cm", Dim::LENGTH, 0.01);
        reg.register("mm", Dim::LENGTH, 0.001);

        // Time
        reg.register("s", Dim::TIME, 1.0);
        reg.register("ms", Dim::TIME, 0.001);
        reg.register("us", Dim::TIME, 1e-6);
        reg.register("ns", Dim::TIME, 1e-9);
        reg.register("min", Dim::TIME, 60.0);
        reg.register("h", Dim::TIME, 3600.0);
        reg.register("day", Dim::TIME, 86400.0);

        // Mass
        reg.register("kg", Dim::MASS, 1.0);
        reg.register("g", Dim::MASS, 0.001);

        // Money
        reg.register("$", Dim::MONEY, 1.0);
        reg.register("usd", Dim::MONEY, 1.0);
        reg.register("eur", Dim::MONEY, 1.10);
        reg.register("gbp", Dim::MONEY, 1.28);

        // Angle
        reg.register("rad", Dim::ANGLE, 1.0);
        reg.register("deg", Dim::ANGLE, std::f64::consts::PI / 180.0);

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

        // Systems Engineering: Transactions & Operations
        reg.register("req", Dim::TRANSACTION, 1.0);
        reg.register("iop", Dim::TRANSACTION, 1.0);
        reg.register("pkt", Dim::TRANSACTION, 1.0);

        // Network Bandwidth / Line Rates (Data / Time)
        let rate_dim = Dim::DATA / Dim::TIME;
        reg.register("bps", rate_dim, 1.0);
        reg.register("kbps", rate_dim, 1_000.0);
        reg.register("mbps", rate_dim, 1_000_000.0);
        reg.register("gbps", rate_dim, 1_000_000_000.0);
        reg.register("tbps", rate_dim, 1_000_000_000_000.0);

        // Throughput (Transactions / Time)
        let tx_rate_dim = Dim::TRANSACTION / Dim::TIME;
        reg.register("iops", tx_rate_dim, 1.0);

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

    /// Lookup simple or compound units like "km/h" or "m/s"
    pub fn lookup(&self, name: &str) -> Option<Unit> {
        let lower = name.to_lowercase();
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

        None
    }
}
