//! OpenFlow 1.5 flow-description readback, used inside the sole connection
//! command queue. Layout: OVS include/openflow/openflow-1.5.h ofp15_flow_desc.
//! A successful socket write is never an installation receipt.
use super::{OF_VERSION, OfIo, build_barrier_request, build_echo_reply, recv_msg, send_msg};
use anyhow::{Context, Result, ensure};
use rovs_openflow::{Flow, FlowStatsRequest, InstructionList, Match};
use std::time::Duration;
use tokio::time::{Instant, timeout_at};

#[derive(Debug)]
pub(crate) struct Description {
    table: u8,
    priority: u16,
    idle: u16,
    hard: u16,
    cookie: u64,
    fields: Vec<u8>,
    instructions: Vec<u8>,
}

fn descriptions(payload: &[u8]) -> Result<(Vec<Description>, bool)> {
    ensure!(
        payload.len() >= 8 && payload[..2] == [0, 1],
        "invalid OF1.5 flow-description multipart"
    );
    let flags = u16::from_be_bytes([payload[2], payload[3]]);
    ensure!(flags & !1 == 0, "unknown flow-description multipart flags");
    let mut rest = &payload[8..];
    let mut entries = Vec::new();
    while !rest.is_empty() {
        ensure!(rest.len() >= 32, "truncated OF1.5 flow description");
        let length = u16::from_be_bytes([rest[0], rest[1]]) as usize;
        ensure!(
            length >= 32 && length <= rest.len() && length.is_multiple_of(8),
            "invalid OF1.5 flow-description length"
        );
        let entry = &rest[..length];
        let (fields, consumed) =
            Match::decode(&entry[24..]).context("invalid flow-description match")?;
        ensure!(
            24 + consumed <= length,
            "flow-description match exceeds entry"
        );
        // OF1.5 inserts an OXS stats block between the match and instructions;
        // OF1.3's flow-stats layout is incompatible. We compare config only.
        let stats_start = 24 + consumed;
        ensure!(
            length >= stats_start + 4,
            "missing flow-description OXS header"
        );
        let stats_len =
            u16::from_be_bytes([entry[stats_start + 2], entry[stats_start + 3]]) as usize;
        ensure!(stats_len >= 4, "invalid flow-description OXS length");
        let instructions_start = stats_start + stats_len.div_ceil(8) * 8;
        ensure!(
            instructions_start <= length,
            "truncated flow-description OXS block"
        );
        let instructions = InstructionList::decode(&entry[instructions_start..])?.encode();
        entries.push(Description {
            table: entry[4],
            priority: u16::from_be_bytes([entry[6], entry[7]]),
            idle: u16::from_be_bytes([entry[8], entry[9]]),
            hard: u16::from_be_bytes([entry[10], entry[11]]),
            cookie: u64::from_be_bytes(entry[16..24].try_into()?),
            fields: fields.encode(),
            instructions,
        });
        rest = &rest[length..];
    }
    Ok((entries, flags & 1 != 0))
}

pub(crate) async fn query(
    stream: &mut impl OfIo,
    flow: &Flow,
    xid: &mut u32,
) -> Result<Vec<Description>> {
    let request_id = *xid;
    *xid = xid.wrapping_add(1);
    // Do not filter by cookie: a different cookie at the same table/priority/
    // match is a conflict, and must not be overwritten as an absent entry.
    let request = FlowStatsRequest::new()
        .table(flow.table_id)
        .match_fields(flow.match_fields.clone())
        .to_message(OF_VERSION, request_id);
    send_msg(stream, &request.encode()).await?;
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut result = Vec::new();
    let mut total = 0usize;
    loop {
        let reply = timeout_at(deadline, recv_msg(stream))
            .await
            .context("flow-description timeout")??;
        if reply.msg_type == 2 {
            send_msg(stream, &build_echo_reply(reply.xid, &reply.payload)).await?;
            continue;
        }
        if reply.xid != request_id {
            continue;
        }
        ensure!(
            reply.msg_type != 1,
            "switch rejected flow-description readback"
        );
        ensure!(reply.msg_type == 19, "unexpected flow-description response");
        total += reply.payload.len();
        ensure!(
            total <= 8 * 1024 * 1024,
            "flow-description response exceeds limit"
        );
        let (entries, more) = descriptions(&reply.payload)?;
        result.extend(entries);
        if !more {
            return Ok(result);
        }
    }
}

fn same_identity(entry: &Description, flow: &Flow) -> bool {
    entry.table == flow.table_id
        && entry.priority == flow.priority
        && entry.fields == flow.match_fields.encode()
}

pub(crate) fn verify(
    entries: &[Description],
    expected: Option<&Flow>,
    selector: &Flow,
) -> Result<()> {
    let matching: Vec<_> = entries
        .iter()
        .filter(|entry| same_identity(entry, selector))
        .collect();
    match expected {
        None => ensure!(matching.is_empty(), "flow revision conflict: entry exists"),
        Some(flow) => {
            ensure!(
                matching.len() == 1,
                "flow revision conflict: expected exact entry"
            );
            let entry = matching[0];
            let instructions = if flow.instructions.is_empty() {
                InstructionList::new()
                    .apply_actions(flow.actions.clone())
                    .encode()
            } else {
                flow.instructions.encode()
            };
            ensure!(
                entry.cookie == flow.cookie
                    && entry.idle == flow.idle_timeout
                    && entry.hard == flow.hard_timeout
                    && entry.instructions == instructions,
                "flow revision conflict: live entry differs"
            );
        }
    }
    Ok(())
}

