//! Native Fail2ban control-socket adapter.
//!
//! Fail2ban frames pickled data with `<F2B_END_COMMAND>`. Decoding uses
//! `serde_pickle::Value`, whose model contains data only and cannot import or
//! invoke Python globals. Every frame and decoded tree is bounded before use.

use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::time::Duration;
use std::{collections::HashMap, str};

use anyhow::{bail, ensure, Context, Result};
use serde_json::Value as JsonValue;
use serde_pickle::value::Value as PickleValue;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;

use super::fail2ban::{
    BanIpInput, BanIpOutput, JailInput, JailStatusOutput, MonitorSourceKind, UnbanIpOutput,
};

const END: &[u8] = b"<F2B_END_COMMAND>";
const MAX_FRAME_BYTES: usize = 1024 * 1024;
const MAX_VALUE_DEPTH: usize = 16;
const MAX_VALUE_NODES: usize = 4096;
const MAX_COLLECTION_ITEMS: usize = 1024;
const MAX_TEXT_BYTES: usize = 64 * 1024;

pub struct Fail2banMethods {
    socket: PathBuf,
    deadline: Duration,
}

impl Fail2banMethods {
    pub fn new(socket: impl AsRef<Path>, deadline: Duration) -> Result<Self> {
        let socket = socket.as_ref();
        ensure!(
            socket.is_absolute(),
            "Fail2ban socket path must be absolute"
        );
        ensure!(
            !deadline.is_zero(),
            "Fail2ban request deadline must be positive"
        );
        ensure!(
            deadline <= Duration::from_secs(30),
            "Fail2ban request deadline exceeds 30 seconds"
        );
        Ok(Self {
            socket: socket.to_path_buf(),
            deadline,
        })
    }

    pub async fn dispatch(&self, method: &str, args: &JsonValue) -> Result<JsonValue> {
        match method {
            "ban_ip" => {
                let input: BanIpInput = serde_json::from_value(args.clone())?;
                validate_jail(&input.jail)?;
                let ip = validate_ip(&input.ip)?;
                let affected = self
                    .request(vec!["set", &input.jail, "banip", &ip])
                    .await
                    .and_then(as_nonnegative_count)?;
                Ok(serde_json::to_value(BanIpOutput {
                    jail: input.jail,
                    ip,
                    affected,
                })?)
            }
            "unban_ip" => {
                let input: BanIpInput = serde_json::from_value(args.clone())?;
                validate_jail(&input.jail)?;
                let ip = validate_ip(&input.ip)?;
                let affected = self
                    .request(vec!["set", &input.jail, "unbanip", "--report-absent", &ip])
                    .await
                    .and_then(as_nonnegative_count)?;
                Ok(serde_json::to_value(UnbanIpOutput {
                    jail: input.jail,
                    ip,
                    affected,
                })?)
            }
            "get_jail_status" => {
                let input: JailInput = serde_json::from_value(args.clone())?;
                validate_jail(&input.jail)?;
                let response = self.request(vec!["status", &input.jail]).await?;
                Ok(serde_json::to_value(parse_jail_status(
                    input.jail, &response,
                )?)?)
            }
            "reload" => bail!("fail2ban reload requires a validated configuration stream"),
            _ => bail!("unsupported fail2ban method"),
        }
    }

    async fn request(&self, command: Vec<&str>) -> Result<PickleValue> {
        let command: Vec<String> = command.into_iter().map(str::to_string).collect();
        let mut encoded =
            serde_pickle::to_vec(&command, serde_pickle::SerOptions::new().proto_v2())?;
        ensure!(
            encoded.len() + END.len() <= MAX_FRAME_BYTES,
            "Fail2ban request is too large"
        );
        encoded.extend_from_slice(END);
        let socket = self.socket.clone();
        tokio::time::timeout(self.deadline, async move {
            let mut stream = UnixStream::connect(&socket)
                .await
                .with_context(|| format!("connect Fail2ban socket {}", socket.display()))?;
            stream.write_all(&encoded).await?;
            let frame = read_frame(&mut stream).await?;
            decode_ack(&frame)
        })
        .await
        .context("Fail2ban request deadline exceeded")?
    }
}

