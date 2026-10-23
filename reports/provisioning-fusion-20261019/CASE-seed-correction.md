# REQUIRED FIRST PANEL ROUND — owner corrects seed/sprout interpretation

# Latest owner correction — required panel emphasis

Owner statements, in order:

- “the seed is the lbacking . by adding the btrfs as a device to acieve the rw”
- “that is haow it worked before”
- “enphasooze that to the panel”
- “specy the command btrfs device add”

The mechanism to investigate and preserve is explicitly:

```sh
btrfs device add <writable-device> <mounted-seed-path>
```

This is explanatory command syntax, NOT an authorized command to execute and NOT a selection of actual device/path arguments. The immutable seed supplies the existing backing. Adding the writable device is how the previously working seed/sprout arrangement acquires writable storage. Trace the old provisioning implementation, its identity/Cozo association, and the subsequent sprout mount/remount; do not substitute an OverlayFS writable upper or copied root.

The parent withdraws its implication that the owner's requested Incus/subvolume arrangement must be evaluated as though each subvolume itself had to be a standalone block device passed to device-add. Separate the seed backing, writable device/member, Cozo identity association, exposed subvolumes and upgrade replica. Do not turn a statement about the command's operand type into a blanket feasibility verdict about the complete arrangement.

No host loop devices remains mandatory. Prefer no disk images and subvolumes only if the retained semantics can be preserved. Existing scripts may contain historical loops; trace their role without silently preserving or reintroducing them. Investigate exact implementation constraints, but do not erase the owner-confirmed previously working mechanism or demand an architecture replacement from an unverified interpretation.

Panel must explicitly acknowledge `btrfs device add`, identify what the actual old script added and where, distinguish historical implementation from proposed no-loop changes, and revise any conclusion based on the incorrect layer conflation. JEV is decision/evidence judge only, not routing. This correction authorizes no storage mutation, provisioning replay, source repair, build, test or deployment.

## Original implementation evidence, not a command to execute

Commit26a4ad132fbbd4523b05724060bfdeb9ca661824, deploy/runit/libexec-3tched/op-identity-root:

```text
1172:    mount -t btrfs -o "ro,subvolid=5,device=$seed_view" "$seed_view" "$seed_dir/volume"
1173:    # Validate the inherited siblings before creating the sprout. Nothing is
1174:    # copied or snapshotted; all writes after ADD_DEV go to this new leaf.
1175:    initial_members=$(seed_layout_pins) || seed_fail "seed lacks seven distinct sibling subvolumes"
1176:    initial_image=$(seed_image_metadata verify) || seed_fail "seed image verification failed before device add"
1177:    seed_write --argjson image "$initial_image" '.image=$image'
1178:    [ "$(btrfs property get -ts "$seed_dir/volume/@" ro)" = ro=false ] || seed_fail "seed @ must be a normal writable subvolume"
1179:    [ -d "$seed_dir/volume/@/etc" ] && [ ! -L "$seed_dir/volume/@/etc" ] || seed_fail "seed root lacks safe etc directory"
1180:    seed_write --argjson members "$initial_members" '.members=$members'
1181:    current_seed_super=$(btrfs inspect-internal dump-super "$seed_device")
1182:    [ "$(blockdev --getro "$seed_device")" = 1 ] || seed_fail "physical seed lost block read-only enforcement"
1183:    printf '%s\n' "$current_seed_super" | grep -q SEEDING &&
1184:        [ "$(printf '%s\n' "$current_seed_super" | awk '$1 == "fsid" {print $2; exit}')" = "$source_fsid" ] &&
1185:        [ "$(printf '%s\n' "$current_seed_super" | awk '$1 == "generation" {print $2; exit}')" = "$source_generation" ] || seed_fail "seed changed before sprout creation"
1186:    seed_no_direct_mount
1187:    seed_write '.phase="device-add-intent"'
1188:    btrfs device add "$leaf_loop" "$seed_dir/volume"
1189:    sync -f "$seed_dir/volume"
1190:    umount "$seed_dir/volume"
1191:    sprout_fsid=$(seed_super "$leaf")
1192:    valid_session "$sprout_fsid" && [ "$sprout_fsid" != "$source_fsid" ] || seed_fail "invalid new sprout FSID"
1193:    seed_write --arg fsid "$sprout_fsid" '.sprout_fsid=$fsid | .phase="layout-intent"'
1194:    mount -t btrfs -o "rw,subvolid=5,device=$seed_view,device=$leaf_loop" "$leaf_loop" "$seed_dir/volume"
1195:    seed_verify_volume
1196:    [ "$(seed_layout_pins | jq -cS .)" = "$(printf '%s\n' "$initial_members" | jq -cS .)" ] || seed_fail "inherited sibling identities changed after sprout creation"
```

