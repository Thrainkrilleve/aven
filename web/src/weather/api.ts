// weather/api.ts — Read-only API behind /weather.

import { WeatherEnv, RAIN_DBZ } from "./cycle";

const json = (body: unknown, headers: Record<string, string>, status = 200) =>
  new Response(JSON.stringify(body), { status, headers: { ...headers, "Cache-Control": "public, max-age=30" } });

const parseRun = (r: any) => ({
  id: r.id, created_ms: r.created_ms, site_kind: r.site_kind, site_label: r.site_label,
  site_lat: r.site_lat, site_lon: r.site_lon, status: r.status, error: r.error, model_sha256: r.model_sha256,
  inputs: r.inputs ? JSON.parse(r.inputs) : null,
  outputs: r.outputs ? JSON.parse(r.outputs) : null,
  sources: r.sources ? JSON.parse(r.sources) : [],
  record: r.record ? JSON.parse(r.record) : null,
});

export async function handleWeatherApi(env: WeatherEnv, url: URL, headers: Record<string, string>): Promise<Response | null> {
  const path = url.pathname;

  // GET /api/v1/weather/latest?limit=N — newest runs with their forecasts
  if (path === "/api/v1/weather/latest") {
    const limit = Math.min(Math.max(parseInt(url.searchParams.get("limit") || "8", 10) || 8, 1), 40);
    const { results } = await env.DB.prepare("SELECT * FROM weather_runs ORDER BY created_ms DESC LIMIT ?").bind(limit).all();
    const runs = results.map(parseRun);
    const ids = runs.map((r) => r.id);
    let forecasts: any[] = [];
    if (ids.length) {
      forecasts = (await env.DB.prepare(
        `SELECT run_id, lead_min, method, issued_ms, valid_ms, predicted_dbz, observed_dbz, radar_ms, status
         FROM weather_forecasts WHERE run_id IN (${ids.map(() => "?").join(",")})`
      ).bind(...ids).all()).results;
    }
    const counts = await env.DB.prepare(
      "SELECT COUNT(*) AS runs, MIN(created_ms) AS first_ms, SUM(status = 'ok') AS ok, SUM(status = 'gated') AS gated, SUM(status = 'error') AS errors FROM weather_runs"
    ).first();
    const cycles = (await env.DB.prepare(
      "SELECT started_ms, task, finished_ms, sites, verified, requests, error FROM weather_cycles ORDER BY started_ms DESC LIMIT 6"
    ).all()).results;
    return json({
      now_ms: Date.now(), totals: counts, cycles,
      runs: runs.map((r) => ({ ...r, forecasts: forecasts.filter((f) => f.run_id === r.id) })),
    }, headers);
  }

  // GET /api/v1/weather/runs/:id — one run in full (for re-running the model in a browser)
  if (path.startsWith("/api/v1/weather/runs/")) {
    const id = Number(path.slice("/api/v1/weather/runs/".length));
    const row = await env.DB.prepare("SELECT * FROM weather_runs WHERE id = ?").bind(id).first();
    return row ? json(parseRun(row), headers) : json({ error: "run not found" }, headers, 404);
  }

  // GET /api/v1/weather/skill?kind=storm_cell|anchor|all — verified forecasts, by lead and method
  if (path === "/api/v1/weather/skill") {
    const kind = url.searchParams.get("kind") || "storm_cell";
    const where = kind === "all" ? "" : "AND site_kind = ?";
    const binds = kind === "all" ? [] : [kind];
    const { results } = await env.DB.prepare(
      `SELECT lead_min, method, COUNT(*) AS n,
              AVG(ABS(predicted_dbz - observed_dbz)) AS mae_dbz,
              AVG(predicted_dbz - observed_dbz) AS bias_dbz,
              SUM(predicted_dbz >= ${RAIN_DBZ} AND observed_dbz >= ${RAIN_DBZ}) AS hits,
              SUM(predicted_dbz < ${RAIN_DBZ} AND observed_dbz >= ${RAIN_DBZ}) AS misses,
              SUM(predicted_dbz >= ${RAIN_DBZ} AND observed_dbz < ${RAIN_DBZ}) AS false_alarms,
              SUM(predicted_dbz < ${RAIN_DBZ} AND observed_dbz < ${RAIN_DBZ}) AS correct_nulls,
              MIN(issued_ms) AS first_ms, MAX(verified_ms) AS last_ms
       FROM weather_forecasts WHERE status = 'verified' ${where}
       GROUP BY lead_min, method ORDER BY lead_min, method`
    ).bind(...binds).all();
    const pending = await env.DB.prepare(`SELECT COUNT(*) AS n FROM weather_forecasts WHERE status = 'pending' ${where}`).bind(...binds).first<{ n: number }>();
    const expired = await env.DB.prepare(`SELECT COUNT(*) AS n FROM weather_forecasts WHERE status = 'expired' ${where}`).bind(...binds).first<{ n: number }>();
    return json({ kind, rain_dbz: RAIN_DBZ, pending: pending?.n ?? 0, expired: expired?.n ?? 0, rows: results }, headers);
  }

  // GET /api/v1/weather/verifications?limit=N — the latest scored forecasts
  if (path === "/api/v1/weather/verifications") {
    const limit = Math.min(Math.max(parseInt(url.searchParams.get("limit") || "60", 10) || 60, 1), 500);
    const { results } = await env.DB.prepare(
      `SELECT f.run_id, f.site_kind, f.lat, f.lon, f.lead_min, f.method, f.issued_ms, f.valid_ms, f.predicted_dbz,
              f.observed_dbz, f.radar_ms, r.site_label
       FROM weather_forecasts f JOIN weather_runs r ON r.id = f.run_id
       WHERE f.status = 'verified' ORDER BY f.verified_ms DESC, f.id DESC LIMIT ?`
    ).bind(limit).all();
    return json({ verifications: results }, headers);
  }

  return null;
}
