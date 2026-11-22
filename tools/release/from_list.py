#!/usr/bin/env python3
"""The `--from` list of the update check (release steps 3b and 6b), from the download stats.

It prints the older releases the update check runs against, newest first, one `vX.Y.Z` per line
(`--args` prints them as `--from vX.Y.Z --from ...`):

- the previous release (the newest published one before `--to`; before the tag, without `--to`,
  the newest published release);
- the oldest release of each minor line (X.Y) that still had player downloads in the last
  `--window` days (default 30): the oldest one that is still in use, per line;
- v1.5.0 and v1.4.1 whenever they exist (the owner's floor, 2026-11-22).

"Had downloads" is the growth of the release's player-platform counts (windows, linux, macos,
android, ios) between the last snapshot at or before the window's start and the newest snapshot.
The `translate` and `sums` rows are not counted (bots and the updater's checksum fetch). A release
published inside the window counts from zero; a release older than the window with no snapshot
before its start counts from its first snapshot (a lower bound, reported on stderr).

Inputs: the stats CSV (`--csv`, else `$RAZDOR_STATS_CSV`, else `/mnt/ai_models/razdor-stats/downloads.csv`;
private, never printed except the reasons on stderr, never committed) and the published releases
(the GitHub API, all pages, through `update_check.load_published`; or `--releases FILE`, a JSON list
of release objects, for tests and offline use).

Exit codes: 0 the list is printed, 2 the list could not be made (no stats file, no network, no
published release, a bad version). `--allow-no-stats` prints the previous release and the floor
only, with a warning on stderr.

    python3 -I tools/release/from_list.py --to v1.5.5 --args
"""

import argparse
import csv
import datetime as dt
import json
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)

import update_check as uc  # noqa: E402

DEFAULT_CSV = "/mnt/ai_models/razdor-stats/downloads.csv"
PLAYER_PLATFORMS = ("windows", "linux", "macos", "android", "ios")
FLOOR = ((1, 5, 0), (1, 4, 1))  # always checked when published (the owner's floor, 2026-11-22)
REPO = "indicozy/razdor"
EXIT_OK, EXIT_ERROR = 0, 2


class ListError(Exception):
    """The list could not be made (exit 2)."""


def parse_time(text):
    return dt.datetime.strptime(text.strip(), "%Y-%m-%dT%H:%M:%SZ").replace(tzinfo=dt.timezone.utc)


def read_stats(path):
    """(published_at per tag, {tag: [(snapshot time, player total)]}) of the razdor release rows."""
    published, series = {}, {}
    totals = {}  # (tag, snapshot) -> total
    try:
        f = open(path, newline="", encoding="utf-8")
    except OSError as e:
        raise ListError(f"cannot read the stats CSV {path}: {e}") from e
    with f:
        for row in csv.DictReader(f):
            if row.get("repo") != REPO:
                continue
            tag = row.get("tag", "")
            if not uc.VERSION_RE.match(tag):
                continue
            if row.get("published_at"):
                published.setdefault(tag, row["published_at"])
            if row.get("platform") not in PLAYER_PLATFORMS:
                continue
            try:
                n = int(row.get("downloads", ""))
                when = parse_time(row["snapshot_utc"])
            except (ValueError, KeyError):
                continue
            key = (tag, when)
            totals[key] = totals.get(key, 0) + n
    for (tag, when), n in totals.items():
        series.setdefault(tag, []).append((when, n))
    for points in series.values():
        points.sort()
    return published, series


def growth(points, published_at, now, window):
    """The player downloads a release got inside the window, and whether that is a lower bound."""
    if not points:
        return 0, False
    cutoff = now - dt.timedelta(days=window)
    before = [p for p in points if p[0] <= cutoff]
    latest = points[-1][1]
    if before:
        return latest - before[-1][1], False
    if published_at and parse_time(published_at) > cutoff:
        return latest, False
    return latest - points[0][1], True


