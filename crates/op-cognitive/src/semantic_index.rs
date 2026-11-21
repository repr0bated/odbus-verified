use anyhow::{Result, ensure};
use op_cozo_store::{CozoGraphShuttle, SemanticHit, SemanticPoint};
use sha2::{Digest, Sha256};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

#[derive(Clone)]
pub struct SemanticIndex {
    path: Option<PathBuf>,
    memory: Option<Arc<CozoGraphShuttle>>,
}

impl SemanticIndex {
    pub fn from_env() -> Self {
        let path = std::env::var_os("OPDBUS_SEMANTIC_COZO_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/var/lib/opdbus/semantic-index"));
        Self {
            path: Some(path),
            memory: None,
        }
    }

    pub fn in_memory() -> Result<Self> {
        Ok(Self {
            path: None,
            memory: Some(Arc::new(CozoGraphShuttle::new_in_memory()?)),
        })
    }

    async fn run<T: Send + 'static>(
        &self,
        namespace: &str,
        operation: impl FnOnce(&CozoGraphShuttle, &str) -> Result<T> + Send + 'static,
    ) -> Result<T> {
        ensure!(!namespace.is_empty(), "semantic namespace is required");
        let relation = format!("semantic_{:x}", Sha256::digest(namespace.as_bytes()));
        let index = self.clone();
        tokio::task::spawn_blocking(move || {
            static ACCESS: Mutex<()> = Mutex::new(());
            let _access = ACCESS.lock().unwrap_or_else(|error| error.into_inner());
            let store = match (index.path, index.memory) {
                (_, Some(store)) => store,
                (Some(path), None) => Arc::new(CozoGraphShuttle::new_persistent_private(path)?),
                (None, None) => anyhow::bail!("semantic index has no backing store"),
            };
            operation(&store, &relation)
        })
        .await?
    }

    pub async fn put(&self, namespace: &str, points: Vec<SemanticPoint>) -> Result<()> {
        self.run(namespace, move |store, relation| {
            store.prepare_semantic_vectors(relation)?;
            store.put_semantic_vectors(relation, points)
        })
        .await
    }

    pub async fn count(&self, namespace: &str) -> Result<u64> {
        self.run(namespace, |store, relation| {
            if !store.has_semantic_vectors(relation)? {
                return Ok(0);
            }
            store.semantic_count(relation)
        })
        .await
    }

    pub async fn publish_readiness(&self, namespace: &str) -> Result<()> {
        if let Some(root) = &self.path {
            let count = self.count(namespace).await?;
            let fingerprint = Sha256::digest(namespace.as_bytes());
            crate::chain_vectors::publish_checkpoint(
                root.join(format!(".indexed-count-{fingerprint:x}")),
                format!("{count}\n").into_bytes(),
            )
            .await?;
        }
        Ok(())
    }

    pub async fn search(
        &self,
        namespace: &str,
        vector: Vec<f32>,
        limit: u64,
        filters: Vec<(String, String)>,
    ) -> Result<Vec<SemanticHit>> {
        self.run(namespace, move |store, relation| {
            if !store.has_semantic_vectors(relation)? {
                return Ok(Vec::new());
            }
            store.search_semantic_vectors(relation, vector, limit, &filters)
        })
        .await
    }

    pub async fn list(
        &self,
        namespace: &str,
        limit: u32,
        filters: Vec<(String, String)>,
    ) -> Result<Vec<SemanticPoint>> {
        self.run(namespace, move |store, relation| {
            if !store.has_semantic_vectors(relation)? {
                return Ok(Vec::new());
            }
            store.list_semantic_vectors(relation, limit, &filters)
        })
        .await
    }

    pub async fn delete(&self, namespace: &str, filters: Vec<(String, String)>) -> Result<()> {
        self.run(namespace, move |store, relation| {
            if !store.has_semantic_vectors(relation)? {
                return Ok(());
            }
            store.delete_semantic_vectors(relation, &filters)
        })
        .await
    }
}
