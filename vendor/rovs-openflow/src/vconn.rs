//! Virtual connection (VConn) for OpenFlow.

use bytes::{Bytes, BytesMut};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use rovs_transport::{Address, Stream};

use crate::error::OfError;
use crate::flow_monitor::{parse_flow_monitor_reply, FlowMonitorRequest, FlowUpdate};
use crate::multipart::{parse_flow_stats_reply, FlowStatsEntry, FlowStatsRequest};
use crate::packet_in::PacketIn;
use crate::packet_out::PacketOut;
use crate::{Error, Flow, Header, Message, MessageType, Result, Version};

/// An OpenFlow virtual connection.
pub struct VConn {
    stream: Stream,
    version: Version,
    next_xid: u32,
}

impl VConn {
    /// Connect to an OpenFlow switch.
    pub async fn connect(addr: &Address) -> Result<Self> {
        let stream = Stream::connect(addr).await?;

        let mut conn = Self {
            stream,
            version: Version::Of13, // Will be negotiated
            next_xid: 1,
        };

        conn.handshake().await?;
        Ok(conn)
    }

    /// Get the negotiated OpenFlow version.
    pub fn version(&self) -> Version {
        self.version
    }

    /// Perform the OpenFlow handshake.
    async fn handshake(&mut self) -> Result<()> {
        // Send Hello
        let hello = Message::new(
            Version::Of13,
            MessageType::Hello,
            self.next_xid(),
            Bytes::new(),
        );
        self.send_message(&hello).await?;

        // Receive Hello
        let reply = self.recv_message().await?;
        if reply.header.msg_type != MessageType::Hello {
            return Err(Error::InvalidMessage("expected Hello".into()));
        }

        // Use the lower of the two versions
        self.version = std::cmp::min(self.version, reply.header.version);

        Ok(())
    }

    /// Get the next transaction ID.
    fn next_xid(&mut self) -> u32 {
        let xid = self.next_xid;
        self.next_xid = self.next_xid.wrapping_add(1);
        xid
    }

    /// Send a message.
    pub async fn send_message(&mut self, msg: &Message) -> Result<()> {
        let bytes = msg.encode();
        self.stream.write_all(&bytes).await?;
        self.stream.flush().await?;
        Ok(())
    }

    /// Receive a message.
    pub async fn recv_message(&mut self) -> Result<Message> {
        // Read header
        let mut header_buf = [0u8; Header::SIZE];
        self.stream.read_exact(&mut header_buf).await?;
        let header = Header::decode(&mut Bytes::copy_from_slice(&header_buf))?;

        // Read body
        let body_len = header.length as usize - Header::SIZE;
        let mut body = BytesMut::zeroed(body_len);
        if body_len > 0 {
            self.stream.read_exact(&mut body).await?;
        }

        Ok(Message {
            header,
            body: body.freeze(),
        })
    }

    /// Check if a message is an error and return the appropriate error.
    fn check_error(msg: &Message) -> Result<()> {
        if msg.header.msg_type == MessageType::Error {
            let of_error = OfError::parse(&msg.body)?;
            return Err(Error::OfError(of_error));
        }
        Ok(())
    }

    /// Send a flow modification to the switch.
    ///
    /// This sends the FlowMod message asynchronously without waiting for
    /// confirmation. Use `send_flow_sync` if you need to ensure the flow
    /// was successfully installed.
    pub async fn send_flow(&mut self, flow: &Flow) -> Result<()> {
        let xid = self.next_xid();
        let msg = flow.to_message(self.version, xid);
        self.send_message(&msg).await
    }

