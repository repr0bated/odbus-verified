//! Bounded, at-most-once browser fd custody on identity-bound lungs connections.

use std::collections::{HashMap, VecDeque};
use std::os::fd::OwnedFd;
use std::sync::{Arc, Mutex, Weak};
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{Context, Result, ensure};
use tokio::sync::oneshot;

use super::identity::LungsIdentity;

const MAX_BROWSER_WAITERS: usize = 64;

struct Waiter {
    id: u64,
    identity: Weak<LungsIdentity>,
    sender: oneshot::Sender<OwnedFd>,
}

#[derive(Default)]
struct Waiters {
    by_session: HashMap<String, VecDeque<Waiter>>,
    total: usize,
}

pub(crate) struct BrowserBroker {
    waiters: Mutex<Waiters>,
    next_id: AtomicU64,
}

impl Default for BrowserBroker {
    fn default() -> Self {
        Self { waiters: Mutex::new(Waiters::default()), next_id: AtomicU64::new(1) }
    }
}

struct WaiterGuard {
    broker: Arc<BrowserBroker>,
    session_id: String,
    id: u64,
}

impl Drop for WaiterGuard {
    fn drop(&mut self) {
        self.broker.remove(&self.session_id, self.id);
    }
}

impl BrowserBroker {
    pub(crate) async fn accept(self: &Arc<Self>, identity: Arc<LungsIdentity>) -> Result<OwnedFd> {
        let session_id = identity.browser_session_id().await?;
        let (sender, receiver) = oneshot::channel();
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        ensure!(id != 0, "browser waiter identifier exhausted");
        {
            let mut waiters = self.waiters.lock().map_err(|_| anyhow::anyhow!("browser waiter state poisoned"))?;
            Self::remove_closed(&mut waiters);
            ensure!(waiters.total < MAX_BROWSER_WAITERS, "browser waiter bound reached");
            waiters.by_session.entry(session_id.clone()).or_default().push_back(Waiter {
                id,
                identity: Arc::downgrade(&identity),
                sender,
            });
            waiters.total += 1;
        }
        let _guard = WaiterGuard { broker: self.clone(), session_id, id };
        receiver.await.context("browser waiter ended")
    }

    pub(crate) async fn offer(&self, session_id: &str, mut fd: OwnedFd) -> Result<()> {
        loop {
            let waiter = {
                let mut waiters = self.waiters.lock().map_err(|_| anyhow::anyhow!("browser waiter state poisoned"))?;
                Self::remove_closed(&mut waiters);
                let waiter = waiters.by_session.get_mut(session_id).and_then(VecDeque::pop_front);
                if waiter.is_some() {
                    waiters.total = waiters.total.saturating_sub(1);
                }
                if waiters.by_session.get(session_id).is_some_and(VecDeque::is_empty) {
                    waiters.by_session.remove(session_id);
                }
                waiter
            };
            let waiter = waiter.context("identity has no ready browser waiter")?;
            let Some(identity) = waiter.identity.upgrade() else { continue };
            if identity.revalidate_browser_session(session_id).await.is_err() {
                continue;
            }
            match waiter.sender.send(fd) {
                Ok(()) => return Ok(()),
                Err(returned) => fd = returned,
            }
        }
    }

    fn remove(&self, session_id: &str, id: u64) {
        let Ok(mut waiters) = self.waiters.lock() else { return; };
        let mut removed = false;
        if let Some(queue) = waiters.by_session.get_mut(session_id)
            && let Some(index) = queue.iter().position(|waiter| waiter.id == id)
        {
            queue.remove(index);
            removed = true;
        }
        if removed {
            waiters.total = waiters.total.saturating_sub(1);
        }
        if waiters.by_session.get(session_id).is_some_and(VecDeque::is_empty) {
            waiters.by_session.remove(session_id);
        }
    }

    fn remove_closed(waiters: &mut Waiters) {
        let before = waiters.total;
        waiters.by_session.retain(|_, queue| {
            queue.retain(|waiter| !waiter.sender.is_closed() && waiter.identity.strong_count() > 0);
            !queue.is_empty()
        });
        waiters.total = waiters.by_session.values().map(VecDeque::len).sum();
        debug_assert!(waiters.total <= before);
    }

    #[cfg(test)]
    pub(crate) fn waiter_count(&self) -> usize {
        self.waiters.lock().map(|waiters| waiters.total).unwrap_or(0)
    }
}
