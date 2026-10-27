// weather/sources.ts — Live U.S. weather data for the /weather nowcast.
//
// Each function fetches one public source and returns plain numbers; nothing is estimated
// or filled in. A value a source does not report stays NaN and is shown as unavailable.
// The same sources, and the same choices, as evaluations/possible/weather_adapter.py,
// plus NEXRAD storm-cell tracking and NWS alerts and hourly forecasts.

export const UA = {
  "User-Agent": "avenc.dev weather nowcast (https://github.com/Thrainkrilleve/aven, admin@avenc.dev)",
  Accept: "application/geo+json, application/json, text/plain",
};

export const EARTH_M_PER_DEG = 111_320.0;
export const KT_TO_MS = 0.514444;
export const LEADS_MIN = [15, 30, 60] as const;

/** One request to a source: what was asked, when, and whether it answered. */
export interface SourceRecord {
  source: string;
  url: string;
  ok: boolean;
  fetched_ms: number;
  data_ms?: number;
  detail?: string;
}

/** Counts outbound requests (Workers cap them per invocation) and records each one. */
export class Client {
  count = 0;
  records: SourceRecord[] = [];
  constructor(public limit = 45) {}

  remaining(): number {
    return this.limit - this.count;
  }

  async text(source: string, url: string): Promise<string> {
    if (this.count >= this.limit) throw new Error(`request budget of ${this.limit} reached`);
    this.count++;
    const rec: SourceRecord = { source, url, ok: false, fetched_ms: Date.now() };
    this.records.push(rec);
    let res: Response;
    try {
      res = await fetch(url, { headers: UA, signal: AbortSignal.timeout(20_000) });
    } catch (e: any) {
      rec.detail = e?.name === "TimeoutError" ? "no answer within 20 s" : String(e?.message ?? e);
      throw new Error(`${source}: ${rec.detail}`);
    }
    if (!res.ok) {
      rec.detail = `HTTP ${res.status}`;
      throw new Error(`${source}: HTTP ${res.status}`);
    }
    const body = await res.text();
    rec.ok = true;
    return body;
  }

  async json<T = any>(source: string, url: string): Promise<T | null> {
    const body = await this.text(source, url);
    return body.trim() ? (JSON.parse(body) as T) : null;
  }

  /** Marks the most recent record of a source with the time its data describes. */
  stamp(source: string, data_ms: number, detail?: string) {
    const rec = [...this.records].reverse().find((r) => r.source === source);
    if (rec) {
      rec.data_ms = data_ms;
      if (detail) rec.detail = detail;
    }
  }
}

/** East/north offset (m) of (lat, lon) from (lat0, lon0), equirectangular. */
export function offsetM(lat0: number, lon0: number, lat: number, lon: number): [number, number] {
  return [(lon - lon0) * EARTH_M_PER_DEG * Math.cos((lat0 * Math.PI) / 180), (lat - lat0) * EARTH_M_PER_DEG];
}

/** The point (lat, lon) displaced by (dx, dy) metres. */
export function displace(lat: number, lon: number, dx: number, dy: number): [number, number] {
  return [lat + dy / EARTH_M_PER_DEG, lon + dx / (EARTH_M_PER_DEG * Math.cos((lat * Math.PI) / 180))];
}

/** Meteorological convention: the direction the wind (or storm) moves FROM. */
export function windUV(speedMs: number, fromDeg: number): [number, number] {
  const rad = (fromDeg * Math.PI) / 180;
  return [-speedMs * Math.sin(rad), -speedMs * Math.cos(rad)];
}

// ---------------------------------------------------------------------------
// NEXRAD storm cells (NWS Level III storm tracking, served by Iowa Environmental Mesonet)
// ---------------------------------------------------------------------------
// drct is the direction the cell moves FROM, like wind: across 46 cells moving 10 kt or
// more on 2026-10-27, it was within 45 degrees of the 9-18k ft winds-aloft FROM direction
// for 80% of them (median 19 degrees) and never more than 135 degrees off.

export interface StormCell {
  lat: number;
  lon: number;
  radar: string;
  id: string;
  max_dbz: number;
  top_km: number;
  vil: number;
  from_deg: number;
  speed_kt: number;
  u: number;
  v: number;
  valid_ms: number;
}

