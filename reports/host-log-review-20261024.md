# Host log review — 2026-10-24

Task: `host-log-review-20261024`. Agent: Codex `/root`, local caller `jeremy` (UID 1000), with authorized read-only host diagnostics through `sudo -n`.

Observed approximately 18:01–18:05 UTC / 14:01–14:05 EDT. Host `3tched`; boot ID `6767d86f-0030-439f-8471-dc078144167a`; kernel `6.18.57-x64v3-op-release-lifetime1-xanmod1`. Repository HEAD: `86f03d2dede738e2d780ab789dd5800b58678479`. The checkout already contained extensive unrelated modifications; this task adds only this report.

## Outcome

The host is running after its approximately 17:58 UTC reboot, with ample current memory and disk headroom. There was a significant OOM incident earlier today, and several application, desktop, and logging faults remain. No repair, service restart, build, test suite, deployment, grant change, or storage mutation was performed.

Live `/etc/os-release` identifies **Devuan GNU/Linux 6 (excalibur)**, despite the supplied repository guidance describing Artix. PID 1 is **runit** and live service links resolve into `/etc/sv` or packaged runit definitions. This is a verified environment discrepancy, not authorization to change the operating system or repository policy.

## Coverage and evidence

- Inventoried 1,417 files under `/var/log`. Searched retained text logs, including supported compressed rotations, for warning/error/failure patterns and separately for OOM, crashes, filesystem errors, and lockups. The broad pattern scan found matches in 592 files; those counts include historical messages, duplication, and harmless keyword matches and are not incident counts. Scan stderr was empty.
- Inspected current runit/application logs, system/kernel/authentication/boot logs, Incus logs, package/cron logs, and the current Hyprland log. Captured the complete current `dmesg` ring (558 lines at collection), not only its warning/error priorities. The Waybar segfault is present outside that priority-only selection.
- Checked all 39 enabled runit entries, relevant listener ownership, current memory/disk headroom, mounted 3tchedFS metadata and a supported NotebookLM read operation.
- Private scratch evidence: `/tmp/odbus-log-review-20261024-gnBcQe/` (0700). Files include `inventory.tsv`, `dmesg.txt`, `services.txt`, `all-match-counts.txt`, `all-severe.txt`, `scan-errors.txt`, `recent-matches.txt`, `kernel-incidents-today.txt`, and mounted NotebookLM receipts. Raw logs remain private and are not copied into shared memory.
- This is a pattern scan plus targeted review, not a line-by-line interpretation of every historical record. Binary login databases were not treated as text. EMQX and threetched currently send stdout/stderr to `/dev/console`; `/var/log/emqx` is empty, so complete persistent application-console history is unavailable. FUSE daemon log files under `/run/opdbus/host-fuse/` are empty.

## Findings requiring attention

| Priority | Finding and evidence | Status / next action |
| --- | --- | --- |
| High, earlier incident | **15 OOM kills at 06:04:36–06:04:49 EDT (10:04 UTC).** `/var/log/kern.log:17840` through the incident ending at line 21729 records Paseo, Chromium, and rust-analyzer victims. OOM diagnostics report all 8 GiB swap exhausted. The killed rust-analyzer had about 5.7 GiB anonymous resident memory. | Earlier boot; no OOM in the current ring. Investigate aggregate workload and indexing concurrency from the retained OOM task tables before selecting a remedy. A victim or allocating thread alone does not establish the root cause. |
| High, current boot | **Waybar segfault at 13:58:21 EDT (17:58 UTC)** in `libwayland-client.so.0.23.1`, PID 3981; `/var/log/kern.log:22844` and captured `dmesg.txt:557`. No Waybar process remained in the process snapshot. | Desktop bar has crashed. Inspect the crash/configuration and Wayland compatibility in a separate repair. Hyprland also logs EGL/renderer failures; the portal cannot connect to PipeWire, and no PipeWire process/socket was observed. |
| High, application degradation | **Bridge initialized before Qdrant was ready.** Bridge log lines 53 and 66 report unavailable chain-vector index and semantic shuttle at 17:58:34 and 17:58:52 UTC. Qdrant only reports gRPC listening at 17:58:56 UTC (`/var/log/qdrant/current:242`). | Qdrant is listening now. Chain projection subsequently resumes, with `write_qdrant=false`; that does not prove semantic search recovered. Current `server.rs:1075` attaches the semantic shuttle once, while `chain_vector_projector.rs:176` retries independently. Review startup readiness/recovery; an authenticated semantic-search call was not exercised. |
| High, persistent external-service failure | **Cloudflared repeatedly receives `Unauthorized: Tunnel not found`.** `/var/log/cloudflared.err`, latest inspected errors through 18:03:49 UTC. | Process remains alive but tunnel registration fails. Reconcile the configured tunnel with its account/resource through the authorized owner; do not replace credentials or alter ingress based solely on this message. This is separate from the WARP daemon. |
| Medium, persistent | **WARP's power-notifier cannot connect to D-Bus**, retrying every three seconds; `/var/log/cf-mesh/current`. | Host D-Bus listens normally. WARP uses a different mount namespace and sees a different socket device/inode at the same path. That is evidence for namespace/socket reconciliation, not proof the entire tunnel is down; WARP logs show packet traffic. |
| Medium, current | **Notebook source synchronization blocked:** `capture 0; cycle blocked (exit 1; owned_sync_journal_missing)` at 17:59:08 UTC; `/var/log/op-dbus/notebook-sources-sync/current:1`. | Supervisor being up does not establish successful synchronization. Reconcile the existing owned journal through the sync owner's supported path. Do not manufacture a journal or replay an unknown write. |
| Medium, current | **Mounted 3tchedFS invocation state read returns EIO:** `/run/mounts/3tchedFS/plugins/tched_fs/data/invocation/.value.json`. Static metadata and schema reads succeed. | Narrow state/projection fault, not a complete dead mount: mounted `notebook_list` and `notebook_query` completed with terminal receipts under the same human session. Diagnose the declared state's backing projection without routing around identity enforcement. |
| Medium, earlier failed maintenance | **`cron.daily` exited 1 at 06:22:28 EDT**; `/var/log/runit/anacron/current:5`. | Exact failing daily script not established. Unattended-upgrade log separately says all selected upgrades installed. Anacron's current down state is intentional after its one-shot run. |
| Medium, configuration gaps | SSH startup logs missing `auditd` service directory twice; D-Bus logs `Unknown username "op-dbus"`; boot and Incus report AppArmor unavailable. `/etc/machine-id` is currently absent, consistent with earlier Chromium machine-ID complaints in `/var/log/user.log`. | Review these separately against the intended deployment. Do not add identities, enable services, or weaken the active storage guard to silence them. Incus also reports absent KVM; VM capability is unavailable, while container services are running. |

