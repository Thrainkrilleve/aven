# Aven Engineering & Mathematical Standards (STANDARDS.md)

This document specifies the authoritative engineering, numerical, cryptographic, and architectural standards for the **Aven** language, compiler (`avenc`), runtime targets, and package ecosystem.

All implementations, compiler modifications, codegen backends, and community contributions MUST comply with these standards.

---

## 1. Architectural Separation ("Rust Compiles, C Runs")

### 1.1 The Compiler Layer (`avenc`)
- **Language**: Pure standard library Rust (`std`).
- **Zero Third-Party Dependencies**: The compiler and developer toolchain must compile with `cargo build --release` using exclusively `std`. No external crates are permitted in `Cargo.toml`.
- **Scope**: Lexing, AST construction, topological sorting, symbolic differentiation, interval arithmetic, SMT-LIB2 transpilation, C code emission, WebAssembly Core 1.0 binary generation, temporal debugging, and package management are strictly the responsibility of `avenc`.

### 1.2 The Execution Runtime Layer (`aeric/`)
- **Language**: Standard ANSI C99 (`-std=c99 -pedantic -Wall -Werror`).
- **Zero-Allocation Invariants**: Inner execution kernels, relaxation loops, and memory-mapped shared telemetry rings MUST NOT invoke dynamic heap allocations (`malloc`, `free`, `realloc`) during steady-state solving cycles.
- **Header Standards**: Freestanding ANSI C library references limited strictly to `<stdint.h>`, `<stdbool.h>`, and `<math.h>`.
- **Architectural Boundary**: Never implement language parsing, AST representation, or compiler optimizations in C.

---

## 2. Mathematical Rigor & Numerical Stability

### 2.1 Floating-Point Semantics
- **Precision**: Calculations use standard IEEE-754 double-precision 64-bit binary floating-point (`f64`).
- **Domain Fault Guards**: Math operations must actively guard against undefined mathematical domains:
  - Division by zero: Denominators satisfying $|d| < 10^{-15}$ must generate structured domain errors or soft-epsilon clamping.
  - Non-positive logarithms: Arguments $x \le 0$ to $\ln(x)$ or $\log_{10}(x)$ must reject evaluation prior to hardware NaN emission.
  - Square roots of negatives: Arguments $x < 0$ to $\sqrt{x}$ must raise domain violations.
- **NaN / $\pm\infty$ Rejection**: Any intermediate non-finite floating-point state immediately triggers an `EvalError::DomainViolation` or `EvalError::NumericalDivergence`. Unchecked NaN propagation is considered a critical compiler defect.

### 2.2 Relaxation & Root-Finding Convergence
- **Jacobian Exactness**: Coupled multidimensional systems must evaluate derivatives via exact symbolic differentiation; finite-difference approximations are prohibited in core relaxation kernels.
- **Convergence Metric**: A system is deemed settled if and only if the Euclidean residual norm satisfies:
  $$\|F(\mathbf{x})\|_2 = \sqrt{\sum_{i=1}^n f_i(\mathbf{x})^2} \le \epsilon, \quad \epsilon = 10^{-6}$$
- **Iteration Limits**: Standard Newton-Raphson relaxation loops must impose a strict deterministic iteration ceiling (default: $N_{\max} = 100$). Systems failing to converge within $N_{\max}$ iterations must exit with `EvalError::MaxIterationsExceeded`.
- **Multi-Start Determinism**: Multi-start relaxation must evaluate initial guesses in deterministic, sorted variable order. Pseudo-random restarts must be reproducibly seeded.

---

## 3. Dimensional Homogeneity & Physical Units

### 3.1 7-Dimensional SI Base Tensor
Aven enforces compile-time dimensional checking across the 7 fundamental SI base quantities:
$$[M, L, T, I, \Theta, N, J]$$
Where:
- $M$: Mass (kilogram, `kg`)
- $L$: Length (meter, `m`)
- $T$: Time (second, `s`)
- $I$: Electric Current (ampere, `A`)
- $\Theta$: Thermodynamic Temperature (kelvin, `K`)
- $N$: Amount of Substance (mole, `mol`)
- $J$: Luminous Intensity (candela, `cd`)

