"""Release packing, SHA256SUMS, the update feed and the cleanup (no network)."""

import hashlib
import json
import os
import sys
import tempfile
import unittest
import zipfile

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)

import gitverse as gv  # noqa: E402
from testkit import HermeticTestCase  # noqa: E402
import publish_release as pr  # noqa: E402

V = "1.5.2"
PAYLOAD = {
    f"Razdor-{V}.exe": b"win program\n",
    f"Razdor-translate-{V}.exe": b"win translator\n",
    f"razdor-{V}-linux": b"linux program\n",
    f"razdor-translate-{V}-linux": b"linux translator\n",
    f"razdor-{V}-macos": b"mac program\n",
    f"razdor-translate-{V}-macos": b"mac translator\n",
    f"razdor-{V}.apk": b"apk bytes\n",
    f"razdor-{V}.ipa": b"ipa bytes\n",
    f"razdor-ads-{V}.zip": b"ads bytes\n",
    # the unversioned copies the pipeline also publishes
    "Razdor.exe": b"win program\n",
    "Razdor-translate.exe": b"win translator\n",
    "razdor": b"linux program\n",
    "razdor-translate": b"linux translator\n",
    "razdor-macos": b"mac program\n",
    "razdor-translate-macos": b"mac translator\n",
    "razdor.apk": b"apk bytes\n",
    "razdor.ipa": b"ipa bytes\n",
}


def sha(data):
    return hashlib.sha256(data).hexdigest()


class ReleaseCase(HermeticTestCase):
    def setUp(self):
        super().setUp()
        self._tmp = tempfile.TemporaryDirectory()
        self.dir = self._tmp.name
        self.paths = []
        for name, data in PAYLOAD.items():
            q = os.path.join(self.dir, name)
            with open(q, "wb") as f:
                f.write(data)
            self.paths.append(q)

    def tearDown(self):
        self._tmp.cleanup()

    def full_plan(self):
        return pr.plan(self.paths, V)


class PlanTests(ReleaseCase):
    def test_versioned_programs_group_and_unversioned_copies_skipped(self):
        p = self.full_plan()
        self.assertEqual(sorted(g["zip"] for g in p["groups"]),
                         [f"Razdor-{V}-windows.zip", f"razdor-{V}-linux.zip", f"razdor-{V}-macos.zip"])
        skipped = {n for n, _ in p["skipped"]}
        self.assertTrue({"Razdor.exe", "razdor", "razdor-macos", "razdor.apk", "razdor.ipa",
                         "Razdor-translate.exe", "razdor-translate", "razdor-translate-macos"} <= skipped)
        self.assertEqual(sorted(p["direct"]), sorted([f"razdor-{V}.apk", f"razdor-ads-{V}.zip"]))
        self.assertTrue(any(f"razdor-{V}.ipa" in w and "not mirrored" in w for w in p["warnings"]))
        self.assertNotIn(f"razdor-{V}.ipa", p["direct"])

    def test_refused_name_stops(self):
        extra = os.path.join(self.dir, "Setup.exe")
        with open(extra, "wb") as f:
            f.write(b"x")
        with self.assertRaises(pr.PlanError):
            pr.plan(self.paths + [extra], V)

    def test_other_version_stops(self):
        other = os.path.join(self.dir, "razdor-1.5.1-linux")
        with open(other, "wb") as f:
            f.write(b"x")
        with self.assertRaises(pr.PlanError):
            pr.plan(self.paths + [other], V)

    def test_version_of_tag(self):
        self.assertEqual(pr.version_of("v1.5.2"), "1.5.2")
        self.assertEqual(pr.version_of("v0.0.1-tooltest"), "0.0.1")
        with self.assertRaises(pr.PlanError):
            pr.version_of("1.5.2")


