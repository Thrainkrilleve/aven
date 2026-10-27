// wasm_engine.js — Runs models emitted by `avenc emit-wasm` in the browser.
//
// ABI (see src/codegen_wasm.rs): aven_solve(in_ptr, out_ptr) reads one f64 per input,
// in sorted-name order, and writes one f64 per output, in evaluation order. The order
// for each model is recorded in models/manifest.json by scripts/build-models.mjs.

const IN_PTR = 0;
const OUT_PTR = 1024;

export class AvenWasmEngine {
  constructor() {
    this.instances = new Map();
  }

  async load(model) {
    if (this.instances.has(model.key)) return;
    const response = await fetch(`/models/${model.key}.wasm`);
    if (!response.ok) throw new Error(`Failed to load model '${model.key}': HTTP ${response.status}`);
    const { instance } = await WebAssembly.instantiate(await response.arrayBuffer(), {});
    this.instances.set(model.key, instance);
  }

  /** Solves `model` for the given values (an object keyed by input name). */
  solve(model, values) {
    const instance = this.instances.get(model.key);
    if (!instance) throw new Error(`Model '${model.key}' is not loaded`);
    const memory = instance.exports.memory.buffer;
    new Float64Array(memory, IN_PTR, model.inputs.length).set(model.inputs.map((i) => values[i.name]));
    instance.exports.aven_solve(IN_PTR, OUT_PTR);
    const raw = new Float64Array(memory, OUT_PTR, model.outputs.length);
    return Object.fromEntries(model.outputs.map((name, i) => [name, raw[i]]));
  }

  /** Mean wall-clock time of one aven_solve call on the inputs last passed to solve(), in µs. */
  timeSolve(model, iterations = 2000) {
    const solve = this.instances.get(model.key).exports.aven_solve;
    const t0 = performance.now();
    for (let i = 0; i < iterations; i++) solve(IN_PTR, OUT_PTR);
    return ((performance.now() - t0) * 1000) / iterations;
  }
}
