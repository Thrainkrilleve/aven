// weather.js — /weather: "will it rain here in the next hour?", four forecasters and the radar's
// answer, from /api/v1/weather. External text (place names, alert headlines) is only ever set
// with textContent.

import { AVEN_HOST_IMPORTS } from "./aven_math.js";

const $ = (id) => document.getElementById(id);
const RAIN_DBZ = 20;
const NWS_RAIN_PCT = 50;
const LEADS = [15, 30, 60];
// Display order: Aven first, then the baseline, then NWS
const FORECASTERS = [
  { key: "cell_motion", name: "Aven · storm motion", cls: "aven" },
  { key: "steering", name: "Aven · winds aloft", cls: "aven" },
  { key: "persistence", name: "No change", cls: "base" },
  { key: "nws", name: "NWS forecast", cls: "nws" },
];
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
const pct = (num, den) => (den ? `${Math.round((100 * num) / den)}%` : "–");
const utc = (ms) => new Date(ms).toISOString().slice(11, 16);
function ago(ms) {
  const m = Math.round((Date.now() - ms) / 60000);
  if (m < 1) return "just now";
  if (m < 60) return `${m} min ago`;
  const h = Math.floor(m / 60);
  return h < 48 ? `${h} h ${m % 60} min ago` : `${Math.floor(h / 24)} days ago`;
}
function emptyRow(body, text, cols) {
  const td = el("td", "empty", text);
  td.colSpan = cols;
  const tr = el("tr");
  tr.append(td);
  body.replaceChildren(tr);
}
async function getJSON(url) {
  const res = await fetch(url);
  if (!res.ok) throw new Error(`${url}: HTTP ${res.status}`);
  return res.json();
}

/** A forecaster's call for one place and time: { rain: true|false|null, text, detail } */
function call(key, f) {
  if (key === "nws") {
    if (f.nws_pop_pct === null || f.nws_pop_pct === undefined) return { rain: null, text: "n/a", detail: "" };
    return { rain: f.nws_pop_pct >= NWS_RAIN_PCT, text: `${Math.round(f.nws_pop_pct)}%`, detail: "chance" };
  }
  const dbz = f.predicted?.[key];
  if (dbz === null || dbz === undefined) return { rain: null, text: "–", detail: "" };
  return { rain: dbz >= RAIN_DBZ, text: dbz >= RAIN_DBZ ? "Rain" : "Dry", detail: `${fmt(dbz, 0)} dBZ` };
}

/** A table cell for a call, marked right or wrong once the radar outcome is known */
function callCell(c, observed) {
  const td = el("td", "wx-call");
  if (c.rain === null) {
    td.append(el("span", "wx-dim", c.text));
    return td;
  }
  const known = observed !== null && observed !== undefined;
  const right = known && c.rain === observed >= RAIN_DBZ;
  if (known) {
    const mark = el("span", right ? "wx-mark ok" : "wx-mark bad", right ? "✓" : "✗");
    mark.setAttribute("aria-label", right ? "right" : "wrong");
    td.append(mark);
  }
  td.append(el("span", c.rain ? "wx-rain" : "wx-dry", c.text));
  if (c.detail) td.append(el("small", "wx-dim", ` ${c.detail}`));
  return td;
}

function outcomeCell(observed, radarMs, dueMs) {
  const td = el("td", "wx-call wx-outcome");
  if (observed === null || observed === undefined) {
    if (dueMs) td.append(el("small", "wx-dim", "due "), el("span", "wx-dim", utc(dueMs)), el("small", "wx-dim", " UTC"));
    else td.append(el("span", "wx-dim", "waiting"));
    return td;
  }
  td.append(el("span", observed >= RAIN_DBZ ? "wx-rain" : "wx-dry", observed >= RAIN_DBZ ? "Rain" : "Dry"));
  td.append(el("small", "wx-dim", ` ${fmt(observed, 0)} dBZ${radarMs ? ` at ${utc(radarMs)}` : ""}`));
  return td;
}

// ---------------------------------------------------------------------------
// Scoreboard

let kind = "storm_cell";

