#!/usr/bin/env python3
"""Private release download statistics for Razdor (stdlib only).

Premise: the in-game updater downloads release assets, so a release's download
count is roughly the number of players who updated to it. This is a trend, not
a headcount: manual downloads count too, a retried update can count twice,
players with updates off are missing.

Each run:
  1. Fetches the releases of indicozy/razdor (one CSV row per release and
     platform) and of indicozy/razdor-community (one row per catalog release,
     platform "all"), and appends a live snapshot (approx=0).
  2. Prints a summary: the latest finished window, the current release so far,
     the Hub catalog (with its delta since the previous snapshot in the CSV),
     and a text chart of downloads per day over the last releases.

Backfill: when the CSV does not exist yet, a one-time reconstructed history is
written before the live snapshot. For each razdor release that has a successor,
its current total per platform is stamped with the successor's published_at
(approx=1). The latest release gets no backfill. Backfill OVERESTIMATES old
releases: their counts today include downloads that came long after their
window closed (late downloads, people who updated weeks later).

Asset-to-platform mapping (razdor): names with "translate" and SHA256SUMS are
developer tools and checksums, skipped; *.exe -> windows; *.apk -> android;
*.ipa -> ios; names containing "mac" (or *.dmg) -> macos; "razdor" itself, names
containing "linux", and *.tar.gz / *.tgz / *.AppImage -> linux; anything else
-> other (listed in the output so the mapping can be checked).

Usage:
  python3 -I tools/stats/downloads.py [--csv PATH] [--no-save] [--releases N]

The CSV defaults to $RAZDOR_STATS_CSV, else /mnt/ai_models/razdor-stats/downloads.csv.
It is private (download counts of the owner's players): never commit it.
The GitHub token is read from $GH_TOKEN, else from `gh auth token -u indicozy`,
else the API is called unauthenticated (60 requests an hour).
"""

import argparse
import csv
import json
import os
import re
import subprocess
import sys
import urllib.error
import urllib.request
from datetime import datetime, timezone

RAZDOR = "indicozy/razdor"
COMMUNITY = "indicozy/razdor-community"
DEFAULT_CSV = "/mnt/ai_models/razdor-stats/downloads.csv"
COLUMNS = ["snapshot_utc", "repo", "tag", "published_at", "platform", "downloads", "approx"]
PLATFORM_ORDER = ["windows", "linux", "macos", "android", "ios", "other"]
CATALOG_RE = re.compile(r"^catalog-\d{4}-\d{2}-\d{2}$")
BAR_WIDTH = 40
TREND = ("Trend, not a headcount: manual downloads count too, a retried update can count twice, "
         "players with updates off are missing; backfilled history overestimates old releases.")


class StatsError(Exception):
    pass


def utc_now():
    return datetime.now(timezone.utc).replace(microsecond=0)


def parse_time(text):
    return datetime.strptime(text, "%Y-%m-%dT%H:%M:%SZ").replace(tzinfo=timezone.utc)


def fmt_time(dt):
    return dt.strftime("%Y-%m-%dT%H:%M:%SZ")


def days_between(later, earlier):
    return (later - earlier).total_seconds() / 86400.0


def per_day(total, days):
    return total / max(days, 1.0)


def github_token():
    token = os.environ.get("GH_TOKEN", "").strip()
    if token:
        return token
    try:
        out = subprocess.run(["gh", "auth", "token", "-u", "indicozy"],
                             capture_output=True, text=True, timeout=20)
        if out.returncode == 0 and out.stdout.strip():
            return out.stdout.strip()
    except (OSError, subprocess.SubprocessError):
        pass
    return None


def fetch_releases(repo, token):
    headers = {
        "Accept": "application/vnd.github+json",
        "X-GitHub-Api-Version": "2022-11-28",
        "User-Agent": "razdor-download-stats",
    }
    if token:
        headers["Authorization"] = f"Bearer {token}"
    items, page = [], 1
    while True:
        url = f"https://api.github.com/repos/{repo}/releases?per_page=100&page={page}"
        request = urllib.request.Request(url, headers=headers)
        try:
            with urllib.request.urlopen(request, timeout=30) as response:
                batch = json.load(response)
        except urllib.error.HTTPError as e:
            raise StatsError(f"GitHub API answered {e.code} {e.reason} for {repo} (page {page})") from e
        except (urllib.error.URLError, OSError) as e:
            raise StatsError(f"cannot reach the GitHub API for {repo}: {e}") from e
        except ValueError as e:
            raise StatsError(f"GitHub API sent a bad response for {repo}: {e}") from e
        if not isinstance(batch, list):
            raise StatsError(f"GitHub API sent an unexpected answer for {repo}")
        items.extend(batch)
        if len(batch) < 100:
            return items
        page += 1


