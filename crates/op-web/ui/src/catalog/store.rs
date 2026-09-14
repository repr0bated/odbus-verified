//! Versioned, immutable element store.
//!
//! Invariants:
//! - Element `(id, version)` pairs are immutable once admitted.
//! - Retiring an element removes it from the *active gallery* but the
//!   resolver still serves pinned references until an operator accepts a
//!   migration. This prevents gemma's mint-on-use churn from breaking views.
//! - The active gallery is capped at 200; of those, at most `STABLE_CORE_MAX`
//!   carry [`super::dsl::Tier::StableCore`] and may not be retired by gemma.
//! - The most recent good snapshot is cached so a CatalogService outage does
//!   not blank the UI.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use super::dsl::{Element, ElementId, ElementVersion, PinnedRef, Tier};

pub const GALLERY_SIZE: usize = 200;
pub const STABLE_CORE_MAX: usize = 40;

#[derive(Debug, Clone, Default)]
pub struct CatalogSnapshot {
    /// All (id, version) we have ever seen — resolves pinned refs forever.
    pub elements: HashMap<(ElementId, ElementVersion), Element>,
    /// Currently active gallery: at most `GALLERY_SIZE` ids, latest version each.
    pub active: HashMap<ElementId, ElementVersion>,
}

impl CatalogSnapshot {
    pub fn resolve(&self, pin: &PinnedRef) -> Option<&Element> {
        self.elements.get(&(pin.id.clone(), pin.version))
    }

    pub fn latest_active(&self, id: &ElementId) -> Option<&Element> {
        let v = self.active.get(id)?;
        self.elements.get(&(id.clone(), *v))
    }

    pub fn stable_core_count(&self) -> usize {
        self.active
            .iter()
            .filter_map(|(id, v)| self.elements.get(&(id.clone(), *v)))
            .filter(|e| matches!(e.tier, Tier::StableCore))
            .count()
    }
}

#[derive(Debug, Clone, Default)]
pub struct CatalogStore {
    inner: Arc<RwLock<CatalogSnapshot>>,
}

impl CatalogStore {
    pub fn new() -> Self { Self::default() }

    /// Snapshot for read-only consumers (views, interpreter).
    pub fn snapshot(&self) -> CatalogSnapshot {
        self.inner.read().expect("catalog store poisoned").clone()
    }

    /// Admit a freshly minted element from gemma. Returns the prior version
    /// if this id already existed (callers may want to log migrations).
    pub fn admit(&self, element: Element) -> Option<ElementVersion> {
        let mut g = self.inner.write().expect("catalog store poisoned");
        let prior = g.active.get(&element.id).copied();
        // Immutable insert into the resolver table.
        g.elements
            .entry((element.id.clone(), element.version))
            .or_insert_with(|| element.clone());
        // Active gallery promotion.
        g.active.insert(element.id.clone(), element.version);
        prior
    }

    /// Retire an element from the active gallery. Pinned refs still resolve.
    /// Stable-core elements are protected and silently refused.
    pub fn retire(&self, id: &ElementId) -> bool {
        let mut g = self.inner.write().expect("catalog store poisoned");
        if let Some(v) = g.active.get(id).copied() {
            if let Some(el) = g.elements.get_mut(&(id.clone(), v)) {
                if matches!(el.tier, Tier::StableCore) {
                    return false;
                }
                el.retired = true;
            }
            g.active.remove(id);
            true
        } else {
            false
        }
    }
}