Hermes original acknowledgments:

[
  {
    "id": 4095,
    "role": "user",
    "content": "no the persistent cozo iws addedwith btrfs add disk"
  },
  {
    "id": 4099,
    "role": "assistant",
    "content": "Right—the persistent writable storage is added to the seed with btrfs device add, and Cozo tracks the identity’s leaf assignment. It is not a writable OverlayFS upper layer.\n\nThe separate upgrade partition supplies upgrade/@ for the read-only overlay. I conflated those two roles."
  },
  {
    "id": 4111,
    "role": "assistant",
    "content": "Yes—the script uses btrfs device add to attach the persistent root partition to the seed, then mounts that pair read-write.\n\n  prepare-instance-disk.sh:111 — adds the root partition.\n  prepare-instance-disk.sh:116–118 — formats the separate upgrade partition and creates @.\n  bind-instance-root.sh:216 — combines upgrade and seed layers into a read-only overlay.\n\nBut the storage path is still wrong: the script requires /var/lib/3tched-leaf-pool, rather than your selected /var/lib/incus/storage-pools/3tched-pool. The layout is implemented; that integration is not finished."
  }
]

## Required panel and continuation actions

This fresh outer session is intentional so the first required Fusion panel receives this owner correction verbatim. First panel MUST name `btrfs device add`, acknowledge the demonstrated mechanism, and distinguish backing/member from exposed subvolumes. Do not extrapolate an operand-type fact into rejection of the complete arrangement. Save its original responses and actual successful/failed members to analysis/seed-mechanism-panel-receipt.json. Do not claim consensus from one successful model.

The prior resumed runner was stopped only to deliver this latest owner correction, not because provisioning failed. Preserve all completed semantic reviews and four source-research outputs. Read analysis/recovery-semantic-coverage.json using its actual keys completedPackets, pending and supplements_pending. Reuse completed reviews; do not re-export histories, regenerate packets or redo them. Continue unfinished review and revise diagnosis/proposed-changes/drift-ledger to distinguish actual findings from the withdrawn layer inference. Parent judgment jev-seed-mechanism-correction.json is available. Old no-host-loop and no-image/subvolume preference remain, and no storage operation is authorized.

Execution: NO new agents/workpools, only one explicit native scout/smol completion at a time for packet analysis; retain existing actual model receipts. Fusion remains outer synthesis, JEV evidence/decision judge only. Do not call internal Fusion after its per-request instructions prohibit it.

# Provisioning Fusion: complete history, drift and evidence-backed decisions

Work in `/srv/git/odbus-verified`. Task: `provisioning-fusion-20261019`.

The owner wants LiteLLM Fusion to diagnose why the existing identity-container provisioning workflow remains unfinished, using the latest relevant Astra, Claude and Kiro conversations and especially the COMPLETE stored Hermes/Astra Pro audit and follow-through, including tool calls, arguments, results, errors, worker histories and owner corrections. Previous provisioning scripts are evidence, not commands to replay. Do not replace this task with another partial component audit.

## Latest owner instructions — override historical interpretations

1. **“no loops  on host”**: NO HOST LOOP DEVICES. Not merely no per-container loops. Do not silently exempt the existing pool loop, seed wrappers or a mapping described as stored in Cozo. Existing pool backing is a recorded conflict, not permission to detach it.
2. **“woould prfer no  disk image if possibgle to use subvolumes olnly”**: prefer subvolumes only, without disk images, if compatible with the complete required behavior. This is a preference, not permission to silently replace immutable-seed/sprout semantics with copied roots or snapshots.
3. JEV is the **decision judge**, NOT model routing. Never activate `jev-router`, class-router, a Cargo model dispatcher or Haiku fallback. Judge substantive proposals, evidence sufficiency, drift and completion with saved questions/answers and citations.
4. Bootstrap already exists and is out of scope. Preserve existing identity/session/native UUID/creation records. Modify the design of the EXISTING Hermes workflow, not a new provisioning script, orchestrator, endpoint or control plane.

