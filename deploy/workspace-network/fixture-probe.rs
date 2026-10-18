// Standalone fixture utility, compiled with rustc; no project/Cargo dependencies.
use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddrV4, TcpListener, TcpStream, UdpSocket};
use std::time::Duration;

fn connect(port: u16) -> TcpStream {
    let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
    stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    stream.set_write_timeout(Some(Duration::from_secs(5))).unwrap();
    stream.write_all(&[5, 1, 0]).unwrap();
    let mut reply = [0; 2];
    stream.read_exact(&mut reply).unwrap();
    assert_eq!(reply, [5, 0]);
    stream
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args[1] == "echo" {
        let tcp = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let udp = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        std::fs::write(&args[2], format!("{{\"tcp\":{},\"udp\":{}}}", tcp.local_addr().unwrap().port(), udp.local_addr().unwrap().port())).unwrap();
        std::thread::spawn(move || {
            for connection in tcp.incoming() {
                let mut stream = connection.unwrap();
                std::thread::spawn(move || {
                    let mut data = [0; 4096];
                    while let Ok(n) = stream.read(&mut data) {
                        if n == 0 { break; }
                        stream.write_all(&data[..n]).unwrap();
                    }
                });
            }
        });
        let mut data = [0; 4096];
        loop { let (n, peer) = udp.recv_from(&mut data).unwrap(); udp.send_to(&data[..n], peer).unwrap(); }
    }
    let socks: u16 = args[2].parse().unwrap();
    let tcp: u16 = args[3].parse().unwrap();
    let udp: u16 = args[4].parse().unwrap();
    // The echo listener is in the host namespace, not this loopback-only guest.
    assert!(TcpStream::connect_timeout(&SocketAddrV4::new(Ipv4Addr::LOCALHOST, tcp).into(), Duration::from_secs(1)).is_err());
    let payload = b"workspace TCP UDP over UDS fixture";
    let mut stream = connect(socks);
    let [hi, lo] = tcp.to_be_bytes();
    stream.write_all(&[5, 1, 0, 1, 127, 0, 0, 1, hi, lo]).unwrap();
    let mut reply = [0; 10];
    stream.read_exact(&mut reply).unwrap();
    assert_eq!(&reply[..4], &[5, 0, 0, 1]);
    stream.write_all(payload).unwrap();
    let mut echoed = vec![0; payload.len()];
    stream.read_exact(&mut echoed).unwrap();
    assert_eq!(echoed, payload);
    println!("PASS TCP: SOCKS CONNECT -> VLESS -> Unix socket -> host echo");
    let udp_socket = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    udp_socket.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let [hi, lo] = udp_socket.local_addr().unwrap().port().to_be_bytes();
    let mut control = connect(socks);
    control.write_all(&[5, 3, 0, 1, 127, 0, 0, 1, hi, lo]).unwrap();
    control.read_exact(&mut reply).unwrap();
    assert_eq!(&reply[..4], &[5, 0, 0, 1]);
    let relay_port = u16::from_be_bytes([reply[8], reply[9]]);
    let [hi, lo] = udp.to_be_bytes();
    let mut datagram = vec![0, 0, 0, 1, 127, 0, 0, 1, hi, lo];
    datagram.extend_from_slice(payload);
    udp_socket.send_to(&datagram, (Ipv4Addr::LOCALHOST, relay_port)).unwrap();
    let mut response = [0; 4096];
    let (n, _) = udp_socket.recv_from(&mut response).unwrap();
    assert_eq!(&response[..10], &datagram[..10]);
    assert_eq!(&response[10..n], payload);
    println!("PASS UDP: SOCKS UDP ASSOCIATE -> VLESS UoT -> Unix socket -> host echo");
}
