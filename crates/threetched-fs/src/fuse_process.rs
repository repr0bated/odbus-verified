//! Bounded execution of the binary-backed filesystem surface.
//!
//! The child owns a new process group. Its leader remains unreaped until group
//! cleanup is complete, so a group signal cannot race reuse of the leader PID.
//! Both pipes are drained on this thread; no reader can outlive the mount job.

use crate::fuse_buffers::{Budget, Buffer, DOCUMENT_LIMIT};
use std::io::{self, Read};
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};

const POLL_INTERVAL: Duration = Duration::from_millis(50);
const READ_CHUNK: usize = 8192;

/// Discovery uses the same bounded child lifetime as mounted execution,
/// without depending on the FUSE engine or importing it into read examples.
#[cfg(test)]
pub(crate) fn capture_surface(binary: &Path, args: &[&str]) -> anyhow::Result<String> {
    capture_surface_with_config(binary, args, None)
}

pub(crate) fn capture_surface_with_config(
    binary: &Path,
    args: &[&str],
    config_dir: Option<&Path>,
) -> anyhow::Result<String> {
    let budget = Arc::new(Budget::default());
    let cancelled = AtomicBool::new(false);
    let mut command = Command::new(binary);
    if let Some(config_dir) = config_dir {
        anyhow::ensure!(config_dir.is_absolute(), "capture config directory must be absolute");
        command.env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", config_dir.join("home"))
            .env("THREETCHED_CONFIG_DIR", config_dir)
            .env("ZEROCLAW_CONFIG_DIR", config_dir);
    }
    command
        .args(args)
        .env("NO_COLOR", "1")
        .env("CLICOLOR", "0")
        .env("TERM", "dumb")
        .env("LANGUAGE", "en")
        .env("LC_ALL", "C")
        .env("LANG", "C");
    let output = run(
        &mut command,
        &budget,
        &cancelled,
        Limits {
            timeout: Duration::from_secs(10),
            output: 4 * 1024 * 1024,
        },
    )?;
    anyhow::ensure!(
        output.status.success(),
        "{} {:?} exited with {}: {}",
        binary.display(),
        args,
        output.status,
        String::from_utf8_lossy(&output.stderr.bytes)
    );
    Ok(std::str::from_utf8(&output.stdout.bytes)?.to_owned())
}

#[derive(Clone, Copy)]
pub(super) struct Limits {
    pub(super) timeout: Duration,
    /// Combined stdout and stderr length, additionally capped by DOCUMENT_LIMIT.
    pub(super) output: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(600),
            output: DOCUMENT_LIMIT,
        }
    }
}

pub(super) struct Output {
    pub(super) stdout: Buffer,
    pub(super) stderr: Buffer,
    pub(super) status: ExitStatus,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("upstream execution was not started (errno {0})")]
    NotStarted(i32),
    #[error("upstream process could not be spawned: {0}")]
    Spawn(io::Error),
    #[error("upstream process I/O failed: {0}; outcome unknown, no automatic retry")]
    Io(#[from] io::Error),
    #[error(
        "upstream process exceeded its output or memory budget (errno {0}); outcome unknown, no automatic retry"
    )]
    Resource(i32),
    #[error("upstream process timed out; outcome unknown, no automatic retry")]
    Timeout,
    #[error("upstream process was cancelled; outcome unknown, no automatic retry")]
    Cancelled,
    #[error("threetched output is not UTF-8; outcome unknown, no automatic retry")]
    InvalidUtf8,
}

impl Error {
    pub fn stage(&self) -> &'static str {
        match self {
            Self::NotStarted(_) => "admission",
            Self::Spawn(_) => "transport",
            _ => "outcome_unknown",
        }
    }

    pub fn errno(&self) -> i32 {
        match self {
            Self::NotStarted(_) => libc::EBUSY,
            _ => libc::EIO,
        }
    }
}

/// Own output and its byte charges until backend receipt serialization finishes.
#[derive(serde::Serialize)]
pub struct CommandOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit: i32,
    #[serde(skip)]
    _charges: (crate::fuse_buffers::Charge, crate::fuse_buffers::Charge),
}

