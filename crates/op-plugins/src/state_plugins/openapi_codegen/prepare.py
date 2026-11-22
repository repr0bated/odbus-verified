#!/usr/bin/env python3
"""Turn a pinned upstream OpenAPI document into a JSON Schema bundle for typify.

Deterministic; for the selected operations:
  * every component schema reachable from the selected operations is copied,
    with `#/components/schemas/X` rewritten to `#/definitions/X` and
    documentation-only keys (`example(s)`, `xml`, `externalDocs`) dropped;
  * each operation's parameters (query + path) become `<Method>Input`, an
    object schema built only from the upstream parameter objects;
  * each operation's 200 JSON body becomes `<Method>Output`, a `$ref` alias of
    the upstream schema (component responses are resolved);
  * OpenAPI 3.0 `nullable: true` is rewritten to the JSON Schema `null` type;
  * string `format`s with no schemars impl in this workspace (`date-time`,
    `uuid`, ...) are kept as plain strings so upstream text round-trips
    byte-for-byte; the original format is preserved as `x-upstream-format`;
  * `pattern` is preserved as `x-upstream-pattern` (typify would otherwise
    pull in the `regress` crate, which this workspace does not carry).
  * an `anyOf`/`oneOf` of inline object variants discriminated by a
    single-value `enum` property (e.g. Prometheus `QueryData.resultType`) has
    each variant hoisted to its own definition `<Parent><Value>`, so typify
    names each variant's nested types distinctly (typify otherwise merges
    same-named inline item types across variants);
  * documented spec defects in `<plugin>/openapi/errata.json` are applied first, as
    JSON-pointer replacements guarded by the expected upstream value.
Also writes `<api>.methods.json` (method -> path, operationId, Input, Output).

usage: prepare.py <state_plugins/<plugin>> <out-dir>
(reads <plugin>/openapi/{select,errata}.json and the vendored spec beside them)
"""
import copy, json, os, sys

KEEP_FORMATS = {"int32", "int64", "uint32", "uint64", "float", "double"}

def load(path):
    with open(path) as f:
        if path.endswith((".yaml", ".yml")):
            import yaml  # PyYAML
            return yaml.safe_load(f)
        return json.load(f)

def camel(name):
    return "".join(p[:1].upper() + p[1:] for p in name.split("_"))

def resolve(doc, node):
    while isinstance(node, dict) and "$ref" in node and node["$ref"].startswith("#/components/") \
            and not node["$ref"].startswith("#/components/schemas/"):
        cur = doc
        for part in node["$ref"][2:].split("/"):
            cur = cur[part]
        node = cur
    return node

def normalize(node):
    if isinstance(node, list):
        return [normalize(n) for n in node]
    if not isinstance(node, dict):
        return node
    out = {}
    for k, v in node.items():
        if k == "$ref" and isinstance(v, str) and v.startswith("#/components/schemas/"):
            out[k] = "#/definitions/" + v.rsplit("/", 1)[1]
        elif k in ("example", "examples", "nullable", "discriminator", "xml", "externalDocs"):
            continue
        else:
            out[k] = normalize(v)
    if node.get("nullable") is True:
        t = out.get("type")
        if isinstance(t, str):
            out["type"] = [t, "null"]
        elif "$ref" in out or "allOf" in out or "oneOf" in out or "anyOf" in out:
            out = {"anyOf": [out, {"type": "null"}]}
    if isinstance(out.get("pattern"), str):
        out["x-upstream-pattern"] = out.pop("pattern")
    fmt = out.get("format")
    if fmt is not None and fmt not in KEEP_FORMATS:
        out.pop("format")
        out["x-upstream-format"] = fmt
    return out

def refs(node, acc):
    if isinstance(node, dict):
        r = node.get("$ref")
        if isinstance(r, str) and r.startswith("#/components/schemas/"):
            acc.add(r.rsplit("/", 1)[1])
        for v in node.values():
            refs(v, acc)
    elif isinstance(node, list):
        for v in node:
            refs(v, acc)

def pointer_get(doc, ptr):
    cur = doc
    for raw in ptr.lstrip("/").split("/"):
        key = raw.replace("~1", "/").replace("~0", "~")
        cur = cur[int(key)] if isinstance(cur, list) else cur[key]
    return cur

def pointer_set(doc, ptr, value):
    parts = ptr.lstrip("/").split("/")
    parent = pointer_get(doc, "/" + "/".join(parts[:-1])) if len(parts) > 1 else doc
    key = parts[-1].replace("~1", "/").replace("~0", "~")
    if isinstance(parent, list):
        parent[int(key)] = value
    else:
        parent[key] = value

