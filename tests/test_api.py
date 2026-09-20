from __future__ import annotations

import json
import unittest
from io import BytesIO
from unittest.mock import patch

from panemorph.api import HerdrClient, HerdrError


class HerdrClientTests(unittest.TestCase):
    class FakeSocket:
        def __init__(self, response_factory) -> None:
            self.response_factory = response_factory
            self.sent = b""

        def __enter__(self):
            return self

        def __exit__(self, *_args):
            return False

        def settimeout(self, _timeout):
            return None

        def connect(self, _path):
            return None

        def sendall(self, payload):
            self.sent = payload

        def makefile(self, _mode):
            request = json.loads(self.sent)
            response = self.response_factory(request)
            return BytesIO(json.dumps(response).encode() + b"\n")

    def test_round_trip_uses_newline_json_and_matching_id(self) -> None:
        fake = self.FakeSocket(
            lambda request: {
                    "id": request["id"],
                    "result": {"type": "pong", "method": request["method"]},
            }
        )
        with patch("panemorph.api.socket.socket", return_value=fake):
            result = HerdrClient("/fake/herdr.sock").request("ping")
        self.assertEqual(result, {"type": "pong", "method": "ping"})
        self.assertTrue(fake.sent.endswith(b"\n"))

    def test_error_response_raises(self) -> None:
        fake = self.FakeSocket(
            lambda request: {
                    "id": request["id"],
                    "error": {"code": "nope", "message": "not available"},
            }
        )
        with patch("panemorph.api.socket.socket", return_value=fake):
            with self.assertRaisesRegex(HerdrError, "not available"):
                HerdrClient("/fake/herdr.sock").request("missing")


if __name__ == "__main__":
    unittest.main()
