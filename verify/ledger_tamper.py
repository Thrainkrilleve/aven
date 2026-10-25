#!/usr/bin/env python3
"""verify/ledger_tamper.py — Attacks an exported audit log and checks every attack is caught.

Writes a real ledger with `avenc run --ledger`, then applies each tampering technique to a
copy and runs `avenc verify-ledger`. The untouched log must verify; every tampered copy must
be rejected. With a signing key, it also checks that an attacker who recomputes the whole
hash chain (but lacks the key) is still caught.

Usage:  .venv/Scripts/python verify/ledger_tamper.py
"""

import hashlib
import json
import os
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
AVENC = str(ROOT / "target" / "release" / ("avenc.exe" if os.name == "nt" else "avenc"))
MODEL = str(ROOT / "examples" / "fleet.aven")
GENESIS = "0" * 64


def avenc(*args, key=None):
    env = dict(os.environ)
    env.pop("AVEN_LEDGER_KEY", None)
    if key:
        env["AVEN_LEDGER_KEY"] = key
    return subprocess.run([AVENC, *args], capture_output=True, text=True, env=env)


def canonical(f):
    m = f["metadata"]
    meta = "-" if m is None else "ts={};ttl={};stale={};warn={}".format(
        "None" if m["timestamp_ms"] is None else f"Some({m['timestamp_ms']})",
        "None" if m["ttl_ms"] is None else f"Some({m['ttl_ms']})",
        "true" if m["is_stale"] else "false",
        "\x1e".join(m["warnings"]),
    )
    return "\x1f".join([str(f["seq"]), str(f["tick"]), f["kind"], f["name"], f["value_bits"],
                        ",".join(map(str, f["causes"])), f["origin"], meta])


def rechain(lines):
    """What an attacker without the key does: recompute every prev/hash after editing."""
    prev = GENESIS
    for f in lines:
        f["prev"] = prev
        f["hash"] = hashlib.sha256(f"{prev}\x1d{canonical(f)}".encode()).hexdigest()
        prev = f["hash"]
    return lines


def dump(lines, seal=None):
    """Facts plus the original closing seal (an attacker without the key cannot re-sign it)."""
    return "".join(json.dumps(f) + "\n" for f in lines + (seal or []))


def set_value(f, v):
    import struct
    f["value"] = v
    f["value_bits"] = struct.pack(">d", v).hex()


def main():
    tmp = Path(tempfile.mkdtemp(prefix="aven_ledger_"))
    results = []
    for key in (None, "test-signing-key"):
        log = tmp / ("signed.jsonl" if key else "plain.jsonl")
        r = avenc("run", MODEL, "--ledger", str(log), key=key)
        assert r.returncode == 0, r.stderr
        original = log.read_text()
        lines_all = [json.loads(l) for l in original.splitlines()]
        seal = [f for f in lines_all if f.get("seal")]
        base = [f for f in lines_all if not f.get("seal")]
        cost = next(i for i, f in enumerate(base) if f["name"] == "cost")

        def attack(name, text, must_fail=True):
            p = tmp / "attack.jsonl"
            p.write_text(text)
            r = avenc("verify-ledger", str(p), key=key)
            caught = r.returncode != 0
            ok = caught == must_fail
            results.append(ok)
            label = "signed" if key else "unsigned"
            verdict = "ok" if ok else "FAILED"
            outcome = "rejected" if caught else "accepted"
            print(f"[{verdict}] {label:8s} {name:46s} -> {outcome}: {(r.stderr or r.stdout).strip()[:90]}")

        attack("untouched log", original, must_fail=False)

        def edited(fn, chain=False):
            lines = [dict(f) for f in base]
            fn(lines)
            if not chain:
                return dump(lines, seal)
            # A full rewrite also rewrites the seal; its signature (if any) cannot be redone
            new = rechain(lines)
            forged_seal = [dict(s, facts=len(new), head=new[-1]["hash"]) for s in seal]
            return dump(new, forged_seal)

        attack("change a value (value and bits)", edited(lambda L: set_value(L[cost], 40.0)))
        attack("change only the displayed value", edited(lambda L: L[cost].__setitem__("value", 40.0)))
        attack("change a cause", edited(lambda L: L[cost].__setitem__("causes", [1])))
        attack("change an origin", edited(lambda L: L[cost].__setitem__("origin", "user binding")))
        attack("delete a fact", edited(lambda L: L.pop(cost)))
        attack("swap two facts", edited(lambda L: L.__setitem__(slice(cost - 1, cost + 1), [L[cost], L[cost - 1]])))
        attack("duplicate a fact", edited(lambda L: L.insert(cost, dict(L[cost]))))
        # Cutting lines off the end: an unsigned log has no protection against an attacker who
        # also drops the seal; a signed log requires its signed seal
        attack("truncate facts and drop the seal", dump(base[:-2]), must_fail=bool(key))
        attack("truncate facts, keep the old seal", dump(base[:-2], seal))
        # Rewriting the whole chain is undetectable without a key, and caught with one
        attack("edit value + recompute whole chain", edited(lambda L: set_value(L[cost], 40.0), chain=True),
               must_fail=bool(key))
    passed = sum(results)
    print(f"ledger tamper checks: {passed}/{len(results)} as expected")
    sys.exit(0 if passed == len(results) else 1)


if __name__ == "__main__":
    main()
