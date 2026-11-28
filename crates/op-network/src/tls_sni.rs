//! Read the TLS ClientHello `server_name` from an accepted socket without
//! consuming a byte. The same bytes then reach the identity's TLS door.

use std::os::fd::AsRawFd;
use std::time::Duration;

use anyhow::{Context, Result, bail, ensure};
use tokio::io::Interest;
use tokio::net::TcpStream;

/// Largest ClientHello record accepted (one TLS record).
pub const MAX_CLIENT_HELLO: usize = 16 * 1024;
const RECORD_HEADER: usize = 5;
const EXT_SERVER_NAME: u16 = 0x0000;

#[derive(Debug, PartialEq, Eq)]
pub enum ClientHello {
    /// More bytes are needed: the full first record is this long.
    Incomplete(usize),
    /// The host_name from the server_name extension.
    ServerName(String),
}

struct Cursor<'a>(&'a [u8]);

impl<'a> Cursor<'a> {
    fn take(&mut self, count: usize) -> Result<&'a [u8]> {
        ensure!(self.0.len() >= count, "ClientHello is truncated");
        let (head, tail) = self.0.split_at(count);
        self.0 = tail;
        Ok(head)
    }
    fn u8(&mut self) -> Result<usize> { Ok(self.take(1)?[0] as usize) }
    fn u16(&mut self) -> Result<usize> {
        let bytes = self.take(2)?;
        Ok(u16::from_be_bytes([bytes[0], bytes[1]]) as usize)
    }
    fn u24(&mut self) -> Result<usize> {
        let bytes = self.take(3)?;
        Ok(((bytes[0] as usize) << 16) | ((bytes[1] as usize) << 8) | bytes[2] as usize)
    }
    fn vec8(&mut self) -> Result<&'a [u8]> { let length = self.u8()?; self.take(length) }
    fn vec16(&mut self) -> Result<&'a [u8]> { let length = self.u16()?; self.take(length) }
}

/// Parse the first TLS record. Refuses anything that is not a single-record
/// ClientHello with one visible host_name. All other extensions are left for
/// the destination TLS server, including the ECH extension that browsers
/// may send as GREASE.
pub fn parse_client_hello(buffer: &[u8]) -> Result<ClientHello> {
    if buffer.len() < RECORD_HEADER {
        return Ok(ClientHello::Incomplete(RECORD_HEADER));
    }
    ensure!(buffer[0] == 0x16, "not a TLS handshake record");
    ensure!(buffer[1] == 0x03, "not a TLS record version");
    let record_len = u16::from_be_bytes([buffer[3], buffer[4]]) as usize;
    let total = RECORD_HEADER + record_len;
    ensure!(record_len > 0 && total <= MAX_CLIENT_HELLO, "ClientHello record size out of bounds");
    if buffer.len() < total {
        return Ok(ClientHello::Incomplete(total));
    }
    let mut record = Cursor(&buffer[RECORD_HEADER..total]);
    ensure!(record.u8()? == 1, "first handshake message is not ClientHello");
    let hello_len = record.u24()?;
    ensure!(hello_len <= record.0.len(), "ClientHello spans more than one record");
    let mut hello = Cursor(record.take(hello_len)?);
    hello.take(2 + 32)?; // legacy_version, random
    hello.vec8()?; // legacy_session_id
    hello.vec16()?; // cipher_suites
    hello.vec8()?; // legacy_compression_methods
    let mut extensions = Cursor(hello.vec16()?);
    let mut server_name = None;
    while !extensions.0.is_empty() {
        let kind = extensions.u16()? as u16;
        let data = extensions.vec16()?;
        match kind {
            EXT_SERVER_NAME => {
                ensure!(server_name.is_none(), "duplicate server_name extension");
                let mut list = Cursor(Cursor(data).vec16()?);
                let name_type = list.u8()?;
                let name = list.vec16()?;
                ensure!(name_type == 0 && list.0.is_empty(), "server_name is not one host_name");
                ensure!(!name.is_empty() && name.len() <= 253 && name.is_ascii(), "server_name is malformed");
                server_name = Some(String::from_utf8(name.to_vec())?);
            }
            _ => {}
        }
    }
    server_name.map(ClientHello::ServerName).context("ClientHello has no server_name")
}

fn set_rcvlowat(stream: &TcpStream, bytes: usize) -> Result<()> {
    let value = libc::c_int::try_from(bytes)?;
    // SAFETY: the descriptor is live for the call and `value` is a valid c_int.
    let result = unsafe {
        libc::setsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_RCVLOWAT,
            (&value as *const libc::c_int).cast(),
            std::mem::size_of::<libc::c_int>() as libc::socklen_t,
        )
    };
    if result != 0 {
        return Err(std::io::Error::last_os_error()).context("set SO_RCVLOWAT");
    }
    Ok(())
}

fn peek_at_least(stream: &TcpStream, buffer: &mut [u8], wanted: usize) -> std::io::Result<usize> {
    // SAFETY: buffer is writable for its length; MSG_PEEK leaves the bytes queued.
    let read = unsafe {
        libc::recv(
            stream.as_raw_fd(),
            buffer.as_mut_ptr().cast(),
            buffer.len(),
            libc::MSG_PEEK | libc::MSG_DONTWAIT,
        )
    };
    if read < 0 {
        return Err(std::io::Error::last_os_error());
    }
    let read = read as usize;
    if read == 0 {
        return Err(std::io::Error::from(std::io::ErrorKind::UnexpectedEof));
    }
    if read < wanted {
        // Clears tokio readiness; with SO_RCVLOWAT the kernel signals again
        // only once `wanted` bytes are queued. No byte is consumed.
        return Err(std::io::Error::from(std::io::ErrorKind::WouldBlock));
    }
    Ok(read)
}

