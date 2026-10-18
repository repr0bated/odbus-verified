#!/bin/bash
# Build three independent boot artifacts for the running xanmod kernel:
#   initramfs-opdbus.img   mkinitcpio image with the opnet toolbox
#   opdbus.efi             UKI (kernel + that initramfs + cmdline)
#   opdbus-efi-boot.img    FAT ESP disk image (grub + kernel + initramfs, no UKI)
# The UKI is also copied to the ESP and registered as a non-default EFI entry.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")" && pwd)"
KVER="$(uname -r)"
VMLINUZ="/boot/vmlinuz-linux-xanmod-edge-linux-bin-x64v3"
OUT="/var/lib/opdbus-boot"
ESP="/boot/efi"
DISK="/dev/sda"
PART=2

if [[ ! -f "$VMLINUZ" ]]; then
    echo "missing kernel $VMLINUZ" >&2
    exit 1
fi

SRC="$(findmnt -no SOURCE /)"
DEV="${SRC%%[*}"
UUID="$(blkid -s UUID -o value "$DEV")"
FSROOT="$(findmnt -no FSROOT /)"
SUBVOL="${FSROOT#/}"
if [[ -z "$UUID" || -z "$SUBVOL" ]]; then
    echo "could not read root UUID/subvol (source=$SRC)" >&2
    exit 1
fi

CMDLINE="root=UUID=${UUID} rw rootflags=subvol=${SUBVOL} quiet loglevel=6"
echo "kernel $KVER"
echo "cmdline $CMDLINE"

install -d -m 755 "$OUT"
INITRD="$OUT/initramfs-opdbus.img"
UKI="$OUT/opdbus.efi"
ESPIMG="$OUT/opdbus-efi-boot.img"

install -D -m 755 "$ROOT/initcpio/install/opnet" /etc/initcpio/install/opnet
install -D -m 755 "$ROOT/initcpio/hooks/opnet" /etc/initcpio/hooks/opnet
install -D -m 755 "$ROOT/initcpio/opnet" /etc/initcpio/opnet

mkinitcpio \
    -c "$ROOT/mkinitcpio.conf" \
    -k "$KVER" \
    -g "$INITRD"

ukify build \
    --linux="$VMLINUZ" \
    --initrd="$INITRD" \
    --cmdline="$CMDLINE" \
    --os-release=@/etc/os-release \
    --uname="$KVER" \
    --output="$UKI"

STUBDIR="$(mktemp -d)"
trap 'rm -rf "$STUBDIR"' EXIT
cat >"$STUBDIR/grub.cfg" <<EOF
set timeout=5
set default=0
# Standalone GRUB's prefix is the memdisk. Point root at the FAT volume
# that actually holds the kernel and initramfs.
search --file --set=root /vmlinuz-linux-xanmod-edge-linux-bin-x64v3
menuentry 'OP-DBUS btrfs root' {
    linux /vmlinuz-linux-xanmod-edge-linux-bin-x64v3 ${CMDLINE}
    initrd /initramfs-opdbus.img
}
menuentry 'OP-DBUS btrfs root, early DHCP' {
    linux /vmlinuz-linux-xanmod-edge-linux-bin-x64v3 ${CMDLINE} opnet
    initrd /initramfs-opdbus.img
}
EOF

grub-mkstandalone -O x86_64-efi -o "$STUBDIR/BOOTX64.EFI" \
    --modules="part_gpt part_msdos fat normal linux configfile search search_fs_file echo test all_video gzio boot" \
    "boot/grub/grub.cfg=$STUBDIR/grub.cfg"

rm -f "$ESPIMG"
truncate -s 384M "$ESPIMG"
mkfs.fat -F 32 -n OPDBUS "$ESPIMG" >/dev/null
MNT="$(mktemp -d)"
mount -o loop "$ESPIMG" "$MNT"
install -d "$MNT/EFI/BOOT"
cp "$STUBDIR/BOOTX64.EFI" "$MNT/EFI/BOOT/BOOTX64.EFI"
cp "$VMLINUZ" "$MNT/vmlinuz-linux-xanmod-edge-linux-bin-x64v3"
cp "$INITRD" "$MNT/initramfs-opdbus.img"
sync
umount "$MNT"
rmdir "$MNT"

install -d "$ESP/EFI/opdbus"
cp "$UKI" "$ESP/EFI/opdbus/opdbus.efi"

BEFORE="$(efibootmgr | sed -n 's/^BootOrder: //p' | tr -d ' ')"
# Drop a previous opdbus-uki entry so rebuilds do not stack duplicates.
while read -r hex; do
    [[ -n "$hex" ]] || continue
    efibootmgr -b "$hex" -B >/dev/null || true
done < <(efibootmgr | awk '/opdbus-uki/{print substr($1,5,4)}')
BEFORE="$(efibootmgr | sed -n 's/^BootOrder: //p' | tr -d ' ')"

efibootmgr -c -d "$DISK" -p "$PART" -L opdbus-uki -l '\EFI\opdbus\opdbus.efi' >/dev/null
NEW="$(efibootmgr | sed -n 's/^BootOrder: //p' | tr -d ' ' | cut -d, -f1)"
if [[ -n "$BEFORE" ]]; then
    efibootmgr -o "${BEFORE},${NEW}" >/dev/null
fi

echo
echo "initramfs  $INITRD"
echo "uki        $UKI"
echo "efi image  $ESPIMG"
echo "esp copy   $ESP/EFI/opdbus/opdbus.efi"
echo "boot order $(efibootmgr | sed -n 's/^BootOrder: //p')"
ukify inspect "$UKI" | head -20
