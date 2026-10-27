// weather.js — /weather: the live nowcast's scoreboard, runs and scores, from /api/v1/weather.
// External text (place names, alert headlines) is only ever set with textContent.

import { AVEN_HOST_IMPORTS } from "./aven_math.js";

const $ = (id) => document.getElementById(id);
const METHODS = { steering: "Steering wind", cell_motion: "Storm motion", persistence: "Persistence" };
const SOURCE_NAMES = {
  nexrad_cells: "NEXRAD storm cells", radar_times: "Radar image list", radar: "Radar mosaic",
  winds_aloft: "Winds aloft", winds_aloft_stations: "Winds-aloft stations", nws_point: "NWS point",
  metar: "METAR", nws_observation: "NWS observation", usgs_gauges: "USGS gauges",
  nws_alerts: "NWS alerts", nws_hourly_forecast: "NWS hourly forecast",
};

function el(tag, cls, text) {
  const n = document.createElement(tag);
  if (cls) n.className = cls;
  if (text !== undefined) n.textContent = text;
  return n;
}
const fmt = (v, d = 1) => (v === null || v === undefined || !Number.isFinite(v) ? "n/a" : v.toFixed(d));
const utc = (ms) => new Date(ms).toISOString().slice(11, 16);
function ago(ms) {
  const m = Math.round((Date.now() - ms) / 60000);
  if (m < 1) return "just now";
  if (m < 60) return `${m} min ago`;
  const h = Math.floor(m / 60);
  return h < 48 ? `${h} h ${m % 60} min ago` : `${Math.floor(h / 24)} days ago`;
}

function emptyRow(body, text) {
  const td = el("td", "empty", text);
  td.colSpan = 7;
  const tr = el("tr");
  tr.append(td);
  body.replaceChildren(tr);
}

async function getJSON(url) {
  const res = await fetch(url);
  if (!res.ok) throw new Error(`${url}: HTTP ${res.status}`);
  return res.json();
}

// ---------------------------------------------------------------------------
// Scoreboard

let kind = "storm_cell";

async function loadScore() {
  const body = $("wx-score");
  try {
    const s = await getJSON(`/api/v1/weather/skill?kind=${kind}`);
    body.replaceChildren();
    if (!s.rows.length) emptyRow(body, s.pending ? `No forecasts scored yet; ${s.pending} waiting for their time to arrive.` : "No forecasts scored yet.");
    const leads = [...new Set(s.rows.map((r) => r.lead_min))];
    for (const lead of leads) {
      const rows = s.rows.filter((r) => r.lead_min === lead);
      const best = Math.min(...rows.map((r) => r.mae_dbz));
      rows.forEach((r, i) => {
        const tr = el("tr");
        if (i === 0) {
          const td = el("td", "wx-lead", `+${lead} min`);
          td.rowSpan = rows.length;
          tr.append(td);
        }
        const csiDen = r.hits + r.misses + r.false_alarms;
        tr.append(
          el("td", r.method === "persistence" ? "wx-base" : "", METHODS[r.method] ?? r.method),
          el("td", "num", String(r.n)),
          el("td", `num${r.mae_dbz === best && rows.length > 1 ? " wx-best" : ""}`, fmt(r.mae_dbz, 2)),
          el("td", "num", fmt(r.bias_dbz, 2)),
          el("td", "num", csiDen ? (r.hits / csiDen).toFixed(2) : "n/a"),
          el("td", "num wx-dim", `${r.hits} · ${r.misses} · ${r.false_alarms}`),
        );
        body.append(tr);
      });
    }
    const first = Math.min(...s.rows.map((r) => r.first_ms));
    $("wx-score-foot").textContent =
      (s.rows.length ? `Scored since ${new Date(first).toUTCString().replace(" GMT", " UTC")}. ` : "") +
      `${s.pending} forecast(s) waiting; ${s.expired} could not be scored (no radar image near their time). Best MAE per lead is highlighted.`;
  } catch (e) {
    emptyRow(body, "The scoreboard could not be loaded.");
  }
}

// ---------------------------------------------------------------------------
// Runs

function check(label, value, verdict, detail) {
  const row = el("div", "wx-check");
  row.append(el("span", `wx-dot ${verdict}`), el("span", "wx-check-label", label), el("span", "wx-check-val", value));
  if (detail) row.title = detail;
  return row;
}

