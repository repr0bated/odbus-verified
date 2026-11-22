# systemd1-compat: opt-in, private org.freedesktop.systemd1 for runit hosts

Some vendor binaries (Huntress, anything built on coreos/go-systemd or sd-bus
unit queries) ask systemd over D-Bus whether their services are running. A
runit host has no systemd. Claiming `org.freedesktop.systemd1` on the real
system bus would make *every* program think systemd is present. This instead
runs a **separate, private dbus-daemon** that carries only a read-only
responder. Only programs that are explicitly opted in can see it.

- `systemd1-compat-bus` (runit): `dbus-daemon --config-file=/etc/systemd1-compat/bus.conf`,
  listening on `unix:path=/run/systemd1-compat/system_bus_socket`.
  `/run/systemd1-compat` is root-only (0700) by default.
- `systemd1-compat-shim` (runit): owns `org.freedesktop.systemd1` on that bus only.
  It answers `Properties.Get/GetAll` on `/org/freedesktop/systemd1/unit/<escaped>`
  (Unit: Id, LoadState, ActiveState, SubState, UnitFileState; Service: MainPID) and
  `Manager.GetUnit/LoadUnit`, **only for units listed in `/etc/systemd1-compat/units`**.
  State comes from `sv status /etc/service/<svc>`, with `/var/run/<svc>.pid` as the fallback.
  There are no write methods (no Start/Stop/Restart).
- The real system bus is untouched: `busctl --system list | grep systemd1` is empty.

## Install
```
install -d -m 0755 /etc/systemd1-compat/bus.d /usr/local/libexec/systemd1-compat
install -m 0644 bus.conf units /etc/systemd1-compat/
install -m 0755 systemd1-compat-shim with-bus /usr/local/libexec/systemd1-compat/
install -m 0755 systemd1-compat-optin /usr/local/sbin/
cp -a sv/systemd1-compat-bus sv/systemd1-compat-shim /etc/sv/
ln -s /etc/sv/systemd1-compat-bus /etc/sv/systemd1-compat-shim /etc/runit/runsvdir/default/
```

## How to add a program
1. `systemd1-compat-optin <runit-svc> <unit>.service [...]`
   This writes `/etc/sv/<runit-svc>/env/DBUS_SYSTEM_BUS_ADDRESS` and appends the units
   to `/etc/systemd1-compat/units`. The format is `<unit> [runit-svc]`; the runit service
   defaults to the unit name minus `.service`. The shim rereads the file on change, so no restart is needed.
2. Make the run script end with
   `exec /usr/local/libexec/systemd1-compat/with-bus /etc/sv/<runit-svc>/env <program> [args]`
   `with-bus` waits up to 15s for the socket, then runs `chpst -e <envdir>`, so the program
   sees `DBUS_SYSTEM_BUS_ADDRESS` set to the private bus.
   Only opt in programs that use the system bus for nothing but systemd1:
   names like login1 do not exist on this bus.
   For an env-dir-only alternative: `exec chpst -e /etc/sv/<svc>/env <program>`.
3. `sv restart <runit-svc>`

Current clients: huntress-agent, huntress-updater, huntress-rio (see ../huntress).

## Granting non-root access
The socket's directory controls who can connect. To let a group use it:
1. `echo <group> > /etc/systemd1-compat/socket-group` (the directory becomes 0750 root:<group>)
2. Add `/etc/systemd1-compat/bus.d/<group>.conf`:
   `<busconfig><policy group="<group>"><allow send_destination="org.freedesktop.systemd1"/></policy></busconfig>`
3. `sv restart systemd1-compat-bus systemd1-compat-shim`

## Check
```
A=unix:path=/run/systemd1-compat/system_bus_socket
dbus-send --bus=$A --print-reply --dest=org.freedesktop.systemd1 \
  /org/freedesktop/systemd1/unit/huntress_2dagent_2eservice \
  org.freedesktop.DBus.Properties.Get string:org.freedesktop.systemd1.Unit string:ActiveState
```
