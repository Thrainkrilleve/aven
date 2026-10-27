# Aven Language Extension for VS Code / OpenVSCode / Antigravity

Official language support for **Aven** (`.aven`), the coupled continuous-discrete physics and systems engineering language.

## Features

- **Syntax Highlighting**:
  - Highlights blocks and declarations (`relation`, `forward`, `solve … with`, `signal`, `unit`, `import`) and backends (`using external(…)`, `ipc(…)`, `shm(…)`, `native(…)`).
  - Highlights units where Aven allows them: after a number in a `solve` block (`250 km/h`) and in `signal` declarations (`signal velocity : km/h`).
  - Builtin functions (`abs`, `ceil`, `floor`, `round`, `sign`, `sqrt`, `exp`, `ln`/`log`, `sin`, `cos`, `tan`) and constants (`pi`, `e`, `tau`).
  - Discrete integer optimization keywords (`int`, `minimize`, `maximize`).
- **Live Diagnostics (Red Squigglies)**:
  - Validates models on save using `avenc check`.
  - Flags syntax and type errors (including dimensional mismatches) on the offending line and column. Under-determined systems show as a warning on the first line of the file.
- **Commands & Context Menus**:
  - Right-click any `.aven` file to:
    - **Aven: Run / Solve Model** (`avenc run`)
    - **Aven: Check Model** (`avenc check`)
    - **Aven: Explain Variable (Causal Why)** (`avenc why`)
    - **Aven: Transpile to C99** (`avenc build --emit-c`)
- **Productivity Snippets**:
  - `relation`: Standard physical relation block.
  - `relation-ipc`: Relation whose facts come from a TCP daemon (`using ipc(...)`).
  - `solve`: Continuous non-linear coupled solve block.
  - `solve-opt`: Mixed-integer branch-and-bound optimization block.
  - `forward`: Directed acyclic compute block.
  - `signal`: Declare the unit a variable is read and shown in.
  - `unit`: Declare units of a new dimension.

## Installation

### In OpenVSCode / Antigravity / VS Code

1. Package as VSIX:
   ```bash
   npx @vscode/vsce package
   ```
2. Or install directly to your local extensions folder:
   - Windows: `%USERPROFILE%\.vscode\extensions\aven-lang-0.1.1`
   - Linux / macOS: `~/.vscode/extensions/aven-lang-0.1.1`
