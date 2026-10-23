//! One-use operation admission on the existing system bus. Service ownership
//! authenticates the callback channel; the closure retains the actual caller.

use anyhow::{Context, Result, ensure};
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use uuid::Uuid;
use zbus::{Connection, message::Header};

pub const BRIDGE_SERVICE: &str = "org.opdbus.v1.routing_admission";
pub const CONTROLLER_SERVICE: &str = "org.opdbus.v1.plugins.openflow";
pub const PERMIT_PATH: &str = "/org/opdbus/v1/routing_admission";
const PERMIT_LIMIT: usize = 128;
const TRANSCRIPT_LIMIT: usize = 65_536;
const PERMIT_LIFETIME: Duration = Duration::from_secs(45);

pub type FinalAdmission = Arc<
    dyn Fn() -> Pin<Box<dyn Future<Output = Result<()>> + Send>> + Send + Sync,
>;

#[derive(Clone, PartialEq, Eq)]
struct Transcript {
    bridge: String,
    desired: String,
    expected: String,
    delete: bool,
}

struct Entry {
    transcript: Transcript,
    controller: String,
    deadline: Instant,
    admit: FinalAdmission,
    cancelled: Arc<AtomicBool>,
}

#[derive(Clone)]
pub struct OpenFlowPermitAuthority {
    connection: Connection,
    permits: Arc<Mutex<HashMap<String, Entry>>>,
}

impl OpenFlowPermitAuthority {
    /// Retain the engine's system-bus connection. A conflicting owner prevents
    /// initialization rather than queuing this connection behind another owner.
    pub async fn new(connection: Connection) -> Result<Self> {
        let reply = connection.request_name_with_flags(
            BRIDGE_SERVICE,
            zbus::fdo::RequestNameFlags::DoNotQueue.into(),
        ).await.context("own OpenFlow bridge admission service")?;
        ensure!(matches!(reply,
            zbus::fdo::RequestNameReply::PrimaryOwner | zbus::fdo::RequestNameReply::AlreadyOwner),
            "OpenFlow bridge admission name is not owned");
        let authority = Self {
            connection,
            permits: Arc::new(Mutex::new(HashMap::new())),
        };
        authority.ensure_bridge_owner().await?;
        let registered = authority.connection.object_server().at(PERMIT_PATH, authority.clone()).await
            .context("register OpenFlow admission callback")?;
        ensure!(registered, "OpenFlow admission callback is already registered");
        Ok(authority)
    }

    pub fn connection(&self) -> &Connection {
        &self.connection
    }

    async fn ensure_bridge_owner(&self) -> Result<()> {
        let own = self.connection.unique_name().context("system bus has no unique sender")?;
        ensure!(owner(&self.connection, BRIDGE_SERVICE).await? == own.as_str(),
            "OpenFlow bridge admission owner changed");
        Ok(())
    }

    pub(crate) async fn issue(
        &self,
        bridge: &str,
        desired: &str,
        expected: &str,
        delete: bool,
        admit: FinalAdmission,
    ) -> Result<OperationPermit> {
        ensure!(bridge.len().saturating_add(desired.len()).saturating_add(expected.len()) <= TRANSCRIPT_LIMIT,
            "OpenFlow permit transcript exceeds its byte bound");
        self.ensure_bridge_owner().await?;
        let controller = owner(&self.connection, CONTROLLER_SERVICE).await?;
        let id = Uuid::new_v4().to_string();
        let cancelled = Arc::new(AtomicBool::new(false));
        let mut permits = self.permits.lock().map_err(|_| anyhow::anyhow!("OpenFlow permits poisoned"))?;
        reserve_slot(&mut permits, Instant::now())?;
        permits.insert(id.clone(), Entry {
            transcript: Transcript { bridge: bridge.into(), desired: desired.into(), expected: expected.into(), delete },
            controller: controller.clone(),
            deadline: Instant::now() + PERMIT_LIFETIME,
            admit,
            cancelled: cancelled.clone(),
        });
        Ok(OperationPermit { id, controller, permits: self.permits.clone(), cancelled })
    }

