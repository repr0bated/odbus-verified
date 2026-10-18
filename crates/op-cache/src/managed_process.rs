//! Bounded process I/O and explicit ownership for cache filesystem/transport work.
//!
//! Dropping a cache operation kills its owned children. Reaping is delegated to
//! the runtime only on cancellation; the normal path waits for each child.
use anyhow::{Context, Result, ensure};
use std::process::{ExitStatus, Stdio};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, Command};

pub(crate) const OUTPUT_LIMIT: usize = 64 * 1024;
pub(crate) const COMMAND_TIMEOUT: Duration = Duration::from_secs(60);

pub(crate) struct Output {
    pub status: ExitStatus,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

struct OwnedChild(Option<Child>);

impl OwnedChild {
    fn spawn(command: &mut Command) -> Result<Self> {
        Ok(Self(Some(command.kill_on_drop(true).spawn()?)))
    }

    fn child(&mut self) -> &mut Child {
        self.0.as_mut().expect("child exists until it is reaped")
    }

    async fn wait(&mut self) -> Result<ExitStatus> {
        let status = self.child().wait().await?;
        self.0.take();
        Ok(status)
    }

    async fn wait_success(&mut self, label: &str) -> Result<ExitStatus> {
        let status = self.wait().await?;
        ensure!(status.success(), "{label} exited with {status}");
        Ok(status)
    }
}

impl Drop for OwnedChild {
    fn drop(&mut self) {
        if let Some(mut child) = self.0.take() {
            // Send the signal synchronously, before a replacement operation can
            // start. Child::wait itself is cancellation safe. Tokio's orphan
            // reaper is the fallback if the runtime is already shutting down.
            let _ = child.start_kill();
            if let Ok(runtime) = tokio::runtime::Handle::try_current() {
                runtime.spawn(async move {
                    let _ = child.wait().await;
                });
            }
        }
    }
}

async fn read_output(mut stream: impl AsyncRead + Unpin) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut buffer = [0u8; 8192];
    loop {
        let count = stream.read(&mut buffer).await?;
        if count == 0 {
            return Ok(bytes);
        }
        ensure!(
            count <= OUTPUT_LIMIT.saturating_sub(bytes.len()),
            "cache subprocess output exceeded {OUTPUT_LIMIT} bytes"
        );
        bytes.extend_from_slice(&buffer[..count]);
    }
}

/// Capture only small command responses. Overflow, timeout, I/O failure and
/// caller cancellation all drop the child owner and terminate the process.
pub(crate) async fn output(command: &mut Command, deadline: Duration) -> Result<Output> {
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut process = OwnedChild::spawn(command)?;
    let stdout = process.child().stdout.take().context("missing stdout")?;
    let stderr = process.child().stderr.take().context("missing stderr")?;
    let (stdout, stderr, status) = tokio::time::timeout(deadline, async {
        tokio::try_join!(read_output(stdout), read_output(stderr), process.wait())
    })
    .await
    .context("cache subprocess timed out")??;
    Ok(Output {
        status,
        stdout,
        stderr,
    })
}

