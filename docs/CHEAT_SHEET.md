# Aven Quick Reference & Cheat Sheet (CHEAT_SHEET.md)

A rapid-lookup reference card for Aven language syntax, operators, built-in functions, physical units, CLI commands, and exit codes.

---

## 1. Syntax at a Glance

### Omnidirectional Relation
```text
relation <Name> {
    <equation_1>
    <equation_2>
    [bounds / inequalities]
}
```

### Direction-Pinned Forward Block (Solver-Free C/WASM)
```text
forward <Name> {
    <equation_1>
    <equation_2>
}
```

### Solve Directives & Observations
```text
solve <RelationName> with {
    <var_1> = <value> <unit>        // unit optional
    <var_2> = <value>
    <var> >= <expr>                 // bounds, optional
    minimize(<var>)                 // or maximize(<var>), optional
}
```

### Typed Signals & Custom Units
```aven
// Declare a custom unit alias:
unit velocity_unit : knots, mach

// Declare a typed signal:
signal air_speed : km/h
signal altitude  : m
```

### Telemetry Adapters & IPC
A `using` clause attaches a backend to a relation:
```aven
relation Sensors using external("python", "adapter.py") {
    // equations over the adapter's facts
}

relation Telemetry using ipc("127.0.0.1:9090") {
    // equations over the daemon's facts
}
```

### Package Imports
```text
import <package_name>;
```

---

## 2. Operators & Precedence

| Operator | Description | Associativity | Precedence |
|:---:|:---|:---:|:---:|
| `^` | Exponentiation ($x^y$) | Right-to-Left | 1 (Highest) |
| `*`, `/` | Multiplication, Division | Left-to-Right | 2 |
| `+`, `-` | Addition, Subtraction (and Unary `-`) | Left-to-Right | 3 |
| `<`, `<=`, `>`, `>=` | Inequality Bounds | Left-to-Right | 4 |
| `==`, `!=` | Equality Invariants | Left-to-Right | 5 |
| `=` | Relational Binding / Invariant Definition | Non-associative | 6 (Lowest) |

---

## 3. Built-In Mathematical Functions & Constants

All mathematical functions are unary expressions and validate inputs against numerical domain faults:

| Function | Description | Valid Domain |
|:---|:---|:---:|
| `sqrt(x)` | Square root | $x \ge 0$ |
| `abs(x)` | Absolute value | $\mathbb{R}$ |
| `ceil(x)` | Smallest integer $\ge x$ (1e-9 snap) | $\mathbb{R}$ |
| `floor(x)` | Largest integer $\le x$ (1e-9 snap) | $\mathbb{R}$ |
| `round(x)` | Nearest integer rounding | $\mathbb{R}$ |
| `sign(x)` | Signum (-1.0, 0.0, or +1.0) | $\mathbb{R}$ |
| `ln(x)`, `log(x)` | Natural logarithm (base $e$) | $x > 0$ |
| `exp(x)` | Natural exponential ($e^x$) | $\mathbb{R}$ (dimensionless) |
| `sin(x)` | Trigonometric sine | $\mathbb{R}$ (dimensionless) |
| `cos(x)` | Trigonometric cosine | $\mathbb{R}$ (dimensionless) |
| `tan(x)` | Trigonometric tangent | $x \ne \frac{\pi}{2} + k\pi$ |

> [!NOTE]
> *To compute $\log_{10}(x)$:* use `ln(x) / 2.302585092994046`.  
> *To clamp or bound variables:* use inequality bounds (e.g. `x >= min`, `x <= max`) in `relation` or `solve` blocks.

### Built-in Mathematical Constants
- `pi`, `PI` = `3.141592653589793`
- `e`, `E` = `2.718281828459045`
- `tau`, `TAU` = `6.283185307179586`

---

## 4. Units

Aven checks dimensions across ten base dimensions: the SI quantities length, mass, time, current, temperature and amount of substance, plus angle, money, data and transactions. Values are converted to base units (SI, bit, radian, transaction, US dollar) before solving and shown back in each variable's declared unit.

