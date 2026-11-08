//! Shared authenticated execution owner for the TLS door and identity-bound lungs.
//! Native effects outlive a disconnected transport, and only committed backend
//! observations enter SHM. A lost response is recovered by operation id, not replay.
use std::{collections::HashSet, future::Future, path::{Path, PathBuf}, pin::Pin,
    sync::{Arc, Mutex as StdMutex, atomic::{AtomicBool, Ordering}}};
use anyhow::Context as _;
use base64::Engine as _;
use op_cozo_store::CozoGraphShuttle;
use op_identity::sealed_id::SealedId;
use op_plugins::state_plugins::runtime_documents::{RuntimeDocuments, runtime_documents};
use op_state_store::{MethodDecl, PluginSchema};
use op_state_store::method_receipt::{MethodExecutionReceipt, UnsupportedBeforeEffect};
use serde_json::Value;
use tokio::sync::{Mutex, OwnedMutexGuard};
use uuid::Uuid;
use crate::{Catalog, core::{BackendCommit, BackendProjection, BackendStateUpdate, BridgeCore, CoreMutation, CoreOperation, MethodOperation, StateScope}, identity_receiver::VerifiedSid1};

#[async_trait::async_trait]
pub(crate) trait CallerAuthority: Send + Sync {
    async fn revalidate(&self) -> anyhow::Result<()>;
    /// Only a completed native-owned refresh may request this transition.
    /// Authorities with additional peer proofs must retain and recheck them.
    async fn refreshed(self: Arc<Self>, _refresh: &crate::runtime_identity::CredentialRefresh)
        -> anyhow::Result<Arc<dyn CallerAuthority>> {
        anyhow::bail!("this caller authority does not admit credential refresh")
    }
}

#[derive(Clone)]
pub(crate) struct VerifiedCaller {
    pub principal_id: String,
    pub session_id: String,
    pub trace_id: String,
    pub session_genesis: String,
    pub sealed_identity: Option<String>,
    pub authority: Arc<dyn CallerAuthority>,
    identity: VerifiedSid1,
    execution: Option<Arc<Execution>>,
    call_path: Vec<(String, String)>,
}

impl VerifiedCaller {
    pub(crate) fn new(identity: &VerifiedSid1, authority: Arc<dyn CallerAuthority>) -> Self {
        let claims = identity.claims();
        Self {
            principal_id: claims.principal_id.clone(), session_id: claims.session_id.clone(),
            trace_id: claims.trace_id.clone(), session_genesis: claims.session_genesis.clone(),
            sealed_identity: None, authority, identity: identity.clone(), execution: None,
            call_path: Vec::new(),
        }
    }
    pub(crate) fn claims(&self) -> &SealedId { self.identity.claims() }
    pub(crate) fn private_scope(&self) -> StateScope {
        StateScope::Private { principal_id: self.principal_id.clone(), session_id: self.session_id.clone() }
    }
    pub(crate) fn inherit_execution(&mut self, caller: &Self) {
        self.execution = caller.execution.clone();
        self.call_path = caller.call_path.clone();
    }
    pub(crate) fn for_background(&self) -> Self {
        let mut caller = Self::new(&self.identity, self.authority.clone());
        caller.sealed_identity = self.sealed_identity.clone();
        caller
    }
}

pub(crate) struct BackendContext<'a> {
    pub runtime: &'a Runtime,
    pub caller: &'a VerifiedCaller,
    pub schema: &'a PluginSchema,
    pub declaration: &'a MethodDecl,
    pub operation_id: Uuid,
    pub operation: &'a MethodOperation,
    pub request_event_id: u64,
    pub request_event_hash: String,
}

pub(crate) struct BackendOutcome {
    pub result: Value,
    pub state: BackendStateUpdate,
    pub scope: StateScope,
    pub additional: Vec<BackendProjection>,
    /// Native observation/selection ownership extends through publication.
    pub guard: Option<Box<dyn Send + Sync>>,
}

pub(crate) struct RuntimeResult {
    pub result: Value,
    pub receipt: MethodExecutionReceipt,
    pub effective_hash: String,
    /// Verified disclosure authority, with no execution gate or inherited path.
    pub disclosure_caller: VerifiedCaller,
}

/// One admitted root retains the gate until all of its nested native work ends.
/// The dispatch log prevents an outer refusal from erasing a nested effect.
struct Execution {
    _guard: OwnedMutexGuard<()>,
    dispatched: StdMutex<Vec<Uuid>>,
}

impl Execution {
    fn dispatch(&self, operation_id: Uuid, nested: bool) -> usize {
        let mut dispatched = self.dispatched.lock().expect("execution dispatch log poisoned");
        if nested { dispatched.push(operation_id); }
        dispatched.len()
    }

