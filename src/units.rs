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
    /// Raises every exponent to the real power `n`, when each result is a whole number
    /// (e.g. m^2 to the 0.5 is m; m to the 0.5 has no dimension and gives None).
    pub fn pow_real(&self, n: f64) -> Option<Dim> {
        let scale = |e: i32| -> Option<i32> {
            let v = e as f64 * n;
            ((v - v.round()).abs() < 1e-9 && v.abs() < 1e6).then(|| v.round() as i32)
        };
        let mut custom = BTreeMap::new();
        for (k, e) in &self.custom {
            let v = scale(*e)?;
            if v != 0 {
                custom.insert(k.clone(), v);
            }
        }
        Some(Dim {
            length: scale(self.length)?,
            time: scale(self.time)?,
            mass: scale(self.mass)?,
            current: scale(self.current)?,
            money: scale(self.money)?,
            temperature: scale(self.temperature)?,
            amount: scale(self.amount)?,
            angle: scale(self.angle)?,
            data: scale(self.data)?,
            transaction: scale(self.transaction)?,
            custom,
        })
    }

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
    /// base value = value * to_base + offset
    pub to_base: f64,
    /// Non-zero only for absolute temperature scales (degC, degF); compound units
    /// built from them (e.g. degC/s) are rates of change and carry no offset.
    pub offset: f64,
}

impl Unit {
    pub fn to_base_value(&self, v: f64) -> f64 {
        v * self.to_base + self.offset
    }

    pub fn to_unit_value(&self, v: f64) -> f64 {
        (v - self.offset) / self.to_base
    }
}

#[derive(Debug, Clone)]
pub struct UnitRegistry {
    /// Units by their exact symbol ("MW" and "mW" are different units)
    pub units: HashMap<String, Unit>,
    /// Lower-cased symbol -> exact symbols, for case-insensitive lookup when unambiguous
    folded: HashMap<String, Vec<String>>,
    /// Lower-cased spellings that are refused outright, with the symbols to write instead
    reserved: HashMap<String, Vec<String>>,
}

