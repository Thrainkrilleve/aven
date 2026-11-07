#!/usr/bin/env python3
"""Generate an N-stage Aven route model that matches a running mesh.

Two modes:
  --stages N              distribute 94 layers across N stages (synthetic)
  --from-url <base>       read <base>/admin/status and build a model whose stages and
                          layer counts match the live route (optionally --key for auth)

The model mirrors evaluations/possible/isharded/isharded_route.aven, generalized to N
stages, and carries the same provisional monitoring thresholds so `avenc stream` alarms.
Dependency-free (stdlib only). Aven solves a 500-stage model in <0.1s, so N is not a limit.
"""
import argparse
import json
import sys
import urllib.request

BW_CYCLE = [336, 896, 672, 696, 696, 696, 800]


def route_from_url(base, key):
    url = base.rstrip("/") + "/admin/status"
    req = urllib.request.Request(url)
    if key:
        req.add_header("Authorization", f"Bearer {key}")
    with urllib.request.urlopen(req, timeout=10) as r:
        status = json.loads(r.read().decode())
    route = status.get("route") or []
    layers = [int(s.get("layers", 0)) for s in route]
    return layers if layers else None


def distribute(total, n):
    base, rem = divmod(total, n)
    return [base + (1 if i < rem else 0) for i in range(n)]


def emit(layers):
    n = len(layers)
    bws = [BW_CYCLE[i % len(BW_CYCLE)] for i in range(n)]
    total_layers = sum(layers) or 94
    L = [f"// generated {n}-stage route model (mesh-sim)"]
    L += ["unit tokens : token",
          "signal weights_total : GiB", "signal active_params : scalar",
          "signal total_params : scalar", "signal model_layers : scalar",
          "signal active_fraction : ratio", "signal active_per_layer : GiB",
          "signal efficiency : ratio"]
    for i in range(1, n + 1):
        L += [f"signal s{i}_layers : scalar", f"signal s{i}_bw : GB/s", f"signal s{i}_ms : ms"]
    L += ["signal layers_placed : scalar", "signal compute_ms : ms", "signal hops : scalar",
          "signal avg_hop_ms : ms", "signal network_ms : ms", "signal token_ms : ms",
          "signal per_token : token", "signal observed_rate : token/s",
          "signal planned_rate : token/s",
          "signal min_rate : token/s", "signal max_hop_ms : ms"]
    L.append("")
    L.append("relation RouteTrip {")
    L.append("    active_fraction = active_params / total_params")
    L.append("    active_per_layer = weights_total * active_fraction / model_layers")
    for i in range(1, n + 1):
        L.append(f"    s{i}_ms = s{i}_layers * active_per_layer / (s{i}_bw * efficiency)")
    L.append("    layers_placed = " + " + ".join(f"s{i}_layers" for i in range(1, n + 1)))
    L.append("    compute_ms = " + " + ".join(f"s{i}_ms" for i in range(1, n + 1)))
    L.append("    network_ms = hops * avg_hop_ms")
    L.append("    token_ms = compute_ms + network_ms")
    L.append("    observed_rate = per_token / token_ms")
    L.append("}")
    L.append("")
    L.append("solve RouteTrip with {")
    L += [f"    weights_total = 172.52 GiB", "    active_params = 22",
          "    total_params = 235", f"    model_layers = {total_layers}"]
    for i in range(1, n + 1):
        L.append(f"    s{i}_layers = {layers[i-1]}")
        L.append(f"    s{i}_bw = {bws[i-1]} GB/s")
    L += ["    efficiency = 0.6", f"    hops = {n - 1}", "    per_token = 1 token",
          "    observed_rate = 1.01 token/s", "    planned_rate = 4.9 token/s",
          "    min_rate = 0.8 token/s", "    max_hop_ms = 200 ms",
          f"    layers_placed == {total_layers}", "    efficiency <= 1",
          "    observed_rate >= min_rate", "    avg_hop_ms <= max_hop_ms"]
    L.append("}")
    return "\n".join(L) + "\n"


def main():
    p = argparse.ArgumentParser(description="Generate an N-stage Aven route model.")
    g = p.add_mutually_exclusive_group(required=True)
    g.add_argument("--stages", type=int, help="synthetic: distribute 94 layers across N stages")
    g.add_argument("--from-url", help="read <base>/admin/status and match the live route")
    p.add_argument("--key", default="", help="bearer key for --from-url")
    p.add_argument("-o", "--out", help="output file (default stdout)")
    a = p.parse_args()

    if a.from_url:
        layers = route_from_url(a.from_url, a.key)
        if not layers:
            print("error: no route in /admin/status", file=sys.stderr)
            sys.exit(2)
    else:
        layers = distribute(94, a.stages)

    text = emit(layers)
    if a.out:
        with open(a.out, "w", encoding="utf-8") as fh:
            fh.write(text)
        print(f"wrote {len(layers)}-stage model to {a.out}", file=sys.stderr)
    else:
        sys.stdout.write(text)


if __name__ == "__main__":
    main()
