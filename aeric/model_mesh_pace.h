/* model_mesh_pace.h -- declaration for the AVEN-generated mesh pace controller.
 *
 * Produced by:  avenc emit-c mesh_pace.aven --traceable --no-main -o model_mesh_pace.c
 *
 * ABI (exactly what avenc emits, not hand-chosen):
 *   - every free input is a scalar double BY VALUE, in avenc's canonical
 *     ALPHABETICAL order: gain, observed_rate, pace_prev, target_rate;
 *   - the single solved variable (pace_cmd) is a double* out-param;
 *   - returns void; the default (non-checked) emit never aborts -- a bad domain
 *     yields NaN/Inf, which the shim must catch.
 */
#ifndef MODEL_MESH_PACE_H
#define MODEL_MESH_PACE_H

#ifndef AVEN_EXPORT
#define AVEN_EXPORT
#endif

AVEN_EXPORT void aven_MeshPace(
    double gain,
    double observed_rate,
    double pace_prev,
    double target_rate,
    double* out_pace_cmd);

#endif /* MODEL_MESH_PACE_H */
