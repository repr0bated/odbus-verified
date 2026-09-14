#!/bin/sh
# Apply the release-owned mail overlay to an offline Alpine rootfs. This script
# must run while preparing the btrfs/Incus image, never against a running
# container.
set -eu

die() {
    printf 'mail-3tched rootfs: %s\n' "$*" >&2
    exit 1
}

[ "$#" -eq 2 ] || die "usage: $0 ROOTFS ARTIFACT_ROOTFS"
[ "${OP_MAIL_OFFLINE_ROOTFS:-}" = 1 ] ||
    die "set OP_MAIL_OFFLINE_ROOTFS=1 for prepared-image application"
ROOTFS=$(readlink -f "$1")
ARTIFACT_ROOTFS=$(readlink -f "$2")
[ "$ROOTFS" != / ] || die "refusing the host rootfs"
[ -f "$ROOTFS/etc/alpine-release" ] || die "target is not an Alpine rootfs"
[ -f "$ROOTFS/etc/postfix/main.cf" ] || die "Postfix main.cf is absent"
[ -f "$ROOTFS/etc/dovecot/dovecot.conf" ] || die "Dovecot config is absent"
[ -f "$ROOTFS/etc/passwd" ] || die "target passwd database is absent"
[ -f "$ROOTFS/etc/group" ] || die "target group database is absent"
[ -x "$ARTIFACT_ROOTFS/usr/local/bin/op-mail-admin" ] ||
    die "artifact is missing op-mail-admin"

