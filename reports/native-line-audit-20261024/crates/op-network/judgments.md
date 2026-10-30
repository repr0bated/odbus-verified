# Declarative JEV receipts

Installed facility: `xd://eval/judge`; actual successful eval call J1, JS `await judge(state, questions)`. No routing extension or model selection.

## J1 — storage / OpenFlow / D-Bus TLS
State source references: lungs_attach.rs:213-245,345-368; incus_root_subvolume.rs:46-48; btrfs.rs:1,33-39,419; controller.rs:49-50,1131-1140,1190-1225; controller_flow_verify.rs:21-187,215,237,293; controller_native_tests.rs:1-15,145-212; dbus_tls.rs:1-5,41-68,85-129,165-170,218-268. Paths relative to crates/op-network/src except bin/op-identity-storage/lungs_attach.rs. Evidence supplied the actual names, descriptive strings, direct ioctl/FSID/root checks, flow readback/final-admission behavior and TLS ALPN/fixture contexts (full call retained in tool transcript).

Questions (typed choices, independent in one call):
- storage: Are native/natively names and descriptions accurate underlying Incus identity/ioctl terminology, a specific inaccurate native claim, or insufficient context?
- flow: Are native fixture/module/type names and readback descriptions conventional protocol/backend test names, inaccurate native claims, or insufficient context? Distinguish synthetic frames from installed OVS and do not imply execution/deployment.
- tls: Are direct D-Bus transport, fixed ALPN and fixture native names accurate vocabulary, inaccurate semantics/unauthorized dispatch, or insufficient context?

Actual response:
```json
{"storage":{"type":"choice","choice":"preserve","confidence":0.33,"probabilities":{"preserve":0.55,"change":0.36,"insufficient":0.09}},"flow":{"type":"choice","choice":"preserve","confidence":0.98,"probabilities":{"preserve":0.99,"change":0.01,"insufficient":0}},"tls":{"type":"choice","choice":"preserve","confidence":0.98,"probabilities":{"preserve":0.99,"change":0.01,"insufficient":0}}}
```
Decision: preserve flow/TLS terminology; fixture evidence is static only. Storage answer below0.6 is NOT accepted as a decision; closer exact-source review and supplemental JEV required. No source mutation authorized by JEV.

## J2 — repeated descriptions, caller prompt, pending DHCP

Actual successful `await judge` call; evidence read before call:
- wording: Cargo.toml:7, SPEC.md:83, compare-op-network.md:25, lib.rs:4,9; exact “Native networking: OpenFlow (all versions, pure Rust)” / “Native OpenFlow protocol implementation (all versions, pure Rust)” strings; openflow.rs:69-90,104-133 public V1_0/V1_3 and active1.3; controller.rs:251-258 passive required1.5. Proposed remove all-versions overclaim, retain native Rust library-backed client/controller.
- caps: ovs_capabilities.rs:206-235 exact direct native socket and “all operations are direct socket communication” prompt strings; ovsdb.rs:1-7,18-20,98-116 canonical sealed D-Bus PluginV1 Call; ovs_netlink.rs/rtnetlink.rs direct Rust kernel APIs. Proposed correct two OVSDB strings and preserve “Both are native Rust.”
- dhcp: plugin.rs:398-427 exact native DHCP TODO / native client required / native client pending, metadata-only lease check and unconditional Ok.

Typed independent questions: (wording) supported narrow correction vs supported all-versions vs insufficient; (caps) correct two caller/backend/grant strings vs preserve all direct-socket claims vs insufficient; (dhcp) preserve explicitly absent native wording and report generic behavior vs native-specific wording fix vs insufficient.

```json
{"wording":{"type":"choice","choice":"correct_wording","confidence":0.97,"probabilities":{"correct_wording":0.98,"preserve":0,"insufficient":0.02}},"caps":{"type":"choice","choice":"correct_two","confidence":1,"probabilities":{"correct_two":1,"preserve_all":0,"insufficient":0}},"dhcp":{"type":"choice","choice":"preserve_native_report_generic","confidence":0.95,"probabilities":{"preserve_native_report_generic":0.97,"native_fix_needed":0.03,"insufficient":0}}}
```
Decision: implement F1/F2 wording corrections; preserve DHCP native absence labels, report generic gap separately.

## J3 — concrete storage, ABI, protocols, routes, REST, denied legacy entrypoint

Actual successful `await judge` call, full response retrieved from `artifact://2:raw:1-1` after display truncation.

Evidence:
- storage: lungs_attach.rs:223-245,345-368 exact native_uuid/native_root/error strings and UUID/root/FSID checks; incus_root_subvolume.rs:46-48 native pool source; btrfs.rs:1,33-39,419 direct ioctl and ignored fixture; seeded_prepare.rs:215-249 all native receipt-local member uses/readback error; seeded_boot.rs:209-215,294-334 native guard/mount verification; seeded_root.rs:324-335,1851-1854 native owner/INTENT cleanup. This supersedes rejected J1.storage0.33.
- netlink: ovs_netlink.rs:1,8,250-328,381-433,460-484,679-695,719-734,782-788,860-866,942-948 NativeEndian reads/writes of kernel headers, scalar OVS attributes, errors, ifindices and stats; direct NETLINK_GENERIC backend title. No IP-packet address encoding alleged.
- protocol: rovs_proxy.rs:1-38,810-842,1018-1062 native direct backend/monitor/RPC/session labels; ovsdb_monitor.rs:1-11,70-75,129-168 upstream parser/schema/revision; ovsdb_monitor_tests.rs:26-27,132-203 synthetic protocol tests; rovs_proxy_native_tests.rs:8-76,102-118,169-186 installed private server fixture.
- routes: bin/op-rtnetlink-init.rs:1-7,32-42; rtnetlink.rs:1-8,461-479,726-760,816-843 exact native RTM_NEWLINK/DELLINK/route descriptions and unsupported-type error; wireguard_kernel.rs:307-346 opt-in fresh network namespace fixture.
- rest: proxmox.rs:1-4,225-239,304-371 and lib.rs:9 direct reqwest REST native labels, separately disabled cert verification.
- legacy: bin/op-of-controller.rs:46-55,103-113,61-101 exact native readback promise on denied method versus attributed callback/permit path; controller.rs:834-876 readback/final admission.

