// solver-demo.js — the live demo on the homepage.
//
// Unlike a compiled forward model, this runs the REAL avenc omnidirectional
// solver (web/solver-wasm, compiled to wasm32). Every change rebuilds the
// model's .aven source and calls the solver in the browser — forward from the
// sliders, or backward when you target an output and free an input. Nothing is
// sent to a server.

const WASM_URL = "/wasm/solver.wasm";

// The canonical sizing model (examples/server_capacity.aven, relation form).
const RELATION = `relation WorkerScaling {
    raw_workers       = (reqs_per_second * avg_latency_ms) / 1000
    allocated_workers = ceil(raw_workers / target_utilization)
    total_ram_needed  = (allocated_workers * (ram_per_worker + socket_buffer_mb)) + os_kernel_ram_mb
    payload_gbps      = (reqs_per_second * avg_payload_kb * 8) / 1000000
    wire_rate_gbps    = payload_gbps * framing_overhead_ratio
}`;

const INPUTS = [
  { name: "reqs_per_second", label: "Requests / second", def: 50000, min: 1000, max: 200000, step: 1000 },
  { name: "avg_latency_ms", label: "Avg latency (ms)", def: 20, min: 1, max: 200, step: 1 },
  { name: "avg_payload_kb", label: "Avg payload (KB)", def: 64, min: 1, max: 1024, step: 1 },
  { name: "ram_per_worker", label: "RAM / worker (MB)", def: 16, min: 1, max: 128, step: 1 },
  { name: "socket_buffer_mb", label: "Socket buffer (MB)", def: 4, min: 1, max: 64, step: 1 },
  { name: "os_kernel_ram_mb", label: "OS/kernel RAM (MB)", def: 4096, min: 512, max: 16384, step: 128 },
  { name: "target_utilization", label: "Target utilization", def: 0.7, min: 0.3, max: 0.95, step: 0.01 },
  { name: "framing_overhead_ratio", label: "Framing overhead", def: 1.04, min: 1.0, max: 1.2, step: 0.01 },
];

const OUTPUTS = [
  { name: "raw_workers", label: "Raw workers" },
  { name: "allocated_workers", label: "Allocated workers" },
  { name: "total_ram_needed", label: "Total RAM needed (MB)" },
  { name: "payload_gbps", label: "Payload (Gbps)" },
  { name: "wire_rate_gbps", label: "Wire rate (Gbps)" },
];

// Outputs that invert cleanly for a backward solve (linear in the freed input).
const REVERSE_TARGETS = ["payload_gbps", "wire_rate_gbps", "total_ram_needed"];

// Starting point for the free-form editor. Shows both directions in one model:
// mass is solved backward from force, then momentum is filled forward.
const EDITOR_EXAMPLE = `// Edit this model and press Run (Ctrl/Cmd+Enter).
// Equations are invariants, not assignments: leave a variable out of the
// solve block and Aven solves for it — forward or backward.
relation Motion {
    force    = mass * acceleration
    momentum = mass * velocity
}

solve Motion with {
    force        = 4200
    acceleration = 5
    velocity     = 12
}
`;

class Solver {
  constructor(instance) {
    this.ex = instance.exports;
    this.enc = new TextEncoder();
    this.dec = new TextDecoder();
  }
  static async load() {
    const resp = await fetch(WASM_URL);
    if (!resp.ok) throw new Error(`solver.wasm: HTTP ${resp.status}`);
    const { instance } = await WebAssembly.instantiate(await resp.arrayBuffer(), {});
    return new Solver(instance);
  }
  mem() {
    return new Uint8Array(this.ex.memory.buffer);
  }
  solve(source) {
    const bytes = this.enc.encode(source);
    const inPtr = this.ex.alloc(bytes.length);
    this.mem().set(bytes, inPtr);
    const outPtr = this.ex.solve(inPtr, bytes.length);
    const dv = new DataView(this.ex.memory.buffer);
    const len = dv.getUint32(outPtr, true);
    const json = this.dec.decode(this.mem().subarray(outPtr + 4, outPtr + 4 + len));
    this.ex.dealloc(inPtr, bytes.length);
    this.ex.dealloc(outPtr, 4 + len);
    return JSON.parse(json);
  }
}

function fmt(v) {
  if (v === null || v === undefined || !isFinite(v)) return "—";
  if (Math.abs(v) >= 1000 || v === Math.trunc(v)) return v.toLocaleString(undefined, { maximumFractionDigits: 2 });
  return v.toLocaleString(undefined, { maximumFractionDigits: 4 });
}

function num(name, v) {
  // target_utilization and framing print with decimals; others are plain.
  return Number.isFinite(v) ? v : 0;
}

