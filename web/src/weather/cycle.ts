// weather/cycle.ts — One run of the /weather nowcast (the worker's scheduled job).
//
// Picks sites (the strongest NEXRAD-tracked storm cells, else fixed airports), gathers live
// data for each from seven public sources, runs examples/weather_nowcast.aven compiled to
// WebAssembly, stores inputs, outputs and every request in D1, and checks earlier
// forecasts against the radar once they fall due.

import weatherWasm from "../../public/models/weather.wasm";
import manifest from "../../public/models/manifest.json";
import { AVEN_HOST_IMPORTS } from "../../public/aven_math.js";
import {
  Client, LEADS_MIN, StormCell, aloftAt, displace, fbTable, metarsNear, nwsAlerts, nwsHourly, nwsLatest,
  nwsPoint, offsetM, radarDbz, radarTimes, nearestImage, stormCells, triangle, usgsRain, FbTable, PointInfo, HourlyForecast,
} from "./sources";

export interface WeatherEnv {
  DB: D1Database;
}

const MODEL = manifest.models.find((m) => m.key === "weather")!;
const SITES_PER_RUN = 2;
const FRESH_MS = 75 * 60 * 1000;           // hourly reports stay valid 75 minutes
const CELL_MAX_AGE_MS = 30 * 60 * 1000;
const RECENT_SITE_MS = 20 * 60 * 1000;     // don't revisit a site sooner than this
const MIN_SITE_SPACING_M = 150_000;
const RAIN_DBZ = 20;
const LAYER_DEPTH_M = 1000;

// Fixed sites used when too few storm cells are tracked; a different one each run
export const ANCHORS: [string, number, number][] = [
  ["Denver International (KDEN)", 39.86, -104.67],
  ["Kansas City International (KMCI)", 39.3, -94.71],
  ["Atlanta Hartsfield-Jackson (KATL)", 33.64, -84.43],
  ["Chicago O'Hare (KORD)", 41.98, -87.9],
  ["Dallas/Fort Worth (KDFW)", 32.9, -97.04],
  ["Seattle-Tacoma (KSEA)", 47.45, -122.31],
  ["Minneapolis-St Paul (KMSP)", 44.88, -93.22],
  ["Nashville International (KBNA)", 36.12, -86.68],
];

// The data-quality gate: the bounds in evaluations/possible/weather-predict.aven
export const GATE: [string, (o: Record<string, number>, i: Record<string, number>) => boolean][] = [
  ["rh_residual <= 5", (o) => !Number.isFinite(o.rh_residual) || o.rh_residual <= 5],
  ["dewpoint_depression_c >= -0.5", (o) => o.dewpoint_depression_c >= -0.5],
  ["-80 <= temp_c <= 60", (_o, i) => i.temp_c >= -80 && i.temp_c <= 60],
  ["500 <= pressure_hpa <= 1090", (_o, i) => i.pressure_hpa >= 500 && i.pressure_hpa <= 1090],
];

interface Site {
  kind: "storm_cell" | "anchor";
  label: string;
  lat: number;
  lon: number;
  cell?: StormCell;
}

// ---------------------------------------------------------------------------

let instance: WebAssembly.Instance | null = null;

/** Runs the compiled model: inputs by name in, outputs by name out (NaN stays NaN). */
export async function runModel(values: Record<string, number>): Promise<Record<string, number>> {
  instance ??= await WebAssembly.instantiate(weatherWasm, AVEN_HOST_IMPORTS);
  const exports = instance.exports as any;
  const mem = new Float64Array(exports.memory.buffer);
  MODEL.inputs.forEach((inp, i) => (mem[i] = values[inp.name] ?? NaN));
  exports.aven_solve(0, 1024);
  const out = new Float64Array(exports.memory.buffer, 1024, MODEL.outputs.length);
  return Object.fromEntries(MODEL.outputs.map((name, j) => [name, out[j]]));
}

/** A slowly changing source, kept in D1 for maxAgeMs; a cache hit is recorded as a source too. */
async function cached<T>(env: WeatherEnv, c: Client, source: string, key: string, maxAgeMs: number, load: () => Promise<T>): Promise<T> {
  const row = await env.DB.prepare("SELECT value, fetched_ms FROM weather_cache WHERE key = ?").bind(key).first<{ value: string; fetched_ms: number }>();
  if (row && Date.now() - row.fetched_ms < maxAgeMs) {
    c.records.push({ source, url: key, ok: true, fetched_ms: row.fetched_ms, detail: "cached" });
    return JSON.parse(row.value) as T;
  }
  const value = await load();
  await env.DB.prepare("INSERT OR REPLACE INTO weather_cache (key, value, fetched_ms) VALUES (?, ?, ?)")
    .bind(key, JSON.stringify(value), Date.now()).run();
  return value;
}