function runCard(run) {
  const card = el("article", `wx-run wx-${run.status}`);
  const rec = run.record ?? {};
  const i = run.inputs ?? {};
  const o = run.outputs ?? {};

  const head = el("div", "wx-run-head");
  const title = el("div");
  title.append(el("h3", "", run.site_label));
  const where = [rec.site?.place, `${run.site_lat.toFixed(2)}, ${run.site_lon.toFixed(2)}`].filter(Boolean).join(" · ");
  title.append(el("div", "wx-dim", where));
  const badge = el("span", `wx-badge ${run.status}`, { ok: "Forecast issued", gated: "Gated", error: "No forecast" }[run.status]);
  head.append(title, badge);
  card.append(head);

  const meta = el("div", "wx-dim wx-run-meta",
    `Run ${utc(run.created_ms)} UTC (${ago(run.created_ms)})` + (rec.radar?.site_ms ? ` · radar image ${utc(rec.radar.site_ms)} UTC` : ""));
  card.append(meta);

  if (run.status === "error" || run.status === "gated") card.append(el("p", "wx-error", run.error ?? ""));

  if (run.outputs) {
    // Nowcast strip
    const strip = el("div", "wx-strip");
    const methods = ["steering", "cell_motion", "persistence"];
    const grid = el("div", "wx-grid");
    grid.append(el("span", "wx-dim", ""), ...["Now", "+15", "+30", "+60"].map((h) => el("span", "wx-dim wx-h", h)));
    for (const m of methods) {
      if (m === "cell_motion" && i.cell_upstream_dbz_15 == null) continue;
      grid.append(el("span", "wx-m", METHODS[m]));
      grid.append(el("span", "wx-v", `${fmt(i.radar_dbz, 0)}`));
      for (const lead of [15, 30, 60]) {
        const pred = m === "steering" ? i[`upstream_dbz_${lead}`] : m === "cell_motion" ? i[`cell_upstream_dbz_${lead}`] : i.radar_dbz;
        const f = run.forecasts?.find((x) => x.method === m && x.lead_min === lead);
        const cell = el("span", "wx-v", fmt(pred, 0));
        if (f?.status === "verified") {
          cell.append(el("small", Math.abs(f.predicted_dbz - f.observed_dbz) <= 5 ? "wx-good" : "wx-off", ` obs ${fmt(f.observed_dbz, 0)}`));
        } else if (f?.status === "pending") cell.append(el("small", "wx-dim", ` due ${utc(f.valid_ms)}`));
        grid.append(cell);
      }
    }
    strip.append(el("div", "wx-label", "Reflectivity (dBZ) at the site"), grid);
    card.append(strip);

    // Cross-checks
    const checks = el("div", "wx-checks");
    checks.append(el("div", "wx-label", "Cross-checks"));
    const rhOk = o.rh_residual == null ? "na" : o.rh_residual <= 5 ? "ok" : "bad";
    const nwsAt = rec.stations?.nws_observation ? ` at ${utc(rec.stations.nws_observation.timestamp_ms)} UTC` : "";
    checks.append(check(`Humidity: NWS reported vs recomputed from the METAR${nwsAt}`,
      o.rh_residual == null ? (rec.stations?.nws_note ?? "NWS reported none") : `${fmt(i.rh_reported)} % vs ${fmt(o.check_relative_humidity)} % (Δ ${fmt(o.rh_residual, 2)})`, rhOk));
    const pOk = o.altimeter_residual_hpa == null ? "na" : o.altimeter_residual_hpa <= 0.5 ? "ok" : "warn";
    checks.append(check(`Altimeter setting: NWS vs METAR${nwsAt}`,
      o.altimeter_residual_hpa == null ? (rec.stations?.nws_note ?? "NWS reported none") : `${fmt(i.nws_altimeter_hpa)} vs ${fmt(i.check_altimeter_hpa)} hPa (Δ ${fmt(o.altimeter_residual_hpa, 2)})`, pOk));
    checks.append(check("Rain: radar estimate vs USGS gauge",
      i.gauge_available ? `${fmt(o.radar_rain_mmh, 2)} vs ${fmt(i.gauge_rate_mmh, 2)} mm/h` : "no gauge within 25 km",
      i.gauge_available ? (Math.abs(o.radar_minus_gauge_mmh) <= 5 ? "ok" : "warn") : "na"));
    checks.append(check("Motion: steering wind vs radar-tracked storm",
      o.cell_speed_ms == null ? "no tracked cell nearby" : `${fmt(o.steering_speed_ms)} vs ${fmt(o.cell_speed_ms)} m/s (|Δv| ${fmt(o.steering_vs_cell_ms)})`,
      o.cell_speed_ms == null ? "na" : o.steering_vs_cell_ms <= 5 ? "ok" : "warn"));
    const gateFailed = rec.gate_failed ?? [];
    checks.append(check("Data-quality gate (weather-predict.aven bounds)", gateFailed.length ? gateFailed.join("; ") : "all bounds hold", gateFailed.length ? "bad" : "ok"));
    card.append(checks);

    // Physics
    const phys = el("dl", "wx-phys");
    const add = (k, v) => {
      const pair = el("div");
      pair.append(el("dt", "", k), el("dd", "", v));
      phys.append(pair);
    };
    add("Rain rate (Marshall-Palmer)", `${fmt(o.radar_rain_mmh, 2)} mm/h`);
    add("Air density", `${fmt(o.air_density_kg_m3, 3)} kg/m³`);
    add("Specific humidity", `${fmt(o.specific_humidity * 1000, 2)} g/kg`);
    add("Surface divergence", `${fmt(o.divergence_per_s * 1e5, 2)} ×10⁻⁵ /s`);
    add("Vertical velocity at 1 km", `${fmt(o.vertical_velocity_ms * 100, 1)} cm/s`);
    add("Vertical shear", `${fmt(o.vertical_shear_ms_per_km, 1)} m/s per km`);
    const pd = el("div");
    pd.append(el("div", "wx-label", "Derived by the model"), phys);
    card.append(pd);
  }

  // Context, sources, re-run
  const ctx = el("div", "wx-ctx");
  if (rec.alerts?.length) {
    const a = el("div", "wx-alerts");
    rec.alerts.slice(0, 3).forEach((x) => a.append(el("div", "", `⚠ ${x.event}`)));
    ctx.append(a);
  }
  if (rec.hourly?.length) ctx.append(el("div", "wx-dim", `NWS hourly forecast: ${rec.hourly[0].short}, ${fmt(rec.hourly[0].pop_pct, 0)} % chance of precipitation`));
  if (rec.stations) {
    ctx.append(el("div", "wx-dim",
      `Stations: ${rec.stations.primary} (primary), triangle ${rec.stations.triangle.map((s) => s.id).join(" · ")}; winds aloft ${rec.stations.winds_aloft.station}`));
  }
  card.append(ctx);

  const details = el("details", "wx-sources");
  const ok = run.sources.filter((s) => s.ok).length;
  details.append(el("summary", "", `${run.sources.length} source requests, ${ok} answered`));
  const list = el("ul");
  for (const s of run.sources) {
    const li = el("li");
    li.append(el("span", `wx-dot ${s.ok ? "ok" : "bad"}`), el("span", "", SOURCE_NAMES[s.source] ?? s.source));
    const extra = [s.detail, s.data_ms ? `data ${utc(s.data_ms)} UTC` : ""].filter(Boolean).join(" · ");
    if (extra) li.append(el("span", "wx-dim", extra));
    list.append(li);
  }
  details.append(list);
  card.append(details);

  if (run.inputs) {
    const btn = el("button", "btn btn-ghost btn-sm", "Re-run this model in your browser");
    btn.type = "button";
    const out = el("div", "wx-rerun");
    btn.addEventListener("click", () => rerun(run, out));
    const foot = el("div", "wx-run-foot");
    foot.append(btn, out);
    card.append(foot);
  }
  return card;
}