async fn read_frame(stream: &mut UnixStream) -> Result<Vec<u8>> {
    let mut frame = Vec::new();
    let mut chunk = [0_u8; 8192];
    loop {
        let read = stream.read(&mut chunk).await?;
        ensure!(read != 0, "truncated Fail2ban response frame");
        frame.extend_from_slice(&chunk[..read]);
        ensure!(
            frame.len() <= MAX_FRAME_BYTES,
            "Fail2ban response frame is too large"
        );
        if let Some(position) = find_subslice(&frame, END) {
            ensure!(
                position + END.len() == frame.len(),
                "trailing data after Fail2ban response terminator"
            );
            frame.truncate(position);
            ensure!(!frame.is_empty(), "empty Fail2ban response frame");
            return Ok(frame);
        }
    }
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn decode_ack(frame: &[u8]) -> Result<PickleValue> {
    preflight_pickle(frame)?;
    let value =
        serde_pickle::value_from_slice(frame, serde_pickle::DeOptions::new().keep_restore_state())
            .context("invalid Fail2ban pickle response")?;
    let mut nodes = 0;
    validate_value(&value, 0, &mut nodes)?;
    let fields = as_sequence(&value).context("Fail2ban response must be an ack tuple")?;
    ensure!(fields.len() == 2, "Fail2ban ack tuple must have two fields");
    let ack = as_i64(&fields[0]).context("invalid Fail2ban ack code")?;
    ensure!(matches!(ack, 0 | 1), "unknown Fail2ban ack code");
    if ack == 1 {
        let detail = first_text(&fields[1]).unwrap_or("provider returned an opaque error");
        bail!("Fail2ban rejected request: {detail}");
    }
    Ok(fields[1].clone())
}

#[derive(Clone, Copy)]
struct PickleShape {
    nodes: usize,
    depth: usize,
    items: usize,
    container: bool,
    global: bool,
}

impl PickleShape {
    const fn scalar() -> Self {
        Self {
            nodes: 1,
            depth: 0,
            items: 0,
            container: false,
            global: false,
        }
    }

    const fn global() -> Self {
        Self {
            global: true,
            ..Self::scalar()
        }
    }

    fn collection(values: &[Self]) -> Result<Self> {
        ensure!(
            values.len() <= MAX_COLLECTION_ITEMS,
            "Fail2ban pickle collection is too large"
        );
        let nodes = values.iter().try_fold(1_usize, |total, value| {
            total
                .checked_add(value.nodes)
                .context("pickle node overflow")
        })?;
        let depth = values
            .iter()
            .map(|value| value.depth)
            .max()
            .unwrap_or(0)
            .checked_add(1)
            .context("pickle depth overflow")?;
        ensure!(
            nodes <= MAX_VALUE_NODES,
            "Fail2ban pickle has too many values"
        );
        ensure!(
            depth <= MAX_VALUE_DEPTH,
            "Fail2ban pickle is too deeply nested"
        );
        Ok(Self {
            nodes,
            depth,
            items: values.len(),
            container: true,
            global: false,
        })
    }

    fn append(&mut self, values: &[Self]) -> Result<()> {
        ensure!(self.container, "pickle mutation target is not a container");
        self.items = self
            .items
            .checked_add(values.len())
            .context("pickle item count overflow")?;
        ensure!(
            self.items <= MAX_COLLECTION_ITEMS,
            "Fail2ban pickle collection is too large"
        );
        self.nodes = values.iter().try_fold(self.nodes, |total, value| {
            total
                .checked_add(value.nodes)
                .context("pickle node overflow")
        })?;
        self.depth = self.depth.max(
            values
                .iter()
                .map(|value| value.depth)
                .max()
                .unwrap_or(0)
                .checked_add(1)
                .context("pickle depth overflow")?,
        );
        ensure!(
            self.nodes <= MAX_VALUE_NODES,
            "Fail2ban pickle has too many values"
        );
        ensure!(
            self.depth <= MAX_VALUE_DEPTH,
            "Fail2ban pickle is too deeply nested"
        );
        Ok(())
    }
}

enum StackItem {
    Mark,
    Value(PickleShape),
}

/// Bounds the pickle VM before `serde_pickle` resolves memo references into an
/// owned tree. Container memo reads are forbidden because they can encode
/// cycles or exponentially expand a small frame while aliases are resolved.
fn preflight_pickle(input: &[u8]) -> Result<()> {
    ensure!(
        input.len() <= MAX_FRAME_BYTES,
        "Fail2ban pickle is too large"
    );
    let mut cursor = 0;
    let mut operations = 0_usize;
    let mut stack = Vec::<StackItem>::new();
    let mut memo = HashMap::<u32, PickleShape>::new();
    let mut next_memo = 0_u32;
    let mut stopped = false;
    while cursor < input.len() {
        operations += 1;
        ensure!(
            operations <= MAX_VALUE_NODES * 4,
            "Fail2ban pickle has too many opcodes"
        );
        let opcode = take(input, &mut cursor, 1)?[0];
        match opcode {
            b'.' => {
                stopped = true;
                break;
            }
            b'(' => stack.push(StackItem::Mark),
            b'0' => {
                stack.pop().context("pickle POP underflow")?;
            }
            b'1' => {
                take_marked(&mut stack)?;
            }
            b'2' => {
                let value = top_value(&stack)?;
                ensure!(
                    !value.container,
                    "container pickle duplication is forbidden"
                );
                stack.push(StackItem::Value(value));
            }
            b'N' | 0x88 | 0x89 => stack.push(StackItem::Value(PickleShape::scalar())),
            b'I' | b'L' | b'F' | b'S' | b'V' => {
                ensure!(
                    take_line(input, &mut cursor)?.len() <= MAX_TEXT_BYTES,
                    "Fail2ban pickle scalar is too large"
                );
                stack.push(StackItem::Value(PickleShape::scalar()));
            }
            b'J' => push_fixed(input, &mut cursor, &mut stack, 4)?,
            b'K' => push_fixed(input, &mut cursor, &mut stack, 1)?,
            b'M' => push_fixed(input, &mut cursor, &mut stack, 2)?,
            b'G' => push_fixed(input, &mut cursor, &mut stack, 8)?,
            b'T' | b'X' | b'B' => {
                let length = take_u32(input, &mut cursor)? as usize;
                push_blob(input, &mut cursor, &mut stack, length)?;
            }
            b'U' | b'C' | 0x8c => {
                let length = usize::from(take(input, &mut cursor, 1)?[0]);
                push_blob(input, &mut cursor, &mut stack, length)?;
            }
            0x8a => {
                let length = usize::from(take(input, &mut cursor, 1)?[0]);
                push_blob(input, &mut cursor, &mut stack, length)?;
            }
            0x8b => {
                let length = take_u32(input, &mut cursor)? as usize;
                push_blob(input, &mut cursor, &mut stack, length)?;
            }
            0x8d | 0x8e | 0x96 => {
                let length = take_u64(input, &mut cursor)?;
                ensure!(length <= usize::MAX as u64, "pickle blob length overflow");
                push_blob(input, &mut cursor, &mut stack, length as usize)?;
            }
            b')' | b']' | b'}' | 0x8f => {
                stack.push(StackItem::Value(PickleShape::collection(&[])?))
            }
            b'l' | b't' | b'd' | 0x91 => {
                let values = take_marked(&mut stack)?;
                stack.push(StackItem::Value(PickleShape::collection(&values)?));
            }
            0x85..=0x87 => {
                let values = pop_values(&mut stack, usize::from(opcode - 0x84))?;
                stack.push(StackItem::Value(PickleShape::collection(&values)?));
            }
            b'a' => mutate_container(&mut stack, 1)?,
            b's' => mutate_container(&mut stack, 2)?,
            b'e' | b'u' | 0x90 => {
                let values = take_marked(&mut stack)?;
                value_mut(stack.last_mut().context("pickle mutation target missing")?)?
                    .append(&values)?;
            }
            b'c' => {
                ensure!(
                    take_line(input, &mut cursor)?.len() <= MAX_TEXT_BYTES,
                    "pickle global is too large"
                );
                ensure!(
                    take_line(input, &mut cursor)?.len() <= MAX_TEXT_BYTES,
                    "pickle global is too large"
                );
                stack.push(StackItem::Value(PickleShape::global()));
            }
            0x93 => {
                let values = pop_values(&mut stack, 2)?;
                ensure!(
                    values.iter().all(|value| !value.container),
                    "invalid STACK_GLOBAL operands"
                );
                stack.push(StackItem::Value(PickleShape::global()));
            }
            b'R' => {
                let args = pop_value(&mut stack)?;
                let callable = pop_value(&mut stack)?;
                ensure!(
                    callable.global && args.container,
                    "unsafe pickle REDUCE operands"
                );
                stack.push(StackItem::Value(args));
            }
            b'p' => memo_put(
                parse_decimal(take_line(input, &mut cursor)?)?,
                &stack,
                &mut memo,
            )?,
            b'q' => {
                let key = u32::from(take(input, &mut cursor, 1)?[0]);
                memo_put(key, &stack, &mut memo)?;
            }
            b'r' => {
                let key = take_u32(input, &mut cursor)?;
                memo_put(key, &stack, &mut memo)?;
            }
            0x94 => {
                memo_put(next_memo, &stack, &mut memo)?;
                next_memo = next_memo.checked_add(1).context("pickle memo overflow")?;
            }
            b'g' => memo_get(
                parse_decimal(take_line(input, &mut cursor)?)?,
                &mut stack,
                &memo,
            )?,
            b'h' => {
                let key = u32::from(take(input, &mut cursor, 1)?[0]);
                memo_get(key, &mut stack, &memo)?;
            }
            b'j' => {
                let key = take_u32(input, &mut cursor)?;
                memo_get(key, &mut stack, &memo)?;
            }
            0x80 => ensure!(
                take(input, &mut cursor, 1)?[0] <= 5,
                "unsupported pickle protocol"
            ),
            0x95 => {
                let length = take_u64(input, &mut cursor)?;
                ensure!(
                    length <= MAX_FRAME_BYTES as u64,
                    "pickle FRAME is too large"
                );
                ensure!(
                    length <= (input.len() - cursor) as u64,
                    "truncated pickle FRAME"
                );
            }
            b'P' | b'Q' | b'b' | b'i' | b'o' | 0x81..=0x84 | 0x92 | 0x97 | 0x98 => {
                bail!("unsupported stateful pickle opcode")
            }
            _ => bail!("unsupported pickle opcode 0x{opcode:02x}"),
        }
        ensure!(
            stack.len() <= MAX_VALUE_NODES,
            "Fail2ban pickle stack is too large"
        );
    }
    ensure!(stopped, "pickle is missing STOP");
    ensure!(cursor == input.len(), "trailing bytes after pickle STOP");
    ensure!(
        stack.len() == 1 && matches!(stack[0], StackItem::Value(_)),
        "pickle result stack is invalid"
    );
    Ok(())
}

fn take<'a>(input: &'a [u8], cursor: &mut usize, length: usize) -> Result<&'a [u8]> {
    let end = cursor
        .checked_add(length)
        .context("pickle length overflow")?;
    let value = input
        .get(*cursor..end)
        .context("truncated pickle operand")?;
    *cursor = end;
    Ok(value)
}
fn take_line<'a>(input: &'a [u8], cursor: &mut usize) -> Result<&'a [u8]> {
    let rest = input.get(*cursor..).context("truncated pickle line")?;
    let length = rest
        .iter()
        .position(|byte| *byte == b'\n')
        .context("unterminated pickle line")?;
    let line = take(input, cursor, length + 1)?;
    Ok(&line[..length])
}
fn take_u32(input: &[u8], cursor: &mut usize) -> Result<u32> {
    Ok(u32::from_le_bytes(take(input, cursor, 4)?.try_into()?))
}
fn take_u64(input: &[u8], cursor: &mut usize) -> Result<u64> {
    Ok(u64::from_le_bytes(take(input, cursor, 8)?.try_into()?))
}
fn push_fixed(
    input: &[u8],
    cursor: &mut usize,
    stack: &mut Vec<StackItem>,
    length: usize,
) -> Result<()> {
    take(input, cursor, length)?;
    stack.push(StackItem::Value(PickleShape::scalar()));
    Ok(())
}
fn push_blob(
    input: &[u8],
    cursor: &mut usize,
    stack: &mut Vec<StackItem>,
    length: usize,
) -> Result<()> {
    ensure!(
        length <= MAX_TEXT_BYTES,
        "Fail2ban pickle blob is too large"
    );
    push_fixed(input, cursor, stack, length)
}
fn top_value(stack: &[StackItem]) -> Result<PickleShape> {
    match stack.last().context("pickle stack underflow")? {
        StackItem::Value(value) => Ok(*value),
        StackItem::Mark => bail!("pickle value expected"),
    }
}
fn pop_value(stack: &mut Vec<StackItem>) -> Result<PickleShape> {
    match stack.pop().context("pickle stack underflow")? {
        StackItem::Value(value) => Ok(value),
        StackItem::Mark => bail!("pickle value expected"),
    }
}
fn pop_values(stack: &mut Vec<StackItem>, count: usize) -> Result<Vec<PickleShape>> {
    ensure!(stack.len() >= count, "pickle stack underflow");
    let mut values = Vec::with_capacity(count);
    for _ in 0..count {
        values.push(pop_value(stack)?);
    }
    values.reverse();
    Ok(values)
}
fn take_marked(stack: &mut Vec<StackItem>) -> Result<Vec<PickleShape>> {
    let mark = stack
        .iter()
        .rposition(|item| matches!(item, StackItem::Mark))
        .context("pickle MARK missing")?;
    let tail = stack.split_off(mark);
    tail.into_iter()
        .skip(1)
        .map(|item| match item {
            StackItem::Value(value) => Ok(value),
            StackItem::Mark => bail!("nested pickle MARK in collection"),
        })
        .collect()
}
fn value_mut(item: &mut StackItem) -> Result<&mut PickleShape> {
    match item {
        StackItem::Value(value) => Ok(value),
        StackItem::Mark => bail!("pickle value expected"),
    }
}
fn mutate_container(stack: &mut Vec<StackItem>, count: usize) -> Result<()> {
    let values = pop_values(stack, count)?;
    value_mut(stack.last_mut().context("pickle mutation target missing")?)?.append(&values)
}
fn parse_decimal(value: &[u8]) -> Result<u32> {
    Ok(str::from_utf8(value)?.parse::<u32>()?)
}
fn memo_put(key: u32, stack: &[StackItem], memo: &mut HashMap<u32, PickleShape>) -> Result<()> {
    ensure!(
        memo.len() < MAX_VALUE_NODES || memo.contains_key(&key),
        "Fail2ban pickle memo is too large"
    );
    memo.insert(key, top_value(stack)?);
    Ok(())
}
fn memo_get(key: u32, stack: &mut Vec<StackItem>, memo: &HashMap<u32, PickleShape>) -> Result<()> {
    let value = *memo.get(&key).context("pickle memo reference is missing")?;
    ensure!(
        !value.container,
        "container pickle memo references are forbidden"
    );
    stack.push(StackItem::Value(value));
    Ok(())
}

