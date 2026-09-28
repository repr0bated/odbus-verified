//! Event-driven, selected-column OVSDB topology observation.
//!
//! The native JSON-RPC transport and IDL remain upstream implementations. Its
//! ClientConfig cannot select columns, so this small monitor driver requests
//! only topology data: options, other_config, external_ids and SSL tables are
//! never requested. RFC 7047 monitor V1 is deliberate: the vendor IDL accepts
//! old/new row updates, not update2/update3 difference semantics.
//!
//! LIMIT: upstream Connection has an unbounded private receive buffer. The
//! handshake deadline and post-parse size checks below do not claim to bound
//! allocation before upstream JSON parsing. There is no configuration hook.

use anyhow::{bail, ensure, Context, Result};
use rovs_jsonrpc::{Connection, Message, Request, Response, RpcId};
use rovs_ovsdb::{DbSchema, Idl};
use rovs_transport::{Address, Stream};
use serde_json::{json, Value};
use std::{collections::BTreeMap, io::Write, sync::Arc, time::Duration};
use tokio::sync::{oneshot, watch};
use uuid::Uuid;

pub const MAX_SNAPSHOT_ROWS: usize = 16_384;
const MAX_JSON_BYTES: usize = 16 * 1024 * 1024;
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(15);
const CONTROL_TIMEOUT: Duration = Duration::from_secs(15);
const MAX_MONITOR_REQUESTS: usize = 64;
const MAX_CONDITIONS: usize = 256;

// Backing-protocol column translation, not a second plugin schema. The
// plugin-owned OvsBridgeState and its sealed exposure policy own wire views.
const COLUMNS: &[(&str, &[&str])] = &[
    ("Open_vSwitch", &["bridges"]),
    (
        "Bridge",
        &[
            "name",
            "ports",
            "datapath_type",
            "fail_mode",
            "stp_enable",
            "mcast_snooping_enable",
        ],
    ),
    (
        "Port",
        &[
            "name",
            "interfaces",
            "tag",
            "trunks",
            "vlan_mode",
            "bond_mode",
        ],
    ),
    (
        "Interface",
        &[
            "name",
            "type",
            "mac_in_use",
            "mac",
            "admin_state",
            "link_state",
        ],
    ),
];

pub type OvsdbRow = BTreeMap<String, Value>;

/// Full selected-table contents at one native monitor revision. It is an
/// internal observation, never a public state payload or existence authority.
#[derive(Clone)]
pub struct OvsdbSnapshot {
    pub sequence: u64,
    pub tables: BTreeMap<String, BTreeMap<Uuid, OvsdbRow>>,
}

#[derive(Clone)]
pub enum MonitorObservation {
    Snapshot(Arc<OvsdbSnapshot>),
    /// Terminal failure. Consumers must preserve the last committed topology,
    /// report unavailable, and never manufacture an empty database snapshot.
    Unavailable(Arc<str>),
}

/// The initial monitor reply is available immediately through borrow(). Slow
/// consumers use borrow_and_update() after changed(): watch retains the latest
/// complete snapshot. There is no delta loss or application polling timer.
pub async fn subscribe(
    address: &str,
    database: &str,
) -> Result<watch::Receiver<MonitorObservation>> {
    ensure!(
        database == "Open_vSwitch",
        "topology monitor only supports Open_vSwitch"
    );
    let (mut connection, mut idl, monitor_id) =
        tokio::time::timeout(HANDSHAKE_TIMEOUT, connect(address, database))
            .await
            .context("OVSDB monitor handshake timed out")??;
    let initial = snapshot(&idl, 1)?;
    let (sender, receiver) = watch::channel(MonitorObservation::Snapshot(Arc::new(initial)));
    tokio::spawn(async move {
        let mut sequence = 1u64;
        loop {
            let result = tokio::select! {
                biased;
                _ = sender.closed() => return,
                result = next_update(&mut connection, &mut idl, &monitor_id, sequence) => result,
            };
            match result {
                Ok(next) => {
                    sequence = next.sequence;
                    sender.send_replace(MonitorObservation::Snapshot(Arc::new(next)));
                }
                Err(error) => {
                    // Do not include database values in errors or debug logs.
                    sender.send_replace(MonitorObservation::Unavailable(Arc::from(
                        error.to_string(),
                    )));
                    return;
                }
            }
        }
    });
    Ok(receiver)
}

