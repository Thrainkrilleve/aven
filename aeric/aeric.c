/* aeric.c -- Hardened AERIC orchestrator implementation (C99). */
#include "aeric.h"
#include <string.h>  /* memset, memcpy        */
#include <math.h>    /* isfinite              */

/* ======================================================================
 * SHA-256 (FIPS 180-4). Compact, self-contained, static. Used only for the
 * provenance seal; the per-tick heartbeat uses the cheaper chained FNV-1a
 * below. Verified against the standard "abc" test vector.
 * ====================================================================== */

static uint32_t sha_ror(uint32_t x, uint32_t n) { return (x >> n) | (x << (32 - n)); }

static const uint32_t SHA_K[64] = {
    0x428a2f98u,0x71374491u,0xb5c0fbcfu,0xe9b5dba5u,0x3956c25bu,0x59f111f1u,
    0x923f82a4u,0xab1c5ed5u,0xd807aa98u,0x12835b01u,0x243185beu,0x550c7dc3u,
    0x72be5d74u,0x80deb1feu,0x9bdc06a7u,0xc19bf174u,0xe49b69c1u,0xefbe4786u,
    0x0fc19dc6u,0x240ca1ccu,0x2de92c6fu,0x4a7484aau,0x5cb0a9dcu,0x76f988dau,
    0x983e5152u,0xa831c66du,0xb00327c8u,0xbf597fc7u,0xc6e00bf3u,0xd5a79147u,
    0x06ca6351u,0x14292967u,0x27b70a85u,0x2e1b2138u,0x4d2c6dfcu,0x53380d13u,
    0x650a7354u,0x766a0abbu,0x81c2c92eu,0x92722c85u,0xa2bfe8a1u,0xa81a664bu,
    0xc24b8b70u,0xc76c51a3u,0xd192e819u,0xd6990624u,0xf40e3585u,0x106aa070u,
    0x19a4c116u,0x1e376c08u,0x2748774cu,0x34b0bcb5u,0x391c0cb3u,0x4ed8aa4au,
    0x5b9cca4fu,0x682e6ff3u,0x748f82eeu,0x78a5636fu,0x84c87814u,0x8cc70208u,
    0x90befffau,0xa4506cebu,0xbef9a3f7u,0xc67178f2u
};

static void sha_compress(uint32_t state[8], const uint8_t block[64]) {
    uint32_t w[64];
    for (int i = 0; i < 16; ++i) {
        w[i] = ((uint32_t)block[i*4] << 24) | ((uint32_t)block[i*4+1] << 16)
             | ((uint32_t)block[i*4+2] << 8) | ((uint32_t)block[i*4+3]);
    }
    for (int i = 16; i < 64; ++i) {
        uint32_t s0 = sha_ror(w[i-15],7) ^ sha_ror(w[i-15],18) ^ (w[i-15] >> 3);
        uint32_t s1 = sha_ror(w[i-2],17) ^ sha_ror(w[i-2],19) ^ (w[i-2] >> 10);
        w[i] = w[i-16] + s0 + w[i-7] + s1;
    }
    uint32_t a=state[0],b=state[1],c=state[2],d=state[3];
    uint32_t e=state[4],f=state[5],g=state[6],h=state[7];
    for (int i = 0; i < 64; ++i) {
        uint32_t S1 = sha_ror(e,6) ^ sha_ror(e,11) ^ sha_ror(e,25);
        uint32_t ch = (e & f) ^ (~e & g);
        uint32_t t1 = h + S1 + ch + SHA_K[i] + w[i];
        uint32_t S0 = sha_ror(a,2) ^ sha_ror(a,13) ^ sha_ror(a,22);
        uint32_t maj = (a & b) ^ (a & c) ^ (b & c);
        uint32_t t2 = S0 + maj;
        h=g; g=f; f=e; e=d+t1; d=c; c=b; b=a; a=t1+t2;
    }
    state[0]+=a; state[1]+=b; state[2]+=c; state[3]+=d;
    state[4]+=e; state[5]+=f; state[6]+=g; state[7]+=h;
}

static void sha_init(aeric_sha256_t *s) {
    s->state[0]=0x6a09e667u; s->state[1]=0xbb67ae85u;
    s->state[2]=0x3c6ef372u; s->state[3]=0xa54ff53au;
    s->state[4]=0x510e527fu; s->state[5]=0x9b05688cu;
    s->state[6]=0x1f83d9abu; s->state[7]=0x5be0cd19u;
    s->bitlen = 0; s->fill = 0;
    memset(s->block, 0, sizeof(s->block));
}

static void sha_update(aeric_sha256_t *s, const uint8_t *data, size_t len) {
    s->bitlen += (uint64_t)len * 8u;
    for (size_t i = 0; i < len; ++i) {
        s->block[s->fill++] = data[i];
        if (s->fill == 64) { sha_compress(s->state, s->block); s->fill = 0; }
    }
}

