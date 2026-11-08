#!/usr/bin/env bash
# Run Nova's chain stage(s) for the split-host demo. Run ON Nova, in the
# mesh-sim/worker directory (where worker.py lives).
#
#   bash nova_chain_stages.sh          # 5-model chain: ONE stage (nova, GPU, last)
#   bash nova_chain_stages.sh both     # 4-stage demo: blue + gold (two stages)
#   bash nova_chain_stages.sh stop     # stop whatever is running
#
# The nova stage runs real GPU inference via the local llama-server and is the
# last stage in the chain (no downstream). It binds 0.0.0.0 so the Windows
# 'gold' stage can forward to it over the tailnet.
set -u
LLM="${LLM_URL:-http://127.0.0.1:8780}"     # Nova's local llama-server
PORT="${NOVA_PORT:-9303}"
PY="$(command -v python3 || command -v python)"

if [ "${1:-}" = "stop" ]; then
    for id in nova blue gold; do pkill -f "WORKER_ID=$id" 2>/dev/null; done
    [ -f /tmp/nova_chain.pids ] && kill $(cat /tmp/nova_chain.pids) 2>/dev/null
    rm -f /tmp/nova_chain.pids
    echo "stopped Nova stage(s)"
    exit 0
fi
if [ ! -f worker.py ]; then
    echo "error: run this from the directory containing worker.py" >&2
    exit 1
fi

if [ "${1:-}" = "both" ]; then
    # legacy 2-stage layout: blue -> gold
    WORKER_ID=gold LAYERS=23 N_PREDICT=8 LLM_URL="$LLM" PORT=9304 "$PY" worker.py &
    g=$!
    WORKER_ID=blue LAYERS=24 N_PREDICT=8 LLM_URL="$LLM" NEXT_URL="http://127.0.0.1:9304" PORT=9303 "$PY" worker.py &
    b=$!
    echo "$b $g" > /tmp/nova_chain.pids
    echo "started: blue :9303 -> gold :9304   (model $LLM)"
else
    # 5-model chain: a single last stage.
    WORKER_ID=nova LAYERS=28 N_PREDICT=8 LLM_URL="$LLM" PORT="$PORT" "$PY" worker.py &
    n=$!
    echo "$n" > /tmp/nova_chain.pids
    echo "started: nova (last stage) on :$PORT   (GPU model $LLM)"
fi
echo "verify: curl -s localhost:$PORT/health"
echo "stop:   bash nova_chain_stages.sh stop"
