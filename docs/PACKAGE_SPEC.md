# Aven Package Specification (`Aven.toml`)

Version: 1.0 (Phase 7 Ecosystem Bootstrap)

This document formally specifies the package manifest, dependency resolution, archive format (`.avenpkg`), and unit-verified relation publishing model for the Aven ecosystem.

---

## 1. Package Manifest Schema (`Aven.toml`)

Every Aven package root contains an `Aven.toml` manifest:

```toml
[package]
name = "aerospace_dynamics"
version = "0.2.1"
description = "US Standard Atmosphere (1976) & Hypersonic Aerodynamic Relations"
authors = ["Aero WG <aero@avenc.dev>"]
license = "Apache-2.0"
entry = "lib.aven"

[dependencies]
std_physics = { path = "../physics" }
numerical_methods = "0.1.0"

[[units]]
name = "kilo_pascal"
dimension = "Pressure"
scale = 1000.0

[[units]]
name = "mach"
dimension = "SpeedRatio"
scale = 1.0
```

### Fields

#### `[package]` Table
- `name` *(required string)*: Namespaced package identifier (alphanumeric and underscores).
- `version` *(required string)*: Semantic Versioning 2.0.0 (`MAJOR.MINOR.PATCH`).
- `description` *(optional string)*: One-line summary of package domain and invariants.
- `authors` *(optional list of strings)*: Maintainer contact info.
- `license` *(optional string)*: SPDX license expression.
- `entry` *(optional string)*: Root Aven source file (defaults to `lib.aven`).

#### `[dependencies]` Table
- Path dependency: `{ path = "relative/or/absolute/path" }`
- Registry dependency: `"version_requirement"` (e.g. `"0.1.0"`)

#### `[[units]]` Table (Repeatable)
- `name`: Identifier for the physical unit in equations (e.g. `kilo_pascal`).
- `dimension`: Base SI dimension or compound algebraic dimension (`Pressure`, `Force`, `Energy`).
- `scale`: Multiplier relative to standard SI base unit.

---

## 2. Package Archive Format (`.avenpkg`)

When packaged via `avenc pkg pack`, Aven creates a portable `.avenpkg` archive:

```text
+-------------------------------------------------------+
| Magic Bytes: "AVENPKG1\n" (9 bytes)                   |
+-------------------------------------------------------+
| Manifest Length (u64 LE, 8 bytes)                     |
+-------------------------------------------------------+
| Manifest Content (UTF-8 TOML)                         |
+-------------------------------------------------------+
| Number of Files (u64 LE, 8 bytes)                     |
+-------------------------------------------------------+
| File 1: Name Len (u64) | Name | Data Len (u64) | Data |
+-------------------------------------------------------+
| File 2: Name Len (u64) | Name | Data Len (u64) | Data |
+-------------------------------------------------------+
| Footer: HMAC/SHA-256 Digest (64 hex characters)      |
+-------------------------------------------------------+
```

Every distribution archive carries an immutable SHA-256 payload digest ensuring supply chain integrity.

---

## 3. Dependency Resolution & Inlining

When an Aven model references external packages:
```aven
import aerospace_dynamics;

relation MissionFlight {
    dynamic_pressure = 0.5 * air_density * (velocity^2)
}
```

The compiler searches:
1. Current working directory or relative path.
2. Local project `vendor/` directory.
3. System cache `.aven/packages/<name>/<version>/`.

During compilation, imported relations are validated against the global unit registry to guarantee dimensional homogeneity across package boundaries before execution.
