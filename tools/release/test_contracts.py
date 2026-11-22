"""Contract tests: every published file is accepted by the parser rules of each old release that reads it.

The rules are in tools/release/contracts/rules.py (taken from the tags v1.4.1, v1.5.0, v1.5.2, v1.5.3,
v1.5.4). The files are made in a temp folder by the publishers' own functions (publish_release,
publish_config, publish_hub), plus the made-up samples in tools/release/contracts/samples. Hermetic: no
network, no token, no game files.

    python3 -I -m unittest discover -s tools/release -p 'test_*.py'
"""

import copy
import json
import os
import shutil
import struct
import sys
import tempfile
import unittest
import zlib

HERE = os.path.dirname(os.path.abspath(__file__))
TOOLS = os.path.abspath(os.path.join(HERE, "..", ""))
GITVERSE = os.path.join(TOOLS, "gitverse")
CONTRACTS = os.path.join(HERE, "contracts")
SAMPLES = os.path.join(CONTRACTS, "samples")
for p in (CONTRACTS, GITVERSE):
    if p not in sys.path:
        sys.path.insert(0, p)

import rules  # noqa: E402
from rules import ContractError  # noqa: E402
import publish_config  # noqa: E402
import publish_hub  # noqa: E402
import publish_release  # noqa: E402


def sample(name):
    with open(os.path.join(SAMPLES, name), "rb") as f:
        return json.loads(f.read().decode("utf-8"))


def add_unknown(node):
    """Adds a field no version knows to every object, at every depth (serde ignores it)."""
    if isinstance(node, dict):
        for v in list(node.values()):
            add_unknown(v)
        node["future_field"] = {"x": [1, "two", None]}
    elif isinstance(node, list):
        for v in node:
            add_unknown(v)
    return node


def make_png(width, height):
    """A tiny valid PNG (RGBA, 8 bit) of the given size: its pixels are made up."""
    def chunk(kind, data):
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data) & 0xFFFFFFFF)
    ihdr = struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0)
    row = b"\x00" + b"\x00\x00\x00\x00" * width
    idat = zlib.compress(row * height)
    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", ihdr) + chunk(b"IDAT", idat) + chunk(b"IEND", b"")


def run(check, data):
    check(data)


class SamplesTest(unittest.TestCase):
    """The made-up samples are accepted by every rule of every version that reads them."""

    def test_every_sample_passes_each_version_that_reads_it(self):
        for file_name, by_version in rules.RULES.items():
            data = sample(file_name)
            for version, check in by_version.items():
                with self.subTest(file=file_name, version=version):
                    check(copy.deepcopy(data))

    def test_every_sample_file_is_listed_in_the_rules(self):
        names = sorted(n for n in os.listdir(SAMPLES) if n.endswith(".json"))
        self.assertEqual(names, sorted(rules.RULES), "a published format has no rule, or a rule has no sample")


class PublisherOutputTest(unittest.TestCase):
    """The files the publishers write, made in a temp folder, are accepted by the old rules."""

    def setUp(self):
        self.tmp = tempfile.mkdtemp(prefix="razdor-contracts-")

    def tearDown(self):
        shutil.rmtree(self.tmp, ignore_errors=True)

    def write(self, name, data):
        path = os.path.join(self.tmp, name)
        with open(path, "wb") as f:
            f.write(data)
        with open(path, "rb") as f:
            return f.read()

    def test_feed_from_publish_release_is_accepted_by_1_4_1_and_1_5_2_to_1_5_4(self):
        groups = [{"prog_name": "razdor-9.9.2-linux.zip", "inner": "razdor", "zip": "razdor-9.9.2-linux.zip"}]
        entry = publish_release.feed_entry(
            "v9.9.2", "9.9.2", False, "Sample body.", "https://sample.invalid/releases/v9.9.2", groups, {},
            {"razdor-9.9.2-linux.zip": "https://sample.invalid/dl/razdor-9.9.2-linux.zip"},
            "https://sample.invalid/dl/SHA256SUMS")
        feed = publish_release.merge_feed(sample("releases.json")[1:], entry)
        raw = self.write("releases.json", publish_release.feed_bytes(feed))
        data = json.loads(raw.decode("utf-8"))
        for version in ("v1.4.1", "v1.5.0", "v1.5.2", "v1.5.3", "v1.5.4"):
            with self.subTest(version=version):
                rules.RULES["releases.json"][version](copy.deepcopy(data))

    def test_config_from_publish_config_is_accepted_by_1_5_2_to_1_5_4(self):
        cfg = sample("config.json")
        cfg.pop("note")
        raw = self.write("config.json", json.dumps(cfg).encode("utf-8"))
        checked = publish_config.check(raw)
        self.assertEqual(checked["schema"], 1)
        for version in ("v1.5.2", "v1.5.3", "v1.5.4"):
            with self.subTest(version=version):
                rules.RULES["config.json"][version](json.loads(raw.decode("utf-8")))

    def test_index_v2_from_publish_hub_is_accepted_by_1_5_2_to_1_5_4(self):
        cat = sample("index.json")
        cat["entries"][0]["files"][0]["path"] = "Sample.DTm"
        cat["entries"][0]["preview"] = {"path": "sample-map.png", "name": "sample-map.png", "size": 68,
                                        "sha256": "b" * 64}
        cat["ad"] = {"id": "sample-ad", "version": 1, "image": "sample-ad.png", "size": 68, "sha256": "b" * 64,
                     "url": "https://sample.invalid/ad", "caption_ru": "Образец", "caption_en": "Sample"}
        cat["game"] = {"url": "https://sample.invalid/game.zip", "mirrors": [], "size": 1024, "sha256": "a" * 64,
                       "name": "sample-game.zip"}
        links = {"Sample.DTm": {"url": "https://sample.invalid/hub/0001", "size": 90, "sha256": "d" * 64}}
        game = publish_hub.zipped_game("files/" + "a" * 64 + ".dat")
        idx = publish_hub.gitverse_index(cat, links, game)
        raw = self.write("index-v2.json", (json.dumps(idx, ensure_ascii=False, indent=2) + "\n").encode("utf-8"))
        data = json.loads(raw.decode("utf-8"))
        for version in ("v1.5.2", "v1.5.3", "v1.5.4"):
            with self.subTest(version=version):
                rules.RULES["index-v2.json"][version](copy.deepcopy(data))
        # the per-file mirrors are the GitHub copy of the file: an unknown field to 1.5.2 (ignored)
        self.assertIn("mirrors", data["entries"][0]["files"][0])

    def test_sums_file_and_previews(self):
        text = publish_release.sums_text({"razdor-9.9.2-linux.zip": "a" * 64, "SHA256SUMS.txt": "b" * 64})
        rules.check_sums(text)
        rules.check_preview_png(make_png(480, 270))
        rules.check_preview_png(make_png(1, 1))


