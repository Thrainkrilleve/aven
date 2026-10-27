/* backend_c.c -- Aven-C backend: drives the ACTUAL avenc emit-c output
 * (aven_ProductionProfile, compiled from examples/server_capacity.aven).
 * Reads battery.txt, writes out_c.txt as raw IEEE-754 bits, and times the
 * solve-only loop. This is the real artifact under test, not a reimplementation. */
#define _POSIX_C_SOURCE 199309L /* clock_gettime under -std=c99 */
#include <stdio.h>
#include <stdlib.h>
#include <stdint.h>
#include <string.h>

/* declared in ../aeric/model_server_capacity.c (compiled alongside) */
void aven_ProductionProfile(
    double avg_latency_ms, double avg_payload_kb, double framing_overhead_ratio,
    double os_kernel_ram_mb, double ram_per_worker, double reqs_per_second,
    double socket_buffer_mb, double target_utilization,
    double* out_raw_workers, double* out_allocated_workers,
    double* out_total_ram_needed, double* out_payload_gbps, double* out_wire_rate_gbps);

#ifdef _WIN32
#include <windows.h>
static double now_s(void){ LARGE_INTEGER f,c; QueryPerformanceFrequency(&f);
    QueryPerformanceCounter(&c); return (double)c.QuadPart/(double)f.QuadPart; }
#else
#include <time.h>
static double now_s(void){ struct timespec t; clock_gettime(CLOCK_MONOTONIC,&t);
    return t.tv_sec + t.tv_nsec*1e-9; }
#endif

static uint64_t bits(double d){ uint64_t u; memcpy(&u,&d,8); return u; }

int main(int argc, char** argv){
    const char* sp = argc>1 ? argv[1] : ".";
    char path[1024]; snprintf(path,sizeof path,"%s/battery.txt",sp);
    FILE* f = fopen(path,"r");
    if(!f){ fprintf(stderr,"cannot open %s\n",path); return 1; }
    int n,nin,nout;
    if(fscanf(f,"%d %d %d",&n,&nin,&nout)!=3){ fprintf(stderr,"bad header\n"); return 1; }
    double* in = (double*)malloc((size_t)n*8*sizeof(double));
    for(int i=0;i<n;i++) for(int j=0;j<8;j++)
        if(fscanf(f,"%lf",&in[(size_t)i*8+j])!=1){ fprintf(stderr,"parse @%d\n",i); return 1; }
    fclose(f);

    double* out = (double*)malloc((size_t)n*5*sizeof(double));
    for(int i=0;i<n;i++){
        double* v=&in[(size_t)i*8]; double* o=&out[(size_t)i*5];
        aven_ProductionProfile(v[0],v[1],v[2],v[3],v[4],v[5],v[6],v[7],
                               &o[0],&o[1],&o[2],&o[3],&o[4]);
    }
    snprintf(path,sizeof path,"%s/out_c.txt",sp);
    FILE* of=fopen(path,"w");
    for(int i=0;i<n;i++){ double* o=&out[(size_t)i*5];
        fprintf(of,"%d %016llx %016llx %016llx %016llx %016llx\n", i,
            (unsigned long long)bits(o[0]),(unsigned long long)bits(o[1]),
            (unsigned long long)bits(o[2]),(unsigned long long)bits(o[3]),
            (unsigned long long)bits(o[4])); }
    fclose(of);

    /* timing: solve-only, K reps over the whole battery, volatile sink */
    int K=200; volatile double sink=0.0;
    double t0=now_s();
    for(int k=0;k<K;k++) for(int i=0;i<n;i++){
        double* v=&in[(size_t)i*8]; double a,b,c,d,e;
        aven_ProductionProfile(v[0],v[1],v[2],v[3],v[4],v[5],v[6],v[7],&a,&b,&c,&d,&e);
        sink+=b;
    }
    double t1=now_s();
    double ns=(t1-t0)/((double)n*K)*1e9;
    printf("{\"backend\":\"c\",\"n\":%d,\"ns_per_solve\":%.3f,"
           "\"solves_per_sec\":%.0f,\"_sink\":%g}\n", n, ns, 1e9/ns, (double)sink);
    free(in); free(out);
    return 0;
}
