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
  nwsPoint, offsetM, radarDbz, radarTimes, nearestImage, stormCells, triangle, usgsRain, FbTable, PointInfo, HourlyForecast, Metar,
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
  ["500 <= pressure_hpa <= 1090", (o) => o.pressure_hpa >= 500 && o.pressure_hpa <= 1090],
];

/** The site lacks the station coverage the model needs; the task tries the next candidate. */
class SiteUnusable extends Error {}

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

/** What one invocation does. The cron triggers split a cycle in three so each stays small
 *  (Workers cap CPU time per invocation; 10 ms on the free plan): one storm-cell site at
 *  minute 0 of every 5, another at minute 2, and scoring at minute 4. */
export interface Task {
  sites: number;
  verify: boolean;
}

export const TASKS_BY_CRON: Record<string, Task> = {
  "*/5 * * * *": { sites: 1, verify: false },
  "2-59/5 * * * *": { sites: 1, verify: false },
  "4-59/5 * * * *": { sites: 0, verify: true },
};

/** Runs one task and records it in weather_cycles: a row is written when the task starts and
 *  completed when it ends, so a task the platform stopped (CPU or time limit) is visible as
 *  started and never finished. */
export async function runCycle(env: WeatherEnv, task: Task = { sites: SITES_PER_RUN, verify: true }, now = Date.now()) {
  const label = [task.sites ? `${task.sites} site(s)` : "", task.verify ? "scoring" : ""].filter(Boolean).join(" + ");
  const started = await env.DB.prepare("INSERT INTO weather_cycles (started_ms, task) VALUES (?, ?)").bind(now, label).run();
  const cycleId = started.meta.last_row_id;
  const c = new Client(45);
  let runs = 0, verified = 0;
  try {
    // Every radar read in this task, and every verification, names the image it reads
    const times = await radarTimes(c);
    const timesRecord = c.records[c.records.length - 1];
    if (task.sites) {
      let cells: StormCell[] = [];
      try {
        cells = (await stormCells(c)).filter((x) => now - x.valid_ms < CELL_MAX_AGE_MS);
      } catch (e) {
        console.error("nexrad cells:", e);
      }
      const cellsRecord = c.records[c.records.length - 1];
      // Up to two spare candidates per site: a storm cell far from enough weather stations
      // is recorded and skipped rather than costing the task its forecast
      let done = 0;
      for (const site of await chooseSites(env, cells, now, task.sites + 2)) {
        if (done === task.sites || c.remaining() < 14) break;
        c.records = [cellsRecord, timesRecord].filter(Boolean);
        try {
          const fb = await cached(env, c, "winds_aloft", "fb_winds", 60 * 60 * 1000, () => fbTable(c));
          await forecastSite(env, c, site, cells, fb, times[times.length - 1], now);
          done++;
        } catch (e: any) {
          if (!(e instanceof SiteUnusable)) done++;
          await env.DB.prepare(
            "INSERT INTO weather_runs (created_ms, site_kind, site_label, site_lat, site_lon, status, error, sources, model_sha256) VALUES (?, ?, ?, ?, ?, 'error', ?, ?, ?)"
          ).bind(now, site.kind, site.label, site.lat, site.lon, String(e?.message ?? e), JSON.stringify(c.records), MODEL.sha256).run();
        }
        runs++;
      }
    }
    if (task.verify) {
      verified = await verifyDue(env, c, times);
      // Keep full run records for 30 days and cycle records for 7; forecasts are kept
      await env.DB.prepare("DELETE FROM weather_runs WHERE created_ms < ?").bind(now - 30 * 86400_000).run();
      await env.DB.prepare("DELETE FROM weather_cycles WHERE started_ms < ?").bind(now - 7 * 86400_000).run();
    }
    await env.DB.prepare("UPDATE weather_cycles SET finished_ms = ?, sites = ?, verified = ?, requests = ? WHERE id = ?")
      .bind(Date.now(), runs, verified, c.count, cycleId).run();
  } catch (e: any) {
    await env.DB.prepare("UPDATE weather_cycles SET finished_ms = ?, sites = ?, verified = ?, requests = ?, error = ? WHERE id = ?")
      .bind(Date.now(), runs, verified, c.count, String(e?.message ?? e), cycleId).run();
    throw e;
  }
  return { runs, verified, requests: c.count };
}

