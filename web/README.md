# Aven Scientific Web Platform & Distributed Edge Services (`avenc.dev`)

High-performance, scientific-facing website, client-side WebAssembly solver playground, and distributed SQL package registry for **Aven**.

---

## Architecture & Technology Stack

- **Edge Runtime:** Distributed Edge Worker (TypeScript) with Static Assets binding (`env.ASSETS`).
- **Database:** Distributed SQL database (`env.DB`) for package metadata, shared playground shortlinks, and solve telemetry logs.
- **Client-Side Solver Engine:** Pure standard library WebAssembly Core 1.0 (`.wasm`) compiled directly by `avenc emit-wasm`—runs Newton-Raphson relaxation client-side in the browser with sub-millisecond execution times and zero backend server costs.
- **Frontend:** Vanilla HTML5 / modern CSS3 (obsidian scientific dark theme, glassmorphism, responsive grids) with Google Fonts (`Inter`, `Outfit`, `JetBrains Mono`).

---

## Directory Structure

```text
web/
├── wrangler.jsonc            # Worker configuration & database bindings
├── package.json              # NPM scripts and devDependencies (wrangler, typescript)
├── tsconfig.json             # TypeScript compiler settings
├── migrations/
│   └── 0001_initial_schema.sql  # D1 schema for packages, models, and telemetry
├── src/
│   ├── index.ts              # Edge worker routing, REST API, install script handlers
│   └── types.ts              # Environment and database record interfaces
└── public/
    ├── index.html            # Scientific hero, live stats, and reactive WASM playground
    ├── docs.html             # Documentation explorer (Quickstart, Cookbook, Specs)
    ├── registry.html         # Interactive D1 package registry browser
    ├── styles.css            # Mathematical / scientific design system
    ├── app.js                # Reactive slider state & D1 model sharing
    ├── wasm_engine.js        # Zero-copy browser WebAssembly execution engine
    ├── models/               # Emitted .wasm binaries (quadcopter, server_capacity, portfolio)
    ├── install.sh            # Universal POSIX installer
    └── install.ps1           # Windows PowerShell installer
```

---

## Local Development

```bash
cd web

# 1. Install dependencies
npm install

# 2. Apply initial D1 database schema to local SQLite simulator
npm run db:migrate:local

# 3. Start local edge development server
npm run dev
```

Visit `http://localhost:8787` to explore the website, manipulate the reactive WASM sliders, and search the D1 package registry.

---

## Production Deployment

Production deployment benefits from high-throughput SQL queries, global edge CDN caching, and low latency.

### Step 1: Create your Production Database

```bash
npx wrangler d1 create avenc_db
```
Wrangler will output your unique `database_id`:
```text
[[d1_databases]]
binding = "DB"
database_name = "avenc_db"
database_id = "xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx"
```

### Step 2: Update `wrangler.jsonc`

Open `wrangler.jsonc` and replace the placeholder `database_id` with your real ID:
```jsonc
"d1_databases": [
  {
    "binding": "DB",
    "database_name": "avenc_db",
    "database_id": "<YOUR_ACTUAL_DATABASE_ID_HERE>",
    "migrations_dir": "./migrations"
  }
]
```

### Step 3: Apply Database Migrations to Remote Database

```bash
npx wrangler d1 migrations apply avenc_db --remote
```

### Step 4: Deploy to Edge

```bash
npm run deploy
```

### Step 5: Bind Custom Domain `avenc.dev`

1. In your edge dashboard, navigate to **Domains & Routes**.
2. Add Custom Domain `avenc.dev`.
3. Edge routing will automatically provision the DNS records and SSL/TLS certificates.

---

## API Endpoints

| Endpoint | Method | Description |
|:---|:---:|:---|
| `/install.sh` | `GET` | Serves dynamic POSIX shell installer with version injection. |
| `/install.ps1` | `GET` | Serves dynamic Windows PowerShell installer. |
| `/p/:id` | `GET` | Permalinks that redirect to `/?model=:id` to load shared models. |
| `/api/v1/stats` | `GET` | Returns aggregated platform metrics from D1. |
| `/api/v1/packages` | `GET` | Searches and lists unit-verified packages from D1. |
| `/api/v1/packages/:name` | `GET` | Retrieves package versions and cryptographic seals. |
| `/api/v1/packages` | `POST` | Publishes new package with SHA-256 seal verification. |
| `/api/v1/models/share` | `POST` | Saves custom model to D1 and returns permanent shortlink. |
| `/api/v1/models/:id` | `GET` | Retrieves shared model source code. |
| `/api/v1/telemetry` | `POST` | Records solve benchmarks and latency logs. |