export async function stormCells(c: Client): Promise<StormCell[]> {
  const doc = await c.json("nexrad_cells", "https://mesonet.agron.iastate.edu/geojson/nexrad_attr.geojson");
  const cells: StormCell[] = [];
  for (const f of doc?.features ?? []) {
    const p = f.properties;
    const [lon, lat] = f.geometry.coordinates;
    const [u, v] = windUV(p.sknt * KT_TO_MS, p.drct);
    cells.push({
      lat, lon, radar: p.nexrad, id: p.storm_id, max_dbz: p.max_dbz, top_km: p.top, vil: p.vil,
      from_deg: p.drct, speed_kt: p.sknt, u, v, valid_ms: Date.parse(p.valid),
    });
  }
  const newest = Math.max(0, ...cells.map((x) => x.valid_ms));
  c.stamp("nexrad_cells", newest, `${cells.length} tracked cells`);
  return cells;
}

// ---------------------------------------------------------------------------
// NWS points, observations, alerts and hourly forecast (api.weather.gov)
// ---------------------------------------------------------------------------

export interface PointInfo {
  forecastHourly: string | null;
  observationStations: string | null;
  radarStation: string | null;
  place: string;
}

export async function nwsPoint(c: Client, lat: number, lon: number): Promise<PointInfo> {
  const p = (await c.json("nws_point", `https://api.weather.gov/points/${lat.toFixed(4)},${lon.toFixed(4)}`))?.properties ?? {};
  const rel = p.relativeLocation?.properties;
  return {
    forecastHourly: p.forecastHourly ?? null,
    observationStations: p.observationStations ?? null,
    radarStation: p.radarStation ?? null,
    place: rel ? `${rel.city}, ${rel.state}` : "",
  };
}

export interface NwsObservation {
  station: string;
  timestamp_ms: number;
  temp_c: number;
  dewpoint_c: number;
  rh: number;
  /** NWS "barometricPressure": the altimeter setting (sea-level reduced), not station pressure */
  altimeter_hpa: number;
}

const num = (v: any): number => (typeof v === "number" && Number.isFinite(v) ? v : NaN);

export async function nwsLatest(c: Client, station: string): Promise<NwsObservation> {
  const p = (await c.json("nws_observation", `https://api.weather.gov/stations/${station}/observations/latest`))?.properties ?? {};
  const val = (k: string) => num(p[k]?.value);
  const obs = {
    station,
    timestamp_ms: Date.parse(p.timestamp),
    temp_c: val("temperature"),
    dewpoint_c: val("dewpoint"),
    rh: val("relativeHumidity"),
    altimeter_hpa: val("barometricPressure") / 100,
  };
  c.stamp("nws_observation", obs.timestamp_ms, station);
  return obs;
}

export interface Alert {
  event: string;
  severity: string;
  headline: string;
}

export async function nwsAlerts(c: Client, lat: number, lon: number): Promise<Alert[]> {
  const doc = await c.json("nws_alerts", `https://api.weather.gov/alerts/active?point=${lat.toFixed(4)},${lon.toFixed(4)}`);
  const alerts = (doc?.features ?? []).map((f: any) => ({
    event: f.properties.event, severity: f.properties.severity, headline: f.properties.headline ?? "",
  }));
  c.stamp("nws_alerts", Date.now(), `${alerts.length} active`);
  return alerts;
}

export interface HourlyForecast {
  start_ms: number;
  pop_pct: number;
  short: string;
}

/** The NWS hourly forecast periods covering the next `hours` hours. */
export async function nwsHourly(c: Client, url: string, hours = 2): Promise<HourlyForecast[]> {
  const doc = await c.json("nws_hourly_forecast", url);
  const periods = (doc?.properties?.periods ?? []).slice(0, hours).map((p: any) => ({
    start_ms: Date.parse(p.startTime),
    pop_pct: num(p.probabilityOfPrecipitation?.value ?? 0),
    short: p.shortForecast ?? "",
  }));
  c.stamp("nws_hourly_forecast", Date.parse(doc?.properties?.updateTime ?? doc?.properties?.generatedAt ?? ""), periods[0]?.short);
  return periods;
}

