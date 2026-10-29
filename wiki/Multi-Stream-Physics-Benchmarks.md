# Multi-Stream Physics Stress Benchmarks

To validate Aven under heavy real-world loads, the toolchain was subjected to continuous multi-source physical stress tests fusing live public telemetry streams.

---

## 1. Multi-Stream Ingestion Architecture

The stress harness ([`stream_ringer.py`](https://github.com/Thrainkrilleve/aven/blob/main/evaluations/flight_mesh/stream_ringer.py)) integrates three disparate public feeds into a single unified physical state space:

1. **OpenSky Network ADS-B Transponders:** Real-time spatial positions, barometric altitudes, true airspeed, and vertical rates from the aircraft currently reporting.
2. **NOAA Aviation Weather Center (AWC METARs):** Live ground altimeter pressure settings and ambient surface temperatures across major US airport hubs (KATL, KORD, KDEN, KDFW, KJFK, KSEA, KLAX, KSFO, KMIA, KBOS).
3. **USGS Water Services:** Real-time streamflow discharge and gauge station elevations across regional watersheds (Colorado, Mississippi, and Atlantic basins).

```
OpenSky Network (ADS-B) ──┐
NOAA Aviation Weather ────┼──► [Flight Mesh Daemon] ──► [avenc Relational Solver] ──► [Immutable Fact Ledger]
USGS Water Services ──────┘      (Pre-warmed RAM)          (Physical Invariants)         (SHA-256 Provenance)
```

---

## 2. Physical Invariants Modeled

The model ([`flight_mesh_ipc.aven`](https://github.com/Thrainkrilleve/aven/blob/main/evaluations/flight_mesh/flight_mesh_ipc.aven)) enforces:

- **ISA temperature, troposphere capped at the 11 km tropopause:**  
  $$T(h) = T_0 - \Gamma \cdot \min(h, 11\,000) \quad (\Gamma = 0.0065\,\text{K/m}),\ \text{isothermal above}$$
- **Barometric pressure, with isothermal decay above the tropopause:**  
  $$p(h) = p_0 \left(1 - \frac{\Gamma \min(h, 11\,000)}{T_0}\right)^{5.2561} \exp\!\left(-\frac{g\,(h - \min(h, 11\,000))}{R\,T}\right)$$
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

`verify/live_atmosphere.py` checks temperature, pressure and Mach against an independent two-layer ISA computation on live aircraft (150 checked, 30 above the tropopause, all matching).

---

## 3. Benchmark Results (50 Live Commercial Flights)

> These results were recorded with an **earlier model** that extended the tropospheric lapse rate above 11 km (see §5); the run log is `evaluations/flight_mesh/flight_mesh_cycle_log.jsonl`. Timings were measured on a Windows workstation.

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

| Optimization Phase | Median Latency | Multi-Worker Throughput | Source |
| :--- | :--- | :--- | :--- |
| **1. Uncached Subprocess** | `125.1 ms` | 7.4 solves/sec | Windows run; no log in the repository |
| **2. Lazy-Import Subprocess** | `71.5 ms` | 54.1 solves/sec | Windows run; no log in the repository |
| **3. Persistent IPC Daemon** | `9.7 ms` | 352.9 solves/sec | Windows run; `flight_mesh_summary_report.md` |
| Subprocess (`flight_mesh.aven`), re-run | `70.8 ms` | 51.4 solves/sec | 4-vCPU Linux container, 2026-10-27, 50 live flights |
| IPC daemon (`flight_mesh_ipc.aven`), re-run | `6.2 ms` | 570.9 solves/sec | Same container and date, 50 live flights |

The 2026-10-27 re-runs used live feeds (4,513 cruising flights at the first refresh); their logs and reports are in `evaluations/flight_mesh/runs/2026-10-27-linux/`. Both solved 50/50 flights with **0 bound violations**: the current model no longer produces the §5 error.

---

## 5. Case Study: Flight EJA582 — a Model Error, Since Fixed

During the §3 run, flight vector #1 (NetJets `EJA582`) reported **43,000 ft** ($13,106\,\text{m}$). The model of the time applied the $6.5^\circ\text{C}/\text{km}$ lapse rate all the way up, giving $-81.2^\circ\text{C}$ and tripping the $-80^\circ\text{C}$ bound:
```text
⚠️ BOUND_EXCEEDED: ambient_temp_c >= -80 (-81.2°C violates bound -80°C)
```
The violation was real behaviour of the solver (it refused to settle a state outside the declared bounds), but the cause was the model, not the atmosphere: above 11 km the standard atmosphere is isothermal at about $-56.5^\circ\text{C}$. The model now caps the lapse at the tropopause, and `verify/live_atmosphere.py` confirms it against an independent ISA computation.
