# Runtime

Where things run, how to call the platform, and what the product may open.
State as of 2026-10-28; **proven** = demonstrated, **planned** = to build.

## Two roles, two users

| Role | Linux user | Where | Has |
|---|---|---|---|
| **Develop** (omp, builds, OSCAL authoring) | `jeremy` | `/srv/git/etched-in-schema` | cargo, trestle, node/npm, python3 |
| **Run** (the governed service: router, policy, enforcement, chat adapter) | `3tched` (uid 1001) — the Hackathon subsidiary | `/home/3tched/Hackathon` | node/npm, python3; the identity mount |

Only `3tched` can act through the subsidiary's mount; `jeremy` gets
`Permission denied` (**proven**). Build as `jeremy`, run as `3tched`.

## Calling the platform (proven)

The subsidiary's operations are files under
`/home/3tched/Hackathon/3tchedfs/operations/<plugin>/<method>/call`.
Open read-write, write the JSON arguments, fsync, read the result from the same
handle. Helper:

```sh
sudo -u 3tched 3tched-callfile \
  /home/3tched/Hackathon/3tchedfs/operations/identity_sled/get_own_identity/call '{}'
```

Each result includes `required_capability`, `operation_id`, chain
`event_id`/`event_hash` and `outcome`; a refusal names the missing capability.
Schemas: `operations/<plugin>/<method>/{input,output}.schema.json`.
Live state files (`plugins/*/value.json`) currently return EIO — use operations.

The mount is supervised by runit `subsidiary-hackathon-3tchedfs` and survives
reboots (**proven**). It acts as identity `<subsidiary-session>`, which today also holds
**root** grants — keep demo actions to pre-tested operations until a dedicated
low-privilege identity exists (**planned**; blocked on storage repair).

## OpenFlow development (2026-10-28)

- Bridge `etched0`: isolated OVS bridge for the subsidiary (no ports, no
  address, no uplink, fail-mode `secure`). Never touch `ovsbr0` — it carries
  3tched's own networking.
- The subsidiary holds `openflow.read` (**proven**: `get_datapath_health
  {bridge: etched0}` completed). `openflow.write` is withheld until
  `enforcement/` refuses any flow operation whose `bridge` is not `etched0`;
  platform capabilities are not resource-scoped, so this check is ours to
  build (and a good demo of a refusal).

## Process supervision

The host runs **runit** (not systemd). A long-running service is a runit
service directory (`run` script, `log/run` with svlogd), installed under
`/etc/sv/<name>`, enabled by a symlink in `/etc/runit/runsvdir/default`,
controlled with `sudo sv start|stop|status <name>`. Model new units on
`/etc/sv/subsidiary-hackathon-3tchedfs`. Run product services as `3tched`
(`chpst -u 3tched`).

## Network rules

- The **only** service ingress on this host is `<door-ip>:8090`. The product
  must not open new listening ports to the outside.
- The team-chat adapter connects **outbound** to its homeserver (no inbound
  webhook). A local homeserver for the demo binds to loopback only.
- The identity container (`3tchedCtlChatbot`) has internet-only egress
  (bridge `hackbr0`, ACL `internet-only`) and no network path to 3tched; it
  reaches 3tched only through its mount (**proven**).
- Any network change must be recorded in
  `/srv/git/odbus-verified/docs/src/operations/network.md`.

## Models

- **Meta (required for the product's AI steps)**: Meta's Model API,
  `https://api.meta.ai/v1` (OpenAI-style `/chat/completions`), model
  `muse-spark-1.3` (**proven**, 2026-10-28). Credentials: `META_API_KEY`,
  `META_MODEL`, `META_MODEL_BASE_URL` — in `~/.bash_secrets` for `jeremy`
  (development) and in `/home/3tched/.config/etched/env` (owner `3tched`,
  mode 0600) for the running service. The model reasons before answering:
  give it a generous `max_tokens` (800+).
- **TypeSafe Jev** (judgments): `TYPESAFE_API_KEY` in `~/.bash_secrets` (user
  `jeremy`); see `review/jev_strategy.py` for the request format.
- 3tched's chat router (`127.0.0.1:8084`) currently uses a Qwen model; do not
  depend on it for the judged product.

## Team chat (planned)

- Matrix first; demo client Element. A homeserver is needed: a lightweight
  one (e.g. a Rust homeserver such as Conduit/conduwuit) on loopback, or an
  existing account on a public homeserver.
- Adapter: a Matrix bot (e.g. `matrix-rust-sdk`), outbound only, run as
  `3tched` under runit.

## Language choices

- Router, policy, enforcement, chat adapter: **Rust** preferred (matches
  3tched; `cargo` as `jeremy`, run the binary as `3tched`). TypeScript is
  acceptable where the UI generator's renderer code is reused.
- UI renderer for the web: Svelte + json-render (existing).
- OSCAL: `trestle` (as `jeremy`).

## Secrets

Never commit keys. Put runtime secrets in a root- or `3tched`-owned file with
mode `0600` (e.g. `/home/3tched/.config/etched/env`) and load them in the
runit `run` script.

## Logs and receipts

- Service logs: `/var/log/<service>/current` (svlogd).
- The subsidiary mount's log: `/var/log/op-dbus/subsidiary-hackathon-3tchedfs/`.
- Receipts are the evidence: keep `operation_id` and `event_hash` for every
  action the product takes, and show them to the user.
