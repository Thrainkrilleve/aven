---
name: aven-toolchain
description: Expertise in inspecting, compiling, solving, and diagnosing Aven models (.aven) using the native avenc toolchain.
---

# Aven Toolchain Skill

Use this skill when reading, creating, modifying, or diagnosing `.aven` models and their interactions with the Aven compiler (`avenc`).

For the comprehensive grammar specification, anti-patterns, and cookbook templates, see [docs/AVEN_AI_MODEL_GUIDE.md](../../../docs/AVEN_AI_MODEL_GUIDE.md).

## Critical AI Guardrails
1. **Units follow the number, without brackets:** `velocity = 250 km/h`, `signal velocity : km/h`. Annotated values are converted to SI before solving, so formulas written for °C or hPa need plain numbers. Aven has NO array slicing or indexing syntax.
2. **Exponentiation Operator:** Always use `^` (e.g. `x ^ 1.6`), NEVER `pow(x, y)` or `x ** y`.
3. **Block Structure:** Models are written with `relation <Name> { ... }` and `solve <Name> with { ... }`.
4. **No Imperative Control Flow:** Equations are simultaneous mathematical constraints. Do not write `if/else`, `while`, or `return`.
5. **Discrete Optimization:** Declare discrete variables with `int <varname>` and provide an objective (`minimize(cost)`).

## Capabilities
1. **Model Validation:** Run `cargo run --release -- check <file.aven>` to verify syntax, dimensional unit homogeneity, and degree-of-freedom determinacy.
2. **Coupled & Mixed-Integer Solving:** Run `cargo run --release -- run <file.aven>` to solve continuous physical systems or optimize discrete integer variables.
3. **Causal Lineage & Provenance:** Run `cargo run --release -- why <file.aven> <variable>` to trace back the exact derivation tree of settled values.
4. **C99 Transpilation:** Run `cargo run --release -- build <file.aven> --emit-c -o out.c` to emit dependency-free C99 for the model's `forward` block.
