# Aven Orchestrator (Tauri)

Desktop app skeleton that matches the mockup layout.

## Structure
```
aven-orchestrator/
├── package.json
├── src-tauri/
│   └── tauri.conf.json
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

## Next steps to make it real
1. Add a Tauri command in `src-tauri/src/main.rs` that shells out to `flame_eval.py` (or your Rust ExternalEvaluator).
2. Replace the placeholder `testBackend` / `solve` / `pareto` JS functions with `invoke("your_command", { vars })`.
3. Pipe the returned diagnostics into the bottom bar.

The HTML/CSS already follows the mockup’s dark panel layout, so once the Tauri commands are wired you have a native app that looks like the design.
