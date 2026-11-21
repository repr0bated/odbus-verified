//! Assemble a [`Door`] from configuration.

use crate::audit::AuditLog;
use crate::catalog::Catalog;
use crate::config::Config;
use crate::fusecall::{CallOpener, Mount};
use crate::grants::{DurableGrants, GrantStore, TicketStore};
use crate::identity::{MountMap, Verifier};
use crate::mcp::{Door, RateLimiter};
use crate::policy::Policy;
use crate::readiness::{CozoReadiness, Readiness, default_gates};
use crate::toolsets::Toolsets;
use anyhow::{Context, Result};
use std::sync::Arc;
use std::time::Duration;

/// How long a vector-readiness probe result is reused.
pub const READINESS_TTL: Duration = Duration::from_secs(30);

/// The coding/knowledge vector gates from configuration.
pub fn build_readiness(config: &Config) -> Result<Readiness> {
    Ok(Readiness::new(
        Arc::new(CozoReadiness::new(
            config.footprint_cozo_path.clone(),
            config.semantic_cozo_path.clone(),
        )),
        default_gates(config.coding_collections.clone(), None),
        READINESS_TTL,
    ))
}

/// Load the mount map; the catalog is projected from its `default` mount.
pub fn load_mounts(config: &Config) -> Result<MountMap> {
    let path = config.mounts_file();
    let value = crate::catalog::read_json(&path)
        .with_context(|| format!("mount map {}", path.display()))?;
    serde_json::from_value(value).context("mounts.json shape")
}

/// Build the door. Returns startup warnings (never secrets).
pub fn build_door(
    config: &Config,
    audit: AuditLog,
    opener: Arc<dyn CallOpener>,
) -> Result<(Door, Vec<String>)> {
    let mut warnings = Vec::new();
    let mounts = load_mounts(config)?;
    let catalog_spec = mounts
        .default
        .clone()
        .context("mounts.json needs a `default` mount to project the catalog from")?;
    let mount = Mount::open(&catalog_spec).map_err(|e| anyhow::anyhow!("catalog mount: {e}"))?;
    let catalog = Catalog::scan(mount.root())?;
    if !catalog.skipped.is_empty() {
        warnings.push(format!("{} operations skipped", catalog.skipped.len()));
    }
    let (toolsets, fallback) = Toolsets::load_or_generate(&config.toolsets_path, &catalog);
    if let Some(reason) = fallback {
        warnings.push(format!("toolsets generated in memory ({reason})"));
    }
    if !toolsets.file.missing.is_empty() {
        warnings.push(format!(
            "hot tools absent from catalog: {}",
            toolsets.file.missing.join(",")
        ));
    }
    let verifier = Verifier::load(&config.oia1_keys_file(), &config.principals_file())?;
    let policy = Policy::new(
        DurableGrants::new(&config.grants_document),
        GrantStore::new(config.grants_dir()),
        TicketStore::new(config.tickets_dir()),
        &toolsets,
    );
    let door = Door::new(
        catalog,
        toolsets,
        policy,
        verifier,
        mounts,
        opener,
        audit,
        RateLimiter::new(config.public_rate_per_minute),
        config.allowed_origins.clone(),
        config.max_body_bytes,
    )
    .with_readiness(Arc::new(build_readiness(config)?));
    Ok((door, warnings))
}
