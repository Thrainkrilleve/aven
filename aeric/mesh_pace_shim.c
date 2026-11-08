/* mesh_pace_shim.c -- model-specific adapter: stable AERIC ABI -> aven_MeshPace.
 *
 * The ONLY place that knows the generated function's exact shape. AERIC passes
 * in[] in [sensors | state] order; this model is stateless (n_state == 0), so
 * in[] is exactly the sensor frame in avenc's canonical alphabetical order:
 *
 *   [0] gain           [1] observed_rate
 *   [2] pace_prev      [3] target_rate
 *
 * Output (forward-block equation order):
 *   [0] pace_cmd
 */
#include "aeric.h"
#include "model_mesh_pace.h"
#include <math.h>

#define MODEL_N_IN   4
#define MODEL_N_OUT  1

int aven_solve(const double *in, size_t n_in, double *out, size_t n_out) {
    if (!in || !out) return 1;
    if (n_in != MODEL_N_IN || n_out != MODEL_N_OUT) return 1; /* contract mismatch */

    double pace_cmd;
    aven_MeshPace(in[0], in[1], in[2], in[3], &pace_cmd);

    /* Non-aborting domain check: reject NaN/Inf so AERIC HOLDs (fail-safe pace)
     * instead of publishing a garbage setpoint. */
    if (!isfinite(pace_cmd)) return 2;

    out[0] = pace_cmd;
    return 0;
}
