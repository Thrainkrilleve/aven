# Aven Language & Model Guide for AI Models and Systems Engineers

**Version:** 0.6.0  
**Target Audience:** LLMs, Autonomous Coding Agents, Prompt Engineers, and Systems Architects.

---

## 1. Executive Overview

**Aven** is a declarative, domain-specific systems modeling language designed for physical infrastructure, hardware sizing, digital twins, and continuous-discrete simulation. Unlike imperative programming languages (Python, C, Rust), an Aven program is a **system of simultaneous physical and mathematical relations**.

The compiler toolchain (`avenc`) solves these systems omnidirectionally:
- **Forward & Backward Solving:** Solves for any unknown given any valid combination of inputs.
- **Exact Analytical Jacobians:** Calculates symbolic derivatives for continuous non-linear coupled systems.
- **Compile-Time Dimensional Analysis:** Enforces dimensional homogeneity across SI and computing units (e.g. adding `[Gbps]` to `[MB]` is a compile-time type error).
- **Mixed-Integer Branch-and-Bound:** Finds provable global optima for discrete hardware decisions (`nodes`, `drives`, `replicas`).
- **Telemetry-Driven IPC Daemons:** Ingests live telemetry from external services (`using ipc(...)`) with cryptographic provenance ledgers and TTL freshness guarantees.

---

## 2. The AI Model Golden Rules (Avoiding Critical Hallucinations)

When an AI model generates Aven code, it frequently defaults to Python, C, or MiniZinc habits. **Adhere strictly to these six rules:**

| Rule | ❌ NEVER DO THIS | ✅ ALWAYS DO THIS |
| :--- | :--- | :--- |
| **1. Square Brackets are Units** | `var wind_x[100] : float32;` | `temperature = 295.15 [K]`<br>`capacity = 100.0 [TB]` |
| **2. Exponentiation Operator** | `z = pow(x, 1.6);` or `x ** 1.6` | `z = x ^ 1.6` |
| **3. Block Structure** | `model Foo { constraint bar; }` | `relation Foo { ... }`<br>`solve Foo with { ... }` |
| **4. Imperative Types & Control Flow** | `int x = 5;`<br>`if (x > 0) { ... }` | `int x`<br>`x = 5`<br>Use continuous equations instead of `if` |
| **5. Equation Determinacy** | Leaving degrees of freedom unconstrained without an objective | $N$ equations for $N$ unknowns, or provide `minimize(...)` / `maximize(...)` |
| **6. Discontinuous Inversion** | Setting `ceil(x) = 14.5` | Targets for `ceil`, `floor`, and `round` must be integers |

> [!IMPORTANT]
> **Square brackets `[...]` in Aven are ONLY used for physical units of measure.**
> Aven does NOT have array or slice indexing syntax. To model multi-cell grids or fleets, either declare a cell relation and solve across inputs, or unroll variables with descriptive suffixes (e.g., `cell_0_0`, `cell_0_1`).

---

## 3. Formal Syntax & Grammar

### 3.1 Top-Level Blocks
An `.aven` file consists of one or more of the following blocks:

1. **`relation <Name> { ... }`**  
   Declares a set of simultaneous continuous or discrete equations that define physical laws or system relationships.
2. **`solve <TargetRelation> with { ... }`**  
   Sets observation values, boundary constraints (`>=`, `<=`), integer variables (`int`), and optimization objectives (`minimize`, `maximize`).
3. **`forward <Name> { ... }`**  
   Declares a directed acyclic computation block that evaluates strictly in top-to-bottom sequence without Newton-Raphson relaxation.
4. **`external relation <Name> using ipc("<addr>") { ... }`**  
   Binds equations to an active TCP socket IPC telemetry daemon.
5. **`unit <NewUnit> = <Expr> * <BaseUnit>`**  
   Declares a user-defined unit of measure mapped to the SI/Systems dimension registry.

---

