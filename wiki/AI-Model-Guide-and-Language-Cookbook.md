# AI Model Guide and Language Cookbook

This guide serves as the definitive reference for AI models (LLMs, coding assistants) and systems engineers modeling physical systems, hardware infrastructure, and digital twins in **Aven**.

For the repository-level document, see [`docs/AVEN_AI_MODEL_GUIDE.md`](file:///f:/aven/docs/AVEN_AI_MODEL_GUIDE.md).

---

## 1. Quick Syntax Rules for AI Assistants

1. **Units in Brackets:** Square brackets `[...]` are ONLY used for physical dimensions (e.g. `100.0 [TB]`, `295.15 [K]`, `10.0 [Gbps]`). Aven does NOT have array slicing or indexing syntax.
2. **Infix Exponents:** Use `^` for powers (`z = x ^ 1.6`), NEVER `pow(x, y)` or `x ** y`.
3. **Relation vs. Solve:** 
   - Define physical relationships in `relation Name { ... }`.
   - Supply bindings, inequalities, and objectives in `solve Name with { ... }`.
4. **Discrete Optimization:** Declare discrete variables with `int varname` and set an objective (`minimize(cost)`).
5. **No Imperative Control Flow:** Aven equations are simultaneous constraints, not sequential assignments. Do not use `if`, `while`, or `return`.

---

## 2. Dimensional Checking Cheat Sheet

Aven validates physical unit consistency at compile time.

| Operation | Dimension Rule | Example |
| :--- | :--- | :--- |
| `a + b` / `a - b` | Dimensions must match | `10 [kW] + 500 [W]` $\to$ `10.5 [kW]` |
| `a * b` | Dimensions multiply | `speed [m/s] * time [s]` $\to$ `[m]` |
| `a / b` | Dimensions divide | `distance [m] / time [s]` $\to$ `[m/s]` |
| `a ^ n` | Exponent must be dimensionless | `radius [m] ^ 2` $\to$ `[m^2]` |

---

## 3. Standard Cookbooks

### Capacity Planning (Discrete Sizing)
```aven
relation HardwareSizing {
    int nodes
    int drives_per_node
    total_drives = nodes * drives_per_node
    usable_tb = total_drives * drive_size_tb * raid_eff
    total_cost = nodes * 2500.0 + total_drives * 450.0
}

solve HardwareSizing with {
    drive_size_tb = 18.0 [TB]
    raid_eff = 0.75
    usable_tb >= 100.0 [TB]
    drives_per_node <= 12
    minimize(total_cost)
}
```

### Thermal Digital Twin
```aven
relation ThermalTwin {
    power_w = idle_w + util * dynamic_w
    exhaust_c = inlet_c + (1.0 + recirc) * power_w / (rho * cp * flow_m3s)
    headroom_c = limit_c - exhaust_c
}

solve ThermalTwin with {
    idle_w = 110.0 [W]
    dynamic_w = 240.0 [W]
    util = 0.70
    inlet_c = 18.0 [degC]
    recirc = 0.25
    rho = 1.2
    cp = 1005.0
    flow_m3s = 0.05
    limit_c = 45.0 [degC]
}
```

---

## 4. Compiler Commands

```bash
# Validate syntax, units, and rank determinacy
avenc check model.aven

# Solve continuous or discrete system
avenc run model.aven

# Inspect causal lineage
avenc why model.aven varname

# Compile to standalone C99
avenc build model.aven --emit-c -o model.c
```