class PackTests(ReleaseCase):
    def test_zips_hold_unversioned_names_with_mode_0755(self):
        p = self.full_plan()
        out = os.path.join(self.dir, "out")
        os.makedirs(out)
        zips = pr.pack(p["groups"], out)
        want = {
            f"Razdor-{V}-windows.zip": {"Razdor.exe": b"win program\n", "Razdor-translate.exe": b"win translator\n"},
            f"razdor-{V}-linux.zip": {"razdor": b"linux program\n", "razdor-translate": b"linux translator\n"},
            f"razdor-{V}-macos.zip": {"razdor-macos": b"mac program\n",
                                      "razdor-translate-macos": b"mac translator\n"},
        }
        self.assertEqual(sorted(zips), sorted(want))
        for zname, members in want.items():
            with zipfile.ZipFile(zips[zname]) as z:
                self.assertEqual(sorted(z.namelist()), sorted(members))
                for info in z.infolist():
                    self.assertEqual((info.external_attr >> 16) & 0o777, 0o755, info.filename)
                    self.assertEqual(z.read(info.filename), members[info.filename])

    def test_program_without_translator_packs_alone(self):
        paths = [p for p in self.paths if not p.endswith("razdor-translate-1.5.2-linux")]
        p = pr.plan(paths, V)
        linux = next(g for g in p["groups"] if g["platform"] == "linux")
        self.assertIsNone(linux["tr"])
        out = os.path.join(self.dir, "out")
        os.makedirs(out)
        zips = pr.pack(p["groups"], out)
        with zipfile.ZipFile(zips[f"razdor-{V}-linux.zip"]) as z:
            self.assertEqual(z.namelist(), ["razdor"])


class SumsTests(ReleaseCase):
    def test_sums_lines_cover_uploads_members_and_names_the_updater_looks_up(self):
        p = self.full_plan()
        out = os.path.join(self.dir, "out")
        os.makedirs(out)
        zips = pr.pack(p["groups"], out)
        lines = pr.sums_lines(p, V, zips, lambda path: gv.sha256_file(path))
        win = sha(PAYLOAD["Razdor.exe"])
        self.assertEqual(lines["Razdor.exe"], win)
        self.assertEqual(lines[f"Razdor-{V}.exe"], win)
        self.assertEqual(lines["razdor"], sha(PAYLOAD["razdor"]))
        self.assertEqual(lines[f"razdor-{V}-linux"], sha(PAYLOAD["razdor"]))
        self.assertEqual(lines["razdor-translate"], sha(PAYLOAD["razdor-translate"]))
        self.assertEqual(lines["razdor.apk"], sha(PAYLOAD[f"razdor-{V}.apk"]))
        self.assertEqual(lines[f"razdor-{V}.apk"], sha(PAYLOAD[f"razdor-{V}.apk"]))
        self.assertEqual(lines[f"Razdor-{V}-windows.zip"], gv.sha256_file(zips[f"Razdor-{V}-windows.zip"]))
        self.assertEqual(lines[f"razdor-ads-{V}.zip"], sha(PAYLOAD[f"razdor-ads-{V}.zip"]))
        self.assertNotIn("SHA256SUMS.txt", lines)

    def test_sums_text_is_sha256sum_format_sorted(self):
        text = pr.sums_text({"b.zip": "22", "a.zip": "11"})
        self.assertEqual(text, "11  a.zip\n22  b.zip\n")


