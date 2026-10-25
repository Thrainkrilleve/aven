# Fact Ledgers, Cryptographic Provenance & Freshness

In autonomous systems, industrial PLCs, and cloud infrastructure, **untraceable mutable state is the primary cause of silent cascading failures**. When a decision is made based on stale sensor data, an unvalidated override, or a transient glitch, debugging after the fact is nearly impossible without an immutable audit trail.

Aven eliminates this hazard through its built-in **Fact Ledger** architecture.

---

## 1. What is a Fact?

In Aven, variables are not mutable memory locations; they are **immutable settled facts**. Once a fact is settled for an evaluation tick, it cannot be silently mutated:

```rust
pub struct Fact {
    pub seq: usize,                     // Global sequence number
    pub tick: usize,                    // Execution tick / cycle index
    pub kind: String,                   // "observed", "derived", or "override"
    pub name: String,                   // Variable identifier
    pub value: f64,                     // Settled numerical value
    pub causes: Vec<String>,            // Causal dependency list
    pub origin: String,                 // Origin source (e.g. "ipc: mesh_UAL605")
    pub metadata: Option<FactMetadata>, // Freshness & cryptographic signature
}
```

---

## 2. Cryptographic Provenance (SHA-256)

Every fact derived from an external sensor, IPC daemon, or relational evaluation carries a cryptographic SHA-256 fingerprint of the raw source payload:
- **Zero External Crates:** Computed natively via Aven's standard-library FIPS 180-4 compliant SHA-256 implementation.
- **Tamper Evidence:** Confirms that the external telemetry received by `avenc` matches the exact bytes emitted by the sensor adapter.

---

## 3. Freshness & Staleness Protection (`HOLD` Status)

Physical sensors report data at discrete intervals. If a network partition occurs, or an ADS-B transponder drops out, traditional control systems often continue operating on the last-known value (a "ghost reading").

Aven natively enforces **Time-to-Live (TTL)**:
1. Every external payload provides `timestamp_ms` and `ttl_ms` (e.g. 120,000 ms).
2. When evaluating relations, `avenc` checks:
   $$\text{now}_{\text{ms}} > \text{timestamp}_{\text{ms}} + \text{ttl}_{\text{ms}}$$
3. If the timestamp has expired, or the adapter explicitly returns `ttl_ms = 0`:
   - The variable is marked `is_stale: true`.
   - The fact ledger appends a warning tag: `stale-telemetry`.
   - Aven halts execution with:
     ```text
     DECISION BLOCKED: The following external fact(s) are STALE (expired TTL):
       - baro_altitude_m (last reported 142s ago, TTL=120s)
     ```
   - Autonomous controllers shift to a safe `HOLD` state rather than actuating on invalid telemetry.

---

## 4. Audit Trail Ledger Schema (JSONL)

When running monitors or batch benchmarks (`overnight_monitor.py`, `stream_ringer.py`), Aven emits an append-only JSON Lines (`.jsonl`) audit log:

```json
{
  "index": 4,
  "time": "15:33:07.835",
  "callsign": "UAL605",
  "baro_altitude_m": 11887.2,
  "velocity_ms": 243.18,
  "vertical_rate_ms": 0.0,
  "ambient_temp_c": -60.57,
  "ambient_pressure_pa": 20207.49,
  "air_density_kg_m3": 0.3311,
  "speed_of_sound_ms": 292.29,
  "mach_number": 0.832,
  "dynamic_pressure_pa": 9791.28,
  "specific_excess_power_w_kg": 0.0,
  "terrain_clearance_m": 11683.2,
  "relative_river_discharge_m3s": 0.0153,
  "elapsed_ms": 15.6,
  "is_blocked": false,
  "is_violated": false,
  "violation_msg": ""
}
```

This log provides complete mathematical reproducibility for post-incident root-cause analysis, FAA/regulatory compliance, and insurance verification.
