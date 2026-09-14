//! Explicit transport of one operator-prepared, read-only Btrfs snapshot.
//!
//! This module never creates snapshots, deploys files, or transfers Snowball's
//! logical recovery directories. The operator prepares the filesystem image
//! and the receiving directory beforehand. Nested subvolumes are not included
//! by Btrfs send; the prepared image must contain the intended ordinary data
//! directories. Incremental receive also requires the unchanged parent at the
//! destination. Native receive errors are preserved, without remote cleanup.

use anyhow::{bail, ensure, Context, Result};
use btrfs_uapi::{filesystem, send_receive, subvolume};
use nix::{fcntl::OFlag, sys::stat::Mode};
use serde::Serialize;
use std::{
    fs::File,
    os::{fd::AsFd, fd::AsRawFd, unix::fs::MetadataExt},
    path::{Component, Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::{io::AsyncReadExt, process::Command};
use uuid::Uuid;

const SUBVOLUME_ROOT_INODE: u64 = 256;
const MAX_DIAGNOSTIC_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone)]
pub struct PreparedSnapshotTransfer {
    pub snapshot: PathBuf,
    pub parent: Option<PathBuf>,
    pub host: String,
    pub remote_directory: String,
    /// An explicitly requested remote executable, invoked with the received
    /// snapshot's absolute path and stream UUID. It is never a shell fragment.
    pub on_receive: Option<String>,
    /// Applies to transfer and, separately, the optional arrival hook.
    pub timeout: Duration,
}

/// Returned only after the native sender and SSH receiver both succeed.
#[derive(Debug, Clone, Serialize)]
pub struct PreparedSnapshotReport {
    pub source: PathBuf,
    pub source_uuid: Uuid,
    /// A retransmitted received snapshot keeps its original received UUID in
    /// the stream; this can differ from its local source UUID.
    pub stream_uuid: Uuid,
    pub parent_uuid: Option<Uuid>,
    pub parent_stream_uuid: Option<Uuid>,
    pub remote_host: String,
    pub received_path: String,
    pub arrival_hook_requested: bool,
    /// A hook failure does not undo or disguise the completed transfer.
    pub arrival_hook_error: Option<String>,
}

#[derive(Debug, Clone)]
struct SnapshotFacts {
    inode: u64,
    filesystem_uuid: Uuid,
    root_id: u64,
    uuid: Uuid,
    parent_uuid: Uuid,
    received_uuid: Uuid,
    name: String,
    readonly: bool,
}

impl SnapshotFacts {
    fn stream_uuid(&self) -> Uuid {
        if self.received_uuid.is_nil() {
            self.uuid
        } else {
            self.received_uuid
        }
    }
}

struct PreparedSnapshot {
    file: File,
    facts: SnapshotFacts,
}

fn native_facts(file: &File) -> Result<SnapshotFacts> {
    let filesystem = filesystem::filesystem_info(file.as_fd())
        .context("source must be on a Btrfs filesystem (native filesystem-info ioctl)")?;
    let info = subvolume::subvolume_info(file.as_fd())
        .context("read native Btrfs subvolume information")?;
    Ok(SnapshotFacts {
        inode: file.metadata()?.ino(),
        filesystem_uuid: filesystem.uuid,
        root_id: info.id,
        uuid: info.uuid,
        parent_uuid: info.parent_uuid,
        received_uuid: info.received_uuid,
        name: info.name,
        readonly: info.flags.contains(subvolume::SubvolumeFlags::RDONLY),
    })
}

fn validate_snapshot(facts: &SnapshotFacts) -> Result<()> {
    ensure!(
        facts.inode == SUBVOLUME_ROOT_INODE,
        "source must be a subvolume root, not a data directory within it"
    );
    ensure!(facts.readonly, "prepared Btrfs snapshot must be read-only");
    ensure!(
        !facts.uuid.is_nil() && !facts.filesystem_uuid.is_nil(),
        "Btrfs snapshot identity is missing"
    );
    ensure!(
        !facts.parent_uuid.is_nil() || !facts.received_uuid.is_nil(),
        "source is a subvolume without snapshot or received-snapshot provenance"
    );
    ensure!(
        !facts.name.is_empty()
            && facts.name != "."
            && facts.name != ".."
            && !facts.name.contains('/')
            && !facts.name.chars().any(char::is_control),
        "native snapshot name is not a safe single path component"
    );
    Ok(())
}

