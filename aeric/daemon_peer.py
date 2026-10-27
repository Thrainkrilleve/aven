#!/usr/bin/env python3
"""Test peer for aeric_daemon: a sensor/actuator client that drives the three
Level-1 scenarios over the length-prefixed TCP protocol:

  1. happy path   -- stream sensor frames at ~tick rate, read setpoints back
  2. deadline miss -- pause longer than deadline_ms (daemon -> HOLD)
  3. disconnect    -- close (daemon -> HOLD until a peer returns)

Usage: daemon_peer.py <port> [n_inputs] [n_outputs]
"""
import sys, socket, struct, time

port = int(sys.argv[1]) if len(sys.argv) > 1 else 5555
N_IN = int(sys.argv[2]) if len(sys.argv) > 2 else 8
N_OUT = int(sys.argv[3]) if len(sys.argv) > 3 else 5
NOMINAL = [20.0, 64.0, 1.04, 4096.0, 16.0, 50000.0, 4.0, 0.70]
SENSORS = (NOMINAL * ((N_IN + len(NOMINAL) - 1) // len(NOMINAL)))[:N_IN]  # length n_sensors

def recv_all(s, n):
    b = b""
    while len(b) < n:
        c = s.recv(n - len(b))
        if not c: raise ConnectionError("peer closed")
        b += c
    return b

def exchange(s, vals):
    payload = struct.pack("<%dd" % N_IN, *vals)
    s.sendall(struct.pack("<I", len(payload)) + payload)
    (ln,) = struct.unpack("<I", recv_all(s, 4))
    out = struct.unpack("<%dd" % N_OUT, recv_all(s, ln))
    return out

def main():
    # wait for the daemon to be listening
    for _ in range(50):
        try:
            s = socket.create_connection(("127.0.0.1", port), timeout=2); break
        except OSError:
            time.sleep(0.05)
    else:
        print("could not connect"); return 1
    s.settimeout(2.0)

    sent = 0; last = None
    # 1. happy path
    for _ in range(40):
        last = exchange(s, SENSORS); sent += 1; time.sleep(0.015)
    print(f"happy: sent {sent} frames, last setpoint[0]={last[0]:.4g}")
    # 2. deadline miss: go quiet past the deadline
    time.sleep(0.25)
    print("paused 250 ms (expect daemon HOLD ticks)")
    # 3. resume briefly, then disconnect
    for _ in range(20):
        last = exchange(s, SENSORS); sent += 1; time.sleep(0.015)
    s.close()
    print(f"resumed + disconnected: total {sent} live exchanges")
    return 0

if __name__ == "__main__":
    sys.exit(main())
