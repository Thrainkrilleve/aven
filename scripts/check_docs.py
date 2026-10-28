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
  * README.md, STANDARDS.md, and web/public/ do not say Merkle. The ledger is a
    SHA-256 hash chain.
  * A key figure on the homepage names a committed log in data-log and a number in
    data-figure. The visible value is that number, and the log contains it.
  * README.md, web/public/, and examples/*.aven do not mention avenc.dev/weather
    or web/src/weather/. That page and its worker were removed.
  * The RELATION template in web/public/solver-demo.js has the same assignments,
    in the same order, as the relation block in examples/server_capacity.aven.
  * web/public/sitemap.xml lists https://avenc.dev/ for index.html and
    https://avenc.dev/<file> for every other HTML page in web/public/ except
    404.html. web/public/robots.txt references that sitemap and declares
    Content-Signal ai-train=no, search=yes, and ai-input=no.
  * SECURITY.md names the root Cargo.toml version as the supported release
    and points vulnerability reports at the GitHub advisory form.

Exits non-zero when anything does not match.
"""

import html
import os
import re
import subprocess
import sys
import tempfile
import textwrap
import xml.etree.ElementTree as ET
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
    return subprocess.run([str(AVENC), *args], capture_output=True, text=True, encoding="utf-8", errors="replace", cwd=cwd, timeout=120)


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
        ["cargo", "fmt", "--check"], cwd=ROOT, capture_output=True, text=True, encoding="utf-8", errors="replace"
    )
    if fmt.returncode != 0:
        tail = (fmt.stdout or fmt.stderr).strip().splitlines()
        detail = tail[-1] if tail else "no output"
        problems.append(f"cargo fmt --check failed: {detail}")
    return problems


def merkle_problems():
    """The public ledger claim is a hash chain. Merkle is a different structure."""
    paths = [ROOT / "README.md", ROOT / "STANDARDS.md"]
    paths.extend(sorted((ROOT / "web" / "public").glob("*.html")))
    problems = []
    for path in paths:
        if not path.is_file():
            problems.append(f"{path.relative_to(ROOT)} is missing")
            continue
        if re.search(r"merkle", path.read_text(encoding="utf-8"), re.I):
            problems.append(
                f"{path.relative_to(ROOT)} says Merkle; the ledger is a SHA-256 hash chain"
            )
    return problems


RETIRED_WEATHER_NEEDLES = ("avenc.dev/weather", "web/src/weather/")
TEXT_SUFFIXES = {".md", ".html", ".js", ".css", ".svg", ".txt", ".sh", ".ps1", ".json", ".aven"}


def normalized_assignments(body):
    """Assignment lines in source order, with comments and spacing removed."""
    found = []
    for raw in body.splitlines():
        code = raw.split("//", 1)[0].strip()
        match = re.fullmatch(r"([A-Za-z_]\w*)\s*=\s*(.+)", code)
        if not match:
            continue
        rhs = re.sub(r"\s+", "", match.group(2))
        found.append(f"{match.group(1)}={rhs}")
    return found


def relation_assignments(source):
    """Assignments inside the first `relation` block, or None when there is no block."""
    match = re.search(r"relation\s+\w+\s*\{(.*?)\n\}", source, re.S)
    if not match:
        return None
    return normalized_assignments(match.group(1))


def retired_weather_site_problems():
    """The live radar page and its worker are gone. These paths must not be claimed."""
    paths = [ROOT / "README.md"]
    public = ROOT / "web" / "public"
    if public.is_dir():
        paths.extend(
            path
            for path in sorted(public.rglob("*"))
            if path.is_file() and path.suffix.lower() in TEXT_SUFFIXES
        )
    paths.extend(sorted((ROOT / "examples").glob("*.aven")))
    problems = []
    seen = set()
    for path in paths:
        if path in seen or not path.is_file():
            continue
        seen.add(path)
        text = path.read_text(encoding="utf-8")
        for needle in RETIRED_WEATHER_NEEDLES:
            if needle in text:
                problems.append(f"{path.relative_to(ROOT)} mentions {needle}")
    return problems


def demo_relation_problems():
    """The browser demo's copied relation must match examples/server_capacity.aven."""
    aven_path = ROOT / "examples" / "server_capacity.aven"
    js_path = ROOT / "web" / "public" / "solver-demo.js"
    if not aven_path.is_file() or not js_path.is_file():
        return ["solver demo relation gate is missing examples/server_capacity.aven or web/public/solver-demo.js"]
    js = js_path.read_text(encoding="utf-8")
    template = re.search(r"const RELATION = `([\s\S]*?)`;", js)
    if template is None:
        return ["web/public/solver-demo.js has no RELATION template"]
    model = relation_assignments(aven_path.read_text(encoding="utf-8"))
    demo = relation_assignments(template.group(1))
    if model is None:
        return ["examples/server_capacity.aven has no relation block"]
    if demo is None:
        return ["web/public/solver-demo.js RELATION template has no relation block"]
    if model != demo:
        return [
            "web/public/solver-demo.js RELATION assignments differ from "
            f"examples/server_capacity.aven: {demo} vs {model}"
        ]
    return []


def homepage_figure_problems():
    """Every homepage timing figure is a number copied from a committed log."""
    page = ROOT / "web" / "public" / "index.html"
    text = page.read_text(encoding="utf-8")
    section = re.search(r'aria-label="Key figures"(.*?)</section>', text, re.S)
    if not section:
        return ["web/public/index.html has no key-figures section"]
    problems = []
    for part in re.split(r'<div class="proof-item"', section.group(1))[1:]:
        attrs, _, rest = part.partition(">")
        visible_m = re.search(r'class="proof-val">([^<]*)', rest)
        visible = (visible_m.group(1) if visible_m else "").strip().replace(",", "")
        figure = re.search(r'data-figure="([^"]*)"', attrs)
        log_attr = re.search(r'data-log="([^"]*)"', attrs)
        if visible in {"0", "4"} and figure is None:
            continue
        if figure is None or log_attr is None:
            problems.append(
                f"web/public/index.html figure {visible!r} is not tied to a committed log"
            )
            continue
        number = figure.group(1).replace(",", "")
        if number not in visible:
            problems.append(
                f"web/public/index.html shows {visible!r} but data-figure is {number!r}"
            )
        log_path = ROOT / log_attr.group(1)
        if not log_path.is_file():
            problems.append(f"web/public/index.html names missing log {log_attr.group(1)}")
            continue
        if number not in log_path.read_text(encoding="utf-8"):
            problems.append(
                f"web/public/index.html states {number}, which {log_attr.group(1)} does not contain"
            )
    return problems


SITEMAP_NS = {"sm": "http://www.sitemaps.org/schemas/sitemap/0.9"}
SITEMAP_SKIP = {"404.html"}
CONTENT_SIGNAL = "Content-Signal: ai-train=no, search=yes, ai-input=no"
SITEMAP_LINE = "Sitemap: https://avenc.dev/sitemap.xml"


def public_page_locs():
    """Canonical URLs for HTML pages a crawler should fetch. 404.html is the error page."""
    public = ROOT / "web" / "public"
    names = sorted(path.name for path in public.glob("*.html") if path.name not in SITEMAP_SKIP)
    locs = []
    if "index.html" in names:
        locs.append("https://avenc.dev/")
    locs.extend(f"https://avenc.dev/{name}" for name in names if name != "index.html")
    return locs


def site_map_problems():
    """The sitemap and robots.txt must name the same public pages the site serves."""
    problems = []
    sitemap_path = ROOT / "web" / "public" / "sitemap.xml"
    robots_path = ROOT / "web" / "public" / "robots.txt"
    expected = public_page_locs()
    try:
        root = ET.parse(sitemap_path).getroot()
    except (ET.ParseError, OSError) as exc:
        return [f"web/public/sitemap.xml is not valid XML: {exc}"]
    locs = [
        (element.text or "").strip()
        for element in root.findall("sm:url/sm:loc", SITEMAP_NS)
    ]
    if locs != expected:
        problems.append(
            f"web/public/sitemap.xml locs {locs} differ from public pages {expected}"
        )
    robots = robots_path.read_text(encoding="utf-8") if robots_path.is_file() else ""
    if CONTENT_SIGNAL not in robots:
        problems.append(f"web/public/robots.txt is missing {CONTENT_SIGNAL!r}")
    if SITEMAP_LINE not in robots:
        problems.append(f"web/public/robots.txt is missing {SITEMAP_LINE!r}")
    return problems


ADVISORY_URL = "https://github.com/Thrainkrilleve/aven/security/advisories/new"


def package_version():
    """The [package] version in the root Cargo.toml."""
    text = (ROOT / "Cargo.toml").read_text(encoding="utf-8")
    package = re.search(r"(?ms)^\[package\](.*?)(?:^\[|\Z)", text)
    body = package.group(1) if package else ""
    match = re.search(r'(?m)^version = "([^"]+)"', body)
    return match.group(1) if match else None


def security_policy_problems():
    """SECURITY.md must name the compiler version Cargo.toml actually ships."""
    problems = []
    version = package_version()
    policy_path = ROOT / "SECURITY.md"
    policy = policy_path.read_text(encoding="utf-8") if policy_path.is_file() else ""
    if version is None:
        return ["Cargo.toml [package] has no version"]
    supported = rf"^\| {re.escape(version)} +\| :white_check_mark: +\|$"
    unsupported = rf"^\| < {re.escape(version)} +\| :x: +\|$"
    sentence = f"That version is {version}."
    if sentence not in policy:
        problems.append(f"SECURITY.md is missing the sentence {sentence!r}")
    if not re.search(supported, policy, re.M):
        problems.append(f"SECURITY.md is missing the supported row for Cargo.toml version {version}")
    if not re.search(unsupported, policy, re.M):
        problems.append(f"SECURITY.md is missing the unsupported row for versions before {version}")
    if ADVISORY_URL not in policy:
        problems.append(f"SECURITY.md is missing {ADVISORY_URL}")
    return problems


def main():
    if not AVENC.exists():
        sys.exit(f"{AVENC} not found; run `cargo build --release` first")
    subcommands = set(re.findall(r"^\s+avenc ([a-z][a-z-]*)", avenc("--help").stdout, re.M))

    problems = toolchain_problems()
    problems.extend(merkle_problems())
    problems.extend(homepage_figure_problems())
    problems.extend(retired_weather_site_problems())
    problems.extend(demo_relation_problems())
    problems.extend(site_map_problems())
    problems.extend(security_policy_problems())
    compiled, fragments, outputs = 0, 0, 0

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
