"""Unit tests for the battle verifier's script (no server, no verifier binary)."""

import unittest

import verify_battles as vb


class Verdicts(unittest.TestCase):
    def test_ok_is_posted_as_ok(self):
        body = vb.verdict_body("b1", {"verdict": "ok", "reason": ""})
        self.assertEqual(body, {"schema": 1, "battle_id": "b1", "kind": "ok"})

    def test_suspicious_keeps_its_reason(self):
        body = vb.verdict_body("b2", {"verdict": "suspicious", "reason": "act 3 is illegal"})
        self.assertEqual(body["kind"], "suspicious")
        self.assertEqual(body["reason"], "act 3 is illegal")

    def test_suspicious_without_a_reason_gets_one(self):
        self.assertTrue(vb.verdict_body("b2", {"verdict": "suspicious"})["reason"])

    def test_unverified_is_posted_as_suspicious_with_a_prefix(self):
        body = vb.verdict_body("b3", {"verdict": "unverified", "reason": "a mod's battle"})
        self.assertEqual(body["kind"], "suspicious")
        self.assertEqual(body["reason"], "unverified: a mod's battle")

    def test_unknown_kind_is_not_posted(self):
        self.assertIsNone(vb.verdict_body("b4", {"verdict": "maybe"}))


class Parsing(unittest.TestCase):
    def test_the_last_json_line_is_the_verdict(self):
        out = 'warning: something\n{"verdict": "ok", "reason": ""}\n'
        self.assertEqual(vb.parse_verdict(out), {"verdict": "ok", "reason": ""})

    def test_no_json_means_no_verdict(self):
        self.assertIsNone(vb.parse_verdict("panic: boom\n"))
        self.assertIsNone(vb.parse_verdict(""))

    def test_broken_json_is_skipped(self):
        self.assertIsNone(vb.parse_verdict('{"verdict": '))


if __name__ == "__main__":
    unittest.main()
