//! Generic request registry: owner reserves a slot, designated responder claims it.
//!
//! The pattern: a verified principal opens a request designating another
//! principal as the responder. The responder claims the request within its
//! TTL and fulfills it exactly once. Concurrent claims are serialized; late
//! or unauthorized claims are refused.
//!
//! Pending requests live in memory. A bridge restart drops them, so a late
//! response is refused (fail closed) rather than attributed to a guess.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result, ensure};
use chrono::{DateTime, Duration, Utc};

/// Default request lifetime.
pub(crate) const DEFAULT_TTL_SECONDS: i64 = 15 * 60;
/// Bounds on open requests (pending or being answered).
pub(crate) const MAX_OPEN_REQUESTS: usize = 4096;
pub(crate) const MAX_OPEN_REQUESTS_PER_OWNER: usize = 16;

/// The verified principal who owns the request. The scope key is `principal_id`;
/// the session fields are provenance recorded on the committed response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RequestOwner {
    pub principal_id: String,
    pub session_id: String,
    pub session_genesis: String,
}

/// A pending request awaiting a response from its designated responder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PendingRequest<T> {
    pub request_id: String,
    pub owner: RequestOwner,
    pub responder_principal_id: String,
    pub opened_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    /// Domain-specific context (e.g., conversation_id + turn_index for chat).
    pub context: T,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RequestState {
    Pending,
    Claiming,
    Fulfilled,
}

struct Entry<T> {
    request: PendingRequest<T>,
    state: RequestState,
}

/// Uniqueness key for deduplication. Implementors define what makes a request
/// unique within the registry (e.g., conversation_id for chat turns).
pub(crate) trait RequestContext: Clone + Send + Sync + 'static {
    /// Returns a key that must be unique among open requests for the same owner.
    /// Two requests with the same owner and uniqueness key cannot both be open.
    fn uniqueness_key(&self) -> Option<String>;

    /// Validate the context before opening. Return an error to refuse the request.
    fn validate(&self) -> Result<()> {
        Ok(())
    }
}

/// Inner state for the request registry.
struct RegistryInner<T> {
    requests: Mutex<HashMap<String, Entry<T>>>,
    max_open: usize,
    max_per_owner: usize,
}

/// Generic request registry parameterized by context type.
///
/// The registry wraps its state in Arc to support the claim pattern where
/// a claim holds a reference back to the registry for settlement.
pub(crate) struct RequestRegistry<T> {
    inner: Arc<RegistryInner<T>>,
}

impl<T: RequestContext> Clone for RequestRegistry<T> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl<T: RequestContext> Default for RequestRegistry<T> {
    fn default() -> Self {
        Self::new(MAX_OPEN_REQUESTS, MAX_OPEN_REQUESTS_PER_OWNER)
    }
}