    fn nested_after(&self, index: usize) -> Vec<Uuid> {
        self.dispatched.lock().expect("execution dispatch log poisoned")[index..].to_vec()
    }
}

#[derive(Default)]
struct ExecutionAdmission {
    closed: AtomicBool,
    gate: Arc<Mutex<()>>,
}

impl ExecutionAdmission {
    fn require_open(&self) -> Result<(), RuntimeError> {
        if self.closed.load(Ordering::Acquire) {
            return Err(RuntimeError::denied("runtime is stopping; new operations are not admitted"));
        }
        Ok(())
    }

    async fn admit(&self) -> Result<Arc<Execution>, RuntimeError> {
        self.require_open()?;
        let guard = self.gate.clone().lock_owned().await;
        self.require_open()?;
        Ok(Arc::new(Execution { _guard: guard, dispatched: StdMutex::new(Vec::new()) }))
    }

    fn begin_shutdown(&self) { self.closed.store(true, Ordering::Release); }

    async fn drain(&self) { let _guard = self.gate.lock().await; }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ErrorKind { Denied, Unsupported, BeforeEffect, FailedBeforeEffect, Backend, Unknown }

pub(crate) struct RuntimeError {
    kind: ErrorKind,
    message: String,
    operation_id: Option<String>,
    related_operation_ids: Vec<String>,
    observed: Option<(BackendStateUpdate, StateScope)>,
    additional: Vec<BackendProjection>,
    guard: Option<Box<dyn Send + Sync>>,
    // Native errors may contain provider credentials or private query values.
    // Never expose their source chain through a transport error formatter.
    _cause: Option<anyhow::Error>,
}
impl RuntimeError {
    fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self { kind, message: message.into(), operation_id: None, related_operation_ids: Vec::new(),
            observed: None, additional: Vec::new(), guard: None, _cause: None }
    }
    pub(crate) fn denied(message: impl Into<String>) -> Self { Self::new(ErrorKind::Denied, message) }
    pub(crate) fn unsupported(message: impl Into<String>) -> Self { Self::new(ErrorKind::Unsupported, message) }
    fn unsupported_before_effect(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::BeforeEffect, message)
    }
    pub(crate) fn backend(error: anyhow::Error) -> Self {
        match error.downcast::<Self>() {
            Ok(error) => error,
            Err(error) => {
                let mut result = Self::new(ErrorKind::Backend, "native backend operation failed");
                result._cause = Some(error);
                result
            }
        }
    }
    pub(crate) fn before_effect(error: anyhow::Error) -> Self {
        // A nested runtime error is already attributed; wrapping it cannot
        // turn its child proof into proof about the outer operation.
        match error.downcast::<Self>() {
            Ok(error) => error,
            Err(error) => {
                let mut result = Self::new(ErrorKind::BeforeEffect, "native operation refused before effect");
                result._cause = Some(error);
                result
            }
        }
    }
    pub(crate) fn unknown(error: anyhow::Error) -> Self {
        let mut result = match error.downcast::<Self>() {
            Ok(error) => error,
            Err(error) => {
                let mut result = Self::new(ErrorKind::Unknown, "method_outcome_unknown");
                result._cause = Some(error);
                result
            }
        };
        result.kind = ErrorKind::Unknown;
        result.message = "method_outcome_unknown".into();
        result
    }
    pub(crate) fn is_denied(&self) -> bool { self.kind == ErrorKind::Denied }
    pub(crate) fn is_unsupported(&self) -> bool { self.kind == ErrorKind::Unsupported }
    pub(crate) fn outcome_unknown(&self) -> bool { self.kind == ErrorKind::Unknown }
    pub(crate) fn failed_before_effect(&self) -> bool { self.kind == ErrorKind::FailedBeforeEffect }
    pub(crate) fn related_operation_ids(&self) -> &[String] { &self.related_operation_ids }
    pub(crate) fn with_state(mut self, state: BackendStateUpdate, scope: StateScope) -> Self {
        self.observed = Some((state, scope)); self
    }
    pub(crate) fn with_additional(mut self, additional: Vec<BackendProjection>) -> Self {
        self.additional = additional; self
    }
    pub(crate) fn with_guard(mut self, guard: Box<dyn Send + Sync>) -> Self {
        self.guard = Some(guard); self
    }
    fn proves_before_effect(&self, operation_id: Uuid, nested_dispatch: bool) -> bool {
        self.kind == ErrorKind::BeforeEffect && !nested_dispatch && self.observed.is_none()
            && self.additional.is_empty() && self.related_operation_ids.is_empty()
            && self.operation_id.as_deref().is_none_or(|id| Uuid::parse_str(id).is_ok_and(|id| id == operation_id))
    }
    fn at_operation(mut self, operation_id: Uuid) -> Self {
        let id = operation_id.to_string();
        if let Some(previous) = self.operation_id.replace(id.clone()) {
            if previous != id && !self.related_operation_ids.contains(&previous) {
                self.related_operation_ids.push(previous);
            }
        }
        self
    }
    fn with_nested_operations(mut self, nested: Vec<Uuid>) -> Self {
        for id in nested {
            let id = id.to_string();
            if self.operation_id.as_ref() != Some(&id) && !self.related_operation_ids.contains(&id) {
                self.related_operation_ids.push(id);
            }
        }
        self
    }
}
impl std::fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)?;
        if let Some(id) = &self.operation_id { write!(f, "; operation_id={id}")?; }
        for id in &self.related_operation_ids { write!(f, "; related_operation_id={id}")?; }
        Ok(())
    }
}
impl std::fmt::Debug for RuntimeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RuntimeError").field("kind", &self.kind).field("operation_id", &self.operation_id).finish()
    }
}
impl std::error::Error for RuntimeError {}
impl From<anyhow::Error> for RuntimeError {
    fn from(error: anyhow::Error) -> Self {
        match error.downcast::<RuntimeError>() {
            Ok(error) => error,
            Err(error) => Self::backend(error),
        }
    }
}
macro_rules! backend_error {
    ($($ty:ty),+ $(,)?) => { $(impl From<$ty> for RuntimeError {
        fn from(error: $ty) -> Self { Self::backend(error.into()) }
    })+ };
}
backend_error!(serde_json::Error, simd_json::Error, tokio::task::JoinError, op_cozo_store::CozoError);

