# AERIC — standalone C99 orchestrator for AVEN

AERIC is a thin, deterministic control layer that wraps AVEN's AOT-compiled C
output (`avenc emit-c`). Each tick it validates sensor inputs, runs the compiled
AVEN solver through a stable shim, publishes setpoints, and folds the result
into a hash-chained ledger. On any fault it latches **HOLD** and drives the
configured fail-safe setpoints until recovery.

This directory is a corrected, compilable implementation. It replaces the
earlier `engine-old.aer` sketch, which did not compile and had memory-safety and
ABI defects (see "What was wrong before").

## Files

| File | Role |
|------|------|
| `aeric.h` | Runtime type, the stable `aven_solve` ABI, public API |
| `aeric.c` | Orchestrator: ingress validation, HOLD latch + recovery, chained FNV-1a heartbeat, keyed HMAC-SHA256 provenance seal |
| `model_server_capacity.h/.c` | The AVEN-generated solver (`aven_ProductionProfile`) for `examples/server_capacity.aven`, as a linkable unit |
| `aven_shim.c` | The one model-specific adapter: maps the stable ABI to the generated function and rejects non-finite results |
| `main.c` | Compilable demo driver (nominal, domain fault, bad arity, recovery) |
| `Makefile` / `build.bat` | gcc/clang / MSVC builds |

## Build & run

POSIX (gcc/clang):

```bash
make run
```

Windows (MSVC):

```bat
build.bat
```

Expected output: two nominal ticks solve to `alloc_workers=1429`,
`ram_mb=32676` (matching `avenc run`), a `target_utilization=0` tick and a
wrong-arity tick drop to fail-safe HOLD, and the loop auto-recovers after 3
clean solves. Every tick prints a distinct chained hash, and an HMAC-SHA256 seal
is printed at the end.

## The AVEN ABI (why the shim exists)

`avenc emit-c` does **not** emit a generic `aven_solve(void*, ...)`. For a
`forward` block it emits, for example:

```c
void aven_ProductionProfile(
    double avg_latency_ms, double avg_payload_kb, /* ... inputs, BY VALUE ... */
    double* out_raw_workers, double* out_allocated_workers /* ... */);
```

- Inputs are scalar `double`s **by value**, in avenc's canonical **alphabetical**
  order.
- Outputs are `double*` out-params, in the block's equation order.
- The function returns `void`. In the default (non-checked) emit it never
  aborts; a bad domain yields `NaN`/`Inf`.

Because this shape is per-model, AERIC never calls it directly. It calls one
stable symbol:

```c
int aven_solve(const double *in, size_t n_in, double *out, size_t n_out);
```

`aven_shim.c` implements that symbol for this model — it is the only file that
knows the generated function's exact signature and the input/output ordering.
To drive a different model, regenerate `model_<name>.c` and write a matching
shim; `aeric.c`/`aeric.h` do not change.

### Regenerating the model unit

```bash
make regen-model MODEL=server_capacity
```

This runs `avenc emit-c <model>.aven --traceable` (so the function has external
linkage) and strips the emitter's demo `main()`.

**Do not build the linked model with `--checked-c`.** That mode calls
`exit(EXIT_FAILURE)` on a domain violation, which would terminate the process
instead of letting AERIC HOLD. Domain checking belongs in the shim as a
return-valued rejection (see `aven_shim.c`), not as a process abort.

## Safety model

- **No dynamic allocation.** All state is in `aeric_runtime_t`; the per-tick
  ledger record is built in a bounded, O(1) arena. Input/output vectors have
  compile-time caps (`AERIC_MAX_INPUTS`, `AERIC_MAX_OUTPUTS`).
- **Strict ingress.** A tick is rejected (→ HOLD) unless the arity matches the
  schema and every sensor value is finite.
- **HOLD is enforced, not just flagged.** While latched, `AERIC_Tick` publishes
  the configured fail-safe setpoints every cycle. It leaves HOLD only after
  `recover_ticks` consecutive clean solves (set `recover_ticks = 0` for
  manual-only recovery via `AERIC_ClearFault`).
- **Faults never crash the loop.** The solver reports errors by return value;
  the shim converts `NaN`/`Inf` into a nonzero return.
