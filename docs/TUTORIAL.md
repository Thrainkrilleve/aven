# Aven by Example: The Interactive Language Cookbook (TUTORIAL.md)

Welcome to **Aven by Example**! This tutorial teaches you how to think, design, and code in Aven through progressive, copy-pasteable, and executable recipes.

---

## Table of Contents
1. [Recipe 1: The Relational Mindset](#recipe-1-the-relational-mindset)
2. [Recipe 2: Omnidirectional Solving](#recipe-2-omnidirectional-solving)
3. [Recipe 3: Physical Units & Dimensional Analysis](#recipe-3-physical-units--dimensional-analysis)
4. [Recipe 4: Mixed-Integer Optimization & Constraints](#recipe-4-mixed-integer-optimization--constraints)
5. [Recipe 5: Direction-Pinning & C99 Compilation](#recipe-5-direction-pinning--c99-compilation)
6. [Recipe 6: Compiling to WebAssembly](#recipe-6-compiling-to-webassembly)
7. [Recipe 7: Real-Time Telemetry & External Adapters](#recipe-7-real-time-telemetry--external-adapters)
8. [Recipe 8: Time-Travel Debugging & Counterfactuals](#recipe-8-time-travel-debugging--counterfactuals)
9. [Recipe 9: Creating & Installing Packages](#recipe-9-creating--installing-packages)
10. [Recipe 10: Troubleshooting Solver Diagnostics](#recipe-10-troubleshooting-solver-diagnostics)

---

## Recipe 1: The Relational Mindset

In procedural programming (Python, C, Rust), you write functions that compute outputs from inputs in a fixed sequence:
```python
# Procedural thinking: direction is frozen at authoring time
def calc_force(mass, acceleration):
    return mass * acceleration

def calc_acceleration(force, mass):
    return force / mass
```

If you have 10 interrelated variables, procedural code requires writing dozens of distinct functions.

In **Aven**, you declare **invariants** that must hold true. The compiler determines which unknowns need to be calculated based on what you observe:

```aven
// The physical law is declared once
relation Dynamics {
    force = mass * acceleration
}
```

---

## Recipe 2: Omnidirectional Solving

Save this to `dynamics.aven`:

```aven
relation Dynamics {
    force = mass * acceleration
}

// Scenario A: Standard Forward Calculation
solve Dynamics with {
    mass = 1200.0
    acceleration = 3.5
}
```

Run it:
```bash
avenc run dynamics.aven
```
Output:
```text
Settled Environment:
  acceleration     = 3.5000
  force            = 4200.0000
  mass             = 1200.0000
```

Now, solve **backwards**. Suppose you measure $4200\,\text{N}$ of force and want to determine the required mass to achieve $5.0\,\text{m/s}^2$. Replace the `solve` block in `dynamics.aven` with this one (all solve blocks in a file describe one scenario, so two contradictory ones would be reported as an inconsistency):

```aven-fragment
solve Dynamics with {
    force = 4200.0
    acceleration = 5.0
}
```

Run again:
```bash
avenc run dynamics.aven
```
Output:
```text
Solver Trace:
  [force = mass * acceleration] mass <- backward Newton

Settled Environment:
  acceleration     = 5.0000
  force            = 4200.0000
  mass             = 840.0000
```
> **Notice**: You did not write `mass = force / acceleration`. Aven solved the same equation for `mass` with Newton's method, using the equation's symbolic derivative.

---

## Recipe 3: Physical Units & Dimensional Analysis

Aven checks dimensions across ten base dimensions: the SI quantities length, mass, time, current, temperature and amount of substance, plus angle, money, data and transactions. It also converts values: every number is turned into base units before solving and shown back in its declared unit.

This is `examples/aerodynamics.aven`:
```aven
unit velocity_unit : knots, mach

signal velocity : km/h
signal wing_area : m^2
signal air_density : kg/m^3

relation Aerodynamics {
    dynamic_pressure = 0.5 * air_density * velocity^2
    lift = dynamic_pressure * wing_area * lift_coeff
}

solve Aerodynamics with {
    velocity = 250 km/h
    wing_area = 32 m^2
    lift_coeff = 0.55
    air_density = 1.225 kg/m^3
}
```

```bash
avenc run examples/aerodynamics.aven
```
```text
Solver Trace:
  [dynamic_pressure = 0.5 * air_density * velocity ^ 2] dynamic_pressure <- forward eval
  [lift = dynamic_pressure * wing_area * lift_coeff] lift <- forward eval

Settled Environment:
  air_density      = 1.2250 kg/m^3
  dynamic_pressure = 2953.8002 Pa
  lift             = 51986.8827
  lift_coeff       = 0.5500
  velocity         = 250.0000 km/h
  wing_area        = 32.0000 m^2

Fact Ledger: 6 facts settled.
```

250 km/h was solved as 69.44 m/s, so the dynamic pressure is in pascals. `lift` carries no label because `lift_coeff` has no unit, which leaves the dimension of `lift` unknown.

If an equation mixes incompatible dimensions, `avenc check` (and `run`) stop before anything executes:
```aven-error
signal velocity : km/h
signal gust_duration : s

relation Gust {
    test = velocity + gust_duration
}
```
```text
✗ Type Error at line 5, col 5: dimensional mismatch in expression '(velocity + gust_duration)' — cannot perform '+/-' between [m·s^-1] and [s]
```

Unit symbols are case-sensitive where case matters: `MW` is a megawatt and `mW` a milliwatt, `MB` a megabyte and `Mbit` a megabit. Spellings that could mean either (`mw`, `Mb`, `gb`) are rejected with a list of the valid choices.

---

## Recipe 4: Mixed-Integer Optimization & Constraints

Physical engineering often involves discrete units (e.g. whole server nodes, whole hard drives, integer gear teeth). Aven natively incorporates branch-and-bound mixed-integer simplex solving.

Create `cluster_sizing.aven`:
```aven
relation ServerProvisioning {
    int nodes
    total_cores = nodes * cores_per_node
    total_ram_gb = nodes * ram_per_node_gb
    total_cost_monthly = nodes * node_monthly_cost
    
    // Bounds / Invariant inequalities:
    total_cores >= required_cores
    total_ram_gb >= required_ram_gb
}

solve ServerProvisioning with {
    cores_per_node = 16.0
    ram_per_node_gb = 64.0
    node_monthly_cost = 145.0
    
    required_cores = 100.0
    required_ram_gb = 350.0
    
    minimize(nodes)
}
```

Run:
```bash
avenc run cluster_sizing.aven
```

Output:
```text
Solver Trace:
  [optimize] simplex branch-and-bound (1 nodes) — objective = 7.000000 (proven optimal) [bound = 7.000000, duality_gap = 0.00e0, cuts = 1, dual_norm = 3.00e0]

Settled Environment:
  cores_per_node   = 16.0000
  node_monthly_cost = 145.0000
  nodes            = 7 (int)
  ram_per_node_gb  = 64.0000
  required_cores   = 100.0000
  required_ram_gb  = 350.0000
  total_cores      = 112.0000
  total_cost_monthly = 1015.0000
  total_ram_gb     = 448.0000

Fact Ledger: 10 facts settled.
```
Aven determines that $7$ nodes are required (cores require $\lceil 100/16 \rceil = 7$; RAM requires $\lceil 350/64 \rceil = 6$; the maximum 7 satisfies both bounds).

---

## Recipe 5: Direction-Pinning & C99 Compilation

When deploying to embedded microcontrollers or fixed-rate control loops, an iterative solver adds variable latency. Aven provides `forward` blocks for compile-time direction-pinning and solver-free C99 code generation. The generated function works in base units (SI), the same values `avenc run --json` reports.

Create `governor.aven`:
```aven
forward ThrottleGovernor {
    error = target_rpm - current_rpm
    p_term = error * kp
    throttle_pct = base_throttle + p_term
}

solve ThrottleGovernor with {
    target_rpm = 3000.0
    current_rpm = 2800.0
    kp = 0.25
    base_throttle = 40.0
}
```

Emit C99:
```bash
avenc emit-c governor.aven -o governor.c
```

Aven outputs topologically sorted, solver-free C code:
```c
/* Generated by avenc -- direction-pinned, solver-free */
#include <stdio.h>

static void aven_ThrottleGovernor(double base_throttle, double current_rpm, double kp, double target_rpm, double* out_error, double* out_p_term, double* out_throttle_pct) {
    const double error = (target_rpm - current_rpm);
    const double p_term = (error * kp);
    const double throttle_pct = (base_throttle + p_term);
    *out_error = error;
    *out_p_term = p_term;
    *out_throttle_pct = throttle_pct;
}

int main(void) {
    double error = 0.0;
    double p_term = 0.0;
    double throttle_pct = 0.0;
    aven_ThrottleGovernor(40.0, 2800.0, 0.25, 3000.0, &error, &p_term, &throttle_pct);
    printf("error = %.6g\n", error);
    printf("p_term = %.6g\n", p_term);
    printf("throttle_pct = %.6g\n", throttle_pct);
    return 0;
}
```

---

## Recipe 6: Compiling to WebAssembly

You can compile Aven relations directly to standard binary WebAssembly (`.wasm`) or WebAssembly Text (`.wat`) for execution in browsers, edge workers, and Node.js:

```bash
# Emit standard binary WASM
avenc emit-wasm governor.aven -o governor.wasm

# Or inspect human-readable WAT
avenc emit-wasm governor.aven --wat
```

Run in Node.js with zero dependencies:
```javascript
const fs = require('fs');
const wasmBuffer = fs.readFileSync('governor.wasm');

WebAssembly.instantiate(wasmBuffer).then(({ instance }) => {
    // Direct execution export (parameters sorted alphabetically):
    // ThrottleGovernor(base_throttle, current_rpm, kp, target_rpm)
    const throttle = instance.exports.ThrottleGovernor(40.0, 2800.0, 0.25, 3000.0);
    console.log("Throttle %:", throttle); // 90.0
});
```

---

## Recipe 7: Real-Time Telemetry & External Adapters

Aven connects to live sensors, message brokers, and telemetry streams via subprocesses, IPC sockets, or shared memory rings:

A `using` clause attaches a backend to a relation; the backend supplies facts and the relation's equations are solved over them:

```aven
// Ingest from an external Python simulator (prints JSON facts):
relation Altimeter using external("python", "sensor_stream.py") {
    altitude_ft = current_altitude_m * 3.28084
}

// Ingest from a persistent local TCP daemon:
relation Turbine using ipc("127.0.0.1:9090") {
    vibration_ms2 = turbine_vibration_g * 9.80665
}
```

See `examples/flame.aven` and `evaluations/flight_mesh/flight_mesh_ipc.aven` for complete, runnable models.

Every fact from an adapter records the SHA-256 of the adapter's raw output in the fact ledger. Adapters send and receive values in each variable's declared unit.

### Supervising a live feed with `avenc stream`

`avenc stream` reads one JSON object per line from stdin, adds its fields to the model's facts and solves each line as one cycle. Every cycle ends in one of three states:

- `SETTLED`: the model solved and every bound held; the derived values are printed.
- `ALARM`: the model solved but a bound is broken; the line names the bound and the value.
- `UNSOLVED`: the reading does not determine the model (for example, a field is missing) or the solve failed. A missing reading is never filled in.

[`examples/stream/openplc_supervisor.aven`](../examples/stream/openplc_supervisor.aven) checks PLC readings (`scan_ms`, `temp_c`, `power_w`, `quality_pct`) against limits. Three readings, the last one incomplete:

```bash
printf '%s\n' \
  '{"scan_ms": 9.2, "power_w": 211, "temp_c": 43.1, "quality_pct": 98.7}' \
  '{"scan_ms": 9.4, "power_w": 215, "temp_c": 61.0, "quality_pct": 98.5}' \
  '{"scan_ms": 9.3}' \
  | avenc stream examples/stream/openplc_supervisor.aven
```

```text
#1      | a7c77c66d26612c6 | SETTLED
        power_margin_w = 39.0000, quality_margin = 3.7000, scan_margin_ms = 8.8000, thermal_margin_c = 11.9000
#2      | c553b51e377de931 | ALARM: Constraint violation in 'thermal_margin_c >= 0': thermal_margin_c = -6.0000 violates bound 0.0000
#3      | 429fb659cb5aa557 | UNSOLVED: System is under-determined (3 degree(s) of freedom):
...
Stream completed: 3 cycle(s), 1 settled, 1 alarm(s), 1 unsolved.
```

To supervise a running OpenPLC controller, pipe in the Modbus TCP poller, which reads the controller's holding registers once a second (the register map is at the top of the script; `pip install pymodbus`):

```bash
python evaluations/tests/PLC-test/openplc_poller.py --host <plc-address> --port 502 \
  | avenc stream examples/stream/openplc_supervisor.aven
```

The limits in the example (18 ms scan deadline, 55 °C enclosure, 250 W supply, 95 % quality floor) are placeholders: set them for your own controller and line.

---

## Recipe 8: Time-Travel Debugging & Counterfactuals

Aven features a first-class temporal debugger (`avenc debug`).

Replay a model, optionally from a ledger written earlier with `avenc run --ledger`:
```bash
avenc debug examples/server_capacity.aven
avenc debug examples/server_capacity.aven audit.jsonl
```

Commands inside the REPL:
- `step` / `prev`: Move forward or back one logical tick.
- `goto <n>`: Jump to tick `n`.
- `vars`: Show every variable at the current tick.
- `why <var>`: Print the derivation tree of `<var>`.
- `what-if <var>=<value>`: Branch a counterfactual where `<var>` has a different value (in its declared unit), and show the effect on every output.
- `diff [t1 t2]`: Show what changed between two ticks.
- `seal`: Show the ledger's chain head and verify it.
- `help`, `quit`.

The same operations work non-interactively: `--goto <n>`, `--what-if "<var>=<value>"`, `--diff <t1>,<t2>`.

---

## Recipe 9: Creating & Installing Packages

Aven includes a built-in package manager with cryptographic SHA-256 integrity sealing:

```bash
# 1. Initialize a package in a new directory (writes Aven.toml and lib.aven there)
mkdir my_physics_lib && cd my_physics_lib
avenc pkg init my_physics_lib

# 2. Dimension-check and seal it into my_physics_lib-0.1.0.avenpkg
avenc pkg pack

# 3. Install it into another project's .aven/packages/
cd ../my_project
avenc pkg install ../my_physics_lib/my_physics_lib-0.1.0.avenpkg

# 4. List installed packages
avenc pkg list
```

`pkg init` generates a starter relation, `my_physics_libRelations`. Import the package and solve it:
```aven-fragment
import my_physics_lib;

solve my_physics_libRelations with {
    voltage_v = 230
    current_a = 10
    operating_hours = 5
}
```
```text
Settled Environment:
  current_a        = 10.0000
  energy_kwh       = 11.5000
  operating_hours  = 5.0000
  power_kw         = 2.3000
  voltage_v        = 230.0000
```

---

---

## Recipe 10: Troubleshooting Solver Diagnostics

### Diagnostic: Underdetermined System
If you provide fewer facts than unknowns, Aven reports the degrees of freedom and the bindings that would fix them, and exits with code 1:
```aven
relation Box {
    volume = length * width * height
}

solve Box with {
    volume = 1000
    height = 10
}
```
```text
┌────────────────────────────────────────────────────────────────────┐
│ UNDER-DETERMINED SYSTEM — solver stalled, here is why:             │
│                                                                    │
│   unknowns  (2): length, width                                     │
│   relations (1):                                                   │
│       volume = length * width * height                             │
│                                                                    │
│   Jacobian rank: 1 of 2 columns ⟹ 1 degree(s) of freedom           │
│   FREE variables (unconstrained directions):                       │
│       width                                                        │
│                                                                    │
│   MINIMAL FIX — any ONE of these bindings resolves it:             │
│       • bind 'length' — reduces system to 1 independent unknown(s) [width] (0 DOF) │
│       • bind 'width' — reduces system to 1 independent unknown(s) [length] (0 DOF) │
└────────────────────────────────────────────────────────────────────┘
```

### Diagnostic: Stale Input Data
If an adapter reports facts with a `ttl_ms` that has expired, Aven refuses to settle the decision and exits with code 3 (pass `--allow-stale` to override):
```text
❌ DECISION BLOCKED: Freshness Enforcement Failure
  The following external fact(s) are STALE (expired TTL or ttl_ms=0):
    • pitot_pressure_pa
    • airspeed_ms

  Refusing to settle a decision on expired inputs.
  Pass --allow-stale to explicitly override in non-production environments.
```

---

## Next Steps
- Explore the complete [Engineering & Mathematical Standards](../STANDARDS.md).
- Keep the [Quick-Reference Cheat Sheet](CHEAT_SHEET.md) handy while coding.
- Study larger example models: [examples/portfolio_rebalancing.aven](../examples/portfolio_rebalancing.aven) and [examples/quadcopter_avionics.aven](../examples/quadcopter_avionics.aven).