    async fn consume_inner(&self, sender: &str, id: &str, transcript: Transcript) -> Result<()> {
        ensure!(id.len() == 36, "invalid OpenFlow operation permit");
        self.ensure_bridge_owner().await?;
        let controller = owner(&self.connection, CONTROLLER_SERVICE).await?;
        ensure!(sender == controller, "OpenFlow callback is not the current controller owner");
        let entry = take_entry(&self.permits, sender, id, &transcript, Instant::now())?;
        ensure!(!entry.cancelled.load(Ordering::Acquire), "OpenFlow operation permit cancelled");
        self.ensure_bridge_owner().await?;
        ensure!(owner(&self.connection, CONTROLLER_SERVICE).await? == sender,
            "OpenFlow controller owner changed before final admission");
        // Final canonical admission is the last asynchronous authority check.
        // Both service names are held without allowing replacement.
        tokio::time::timeout_at(tokio::time::Instant::from_std(entry.deadline), (entry.admit)())
            .await.context("OpenFlow final admission expired")??;
        ensure!(entry.deadline > Instant::now(), "OpenFlow operation permit expired during admission");
        ensure!(!entry.cancelled.load(Ordering::Acquire), "OpenFlow operation permit cancelled during admission");
        Ok(())
    }
}

pub(crate) async fn owner(connection: &Connection, service: &str) -> Result<String> {
    let proxy = zbus::fdo::DBusProxy::new(connection).await?;
    Ok(tokio::time::timeout(Duration::from_secs(5), proxy.get_name_owner(service.try_into()?))
        .await.context("OpenFlow service owner lookup timed out")??.to_string())
}

pub(crate) struct OperationPermit {
    pub id: String,
    pub controller: String,
    permits: Arc<Mutex<HashMap<String, Entry>>>,
    cancelled: Arc<AtomicBool>,
}

impl Drop for OperationPermit {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Release);
        if let Ok(mut permits) = self.permits.lock() {
            permits.remove(&self.id);
        }
    }
}

fn take_entry(
    permits: &Mutex<HashMap<String, Entry>>,
    sender: &str,
    id: &str,
    transcript: &Transcript,
    now: Instant,
) -> Result<Entry> {
    let mut permits = permits.lock().map_err(|_| anyhow::anyhow!("OpenFlow permits poisoned"))?;
    let entry = permits.get(id).context("OpenFlow operation permit absent or already consumed")?;
    ensure!(entry.controller == sender, "OpenFlow controller owner changed since admission");
    ensure!(entry.deadline > now, "OpenFlow operation permit expired");
    ensure!(&entry.transcript == transcript, "OpenFlow operation permit transcript mismatch");
    // Removal precedes async admission: concurrent callbacks cannot use this
    // permit twice, including when final admission fails or is cancelled.
    Ok(permits.remove(id).expect("permit checked under the same lock"))
}

fn reserve_slot(permits: &mut HashMap<String, Entry>, now: Instant) -> Result<()> {
    permits.retain(|_, entry| entry.deadline > now);
    ensure!(permits.len() < PERMIT_LIMIT, "OpenFlow operation permit budget exhausted");
    Ok(())
}

