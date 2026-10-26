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
| `aeric.c` | Orchestrator: ingress validation, HOLD latch + recovery, chained FNV-1a heartbeat, streaming SHA-256 provenance seal |
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
clean solves. Every tick prints a distinct chained hash, and a SHA-256 seal is
printed at the end.

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
  (chained FNV-1a heartbeat), and a streaming **SHA-256** seal covers every
  record for cryptographic provenance. `AERIC_Seal` snapshots the digest
  without disturbing the ongoing chain. (The SHA-256 is verified against the
  FIPS-180-4 `abc` and empty-string vectors.)

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
   real chain plus a SHA-256 seal.
