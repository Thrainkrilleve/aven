PROGRAM Main
VAR
    // --- Simulated process state ---
    cycle_count     : UDINT := 0;
    run_cmd         : BOOL := TRUE;
    estop           : BOOL := FALSE;

    // --- Outputs / data points for Aven ---
    scan_ms         : REAL := 8.0;      // simulated scan time
    io_points       : INT := 24;        // active I/O count
    power_w         : REAL := 180.0;    // cabinet power
    temp_c          : REAL := 38.0;     // cabinet temperature
    throughput_pph  : REAL := 120.0;    // parts per hour
    alarm_count     : INT := 0;
    energy_kwh      : REAL := 0.0;      // accumulated energy
    cpu_load_pct    : REAL := 35.0;
    network_load_pct: REAL := 12.0;
    quality_pct     : REAL := 99.2;

    // --- Internal ---
    t_scan          : TON;
    noise           : REAL;
    dt_hours        : REAL;
END_VAR

// Simple time base (20 ms task assumed)
cycle_count := cycle_count + 1;
dt_hours := 0.020 / 3600.0;   // 20 ms in hours

// Simulated noise
noise := (cycle_count MOD 17) * 0.01 - 0.08;

// --- Dynamic models ---

// Scan time drifts with load and occasional spikes
scan_ms := 7.5 + (cpu_load_pct * 0.04) + noise * 2.0;
IF (cycle_count MOD 300) = 0 THEN
    scan_ms := scan_ms + 4.0;          // occasional spike
END_IF;
scan_ms := LIMIT(5.0, scan_ms, 25.0);

// I/O count slowly varies
IF (cycle_count MOD 200) = 0 THEN
    io_points := io_points + SEL(cycle_count MOD 400 < 200, -1, 1);
END_IF;
io_points := LIMIT(8, io_points, 64);

// Power follows I/O + base load
power_w := 120.0 + io_points * 2.8 + noise * 15.0;
power_w := LIMIT(100.0, power_w, 450.0);

// Temperature rises with power, cools slowly
temp_c := temp_c + (power_w - 200.0) * 0.0008 - 0.02;
temp_c := LIMIT(25.0, temp_c, 65.0);

// Throughput depends on run state and scan health
IF run_cmd AND NOT estop AND scan_ms < 15.0 THEN
    throughput_pph := 140.0 - (scan_ms * 2.0) + noise * 10.0;
ELSE
    throughput_pph := 0.0;
END_IF;
throughput_pph := LIMIT(0.0, throughput_pph, 200.0);

// Alarms
IF temp_c > 55.0 OR scan_ms > 18.0 THEN
    alarm_count := alarm_count + 1;
ELSIF alarm_count > 0 AND (cycle_count MOD 100) = 0 THEN
    alarm_count := alarm_count - 1;
END_IF;
alarm_count := LIMIT(0, alarm_count, 20);

// Energy accumulation
energy_kwh := energy_kwh + power_w * dt_hours / 1000.0;

// CPU & network load
cpu_load_pct := 25.0 + (io_points * 0.4) + noise * 8.0;
cpu_load_pct := LIMIT(10.0, cpu_load_pct, 95.0);

network_load_pct := 8.0 + (throughput_pph * 0.05) + noise * 5.0;
network_load_pct := LIMIT(0.0, network_load_pct, 80.0);

// Quality degrades slightly with high scan or temperature
quality_pct := 99.8 - (scan_ms * 0.05) - (temp_c - 35.0) * 0.02;
quality_pct := LIMIT(90.0, quality_pct, 100.0);

// Occasional E-stop simulation (very rare)
IF (cycle_count MOD 5000) = 0 THEN
    estop := TRUE;
ELSIF estop AND (cycle_count MOD 100) = 0 THEN
    estop := FALSE;
END_IF;

END_PROGRAM