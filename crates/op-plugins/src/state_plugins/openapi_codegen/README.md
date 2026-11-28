# Generated upstream API contracts (Prometheus, Grafana)

The `prometheus` and `grafana` state plugins expose read-only methods whose
input and output schemas are **generated** from the projects' own pinned
OpenAPI documents. Nothing in `state_plugins/{prometheus,grafana}.rs` or their `methods.rs`
restates an API shape; they only bind generated types to methods.

Per-plugin layout (same shape as `mail_server.rs` + `mail_server/`):

```
state_plugins/<plugin>.rs              ENTRY (identity, state) / BODY (StatePlugin) / EXIT (PluginSchema, inventory)
state_plugins/<plugin>/methods.rs      endpoint, bounds, one dispatch arm per generated operation, current_state
state_plugins/<plugin>/api_types.rs    @generated types + OPERATIONS table (do not edit)
state_plugins/<plugin>/native_tests.rs schema, fixture round-trip, limit and dispatch tests
state_plugins/<plugin>/openapi/        vendored upstream spec, source.json, select.json, errata.json
state_plugins/<plugin>/fixtures/       live responses captured from the installed server
state_plugins/openapi_http.rs          shared bounded HTTP caller (like plugin_scaffold_helpers.rs)
state_plugins/openapi_codegen/         this generator
```

| api | upstream file (vendored here) | tag / commit | sha256 | license |
|---|---|---|---|---|
| prometheus | [`web/api/v1/testdata/openapi_3.1_golden.yaml`](https://github.com/prometheus/prometheus/blob/v3.16.0/web/api/v1/testdata/openapi_3.1_golden.yaml) → `prometheus/openapi/openapi-3.1.yaml` | v3.16.0 / e2e28adeee2c5883d000fc82aca6008cfbf7e96d | f52088b9776010d49ad1ef85000198d39d69fdcc3d1f8f944eb46c5a4781b8b7 | Apache-2.0 |
| grafana | [`public/openapi3.json`](https://github.com/grafana/grafana/blob/v13.3.1/public/openapi3.json) → `grafana/openapi/openapi3.json` | v13.3.1 / 5db6ba887630c7fa12e5bfd063f2fa854025468c | upstream 89ff3e97e4d0e47ddd5a0bc757f2be2c909e655d275486a776a85fbde44dc661; vendored 33bc1e11a1231636676c1855244c8af55f2ab94e98b39fa1eb508563ba993d3e (one example value redacted) | AGPL-3.0-only |

Both are official. The vendored Grafana file differs from upstream in one
documentation `example` value (`NewApiKeyResult.key`, a token-shaped sample that
GitHub push protection rejects); it is listed under `redactions` in
`grafana/openapi/source.json` with the untouched upstream sha256. Examples are
dropped before codegen, so the generated types are identical.

 Prometheus generates its spec in `web/api/v1/openapi.go`
and serves it at `/api/v1/openapi.yaml`; the antix1 3.16.0 server (revision
e2e28ade) serves this document byte-for-byte apart from `info.version` and
`servers[0].url`. Machine-readable provenance: `<plugin>/openapi/source.json`.

## Pipeline

```
<plugin>/openapi/select.json ─┐
<plugin>/openapi/errata.json  ├─ prepare.py ─► <plugin>.schema.json ─ cargo-typify 0.10.1 ─► emit.py ─► <plugin>/api_types.rs
spec file ───┘                 <api>.methods.json ───────────────────────────────┘
```

* `<plugin>/openapi/select.json` — the read-only operations exposed, by method name.
* `prepare.py` — copies every component schema reachable from those
  operations, builds `<Method>Input` from each operation's parameters and
  `<Method>Output` from its 200 JSON body. Transformations (all documented in
  the script): OpenAPI 3.0 `nullable` → JSON Schema `null`; non-numeric
  `format`s and `pattern`s are kept as `x-upstream-*` extensions so values stay
  plain strings (no chrono/uuid/regress in this crate); discriminated inline
  `anyOf` variants are hoisted to named definitions (works around typify
  merging same-named nested types).
* `<plugin>/openapi/errata.json` — upstream spec defects, each proven by a captured live
  response in `<plugin>/fixtures/` and guarded by the expected upstream
  value, so a fixed upstream fails regeneration loudly. Current entries:
  Prometheus `string` query results (timestamp is a number), Grafana
  datasource health (body carries `status` and `details`).
* `emit.py` — writes the committed `<plugin>/api_types.rs` with a provenance header and
  the generated `OPERATIONS` table (method, HTTP method, path template,
  operationId, input/output type) that the plugins and the bridge dispatch use.

Regenerate (needs `cargo install cargo-typify --version 0.10.1 --locked` and
python3 with PyYAML): `crates/op-plugins/src/state_plugins/openapi_codegen/regenerate.sh [plugin ...]`. Upgrading a
server: vendor the new tag's file into `<plugin>/openapi/`, update `source.json`, regenerate, recapture
fixtures, run `cargo test -p op-plugins prometheus grafana`.

## Runtime configuration (deploy-time only, never method input)

* `PROMETHEUS_URL` (default `http://127.0.0.1:9090`)
* `GRAFANA_URL` (default `http://100.96.0.5:3000`), `GRAFANA_API_TOKEN`
  (optional Viewer service-account token; without it only `health` succeeds)

Bounds: Prometheus 30 s / 16 MiB / 10 000 series / 250 000 points, `limit`
clamped and a 25 s PromQL timeout added when absent; Grafana 15 s / 16 MiB /
1 000 items. Violations are typed `UpstreamCallError`s.