// Paint the track's filled portion up to the thumb. The CSS track reads the
// --fill custom property (default 50%); without this the fill never tracks.
function setFill(range) {
  const min = parseFloat(range.min);
  const max = parseFloat(range.max);
  const v = parseFloat(range.value);
  const pct = max > min ? ((v - min) / (max - min)) * 100 : 0;
  range.style.setProperty("--fill", `${Math.max(0, Math.min(100, pct))}%`);
}

function buildSource(bindings) {
  const lines = Object.entries(bindings).map(([k, v]) => `    ${k} = ${v}`);
  return `${RELATION}\n\nsolve WorkerScaling with {\n${lines.join("\n")}\n}\n`;
}

function escapeHtml(s) {
  return String(s).replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" }[c]));
}

// Turn the solver's Debug-formatted error into a readable one line.
function formatErr(res) {
  const m = /message: "(.*?)", line: (\d+), col: (\d+)/.exec(res.error || "");
  if (m) return `${res.stage} error (line ${m[2]}, col ${m[3]}): ${m[1]}`;
  return `${res.stage || "solver"} error: ${res.error || "unknown"}`;
}

export async function mountDemo(root) {
  const statusEl = root.querySelector("#demo-status");
  let solver;
  try {
    solver = await Solver.load();
  } catch (e) {
    if (statusEl) statusEl.textContent = `Could not load the solver: ${e.message}`;
    return;
  }

  const state = Object.fromEntries(INPUTS.map((i) => [i.name, i.def]));
  let mode = "forward";
  let freed = "reqs_per_second";
  let target = "wire_rate_gbps";
  let targetValue = 40;

  const slidersEl = root.querySelector("#demo-sliders");
  const outputsEl = root.querySelector("#demo-outputs");
  const srcEl = root.querySelector("#demo-src");
  const timingEl = root.querySelector("#demo-timing");
  const modeBtns = root.querySelectorAll("[data-mode]");
  const reverseEl = root.querySelector("#demo-reverse");
  const freedSel = root.querySelector("#demo-freed");
  const targetSel = root.querySelector("#demo-target");
  const targetInput = root.querySelector("#demo-target-value");
  const editorPane = root.querySelector("#demo-editor-pane");
  const editorEl = root.querySelector("#demo-editor");
  const runBtn = root.querySelector("#demo-run");
  const editorOutEl = root.querySelector("#demo-editor-out");
  const srcTitle = root.querySelector("#demo-src-title");
  const leftTitle = root.querySelector("#demo-left-title");

  // Populate the reverse-mode selectors.
  for (const i of INPUTS) {
    const o = document.createElement("option");
    o.value = i.name;
    o.textContent = i.label;
    freedSel.appendChild(o);
  }
  freedSel.value = freed;
  for (const name of REVERSE_TARGETS) {
    const out = OUTPUTS.find((o) => o.name === name);
    const o = document.createElement("option");
    o.value = name;
    o.textContent = out.label;
    targetSel.appendChild(o);
  }
  targetSel.value = target;
  targetInput.value = targetValue;

  // Build the sliders.
  for (const i of INPUTS) {
    const row = document.createElement("label");
    row.className = "slider-row";
    row.dataset.name = i.name;
    row.innerHTML =
      `<label><span>${i.label}</span><output>${fmt(i.def)}</output></label>` +
      `<input type="range" min="${i.min}" max="${i.max}" step="${i.step}" value="${i.def}">`;
    const range = row.querySelector("input");
    const out = row.querySelector("output");
    setFill(range);
    range.addEventListener("input", () => {
      state[i.name] = parseFloat(range.value);
      out.textContent = fmt(state[i.name]);
      setFill(range);
      recompute();
    });
    slidersEl.appendChild(row);
  }

  function setMode(next) {
    mode = next;
    modeBtns.forEach((b) => b.setAttribute("aria-selected", String(b.dataset.mode === next)));
    const isEditor = next === "editor";
    const isReverse = next === "reverse";
    reverseEl.hidden = !isReverse;
    slidersEl.hidden = isEditor;
    editorPane.hidden = !isEditor;
    outputsEl.hidden = isEditor; // fixed sizing-model grid
    editorOutEl.hidden = !isEditor; // generic settled-values grid
    srcTitle.hidden = isEditor;
    srcEl.hidden = isEditor;
    leftTitle.textContent = isEditor ? "Your model" : "Inputs";
    const resetBtn = root.querySelector("#demo-reset");
    if (resetBtn) resetBtn.hidden = isEditor;
    // In reverse mode the freed input's slider is disabled (the solver sets it).
    for (const row of slidersEl.querySelectorAll(".slider-row")) {
      const isFreed = isReverse && row.dataset.name === freed;
      row.classList.toggle("is-freed", isFreed);
      row.querySelector("input").disabled = isFreed;
    }
    if (isEditor) runEditor();
    else recompute();
  }

  function runEditor() {
    const source = editorEl.value;
    const t0 = performance.now();
    let res;
    try {
      res = solver.solve(source);
    } catch (e) {
      statusEl.textContent = `solver error: ${e.message}`;
      statusEl.className = "status-bad";
      return;
    }
    const us = (performance.now() - t0) * 1000;
    if (!res.ok) {
      statusEl.textContent = formatErr(res);
      statusEl.className = "status-bad";
      editorOutEl.innerHTML = "";
      timingEl.textContent = "";
      return;
    }
    const keys = Object.keys(res.values).sort();
    editorOutEl.innerHTML = keys
      .map(
        (k) =>
          `<div class="output"><div class="output-name">${escapeHtml(k)}</div>` +
          `<div class="output-val">${fmt(res.values[k])}</div></div>`
      )
      .join("");
    statusEl.textContent = `Solved — ${keys.length} value${keys.length === 1 ? "" : "s"}`;
    statusEl.className = "status-ok";
    timingEl.textContent = `${us < 1 ? us.toFixed(2) : Math.round(us)} µs in WebAssembly`;
  }

  editorEl.value = EDITOR_EXAMPLE;
  runBtn.addEventListener("click", runEditor);
  editorEl.addEventListener("keydown", (e) => {
    if ((e.ctrlKey || e.metaKey) && e.key === "Enter") {
      e.preventDefault();
      runEditor();
    }
  });

  modeBtns.forEach((b) => b.addEventListener("click", () => setMode(b.dataset.mode)));
  freedSel.addEventListener("change", () => {
    freed = freedSel.value;
    setMode("reverse");
  });
  targetSel.addEventListener("change", () => {
    target = targetSel.value;
    recompute();
  });
  targetInput.addEventListener("input", () => {
    targetValue = parseFloat(targetInput.value) || 0;
    recompute();
  });

  const resetBtn = root.querySelector("#demo-reset");
  if (resetBtn)
    resetBtn.addEventListener("click", () => {
      for (const i of INPUTS) {
        state[i.name] = i.def;
        const row = slidersEl.querySelector(`.slider-row[data-name="${i.name}"]`);
        const range = row.querySelector("input");
        range.value = i.def;
        setFill(range);
        row.querySelector("output").textContent = fmt(i.def);
      }
      recompute();
    });

  function recompute() {
    let bindings;
    if (mode === "forward") {
      bindings = {};
      for (const i of INPUTS) bindings[i.name] = num(i.name, state[i.name]);
    } else {
      // Bind every input except the freed one, plus the target output.
      bindings = {};
      for (const i of INPUTS) if (i.name !== freed) bindings[i.name] = num(i.name, state[i.name]);
      bindings[target] = targetValue;
    }
    const source = buildSource(bindings);
    srcEl.textContent = source;

    const t0 = performance.now();
    let res;
    try {
      res = solver.solve(source);
    } catch (e) {
      statusEl.textContent = `solver error: ${e.message}`;
      statusEl.className = "status-bad";
      return;
    }
    const us = (performance.now() - t0) * 1000;

    if (!res.ok) {
      statusEl.textContent = `${res.stage}: ${res.error}`;
      statusEl.className = "status-bad";
      outputsEl.querySelectorAll(".output-val").forEach((el) => (el.textContent = "—"));
      return;
    }

    const v = res.values;
    renderOutputs(v);
    if (mode === "reverse") {
      const solved = v[freed];
      const label = INPUTS.find((i) => i.name === freed).label;
      statusEl.textContent = `Solved backward: ${label} = ${fmt(solved)}`;
      statusEl.className = "status-ok";
      // Reflect the solved value onto its (disabled) slider.
      const row = slidersEl.querySelector(`.slider-row[data-name="${freed}"]`);
      if (row && isFinite(solved)) {
        const range = row.querySelector("input");
        range.value = solved;
        setFill(range);
        row.querySelector("output").textContent = fmt(solved);
      }
    } else {
      statusEl.textContent = "Solved forward";
      statusEl.className = "status-ok";
    }
    timingEl.textContent = `${us < 1 ? us.toFixed(2) : Math.round(us)} µs in WebAssembly`;
  }

  function renderOutputs(values) {
    if (!outputsEl.dataset.built) {
      outputsEl.innerHTML = "";
      for (const o of OUTPUTS) {
        const cell = document.createElement("div");
        cell.className = "output";
        cell.dataset.name = o.name;
        cell.innerHTML = `<div class="output-name">${o.label}</div><div class="output-val">—</div>`;
        outputsEl.appendChild(cell);
      }
      outputsEl.dataset.built = "1";
    }
    for (const o of OUTPUTS) {
      const cell = outputsEl.querySelector(`.output[data-name="${o.name}"]`);
      cell.querySelector(".output-val").textContent = fmt(values[o.name]);
    }
  }

  setMode("forward");
}

const root = document.querySelector("#demo");
if (root) mountDemo(root);
