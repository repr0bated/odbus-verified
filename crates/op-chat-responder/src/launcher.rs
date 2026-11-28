//! Fixed host maintenance launcher for the already provisioned responder.
//! No shell, lifecycle/storage mutation, selected remote, or host chat fallback.

use std::collections::BTreeMap;
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use anyhow::{Context, Result, ensure};
use serde::Deserialize;
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use uuid::Uuid;

use crate::identity::ResponderIdentity;

const INCUS: &str = "/usr/bin/incus";
const MAX_NATIVE_BYTES: u64 = 1024 * 1024;
const NATIVE_QUERY_TIMEOUT: Duration = Duration::from_secs(10);
const CHAT_ROUTE: &str = "/usr/local/bin/chat-route";
const GUEST_MOUNT: &str = "/run/mounts/3tchedFS";

#[derive(Debug, Deserialize)]
struct NativeInstance {
    name: String,
    #[serde(rename = "type")]
    kind: String,
    status: String,
    config: BTreeMap<String, String>,
    #[serde(default)]
    expanded_config: BTreeMap<String, String>,
}

#[derive(Debug, PartialEq, Eq)]
struct ContainerBinding {
    name: String,
    native_uuid: Uuid,
}

struct ProtectedIncus {
    device: u64,
    inode: u64,
}

impl ProtectedIncus {
    fn open() -> Result<Self> {
        let metadata = protected_client()?;
        Ok(Self { device: metadata.dev(), inode: metadata.ino() })
    }

    fn command(&self) -> Result<Command> {
        let metadata = protected_client()?;
        ensure!(metadata.dev() == self.device && metadata.ino() == self.inode,
            "protected Incus executable changed; stop without replay");
        let mut command = Command::new(INCUS);
        command.args(["--force-local", "--project", "default"])
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .current_dir("/")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        Ok(command)
    }
}

fn protected_client() -> Result<std::fs::Metadata> {
    for directory in ["/", "/usr", "/usr/bin"] {
        let metadata = std::fs::symlink_metadata(directory)?;
        ensure!(metadata.is_dir() && metadata.uid() == 0 && metadata.mode() & 0o022 == 0,
            "Incus client ancestor is not a protected root-owned directory");
    }
    let metadata = std::fs::symlink_metadata(Path::new(INCUS))?;
    ensure!(metadata.is_file() && metadata.uid() == 0
        && metadata.mode() & 0o022 == 0 && metadata.mode() & 0o111 != 0,
        "Incus client is not a protected root-owned executable");
    Ok(metadata)
}

pub(crate) struct ContainerLauncher {
    identity: ResponderIdentity,
    incus: ProtectedIncus,
    binding: ContainerBinding,
}

impl ContainerLauncher {
    pub async fn connect(identity: ResponderIdentity) -> Result<Self> {
        identity.revalidate()?;
        let incus = ProtectedIncus::open()?;
        let binding = resolve_binding(&incus, &identity).await?;
        Ok(Self { identity, incus, binding })
    }

    pub async fn process(&self, turn_id: Uuid) -> Result<()> {
        self.identity.revalidate()?;
        ensure!(resolve_binding(&self.incus, &self.identity).await? == self.binding,
            "configured responder container binding changed; stop without replay");
        let mut command = self.incus.command()?;
        configure_exec(&mut command, &self.binding, turn_id);
        let mut child = command.spawn().context("bound container execution could not start")?;
        let status = child.wait().await
            .context("bound container execution outcome unknown; do not replay")?;
        // The container CLI's exit zero means its original response receipt and
        // committed-turn readback matched. Never use private stdout as success.
        ensure!(status.success(), "container response was not confirmed; do not replay");
        self.identity.revalidate()?;
        ensure!(resolve_binding(&self.incus, &self.identity).await? == self.binding,
            "responder container changed after execution; outcome unknown; do not replay");
        Ok(())
    }
}

fn configure_exec(command: &mut Command, binding: &ContainerBinding, turn_id: Uuid) {
    command.args(["exec", "--mode", "non-interactive", "--disable-stdin", "--cwd", "/"])
        .arg(&binding.name)
        .arg("--")
        .arg(CHAT_ROUTE)
        .arg("--process-pending-turn")
        .arg(turn_id.to_string())
        .args(["--mount", GUEST_MOUNT]);
}