def platform_of(name):
    """Map an asset name to a platform; None for developer tools and checksums."""
    low = name.lower()
    if "translate" in low or low == "sha256sums" or low.endswith((".sha256", ".sig", ".txt", ".md")):
        return None
    if low.endswith(".exe"):
        return "windows"
    if low.endswith(".apk"):
        return "android"
    if low.endswith(".ipa"):
        return "ios"
    if "mac" in low or low.endswith(".dmg"):
        return "macos"
    if low == "razdor" or "linux" in low or low.endswith((".tar.gz", ".tgz", ".appimage")):
        return "linux"
    return "other"


def ordered(platforms):
    return sorted(platforms.items(),
                  key=lambda kv: PLATFORM_ORDER.index(kv[0]) if kv[0] in PLATFORM_ORDER else len(PLATFORM_ORDER))


def razdor_releases(raw):
    """Published razdor releases, oldest first, with downloads per platform."""
    releases, unknown = [], set()
    for rel in raw:
        if rel.get("draft") or not rel.get("published_at"):
            continue
        platforms = {}
        for asset in rel.get("assets") or []:
            name = asset.get("name") or ""
            platform = platform_of(name)
            if platform is None:
                continue
            if platform == "other":
                unknown.add(name)
            platforms[platform] = platforms.get(platform, 0) + int(asset.get("download_count") or 0)
        releases.append({
            "tag": rel.get("tag_name") or "?",
            "published": parse_time(rel["published_at"]),
            "platforms": platforms,
            "total": sum(platforms.values()),
        })
    releases.sort(key=lambda r: r["published"])
    return releases, unknown


def catalog_releases(raw):
    """Published catalog releases, oldest first."""
    catalogs = []
    for rel in raw:
        tag = rel.get("tag_name") or ""
        if rel.get("draft") or not rel.get("published_at") or not CATALOG_RE.match(tag):
            continue
        assets = rel.get("assets") or []
        catalogs.append({
            "tag": tag,
            "published": parse_time(rel["published_at"]),
            "total": sum(int(a.get("download_count") or 0) for a in assets),
            "assets": len(assets),
        })
    catalogs.sort(key=lambda c: c["published"])
    return catalogs


def read_rows(path):
    if not os.path.exists(path):
        return []
    with open(path, newline="", encoding="utf-8") as f:
        return list(csv.DictReader(f))


def previous_catalog_snapshot(rows, tag):
    """(snapshot_utc, downloads) of the latest live snapshot of this catalog, or None."""
    found = [r for r in rows if r.get("repo") == COMMUNITY and r.get("tag") == tag and r.get("approx") == "0"]
    if not found:
        return None
    last = max(found, key=lambda r: r["snapshot_utc"])
    return last["snapshot_utc"], int(last["downloads"])


def csv_row(snapshot, repo, tag, published, platform, downloads, approx):
    return [snapshot, repo, tag, fmt_time(published), platform, str(downloads), str(approx)]


def backfill_rows(releases):
    rows = []
    for i in range(len(releases) - 1):
        rel, successor = releases[i], releases[i + 1]
        for platform, downloads in ordered(rel["platforms"]):
            rows.append(csv_row(fmt_time(successor["published"]), RAZDOR, rel["tag"],
                                rel["published"], platform, downloads, 1))
    return rows


def live_rows(snapshot, releases, catalogs):
    rows = []
    for rel in releases:
        for platform, downloads in ordered(rel["platforms"]):
            rows.append(csv_row(snapshot, RAZDOR, rel["tag"], rel["published"], platform, downloads, 0))
    for cat in catalogs:
        rows.append(csv_row(snapshot, COMMUNITY, cat["tag"], cat["published"], "all", cat["total"], 0))
    return rows


def append_rows(path, rows, write_header):
    os.makedirs(os.path.dirname(os.path.abspath(path)), exist_ok=True)
    with open(path, "a", newline="", encoding="utf-8") as f:
        writer = csv.writer(f)
        if write_header:
            writer.writerow(COLUMNS)
        writer.writerows(rows)


def signed(n):
    return f"{n:+,}" if isinstance(n, int) else f"{n:+,.1f}"


def change_pct(new, old):
    if old == 0:
        return "n/a"
    return f"{(new - old) / old * 100:+.0f}%"


def platform_line(platforms):
    if not platforms:
        return "no assets"
    return ", ".join(f"{p} {n:,}" for p, n in ordered(platforms))


def window_days(releases, i, now):
    end = releases[i + 1]["published"] if i + 1 < len(releases) else now
    return days_between(end, releases[i]["published"])


