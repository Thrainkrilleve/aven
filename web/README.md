# Aven Website & Edge Services (`avenc.dev`)

Website, in-browser WebAssembly playground, and D1-backed package index for **Aven**.

---

## Architecture & Technology Stack

- **Static site:** Plain HTML, CSS and ES modules in `public/`, served by the platform's static-assets layer. Assets are served *without* running the worker, so response headers for them live in `public/_headers`.
- **Edge worker (TypeScript):** `src/index.ts` handles only paths with no matching file: the `/api/v1/*` routes, `/p/:id` share redirects, and the 404 page.
- **Database:** D1 SQL database (`env.DB`) for the package index, legacy shared-model links, and solve telemetry.
- **Playground:** WebAssembly modules emitted by `avenc emit-wasm` from the `forward` blocks in `examples/`, run client-side. `scripts/build-models.mjs` regenerates them and cross-checks every output against `avenc run`.

---

## Directory Structure

```text
web/
├── wrangler.jsonc               # Worker, static assets, D1 binding, vars
├── package.json                 # Scripts: dev, deploy, build:models, typecheck, db:migrate:*
├── scripts/
│   └── build-models.mjs         # Rebuilds public/models/ from examples/ and verifies them
├── migrations/
│   ├── 0001_initial_schema.sql
│   └── 0002_remove_placeholder_packages.sql
├── src/
│   ├── index.ts                 # API routes and share redirects
│   └── types.ts
└── public/
    ├── index.html               # Home page and playground
    ├── docs.html                # Documentation
    ├── registry.html            # Package index
    ├── 404.html
    ├── styles.css               # Shared design system
    ├── site.js                  # Nav, copy buttons, syntax highlighting, docs TOC
    ├── app.js                   # Playground UI (presentation only)
    ├── wasm_engine.js           # Loads and runs the emitted WebAssembly
    ├── models/                  # *.wasm + manifest.json (generated; do not edit)
    ├── install.sh, install.ps1  # Build-from-source installers (cargo install)
    ├── favicon.svg
    └── _headers                 # Security and cache headers for static assets
```

---

## Updating the playground models

The playground reads input order, output order, source text and defaults from `public/models/manifest.json`, so the page cannot drift out of step with the compiled modules. After changing a `forward` block in `examples/`, or the WASM code generator:

```bash
npm run build:models
```

The script fails if any WASM output differs from `avenc run --json` on the example's own solve block. Slider ranges and labels live in `public/app.js`.

---

## Local Development

```bash
cd web

# 1. Install dependencies
npm install

# 2. Apply initial D1 database schema to local SQLite simulator
npm run db:migrate:local

# 3. Start the local edge development server
npm run dev
```

Visit `http://localhost:8787` to explore the website, manipulate the reactive WASM sliders, and search the D1 package registry.

---

## Production Deployment

The worker is configured for the existing `avenc_db` D1 database (see `wrangler.jsonc`).

```bash
npm run typecheck
npm run db:migrate:prod   # applies pending migrations, including 0002 (removes placeholder packages)
npm run deploy
```

Publishing packages through the API requires a secret; without it, `POST /api/v1/packages` returns 503:

```bash
npx wrangler secret put REGISTRY_PUBLISH_TOKEN
```

The custom domain `avenc.dev` is bound under **Workers & Pages → avenc-web → Settings → Domains & Routes**.

---

## API Endpoints

| Endpoint | Method | Description |
|:---|:---:|:---|
| `/install.sh`, `/install.ps1` | `GET` | Static installers (build from source with `cargo install`). |
| `/p/:id` | `GET` | Legacy share links; redirects to `/?model=:id#playground`. |
| `/api/v1/stats` | `GET` | Row counts from D1 (no synthetic values). |
| `/api/v1/packages` | `GET` | Searches the package index (`?q=`, `?limit=`). |
| `/api/v1/packages/:name` | `GET` | Versions and SHA-256 digests for one package. |
| `/api/v1/packages` | `POST` | Adds a package record. Requires `Authorization: Bearer $REGISTRY_PUBLISH_TOKEN`. |
| `/api/v1/models/share` | `POST` | Saves custom model to D1 and returns permanent shortlink. |
| `/api/v1/models/:id` | `GET` | Retrieves shared model source code. |
| `/api/v1/telemetry` | `POST` | Records solve benchmarks and latency logs. |