// ---------------------------------------------------------------------------
// METAR (NOAA Aviation Weather Center): every station in a box around the site
// ---------------------------------------------------------------------------

export interface Metar {
  id: string;
  lat: number;
  lon: number;
  elev_m: number;
  obs_ms: number;
  temp_c: number;
  dewpoint_c: number;
  altimeter_hpa: number;
  u: number;
  v: number;
}

/** Every METAR in a box around the site over the last two hours: `latest` holds each
 *  station's newest report (nearest station first), `history` all reports by station. */
export async function metarsNear(c: Client, lat: number, lon: number, half = 0.9): Promise<{ latest: Metar[]; history: Record<string, Metar[]> }> {
  const url = `https://aviationweather.gov/api/data/metar?bbox=${(lat - half).toFixed(2)},${(lon - half).toFixed(2)},${(lat + half).toFixed(2)},${(lon + half).toFixed(2)}&format=json&hours=2`;
  const rows = (await c.json<any[]>("metar", url)) ?? [];
  const out: Metar[] = [];
  for (const r of rows) {
    // Variable ("VRB") or missing direction gives no wind vector; calm is (0, 0)
    let u = NaN, v = NaN;
    const spd = num(r.wspd);
    if (spd === 0) [u, v] = [0, 0];
    else if (Number.isFinite(spd) && typeof r.wdir === "number") [u, v] = windUV(spd * KT_TO_MS, r.wdir);
    out.push({
      id: r.icaoId, lat: r.lat, lon: r.lon, elev_m: num(r.elev), obs_ms: num(r.obsTime) * 1000,
      temp_c: num(r.temp), dewpoint_c: num(r.dewp), altimeter_hpa: num(r.altim), u, v,
    });
  }
  const history: Record<string, Metar[]> = {};
  for (const m of out) (history[m.id] ??= []).push(m);
  const latest = Object.values(history).map((h) => h.reduce((a, b) => (b.obs_ms > a.obs_ms ? b : a)));
  latest.sort((a, b) => Math.hypot(...offsetM(lat, lon, a.lat, a.lon)) - Math.hypot(...offsetM(lat, lon, b.lat, b.lon)));
  c.stamp("metar", Math.max(0, ...latest.map((m) => m.obs_ms || 0)), `${latest.length} stations, ${out.length} reports`);
  return { latest, history };
}

/** Three wind-reporting stations whose triangle is well conditioned for a plane fit,
 *  preferring triangles that contain the site and are small (as weather_adapter.py). */
export function triangle(lat: number, lon: number, stations: Metar[]): { tri: Metar[]; pts: [number, number][] } | null {
  const cands = stations.filter((s) => Number.isFinite(s.u)).slice(0, 10);
  let best: { score: number; tri: Metar[]; pts: [number, number][] } | null = null;
  const d = (p: number[], q: number[], r: number[]) => (q[0] - p[0]) * (r[1] - p[1]) - (r[0] - p[0]) * (q[1] - p[1]);
  for (let i = 0; i < cands.length; i++)
    for (let j = i + 1; j < cands.length; j++)
      for (let k = j + 1; k < cands.length; k++) {
        const tri = [cands[i], cands[j], cands[k]];
        const pts = tri.map((t) => offsetM(lat, lon, t.lat, t.lon));
        const [[x1, y1], [x2, y2], [x3, y3]] = pts;
        const area = Math.abs((x2 - x1) * (y3 - y1) - (x3 - x1) * (y2 - y1)) / 2;
        const sides = [Math.hypot(x2 - x1, y2 - y1), Math.hypot(x3 - x2, y3 - y2), Math.hypot(x3 - x1, y3 - y1)];
        const longest = Math.max(...sides);
        if (area < 1e7 || longest > 150_000) continue;
        const s1 = d(pts[0], pts[1], [0, 0]), s2 = d(pts[1], pts[2], [0, 0]), s3 = d(pts[2], pts[0], [0, 0]);
        const inside = (s1 >= 0 && s2 >= 0 && s3 >= 0) || (s1 <= 0 && s2 <= 0 && s3 <= 0);
        const score = ((area / longest ** 2) * (inside ? 2 : 1)) / longest;
        if (!best || score > best.score) best = { score, tri, pts };
      }
  return best ? { tri: best.tri, pts: best.pts } : null;
}

