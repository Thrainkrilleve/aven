-- Migration 0002: Add OpenPLC Industrial Automation Package & Reference Model

INSERT OR IGNORE INTO packages (name, version, description, authors, license, entry, sha256_seal, tarball_size_bytes, downloads, verified_units)
VALUES (
    'ind_openplc',
    '0.2.0',
    'OpenPLC IEC 61131-3 Modbus supervisory relations, scan cycle limits, and thermal invariants',
    'Industrial Automation WG <admin@avenc.dev>',
    'MIT',
    'lib.aven',
    '1e2f3a4b5c6d7e8f9a0b1c2d3e4f5a6b7c8d9e0f1a2b3c4d5e6f7a8b9c0d1e2f',
    3450,
    1180,
    '["w", "ms", "kelvin", "pph", "pct"]'
);

INSERT OR IGNORE INTO shared_models (id, title, category, source_code, sha256, views, settled_facts_count)
VALUES (
    'openplc',
    'OpenPLC Manufacturing Line & Supervisory Safety',
    'industrial',
    '// OpenPLC Manufacturing Line — Modbus supervisory relations and thermal invariants\nrelation OpenPLCSupervisor {\n    scan_ms = 7.5 + (cpu_load_pct * 0.04) + (io_points * 0.08)\n    power_w = 120.0 + (io_points * 2.8) + (cpu_load_pct * 0.75)\n    temp_c = 25.0 + (power_w * 0.085)\n    throughput_pph = target_rate - (scan_ms * 1.85)\n    quality_pct = 99.9 - (scan_ms * 0.065) - ((temp_c - 25.0) * 0.04)\n}\n\nsolve OpenPLCSupervisor with {\n    io_points = 28.0\n    cpu_load_pct = 35.0\n    target_rate = 180.0\n    scan_ms <= 18.0\n    temp_c <= 55.0\n    power_w <= 250.0\n}',
    'e7c2a4f6d8b0e1c3a5f7d9b1e3c5a7f9d1b3e5c7a9f1d3b5e7c9a1f3d5b7e9c1',
    428,
    5
);
