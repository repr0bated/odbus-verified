use anyhow::{Context, Result};
use async_trait::async_trait;
use op_state::{ApplyResult, Checkpoint, DiffMetadata, PluginCapabilities, StateDiff, StatePlugin};
use op_state_store::{CapabilityDecl, PluginSchema};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use simd_json::OwnedValue as Value;
use simd_json::prelude::*;

use super::plugin_scaffold_helpers::method_decl_from_schemars_with_output;

#[derive(Debug, Clone, Serialize, Deserialize, Default, schemars::JsonSchema)]
pub struct SoftwareState {
    #[serde(default)]
    pub packages: Vec<PackageInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PackageInfo {
    pub name: String,
    pub version: String,
    pub architecture: String,
    pub package_id: String,
    pub status: String,
    pub manager: String,
}

/// Input struct for Install method.
/// See: https://www.freedesktop.org/software/systemd/man/org.freedesktop.systemd1.html
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct InstallInput {
    /// Package name to install
    pub name: String,
    /// Package version (optional)
    pub version: Option<String>,
    #[serde(default)]
    pub architecture: Option<String>,
}

/// Input struct for Uninstall method.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct UninstallInput {
    /// Package name to uninstall
    pub name: String,
    /// Force removal even if dependent
    #[serde(default)]
    pub force: bool,
    #[serde(default)]
    pub architecture: Option<String>,
}

/// Input struct for Update method.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct UpdateInput {
    /// Package name to update
    pub name: String,
    /// Target version (optional, defaults to latest)
    pub version: Option<String>,
    #[serde(default)]
    pub architecture: Option<String>,
}

/// Input struct for GetInfo method.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct GetInfoInput {
    /// Package name to query
    pub name: String,
    #[serde(default)]
    pub architecture: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PackageMutationOutput {
    pub operation: String,
    pub package_id: String,
    pub transaction: String,
    pub exit_code: u32,
    pub runtime_ms: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct GetInfoOutput {
    pub packages: Vec<PackageInfo>,
    pub candidates: Vec<String>,
    pub transaction: String,
}

/// Published schema state for the `software` plugin.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "sch.software.plugin.software.schema@v1"))]
#[schemars(extend("x-oscal-category" = "software"))]
pub struct SoftwareSchemaState {
    #[schemars(
        description = "Package list",
        extend("x-oscal-subid" = "obs.software.plugin.software.packages@v1")
    )]
    pub packages: Vec<PackageInfo>,
}

pub struct SoftwarePlugin;

impl Default for SoftwarePlugin {
    fn default() -> Self {
        Self
    }
}

impl SoftwarePlugin {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl StatePlugin for SoftwarePlugin {
    fn name(&self) -> &str {
        "software"
    }

    fn version(&self) -> &str {
        "2.0.0"
    }

    fn schema(&self) -> Option<op_state_store::PluginSchema> {
        Some(software_schema())
    }

    async fn calculate_diff(&self, _current: &Value, _desired: &Value) -> Result<StateDiff> {
        Ok(StateDiff {
            plugin: self.name().to_string(),
            actions: vec![],
            metadata: DiffMetadata {
                timestamp: chrono::Utc::now().timestamp(),
                current_hash: "unknown".to_string(),
                desired_hash: "unknown".to_string(),
            },
        })
    }

    async fn apply_state(&self, _diff: &StateDiff) -> Result<ApplyResult> {
        Ok(ApplyResult {
            success: true,
            changes_applied: vec![],
            errors: vec![],
            checkpoint: None,
        })
    }

    async fn verify_state(&self, _desired: &Value) -> Result<bool> {
        Ok(true)
    }

    async fn create_checkpoint(&self) -> Result<Checkpoint> {
        Ok(Checkpoint {
            id: uuid::Uuid::new_v4().to_string(),
            plugin: self.name().to_string(),
            timestamp: chrono::Utc::now().timestamp(),
            state_snapshot: Value::null(),
            backend_checkpoint: None,
        })
    }

    async fn rollback(&self, _checkpoint: &Checkpoint) -> Result<()> {
        Ok(())
    }