pub(crate) async fn apply(stream: &mut impl OfIo, flow: &Flow, xid: &mut u32) -> Result<()> {
    let operation = *xid;
    *xid = xid.wrapping_add(1);
    send_msg(stream, &flow.to_message(OF_VERSION, operation).encode()).await?;
    let barrier = *xid;
    *xid = xid.wrapping_add(1);
    send_msg(stream, &build_barrier_request(barrier)).await?;
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut rejected = false;
    loop {
        let reply = timeout_at(deadline, recv_msg(stream))
            .await
            .context("flow mutation barrier timeout; reconcile outcome")??;
        if reply.msg_type == 2 {
            send_msg(stream, &build_echo_reply(reply.xid, &reply.payload)).await?;
        }
        if reply.msg_type == 1 && (reply.xid == operation || reply.xid == barrier) {
            rejected = true;
            ensure!(reply.xid != barrier, "switch rejected mutation barrier");
        }
        if reply.msg_type == 21 && reply.xid == barrier {
            ensure!(!rejected, "switch rejected flow mutation");
            return Ok(());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rovs_openflow::ActionList;
    use tokio::net::TcpStream;

    fn reply(flow: &Flow) -> Vec<u8> {
        let mut entry = vec![0u8; 24];
        entry[4] = flow.table_id;
        entry[6..8].copy_from_slice(&flow.priority.to_be_bytes());
        entry[16..24].copy_from_slice(&flow.cookie.to_be_bytes());
        entry.extend(flow.match_fields.encode());
        entry.extend([0, 0, 0, 4, 0, 0, 0, 0]); // Empty padded OXS stats.
        entry.extend(
            InstructionList::new()
                .apply_actions(flow.actions.clone())
                .encode(),
        );
        let length = entry.len() as u16;
        entry[..2].copy_from_slice(&length.to_be_bytes());
        let mut payload = vec![0, 1, 0, 0, 0, 0, 0, 0];
        payload.extend(entry);
        payload
    }

    #[test]
    fn native_description_requires_exact_cookie_match_actions_and_valid_lengths() {
        let flow = Flow::add()
            .table(7)
            .priority(123)
            .cookie(901)
            .match_fields(Match::new().in_port(3))
            .actions(ActionList::new().normal());
        let bytes = reply(&flow);
        let (entries, _) = descriptions(&bytes).unwrap();
        verify(&entries, Some(&flow), &flow).unwrap();
        assert!(verify(&entries, None, &flow).is_err());
        let other = flow.clone().cookie(902);
        assert!(verify(&entries, Some(&other), &flow).is_err());
        for length in 9..bytes.len() {
            assert!(descriptions(&bytes[..length]).is_err());
        }
        let mut corrupted = bytes;
        corrupted[8..10].copy_from_slice(&u16::MAX.to_be_bytes());
        assert!(descriptions(&corrupted).is_err());
    }

    #[tokio::test]
    async fn native_wire_requires_matching_barrier_and_actual_readback() {
        use super::super::build_raw_msg;
        use rovs_openflow::MessageType;
        use tokio::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mut connection = TcpStream::connect(listener.local_addr().unwrap())
            .await
            .unwrap();
        let (mut switch, _) = listener.accept().await.unwrap();
        let flow = Flow::add()
            .table(7)
            .priority(123)
            .cookie(901)
            .match_fields(Match::new().in_port(3))
            .actions(ActionList::new().normal());
        let expected = flow.clone();
        let peer = tokio::spawn(async move {
            let command = recv_msg(&mut switch).await.unwrap();
            assert_eq!(command.msg_type, 14);
            assert_eq!(
                command.payload,
                expected.to_message(OF_VERSION, command.xid).body.as_ref()
            );
            let barrier = recv_msg(&mut switch).await.unwrap();
            assert_eq!(barrier.msg_type, 20);
            // An unrelated BarrierReply cannot acknowledge this write.
            send_msg(
                &mut switch,
                &build_raw_msg(MessageType::BarrierReply, barrier.xid + 42, &[]),
            )
            .await
            .unwrap();
            send_msg(
                &mut switch,
                &build_raw_msg(MessageType::BarrierReply, barrier.xid, &[]),
            )
            .await
            .unwrap();
            let query = recv_msg(&mut switch).await.unwrap();
            assert_eq!(query.msg_type, 18);
            assert_eq!(&query.payload[..2], &[0, 1]);
            send_msg(
                &mut switch,
                &build_raw_msg(MessageType::MultipartReply, query.xid, &reply(&expected)),
            )
            .await
            .unwrap();
        });
        let mut xid = 3;
        apply(&mut connection, &flow, &mut xid).await.unwrap();
        let entries = query(&mut connection, &flow, &mut xid).await.unwrap();
        verify(&entries, Some(&flow), &flow).unwrap();
        peer.await.unwrap();
    }

    #[tokio::test]
    async fn native_rejection_is_not_success_even_after_barrier() {
        use super::super::build_raw_msg;
        use rovs_openflow::MessageType;
        use tokio::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mut connection = TcpStream::connect(listener.local_addr().unwrap())
            .await
            .unwrap();
        let (mut switch, _) = listener.accept().await.unwrap();
        let peer = tokio::spawn(async move {
            let command = recv_msg(&mut switch).await.unwrap();
            let barrier = recv_msg(&mut switch).await.unwrap();
            send_msg(
                &mut switch,
                &build_raw_msg(MessageType::Error, command.xid, &[0, 3, 0, 2]),
            )
            .await
            .unwrap();
            send_msg(
                &mut switch,
                &build_raw_msg(MessageType::BarrierReply, barrier.xid, &[]),
            )
            .await
            .unwrap();
        });
        assert!(apply(&mut connection, &Flow::add(), &mut 1).await.is_err());
        peer.await.unwrap();
    }
}
