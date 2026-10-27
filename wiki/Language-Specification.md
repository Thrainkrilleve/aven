# Aven Language Specification

This document provides the formal syntax, grammar, and semantic specification for the **Aven** relational language (v0.4+).

---

## 1. Lexical Grammar

### 1.1 Identifiers & Keywords
- **Identifiers:** Start with an ASCII letter or underscore `[a-zA-Z_]`, followed by alphanumeric characters or underscores `[a-zA-Z0-9_]*`.
- **Reserved Keywords:**
  `relation`, `forward`, `solve`, `with`, `using`, `external`, `ipc`, `unit`, `signal`, `minimize`, `maximize`, `int`, `integer`.

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
- `ceil(expr)`: Smallest integer $\ge \text{expr}$
- `floor(expr)`: Largest integer $\le \text{expr}$
- `sin(expr)`, `cos(expr)`, `tan(expr)`: Standard trigonometric functions (radians)
- `ln(expr)`: Natural logarithm (base $e$)
- `exp(expr)`: Exponential function ($e^{\text{expr}}$)

---

## 3. Block Declarations

### 3.1 Relational Blocks (`relation`)
A `relation` defines a system of mathematical equations that hold true simultaneously:

```aven
relation CompressibleAero {
    mach = velocity / speed_of_sound
    dynamic_pressure = 0.5 * air_density * (velocity ^ 2)
}
```

### 3.2 Direction-Pinned Blocks (`forward`)
A `forward` block specifies an explicit forward evaluation order without solver relaxation:

```aven
forward WorkloadCalculation {
    total_cpu_cores = ceil(total_workers * cores_per_worker)
}
```

### 3.3 Backend Declarations (`using`)
Relations may delegate telemetry ingestion or evaluation to external or persistent backends:

1. **Subprocess External Evaluator:**
   ```aven
   relation WeatherMesh using external("python", "evaluations/weather/nws_adapter.py") {
       free_cooling_margin = max_operating_temp_c - ambient_temp_c
   }
   ```
2. **Persistent Socket IPC Daemon:**
   ```aven
   relation FlightMesh using ipc("127.0.0.1:9090") {
       terrain_clearance_m = baro_altitude_m - gauge_elevation_m
   }
   ```

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

### 4.2 Supported Comparison Operators
- Equality: `=` or `==`
- Inequality: `!=`
- Relational: `<=`, `>=`, `<`, `>`

### 4.3 Integer Variables
To enforce discrete solutions (e.g. server instances, pods, workers), declare integer constraints.
Integer variables represent counts: **without an explicit lower bound they are constrained to be `>= 0`**. Declare a bound (e.g. `offset >= -10`) for an integer that may be negative.
```aven
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
