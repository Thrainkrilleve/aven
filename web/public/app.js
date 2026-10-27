// app.js — Interactive UI & Reactive State for Aven Scientific Web Application
import { AvenWasmEngine } from './wasm_engine.js';

const wasmEngine = new AvenWasmEngine();

// Model Configurations
const MODELS = {
  quadcopter: {
    title: "Quadcopter Avionics & Aerodynamic Thrust",
    file: "quadcopter",
    code: `relation QuadcopterAvionics {
    // Aerodynamic thrust: T = 4 * c_T * rho * omega^2 * D^4
    thrust_n = 4.0 * thrust_coeff * air_density * (motor_omega ^ 2) * (prop_diameter ^ 4)
    twr = thrust_n / (total_mass * 9.80665)
    climb_accel = (thrust_n - (total_mass * 9.80665)) / total_mass
    
    // Battery Internal Resistance Sag: V_term = V_oc - I * R_int
    terminal_voltage = battery_voc - (motor_current * internal_resistance)
    electrical_power = terminal_voltage * motor_current
}`,
    inputs: [
      { id: 'omega', name: 'Rotor Omega (rad/s)', min: 600, max: 2000, step: 10, val: 1350 },
      { id: 'prop_d', name: 'Prop Diameter (m)', min: 0.15, max: 0.40, step: 0.01, val: 0.254 },
      { id: 'mass', name: 'Total Mass (kg)', min: 1.0, max: 8.0, step: 0.1, val: 2.5 },
      { id: 'voc', name: 'Battery Open-Circuit (V)', min: 7.4, max: 25.2, step: 0.2, val: 11.1 },
      { id: 'amps', name: 'Motor Current (A)', min: 10, max: 120, step: 1, val: 55 },
    ],
    // Sorted input vector mapping for WASM:
    // [air_density, internal_resistance, voc, cell_count, current, kv, omega, prop_d, c_T, mass]
    getInputsVector: (state) => [
      1.225, // air_density
      0.040, // internal_resistance
      state.voc,
      3.0,   // cell_count
      state.amps,
      920.0, // kv
      state.omega,
      state.prop_d,
      0.115, // c_T
      state.mass
    ],
    outputsMeta: [
      { name: "Thrust", unit: "N", index: 0, decimals: 2 },
      { name: "Thrust-to-Weight", unit: "TWR", index: 1, decimals: 2 },
      { name: "Vertical Climb", unit: "m/s²", index: 2, decimals: 2 },
      { name: "Terminal Voltage", unit: "V", index: 3, decimals: 2 },
      { name: "Electrical Power", unit: "W", index: 4, decimals: 2 },
    ]
  },

  server_capacity: {
    title: "Little's Law Server Capacity & Network Wire-Rate",
    file: "server_capacity",
    code: `relation WorkerScaling {
    raw_workers       = (reqs_per_second * avg_latency_ms) / 1000.0
    allocated_workers = ceil(raw_workers / target_utilization)
    total_ram_needed  = (allocated_workers * (ram_per_worker + socket_buffer_mb)) + os_kernel_ram_mb
    payload_gbps      = (reqs_per_second * avg_payload_kb * 8.0) / 1000000.0
    wire_rate_gbps    = payload_gbps * framing_overhead_ratio
}`,
    inputs: [
      { id: 'rps', name: 'Arrival Rate (req/s)', min: 5000, max: 150000, step: 5000, val: 50000 },
      { id: 'latency', name: 'Avg Latency (ms)', min: 5, max: 100, step: 1, val: 20 },
      { id: 'util', name: 'Target Utilization', min: 0.4, max: 0.95, step: 0.05, val: 0.70 },
      { id: 'payload', name: 'Avg Payload (KB)', min: 16, max: 256, step: 8, val: 64 },
    ],
    // Sorted input vector mapping:
    // [latency, payload, framing_overhead, os_ram, ram_worker, rps, socket_buf, util]
    getInputsVector: (state) => [
      state.latency,
      state.payload,
      1.04,   // framing overhead
      4096.0, // os kernel ram
      16.0,   // ram per worker
      state.rps,
      4.0,    // socket buffer
      state.util
    ],
    outputsMeta: [
      { name: "Raw Workers", unit: "threads", index: 0, decimals: 0 },
      { name: "Allocated Workers", unit: "nodes", index: 1, decimals: 0 },
      { name: "Total RAM Needed", unit: "MB", index: 2, decimals: 0 },
      { name: "Payload Bandwidth", unit: "Gbps", index: 3, decimals: 3 },
      { name: "Wire-Rate Bandwidth", unit: "Gbps", index: 4, decimals: 3 },
    ]
  },

  portfolio: {
    title: "Multi-Asset Markowitz Risk-Return Portfolio",
    file: "portfolio",
    code: `relation PortfolioModel {
    port_return     = (w1 * ret_1) + (w2 * ret_2)
    port_variance   = ((w1^2) * (vol_1^2)) + ((w2^2) * (vol_2^2)) + (2 * w1 * w2 * covar_1_2)
    port_volatility = sqrt(port_variance)
    sharpe_ratio    = (port_return - r_rf) / port_volatility
    weight_sum      = w1 + w2
}`,
    inputs: [
      { id: 'ret1', name: 'Asset 1 Return (%)', min: 2, max: 20, step: 0.5, val: 8.0 },
      { id: 'ret2', name: 'Asset 2 Return (%)', min: 4, max: 35, step: 0.5, val: 14.0 },
      { id: 'vol1', name: 'Asset 1 Volatility (%)', min: 5, max: 30, step: 0.5, val: 10.0 },
      { id: 'vol2', name: 'Asset 2 Volatility (%)', min: 10, max: 50, step: 0.5, val: 22.0 },
    ],
    getInputsVector: (state) => {
      const r1 = state.ret1 / 100.0;
      const r2 = state.ret2 / 100.0;
      const v1 = state.vol1 / 100.0;
      const v2 = state.vol2 / 100.0;
      const covar = 0.25 * v1 * v2;
      // [cash_initial, corr_1_2, covar_1_2, p1_new_units, p2_new_units, price_1, price_2, r_rf, ret_1, ret_2, vol_1, vol_2]
      return [
        100000.0,
        0.25,
        covar,
        700.0,
        300.0,
        100.0,
        100.0,
        0.035, // risk-free rate 3.5%
        r1,
        r2,
        v1,
        v2
      ];
    },
    outputsMeta: [
      { name: "Expected Return", unit: "%", index: 0, decimals: 2, scale: 100 },
      { name: "Portfolio Volatility", unit: "%", index: 2, decimals: 2, scale: 100 },
      { name: "Sharpe Ratio", unit: "ratio", index: 3, decimals: 4 },
      { name: "Total Weight Sum", unit: "1.0", index: 5, decimals: 4 },
    ]
  },

  openplc: {
    title: "OpenPLC Manufacturing Line & Supervisory Safety",
    file: "openplc",
    code: `relation OpenPLCSupervisor {
    // PLC Cycle Scan Time & Cabinet Thermal Dissipation
    scan_ms        = 7.5 + (cpu_load_pct * 0.04) + (io_points * 0.08)
    power_w        = 120.0 + (io_points * 2.8) + (cpu_load_pct * 0.75)
    temp_c         = 25.0 + (power_w * 0.085)
    
    // Production Line Throughput & Defect Yield Invariants
    throughput_pph = target_rate - (scan_ms * 1.85)
    quality_pct    = 99.9 - (scan_ms * 0.065) - ((temp_c - 25.0) * 0.04)
    
    // Invariant Bounds: scan_ms <= 18.0 ms, temp_c <= 55.0 °C
}`,
    inputs: [
      { id: 'io', name: 'Active I/O Points', min: 8, max: 64, step: 2, val: 28 },
      { id: 'cpu', name: 'PLC CPU Load (%)', min: 10, max: 95, step: 5, val: 35 },
      { id: 'rate', name: 'Target Rate (parts/h)', min: 100, max: 250, step: 5, val: 180 },
    ],
    // Sorted input vector mapping for WASM: [cpu_load_pct, io_points, target_rate]
    getInputsVector: (state) => [
      state.cpu,
      state.io,
      state.rate
    ],
    outputsMeta: [
      { name: "Scan Cycle Time", unit: "ms", index: 0, decimals: 2 },
      { name: "Cabinet Power", unit: "W", index: 1, decimals: 1 },
      { name: "Enclosure Temp", unit: "°C", index: 2, decimals: 1 },
      { name: "Line Throughput", unit: "parts/h", index: 3, decimals: 1 },
      { name: "Quality Yield", unit: "%", index: 4, decimals: 2 },
    ]
  }
};