impl Default for UnitRegistry {
    fn default() -> Self {
        let mut reg = Self {
            units: HashMap::new(),
            folded: HashMap::new(),
            reserved: HashMap::new(),
        };

        // Length (base: metre)
        reg.register("m", Dim::LENGTH, 1.0);
        reg.register("km", Dim::LENGTH, 1000.0);
        reg.register("cm", Dim::LENGTH, 0.01);
        reg.register("mm", Dim::LENGTH, 0.001);
        reg.register("um", Dim::LENGTH, 1e-6);
        reg.register("in", Dim::LENGTH, 0.0254);
        reg.register("ft", Dim::LENGTH, 0.3048);
        reg.register("mi", Dim::LENGTH, 1609.344);
        reg.register("nmi", Dim::LENGTH, 1852.0);
        // Miles per hour. The same quantity as mi/h. 1 mph = 0.44704 m/s.
        reg.register("mph", Dim::LENGTH / Dim::TIME, 1609.344 / 3600.0);

        // Area. acre is the international acre (exactly 4046.8564224 m^2), not the
        // US survey acre. ha is the hectare, a square 100 m on a side.
        reg.register("acre", Dim::LENGTH.pow(2), 4046.8564224);
        reg.register("ha", Dim::LENGTH.pow(2), 10_000.0);

        // Volume, base m^3. L is the litre (1 dm^3). gal is the US liquid gallon
        // (231 cubic inches) and bu the US bushel (2150.42 cubic inches).
        let litre = 1e-3;
        let cubic_inch = 0.0254_f64.powi(3);
        let volume = Dim::LENGTH.pow(3);
        reg.register("L", volume.clone(), litre);
        reg.register("mL", volume.clone(), litre / 1_000.0);
        reg.register("uL", volume.clone(), litre / 1_000_000.0);
        reg.register("gal", volume.clone(), 231.0 * cubic_inch);
        reg.register("bu", volume, 2150.42 * cubic_inch);

        // Time (base: second)
        reg.register("s", Dim::TIME, 1.0);
        reg.register("ms", Dim::TIME, 0.001);
        reg.register("us", Dim::TIME, 1e-6);
        reg.register("ns", Dim::TIME, 1e-9);
        reg.register("min", Dim::TIME, 60.0);
        reg.register("h", Dim::TIME, 3600.0);
        reg.register("hr", Dim::TIME, 3600.0);
        reg.register("day", Dim::TIME, 86400.0);

        // Mass (base: kilogram)
        reg.register("kg", Dim::MASS, 1.0);
        reg.register("g", Dim::MASS, 0.001);
        reg.register("mg", Dim::MASS, 1e-6);
        reg.register("lb", Dim::MASS, 0.45359237);
        // Avoirdupois ounce, 1/16 pound. A fluid ounce is a volume and is not this unit.
        reg.register("oz", Dim::MASS, 0.45359237 / 16.0);
        // US short ton, 2,000 pounds. The metric tonne is `tonne` (1,000 kg).
        // A lone `t` is not registered: it is too easy to read as time or as tesla.
        reg.register("ton", Dim::MASS, 2_000.0 * 0.45359237);
        reg.register("tonne", Dim::MASS, 1_000.0);

        // Electric current (base: ampere)
        reg.register("A", Dim::CURRENT, 1.0);
        reg.register("mA", Dim::CURRENT, 0.001);

        // Money (base: US dollar). Other currencies get their own dimension: exchange rates
        // change daily, so mixing them must go through an explicit rate variable.
        reg.register("$", Dim::MONEY, 1.0);
        reg.register("USD", Dim::MONEY, 1.0);
        reg.register("EUR", Dim::custom("eur"), 1.0);
        reg.register("GBP", Dim::custom("gbp"), 1.0);

        // Temperature (base: kelvin). degC and degF are absolute temperatures; write
        // temperature differences in K.
        reg.register("K", Dim::TEMPERATURE, 1.0);
        reg.register_affine("degC", Dim::TEMPERATURE, 1.0, 273.15);
        reg.register_affine("degF", Dim::TEMPERATURE, 5.0 / 9.0, 459.67 * 5.0 / 9.0);

        // Amount of substance (base: mole)
        reg.register("mol", Dim::AMOUNT, 1.0);

        // Dimensionless (ratios, coefficients): declares a quantity as a pure
        // scalar so products carry the dimension of the other factors.
        reg.register("scalar", Dim::SCALAR, 1.0);
        reg.register("ratio", Dim::SCALAR, 1.0);
        // Parts of a whole. percent is 0.01, ppm is 10^-6, ppb is 10^-9.
        reg.register("percent", Dim::SCALAR, 0.01);
        reg.register("ppm", Dim::SCALAR, 1e-6);
        reg.register("ppb", Dim::SCALAR, 1e-9);
        // Logarithmic labels. The number is kept as written: 35 dBZ stays 35.
        // Nothing here converts dB to a power ratio or dBZ to linear reflectivity.
        reg.register("dB", Dim::SCALAR, 1.0);
        reg.register("dBm", Dim::SCALAR, 1.0);
        reg.register("dBZ", Dim::SCALAR, 1.0);

        // Angle (base: radian)
        reg.register("rad", Dim::ANGLE, 1.0);
        reg.register("deg", Dim::ANGLE, std::f64::consts::PI / 180.0);

        // Force (N = kg·m/s^2)
        let force_dim = (Dim::MASS * Dim::LENGTH) / (Dim::TIME * Dim::TIME);
        reg.register("N", force_dim.clone(), 1.0);
        reg.register("kN", force_dim, 1000.0);

        // Pressure (Pa = N/m^2)
        let pressure_dim = Dim::MASS / (Dim::LENGTH * Dim::TIME * Dim::TIME);
        reg.register("Pa", pressure_dim.clone(), 1.0);
        reg.register("hPa", pressure_dim.clone(), 100.0);
        reg.register("kPa", pressure_dim.clone(), 1000.0);
        reg.register("MPa", pressure_dim.clone(), 1e6);
        reg.register("bar", pressure_dim.clone(), 1e5);
        reg.register("atm", pressure_dim.clone(), 101_325.0);
        reg.register("psi", pressure_dim, 6894.757293168361);

        // Energy (J = kg·m^2/s^2)
        let energy_dim = (Dim::MASS * Dim::LENGTH * Dim::LENGTH) / (Dim::TIME * Dim::TIME);
        reg.register("J", energy_dim.clone(), 1.0);
        reg.register("kJ", energy_dim.clone(), 1000.0);
        reg.register("MJ", energy_dim.clone(), 1e6);
        reg.register("Wh", energy_dim.clone(), 3600.0);
        reg.register("kWh", energy_dim.clone(), 3.6e6);
        reg.register("MWh", energy_dim, 3.6e9);

        // Power (W = J/s)
        let power_dim =
            (Dim::MASS * Dim::LENGTH * Dim::LENGTH) / (Dim::TIME * Dim::TIME * Dim::TIME);
        reg.register("W", power_dim.clone(), 1.0);
        reg.register("mW", power_dim.clone(), 0.001);
        reg.register("kW", power_dim.clone(), 1000.0);
        reg.register("MW", power_dim.clone(), 1e6);

        // Voltage (V = W/A) and resistance (ohm = V/A)
        let voltage_dim = power_dim / Dim::CURRENT;
        reg.register("V", voltage_dim.clone(), 1.0);
        reg.register("mV", voltage_dim.clone(), 0.001);
        reg.register("kV", voltage_dim.clone(), 1000.0);
        reg.register("ohm", voltage_dim / Dim::CURRENT, 1.0);

        // Frequency (Hz = 1/s)
        let freq_dim = Dim::SCALAR / Dim::TIME;
        reg.register("Hz", freq_dim.clone(), 1.0);
        reg.register("kHz", freq_dim.clone(), 1000.0);
        reg.register("MHz", freq_dim.clone(), 1_000_000.0);
        reg.register("GHz", freq_dim.clone(), 1_000_000_000.0);
        // Tachometer turns per minute: 1 rpm = 1/60 Hz. A turn is a count.
        // Radians per second are a separate multiply by 2*pi.
        reg.register("rpm", freq_dim, 1.0 / 60.0);

        // Data (base: bit). b = bit, B = byte, per IEC 80000-13.
        reg.register("b", Dim::DATA, 1.0);
        reg.register("bit", Dim::DATA, 1.0);
        reg.register("B", Dim::DATA, 8.0);
        reg.register("byte", Dim::DATA, 8.0);
        reg.register("bytes", Dim::DATA, 8.0);
        reg.register("kbit", Dim::DATA, 1e3);
        reg.register("Mbit", Dim::DATA, 1e6);
        reg.register("Gbit", Dim::DATA, 1e9);
        reg.register("Tbit", Dim::DATA, 1e12);
        reg.register("kB", Dim::DATA, 1e3 * 8.0);
        reg.register("KB", Dim::DATA, 1e3 * 8.0);
        reg.register("MB", Dim::DATA, 1e6 * 8.0);
        reg.register("GB", Dim::DATA, 1e9 * 8.0);
        reg.register("TB", Dim::DATA, 1e12 * 8.0);
        reg.register("PB", Dim::DATA, 1e15 * 8.0);
        reg.register("KiB", Dim::DATA, 1024.0 * 8.0);
        reg.register("MiB", Dim::DATA, 1024.0 * 1024.0 * 8.0);
        reg.register("GiB", Dim::DATA, 1024.0 * 1024.0 * 1024.0 * 8.0);
        reg.register("TiB", Dim::DATA, 1024.0 * 1024.0 * 1024.0 * 1024.0 * 8.0);
        // "Mb" is written for both megabits and megabytes, so any spelling other than the
        // exact byte symbol is refused rather than guessed (an 8x error either way).
        for (prefix, bytes, bits) in [
            ("k", "kB", "kbit"),
            ("m", "MB", "Mbit"),
            ("g", "GB", "Gbit"),
            ("t", "TB", "Tbit"),
            ("p", "PB", "PB"),
        ] {
            reg.reserve_spellings(&format!("{}b", prefix), &[bytes, bits]);
        }

        // Transactions & operations
        reg.register("req", Dim::TRANSACTION, 1.0);
        reg.register("iop", Dim::TRANSACTION, 1.0);
        reg.register("pkt", Dim::TRANSACTION, 1.0);
        reg.register("tx", Dim::TRANSACTION, 1.0);

        // Line rates (bits per second)
        let rate_dim = Dim::DATA / Dim::TIME;
        reg.register("bps", rate_dim.clone(), 1.0);
        reg.register("kbps", rate_dim.clone(), 1e3);
        reg.register("Mbps", rate_dim.clone(), 1e6);
        reg.register("Gbps", rate_dim.clone(), 1e9);
        reg.register("Tbps", rate_dim, 1e12);

        // Throughput (transactions per time)
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
        self.register_affine(name, dimension, to_base, 0.0);
    }

