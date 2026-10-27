#!/usr/bin/env python3
"""
openplc_poller.py

Polls an OpenPLC runtime/simulator via Modbus TCP and prints
JSON suitable for Aven (one object per line).

Works with both older and newer pymodbus APIs.

Requirements:
    pip install pymodbus

Usage:
    python openplc_poller.py
    python openplc_poller.py --host 127.0.0.1 --port 502 --interval 1.0
"""

import argparse
import json
import time
import sys

try:
    from pymodbus.client import ModbusTcpClient
except ImportError:
    print("Install pymodbus:  pip install pymodbus", file=sys.stderr)
    sys.exit(1)


# Adjust these if your located variables use different addresses
REGISTER_MAP = {
    0: ("scan_ms",          0.1),
    1: ("io_points",        1.0),
    2: ("power_w",          0.1),
    3: ("temp_c",           0.1),
    4: ("throughput_pph",   0.1),
    5: ("alarm_count",      1.0),
    6: ("cpu_load_pct",     0.1),
    7: ("network_load_pct", 0.1),
    8: ("quality_pct",      0.1),
}


def read_holding(client, address, count):
    """Compatible read for different pymodbus versions."""
    try:
        # Newer pymodbus (3.x) uses keyword for count
        rr = client.read_holding_registers(address, count=count)
    except TypeError:
        try:
            # Older style
            rr = client.read_holding_registers(address, count)
        except Exception as e:
            print(f"Modbus error: {e}", file=sys.stderr)
            return None
    except Exception as e:
        print(f"Modbus error: {e}", file=sys.stderr)
        return None

    if rr is None or rr.isError():
        return None
    return rr.registers


def build_payload(registers, base=0):
    data = {}
    for addr, (name, scale) in REGISTER_MAP.items():
        idx = addr - base
        if registers and 0 <= idx < len(registers):
            data[name] = round(registers[idx] * scale, 3)
        else:
            data[name] = None
    return data


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--host", default="127.0.0.1")
    parser.add_argument("--port", type=int, default=502)
    parser.add_argument("--interval", type=float, default=1.0)
    parser.add_argument("--start", type=int, default=0)
    parser.add_argument("--count", type=int, default=len(REGISTER_MAP))
    args = parser.parse_args()

    client = ModbusTcpClient(args.host, port=args.port)
    if not client.connect():
        print(f"Could not connect to {args.host}:{args.port}", file=sys.stderr)
        sys.exit(1)

    print(f"Connected to OpenPLC at {args.host}:{args.port}", file=sys.stderr)
    print("Emitting JSON lines (Ctrl-C to stop)...", file=sys.stderr)

    try:
        while True:
            regs = read_holding(client, args.start, args.count)
            if regs is not None:
                payload = build_payload(regs, base=args.start)
                payload["ts"] = time.time()
                print(json.dumps(payload), flush=True)
            else:
                print(json.dumps({"error": "modbus_read_failed", "ts": time.time()}), flush=True)
            time.sleep(args.interval)
    except KeyboardInterrupt:
        print("\nStopped.", file=sys.stderr)
    finally:
        client.close()


if __name__ == "__main__":
    main()