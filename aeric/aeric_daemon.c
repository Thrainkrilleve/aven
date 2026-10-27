/* aeric_daemon.c -- AERIC Level-1 standalone daemon (C99).
 *
 * Turns the AERIC library into a backend: loads a .aer runtime config, runs a
 * fixed-rate control loop that pulls sensor frames over TCP and pushes setpoint
 * frames back, drops to HOLD on a missed deadline or a lost peer, and appends a
 * hash-chained, HMAC-sealed ledger to disk.
 *
 * Transport (length-prefixed TCP, little-endian):
 *   peer -> daemon : u32 len, then len bytes = n_inputs  float64   (sensors)
 *   daemon -> peer : u32 len, then len bytes = n_outputs float64   (setpoints)
 * One tick = read one sensor frame within the deadline, solve, write setpoints.
 * No sensor frame in time (or peer gone) => deadline/transport fault => HOLD.
 *
 * The AERIC core (aeric.c) is untouched and transport-free; this is the edge
 * wiring around it. Build: see build_daemon notes in README.
 */
#include "aeric.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#ifdef _WIN32
  #include <winsock2.h>
  #include <ws2tcpip.h>
  #ifdef _MSC_VER
  #pragma comment(lib, "ws2_32.lib")
  #endif
  typedef SOCKET sock_t;
  #define BAD_SOCK INVALID_SOCKET
  #define closesock closesocket
  static void sock_startup(void){ WSADATA w; WSAStartup(MAKEWORD(2,2), &w); }
  static void sock_cleanup(void){ WSACleanup(); }
  static void set_rcv_timeout(sock_t s, int ms){ DWORD t=(DWORD)ms; setsockopt(s,SOL_SOCKET,SO_RCVTIMEO,(const char*)&t,sizeof t); }
  static int sock_would_block(void){ int e=WSAGetLastError(); return e==WSAETIMEDOUT||e==WSAEWOULDBLOCK; }
  #include <windows.h>
  #include <timeapi.h>
  #ifdef _MSC_VER
  #pragma comment(lib, "winmm.lib")
  #endif
  static void hires_begin(void){ timeBeginPeriod(1); }   /* 1 ms scheduler tick */
  static void hires_end(void){ timeEndPeriod(1); }
  static double now_s(void){ LARGE_INTEGER f,c; QueryPerformanceFrequency(&f); QueryPerformanceCounter(&c); return (double)c.QuadPart/(double)f.QuadPart; }
  static void sleep_s(double s){ if(s>0) Sleep((DWORD)(s*1000.0)); }
#else
  #include <sys/socket.h>
  #include <netinet/in.h>
  #include <arpa/inet.h>
  #include <unistd.h>
  #include <errno.h>
  #include <time.h>
  #include <sys/select.h>
  typedef int sock_t;
  #define BAD_SOCK (-1)
  #define closesock close
  static void sock_startup(void){}
  static void sock_cleanup(void){}
  static void set_rcv_timeout(sock_t s, int ms){ struct timeval t; t.tv_sec=ms/1000; t.tv_usec=(ms%1000)*1000; setsockopt(s,SOL_SOCKET,SO_RCVTIMEO,&t,sizeof t); }
  static int sock_would_block(void){ return errno==EAGAIN||errno==EWOULDBLOCK; }
  static void hires_begin(void){}
  static void hires_end(void){}
  static double now_s(void){ struct timespec t; clock_gettime(CLOCK_MONOTONIC,&t); return t.tv_sec+t.tv_nsec*1e-9; }
  static void sleep_s(double s){ if(s>0){ struct timespec t; t.tv_sec=(time_t)s; t.tv_nsec=(long)((s-(double)t.tv_sec)*1e9); nanosleep(&t,NULL); } }
#endif

/* portable readable-wait on a socket: 1=readable, 0=timeout, -1=error */
static int wait_readable(sock_t s, int ms){
    fd_set r; FD_ZERO(&r); FD_SET(s,&r);
    struct timeval tv; tv.tv_sec=ms/1000; tv.tv_usec=(ms%1000)*1000;
    int n=select((int)s+1,&r,NULL,NULL,&tv);
    return n>0?1:(n==0?0:-1);
}