    pub fn register_affine(&mut self, name: &str, dimension: Dim, to_base: f64, offset: f64) {
        let folded = self.folded.entry(name.to_lowercase()).or_default();
        if !folded.iter().any(|n| n == name) {
            folded.push(name.to_string());
        }
        self.units.insert(
            name.to_string(),
            Unit {
                name: name.to_string(),
                dimension,
                to_base,
                offset,
            },
        );
    }

    /// Registers the units of a `unit <dimension> : a, b` declaration. The syntax carries no
    /// scale, so a declaration can only add units that are interchangeable with each other:
    /// a custom dimension gets its units at scale 1, and a name that is already a built-in
    /// unit keeps its built-in meaning. A new name under a built-in dimension (e.g.
    /// `unit length : furlong`) gets a dimension of its own rather than silently equalling
    /// the base unit.
    pub fn register_unit_decl(&mut self, decl: &UnitDecl) {
        let builtin = matches!(
            decl.dimension.to_lowercase().as_str(),
            "length"
                | "time"
                | "mass"
                | "current"
                | "money"
                | "currency"
                | "temperature"
                | "amount"
                | "angle"
                | "data"
                | "transaction"
        );
        for u in &decl.units {
            if builtin {
                if self.lookup(u).is_none() {
                    self.register(u, Dim::custom(&u.to_lowercase()), 1.0);
                }
            } else {
                self.register(u, Dim::custom(&decl.dimension), 1.0);
            }
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

        Ok(u_to.to_unit_value(u_from.to_base_value(val)))
    }

    /// Lookup simple or compound units like "km/h", "m/s^2", "req/s", or "kW*h"
    /// Refuses every case variant of `spelling` except exact registered symbols, pointing
    /// users to `instead`.
    pub fn reserve_spellings(&mut self, spelling: &str, instead: &[&str]) {
        let mut c: Vec<String> = instead.iter().map(|s| s.to_string()).collect();
        c.dedup();
        self.reserved.insert(spelling.to_lowercase(), c);
    }

    /// The exact symbols a spelling could mean, when it is ambiguous or reserved
    /// (e.g. "Mb" -> ["MB", "Mbit"], "mw" -> ["MW", "mW"]).
    pub fn ambiguous(&self, name: &str) -> Option<Vec<String>> {
        let name = name.trim();
        if self.units.contains_key(name) {
            return None;
        }
        if let Some(c) = self.reserved.get(&name.to_lowercase()) {
            return Some(c.clone());
        }
        match self.folded.get(&name.to_lowercase()) {
            Some(c) if c.len() > 1 => {
                let mut c = c.clone();
                c.sort();
                Some(c)
            }
            _ => None,
        }
    }

    /// Looks up a unit by exact symbol, then case-insensitively when that is unambiguous
    /// ("gbps" finds Gbps, but "mb" could be Mb or MB and finds nothing).
    ///
    /// Compounds are products and quotients of those symbols. `*` and `/` associate
    /// left to right, `^` binds tighter, and parentheses group. `1` is a dimensionless
    /// numerator, so `1/s` is the same quantity as Hz. `m/s/s` is the same as `m/s^2`.
    pub fn lookup(&self, name: &str) -> Option<Unit> {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            return None;
        }
        let mut i = 0;
        let mut unit = self.parse_product(trimmed, &mut i)?;
        self.skip_ws(trimmed, &mut i);
        if i != trimmed.len() {
            return None;
        }
        if trimmed.contains(['/', '*', '^', '·', '(', ')']) {
            unit.name = trimmed.to_string();
        }
        Some(unit)
    }

