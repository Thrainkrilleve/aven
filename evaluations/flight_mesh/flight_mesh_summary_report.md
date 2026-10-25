# Aven Multi-Stream Physical Stress Benchmark — OpenSky, NOAA & USGS

**Test Period:** 2026-10-25T15:33:07.818320+00:00 to 2026-10-25T15:33:07.960011+00:00 (0.1s)  
**Telemetry Sources:** OpenSky Network (ADS-B Transponders), NOAA Aviation Weather Center (AWC METARs), USGS Water Services (River Basin Hydrology)  
**Total Physical Vectors Evaluated:** 50  
**Unique Aircraft Tracked:** 50  
**Feasible Physical Solves:** 49 / 50 (98.0%)  
**Safety Bound Violations (Domain / Hazard Enforcement):** 1 / 50 (2.0%)  
**Freshness Blocks (`HOLD` Stale TTL):** 0 / 50 (0.0%)  
**Ground / Corrupted Fallback Tracks:** 0 / 50 (0.0%)  
**Multi-Worker Solve Throughput:** 352.9 physical solves / second  
**Cold-Start Ingestion / Cache Warm Duration:** 3.01 s  
**Steady-State Median Solve Latency:** 9.7 ms  

---

## Core Flight Dynamics & Atmospheric Statistics (Cruising Flights)

| Physical Invariant | Minimum | Median | Mean / Average | Maximum | Unit |
| :--- | :--- | :--- | :--- | :--- | :--- |
| **Flight Mach Number** | 0.181 | 0.699 | 0.589 | 0.887 | Mach |
| **Barometric Cruise Altitude** | 3175 | 29000 | 23412 | 40000 | ft |
| **High-Altitude Air Density** | 0.3194 | 0.4845 | 0.6383 | 1.1266 | kg/m³ |
| **Dynamic Pressure (q)** | 2098.8 | 9093.8 | 8860.2 | 17454.3 | Pa |
| **Specific Excess Power (Climb)** | -95.6 | 0.0 | 3.7 | 172.2 | W/kg |
| **Hydrological Terrain Clearance** | 782.7 | 8526.2 | 6922.6 | 12007.0 | m |
| **Regional Hydro Proxy (Basin Discharge)** | 0.00 | 0.01 | 0.16 | 2.94 | m³/s |
| **End-to-End Solve Latency** | 8.4 | 9.7 | 10.2 | 17.0 | ms |

---

## Sample High-Altitude Aircraft Solves

| Callsign | Altitude | Airspeed | Mach | Air Density | Temp (Alt) | Climb Power | Settle Time | Status |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| **DAL2147** | 35,000 ft (10668m) | 433 kts (222.7m/s) | `M 0.750` | 0.3882 kg/m³ | -53.7°C | +6.4 W/kg | 11.2 ms | `SOLVED` |
| **DAL539** | 34,000 ft (10363m) | 471 kts (242.1m/s) | `M 0.832` | 0.3943 kg/m³ | -62.4°C | +0.0 W/kg | 9.2 ms | `SOLVED` |
| **N454SP** | 11,575 ft (3528m) | 178 kts (91.5m/s) | `M 0.280` | 0.8736 kg/m³ | -6.2°C | +0.0 W/kg | 9.4 ms | `SOLVED` |
| **AAL1691** | 22,125 ft (6744m) | 463 kts (237.9m/s) | `M 0.757` | 0.6166 kg/m³ | -27.1°C | +98.9 W/kg | 9.9 ms | `SOLVED` |
| **N9714X** | 5,100 ft (1554m) | 149 kts (76.6m/s) | `M 0.227` | 1.0492 kg/m³ | +10.5°C | +9.6 W/kg | 9.6 ms | `SOLVED` |
| **SWA800** | 19,425 ft (5921m) | 321 kts (165.2m/s) | `M 0.519` | 0.6662 kg/m³ | -21.3°C | -82.9 W/kg | 10.4 ms | `SOLVED` |
| **WUP657** | 40,000 ft (12192m) | 349 kts (179.4m/s) | `M 0.611` | 0.3194 kg/m³ | -58.6°C | +3.2 W/kg | 8.9 ms | `SOLVED` |
| **DAL2937** | 13,075 ft (3985m) | 360 kts (184.9m/s) | `M 0.568` | 0.8329 kg/m³ | -9.2°C | -35.1 W/kg | 10.0 ms | `SOLVED` |
| **EJA246** | 30,150 ft (9190m) | 416 kts (214.0m/s) | `M 0.703` | 0.4578 kg/m³ | -42.5°C | +73.4 W/kg | 10.6 ms | `SOLVED` |
| **NOAA65** | 9,900 ft (3018m) | 163 kts (83.8m/s) | `M 0.255` | 0.9261 kg/m³ | -4.0°C | +0.0 W/kg | 11.0 ms | `SOLVED` |
| **OKC26** | 29,000 ft (8839m) | 489 kts (251.6m/s) | `M 0.823` | 0.4845 kg/m³ | -40.8°C | +0.0 W/kg | 8.7 ms | `SOLVED` |
| **SWA2323** | 7,800 ft (2377m) | 255 kts (131.0m/s) | `M 0.394` | 0.9668 kg/m³ | +1.7°C | +0.0 W/kg | 8.6 ms | `SOLVED` |
| **DAL898** | 32,000 ft (9754m) | 441 kts (227.0m/s) | `M 0.752` | 0.4276 kg/m³ | -46.2°C | +0.0 W/kg | 9.0 ms | `SOLVED` |
| **DAL607** | 34,000 ft (10363m) | 439 kts (225.8m/s) | `M 0.776` | 0.3943 kg/m³ | -62.4°C | +0.0 W/kg | 9.1 ms | `SOLVED` |
| **SWA3103** | 39,000 ft (11887m) | 435 kts (224.0m/s) | `M 0.766` | 0.3316 kg/m³ | -60.6°C | +0.0 W/kg | 8.4 ms | `SOLVED` |
| **UAL2006** | 36,000 ft (10973m) | 469 kts (241.4m/s) | `M 0.815` | 0.3729 kg/m³ | -54.6°C | +0.0 W/kg | 8.7 ms | `SOLVED` |
| **PDT5868** | 4,400 ft (1341m) | 260 kts (133.9m/s) | `M 0.399` | 1.0966 kg/m³ | +6.9°C | +3.2 W/kg | 8.9 ms | `SOLVED` |
| **N11CE** | 3,175 ft (968m) | 120 kts (61.5m/s) | `M 0.181` | 1.1105 kg/m³ | +14.3°C | +3.2 W/kg | 9.9 ms | `SOLVED` |
| **N992DC** | 4,800 ft (1463m) | 209 kts (107.6m/s) | `M 0.320` | 1.0590 kg/m³ | +7.7°C | -6.4 W/kg | 11.6 ms | `SOLVED` |
| **WJA2032** | 36,000 ft (10973m) | 493 kts (253.6m/s) | `M 0.858` | 0.3735 kg/m³ | -55.7°C | +3.2 W/kg | 17.0 ms | `SOLVED` |

*Generated autonomously by Aven Relational Toolchain.*
