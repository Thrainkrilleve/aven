-- Aven D1 Database Initial Schema (Migration 0001)

CREATE TABLE IF NOT EXISTS packages (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    version TEXT NOT NULL,
    description TEXT,
    authors TEXT,
    license TEXT DEFAULT 'MIT',
    entry TEXT DEFAULT 'lib.aven',
    sha256_seal TEXT NOT NULL,
    tarball_size_bytes INTEGER DEFAULT 0,
    downloads INTEGER DEFAULT 0,
    verified_units TEXT DEFAULT '[]',
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(name, version)
);

CREATE INDEX IF NOT EXISTS idx_packages_name ON packages(name);

CREATE TABLE IF NOT EXISTS shared_models (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    category TEXT DEFAULT 'general',
    source_code TEXT NOT NULL,
    sha256 TEXT NOT NULL,
    views INTEGER DEFAULT 0,
    settled_facts_count INTEGER DEFAULT 0,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS telemetry_benchmarks (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    model_name TEXT NOT NULL,
    solve_time_us REAL NOT NULL,
    facts_settled INTEGER NOT NULL,
    platform TEXT NOT NULL,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP
);

-- Seed Initial Verified Standard Packages
INSERT OR IGNORE INTO packages (name, version, description, authors, license, entry, sha256_seal, tarball_size_bytes, downloads, verified_units)
VALUES 
(
    'std_aerodynamics', 
    '0.2.1', 
    'US Standard Atmosphere 1976, Mach compressibility & aerodynamic drag relations', 
    'Aven Aeronautics WG <aero@avenc.dev>', 
    'MIT', 
    'lib.aven', 
    '7a8b9c1d2e3f4a5b6c7d8e9f0a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b', 
    2048, 
    1420, 
    '["mach", "knots", "slug_ft3", "pa"]'
),
(
    'std_thermodynamics', 
    '0.1.4', 
    'Compressible fluid dynamics, Brayton & Rankine thermodynamic cycle efficiency', 
    'Thermal Systems WG <thermal@avenc.dev>', 
    'Apache-2.0', 
    'lib.aven', 
    '8b9c0d1e2f3a4b5c6d7e8f9a0b1c2d3e4f5a6b7c8d9e0f1a2b3c4d5e6f7a8b9c', 
    1840, 
    895, 
    '["kelvin", "joule", "watt", "bar"]'
),
(
    'fin_markowitz', 
    '0.3.0', 
    'Multi-asset risk-return frontier, cash conservation, and Sharpe ratio maximization', 
    'Quantitative Methods Group <quant@avenc.dev>', 
    'MIT', 
    'lib.aven', 
    '9c0d1e2f3a4b5c6d7e8f9a0b1c2d3e4f5a6b7c8d9e0f1a2b3c4d5e6f7a8b9c0d', 
    2560, 
    2104, 
    '["usd", "bps", "ratio"]'
),
(
    'robotics_kinematics', 
    '0.1.0', 
    'Forward and inverse kinematic link transformations with joint torque invariants', 
    'Robotics & Motion WG <motion@avenc.dev>', 
    'MIT', 
    'lib.aven', 
    '0d1e2f3a4b5c6d7e8f9a0b1c2d3e4f5a6b7c8d9e0f1a2b3c4d5e6f7a8b9c0d1e', 
    3120, 
    640, 
    '["rad", "deg", "newton_meter", "rpm"]'
);

-- Seed Initial Reference Playground Models
INSERT OR IGNORE INTO shared_models (id, title, category, source_code, sha256, views, settled_facts_count)
VALUES
(
    'quadcopter',
    'Quadcopter Avionics & Aerodynamic Thrust',
    'aerospace',
    '// Quadcopter multi-rotor aerodynamic thrust & electrical power\nrelation RotorThrust {\n    thrust_n = 4.0 * thrust_coeff * air_density_kg_m3 * ((rotor_rpm / 60.0) ^ 2) * (prop_diameter_m ^ 4)\n    twr = thrust_n / (total_mass_kg * 9.80665)\n    climb_accel_ms2 = (thrust_n - (total_mass_kg * 9.80665)) / total_mass_kg\n    motor_voltage_v = battery_open_circuit_v - (motor_current_a * battery_internal_resistance_ohm)\n    motor_power_w = motor_voltage_v * motor_current_a\n}\n\nsolve RotorThrust with {\n    thrust_coeff = 0.115\n    air_density_kg_m3 = 1.225\n    rotor_rpm = 6200.0\n    prop_diameter_m = 0.254\n    total_mass_kg = 2.5\n    battery_open_circuit_v = 16.8\n    battery_internal_resistance_ohm = 0.022\n    motor_current_a = 68.0\n}',
    'e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855',
    340,
    19
),
(
    'server_capacity',
    'Little''s Law Server Capacity & Queueing Headroom',
    'systems',
    '// Sizing server capacity with Little''s Law & network wire framing\nrelation WorkerScaling {\n    raw_workers       = (reqs_per_second * avg_latency_ms) / 1000.0\n    allocated_workers = ceil(raw_workers / target_utilization)\n    total_ram_needed  = (allocated_workers * (ram_per_worker + socket_buffer_mb)) + os_kernel_ram_mb\n    payload_gbps      = (reqs_per_second * avg_payload_kb * 8.0) / 1000000.0\n    wire_rate_gbps    = payload_gbps * framing_overhead_ratio\n}\n\nsolve WorkerScaling with {\n    reqs_per_second        = 50000.0\n    avg_latency_ms         = 20.0\n    avg_payload_kb         = 64.0\n    ram_per_worker         = 16.0\n    socket_buffer_mb       = 4.0\n    os_kernel_ram_mb       = 4096.0\n    target_utilization     = 0.70\n    framing_overhead_ratio = 1.04\n}',
    '5a4b3c2d1e0f9a8b7c6d5e4f3a2b1c0d9e8f7a6b5c4d3e2f1a0b9c8d7e6f5a4b',
    512,
    13
);