    /// Send a flow modification and wait for confirmation.
    ///
    /// Sends the FlowMod followed by a barrier request, then waits for
    /// the barrier reply. If an error occurs (e.g., invalid flow), it
    /// will be returned.
    ///
    /// This ensures the flow is installed (or rejected) before returning.
    pub async fn send_flow_sync(&mut self, flow: &Flow) -> Result<()> {
        // Send the flow
        let flow_xid = self.next_xid();
        let flow_msg = flow.to_message(self.version, flow_xid);
        self.send_message(&flow_msg).await?;

        // Send barrier
        let barrier_xid = self.next_xid();
        let barrier_msg =
            Message::new(self.version, MessageType::BarrierRequest, barrier_xid, Bytes::new());
        self.send_message(&barrier_msg).await?;

        // Wait for response - could be error or barrier reply
        loop {
            let reply = self.recv_message().await?;

            // Check for error (could be from flow or barrier)
            Self::check_error(&reply)?;

            // If we got the barrier reply, flow was successfully installed
            if reply.header.msg_type == MessageType::BarrierReply {
                return Ok(());
            }

            // Handle echo requests while waiting (keep-alive)
            if reply.header.msg_type == MessageType::EchoRequest {
                let echo_reply = Message::new(
                    self.version,
                    MessageType::EchoReply,
                    reply.header.xid,
                    reply.body.clone(),
                );
                self.send_message(&echo_reply).await?;
            }

            // Skip other async messages (PacketIn, PortStatus, FlowRemoved)
            // In a full implementation, these would be queued for processing
        }
    }

    /// Send an echo request and wait for reply.
    pub async fn echo(&mut self) -> Result<()> {
        let xid = self.next_xid();
        let request = Message::new(self.version, MessageType::EchoRequest, xid, Bytes::new());
        self.send_message(&request).await?;

        let reply = self.recv_message().await?;
        if reply.header.msg_type != MessageType::EchoReply {
            return Err(Error::InvalidMessage("expected EchoReply".into()));
        }
        if reply.header.xid != xid {
            return Err(Error::InvalidMessage("xid mismatch".into()));
        }

        Ok(())
    }

    /// Send a barrier and wait for reply.
    pub async fn barrier(&mut self) -> Result<()> {
        let xid = self.next_xid();
        let request = Message::new(self.version, MessageType::BarrierRequest, xid, Bytes::new());
        self.send_message(&request).await?;

        loop {
            let reply = self.recv_message().await?;

            // Check for errors
            Self::check_error(&reply)?;

            if reply.header.msg_type == MessageType::BarrierReply {
                return Ok(());
            }

            // Handle echo requests while waiting
            if reply.header.msg_type == MessageType::EchoRequest {
                let echo_reply = Message::new(
                    self.version,
                    MessageType::EchoReply,
                    reply.header.xid,
                    reply.body.clone(),
                );
                self.send_message(&echo_reply).await?;
            }

            // Skip other async messages
        }
    }

    /// Dump all flows from the switch.
    ///
    /// Returns all flow entries from all tables. Use `dump_flows_filtered`
    /// for more specific queries.
    pub async fn dump_flows(&mut self) -> Result<Vec<FlowStatsEntry>> {
        self.dump_flows_filtered(FlowStatsRequest::new()).await
    }

    /// Dump flows matching the given filter.
    ///
    /// The request can filter by table ID, match fields, cookie, etc.
    pub async fn dump_flows_filtered(
        &mut self,
        request: FlowStatsRequest,
    ) -> Result<Vec<FlowStatsEntry>> {
        let xid = self.next_xid();
        let msg = request.to_message(self.version, xid);
        self.send_message(&msg).await?;

        let mut all_entries = Vec::new();

        // Receive multipart replies until we get one without MORE flag
        loop {
            let reply = self.recv_message().await?;

            // Check for errors
            Self::check_error(&reply)?;

            // Handle echo requests
            if reply.header.msg_type == MessageType::EchoRequest {
                let echo_reply = Message::new(
                    self.version,
                    MessageType::EchoReply,
                    reply.header.xid,
                    reply.body.clone(),
                );
                self.send_message(&echo_reply).await?;
                continue;
            }

            // Skip non-multipart replies
            if reply.header.msg_type != MessageType::MultipartReply {
                continue;
            }

            // Parse the flow stats reply
            let (entries, has_more) = parse_flow_stats_reply(&reply.body)?;
            all_entries.extend(entries);

            if !has_more {
                break;
            }
        }

        Ok(all_entries)
    }

    /// Wait for and receive a Packet-In message.
    ///
    /// This blocks until a Packet-In message is received, handling
    /// echo requests and skipping other message types.
    pub async fn recv_packet_in(&mut self) -> Result<PacketIn> {
        loop {
            let msg = self.recv_message().await?;

            // Check for errors
            Self::check_error(&msg)?;

            // Handle echo requests (keep-alive)
            if msg.header.msg_type == MessageType::EchoRequest {
                let echo_reply = Message::new(
                    self.version,
                    MessageType::EchoReply,
                    msg.header.xid,
                    msg.body.clone(),
                );
                self.send_message(&echo_reply).await?;
                continue;
            }

            // Process Packet-In
            if msg.header.msg_type == MessageType::PacketIn {
                return PacketIn::parse(msg.body);
            }

            // Skip other async messages (PortStatus, FlowRemoved)
        }
    }

