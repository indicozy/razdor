#!/usr/bin/env python3
"""Replays the pending battles with `razdor-verify` and posts the verdicts to the server.

The owner runs it nightly on the PC that holds the DT install (`RAZDOR_DT_DIR` in `.env`). It pulls
the battles with no verdict (`GET /v1/battles/pending`), fetches each one (`GET /v1/battles/{id}`:
the metadata, then the record, framed), runs `razdor-verify` on it and posts the verdict
(`POST /v1/verdicts`). Only `ok` battles go into training data.

The server's verdict kinds are `ok` and `suspicious`. A battle the verifier cannot replay
(`unverified`: a mod's battle, «Максимум» on the enemy side) is posted as `suspicious` with a reason
that starts with `unverified:`, so it leaves the pending queue and never reaches training data.

Settings (environment): `RAZDOR_ADMIN_TOKEN` (the raw admin token, never in git), `RAZDOR_SERVER`
(default https://razdor.indicozy.dev/api), `RAZDOR_VERIFY` (the verifier binary, default
`razdor-verify` on PATH), `RAZDOR_VERIFY_SCRATCH` (where the fetched records go, default a temp
folder that is removed at the end).

Usage: python3 -I tools/stats/verify_battles.py [--dry-run] [--limit N]
"""

import argparse
import json
import os
import shutil
import subprocess
import sys
import tempfile
import urllib.error
import urllib.request

DEFAULT_SERVER = "https://razdor.indicozy.dev/api"
ADMIN_HEADER = "x-razdor-admin"
SCHEMA = 1


def verdict_body(battle_id, verdict):
    """The POST body for the verifier's JSON line (`{"verdict", "reason"}`)."""
    kind = verdict.get("verdict")
    reason = verdict.get("reason") or ""
    if kind == "ok":
        return {"schema": SCHEMA, "battle_id": battle_id, "kind": "ok"}
    if kind == "suspicious":
        return {"schema": SCHEMA, "battle_id": battle_id, "kind": "suspicious", "reason": reason or "no reason given"}
    if kind == "unverified":
        return {"schema": SCHEMA, "battle_id": battle_id, "kind": "suspicious", "reason": "unverified: " + reason}
    return None


def parse_verdict(stdout):
    """The verifier's last JSON line, or None when it printed no verdict."""
    for line in reversed(stdout.strip().splitlines()):
        line = line.strip()
        if not line.startswith("{"):
            continue
        try:
            value = json.loads(line)
        except json.JSONDecodeError:
            continue
        if isinstance(value, dict) and "verdict" in value:
            return value
    return None


def run_verifier(binary, record_path):
    """Runs `razdor-verify` on one fetched battle. Returns its verdict dict, or raises on no verdict."""
    done = subprocess.run([binary, record_path], capture_output=True, text=True, check=False)
    verdict = parse_verdict(done.stdout)
    if verdict is None:
        raise RuntimeError(f"no verdict (exit {done.returncode}): {done.stderr.strip()[-300:]}")
    return verdict


def request(server, token, method, path, body=None):
    """One call to the server. Returns the response bytes."""
    data = None
    headers = {ADMIN_HEADER: token}
    if body is not None:
        data = json.dumps(body).encode("utf-8")
        headers["Content-Type"] = "application/json"
    req = urllib.request.Request(server.rstrip("/") + path, data=data, method=method, headers=headers)
    with urllib.request.urlopen(req, timeout=60) as resp:
        return resp.read()


def pending(server, token, limit):
    rows = json.loads(request(server, token, "GET", "/v1/battles/pending"))
    return [row["id"] for row in rows[:limit]]


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--dry-run", action="store_true", help="verify and print, post nothing")
    ap.add_argument("--limit", type=int, default=200, help="at most this many battles")
    args = ap.parse_args(argv)

    token = os.environ.get("RAZDOR_ADMIN_TOKEN", "")
    if not token:
        print("RAZDOR_ADMIN_TOKEN is not set", file=sys.stderr)
        return 2
    server = os.environ.get("RAZDOR_SERVER", DEFAULT_SERVER)
    binary = os.environ.get("RAZDOR_VERIFY", "razdor-verify")
    scratch = os.environ.get("RAZDOR_VERIFY_SCRATCH") or tempfile.mkdtemp(prefix="razdor-verify-")
    os.makedirs(scratch, exist_ok=True)
    counts = {"ok": 0, "suspicious": 0, "unverified": 0, "failed": 0}
    try:
        for battle_id in pending(server, token, args.limit):
            path = os.path.join(scratch, battle_id + ".bin")
            try:
                with open(path, "wb") as f:
                    f.write(request(server, token, "GET", "/v1/battles/" + battle_id))
                verdict = run_verifier(binary, path)
            except (OSError, RuntimeError, urllib.error.URLError) as e:
                counts["failed"] += 1
                print(f"{battle_id}: not verified: {e}", file=sys.stderr)
                continue
            finally:
                if os.path.exists(path):
                    os.remove(path)
            counts[verdict.get("verdict", "failed")] = counts.get(verdict.get("verdict", "failed"), 0) + 1
            body = verdict_body(battle_id, verdict)
            if body is None:
                counts["failed"] += 1
                continue
            if args.dry_run:
                print(json.dumps(body, ensure_ascii=False))
            else:
                request(server, token, "POST", "/v1/verdicts", body)
    finally:
        if not os.environ.get("RAZDOR_VERIFY_SCRATCH"):
            shutil.rmtree(scratch, ignore_errors=True)
    print("verdicts: " + ", ".join(f"{k} {v}" for k, v in counts.items()))
    return 0


if __name__ == "__main__":
    sys.exit(main())
