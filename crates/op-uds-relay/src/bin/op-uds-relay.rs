//! Byte relay for the NIC-less container transport.
//!
//! An unprivileged Incus container with no `nic` device and no `proxy` device
//! has exactly one channel to the host: a bind-mounted directory. This binary
//! turns that directory into the transport.
//!
//! Two directions, deliberately in one binary so both ends of a hop are the
//! same code:
//!
//! * `--unix-to-tcp <sock>=<host>:<port>` — runs **inside** the container.
//!   Binds `<sock>` on the shared mount, forwards to a loopback listener that
//!   only exists in the container's netns.
//! * `--tcp-to-unix <bind>:<port>=<sock>` — runs **on the host**. Binds a
//!   public/mesh address and forwards into the container's socket.
//!
//! Fail-closed: any listener that cannot bind aborts the process rather than
//! serving a partial port set. SIGTERM/SIGINT stop listener workers and remove
//! only socket inodes bound by this process. Crash/SIGKILL leftovers require
//! explicit lifecycle-owner cleanup; startup never removes an existing path.
//! Socket directories must remain under the trusted service lifecycle owner's
//! control. The device/inode guard detects replacement before cleanup; it is
//! not an atomic compare-and-unlink against an adversary who can rename paths.
//! `--max-connections` bounds dialing and active connections across all listeners
//! (default 64). Existing connections have no added idle timeout.

use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

const DEFAULT_MAX_CONNECTIONS: usize = 64;
const ACCEPT_INTERVAL: Duration = Duration::from_millis(50);
static SHUTDOWN_REQUESTED: AtomicBool = AtomicBool::new(false);

// POSIX signal ABI on the supported Linux GNU/musl targets. Keep this small
// executable dependency-free; the handler performs only a lock-free store.
extern "C" {
    fn signal(number: std::ffi::c_int, handler: usize) -> usize;
}

extern "C" fn request_shutdown(_: std::ffi::c_int) {
    SHUTDOWN_REQUESTED.store(true, Ordering::Relaxed);
}

fn install_shutdown_handlers() -> std::io::Result<()> {
    for number in [2, 15] { // SIGINT, SIGTERM
        // SAFETY: the process-lifetime handler has the C signal ABI and uses
        // no allocation, locks, I/O or non-signal-safe library calls.
        if unsafe { signal(number, request_shutdown as *const () as usize) } == usize::MAX {
            return Err(std::io::Error::last_os_error());
        }
    }
    Ok(())
}

struct ListenerWorker {
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}

impl ListenerWorker {
    fn spawn(serve: impl FnOnce(Arc<AtomicBool>) + Send + 'static) -> std::io::Result<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let thread = thread::Builder::new().spawn(move || serve(worker_stop))?;
        Ok(Self { stop, thread: Some(thread) })
    }

    fn finished(&self) -> bool {
        self.thread.as_ref().is_some_and(thread::JoinHandle::is_finished)
    }
}

impl Drop for ListenerWorker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(worker) = self.thread.take() {
            if worker.join().is_err() {
                eprintln!("op-uds-relay: listener worker panicked during shutdown");
            }
        }
    }
}

fn stop_listeners(listeners: Vec<ListenerWorker>) {
    for worker in &listeners {
        worker.stop.store(true, Ordering::Relaxed);
    }
    drop(listeners);
}

struct UnixEndpoint {
    listener: Option<UnixListener>,
    path: PathBuf,
    device: u64,
    inode: u64,
}

impl UnixEndpoint {
    fn bind(path: PathBuf) -> std::io::Result<Self> {
        let listener = UnixListener::bind(&path)?;
        let metadata = std::fs::symlink_metadata(&path)?;
        if !metadata.file_type().is_socket() {
            return Err(std::io::Error::other("bound socket pathname changed"));
        }
        let endpoint = Self {
            listener: Some(listener), path, device: metadata.dev(), inode: metadata.ino(),
        };
        endpoint.listener.as_ref().unwrap().set_nonblocking(true)?;
        Ok(endpoint)
    }
}