Retain immutable shared seed, isolated persistent writes, separate upgrade replica, Btrfs send/receive, correct root composition, unprivileged Incus container, native instance/profile declarations, identity admission, storage proofs, durable intents/recovery, final authoritative adoption and startup. Do not introduce new LVM management or VM conversion. Do not infer that Cozo metadata itself supplies a kernel block device. Conversely, do not declare native Incus integration impossible merely because a familiar image-based implementation cannot do it: investigate the actual older scripts and exact supported primitives.

## Scope and safety

This run is **read-only diagnosis and a concrete modification specification**. You may write analysis, coverage and decision receipts only under `reports/provisioning-fusion-20261019/analysis/`. Do not edit application or host source, shared HANDOFF/SIGNALS owned by the parent, grants, credentials, service settings or storage. Do not run provisioning helpers, storage operations, builds, tests, formatters, deployments, service actions, or historical commands. Existing historical approvals are not current authorization. No direct database mutations or alternate-identity bypasses. Do not read authentication stores or unredacted history; use the supplied exports. No missing-prerequisite claim until evidence distinguishes a genuine current blocker from a stale or invented one.

## Required workflow

- Read the latest constraint files and corpus manifest. Write `analysis/ingestion-receipt.json` early, listing exact files/record IDs actually read, the two latest owner constraints, the judge-only role and known coverage gaps. A manifest read is NOT complete history ingestion.
- Review the complete supplied Hermes main conversation and all linked workers, with the Astra, Claude and Kiro records. Use bounded batches/read-only research workers if needed. All workers must preload the ten mandatory repo skills and preserve this scope. Retain physical record order, IDs, parent links, correction events, active/compacted flags and source dates. Exact duplicate content can be read once only if every duplicate reference remains accounted for.
- Maintain `analysis/coverage.json`: file hashes, covered record/byte ranges, duplicates, unavailable/truncated content and outstanding ranges. The corpus is too large for one model context. Never silently truncate it or claim every panel member read a file merely because a path was supplied.
- The Fusion outer agent has tools; individual panel/analyst calls are stateless. Include the relevant ORIGINAL evidence excerpts and IDs in each panel question, not just filesystem paths or an outer-agent summary. Save which evidence actually reached each panel round. Separate exported/available/read/panel-reviewed/JEV-adjudicated status.
- Trace the complete existing post-bootstrap chain: admission → native preparation → storage establishment → initial upgrade receive/pairing → root binding → authoritative Cozo adoption → start → stopped boot restoration/sync → failure/cancellation/resume. Identify the owner and authoritative receipt at every stage.
- Compare current source snapshots, installed application helpers, installed base boot service and immutable historical commit blobs. They are not interchangeable. Where current state needs inspection, use read-only source inspection; runtime operations outside supplied receipts require a separately authorized follow-up, not speculative replay.
- Build an explicit drift ledger: original ask; agent interpretation; actual tool action/result; owner correction; propagation into code/delegates/deployment; remaining discrepancy. Distinguish legitimate clarification, agent-added work, stale evidence, genuine encountered failures and incomplete execution.
- Use JEV through native typed `judge`/`judge_batch` with the configured `typesafe/jev-latest` judge role. Read `xd://eval/judge`. For each substantive decision, save exact state/evidence refs, question/rubric, returned answer and uncertainty. Never use JEV to choose models or to grant mutation authority. Existing parent judgments cover scope and the no-host-loop rule, not root-cause or design feasibility.
- Produce `analysis/diagnosis.md`, `analysis/drift-ledger.md`, `analysis/proposed-changes.md`, `analysis/decisions.jsonl`, final coverage and completion receipts. Concrete modifications must target existing scripts/modules, include exact proposed Incus declarations when supported, retain security checks, and expose unresolved requirement conflicts. A guessed YAML configuration is not a solution.
- If subvolume-only native storage cannot preserve a required semantic, show the specific evidence and smallest decision needed. Do not silently relax no-host-loops, invent a pool-loop exemption, remove the seed contract, or call a copied root equivalent.

