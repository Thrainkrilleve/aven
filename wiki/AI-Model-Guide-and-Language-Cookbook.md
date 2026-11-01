# AI Model Guide and Language Cookbook

This guide serves as the definitive reference for AI models (LLMs, coding assistants) and systems engineers modeling physical systems, hardware infrastructure, and digital twins in **Aven**.

For the repository-level document, see [`docs/AVEN_AI_MODEL_GUIDE.md`](https://github.com/Thrainkrilleve/aven/blob/main/docs/AVEN_AI_MODEL_GUIDE.md).

---

## 1. Quick Syntax Rules for AI Assistants

1. **Units follow the number, without brackets:** `295.15 K`, `10.0 Gbps`, `250 km/h`. Aven has no array slicing or indexing syntax.
2. **Infix Exponents:** Use `^` for powers (`z = x ^ 1.6`), NEVER `pow(x, y)` or `x ** y`.
3. **Relation vs. Solve:** 
   - Define physical relationships in `relation Name { ... }`.
   - Supply bindings, inequalities, and objectives in `solve Name with { ... }`.
4. **Discrete Optimization:** Declare discrete variables with `int varname` and set an objective (`minimize(cost)`).
5. **No Imperative Control Flow:** Aven equations are simultaneous constraints, not sequential assignments. Do not use `if`, `while`, or `return`.
6. **Units convert to SI:** an annotated value is converted to base SI before solving (`18 degC` becomes 291.15 K). If a formula's constants assume °C, hPa or another non-SI unit, give plain numbers instead of annotations.

---

## 2. Dimensional Checking Cheat Sheet

Aven validates physical unit consistency at compile time and converts values to base SI before solving.

| Operation | Dimension Rule | Example (with `signal`-declared units) |
| :--- | :--- | :--- |
| `a + b` / `a - b` | Dimensions must match | `p_kw + p_w` with `kW` and `W` → `W` (values converted) |
| `a * b` | Dimensions multiply | `speed * time` with `m/s` and `s` → `m` |
| `a / b` | Dimensions divide | `distance / time` with `m` and `s` → `m/s` |
| `a ^ n` | Exponent is a dimensionless constant; result must have whole-number dimensions | `radius ^ 2` with `m` → `m^2`; `area ^ 0.5` with `m^2` → `m` |

---

## 3. Standard Cookbooks

### Capacity Planning (Discrete Sizing)
```aven
signal drive_size_tb   : scalar
signal raid_eff        : scalar
signal nodes           : scalar
signal drives_per_node : scalar

relation HardwareSizing {
    int nodes
    int drives_per_node
    total_drives = nodes * drives_per_node
    usable_tb = total_drives * drive_size_tb * raid_eff
    total_cost = nodes * 2500.0 + total_drives * 450.0
}

solve HardwareSizing with {
    drive_size_tb = 18.0
    raid_eff = 0.75
    usable_tb >= 100.0
    drives_per_node <= 12
    minimize(total_cost)
}
```

### Thermal Digital Twin
```aven
signal idle_w    : scalar
signal dynamic_w : scalar
signal util      : scalar
signal inlet_c   : scalar
signal recirc    : scalar
signal rho       : scalar
signal cp        : scalar
signal flow_m3s  : scalar
signal limit_c   : scalar

relation ThermalTwin {
    power_w = idle_w + util * dynamic_w
    exhaust_c = inlet_c + (1.0 + recirc) * power_w / (rho * cp * flow_m3s)
    headroom_c = limit_c - exhaust_c
}

solve ThermalTwin with {
    idle_w = 110.0
    dynamic_w = 240.0
    util = 0.70
    inlet_c = 18.0
    recirc = 0.25
    rho = 1.2
    cp = 1005.0
    flow_m3s = 0.05
    limit_c = 45.0
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

# Compile the forward block to standalone C99
avenc emit-c model.aven -o model.c
```