    /// One registered symbol, including an unambiguous case fold. No compounds.
    fn lookup_symbol(&self, name: &str) -> Option<Unit> {
        if let Some(u) = self.units.get(name) {
            return Some(u.clone());
        }
        if self.reserved.contains_key(&name.to_lowercase()) {
            return None;
        }
        if let Some([only]) = self.folded.get(&name.to_lowercase()).map(Vec::as_slice) {
            return self.units.get(only).cloned();
        }
        None
    }

    fn skip_ws(&self, s: &str, i: &mut usize) {
        while matches!(s[*i..].chars().next(), Some(' ' | '\t')) {
            *i += 1;
        }
    }

    /// `factor ((*|·|/) factor)*`, left to right.
    fn parse_product(&self, s: &str, i: &mut usize) -> Option<Unit> {
        let mut acc = self.parse_factor(s, i)?;
        loop {
            self.skip_ws(s, i);
            let Some(op) = s[*i..].chars().next() else {
                break;
            };
            if op != '*' && op != '/' && op != '·' {
                break;
            }
            *i += op.len_utf8();
            let rhs = self.parse_factor(s, i)?;
            let (dimension, to_base) = if op == '/' {
                (acc.dimension.div(&rhs.dimension), acc.to_base / rhs.to_base)
            } else {
                (acc.dimension.mul(&rhs.dimension), acc.to_base * rhs.to_base)
            };
            acc = Unit {
                name: String::new(),
                dimension,
                to_base,
                offset: 0.0,
            };
        }
        Some(acc)
    }

