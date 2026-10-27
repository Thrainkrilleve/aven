#!/usr/bin/env python3
"""Independent verifier for an aeric_daemon ledger (aeric_ledger.jsonl).

Reconstructs each tick's exact binary record from the JSON fields, recomputes the
FNV-1a chain and verifies continuity, then recomputes the HMAC-SHA256 seal with
the key and checks it against the seal line. Also runs a tamper test: mutating a
single setpoint bit must break verification.

Usage: verify_ledger.py <ledger.jsonl> <seal_key>
"""
import sys, json, struct, hmac, hashlib

FNV_OFFSET = 14695981039346656037
FNV_PRIME = 1099511628211
MASK = (1 << 64) - 1

def fnv1a(data):
    h = FNV_OFFSET
    for b in data:
        h ^= b; h = (h * FNV_PRIME) & MASK
    return h

def record_bytes(tick, prev, status, bits):
    r = struct.pack("<Q", tick) + struct.pack("<Q", prev) + struct.pack("<i", status)
    for b in bits:
        r += struct.pack("<Q", b)
    return r

def load(path):
    ticks, seal = [], None
    for line in open(path):
        line = line.strip()
        if not line: continue
        o = json.loads(line)
        if o.get("seal"): seal = o
        else: ticks.append(o)
    return ticks, seal

def verify(ticks, seal, key, mutate=None):
    prev = FNV_OFFSET
    msg = b""
    for i, t in enumerate(ticks):
        bits = [int(x, 16) for x in t["bits"]]
        if mutate is not None and i == mutate:
            bits[0] ^= 1  # flip one bit of one setpoint
        rec = record_bytes(t["tick"], prev, t["status"], bits)
        h = fnv1a(rec)
        if int(t["prev"], 16) != prev: return False, f"chain break at tick {t['tick']} (prev mismatch)"
        if mutate is None and h != int(t["hash"], 16): return False, f"hash mismatch at tick {t['tick']}"
        if mutate is not None and i == mutate and h == int(t["hash"], 16): return False, "mutation not detected by hash"
        prev = int(t["hash"], 16)  # chain uses the recorded hash
        msg += rec
    sig = hmac.new(key.encode(), msg, hashlib.sha256).hexdigest()
    if mutate is None:
        if int(seal["head"], 16) != int(ticks[-1]["hash"], 16): return False, "seal head != last hash"
        if sig != seal["sig"]: return False, "seal HMAC mismatch"
        return True, "ok"
    else:
        return (sig != seal["sig"]), "tamper detected by seal" if sig != seal["sig"] else "tamper NOT detected"

def main():
    path, key = sys.argv[1], sys.argv[2]
    ticks, seal = load(path)
    if seal is None: print("no seal line"); return 1
    live = sum(1 for t in ticks if t["status"] == 0)
    hold = len(ticks) - live
    ok, why = verify(ticks, seal, key)
    print(f"ledger: {len(ticks)} ticks ({live} live, {hold} hold), seal over {len(ticks)} records")
    print(f"chain + HMAC seal verification: {'PASS' if ok else 'FAIL - ' + why}")
    # tamper test: mutate a live tick's setpoint, expect detection
    live_idx = next((i for i, t in enumerate(ticks) if t["status"] == 0), 0)
    det, _ = verify(ticks, seal, key, mutate=live_idx)
    print(f"tamper test (flip 1 setpoint bit @ tick {ticks[live_idx]['tick']}): {'DETECTED' if det else 'MISSED'}")
    # wrong key must fail the seal
    wrong, _ = verify(ticks, seal, key + "x")
    print(f"wrong-key seal check: {'correctly rejected' if not wrong else 'WRONGLY ACCEPTED'}")
    return 0 if (ok and det) else 1

if __name__ == "__main__":
    sys.exit(main())
