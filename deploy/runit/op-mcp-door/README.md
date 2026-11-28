# op-mcp-door runit service (template, not installed)

Spec: `.kiro/specs/mcp-door-adapter/`. This directory is a repository
template. Nothing here is installed, linked into a runsvdir, or started by the
commit that added it. Installing it is an owner decision.

- `run` — loads `/etc/op-dbus/env` with `chpst -e` and execs
  `op-mcp-door serve`. stdout is the audit log; operational events are JSON
  lines on stderr, folded into the same stream with `exec 2>&1`.
- `log/run` — `svlogd` into `/var/log/runit/op-mcp-door` as `_runit-log`,
  without `-tt`, so every line is one JSON object. Never remove the log service.
- `check` — TCP connect to the TLS listener.

Listener: TLS only, default `127.0.0.1:8091` (a port separate from the bridge
door). The certificate and key default to the door's
`THREETCHED_TLS_CERT_FILE` / `THREETCHED_TLS_KEY_FILE`
(`/var/lib/op-dbus/door-tls/door.{crt,key}`); the key must be mode 0600.
Reaching 8091 from outside through `op-tunnel-door` needs an
`op-tunnel-mux` change and is out of scope for this spec.

Configuration files (examples in `deploy/config/op-mcp-door/`):
`/etc/opdbus/mcp-door/{mounts,principals,oia1-keys}.json`. Ephemeral grants
and tickets live under `/dev/shm/opdbus/mcp-door/{grants,tickets}` (0700) and
are issued by a human with `op-mcp-door grant issue` / `ticket issue`.

Startup scans the whole mounted catalog (`metadata.json` per operation). On
the live `/run/mounts/3tchedFS` (~1,600 operations) that takes about 3
minutes, so `check` stays failing until the listener binds. Generate the
toolsets file once, as root, before the first start:

```sh
op-mcp-door toolsets generate --mount /run/mounts/3tchedFS \
  --mode '{"kind":"guest_kernel"}' --out /etc/opdbus/mcp-toolsets.json
```

`deploy/config/op-mcp-door/mcp-toolsets.json.example` is that command's
output against the live antix1 catalog on 2026-11-19 (1,620 tools, view
`c75d9fcf…`); regenerate after any reseal rather than copying it.

The toolsets file is an **interim** source. Tool grouping is being built
FUSE-side by Wonder (Muse) as a general mechanism; once it lands, the door
reads toolsets from it instead and this file and `toolsets generate` retire.
Do not extend the generator's grouping rules in the meantime, beyond the
owner's 2026-11-19 cognitive layout: memory (7) always loaded and never
grouped, ten hot tools (coding and knowledge mandatory), and every other
`cognitive` operation in one lazy `cognitive` set. The same layout is recorded
for the FUSE projection in `deploy/config/toolsets/` (see its README).

Coding (`code_search`, `code_context`, `code_index`) and knowledge
(`search_own_events`) stay listed but answer `pending_revectorization` until
their Qdrant collections exist and hold points. Check without changing
anything (read-only GETs, exit 3 while pending):

```sh
chpst -e /etc/op-dbus/env op-mcp-door vectors status
```

The door-side grant store, tickets, `authorize()` and audit records are
likewise interim, pending Wonder's shared FUSE contracts (identity binding,
capability checks, toolset projection, auditable receipts). The door keeps
TLS, auth mapping and the MCP protocol.

Grants and tickets (root, with the same `/etc/op-dbus/env` loaded so the
state directory matches):

```sh
# Standing-for-a-while grant: principal, tool (or toolset:<id>), TTL seconds, call count.
chpst -e /etc/op-dbus/env op-mcp-door grant issue --principal <uuid> \
  --tool cognitive.memory_store --ttl 3600 --max-calls 20
chpst -e /etc/op-dbus/env op-mcp-door grant list
chpst -e /etc/op-dbus/env op-mcp-door grant revoke --id g-...
# Single-use elevation ticket for shell/exec/admin/secret tools, bound to exact args (TTL <= 300 s).
chpst -e /etc/op-dbus/env op-mcp-door ticket issue --principal <uuid> \
  --tool <plugin>.<method> --args-json '{"...":"..."}' --ttl 120
```

The caller passes the printed ticket id as the `elevation_ticket` argument of
`call_tool`, or as `_meta["opdbus/elevation_ticket"]` on a direct `tools/call`.

Cursor (`~/.cursor/mcp.json`), both endpoints:

```json
{"mcpServers": {
  "door-full":   {"url": "https://<door>:8091/mcp/full",
                  "headers": {"x-oracle-identity-assertion-bin": "<base64 OIA1>"}},
  "door-loader": {"url": "https://<door>:8091/mcp/loader",
                  "headers": {"x-oracle-identity-assertion-bin": "<base64 OIA1>"}}}}
```

The door certificate is self-signed; point `NODE_EXTRA_CA_CERTS` at
`door.crt` on the client. OIA1 assertions live at most 15 minutes (see the
spec's open questions).
