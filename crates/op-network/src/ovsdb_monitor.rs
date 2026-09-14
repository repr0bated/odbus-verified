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
use tokio::sync::watch;
use uuid::Uuid;

pub const MAX_SNAPSHOT_ROWS: usize = 16_384;
const MAX_JSON_BYTES: usize = 16 * 1024 * 1024;
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(15);

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

#[cfg(test)]
#[path = "ovsdb_monitor_tests.rs"]
mod tests;