    /// `primary (^ integer)?`. An exponent drops any temperature offset.
    fn parse_factor(&self, s: &str, i: &mut usize) -> Option<Unit> {
        let base = self.parse_primary(s, i)?;
        self.skip_ws(s, i);
        if !s[*i..].starts_with('^') {
            return Some(base);
        }
        *i += 1;
        self.skip_ws(s, i);
        let neg = if s[*i..].starts_with('-') {
            *i += 1;
            true
        } else {
            false
        };
        let start = *i;
        while s.as_bytes().get(*i).is_some_and(|b| b.is_ascii_digit()) {
            *i += 1;
        }
        if start == *i {
            return None;
        }
        let mut exp: i32 = s[start..*i].parse().ok()?;
        if neg {
            exp = -exp;
        }
        Some(Unit {
            name: String::new(),
            dimension: base.dimension.pow(exp),
            to_base: base.to_base.powi(exp),
            offset: 0.0,
        })
    }

    /// A symbol, the dimensionless `1`, or a parenthesized product.
    fn parse_primary(&self, s: &str, i: &mut usize) -> Option<Unit> {
        self.skip_ws(s, i);
        if *i >= s.len() {
            return None;
        }
        if s[*i..].starts_with('(') {
            *i += 1;
            let inner = self.parse_product(s, i)?;
            self.skip_ws(s, i);
            if !s[*i..].starts_with(')') {
                return None;
            }
            *i += 1;
            return Some(inner);
        }
        let rest = &s[*i..];
        let token_len = rest
            .bytes()
            .take_while(|b| b.is_ascii_alphanumeric() || *b == b'$' || *b == b'_')
            .count();
        if token_len > 0 {
            for len in (1..=token_len).rev() {
                if let Some(u) = self.lookup_symbol(&rest[..len]) {
                    *i += len;
                    return Some(u);
                }
            }
        }
        // "1" is a dimensionless factor so "1/s" can be written. "1s" is not a unit.
        if rest.starts_with('1') {
            let glued = rest
                .as_bytes()
                .get(1)
                .is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'$' || *b == b'_');
            if !glued {
                *i += 1;
                return Some(Unit {
                    name: "1".to_string(),
                    dimension: Dim::SCALAR,
                    to_base: 1.0,
                    offset: 0.0,
                });
            }
        }
        None
    }
}