# Resolve IDs from the target image, not the preparation host's NSS database.
vmail_record=$(awk -F: '
    $1 == "vmail" { count++; uid = $3; gid = $4 }
    END { if (count != 1) exit 1; print uid ":" gid }
' "$ROOTFS/etc/passwd") || die "target must contain exactly one vmail account"
vmail_uid=${vmail_record%%:*}
vmail_gid=${vmail_record#*:}
target_vmail_gid=$(awk -F: '
    $1 == "vmail" { count++; gid = $3 }
    END { if (count != 1) exit 1; print gid }
' "$ROOTFS/etc/group") || die "target must contain exactly one vmail group"
dovecot_gid=$(awk -F: '
    $1 == "dovecot" { count++; gid = $3 }
    END { if (count != 1) exit 1; print gid }
' "$ROOTFS/etc/group") || die "target must contain exactly one dovecot group"
dovecot_record=$(awk -F: '
    $1 == "dovecot" { count++; uid = $3; gid = $4 }
    END { if (count != 1) exit 1; print uid ":" gid }
' "$ROOTFS/etc/passwd") || die "target must contain exactly one dovecot account"
dovecot_uid=${dovecot_record%%:*}
dovecot_primary_gid=${dovecot_record#*:}
for id in "$vmail_uid" "$vmail_gid" "$target_vmail_gid" "$dovecot_uid" "$dovecot_primary_gid" "$dovecot_gid"; do
    case "$id" in
        ''|*[!0-9]*|0) die "mail account IDs must be nonzero decimal numbers" ;;
    esac
done
[ "$vmail_gid" = "$target_vmail_gid" ] ||
    die "vmail passwd and group primary IDs differ"
[ "$dovecot_primary_gid" = "$dovecot_gid" ] ||
    die "dovecot passwd and group primary IDs differ"

# Existing mailbox contents remain in place. The owner changes only the root
# directory here; op-mail-admin validates it before effects and owns current
# mailbox directory materialization thereafter.
install -d -m 0700 "$ROOTFS/var/mail/vhosts"
chown "$vmail_uid:$vmail_gid" "$ROOTFS/var/mail/vhosts"
install -d -m 0755 "$ROOTFS/etc/conf.d"
cat > "$ROOTFS/etc/conf.d/op-mail-admin" <<EOF
OP_MAIL_VMAIL_UID=$vmail_uid
OP_MAIL_VMAIL_GID=$vmail_gid
OP_MAIL_DOVECOT_GID=$dovecot_gid
EOF
chmod 0644 "$ROOTFS/etc/conf.d/op-mail-admin"
chown --reference="$ROOTFS/etc/passwd" "$ROOTFS/etc/conf.d/op-mail-admin"

install -Dm755 "$ARTIFACT_ROOTFS/usr/local/bin/op-mail-admin" \
    "$ROOTFS/usr/local/bin/op-mail-admin"
runtime_destination="$ROOTFS/usr/local/lib/op-mail-admin"
case "$runtime_destination" in
    "$ROOTFS"/usr/local/lib/op-mail-admin) ;;
    *) die "unsafe runtime destination" ;;
esac
if [ -e "$runtime_destination" ]; then
    find "$runtime_destination" -depth -delete
fi
install -d -m 0755 "$runtime_destination/lib"
find "$ARTIFACT_ROOTFS/usr/local/lib/op-mail-admin" -type f -print |
while IFS= read -r source; do
    relative=${source#"$ARTIFACT_ROOTFS/usr/local/lib/op-mail-admin/"}
    case "$relative" in
        lib/*)
            install -Dm755 "$source" \
                "$ROOTFS/usr/local/lib/op-mail-admin/$relative"
            ;;
        ld-linux-x86-64.so.2|op-mail-admin.bin)
            install -Dm755 "$source" \
                "$ROOTFS/usr/local/lib/op-mail-admin/$relative"
            ;;
        *) die "unexpected runtime bundle member: $relative" ;;
    esac
done
install -Dm755 "$ARTIFACT_ROOTFS/etc/init.d/op-mail-admin" \
    "$ROOTFS/etc/init.d/op-mail-admin"
install -Dm644 "$ARTIFACT_ROOTFS/etc/postfix/opdbus-main.cf" \
    "$ROOTFS/etc/postfix/opdbus-main.cf"
grep -q '@OP_MAIL_VMAIL_UID@' "$ARTIFACT_ROOTFS/etc/dovecot/opdbus.conf" ||
    die "Dovecot overlay has no target UID placeholder"
grep -q '@OP_MAIL_VMAIL_GID@' "$ARTIFACT_ROOTFS/etc/dovecot/opdbus.conf" ||
    die "Dovecot overlay has no target GID placeholder"
sed \
    -e "s/@OP_MAIL_VMAIL_UID@/$vmail_uid/g" \
    -e "s/@OP_MAIL_VMAIL_GID@/$vmail_gid/g" \
    "$ARTIFACT_ROOTFS/etc/dovecot/opdbus.conf" > "$ROOTFS/etc/dovecot/opdbus.conf"
if grep -q '@OP_MAIL_' "$ROOTFS/etc/dovecot/opdbus.conf"; then
    die "Dovecot overlay contains an unresolved target account placeholder"
fi
chmod 0644 "$ROOTFS/etc/dovecot/opdbus.conf"
chown --reference="$ROOTFS/etc/dovecot/dovecot.conf" \
    "$ROOTFS/etc/dovecot/opdbus.conf"
if [ ! -e "$ROOTFS/etc/postfix/opdbus-domains" ]; then
    install -Dm644 "$ARTIFACT_ROOTFS/etc/postfix/opdbus-domains" \
        "$ROOTFS/etc/postfix/opdbus-domains"
fi

# Postfix has no portable include directive for main.cf. Replace these three
# release-owned keys and append the tracked block atomically. The source text
# tables remain intact for first-start adoption; stale LMDB products are no
# longer referenced.
postfix_stage=$(mktemp "$ROOTFS/etc/postfix/.main.cf.opdbus.XXXXXX")
trap 'rm -f -- "$postfix_stage" "${dovecot_stage:-}"' EXIT HUP INT TERM
awk '
    $0 == "# BEGIN OP-DBUS MAIL ADMIN" { in_block = 1; next }
    $0 == "# END OP-DBUS MAIL ADMIN" { in_block = 0; next }
    in_block { next }
    /^[[:space:]]*virtual_mailbox_domains[[:space:]]*=/ { next }
    /^[[:space:]]*virtual_mailbox_maps[[:space:]]*=/ { next }
    /^[[:space:]]*virtual_alias_maps[[:space:]]*=/ { next }
    { print }
' "$ROOTFS/etc/postfix/main.cf" > "$postfix_stage"
printf '\n# BEGIN OP-DBUS MAIL ADMIN\n' >> "$postfix_stage"
cat "$ARTIFACT_ROOTFS/etc/postfix/opdbus-main.cf" >> "$postfix_stage"
printf '# END OP-DBUS MAIL ADMIN\n' >> "$postfix_stage"
chmod --reference="$ROOTFS/etc/postfix/main.cf" "$postfix_stage"
chown --reference="$ROOTFS/etc/postfix/main.cf" "$postfix_stage"
mv -f -- "$postfix_stage" "$ROOTFS/etc/postfix/main.cf"

# The deployed mail image carries a compact, self-contained dovecot.conf. Its
# generated passwd file must own both authentication and user lookup: a static
# userdb cannot return the per-user quota field. Reject indirect configuration
# here because an included auth database could precede this release-owned one.
if awk '
    /^[[:space:]]*!include(_try)?[[:space:]]+/ &&
        $0 != "!include_try /etc/dovecot/opdbus.conf" { found = 1 }
    END { exit found ? 0 : 1 }
' "$ROOTFS/etc/dovecot/dovecot.conf"; then
    die "Dovecot base config has indirect includes; flatten it before applying the owned passwd-file auth path"
fi
dovecot_stage=$(mktemp "$ROOTFS/etc/dovecot/.dovecot.conf.opdbus.XXXXXX")
awk '
    function brace_delta(text, copy) {
        copy = text
        return gsub(/\{/, "", copy) - gsub(/\}/, "", copy)
    }
    skipping {
        depth += brace_delta($0)
        if (depth <= 0) {
            skipping = 0
        }
        next
    }
    /^[[:space:]]*(passdb|userdb)[[:space:]]*\{/ {
        skipping = 1
        depth = brace_delta($0)
        if (depth <= 0) {
            skipping = 0
        }
        next
    }
    $0 != "!include_try /etc/dovecot/opdbus.conf" { print }
' "$ROOTFS/etc/dovecot/dovecot.conf" > "$dovecot_stage"
printf '\n!include_try /etc/dovecot/opdbus.conf\n' >> "$dovecot_stage"
chmod --reference="$ROOTFS/etc/dovecot/dovecot.conf" "$dovecot_stage"
chown --reference="$ROOTFS/etc/dovecot/dovecot.conf" "$dovecot_stage"
mv -f -- "$dovecot_stage" "$ROOTFS/etc/dovecot/dovecot.conf"

install -d -m 0755 "$ROOTFS/etc/runlevels/default"
ln -sfn /etc/init.d/op-mail-admin \
    "$ROOTFS/etc/runlevels/default/op-mail-admin"
trap - EXIT HUP INT TERM