/* Finalise a COPY so the live context keeps streaming. */
static void sha_final_snapshot(const aeric_sha256_t *src, uint8_t out[32]) {
    aeric_sha256_t s = *src;
    uint64_t bitlen = s.bitlen;
    uint8_t pad = 0x80;
    sha_update(&s, &pad, 1);
    uint8_t zero = 0x00;
    while (s.fill != 56) sha_update(&s, &zero, 1);
    uint8_t lb[8];
    for (int i = 0; i < 8; ++i) lb[i] = (uint8_t)(bitlen >> (56 - i*8));
    sha_update(&s, lb, 8);
    for (int i = 0; i < 8; ++i) {
        out[i*4]   = (uint8_t)(s.state[i] >> 24);
        out[i*4+1] = (uint8_t)(s.state[i] >> 16);
        out[i*4+2] = (uint8_t)(s.state[i] >> 8);
        out[i*4+3] = (uint8_t)(s.state[i]);
    }
}

/* ======================================================================
 * Chained FNV-1a 64-bit heartbeat.
 * ====================================================================== */
static const uint64_t FNV_OFFSET = 14695981039346656037ULL;
static const uint64_t FNV_PRIME  = 1099511628211ULL;

static uint64_t fnv1a(uint64_t h, const void *data, size_t len) {
    const uint8_t *p = (const uint8_t*)data;
    for (size_t i = 0; i < len; ++i) { h ^= p[i]; h *= FNV_PRIME; }
    return h;
}

/* ======================================================================
 * Arena (per-tick ledger record scratch). O(1) alloc/reset, bounded.
 * ====================================================================== */
static void arena_reset(aeric_arena_t *a) { a->write_pos = 0; }

static void *arena_alloc(aeric_arena_t *a, size_t bytes) {
    if (bytes > AERIC_ARENA_CAPACITY || a->write_pos > AERIC_ARENA_CAPACITY - bytes)
        return NULL;
    void *p = a->buffer + a->write_pos;
    a->write_pos += bytes;
    return p;
}

/* Build this tick's ledger record in the arena, update the chained heartbeat
 * (prev hash folded in => tamper-evident chain) and the SHA-256 seal, then
 * reset the arena. Status is one of the AERIC_* result codes. */
static void ledger_record(aeric_runtime_t *rt, int status) {
    arena_reset(&rt->arena);

    size_t out_bytes = rt->schema.n_outputs * sizeof(double);
    size_t rec_len = sizeof(uint64_t)   /* tick_index */
                   + sizeof(uint64_t)   /* prev hash  */
                   + sizeof(int32_t)    /* status     */
                   + out_bytes;         /* setpoints  */
    uint8_t *rec = (uint8_t*)arena_alloc(&rt->arena, rec_len);
    if (!rec) return; /* cannot happen for bounded schemas; fail closed on ledger */

    size_t off = 0;
    uint64_t ti = rt->tick_index;
    uint64_t prev = rt->last_tick_hash;
    int32_t  st = (int32_t)status;
    memcpy(rec + off, &ti,   sizeof ti);   off += sizeof ti;
    memcpy(rec + off, &prev, sizeof prev); off += sizeof prev;
    memcpy(rec + off, &st,   sizeof st);   off += sizeof st;
    memcpy(rec + off, rt->outputs, out_bytes); off += out_bytes;

    rt->last_tick_hash = fnv1a(FNV_OFFSET, rec, rec_len);
    sha_update(&rt->seal, rec, rec_len);

    arena_reset(&rt->arena);
}

/* ======================================================================
 * Public API
 * ====================================================================== */

bool AERIC_Init(aeric_runtime_t *rt,
                const tick_schema_t *schema,
                const double *safe_setpoints,
                const uint8_t *seal_key, size_t seal_key_len) {
    if (!rt || !schema || !safe_setpoints) return false;
    if (!seal_key || seal_key_len == 0) return false;   /* keyed seal is mandatory */
    if (schema->n_inputs  > AERIC_MAX_INPUTS)  return false;
    if (schema->n_outputs > AERIC_MAX_OUTPUTS) return false;
    for (size_t i = 0; i < schema->n_outputs; ++i)
        if (!isfinite(safe_setpoints[i])) return false;

    memset(rt, 0, sizeof *rt);
    rt->schema = *schema;
    memcpy(rt->safe, safe_setpoints, schema->n_outputs * sizeof(double));
    /* Start in the fail-safe state until the first clean solve publishes. */
    memcpy(rt->outputs, safe_setpoints, schema->n_outputs * sizeof(double));
    rt->hold_mode = false;
    rt->last_tick_hash = FNV_OFFSET;

    /* HMAC-SHA256 key schedule (matches ledger.rs hmac_sha256_hex): a key longer
     * than the 64-byte block is SHA-256'd first, then zero-padded to 64 bytes. */
    uint8_t kblock[64];
    memset(kblock, 0, sizeof kblock);
    if (seal_key_len > 64) {
        uint8_t kd[32];
        aeric_sha256_t ks; sha_init(&ks);
        sha_update(&ks, seal_key, seal_key_len);
        sha_final_snapshot(&ks, kd);
        memcpy(kblock, kd, 32);
    } else {
        memcpy(kblock, seal_key, seal_key_len);
    }
    uint8_t k_ipad[64];
    for (int i = 0; i < 64; ++i) {
        k_ipad[i]        = (uint8_t)(kblock[i] ^ 0x36);
        rt->seal_opad[i] = (uint8_t)(kblock[i] ^ 0x5c);
    }
    sha_init(&rt->seal);
    sha_update(&rt->seal, k_ipad, 64);   /* inner = SHA256(K^ipad || records...) */
    return true;
}