// ---------------------------------------------------------------------------
// Re-run: the same WebAssembly module, on the stored inputs, in this browser

let manifestPromise = null;

async function rerun(run, out) {
  out.textContent = "Running…";
  try {
    manifestPromise ??= getJSON("/models/manifest.json");
    const model = (await manifestPromise).models.find((m) => m.key === "weather");
    const bytes = await (await fetch("/models/weather.wasm")).arrayBuffer();
    const sha = [...new Uint8Array(await crypto.subtle.digest("SHA-256", bytes))].map((b) => b.toString(16).padStart(2, "0")).join("");
    if (sha !== run.model_sha256) {
      out.textContent = `This run used a different build of the model (${run.model_sha256.slice(0, 12)}…); the current one is ${sha.slice(0, 12)}….`;
      return;
    }
    const { instance } = await WebAssembly.instantiate(bytes, AVEN_HOST_IMPORTS);
    const mem = new Float64Array(instance.exports.memory.buffer);
    model.inputs.forEach((inp, k) => (mem[k] = run.inputs[inp.name] ?? NaN));
    instance.exports.aven_solve(0, 1024);
    const got = new Float64Array(instance.exports.memory.buffer, 1024, model.outputs.length);
    let same = 0, worst = 0, compared = 0;
    model.outputs.forEach((name, j) => {
      const want = run.outputs[name];
      if (want === null || want === undefined) {
        if (!Number.isFinite(got[j])) same++;
        compared++;
        return;
      }
      compared++;
      if (got[j] === want) same++;
      else worst = Math.max(worst, Math.abs(got[j] - want) / Math.max(Math.abs(want), 1e-300));
    });
    out.textContent = `sha256 ${sha.slice(0, 12)}… matches. ${same} of ${compared} outputs identical to the stored run` +
      (same < compared
        ? `; the rest differ by at most ${worst.toExponential(1)} relative. exp and pow come from the JavaScript engine, and versions can differ in the last bit; outputs that subtract nearly equal values (rh_residual) magnify that.`
        : ".");
  } catch (e) {
    out.textContent = `Could not re-run: ${e.message}`;
  }
}