Typed independent questions: storage legitimate system receipt vocabulary vs specific defect vs insufficient; netlink legitimate host ABI vs wrong endianness vs insufficient; protocol legitimate backend terminology vs inaccurate specific label vs insufficient; routes kernel API/fixture vocabulary vs backend misdescription vs insufficient; rest preserve REST wording/report TLS separately vs native label false vs insufficient; legacy clarify denied entrypoint comment vs existing accurate vs insufficient.

```json
{"storage":{"type":"choice","choice":"preserve","confidence":0.85,"probabilities":{"preserve":0.9,"change":0.07,"insufficient":0.03}},"netlink":{"type":"choice","choice":"preserve","confidence":1,"probabilities":{"preserve":1,"change":0,"insufficient":0}},"protocol":{"type":"choice","choice":"preserve","confidence":0.33,"probabilities":{"preserve":0.55,"change":0.4,"insufficient":0.04}},"routes":{"type":"choice","choice":"preserve","confidence":0.94,"probabilities":{"preserve":0.96,"change":0.02,"insufficient":0.02}},"rest":{"type":"choice","choice":"preserve_report_tls","confidence":1,"probabilities":{"preserve_report_tls":1,"change":0,"insufficient":0}},"legacy":{"type":"choice","choice":"clarify_comment","confidence":0.91,"probabilities":{"clarify_comment":0.94,"preserve":0.06,"insufficient":0}}}
```
Decision: preserve storage/ABI/routes/REST terms; generic TLS observation remains report-only. Implement F3 comment correction. Reject weak protocol0.33 verdict; split actual contexts in J4 before deciding.

## J4 — split supplemental OVSDB protocol decisions

Actual successful `await judge` call. Four independent questions with preserve/change/insufficient alternatives:
- backend: rovs_proxy.rs:3-7,32-38,137,166 vs openflow.rs imports/VConn; exact native backend language and false parenthetical that rovs_ovsdb is already used by OpenFlow. Clarify separate library attribution vs preserve all vs insufficient.
- rpc: rovs_proxy.rs:810-842,1018-1062 and23-24; native monitor/session/RPC names, direct rovs stream/Connection/transact implementation and timeout diagnostics. Preserve implementation vocabulary vs specific inaccurate label vs insufficient.
- monitor: ovsdb_monitor.rs:3,70,137 direct get_schema + DbSchema parsing, safe columns/IDL revision; ovsdb_monitor_tests.rs:27,132,186 UnixListener/rovs parser fixtures. Preserve protocol/synthetic-test terms vs inaccurate term vs insufficient.
- fixtures: rovs_proxy_native_tests.rs:8,14,21(two positions),72,102,104,114,169; installed private ovsdb-server fixture and RFC lock connection ownership. Preserve backend fixture names without D-Bus end-to-end claim vs native label inaccurate vs insufficient.

```json
{"backend":{"type":"choice","choice":"clarify_family","confidence":0.79,"probabilities":{"clarify_family":0.86,"preserve":0.06,"insufficient":0.08}},"rpc":{"type":"choice","choice":"preserve","confidence":0.7,"probabilities":{"preserve":0.8,"change":0.15,"insufficient":0.05}},"monitor":{"type":"choice","choice":"preserve","confidence":0.72,"probabilities":{"preserve":0.81,"change":0.16,"insufficient":0.03}},"fixtures":{"type":"choice","choice":"preserve","confidence":0.88,"probabilities":{"preserve":0.92,"change":0.06,"insufficient":0.02}}}
```
Decision: F4 comment-only correction; preserve actual backend native names, protocol schema/IDL terms and fixtures. J4 resolves weak J3.protocol answer; no global admission, runtime availability, D-Bus endpoint coverage, tests or deployment certified.

## J5 — final static evidence and accounting

Actual successful `await judge` call. State:42 full sequential inventory reads and reconciled listings;154 occurrence rows (152 source +2 filename); eight corrected wording contexts,144 other source occurrences preserved; two removed native tokens =>150 current source tokens; exact before/patch receipts for seven files; J1–J4 actual calls with weak answers rejected/supplemented; explicit no external admission/build/test/runtime/deployment proof.

Typed questions: complete STATIC scoped assignment vs partial named native-specific action/coverage vs insufficient; coherent supplied arithmetic vs inconsistent vs insufficient. JEV was not asked to independently certify source absence or search for additional occurrences.

```json
{"completion":{"type":"choice","choice":"static_complete","confidence":0.88,"probabilities":{"static_complete":0.92,"partial":0.05,"insufficient":0.03}},"accounting":{"type":"choice","choice":"coherent","confidence":0.8,"probabilities":{"coherent":0.87,"inconsistent":0.12,"insufficient":0.01}}}
```
Decision: report complete static scoped audit with supplied-hash/current-runtime limits. Five successful calls,18 typed answers, two weak0.33 answers rejected and resolved by supplements;16 accepted source/accounting/completion answers. Occurrence absence outside ledger is reader observation over full coverage, not an excerpt-judge search proof.