Stop when the diagnostic/specification deliverables are complete and their limitations explicit. Do not start implementation or provisioning. Do not report the chatbot provisioned, built, deployed, started or tested based on this run.

---
# Evidence index and preliminary anchors

Evidence root: `/srv/git/odbus-verified/reports/provisioning-fusion-20261019/`.
Repository snapshot commit: `051a20d056afba074e21d797aa34f8947c543c5a`. Working-tree snapshots include later uncommitted changes and are NOT that commit's contents.

Read `manifest.json`, `latest-owner-correction.json`, `subvolume-preference.json`, `jev-initial-decision.json`, `jev-no-host-loops.json` first. The source transcripts, not these preliminary anchors, are the evidence for conclusions.

## Complete stored conversation exports

**42 selected session histories, 6,620 public message/event records.** Hidden reasoning and credential metadata are excluded; credential-like content is redacted. Historical tool-output truncation remains a declared limitation, not silently reconstructed text.

### Hermes — main priority

`evidence/hermes/manifest.json` maps 35 sessions and 4,462 message rows: full main audit/follow-through, all 30 recursively linked workers, two preceding failed audit starts and two later failed handoff attempts. `delegations.jsonl` preserves 23 delegation records. No active-only filtering; compacted/inactive copies are retained with provenance.

Main: `evidence/hermes/20261018_190519_b780ef.jsonl`, actual model `openai/gpt-6-astra-pro`, Oct18 19:05:49Z–Oct19 01:29:54Z. 1,778 rows: 152 user, 480 assistant, 1,146 tool results. Some role=user entries are injected worker messages, not new owner instructions.

Useful ORIGINAL message IDs, not a replacement for full reading:
- 23 audit request; 216 D-Bus first; 617 fix.
- 1123/1125/1171 and 1435/1437 numbering/display naming; 2655 explicitly says numbering is only Incus display for humans. 2658/2663 acknowledge overcomplication and unnecessary boot restrictions. Inspect correction patches/delegates, not just apology.
- 4012–4032: owner asks whether provisioning happened; assistant admits no genuine blocker had been established and unfinished checks were presented as blockers.
- 4091/4095/4099: persistent writable leaf was conflated with an overlay upper, then corrected.
- 4111: implemented layout but wrong pool integration acknowledged. 4112/4120: upgrade send/receive each boot and pairing at provisioning. 4123/4125 explicitly acknowledge component-only audit missed complete integration.
- 4127 delegates actual repair; subsequent real admission failures must not be erased. Later Oct19 repairs supersede the final historical denial.

### Astra

`evidence/astra/2026-10-19T02-58-21-842Z_01a15218-8812-74df-8b47-4bcae55ac905.jsonl` plus its two worker exports and the 02:28 seed/FUSE session. Actual model `openai-codex/gpt-6-astra`.

**87 owner corrections in the main session are `custom_message/live-delegation`, not ordinary user-role messages.** Preserve them. The final prompt `e1aa9d0e`, original physical line1204, is separately available as `evidence/astra/latest-kiro-brief.txt`; its narrower loop wording is superseded by the latest owner constraint above.

Anchors: actual installed helper failure `e84b7ba8` (`fsconfig: File exists`), failed blank disk deletion `47e15770`, temporary seed-loop cleanup `54f08ba3`; worker edits stopped unbuilt/undeployed. Rejected extra egress and unrelated network daemon were removed in the earlier session. Do not reintroduce either as a prerequisite.

### Claude

`evidence/claude/f4c461eb-d25f-4247-9bd7-eddda9ade635.jsonl` and linked worker `agent-a6b199e4c3071de99.jsonl`. Main actual `claude-opus-5-5`; worker `claude-sonnet-5-5`. Queued owner corrections and approval tool results are retained.

Original request to read Hermes history instead caused a read of an older Claude network session. Native creation later succeeded, but storage/binding/adoption remained separate. Historical `seed is not sealed` was genuine; claiming old missing `/dev/sdc2` was the required seed was retracted. A background wrapper reported exit0 while its build receipt says exit101; judge the underlying command, not notification status.

