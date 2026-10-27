#!/usr/bin/env node
// weather-local.mjs — Runs the worker's weather nowcast cycle outside Cloudflare.
//
// Bundles src/weather/cycle.ts and src/weather/api.ts (the code the scheduled job and the
// /api/v1/weather routes run) and executes them in Node against a local SQLite file with
// the migrations applied, through a small D1-compatible shim. Useful where `wrangler dev`
// cannot reach the internet, and for checking a change before deploying.
//
// Usage:
//   node scripts/weather-local.mjs cycle [--db weather-local.sqlite]   one scheduled run
//   node scripts/weather-local.mjs api <path> [--db ...]               e.g. /api/v1/weather/skill
//   node scripts/weather-local.mjs serve [--port 8790] [--every 5]     the site and API, running
//                                                                       a cycle every N minutes
// Behind an HTTP proxy, set NODE_USE_ENV_PROXY=1 so fetch uses HTTPS_PROXY.

import { build } from "esbuild";
import { createServer } from "node:http";
import { DatabaseSync } from "node:sqlite";
import { readFileSync, readdirSync, mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve, dirname } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const webDir = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const args = process.argv.slice(2);
const flag = (name, dflt) => {
  const i = args.indexOf(name);
  return i >= 0 ? args[i + 1] : dflt;
};
const command = args[0];
const dbPath = flag("--db", join(webDir, "weather-local.sqlite"));

// --- D1 shim over node:sqlite -------------------------------------------------
const sqlite = new DatabaseSync(dbPath);
sqlite.exec("CREATE TABLE IF NOT EXISTS _migrations (name TEXT PRIMARY KEY)");
for (const file of readdirSync(join(webDir, "migrations")).sort()) {
  if (sqlite.prepare("SELECT 1 FROM _migrations WHERE name = ?").get(file)) continue;
  sqlite.exec(readFileSync(join(webDir, "migrations", file), "utf8"));
  sqlite.prepare("INSERT INTO _migrations (name) VALUES (?)").run(file);
}

class Statement {
  constructor(sql, params = []) {
    this.sql = sql;
    this.params = params;
  }
  bind(...params) {
    return new Statement(this.sql, params);
  }
  async first() {
    return sqlite.prepare(this.sql).get(...this.params) ?? null;
  }
  async all() {
    return { results: sqlite.prepare(this.sql).all(...this.params), success: true };
  }
  async run() {
    const r = sqlite.prepare(this.sql).run(...this.params);
    return { success: true, meta: { changes: Number(r.changes), last_row_id: Number(r.lastInsertRowid) } };
  }
}
const DB = {
  prepare: (sql) => new Statement(sql),
  batch: async (stmts) => Promise.all(stmts.map((s) => s.run())),
};

// --- Bundle the worker's weather code ------------------------------------------
const outdir = mkdtempSync(join(tmpdir(), "aven-weather-"));
await build({
  stdin: {
    contents: 'export { runCycle } from "./src/weather/cycle.ts"; export { handleWeatherApi } from "./src/weather/api.ts";',
    resolveDir: webDir,
    loader: "ts",
  },
  bundle: true,
  format: "esm",
  platform: "node",
  outfile: join(outdir, "weather.mjs"),
  logLevel: "warning",
  plugins: [{
    name: "wasm-module",
    setup(b) {
      // As in a Worker: importing a .wasm file gives a compiled WebAssembly.Module
      b.onLoad({ filter: /\.wasm$/ }, (a) => ({
        contents: `import { readFileSync } from "node:fs"; export default new WebAssembly.Module(readFileSync(${JSON.stringify(a.path)}));`,
        loader: "js",
      }));
    },
  }],
});
const { runCycle, handleWeatherApi } = await import(pathToFileURL(join(outdir, "weather.mjs")).href);
const env = { DB };

if (command === "cycle") {
  const started = Date.now();
  const r = await runCycle(env);
  console.log(`cycle: ${r.runs} site(s), ${r.verified} forecast(s) verified, ${r.requests} requests, ${((Date.now() - started) / 1000).toFixed(1)} s`);
  for (const row of sqlite.prepare("SELECT id, site_label, status, error FROM weather_runs WHERE created_ms >= ? ORDER BY id").all(started - 1000)) {
    console.log(`  run ${row.id}: ${row.site_label} — ${row.status}${row.error ? `: ${row.error}` : ""}`);
  }
} else if (command === "api") {
  const res = await handleWeatherApi(env, new URL(args[1], "http://local"), {});
  console.log(res ? await res.text() : "no such route");
} else if (command === "serve") {
  const port = Number(flag("--port", "8790"));
  const every = Number(flag("--every", "5"));
  const types = { html: "text/html", js: "text/javascript", css: "text/css", json: "application/json", wasm: "application/wasm", svg: "image/svg+xml" };
  createServer(async (req, res) => {
    const url = new URL(req.url, `http://localhost:${port}`);
    if (url.pathname.startsWith("/api/v1/weather/")) {
      const r = await handleWeatherApi(env, url, { "Content-Type": "application/json" });
      res.writeHead(r ? r.status : 404, { "Content-Type": "application/json" });
      return res.end(r ? await r.text() : "{}");
    }
    let path = url.pathname === "/" ? "/index.html" : url.pathname;
    if (!path.includes(".")) path += ".html";
    try {
      const body = readFileSync(join(webDir, "public", path.replace(/\.\.+/g, "")));
      res.writeHead(200, { "Content-Type": types[path.split(".").pop()] ?? "application/octet-stream" });
      res.end(body);
    } catch {
      res.writeHead(404);
      res.end("not found");
    }
  }).listen(port, () => console.log(`http://localhost:${port}/weather (cycle every ${every} min)`));
  const tick = () => runCycle(env).then((r) => console.log(`${new Date().toISOString()} cycle: ${r.runs} site(s), ${r.verified} verified, ${r.requests} requests`)).catch((e) => console.error(e));
  if (every > 0) {
    tick();
    setInterval(tick, every * 60 * 1000);
  }
} else {
  console.error("usage: node scripts/weather-local.mjs cycle|api <path>|serve [--db file]");
  process.exit(2);
}
