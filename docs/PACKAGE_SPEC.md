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

> [!NOTE]
> **Not yet applied.** `avenc` (v0.6.0) reads and preserves `[dependencies]` and `[[units]]`, but does not act on them: dependencies are not fetched or resolved (install each package yourself), and manifest units are not registered with the unit checker. Declare units your relations need with `unit` declarations in the entry file instead.

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
| Footer: SHA-256 of all preceding bytes (64 hex chars) |
+-------------------------------------------------------+
```

`avenc pkg pack` dimension-checks the entry file before sealing, then appends the SHA-256 digest of everything before the footer. `avenc pkg install` recomputes it and refuses an archive whose digest does not match, which detects corruption and accidental edits. The digest is not a signature: anyone who changes an archive can recompute it, so it does not prove who published the package.

---

## 3. Dependency Resolution & Inlining

When an Aven model references external packages:
```aven-fragment
import aerospace_dynamics;

relation MissionFlight {
    dynamic_pressure = 0.5 * air_density * (velocity^2)
}
```

`import <name>;` inlines the package's entry file into the model. The compiler searches, in order:
1. The directory containing the model file (a `<name>/` subdirectory, `<name>.aven`, or an `Aven.toml` whose `name` matches).
2. `vendor/` in the current working directory.
3. `.aven/packages/<name>/<version>/` in the current working directory, where `avenc pkg install` puts packages; the highest installed version wins.

The inlined relations are then dimension-checked together with the model, so unit mismatches across the package boundary are caught before anything runs.
