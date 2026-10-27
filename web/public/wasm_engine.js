// wasm_engine.js — Client-Side Aven WebAssembly Solver Engine
// Runs pure standard WebAssembly Core 1.0 bytecode directly in the browser

export class AvenWasmEngine {
  constructor() {
    this.moduleCache = new Map();
  }

  async loadModel(modelName) {
    if (this.moduleCache.has(modelName)) {
      return this.moduleCache.get(modelName);
    }

    const response = await fetch(`/models/${modelName}.wasm`);
    if (!response.ok) {
      throw new Error(`Failed to load WASM model '${modelName}': HTTP ${response.status}`);
    }

    const buffer = await response.arrayBuffer();
    const wasmModule = await WebAssembly.compile(buffer);
    const instance = await WebAssembly.instantiate(wasmModule, {});

    const bundle = { module: wasmModule, instance };
    this.moduleCache.set(modelName, bundle);
    return bundle;
  }

  solve(modelName, inputsArray, outputCount = 10) {
    const bundle = this.moduleCache.get(modelName);
    if (!bundle) {
      throw new Error(`Model '${modelName}' not loaded. Call loadModel() first.`);
    }

    const instance = bundle.instance;
    const memory = instance.exports.memory;
    const avenSolve = instance.exports.aven_solve;

    const t0 = performance.now();

    // In-memory buffer pointers:
    // Inputs start at offset 0; outputs start at offset 1024
    const inPtr = 0;
    const outPtr = 1024;

    const inView = new Float64Array(memory.buffer, inPtr, inputsArray.length);
    for (let i = 0; i < inputsArray.length; i++) {
      inView[i] = inputsArray[i];
    }

    // Execute solver
    const status = avenSolve(inPtr, outPtr);

    const t1 = performance.now();
    const solveTimeUs = (t1 - t0) * 1000.0;

    const outView = new Float64Array(memory.buffer, outPtr, outputCount);
    const outputs = Array.from(outView);

    return {
      status,
      solveTimeUs: Math.max(0.1, parseFloat(solveTimeUs.toFixed(2))),
      outputs,
    };
  }
}