| Dimension | Built-in units |
|:---|:---|
| Length | `m`, `km`, `cm`, `mm`, `um`, `in`, `ft`, `mi`, `nmi` |
| Time | `s`, `ms`, `us`, `ns`, `min`, `h`, `hr`, `day` |
| Mass | `kg`, `g`, `mg`, `lb` |
| Current | `A`, `mA` |
| Temperature | `K`, `degC`, `degF` (absolute temperatures; write differences in `K`) |
| Amount | `mol` |
| Angle | `rad`, `deg` |
| Money | `$`, `USD`. `EUR` and `GBP` are separate dimensions: there are no built-in exchange rates |
| Force | `N`, `kN` |
| Pressure | `Pa`, `hPa`, `kPa`, `MPa`, `bar`, `atm`, `psi` |
| Energy | `J`, `kJ`, `MJ`, `Wh`, `kWh`, `MWh` |
| Power | `W`, `mW`, `kW`, `MW` |
| Electrical | `V`, `mV`, `kV`, `ohm` |
| Frequency | `Hz`, `kHz`, `MHz`, `GHz` |
| Data | `b`/`bit`, `B`/`byte`, `kbit`, `Mbit`, `Gbit`, `Tbit`, `kB`/`KB`, `MB`, `GB`, `TB`, `PB`, `KiB`, `MiB`, `GiB`, `TiB` |
| Transactions | `req`, `iop`, `pkt`, `tx` |
| Rates | `bps`, `kbps`, `Mbps`, `Gbps`, `Tbps`, `iops`, `rps`, `pph` |

- **Compound units** combine these with `/`, `*` and integer `^`: `km/h`, `kg/m^3`, `kW*h`.
- **Case**: symbols are matched exactly first (`MW` is a megawatt, `mW` a milliwatt). Another casing is accepted only when it is unambiguous (`gbps` → `Gbps`). Spellings that could mean bits or bytes (`kb`, `Mb`, `gb`, …) and other ambiguous ones (`mw`) are rejected with the valid choices listed.
- **`unit` declarations** (`unit velocity_unit : knots, mach`) add units of a new dimension that convert only among themselves.

---

## 5. CLI Command Reference

```bash
# Syntax and Type Checking
avenc check <file.aven>

# Execute Solver & Emit Fact Ledger
avenc run <file.aven> [--ledger <path.jsonl>]

# Transpile to Standalone C99
avenc emit-c <file.aven> [-o <path.c>] [--aeric-shim]

# Transpile to WebAssembly (Binary or WAT)
avenc emit-wasm <file.aven> [-o <path.wasm>] [--wat]

# Formal SMT Theorem Verification (Z3 / CVC5)
avenc emit-smt <file.aven> [-o <path.smt2>] [--prove]

# Safety Compliance Assurance Case (DO-178C / ISO 26262)
avenc safety-case <file.aven> [-o <path.md>] [--standard do178c|iso26262]

# High-Resolution Latency & Jitter Benchmark
avenc bench <file.aven> [--latency-profile]

# Interactive Time-Travel Debugger REPL
avenc debug <file.aven> [ledger.jsonl] [--goto <N>] [--what-if "<var>=<val>"] [--diff <t1>,<t2>]

# Segmented Ledger Audit Compaction
avenc compact-ledger <path.jsonl> -o <compacted.jsonl>

# Package Management
avenc pkg init [pkg_name]          # writes Aven.toml and lib.aven in the current directory
avenc pkg pack [dir] [-o <file.avenpkg>]
avenc pkg install <file.avenpkg>
avenc pkg list
```

---

## 6. Process Exit Codes

| Exit Code | Identifier | Meaning |
|:---:|:---|:---|
| **0** | `SUCCESS` | Model settled; all equations and constraints satisfied. |
| **1** | `SOLVE_ERROR` | Unsolvable, singular Jacobian, division by zero, or syntax error. |
| **2** | `CONSTRAINT_VIOLATION` | Converged, but inequality constraint or safety boundary failed. |
| **3** | `STALE_INPUT` | Input telemetry exceeded Time-To-Live (TTL) without valid fallback. |

---

## 7. Interactive Debugger (`avenc debug`) Commands

| Command | Action |
|:---|:---|
| `step` | Advance one logical tick. |
| `prev [n] [--force]` | Rewind one (or `n`) ticks; `--force` crosses a committed effect. |
| `goto <tick> [--force]` | Jump to tick `<tick>`. |
| `vars` | Show every variable at the current tick. |
| `why <var>` | Print the derivation tree of `<var>`. |
| `what-if <var>=<val>` | Branch a counterfactual with `<var>` changed (value in its declared unit) and show the effect on every output. |
| `diff [t1 t2]` | Show what changed between two ticks (default: the previous and current tick). |
| `seal` | Show the ledger's chain head and verify it. |
| `help` | List available commands. |
| `quit` | Exit the debugger. |