fn validate_parent(source: &SnapshotFacts, parent: &SnapshotFacts) -> Result<()> {
    validate_snapshot(parent).context("invalid incremental parent")?;
    ensure!(
        source.filesystem_uuid == parent.filesystem_uuid,
        "incremental parent must be on the source Btrfs filesystem"
    );
    ensure!(
        source.root_id != parent.root_id
            && source.uuid != parent.uuid
            && source.stream_uuid() != parent.stream_uuid(),
        "incremental parent must be a different snapshot with a distinct stream identity"
    );
    // A preceding sibling snapshot is a valid parent. Do not incorrectly
    // require source.parent_uuid == parent.uuid; the native send ioctl checks
    // the actual incremental relationship and receive checks its remote copy.
    Ok(())
}

fn validate_absolute_path(path: &Path) -> Result<()> {
    let text = path
        .to_str()
        .context("snapshot/remote paths must be UTF-8")?;
    ensure!(
        path.is_absolute() && path != Path::new("/"),
        "path must be absolute and must not be the filesystem root"
    );
    ensure!(
        !text.chars().any(char::is_control)
            && !text.split('/').any(|part| part == "." || part == ".."),
        "path must not contain control characters or dot components"
    );
    Ok(())
}

/// Every component is opened relative to the preceding directory descriptor.
/// This rejects symlink ancestors as well as a symlink final component.
fn open_directory_without_symlinks(path: &Path) -> Result<File> {
    validate_absolute_path(path)?;
    let mut file = File::open("/")?;
    for component in path.components() {
        match component {
            Component::RootDir => {}
            Component::Normal(name) => {
                let fd = nix::fcntl::openat(
                    &file,
                    name,
                    OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC,
                    Mode::empty(),
                )
                .with_context(|| format!("open real directory component in {}", path.display()))?;
                file = File::from(fd);
            }
            _ => bail!("path must contain only normal absolute directory components"),
        }
    }
    Ok(file)
}

fn prepare_with(
    path: &Path,
    inspect: impl FnOnce(&File) -> Result<SnapshotFacts>,
) -> Result<PreparedSnapshot> {
    let file = open_directory_without_symlinks(path)?;
    let facts = inspect(&file)?;
    validate_snapshot(&facts).with_context(|| format!("invalid snapshot {}", path.display()))?;
    Ok(PreparedSnapshot { file, facts })
}

fn validate_host(host: &str) -> Result<()> {
    ensure!(
        !host.is_empty()
            && !host.starts_with('-')
            && host.bytes().all(|byte| byte.is_ascii_alphanumeric()
                || matches!(byte, b'.' | b'-' | b'_' | b'@' | b':' | b'[' | b']' | b'%')),
        "SSH host must be a hostname/address or user@host without options or shell syntax"
    );
    ensure!(host.matches('@').count() <= 1, "invalid SSH user@host");
    ensure!(
        !host.starts_with('@') && !host.ends_with('@'),
        "SSH username and hostname must be nonempty"
    );
    Ok(())
}

fn shell_quote(argument: &str) -> String {
    format!("'{}'", argument.replace('\'', "'\\''"))
}

fn receive_command(directory: &str) -> Result<String> {
    validate_absolute_path(Path::new(directory))?;
    Ok(format!("exec btrfs receive -- {}", shell_quote(directory)))
}

fn hook_command(program: &str, received_path: &str, stream_uuid: Uuid) -> Result<String> {
    validate_absolute_path(Path::new(program))?;
    validate_absolute_path(Path::new(received_path))?;
    Ok(format!(
        "exec {} {} {}",
        shell_quote(program),
        shell_quote(received_path),
        shell_quote(&stream_uuid.to_string())
    ))
}

fn ssh_arguments(host: &str, remote_command: String) -> Result<Vec<String>> {
    validate_host(host)?;
    Ok([
        "-T",
        "-oBatchMode=yes",
        "-oConnectTimeout=15",
        "-oServerAliveInterval=15",
        "-oServerAliveCountMax=3",
        "--",
        host,
        &remote_command,
    ]
    .into_iter()
    .map(str::to_owned)
    .collect())
}