async function loadScore() {
  const body = $("wx-score");
  const detail = $("wx-score-detail");
  try {
    const s = await getJSON(`/api/v1/weather/skill?kind=${kind}`);
    const get = (key, lead) => s.rows.find((r) => r.method === key && r.lead_min === lead);
    detail.replaceChildren();
    if (!s.rows.length) {
      emptyRow(body, s.pending ? `Nothing scored yet: ${s.pending} forecast time(s) are waiting for the radar.` : "Nothing scored yet.", 4);
      $("wx-headline").textContent = "";
      $("wx-score-foot").textContent = "";
      return;
    }
    body.replaceChildren();
    for (const f of FORECASTERS) {
      const tr = el("tr", `wx-row-${f.cls}`);
      tr.append(el("td", "wx-fname", f.name));
      for (const lead of LEADS) {
        const r = get(f.key, lead);
        const td = el("td", "num");
        if (r && r.n) {
          td.append(el("b", "", pct(r.correct, r.n)));
          td.append(el("small", "wx-dim wx-n", `of ${r.n}`));
        } else td.append(el("span", "wx-dim", "–"));
        tr.append(td);
      }
      body.append(tr);
    }

    // Plain-language headline for 30 minutes ahead (or the lead with the most data)
    const lead = [30, 15, 60].find((l) => s.rows.some((r) => r.lead_min === l && r.n));
    const scored = FORECASTERS.map((f) => ({ f, r: get(f.key, lead) })).filter((x) => x.r && x.r.n);
    scored.sort((a, b) => b.r.correct / b.r.n - a.r.correct / a.r.n);
    const most = Math.max(...scored.map((x) => x.r.n));
    $("wx-headline").textContent = scored.length
      ? `${lead} minutes ahead, ${scored[0].f.name} has been right most often: ${pct(scored[0].r.correct, scored[0].r.n)} of ${scored[0].r.n} calls` +
        (scored.length > 1 ? `, against ${scored.slice(1).map((x) => `${pct(x.r.correct, x.r.n)} for ${x.f.name}`).join(", ")}.` : ".") +
        (most < 30 ? " That is still too few forecasts to tell the forecasters apart." : "")
      : "";

    for (const l of LEADS) {
      for (const f of FORECASTERS) {
        const r = get(f.key, l);
        if (!r || !r.n) continue;
        const tr = el("tr");
        tr.append(el("td", "", `${l} min`), el("td", "", f.name), el("td", "num", String(r.n)),
          el("td", "num", String(r.hits)), el("td", "num", String(r.misses)), el("td", "num", String(r.false_alarms)),
          el("td", "num", String(r.correct_nulls)),
          el("td", "num", r.mae_dbz !== undefined && r.mae_dbz !== null ? fmt(r.mae_dbz, 1) : "–"),
          el("td", "num", fmt(r.brier !== undefined && r.brier !== null ? r.brier : 1 - r.correct / r.n, 3)));
        detail.append(tr);
      }
    }
    const first = Math.min(...s.rows.map((r) => r.first_ms));
    $("wx-score-foot").textContent =
      `Share of rain / no-rain calls that matched the radar, since ${new Date(first).toUTCString().slice(5, 22)} UTC. ` +
      `${s.pending} forecast time(s) are still waiting for the radar` + (s.expired ? `; ${s.expired} could not be checked (no radar image near their time).` : ".");
  } catch (e) {
    emptyRow(body, "The scores could not be loaded.", 4);
  }
}

// ---------------------------------------------------------------------------
// Latest forecasts

function check(label, value, verdict) {
  const row = el("div", "wx-check");
  row.append(el("span", `wx-dot ${verdict}`), el("span", "wx-check-label", label), el("span", "wx-check-val", value));
  return row;
}

