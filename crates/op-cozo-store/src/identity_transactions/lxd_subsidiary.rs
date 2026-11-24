//! Durable creation intent for one LXD subsidiary per session. The stored
//! request fixes its name, and LXD names are unique, so replaying the same
//! intent can never create a second container.
use super::*;

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LxdSubsidiary {
    pub session_id: String,
    pub name: String,
    pub wireguard_pubkey: String,
    pub actor_id: String,
    pub caller_session: String,
    pub caller_genesis: String,
    /// The exact admitted `create_instance` instance body.
    pub request_json: String,
    /// Empty until the live instance is verified; immutable afterwards.
    pub native_uuid: String,
}

fn validate(row: &LxdSubsidiary) -> std::result::Result<(), CozoError> {
    let body: Value = serde_json::from_str(&row.request_json)?;
    if op_identity::session::derive_session_id(&row.wireguard_pubkey) != row.session_id
        || row.session_id == row.caller_session
        || row.caller_genesis.is_empty()
        || row.name.is_empty()
        || row.request_json.len() > 32768
        || body["name"] != row.name
        || body["config"]["user.opdbus.session_id"] != row.session_id
        || body["config"]["user.opdbus.wireguard_public_key"] != row.wireguard_pubkey
        || body["config"]["user.opdbus.parent_principal_id"] != row.actor_id
        || !(row.native_uuid.is_empty()
            || uuid::Uuid::parse_str(&row.native_uuid)
                .is_ok_and(|id| id.to_string() == row.native_uuid))
    {
        return Err(err("invalid LXD subsidiary binding"));
    }
    Ok(())
}

fn rows(tx: &MultiTransaction) -> std::result::Result<Vec<LxdSubsidiary>, CozoError> {
    query(
        tx,
        "?[document,session_id] := *identity_lxd_subsidiaries{document,session_id}",
        Params::new(),
    )?
    .rows
    .iter()
    .map(|row| {
        let record: LxdSubsidiary = serde_json::from_str(&string(row, 0)?)?;
        validate(&record)?;
        if string(row, 1)? != record.session_id {
            return Err(err("corrupt LXD subsidiary journal"));
        }
        Ok(record)
    })
    .collect()
}

fn put(tx: &MultiTransaction, row: &LxdSubsidiary) -> std::result::Result<(), CozoError> {
    query(
        tx,
        "?[session_id,document] <- [[$sid,$doc]] :put identity_lxd_subsidiaries {session_id => document}",
        BTreeMap::from([
            ("sid".into(), DataValue::Str(row.session_id.as_str().into())),
            ("doc".into(), DataValue::Str(serde_json::to_string(row)?.into())),
        ]),
    )?;
    Ok(())
}

impl CozoGraphShuttle {
    pub fn lxd_subsidiaries(&self) -> std::result::Result<Vec<LxdSubsidiary>, CozoError> {
        let _guard = self.identity_transaction.lock().map_err(|_| err("identity owner poisoned"))?;
        let tx = self.db.multi_transaction(false);
        let rows = rows(&tx)?;
        commit_checked(&tx)?;
        Ok(rows)
    }

    /// Record the parent-authorized intent before any LXD request.
    pub fn reserve_lxd_subsidiary(&self, row: LxdSubsidiary) -> std::result::Result<(), CozoError> {
        validate(&row)?;
        if !row.native_uuid.is_empty() {
            return Err(err("a new LXD subsidiary has no native UUID"));
        }
        let _guard = self.identity_transaction.lock().map_err(|_| err("identity owner poisoned"))?;
        let tx = self.db.multi_transaction(true);
        for old in rows(&tx)? {
            if old.session_id == row.session_id {
                let same = LxdSubsidiary { native_uuid: String::new(), ..old.clone() } == row;
                if !same || !old.native_uuid.is_empty() {
                    return Err(err("LXD subsidiary already exists with another intent or UUID"));
                }
                commit_checked(&tx)?;
                return Ok(());
            }
            if old.name == row.name {
                return Err(err("LXD subsidiary name belongs to another identity"));
            }
        }
        let caller = read_one(&tx, &row.caller_session)?.0.record
            .ok_or_else(|| err("subsidiary parent absent"))?;
        let target = read_one(&tx, &row.session_id)?.0;
        if caller.principal_kind.as_deref() != Some("human") || !caller.active
            || caller.session_genesis != row.caller_genesis
            || op_identity::session::derive_principal_id(&caller.wireguard_pubkey) != row.actor_id
            || target.revision != 0 || target.record.is_some() || target.genesis.is_some()
        {
            return Err(err("LXD subsidiary needs its live human parent and a fresh identity"));
        }
        put(&tx, &row)?;
        commit_checked(&tx)?;
        self.sync_identity_store()
    }

    /// Supersede an intent whose container the caller has proved absent. The
    /// parent, key, session and name stay fixed; only the request changes, and
    /// the next admission pins the new native UUID.
    pub fn replace_lxd_subsidiary(&self, old: &LxdSubsidiary, row: LxdSubsidiary) -> std::result::Result<(), CozoError> {
        validate(&row)?;
        let same = LxdSubsidiary {
            request_json: row.request_json.clone(), native_uuid: String::new(), ..old.clone()
        } == row;
        if !same {
            return Err(err("a replacement keeps its parent, key, session and name"));
        }
        let _guard = self.identity_transaction.lock().map_err(|_| err("identity owner poisoned"))?;
        let tx = self.db.multi_transaction(true);
        if !rows(&tx)?.contains(old) {
            return Err(err("LXD subsidiary intent changed"));
        }
        let caller = read_one(&tx, &row.caller_session)?.0.record
            .ok_or_else(|| err("subsidiary parent absent"))?;
        if caller.principal_kind.as_deref() != Some("human") || !caller.active
            || caller.session_genesis != row.caller_genesis
            || op_identity::session::derive_principal_id(&caller.wireguard_pubkey) != row.actor_id
        {
            return Err(err("LXD subsidiary needs its live human parent"));
        }
        put(&tx, &row)?;
        commit_checked(&tx)?;
        self.sync_identity_store()
    }

    /// Pin the verified native UUID once; a different UUID is never accepted.
    pub fn bind_lxd_subsidiary(&self, session: &str, uuid: &str) -> std::result::Result<(), CozoError> {
        let _guard = self.identity_transaction.lock().map_err(|_| err("identity owner poisoned"))?;
        let tx = self.db.multi_transaction(true);
        let mut row = rows(&tx)?.into_iter().find(|row| row.session_id == session)
            .ok_or_else(|| err("LXD subsidiary intent absent"))?;
        if row.native_uuid != uuid {
            if !row.native_uuid.is_empty() {
                return Err(err("LXD subsidiary native UUID is immutable"));
            }
            row.native_uuid = uuid.into();
            validate(&row)?;
            put(&tx, &row)?;
        }
        commit_checked(&tx)?;
        self.sync_identity_store()
    }
}
