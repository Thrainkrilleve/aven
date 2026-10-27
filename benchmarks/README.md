# benchmarks/ — four-backend differential & performance harness

Proves AVEN's compiled output is both **correct** (agrees to the bit with
independent implementations and a high-precision oracle) and **fast** (as fast as
hand-written C++ with MSVC, within about 10% with gcc; see RESULTS.md §2a), on a brutal adversarial battery — not a happy-path demo.

See **[RESULTS.md](RESULTS.md)** for the measured proof.

## Backends (all run the same `battery.txt`)
- **Aven-C** — `backend_c.c` drives the real `avenc emit-c` function
  (`../aeric/model_server_capacity.c`). The artifact under test.
- **C++** — `backend_cpp.cpp`, independent hand port.
- **Python** — `backend_python.py`, independent numpy port (+ pure-Python baseline).
- **Rust** — `../src/bin/bench_aven.rs` drives AVEN's own evaluator
  (`diff::eval_expr`) in-process: the interpreter path.

## Files
| File | Role |
|---|---|
| `gen_and_oracle.py` | writes `battery.txt` (shared inputs) + `oracle.txt` (50-digit decimal reference) |
| `backend_*.{c,cpp,py}`, `../src/bin/bench_aven.rs` | the four backends; each writes `out_<name>.txt` (IEEE bits) + prints timing JSON |
| `compare.py` | cross-backend bit identity + error vs oracle, with finite/non-finite and ≤2⁵³ classification |
| `run_all.ps1` | one-shot build+run+compare (Windows/MSVC) |
| `run_all.sh` | one-shot build+run+compare (Linux/macOS, gcc/clang) |

## Run
From the repo root, in an MSVC x64 environment (the script will locate vcvars):
```
powershell -File benchmarks\run_all.ps1 C:\path\to\scratch
```
or on Linux/macOS (needs numpy):
```
sh benchmarks/run_all.sh /path/to/scratch
```
Large data files (`battery.txt`, `oracle.txt`, `out_*.txt`) are written to the
scratch dir, not the repo.

## Notes
- Outputs are compared as raw IEEE-754 bits → cross-backend agreement is exact.
- The compiled timing is amortized compute-kernel throughput, **not** end-to-end
  tick latency and **not** worst-case/jitter. See RESULTS.md §3 for scope.
- To benchmark a different model: regenerate the emit-c unit and shim (see
  `../aeric/README.md`), port the equations into `backend_cpp.cpp` /
  `backend_python.py` / the oracle, and update the input/output name lists.
