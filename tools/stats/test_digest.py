"""Unit tests for the weekly digest's formatting (no server, no Telegram)."""

import unittest

import digest

SAMPLE = {
    "generated_at": 1763827200,
    "players_per_day": [{"name": f"2026-11-{d:02d}", "value": 100 + d} for d in range(10, 20)],
    "week_by_version": [{"name": "1.5.4", "value": 300}, {"name": "1.5.3", "value": 40}],
    "week_by_platform": [{"name": "linux", "value": 200}, {"name": "android", "value": 140}],
    "week_by_game": [{"name": "dt", "value": 330}],
    "retention": {"week1_cohort": 50, "week1_returned": 20, "week4_cohort": 0, "week4_returned": 0},
    "maps": [
        {"map": "Small", "starts": 5, "wins": 1, "losses": 1, "abandoned": 3, "abandoned_at": [{"name": "map", "value": 3}]},
        {"map": "Big", "starts": 90, "wins": 40, "losses": 30, "abandoned": 20, "abandoned_at": [{"name": "town screen", "value": 12}, {"name": "battle", "value": 8}]},
    ],
    "crashes": [{"location": "src/ui/map.rs:10", "message": "index out of bounds", "count": 7, "last_day": "2026-11-21", "last_version": "1.5.4"}],
    "fps": [
        {"gpu": "Intel UHD", "phone": "", "sessions": 3, "median_ms": 22.5, "p99_ms": 40.0},
        {"gpu": "RTX 4060", "phone": "", "sessions": 80, "median_ms": 7.2, "p99_ms": 12.3},
    ],
    "experiments": [],
    "bans": [
        {"kind": "install", "key": "abcdef0123456789", "strikes": 2, "active": True, "reason": "more than 3 pings a day", "created_day": "2026-11-20"},
        {"kind": "ip", "key": "10.0.0.1", "strikes": 1, "active": False, "reason": "flood", "created_day": "2026-11-01"},
    ],
    "cloud": {"accounts": 4, "used_bytes": 1000, "syncs_per_day": []},
}


class Digest(unittest.TestCase):
    def setUp(self):
        self.text = digest.format_digest(SAMPLE)

    def test_players_show_the_last_seven_days(self):
        self.assertIn("2026-11-13 113", self.text)
        self.assertIn("2026-11-19 119", self.text)
        self.assertNotIn("2026-11-12 112", self.text)

    def test_versions_and_platforms_are_listed_in_order(self):
        self.assertIn("By version (week): 1.5.4 300, 1.5.3 40", self.text)
        self.assertIn("By platform (week): linux 200, android 140", self.text)

    def test_retention_has_counts_and_percent(self):
        self.assertIn("Retention: week 1 20/50 (40%), week 4 0/0 (n/a)", self.text)

    def test_maps_are_by_starts_with_the_top_drop_point(self):
        self.assertLess(self.text.index("Big:"), self.text.index("Small:"))
        self.assertIn("Big: 90, 40, 30, 20; most quit at town screen (12)", self.text)
        self.assertIn("Small: 5, 1, 1, 3; most quit at map (3)", self.text)

    def test_crashes_and_devices(self):
        self.assertIn("Crash groups: 1", self.text)
        self.assertIn("7x src/ui/map.rs:10: index out of bounds (last 1.5.4)", self.text)
        self.assertIn("RTX 4060: 7.2, 12.3 (80 sessions)", self.text)
        self.assertLess(self.text.index("RTX 4060"), self.text.index("Intel UHD"))

    def test_bans_count_only_the_active_ones(self):
        self.assertIn("Bans: 1 active, 2 listed", self.text)
        self.assertIn("install abcdef0123456789: more than 3 pings a day (2026-11-20)", self.text)
        self.assertNotIn("flood", self.text)

    def test_empty_summary_still_formats(self):
        text = digest.format_digest({})
        self.assertIn("Players per day: none", text)
        self.assertIn("Bans: 0 active, 0 listed", text)

    def test_long_text_is_cut(self):
        self.assertEqual(len(digest.cut("x" * 100)), digest.MAX_MESSAGE)
        self.assertEqual(digest.cut("a   b"), "a b")


if __name__ == "__main__":
    unittest.main()