async fn connect(address: &str, database: &str) -> Result<(Connection, Idl, String)> {
    let address: Address = address.parse().context("invalid OVSDB transport address")?;
    ensure!(
        matches!(address, Address::Unix(_)),
        "topology observation requires the configured local OVSDB Unix socket"
    );
    let mut connection = Connection::new(Stream::connect(&address).await?);
    let raw_schema = request(&mut connection, 1, "get_schema", json!([database])).await?;
    let schema = DbSchema::from_json(&raw_schema).context("invalid native OVSDB schema")?;
    ensure!(
        schema.name == database,
        "OVSDB replied with a different database schema"
    );
    let mut requests = serde_json::Map::new();
    for (table, columns) in COLUMNS {
        let table_schema = schema
            .table(table)
            .context("required topology table is absent")?;
        for column in *columns {
            ensure!(
                table_schema.column(column).is_some(),
                "required topology column is absent"
            );
        }
        requests.insert((*table).to_owned(), json!({"columns": columns}));
    }
    let monitor_id = Uuid::new_v4().to_string();
    let initial = request(
        &mut connection,
        2,
        "monitor",
        json!([database, monitor_id, requests]),
    )
    .await?;
    validate_update(&initial, true)?;
    let mut idl = Idl::new();
    idl.set_schema(schema);
    idl.process_update(&initial);
    idl.set_monitoring();
    Ok((connection, idl, monitor_id))
}

async fn request(
    connection: &mut Connection,
    id: u64,
    method: &str,
    params: Value,
) -> Result<Value> {
    connection
        .send_message(&Message::Request(Request::new(method, params, id)))
        .await?;
    loop {
        let message = connection.recv_message().await?;
        bounded_json(&message)?;
        match message {
            Message::Response(response) => {
                ensure!(
                    response.id == RpcId::Number(id),
                    "unexpected OVSDB response ID"
                );
                ensure!(
                    response.error.is_none(),
                    "OVSDB rejected monitor handshake request"
                );
                return response.result.context("OVSDB response omitted result");
            }
            Message::Request(request) if request.method == "echo" => {
                echo(connection, request).await?
            }
            Message::Request(_) => bail!("unexpected notification during OVSDB handshake"),
        }
    }
}

async fn echo(connection: &mut Connection, request: Request) -> Result<()> {
    ensure!(request.params.is_array(), "invalid OVSDB echo parameters");
    if let Some(id) = request.id {
        connection
            .send_message(&Message::Response(Response::success(id, request.params)))
            .await?;
    }
    Ok(())
}

async fn next_update(
    connection: &mut Connection,
    idl: &mut Idl,
    monitor_id: &str,
    sequence: u64,
) -> Result<OvsdbSnapshot> {
    loop {
        let message = connection.recv_message().await?;
        bounded_json(&message)?;
        match message {
            Message::Request(request) if request.method == "echo" => {
                echo(connection, request).await?
            }
            Message::Request(request) if request.method == "update" => {
                ensure!(request.id.is_none(), "OVSDB update must be a notification");
                let params = request
                    .params
                    .as_array()
                    .context("invalid OVSDB update parameters")?;
                ensure!(
                    params.len() == 2 && params[0].as_str() == Some(monitor_id),
                    "OVSDB update has wrong subscription ID or shape"
                );
                validate_update(&params[1], false)?;
                let next = sequence
                    .checked_add(1)
                    .context("OVSDB observation sequence exhausted")?;
                // Check before the vendor IDL's unchecked sequence increment.
                ensure!(
                    idl.change_seqno() < u64::MAX,
                    "OVSDB IDL sequence exhausted"
                );
                idl.process_update(&params[1]);
                return snapshot(idl, next);
            }
            Message::Request(_) => {
                bail!("unexpected OVSDB monitor notification or protocol version")
            }
            Message::Response(_) => bail!("unexpected response on OVSDB monitor connection"),
        }
    }
}