/* ---------------- minimal .aer config (key = value, # comments) ------------- */
typedef struct {
    size_t   n_inputs, n_outputs;
    unsigned recover_ticks;
    double   tick_hz;
    int      deadline_ms;
    int      tcp_port;
    unsigned max_ticks;          /* 0 = run until Ctrl+C */
    char     ledger[512];
    char     seal_key[256];      /* literal key (demo); real use: seal_key_env */
    char     seal_key_env[128];
    double   safe[AERIC_MAX_OUTPUTS];
    size_t   n_safe;
} daemon_cfg;

static char *trim(char *s){ while(*s==' '||*s=='\t')s++; char*e=s+strlen(s); while(e>s&&(e[-1]=='\n'||e[-1]=='\r'||e[-1]==' '||e[-1]=='\t'))*--e=0; return s; }

static int load_cfg(const char *path, daemon_cfg *c){
    memset(c,0,sizeof *c);
    c->tick_hz=50; c->deadline_ms=40; c->tcp_port=5555; c->recover_ticks=3;
    strcpy(c->ledger,"aeric_ledger.jsonl");
    FILE *f=fopen(path,"r"); if(!f){ fprintf(stderr,"cannot open config '%s'\n",path); return 0; }
    char line[1024];
    while(fgets(line,sizeof line,f)){
        char *cmt=strchr(line,'#'); if(cmt) *cmt=0;   /* '#' starts a comment (anywhere) */
        char *h=line; while(*h==' '||*h=='\t')h++;
        if(*h=='\n'||*h=='\r'||*h==0) continue;
        char *eq=strchr(h,'='); if(!eq) continue; *eq=0;
        char *k=trim(h), *v=trim(eq+1);
        if(!strcmp(k,"n_inputs")) c->n_inputs=(size_t)strtoul(v,0,10);
        else if(!strcmp(k,"n_outputs")) c->n_outputs=(size_t)strtoul(v,0,10);
        else if(!strcmp(k,"recover_ticks")) c->recover_ticks=(unsigned)strtoul(v,0,10);
        else if(!strcmp(k,"tick_hz")) c->tick_hz=strtod(v,0);
        else if(!strcmp(k,"deadline_ms")) c->deadline_ms=(int)strtol(v,0,10);
        else if(!strcmp(k,"tcp_port")) c->tcp_port=(int)strtol(v,0,10);
        else if(!strcmp(k,"max_ticks")) c->max_ticks=(unsigned)strtoul(v,0,10);
        else if(!strcmp(k,"ledger")) { strncpy(c->ledger,v,sizeof c->ledger-1); }
        else if(!strcmp(k,"seal_key")) { strncpy(c->seal_key,v,sizeof c->seal_key-1); }
        else if(!strcmp(k,"seal_key_env")) { strncpy(c->seal_key_env,v,sizeof c->seal_key_env-1); }
        else if(!strcmp(k,"safe")) {
            c->n_safe=0; char *p=v;
            while(*p && c->n_safe<AERIC_MAX_OUTPUTS){ c->safe[c->n_safe++]=strtod(p,&p); if(*p==',')p++; }
        }
    }
    fclose(f);
    if(c->n_inputs==0||c->n_outputs==0){ fprintf(stderr,"config: n_inputs/n_outputs required\n"); return 0; }
    if(c->n_safe!=c->n_outputs){ fprintf(stderr,"config: 'safe' needs %zu values, got %zu\n",c->n_outputs,c->n_safe); return 0; }
    return 1;
}

/* ---------------- framed TCP I/O ------------------------------------------- */
/* returns 0=ok, 1=timeout/incomplete, -1=closed/error */
static int recv_all(sock_t s, void *buf, size_t n){
    uint8_t *p=(uint8_t*)buf; size_t got=0;
    while(got<n){
        int r=recv(s,(char*)p+got,(int)(n-got),0);
        if(r>0){ got+=(size_t)r; continue; }
        if(r==0) return -1;                       /* peer closed */
        return sock_would_block()?1:-1;           /* timeout vs error */
    }
    return 0;
}
static int send_all(sock_t s, const void *buf, size_t n){
    const uint8_t *p=(const uint8_t*)buf; size_t sent=0;
    while(sent<n){ int r=send(s,(const char*)p+sent,(int)(n-sent),0); if(r<=0) return -1; sent+=(size_t)r; }
    return 0;
}
/* read one length-prefixed input frame of exactly n_inputs doubles */
static int read_frame(sock_t s, double *inb, size_t n_in){
    uint32_t len; int r=recv_all(s,&len,4); if(r) return r;
    if(len!=n_in*sizeof(double)) return -1;       /* schema violation */
    return recv_all(s,inb,len);
}
static int write_frame(sock_t s, const double *outb, size_t n_out){
    uint32_t len=(uint32_t)(n_out*sizeof(double));
    if(send_all(s,&len,4)) return -1;
    return send_all(s,outb,len);
}

