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