def section_window(releases, now):
    print("== Latest finished window ==")
    if len(releases) < 2:
        print("  none yet: only one razdor release")
        return
    prev, newest = releases[-2], releases[-1]
    days = window_days(releases, len(releases) - 2, now)
    pd = per_day(prev["total"], days)
    print(f"{prev['tag']}: {prev['total']:,} downloads, newest for {days:.2f} days "
          f"({fmt_time(prev['published'])} to {fmt_time(newest['published'])})")
    print(f"  per day: {pd:,.1f}")
    print(f"  by platform: {platform_line(prev['platforms'])}")
    if len(releases) >= 3:
        before = releases[-3]
        bdays = window_days(releases, len(releases) - 3, now)
        bpd = per_day(before["total"], bdays)
        diff = prev["total"] - before["total"]
        print(f"  vs {before['tag']}: downloads {prev['total']:,} vs {before['total']:,}: "
              f"{signed(diff)} ({change_pct(prev['total'], before['total'])})")
        print(f"  vs {before['tag']}: per day {pd:,.1f} vs {bpd:,.1f}: "
              f"{signed(pd - bpd)} ({change_pct(pd, bpd)})")


def section_current(releases, now):
    print("== Current release so far ==")
    if not releases:
        print("  no razdor release yet")
        return
    newest = releases[-1]
    days = max(days_between(now, newest["published"]), 0.0)
    print(f"{newest['tag']}: {newest['total']:,} downloads so far, published {fmt_time(newest['published'])}, "
          f"{days:.2f} days ago")
    print(f"  per day: {per_day(newest['total'], days):,.1f}")
    print(f"  by platform: {platform_line(newest['platforms'])}")


def section_catalog(catalogs, existing):
    print("== Hub catalog (indicozy/razdor-community) ==")
    if not catalogs:
        print("  no catalog release yet")
        return
    newest = catalogs[-1]
    print(f"{newest['tag']}: {newest['total']:,} downloads across {newest['assets']} assets")
    previous = previous_catalog_snapshot(existing, newest["tag"])
    if previous is None:
        print("  no earlier snapshot of this catalog in the CSV")
    else:
        taken, downloads = previous
        diff = newest["total"] - downloads
        print(f"  since {taken}: {signed(diff)} ({change_pct(newest['total'], downloads)})")


def section_chart(releases, now, count):
    print(f"== Downloads per day, last {min(count, len(releases))} releases (oldest -> newest) ==")
    if not releases:
        print("  no razdor release yet")
        return
    start = max(0, len(releases) - count)
    points = []
    for i in range(start, len(releases)):
        rel = releases[i]
        points.append((rel["tag"], per_day(rel["total"], window_days(releases, i, now))))
    top = max(value for _, value in points)
    for tag, value in points:
        width = round(value / top * BAR_WIDTH) if top > 0 else 0
        if value > 0 and width == 0:
            width = 1
        print(f"{tag:<10} {value:>9,.1f}  {'█' * width}")


def run(args):
    token = github_token()
    now = utc_now()
    releases, unknown = razdor_releases(fetch_releases(RAZDOR, token))
    catalogs = catalog_releases(fetch_releases(COMMUNITY, token))

    existing = read_rows(args.csv)
    snapshot = fmt_time(now)
    rows = live_rows(snapshot, releases, catalogs)
    if not args.no_save:
        existed = os.path.exists(args.csv)
        header = not existed or os.path.getsize(args.csv) == 0
        backfill = backfill_rows(releases) if not existed else []
        append_rows(args.csv, backfill + rows, header)
        print(f"Saved {len(rows)} live rows at {snapshot}"
              + (f" and {len(backfill)} backfill rows (first run)" if backfill else "")
              + f" to {args.csv}")
        print()

    if unknown:
        print("Note: asset names counted as 'other': " + ", ".join(sorted(unknown)))
        print()
    section_window(releases, now)
    print()
    section_current(releases, now)
    print()
    section_catalog(catalogs, existing)
    print()
    section_chart(releases, now, args.releases)
    print()
    print(TREND)
    return 0


def main(argv=None):
    parser = argparse.ArgumentParser(description="Razdor release download statistics (private).")
    parser.add_argument("--csv", default=os.environ.get("RAZDOR_STATS_CSV") or DEFAULT_CSV,
                        help="CSV to append to (default: $RAZDOR_STATS_CSV or %(default)s)")
    parser.add_argument("--no-save", action="store_true", help="print the summary only, write nothing")
    parser.add_argument("--releases", type=int, default=12, help="releases in the chart (default 12)")
    args = parser.parse_args(argv)
    if args.releases < 1:
        parser.error("--releases must be at least 1")
    try:
        return run(args)
    except StatsError as e:
        print(f"error: {e}", file=sys.stderr)
        print("Nothing was written to the CSV.", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
