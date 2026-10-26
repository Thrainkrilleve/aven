// backend_cpp.cpp -- hand-written C++ baseline of the ProductionProfile model.
// Independent reimplementation (replicating Aven's exact parenthesization and
// the 1e-9 ceil-snap) so agreement with Aven-C/Rust/Python is real cross-checking.
#include <cstdio>
#include <cstdlib>
#include <cstdint>
#include <cstring>
#include <cmath>
#include <vector>
#include <chrono>

static inline double aven_ceil(double v){
    double r = std::round(v);                 // half away from zero, like C round()
    return (std::fabs(v - r) < 1e-9) ? r : std::ceil(v);
}

static inline void solve(const double* v, double* o){
    double raw   = (v[5] * v[0]) / 1000.0;                 // reqs*lat/1000
    double alloc = aven_ceil(raw / v[7]);                  // ceil(raw/util)
    double total = (alloc * (v[4] + v[6])) + v[3];         // alloc*(rpw+sock)+osk
    double payload = ((v[5] * v[1]) * 8.0) / 1000000.0;    // reqs*pay*8/1e6
    double wire  = payload * v[2];                         // payload*framing
    o[0]=raw; o[1]=alloc; o[2]=total; o[3]=payload; o[4]=wire;
}

static inline uint64_t bits(double d){ uint64_t u; std::memcpy(&u,&d,8); return u; }

int main(int argc, char** argv){
    const char* sp = argc>1 ? argv[1] : ".";
    char path[1024]; std::snprintf(path,sizeof path,"%s/battery.txt",sp);
    FILE* f = std::fopen(path,"r");
    if(!f){ std::fprintf(stderr,"cannot open %s\n",path); return 1; }
    int n,nin,nout;
    if(std::fscanf(f,"%d %d %d",&n,&nin,&nout)!=3){ std::fprintf(stderr,"bad header\n"); return 1; }
    std::vector<double> in((size_t)n*8), out((size_t)n*5);
    for(int i=0;i<n;i++) for(int j=0;j<8;j++)
        if(std::fscanf(f,"%lf",&in[(size_t)i*8+j])!=1){ std::fprintf(stderr,"parse @%d\n",i); return 1; }
    std::fclose(f);

    for(int i=0;i<n;i++) solve(&in[(size_t)i*8], &out[(size_t)i*5]);

    std::snprintf(path,sizeof path,"%s/out_cpp.txt",sp);
    FILE* of=std::fopen(path,"w");
    for(int i=0;i<n;i++){ const double* o=&out[(size_t)i*5];
        std::fprintf(of,"%d %016llx %016llx %016llx %016llx %016llx\n", i,
            (unsigned long long)bits(o[0]),(unsigned long long)bits(o[1]),
            (unsigned long long)bits(o[2]),(unsigned long long)bits(o[3]),
            (unsigned long long)bits(o[4])); }
    std::fclose(of);

    int K=200; volatile double sink=0.0;
    auto t0=std::chrono::steady_clock::now();
    for(int k=0;k<K;k++) for(int i=0;i<n;i++){ double o[5]; solve(&in[(size_t)i*8],o); sink+=o[1]; }
    auto t1=std::chrono::steady_clock::now();
    double ns=std::chrono::duration<double,std::nano>(t1-t0).count()/((double)n*K);
    std::printf("{\"backend\":\"cpp\",\"n\":%d,\"ns_per_solve\":%.3f,"
                "\"solves_per_sec\":%.0f,\"_sink\":%g}\n", n, ns, 1e9/ns, (double)sink);
    return 0;
}
