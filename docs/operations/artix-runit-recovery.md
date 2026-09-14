# Artix runit service recovery

This host boots **runit** as PID 1 and is controlled with `sv`. Runit has **no
compiled service database**, which removes the whole class of "the bootdb is
corrupt" failures that s6-rc had. There is nothing to recompile and nothing to
re-init: definitions on disk *are* the configuration.

If you arrived here looking for `s6-rc-compile`, `s6-rc-init`, or bundle
recovery: none of it applies. s6 is not installed on this host.

## Layout

| Path | Role |
|---|---|
| `/etc/runit/sv/<service>/run` | The service definition |
| `/etc/runit/sv/<service>/down` | Marker: do not auto-start when supervised |
| `/etc/runit/runsvdir/default/<service>` | Symlink enabling the service at boot |
| `/etc/runit/runsvdir/current` | Symlink to the active runlevel (`-> default`) |
| `/run/runit/service/<service>` | The tree `runsvdir -P` supervises |
| `/run/runit/service/<service>/supervise/` | Live state written by `runsv` |

`sv` needs root to read `supervise/ok`; an unprivileged `sv status` fails with
"access denied".

## Diagnosis order

```sh
ps -p 1 -o comm=                 # expect: runit
pgrep -a runsvdir                # expect: runsvdir -P /run/runit/service
sudo sv status <service>
ls -l /etc/runit/runsvdir/current   # which runlevel is active
```

`sv status` output reads `run: <svc>: (pid N) Ss` when healthy, `down: <svc>: Ns`
when stopped, and `warning: <svc>: unable to open supervise/ok` when `runsv` is
not supervising it at all.

## Common failures

### A service is enabled but never starts

`runsvdir` scans its directory on its own, so a missing service usually means the
symlink or the `run` script is wrong.

```sh
ls -l /etc/runit/runsvdir/default/<service>   # symlink present and not dangling?
test -x /etc/runit/sv/<service>/run           # run script executable?
sudo sh -n /etc/runit/sv/<service>/run        # syntax valid?
```

A `run` script that is not executable, or whose interpreter line is wrong, makes
`runsv` fail silently in a loop.

### A service restarts in a loop

Almost always because the process **daemonises**. Runit supervises the process it
starts; if that process forks and exits, `runsv` concludes it died and starts it
again. Fix the `run` script to keep the binary in the foreground (`-N`, `-f`,
`--foreground`, or `--nodaemon`, depending on the program) and ensure it ends
with `exec`.

### A stuck `supervise/` directory

If a service will not respond to `sv` after an unclean shutdown, the supervise
state can be stale. Stop supervision, clear the runtime view, and let `runsvdir`
re-create it:

```sh
sudo sv down <service>
sudo rm -f /run/runit/service/<service>          # runtime symlink only
# runsvdir re-creates it from /etc/runit/runsvdir/default within ~5s
sudo sv status <service>
```

Never delete `/etc/runit/sv/<service>` to fix a runtime problem — that is the
definition, not the state.

### Recovering into single-user

`/etc/runit/runsvdir/single` holds the minimal runlevel. To boot into it, append
`single` to the kernel command line, or switch at runtime:

```sh
sudo runsvchdir single      # switch runlevel
sudo runsvchdir default     # switch back
```

`runsvchdir` is the sanctioned way to change runlevels; it repoints
`/etc/runit/runsvdir/current`.

## Shipping new binaries

Deployment is **btrfs send/receive** of subvolume snapshots — see
`deploy/btrfs-layout.sh` for the base/modules/snapshots/staging layout. A release
is a snapshot sent to the target, not a file copy onto a live host.

`deploy/runit/build-golden.sh` only populates and validates the golden
subvolume, including the committed persisted plugin catalog. Use
`OP_PERSISTED_CATALOG_SOURCE` when preparing from an isolated catalog. The
builder validates that input and never reseals it, installs into the running
host, or restarts services. After
reviewing its manifest, an operator creates a read-only snapshot and transfers
that snapshot with `btrfs send/receive`. Receiving, activating, and restarting
the target are separate recovery or release steps.

## Third-party installers that expect systemd

A `systemctl` shim is installed at `/usr/local/bin/systemctl`. It maps systemd
verbs onto `sv`, and converts a `.service` unit into `/etc/runit/sv/<name>/run`
using `/usr/local/sbin/systemd-unit-to-runit`. A pacman hook converts units that
packages drop, without enabling them — enabling stays an operator decision.

To convert a unit by hand and inspect the result first:

```sh
systemd-unit-to-runit /usr/lib/systemd/system/<name>.service --dry-run
sudo systemd-unit-to-runit /usr/lib/systemd/system/<name>.service --enable
```

Conversions report what runit cannot express: `Type=forking` (will restart-loop
until you add the no-fork flag), `Type=notify` (no sd_notify socket),
`Type=oneshot` (use `sv once`), and `Restart=no` (runit always restarts, so a
`down` file is written).
