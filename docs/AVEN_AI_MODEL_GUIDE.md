# Aven Language & Model Guide for AI Models and Systems Engineers

**Version:** 0.6.0  
**Target Audience:** LLMs, Autonomous Coding Agents, Prompt Engineers, and Systems Architects.

---

## 1. Executive Overview

**Aven** is a declarative, domain-specific systems modeling language designed for physical infrastructure, hardware sizing, digital twins, and continuous-discrete simulation. Unlike imperative programming languages (Python, C, Rust), an Aven program is a **system of simultaneous physical and mathematical relations**.

The compiler toolchain (`avenc`) solves these systems omnidirectionally:
- **Forward & Backward Solving:** Solves for any unknown given any valid combination of inputs.
- **Symbolic Derivatives:** Newton steps use exact symbolic derivatives of each equation (with a central-difference fallback where a derivative cannot be evaluated).
- **Units:** Checks dimensional homogeneity (adding `Gbps` to `MB` is a compile-time type error) and converts unit-annotated values to base units before solving.
- **Mixed-Integer Branch-and-Bound:** Linear models are solved exactly by simplex with branch-and-bound; nonlinear models are certified by interval branch-and-bound when every variable is bounded. Results are labelled `proven optimal` only when certified.
- **Live Data:** Ingests facts from subprocess adapters (`using external(...)`) and TCP daemons (`using ipc(...)`) into a hash-chained ledger, with TTL freshness checks.

---

## 2. The AI Model Golden Rules (Avoiding Critical Hallucinations)

When an AI model generates Aven code, it frequently defaults to Python, C, or MiniZinc habits. **Adhere strictly to these rules:**

| Rule | ❌ NEVER DO THIS | ✅ ALWAYS DO THIS |
| :--- | :--- | :--- |
| **1. Units follow the number, without brackets** | `temperature = 295.15` | `temperature = 295.15 K`<br>`capacity = 100.0 TB` |
| **2. Exponentiation Operator** | `z = pow(x, 1.6);` or `x ** 1.6` | `z = x ^ 1.6` |
| **3. Block Structure** | `model Foo { constraint bar; }` | `relation Foo { ... }`<br>`solve Foo with { ... }` |
| **4. Imperative Types & Control Flow** | `int x = 5;`<br>`if (x > 0) { ... }` | `int x` in a block, `x = 5` in `solve`<br>Use continuous equations instead of `if` |
| **5. Equation Determinacy** | Leaving degrees of freedom unconstrained without an objective | $N$ equations for $N$ unknowns, or provide `minimize(...)` / `maximize(...)` |
| **6. Discontinuous Inversion** | Setting `ceil(x) = 14.5` | Targets for `ceil`, `floor`, and `round` must be integers |
| **7. Units and constants must agree** | `chiller_cop = 7.5 - 0.12 * outside_temp_c` with `outside_temp_c = 28 degC` | Either give plain numbers (`outside_temp_c = 28`) to formulas written for those units, or give units and write the formula in SI |

> [!IMPORTANT]
> **A unit annotation converts the value to base SI.** `outside_temp_c = 28 degC` is solved as 301.15 K and `pressure_hpa = 1013.25 hPa` as 101,325 Pa. A formula whose constants assume °C or hPa will then be wrong. Use unit annotations only when the equations are written in SI; otherwise give plain numbers and let the variable name carry the unit.

> [!IMPORTANT]
> Aven has no array or slice indexing syntax. To model multi-cell grids or fleets, either declare a cell relation and solve across inputs, or unroll variables with descriptive suffixes (e.g., `cell_0_0`, `cell_0_1`).

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
4. **`relation <Name> using ipc("<addr>") { ... }`** (or `using external("cmd", "script")`)  
   Takes facts from a TCP daemon or a subprocess adapter and solves the block's equations over them.
5. **`signal <name> : <unit>`**  
   Declares the unit a variable's bare numbers are read and shown in.