/// Execute an admitted, caller-configured command using the one bounded runner.
/// The caller owns identity, grants, environment, configuration and OS privileges.
pub fn run_command(command: &mut Command, cancelled: &AtomicBool) -> Result<CommandOutput, Error> {
    let budget = Arc::new(Budget::default());
    let output = run(command, &budget, cancelled, Limits::default())?;
    let exit = output.status.code().unwrap_or(-1);
    let (stdout, stdout_charge) = output.stdout.into_parts();
    let (stderr, stderr_charge) = output.stderr.into_parts();
    Ok(CommandOutput {
        stdout: String::from_utf8(stdout).map_err(|_| Error::InvalidUtf8)?,
        stderr: String::from_utf8(stderr).map_err(|_| Error::InvalidUtf8)?,
        exit,
        _charges: (stdout_charge, stderr_charge),
    })
}

struct OwnedChild {
    child: Child,
    owned: bool,
    group_killed: bool,
}

impl OwnedChild {
    fn new(child: Child) -> Self {
        Self {
            child,
            owned: true,
            group_killed: false,
        }
    }

    fn exited(&mut self) -> io::Result<bool> {
        // SAFETY: siginfo_t is an output parameter and zero is a valid initial
        // representation. WNOWAIT observes our child without releasing its PID.
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        let result = unsafe {
            libc::waitid(
                libc::P_PID,
                self.child.id(),
                &mut info,
                libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
            )
        };
        if result == -1 {
            let error = io::Error::last_os_error();
            if error.raw_os_error() == Some(libc::ECHILD) {
                // An external reaper has removed ownership. Never signal a PID
                // or group that might now refer to an unrelated process.
                self.owned = false;
            }
            if error.kind() == io::ErrorKind::Interrupted {
                return Ok(false);
            }
            return Err(error);
        }
        // SAFETY: waitid initialized info, and si_pid is valid for child status.
        Ok(unsafe { info.si_pid() } != 0)
    }

    fn kill_group(&mut self) -> io::Result<()> {
        if !self.owned || self.group_killed {
            return Ok(());
        }
        let pid = i32::try_from(self.child.id())
            .map_err(|_| io::Error::other("child PID exceeds pid_t"))?;
        // SAFETY: Command::process_group(0) created this group. The owned,
        // unreaped leader pins its identifier until the following wait.
        if unsafe { libc::kill(-pid, libc::SIGKILL) } == -1 {
            let error = io::Error::last_os_error();
            if error.raw_os_error() != Some(libc::ESRCH) {
                return Err(error);
            }
        }
        self.group_killed = true;
        Ok(())
    }