/// Copy the send stream with Tokio's fixed-size I/O buffer. No detached copier
/// task or whole-stream capture exists. An early failed child aborts the peer.
pub(crate) async fn pipeline(
    sender: &mut Command,
    receiver: &mut Command,
    deadline: Duration,
) -> Result<u64> {
    sender.stdout(Stdio::piped()).stderr(Stdio::piped());
    receiver
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // Wrap the sender before spawning the receiver, including spawn failure.
    let mut sender = OwnedChild::spawn(sender)?;
    let mut receiver = OwnedChild::spawn(receiver)?;
    let mut source = sender
        .child()
        .stdout
        .take()
        .context("missing send stream")?;
    let send_error = sender
        .child()
        .stderr
        .take()
        .context("missing send stderr")?;
    let mut sink = receiver
        .child()
        .stdin
        .take()
        .context("missing receive stream")?;
    let receive_output = receiver
        .child()
        .stdout
        .take()
        .context("missing receive stdout")?;
    let receive_error = receiver
        .child()
        .stderr
        .take()
        .context("missing receive stderr")?;
    let (bytes, _, _, _, _, _) = tokio::time::timeout(deadline, async {
        tokio::try_join!(
            async {
                let bytes = tokio::io::copy(&mut source, &mut sink).await?;
                sink.shutdown().await?;
                drop(sink);
                Ok::<_, anyhow::Error>(bytes)
            },
            read_output(send_error),
            read_output(receive_output),
            read_output(receive_error),
            sender.wait_success("Btrfs sender"),
            receiver.wait_success("Btrfs receiver"),
        )
    })
    .await
    .context("Btrfs transfer timed out")??;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    async fn gone(pid: u32) {
        let path = format!("/proc/{pid}");
        tokio::time::timeout(Duration::from_secs(3), async {
            while Path::new(&path).exists() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("owned child was not killed and reaped");
    }

    #[tokio::test]
    async fn output_rejects_unbounded_diagnostics() {
        let mut command = Command::new("sh");
        command.args([
            "-c",
            "while :; do printf '0123456789abcdef0123456789abcdef\\n'; done",
        ]);
        let error = output(&mut command, Duration::from_secs(5))
            .await
            .err()
            .unwrap();
        assert!(error.to_string().contains("output exceeded"), "{error:#}");
    }

    #[tokio::test]
    async fn cancellation_kills_and_reaps_the_owner() {
        let mut command = Command::new("sleep");
        command.arg("30");
        let mut child = OwnedChild::spawn(&mut command).unwrap();
        let pid = child.child().id().unwrap();
        drop(child);
        gone(pid).await;
    }

    #[tokio::test]
    async fn pipeline_streams_bytes_without_capturing_them() {
        let mut sender = Command::new("sh");
        sender.args(["-c", "printf 'synthetic-cache-payload'"]);
        let mut receiver = Command::new("sh");
        receiver.args(["-c", "test \"$(cat)\" = synthetic-cache-payload"]);
        assert_eq!(
            pipeline(&mut sender, &mut receiver, Duration::from_secs(5))
                .await
                .unwrap(),
            23
        );
    }

    #[tokio::test]
    async fn failed_sender_does_not_wait_for_stalled_receiver() {
        let mut sender = Command::new("sh");
        sender.args(["-c", "exit 7"]);
        let mut receiver = Command::new("sleep");
        receiver.arg("30");
        let error = tokio::time::timeout(
            Duration::from_secs(2),
            pipeline(&mut sender, &mut receiver, Duration::from_secs(30)),
        )
        .await
        .unwrap()
        .unwrap_err();
        assert!(error.to_string().contains("sender"), "{error:#}");
    }

    #[tokio::test]
    async fn failed_receiver_spawn_reaps_sender() {
        let fixture = tempfile::tempdir().unwrap();
        let pidfile = fixture.path().join("sender.pid");
        let mut sender = Command::new("sh");
        sender
            .arg("-c")
            .arg("echo $$ > \"$1\"; exec sleep 30")
            .arg("probe")
            .arg(&pidfile);
        let mut receiver = Command::new("/does-not-exist-cache-receiver");
        assert!(
            pipeline(&mut sender, &mut receiver, Duration::from_secs(3))
                .await
                .is_err()
        );
        // Sender can be killed before it executes; if it got far enough to
        // publish its PID, it must disappear. A separate direct owner test
        // establishes reaping without depending on this scheduling race.
        tokio::time::sleep(Duration::from_millis(100)).await;
        if let Ok(pid) = std::fs::read_to_string(pidfile) {
            gone(pid.trim().parse().unwrap()).await;
        }
    }

    #[tokio::test]
    async fn cancelling_pipeline_kills_both_processes() {
        let fixture = tempfile::tempdir().unwrap();
        let first = fixture.path().join("sender.pid");
        let second = fixture.path().join("receiver.pid");
        let mut sender = Command::new("sh");
        sender
            .arg("-c")
            .arg("echo $$ > \"$1\"; exec sleep 30")
            .arg("probe")
            .arg(&first);
        let mut receiver = Command::new("sh");
        receiver
            .arg("-c")
            .arg("echo $$ > \"$1\"; exec sleep 30")
            .arg("probe")
            .arg(&second);
        let pids = {
            let future = pipeline(&mut sender, &mut receiver, Duration::from_secs(30));
            tokio::pin!(future);
            tokio::select! {
                result = &mut future => panic!("pipeline completed early: {result:?}"),
                _ = tokio::time::timeout(Duration::from_secs(3), async {
                    while !first.exists() || !second.exists() {
                        tokio::time::sleep(Duration::from_millis(10)).await;
                    }
                }) => {},
            }
            [
                std::fs::read_to_string(first).unwrap(),
                std::fs::read_to_string(second).unwrap(),
            ]
        }; // The actual pipeline future is dropped here, not just its Pin.
        for pid in pids {
            gone(pid.trim().parse().unwrap()).await;
        }
    }
}
