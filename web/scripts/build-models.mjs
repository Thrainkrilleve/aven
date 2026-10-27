#!/usr/bin/env node
// build-models.mjs — Regenerates the playground's WebAssembly models from examples/.
//
// For each model: runs `avenc emit-wasm` on the example, reads the input/output ABI
// from the emitted WAT, extracts the forward block source and the solve-block defaults,
// then runs the WASM on those defaults and checks every output against `avenc run --json`.
// Writes public/models/<key>.wasm and public/models/manifest.json.
//
// Usage: node scripts/build-models.mjs   (AVENC=/path/to/avenc to override the binary)

import { execFileSync } from "node:child_process";
import { readFileSync, writeFileSync, mkdirSync, mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const webDir = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const repoDir = resolve(webDir, "..");
const avenc = process.env.AVENC || join(repoDir, "target", "release", "avenc");
const outDir = join(webDir, "public", "models");

const MODELS = [
  { key: "quadcopter", example: "examples/quadcopter_avionics.aven", title: "Quadcopter flight envelope" },
  { key: "server_capacity", example: "examples/server_capacity.aven", title: "Server capacity (Little's Law)" },
  { key: "portfolio", example: "examples/portfolio_rebalancing.aven", title: "Portfolio allocation" },
];

const run = (...args) => execFileSync(avenc, args, { cwd: repoDir, encoding: "utf8" });

function extractBlock(source, keyword, name) {
  const start = source.search(new RegExp(`^${keyword}\\s+${name ?? "\\w+"}\\b`, "m"));
  if (start < 0) throw new Error(`no '${keyword}' block found`);
  const end = source.indexOf("\n}", start);
  return source.slice(start, end + 2);
}

function parseAbi(wat) {
  // The first function is the direct-call form: its params are the inputs (sorted)
  // and its locals are the outputs (in evaluation order), matching aven_solve's buffers.
  const direct = wat.slice(wat.indexOf("(func $"), wat.indexOf("(func (export \"aven_solve\")"));
  const names = (kind) => [...direct.matchAll(new RegExp(`\\(${kind} \\$(\\w+) f64\\)`, "g"))].map((m) => m[1]);
  const block = wat.match(/\| Block: (\w+)/)[1];
  return { block, inputs: names("param"), outputs: names("local") };
}

function parseDefaults(solveBlock) {
  const defaults = {};
  for (const m of solveBlock.matchAll(/^\s*(\w+)\s*=\s*(-?[\d.]+(?:e-?\d+)?)\s*$/gm)) {
    defaults[m[1]] = Number(m[2]);
  }
  return defaults;
}

async function runWasm(bytes, inputs) {
  const { instance } = await WebAssembly.instantiate(bytes, {});
  const mem = instance.exports.memory.buffer;
  new Float64Array(mem, 0, inputs.length).set(inputs);
  instance.exports.aven_solve(0, 1024);
  return Array.from(new Float64Array(mem, 1024, 64));
}

mkdirSync(outDir, { recursive: true });
const tmp = mkdtempSync(join(tmpdir(), "aven-models-"));
const manifest = { generated_by: run("--version").trim(), models: [] };
let failed = false;

for (const m of MODELS) {
  const source = readFileSync(join(repoDir, m.example), "utf8");
  const wasmPath = join(tmp, `${m.key}.wasm`);
  run("emit-wasm", m.example, "-o", wasmPath);
  const wasm = readFileSync(wasmPath);
  const { block, inputs, outputs } = parseAbi(run("emit-wasm", m.example, "--wat"));
  const defaults = parseDefaults(extractBlock(source, "solve"));

  const missing = inputs.filter((n) => !(n in defaults));
  if (missing.length) throw new Error(`${m.key}: solve block has no value for ${missing.join(", ")}`);

  // Cross-check: the WASM must reproduce the native solver on the example's own scenario.
  const native = JSON.parse(run("run", m.example, "--json")).env;
  const got = await runWasm(wasm, inputs.map((n) => defaults[n]));
  for (const [i, name] of outputs.entries()) {
    const want = native[name];
    const ok = want !== undefined && Math.abs(got[i] - want) <= 1e-9 * Math.max(1, Math.abs(want));
    if (!ok) {
      failed = true;
      console.error(`✗ ${m.key}.${name}: wasm=${got[i]} native=${want}`);
    }
  }

  writeFileSync(join(outDir, `${m.key}.wasm`), wasm);
  manifest.models.push({
    key: m.key,
    title: m.title,
    example: m.example,
    block,
    source: extractBlock(source, "forward", block),
    inputs: inputs.map((name) => ({ name, default: defaults[name] })),
    outputs,
  });
  console.log(`✓ ${m.key}: ${block} — ${inputs.length} inputs, ${outputs.length} outputs match avenc run`);
}

if (failed) process.exit(1);
writeFileSync(join(outDir, "manifest.json"), JSON.stringify(manifest, null, 2) + "\n");
console.log(`Wrote ${join("public", "models", "manifest.json")}`);
