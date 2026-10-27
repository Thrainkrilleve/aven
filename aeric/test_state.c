/* test_state.c -- proves AERIC statefulness with the PI controller.
 *
 * Drives a unit step (r=1, y=0). A stateful controller's integral accumulates,
 * so u and the state i must RAMP across ticks (a stateless model would repeat
 * the same value forever). Tick 7 induces a HOLD (bad arity) to show the state
 * is FROZEN and fail-safe is published, then the controller resumes from the
 * frozen integral. Prints one line per tick for an independent checker. */
#include "aeric.h"
#include <stdio.h>
#include <string.h>

int main(void){
    tick_schema_t s;
    s.version=1; s.n_inputs=3; s.n_outputs=2; s.n_state=1; s.recover_ticks=1;
    double safe[1]={0.0};   /* fail-safe setpoint u=0 */
    double x0[1]={0.0};     /* initial integral = 0 */
    const char *key="pi-key";
    aeric_runtime_t rt;
    if(!AERIC_Init(&rt,&s,safe,x0,(const unsigned char*)key,strlen(key))){ printf("init-fail\n"); return 1; }

    double step[2]={1.0,0.0};   /* r=1, y=0 */
    for(int k=1;k<=11;k++){
        int st;
        if(k==7){ double d=0.0; st=AERIC_Tick(&rt,&d,0); }  /* bad arity -> HOLD */
        else      st=AERIC_Tick(&rt,step,2);
        const double *u=AERIC_Setpoints(&rt);
        const double *x=AERIC_State(&rt);
        printf("tick=%d status=%d u=%.17g i=%.17g\n",k,st,u[0],x[0]);
    }
    return 0;
}
