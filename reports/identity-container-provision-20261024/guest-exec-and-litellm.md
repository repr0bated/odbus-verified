# Guest exec boundary and LiteLLM preparation

Task: identity-container-provision-20261024. Agent: Codex /root.
Updated: 2026-10-25T03:17Z.
Source commit: 86f03d2dede738e2d780ab789dd5800b58678479; unrelated WIP retained.

## Owner direction

The owner requested guest-side Btrfs send/receive and overlay operations using
Incus exec, then selected LiteLLM with JEV configuration and a systemd service
inside the seed. API-key delivery is a later activation step. The latest
requirement is to skip service startup cleanly when the key is missing, rather
than produce service failures. This supersedes the earlier local-weights/client
clarification; do not reopen it or copy any personal JEV state or key.

## Canonical guest exec attempt

Caller session: 414fbc56-03fb-20ab-758a-bcc7a7cbdc0e through
`/run/mounts/3tchedFS/plugins/incus/operations/exec_instance/call`.
Requested name ID-0; command
`/usr/bin/findmnt --json --output TARGET,SOURCE,FSTYPE,FSROOT,OPTIONS`.
The canonical invocation refused admission, exit1, without an operation ID:

```
org.freedesktop.DBus.Error.AccessDenied: capability
'cap.service.incus.instance.exec@v1' is not granted to the authenticated
caller of method 'exec_instance'
```

Exact private evidence: `/var/tmp/identity-incus-guest-check-20261025.dfizY5Yz/`
(`args.json`, empty `receipt.json`, `stderr.txt`, `exit-code`). No guest command
ran. No native Incus, alternate identity, host privilege, or transport bypass
was attempted. This denial is independent of the authorized offline seed work.

Current source identity creation declares an unprivileged container with root,
runtime, FUSE and lungs-socket mounts, but no unix-block attachment. A disk mount
is not raw guest block access; moving a command into Incus exec alone does not
establish the requested block-device or Btrfs authority. Existing
`sync-instance-upgrade.sh` expects a stopped container and performs host-side
receive/overlay work, so it cannot simply be wrapped in an exec requiring a
running container. No privileged-container conversion or capability grant was
made. References: `incus.rs:1490`, `native_container_creation.rs:198`, and
[Incus unix-block devices](https://linuxcontainers.org/incus/docs/main/reference/devices_unix_block/).

## Context recall

The canonical NotebookLM query for notebook
`c5c7f347-77cd-464d-bb02-50f422a74df4` returned outcome unknown/timed out.
Operation ID: `34bf6f1d-4883-493e-bc4b-ef0b1c7f9044`.
The mounted invocation state returned EIO; no automatic retry or successful
remote memory-sync claim is made. Present runtime and repository receipts are
authoritative for this continuation.

## LiteLLM build

Public, hash-locked LiteLLM1.105.1 proxy runtime:117 distributions,575MiB staged.
Resolution completed03:09:41Z and wheel installation03:10:34Z, both exit0.
Durable worker directory: `/var/tmp/identity-seed-litellm-20261025.BaoEqDYf`.
Inputs: `deploy/workspace-image/litellm/requirements.{in,lock}`.
Output: worker `payload/`, with complete runtime file hashes and preparation
manifest. The public payload was copied to a root-controlled directory:
`/opt/op-dbus/release-send/identity-container-provision-20261024-litellm-input`.
Both package-manifest and file-digest checks succeeded on that retained copy.
It is a preparation input directory, not a received release or live activation.

The resolver targets CPython3.14 and x86_64-manylinux_2_28; static WHEEL readbacks
show cp314 or stable-ABI extensions. The host installer used /usr/bin/python3
(host3.13) only as the installation interpreter; explicit target/version flags
selected guest3.14 wheels. Generated entrypoint shebangs use /usr/bin/python3,
also the guest's existing3.14 interpreter. The service uses module invocation
with PYTHONPATH and does not rely on generated entrypoints or host venv paths.
No new Python helper, guest execution, provider call, credential, test-suite,
Clippy, formatting check, or host service change was involved.

Service implementation uses ConditionFileNotEmpty for absent/empty activation
files and an optional EnvironmentFile plus a silent ExecCondition checker for
unset/empty/whitespace-only TYPESAFE_API_KEY. The checker returns1 to skip,
without marking the unit failed or applying its restart policy. It does not
validate provider authentication. No activation file belongs in the seed.
Official references: [systemd ExecCondition](https://www.freedesktop.org/software/systemd/man/latest/systemd.service.html#ExecCondition=)
and [LiteLLM JEV decision configuration](https://docs.litellm.ai/docs/decisions/).

## Completed offline installation

Installation completed exit0 at2026-10-25T03:16:40Z. The runtime, JEV
configuration, silent key checker and enabled systemd unit are installed in
the existing offline root. The private activation directory exists and is
empty. Installed runtime file hashes, service/config/checker bytes and enablement
link were read back successfully. No guest code was executed.

Durable root worker: `/var/tmp/identity-seed-litellm-install-20261025.x0HY7ZFC`.
It pinned image device/inode/logical size, outer ext4UUID and inner BtrfsFSID
before invoking the reviewed installer. The source-only independent review is
recorded in `litellm-review.md`; restrictive build umask and public ancestor
permissions were corrected before installation. Package bytes did not change.

Local copied receipts: `litellm-{resolve,build,install}-completion.json`,
`litellm-preparation.json`, `litellm-image-stat.json`, `litellm-space.txt`,
and `litellm-source.sha256`. The installed receipt is
`@/usr/share/op-dbus/litellm/preparation.json`.

The sparse whole image still has42949672960 logical bytes; allocated bytes are
now9318735872 (8.68GiB), superseding the pre-LiteLLM8.14GiB figure. Root p1
remains11849957376B with2065272832B available. No resize, partition2 mount or
write, sealing, new subvolume/member, host activation, key, identity mint, or
container provisioning occurred. Original backups remain retained.

This proves installed offline contents only. Guest boot, runtime activation,
provider connectivity/authentication and the numbered container workflow remain
unverified or incomplete. Tests/smokes/Clippy/formatting were not requested and
were not run. Next broader action is to resolve the supported image-aware
provisioning/guest-exec authority and block-device contract, preserving the
recorded admission denial and actual human approval requirements.