/// The ClientHello `server_name` of an accepted socket, read with MSG_PEEK
/// within `deadline`. The socket's receive low-water mark is restored to 1
/// before returning, so the receiver sees an ordinary socket.
pub async fn peek_server_name(stream: &TcpStream, deadline: Duration) -> Result<String> {
    let result = tokio::time::timeout(deadline, async {
        let mut buffer = vec![0u8; MAX_CLIENT_HELLO];
        let mut wanted = RECORD_HEADER;
        loop {
            set_rcvlowat(stream, wanted)?;
            stream.readable().await?;
            let read = match stream.try_io(Interest::READABLE, || peek_at_least(stream, &mut buffer, wanted)) {
                Ok(read) => read,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => continue,
                Err(error) => return Err(error).context("peek ClientHello"),
            };
            match parse_client_hello(&buffer[..read])? {
                ClientHello::ServerName(name) => return Ok(name),
                ClientHello::Incomplete(total) => wanted = total,
            }
        }
    })
    .await;
    let restore = set_rcvlowat(stream, 1);
    let name = result.context("ClientHello deadline passed")??;
    restore?;
    Ok(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hello(extensions: &[u8]) -> Vec<u8> {
        let mut body = vec![0x03, 0x03];
        body.extend([0u8; 32]);
        body.push(0); // session id
        body.extend([0x00, 0x02, 0x13, 0x01]); // one cipher suite
        body.extend([0x01, 0x00]); // null compression
        body.extend((extensions.len() as u16).to_be_bytes());
        body.extend(extensions);
        let mut handshake = vec![0x01];
        handshake.extend(&(body.len() as u32).to_be_bytes()[1..]);
        handshake.extend(body);
        let mut record = vec![0x16, 0x03, 0x01];
        record.extend((handshake.len() as u16).to_be_bytes());
        record.extend(handshake);
        record
    }

    fn sni(name: &str) -> Vec<u8> {
        let mut entry = vec![0x00];
        entry.extend((name.len() as u16).to_be_bytes());
        entry.extend(name.as_bytes());
        let mut list = (entry.len() as u16).to_be_bytes().to_vec();
        list.extend(entry);
        let mut extension = vec![0x00, 0x00];
        extension.extend((list.len() as u16).to_be_bytes());
        extension.extend(list);
        extension
    }

    #[test]
    fn reads_the_server_name() {
        let record = hello(&sni("abc.perspective.3tched.com"));
        assert_eq!(
            parse_client_hello(&record).unwrap(),
            ClientHello::ServerName("abc.perspective.3tched.com".into())
        );
    }

    #[test]
    fn short_input_asks_for_the_whole_record() {
        let record = hello(&sni("abc.perspective.3tched.com"));
        assert_eq!(parse_client_hello(&record[..3]).unwrap(), ClientHello::Incomplete(5));
        assert_eq!(parse_client_hello(&record[..20]).unwrap(), ClientHello::Incomplete(record.len()));
    }

    #[test]
    fn reads_visible_server_name_with_ech_grease() {
        let hostname = "abc.perspective.3tched.com";
        let grease = [0xfe, 0x0d, 0x00, 0x01, 0x00];
        let mut extensions = sni(hostname);
        extensions.extend(grease);
        assert_eq!(
            parse_client_hello(&hello(&extensions)).unwrap(),
            ClientHello::ServerName(hostname.into())
        );
        assert!(parse_client_hello(&hello(&grease)).is_err());
    }

    #[test]
    fn refuses_no_sni_and_non_tls() {
        assert!(parse_client_hello(&hello(&[])).is_err());
        assert!(parse_client_hello(b"GET / HTTP/1.1\r\n").is_err());
    }

    #[test]
    fn truncated_or_lying_lengths_are_refused_not_panics() {
        let mut record = hello(&sni("abc.perspective.3tched.com"));
        let last = record.len() - 1;
        record[last - 10] = 0xff;
        let _ = parse_client_hello(&record);
        for cut in 5..record.len() {
            let mut prefix = record[..cut].to_vec();
            let len = (cut - 5) as u16;
            prefix[3..5].copy_from_slice(&len.to_be_bytes());
            let _ = parse_client_hello(&prefix);
        }
    }

    #[tokio::test]
    async fn peek_leaves_every_byte_for_the_receiver() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let record = hello(&sni("abc.perspective.3tched.com"));
        let sent = record.clone();
        let client = tokio::spawn(async move {
            let mut stream = TcpStream::connect(address).await.unwrap();
            let (head, tail) = sent.split_at(7);
            stream.write_all(head).await.unwrap();
            tokio::time::sleep(Duration::from_millis(50)).await;
            stream.write_all(tail).await.unwrap();
            stream
        });
        let (mut accepted, _) = listener.accept().await.unwrap();
        let name = peek_server_name(&accepted, Duration::from_secs(2)).await.unwrap();
        assert_eq!(name, "abc.perspective.3tched.com");
        let mut received = vec![0u8; record.len()];
        accepted.read_exact(&mut received).await.unwrap();
        assert_eq!(received, record);
        drop(client.await.unwrap());
    }
}
