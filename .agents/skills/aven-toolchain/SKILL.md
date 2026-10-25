---
name: aven-toolchain
description: Expertise in inspecting, compiling, solving, and diagnosing Aven models (.aven) using the native avenc toolchain.
---

# Aven Toolchain Skill

Use this skill when reading, creating, modifying, or diagnosing `.aven` models and their interactions with the Aven compiler (`avenc`).

## Capabilities
1. **Model Validation:** Run `cargo run --release -- check <file.aven>` to verify syntax, dimensional unit homogeneity, and degree-of-freedom determinacy.
2. **Coupled & Mixed-Integer Solving:** Run `cargo run --release -- run <file.aven>` to solve continuous physical systems or optimize discrete integer variables.
3. **Causal Lineage & Provenance:** Run `cargo run --release -- why <file.aven> <variable>` to trace back the exact derivation tree of settled values.
4. **C99 Transpilation:** Run `cargo run --release -- build <file.aven> --emit-c -o out.c` to emit zero-dependency C code for real-time sliding windows.
