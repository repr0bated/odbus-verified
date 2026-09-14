# Subid Taxonomy — GhostBridge Operational Routing

This document is the authoritative subid taxonomy for the GhostBridge host
consolidation. It is derived from `oscal-subids-report.md` and is used by the
Gemma routing brain to map subids → tags → OpenFlow flow rules + xray routing
rules.

## Dual-identifier model

- `uuid` is the immutable machine identity (OSCAL-style).
- `subid` is the stable human-readable operational taxonomy key.
- Compliance mappings (`control_refs`, `statement_refs`, `control_source`) live in
  metadata arrays, never inside the `subid` string.

## Canonical subid pattern

```
<category>.<component-type>.<subject>.<verb>[.<facet>][@vN]
```

- `category` — one of `src`, `prj`, `sch`, `mut`, `obs`, `evt`, `exp`.
- `component-type` — OSCAL vocabulary: `service`, `software`, `network`, etc.
- `subject` — lowercase hyphenated noun (e.g. `xray`, `cognitive-mcp`).
- `verb` — lowercase hyphenated action (e.g. `serve`, `route`, `monitor`).
- `facet` — optional qualifier for routing variants.
- `@vN` — optional version, default `@v1`.

## Routing tags

A routing tag is a short stable label used by both OpenFlow and xray. Each tag
is declared in the subid registry and maps to one or more subids.

Tags used by the host consolidation:

| tag | purpose | primary subid | backend |
|---|---|---|---|
| `xray-tls` | xray TLS ingress on 443 | `src.network.xray-serve@v1` | ingress |
| `xray-reality` | xray REALITY ingress on 8443 | `src.network.xray-serve@v1` | ingress |
| `grpc-bridge` | zeroclaw gateway on 127.0.0.1:8090 | `exp.service.zeroclaw-serve@v1` | gRPC |
| `cognitive-mcp` | cognitive MCP on 127.0.0.1:3003 | `exp.service.cognitive-mcp-serve@v1` | gRPC |
| `qdrant` | qdrant vector store on 127.0.0.1:6334 | `src.service.qdrant-serve@v1` | TCP |
| `netmaker-api` | Netmaker REST API on 127.0.0.1:28081 | `src.service.netmaker-api-serve@v1` | TCP |
| `netmaker-mq` | Netmaker message broker on 127.0.0.1:21883 | `src.service.netmaker-mq-serve@v1` | TCP |
| `netmaker-ui` | Netmaker dashboard on 127.0.0.1:28082 | `exp.service.netmaker-ui-serve@v1` | TCP |
| `nextdns` | local NextDNS resolver on 127.0.0.1:53 | `src.service.nextdns-serve@v1` | DNS redirect |

## Subid → tag → xray rule

Gemma reads each subid, extracts its tag, and emits an xray routing entry:

```json
{
  "tag": "qdrant",
  "subdomains": ["qdrant.ghostbridge.tech"],
  "outbound": "to-qdrant"
}
```

The shuttle merges these entries into `routing.rules` before the catch-all rule.

## Subid → tag → OpenFlow rule

Gemma also emits OpenFlow rules for the `ovsbr0` privacy bridge. The rules are
passed to the OpenFlow controller as structured match/action pairs. The exact
match syntax follows the controller's existing privacy-flow convention.

Example:

```json
{
  "tag": "netmaker-api",
  "match_fields": { "dl_type": "0x0800", "nw_proto": "6", "tp_dst": "28081" },
  "actions": ["output:netmaker-api"],
  "priority": 100
}
```

## Registry location

The canonical subid registry is `/etc/ghostbridge/subid-registry.json`.
Gemma watches this file (or receives it via D-Bus /dev/shm) and regenerates
routing artifacts on change.

## Rules

- `subid` is immutable per subject; material changes require a new `@vN`.
- `mut.*` records must carry `actor_id` and `capability_id`.
- `evt.*` records must carry `event_id` or `event_hash`.
- All records must carry a paired `uuid`.
