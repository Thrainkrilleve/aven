#!/usr/bin/env python3
"""
reporter_adapter.py

External evaluator for Aven.
Pulls recruit data from aa-reporter.

Environment:
    REPORTER_API_URL=https://r3v-w.space
    REPORTER_API_TOKEN=rpt_...
"""

import json
import os
import sys
import urllib.request
import urllib.parse


def load_dotenv(path=".env"):
    if not os.path.exists(path):
        return
    with open(path) as f:
        for line in f:
            line = line.strip()
            if not line or line.startswith("#") or "=" not in line:
                continue
            k, v = line.split("=", 1)
            os.environ.setdefault(k.strip(), v.strip().strip('"').strip("'"))


load_dotenv()


def api_get(path, params=None):
    base = os.environ.get("REPORTER_API_URL", "").rstrip("/")
    token = os.environ.get("REPORTER_API_TOKEN", "")
    if not base or not token:
        return {"error": "missing env", "backend_id": "aa-reporter"}

    url = f"{base}/reporter/{path.lstrip('/')}"
    if params:
        url += "?" + urllib.parse.urlencode(params)

    print(f"DEBUG URL: {url}", file=sys.stderr)

    req = urllib.request.Request(
        url,
        headers={
            "Authorization": f"Bearer {token}",
            "Accept": "application/json",
            "User-Agent": "curl/8.21.0",
        },
    )
    try:
        with urllib.request.urlopen(req, timeout=25) as resp:
            return json.loads(resp.read().decode())
    except urllib.error.HTTPError as e:
        body = e.read().decode(errors="replace")[:500]
        return {"error": f"HTTP {e.code}", "body": body, "backend_id": "aa-reporter"}
    except Exception as e:
        return {"error": str(e), "backend_id": "aa-reporter"}

def from_applicant(entry):
    main = entry.get("main") or {}
    return {
        "character_id": float(main.get("id") or 0),
        "character_name": main.get("name") or "",
        "report_high_count": float(entry.get("high") or 0),
        "report_medium_count": float(entry.get("medium") or 0),
        "report_background_count": float(entry.get("background") or 0),
        "unreviewed_findings": float(entry.get("unreviewed_findings") or 0),
        "is_applicant": 1.0 if entry.get("applicant") else 0.0,
        "characters": float(entry.get("characters") or 0),
        "audited_characters": float(entry.get("audited_characters") or 0),
        "backend_id": "aa-reporter",
        "value": float(entry.get("high") or 0),
        "jacobian": None,
        "warnings": ["live"]
    }


def main():
    data = api_get("api/v1/applicants/", {"days": 7})
    if isinstance(data, dict) and "error" in data:
        json.dump(data, sys.stdout)
        sys.stdout.write("\n")
        return

    applicants = data.get("applicants") or []
    if not applicants:
        json.dump({"error": "no applicants", "backend_id": "aa-reporter"}, sys.stdout)
    else:
        json.dump(from_applicant(applicants[0]), sys.stdout)
    sys.stdout.write("\n")


if __name__ == "__main__":
    main()