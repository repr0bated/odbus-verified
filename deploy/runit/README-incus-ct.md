# Incus container runit services

Generator: `3tched-incus-svcgen` (installed at `/usr/local/bin/3tched-incus-svcgen`).

```sh
# one container
sudo 3tched-incus-svcgen mail-3tched

# all named workload CTs (skips UUID identity containers)
sudo 3tched-incus-svcgen --all-named

# define only, do not enable/start
sudo 3tched-incus-svcgen assistant --no-enable
```

| Path | Purpose |
|------|---------|
| `/etc/runit/sv/incus-ct-<name>/` | service def (`run`, `check`, `finish`, `log/run`) |
| `/etc/runit/runsvdir/default/incus-ct-<name>` | enabled symlink |
| `/var/log/op-dbus/incus-ct-<name>/current` | svlogd logs (`s6log` user) |

Manage with standard runit:

```sh
sv status incus-ct-mail-3tched
sv restart incus-ct-netmaker
tail -F /var/log/op-dbus/incus-ct-mail-3tched/current
```

`boot.autostart` is forced off so **runit** owns lifecycle (not Incus).
