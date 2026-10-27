# Aven Language & Toolchain Rules for Antigravity

When working with `.aven` files or developing the `avenc` compiler in Antigravity:

## 1. Language Overview
Aven is a declarative systems modeling language combining continuous non-linear physics solving, compile-time units of measure, mixed-integer branch-and-bound optimization, causal lineage tracking, and real-time IPC sensor fusion.

## 2. Syntax Conventions
- **Relation Blocks:**
  ```aven
  relation PhysicalSizing {
      int drives
      raw_tb = drives * drive_tb
      usable_tb = raw_tb * raid_efficiency
  }
  ```
- **Solve & Optimization Blocks:**
  ```aven
  solve PhysicalSizing with {
      drive_tb = 18.0
      raid_efficiency = 0.75
      usable_tb >= 100.0
      drives <= 12
      minimize(drives)
  }
  ```
- **Live IPC Telemetry:**
  ```aven
  relation OutsideWeather using ipc("127.0.0.1:9090") {
      temp_c = observed_temp_c
  }
  ```

## 3. Toolchain Commands (`avenc`)
- **Validate syntax, units, and rank:**
  `cargo run --release -- check <file.aven>`
- **Solve / settle relations:**
  `cargo run --release -- run <file.aven> [--json] [--ledger out.jsonl]`
- **Explain causal lineage:**
  `cargo run --release -- why <file.aven> <variable> [--depth N]`
- **Generate standalone C99:**
  `cargo run --release -- build <file.aven> --emit-c -o out.c`

## 4. IDE Integration in Antigravity
- Language extension is installed at: `C:\Users\attho\.vscode\extensions\thrain.aven-lang-0.1.0`
- VSIX distribution package is available at: `f:\aven\editors\vscode\aven-lang-0.1.0.vsix`
- Workspace build and solve tasks are configured in: `f:\aven\.vscode\tasks.json`
