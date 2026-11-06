# Aven Language Specification

This document provides the formal syntax, grammar, and semantic specification for the **Aven** relational language as implemented by `avenc` v0.6.0.

---

## 1. Lexical Grammar

### 1.1 Identifiers & Keywords
- **Identifiers:** Start with an ASCII letter or underscore `[a-zA-Z_]`, followed by alphanumeric characters or underscores `[a-zA-Z0-9_]*`.
- **Reserved Keywords:**
  `relation`, `forward`, `solve`, `with`, `using`, `external`, `ipc`, `shm`, `native`, `unit`, `signal`, `minimize`, `maximize`, `int`, `integer`. Lines of the form `import <name>;` are resolved before parsing (see [`docs/PACKAGE_SPEC.md`](https://github.com/Thrainkrilleve/aven/blob/main/docs/PACKAGE_SPEC.md)).

### 1.2 Comments
- **Line Comments:** Begin with `//` and extend to the end of the line.
- **Block Comments:** Supported via standard C-style `/* ... */` syntax.

### 1.3 Numeric Literals & Built-in Constants
- **Numbers:** IEEE-754 double-precision floating-point format: `42`, `3.14159`, `1e-4`, `-0.0065`.
- **Built-in Constants:**
  - `pi` / `PI`: $3.141592653589793$
  - `e` / `E`: $2.718281828459045$
  - `tau` / `TAU`: $6.283185307179586$

---

## 2. Expressions and Operators

### 2.1 Arithmetic Operators
Operators follow standard mathematical precedence (highest to lowest):
1. **Exponentiation:** `^` (right-associative)
2. **Unary Negation:** `-`
3. **Multiplication & Division:** `*`, `/` (left-associative)
4. **Addition & Subtraction:** `+`, `-` (left-associative)
5. **Grouping:** Parentheses `( ... )` override default precedence.

### 2.2 Built-in Mathematical Functions
Aven supports unary function calls over expressions:
- `sqrt(expr)`: Principal square root
- `abs(expr)`: Absolute value
- `ceil(expr)`: Smallest integer $\ge \text{expr}$ (values within $10^{-9}$ of an integer snap to it)
- `floor(expr)`: Largest integer $\le \text{expr}$ (same snap)
- `round(expr)`: Nearest integer
- `sign(expr)`: $-1$, $0$ or $+1$
- `sin(expr)`, `cos(expr)`, `tan(expr)`: Standard trigonometric functions (radians)
- `ln(expr)` / `log(expr)`: Natural logarithm (base $e$)
- `exp(expr)`: Exponential function ($e^{\text{expr}}$)

Undefined operations (division by zero, `sqrt` of a negative, `ln` of a non-positive value, `tan` at its poles) and non-finite results are evaluation errors.

---

## 3. Block Declarations

### 3.1 Relational Blocks (`relation`)
A `relation` defines a system of mathematical equations that hold true simultaneously:

```aven
signal velocity       : m/s
signal speed_of_sound : m/s
signal air_density    : kg/m^3

relation CompressibleAero {
    mach = velocity / speed_of_sound
    dynamic_pressure = 0.5 * air_density * (velocity ^ 2)
}
```

### 3.2 Direction-Pinned Blocks (`forward`)
A `forward` block specifies an explicit forward evaluation order without solver relaxation:

```aven
signal total_workers    : scalar
signal cores_per_worker : scalar

forward WorkloadCalculation {
    total_cpu_cores = ceil(total_workers * cores_per_worker)
}
```

### 3.3 Backend Declarations (`using`)
Relations may delegate telemetry ingestion or evaluation to external or persistent backends:

1. **Subprocess External Evaluator:**
   ```aven
   signal max_operating_temp_c : degC
   signal ambient_temp_c       : degC

   relation WeatherMesh using external("python", "evaluations/weather/nws_adapter.py") {
       free_cooling_margin = max_operating_temp_c - ambient_temp_c
   }
   ```
2. **Persistent Socket IPC Daemon:**
   ```aven
   signal baro_altitude_m   : m
   signal gauge_elevation_m : m

   relation FlightMesh using ipc("127.0.0.1:9090") {
       terrain_clearance_m = baro_altitude_m - gauge_elevation_m
   }
   ```
3. **Shared-Memory Ring** (`using shm("name")`) and **In-Process Native Library** (`using native("lib.so")`), for low-latency local sources.

### 3.4 Units (`signal`, `unit`)
```aven
signal velocity : km/h
unit velocity_unit : knots, mach
```
- `signal <name> : <unit>` declares the unit a variable's bare numbers are read and shown in.
- A number in a `solve` block may be followed by a unit: `velocity = 250 km/h`.
- `unit <dimension> : <a>, <b>` declares units of a new dimension, which convert only among themselves.
- Every equation is dimension-checked before anything runs, and values are converted to base units (SI, plus bit, radian, transaction and US dollar) before solving. See [`docs/CHEAT_SHEET.md`](https://github.com/Thrainkrilleve/aven/blob/main/docs/CHEAT_SHEET.md) for the unit table.

---

## 4. Solve Directives & Constraints

### 4.1 Boundary Constraints
The `solve <Target> with { ... }` block specifies observed inputs and safety invariants:

```aven
solve FlightMesh with {
    // Known observed inputs
    baro_altitude_m = 10668.0
    velocity_ms = 235.0

    // Safety boundary constraints
    mach_number >= 0.0
    mach_number <= 1.2
    ambient_temp_c >= -80.0
    ambient_temp_c <= 60.0
    terrain_clearance_m >= -100.0
}
```

Inequalities may also appear inside a `relation` block, where they constrain every solve of it. All `solve` blocks in a file contribute to one settled state.

Without a `minimize` or `maximize` objective, bounds check values; they never choose them. Every variable a bound mentions must be given or derived from the equations, otherwise the system is reported as under-determined. A bound that is broken is a constraint violation (exit code 2 for `avenc run`, `ALARM` for `avenc stream`).

### 4.2 Supported Comparison Operators
- Equality: `=` or `==`
- Inequality: `!=`
- Relational: `<=`, `>=`, `<`, `>`

### 4.3 Integer Variables
To enforce discrete solutions (e.g. server instances, pods, workers), declare integer constraints.
Integer variables represent counts: **without an explicit lower bound they are constrained to be `>= 0`**. Declare a bound (e.g. `offset >= -10`) for an integer that may be negative.
```aven
signal workers : scalar
signal pods    : scalar

relation ClusterAllocation {
    int workers
    int pods
    pods = ceil(workers / 4.0)
}
```

### 4.4 Optimization Directives
Objectives may be written `minimize total_cost_usd` or `minimize(total_cost_usd)`. The first objective is optimized; use `avenc pareto` to explore trade-offs between several.

Linear models are solved exactly (simplex with branch-and-bound) and the result is proven optimal. Nonlinear models are solved locally and then certified by interval branch-and-bound; `avenc run --json` reports `"optimality": "proven"` when the certificate completes (global optimum within a relative gap of 1e-6) and `"local"` when it cannot (for example, an unbounded variable that constraint propagation cannot bound).

Multi-objective optimization targets are declared inside `solve`:
```aven
solve CapacityPlan with {
    minimize total_cost_usd
    maximize throughput_rps
}
```
