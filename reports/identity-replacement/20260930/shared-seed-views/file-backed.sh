#!/bin/sh
set -eu
D=/var/tmp/opdbus-shared-seed-views-20260930
mkdir "$D"
chmod 700 "$D"
exec >"$D/experiment.log" 2>&1
set -x
S="$D/seed.img" A="$D/leaf-a.img" B="$D/leaf-b.img"
M="$D/mounts"
mkdir -p "$M/build" "$M/seed-a" "$M/seed-b" "$M/a" "$M/b"
S_BUILD= S_A= S_B= L_A= L_B=
cleanup() {
  rc=$?
  set +e
  set +x
  for m in "$M/a" "$M/b" "$M/seed-a" "$M/seed-b" "$M/build"; do
    mountpoint -q "$m" && umount "$m"
  done
  for l in "$L_A" "$L_B" "$S_A" "$S_B" "$S_BUILD"; do
    [ -n "$l" ] || continue
    back=$(losetup -n -O BACK-FILE "$l" 2>/dev/null)
    case "$back" in "$S"|"$A"|"$B") losetup -d "$l" ;; *) echo "REFUSE_DETACH $l back=$back"; rc=1 ;; esac
  done
  dmesg | tail -n 150 > "$D/kernel-tail.log"
  for img in "$S" "$A" "$B"; do
    [ -e "$img" ] || continue
    remaining=$(losetup -j "$img")
    [ -z "$remaining" ] || { echo "CLEANUP_FAILURE $remaining"; rc=1; }
  done
  echo "EXIT=$rc"
  exit "$rc"
}
trap cleanup EXIT HUP INT TERM
truncate -s 256M "$S" "$A" "$B"
chmod 600 "$S" "$A" "$B"
S_BUILD=$(losetup --find --show "$S")
mkfs.btrfs -q -f -L shared-seed-views "$S_BUILD"
mount "$S_BUILD" "$M/build"
printf 'immutable-source\n' > "$M/build/source.marker"
sync
umount "$M/build"
losetup -d "$S_BUILD"
S_BUILD=
btrfstune -S 1 "$S"
hash_before=$(sha256sum "$S" | cut -d ' ' -f 1)
echo "KERNEL=$(uname -r) BTRFS=$(btrfs --version) SEED_INODE=$(stat -c '%d:%i' "$S") SEED_HASH=$hash_before"
map_views() {
  S_A=$(losetup --find --show --read-only "$S")
  S_B=$(losetup --find --show --read-only "$S")
  L_A=$(losetup --find --show "$A")
  L_B=$(losetup --find --show "$B")
  [ "$S_A" != "$S_B" ]
  losetup --list --output NAME,RO,BACK-FILE | tee -a "$D/views.log"
}
map_views
mount -o ro "$S_A" "$M/seed-a"
btrfs device add "$L_A" "$M/seed-a"
umount "$M/seed-a"
mount -o "rw,device=$S_A,device=$L_A" "$L_A" "$M/a"
printf 'a-private\n' > "$M/a/a.marker"
sync
btrfs filesystem show --raw "$M/a"
umount "$M/a"
mount -o ro "$S_B" "$M/seed-b"
btrfs device add "$L_B" "$M/seed-b"
umount "$M/seed-b"
mount -o "rw,device=$S_B,device=$L_B" "$L_B" "$M/b"
printf 'b-private\n' > "$M/b/b.marker"
sync
btrfs filesystem show --raw "$M/b"
umount "$M/b"
verify_pair() {
  test "$(cat "$M/a/source.marker")" = immutable-source
  test "$(cat "$M/b/source.marker")" = immutable-source
  test "$(cat "$M/a/a.marker")" = a-private
  test "$(cat "$M/b/b.marker")" = b-private
  test ! -e "$M/a/b.marker"
  test ! -e "$M/b/a.marker"
  btrfs filesystem show --raw "$M/a"
  btrfs filesystem show --raw "$M/b"
}
mount -o "rw,device=$S_A,device=$L_A" "$L_A" "$M/a"
echo SIMULTANEOUS_A=pass
if mount -o "rw,device=$S_B,device=$L_B" "$L_B" "$M/b"; then
  echo SIMULTANEOUS_B=pass
else
  rc=$?
  echo "SIMULTANEOUS_B=fail status=$rc"
  exit "$rc"
fi
verify_pair
echo ISOLATION=pass
umount "$M/a"
mount -o "rw,device=$S_A,device=$L_A" "$L_A" "$M/a"
verify_pair
echo A_REOPEN_WITH_B_ACTIVE=pass
umount "$M/b"
mount -o "rw,device=$S_B,device=$L_B" "$L_B" "$M/b"
verify_pair
echo B_REOPEN_WITH_A_ACTIVE=pass
umount "$M/a"
umount "$M/b"
for l in "$L_A" "$L_B" "$S_A" "$S_B"; do losetup -d "$l"; done
L_A= L_B= S_A= S_B=
map_views
mount -o "rw,device=$S_B,device=$L_B" "$L_B" "$M/b"
mount -o "rw,device=$S_A,device=$L_A" "$L_A" "$M/a"
verify_pair
echo COLD_REVERSE_ORDER=pass
umount "$M/a"
umount "$M/b"
hash_after=$(sha256sum "$S" | cut -d ' ' -f 1)
test "$hash_before" = "$hash_after"
echo "SEED_HASH_UNCHANGED=pass $hash_after"
