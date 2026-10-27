// Compiled model modules: wrangler bundles .wasm imports as a WebAssembly.Module
declare module "*.wasm" {
  const module: WebAssembly.Module;
  export default module;
}
