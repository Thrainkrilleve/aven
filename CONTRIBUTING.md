# Contributing to Aven

Thank you for contributing to **Aven**!

Aven replaces fragile imperative code with verified mathematical invariants and cryptographic audit trails. Because Aven has the possibility to be used in safety-critical, high-reliability engineering domains (aerospace, industrial robotics, quantitative finance), contributions are held to rigorous standards. Please review [STANDARDS.md](STANDARDS.md) for the complete numerical, cryptographic, and architectural specifications.

---

## Architectural Ground Rules

1. **Rust Compiles, C Runs**:
   - All compiler logic, solver algorithms, dimensional analysis, SMT export, package management, and WASM transpilation live in the pure Rust compiler (`avenc`).
   - The deployment target runtime (`aeric/`) is a thin C99 execution layer.
   - Never write compiler features in C.
2. **Zero External Crates (`std` Only)**:
   - The entire `avenc` compiler and toolchain is built exclusively on the Rust standard library (`std`).
   - Do not add third-party dependencies to `Cargo.toml`.
3. **Deterministic Numerical Invariance**:
   - Solves must produce bit-for-bit identical outputs across runs for identical inputs.
   - Any floating-point operation must be protected against NaN and $\pm\infty$ contamination.
4. **Freshness & Provenance**:
   - Every computed value must carry its causal ancestor graph and freshness metadata in the fact ledger.

---

## Development Setup

```bash
# Clone the repository
git clone https://git.thrainkrill.space/thrain/aven.git
cd aven

# Check compilation
cargo check

# Run full test suite in release mode
cargo test --release
```

---

## Testing Guidelines

Every new feature or bugfix must include:
1. **Deterministic Golden Test**: Explicit, reproducible inputs checking mathematical convergence and ledger records.
2. **Randomized Stress Tests**: A minimum of 3 randomized tests using OS entropy (`SystemTime::now()` + PID) so data is never recycled.
3. **Subprocess CLI Tests**: Validation that CLI command invocations (`avenc run`, `avenc build`, `avenc emit-wasm`, `avenc pkg`, `avenc debug`) produce expected exit codes and output formats.

---

## Submitting Pull Requests

1. Fork the repository and create your feature branch from `main`.
2. Ensure `cargo test --release` passes 100% with zero warnings (the SMT and safety-case tests need `z3` or `cvc5` on `PATH`), and that `python3 scripts/check_docs.py` reports no problems: every Aven example in the docs and on the website must compile, and documented outputs must match `avenc run`.
3. If you change a `forward` block in `examples/` or the WebAssembly emitter, run `npm run build:models` in `web/` so the website playground matches the compiler.
4. Open a Pull Request describing:
   - The motivation for the change.
   - The mathematical relations or physical invariants touched.
   - Test results across all test suites.
