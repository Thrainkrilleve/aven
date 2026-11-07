#!/usr/bin/env python3
"""isharded_poller.py - feed live iSharded readings into the Aven route model.

A small, dependency-free poller (Python standard library only). It reads a
read-only *user* key from the environment and polls one mesh node's
`/admin/status`, then emits either:

  * JSON lines for `avenc stream` (option B, a live monitor), or
  * a `name,value,unit` CSV snapshot for the IDE's "import sheet" (option A).

It feeds only what a user key exposes today: measured tokens/s, the planner's
estimate, and - when the live route still has 7 stages - each stage's layer
count. The model then solves `avg_hop_ms` (the round trip per link) for each
reading. See LIVE_DATA_PLAN.md in this folder.

Security: the key comes from $ISHARDED_KEY and is sent only as a request
header, never in the URL, the CSV, or this file. Point --url at a node you can
reach (same LAN, Tailscale, or the mesh's TLS endpoint).

Two things to confirm against your iSharded build, both adjustable without
editing code:
  * the auth header (default "Authorization: Bearer <key>"; use --key-header /
    --key-scheme, e.g. --key-header X-API-Key --key-scheme "" if it differs);
  * the JSON field names - run `--raw` once to print /admin/status and check
    the paths this script looks for (listed in extract() below).

Examples:
  # Live monitor into the stream supervisor (option B):
  ISHARDED_URL=https://node ISHARDED_KEY=... \
    python isharded_poller.py --interval 15 \
      | avenc stream evaluations/possible/isharded/isharded_route.aven

  # One CSV snapshot for the IDE (option A):
  ISHARDED_URL=https://node ISHARDED_KEY=... \
    python isharded_poller.py --out csv --csv live.csv

  # Inspect the raw status shape, or test offline with a saved response:
  python isharded_poller.py --raw
  python isharded_poller.py --from-file sample_status.json --interval 0
"""

import argparse
import json
import os
import ssl
import sys
import time
import urllib.error
import urllib.request

# The model's seven fixed stage slots, entry -> exit.
STAGE_KEYS = [f"s{i}_layers" for i in range(1, 8)]

# Declared units for the signals this poller can set (for CSV output).
UNITS = {"observed_rate": "token/s", "planned_rate": "token/s"}
for _k in STAGE_KEYS:
    UNITS[_k] = "scalar"


def log(*args):
    """Diagnostics go to stderr so stdout stays pure JSON lines / CSV."""
    print(*args, file=sys.stderr, flush=True)


def dig(obj, *paths):
    """Return the first present value among dotted paths, else None."""
    for path in paths:
        cur = obj
        ok = True
        for part in path.split("."):
            if isinstance(cur, dict) and part in cur:
                cur = cur[part]
            else:
                ok = False
                break
        if ok:
            return cur
    return None


def fetch_status(args):
    """GET /admin/status, returning the decoded JSON object."""
    endpoint = args.url.rstrip("/") + "/admin/status"
    req = urllib.request.Request(endpoint, method="GET")
    req.add_header("Accept", "application/json")
    if args.key:
        header_value = f"{args.key_scheme} {args.key}".strip() if args.key_scheme else args.key
        req.add_header(args.key_header, header_value)
    ctx = None
    if args.insecure and endpoint.lower().startswith("https"):
        ctx = ssl.create_default_context()
        ctx.check_hostname = False
        ctx.verify_mode = ssl.CERT_NONE
    with urllib.request.urlopen(req, timeout=args.timeout, context=ctx) as resp:
        return json.loads(resp.read().decode("utf-8"))


def load_status(args):
    """Fetch from the mesh, or read a saved response with --from-file."""
    if args.from_file:
        with open(args.from_file, "r", encoding="utf-8") as fh:
            return json.load(fh)
    if not args.url:
        log("Error: no mesh URL. Set $ISHARDED_URL or pass --url (or use --from-file).")
        sys.exit(2)
    return fetch_status(args)