    fn reap(&mut self) -> io::Result<ExitStatus> {
        loop {
            match self.child.wait() {
                Ok(status) => {
                    self.owned = false;
                    return Ok(status);
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => {
                    if error.raw_os_error() == Some(libc::ECHILD) {
                        self.owned = false;
                    }
                    return Err(error);
                }
            }
        }
    }
}

impl Drop for OwnedChild {
    fn drop(&mut self) {
        if self.owned {
            let _ = self.kill_group();
            let _ = self.reap();
        }
    }
}

fn nonblocking(fd: RawFd) -> io::Result<()> {
    // SAFETY: the caller retains its owned pipe throughout these fcntl calls.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags == -1 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } == -1 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

fn drain(
    pipe: &mut impl Read,
    output: &mut Buffer,
    other_length: usize,
    ceiling: usize,
) -> Result<bool, Error> {
    let mut scratch = [0_u8; READ_CHUNK];
    // Read at most one chunk per stream per iteration, so a noisy stream cannot
    // starve stderr, child status, cancellation or the deadline.
    let count = match pipe.read(&mut scratch) {
        Ok(0) => return Ok(true),
        Ok(count) => count,
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
            ) =>
        {
            return Ok(false);
        }
        Err(error) => return Err(Error::Io(error)),
    };
    let previous = output.bytes.len();
    let length = previous
        .checked_add(count)
        .filter(|next| other_length <= ceiling && *next <= ceiling - other_length)
        .ok_or(Error::Resource(libc::EFBIG))?;
    if length > output.bytes.capacity() {
        let capacity = length
            .max(output.bytes.capacity().saturating_mul(2))
            .min(ceiling);
        output.resize(capacity, ceiling).map_err(Error::Resource)?;
    }
    output.bytes.resize(length, 0);
    output.bytes[previous..length].copy_from_slice(&scratch[..count]);
    Ok(false)
}

pub(super) fn run(
    command: &mut Command,
    budget: &Arc<Budget>,
    cancellation: &AtomicBool,
    limits: Limits,
) -> Result<Output, Error> {
    if cancellation.load(Ordering::Acquire) {
        return Err(Error::NotStarted(libc::ECANCELED));
    }
    let deadline = Instant::now()
        .checked_add(limits.timeout)
        .ok_or(Error::NotStarted(libc::EINVAL))?;
    if limits.timeout.is_zero() {
        return Err(Error::NotStarted(libc::ETIMEDOUT));
    }
    let ceiling = limits.output.min(DOCUMENT_LIMIT);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0);
    let mut child = OwnedChild::new(command.spawn().map_err(Error::Spawn)?);
    let mut stdout_pipe = child
        .child
        .stdout
        .take()
        .ok_or_else(|| io::Error::other("missing upstream stdout pipe"))?;
    let mut stderr_pipe = child
        .child
        .stderr
        .take()
        .ok_or_else(|| io::Error::other("missing upstream stderr pipe"))?;
    nonblocking(stdout_pipe.as_raw_fd())?;
    nonblocking(stderr_pipe.as_raw_fd())?;
    let mut stdout = Buffer::empty(budget);
    let mut stderr = Buffer::empty(budget);
    let mut stdout_eof = false;
    let mut stderr_eof = false;
    let mut exited = false;