async fn resolve_binding(incus: &ProtectedIncus, identity: &ResponderIdentity) -> Result<ContainerBinding> {
    let mut command = incus.command()?;
    command.args(["list", "--format", "json", "type=container"])
        .arg(format!("user.opdbus.session_id={}", identity.session_id))
        .arg(format!("user.opdbus.wireguard_public_key={}", identity.wireguard_pubkey))
        .stdout(Stdio::piped());
    let bytes = tokio::time::timeout(NATIVE_QUERY_TIMEOUT, async {
        let mut child = command.spawn().context("native responder binding query could not start")?;
        let stdout = child.stdout.take().context("native responder binding output unavailable")?;
        let mut bytes = Vec::new();
        stdout.take(MAX_NATIVE_BYTES + 1).read_to_end(&mut bytes).await
            .context("native responder binding query failed")?;
        ensure!(bytes.len() as u64 <= MAX_NATIVE_BYTES, "native responder binding output exceeds bound");
        ensure!(child.wait().await?.success(), "native responder binding query refused");
        Ok::<_, anyhow::Error>(bytes)
    }).await.context("native responder binding query timed out")??;
    let instances: Vec<NativeInstance> = serde_json::from_slice(&bytes)
        .map_err(|_| anyhow::anyhow!("invalid native responder binding response"))?;
    select_binding(instances, &identity.session_id, &identity.wireguard_pubkey)
}

fn select_binding(instances: Vec<NativeInstance>, session: &str, key: &str) -> Result<ContainerBinding> {
    let mut matches = instances.into_iter().filter(|instance| {
        instance.kind == "container"
            && instance.config.get("user.opdbus.session_id").map(String::as_str) == Some(session)
            && instance.config.get("user.opdbus.wireguard_public_key").map(String::as_str) == Some(key)
    });
    let instance = matches.next().context("configured responder has no existing native container")?;
    ensure!(matches.next().is_none(), "configured responder native container is ambiguous");
    ensure!(instance.status == "Running", "configured responder container is not running");
    ensure!(instance.name.len() <= 63 && instance.name.as_bytes().first().is_some_and(u8::is_ascii_alphanumeric)
        && instance.name.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'-'),
        "native responder container name is invalid");
    let privileged = instance.expanded_config.get("security.privileged")
        .or_else(|| instance.config.get("security.privileged"));
    ensure!(privileged.is_none_or(|value| value == "false"), "responder container must remain unprivileged");
    let native_uuid = instance.config.get("volatile.uuid").context("native responder UUID missing")?;
    let parsed = Uuid::parse_str(native_uuid)?;
    ensure!(parsed.to_string() == *native_uuid, "native responder UUID is not canonical");
    Ok(ContainerBinding { name: instance.name, native_uuid: parsed })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn instance() -> NativeInstance {
        NativeInstance {
            name: "BoundResponder".into(), kind: "container".into(), status: "Running".into(),
            config: BTreeMap::from([
                ("user.opdbus.session_id".into(), "session".into()),
                ("user.opdbus.wireguard_public_key".into(), "public-key".into()),
                ("volatile.uuid".into(), "00000000-0000-4000-8000-000000000001".into()),
            ]), expanded_config: BTreeMap::new(),
        }
    }

    #[test]
    fn native_binding_requires_one_running_unprivileged_exact_service() {
        assert!(select_binding(vec![instance()], "session", "public-key").is_ok());
        assert!(select_binding(vec![instance()], "another-session", "public-key").is_err());
        assert!(select_binding(vec![instance()], "session", "another-key").is_err());
        assert!(select_binding(vec![instance(), instance()], "session", "public-key").is_err());
        let mut stopped = instance();
        stopped.status = "Stopped".into();
        assert!(select_binding(vec![stopped], "session", "public-key").is_err());
        let mut privileged = instance();
        privileged.expanded_config.insert("security.privileged".into(), "true".into());
        assert!(select_binding(vec![privileged], "session", "public-key").is_err());
    }

    #[test]
    fn native_uuid_and_name_cannot_be_substituted_by_client_selectors() {
        let mut invalid = instance();
        invalid.config.insert("volatile.uuid".into(), "not-a-uuid".into());
        assert!(select_binding(vec![invalid], "session", "public-key").is_err());
        let mut remote = instance();
        remote.name = "remote:other".into();
        assert!(select_binding(vec![remote], "session", "public-key").is_err());
    }

    #[test]
    fn execution_is_fixed_argv_inside_the_bound_container_without_shell() {
        let binding = select_binding(vec![instance()], "session", "public-key").unwrap();
        let mut command = Command::new(INCUS);
        configure_exec(&mut command, &binding, binding.native_uuid);
        let arguments: Vec<_> = command.as_std().get_args()
            .map(|argument| argument.to_str().expect("fixed UTF-8 argv")).collect();
        assert_eq!(arguments, ["exec", "--mode", "non-interactive", "--disable-stdin", "--cwd", "/",
            "BoundResponder", "--", CHAT_ROUTE, "--process-pending-turn",
            "00000000-0000-4000-8000-000000000001", "--mount", GUEST_MOUNT]);
    }
}
