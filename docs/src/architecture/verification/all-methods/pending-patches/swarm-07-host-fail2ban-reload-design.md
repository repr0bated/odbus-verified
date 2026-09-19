# Fail2ban reload design (not implemented)

The installed native client (`/usr/lib/python3.14/site-packages/fail2ban/client/fail2banclient.py:308-334`) does not send a bare reload. For global reload it calls `readConfig()` and sends one pickle command:

```
["reload", "--all", opts, stream]
```

For a named jail it sends `["reload", jail, opts, stream]`. `opts` is a list containing `--restart`, `--unban`, and/or `--if-exists`; `stream` is the ordered command stream produced by `Configurator.convertToProtocol()`: general `fail2ban.conf` commands first, then each enabled jail's filter/backend/logpath/action commands. `CSocket.send()` pickles this heterogeneous nested list with `HIGHEST_PROTOCOL`, appends `CSPROTO.END`, and expects the normal two-element ack.

A safe Rust implementation needs a trusted configuration owner. The current adapter has no config owner, no parser, and only `Vec<&str>` requests. Do not accept an arbitrary caller supplied stream, invoke Python/fail2ban-client as a subprocess, or send `["reload"]`. The intended follow-up is:

1. Introduce a private owner interface that supplies a validated, generation-bound `ReloadPlan { jail: --all|validated jail, options: bounded enum set, stream: bounded ordered command tree }` from the configured Fail2ban owner.
2. Extend the native adapter's pickle encoder to encode only the finite allowed scalar/list/map shapes needed by Fail2ban protocol, with limits on commands, nesting, strings, total bytes, and option set.
3. Encode `["reload", jail, opts, stream]` and require an acknowledged response; publish a typed receipt containing the owner generation and selected jail/options. Re-read jail status after reload before reporting success.
4. Add a native Unix fixture that verifies the exact four-element command and nested stream, returns an ack, then exposes changed status for readback. Until that owner and fixture exist, `reload` remains an explicit unsupported/missing method.