- **Tamper-evident ledger.** Each tick's record folds in the previous hash
  (chained FNV-1a heartbeat), and a keyed streaming **HMAC-SHA256** seal (keyed
  via `AERIC_Init`, construction matching the Rust ledger's `hmac_sha256_hex`)
  covers every record for forgery-resistant provenance. `AERIC_Seal` snapshots
  the digest without disturbing the ongoing chain. (The SHA-256 core is verified
  against the FIPS-180-4 `abc`/empty vectors; the HMAC seal is verified end-to-end
  against Python's reference `hmac` — identical digest, key-dependent.)

## What was wrong before (`engine-old.aer`)

1. **Memory corruption** — the arena was overlaid on an 8-byte pointer field
   (`(arena_t*)&rt->aven_output_ptr`) and `memset` 2 KB over the struct. Here the
   arena is a real embedded member.
2. **Ingress always failed** — it compared `ARENA_CAPACITY` (2048) against
   `input_size` (64). Here ingress checks the actual arity and finiteness.
3. **Input never staged** — raw bytes were validated but never copied to the
   solver's buffer. Here inputs are copied into `rt->inputs` before the solve.
4. **Fabricated ABI** — it declared `aven_solve(const void*, float*, uint16_t*,
   bool*)`, which avenc never emits, and `float` setpoints for a `double` solver.
   Here the shim marshals the real `double`-based generated function.
5. **HOLD did nothing** — the flag was set but never read. Here it is enforced
   at tick entry with a defined recovery policy.
6. **Ledger wasn't a ledger** — independent, non-chained FNV-1a. Here it is a
   real chain plus a keyed HMAC-SHA256 seal.

## Level-1 daemon (`aeric_daemon.c`)

Turns the library into a standalone backend: a config-driven, fixed-rate control
loop that pulls sensor frames over TCP, pushes setpoints back, drops to HOLD on a
missed deadline or lost peer, and appends a ledger to disk **in the Rust ledger's
own JSONL format** — so the host's `avenc verify-ledger` validates an edge run
unmodified. The AERIC core stays transport-free; the daemon is the edge wiring.

| File | Role |
|------|------|
| `aeric_daemon.c` | config parse + timed loop + TCP transport + host-format ledger |
| `daemon.aer` | runtime config (`key = value`, `#` comments) |
| `daemon_peer.py` | test peer: happy-path / deadline-miss / disconnect |

**Transport** (length-prefixed TCP, little-endian): peer → daemon sends
`u32 len` + `n_inputs` float64 (sensors); daemon → peer replies `u32 len` +
`n_outputs` float64 (setpoints), one exchange per live tick. A tick with no
sensor frame within `deadline_ms`, or a lost peer, is a fault → HOLD (no reply).

**Ledger.** Each setpoint is emitted as a fact in the exact format of the Rust
`Ledger::to_jsonl` (SHA-256 chain over the canonical `\x1f`-joined form from a
64-zero genesis, per-fact HMAC signature, closing `{"seal": true, ...}` line).
The daemon reaches the SHA-256/HMAC primitives through `AERIC_SHA256` /
`AERIC_HMAC_SHA256` exposed by the core. Verify with the host tool:
```
avenc verify-ledger aeric_ledger.jsonl --key <key>
```

**Run the demo** (MSVC shown; gcc/clang use `-lws2_32 -lwinmm`):
```
cl /O2 aeric_daemon.c aeric.c aven_shim.c model_server_capacity.c /Fe:aericd.exe
aericd.exe daemon.aer                                   # terminal 1
python daemon_peer.py 5555 8 5                           # terminal 2
avenc verify-ledger aeric_ledger.jsonl --key aven-aeric-demo-key-v1
```

**Verified** (Ryzen 7 9800X3D, 50 Hz, `max_ticks=120`): the peer streams, pauses
250 ms (→ HOLD), resumes, disconnects (→ HOLD). The daemon records 120 ticks
(56 live / 64 hold), max jitter ~17 ms across connect/pause transitions (~1.7 ms
headless). `avenc verify-ledger` reports **"Ledger OK: 600 facts, chain intact,
every signature valid"**; the wrong key and a tampered `value_bits` are both
**rejected**. Builds clean at `-Wall -Wextra -Wpedantic` on MSVC, gcc 14.2, and
clang 19.1; a gcc-built daemon's ledger verifies identically.

**Honest limits.** Jitter is bounded by `deadline_ms`, not sub-ms — true
worst-case/RT timing (SCHED_FIFO, busy-spin) is the Level-2/3 track. Single
client, one transport (TCP) — more drivers (Modbus/MQTT/OPC-UA/CAN) are Level-2.
