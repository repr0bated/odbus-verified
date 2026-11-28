#!/usr/bin/env python3
"""Normalise grafana.com dashboard exports for file provisioning.

Replaces ${DS_*}/${ds_prometheus} datasource inputs with the provisioned
Prometheus uid, drops panels that need a non-Prometheus datasource (e.g. Loki),
and pins a stable uid.  Usage: prep-dashboards.py SRC.json DST.json UID
"""
import json, re, sys

src, dst, uid = sys.argv[1:4]
d = json.load(open(src))
inputs = {i["name"]: i.get("pluginId") for i in d.pop("__inputs", [])}
d.pop("__requires", None)
d.pop("__elements", None)
drop = {n for n, p in inputs.items() if p and p != "prometheus"}
keep = {n for n in inputs if n not in drop}

def uses_dropped(obj):
    s = json.dumps(obj)
    return any("${%s}" % n in s for n in drop)

if "panels" in d:
    d["panels"] = [p for p in d["panels"] if not uses_dropped(p)]
    for p in d["panels"]:
        if "panels" in p:
            p["panels"] = [q for q in p["panels"] if not uses_dropped(q)]
if "templating" in d:
    d["templating"]["list"] = [v for v in d["templating"]["list"] if not uses_dropped(v)]

s = json.dumps(d)
for n in keep:
    s = s.replace("${%s}" % n, "prometheus")
d = json.loads(s)
d["id"] = None
d["uid"] = uid
json.dump(d, open(dst, "w"), indent=1)
left = re.findall(r"\$\{DS_[A-Z_]+\}", json.dumps(d))
print(dst, d.get("title"), "panels:", len(d.get("panels", [])), "unresolved:", sorted(set(left)))
