"""Checks that the ScreenScraper secrets are set and accepted, without printing them."""
import json
import os
import sys
import urllib.parse
import urllib.request

NAMES = {
    "SS_DEVID": "SCREENSCRAPER_DEV_ID",
    "SS_DEVPASSWORD": "SCREENSCRAPER_DEV_PASSWORD",
    "SS_SOFTNAME": "SCREENSCRAPER_SOFTNAME",
    "SS_USER": "SCREENSCRAPER_USER",
    "SS_PASSWORD": "SCREENSCRAPER_PASSWORD",
}
for env, secret in NAMES.items():
    print(f"{secret}: {'set' if os.environ.get(env) else 'MISSING'}")


def call(endpoint, **extra):
    q = {
        "devid": os.environ.get("SS_DEVID", ""),
        "devpassword": os.environ.get("SS_DEVPASSWORD", ""),
        "softname": os.environ.get("SS_SOFTNAME", ""),
        "output": "json",
        **extra,
    }
    url = f"https://api.screenscraper.fr/api2/{endpoint}?{urllib.parse.urlencode(q)}"
    try:
        with urllib.request.urlopen(url, timeout=30) as r:
            body = r.read().decode("utf-8", "replace")
            status = r.status
    except urllib.error.HTTPError as e:
        body, status = e.read().decode("utf-8", "replace"), e.code
    try:
        return status, json.loads(body)
    except ValueError:
        return status, body.strip()[:200]


ok = True
status, infra = call("ssinfraInfos.php")
print(f"developer login: HTTP {status}")
if isinstance(infra, dict):
    print("  accepted")
else:
    print(f"  answer: {infra}")
    ok = False

if os.environ.get("SS_USER"):
    status, user = call(
        "ssuserInfos.php",
        ssid=os.environ["SS_USER"],
        sspassword=os.environ.get("SS_PASSWORD", ""),
    )
    print(f"user login: HTTP {status}")
    if isinstance(user, dict):
        u = user.get("response", {}).get("ssuser", {})
        for k in ("niveau", "contribution", "maxthreads", "maxrequestsperday",
                  "requeststoday", "maxrequestskoperday", "requestskotoday"):
            print(f"  {k}: {u.get(k)}")
    else:
        print(f"  answer: {user}")
        ok = False

sys.exit(0 if ok else 1)