#[zbus::interface(name = "org.opdbus.v1.RoutingAdmissionV1")]
impl OpenFlowPermitAuthority {
    async fn consume_flow_permit(
        &self,
        permit: String,
        bridge: String,
        desired_json: String,
        expected_json: String,
        delete: bool,
        #[zbus(header)] header: Header<'_>,
    ) -> zbus::fdo::Result<bool> {
        if bridge.len().saturating_add(desired_json.len()).saturating_add(expected_json.len()) > TRANSCRIPT_LIMIT {
            return Err(zbus::fdo::Error::InvalidArgs("OpenFlow callback transcript exceeds its byte bound".into()));
        }
        let sender = header.sender().ok_or_else(|| zbus::fdo::Error::AccessDenied("OpenFlow callback has no sender".into()))?;
        self.consume_inner(sender.as_str(), &permit, Transcript {
            bridge, desired: desired_json, expected: expected_json, delete,
        }).await.map(|()| true).map_err(|error| zbus::fdo::Error::AccessDenied(error.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn transcript() -> Transcript {
        Transcript { bridge: "ovsbr0".into(), desired: "{\"priority\":100}".into(), expected: String::new(), delete: false }
    }

    fn entry(deadline: Instant) -> Entry {
        Entry {
            transcript: transcript(), controller: ":1.7".into(), deadline,
            admit: Arc::new(|| Box::pin(async { Ok(()) })),
            cancelled: Arc::new(AtomicBool::new(false)),
        }
    }

    #[test]
    fn exact_transcript_owner_expiry_and_replay_are_enforced() {
        let now = Instant::now();
        let permits = Mutex::new(HashMap::from([("permit".into(), entry(now + Duration::from_secs(1)))]));
        let mut changed = transcript();
        changed.delete = true;
        assert!(take_entry(&permits, ":1.7", "permit", &changed, now).is_err());
        assert!(take_entry(&permits, ":1.8", "permit", &transcript(), now).is_err());
        assert!(take_entry(&permits, ":1.7", "permit", &transcript(), now + Duration::from_secs(2)).is_err());
        assert!(take_entry(&permits, ":1.7", "permit", &transcript(), now).is_ok());
        assert!(take_entry(&permits, ":1.7", "permit", &transcript(), now).is_err());
    }

    #[test]
    fn dropping_worker_permit_cancels_consumed_admission_and_removes_pending_entry() {
        let value = entry(Instant::now() + Duration::from_secs(1));
        let cancelled = value.cancelled.clone();
        let permits = Arc::new(Mutex::new(HashMap::from([("permit".into(), value)])));
        let guard = OperationPermit { id: "permit".into(), controller: ":1.7".into(), permits: permits.clone(), cancelled: cancelled.clone() };
        let consumed = take_entry(&permits, ":1.7", "permit", &transcript(), Instant::now()).unwrap();
        drop(guard);
        assert!(consumed.cancelled.load(Ordering::Acquire));
        assert!(permits.lock().unwrap().is_empty());
        let pending = entry(Instant::now() + Duration::from_secs(1));
        let cancelled = pending.cancelled.clone();
        permits.lock().unwrap().insert("pending".into(), pending);
        drop(OperationPermit { id: "pending".into(), controller: ":1.7".into(), permits: permits.clone(), cancelled });
        assert!(permits.lock().unwrap().is_empty());
    }

    #[test]
    fn capacity_fails_fast_and_expired_slots_are_reclaimed() {
        let now = Instant::now();
        let mut permits: HashMap<String, Entry> = (0..PERMIT_LIMIT)
            .map(|index| (index.to_string(), entry(now + Duration::from_secs(1))))
            .collect();
        assert!(reserve_slot(&mut permits, now).is_err());
        assert!(reserve_slot(&mut permits, now + Duration::from_secs(2)).is_ok());
        assert!(permits.is_empty());
    }

    #[test]
    fn concurrent_callbacks_consume_exactly_once() {
        let permits = Arc::new(Mutex::new(HashMap::from([("permit".into(), entry(Instant::now() + Duration::from_secs(30)))])));
        let callbacks = (0..2).map(|_| {
            let permits = permits.clone();
            std::thread::spawn(move || take_entry(&permits, ":1.7", "permit", &transcript(), Instant::now()).is_ok())
        }).collect::<Vec<_>>();
        assert_eq!(callbacks.into_iter().map(|callback| callback.join().unwrap()).filter(|success| *success).count(), 1);
    }
}
