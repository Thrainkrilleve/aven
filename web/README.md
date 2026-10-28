# Aven website (`avenc.dev`)

Static pages and an in-browser solver demo for **Aven**. The homepage demo loads `public/wasm/solver.wasm`, the Newton solver compiled from `solver-wasm/`, and solves `examples/server_capacity.aven` in the browser. Nothing from that demo is sent to a server.

---

## Architecture

- **Static site:** HTML, CSS and ES modules in `public/`, served by the platform's static-assets layer. Response headers for those files live in `public/_headers`.
- **Edge worker (TypeScript):** `src/index.ts` runs only for paths with no matching file: `/api/v1/*`, `/p/:id`, and the 404 page.
- **Database:** D1 (`env.DB`) holds package rows and legacy shared-model rows. The pages do not call the API. Installing a package by name is not implemented; `POST /api/v1/packages` answers 503 until `REGISTRY_PUBLISH_TOKEN` is set.
- **Browser solver:** `solver-wasm/` is a cdylib over `avenc`. `solver-wasm/build.sh` writes `public/wasm/solver.wasm`. `public/solver-demo.js` copies the `WorkerScaling` relation. The repository's `scripts/check_docs.py` fails if those assignments drift from `examples/server_capacity.aven`.

---

## Directory structure

```text
web/
├── wrangler.jsonc          # Worker, static assets, D1 binding, vars
├── package.json            # dev, deploy, typecheck, db:migrate:*
├── migrations/             # D1 schema, applied in order
├── solver-wasm/            # Browser solver (alloc / solve / dealloc)
│   ├── build.sh            # Rebuilds public/wasm/solver.wasm
│   └── src/lib.rs
├── src/
│   ├── index.ts            # API routes and /p/:id redirect
│   └── types.ts, wasm.d.ts
└── public/
    ├── index.html          # Home page and live demo
    ├── docs.html           # Documentation
    ├── 404.html
    ├── solver-demo.js      # Demo UI
    ├── site.js             # Nav, copy buttons, syntax highlighting
    ├── styles.css
    ├── wasm/solver.wasm    # Built by solver-wasm/build.sh
    ├── install.sh, install.ps1
    ├── favicon.svg
    └── _headers
```

---

## Rebuilding the browser solver

Requires the `wasm32-unknown-unknown` target. `wasm-opt` is optional.

```bash
web/solver-wasm/build.sh
```

The script copies the release wasm to `public/wasm/solver.wasm`.

---

## Local development

```bash
cd web
npm install
npm run db:migrate:local
npm run dev                       # http://localhost:8787
```

---

## Production deployment

The worker uses the existing `avenc_db` D1 database (`wrangler.jsonc`).

```bash
npm run typecheck
npm run db:migrate:prod
npm run deploy
```

Publishing through the API requires a secret. Without it, `POST /api/v1/packages` returns 503:

```bash
npx wrangler secret put REGISTRY_PUBLISH_TOKEN
```

The custom domain `avenc.dev` is set by `routes` in `wrangler.jsonc`.

---

## API endpoints

| Endpoint | Method | Description |
|:---|:---:|:---|
| `/install.sh`, `/install.ps1` | `GET` | Static installers (build from source with `cargo install`). |
| `/p/:id` | `GET` | Old share links. Redirects to `/#demo`. The id is not loaded. |
| `/api/v1/stats` | `GET` | Row counts from D1. |
| `/api/v1/packages` | `GET` | Searches package rows (`?q=`, `?limit=`). |
| `/api/v1/packages/:name` | `GET` | Versions and SHA-256 digests for one name. |
| `/api/v1/packages` | `POST` | Adds a package record. Requires `Authorization: Bearer $REGISTRY_PUBLISH_TOKEN`. |
| `/api/v1/models/:id` | `GET` | One legacy shared-model row. |
| `/api/v1/models/share`, `/api/v1/telemetry` | `POST` | Removed. Answer `410 Gone`. |

All `GET` routes also answer `HEAD`.
