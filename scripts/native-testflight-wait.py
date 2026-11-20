#!/usr/bin/env python3
"""Waits for App Store Connect to process the newest build of an app uploaded
since a time, for scripts/native-testflight. The API key's .p8 comes on stdin,
its ids in ASC_KEY_ID and ASC_ISSUER_ID. Exits 0 once TestFlight has it (VALID).
    native-testflight-wait.py BUNDLE_ID SINCE_UTC"""

import json
import os
import sys
import time
import urllib.request
from datetime import datetime

import jwt

bundle, since = sys.argv[1], datetime.fromisoformat(sys.argv[2])
key = sys.stdin.read()


def get(path):
    now = int(time.time())
    claims = {"iss": os.environ["ASC_ISSUER_ID"], "iat": now, "exp": now + 600, "aud": "appstoreconnect-v1"}
    token = jwt.encode(claims, key, algorithm="ES256", headers={"kid": os.environ["ASC_KEY_ID"], "typ": "JWT"})
    req = urllib.request.Request(
        "https://api.appstoreconnect.apple.com" + path, headers={"Authorization": "Bearer " + token}
    )
    with urllib.request.urlopen(req) as r:
        return json.load(r)


apps = get(f"/v1/apps?filter[bundleId]={bundle}")["data"]
if not apps:
    sys.exit(f"App Store Connect has no app for {bundle}")
app = apps[0]["id"]
for _ in range(90):
    builds = get(f"/v1/builds?filter[app]={app}&sort=-uploadedDate&limit=1")["data"]
    b = builds[0]["attributes"] if builds else None
    if b and datetime.fromisoformat(b["uploadedDate"]) >= since:
        state = b["processingState"]
        print(f"build {b['version']}: {state}", flush=True)
        if state != "PROCESSING":
            sys.exit(0 if state == "VALID" else 1)
    else:
        print("waiting for App Store Connect to list the upload", flush=True)
    time.sleep(20)
sys.exit("still processing after 30 minutes: look in App Store Connect")
