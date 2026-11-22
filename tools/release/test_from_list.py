"""Tests of from_list.py (the --from list) and of update_check's release list paging. Hermetic."""

import contextlib
import datetime as dt
import io
import json
import os
import sys
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)

import from_list as fl  # noqa: E402
import update_check as uc  # noqa: E402
from testkit import HermeticTestCase  # noqa: E402

NOW = dt.datetime(2026, 11, 22, 12, 0, tzinfo=dt.timezone.utc)
REPO = "indicozy/razdor"


def snap(days_ago):
    return (NOW - dt.timedelta(days=days_ago)).strftime("%Y-%m-%dT%H:%M:%SZ")


def csv_text(rows):
    lines = ["snapshot_utc,repo,tag,published_at,platform,downloads,approx"]
    lines += [",".join(str(x) for x in r) for r in rows]
    return "\n".join(lines) + "\n"


class ChooseTests(unittest.TestCase):
    V = [(1, 5, 4), (1, 5, 3), (1, 5, 0), (1, 4, 1), (1, 4, 0), (1, 3, 0), (0, 4, 13)]

    def test_previous_is_the_newest_published_before_the_new_one(self):
        got = fl.choose(self.V, (1, 5, 5), {})
        self.assertEqual(got[0][0], (1, 5, 4))
        self.assertEqual(got[0][1], "previous release")

    def test_before_the_tag_the_previous_is_the_newest_published(self):
        got = fl.choose(self.V, None, {})
        self.assertEqual(got[0][0], (1, 5, 4))

    def test_the_new_release_and_newer_ones_are_never_listed(self):
        got = dict(fl.choose(self.V + [(1, 5, 5), (1, 6, 0)], (1, 5, 5), {}))
        self.assertNotIn((1, 5, 5), got)
        self.assertNotIn((1, 6, 0), got)

    def test_oldest_still_in_use_per_line(self):
        counts = {(1, 5, 0): (3, False), (1, 5, 3): (9, False), (1, 4, 0): (0, False), (1, 3, 0): (2, True)}
        got = dict(fl.choose(self.V, (1, 5, 5), counts))
        self.assertIn((1, 5, 0), got)  # the oldest of 1.5 with downloads (the floor too)
        self.assertNotIn((1, 5, 3), got)  # a newer 1.5 is not added once 1.5.0 is
        self.assertNotIn((1, 4, 0), got)  # no downloads in the window
        self.assertIn((1, 3, 0), got)  # a lower bound still counts as in use
        self.assertIn("a lower bound", dict(fl.choose(self.V, (1, 5, 5), counts))[(1, 3, 0)])

    def test_floor_is_kept_when_it_exists(self):
        got = dict(fl.choose(self.V, (1, 5, 5), {}))
        self.assertIn((1, 4, 1), got)
        self.assertIn((1, 5, 0), got)
        self.assertNotIn((1, 4, 0), got)

    def test_floor_missing_is_skipped(self):
        got = dict(fl.choose([(1, 5, 4), (1, 4, 0)], (1, 5, 5), {}))
        self.assertEqual(sorted(got), [(1, 5, 4)])  # 1.4.1 is not published; 1.4.0 is no floor and has no downloads

    def test_min_downloads_raises_the_bar(self):
        counts = {(1, 3, 0): (5, False), (0, 4, 13): (1, False)}
        got = dict(fl.choose(self.V, (1, 5, 5), counts, min_downloads=2))
        self.assertIn((1, 3, 0), got)
        self.assertNotIn((0, 4, 13), got)

    def test_nothing_before_the_new_release_is_an_error(self):
        with self.assertRaises(fl.ListError):
            fl.choose([(1, 5, 5)], (1, 5, 5), {})


class GrowthTests(unittest.TestCase):
    def test_counts_growth_since_the_last_snapshot_before_the_window(self):
        pts = [(fl.parse_time(snap(40)), 10), (fl.parse_time(snap(20)), 14), (fl.parse_time(snap(1)), 19)]
        # the baseline is the last snapshot at or before the window's start (40 days ago: 10), the newest is 19
        self.assertEqual(fl.growth(pts, snap(100), NOW, 30), (9, False))

    def test_a_release_published_inside_the_window_counts_from_zero(self):
        pts = [(fl.parse_time(snap(10)), 7)]
        self.assertEqual(fl.growth(pts, snap(12), NOW, 30), (7, False))

    def test_no_baseline_before_the_window_is_a_lower_bound(self):
        pts = [(fl.parse_time(snap(5)), 4), (fl.parse_time(snap(1)), 9)]
        self.assertEqual(fl.growth(pts, snap(200), NOW, 30), (5, True))

    def test_no_snapshot_no_growth(self):
        self.assertEqual(fl.growth([], snap(200), NOW, 30), (0, False))


class StatsReadTests(HermeticTestCase):
    def test_only_player_platforms_of_razdor_releases_count(self):
        path = os.path.join(self.root, "stats.csv")
        rows = [
            (snap(40), REPO, "v1.5.0", snap(90), "windows", 4, 0),
            (snap(40), REPO, "v1.5.0", snap(90), "linux", 2, 0),
            (snap(40), REPO, "v1.5.0", snap(90), "translate", 50, 0),  # bots: not counted
            (snap(40), REPO, "v1.5.0", snap(90), "sums", 50, 0),  # the checksum: not counted
            (snap(40), REPO, "v1.5.0", snap(90), "other", 50, 0),  # not a player platform
            (snap(40), "indicozy/razdor-community", "catalog-2026-11-01", snap(90), "all", 99, 0),
            (snap(1), REPO, "v1.5.0", snap(90), "windows", 5, 0),
        ]
        with open(path, "w", encoding="utf-8") as f:
            f.write(csv_text(rows))
        published, series = fl.read_stats(path)
        self.assertEqual(published["v1.5.0"], snap(90))
        # the early snapshot: windows 4 + linux 2 (translate, sums, other left out); the later one: windows 5
        self.assertEqual([n for _, n in series["v1.5.0"]], [6, 5])
        self.assertNotIn("catalog-2026-11-01", series)