6. **`unit <dimension> : <name>, <name>`**  
   Declares units of a new dimension (they convert only among themselves).

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
- `sign(x)` — −1, 0 or +1
- `sin(x)`, `cos(x)`, `tan(x)` — Trigonometric functions (in radians)

Constants: `pi`, `e`, `tau`.

*Note on log base 10:* To compute $\log_{10}(x)$, use `log(x) / 2.302585092994046`.

---

## 4. Units

A unit is written after a number in a `solve` block (`velocity = 250 km/h`) or declared for a variable with `signal velocity : km/h`, in which case bare numbers for it are read in that unit. Values are converted to base units (SI, plus bit, radian, transaction and US dollar) before solving, and shown back in each variable's declared unit.

### 4.1 Built-in Units
- **Length:** `m`, `km`, `cm`, `mm`, `um`, `in`, `ft`, `mi`, `nmi`
- **Time:** `s`, `ms`, `us`, `ns`, `min`, `h`, `hr`, `day`
- **Mass:** `kg`, `g`, `mg`, `lb`
- **Temperature:** `K`, `degC`, `degF` (absolute temperatures)
- **Electric Current:** `A`, `mA`
- **Force & Energy:** `N`, `kN`, `J`, `kJ`, `MJ`, `Wh`, `kWh`, `MWh`
- **Power & Electrical:** `W`, `mW`, `kW`, `MW`, `V`, `mV`, `kV`, `ohm`
- **Pressure:** `Pa`, `hPa`, `kPa`, `MPa`, `bar`, `atm`, `psi`
- **Data:** `b`/`bit`, `B`/`byte`, `kB`/`KB`, `MB`, `GB`, `TB`, `PB`, `KiB`, `MiB`, `GiB`, `TiB`, `kbit`, `Mbit`, `Gbit`, `Tbit`
- **Rates:** `bps`, `kbps`, `Mbps`, `Gbps`, `Tbps`, `iops`, `rps`, `pph`
- **Transactions:** `req`, `iop`, `pkt`, `tx`
- **Money:** `$`, `USD`; `EUR` and `GBP` are separate dimensions (no built-in exchange rates)
- **Compound:** combine with `/`, `*` and integer `^`, e.g. `km/h`, `kg/m^3`, `req/s`

Symbols are case-sensitive where case matters (`MW` megawatt, `mW` milliwatt). Ambiguous spellings such as `Mb` (megabit or megabyte?) are rejected.

### 4.2 Dimensional Homogeneity Rules
- **Addition & Subtraction:** Operands must have identical dimensions; `network_rate + storage_size` with `Gbps` and `MB` is a compile-time type error.
- **Multiplication & Division:** Dimensions compound or cancel out.
- **Exponents:** The exponent is dimensionless. A dimensioned base needs a constant exponent that keeps whole-number dimensions (`area ^ 0.5` with `area` in `m^2` is in `m`).
- **Functions:** `exp`, `ln` take dimensionless arguments; `sin`, `cos`, `tan` take dimensionless values or angles.

---

## 5. Solving Paradigms

### 5.1 Continuous Non-Linear Solving (Newton-Raphson)
When a relation has $N$ unknowns and $N$ independent equations, Aven builds the Jacobian from symbolic derivatives:
$$J_{ij} = \frac{\partial f_i}{\partial x_j}$$
It solves each Newton step by Gaussian elimination with partial pivoting and iterates until every residual satisfies $|f_i(\mathbf{x})| < 10^{-9}$, trying a fixed set of starting points.
- If rank deficiency is detected, `avenc` prints an under-determination diagnostic naming the free variables and the bindings that would fix them.

### 5.2 Discrete Mixed-Integer Optimization (Branch-and-Bound)
For capacity sizing and hardware selection:
- Declare discrete whole-number variables with `int <varname>`.
- Set inequality constraints: `usable_tb >= 100.0`, `drives <= 12`.
- Set the target objective: `minimize(cost)` or `maximize(throughput)`.