    loop {
        if cancellation.load(Ordering::Acquire) {
            return Err(Error::Cancelled);
        }
        if Instant::now() >= deadline {
            return Err(Error::Timeout);
        }
        if !stdout_eof {
            stdout_eof = drain(&mut stdout_pipe, &mut stdout, stderr.bytes.len(), ceiling)?;
        }
        if !stderr_eof {
            stderr_eof = drain(&mut stderr_pipe, &mut stderr, stdout.bytes.len(), ceiling)?;
        }
        if !exited && child.exited()? {
            exited = true;
            // A descendant may retain either pipe after the leader exits. Kill
            // the owned group while its zombie leader still pins the group ID.
            child.kill_group()?;
        }
        if exited && stdout_eof && stderr_eof {
            let status = child.reap()?;
            return Ok(Output {
                stdout,
                stderr,
                status,
            });
        }

        let mut descriptors = [
            libc::pollfd {
                fd: if stdout_eof {
                    -1
                } else {
                    stdout_pipe.as_raw_fd()
                },
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: if stderr_eof {
                    -1
                } else {
                    stderr_pipe.as_raw_fd()
                },
                events: libc::POLLIN,
                revents: 0,
            },
        ];
        let remaining = deadline.saturating_duration_since(Instant::now());
        let timeout = remaining.min(POLL_INTERVAL).as_millis() as i32;
        // SAFETY: the fixed array and both owned descriptors remain live during
        // poll. EOF streams use -1, which poll explicitly ignores.
        if unsafe { libc::poll(descriptors.as_mut_ptr(), 2, timeout) } == -1 {
            let error = io::Error::last_os_error();
            if error.kind() != io::ErrorKind::Interrupted {
                return Err(Error::Io(error));
            }
        }
        if descriptors
            .iter()
            .any(|descriptor| descriptor.revents & libc::POLLNVAL != 0)
        {
            return Err(Error::Io(io::Error::from_raw_os_error(libc::EBADF)));
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::fuse_buffers::TOTAL_LIMIT;

    #[test]
    fn metadata_capture_declares_stable_locale_and_plain_output() {
        let text = capture_surface(
            Path::new("/bin/sh"),
            &[
                "-c",
                "printf '%s' \"$LANGUAGE|$LC_ALL|$LANG|$NO_COLOR|$CLICOLOR|$TERM\"",
            ],
        )
        .unwrap();
        assert_eq!(text, "en|C|C|1|0|dumb");
    }

    #[test]
    fn configured_capture_uses_selected_config_home_and_clean_path() {
        let directory = tempfile::tempdir().unwrap();
        let text = capture_surface_with_config(Path::new("/bin/sh"), &[
            "-c", "printf '%s' \"$THREETCHED_CONFIG_DIR|$ZEROCLAW_CONFIG_DIR|$HOME|$PATH|$LANGUAGE\"",
        ], Some(directory.path())).unwrap();
        assert_eq!(text, format!("{}|{}|{}|/usr/bin:/bin|en", directory.path().display(),
            directory.path().display(), directory.path().join("home").display()));
    }

    #[test]
    fn shared_backend_execution_retains_literal_output_and_pre_spawn_cancellation() {
        let cancelled = AtomicBool::new(false);
        let mut command = fixture("printf hello; printf error >&2; exit 7");
        let output = run_command(&mut command, &cancelled).unwrap();
        assert_eq!(output.stdout, "hello");
        assert_eq!(output.stderr, "error");
        assert_eq!(output.exit, 7);
        assert_eq!(serde_json::to_value(&output).unwrap(),
            serde_json::json!({"stdout":"hello","stderr":"error","exit":7}));
        let cancelled = AtomicBool::new(true);
        assert!(matches!(run_command(&mut fixture("exit 91"), &cancelled), Err(Error::NotStarted(_))));
    }
    use super::*;

    fn fixture(script: &str) -> Command {
        let mut command = Command::new("/bin/sh");
        command.args(["-c", script]);
        command
    }

    fn limits() -> Limits {
        Limits {
            timeout: Duration::from_secs(2),
            output: 32 * 1024,
        }
    }

    #[test]
    fn success_and_nonzero_keep_both_streams_charged() {
        for exit in [0, 7] {
            let budget = Arc::new(Budget::default());
            let script = format!("printf hello; printf problem >&2; exit {exit}");
            let output = run(
                &mut fixture(&script),
                &budget,
                &AtomicBool::new(false),
                limits(),
            )
            .unwrap();
            assert_eq!(output.status.code(), Some(exit));
            assert_eq!(output.stdout.bytes, b"hello");
            assert_eq!(output.stderr.bytes, b"problem");
            assert!(budget.take(TOTAL_LIMIT, TOTAL_LIMIT).is_err());
            drop(output);
            assert!(budget.take(TOTAL_LIMIT, TOTAL_LIMIT).is_ok());
        }
    }

    #[test]
    fn drains_both_streams_without_pipe_deadlock() {
        let output = run(
            &mut fixture(
                "i=0; while [ \"$i\" -lt 3000 ]; do printf err >&2; printf out; i=$((i + 1)); done",
            ),
            &Arc::new(Budget::default()),
            &AtomicBool::new(false),
            limits(),
        )
        .unwrap();
        assert!(output.status.success());
        assert_eq!(output.stdout.bytes.len(), 9000);
        assert_eq!(output.stderr.bytes.len(), 9000);
    }

    #[test]
    fn combined_output_overflow_returns_charges() {
        let budget = Arc::new(Budget::default());
        let result = run(
            &mut fixture("printf 12345678; printf 12345678 >&2; sleep 30"),
            &budget,
            &AtomicBool::new(false),
            Limits {
                output: 12,
                ..limits()
            },
        );
        assert!(matches!(result, Err(Error::Resource(libc::EFBIG))));
        assert!(budget.take(TOTAL_LIMIT, TOTAL_LIMIT).is_ok());
    }

    #[test]
    fn memory_exhaustion_returns_charges() {
        let budget = Arc::new(Budget::default());
        let reservation = budget.take(TOTAL_LIMIT, TOTAL_LIMIT).unwrap();
        let result = run(
            &mut fixture("printf output; sleep 30"),
            &budget,
            &AtomicBool::new(false),
            limits(),
        );
        assert!(matches!(result, Err(Error::Resource(libc::ENOMEM))));
        drop(reservation);
        assert!(budget.take(TOTAL_LIMIT, TOTAL_LIMIT).is_ok());
    }

    #[test]
    fn timeout_kills_and_reaps_the_child() {
        let started = Instant::now();
        let result = run(
            &mut fixture("sleep 30"),
            &Arc::new(Budget::default()),
            &AtomicBool::new(false),
            Limits {
                timeout: Duration::from_millis(50),
                ..limits()
            },
        );
        assert!(matches!(result, Err(Error::Timeout)));
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn cancellation_stops_execution_and_prevents_new_spawn() {
        let cancellation = AtomicBool::new(false);
        let result = std::thread::scope(|scope| {
            scope.spawn(|| {
                std::thread::sleep(Duration::from_millis(30));
                cancellation.store(true, Ordering::Release);
            });
            run(
                &mut fixture("sleep 30"),
                &Arc::new(Budget::default()),
                &cancellation,
                limits(),
            )
        });
        assert!(matches!(result, Err(Error::Cancelled)));
        let result = run(
            &mut fixture("exit 0"),
            &Arc::new(Budget::default()),
            &cancellation,
            limits(),
        );
        assert!(matches!(result, Err(Error::NotStarted(libc::ECANCELED))));
    }

    #[test]
    fn leader_exit_terminates_descendant_holding_the_pipes() {
        let directory = tempfile::tempdir().unwrap();
        let pid_file = directory.path().join("child-pid");
        let mut command = fixture("sleep 30 & printf '%s' \"$!\" > \"$1\"; exit 0");
        command.arg("fixture").arg(&pid_file);
        let started = Instant::now();
        let output = run(
            &mut command,
            &Arc::new(Budget::default()),
            &AtomicBool::new(false),
            limits(),
        )
        .unwrap();
        assert!(output.status.success());
        assert!(started.elapsed() < Duration::from_secs(2));
        let pid: u32 = std::fs::read_to_string(pid_file).unwrap().parse().unwrap();
        // An adopted descendant may remain a zombie until the system reaper
        // handles it. It must no longer execute or retain the output pipes.
        let deadline = Instant::now() + Duration::from_millis(500);
        loop {
            match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
                Err(error) if error.kind() == io::ErrorKind::NotFound => break,
                Ok(stat) if stat.rsplit_once(") ").unwrap().1.starts_with(['Z', 'X']) => break,
                stat => {
                    assert!(
                        Instant::now() < deadline,
                        "descendant still alive: {stat:?}"
                    );
                    std::thread::sleep(Duration::from_millis(10));
                }
            }
        }
    }

    #[test]
    fn dropping_owned_child_reaps_it() {
        let child = fixture("sleep 30")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0)
            .spawn()
            .unwrap();
        let pid = child.id();
        drop(OwnedChild::new(child));
        let mut status = 0;
        // SAFETY: waitpid targets only the disposable fixture child; Drop must
        // already have reaped it, so ECHILD is the expected result.
        assert_eq!(
            unsafe { libc::waitpid(pid as i32, &mut status, libc::WNOHANG) },
            -1
        );
        assert_eq!(
            io::Error::last_os_error().raw_os_error(),
            Some(libc::ECHILD)
        );
    }

    #[test]
    fn spawn_failure_did_not_start_execution() {
        let result = run(
            &mut Command::new("/this/fixture/binary/does/not/exist"),
            &Arc::new(Budget::default()),
            &AtomicBool::new(false),
            limits(),
        );
        assert!(matches!(result, Err(Error::Spawn(_))));
        assert_eq!(Error::NotStarted(libc::ECANCELED).stage(), "admission");
        assert_eq!(Error::NotStarted(libc::ECANCELED).errno(), libc::EBUSY);
        assert_eq!(Error::Timeout.stage(), "outcome_unknown");
        assert_eq!(Error::Timeout.errno(), libc::EIO);
    }
}
