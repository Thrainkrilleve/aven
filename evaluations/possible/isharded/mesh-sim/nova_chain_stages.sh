#!/usr/bin/env bash
# Run the Nova-side chain stages (blue, gold) for the split-host demo.
# Run this ON Nova, in the mesh-sim/worker directory (where worker.py lives).
#
#   bash nova_chain_stages.sh          # start blue + gold
#   bash nova_chain_stages.sh stop     # stop them
#
# blue  = stage 3: real GPU inference via the local llama-server, forwards to gold.
# gold  = stage 4 (last): real GPU inference, no downstream.
# Both bind 0.0.0.0 so the Windows 'green' stage can reach blue over the tailnet.
set -u
LLM="${LLM_URL:-http://127.0.0.1:8780}"     # Nova's local llama-server
BLUE_PORT="${BLUE_PORT:-9303}"
GOLD_PORT="${GOLD_PORT:-9304}"
PY="$(command -v python3 || command -v python)"

if [ "${1:-}" = "stop" ]; then
    pkill -f "WORKER_ID=blue" 2>/dev/null
    pkill -f "WORKER_ID=gold" 2>/dev/null
    # fallback: kill by the recorded pids
    [ -f /tmp/nova_chain.pids ] && kill $(cat /tmp/nova_chain.pids) 2>/dev/null
    rm -f /tmp/nova_chain.pids
    echo "stopped blue + gold"
    exit 0
fi

if [ ! -f worker.py ]; then
    echo "error: run this from the directory containing worker.py" >&2
    exit 1
fi

# gold first (blue needs to forward to it)
WORKER_ID=gold LAYERS=23 N_PREDICT=8 LLM_URL="$LLM" PORT="$GOLD_PORT" "$PY" worker.py &
g=$!
WORKER_ID=blue LAYERS=24 N_PREDICT=8 LLM_URL="$LLM" NEXT_URL="http://127.0.0.1:$GOLD_PORT" PORT="$BLUE_PORT" "$PY" worker.py &
b=$!
echo "$b $g" > /tmp/nova_chain.pids
echo "started: blue pid=$b on :$BLUE_PORT  ->  gold pid=$g on :$GOLD_PORT   (model $LLM)"
echo "verify:  curl -s localhost:$BLUE_PORT/health ; curl -s localhost:$GOLD_PORT/health"
echo "stop:    bash nova_chain_stages.sh stop"