int AERIC_Tick(aeric_runtime_t *rt, const double *inputs, size_t n_inputs) {
    if (!rt || !inputs) return AERIC_BADCALL;

    rt->tick_index++;

    /* 1. Strict ingress: exact arity and finite sensor values. */
    bool ingress_ok = (n_inputs == rt->schema.n_inputs);
    for (size_t i = 0; ingress_ok && i < n_inputs; ++i)
        if (!isfinite(inputs[i])) ingress_ok = false;

    if (!ingress_ok) {
        rt->fault_count++;
        rt->consecutive_ok = 0;
        rt->hold_mode = true;
        memcpy(rt->outputs, rt->safe, rt->schema.n_outputs * sizeof(double));
        ledger_record(rt, AERIC_HOLD);
        return AERIC_HOLD;
    }

    /* 2. Staging copy (the solver reads our buffer, never the caller's). */
    memcpy(rt->inputs, inputs, n_inputs * sizeof(double));

    /* 3. Run the AVEN solver via the stable shim. */
    double tmp[AERIC_MAX_OUTPUTS];
    int rc = aven_solve(rt->inputs, rt->schema.n_inputs, tmp, rt->schema.n_outputs);

    bool solved = (rc == 0);
    for (size_t j = 0; solved && j < rt->schema.n_outputs; ++j)
        if (!isfinite(tmp[j])) solved = false;   /* belt-and-braces vs the shim */

    /* 4. Fault -> latch HOLD, publish fail-safe. */
    if (!solved) {
        rt->fault_count++;
        rt->consecutive_ok = 0;
        rt->hold_mode = true;
        memcpy(rt->outputs, rt->safe, rt->schema.n_outputs * sizeof(double));
        ledger_record(rt, AERIC_HOLD);
        return AERIC_HOLD;
    }

    /* 5. Clean solve. If latched, count toward auto-recovery but keep holding
     *    until the stability threshold is met (or an operator clears it). */
    if (rt->hold_mode) {
        rt->consecutive_ok++;
        if (rt->schema.recover_ticks != 0 &&
            rt->consecutive_ok >= rt->schema.recover_ticks) {
            rt->hold_mode = false;
        }
        if (rt->hold_mode) {
            memcpy(rt->outputs, rt->safe, rt->schema.n_outputs * sizeof(double));
            ledger_record(rt, AERIC_HOLD);
            return AERIC_HOLD;
        }
    }

    /* 6. Normal operation: publish live setpoints. */
    rt->consecutive_ok++;
    memcpy(rt->outputs, tmp, rt->schema.n_outputs * sizeof(double));
    ledger_record(rt, AERIC_OK);
    return AERIC_OK;
}

void AERIC_ClearFault(aeric_runtime_t *rt) {
    if (!rt) return;
    rt->hold_mode = false;
    rt->consecutive_ok = 0;
}

const double *AERIC_Setpoints(const aeric_runtime_t *rt) {
    return rt ? rt->outputs : NULL;
}

void AERIC_Seal(aeric_runtime_t *rt, uint8_t out_digest[32]) {
    if (!rt || !out_digest) return;
    /* HMAC finalise: outer = SHA256(K^opad || SHA256(K^ipad || records)).
     * sha_final_snapshot clones the inner context, so streaming continues. */
    uint8_t inner[32];
    sha_final_snapshot(&rt->seal, inner);
    aeric_sha256_t outer;
    sha_init(&outer);
    sha_update(&outer, rt->seal_opad, 64);
    sha_update(&outer, inner, 32);
    sha_final_snapshot(&outer, out_digest);
}

void AERIC_SHA256(const uint8_t *data, size_t len, uint8_t out[32]) {
    aeric_sha256_t s;
    sha_init(&s);
    sha_update(&s, data, len);
    sha_final_snapshot(&s, out);
}

void AERIC_HMAC_SHA256(const uint8_t *key, size_t key_len,
                       const uint8_t *msg, size_t msg_len, uint8_t out[32]) {
    uint8_t kb[64];
    memset(kb, 0, sizeof kb);
    if (key_len > 64) { uint8_t kd[32]; AERIC_SHA256(key, key_len, kd); memcpy(kb, kd, 32); }
    else memcpy(kb, key, key_len);
    uint8_t ip[64], op[64];
    for (int i = 0; i < 64; ++i) { ip[i] = (uint8_t)(kb[i] ^ 0x36); op[i] = (uint8_t)(kb[i] ^ 0x5c); }
    aeric_sha256_t s; sha_init(&s); sha_update(&s, ip, 64); sha_update(&s, msg, msg_len);
    uint8_t inner[32]; sha_final_snapshot(&s, inner);
    aeric_sha256_t o; sha_init(&o); sha_update(&o, op, 64); sha_update(&o, inner, 32);
    sha_final_snapshot(&o, out);
}