/** "Will it rain here?": one row per forecaster, one column per lead, then the radar's answer */
function forecastTable(run) {
  const i = run.inputs ?? {};
  const rec = run.record ?? {};
  const byLead = (lead) => {
    const rows = (run.forecasts ?? []).filter((x) => x.lead_min === lead);
    const f = { predicted: {}, nws_pop_pct: rows[0]?.nws_pop_pct ?? null, observed: rows[0]?.observed_dbz ?? null,
                radar_ms: rows[0]?.radar_ms ?? null, valid_ms: rows[0]?.valid_ms ?? null };
    for (const r of rows) f.predicted[r.method] = r.predicted_dbz;
    if (!rows.length) {
      // No forecast rows (a run stopped by a data check): show what the model computed
      f.predicted = { steering: i[`upstream_dbz_${lead}`], cell_motion: i[`cell_upstream_dbz_${lead}`], persistence: i.radar_dbz };
      const t = (rec.radar?.site_ms ?? run.created_ms) + lead * 60000;
      f.nws_pop_pct = (rec.hourly ?? []).find((p) => p.start_ms <= t && t < p.start_ms + 3600000)?.pop_pct ?? null;
    }
    return f;
  };
  const leads = LEADS.map(byLead);

  const table = el("table", "wx-q");
  const thead = el("thead");
  const head = el("tr");
  head.append(el("th", "", "Rain here?"), ...LEADS.map((l) => el("th", "", `${l} min`)));
  thead.append(head);
  const tbody = el("tbody");
  for (const fc of FORECASTERS) {
    if (fc.key === "cell_motion" && i.cell_upstream_dbz_15 == null) continue;
    const tr = el("tr", `wx-row-${fc.cls}`);
    tr.append(el("th", "wx-fname", fc.name));
    leads.forEach((f) => tr.append(callCell(call(fc.key, f), f.observed)));
    tbody.append(tr);
  }
  const out = el("tr", "wx-row-outcome");
  out.append(el("th", "wx-fname", "Radar showed"));
  leads.forEach((f) => out.append(outcomeCell(f.observed, f.radar_ms, f.valid_ms)));
  tbody.append(out);
  table.append(thead, tbody);
  const wrap = el("div", "wx-q-wrap");
  wrap.append(table);
  return wrap;
}