const distM = (a: { lat: number; lon: number }, b: { lat: number; lon: number }) => Math.hypot(...offsetM(a.lat, a.lon, b.lat, b.lon));
const inConus = (lat: number, lon: number) => lat > 24.5 && lat < 49.5 && lon > -125 && lon < -66.5;
const finiteOrNull = (o: Record<string, number>) =>
  Object.fromEntries(Object.entries(o).map(([k, v]) => [k, Number.isFinite(v) ? v : null]));

// ---------------------------------------------------------------------------

export async function runCycle(env: WeatherEnv, now = Date.now()): Promise<{ runs: number; verified: number; requests: number }> {
  const c = new Client(45);
  let cells: StormCell[] = [];
  try {
    cells = (await stormCells(c)).filter((x) => now - x.valid_ms < CELL_MAX_AGE_MS);
  } catch (e) {
    console.error("nexrad cells:", e);
  }
  const cellsRecord = c.records[0];
  // Every radar read in this run, and every verification, names the image it reads
  const times = await radarTimes(c);
  const timesRecord = c.records[c.records.length - 1];
  const sites = await chooseSites(env, cells, now);

  let fb: FbTable;
  let runs = 0;
  for (const site of sites) {
    if (c.remaining() < 14) break;
    c.records = [cellsRecord, timesRecord].filter(Boolean);
    try {
      fb = await cached(env, c, "winds_aloft", "fb_winds", 60 * 60 * 1000, () => fbTable(c));
      await forecastSite(env, c, site, cells, fb, times[times.length - 1], now);
    } catch (e: any) {
      await env.DB.prepare(
        "INSERT INTO weather_runs (created_ms, site_kind, site_label, site_lat, site_lon, status, error, sources, model_sha256) VALUES (?, ?, ?, ?, ?, 'error', ?, ?, ?)"
      ).bind(now, site.kind, site.label, site.lat, site.lon, String(e?.message ?? e), JSON.stringify(c.records), MODEL.sha256).run();
    }
    runs++;
  }

  const verified = await verifyDue(env, c, times);
  // Keep full run records for 30 days; forecast/verification rows are kept
  await env.DB.prepare("DELETE FROM weather_runs WHERE created_ms < ?").bind(now - 30 * 86400_000).run();
  return { runs, verified, requests: c.count };
}

async function chooseSites(env: WeatherEnv, cells: StormCell[], now: number): Promise<Site[]> {
  const { results: recent } = await env.DB.prepare("SELECT site_lat AS lat, site_lon AS lon FROM weather_runs WHERE created_ms > ?")
    .bind(now - RECENT_SITE_MS).all<{ lat: number; lon: number }>();
  const chosen: Site[] = [];
  const clear = (p: { lat: number; lon: number }) => [...recent, ...chosen].every((q) => distM(p, q) > MIN_SITE_SPACING_M);
  for (const cell of [...cells].sort((a, b) => b.max_dbz - a.max_dbz)) {
    if (chosen.length === SITES_PER_RUN) break;
    if (cell.max_dbz < 30 || !inConus(cell.lat, cell.lon) || !clear(cell)) continue;
    chosen.push({ kind: "storm_cell", label: `${cell.radar} storm cell ${cell.id}`, lat: round4(cell.lat), lon: round4(cell.lon), cell });
  }
  let turn = Math.floor(now / (5 * 60 * 1000));
  for (let tries = 0; chosen.length < SITES_PER_RUN && tries < ANCHORS.length; tries++, turn++) {
    const [label, lat, lon] = ANCHORS[turn % ANCHORS.length];
    if (clear({ lat, lon })) chosen.push({ kind: "anchor", label, lat, lon });
  }
  return chosen;
}

const round4 = (x: number) => Math.round(x * 1e4) / 1e4;

