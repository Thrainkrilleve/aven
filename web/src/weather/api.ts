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
        `SELECT run_id, lead_min, method, issued_ms, valid_ms, predicted_dbz, nws_pop_pct, observed_dbz, radar_ms, status
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

  // GET /api/v1/weather/skill?kind=storm_cell|anchor|all — who called rain right, by lead.
  // "Rain" is a radar echo of RAIN_DBZ or more at the site at the valid time. Aven's methods
  // call rain when they predict at least RAIN_DBZ; NWS when its chance of precipitation for
  // that hour is 50% or more (scored once per forecast time, from the persistence rows).
  if (path === "/api/v1/weather/skill") {
    const kind = url.searchParams.get("kind") || "storm_cell";
    const where = kind === "all" ? "" : "AND site_kind = ?";
    const binds = kind === "all" ? [] : [kind];
    const R = RAIN_DBZ;
    const { results: aven } = await env.DB.prepare(
      `SELECT lead_min, method, COUNT(*) AS n,
              SUM((predicted_dbz >= ${R}) = (observed_dbz >= ${R})) AS correct,
              SUM(predicted_dbz >= ${R} AND observed_dbz >= ${R}) AS hits,
              SUM(predicted_dbz < ${R} AND observed_dbz >= ${R}) AS misses,
              SUM(predicted_dbz >= ${R} AND observed_dbz < ${R}) AS false_alarms,
              SUM(predicted_dbz < ${R} AND observed_dbz < ${R}) AS correct_nulls,
              AVG(ABS(predicted_dbz - observed_dbz)) AS mae_dbz,
              AVG(predicted_dbz - observed_dbz) AS bias_dbz,
              MIN(issued_ms) AS first_ms
       FROM weather_forecasts WHERE status = 'verified' ${where}
       GROUP BY lead_min, method`
    ).bind(...binds).all();
    const { results: nws } = await env.DB.prepare(
      `SELECT lead_min, 'nws' AS method, COUNT(*) AS n,
              SUM((nws_pop_pct >= 50) = (observed_dbz >= ${R})) AS correct,
              SUM(nws_pop_pct >= 50 AND observed_dbz >= ${R}) AS hits,
              SUM(nws_pop_pct < 50 AND observed_dbz >= ${R}) AS misses,
              SUM(nws_pop_pct >= 50 AND observed_dbz < ${R}) AS false_alarms,
              SUM(nws_pop_pct < 50 AND observed_dbz < ${R}) AS correct_nulls,
              AVG((nws_pop_pct / 100.0 - (observed_dbz >= ${R})) * (nws_pop_pct / 100.0 - (observed_dbz >= ${R}))) AS brier,
              MIN(issued_ms) AS first_ms
       FROM weather_forecasts WHERE status = 'verified' AND method = 'persistence' AND nws_pop_pct IS NOT NULL ${where}
       GROUP BY lead_min`
    ).bind(...binds).all();
    const rows = [...aven, ...nws].sort((a: any, b: any) => a.lead_min - b.lead_min);
    const pending = await env.DB.prepare(`SELECT COUNT(*) AS n FROM weather_forecasts WHERE status = 'pending' AND method = 'persistence' ${where}`).bind(...binds).first<{ n: number }>();
    const expired = await env.DB.prepare(`SELECT COUNT(*) AS n FROM weather_forecasts WHERE status = 'expired' AND method = 'persistence' ${where}`).bind(...binds).first<{ n: number }>();
    return json({ kind, rain_dbz: RAIN_DBZ, nws_rain_pop_pct: 50, pending: pending?.n ?? 0, expired: expired?.n ?? 0, rows }, headers);
  }

  // GET /api/v1/weather/verifications?limit=N — the latest outcomes: one row per forecast
  // time (run and lead) with every forecaster's call and what the radar showed
  if (path === "/api/v1/weather/verifications") {
    const limit = Math.min(Math.max(parseInt(url.searchParams.get("limit") || "30", 10) || 30, 1), 200);
    const { results } = await env.DB.prepare(
      `SELECT f.run_id, f.lead_min, f.method, f.valid_ms, f.predicted_dbz, f.nws_pop_pct, f.observed_dbz, f.radar_ms,
              r.site_label, r.site_kind, r.record
       FROM weather_forecasts f JOIN weather_runs r ON r.id = f.run_id
       WHERE f.status = 'verified' AND (f.run_id, f.lead_min) IN (
         SELECT run_id, lead_min FROM weather_forecasts WHERE status = 'verified' AND method = 'persistence'
         ORDER BY valid_ms DESC LIMIT ?)
       ORDER BY f.valid_ms DESC`
    ).bind(limit).all<any>();
    const outcomes: any[] = [];
    for (const f of results) {
      let o = outcomes.find((x) => x.run_id === f.run_id && x.lead_min === f.lead_min);
      if (!o) {
        let place = "";
        try {
          place = JSON.parse(f.record)?.site?.place ?? "";
        } catch {}
        o = { run_id: f.run_id, lead_min: f.lead_min, valid_ms: f.valid_ms, site_label: f.site_label, site_kind: f.site_kind,
              place, observed_dbz: f.observed_dbz, radar_ms: f.radar_ms, nws_pop_pct: f.nws_pop_pct, predicted: {} };
        outcomes.push(o);
      }
      o.predicted[f.method] = f.predicted_dbz;
    }
    return json({ rain_dbz: RAIN_DBZ, outcomes }, headers);
  }

  return null;
}