// ---------------------------------------------------------------------------
// FB winds aloft (NOAA Aviation Weather Center)
// ---------------------------------------------------------------------------

export interface FbTable {
  issued: string;
  rows: Record<string, Record<number, string>>;
  coords: Record<string, [number, number]>;
}

export async function fbTable(c: Client): Promise<FbTable> {
  const text = await c.text("winds_aloft", "https://aviationweather.gov/api/data/windtemp?region=all&level=low&fcst=06");
  const lines = text.split(/\r?\n/);
  const head = lines.findIndex((l) => l.startsWith("FT"));
  const issued = lines.find((l) => /DATA BASED ON/.test(l))?.trim() ?? "";
  // Fixed-width table: each group is right-aligned to the end of its level header
  const levels = [...lines[head].matchAll(/\d+/g)].map((m) => [Number(m[0]), m.index! + m[0].length]);
  const rows: FbTable["rows"] = {};
  for (const l of lines.slice(head + 1)) {
    if (!/^[A-Z0-9]{3}\b/.test(l)) continue;
    const groups: Record<number, string> = {};
    let prev = 3;
    for (const [ft, end] of levels) {
      const field = l.slice(prev, end).trim();
      prev = end;
      if (field.length >= 4 && /^\d{4}/.test(field)) groups[ft] = field.slice(0, 4);
    }
    rows[l.slice(0, 3)] = groups;
  }
  const ids = Object.keys(rows).map((s) => "K" + s).join(",");
  const info = (await c.json<any[]>("winds_aloft_stations", `https://aviationweather.gov/api/data/stationinfo?ids=${ids}&format=json`)) ?? [];
  const coords: FbTable["coords"] = {};
  for (const s of info) if (s.icaoId) coords[s.icaoId.slice(1)] = [s.lat, s.lon];
  return { issued, rows, coords };
}

export interface Aloft {
  fb_station: string;
  aloft_ft: number;
  aloft_u: number;
  aloft_v: number;
  aloft_height_m: number;
  steer_u: number;
  steer_v: number;
  steer_levels_ft: number[];
}

export function aloftAt(fb: FbTable, lat: number, lon: number, siteElevM: number): Aloft {
  const near = Object.keys(fb.rows)
    .filter((s) => fb.coords[s])
    .sort((a, b) => Math.hypot(...offsetM(lat, lon, ...fb.coords[a])) - Math.hypot(...offsetM(lat, lon, ...fb.coords[b])))[0];
  if (!near) throw new Error("no winds-aloft station with coordinates");
  const decode = (g: string): [number, number] => {
    let dd = Number(g.slice(0, 2)), ss = Number(g.slice(2, 4));
    if (dd === 99 && ss === 0) return [0, 0]; // light and variable
    if (dd > 36) [dd, ss] = [dd - 50, ss + 100]; // 100 kt or more
    return windUV(ss * KT_TO_MS, dd * 10);
  };
  const winds: Record<number, [number, number]> = {};
  for (const [ft, g] of Object.entries(fb.rows[near])) winds[Number(ft)] = decode(g);
  const above = Object.keys(winds).map(Number).filter((ft) => ft * 0.3048 > siteElevM + 300).sort((a, b) => a - b);
  if (!above.length) throw new Error(`winds-aloft station ${near} has no level above the site`);
  const low = above[0];
  let steer = Object.keys(winds).map(Number).filter((ft) => ft >= 9000 && ft <= 18000);
  if (!steer.length) steer = above.slice(0, 2);
  const su = steer.reduce((s, f) => s + winds[f][0], 0) / steer.length;
  const sv = steer.reduce((s, f) => s + winds[f][1], 0) / steer.length;
  return {
    fb_station: near, aloft_ft: low, aloft_u: winds[low][0], aloft_v: winds[low][1],
    aloft_height_m: low * 0.3048 - siteElevM, steer_u: su, steer_v: sv, steer_levels_ft: steer,
  };
}

// ---------------------------------------------------------------------------
// NOAA nowCOAST base reflectivity mosaic and USGS precipitation gauges
// ---------------------------------------------------------------------------

const RADAR_WMS = "https://nowcoast.noaa.gov/geoserver/observations/weather_radar";
const RADAR_LAYER = "conus_base_reflectivity_mosaic";

