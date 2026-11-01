#!/usr/bin/env python3
"""Private release download statistics for Razdor (stdlib only).

Premise: the in-game updater downloads release assets, so a release's download
count is roughly the number of players who updated to it. This is a trend, not
a headcount: manual downloads count too, a retried update can count twice,
players with updates off are missing.

Each run:
  1. Fetches the releases of indicozy/razdor (one CSV row per release and
     platform, plus a "translate" row with the bot floor and a "sums" row with
     the SHA256SUMS downloads) and of indicozy/razdor-community (one row per
     catalog release, platform "all"), and appends a live snapshot (approx=0).
  2. Prints a summary: the latest finished window, the current release so far,
     the Hub catalog (with its delta since the previous snapshot in the CSV),
     a weekly view (downloads of the releases published each ISO week, with the
     week-over-week change), a daily view of the last 14 days, and a text chart
     of downloads per day over the last releases. The weekly and daily views
     are the trend to read: per-release numbers are noisy while releases were
     more frequent than daily.

Bot floor: every release also ships developer tools (razdor-translate,
razdor-translate-macos, Razdor-translate.exe) that players never download; bots
(mirrors, scanners, crawlers) fetch every asset about once per release. So
floor = min(download_count) over the translate assets of the release, and the
"after bots" count of a player platform = max(0, raw - floor). A release with no
translate assets (the early releases) uses an assumed floor of 1, the level seen
on every release that has them. Approximate: it assumes bots hit every file
equally, once per release.

Updater count: the in-game updater downloads SHA256SUMS to verify, manual
downloads almost never do, so updater = max(0, sums - floor).

Backfill: when the CSV does not exist yet, a one-time reconstructed history is
written before the live snapshot. For each razdor release that has a successor,
its current total per platform is stamped with the successor's published_at
(approx=1). The latest release gets no backfill. Backfill OVERESTIMATES old
releases: their counts today include downloads that came long after their
window closed (late downloads, people who updated weeks later).

Asset-to-platform mapping (razdor): names with "translate" are the bot-floor
tools and SHA256SUMS is the checksum file (both counted separately, see above);
*.sha256, *.sig, *.txt and *.md are skipped; *.exe -> windows; *.apk -> android;
*.ipa -> ios; names containing "mac" (or *.dmg) -> macos; "razdor" itself, names
containing "linux", and *.tar.gz / *.tgz / *.AppImage -> linux; anything else
-> other (listed in the output so the mapping can be checked).

Usage:
  python3 -I tools/stats/downloads.py [--csv PATH] [--no-save] [--releases N] [--weeks N]

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
from datetime import datetime, timedelta, timezone

RAZDOR = "indicozy/razdor"
COMMUNITY = "indicozy/razdor-community"
DEFAULT_CSV = "/mnt/ai_models/razdor-stats/downloads.csv"
COLUMNS = ["snapshot_utc", "repo", "tag", "published_at", "platform", "downloads", "approx"]
PLATFORM_ORDER = ["windows", "linux", "macos", "android", "ios", "other"]
COMPACT_ORDER = ["windows", "linux", "macos", "android", "ios"]
CATALOG_RE = re.compile(r"^catalog-\d{4}-\d{2}-\d{2}$")
BAR_WIDTH = 40
BOT_FLOOR_ASSUMED = 1
TREND = ("Trend, not a headcount: manual downloads count too, a retried update can count twice, "
         "players with updates off are missing; backfilled history overestimates old releases. "
         "'After bots' subtracts the bot floor per platform (approximate: assumes bots fetch every "
         "file equally, once per release).")
UPDATER_NOTE = ("updater = SHA256SUMS downloads minus bots: the in-game updater downloads SHA256SUMS "
                "to verify, manual downloads almost never do.")


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
    """Map an asset name to a player platform; None for developer tools, checksums and other non-player files."""
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
    """Published razdor releases, oldest first, with raw and after-bots downloads per platform."""
    releases, unknown = [], set()
    for rel in raw:
        if rel.get("draft") or not rel.get("published_at"):
            continue
        platforms, translate_counts, sums = {}, [], 0
        for asset in rel.get("assets") or []:
            name = asset.get("name") or ""
            low = name.lower()
            count = int(asset.get("download_count") or 0)
            if "translate" in low:
                translate_counts.append(count)
                continue
            if low == "sha256sums":
                sums = count
                continue
            platform = platform_of(name)
            if platform is None:
                continue
            if platform == "other":
                unknown.add(name)
            platforms[platform] = platforms.get(platform, 0) + count
        if translate_counts:
            floor, assumed = min(translate_counts), False
        else:
            floor, assumed = BOT_FLOOR_ASSUMED, True
        clean = {p: max(0, n - floor) for p, n in platforms.items()}
        releases.append({
            "tag": rel.get("tag_name") or "?",
            "published": parse_time(rel["published_at"]),
            "platforms": platforms,
            "total": sum(platforms.values()),
            "clean": clean,
            "clean_total": sum(clean.values()),
            "floor": floor,
            "floor_assumed": assumed,
            "sums": sums,
            "updater": max(0, sums - floor),
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
        rows.append(csv_row(snapshot, RAZDOR, rel["tag"], rel["published"], "translate", rel["floor"], 0))
        rows.append(csv_row(snapshot, RAZDOR, rel["tag"], rel["published"], "sums", rel["sums"], 0))
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


def after_bots(rel):
    return f"{rel['clean_total']:,} after bots (raw {rel['total']:,})"


def floor_text(rel):
    return f"{rel['floor']:,}" + (" assumed" if rel["floor_assumed"] else "")


def section_window(releases, now):
    print("== Latest finished window ==")
    if len(releases) < 2:
        print("  none yet: only one razdor release")
        return
    prev, newest = releases[-2], releases[-1]
    days = window_days(releases, len(releases) - 2, now)
    pd = per_day(prev["clean_total"], days)
    print(f"{prev['tag']}: {after_bots(prev)}, newest for {days:.2f} days "
          f"({fmt_time(prev['published'])} to {fmt_time(newest['published'])})")
    print(f"  per day: {pd:,.1f} (raw {per_day(prev['total'], days):,.1f})")
    print(f"  by platform, after bots: {platform_line(prev['clean'])}")
    print(f"  updater: {prev['updater']:,} (SHA256SUMS {prev['sums']:,} minus bots {floor_text(prev)})")
    if len(releases) >= 3:
        before = releases[-3]
        bdays = window_days(releases, len(releases) - 3, now)
        bpd = per_day(before["clean_total"], bdays)
        diff = prev["clean_total"] - before["clean_total"]
        print(f"  vs {before['tag']}: after bots {prev['clean_total']:,} vs {before['clean_total']:,}: "
              f"{signed(diff)} ({change_pct(prev['clean_total'], before['clean_total'])})")
        print(f"  vs {before['tag']}: per day after bots {pd:,.1f} vs {bpd:,.1f}: "
              f"{signed(pd - bpd)} ({change_pct(pd, bpd)})")


def section_current(releases, now):
    print("== Current release so far ==")
    if not releases:
        print("  no razdor release yet")
        return
    newest = releases[-1]
    days = max(days_between(now, newest["published"]), 0.0)
    print(f"{newest['tag']}: {after_bots(newest)} so far, published {fmt_time(newest['published'])}, "
          f"{days:.2f} days ago")
    print(f"  per day: {per_day(newest['clean_total'], days):,.1f} (raw {per_day(newest['total'], days):,.1f})")
    print(f"  by platform, after bots: {platform_line(newest['clean'])}")
    print(f"  updater so far: {newest['updater']:,} (SHA256SUMS {newest['sums']:,} minus bots {floor_text(newest)})")


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


def empty_bucket():
    return {"total": 0, "raw": 0, "platforms": {}, "updater": 0, "releases": 0}


def bucket_by(releases, key):
    """Sum the releases per key (a date); total and platforms are after bots, raw is before."""
    buckets = {}
    for rel in releases:
        bucket = buckets.setdefault(key(rel), empty_bucket())
        bucket["total"] += rel["clean_total"]
        bucket["raw"] += rel["total"]
        bucket["updater"] += rel["updater"]
        bucket["releases"] += 1
        for platform, downloads in rel["clean"].items():
            bucket["platforms"][platform] = bucket["platforms"].get(platform, 0) + downloads
    return buckets


def weekly_rows(releases, now):
    """(week start, bucket) for every ISO week from the first release to the current one, oldest first."""
    if not releases:
        return []
    buckets = bucket_by(releases, lambda r: week_start(r["published"]))
    rows, start, current = [], week_start(releases[0]["published"]), week_start(now)
    while start <= current:
        rows.append((start, buckets.get(start, empty_bucket())))
        start += timedelta(days=7)
    return rows


def daily_rows(releases, now, days=14):
    """(UTC date, bucket) for the last `days` UTC dates, oldest first, zero days included."""
    buckets = bucket_by(releases, lambda r: r["published"].date())
    today = now.date()
    dates = [today - timedelta(days=k) for k in range(days - 1, -1, -1)]
    return [(day, buckets.get(day, empty_bucket())) for day in dates]


def week_start(dt):
    """Monday of the ISO week of dt (UTC date)."""
    return dt.date() - timedelta(days=dt.weekday())


def compact_platforms(platforms):
    """Downloads as win/linux/mac/android/ios, plus other when there is any."""
    text = "/".join(f"{platforms.get(p, 0):,}" for p in COMPACT_ORDER)
    other = platforms.get("other", 0)
    return f"{text} +{other:,} other" if other else text


def total_text(bucket):
    return f"{bucket['total']:,} (raw {bucket['raw']:,})"


def bar(value, top):
    width = round(value / top * BAR_WIDTH) if top > 0 else 0
    if value > 0 and width == 0:
        width = 1
    return "█" * width


def section_weekly(releases, now, weeks):
    print("== Downloads by week (releases published that ISO week, Monday-start, UTC) ==")
    rows = weekly_rows(releases, now)
    if not rows:
        print("  no razdor release yet")
        return
    shown = rows[-weeks:]
    labels = []
    current = week_start(now)
    for start, _ in shown:
        label = start.isoformat()
        if start == current:
            label += f" (so far, {(now.date() - start).days + 1} days)"
        labels.append(label)
    label_width = max(len(label) for label in labels + ["week of (Monday)"])
    totals = [total_text(bucket) for _, bucket in shown]
    total_width = max(len(text) for text in totals + ["after bots (raw)"])
    compacts = [compact_platforms(bucket["platforms"]) for _, bucket in shown]
    compact_width = max(len(text) for text in compacts + ["win/linux/mac/android/ios"])
    top = max(bucket["total"] for _, bucket in shown)
    print(f"  {'week of (Monday)':<{label_width}}  {'after bots (raw)':<{total_width}}  "
          f"{'updater':>8}  {'win/linux/mac/android/ios':<{compact_width}}")
    for label, text, compact, (_, bucket) in zip(labels, totals, compacts, shown):
        print(f"  {label:<{label_width}}  {text:<{total_width}}  {bucket['updater']:>8,}  "
              f"{compact:<{compact_width}}  {bar(bucket['total'], top)}")
    # The current week is always the last row, so the complete weeks are the rest.
    complete = rows[:-1]
    if len(complete) < 2:
        print("  week over week: not enough complete weeks yet")
        return
    (prev_start, prev), (last_start, last) = complete[-2], complete[-1]
    diff = last["total"] - prev["total"]
    print(f"  last complete week vs the one before (week of {last_start} vs {prev_start}), after bots: "
          f"{signed(diff)} ({change_pct(last['total'], prev['total'])}), "
          f"{last['total']:,} vs {prev['total']:,}")


def section_daily(releases, now):
    print("== Downloads by day, last 14 days (releases published that UTC day) ==")
    rows = daily_rows(releases, now)
    if not any(bucket["releases"] for _, bucket in rows):
        print("  no razdor release in the last 14 days")
        return
    totals = [total_text(bucket) for _, bucket in rows]
    total_width = max(len(text) for text in totals + ["after bots (raw)"])
    compacts = [compact_platforms(bucket["platforms"]) for _, bucket in rows]
    compact_width = max(len(text) for text in compacts + ["win/linux/mac/android/ios"])
    top = max(bucket["total"] for _, bucket in rows)
    print(f"  {'day (UTC)':<10}  {'after bots (raw)':<{total_width}}  "
          f"{'updater':>8}  {'win/linux/mac/android/ios':<{compact_width}}")
    for (day, bucket), text, compact in zip(rows, totals, compacts):
        print(f"  {day.isoformat():<10}  {text:<{total_width}}  {bucket['updater']:>8,}  "
              f"{compact:<{compact_width}}  {bar(bucket['total'], top)}")


def section_chart(releases, now, count):
    print(f"== Downloads per day, last {min(count, len(releases))} releases: per release, after bots "
          f"(raw in brackets; noisy while releases were more frequent than daily), oldest -> newest ==")
    if not releases:
        print("  no razdor release yet")
        return
    start = max(0, len(releases) - count)
    points = []
    for i in range(start, len(releases)):
        rel = releases[i]
        days = window_days(releases, i, now)
        points.append((rel["tag"], per_day(rel["clean_total"], days), per_day(rel["total"], days)))
    top = max(clean for _, clean, _ in points)
    for tag, clean, raw in points:
        width = round(clean / top * BAR_WIDTH) if top > 0 else 0
        if clean > 0 and width == 0:
            width = 1
        print(f"{tag:<10} {clean:>9,.1f} (raw {raw:,.1f})  {'█' * width}")


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
    assumed = [rel["tag"] for rel in releases if rel["floor_assumed"]]
    if assumed:
        print(f"Note: {len(assumed)} releases have no translate assets; their bot floor is assumed to be "
              f"{BOT_FLOOR_ASSUMED} (the level seen on every release that has them).")
        print()
    section_window(releases, now)
    print()
    section_current(releases, now)
    print()
    section_catalog(catalogs, existing)
    print()
    section_weekly(releases, now, args.weeks)
    print()
    section_daily(releases, now)
    print()
    section_chart(releases, now, args.releases)
    print()
    print(UPDATER_NOTE)
    print(TREND)
    return 0


def main(argv=None):
    parser = argparse.ArgumentParser(description="Razdor release download statistics (private).")
    parser.add_argument("--csv", default=os.environ.get("RAZDOR_STATS_CSV") or DEFAULT_CSV,
                        help="CSV to append to (default: $RAZDOR_STATS_CSV or %(default)s)")
    parser.add_argument("--no-save", action="store_true", help="print the summary only, write nothing")
    parser.add_argument("--releases", type=int, default=12, help="releases in the chart (default 12)")
    parser.add_argument("--weeks", type=int, default=8, help="weeks in the weekly view (default 8)")
    args = parser.parse_args(argv)
    if args.releases < 1:
        parser.error("--releases must be at least 1")
    if args.weeks < 1:
        parser.error("--weeks must be at least 1")
    try:
        return run(args)
    except StatsError as e:
        print(f"error: {e}", file=sys.stderr)
        print("Nothing was written to the CSV.", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