    /// Try to receive a Packet-In message without blocking.
    ///
    /// Returns `Ok(Some(packet_in))` if a Packet-In is available,
    /// `Ok(None)` if a different message was received (and handled),
    /// or an error if something went wrong.
    ///
    /// Note: This still blocks on the read itself; it just doesn't loop
    /// waiting specifically for a Packet-In.
    pub async fn try_recv_packet_in(&mut self) -> Result<Option<PacketIn>> {
        let msg = self.recv_message().await?;

        // Check for errors
        Self::check_error(&msg)?;

        // Handle echo requests
        if msg.header.msg_type == MessageType::EchoRequest {
            let echo_reply = Message::new(
                self.version,
                MessageType::EchoReply,
                msg.header.xid,
                msg.body.clone(),
            );
            self.send_message(&echo_reply).await?;
            return Ok(None);
        }

        // Process Packet-In
        if msg.header.msg_type == MessageType::PacketIn {
            return Ok(Some(PacketIn::parse(msg.body)?));
        }

        // Other message types
        Ok(None)
    }

    /// Register a flow monitor and receive the initial snapshot.
    ///
    /// Sends the monitor request and collects the initial flow updates
    /// (if `NXFMF_INITIAL` flag was set in the request). After this returns,
    /// use `recv_flow_updates()` to receive ongoing updates.
    ///
    /// Use a dedicated VConn for monitoring — the monitor produces a
    /// continuous stream that occupies the connection's recv path.
    pub async fn monitor_flows(
        &mut self,
        request: FlowMonitorRequest,
    ) -> Result<Vec<FlowUpdate>> {
        let xid = self.next_xid();
        let msg = request.to_message(self.version, xid);
        self.send_message(&msg).await?;

        let mut all_updates = Vec::new();

        // Collect initial snapshot (multipart replies until no MORE flag)
        loop {
            let reply = self.recv_message().await?;

            Self::check_error(&reply)?;

            if reply.header.msg_type == MessageType::EchoRequest {
                let echo_reply = Message::new(
                    self.version,
                    MessageType::EchoReply,
                    reply.header.xid,
                    reply.body.clone(),
                );
                self.send_message(&echo_reply).await?;
                continue;
            }

            if reply.header.msg_type != MessageType::MultipartReply {
                continue;
            }

            let (updates, has_more) = parse_flow_monitor_reply(&reply.body)?;
            all_updates.extend(updates);

            if !has_more {
                break;
            }
        }

        Ok(all_updates)
    }

    /// Receive the next batch of flow monitor updates.
    ///
    /// Blocks until flow update messages are received from OVS.
    /// Handles echo requests internally. Returns the parsed updates.
    ///
    /// Call this in a loop after `monitor_flows()` to receive ongoing
    /// flow change notifications.
    pub async fn recv_flow_updates(&mut self) -> Result<Vec<FlowUpdate>> {
        loop {
            let msg = self.recv_message().await?;

            Self::check_error(&msg)?;

            if msg.header.msg_type == MessageType::EchoRequest {
                let echo_reply = Message::new(
                    self.version,
                    MessageType::EchoReply,
                    msg.header.xid,
                    msg.body.clone(),
                );
                self.send_message(&echo_reply).await?;
                continue;
            }

            if msg.header.msg_type == MessageType::MultipartReply {
                let (updates, _has_more) = parse_flow_monitor_reply(&msg.body)?;
                if !updates.is_empty() {
                    return Ok(updates);
                }
            }

            // Skip other message types (PacketIn, PortStatus, FlowRemoved, etc.)
        }
    }

    /// Send a Packet-Out message.
    ///
    /// This injects a packet into the switch's datapath or releases
    /// a buffered packet with the specified actions.
    pub async fn send_packet_out(&mut self, packet_out: &PacketOut) -> Result<()> {
        let xid = self.next_xid();
        let msg = packet_out.to_message(self.version, xid);
        self.send_message(&msg).await
    }
}
