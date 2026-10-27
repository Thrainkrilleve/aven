/* model_pi_controller.h -- declaration for the AVEN-generated PI controller.
 * Produced by: avenc emit-c examples/pi_controller.aven --traceable (main stripped).
 * Inputs alphabetical (i_prev, r, y); outputs alphabetical (i_next, u). */
#ifndef MODEL_PI_CONTROLLER_H
#define MODEL_PI_CONTROLLER_H
void aven_PIController(double i_prev, double r, double y,
                      double* out_i_next, double* out_u);
#endif /* MODEL_PI_CONTROLLER_H */
