#!/usr/bin/env python3
"""verify/c_diff.py — Differential testing of `avenc emit-c` against `avenc run`.

For each random forward model:
  1. `avenc run --json` gives the reference values computed by the Rust evaluator.
  2. `avenc emit-c` (and `--checked-c`) emits C; it is compiled warning-free
     (MSVC /W4 /WX, or cc -std=c99 -Wall -Wextra -Werror) and run.
  3. Every printed output must match the Rust value.
Also checks the README claim that plain emitted C is branchless.

The harness rewrites the emitted printf format from %.6g to %.17g in its own copy so the
comparison is at full double precision; the code generator itself is not modified.

Usage:  .venv/Scripts/python verify/c_diff.py --cases 80 --seed 1
"""

import argparse
import json
import math
import os
import random
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
AVENC = str(ROOT / "target" / "release" / ("avenc.exe" if os.name == "nt" else "avenc"))
FAIL_DIR = ROOT / "verify" / "failures"


def find_vcvars():
    vswhere = Path(r"C:\Program Files (x86)\Microsoft Visual Studio\Installer\vswhere.exe")
    if not vswhere.exists():
        return None
    out = subprocess.run(
        [str(vswhere), "-latest", "-products", "*", "-requires",
         "Microsoft.VisualStudio.Component.VC.Tools.x86.x64", "-property", "installationPath"],
        capture_output=True, text=True,
    ).stdout.strip()
    bat = Path(out) / "VC" / "Auxiliary" / "Build" / "vcvars64.bat"
    return bat if bat.exists() else None


VCVARS = find_vcvars() if os.name == "nt" else None
CC = shutil.which("cc") or shutil.which("gcc") or shutil.which("clang")


def msvc_env():
    """Environment produced by vcvars64.bat, captured once."""
    bat = Path(tempfile.mkdtemp()) / "env.bat"
    bat.write_text(f'@call "{VCVARS}" >nul\n@set\n')
    out = subprocess.run(["cmd", "/c", str(bat)], capture_output=True, text=True).stdout
    env = {}
    for line in out.splitlines():
        if "=" in line:
            k, v = line.split("=", 1)
            env[k] = v
    return env


MSVC_ENV = msvc_env() if VCVARS else None


def compile_c(c_path, exe_path):
    if MSVC_ENV:
        p = subprocess.run(
            ["cl", "/nologo", "/W4", "/WX", "/O2", "/TC", c_path, f"/Fe:{exe_path}", f"/Fo:{exe_path}.obj"],
            capture_output=True, text=True, env=MSVC_ENV, shell=False,
            executable=shutil.which("cl", path=MSVC_ENV.get("PATH") or MSVC_ENV.get("Path")),
        )
    elif CC:
        p = subprocess.run([CC, "-std=c99", "-Wall", "-Wextra", "-Werror", "-O2", c_path, "-o", exe_path, "-lm"],
                           capture_output=True, text=True)
    else:
        sys.exit("no C compiler found")
    return p.returncode == 0, (p.stdout + p.stderr).strip()


FUNCS = ["sqrt", "abs", "ceil", "floor", "exp", "ln", "sin", "cos", "tan", "round"]