let currentModelKey = 'quadcopter';
let currentState = {};

// Initialize Application
document.addEventListener('DOMContentLoaded', async () => {
  setupModelTabs();
  setupTerminalCopy();
  setupShareButton();
  loadLiveStats();

  // Load initial model from URL if present (?model=...)
  const params = new URLSearchParams(window.location.search);
  const requestedModel = params.get('model');
  if (requestedModel && MODELS[requestedModel]) {
    currentModelKey = requestedModel;
  }

  await activateModel(currentModelKey);
});

// Setup Model Switching Tabs
function setupModelTabs() {
  const buttons = document.querySelectorAll('.model-btn');
  buttons.forEach(btn => {
    btn.addEventListener('click', async () => {
      buttons.forEach(b => b.classList.remove('active'));
      btn.classList.add('active');
      const modelKey = btn.dataset.model;
      await activateModel(modelKey);
    });
  });
}

// Activate Model and Compile WASM
async function activateModel(modelKey) {
  currentModelKey = modelKey;
  const config = MODELS[modelKey];

  // Update Code Preview
  const codeEl = document.getElementById('code-display');
  if (codeEl) codeEl.textContent = config.code;

  // Initialize State
  currentState = {};
  config.inputs.forEach(inp => {
    currentState[inp.id] = inp.val;
  });

  // Render Sliders
  renderSliders(config);

  // Load WASM Module
  try {
    await wasmEngine.loadModel(config.file);
    runSolve();
  } catch (err) {
    console.error("WASM Load Error:", err);
  }
}

