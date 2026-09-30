# Focused source validation and profiling

Run from `/srv/git/odbus-verified`:

```sh
reports/btrfs-cache-memory/20260930-implementation/snowball/build-tests.sh
reports/btrfs-cache-memory/20260930-implementation/snowball/focused-tests pending_ --test-threads=1
reports/btrfs-cache-memory/20260930-implementation/snowball/focused-tests bounded_ --test-threads=1
for filter in block_ref_reads embedding_text_is long_fields_are; do reports/btrfs-cache-memory/20260930-implementation/snowball/focused-tests "$filter" --test-threads=1; done
reports/btrfs-cache-memory/20260930-implementation/snowball/build-profile.sh
for repeat in 1 2 3; do for version in before after; do timeout --signal=TERM --kill-after=5s 120s reports/btrfs-cache-memory/20260930-implementation/snowball/profile-$version 10000 > reports/btrfs-cache-memory/20260930-implementation/snowball/raw/profile-$version-r$repeat.log 2>&1; done; done
```

Build scripts record exact GNU rustc target, existing extern artifacts, native archives and one process at a time, with five-minute build timeout. Profiles enforce 512MiB RSS at checkpoints, at most 10,000 small synthetic records (under256MiB), and two-minute process timeout. Reproductions should use fresh log destinations to preserve previous evidence. No Python, broad Cargo invocation, live history deletion or external provider call was used.
