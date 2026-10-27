// aven_math.js — Host math for models emitted by `avenc emit-wasm`.
//
// WebAssembly 1.0 has no exp, log, trig or general pow instruction, so a model that uses
// them imports these from module "math" (see src/codegen_wasm.rs). Models that don't use
// them import nothing. Pass this object as the import object to WebAssembly.instantiate.

export const AVEN_HOST_IMPORTS = {
  math: {
    exp: Math.exp,
    ln: Math.log,
    sin: Math.sin,
    cos: Math.cos,
    tan: Math.tan,
    pow: Math.pow,
  },
};