impl<T: RequestContext> RequestRegistry<T> {
    pub(crate) fn new(max_open: usize, max_per_owner: usize) -> Self {
        Self {
            inner: Arc::new(RegistryInner {
                requests: Mutex::new(HashMap::new()),
                max_open,
                max_per_owner,
            }),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Entry<T>>> {
        // A poisoned map still holds consistent entries: every mutation is a
        // single insert/remove/state assignment.
        self.inner
            .requests
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn collect_expired(requests: &mut HashMap<String, Entry<T>>, now: DateTime<Utc>) {
        // A claim in flight keeps its entry until the commit settles.
        requests.retain(|_, entry| {
            entry.state == RequestState::Claiming || entry.request.expires_at > now
        });
    }

    /// Open a request now with the default lifetime.
    #[allow(dead_code)]
    pub(crate) fn open(
        &self,
        owner: RequestOwner,
        responder_principal_id: &str,
        context: T,
    ) -> Result<PendingRequest<T>> {
        self.open_at(
            owner,
            responder_principal_id,
            context,
            Utc::now(),
            Duration::seconds(DEFAULT_TTL_SECONDS),
        )
    }

    /// Withdraw a request that was reserved but never offered to its responder.
    /// Only a request still pending is removed; one being claimed or fulfilled stays.
    pub(crate) fn cancel(&self, request_id: &str) {
        let mut requests = self.lock();
        if requests
            .get(request_id)
            .is_some_and(|entry| entry.state == RequestState::Pending)
        {
            requests.remove(request_id);
        }
    }

    /// Open a request at `now` for `ttl`.
    pub(crate) fn open_at(
        &self,
        owner: RequestOwner,
        responder_principal_id: &str,
        context: T,
        now: DateTime<Utc>,
        ttl: Duration,
    ) -> Result<PendingRequest<T>> {
        ensure!(
            !owner.principal_id.is_empty()
                && !owner.session_id.is_empty()
                && !owner.session_genesis.is_empty(),
            "request owner must be a verified identity"
        );
        ensure!(
            !responder_principal_id.is_empty(),
            "request responder principal is required"
        );
        ensure!(
            responder_principal_id != owner.principal_id,
            "a principal cannot be the designated responder of their own request"
        );
        ensure!(
            ttl > Duration::zero() && ttl <= Duration::hours(1),
            "request lifetime must be positive and at most one hour"
        );
        context.validate()?;

        let mut requests = self.lock();
        Self::collect_expired(&mut requests, now);

        // Check uniqueness constraint if the context defines one.
        if let Some(key) = context.uniqueness_key() {
            ensure!(
                !requests.values().any(|entry| {
                    entry.state != RequestState::Fulfilled
                        && entry.request.owner.principal_id == owner.principal_id
                        && entry.request.context.uniqueness_key().as_ref() == Some(&key)
                }),
                "a request with this uniqueness key is already open for this owner"
            );
        }

        ensure!(
            requests.len() < self.inner.max_open,
            "too many open requests"
        );
        ensure!(
            requests
                .values()
                .filter(|entry| {
                    entry.state != RequestState::Fulfilled
                        && entry.request.owner.principal_id == owner.principal_id
                })
                .count()
                < self.inner.max_per_owner,
            "too many open requests for this owner"
        );

        let request = PendingRequest {
            request_id: uuid::Uuid::now_v7().to_string(),
            owner,
            responder_principal_id: responder_principal_id.to_string(),
            opened_at: now,
            expires_at: now + ttl,
            context,
        };
        requests.insert(
            request.request_id.clone(),
            Entry {
                request: request.clone(),
                state: RequestState::Pending,
            },
        );
        Ok(request)
    }

    #[allow(dead_code)]
    pub(crate) fn claim(&self, request_id: &str, caller: &str) -> Result<RequestClaim<T>> {
        self.claim_at(request_id, caller, Utc::now())
    }

    /// Claim a pending request for its designated responder. Nothing changes
    /// when the claim is refused.
    pub(crate) fn claim_at(
        &self,
        request_id: &str,
        caller: &str,
        now: DateTime<Utc>,
    ) -> Result<RequestClaim<T>> {
        let mut requests = self.lock();
        let entry = requests
            .get_mut(request_id)
            .context("request is not open: the bridge never opened it or it was dropped")?;
        match entry.state {
            RequestState::Pending => {}
            RequestState::Claiming => anyhow::bail!("request is already being fulfilled"),
            RequestState::Fulfilled => anyhow::bail!("request is already fulfilled"),
        }
        if entry.request.expires_at <= now {
            requests.remove(request_id);
            anyhow::bail!("request has expired");
        }
        ensure!(
            entry.request.responder_principal_id == caller,
            "caller is not the designated responder of this request"
        );
        entry.state = RequestState::Claiming;
        Ok(RequestClaim {
            registry: self.clone(),
            request: entry.request.clone(),
            settled: false,
        })
    }

    /// List pending requests for a given responder principal.
    pub(crate) fn list_pending_for_responder(
        &self,
        responder_principal_id: &str,
    ) -> Vec<PendingRequest<T>> {
        let now = Utc::now();
        let requests = self.lock();
        requests
            .values()
            .filter(|entry| {
                entry.state == RequestState::Pending
                    && entry.request.responder_principal_id == responder_principal_id
                    && entry.request.expires_at > now
            })
            .map(|entry| entry.request.clone())
            .collect()
    }

    fn settle(&self, request_id: &str, fulfilled: bool) {
        let mut requests = self.lock();
        if let Some(entry) = requests.get_mut(request_id) {
            entry.state = if fulfilled {
                RequestState::Fulfilled
            } else {
                RequestState::Pending
            };
        }
    }

    #[cfg(test)]
    pub(crate) fn is_open(&self, request_id: &str) -> bool {
        self.lock()
            .get(request_id)
            .is_some_and(|entry| entry.state != RequestState::Fulfilled)
    }
}

/// Exclusive right to fulfill one request. Dropping it without `fulfilled()`
/// returns the request to pending (the durable owner still refuses a second
/// row, so a retry after an uncertain commit cannot fulfill twice).
pub(crate) struct RequestClaim<T: RequestContext> {
    registry: RequestRegistry<T>,
    request: PendingRequest<T>,
    settled: bool,
}

impl<T: RequestContext> RequestClaim<T> {
    pub(crate) fn request(&self) -> &PendingRequest<T> {
        &self.request
    }

    pub(crate) fn fulfilled(mut self) {
        self.settled = true;
        self.registry.settle(&self.request.request_id, true);
    }

    pub(crate) fn release(mut self) {
        self.settled = true;
        self.registry.settle(&self.request.request_id, false);
    }
}

impl<T: RequestContext> Drop for RequestClaim<T> {
    fn drop(&mut self) {
        if !self.settled {
            self.registry.settle(&self.request.request_id, false);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HUMAN: &str = "human-principal";
    const AGENT: &str = "agent-principal";

    /// Simple test context with optional uniqueness key.
    #[derive(Debug, Clone, PartialEq, Eq)]
    struct TestContext {
        key: Option<String>,
    }

    impl RequestContext for TestContext {
        fn uniqueness_key(&self) -> Option<String> {
            self.key.clone()
        }
    }

    fn owner(session: &str) -> RequestOwner {
        RequestOwner {
            principal_id: HUMAN.into(),
            session_id: session.into(),
            session_genesis: "ab".repeat(32),
        }
    }

    fn unique_key() -> String {
        uuid::Uuid::now_v7().to_string()
    }

    #[test]
    fn only_the_designated_responder_fulfills_an_open_request_once() {
        let registry = RequestRegistry::<TestContext>::default();
        let key = unique_key();
        let request = registry
            .open(owner("s1"), AGENT, TestContext { key: Some(key) })
            .unwrap();
        assert_eq!(
            uuid::Uuid::parse_str(&request.request_id)
                .unwrap()
                .get_version_num(),
            7
        );
        assert!(registry.claim(&request.request_id, "someone-else").is_err());
        assert!(registry.claim(&request.request_id, HUMAN).is_err());
        let claim = registry.claim(&request.request_id, AGENT).unwrap();
        assert_eq!(claim.request().owner.principal_id, HUMAN);
        assert!(
            registry.claim(&request.request_id, AGENT).is_err(),
            "concurrent fulfillment is refused while one is in flight"
        );
        // A failed commit returns the request to pending.
        drop(claim);
        let claim = registry.claim(&request.request_id, AGENT).unwrap();
        claim.fulfilled();
        let error = registry
            .claim(&request.request_id, AGENT)
            .err()
            .unwrap()
            .to_string();
        assert!(error.contains("already fulfilled"), "{error}");
        assert!(
            registry
                .claim(&uuid::Uuid::now_v7().to_string(), AGENT)
                .is_err(),
            "an unopened request is refused"
        );
    }

    #[test]
    fn expired_requests_are_refused_and_collected() {
        let registry = RequestRegistry::<TestContext>::default();
        let key = unique_key();
        let opened = Utc::now() - Duration::minutes(30);
        let request = registry
            .open_at(
                owner("s1"),
                AGENT,
                TestContext {
                    key: Some(key.clone()),
                },
                opened,
                Duration::minutes(15),
            )
            .unwrap();
        let error = registry
            .claim(&request.request_id, AGENT)
            .err()
            .unwrap()
            .to_string();
        assert!(error.contains("expired"), "{error}");
        assert!(!registry.is_open(&request.request_id));
        // An expired request does not block opening another with the same key.
        registry
            .open(owner("s1"), AGENT, TestContext { key: Some(key) })
            .unwrap();
    }

    #[test]
    fn uniqueness_key_enforced_and_bounded_per_owner() {
        let registry = RequestRegistry::<TestContext>::default();
        let key = unique_key();
        registry
            .open(
                owner("s1"),
                AGENT,
                TestContext {
                    key: Some(key.clone()),
                },
            )
            .unwrap();
        // Same uniqueness key for same owner is refused.
        assert!(
            registry
                .open(
                    owner("s1"),
                    AGENT,
                    TestContext {
                        key: Some(key.clone())
                    }
                )
                .is_err()
        );
        // Different key is allowed.
        for _ in 1..MAX_OPEN_REQUESTS_PER_OWNER {
            registry
                .open(
                    owner("s1"),
                    AGENT,
                    TestContext {
                        key: Some(unique_key()),
                    },
                )
                .unwrap();
        }
        assert!(
            registry
                .open(
                    owner("s1"),
                    AGENT,
                    TestContext {
                        key: Some(unique_key())
                    }
                )
                .is_err(),
            "per-owner limit reached"
        );
        assert!(
            registry
                .open(owner("s1"), HUMAN, TestContext { key: None })
                .is_err(),
            "a principal cannot respond to their own request"
        );
    }

    #[test]
    fn list_pending_for_responder_works() {
        let registry = RequestRegistry::<TestContext>::default();
        let key1 = unique_key();
        let key2 = unique_key();
        let r1 = registry
            .open(owner("s1"), AGENT, TestContext { key: Some(key1) })
            .unwrap();
        let r2 = registry
            .open(owner("s2"), AGENT, TestContext { key: Some(key2) })
            .unwrap();
        let _r3 = registry
            .open(owner("s3"), "other-agent", TestContext { key: None })
            .unwrap();

        let pending = registry.list_pending_for_responder(AGENT);
        assert_eq!(pending.len(), 2);
        let ids: Vec<_> = pending.iter().map(|r| r.request_id.as_str()).collect();
        assert!(ids.contains(&r1.request_id.as_str()));
        assert!(ids.contains(&r2.request_id.as_str()));

        // Claim one, it should no longer appear.
        let claim = registry.claim(&r1.request_id, AGENT).unwrap();
        let pending = registry.list_pending_for_responder(AGENT);
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].request_id, r2.request_id);

        // Release claim, it reappears.
        claim.release();
        let pending = registry.list_pending_for_responder(AGENT);
        assert_eq!(pending.len(), 2);
    }

    #[test]
    fn no_uniqueness_key_allows_multiple_requests() {
        let registry = RequestRegistry::<TestContext>::default();
        // No uniqueness key means multiple requests are allowed.
        for _ in 0..5 {
            registry
                .open(owner("s1"), AGENT, TestContext { key: None })
                .unwrap();
        }
    }
}
