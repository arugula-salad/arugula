"""Every vector in ../../vectors.json, byte for byte, and every invalid
input refused."""

import json
import pathlib
import time
import unittest

from arugula_presence import create_room_token

VECTORS = json.loads((pathlib.Path(__file__).resolve().parents[2] / "vectors.json").read_text(encoding="utf-8"))


def kwargs(i):
    k = dict(app=i["app"], secret=i["secret"], room=i["room"], user_id=i["user_id"], name=i["user_name"], now=i["now"])
    if i["look"] is not None:
        k["look"] = i["look"]
    if i["ttl"] is not None:
        k["ttl"] = i["ttl"]
    return k


class TokenTest(unittest.TestCase):
    def test_vectors(self):
        for v in VECTORS["valid"]:
            with self.subTest(v["name"]):
                self.assertEqual(create_room_token(**kwargs(v["input"])), v["token"])

    def test_invalid(self):
        for v in VECTORS["invalid"]:
            with self.subTest(v["name"]):
                with self.assertRaises(ValueError):
                    create_room_token(**kwargs(v["input"]))

    def test_now_defaults_to_the_clock(self):
        import base64

        t = create_room_token(app="acme", secret="s", room="r", user_id="u", name="A")
        p = t.split(".")[1]
        exp = json.loads(base64.urlsafe_b64decode(p + "=" * (-len(p) % 4)))["exp"]
        self.assertLessEqual(abs(exp - (int(time.time()) + 3600)), 2)


if __name__ == "__main__":
    unittest.main()