fn validate_value(value: &PickleValue, depth: usize, nodes: &mut usize) -> Result<()> {
    ensure!(
        depth <= MAX_VALUE_DEPTH,
        "Fail2ban response nesting is too deep"
    );
    *nodes = nodes
        .checked_add(1)
        .context("Fail2ban node count overflow")?;
    ensure!(
        *nodes <= MAX_VALUE_NODES,
        "Fail2ban response has too many values"
    );
    match value {
        PickleValue::String(value) => ensure!(
            value.len() <= MAX_TEXT_BYTES,
            "Fail2ban string is too large"
        ),
        PickleValue::Bytes(value) => ensure!(
            value.len() <= MAX_TEXT_BYTES,
            "Fail2ban bytes are too large"
        ),
        PickleValue::List(values) | PickleValue::Tuple(values) => {
            ensure!(
                values.len() <= MAX_COLLECTION_ITEMS,
                "Fail2ban collection is too large"
            );
            for value in values {
                validate_value(value, depth + 1, nodes)?;
            }
        }
        PickleValue::None | PickleValue::Bool(_) | PickleValue::I64(_) | PickleValue::F64(_) => {}
        PickleValue::Int(_)
        | PickleValue::Set(_)
        | PickleValue::FrozenSet(_)
        | PickleValue::Dict(_) => {
            bail!("unsupported Fail2ban response value")
        }
    }
    Ok(())
}

