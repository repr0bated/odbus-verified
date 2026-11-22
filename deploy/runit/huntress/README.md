# Huntress Linux EDR on runit hosts (antix1)

Huntress supports systemd only. Its installer dies when PID 1 is not systemd,
and its binaries use kardianos/service plus coreos/go-systemd D-Bus. This
directory runs it under runit. No keys or agent config live here.

## Install (as root)
1. Download `https://huntresscdn.com/huntress-installers/linux/huntress-linux-install.sh`.
   Make a local copy with only two edits (don't redistribute Huntress's script):
   - delete the `ps -p 1 ... != systemd` die in `validate_requirements`
   - in `install_pkg`, stop after `register`: drop the `install`, `start` and `update` calls
   Run it: `--batch_only --account_key ... --organization_key ...`. It downloads,
   unpacks to /usr/share/huntress, writes /etc/huntress/agent_config.yaml and registers.
2. `install -m 0644 ../boot-run/S98-huntress-kfs.stage1.sh /etc/runit/boot-run/`, then
   source it once. It mounts bpffs and tracefs; it's sourced by /etc/runit/1, so it never exits.
3. Install ../systemd1-compat (private systemd1 bus) first, then
   `cp -a sv/huntress-agent sv/huntress-updater sv/huntress-rio /etc/sv/`
   (each comes with `env/DBUS_SYSTEM_BUS_ADDRESS`, which opts it in to the private bus).
4. `ln -s /etc/sv/huntress-{agent,updater,rio} /etc/runit/runsvdir/default/`
   huntress-rio idles until the updater has fetched /usr/share/huntress/rio/rio.
5. Optionally `huntress-agent update` triggers the first update, which delivers Rio.

## How it fits together
- Each `sv/<svc>/run` translates the unit that `<svc> install` writes under
  systemd (Type=simple, root, cwd /run/huntress, Restart=always/RestartSec=5 via `finish`).
- kardianos/service picks SysV here. The updater controls services through
  `service <name> start|stop|restart|status` and installs new ones (Rio) as
  /etc/init.d scripts with an unsupervised start. Each run script therefore keeps
  `/etc/init.d/<svc>` replaced by `sv/<svc>/initd`, which maps those verbs to
  `sv` and prints Running/Stopped for kardianos. It also kills any unsupervised
  copy found via /var/run/<svc>.pid. runit's SysV emulation skips init.d scripts
  that have an /etc/sv/<name>, so nothing starts twice at boot.
- The agent, updater and Rio check sibling service health only through
  go-systemd -> godbus `SystemBusPrivate`, which honours `DBUS_SYSTEM_BUS_ADDRESS`.
  They use no other bus names, no /run/systemd/private and no persistent real-bus connection.
  Their run scripts exec through `systemd1-compat/with-bus`, so they talk to the
  private systemd1-compat bus. Nothing else on the host sees a systemd1.
- No systemctl CLI shim: neither binary execs systemctl on a non-systemd host.
  `uninstall.sh` ends with `systemctl daemon-reload`, which fails after
  everything has been removed. Then remove /etc/sv/huntress-* and the runsvdir links.
- Logs: Huntress writes /var/log/Huntress*.log and /usr/share/huntress/rio/rio.log.
  runit stdout goes to /var/log/huntress/<svc>/ (svlogd).
- /etc/machine-id -> /var/lib/dbus/machine-id (symlink). The agent reads it for host_unique_id.
  It was absent at registration; linking it later did not re-register (same agent id).
- All three are opted in (`sv/<svc>/env/PATH`) to ../systemd-run-compat (the updater applies updates via systemd-run) and journal-compat; Rio uses journal-compat so it can read
  sshd/polkitd/cron events without journald. See ../logging/README.md.
