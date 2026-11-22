# systemd-run-compat: opt-in `systemd-run` for runit hosts

Huntress's updater applies updates with
`systemd-run --uid 0 --gid 0 --description ... --unit huntress-update-helper --collect --quiet /usr/share/huntress/huntress-updater update`.
Without systemd-run every update fails ("update failed ... systemd-run: executable file not found").

`systemd-run` here starts the command as a detached "transient unit": it gets its own session
(`setsid -f`), is reparented to init, and outlives the caller being stopped. It holds a per-unit
flock in /run/systemd-run-compat (a second start of the same `--unit` is refused, as in systemd)
and logs to /var/log/systemd-run-compat/<unit>.log. `--uid/--gid` switch via setpriv only when they
differ from the caller. `--scope/--wait/--pipe` run in the foreground. Unit properties are ignored.

Install: `install -D -m 0755 systemd-run /usr/local/libexec/systemd-run-compat/bin/systemd-run`

Opt-in only. Put `/usr/local/libexec/systemd-run-compat/bin` first in a runit service's
`env/PATH` (huntress-agent/updater/rio do, via `systemd1-compat/with-bus` -> `chpst -e env`).
Nothing else on the host sees a systemd-run.