### 3.2 Operators & Precedence
Operators in mathematical expressions follow standard scientific precedence:
1. **Exponentiation:** `^` (right-associative: `a ^ b ^ c` = `a ^ (b ^ c)`)
2. **Unary Negation:** `-`
3. **Multiplication & Division:** `*`, `/` (left-associative)
4. **Addition & Subtraction:** `+`, `-` (left-associative)
5. **Relational / Constraints:** `=`, `==`, `!=`, `<`, `<=`, `>`, `>=`
6. **Parentheses:** `(` and `)` override precedence.

---

### 3.3 Built-in Mathematical Functions
Aven supports unary mathematical functions:
- `abs(x)` — Absolute value
- `ceil(x)` — Ceiling integer step function
- `floor(x)` — Floor integer step function
- `round(x)` — Nearest integer rounding
- `sqrt(x)` — Square root
- `exp(x)` — Exponential ($e^x$)
- `log(x)` or `ln(x)` — Natural logarithm (base $e$)
- `sin(x)`, `cos(x)`, `tan(x)` — Trigonometric functions (in radians)

*Note on log base 10:* To compute $\log_{10}(x)$, use `log(x) / 2.302585092994046`.

---

## 4. Compile-Time Physical Units System

Aven includes a compile-time dimensional checking engine. Physical quantities are tagged with bracketed unit expressions.

### 4.1 Built-in Base & Derived Dimensions

#### Fundamental SI Dimensions:
- **Time:** `[s]`, `[ms]`, `[us]`, `[ns]`
- **Length:** `[m]`, `[km]`, `[ft]`, `[in]`, `[mi]`
- **Mass:** `[kg]`, `[g]`, `[lb]`
- **Temperature:** `[K]`, `[degC]`, `[degF]`
- **Electric Current:** `[A]`, `[mA]`
- **Force & Energy:** `[N]`, `[J]`, `[kJ]`, `[MJ]`
- **Power:** `[W]`, `[kW]`, `[MW]`
- **Pressure:** `[Pa]`, `[hPa]`, `[bar]`, `[atm]`, `[psi]`

#### Computing & Systems Dimensions:
- **Data Volume (Bits & Bytes):**
  - Decimal: `[B]`, `[KB]`, `[MB]`, `[GB]`, `[TB]`, `[PB]`
  - Binary: `[KiB]`, `[MiB]`, `[GiB]`, `[TiB]`
- **Throughput & Bandwidth:**
  - `[bps]`, `[Kbps]`, `[Mbps]`, `[Gbps]`
- **Transactions & Rates:**
  - `[req]`, `[reqs]`, `[iops]`, `[reqs/s]`, `[iops/s]`

### 4.2 Dimensional Homogeneity Rules
- **Addition & Subtraction:** Operands must have identical dimensions.
  - `10 [MB] + 5 [MB]` $\implies$ `15 [MB]` (Valid)
  - `10 [Gbps] + 5 [MB]` $\implies$ **Compile-time TypeError: DimensionalMismatch**
- **Multiplication & Division:** Dimensions compound or cancel out:
  - `bandwidth = reqs_per_second * payload_bytes * 8`
- **Exponents:** Powers must be dimensionless scalar quantities.

---

## 5. Solving Paradigms

### 5.1 Continuous Non-Linear Solving (Newton-Raphson)
When a relation has $N$ unknowns and $N$ independent equations, Aven constructs the exact symbolic Jacobian matrix:
$$J_{ij} = \frac{\partial f_i}{\partial x_j}$$
It performs LU decomposition with partial pivoting and iterates until $\|\mathbf{f}(\mathbf{x})\| < 10^{-6}$.
- If rank deficiency is detected, `avenc` prints an under-determination diagnostic naming the exact free variables and redundant equations.

