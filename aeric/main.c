/* main.c -- compilable AERIC test driver for the server_capacity model.
 *
 * Exercises: a nominal solve, a domain fault (target_utilization = 0 -> Inf),
 * a malformed-arity tick, and auto-recovery after the stability threshold.
 * Prints the published setpoints, the chained heartbeat hash, and finally the
 * SHA-256 provenance seal.
 *
 * Build (see Makefile / build.sh):
 *   cc -std=c99 -O2 -Wall -Wextra aeric.c aven_shim.c model_server_capacity.c \
 *      main.c -lm -o aeric_demo
 */
#include "aeric.h"
#include <stdio.h>

static const char *status_str(int s) {
    switch (s) {
        case AERIC_OK:      return "OK   (live setpoints)";
        case AERIC_HOLD:    return "HOLD (fail-safe)";
        case AERIC_BADCALL: return "BADCALL";
        default:            return "?";
    }
}

static void run_tick(aeric_runtime_t *rt, const char *label,
                     const double *inputs, size_t n) {
    int s = AERIC_Tick(rt, inputs, n);
    const double *sp = AERIC_Setpoints(rt);
    printf("%-26s -> %-22s | alloc_workers=%8.3f ram_mb=%10.3f | hash=0x%016llX\n",
           label, status_str(s), sp[1], sp[2],
           (unsigned long long)rt->last_tick_hash);
}

int main(void) {
    tick_schema_t schema;
    schema.version       = 1;
    schema.n_inputs      = 8;
    schema.n_outputs     = 5;
    schema.recover_ticks = 3;   /* 3 clean solves to leave HOLD automatically */

    /* Fail-safe setpoints driven while HOLD: 1 worker, base kernel RAM, no net. */
    const double safe[5] = { 0.0, 1.0, 4096.0, 0.0, 0.0 };

    /* HMAC seal key. In production this comes from secure storage, not source. */
    const uint8_t seal_key[] = "aven-aeric-demo-key-v1";

    aeric_runtime_t rt;
    if (!AERIC_Init(&rt, &schema, safe, seal_key, sizeof(seal_key) - 1)) {
        fprintf(stderr, "AERIC_Init failed\n");
        return 1;
    }

    /* Canonical (alphabetical) input order; matches aven_shim.c. */
    const double nominal[8] = {
        20.0,    /* avg_latency_ms         */
        64.0,    /* avg_payload_kb         */
        1.04,    /* framing_overhead_ratio */
        4096.0,  /* os_kernel_ram_mb       */
        16.0,    /* ram_per_worker         */
        50000.0, /* reqs_per_second        */
        4.0,     /* socket_buffer_mb       */
        0.70     /* target_utilization     */
    };

    double faulty[8];
    for (int i = 0; i < 8; ++i) faulty[i] = nominal[i];
    faulty[7] = 0.0;  /* target_utilization = 0 -> allocated_workers = Inf */

    puts("AERIC demo -- server_capacity (ProductionProfile)\n");
    run_tick(&rt, "nominal #1",        nominal, 8);
    run_tick(&rt, "nominal #2",        nominal, 8);
    run_tick(&rt, "DOMAIN FAULT (u=0)", faulty, 8);
    run_tick(&rt, "WRONG ARITY (n=7)",  nominal, 7);
    run_tick(&rt, "recovering #1",      nominal, 8);
    run_tick(&rt, "recovering #2",      nominal, 8);
    run_tick(&rt, "recovering #3",      nominal, 8);
    run_tick(&rt, "nominal (recovered)",nominal, 8);

    printf("\nfaults=%u  hold=%s  consecutive_ok=%u  ticks=%llu\n",
           rt.fault_count, rt.hold_mode ? "true" : "false",
           rt.consecutive_ok, (unsigned long long)rt.tick_index);

    uint8_t digest[32];
    AERIC_Seal(&rt, digest);
    printf("HMAC-SHA256 seal: ");
    for (int i = 0; i < 32; ++i) printf("%02x", digest[i]);
    printf("\n");
    return 0;
}
