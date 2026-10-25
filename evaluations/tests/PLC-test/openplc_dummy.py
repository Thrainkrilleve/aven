#!/usr/bin/env python3
import json, time, random, math, os

cache_dir = os.path.dirname(os.path.abspath(__file__))
cache_path = os.path.join(cache_dir, "latest_plc.json")

t0 = time.time()
while True:
    t = time.time() - t0
    data = {
        "scan_ms": round(8.0 + 2*math.sin(t/5) + random.uniform(-0.2, 0.2), 2),
        "io_points": 24 + int(t) % 10,
        "power_w": round(180 + 20*math.sin(t/8), 1),
        "temp_c": round(38 + 3*math.sin(t/12), 1),
        "throughput_pph": round(120 + 15*math.sin(t/6), 1),
        "alarm_count": 1 if (int(t) % 30) < 2 else 0,
        "cpu_load_pct": round(35 + 10*math.sin(t/7), 1),
        "network_load_pct": round(12 + 5*math.sin(t/9), 1),
        "quality_pct": round(99.2 - 0.5*math.sin(t/10), 2),
        "ts": time.time()
    }
    print(json.dumps(data), flush=True)
    try:
        tmp_path = cache_path + ".tmp"
        with open(tmp_path, "w") as f:
            json.dump(data, f)
        os.replace(tmp_path, cache_path)
    except Exception:
        pass
    time.sleep(1.0)