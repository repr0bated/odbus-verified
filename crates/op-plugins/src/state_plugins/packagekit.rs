//! PackageKit Plugin for op-dbus
//!
//! Manages system packages via PackageKit D-Bus interface
//! Provides declarative package installation/removal

use anyhow::{Context, Result};
use async_trait::async_trait;
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use simd_json::prelude::*;
use simd_json::OwnedValue as Value;
use std::collections::HashMap;
use zbus::{proxy, zvariant::OwnedObjectPath};

use op_state::{
    ApplyResult, Checkpoint, DiffMetadata, PluginCapabilities, StateAction, StateDiff, StatePlugin,
};
use op_state_store::{Constraint, FieldSchema, FieldType, PluginSchema};

// PackageKit D-Bus interface
#[proxy(
    interface = "org.freedesktop.PackageKit",
    default_service = "org.freedesktop.PackageKit",
    default_path = "/org/freedesktop/PackageKit"
)]
trait PackageKit {
    /// Get transaction list
    async fn get_transaction_list(&self) -> zbus::Result<Vec<String>>;

    /// Create transaction
    async fn create_transaction(&self) -> zbus::Result<OwnedObjectPath>;
}

// Transaction interface
#[proxy(
    interface = "org.freedesktop.PackageKit.Transaction",
    default_service = "org.freedesktop.PackageKit"
)]
trait PackageKitTransaction {
    /// Install packages
    async fn install_packages(
        &self,
        transaction_flags: u64,
        package_ids: Vec<String>,
    ) -> zbus::Result<()>;

    /// Remove packages
    async fn remove_packages(
        &self,
        transaction_flags: u64,
        package_ids: Vec<String>,
        allow_deps: bool,
        autoremove: bool,
    ) -> zbus::Result<()>;