async function chooseSites(env: WeatherEnv, cells: StormCell[], now: number, wanted: number): Promise<Site[]> {
  const { results: recent } = await env.DB.prepare("SELECT site_lat AS lat, site_lon AS lon FROM weather_runs WHERE created_ms > ?")
    .bind(now - RECENT_SITE_MS).all<{ lat: number; lon: number }>();
  const chosen: Site[] = [];
  const clear = (p: { lat: number; lon: number }) => [...recent, ...chosen].every((q) => distM(p, q) > MIN_SITE_SPACING_M);
  for (const cell of [...cells].sort((a, b) => b.max_dbz - a.max_dbz)) {
    if (chosen.length === wanted) break;
    if (cell.max_dbz < 30 || !inConus(cell.lat, cell.lon) || !clear(cell)) continue;
    chosen.push({ kind: "storm_cell", label: `${cell.radar} storm cell ${cell.id}`, lat: round4(cell.lat), lon: round4(cell.lon), cell });
  }
  let turn = Math.floor(now / (5 * 60 * 1000));
  for (let tries = 0; chosen.length < wanted && tries < ANCHORS.length; tries++, turn++) {
    const [label, lat, lon] = ANCHORS[turn % ANCHORS.length];
    if (clear({ lat, lon })) chosen.push({ kind: "anchor", label, lat, lon });
  }
  return chosen;
}

const round4 = (x: number) => Math.round(x * 1e4) / 1e4;

async function forecastSite(env: WeatherEnv, c: Client, site: Site, cells: StormCell[], fb: FbTable, image_ms: number, now: number) {
  const { lat, lon } = site;
  const point = await cached<PointInfo>(env, c, "nws_point", `point:${lat.toFixed(2)},${lon.toFixed(2)}`, 30 * 86400_000, () => nwsPoint(c, lat, lon));
  if (!point.observationStations) throw new SiteUnusable("outside NWS land coverage (no observation stations)");

  // Surface stations: every METAR in a box around the site; the primary is the nearest
  // current report with temperature, dewpoint, wind and altimeter
  const near = await metarsNear(c, lat, lon);
  const metars = near.latest.filter((m) => now - m.obs_ms <= FRESH_MS);
  const primary = metars.find((m) => [m.temp_c, m.dewpoint_c, m.altimeter_hpa, m.u, m.elev_m].every(Number.isFinite));
  if (!primary) throw new SiteUnusable("no current METAR near the site reports temperature, dewpoint, wind and altimeter");
  const tri = triangle(lat, lon, metars);
  if (!tri) throw new SiteUnusable("no well-conditioned triangle of wind-reporting METAR stations near the site");

  // The same station through the NWS observation pipeline: reported RH and station pressure.
  // NWS publishes reports later than the METAR feed, so its latest observation is compared
  // with the METAR issued at the same time (within 5 minutes), from the two hours fetched.
  let nws = null;
  let checkMetar: Metar | null = null;
  let nwsNote = "";
  try {
    const obs = await nwsLatest(c, primary.id);
    const same = (near.history[primary.id] ?? [])
      .filter((m) => Math.abs(m.obs_ms - obs.timestamp_ms) <= 5 * 60 * 1000)
      .sort((a, b) => Math.abs(a.obs_ms - obs.timestamp_ms) - Math.abs(b.obs_ms - obs.timestamp_ms))[0];
    const hhmm = (ms: number) => new Date(ms).toISOString().slice(11, 16);
    if (same) [nws, checkMetar] = [obs, same];
    else nwsNote = `no METAR in the last two hours matches the NWS observation at ${hhmm(obs.timestamp_ms)} UTC`;
  } catch {
    nwsNote = "NWS observation unavailable";
  }

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
    nws_altimeter_hpa: nws?.altimeter_hpa ?? NaN,
    check_temp_c: checkMetar?.temp_c ?? NaN,
    check_dewpoint_c: checkMetar?.dewpoint_c ?? NaN,
    check_altimeter_hpa: checkMetar?.altimeter_hpa ?? NaN,
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
      nws_observation: nws ? { station: nws.station, timestamp_ms: nws.timestamp_ms, metar_ms: checkMetar!.obs_ms } : null,
      nws_note: nwsNote || null,
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