fn validate_update(value: &Value, initial: bool) -> Result<()> {
    bounded_json(value)?;
    let tables = value
        .as_object()
        .context("OVSDB table updates must be an object")?;
    let mut rows = 0usize;
    for (table, changes) in tables {
        let columns = COLUMNS
            .iter()
            .find_map(|(name, columns)| (*name == table).then_some(*columns))
            .context("OVSDB sent an unrequested table")?;
        for (id, change) in changes
            .as_object()
            .context("OVSDB table update must be an object")?
        {
            id.parse::<Uuid>().context("invalid OVSDB row UUID")?;
            rows += 1;
            ensure!(rows <= MAX_SNAPSHOT_ROWS, "OVSDB update exceeds row limit");
            let change = change
                .as_object()
                .context("OVSDB row update must be an object")?;
            ensure!(
                !change.is_empty()
                    && change
                        .keys()
                        .all(|key| matches!(key.as_str(), "old" | "new")),
                "invalid OVSDB row update operation"
            );
            ensure!(
                !initial || (change.contains_key("new") && !change.contains_key("old")),
                "initial OVSDB rows must be insertions"
            );
            for row in change.values() {
                let row = row.as_object().context("OVSDB row must be an object")?;
                ensure!(
                    row.keys().all(|key| columns.contains(&key.as_str())),
                    "OVSDB sent an unrequested column"
                );
            }
            if initial || !change.contains_key("old") {
                let new = change
                    .get("new")
                    .and_then(Value::as_object)
                    .context("OVSDB insertion omitted new row")?;
                ensure!(
                    columns.iter().all(|column| new.contains_key(*column)),
                    "OVSDB insertion omitted a monitored column"
                );
            }
        }
    }
    Ok(())
}

fn snapshot(idl: &Idl, sequence: u64) -> Result<OvsdbSnapshot> {
    let mut tables = BTreeMap::new();
    let mut count = 0usize;
    for (name, columns) in COLUMNS {
        let mut rows = BTreeMap::new();
        for row in idl.rows(name) {
            count += 1;
            ensure!(
                count <= MAX_SNAPSHOT_ROWS,
                "OVSDB snapshot exceeds row limit"
            );
            let mut values = BTreeMap::new();
            for column in *columns {
                values.insert(
                    (*column).to_owned(),
                    row.get(column)
                        .context("IDL snapshot omitted monitored column")?
                        .clone(),
                );
            }
            rows.insert(row.uuid, values);
        }
        tables.insert((*name).to_owned(), rows);
    }
    bounded_json(&tables)?;
    Ok(OvsdbSnapshot { sequence, tables })
}