def apply_errata(doc, path):
    if not os.path.exists(path):
        return
    for patch in json.load(open(path))["patches"]:
        current = pointer_get(doc, patch["pointer"])
        if current != patch["expect"]:
            sys.exit(f"erratum no longer applies at {patch['pointer']}: upstream changed; review and drop it")
        pointer_set(doc, patch["pointer"], copy.deepcopy(patch["value"]))

def discriminator_value(variant):
    if not isinstance(variant, dict) or "$ref" in variant or variant.get("type") != "object":
        return None
    for prop in variant.get("properties", {}).values():
        if isinstance(prop, dict) and isinstance(prop.get("enum"), list) and len(prop["enum"]) == 1:
            return str(prop["enum"][0])
    return None

def hoist_variants(defs):
    for name in sorted(list(defs)):
        schema = defs[name]
        for key in ("anyOf", "oneOf"):
            variants = schema.get(key) if isinstance(schema, dict) else None
            if not variants:
                continue
            values = [discriminator_value(v) for v in variants]
            if any(v is None for v in values) or len(set(values)) != len(values):
                continue
            for i, value in enumerate(values):
                vname = name + camel(value.replace("-", "_"))
                assert vname not in defs, f"name clash: {vname}"
                defs[vname] = variants[i]
                variants[i] = {"$ref": "#/definitions/" + vname}

def main():
    plugin_dir, out_dir = sys.argv[1:3]
    api = os.path.basename(os.path.abspath(plugin_dir))
    base = os.path.join(plugin_dir, "openapi")
    sel = json.load(open(os.path.join(base, "select.json")))
    doc = load(os.path.join(base, sel["spec"]))
    apply_errata(doc, os.path.join(base, "errata.json"))
    comps = doc["components"]["schemas"]
    defs, methods, roots = {}, {}, set(sel.get("extra_schemas", []))
    for name, m in sorted(sel["methods"].items()):
        op = doc["paths"][m["path"]][m["op"]]
        props, required = {}, []
        for p in op.get("parameters", []):
            p = resolve(doc, p)
            if p.get("in") not in ("query", "path"):
                continue
            s = copy.deepcopy(p.get("schema", {}))
            if p.get("description") and "description" not in s:
                s["description"] = p["description"]
            props[p["name"]] = s
            if p.get("required"):
                required.append(p["name"])
        inp = {"type": "object", "properties": props, "additionalProperties": False,
               "description": f"Parameters of `{m['op'].upper()} {m['path']}` "
                              f"(operationId `{op.get('operationId')}`)."}
        if required:
            inp["required"] = required
        resp = resolve(doc, op["responses"]["200"])
        body = (resp.get("content", {}).get("application/json", {}) or {}).get("schema")
        if body is None:
            body = resp.get("schema", {})  # tolerate swagger-style
        if not body:
            body = {"type": "object", "additionalProperties": True}
        out = copy.deepcopy(body)
        out.setdefault("description", resp.get("description") or
                       f"200 response of `{m['op'].upper()} {m['path']}`.")
        acc = set(); refs(inp, acc); refs(out, acc); roots |= acc
        defs[camel(name) + "Input"] = normalize(inp)
        defs[camel(name) + "Output"] = normalize(out)
        methods[name] = {"path": m["path"], "http_method": m["op"].upper(),
                         "operation_id": op.get("operationId"),
                         "input": camel(name) + "Input", "output": camel(name) + "Output"}
    seen, todo = set(), sorted(roots)
    while todo:
        n = todo.pop()
        if n in seen:
            continue
        seen.add(n)
        acc = set(); refs(comps[n], acc); todo.extend(sorted(acc - seen))
    for n in sorted(seen):
        assert n not in defs, f"name clash: {n}"
        defs[n] = normalize(comps[n])
    hoist_variants(defs)
    bundle = {"$schema": "http://json-schema.org/draft-07/schema#",
              "title": f"{api} selected API surface", "definitions": dict(sorted(defs.items()))}
    os.makedirs(out_dir, exist_ok=True)
    with open(os.path.join(out_dir, f"{api}.schema.json"), "w") as f:
        json.dump(bundle, f, indent=1, sort_keys=True); f.write("\n")
    with open(os.path.join(out_dir, f"{api}.methods.json"), "w") as f:
        json.dump(methods, f, indent=1, sort_keys=True); f.write("\n")
    print(f"{api}: {len(methods)} methods, {len(defs)} definitions")

main()
