# Declarative JEV receipts — op-jsonrpc

Date: 2026-10-24. Installed `judge()` through Python eval after reading `xd://eval/judge`. One successful call, three independent typed choice questions over one combined evidence state. No model routing, searches, filtering, filesystem execution or recursive delegation.

## Evidence state supplied

Task: native-line-audit-20261024/op-jsonrpc; source-only audit, no execution authority.

Coverage: Sequential full-file reads: Cargo.toml 1–16; SPEC.md 1–220,221–414; compare-op-jsonrpc.md 1–63; src/lib.rs 1–19; src/ovsdb.rs 1–240,241–480,481–697; src/protocol.rs 1–144; src/server.rs 1–220,221–365. Root and src directory listings reconcile seven inventory files. Auditor observed zero case/compound native occurrences; this is a full-read observation, NOT proven by the excerpts below. JEV was explicitly told not to invent occurrences or certify whole-source absence from excerpts.

- Cargo.toml:7 description: JSON-RPC server with OVSDB and NonNet database support for op-dbus-v2. Dependencies9–16: tokio, serde, simd-json, anyhow, thiserror, tracing, uuid.
- src/lib.rs:1–19: documentation claims JSON-RPC Unix server, OVSDB client and NonNet database. Actual declarations: `pub mod ovsdb; pub mod protocol; pub mod server;` exports OvsdbDbusClient, JsonRpcServer; prelude also exports JsonRpcRequest/JsonRpcResponse. No nonnet module.
- src/ovsdb.rs:13–40: OvsdbDbusClient has socket_path:String, timeout:Duration. New uses `/var/run/openvswitch/db.sock`,30sec; with_socket and with_timeout set caller values.43–84 rpc_call connects UnixStream to self.socket_path, serializes method/params/id0, writes newline, shuts down writes, timeout reads to end, parses response, rejects non-null error and returns result.116–156 list_dbs invokes list_dbs; get_schema passes db; transact forms [db,operations...] and rejects per-operation errors.158–615 bridge/port insert/mutate/select/delete and schema-derived dump/monitor operations. These contexts describe OVSDB transport/database operations, not an LLM API.
- src/server.rs:26–45: JsonRpcServerConfig fields unix_socket:Option<String>,tcp_addr:Option<String>,ovsdb_enabled:bool; defaults `/var/run/op-dbus/jsonrpc.sock`,None,true.74–86 run matches listener options.118–125 parse SocketAddr, reject non-loopback, then bind TCP.
- src/server.rs:212–266 handle_request: method=&request.method; if method==`ovsdb.transact`, return PERMISSION_DENIED with `OVSDB transactions require verified control-plane admission`; then custom handler map; built-ins `ovsdb.list_dbs|ovsdb.get_schema` if ovsdb_enabled -> handle_ovsdb_request; server.info returns name/version/ovsdb_enabled; echo returns request.params; other -> METHOD_NOT_FOUND.269–311 handle_ovsdb_request makes OvsdbDbusClient::new(), handles list_dbs or get_schema with db param default Open_vSwitch; errors DATABASE_ERROR, otherwise success. No mutation dispatch there.
- src/protocol.rs:7–37 JsonRpcRequest fields jsonrpc,method,params,id; new sets jsonrpc2.0 and id0; with_id uses supplied id.114–124 parse_request deserializes to JsonRpcRequest or INVALID_REQUEST. No model/provider fields.
- SPEC.md:130–134 claims new(socket_path),register_method,run,shutdown;152–179 claims NonNetDb,nonnet,ovsdb_jsonrpc,nonnet_staging modules. compare-op-jsonrpc.md:13 claims eight Rust files;32,63 list nonnet modules. Actual directory has four Rust files (lib,ovsdb,protocol,server); server.rs:55 new(config)->Self;68 register_handler(&self,method,HandlerFn);74 run(self:Arc<Self>). No native text observed anywhere in these full-read docs.

## J1_transport_not_ai_routing

Instructions: Classify the exact supplied runtime configuration/dispatch contexts. Is there evidence that these selections impose an implicit LLM provider/model override, or are they database transport/method selections? Do not assert callers or deployment outside the evidence.

Criteria:
- database_transport_and_methods: Socket/timeout/listener/database/method choices concern OVSDB/JSON-RPC; no AI selection override established in these contexts.
- implicit_ai_override: Evidence shows hardcoded or implicit provider/model/config selection for AI execution.
- insufficient_evidence: Evidence does not distinguish the two interpretations.

Actual result: `{"type":"choice","choice":"database_transport_and_methods","confidence":1,"probabilities":{"database_transport_and_methods":1,"implicit_ai_override":0,"insufficient_evidence":0}}`.

Decision: preserve database transport and method selection. No native-specific routing repair established.

## J2_admission_order

Instructions: For the exact supplied handle_request source, classify whether credential-free ovsdb.transact can reach a custom handler or built-in OVSDB mutation. Limit claim to this shown reserved method, not arbitrary custom handler security.

Criteria:
- denied_before_custom_dispatch: Exact ovsdb.transact is denied before custom lookup and built-in OVSDB handling.
- mutation_path_reachable: Shown code permits ovsdb.transact through custom or built-in dispatch.
- insufficient_evidence: Source does not establish order or behavior.

Actual result: `{"type":"choice","choice":"denied_before_custom_dispatch","confidence":0.98,"probabilities":{"denied_before_custom_dispatch":0.99,"mutation_path_reachable":0,"insufficient_evidence":0.01}}`.

Decision: preserve existing reserved transaction denial. This does not certify arbitrary registered custom handlers, external callers, socket permissions or deployment.

## J3_documentation_scope

Instructions: Compare exact supplied SPEC/comparison/library claims against actual module declarations and API. Under an audit requiring only native-specific fixes, classify the stale NonNet/module/API mismatch. No native occurrence was observed in these documents during full reads; do not fabricate one.

Criteria:
- generic_stale_documentation_report_only: Concrete module/API drift is shown, but it is generic documentation debt rather than a native-specific defect; record separately without expanding mutation scope.
- native_specific_change_needed: Mismatch demonstrates an inaccurate native usage or native-specific behavior requiring change.
- documentation_matches: Supplied claims match actual modules/APIs.
- insufficient_evidence: Cannot determine mismatch or relevance.

Actual result: `{"type":"choice","choice":"generic_stale_documentation_report_only","confidence":0.99,"probabilities":{"generic_stale_documentation_report_only":0.99,"native_specific_change_needed":0.01,"documentation_matches":0,"insufficient_evidence":0}}`.

Decision: report generic documentation drift separately; no mutation under native-only assignment.

## Interpretation limits

JEV judges supplied evidence, not runtime or permission. Zero occurrences rests on all source lines read and the complete coverage ledger, not a claim that JEV received the entire crate. No literal occurrences existed to map to separate per-occurrence judgments. No build/test/deployment claim.
