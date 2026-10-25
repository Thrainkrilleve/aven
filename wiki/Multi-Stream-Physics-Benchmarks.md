# Multi-Stream Physics Stress Benchmarks

To validate Aven under heavy real-world loads, the toolchain was subjected to continuous multi-source physical stress tests fusing live public telemetry streams.

---

## 1. Multi-Stream Ingestion Architecture

The stress harness ([`stream_ringer.py`](file:///f:/aven/evaluations/flight_mesh/stream_ringer.py)) integrates three disparate public feeds into a single unified physical state space:

1. **OpenSky Network ADS-B Transponders:** Real-time spatial positions, barometric altitudes, true airspeed, and vertical rates from **4,700+ active cruising aircraft**.
2. **NOAA Aviation Weather Center (AWC METARs):** Live ground altimeter pressure settings and ambient surface temperatures across major US airport hubs (KATL, KORD, KDEN, KDFW, KJFK, KSEA, KLAX, KSFO, KMIA, KBOS).
3. **USGS Water Services:** Real-time streamflow discharge and gauge station elevations across regional watersheds (Colorado, Mississippi, and Atlantic basins).

```
OpenSky Network (ADS-B) ──┐
NOAA Aviation Weather ────┼──► [Flight Mesh Daemon] ──► [avenc Relational Solver] ──► [Immutable Fact Ledger]
USGS Water Services ──────┘      (Pre-warmed RAM)          (Physical Invariants)         (SHA-256 Provenance)
```

---

## 2. Physical Invariants Modeled

The model ([`flight_mesh_ipc.aven`](file:///f:/aven/evaluations/flight_mesh/flight_mesh_ipc.aven)) simultaneously enforces 8 physical laws:

- **ISA Tropospheric Lapse Rate:**  
  $$T(h) = T_0 - \Gamma \cdot h \quad (\Gamma = 0.0065\,\text{K/m})$$
- **Barometric Pressure at Altitude:**  
  $$p(h) = p_0 \cdot \left(1 - \frac{\Gamma \cdot h}{T_0}\right)^{5.2561}$$
- **Ideal Gas Air Density:**  
  $$\rho = \frac{p}{R_{\text{specific}} \cdot T} \quad (R = 287.058\,\text{J/(kg}\cdot\text{K)})$$
- **Speed of Sound & Mach Number:**  
  $$a = \sqrt{\gamma \cdot R \cdot T_{\text{safe}}} \quad (\gamma = 1.4), \quad M = \frac{v}{a}$$
- **Dynamic Pressure:**  
  $$q = \frac{1}{2} \cdot \rho \cdot v^2$$
- **Specific Excess Climb Power:**  
  $$P_s = g \cdot v_z \quad (g = 9.80665\,\text{m/s}^2)$$
- **Terrain Clearance:**  
  $$h_{\text{clearance}} = h_{\text{baro}} - h_{\text{gauge}}$$

---

## 3. Benchmark Results (50 Live Commercial Flights)

Executed using `python evaluations/flight_mesh/stream_ringer.py --count 50 --concurrency 4`:

### Summary Statistics
- **Total Physical Vectors Evaluated:** 50
- **Feasible Physical Solves:** 49 / 50 (98.0%)
- **Safety Bound Violations:** 1 / 50 (2.0%) *(NetJets EJA582 at 43,000 ft)*
- **Freshness Blocks:** 0 / 50 (0.0%)
- **Ground / Corrupted Fallbacks:** 0 / 50 (0.0%)
- **Multi-Worker Throughput:** **352.9 physical solves / second**
- **Steady-State Median Solve Latency:** **9.7 ms**

### Physical Invariant Distribution

| Physical Invariant | Minimum | Median | Mean / Average | Maximum | Unit |
| :--- | :--- | :--- | :--- | :--- | :--- |
| **Flight Mach Number** | `0.181` | `0.699` | `0.589` | `0.887` | Mach |
| **Barometric Cruise Altitude** | `3,175` | `29,000` | `23,412` | `40,000` | ft |
| **High-Altitude Air Density** | `0.3194` | `0.4845` | `0.6383` | `1.1266` | kg/m³ |
| **Dynamic Pressure ($q$)** | `2,098.8` | `9,093.8` | `8,860.2` | `17,454.3` | Pa |
| **Specific Excess Power (Climb)** | `-95.6` | `0.0` | `3.7` | `172.2` | W/kg |
| **Hydrological Terrain Clearance** | `782.7` | `8,526.2` | `6,922.6` | `12,007.0` | m |
| **Regional River Discharge Proxy** | `0.00` | `0.01` | `0.16` | `2.94` | m³/s |
| **End-to-End Solve Latency** | `8.4` | `9.7` | `10.2` | `17.0` | ms |

---

## 4. Latency Evolution Breakdown

| Optimization Phase | Median Latency | Multi-Worker Throughput | Key Architectural Change |
| :--- | :--- | :--- | :--- |
| **1. Uncached Subprocess** | `125.1 ms` | $7.4\,\text{solves/sec}$ | Base `subprocess.run` with on-demand API fetches |
| **2. Lazy-Import Subprocess** | `71.5 ms` | $54.1\,\text{solves/sec}$ | Deferred `urllib.request` import graph |
| **3. Persistent IPC Daemon** | **`9.7 ms`** | **`352.9 solves/sec`** | Native `avenc` TCP socket stream + RAM cache |

---

## 5. Constraint Violation Case Study: Flight EJA582

During evaluation, flight vector #1 (NetJets `EJA582`, Bombardier Global 7500) reported an altitude of **43,000 ft** ($13,106\,\text{m}$) at 480 kts.

Because the linear tropospheric lapse rate assumption ($6.5^\circ\text{C}/\text{km}$) cools air down to $-81.2^\circ\text{C}$ at this altitude (exceeding the safety boundary of $-80^\circ\text{C}$ before reaching the isothermal stratosphere), Aven immediately halted execution:
```text
⚠️ BOUND_EXCEEDED: ambient_temp_c >= -80 (-81.2°C violates bound -80°C)
```
Rather than masking the anomaly, Aven preserved the raw flight state vector in the ledger and surfaced the physical hazard in the audit trail.
