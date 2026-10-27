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
9. [Recipe 9: Creating & Installing Packages](#recipe-9-creating--installating-packages)
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

Now, solve **backwards**. Suppose you measure $4200\,\text{N}$ of force and want to determine the required mass to achieve $5.0\,\text{m/s}^2$:

```aven
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
Settled Environment:
  acceleration     = 5.0000
  force            = 4200.0000
  mass             = 840.0000
```
> **Notice**: You did not write `mass = force / acceleration`. Aven applied symbolic differentiation and analytical substitution automatically.

---

## Recipe 3: Physical Units & Dimensional Analysis

Aven validates physical units at compile-time across the 7 SI base dimensions: Mass $[M]$, Length $[L]$, Time $[T]$, Current $[I]$, Temperature $[\Theta]$, Substance $[N]$, and Luminous Intensity $[J]$.

Create `aerodynamics.aven`:
```aven
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

Run type check:
```bash
avenc check aerodynamics.aven
```

If you mistakenly attempt to add seconds to velocity (`velocity + 10 s`), Aven halts compilation immediately:
```text
error[E0102]: dimensional mismatch in equation
  --> aerodynamics.aven:9:24
   |
 9 |   test = velocity + 10 s;
   |          ^^^^^^^^   ^^^^
   |          [L T^-1]   [T]
```

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
  [optimize] simplex branch-and-bound (1 nodes) — objective = 7.000000 (proven optimal)

Settled Environment:
  nodes            = 7 (int)
  total_cores      = 112.0000
  total_ram_gb     = 448.0000
  total_cost_monthly = 1015.0000
```
Aven determines that $7$ nodes are required (cores require $\lceil 100/16 \rceil = 7$; RAM requires $\lceil 350/64 \rceil = 6$; the maximum 7 satisfies both bounds).

---

## Recipe 5: Direction-Pinning & C99 Compilation

When deploying to embedded microcontrollers, automotive ECUs, or bare-metal flight controllers, runtime numerical solvers add latency and indeterminism. Aven provides `forward` blocks for compile-time direction-pinning and zero-overhead C99 code generation.

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

Emit freestanding ANSI C99:
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

```aven
// Ingest from external Python simulator:
using external("python", "sensor_stream.py") {
    current_altitude_m
    outside_temp_c
}

// Ingest from persistent local TCP daemon:
using ipc("127.0.0.1:9090") {
    turbine_vibration_g
}

// Ingest from shared memory ring buffer (< 15 microseconds latency):
using shm("/dev/shm/telemetry_ring") {
    shaft_torque_nm
}
```

Every external fact records the SHA-256 fingerprint of the incoming payload in the cryptographic fact ledger.

---

## Recipe 8: Time-Travel Debugging & Counterfactuals

Aven features a first-class temporal debugger (`avenc debug`).

Launch an interactive debugging session on a recorded ledger:
```bash
avenc debug --ledger flight_ledger.jsonl
```

Available commands inside the REPL:
- `step`: Advance by 1 logical cycle.
- `prev`: Step backwards by 1 cycle.
- `goto 42`: Scrub timeline directly to cycle 42.
- `why <var>`: Trace the full causal provenance tree showing which physical equations and ancestor facts produced `<var>`.
- `what-if <var> = <value>`: Branch into a speculative counterfactual universe where `<var>` changed at this point in time.
- `diff`: Display side-by-side delta between the baseline physical timeline and your counterfactual branch.
- `quit`: Exit the debugger.

---

## Recipe 9: Creating & Installing Packages

Aven includes a built-in package manager with cryptographic SHA-256 integrity sealing:

```bash
# 1. Initialize a new package
avenc pkg init my_physics_lib
cd my_physics_lib

# 2. Package into a sealed .avenpkg distribution archive
avenc pkg pack

# 3. Install into local/cache registry
avenc pkg install my_physics_lib-0.1.0.avenpkg

# 4. List installed packages
avenc pkg list
```

In your project models, import verified package relations:
```aven
import my_physics_lib;

relation MissionControl {
    payload_mass_kg = 500.0
}

// Solve using the inlined relations from my_physics_lib:
solve OrbitalDecay with {
    altitude_km = 400.0
    spacecraft_mass_kg = 1200.0
}
```

---

## Recipe 10: Troubleshooting Solver Diagnostics

### Diagnostic: Underdetermined System
If you provide fewer facts than unknowns, Aven identifies the exact degrees of freedom and suggests minimal fixes:
```text
error[E0301]: underdetermined system (1 degree of freedom)
  --> model.aven:5:1
   |
 5 | relation Box { volume = length * width * height }
   | ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
   | unknowns: [length, width]
   | suggestion: bind one of ['length', 'width'] in solve block
```

### Diagnostic: Stale Input Data
If live sensor inputs exceed their declared Time-To-Live (`ttl_ms`), Aven refuses to make ungrounded decisions and exits with Code 3:
```text
DECISION BLOCKED: input 'pitot_pressure_pa' expired (stale by 420ms > TTL 100ms)
exit code: 3 (STALE_INPUT)
```

---

## Next Steps
- Explore the complete [Engineering & Mathematical Standards](file:///f:/aven/STANDARDS.md).
- Keep the [Quick-Reference Cheat Sheet](file:///f:/aven/docs/CHEAT_SHEET.md) handy while coding.
- Study real-world production models in [examples/portfolio_rebalancing.aven](file:///f:/aven/examples/portfolio_rebalancing.aven) and [examples/quadcopter_avionics.aven](file:///f:/aven/examples/quadcopter_avionics.aven).