/* ---------------- host-verifiable ledger (Rust ledger JSONL format) ---------
 * One fact per setpoint, chained with SHA-256 over the canonical form
 *   seq \x1f tick \x1f kind \x1f name \x1f value_bits:016x \x1f causes \x1f origin \x1f meta
 * hashed as SHA256(prev_hex \x1d canonical); each fact and the closing seal are
 * HMAC-signed. Verifiable by `avenc verify-ledger <file> --key <key>`. */
static void hex32(const uint8_t *d, char *out){
    static const char *H="0123456789abcdef";
    for(int i=0;i<32;i++){ out[2*i]=H[d[i]>>4]; out[2*i+1]=H[d[i]&15]; }
    out[64]=0;
}
static void write_fact(FILE *lf, const char *key, size_t keylen, char prev[65],
                       uint64_t *seq, uint64_t tick, const char *name, double val){
    (*seq)++;
    uint64_t bits; memcpy(&bits,&val,8);
    char canon[600];
    int cn=snprintf(canon,sizeof canon,"%llu\x1f%llu\x1f%s\x1f%s\x1f%016llx\x1f\x1f%s\x1f%s",
        (unsigned long long)*seq,(unsigned long long)tick,"setpoint",name,
        (unsigned long long)bits,"aeric","-");
    uint8_t msg[800]; size_t ml=0;
    memcpy(msg,prev,64); ml=64; msg[ml++]=0x1d; memcpy(msg+ml,canon,(size_t)cn); ml+=(size_t)cn;
    uint8_t h[32]; AERIC_SHA256(msg,ml,h); char hhex[65]; hex32(h,hhex);
    uint8_t sg[32]; AERIC_HMAC_SHA256((const uint8_t*)key,keylen,(const uint8_t*)hhex,64,sg);
    char sghex[65]; hex32(sg,sghex);
    fprintf(lf,"{\"seq\": %llu, \"tick\": %llu, \"kind\": \"setpoint\", \"name\": \"%s\", ",
            (unsigned long long)*seq,(unsigned long long)tick,name);
    if(val==val && val-val==0.0) fprintf(lf,"\"value\": %.17g, ",val);
    else fprintf(lf,"\"value\": null, ");
    fprintf(lf,"\"value_bits\": \"%016llx\", \"causes\": [], \"origin\": \"aeric\", \"metadata\": null, "
               "\"prev\": \"%s\", \"hash\": \"%s\", \"sig\": \"%s\"}\n",
            (unsigned long long)bits,prev,hhex,sghex);
    fflush(lf);
    memcpy(prev,hhex,65);
}