def choose(published, to, counts, min_downloads=1):
    """The `--from` versions, newest first, and a reason for each.

    `published`: the published versions (tuples). `to`: the new version (a tuple) or None before the
    tag. `counts`: {version: (downloads in the window, lower bound?)}; a version missing from it has none.
    """
    older = sorted((v for v in published if to is None or v < to), reverse=True)
    if not older:
        raise ListError("no published release before " + ("the new one" if to else "the newest"))
    chosen = {}
    previous = older[0]
    chosen[previous] = "previous release"
    lines = {}
    for v in older:
        lines.setdefault(v[:2], []).append(v)
    for line in sorted(lines, reverse=True):
        for v in sorted(lines[line]):  # oldest first
            n, bound = counts.get(v, (0, False))
            if n >= min_downloads:
                chosen.setdefault(v, f"oldest still in use of {line[0]}.{line[1]}: {n} player downloads"
                                  + (" (a lower bound)" if bound else "") + " in the window")
                break
    for f in FLOOR:
        if f in older:
            chosen.setdefault(f, "floor: kept whenever it exists")
    return sorted(chosen.items(), reverse=True)


def vtag(v):
    return "v%d.%d.%d" % v


def published_versions(releases_file=None):
    if releases_file:
        try:
            with open(releases_file, encoding="utf-8") as f:
                data = json.load(f)
        except (OSError, ValueError) as e:
            raise ListError(f"cannot read {releases_file}: {e}") from e
        if not isinstance(data, list):
            raise ListError(f"{releases_file} is not a JSON list of releases")
    else:
        try:
            return sorted(uc.load_published())
        except uc.ToolError as e:
            raise ListError(str(e)) from e
    return sorted(uc.published_map(data))


def build(to, csv_path, releases_file, window, now, allow_no_stats, min_downloads=1):
    published = published_versions(releases_file)
    if not os.path.isfile(csv_path):
        if not allow_no_stats:
            raise ListError(f"no stats CSV at {csv_path} (set RAZDOR_STATS_CSV, or --allow-no-stats)")
        print(f"from_list: no stats CSV at {csv_path}: previous release and the floor only", file=sys.stderr)
        counts = {}
        dates = {}
    else:
        dates, series = read_stats(csv_path)
        counts = {}
        for tag, points in series.items():
            v = tuple(int(x) for x in uc.VERSION_RE.match(tag).groups())
            counts[v] = growth(points, dates.get(tag), now, window)
    return choose(published, to, counts, min_downloads)


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--to", metavar="vX.Y.Z", help="the new release (after the release: the newest published one); "
                    "omit before the tag")
    ap.add_argument("--csv", help=f"the stats CSV (default: $RAZDOR_STATS_CSV, else {DEFAULT_CSV})")
    ap.add_argument("--releases", metavar="FILE", help="the published releases as a JSON list (default: the GitHub API)")
    ap.add_argument("--window", type=int, default=30, help="days of downloads that count as in use (default 30)")
    ap.add_argument("--now", help="the time to measure from, ISO 8601 UTC (default: now; for tests)")
    ap.add_argument("--min-downloads", type=int, default=1, help="player downloads in the window that make a release "
                    "still in use (default 1)")
    ap.add_argument("--allow-no-stats", action="store_true", help="without a stats CSV: previous release and the floor only")
    ap.add_argument("--args", action="store_true", help="print them as update_check's arguments: --from vX.Y.Z ...")
    a = ap.parse_args(argv)
    try:
        to = None
        if a.to:
            to = uc.parse_version(a.to)
        csv_path = a.csv or os.environ.get("RAZDOR_STATS_CSV") or DEFAULT_CSV
        now = parse_time(a.now) if a.now else dt.datetime.now(dt.timezone.utc)
        picks = build(to, csv_path, a.releases, a.window, now, a.allow_no_stats, a.min_downloads)
    except (ListError, uc.ToolError) as e:
        print(f"from_list: {e}", file=sys.stderr)
        return EXIT_ERROR
    for v, why in picks:
        print(f"from_list: {vtag(v)}: {why}", file=sys.stderr)
    if a.args:
        print(" ".join(f"--from {vtag(v)}" for v, _ in picks))
    else:
        for v, _ in picks:
            print(vtag(v))
    return EXIT_OK


if __name__ == "__main__":
    sys.exit(main())