fn ssh_command(arguments: &[String], stdin: Stdio) -> Command {
    let mut command = Command::new("ssh");
    command
        .args(arguments)
        .stdin(stdin)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    command
}

/// Drain the whole pipe concurrently, but retain only bounded diagnostics.
async fn diagnostic_text(mut reader: impl tokio::io::AsyncRead + Unpin) -> Result<String> {
    let mut kept = Vec::new();
    let mut truncated = false;
    let mut buffer = [0_u8; 8192];
    loop {
        let count = reader.read(&mut buffer).await?;
        if count == 0 {
            break;
        }
        let retain = count.min(MAX_DIAGNOSTIC_BYTES - kept.len());
        kept.extend_from_slice(&buffer[..retain]);
        truncated |= retain < count;
    }
    let mut text = String::from_utf8_lossy(&kept).into_owned();
    if truncated {
        text.push_str("\n[diagnostics truncated]");
    }
    Ok(text)
}

async fn run_hook(arguments: &[String], timeout: Duration) -> Result<()> {
    let mut child = ssh_command(arguments, Stdio::null())
        .spawn()
        .context("start explicitly requested SSH arrival hook")?;
    let stderr = child.stderr.take().context("SSH hook stderr missing")?;
    let result = tokio::time::timeout(timeout, async {
        let (status, diagnostic) = tokio::join!(child.wait(), diagnostic_text(stderr));
        let status = status?;
        let diagnostic = diagnostic?;
        ensure!(
            status.success(),
            "arrival hook exited {status}: {diagnostic}"
        );
        Ok(())
    })
    .await;
    match result {
        Ok(result) => result,
        Err(_) => {
            let _ = child.start_kill();
            bail!("arrival hook timed out; transfer already completed; hook outcome is uncertain")
        }
    }
}