fn as_sequence(value: &PickleValue) -> Option<&[PickleValue]> {
    match value {
        PickleValue::List(values) | PickleValue::Tuple(values) => Some(values),
        _ => None,
    }
}

fn as_i64(value: &PickleValue) -> Option<i64> {
    match value {
        PickleValue::I64(value) => Some(*value),
        _ => None,
    }
}

fn as_nonnegative_count(value: PickleValue) -> Result<u64> {
    u64::try_from(as_i64(&value).context("Fail2ban response is not an integer count")?)
        .context("Fail2ban returned a negative count")
}

fn as_text(value: &PickleValue) -> Option<&str> {
    match value {
        PickleValue::String(value) => Some(value),
        PickleValue::Bytes(value) => std::str::from_utf8(value).ok(),
        // Fail2ban IPAddr.__reduce__ serializes as str(<address>); the inert
        // data decoder retains the single constructor argument as a tuple.
        PickleValue::Tuple(values) if values.len() == 1 => as_text(&values[0]),
        _ => None,
    }
}

fn first_text(value: &PickleValue) -> Option<&str> {
    as_text(value).or_else(|| as_sequence(value)?.iter().find_map(first_text))
}

fn parse_jail_status(jail: String, value: &PickleValue) -> Result<JailStatusOutput> {
    let sections = as_sequence(value).context("jail status must be a sequence")?;
    let filter = named_value(sections, "Filter")?;
    let actions = named_value(sections, "Actions")?;
    let filter = as_sequence(filter).context("Filter status must be a sequence")?;
    let actions = as_sequence(actions).context("Actions status must be a sequence")?;
    let currently_failed = named_count(filter, "Currently failed")?;
    let total_failed = named_count(filter, "Total failed")?;
    let currently_banned = named_count(actions, "Currently banned")?;
    let total_banned = named_count(actions, "Total banned")?;
    let banned_ips = named_strings(actions, "Banned IP list")?;
    let (monitor_source_kind, monitored_sources) = if has_named(filter, "File list") {
        (
            MonitorSourceKind::Files,
            named_strings(filter, "File list")?,
        )
    } else {
        (
            MonitorSourceKind::Journal,
            named_strings(filter, "Journal matches")?,
        )
    };
    ensure!(
        u64::try_from(banned_ips.len())? == currently_banned,
        "Fail2ban current-ban count does not match its IP list"
    );
    Ok(JailStatusOutput {
        jail,
        currently_failed,
        total_failed,
        currently_banned,
        total_banned,
        banned_ips,
        monitor_source_kind,
        monitored_sources,
    })
}