/// The unit each variable is read and shown in: its `signal` declaration's unit, or the
/// base unit of its dimension when it has none.
///
/// Values inside the solver are always in coherent base units (SI, plus bit, radian,
/// transaction and US dollar), so equations written with SI constants hold whatever unit an
/// input arrives in. Numbers cross that boundary in the variable's own unit: solve-block
/// bindings, adapter and stream facts, and command-line values on the way in, and printed
/// results on the way out.
#[derive(Debug, Clone, Default)]
pub struct UnitScale {
    declared: HashMap<String, (Unit, String)>,
}

impl UnitScale {
    pub fn for_module(module: &AvenModule) -> Result<Self, TypeError> {
        let registry = module_registry(module);
        let mut declared = HashMap::new();
        for sig in &module.signals {
            if let Some(ref u_str) = sig.unit {
                let unit = registry
                    .lookup(u_str)
                    .ok_or_else(|| unknown_unit(&registry, u_str, sig.line, sig.col))?;
                declared.insert(sig.name.clone(), (unit, u_str.clone()));
            }
        }
        Ok(Self { declared })
    }

    /// Converts a value given in `name`'s declared unit to base units.
    pub fn to_base(&self, name: &str, v: f64) -> f64 {
        match self.declared.get(name) {
            Some((u, _)) => u.to_base_value(v),
            None => v,
        }
    }

    /// Converts a base-unit value to `name`'s declared unit.
    pub fn to_declared(&self, name: &str, v: f64) -> f64 {
        match self.declared.get(name) {
            Some((u, _)) => u.to_unit_value(v),
            None => v,
        }
    }