impl Drop for UnixEndpoint {
    fn drop(&mut self) {
        // Close the listening descriptor before retiring its owned name.
        // The service owner must control parent-directory mutations during
        // bind/chmod/cleanup; POSIX unlink has no inode-conditional variant.
        drop(self.listener.take());
        match std::fs::symlink_metadata(&self.path) {
            Ok(metadata) if metadata.file_type().is_socket()
                && (metadata.dev(), metadata.ino()) == (self.device, self.inode) => {
                if let Err(error) = std::fs::remove_file(&self.path) {
                    eprintln!("op-uds-relay: cannot remove owned socket {}: {error}", self.path.display());
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            _ => eprintln!("op-uds-relay: socket {} changed; refusing cleanup", self.path.display()),
        }
    }
}

struct Config {
    specs: Vec<Spec>,
    max_connections: usize,
}

struct ConnectionBudget {
    maximum: usize,
    active: AtomicUsize,
}

impl ConnectionBudget {
    fn new(maximum: usize) -> Arc<Self> {
        Arc::new(Self { maximum, active: AtomicUsize::new(0) })
    }

    fn try_acquire(self: &Arc<Self>) -> Option<ConnectionPermit> {
        self.active.fetch_update(Ordering::AcqRel, Ordering::Relaxed, |active| {
            if active < self.maximum { Some(active + 1) } else { None }
        }).ok()?;
        Some(ConnectionPermit(Arc::clone(self)))
    }
}

struct ConnectionPermit(Arc<ConnectionBudget>);

impl Drop for ConnectionPermit {
    fn drop(&mut self) {
        self.0.active.fetch_sub(1, Ordering::AcqRel);
    }
}

/// One relay hop. `UnixToTcp` is the container end, `TcpToUnix` the host end.
enum Spec {
    UnixToTcp {
        sock: PathBuf,
        host: String,
        port: u16,
    },
    TcpToUnix {
        bind: String,
        port: u16,
        sock: PathBuf,
    },
}

/// `UnixStream` and `TcpStream` share no cloneable trait, and the relay needs
/// an owned handle per direction, so unify them here instead of duplicating the
/// copy loop per combination.
enum Stream {
    Tcp(TcpStream),
    Unix(UnixStream),
}

impl Stream {
    fn try_clone(&self) -> std::io::Result<Stream> {
        match self {
            Stream::Tcp(s) => s.try_clone().map(Stream::Tcp),
            Stream::Unix(s) => s.try_clone().map(Stream::Unix),
        }
    }

    fn shutdown(&self, how: Shutdown) {
        let _ = match self {
            Stream::Tcp(s) => s.shutdown(how),
            Stream::Unix(s) => s.shutdown(how),
        };
    }
}

impl Read for Stream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            Stream::Tcp(s) => s.read(buf),
            Stream::Unix(s) => s.read(buf),
        }
    }
}

impl Write for Stream {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self {
            Stream::Tcp(s) => s.write(buf),
            Stream::Unix(s) => s.write(buf),
        }
    }
    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            Stream::Tcp(s) => s.flush(),
            Stream::Unix(s) => s.flush(),
        }
    }
}

