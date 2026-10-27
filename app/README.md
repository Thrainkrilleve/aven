# Aven Orchestrator (Tauri)

Desktop app (Tauri v2) for exploring Aven models. The backend links the `avenc` crate directly (`src-tauri/src/main.rs`), so it uses the same parser, solver, unit conversion and ledger as the CLI.

## Structure
```
aven-orchestrator/
├── package.json
├── src-tauri/
│   ├── tauri.conf.json
│   └── src/main.rs         ← Tauri commands backed by the avenc crate
└── dist/
    └── index.html          ← UI (open this directly in a browser to preview)
```

## Preview the UI immediately
```bash
# Just open the HTML
start dist/index.html          # Windows
# or
open dist/index.html           # macOS
```

## Full Tauri setup (when you want the native binary)
```bash
cd aven-orchestrator
npm install
npm install -D @tauri-apps/cli
npx tauri init                 # if needed
npx tauri dev
```

## Tauri commands

`src-tauri/src/main.rs` exposes these commands to the UI: `list_backends`, `test_backend`, `solve_point`, `solve_pareto`, `validate_model`, `get_sample_models`, `get_live_telemetry`, `call_flame`, `explain_why` and `solve_k8s_hpa`.

Building on Linux needs the WebKitGTK and GTK 3 development packages (e.g. `libwebkit2gtk-4.1-dev libgtk-3-dev`); `cargo check` in `src-tauri/` then succeeds.