The solver:
1. **Linear models** (everything affine in the unknowns): two-phase simplex with Bland's rule, and branch-and-bound on LP relaxations for integers. Optimality, infeasibility and unboundedness are proven.
2. **Nonlinear models**: augmented Lagrangian with a spectral projected-gradient inner solver from several starting points, branch-and-bound for integers, then interval branch-and-bound to certify the optimum when every variable is bounded.
3. The trace says `proven optimal` only for certified results; otherwise the result is a local optimum.

### 5.3 Live Telemetry & IPC Sensor Fusion
Relations can take facts from a local TCP daemon:
```aven
relation WeatherStation using ipc("127.0.0.1:9090") {
    temp_c = observed_temp_c
}
```
- The daemon answers each request with one JSON object; its numeric fields become facts. Optional `timestamp_ms` and `ttl_ms` fields mark freshness.
- If a fact's TTL has expired, `avenc run` blocks the decision and exits with code `3`, unless `--allow-stale` is given.
- Every settled fact is appended to a SHA-256 hash chain (`--ledger out.jsonl`), HMAC-signed when a key is supplied.

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
    drive_size_tb = 18.0
    ec_efficiency = 0.75
    node_base_cost = 2500.0
    drive_unit_cost = 450.0

    // Design Requirements
    usable_capacity_tb >= 100.0
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
    idle_w = 110.0
    dynamic_w = 240.0
    cluster_util = 0.65
    crac_status = 1.0
    crac_setpoint_c = 18.0
    outside_temp_c = 28.0
    recirc_ratio = 0.25
    air_density = 1.2
    air_cp = 1005.0
    airflow_m3s = 0.05
    thermal_limit_c = 45.0

    // Invariant: IT power must not exceed UPS capacity
    total_it_kw <= 800.0
}
```

### 6.3 Radar Atmospheric Nowcasting & Physics Bounds

A live build of this is in `evaluations/possible/weather-predict.aven` (driven by
`weather_adapter.py` against NWS/NOAA/USGS feeds). `verify/weather_reference.py` checks it
against MetPy, NumPy and the published formulas, and checks the forward block
(`examples/weather_nowcast.aven`) against it: on 150 random states (seed 1) every output
agrees to within 1e-9 relative (largest 1.3e-10, the radar inverse that the relational model
solves by Newton iteration). The simplified single-cell form below shows the
physics — note the distinction between divergence, shear and deformation. Its formulas are
written for °C and hPa, so the inputs are plain numbers (see rule 7).

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
    temperature_k = 294.15
    dewpoint_c = 12.0
    pressure_hpa = 1013.25
    layer_depth_m = 1000.0
    du_dx = 0.0003
    dv_dy = 0.0002
    du_dy = 0.0001
    dv_dx = -0.0001
    wind_u = 14.2
    wind_v = 8.6
    aloft_u = 22.0
    aloft_v = 4.0
    aloft_height_m = 1500.0
    radar_dbz = 43.0   // solved backward -> precip_rate_mmh

    // Physical gates: inconsistent sensors stop the solve
    temperature_k >= 250.0
    temperature_k <= 320.0
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
| `avenc run <file.aven> --ledger log.jsonl` | Writes the SHA-256 hash-chained audit ledger. |
| `avenc why <file.aven> <var>` | Generates an ASCII causal dependency derivation tree explaining why a variable has its settled value. |
| `avenc emit-c <file.aven> -o out.c` | Transpiles the `forward` block to standalone C99 (base units). |
| `avenc emit-wasm <file.aven> -o out.wasm` | Transpiles the `forward` block to WebAssembly 1.0. |
| `avenc pareto <file.aven> <var> <min> <max>` | Sweeps a variable across a range, tabulates every output, and marks the knee point. |
| `avenc verify-ledger <log.jsonl>` | Verifies SHA-256 hash-chain integrity of an audit ledger. |
