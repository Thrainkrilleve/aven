#!/usr/bin/env python3
"""check_docs.py — Checks the documentation against the compiler.

Run from the repository root after `cargo build --release`:

    python3 scripts/check_docs.py

For every Markdown file under the repository (excluding vendored and archived trees) and
every page in web/public/:

  * ```aven blocks (and <pre data-lang="aven">) must pass `avenc check`.
  * ```aven-error blocks (and data-lang="aven-error") are deliberate mistakes and must FAIL
    `avenc check`.
  * ```aven-fragment blocks are partial snippets (e.g. they import a package that does not
    exist locally). They are not compiled, only counted.
  * Every `avenc <subcommand>` mentioned must be a real subcommand.
  * A ```bash block holding a single `avenc run <file>` for a file in this repository,
    followed by a ```text block with a "Settled Environment:" section, must match what
    `avenc run` prints: same variables, same numbers, same unit labels.
  * `rust-toolchain.toml` pins a fixed compiler, both linter workflows install that
    file, `docs/proof/lint-toolchain-1.99.1.txt` records exit 0 for fmt and clippy
    on that same channel, and `cargo fmt --check` passes.

Exits non-zero when anything does not match.
"""

import html
import os
import re
import subprocess
import sys
import tempfile
import textwrap
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
AVENC = ROOT / "target" / "release" / ("avenc.exe" if os.name == "nt" else "avenc")
SKIP_DIRS = {"target", "node_modules", "archive", ".git", ".wrangler", "evaluations"}
TOOLCHAIN = ROOT / "rust-toolchain.toml"
PROOF_LOG = ROOT / "docs" / "proof" / "lint-toolchain-1.99.1.txt"
LINTER_WORKFLOWS = (
    ROOT / ".github" / "workflows" / "linter.yml",
    ROOT / ".forgejo" / "workflows" / "linter.yml",
)
FLOATING_CHANNELS = {"stable", "beta", "nightly"}

FENCE = re.compile(r"^([ \t]*)```([\w-]*)[^\n]*\n(.*?)^\1```", re.S | re.M)
PRE = re.compile(r'<pre data-lang="(aven(?:-error)?)">(.*?)</pre>', re.S)
COMMAND = re.compile(r"(?:^|(?<=`)|(?<=\$ ))[ \t]*(?:\./target/release/)?avenc ([a-z][a-z-]*)\b(?![.\d])", re.M)
# Code where commands appear: fenced blocks, inline `code`, and <code>/<pre> in HTML
CODE_SPANS = re.compile(r"```.*?```|`[^`]+`|<code>.*?</code>|<pre[^>]*>.*?</pre>", re.S)
SETTLED_LINE = re.compile(r"^\s+([A-Za-z_]\w*)\s+=\s+(\S+)(?:\s+(.*?))?\s*$")


def line_of(text, index):
    return text.count("\n", 0, index) + 1


def doc_files():
    for path in sorted(ROOT.rglob("*.md")):
        if not SKIP_DIRS.intersection(path.relative_to(ROOT).parts):
            yield path
    yield from sorted((ROOT / "web" / "public").glob("*.html"))


def avenc(*args, cwd=ROOT):
    return subprocess.run([str(AVENC), *args], capture_output=True, text=True, cwd=cwd, timeout=120)


def check_source(code):
    with tempfile.TemporaryDirectory() as tmp:
        path = Path(tmp) / "snippet.aven"
        path.write_text(code, encoding="utf-8")
        result = avenc("check", str(path))
    message = (result.stderr.strip() or result.stdout.strip()).splitlines()
    return result.returncode == 0, (message[-1] if message else "")


def settled(output):
    """name -> (number, unit label) from a "Settled Environment:" section."""
    env, inside = {}, False
    for line in output.splitlines():
        if line.startswith("Settled Environment:"):
            inside = True
            continue
        if inside:
            if not line.strip() or line.startswith("Fact Ledger"):
                break
            m = SETTLED_LINE.match(line)
            if m:
                unit = (m.group(3) or "").replace("(int)", "").strip()
                env[m.group(1)] = (float(m.group(2)), unit)
    return env


