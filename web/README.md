# Aven Website & Edge Services (`avenc.dev`)

Website, in-browser WebAssembly playground, live weather nowcast, and D1-backed package index for **Aven**.

---

## Architecture

- **Static site:** Plain HTML, CSS and ES modules in `public/`, served by the platform's static-assets layer. Assets are served *without* running the worker, so response headers for them live in `public/_headers`.
- **Edge worker (TypeScript):** `src/index.ts` handles paths with no matching file (the `/api/v1/*` routes, `/p/:id` share redirects, the 404 page) and the scheduled job.
- **Scheduled job:** every 5 minutes (`triggers.crons` in `wrangler.jsonc`) the worker runs one cycle of the live weather nowcast (`src/weather/`), described below.
- **Database:** D1 (`env.DB`): the package index, legacy shared-model links, and the nowcast's runs, forecasts and cached source data.
- **Models:** WebAssembly modules emitted by `avenc emit-wasm` from `forward` blocks in `examples/`. The playground runs them in the browser; the scheduled job runs the weather model in the worker. `scripts/build-models.mjs` regenerates them and checks every output against `avenc run`.

---

## Directory Structure

```text
web/
├── wrangler.jsonc               # Worker, static assets, D1 binding, cron trigger, vars
├── package.json                 # Scripts: dev, deploy, build:models, typecheck, db:migrate:*
├── scripts/
│   ├── build-models.mjs         # Rebuilds public/models/ from examples/ and verifies them
│   └── weather-local.mjs        # Runs the nowcast cycle, its API and the site in Node
├── migrations/                  # D1 schema, applied in order
├── src/
│   ├── index.ts                 # API routes, share redirects, scheduled handler
│   ├── types.ts, wasm.d.ts
│   └── weather/
│       ├── sources.ts           # One function per live data source
│       ├── cycle.ts             # One scheduled run: sites, data, model, forecasts, scoring
│       └── api.ts               # /api/v1/weather/* (read-only)
└── public/
    ├── index.html               # Home page and playground
    ├── weather.html, weather.js # Live radar nowcast
    ├── docs.html                # Documentation
    ├── registry.html            # Package index
    ├── 404.html
    ├── styles.css               # Shared design system
    ├── site.js                  # Nav, copy buttons, syntax highlighting, docs TOC
    ├── app.js                   # Playground UI (presentation only)
    ├── wasm_engine.js           # Loads and runs the emitted WebAssembly
    ├── aven_math.js             # Host math (exp, ln, trig, pow) for models that import it
    ├── models/                  # *.wasm + manifest.json (generated; do not edit)
    ├── install.sh, install.ps1  # Build-from-source installers (cargo install)
    ├── favicon.svg
    └── _headers                 # Security and cache headers for static assets
```

---

## Updating the models

The playground and the weather job read input order, output order, source text, defaults and each module's SHA-256 from `public/models/manifest.json`, so nothing can drift out of step with the compiled modules. After changing a `forward` block in `examples/` or the WASM code generator:

```bash
npm run build:models
```

The script fails if any WASM output differs from `avenc run --json` on the example's own solve block. Slider ranges and labels live in `public/app.js`.

---

## Live weather nowcast (`/weather`)

Each cycle (`src/weather/cycle.ts`):

1. Reads the NEXRAD storm cells being tracked (Iowa Environmental Mesonet) and the list of NOAA nowCOAST radar images.
2. Picks up to two sites: the strongest cells in the contiguous U.S. not visited in the last 20 minutes, else fixed airports in rotation.
3. For each site, gathers METAR stations in a box around it (AWC), the primary station's NWS observation, winds aloft (AWC, cached an hour), radar reflectivity at the site and upstream along the steering wind and the storm motion (all from one named image), the nearest USGS rain gauge, NWS alerts and the NWS hourly forecast.
4. Runs `models/weather.wasm` (`examples/weather_nowcast.aven`) and applies the data-quality gate (the bounds of `evaluations/possible/weather-predict.aven`).
5. Stores the inputs, outputs and every request in `weather_runs`, and the forecasts (+15, +30, +60 min; steering wind, storm motion, persistence) in `weather_forecasts`.
6. Scores due forecasts against the archived radar image nearest their valid time.

Each cycle makes at most 45 outbound requests (Workers limit subrequests per invocation; the free plan allows 50) and typically about 25.

To run it outside Cloudflare (for instance where `wrangler dev` has no internet access):

```bash
node scripts/weather-local.mjs cycle            # one cycle into weather-local.sqlite
node scripts/weather-local.mjs serve --every 5  # site + API on http://localhost:8790, a cycle every 5 min
node scripts/weather-local.mjs api /api/v1/weather/skill
```

Behind an HTTP proxy, set `NODE_USE_ENV_PROXY=1`.

---

## Local Development

```bash
cd web
npm install
npm run db:migrate:local
npm run dev                       # http://localhost:8787
```

`npx wrangler dev --test-scheduled` also exposes `/__scheduled` to trigger the cron handler.

---

## Production Deployment

The worker is configured for the existing `avenc_db` D1 database (see `wrangler.jsonc`).

```bash
npm run typecheck
npm run db:migrate:prod   # applies pending migrations (0002–0005 as of this release)
npm run deploy
```

Publishing packages through the API requires a secret; without it, `POST /api/v1/packages` returns 503:

```bash
npx wrangler secret put REGISTRY_PUBLISH_TOKEN
```

The custom domain `avenc.dev` is set by `routes` in `wrangler.jsonc`.

---

## API Endpoints

| Endpoint | Method | Description |
|:---|:---:|:---|
| `/install.sh`, `/install.ps1` | `GET` | Static installers (build from source with `cargo install`). |
| `/p/:id` | `GET` | Legacy share links; redirects to `/?model=:id#playground`. |
| `/api/v1/stats` | `GET` | Row counts from D1. |
| `/api/v1/packages` | `GET` | Searches the package index (`?q=`, `?limit=`). |
| `/api/v1/packages/:name` | `GET` | Versions and SHA-256 digests for one package. |
| `/api/v1/packages` | `POST` | Adds a package record. Requires `Authorization: Bearer $REGISTRY_PUBLISH_TOKEN`. |
| `/api/v1/models/:id` | `GET` | A legacy shared model (its category is the playground model). |
| `/api/v1/models/share`, `/api/v1/telemetry` | `POST` | Removed; answer `410 Gone`. |
| `/api/v1/weather/latest` | `GET` | Newest nowcast runs with inputs, outputs, sources and forecasts (`?limit=`, up to 40). |
| `/api/v1/weather/runs/:id` | `GET` | One run in full. |
| `/api/v1/weather/skill` | `GET` | Scores by lead and method: count, MAE, bias, CSI (`?kind=storm_cell\|anchor\|all`). |
| `/api/v1/weather/verifications` | `GET` | The latest scored forecasts (`?limit=`, up to 500). |

All `GET` routes also answer `HEAD`.
