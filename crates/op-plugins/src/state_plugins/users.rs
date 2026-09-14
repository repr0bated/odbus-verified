//! User-account operations backed by AccountsService on the system D-Bus.

use anyhow::{Context, Result};
use async_trait::async_trait;
use op_state::{ApplyResult, Checkpoint, DiffMetadata, PluginCapabilities, StateDiff, StatePlugin};
use op_state_store::{CapabilityDecl, PluginSchema, SideEffect};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use simd_json::OwnedValue as Value;
use std::time::Duration;
use zbus::zvariant::OwnedObjectPath;

const DESTINATION: &str = "org.freedesktop.Accounts";
const MANAGER_PATH: &str = "/org/freedesktop/Accounts";
const MANAGER_INTERFACE: &str = "org.freedesktop.Accounts";
const USER_INTERFACE: &str = "org.freedesktop.Accounts.User";
const PROVIDER_DEADLINE: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(extend("x-oscal-category" = "service"))]
pub struct UsersState {
    pub provider: String,
    pub list_scope: String,
}
impl Default for UsersState {
    fn default() -> Self {
        Self {
            provider: "accountsservice".into(),
            list_scope: "cached_accounts".into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AccountType {
    Standard,
    Administrator,
}
impl AccountType {
    fn code(self) -> i32 {
        match self {
            Self::Standard => 0,
            Self::Administrator => 1,
        }
    }
    fn from_code(code: i32) -> Result<Self> {
        match code {
            0 => Ok(Self::Standard),
            1 => Ok(Self::Administrator),
            _ => anyhow::bail!("AccountsService returned unknown account type {code}"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateUserInput {
    pub username: String,
    pub real_name: String,
    pub account_type: AccountType,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DeleteUserInput {
    pub username: String,
    #[serde(default)]
    pub remove_files: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ModifyUserInput {
    pub username: String,
    #[serde(default)]
    pub real_name: Option<String>,
    #[serde(default)]
    pub shell: Option<String>,
    #[serde(default)]
    pub account_type: Option<AccountType>,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListUsersInput {
    #[serde(default)]
    pub include_system_accounts: bool,
    #[serde(default)]
    pub account_type: Option<AccountType>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UserRecord {
    pub uid: u64,
    pub username: String,
    pub real_name: String,
    pub account_type: AccountType,
    pub shell: String,
    pub system_account: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UserMutationOutput {
    pub operation: String,
    pub changed: bool,
    pub user: UserRecord,
    pub changed_fields: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DeleteUserOutput {
    pub username: String,
    pub uid: Option<u64>,
    pub deleted: bool,
    pub remove_files_requested: bool,
    pub provider_completed: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListUsersOutput {
    pub scope: String,
    pub exhaustive: bool,
    pub users: Vec<UserRecord>,
    pub total: u64,
}

fn validate_username(value: &str) -> Result<()> {
    anyhow::ensure!(
        !value.is_empty() && !value.contains('\0'),
        "username must be nonempty and contain no NUL"
    );
    Ok(())
}
fn validate_text(value: &str, field: &str, allow_empty: bool) -> Result<()> {
    anyhow::ensure!(
        (allow_empty || !value.is_empty()) && !value.contains('\0'),
        "{field} is invalid"
    );
    Ok(())
}
fn is_missing(error: &zbus::Error) -> bool {
    matches!(error, zbus::Error::MethodError(name, _, _) if name.as_str() == "org.freedesktop.Accounts.Error.UserDoesNotExist")
}

#[async_trait]
trait AccountsProvider: Send + Sync {
    async fn list_cached(&self) -> Result<Vec<String>>;
    async fn find(&self, username: &str) -> Result<Option<String>>;
    async fn read(&self, path: &str) -> Result<UserRecord>;
    async fn create(
        &self,
        username: &str,
        real_name: &str,
        account_type: AccountType,
    ) -> Result<String>;
    async fn delete(&self, uid: i64, remove_files: bool) -> Result<()>;
    async fn set_real_name(&self, path: &str, value: &str) -> Result<()>;
    async fn set_shell(&self, path: &str, value: &str) -> Result<()>;
    async fn set_account_type(&self, path: &str, value: AccountType) -> Result<()>;
}

struct DbusAccountsProvider {
    connection: zbus::Connection,
}
impl DbusAccountsProvider {
    async fn manager(&self) -> Result<zbus::Proxy<'_>> {
        zbus::Proxy::new(
            &self.connection,
            DESTINATION,
            MANAGER_PATH,
            MANAGER_INTERFACE,
        )
        .await
        .context("create AccountsService manager proxy")
    }
    async fn user<'a>(&'a self, path: &'a str) -> Result<zbus::Proxy<'a>> {
        zbus::Proxy::new(&self.connection, DESTINATION, path, USER_INTERFACE)
            .await
            .context("create AccountsService user proxy")
    }
}

#[async_trait]
impl AccountsProvider for DbusAccountsProvider {
    async fn list_cached(&self) -> Result<Vec<String>> {
        let paths: Vec<OwnedObjectPath> = self
            .manager()
            .await?
            .call("ListCachedUsers", &())
            .await
            .context("AccountsService ListCachedUsers")?;
        Ok(paths.into_iter().map(|path| path.to_string()).collect())
    }
    async fn find(&self, username: &str) -> Result<Option<String>> {
        match self
            .manager()
            .await?
            .call::<_, _, OwnedObjectPath>("FindUserByName", &(username,))
            .await
        {
            Ok(path) => Ok(Some(path.to_string())),
            Err(error) if is_missing(&error) => Ok(None),
            Err(error) => Err(error).context("AccountsService FindUserByName"),
        }
    }
    async fn read(&self, path: &str) -> Result<UserRecord> {
        let proxy = self.user(path).await?;
        Ok(UserRecord {
            uid: proxy
                .get_property("Uid")
                .await
                .context("read AccountsService Uid")?,
            username: proxy
                .get_property("UserName")
                .await
                .context("read AccountsService UserName")?,
            real_name: proxy
                .get_property("RealName")
                .await
                .context("read AccountsService RealName")?,
            account_type: AccountType::from_code(
                proxy
                    .get_property("AccountType")
                    .await
                    .context("read AccountsService AccountType")?,
            )?,
            shell: proxy
                .get_property("Shell")
                .await
                .context("read AccountsService Shell")?,
            system_account: proxy
                .get_property("SystemAccount")
                .await
                .context("read AccountsService SystemAccount")?,
        })
    }
    async fn create(
        &self,
        username: &str,
        real_name: &str,
        account_type: AccountType,
    ) -> Result<String> {
        let path: OwnedObjectPath = self
            .manager()
            .await?
            .call("CreateUser", &(username, real_name, account_type.code()))
            .await
            .context("AccountsService CreateUser")?;
        Ok(path.to_string())
    }
    async fn delete(&self, uid: i64, remove_files: bool) -> Result<()> {
        self.manager()
            .await?
            .call::<_, _, ()>("DeleteUser", &(uid, remove_files))
            .await
            .context("AccountsService DeleteUser")
    }
    async fn set_real_name(&self, path: &str, value: &str) -> Result<()> {
        self.user(path)
            .await?
            .call("SetRealName", &(value,))
            .await
            .context("AccountsService SetRealName")
    }
    async fn set_shell(&self, path: &str, value: &str) -> Result<()> {
        self.user(path)
            .await?
            .call("SetShell", &(value,))
            .await
            .context("AccountsService SetShell")
    }
    async fn set_account_type(&self, path: &str, value: AccountType) -> Result<()> {
        self.user(path)
            .await?
            .call("SetAccountType", &(value.code(),))
            .await
            .context("AccountsService SetAccountType")
    }
}

async fn dispatch_with_provider(
    provider: &dyn AccountsProvider,
    method: &str,
    args: &serde_json::Value,
) -> Result<serde_json::Value> {
    match method {
        "create_user" => {
            let input: CreateUserInput =
                serde_json::from_value(args.clone()).context("invalid create_user arguments")?;
            validate_username(&input.username)?;
            validate_text(&input.real_name, "real_name", true)?;
            anyhow::ensure!(
                provider.find(&input.username).await?.is_none(),
                "user already exists"
            );
            let path = provider
                .create(&input.username, &input.real_name, input.account_type)
                .await?;
            let user = provider.read(&path).await.context(
                "user creation completed but readback failed; account state is indeterminate",
            )?;
            anyhow::ensure!(
                user.username == input.username
                    && user.real_name == input.real_name
                    && user.account_type == input.account_type,
                "user creation completed but readback does not match request"
            );
            Ok(serde_json::to_value(UserMutationOutput {
                operation: "created".into(),
                changed: true,
                user,
                changed_fields: vec!["username".into(), "real_name".into(), "account_type".into()],
            })?)
        }
        "delete_user" => {
            let input: DeleteUserInput =
                serde_json::from_value(args.clone()).context("invalid delete_user arguments")?;
            validate_username(&input.username)?;
            let Some(path) = provider.find(&input.username).await? else {
                return Ok(serde_json::to_value(DeleteUserOutput {
                    username: input.username,
                    uid: None,
                    deleted: false,
                    remove_files_requested: input.remove_files,
                    provider_completed: false,
                })?);
            };
            let user = provider.read(&path).await?;
            anyhow::ensure!(
                user.username == input.username,
                "AccountsService lookup returned foreign username '{}' for requested '{}'",
                user.username,
                input.username
            );
            let uid = i64::try_from(user.uid)
                .context("AccountsService UID exceeds signed D-Bus range")?;
            provider.delete(uid, input.remove_files).await?;
            anyhow::ensure!(provider.find(&input.username).await.context("user deletion completed but absence readback failed; deletion state is indeterminate")?.is_none(), "user deletion returned but account remains present");
            Ok(serde_json::to_value(DeleteUserOutput {
                username: input.username,
                uid: Some(user.uid),
                deleted: true,
                remove_files_requested: input.remove_files,
                provider_completed: true,
            })?)
        }
        "modify_user" => {
            let input: ModifyUserInput =
                serde_json::from_value(args.clone()).context("invalid modify_user arguments")?;
            validate_username(&input.username)?;
            if let Some(value) = &input.real_name {
                validate_text(value, "real_name", true)?;
            }
            if let Some(value) = &input.shell {
                validate_text(value, "shell", false)?;
                anyhow::ensure!(value.starts_with('/'), "shell must be an absolute path");
            }
            let path = provider
                .find(&input.username)
                .await?
                .context("user does not exist")?;
            let before = provider.read(&path).await?;
            anyhow::ensure!(
                before.username == input.username,
                "AccountsService lookup returned foreign username '{}' for requested '{}'",
                before.username,
                input.username
            );
            let mut changed_fields = Vec::new();
            if input
                .real_name
                .as_ref()
                .is_some_and(|value| value != &before.real_name)
            {
                provider
                    .set_real_name(&path, input.real_name.as_deref().unwrap())
                    .await?;
                changed_fields.push("real_name".into());
            }
            if input
                .shell
                .as_ref()
                .is_some_and(|value| value != &before.shell)
            {
                provider
                    .set_shell(&path, input.shell.as_deref().unwrap())
                    .await
                    .with_context(|| {
                        format!(
                            "modify_user failed after fields {:?} were applied",
                            changed_fields
                        )
                    })?;
                changed_fields.push("shell".into());
            }
            if input
                .account_type
                .is_some_and(|value| value != before.account_type)
            {
                provider
                    .set_account_type(&path, input.account_type.unwrap())
                    .await
                    .with_context(|| {
                        format!(
                            "modify_user failed after fields {:?} were applied",
                            changed_fields
                        )
                    })?;
                changed_fields.push("account_type".into());
            }
            let user = provider.read(&path).await.context(
                "user modification completed but readback failed; applied state is indeterminate",
            )?;
            anyhow::ensure!(
                user.uid == before.uid && user.username == before.username,
                "user identity changed during modification"
            );
            if let Some(value) = input.real_name {
                anyhow::ensure!(
                    user.real_name == value,
                    "real_name readback mismatch after modification"
                );
            }
            if let Some(value) = input.shell {
                anyhow::ensure!(
                    user.shell == value,
                    "shell readback mismatch after modification"
                );
            }
            if let Some(value) = input.account_type {
                anyhow::ensure!(
                    user.account_type == value,
                    "account_type readback mismatch after modification"
                );
            }
            Ok(serde_json::to_value(UserMutationOutput {
                operation: "modified".into(),
                changed: !changed_fields.is_empty(),
                user,
                changed_fields,
            })?)
        }
        "list_users" => {
            let input: ListUsersInput =
                serde_json::from_value(args.clone()).context("invalid list_users arguments")?;
            let mut users = Vec::new();
            for path in provider.list_cached().await? {
                let user = provider.read(&path).await?;
                if (!input.include_system_accounts && user.system_account)
                    || input
                        .account_type
                        .is_some_and(|kind| kind != user.account_type)
                {
                    continue;
                }
                users.push(user);
            }
            users.sort_by(|a, b| a.username.cmp(&b.username));
            Ok(serde_json::to_value(ListUsersOutput {
                scope: "cached_accounts".into(),
                exhaustive: false,
                total: u64::try_from(users.len())?,
                users,
            })?)
        }
        other => anyhow::bail!("unknown users method '{other}'"),
    }
}

pub async fn dispatch_users_method(
    method: &str,
    args: &serde_json::Value,
) -> Result<serde_json::Value> {
    let connection = zbus::Connection::system()
        .await
        .context("connect to AccountsService system bus")?;
    tokio::time::timeout(
        PROVIDER_DEADLINE,
        dispatch_with_provider(&DbusAccountsProvider { connection }, method, args),
    )
    .await
    .context("AccountsService operation timeout")?
}

pub(crate) fn users_schema() -> PluginSchema {
    let root = serde_json::to_value(schemars::schema_for!(UsersState)).unwrap();
    let mut schema = super::schemars_adapter::plugin_schema_from_json(
        "users",
        "2.0.0",
        "AccountsService cached-account management",
        &root,
    );
    macro_rules! method {
        ($name:literal,$input:ty,$output:ty,$effect:expr,$idem:expr,$cap:literal,$subid:literal) => {{
            schema.methods.insert(
                $name.into(),
                super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
                    $input,
                    $output,
                >($name, $effect, $idem, $cap, $subid),
            );
        }};
    }
    method!(
        "create_user",
        CreateUserInput,
        UserMutationOutput,
        SideEffect::Mutation,
        false,
        "users.write",
        "mut.software.plugin.users.create@v2"
    );
    method!(
        "delete_user",
        DeleteUserInput,
        DeleteUserOutput,
        SideEffect::Mutation,
        false,
        "users.write",
        "mut.software.plugin.users.delete@v2"
    );
    method!(
        "modify_user",
        ModifyUserInput,
        UserMutationOutput,
        SideEffect::Mutation,
        false,
        "users.write",
        "mut.software.plugin.users.modify@v2"
    );
    method!(
        "list_users",
        ListUsersInput,
        ListUsersOutput,
        SideEffect::Read,
        true,
        "users.read",
        "obs.software.plugin.users.list@v2"
    );
    schema.capabilities.insert(
        "users.write".into(),
        CapabilityDecl {
            id: "users.write".into(),
            description: "Grants: create_user, delete_user, modify_user.".into(),
        },
    );
    schema.capabilities.insert(
        "users.read".into(),
        CapabilityDecl {
            id: "users.read".into(),
            description: "Grants: list_users.".into(),
        },
    );
    schema.example =
        Some(simd_json::json!({"provider":"accountsservice","list_scope":"cached_accounts"}));
    schema
}

pub struct UsersPlugin;
impl Default for UsersPlugin {
    fn default() -> Self {
        Self
    }
}
impl UsersPlugin {
    pub fn new() -> Self {
        Self
    }
}
#[async_trait]
impl StatePlugin for UsersPlugin {
    fn name(&self) -> &str {
        "users"
    }
    fn version(&self) -> &str {
        "2.0.0"
    }
    fn schema(&self) -> Option<PluginSchema> {
        Some(users_schema())
    }
    async fn calculate_diff(&self, _: &Value, _: &Value) -> Result<StateDiff> {
        Ok(StateDiff {
            plugin: self.name().into(),
            actions: vec![],
            metadata: DiffMetadata {
                timestamp: chrono::Utc::now().timestamp(),
                current_hash: String::new(),
                desired_hash: String::new(),
            },
        })
    }
    async fn apply_state(&self, _: &StateDiff) -> Result<ApplyResult> {
        Ok(ApplyResult {
            success: true,
            changes_applied: vec![],
            errors: vec![],
            checkpoint: None,
        })
    }
    async fn verify_state(&self, _: &Value) -> Result<bool> {
        Ok(true)
    }
    async fn create_checkpoint(&self) -> Result<Checkpoint> {
        Ok(Checkpoint {
            id: uuid::Uuid::new_v4().to_string(),
            plugin: self.name().into(),
            timestamp: chrono::Utc::now().timestamp(),
            state_snapshot: simd_json::serde::to_owned_value(UsersState::default())?,
            backend_checkpoint: None,
        })
    }
    async fn rollback(&self, _: &Checkpoint) -> Result<()> {
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
inventory::submit! {crate::default_registry::PluginReg::new("users",|_ctx|std::sync::Arc::new(UsersPlugin::new()))}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use tokio::time::Duration;
    use zbus::{connection::Builder, Guid};

    const USER_PATH: &str = "/org/freedesktop/Accounts/User1000";

    #[derive(Debug, zbus::DBusError)]
    #[zbus(prefix = "org.freedesktop.Accounts.Error")]
    enum FixtureError {
        UserDoesNotExist(String),
        Failed(String),
    }

    #[derive(Clone)]
    struct FixtureState {
        user: Arc<Mutex<Option<UserRecord>>>,
        calls: Arc<Mutex<Vec<String>>>,
        fail_shell: bool,
    }
    #[derive(Clone)]
    struct ManagerFixture(FixtureState);
    #[derive(Clone)]
    struct UserFixture(FixtureState);

    #[zbus::interface(name = "org.freedesktop.Accounts")]
    impl ManagerFixture {
        async fn list_cached_users(&self) -> Vec<OwnedObjectPath> {
            if self.0.user.lock().unwrap().is_some() {
                vec![OwnedObjectPath::try_from(USER_PATH).unwrap()]
            } else {
                vec![]
            }
        }
        async fn find_user_by_name(
            &self,
            name: &str,
        ) -> std::result::Result<OwnedObjectPath, FixtureError> {
            if name == "provider-failure" {
                return Err(FixtureError::Failed("provider unavailable".into()));
            }
            match self.0.user.lock().unwrap().as_ref() {
                Some(user) if user.username == name => {
                    Ok(OwnedObjectPath::try_from(USER_PATH).unwrap())
                }
                _ => Err(FixtureError::UserDoesNotExist(name.into())),
            }
        }
        async fn create_user(
            &self,
            name: &str,
            real_name: &str,
            account_type: i32,
        ) -> std::result::Result<OwnedObjectPath, FixtureError> {
            self.0.calls.lock().unwrap().push("CreateUser".into());
            let account_type = AccountType::from_code(account_type)
                .map_err(|error| FixtureError::Failed(error.to_string()))?;
            *self.0.user.lock().unwrap() = Some(UserRecord {
                uid: 1000,
                username: name.into(),
                real_name: real_name.into(),
                account_type,
                shell: "/bin/sh".into(),
                system_account: false,
            });
            Ok(OwnedObjectPath::try_from(USER_PATH).unwrap())
        }
        async fn delete_user(
            &self,
            uid: i64,
            _remove_files: bool,
        ) -> std::result::Result<(), FixtureError> {
            self.0.calls.lock().unwrap().push("DeleteUser".into());
            if uid != 1000 {
                return Err(FixtureError::UserDoesNotExist(uid.to_string()));
            }
            *self.0.user.lock().unwrap() = None;
            Ok(())
        }
    }

    #[zbus::interface(name = "org.freedesktop.Accounts.User")]
    impl UserFixture {
        async fn set_real_name(&self, value: &str) {
            self.0.calls.lock().unwrap().push("SetRealName".into());
            self.0.user.lock().unwrap().as_mut().unwrap().real_name = value.into();
        }
        async fn set_shell(&self, value: &str) -> std::result::Result<(), FixtureError> {
            self.0.calls.lock().unwrap().push("SetShell".into());
            if self.0.fail_shell {
                return Err(FixtureError::Failed("shell rejected".into()));
            }
            self.0.user.lock().unwrap().as_mut().unwrap().shell = value.into();
            Ok(())
        }
        async fn set_account_type(&self, value: i32) -> std::result::Result<(), FixtureError> {
            self.0.calls.lock().unwrap().push("SetAccountType".into());
            self.0.user.lock().unwrap().as_mut().unwrap().account_type =
                AccountType::from_code(value)
                    .map_err(|error| FixtureError::Failed(error.to_string()))?;
            Ok(())
        }
        #[zbus(property)]
        async fn uid(&self) -> u64 {
            self.0.user.lock().unwrap().as_ref().unwrap().uid
        }
        #[zbus(property)]
        async fn user_name(&self) -> String {
            self.0
                .user
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .username
                .clone()
        }
        #[zbus(property)]
        async fn real_name(&self) -> String {
            self.0
                .user
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .real_name
                .clone()
        }
        #[zbus(property)]
        async fn account_type(&self) -> i32 {
            self.0
                .user
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .account_type
                .code()
        }
        #[zbus(property)]
        async fn shell(&self) -> String {
            self.0.user.lock().unwrap().as_ref().unwrap().shell.clone()
        }
        #[zbus(property)]
        async fn system_account(&self) -> bool {
            self.0.user.lock().unwrap().as_ref().unwrap().system_account
        }
    }

    async fn fixture(
        user: Option<UserRecord>,
        fail_shell: bool,
    ) -> (
        zbus::Connection,
        DbusAccountsProvider,
        Arc<Mutex<Vec<String>>>,
    ) {
        let (server_stream, client_stream) = tokio::net::UnixStream::pair().unwrap();
        let calls = Arc::new(Mutex::new(Vec::new()));
        let state = FixtureState {
            user: Arc::new(Mutex::new(user)),
            calls: calls.clone(),
            fail_shell,
        };
        let server = Builder::unix_stream(server_stream)
            .server(Guid::generate())
            .unwrap()
            .p2p()
            .serve_at(MANAGER_PATH, ManagerFixture(state.clone()))
            .unwrap()
            .serve_at(USER_PATH, UserFixture(state))
            .unwrap()
            .build();
        let client = Builder::unix_stream(client_stream).p2p().build();
        let (server, client) = tokio::time::timeout(Duration::from_secs(3), async {
            tokio::try_join!(server, client)
        })
        .await
        .unwrap()
        .unwrap();
        (server, DbusAccountsProvider { connection: client }, calls)
    }
    fn user() -> UserRecord {
        UserRecord {
            uid: 1000,
            username: "alice".into(),
            real_name: "Alice".into(),
            account_type: AccountType::Standard,
            shell: "/bin/sh".into(),
            system_account: false,
        }
    }

    #[tokio::test]
    async fn all_four_methods_use_accountsservice_and_typed_readback() {
        let (_server, provider, calls) = fixture(None, false).await;
        let created = dispatch_with_provider(
            &provider,
            "create_user",
            &serde_json::json!({"username":"alice","real_name":"Alice","account_type":"standard"}),
        )
        .await
        .unwrap();
        assert_eq!(created["user"]["uid"], 1000);
        let modified=dispatch_with_provider(&provider,"modify_user",&serde_json::json!({"username":"alice","real_name":"Alice A","shell":"/bin/bash","account_type":"administrator"})).await.unwrap();
        assert_eq!(modified["user"]["shell"], "/bin/bash");
        let listed = dispatch_with_provider(&provider, "list_users", &serde_json::json!({}))
            .await
            .unwrap();
        assert_eq!(listed["scope"], "cached_accounts");
        assert_eq!(listed["exhaustive"], false);
        assert_eq!(listed["total"], 1);
        let deleted = dispatch_with_provider(
            &provider,
            "delete_user",
            &serde_json::json!({"username":"alice","remove_files":true}),
        )
        .await
        .unwrap();
        assert_eq!(deleted["deleted"], true);
        assert_eq!(
            *calls.lock().unwrap(),
            vec![
                "CreateUser",
                "SetRealName",
                "SetShell",
                "SetAccountType",
                "DeleteUser"
            ]
        );
    }

    #[tokio::test]
    async fn modify_noop_and_partial_failure_are_honest() {
        let (_server, provider, _) = fixture(Some(user()), false).await;
        let output = dispatch_with_provider(
            &provider,
            "modify_user",
            &serde_json::json!({"username":"alice","real_name":"Alice"}),
        )
        .await
        .unwrap();
        assert_eq!(output["changed"], false);

        let (_server, provider, calls) = fixture(Some(user()), true).await;
        let error = dispatch_with_provider(
            &provider,
            "modify_user",
            &serde_json::json!({"username":"alice","real_name":"Applied","shell":"/bad"}),
        )
        .await
        .unwrap_err();
        assert!(error.to_string().contains("after fields"));
        assert_eq!(*calls.lock().unwrap(), vec!["SetRealName", "SetShell"]);
    }

    #[tokio::test]
    async fn unsupported_old_fields_and_missing_users_are_distinct() {
        let (_server, provider, calls) = fixture(None, false).await;
        assert!(dispatch_with_provider(&provider,"create_user",&serde_json::json!({"username":"alice","real_name":"Alice","account_type":"standard","uid":1000})).await.is_err());
        assert!(calls.lock().unwrap().is_empty());
        let deleted = dispatch_with_provider(
            &provider,
            "delete_user",
            &serde_json::json!({"username":"absent"}),
        )
        .await
        .unwrap();
        assert_eq!(deleted["deleted"], false);

        let error = dispatch_with_provider(
            &provider,
            "delete_user",
            &serde_json::json!({"username":"provider-failure"}),
        )
        .await
        .unwrap_err();
        assert!(error.to_string().contains("FindUserByName"));
    }

    #[test]
    fn schema_is_typed_and_preserves_capability_ids() {
        let schema = users_schema();
        assert_eq!(schema.methods.len(), 4);
        assert!(schema.capabilities.contains_key("users.write"));
        assert!(schema.capabilities.contains_key("users.read"));
        let encoded = serde_json::to_string(&schema).unwrap();
        assert!(!encoded.contains("AckOutput"));
        assert!(!encoded.contains("groups"));
    }
}
