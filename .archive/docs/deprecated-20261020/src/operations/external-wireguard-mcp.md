# WireGuard-scoped external MCP

The deployed bridge's external HTTPS MCP admission accepts one fresh OIA1 assertion from the trusted
WireGuard issuer. The receiver derives a stable logical origin from the issuer
key ID, registered human principal, peer public key, and signed exact AllowedIP
host route (`/32` or `/128`). It does not require an identity container or SID1.

This logical origin is separate from the browser `Origin` header. Browser-origin
checks remain in force. The issuer already verifies the inner source address
against an unambiguous WireGuard peer's exact host route before signing it.

The bridge records the origin and genesis in a separate durable Cozo relation.
Every request verifies the signature, lifetime, registration, revocation, and
existing principal grants; it consumes the single-use nonce before publishing
the connected session. Renewal preserves that origin. A change of peer, issuer,
or AllowedIP requires a new MCP conversation. No capabilities are granted by
being on WireGuard, and a stored origin record is not a credential.

Generic human-session checks accept the connected external session. Canonical
method receipts and synchronous secondary mutations retain its exact session
and genesis. Machine-specific SID1 and container execution checks remain
separate; this change does not provision a machine credential for native model
execution.

The 3tched CLI now launches MCP directly, uses native HTTPS with explicit CA
verification, encodes the HTTP identity header as canonical base64url, and
obtains a fresh assertion for every request. It pins the peer, address, issuer,
and door. Local Codex and Cursor `opdbus-context` configuration now launches
this adapter directly instead of using the incomplete SSH command.

## D-Bus dispatch

The live session bus already exposes `mcp` and `cognitive_mcp` PluginV1 objects.
The repaired HTTP MCP adapter also traverses actual D-Bus: a private per-call
Unix socket pair carries `PluginV1.CallWithOperationId` into the canonical sealed
schema receiver. It preserves the authenticated caller and rechecks the session
and current grants at that receiver. The pair has no public listener or shared
root sender identity. A production future-boxing fix makes this path pass its
real-wire integration test on the default Tokio worker stack.

## OpenCode and Paseo servers

These are the actual applications, with MCP used only for their door tools.
OpenCode 2.0.18 and Paseo 0.10.3 are installed in the received read-only release
`20261007T083404Z-agent-servers`. Both new services were enabled and started through
`org.opdbus.v1.Runit.Systemctl` on the system bus. Existing services were not
restarted; the existing OpenCode daemon on port 49374 remains intact.

The new servers bind only to loopback: OpenCode 4096 and Paseo 6767. Both native
authentication checks pass, and OpenCode's `opdbus-context` entry reports
connected through the existing identity-bound native 3tchedFS mount. Paseo uses
the same user's OpenCode configuration. Paseo public relay is disabled.

Private URLs are served on the VPS as `https://opencode.wg.internal:8090` and
`https://paseo.wg.internal:8090`. The local hosts file maps both names to 10.0.0.2,
and the existing door CA has been verified against the VPS and installed in the
local trust store. The live certificate retains every existing SAN plus these
two names. VPS HTTPS checks pass for both root pages, native login/refusal,
OpenCode event streaming/cancellation, Paseo authenticated WebSockets, and
cross-origin WebSocket refusal. Home-to-VPS TCP access is still under diagnosis:
SYN packets from the actual WireGuard peer 10.0.0.9 reach tun5 but receive no reply.
The user's WireGuard connection is on the home router, not on the VPS.

Service credentials stay in mode-600 files under
`/home/jeremy/.config/opdbus-agents` on the VPS. They are not embedded in URLs,
process arguments or this report. Service logs are root-private. Activation and
rollback evidence is in `/opt/op-dbus/recovery/agent-servers-20261007`.

## Verification and activation

Rust and client verification results are recorded in the task artifact
`/home/jeremy/Documents/Codex/wireguard-mcp-20261007/validation.md`.
The running bridge now matches received release
`20261007T094537Z-wireguard-mcp-dbus`, SHA-256
`5c1660b6f9bc9994ad5ce6aa28cb1a3435db5604395980794584b4fa0887fb91`.
The preceding server-only release
was snapshotted from `20261006T054013Z-ed02a78-notebook-progress`, transferred by
Btrfs send/receive, and checked to retain the exact existing backend payload.

Current focused checks: real D-Bus 1/1, external MCP 7/7, oracle assertions 50/50,
full MCP frontend 49/49, UI proxy 4/4, Cozo external sessions 4/4, CLI 31/31 plus
CLI lint. The older projection fixture now uses authoritative identity and a
test-only cognitive owner. Full workspace formatting found pre-existing drift
and a rustfmt allocation panic; workspace Clippy fails in pre-existing generated
filesystem/examples code. The broader bridge test binary finished with 697
passed, 70 failed and 5 ignored; all owned MCP tests passed. The causes of every
failure outside the focused checks are not established. Isolated contract
generation and the native GNU workspace release build passed. The isolated
release-profile checks also passed: external MCP 7, oracle assertions 50,
frontend 49 and real D-Bus dispatch 1. The frozen source and binary checksums
remain unchanged after those tests. The complete 3 GiB filesystem was sent and
received; all 69 plugins remain, with only threetched_fs changing to 2.0.6.

The user explicitly approved adding only `cognitive_mcp.read` to external peer
principal `e4d3c524-7478-f714-bcfd-6c21ddcd93f2`. That exact grant is now applied,
with every other principal entry verified unchanged. The existing materializer
published an identical root-only projection; both SHA-256 hashes are
`80b65206244c88440a962b7855537da687f6c72b09b650bab1ba959ecfb5f5c0`.
This capability lasts until removed and has the resource limits documented in
the grant proposal. Registration remains unverified; no key registration,
identity credential, write capability or administrative grant was changed.
Receipt: `/opt/op-dbus/recovery/wireguard-read-grant-20261007/receipt.json`.

The post-restart context failure was reproduced with both the original and new
bridge. The user had removed 10.200.0.2/24 from ovsbr0 as a presumed leftover;
that address is the configured host relay for the healthy Qdrant container.
The existing ovsbr0-svc-addr owner restored it through the D-Bus service
lifecycle. Native Qdrant
HealthCheck now passes (1.18.2). Restarting only the bridge cleared a failed
initialization's retained database handle. The exact OpenCode 3tchedFS adapter,
as jeremy with its existing assigned session, successfully completed
cognitive_mcp.memory_recall: operation 28ce3afb-008e-4c83-bd80-f885f645aa68,
attempt 245761, terminal 245762. No identity, container, or grant was changed to
repair this dependency. The pre-existing failed-initialization task cleanup
defect is separately recorded; mandatory durable vector availability stays in force.

At the user's request, all backend and UI/CLI uncommitted paths were classified.
Retained source was reconstructed from exact active provenance plus the tested
owned patch. Rolled-back Render/Accountability and unproven pending work are
preserved losslessly outside the checkout under
`/var/tmp/opdbus-git-reconcile-20261007.N5WhfJ`. Source commits are prepared as
backend fbf98971c65b655848a3c992bb7b43c90152c24e and
UI/CLI d5e14cdb0c90fe8735acade1048a5ded50170af4. The retained branches are
`feat/od-108-native-lookup` and `claude/ui-gallery-promotion`. Classification,
pre-cleanup archives (including nested Git), incremental commit bundles, and
final clean-status/remote-ref receipts belong to that recovery directory.
The retained CLI passes type-check, lint, production build and all 86 affected
CLI/adapter tests; the unsupported pending search/evidence wiring is archived.
Distributed local/VPS CLI
copies match the retained CLI source, with CA/configuration hashes preserved.