    /// Resolve packages
    async fn resolve(&self, filters: u64, packages: Vec<String>) -> zbus::Result<()>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageState {
    pub ensure: String,           // "installed", "removed", "latest"
    pub provider: Option<String>, // "apt", "dnf", "pacman", etc.
    pub version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageKitState {
    pub version: u32,
    pub packages: HashMap<String, PackageState>,
}

#[derive(Debug, Clone)]
pub struct PackageKitPlugin;

impl Default for PackageKitPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl PackageKitPlugin {
    pub fn new() -> Self {
        Self
    }

    /// Install package via PackageKit D-Bus (AGENTS.md §4: no subprocess bypasses)
    async fn install_via_direct(&self, package_name: &str) -> Result<()> {
        let conn = zbus::Connection::system()
            .await
            .context("Failed to connect to D-Bus for PackageKit")?;
        let package_id = resolve_unique_package_id(&conn, package_name, (1 << 3) | (1 << 16), 2)
            .await
            .context("resolve available PackageKit package")?;
        dispatch_packagekit_on_connection(
            &conn,
            "install_packages",
            &serde_json::json!({
                "transaction_flags": 0u64, "package_ids": [package_id]
            }),
        )
        .await
        .context("PackageKit install transaction")?;
        Ok(())
    }

    /// Remove package via PackageKit D-Bus (AGENTS.md §4: no subprocess bypasses)
    async fn remove_via_direct(&self, package_name: &str) -> Result<()> {
        let conn = zbus::Connection::system()
            .await
            .context("Failed to connect to D-Bus for PackageKit")?;
        let package_id = resolve_unique_package_id(&conn, package_name, 1 << 2, 1)
            .await
            .context("resolve installed PackageKit package")?;
        dispatch_packagekit_on_connection(
            &conn,
            "remove_packages",
            &serde_json::json!({
                "transaction_flags": 0u64, "package_ids": [package_id],
                "allow_deps": false, "autoremove": false
            }),
        )
        .await
        .context("PackageKit remove transaction")?;
        Ok(())
    }

    /// Query installed state through PackageKit's canonical D-Bus interface.
    async fn package_installed(&self, package_name: &str) -> Result<bool> {
        let conn = zbus::Connection::system()
            .await
            .context("Failed to connect to D-Bus for PackageKit")?;
        let result = dispatch_packagekit_on_connection(
            &conn,
            "resolve",
            &serde_json::json!({"filter": 1u64 << 2, "packages": [package_name]}),
        )
        .await
        .context("resolve installed PackageKit package")?;
        let installed = result.package_details.as_slice().iter().any(|package| {
            package.info == 1
                && package
                    .package_id
                    .split(';')
                    .next()
                    .is_some_and(|name| name == package_name)
        });
        Ok(installed)
    }
}

#[async_trait]
impl StatePlugin for PackageKitPlugin {
    fn name(&self) -> &str {
        "packagekit"
    }

    fn version(&self) -> &str {
        "1.1.0"
    }

    fn schema(&self) -> Option<PluginSchema> {
        Some(packagekit_schema())
    }

    async fn calculate_diff(&self, _current: &Value, desired: &Value) -> Result<StateDiff> {
        println!("PackageKit calculate_diff called with: {}", desired);
        let packages_obj = desired
            .get("packages")
            .ok_or_else(|| anyhow::anyhow!("missing packages field"))?;
        let packages: HashMap<String, PackageState> =
            simd_json::serde::from_owned_value(packages_obj.clone())?;
        let desired_state = PackageKitState {
            version: 1,
            packages,
        };

        let mut actions = Vec::new();

        for (package_name, package_config) in &desired_state.packages {
            let is_installed = self.package_installed(package_name).await?;

            match package_config.ensure.as_str() {
                "installed" if !is_installed => {
                    actions.push(StateAction::Create {
                        resource: package_name.clone(),
                        config: simd_json::json!({
                            "ensure": "installed",
                            "provider": package_config.provider,
                            "version": package_config.version
                        }),
                    });
                }
                "removed" if is_installed => {
                    actions.push(StateAction::Delete {
                        resource: package_name.clone(),
                    });
                }
                _ => {
                    actions.push(StateAction::NoOp {
                        resource: package_name.clone(),
                    });
                }
            }
        }

        Ok(StateDiff {
            plugin: self.name().to_string(),
            actions,
            metadata: DiffMetadata {
                timestamp: chrono::Utc::now().timestamp(),
                current_hash: "placeholder".to_string(),
                desired_hash: "placeholder".to_string(),
            },
        })
    }

    async fn apply_state(&self, diff: &StateDiff) -> Result<ApplyResult> {
        let mut changes_applied = Vec::new();
        let mut errors = Vec::new();

        for action in &diff.actions {
            match action {
                StateAction::Create {
                    resource,
                    config: _,
                } => match self.install_via_direct(resource).await {
                    Ok(()) => {
                        changes_applied.push(format!("✅ Installed package: {}", resource));
                    }
                    Err(e) => {
                        errors.push(format!("❌ Failed to install {}: {}", resource, e));
                    }
                },
                StateAction::Delete { resource } => match self.remove_via_direct(resource).await {
                    Ok(()) => {
                        changes_applied.push(format!("✅ Removed package: {}", resource));
                    }
                    Err(e) => {
                        errors.push(format!("❌ Failed to remove {}: {}", resource, e));
                    }
                },
                StateAction::Modify {
                    resource,
                    changes: _,
                } => {
                    changes_applied.push(format!(
                        "⚠️  Package {} modification not implemented",
                        resource
                    ));
                }
                StateAction::NoOp { resource } => {
                    changes_applied.push(format!("📦 Package {}: no action required", resource));
                }
            }
        }

        Ok(ApplyResult {
            success: errors.is_empty(),
            changes_applied,
            errors,
            checkpoint: None,
        })
    }

    async fn verify_state(&self, desired: &Value) -> Result<bool> {
        let packages_obj = desired
            .get("packages")
            .ok_or_else(|| anyhow::anyhow!("missing packages field"))?;
        let packages: HashMap<String, PackageState> =
            simd_json::serde::from_owned_value(packages_obj.clone())?;

        for (package_name, package_config) in &packages {
            let is_installed = self.package_installed(package_name).await?;

            match package_config.ensure.as_str() {
                "installed" if !is_installed => {
                    return Ok(false);
                }
                "removed" if is_installed => {
                    return Ok(false);
                }
                _ => {}
            }
        }

        Ok(true)
    }

    async fn create_checkpoint(&self) -> Result<Checkpoint> {
        Ok(Checkpoint {
            id: format!("{}-{}", self.name(), chrono::Utc::now().timestamp()),
            plugin: self.name().to_string(),
            timestamp: chrono::Utc::now().timestamp(),
            state_snapshot: simd_json::json!({}),
            backend_checkpoint: None,
        })
    }

    async fn rollback(&self, _checkpoint: &Checkpoint) -> Result<()> {
        Ok(())
    }

    fn capabilities(&self) -> PluginCapabilities {
        PluginCapabilities {
            supports_rollback: false,
            supports_checkpoints: true,
            supports_verification: true,
            atomic_operations: false,
        }
    }
}

// Self-registration: the plugin registry discovers this via inventory
// (single source of the catalog; no central dispatch list).
inventory::submit! {
    crate::default_registry::PluginReg::new("packagekit", |_ctx| std::sync::Arc::new(PackageKitPlugin::new()))
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ResolvePackagesInput {
    pub filter: u64,
    pub packages: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct InstallPackagesInput {
    pub transaction_flags: u64,
    pub package_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct RemovePackagesInput {
    pub transaction_flags: u64,
    pub package_ids: Vec<String>,
    pub allow_deps: bool,
    pub autoremove: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct UpdatePackagesInput {
    pub transaction_flags: u64,
    pub package_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct GetUpdatesInput {
    pub filter: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SearchNamesInput {
    pub filter: u64,
    pub values: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PackageKitPackage {
    /// PkInfoEnum value supplied by the Package signal.
    pub info: u32,
    pub package_id: String,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PackageKitResult {
    pub transaction: String,
    pub packages: Vec<String>,
    pub package_details: Vec<PackageKitPackage>,
    pub exit_code: u32,
    pub runtime_ms: u32,
}

async fn resolve_unique_package_id(
    conn: &zbus::Connection,
    package_name: &str,
    filter: u64,
    expected_info: u32,
) -> Result<String> {
    if package_name.is_empty()
        || package_name.contains(';')
        || package_name.trim() != package_name
        || package_name.chars().any(char::is_control)
    {
        anyhow::bail!("invalid package name");
    }
    let result = dispatch_packagekit_on_connection(
        conn,
        "resolve",
        &serde_json::json!({"filter": filter, "packages": [package_name]}),
    )
    .await?;
    let mut matches = result
        .package_details
        .into_iter()
        .filter_map(|package| {
            let fields = package.package_id.split(';').collect::<Vec<_>>();
            (package.info == expected_info
                && fields.len() == 4
                && fields[0] == package_name
                && !fields[1].is_empty()
                && !fields[2].is_empty()
                && !fields[3].is_empty())
            .then_some(package.package_id)
        })
        .collect::<Vec<_>>();
    matches.sort();
    matches.dedup();
    if matches.len() != 1 {
        anyhow::bail!("PackageKit selection for {package_name} is ambiguous or unavailable");
    }
    Ok(matches.remove(0))
}

async fn dispatch_packagekit_on_connection(
    conn: &zbus::Connection,
    method: &str,
    args: &serde_json::Value,
) -> Result<PackageKitResult> {
    dispatch_packagekit_on_connection_with_deadline(
        conn,
        method,
        args,
        std::time::Duration::from_secs(120),
    )
    .await
}

pub(crate) async fn dispatch_packagekit_method_with_connection(
    conn: &zbus::Connection,
    method: &str,
    args: &serde_json::Value,
) -> Result<serde_json::Value> {
    Ok(serde_json::to_value(
        dispatch_packagekit_on_connection(conn, method, args).await?,
    )?)
}

async fn dispatch_packagekit_on_connection_with_deadline(
    conn: &zbus::Connection,
    method: &str,
    args: &serde_json::Value,
    deadline: std::time::Duration,
) -> Result<PackageKitResult> {
    let root = tokio::time::timeout(
        deadline,
        zbus::Proxy::new(
            conn,
            "org.freedesktop.PackageKit",
            "/org/freedesktop/PackageKit",
            "org.freedesktop.PackageKit",
        ),
    )
    .await
    .context("PackageKit root proxy timeout")??;
    let path: OwnedObjectPath = tokio::time::timeout(deadline, root.call("CreateTransaction", &()))
        .await
        .context("PackageKit CreateTransaction timeout")?
        .context("PackageKit CreateTransaction")?;
    let tx = tokio::time::timeout(
        deadline,
        zbus::Proxy::new(
            conn,
            "org.freedesktop.PackageKit",
            &path,
            "org.freedesktop.PackageKit.Transaction",
        ),
    )
    .await
    .context("PackageKit transaction proxy timeout")??;
    // One ordered stream is required: separate streams can observe Finished
    // before queued Package/ErrorCode messages and report a false success.
    let mut signals = tx.receive_all_signals().await?;
    let mut package_ids = Vec::new();
    let mut package_details = Vec::new();
    match method {
        "resolve" => {
            let input: ResolvePackagesInput = serde_json::from_value(args.clone())?;
            tokio::time::timeout(
                deadline,
                tx.call::<_, _, ()>("Resolve", &(input.filter, input.packages)),
            )
            .await
            .context("PackageKit Resolve timeout")??;
        }
        "install_packages" => {
            let input: InstallPackagesInput = serde_json::from_value(args.clone())?;
            tokio::time::timeout(
                deadline,
                tx.call::<_, _, ()>(
                    "InstallPackages",
                    &(input.transaction_flags, input.package_ids),
                ),
            )
            .await
            .context("PackageKit InstallPackages timeout")??;
        }
        "remove_packages" => {
            let input: RemovePackagesInput = serde_json::from_value(args.clone())?;
            tokio::time::timeout(
                deadline,
                tx.call::<_, _, ()>(
                    "RemovePackages",
                    &(
                        input.transaction_flags,
                        input.package_ids,
                        input.allow_deps,
                        input.autoremove,
                    ),
                ),
            )
            .await
            .context("PackageKit RemovePackages timeout")??;
        }
        "update_packages" => {
            let input: UpdatePackagesInput = serde_json::from_value(args.clone())?;
            tokio::time::timeout(
                deadline,
                tx.call::<_, _, ()>(
                    "UpdatePackages",
                    &(input.transaction_flags, input.package_ids),
                ),
            )
            .await
            .context("PackageKit UpdatePackages timeout")??;
        }
        "get_updates" => {
            let input: GetUpdatesInput = serde_json::from_value(args.clone())?;
            tokio::time::timeout(
                deadline,
                tx.call::<_, _, ()>("GetUpdates", &(input.filter,)),
            )
            .await
            .context("PackageKit GetUpdates timeout")??;
        }
        "search_names" => {
            let input: SearchNamesInput = serde_json::from_value(args.clone())?;
            tokio::time::timeout(
                deadline,
                tx.call::<_, _, ()>("SearchNames", &(input.filter, input.values)),
            )
            .await
            .context("PackageKit SearchNames timeout")??;
        }
        _ => anyhow::bail!("unknown PackageKit method: {method}"),
    }
    let result = tokio::time::timeout(deadline, async {
        loop {
            tokio::select! {
                Some(signal) = signals.next() => {
                    let header = signal.header();
                    let member = header.member().map(|m| m.as_str()).unwrap_or_default();
                    match member {
                        "Package" => {
                            let (info, package_id, summary) =
                                signal.body().deserialize::<(u32, String, String)>()?;
                            package_ids.push(package_id.clone());
                            package_details.push(PackageKitPackage {
                                info,
                                package_id,
                                summary,
                            });
                        }
                        "ErrorCode" => {
                            let (code, description) = signal.body().deserialize::<(u32, String)>()?;
                            anyhow::bail!("PackageKit error {code}: {description}");
                        }
                        "Finished" => {
                            let (code, runtime) = signal.body().deserialize::<(u32, u32)>()?;
                            break Ok((code, runtime));
                        }
                        _ => {}
                    }
                }
                else => anyhow::bail!("PackageKit transaction ended without Finished"),
            }
        }
    })
    .await
    .context("PackageKit transaction timeout")??;
    if result.0 != 1 {
        anyhow::bail!("PackageKit transaction failed with exit code {}", result.0);
    }
    package_ids.sort();
    package_ids.dedup();
    package_details.sort_by(|left, right| left.package_id.cmp(&right.package_id));
    package_details.dedup_by(|left, right| {
        left.info == right.info
            && left.package_id == right.package_id
            && left.summary == right.summary
    });
    Ok(PackageKitResult {
        transaction: path.to_string(),
        packages: package_ids,
        package_details,
        exit_code: result.0,
        runtime_ms: result.1,
    })
}

pub async fn dispatch_packagekit_method(
    method: &str,
    args: &serde_json::Value,
) -> Result<serde_json::Value> {
    let connection = zbus::Connection::system()
        .await
        .context("connect to PackageKit system bus")?;
    Ok(serde_json::to_value(
        dispatch_packagekit_on_connection(&connection, method, args).await?,
    )?)
}

pub(crate) fn packagekit_schema() -> PluginSchema {
    let mut schema = PluginSchema::builder("packagekit")
        .category("service")
        .version("1.1.0")
        .description("PackageKit package declarations")
        .field(
            "version",
            FieldSchema {
                field_type: FieldType::Integer,
                required: false,
                description: "Schema version".to_string(),
                default: Some(simd_json::json!(1)),
                example: None,
                constraints: vec![Constraint::Min { value: 1.0 }],
                read_only: false,
                read_only_when: None,
            },
        )
        .field(
            "packages",
            FieldSchema {
                field_type: FieldType::Any,
                required: true,
                description: "Package declaration map".to_string(),
                default: Some(simd_json::json!({})),
                example: None,
                constraints: Vec::new(),
                read_only: false,
                read_only_when: None,
            },
        )
        .build();

    // org.freedesktop.PackageKit methods
    // Spec: https://www.freedesktop.org/software/PackageKit/gtk-doc/api-reference.html
    schema.methods.insert(
        "resolve".to_string(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
            ResolvePackagesInput,
            PackageKitResult,
        >(
            "Resolve",
            op_state_store::SideEffect::Read,
            true,
            "packagekit.read",
            "obs.software.packagekit.package.resolve@v1",
        ),
    );
    schema.methods.insert(
        "install_packages".to_string(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
            InstallPackagesInput,
            PackageKitResult,
        >(
            "InstallPackages",
            op_state_store::SideEffect::Mutation,
            false,
            "packagekit.admin",
            "mut.software.packagekit.package.install@v1",
        ),
    );
    schema.methods.insert(
        "remove_packages".to_string(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
            RemovePackagesInput,
            PackageKitResult,
        >(
            "RemovePackages",
            op_state_store::SideEffect::Mutation,
            false,
            "packagekit.admin",
            "mut.software.packagekit.package.remove@v1",
        ),
    );
    schema.methods.insert(
        "update_packages".to_string(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
            UpdatePackagesInput,
            PackageKitResult,
        >(
            "UpdatePackages",
            op_state_store::SideEffect::Mutation,
            false,
            "packagekit.admin",
            "mut.software.packagekit.package.update@v1",
        ),
    );
    schema.methods.insert(
        "get_updates".to_string(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
            GetUpdatesInput,
            PackageKitResult,
        >(
            "GetUpdates",
            op_state_store::SideEffect::Read,
            true,
            "packagekit.read",
            "obs.software.packagekit.updates.get@v1",
        ),
    );
    schema.methods.insert(
        "search_names".to_string(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
            SearchNamesInput,
            PackageKitResult,
        >(
            "SearchNames",
            op_state_store::SideEffect::Read,
            true,
            "packagekit.read",
            "obs.software.packagekit.names.search@v1",
        ),
    );

    schema.capabilities.insert(
        "packagekit.read".to_string(),
        op_state_store::CapabilityDecl {
            id: "packagekit.read".to_string(),
            description: "Grants: resolve, get_updates, search_names.".to_string(),
        },
    );
    schema.capabilities.insert(
        "packagekit.admin".to_string(),
        op_state_store::CapabilityDecl {
            id: "packagekit.admin".to_string(),
            description: "Grants: install_packages, remove_packages, update_packages.".to_string(),
        },
    );

    schema
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::{Arc, Mutex};
    use tokio::time::{timeout, Duration};
    use zbus::{connection::Builder, object_server::SignalContext, Guid};

    const TX_PATH: &str = "/org/freedesktop/PackageKit/transactions/test1";
    type Calls = Arc<Mutex<Vec<(String, u64, Vec<String>)>>>;

    #[derive(Clone, Copy)]
    enum Mode {
        Success,
        Error,
        Failed,
        Malformed,
        NoFinished,
    }

    #[derive(Clone)]
    struct FixtureState {
        mode: Arc<Mutex<Mode>>,
        calls: Calls,
    }

    #[derive(Clone)]
    struct RootFixture {
        _state: FixtureState,
    }

    #[derive(Clone)]
    struct TransactionFixture {
        state: FixtureState,
    }

    impl TransactionFixture {
        async fn run(
            &self,
            ctxt: SignalContext<'_>,
            method: &str,
            flags: u64,
            ids: Vec<String>,
        ) -> zbus::fdo::Result<()> {
            self.state
                .calls
                .lock()
                .unwrap()
                .push((method.to_string(), flags, ids));
            let mode = *self.state.mode.lock().unwrap();
            match mode {
                Mode::Malformed => {
                    ctxt.connection()
                        .emit_signal(
                            None::<&str>,
                            ctxt.path(),
                            "org.freedesktop.PackageKit.Transaction",
                            "Package",
                            &(1u32,),
                        )
                        .await?;
                }
                Mode::Error => {
                    Self::error_code(&ctxt, 42, "fixture failure").await?;
                    Self::finished(&ctxt, 1, 2).await?;
                    return Ok(());
                }
                Mode::Success | Mode::Failed => {
                    Self::package(&ctxt, 1, "fixture;1;x86_64;repo", "fixture package").await?;
                }
                Mode::NoFinished => return Ok(()),
            }
            if !matches!(mode, Mode::Malformed) {
                let exit = if matches!(mode, Mode::Failed) { 2 } else { 1 };
                Self::finished(&ctxt, exit, 7).await?;
            }
            Ok(())
        }
    }

    #[zbus::interface(name = "org.freedesktop.PackageKit")]
    impl RootFixture {
        async fn create_transaction(&self) -> OwnedObjectPath {
            OwnedObjectPath::try_from(TX_PATH).unwrap()
        }
    }

    #[zbus::interface(name = "org.freedesktop.PackageKit.Transaction")]
    impl TransactionFixture {
        async fn resolve(
            &self,
            #[zbus(signal_context)] ctxt: SignalContext<'_>,
            filters: u64,
            packages: Vec<String>,
        ) -> zbus::fdo::Result<()> {
            self.run(ctxt, "Resolve", filters, packages).await
        }
        async fn install_packages(
            &self,
            #[zbus(signal_context)] ctxt: SignalContext<'_>,
            flags: u64,
            packages: Vec<String>,
        ) -> zbus::fdo::Result<()> {
            self.run(ctxt, "InstallPackages", flags, packages).await
        }
        async fn remove_packages(
            &self,
            #[zbus(signal_context)] ctxt: SignalContext<'_>,
            flags: u64,
            packages: Vec<String>,
            _allow: bool,
            _autoremove: bool,
        ) -> zbus::fdo::Result<()> {
            self.run(ctxt, "RemovePackages", flags, packages).await
        }
        async fn update_packages(
            &self,
            #[zbus(signal_context)] ctxt: SignalContext<'_>,
            flags: u64,
            packages: Vec<String>,
        ) -> zbus::fdo::Result<()> {
            self.run(ctxt, "UpdatePackages", flags, packages).await
        }
        async fn get_updates(
            &self,
            #[zbus(signal_context)] ctxt: SignalContext<'_>,
            filters: u64,
        ) -> zbus::fdo::Result<()> {
            self.run(ctxt, "GetUpdates", filters, Vec::new()).await
        }
        async fn search_names(
            &self,
            #[zbus(signal_context)] ctxt: SignalContext<'_>,
            filters: u64,
            names: Vec<String>,
        ) -> zbus::fdo::Result<()> {
            self.run(ctxt, "SearchNames", filters, names).await
        }

        #[zbus(signal)]
        async fn package(
            ctxt: &SignalContext<'_>,
            info: u32,
            package_id: &str,
            summary: &str,
        ) -> zbus::Result<()>;
        #[zbus(signal)]
        async fn error_code(ctxt: &SignalContext<'_>, code: u32, details: &str)
            -> zbus::Result<()>;
        #[zbus(signal)]
        async fn finished(ctxt: &SignalContext<'_>, exit: u32, runtime: u32) -> zbus::Result<()>;
    }

    async fn fixture(mode: Mode) -> (zbus::Connection, zbus::Connection, Calls) {
        let (a, b) = tokio::net::UnixStream::pair().unwrap();
        let calls = Arc::new(Mutex::new(Vec::new()));
        let state = FixtureState {
            mode: Arc::new(Mutex::new(mode)),
            calls: calls.clone(),
        };
        let server = Builder::unix_stream(a)
            .server(Guid::generate())
            .unwrap()
            .p2p()
            .serve_at(
                "/org/freedesktop/PackageKit",
                RootFixture {
                    _state: state.clone(),
                },
            )
            .unwrap()
            .serve_at(TX_PATH, TransactionFixture { state })
            .unwrap()
            .build();
        let client = Builder::unix_stream(b).p2p().build();
        let (_server, client) = timeout(Duration::from_secs(5), async {
            tokio::try_join!(server, client)
        })
        .await
        .unwrap()
        .unwrap();
        (_server, client, calls)
    }

    fn args(method: &str) -> serde_json::Value {
        match method {
            "resolve" => json!({"filter": 9u64, "packages":["a"]}),
            "install_packages" | "update_packages" => {
                json!({"transaction_flags": 11u64, "package_ids":["a"]})
            }
            "remove_packages" => {
                json!({"transaction_flags": 12u64, "package_ids":["a"],"allow_deps":false,"autoremove":false})
            }
            "get_updates" => json!({"filter": 13u64}),
            "search_names" => json!({"filter": 14u64, "values":["a"]}),
            _ => unreachable!(),
        }
    }

    #[tokio::test]
    async fn all_six_methods_use_u64_and_ordered_package_finished() {
        for method in [
            "resolve",
            "install_packages",
            "remove_packages",
            "update_packages",
            "get_updates",
            "search_names",
        ] {
            let (_server, conn, calls) = fixture(Mode::Success).await;
            let result = dispatch_packagekit_on_connection_with_deadline(
                &conn,
                method,
                &args(method),
                Duration::from_secs(5),
            )
            .await
            .unwrap();
            assert_eq!(result.exit_code, 1);
            assert_eq!(result.packages, vec!["fixture;1;x86_64;repo"]);
            assert_eq!(result.package_details.len(), 1);
            assert_eq!(result.package_details[0].info, 1);
            assert_eq!(result.package_details[0].package_id, result.packages[0]);
            let calls = calls.lock().unwrap();
            assert_eq!(calls.len(), 1);
            let expected: u64 = match method {
                "resolve" => 9,
                "install_packages" => 11,
                "remove_packages" => 12,
                "update_packages" => 11,
                "get_updates" => 13,
                "search_names" => 14,
                _ => unreachable!(),
            };
            assert_eq!(calls[0].1, expected);
        }
    }

    #[tokio::test]
    async fn errors_failed_exit_malformed_and_missing_finished_fail() {
        for mode in [Mode::Error, Mode::Failed, Mode::Malformed, Mode::NoFinished] {
            let (_server, conn, _) = fixture(mode).await;
            assert!(dispatch_packagekit_on_connection_with_deadline(
                &conn,
                "resolve",
                &args("resolve"),
                Duration::from_millis(100)
            )
            .await
            .is_err());
        }
    }
}
