# Spec 10: Golden Deployment Pipeline

**Spec Path**: [`deploy/runit/build-golden.sh`](file:///srv/git/odbus/deploy/runit/build-golden.sh) & [`deploy/btrfs-layout.sh`](file:///srv/git/odbus/deploy/btrfs-layout.sh)  
**Domain**: Host Release & BTRFS Subvolume Deployment  
**Status**: **PASS (Verified & Dry-Run Tested)**

---

## Requirements Verification Matrix

| Requirement | Requirement Statement | Code Implementation | Status |
|---|---|---|:---:|
| **REQ-1** | Single release compilation: `CXXFLAGS="-include cstdint" cargo build --workspace --release`. | Verified (41 release binaries in `target/release`). | **PASS** |
| **REQ-2** | Destination subvolume (`/opt/op-dbus/golden`) MUST be on BTRFS filesystem (`stat -f -c %T`). | [`deploy/runit/build-golden.sh:106-110`](file:///srv/git/odbus/deploy/runit/build-golden.sh#L106-L110). | **PASS** |
| **REQ-3** | Golden subvolume MUST write `MANIFEST` with commit, build timestamp, and per-binary SHA-256. | [`deploy/runit/build-golden.sh:167-178`](file:///srv/git/odbus/deploy/runit/build-golden.sh#L167-L178): Emits SHA-256 hash manifest. | **PASS** |
| **REQ-4** | Live installation preserves host-modified `/etc/runit/sv/<svc>/run` files. | [`deploy/runit/build-golden.sh:259-262`](file:///srv/git/odbus/deploy/runit/build-golden.sh#L259-L262): Leaves modified host versions alone. | **PASS** |
| **REQ-5** | Network-critical services held back from automatic restart (`NEVER_AUTO_RESTART`). | [`deploy/runit/build-golden.sh:188-190`](file:///srv/git/odbus/deploy/runit/build-golden.sh#L188-L190). | **PASS** |