### Kiro

`evidence/kiro/sess_e49a5a84-8491-4c1d-987a-a3b2916a8615.jsonl`, Oct19 04:02–04:18Z. Dispatched `claude-opus-5.5` (provider metadata qdev). User steering and all tool receipts retained; hidden Reasoning omitted. Only one public assistant utterance was stored; no completed specification. The only write was cancelled before execution.

Latest corrections: old provisioning worked; inspect prior scripts; persistent device added to seed supplies RW; seed remains permanently mounted and immutable; no retiring seed; no requirement that device-add use a loop. Do not manufacture a public Kiro statement from hidden reasoning. User's later current no-host-loop instruction has highest precedence.

## Source, installed and historical versions

`manifest.json` maps **49 current/installed snapshots** and **65 historical blobs** from Oct5 admission/helper, Oct11 successful-start and boot-persistence checkpoints, and pre-Hermes HEAD. Seven paths did not yet exist at older commits; extraction errors are recorded rather than replaced by modern content.

- Current worktree: `evidence/source/`.
- Installed application helpers and base service: `evidence/installed/`.
- Immutable historical commit blobs: `evidence/history/`.
- Full research indices with source IDs: `evidence/research/{AstraHistory,ClaudeKiroHistory,ProvisionScriptHistory}.md`.
- Dated shared context snapshots: `evidence/context/{HANDOFF,SIGNALS}.md`; these are historical claims, not fresh runtime verification.

Existing call chain:
`provision-workspace-subscriber` → `prepare-instance-disk` → `sync-instance-upgrade` → `bind-instance-root` → `prepare-chatbot-adoption` → `identity_sled.provision_container` / Rust native+storage proof / Cozo final CAS.
Boot: `identity-roots` → `restore-chatbot-root` → established-storage restoration.

Preliminary source-backed mismatches to verify in context:
- Most Oct19 drafts use the new pool/upgrade FSID; sync still uses old pool/FSID.
- Installed base `/etc/sv/identity-roots/run` remains old single-chatbot/early-exit wiring, distinct from application helper staging.
- Both old root helper and current GPT proof rely on host loops. Cozo association alone does not remove those mappings.
- Oct5 manual working leaf and scripted candidate receipts were unreconciled. Oct11 has a canonical-start/guest-identity receipt; cold reboot was not exercised. Neither proves today's requested subvolume-only/no-host-loop architecture already works.

## Last recorded target and limits

Native `3tchedCtlChatbot`, logical ID0, UUID `137c61ff-435c-4300-b3c0-c605e461be13`, stopped/never-used in latest receipts, root still directory pool `identity-roots`. Native creation is not authoritative identity adoption. Existing ordinary names begin ID-1; naming is not identity authorization.

Historical pool `3tched`: Btrfs, 150GiB, backing `/var/lib/incus/disks/3tched.img` on `/dev/loop0`; no managed volumes listed in Kiro receipt. **This conflicts with the latest no-host-loops requirement; do not exempt it.** No pool alteration authorized by this investigation.

Seed `8468cc14-a5e4-409c-96f6-1e157d470ab5`, SEEDING, read-only, seven original subvolumes. Upgrade filesystem `7ac6c955-4028-439c-80b4-c55a647abcb4`, filesystem writable, `@` read-only in latest recorded preparation. Failed blank image deleted; retained filesystem-prepare-intent is NOT completion.

Bootstrap/self-read admission denial was repaired after Hermes; native creation followed. Do not recycle the last Hermes denial as current root cause. Application release was deployed; this does not mean chatbot storage/adoption/start completed. No guest boot or sole8090 network completion proof is supplied. Guard/AppArmor protections require separate runtime evidence, not assumed from source.

Keep same-handle operation receipts, exact identity/native/storage binding, root-private locked plans/intents, pre-receive destination proof, immutable seed, one-time explicit membership establishment, no lifecycle member churn, unprivileged/nonnested isolation, storage ioctl restrictions, stopped overlay updates, ancestry proof and final digest/revision CAS. Preserve security fixes while resolving architectural drift.
