/* model_server_capacity.h -- declaration for the AVEN-generated solver.
 *
 * The implementation in model_server_capacity.c was produced verbatim by:
 *     avenc emit-c examples/server_capacity.aven --traceable
 * (with the emitter's demo main() stripped so it links as a library unit).
 *
 * ABI reminder -- this is exactly what avenc emits, not a hand-chosen shape:
 *   - every free input is a scalar `double` passed BY VALUE, in avenc's
 *     canonical ALPHABETICAL order;
 *   - every solved variable is a `double*` out-param, in the forward block's
 *     equation order;
 *   - the function returns void and (in the default, non-checked emit) never
 *     aborts -- a bad domain yields NaN/Inf, which the shim must catch.
 */
#ifndef MODEL_SERVER_CAPACITY_H
#define MODEL_SERVER_CAPACITY_H

void aven_ProductionProfile(
    double avg_latency_ms,
    double avg_payload_kb,
    double framing_overhead_ratio,
    double os_kernel_ram_mb,
    double ram_per_worker,
    double reqs_per_second,
    double socket_buffer_mb,
    double target_utilization,
    double* out_raw_workers,
    double* out_allocated_workers,
    double* out_total_ram_needed,
    double* out_payload_gbps,
    double* out_wire_rate_gbps);

#endif /* MODEL_SERVER_CAPACITY_H */
