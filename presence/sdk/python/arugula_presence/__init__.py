"""Sign an arugula.stream room token on your server (PROTOCOL.md, "Room
token"): an HS256 JWT with the claims in a fixed order and compact JSON,
so every arugula SDK makes the same bytes for the same inputs
(../vectors.json). Standard library only."""

import base64
import hashlib
import hmac
import json
import re
import time
from typing import Optional

__all__ = ["create_room_token"]

_ROOM = re.compile(r"\A[A-Za-z0-9_.:-]{1,128}\Z")
_SUB = re.compile(r"\A[A-Za-z0-9_.:@-]{1,128}\Z")
# Control characters (and the two line separators some JSON encoders
# escape) aren't names: refusing them keeps every SDK's JSON identical.
_CONTROL = re.compile("[\u0000-\u001f\u007f  ]")
_MAX_TTL = 86400
_HEADER = '{"alg":"HS256","typ":"JWT"}'


def _b64(data: bytes) -> str:
    return base64.urlsafe_b64encode(data).rstrip(b"=").decode("ascii")


def _str(s: str) -> str:
    return json.dumps(s, ensure_ascii=False)


def _byte(v) -> bool:
    return isinstance(v, int) and not isinstance(v, bool) and 0 <= v <= 255


def create_room_token(
    *,
    app: str,
    secret: str,
    room: str,
    user_id: str,
    name: str,
    look: Optional[dict] = None,
    ttl: int = 3600,
    now: Optional[int] = None,
) -> str:
    """A room token for `user_id` (called `name`) in `room`, signed with
    your app's secret. `look` is an optional {"character": n, "colour": n}.
    Raises ValueError on bad input."""
    if not isinstance(app, str) or not _ROOM.match(app):
        raise ValueError("app: your app's id, 1-128 of A-Z a-z 0-9 _ . : -")
    if not isinstance(secret, str) or not secret:
        raise ValueError("secret: your app's secret")
    if not isinstance(room, str) or not _ROOM.match(room):
        raise ValueError("room: 1-128 of A-Z a-z 0-9 _ . : -")
    if not isinstance(user_id, str) or not _SUB.match(user_id):
        raise ValueError("user_id: 1-128 of A-Z a-z 0-9 _ . : @ -")
    if not isinstance(name, str) or not name or len(name) > 80 or _CONTROL.search(name):
        raise ValueError("name: 1-80 characters, no control characters")
    if not isinstance(ttl, int) or isinstance(ttl, bool) or not 1 <= ttl <= _MAX_TTL:
        raise ValueError("ttl: 1-86400 seconds")
    if look is not None and not (isinstance(look, dict) and _byte(look.get("character")) and _byte(look.get("colour"))):
        raise ValueError("look: {character, colour}, each 0-255")
    at = int(time.time()) if now is None else now
    claims = '{"app":%s,"room":%s,"sub":%s,"name":%s,"exp":%d' % (_str(app), _str(room), _str(user_id), _str(name), at + ttl)
    if look is not None:
        claims += ',"look":{"character":%d,"colour":%d}' % (look["character"], look["colour"])
    claims += "}"
    body = _b64(_HEADER.encode()) + "." + _b64(claims.encode("utf-8"))
    sig = hmac.new(secret.encode("utf-8"), body.encode("ascii"), hashlib.sha256).digest()
    return body + "." + _b64(sig)