int main(int argc, char **argv){
    const char *cfg_path = argc>1 ? argv[1] : "daemon.aer";
    daemon_cfg cfg;
    if(!load_cfg(cfg_path,&cfg)) return 1;

    const char *key = cfg.seal_key;
    if(cfg.seal_key_env[0]){ const char *e=getenv(cfg.seal_key_env); if(e) key=e; }
    if(!key[0]){ fprintf(stderr,"no seal key (set 'seal_key' or 'seal_key_env')\n"); return 1; }

    tick_schema_t schema; schema.version=1; schema.n_inputs=cfg.n_inputs;
    schema.n_outputs=cfg.n_outputs; schema.recover_ticks=cfg.recover_ticks;
    aeric_runtime_t rt;
    if(!AERIC_Init(&rt,&schema,cfg.safe,(const uint8_t*)key,strlen(key))){ fprintf(stderr,"AERIC_Init failed\n"); return 1; }

    FILE *lf=fopen(cfg.ledger,"w"); if(!lf){ fprintf(stderr,"cannot open ledger '%s'\n",cfg.ledger); return 1; }

    sock_startup();
    hires_begin();
    sock_t srv=socket(AF_INET,SOCK_STREAM,0);
    int yes=1; setsockopt(srv,SOL_SOCKET,SO_REUSEADDR,(const char*)&yes,sizeof yes);
    struct sockaddr_in addr; memset(&addr,0,sizeof addr);
    addr.sin_family=AF_INET; addr.sin_addr.s_addr=htonl(INADDR_LOOPBACK); addr.sin_port=htons((unsigned short)cfg.tcp_port);
    if(bind(srv,(struct sockaddr*)&addr,sizeof addr)||listen(srv,1)){ fprintf(stderr,"bind/listen failed on port %d\n",cfg.tcp_port); return 1; }
    printf("AERIC daemon: port %d, %.0f Hz, deadline %d ms, ledger '%s'\n",cfg.tcp_port,cfg.tick_hz,cfg.deadline_ms,cfg.ledger);
    fflush(stdout);

    const double period = 1.0/cfg.tick_hz;
    double dummy=0.0, next=now_s(), max_jit=0.0, last=next;
    uint64_t tick=0; unsigned live=0, held=0;
    sock_t cli=BAD_SOCK;
    char prev[65]; memset(prev,'0',64); prev[64]=0;   /* ledger chain head = genesis */
    uint64_t seq=0;

    while(cfg.max_ticks==0 || tick<cfg.max_ticks){
        int status;

        if(cli==BAD_SOCK){
            if(wait_readable(srv,cfg.deadline_ms)==1){
                cli=accept(srv,NULL,NULL);
                if(cli!=BAD_SOCK) set_rcv_timeout(cli,cfg.deadline_ms);
            }
        }
        if(cli==BAD_SOCK){
            status=AERIC_Tick(&rt,&dummy,0);            /* no peer -> HOLD tick */
        } else {
            double inb[AERIC_MAX_INPUTS];
            int fr=read_frame(cli,inb,cfg.n_inputs);
            if(fr==0){
                status=AERIC_Tick(&rt,inb,cfg.n_inputs); /* live solve */
                if(write_frame(cli,AERIC_Setpoints(&rt),cfg.n_outputs)){ closesock(cli); cli=BAD_SOCK; }
            } else {
                status=AERIC_Tick(&rt,&dummy,0);          /* deadline miss / schema fault -> HOLD, no reply */
                if(fr<0){ closesock(cli); cli=BAD_SOCK; } /* peer gone */
            }
        }
        tick++; if(status==AERIC_OK) live++; else held++;
        { const double *sp=AERIC_Setpoints(&rt); char nm[32];
          for(size_t i=0;i<cfg.n_outputs;i++){ snprintf(nm,sizeof nm,"sp%zu",i); write_fact(lf,key,strlen(key),prev,&seq,tick,nm,sp[i]); } }

        double t=now_s(); double jit=(t-last)-period; if(tick>1 && jit>max_jit) max_jit=jit; last=t;
        next+=period; sleep_s(next-now_s());
    }

    /* closing seal: {"seal": true, "facts": N, "head": "<hash>",
     *                "sig": hmac(key, "seal\x1d" N "\x1d" head)} */
    { char sm[160]; int sl=snprintf(sm,sizeof sm,"seal\x1d%llu\x1d%s",(unsigned long long)seq,prev);
      uint8_t sg[32]; AERIC_HMAC_SHA256((const uint8_t*)key,strlen(key),(const uint8_t*)sm,(size_t)sl,sg);
      char sghex[65]; hex32(sg,sghex);
      fprintf(lf,"{\"seal\": true, \"facts\": %llu, \"head\": \"%s\", \"sig\": \"%s\"}\n",
              (unsigned long long)seq,prev,sghex); }
    fclose(lf);

    if(cli!=BAD_SOCK) closesock(cli);
    closesock(srv);
    sock_cleanup();
    hires_end();
    printf("done: %llu ticks (%u live, %u hold), max jitter %.3f ms, faults %u\n",
           (unsigned long long)tick,live,held,max_jit*1000.0,rt.fault_count);
    return 0;
}
