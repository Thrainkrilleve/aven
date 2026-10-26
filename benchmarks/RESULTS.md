# AERIC / AVEN — Four-Backend Differential & Performance Proof

**Model under test:** `examples/server_capacity.aven`, forward block `ProductionProfile`
(5 outputs: `raw_workers`, `allocated_workers` = `ceil(raw/util)`, `total_ram_needed`,
`payload_gbps`, `wire_rate_gbps`).
**Machine:** AMD Ryzen 7 9800X3D, Windows 11 Pro, MSVC (`cl /O2`), single thread.
**Battery:** 260,121 input vectors — 121 hand-built pathologies (zeros, ±0, 1-ULP
subnormals, max-double, `target_utilization=0`, overflow products, ±Inf, NaN),
10,000 **ceil-boundary** cases (ratios placed {0, ±1e-12, ±1e-10, ±1e-9, ±2e-9,
±1e-8} from an integer to stress the 1e-9 snap), and 250,000 wide log-uniform
random vectors. Every backend reads the **same** `battery.txt`.
**Oracle:** Python `decimal` at 50 digits, implementing AVEN's exact semantics
including the `ceil` 1e-9 snap (identical in `diff::eval_expr` and emit-c `aven_ceil`).

The four backends:

| Backend | What it is |
|---|---|
| **Aven-C** | the real `avenc emit-c` output (`aven_ProductionProfile`), compiled |
| **C++** | independent hand-written port (same parenthesization + snap) |
| **Python** | independent numpy port |
| **Rust** | **AVEN's own evaluator** (`diff::eval_expr`) in-process — the interpreter path |

Each backend emits every output as raw IEEE-754 bits, so cross-backend agreement
is an exact integer comparison.

---

## 1. Correctness

**All four backends are IEEE-754 bit-identical on every one of the 260,085
finite-output rows** — including all 10,000 ceil-boundary cases.

```
Aven-C vs C++    : BIT-IDENTICAL on all 260,085 rows
Aven-C vs Python : BIT-IDENTICAL on all 260,085 rows
Aven-C vs Rust   : BIT-IDENTICAL on all 260,085 rows
```

Against the 50-digit decimal oracle (finite rows):

| Check | Result | Meaning |
|---|---|---|
| `allocated_workers`, true value ≤ 2⁵³ | **259,468 / 260,077 exact** | — |
| `allocated_workers`, true value > 2⁵³ | 3 / 6 correctly-rounded | float64 can't hold integers > 2⁵³ exactly |
| continuous outputs, **normal** results | **≤ 5.1 × 10⁻¹⁶ rel. err (~2 ULP)** | correctly-rounded arithmetic |
| continuous outputs, subnormal results | up to ~2 × 10⁻² rel. err | IEEE subnormals lose precision *by design* |

**The 609 `allocated_workers` rows that differ from the oracle are all
ceil-boundary cases** where float64's computed ratio and the exact ratio fall on
opposite sides of the 1e-9 snap. These are **not** implementation disagreements —
all four float64 backends produce the *same* value; they differ only from
infinite-precision math, which any float64 engine does. `total_ram_needed` depends
on `allocated_workers`, so it inherits those off-by-ones (hence its ~2e-2 max
error at boundaries); the arithmetic that does **not** go through `ceil`
(`raw/payload/wire`) matches the oracle to ~2 ULP on all normal results.

The single largest "pure arithmetic" deviation (2e-2) is row 6, where
`avg_latency_ms = 4.94e-324` (the smallest subnormal) drives the result into the
subnormal range (~2.5e-322). All four backends still return the identical bits
there — the gap is only versus ideal math, and is inherent to subnormals.

**36 non-finite pathology rows** (overflow / divide-by-zero → ±Inf/NaN) differ
between backends only in Inf-sign or NaN payload bits — no backend is "more
correct"; these inputs have no finite answer. (In AERIC these inputs are caught
by the shim's finiteness check and routed to HOLD.)

---

## 2. Performance

Solve-only throughput, amortized over the full battery, repeated K times with a
volatile sink (K=200 compiled, 50 numpy, 20 interpreter). **This measures the
compute kernel, not an end-to-end control-loop tick** (no I/O, no transport), and
it is amortized throughput, not worst-case single-tick latency.

| Backend | ns / solve | solves / sec | vs Aven-C (LTO) |
|---|---:|---:|---:|
| **Aven-C** (emit-c, `/O2 /GL /LTCG`, inlined) | **5.49** | **182,000,000** | 1.0× |
| C++ (hand-written, inlined) | 5.67 | 176,000,000 | 0.97× |
| Aven-C (emit-c, separate TU, no LTO) | 8.69 | 115,000,000 | 0.63× |
| Python — numpy (vectorized) | 27.6 | 36,000,000 | 0.03× |
| Rust — **AVEN evaluator (interpreter)** | 729 | 1,370,000 | 0.0075× |
| Python — pure (scalar loop) | 1,271 | 787,000 | 0.0043× |

**Headlines:**
- **AVEN's generated C matches hand-written C++ to within ~3%** (5.49 vs 5.67
  ns) — i.e. the compiler adds *no* computational overhead. The separate-TU
  number (8.69 ns) is pure cross-translation-unit call cost; whole-program
  optimization (`/GL /LTCG`) lets the generated function inline and it closes the
  gap exactly, confirming the overhead was linkage, not code.
- **Compiled AVEN is ~133× the AVEN interpreter and ~230× pure Python.** This is
  the concrete value of `emit-c`: the same model that the Rust evaluator runs at
  ~730 ns/solve runs at ~5.5 ns/solve once compiled.

---

## 3. What this does and does not prove

**Proves:**
- AVEN's compiled C, an independent C++ port, an independent Python port, and
  AVEN's own Rust evaluator compute this model **identically to the bit** across
  a 260k adversarial battery — so AVEN's result is not an artifact of one
  implementation.
- AVEN's arithmetic is correctly-rounded against a 50-digit reference (~2 ULP)
  wherever float64 can represent the answer.
- The `ceil` 1e-9 snap is consistent across the interpreter and the generated C.
- Generated C is as fast as hand C++ and ~100–230× faster than interpreted paths.

**Does not prove (future work, per the roadmap):**
- Worst-case single-tick latency or jitter (these are amortized throughput on one
  machine/compiler). A dedicated timing harness with p99/max is tracked separately.
- Cross-platform / cross-compiler bit-reproducibility (only MSVC here; gcc/clang
  determinism is a roadmap track).
- Anything about I/O, transport, or the full control loop.

---

## 4. Reproduce

```bash
# 1. generate battery + oracle (writes into <scratch>)
python benchmarks/gen_and_oracle.py <scratch>

# 2. build + run the compiled backends (from an MSVC x64 env; see run_all.ps1)
cl /O2 /GL benchmarks/backend_c.c aeric/model_server_capacity.c /Fe:bench_c.exe /link /LTCG
cl /O2 /EHsc benchmarks/backend_cpp.cpp /Fe:bench_cpp.exe
bench_c.exe <scratch> ; bench_cpp.exe <scratch>

# 3. Rust (AVEN evaluator) and Python
cargo run --release --bin bench_aven -- <scratch>
python benchmarks/backend_python.py <scratch>

# 4. differential comparison
python benchmarks/compare.py <scratch>
```

`run_all.ps1 <scratch>` does all of the above on Windows/MSVC.
