//! Introspection caching

use op_core::{BusType, ObjectInfo};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

type CacheKey = (BusType, String, String);
type CacheMap = HashMap<CacheKey, ObjectInfo>;

pub struct IntrospectionCache {
    cache: Arc<RwLock<CacheMap>>,
}

impl IntrospectionCache {
    pub fn new() -> Self {
        Self {
            cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn get(&self, bus: BusType, service: &str, path: &str) -> Option<ObjectInfo> {
        let cache = self.cache.read().await;
        cache
            .get(&(bus, service.to_string(), path.to_string()))
            .cloned()
    }

    pub async fn set(&self, bus: BusType, service: &str, path: &str, info: ObjectInfo) {
        let mut cache = self.cache.write().await;
        cache.insert((bus, service.to_string(), path.to_string()), info);
    }

    pub async fn clear(&self) {
        let mut cache = self.cache.write().await;
        cache.clear();
    }
}

impl Default for IntrospectionCache {
    fn default() -> Self {
        Self::new()
    }
}
