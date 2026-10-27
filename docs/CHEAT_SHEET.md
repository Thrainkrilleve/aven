# Aven Quick Reference & Cheat Sheet (CHEAT_SHEET.md)

A rapid-lookup reference card for Aven language syntax, operators, built-in functions, physical units, CLI commands, and exit codes.

---

## 1. Syntax at a Glance

### Omnidirectional Relation
```aven
relation <Name> {
    <equation_1>
    <equation_2>
    [bounds / inequalities]
}
```

### Direction-Pinned Forward Block (Zero-Overhead C/WASM)
```aven
forward <Name> {
    <equation_1>
    <equation_2>
}
```

### Solve Directives & Observations
```aven
solve <RelationName> with {
    <var_1> = <value> [<unit>]
    <var_2> = <value> [<unit>]
    [minimize(<var>) | maximize(<var>)]
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
```aven
using external("python", "adapter.py") { sensor_var }
using ipc("127.0.0.1:9090") { telemetry_var }
using shm("/dev/shm/aven_ring") { high_rate_var }
```

### Namespaced Package Imports
```aven
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

## 3. Built-In Mathematical Functions

All mathematical functions validate inputs against numerical domain faults:

| Function | Description | Valid Domain |
|:---|:---|:---:|
| `sqrt(x)` | Square root | $x \ge 0$ |
| `abs(x)` | Absolute value | $\mathbb{R}$ |
| `sin(x)` | Trigonometric sine | $\mathbb{R}$ (dimensionless) |
| `cos(x)` | Trigonometric cosine | $\mathbb{R}$ (dimensionless) |
| `tan(x)` | Trigonometric tangent | $x \ne \frac{\pi}{2} + k\pi$ |
| `exp(x)` | Natural exponential ($e^x$) | $\mathbb{R}$ (dimensionless) |
| `ln(x)` | Natural logarithm | $x > 0$ |
| `log10(x)` | Base-10 logarithm | $x > 0$ |
| `ceil(x)` | Smallest integer $\ge x$ | $\mathbb{R}$ |
| `floor(x)` | Largest integer $\le x$ | $\mathbb{R}$ |
| `clamp(x, min, max)` | Bounds $x$ into $[\min, \max]$ | $\min \le \max$ |

---

## 4. Standard Physical Units

Aven verifies dimensions across the 7 SI base quantities: $[M, L, T, I, \Theta, N, J]$.

| Symbol | Quantity | SI Base Dimension | Compatible Aliases |
|:---:|:---|:---:|:---|
| `kg` | Mass | $[M]$ | `g`, `mg`, `ton`, `lb` |
| `m` | Length | $[L]$ | `km`, `cm`, `mm`, `nm`, `ft`, `in`, `mile` |
| `s` | Time | $[T]$ | `ms`, `us`, `ns`, `min`, `h`, `day` |
| `A` | Current | $[I]$ | `mA`, `uA` |
| `K` | Temperature | $[\Theta]$ | `celsius` (via affine conversion) |
| `mol` | Substance | $[N]$ | `mmol` |
| `cd` | Luminous Intensity | $[J]$ | `lm` |
| `N` | Force | $[M L T^{-2}]$ | `kN` |
| `Pa` | Pressure | $[M L^{-1} T^{-2}]$ | `kPa`, `bar`, `hpa`, `psi` |
| `J` | Energy / Work | $[M L^2 T^{-2}]$ | `kJ`, `wh`, `kwh` |
| `W` | Power | $[M L^2 T^{-3}]$ | `kW`, `mw`, `hp` |
| `V` | Voltage | $[M L^2 T^{-3} I^{-1}]$ | `mV`, `kv` |
| `ohm` | Resistance | $[M L^2 T^{-3} I^{-2}]$ | `kohm`, `mohm` |
| `km/h` | Speed | $[L T^{-1}]$ | `m/s`, `knots`, `mph` |
| `m/s^2`| Acceleration | $[L T^{-2}]$ | `g_accel` |

---

## 5. CLI Command Reference

```bash
# Syntax and Type Checking
avenc check <file.aven>

# Execute Solver & Emit Fact Ledger
avenc run <file.aven> [--ledger <path.jsonl>]

# Transpile to Standalone ANSI C99
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
avenc debug [--ledger <path.jsonl>]

# Segmented Ledger Audit Compaction
avenc compact-ledger <path.jsonl> [-o <compacted.jsonl>]

# Package Management
avenc pkg init <pkg_name>
avenc pkg pack [--out <file.avenpkg>]
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
| `step` | Advance to the next logical cycle. |
| `prev` | Rewind to the previous logical cycle. |
| `goto <cycle>` | Jump directly to cycle number `<cycle>`. |
| `why <var>` | Render causal provenance DAG showing ancestors of `<var>`. |
| `what-if <var> = <val>` | Branch timeline into a speculative counterfactual universe. |
| `diff` | Display delta between baseline and counterfactual timeline. |
| `help` | List available commands. |
| `quit` | Exit debugger. |
