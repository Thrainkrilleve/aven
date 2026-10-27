/* test_envelope.c -- proves the safety envelope + fault taxonomy.
 *
 * server_capacity solves allocated_workers (out[1]) to 1429 for the nominal
 * input. We clamp out[1] to [0, 1000] and rate-limit it to 50/tick: the
 * published value must RAMP from the fail-safe (1) by 50 each tick, then plateau
 * at the clamp ceiling 1000 — and `clamped` stays set the whole time (1429 > max).
 * Then we trip each fault class and print its reason name. */
#include "aeric.h"
#include <stdio.h>
#include <string.h>
#include <math.h>

int main(void){
    tick_schema_t s; s.version=1; s.n_inputs=8; s.n_outputs=5; s.n_state=0; s.recover_ticks=1;
    double safe[5]={0,1,4096,0,0};
    const char *key="env-key";
    aeric_runtime_t rt;
    if(!AERIC_Init(&rt,&s,safe,NULL,(const unsigned char*)key,strlen(key))){ printf("init-fail\n"); return 1; }

    /* clamp allocated_workers (out[1]) to [0,1000], 50/tick; others unbounded */
    double mn[5]={-INFINITY,0,-INFINITY,-INFINITY,-INFINITY};
    double mx[5]={ INFINITY,1000, INFINITY, INFINITY, INFINITY};
    double rr[5]={ INFINITY,50,   INFINITY, INFINITY, INFINITY};
    if(!AERIC_SetEnvelope(&rt,mn,mx,rr)){ printf("envelope-fail\n"); return 1; }

    double nominal[8]={20,64,1.04,4096,16,50000,4,0.70};
    for(int k=1;k<=25;k++){
        int st=AERIC_Tick(&rt,nominal,8);
        const double *u=AERIC_Setpoints(&rt);
        printf("tick=%d status=%d clamped=%d alloc=%.17g\n",k,st,rt.clamped?1:0,u[1]);
    }

    /* fault taxonomy */
    double bad7[7]; for(int i=0;i<7;i++) bad7[i]=nominal[i];
    AERIC_Tick(&rt,bad7,7);                              /* wrong arity -> sensor */
    printf("fault=sensor name=%s\n",AERIC_FaultName(rt.last_fault));
    double u0[8]; for(int i=0;i<8;i++) u0[i]=nominal[i]; u0[7]=0.0;  /* util=0 -> Inf -> solver */
    AERIC_Tick(&rt,u0,8);
    printf("fault=solver name=%s\n",AERIC_FaultName(rt.last_fault));
    AERIC_HoldTick(&rt,AERIC_FAULT_DEADLINE);
    printf("fault=deadline name=%s\n",AERIC_FaultName(rt.last_fault));
    AERIC_HoldTick(&rt,AERIC_FAULT_TRANSPORT);
    printf("fault=transport name=%s\n",AERIC_FaultName(rt.last_fault));
    printf("counts sensor=%u solver=%u deadline=%u transport=%u clamp=%u\n",
           rt.fault_counts[AERIC_FAULT_SENSOR],rt.fault_counts[AERIC_FAULT_SOLVER],
           rt.fault_counts[AERIC_FAULT_DEADLINE],rt.fault_counts[AERIC_FAULT_TRANSPORT],rt.clamp_count);
    return 0;
}
