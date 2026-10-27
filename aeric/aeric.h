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
#define AERIC_MAX_STATE      16
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

/* ===== Fault taxonomy ===== */
typedef enum {
    AERIC_FAULT_NONE = 0,
    AERIC_FAULT_SENSOR,     /* ingress: bad arity or non-finite sensor      */
    AERIC_FAULT_SOLVER,     /* solver returned error or non-finite output   */
    AERIC_FAULT_DEADLINE,   /* missed the tick deadline (signalled by host)  */
    AERIC_FAULT_TRANSPORT   /* lost/absent peer or egress NAK (host)         */
} aeric_fault_t;
#define AERIC_FAULT_KINDS 5

/* ===== Tick schema ===== */
typedef struct {
    uint32_t version;        /* schema version for .aer evolution                */
    size_t   n_inputs;       /* full solver input arity  = n_sensors + n_state   */
    size_t   n_outputs;      /* full solver output arity = n_setpoints + n_state  */
    size_t   n_state;        /* persistent state vars fed back each tick (0 = stateless) */
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

    size_t n_sensors;                            /* n_inputs  - n_state (caller supplies these)  */
    size_t n_setpoints;                          /* n_outputs - n_state (published to the plant) */
    double inputs[AERIC_MAX_INPUTS];             /* staging: [sensors | state] in canonical order */
    double outputs[AERIC_MAX_OUTPUTS];           /* published setpoints (safe in HOLD) */
    double safe[AERIC_MAX_OUTPUTS];              /* configured fail-safe setpoints   */
    double state[AERIC_MAX_STATE];               /* persistent x[t]; advanced only on a live tick */

    bool     hold_mode;                          /* latched on fault                */
    uint32_t fault_count;                        /* lifetime fault count            */
    uint32_t consecutive_ok;                     /* clean solves since last fault   */
    uint64_t tick_index;                         /* monotonic tick counter          */

    uint64_t       last_tick_hash;               /* chained FNV-1a heartbeat        */
    aeric_sha256_t seal;                         /* HMAC-SHA256 inner ctx (K^ipad + records) */
    uint8_t        seal_opad[64];                /* K'^opad, kept for HMAC finalisation */

    /* Optional safety envelope (see AERIC_SetEnvelope; inert until configured). */
    bool     envelope;
    double   out_min[AERIC_MAX_OUTPUTS];
    double   out_max[AERIC_MAX_OUTPUTS];
    double   out_rate[AERIC_MAX_OUTPUTS];        /* max |Δsetpoint| per tick        */
    bool     clamped;                            /* last live tick hit a limit      */
    uint32_t clamp_count;                        /* lifetime clamp/rate-limit events */

    /* Fault taxonomy. */
    aeric_fault_t last_fault;
    uint32_t      fault_counts[AERIC_FAULT_KINDS];
} aeric_runtime_t;

/* ===== Tick result codes ===== */
#define AERIC_OK        0   /* solved; outputs are live setpoints            */
#define AERIC_HOLD      1   /* fault/latched; outputs are fail-safe setpoints */
#define AERIC_BADCALL  (-2) /* null args; nothing done                       */

/* ===== Public API ===== */

/* Initialise the runtime. `safe_setpoints` is n_setpoints doubles describing the
 * state to drive while in HOLD (required; must be finite). `initial_state` is
 * n_state doubles for the initial state vector (may be NULL when n_state == 0;
 * must be finite otherwise). `seal_key` / `seal_key_len` key the HMAC-SHA256
 * provenance seal (required, len > 0); construction matches the Rust ledger's
 * hmac_sha256_hex. Returns false if the schema exceeds the compile-time caps,
 * n_state > n_inputs/n_outputs, any value is non-finite, or no key is given. */
bool AERIC_Init(aeric_runtime_t *rt,
                const tick_schema_t *schema,
                const double *safe_setpoints,
                const double *initial_state,
                const uint8_t *seal_key, size_t seal_key_len);

/* Run one control cycle. `sensors` is n_sensors finite doubles (the caller never
 * supplies the state — AERIC appends x[t] internally as [sensors | state], solves,
 * publishes the first n_setpoints outputs, and on a LIVE tick advances x[t+1] from
 * the trailing n_state outputs; on HOLD the state is frozen and fail-safe setpoints
 * are published). Returns AERIC_OK / AERIC_HOLD / AERIC_BADCALL. */
int AERIC_Tick(aeric_runtime_t *rt, const double *sensors, size_t n_sensors);

/* Force-clear a latched HOLD (operator acknowledgement). */
void AERIC_ClearFault(aeric_runtime_t *rt);

/* Current published setpoints (n_setpoints doubles). Fail-safe values in HOLD. */
const double *AERIC_Setpoints(const aeric_runtime_t *rt);

/* Current persistent state vector x[t] (n_state doubles; NULL-safe). */
const double *AERIC_State(const aeric_runtime_t *rt);

/* Configure the optional safety envelope (call after Init). For each of the
 * n_setpoints outputs AERIC clamps a live setpoint to [min[i], max[i]] and then
 * limits its change from the previous published value to ±rate[i]. A NULL array
 * leaves that dimension unbounded (-inf / +inf / +inf). Returns false on a null
 * rt or min[i] > max[i]. HOLD still publishes the fail-safe unchanged. */
bool AERIC_SetEnvelope(aeric_runtime_t *rt,
                       const double *min, const double *max, const double *rate);

/* Record a typed HOLD tick for faults the core cannot see itself (deadline,
 * transport): latches HOLD, publishes fail-safe, freezes state, advances the
 * ledger, and tags the reason. Returns AERIC_HOLD (or AERIC_BADCALL). */
int AERIC_HoldTick(aeric_runtime_t *rt, aeric_fault_t reason);

/* Human-readable fault name ("none", "sensor", "solver", "deadline", "transport"). */
const char *AERIC_FaultName(aeric_fault_t reason);

/* Finalise a snapshot of the HMAC-SHA256 provenance seal into out_digest[32].
 * The running inner context is cloned, so AERIC_Seal may be called repeatedly
 * (e.g. at state transitions or shutdown) without disturbing the ongoing chain.
 * Without the key the digest cannot be forged or recomputed. */
void AERIC_Seal(aeric_runtime_t *rt, uint8_t out_digest[32]);

/* One-shot SHA-256 and HMAC-SHA256 (the same primitives the seal uses), exposed
 * so an edge daemon can emit a host-verifiable ledger (chain hash + signatures)
 * in the Rust ledger's format. `out` is 32 bytes. HMAC matches hmac_sha256_hex
 * (key > 64 bytes is SHA-256'd, then zero-padded to the 64-byte block). */
void AERIC_SHA256(const uint8_t *data, size_t len, uint8_t out[32]);
void AERIC_HMAC_SHA256(const uint8_t *key, size_t key_len,
                       const uint8_t *msg, size_t msg_len, uint8_t out[32]);

#endif /* AERIC_H */
