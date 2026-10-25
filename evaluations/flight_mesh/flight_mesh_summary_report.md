# Aven Multi-Stream Physical Stress Benchmark — OpenSky, NOAA & USGS

**Test Period:** 2026-10-25T15:16:11.724873+00:00 to 2026-10-25T15:16:13.370176+00:00 (1.6s)  
**Telemetry Sources:** OpenSky Network (ADS-B Transponders), NOAA Aviation Weather Center (AWC METARs), USGS Water Services (River Basin Hydrology)  
**Total Physical Vectors Evaluated:** 50  
**Unique Aircraft Tracked:** 50  
**Feasible Physical Solves:** 49 / 50 (98.0%)  
**Safety Bound Violations (Domain / Hazard Enforcement):** 1 / 50 (2.0%)  
**Freshness Blocks (`HOLD` Stale TTL):** 0 / 50 (0.0%)  
**Ground / Corrupted Fallback Tracks:** 0 / 50 (0.0%)  
**Multi-Worker Solve Throughput:** 30.4 physical solves / second  
**Cold-Start Ingestion / Cache Warm Duration:** 9.10 s  
**Steady-State Median Solve Latency:** 125.1 ms  

---

## Core Flight Dynamics & Atmospheric Statistics (Cruising Flights)

| Physical Invariant | Minimum | Median | Mean / Average | Maximum | Unit |
| :--- | :--- | :--- | :--- | :--- | :--- |
| **Flight Mach Number** | 0.178 | 0.676 | 0.593 | 0.865 | Mach |
| **Barometric Cruise Altitude** | 2625 | 21550 | 22653 | 45000 | ft |
| **High-Altitude Air Density** | 0.2596 | 0.6144 | 0.6595 | 1.1471 | kg/m³ |
| **Dynamic Pressure (q)** | 1895.8 | 9545.6 | 9480.5 | 26627.7 | Pa |
| **Specific Excess Power (Climb)** | -118.0 | 0.0 | -1.7 | 153.1 | W/kg |
| **Hydrological Terrain Clearance** | 100.0 | 6566.4 | 6529.5 | 13403.0 | m |
| **Regional Hydro Proxy (Basin Discharge)** | 0.00 | 0.01 | 0.16 | 2.94 | m³/s |
| **End-to-End Solve Latency** | 117.9 | 125.1 | 126.5 | 138.2 | ms |

---

## Sample High-Altitude Aircraft Solves

| Callsign | Altitude | Airspeed | Mach | Air Density | Temp (Alt) | Climb Power | Settle Time | Status |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| **SWA282** | 33,000 ft (10058m) | 457 kts (235.3m/s) | `M 0.784` | 0.4180 kg/m³ | -48.7°C | +0.0 W/kg | 124.4 ms | `SOLVED` |
| **DAL2147** | 35,025 ft (10676m) | 436 kts (224.1m/s) | `M 0.753` | 0.3871 kg/m³ | -52.7°C | +0.0 W/kg | 125.4 ms | `SOLVED` |
| **DAL539** | 34,000 ft (10363m) | 471 kts (242.1m/s) | `M 0.832` | 0.3943 kg/m³ | -62.4°C | +0.0 W/kg | 122.2 ms | `SOLVED` |
| **SWA800** | 36,000 ft (10973m) | 417 kts (214.4m/s) | `M 0.744` | 0.3637 kg/m³ | -66.3°C | -3.2 W/kg | 122.0 ms | `SOLVED` |
| **RPA4757** | 5,050 ft (1539m) | 255 kts (130.9m/s) | `M 0.391` | 1.0753 kg/m³ | +5.6°C | -67.0 W/kg | 122.6 ms | `SOLVED` |
| **N454SP** | 11,625 ft (3543m) | 179 kts (92.2m/s) | `M 0.282` | 0.8722 kg/m³ | -6.3°C | -3.2 W/kg | 124.9 ms | `SOLVED` |
| **NOAA65** | 9,875 ft (3010m) | 167 kts (85.9m/s) | `M 0.261` | 0.9269 kg/m³ | -4.0°C | +0.0 W/kg | 129.5 ms | `SOLVED` |
| **DAL2937** | 32,000 ft (9754m) | 433 kts (222.7m/s) | `M 0.738` | 0.4339 kg/m³ | -46.7°C | +3.2 W/kg | 129.1 ms | `SOLVED` |
| **OKC26** | 45,000 ft (13716m) | 472 kts (242.7m/s) | `M 0.855` | 0.2596 kg/m³ | -72.5°C | -3.2 W/kg | 129.7 ms | `SOLVED` |
| **WUP657** | 40,000 ft (12192m) | 374 kts (192.4m/s) | `M 0.661` | 0.3186 kg/m³ | -62.5°C | +0.0 W/kg | 138.2 ms | `SOLVED` |
| **DAL898** | 32,050 ft (9769m) | 418 kts (214.8m/s) | `M 0.732` | 0.4260 kg/m³ | -58.5°C | +0.0 W/kg | 121.7 ms | `SOLVED` |
| **DAL1944** | 21,550 ft (6568m) | 422 kts (217.0m/s) | `M 0.676` | 0.6144 kg/m³ | -16.6°C | -118.0 W/kg | 126.6 ms | `SOLVED` |
| **SWA2323** | 31,400 ft (9571m) | 498 kts (256.4m/s) | `M 0.847` | 0.4373 kg/m³ | -45.0°C | -44.6 W/kg | 127.7 ms | `SOLVED` |
| **SKW5080** | 5,761 ft (1756m) | 264 kts (136.1m/s) | `M 0.415` | 1.0738 kg/m³ | -6.4°C | -51.0 W/kg | 123.7 ms | `SOLVED` |
| **RIVER11** | 14,625 ft (4458m) | 503 kts (258.6m/s) | `M 0.800` | 0.7966 kg/m³ | -13.4°C | -60.6 W/kg | 123.6 ms | `SOLVED` |
| **DAL607** | 36,025 ft (10980m) | 438 kts (225.4m/s) | `M 0.782` | 0.3633 kg/m³ | -66.4°C | +0.0 W/kg | 127.8 ms | `SOLVED` |
| **SWA3103** | 39,000 ft (11887m) | 445 kts (228.9m/s) | `M 0.783` | 0.3316 kg/m³ | -60.6°C | +0.0 W/kg | 125.2 ms | `SOLVED` |
| **UAL2006** | 36,000 ft (10973m) | 462 kts (237.9m/s) | `M 0.803` | 0.3729 kg/m³ | -54.6°C | +0.0 W/kg | 122.2 ms | `SOLVED` |
| **AAL1801** | 4,375 ft (1334m) | 274 kts (141.0m/s) | `M 0.420` | 1.0974 kg/m³ | +6.9°C | +0.0 W/kg | 118.3 ms | `SOLVED` |
| **PDT5868** | 18,000 ft (5486m) | 387 kts (199.1m/s) | `M 0.624` | 0.7129 kg/m³ | -20.1°C | +9.6 W/kg | 117.9 ms | `SOLVED` |

*Generated autonomously by Aven Relational Toolchain.*