struct RuntimeInner {
    core: BridgeCore,
    catalog_dir: PathBuf,
    execution: ExecutionAdmission,
    documents: Arc<RuntimeDocuments>,
    identity_store: Arc<CozoGraphShuttle>,
    chat: crate::runtime_chat::ChatRuntime,
    workspace: crate::runtime_workspace::WorkspaceRuntime,
    cognitive: crate::runtime_cognitive::CognitiveBackend,
    network: crate::runtime_network::NetworkRuntime,
    elevation: crate::task_elevation::TaskElevation,
}

#[derive(Clone)]
pub(crate) struct Runtime { inner: Arc<RuntimeInner> }

impl Runtime {
    pub(crate) async fn open(catalog: Catalog, catalog_dir: PathBuf, database_path: PathBuf) -> anyhow::Result<Self> {
        let documents = runtime_documents().await?;
        let identity_path = std::env::var_os("IDENTITY_SLED_COZO_DB_PATH")
            .map(PathBuf::from).unwrap_or_else(|| "/var/lib/op-dbus/identity-cozo".into());
        let identity_store = Arc::new(tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
            anyhow::ensure!(identity_path.is_dir(), "configured durable identity owner is absent");
            Ok(CozoGraphShuttle::new_persistent_private(identity_path)?)
        }).await??);
        identity_store.ensure_distinct_from(&documents.shared_store())?;
        crate::runtime_identity::recover_startup(identity_store.clone(), catalog_dir.clone()).await?;
        let core = BridgeCore::open(catalog, database_path)?;
        let workspace = crate::runtime_workspace::WorkspaceRuntime::open(core.catalog(), &catalog_dir).await?;
        let chat = crate::runtime_chat::ChatRuntime::open().await?;
        chat.ensure_distinct_from(&identity_store)?;
        chat.ensure_distinct_from(&documents.shared_store())?;
        let cognitive = crate::runtime_cognitive::CognitiveBackend::new(identity_store.clone(), documents.clone());
        let network = crate::runtime_network::NetworkRuntime::new(documents.clone());
        Ok(Self { inner: Arc::new(RuntimeInner { core, catalog_dir, execution: ExecutionAdmission::default(), documents, identity_store, chat, workspace, cognitive, network, elevation: crate::task_elevation::TaskElevation::default() }) })
    }
    pub(crate) fn core(&self) -> &BridgeCore { &self.inner.core }
    pub(crate) fn catalog(&self) -> &Catalog { self.core().catalog() }
    pub(crate) fn catalog_dir(&self) -> &Path { &self.inner.catalog_dir }
    pub(crate) fn documents(&self) -> &Arc<RuntimeDocuments> { &self.inner.documents }
    pub(crate) fn identity_store(&self) -> &Arc<CozoGraphShuttle> { &self.inner.identity_store }
    pub(crate) fn workspace(&self) -> &crate::runtime_workspace::WorkspaceRuntime { &self.inner.workspace }
    pub(crate) fn chat(&self) -> &crate::runtime_chat::ChatRuntime { &self.inner.chat }
    pub(crate) fn network(&self) -> &crate::runtime_network::NetworkRuntime { &self.inner.network }
    pub(crate) fn cognitive(&self) -> &crate::runtime_cognitive::CognitiveBackend { &self.inner.cognitive }
    pub(crate) fn task_elevation(&self) -> &crate::task_elevation::TaskElevation { &self.inner.elevation }
    /// Close root admission synchronously; inherited executions remain valid.
    pub(crate) fn begin_shutdown(&self) { self.inner.execution.begin_shutdown(); }

    /// Wait for admitted native roots and their nested calls without aborting them.
    pub(crate) async fn drain_admitted(&self) { self.inner.execution.drain().await; }

    /// A committed effect is never recast as a before-effect refusal when its
    /// original caller loses the exact sealed capability before disclosure.
    pub(crate) async fn revalidate_result(&self, result: &RuntimeResult, plugin: &str, method: &str)
        -> Result<(), RuntimeError> {
        let operation_id = Uuid::parse_str(&result.receipt.operation_id).map_err(|error| RuntimeError::unknown(error.into()))?;
        result.receipt.validate_attribution(plugin, method)
            .map_err(|error| RuntimeError::unknown(error).at_operation(operation_id))?;
        self.authorize_nested(&result.disclosure_caller, plugin, method).await
            .map_err(|error| RuntimeError::unknown(error.into()).at_operation(operation_id))
    }

    pub(crate) async fn identity_wire(&self, caller: &VerifiedCaller) -> Result<Vec<u8>, RuntimeError> {
        caller.authority.revalidate().await.map_err(|_| RuntimeError::denied("identity authority is no longer valid"))?;
        crate::identity_receiver::authored_transport_wire(&caller.identity).await.map_err(|_| RuntimeError::denied("authored identity credential is unavailable"))
    }
    pub(crate) async fn current_grants(&self, caller: &VerifiedCaller) -> Result<HashSet<String>, RuntimeError> {
        caller.authority.revalidate().await.map_err(|_| RuntimeError::denied("identity authority is no longer valid"))?;
        let principal = caller.principal_id.clone();
        tokio::task::spawn_blocking(move || {
            let document = crate::admission::read_grant_document()?;
            Ok(crate::grant_document::capabilities_for_principal(&document, &principal))
        }).await.map_err(RuntimeError::from)?
    }
    pub(crate) async fn require_capability(&self, caller: &VerifiedCaller, capability: &str) -> Result<(), RuntimeError> {
        if capability.is_empty() || !self.current_grants(caller).await?.contains(capability) {
            return Err(RuntimeError::denied("current exact-principal capability is required"));
        }
        Ok(())
    }
    fn pinned_method(&self, plugin: &str, method: &str) -> Result<(crate::sealed_schema_reader::ManifestPinnedSchemaDocument, MethodDecl), RuntimeError> {
        let pinned = crate::sealed_schema_reader::read_manifest_pinned_schema_document(self.catalog_dir(), plugin)?;
        let expected = self.catalog().plugin(plugin).ok_or_else(|| RuntimeError::denied("plugin is absent from the runtime catalog"))?;
        if expected.schema_hash != pinned.schema_hash { return Err(RuntimeError::denied("sealed plugin changed; reopen the runtime")); }
        let required = pinned.schema.methods.get(method).ok_or_else(|| RuntimeError::unsupported("method is absent from the sealed plugin"))?.required_capability.as_deref();
        let declaration = crate::method_contract_admission::admit_pinned_compiled_method(&pinned, plugin, method, required)?;
        Ok((pinned, declaration))
    }
    pub(crate) async fn authorize_nested(&self, caller: &VerifiedCaller, plugin: &str, method: &str) -> Result<(), RuntimeError> {
        let (_, declaration) = self.pinned_method(plugin, method)?;
        let required = declaration.required_capability.as_deref().filter(|s| !s.is_empty())
            .ok_or_else(|| RuntimeError::denied("sealed method has no exact capability"))?;
        self.require_capability(caller, required).await
    }
    pub(crate) async fn can_read_plugin(&self, caller: &VerifiedCaller, plugin: &str) -> Result<bool, RuntimeError> {
        let grants = self.current_grants(caller).await?;
        let pinned = crate::sealed_schema_reader::read_manifest_pinned_schema_document(self.catalog_dir(), plugin)?;
        if self.catalog().plugin(plugin).map(|p| p.schema_hash.as_str()) != Some(pinned.schema_hash.as_str()) { return Ok(false); }
        Ok(pinned.schema.methods.values().any(|method| {
            method.side_effect == op_state_store::SideEffect::Read
                && method.required_capability.as_ref().is_some_and(|cap| grants.contains(cap))
        }))
    }

    pub(crate) fn call<'a>(&'a self, caller: VerifiedCaller, plugin: &'a str, method: &'a str, args: Value, operation_id: Uuid)
        -> Pin<Box<dyn Future<Output = Result<RuntimeResult, RuntimeError>> + Send + 'a>> {
        Box::pin(async move {
            // A transport dropping its waiter does not cancel a submitted native effect.
            let runtime = self.clone(); let plugin = plugin.to_owned(); let method = method.to_owned();
            tokio::spawn(async move { runtime.call_owned(caller, plugin, method, args, operation_id).await })
                .await.map_err(|error| RuntimeError::unknown(error.into()).at_operation(operation_id))?
                .map_err(|error| error.at_operation(operation_id))
        })
    }
    pub(crate) async fn call_nested(&self, caller: &VerifiedCaller, plugin: &str, method: &str, args: Value) -> Result<RuntimeResult, RuntimeError> {
        self.call(caller.clone(), plugin, method, args, Uuid::new_v4()).await
    }

    async fn call_owned(&self, mut caller: VerifiedCaller, plugin: String, method: String, args: Value, operation_id: Uuid) -> Result<RuntimeResult, RuntimeError> {
        if caller.execution.is_none() { self.inner.execution.require_open()?; }
        self.authorize_nested(&caller, &plugin, &method).await?;
        if caller.call_path.len() >= 16 || caller.call_path.iter().any(|(p, m)| p == &plugin && m == &method) {
            return Err(RuntimeError::denied("recursive native operation is not admitted"));
        }
        caller.call_path.push((plugin.clone(), method.clone()));
        if caller.execution.is_none() { caller.execution = Some(self.inner.execution.admit().await?); }
        // The effect boundary is after queueing, not the transport's first check.
        self.authorize_nested(&caller, &plugin, &method).await?;
        let (pinned, declaration) = self.pinned_method(&plugin, &method)?;
        op_state_store::method_receipt::validate_json_size(&args, op_state_store::method_receipt::MAX_METHOD_ARGUMENT_BYTES)?;
        crate::method_contract_admission::validate_admitted_arguments(&declaration, &args)?;
        caller.sealed_identity = Some(format!("{}{}", op_identity::sealed_id::INLINE_PREFIX,
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(self.identity_wire(&caller).await?)));
        crate::runtime_identity::authorize_protected(self, &caller, &plugin, &method, &args).await?;
        let audit_value = if plugin == "elevation" && method == "authorize_task" {
            crate::task_elevation::audit_arguments(&args)
        } else {
            args.clone()
        };
        let mutation = CoreMutation {
            plugin_id: plugin.clone(), object_path: format!("/org/opdbus/v1/plugins/{plugin}"),
            operation: CoreOperation::CallMethod, member_name: method.clone(), value: audit_value,
            actor_id: caller.principal_id.clone(), capability_id: declaration.required_capability.clone().unwrap_or_default(),
            idempotency_key: operation_id.to_string(),
        };
        let operation = self.core().begin_operation(&operation_id.to_string(), &caller.principal_id, &caller.session_id, &mutation, &pinned.schema_hash)
            .map_err(|error| RuntimeError::unknown(error).at_operation(operation_id))?;
        let ctx = BackendContext { runtime: self, caller: &caller, schema: &pinned.schema, declaration: &declaration,
            operation_id, request_event_id: operation.attempt().event_id, request_event_hash: operation.attempt().event_hash.clone(), operation: &operation };
        let execution = caller.execution.as_ref().expect("admitted execution");
        let dispatch_index = execution.dispatch(operation_id, caller.call_path.len() > 1);
        let correlate = |error: RuntimeError| {
            error.with_nested_operations(execution.nested_after(dispatch_index)).at_operation(operation_id)
        };
        let outcome = match self.dispatch(&ctx, &plugin, &method, &args).await {
            Ok(outcome) => outcome,
            Err(mut error) => {
                let nested = execution.nested_after(dispatch_index);
                let before_effect = error.proves_before_effect(operation_id, !nested.is_empty());
                let mut updates = std::mem::take(&mut error.additional);
                if let Some((state, scope)) = error.observed.take() {
                    updates.push(BackendProjection { plugin_id: plugin.clone(), object_path: mutation.object_path.clone(), schema_hash: pinned.schema_hash.clone(), state, scope });
                }
                // Empty observations are not evidence of no effect: a wrapper
                // may have already committed a nested operation.
                let recorded = if before_effect {
                    self.core().fail_operation_before_effect_with_updates(&operation,
                        UnsupportedBeforeEffect::new("native_operation_refused", "native owner refused before effect"), updates)
                } else {
                    self.core().outcome_unknown_with_updates(&operation, updates)
                };
                if before_effect && recorded.is_ok() {
                    error.kind = ErrorKind::FailedBeforeEffect;
                    error.message = "method_failed_before_effect".into();
                } else {
                    error.kind = ErrorKind::Unknown;
                    error.message = "method_outcome_unknown".into();
                }
                return Err(error.with_nested_operations(nested).at_operation(operation_id));
            }
        };
        let BackendOutcome { result, state, scope, additional, guard: _guard } = outcome;
        if let Err(error) = crate::method_contract_admission::validate_admitted_output(&declaration, &result) {
            let mut updates = additional;
            updates.push(BackendProjection { plugin_id: plugin, object_path: mutation.object_path.clone(), schema_hash: pinned.schema_hash, state, scope });
            let _ = self.core().outcome_unknown_with_updates(&operation, updates);
            return Err(correlate(RuntimeError::unknown(error)));
        }
        let refreshed = plugin == "identity_sled" && method == "refresh_credential"
            && result.get("refreshed").and_then(Value::as_bool) == Some(true);
        let refresh_publication = matches!(&state, BackendStateUpdate::IdentityPublished(_));
        let committed_scope = if plugin == "identity_sled" { StateScope::Public } else { scope.clone() };
        let committed_path = mutation.object_path.clone();
        let commit = BackendCommit { mutation, scope, schema_hash: pinned.schema_hash, state, result: Some(result.clone()) };
        let receipt = self.core().complete_operation_with_updates(&operation, commit, additional)
            .map_err(|error| correlate(RuntimeError::unknown(error)))?;
        let effective_hash = self.core().committed_snapshot(&committed_scope, &plugin, &committed_path)
            .map_err(|error| correlate(RuntimeError::unknown(error)))?
            .map(|snapshot| snapshot.effective_hash).unwrap_or_default();
        let post_commit = async {
            self.workspace().after_commit(&ctx, &plugin, &method, &result).await?;
            self.network().after_commit(&ctx, &plugin, &method, &result).await
        }.await;
        if let Err(error) = post_commit {
            return Err(correlate(RuntimeError::unknown(error.into())));
        }
        let disclosure_caller = if refreshed {
            if !refresh_publication {
                return Err(correlate(RuntimeError::unknown(anyhow::anyhow!("refresh lacks its paired native publication"))));
            }
            crate::runtime_identity::refreshed_caller(&caller, &receipt).await
                .map_err(|error| correlate(RuntimeError::unknown(error)))?
        } else {
            caller.for_background()
        };
        let completed = RuntimeResult { result, receipt, effective_hash, disclosure_caller };
        self.revalidate_result(&completed, &plugin, &method).await.map_err(correlate)?;
        Ok(completed)
    }

    async fn dispatch(&self, ctx: &BackendContext<'_>, plugin: &str, method: &str, args: &Value) -> Result<BackendOutcome, RuntimeError> {
        if plugin == "incus" && matches!(method, "create_instance" | "reconcile_creation") {
            return crate::runtime_creation::dispatch(ctx, method, args).await;
        }
        if crate::runtime_platform::supports(plugin, method) { return crate::runtime_platform::dispatch(ctx, plugin, method, args).await; }
        if crate::runtime_chat::supports(plugin, method) { return self.chat().dispatch(ctx, plugin, method, args).await; }
        if crate::runtime_cognitive::supports(plugin, method) { return self.inner.cognitive.dispatch(ctx, plugin, method, args).await; }
        if crate::runtime_workspace::supports(plugin, method) { return crate::runtime_workspace::dispatch(ctx, plugin, method, args).await; }
        if crate::runtime_network::supports(plugin, method) { return self.network().dispatch(ctx, plugin, method, args).await; }
        if plugin == "elevation" {
            let result = crate::task_elevation::dispatch(self, ctx.caller, method, args).await?;
            return Ok(BackendOutcome { result, state: BackendStateUpdate::Unchanged,
                scope: ctx.caller.private_scope(), additional: Vec::new(), guard: None });
        }
        if crate::runtime_identity_owner::supports(plugin, method) {
            return crate::runtime_identity_owner::dispatch(ctx, plugin, method, args).await;
        }
        if crate::runtime_misc::supports(plugin, method) {
            return crate::runtime_misc::dispatch(ctx, plugin, method, args).await;
        }
        Err(RuntimeError::unsupported_before_effect("sealed method has no retained native owner"))
    }

    /// Called only with a backend-observed state and its native ordering guard.
    /// Taking the outer execution gate here would invert observer/native locks.
    pub(crate) async fn publish_backend_state(&self, caller: &VerifiedCaller, plugin: &str, state: BackendStateUpdate, scope: StateScope) -> Result<(), RuntimeError> {
        caller.authority.revalidate().await.map_err(|_| RuntimeError::denied("observation owner is no longer valid"))?;
        if let StateScope::Private { principal_id, session_id } = &scope {
            if principal_id != &caller.principal_id || session_id != &caller.session_id { return Err(RuntimeError::denied("observation private owner mismatch")); }
        }
        let pinned = crate::sealed_schema_reader::read_manifest_pinned_schema_document(self.catalog_dir(), plugin)?;
        crate::method_contract_admission::admit_compiled_state_contract(&pinned, plugin)?;
        if self.catalog().plugin(plugin).map(|entry| entry.schema_hash.as_str()) != Some(pinned.schema_hash.as_str()) { return Err(RuntimeError::denied("observation seal changed")); }
        let mutation = CoreMutation { plugin_id: plugin.into(), object_path: format!("/org/opdbus/v1/plugins/{plugin}"),
            operation: CoreOperation::CallMethod, member_name: "committed_observation".into(), value: Value::Null,
            actor_id: caller.principal_id.clone(), capability_id: String::new(), idempotency_key: String::new() };
        self.core().commit_backend_state(BackendCommit { mutation, scope, schema_hash: pinned.schema_hash, state, result: None })
            .map_err(|error| RuntimeError::unknown(anyhow::anyhow!("{}", error.message)))?;
        Ok(())
    }
    pub(crate) async fn configured_responder(&self, caller: &VerifiedCaller) -> Result<VerifiedCaller, RuntimeError> {
        crate::runtime_identity::configured_responder(self, caller).await
    }
    pub(crate) async fn create_project_container(&self, caller: &VerifiedCaller, operation_id: &str,
        project: &op_plugins::state_plugins::project::Project, image_fingerprint: &str, pool: &str)
        -> Result<op_cozo_store::NativeCreationRecord, RuntimeError> {
        crate::runtime_creation::create_project_container(self, caller, operation_id, project, image_fingerprint, pool).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct RefusedAuthority;
    #[async_trait::async_trait]
    impl CallerAuthority for RefusedAuthority {
        async fn revalidate(&self) -> anyhow::Result<()> { anyhow::bail!("fixture authority refused") }
    }

    fn caller_fixture() -> VerifiedCaller {
        let directory = tempfile::tempdir().unwrap();
        let catalog = directory.path().join("catalog");
        let state = directory.path().join("state");
        let state = state.to_str().unwrap();
        let claims = SealedId {
            principal_id: op_identity::session::derive_principal_id("fixture-key"),
            principal_kind: "wireguard-principal".into(),
            session_id: op_identity::session::derive_session_id("fixture-key"),
            wireguard_pubkey: "fixture-key".into(), session_genesis: "genesis".into(),
            trace_id: "trace".into(), schema_version: 3, issued_at: 100, expires_at: 0,
            arrival_timestamp: 100, chain_head_at_arrival: "chain".into(),
            catalog_hash_at_arrival: "catalog".into(), head_timestamp_at_arrival: 99,
            transport_scope: "dbus,grpc,mcp,mqtt".into(), nextdns: None,
        };
        let wire = claims.seal().unwrap();
        let inline = claims.to_inline_ref().unwrap();
        let schema = op_state::StatePlugin::schema(
            &op_plugins::state_plugins::identity_sled::IdentitySledPlugin::new(),
        ).unwrap();
        op_blob::BlobStore::open(&catalog).unwrap()
            .write(&op_blob::blobify_plugin_schema("identity_sled", schema)).unwrap();
        let private = serde_json::json!({"sleds":[{
            "session_id":claims.session_id,"wireguard_pubkey":claims.wireguard_pubkey,
            "principal_kind":"human","genesis":claims.session_genesis,"trace_id":claims.trace_id,
            "schema_version":claims.schema_version,"expires_at":claims.expires_at,
            "arrival_timestamp":claims.arrival_timestamp,"chain_head_at_arrival":claims.chain_head_at_arrival,
            "catalog_hash_at_arrival":claims.catalog_hash_at_arrival,
            "head_timestamp_at_arrival":claims.head_timestamp_at_arrival,"sealed_id":inline
        }],"events":[]});
        op_core::projection_shm::update_projection_with_credentials_in(
            state, "identity_sled", nix::unistd::getgid().as_raw(), |_| {
                Ok(Some((br#"{"sleds":[],"events":[]}"#.to_vec(), serde_json::to_vec(&private)?)))
            },
        ).unwrap();
        let identity = crate::identity_receiver::verify_registered_for_test(
            crate::identity_receiver::verify_in(&catalog, state, &wire, "grpc", 150).unwrap(),
            Some(op_cozo_store::HumanPrincipalRecord {
                principal_id: claims.principal_id, human_pubkey: claims.wireguard_pubkey,
                display_alias: String::new(), registered_at: 1, revoked_at: 0,
            }),
        ).unwrap();
        let mut caller = VerifiedCaller::new(&identity, Arc::new(RefusedAuthority));
        caller.sealed_identity = Some(inline);
        caller
    }

    #[tokio::test]
    async fn background_identity_retains_authority_but_not_the_execution_gate() {
        let admission = ExecutionAdmission::default();
        let mut caller = caller_fixture();
        caller.execution = Some(admission.admit().await.unwrap());
        caller.call_path.push(("notebooklm".into(), "notebook_query_start".into()));
        let background = caller.for_background();
        assert_eq!(background.claims(), caller.claims());
        assert_eq!(background.sealed_identity, caller.sealed_identity);
        assert!(Arc::ptr_eq(&background.authority, &caller.authority));
        assert!(background.execution.is_none());
        assert!(background.call_path.is_empty());
        drop(caller);
        assert!(admission.gate.try_lock().is_ok());
        assert!(background.authority.revalidate().await.is_err());
    }

    #[test]
    fn only_an_explicit_local_refusal_proves_no_effect() {
        let root = Uuid::new_v4();
        assert!(!RuntimeError::denied("denied").proves_before_effect(root, false));
        assert!(!RuntimeError::unsupported("unsupported").proves_before_effect(root, false));
        assert!(RuntimeError::before_effect(anyhow::anyhow!("native preflight"))
            .proves_before_effect(root, false));
        assert!(RuntimeError::unsupported_before_effect("no owner")
            .proves_before_effect(root, false));
        assert!(!RuntimeError::before_effect(anyhow::anyhow!("native preflight"))
            .proves_before_effect(root, true));
        assert!(!RuntimeError::before_effect(anyhow::anyhow!("native preflight"))
            .with_state(BackendStateUpdate::Unchanged, StateScope::Public)
            .proves_before_effect(root, false));
    }

    #[test]
    fn nested_refusals_keep_both_locators_and_never_prove_outer_no_effect() {
        let root = Uuid::new_v4();
        let child = Uuid::new_v4();
        let nested = RuntimeError::before_effect(anyhow::anyhow!("child refused"))
            .at_operation(child);
        assert!(!nested.proves_before_effect(root, false));
        let wrapped = RuntimeError::before_effect(RuntimeError::backend(anyhow::Error::new(nested)).into());
        assert!(!wrapped.proves_before_effect(root, false));
        let outer = wrapped.at_operation(root);
        assert_eq!(outer.operation_id.as_deref(), Some(root.to_string().as_str()));
        assert_eq!(outer.related_operation_ids, vec![child.to_string()]);
        assert!(!outer.proves_before_effect(root, false));
        let audited = RuntimeError::new(ErrorKind::FailedBeforeEffect, "method_failed_before_effect")
            .at_operation(root);
        assert!(audited.failed_before_effect());
        assert!(!audited.proves_before_effect(root, false), "a terminal cannot be reused as native proof");
    }

    #[tokio::test]
    async fn a_successful_nested_dispatch_prevents_an_outer_no_effect_proof() {
        let admission = ExecutionAdmission::default();
        let execution = admission.admit().await.unwrap();
        let root = Uuid::new_v4();
        let child = Uuid::new_v4();
        let root_index = execution.dispatch(root, false);
        let child_index = execution.dispatch(child, true);
        assert!(execution.nested_after(child_index).is_empty());
        let nested = execution.nested_after(root_index);
        assert_eq!(nested, vec![child]);
        let refusal = RuntimeError::before_effect(anyhow::anyhow!("outer refused after child"));
        assert!(!refusal.proves_before_effect(root, !nested.is_empty()));
        let unknown = RuntimeError::unknown(refusal.with_nested_operations(nested).into()).at_operation(root);
        assert!(unknown.outcome_unknown());
        assert_eq!(unknown.related_operation_ids, vec![child.to_string()]);
    }

    #[tokio::test]
    async fn shutdown_rejects_queued_roots_without_cancelling_the_admitted_owner() {
        let admission = ExecutionAdmission::default();
        let active = admission.admit().await.unwrap();
        let waiting = admission.admit();
        tokio::pin!(waiting);
        std::future::poll_fn(|context| {
            assert!(waiting.as_mut().poll(context).is_pending());
            std::task::Poll::Ready(())
        }).await;
        admission.begin_shutdown();
        assert!(admission.admit().await.is_err());
        assert!(admission.gate.try_lock().is_err());
        drop(active);
        assert!(waiting.await.is_err());
        admission.drain().await;
    }
}
