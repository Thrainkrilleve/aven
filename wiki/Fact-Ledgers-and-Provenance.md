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
    pub kind: String,                   // "input", "derived", "effect" or "committed-effect"
    pub name: String,                   // Variable identifier
    pub value: f64,                     // Settled numerical value
    pub causes: Vec<usize>,             // Sequence numbers of the facts it was derived from
    pub origin: String,                 // e.g. "= y + 1", "ipc: mesh_UAL605 sha256:…"
    pub metadata: Option<FactMetadata>, // Freshness (timestamp, TTL, stale flag, warnings)
}
```

---

## 2. Cryptographic Provenance and Tamper Evidence

All hashing uses Aven's own standard-library SHA-256 (FIPS 180-4 algorithm; not a FIPS-validated module).

- **Payload fingerprints:** every fact received from an external adapter or IPC daemon records `sha256:<hash>` of the adapter's raw output in its origin, so it can be matched against the bytes the adapter emitted.
- **Hash chain:** every appended fact extends a chain, `hash_i = SHA-256(hash_{i-1} ‖ canonical(fact_i))`, starting from 64 zeros. The canonical form covers every field, with the value as its exact bit pattern. Editing, deleting, reordering or inserting a fact changes every later hash. Inside `avenc` the ledger's facts are only reachable read-only.
- **Signing:** with `AVEN_LEDGER_KEY` set, each chain hash is signed with HMAC-SHA256, and the log ends with a signed seal recording the fact count and head hash.
- **Audit log:** `avenc run model.aven --ledger out.jsonl` writes one JSON object per fact (with `prev`, `hash` and, when signed, `sig`) followed by the seal. `avenc verify-ledger out.jsonl` recomputes everything and names the first broken line (exit 1).

What this protects against: with a key, any modification, deletion, insertion, reordering or truncation of the log, even by someone who recomputes the whole chain. Without a key, any partial edit is detected, but someone who rewrites the entire log (chain and seal) cannot be detected — use a key, or record the head hash elsewhere. `verify/ledger_tamper.py` exercises each of these cases.

---

## 3. Freshness & Staleness Protection (`HOLD` Status)

Physical sensors report data at discrete intervals. If a network partition occurs, or an ADS-B transponder drops out, traditional control systems often continue operating on the last-known value (a "ghost reading").

Aven enforces **Time-to-Live (TTL)** on adapter facts:
1. An adapter or daemon may report `timestamp_ms` and `ttl_ms` (e.g. 120,000 ms) alongside its values.
2. When settling, `avenc` checks
   $$\text{now}_{\text{ms}} > \text{timestamp}_{\text{ms}} + \text{ttl}_{\text{ms}}$$
3. If a fact has expired, or the adapter returns `ttl_ms = 0`:
   - Its ledger entry is marked `is_stale: true`, and every fact derived from it carries the warning `causal-stale-taint`.
   - `avenc run` refuses to settle the decision and exits with code 3:
     ```text
     ❌ DECISION BLOCKED: Freshness Enforcement Failure
       The following external fact(s) are STALE (expired TTL or ttl_ms=0):
         • pitot_pressure_pa
         • airspeed_ms

       Refusing to settle a decision on expired inputs.
       Pass --allow-stale to explicitly override in non-production environments.
     ```
   - The AERIC runtime (`aeric/`) reacts to the equivalent condition by latching a `HOLD` state and driving its configured fail-safe setpoints.

---

## 4. Monitor Audit Logs (JSONL)

The evaluation harnesses below write their own per-cycle JSON Lines logs (different from the hash-chained `--ledger` output above):

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

These logs record every input and result of a run for later analysis. Unlike `--ledger` output they are not hash-chained, so they are not tamper-evident.
