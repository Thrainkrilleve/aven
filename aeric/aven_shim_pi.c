/* aven_shim_pi.c -- shim for the stateful PI controller.
 *
 * Maps AERIC's canonical layout to the generated function. AERIC's convention is
 *   in  = [ sensors | state ]  = [ r, y | i_prev ]   (n_sensors=2, n_state=1)
 *   out = [ setpoints | state] = [ u | i_next ]      (n_setpoints=1, n_state=1)
 * The generated aven_PIController takes/returns alphabetical order
 * (i_prev, r, y) -> (i_next, u), so the shim reorders.
 */
#include "aeric.h"
#include "model_pi_controller.h"
#include <math.h>

#define N_IN  3   /* r, y, i_prev */
#define N_OUT 2   /* u, i_next    */

int aven_solve(const double *in, size_t n_in, double *out, size_t n_out) {
    if (!in || !out) return 1;
    if (n_in != N_IN || n_out != N_OUT) return 1;
    double r = in[0], y = in[1], i_prev = in[2];   /* [sensors | state] */
    double i_next, u;
    aven_PIController(i_prev, r, y, &i_next, &u);
    if (!(isfinite(u) && isfinite(i_next))) return 2;
    out[0] = u;        /* setpoint  */
    out[1] = i_next;   /* state x[t+1] */
    return 0;
}
