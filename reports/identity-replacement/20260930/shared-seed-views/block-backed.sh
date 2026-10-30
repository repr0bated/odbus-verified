#!/bin/sh
set -eu
D=/var/tmp/opdbus-shared-seed-views-20260930/block-views
mkdir "$D"
chmod 700 "$D"
exec >"$D/experiment.log" 2>&1
set -x
S=/var/tmp/opdbus-shared-seed-views-20260930/seed.img
A="$D/leaf-a.img" B="$D/leaf-b.img"
M="$D/mounts"
mkdir -p "$M/seed-a" "$M/seed-b" "$M/a" "$M/b"
BASE= S_A= S_B= L_A= L_B=
cleanup() {
  rc=$?
  set +e
  set +x
  for m in "$M/a" "$M/b" "$M/seed-a" "$M/seed-b"; do
    mountpoint -q "$m" && { umount "$m" || rc=1; }
  done
  for l in "$L_A" "$L_B" "$S_A" "$S_B" "$BASE"; do
    [ -n "$l" ] || continue
    back=$(losetup -n -O BACK-FILE "$l" 2>/dev/null)
    case "$back" in "$S"|"$A"|"$B"|"$BASE") losetup -d "$l" || rc=1 ;; *) echo "REFUSE_DETACH $l back=$back"; rc=1 ;; esac
  done
  dmesg | tail -n 160 > "$D/kernel-tail.log"
  for img in "$S" "$A" "$B"; do
    remaining=$(losetup -j "$img")
    [ -z "$remaining" ] || { echo "CLEANUP_FAILURE $remaining"; rc=1; }
  done
  echo "EXIT=$rc"
  exit "$rc"
}
trap cleanup EXIT HUP INT TERM
hash_before=$(sha256sum "$S" | cut -d ' ' -f 1)
truncate -s 256M "$A" "$B"
chmod 600 "$A" "$B"
map_views() {
  BASE=$(losetup --find --show --read-only "$S")
  S_A=$(losetup --find --show --read-only "$BASE")
  S_B=$(losetup --find --show --read-only "$BASE")
  L_A=$(losetup --find --show "$A")
  L_B=$(losetup --find --show "$B")
  [ "$S_A" != "$S_B" ]
  for l in "$BASE" "$S_A" "$S_B" "$L_A" "$L_B"; do losetup --list --output NAME,RO,BACK-FILE "$l"; done
}
map_views
mount -o ro "$S_A" "$M/seed-a"
btrfs device add "$L_A" "$M/seed-a"
umount "$M/seed-a"
mount -o "rw,device=$S_A,device=$L_A" "$L_A" "$M/a"
printf 'block-a-private\n' > "$M/a/a.marker"
sync
umount "$M/a"
mount -o ro "$S_B" "$M/seed-b"
btrfs device add "$L_B" "$M/seed-b"
umount "$M/seed-b"
mount -o "rw,device=$S_B,device=$L_B" "$L_B" "$M/b"
printf 'block-b-private\n' > "$M/b/b.marker"
sync
umount "$M/b"
verify_pair() {
  test "$(cat "$M/a/source.marker")" = immutable-source
  test "$(cat "$M/b/source.marker")" = immutable-source
  test "$(cat "$M/a/a.marker")" = block-a-private
  test "$(cat "$M/b/b.marker")" = block-b-private
  test ! -e "$M/a/b.marker"
  test ! -e "$M/b/a.marker"
  btrfs filesystem show --raw "$M/a"
  btrfs filesystem show --raw "$M/b"
}
mount -o "rw,device=$S_A,device=$L_A" "$L_A" "$M/a"
mount -o "rw,device=$S_B,device=$L_B" "$L_B" "$M/b"
verify_pair
printf 'written-with-b-active\n' > "$M/a/concurrent-a.marker"
printf 'written-with-a-active\n' > "$M/b/concurrent-b.marker"
sync
test ! -e "$M/a/concurrent-b.marker"
test ! -e "$M/b/concurrent-a.marker"
echo SIMULTANEOUS_BLOCK_VIEWS_AND_WRITES=pass
umount "$M/a"
mount -o "rw,device=$S_A,device=$L_A" "$L_A" "$M/a"
verify_pair
test "$(cat "$M/a/concurrent-a.marker")" = written-with-b-active
echo A_REOPEN_WITH_B_ACTIVE=pass
umount "$M/b"
mount -o "rw,device=$S_B,device=$L_B" "$L_B" "$M/b"
verify_pair
test "$(cat "$M/b/concurrent-b.marker")" = written-with-a-active
echo B_REOPEN_WITH_A_ACTIVE=pass
umount "$M/a"
umount "$M/b"
for l in "$L_A" "$L_B" "$S_A" "$S_B" "$BASE"; do losetup -d "$l"; done
L_A= L_B= S_A= S_B= BASE=
map_views
mount -o "rw,device=$S_B,device=$L_B" "$L_B" "$M/b"
mount -o "rw,device=$S_A,device=$L_A" "$L_A" "$M/a"
verify_pair
test "$(cat "$M/a/concurrent-a.marker")" = written-with-b-active
test "$(cat "$M/b/concurrent-b.marker")" = written-with-a-active
test ! -e "$M/a/concurrent-b.marker"
test ! -e "$M/b/concurrent-a.marker"
echo COLD_REVERSE_BLOCK_VIEW_ORDER=pass
umount "$M/a"
umount "$M/b"
hash_after=$(sha256sum "$S" | cut -d ' ' -f 1)
test "$hash_before" = "$hash_after"
echo "SEED_HASH_UNCHANGED=pass $hash_after"