/// Send one prepared filesystem snapshot, without creating preparation/cache
/// snapshots or deleting any source or destination. Native receive materializes
/// the one transferred snapshot at the destination.
///
/// Preflight runs before SSH or the send ioctl. File descriptors remain pinned
/// across transfer, so replacing a source pathname cannot redirect the sender.
/// SSH uses existing operator authentication and host-key policy. A timeout or
/// cancellation kills the local SSH process, closing the pipe reader; a blocked
/// kernel writer then fails rather than completing a detached transfer. Kernel
/// I/O itself is not forcibly interruptible, so a stalled filesystem operation
/// may outlive the caller. Failed/uncertain receives may leave partial remote
/// data; this function deliberately neither deletes it nor retries blindly.
pub async fn transfer_prepared_snapshot(
    request: PreparedSnapshotTransfer,
) -> Result<PreparedSnapshotReport> {
    ensure!(
        request.timeout >= Duration::from_secs(1) && request.timeout <= Duration::from_secs(86_400),
        "transfer timeout must be between 1 and 86400 seconds"
    );
    let receive_arguments =
        ssh_arguments(&request.host, receive_command(&request.remote_directory)?)?;
    if let Some(hook) = &request.on_receive {
        validate_absolute_path(Path::new(hook))?;
    }
    let source = prepare_with(&request.snapshot, native_facts)?;
    let parent = request
        .parent
        .as_deref()
        .map(|path| prepare_with(path, native_facts))
        .transpose()?;
    if let Some(parent) = &parent {
        validate_parent(&source.facts, &parent.facts)?;
    }
    let received_path = Path::new(&request.remote_directory)
        .join(&source.facts.name)
        .to_str()
        .context("received path must be UTF-8")?
        .to_owned();
    let hook_arguments = request
        .on_receive
        .as_deref()
        .map(|program| {
            ssh_arguments(
                &request.host,
                hook_command(program, &received_path, source.facts.stream_uuid())?,
            )
        })
        .transpose()?;
    let source_facts = source.facts.clone();
    let parent_uuid = parent.as_ref().map(|parent| parent.facts.uuid);
    let parent_stream_uuid = parent.as_ref().map(|parent| parent.facts.stream_uuid());
    let (read_end, write_end) = nix::unistd::pipe2(OFlag::O_CLOEXEC)?;
    // Drop the Command immediately after spawn so it cannot keep another copy
    // of the pipe reader alive after SSH exits or cancellation kills it.
    let mut child = ssh_command(&receive_arguments, Stdio::from(read_end))
        .spawn()
        .context("start SSH Btrfs receiver")?;
    let stderr = child.stderr.take().context("SSH receive stderr missing")?;
    let sender = tokio::task::spawn_blocking(move || -> Result<()> {
        let parent_root = parent.as_ref().map_or(0, |parent| parent.facts.root_id);
        let result = send_receive::send(
            source.file.as_fd(),
            write_end.as_raw_fd(),
            parent_root,
            &mut [],
            send_receive::SendFlags::empty(),
            0,
        )
        .context("native Btrfs send failed");
        // Close immediately, including on errors, so receive observes EOF.
        drop(write_end);
        result?;
        let after = native_facts(&source.file)?;
        ensure!(
            after.name == source.facts.name
                && after.uuid == source.facts.uuid
                && after.stream_uuid() == source.facts.stream_uuid(),
            "snapshot metadata changed during send; received-path identity is uncertain"
        );
        Ok(())
    });
    let completed = tokio::time::timeout(request.timeout, async {
        let (send_result, status, diagnostic) =
            tokio::join!(sender, child.wait(), diagnostic_text(stderr));
        let diagnostic = diagnostic?;
        let status = status?;
        let send_result = send_result.context("native Btrfs sender task failed")?;
        ensure!(
            status.success() && send_result.is_ok(),
            "prepared snapshot transfer failed; remote data may be partial: sender={send_result:?}; receiver={status}; {diagnostic}"
        );
        Ok(())
    })
    .await;
    match completed {
        Ok(result) => result?,
        Err(_) => {
            let _ = child.start_kill();
            bail!("prepared snapshot transfer timed out; remote data may be partial; no arrival hook ran")
        }
    }
    let arrival_hook_error = if let Some(arguments) = &hook_arguments {
        run_hook(arguments, request.timeout)
            .await
            .err()
            .map(|error| format!("{error:#}"))
    } else {
        None
    };
    Ok(PreparedSnapshotReport {
        source: request.snapshot,
        source_uuid: source_facts.uuid,
        stream_uuid: source_facts.stream_uuid(),
        parent_uuid,
        parent_stream_uuid,
        remote_host: request.host,
        received_path,
        arrival_hook_requested: hook_arguments.is_some(),
        arrival_hook_error,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts() -> SnapshotFacts {
        SnapshotFacts {
            inode: SUBVOLUME_ROOT_INODE,
            filesystem_uuid: Uuid::from_u128(1),
            root_id: 300,
            uuid: Uuid::from_u128(2),
            parent_uuid: Uuid::from_u128(3),
            received_uuid: Uuid::nil(),
            name: "prepared golden".to_owned(),
            readonly: true,
        }
    }

    #[test]
    fn preflight_requires_snapshot_root_readonly_and_provenance() {
        validate_snapshot(&facts()).unwrap();
        for invalid in [
            SnapshotFacts {
                readonly: false,
                ..facts()
            },
            SnapshotFacts {
                inode: 257,
                ..facts()
            },
            SnapshotFacts {
                parent_uuid: Uuid::nil(),
                ..facts()
            },
            SnapshotFacts {
                uuid: Uuid::nil(),
                ..facts()
            },
            SnapshotFacts {
                name: "../escape".into(),
                ..facts()
            },
        ] {
            assert!(validate_snapshot(&invalid).is_err());
        }
        let received = SnapshotFacts {
            parent_uuid: Uuid::nil(),
            received_uuid: Uuid::from_u128(90),
            ..facts()
        };
        validate_snapshot(&received).unwrap();
        assert_eq!(received.stream_uuid(), Uuid::from_u128(90));
    }

    #[test]
    fn incremental_parent_is_distinct_readonly_snapshot_on_same_filesystem() {
        let parent = SnapshotFacts {
            root_id: 301,
            uuid: Uuid::from_u128(4),
            ..facts()
        };
        validate_parent(&facts(), &parent).unwrap();
        assert!(validate_parent(&facts(), &facts()).is_err());
        assert!(validate_parent(
            &facts(),
            &SnapshotFacts {
                readonly: false,
                ..parent.clone()
            }
        )
        .is_err());
        assert!(validate_parent(
            &facts(),
            &SnapshotFacts {
                filesystem_uuid: Uuid::from_u128(99),
                ..parent
            }
        )
        .is_err());
    }

    #[test]
    fn absolute_paths_reject_traversal_controls_and_root() {
        for path in [
            "relative",
            "/",
            "/tmp/../other",
            "/tmp/./other",
            "/tmp/a\nb",
            "/tmp/a\0b",
        ] {
            assert!(validate_absolute_path(Path::new(path)).is_err(), "{path:?}");
        }
        validate_absolute_path(Path::new("/srv/prepared snapshots/golden's image")).unwrap();
    }

    #[test]
    fn descriptor_walk_rejects_final_and_ancestor_symlinks() {
        let temp = tempfile::tempdir().unwrap();
        let actual = temp.path().join("actual");
        std::fs::create_dir_all(actual.join("image")).unwrap();
        std::os::unix::fs::symlink(&actual, temp.path().join("alias")).unwrap();
        assert!(open_directory_without_symlinks(&temp.path().join("alias")).is_err());
        assert!(open_directory_without_symlinks(&temp.path().join("alias/image")).is_err());
        let prepared = prepare_with(&actual.join("image"), |_| Ok(facts())).unwrap();
        let inode = prepared.file.metadata().unwrap().ino();
        std::fs::rename(actual.join("image"), actual.join("moved")).unwrap();
        std::fs::create_dir(actual.join("image")).unwrap();
        assert_eq!(prepared.file.metadata().unwrap().ino(), inode);
        assert_ne!(
            std::fs::metadata(actual.join("image")).unwrap().ino(),
            inode
        );
    }

    #[tokio::test]
    async fn ordinary_directory_is_rejected_before_any_transport() {
        let temp = tempfile::tempdir().unwrap();
        let error = transfer_prepared_snapshot(PreparedSnapshotTransfer {
            snapshot: temp.path().to_owned(),
            parent: None,
            host: "must-never-be-contacted.invalid".into(),
            remote_directory: "/prepared/receive".into(),
            on_receive: None,
            timeout: Duration::from_secs(1),
        })
        .await
        .unwrap_err();
        let message = format!("{error:#}");
        assert!(
            message.contains("Btrfs") || message.contains("subvolume root"),
            "{message}"
        );
        assert!(!message.contains("start SSH"));
    }

    #[test]
    fn remote_arguments_are_one_quoted_command_and_never_ssh_options() {
        let directory = "/receive/golden's $(touch nope); folder";
        let arguments = ssh_arguments("user@host", receive_command(directory).unwrap()).unwrap();
        assert_eq!(arguments.len(), 8);
        assert_eq!(arguments[5], "--");
        assert_eq!(arguments[6], "user@host");
        assert_eq!(
            arguments[7],
            "exec btrfs receive -- '/receive/golden'\\''s $(touch nope); folder'"
        );
        for host in [
            "-oProxyCommand=evil",
            "host;evil",
            "host\nother",
            "user@",
            "@host",
            "user@@host",
            "host $(evil)",
        ] {
            assert!(ssh_arguments(host, "ignored".into()).is_err());
        }
        for host in ["user@host", "10.0.0.2", "user@[2001:db8::1]", "backup-host"] {
            assert!(validate_host(host).is_ok());
        }
    }

    #[test]
    fn arrival_hook_uses_snapshot_path_and_stream_identity_not_logical_counter() {
        let command = hook_command(
            "/opt/my hook",
            "/receive/golden's image",
            Uuid::from_u128(2),
        )
        .unwrap();
        assert_eq!(command, "exec '/opt/my hook' '/receive/golden'\\''s image' '00000000-0000-0000-0000-000000000002'");
        assert!(hook_command("relative-hook", "/receive/image", Uuid::nil()).is_err());
    }

    #[tokio::test]
    async fn diagnostics_are_fully_drained_with_bounded_retention() {
        let bytes = vec![b'x'; MAX_DIAGNOSTIC_BYTES * 3];
        let text = diagnostic_text(bytes.as_slice()).await.unwrap();
        assert!(text.ends_with("[diagnostics truncated]"));
        assert!(text.len() < MAX_DIAGNOSTIC_BYTES + 100);
    }
}
