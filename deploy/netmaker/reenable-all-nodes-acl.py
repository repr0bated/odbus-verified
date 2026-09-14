#!/usr/bin/env python3
"""Re-enable Netmaker default ACL 3tched.all-nodes via local API.

Run on the VPS host (where Incus NetMaker is local):

  python3 deploy/netmaker/reenable-all-nodes-acl.py

Or:

  sudo python3 /path/to/reenable-all-nodes-acl.py
"""

from __future__ import annotations

import json
import os
import urllib.error
import urllib.request


def master_key() -> str:
    mk = os.environ.get("NETMAKER_MASTER_KEY") or os.environ.get("MASTER_KEY")
    if mk:
        return mk.strip()
    stream = os.popen(
        "sudo incus exec NetMaker -- grep ^MASTER_KEY= /etc/netmaker/netmaker.env | cut -d= -f2-"
    )
    mk = stream.read().strip()
    if not mk:
        raise SystemExit("MASTER_KEY not found (set NETMAKER_MASTER_KEY or check NetMaker env)")
    return mk


def main() -> None:
    api = os.environ.get("NETMAKER_API_BASE", "http://127.0.0.1:8081").rstrip("/")
    body = {
        "id": "3tched.all-nodes",
        "name": "All Nodes",
        "network_id": "3tched",
        "policy_type": "device-policy",
        "src_type": [{"id": "tag", "value": "*"}],
        "dst_type": [{"id": "tag", "value": "*"}],
        "protocol": "all",
        "type": "Any",
        "ports": [],
        "allowed_traffic_direction": 1,
        "enabled": True,
        "default": True,
    }
    req = urllib.request.Request(
        f"{api}/api/v1/acls",
        data=json.dumps(body).encode(),
        headers={
            "Authorization": "Bearer " + master_key(),
            "Content-Type": "application/json",
        },
        method="PUT",
    )
    try:
        with urllib.request.urlopen(req) as resp:
            print(resp.read().decode())
    except urllib.error.HTTPError as e:
        print(e.read().decode())
        raise SystemExit(e.code) from e


if __name__ == "__main__":
    main()
