#!/bin/sh
# Keep agent sessions on the sanctioned `sv` management surface.
#
# Boot and console recovery run these programs as root and are unaffected; this
# only shadows them for interactive agent sessions.
#
# Why: runit's supervisor entry points (`runsv`, `runsvdir`, `runit-init`) are
# not service-management commands. Invoking them by hand starts a second
# supervisor or reparents services, which desynchronises /run/runit/service from
# /etc/runit/runsvdir/default and is hard to unwind. Service management goes
# through `sudo sv <command> <service>` per AGENTS.md.
#
# The s6 entry points are still listed even though the packages are not
# installed, so that reinstalling them cannot silently reopen the old path.
set -eu

GUARD_DIR=${GUARD_DIR:-/usr/local/lib/agent-runit-guard}
BIN_DIR=${BIN_DIR:-/usr/local/bin}

mkdir -p "$GUARD_DIR"

# Runit supervisor internals — not for hand invocation.
RUNIT_INTERNALS="runsv runsvdir runit runit-init utmpset"

# s6 is legacy on this host and must not come back through the side door.
S6_LEGACY="s6 s6-rc s6-rc-bundle s6-rc-compile s6-rc-db s6-rc-init \
s6-rc-update s6-svc s6-svok s6-svscan s6-svscanctl s6-svstat s6-svwait \
s6-supervise service6 s6d"

# foreign service managers
FOREIGN="systemctl dinitctl"

write_shim() {
    command_name=$1
    guidance=$2
    cat > "$GUARD_DIR/$command_name" <<EOF
#!/bin/sh
echo "agent-runit-guard: '$command_name' is not the sanctioned interface." >&2
echo "$guidance" >&2
exit 126
EOF
    chmod 755 "$GUARD_DIR/$command_name"
}

for command_name in $RUNIT_INTERNALS; do
    write_shim "$command_name" \
        "  Use: sudo sv <up|down|restart|status> <service>
  Runit internals belong to boot and to human console recovery."
done

for command_name in $S6_LEGACY; do
    write_shim "$command_name" \
        "  s6 is legacy on this host — it boots runit.
  Use: sudo sv <up|down|restart|status> <service>"
done

for command_name in $FOREIGN; do
    # `systemctl` is deliberately NOT shimmed to a hard failure: the
    # compatibility wrapper installed at $BIN_DIR/systemctl translates it to
    # `sv` so third-party installers keep working. Only shim it when that
    # wrapper is absent.
    if [ "$command_name" = "systemctl" ] && [ -x "$BIN_DIR/systemctl" ]; then
        continue
    fi
    write_shim "$command_name" \
        "  This host runs runit, not systemd/dinit.
  Use: sudo sv <up|down|restart|status> <service>"
done

echo "agent-runit-guard: shims installed in $GUARD_DIR"
echo "Prepend it to an agent session PATH to activate:"
echo "  PATH=$GUARD_DIR:\$PATH"
