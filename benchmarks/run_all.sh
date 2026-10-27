#!/bin/sh
# run_all.sh — Linux/macOS equivalent of run_all.ps1: build and run all four backends on
# the same battery, then compare them bit-for-bit and against the 50-digit oracle.
# Usage (from the repo root): sh benchmarks/run_all.sh <scratch-dir>
# Needs a C/C++ compiler (cc/c++), cargo, and Python 3 with numpy.
set -eu
SCRATCH="${1:-${TMPDIR:-/tmp}/aven_bench}"
PY="${PYTHON:-python3}"
mkdir -p "$SCRATCH"

"$PY" benchmarks/gen_and_oracle.py "$SCRATCH"

# Aven-C with link-time optimization (lets the generated function inline, like /GL /LTCG)
cc -O2 -flto -std=c99 benchmarks/backend_c.c aeric/model_server_capacity.c -lm -o "$SCRATCH/bench_c"
c++ -O2 benchmarks/backend_cpp.cpp -o "$SCRATCH/bench_cpp"
"$SCRATCH/bench_c" "$SCRATCH"
"$SCRATCH/bench_cpp" "$SCRATCH"

cargo run --release --bin bench_aven -- "$SCRATCH"
"$PY" benchmarks/backend_python.py "$SCRATCH"

"$PY" benchmarks/compare.py "$SCRATCH"
