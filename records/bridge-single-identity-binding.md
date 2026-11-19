# Bridge: accept the existing host binding, no second container binding

To: Muse / Wonder. From: jeremy (implemented by Grok because Muse is rate-limited; t588u, t590u).
Status: implemented on `dev` in `crates/op-grpc-bridge/src/runtime_lungs_identity.rs`. Not deployed.
The running bridge (`b58d7e11`, pid 2131, from `bridge-direct.sh`) has not been touched or restarted.

## Problem

Host 3tchedFS views (`/run/mounts/3tchedFS`, `/run/mounts/threetched`) are mounted with
`--lungs-socket /run/opdbus/lungs/lungs.sock` and no `--lungs-session`. That makes them
`MountIdentity::GuestKernel` (`crates/_3tched-fs/src/fuse_fs.rs:208-217`), so every call
sends `BindCurrentContainer()` (`crates/_3tched-fs/src/dispatch.rs:314-319`).

On the bridge, the connection's identity is already bound when it is accepted:
`LungsIdentity::capture` (`runtime_lungs_identity.rs:169-187`, called from `runtime_lungs.rs:317`)
pins the kernel peer via `SO_PEERPIDFD`, `SO_PEERCRED` uid and the user/pid namespaces.
For a host root peer that already proves `host_root()` (`runtime_lungs_identity.rs:189-191`).
`bind_current_container` then tried a second, container binding and refused any peer in the
host pid namespace outright (old lines 266-267):

    ensure!(self.peer.pid_namespace != self.host.pid_namespace,
        "host process has no container binding");

Live log (`/var/log/op-grpc-bridge-direct.log`, 2026-11-19 14:51:58 EST):
`native lungs identity binding refused error=host process has no container binding`,
then `lungs identity bootstrap timed out` 10 s later.

Git history shows no committed change by astra here. The refusal has been there since
`4a6a483` ("one shared lungs socket, bound per connection") and was carried into the new runtime
by `7c38825`. The legacy `lungs_socket.rs:380-384` has the same check, but the running runtime
doesn't use it.

## Change (applied)

In `bind_current_container`, a host-namespace peer no longer goes looking for a container.
It is bound by the kernel proof it already has, as `Binding::Host`, to the bridge's configured
host identity: `OP_LOCAL_HUMAN_SESSION_ID`, falling back to `IDENTITY_SLED_HOST_SESSION_ID`,
which `runtime_identity_owner.rs:970-973` already treats as the host identity.
Container peers are unchanged.

```diff
@@ pub(super) async fn bind_current_container(&self) -> Result<()> {
             ensure!(self.binding.get().is_none(), "lungs connection is already bound");
-            ensure!(self.peer.pid_namespace != self.host.pid_namespace,
-                "host process has no container binding");
+            if self.peer.pid_namespace == self.host.pid_namespace {
+                // The kernel peer proof captured at accept already identifies a
+                // host caller. Accept that one binding: host root maps to the
+                // configured host identity (Bind's own authority, no session
+                // argument); there is no second, container lookup for it.
+                let session_id = host_peer_session(self.host_root(), |name| std::env::var(name).ok())?;
+                return self.publish_binding(Binding::Host, &session_id).await;
+            }
             let mut candidate = self.find_container_binding().await?;
@@
+const HOST_SESSION_SELECTORS: [&str; 2] = ["OP_LOCAL_HUMAN_SESSION_ID", "IDENTITY_SLED_HOST_SESSION_ID"];
+
+fn host_peer_session(host_root: bool, lookup: impl Fn(&str) -> Option<String>) -> Result<String> {
+    ensure!(host_root, "host process has no container binding");
+    let session_id = HOST_SESSION_SELECTORS.iter()
+        .find_map(|name| lookup(name).map(|value| value.trim().to_owned()).filter(|value| !value.is_empty()))
+        .context("host root has no configured host identity")?;
+    ensure!(uuid::Uuid::parse_str(&session_id).is_ok_and(|uuid| uuid.to_string() == session_id),
+        "configured host identity is not canonical");
+    Ok(session_id)
+}
```

After the change, file references are:
- `runtime_lungs_identity.rs:261-289`: `bind_current_container`
- `runtime_lungs_identity.rs:394-410`: `host_peer_session`
- `runtime_lungs_identity.rs:437-461`: test

## Security properties kept

- **No identity, no binding.** `publish_binding` (`:303`) still resolves the session through
  `authoritative_identity`. That requires a committed, anchored sled in the private SHM projection
  plus an unrevoked registered principal. If either is missing, the bind is refused.
- **Non-root host processes stay refused**, with the same error. A shared mapped UID still can't
  claim anything.
- **Host root gains nothing new.** It could already call `Bind(<any session>)`. Now it gets only the
  one operator-configured session, never a session it chooses.
- **Every call is still revalidated.** `revalidate_native_binding` (`:330-344`) rechecks `host_root()`
  for `Binding::Host`, and `revalidate_binding` re-resolves the identity term on every call.
- **Container peers are unchanged:** the native Incus UUID, init process and namespace proof all still apply.

## Tests

- **Added** `host_peers_reuse_their_kernel_binding_and_never_gain_an_identity`. It checks that:
  - non-root is refused with the original message;
  - root with no selector, or a blank one, is refused;
  - non-canonical selectors are refused (path, name, uppercase, no dashes);
  - both selectors resolve;
  - `OP_LOCAL_HUMAN_SESSION_ID` takes precedence.
- **Suggested follow-up for Wonder:** a socket-level test with a committed fixture identity.
  It would show host root `BindCurrentContainer()` binding `Binding::Host`, and an uncommitted
  configured session being refused.

## Risks and open items

- **The live bridge has no committed identities.** `/dev/shm/opdbus/state/identity_sled.json`
  and the credential projection are `{"sleds":[]}`. Even with this fix, host view calls will fail
  next, with `native peer has no committed identity session`, until the configured host human
  (`OP_LOCAL_HUMAN_SESSION_ID`) is initialized or provisioned.
- **`/etc/op-dbus/human-lungs-session` is missing on antix1.** The runit `3tched-fs` and `threetched`
  run scripts expect it, to mount as `host_session` with `--lungs-session`. Mounting with a session is
  the other, already-supported route: `Bind(session)`.
- `mutation_engine.rs:835` `bootstrap_configured_local_human_identity` has no callers in the new
  runtime, so nothing materializes the host human at startup.
- **Not changed:** the legacy `lungs_socket.rs:380-384`, which is unused by the running runtime.

## threetched "missing field `name`"

This error doesn't come from this path. Lungs binding failures surface as D-Bus `AccessDenied`
("lungs identity binding refused"), not as serde errors. None of the `Deserialize` types on the
3tchedFS threetched surface (`threetched-surface.rs`) has a `name` field. It needs its own trace
from the threetched client log.