class BuildTests(HermeticTestCase):
    def write_stats(self):
        path = os.path.join(self.root, "stats.csv")
        rows = [
            (snap(40), REPO, "v1.5.0", snap(120), "windows", 10, 0),
            (snap(1), REPO, "v1.5.0", snap(120), "windows", 12, 0),
            (snap(40), REPO, "v1.4.1", snap(150), "windows", 3, 0),
            (snap(1), REPO, "v1.4.1", snap(150), "windows", 3, 0),
            (snap(1), REPO, "v1.5.4", snap(20), "windows", 500, 0),
        ]
        with open(path, "w", encoding="utf-8") as f:
            f.write(csv_text(rows))
        return path

    def write_releases(self, tags):
        path = os.path.join(self.root, "releases.json")
        data = [{"tag_name": t, "draft": False, "prerelease": False} for t in tags]
        data.append({"tag_name": "v9.9.9", "draft": True, "prerelease": False})
        with open(path, "w", encoding="utf-8") as f:
            json.dump(data, f)
        return path

    def test_build_uses_the_stats_and_the_published_list(self):
        csv_path = self.write_stats()
        rel = self.write_releases(["v1.5.4", "v1.5.3", "v1.5.0", "v1.4.1", "v1.4.0"])
        got = fl.build((1, 5, 5), csv_path, rel, 30, NOW, False)
        vs = [v for v, _ in got]
        self.assertEqual(vs[0], (1, 5, 4))
        self.assertIn((1, 5, 0), vs)  # 2 downloads in the window
        self.assertIn((1, 4, 1), vs)  # the floor, and 0 new downloads
        self.assertNotIn((1, 4, 0), vs)
        self.assertNotIn((9, 9, 9), vs)

    def test_main_prints_the_args_line(self):
        csv_path = self.write_stats()
        rel = self.write_releases(["v1.5.4", "v1.5.0", "v1.4.1"])
        out, err = io.StringIO(), io.StringIO()
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
            code = fl.main(["--to", "v1.5.5", "--csv", csv_path, "--releases", rel, "--now", snap(0), "--args"])
        self.assertEqual(code, 0)
        self.assertEqual(out.getvalue().strip(), "--from v1.5.4 --from v1.5.0 --from v1.4.1")

    def test_missing_csv_is_an_error_unless_allowed(self):
        rel = self.write_releases(["v1.5.4", "v1.4.1"])
        missing = os.path.join(self.root, "none.csv")
        with contextlib.redirect_stderr(io.StringIO()):
            self.assertEqual(fl.main(["--to", "v1.5.5", "--csv", missing, "--releases", rel, "--now", snap(0)]), 2)
            out = io.StringIO()
            with contextlib.redirect_stdout(out):
                code = fl.main(["--to", "v1.5.5", "--csv", missing, "--releases", rel, "--now", snap(0),
                                "--allow-no-stats", "--args"])
        self.assertEqual(code, 0)
        self.assertEqual(out.getvalue().strip(), "--from v1.5.4 --from v1.4.1")

    def test_no_published_release_before_is_exit_2(self):
        csv_path = self.write_stats()
        rel = self.write_releases(["v1.5.5"])
        with contextlib.redirect_stderr(io.StringIO()):
            self.assertEqual(fl.main(["--to", "v1.5.5", "--csv", csv_path, "--releases", rel, "--now", snap(0)]), 2)


class ReleaseListTests(HermeticTestCase):
    def page(self, n, start=0):
        return [{"tag_name": f"v0.{start + i}.0", "draft": False, "prerelease": False} for i in range(n)]

    def test_all_pages_are_read(self):
        pages = {1: json.dumps(self.page(100)), 2: json.dumps(self.page(100, 100)), 3: json.dumps(self.page(7, 200))}
        asked = []

        def get(url):
            page = int(url.rsplit("page=", 1)[1])
            asked.append(page)
            return pages.get(page, "[]")

        data = uc.release_list(get)
        self.assertEqual(len(data), 207)
        self.assertEqual(asked, [1, 2, 3])

    def test_a_short_first_page_stops_the_paging(self):
        asked = []

        def get(url):
            asked.append(url)
            return json.dumps(self.page(3))

        self.assertEqual(len(uc.release_list(get)), 3)
        self.assertEqual(len(asked), 1)
        self.assertIn("per_page=100", asked[0])

    def test_drafts_and_pre_releases_are_not_published(self):
        data = [
            {"tag_name": "v1.5.4", "draft": False, "prerelease": False},
            {"tag_name": "v1.5.5", "draft": False, "prerelease": True},
            {"tag_name": "v1.5.6", "draft": True, "prerelease": False},
            {"tag_name": "debug-1618", "draft": False, "prerelease": False},
        ]
        self.assertEqual(sorted(uc.published_map(data)), [(1, 5, 4)])

    def test_a_list_that_is_not_json_is_an_error(self):
        with self.assertRaises(uc.ToolError):
            uc.release_list(lambda url: "<html>")

    def test_no_published_release_is_an_error(self):
        with self.assertRaises(uc.ToolError):
            uc.load_published(lambda url: "[]")


if __name__ == "__main__":
    unittest.main()