    /// The unit label for `name` as shown to users: the declared unit as written, else the
    /// base unit of its inferred dimension, else empty (dimensionless or unknown).
    pub fn label(&self, name: &str, inferred: Option<&Dim>) -> String {
        if let Some((_, written)) = self.declared.get(name) {
            return written.clone();
        }
        match inferred {
            Some(d) if !d.is_dimensionless() => derived_symbol(d).unwrap_or_else(|| d.to_string()),
            _ => String::new(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.declared.is_empty()
    }
}

/// The built-in unit registry plus the module's own `unit` declarations.
/// The SI symbol for a coherent derived dimension (N, Pa, J, W, V, ohm), if it is one.
pub fn derived_symbol(d: &Dim) -> Option<String> {
    let m2 = Dim::LENGTH * Dim::LENGTH;
    let s2 = Dim::TIME * Dim::TIME;
    let s3 = s2.clone() * Dim::TIME;
    let named = [
        ("N", (Dim::MASS * Dim::LENGTH) / s2.clone()),
        ("Pa", Dim::MASS / (Dim::LENGTH * s2.clone())),
        ("J", (Dim::MASS * m2.clone()) / s2),
        ("W", (Dim::MASS * m2.clone()) / s3.clone()),
        ("V", (Dim::MASS * m2.clone()) / (s3.clone() * Dim::CURRENT)),
        ("ohm", (Dim::MASS * m2) / (s3 * Dim::CURRENT * Dim::CURRENT)),
    ];
    named
        .into_iter()
        .find(|(_, dim)| dim == d)
        .map(|(sym, _)| sym.to_string())
}

/// The error for a unit the registry cannot resolve, naming the candidates when the
/// spelling only differs from known units by case.
fn unknown_unit(registry: &UnitRegistry, unit: &str, line: usize, col: usize) -> TypeError {
    match registry.ambiguous(unit) {
        Some(candidates) => TypeError::AmbiguousUnit {
            unit: unit.to_string(),
            candidates,
            line,
            col,
        },
        None => TypeError::UnknownUnit {
            unit: unit.to_string(),
            line,
            col,
        },
    }
}

pub fn module_registry(module: &AvenModule) -> UnitRegistry {
    let mut registry = UnitRegistry::new();
    for u in &module.units {
        registry.register_unit_decl(u);
    }
    registry
}

/// Converts every solve-block binding to base units, reading it in the unit written after
/// the number, else in the variable's declared signal unit. Returns the module's scale for
/// converting values that arrive or leave later.
pub fn normalize_bindings(module: &mut AvenModule) -> Result<UnitScale, TypeError> {
    let scale = UnitScale::for_module(module)?;
    let registry = module_registry(module);
    for solve in &mut module.solves {
        for b in &mut solve.bindings {
            b.value = match b.unit {
                Some(ref u_str) => registry
                    .lookup(u_str)
                    .ok_or_else(|| unknown_unit(&registry, u_str, b.line, b.col))?
                    .to_base_value(b.value),
                None => scale.to_base(&b.name, b.value),
            };
        }
    }
    Ok(scale)
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
    AmbiguousUnit {
        unit: String,
        candidates: Vec<String>,
        line: usize,
        col: usize,
    },
}

impl TypeError {
    /// The (line, col) where the error was detected.
    pub fn position(&self) -> (usize, usize) {
        match self {
            TypeError::DimensionalMismatch { line, col, .. }
            | TypeError::InvalidFunctionArgument { line, col, .. }
            | TypeError::UnknownUnit { line, col, .. }
            | TypeError::AmbiguousUnit { line, col, .. } => (*line, *col),
        }
    }
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
            TypeError::AmbiguousUnit {
                unit,
                candidates,
                line,
                col,
            } => write!(
                f,
                "Type Error at line {}, col {}: ambiguous unit '{}' — write one of {} (unit symbols are case-sensitive)",
                line,
                col,
                unit,
                candidates.join(", ")
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
                    return Err(unknown_unit(&self.registry, u_str, sig.line, sig.col));
                }
            }
        }

        // 2. Seed from Solve directive bindings with unit literals
        for solve in &module.solves {
            for b in &solve.bindings {
                if let Some(ref u_str) = b.unit {
                    if let Some(u) = self.registry.lookup(u_str) {
                        if let Some(declared) = var_dims.get(&b.name) {
                            if *declared != u.dimension {
                                return Err(TypeError::DimensionalMismatch {
                                    context: format!(
                                        "binding '{} = {} {}'",
                                        b.name, b.value, u_str
                                    ),
                                    left_dim: declared.clone(),
                                    right_dim: u.dimension,
                                    op: "=".to_string(),
                                    line: b.line,
                                    col: b.col,
                                });
                            }
                        }
                        var_dims.insert(b.name.clone(), u.dimension);
                    } else {
                        return Err(unknown_unit(&self.registry, u_str, b.line, b.col));
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
                let Some(db) = d_base else {
                    return Ok(None);
                };
                if db.is_dimensionless() {
                    return Ok(Some(Dim::SCALAR));
                }
                let constant = match exp.as_ref() {
                    Expr::Num(n) => Some(*n),
                    Expr::UnaryMinus(inner) => match inner.as_ref() {
                        Expr::Num(n) => Some(-*n),
                        _ => None,
                    },
                    _ => None,
                };
                // A dimensioned base needs a constant exponent that keeps every exponent whole
                // (m^2 ^ 0.5 = m); otherwise the result has no well-defined dimension.
                match constant.and_then(|n| db.pow_real(n)) {
                    Some(d) => Ok(Some(d)),
                    None => Err(TypeError::DimensionalMismatch {
                        context: format!("power '({} ^ {})'", base, exp),
                        left_dim: db,
                        right_dim: Dim::SCALAR,
                        op: "^ (exponent must be a constant giving whole-number dimensions)"
                            .to_string(),
                        line,
                        col,
                    }),
                }
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
