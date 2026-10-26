/* aeric.h -- Standalone AERIC orchestrator (C99, freestanding-friendly)
 *
 * A thin, deterministic control layer that wraps AVEN's AOT-compiled C output
 * (`avenc emit-c`). Each tick: validate sensor inputs, run the compiled AVEN
 * solver through a stable shim, publish setpoints, and fold the result into a
 * hash-chained ledger. On any fault it latches HOLD and emits the configured
 * fail-safe setpoints until recovery.
 *
 * Design contract (what makes this memory-safe and real-time friendly):
 *   - No dynamic allocation anywhere; all state lives in aeric_runtime_t.
 *   - Fixed, bounded output/input vectors (compile-time caps).
 *   - The AVEN solver is reached only through aven_solve() (see below), so the
 *     orchestrator never guesses the generated ABI.
 *   - Faults never crash the loop: the solver shim reports errors by return
 *     value, never by exit(). (Do NOT link an avenc `--checked-c` build here:
 *     that mode calls exit(EXIT_FAILURE) on a domain violation and would take
 *     the whole process down instead of letting AERIC HOLD. Use the default
 *     emit and let the shim validate outputs.)
 */
#ifndef AERIC_H
#define AERIC_H

#include <stdint.h>
#include <stddef.h>
#include <stdbool.h>

/* Compile-time bounds. Raise if a model needs more; they size the runtime. */
#define AERIC_MAX_INPUTS     32
#define AERIC_MAX_OUTPUTS    16
#define AERIC_ARENA_CAPACITY 2048

/* ===== Stable solver ABI (provided by a per-model shim, e.g. aven_shim.c) =====
 *
 * AERIC calls exactly this symbol; the shim marshals it to the generated
 * aven_<block>(double, ..., double*, ...) function (whose inputs are scalar
 * doubles in avenc's canonical/alphabetical order and whose outputs are
 * double* out-params in equation order).
 *
 * Returns 0 on success. Any nonzero value is a solver fault and makes AERIC
 * latch HOLD. The shim MUST reject non-finite results (NaN/Inf) with a nonzero
 * return rather than publishing them.
 *
 *   in   : n_in finite doubles, in the model's canonical input order.
 *   out  : receives n_out doubles on success; untouched on fault.
 */
int aven_solve(const double *in, size_t n_in, double *out, size_t n_out);

/* ===== Tick schema ===== */
typedef struct {
    uint32_t version;        /* schema version for .aer evolution           */
    size_t   n_inputs;       /* count of double sensor inputs per tick       */
    size_t   n_outputs;      /* count of double setpoints per tick           */
    uint32_t recover_ticks;  /* clean solves needed to auto-exit HOLD;
                              * 0 => HOLD is manual-clear only (ClearFault)  */
} tick_schema_t;

/* ===== Per-tick ledger scratch arena (O(1), no heap) ===== */
typedef struct {
    uint8_t buffer[AERIC_ARENA_CAPACITY];
    size_t  write_pos;
} aeric_arena_t;

/* ===== Streaming SHA-256 context (provenance seal) ===== */
typedef struct {
    uint32_t state[8];
    uint64_t bitlen;
    uint8_t  block[64];
    size_t   fill;
} aeric_sha256_t;

/* ===== AERIC runtime state (all storage; no pointers into caller memory) ===== */
typedef struct {
    tick_schema_t  schema;
    aeric_arena_t  arena;                        /* per-tick ledger record scratch */

    double inputs[AERIC_MAX_INPUTS];             /* validated staging copy          */
    double outputs[AERIC_MAX_OUTPUTS];           /* published setpoints (safe in HOLD) */
    double safe[AERIC_MAX_OUTPUTS];              /* configured fail-safe setpoints   */

    bool     hold_mode;                          /* latched on fault                */
    uint32_t fault_count;                        /* lifetime fault count            */
    uint32_t consecutive_ok;                     /* clean solves since last fault   */
    uint64_t tick_index;                         /* monotonic tick counter          */

    uint64_t       last_tick_hash;               /* chained FNV-1a heartbeat        */
    aeric_sha256_t seal;                         /* running SHA-256 over all records */
} aeric_runtime_t;

/* ===== Tick result codes ===== */
#define AERIC_OK        0   /* solved; outputs are live setpoints            */
#define AERIC_HOLD      1   /* fault/latched; outputs are fail-safe setpoints */
#define AERIC_BADCALL  (-2) /* null args; nothing done                       */

/* ===== Public API ===== */

/* Initialise the runtime. `safe_setpoints` is n_outputs doubles describing the
 * state to drive while in HOLD (required; must be finite). Returns false if the
 * schema exceeds the compile-time caps or any safe setpoint is non-finite. */
bool AERIC_Init(aeric_runtime_t *rt,
                const tick_schema_t *schema,
                const double *safe_setpoints);

/* Run one control cycle. `inputs` is n_inputs finite doubles. Publishes either
 * live or fail-safe setpoints (read with AERIC_Setpoints) and advances the
 * ledger. Returns AERIC_OK / AERIC_HOLD / AERIC_BADCALL. */
int AERIC_Tick(aeric_runtime_t *rt, const double *inputs, size_t n_inputs);

/* Force-clear a latched HOLD (operator acknowledgement). */
void AERIC_ClearFault(aeric_runtime_t *rt);

/* Current published setpoints (n_outputs doubles). Fail-safe values in HOLD. */
const double *AERIC_Setpoints(const aeric_runtime_t *rt);

/* Finalise a snapshot of the provenance digest into out_digest[32]. The running
 * seal is cloned, so AERIC_Seal may be called repeatedly (e.g. at state
 * transitions or shutdown) without disturbing the ongoing chain. */
void AERIC_Seal(aeric_runtime_t *rt, uint8_t out_digest[32]);

#endif /* AERIC_H */