    fn capabilities(&self) -> PluginCapabilities {
        PluginCapabilities {
            supports_rollback: true,
            supports_checkpoints: true,
            supports_verification: true,
            atomic_operations: false,
        }
    }
}

// PackageKit filter values are bitfields: each PkFilterEnum ordinal selects a bit.
// Source: PackageKit lib/pk-enum.h and org.freedesktop.PackageKit.Transaction.xml.
const FILTER_NONE: u64 = 1 << 1;
const FILTER_INSTALLED: u64 = 1 << 2;
const FILTER_NOT_INSTALLED: u64 = 1 << 3;
const FILTER_NEWEST: u64 = 1 << 16;

const INFO_INSTALLED: u32 = 1;
const INFO_AVAILABLE: u32 = 2;
const INFO_LOW: u32 = 3;
const INFO_ENHANCEMENT: u32 = 4;
const INFO_NORMAL: u32 = 5;
const INFO_BUGFIX: u32 = 6;
const INFO_IMPORTANT: u32 = 7;
const INFO_SECURITY: u32 = 8;
const INFO_BLOCKED: u32 = 9;
const INFO_CRITICAL: u32 = 25;

#[derive(Debug, Clone)]
struct PackageIdParts {
    name: String,
    version: String,
    architecture: String,
    data: String,
}

#[derive(Clone, Copy)]
enum CandidateKind {
    Installed,
    Available,
    Update,
}

fn validate_name(name: &str) -> Result<()> {
    if name.is_empty()
        || name.contains(';')
        || name.chars().any(char::is_control)
        || name.trim() != name
    {
        anyhow::bail!("invalid package name")
    }
    Ok(())
}

fn package_parts(id: &str) -> Result<PackageIdParts> {
    let fields = id.split(';').collect::<Vec<_>>();
    if fields.len() != 4
        || fields[0].is_empty()
        || fields[1].is_empty()
        || fields[2].is_empty()
        || fields[3].is_empty()
        || fields
            .iter()
            .any(|field| field.chars().any(char::is_control))
    {
        anyhow::bail!("invalid PackageKit package ID: {id:?}")
    }
    Ok(PackageIdParts {
        name: fields[0].to_string(),
        version: fields[1].to_string(),
        architecture: fields[2].to_string(),
        data: fields[3].to_string(),
    })
}

fn is_update_info(info: u32) -> bool {
    matches!(
        info,
        INFO_LOW
            | INFO_ENHANCEMENT
            | INFO_NORMAL
            | INFO_BUGFIX
            | INFO_IMPORTANT
            | INFO_SECURITY
            | INFO_CRITICAL
    )
}

fn candidate_matches(
    candidate: &super::packagekit::PackageKitPackage,
    parts: &PackageIdParts,
    kind: CandidateKind,
) -> bool {
    match kind {
        CandidateKind::Installed => {
            candidate.info == INFO_INSTALLED && parts.data.starts_with("installed")
        }
        CandidateKind::Available => {
            candidate.info == INFO_AVAILABLE && !parts.data.starts_with("installed")
        }
        CandidateKind::Update => is_update_info(candidate.info),
    }
}

async fn resolve_one(
    conn: &zbus::Connection,
    name: &str,
    version: Option<&str>,
    architecture: Option<&str>,
    filter: u64,
    kind: CandidateKind,
) -> Result<String> {
    validate_name(name)?;
    let response = super::packagekit::dispatch_packagekit_method_with_connection(
        conn,
        "resolve",
        &serde_json::json!({"filter": filter, "packages": [name]}),
    )
    .await?;
    let response: super::packagekit::PackageKitResult =
        serde_json::from_value(response).context("decode PackageKit resolve result")?;
    select_package_id(&response.package_details, name, version, architecture, kind)
}

fn select_package_id(
    candidates: &[super::packagekit::PackageKitPackage],
    name: &str,
    version: Option<&str>,
    architecture: Option<&str>,
    kind: CandidateKind,
) -> Result<String> {
    validate_name(name)?;
    let mut matches = Vec::new();
    for candidate in candidates {
        let parts = package_parts(&candidate.package_id)?;
        if parts.name == name
            && version.is_none_or(|value| value == parts.version)
            && architecture.is_none_or(|value| value == parts.architecture)
            && candidate_matches(candidate, &parts, kind)
        {
            matches.push(candidate.package_id.clone());
        }
    }
    matches.sort();
    matches.dedup();
    if matches.len() != 1 {
        anyhow::bail!("PackageKit selection for {name} is ambiguous or unavailable")
    }
    Ok(matches.remove(0))
}

fn mutation_output(operation: &str, package_id: String, response: JsonValue) -> Result<JsonValue> {
    let receipt: super::packagekit::PackageKitResult =
        serde_json::from_value(response).context("decode completed PackageKit transaction")?;
    Ok(serde_json::to_value(PackageMutationOutput {
        operation: operation.to_string(),
        package_id,
        transaction: receipt.transaction,
        exit_code: receipt.exit_code,
        runtime_ms: receipt.runtime_ms,
    })?)
}

pub async fn dispatch_software_method(method: &str, args: &JsonValue) -> Result<JsonValue> {
    let conn = zbus::Connection::system()
        .await
        .context("connect to PackageKit system bus")?;
    dispatch_software_method_with_connection(&conn, method, args).await
}

pub(crate) async fn dispatch_software_method_with_connection(
    conn: &zbus::Connection,
    method: &str,
    args: &JsonValue,
) -> Result<JsonValue> {
    match method {
        "get_info" => {
            let input: GetInfoInput = serde_json::from_value(args.clone())?;
            validate_name(&input.name)?;
            let response = super::packagekit::dispatch_packagekit_method_with_connection(
                conn,
                "resolve",
                &serde_json::json!({"filter": FILTER_NONE, "packages": [input.name]}),
            )
            .await?;
            let response: super::packagekit::PackageKitResult =
                serde_json::from_value(response).context("decode PackageKit resolve result")?;
            let mut packages = Vec::new();
            let mut candidates = Vec::new();
            for candidate in response.package_details {
                let parts = package_parts(&candidate.package_id)?;
                if parts.name == input.name
                    && input
                        .architecture
                        .as_deref()
                        .is_none_or(|value| value == parts.architecture)
                {
                    let status = if candidate.info == INFO_INSTALLED
                        && parts.data.starts_with("installed")
                    {
                        "installed"
                    } else if candidate.info == INFO_AVAILABLE
                        && !parts.data.starts_with("installed")
                    {
                        "available"
                    } else if candidate.info == INFO_BLOCKED {
                        "blocked"
                    } else if is_update_info(candidate.info) {
                        "update"
                    } else {
                        "other"
                    };
                    candidates.push(candidate.package_id.clone());
                    packages.push(PackageInfo {
                        name: parts.name,
                        version: parts.version,
                        architecture: parts.architecture,
                        package_id: candidate.package_id,
                        status: status.to_string(),
                        manager: "packagekit".into(),
                    });
                }
            }
            if packages.is_empty() {
                anyhow::bail!("PackageKit returned no matching package")
            }
            packages.sort_by(|left, right| left.package_id.cmp(&right.package_id));
            candidates.sort();
            candidates.dedup();
            Ok(serde_json::to_value(GetInfoOutput {
                packages,
                candidates,
                transaction: response.transaction,
            })?)
        }
        "install" => {
            let input: InstallInput = serde_json::from_value(args.clone())?;
            let filter = if input.version.is_some() {
                FILTER_NOT_INSTALLED
            } else {
                FILTER_NOT_INSTALLED | FILTER_NEWEST
            };
            let id = resolve_one(
                conn,
                &input.name,
                input.version.as_deref(),
                input.architecture.as_deref(),
                filter,
                CandidateKind::Available,
            )
            .await?;
            let receipt = super::packagekit::dispatch_packagekit_method_with_connection(
                conn,
                "install_packages",
                &serde_json::json!({"transaction_flags":0u64,"package_ids":[id]}),
            )
            .await?;
            mutation_output(method, id, receipt)
        }
        "uninstall" => {
            let input: UninstallInput = serde_json::from_value(args.clone())?;
            let id = resolve_one(
                conn,
                &input.name,
                None,
                input.architecture.as_deref(),
                FILTER_INSTALLED,
                CandidateKind::Installed,
            )
            .await?;
            let receipt = super::packagekit::dispatch_packagekit_method_with_connection(conn, "remove_packages", &serde_json::json!({"transaction_flags":0u64,"package_ids":[id],"allow_deps":input.force,"autoremove":false})).await?;
            mutation_output(method, id, receipt)
        }
        "update" => {
            let input: UpdateInput = serde_json::from_value(args.clone())?;
            validate_name(&input.name)?;
            let response = super::packagekit::dispatch_packagekit_method_with_connection(
                conn,
                "get_updates",
                &serde_json::json!({"filter": FILTER_NONE}),
            )
            .await?;
            let response: super::packagekit::PackageKitResult =
                serde_json::from_value(response).context("decode PackageKit update result")?;
            let id = select_package_id(
                &response.package_details,
                &input.name,
                input.version.as_deref(),
                input.architecture.as_deref(),
                CandidateKind::Update,
            )?;
            let receipt = super::packagekit::dispatch_packagekit_method_with_connection(
                conn,
                "update_packages",
                &serde_json::json!({"transaction_flags":0u64,"package_ids":[id]}),
            )
            .await?;
            mutation_output(method, id, receipt)
        }
        _ => anyhow::bail!("unknown software method: {method}"),
    }
}

pub(crate) fn software_schema() -> PluginSchema {
    let mut schema = super::schemars_adapter::plugin_schema_from_json(
        "software",
        "2.0.0",
        "Software package inventory",
        &serde_json::to_value(schemars::schema_for!(SoftwareSchemaState)).unwrap(),
    );
    schema.subids.insert(
        "__schema__".to_string(),
        "sch.software.plugin.software.schema@v1".to_string(),
    );
    super::schemars_adapter::apply_state_defaults(
        &mut schema,
        &simd_json::serde::to_owned_value(&SoftwareState::default()).unwrap(),
    );

    // Package operations are backed by org.freedesktop.PackageKit transactions.
    schema.methods.insert(
        "install".to_string(),
        method_decl_from_schemars_with_output::<InstallInput, PackageMutationOutput>(
            "Install",
            op_state_store::SideEffect::Mutation,
            false,
            "software.write",
            "mut.software.plugin.software.install@v1",
        ),
    );

    // Uninstall method
    schema.methods.insert(
        "uninstall".to_string(),
        method_decl_from_schemars_with_output::<UninstallInput, PackageMutationOutput>(
            "Uninstall",
            op_state_store::SideEffect::Mutation,
            false,
            "software.write",
            "mut.software.plugin.software.uninstall@v1",
        ),
    );

    // Update method
    schema.methods.insert(
        "update".to_string(),
        method_decl_from_schemars_with_output::<UpdateInput, PackageMutationOutput>(
            "Update",
            op_state_store::SideEffect::Mutation,
            false,
            "software.write",
            "mut.software.plugin.software.update@v1",
        ),
    );

    // GetInfo method
    schema.methods.insert(
        "get_info".to_string(),
        method_decl_from_schemars_with_output::<GetInfoInput, GetInfoOutput>(
            "GetInfo",
            op_state_store::SideEffect::Read,
            true,
            "software.read",
            "obs.software.plugin.software.get-info@v1",
        ),
    );

    schema.capabilities.insert(
        "software.write".to_string(),
        CapabilityDecl {
            id: "software.write".to_string(),
            description: "Grants: install, uninstall, update.".to_string(),
        },
    );
    schema.capabilities.insert(
        "software.read".to_string(),
        CapabilityDecl {
            id: "software.read".to_string(),
            description: "Grants: get_info.".to_string(),
        },
    );

    schema
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::{Arc, Mutex};
    use tokio::time::{Duration, timeout};
    use zbus::{
        Guid, connection::Builder, object_server::SignalContext, zvariant::OwnedObjectPath,
    };

    const TX_PATH: &str = "/org/freedesktop/PackageKit/transactions/software_test";
    type Calls = Arc<Mutex<Vec<RecordedCall>>>;

    #[derive(Clone, Copy)]
    enum FixtureMode {
        Normal,
        Ambiguous,
        Malformed,
        BackendError,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct RecordedCall {
        method: String,
        flags: u64,
        package_ids: Vec<String>,
        allow_deps: bool,
        autoremove: bool,
    }

    #[derive(Clone)]
    struct FixtureState {
        mode: FixtureMode,
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
        fn record(
            &self,
            method: &str,
            flags: u64,
            package_ids: Vec<String>,
            allow_deps: bool,
            autoremove: bool,
        ) {
            self.state.calls.lock().unwrap().push(RecordedCall {
                method: method.to_string(),
                flags,
                package_ids,
                allow_deps,
                autoremove,
            });
        }

        async fn finish(ctxt: &SignalContext<'_>) -> zbus::fdo::Result<()> {
            Self::finished(ctxt, 1, 17).await?;
            Ok(())
        }

        async fn emit_package(
            ctxt: &SignalContext<'_>,
            info: u32,
            package_id: &str,
        ) -> zbus::fdo::Result<()> {
            Self::package(ctxt, info, package_id, "fixture package").await?;
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
            filter: u64,
            packages: Vec<String>,
        ) -> zbus::fdo::Result<()> {
            self.record("Resolve", filter, packages, false, false);
            match self.state.mode {
                FixtureMode::Malformed => {
                    Self::emit_package(&ctxt, INFO_AVAILABLE, "malformed").await?;
                }
                FixtureMode::Ambiguous => {
                    Self::emit_package(&ctxt, INFO_AVAILABLE, "demo;2.0;x86_64;repo-a").await?;
                    Self::emit_package(&ctxt, INFO_AVAILABLE, "demo;2.0;x86_64;repo-b").await?;
                }
                FixtureMode::Normal | FixtureMode::BackendError => {
                    if filter == FILTER_INSTALLED {
                        Self::emit_package(
                            &ctxt,
                            INFO_INSTALLED,
                            "demo;1.0;x86_64;installed:fixture",
                        )
                        .await?;
                    } else if filter & FILTER_NOT_INSTALLED != 0 {
                        if filter & FILTER_NEWEST == 0 {
                            Self::emit_package(&ctxt, INFO_AVAILABLE, "demo;1.0;x86_64;fixture")
                                .await?;
                        }
                        Self::emit_package(&ctxt, INFO_AVAILABLE, "demo;2.0;x86_64;fixture")
                            .await?;
                        Self::emit_package(&ctxt, INFO_AVAILABLE, "demo;2.0;aarch64;fixture")
                            .await?;
                    } else {
                        Self::emit_package(
                            &ctxt,
                            INFO_INSTALLED,
                            "demo;1.0;x86_64;installed:fixture",
                        )
                        .await?;
                        Self::emit_package(&ctxt, INFO_AVAILABLE, "demo;2.0;x86_64;fixture")
                            .await?;
                    }
                }
            }
            Self::finish(&ctxt).await
        }

        async fn get_updates(
            &self,
            #[zbus(signal_context)] ctxt: SignalContext<'_>,
            filter: u64,
        ) -> zbus::fdo::Result<()> {
            self.record("GetUpdates", filter, Vec::new(), false, false);
            Self::emit_package(&ctxt, INFO_SECURITY, "demo;2.0;x86_64;updates").await?;
            Self::emit_package(&ctxt, INFO_BLOCKED, "demo;3.0;x86_64;updates").await?;
            Self::finish(&ctxt).await
        }

        async fn install_packages(
            &self,
            #[zbus(signal_context)] ctxt: SignalContext<'_>,
            flags: u64,
            package_ids: Vec<String>,
        ) -> zbus::fdo::Result<()> {
            self.record("InstallPackages", flags, package_ids, false, false);
            if matches!(self.state.mode, FixtureMode::BackendError) {
                Self::error_code(&ctxt, 42, "installation failed").await?;
            } else {
                Self::finish(&ctxt).await?;
            }
            Ok(())
        }

        async fn remove_packages(
            &self,
            #[zbus(signal_context)] ctxt: SignalContext<'_>,
            flags: u64,
            package_ids: Vec<String>,
            allow_deps: bool,
            autoremove: bool,
        ) -> zbus::fdo::Result<()> {
            self.record("RemovePackages", flags, package_ids, allow_deps, autoremove);
            Self::finish(&ctxt).await
        }

        async fn update_packages(
            &self,
            #[zbus(signal_context)] ctxt: SignalContext<'_>,
            flags: u64,
            package_ids: Vec<String>,
        ) -> zbus::fdo::Result<()> {
            self.record("UpdatePackages", flags, package_ids, false, false);
            Self::finish(&ctxt).await
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

    async fn fixture(mode: FixtureMode) -> (zbus::Connection, zbus::Connection, Calls) {
        let (server_stream, client_stream) = tokio::net::UnixStream::pair().unwrap();
        let calls = Arc::new(Mutex::new(Vec::new()));
        let state = FixtureState {
            mode,
            calls: calls.clone(),
        };
        let server = Builder::unix_stream(server_stream)
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
        let client = Builder::unix_stream(client_stream).p2p().build();
        let (_server, client) = timeout(Duration::from_secs(5), async {
            tokio::try_join!(server, client)
        })
        .await
        .unwrap()
        .unwrap();
        (_server, client, calls)
    }

    #[test]
    fn package_selection_requires_one_valid_exact_candidate() {
        let candidates = vec![
            super::super::packagekit::PackageKitPackage {
                info: INFO_AVAILABLE,
                package_id: "demo;1.0;x86_64;repo".to_string(),
                summary: String::new(),
            },
            super::super::packagekit::PackageKitPackage {
                info: INFO_AVAILABLE,
                package_id: "demo;2.0;x86_64;repo".to_string(),
                summary: String::new(),
            },
        ];
        assert!(
            select_package_id(&candidates, "demo", None, None, CandidateKind::Available).is_err()
        );
        assert_eq!(
            select_package_id(
                &candidates,
                "demo",
                Some("2.0"),
                Some("x86_64"),
                CandidateKind::Available
            )
            .unwrap(),
            "demo;2.0;x86_64;repo"
        );
        let malformed = [super::super::packagekit::PackageKitPackage {
            info: INFO_AVAILABLE,
            package_id: "corrupt".to_string(),
            summary: String::new(),
        }];
        assert!(
            select_package_id(&malformed, "demo", None, None, CandidateKind::Available).is_err()
        );
        assert!(select_package_id(&[], "demo", None, None, CandidateKind::Available).is_err());
    }

    #[tokio::test]
    async fn all_four_methods_use_exact_packagekit_transactions() {
        let (_server, conn, calls) = fixture(FixtureMode::Normal).await;
        let info: GetInfoOutput = serde_json::from_value(
            dispatch_software_method_with_connection(
                &conn,
                "get_info",
                &json!({"name":"demo", "architecture":"x86_64"}),
            )
            .await
            .unwrap(),
        )
        .unwrap();
        assert_eq!(info.packages.len(), 2);
        assert_eq!(info.packages[0].status, "installed");
        assert_eq!(info.packages[0].architecture, "x86_64");
        assert_eq!(info.packages[1].status, "available");

        let install: PackageMutationOutput = serde_json::from_value(
            dispatch_software_method_with_connection(
                &conn,
                "install",
                &json!({"name":"demo", "version":null, "architecture":"x86_64"}),
            )
            .await
            .unwrap(),
        )
        .unwrap();
        assert_eq!(install.package_id, "demo;2.0;x86_64;fixture");
        assert_eq!(install.exit_code, 1);

        let uninstall: PackageMutationOutput = serde_json::from_value(
            dispatch_software_method_with_connection(
                &conn,
                "uninstall",
                &json!({"name":"demo", "force":true, "architecture":"x86_64"}),
            )
            .await
            .unwrap(),
        )
        .unwrap();
        assert_eq!(uninstall.package_id, "demo;1.0;x86_64;installed:fixture");

        let update: PackageMutationOutput = serde_json::from_value(
            dispatch_software_method_with_connection(
                &conn,
                "update",
                &json!({"name":"demo", "version":null, "architecture":"x86_64"}),
            )
            .await
            .unwrap(),
        )
        .unwrap();
        assert_eq!(update.package_id, "demo;2.0;x86_64;updates");

        let calls = calls.lock().unwrap();
        assert_eq!(calls[0].method, "Resolve");
        assert_eq!(calls[0].flags, FILTER_NONE);
        assert_eq!(calls[1].flags, FILTER_NOT_INSTALLED | FILTER_NEWEST);
        assert_eq!(calls[2].method, "InstallPackages");
        assert_eq!(calls[3].flags, FILTER_INSTALLED);
        assert_eq!(calls[4].method, "RemovePackages");
        assert!(calls[4].allow_deps);
        assert!(!calls[4].autoremove);
        assert_eq!(calls[5].method, "GetUpdates");
        assert_eq!(calls[5].flags, FILTER_NONE);
        assert_eq!(calls[6].method, "UpdatePackages");
    }

    #[tokio::test]
    async fn exact_version_and_architecture_select_before_install() {
        let (_server, conn, calls) = fixture(FixtureMode::Normal).await;
        let result: PackageMutationOutput = serde_json::from_value(
            dispatch_software_method_with_connection(
                &conn,
                "install",
                &json!({"name":"demo", "version":"1.0", "architecture":"x86_64"}),
            )
            .await
            .unwrap(),
        )
        .unwrap();
        assert_eq!(result.package_id, "demo;1.0;x86_64;fixture");
        let calls = calls.lock().unwrap();
        assert_eq!(calls[0].flags, FILTER_NOT_INSTALLED);
        assert_eq!(calls[1].package_ids, ["demo;1.0;x86_64;fixture"]);
    }

    #[tokio::test]
    async fn ambiguous_and_malformed_candidates_never_reach_mutation() {
        for mode in [FixtureMode::Ambiguous, FixtureMode::Malformed] {
            let (_server, conn, calls) = fixture(mode).await;
            let result = dispatch_software_method_with_connection(
                &conn,
                "install",
                &json!({"name":"demo", "version":null, "architecture":"x86_64"}),
            )
            .await;
            assert!(result.is_err());
            let calls = calls.lock().unwrap();
            assert_eq!(calls.len(), 1);
            assert_eq!(calls[0].method, "Resolve");
        }
    }

    #[tokio::test]
    async fn backend_error_is_not_reported_as_completed_install() {
        let (_server, conn, calls) = fixture(FixtureMode::BackendError).await;
        let error = dispatch_software_method_with_connection(
            &conn,
            "install",
            &json!({"name":"demo", "version":null, "architecture":"x86_64"}),
        )
        .await
        .unwrap_err();
        assert!(error.to_string().contains("PackageKit error 42"));
        assert_eq!(calls.lock().unwrap().len(), 2);
    }

    #[test]
    fn all_subids_are_valid() {
        let raw = serde_json::to_value(schemars::schema_for!(SoftwareSchemaState)).unwrap();
        let mut subids = Vec::new();
        collect_subids(&raw, &mut subids);
        for subid in subids {
            assert!(
                crate::state_plugins::common::oscal::validate_subid(&subid).is_ok(),
                "invalid subid: {subid}"
            );
        }
    }

    fn collect_subids(value: &serde_json::Value, out: &mut Vec<String>) {
        if let Some(obj) = value.as_object() {
            if let Some(subid) = obj.get("x-oscal-subid").and_then(|v| v.as_str()) {
                out.push(subid.to_string());
            }
            for v in obj.values() {
                collect_subids(v, out);
            }
        }
        if let Some(arr) = value.as_array() {
            for v in arr {
                collect_subids(v, out);
            }
        }
    }
}

// Self-registration: the plugin registry discovers this via inventory
// (single source of the catalog; no central dispatch list).
inventory::submit! {
    crate::default_registry::PluginReg::new("software", |_ctx| std::sync::Arc::new(SoftwarePlugin::new()))
}