async function forecastSite(env: WeatherEnv, c: Client, site: Site, cells: StormCell[], fb: FbTable, image_ms: number, now: number) {
  const { lat, lon } = site;
  const point = await cached<PointInfo>(env, c, "nws_point", `point:${lat.toFixed(2)},${lon.toFixed(2)}`, 30 * 86400_000, () => nwsPoint(c, lat, lon));
  if (!point.observationStations) throw new Error("outside NWS land coverage (no observation stations)");

  // Surface stations: every METAR in a box around the site; the primary is the nearest
  // current report with temperature, dewpoint, wind and altimeter
  const metars = (await metarsNear(c, lat, lon)).filter((m) => now - m.obs_ms <= FRESH_MS);
  const primary = metars.find((m) => [m.temp_c, m.dewpoint_c, m.altimeter_hpa, m.u, m.elev_m].every(Number.isFinite));
  if (!primary) throw new Error("no current METAR near the site reports temperature, dewpoint, wind and altimeter");
  const tri = triangle(lat, lon, metars);
  if (!tri) throw new Error("no well-conditioned triangle of wind-reporting METAR stations near the site");

  // The same station through the NWS observation pipeline: reported RH and station pressure
  let nws = null;
  try {
    nws = await nwsLatest(c, primary.id);
    if (now - nws.timestamp_ms > FRESH_MS) nws = null;
  } catch {}

  const aloft = aloftAt(fb, lat, lon, primary.elev_m);
  const cell = site.cell ?? cells.filter((x) => distM(x, site) < 75_000).sort((a, b) => distM(a, site) - distM(b, site))[0];

  // Radar at the site and upstream along each motion: the echo there now arrives at the
  // site after the lead time if it keeps moving with that flow
  const now_radar = await radarDbz(c, lat, lon, image_ms);
  const upstream = async (u: number, v: number) => {
    const reads = await Promise.all(LEADS_MIN.map(async (lead) => {
      const [ulat, ulon] = displace(lat, lon, -u * lead * 60, -v * lead * 60);
      return [lead, { ...(await radarDbz(c, ulat, ulon, image_ms)), lat: ulat, lon: ulon }] as const;
    }));
    return Object.fromEntries(reads) as Record<number, { dbz: number; radar_ms: number; lat: number; lon: number }>;
  };
  const [steerUp, cellUp] = await Promise.all([
    upstream(aloft.steer_u, aloft.steer_v),
    cell ? upstream(cell.u, cell.v) : Promise.resolve(null),
  ]);

  const gauge = await usgsRain(c, lat, lon);
  let alerts: Awaited<ReturnType<typeof nwsAlerts>> = [];
  try {
    alerts = await nwsAlerts(c, lat, lon);
  } catch {}
  let hourly: HourlyForecast[] = [];
  if (point.forecastHourly) {
    try {
      hourly = await cached(env, c, "nws_hourly_forecast", `hourly:${point.forecastHourly}`, 30 * 60 * 1000, () => nwsHourly(c, point.forecastHourly!));
    } catch {}
  }

  const inputs: Record<string, number> = {
    temp_c: primary.temp_c,
    dewpoint_c: primary.dewpoint_c,
    rh_reported: nws?.rh ?? NaN,
    // Station pressure from NWS; without it, the METAR altimeter reduced to the station
    // (set below), and the NWS-vs-METAR pressure check is unavailable
    pressure_hpa: nws && Number.isFinite(nws.pressure_hpa) ? nws.pressure_hpa : NaN,
    nws_pressure_hpa: nws?.pressure_hpa ?? NaN,
    metar_altimeter_hpa: primary.altimeter_hpa,
    station_elev_m: primary.elev_m,
    layer_depth_m: LAYER_DEPTH_M,
    wind_u: primary.u,
    wind_v: primary.v,
    aloft_u: aloft.aloft_u,
    aloft_v: aloft.aloft_v,
    aloft_height_m: aloft.aloft_height_m,
    steer_u: aloft.steer_u,
    steer_v: aloft.steer_v,
    cell_u: cell?.u ?? NaN,
    cell_v: cell?.v ?? NaN,
    radar_dbz: now_radar.dbz,
    gauge_available: gauge ? 1 : 0,
    gauge_rate_mmh: gauge?.rate_mmh ?? 0,
  };
  tri.tri.forEach((s, n) => {
    inputs[`x${n + 1}`] = tri.pts[n][0];
    inputs[`y${n + 1}`] = tri.pts[n][1];
    inputs[`u${n + 1}`] = s.u;
    inputs[`v${n + 1}`] = s.v;
  });
  for (const lead of LEADS_MIN) {
    inputs[`upstream_dbz_${lead}`] = steerUp[lead].dbz;
    inputs[`cell_upstream_dbz_${lead}`] = cellUp?.[lead].dbz ?? NaN;
  }
  const pressureFromNws = Number.isFinite(inputs.pressure_hpa);
  if (!pressureFromNws) {
    // Same reduction the model applies to metar_altimeter_hpa
    inputs.pressure_hpa = primary.altimeter_hpa * ((288.15 - 0.0065 * primary.elev_m) / 288.15) ** 5.2561;
  }

  const outputs = await runModel(inputs);
  const failed = GATE.filter(([, ok]) => !ok(outputs, inputs)).map(([name]) => name);
  const status = failed.length ? "gated" : "ok";

  const oldest = Math.min(primary.obs_ms, now_radar.radar_ms, ...tri.tri.map((s) => s.obs_ms));
  const record = {
    site: { kind: site.kind, label: site.label, lat, lon, place: point.place, nws_radar: point.radarStation },
    stations: {
      primary: primary.id,
      primary_obs_ms: primary.obs_ms,
      triangle: tri.tri.map((s) => ({ id: s.id, lat: s.lat, lon: s.lon, obs_ms: s.obs_ms })),
      nws_observation: nws ? { station: nws.station, timestamp_ms: nws.timestamp_ms } : null,
      pressure_from: pressureFromNws ? "nws_station_pressure" : "metar_altimeter",
      winds_aloft: { station: aloft.fb_station, issued: fb.issued, aloft_ft: aloft.aloft_ft, steer_levels_ft: aloft.steer_levels_ft },
    },
    cell: cell ? { radar: cell.radar, id: cell.id, lat: cell.lat, lon: cell.lon, max_dbz: cell.max_dbz, top_km: cell.top_km,
                   from_deg: cell.from_deg, speed_kt: cell.speed_kt, valid_ms: cell.valid_ms } : null,
    radar: { site_ms: now_radar.radar_ms, steering: steerUp, cell: cellUp },
    gauge,
    alerts,
    hourly,
    gate_failed: failed,
    oldest_data_ms: oldest,
  };

  const res = await env.DB.prepare(
    `INSERT INTO weather_runs (created_ms, site_kind, site_label, site_lat, site_lon, status, error, inputs, outputs, sources, record, model_sha256)
     VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)`
  ).bind(
    now, site.kind, site.label, lat, lon, status, failed.length ? `data-quality gate: ${failed.join("; ")}` : null,
    JSON.stringify(finiteOrNull(inputs)), JSON.stringify(finiteOrNull(outputs)), JSON.stringify(c.records),
    JSON.stringify(record), MODEL.sha256
  ).run();
  if (status !== "ok") return;

  // Forecasts to verify: the steering-wind nowcast, the storm-motion nowcast, and the
  // standard baseline (persistence: the echo at the site stays as it is)
  const runId = res.meta.last_row_id;
  const rows: [number, string, number][] = [];
  for (const lead of LEADS_MIN) {
    rows.push([lead, "steering", inputs[`upstream_dbz_${lead}`]]);
    if (cellUp) rows.push([lead, "cell_motion", inputs[`cell_upstream_dbz_${lead}`]]);
    rows.push([lead, "persistence", inputs.radar_dbz]);
  }
  // Issued at the time of the radar image the forecast is based on: leads count from the data
  const issued = image_ms;
  await env.DB.batch(rows.map(([lead, method, dbz]) =>
    env.DB.prepare(
      "INSERT INTO weather_forecasts (run_id, site_kind, lat, lon, lead_min, method, issued_ms, valid_ms, predicted_dbz) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)"
    ).bind(runId, site.kind, lat, lon, lead, method, issued, issued + lead * 60_000, dbz)
  ));
}