Other retained observations: rejected SSH login attempts, a `cups-browsed` segfault immediately before the latest reboot, Exim using a self-signed certificate, and `saned` startup bind/Avahi failures. These do not by themselves prove a compromise or explain the current application faults.

## Recovered or nonfatal messages

- **Storage guard recovered on this boot.** Earlier logs repeatedly say `kernel/BTF requires guard policy review`. The later receipt at **17:58:17 UTC** says `READY all four LSM hooks, GLOBAL mode, exact cgroup/FSID binding set`; the supervised process is now its expected `sleep infinity`. The older denial loop is not the current state.
- Xray initially failed to create a socket lock due to permissions at 17:58:16 UTC, then logged successful startup at 17:58:17 UTC and remained running.
- OVS initially could not open `mesh-ovs`, then added that interface at 17:58:16 UTC.
- Qdrant recovered its collections and now listens on 127.0.0.1:6333/6334. It warns that `.qdrant-initialized` cannot be created and its static web UI is absent; these did not prevent listener startup.
- The bridge's first schema load took 63 ms against a 50 ms budget. Repeated missing Ghostbridge genesis/trace warnings remain; they also occur around this review's admitted read calls and are not evidence that every request failed.

## Current health and limitations

- 35 enabled runit entries running; four down: `anacron` and `dbus.dep-fixer` intentionally terminate after one-shot work; `mdadm` and `op-storage` have explicit `down` files. No restart loop established among the running services during this review.
- Expected listeners present and process-owned: web 8080, TLS door 8090, threetched 8084, Qdrant 6333/6334, private EMQX 1883/18083/8085, and bridge ExHook 9000. Listener presence is not an authenticated end-to-end readiness test.
- Around 18:04:48 UTC: 31 GiB RAM, **24 GiB available**, **0 / 8 GiB swap used**; load 0.63 / 0.63 / 0.36. Root filesystem **48% used**, about **372 GiB available**; SHM and vector filesystem about 1% used.
- No current-boot OOM, kernel panic/oops, disk I/O error, or Btrfs error found in the captured ring. Today's retained `kern.log` also has no matching Btrfs warning/error or block-device I/O error. Older archives do contain historical storage failures and are not evidence of a current recurrence.
- Kernel warning/error priorities contain ACPI MMCONFIG, missing AMD `_CPC`, and two device reset-at-boot messages. The userspace Waybar crash was found by examining the full ring.
- **Btrfs device-error counters unassessed:** `sudo -n btrfs device stats` on `/`, `/var/lib/incus/storage-pools/3tched`, and `/srv/vectors` each returned `Operation not permitted`. Caller was the authorized UID 0 diagnostic process, but the exact policy rule was not identified. No alternate cgroup, identity, transport, guard modification, or bypass was attempted. Absence of kernel errors is not a clean device-counter receipt.
- `/sys/fs/fuse/connections` is empty in this namespace despite live FUSE mounts and successful mounted operations; no queue-depth conclusion is drawn.

## Memory and handoff

| Source | Purpose / owner / scope | Retrieval and update |
| --- | --- | --- |
| Host logs and live process/mount state | Authoritative observed runtime; host operator | Read-only native diagnostics; no log contents changed |
| This report | Shared review evidence; Codex task scoped to this checkout | `reports/host-log-review-20261024.md`; add dated corrections when evidence changes |
| HANDOFF / SIGNALS / network operations document | Prior project decisions; repository owner | Searched as context, not treated as current deployment proof; unchanged |
| Mounted NotebookLM | Prior conversational context under existing human session | `notebook_list` receipt `b0bd2302-e819-48d2-9a06-1dfc633af072`; `notebook_query` receipt `5a57b20e-416d-48c3-a235-1bf5e1dd4c18`, both completed. Selected Consolidated Agent & Tool Sessions notebook returned no relevant prior context; no global absence claim |
| ECC vault | Portable memory guidance | CLI absent and no `.ecc` directory observed; no installation attempted |

Next concrete action: prioritize a separate repair of bridge/Qdrant startup recovery and the Waybar crash, then reconcile Cloudflared registration, WARP's D-Bus view, and the owned notebook sync journal. The earlier OOM needs workload-level attribution. Preserve the active kernel guard and identity boundaries throughout. No repairs or deployments are claimed by this review.

Self-review: accuracy 4/5 (root causes explicitly bounded); completeness 4/5 (console history and device counters unavailable); clarity 4/5 (UTC and EDT both recorded); actionability 4/5 (owning components and next checks named); conciseness 4/5 (detailed evidence retained here so the user-facing summary can stay short).