async function loadRuns() {
  const box = $("wx-runs");
  try {
    const data = await getJSON("/api/v1/weather/latest?limit=8");
    const t = data.totals ?? {};
    const last = data.runs[0];
    const status = $("wx-status");
    status.replaceChildren(el("div", "", last
      ? `Last site run ${ago(last.created_ms)} · ${t.runs} runs since ${new Date(t.first_ms).toUTCString().slice(5, 22)} UTC: ${t.ok} issued forecasts, ${t.gated} gated, ${t.errors} without enough data`
      : "No site runs yet. The scheduled job runs a site every few minutes once deployed."));
    // The scheduled tasks themselves: a task with no finish time was stopped by the platform
    const cycles = data.cycles ?? [];
    const c = cycles[0];
    if (c) {
      const stuck = cycles.filter((x) => !x.finished_ms && Date.now() - x.started_ms > 2 * 60 * 1000).length;
      let line = `Last scheduled task (${c.task}) started ${ago(c.started_ms)}: `;
      if (!c.finished_ms) line += Date.now() - c.started_ms > 2 * 60 * 1000 ? "never finished" : "running";
      else line += c.error ? `failed: ${c.error}` : `finished, ${c.requests} requests`;
      status.append(el("div", c.error || stuck ? "wx-off" : "", line));
      if (stuck) status.append(el("div", "wx-off", `${stuck} of the last ${cycles.length} tasks started but never finished: the platform stopped them, usually at its CPU-time limit.`));
    } else {
      status.append(el("div", "wx-off", "No scheduled task has run yet. If this persists more than five minutes after deploying, check the worker's cron triggers and logs."));
    }
    status.append(el("div", "wx-dim", `Page updated ${new Date().toISOString().slice(11, 19)} UTC; refreshes every minute.`));
    $("wx-live-dot").classList.toggle("stale", !last || Date.now() - last.created_ms > 15 * 60 * 1000);
    box.replaceChildren(...(data.runs.length ? data.runs.map(runCard) : [el("div", "empty", "No runs yet.")]));
  } catch (e) {
    $("wx-status").textContent = "The live data could not be loaded.";
    box.replaceChildren(el("div", "empty", "The live data could not be loaded. Try again later."));
  }
}

async function loadVerifications() {
  const body = $("wx-ver");
  try {
    const { verifications } = await getJSON("/api/v1/weather/verifications?limit=40");
    body.replaceChildren();
    if (!verifications.length) return emptyRow(body, "Nothing scored yet.");
    for (const v of verifications) {
      const err = v.predicted_dbz - v.observed_dbz;
      const tr = el("tr");
      tr.append(
        el("td", "", utc(v.valid_ms)), el("td", "", v.site_label), el("td", "num", `+${v.lead_min}`),
        el("td", v.method === "persistence" ? "wx-base" : "", METHODS[v.method] ?? v.method),
        el("td", "num", fmt(v.predicted_dbz, 0)), el("td", "num", fmt(v.observed_dbz, 0)),
        el("td", `num ${Math.abs(err) <= 5 ? "wx-good" : "wx-off"}`, (err > 0 ? "+" : "") + fmt(err, 0)),
      );
      body.append(tr);
    }
  } catch {
    emptyRow(body, "Scores could not be loaded.");
  }
}

function loadAll() {
  loadRuns();
  loadScore();
  loadVerifications();
}

document.querySelectorAll(".wx-seg button").forEach((b) =>
  b.addEventListener("click", () => {
    kind = b.dataset.kind;
    document.querySelectorAll(".wx-seg button").forEach((x) => x.setAttribute("aria-selected", String(x === b)));
    loadScore();
  })
);
$("wx-refresh").addEventListener("click", loadAll);
loadAll();
// The job writes every few minutes; poll each minute while the page is visible
setInterval(() => {
  if (document.visibilityState === "visible") loadAll();
}, 60 * 1000);
document.addEventListener("visibilitychange", () => {
  if (document.visibilityState === "visible") loadAll();
});