fn named_value<'a>(pairs: &'a [PickleValue], name: &str) -> Result<&'a PickleValue> {
    pairs
        .iter()
        .find_map(|pair| {
            let pair = as_sequence(pair)?;
            (pair.len() == 2 && as_text(&pair[0]) == Some(name)).then_some(&pair[1])
        })
        .with_context(|| format!("Fail2ban status is missing '{name}'"))
}

fn has_named(pairs: &[PickleValue], name: &str) -> bool {
    named_value(pairs, name).is_ok()
}

fn named_count(pairs: &[PickleValue], name: &str) -> Result<u64> {
    u64::try_from(as_i64(named_value(pairs, name)?).context("status count is not an integer")?)
        .with_context(|| format!("Fail2ban status '{name}' is negative"))
}

fn named_strings(pairs: &[PickleValue], name: &str) -> Result<Vec<String>> {
    let values = as_sequence(named_value(pairs, name)?)
        .with_context(|| format!("Fail2ban status '{name}' is not a sequence"))?;
    values
        .iter()
        .map(|value| {
            as_text(value)
                .with_context(|| format!("Fail2ban status '{name}' contains a non-string"))
                .map(str::to_string)
        })
        .collect()
}

fn validate_jail(jail: &str) -> Result<()> {
    ensure!(
        !jail.is_empty() && jail.len() <= 64,
        "invalid Fail2ban jail name"
    );
    ensure!(jail != "." && jail != "..", "invalid Fail2ban jail name");
    ensure!(
        jail.bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.')),
        "invalid Fail2ban jail name"
    );
    Ok(())
}