function runCard(run) {
  const card = el("article", `wx-run wx-${run.status}`);
  const rec = run.record ?? {};
  const i = run.inputs ?? {};
  const o = run.outputs ?? {};

  const head = el("div", "wx-run-head");
  const title = el("div");
  title.append(el("h3", "", rec.site?.place || run.site_label));
  title.append(el("div", "wx-dim", `${run.site_label} · ${run.site_lat.toFixed(2)}, ${run.site_lon.toFixed(2)}`));
  const badge = el("span", `wx-badge ${run.status}`, { ok: "Forecast made", gated: "Data failed a check", error: "Not enough data" }[run.status]);
  head.append(title, badge);
  card.append(head);

  if (run.status === "error") {
    card.append(el("p", "wx-error", run.error ?? ""));
    card.append(el("div", "wx-dim", `Tried ${utc(run.created_ms)} UTC (${ago(run.created_ms)}); the job moved on to another place.`));
    return card;
  }
  if (run.status === "gated") card.append(el("p", "wx-error", `${run.error ?? ""}. These forecasts are shown but not scored.`));

  const now = el("p", "wx-now");
  const raining = i.radar_dbz >= RAIN_DBZ;
  now.append(el("span", "", `Radar at ${utc(rec.radar?.site_ms ?? run.created_ms)} UTC: `),
    el("b", raining ? "wx-rain" : "wx-dry", raining ? `raining (${fmt(i.radar_dbz, 0)} dBZ, about ${fmt(o.radar_rain_mmh, 1)} mm/h)` : `dry (${fmt(i.radar_dbz, 0)} dBZ)`));
  if (rec.alerts?.length) now.append(el("span", "wx-alert", ` · ⚠ ${rec.alerts.map((a) => a.event).join(", ")}`));
  card.append(now);
  card.append(forecastTable(run));

  // Everything behind the forecast, collapsed
  const more = el("details", "wx-more");
  more.append(el("summary", "", "Data and checks"));
  const checks = el("div", "wx-checks");
  const nwsAt = rec.stations?.nws_observation ? ` at ${utc(rec.stations.nws_observation.timestamp_ms)} UTC` : "";
  checks.append(check(`Humidity: NWS reported vs recomputed from the METAR${nwsAt}`,
    o.rh_residual == null ? (rec.stations?.nws_note ?? "NWS reported none") : `${fmt(i.rh_reported)} % vs ${fmt(o.check_relative_humidity)} % (difference ${fmt(o.rh_residual, 2)})`,
    o.rh_residual == null ? "na" : o.rh_residual <= 5 ? "ok" : "bad"));
  checks.append(check(`Altimeter setting: NWS vs METAR${nwsAt}`,
    o.altimeter_residual_hpa == null ? (rec.stations?.nws_note ?? "NWS reported none") : `${fmt(i.nws_altimeter_hpa)} vs ${fmt(i.check_altimeter_hpa)} hPa (difference ${fmt(o.altimeter_residual_hpa, 2)})`,
    o.altimeter_residual_hpa == null ? "na" : o.altimeter_residual_hpa <= 0.5 ? "ok" : "warn"));
  checks.append(check("Rain: radar estimate vs USGS rain gauge",
    i.gauge_available ? `${fmt(o.radar_rain_mmh, 2)} vs ${fmt(i.gauge_rate_mmh, 2)} mm/h` : "no gauge within 25 km",
    i.gauge_available ? (Math.abs(o.radar_minus_gauge_mmh) <= 5 ? "ok" : "warn") : "na"));
  checks.append(check("Storm motion: winds aloft vs radar-tracked",
    o.cell_speed_ms == null ? "no tracked storm nearby" : `${fmt(o.steering_speed_ms)} vs ${fmt(o.cell_speed_ms)} m/s (vector difference ${fmt(o.steering_vs_cell_ms)})`,
    o.cell_speed_ms == null ? "na" : o.steering_vs_cell_ms <= 5 ? "ok" : "warn"));
  const gateFailed = rec.gate_failed ?? [];
  checks.append(check("Data-quality bounds (from weather-predict.aven)", gateFailed.length ? gateFailed.join("; ") : "all hold", gateFailed.length ? "bad" : "ok"));
  more.append(el("div", "wx-label", "Cross-checks between sources"), checks);

  if (run.outputs) {
    const phys = el("dl", "wx-phys");
    const add = (k, v) => {
      const pair = el("div");
      pair.append(el("dt", "", k), el("dd", "", v));
      phys.append(pair);
    };
    add("Temperature / dewpoint", `${fmt(i.temp_c)} / ${fmt(i.dewpoint_c)} °C`);
    add("Station pressure", `${fmt(o.pressure_hpa)} hPa`);
    add("Air density", `${fmt(o.air_density_kg_m3, 3)} kg/m³`);
    add("Specific humidity", `${fmt(o.specific_humidity * 1000, 2)} g/kg`);
    add("Surface divergence", `${fmt(o.divergence_per_s * 1e5, 2)} ×10⁻⁵ /s`);
    add("Vertical velocity at 1 km", `${fmt(o.vertical_velocity_ms * 100, 1)} cm/s`);
    add("Vertical wind shear", `${fmt(o.vertical_shear_ms_per_km, 1)} m/s per km`);
    add("Storm speed (radar)", o.cell_speed_ms == null ? "n/a" : `${fmt(o.cell_speed_ms, 1)} m/s`);
    more.append(el("div", "wx-label", "Computed by the Aven model"), phys);
  }

  const ctx = el("div", "wx-ctx");
  if (rec.hourly?.length) ctx.append(el("div", "wx-dim", `NWS hourly forecast: ${rec.hourly.map((h) => `${utc(h.start_ms)} ${fmt(h.pop_pct, 0)}% (${h.short})`).join(" · ")}`));
  if (rec.stations) {
    ctx.append(el("div", "wx-dim",
      `Stations: ${rec.stations.primary} (main), wind triangle ${rec.stations.triangle.map((s) => s.id).join(" · ")}; winds aloft ${rec.stations.winds_aloft.station}`));
  }
  more.append(ctx);

  const list = el("ul", "wx-sources");
  for (const s of run.sources) {
    const li = el("li");
    li.append(el("span", `wx-dot ${s.ok ? "ok" : "bad"}`), el("span", "", SOURCE_NAMES[s.source] ?? s.source));
    const extra = [s.detail, s.data_ms ? `data ${utc(s.data_ms)} UTC` : ""].filter(Boolean).join(" · ");
    if (extra) li.append(el("span", "wx-dim", extra));
    list.append(li);
  }
  more.append(el("div", "wx-label", `${run.sources.length} requests, ${run.sources.filter((s) => s.ok).length} answered`), list);

  if (run.inputs) {
    const btn = el("button", "btn btn-ghost btn-sm", "Re-run this model in your browser");
    btn.type = "button";
    const out = el("div", "wx-rerun");
    btn.addEventListener("click", () => rerun(run, out));
    const foot = el("div", "wx-run-foot");
    foot.append(btn, out);
    more.append(foot);
  }
  card.append(more);
  card.append(el("div", "wx-dim wx-run-meta", `Made ${utc(run.created_ms)} UTC (${ago(run.created_ms)}) from the radar image of ${utc(rec.radar?.site_ms ?? run.created_ms)} UTC`));
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
      out.textContent = `This forecast used a different build of the model (${run.model_sha256.slice(0, 12)}…); the current one is ${sha.slice(0, 12)}….`;
      return;
    }
    const { instance } = await WebAssembly.instantiate(bytes, AVEN_HOST_IMPORTS);
    const mem = new Float64Array(instance.exports.memory.buffer);
    model.inputs.forEach((inp, k) => (mem[k] = run.inputs[inp.name] ?? NaN));
    instance.exports.aven_solve(0, 1024);
    const got = new Float64Array(instance.exports.memory.buffer, 1024, model.outputs.length);
    let same = 0, worst = 0;
    model.outputs.forEach((name, j) => {
      const want = run.outputs[name];
      if (want === null || want === undefined ? !Number.isFinite(got[j]) : got[j] === want) same++;
      else worst = Math.max(worst, Math.abs(got[j] - want) / Math.max(Math.abs(want), 1e-300));
    });
    const n = model.outputs.length;
    out.textContent = `Model sha256 ${sha.slice(0, 12)}… matches. ${same} of ${n} outputs identical to the stored run` +
      (same < n
        ? `; the rest differ by at most ${worst.toExponential(1)} relative. exp and pow come from the JavaScript engine, and versions can differ in the last bit; outputs that subtract nearly equal values magnify that.`
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
      ? `Last forecast ${ago(last.created_ms)} · ${t.ok} forecasts made since ${new Date(t.first_ms).toUTCString().slice(5, 22)} UTC` +
        (t.gated || t.errors ? ` (plus ${t.gated} stopped by a data check and ${t.errors} places without enough data)` : "")
      : "No forecasts yet. The scheduled job makes one every few minutes once deployed."));
    const cycles = data.cycles ?? [];
    const c = cycles[0];
    if (c) {
      const stuck = cycles.filter((x) => !x.finished_ms && Date.now() - x.started_ms > 2 * 60 * 1000).length;
      let line = `Last scheduled task (${c.task}) started ${ago(c.started_ms)}: `;
      if (!c.finished_ms) line += Date.now() - c.started_ms > 2 * 60 * 1000 ? "never finished" : "running";
      else line += c.error ? `failed: ${c.error}` : `finished, ${c.requests} requests`;
      status.append(el("div", c.error || stuck ? "wx-off" : "wx-dim", line));
      if (stuck) status.append(el("div", "wx-off", `${stuck} of the last ${cycles.length} tasks started but never finished: the platform stopped them, usually at its CPU-time limit.`));
    } else {
      status.append(el("div", "wx-off", "No scheduled task has run yet. If this persists more than five minutes after deploying, check the worker's cron triggers and logs."));
    }
    status.append(el("div", "wx-dim", `Page updated ${new Date().toISOString().slice(11, 19)} UTC; refreshes every minute.`));
    $("wx-live-dot").classList.toggle("stale", !last || Date.now() - last.created_ms > 15 * 60 * 1000);
    // Forecasts first; places without enough data after them
    const runs = [...data.runs].sort((a, b) => (a.status === "error") - (b.status === "error") || b.created_ms - a.created_ms);
    box.replaceChildren(...(runs.length ? runs.map(runCard) : [el("div", "empty", "No forecasts yet.")]));
  } catch (e) {
    $("wx-status").textContent = "The live data could not be loaded.";
    box.replaceChildren(el("div", "empty", "The live data could not be loaded. Try again later."));
  }
}

async function loadOutcomes() {
  const body = $("wx-ver");
  try {
    const { outcomes } = await getJSON("/api/v1/weather/verifications?limit=30");
    if (!outcomes.length) return emptyRow(body, "Nothing to compare yet: the first forecasts are waiting for their time to come.", 8);
    body.replaceChildren();
    for (const o of outcomes) {
      const tr = el("tr");
      tr.append(el("td", "", utc(o.valid_ms)), el("td", "", o.place || o.site_label), el("td", "num", `${o.lead_min} min`));
      for (const f of FORECASTERS) tr.append(callCell(call(f.key, o), o.observed_dbz));
      tr.append(outcomeCell(o.observed_dbz, o.radar_ms));
      body.append(tr);
    }
  } catch {
    emptyRow(body, "Outcomes could not be loaded.", 8);
  }
}

function loadAll() {
  loadRuns();
  loadScore();
  loadOutcomes();
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