/// Propagate clean EOF as a write half-close, allowing the reverse response to
/// drain. Fatal I/O errors abort both directions; interrupts retry the read.
fn pump(mut from: Stream, mut to: Stream) {
    let mut buf = [0u8; 65536];
    loop {
        match from.read(&mut buf) {
            Ok(0) => {
                to.shutdown(Shutdown::Write);
                return;
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => break,
            Ok(n) => {
                if to.write_all(&buf[..n]).is_err() {
                    break;
                }
            }
        }
    }
    from.shutdown(Shutdown::Both);
    to.shutdown(Shutdown::Both);
}

/// Wire an accepted client to a freshly dialled backend.
fn splice(client: Stream, backend: Stream, label: &str, permit: ConnectionPermit) {
    let (Ok(client_rx), Ok(backend_rx)) = (client.try_clone(), backend.try_clone()) else {
        eprintln!("op-uds-relay: {label}: cannot clone stream handles");
        return;
    };
    // The connector becomes one pump; only one additional thread is needed.
    // The budget remains charged until both directions have finished.
    let permit = Arc::new(permit);
    let reverse_permit = Arc::clone(&permit);
    match thread::Builder::new().spawn(move || {
        let _permit = reverse_permit;
        pump(backend_rx, client);
    }) {
        Ok(_) => {
            let _permit = permit;
            pump(client_rx, backend);
        }
        Err(error) => {
            client_rx.shutdown(Shutdown::Both);
            backend.shutdown(Shutdown::Both);
            eprintln!("op-uds-relay: {label}: cannot start reverse pump: {error}");
        }
    }
}

fn serve_unix_to_tcp(
    sock: PathBuf,
    host: String,
    port: u16,
    budget: Arc<ConnectionBudget>,
) -> std::io::Result<ListenerWorker> {
    if let Some(parent) = sock.parent() {
        std::fs::create_dir_all(parent)?;
    }
    // bind atomically refuses an existing endpoint, including a live listener
    // or symlink. A stale socket requires explicit cleanup by its lifecycle
    // owner; a second relay must never unlink and replace the first one's path.
    let endpoint = UnixEndpoint::bind(sock.clone())?;
    // The host side connects as a different uid than the container's root, so
    // the socket has to be world-connectable to be reachable across the idmap.
    std::fs::set_permissions(&sock, std::fs::Permissions::from_mode(0o666))?;
    let label = format!("{}->{}:{}", sock.display(), host, port);
    println!("op-uds-relay: unix-listen {label}");
    ListenerWorker::spawn(move |stop| {
        while !stop.load(Ordering::Relaxed) {
            let client = match endpoint.listener.as_ref().unwrap().accept() {
                Ok((client, _)) => client,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::park_timeout(ACCEPT_INTERVAL);
                    continue;
                }
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(error) => {
                    eprintln!("op-uds-relay: {label}: accept failed: {error}");
                    break;
                }
            };
            let Some(permit) = budget.try_acquire() else {
                // Drop excess connections without injecting protocol bytes.
                continue;
            };
            let (host, label) = (host.clone(), label.clone());
            if let Err(error) = thread::Builder::new().spawn(move || match TcpStream::connect((host.as_str(), port)) {
                Ok(backend) => splice(Stream::Unix(client), Stream::Tcp(backend), &label, permit),
                Err(e) => eprintln!("op-uds-relay: {label}: backend connect failed: {e}"),
            }) {
                eprintln!("op-uds-relay: cannot start connection worker: {error}");
            }
        }
        drop(endpoint);
    })
}

fn serve_tcp_to_unix(
    bind: String,
    port: u16,
    sock: PathBuf,
    budget: Arc<ConnectionBudget>,
) -> std::io::Result<ListenerWorker> {
    let listener = TcpListener::bind((bind.as_str(), port))?;
    listener.set_nonblocking(true)?;
    let label = format!("{}:{}->{}", bind, port, sock.display());
    println!("op-uds-relay: tcp-listen {label}");
    ListenerWorker::spawn(move |stop| {
        while !stop.load(Ordering::Relaxed) {
            let client = match listener.accept() {
                Ok((client, _)) => client,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::park_timeout(ACCEPT_INTERVAL);
                    continue;
                }
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(error) => {
                    eprintln!("op-uds-relay: {label}: accept failed: {error}");
                    break;
                }
            };
            let Some(permit) = budget.try_acquire() else {
                continue;
            };
            let (sock, label) = (sock.clone(), label.clone());
            if let Err(error) = thread::Builder::new().spawn(move || match UnixStream::connect(&sock) {
                Ok(backend) => splice(Stream::Tcp(client), Stream::Unix(backend), &label, permit),
                Err(e) => eprintln!("op-uds-relay: {label}: backend connect failed: {e}"),
            }) {
                eprintln!("op-uds-relay: cannot start connection worker: {error}");
            }
        }
    })
}

/// `<sock>=<host>:<port>` — the container end.
fn parse_unix_to_tcp(raw: &str) -> Result<Spec, String> {
    let (sock, target) = raw
        .split_once('=')
        .ok_or_else(|| format!("--unix-to-tcp expects <sock>=<host>:<port>, got {raw:?}"))?;
    let (host, port) = target
        .rsplit_once(':')
        .ok_or_else(|| format!("--unix-to-tcp target expects <host>:<port>, got {target:?}"))?;
    Ok(Spec::UnixToTcp {
        sock: PathBuf::from(sock),
        host: host.to_string(),
        port: port.parse().map_err(|_| format!("bad port {port:?}"))?,
    })
}