// Render Reactive Sliders
function renderSliders(config) {
  const container = document.getElementById('sliders-container');
  if (!container) return;

  container.innerHTML = '';
  config.inputs.forEach(inp => {
    const group = document.createElement('div');
    group.className = 'slider-group';
    group.innerHTML = `
      <div class="slider-header">
        <span class="slider-label">${inp.name}</span>
        <span class="slider-val" id="val-${inp.id}">${inp.val}</span>
      </div>
      <input type="range" class="slider-input" id="input-${inp.id}" 
             min="${inp.min}" max="${inp.max}" step="${inp.step}" value="${inp.val}">
    `;

    const slider = group.querySelector('.slider-input');
    slider.addEventListener('input', (e) => {
      const val = parseFloat(e.target.value);
      currentState[inp.id] = val;
      group.querySelector(`#val-${inp.id}`).textContent = val;
      runSolve();
    });

    container.appendChild(group);
  });
}

// Execute In-Browser WASM Solve
function runSolve() {
  const config = MODELS[currentModelKey];
  const vector = config.getInputsVector(currentState);

  try {
    const res = wasmEngine.solve(config.file, vector, 10);
    renderSettledOutputs(config, res.outputs, res.solveTimeUs);
  } catch (err) {
    console.error("Solve error:", err);
  }
}

// Render Output Telemetry Cards
function renderSettledOutputs(config, outputs, latencyUs) {
  const container = document.getElementById('settled-outputs');
  if (!container) return;

  // Update Latency Badge
  const latencyBadge = document.getElementById('solve-latency');
  if (latencyBadge) {
    latencyBadge.textContent = `${latencyUs.toFixed(1)} µs`;
  }

  container.innerHTML = '';
  config.outputsMeta.forEach(out => {
    let rawVal = outputs[out.index] || 0.0;
    if (out.scale) rawVal *= out.scale;
    const formatted = rawVal.toFixed(out.decimals);

    const card = document.createElement('div');
    card.className = 'output-card';
    card.innerHTML = `
      <div class="output-name">${out.name} (${out.unit})</div>
      <div class="output-value">${formatted}</div>
    `;
    container.appendChild(card);
  });
}

// Setup Terminal Copy Button & Tab Switching
function setupTerminalCopy() {
  const tabs = document.querySelectorAll('.tab-btn');
  const cmdEl = document.getElementById('install-cmd');
  const copyBtn = document.getElementById('copy-install-btn');

  const commands = {
    curl: 'curl -proto \'=https\' --tlsv1.2 -sSf https://avenc.dev/install.sh | sh',
    powershell: 'iwr -useb https://avenc.dev/install.ps1 | iex',
    cargo: 'cargo install --git https://github.com/Thrainkrilleve/aven.git'
  };

  tabs.forEach(tab => {
    tab.addEventListener('click', () => {
      tabs.forEach(t => t.classList.remove('active'));
      tab.classList.add('active');
      const tool = tab.dataset.tool;
      if (cmdEl && commands[tool]) {
        cmdEl.textContent = commands[tool];
      }
    });
  });

  if (copyBtn && cmdEl) {
    copyBtn.addEventListener('click', async () => {
      await navigator.clipboard.writeText(cmdEl.textContent.trim());
      copyBtn.textContent = 'Copied!';
      setTimeout(() => { copyBtn.textContent = 'Copy'; }, 2000);
    });
  }
}

// Setup Model Share Button (D1 Database integration)
function setupShareButton() {
  const shareBtn = document.getElementById('share-model-btn');
  if (!shareBtn) return;

  shareBtn.addEventListener('click', async () => {
    const config = MODELS[currentModelKey];
    shareBtn.textContent = 'Sharing...';

    try {
      const res = await fetch('/api/v1/models/share', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          title: config.title,
          category: currentModelKey,
          source_code: config.code,
        })
      });

      const data = await res.json();
      if (data.shortUrl) {
        await navigator.clipboard.writeText(data.shortUrl);
        shareBtn.textContent = 'Link Copied!';
      } else {
        shareBtn.textContent = 'Shared!';
      }
    } catch (err) {
      console.error("Share error:", err);
      shareBtn.textContent = 'Share Failed';
    }

    setTimeout(() => { shareBtn.textContent = 'Share Model'; }, 3000);
  });
}

// Fetch Live Platform Statistics
async function loadLiveStats() {
  try {
    const res = await fetch('/api/v1/stats');
    if (!res.ok) return;
    const stats = await res.json();

    const statPackages = document.getElementById('stat-packages');
    const statSolves = document.getElementById('stat-solves');
    const statLatency = document.getElementById('stat-latency');

    if (statPackages) statPackages.textContent = stats.packages_indexed;
    if (statSolves) statSolves.textContent = `${stats.total_solves_logged.toLocaleString()}+`;
    if (statLatency) statLatency.textContent = `< ${stats.median_latency_us} µs`;
  } catch (err) {
    console.warn("Stats error:", err);
  }
}
