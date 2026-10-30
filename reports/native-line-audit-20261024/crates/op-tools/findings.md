# Findings — `op-tools`

## N1 — Native-alternative table advertised unsupported tools

**Evidence:** `src/security.rs:232-266` initially mapped `systemctl` and `journalctl` to `dbus_systemd_*`, `ip` to `network_*`, `nmcli` to a NetworkManager D-Bus interface, and apt/apt-get/dnf to `packagekit_*`. The fully read `src/builtin/mod.rs:28-74` registers none of those specialized tool families. It registers `ovs_tools`, `rtnetlink_tools`, `sv`, and generic D-Bus introspection. `src/builtin/rtnetlink_tools.rs` contains actual rtnetlink tools, including `list_network_interfaces` and `rtnetlink_*` operations. Repository policy identifies runit/`sv` as current and forbids systemctl/foreign service-manager CLIs.

**Decision:** native-specific accuracy defect; corrected narrowly in `src/security.rs`. Removed the unsupported/stale systemd, NetworkManager, and PackageKit recommendations; replaced `network_*` with available rtnetlink-tool wording for supported operations; limited OVS recommendations to supported operations. The table, `get_native_recommendation`, warning-only behavior, and existing OVS warning test remain. No claim about deployed availability.

**Before evidence:** `crates/op-tools/before-security.txt` contains the exact affected-hunk excerpt reconstructed from the pre-edit read receipt. Process limitation: the excerpt was written after the edit rather than saved before it. The full original file was read before the edit; this is not a byte-for-byte whole-file backup or a contemporaneous backup.

## N2 — “Native” could imply direct `sv` CLI execution

**Evidence:** `src/builtin/sv.rs:23-42,138-152` defines a D-Bus proxy with a `restart` method; `SvRestartServiceTool::execute` calls `control("restart", ...)`. The operation is one restart call, but the op-tools tool itself uses the D-Bus service, not a direct shell `sv` invocation.

**Decision:** removed “native” from the module comment and tool description; retained the precise one-restart-operation/not-stop-start meaning. No API shape or behavior changed.

**Before evidence:** `crates/op-tools/before-sv.txt` contains the exact affected-hunk excerpts reconstructed from the pre-edit read receipt. As with N1, it was written after the edit and is not represented as a contemporaneous backup.

## N3 — Unsupported comparative error-handling claim

**Evidence:** `src/builtin/shell.rs:17-18` recommended D-Bus/OVSDB “for better error handling,” but the source establishes the concrete benefit of structured responses rather than a general comparison of error handling. The tool description at line 46 already describes structured responses.

**Decision:** replaced the unsupported comparative claim with “for structured responses”; preserved the advisory/non-blocking shell clause. JEV `shell_error_handling_claim` supports the narrowed wording at confidence 1.0.

**Before evidence:** `crates/op-tools/before-shell.txt` is the exact affected-hunk excerpt saved from a pre-edit read.

## N4 — OVS warning text no longer matched its retained assertion

**Evidence:** After N1, the two OVS recommendation strings at `src/security.rs:237,241` said “available ovs_* tools,” while the current `test_admin_gets_native_warnings` assertion at `src/security.rs:647-655` requires `.contains("native tools")`. `check_command` returns these strings as optional, non-blocking warnings for unrestricted commands (`security.rs:309-323`); the shell caller logs the same warning and forwards it unchanged as `native_alternative_hint` (`builtin/shell.rs:122-135,146-179`). The registered OVS tools make an OVS-tool recommendation accurate for supported operations.

**Decision:** Added “native tools (ovs_*)” to both OVS strings, retaining the supported-operation qualification and structured-response claim. This restores the existing assertion contract without changing callers or assertion. JEV `ovs_warning_contract` supports this minimal correction at confidence 0.97. No test was run.

**Before evidence:** `crates/op-tools/follow-up-before-security.txt` contains the exact current affected hunk before this correction.

## N5 — Blanket native-protocol advantages were unsupported

**Evidence:** `src/security.rs:22-29` generalized that D-Bus, OVSDB, and rtnetlink provide “Better error handling” and “No parsing issues.” The inspected OVSDB, rtnetlink, and D-Bus tool paths return structured JSON for supported operations, but their errors are often converted to `anyhow` text or collected as strings. The shell path captures stdout/stderr as strings and returns an exit code; it does not establish that every protocol alternative has categorically better error handling or can never encounter parsing issues.

**Decision:** Retained the protocol preference and non-blocking clause, but replaced the two blanket guarantees with the qualified, source-supported benefit: these tools provide structured results for supported operations. JEV `native_protocol_rationale` selected this narrow correction at confidence 1.0. No generic shell/security behavior changed; no tests were run.

**Before evidence:** `crates/op-tools/follow-up-before-rationale.txt` records the exact affected hunk before the edit.

## Retained contextual uses

- OVSDB JSON-RPC occurrences accurately describe the OVSDB protocol call and result marker.
- Rtnetlink occurrences accurately describe the Linux API/protocol invoked by the tools.
- Native-protocol rationale now states structured results for supported operations; no general superiority or no-parsing guarantee is made.
- The OVS warning remains advisory/non-blocking and retains the `native tools` wording expected by the existing warning assertion; tests were not run.

No additional native-specific defect or cross-crate overlap was identified. Generic defects are outside this assignment's scope.
