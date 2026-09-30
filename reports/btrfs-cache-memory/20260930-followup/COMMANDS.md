# Exact commands for the bounded API suite

Run from `/srv/git/odbus-verified`. Shell scripts retain their exact environment, rustc flags, dependency fingerprints, watchdog bounds and case sequence. One rustc process at a time; no Cargo or Python invocation.

```sh
reports/btrfs-cache-memory/20260930-followup/build.sh > reports/btrfs-cache-memory/20260930-followup/raw/build.log 2>&1
reports/btrfs-cache-memory/20260930-followup/build.sh wrapper-mitigated.rs cache-probe-mitigated > reports/btrfs-cache-memory/20260930-followup/raw/build-mitigated.log 2>&1
reports/btrfs-cache-memory/20260930-followup/run-all.sh > reports/btrfs-cache-memory/20260930-followup/raw/run-all.log 2>&1
reports/btrfs-cache-memory/20260930-followup/build.sh wrapper-cold.rs cache-probe-cold > reports/btrfs-cache-memory/20260930-followup/raw/build-cold.log 2>&1
reports/btrfs-cache-memory/20260930-followup/run-cold.sh > reports/btrfs-cache-memory/20260930-followup/raw/run-cold.log 2>&1
reports/btrfs-cache-memory/20260930-followup/summarize.sh > reports/btrfs-cache-memory/20260930-followup/summary.tsv
sha256sum crates/op-cache/src/{btrfs_cache.rs,numa.rs,snapshot_manager.rs} reports/btrfs-cache-memory/20260930-followup/source/*.rs > reports/btrfs-cache-memory/20260930-followup/source-after.sha256
diff -u reports/btrfs-cache-memory/20260930-followup/source-before.sha256 reports/btrfs-cache-memory/20260930-followup/source-after.sha256
```

A build has a 300-second timeout. Each case has its own 120-second watchdog and outer 125-second timeout, sampling RSS every 100 ms with a 524,288 KiB stop. Checkpoints enforce the same RSS limit. Fixed fixtures contain 16 keys × 4 KiB or 64 KiB vectors; growth is bounded to 256 rows/240 long texts, not a load-growth sweep. Calibration holds at most ten MiB. These bounded workloads stay far below the 256 MiB data limit. The watchdog is sampling, not a kernel RSS hard limit.

For reproduction, preserve existing evidence by moving it into a new run directory or adapting the log destination before running again; the commands above reproduce their original filenames. The initial failed orchestration logs were moved to `raw/initial-watchdog-race/` before the complete run. `probe-main.rs` is an identical archival copy of the source used for the main and mitigation binaries; `probe-cold.rs` adds only the cold case and full-vector recovery assertion.

Shell syntax verification (completed):

```sh
bash -n reports/btrfs-cache-memory/20260930-followup/build.sh reports/btrfs-cache-memory/20260930-followup/run-one.sh reports/btrfs-cache-memory/20260930-followup/run-all.sh reports/btrfs-cache-memory/20260930-followup/run-cold.sh reports/btrfs-cache-memory/20260930-followup/summarize.sh
```

No full-workspace verification is claimed here; the parent investigator owns that gate. Existing probes, their evidence and production files outside this new report directory were preserved.
