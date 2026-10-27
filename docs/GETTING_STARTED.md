# Getting Started with Aven: The Weekend-First Guide

Welcome to **Aven** — the non-procedural programming language that replaces fragile imperative scripts with unbreakable mathematical invariants and cryptographic audit trails.

This guide takes a motivated engineer from zero to building, compiling, verifying, and deploying production invariant relations in under 15 minutes.

---

## 1. What Makes Aven Different?

In traditional languages (Python, C, Rust), a system's state is mutated procedurally:
```python
# Fragile procedural code
raw_workers = (reqs * latency) / 1000
allocated_workers = math.ceil(raw_workers / 0.7)
# What if reqs is missing? What if latency is negative? What if a sensor is stale?
```

In Aven, you declare **physical and mathematical relationships** as timeless bidirectional invariants:
```aven
relation ClusterCapacity {
    raw_workers       = (reqs_per_second * avg_latency_ms) / 1000
    allocated_workers = ceil(raw_workers / target_utilization)
    total_ram_needed  = (allocated_workers * (ram_per_worker + socket_buffer_mb)) + os_kernel_ram_mb
}
```

Aven's compiler:
1. **Solves in any direction**: Give it `allocated_workers` and `avg_latency_ms`, and Aven's exact Newton-Raphson solver will solve backwards for maximum allowable `reqs_per_second`.
2. **Enforces dimensional homogeneity**: You cannot accidentally add Megabytes to Volts or subtract Seconds from Watts.
3. **Guarantees data freshness**: Decisions are blocked automatically if incoming sensor telemetry has expired its declared Time-to-Live (TTL).
4. **Maintains a cryptographic ledger**: Every deduction produces an immutable HMAC-SHA256 audit record proving exactly why the decision was made.

---

## 2. Installation & Quick Check

Aven is built with 100% pure standard library Rust (`std`) with zero external dependencies.

```bash
git clone https://git.thrainkrill.space/thrain/aven.git
cd aven
cargo install --path .
```

Verify your installation:
```bash
avenc check examples/server_capacity.aven
avenc run examples/server_capacity.aven
```

---

## 3. Your First Relation Model

Create `thermostat.aven`:
```aven
relation HVACModel {
    temp_delta_c = target_temp_c - current_temp_c
    cooling_power_w = temp_delta_c * thermal_conductivity_w_per_c
    compressor_amps = cooling_power_w / line_voltage_v
}

solve HVACModel with {
    current_temp_c = 28.5
    target_temp_c = 21.0
    thermal_conductivity_w_per_c = 120.0
    line_voltage_v = 240.0
}
```

Run it:
```bash
avenc run thermostat.aven
```

Outputs:
```text
Settled Environment:
  compressor_amps              = -3.7500
  cooling_power_w              = -900.0000
  current_temp_c               = 28.5000
  line_voltage_v               = 240.0000
  target_temp_c                = 21.0000
  temp_delta_c                 = -7.5000
  thermal_conductivity_w_per_c = 120.0000

Fact Ledger: 7 facts settled.
```

---

## 4. Formal SMT Verification

Verify that your system satisfies formal safety envelopes using Z3 or CVC5 via SMT-LIB2:
```bash
avenc emit-smt thermostat.aven --prove
```

Output:
```smt2
; SMT-LIB2 translation for Aven block 'HVACModel'
; Emitted by avenc v0.6.0 -- SMT Formal Verification Engine
; Mode: Prove

(set-logic QF_NRA)
(set-option :produce-models true)

; Variable declarations
(declare-const compressor_amps Real)
(declare-const cooling_power_w Real)
(declare-const current_temp_c Real)
(declare-const line_voltage_v Real)
(declare-const target_temp_c Real)
(declare-const temp_delta_c Real)
(declare-const thermal_conductivity_w_per_c Real)

; Physical relations and invariant equations
(assert (= temp_delta_c (- target_temp_c current_temp_c)))
(assert (= cooling_power_w (* temp_delta_c thermal_conductivity_w_per_c)))
(assert (= compressor_amps (/ cooling_power_w line_voltage_v)))

; Ingress observed bindings
(assert (= current_temp_c 28.5))
(assert (= target_temp_c 21.0))
(assert (= thermal_conductivity_w_per_c 120.0))
(assert (= line_voltage_v 240.0))

(check-sat)
(get-model)
```

---

## 5. WebAssembly (WASM) Deployment

Transpile your model into zero-dependency WebAssembly bytecode for browser dashboards or edge controllers:

```bash
# Emit WebAssembly binary
avenc emit-wasm thermostat.aven -o thermostat.wasm

# Inspect WebAssembly Text format
avenc emit-wasm thermostat.aven --wat
```

Load directly in Node.js or the browser:
```javascript
const fs = require('fs');
const wasmBuffer = fs.readFileSync('thermostat.wasm');

WebAssembly.instantiate(wasmBuffer).then(({ instance }) => {
    // Zero-copy in-memory solve
    console.log("Aven WASM engine loaded successfully");
});
```

---

## 6. Time-Travel Debugging & What-If Exploration

Explore alternative scenarios without running tests again:
```bash
avenc debug thermostat.aven --what-if "current_temp_c=35.0"
```

Aven immediately calculates the counterfactual impact across the dependency graph:
```text
Counterfactual What-If Result (mutating 'current_temp_c' = 35 at t0):
  Output Variable                 Nominal         Branch          Delta
  --------------------------------------------------------------------
  compressor_amps                 -3.7500        -7.0000        -3.2500
  cooling_power_w               -900.0000     -1680.0000      -780.0000
  temp_delta_c                    -7.5000       -14.0000        -6.5000
```
