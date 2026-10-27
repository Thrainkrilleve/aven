# Aven Engineering & Mathematical Standards (STANDARDS.md)

This document specifies the authoritative engineering, numerical, cryptographic, and architectural standards for the **Aven** language, compiler (`avenc`), runtime targets, and package ecosystem.

All implementations, compiler modifications, codegen backends, and community contributions MUST comply with these standards. Each rule below describes what the code in this repository does today; where something exists only as a library API and not yet in the language or CLI, the rule says so.

---

## 1. Architectural Separation ("Rust Compiles, C Runs")

### 1.1 The Compiler Layer (`avenc`)
- **Language**: Pure standard library Rust (`std`).
- **Zero Third-Party Dependencies**: The compiler and developer toolchain must compile with `cargo build --release` using exclusively `std`. No external crates are permitted in `Cargo.toml`.
- **Scope**: Lexing, AST construction, topological sorting, symbolic differentiation, interval arithmetic, SMT-LIB2 transpilation, C code emission, WebAssembly Core 1.0 binary generation, temporal debugging, and package management are strictly the responsibility of `avenc`.

### 1.2 The Execution Runtime Layer (`aeric/`)
- **Language**: C99. `aeric/Makefile` builds with `-std=c99 -Wall -Wextra -Wpedantic`, and the code also builds cleanly with `-Werror`.
- **Zero-Allocation Invariants**: No `aeric/` source calls `malloc`, `calloc`, `realloc` or `free`.
- **Header Standards**: The runtime kernel (`aeric.c`, `aeric.h`, the `aven_shim*.c` adapters) includes only `<stdint.h>`, `<stddef.h>`, `<stdbool.h>`, `<string.h>` and `<math.h>`. Demo drivers, tests and the network daemon (`aeric_daemon.c`) use the C library and OS socket headers.
- **Architectural Boundary**: Never implement language parsing, AST representation, or compiler optimizations in C.

---

## 2. Mathematical Rigor & Numerical Stability

### 2.1 Floating-Point Semantics
- **Precision**: Calculations use standard IEEE-754 double-precision 64-bit binary floating-point (`f64`).
- **Domain Fault Guards**: Evaluation (`diff::eval_expr`) rejects undefined operations before they produce a value:
  - Division by a denominator that is exactly zero.
  - `ln(x)` / `log(x)` with $x \le 0$; `sqrt(x)` with $x < 0$; `tan(x)` where $|\cos x| < 10^{-12}$.
- **NaN / $\pm\infty$ Rejection**: Any operation or function whose result is not finite (e.g. `exp(1000)`) is an evaluation error. `avenc run` reports it as a solver error and exits with code 1; a non-finite value is never printed as a result. `tests/claims.rs` checks this.

### 2.2 Relaxation & Root-Finding Convergence
- **Derivatives**: Newton steps use the exact symbolic derivative of each equation (`src/diff.rs`). Where a symbolic derivative cannot be evaluated at a point (e.g. a non-finite value), the solver falls back to a central difference with step $h = 10^{-6}\max(1, |x|)$.
- **Convergence Metric**: A single equation is settled when $|f(x)| < 10^{-9}$; a coupled system when the largest residual satisfies $\max_i |f_i(\mathbf{x})| < 10^{-9}$.
- **Iteration Limits**: Each start runs at most 60 Newton iterations for one unknown, or 80 for a coupled system, before the next start is tried.
- **Multi-Start Determinism**: Starting points are fixed, not random: 8 seeds for one unknown (1, 0.5, 2, 10, 0.1, −1, 100, −10) and 6 for coupled systems, applied in sorted variable order. The same model always takes the same path.

---

## 3. Dimensional Homogeneity & Physical Units

### 3.1 Base Dimensions
Aven enforces compile-time dimensional checking across ten base dimensions (`Dim` in `src/units.rs`):
- Six SI base quantities: mass (`kg`), length (`m`), time (`s`), electric current (`A`), thermodynamic temperature (`K`) and amount of substance (`mol`). Luminous intensity is not modelled.
- Four engineering dimensions: angle (`rad`), money (`$`), data (`b`) and transactions (`tx`).
- Packages and modules may add named custom dimensions.

Units are checked for consistency only; `avenc run` does not rescale numeric values between units.

### 3.2 Homogeneity Rules
1. **Equation Invariance**: In any equation $LHS = RHS$, the dimensional vectors of $LHS$ and $RHS$ must be identical ($[d_{LHS}] == [d_{RHS}]$).
2. **Addition & Subtraction**: In $A \pm B$, $[d_A] == [d_B]$. The resulting dimension is $[d_A]$.
3. **Multiplication & Division**: 
   - $[d_{A \cdot B}] = [d_A] + [d_B]$
   - $[d_{A / B}] = [d_A] - [d_B]$
4. **Dimensionless Exponentiation & Functions**:
   - For $A^k$: if $A$ is dimensionless, so is the result. Otherwise $k$ must be a constant for which every exponent of $[d_A]$ times $k$ is a whole number ($[\text{m}^2]^{0.5} = [\text{m}]$); any other power of a dimensioned quantity is a compile error.
   - Arguments of $\sin, \cos, \tan$ must be dimensionless or an angle; arguments of $\exp$ and $\ln$ must be dimensionless.
5. **Static Enforcement**: Dimensional mismatches abort compilation before code generation or solver execution, reporting the source `line` and `col`.
6. **Unit Conversion**: Values are converted to coherent base units (SI, plus bit, radian, transaction and US dollar) before solving. A number in a `solve` block is read in the unit written after it, else in the variable's declared `signal` unit; adapter and stream values cross in the declared unit. Unit symbols are matched case-sensitively first; a case-insensitive spelling is accepted only when it is unambiguous, and spellings that could mean bits or bytes (`kb`, `Mb`, `Gb`, …) are refused.