fn bounded_json(value: &impl serde::Serialize) -> Result<()> {
    struct Limit(usize);
    impl Write for Limit {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0 = self
                .0
                .checked_add(bytes.len())
                .ok_or_else(|| std::io::Error::other("JSON size overflow"))?;
            if self.0 > MAX_JSON_BYTES {
                return Err(std::io::Error::other("OVSDB JSON exceeds 16 MiB"));
            }
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    serde_json::to_writer(Limit(0), value).context("OVSDB parsed message exceeds size bound")
}

// -------------------------------------------------------------------------
// Caller-selected protocol monitors. These are separate from the canonical
// full-topology observer above: a filtered view must never replace the full
// Bridge -> Port -> Interface state tree.

pub type ProtocolRow = BTreeMap<String, Value>;
pub type ProtocolTables = BTreeMap<String, BTreeMap<String, ProtocolRow>>;

#[derive(Clone, Debug, serde::Serialize)]
pub struct ProtocolSnapshot {
    pub monitor_id: String,
    pub method: String,
    pub sequence: u64,
    pub tables: ProtocolTables,
}

#[derive(Clone)]
pub enum ProtocolObservation {
    Snapshot(Arc<ProtocolSnapshot>),
    Unavailable(Arc<str>),
}

struct CancelCommand {
    reply: oneshot::Sender<std::result::Result<Value, String>>,
}

pub struct MonitorSubscription {
    pub receiver: watch::Receiver<ProtocolObservation>,
    cancel: Option<oneshot::Sender<CancelCommand>>,
}

impl MonitorSubscription {
    pub fn is_active(&self) -> bool {
        self.cancel
            .as_ref()
            .is_some_and(|cancel| !cancel.is_closed())
    }

    /// Send monitor_cancel on the connection that owns the monitor and wait
    /// for ovsdb-server's reply. Dropping a receiver is not a protocol cancel.
    pub async fn cancel(mut self) -> Result<Value> {
        let cancel = self
            .cancel
            .take()
            .context("OVSDB monitor cancellation is already in progress")?;
        let (reply, result) = oneshot::channel();
        cancel
            .send(CancelCommand { reply })
            .map_err(|_| anyhow::anyhow!("OVSDB monitor task already ended"))?;
        result
            .await
            .context("OVSDB monitor task ended without a cancellation receipt")?
            .map_err(anyhow::Error::msg)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ProtocolVersion {
    V1,
    V2,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ColumnKind {
    Scalar,
    Set,
    Map,
}

#[derive(Clone)]
struct ProtocolSelection {
    columns: BTreeMap<String, BTreeMap<String, ColumnKind>>,
}

/// Start a bounded RFC monitor or Open vSwitch monitor_cond session. The
/// request is forwarded exactly after validating the plugin's selected safe
/// topology surface. Results are materialized as current rows for SHM.
pub async fn subscribe_protocol(
    address: &str,
    database: &str,
    monitor_id: &str,
    method: &str,
    requests: Value,
) -> Result<MonitorSubscription> {
    ensure!(database == "Open_vSwitch", "unsupported OVSDB database");
    validate_identifier(monitor_id, "monitor ID")?;
    let version = match method {
        "monitor" => ProtocolVersion::V1,
        "monitor_cond" => ProtocolVersion::V2,
        _ => bail!("unsupported OVSDB monitor method"),
    };
    bounded_json(&requests)?;
    let (mut connection, selection, initial) = tokio::time::timeout(
        HANDSHAKE_TIMEOUT,
        connect_protocol(address, database, monitor_id, method, requests, version),
    )
    .await
    .context("OVSDB protocol monitor handshake timed out")??;
    let mut snapshot = ProtocolSnapshot {
        monitor_id: monitor_id.to_owned(),
        method: method.to_owned(),
        sequence: 1,
        tables: selection
            .columns
            .keys()
            .map(|table| (table.clone(), BTreeMap::new()))
            .collect(),
    };
    apply_protocol_update(&mut snapshot, &selection, version, &initial, true)?;
    let (sender, receiver) =
        watch::channel(ProtocolObservation::Snapshot(Arc::new(snapshot.clone())));
    let (cancel, mut cancellation) = oneshot::channel::<CancelCommand>();
    let monitor_id_owned = monitor_id.to_owned();
    tokio::spawn(async move {
        loop {
            tokio::select! {
                biased;
                command = &mut cancellation => {
                    let Ok(command) = command else { return; };
                    let result = cancel_protocol_monitor(
                        &mut connection,
                        &monitor_id_owned,
                        version,
                        &selection,
                        &mut snapshot,
                        &sender,
                    ).await;
                    if let Err(error) = &result {
                        sender.send_replace(ProtocolObservation::Unavailable(Arc::from(error.to_string())));
                    }
                    let _ = command.reply.send(result.map_err(|error| error.to_string()));
                    return;
                }
                _ = sender.closed() => return,
                result = next_protocol_update(
                    &mut connection,
                    &monitor_id_owned,
                    version,
                    &selection,
                    &mut snapshot,
                ) => {
                    match result {
                        Ok(()) => sender.send_replace(ProtocolObservation::Snapshot(Arc::new(snapshot.clone()))),
                        Err(error) => {
                            sender.send_replace(ProtocolObservation::Unavailable(Arc::from(error.to_string())));
                            return;
                        }
                    };
                }
            }
        }
    });
    Ok(MonitorSubscription {
        receiver,
        cancel: Some(cancel),
    })
}

async fn connect_protocol(
    address: &str,
    database: &str,
    monitor_id: &str,
    method: &str,
    requests: Value,
    version: ProtocolVersion,
) -> Result<(Connection, ProtocolSelection, Value)> {
    let address: Address = address.parse().context("invalid OVSDB transport address")?;
    ensure!(
        matches!(address, Address::Unix(_)),
        "OVSDB protocol monitors require the configured local Unix socket"
    );
    let mut connection = Connection::new(Stream::connect(&address).await?);
    let schema_id = connection.next_id();
    let raw_schema = request(&mut connection, schema_id, "get_schema", json!([database])).await?;
    let selection = validate_protocol_requests(&raw_schema, &requests, version)?;
    let monitor_request_id = connection.next_id();
    let initial = request(
        &mut connection,
        monitor_request_id,
        method,
        json!([database, monitor_id, requests]),
    )
    .await?;
    Ok((connection, selection, initial))
}

fn validate_identifier(value: &str, label: &str) -> Result<()> {
    ensure!(
        !value.is_empty()
            && value.len() <= 128
            && value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.')),
        "invalid OVSDB {label}"
    );
    Ok(())
}

fn validate_protocol_requests(
    raw_schema: &Value,
    requests: &Value,
    version: ProtocolVersion,
) -> Result<ProtocolSelection> {
    let schema_tables = raw_schema
        .get("tables")
        .and_then(Value::as_object)
        .context("OVSDB schema omitted tables")?;
    let tables = requests
        .as_object()
        .context("OVSDB monitor requests must be an object")?;
    ensure!(!tables.is_empty(), "OVSDB monitor requests are empty");
    ensure!(
        tables.len() <= COLUMNS.len(),
        "too many OVSDB monitor tables"
    );
    let mut request_count = 0usize;
    let mut selected = BTreeMap::new();
    for (table, groups) in tables {
        let allowed = COLUMNS
            .iter()
            .find_map(|(name, columns)| (*name == table).then_some(*columns))
            .context("OVSDB monitor requested a table outside the selected topology surface")?;
        let schema_columns = schema_tables
            .get(table)
            .and_then(|table| table.get("columns"))
            .and_then(Value::as_object)
            .context("OVSDB schema omitted a requested table")?;
        let groups = groups
            .as_array()
            .context("each OVSDB monitor table must contain an array of requests")?;
        ensure!(!groups.is_empty(), "OVSDB monitor request array is empty");
        request_count = request_count
            .checked_add(groups.len())
            .context("OVSDB monitor request count overflow")?;
        ensure!(
            request_count <= MAX_MONITOR_REQUESTS,
            "too many OVSDB monitor requests"
        );
        let mut table_columns = BTreeMap::new();
        for group in groups {
            let group = group
                .as_object()
                .context("OVSDB monitor request must be an object")?;
            ensure!(
                group
                    .keys()
                    .all(|key| matches!(key.as_str(), "columns" | "where" | "select")),
                "OVSDB monitor request contains an unsupported member"
            );
            let columns = group
                .get("columns")
                .and_then(Value::as_array)
                .context("OVSDB monitor requests must explicitly select safe columns")?;
            ensure!(!columns.is_empty(), "OVSDB monitor selected no columns");
            for column in columns {
                let column = column
                    .as_str()
                    .context("OVSDB monitor column must be text")?;
                ensure!(
                    allowed.contains(&column),
                    "OVSDB monitor requested a non-public topology column"
                );
                let definition = schema_columns
                    .get(column)
                    .context("OVSDB schema omitted a requested column")?;
                ensure!(
                    table_columns
                        .insert(column.to_owned(), column_kind(definition)?)
                        .is_none(),
                    "OVSDB monitor selected a column more than once"
                );
            }
            if let Some(conditions) = group.get("where") {
                ensure!(
                    version == ProtocolVersion::V2,
                    "monitor does not accept where conditions"
                );
                validate_conditions(conditions, allowed)?;
            }
            if let Some(select) = group.get("select") {
                let select = select
                    .as_object()
                    .context("OVSDB monitor select must be an object")?;
                ensure!(
                    select.keys().all(|key| matches!(
                        key.as_str(),
                        "initial" | "insert" | "delete" | "modify"
                    )) && select.values().all(Value::is_boolean),
                    "invalid OVSDB monitor select object"
                );
            }
        }
        selected.insert(table.clone(), table_columns);
    }
    Ok(ProtocolSelection { columns: selected })
}

fn validate_conditions(value: &Value, allowed: &[&str]) -> Result<()> {
    let conditions = value
        .as_array()
        .context("OVSDB monitor where must be an array")?;
    ensure!(
        conditions.len() <= MAX_CONDITIONS,
        "too many OVSDB monitor conditions"
    );
    for condition in conditions {
        if condition.is_boolean() {
            continue;
        }
        let condition = condition
            .as_array()
            .context("OVSDB monitor condition must be boolean or a three-element array")?;
        ensure!(
            condition.len() == 3,
            "OVSDB monitor condition must have three elements"
        );
        let column = condition[0]
            .as_str()
            .context("OVSDB monitor condition column must be text")?;
        ensure!(
            allowed.contains(&column),
            "OVSDB monitor condition uses a non-public topology column"
        );
        ensure!(
            matches!(
                condition[1].as_str(),
                Some("==" | "!=" | "<" | "<=" | ">" | ">=" | "includes" | "excludes")
            ),
            "unsupported OVSDB monitor condition function"
        );
    }
    Ok(())
}

fn column_kind(definition: &Value) -> Result<ColumnKind> {
    let type_ = definition
        .get("type")
        .context("OVSDB column omitted type")?;
    let Some(type_) = type_.as_object() else {
        return Ok(ColumnKind::Scalar);
    };
    if type_.contains_key("value") {
        return Ok(ColumnKind::Map);
    }
    let min = type_.get("min").and_then(Value::as_u64).unwrap_or(1);
    let max_one = type_.get("max").and_then(Value::as_u64) == Some(1);
    Ok(if min == 1 && max_one {
        ColumnKind::Scalar
    } else {
        ColumnKind::Set
    })
}

async fn next_protocol_update(
    connection: &mut Connection,
    monitor_id: &str,
    version: ProtocolVersion,
    selection: &ProtocolSelection,
    snapshot: &mut ProtocolSnapshot,
) -> Result<()> {
    loop {
        let message = connection.recv_message().await?;
        bounded_json(&message)?;
        match message {
            Message::Request(request) if request.method == "echo" => {
                echo(connection, request).await?
            }
            Message::Request(request)
                if request.method
                    == if version == ProtocolVersion::V1 {
                        "update"
                    } else {
                        "update2"
                    } =>
            {
                ensure!(request.id.is_none(), "OVSDB update must be a notification");
                let params = request
                    .params
                    .as_array()
                    .context("invalid OVSDB update parameters")?;
                ensure!(
                    params.len() == 2 && params[0].as_str() == Some(monitor_id),
                    "OVSDB update has wrong monitor ID or shape"
                );
                snapshot.sequence = snapshot
                    .sequence
                    .checked_add(1)
                    .context("OVSDB monitor sequence exhausted")?;
                apply_protocol_update(snapshot, selection, version, &params[1], false)?;
                return Ok(());
            }
            Message::Request(request) if request.method == "monitor_canceled" => {
                ensure!(
                    request.id.is_none() && request.params == json!([monitor_id]),
                    "invalid OVSDB monitor_canceled notification"
                );
                bail!("OVSDB server canceled the monitor")
            }
            Message::Request(_) => bail!("unexpected OVSDB monitor notification"),
            Message::Response(_) => bail!("unexpected OVSDB monitor response"),
        }
    }
}

async fn cancel_protocol_monitor(
    connection: &mut Connection,
    monitor_id: &str,
    version: ProtocolVersion,
    selection: &ProtocolSelection,
    snapshot: &mut ProtocolSnapshot,
    sender: &watch::Sender<ProtocolObservation>,
) -> Result<Value> {
    let result = tokio::time::timeout(
        CONTROL_TIMEOUT,
        connection.transact("monitor_cancel", json!([monitor_id])),
    )
    .await
    .context("OVSDB monitor_cancel timed out")?
    .map_err(anyhow::Error::from)
    .context("OVSDB monitor_cancel failed")?;
    ensure!(
        result.as_object().is_some_and(serde_json::Map::is_empty),
        "OVSDB monitor_cancel returned an invalid result"
    );
    let pending = connection.drain_notifications().collect::<Vec<_>>();
    ensure!(
        pending.len() <= MAX_MONITOR_REQUESTS,
        "too many notifications preceded monitor_cancel"
    );
    for request in pending {
        if request.method == "echo" {
            echo(connection, request).await?;
            continue;
        }
        let expected = if version == ProtocolVersion::V1 {
            "update"
        } else {
            "update2"
        };
        ensure!(
            request.method == expected,
            "unexpected notification before monitor_cancel reply"
        );
        let params = request
            .params
            .as_array()
            .context("invalid OVSDB update parameters")?;
        ensure!(
            params.len() == 2 && params[0].as_str() == Some(monitor_id),
            "wrong monitor update before cancellation"
        );
        snapshot.sequence = snapshot
            .sequence
            .checked_add(1)
            .context("OVSDB monitor sequence exhausted")?;
        apply_protocol_update(snapshot, selection, version, &params[1], false)?;
        sender.send_replace(ProtocolObservation::Snapshot(Arc::new(snapshot.clone())));
    }
    Ok(result)
}

fn apply_protocol_update(
    snapshot: &mut ProtocolSnapshot,
    selection: &ProtocolSelection,
    version: ProtocolVersion,
    update: &Value,
    initial: bool,
) -> Result<()> {
    bounded_json(update)?;
    let tables = update
        .as_object()
        .context("OVSDB table updates must be an object")?;
    for (table, rows) in tables {
        let columns = selection
            .columns
            .get(table)
            .context("OVSDB sent an unrequested table")?;
        let target = snapshot
            .tables
            .get_mut(table)
            .context("OVSDB snapshot omitted requested table")?;
        for (uuid, change) in rows
            .as_object()
            .context("OVSDB table update must be an object")?
        {
            uuid.parse::<Uuid>().context("invalid OVSDB row UUID")?;
            let change = change
                .as_object()
                .context("OVSDB row update must be an object")?;
            match version {
                ProtocolVersion::V1 => apply_v1_row(target, uuid, columns, change, initial)?,
                ProtocolVersion::V2 => apply_v2_row(target, uuid, columns, change, initial)?,
            }
        }
    }
    let rows = snapshot.tables.values().try_fold(0usize, |count, table| {
        count
            .checked_add(table.len())
            .context("OVSDB row count overflow")
    })?;
    ensure!(
        rows <= MAX_SNAPSHOT_ROWS,
        "OVSDB monitor snapshot exceeds row limit"
    );
    bounded_json(&snapshot.tables)
}

fn checked_row(row: &Value, columns: &BTreeMap<String, ColumnKind>) -> Result<ProtocolRow> {
    let row = row
        .as_object()
        .context("OVSDB monitored row must be an object")?;
    ensure!(
        row.keys().all(|column| columns.contains_key(column)),
        "OVSDB sent an unrequested column"
    );
    Ok(row
        .iter()
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect())
}

fn apply_v1_row(
    rows: &mut BTreeMap<String, ProtocolRow>,
    uuid: &str,
    columns: &BTreeMap<String, ColumnKind>,
    change: &serde_json::Map<String, Value>,
    initial: bool,
) -> Result<()> {
    ensure!(
        !change.is_empty()
            && change
                .keys()
                .all(|key| matches!(key.as_str(), "old" | "new")),
        "invalid OVSDB update row"
    );
    if initial {
        ensure!(
            change.len() == 1 && change.contains_key("new"),
            "invalid initial OVSDB update row"
        );
    }
    match change.get("new") {
        Some(new) => {
            let new = checked_row(new, columns)?;
            if change.contains_key("old") {
                let current = rows
                    .get_mut(uuid)
                    .context("OVSDB modified an unknown row")?;
                current.extend(new);
            } else {
                ensure!(
                    rows.insert(uuid.to_owned(), new).is_none(),
                    "OVSDB inserted an existing row"
                );
            }
        }
        None => {
            checked_row(
                change
                    .get("old")
                    .context("OVSDB deletion omitted old row")?,
                columns,
            )?;
            ensure!(rows.remove(uuid).is_some(), "OVSDB deleted an unknown row");
        }
    }
    Ok(())
}

fn apply_v2_row(
    rows: &mut BTreeMap<String, ProtocolRow>,
    uuid: &str,
    columns: &BTreeMap<String, ColumnKind>,
    change: &serde_json::Map<String, Value>,
    initial_reply: bool,
) -> Result<()> {
    ensure!(
        change.len() == 1,
        "OVSDB update2 row must contain one operation"
    );
    let (operation, value) = change.iter().next().expect("checked one operation");
    match operation.as_str() {
        "initial" => {
            ensure!(
                initial_reply,
                "initial update2 appeared after the monitor reply"
            );
            let row = checked_row(value, columns)?;
            ensure!(
                rows.insert(uuid.to_owned(), row).is_none(),
                "OVSDB initialized an existing row"
            );
        }
        "insert" => {
            ensure!(!initial_reply, "insert appeared in initial update2 reply");
            let row = checked_row(value, columns)?;
            ensure!(
                rows.insert(uuid.to_owned(), row).is_none(),
                "OVSDB inserted an existing row"
            );
        }
        "delete" => {
            // ovsdb-server emits `{"delete": null}` in update2/update3
            // notifications (ovsdb/monitor.c composes a JSON null); an empty
            // row object is the RFC 7047 spelling. Accept both.
            ensure!(
                !initial_reply
                    && (value.is_null()
                        || value.as_object().is_some_and(serde_json::Map::is_empty)),
                "invalid update2 delete row"
            );
            ensure!(rows.remove(uuid).is_some(), "OVSDB deleted an unknown row");
        }
        "modify" => {
            ensure!(!initial_reply, "modify appeared in initial update2 reply");
            let difference = checked_row(value, columns)?;
            let row = rows
                .get_mut(uuid)
                .context("OVSDB modified an unknown row")?;
            for (column, difference) in difference {
                apply_difference(row, &column, columns[&column], difference)?;
            }
        }
        _ => bail!("unknown OVSDB update2 row operation"),
    }
    Ok(())
}

fn apply_difference(
    row: &mut ProtocolRow,
    column: &str,
    kind: ColumnKind,
    difference: Value,
) -> Result<()> {
    match kind {
        ColumnKind::Scalar => {
            row.insert(column.to_owned(), difference);
        }
        ColumnKind::Set => {
            let mut current = decode_set(row.get(column))?;
            for item in decode_set(Some(&difference))? {
                if let Some(index) = current.iter().position(|existing| existing == &item) {
                    current.remove(index);
                } else {
                    current.push(item);
                }
            }
            row.insert(column.to_owned(), json!(["set", current]));
        }
        ColumnKind::Map => {
            let mut current = decode_map(row.get(column))?;
            for (key, value) in decode_map(Some(&difference))? {
                if let Some(index) = current.iter().position(|(existing, _)| existing == &key) {
                    if current[index].1 == value {
                        current.remove(index);
                    } else {
                        current[index].1 = value;
                    }
                } else {
                    current.push((key, value));
                }
            }
            let pairs = current
                .into_iter()
                .map(|(key, value)| json!([key, value]))
                .collect::<Vec<_>>();
            row.insert(column.to_owned(), json!(["map", pairs]));
        }
    }
    Ok(())
}

fn decode_set(value: Option<&Value>) -> Result<Vec<Value>> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    if let Some(encoded) = value.as_array()
        && encoded.first().and_then(Value::as_str) == Some("set")
    {
        ensure!(encoded.len() == 2, "invalid OVSDB set encoding");
        return Ok(encoded[1]
            .as_array()
            .context("invalid OVSDB set members")?
            .clone());
    }
    Ok(vec![value.clone()])
}

fn decode_map(value: Option<&Value>) -> Result<Vec<(Value, Value)>> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let encoded = value.as_array().context("invalid OVSDB map encoding")?;
    ensure!(
        encoded.len() == 2 && encoded[0].as_str() == Some("map"),
        "invalid OVSDB map encoding"
    );
    encoded[1]
        .as_array()
        .context("invalid OVSDB map entries")?
        .iter()
        .map(|pair| {
            let pair = pair.as_array().context("invalid OVSDB map pair")?;
            ensure!(pair.len() == 2, "invalid OVSDB map pair");
            Ok((pair[0].clone(), pair[1].clone()))
        })
        .collect()
}

#[cfg(test)]
#[path = "ovsdb_monitor_tests.rs"]
mod tests;