### 3.2 Homogeneity Rules
1. **Equation Invariance**: In any equation $LHS = RHS$, the dimensional vectors of $LHS$ and $RHS$ must be identical ($[d_{LHS}] == [d_{RHS}]$).
2. **Addition & Subtraction**: In $A \pm B$, $[d_A] == [d_B]$. The resulting dimension is $[d_A]$.
3. **Multiplication & Division**: 
   - $[d_{A \cdot B}] = [d_A] + [d_B]$
   - $[d_{A / B}] = [d_A] - [d_B]$
4. **Dimensionless Exponentiation & Functions**:
   - For $A^k$, if $k$ is non-integer, $A$ must be dimensionless or $k$ must be a compile-time rational constant.
   - Transcendental function arguments ($\sin, \cos, \tan, \exp, \ln, \log_{10}$) MUST be dimensionless ($[0, 0, 0, 0, 0, 0, 0]$).
5. **Static Enforcement**: Dimensional mismatches must abort compilation prior to code generation or solver execution, indicating the exact source span (`line:column`).

---

## 4. Cryptographic Provenance & Fact Ledgers

### 4.1 Hash Standard
- All cryptographic digests, model signatures, fact seals, and package integrity fingerprints must use standard **SHA-256** (NIST FIPS 180-4).

### 4.2 Append-Only Fact Ledger
- Facts written to disk (`.aven_ledger.jsonl`) are immutable and append-only.
- Each ledger entry must record:
  - Logical cycle sequence number ($k \in \mathbb{N}$).
  - High-resolution wall-clock timestamp (UTC RFC-3339 / ISO-8601).
  - Variable name, settled numerical value, and inferred physical unit.
  - Causal provenance dependency tree (ancestor fact IDs).
  - Merkle hash-chain link:
    $$H_k = \text{SHA256}(H_{k-1} \parallel \text{Payload}_k)$$

### 4.3 Segmented Ledger Compaction & Tamper Detection
- **Segment Size Ceiling**: Continuous telemetry logging must rotate into segmented files (e.g. `ledger_segment_0001.jsonl`) upon reaching segment byte limits.
- **Chain Continuity**: The first entry of segment $s+1$ must reference the terminal hash of segment $s$.
- **Tamper Detection**: Running `avenc debug` or `verify_segmented_directory` must verify cryptographic continuity across every segment. Any missing, reordered, or edited entry must fail verification immediately.

---

## 5. Temporal Streams & Signal Calculus

### 5.1 Stream Retention & Buffers
- Signal streams declared with `stream<T, N>` must be backed by bounded circular FIFO buffers of fixed capacity $N$.
- Memory usage for temporal streams must be strictly $O(N)$ and bounded at compile-time.

### 5.2 Staleness & Decay Semantics
- Ingested facts carry an associated Time-To-Live ($\text{TTL}$).
- Values beyond their TTL transition through defined decay modes:
  - `Linear`: Value interpolates toward baseline default over $t_{\text{grace}}$.
  - `Exponential`: Value decays via $v(t) = v_0 \cdot e^{-\lambda \Delta t}$.
  - `Step`: Value immediately invalidates upon $t > \text{TTL}$.
- Solves relying on expired inputs without valid fallback MUST fail with Exit Code `3` (Stale Input Violation).

### 5.3 Commit Boundaries & Irreversible Rewind Guards
- Actions marked with `committed-effect` (e.g. physical valve opening, rocket ignition, financial trade execution) form an irreversible causal boundary.
- Time-travel rewind operations (`avenc debug goto <cycle>`) attempting to step across a committed boundary without explicit `--force` override must be rejected to prevent unsafe physical counterfactual states.

---

## 6. Target Code Emission Standards