class MutationTest(unittest.TestCase):
    """Each rule refuses what the program refuses: one bad change to a good sample, one refusal."""

    def assertRefused(self, check, data, why):
        with self.assertRaises(ContractError, msg=why):
            check(data)

    def feed(self):
        return copy.deepcopy(sample("releases.json"))

    def test_feed_refusals(self):
        bad = self.feed()
        bad[0]["draft"] = None
        self.assertRefused(rules.check_feed_v152, bad, "a null draft (bool, not an Option)")
        bad = self.feed()
        bad[0]["html_url"] = 5
        self.assertRefused(rules.check_feed_v152, bad, "html_url not a string")
        bad = self.feed()
        del bad[0]["assets"][0]["browser_download_url"]
        self.assertRefused(rules.check_feed_v152, bad, "an asset without its address")
        bad = self.feed()
        bad[0]["assets"][0]["archive"] = 1
        self.assertRefused(rules.check_feed_v152, bad, "an archive that is not a string")

    def test_null_member_is_an_option_and_accepted(self):
        good = self.feed()
        good[0]["assets"].append({"name": "razdor", "browser_download_url": "https://sample.invalid/dl/razdor",
                                  "archive": None, "member": None})
        rules.check_feed_v152(good)

    def test_1_4_1_needs_the_sums_name_exactly(self):
        bad = self.feed()
        bad[0]["assets"] = [a for a in bad[0]["assets"] if a["name"] != "SHA256SUMS"]
        self.assertRefused(rules.check_feed_v141, bad, "v1.4.1 wants SHA256SUMS exactly")
        rules.check_feed_v152(bad)  # v1.5.2 takes SHA256SUMS.txt too

    def test_config_refusals(self):
        def cfg():
            return copy.deepcopy(sample("config.json"))
        bad = cfg()
        bad["schema"] = "1"
        self.assertRefused(rules.check_config_v152, bad, "schema as a string")
        bad = cfg()
        bad["schema"] = 2
        self.assertRefused(rules.check_config_v152, bad, "schema 2")
        bad = cfg()
        bad["schema"] = True
        self.assertRefused(rules.check_config_v152, bad, "schema as a bool")
        bad = cfg()
        bad["update_feed"] = None
        self.assertRefused(rules.check_config_v152, bad, "a null list")
        bad = cfg()
        bad["hub"] = ["http://insecure.invalid/hub"]
        self.assertRefused(rules.check_config_v152, bad, "an http address")
        bad = cfg()
        bad["game"]["size"] = 0
        self.assertRefused(rules.check_config_v152, bad, "game size 0")
        bad = cfg()
        bad["game"]["size"] = rules.MAX_ARCHIVE_DOWNLOAD + 1
        self.assertRefused(rules.check_config_v152, bad, "game size over the limit")
        bad = cfg()
        bad["game"]["sha256"] = "a" * 63
        self.assertRefused(rules.check_config_v152, bad, "a 63-digit sha")
        bad = cfg()
        bad["game"]["name"] = "dir/game.zip"
        self.assertRefused(rules.check_config_v152, bad, "a name with a folder")

    def catalog(self):
        return copy.deepcopy(sample("index-v2.json"))

    def test_catalog_refusals(self):
        bad = self.catalog()
        bad["format"] = 2
        self.assertRefused(rules.check_catalog_v152, bad, "format 2")
        bad = self.catalog()
        bad["entries"][0]["kind"] = "zip"
        self.assertRefused(rules.check_catalog_v152, bad, "an unknown kind")
        bad = self.catalog()
        bad["entries"][0]["status"] = "great"
        self.assertRefused(rules.check_catalog_v152, bad, "an unknown status")
        bad = self.catalog()
        del bad["entries"][0]["files"]
        self.assertRefused(rules.check_catalog_v152, bad, "an entry without files")
        bad = self.catalog()
        bad["entries"][0]["files"][0]["sha256"] = "c" * 63
        self.assertRefused(rules.check_catalog_v152, bad, "a short sha")
        bad = self.catalog()
        del bad["entries"][0]["files"][0]["member"]
        self.assertRefused(rules.check_catalog_v152, bad, "a wrapped file without its member")
        bad = self.catalog()
        bad["entries"][0]["files"][0]["wrapped"] = "tar"
        self.assertRefused(rules.check_catalog_v152, bad, "a wrapper that is not a zip")
        bad = self.catalog()
        bad["entries"][0]["files"][0]["archive"] = True
        self.assertRefused(rules.check_catalog_v152, bad, "a map as an archive")
        bad = self.catalog()
        bad["entries"][0]["id"] = "bad id"
        self.assertRefused(rules.check_catalog_v152, bad, "an id with a space")
        bad = self.catalog()
        bad["entries"][0]["rating"] = {"up": 1}
        self.assertRefused(rules.check_catalog_v152, bad, "a rating without down")
        bad = self.catalog()
        bad["entries"][0]["preview"]["path"] = "/abs/sample.png"
        self.assertRefused(rules.check_catalog_v152, bad, "an absolute preview path")
        bad = self.catalog()
        bad["game"]["url"] = "http://sample.invalid/game.zip"
        self.assertRefused(rules.check_catalog_v152, bad, "a game over http")
        bad = self.catalog()
        bad["game"]["size"] = rules.MAX_ARCHIVE_DOWNLOAD + 1
        self.assertRefused(rules.check_catalog_v152, bad, "a game over the size limit")
        bad = self.catalog()
        bad["ad"]["size"] = rules.MAX_AD_BYTES + 1
        self.assertRefused(rules.check_catalog_v152, bad, "an ad over 2 MB")
        bad = self.catalog()
        bad["ad"]["caption_ru"] = "x" * (rules.MAX_CAPTION + 1)
        self.assertRefused(rules.check_catalog_v152, bad, "a caption over 200")
        bad = self.catalog()
        bad["ad"]["image"] = "http://sample.invalid/ad.png"
        self.assertRefused(rules.check_catalog_v152, bad, "an ad picture over http")

    def test_flat_catalog_has_no_wrapped_files_for_1_4_1(self):
        flat = copy.deepcopy(sample("index.json"))
        rules.check_catalog_v141(copy.deepcopy(flat))
        flat["entries"][0]["files"][0]["wrapped"] = "zip"
        flat["entries"][0]["files"][0]["member"] = "Sample.DTm"
        self.assertRefused(rules.check_catalog_v141, flat, "1.4.1 never reads wrapped files")

    def test_sums_and_previews_refusals(self):
        self.assertRefused(rules.check_sums, "a" * 64 + " name\n", "one space, not two")
        self.assertRefused(rules.check_sums, "", "no lines")
        self.assertRefused(rules.check_preview_png, make_png(481, 270), "a preview wider than 480")
        self.assertRefused(rules.check_preview_png, make_png(480, 271), "a preview taller than 270")
        self.assertRefused(rules.check_preview_png, b"not a png at all", "not a PNG")


class UnknownFieldsTest(unittest.TestCase):
    """No version denies unknown fields: a field added to every object is ignored by every rule."""

    def test_every_rule_ignores_unknown_fields(self):
        for file_name, by_version in rules.RULES.items():
            for version, check in by_version.items():
                with self.subTest(file=file_name, version=version):
                    check(add_unknown(copy.deepcopy(sample(file_name))))


class RuleFieldsTest(unittest.TestCase):
    """The strict cases: fields that must be present, and nulls where serde takes none."""

    def test_missing_required_catalog_field_is_refused(self):
        bad = copy.deepcopy(sample("index-v2.json"))
        del bad["entries"][0]["title"]
        with self.assertRaises(ContractError):
            rules.check_catalog_v152(bad)

    def test_missing_optional_catalog_field_is_accepted(self):
        good = copy.deepcopy(sample("index-v2.json"))
        for key in ("description", "about", "languages", "status", "status_note", "rating", "preview", "requires"):
            good["entries"][0].pop(key, None)
        rules.check_catalog_v152(good)


if __name__ == "__main__":
    unittest.main()
