# Reproduction and continuation

Run source verification from `/srv/git/odbus-verified` with native `/usr/bin/rustc`
(1.97.1, x86_64-unknown-linux-gnu), `/usr/bin/cargo`, one build job and no
incremental cache. `Cargo.lock` is unchanged.

```sh
env PATH=/usr/bin:/bin CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 \
  CXXFLAGS='-include cstdint' cargo check --locked -p op-cache --lib
```

After that successful build, this run reused its same-version GNU RocksDB
archive to avoid rebuilding 318 native translation units for each Cargo
feature graph. The exact artifact is
`target/debug/build/cozorocks-b295ed40898fbbf1/out/librocksdb.a`; source/archive
hashes are recorded alongside the final source provenance. Set
`ROCKSDB_LIB_DIR` to that absolute `out` directory and `ROCKSDB_STATIC=1` for the
following commands, or omit both to rebuild the dependency normally.

```sh
# Isolated ordinary filesystem is sufficient for op-cache's nonignored tests.
TMPDIR=/dev/shm cargo test --locked -p op-cache --lib
# Snowball's nonignored lifecycle fixtures require a real Btrfs temp directory.
TMPDIR=/var/tmp cargo test --locked -p op-snowball --lib -- --test-threads=1
cargo test --locked -p op-grpc-bridge --lib model_registry_dispatch::tests -- --test-threads=1
```

The environment above (`PATH`, `CARGO_BUILD_JOBS`, `CARGO_INCREMENTAL`,
`CXXFLAGS`, optional supported native library override) applies to each Cargo
command. Use dedicated disposable fixtures only. Do not run native tests
against a real cache or a received release.

Exact-source fallback probes retain dependency selections from the baseline:

- `verification/test-cache.sh`: cache/process/snapshot/NUMA unit wrapper.
- `transport/build.sh`, then `sudo -n env PATH=/usr/bin:/bin timeout -k 5 120 transport/native`
  from this report directory: real disposable six-member send/receive cycles,
  cancelled source creation and partial received cleanup.
- `numa/run-tests.sh` and `numa/run-native-policy.sh`: kernel/readback checks.
- `snowball/COMMANDS.md`: paired real source pending-scan profiles and regressions.
- `scripts/probes/cache-memory-api.sh` with
  `OPDBUS_CACHE_MEMORY_ARTIFACT_RECEIPT=reports/btrfs-cache-memory/20260930-followup`:
  current cache API profiles. The script records all selected artifact hashes.
- `cache-validation/final-*.log`: retained-allocation and paired warm-read evidence;
  see `cache-notes.md` for exact workload and qualifications.

The eBPF programs in `verification/trace-cache.bt` use bpftrace against a child
launched with `-c`. The run used a private `unshare --mount --propagation private`
namespace and mounted tracefs there only. The aggregate counters are not a
live-allocation profiler. Do not replay them against the production bridge and
interpret turnover as a leak.

Repository-wide verification was attempted with:

```sh
env PATH=/usr/bin:/bin CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 \
  CXXFLAGS='-include cstdint' sh scripts/check-workspace.sh verify
```

It stops at workspace formatting drift. Do not erase another agent's work to
make this gate pass. Logs are under `verification/`.

## Skills actually read

- `/srv/git/odbus-verified/.agents/skills/grpc-expert/SKILL.md`
- `/srv/git/odbus-verified/.agents/skills/grpc-protocol-expert/SKILL.md`
- `/srv/git/odbus-verified/.agents/skills/json-render/SKILL.md`
- `/srv/git/odbus-verified/.agents/skills/ovs-db-analysis/SKILL.md`
- `/home/jeremy/.claude/remote/plugins/a3f7d226a8113882/skills/debug/SKILL.md`
- `/home/jeremy/.claude/remote/plugins/a3f7d226a8113882/skills/testing-strategy/SKILL.md`

These are actual available skills, not invented Rust/Btrfs/NUMA skill names.
Version-specific behavior was checked in local source/kernel readback and
bpftrace's official 0.27 documentation. The mandatory repository skills were
also loaded independently by delegated agents.