def gen_model(rng):
    inputs = {f"i{k}": round(rng.uniform(0.2, 9.0), rng.choice([1, 2, 3])) for k in range(rng.randint(2, 4))}
    vals = dict(inputs)
    eqs = []
    for k in range(rng.randint(2, 6)):
        a, b = rng.choice(list(vals)), rng.choice(list(vals))
        form = rng.choice(["arith", "func", "pow", "near_int"])
        # Intermediate values are only known to avenc, so every form is safe for any input.
        if form == "arith":
            op = rng.choice(["+", "-", "*", "/"])
            expr = f"{a} / (abs({b}) + 0.5)" if op == "/" else f"{a} {op} {b}"
        elif form == "pow":
            expr = f"abs({a}) ^ {rng.choice([2, 3, 0.5])}"
        elif form == "near_int":
            # ceil/floor of a quotient landing within float noise of a whole number,
            # e.g. ceil(1000.3 / 0.7) where the exact quotient is 1429
            whole = rng.randint(2, 2000)
            div = rng.choice([0.7, 0.3, 0.1, 1.1, 0.9, 3.3])
            num = round(whole * div, 6)
            fn = rng.choice(["ceil", "floor"])
            expr = f"{fn}({num} / ({div} + {a} - {a}))"
        else:
            fn = rng.choice(FUNCS)
            arg = {
                "sqrt": f"abs({a}) + 1",
                "ln": f"abs({a}) + 1",
                "exp": f"sin({a})",
                "tan": f"sin({a})",
            }.get(fn, a)
            expr = f"{fn}({arg})"
        name = f"o{k}"
        eqs.append((name, expr))
        vals[name] = None  # value comes from avenc
    lines = ["forward F {"] + [f"    {n} = {e}" for n, e in eqs] + ["}"]
    lines += ["solve F with {"] + [f"    {k} = {v}" for k, v in inputs.items()] + ["}"]
    return "\n".join(lines) + "\n", [n for n, _ in eqs]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--cases", type=int, default=60)
    ap.add_argument("--seed", type=int, default=1)
    ap.add_argument("--verbose", action="store_true")
    args = ap.parse_args()
    rng = random.Random(args.seed)
    tally = {}
    work = Path(tempfile.mkdtemp(prefix="aven_cdiff_"))

    def note(verdict, src, info, case):
        tally[verdict] = tally.get(verdict, 0) + 1
        if verdict != "ok":
            FAIL_DIR.mkdir(parents=True, exist_ok=True)
            (FAIL_DIR / f"cdiff_{args.seed}_{case}.aven").write_text(f"// {verdict}: {info}\n" + src)
            if args.verbose:
                print(f"[case {case}] {verdict}: {info}")

    for case in range(args.cases):
        src, outputs = gen_model(rng)
        model = work / f"m{case}.aven"
        model.write_text(src)
        run = subprocess.run([AVENC, "run", str(model), "--json"], capture_output=True, text=True)
        lines = run.stdout.strip().splitlines()
        if run.returncode != 0 or not lines or not lines[-1].startswith("{"):
            # Domain error in the model itself (e.g. tan pole); not a C question
            continue
        ref = json.loads(lines[-1])["env"]

        for mode in ["plain", "checked"]:
            flags = ["--checked-c"] if mode == "checked" else []
            emit = subprocess.run([AVENC, "emit-c", str(model), *flags], capture_output=True, text=True)
            if emit.returncode != 0:
                note("EMIT_FAILED", src, emit.stderr.strip()[-200:], case)
                continue
            code = emit.stdout
            if mode == "plain":
                body = code[code.find("aven_F("):code.find("int main")]
                if re.search(r"\b(if|else|while|for|switch|goto)\b|\?", body):
                    note("NOT_BRANCHLESS", src, "control flow in emitted function body", case)
            c_path = work / f"m{case}_{mode}.c"
            c_path.write_text(code.replace("%.6g", "%.17g"))
            exe = work / f"m{case}_{mode}.exe"
            ok, log = compile_c(str(c_path), str(exe))
            if not ok:
                note(f"COMPILE_FAILED_{mode.upper()}", src, log[-300:], case)
                continue
            out = subprocess.run([str(exe)], capture_output=True, text=True)
            if out.returncode != 0:
                note(f"C_RUNTIME_EXIT_{mode.upper()}", src, (out.stderr or out.stdout)[-200:], case)
                continue
            got = {}
            for line in out.stdout.splitlines():
                m = re.match(r"\s*(\w+) = (\S+)", line)
                if m:
                    got[m.group(1)] = float(m.group(2))
            worst, where = 0.0, None
            for name in outputs:
                r, c = ref.get(name), got.get(name)
                if r is None or c is None:
                    worst, where = math.inf, f"{name} missing (rust={r}, c={c})"
                    break
                err = abs(r - c) / (1 + abs(r))
                if err > worst:
                    worst, where = err, f"{name}: rust={r!r} c={c!r}"
            note("ok" if worst <= 1e-12 else f"MISMATCH_{mode.upper()}", src, where, case)

    shutil.rmtree(work, ignore_errors=True)
    summary = ", ".join(f"{v} {k}" for k, v in sorted(tally.items(), key=lambda t: -t[1]))
    print(f"emit-c vs run: {summary}")
    sys.exit(0 if set(tally) <= {"ok"} else 1)


if __name__ == "__main__":
    main()