---

## 4. Cryptographic Provenance & Fact Ledgers

### 4.1 Hash Standard
- All cryptographic digests, model signatures, fact seals, and package integrity fingerprints must use standard **SHA-256** (NIST FIPS 180-4).

### 4.2 Append-Only Fact Ledger
- `avenc run --ledger <file>` writes one JSON object per settled fact, then a closing seal line.
- Each fact records: sequence number `seq`, logical `tick`, `kind` (`input`, `derived`, `effect`, `committed-effect`), variable `name`, `value` (in base units) and its exact IEEE-754 bits `value_bits`, the `causes` (ancestor `seq` numbers), `origin`, optional freshness `metadata`, and the hash-chain link:
  $$H_k = \text{SHA256}(H_{k-1} \parallel \text{Payload}_k)$$
  stored as `prev` and `hash`. With a key (`--key` or `AVEN_LEDGER_KEY`) each fact also carries an HMAC-SHA256 signature.
- The seal line records the fact count and the chain head, so truncation is detectable. `avenc verify-ledger` checks the chain, the seal and (given the key) every signature.

### 4.3 Segmented Ledger Compaction & Tamper Detection
- `avenc compact-ledger <in> -o <out>` compacts a ledger into a shorter, re-chained one.
- **Library API (not yet in the CLI)**: `compaction::SegmentedLedgerWriter` rotates long-running logs into segment files whose first entry links to the previous segment's terminal hash, and `compaction::verify_segmented_directory` verifies continuity across segments. `avenc run` writes a single file.

---

## 5. Temporal Streams & Signal Calculus

### 5.1 Stream Retention & Buffers
- **Library API (not yet in the language)**: `temporal::RetentionWindow` keeps a bounded window of samples (by count and by age), so memory per stream is $O(N)$. There is no `stream<T, N>` declaration syntax yet.

### 5.2 Staleness & Decay Semantics
- Adapter facts may carry `timestamp_ms` and `ttl_ms`. In `avenc run`, a decision that depends on an expired fact (or one with `ttl_ms = 0`) is blocked: the run prints `DECISION BLOCKED` and exits with code `3`, unless `--allow-stale` is passed.
- **Library API**: `temporal::StalenessDecay` defines `Step`, `Linear` and `Exponential { half_life_ms }` confidence decay for consumers of the temporal API; `avenc run` itself applies the step rule above.

### 5.3 Commit Boundaries & Irreversible Rewind Guards
- Facts of kind `committed-effect` (e.g. a valve opening or a trade executing) form an irreversible boundary. In `avenc debug`, `goto` and `prev` refuse to rewind across one unless given `--force`. No `avenc` command produces `committed-effect` facts yet; they come from ledgers written through the library API.

---

## 6. Target Code Emission Standards

### 6.1 C Codegen (`avenc emit-c`)
- `avenc emit-c` writes one C99 file: a `static` function for the model's first `forward` block, plus a demo `main` that runs the solve-block values (omit it with `--no-main`). Generated code includes `<math.h>` (and `<stdio.h>` for `main`) and compiles cleanly with `-std=c99 -Wall -Wextra -Werror -pedantic`; models using `ceil`/`floor` need C99's `round`, so the output is not C89.
- `--aeric-shim --out-dir <dir>` writes an AERIC bundle: `model_<name>.c`, `model_<name>.h` and `aven_shim.c`.
- Equations are emitted in topologically sorted order; the generated functions are stateless and branch-free (`verify/c_diff.py` checks both, and that results match `avenc run`).
- Generated code works in base units, like `avenc run --json`.

### 6.2 WebAssembly Core 1.0 (`avenc emit-wasm`)
- Emitted `.wasm` binaries must conform to the official WebAssembly Core 1.0 W3C recommendation.
- Code generation must use standard LEB128 variable-length integer encoding.
- Dual-ABI compliance:
  1. **Direct Calculation**: Exported function taking numeric inputs as parameters and returning settled outputs directly on the WebAssembly operand stack.
  2. **Memory-Mapped Solver**: Standard `(export "aven_solve") (param $in_ptr i32) (param $out_ptr i32) (result i32)` reading IEEE-754 `f64` values from linear memory and returning status code (0 for success).
- WebAssembly 1.0 has no power instruction: `x ^ n` is lowered to exact multiplication for integer constants `|n| <= 64`, and to `f64.sqrt` for `n = 0.5`. Any other exponent is a compile error, never an approximation.

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
- Type errors report the 1-based line and column of the offending equation:
  ```text
  Type Error at line 5, col 5: dimensional mismatch in expression '(velocity + gust_duration)' — cannot perform '+/-' between [m·s^-1] and [s]
  ```
- Underdetermined systems report the Jacobian rank, the free variables, and the minimal bindings that would fix them.

---

## 9. Testing & Quality Assurance Requirements

### 9.1 Test Types Required for Every PR
1. **Deterministic Golden Tests**: Verifies known physical ground truths (e.g., standard atmospheric models, Markowitz efficient frontiers, flight equations) against static expected values.
2. **Randomized Stress / Property Tests**: Minimum of 3 randomized property-based tests using OS entropy (`SystemTime::now()` + PID) seeds to ensure numerical stability and prevent regression under unencountered inputs.
3. **Subprocess Integration Tests**: Validates that CLI commands (`run`, `emit-c`, `emit-wasm`, `pkg`, `debug`) output correct exit codes and format contracts.

### 9.2 Build Hygiene
- The repository must pass `cargo test --release` across all test suites with **zero test failures** and **zero compiler warnings**.
