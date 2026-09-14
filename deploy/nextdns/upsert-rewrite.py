#!/usr/bin/env python3
"""Upsert a NextDNS DNS Rewrite entry.

Required environment:
  NEXTDNS_API_KEY - API key from https://my.nextdns.io/account

Optional environment:
  NEXTDNS_PROFILE - profile id, defaults to the current 3tched profile

Usage:
  NEXTDNS_API_KEY=... ./upsert-rewrite.py dashboard.3tched.com 10.0.0.1
"""

from __future__ import annotations

import json
import os
import sys
import urllib.error
import urllib.parse
import urllib.request


DEFAULT_PROFILE = "689ec7"
DEFAULT_DOMAIN = "dashboard.3tched.com"
DEFAULT_CONTENT = "10.0.0.1"


def request(method: str, url: str, api_key: str, payload: dict | None = None) -> dict:
    data = None
    headers = {
        "Accept": "application/json",
        "User-Agent": "operation-dbus-proto-deploy/1.0",
        "X-Api-Key": api_key,
    }

    if payload is not None:
        data = json.dumps(payload).encode("utf-8")
        headers["Content-Type"] = "application/json"

    req = urllib.request.Request(url, data=data, headers=headers, method=method)
    try:
        with urllib.request.urlopen(req, timeout=20) as response:
            body = response.read().decode("utf-8")
    except urllib.error.HTTPError as exc:
        detail = exc.read().decode("utf-8", errors="replace")
        raise SystemExit(f"{method} {url} failed: HTTP {exc.code}: {detail}") from exc

    if not body:
        return {}

    parsed = json.loads(body)
    if parsed.get("errors"):
        raise SystemExit(f"{method} {url} failed: {json.dumps(parsed['errors'])}")

    return parsed


def entry_id(entry: dict) -> str:
    for key in ("id", "name"):
        value = entry.get(key)
        if isinstance(value, str) and value:
            return value
    raise SystemExit(f"rewrite entry has no usable id/name: {entry!r}")


def main() -> int:
    api_key = os.environ.get("NEXTDNS_API_KEY")
    if not api_key:
        raise SystemExit("NEXTDNS_API_KEY is required")

    profile = os.environ.get("NEXTDNS_PROFILE", DEFAULT_PROFILE)
    domain = sys.argv[1] if len(sys.argv) > 1 else DEFAULT_DOMAIN
    content = sys.argv[2] if len(sys.argv) > 2 else DEFAULT_CONTENT

    profile_path = urllib.parse.quote(profile, safe="")
    base = f"https://api.nextdns.io/profiles/{profile_path}/rewrites"

    response = request("GET", base, api_key)
    rewrites = response.get("data", [])
    if not isinstance(rewrites, list):
        raise SystemExit(f"unexpected rewrites response: {response!r}")

    matches = [entry for entry in rewrites if entry.get("name") == domain]
    exact_match = any(entry.get("content") == content for entry in matches)

    for entry in matches:
        if entry.get("content") == content:
            continue
        item = urllib.parse.quote(entry_id(entry), safe="")
        request("DELETE", f"{base}/{item}", api_key)
        print(f"deleted stale rewrite {domain} -> {entry.get('content')}")

    if exact_match:
        print(f"rewrite already present: {domain} -> {content}")
        return 0

    request("POST", base, api_key, {"name": domain, "content": content})
    print(f"created rewrite: {domain} -> {content}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