### 5.2 Discrete Mixed-Integer Optimization (Branch-and-Bound)
For capacity sizing and hardware selection:
- Declare discrete whole-number variables with `int <varname>`.
- Set inequality constraints: `usable_tb >= 100.0`, `drives <= 12`.
- Set the target objective: `minimize(cost)` or `maximize(throughput)`.

The solver:
1. Calculates continuous relaxation using Nelder-Mead simplex search over quadratic penalty functions.
2. Identifies unconstrained stationary points via symbolic differentiation ($\nabla f = 0$).
3. Bounded integer decision variables default to $\ge 0$ unless bounded otherwise.
4. Performs binary branching on fractional variables ($x \le \lfloor x \rfloor$ vs. $x \ge \lceil x \rceil$) and prunes non-improving subtrees.

### 5.3 Live Telemetry & IPC Sensor Fusion
Relations can stream external data over localhost sockets:
```aven
external relation WeatherStation using ipc("127.0.0.1:9090") {
    temp_c = observed_temp_c
}
```
- The IPC daemon responds with a strict JSON contract containing `value` and `ttl_ms`.
- If `ttl_ms` expires, `avenc run` blocks decision and exits with code `3` (`EXIT_DECISION_BLOCKED`), preventing stale decisions.
- Every settled fact is hashed into a SHA-256 Merkle audit ledger (`--ledger out.jsonl`).

---

## 6. Canonical Cookbooks & Templates

### 6.1 Hardware Capacity & Cost Sizing (Mixed-Integer)
```aven
relation StorageCluster {
    int nodes
    int drives_per_node

    total_drives = nodes * drives_per_node
    raw_capacity_tb = total_drives * drive_size_tb
    usable_capacity_tb = raw_capacity_tb * ec_efficiency

    hardware_cost = nodes * node_base_cost + total_drives * drive_unit_cost
    idle_power_w = nodes * 120.0 + total_drives * 8.0
}

solve StorageCluster with {
    drive_size_tb = 18.0 [TB]
    ec_efficiency = 0.75
    node_base_cost = 2500.0
    drive_unit_cost = 450.0

    // Design Requirements
    usable_capacity_tb >= 100.0 [TB]
    drives_per_node <= 12

    // Global Objective
    minimize(hardware_cost)
}
```

### 6.2 Data Center Fleet & Thermal Digital Twin
```aven
relation DatacenterFleet {
    // IT Power & compute utilization
    server_power_w = idle_w + cluster_util * dynamic_w
    total_it_kw = servers * server_power_w / 1000.0

    // Thermal airflow heat transfer: dT = P / (rho * cp * Q)
    supply_temp_c = crac_status * crac_setpoint_c + (1.0 - crac_status) * (outside_temp_c + 4.0)
    exhaust_temp_c = supply_temp_c + (1.0 + recirc_ratio) * server_power_w / (air_density * air_cp * airflow_m3s)
    thermal_headroom_c = thermal_limit_c - exhaust_temp_c

    // Chiller efficiency & overall facility power
    chiller_cop = 7.5 - 0.12 * outside_temp_c
    facility_kw = total_it_kw + (crac_status * total_it_kw / chiller_cop) + 0.06 * total_it_kw
    pue = facility_kw / total_it_kw
}

solve DatacenterFleet with {
    servers = 2500
    idle_w = 110.0 [W]
    dynamic_w = 240.0 [W]
    cluster_util = 0.65
    crac_status = 1.0
    crac_setpoint_c = 18.0 [degC]
    outside_temp_c = 28.0 [degC]
    recirc_ratio = 0.25
    air_density = 1.2
    air_cp = 1005.0
    airflow_m3s = 0.05
    thermal_limit_c = 45.0 [degC]

    // Invariant: IT power must not exceed UPS capacity
    total_it_kw <= 800.0 [kW]
}
```

### 6.3 Radar Atmospheric Nowcasting & Physics Bounds

