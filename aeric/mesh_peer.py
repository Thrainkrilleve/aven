#!/usr/bin/env python3
"""mesh_peer.py -- AERIC sensor/actuator peer for the iSharded mesh simulator.

Bridges the mesh's HTTP admin API to the AERIC daemon's length-prefixed TCP
frame protocol (see aeric_daemon.c). Each tick:

  1. GET  <mesh>/admin/status   -> observed tokens/s and the currently applied
                                   throttle (control.pace_ms = pace_prev).
  2. send a sensor frame to the daemon, in avenc-alphabetical order:
        [gain, observed_rate, pace_prev, target_rate]
  3. recv the daemon's setpoint frame: [pace_cmd] (ms of per-token throttle).
  4. POST <mesh>/admin/control {"pace_ms": pace_cmd}  -> actuate.

The integrator lives in the actuator (the coordinator clamps the applied pace
and reports it back as pace_prev), so there is no unbounded state in the loop.
Dependency-free (Python standard library only).

The key is read from $ISHARDED_KEY (or --key) and sent only as a request header.

Usage:
  python mesh_peer.py --port 5556 --url http://127.0.0.1:8080 \
      --target 0.40 --gain 2000 --interval 1.0 --ticks 40
"""
import argparse
import json
import os
import socket
import struct
import sys
import time
import urllib.error
import urllib.request

N_SENSORS = 4   # gain, observed_rate, pace_prev, target_rate
N_SETPTS = 1    # pace_cmd


def log(*a):
    print(*a, file=sys.stderr, flush=True)


def http_json(url, key, method="GET", data=None, timeout=5):
    body = json.dumps(data).encode() if data is not None else None
    req = urllib.request.Request(url, method=method, data=body)
    req.add_header("Accept", "application/json")
    if body is not None:
        req.add_header("Content-Type", "application/json")
    if key:
        req.add_header("Authorization", f"Bearer {key}")
    with urllib.request.urlopen(req, timeout=timeout) as r:
        return json.loads(r.read().decode())


def recv_all(s, n):
    b = b""
    while len(b) < n:
        c = s.recv(n - len(b))
        if not c:
            raise ConnectionError("daemon closed the connection")
        b += c
    return b


def exchange(s, sensors):
    """Send one sensor frame, return the setpoint tuple."""
    payload = struct.pack("<%dd" % N_SENSORS, *sensors)
    s.sendall(struct.pack("<I", len(payload)) + payload)
    (ln,) = struct.unpack("<I", recv_all(s, 4))
    return struct.unpack("<%dd" % N_SETPTS, recv_all(s, ln))


def connect_daemon(port, tries=100):
    for _ in range(tries):
        try:
            return socket.create_connection(("127.0.0.1", port), timeout=2)
        except OSError:
            time.sleep(0.05)
    raise SystemExit(f"could not connect to AERIC daemon on :{port}")


def main():
    p = argparse.ArgumentParser(description="AERIC peer for the iSharded mesh sim.")
    p.add_argument("--port", type=int, default=5556, help="AERIC daemon TCP port")
    p.add_argument("--url", default=os.environ.get("ISHARDED_URL", "http://127.0.0.1:8080"))
    p.add_argument("--key", default=os.environ.get("ISHARDED_KEY", "dev-user-key"))
    p.add_argument("--target", type=float, default=0.40, help="target tokens/s setpoint")
    p.add_argument("--gain", type=float, default=2000.0, help="controller gain (ms per tok/s)")
    p.add_argument("--interval", type=float, default=1.0, help="seconds between ticks")
    p.add_argument("--ticks", type=int, default=40, help="ticks to run (0 = forever)")
    a = p.parse_args()

    s = connect_daemon(a.port)
    s.settimeout(5.0)
    log(f"peer: mesh {a.url}  daemon :{a.port}  target {a.target} tok/s  gain {a.gain}")
    print("tick | observed_rate | pace_prev | -> pace_cmd  (target %.3f tok/s)" % a.target)
    print("-" * 64)

    i = 0
    while a.ticks == 0 or i < a.ticks:
        i += 1
        try:
            st = http_json(a.url + "/admin/status", a.key)
            y = st.get("metrics", {}).get("tokens_per_second_ema")
            pace_prev = (st.get("control") or {}).get("pace_ms", 0.0)
            if y is None:
                log(f"#{i}: no observed rate yet; skipping")
                time.sleep(a.interval)
                continue
            sensors = [a.gain, float(y), float(pace_prev), a.target]
            (pace_cmd,) = exchange(s, sensors)
            applied = http_json(a.url + "/admin/control", a.key, "POST",
                                {"pace_ms": pace_cmd}).get("pace_ms", pace_cmd)
            err = float(y) - a.target
            flag = "FAST" if err > 0.01 else ("slow" if err < -0.01 else " ok ")
            print(f"#{i:<3} | {float(y):8.4f}     | {float(pace_prev):7.1f}   | "
                  f"{applied:7.1f} ms  [{flag} e={err:+.3f}]", flush=True)
        except urllib.error.URLError as e:
            log(f"#{i}: mesh unreachable: {e.reason}")
        except (ConnectionError, struct.error) as e:
            log(f"#{i}: daemon link lost: {e}")
            break
        time.sleep(a.interval)

    s.close()
    log("peer: done")
    return 0


if __name__ == "__main__":
    sys.exit(main())
