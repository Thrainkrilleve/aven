# Aven Language Extension for VS Code / OpenVSCode / Antigravity

Official language support for **Aven** (`.aven`), the coupled continuous-discrete physics and systems engineering language.

## Features

- **Syntax Highlighting**:
  - Highlights physical relation blocks (`relation`, `forward`, `solve with`, `external using ipc`).
  - Native visual distinction for compile-time physical units: `[s]`, `[ms]`, `[m]`, `[kW]`, `[TB]`, `[Gbps]`, `[iops]`, etc.
  - Builtin functions (`ceil`, `floor`, `round`, `abs`, `sqrt`, `exp`, `log`, `sin`, `cos`).
  - Discrete integer optimization keywords (`int`, `minimize`, `maximize`).
- **Live Diagnostics (Red Squigglies)**:
  - Validates models on save using `avenc check`.
  - Flags syntax errors, dimensional unit mismatches, and rank-deficient / under-determined systems directly on the offending line and column.
- **Commands & Context Menus**:
  - Right-click any `.aven` file to:
    - **Aven: Run / Solve Model** (`avenc run`)
    - **Aven: Check Model** (`avenc check`)
    - **Aven: Explain Variable (Causal Why)** (`avenc why`)
    - **Aven: Transpile to C99** (`avenc build --emit-c`)
- **Productivity Snippets**:
  - `relation`: Standard physical relation block.
  - `relation-ipc`: Live sensor streaming relation via TCP socket / IPC daemon.
  - `solve`: Continuous non-linear coupled solve block.
  - `solve-opt`: Mixed-integer branch-and-bound optimization block.
  - `forward`: Directed acyclic compute block.
  - `unit`: Custom extensible unit declaration.

## Installation

### In OpenVSCode / Antigravity / VS Code

1. Package as VSIX:
   ```bash
   npx @vscode/vsce package
   ```
2. Or install directly to your local extensions folder:
   - Windows: `%USERPROFILE%\.vscode\extensions\aven-lang-0.1.0`
   - Linux / macOS: `~/.vscode/extensions/aven-lang-0.1.0`