fn validate_ip(ip: &str) -> Result<String> {
    Ok(ip
        .parse::<IpAddr>()
        .context("invalid IP address")?
        .to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::UnixListener;

    fn text(value: &str) -> PickleValue {
        PickleValue::String(value.to_string())
    }
    fn pair(name: &str, value: PickleValue) -> PickleValue {
        PickleValue::Tuple(vec![text(name), value])
    }
    fn ack(value: PickleValue) -> Vec<u8> {
        serde_pickle::value_to_vec(
            &PickleValue::Tuple(vec![PickleValue::I64(0), value]),
            serde_pickle::SerOptions::new().proto_v2(),
        )
        .unwrap()
    }
    fn status() -> PickleValue {
        PickleValue::List(vec![
            pair(
                "Filter",
                PickleValue::List(vec![
                    pair("Currently failed", PickleValue::I64(1)),
                    pair("Total failed", PickleValue::I64(7)),
                    pair(
                        "File list",
                        PickleValue::List(vec![text("/fixture/auth.log")]),
                    ),
                ]),
            ),
            pair(
                "Actions",
                PickleValue::List(vec![
                    pair("Currently banned", PickleValue::I64(1)),
                    pair("Total banned", PickleValue::I64(3)),
                    pair("Banned IP list", PickleValue::List(vec![text("192.0.2.4")])),
                ]),
            ),
        ])
    }

    async fn fixture(responses: Vec<Vec<u8>>) -> (tempfile::TempDir, Fail2banMethods) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("fail2ban.sock");
        let listener = UnixListener::bind(&path).unwrap();
        tokio::spawn(async move {
            for response in responses {
                let (mut stream, _) = listener.accept().await.unwrap();
                let request = read_frame(&mut stream).await.unwrap();
                let command: Vec<String> =
                    serde_pickle::from_slice(&request, serde_pickle::DeOptions::new()).unwrap();
                assert!(matches!(command[0].as_str(), "set" | "status"));
                stream.write_all(&response).await.unwrap();
                stream.write_all(END).await.unwrap();
            }
        });
        let service = Fail2banMethods::new(&path, Duration::from_secs(2)).unwrap();
        (dir, service)
    }

    #[tokio::test]
    async fn native_fixture_covers_ban_unban_and_status() {
        let (_dir, service) = fixture(vec![
            ack(PickleValue::I64(1)),
            ack(PickleValue::I64(1)),
            ack(status()),
        ])
        .await;
        let banned = service
            .dispatch(
                "ban_ip",
                &serde_json::json!({"jail":"sshd","ip":"192.0.2.4"}),
            )
            .await
            .unwrap();
        assert_eq!(banned["affected"], 1);
        let unbanned = service
            .dispatch(
                "unban_ip",
                &serde_json::json!({"jail":"sshd","ip":"192.0.2.4"}),
            )
            .await
            .unwrap();
        assert_eq!(unbanned["affected"], 1);
        let status = service
            .dispatch("get_jail_status", &serde_json::json!({"jail":"sshd"}))
            .await
            .unwrap();
        assert_eq!(status["currently_failed"], 1);
        assert_eq!(status["banned_ips"], serde_json::json!(["192.0.2.4"]));
    }

    #[tokio::test]
    async fn rejects_provider_error_truncated_oversized_and_unknown_values() {
        let rejected = serde_pickle::value_to_vec(
            &PickleValue::Tuple(vec![PickleValue::I64(1), text("Unknown Jail")]),
            serde_pickle::SerOptions::new().proto_v2(),
        )
        .unwrap();
        assert!(decode_ack(&rejected)
            .unwrap_err()
            .to_string()
            .contains("Unknown Jail"));
        assert!(decode_ack(&[0x80]).is_err());
        assert!(decode_ack(&ack(PickleValue::Dict(Default::default()))).is_err());

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("oversized.sock");
        let listener = UnixListener::bind(&path).unwrap();
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let _ = read_frame(&mut stream).await.unwrap();
            stream
                .write_all(&vec![b'x'; MAX_FRAME_BYTES + 1])
                .await
                .unwrap();
        });
        let service = Fail2banMethods::new(path, Duration::from_secs(2)).unwrap();
        assert!(service
            .dispatch("get_jail_status", &serde_json::json!({"jail":"sshd"}))
            .await
            .is_err());
    }

    #[tokio::test]
    async fn validates_jail_and_ip_before_connecting() {
        let service = Fail2banMethods::new("/fixture/absent.sock", Duration::from_secs(1)).unwrap();
        for args in [
            serde_json::json!({"jail":"../sshd","ip":"192.0.2.4"}),
            serde_json::json!({"jail":"sshd","ip":"not-an-ip"}),
            serde_json::json!({"jail":"sshd","ip":"192.0.2.4","extra":true}),
        ] {
            assert!(service.dispatch("ban_ip", &args).await.is_err());
        }
    }

    #[test]
    fn preflight_rejects_container_memo_aliases_and_cycles() {
        // PROTO 2; EMPTY_LIST; BINPUT 0; MARK; BINGET 0; APPENDS; STOP.
        // Resolving the memo produces a list containing itself.
        let cycle = [0x80, 2, b']', b'q', 0, b'(', b'h', 0, b'e', b'.'];
        let error = preflight_pickle(&cycle).unwrap_err().to_string();
        assert!(error.contains("container pickle memo references"));

        // Even an acyclic repeated container reference is rejected before the
        // decoder can clone its subtree repeatedly.
        let alias = [
            0x80, 2, b']', b'q', 0, b'0', b']', b'(', b'h', 0, b'h', 0, b'e', b'.',
        ];
        assert!(preflight_pickle(&alias).is_err());
    }

    #[test]
    fn preflight_rejects_excessive_nesting_without_decoding() {
        let mut nested = vec![0x80, 2];
        nested.extend(std::iter::repeat_n(b']', MAX_VALUE_DEPTH + 2));
        nested.extend(std::iter::repeat_n(b'a', MAX_VALUE_DEPTH + 1));
        nested.push(b'.');
        let error = preflight_pickle(&nested).unwrap_err().to_string();
        assert!(error.contains("deeply nested"));
    }
}