def extract(status):
    """Map /admin/status to {signal: number} the model understands.

    Field paths are tolerant (first match wins). Run with --raw to confirm them
    against your build. Returns (readings, notes)."""
    readings = {}
    notes = []

    tps = dig(
        status,
        "metrics.tokens_per_second_ema",
        "metrics.tokens_per_second",
        "tokens_per_second_ema",
    )
    if tps is not None:
        readings["observed_rate"] = float(tps)
    else:
        notes.append("no measured tokens/s found (looked in metrics.tokens_per_second_ema)")

    planned = dig(
        status,
        "active_plan.estimated_tokens_per_second",
        "plan.estimated_tokens_per_second",
        "active_plan.estimated_tps",
    )
    if planned is not None:
        readings["planned_rate"] = float(planned)

    route = dig(status, "route", "active_plan.route")
    if isinstance(route, list):
        if len(route) == len(STAGE_KEYS):
            layers = []
            for stage in route:
                n = dig(stage, "layers", "n_layers", "layer_count")
                if isinstance(n, list):
                    n = len(n)
                layers.append(n)
            if all(isinstance(n, (int, float)) for n in layers):
                for key, n in zip(STAGE_KEYS, layers):
                    readings[key] = float(n)
            else:
                notes.append("route present but layer counts not numeric; layers not refreshed")
        else:
            notes.append(
                f"route has {len(route)} stages, model has {len(STAGE_KEYS)} slots; "
                "layers not refreshed (see the route-shape note in LIVE_DATA_PLAN.md)"
            )

    return readings, notes


def write_csv(readings, path):
    lines = ["name,value,unit"]
    for name in sorted(readings):
        lines.append(f"{name},{readings[name]:g},{UNITS.get(name, '')}")
    text = "\n".join(lines) + "\n"
    if path in ("-", "", None):
        sys.stdout.write(text)
        sys.stdout.flush()
    else:
        with open(path, "w", encoding="utf-8") as fh:
            fh.write(text)
        log(f"Wrote {len(readings)} value(s) to {path}")


def main():
    p = argparse.ArgumentParser(
        description="Poll an iSharded mesh node and feed the Aven route model.",
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    p.add_argument("--url", default=os.environ.get("ISHARDED_URL", ""),
                   help="mesh node base URL (or env ISHARDED_URL)")
    p.add_argument("--out", choices=["stream", "csv"], default="stream",
                   help="stream = JSON lines for `avenc stream` (default); csv = snapshot")
    p.add_argument("--csv", default="-", metavar="PATH",
                   help="with --out csv: file to write (default stdout)")
    p.add_argument("--interval", type=float, default=15.0, metavar="SECONDS",
                   help="with --out stream: seconds between polls (0 = one reading, then exit)")
    p.add_argument("--from-file", metavar="PATH",
                   help="read a saved /admin/status JSON instead of HTTP (offline testing)")
    p.add_argument("--raw", action="store_true",
                   help="fetch /admin/status, pretty-print it, and exit")
    p.add_argument("--timeout", type=float, default=10.0, metavar="SECONDS",
                   help="HTTP timeout (default 10)")
    p.add_argument("--insecure", action="store_true",
                   help="skip TLS verification (self-signed mesh endpoints)")
    p.add_argument("--key-header", default="Authorization",
                   help="header carrying the key (default Authorization)")
    p.add_argument("--key-scheme", default="Bearer",
                   help='scheme prefix for the key (default "Bearer"; use "" for none)')
    args = p.parse_args()
    args.key = os.environ.get("ISHARDED_KEY", "")

    if args.raw:
        status = load_status(args)
        print(json.dumps(status, indent=2, sort_keys=True))
        return

    if not args.key and not args.from_file:
        log("Warning: $ISHARDED_KEY is empty; /admin/status needs a read-only user key.")

    def one_reading():
        status = load_status(args)
        readings, notes = extract(status)
        for note in notes:
            log(f"note: {note}")
        if not readings:
            log("Warning: no usable readings extracted; try --raw to inspect the shape.")
        return readings

    if args.out == "csv":
        write_csv(one_reading(), args.csv)
        return

    # stream mode: JSON lines on stdout
    while True:
        try:
            readings = one_reading()
            if readings:
                sys.stdout.write(json.dumps(readings) + "\n")
                sys.stdout.flush()
        except urllib.error.HTTPError as e:
            log(f"HTTP {e.code} from the mesh: {e.reason} (check the key and --key-header)")
        except urllib.error.URLError as e:
            log(f"Cannot reach the mesh: {e.reason} (check --url and connectivity)")
        except Exception as e:  # noqa: BLE001 - keep the monitor alive across transient errors
            log(f"Skipped a reading: {e}")
        if args.interval <= 0:
            break
        time.sleep(args.interval)


if __name__ == "__main__":
    main()