/// `<bind>:<port>=<sock>` — the host end.
fn parse_tcp_to_unix(raw: &str) -> Result<Spec, String> {
    let (source, sock) = raw
        .split_once('=')
        .ok_or_else(|| format!("--tcp-to-unix expects <bind>:<port>=<sock>, got {raw:?}"))?;
    let (bind, port) = source
        .rsplit_once(':')
        .ok_or_else(|| format!("--tcp-to-unix source expects <bind>:<port>, got {source:?}"))?;
    Ok(Spec::TcpToUnix {
        bind: bind.to_string(),
        port: port.parse().map_err(|_| format!("bad port {port:?}"))?,
        sock: PathBuf::from(sock),
    })
}

fn parse_args(args: &[String]) -> Result<Config, String> {
    let mut specs = Vec::new();
    let mut max_connections = DEFAULT_MAX_CONNECTIONS;
    let mut i = 0;
    while i < args.len() {
        let flag = args[i].as_str();
        let value = args
            .get(i + 1)
            .ok_or_else(|| format!("{flag} requires a value"))?;
        match flag {
            "--unix-to-tcp" => specs.push(parse_unix_to_tcp(value)?),
            "--tcp-to-unix" => specs.push(parse_tcp_to_unix(value)?),
            "--max-connections" => {
                max_connections = value.parse::<usize>()
                    .ok().filter(|value| *value > 0)
                    .ok_or_else(|| "--max-connections requires a positive integer".to_string())?;
            }
            other => return Err(format!("unknown argument {other:?}")),
        }
        i += 2;
    }
    if specs.is_empty() {
        return Err("no relay specs given".to_string());
    }
    Ok(Config { specs, max_connections })
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let config = match parse_args(&args) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("op-uds-relay: {e}");
            eprintln!(
                "usage: op-uds-relay [--unix-to-tcp <sock>=<host>:<port>] \
                 [--tcp-to-unix <bind>:<port>=<sock>] ... [--max-connections <count>]"
            );
            return ExitCode::FAILURE;
        }
    };

    if let Err(error) = install_shutdown_handlers() {
        eprintln!("op-uds-relay: cannot install shutdown handlers: {error}");
        return ExitCode::FAILURE;
    }
    let budget = ConnectionBudget::new(config.max_connections);
    let mut listeners = Vec::new();
    for spec in config.specs {
        let bound = match spec {
            Spec::UnixToTcp { sock, host, port } => serve_unix_to_tcp(sock, host, port, Arc::clone(&budget)),
            Spec::TcpToUnix { bind, port, sock } => serve_tcp_to_unix(bind, port, sock, Arc::clone(&budget)),
        };
        // Serving a partial port set silently is worse than not starting: mail
        // would look healthy while one protocol black-holed.
        match bound {
            Ok(listener) => listeners.push(listener),
            Err(error) => {
                eprintln!("op-uds-relay: bind failed: {error}");
                stop_listeners(listeners);
                return ExitCode::FAILURE;
            }
        }
        if SHUTDOWN_REQUESTED.load(Ordering::Relaxed) {
            stop_listeners(listeners);
            return ExitCode::SUCCESS;
        }
    }

    println!("op-uds-relay: all listeners bound; max connections {}", config.max_connections);
    while !SHUTDOWN_REQUESTED.load(Ordering::Relaxed) {
        if listeners.iter().any(ListenerWorker::finished) {
            eprintln!("op-uds-relay: listener stopped unexpectedly");
            stop_listeners(listeners);
            return ExitCode::FAILURE;
        }
        thread::park_timeout(ACCEPT_INTERVAL);
    }
    // Connection workers terminate with this process, as under the previous
    // supervisor stop behavior. Normal shutdown joins listeners first so their
    // Unix socket cleanup completes before the supervisor can restart us.
    stop_listeners(listeners);
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec_names(specs: &[Spec]) -> Vec<String> {
        specs
            .iter()
            .map(|s| match s {
                Spec::UnixToTcp { sock, host, port } => {
                    format!("u:{}={}:{}", sock.display(), host, port)
                }
                Spec::TcpToUnix { bind, port, sock } => {
                    format!("t:{}:{}={}", bind, port, sock.display())
                }
            })
            .collect()
    }

    #[test]
    fn parses_both_directions() {
        let args: Vec<String> = [
            "--unix-to-tcp",
            "/run/gb/mail/smtp.sock=127.0.0.1:25",
            "--tcp-to-unix",
            "188.68.58.237:25=/run/gb/mail/smtp.sock",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let specs = parse_args(&args).expect("parse").specs;
        assert_eq!(
            spec_names(&specs),
            vec![
                "u:/run/gb/mail/smtp.sock=127.0.0.1:25",
                "t:188.68.58.237:25=/run/gb/mail/smtp.sock",
            ]
        );
    }

    /// IPv6 binds contain colons, so the port must be split from the right.
    #[test]
    fn splits_port_from_the_right() {
        let specs = parse_args(&[
            "--tcp-to-unix".to_string(),
            "::1:993=/run/gb/mail/imaps.sock".to_string(),
        ])
        .expect("parse").specs;
        assert_eq!(
            spec_names(&specs),
            vec!["t:::1:993=/run/gb/mail/imaps.sock"]
        );
    }

    #[test]
    fn rejects_empty_and_malformed() {
        assert!(parse_args(&[]).is_err());
        assert!(parse_args(&["--unix-to-tcp".to_string()]).is_err());
        assert!(parse_args(&["--unix-to-tcp".to_string(), "/no-equals-sign".to_string()]).is_err());
        assert!(parse_args(&["--bogus".to_string(), "x".to_string()]).is_err());
    }

    #[test]
    fn connection_limit_is_positive_and_shared_until_last_owner_drops() {
        let config = parse_args(&[
            "--tcp-to-unix".into(), "127.0.0.1:25=/tmp/mail.sock".into(),
            "--max-connections".into(), "2".into(),
        ]).expect("parse");
        assert_eq!(config.max_connections, 2);
        assert!(parse_args(&[
            "--tcp-to-unix".into(), "127.0.0.1:25=/tmp/mail.sock".into(),
            "--max-connections".into(), "0".into(),
        ]).is_err());
        let budget = ConnectionBudget::new(1);
        let permit = Arc::new(budget.try_acquire().expect("first connection"));
        let reverse = Arc::clone(&permit);
        assert!(budget.try_acquire().is_none());
        drop(permit);
        assert!(budget.try_acquire().is_none());
        drop(reverse);
        assert!(budget.try_acquire().is_some());
    }

    #[test]
    fn clean_request_eof_allows_reverse_response() {
        let (mut client, relay_client) = UnixStream::pair().expect("client pair");
        let (relay_backend, mut backend) = UnixStream::pair().expect("backend pair");
        let timeout = Some(std::time::Duration::from_secs(2));
        client.set_read_timeout(timeout).expect("client timeout");
        backend.set_read_timeout(timeout).expect("backend timeout");
        let budget = ConnectionBudget::new(1);
        let permit = budget.try_acquire().expect("connection permit");
        let relay = thread::spawn(move || {
            splice(Stream::Unix(relay_client), Stream::Unix(relay_backend), "fixture", permit);
        });
        client.write_all(b"request").expect("request");
        client.shutdown(Shutdown::Write).expect("request EOF");
        let mut request = Vec::new();
        backend.read_to_end(&mut request).expect("drain request");
        assert_eq!(request, b"request");
        backend.write_all(b"response").expect("response after EOF");
        backend.shutdown(Shutdown::Write).expect("response EOF");
        let mut response = Vec::new();
        client.read_to_end(&mut response).expect("drain response");
        assert_eq!(response, b"response");
        relay.join().expect("relay");
    }

    #[test]
    fn existing_socket_path_is_not_replaced() {
        use std::os::unix::fs::MetadataExt;
        let dir = std::env::temp_dir().join(format!("op-uds-relay-owner-test-{}", std::process::id()));
        std::fs::create_dir(&dir).expect("exclusive fixture directory");
        let sock = dir.join("owner.sock");
        let owner = UnixListener::bind(&sock).expect("existing owner");
        let before = std::fs::symlink_metadata(&sock).expect("original socket");
        assert!(serve_unix_to_tcp(
            sock.clone(), "127.0.0.1".into(), 1, ConnectionBudget::new(1),
        ).is_err());
        let after = std::fs::symlink_metadata(&sock).expect("retained socket");
        assert_eq!((before.dev(), before.ino()), (after.dev(), after.ino()));
        drop(owner);
        std::fs::remove_file(sock).expect("fixture cleanup");
        std::fs::remove_dir(dir).expect("fixture cleanup");
    }

    #[test]
    fn stopped_listener_releases_its_owned_path_for_restart() {
        let dir = std::env::temp_dir().join(format!("op-uds-relay-restart-test-{}", std::process::id()));
        std::fs::create_dir(&dir).expect("exclusive fixture directory");
        let sock = dir.join("owned.sock");
        for _ in 0..2 {
            let worker = serve_unix_to_tcp(
                sock.clone(), "127.0.0.1".into(), 1, ConnectionBudget::new(1),
            ).expect("bind owned listener");
            assert!(sock.exists());
            stop_listeners(vec![worker]);
            assert!(!sock.exists());
        }
        std::fs::remove_dir(dir).expect("fixture cleanup");
    }

    #[test]
    fn shutdown_retains_a_replaced_socket_path() {
        let dir = std::env::temp_dir().join(format!("op-uds-relay-replaced-test-{}", std::process::id()));
        std::fs::create_dir(&dir).expect("exclusive fixture directory");
        let sock = dir.join("owned.sock");
        let worker = serve_unix_to_tcp(
            sock.clone(), "127.0.0.1".into(), 1, ConnectionBudget::new(1),
        ).expect("bind owned listener");
        let old = dir.join("old.sock");
        std::fs::rename(&sock, &old).expect("retain old inode");
        let replacement = UnixListener::bind(&sock).expect("replacement listener");
        let before = std::fs::symlink_metadata(&sock).unwrap();
        stop_listeners(vec![worker]);
        let after = std::fs::symlink_metadata(&sock).expect("replacement retained");
        assert_eq!((before.dev(), before.ino()), (after.dev(), after.ino()));
        drop(replacement);
        std::fs::remove_file(sock).unwrap();
        std::fs::remove_file(old).unwrap();
        std::fs::remove_dir(dir).unwrap();
    }

    #[test]
    fn partial_startup_cleanup_keeps_the_preexisting_listener() {
        let dir = std::env::temp_dir().join(format!("op-uds-relay-startup-test-{}", std::process::id()));
        std::fs::create_dir(&dir).expect("exclusive fixture directory");
        let owned = dir.join("owned.sock");
        let occupied = dir.join("occupied.sock");
        let other = UnixListener::bind(&occupied).expect("preexisting listener");
        let worker = serve_unix_to_tcp(
            owned.clone(), "127.0.0.1".into(), 1, ConnectionBudget::new(1),
        ).unwrap();
        assert!(serve_unix_to_tcp(
            occupied.clone(), "127.0.0.1".into(), 1, ConnectionBudget::new(1),
        ).is_err());
        stop_listeners(vec![worker]);
        assert!(!owned.exists());
        assert!(occupied.exists());
        drop(other);
        std::fs::remove_file(occupied).unwrap();
        std::fs::remove_dir(dir).unwrap();
    }

    /// End-to-end through both hops: tcp -> unix -> tcp, the exact shape mail
    /// uses (host public port -> shared socket -> container loopback).
    #[test]
    fn relays_bytes_across_both_hops() {
        let dir = std::env::temp_dir().join(format!("op-uds-relay-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("tmpdir");
        let sock = dir.join("hop.sock");

        // Stand-in for the in-container daemon: echoes one line back.
        let backend = TcpListener::bind("127.0.0.1:0").expect("backend bind");
        let backend_port = backend.local_addr().expect("addr").port();
        thread::spawn(move || {
            for mut client in backend.incoming().flatten() {
                let mut buf = [0u8; 64];
                if let Ok(n) = client.read(&mut buf) {
                    let _ = client.write_all(&buf[..n]);
                }
            }
        });

        let budget = ConnectionBudget::new(DEFAULT_MAX_CONNECTIONS);
        let unix_hop = serve_unix_to_tcp(sock.clone(), "127.0.0.1".to_string(), backend_port, Arc::clone(&budget)).expect("unix hop");
        let front = TcpListener::bind("127.0.0.1:0").expect("front probe");
        let front_port = front.local_addr().expect("addr").port();
        drop(front);
        let tcp_hop = serve_tcp_to_unix("127.0.0.1".to_string(), front_port, sock.clone(), budget).expect("tcp hop");

        let mut client =
            TcpStream::connect(("127.0.0.1", front_port)).expect("connect through relay");
        client.write_all(b"ping").expect("write");
        let mut got = [0u8; 4];
        client.read_exact(&mut got).expect("read echo");
        assert_eq!(&got, b"ping");

        stop_listeners(vec![unix_hop, tcp_hop]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
