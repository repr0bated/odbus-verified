# incus-ct-<name>: Incus identity containers under runit

runit starts and stops each identity container (Jeremy, 3tchedChatbot); Incus
`boot.autostart` stays false. One service per container, with its own log.

| File | Installed as |
|---|---|
| `ct-run` | `/usr/local/libexec/op-dbus/incus-ct-run` (0755) |
| per container | `/etc/sv/incus-ct-<name>/{run,log/run}`, linked from `/etc/service/` |

`run` is `exec /usr/local/libexec/op-dbus/incus-ct-run <name>`. Logs: `/var/log/op-dbus/incus-ct-<name>/current`
(svlogd -tt as `svlog`).

The service waits for `incus` and `cozo-ublk-500g` (the pool disk), recreates
the container's `/run/opdbus` bind sources, starts the container, then waits on
the Incus lifecycle feed. A stop under runit exits the service so runit starts
the container again; `sv down incus-ct-<name>` stops it.

`incus-ct-<name>` has no `down` file, so containers start at boot.
