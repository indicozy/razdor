"""The API client and anonymous downloads, against a local HTTP server (no network)."""

import hashlib
import http.server
import json
import os
import re
import sys
import tempfile
import threading
import unittest
import unittest.mock

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)

import gitverse as gv  # noqa: E402
from testkit import HermeticTestCase  # noqa: E402


class FakeHost(http.server.BaseHTTPRequestHandler):
    """Serves /file (Range aware; a short part once when `short` is set), /norange (ignores Range),
    and an API: POST …/assets records the multipart body; POST …/releases answers 429 once."""

    protocol_version = "HTTP/1.1"
    data = b""
    short_once = set()
    seen = []
    release_429 = [1]

    def log_message(self, *a):
        pass

    def _send(self, code, body=b"", headers=None, short=False):
        self.send_response(code)
        for k, v in (headers or {}).items():
            self.send_header(k, v)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        if short:
            self.wfile.write(body[: len(body) // 2])
            self.close_connection = True
        else:
            self.wfile.write(body)

    def do_GET(self):
        rng = self.headers.get("Range")
        if self.path.startswith("/norange"):
            return self._send(200, FakeHost.data)
        if rng:
            a, b = re.match(r"bytes=(\d+)-(\d+)", rng).groups()
            a, b = int(a), int(b)
            key = (a, b)
            short = key in FakeHost.short_once
            FakeHost.short_once.discard(key)
            body = FakeHost.data[a:b + 1]
            return self._send(206, body, {"Content-Range": f"bytes {a}-{b}/{len(FakeHost.data)}"}, short=short)
        return self._send(200, FakeHost.data)

    def do_POST(self):
        n = int(self.headers.get("Content-Length", "0"))
        body = self.rfile.read(n)
        FakeHost.seen.append({"path": self.path, "headers": dict(self.headers), "body": body})
        if self.path.startswith("/repos/o/r/releases") and "/assets" not in self.path:
            if FakeHost.release_429[0] > 0:
                FakeHost.release_429[0] -= 1
                return self._send(429, b'{"error":"slow down"}', {"Retry-After": "0"})
            return self._send(201, json.dumps({"id": 7, "tag_name": "v1"}).encode(),
                              {"Content-Type": "application/json"})
        return self._send(201, json.dumps({"id": 9, "browser_download_url": "http://127.0.0.1/x"}).encode(),
                          {"Content-Type": "application/json"})


class Server:
    def __enter__(self):
        self.httpd = http.server.ThreadingHTTPServer(("127.0.0.1", 0), FakeHost)
        self.base = f"http://127.0.0.1:{self.httpd.server_address[1]}"
        self.t = threading.Thread(target=self.httpd.serve_forever, daemon=True)
        self.t.start()
        return self

    def __exit__(self, *exc):
        self.httpd.shutdown()
        self.httpd.server_close()


def no_sleep(_):
    return None


class MultipartTests(HermeticTestCase):
    def test_body_has_the_attachment_field_and_streams_like_bytes(self):
        with tempfile.TemporaryDirectory() as d:
            path = os.path.join(d, "razdor-1.0.0.apk")
            payload = os.urandom(300_000)
            with open(path, "wb") as f:
                f.write(payload)
            mp = gv.Multipart([("attachment", "razdor-1.0.0.apk", "application/vnd.android.package-archive", path)])
            whole = mp.to_bytes()
            self.assertEqual(len(whole), mp.length)
            self.assertIn(b'name="attachment"; filename="razdor-1.0.0.apk"', whole)
            self.assertIn(payload, whole)
            self.assertTrue(whole.endswith(f"--{mp.boundary}--\r\n".encode()))
            self.assertEqual(mp.content_type, f"multipart/form-data; boundary={mp.boundary}")
            # streamed in small reads, the same bytes come out
            mp2 = gv.Multipart([("attachment", "razdor-1.0.0.apk", "application/octet-stream", path)])
            out = b""
            while True:
                block = mp2.read(7000)
                if not block:
                    break
                out += block
            self.assertEqual(len(out), mp2.length)
            self.assertIn(payload, out)

    def test_name_rules(self):
        self.assertIsNone(gv.name_problem("razdor-1.5.2-linux.zip"))
        self.assertIsNone(gv.name_problem("razdor-1.5.2.apk"))
        self.assertIsNone(gv.name_problem("aaaaaaaaaaaaaaaa.png"))
        self.assertIsNone(gv.name_problem("Razdor-1.5.2-windows.zip"))
        self.assertIsNotNone(gv.name_problem("0123456789abcdef.dtm"))
        self.assertIsNotNone(gv.name_problem("razdor-1.5.2.ipa"))
        self.assertIsNotNone(gv.name_problem("razdor.webp"))
        self.assertIsNotNone(gv.name_problem("Razdor.exe"))
        self.assertIsNotNone(gv.name_problem("razdor.bin"))
        self.assertIsNotNone(gv.name_problem("razdor"))
        self.assertIsNotNone(gv.name_problem("with space.zip"))
        self.assertIsNotNone(gv.name_problem("a" * 252 + ".zip"))
        self.assertIsNone(gv.name_problem("a" * 250 + ".zip"))
        self.assertIsNotNone(gv.name_problem("Пример.zip"))

    def test_env_value_strips_quotes_and_reads_the_named_key(self):
        with tempfile.TemporaryDirectory() as d:
            p = os.path.join(d, ".env")
            with open(p, "w", encoding="utf-8") as f:
                f.write('# c\nRAZDOR_DT_DIR="/x/y"\nGITVERSE_TOKEN = "tok en"\nexport OTHER=1\n')
            self.assertEqual(gv.env_value("GITVERSE_TOKEN", p), "tok en")
            self.assertEqual(gv.env_value("OTHER", p), "1")
            self.assertEqual(gv.env_value("MISSING", p), "")
            self.assertEqual(gv.env_value("X", os.path.join(d, "nope")), "")


class RetryTests(HermeticTestCase):
    def test_retries_429_and_5xx_then_succeeds(self):
        calls = []

        def fn():
            calls.append(1)
            if len(calls) < 3:
                raise gv.ApiError(503, "busy")
            return "ok"

        self.assertEqual(gv.retrying("t", fn, sleep=no_sleep), "ok")
        self.assertEqual(len(calls), 3)

    def test_404_and_400_are_not_retried(self):
        for status in (400, 404):
            calls = []

            def fn(status=status):
                calls.append(1)
                raise gv.ApiError(status, "no")

            with self.assertRaises(gv.ApiError):
                gv.retrying("t", fn, sleep=no_sleep)
            self.assertEqual(len(calls), 1)

    def test_connection_errors_are_retried(self):
        calls = []

        def fn():
            calls.append(1)
            if len(calls) == 1:
                raise ConnectionResetError("reset")
            return 1

        self.assertEqual(gv.retrying("t", fn, sleep=no_sleep), 1)


class ClientTests(HermeticTestCase):
    def test_upload_sends_token_accept_and_the_multipart_body(self):
        FakeHost.seen = []
        with tempfile.TemporaryDirectory() as d, Server() as s:
            path = os.path.join(d, "razdor-1.0.0-linux.zip")
            with open(path, "wb") as f:
                f.write(b"zip bytes")
            c = gv.Client(token="SECRET-TOKEN", api=s.base)
            asset = c.upload_asset("o", "r", 5, "razdor-1.0.0-linux.zip", path)
            self.assertEqual(asset["id"], 9)
            req = FakeHost.seen[-1]
            self.assertEqual(req["path"], "/repos/o/r/releases/5/assets?name=razdor-1.0.0-linux.zip")
            self.assertEqual(req["headers"]["Authorization"], "Bearer SECRET-TOKEN")
            self.assertEqual(req["headers"]["Accept"], gv.ACCEPT)
            self.assertTrue(req["headers"]["Content-Type"].startswith("multipart/form-data; boundary="))
            self.assertIn(b'name="attachment"; filename="razdor-1.0.0-linux.zip"', req["body"])
            self.assertIn(b"zip bytes", req["body"])
            self.assertEqual(int(req["headers"]["Content-Length"]), len(req["body"]))

    def test_create_release_retries_a_429(self):
        FakeHost.release_429 = [1]
        with Server() as s:
            c = gv.Client(token="t", api=s.base)
            rel = c.create_release("o", "r", "v1", "main", "name", "body", False)
            self.assertEqual(rel["id"], 7)

    def test_upload_refuses_a_bad_name_before_sending(self):
        c = gv.Client(token="t", api="http://127.0.0.1:9")
        with self.assertRaises(gv.GitVerseError):
            c.upload_asset("o", "r", 1, "Razdor.exe", __file__)


class DownloadTests(HermeticTestCase):
    def test_fetch_exact_in_parts_checks_sha(self):
        data = os.urandom(3 * gv.PART_MIN + 17)
        FakeHost.data = data
        FakeHost.short_once = set()
        with Server() as s:
            gv.fetch_exact(s.base + "/file", len(data), hashlib.sha256(data).hexdigest())

    def test_a_short_part_is_retried(self):
        data = os.urandom(2 * gv.PART_MIN)
        FakeHost.data = data
        step = (len(data) + 1) // 2
        FakeHost.short_once = {(0, step - 1)}
        with Server() as s, unittest.mock.patch.object(gv.time, "sleep", no_sleep):
            gv.fetch_exact(s.base + "/file", len(data), hashlib.sha256(data).hexdigest())
        self.assertEqual(FakeHost.short_once, set())

    def test_a_wrong_sha_is_refused(self):
        data = b"abc" * 1000
        FakeHost.data = data
        FakeHost.short_once = set()
        with Server() as s:
            with self.assertRaises(gv.VerifyError):
                gv.fetch_exact(s.base + "/file", len(data), "0" * 64)

    def test_a_server_ignoring_range_is_accepted(self):
        data = os.urandom(5 * gv.PART_MIN)
        FakeHost.data = data
        with Server() as s:
            gv.fetch_exact(s.base + "/norange", len(data), hashlib.sha256(data).hexdigest())

    def test_small_and_empty_files(self):
        FakeHost.data = b"hello"
        FakeHost.short_once = set()
        with Server() as s:
            gv.fetch_exact(s.base + "/file", 5, hashlib.sha256(b"hello").hexdigest())
            FakeHost.data = b""
            gv.fetch_exact(s.base + "/file", 0, hashlib.sha256(b"").hexdigest())


class UploadRetryTests(HermeticTestCase):
    """An upload answered 400 VALIDATION_ERROR is tried again twice (2 s, 4 s), and no other status is."""

    def client(self, answers):
        client = gv.Client(token="t", api="http://127.0.0.1:9")
        calls = []
        waits = []

        def _json(method, path, **kw):
            calls.append(path)
            answer = answers[len(calls) - 1]
            if isinstance(answer, Exception):
                raise answer
            return answer

        client._json = _json
        client.sleep = lambda s: waits.append(s)
        return client, calls, waits

    def test_a_400_twice_then_the_upload_goes_through(self):
        bad = gv.ApiError(400, '{"error": {"code": "VALIDATION_ERROR"}}')
        ok = {"id": 9, "browser_download_url": "https://gitverse.ru/api/attachments/x"}
        client, calls, waits = self.client([bad, bad, ok])
        self.assertEqual(client.upload_asset("o", "r", 7, "razdor-test.zip", "/nonexistent"), ok)
        self.assertEqual(len(calls), 3)
        self.assertEqual(waits, [2, 4])

    def test_a_third_400_fails_the_upload(self):
        bad = gv.ApiError(400, "VALIDATION_ERROR")
        client, calls, waits = self.client([bad, bad, bad])
        with self.assertRaises(gv.ApiError) as cm:
            client.upload_asset("o", "r", 7, "razdor-test.zip", "/nonexistent")
        self.assertEqual(cm.exception.status, 400)
        self.assertEqual(len(calls), 3)
        self.assertEqual(waits, [2, 4])

    def test_other_statuses_are_not_retried_by_the_upload(self):
        for status in (404, 429, 500):
            client, calls, waits = self.client([gv.ApiError(status, "x")])
            with self.assertRaises(gv.ApiError):
                client.upload_asset("o", "r", 7, "razdor-test.zip", "/nonexistent")
            self.assertEqual(len(calls), 1, f"HTTP {status} is not retried here")
            self.assertEqual(waits, [])


if __name__ == "__main__":
    unittest.main()