/** Times of the radar mosaic images nowCOAST holds (about every 4 minutes, several hours
 *  back), oldest first. The last one is the latest image. */
export async function radarTimes(c: Client): Promise<number[]> {
  const xml = await c.text("radar_times", `${RADAR_WMS}/${RADAR_LAYER}/ows?service=WMS&version=1.3.0&request=GetCapabilities`);
  const dim = xml.match(/<Dimension name="time"[^>]*>([^<]*)</);
  const times = (dim?.[1] ?? "").split(",").map((t) => Date.parse(t.trim())).filter(Number.isFinite).sort((a, b) => a - b);
  if (!times.length) throw new Error("radar capabilities list no image times");
  c.stamp("radar_times", times[times.length - 1], `${times.length} images`);
  return times;
}

/** Reflectivity (dBZ, 0 for no echo) at a point in the radar image taken at `image_ms`. */
export async function radarDbz(c: Client, lat: number, lon: number, image_ms: number): Promise<{ dbz: number; radar_ms: number }> {
  const d = 0.005;
  const url =
    `${RADAR_WMS}/ows?service=WMS&version=1.3.0&request=GetFeatureInfo&layers=${RADAR_LAYER}&query_layers=${RADAR_LAYER}` +
    `&crs=EPSG:4326&bbox=${lat - d},${lon - d},${lat + d},${lon + d}&width=3&height=3&i=1&j=1&info_format=application/json` +
    `&time=${new Date(image_ms).toISOString()}`;
  const doc = await c.json("radar", url);
  const vals = (doc?.features ?? []).flatMap((f: any) => Object.values(f.properties ?? {})).filter((v: any) => typeof v === "number");
  if (!vals.length) throw new Error(`no radar value at ${lat.toFixed(3)},${lon.toFixed(3)}`);
  c.stamp("radar", image_ms);
  return { dbz: Math.max(0, vals[0] as number), radar_ms: image_ms };
}

/** The image time nearest `t`, or null if none is within `maxMs`. */
export function nearestImage(times: number[], t: number, maxMs: number): number | null {
  let best: number | null = null;
  for (const x of times) if (best === null || Math.abs(x - t) < Math.abs(best - t)) best = x;
  return best !== null && Math.abs(best - t) <= maxMs ? best : null;
}

export interface Gauge {
  site: string;
  rate_mmh: number;
  time_ms: number;
  distance_m: number;
}

/** Most recent precipitation increment from the nearest USGS gauge within ~25 km. */
export async function usgsRain(c: Client, lat: number, lon: number): Promise<Gauge | null> {
  const d = 0.25;
  const url =
    "https://waterservices.usgs.gov/nwis/iv/?format=json&parameterCd=00045&siteStatus=active&period=PT3H" +
    `&bBox=${(lon - d).toFixed(3)},${(lat - d).toFixed(3)},${(lon + d).toFixed(3)},${(lat + d).toFixed(3)}`;
  let series: any[] = [];
  try {
    series = (await c.json("usgs_gauges", url))?.value?.timeSeries ?? [];
  } catch {
    return null; // gauges are optional; the request is recorded as failed
  }
  let best: Gauge | null = null;
  for (const s of series) {
    const loc = s.sourceInfo.geoLocation.geogLocation;
    const dist = Math.hypot(...offsetM(lat, lon, loc.latitude, loc.longitude));
    const vals = (s.values?.[0]?.value ?? []).filter((v: any) => v.value !== "" && v.value != null && Number(v.value) >= 0);
    if (vals.length < 2) continue;
    const [a, b] = vals.slice(-2);
    const minutes = (Date.parse(b.dateTime) - Date.parse(a.dateTime)) / 60000;
    if (minutes <= 0) continue;
    const rate = (Number(b.value) * 25.4 * 60) / minutes; // inches per interval -> mm/h
    if (!best || dist < best.distance_m) best = { site: s.sourceInfo.siteCode[0].value, rate_mmh: rate, time_ms: Date.parse(b.dateTime), distance_m: dist };
  }
  c.stamp("usgs_gauges", best?.time_ms ?? Date.now(), best ? `gauge ${best.site}` : `${series.length} series, none usable`);
  return best;
}
