/* aven_shim.c -- model-specific adapter: stable AERIC ABI -> generated AVEN fn.
 *
 * This is the ONLY place that knows the generated function's exact shape. One
 * shim per linked model; swap this file (and model_server_capacity.*) to drive
 * a different .aven model. AERIC itself never changes.
 *
 * Canonical input order (avenc sorts free inputs alphabetically):
 *   [0] avg_latency_ms        [1] avg_payload_kb
 *   [2] framing_overhead_ratio[3] os_kernel_ram_mb
 *   [4] ram_per_worker        [5] reqs_per_second
 *   [6] socket_buffer_mb      [7] target_utilization
 *
 * Output order (forward-block equation order):
 *   [0] raw_workers           [1] allocated_workers
 *   [2] total_ram_needed      [3] payload_gbps
 *   [4] wire_rate_gbps
 */
#include "aeric.h"
#include "model_server_capacity.h"
#include <math.h>

#define MODEL_N_IN   8
#define MODEL_N_OUT  5

int aven_solve(const double *in, size_t n_in, double *out, size_t n_out) {
    if (!in || !out) return 1;
    if (n_in != MODEL_N_IN || n_out != MODEL_N_OUT) return 1; /* contract mismatch */

    double r_raw, r_alloc, r_ram, r_payload, r_wire;
    aven_ProductionProfile(
        in[0], in[1], in[2], in[3], in[4], in[5], in[6], in[7],
        &r_raw, &r_alloc, &r_ram, &r_payload, &r_wire);

    /* Non-aborting domain check: the default emit never calls exit(), so a bad
     * input (e.g. target_utilization == 0) surfaces here as NaN/Inf. Reject it
     * so AERIC can HOLD instead of publishing garbage setpoints. */
    if (!(isfinite(r_raw) && isfinite(r_alloc) && isfinite(r_ram) &&
          isfinite(r_payload) && isfinite(r_wire)))
        return 2;

    out[0] = r_raw;
    out[1] = r_alloc;
    out[2] = r_ram;
    out[3] = r_payload;
    out[4] = r_wire;
    return 0;
}
