use std::io;
use std::os::fd::FromRawFd;
use std::os::unix::net::UnixDatagram;
use std::process::Command;

fn valid_turn(bytes: &[u8]) -> bool {
    bytes.len() == 36
        && bytes.iter().enumerate().all(|(i, byte)| match i {
            8 | 13 | 18 | 23 => *byte == b'-',
            _ => matches!(byte, b'0'..=b'9' | b'a'..=b'f'),
        })
}

fn main() -> io::Result<()> {
    // systemd supplies its bound datagram socket on stdin.
    let socket = unsafe { UnixDatagram::from_raw_fd(0) };
    let mut bytes = [0; 37];
    loop {
        let length = socket.recv(&mut bytes)?;
        let turn = &bytes[..length];
        if !valid_turn(turn) {
            eprintln!("ignored invalid chat turn identifier");
            continue;
        }
        let turn = std::str::from_utf8(turn).expect("validated ASCII turn identifier");
        match Command::new("/usr/local/bin/chat-route")
            .arg("--process-pending-turn")
            .arg(turn)
            .status()
        {
            Ok(status) if status.success() => eprintln!("completed chat turn {turn}"),
            Ok(status) => eprintln!("chat turn {turn} failed: {status}"),
            Err(error) => eprintln!("could not start chat route for {turn}: {error}"),
        }
    }
}