A live, verified build of this is in `evaluations/possible/weather-predict.aven` (driven by
`weather_adapter.py` against NWS/NOAA/USGS feeds; checked to 1e-9 against MetPy and NumPy by
`verify/weather_reference.py`). The simplified single-cell form below shows the corrected
physics — note the units and the distinction between divergence, shear and deformation.

```aven
relation RadarAtmosphericCell {
    // 1. Kinematics: divergence (1/s) and vertical motion via continuity (w = 0 at ground)
    div_horizontal = du_dx + dv_dy
    vertical_velocity_ms = -div_horizontal * layer_depth_m   // m/s = (1/s) * m

    // Deformation and vorticity use the cross terms (not |du_dx| + |dv_dy|, which is stretching)
    vorticity_per_s = dv_dx - du_dy
    deformation_per_s = sqrt((du_dx - dv_dy) ^ 2 + (dv_dx + du_dy) ^ 2)

    // 2. Hydrostatic pressure gradient
    temp_c = temperature_k - 273.15
    dP_dz = -pressure_hpa * 9.80665 / (287.04749 * temperature_k)

    // 3. Saturation vapor pressure (Magnus form shown; the shipped model uses Ambaum 2020)
    e_sat_hpa = 6.112 * exp(17.67 * temp_c / (temp_c + 243.5))
    vapor_pressure_hpa = 6.112 * exp(17.67 * dewpoint_c / (dewpoint_c + 243.5))
    relative_humidity = 100.0 * vapor_pressure_hpa / e_sat_hpa

    // 4. Marshall-Palmer reflectivity. The radar MEASURES dBZ, so avenc solves this backward
    //    for the rain rate — one relation, either direction.
    radar_dbz = 10.0 * log(200.0 * precip_rate_mmh ^ 1.6) / 2.302585092994046

    // 5. Vertical wind shear between the surface and winds aloft (m/s per km)
    vertical_shear_ms_per_km = 1000.0 * sqrt(((aloft_u - wind_u) / aloft_height_m) ^ 2
                                           + ((aloft_v - wind_v) / aloft_height_m) ^ 2)
}

solve RadarAtmosphericCell with {
    temperature_k = 294.15 [K]
    dewpoint_c = 12.0 [degC]
    pressure_hpa = 1013.25 [hPa]
    layer_depth_m = 1000.0 [m]
    du_dx = 0.0003
    dv_dy = 0.0002
    du_dy = 0.0001
    dv_dx = -0.0001
    wind_u = 14.2 [m/s]
    wind_v = 8.6 [m/s]
    aloft_u = 22.0 [m/s]
    aloft_v = 4.0 [m/s]
    aloft_height_m = 1500.0 [m]
    radar_dbz = 43.0   // solved backward -> precip_rate_mmh

    // Physical gates: inconsistent sensors stop the solve
    temperature_k >= 250.0 [K]
    temperature_k <= 320.0 [K]
    vertical_shear_ms_per_km <= 40.0
    relative_humidity <= 105.0
}
```

---

## 7. Toolchain CLI Reference

| Command | Purpose |
| :--- | :--- |
| `avenc check <file.aven>` | Checks syntax, verifies dimensional unit homogeneity, and validates Jacobian rank determinacy. |
| `avenc run <file.aven>` | Solves the model and prints the settled environment and fact ledger. |
| `avenc run <file.aven> --json` | Emits settled variables as structured JSON. |
| `avenc run <file.aven> --ledger log.jsonl` | Writes cryptographically hashed audit trail. |
| `avenc why <file.aven> <var>` | Generates an ASCII causal dependency derivation tree explaining why a variable has its settled value. |
| `avenc build <file.aven> --emit-c -o out.c` | Transpiles pinned relations to standalone, zero-dependency C99 code. |
| `avenc pareto <file.aven> <var> <min> <max>` | Sweeps variable across range to generate Pareto trade-off curve and detects knee point. |
| `avenc verify-ledger <log.jsonl>` | Verifies SHA-256 hash-chain integrity of an audit ledger. |
