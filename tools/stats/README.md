# Download statistics (private)

`downloads.py` reads the download counts of Razdor's releases and of the Hub's catalog
releases from GitHub, appends a snapshot to a CSV, and prints a short summary: the latest
finished release window, the current release so far, the catalog, and downloads per day over
the last releases.

## The premise

The in-game updater downloads release assets, so a release's download count is roughly the
number of players who updated to it. It is a trend, not a headcount: manual downloads count
too, a retried update can count twice, and players with updates off are missing.

## Weekly and daily views

Releases came up to ten a day at times, so a single release's count mostly shows how often
releases were cut, not how popular the game is. The summary therefore has two trend views:
"Downloads by week" (the downloads of the releases published in each ISO week, Monday start,
UTC, with the change of the last complete week against the one before; `--weeks N`, default 8)
and "Downloads by day" (the same for each UTC day of the last 14). Read these two; the per-release
chart is kept, but it is noisy while releases were more frequent than daily. The totals are
cumulative, so an older week has had longer to collect downloads than the current one.

## Run

```
python3 -I tools/stats/downloads.py                 # append a snapshot, print the summary
python3 -I tools/stats/downloads.py --no-save       # print only
python3 -I tools/stats/downloads.py --releases 20   # longer per-release chart
python3 -I tools/stats/downloads.py --weeks 12      # longer weekly view
```

The release skill runs it at every release and shows the summary to the owner.

Options: `--csv PATH` (default: `$RAZDOR_STATS_CSV`, else
`/mnt/ai_models/razdor-stats/downloads.csv`). A GitHub token in `$GH_TOKEN` is used when set,
else `gh auth token -u indicozy`, else the API is called unauthenticated (60 requests an hour).

## The CSV

Columns: `snapshot_utc,repo,tag,published_at,platform,downloads,approx`.

- One row per razdor release and platform, one row per catalog release (platform `all`), per run.
- `approx=0` for live snapshots.
- On the first run, when the CSV does not exist, a backfill is written first with `approx=1`:
  each release's total up to the next release's publication. The backfill overestimates old
  releases, because their counts include downloads that came long after their window closed.

The CSV holds the counts of the owner's players. It lives outside the repository and is never
committed.