class FeedTests(ReleaseCase):
    def entry_for(self, tag=f"v{V}", prerelease=False):
        p = self.full_plan()
        zip_urls = {g["zip"]: f"https://gitverse.ru/api/attachments/{g['zip']}" for g in p["groups"]}
        direct_urls = {n: f"https://gitverse.ru/api/attachments/{n}" for n in p["direct"]}
        return pr.feed_entry(tag, V, prerelease, "\n### Исправлено\n- Всё.", "https://example/release",
                             p["groups"], direct_urls, zip_urls, "https://gitverse.ru/api/attachments/sums")

    def test_entry_shape_and_archive_members(self):
        e = self.entry_for()
        self.assertEqual(e["tag_name"], f"v{V}")
        self.assertFalse(e["draft"])
        self.assertFalse(e["prerelease"])
        by_name = {a["name"]: a for a in e["assets"]}
        for name in (f"Razdor-{V}.exe", "Razdor.exe"):
            self.assertEqual(by_name[name]["archive"], "zip")
            self.assertEqual(by_name[name]["member"], "Razdor.exe")
            self.assertTrue(by_name[name]["browser_download_url"].endswith(f"Razdor-{V}-windows.zip"))
        for name in (f"razdor-{V}-linux", "razdor"):
            self.assertEqual(by_name[name]["member"], "razdor")
        self.assertEqual(by_name["razdor-macos"]["member"], "razdor-macos")
        self.assertNotIn("archive", by_name[f"razdor-{V}.apk"])
        self.assertEqual(by_name["razdor.apk"]["browser_download_url"],
                         by_name[f"razdor-{V}.apk"]["browser_download_url"])
        self.assertEqual(by_name["SHA256SUMS"]["browser_download_url"],
                         by_name["SHA256SUMS.txt"]["browser_download_url"])
        for a in e["assets"]:
            self.assertIn("name", a)
            self.assertIn("browser_download_url", a)

    def test_feed_keeps_newest_two_non_prereleases_newest_first(self):
        def e(tag, pre=False):
            return {"tag_name": tag, "draft": False, "prerelease": pre, "body": tag, "assets": []}

        prev = [e("v1.5.0"), e("v1.5.1"), e("v1.6.0-rc", True), e("v1.4.9")]
        merged = pr.merge_feed(prev, e("v1.5.2"), keep=2)
        self.assertEqual([x["tag_name"] for x in merged], ["v1.5.2", "v1.5.1"])
        self.assertEqual(json.loads(pr.feed_bytes(merged).decode("utf-8"))[0]["tag_name"], "v1.5.2")

    def test_feed_replaces_same_tag(self):
        prev = [{"tag_name": "v1.5.2", "assets": [], "body": "old"}]
        merged = pr.merge_feed(prev, {"tag_name": "v1.5.2", "assets": [], "body": "new"})
        self.assertEqual(merged, [{"tag_name": "v1.5.2", "assets": [], "body": "new"}])

    def test_feed_is_a_list_in_the_api_shape(self):
        data = json.loads(pr.feed_bytes(pr.merge_feed([], self.entry_for())).decode("utf-8"))
        self.assertIsInstance(data, list)
        self.assertEqual(set(data[0]), {"tag_name", "name", "html_url", "draft", "prerelease", "body", "assets"})


class CleanupTests(HermeticTestCase):
    def test_prune_keeps_newest_two_app_releases_by_version(self):
        reg = [
            {"repo": "razdor", "tag": "v1.5.2", "kind": "app", "prerelease": False, "id": 3},
            {"repo": "razdor", "tag": "v1.10.0", "kind": "app", "prerelease": False, "id": 4},
            {"repo": "razdor", "tag": "v1.5.10", "kind": "app", "prerelease": False, "id": 5},
            {"repo": "razdor", "tag": "v1.5.1", "kind": "app", "prerelease": False, "id": 2},
            {"repo": "razdor", "tag": "v2.0.0-rc", "kind": "app", "prerelease": True, "id": 9},
            {"repo": "razdor-community", "tag": "catalog-2026-11-01", "kind": "catalog", "prerelease": False},
        ]
        gone = [e["tag"] for e in gv.select_prune(reg, keep=2)]
        self.assertEqual(sorted(gone), ["v1.5.1", "v1.5.2"])

    def test_prune_catalogs_by_creation_time(self):
        reg = [{"tag": f"catalog-{d}", "kind": "catalog", "created": d} for d in
               ("2026-11-01", "2026-11-03", "2026-11-02")]
        gone = [e["tag"] for e in gv.select_prune_catalogs(reg, keep=2)]
        self.assertEqual(gone, ["catalog-2026-11-01"])


if __name__ == "__main__":
    unittest.main()
