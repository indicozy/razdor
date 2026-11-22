#!/usr/bin/env python3
"""The weekly digest: reads the server's summary (`GET /admin/api/summary.json`) and sends a short
text to Telegram with `tg-notify`.

The digest holds: players per day (the last week), this week's players by version and by platform,
retention, the top maps with their starts and where games are abandoned, the crash groups, the
frame times by device, and the bans.

Settings (environment): `RAZDOR_ADMIN_TOKEN` (the raw admin token, never in git), `RAZDOR_SERVER`
(default https://razdor.indicozy.dev/api).

Usage: python3 -I tools/stats/digest.py [--dry-run] [--file summary.json]
  --dry-run   print the digest, send nothing
  --file      read the summary from a saved JSON file instead of the server
"""

import argparse
import json
import os
import subprocess
import sys
import urllib.request

DEFAULT_SERVER = "https://razdor.indicozy.dev/api"
ADMIN_HEADER = "x-razdor-admin"
TOP = 5
MAX_MESSAGE = 60


def named_line(items, top=TOP):
    """`name value, name value` for the top rows (the rows arrive sorted by the server)."""
    rows = [f"{item['name']} {item['value']}" for item in items[:top]]
    return ", ".join(rows) if rows else "none"


def percent(part, whole):
    return f"{round(100 * part / whole)}%" if whole else "n/a"


def cut(text, width=MAX_MESSAGE):
    text = " ".join(str(text).split())
    return text if len(text) <= width else text[: width - 1] + "…"


def format_digest(s):
    """The digest text from the summary's JSON object."""
    lines = [f"Razdor weekly digest ({s.get('generated_at', 0)})", ""]

    days = s.get("players_per_day", [])[-7:]
    lines.append("Players per day: " + (", ".join(f"{d['name']} {d['value']}" for d in days) or "none"))
    lines.append("By version (week): " + named_line(s.get("week_by_version", [])))
    lines.append("By platform (week): " + named_line(s.get("week_by_platform", [])))
    lines.append("By game (week): " + named_line(s.get("week_by_game", [])))

    r = s.get("retention", {})
    lines.append(
        "Retention: week 1 {}/{} ({}), week 4 {}/{} ({})".format(
            r.get("week1_returned", 0),
            r.get("week1_cohort", 0),
            percent(r.get("week1_returned", 0), r.get("week1_cohort", 0)),
            r.get("week4_returned", 0),
            r.get("week4_cohort", 0),
            percent(r.get("week4_returned", 0), r.get("week4_cohort", 0)),
        )
    )

    lines.append("")
    lines.append("Top maps (starts, wins, losses, abandoned):")
    maps = sorted(s.get("maps", []), key=lambda m: m.get("starts", 0), reverse=True)[:TOP]
    for m in maps:
        drop = m.get("abandoned_at", [])
        where = f"; most quit at {cut(drop[0]['name'], 40)} ({drop[0]['value']})" if drop and m.get("abandoned") else ""
        lines.append(f"  {cut(m['map'], 40)}: {m['starts']}, {m['wins']}, {m['losses']}, {m['abandoned']}{where}")
    if not maps:
        lines.append("  none")

    lines.append("")
    crashes = s.get("crashes", [])[:TOP]
    lines.append(f"Crash groups: {len(s.get('crashes', []))}")
    for c in crashes:
        lines.append(f"  {c['count']}x {cut(c['location'], 40)}: {cut(c['message'])} (last {c['last_version']})")

    lines.append("")
    fps = sorted(s.get("fps", []), key=lambda f: f.get("sessions", 0), reverse=True)[:TOP]
    lines.append("Frame time by device (median, p99 ms):")
    for f in fps:
        device = " / ".join(x for x in (f.get("gpu", ""), f.get("phone", "")) if x) or "unknown"
        lines.append(f"  {cut(device, 40)}: {f['median_ms']:.1f}, {f['p99_ms']:.1f} ({f['sessions']} sessions)")
    if not fps:
        lines.append("  none")

    lines.append("")
    bans = s.get("bans", [])
    active = [b for b in bans if b.get("active")]
    lines.append(f"Bans: {len(active)} active, {len(bans)} listed")
    for b in active[:TOP]:
        lines.append(f"  {b['kind']} {cut(b['key'], 24)}: {cut(b['reason'])} ({b['created_day']})")
    return "\n".join(lines)


def fetch_summary(server, token):
    req = urllib.request.Request(server.rstrip("/") + "/admin/api/summary.json", headers={ADMIN_HEADER: token})
    with urllib.request.urlopen(req, timeout=60) as resp:
        return json.loads(resp.read())


def send(text):
    """Sends the text through `tg-notify` (silent: the digest is a report, not an alarm)."""
    subprocess.run(["tg-notify", "--silent", "--raw", text], check=True)


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--dry-run", action="store_true")
    ap.add_argument("--file")
    args = ap.parse_args(argv)
    if args.file:
        with open(args.file, encoding="utf-8") as f:
            summary = json.load(f)
    else:
        token = os.environ.get("RAZDOR_ADMIN_TOKEN", "")
        if not token:
            print("RAZDOR_ADMIN_TOKEN is not set", file=sys.stderr)
            return 2
        summary = fetch_summary(os.environ.get("RAZDOR_SERVER", DEFAULT_SERVER), token)
    text = format_digest(summary)
    if args.dry_run:
        print(text)
    else:
        send(text)
    return 0


if __name__ == "__main__":
    sys.exit(main())
