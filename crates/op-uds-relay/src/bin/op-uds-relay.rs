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
//! serving a partial port set, so the supervisor restarts a clean whole.

use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::process::ExitCode;
use std::thread;

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

    /// Unblocks the peer direction's pending `read` so a half-close on one side
    /// tears the whole pair down instead of leaking a thread per connection.
    fn shutdown(&self) {
        let _ = match self {
            Stream::Tcp(s) => s.shutdown(Shutdown::Both),
            Stream::Unix(s) => s.shutdown(Shutdown::Both),
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

/// Pump `from` into `to` until EOF, then hard-close both so the opposite
/// direction's thread also exits.
fn pump(mut from: Stream, mut to: Stream) {
    let mut buf = [0u8; 65536];
    loop {
        match from.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                if to.write_all(&buf[..n]).is_err() {
                    break;
                }
            }
        }
    }
    from.shutdown();
    to.shutdown();
}

/// Wire an accepted client to a freshly dialled backend.
fn splice(client: Stream, backend: Stream, label: &str) {
    let (Ok(client_rx), Ok(backend_rx)) = (client.try_clone(), backend.try_clone()) else {
        eprintln!("op-uds-relay: {label}: cannot clone stream handles");
        return;
    };
    thread::spawn(move || pump(client_rx, backend));
    thread::spawn(move || pump(backend_rx, client));
}

fn serve_unix_to_tcp(sock: PathBuf, host: String, port: u16) -> std::io::Result<()> {
    if let Some(parent) = sock.parent() {
        std::fs::create_dir_all(parent)?;
    }
    // A leftover socket file from a previous run makes bind fail with EADDRINUSE
    // even though nothing is listening.
    if sock.exists() {
        std::fs::remove_file(&sock)?;
    }
    let listener = UnixListener::bind(&sock)?;
    // The host side connects as a different uid than the container's root, so
    // the socket has to be world-connectable to be reachable across the idmap.
    std::fs::set_permissions(&sock, std::fs::Permissions::from_mode(0o666))?;
    let label = format!("{}->{}:{}", sock.display(), host, port);
    println!("op-uds-relay: unix-listen {label}");
    thread::spawn(move || {
        for client in listener.incoming().flatten() {
            let (host, label) = (host.clone(), label.clone());
            thread::spawn(move || match TcpStream::connect((host.as_str(), port)) {
                Ok(backend) => splice(Stream::Unix(client), Stream::Tcp(backend), &label),
                Err(e) => eprintln!("op-uds-relay: {label}: backend connect failed: {e}"),
            });
        }
    });
    Ok(())
}

fn serve_tcp_to_unix(bind: String, port: u16, sock: PathBuf) -> std::io::Result<()> {
    let listener = TcpListener::bind((bind.as_str(), port))?;
    let label = format!("{}:{}->{}", bind, port, sock.display());
    println!("op-uds-relay: tcp-listen {label}");
    thread::spawn(move || {
        for client in listener.incoming().flatten() {
            let (sock, label) = (sock.clone(), label.clone());
            thread::spawn(move || match UnixStream::connect(&sock) {
                Ok(backend) => splice(Stream::Tcp(client), Stream::Unix(backend), &label),
                Err(e) => eprintln!("op-uds-relay: {label}: backend connect failed: {e}"),
            });
        }
    });
    Ok(())
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

fn parse_args(args: &[String]) -> Result<Vec<Spec>, String> {
    let mut specs = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let flag = args[i].as_str();
        let value = args
            .get(i + 1)
            .ok_or_else(|| format!("{flag} requires a value"))?;
        match flag {
            "--unix-to-tcp" => specs.push(parse_unix_to_tcp(value)?),
            "--tcp-to-unix" => specs.push(parse_tcp_to_unix(value)?),
            other => return Err(format!("unknown argument {other:?}")),
        }
        i += 2;
    }
    if specs.is_empty() {
        return Err("no relay specs given".to_string());
    }
    Ok(specs)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let specs = match parse_args(&args) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("op-uds-relay: {e}");
            eprintln!(
                "usage: op-uds-relay [--unix-to-tcp <sock>=<host>:<port>] \
                 [--tcp-to-unix <bind>:<port>=<sock>] ..."
            );
            return ExitCode::FAILURE;
        }
    };

    for spec in specs {
        let bound = match spec {
            Spec::UnixToTcp { sock, host, port } => serve_unix_to_tcp(sock, host, port),
            Spec::TcpToUnix { bind, port, sock } => serve_tcp_to_unix(bind, port, sock),
        };
        // Serving a partial port set silently is worse than not starting: mail
        // would look healthy while one protocol black-holed.
        if let Err(e) = bound {
            eprintln!("op-uds-relay: bind failed: {e}");
            return ExitCode::FAILURE;
        }
    }

    println!("op-uds-relay: all listeners bound");
    // Listener threads are detached; park the main thread to keep the process
    // alive under the supervisor.
    loop {
        thread::park();
    }
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
        let specs = parse_args(&args).expect("parse");
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
        .expect("parse");
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

        serve_unix_to_tcp(sock.clone(), "127.0.0.1".to_string(), backend_port).expect("unix hop");
        let front = TcpListener::bind("127.0.0.1:0").expect("front probe");
        let front_port = front.local_addr().expect("addr").port();
        drop(front);
        serve_tcp_to_unix("127.0.0.1".to_string(), front_port, sock.clone()).expect("tcp hop");

        let mut client =
            TcpStream::connect(("127.0.0.1", front_port)).expect("connect through relay");
        client.write_all(b"ping").expect("write");
        let mut got = [0u8; 4];
        client.read_exact(&mut got).expect("read echo");
        assert_eq!(&got, b"ping");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
