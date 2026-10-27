#!/usr/bin/env python3
"""verify/fuzz.py — Mutation fuzzer for the avenc lexer, parser, checker and solver.

Feeds mutated example models and adversarial inputs to `avenc check` and `avenc run`.
A failure is anything other than a clean result or a clean error:
  * a Rust panic ("panicked at" on stderr)
  * an exit code outside the documented 0–3 (e.g. stack overflow 0xC00000FD)
  * no answer within the time limit (hang)

Usage:  .venv/Scripts/python verify/fuzz.py --cases 2000 --seed 1
"""

import argparse
import os
import random
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
AVENC = str(ROOT / "target" / "release" / ("avenc.exe" if os.name == "nt" else "avenc"))
FAIL_DIR = ROOT / "verify" / "failures"
TIMEOUT_S = 15

SEEDS = [p.read_text() for p in sorted((ROOT / "examples").glob("*.aven"))]
TOKENS = [
    "relation", "forward", "solve", "with", "using", "external", "ipc", "unit", "signal",
    "minimize", "maximize", "int", "integer", "{", "}", "(", ")", "=", "==", "!=", "<=", ">=",
    "<", ">", "+", "-", "*", "/", "^", ",", ":", "\"", "/*", "*/", "//", "\n", "x", "y",
    "1e308", "-1e308", "1e-320", "0", "0.0", "-0", "9" * 400, "1e", "1e+", ".", "..", "1.2.3",
    "sqrt", "ln", "tan", "ceil", "floor", "abs", "exp", "sin", "cos", "pi", "tau", "km/h",
    "m^2", "MB", "Gbps", "é", "\x00", "\t", "\r", "\\",
]


def mutate(rng, text):
    toks = text.split(" ")
    for _ in range(rng.randint(1, 8)):
        op = rng.random()
        i = rng.randrange(len(toks)) if toks else 0
        if op < 0.25 and toks:
            del toks[i]
        elif op < 0.5:
            toks.insert(i, rng.choice(TOKENS))
        elif op < 0.65 and toks:
            toks[i] = rng.choice(TOKENS)
        elif op < 0.8 and toks:
            j = rng.randrange(len(toks))
            toks[i], toks[j] = toks[j], toks[i]
        else:
            toks.insert(i, toks[rng.randrange(len(toks))] if toks else "x")
    return " ".join(toks)


def adversarial(rng):
    depth = rng.choice([50, 500, 5000, 50000])
    kind = rng.randrange(8)
    if kind == 0:
        return f"relation R {{\n y = {'(' * depth}x{')' * depth}\n}}\nsolve R with {{\n x = 1\n}}\n"
    if kind == 1:
        return f"relation R {{\n y = {'-' * depth}x\n}}\nsolve R with {{\n x = 1\n}}\n"
    if kind == 2:
        chain = " + ".join(["x"] * depth)
        return f"relation R {{\n y = {chain}\n}}\nsolve R with {{\n x = 1\n}}\n"
    if kind == 3:
        return f"relation R {{\n y = x {'^ 2 ' * min(depth, 2000)}\n}}\nsolve R with {{\n x = 1.0001\n}}\n"
    if kind == 4:
        eqs = "\n".join(f" v{i + 1} = v{i} * 1.0001 + 1" for i in range(min(depth, 3000)))
        return f"relation R {{\n{eqs}\n}}\nsolve R with {{\n v0 = 1\n}}\n"
    if kind == 5:
        return "/* unterminated " + "x" * depth
    if kind == 6:
        return "relation R using external(\"" + "a" * depth
    return "".join(chr(rng.randrange(1, 0x2FF)) for _ in range(rng.randint(1, 400)))


def check(src, case, args, tally):
    with tempfile.NamedTemporaryFile("w", suffix=".aven", delete=False, encoding="utf-8") as f:
        f.write(src)
        path = f.name
    try:
        for cmd in (["check", path], ["run", path, "--json"]):
            # External adapters in mutated examples could run arbitrary commands; skip them
            if cmd[0] == "run" and ("external" in src or "ipc" in src):
                continue
            try:
                p = subprocess.run([AVENC, *cmd], capture_output=True, text=True,
                                   timeout=TIMEOUT_S, errors="replace")
            except subprocess.TimeoutExpired:
                verdict, info = "HANG", f"{cmd[0]} exceeded {TIMEOUT_S}s"
            else:
                if "panicked at" in p.stderr:
                    verdict = "PANIC"
                    info = p.stderr[p.stderr.find("panicked at"):][:300]
                elif p.returncode not in (0, 1, 2, 3):
                    verdict, info = f"CRASH_EXIT_{p.returncode:#x}", p.stderr[-300:]
                else:
                    verdict, info = "ok", ""
            tally[verdict] = tally.get(verdict, 0) + 1
            if verdict != "ok":
                FAIL_DIR.mkdir(parents=True, exist_ok=True)
                out = FAIL_DIR / f"fuzz_{args.seed}_{case}_{cmd[0]}.aven"
                out.write_text(f"// {verdict}: {info!r}\n{src}", encoding="utf-8", errors="replace")
                if args.verbose:
                    print(f"[case {case} {cmd[0]}] {verdict}: {info[:160]!r}")
    finally:
        os.unlink(path)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--cases", type=int, default=500)
    ap.add_argument("--seed", type=int, default=1)
    ap.add_argument("--verbose", action="store_true")
    args = ap.parse_args()
    rng = random.Random(args.seed)
    tally = {}
    for case in range(args.cases):
        src = adversarial(rng) if rng.random() < 0.25 else mutate(rng, rng.choice(SEEDS))
        check(src, case, args, tally)
    summary = ", ".join(f"{v} {k}" for k, v in sorted(tally.items(), key=lambda t: -t[1]))
    print(f"fuzz: {summary}")
    sys.exit(0 if set(tally) <= {"ok"} else 1)


if __name__ == "__main__":
    main()
