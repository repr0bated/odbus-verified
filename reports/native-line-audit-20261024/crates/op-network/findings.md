# op-network native contextual audit

**Coordinator correction:** the original cut/paste BEFORE copies and restored
source each lacked one final newline. Their “exact full BEFORE” claim below is
superseded by byte verification: adding that newline to each private reconstructed
copy reproduced all seven initial SHA256 hashes exactly. Original source EOF
newlines were restored after checking unchanged current hashes. Use assignment
`BEFORE-reconstructed/` as the hash-confirmed original; `BEFORE/` retains the
imperfect worker copies for provenance. Remaining source diffs are the intended
wording corrections only. No executable logic was changed by this reconciliation.

Task `native-line-audit-20261024/op-network`, Codex crate worker, 2026-10-24. Static source audit only. All 42 supplied inventory entries read fully and sequentially through EOF; root/src/examples/bin/bin/op-identity-storage/door/identity/transport directory listings reconcile with inventory. No nested instructions, binary, missing, symlink or dependency entries discovered in the crate. Initial hashes are inventory receipts, not current recomputed hashes; commit unqueried.

## Resolved native-specific wording defects

- F1 — Cargo.toml:7, SPEC.md:83, compare-op-network.md:25, src/lib.rs:4: “Native ... OpenFlow (all versions, pure Rust)” overclaimed support. Public version enum exposes1.0/1.3, active client uses1.3, passive controller requires1.5. Removed all-versions claim and described Rust client/controller; library docs name rovs-openflow and1.5 controller. J2.wording correct_wording0.97. Native terminology retained. No protocol API changes.
- F2 — src/ovs_capabilities.rs:208-209,227-228: LLM prompt strings incorrectly presented OvsdbClient as a direct native socket client and “all operations” as direct socket access. Current client calls the sealed ovsdb_bridge PluginV1 via canonical D-Bus; rovs_proxy is the backend. Replaced the two strings with explicit caller-facing D-Bus/backend JSON-RPC distinction and required-grant/caller-authorization boundaries. J2.caps correct_two1.0. No capability checks or operations changed; remaining generic capability overclaims below are not silently certified.
- F3 — src/bin/op-of-controller.rs:103-104: native-readback description promised installation on an always-denied legacy entrypoint. Clarified denial and attributed CAS with native switch readback. J3.legacy clarify_comment0.91. Denial, owner callback, permit consumption and final admission untouched.
- F4 — src/rovs_proxy.rs:32-38 before /32-36 after: parenthetical incorrectly attributed rovs_ovsdb usage to OpenFlow. Replaced with direct native OVSDB backend, distinct D-Bus caller client, separate rovs_openflow library and existing mutex ownership. J4.backend clarify_family0.79. No function/type names or caller contracts changed.

Full exact BEFORE copies for seven changed files: `/var/tmp/native-line-audit-20261024.ILroaQ/crates/op-network/BEFORE/`, preserving relative paths. Native patch application used hashline edits after rereading current files. Backup creation used named CUT/paste registers with immediate identical source restoration and duplicate paste into empty BEFORE files; tool snapshots for each source/backup matched before actual wording edits. Other agents' work was not reset/stashed/replaced.

## Preserved native meanings

- Linux Netlink host-endian ABI (`NativeEndian`) reads/writes; direct rtnetlink/ioctl implementation vocabulary.
- Incus instance UUID/root pins; Btrfs held mount/receipt/guard names; provisioning receipt locals. These are not model/provider selectors or authorization grants.
- OpenFlow actual readback and backend fixtures, native D-Bus/mTLS transport with fixed `opdbus-native-v1` ALPN. No protocol string rename.
- OVSDB upstream parser/schema/monitor revision/RPC/session and private installed-server fixture names. They describe lower-level backend work, not D-Bus end-to-end coverage.
- Proxmox direct reqwest REST client terminology. Native DHCP client is explicitly TODO/required/pending, not implemented; no new DHCP scope.

J1.storage0.33 and J3.protocol0.33 were rejected as weak answers, not used to settle occurrences; concrete supplemental J3.storage0.85 and split J4.protocol answers resolve them. See judgments.md and occurrence mappings.

## Separate generic observations — not native fixes

These are source-only observations; external caller reachability, exploitability, deployment and runtime behavior are unverified.

- SPEC/comparison source counts, binary counts, dependency lists and implemented-status inventory are stale. Other controller comments still say OF1.3 although passive wire version is1.5; rovs_proxy logs/compatibility comments still refer to a D-Bus daemon. No general documentation rewrite.
- ProxmoxClient::with_config (proxmox.rs:225-239) disables certificate verification; token/request/response structs derive Debug with sensitive fields. Native REST describes direct implementation, not security assurance. No transport or credential changes.
- Capability detection assigns create/add/flow booleans from list_dbs reachability; remaining counter-excuse strings overclaim query_flows (currently empty), root/permission and operation availability. Requires separate capability/consumer review; correcting two native-specific strings does not fix all prompts.
- openflow.rs:149-151 add_flow_rule warns then returns Ok, :218-220 query_flows returns empty; plugin.rs:398-427 DHCP only checks lease-file metadata then returns Ok. TODO/required/pending native words accurately identify missing client. No placeholder implementation added.
- ovsdb.rs:532-542 monitor_db returns a receiver whose sender immediately drops, despite idle-open prose; real rovs_proxy monitor path is separate. No new monitor shim.
- rovs_proxy::with_client retries failures once, including mutation callbacks; ambiguous transport failures can replay. Native monitor/RPC terminology does not justify retries. No retry redesign.
- datapath_safe fallback matching ignores cookie and controller-in-band UUID selection is broad; static FDB matching ignores port/VLAN. Boot setup invokes utilities and contains older network prose. No network operations were run or changed.
- door, identity, error and transport files are not declared by lib.rs. DoorProxy constructs/discards metadata before raw stream forwarding; transport/mod.rs refers to absent child files but is not declared. Report does not represent these as active ingress paths or bypass them.
- Seed/storage paths enforce older image/pool layouts; current provisioning semantics were not deployed or validated. No membership, subvolume, mount, identity or grant mutation.

## Verification and limits

Verification exercised: complete sequential read receipts, directory reconciliation, five actual successful declarative JEV calls (including weak-answer supplements and final static accounting), exact source/backup snapshot pairing, and hashline patch receipts showing only specified descriptions/prompt text/comments. No significant protocol/admission behavior changed. No searches, extraction, shell, arbitrary eval, Cargo/build, tests, lint, formatting, custom smoke, runtime, deployment, seal, credentials, grants, staging or commits. User assignment explicitly excludes these checks; primary coordinator owns reconciliation and any required build/check.

No tched/threetched hunks changed; no external caller migration or unresolved native-specific overlap. Shared HANDOFF bookkeeping alone is outside exclusive report path because higher-priority session instructions require it. No built/tested/installed/deployed claim.

Next: coordinator reconcile seven wording-only patches, full coverage, occurrence ledger and JEV receipts. No runtime success is implied by this static completion.