### 6.1 ANSI C Codegen (`avenc emit-c`)
- Emitted C files (`model.c`, `model.h`) must be freestanding and pass `-Wall -Wextra -Werror -pedantic`.
- Must emit direction-pinned calculation routines with exact execution order topologically sorted.
- Inlined mathematical macros or pure functions must be stateless.

### 6.2 WebAssembly Core 1.0 (`avenc emit-wasm`)
- Emitted `.wasm` binaries must conform to the official WebAssembly Core 1.0 W3C recommendation.
- Code generation must use standard LEB128 variable-length integer encoding.
- Dual-ABI compliance:
  1. **Direct Calculation**: Exported function taking numeric inputs as parameters and returning settled outputs directly on the WebAssembly operand stack.
  2. **Memory-Mapped Solver**: Standard `(export "aven_solve") (param $in_ptr i32) (param $out_ptr i32) (result i32)` reading IEEE-754 `f64` values from linear memory and returning status code (0 for success).

---

## 7. Package Distribution & Registry Standards

### 7.1 Manifest (`Aven.toml`)
- Every distributable package must contain a valid `Aven.toml` specifying:
  - `[package]`: `name` (alphanumeric snake_case), `version` (SemVer 2.0.0), `entry` (`.aven` entrypoint), `license`, and `description`.
  - `[dependencies]`: Semantic version or relative path bindings.

### 7.2 Archive Format (`.avenpkg`)
- Binary package archives must use the standard single-file distribution layout:
  - Header: `AVENPKG1\n` magic identifier.
  - Manifest chunk: Manifest length + UTF-8 `Aven.toml` content.
  - Source payload: File count + (relative path length, path, byte size, file bytes).
  - Cryptographic seal: Trailing 64-character hexadecimal SHA-256 hash verifying all preceding archive bytes.
- Packages with mismatched or corrupted SHA-256 seals must be rejected at installation time.

---

## 8. CLI Exit Codes & Diagnostic Standards

### 8.1 Deterministic Exit Codes
Every invocation of the `avenc` binary must return one of the following canonical process exit codes:

| Exit Code | Identifier | Semantic Meaning |
|:---:|:---|:---|
| **0** | `SUCCESS` | Model fully settled, all equations and constraints satisfied. |
| **1** | `SOLVE_ERROR` | Unsolvable system, singular Jacobian, division by zero, or compiler syntax error. |
| **2** | `CONSTRAINT_VIOLATION` | Mathematical equations converged, but user-defined inequalities or invariants failed. |
| **3** | `STALE_INPUT` | Input fact exceeded its Time-To-Live (TTL) without acceptable fallback. |

### 8.2 Diagnostic Spans
- Error messages must report the canonical file URI, 1-based line number, and 1-based column position:
  ```text
  error[E0102]: dimensional mismatch in equation
    --> flight_envelope.aven:14:9
     |
  14 |   thrust = mass * acceleration + drag_coefficient;
     |            ^^^^^^^^^^^^^^^^^^^   ^^^^^^^^^^^^^^^^
     |            dimension: [M L T^-2]  dimension: [1]
  ```
- Underdetermined systems must provide rank-deficiency diagnostics identifying missing constraints and minimal candidate variables to bind.

---

## 9. Testing & Quality Assurance Requirements

### 9.1 Test Types Required for Every PR
1. **Deterministic Golden Tests**: Verifies known physical ground truths (e.g., standard atmospheric models, Markowitz efficient frontiers, flight equations) against static expected values.
2. **Randomized Stress / Property Tests**: Minimum of 3 randomized property-based tests using OS entropy (`SystemTime::now()` + PID) seeds to ensure numerical stability and prevent regression under unencountered inputs.
3. **Subprocess Integration Tests**: Validates that CLI commands (`run`, `emit-c`, `emit-wasm`, `pkg`, `debug`) output correct exit codes and format contracts.

### 9.2 Build Hygiene
- The repository must pass `cargo test --release` across all test suites with **zero test failures** and **zero compiler warnings**.