def toolchain_problems():
    """The lint pin is a file, the workflows read that file, and fmt still passes."""
    problems = []
    if not TOOLCHAIN.exists():
        return ["rust-toolchain.toml is missing"]
    text = TOOLCHAIN.read_text(encoding="utf-8")
    match = re.search(r'(?m)^channel\s*=\s*"([^"]+)"', text)
    if not match:
        return ["rust-toolchain.toml has no channel pin"]
    channel = match.group(1)
    if channel in FLOATING_CHANNELS:
        problems.append(f"rust-toolchain.toml channel {channel} is not a fixed compiler")
    for path in LINTER_WORKFLOWS:
        rel = path.relative_to(ROOT)
        body = path.read_text(encoding="utf-8") if path.exists() else ""
        if "rust-toolchain.toml" not in body:
            problems.append(f"{rel} does not install the rust-toolchain.toml pin")
    if not PROOF_LOG.exists():
        problems.append(f"{PROOF_LOG.relative_to(ROOT)} is missing")
    else:
        log = PROOF_LOG.read_text(encoding="utf-8")
        if f"channel: {channel}" not in log:
            problems.append(f"{PROOF_LOG.relative_to(ROOT)} does not record channel {channel}")
        for command in ("cargo fmt --check", "cargo clippy --all-targets -- -D warnings"):
            if f"command: {command}\nexit: 0\n" not in log:
                problems.append(
                    f"{PROOF_LOG.relative_to(ROOT)} does not record exit 0 for `{command}`"
                )
    fmt = subprocess.run(
        ["cargo", "fmt", "--check"], cwd=ROOT, capture_output=True, text=True
    )
    if fmt.returncode != 0:
        tail = (fmt.stdout or fmt.stderr).strip().splitlines()
        detail = tail[-1] if tail else "no output"
        problems.append(f"cargo fmt --check failed: {detail}")
    return problems


def main():
    if not AVENC.exists():
        sys.exit(f"{AVENC} not found; run `cargo build --release` first")
    subcommands = set(re.findall(r"^\s+avenc ([a-z][a-z-]*)", avenc("--help").stdout, re.M))

    problems, compiled, fragments, outputs = toolchain_problems(), 0, 0, 0

    for path in doc_files():
        rel = path.relative_to(ROOT)
        text = path.read_text(encoding="utf-8")

        # Fences may be indented (inside list items); strip that indentation from the body
        blocks = [
            (m.group(2), textwrap.dedent(m.group(3)), m.start()) for m in FENCE.finditer(text)
        ]
        if path.suffix == ".html":
            blocks += [(m.group(1), html.unescape(m.group(2)).lstrip("\n"), m.start()) for m in PRE.finditer(text)]

        for i, (lang, code, start) in enumerate(blocks):
            where = f"{rel}:{line_of(text, start)}"
            if lang in ("aven", "aven-error"):
                compiled += 1
                ok, message = check_source(code)
                if lang == "aven" and not ok:
                    problems.append(f"{where}: example does not compile: {message}")
                if lang == "aven-error" and ok:
                    problems.append(f"{where}: example marked aven-error compiles cleanly")
            elif lang == "aven-fragment":
                fragments += 1
            elif lang in ("bash", "sh", "shell") and path.suffix == ".md":
                lines = [l.strip() for l in code.strip().splitlines() if l.strip()]
                run = re.fullmatch(r"(?:\./target/release/)?avenc run (\S+\.aven)", lines[0]) if len(lines) == 1 else None
                nxt = blocks[i + 1] if i + 1 < len(blocks) else None
                # Models with a `using` backend need live data, so their output is not reproducible here
                live = run and (ROOT / run.group(1)).exists() and "using " in (ROOT / run.group(1)).read_text(encoding="utf-8")
                if run and not live and (ROOT / run.group(1)).exists() and nxt and nxt[0] in ("text", "") and "Settled Environment:" in nxt[1]:
                    outputs += 1
                    documented = settled(nxt[1])
                    actual = settled(avenc("run", run.group(1)).stdout)
                    if set(documented) != set(actual):
                        problems.append(f"{where}: documented variables {sorted(documented)} differ from `avenc run` {sorted(actual)}")
                    for name, (value, unit) in documented.items():
                        if name in actual:
                            got, got_unit = actual[name]
                            if abs(got - value) > 1e-4 * max(1.0, abs(got)) or got_unit != unit:
                                problems.append(f"{where}: documents {name} = {value} {unit}, `avenc run` prints {got} {got_unit}")

        for span in CODE_SPANS.finditer(text):
            for m in COMMAND.finditer(span.group(0)):
                if m.group(1) not in subcommands:
                    at = line_of(text, span.start() + m.start())
                    problems.append(f"{rel}:{at}: `avenc {m.group(1)}` is not an avenc subcommand")

    for p in problems:
        print(p)
    print(f"\n{compiled} examples compiled, {fragments} fragments skipped, {outputs} documented outputs compared, {len(problems)} problem(s)")
    sys.exit(1 if problems else 0)


if __name__ == "__main__":
    main()