/** Scores due forecasts against the archived radar image nearest each one's valid time:
 *  one read per site and image, shared by the three methods. */
async function verifyDue(env: WeatherEnv, c: Client, times: number[]): Promise<number> {
  const MATCH_MS = 3 * 60 * 1000;
  const latest = times[times.length - 1];
  const { results } = await env.DB.prepare(
    "SELECT DISTINCT lat, lon, valid_ms FROM weather_forecasts WHERE status = 'pending' AND valid_ms <= ? ORDER BY valid_ms LIMIT ?"
  ).bind(latest + MATCH_MS, Math.max(0, c.remaining())).all<{ lat: number; lon: number; valid_ms: number }>();
  let verified = 0;
  for (const { lat, lon, valid_ms } of results) {
    const image = nearestImage(times, valid_ms, MATCH_MS);
    if (image === null) {
      // Too old for the archive, or the nearest image is not out yet
      if (valid_ms < times[0]) {
        await env.DB.prepare("UPDATE weather_forecasts SET status = 'expired' WHERE status = 'pending' AND lat = ? AND lon = ? AND valid_ms = ?")
          .bind(lat, lon, valid_ms).run();
      }
      continue;
    }
    let obs;
    try {
      obs = await radarDbz(c, lat, lon, image);
    } catch {
      continue;
    }
    const r = await env.DB.prepare(
      `UPDATE weather_forecasts SET observed_dbz = ?, radar_ms = ?, verified_ms = ?, status = 'verified'
       WHERE status = 'pending' AND lat = ? AND lon = ? AND valid_ms = ?`
    ).bind(obs.dbz, image, Date.now(), lat, lon, valid_ms).run();
    verified += r.meta.changes ?? 0;
  }
  return verified;
}

export { RAIN_DBZ };
